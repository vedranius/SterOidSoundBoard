use crate::ai::{self, AiConfig};
use crate::clinic::{self, AiReport, Clinic, Recording, Session};
use anyhow::{anyhow, Result};
use steroid_engine::fft::Spectrum;
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
}

impl Paths {
    pub fn new() -> Self {
        let data = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("SterOidSoundBoard");
        for d in ["boards", "presets", "recordings"] {
            let _ = std::fs::create_dir_all(data.join(d));
        }
        Paths {
            board: data.join("boards").join("default.json"),
            audio: data.join("audio.json"),
            presets: data.join("presets"),
            recordings: data.join("recordings"),
            clinic: data.join("clinic.json"),
            ai: data.join("ai.json"),
            data,
        }
    }
}

/// Spectrum resolution sent to clients: 240 bins, 0–12 kHz (50 Hz per bin).
const SPEC_BINS: usize = 240;
const SPEC_FMAX: f32 = 12000.0;

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
    pub spectrum: Spectrum,
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
                spectrum: Spectrum::new(2048),
            }),
            stats: Arc::new(EngineStats::default()),
            events,
            paths,
            dirty: AtomicBool::new(false),
            clinic: Mutex::new(clinic),
            ai: Mutex::new(ai),
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
    pub fn set_board(&self, mut board: Board) -> Result<()> {
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
        let s = Session { id: clinic::new_id("s"), patient_id: patient_id.into(), start: now, end: None, notes: String::new(), board: Some(board) };
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

    /// Voice analysis of a recording (optionally a [start, end] second range).
    pub fn analyze(&self, id: &str, start: Option<f32>, end: Option<f32>) -> Result<analysis::VoiceReport> {
        if !self.clinic.lock().unwrap().recordings.iter().any(|r| r.id == id) {
            return Err(anyhow!("no such recording"));
        }
        let (x, sr) = record::load_mono(&clinic::wav_path(&self.paths.recordings, id))?;
        let len = x.len() as f32 / sr as f32;
        let a = ((start.unwrap_or(0.0).clamp(0.0, len)) * sr as f32) as usize;
        let b = ((end.unwrap_or(len).clamp(0.0, len)) * sr as f32) as usize;
        let (a, b) = (a.min(b), a.max(b));
        Ok(analysis::analyze(&x[a..b], sr as f32))
    }

    // ------------------------------------------------------------ AI opinion

    /// Build the full AI request for a recording range. `image` is a data URL.
    pub fn ai_job(&self, id: &str, start: Option<f32>, end: Option<f32>, question: &str, image: Option<&str>) -> Result<AiJob> {
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
        let report = self.analyze(id, start, end)?;
        let (a, b) = (start.unwrap_or(0.0).max(0.0), end.unwrap_or(rec.duration).min(rec.duration));
        let history: Vec<(String, String, analysis::VoiceReport)> = earlier
            .iter()
            .filter_map(|r| self.analyze(&r.id, None, None).ok().map(|rep| (clinic::date_str(r.created), r.task.clone(), rep)))
            .collect();
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
        });
        Ok(AiJob { cfg, rec, start: a.min(b), end: a.max(b), system, user, image })
    }

    /// Run the job against the configured provider and store the opinion.
    pub fn ai_run(&self, job: AiJob, question: &str) -> Result<AiReport> {
        let ans = ai::ask_report(&job.cfg, &job.system, &job.user, job.image.as_ref())?;
        let rep = ai::new_report(&job.rec, &job.cfg, job.start, job.end, question, ans);
        let mut c = self.clinic.lock().unwrap();
        if !c.recordings.iter().any(|r| r.id == job.rec.id) {
            return Err(anyhow!("snimka je u međuvremenu obrisana"));
        }
        c.ai_reports.push(rep.clone());
        self.save_clinic(&c)?;
        Ok(rep)
    }

    pub fn meters_json(&self) -> String {
        let mut inner = self.inner.lock().unwrap();
        let mut spec = None;
        let Inner { engine, spectrum, .. } = &mut *inner;
        if let Some(e) = engine.as_mut() {
            let n = e.scope.slots();
            if let Ok(chunk) = e.scope.read_chunk(n) {
                let (a, b) = chunk.as_slices();
                a.iter().chain(b).for_each(|&v| spectrum.push(v));
                chunk.commit_all();
            }
            if spectrum.has_fresh() {
                let bins = spectrum.bins(e.info.sample_rate as f32, SPEC_FMAX, SPEC_BINS);
                spec = Some(bins.iter().map(|v| v.round() as i32).collect::<Vec<_>>());
            }
        }
        let rec = inner.recording.as_ref().map(|r| r.rec.seconds());
        let nodes: serde_json::Map<String, serde_json::Value> =
            inner.live.iter().map(|(id, n)| (id.clone(), json!(n.meter.take()))).collect();
        let s = &self.stats;
        json!({
            "spec": spec,
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
