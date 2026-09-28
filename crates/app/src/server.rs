use crate::clinic::{self, Patient, PatientIn};
use crate::state::App;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use steroid_engine::*;
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(RustEmbed)]
#[folder = "../../web/"]
struct Assets;

type S = State<Arc<App>>;

fn err(e: impl std::fmt::Display) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response()
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/devices", get(|| async { Json(tokio::task::spawn_blocking(list_devices).await.unwrap_or_default()) }))
        .route("/api/node-types", get(|| async { Json(NodeKind::ALL.iter().map(|k| k.info()).collect::<Vec<_>>()) }))
        .route("/api/audio/start", post(audio_start))
        .route("/api/audio/stop", post(audio_stop))
        .route("/api/board", get(board))
        .route("/api/nodes", post(add_node))
        .route("/api/nodes/{id}", delete(remove_node))
        .route("/api/connections", post(connect).delete(disconnect))
        .route("/api/board/replace", post(replace_board))
        .route("/api/templates", get(templates))
        .route("/api/templates/{id}", post(load_template))
        .route("/api/presets", get(list_presets).post(save_preset))
        .route("/api/presets/{name}", delete(delete_preset))
        .route("/api/presets/{name}/load", post(load_preset))
        .route("/api/patients", get(list_patients).post(add_patient))
        .route("/api/patients/{id}", put(update_patient).delete(delete_patient))
        .route("/api/sessions", get(list_sessions).post(start_session))
        .route("/api/sessions/{id}", put(update_session))
        .route("/api/sessions/{id}/stop", post(stop_session))
        .route("/api/record/start", post(record_start))
        .route("/api/record/stop", post(record_stop))
        .route("/api/recordings", get(list_recordings))
        .route("/api/recordings/{id}", put(update_recording).delete(delete_recording))
        .route("/api/recordings/{id}/wav", get(recording_wav))
        .route("/api/recordings/{id}/analyze", post(analyze))
        .route("/api/recordings/{id}/tracks", post(tracks))
        .route("/api/recordings/{id}/textgrid", get(textgrid))
        .route("/api/recordings/{id}/summary", get(annotation_summary))
        .route("/api/patients/{id}/progress", get(progress))
        .route("/api/sessions/{id}/detail", get(session_detail))
        .route("/api/sessions/{id}/apply", post(session_apply))
        .route("/api/clinic/settings", get(get_clinic_settings).put(put_clinic_settings))
        .route("/api/clinic/annotation-kinds", get(annotation_kinds))
        .route("/api/clinic/presets", get(list_clinical_presets).post(save_clinical_preset))
        .route("/api/clinic/presets/{id}", delete(delete_clinical_preset))
        .route("/api/clinic/presets/{id}/load", post(load_clinical_preset))
        .route("/api/ai/config", get(ai_config).put(set_ai_config))
        .route("/api/ai/test", post(ai_test))
        .route("/api/recordings/{id}/ai/preview", post(ai_preview).layer(DefaultBodyLimit::max(12 << 20)))
        .route("/api/recordings/{id}/ai", get(ai_list).post(ai_run).layer(DefaultBodyLimit::max(12 << 20)))
        .route("/api/ai/tasks", get(ai_tasks))
        .route("/api/ai/tasks/{id}", get(ai_task))
        .route("/api/ai/tasks/{id}/cancel", post(ai_cancel))
        .route("/api/ai/ollama", get(ollama_status))
        .route("/api/ai/ollama/pull", post(ollama_pull))
        .route("/api/ai/ollama/delete", post(ollama_delete))
        .route("/api/ai-reports/{id}", delete(ai_delete))
        .route("/ws", get(ws))
        .fallback(static_file)
        .with_state(app)
}

