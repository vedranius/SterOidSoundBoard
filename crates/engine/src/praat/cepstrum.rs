//! Smoothed cepstral peak prominence (CPPS), Praat `PowerCepstrogram: Get
//! CPPS…` with the settings of the AVQI protocol (Maryn & Weenink 2015):
//! `To PowerCepstrogram: 60, 0.002, 5000, 50` and
//! `Get CPPS: no, 0.01, 0.001, 60, 330, 0.05, parabolic, 0.001, 0, straight, robust`.
use super::{sampled_mean, Fft64, Sound};

pub struct CppsParams {
    pub pitch_floor: f64,
    pub time_step: f64,
    pub max_frequency: f64,
    pub pre_emphasis: f64,
    pub time_smoothing: f64,
    pub quefrency_smoothing: f64,
    pub peak_floor: f64,
    pub peak_ceiling: f64,
    /// Trend-line quefrency range; qmin ≥ qmax means "whole domain" (as the
    /// AVQI script's `0.001, 0`: Praat then fits from the 2nd sample to the end).
    pub trend_qmin: f64,
    pub trend_qmax: f64,
}

impl Default for CppsParams {
    fn default() -> Self {
        CppsParams {
            pitch_floor: 60.0,
            time_step: 0.002,
            max_frequency: 5000.0,
            pre_emphasis: 50.0,
            time_smoothing: 0.01,
            quefrency_smoothing: 0.001,
            peak_floor: 60.0,
            peak_ceiling: 330.0,
            trend_qmin: 0.001,
            trend_qmax: 0.0,
        }
    }
}

/// Power cepstrogram: frames × quefrency bins (dq = 1 / resampled rate).
/// Returns (rows, first frame time, dq, duration).
pub fn cepstrogram(s: &Sound, p: &CppsParams) -> (Vec<Vec<f64>>, f64, f64, f64) {
    let mut r = s.resample(2.0 * p.max_frequency);
    r.pre_emphasize(p.pre_emphasis);
    let effective = 3.0 / p.pitch_floor;
    let physical = 2.0 * effective; // Gaussian window
    let (nframes, t1) = r.short_term(physical.min(r.duration()), p.time_step);
    let size = (physical * r.sr).round() as usize;
    let nfft = size.next_power_of_two().max(2);
    let fft = Fft64::new(nfft);
    let window: Vec<f64> = (1..=size)
        .map(|i| {
            let imid = 0.5 * (size as f64 + 1.0);
            let phase = (i as f64 - imid) / size as f64;
            let edge = (-12.0f64).exp();
            ((-48.0 * phase * phase).exp() - edge) / (1.0 - edge)
        })
        .collect();
    let nf = nfft / 2 + 1;
    let dx = 1.0 / r.sr;
    let mut rows = Vec::with_capacity(nframes);
    let mut frame = vec![0.0; size];
    for i in 0..nframes {
        let t = t1 + i as f64 * p.time_step;
        let start = r.nearest_index(t) - size as isize / 2;
        for (k, v) in frame.iter_mut().enumerate() {
            *v = r.at(start + k as isize);
        }
        let mean = frame.iter().sum::<f64>() / size as f64;
        for (k, v) in frame.iter_mut().enumerate() {
            *v = (*v - mean) * window[k];
        }
        let pw = fft.power(&frame);
        // log one-sided power spectrum, then inverse transform
        let scale = 2.0 * dx * dx;
        let mut re = vec![0.0; nfft];
        let mut im = vec![0.0; nfft];
        for k in 0..nf {
            let l = (pw[k] * scale + 1e-300).ln();
            re[k] = l;
            if k > 0 && k < nfft / 2 {
                re[nfft - k] = l;
            }
        }
        fft.forward(&mut re, &mut im); // real & even: forward == backward
        let df = 1.0 / (dx * nfft as f64);
        rows.push((0..nf).map(|k| (re[k] * df) * (re[k] * df)).collect::<Vec<f64>>());
    }
    (rows, t1, 1.0 / r.sr, r.duration())
}

