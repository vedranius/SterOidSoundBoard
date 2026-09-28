//! Built-in DSP nodes. Every processor is allocation-free in `process`.
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

pub const MAX_BLOCK: usize = 512;
/// Stereo block buffer, each channel has MAX_BLOCK samples.
pub type Buf = [Vec<f32>; 2];

pub fn new_buf() -> Buf {
    [vec![0.0; MAX_BLOCK], vec![0.0; MAX_BLOCK]]
}

/// Lock-free f32 shared between control and audio threads.
#[derive(Debug, Default)]
pub struct AtomicF32(AtomicU32);
impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    #[inline]
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }
    #[inline]
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Relaxed)
    }
    /// Store the maximum (used for peak meters).
    #[inline]
    pub fn max(&self, v: f32) {
        let mut cur = self.0.load(Relaxed);
        while f32::from_bits(cur) < v {
            match self.0.compare_exchange_weak(cur, v.to_bits(), Relaxed, Relaxed) {
                Ok(_) => return,
                Err(c) => cur = c,
            }
        }
    }
    /// Read and reset to zero.
    pub fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0f32.to_bits(), Relaxed))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ParamSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    /// Logarithmic UI mapping.
    pub log: bool,
    /// Stepped/enum values; empty = continuous.
    pub options: &'static [&'static str],
}

impl ParamSpec {
    pub fn clamp(&self, v: f32) -> f32 {
        if !v.is_finite() {
            return self.default;
        }
        let v = v.clamp(self.min, self.max);
        if self.options.is_empty() { v } else { v.round() }
    }
}

const fn p(
    id: &'static str,
    name: &'static str,
    min: f32,
    max: f32,
    default: f32,
    unit: &'static str,
    log: bool,
) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, log, options: &[] }
}

static GAIN_P: [ParamSpec; 1] = [p("gain", "Gain", -60.0, 24.0, 0.0, "dB", false)];
static FILTER_P: [ParamSpec; 4] = [
    ParamSpec {
        id: "mode",
        name: "Mode",
        min: 0.0,
        max: 3.0,
        default: 0.0,
        unit: "",
        log: false,
        options: &["Low-pass", "High-pass", "Band-pass", "Peak"],
    },
    p("freq", "Frequency", 20.0, 20000.0, 1000.0, "Hz", true),
    p("q", "Q", 0.1, 18.0, 0.707, "", true),
    p("gain", "Gain", -24.0, 24.0, 0.0, "dB", false),
];
static DRIVE_P: [ParamSpec; 4] = [
    p("drive", "Drive", 0.0, 40.0, 12.0, "dB", false),
    p("tone", "Tone", 500.0, 12000.0, 4000.0, "Hz", true),
    p("level", "Level", -40.0, 6.0, -6.0, "dB", false),
    p("mix", "Mix", 0.0, 1.0, 1.0, "", false),
];
static DELAY_P: [ParamSpec; 4] = [
    p("time", "Time", 1.0, 2000.0, 350.0, "ms", true),
    p("feedback", "Feedback", 0.0, 0.95, 0.35, "", false),
    p("tone", "Tone", 500.0, 16000.0, 6000.0, "Hz", true),
    p("mix", "Mix", 0.0, 1.0, 0.3, "", false),
];
static TREMOLO_P: [ParamSpec; 2] = [
    p("rate", "Rate", 0.1, 20.0, 5.0, "Hz", true),
    p("depth", "Depth", 0.0, 1.0, 0.5, "", false),
];

/// ISO 266 1/3-octave centre frequencies (31 bands, 20 Hz – 20 kHz).
pub const ISO_FREQS: [f32; 31] = [
    20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0,
    800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0,
    16000.0, 20000.0,
];
/// Q of a 1/3-octave band.
const THIRD_OCT_Q: f32 = 4.318;