async fn status(State(app): S) -> Json<Value> {
    let i = app.inner.lock().unwrap();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "running": i.engine.as_ref().map(|e| e.info.clone()),
        "config": i.audio_cfg,
        "error": i.last_error,
        "mute": app.stats.mute.load(Ordering::Relaxed),
        "preset": *app.current_preset.lock().unwrap(),
        "recording": i.recording.is_some(),
    }))
}

async fn board(State(app): S) -> Json<Board> {
    Json(app.inner.lock().unwrap().board.clone())
}

async fn audio_start(State(app): S, Json(cfg): Json<AudioConfig>) -> Response {
    match tokio::task::spawn_blocking(move || app.start_audio(Some(cfg))).await {
        Ok(Ok(info)) => Json(info).into_response(),
        Ok(Err(e)) => err(e),
        Err(e) => err(e),
    }
}

async fn audio_stop(State(app): S) -> StatusCode {
    let _ = tokio::task::spawn_blocking(move || app.stop_audio()).await;
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct AddNode {
    kind: NodeKind,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    role: Option<String>,
}

async fn add_node(State(app): S, Json(b): Json<AddNode>) -> Response {
    match app.add_node(b.kind, b.x, b.y, b.role) {
        Ok(n) => Json(n).into_response(),
        Err(e) => err(e),
    }
}

async fn remove_node(State(app): S, Path(id): Path<String>) -> Response {
    match app.remove_node(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(e),
    }
}

async fn connect(State(app): S, Json(c): Json<Connection>) -> Response {
    match app.connect(c) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(e),
    }
}

async fn disconnect(State(app): S, Json(c): Json<Connection>) -> Response {
    match app.disconnect(&c) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(e),
    }
}

fn done(r: anyhow::Result<()>) -> Response {
    match r {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(e),
    }
}

// ------------------------------------------------------------- boards, templates, presets
async fn replace_board(State(app): S, Json(b): Json<Board>) -> Response {
    done(app.set_board(b))
}

async fn templates() -> Json<Value> {
    Json(json!(steroid_engine::templates::ALL
        .iter()
        .map(|t| json!({"id": t.id, "name": t.name, "description": t.description}))
        .collect::<Vec<_>>()))
}

async fn load_template(State(app): S, Path(id): Path<String>) -> Response {
    match steroid_engine::templates::build(&id) {
        Some(b) => done(app.set_board(b)),
        None => err("no such template"),
    }
}

async fn list_presets(State(app): S) -> Json<Vec<String>> {
    Json(app.list_presets())
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

async fn save_preset(State(app): S, Json(n): Json<Named>) -> Response {
    match app.save_preset(&n.name) {
        Ok(name) => Json(json!({"name": name})).into_response(),
        Err(e) => err(e),
    }
}

async fn load_preset(State(app): S, Path(name): Path<String>) -> Response {
    done(app.load_preset(&name))
}

async fn delete_preset(State(app): S, Path(name): Path<String>) -> Response {
    done(app.delete_preset(&name))
}

// ------------------------------------------------------------- DigiLingua: patients, sessions
async fn list_patients(State(app): S) -> Json<Vec<Patient>> {
    let mut v = app.clinic.lock().unwrap().patients.clone();
    v.sort_by_key(|p| p.data.name.to_lowercase());
    Json(v)
}

fn check_patient(p: &PatientIn) -> Result<(), &'static str> {
    if p.name.trim().is_empty() {
        return Err("ime je obavezno");
    }
    if p.text_fields().iter().any(|(v, max)| v.len() > *max) {
        return Err("predugačak unos");
    }
    Ok(())
}

async fn add_patient(State(app): S, Json(p): Json<PatientIn>) -> Response {
    if let Err(e) = check_patient(&p) {
        return err(e);
    }
    let now = clinic::now_ms();
    let pat = Patient { id: clinic::new_id("p"), data: p, created: now, updated: now };
    let mut c = app.clinic.lock().unwrap();
    c.patients.push(pat.clone());
    match app.save_clinic(&c) {
        Ok(()) => Json(pat).into_response(),
        Err(e) => err(e),
    }
}

