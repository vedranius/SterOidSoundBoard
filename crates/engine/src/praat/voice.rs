//! Praat "Voice report": jitter, shimmer, harmonicity and voicing measures
//! from glottal pulses (Praat `VoiceAnalysis.cpp`, `AmplitudeTier.cpp`,
//! `PointProcess.cpp`).
use super::pitch::{Pitch, PitchParams};
use super::{quantile, Sound};
use serde::Serialize;

pub const MAX_PERIOD_FACTOR: f64 = 1.3;
pub const MAX_AMPLITUDE_FACTOR: f64 = 1.6;

#[derive(Debug, Clone, Default, Serialize)]
pub struct VoiceReport {
    pub tmin: f64,
    pub tmax: f64,
    // pitch
    pub median_pitch: Option<f64>,
    pub mean_pitch: Option<f64>,
    pub sd_pitch: Option<f64>,
    pub min_pitch: Option<f64>,
    pub max_pitch: Option<f64>,
    // pulses
    pub pulses: usize,
    pub periods: usize,
    pub mean_period: Option<f64>,
    pub sd_period: Option<f64>,
    // voicing
    pub unvoiced_fraction: f64,
    pub voice_breaks: usize,
    pub voice_break_degree: f64,
    // jitter (fractions, not %; absolute in s)
    pub jitter_local: Option<f64>,
    pub jitter_local_abs: Option<f64>,
    pub jitter_rap: Option<f64>,
    pub jitter_ppq5: Option<f64>,
    pub jitter_ddp: Option<f64>,
    // shimmer (fractions; local_db in dB)
    pub shimmer_local: Option<f64>,
    pub shimmer_local_db: Option<f64>,
    pub shimmer_apq3: Option<f64>,
    pub shimmer_apq5: Option<f64>,
    pub shimmer_apq11: Option<f64>,
    pub shimmer_dda: Option<f64>,
    // harmonicity of the voiced parts
    pub mean_autocorrelation: Option<f64>,
    pub mean_nhr: Option<f64>,
    pub mean_hnr: Option<f64>,
}

fn window_points(t: &[f64], tmin: f64, tmax: f64) -> &[f64] {
    let a = t.partition_point(|&v| v < tmin);
    let b = t.partition_point(|&v| v <= tmax);
    &t[a..b]
}

#[inline]
fn factor(a: f64, b: f64) -> f64 {
    if a > b { a / b } else { b / a }
}

/// Praat `PointProcess_isPeriod` on the interval t[i]..t[i+1].
fn is_period(t: &[f64], i: usize, pmin: f64, pmax: f64) -> bool {
    if i + 1 >= t.len() {
        return false;
    }
    let iv = t[i + 1] - t[i];
    if iv <= 0.0 || iv < pmin || iv > pmax {
        return false;
    }
    let prev = if i == 0 { None } else { Some(t[i] - t[i - 1]) };
    let next = if i + 2 >= t.len() { None } else { Some(t[i + 2] - t[i + 1]) };
    let pf = prev.filter(|&p| p > 0.0).map(|p| factor(iv, p));
    let nf = next.filter(|&p| p > 0.0).map(|p| factor(iv, p));
    !matches!((pf, nf), (Some(a), Some(b)) if a > MAX_PERIOD_FACTOR && b > MAX_PERIOD_FACTOR)
}

fn periods(t: &[f64], pmin: f64, pmax: f64) -> Vec<f64> {
    (0..t.len().saturating_sub(1)).filter(|&i| is_period(t, i, pmin, pmax)).map(|i| t[i + 1] - t[i]).collect()
}

fn ok(p: f64, pmin: f64, pmax: f64) -> bool {
    p >= pmin && p <= pmax
}

fn jitter_local_sum(t: &[f64], pmin: f64, pmax: f64) -> Option<(f64, usize)> {
    let mut n = t.len().checked_sub(1)?;
    if n < 2 {
        return None;
    }
    let mut sum = 0.0;
    for i in 1..t.len() - 1 {
        let (p1, p2) = (t[i] - t[i - 1], t[i + 1] - t[i]);
        if ok(p1, pmin, pmax) && ok(p2, pmin, pmax) && factor(p1, p2) <= MAX_PERIOD_FACTOR {
            sum += (p1 - p2).abs();
        } else {
            n -= 1;
        }
    }
    (n >= 2).then_some((sum / (n - 1) as f64, n))
}

