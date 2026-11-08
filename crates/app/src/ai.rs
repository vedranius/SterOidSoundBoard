//! AI opinion on a voice recording (DigiLingua). Providers are the user's
//! choice with the user's own key: Anthropic Claude, any OpenAI-compatible
//! API, or a local Ollama server (no data leaves the machine).
//!
//! The AI never receives audio: it gets the acoustic measurements, a
//! pseudonymised patient profile (no name, code or birth date), the
//! clinician's notes and, optionally, the sonagram image.
use crate::clinic::{AiReport, Recording};
use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};


pub const DEFAULT_CLAUDE_MODEL: &str = "claude-opus-5";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Server-side refusal fallback ("default" = Anthropic picks the fallback model).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_TOKENS: u32 = 16_000;
const MAX_IMAGE_BYTES: usize = 5 << 20;

/// Receives what an AI request is doing: log lines, streamed text, numbers.
/// Called from the worker thread (and Ollama's load watcher), hence `Sync`.
pub trait Progress: Sync {
    fn log(&self, msg: &str);
    fn delta(&self, _text: &str) {}
    fn stats(&self, _v: Value) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// No progress reporting (connection test).
pub struct Silent;
impl Progress for Silent {
    fn log(&self, _msg: &str) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    /// "anthropic", "openai" (any OpenAI-compatible server) or "ollama".
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    /// Attach the sonagram image (needs a vision-capable model).
    pub send_image: bool,
    /// Ollama context size in tokens; 0 = automatic (prompt + answer).
    pub ollama_ctx: u32,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            provider: "anthropic".into(),
            base_url: String::new(),
            model: DEFAULT_CLAUDE_MODEL.into(),
            api_key: String::new(),
            send_image: true,
            ollama_ctx: 0,
        }
    }
}

impl AiConfig {
    pub fn load(path: &Path) -> AiConfig {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Written owner-readable only on Unix: the file holds the API key.
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn base(&self) -> String {
        if self.provider == "ollama" {
            return crate::ollama::root(&self.base_url);
        }
        let b = self.base_url.trim().trim_end_matches('/');
        if !b.is_empty() {
            return b.to_string();
        }
        match self.provider.as_str() {
            "openai" => "https://api.openai.com/v1".into(),
            "ollama" => crate::ollama::DEFAULT_URL.into(),
            _ => "https://api.anthropic.com".into(),
        }
    }

    pub fn provider_name(&self) -> &'static str {
        match self.provider.as_str() {
            "openai" => "OpenAI-kompatibilan",
            "ollama" => "Ollama (lokalno)",
            _ => "Anthropic Claude",
        }
    }

    /// What the UI may see: never the key itself.
    pub fn public(&self) -> Value {
        json!({
            "provider": self.provider,
            "base_url": self.base_url,
            "effective_url": self.base(),
            "model": self.model,
            "has_key": !self.api_key.is_empty(),
            "send_image": self.send_image,
            "ollama_ctx": self.ollama_ctx,
            "default_claude_model": DEFAULT_CLAUDE_MODEL,
            "default_ollama_model": crate::ollama::DEFAULT_MODEL,
        })
    }

    pub fn check(&self) -> Result<()> {
        if !matches!(self.provider.as_str(), "anthropic" | "openai" | "ollama") {
            bail!(tr!("nepoznat AI servis", "unknown AI service"));
        }
        if self.model.trim().is_empty() {
            bail!(tr!("AI model nije postavljen (AI postavke)", "no AI model set (AI settings)"));
        }
        if self.provider == "anthropic" && self.api_key.is_empty() {
            bail!(tr!("nedostaje Anthropic API ključ (AI postavke)", "the Anthropic API key is missing (AI settings)"));
        }
        Ok(())
    }
}

