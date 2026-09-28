//! Acoustic voice analysis (DigiLingua): F0 track, jitter, shimmer, HNR.
//! Follows Praat's approach (Boersma 1993 autocorrelation pitch, peak-picked
//! glottal periods) so values are comparable to the usual clinical norms.
//! Runs on the control thread on recorded audio.
use crate::fft::{hann, Fft};
use serde::Serialize;

const FMIN: f32 = 75.0;
const FMAX: f32 = 600.0;
const VOICING: f32 = 0.45;
const SILENCE: f32 = 0.03;
const OCTAVE_COST: f32 = 0.01;
const MAX_PERIOD_FACTOR: f32 = 1.3;
const MAX_AMP_FACTOR: f32 = 1.6;

/// Clinical reference thresholds (MDVP / Praat literature, sustained vowel).
pub const JITTER_MAX: f32 = 1.04; // %
pub const SHIMMER_MAX: f32 = 3.81; // %
pub const SHIMMER_DB_MAX: f32 = 0.35; // dB
pub const HNR_MIN: f32 = 20.0; // dB

#[derive(Debug, Clone, Serialize, Default)]
pub struct VoiceReport {
    pub duration: f32,
    pub sample_rate: u32,
    pub voiced_fraction: f32,
    pub f0_mean: Option<f32>,
    pub f0_median: Option<f32>,
    pub f0_sd: Option<f32>,
    pub f0_min: Option<f32>,
    pub f0_max: Option<f32>,
    /// Jitter (local), %.
    pub jitter_local: Option<f32>,
    /// Jitter (local, absolute), µs.
    pub jitter_abs_us: Option<f32>,
    /// Jitter (RAP), %.
    pub jitter_rap: Option<f32>,
    /// Shimmer (local), %.
    pub shimmer_local: Option<f32>,
    /// Shimmer (local, dB).
    pub shimmer_db: Option<f32>,
    /// Harmonics-to-noise ratio, dB (mean over voiced frames).
    pub hnr_db: Option<f32>,
    /// RMS level of voiced frames, dBFS.
    pub intensity_dbfs: Option<f32>,
    pub periods: usize,
    /// [time s, f0 Hz or 0 when unvoiced] every 10 ms.
    pub pitch: Vec<[f32; 2]>,
    /// True if every measured parameter is inside the reference range.
    pub normal: bool,
    pub findings: Vec<String>,
    /// Human-readable clinical report (Croatian).
    pub report: String,
}

struct Frame {
    t: f32,
    f0: f32,
    r: f32,
    rms: f32,
}

