//! Glottal pulses: Praat `Sound & Pitch: To PointProcess (cc)`.
//! Starting at an extremum in the middle of each voiced interval, walks
//! left and right one period at a time, placing each pulse where the
//! waveform best correlates with the previous cycle.
use super::pitch::Pitch;
use super::Sound;

/// Absolute extremum in [tmin, tmax] with parabolic refinement.
fn find_extremum(s: &Sound, tmin: f64, tmax: f64) -> f64 {
    let a = s.low_index(tmin).max(0);
    let b = s.high_index(tmax).min(s.x.len() as isize - 1);
    if b <= a {
        return 0.5 * (tmin + tmax);
    }
    let (mut imax, mut imin) = (a, a);
    for i in a..=b {
        if s.at(i) > s.at(imax) {
            imax = i;
        }
        if s.at(i) < s.at(imin) {
            imin = i;
        }
    }
    let i = if s.at(imax).abs() >= s.at(imin).abs() { imax } else { imin };
    let (y0, y1, y2) = (s.at(i - 1), s.at(i), s.at(i + 1));
    let den = y0 - 2.0 * y1 + y2;
    let d = if i > a && i < b && den.abs() > 1e-300 { (0.5 * (y0 - y2) / den).clamp(-0.5, 0.5) } else { 0.0 };
    s.t(i) + d / s.sr
}

/// Praat `Sound_findMaximumCorrelation`: where in [tmin2, tmax2] does a
/// window of `win` s look most like the window centred at `t1`?
/// Returns (correlation, time, local peak); correlation -1 = nothing found.
fn find_max_correlation(s: &Sound, t1: f64, win: f64, tmin2: f64, tmax2: f64) -> (f64, f64, f64) {
    let hw = 0.5 * win;
    let ileft1 = s.nearest_index(t1 - hw);
    let iright1 = s.nearest_index(t1 + hw);
    let ileft2min = s.low_index(tmin2 - hw);
    let ileft2max = s.high_index(tmax2 - hw);
    let n = s.x.len() as isize;
    let (mut r2, mut r3) = (0.0, 0.0);
    let mut r1: f64;
    let (mut best, mut r1b, mut r3b, mut ir, mut peak) = (-1.0f64, f64::NAN, f64::NAN, f64::NAN, 0.0);
    for ileft2 in ileft2min..=ileft2max {
        let (mut n1, mut n2, mut prod, mut lp) = (0.0, 0.0, 0.0, 0f64);
        let mut i2 = ileft2;
        for i1 in ileft1..=iright1 {
            if i1 >= 0 && i1 < n && i2 >= 0 && i2 < n {
                let (a1, a2) = (s.x[i1 as usize], s.x[i2 as usize]);
                n1 += a1 * a1;
                n2 += a2 * a2;
                prod += a1 * a2;
                lp = lp.max(a2.abs());
            }
            i2 += 1;
        }
        r1 = r2;
        r2 = r3;
        r3 = if n1 == 0.0 || n2 == 0.0 { 0.0 } else { prod / (n1 * n2).sqrt() };
        if r2 > best && r2 >= r1 && r2 >= r3 {
            r1b = r1;
            best = r2;
            r3b = r3;
            ir = (ileft2 - 1) as f64;
            peak = lp;
        }
    }
    let mut tout = f64::NAN;
    if best > -1.0 {
        let d2r = (best - r1b) + (best - r3b);
        let mut height = f64::NAN;
        if d2r != 0.0 {
            let dr = 0.5 * (r3b - r1b);
            height = best + 0.5 * dr * dr / d2r;
            ir += dr / d2r;
        }
        let peak_time = t1 + (ir - ileft1 as f64) / s.sr;
        if peak_time >= tmin2 && peak_time <= tmax2 {
            tout = peak_time;
            if height.is_finite() {
                best = height;
            }
        } else {
            let mid = ((tmin2 - t1) * (tmax2 - t1)).abs().sqrt();
            tout = if tmin2 < t1 { t1 - mid } else { t1 + mid };
            if r3 > best {
                best = r3;
            }
        }
    }
    (best, tout, peak)
}

/// Pulse times (s), sorted.
pub fn to_pulses(s: &Sound, pitch: &Pitch) -> Vec<f64> {
    let mut pts: Vec<f64> = vec![];
    let global_peak = s.x.iter().fold(0f64, |m, v| m.max(v.abs()));
    let mut t = 0.0;
    let mut added_right = -1e308;
    while let Some((tleft, tright)) = pitch.voiced_interval_after(t) {
        let tmid = 0.5 * (tleft + tright);
        let Some(f0mid) = pitch.value_at(tmid) else {
            t = tright;
            continue;
        };
        let start = find_extremum(s, tmid - 0.5 / f0mid, tmid + 0.5 / f0mid);
        pts.push(start);
        // leftwards
        let mut tmax = start;
        let mut guard = 0;
        while let Some(f0) = pitch.value_at(tmax) {
            guard += 1;
            if guard > 100_000 {
                break;
            }
            let (c, tn, peak) = find_max_correlation(s, tmax, 1.0 / f0, tmax - 1.25 / f0, tmax - 0.8 / f0);
            tmax = if c == -1.0 { tmax - 1.0 / f0 } else { tn };
            if tmax < tleft {
                if c > 0.7 && peak > 0.023333 * global_peak && tmax - added_right > 0.8 / f0 {
                    pts.push(tmax);
                }
                break;
            }
            if c > 0.3 && (peak == 0.0 || peak > 0.01 * global_peak) && tmax - added_right > 0.8 / f0 {
                pts.push(tmax);
            }
        }
        // rightwards
        tmax = start;
        guard = 0;
        while let Some(f0) = pitch.value_at(tmax) {
            guard += 1;
            if guard > 100_000 {
                break;
            }
            let (c, tn, peak) = find_max_correlation(s, tmax, 1.0 / f0, tmax + 0.8 / f0, tmax + 1.25 / f0);
            tmax = if c == -1.0 { tmax + 1.0 / f0 } else { tn };
            if tmax > tright {
                if c > 0.7 && peak > 0.023333 * global_peak {
                    pts.push(tmax);
                    added_right = tmax;
                }
                break;
            }
            if c > 0.3 && (peak == 0.0 || peak > 0.01 * global_peak) {
                pts.push(tmax);
                added_right = tmax;
            }
        }
        t = tright;
    }
    pts.retain(|v| v.is_finite());
    pts.sort_by(f64::total_cmp);
    pts.dedup();
    pts
}

#[cfg(test)]
mod tests {
    use super::super::pitch::{to_pitch, PitchParams};
    use super::super::testsig::voice;
    use super::*;

    #[test]
    fn one_pulse_per_period() {
        let sr = 44100.0;
        let s = Sound { x: voice(sr, 1.0, 125.0, 0.0, 0.0, 0.0), sr };
        let p = to_pitch(&s, &PitchParams::default());
        let pulses = to_pulses(&s, &p);
        assert!((115..=127).contains(&pulses.len()), "{}", pulses.len());
        for w in pulses.windows(2) {
            let per = w[1] - w[0];
            assert!((per - 0.008).abs() < 2e-5, "period {per}");
        }
    }
}
