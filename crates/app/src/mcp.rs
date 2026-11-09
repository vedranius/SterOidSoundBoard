//! Model Context Protocol server: lets an AI assistant (Claude Desktop,
//! Claude Code, ChatGPT and other MCP clients) read a patient's chart, run the
//! Praat-compatible analysis on recordings, follow progress and save its own
//! assessment as a note in the chart.
//!
//! Transports: Streamable HTTP at `POST /mcp` of the running app (bearer
//! token), and stdio (`steroidsoundboard --mcp`), which forwards to the running
//! app when there is one and otherwise works on the data directory itself.
//!
//! Privacy: off until enabled; by default only patients with recorded AI
//! consent are visible and they are pseudonymised (no name, code or birth
//! date leaves the app — a patient can still be *found* by name).
use crate::clinic::{self, Patient};
use crate::state::App;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

pub const PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    pub enabled: bool,
    /// Bearer token for HTTP clients.
    pub token: String,
    /// "pseudonym" (default) or "full".
    pub identity: String,
    /// Only patients who consented to AI processing.
    pub consent_only: bool,
    /// Assistants may add notes to the chart.
    pub allow_write: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        McpConfig { enabled: false, token: String::new(), identity: "pseudonym".into(), consent_only: true, allow_write: true }
    }
}

impl McpConfig {
    pub fn load(path: &Path) -> McpConfig {
        let mut c: McpConfig = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if c.token.len() < 32 {
            c.token = new_token();
            let _ = c.save(path);
        }
        c
    }
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
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
    pub fn full(&self) -> bool {
        self.identity == "full"
    }
}

/// 48 random hex characters from the OS (via std's randomly seeded hasher state).
pub fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut s = String::new();
    for i in 0..6u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i ^ clinic::now_ms());
        h.write_usize(&s as *const _ as usize);
        s.push_str(&format!("{:016x}", h.finish()));
    }
    s[..48].to_string()
}