fn jitter_rap(t: &[f64], pmin: f64, pmax: f64, mean_period: f64) -> Option<f64> {
    let mut n = t.len().checked_sub(1)?;
    if n < 3 {
        return None;
    }
    let mut sum = 0.0;
    for i in 2..t.len() - 1 {
        let (p1, p2, p3) = (t[i - 1] - t[i - 2], t[i] - t[i - 1], t[i + 1] - t[i]);
        if ok(p1, pmin, pmax) && ok(p2, pmin, pmax) && ok(p3, pmin, pmax) && factor(p1, p2) <= MAX_PERIOD_FACTOR && factor(p2, p3) <= MAX_PERIOD_FACTOR {
            sum += (p2 - (p1 + p2 + p3) / 3.0).abs();
        } else {
            n -= 1;
        }
    }
    (n >= 3).then(|| sum / (n - 2) as f64 / mean_period)
}

fn jitter_ppq5(t: &[f64], pmin: f64, pmax: f64, mean_period: f64) -> Option<f64> {
    let mut n = t.len().checked_sub(1)?;
    if n < 5 {
        return None;
    }
    let mut sum = 0.0;
    for i in 5..t.len() {
        let p: Vec<f64> = (0..5).map(|k| t[i - 4 + k] - t[i - 5 + k]).collect();
        let good = p.iter().all(|&v| ok(v, pmin, pmax)) && p.windows(2).all(|w| factor(w[0], w[1]) <= MAX_PERIOD_FACTOR);
        if good {
            sum += (p[2] - p.iter().sum::<f64>() / 5.0).abs();
        } else {
            n -= 1;
        }
    }
    (n >= 5).then(|| sum / (n - 4) as f64 / mean_period)
}

/// Praat `Sound_getHannWindowedRms` around a pulse.
fn hann_rms(s: &Sound, tmid: f64, wl: f64, wr: f64) -> Option<f64> {
    let (a, b) = s.window_samples(tmid - wl, tmid + wr);
    if b - a + 1 < 3 {
        return None;
    }
    let (mut sx, mut sw) = (0.0, 0.0);
    for i in a..=b {
        let t = s.t(i);
        let width = if t < tmid { wl } else { wr };
        let w = 0.5 + 0.5 * (std::f64::consts::PI * (t - tmid) / width).cos();
        let v = s.x[i as usize] * w;
        sx += v * v;
        sw += w * w;
    }
    Some((sx / sw).sqrt())
}

/// Praat `PointProcess_Sound_to_AmplitudeTier_period`: (time, amplitude).
fn amplitudes(s: &Sound, t: &[f64], pmin: f64, pmax: f64) -> Vec<(f64, f64)> {
    let mut out = vec![];
    for i in 1..t.len().saturating_sub(1) {
        let (p1, p2) = (t[i] - t[i - 1], t[i + 1] - t[i]);
        if ok(p1, pmin, pmax) && ok(p2, pmin, pmax) && factor(p1, p2) <= MAX_PERIOD_FACTOR {
            if let Some(a) = hann_rms(s, t[i], 0.2 * p1, 0.2 * p2).filter(|&a| a > 0.0) {
                out.push((t[i], a));
            }
        }
    }
    out
}

/// Denominator shared by the shimmer measures: mean of all points but the last.
fn amp_mean(a: &[(f64, f64)]) -> f64 {
    let n = a.len().saturating_sub(1).max(1);
    a[..a.len().saturating_sub(1)].iter().map(|p| p.1).sum::<f64>() / n as f64
}

