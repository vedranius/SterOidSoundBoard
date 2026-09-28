use crate::ai::{self, AiConfig};
use crate::clinic::{self, AiReport, Clinic, Recording, Session};
use steroid_engine::analysis::{AnalysisSettings, Tracks, VoiceReport};
use anyhow::{anyhow, Result};
use steroid_engine::live::{LiveAnalyzer, SPEC_DB_MIN, SPEC_DB_STEP};
use base64::Engine as _;
use steroid_engine::record::Recorder;
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
    pub presets: PathBuf,
    pub recordings: PathBuf,
    pub clinic: PathBuf,
    pub ai: PathBuf,
    pub clinic_presets: PathBuf,
}

impl Paths {
    pub fn new() -> Self {
        let data = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("SterOidSoundBoard");
        for d in ["boards", "presets", "recordings", "clinic_presets"] {
            let _ = std::fs::create_dir_all(data.join(d));
        }
        Paths {
            board: data.join("boards").join("default.json"),
            audio: data.join("audio.json"),
            presets: data.join("presets"),
            recordings: data.join("recordings"),
            clinic: data.join("clinic.json"),
            ai: data.join("ai.json"),
            clinic_presets: data.join("clinic_presets"),
            data,
        }
    }
}


pub struct ActiveRec {
    pub rec: Recorder,
    pub meta: Recording,
}

pub struct Inner {
    pub board: Board,
    pub live: HashMap<String, LiveNode>,
    pub engine: Option<AudioEngine>,
    pub audio_cfg: AudioConfig,
    pub last_error: Option<String>,
    pub recording: Option<ActiveRec>,
    /// Live sonagram / F0 / intensity of the tapped signal.
    pub analyzer: LiveAnalyzer,
}

pub struct App {
    pub inner: Mutex<Inner>,
    pub stats: Arc<EngineStats>,
    pub events: broadcast::Sender<String>,
    pub paths: Paths,
    pub dirty: AtomicBool,
    /// Lock order: `inner` before `clinic`, never the reverse.
    pub clinic: Mutex<Clinic>,
    pub ai: Mutex<AiConfig>,
    /// Name of the clinical preset the current board came from (cleared on other loads).
    pub current_preset: Mutex<Option<String>>,
    /// Whole-recording analyses and editor tracks, keyed by recording id + settings.
    reports: Mutex<HashMap<String, Arc<VoiceReport>>>,
    tracks: Mutex<Vec<(String, Arc<Tracks>)>>,
    /// Clients currently showing a live sonagram, and the requested window (s).
    pub live_subs: std::sync::atomic::AtomicUsize,
    pub live_window: Mutex<f32>,
    /// AI opinions and model downloads running in the background (and recent ones).
    pub ai_tasks: Mutex<Vec<AiTask>>,
}

/// A background AI job with its log, streamed text and outcome.
#[derive(Clone, serde::Serialize)]
pub struct AiTask {
    pub id: String,
    /// "report" (AI opinion) or "pull" (Ollama model download).
    pub kind: String,
    pub recording_id: Option<String>,
    /// Model name.
    pub label: String,
    pub started: u64,
    /// running, done, error or cancelled.
    pub status: String,
    /// (milliseconds since start, message)
    pub log: Vec<(u64, String)>,
    pub text: String,
    pub stats: serde_json::Value,
    pub report: Option<AiReport>,
    pub error: Option<String>,
    #[serde(skip)]
    cancel: Arc<AtomicBool>,
}

const MAX_TASKS: usize = 30;

/// Progress sink of one task: stores everything and forwards it to the
/// browsers (text in batches, so a fast model does not flood the socket).
struct TaskProgress {
    app: Arc<App>,
    id: String,
    t0: std::time::Instant,
    cancel: Arc<AtomicBool>,
    pending: Mutex<Pending>,
}

/// Streamed text not yet sent to the browsers; `sent` counts characters
/// already sent, so a browser that loaded the task meanwhile can skip overlaps.
struct Pending {
    buf: String,
    last: std::time::Instant,
    sent: usize,
}

