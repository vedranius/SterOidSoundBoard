//! Live analysis of the monitored signal for the real-time sonagram:
//! a spectrum every 10 ms (Gaussian window, Praat's wide/narrow band
//! lengths), plus F0 (single-frame autocorrelation, Praat-style
//! normalisation and octave cost) and intensity on Praat's dB scale.
//! Control thread only (fed from the wait-free scope ring).
use crate::fft::Fft;
use serde::Serialize;

/// dB value of spectrum byte 0; one byte step = 0.5 dB.
pub const SPEC_DB_MIN: f32 = -120.0;
pub const SPEC_DB_STEP: f32 = 0.5;
const HOP_S: f64 = 0.010;
const F0_FLOOR: f32 = 75.0;
const F0_CEILING: f32 = 600.0;
const VOICING: f32 = 0.45;
const SILENCE: f32 = 0.03;
const OCTAVE_COST: f32 = 0.01;
const MAX_FRAMES_PER_CALL: usize = 30;

#[derive(Debug, Clone, Serialize)]
pub struct LiveFrame {
    /// Spectrum bytes: dB = SPEC_DB_MIN + b · SPEC_DB_STEP (0 dB ≈ full-scale sine).
    #[serde(skip)]
    pub spec: Vec<u8>,
    /// F0 in Hz, 0 when unvoiced.
    pub f0: f32,
    /// Intensity, Praat dB scale (sample 1.0 = 1 Pa).
    pub db: f32,
    /// Lowest and highest sample of this 10 ms hop (live waveform).
    pub lo: f32,
    pub hi: f32,
}

pub struct LiveAnalyzer {
    sr: f32,
    ring: Vec<f32>,
    pos: usize,
    total: u64,
    next: u64,
    hop: u64,
    /// Effective (Praat) window length in seconds: 0.005 wide band, 0.03 narrow band.
    window_s: f32,
    fmax: f32,
    fft: Fft,
    spec_win: Vec<f32>,
    f0_win: Vec<f32>,
    f0_win_r: Vec<f32>,
    f0_fft: Fft,
    running_peak: f32,
}