fn pitch_track(x: &[f32], sr: f32) -> (Vec<Frame>, f32) {
    let l = (3.0 * sr / FMIN) as usize;
    let hop = ((0.01 * sr) as usize).max(1);
    let fft = Fft::new((2 * l).next_power_of_two());
    let win = hann(l);
    let mut rw = vec![];
    fft.autocorr(&win, &mut rw);
    let rw0 = rw[0];
    let global = x.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-9);
    let tmin = (sr / FMAX).floor().max(2.0) as usize;
    let tmax = (l / 3).min((sr / FMIN).ceil() as usize);
    let mut frames = vec![];
    let mut buf = vec![0.0; l];
    let mut ac = vec![];
    let mut s = 0;
    while s + l <= x.len() {
        let seg = &x[s..s + l];
        let mean = seg.iter().sum::<f32>() / l as f32;
        let local = seg.iter().fold(0f32, |m, v| m.max((v - mean).abs()));
        let rms = (seg.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / l as f32).sqrt();
        let t = (s + l / 2) as f32 / sr;
        let mut fr = Frame { t, f0: 0.0, r: 0.0, rms };
        if local >= SILENCE * global {
            for i in 0..l {
                buf[i] = (seg[i] - mean) * win[i];
            }
            fft.autocorr(&buf, &mut ac);
            if ac[0] > 0.0 {
                let r = |k: usize| (ac[k] / ac[0]) / (rw[k] / rw0).max(1e-6);
                let mut best = (f32::MIN, 0.0, 0.0);
                for k in tmin.max(1)..tmax {
                    let (a, b, c) = (r(k - 1), r(k), r(k + 1));
                    if b > a && b >= c && b > 0.5 * VOICING {
                        let den = a - 2.0 * b + c;
                        let d = if den.abs() > 1e-12 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
                        let tau = k as f32 + d;
                        let rr = (b - 0.25 * (a - c) * d).min(1.0);
                        let strength = rr - OCTAVE_COST * (FMIN * tau / sr).log2();
                        if strength > best.0 {
                            best = (strength, tau, rr);
                        }
                    }
                }
                if best.2 > VOICING {
                    fr.f0 = sr / best.1;
                    fr.r = best.2;
                }
            }
        }
        frames.push(fr);
        s += hop;
    }
    // 5-point median over voiced neighbours removes isolated octave jumps;
    // runs shorter than 3 frames are treated as unvoiced.
    let f0s: Vec<f32> = frames.iter().map(|f| f.f0).collect();
    for i in 0..frames.len() {
        if f0s[i] == 0.0 {
            continue;
        }
        let mut v: Vec<f32> = f0s[i.saturating_sub(2)..(i + 3).min(f0s.len())].iter().copied().filter(|&f| f > 0.0).collect();
        v.sort_by(f32::total_cmp);
        frames[i].f0 = v[v.len() / 2];
    }
    let mut i = 0;
    while i < frames.len() {
        if frames[i].f0 > 0.0 {
            let j = (i..frames.len()).find(|&k| frames[k].f0 == 0.0).unwrap_or(frames.len());
            if j - i < 3 {
                frames[i..j].iter_mut().for_each(|f| f.f0 = 0.0);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    (frames, hop as f32 / sr)
}

/// Peak-picked glottal cycle marks in voiced runs: (sample position, peak amplitude, run id).
fn period_marks(x: &[f32], sr: f32, frames: &[Frame], hop: f32) -> Vec<(f32, f32, usize)> {
    let mut marks = vec![];
    let mut run = 0;
    let mut i = 0;
    while i < frames.len() {
        if frames[i].f0 == 0.0 {
            i += 1;
            continue;
        }
        let j = (i..frames.len()).find(|&k| frames[k].f0 == 0.0).unwrap_or(frames.len());
        let run_frames = &frames[i..j];
        let a = (((run_frames[0].t - hop) * sr).max(0.0)) as usize;
        let b = (((run_frames[run_frames.len() - 1].t + hop) * sr) as usize).min(x.len());
        let f0_at = |pos: usize| -> f32 {
            let t = pos as f32 / sr;
            let k = run_frames.partition_point(|f| f.t < t);
            if k == 0 {
                run_frames[0].f0
            } else if k >= run_frames.len() {
                run_frames[run_frames.len() - 1].f0
            } else {
                let (p, q) = (&run_frames[k - 1], &run_frames[k]);
                p.f0 + (q.f0 - p.f0) * (t - p.t) / (q.t - p.t)
            }
        };
        let seg = &x[a..b];
        let (mx, mn) = seg.iter().fold((f32::MIN, f32::MAX), |(hi, lo), &v| (hi.max(v), lo.min(v)));
        let sign = if -mn > mx { -1.0 } else { 1.0 };
        let argmax = |lo: usize, hi: usize| -> usize {
            (lo..hi).max_by(|&p, &q| (sign * x[p]).total_cmp(&(sign * x[q]))).unwrap_or(lo)
        };
        let t0 = sr / f0_at(a);
        let mut cur = argmax(a, (a + t0 as usize + 1).min(b));
        loop {
            let (y0, y1, y2) = if cur > 0 && cur + 1 < x.len() {
                (sign * x[cur - 1], sign * x[cur], sign * x[cur + 1])
            } else {
                (0.0, sign * x[cur], 0.0)
            };
            let den = y0 - 2.0 * y1 + y2;
            let d = if den.abs() > 1e-12 { (0.5 * (y0 - y2) / den).clamp(-0.5, 0.5) } else { 0.0 };
            marks.push((cur as f32 + d, y1 - 0.25 * (y0 - y2) * d, run));
            let t = sr / f0_at(cur);
            let (lo, hi) = (cur + (0.8 * t) as usize, cur + (1.2 * t) as usize + 1);
            if hi >= b {
                break;
            }
            cur = argmax(lo, hi);
        }
        run += 1;
        i = j;
    }
    marks
}

fn mean(v: &[f32]) -> f32 {
    v.iter().sum::<f32>() / v.len() as f32
}

pub fn analyze(x: &[f32], sr: f32) -> VoiceReport {
    let mut rep = VoiceReport { duration: x.len() as f32 / sr, sample_rate: sr as u32, ..Default::default() };
    if rep.duration < 0.1 {
        rep.findings.push("Odabir je prekratak za analizu (minimalno 100 ms).".into());
        rep.report = rep.findings[0].clone();
        return rep;
    }
    let (frames, hop) = pitch_track(x, sr);
    rep.pitch = frames.iter().map(|f| [f.t, f.f0]).collect();
    let voiced: Vec<&Frame> = frames.iter().filter(|f| f.f0 > 0.0).collect();
    rep.voiced_fraction = if frames.is_empty() { 0.0 } else { voiced.len() as f32 / frames.len() as f32 };
    if voiced.len() >= 3 {
        let mut f0: Vec<f32> = voiced.iter().map(|f| f.f0).collect();
        let m = mean(&f0);
        rep.f0_mean = Some(m);
        rep.f0_sd = Some((f0.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / f0.len() as f32).sqrt());
        f0.sort_by(f32::total_cmp);
        rep.f0_median = Some(f0[f0.len() / 2]);
        rep.f0_min = f0.first().copied();
        rep.f0_max = f0.last().copied();
        let hnr: Vec<f32> = voiced.iter().map(|f| {
            let r = f.r.clamp(1e-6, 0.99999);
            10.0 * (r / (1.0 - r)).log10()
        }).collect();
        rep.hnr_db = Some(mean(&hnr));
        let pw = mean(&voiced.iter().map(|f| f.rms * f.rms).collect::<Vec<_>>());
        rep.intensity_dbfs = Some(10.0 * (pw + 1e-12).log10());

        let marks = period_marks(x, sr, &frames, hop);
        // (period s, amplitude, run)
        let per: Vec<(f32, f32, usize)> = marks
            .windows(2)
            .filter(|w| w[0].2 == w[1].2)
            .map(|w| ((w[1].0 - w[0].0) / sr, w[0].1, w[0].2))
            .filter(|p| p.0 >= 1.0 / (FMAX * 1.25) && p.0 <= 1.25 / FMIN)
            .collect();
        rep.periods = per.len();
        if per.len() >= 10 {
            let mean_t = mean(&per.iter().map(|p| p.0).collect::<Vec<_>>());
            let mean_a = mean(&per.iter().map(|p| p.1.abs()).collect::<Vec<_>>());
            let (mut jd, mut jn, mut sd, mut sdb, mut sn, mut rap, mut rn) = (0.0, 0, 0.0, 0.0, 0, 0.0, 0);
            for w in per.windows(2) {
                let (p, q) = (w[0], w[1]);
                if p.2 != q.2 {
                    continue;
                }
                if p.0.max(q.0) / p.0.min(q.0) <= MAX_PERIOD_FACTOR {
                    jd += (p.0 - q.0).abs();
                    jn += 1;
                }
                let (a, b) = (p.1.abs().max(1e-9), q.1.abs().max(1e-9));
                if a.max(b) / a.min(b) <= MAX_AMP_FACTOR {
                    sd += (a - b).abs();
                    sdb += (20.0 * (b / a).log10()).abs();
                    sn += 1;
                }
            }
            for w in per.windows(3) {
                if w[0].2 == w[2].2 && w[0].0.max(w[1].0).max(w[2].0) / w[0].0.min(w[1].0).min(w[2].0) <= MAX_PERIOD_FACTOR {
                    rap += (w[1].0 - (w[0].0 + w[1].0 + w[2].0) / 3.0).abs();
                    rn += 1;
                }
            }
            if jn > 0 {
                rep.jitter_local = Some(jd / jn as f32 / mean_t * 100.0);
                rep.jitter_abs_us = Some(jd / jn as f32 * 1e6);
            }
            if rn > 0 {
                rep.jitter_rap = Some(rap / rn as f32 / mean_t * 100.0);
            }
            if sn > 0 {
                rep.shimmer_local = Some(sd / sn as f32 / mean_a * 100.0);
                rep.shimmer_db = Some(sdb / sn as f32);
            }
        }
    }
    write_report(&mut rep);
    rep
}

fn write_report(r: &mut VoiceReport) {
    use std::fmt::Write;
    let mut s = String::from("KLINIČKI IZVJEŠTAJ — AKUSTIČKA ANALIZA GLASA\n\n");
    let o = |v: Option<f32>, d: usize| v.map(|x| format!("{x:.d$}")).unwrap_or_else(|| "—".into());
    let _ = writeln!(s, "Trajanje odabira: {:.2} s · zvučni dio: {:.0} %", r.duration, r.voiced_fraction * 100.0);
    let _ = writeln!(
        s,
        "➤ F0 (osnovna frekvencija): {} Hz  (medijan {}, SD {}, raspon {}–{} Hz)",
        o(r.f0_mean, 1), o(r.f0_median, 1), o(r.f0_sd, 1), o(r.f0_min, 1), o(r.f0_max, 1)
    );
    let _ = writeln!(s, "➤ Jitter (local): {} %   [norma < {JITTER_MAX} %]", o(r.jitter_local, 2));
    let _ = writeln!(s, "➤ Jitter (abs): {} µs", o(r.jitter_abs_us, 1));
    let _ = writeln!(s, "➤ Jitter (RAP): {} %", o(r.jitter_rap, 2));
    let _ = writeln!(s, "➤ Shimmer (local): {} %   [norma < {SHIMMER_MAX} %]", o(r.shimmer_local, 2));
    let _ = writeln!(s, "➤ Shimmer (dB): {} dB   [norma < {SHIMMER_DB_MAX} dB]", o(r.shimmer_db, 3));
    let _ = writeln!(s, "➤ HNR (harmoničnost): {} dB   [norma > {HNR_MIN} dB]", o(r.hnr_db, 1));
    let _ = writeln!(s, "➤ Intenzitet (zvučni dio): {} dBFS", o(r.intensity_dbfs, 1));
    let _ = writeln!(s, "➤ Broj analiziranih perioda: {}", r.periods);

    let mut f = vec![];
    let mut bad = false;
    if r.voiced_fraction < 0.2 || r.f0_mean.is_none() {
        f.push("Nije detektirana stabilna fonacija — radi li se o šaptu, tišini ili bezvučnim glasovima?".to_string());
        bad = true;
    } else {
        if let Some(m) = r.f0_mean {
            if !(70.0..=300.0).contains(&m) {
                f.push(format!("F0 ({m:.0} Hz) izvan uobičajenog raspona govornog glasa (70–300 Hz)."));
            }
        }
        if let Some(j) = r.jitter_local {
            if j > JITTER_MAX {
                f.push(format!("Povišen jitter ({j:.2} % > {JITTER_MAX} %): aperiodičnost titranja glasnica, smanjena kontrola (hrapavost)."));
                bad = true;
            }
        }
        if let Some(v) = r.shimmer_local {
            if v > SHIMMER_MAX {
                f.push(format!("Povišen shimmer ({v:.2} % > {SHIMMER_MAX} %): nestabilnost amplitude, promjene glotalnog otpora (šumnost, hukavost)."));
                bad = true;
            }
        }
        if let Some(h) = r.hnr_db {
            if h < HNR_MIN {
                f.push(format!("Snižen HNR ({h:.1} dB < {HNR_MIN} dB): značajan udio šuma u glasu (disfonija)."));
                bad = true;
            }
        }
        if r.jitter_local.is_none() {
            f.push("Premalo stabilnih perioda za jitter/shimmer — snimite produženi vokal /a/ 3–5 s.".into());
        }
    }
    s.push_str("\nZAKLJUČAK:\n");
    if f.is_empty() {
        s.push_str("- NALAZ UREDAN: parametri titranja glasnica su unutar referentnih kliničkih granica.\n");
    }
    for x in &f {
        let _ = writeln!(s, "- {x}");
    }
    s.push_str("\nNapomena: orijentacijski nalaz iz akustičkih mjera (algoritmi po uzoru na Praat/MDVP), \
                nije medicinska dijagnoza. Za usporedive vrijednosti: produženi vokal /a/ 3–5 s, \
                stalna udaljenost mikrofona, tiha prostorija.\n");
    r.normal = !bad && f.is_empty();
    r.findings = f;
    r.report = s;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: f32 = 48000.0;

    /// Glottal-like pulse train with random per-period jitter/shimmer
    /// (uniform ±j, ±a) at sub-sample precision, plus white noise.
    fn voice(secs: f32, f0: f32, j: f32, a: f32, noise: f32) -> Vec<f32> {
        let n = (secs * SR) as usize;
        let mut rng = 12345u32;
        let mut uni = move || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        // Peak instants p_k with random intervals and amplitudes a_k; between
        // peaks the cycle shape is continuous and the envelope crossfades, so
        // the true jitter/shimmer are exactly those of (p_k, a_k).
        let mut peaks = vec![(0.0f64, 0.5 * (1.0 + a * uni()))];
        while peaks.last().unwrap().0 < n as f64 {
            let t = (SR / f0 * (1.0 + j * uni())) as f64;
            peaks.push((peaks.last().unwrap().0 + t, 0.5 * (1.0 + a * uni())));
        }
        let mut x = vec![0.0; n];
        for w in peaks.windows(2) {
            let ((p0, a0), (p1, a1)) = (w[0], w[1]);
            for (i, v) in x.iter_mut().enumerate().take((p1.ceil() as usize).min(n)).skip(p0.ceil() as usize) {
                let u = ((i as f64 - p0) / (p1 - p0)) as f32;
                let g: f32 = (1..=10).map(|h| (2.0 * PI * h as f32 * u).cos() / h as f32).sum::<f32>() / 2.9;
                *v = (a0 * (1.0 - u) + a1 * u) * g;
            }
        }
        for v in x.iter_mut() {
            *v += noise * uni();
        }
        x
    }

    #[test]
    fn clean_voice_is_normal() {
        let r = analyze(&voice(2.0, 150.0, 0.0, 0.0, 0.0), SR);
        let f0 = r.f0_mean.unwrap();
        assert!((f0 - 150.0).abs() < 1.0, "f0 {f0}");
        assert!(r.voiced_fraction > 0.9);
        assert!(r.jitter_local.unwrap() < 0.2, "jitter {:?}", r.jitter_local);
        assert!(r.shimmer_local.unwrap() < 0.5, "shimmer {:?}", r.shimmer_local);
        assert!(r.hnr_db.unwrap() > 25.0, "hnr {:?}", r.hnr_db);
        assert!(r.normal, "{}", r.report);
    }

    #[test]
    fn jitter_and_shimmer_are_measured() {
        // uniform ±3 % periods: E|Ti - Ti+1| = 2/3 · 3 % = 2 % of T
        // uniform ±9 % amplitudes: E|Ai - Ai+1| = 6 % of A
        let r = analyze(&voice(3.0, 200.0, 0.03, 0.09, 0.0), SR);
        let j = r.jitter_local.unwrap();
        let s = r.shimmer_local.unwrap();
        assert!((j - 2.0).abs() < 0.3, "jitter {j}");
        assert!((s - 6.0).abs() < 0.9, "shimmer {s}");
        assert!(!r.normal);
        assert!(r.report.contains("Povišen jitter"));
    }

    #[test]
    fn noise_lowers_hnr() {
        let clean = analyze(&voice(1.5, 120.0, 0.0, 0.0, 0.0), SR).hnr_db.unwrap();
        let noisy = analyze(&voice(1.5, 120.0, 0.0, 0.0, 0.15), SR).hnr_db.unwrap();
        assert!(noisy < clean - 8.0, "clean {clean} noisy {noisy}");
    }

    #[test]
    fn silence_is_reported() {
        let r = analyze(&vec![0.0; 48000], SR);
        assert!(r.f0_mean.is_none());
        assert!(r.report.contains("Nije detektirana stabilna fonacija"));
    }

    #[test]
    fn low_and_high_voices() {
        for f in [85.0, 260.0, 440.0] {
            let r = analyze(&voice(1.0, f, 0.0, 0.0, 0.0), SR);
            let m = r.f0_mean.unwrap();
            assert!((m - f).abs() / f < 0.01, "{f}: {m}");
        }
    }
}