impl TaskProgress {
    fn with_task(&self, f: impl FnOnce(&mut AiTask)) {
        if let Some(t) = self.app.ai_tasks.lock().unwrap().iter_mut().find(|t| t.id == self.id) {
            f(t);
        }
    }
    fn flush(&self) {
        let (text, at) = {
            let mut p = self.pending.lock().unwrap();
            let text = std::mem::take(&mut p.buf);
            let at = p.sent;
            p.sent += text.chars().count();
            p.last = std::time::Instant::now();
            (text, at)
        };
        if !text.is_empty() {
            self.app.emit(json!({"t": "ai_task", "id": self.id, "ev": "delta", "text": text, "at": at}));
        }
    }
}

impl ai::Progress for TaskProgress {
    fn log(&self, msg: &str) {
        let ms = self.t0.elapsed().as_millis() as u64;
        self.flush();
        let mut n = None;
        self.with_task(|t| {
            if t.log.len() < 1000 {
                n = Some(t.log.len());
                t.log.push((ms, msg.to_string()));
            }
        });
        if let Some(n) = n {
            self.app.emit(json!({"t": "ai_task", "id": self.id, "ev": "log", "n": n, "ms": ms, "msg": msg}));
        }
    }
    fn delta(&self, text: &str) {
        self.with_task(|t| {
            if t.text.len() < 400_000 {
                t.text.push_str(text);
            }
        });
        let due = {
            let mut p = self.pending.lock().unwrap();
            p.buf.push_str(text);
            p.last.elapsed() > std::time::Duration::from_millis(150)
        };
        if due {
            self.flush();
        }
    }
    fn stats(&self, v: serde_json::Value) {
        self.flush();
        self.with_task(|t| t.stats = v.clone());
        self.app.emit(json!({"t": "ai_task", "id": self.id, "ev": "stats", "stats": v}));
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Relaxed)
    }
}

/// Saved custom clinical preset (file in `clinic_presets/`).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct StoredPreset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    pub created: u64,
    pub board: Board,
}

fn settings_key(id: &str, s: &AnalysisSettings) -> String {
    format!("{id}|{}|{}|{}|{}|{}", s.pitch_floor, s.pitch_ceiling, s.max_formant, s.n_formants, s.cpps)
}

/// Load a recording's WAV as mono samples.
fn load_rec(dir: &std::path::Path, id: &str) -> Result<(Vec<f32>, u32)> {
    record::load_mono(&clinic::wav_path(dir, id))
}

