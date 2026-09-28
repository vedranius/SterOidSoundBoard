//! Local Ollama server through its native API (<https://github.com/ollama/ollama/blob/main/docs/api.md>):
//! status, installed and loaded models, pulling and deleting models, and a
//! streamed chat with an explicit context size — the OpenAI-compatible
//! endpoint cannot set the context, so long clinical prompts were cut off.
use crate::ai::{agent_with, Answer, Progress};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 127.0.0.1 rather than localhost: on Windows "localhost" may resolve to ::1 first,
/// while Ollama listens on IPv4 by default.
pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";
pub const DEFAULT_MODEL: &str = "gemma3:4b";
/// Longest answer we let a local model write (tokens).
const NUM_PREDICT: u64 = 4096;
const MAX_CTX: u64 = 32768;
/// Waiting for the first token covers loading the model and reading the
/// whole prompt, which on a CPU can take many minutes.
const FIRST_TOKEN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Server root from a configured URL ("…/v1" from older settings is dropped).
pub fn root(base_url: &str) -> String {
    let b = base_url.trim().trim_end_matches('/');
    let b = b.strip_suffix("/v1").unwrap_or(b).trim_end_matches('/');
    if b.is_empty() { DEFAULT_URL.to_string() } else { b.to_string() }
}

fn get(url: &str, timeout: Duration) -> Result<Value> {
    let ag = agent_with(url, timeout);
    match ag.get(url).call() {
        Ok(r) => Ok(r.into_json()?),
        Err(ureq::Error::Status(code, r)) => bail!("Ollama {code}: {}", err_text(&r.into_string().unwrap_or_default())),
        Err(e) => Err(anyhow!("{e}")),
    }
}

fn post(url: &str, body: &Value, timeout: Duration) -> std::result::Result<ureq::Response, (u16, String)> {
    let ag = agent_with(url, timeout);
    match ag.post(url).set("content-type", "application/json").send_json(body.clone()) {
        Ok(r) => Ok(r),
        Err(ureq::Error::Status(code, r)) => Err((code, err_text(&r.into_string().unwrap_or_default()))),
        Err(e) => Err((0, not_running(url, &e.to_string()))),
    }
}

fn err_text(body: &str) -> String {
    serde_json::from_str::<Value>(body).ok().and_then(|v| v["error"].as_str().map(str::to_string)).unwrap_or_else(|| body.chars().take(300).collect())
}

fn not_running(url: &str, e: &str) -> String {
    format!("Ollama nije dostupna na {} ({e}). Instalirajte je s ollama.com i pokrenite (ikona u traci ili `ollama serve`).", root(url.split("/api/").next().unwrap_or(url)))
}

fn gb(bytes: f64) -> String {
    format!("{:.1} GB", bytes / 1e9)
}

/// Capabilities of one model: (vision, context length).
pub fn capabilities(root: &str, model: &str) -> Result<(bool, Option<u64>)> {
    let r = post(&format!("{root}/api/show"), &json!({"model": model}), Duration::from_secs(15)).map_err(|(code, m)| {
        if code == 404 {
            anyhow!("Model „{model}” nije preuzet u Ollami — preuzmite ga u AI postavkama (Preuzmi) ili naredbom `ollama pull {model}`.")
        } else {
            anyhow!(m)
        }
    })?;
    let v: Value = r.into_json()?;
    let caps: Vec<&str> = v["capabilities"].as_array().map(|a| a.iter().filter_map(|c| c.as_str()).collect()).unwrap_or_default();
    // older servers have no capability list: vision models carry a projector / "clip"/"mllama" family
    let families: Vec<&str> = v["details"]["families"].as_array().map(|a| a.iter().filter_map(|c| c.as_str()).collect()).unwrap_or_default();
    let vision = caps.contains(&"vision") || v.get("projector_info").is_some() || families.iter().any(|f| *f == "clip" || *f == "mllama");
    let ctx = v["model_info"].as_object().and_then(|m| m.iter().find(|(k, _)| k.ends_with(".context_length")).and_then(|(_, n)| n.as_u64()));
    Ok((vision, ctx))
}

