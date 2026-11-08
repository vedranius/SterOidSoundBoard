//! SterOidSoundBoard in the browser: the engine (DSP graph, Praat-validated
//! analysis, live sonagram) and the clinic logic (TextGrid, disfluency summary,
//! AI prompt) as one WebAssembly module with a small C ABI.
//!
//! Calls exchange JSON through linear memory: JS copies the method name and the
//! argument into buffers from `ssb_alloc`, calls `ssb_call`, and reads the result
//! from `ssb_out_ptr` / `ssb_out_len` (0 = ok, 1 = error message). Sample buffers
//! use dedicated exports so audio never goes through JSON.
//!
//! The same module runs in three places: the page (quick calls, live analyzer),
//! a Web Worker (whole-recording analysis) and the AudioWorklet (`dsp_*`).
use serde::Deserialize;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use steroid_clinic::clinic::{self, AnnotationSummary, PatientIn, Recording};
use steroid_clinic::prompt;
use steroid_engine::analysis::{self, AnalysisSettings, VoiceReport};
use steroid_engine::graph::{Board, Connection, LiveNode, NodeDesc, Schedule};
use steroid_engine::live::{LiveAnalyzer, SPEC_DB_MIN, SPEC_DB_STEP};
use steroid_engine::nodes::{new_buf, Buf, NodeKind, TapRing, MAX_BLOCK};
use std::sync::Arc;
use steroid_engine::templates;

// ------------------------------------------------------------------ memory + calls

thread_local! {
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// Samples of the recording loaded with `ssb_load_wav` (mono, louder channel).
    static WAV: RefCell<(Vec<f32>, u32)> = const { RefCell::new((Vec::new(), 0)) };
    /// Live analyzers by key ("" = the main sonagram, node id = a sonagram node).
    static LIVE: RefCell<(String, HashMap<String, LiveAnalyzer>)> = RefCell::new((String::new(), HashMap::new()));
    /// Scratch buffer JS writes samples into (live analyzer input).
    static SCRATCH: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    static DSP: RefCell<Option<Dsp>> = const { RefCell::new(None) };
}

/// Allocate `n` bytes for JS to write into (freed with `ssb_free`).
#[no_mangle]
pub extern "C" fn ssb_alloc(n: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(n.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `p` must come from `ssb_alloc(n)` with the same `n`.
#[no_mangle]
pub unsafe extern "C" fn ssb_free(p: *mut u8, n: usize) {
    drop(Vec::from_raw_parts(p, 0, n.max(1)));
}

#[no_mangle]
pub extern "C" fn ssb_out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn ssb_out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}

fn set_out(s: String) {
    OUT.with(|o| *o.borrow_mut() = s.into_bytes());
}

/// # Safety
/// The pointers must address `*_len` readable bytes (UTF-8 text).
unsafe fn text<'a>(p: *const u8, n: usize) -> &'a str {
    if n == 0 {
        return "";
    }
    std::str::from_utf8(std::slice::from_raw_parts(p, n)).unwrap_or("")
}

/// Run `method` with a JSON argument. Returns 0 (result JSON in the out buffer)
/// or 1 (error message in the out buffer).
///
/// # Safety
/// Both pointer/length pairs must address readable UTF-8 bytes.
#[no_mangle]
pub unsafe extern "C" fn ssb_call(m: *const u8, m_len: usize, a: *const u8, a_len: usize) -> i32 {
    let method = text(m, m_len);
    let arg: Value = match text(a, a_len) {
        "" => Value::Null,
        s => match serde_json::from_str(s) {
            Ok(v) => v,
            Err(e) => {
                set_out(format!("bad JSON for {method}: {e}"));
                return 1;
            }
        },
    };
    match call(method, arg) {
        Ok(v) => {
            set_out(v.to_string());
            0
        }
        Err(e) => {
            set_out(e);
            1
        }
    }
}

fn from<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| e.to_string())
}