/// Everything an AI request needs, gathered without holding locks during the call.
pub struct AiJob {
    pub cfg: AiConfig,
    pub rec: Recording,
    pub start: f32,
    pub end: f32,
    pub system: String,
    pub user: String,
    pub image: Option<(String, String)>,
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
        let clinic = Clinic::load(&paths.clinic);
        let ai = AiConfig::load(&paths.ai);
        Arc::new(App {
            inner: Mutex::new(Inner {
                board,
                live,
                engine: None,
                audio_cfg,
                last_error: None,
                recording: None,
                analyzer: LiveAnalyzer::new(48000.0),
            }),
            stats: Arc::new(EngineStats::default()),
            events,
            paths,
            dirty: AtomicBool::new(false),
            clinic: Mutex::new(clinic),
            ai: Mutex::new(ai),
            current_preset: Mutex::new(None),
            reports: Mutex::new(HashMap::new()),
            tracks: Mutex::new(vec![]),
            live_subs: std::sync::atomic::AtomicUsize::new(0),
            live_window: Mutex::new(0.005),
            ai_tasks: Mutex::new(vec![]),
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
        let _ = self.stop_recording();
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
        let _ = self.stop_recording();
        self.inner.lock().unwrap().engine = None;
        self.emit(json!({"t": "audio", "running": null}));
    }

    /// Replace the whole board (template, preset). Same engine, new graph.
    pub fn set_board(&self, board: Board) -> Result<()> {
        self.set_board_named(board, None)
    }

    /// Replace the board and remember which clinical preset it came from.
    pub fn set_board_named(&self, mut board: Board, preset: Option<String>) -> Result<()> {
        board.sanitize().map_err(|e| anyhow!(e))?;
        let mut inner = self.inner.lock().unwrap();
        let old = std::mem::replace(&mut inner.board, board);
        let old_live = std::mem::take(&mut inner.live);
        inner.live = inner.board.nodes.iter().map(|n| (n.id.clone(), LiveNode::from_desc(n))).collect();
        if let Err(e) = self.structural_change(&mut inner) {
            inner.board = old;
            inner.live = old_live;
            return Err(e);
        }
        drop(inner);
        *self.current_preset.lock().unwrap() = preset.clone();
        self.emit(json!({"t": "preset", "name": preset}));
        Ok(())
    }

    pub fn add_node(&self, kind: NodeKind, x: f32, y: f32, role: Option<String>) -> Result<NodeDesc> {
        let mut inner = self.inner.lock().unwrap();
        let id = format!("n{}", inner.board.next_id.max(1));
        inner.board.next_id = inner.board.next_id.max(1) + 1;
        let params = kind.params().iter().map(|p| (p.id.to_string(), p.default)).collect();
        let desc = NodeDesc { id: id.clone(), kind, params, bypass: false, x, y, role };
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

    // ------------------------------------------------------------ presets

    fn preset_path(&self, name: &str) -> Result<PathBuf> {
        let n = clinic::safe_name(name).ok_or_else(|| anyhow!("invalid preset name"))?;
        Ok(self.paths.presets.join(format!("{n}.json")))
    }

    pub fn list_presets(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.paths.presets)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json").map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by_key(|s| s.to_lowercase());
        v
    }

    pub fn save_preset(&self, name: &str) -> Result<String> {
        let path = self.preset_path(name)?;
        let mut board = self.inner.lock().unwrap().board.clone();
        board.name = name.trim().to_string();
        std::fs::write(&path, serde_json::to_string_pretty(&board)?)?;
        self.emit(json!({"t": "presets"}));
        Ok(board.name)
    }

    pub fn load_preset(&self, name: &str) -> Result<()> {
        let s = std::fs::read_to_string(self.preset_path(name)?).map_err(|_| anyhow!("no such preset"))?;
        self.set_board(serde_json::from_str(&s)?)
    }

    pub fn delete_preset(&self, name: &str) -> Result<()> {
        std::fs::remove_file(self.preset_path(name)?).map_err(|_| anyhow!("no such preset"))?;
        self.emit(json!({"t": "presets"}));
        Ok(())
    }

    // ------------------------------------------------------------ output mute / taps

    pub fn set_mute(&self, on: bool) {
        self.stats.mute.store(on, Relaxed);
        self.emit(json!({"t": "mute", "on": on}));
    }

    pub fn set_tap(&self, src: u32) {
        self.stats.tap_src.store(src.min(1), Relaxed);
    }

    // ------------------------------------------------------------ clinic

    pub fn save_clinic(&self, c: &Clinic) -> Result<()> {
        c.save(&self.paths.clinic)?;
        self.emit(json!({"t": "clinic"}));
        Ok(())
    }

    pub fn start_session(&self, patient_id: &str) -> Result<Session> {
        let board = self.inner.lock().unwrap().board.clone();
        let mut c = self.clinic.lock().unwrap();
        if !c.patients.iter().any(|p| p.id == patient_id) {
            return Err(anyhow!("no such patient"));
        }
        let now = clinic::now_ms();
        for s in c.sessions.iter_mut().filter(|s| s.end.is_none()) {
            s.end = Some(now);
        }
        let preset = self.current_preset.lock().unwrap().clone();
        let s = Session { id: clinic::new_id("s"), patient_id: patient_id.into(), start: now, end: None, notes: String::new(), board: Some(board), preset };
        c.sessions.push(s.clone());
        self.save_clinic(&c)?;
        Ok(s)
    }

    pub fn start_recording(&self, patient_id: Option<String>, source: &str, label: String, task: String) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.recording.is_some() {
            return Err(anyhow!("already recording"));
        }
        let session_id = {
            let c = self.clinic.lock().unwrap();
            if let Some(p) = &patient_id {
                if !c.patients.iter().any(|x| &x.id == p) {
                    return Err(anyhow!("no such patient"));
                }
            }
            c.active_session().filter(|s| Some(&s.patient_id) == patient_id.as_ref()).map(|s| s.id.clone())
        };
        let engine = inner.engine.as_mut().ok_or_else(|| anyhow!("audio is not running"))?;
        let cons = engine.rec.take().ok_or_else(|| anyhow!("recorder unavailable — restart audio"))?;
        let src = if source == "out" { 1 } else { 0 };
        let meta = Recording {
            id: clinic::new_id("r"),
            patient_id,
            session_id,
            label,
            created: clinic::now_ms(),
            duration: 0.0,
            sample_rate: engine.info.sample_rate,
            source: if src == 1 { "out" } else { "in" }.into(),
            task,
            notes: String::new(),
            annotations: vec![],
            syllables: None,
        };
        let path = clinic::wav_path(&self.paths.recordings, &meta.id);
        match Recorder::start(cons, &path, meta.sample_rate, src, self.stats.clone()) {
            Ok(rec) => {
                inner.recording = Some(ActiveRec { rec, meta });
                self.emit(json!({"t": "rec", "on": true}));
                Ok(())
            }
            Err((cons, e)) => {
                engine.rec = Some(cons);
                Err(e)
            }
        }
    }

