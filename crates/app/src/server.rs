use crate::state::App;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
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
}

async fn add_node(State(app): S, Json(b): Json<AddNode>) -> Response {
    match app.add_node(b.kind, b.x, b.y) {
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