/// Validated image for the request: (media type, base64 data).
pub fn parse_image(data_url: &str) -> Result<(String, String)> {
    let (mime, b64) = match data_url.strip_prefix("data:") {
        Some(rest) => {
            let (head, data) = rest.split_once(',').ok_or_else(|| anyhow!(tr!("neispravna slika", "invalid image")))?;
            (head.trim_end_matches(";base64").to_string(), data.to_string())
        }
        None => ("image/jpeg".to_string(), data_url.to_string()),
    };
    if mime != "image/jpeg" && mime != "image/png" {
        bail!(tr!("slika mora biti JPEG ili PNG", "the image must be JPEG or PNG"));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| anyhow!(tr!("neispravna slika (base64)", "invalid image (base64)")))?;
    if bytes.len() > MAX_IMAGE_BYTES {
        bail!(tr!("slika je prevelika", "the image is too large"));
    }
    Ok((mime, b64.trim().to_string()))
}

pub use steroid_clinic::prompt::*;

#[derive(Debug)]
pub struct Answer {
    pub text: String,
    pub model: String,
    pub truncated: bool,
}

fn uses_fallbacks(model: &str) -> bool {
    model.starts_with("claude-opus-5") || model.starts_with("claude-fable-5")
}

pub fn anthropic_body(cfg: &AiConfig, system: &str, user: &str, image: Option<&(String, String)>, max_tokens: u32, fallbacks: bool) -> Value {
    let mut content = vec![];
    if let Some((mime, data)) = image {
        content.push(json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}}));
    }
    content.push(json!({"type": "text", "text": user}));
    let mut body = json!({
        "model": cfg.model.trim(),
        "max_tokens": max_tokens,
        "system": system,
        "messages": [{"role": "user", "content": content}],
    });
    if fallbacks {
        body["fallbacks"] = json!("default");
    }
    body
}

pub fn openai_body(cfg: &AiConfig, system: &str, user: &str, image: Option<&(String, String)>) -> Value {
    let user_content = match image {
        Some((mime, data)) => json!([
            {"type": "text", "text": user},
            {"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{data}")}},
        ]),
        None => json!(user),
    };
    json!({
        "model": cfg.model.trim(),
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user_content}],
    })
}

pub fn parse_anthropic(v: &Value) -> Result<Answer> {
    match v["stop_reason"].as_str() {
        Some("refusal") => {
            let cat = v["stop_details"]["category"].as_str().unwrap_or("nepoznato");
            bail!(tr!("AI model je odbio zahtjev (kategorija: {cat}). Pokušajte preformulirati pitanje.", "The AI model declined the request (category: {cat}). Try rephrasing the question."));
        }
        _ => {}
    }
    let text: Vec<&str> = v["content"]
        .as_array()
        .map(|a| a.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect())
        .unwrap_or_default();
    if text.is_empty() {
        bail!(tr!("AI nije vratio tekst (stop_reason: {})", "the AI returned no text (stop_reason: {})", v["stop_reason"]));
    }
    Ok(Answer {
        text: text.join("\n\n"),
        model: v["model"].as_str().unwrap_or_default().to_string(),
        truncated: v["stop_reason"] == "max_tokens",
    })
}

pub fn parse_openai(v: &Value) -> Result<Answer> {
    let ch = &v["choices"][0];
    let msg = &ch["message"];
    let text = match &msg["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    };
    if text.trim().is_empty() {
        if let Some(r) = msg["refusal"].as_str() {
            bail!(tr!("AI model je odbio zahtjev: {r}", "The AI model declined the request: {r}"));
        }
        bail!(tr!("AI nije vratio tekst", "the AI returned no text"));
    }
    Ok(Answer { text, model: v["model"].as_str().unwrap_or_default().to_string(), truncated: ch["finish_reason"] == "length" })
}

/// Host part of an http(s) URL, lower-case, without port or brackets.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|x| x.1).unwrap_or(url);
    let hostport = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = hostport.rsplit_once('@').map(|x| x.1).unwrap_or(hostport);
    let host = if let Some(h) = hostport.strip_prefix('[') { h.split(']').next().unwrap_or("") } else { hostport.split(':').next().unwrap_or("") };
    host.to_ascii_lowercase()
}