const fn band(id: &'static str, name: &'static str) -> ParamSpec {
    p(id, name, -48.0, 24.0, 0.0, "dB", false)
}
static EQ31_P: [ParamSpec; 32] = [
    band("b20", "20"),
    band("b25", "25"),
    band("b31", "31.5"),
    band("b40", "40"),
    band("b50", "50"),
    band("b63", "63"),
    band("b80", "80"),
    band("b100", "100"),
    band("b125", "125"),
    band("b160", "160"),
    band("b200", "200"),
    band("b250", "250"),
    band("b315", "315"),
    band("b400", "400"),
    band("b500", "500"),
    band("b630", "630"),
    band("b800", "800"),
    band("b1k", "1k"),
    band("b1k25", "1.25k"),
    band("b1k6", "1.6k"),
    band("b2k", "2k"),
    band("b2k5", "2.5k"),
    band("b3k15", "3.15k"),
    band("b4k", "4k"),
    band("b5k", "5k"),
    band("b6k3", "6.3k"),
    band("b8k", "8k"),
    band("b10k", "10k"),
    band("b12k5", "12.5k"),
    band("b16k", "16k"),
    band("b20k", "20k"),
    p("out", "Output", -24.0, 24.0, 0.0, "dB", false),
];
static DAF_P: [ParamSpec; 3] = [
    p("time", "Delay", 0.0, 5000.0, 150.0, "ms", false),
    p("dry", "Dry", 0.0, 1.0, 0.0, "", false),
    p("wet", "Delayed", 0.0, 1.0, 1.0, "", false),
];
static PITCH_P: [ParamSpec; 3] = [
    p("semi", "Shift", -12.0, 12.0, -3.0, "st", false),
    p("window", "Window", 20.0, 100.0, 40.0, "ms", false),
    p("mix", "Mix", 0.0, 1.0, 1.0, "", false),
];
static NOISE_P: [ParamSpec; 4] = [
    ParamSpec {
        id: "color",
        name: "Color",
        min: 0.0,
        max: 3.0,
        default: 0.0,
        unit: "",
        log: false,
        options: &["White", "Pink", "Brown", "Narrow-band"],
    },
    p("level", "Level", -80.0, 0.0, -30.0, "dB", false),
    p("freq", "Band centre", 100.0, 8000.0, 1000.0, "Hz", true),
    ParamSpec {
        id: "route",
        name: "Ears",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
        log: false,
        options: &["Both", "Left", "Right"],
    },
];
static INTERRUPT_P: [ParamSpec; 4] = [
    p("period", "Period", 50.0, 5000.0, 1000.0, "ms", true),
    p("gap", "Gap", 10.0, 2000.0, 200.0, "ms", true),
    p("depth", "Depth", 0.0, 1.0, 1.0, "", false),
    p("fade", "Fade", 1.0, 50.0, 5.0, "ms", true),
];
static CHANNELS_P: [ParamSpec; 3] = [
    ParamSpec {
        id: "source",
        name: "Source",
        min: 0.0,
        max: 3.0,
        default: 0.0,
        unit: "",
        log: false,
        options: &["Stereo", "Left \u{2192} both", "Right \u{2192} both", "Mono sum"],
    },
    p("left", "Left", 0.0, 200.0, 100.0, "%", false),
    p("right", "Right", 0.0, 200.0, 100.0, "%", false),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Gain,
    Filter,
    Drive,
    Delay,
    Tremolo,
    Eq31,
    Daf,
    PitchShift,
    Noise,
    Interrupter,
    Channels,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub kind: NodeKind,
    pub name: &'static str,
    pub category: &'static str,
    pub params: &'static [ParamSpec],
}