async fn update_patient(State(app): S, Path(id): Path<String>, Json(p): Json<PatientIn>) -> Response {
    if let Err(e) = check_patient(&p) {
        return err(e);
    }
    let mut c = app.clinic.lock().unwrap();
    let Some(x) = c.patients.iter_mut().find(|x| x.id == id) else { return err("no such patient") };
    x.data = p;
    x.updated = clinic::now_ms();
    let out = x.clone();
    match app.save_clinic(&c) {
        Ok(()) => Json(out).into_response(),
        Err(e) => err(e),
    }
}

async fn delete_patient(State(app): S, Path(id): Path<String>) -> Response {
    done(app.delete_patient(&id))
}

#[derive(Deserialize)]
struct ByPatient {
    patient: Option<String>,
}

async fn list_sessions(State(app): S, Query(q): Query<ByPatient>) -> Json<Value> {
    let c = app.clinic.lock().unwrap();
    let mut v: Vec<Value> = c
        .sessions
        .iter()
        .filter(|s| q.patient.is_none() || q.patient.as_ref() == Some(&s.patient_id))
        .map(|s| {
            let recs = c.recordings.iter().filter(|r| r.session_id.as_deref() == Some(&s.id)).count();
            json!({"id": s.id, "patient_id": s.patient_id, "start": s.start, "end": s.end, "notes": s.notes, "preset": s.preset, "recordings": recs})
        })
        .collect();
    v.reverse();
    Json(json!(v))
}

async fn session_detail(State(app): S, Path(id): Path<String>) -> Response {
    let c = app.clinic.lock().unwrap();
    let Some(s) = c.sessions.iter().find(|s| s.id == id) else { return err("no such session") };
    let nodes: Vec<Value> = s
        .board
        .as_ref()
        .map(|b| {
            b.nodes
                .iter()
                .filter(|n| n.role.is_some())
                .map(|n| json!({"role": n.role, "kind": n.kind, "bypass": n.bypass, "params": n.params}))
                .collect()
        })
        .unwrap_or_default();
    let recs: Vec<&clinic::Recording> = c.recordings.iter().filter(|r| r.session_id.as_deref() == Some(&s.id)).collect();
    Json(json!({"id": s.id, "patient_id": s.patient_id, "start": s.start, "end": s.end, "notes": s.notes,
                "preset": s.preset, "has_board": s.board.is_some(), "nodes": nodes, "recordings": recs}))
    .into_response()
}

async fn session_apply(State(app): S, Path(id): Path<String>) -> Response {
    let (board, preset) = {
        let c = app.clinic.lock().unwrap();
        let Some(s) = c.sessions.iter().find(|s| s.id == id) else { return err("no such session") };
        let Some(b) = s.board.clone() else { return err("sesija nema spremljene postavke") };
        (b, s.preset.clone())
    };
    done(app.set_board_named(board, preset))
}

#[derive(Deserialize)]
struct StartSession {
    patient_id: String,
}