/// Everything the settings panel shows: server, installed and loaded models.
pub fn status(root: &str) -> Value {
    let version = match get(&format!("{root}/api/version"), Duration::from_secs(4)) {
        Ok(v) => v["version"].as_str().unwrap_or("?").to_string(),
        Err(e) => return json!({"running": false, "url": root, "error": not_running(root, &e.to_string())}),
    };
    let tags = get(&format!("{root}/api/tags"), Duration::from_secs(10)).unwrap_or(Value::Null);
    let models: Vec<Value> = tags["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .take(60)
                .map(|m| {
                    let name = m["name"].as_str().unwrap_or_default();
                    let (vision, ctx) = capabilities(root, name).unwrap_or((false, None));
                    json!({
                        "name": name,
                        "size": m["size"],
                        "params": m["details"]["parameter_size"],
                        "quant": m["details"]["quantization_level"],
                        "family": m["details"]["family"],
                        "vision": vision,
                        "context": ctx,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({"running": true, "url": root, "version": version, "models": models, "loaded": loaded(root)})
}

/// Models currently in memory with their share in GPU memory.
pub fn loaded(root: &str) -> Vec<Value> {
    get(&format!("{root}/api/ps"), Duration::from_secs(4))
        .ok()
        .and_then(|v| v["models"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let size = m["size"].as_f64().unwrap_or(0.0);
            let vram = m["size_vram"].as_f64().unwrap_or(0.0);
            json!({"name": m["name"], "size": size, "size_vram": vram, "gpu_pct": if size > 0.0 { (vram / size * 100.0).round() } else { 0.0 },
                   "context": m["context_length"], "expires_at": m["expires_at"]})
        })
        .collect()
}

fn describe_load(m: &Value) -> String {
    let (size, pct) = (m["size"].as_f64().unwrap_or(0.0), m["gpu_pct"].as_f64().unwrap_or(0.0));
    let ctx = m["context"].as_u64().map(|c| format!(", kontekst {c}")).unwrap_or_default();
    if pct >= 99.5 {
        format!("Model je u memoriji: {} potpuno na GPU-u{ctx} — brzo.", gb(size))
    } else if pct <= 0.5 {
        format!("Model je u memoriji: {} samo na CPU-u{ctx} — GPU se ne koristi, bit će sporo (provjerite upravljački program za NVIDIA/CUDA).", gb(size))
    } else {
        format!("Model je u memoriji: {} — {pct:.0} % na GPU-u, {:.0} % na CPU-u{ctx}. Ne stane cijeli u memoriju grafičke kartice pa je sporije; manji model ili manji kontekst bio bi brži.", gb(size), 100.0 - pct)
    }
}

/// Download a model (`ollama pull`), reporting progress.
pub fn pull(root: &str, model: &str, p: &dyn Progress) -> Result<()> {
    p.log(&format!("Preuzimam model {model} s ollama.com (preko Ollame na {root})…"));
    let r = post(&format!("{root}/api/pull"), &json!({"model": model, "stream": true}), Duration::from_secs(10 * 60)).map_err(|(_, m)| anyhow!(m))?;
    let mut last_status = String::new();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    for line in BufReader::new(r.into_reader()).lines() {
        if p.cancelled() {
            bail!("preuzimanje prekinuto");
        }
        let line = line.map_err(|e| anyhow!("veza s Ollamom je prekinuta: {e}"))?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(e) = v["error"].as_str() {
            bail!("Ollama: {e}");
        }
        let status = v["status"].as_str().unwrap_or_default().to_string();
        let (done, total) = (v["completed"].as_f64().unwrap_or(0.0), v["total"].as_f64().unwrap_or(0.0));
        let phase = status.split_whitespace().next().unwrap_or_default().to_string();
        if phase != last_status.split_whitespace().next().unwrap_or_default() {
            p.log(&match phase.as_str() {
                "pulling" if total > 0.0 => format!("Preuzimanje dijela {} ({})", v["digest"].as_str().unwrap_or("").chars().take(19).collect::<String>(), gb(total)),
                "pulling" => "Dohvaćam popis datoteka modela…".into(),
                "verifying" => "Provjeravam preuzete datoteke…".into(),
                "writing" => "Zapisujem model…".into(),
                "success" => format!("Model {model} je preuzet."),
                _ => status.clone(),
            });
            last_status = status.clone();
        }
        if total > 0.0 && last_emit.elapsed() > Duration::from_millis(250) {
            p.stats(json!({"status": status, "completed": done, "total": total, "pct": (done / total * 100.0).round()}));
            last_emit = Instant::now();
        }
        if status == "success" {
            p.stats(json!({"status": "success", "pct": 100}));
            return Ok(());
        }
    }
    bail!("preuzimanje nije završeno (veza prekinuta)")
}

pub fn delete(root: &str, model: &str) -> Result<()> {
    let url = format!("{root}/api/delete");
    let ag = agent_with(&url, Duration::from_secs(30));
    match ag.delete(&url).set("content-type", "application/json").send_json(json!({"model": model})) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, r)) => bail!("Ollama {code}: {}", err_text(&r.into_string().unwrap_or_default())),
        Err(e) => bail!(not_running(&url, &e.to_string())),
    }
}

/// Rough token count for Croatian text (≈ 2.5 characters per token).
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as f64 / 2.5).ceil() as u64
}