fn median(v: &mut [f64]) -> f64 {
    let n = v.len();
    let (_, m, _) = v.select_nth_unstable_by(n / 2, f64::total_cmp);
    let hi = *m;
    if n % 2 == 1 {
        hi
    } else {
        let lo = v[..n / 2].iter().copied().fold(f64::MIN, f64::max);
        0.5 * (lo + hi)
    }
}

/// Siegel repeated-median line fit (Praat's "robust" trend line).
/// Like Praat's `SlopeSelector::getSlope_Siegel`, the median of medians is
/// taken over the first n − 1 points (the last point's median is not used).
fn siegel(x: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x.len();
    let mut outer = Vec::with_capacity(n);
    let mut inner = Vec::with_capacity(n);
    for i in 0..n.saturating_sub(1) {
        inner.clear();
        for j in 0..n {
            if j != i && x[j] != x[i] {
                inner.push((y[j] - y[i]) / (x[j] - x[i]));
            }
        }
        if !inner.is_empty() {
            outer.push(median(&mut inner));
        }
    }
    let slope = median(&mut outer);
    let mut ic: Vec<f64> = (0..n).map(|i| y[i] - slope * x[i]).collect();
    (slope, median(&mut ic))
}

/// Praat's peak search "from both sides" with parabolic refinement.
/// `start_high` scans from the high end. Returns (value, real index).
fn peak(y: &[f64], imin: usize, imax: usize, start_high: bool) -> (f64, f64) {
    let gt = |x: f64, z: f64| (if x == 0.0 || z == 0.0 { x - z } else { (x - z) / x.abs() }) > 1e-12;
    let ge = |x: f64, z: f64| (if x == 0.0 || z == 0.0 { x - z } else { (x - z) / x.abs() }) > -1e-12;
    let (mut max, mut x) = (y[imin], imin as f64);
    if y[imax] > max {
        max = y[imax];
        x = imax as f64;
    }
    let (lo, hi) = (imin.max(1), imax.min(y.len() - 2));
    let mut consider = |i: usize| {
        let dy = 0.5 * (y[i + 1] - y[i - 1]);
        let d2y = 2.0 * y[i] - y[i - 1] - y[i + 1];
        let (v, xi) = if d2y != 0.0 { (y[i] + 0.5 * dy * dy / d2y, i as f64 + dy / d2y) } else { (y[i], i as f64) };
        if v > max {
            max = v;
            x = xi;
        }
    };
    if !start_high {
        for i in lo..=hi {
            if gt(y[i], y[i - 1]) && ge(y[i], y[i + 1]) {
                consider(i);
            }
        }
    } else {
        for i in (lo..=hi).rev() {
            if gt(y[i], y[i + 1]) && ge(y[i], y[i - 1]) {
                consider(i);
            }
        }
    }
    (max, x.clamp(imin as f64, imax as f64))
}

/// Praat `PowerCepstrogram_smooth` (rectangular): time then quefrency averaging.
pub fn smoothed_cepstrogram(s: &Sound, p: &CppsParams) -> (Vec<Vec<f64>>, f64) {
    let (rows, t1, dq, dur) = cepstrogram(s, p);
    if rows.is_empty() {
        return (rows, dq);
    }
    let nq = rows[0].len();
    let nfr = rows.len();
    let qmax = (nq - 1) as f64 * dq;
    let mut sm = rows.clone();
    if (p.time_smoothing / p.time_step).floor() > 1.0 {
        let half = 0.5 * p.time_smoothing;
        let mut col = vec![0.0; nfr];
        for q in 0..nq {
            for (i, v) in col.iter_mut().enumerate() {
                *v = rows[i][q];
            }
            for i in 0..nfr {
                let t = t1 + i as f64 * p.time_step;
                sm[i][q] = sampled_mean(&col, t1, p.time_step, 0.0, dur, t - half, t + half).unwrap_or(col[i]);
            }
        }
    }
    if (p.quefrency_smoothing / dq).floor() > 1.0 {
        let half = 0.5 * p.quefrency_smoothing;
        for row in sm.iter_mut() {
            let src = row.clone();
            for (k, v) in row.iter_mut().enumerate() {
                let q = k as f64 * dq;
                *v = sampled_mean(&src, 0.0, dq, 0.0, qmax, q - half, q + half).unwrap_or(src[k]);
            }
        }
    }
    (sm, dq)
}

