//! Formants by Burg LPC (Praat `Sound: To Formant (burg)…`): resample to
//! twice the maximum formant, pre-emphasise from 50 Hz, Gaussian window,
//! Burg coefficients, polynomial roots → frequencies and bandwidths.
use super::Sound;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FormantParams {
    /// Praat: 5500 Hz for adult female/child voices, 5000 Hz for adult male.
    pub max_formant: f64,
    pub n_formants: f64,
    /// Praat "window length" (the Gaussian window is twice as long).
    pub window: f64,
    pub pre_emphasis: f64,
    /// 0 = window / 4.
    pub time_step: f64,
}

impl Default for FormantParams {
    fn default() -> Self {
        FormantParams { max_formant: 5500.0, n_formants: 5.0, window: 0.025, pre_emphasis: 50.0, time_step: 0.0 }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FormantFrame {
    pub t: f64,
    /// (frequency, bandwidth) sorted by frequency.
    pub formants: Vec<(f64, f64)>,
    /// Max squared sample in the window (Praat's frame "intensity").
    pub intensity: f64,
}

/// Burg's method: `m` prediction coefficients a[1..m] (Praat `VECburg`).
pub fn burg(x: &[f64], m: usize) -> Vec<f64> {
    let n = x.len();
    let mut a = vec![0.0; m + 1];
    if n <= 2 {
        return a;
    }
    let mut b1 = vec![0.0; n + 1];
    let mut b2 = vec![0.0; n + 1];
    let mut aa = vec![0.0; m + 1];
    let p: f64 = x.iter().map(|v| v * v).sum();
    if p <= 0.0 {
        return a;
    }
    // 1-based as in the reference implementation
    b1[1] = x[0];
    b2[n - 1] = x[n - 1];
    for j in 2..=n - 1 {
        b1[j] = x[j - 1];
        b2[j - 1] = x[j - 1];
    }
    for i in 1..=m {
        let (mut num, mut den) = (0.0, 0.0);
        for j in 1..=n - i {
            num += b1[j] * b2[j];
            den += b1[j] * b1[j] + b2[j] * b2[j];
        }
        if den <= 0.0 {
            return a;
        }
        a[i] = 2.0 * num / den;
        for j in 1..i {
            a[j] = aa[j] - a[i] * aa[i - j];
        }
        if i < m {
            aa[1..=i].copy_from_slice(&a[1..=i]);
            for j in 1..=n - i - 1 {
                b1[j] -= aa[i] * b2[j];
                b2[j] = b2[j + 1] - aa[i] * b1[j + 1];
            }
        }
    }
    a
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct C(f64, f64);
impl C {
    fn add(self, o: C) -> C {
        C(self.0 + o.0, self.1 + o.1)
    }
    fn sub(self, o: C) -> C {
        C(self.0 - o.0, self.1 - o.1)
    }
    fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    fn div(self, o: C) -> C {
        let d = o.0 * o.0 + o.1 * o.1;
        C((self.0 * o.0 + self.1 * o.1) / d, (self.1 * o.0 - self.0 * o.1) / d)
    }
    fn abs(self) -> f64 {
        self.0.hypot(self.1)
    }
}

/// Roots of the monic polynomial z^m + c[m-1] z^(m-1) + … + c[0]
/// (Aberth–Ehrlich iteration).
fn roots(c: &[f64]) -> Vec<C> {
    let m = c.len();
    let eval = |z: C| -> (C, C) {
        // Horner for p and p'
        let (mut p, mut dp) = (C(1.0, 0.0), C(0.0, 0.0));
        for k in (0..m).rev() {
            dp = dp.mul(z).add(p);
            p = p.mul(z).add(C(c[k], 0.0));
        }
        (p, dp)
    };
    let radius = c.iter().fold(0f64, |r, v| r.max(v.abs())).powf(1.0 / m as f64).clamp(0.5, 2.0);
    let mut z: Vec<C> = (0..m).map(|k| {
        let a = 2.0 * PI * k as f64 / m as f64 + 0.4;
        C(radius * a.cos(), radius * a.sin())
    }).collect();
    for _ in 0..500 {
        let mut worst = 0f64;
        for i in 0..m {
            let (p, dp) = eval(z[i]);
            if p.abs() == 0.0 {
                continue;
            }
            let ratio = p.div(dp);
            let mut s = C(0.0, 0.0);
            for j in 0..m {
                if j != i {
                    s = s.add(C(1.0, 0.0).div(z[i].sub(z[j])));
                }
            }
            let w = ratio.div(C(1.0, 0.0).sub(ratio.mul(s)));
            z[i] = z[i].sub(w);
            worst = worst.max(w.abs());
        }
        if worst < 1e-14 {
            break;
        }
    }
    z
}

/// Frequencies/bandwidths from LPC coefficients (Praat `burg` → roots).
fn lpc_formants(a: &[f64], nyquist: f64, safety: f64) -> Vec<(f64, f64)> {
    let m = a.len() - 1;
    // polynomial z^m − a1 z^(m−1) − … − am  (c[k] is the z^k coefficient)
    let c: Vec<f64> = (0..m).map(|k| -a[m - k]).collect();
    let mut out: Vec<(f64, f64)> = roots(&c)
        .into_iter()
        .map(|z| {
            let r = z.abs();
            if r > 1.0 { C(z.0 / (r * r), z.1 / (r * r)) } else { z } // fix into the unit circle
        })
        .filter(|z| z.1 >= 0.0)
        .filter_map(|z| {
            let f = z.1.atan2(z.0).abs() * nyquist / PI;
            let bw = -z.abs().ln() * nyquist / PI;
            (f >= safety && f <= nyquist - safety).then_some((f, bw))
        })
        .collect();
    out.sort_by(|x, y| x.0.total_cmp(&y.0));
    out
}

pub fn to_formants(s: &Sound, p: &FormantParams) -> Vec<FormantFrame> {
    let mut r = s.resample(2.0 * p.max_formant);
    let nyquist = 0.5 * r.sr;
    let half = p.window;
    let dt = if p.time_step > 0.0 { p.time_step } else { half / 4.0 };
    let dt_window = 2.0 * half;
    let nsamp = (dt_window * r.sr).floor() as isize;
    let hn = nsamp / 2;
    let poles = (2.0 * p.n_formants).round() as usize;
    if nsamp < poles as isize + 1 {
        return vec![];
    }
    let dur = r.duration();
    let mut nframes = if dur >= dt_window { 1 + ((dur - dt_window) / dt).floor() as usize } else { 0 };
    let mut t1 = 0.5 * (dur - (nframes as f64 - 1.0) * dt);
    if nframes < 1 {
        nframes = 1;
        t1 = 0.5 * dur;
    }
    r.pre_emphasize(p.pre_emphasis);
    let window: Vec<f64> = (1..=nsamp)
        .map(|i| {
            let imid = 0.5 * (nsamp as f64 + 1.0);
            let edge = (-12.0f64).exp();
            ((-48.0 * (i as f64 - imid).powi(2) / (nsamp as f64 + 1.0).powi(2)).exp() - edge) / (1.0 - edge)
        })
        .collect();
    let mut frames = Vec::with_capacity(nframes);
    let mut buf = Vec::with_capacity(nsamp as usize + 2);
    for iframe in 0..nframes {
        let t = t1 + iframe as f64 * dt;
        let left = r.low_index(t);
        let right = left + 1;
        let start = (right - hn).max(0);
        let end = (left + hn).min(r.x.len() as isize - 1);
        let mut fr = FormantFrame { t, ..Default::default() };
        fr.intensity = (start..=end).map(|i| r.x[i as usize] * r.x[i as usize]).fold(0.0, f64::max);
        if fr.intensity > 0.0 && end > start {
            buf.clear();
            buf.extend((start..=end).enumerate().map(|(k, i)| r.x[i as usize] * window.get(k).copied().unwrap_or(0.0)));
            let a = burg(&buf, poles);
            fr.formants = lpc_formants(&a, nyquist, 50.0);
        }
        frames.push(fr);
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pulse train through a cascade of 2-pole resonators (Klatt-style vowel).
    fn vowel(sr: f64, secs: f64, f0: f64, formants: &[(f64, f64)]) -> Vec<f64> {
        let n = (sr * secs) as usize;
        let per = (sr / f0) as usize;
        let mut x: Vec<f64> = (0..n).map(|i| if i % per == 0 { 1.0 } else { 0.0 }).collect();
        // glottal source tilt: two one-pole low-passes (−12 dB/octave above ~100 Hz)
        for _ in 0..2 {
            let a = (-2.0 * PI * 100.0 / sr).exp();
            let mut y = 0.0;
            for v in x.iter_mut() {
                y = (1.0 - a) * *v + a * y;
                *v = y;
            }
        }
        for &(f, bw) in formants {
            let r = (-PI * bw / sr).exp();
            let (b1, b2) = (2.0 * r * (2.0 * PI * f / sr).cos(), -r * r);
            let (mut y1, mut y2) = (0.0, 0.0);
            for v in x.iter_mut() {
                let y = *v + b1 * y1 + b2 * y2;
                y2 = y1;
                y1 = y;
                *v = y;
            }
        }
        let peak = x.iter().fold(0f64, |m, v| m.max(v.abs()));
        x.iter().map(|v| v / peak * 0.5).collect()
    }

    #[test]
    fn finds_vowel_formants() {
        let sr = 44100.0;
        let want = [(700.0, 80.0), (1220.0, 90.0), (2600.0, 120.0), (3300.0, 150.0), (4400.0, 200.0)];
        let s = Sound { x: vowel(sr, 0.5, 120.0, &want), sr };
        let frames = to_formants(&s, &FormantParams::default());
        assert!(!frames.is_empty());
        for (k, &(f, _)) in want.iter().enumerate().take(3) {
            let mut v: Vec<f64> = frames.iter().filter_map(|fr| fr.formants.get(k).map(|x| x.0)).collect();
            v.sort_by(f64::total_cmp);
            let med = v[v.len() / 2];
            assert!((med - f).abs() / f < 0.05, "F{}: {med} vs {f}", k + 1);
        }
    }

    #[test]
    fn roots_of_known_polynomial() {
        // (z − 0.5)(z + 0.25)(z² + 0.81) = z⁴ − 0.25 z³ + 0.685 z² − 0.2025 z − 0.10125
        let r = roots(&[-0.10125, -0.2025, 0.685, -0.25]);
        for want in [C(0.5, 0.0), C(-0.25, 0.0), C(0.0, 0.9), C(0.0, -0.9)] {
            assert!(r.iter().any(|z| z.sub(want).abs() < 1e-9), "{want:?} not in {r:?}");
        }
    }
}