/// Constant-time comparison of the presented token.
pub fn token_ok(cfg: &McpConfig, presented: &str) -> bool {
    let (a, b) = (cfg.token.as_bytes(), presented.as_bytes());
    a.len() == b.len() && !a.is_empty() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Stable pseudonym of a patient id ("P-" + 6 characters).
fn pseudonym(id: &str) -> String {
    let h = id.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("P-{:06X}", h & 0xFFFFFF)
}

fn fold(s: &str) -> String {
    s.to_lowercase().replace('č', "c").replace('ć', "c").replace('ž', "z").replace('š', "s").replace('đ', "d")
}

fn visible(cfg: &McpConfig, p: &Patient) -> bool {
    !cfg.consent_only || p.data.ai_consent
}

/// What an assistant may see of a patient record.
fn patient_json(cfg: &McpConfig, p: &Patient) -> Value {
    let d = &p.data;
    let mut v = json!({
        "patient_id": p.id,
        "pseudonym": pseudonym(&p.id),
        "age": clinic::age_at(&d.birth, clinic::now_ms()),
        "sex": d.sex,
        "diagnosis": d.diagnosis,
        "anamnesis": {
            "complaint": d.complaint, "history": d.history, "medical": d.medical, "medications": d.medications,
            "voice_use": d.voice_use, "smoking": d.smoking, "hearing": d.hearing, "language": d.language,
            "goals": d.goals, "notes": d.notes,
        },
        "ai_consent": d.ai_consent,
        "created": p.created,
        "updated": p.updated,
    });
    if cfg.full() {
        v["name"] = json!(d.name);
        v["code"] = json!(d.code);
        v["birth"] = json!(d.birth);
    }
    v
}

fn tools() -> Value {
    let pid = json!({"type": "string", "description": "patient_id from find_patients"});
    json!([
        {"name": "find_patients", "title": "Find patients",
         "description": "Search the clinic's patients by name, code or diagnosis (empty query = all). Returns patient_id, age, sex, diagnosis, counts of sessions/recordings and the last visit. Start here; names may be withheld (pseudonym mode) but matching by name still works.",
         "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}},
         "annotations": {"readOnlyHint": true}},
        {"name": "get_patient_chart", "title": "Patient chart",
         "description": "The whole chart of one patient: anamnesis (complaint, history, medical history, medications, voice use, smoking, hearing, language, goals), all therapy/assessment sessions with SOAP notes, tasks, homework and next appointment, recordings with perceptual ratings (GRBAS, CAPE-V) and saved measurements, questionnaires (VHI-10…), clinical notes and earlier AI opinions.",
         "inputSchema": {"type": "object", "properties": {"patient_id": pid}, "required": ["patient_id"]},
         "annotations": {"readOnlyHint": true}},
        {"name": "get_voice_profile", "title": "Voice profile (MPT, s/z, DSI)",
         "description": "Clinical voice profile from the patient's task recordings: maximum phonation time, s/z ratio, F0 range of the glissando, softest intensity, jitter and the Dysphonia Severity Index — for one session or the latest recording of each task.",
         "inputSchema": {"type": "object", "properties": {"patient_id": pid, "session_id": {"type": "string"}}, "required": ["patient_id"]},
         "annotations": {"readOnlyHint": true}},
        {"name": "analyze_recording", "title": "Acoustic analysis of a recording",
         "description": "Praat-compatible voice report of a recording (or of start–end seconds): F0, jitter ×5, shimmer ×6, HNR/NHR, CPPS, formants, intensity, voicing, phonation and fluency timing, plus the automatic clinical summary and the disfluency labels summary (%SS).",
         "inputSchema": {"type": "object", "properties": {"recording_id": {"type": "string"}, "start": {"type": "number"}, "end": {"type": "number"}}, "required": ["recording_id"]},
         "annotations": {"readOnlyHint": true}},
        {"name": "get_progress", "title": "Progress over time",
         "description": "One row of key measures per recording of the patient, oldest first (F0, jitter, shimmer, HNR, CPPS, intensity, MPT, speech/articulation rate, pauses, formants, disfluencies, %SS, GRBAS G, CAPE-V) and questionnaire scores — for judging progress and effect of therapy.",
         "inputSchema": {"type": "object", "properties": {"patient_id": pid}, "required": ["patient_id"]},
         "annotations": {"readOnlyHint": true}},
        {"name": "get_session", "title": "Session detail",
         "description": "One session: date, kind, SOAP note, tasks, homework, next appointment, the auditory-feedback settings used (DigiLingua FILTER: EQ, DAF, FAF, noise…) and its recordings.",
         "inputSchema": {"type": "object", "properties": {"session_id": {"type": "string"}}, "required": ["session_id"]},
         "annotations": {"readOnlyHint": true}},
        {"name": "get_reference_values", "title": "Reference values",
         "description": "Indicative norms used by the app (jitter, shimmer, HNR, MPT, s/z, DSI, CPPS) and the speech tasks / protocols it knows.",
         "inputSchema": {"type": "object", "properties": {}},
         "annotations": {"readOnlyHint": true}},
        {"name": "add_clinical_note", "title": "Save a note in the chart",
         "description": "Save your assessment, summary or plan as a clinical note in the patient's chart (marked as written by the AI assistant; the clinician reviews it). Use the clinician's language (Croatian by default).",
         "inputSchema": {"type": "object", "properties": {"patient_id": pid, "title": {"type": "string"}, "text": {"type": "string"}}, "required": ["patient_id", "text"]},
         "annotations": {"readOnlyHint": false, "destructiveHint": false}},
    ])
}

fn prompts() -> Value {
    let arg = json!([{"name": "patient", "description": "patient name or id", "required": true}]);
    json!([
        {"name": "procjena_pacijenta", "title": "Sveobuhvatna procjena pacijenta", "description": "Full assessment of a patient from the chart, the acoustic measures and the history.", "arguments": arg},
        {"name": "napredak_terapije", "title": "Napredak i kontrola", "description": "Progress since the start of therapy and since the last control, with recommendations.", "arguments": arg},
    ])
}

fn prompt_text(name: &str, patient: &str) -> Option<String> {
    Some(match name {
        "procjena_pacijenta" => format!(
            "Pronađi pacijenta „{patient}” (find_patients), pročitaj cijeli karton (get_patient_chart), glasovni profil (get_voice_profile) i napredak (get_progress); za ključne snimke napravi akustičku analizu (analyze_recording). \
Zatim napiši stručnu procjenu na hrvatskom za logopeda/fonetičara: 1) sažetak anamneze, 2) akustički i perceptivni nalaz s usporedbom s referentnim vrijednostima (get_reference_values), \
3) tečnost govora ako ima oznaka, 4) procjena težine i vjerojatni obrasci, 5) prijedlog terapije i kontrole. Navedi nesigurnosti. Na kraju pitaj smijem li procjenu spremiti u karton (add_clinical_note)."
        ),
        "napredak_terapije" => format!(
            "Za pacijenta „{patient}” usporedi mjere kroz vrijeme (get_progress, get_voice_profile po sesijama, upitnici iz kartona). Opiši što se promijenilo od prve procjene i od zadnje kontrole, \
koliko je promjena klinički značajna, kako su sesije (SOAP, domaće zadaće) pratile cilj terapije, i predloži plan za idući period. Odgovor na hrvatskom; na kraju ponudi spremanje u karton."
        ),
        _ => return None,
    })
}

fn text_result(v: &Value) -> Value {
    json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(v).unwrap_or_default()}], "structuredContent": v})
}