fn shimmer_local(a: &[(f64, f64)], pmin: f64, pmax: f64, db: bool) -> Option<f64> {
    let (mut num, mut n) = (0.0, 0usize);
    for i in 1..a.len() {
        let p = a[i].0 - a[i - 1].0;
        if ok(p, pmin, pmax) {
            let (a1, a2) = (a[i - 1].1, a[i].1);
            if factor(a1, a2) <= MAX_AMPLITUDE_FACTOR {
                num += if db { (a1 / a2).log10().abs() } else { (a1 - a2).abs() };
                n += 1;
            }
        }
    }
    if n < 1 {
        return None;
    }
    if db {
        return Some(20.0 * num / n as f64);
    }
    let den = amp_mean(a);
    (den != 0.0).then(|| num / n as f64 / den)
}

/// APQ over `k` (3, 5 or 11) consecutive amplitudes.
fn shimmer_apq(a: &[(f64, f64)], pmin: f64, pmax: f64, k: usize) -> Option<f64> {
    let h = k / 2;
    let (mut num, mut n) = (0.0, 0usize);
    if a.len() < k {
        return None;
    }
    for i in h..a.len() - h {
        let w = &a[i - h..=i + h];
        let pers_ok = w.windows(2).all(|p| ok(p[1].0 - p[0].0, pmin, pmax));
        let amps_ok = w.windows(2).all(|p| factor(p[0].1, p[1].1) <= MAX_AMPLITUDE_FACTOR);
        if pers_ok && amps_ok {
            let avg = w.iter().map(|p| p.1).sum::<f64>() / k as f64;
            num += (a[i].1 - avg).abs();
            n += 1;
        }
    }
    if n < 1 {
        return None;
    }
    let den = amp_mean(a);
    (den != 0.0).then(|| num / n as f64 / den)
}