/// Per frame: (slope dB/s, intercept dB, peak dB, peak quefrency s, CPP dB).
pub fn cpp_frames(s: &Sound, p: &CppsParams) -> Vec<(f64, f64, f64, f64, f64)> {
    let (sm, dq) = smoothed_cepstrogram(s, p);
    if sm.is_empty() {
        return vec![];
    }
    let nq = sm[0].len();
    // trend-line points (never q = 0)
    let (fit_a, fit_b) = if p.trend_qmin >= p.trend_qmax {
        (1, nq - 1)
    } else {
        (((p.trend_qmin / dq).ceil() as usize).max(1), ((p.trend_qmax / dq).floor() as usize).min(nq - 1))
    };
    let xs: Vec<f64> = (fit_a..=fit_b).map(|k| k as f64 * dq).collect();
    // peak search window (0-based sample indices)
    let pa = ((1.0 / p.peak_ceiling) / dq).ceil() as usize;
    let pb = (((1.0 / p.peak_floor) / dq).floor() as usize).min(nq - 1);
    sm.iter()
        .map(|row| {
            let db: Vec<f64> = row.iter().map(|v| 10.0 * (v + 1e-30).log10()).collect();
            let (slope, intercept) = siegel(&xs, &db[fit_a..=fit_b]);
            let (mut peak_db, xi) = peak(&db, pa, pb, false);
            let (_, xr) = peak(&db, pa, pb, true);
            let mut qpk = xi * dq;
            let (i1, i2) = (xi.round() as isize, xr.round() as isize);
            if i1 != i2 && (i2 - i1) <= 5 {
                qpk = 0.5 * (i1 + i2) as f64 * dq;
                peak_db = db[i1 as usize]; // flat peak
            }
            (slope, intercept, peak_db, qpk, peak_db - (slope * qpk + intercept))
        })
        .collect()
}

/// CPPS in dB (mean CPP over frames of the smoothed cepstrogram).
pub fn cpps(s: &Sound, p: &CppsParams) -> Option<f64> {
    let f = cpp_frames(s, p);
    (!f.is_empty()).then(|| f.iter().map(|v| v.4).sum::<f64>() / f.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::super::testsig::voice;
    use super::*;

    #[test]
    fn periodic_voice_has_higher_cpps_than_noisy() {
        let sr = 44100.0;
        let clean = cpps(&Sound { x: voice(sr, 0.6, 140.0, 0.0, 0.0, 0.0), sr }, &CppsParams::default()).unwrap();
        let noisy = cpps(&Sound { x: voice(sr, 0.6, 140.0, 0.0, 0.0, 0.25), sr }, &CppsParams::default()).unwrap();
        let mut seed = 3u32;
        let white = cpps(&Sound { x: (0..26460).map(|_| super::super::testsig::rng(&mut seed)).collect(), sr }, &CppsParams::default()).unwrap();
        assert!(clean > noisy + 3.0, "clean {clean} noisy {noisy}");
        assert!(noisy > white, "noisy {noisy} white {white}");
        assert!(white < 5.0, "white noise CPPS {white}");
    }

    #[test]
    fn siegel_is_robust() {
        let xs: Vec<f64> = (0..50).map(|i| i as f64).collect();
        let mut ys: Vec<f64> = xs.iter().map(|x| 2.0 * x + 1.0).collect();
        ys[10] = 500.0;
        ys[20] = -300.0;
        let (s, i) = siegel(&xs, &ys);
        assert!((s - 2.0).abs() < 1e-9 && (i - 1.0).abs() < 1e-9);
    }
}
