//! AI opinion on a voice recording (DigiLingua). Providers are the user's
//! choice with the user's own key: Anthropic Claude, any OpenAI-compatible
//! API, or a local Ollama server (no data leaves the machine).
//!
//! The AI never receives audio: it gets the acoustic measurements, a
//! pseudonymised patient profile (no name, code or birth date), the
//! clinician's notes and, optionally, the sonagram image.
use crate::clinic::{AiReport, PatientIn, Recording};
use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt::Write;
use std::path::Path;
use std::time::Duration;
use steroid_engine::analysis::{self, VoiceReport};

pub const DEFAULT_CLAUDE_MODEL: &str = "claude-opus-5";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Server-side refusal fallback ("default" = Anthropic picks the fallback model).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_TOKENS: u32 = 16_000;
const MAX_IMAGE_BYTES: usize = 5 << 20;

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
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            provider: "anthropic".into(),
            base_url: String::new(),
            model: DEFAULT_CLAUDE_MODEL.into(),
            api_key: String::new(),
            send_image: true,
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
        let b = self.base_url.trim().trim_end_matches('/');
        if !b.is_empty() {
            return b.to_string();
        }
        match self.provider.as_str() {
            "openai" => "https://api.openai.com/v1".into(),
            "ollama" => "http://localhost:11434/v1".into(),
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
            "default_claude_model": DEFAULT_CLAUDE_MODEL,
        })
    }

    pub fn check(&self) -> Result<()> {
        if !matches!(self.provider.as_str(), "anthropic" | "openai" | "ollama") {
            bail!("nepoznat AI servis");
        }
        if self.model.trim().is_empty() {
            bail!("AI model nije postavljen (AI postavke)");
        }
        if self.provider == "anthropic" && self.api_key.is_empty() {
            bail!("nedostaje Anthropic API ključ (AI postavke)");
        }
        Ok(())
    }
}

/// Validated image for the request: (media type, base64 data).
pub fn parse_image(data_url: &str) -> Result<(String, String)> {
    let (mime, b64) = match data_url.strip_prefix("data:") {
        Some(rest) => {
            let (head, data) = rest.split_once(',').ok_or_else(|| anyhow!("neispravna slika"))?;
            (head.trim_end_matches(";base64").to_string(), data.to_string())
        }
        None => ("image/jpeg".to_string(), data_url.to_string()),
    };
    if mime != "image/jpeg" && mime != "image/png" {
        bail!("slika mora biti JPEG ili PNG");
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| anyhow!("neispravna slika (base64)"))?;
    if bytes.len() > MAX_IMAGE_BYTES {
        bail!("slika je prevelika");
    }
    Ok((mime, b64.trim().to_string()))
}

// ------------------------------------------------------------------ prompt

pub struct Context<'a> {
    pub patient: Option<&'a PatientIn>,
    pub age: Option<u32>,
    pub rec: &'a Recording,
    pub start: f32,
    pub end: f32,
    pub report: &'a VoiceReport,
    /// Earlier recordings of the same patient: (date, task, whole-file analysis).
    pub history: &'a [(String, String, VoiceReport)],
    pub question: &'a str,
    pub has_image: bool,
}

pub const SYSTEM_PROMPT: &str = "\
Ti si iskusni stručnjak za kliničku fonetiku, logopediju i rehabilitaciju govora i glasa. \
Pomažeš rehabilitatoru (logopedu, fonetičaru, surdologu) koji radi s pacijentom u aplikaciji \
SterOidSoundBoard / DigiLingua.

Dobivaš: pseudonimizirani profil pacijenta (bez imena i identifikatora), opis trenutnog problema, \
vrstu govornog zadatka, opažanja rehabilitatora, akustičke mjere izračunate iz snimke algoritmima \
po uzoru na Praat/MDVP, sažetak F0 konture, usporedbu s prethodnim snimkama i ponekad sliku \
sonagrama s ucrtanom F0 konturom. Ne slušaš snimku — zaključuješ iz mjera i slike.