fn to(v: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Range {
    start: Option<f32>,
    end: Option<f32>,
    settings: Option<AnalysisSettings>,
    en: bool,
}

#[derive(Deserialize)]
struct PromptIn {
    patient: Option<PatientIn>,
    age: Option<u32>,
    rec: Recording,
    start: f32,
    end: f32,
    report: VoiceReport,
    #[serde(default)]
    history: Vec<(String, String, VoiceReport)>,
    #[serde(default)]
    question: String,
    #[serde(default)]
    has_image: bool,
    annotations: Option<AnnotationSummary>,
    calibration_db: Option<f32>,
    #[serde(default)]
    en: bool,
}

fn call(method: &str, a: Value) -> Result<Value, String> {
    match method {
        "version" => Ok(json!(env!("CARGO_PKG_VERSION"))),
        // ---- boards
        "node_types" => to(NodeKind::ALL.iter().map(|k| k.info()).collect::<Vec<_>>()),
        "templates" => Ok(json!(templates::ALL.iter().map(|t| json!({"id": t.id, "name": t.name, "description": t.description})).collect::<Vec<_>>())),
        "template" => {
            let id = a.as_str().unwrap_or_default();
            to(templates::build(id).ok_or("no such template")?)
        }
        "clinical_presets" => Ok(json!(templates::clinical_presets()
            .into_iter()
            .map(|p| json!({"id": p.id, "name": p.name, "description": p.description, "category": p.category, "board": p.board}))
            .collect::<Vec<_>>())),
        "sanitize_board" => {
            let mut b: Board = from(a)?;
            b.sanitize()?;
            to(b)
        }
        "can_connect" => {
            let b: Board = from(a["board"].clone())?;
            let c: Connection = from(a["connection"].clone())?;
            b.can_connect(&c)?;
            Ok(Value::Null)
        }
        "new_node" => {
            let kind: NodeKind = from(a["kind"].clone())?;
            let params = kind.params().iter().map(|p| (p.id.to_string(), p.default)).collect();
            to(NodeDesc {
                id: a["id"].as_str().unwrap_or("n1").to_string(),
                kind,
                params,
                bypass: false,
                x: a["x"].as_f64().unwrap_or(0.0) as f32,
                y: a["y"].as_f64().unwrap_or(0.0) as f32,
                role: a["role"].as_str().map(String::from),
            })
        }
        "clamp_param" => {
            let kind: NodeKind = from(a["kind"].clone())?;
            let id = a["param"].as_str().unwrap_or_default();
            let spec = kind.params().iter().find(|p| p.id == id).ok_or("no such param")?;
            Ok(json!(spec.clamp(a["value"].as_f64().unwrap_or(0.0) as f32)))
        }
        // ---- clinic
        "annotation_kinds" => Ok(json!(clinic::ANNOTATION_KINDS
            .iter()
            .map(|(id, label, sld)| json!({"id": id, "label": label, "label_en": clinic::kind_label_en(id), "sld": sld}))
            .collect::<Vec<_>>())),
        "summary" => {
            let rec: Recording = from(a["rec"].clone())?;
            let nuclei = a["nuclei"].as_u64().map(|n| n as usize);
            to(clinic::summarize(&rec, nuclei))
        }
        "textgrid" => {
            let rec: Recording = from(a["rec"].clone())?;
            Ok(json!(clinic::textgrid(&rec, a["en"].as_bool().unwrap_or(false))))
        }
        "patient_check" => {
            // same rules as the server's check_patient
            let en = a["en"].as_bool().unwrap_or(false);
            let p: PatientIn = from(a["patient"].clone())?;
            if p.name.trim().is_empty() {
                return Err(if en { "a name is required" } else { "ime je obavezno" }.into());
            }
            if p.text_fields().iter().any(|(v, max)| v.len() > *max) {
                return Err(if en { "input too long" } else { "predugačak unos" }.into());
            }
            to(p)
        }
        "annotations_check" => {
            let en = a["en"].as_bool().unwrap_or(false);
            let list: Vec<clinic::Annotation> = from(a["annotations"].clone())?;
            let duration = a["duration"].as_f64().unwrap_or(0.0) as f32;
            if list.len() > 5000 {
                return Err(if en { "too many labels" } else { "previše oznaka" }.into());
            }
            for x in &list {
                if !(x.start.is_finite() && x.end.is_finite() && x.start >= 0.0 && x.end > x.start && x.end <= duration + 0.5) {
                    return Err(if en { format!("invalid label time {:.2}–{:.2} s", x.start, x.end) } else { format!("neispravno vrijeme oznake {:.2}–{:.2} s", x.start, x.end) });
                }
                if !clinic::ANNOTATION_KINDS.iter().any(|k| k.0 == x.kind) {
                    return Err(if en { format!("unknown label kind: {}", x.kind) } else { format!("nepoznata vrsta oznake: {}", x.kind) });
                }
                if x.text.len() > 500 || x.id.is_empty() || x.id.len() > 40 || !x.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    return Err(if en { "invalid label" } else { "neispravna oznaka" }.into());
                }
            }
            to(list)
        }
        // Round-trip through the Rust types: fills defaults exactly as the native app reads its files.
        "normalize" => {
            let v = a["value"].clone();
            match a["type"].as_str().unwrap_or_default() {
                // a missing or unreadable file starts empty, like the native `load_json`
                "clinic" => to(from::<clinic::Clinic>(v).unwrap_or_default()),
                "board" => to(from::<Board>(v).unwrap_or_default()),
                "recording" => to(from::<Recording>(v)?),
                "clinic_settings" => {
                    let mut b: clinic::ClinicSettings = from(v)?;
                    b.clinic_name = b.clinic_name.chars().take(200).collect();
                    b.clinician = b.clinician.chars().take(200).collect();
                    b.analysis = b.analysis.sanitized();
                    b.calibration_db = b.calibration_db.filter(|v| v.is_finite() && v.abs() < 200.0);
                    to(b)
                }
                t => Err(format!("unknown type {t}")),
            }
        }
        "protocols" => Ok(json!({"tasks": steroid_clinic::protocol::TASKS, "protocols": steroid_clinic::protocol::PROTOCOLS})),
        "voice_profile" => {
            let items: Vec<(Recording, VoiceReport)> = from(a["items"].clone())?;
            let refs: Vec<(&Recording, &VoiceReport)> = items.iter().map(|(r, v)| (r, v)).collect();
            to(steroid_clinic::protocol::voice_profile(&refs, a["calibration_db"].as_f64().map(|v| v as f32)))
        }
        "routing_sanitized" => to(from::<steroid_engine::routing::Routing>(a)?.sanitized()),
        "age_at" => Ok(json!(clinic::age_at(a["birth"].as_str().unwrap_or_default(), a["at"].as_u64().unwrap_or(0)))),
        "date_str" => Ok(json!(clinic::date_str(a.as_u64().unwrap_or(0)))),
        "settings_sanitized" => {
            let s: AnalysisSettings = if a.is_null() { AnalysisSettings::default() } else { from(a)? };
            to(s.sanitized())
        }
        "prompt" => {
            let p: PromptIn = from(a)?;
            let (system, user) = prompt::build_prompt(&prompt::Context {
                patient: p.patient.as_ref(),
                age: p.age,
                rec: &p.rec,
                start: p.start,
                end: p.end,
                report: &p.report,
                history: &p.history,
                question: &p.question,
                has_image: p.has_image,
                annotations: p.annotations.as_ref(),
                calibration_db: p.calibration_db,
                en: p.en,
            });
            Ok(json!({"system": system, "user": user}))
        }
        "localize" => {
            let mut r: VoiceReport = from(a["report"].clone())?;
            analysis::localize(&mut r, a["en"].as_bool().unwrap_or(false));
            to(r)
        }
        // ---- analysis of the loaded WAV
        "wav_info" => WAV.with(|w| {
            let w = w.borrow();
            Ok(json!({"frames": w.0.len(), "sample_rate": w.1, "duration": if w.1 > 0 { w.0.len() as f64 / w.1 as f64 } else { 0.0 }}))
        }),
        "analyze" => {
            let r: Range = if a.is_null() { Range::default() } else { from(a)? };
            WAV.with(|w| {
                let w = w.borrow();
                let (x, sr) = (&w.0, w.1);
                if sr == 0 {
                    return Err("no recording loaded".to_string());
                }
                let st = r.settings.unwrap_or_default().sanitized();
                // same slicing as the native app (state.rs `analyze`)
                let len = x.len() as f32 / sr as f32;
                let a = ((r.start.unwrap_or(0.0).clamp(0.0, len)) * sr as f32) as usize;
                let b = ((r.end.unwrap_or(len).clamp(0.0, len)) * sr as f32) as usize;
                let (a, b) = (a.min(b), a.max(b));
                let mut rep = analysis::analyze_with(&x[a..b], sr as f32, &st);
                if r.en {
                    analysis::localize(&mut rep, true);
                }
                to(rep)
            })
        }
        "tracks" => {
            let r: Range = if a.is_null() { Range::default() } else { from(a)? };
            WAV.with(|w| {
                let w = w.borrow();
                if w.1 == 0 {
                    return Err("no recording loaded".to_string());
                }
                let st = r.settings.unwrap_or_default().sanitized();
                to(analysis::tracks(&w.0, w.1 as f32, &st))
            })
        }
        // ---- live sonagram
        "live_select" => LIVE.with(|l| {
            let mut l = l.borrow_mut();
            let key = a["key"].as_str().unwrap_or_default().to_string();
            let sr = a["sr"].as_f64().unwrap_or(48000.0) as f32;
            let an = l.1.entry(key.clone()).or_insert_with(|| LiveAnalyzer::new(sr));
            if (an.sample_rate() - sr).abs() > 0.5 {
                *an = LiveAnalyzer::new(sr);
            }
            l.0 = key;
            Ok(Value::Null)
        }),
        "live_drop" => LIVE.with(|l| {
            let keep: Vec<String> = from(a)?;
            l.borrow_mut().1.retain(|k, _| k.is_empty() || keep.contains(k));
            Ok(Value::Null)
        }),
        "live_frames" => LIVE.with(|l| {
            let mut l = l.borrow_mut();
            let (cur, map) = &mut *l;
            let Some(an) = map.get_mut(cur.as_str()) else { return Ok(Value::Null) };
            if let Some(w) = a["win"].as_f64() {
                an.set_window(w as f32);
            }
            let (df, nb) = an.bins();
            let frames: Vec<Value> = an
                .frames()
                .into_iter()
                .map(|f| {
                    json!({"s": base64(&f.spec), "f0": (f.f0 * 10.0).round() / 10.0, "db": (f.db * 10.0).round() / 10.0,
                           "lo": (f.lo * 1000.0).round() / 1000.0, "hi": (f.hi * 1000.0).round() / 1000.0})
                })
                .collect();
            Ok(json!({"df": df, "n": nb, "hop": an.hop_seconds(), "win": an.window(), "db_min": SPEC_DB_MIN, "db_step": SPEC_DB_STEP, "frames": frames}))
        }),
        // ---- DSP control (AudioWorklet)
        "dsp_board" => {
            let mut b: Board = from(a)?;
            b.sanitize()?;
            DSP.with(|d| d.borrow_mut().as_mut().ok_or("dsp not started".to_string())?.set_board(&b))?;
            Ok(Value::Null)
        }
        "dsp_param" => DSP.with(|d| {
            let mut d = d.borrow_mut();
            let d = d.as_mut().ok_or("dsp not started")?;
            let ln = d.live.get(a["node"].as_str().unwrap_or_default()).ok_or("no such node")?;
            let specs = ln.kind.params();
            let id = a["param"].as_str().unwrap_or_default();
            let i = specs.iter().position(|p| p.id == id).ok_or("no such param")?;
            let v = specs[i].clamp(a["value"].as_f64().unwrap_or(0.0) as f32);
            ln.params[i].set(v);
            Ok(json!(v))
        }),
        "dsp_bypass" => DSP.with(|d| {
            let mut d = d.borrow_mut();
            let d = d.as_mut().ok_or("dsp not started")?;
            let ln = d.live.get(a["node"].as_str().unwrap_or_default()).ok_or("no such node")?;
            ln.bypass.store(a["on"].as_bool().unwrap_or(false), std::sync::atomic::Ordering::Relaxed);
            Ok(Value::Null)
        }),
        "dsp_taps" => DSP.with(|d| {
            let d = d.borrow();
            let d = d.as_ref().ok_or("dsp not started")?;
            Ok(json!(d.taps.iter().map(|t| json!({"id": t.0, "kind": t.3})).collect::<Vec<_>>()))
        }),
        "dsp_meters" => DSP.with(|d| {
            let d = d.borrow();
            let d = d.as_ref().ok_or("dsp not started")?;
            Ok(Value::Object(d.live.iter().map(|(id, n)| (id.clone(), json!(n.meter.take()))).collect()))
        }),
        _ => Err(format!("unknown method {method}")),
    }
}

fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(A[(n >> 18) as usize & 63] as char);
        s.push(A[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    s
}

// ------------------------------------------------------------------ recordings

/// Decode WAV bytes (exactly like the native app: hound, louder channel) and keep
/// them for `analyze` / `tracks`. Returns the sample rate, or 0 with the error in
/// the out buffer.
///
/// # Safety
/// `p` must address `n` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn ssb_load_wav(p: *const u8, n: usize) -> u32 {
    let bytes = std::slice::from_raw_parts(p, n);
    match steroid_engine::wav::decode_mono(std::io::Cursor::new(bytes)) {
        Ok((x, sr)) => {
            WAV.with(|w| *w.borrow_mut() = (x, sr));
            sr
        }
        Err(e) => {
            set_out(e.to_string());
            0
        }
    }
}

// ------------------------------------------------------------------ live analyzer

/// Buffer of `n` floats for samples going into the live analyzer.
#[no_mangle]
pub extern "C" fn ssb_scratch(n: usize) -> *mut f32 {
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() < n {
            s.resize(n, 0.0);
        }
        s.as_mut_ptr()
    })
}

#[no_mangle]
pub extern "C" fn ssb_live_reset(sr: f32) {
    LIVE.with(|l| {
        let mut l = l.borrow_mut();
        l.0 = String::new();
        l.1.insert(String::new(), LiveAnalyzer::new(sr));
    });
}