fn err_result(msg: impl std::fmt::Display) -> Value {
    json!({"content": [{"type": "text", "text": msg.to_string()}], "isError": true})
}

/// Handle one JSON-RPC message; `None` for notifications.
pub fn handle(app: &App, msg: &Value, client: &str) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg["method"].as_str().unwrap_or_default();
    id.as_ref()?;
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().filter(|v| ["2025-06-18", "2025-03-26", "2024-11-05"].contains(v)).unwrap_or(PROTOCOL_VERSION),
            "capabilities": {"tools": {"listChanged": false}, "prompts": {"listChanged": false}},
            "serverInfo": {"name": "steroidsoundboard", "title": "SterOidSoundBoard / DigiLingua", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "Clinical records of a speech-language pathology / phonetics practice (voice, fluency, speech). Find the patient with find_patients, then read the chart and measurements. Measurements are Praat-compatible. Data may be pseudonymised. Answer in the clinician's language (Croatian unless asked otherwise). Your output supports the clinician; it is not a diagnosis.",
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools()})),
        "prompts/list" => Ok(json!({"prompts": prompts()})),
        "prompts/get" => {
            let name = msg["params"]["name"].as_str().unwrap_or_default();
            let patient = msg["params"]["arguments"]["patient"].as_str().unwrap_or_default();
            match prompt_text(name, patient) {
                Some(t) => Ok(json!({"messages": [{"role": "user", "content": {"type": "text", "text": t}}]})),
                None => Err((-32602, format!("unknown prompt {name}"))),
            }
        }
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or_default();
            let args = msg["params"]["arguments"].clone();
            Ok(match call_tool(app, name, &args, client) {
                Ok(v) => text_result(&v),
                Err(e) => err_result(e),
            })
        }
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err((code, m)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}}),
    })
}