async fn start_session(State(app): S, Json(b): Json<StartSession>) -> Response {
    match app.start_session(&b.patient_id) {
        Ok(s) => Json(json!({"id": s.id, "patient_id": s.patient_id, "start": s.start, "end": s.end, "notes": s.notes})).into_response(),
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
struct Notes {
    #[serde(default)]
    notes: Option<String>,
}

fn session_edit(app: &App, id: &str, notes: Option<String>, stop: bool) -> anyhow::Result<()> {
    let mut c = app.clinic.lock().unwrap();
    let s = c.sessions.iter_mut().find(|s| s.id == id).ok_or_else(|| anyhow::anyhow!("no such session"))?;
    if let Some(n) = notes {
        anyhow::ensure!(n.len() <= 100_000, "notes too long");
        s.notes = n;
    }
    if stop && s.end.is_none() {
        s.end = Some(clinic::now_ms());
    }
    app.save_clinic(&c)
}

async fn update_session(State(app): S, Path(id): Path<String>, Json(n): Json<Notes>) -> Response {
    done(session_edit(&app, &id, n.notes, false))
}

async fn stop_session(State(app): S, Path(id): Path<String>, Json(n): Json<Notes>) -> Response {
    done(session_edit(&app, &id, n.notes, true))
}

// ------------------------------------------------------------- recording + analysis
#[derive(Deserialize)]
struct RecStart {
    #[serde(default)]
    patient_id: Option<String>,
    #[serde(default)]
    source: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    task: String,
}

async fn record_start(State(app): S, Json(b): Json<RecStart>) -> Response {
    let label: String = b.label.chars().take(200).collect();
    let task: String = b.task.chars().take(200).collect();
    match tokio::task::spawn_blocking(move || app.start_recording(b.patient_id, &b.source, label, task)).await {
        Ok(r) => done(r),
        Err(e) => err(e),
    }
}

async fn record_stop(State(app): S) -> Response {
    match tokio::task::spawn_blocking(move || app.stop_recording()).await {
        Ok(Ok(r)) => Json(r).into_response(),
        Ok(Err(e)) => err(e),
        Err(e) => err(e),
    }
}

async fn list_recordings(State(app): S, Query(q): Query<ByPatient>) -> Json<Value> {
    let c = app.clinic.lock().unwrap();
    let mut v: Vec<&clinic::Recording> =
        c.recordings.iter().filter(|r| q.patient.is_none() || q.patient == r.patient_id).collect();
    v.sort_by_key(|r| std::cmp::Reverse(r.created));
    Json(json!(v))
}

#[derive(Deserialize)]
struct RecordingEdit {
    label: Option<String>,
    task: Option<String>,
    notes: Option<String>,
    annotations: Option<Vec<clinic::Annotation>>,
    /// Manual syllable count; 0 clears it.
    syllables: Option<u32>,
}

fn check_annotations(a: &[clinic::Annotation], duration: f32) -> Result<(), String> {
    if a.len() > 5000 {
        return Err("previše oznaka".into());
    }
    for x in a {
        if !(x.start.is_finite() && x.end.is_finite() && x.start >= 0.0 && x.end > x.start && x.end <= duration + 0.5) {
            return Err(format!("neispravno vrijeme oznake {:.2}–{:.2} s", x.start, x.end));
        }
        if !clinic::ANNOTATION_KINDS.iter().any(|k| k.0 == x.kind) {
            return Err(format!("nepoznata vrsta oznake: {}", x.kind));
        }
        if x.text.len() > 500 || x.id.is_empty() || x.id.len() > 40 || !x.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("neispravna oznaka".into());
        }
    }
    Ok(())
}

async fn update_recording(State(app): S, Path(id): Path<String>, Json(e): Json<RecordingEdit>) -> Response {
    let mut c = app.clinic.lock().unwrap();
    let Some(r) = c.recordings.iter_mut().find(|r| r.id == id) else { return err("no such recording") };
    if let Some(v) = e.label {
        r.label = v.chars().take(200).collect();
    }
    if let Some(v) = e.task {
        r.task = v.chars().take(200).collect();
    }
    if let Some(v) = e.notes {
        r.notes = v.chars().take(20_000).collect();
    }
    if let Some(a) = e.annotations {
        if let Err(m) = check_annotations(&a, r.duration) {
            return err(m);
        }
        r.annotations = a;
    }
    if let Some(n) = e.syllables {
        r.syllables = (n > 0).then_some(n.min(1_000_000));
    }
    done(app.save_clinic(&c))
}

async fn textgrid(State(app): S, Path(id): Path<String>) -> Response {
    let c = app.clinic.lock().unwrap();
    let Some(r) = c.recordings.iter().find(|r| r.id == id) else { return StatusCode::NOT_FOUND.into_response() };
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}.TextGrid\"", r.id)),
        ],
        clinic::textgrid(r),
    )
        .into_response()
}

