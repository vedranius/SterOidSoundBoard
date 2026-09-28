//! Acoustic voice analysis (DigiLingua). Measures come from the
//! Praat-compatible engine in [`crate::praat`] (validated against Praat 7:
//! identical pitch, pulses, jitter, shimmer, HNR, voicing and intensity;
//! formants and CPPS within ±0.5 % on clean signals — see VALIDATION.md).
//! This module adds clinical timing measures, a Croatian report and the
//! tracks the analysis editor draws. Control thread only.
use crate::praat::{self, cepstrum, formant, intensity, pitch, pulses, voice, Sound};
use serde::{Deserialize, Serialize};

/// Clinical reference thresholds (MDVP / Praat literature, sustained vowel).
pub const JITTER_MAX: f32 = 1.04; // %
pub const SHIMMER_MAX: f32 = 3.81; // %
pub const SHIMMER_DB_MAX: f32 = 0.35; // dB
pub const HNR_MIN: f32 = 20.0; // dB
/// Offset from Praat's uncalibrated dB (1.0 = 1 Pa) to dBFS.
pub const PRAAT_DB_TO_DBFS: f64 = -93.9794;
/// CPPS is O(n²) per frame (robust trend line): analyse at most this much.
const CPPS_MAX_SECONDS: f64 = 60.0;

/// User-adjustable analysis parameters (Praat names and defaults).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AnalysisSettings {
    pub pitch_floor: f64,
    pub pitch_ceiling: f64,
    /// Formant ceiling: 5500 Hz (women, children), 5000 Hz (men).
    pub max_formant: f64,
    pub n_formants: f64,
    pub cpps: bool,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        AnalysisSettings { pitch_floor: 75.0, pitch_ceiling: 600.0, max_formant: 5500.0, n_formants: 5.0, cpps: true }
    }
}