fn call_tool(app: &App, name: &str, a: &Value, client: &str) -> anyhow::Result<Value> {
    let cfg = app.mcp.lock().unwrap().clone();
    let s = |k: &str| a[k].as_str().unwrap_or_default().trim().to_string();
    // a patient the assistant may see, else an error that does not reveal anything
    let patient_ok = |pid: &str| -> anyhow::Result<()> {
        let c = app.clinic.lock().unwrap();
        match c.patients.iter().find(|p| p.id == pid) {
            Some(p) if visible(&cfg, p) => Ok(()),
            _ => anyhow::bail!("no such patient (or no AI consent recorded for this patient)"),
        }
    };
    match name {
        "find_patients" => {
            let q = fold(&s("query"));
            let c = app.clinic.lock().unwrap();
            let mut hidden = 0;
            let mut out = vec![];
            for p in &c.patients {
                let hay = fold(&format!("{} {} {} {}", p.data.name, p.data.code, p.data.diagnosis, pseudonym(&p.id)));
                if !q.split_whitespace().all(|w| hay.contains(w)) {
                    continue;
                }
                if !visible(&cfg, p) {
                    hidden += 1;
                    continue;
                }
                let sessions: Vec<&clinic::Session> = c.sessions.iter().filter(|x| x.patient_id == p.id).collect();
                let mut v = patient_json(&cfg, p);
                v.as_object_mut().map(|o| o.remove("anamnesis"));
                v["sessions"] = json!(sessions.len());
                v["recordings"] = json!(c.recordings.iter().filter(|r| r.patient_id.as_deref() == Some(&p.id)).count());
                v["last_visit"] = json!(sessions.iter().map(|x| x.start).max().map(clinic::date_str));
                out.push(v);
            }
            Ok(json!({"patients": out, "hidden_without_ai_consent": hidden, "identity": cfg.identity}))
        }
        "get_patient_chart" => {
            let pid = s("patient_id");
            patient_ok(&pid)?;
            let mut v = app.chart(&pid)?;
            let p = app.clinic.lock().unwrap().patients.iter().find(|p| p.id == pid).cloned().expect("checked");
            v["patient"] = patient_json(&cfg, &p);
            v.as_object_mut().map(|o| o.remove("age"));
            Ok(v)
        }
        "get_voice_profile" => {
            let pid = s("patient_id");
            patient_ok(&pid)?;
            let sess = s("session_id");
            app.voice_profile(&pid, (!sess.is_empty()).then_some(sess.as_str()))
        }
        "analyze_recording" => {
            let rid = s("recording_id");
            let rec = app.clinic.lock().unwrap().recordings.iter().find(|r| r.id == rid).cloned().ok_or_else(|| anyhow::anyhow!("no such recording"))?;
            match &rec.patient_id {
                Some(p) => patient_ok(p)?,
                None if cfg.consent_only => anyhow::bail!("recording without a patient (consent unknown)"),
                None => {}
            }
            let mut rep = app.analyze(&rid, a["start"].as_f64().map(|v| v as f32), a["end"].as_f64().map(|v| v as f32), None)?;
            rep.pitch.clear();
            let st = app.clinic_settings().analysis.sanitized();
            let nuclei = app.whole_report(&rid, &st).ok().map(|w| w.syllable_nuclei);
            Ok(json!({
                "recording": {"id": rec.id, "task": rec.task, "created": rec.created, "duration": rec.duration, "sample_rate": rec.sample_rate, "source": rec.source, "notes": rec.notes, "rating": rec.rating},
                "calibration_db": app.clinic_settings().calibration_db,
                "report": rep,
                "disfluencies": (!rec.annotations.is_empty()).then(|| clinic::summarize(&rec, nuclei)),
            }))
        }
        "get_progress" => {
            let pid = s("patient_id");
            patient_ok(&pid)?;
            let mut v = app.progress(&pid)?;
            let c = app.clinic.lock().unwrap();
            v["questionnaires"] = json!(c.questionnaires.iter().filter(|q| q.patient_id == pid).collect::<Vec<_>>());
            Ok(v)
        }
        "get_session" => {
            let sid = s("session_id");
            let pid = app.clinic.lock().unwrap().sessions.iter().find(|x| x.id == sid).map(|x| x.patient_id.clone()).ok_or_else(|| anyhow::anyhow!("no such session"))?;
            patient_ok(&pid)?;
            app.session_detail(&sid)
        }
        "get_reference_values" => {
            use steroid_clinic::protocol;
            use steroid_engine::analysis as an;
            Ok(json!({
                "jitter_local_pct_max": an::JITTER_MAX, "shimmer_local_pct_max": an::SHIMMER_MAX, "shimmer_db_max": an::SHIMMER_DB_MAX, "hnr_db_min": an::HNR_MIN,
                "mpt_s_min_adult": protocol::MPT_MIN_S, "sz_ratio_max": protocol::SZ_MAX, "dsi_normal_about": protocol::DSI_NORMAL,
                "cpps_note": "CPPS (AVQI settings): about 14 dB and above is typical for normal sustained vowels; lower = more dysphonic (microphone and task dependent).",
                "note": "Indicative adult limits (MDVP / literature); they depend on task, microphone and room.",
                "tasks": protocol::TASKS, "protocols": protocol::PROTOCOLS,
            }))
        }
        "add_clinical_note" => {
            if !cfg.allow_write {
                anyhow::bail!("writing notes is switched off in the app's MCP settings");
            }
            let pid = s("patient_id");
            patient_ok(&pid)?;
            let who = format!("mcp:{}", client.chars().take(60).collect::<String>());
            let n = app.add_note(&pid, &s("title"), &s("text"), &who)?;
            app.auth.audit(&who, "mcp", "POST", &format!("/api/patients/{pid}/notes"));
            Ok(json!({"saved": true, "note_id": n.id, "created": n.created}))
        }
        _ => anyhow::bail!("unknown tool {name}"),
    }
}