async fn annotation_summary(State(app): S, Path(id): Path<String>) -> Response {
    let r = tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
        let rec = app.clinic.lock().unwrap().recordings.iter().find(|r| r.id == id).cloned().ok_or_else(|| anyhow::anyhow!("no such recording"))?;
        let st = app.clinic_settings().analysis;
        let nuclei = app.whole_report(&id, &st.sanitized()).ok().map(|w| w.syllable_nuclei);
        Ok(json!(clinic::summarize(&rec, nuclei)))
    })
    .await;
    match r {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => err(e),
        Err(e) => err(e),
    }
}

async fn annotation_kinds() -> Json<Value> {
    Json(json!(clinic::ANNOTATION_KINDS.iter().map(|(id, label, sld)| json!({"id": id, "label": label, "sld": sld})).collect::<Vec<_>>()))
}

async fn progress(State(app): S, Path(id): Path<String>) -> Response {
    match tokio::task::spawn_blocking(move || app.progress(&id)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => err(e),
        Err(e) => err(e),
    }
}

async fn get_clinic_settings(State(app): S) -> Json<clinic::ClinicSettings> {
    Json(app.clinic_settings())
}

async fn put_clinic_settings(State(app): S, Json(mut b): Json<clinic::ClinicSettings>) -> Response {
    b.clinic_name = b.clinic_name.chars().take(200).collect();
    b.clinician = b.clinician.chars().take(200).collect();
    b.analysis = b.analysis.sanitized();
    b.calibration_db = b.calibration_db.filter(|v| v.is_finite() && v.abs() < 200.0);
    let mut c = app.clinic.lock().unwrap();
    c.settings = b;
    match app.save_clinic(&c) {
        Ok(()) => Json(c.settings.clone()).into_response(),
        Err(e) => err(e),
    }
}

async fn list_clinical_presets(State(app): S) -> Json<Vec<Value>> {
    Json(app.clinical_presets())
}

#[derive(Deserialize)]
struct PresetIn {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
}

async fn save_clinical_preset(State(app): S, Json(b): Json<PresetIn>) -> Response {
    match app.save_clinical_preset(&b.name, &b.description, &b.category) {
        Ok(id) => Json(json!({"id": format!("c:{id}")})).into_response(),
        Err(e) => err(e),
    }
}

async fn load_clinical_preset(State(app): S, Path(id): Path<String>) -> Response {
    done(app.load_clinical_preset(&id))
}

async fn delete_clinical_preset(State(app): S, Path(id): Path<String>) -> Response {
    done(app.delete_clinical_preset(&id))
}

async fn delete_recording(State(app): S, Path(id): Path<String>) -> Response {
    done(app.delete_recording(&id))
}