impl NodeKind {
    pub const ALL: &'static [NodeKind] = &[
        NodeKind::Gain,
        NodeKind::Filter,
        NodeKind::Drive,
        NodeKind::Delay,
        NodeKind::Tremolo,
        NodeKind::Eq31,
        NodeKind::Daf,
        NodeKind::PitchShift,
        NodeKind::Noise,
        NodeKind::Interrupter,
        NodeKind::Channels,
    ];

    pub fn info(self) -> NodeInfo {
        let (name, category) = match self {
            NodeKind::Gain => ("Gain", "Utility"),
            NodeKind::Filter => ("Filter / EQ", "Filter"),
            NodeKind::Drive => ("Drive", "Distortion"),
            NodeKind::Delay => ("Delay", "Time"),
            NodeKind::Tremolo => ("Tremolo", "Modulation"),
            NodeKind::Eq31 => ("31-band EQ", "Filter"),
            NodeKind::Daf => ("DAF Delay", "Time"),
            NodeKind::PitchShift => ("Pitch Shift / FAF", "Pitch"),
            NodeKind::Noise => ("Noise", "Generator"),
            NodeKind::Interrupter => ("Interrupter", "Modulation"),
            NodeKind::Channels => ("Channels / Balance", "Utility"),
        };
        NodeInfo { kind: self, name, category, params: self.params() }
    }

    pub fn params(self) -> &'static [ParamSpec] {
        match self {
            NodeKind::Gain => &GAIN_P,
            NodeKind::Filter => &FILTER_P,
            NodeKind::Drive => &DRIVE_P,
            NodeKind::Delay => &DELAY_P,
            NodeKind::Tremolo => &TREMOLO_P,
            NodeKind::Eq31 => &EQ31_P,
            NodeKind::Daf => &DAF_P,
            NodeKind::PitchShift => &PITCH_P,
            NodeKind::Noise => &NOISE_P,
            NodeKind::Interrupter => &INTERRUPT_P,
            NodeKind::Channels => &CHANNELS_P,
        }
    }

    /// Create the processor. Called on the control thread (may allocate).
    pub fn create(self, sr: f32) -> Box<dyn Processor> {
        match self {
            NodeKind::Gain => Box::new(Gain { g: Smooth::new(sr, 0.01, 1.0) }),
            NodeKind::Filter => Box::new(Filter::new(sr)),
            NodeKind::Drive => Box::new(Drive::new(sr)),
            NodeKind::Delay => Box::new(Delay::new(sr)),
            NodeKind::Tremolo => Box::new(Tremolo { sr, phase: 0.0 }),
            NodeKind::Eq31 => Box::new(Eq31::new(sr)),
            NodeKind::Daf => Box::new(Daf::new(sr)),
            NodeKind::PitchShift => Box::new(PitchShift::new(sr)),
            NodeKind::Noise => Box::new(Noise::new(sr)),
            NodeKind::Interrupter => Box::new(Interrupter::new(sr)),
            NodeKind::Channels => Box::new(Channels::new(sr)),
        }
    }
}

pub trait Processor: Send {
    /// Must not allocate, lock or block.
    fn process(&mut self, params: &[AtomicF32], input: &Buf, output: &mut Buf, n: usize);
}

#[inline]
pub fn db2lin(db: f32) -> f32 {
    10f32.powf(db * 0.05)
}

/// One-pole parameter smoother. State is f64: with f32 a slow smoother on a
/// large value (e.g. a 4800-sample delay time) stalls short of its target.
struct Smooth {
    cur: f64,
    coef: f64,
}
impl Smooth {
    fn new(sr: f32, secs: f32, init: f32) -> Self {
        Self { cur: init as f64, coef: 1.0 - (-1.0 / (secs as f64 * sr as f64)).exp() }
    }
    #[inline]
    fn next(&mut self, target: f32) -> f32 {
        self.cur += (target as f64 - self.cur) * self.coef;
        self.cur as f32
    }
}

// ---------------------------------------------------------------- Gain
struct Gain {
    g: Smooth,
}
impl Processor for Gain {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        let target = db2lin(p[0].get());
        for s in 0..n {
            let g = self.g.next(target);
            o[0][s] = i[0][s] * g;
            o[1][s] = i[1][s] * g;
        }
    }
}