/// Feed `n` samples from the scratch buffer to the live analyzer.
#[no_mangle]
pub extern "C" fn ssb_live_push(n: usize) {
    SCRATCH.with(|s| {
        let s = s.borrow();
        LIVE.with(|l| {
            let mut l = l.borrow_mut();
            let (cur, map) = &mut *l;
            if let Some(a) = map.get_mut(cur.as_str()) {
                a.push(&s[..n.min(s.len())]);
            }
        })
    })
}

// ------------------------------------------------------------------ DSP (AudioWorklet)

struct Dsp {
    sr: f32,
    live: HashMap<String, LiveNode>,
    /// Display nodes: (id, ring, read position, kind).
    taps: Vec<(String, Arc<TapRing>, usize, NodeKind)>,
    tap_buf: Vec<f32>,
    sched: Option<Schedule>,
    inbuf: Buf,
    outbuf: Buf,
}

impl Dsp {
    fn set_board(&mut self, b: &Board) -> Result<(), String> {
        let live: HashMap<String, LiveNode> = b.nodes.iter().map(|n| (n.id.clone(), LiveNode::from_desc(n))).collect();
        let s = Schedule::build(b, &live, self.sr)?;
        self.sched = Some(s);
        let mut taps: Vec<_> = live.iter().filter_map(|(id, n)| n.tap.clone().map(|r| (id.clone(), r, 0usize, n.kind))).collect();
        taps.sort_by(|a, b| a.0.cmp(&b.0));
        self.taps = taps;
        self.live = live;
        Ok(())
    }
}