async fn recording_wav(State(app): S, Path(id): Path<String>) -> Response {
    // Only ids known to the store are served — never a client-supplied path.
    let (file_name, path) = {
        let c = app.clinic.lock().unwrap();
        let Some(r) = c.recordings.iter().find(|r| r.id == id) else { return StatusCode::NOT_FOUND.into_response() };
        let who = r
            .patient_id
            .as_ref()
            .and_then(|p| c.patients.iter().find(|x| &x.id == p))
            .map(|p| p.data.name.clone())
            .unwrap_or_else(|| "snimka".into());
        let who: String = who.chars().map(|ch| if ch.is_alphanumeric() { ch } else { '_' }).collect();
        (format!("{who}_{}.wav", r.id), clinic::wav_path(&app.paths.recordings, &r.id))
    };
    match tokio::fs::read(&path).await {
        Ok(data) => (
            [
                (header::CONTENT_TYPE, "audio/wav".to_string()),
                (header::CONTENT_DISPOSITION, format!("inline; filename=\"{file_name}\"")),
            ],
            data,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize, Default)]
struct Range {
    start: Option<f32>,
    end: Option<f32>,
    settings: Option<steroid_engine::analysis::AnalysisSettings>,
}

async fn analyze(State(app): S, Path(id): Path<String>, body: Option<Json<Range>>) -> Response {
    let r = body.map(|b| b.0).unwrap_or_default();
    match tokio::task::spawn_blocking(move || app.analyze(&id, r.start, r.end, r.settings)).await {
        Ok(Ok(rep)) => Json(rep).into_response(),
        Ok(Err(e)) => err(e),
        Err(e) => err(e),
    }
}

async fn tracks(State(app): S, Path(id): Path<String>, body: Option<Json<Range>>) -> Response {
    let r = body.map(|b| b.0).unwrap_or_default();
    match tokio::task::spawn_blocking(move || app.tracks(&id, r.settings)).await {
        Ok(Ok(t)) => Json(&*t).into_response(),
        Ok(Err(e)) => err(e),
        Err(e) => err(e),
    }
}

// ------------------------------------------------------------- AI opinion
async fn ai_config(State(app): S) -> Json<Value> {
    Json(app.ai.lock().unwrap().public())
}

#[derive(Deserialize)]
struct AiConfigIn {
    provider: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    model: String,
    /// New key; omitted or empty keeps the stored one.
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    clear_key: bool,
    #[serde(default = "yes")]
    send_image: bool,
    #[serde(default)]
    ollama_ctx: u32,
}
fn yes() -> bool {
    true
}

async fn set_ai_config(State(app): S, Json(b): Json<AiConfigIn>) -> Response {
    if !matches!(b.provider.as_str(), "anthropic" | "openai" | "ollama") {
        return err("nepoznat AI servis");
    }
    let base = b.base_url.trim();
    if !base.is_empty() && !(base.starts_with("https://") || base.starts_with("http://")) {
        return err("adresa mora počinjati s http:// ili https://");
    }
    let mut cfg = app.ai.lock().unwrap();
    let provider_changed = cfg.provider != b.provider;
    cfg.provider = b.provider;
    cfg.base_url = base.chars().take(500).collect();
    cfg.model = b.model.trim().chars().take(200).collect();
    cfg.send_image = b.send_image;
    cfg.ollama_ctx = if b.ollama_ctx == 0 { 0 } else { b.ollama_ctx.clamp(2048, 131_072) };
    if b.clear_key || provider_changed {
        cfg.api_key.clear(); // a key never silently follows a switch to another service
    }
    if let Some(k) = b.api_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        cfg.api_key = k.chars().take(1000).collect();
    }
    match cfg.save(&app.paths.ai) {
        Ok(()) => Json(cfg.public()).into_response(),
        Err(e) => err(e),
    }
}

async fn ai_test(State(app): S) -> Response {
    let cfg = app.ai.lock().unwrap().clone();
    match tokio::task::spawn_blocking(move || crate::ai::test(&cfg)).await {
        Ok(Ok(a)) => Json(json!({"ok": true, "model": a.model, "text": a.text.chars().take(200).collect::<String>()})).into_response(),
        Ok(Err(e)) => err(e),
        Err(e) => err(e),
    }
}

#[derive(Deserialize, Default)]
struct AiReq {
    start: Option<f32>,
    end: Option<f32>,
    #[serde(default)]
    question: String,
    /// Sonagram as a JPEG/PNG data URL.
    #[serde(default)]
    image: Option<String>,
    /// Analysis settings used in the editor (default: clinic settings).
    #[serde(default)]
    settings: Option<steroid_engine::analysis::AnalysisSettings>,
}