    pub fn stop_recording(&self) -> Result<Recording> {
        let mut inner = self.inner.lock().unwrap();
        let ActiveRec { rec, mut meta } = inner.recording.take().ok_or_else(|| anyhow!("not recording"))?;
        let (cons, res) = rec.finish();
        if let (Some(e), Some(c)) = (inner.engine.as_mut(), cons) {
            e.rec = Some(c);
        }
        drop(inner);
        self.emit(json!({"t": "rec", "on": false}));
        let frames = res?;
        meta.duration = frames as f32 / meta.sample_rate as f32;
        let mut c = self.clinic.lock().unwrap();
        c.recordings.push(meta.clone());
        self.save_clinic(&c)?;
        Ok(meta)
    }

    pub fn delete_recording(&self, id: &str) -> Result<()> {
        let mut c = self.clinic.lock().unwrap();
        let before = c.recordings.len();
        c.recordings.retain(|r| r.id != id);
        if c.recordings.len() == before {
            return Err(anyhow!("no such recording"));
        }
        c.ai_reports.retain(|a| a.recording_id != id);
        let _ = std::fs::remove_file(clinic::wav_path(&self.paths.recordings, id));
        self.save_clinic(&c)
    }

    pub fn delete_patient(&self, id: &str) -> Result<()> {
        let mut c = self.clinic.lock().unwrap();
        if !c.patients.iter().any(|p| p.id == id) {
            return Err(anyhow!("no such patient"));
        }
        for r in c.remove_patient(id) {
            let _ = std::fs::remove_file(clinic::wav_path(&self.paths.recordings, &r));
        }
        self.save_clinic(&c)
    }

    pub fn clinic_settings(&self) -> clinic::ClinicSettings {
        self.clinic.lock().unwrap().settings.clone()
    }

    fn settings_or_default(&self, s: Option<AnalysisSettings>) -> AnalysisSettings {
        s.unwrap_or_else(|| self.clinic.lock().unwrap().settings.analysis).sanitized()
    }

    /// Voice analysis of a recording (optionally a [start, end] second range).
    pub fn analyze(&self, id: &str, start: Option<f32>, end: Option<f32>, settings: Option<AnalysisSettings>) -> Result<VoiceReport> {
        let dur = self.clinic.lock().unwrap().recordings.iter().find(|r| r.id == id).map(|r| r.duration).ok_or_else(|| anyhow!("no such recording"))?;
        let st = self.settings_or_default(settings);
        // a selection covering the whole recording shares the cached whole-recording report
        let whole = start.is_none_or(|a| a <= 0.001) && end.is_none_or(|b| b >= dur - 0.001);
        if whole {
            return self.whole_report(id, &st).map(|r| (*r).clone());
        }
        let (x, sr) = load_rec(&self.paths.recordings, id)?;
        let len = x.len() as f32 / sr as f32;
        let a = ((start.unwrap_or(0.0).clamp(0.0, len)) * sr as f32) as usize;
        let b = ((end.unwrap_or(len).clamp(0.0, len)) * sr as f32) as usize;
        let (a, b) = (a.min(b), a.max(b));
        Ok(analysis::analyze_with(&x[a..b], sr as f32, &st))
    }

    /// Cached analysis of a whole recording (recordings never change).
    pub fn whole_report(&self, id: &str, st: &AnalysisSettings) -> Result<Arc<VoiceReport>> {
        let key = settings_key(id, st);
        if let Some(r) = self.reports.lock().unwrap().get(&key) {
            return Ok(r.clone());
        }
        let (x, sr) = load_rec(&self.paths.recordings, id)?;
        let r = Arc::new(analysis::analyze_with(&x, sr as f32, st));
        let mut cache = self.reports.lock().unwrap();
        if cache.len() > 500 {
            cache.clear();
        }
        cache.insert(key, r.clone());
        Ok(r)
    }

