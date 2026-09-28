//! Intensity contour (Praat `Sound: To Intensity…`): Kaiser-windowed mean
//! square, dB re 2·10⁻⁵ Pa assuming sample value 1.0 = 1 Pa — i.e. the same
//! uncalibrated scale Praat shows (full-scale sine ≈ 91 dB).
use super::{bessel_i0, Sound};

pub struct Intensity {
    pub t1: f64,
    pub dt: f64,
    pub db: Vec<f64>,
}

impl Intensity {
    pub fn time(&self, i: usize) -> f64 {
        self.t1 + i as f64 * self.dt
    }
    /// Linear interpolation at time t.
    pub fn value_at(&self, t: f64) -> Option<f64> {
        if self.db.is_empty() {
            return None;
        }
        let x = (t - self.t1) / self.dt;
        if x < -0.5 || x > self.db.len() as f64 - 0.5 {
            return None;
        }
        let i = x.floor().clamp(0.0, (self.db.len() - 1) as f64) as usize;
        let j = (i + 1).min(self.db.len() - 1);
        let f = (x - i as f64).clamp(0.0, 1.0);
        Some(self.db[i] + f * (self.db[j] - self.db[i]))
    }
}

/// `pitch_floor` sets the window (6.4 / floor s); Praat default 100 Hz.
pub fn to_intensity(s: &Sound, pitch_floor: f64, time_step: f64) -> Intensity {
    let logical = 3.2 / pitch_floor;
    let dt = if time_step > 0.0 { time_step } else { logical / 4.0 };
    let physical = 2.0 * logical;
    let half = 0.5 * physical;
    let hs = (half * s.sr).floor() as isize;
    let window: Vec<f64> = (-hs..=hs)
        .map(|k| {
            let x = k as f64 / s.sr / half;
            bessel_i0((2.0 * std::f64::consts::PI * std::f64::consts::PI + 0.5) * (1.0 - x * x).max(0.0).sqrt())
        })
        .collect();
    let (n, t1) = s.short_term(physical, dt);
    let mut db = Vec::with_capacity(n);
    let mut amp = Vec::with_capacity(window.len());
    for iframe in 0..n {
        let t = t1 + iframe as f64 * dt;
        let c = s.nearest_index(t);
        let (l, r) = ((c - hs).max(0), (c + hs).min(s.x.len() as isize - 1));
        amp.clear();
        amp.extend((l..=r).map(|i| s.x[i as usize]));
        let mean = amp.iter().sum::<f64>() / amp.len() as f64; // subtract mean pressure
        let (mut sxw, mut sw) = (0.0, 0.0);
        for (k, v) in amp.iter().enumerate() {
            let w = window[(l + k as isize - (c - hs)) as usize];
            sxw += (v - mean) * (v - mean) * w;
            sw += w;
        }
        let ms = sxw / sw;
        db.push(if ms > 0.0 { 10.0 * (ms / 4.0e-10).log10() } else { -300.0 });
    }
    Intensity { t1, dt, db }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_level_matches_praat_scale() {
        // sine of amplitude 1 has mean square 0.5 → 10·log10(0.5 / 4e-10) = 90.97 dB
        let sr = 44100.0;
        let s = Sound { x: (0..44100).map(|i| (2.0 * std::f64::consts::PI * 300.0 * i as f64 / sr).sin()).collect(), sr };
        let it = to_intensity(&s, 100.0, 0.0);
        assert!((it.dt - 0.008).abs() < 1e-12);
        for v in &it.db {
            assert!((v - 90.97).abs() < 0.05, "{v}");
        }
        let half = Sound { x: s.x.iter().map(|v| v * 0.5).collect(), sr };
        assert!((to_intensity(&half, 100.0, 0.0).db[10] - (90.97 - 6.02)).abs() < 0.05);
    }
}