/// Start the DSP engine at the AudioContext sample rate (empty board).
#[no_mangle]
pub extern "C" fn dsp_init(sr: f32) {
    DSP.with(|d| *d.borrow_mut() = Some(Dsp { sr, live: HashMap::new(), taps: vec![], tap_buf: Vec::with_capacity(TapRing::CAP), sched: None, inbuf: new_buf(), outbuf: new_buf() }));
}

/// Read what display node `i` (order of `dsp_taps`) published since the last
/// read; returns the count, samples at `dsp_tap_ptr()`.
#[no_mangle]
pub extern "C" fn dsp_tap_read(i: usize) -> usize {
    DSP.with(|d| {
        let mut d = d.borrow_mut();
        let Some(d) = d.as_mut() else { return 0 };
        let Dsp { taps, tap_buf, .. } = d;
        tap_buf.clear();
        if let Some((_, ring, pos, _)) = taps.get_mut(i) {
            ring.read(pos, tap_buf);
        }
        tap_buf.len()
    })
}

#[no_mangle]
pub extern "C" fn dsp_tap_ptr() -> *const f32 {
    DSP.with(|d| d.borrow().as_ref().map(|d| d.tap_buf.as_ptr()).unwrap_or(std::ptr::null()))
}

/// Input buffer: `ch` 0 = left, 1 = right, MAX_BLOCK floats each.
#[no_mangle]
pub extern "C" fn dsp_in(ch: usize) -> *mut f32 {
    DSP.with(|d| d.borrow_mut().as_mut().map(|d| d.inbuf[ch.min(1)].as_mut_ptr()).unwrap_or(std::ptr::null_mut()))
}