    /// Pitch / intensity / formant / pulse contours of a whole recording.
    pub fn tracks(&self, id: &str, settings: Option<AnalysisSettings>) -> Result<Arc<Tracks>> {
        if !self.clinic.lock().unwrap().recordings.iter().any(|r| r.id == id) {
            return Err(anyhow!("no such recording"));
        }
        let st = self.settings_or_default(settings);
        let key = settings_key(id, &st);
        if let Some((_, t)) = self.tracks.lock().unwrap().iter().find(|(k, _)| *k == key) {
            return Ok(t.clone());
        }
        let (x, sr) = load_rec(&self.paths.recordings, id)?;
        let t = Arc::new(analysis::tracks(&x, sr as f32, &st));
        let mut cache = self.tracks.lock().unwrap();
        cache.retain(|(k, _)| k != &key);
        cache.push((key, t.clone()));
        if cache.len() > 6 {
            cache.remove(0);
        }
        Ok(t)
    }

    /// One row of key measures per recording of a patient, oldest first.
    pub fn progress(&self, patient_id: &str) -> Result<serde_json::Value> {
        let (recs, st) = {
            let c = self.clinic.lock().unwrap();
            if !c.patients.iter().any(|p| p.id == patient_id) {
                return Err(anyhow!("no such patient"));
            }
            let mut v: Vec<Recording> = c.recordings.iter().filter(|r| r.patient_id.as_deref() == Some(patient_id)).cloned().collect();
            v.sort_by_key(|r| r.created);
            (v, c.settings.analysis.sanitized())
        };
        let rows: Vec<serde_json::Value> = recs
            .iter()
            .map(|r| {
                let rep = self.whole_report(&r.id, &st).ok();
                let sum = clinic::summarize(r, rep.as_ref().map(|x| x.syllable_nuclei));
                let g = |f: &dyn Fn(&VoiceReport) -> Option<f32>| rep.as_ref().and_then(|x| f(x));
                json!({
                    "id": r.id, "created": r.created, "task": r.task, "label": r.label, "session_id": r.session_id,
                    "duration": r.duration,
                    "f0_mean": g(&|x| x.f0_mean), "f0_sd": g(&|x| x.f0_sd), "f0_range_st": g(&|x| x.f0_range_st),
                    "jitter_local": g(&|x| x.jitter_local), "shimmer_local": g(&|x| x.shimmer_local),
                    "hnr_db": g(&|x| x.hnr_db), "cpps": g(&|x| x.cpps),
                    "intensity_mean_db": g(&|x| x.intensity_mean_db),
                    "max_voiced_s": g(&|x| Some(x.max_voiced_s)),
                    "speech_rate": g(&|x| Some(x.speech_rate)), "articulation_rate": g(&|x| Some(x.articulation_rate)),
                    "pauses": g(&|x| Some(x.pauses as f32)),
                    "f1": g(&|x| x.formants[0]), "f2": g(&|x| x.formants[1]),
                    "disfluencies": sum.total, "sld": sum.sld, "pct_ss": sum.pct_ss,
                })
            })
            .collect();
        Ok(json!({"settings": st, "rows": rows}))
    }

    // ------------------------------------------------------------ clinical presets

    pub fn clinical_presets(&self) -> Vec<serde_json::Value> {
        let mut v: Vec<serde_json::Value> = templates::clinical_presets()
            .iter()
            .map(|p| json!({"id": format!("f:{}", p.id), "name": p.name, "description": p.description, "category": p.category, "factory": true}))
            .collect();
        let mut custom: Vec<StoredPreset> = std::fs::read_dir(&self.paths.clinic_presets)
            .map(|d| d.filter_map(|e| e.ok()).filter_map(|e| std::fs::read_to_string(e.path()).ok()).filter_map(|s| serde_json::from_str(&s).ok()).collect())
            .unwrap_or_default();
        custom.sort_by_key(|p| p.name.to_lowercase());
        v.extend(custom.iter().map(|p| {
            json!({"id": format!("c:{}", p.id), "name": p.name, "description": p.description,
                   "category": if p.category.is_empty() { "Vlastiti".to_string() } else { p.category.clone() }, "factory": false, "created": p.created})
        }));
        v
    }