/// Local and LAN services (Ollama, LM Studio) are reached directly, never
/// through an HTTP(S)_PROXY from the environment; NO_PROXY is honoured too.
fn bypass_proxy(url: &str) -> bool {
    let no_proxy = std::env::var("NO_PROXY").or_else(|_| std::env::var("no_proxy")).unwrap_or_default();
    bypass_proxy_with(url, &no_proxy)
}

fn bypass_proxy_with(url: &str, no_proxy: &str) -> bool {
    let h = host_of(url);
    if h == "localhost" || h.ends_with(".local") || h.ends_with(".localhost") {
        return true;
    }
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local(),
            std::net::IpAddr::V6(v) => v.is_loopback() || (v.segments()[0] & 0xfe00) == 0xfc00,
        };
    }
    no_proxy.split(',').map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase()).any(|e| {
        !e.is_empty() && (e == "*" || h == e || h.ends_with(&format!(".{e}")))
    })
}

/// HTTP agent; `read` bounds each socket read (for a non-streamed answer:
/// the whole wait; when streaming: the pause between two pieces).
pub fn agent_with(url: &str, read: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(read)
        .timeout_write(Duration::from_secs(120))
        .try_proxy_from_env(!bypass_proxy(url))
        .build()
}

fn agent(url: &str) -> ureq::Agent {
    // non-streaming: a long, carefully reasoned answer can take minutes
    agent_with(url, Duration::from_secs(900))
}

/// Human-readable error from a provider's JSON error body.
fn error_message(code: u16, body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = v["error"]["message"].as_str().or_else(|| v["error"].as_str()).or_else(|| v["message"].as_str()).unwrap_or(body).chars().take(400).collect::<String>();
    let hint = match code {
        401 | 403 => tr!(" — provjerite API ključ", " — check the API key"),
        404 => tr!(" — provjerite model i adresu servisa", " — check the model and the service address"),
        429 => tr!(" — previše zahtjeva ili nema kredita", " — too many requests or no credit"),
        _ => String::new(),
    };
    tr!("AI servis je vratio grešku {code}: {msg}{hint}", "The AI service returned error {code}: {msg}{hint}")
}

/// POST JSON with up to two retries on 408/429/5xx and connection errors.
fn post(url: &str, headers: &[(&str, &str)], body: &Value, p: &dyn Progress) -> std::result::Result<Value, (u16, String)> {
    let ag = agent(url);
    let mut attempt = 0;
    loop {
        let mut req = ag.post(url).set("content-type", "application/json");
        for (k, v) in headers {
            req = req.set(k, v);
        }
        let retry_in = match req.send_json(body.clone()) {
            Ok(resp) => {
                return resp.into_json::<Value>().map_err(|e| (0, tr!("neispravan odgovor AI servisa: {e}", "invalid answer from the AI service: {e}")));
            }
            Err(ureq::Error::Status(code, resp)) => {
                let wait = resp.header("retry-after").and_then(|h| h.parse::<u64>().ok());
                let text = resp.into_string().unwrap_or_default();
                if !(code == 408 || code == 429 || code >= 500) || attempt >= 2 {
                    return Err((code, error_message(code, &text)));
                }
                let d = Duration::from_secs(wait.unwrap_or(2 << attempt).min(30));
                p.log(&tr!("Servis je zauzet ili preopterećen ({code}) — ponovni pokušaj za {} s.", "The service is busy or overloaded ({code}) — retrying in {} s.", d.as_secs()));
                d
            }
            Err(ureq::Error::Transport(t)) => {
                if attempt >= 1 {
                    return Err((0, tr!("nije moguće spojiti se na {url}: {t}", "cannot connect to {url}: {t}")));
                }
                p.log(&tr!("Veza nije uspjela ({t}) — ponovni pokušaj za 2 s.", "Connection failed ({t}) — retrying in 2 s."));
                Duration::from_secs(2)
            }
        };
        attempt += 1;
        if p.cancelled() {
            return Err((0, tr!("prekinuto", "stopped")));
        }
        std::thread::sleep(retry_in);
    }
}