impl AnalysisSettings {
    /// Clamp to ranges the algorithms support.
    pub fn sanitized(mut self) -> Self {
        self.pitch_floor = if self.pitch_floor.is_finite() { self.pitch_floor.clamp(30.0, 500.0) } else { 75.0 };
        self.pitch_ceiling = if self.pitch_ceiling.is_finite() { self.pitch_ceiling.clamp(self.pitch_floor + 20.0, 1500.0) } else { 600.0 };
        self.max_formant = if self.max_formant.is_finite() { self.max_formant.clamp(2000.0, 8000.0) } else { 5500.0 };
        self.n_formants = if self.n_formants.is_finite() { self.n_formants.clamp(3.0, 7.0) } else { 5.0 };
        self
    }
    fn pitch_params(&self) -> pitch::PitchParams {
        pitch::PitchParams { floor: self.pitch_floor, ceiling: self.pitch_ceiling, ..Default::default() }
    }
    fn formant_params(&self) -> formant::FormantParams {
        formant::FormantParams { max_formant: self.max_formant, n_formants: self.n_formants, ..Default::default() }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VoiceReport {
    pub settings: AnalysisSettings,
    pub duration: f32,
    pub sample_rate: u32,
    /// Fraction of pitch frames on the voiced path (0–1).
    pub voiced_fraction: f32,
    pub f0_mean: Option<f32>,
    pub f0_median: Option<f32>,
    pub f0_sd: Option<f32>,
    pub f0_min: Option<f32>,
    pub f0_max: Option<f32>,
    /// F0 range between the 5th and 95th percentile, semitones.
    pub f0_range_st: Option<f32>,
    // --- Praat voice report (percentages unless noted)
    pub pulses: usize,
    pub periods: usize,
    pub mean_period_ms: Option<f32>,
    pub sd_period_ms: Option<f32>,
    /// Praat "fraction of locally unvoiced frames", %.
    pub unvoiced_fraction: f32,
    pub voice_breaks: usize,
    /// Praat "degree of voice breaks", %.
    pub voice_break_degree: f32,
    pub jitter_local: Option<f32>,
    /// Jitter (local, absolute), µs.
    pub jitter_abs_us: Option<f32>,
    pub jitter_rap: Option<f32>,
    pub jitter_ppq5: Option<f32>,
    pub jitter_ddp: Option<f32>,
    pub shimmer_local: Option<f32>,
    /// Shimmer (local, dB).
    pub shimmer_db: Option<f32>,
    pub shimmer_apq3: Option<f32>,
    pub shimmer_apq5: Option<f32>,
    pub shimmer_apq11: Option<f32>,
    pub shimmer_dda: Option<f32>,
    pub mean_autocorrelation: Option<f32>,
    pub nhr: Option<f32>,
    /// Harmonics-to-noise ratio, dB (mean over voiced frames).
    pub hnr_db: Option<f32>,
    // --- spectral / cepstral
    /// Smoothed cepstral peak prominence (AVQI settings), dB.
    pub cpps: Option<f32>,
    pub cpps_note: Option<String>,
    /// Median F1–F4 over voiced frames, Hz.
    pub formants: [Option<f32>; 4],
    // --- intensity (Praat scale: uncalibrated dB, sample 1.0 = 1 Pa)
    pub intensity_mean_db: Option<f32>,
    pub intensity_min_db: Option<f32>,
    pub intensity_max_db: Option<f32>,
    pub intensity_sd_db: Option<f32>,
    /// Mean level of voiced frames, dBFS.
    pub intensity_dbfs: Option<f32>,
    // --- timing / fluency
    /// Longest continuous voiced stretch, s (maximum phonation time on a sustained vowel).
    pub max_voiced_s: f32,
    /// Silent pauses ≥ 250 ms inside the utterance.
    pub pauses: usize,
    pub pause_mean_s: f32,
    /// Share of the utterance spent in silent pauses, %.
    pub pause_ratio: f32,
    /// Syllable nuclei (intensity peaks in voiced frames, de Jong & Wempe 2009).
    pub syllable_nuclei: usize,
    /// Nuclei per second of the whole utterance (incl. pauses).
    pub speech_rate: f32,
    /// Nuclei per second of speaking time (pauses excluded).
    pub articulation_rate: f32,
    /// [time s, f0 Hz or 0 when unvoiced] per pitch frame.
    pub pitch: Vec<[f32; 2]>,
    /// True if every measured parameter is inside the reference range.
    pub normal: bool,
    pub findings: Vec<String>,
    /// Human-readable clinical report (Croatian).
    pub report: String,
}

/// Contours for the analysis editor (times relative to the analysed sound).
#[derive(Debug, Clone, Serialize, Default)]
pub struct Tracks {
    pub duration: f32,
    pub sample_rate: u32,
    pub settings: AnalysisSettings,
    /// [t, f0 (0 = unvoiced), strength]
    pub pitch: Vec<[f32; 3]>,
    /// [t, dB (Praat scale)]
    pub intensity: Vec<[f32; 2]>,
    /// [t, F1, B1, F2, B2, …] (0 = none), only frames above the silence level.
    pub formants: Vec<Vec<f32>>,
    /// Glottal pulse times.
    pub pulses: Vec<f32>,
}

fn f(v: Option<f64>) -> Option<f32> {
    v.map(|x| x as f32)
}
fn pct(v: Option<f64>) -> Option<f32> {
    v.map(|x| (x * 100.0) as f32)
}

pub fn analyze(x: &[f32], sr: f32) -> VoiceReport {
    analyze_with(x, sr, &AnalysisSettings::default())
}

pub fn analyze_with(x: &[f32], sr: f32, settings: &AnalysisSettings) -> VoiceReport {
    let st = settings.sanitized();
    let mut rep = VoiceReport { settings: st, duration: x.len() as f32 / sr, sample_rate: sr as u32, ..Default::default() };
    if rep.duration < 0.1 || (x.len() as f64) < 6.4 / 100.0 * sr as f64 {
        rep.findings.push("Odabir je prekratak za analizu (minimalno 100 ms).".into());
        rep.report = rep.findings[0].clone();
        return rep;
    }
    let s = Sound::new(x, sr);
    let pp = st.pitch_params();
    let pt = pitch::to_pitch(&s, &pp);
    let pl = pulses::to_pulses(&s, &pt);
    let vr = voice::voice_report(&s, &pt, &pl, 0.0, s.duration(), &pp);
    let it = intensity::to_intensity(&s, 100.0, 0.0);

    rep.pitch = (0..pt.len()).map(|i| [pt.time(i) as f32, pt.f0[i] as f32]).collect();
    rep.voiced_fraction = if pt.len() > 0 { (0..pt.len()).filter(|&i| pt.voiced(i)).count() as f32 / pt.len() as f32 } else { 0.0 };
    rep.f0_mean = f(vr.mean_pitch);
    rep.f0_median = f(vr.median_pitch);
    rep.f0_sd = f(vr.sd_pitch);
    rep.f0_min = f(vr.min_pitch);
    rep.f0_max = f(vr.max_pitch);
    let mut f0s: Vec<f64> = pt.f0.iter().copied().filter(|&v| v > 0.0).collect();
    f0s.sort_by(f64::total_cmp);
    if f0s.len() >= 10 {
        if let (Some(a), Some(b)) = (praat::quantile(&f0s, 0.05), praat::quantile(&f0s, 0.95)) {
            rep.f0_range_st = Some((12.0 * (b / a).log2()) as f32);
        }
    }
    rep.pulses = vr.pulses;
    rep.periods = vr.periods;
    rep.mean_period_ms = vr.mean_period.map(|v| (v * 1000.0) as f32);
    rep.sd_period_ms = vr.sd_period.map(|v| (v * 1000.0) as f32);
    rep.unvoiced_fraction = (vr.unvoiced_fraction * 100.0) as f32;
    rep.voice_breaks = vr.voice_breaks;
    rep.voice_break_degree = (vr.voice_break_degree * 100.0) as f32;
    rep.jitter_local = pct(vr.jitter_local);
    rep.jitter_abs_us = vr.jitter_local_abs.map(|v| (v * 1e6) as f32);
    rep.jitter_rap = pct(vr.jitter_rap);
    rep.jitter_ppq5 = pct(vr.jitter_ppq5);
    rep.jitter_ddp = pct(vr.jitter_ddp);
    rep.shimmer_local = pct(vr.shimmer_local);
    rep.shimmer_db = f(vr.shimmer_local_db);
    rep.shimmer_apq3 = pct(vr.shimmer_apq3);
    rep.shimmer_apq5 = pct(vr.shimmer_apq5);
    rep.shimmer_apq11 = pct(vr.shimmer_apq11);
    rep.shimmer_dda = pct(vr.shimmer_dda);
    rep.mean_autocorrelation = f(vr.mean_autocorrelation);
    rep.nhr = f(vr.mean_nhr);
    rep.hnr_db = f(vr.mean_hnr);

    // intensity statistics (whole selection) and level of the voiced parts
    let db: Vec<f64> = it.db.iter().copied().filter(|&v| v > -299.0).collect();
    if !db.is_empty() {
        let m = db.iter().sum::<f64>() / db.len() as f64;
        rep.intensity_mean_db = Some(m as f32);
        rep.intensity_min_db = f(db.iter().copied().reduce(f64::min));
        rep.intensity_max_db = f(db.iter().copied().reduce(f64::max));
        if db.len() > 1 {
            rep.intensity_sd_db = Some((db.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (db.len() - 1) as f64).sqrt() as f32);
        }
    }
    let voiced_db: Vec<f64> = (0..it.db.len()).filter(|&i| pt.value_at(it.time(i)).is_some()).map(|i| it.db[i]).collect();
    if !voiced_db.is_empty() {
        let pw = voiced_db.iter().map(|v| 10f64.powf(v / 10.0)).sum::<f64>() / voiced_db.len() as f64;
        rep.intensity_dbfs = Some((10.0 * pw.log10() + PRAAT_DB_TO_DBFS) as f32);
    }

    // formants: medians over voiced frames
    if !f0s.is_empty() {
        let fr = formant::to_formants(&s, &st.formant_params());
        for k in 0..4 {
            let mut v: Vec<f64> =
                fr.iter().filter(|x| pt.value_at(x.t).is_some()).filter_map(|x| x.formants.get(k).map(|p| p.0)).collect();
            v.sort_by(f64::total_cmp);
            rep.formants[k] = f(praat::quantile(&v, 0.5));
        }
    }
    // CPPS (bounded cost on very long selections)
    if st.cpps && !f0s.is_empty() {
        let limit = (CPPS_MAX_SECONDS * s.sr) as usize;
        let cs = if s.x.len() > limit {
            rep.cpps_note = Some(format!("CPPS izračunat na prvih {CPPS_MAX_SECONDS:.0} s odabira."));
            Sound { x: s.x[..limit].to_vec(), sr: s.sr }
        } else {
            s.clone()
        };
        rep.cpps = f(cepstrum::cpps(&cs, &cepstrum::CppsParams::default()));
    }
    temporal(&mut rep, &pt, &it);
    write_report(&mut rep);
    rep
}

/// Contours for drawing (pitch, intensity, formants, pulses).
pub fn tracks(x: &[f32], sr: f32, settings: &AnalysisSettings) -> Tracks {
    let st = settings.sanitized();
    let mut out = Tracks { duration: x.len() as f32 / sr, sample_rate: sr as u32, settings: st, ..Default::default() };
    if (x.len() as f64) < 0.1 * sr as f64 {
        return out;
    }
    let s = Sound::new(x, sr);
    let pp = st.pitch_params();
    let pt = pitch::to_pitch(&s, &pp);
    out.pitch = (0..pt.len()).map(|i| [pt.time(i) as f32, pt.f0[i] as f32, pt.strength[i] as f32]).collect();
    out.pulses = pulses::to_pulses(&s, &pt).into_iter().map(|t| t as f32).collect();
    let it = intensity::to_intensity(&s, 100.0, 0.0);
    out.intensity = (0..it.db.len()).map(|i| [it.time(i) as f32, it.db[i].max(0.0) as f32]).collect();
    let peak = it.db.iter().copied().fold(f64::MIN, f64::max);
    out.formants = formant::to_formants(&s, &st.formant_params())
        .into_iter()
        .filter(|fr| it.value_at(fr.t).is_some_and(|d| d > peak - 30.0))
        .map(|fr| {
            let mut v = vec![fr.t as f32];
            for k in 0..(st.n_formants.ceil() as usize) {
                let (fq, bw) = fr.formants.get(k).copied().unwrap_or((0.0, 0.0));
                v.push(fq as f32);
                v.push(bw as f32);
            }
            v
        })
        .collect();
    out
}

const PAUSE_MIN_S: f64 = 0.25;
const SILENCE_BELOW_PEAK_DB: f64 = 25.0;
const NUCLEUS_DIP_DB: f64 = 2.0;
/// Absolute silence floor on Praat's scale (≈ −70 dBFS).
const ABS_SILENCE_DB: f64 = 24.0;

/// Timing and prosody measures for fluency (stuttering, cluttering) and
/// connected speech, from the Praat pitch and intensity contours.
fn temporal(r: &mut VoiceReport, pt: &pitch::Pitch, it: &intensity::Intensity) {
    let (mut best, mut run) = (0usize, 0usize);
    for i in 0..pt.len() {
        run = if pt.voiced(i) { run + 1 } else { 0 };
        best = best.max(run);
    }
    r.max_voiced_s = (best as f64 * pt.dt) as f32;
    let db = &it.db;
    if db.is_empty() {
        return;
    }
    let mut sorted = db.clone();
    sorted.sort_by(f64::total_cmp);
    let q99 = sorted[((sorted.len() - 1) as f64 * 0.99) as usize];
    let thr = (q99 - SILENCE_BELOW_PEAK_DB).max(ABS_SILENCE_DB);
    let sounding: Vec<bool> = db.iter().map(|&d| d > thr).collect();
    let (Some(a), Some(b)) = (sounding.iter().position(|&v| v), sounding.iter().rposition(|&v| v)) else { return };
    let hop = it.dt;
    let span_s = (b - a + 1) as f64 * hop;
    let min_frames = (PAUSE_MIN_S / hop).ceil() as usize;
    let (mut pauses, mut pause_frames, mut i) = (0usize, 0usize, a);
    while i <= b {
        if sounding[i] {
            i += 1;
            continue;
        }
        let j = (i..=b).find(|&k| sounding[k]).unwrap_or(b + 1);
        if j - i >= min_frames {
            pauses += 1;
            pause_frames += j - i;
        }
        i = j;
    }
    r.pauses = pauses;
    r.pause_mean_s = if pauses > 0 { (pause_frames as f64 * hop / pauses as f64) as f32 } else { 0.0 };
    r.pause_ratio = (pause_frames as f64 * hop / span_s * 100.0) as f32;
    let mut kept: Vec<usize> = vec![];
    for k in (a + 1)..b {
        if !(db[k] >= db[k - 1] && db[k] > db[k + 1] && sounding[k] && pt.value_at(it.time(k)).is_some()) {
            continue;
        }
        match kept.last().copied() {
            None => kept.push(k),
            Some(last) => {
                let dip = db[last..=k].iter().copied().fold(f64::MAX, f64::min);
                if db[k] - dip >= NUCLEUS_DIP_DB && db[last] - dip >= NUCLEUS_DIP_DB {
                    kept.push(k);
                } else if db[k] > db[last] {
                    *kept.last_mut().unwrap() = k; // same syllable, higher peak
                }
            }
        }
    }
    r.syllable_nuclei = kept.len();
    r.speech_rate = (kept.len() as f64 / span_s) as f32;
    let speaking = span_s - pause_frames as f64 * hop;
    r.articulation_rate = if speaking > 0.0 { (kept.len() as f64 / speaking) as f32 } else { 0.0 };
}

fn write_report(r: &mut VoiceReport) {
    localize(r, false);
}

/// (Re)write the findings and the clinical report text in Croatian or English.
pub fn localize(r: &mut VoiceReport, en: bool) {
    use std::fmt::Write;
    let l = |hr: &'static str, e: &'static str| if en { e } else { hr };
    if r.duration < 0.1 && r.f0_mean.is_none() && r.pulses == 0 {
        let m = l("Odabir je prekratak za analizu (minimalno 100 ms).", "The selection is too short to analyse (at least 100 ms).").to_string();
        r.findings = vec![m.clone()];
        r.report = m;
        return;
    }
    let mut s = String::from(l("KLINIČKI IZVJEŠTAJ — AKUSTIČKA ANALIZA GLASA\n", "CLINICAL REPORT — ACOUSTIC VOICE ANALYSIS\n"));
    let o = |v: Option<f32>, d: usize| v.map(|x| format!("{x:.d$}")).unwrap_or_else(|| "—".into());
    let st = &r.settings;
    let _ = writeln!(
        s,
        "{}\n",
        if en {
            format!("(Praat algorithms · F0 range {:.0}–{:.0} Hz · maximum formant {:.0} Hz)", st.pitch_floor, st.pitch_ceiling, st.max_formant)
        } else {
            format!("(algoritmi Praat · raspon F0 {:.0}–{:.0} Hz · maks. formant {:.0} Hz)", st.pitch_floor, st.pitch_ceiling, st.max_formant)
        }
    );
    if en {
        let _ = writeln!(s, "Selection duration: {:.2} s · voiced part: {:.0} %", r.duration, r.voiced_fraction * 100.0);
        s.push_str("\nPITCH (F0):\n");
        let _ = writeln!(s, "➤ F0 mean {} Hz · median {} Hz · SD {} Hz · range {}–{} Hz ({} semitones, 5th–95th pct.)", o(r.f0_mean, 1), o(r.f0_median, 1), o(r.f0_sd, 1), o(r.f0_min, 1), o(r.f0_max, 1), o(r.f0_range_st, 1));
        s.push_str("\nPULSES AND VOICING:\n");
        let _ = writeln!(s, "➤ Pulses: {} · periods: {} · mean period {} ms (SD {} ms)", r.pulses, r.periods, o(r.mean_period_ms, 3), o(r.sd_period_ms, 3));
        let _ = writeln!(s, "➤ Fraction of locally unvoiced frames: {:.1} %", r.unvoiced_fraction);
        let _ = writeln!(s, "➤ Voice breaks: {} (degree {:.1} %)", r.voice_breaks, r.voice_break_degree);
        s.push_str("\nJITTER:\n");
        let _ = writeln!(s, "➤ Jitter (local): {} %   [norm < {JITTER_MAX} %]", o(r.jitter_local, 3));
        let _ = writeln!(s, "➤ Jitter (local, abs): {} µs · RAP {} % · PPQ5 {} % · DDP {} %", o(r.jitter_abs_us, 1), o(r.jitter_rap, 3), o(r.jitter_ppq5, 3), o(r.jitter_ddp, 3));
        s.push_str("\nSHIMMER:\n");
        let _ = writeln!(s, "➤ Shimmer (local): {} %   [norm < {SHIMMER_MAX} %]", o(r.shimmer_local, 3));
        let _ = writeln!(s, "➤ Shimmer (local, dB): {} dB   [norm < {SHIMMER_DB_MAX} dB]", o(r.shimmer_db, 3));
        let _ = writeln!(s, "➤ APQ3 {} % · APQ5 {} % · APQ11 {} % · DDA {} %", o(r.shimmer_apq3, 3), o(r.shimmer_apq5, 3), o(r.shimmer_apq11, 3), o(r.shimmer_dda, 3));
        s.push_str("\nHARMONICITY AND SPECTRUM:\n");
        let _ = writeln!(s, "➤ HNR: {} dB   [norm > {HNR_MIN} dB] · NHR {} · mean autocorrelation {}", o(r.hnr_db, 2), o(r.nhr, 4), o(r.mean_autocorrelation, 4));
        let _ = writeln!(s, "➤ CPPS: {} dB{}", o(r.cpps, 2), if r.cpps_note.is_some() { " (computed on the first 60 s of the selection)".to_string() } else { String::new() });
        let _ = writeln!(s, "➤ Formants (median, voiced frames): F1 {} · F2 {} · F3 {} · F4 {} Hz", o(r.formants[0], 0), o(r.formants[1], 0), o(r.formants[2], 0), o(r.formants[3], 0));
        s.push_str("\nINTENSITY (Praat scale, uncalibrated):\n");
        let _ = writeln!(s, "➤ Mean {} dB · min {} · max {} · SD {} dB · voiced part {} dBFS", o(r.intensity_mean_db, 1), o(r.intensity_min_db, 1), o(r.intensity_max_db, 1), o(r.intensity_sd_db, 1), o(r.intensity_dbfs, 1));
        s.push_str("\nTIMING / FLUENCY:\n");
        let _ = writeln!(s, "➤ Longest uninterrupted phonation: {:.2} s", r.max_voiced_s);
        let _ = writeln!(s, "➤ Pauses ≥ 250 ms: {} (mean {:.2} s, {:.1} % of the duration)", r.pauses, r.pause_mean_s, r.pause_ratio);
        let _ = writeln!(s, "➤ Syllable nuclei: {} · speech rate {:.2} syll/s · articulation rate {:.2} syll/s", r.syllable_nuclei, r.speech_rate, r.articulation_rate);
    } else {
        let _ = writeln!(s, "Trajanje odabira: {:.2} s · zvučni dio: {:.0} %", r.duration, r.voiced_fraction * 100.0);
        s.push_str("\nVISINA (F0):\n");
        let _ = writeln!(s, "➤ F0 srednja {} Hz · medijan {} Hz · SD {} Hz · raspon {}–{} Hz ({} polutonova, 5.–95. pct.)", o(r.f0_mean, 1), o(r.f0_median, 1), o(r.f0_sd, 1), o(r.f0_min, 1), o(r.f0_max, 1), o(r.f0_range_st, 1));
        s.push_str("\nPULSEVI I ZVUČNOST:\n");
        let _ = writeln!(s, "➤ Pulsevi: {} · periode: {} · srednja perioda {} ms (SD {} ms)", r.pulses, r.periods, o(r.mean_period_ms, 3), o(r.sd_period_ms, 3));
        let _ = writeln!(s, "➤ Udio lokalno bezvučnih okvira: {:.1} %", r.unvoiced_fraction);
        let _ = writeln!(s, "➤ Prekidi zvučnosti: {} (stupanj {:.1} %)", r.voice_breaks, r.voice_break_degree);
        s.push_str("\nJITTER:\n");
        let _ = writeln!(s, "➤ Jitter (local): {} %   [norma < {JITTER_MAX} %]", o(r.jitter_local, 3));
        let _ = writeln!(s, "➤ Jitter (local, abs): {} µs · RAP {} % · PPQ5 {} % · DDP {} %", o(r.jitter_abs_us, 1), o(r.jitter_rap, 3), o(r.jitter_ppq5, 3), o(r.jitter_ddp, 3));
        s.push_str("\nSHIMMER:\n");
        let _ = writeln!(s, "➤ Shimmer (local): {} %   [norma < {SHIMMER_MAX} %]", o(r.shimmer_local, 3));
        let _ = writeln!(s, "➤ Shimmer (local, dB): {} dB   [norma < {SHIMMER_DB_MAX} dB]", o(r.shimmer_db, 3));
        let _ = writeln!(s, "➤ APQ3 {} % · APQ5 {} % · APQ11 {} % · DDA {} %", o(r.shimmer_apq3, 3), o(r.shimmer_apq5, 3), o(r.shimmer_apq11, 3), o(r.shimmer_dda, 3));
        s.push_str("\nHARMONIČNOST I SPEKTAR:\n");
        let _ = writeln!(s, "➤ HNR: {} dB   [norma > {HNR_MIN} dB] · NHR {} · srednja autokorelacija {}", o(r.hnr_db, 2), o(r.nhr, 4), o(r.mean_autocorrelation, 4));
        let _ = writeln!(s, "➤ CPPS: {} dB{}", o(r.cpps, 2), r.cpps_note.as_ref().map(|n| format!(" ({n})")).unwrap_or_default());
        let _ = writeln!(s, "➤ Formanti (medijan, zvučni okviri): F1 {} · F2 {} · F3 {} · F4 {} Hz", o(r.formants[0], 0), o(r.formants[1], 0), o(r.formants[2], 0), o(r.formants[3], 0));
        s.push_str("\nINTENZITET (Praat skala, nekalibrirano):\n");
        let _ = writeln!(s, "➤ Srednji {} dB · min {} · maks {} · SD {} dB · zvučni dio {} dBFS", o(r.intensity_mean_db, 1), o(r.intensity_min_db, 1), o(r.intensity_max_db, 1), o(r.intensity_sd_db, 1), o(r.intensity_dbfs, 1));
        s.push_str("\nVREMENSKI PARAMETRI / TEČNOST:\n");
        let _ = writeln!(s, "➤ Najduža neprekinuta fonacija: {:.2} s", r.max_voiced_s);
        let _ = writeln!(s, "➤ Pauze ≥ 250 ms: {} (prosjek {:.2} s, {:.1} % trajanja)", r.pauses, r.pause_mean_s, r.pause_ratio);
        let _ = writeln!(s, "➤ Slogovne jezgre: {} · brzina govora {:.2} slog/s · brzina artikulacije {:.2} slog/s", r.syllable_nuclei, r.speech_rate, r.articulation_rate);
    }

    let mut f = vec![];
    let mut bad = false;
    if r.voiced_fraction < 0.2 || r.f0_mean.is_none() {
        f.push(l("Nije detektirana stabilna fonacija — radi li se o šaptu, tišini ili bezvučnim glasovima? Provjerite i raspon F0 u postavkama analize.",
                 "No stable phonation detected — is this whisper, silence or voiceless sounds? Also check the F0 range in the analysis settings.").to_string());
        bad = true;
    } else {
        if let Some(m) = r.f0_mean {
            if !(70.0..=300.0).contains(&m) {
                f.push(if en { format!("F0 ({m:.0} Hz) outside the usual range of the speaking voice (70–300 Hz).") } else { format!("F0 ({m:.0} Hz) izvan uobičajenog raspona govornog glasa (70–300 Hz).") });
            }
        }
        if let Some(j) = r.jitter_local {
            if j > JITTER_MAX {
                f.push(if en { format!("Raised jitter ({j:.2} % > {JITTER_MAX} %): aperiodic vocal fold vibration, reduced control (roughness).") } else { format!("Povišen jitter ({j:.2} % > {JITTER_MAX} %): aperiodičnost titranja glasnica, smanjena kontrola (hrapavost).") });
                bad = true;
            }
        }
        if let Some(v) = r.shimmer_local {
            if v > SHIMMER_MAX {
                f.push(if en { format!("Raised shimmer ({v:.2} % > {SHIMMER_MAX} %): amplitude instability, changing glottal resistance (noisiness, breathiness).") } else { format!("Povišen shimmer ({v:.2} % > {SHIMMER_MAX} %): nestabilnost amplitude, promjene glotalnog otpora (šumnost, hukavost).") });
                bad = true;
            }
        }
        if let Some(h) = r.hnr_db {
            if h < HNR_MIN {
                f.push(if en { format!("Lowered HNR ({h:.1} dB < {HNR_MIN} dB): a considerable noise component in the voice (dysphonia).") } else { format!("Snižen HNR ({h:.1} dB < {HNR_MIN} dB): značajan udio šuma u glasu (disfonija).") });
                bad = true;
            }
        }
        if r.jitter_local.is_none() {
            f.push(l("Premalo stabilnih perioda za jitter/shimmer — snimite produženi vokal /a/ 3–5 s.", "Too few stable periods for jitter/shimmer — record a sustained vowel /a/ of 3–5 s.").to_string());
        }
    }
    s.push_str(l("\nZAKLJUČAK:\n", "\nCONCLUSION:\n"));
    if f.is_empty() {
        s.push_str(l("- NALAZ UREDAN: parametri titranja glasnica su unutar referentnih kliničkih granica.\n", "- NORMAL FINDING: vocal fold vibration parameters are within the clinical reference limits.\n"));
    }
    for x in &f {
        let _ = writeln!(s, "- {x}");
    }
    s.push_str(l("\nNapomena: mjere izračunate algoritmima Praata (P. Boersma i D. Weenink), provjereno \
                podudaranje s Praatom 7. Orijentacijski nalaz, nije medicinska dijagnoza. Norme vrijede \
                za produženi vokal /a/ 3–5 s, stalnu udaljenost mikrofona i tihu prostoriju.\n",
                "\nNote: measures computed with Praat's algorithms (P. Boersma and D. Weenink), verified \
                against Praat 7. An orientation finding, not a medical diagnosis. Norms apply to a \
                sustained vowel /a/ of 3–5 s, a constant microphone distance and a quiet room.\n"));
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
    fn pauses_and_syllable_rate() {
        // 1 s voice, 0.6 s silence, 1 s voice at 4 syllables/s (deep 4 Hz AM)
        let v = voice(1.0, 130.0, 0.0, 0.0, 0.0);
        let mut x = v.clone();
        x.extend(std::iter::repeat_n(0.0, (0.6 * SR) as usize));
        x.extend(v.iter().enumerate().map(|(i, s)| s * (0.5 - 0.5 * (2.0 * PI * 4.0 * i as f32 / SR).cos())));
        let r = analyze(&x, SR);
        assert_eq!(r.pauses, 1, "{}", r.report);
        assert!((r.pause_mean_s - 0.6).abs() < 0.08, "pause {}", r.pause_mean_s);
        assert!((r.max_voiced_s - 1.0).abs() < 0.1, "mpt {}", r.max_voiced_s);
        // second second holds 4 nuclei, first second a steady vowel (1 nucleus)
        assert!((4..=6).contains(&r.syllable_nuclei), "nuclei {}", r.syllable_nuclei);
        assert!(r.report.contains("Pauze"));
    }

    #[test]
    fn f0_range_in_semitones() {
        // glide 100 -> 200 Hz is one octave = 12 st; 5-95 % percentiles give ~10.8
        let n = (2.0 * SR) as usize;
        let mut ph = 0.0f32;
        let x: Vec<f32> = (0..n)
            .map(|i| {
                let f = 100.0 * 2f32.powf(i as f32 / n as f32);
                ph += 2.0 * PI * f / SR;
                (1..=8).map(|h| (h as f32 * ph).cos() / h as f32).sum::<f32>() * 0.2
            })
            .collect();
        let st = analyze(&x, SR).f0_range_st.unwrap();
        assert!((9.5..12.5).contains(&st), "{st}");
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