// ---------------------------------------------------------------- Biquad
#[derive(Default, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: [f32; 2],
    z2: [f32; 2],
}
impl Biquad {
    /// RBJ audio EQ cookbook. mode: 0 LP, 1 HP, 2 BP, 3 peak.
    fn set(&mut self, mode: u32, sr: f32, freq: f32, q: f32, gain_db: f32) {
        let f = freq.clamp(10.0, sr * 0.49);
        let w = 2.0 * PI * f / sr;
        let (sn, cs) = w.sin_cos();
        let alpha = sn / (2.0 * q.max(0.05));
        let (b0, b1, b2, a0, a1, a2) = match mode {
            0 => ((1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            1 => ((1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            2 => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            _ => {
                let a = 10f32.powf(gain_db / 40.0);
                (1.0 + alpha * a, -2.0 * cs, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cs, 1.0 - alpha / a)
            }
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }
    /// Peaking EQ from precomputed cos(w) and alpha (fixed-frequency bands).
    fn set_peak(&mut self, cs: f32, alpha: f32, gain_db: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let a0 = 1.0 + alpha / a;
        self.b0 = (1.0 + alpha * a) / a0;
        self.b1 = -2.0 * cs / a0;
        self.b2 = (1.0 - alpha * a) / a0;
        self.a1 = self.b1;
        self.a2 = (1.0 - alpha / a) / a0;
    }
    #[inline]
    fn tick(&mut self, ch: usize, x: f32) -> f32 {
        let y = self.b0 * x + self.z1[ch];
        self.z1[ch] = self.b1 * x - self.a1 * y + self.z2[ch];
        self.z2[ch] = self.b2 * x - self.a2 * y + 1e-20; // anti-denormal
        y
    }
}

const SUB: usize = 32; // coefficient update interval

struct Filter {
    sr: f32,
    bq: Biquad,
    freq: Smooth,
    gain: Smooth,
}
impl Filter {
    fn new(sr: f32) -> Self {
        // smoothing evaluated once per SUB samples
        Self { sr, bq: Biquad::default(), freq: Smooth::new(sr / SUB as f32, 0.02, 1000.0), gain: Smooth::new(sr / SUB as f32, 0.02, 0.0) }
    }
}
impl Processor for Filter {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        let mode = p[0].get() as u32;
        let (tf, q, tg) = (p[1].get(), p[2].get(), p[3].get());
        let mut s = 0;
        while s < n {
            let end = (s + SUB).min(n);
            let f = self.freq.next(tf);
            let g = self.gain.next(tg);
            self.bq.set(mode, self.sr, f, q, g);
            for k in s..end {
                o[0][k] = self.bq.tick(0, i[0][k]);
                o[1][k] = self.bq.tick(1, i[1][k]);
            }
            s = end;
        }
    }
}

// ---------------------------------------------------------------- Drive
struct Drive {
    sr: f32,
    lp: [f32; 2],
    pre: Smooth,
    post: Smooth,
}
impl Drive {
    fn new(sr: f32) -> Self {
        Self { sr, lp: [0.0; 2], pre: Smooth::new(sr, 0.01, 1.0), post: Smooth::new(sr, 0.01, 0.5) }
    }
}
impl Processor for Drive {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        let pre_t = db2lin(p[0].get());
        let a = 1.0 - (-2.0 * PI * p[1].get() / self.sr).exp();
        let post_t = db2lin(p[2].get());
        let mix = p[3].get();
        for s in 0..n {
            let pre = self.pre.next(pre_t);
            let post = self.post.next(post_t);
            for ch in 0..2 {
                let x = i[ch][s];
                let y = (x * pre).tanh();
                self.lp[ch] += (y - self.lp[ch]) * a;
                o[ch][s] = x * (1.0 - mix) + self.lp[ch] * post * mix;
            }
        }
    }
}

// ---------------------------------------------------------------- Delay
struct Delay {
    sr: f32,
    buf: [Vec<f32>; 2],
    w: usize,
    time: Smooth,
    lp: [f32; 2],
}
impl Delay {
    fn new(sr: f32) -> Self {
        let len = (sr * 2.1) as usize + 4;
        Self { sr, buf: [vec![0.0; len], vec![0.0; len]], w: 0, time: Smooth::new(sr, 0.15, sr * 0.35), lp: [0.0; 2] }
    }
}
impl Processor for Delay {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        let len = self.buf[0].len();
        let target = (p[0].get() * 0.001 * self.sr).clamp(1.0, (len - 3) as f32);
        let fb = p[1].get();
        let a = 1.0 - (-2.0 * PI * p[2].get() / self.sr).exp();
        let mix = p[3].get();
        for s in 0..n {
            let d = self.time.next(target);
            let rp = self.w as f32 + len as f32 - d;
            let r0 = rp.floor();
            let frac = rp - r0;
            let r0 = r0 as usize % len;
            let r1 = (r0 + 1) % len;
            for ch in 0..2 {
                let b = &mut self.buf[ch];
                let wet = b[r0] + (b[r1] - b[r0]) * frac;
                self.lp[ch] += (wet - self.lp[ch]) * a;
                let x = i[ch][s];
                b[self.w] = x + self.lp[ch] * fb + 1e-20;
                o[ch][s] = x * (1.0 - mix) + wet * mix;
            }
            self.w = (self.w + 1) % len;
        }
    }
}

// ---------------------------------------------------------------- Tremolo
struct Tremolo {
    sr: f32,
    phase: f32,
}
impl Processor for Tremolo {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        let inc = p[0].get() / self.sr;
        let depth = p[1].get();
        for s in 0..n {
            let lfo = 0.5 + 0.5 * (2.0 * PI * self.phase).sin();
            let g = 1.0 - depth * lfo;
            o[0][s] = i[0][s] * g;
            o[1][s] = i[1][s] * g;
            self.phase += inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

// ---------------------------------------------------------------- 31-band graphic EQ
/// ISO 1/3-octave graphic EQ (DigiLingua core). Bands at 0 dB are skipped,
/// so a flat EQ is bit-transparent and costs almost nothing.
struct Eq31 {
    bands: [Biquad; 31],
    /// (cos w, alpha) per band; alpha < 0 marks a band above Nyquist.
    coef: [(f32, f32); 31],
    cur: [f32; 31],
    sm: f32,
    out: Smooth,
}
impl Eq31 {
    fn new(sr: f32) -> Self {
        let mut coef = [(0.0, -1.0); 31];
        for (c, &f) in coef.iter_mut().zip(ISO_FREQS.iter()) {
            if f < sr * 0.45 {
                let w = 2.0 * PI * f / sr;
                *c = (w.cos(), w.sin() / (2.0 * THIRD_OCT_Q));
            }
        }
        let mut bands = [Biquad::default(); 31];
        for (b, c) in bands.iter_mut().zip(coef.iter()) {
            b.set_peak(c.0, c.1.max(1e-4), 0.0);
        }
        Self { bands, coef, cur: [0.0; 31], sm: 1.0 - (-(SUB as f32) / (0.02 * sr)).exp(), out: Smooth::new(sr, 0.01, 1.0) }
    }
}
impl Processor for Eq31 {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        o[0][..n].copy_from_slice(&i[0][..n]);
        o[1][..n].copy_from_slice(&i[1][..n]);
        let mut s = 0;
        while s < n {
            let end = (s + SUB).min(n);
            for b in 0..31 {
                let (cs, alpha) = self.coef[b];
                if alpha < 0.0 {
                    continue;
                }
                let target = p[b].get();
                let g = &mut self.cur[b];
                if (target - *g).abs() > 1e-4 {
                    *g += (target - *g) * self.sm;
                    if (target - *g).abs() <= 1e-4 {
                        *g = target;
                    }
                    self.bands[b].set_peak(cs, alpha, *g);
                }
                if g.abs() < 1e-3 {
                    continue;
                }
                let bq = &mut self.bands[b];
                for ch in 0..2 {
                    for v in o[ch][s..end].iter_mut() {
                        *v = bq.tick(ch, *v);
                    }
                }
            }
            s = end;
        }
        let target = db2lin(p[31].get());
        for k in 0..n {
            let g = self.out.next(target);
            o[0][k] *= g;
            o[1][k] *= g;
        }
    }
}

// ---------------------------------------------------------------- DAF (delayed auditory feedback)
/// Long clean delay (0–5 s) without feedback. With dry = 0 the speaker hears
/// only the delayed voice (no echo) — the classic DAF setting.
struct Daf {
    sr: f32,
    buf: [Vec<f32>; 2],
    w: usize,
    time: Smooth,
    dry: Smooth,
    wet: Smooth,
}
impl Daf {
    fn new(sr: f32) -> Self {
        let len = (sr * 5.05) as usize + 4;
        Self {
            sr,
            buf: [vec![0.0; len], vec![0.0; len]],
            w: 0,
            time: Smooth::new(sr, 0.05, sr * 0.15),
            dry: Smooth::new(sr, 0.01, 0.0),
            wet: Smooth::new(sr, 0.01, 1.0),
        }
    }
}

/// Write-then-read fractional tap: `d` samples behind the write head `w`.
#[inline]
fn tap(b: &[f32], w: usize, d: f32) -> f32 {
    let len = b.len();
    let rp = w as f32 + len as f32 - d;
    let r0 = rp.floor();
    let frac = rp - r0;
    let r0 = r0 as usize % len;
    let r1 = (r0 + 1) % len;
    b[r0] + (b[r1] - b[r0]) * frac
}

impl Processor for Daf {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        let len = self.buf[0].len();
        let target = (p[0].get() * 0.001 * self.sr).clamp(0.0, (len - 3) as f32);
        let (dry_t, wet_t) = (p[1].get(), p[2].get());
        for s in 0..n {
            let d = self.time.next(target);
            let dry = self.dry.next(dry_t);
            let wet = self.wet.next(wet_t);
            for ch in 0..2 {
                let x = i[ch][s];
                self.buf[ch][self.w] = x;
                o[ch][s] = x * dry + tap(&self.buf[ch], self.w, d) * wet;
            }
            self.w = (self.w + 1) % len;
        }
    }
}

// ---------------------------------------------------------------- Pitch shift (FAF)
/// Two-tap rotating-delay pitch shifter with power-complementary crossfade.
/// Used as FAF (frequency-altered feedback) in DigiLingua and as a
/// harmonizer/octaver for musicians.
struct PitchShift {
    sr: f32,
    buf: [Vec<f32>; 2],
    w: usize,
    phase: f32,
    mix: Smooth,
}
impl PitchShift {
    fn new(sr: f32) -> Self {
        let len = (sr * 0.12) as usize + 4;
        Self { sr, buf: [vec![0.0; len], vec![0.0; len]], w: 0, phase: 0.0, mix: Smooth::new(sr, 0.01, 1.0) }
    }
}
impl Processor for PitchShift {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        let len = self.buf[0].len();
        let ratio = 2f32.powf(p[0].get() / 12.0);
        let win = (p[1].get() * 0.001 * self.sr).clamp(8.0, (len - 4) as f32);
        let inc = (1.0 - ratio) / win;
        let mix_t = p[2].get();
        for s in 0..n {
            let mix = self.mix.next(mix_t);
            let ph2 = if self.phase >= 0.5 { self.phase - 0.5 } else { self.phase + 0.5 };
            let (g1, g2) = ((PI * self.phase).sin(), (PI * ph2).sin());
            for ch in 0..2 {
                let x = i[ch][s];
                self.buf[ch][self.w] = x;
                let b = &self.buf[ch];
                let wet = tap(b, self.w, self.phase * win) * g1 + tap(b, self.w, ph2 * win) * g2;
                o[ch][s] = x * (1.0 - mix) + wet * mix;
            }
            self.w = (self.w + 1) % len;
            self.phase += inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            } else if self.phase < 0.0 {
                self.phase += 1.0;
            }
        }
    }
}