/// OpenAI-compatible chat with server-sent events; servers that ignore
/// `stream` and answer in one piece are handled too.
fn openai_stream(url: &str, headers: &[(&str, &str)], body: &Value, p: &dyn Progress) -> Result<Answer> {
    let ag = agent_with(url, Duration::from_secs(20 * 60));
    let mut req = ag.post(url).set("content-type", "application/json").set("accept", "text/event-stream");
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let t0 = Instant::now();
    let resp = match req.send_json(body.clone()) {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => bail!(error_message(code, &r.into_string().unwrap_or_default())),
        Err(ureq::Error::Transport(t)) => bail!(tr!("nije moguće spojiti se na {url}: {t}", "cannot connect to {url}: {t}")),
    };
    if !resp.content_type().contains("event-stream") {
        let a = parse_openai(&resp.into_json::<Value>().map_err(|e| anyhow!(tr!("neispravan odgovor AI servisa: {e}", "invalid answer from the AI service: {e}")))?)?;
        p.log(&tr!("Odgovor primljen u cjelini nakon {:.0} s.", "Answer received in one piece after {:.0} s.", t0.elapsed().as_secs_f64()));
        p.delta(&a.text);
        return Ok(a);
    }
    let (mut text, mut model, mut finish, mut n, mut thinking) = (String::new(), String::new(), String::new(), 0u64, false);
    let mut gen_start: Option<Instant> = None;
    let mut last_stats = Instant::now();
    for line in BufReader::new(resp.into_reader()).lines() {
        if p.cancelled() {
            bail!(tr!("prekinuto", "stopped"));
        }
        let line = line.map_err(|e| anyhow!(tr!("veza s AI servisom je prekinuta: {e}", "connection to the AI service lost: {e}")))?;
        let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
        if data == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
        if !v["error"].is_null() {
            bail!(tr!("AI servis: {}", "AI service: {}", v["error"]["message"].as_str().or_else(|| v["error"].as_str()).unwrap_or("error")));
        }
        if model.is_empty() {
            model = v["model"].as_str().unwrap_or_default().to_string();
        }
        let ch = &v["choices"][0];
        let reasoning = ch["delta"]["reasoning_content"].as_str().or_else(|| ch["delta"]["reasoning"].as_str()).unwrap_or_default();
        if !reasoning.is_empty() && !thinking {
            thinking = true;
            p.log(&tr!("Model razmišlja prije odgovora (nakon {:.0} s)…", "The model is thinking before answering (after {:.0} s)…", t0.elapsed().as_secs_f64()));
        }
        if let Some(c) = ch["delta"]["content"].as_str().filter(|c| !c.is_empty()) {
            if gen_start.is_none() {
                gen_start = Some(Instant::now());
                p.log(&tr!("Prvi dio odgovora nakon {:.0} s — model piše…", "First words after {:.0} s — the model is writing…", t0.elapsed().as_secs_f64()));
            }
            n += 1;
            text.push_str(c);
            p.delta(c);
        }
        if let Some(f) = ch["finish_reason"].as_str() {
            finish = f.to_string();
        }
        if last_stats.elapsed() > Duration::from_millis(1000) {
            let g = gen_start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
            p.stats(json!({"tokens": n, "tps": if g > 0.5 { (n as f64 / g * 10.0).round() / 10.0 } else { 0.0 }, "elapsed": t0.elapsed().as_secs()}));
            last_stats = Instant::now();
        }
    }
    let g = gen_start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
    p.log(&tr!("Gotovo za {:.0} s: {n} dijelova odgovora{}.", "Done in {:.0} s: {n} answer pieces{}.", t0.elapsed().as_secs_f64(), if g > 0.5 { tr!(" ({:.1} po s)", " ({:.1} per s)", n as f64 / g) } else { String::new() }));
    p.stats(json!({"tokens": n, "tps": if g > 0.5 { (n as f64 / g * 10.0).round() / 10.0 } else { 0.0 }, "elapsed": t0.elapsed().as_secs(), "done": true}));
    let answer = crate::ollama::strip_thinking(&text);
    if answer.is_empty() {
        bail!(tr!("AI nije vratio tekst (razlog završetka: {})", "the AI returned no text (finish reason: {})", if finish.is_empty() { "?" } else { &finish }));
    }
    Ok(Answer { text: answer, model, truncated: finish == "length" })
}

