use crate::clinic::{self, Patient, PatientIn};
use crate::state::App;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
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
        return Err("name is required");
    }
    if p.name.len() > 200 || p.code.len() > 100 || p.birth.len() > 40 || p.notes.len() > 20_000 {
        return Err("field too long");
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
        .map(|s| json!({"id": s.id, "patient_id": s.patient_id, "start": s.start, "end": s.end, "notes": s.notes}))
        .collect();
    v.reverse();
    Json(json!(v))
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
}

async fn record_start(State(app): S, Json(b): Json<RecStart>) -> Response {
    let label: String = b.label.chars().take(200).collect();
    match tokio::task::spawn_blocking(move || app.start_recording(b.patient_id, &b.source, label)).await {
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

async fn update_recording(State(app): S, Path(id): Path<String>, Json(n): Json<Named>) -> Response {
    let mut c = app.clinic.lock().unwrap();
    let Some(r) = c.recordings.iter_mut().find(|r| r.id == id) else { return err("no such recording") };
    r.label = n.name.chars().take(200).collect();
    done(app.save_clinic(&c))
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
}

async fn analyze(State(app): S, Path(id): Path<String>, body: Option<Json<Range>>) -> Response {
    let r = body.map(|b| b.0).unwrap_or_default();
    match tokio::task::spawn_blocking(move || app.analyze(&id, r.start, r.end)).await {
        Ok(Ok(rep)) => Json(rep).into_response(),
        Ok(Err(e)) => err(e),
        Err(e) => err(e),
    }
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
        }
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