    fn custom_preset_path(&self, id: &str) -> Result<PathBuf> {
        let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !ok {
            return Err(anyhow!("invalid preset id"));
        }
        Ok(self.paths.clinic_presets.join(format!("{id}.json")))
    }

    pub fn save_clinical_preset(&self, name: &str, description: &str, category: &str) -> Result<String> {
        let name = name.trim();
        if name.is_empty() || name.len() > 120 {
            return Err(anyhow!("naziv preseta je obavezan (do 120 znakova)"));
        }
        let board = self.inner.lock().unwrap().board.clone();
        let p = StoredPreset {
            id: clinic::new_id("cp"),
            name: name.into(),
            description: description.chars().take(2000).collect(),
            category: category.chars().take(100).collect(),
            created: clinic::now_ms(),
            board,
        };
        std::fs::write(self.custom_preset_path(&p.id)?, serde_json::to_string_pretty(&p)?)?;
        *self.current_preset.lock().unwrap() = Some(p.name.clone());
        self.emit(json!({"t": "clinic_presets"}));
        Ok(p.id)
    }

    pub fn load_clinical_preset(&self, id: &str) -> Result<()> {
        if let Some(f) = id.strip_prefix("f:") {
            let p = templates::clinical_presets().into_iter().find(|p| p.id == f).ok_or_else(|| anyhow!("no such preset"))?;
            return self.set_board_named(p.board, Some(p.name.to_string()));
        }
        let c = id.strip_prefix("c:").ok_or_else(|| anyhow!("no such preset"))?;
        let s = std::fs::read_to_string(self.custom_preset_path(c)?).map_err(|_| anyhow!("no such preset"))?;
        let p: StoredPreset = serde_json::from_str(&s)?;
        self.set_board_named(p.board, Some(p.name))
    }

    pub fn delete_clinical_preset(&self, id: &str) -> Result<()> {
        let c = id.strip_prefix("c:").ok_or_else(|| anyhow!("tvornički preseti se ne mogu brisati"))?;
        std::fs::remove_file(self.custom_preset_path(c)?).map_err(|_| anyhow!("no such preset"))?;
        self.emit(json!({"t": "clinic_presets"}));
        Ok(())
    }

    // ------------------------------------------------------------ AI opinion

    /// Build the full AI request for a recording range. `image` is a data URL.
    pub fn ai_job(&self, id: &str, start: Option<f32>, end: Option<f32>, settings: Option<AnalysisSettings>, question: &str, image: Option<&str>) -> Result<AiJob> {
        let cfg = self.ai.lock().unwrap().clone();
        let (rec, patient, earlier) = {
            let c = self.clinic.lock().unwrap();
            let rec = c.recordings.iter().find(|r| r.id == id).cloned().ok_or_else(|| anyhow!("no such recording"))?;
            let patient = rec.patient_id.as_ref().and_then(|p| c.patients.iter().find(|x| &x.id == p)).map(|p| p.data.clone());
            let mut earlier: Vec<Recording> = c
                .recordings
                .iter()
                .filter(|r| r.patient_id.is_some() && r.patient_id == rec.patient_id && r.created < rec.created)
                .cloned()
                .collect();
            earlier.sort_by_key(|r| r.created);
            let skip = earlier.len().saturating_sub(6);
            (rec, patient, earlier.split_off(skip))
        };
        if let Some(p) = &patient {
            if !p.ai_consent {
                return Err(anyhow!("pacijent nema zabilježenu suglasnost za AI analizu (Uredi pacijenta)"));
            }
        }
        let report = self.analyze(id, start, end, settings)?;
        let st = self.settings_or_default(settings);
        let (a, b) = (start.unwrap_or(0.0).max(0.0), end.unwrap_or(rec.duration).min(rec.duration));
        let history: Vec<(String, String, analysis::VoiceReport)> = earlier
            .iter()
            .filter_map(|r| self.whole_report(&r.id, &st).ok().map(|rep| (clinic::date_str(r.created), r.task.clone(), (*rep).clone())))
            .collect();
        let whole = self.whole_report(id, &st).ok();
        let summary = (!rec.annotations.is_empty()).then(|| clinic::summarize(&rec, whole.as_ref().map(|w| w.syllable_nuclei)));
        let settings = self.clinic_settings();
        let image = match image {
            Some(img) if cfg.send_image && !img.is_empty() => Some(ai::parse_image(img)?),
            _ => None,
        };
        let age = patient.as_ref().and_then(|p| clinic::age_at(&p.birth, rec.created));
        let (system, user) = ai::build_prompt(&ai::Context {
            patient: patient.as_ref(),
            age,
            rec: &rec,
            start: a.min(b),
            end: a.max(b),
            report: &report,
            history: &history,
            question,
            has_image: image.is_some(),
            annotations: summary.as_ref(),
            calibration_db: settings.calibration_db,
        });
        Ok(AiJob { cfg, rec, start: a.min(b), end: a.max(b), system, user, image })
    }