pub fn ask(cfg: &AiConfig, system: &str, user: &str, image: Option<&(String, String)>, max_tokens: u32, p: &dyn Progress) -> Result<Answer> {
    cfg.check()?;
    let base = cfg.base();
    let size = tr!(
        "{} znakova{}",
        "{} characters{}",
        system.chars().count() + user.chars().count(),
        image.map(|(_, d)| tr!(" + slika {} kB", " + image {} kB", d.len() * 3 / 4 / 1024)).unwrap_or_default()
    );
    match cfg.provider.as_str() {
        "ollama" => crate::ollama::chat(&base, cfg.model.trim(), cfg.ollama_ctx, system, user, image, max_tokens as u64, p),
        "anthropic" => {
            let url = if base.ends_with("/v1") { format!("{base}/messages") } else { format!("{base}/v1/messages") };
            let key = cfg.api_key.trim();
            let mut fallbacks = uses_fallbacks(cfg.model.trim());
            p.log(&tr!("Šaljem upit ({size}) na {url}, model {}.", "Sending the prompt ({size}) to {url}, model {}.", cfg.model.trim()));
            p.log(&tr!("Claude šalje odgovor u cjelini — čekam (temeljita analiza obično traje 30 s do 2 min)…", "Claude sends the answer in one piece — waiting (a thorough analysis usually takes 30 s to 2 min)…"));
            let t0 = Instant::now();
            loop {
                let mut headers = vec![("x-api-key", key), ("anthropic-version", ANTHROPIC_VERSION)];
                if fallbacks {
                    headers.push(("anthropic-beta", FALLBACK_BETA));
                }
                match post(&url, &headers, &anthropic_body(cfg, system, user, image, max_tokens, fallbacks), p) {
                    Ok(v) => {
                        let a = parse_anthropic(&v)?;
                        let (i, o) = (v["usage"]["input_tokens"].as_u64().unwrap_or(0), v["usage"]["output_tokens"].as_u64().unwrap_or(0));
                        p.log(&tr!("Odgovor primljen nakon {:.0} s: model {}, upit {i} tokena, odgovor {o} tokena.", "Answer received after {:.0} s: model {}, prompt {i} tokens, answer {o} tokens.", t0.elapsed().as_secs_f64(), a.model));
                        p.stats(json!({"tokens": o, "prompt_tokens": i, "elapsed": t0.elapsed().as_secs(), "done": true}));
                        p.delta(&a.text);
                        return Ok(a);
                    }
                    // a gateway or older deployment may not know the fallback beta: retry without it
                    Err((400, m)) if fallbacks && m.to_lowercase().contains("fallback") => {
                        p.log(&tr!("Servis ne podržava zamjenski model (fallback) — ponavljam bez njega.", "The service does not support the fallback model — retrying without it."));
                        fallbacks = false
                    }
                    Err((_, m)) => bail!(m),
                }
            }
        }
        _ => {
            let url = format!("{base}/chat/completions");
            let auth = format!("Bearer {}", cfg.api_key.trim());
            let headers: Vec<(&str, &str)> = if cfg.api_key.trim().is_empty() { vec![] } else { vec![("authorization", auth.as_str())] };
            p.log(&tr!("Šaljem upit ({size}) na {url}, model {}.", "Sending the prompt ({size}) to {url}, model {}.", cfg.model.trim()));
            let mut body = openai_body(cfg, system, user, image);
            body["stream"] = json!(true);
            openai_stream(&url, &headers, &body, p)
        }
    }
}