async fn ai_preview(State(app): S, Path(id): Path<String>, Json(b): Json<AiReq>) -> Response {
    let q: String = b.question.chars().take(4000).collect();
    let r = tokio::task::spawn_blocking(move || app.ai_job(&id, b.start, b.end, b.settings, &q, b.image.as_deref())).await;
    match r {
        Ok(Ok(j)) => Json(json!({
            "provider": j.cfg.provider_name(),
            "url": j.cfg.base(),
            "model": j.cfg.model,
            "image": j.image.is_some(),
            "system": j.system,
            "user": j.user,
        }))
        .into_response(),
        Ok(Err(e)) => err(e),
        Err(e) => err(e),
    }
}

/// Starts the AI opinion in the background and returns its task id; the log,
/// streamed text and result arrive as `ai_task` WebSocket events.
async fn ai_run(State(app): S, Path(id): Path<String>, Json(b): Json<AiReq>) -> Response {
    let q: String = b.question.chars().take(4000).collect();
    let r = tokio::task::spawn_blocking(move || {
        let job = app.ai_job(&id, b.start, b.end, b.settings, &q, b.image.as_deref())?;
        app.ai_start(job, q)
    })
    .await;
    match r {
        Ok(Ok(task)) => Json(json!({"task": task})).into_response(),
        Ok(Err(e)) => err(e),
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
struct ByRecording {
    recording: Option<String>,
}

/// A task as JSON plus its age, so browsers need not trust the server's clock.
fn task_json(t: &crate::state::AiTask) -> Value {
    let mut v = json!(t);
    v["age_ms"] = json!(clinic::now_ms().saturating_sub(t.started));
    v
}

async fn ai_tasks(State(app): S, Query(q): Query<ByRecording>) -> Json<Value> {
    let tasks = app.ai_tasks.lock().unwrap();
    Json(json!(tasks.iter().filter(|t| q.recording.is_none() || t.recording_id == q.recording).map(task_json).collect::<Vec<_>>()))
}

async fn ai_task(State(app): S, Path(id): Path<String>) -> Response {
    let tasks = app.ai_tasks.lock().unwrap();
    match tasks.iter().find(|t| t.id == id) {
        Some(t) => Json(task_json(t)).into_response(),
        None => err("nema tog zadatka"),
    }
}

async fn ai_cancel(State(app): S, Path(id): Path<String>) -> Response {
    done(app.cancel_task(&id))
}

#[derive(Deserialize)]
struct OllamaUrl {
    url: Option<String>,
}

/// Ollama status for the settings panel; `url` lets the panel check an
/// address before it is saved.
async fn ollama_status(State(app): S, Query(q): Query<OllamaUrl>) -> Response {
    let url = match q.url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()) {
        Some(u) if u.starts_with("http://") || u.starts_with("https://") => u,
        Some(_) => return err("adresa mora počinjati s http:// ili https://"),
        None => {
            let c = app.ai.lock().unwrap();
            if c.provider == "ollama" { c.base_url.clone() } else { String::new() }
        }
    };
    let root = crate::ollama::root(&url);
    Json(tokio::task::spawn_blocking(move || crate::ollama::status(&root)).await.unwrap_or(Value::Null)).into_response()
}

#[derive(Deserialize)]
struct ModelName {
    model: String,
}

fn check_model_name(m: &str) -> Result<String, &'static str> {
    let m = m.trim();
    if m.is_empty() || m.len() > 200 || !m.chars().all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c)) {
        return Err("neispravan naziv modela");
    }
    Ok(m.to_string())
}

async fn ollama_pull(State(app): S, Json(b): Json<ModelName>) -> Response {
    let model = match check_model_name(&b.model) {
        Ok(m) => m,
        Err(e) => return err(e),
    };
    match app.ollama_pull(model) {
        Ok(task) => Json(json!({"task": task})).into_response(),
        Err(e) => err(e),
    }
}