Napiši stručno mišljenje na hrvatskom jeziku za daljnju obradu rehabilitatora, sa sljedećim \
odjeljcima (markdown naslovi ##):
## Sažetak
## Interpretacija akustičkih parametara
(u odnosu na dob, spol i vrstu zadatka; navedi koje su mjere za ovaj zadatak pouzdane, a koje nisu — \
npr. jitter/shimmer/HNR vrijede za produženi vokal, a ne za tečni govor)
## Povezanost s trenutnim problemom i anamnezom
## Mogući obrasci i što dodatno provjeriti
(diferencijalna razmatranja kao hipoteze, ne dijagnoze)
## Prijedlozi za rehabilitaciju
(konkretne vježbe i ciljevi; gdje ima smisla i postavke aplikacije: DAF 0–5000 ms, FAF ±12 \
polutonova, maskirajući šum bijeli/ružičasti/smeđi/uskopojasni po uhu, diskontinuitet, 31-pojasni EQ)
## Preporuke za daljnju obradu
(npr. fonijatrijski/ORL pregled, laringoskopija, audiološka obrada — jasno istakni znakove \
upozorenja koji zahtijevaju liječnički pregled)
## Ograničenja ove analize

Pravila: ne postavljaj medicinsku dijagnozu i ne izmišljaj podatke kojih nema; kad nešto ne znaš, \
reci to. Razlikuj izmjereno od pretpostavljenog. Intenzitet je u dBFS (nije kalibriran na dB SPL). \
Budi sažet i praktičan — rehabilitator je stručnjak.";

fn opt(v: Option<f32>, d: usize, unit: &str) -> String {
    v.map(|x| format!("{x:.d$} {unit}").trim_end().to_string()).unwrap_or_else(|| "nije izmjereno".into())
}

fn measures(s: &mut String, r: &VoiceReport) {
    let _ = writeln!(s, "- Trajanje: {:.2} s; zvučni dio: {:.0} %", r.duration, r.voiced_fraction * 100.0);
    let _ = writeln!(
        s,
        "- F0 srednja: {}; medijan {}; SD {}; min {}; max {}; raspon (5.–95. pct.) {}",
        opt(r.f0_mean, 1, "Hz"),
        opt(r.f0_median, 1, "Hz"),
        opt(r.f0_sd, 1, "Hz"),
        opt(r.f0_min, 1, "Hz"),
        opt(r.f0_max, 1, "Hz"),
        opt(r.f0_range_st, 1, "polutonova")
    );
    let _ = writeln!(
        s,
        "- Jitter local: {} (norma < {} %); jitter abs: {}; jitter RAP: {}",
        opt(r.jitter_local, 2, "%"),
        analysis::JITTER_MAX,
        opt(r.jitter_abs_us, 1, "µs"),
        opt(r.jitter_rap, 2, "%")
    );
    let _ = writeln!(
        s,
        "- Shimmer local: {} (norma < {} %); shimmer dB: {} (norma < {} dB)",
        opt(r.shimmer_local, 2, "%"),
        analysis::SHIMMER_MAX,
        opt(r.shimmer_db, 3, "dB"),
        analysis::SHIMMER_DB_MAX
    );
    let _ = writeln!(s, "- HNR: {} (norma > {} dB); intenzitet zvučnog dijela: {}", opt(r.hnr_db, 1, "dB"), analysis::HNR_MIN, opt(r.intensity_dbfs, 1, "dBFS"));
    let _ = writeln!(s, "- Broj analiziranih glotalnih perioda: {}", r.periods);
    let _ = writeln!(s, "- Najduža neprekinuta fonacija: {:.2} s; prekidi zvučnosti: {} (udio {:.1} %)", r.max_voiced_s, r.voice_breaks, r.voice_break_degree);
    let _ = writeln!(s, "- Pauze ≥ 250 ms: {} (prosjek {:.2} s; {:.1} % trajanja)", r.pauses, r.pause_mean_s, r.pause_ratio);
    let _ = writeln!(
        s,
        "- Slogovne jezgre: {}; brzina govora {:.2} slog/s; brzina artikulacije {:.2} slog/s",
        r.syllable_nuclei, r.speech_rate, r.articulation_rate
    );
}

/// F0 over time in ten equal slices ("—" = no voicing in that slice).
fn contour(r: &VoiceReport) -> String {
    if r.pitch.is_empty() || r.duration <= 0.0 {
        return "nema podataka".into();
    }
    (0..10)
        .map(|i| {
            let (a, b) = (r.duration * i as f32 / 10.0, r.duration * (i + 1) as f32 / 10.0);
            let v: Vec<f32> = r.pitch.iter().filter(|p| p[0] >= a && p[0] < b && p[1] > 0.0).map(|p| p[1]).collect();
            if v.is_empty() { "—".to_string() } else { format!("{:.0}", v.iter().sum::<f32>() / v.len() as f32) }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Returns (system prompt, user message). Identifying fields are never included.
pub fn build_prompt(c: &Context) -> (String, String) {
    let mut s = String::new();
    s.push_str("# Pacijent (pseudonimizirano)\n");
    match c.patient {
        Some(p) => {
            let f = |label: &str, v: &str, s: &mut String| {
                if !v.trim().is_empty() {
                    let _ = writeln!(s, "- {label}: {}", v.trim());
                }
            };
            let _ = writeln!(s, "- Dob: {}", c.age.map(|a| format!("{a} god.")).unwrap_or_else(|| "nije navedena".into()));
            let _ = writeln!(s, "- Spol: {}", if p.sex.is_empty() { "nije naveden" } else { &p.sex });
            f("Dijagnoza", &p.diagnosis, &mut s);
            f("Trenutni problem / razlog dolaska", &p.complaint, &mut s);
            f("Anamneza problema", &p.history, &mut s);
            f("Medicinska povijest", &p.medical, &mut s);
            f("Lijekovi", &p.medications, &mut s);
            f("Zanimanje i vokalno opterećenje", &p.voice_use, &mut s);
            f("Pušenje", &p.smoking, &mut s);
            f("Sluh", &p.hearing, &mut s);
            f("Materinski jezik", &p.language, &mut s);
            f("Ciljevi terapije", &p.goals, &mut s);
            f("Bilješke rehabilitatora o pacijentu", &p.notes, &mut s);
        }
        None => s.push_str("- Snimka nije povezana s pacijentom; profil nije poznat.\n"),
    }
    s.push_str("\n# Snimka\n");
    let _ = writeln!(s, "- Zadatak: {}", if c.rec.task.is_empty() { "nije naveden" } else { &c.rec.task });
    let _ = writeln!(s, "- Oznaka: {}", if c.rec.label.is_empty() { "—" } else { &c.rec.label });
    let _ = writeln!(
        s,
        "- Izvor: {}; frekvencija uzorkovanja {} Hz; analizirani odsječak {:.2}–{:.2} s od ukupno {:.2} s",
        if c.rec.source == "out" { "obrađeni izlaz (s efektima)" } else { "suhi mikrofon" },
        c.rec.sample_rate,
        c.start,
        c.end,
        c.rec.duration
    );
    if !c.rec.notes.trim().is_empty() {
        let _ = writeln!(s, "- Opažanja rehabilitatora tijekom snimanja: {}", c.rec.notes.trim());
    }
    s.push_str("\n# Akustičke mjere (odsječak)\n");
    measures(&mut s, c.report);
    let _ = writeln!(s, "- F0 kontura po desetinama odsječka (Hz): {}", contour(c.report));
    if !c.history.is_empty() {
        s.push_str("\n# Prethodne snimke istog pacijenta (cijele snimke, od starije prema novijoj)\n");
        for (date, task, r) in c.history {
            let _ = writeln!(
                s,
                "- {date} · {}: F0 {} · jitter {} · shimmer {} · HNR {} · najduža fonacija {:.1} s · pauze {} · brzina govora {:.2} slog/s",
                if task.is_empty() { "zadatak nije naveden" } else { task },
                opt(r.f0_mean, 0, "Hz"),
                opt(r.jitter_local, 2, "%"),
                opt(r.shimmer_local, 2, "%"),
                opt(r.hnr_db, 1, "dB"),
                r.max_voiced_s,
                r.pauses,
                r.speech_rate
            );
        }
    }
    if c.has_image {
        s.push_str("\nPriložena slika: sonagram odsječka 0–8 kHz (svjetlije = više energije), žute točke = F0 kontura (desna skala 0–500 Hz).\n");
    }
    s.push_str("\n# Zahtjev rehabilitatora\n");
    if c.question.trim().is_empty() {
        s.push_str("Napiši stručno mišljenje prema zadanoj strukturi.\n");
    } else {
        let _ = writeln!(s, "{}\n(Uz to napiši stručno mišljenje prema zadanoj strukturi.)", c.question.trim());
    }
    (SYSTEM_PROMPT.to_string(), s)
}

// ------------------------------------------------------------------ providers

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
            bail!("AI model je odbio zahtjev (kategorija: {cat}). Pokušajte preformulirati pitanje.");
        }
        _ => {}
    }
    let text: Vec<&str> = v["content"]
        .as_array()
        .map(|a| a.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect())
        .unwrap_or_default();
    if text.is_empty() {
        bail!("AI nije vratio tekst (stop_reason: {})", v["stop_reason"]);
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
            bail!("AI model je odbio zahtjev: {r}");
        }
        bail!("AI nije vratio tekst");
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

fn agent(url: &str) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        // non-streaming: a long, carefully reasoned answer can take minutes
        .timeout_read(Duration::from_secs(600))
        .timeout_write(Duration::from_secs(60))
        .try_proxy_from_env(!bypass_proxy(url))
        .build()
}

/// Human-readable error from a provider's JSON error body.
fn error_message(code: u16, body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = v["error"]["message"].as_str().or_else(|| v["error"].as_str()).or_else(|| v["message"].as_str()).unwrap_or(body).chars().take(400).collect::<String>();
    let hint = match code {
        401 | 403 => " — provjerite API ključ",
        404 => " — provjerite model i adresu servisa",
        429 => " — previše zahtjeva ili nema kredita",
        _ => "",
    };
    format!("AI servis je vratio grešku {code}: {msg}{hint}")
}

/// POST JSON with up to two retries on 408/429/5xx and connection errors.
fn post(url: &str, headers: &[(&str, &str)], body: &Value) -> std::result::Result<Value, (u16, String)> {
    let ag = agent(url);
    let mut attempt = 0;
    loop {
        let mut req = ag.post(url).set("content-type", "application/json");
        for (k, v) in headers {
            req = req.set(k, v);
        }
        let retry_in = match req.send_json(body.clone()) {
            Ok(resp) => {
                return resp.into_json::<Value>().map_err(|e| (0, format!("neispravan odgovor AI servisa: {e}")));
            }
            Err(ureq::Error::Status(code, resp)) => {
                let wait = resp.header("retry-after").and_then(|h| h.parse::<u64>().ok());
                let text = resp.into_string().unwrap_or_default();
                if !(code == 408 || code == 429 || code >= 500) || attempt >= 2 {
                    return Err((code, error_message(code, &text)));
                }
                Duration::from_secs(wait.unwrap_or(2 << attempt).min(30))
            }
            Err(ureq::Error::Transport(t)) => {
                if attempt >= 1 {
                    return Err((0, format!("nije moguće spojiti se na {url}: {t}")));
                }
                Duration::from_secs(2)
            }
        };
        attempt += 1;
        std::thread::sleep(retry_in);
    }
}

pub fn ask(cfg: &AiConfig, system: &str, user: &str, image: Option<&(String, String)>, max_tokens: u32) -> Result<Answer> {
    cfg.check()?;
    let base = cfg.base();
    match cfg.provider.as_str() {
        "anthropic" => {
            let url = if base.ends_with("/v1") { format!("{base}/messages") } else { format!("{base}/v1/messages") };
            let key = cfg.api_key.trim();
            let mut fallbacks = uses_fallbacks(cfg.model.trim());
            loop {
                let mut headers = vec![("x-api-key", key), ("anthropic-version", ANTHROPIC_VERSION)];
                if fallbacks {
                    headers.push(("anthropic-beta", FALLBACK_BETA));
                }
                match post(&url, &headers, &anthropic_body(cfg, system, user, image, max_tokens, fallbacks)) {
                    Ok(v) => return parse_anthropic(&v),
                    // a gateway or older deployment may not know the fallback beta: retry without it
                    Err((400, m)) if fallbacks && m.to_lowercase().contains("fallback") => fallbacks = false,
                    Err((_, m)) => bail!(m),
                }
            }
        }
        _ => {
            let url = format!("{base}/chat/completions");
            let auth = format!("Bearer {}", cfg.api_key.trim());
            let headers: Vec<(&str, &str)> = if cfg.api_key.trim().is_empty() { vec![] } else { vec![("authorization", auth.as_str())] };
            post(&url, &headers, &openai_body(cfg, system, user, image)).map_err(|(_, m)| anyhow!(m)).and_then(|v| parse_openai(&v))
        }
    }
}

pub fn ask_report(cfg: &AiConfig, system: &str, user: &str, image: Option<&(String, String)>) -> Result<Answer> {
    ask(cfg, system, user, image, MAX_TOKENS)
}

/// Small round-trip to verify provider, model and key.
pub fn test(cfg: &AiConfig) -> Result<Answer> {
    ask(cfg, "Odgovaraj kratko.", "Ovo je test veze. Odgovori samo riječju: OK", None, 1024)
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
        });
        for secret in ["Ivan", "Horvat", "MBO-123456", "1980-05-01"] {
            assert!(!user.contains(secret) && !sys.contains(secret), "leaked {secret}");
        }
        for want in ["46 god.", "Spol: M", "promuklost 3 mjeseca", "pušač", "Produženi vokal /a/", "tvrdi počeci", "HNR", "2026-09-01", "Je li HNR", "sonagram"] {
            assert!(user.contains(want), "missing {want}\n{user}");
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
        assert_eq!(o.base(), "http://localhost:11434/v1");
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