// ---------------------------------------------------------------- Noise
/// Masking / training noise added to the signal. `level` is RMS in dBFS.
struct Noise {
    rng: [u32; 2],
    pink: [[f32; 7]; 2],
    brown: [f32; 2],
    bp: Biquad,
    sr: f32,
    level: Smooth,
    route: [Smooth; 2],
}
impl Noise {
    fn new(sr: f32) -> Self {
        Self {
            rng: [0x9E37_79B9, 0x85EB_CA6B],
            pink: [[0.0; 7]; 2],
            brown: [0.0; 2],
            bp: Biquad::default(),
            sr,
            level: Smooth::new(sr, 0.02, 0.0),
            route: [Smooth::new(sr, 0.02, 1.0), Smooth::new(sr, 0.02, 1.0)],
        }
    }
    /// Uniform white noise in [-1, 1) (xorshift32, allocation-free).
    #[inline]
    fn white(&mut self, ch: usize) -> f32 {
        let mut x = self.rng[ch];
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng[ch] = x;
        (x as f32 / 2_147_483_648.0) - 1.0
    }
}
// Calibrated so every colour has RMS ≈ 1.0 before `level` (see tests).
const WHITE_NORM: f32 = 1.732;
const PINK_NORM: f32 = 5.19;
const BROWN_NORM: f32 = 4.98;
impl Processor for Noise {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        let color = p[0].get() as u32;
        let lvl_t = db2lin(p[1].get());
        let route = p[3].get() as u32;
        let rt = [if route == 2 { 0.0 } else { 1.0 }, if route == 1 { 0.0 } else { 1.0 }];
        let nb_norm = if color == 3 {
            let f = p[2].get().clamp(20.0, self.sr * 0.45);
            self.bp.set(2, self.sr, f, THIRD_OCT_Q, 0.0);
            // A 2-pole band-pass passes π/2·f/Q of the sr/2 white-noise bandwidth.
            WHITE_NORM * (self.sr * THIRD_OCT_Q / (PI * f)).sqrt()
        } else {
            0.0
        };
        for s in 0..n {
            let lvl = self.level.next(lvl_t);
            for ch in 0..2 {
                let w = self.white(ch);
                let v = match color {
                    0 => w * WHITE_NORM,
                    1 => {
                        // Paul Kellet's refined pink filter
                        let b = &mut self.pink[ch];
                        b[0] = 0.99886 * b[0] + w * 0.0555179;
                        b[1] = 0.99332 * b[1] + w * 0.0750759;
                        b[2] = 0.96900 * b[2] + w * 0.1538520;
                        b[3] = 0.86650 * b[3] + w * 0.3104856;
                        b[4] = 0.55000 * b[4] + w * 0.5329522;
                        b[5] = -0.7616 * b[5] - w * 0.0168980;
                        let y = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
                        b[6] = w * 0.115926;
                        y * 0.11 * PINK_NORM
                    }
                    2 => {
                        let b = &mut self.brown[ch];
                        *b = (*b + 0.02 * w) / 1.02;
                        *b * 3.5 * BROWN_NORM
                    }
                    _ => self.bp.tick(ch, w) * nb_norm,
                };
                let g = self.route[ch].next(rt[ch]);
                o[ch][s] = i[ch][s] + v * lvl * g;
            }
        }
    }
}