/// Context size: the user's choice, or just enough for prompt + answer.
pub fn context_size(configured: u32, prompt_tokens: u64, image: bool, model_max: Option<u64>) -> u64 {
    let cap = model_max.unwrap_or(MAX_CTX).clamp(2048, MAX_CTX);
    if configured > 0 {
        return (configured as u64).min(cap);
    }
    let need = prompt_tokens + if image { 1600 } else { 0 } + NUM_PREDICT + 256;
    need.div_ceil(1024).saturating_mul(1024).clamp(4096.min(cap), cap)
}

pub fn chat_body(model: &str, system: &str, user: &str, image: Option<&str>, num_ctx: u64, num_predict: u64) -> Value {
    let mut msg = json!({"role": "user", "content": user});
    if let Some(b64) = image {
        msg["images"] = json!([b64]);
    }
    json!({
        "model": model,
        "messages": [{"role": "system", "content": system}, msg],
        "stream": true,
        "keep_alive": "30m",
        "options": {"num_ctx": num_ctx, "num_predict": num_predict, "temperature": 0.4},
    })
}

/// Remove `<think>…</think>` blocks that reasoning models put in the answer.
pub fn strip_thinking(t: &str) -> String {
    let mut out = String::new();
    let mut rest = t;
    while let Some(a) = rest.find("<think>") {
        out.push_str(&rest[..a]);
        match rest[a..].find("</think>") {
            Some(b) => rest = &rest[a + b + "</think>".len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// Streamed chat: the answer arrives token by token, with a running log.
pub fn chat(root: &str, model: &str, ctx_setting: u32, system: &str, user: &str, image: Option<&(String, String)>, max_tokens: u64, p: &dyn Progress) -> Result<Answer> {
    let t0 = Instant::now();
    p.log(&format!("Ollama na {root}, model {model}."));
    let (vision, model_ctx) = capabilities(root, model)?;
    let image = match image {
        Some(img) if vision => Some(img),
        Some(_) => {
            p.log("Ovaj model nema vid (vision) — šaljem samo podatke, bez slike sonagrama.");
            None
        }
        None => None,
    };
    let prompt_tokens = estimate_tokens(system) + estimate_tokens(user);
    let num_predict = max_tokens.min(NUM_PREDICT);
    let num_ctx = context_size(ctx_setting, prompt_tokens, image.is_some(), model_ctx);
    let need = prompt_tokens + if image.is_some() { 1600 } else { 0 };
    p.log(&format!(
        "Upit ≈ {prompt_tokens} tokena{}; kontekst {num_ctx} tokena, odgovor do {num_predict} tokena.",
        image.map(|(_, d)| format!(" + slika ({} kB)", d.len() * 3 / 4 / 1024)).unwrap_or_default()
    ));
    if need + 512 > num_ctx {
        p.log(&format!("UPOZORENJE: kontekst {num_ctx} je premalen za upit (≈ {need}) — Ollama će odrezati početak upita. Povećajte kontekst u AI postavkama ili odaberite „automatski”."));
    }
    let loaded_now = loaded(root);
    match loaded_now.iter().find(|m| m["name"].as_str() == Some(model)) {
        Some(m) => p.log(&describe_load(m)),
        None => p.log("Učitavam model u memoriju (prvi put nakon pokretanja može potrajati)…"),
    }
    p.log("Model čita upit — prvi dio odgovora stiže kad obradi cijeli upit (na CPU-u to može trajati i nekoliko minuta).");

    // report where the model landed (GPU/CPU) as soon as Ollama has loaded it
    let first = Arc::new(AtomicBool::new(false));
    std::thread::scope(|s| -> Result<Answer> {
        let watch_first = first.clone();
        let already = loaded_now.iter().any(|m| m["name"].as_str() == Some(model));
        s.spawn(move || {
            if already {
                return;
            }
            let start = Instant::now();
            while !watch_first.load(Ordering::Relaxed) && start.elapsed() < FIRST_TOKEN_TIMEOUT && !p.cancelled() {
                std::thread::sleep(Duration::from_millis(1500));
                if let Some(m) = loaded(root).into_iter().find(|m| m["name"].as_str() == Some(model)) {
                    p.log(&format!("{} (učitano nakon {:.0} s)", describe_load(&m), start.elapsed().as_secs_f64()));
                    return;
                }
            }
        });
        let result = stream_chat(root, model, system, user, image, num_ctx, num_predict, t0, p, &first);
        first.store(true, Ordering::Relaxed);
        result
    })
}

#[allow(clippy::too_many_arguments)]
fn stream_chat(root: &str, model: &str, system: &str, user: &str, image: Option<&(String, String)>, num_ctx: u64, num_predict: u64, t0: Instant, p: &dyn Progress, first: &AtomicBool) -> Result<Answer> {
    let body = chat_body(model, system, user, image.map(|(_, d)| d.as_str()), num_ctx, num_predict);
    let r = post(&format!("{root}/api/chat"), &body, FIRST_TOKEN_TIMEOUT).map_err(|(code, m)| anyhow!(if code == 0 { m } else { format!("Ollama je vratila grešku {code}: {m}") }))?;
    let (mut text, mut n, mut thinking) = (String::new(), 0u64, 0u64);
    let mut gen_start: Option<Instant> = None;
    let mut last_stats = Instant::now();
    for line in BufReader::new(r.into_reader()).lines() {
        if p.cancelled() {
            bail!("prekinuto");
        }
        let line = line.map_err(|e| anyhow!("veza s Ollamom je prekinuta: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line).map_err(|e| anyhow!("neispravan odgovor Ollame: {e}"))?;
        if let Some(e) = v["error"].as_str() {
            bail!("Ollama: {e}");
        }
        if let Some(th) = v["message"]["thinking"].as_str().filter(|t| !t.is_empty()) {
            if thinking == 0 {
                p.log(&format!("Model razmišlja prije odgovora (nakon {:.0} s)…", t0.elapsed().as_secs_f64()));
                first.store(true, Ordering::Relaxed);
            }
            thinking += 1;
            let _ = th;
        }
        if let Some(c) = v["message"]["content"].as_str().filter(|c| !c.is_empty()) {
            if gen_start.is_none() {
                gen_start = Some(Instant::now());
                first.store(true, Ordering::Relaxed);
                p.log(&format!("Prvi dio odgovora nakon {:.0} s — model piše…", t0.elapsed().as_secs_f64()));
            }
            n += 1;
            text.push_str(c);
            p.delta(c);
        }
        if last_stats.elapsed() > Duration::from_millis(1000) {
            let g = gen_start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
            p.stats(json!({"tokens": n, "thinking": thinking, "tps": if g > 0.5 { (n as f64 / g * 10.0).round() / 10.0 } else { 0.0 }, "elapsed": t0.elapsed().as_secs()}));
            last_stats = Instant::now();
        }
        if v["done"].as_bool() == Some(true) {
            let ns = |k: &str| v[k].as_f64().unwrap_or(0.0) / 1e9;
            let (pc, pd, ec, ed) = (v["prompt_eval_count"].as_f64().unwrap_or(0.0), ns("prompt_eval_duration"), v["eval_count"].as_f64().unwrap_or(n as f64), ns("eval_duration"));
            let rate = |c: f64, d: f64| if d > 0.0 { format!("{:.1} tok/s", c / d) } else { "—".into() };
            p.log(&format!(
                "Gotovo za {:.0} s: učitavanje modela {:.1} s · upit {pc:.0} tokena za {pd:.1} s ({}) · odgovor {ec:.0} tokena za {ed:.1} s ({}).",
                t0.elapsed().as_secs_f64(),
                ns("load_duration"),
                rate(pc, pd),
                rate(ec, ed)
            ));
            if pc > 0.0 && (pc as u64) + 64 >= num_ctx {
                p.log("UPOZORENJE: upit je popunio cijeli kontekst — dio je vjerojatno odrezan. Povećajte kontekst.");
            }
            let reason = v["done_reason"].as_str().unwrap_or("stop").to_string();
            if reason == "length" {
                p.log("Odgovor je dosegnuo najveću duljinu i skraćen je.");
            }
            p.stats(json!({"tokens": ec, "tps": if ed > 0.0 { (ec / ed * 10.0).round() / 10.0 } else { 0.0 }, "prompt_tokens": pc, "prompt_tps": if pd > 0.0 { (pc / pd * 10.0).round() / 10.0 } else { 0.0 }, "elapsed": t0.elapsed().as_secs(), "done": true}));
            let answer = strip_thinking(&text);
            if answer.is_empty() {
                bail!("model nije vratio tekst odgovora");
            }
            return Ok(Answer { text: answer, model: model.to_string(), truncated: reason == "length" });
        }
    }
    bail!("Ollama je prekinula vezu prije kraja odgovora")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_and_context() {
        assert_eq!(root(""), DEFAULT_URL);
        assert_eq!(root("http://localhost:11434/v1/"), "http://localhost:11434");
        assert_eq!(root("http://gpu-box:11434"), "http://gpu-box:11434");
        // auto: prompt + image + answer, rounded up, at least 4096, capped by the model
        assert_eq!(context_size(0, 1000, false, None), 6144);
        assert_eq!(context_size(0, 100, false, None), 5120);
        assert_eq!(context_size(0, 100, false, Some(4096)), 4096);
        assert_eq!(context_size(0, 100, false, Some(2048)), 2048);
        assert_eq!(context_size(0, 5000, true, Some(8192)), 8192);
        assert_eq!(context_size(0, 60000, false, None), MAX_CTX);
        assert_eq!(context_size(16384, 100, false, Some(131072)), 16384);
        assert_eq!(context_size(16384, 100, false, Some(8192)), 8192);
    }

    #[test]
    fn chat_request_shape() {
        let b = chat_body("gemma3:4b", "sys", "user", Some("AAAA"), 8192, 4096);
        assert_eq!(b["stream"], true);
        assert_eq!(b["options"]["num_ctx"], 8192);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["images"][0], "AAAA");
        assert!(chat_body("m", "s", "u", None, 4096, 10)["messages"][1].get("images").is_none());
    }

    #[test]
    fn thinking_is_removed() {
        assert_eq!(strip_thinking("<think>hmm\nok</think>\n## Sažetak\nA"), "## Sažetak\nA");
        assert_eq!(strip_thinking("A<think>x</think>B"), "AB");
        assert_eq!(strip_thinking("A<think>never closed"), "A");
        assert_eq!(strip_thinking("plain"), "plain");
    }
}