#[no_mangle]
pub extern "C" fn dsp_out(ch: usize) -> *mut f32 {
    DSP.with(|d| d.borrow_mut().as_mut().map(|d| d.outbuf[ch.min(1)].as_mut_ptr()).unwrap_or(std::ptr::null_mut()))
}

/// Process `n` (≤ 512) frames from the input buffers into the output buffers,
/// with the same safety limiter as the native output (no NaN, clamp to 0 dBFS).
#[no_mangle]
pub extern "C" fn dsp_process(n: usize) {
    let n = n.min(MAX_BLOCK);
    DSP.with(|d| {
        let mut d = d.borrow_mut();
        let Some(d) = d.as_mut() else { return };
        let Dsp { sched, inbuf, outbuf, .. } = d;
        match sched {
            Some(s) => {
                let ib = s.input_mut();
                ib[0][..n].copy_from_slice(&inbuf[0][..n]);
                ib[1][..n].copy_from_slice(&inbuf[1][..n]);
                s.run(n, outbuf);
            }
            None => {
                outbuf[0][..n].fill(0.0);
                outbuf[1][..n].fill(0.0);
            }
        }
        for ch in outbuf.iter_mut() {
            for v in ch[..n].iter_mut() {
                *v = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(m: &str, a: Value) -> Result<Value, String> {
        call(m, a)
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn dsp_runs_the_digilingua_chain() {
        dsp_init(48000.0);
        let b = run("clinical_presets", Value::Null).unwrap()[0]["board"].clone();
        run("dsp_board", b).unwrap();
        DSP.with(|d| {
            let mut d = d.borrow_mut();
            let d = d.as_mut().unwrap();
            for i in 0..128 {
                d.inbuf[0][i] = (i as f32 * 0.05).sin() * 0.5;
                d.inbuf[1][i] = d.inbuf[0][i];
            }
        });
        for _ in 0..50 {
            dsp_process(128);
        }
        DSP.with(|d| {
            let d = d.borrow();
            let d = d.as_ref().unwrap();
            assert!(d.outbuf[0][..128].iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        });
        assert!(run("dsp_meters", Value::Null).unwrap().as_object().unwrap().len() > 3);
    }

    #[test]
    fn analysis_of_loaded_wav_matches_direct_call() {
        let sr = 44100u32;
        let x: Vec<f32> = (0..sr as usize).map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 150.0 / sr as f32).sin() * 0.3).collect();
        let mut wav = std::io::Cursor::new(Vec::new());
        {
            let spec = hound::WavSpec { channels: 1, sample_rate: sr, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
            let mut w = hound::WavWriter::new(&mut wav, spec).unwrap();
            for v in &x {
                w.write_sample(*v).unwrap();
            }
            w.finalize().unwrap();
        }
        let bytes = wav.into_inner();
        assert_eq!(unsafe { ssb_load_wav(bytes.as_ptr(), bytes.len()) }, sr);
        let r = run("analyze", Value::Null).unwrap();
        let direct = analysis::analyze_with(&x, sr as f32, &AnalysisSettings::default().sanitized());
        assert_eq!(r["f0_median"], json!(direct.f0_median));
        assert_eq!(r["hnr_db"], json!(direct.hnr_db));
    }
}