pub fn voice_report(s: &Sound, pitch: &Pitch, pulses: &[f64], tmin: f64, tmax: f64, p: &PitchParams) -> VoiceReport {
    let mut r = VoiceReport { tmin, tmax, ..Default::default() };
    // pitch statistics over voiced frames inside the window
    let mut f: Vec<f64> =
        (0..pitch.len()).filter(|&i| pitch.voiced(i) && pitch.time(i) >= tmin && pitch.time(i) <= tmax).map(|i| pitch.f0[i]).collect();
    if !f.is_empty() {
        let m = f.iter().sum::<f64>() / f.len() as f64;
        r.mean_pitch = Some(m);
        if f.len() > 1 {
            r.sd_pitch = Some((f.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (f.len() - 1) as f64).sqrt());
        }
        f.sort_by(f64::total_cmp);
        r.median_pitch = quantile(&f, 0.5);
        r.min_pitch = f.first().copied();
        r.max_pitch = f.last().copied();
    }
    // pulses
    let pmin = 0.8 / p.ceiling;
    let pmax = 1.25 / p.floor;
    let t = window_points(pulses, tmin, tmax);
    r.pulses = t.len();
    let per = periods(t, pmin, pmax);
    r.periods = per.len();
    if !per.is_empty() {
        let m = per.iter().sum::<f64>() / per.len() as f64;
        r.mean_period = Some(m);
        if per.len() >= 2 {
            r.sd_period = Some((per.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (per.len() - 1) as f64).sqrt());
        }
    }
    // voicing
    let frames: Vec<usize> = (0..pitch.len()).filter(|&i| pitch.time(i) >= tmin && pitch.time(i) <= tmax).collect();
    if !frames.is_empty() {
        r.unvoiced_fraction = frames.iter().filter(|&&i| !pitch.locally_voiced[i]).count() as f64 / frames.len() as f64;
    }
    if t.len() > 1 {
        let mut prev_voiced = true;
        let mut dur = 0.0;
        for i in 1..t.len() - 1 {
            let period = t[i] - t[i - 1];
            if period > pmax {
                dur += period;
                if prev_voiced {
                    r.voice_breaks += 1;
                    prev_voiced = false;
                }
            } else {
                prev_voiced = true;
            }
        }
        r.voice_break_degree = dur / (tmax - tmin);
    }
    // jitter
    if let (Some(mp), Some((abs, _))) = (r.mean_period, jitter_local_sum(t, pmin, pmax)) {
        r.jitter_local_abs = Some(abs);
        r.jitter_local = Some(abs / mp);
        r.jitter_rap = jitter_rap(t, pmin, pmax, mp);
        r.jitter_ppq5 = jitter_ppq5(t, pmin, pmax, mp);
        r.jitter_ddp = r.jitter_rap.map(|v| 3.0 * v);
    }
    // shimmer
    if t.len() >= 3 {
        let a = amplitudes(s, t, pmin, pmax);
        if !a.is_empty() {
            r.shimmer_local = shimmer_local(&a, pmin, pmax, false);
            r.shimmer_local_db = shimmer_local(&a, pmin, pmax, true);
            r.shimmer_apq3 = shimmer_apq(&a, pmin, pmax, 3);
            r.shimmer_apq5 = shimmer_apq(&a, pmin, pmax, 5);
            r.shimmer_apq11 = shimmer_apq(&a, pmin, pmax, 11);
            r.shimmer_dda = r.shimmer_apq3.map(|v| 3.0 * v);
        }
    }
    // harmonicity from the pitch strength of voiced frames
    let st: Vec<f64> = frames.iter().filter(|&&i| pitch.voiced(i)).map(|&i| pitch.strength[i]).collect();
    if !st.is_empty() {
        let n = st.len() as f64;
        r.mean_autocorrelation = Some(st.iter().sum::<f64>() / n);
        r.mean_nhr = Some(st.iter().map(|&v| if v <= 1e-15 { 1e15 } else if v > 1.0 - 1e-15 { 1e-15 } else { (1.0 - v) / v }).sum::<f64>() / n);
        r.mean_hnr = Some(st.iter().map(|&v| if v <= 1e-15 { -150.0 } else if v > 1.0 - 1e-15 { 150.0 } else { 10.0 * (v / (1.0 - v)).log10() }).sum::<f64>() / n);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::super::pitch::to_pitch;
    use super::super::pulses::to_pulses;
    use super::super::testsig::voice;
    use super::*;

    fn report(x: Vec<f64>) -> VoiceReport {
        let s = Sound { x, sr: 44100.0 };
        let pp = PitchParams::default();
        let p = to_pitch(&s, &pp);
        let pulses = to_pulses(&s, &p);
        voice_report(&s, &p, &pulses, 0.0, s.duration(), &pp)
    }

    #[test]
    fn clean_voice() {
        let r = report(voice(44100.0, 2.0, 150.0, 0.0, 0.0, 0.0));
        assert!((r.median_pitch.unwrap() - 150.0).abs() < 0.1);
        assert!(r.jitter_local.unwrap() < 0.001, "{:?}", r.jitter_local);
        assert!(r.shimmer_local.unwrap() < 0.005, "{:?}", r.shimmer_local);
        assert!(r.mean_hnr.unwrap() > 30.0, "{:?}", r.mean_hnr);
        assert_eq!(r.voice_breaks, 0);
    }

    #[test]
    fn measured_jitter_and_shimmer() {
        // uniform ±3 % periods: E|Ti − Ti+1| = 2 % of T; uniform ±9 % amplitude: ≈ 6 %
        let r = report(voice(44100.0, 3.0, 200.0, 0.03, 0.09, 0.0));
        let j = r.jitter_local.unwrap() * 100.0;
        let sh = r.shimmer_local.unwrap() * 100.0;
        assert!((j - 2.0).abs() < 0.25, "jitter {j}");
        assert!((sh - 6.0).abs() < 1.2, "shimmer {sh}");
        assert!(r.jitter_ppq5.unwrap() < r.jitter_local.unwrap());
        assert!((r.jitter_ddp.unwrap() - 3.0 * r.jitter_rap.unwrap()).abs() < 1e-12);
    }

    #[test]
    fn voice_breaks_counted() {
        let mut x = voice(44100.0, 0.6, 180.0, 0.0, 0.0, 0.0);
        x.extend(vec![0.0; 8820]);
        x.extend(voice(44100.0, 0.6, 180.0, 0.0, 0.0, 0.0));
        let r = report(x);
        assert_eq!(r.voice_breaks, 1, "{r:?}");
        assert!((r.voice_break_degree - 0.2 / 1.4).abs() < 0.05, "{}", r.voice_break_degree);
    }
}