pub fn ask_report(cfg: &AiConfig, system: &str, user: &str, image: Option<&(String, String)>, p: &dyn Progress) -> Result<Answer> {
    ask(cfg, system, user, image, MAX_TOKENS, p)
}

/// Small round-trip to verify provider, model and key.
pub fn test(cfg: &AiConfig) -> Result<Answer> {
    if crate::i18n::en() {
        ask(cfg, "Answer briefly.", "This is a connection test. Answer with the single word: OK", None, 1024, &Silent)
    } else {
        ask(cfg, "Odgovaraj kratko.", "Ovo je test veze. Odgovori samo riječju: OK", None, 1024, &Silent)
    }
}

pub fn new_report(rec: &Recording, cfg: &AiConfig, start: f32, end: f32, question: &str, a: Answer) -> AiReport {
    AiReport {
        id: crate::clinic::new_id("a"),
        recording_id: rec.id.clone(),
        patient_id: rec.patient_id.clone(),
        created: crate::clinic::now_ms(),
        provider: cfg.provider.clone(),
        model: if a.model.is_empty() { cfg.model.clone() } else { a.model },
        start,
        end,
        question: question.to_string(),
        text: a.text,
        truncated: a.truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clinic::PatientIn;
    use steroid_engine::analysis;

    fn rec() -> Recording {
        Recording {
            id: "r1".into(),
            patient_id: Some("p1".into()),
            session_id: None,
            label: "vokal".into(),
            created: 0,
            duration: 3.0,
            sample_rate: 48000,
            source: "in".into(),
            task: "Produženi vokal /a/".into(),
            notes: "tvrdi počeci".into(),
            annotations: vec![],
            syllables: None,
            analyses: vec![],
        }
    }

    #[test]
    fn prompt_is_pseudonymised_and_complete() {
        let p = PatientIn {
            name: "Ivan Horvat".into(),
            code: "MBO-123456".into(),
            birth: "1980-05-01".into(),
            sex: "M".into(),
            complaint: "promuklost 3 mjeseca".into(),
            smoking: "pušač".into(),
            ..Default::default()
        };
        let r = rec();
        let rep = analysis::analyze(&vec![0.0; 48000], 48000.0);
        let hist = vec![("2026-09-01".to_string(), "Produženi vokal /a/".to_string(), rep.clone())];
        let (sys, user) = build_prompt(&Context {
            patient: Some(&p),
            age: Some(46),
            rec: &r,
            start: 0.0,
            end: 1.0,
            report: &rep,
            history: &hist,
            question: "Je li HNR zabrinjavajući?",
            has_image: true,
            annotations: None,
            calibration_db: None,
            en: false,
        });
        for secret in ["Ivan", "Horvat", "MBO-123456", "1980-05-01"] {
            assert!(!user.contains(secret) && !sys.contains(secret), "leaked {secret}");
        }
        for want in ["46 god.", "Spol: M", "promuklost 3 mjeseca", "pušač", "Produženi vokal /a/", "tvrdi počeci", "HNR", "2026-09-01", "Je li HNR", "spektrogram"] {
            assert!(user.contains(want), "missing {want}\n{user}");
        }
        let (sys_en, user_en) = build_prompt(&Context {
            patient: Some(&p),
            age: Some(46),
            rec: &r,
            start: 0.0,
            end: 1.0,
            report: &rep,
            history: &hist,
            question: "Je li HNR zabrinjavajući?",
            has_image: true,
            annotations: None,
            calibration_db: None,
            en: true,
        });
        for secret in ["Ivan", "Horvat", "MBO-123456", "1980-05-01"] {
            assert!(!user_en.contains(secret) && !sys_en.contains(secret), "leaked {secret} (en)");
        }
        for want in ["46 years", "Sex: M", "HNR", "2026-09-01", "spectrogram", "pseudonymised"] {
            assert!(user_en.contains(want) || sys_en.contains(want), "missing {want} (en)\n{user_en}");
        }
    }

    #[test]
    fn anthropic_request_shape() {
        let cfg = AiConfig { api_key: "k".into(), ..Default::default() };
        let img = ("image/jpeg".to_string(), "AAAA".to_string());
        let b = anthropic_body(&cfg, "sys", "user", Some(&img), 16000, true);
        assert_eq!(b["model"], DEFAULT_CLAUDE_MODEL);
        assert_eq!(b["fallbacks"], "default");
        assert_eq!(b["system"], "sys");
        assert_eq!(b["messages"][0]["content"][0]["type"], "image");
        assert_eq!(b["messages"][0]["content"][1]["text"], "user");
        assert!(anthropic_body(&cfg, "s", "u", None, 10, false).get("fallbacks").is_none());
        assert!(uses_fallbacks("claude-opus-5") && !uses_fallbacks("claude-sonnet-5"));
    }

    #[test]
    fn parses_answers_and_refusals() {
        let ok = json!({"model": "claude-opus-5", "stop_reason": "end_turn",
            "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": "## Sažetak\nA"}, {"type": "text", "text": "B"}]});
        let a = parse_anthropic(&ok).unwrap();
        assert_eq!(a.text, "## Sažetak\nA\n\nB");
        assert!(!a.truncated);
        let cut = json!({"stop_reason": "max_tokens", "content": [{"type": "text", "text": "x"}]});
        assert!(parse_anthropic(&cut).unwrap().truncated);
        let refused = json!({"stop_reason": "refusal", "stop_details": {"category": "bio"}, "content": []});
        assert!(parse_anthropic(&refused).unwrap_err().to_string().contains("bio"));
        let oa = json!({"model": "llama3", "choices": [{"message": {"content": "Mišljenje"}, "finish_reason": "stop"}]});
        assert_eq!(parse_openai(&oa).unwrap().text, "Mišljenje");
        assert!(parse_openai(&json!({"choices": [{"message": {"content": null}}]})).is_err());
    }

    #[test]
    fn config_hides_key_and_picks_urls() {
        let c = AiConfig { api_key: "sk-secret".into(), ..Default::default() };
        let p = c.public().to_string();
        assert!(!p.contains("sk-secret") && p.contains("\"has_key\":true"));
        assert_eq!(c.base(), "https://api.anthropic.com");
        let o = AiConfig { provider: "ollama".into(), ..Default::default() };
        assert_eq!(o.base(), "http://127.0.0.1:11434");
        let old = AiConfig { provider: "ollama".into(), base_url: "http://localhost:11434/v1".into(), ..Default::default() };
        assert_eq!(old.base(), "http://localhost:11434");
        let custom = AiConfig { provider: "openai".into(), base_url: "http://x:1234/v1/".into(), ..Default::default() };
        assert_eq!(custom.base(), "http://x:1234/v1");
        assert!(AiConfig::default().check().is_err(), "anthropic without key");
    }

    #[test]
    fn local_services_skip_the_proxy() {
        assert_eq!(host_of("http://user:pw@[::1]:11434/v1"), "::1");
        assert_eq!(host_of("https://API.anthropic.com/v1/messages"), "api.anthropic.com");
        for u in ["http://localhost:11434/v1", "http://127.0.0.1:1234", "http://192.168.1.20:11434/v1", "http://10.0.0.5/v1", "http://[::1]:11434", "http://studio.local:1234/v1"] {
            assert!(bypass_proxy_with(u, ""), "{u}");
        }
        assert!(!bypass_proxy_with("https://api.anthropic.com", ""));
        assert!(!bypass_proxy_with("https://8.8.8.8/v1", "example.com"));
        assert!(bypass_proxy_with("https://llm.clinic.hr/v1", "localhost, .clinic.hr"));
    }

    #[test]
    fn images_are_validated() {
        let (m, d) = parse_image("data:image/png;base64,iVBORw0KGgo=").unwrap();
        assert_eq!((m.as_str(), d.as_str()), ("image/png", "iVBORw0KGgo="));
        assert!(parse_image("data:text/html;base64,PGI+").is_err());
        assert!(parse_image("data:image/jpeg;base64,@@@").is_err());
    }
}
