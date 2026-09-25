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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Gain,
    Filter,
    Drive,
    Delay,
    Tremolo,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub kind: NodeKind,
    pub name: &'static str,
    pub category: &'static str,
    pub params: &'static [ParamSpec],
}

impl NodeKind {
    pub const ALL: &'static [NodeKind] =
        &[NodeKind::Gain, NodeKind::Filter, NodeKind::Drive, NodeKind::Delay, NodeKind::Tremolo];

    pub fn info(self) -> NodeInfo {
        let (name, category) = match self {
            NodeKind::Gain => ("Gain", "Utility"),
            NodeKind::Filter => ("Filter / EQ", "Filter"),
            NodeKind::Drive => ("Drive", "Distortion"),
            NodeKind::Delay => ("Delay", "Time"),
            NodeKind::Tremolo => ("Tremolo", "Modulation"),
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

/// One-pole parameter smoother.
struct Smooth {
    cur: f32,
    coef: f32,
}
impl Smooth {
    fn new(sr: f32, secs: f32, init: f32) -> Self {
        Self { cur: init, coef: 1.0 - (-1.0 / (secs * sr)).exp() }
    }
    #[inline]
    fn next(&mut self, target: f32) -> f32 {
        self.cur += (target - self.cur) * self.coef;
        self.cur
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