async fn ollama_delete(State(app): S, Json(b): Json<ModelName>) -> Response {
    let model = match check_model_name(&b.model) {
        Ok(m) => m,
        Err(e) => return err(e),
    };
    let root = {
        let c = app.ai.lock().unwrap();
        crate::ollama::root(if c.provider == "ollama" { &c.base_url } else { "" })
    };
    match tokio::task::spawn_blocking(move || crate::ollama::delete(&root, &model)).await {
        Ok(r) => done(r),
        Err(e) => err(e),
    }
}

async fn ai_list(State(app): S, Path(id): Path<String>) -> Json<Value> {
    let c = app.clinic.lock().unwrap();
    let mut v: Vec<&clinic::AiReport> = c.ai_reports.iter().filter(|a| a.recording_id == id).collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.created));
    Json(json!(v))
}

async fn ai_delete(State(app): S, Path(id): Path<String>) -> Response {
    let mut c = app.clinic.lock().unwrap();
    let before = c.ai_reports.len();
    c.ai_reports.retain(|a| a.id != id);
    if c.ai_reports.len() == before {
        return err("no such AI report");
    }
    done(app.save_clinic(&c))
}

// ------------------------------------------------------------- WebSocket
static CLIENT_ID: AtomicU64 = AtomicU64::new(1);

async fn ws(State(app): S, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(move |s| client(app, s))
}

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ClientMsg {
    Param { node: String, param: String, value: f32 },
    Bypass { node: String, on: bool },
    Move { node: String, x: f32, y: f32 },
    Mute { on: bool },
    Tap { src: u32 },
    /// Live sonagram subscription; `win` = analysis window in seconds.
    Live { on: bool, #[serde(default)] win: Option<f32> },
}

async fn client(app: Arc<App>, socket: WebSocket) {
    let me = CLIENT_ID.fetch_add(1, Ordering::Relaxed);
    let (mut tx, mut rx) = socket.split();
    let mut events = app.events.subscribe();
    let hello = json!({"t": "hello", "client": me}).to_string();
    if tx.send(Message::Text(hello.into())).await.is_err() {
        return;
    }
    let send_task = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(m) => {
                    if tx.send(Message::Text(m.into())).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
    let mut live_on = false;
    while let Some(Ok(msg)) = rx.next().await {
        let Message::Text(t) = msg else { continue };
        let Ok(m) = serde_json::from_str::<ClientMsg>(t.as_str()) else { continue };
        match m {
            ClientMsg::Param { node, param, value } => {
                if let Ok(v) = app.set_param(&node, &param, value) {
                    app.emit(json!({"t": "param", "node": node, "param": param, "value": v, "src": me}));
                }
            }
            ClientMsg::Bypass { node, on } => {
                if app.set_bypass(&node, on).is_ok() {
                    app.emit(json!({"t": "bypass", "node": node, "on": on, "src": me}));
                }
            }
            ClientMsg::Move { node, x, y } => {
                app.move_node(&node, x, y);
                app.emit(json!({"t": "move", "node": node, "x": x, "y": y, "src": me}));
            }
            ClientMsg::Mute { on } => app.set_mute(on),
            ClientMsg::Tap { src } => app.set_tap(src),
            ClientMsg::Live { on, win } => {
                if let Some(w) = win.filter(|w| w.is_finite()) {
                    *app.live_window.lock().unwrap() = w.clamp(0.002, 0.05);
                }
                if on != live_on {
                    live_on = on;
                    if on {
                        app.live_subs.fetch_add(1, Ordering::Relaxed);
                    } else {
                        app.live_subs.fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
    if live_on {
        app.live_subs.fetch_sub(1, Ordering::Relaxed);
    }
    send_task.abort();
}

async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let (file, path) = match Assets::get(path) {
        Some(f) => (f, path),
        None => match Assets::get("index.html") {
            Some(f) => (f, "index.html"),
            None => return StatusCode::NOT_FOUND.into_response(),
        },
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    ([(header::CONTENT_TYPE, mime.as_ref().to_string())], file.data).into_response()
}