    /// Run the job against the configured provider and store the opinion.
    fn save_ai_report(&self, rep: &AiReport) -> Result<()> {
        let mut c = self.clinic.lock().unwrap();
        if !c.recordings.iter().any(|r| r.id == rep.recording_id) {
            return Err(anyhow!("snimka je u međuvremenu obrisana"));
        }
        c.ai_reports.push(rep.clone());
        self.save_clinic(&c)
    }

    fn new_task(self: &Arc<Self>, kind: &str, recording_id: Option<String>, label: String) -> Result<(String, TaskProgress)> {
        let mut tasks = self.ai_tasks.lock().unwrap();
        if let Some(t) = tasks.iter().find(|t| t.status == "running" && t.kind == kind) {
            return Err(anyhow!(if kind == "pull" {
                format!("već se preuzima model {} — pričekajte da završi", t.label)
            } else {
                "AI već obrađuje drugi zahtjev — pričekajte ga ili ga zaustavite".to_string()
            }));
        }
        let id = clinic::new_id("t");
        let cancel = Arc::new(AtomicBool::new(false));
        tasks.push(AiTask {
            id: id.clone(),
            kind: kind.into(),
            recording_id,
            label,
            started: clinic::now_ms(),
            status: "running".into(),
            log: vec![],
            text: String::new(),
            stats: serde_json::Value::Null,
            report: None,
            error: None,
            cancel: cancel.clone(),
        });
        let running = tasks.iter().filter(|t| t.status == "running").count();
        while tasks.len() > MAX_TASKS.max(running) {
            if let Some(i) = tasks.iter().position(|t| t.status != "running") {
                tasks.remove(i);
            } else {
                break;
            }
        }
        drop(tasks);
        let prog = TaskProgress { app: self.clone(), id: id.clone(), t0: std::time::Instant::now(), cancel, pending: Mutex::new(Pending { buf: String::new(), last: std::time::Instant::now(), sent: 0 }) };
        self.emit(json!({"t": "ai_task", "id": id, "ev": "start", "kind": kind}));
        Ok((id, prog))
    }

    /// Record a task's outcome unless it was already cancelled.
    fn finish_task(&self, id: &str, status: &str, report: Option<AiReport>, error: Option<String>) {
        {
            let mut tasks = self.ai_tasks.lock().unwrap();
            let Some(t) = tasks.iter_mut().find(|t| t.id == id) else { return };
            if t.status != "running" {
                return;
            }
            t.status = status.into();
            t.report = report.clone();
            t.error = error.clone();
        }
        self.emit(json!({"t": "ai_task", "id": id, "ev": "end", "status": status, "report": report, "error": error}));
    }

    /// Start an AI opinion in the background; progress arrives as `ai_task` events.
    pub fn ai_start(self: &Arc<Self>, job: AiJob, question: String) -> Result<String> {
        let (id, prog) = self.new_task("report", Some(job.rec.id.clone()), job.cfg.model.trim().to_string())?;
        let app = self.clone();
        std::thread::Builder::new().name("ai".into()).spawn(move || {
            use ai::Progress;
            let res = ai::ask_report(&job.cfg, &job.system, &job.user, job.image.as_ref(), &prog);
            prog.flush();
            match res {
                Ok(ans) => {
                    if prog.cancelled() {
                        return app.finish_task(&prog.id, "cancelled", None, None);
                    }
                    let rep = ai::new_report(&job.rec, &job.cfg, job.start, job.end, &question, ans);
                    match app.save_ai_report(&rep) {
                        Ok(()) => {
                            prog.log("Mišljenje je spremljeno uz snimku.");
                            app.finish_task(&prog.id, "done", Some(rep), None)
                        }
                        Err(e) => app.finish_task(&prog.id, "error", None, Some(e.to_string())),
                    }
                }
                Err(_) if prog.cancelled() => app.finish_task(&prog.id, "cancelled", None, None),
                Err(e) => {
                    prog.log(&format!("Greška: {e:#}"));
                    app.finish_task(&prog.id, "error", None, Some(format!("{e:#}")))
                }
            }
        })?;
        Ok(id)
    }