/// stdio transport: newline-delimited JSON-RPC on stdin/stdout. Forwards to a
/// running app on `port` (so its state stays the single source of truth);
/// otherwise serves from the data directory.
pub fn run_stdio(app: std::sync::Arc<App>, port: u16) -> anyhow::Result<()> {
    use std::io::{BufRead, Write};
    let token = app.mcp.lock().unwrap().token.clone();
    let url = format!("http://127.0.0.1:{port}/mcp");
    let remote = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(2)).try_proxy_from_env(false).build();
    let running = remote.get(&format!("http://127.0.0.1:{port}/api/status")).call().is_ok();
    eprintln!(
        "steroidsoundboard MCP (stdio) — {}",
        if running { format!("forwarding to the running app on port {port}") } else { format!("data dir {}", app.paths.data.display()) }
    );
    if !running && !app.mcp.lock().unwrap().enabled {
        eprintln!("note: MCP is not enabled in the app's settings (DigiLingua ⚙ → AI asistenti / MCP); serving anyway because it was started locally.");
    }
    let long = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(600)).try_proxy_from_env(false).build();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    let mut client = String::from("stdio");
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}}))?;
                out.flush()?;
                continue;
            }
        };
        if msg["method"] == "initialize" {
            if let Some(n) = msg["params"]["clientInfo"]["name"].as_str() {
                client = n.to_string();
            }
        }
        let reply = if running {
            match long.post(&url).set("authorization", &format!("Bearer {token}")).set("x-mcp-client", &client).send_string(&line) {
                Ok(r) if r.status() == 202 => None,
                Ok(r) => r.into_string().ok().and_then(|s| serde_json::from_str(&s).ok()),
                Err(e) => msg.get("id").map(|id| json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": format!("app not reachable: {e}")}})),
            }
        } else {
            handle(&app, &msg, &client)
        };
        if let Some(r) = reply {
            writeln!(out, "{r}")?;
            out.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_pseudonyms() {
        let t = new_token();
        assert_eq!(t.len(), 48);
        assert_ne!(t, new_token());
        let cfg = McpConfig { token: t.clone(), ..Default::default() };
        assert!(token_ok(&cfg, &t));
        assert!(!token_ok(&cfg, "x"));
        assert!(!token_ok(&McpConfig::default(), ""));
        assert_eq!(pseudonym("p1"), pseudonym("p1"));
        assert_ne!(pseudonym("p1"), pseudonym("p2"));
        assert_eq!(fold("Ćurić Đuro"), "curic duro");
    }

    #[test]
    fn pseudonymised_patient_has_no_identifiers() {
        let p = Patient {
            id: "p1".into(),
            data: clinic::PatientIn { name: "Ivan Horvat".into(), code: "MBO-1".into(), birth: "1980-01-02".into(), ai_consent: true, ..Default::default() },
            created: 0,
            updated: 0,
        };
        let v = patient_json(&McpConfig::default(), &p).to_string();
        assert!(!v.contains("Horvat") && !v.contains("MBO-1") && !v.contains("1980-01-02"));
        let v = patient_json(&McpConfig { identity: "full".into(), ..Default::default() }, &p).to_string();
        assert!(v.contains("Horvat"));
        assert!(!visible(&McpConfig::default(), &Patient { data: clinic::PatientIn { ai_consent: false, ..p.data.clone() }, ..p.clone() }));
    }
}
