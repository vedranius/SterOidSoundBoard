//! Pitch by autocorrelation (Boersma 1993; Praat `Sound: To Pitch (ac)…`)
//! with Praat's Viterbi path finder.
use super::{interpolate_sinc, maximize, Fft64, Sound};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PitchParams {
    pub floor: f64,
    pub ceiling: f64,
    /// 0 = automatic (0.75 / floor, as Praat).
    pub time_step: f64,
    pub max_candidates: usize,
    pub silence_threshold: f64,
    pub voicing_threshold: f64,
    pub octave_cost: f64,
    pub octave_jump_cost: f64,
    pub voiced_unvoiced_cost: f64,
}

impl Default for PitchParams {
    /// Praat's defaults for voice analysis.
    fn default() -> Self {
        PitchParams {
            floor: 75.0,
            ceiling: 600.0,
            time_step: 0.0,
            max_candidates: 15,
            silence_threshold: 0.03,
            voicing_threshold: 0.45,
            octave_cost: 0.01,
            octave_jump_cost: 0.35,
            voiced_unvoiced_cost: 0.14,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    f: f64,
    r: f64,
}

pub struct Pitch {
    pub t1: f64,
    pub dt: f64,
    pub xmax: f64,
    pub ceiling: f64,
    /// F0 per frame, 0 when unvoiced.
    pub f0: Vec<f64>,
    /// Correlation strength of the chosen candidate (0 when unvoiced).
    pub strength: Vec<f64>,
    /// Local peak / global peak per frame.
    pub intensity: Vec<f64>,
    /// Praat's "locally voiced" frame (any candidate voiced & strong enough).
    pub locally_voiced: Vec<bool>,
}

impl Pitch {
    pub fn len(&self) -> usize {
        self.f0.len()
    }
    pub fn time(&self, i: usize) -> f64 {
        self.t1 + i as f64 * self.dt
    }
    pub fn voiced(&self, i: usize) -> bool {
        self.f0[i] > 0.0
    }

    /// Praat `Pitch_getValueAtTime (…, linear)`: nearest frame must be voiced;
    /// interpolates towards the far neighbour when that one is voiced too.
    pub fn value_at(&self, t: f64) -> Option<f64> {
        if t < 0.0 || t > self.xmax || self.f0.is_empty() {
            return None;
        }
        let idx = (t - self.t1) / self.dt;
        let left = idx.floor();
        let mut phase = idx - left;
        let (near, far) = if phase < 0.5 {
            (left as isize, left as isize + 1)
        } else {
            phase = 1.0 - phase;
            (left as isize + 1, left as isize)
        };
        let n = self.f0.len() as isize;
        if near < 0 || near >= n {
            return None;
        }
        let nv = self.f0[near as usize];
        if nv <= 0.0 {
            return None;
        }
        if far < 0 || far >= n || self.f0[far as usize] <= 0.0 {
            return Some(nv);
        }
        Some(nv + phase * (self.f0[far as usize] - nv))
    }

    /// Praat `Pitch_getVoicedIntervalAfter`.
    pub fn voiced_interval_after(&self, after: f64) -> Option<(f64, f64)> {
        let n = self.f0.len();
        let mut i = (((after - self.t1) / self.dt).ceil()).max(0.0) as usize;
        while i < n && !self.voiced(i) {
            i += 1;
        }
        if i >= n {
            return None;
        }
        let mut j = i;
        while j + 1 < n && self.voiced(j + 1) {
            j += 1;
        }
        let tl = (self.time(i) - 0.5 * self.dt).max(0.0);
        let tr = (self.time(j) + 0.5 * self.dt).min(self.xmax);
        if tl >= self.xmax - 0.5 * self.dt {
            return None;
        }
        Some((tl, tr))
    }
}

pub fn to_pitch(s: &Sound, p: &PitchParams) -> Pitch {
    let sr = s.sr;
    let ceiling = p.ceiling.min(0.5 * sr);
    let periods_per_window = 3.0;
    let dt = if p.time_step > 0.0 { p.time_step } else { periods_per_window / p.floor / 4.0 };
    let nsamp_period = (sr / p.floor).floor() as isize;
    let halfnsamp_period = nsamp_period / 2 + 1;
    let dt_window = periods_per_window / p.floor;
    let halfnsamp_window = ((dt_window * sr).floor() as isize) / 2 - 1;
    let nsamp_window = (halfnsamp_window * 2).max(4) as usize;
    let maximum_lag = (((nsamp_window as f64) / periods_per_window).floor() as usize + 2).min(nsamp_window);
    let interpolation_depth = 0.5;
    let mut nfft = 1;
    while (nfft as f64) < nsamp_window as f64 * (1.0 + interpolation_depth) {
        nfft *= 2;
    }
    let brent_ixmax = (nsamp_window as f64 * interpolation_depth).floor() as usize;
    let (nframes, t1) = s.short_term(dt_window, dt);
    let mut out = Pitch {
        t1,
        dt,
        xmax: s.duration(),
        ceiling,
        f0: vec![0.0; nframes],
        strength: vec![0.0; nframes],
        intensity: vec![0.0; nframes],
        locally_voiced: vec![false; nframes],
    };
    if nframes == 0 {
        return out;
    }
    let mean = s.x.iter().sum::<f64>() / s.x.len() as f64;
    let global_peak = s.x.iter().fold(0f64, |m, v| m.max((v - mean).abs()));
    if global_peak == 0.0 {
        return out;
    }
    let fft = Fft64::new(nfft);
    let window: Vec<f64> =
        (1..=nsamp_window).map(|i| 0.5 - 0.5 * (i as f64 * 2.0 * std::f64::consts::PI / (nsamp_window as f64 + 1.0)).cos()).collect();
    let wr = fft.autocorr(&window);
    let window_r: Vec<f64> = wr.iter().map(|v| v / wr[0]).collect();

    let mut frames: Vec<(Vec<Candidate>, f64)> = Vec::with_capacity(nframes);
    let mut buf = vec![0.0; nsamp_window];
    for iframe in 0..nframes {
        let t = out.time(iframe);
        let left = s.low_index(t);
        let right = left + 1;
        // local mean over one longest period each side
        let (a, b) = (right - nsamp_period, left + nsamp_period);
        let mut lm = 0.0;
        for i in a..=b {
            lm += s.at(i);
        }
        lm /= (2 * nsamp_period) as f64;
        let start = right - halfnsamp_window;
        for (j, v) in buf.iter_mut().enumerate() {
            *v = (s.at(start + j as isize) - lm) * window[j];
        }
        // local peak: half a longest period each side of the centre
        let js = (halfnsamp_window + 1 - halfnsamp_period - 1).max(0) as usize;
        let je = ((halfnsamp_window + halfnsamp_period - 1) as usize).min(nsamp_window - 1);
        let local_peak = buf[js..=je].iter().fold(0f64, |m, v| m.max(v.abs()));
        let intensity = if local_peak > global_peak { 1.0 } else { local_peak / global_peak };
        out.intensity[iframe] = intensity;
        let mut cands = vec![Candidate { f: 0.0, r: 0.0 }];
        if local_peak == 0.0 {
            frames.push((cands, intensity));
            continue;
        }
        let ac = fft.autocorr(&buf);
        if ac[0] <= 0.0 {
            frames.push((cands, intensity));
            continue;
        }
        let r: Vec<f64> = (0..=brent_ixmax).map(|i| if i == 0 { 1.0 } else { ac[i] / (ac[0] * window_r[i]) }).collect();
        let rv = |k: isize| -> f64 {
            let k = k.unsigned_abs();
            if k <= brent_ixmax { r[k] } else { 0.0 }
        };
        let mut imax = vec![0usize];
        let top = maximum_lag.min(brent_ixmax);
        for i in 2..top {
            if !(r[i] > 0.5 * p.voicing_threshold && r[i] > r[i - 1] && r[i] >= r[i + 1]) {
                continue;
            }
            let dr = 0.5 * (r[i + 1] - r[i - 1]);
            let d2r = (r[i] - r[i - 1]) + (r[i] - r[i + 1]);
            if d2r <= 0.0 {
                continue;
            }
            let fmax = sr / (i as f64 + dr / d2r);
            let mut smax = interpolate_sinc(rv, sr / fmax, 30);
            if smax > 1.0 {
                smax = 1.0 / smax;
            }
            let mut place = None;
            if cands.len() < p.max_candidates {
                cands.push(Candidate { f: 0.0, r: 0.0 });
                imax.push(0);
                place = Some(cands.len() - 1);
            } else {
                let mut weakest = 2.0;
                for (iw, c) in cands.iter().enumerate().skip(1) {
                    let ls = c.r - p.octave_cost * (p.floor / c.f).log2();
                    if ls < weakest {
                        weakest = ls;
                        place = Some(iw);
                    }
                }
                if smax - p.octave_cost * (p.floor / fmax).log2() <= weakest {
                    place = None;
                }
            }
            if let Some(pl) = place {
                cands[pl] = Candidate { f: fmax, r: smax };
                imax[pl] = i;
            }
        }
        // refine: maximise the sinc-interpolated autocorrelation
        for (ic, c) in cands.iter_mut().enumerate().skip(1) {
            let x0 = imax[ic] as f64;
            let (xm, mut ym) = maximize(|x| interpolate_sinc(rv, x, 70), x0 - 1.0, x0 + 1.0, 50);
            if ym > 1.0 {
                ym = 1.0 / ym;
            }
            c.f = sr / xm;
            c.r = ym;
        }
        out.locally_voiced[iframe] = intensity >= p.silence_threshold
            && cands.iter().any(|c| c.f > 0.0 && c.f < ceiling && c.r >= p.voicing_threshold);
        frames.push((cands, intensity));
    }

    // ---- Viterbi path finder (Praat Pitch_pathFinder)
    let tsc = 0.01 / dt;
    let (ojc, vuc) = (p.octave_jump_cost * tsc, p.voiced_unvoiced_cost * tsc);
    let voiced = |f: f64| f > 0.0 && f < ceiling;
    let mut delta: Vec<Vec<f64>> = frames
        .iter()
        .map(|(cands, inten)| {
            let unv = p.voicing_threshold
                + (if p.silence_threshold <= 0.0 { 0.0 } else { 2.0 - inten / (p.silence_threshold / (1.0 + p.voicing_threshold)) }).max(0.0);
            cands.iter().map(|c| if voiced(c.f) { c.r - p.octave_cost * (ceiling / c.f).log2() } else { unv }).collect()
        })
        .collect();
    let mut psi: Vec<Vec<usize>> = frames.iter().map(|(c, _)| vec![0; c.len()]).collect();
    for i in 1..nframes {
        let (prev, cur) = (&frames[i - 1].0, &frames[i].0);
        for c2 in 0..cur.len() {
            let f2 = cur[c2].f;
            let (mut best, mut place) = (f64::MIN, 0);
            for c1 in 0..prev.len() {
                let f1 = prev[c1].f;
                let cost = match (voiced(f1), voiced(f2)) {
                    (false, false) => 0.0,
                    (true, false) | (false, true) => vuc,
                    (true, true) => ojc * (f1 / f2).log2().abs(),
                };
                let v = delta[i - 1][c1] - cost + delta[i][c2];
                if v > best {
                    best = v;
                    place = c1;
                }
            }
            delta[i][c2] = best;
            psi[i][c2] = place;
        }
    }
    let last = nframes - 1;
    let mut place = (0..frames[last].0.len()).fold(0, |b, c| if delta[last][c] > delta[last][b] { c } else { b });
    for i in (0..nframes).rev() {
        let c = frames[i].0[place];
        if voiced(c.f) {
            out.f0[i] = c.f;
            out.strength[i] = c.r;
        }
        if i > 0 {
            place = psi[i][place];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::testsig::voice;
    use super::*;

    #[test]
    fn tracks_steady_voices() {
        for f0 in [90.0, 150.0, 240.0, 400.0] {
            let s = Sound { x: voice(44100.0, 1.0, f0, 0.0, 0.0, 0.0), sr: 44100.0 };
            let p = to_pitch(&s, &PitchParams::default());
            let v: Vec<f64> = p.f0.iter().copied().filter(|&f| f > 0.0).collect();
            assert!(v.len() as f64 > 0.9 * p.len() as f64, "{f0}: voiced {} / {}", v.len(), p.len());
            let m = v.iter().sum::<f64>() / v.len() as f64;
            assert!((m - f0).abs() < 0.05, "{f0}: {m}");
            assert!(p.strength.iter().filter(|&&r| r > 0.0).all(|&r| r > 0.95));
        }
    }

    #[test]
    fn silence_and_noise_are_unvoiced() {
        let silent = Sound { x: vec![0.0; 22050], sr: 44100.0 };
        assert!(to_pitch(&silent, &PitchParams::default()).f0.iter().all(|&f| f == 0.0));
        let mut seed = 7u32;
        let noise = Sound { x: (0..44100).map(|_| super::super::testsig::rng(&mut seed) * 0.3).collect(), sr: 44100.0 };
        let p = to_pitch(&noise, &PitchParams::default());
        let voiced = p.f0.iter().filter(|&&f| f > 0.0).count();
        assert!(voiced * 10 < p.len(), "noise voiced {voiced}/{}", p.len());
    }

    #[test]
    fn value_at_and_intervals() {
        let mut x = voice(44100.0, 0.5, 200.0, 0.0, 0.0, 0.0);
        x.extend(vec![0.0; 22050]);
        let s = Sound { x, sr: 44100.0 };
        let p = to_pitch(&s, &PitchParams::default());
        let (tl, tr) = p.voiced_interval_after(0.0).unwrap();
        assert!(tl < 0.05 && (tr - 0.5).abs() < 0.05, "{tl} {tr}");
        assert!((p.value_at(0.25).unwrap() - 200.0).abs() < 0.1);
        assert!(p.value_at(0.8).is_none());
    }
}