    /// Download an Ollama model in the background.
    pub fn ollama_pull(self: &Arc<Self>, model: String) -> Result<String> {
        let root = self.ai.lock().unwrap().clone();
        let root = crate::ollama::root(if root.provider == "ollama" { &root.base_url } else { "" });
        let (id, prog) = self.new_task("pull", None, model.clone())?;
        let app = self.clone();
        std::thread::Builder::new().name("ollama-pull".into()).spawn(move || {
            use ai::Progress;
            match crate::ollama::pull(&root, &model, &prog) {
                Ok(()) => app.finish_task(&prog.id, "done", None, None),
                Err(_) if prog.cancelled() => app.finish_task(&prog.id, "cancelled", None, None),
                Err(e) => {
                    prog.log(&format!("Greška: {e:#}"));
                    app.finish_task(&prog.id, "error", None, Some(format!("{e:#}")))
                }
            }
        })?;
        Ok(id)
    }

    pub fn cancel_task(&self, id: &str) -> Result<()> {
        let (cancel, ms, n) = {
            let mut tasks = self.ai_tasks.lock().unwrap();
            let t = tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| anyhow!("nema tog zadatka"))?;
            let ms = clinic::now_ms().saturating_sub(t.started);
            let n = t.log.len();
            if t.status == "running" {
                t.log.push((ms, "Zaustavljeno na zahtjev korisnika.".into()));
            }
            (t.cancel.clone(), ms, n)
        };
        cancel.store(true, Relaxed);
        self.emit(json!({"t": "ai_task", "id": id, "ev": "log", "n": n, "ms": ms, "msg": "Zaustavljeno na zahtjev korisnika."}));
        self.finish_task(id, "cancelled", None, None);
        Ok(())
    }

    pub fn meters_json(&self) -> String {
        let subscribed = self.live_subs.load(Relaxed) > 0;
        let window = *self.live_window.lock().unwrap();
        let mut inner = self.inner.lock().unwrap();
        let mut live = serde_json::Value::Null;
        let Inner { engine, analyzer, .. } = &mut *inner;
        if let Some(e) = engine.as_mut() {
            let sr = e.info.sample_rate as f32;
            if (analyzer.sample_rate() - sr).abs() > 0.5 {
                *analyzer = LiveAnalyzer::new(sr);
            }
            analyzer.set_window(window);
            let n = e.scope.slots();
            if let Ok(chunk) = e.scope.read_chunk(n) {
                let (a, b) = chunk.as_slices();
                analyzer.push(a);
                analyzer.push(b);
                chunk.commit_all();
            }
            if subscribed {
                let (df, nb) = analyzer.bins();
                let frames: Vec<serde_json::Value> = analyzer
                    .frames()
                    .into_iter()
                    .map(|f| json!({"s": base64::engine::general_purpose::STANDARD.encode(&f.spec), "f0": (f.f0 * 10.0).round() / 10.0, "db": (f.db * 10.0).round() / 10.0}))
                    .collect();
                live = json!({"df": df, "n": nb, "hop": analyzer.hop_seconds(), "win": analyzer.window(),
                              "db_min": SPEC_DB_MIN, "db_step": SPEC_DB_STEP, "frames": frames});
            }
        }
        let rec = inner.recording.as_ref().map(|r| r.rec.seconds());
        let nodes: serde_json::Map<String, serde_json::Value> =
            inner.live.iter().map(|(id, n)| (id.clone(), json!(n.meter.take()))).collect();
        let s = &self.stats;
        json!({
            "live": live,
            "rec": rec,
            "rec_dropped": s.rec_dropped.load(Relaxed),
            "mute": s.mute.load(Relaxed),
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