// ---------------------------------------------------------------- Interrupter
/// Periodically mutes (or ducks) the signal: DigiLingua "discontinuity"
/// filter, stutter/gate effect for musicians. Click-free via fade.
struct Interrupter {
    sr: f32,
    pos: f32,
    g: f32,
}
impl Interrupter {
    fn new(sr: f32) -> Self {
        Self { sr, pos: 0.0, g: 1.0 }
    }
}
impl Processor for Interrupter {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        let period = p[0].get() * 0.001 * self.sr;
        let gap = (p[1].get() * 0.001 * self.sr).min(period * 0.95);
        let depth = p[2].get();
        let coef = 1.0 - (-1.0 / (p[3].get() * 0.001 * self.sr * 0.25)).exp();
        for s in 0..n {
            if self.pos >= period {
                self.pos -= period;
                if self.pos >= period {
                    self.pos = 0.0;
                }
            }
            // gap sits at the end of each period so a fresh start is audible first
            let target = if self.pos >= period - gap { 1.0 - depth } else { 1.0 };
            self.g += (target - self.g) * coef;
            o[0][s] = i[0][s] * self.g;
            o[1][s] = i[1][s] * self.g;
            self.pos += 1.0;
        }
    }
}

// ---------------------------------------------------------------- Channels / balance
/// Input channel selection (mono mic on one channel of a stereo interface)
/// and independent left/right level, 0–200 %.
struct Channels {
    l: Smooth,
    r: Smooth,
}
impl Channels {
    fn new(sr: f32) -> Self {
        Self { l: Smooth::new(sr, 0.01, 1.0), r: Smooth::new(sr, 0.01, 1.0) }
    }
}
impl Processor for Channels {
    fn process(&mut self, p: &[AtomicF32], i: &Buf, o: &mut Buf, n: usize) {
        let src = p[0].get() as u32;
        let (lt, rt) = (p[1].get() * 0.01, p[2].get() * 0.01);
        for s in 0..n {
            let (a, b) = (i[0][s], i[1][s]);
            let (l, r) = match src {
                1 => (a, a),
                2 => (b, b),
                3 => {
                    let m = (a + b) * 0.5;
                    (m, m)
                }
                _ => (a, b),
            };
            o[0][s] = l * self.l.next(lt);
            o[1][s] = r * self.r.next(rt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn params(kind: NodeKind) -> Vec<AtomicF32> {
        kind.params().iter().map(|s| AtomicF32::new(s.default)).collect()
    }
    fn set(p: &[AtomicF32], kind: NodeKind, id: &str, v: f32) {
        let i = kind.params().iter().position(|s| s.id == id).unwrap();
        p[i].set(v);
    }
    /// Run `f(sample_index)` mono-in-both-channels through a node, return left output.
    fn run(kind: NodeKind, p: &[AtomicF32], len: usize, f: impl Fn(usize) -> f32) -> Vec<f32> {
        let mut proc = kind.create(SR);
        let (mut i, mut o) = (new_buf(), new_buf());
        let mut out = Vec::with_capacity(len);
        let mut k = 0;
        while k < len {
            let n = (len - k).min(MAX_BLOCK);
            for s in 0..n {
                i[0][s] = f(k + s);
                i[1][s] = f(k + s);
            }
            proc.process(p, &i, &mut o, n);
            out.extend_from_slice(&o[0][..n]);
            k += n;
        }
        out
    }
    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }
    fn sine(f: f32) -> impl Fn(usize) -> f32 {
        move |k| (2.0 * PI * f * k as f32 / SR).sin() * 0.5
    }

    #[test]
    fn eq31_flat_is_transparent() {
        let p = params(NodeKind::Eq31);
        let y = run(NodeKind::Eq31, &p, 4800, sine(440.0));
        for (k, v) in y.iter().enumerate().skip(2000) {
            assert!((v - sine(440.0)(k)).abs() < 1e-5);
        }
    }

    #[test]
    fn eq31_band_boost_hits_its_band_only() {
        let p = params(NodeKind::Eq31);
        set(&p, NodeKind::Eq31, "b1k", 12.0);
        let at = |f| rms(&run(NodeKind::Eq31, &p, 48000, sine(f))[24000..]) / (0.5 / 2f32.sqrt());
        let g1k = 20.0 * at(1000.0).log10();
        let g100 = 20.0 * at(100.0).log10();
        assert!((g1k - 12.0).abs() < 0.5, "1k gain {g1k}");
        assert!(g100.abs() < 0.3, "100 Hz gain {g100}");
    }

    #[test]
    fn daf_delays_exactly_and_mutes_dry() {
        let p = params(NodeKind::Daf);
        set(&p, NodeKind::Daf, "time", 100.0); // 4800 samples
        let mut proc = NodeKind::Daf.create(SR);
        let (mut i, mut o) = (new_buf(), new_buf());
        // let the time smoother settle, input silent
        for _ in 0..200 {
            proc.process(&p, &i, &mut o, 256);
        }
        let mut out = vec![];
        for blk in 0..40 {
            i[0][..256].fill(0.0);
            if blk == 0 {
                i[0][0] = 1.0;
            }
            proc.process(&p, &i, &mut o, 256);
            out.extend_from_slice(&o[0][..256]);
        }
        assert!(out[0].abs() < 1e-6, "dry must be muted");
        let peak = out.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap();
        assert!((peak.0 as i32 - 4800).abs() <= 1 && *peak.1 > 0.9, "{peak:?}");
    }

    #[test]
    fn pitch_shift_octave_up() {
        let p = params(NodeKind::PitchShift);
        set(&p, NodeKind::PitchShift, "semi", 12.0);
        let y = run(NodeKind::PitchShift, &p, 48000, sine(200.0));
        // upward zero crossings in the second half: 400 Hz -> 200 per 0.5 s
        let zc = y[24000..].windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((190..=210).contains(&zc), "zero crossings {zc}");
    }

    #[test]
    fn noise_levels_are_calibrated() {
        for color in 0..4 {
            let p = params(NodeKind::Noise);
            set(&p, NodeKind::Noise, "color", color as f32);
            set(&p, NodeKind::Noise, "level", -20.0);
            let y = run(NodeKind::Noise, &p, 48000 * 4, |_| 0.0);
            let db = 20.0 * rms(&y[48000..]).log10();
            assert!((db + 20.0).abs() < 1.5, "color {color}: {db} dBFS");
        }
    }

    #[test]
    fn noise_routes_to_one_ear() {
        let p = params(NodeKind::Noise);
        set(&p, NodeKind::Noise, "route", 2.0); // right only
        let mut proc = NodeKind::Noise.create(SR);
        let (i, mut o) = (new_buf(), new_buf());
        for _ in 0..100 {
            proc.process(&p, &i, &mut o, 512);
        }
        assert!(rms(&o[0][..512]) < 1e-4 && rms(&o[1][..512]) > 0.01);
    }

    #[test]
    fn interrupter_mutes_during_gap() {
        let p = params(NodeKind::Interrupter);
        set(&p, NodeKind::Interrupter, "period", 100.0);
        set(&p, NodeKind::Interrupter, "gap", 50.0);
        let y = run(NodeKind::Interrupter, &p, 9600, |_| 1.0);
        assert!(y[4800 + 1000] > 0.99, "open part");
        assert!(y[4800 + 2400 + 1000].abs() < 1e-3, "gap part");
    }

    #[test]
    fn channels_select_left_and_balance() {
        let p = params(NodeKind::Channels);
        set(&p, NodeKind::Channels, "source", 1.0);
        set(&p, NodeKind::Channels, "right", 50.0);
        let mut proc = NodeKind::Channels.create(SR);
        let (mut i, mut o) = (new_buf(), new_buf());
        i[0].fill(0.8);
        i[1].fill(0.0);
        for _ in 0..50 {
            proc.process(&p, &i, &mut o, 512);
        }
        assert!((o[0][511] - 0.8).abs() < 1e-3 && (o[1][511] - 0.4).abs() < 1e-3);
    }
}