impl LiveAnalyzer {
    pub fn new(sr: f32) -> Self {
        let mut a = LiveAnalyzer {
            sr,
            ring: vec![0.0; (sr as usize).max(4096)],
            pos: 0,
            total: 0,
            next: 0,
            hop: ((sr as f64 * HOP_S) as u64).max(1),
            window_s: 0.005,
            fmax: 12000.0,
            fft: Fft::new(if sr > 50_000.0 { 4096 } else { 2048 }),
            spec_win: vec![],
            f0_win: vec![],
            f0_win_r: vec![],
            f0_fft: Fft::new(2),
            running_peak: 1e-6,
        };
        a.rebuild_windows();
        a
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    pub fn set_window(&mut self, seconds: f32) {
        let w = seconds.clamp(0.002, 0.05);
        if (w - self.window_s).abs() > 1e-6 {
            self.window_s = w;
            self.rebuild_windows();
        }
    }

    pub fn window(&self) -> f32 {
        self.window_s
    }

    fn rebuild_windows(&mut self) {
        // Praat Gaussian spectrogram window: physical length = 2 × effective
        let n = ((2.0 * self.window_s * self.sr) as usize).clamp(16, self.fft.len());
        let imid = 0.5 * (n as f32 + 1.0);
        let edge = (-12.0f32).exp();
        self.spec_win = (1..=n).map(|i| (((-48.0 * ((i as f32 - imid) / (n as f32 + 1.0)).powi(2)).exp()) - edge) / (1.0 - edge)).collect();
        let nf = ((3.0 / F0_FLOOR) * self.sr) as usize;
        self.f0_win = crate::fft::hann(nf);
        self.f0_fft = Fft::new((2 * nf).next_power_of_two());
        let mut r = vec![];
        self.f0_fft.autocorr(&self.f0_win, &mut r);
        let r0 = r[0];
        self.f0_win_r = r.iter().map(|v| v / r0).collect();
    }

    /// Frequency resolution (Hz per bin) and number of bins sent per frame.
    pub fn bins(&self) -> (f32, usize) {
        let df = self.sr / self.fft.len() as f32;
        (df, ((self.fmax.min(0.5 * self.sr)) / df) as usize)
    }

    pub fn hop_seconds(&self) -> f32 {
        self.hop as f32 / self.sr
    }

    pub fn push(&mut self, x: &[f32]) {
        for &v in x {
            self.ring[self.pos] = v;
            self.pos = (self.pos + 1) % self.ring.len();
        }
        self.total += x.len() as u64;
    }

    /// Last `n` samples ending `back` samples before the newest one.
    fn last(&self, n: usize, back: usize, out: &mut Vec<f32>) {
        out.clear();
        let len = self.ring.len();
        let end = (self.pos + len - back % len) % len;
        for k in 0..n {
            out.push(self.ring[(end + len - n + k) % len]);
        }
    }

    /// New frames since the previous call (at most MAX_FRAMES_PER_CALL; older ones are skipped).
    pub fn frames(&mut self) -> Vec<LiveFrame> {
        let need = self.f0_win.len().max(self.spec_win.len()) as u64;
        if self.total < need {
            return vec![];
        }
        if self.next + self.hop * MAX_FRAMES_PER_CALL as u64 <= self.total {
            self.next = self.total - self.hop * MAX_FRAMES_PER_CALL as u64 + self.hop;
        }
        let mut out = vec![];
        let mut buf = Vec::with_capacity(need as usize);
        while self.next + self.hop <= self.total {
            self.next += self.hop;
            let back = (self.total - self.next) as usize;
            out.push(self.frame(back, &mut buf));
        }
        out
    }

    fn frame(&mut self, back: usize, buf: &mut Vec<f32>) -> LiveFrame {
        // --- waveform envelope of the newest hop
        self.last(self.hop as usize, back, buf);
        let (lo, hi) = buf.iter().fold((0f32, 0f32), |(a, b), &v| (a.min(v), b.max(v)));
        // --- spectrum
        let n = self.spec_win.len();
        self.last(n, back, buf);
        let mean = buf.iter().sum::<f32>() / n as f32;
        let nfft = self.fft.len();
        let mut re = vec![0.0f32; nfft];
        let mut im = vec![0.0f32; nfft];
        let wsum: f32 = self.spec_win.iter().sum();
        for k in 0..n {
            re[k] = (buf[k] - mean) * self.spec_win[k];
        }
        self.fft.forward(&mut re, &mut im);
        let (_, nb) = self.bins();
        let norm = 2.0 / wsum; // full-scale sine → 0 dB
        let spec = (0..nb)
            .map(|k| {
                let m = (re[k] * re[k] + im[k] * im[k]).sqrt() * norm;
                let db = 20.0 * (m + 1e-9).log10();
                ((db - SPEC_DB_MIN) / SPEC_DB_STEP).round().clamp(0.0, 255.0) as u8
            })
            .collect();
        // --- intensity and F0 over 3 periods of the floor
        let nf = self.f0_win.len();
        self.last(nf, back, buf);
        let mean = buf.iter().sum::<f32>() / nf as f32;
        let (mut sw, mut swx, mut peak) = (0.0f32, 0.0f32, 0.0f32);
        for (k, v) in buf.iter_mut().enumerate() {
            let x = *v - mean;
            peak = peak.max(x.abs());
            let w = self.f0_win[k];
            sw += w;
            swx += w * x * x;
            *v = x * w;
        }
        let ms = swx / sw.max(1e-12);
        let db = if ms > 0.0 { 10.0 * (ms / 4.0e-10).log10() } else { 0.0 };
        // running peak for the silence threshold (slow decay ≈ 10 s)
        self.running_peak = (self.running_peak * 0.999).max(peak).max(1e-6);
        let mut f0 = 0.0;
        if peak >= SILENCE * self.running_peak {
            let mut ac = vec![];
            self.f0_fft.autocorr(buf, &mut ac);
            if ac[0] > 0.0 {
                let r = |k: usize| ac[k] / ac[0] / self.f0_win_r[k].max(1e-6);
                let (lo, hi) = (((self.sr / F0_CEILING) as usize).max(2), ((self.sr / F0_FLOOR) as usize).min(nf / 3));
                let mut best = (f32::MIN, 0.0f32, 0.0f32);
                for k in lo..hi {
                    let (a, b, c) = (r(k - 1), r(k), r(k + 1));
                    if b > a && b >= c && b > 0.5 * VOICING {
                        let den = a - 2.0 * b + c;
                        let d = if den.abs() > 1e-12 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
                        let tau = k as f32 + d;
                        let rr = (b - 0.25 * (a - c) * d).min(1.0);
                        let strength = rr - OCTAVE_COST * (F0_CEILING * tau / self.sr).log2();
                        if strength > best.0 {
                            best = (strength, tau, rr);
                        }
                    }
                }
                if best.2 > VOICING {
                    f0 = self.sr / best.1;
                }
            }
        }
        LiveFrame { spec, f0, db, lo, hi }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn frames_every_10ms_with_f0_and_peak_bin() {
        let sr = 48000.0;
        let mut a = LiveAnalyzer::new(sr);
        a.set_window(0.03); // narrow band resolves the harmonics
        let x: Vec<f32> = (0..48000)
            .map(|i| (1..=8).map(|h| (2.0 * PI * 150.0 * h as f32 * i as f32 / sr).sin() / h as f32).sum::<f32>() * 0.3)
            .collect();
        let mut frames = vec![];
        for chunk in x.chunks(2400) {
            a.push(chunk);
            frames.extend(a.frames());
        }
        assert!((90..=100).contains(&frames.len()), "{}", frames.len());
        let last = frames.last().unwrap();
        assert!((last.f0 - 150.0).abs() < 1.5, "f0 {}", last.f0);
        let (df, nb) = a.bins();
        assert_eq!(last.spec.len(), nb);
        let k = last.spec.iter().enumerate().max_by_key(|(_, b)| **b).unwrap().0;
        assert!((k as f32 * df - 150.0).abs() < 2.0 * df + 60.0, "peak {}", k as f32 * df);
        assert!(last.db > 60.0 && last.db < 95.0, "db {}", last.db);
    }

    #[test]
    fn silence_is_unvoiced_and_skips_backlog() {
        let mut a = LiveAnalyzer::new(44100.0);
        a.push(&vec![0.0; 44100 * 3]);
        let f = a.frames();
        assert!(f.len() <= MAX_FRAMES_PER_CALL);
        assert!(f.iter().all(|x| x.f0 == 0.0));
    }
}
