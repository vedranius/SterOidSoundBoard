use anyhow::{anyhow, Result};
use steroid_engine::*;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub struct Paths {
    pub data: PathBuf,
    pub board: PathBuf,
    pub audio: PathBuf,
}

impl Paths {
    pub fn new() -> Self {
        let data = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("SterOidSoundBoard");
        let _ = std::fs::create_dir_all(data.join("boards"));
        Paths { board: data.join("boards").join("default.json"), audio: data.join("audio.json"), data }
    }
}

pub struct Inner {
    pub board: Board,
    pub live: HashMap<String, LiveNode>,
    pub engine: Option<AudioEngine>,
    pub audio_cfg: AudioConfig,
    pub last_error: Option<String>,
}

pub struct App {
    pub inner: Mutex<Inner>,
    pub stats: Arc<EngineStats>,
    pub events: broadcast::Sender<String>,
    pub paths: Paths,
    pub dirty: AtomicBool,
}

fn load_json<T: serde::de::DeserializeOwned + Default>(p: &PathBuf) -> T {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

impl App {
    pub fn new() -> Arc<Self> {
        let paths = Paths::new();
        let board: Board = load_json(&paths.board);
        let audio_cfg: AudioConfig = load_json(&paths.audio);
        let live = board.nodes.iter().map(|n| (n.id.clone(), LiveNode::from_desc(n))).collect();
        let (events, _) = broadcast::channel(256);
        Arc::new(App {
            inner: Mutex::new(Inner { board, live, engine: None, audio_cfg, last_error: None }),
            stats: Arc::new(EngineStats::default()),
            events,
            paths,
            dirty: AtomicBool::new(false),
        })
    }

    pub fn emit(&self, v: serde_json::Value) {
        let _ = self.events.send(v.to_string());
    }

    /// Recompile the graph and hand it to the audio thread.
    fn rebuild(inner: &mut Inner) -> Result<()> {
        if let Some(engine) = inner.engine.as_mut() {
            let s = Schedule::build(&inner.board, &inner.live, engine.info.sample_rate as f32).map_err(|e| anyhow!(e))?;
            engine.send_schedule(s)?;
        }
        Ok(())
    }

    fn structural_change(&self, inner: &mut Inner) -> Result<()> {
        Self::rebuild(inner)?;
        self.dirty.store(true, Relaxed);
        self.emit(json!({"t": "board", "board": inner.board}));
        Ok(())
    }

    pub fn start_audio(&self, cfg: Option<AudioConfig>) -> Result<RunningInfo> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(c) = cfg {
            inner.audio_cfg = c;
        }
        inner.engine = None; // stops previous streams
        match AudioEngine::start(&inner.audio_cfg, self.stats.clone()) {
            Ok(e) => {
                let info = e.info.clone();
                inner.engine = Some(e);
                inner.last_error = None;
                Self::rebuild(&mut inner)?;
                let _ = std::fs::write(&self.paths.audio, serde_json::to_string_pretty(&inner.audio_cfg)?);
                self.emit(json!({"t": "audio", "running": info}));
                Ok(info)
            }
            Err(e) => {
                inner.last_error = Some(format!("{e:#}"));
                self.emit(json!({"t": "audio", "running": null, "error": format!("{e:#}")}));
                Err(e)
            }
        }
    }

    pub fn stop_audio(&self) {
        self.inner.lock().unwrap().engine = None;
        self.emit(json!({"t": "audio", "running": null}));
    }

    pub fn add_node(&self, kind: NodeKind, x: f32, y: f32) -> Result<NodeDesc> {
        let mut inner = self.inner.lock().unwrap();
        let id = format!("n{}", inner.board.next_id.max(1));
        inner.board.next_id = inner.board.next_id.max(1) + 1;
        let params = kind.params().iter().map(|p| (p.id.to_string(), p.default)).collect();
        let desc = NodeDesc { id: id.clone(), kind, params, bypass: false, x, y };
        inner.live.insert(id, LiveNode::from_desc(&desc));
        inner.board.nodes.push(desc.clone());
        self.structural_change(&mut inner)?;
        Ok(desc)
    }

    pub fn remove_node(&self, id: &str) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.board.node(id).is_none() {
            return Err(anyhow!("no such node"));
        }
        inner.board.nodes.retain(|n| n.id != id);
        inner.board.connections.retain(|c| c.from != id && c.to != id);
        self.structural_change(&mut inner)?;
        inner.live.remove(id); // after schedule swap; audio thread holds its own Arcs
        Ok(())
    }

    pub fn connect(&self, c: Connection) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.board.can_connect(&c).map_err(|e| anyhow!(e))?;
        inner.board.connections.push(c);
        self.structural_change(&mut inner)
    }

    pub fn disconnect(&self, c: &Connection) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.board.connections.retain(|x| x != c);
        self.structural_change(&mut inner)
    }

    /// Lock-free on the audio side: just an atomic store.
    pub fn set_param(&self, node: &str, param: &str, value: f32) -> Result<f32> {
        let mut inner = self.inner.lock().unwrap();
        let ln = inner.live.get(node).ok_or_else(|| anyhow!("no such node"))?;
        let specs = ln.kind.params();
        let idx = specs.iter().position(|p| p.id == param).ok_or_else(|| anyhow!("no such param"))?;
        let v = specs[idx].clamp(value);
        ln.params[idx].set(v);
        if let Some(d) = inner.board.node_mut(node) {
            d.params.insert(param.to_string(), v);
        }
        self.dirty.store(true, Relaxed);
        Ok(v)
    }

    pub fn set_bypass(&self, node: &str, on: bool) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.live.get(node).ok_or_else(|| anyhow!("no such node"))?.bypass.store(on, Relaxed);
        if let Some(d) = inner.board.node_mut(node) {
            d.bypass = on;
        }
        self.dirty.store(true, Relaxed);
        Ok(())
    }

    pub fn move_node(&self, node: &str, x: f32, y: f32) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(d) = inner.board.node_mut(node) {
            d.x = x;
            d.y = y;
            self.dirty.store(true, Relaxed);
        }
    }

    pub fn save_if_dirty(&self) {
        if self.dirty.swap(false, Relaxed) {
            let inner = self.inner.lock().unwrap();
            if let Ok(s) = serde_json::to_string_pretty(&inner.board) {
                let tmp = self.paths.board.with_extension("tmp");
                if std::fs::write(&tmp, s).is_ok() {
                    let _ = std::fs::rename(&tmp, &self.paths.board);
                }
            }
            // free old schedules periodically
            drop(inner);
            if let Some(e) = self.inner.lock().unwrap().engine.as_mut() {
                e.collect_garbage();
            }
        }
    }

    pub fn meters_json(&self) -> String {
        let inner = self.inner.lock().unwrap();
        let nodes: serde_json::Map<String, serde_json::Value> =
            inner.live.iter().map(|(id, n)| (id.clone(), json!(n.meter.take()))).collect();
        let s = &self.stats;
        json!({
            "t": "meters",
            "in": [s.in_peak[0].take(), s.in_peak[1].take()],
            "out": [s.out_peak[0].take(), s.out_peak[1].take()],
            "load": s.load.take(),
            "xruns": s.xruns.load(Relaxed),
            "frames": s.callback_frames.load(Relaxed),
            "nodes": nodes,
        })
        .to_string()
    }
}
