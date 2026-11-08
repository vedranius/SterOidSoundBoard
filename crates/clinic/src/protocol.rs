//! Assessment protocols for phoneticians and speech-language pathologists:
//! standard speech tasks, session protocol templates, and the clinical voice
//! profile computed from the task recordings (MPT, s/z ratio, F0 range,
//! DSI). Shared by the desktop app, the browser edition and the MCP server.
use crate::clinic::Recording;
use serde::Serialize;
use steroid_engine::analysis::VoiceReport;

/// Speech tasks. The Croatian names are stored with each recording (stable
/// keys); the UI translates them.
pub const TASKS: &[&str] = &[
    "Produženi vokal /a/",
    "Produženi vokal /i/",
    "Produženi vokal /u/",
    "Produženi /s/",
    "Produženi /z/",
    "Najtiša fonacija /a/",
    "Najglasnija fonacija /a/",
    "Glasovni raspon / glisando",
    "Čitanje standardnog teksta",
    "Spontani govor",
    "Opis slike",
    "Brojanje / automatizirani govor",
    "Ponavljanje rečenica",
    "Dijadohokineza (pa-ta-ka)",
    "Pjevanje",
    "Kalibracija",
    "Ostalo",
];

#[derive(Debug, Clone, Serialize)]
pub struct ProtocolTemplate {
    pub id: &'static str,
    pub name: &'static str,
    /// Session kind it fits: "procjena", "terapija", "kontrola".
    pub kind: &'static str,
    pub description: &'static str,
    pub tasks: &'static [&'static str],
}

/// Built-in session protocols (starting points; the clinician edits the list).
pub const PROTOCOLS: &[ProtocolTemplate] = &[
    ProtocolTemplate {
        id: "glas-procjena",
        name: "Procjena glasa (akustička i aerodinamička)",
        kind: "procjena",
        description: "Produženi vokal /a/ tri puta, /s/ i /z/ (omjer s/z), glisando (F0 raspon), najtiša fonacija (za DSI), čitanje i spontani govor. Uz perceptivnu ocjenu GRBAS/CAPE-V i VHI-10.",
        tasks: &[
            "Produženi vokal /a/",
            "Produženi vokal /a/",
            "Produženi vokal /a/",
            "Produženi /s/",
            "Produženi /z/",
            "Glasovni raspon / glisando",
            "Najtiša fonacija /a/",
            "Najglasnija fonacija /a/",
            "Čitanje standardnog teksta",
            "Spontani govor",
        ],
    },
    ProtocolTemplate {
        id: "glas-kontrola",
        name: "Kontrola glasa (kratka)",
        kind: "kontrola",
        description: "Produženi vokal /a/, čitanje standardnog teksta i spontani govor za usporedbu s prethodnim mjerenjima.",
        tasks: &["Produženi vokal /a/", "Čitanje standardnog teksta", "Spontani govor"],
    },
    ProtocolTemplate {
        id: "tecnost-procjena",
        name: "Procjena tečnosti govora",
        kind: "procjena",
        description: "Čitanje, spontani govor, opis slike, automatizirani govor i ponavljanje rečenica; netečnosti se označe u editoru (%SS, trajanje).",
        tasks: &["Čitanje standardnog teksta", "Spontani govor", "Opis slike", "Brojanje / automatizirani govor", "Ponavljanje rečenica"],
    },
    ProtocolTemplate {
        id: "tecnost-terapija",
        name: "Terapija tečnosti uz DigiLingua FILTER",
        kind: "terapija",
        description: "Spontani govor bez filtra, zatim čitanje i govor s DAF/FAF filtrom — usporedba tečnosti sa i bez slušne povratne veze.",
        tasks: &["Spontani govor", "Čitanje standardnog teksta", "Spontani govor"],
    },
    ProtocolTemplate {
        id: "motorika",
        name: "Motorika govora / dizartrija",
        kind: "procjena",
        description: "Produženi vokali, dijadohokineza (pa-ta-ka), glisando, čitanje i spontani govor.",
        tasks: &["Produženi vokal /a/", "Produženi vokal /i/", "Produženi vokal /u/", "Dijadohokineza (pa-ta-ka)", "Glasovni raspon / glisando", "Čitanje standardnog teksta", "Spontani govor"],
    },
];

/// Reference values used in the profile (adults; indicative).
pub const MPT_MIN_S: f32 = 15.0;
pub const SZ_MAX: f32 = 1.4;
/// DSI of a normal voice is about +5; lower = more dysphonic.
pub const DSI_NORMAL: f32 = 5.0;

/// Clinical voice profile from a set of task recordings.
#[derive(Debug, Clone, Default, Serialize)]
pub struct VoiceProfile {
    /// Maximum phonation time: longest uninterrupted voicing of a sustained vowel, s.
    pub mpt_s: Option<f32>,
    /// Longest /s/ and /z/ (duration of the recording), s, and their ratio.
    pub s_s: Option<f32>,
    pub z_s: Option<f32>,
    pub sz_ratio: Option<f32>,
    /// Highest F0 of the glissando / voice range task, Hz.
    pub f0_high_hz: Option<f32>,
    /// Lowest F0 of the glissando / voice range task, Hz.
    pub f0_low_hz: Option<f32>,
    /// Intensity of the softest phonation, dB SPL (needs microphone calibration).
    pub i_low_db_spl: Option<f32>,
    /// Jitter (local, %) of the latest sustained /a/.
    pub jitter_pct: Option<f32>,
    /// Dysphonia Severity Index (Wuyts et al. 2000).
    pub dsi: Option<f32>,
    /// What was used or why something is missing (Croatian / English pairs).
    pub notes: Vec<[String; 2]>,
}

fn is_vowel(task: &str) -> bool {
    task.starts_with("Produženi vokal")
}

/// Compute the profile; `items` = recordings with their whole-file analysis,
/// `calibration_db` = dB SPL offset of the microphone (Praat dB + offset).
pub fn voice_profile(items: &[(&Recording, &VoiceReport)], calibration_db: Option<f32>) -> VoiceProfile {
    let mut p = VoiceProfile::default();
    let note = |p: &mut VoiceProfile, hr: String, en: String| p.notes.push([hr, en]);
    let by = |pred: &dyn Fn(&str) -> bool| items.iter().filter(|(r, _)| pred(&r.task)).collect::<Vec<_>>();

    let vowels = by(&is_vowel);
    p.mpt_s = vowels.iter().map(|(_, a)| a.max_voiced_s).filter(|v| *v > 0.0).reduce(f32::max);
    if p.mpt_s.is_none() {
        note(&mut p, "MPT: nema snimke produženog vokala.".into(), "MPT: no sustained vowel recording.".into());
    }
    p.s_s = by(&|t| t == "Produženi /s/").iter().map(|(r, _)| r.duration).reduce(f32::max);
    p.z_s = by(&|t| t == "Produženi /z/").iter().map(|(r, _)| r.duration).reduce(f32::max);
    if let (Some(s), Some(z)) = (p.s_s, p.z_s) {
        if z > 0.0 {
            p.sz_ratio = Some(s / z);
        }
        note(&mut p, "Omjer s/z iz trajanja snimki /s/ i /z/ (snimku započnite i završite uz glas).".into(), "s/z ratio from the durations of the /s/ and /z/ recordings (start and stop the recording with the sound).".into());
    }
    let range = by(&|t| t == "Glasovni raspon / glisando");
    p.f0_high_hz = range.iter().filter_map(|(_, a)| a.f0_max).reduce(f32::max);
    p.f0_low_hz = range.iter().filter_map(|(_, a)| a.f0_min).reduce(f32::min);
    match calibration_db {
        Some(cal) => {
            p.i_low_db_spl = by(&|t| t == "Najtiša fonacija /a/").iter().filter_map(|(_, a)| a.intensity_mean_db).reduce(f32::min).map(|v| v + cal);
        }
        None => note(&mut p, "Intenzitet u dB SPL (i DSI) traži kalibraciju mikrofona (⚙ Postavke).".into(), "Intensity in dB SPL (and the DSI) needs microphone calibration (⚙ Settings).".into()),
    }
    // jitter of the most recent sustained /a/
    p.jitter_pct = items
        .iter()
        .filter(|(r, a)| r.task == "Produženi vokal /a/" && a.jitter_local.is_some())
        .max_by_key(|(r, _)| r.created)
        .and_then(|(_, a)| a.jitter_local);
    if let (Some(mpt), Some(f0h), Some(il), Some(j)) = (p.mpt_s, p.f0_high_hz, p.i_low_db_spl, p.jitter_pct) {
        p.dsi = Some(0.13 * mpt + 0.0053 * f0h - 0.26 * il - 1.18 * j + 12.4);
    } else {
        note(&mut p, "DSI traži: produženi vokal (MPT, jitter), glisando (F0 visoki), najtišu fonaciju i kalibriran mikrofon.".into(), "The DSI needs: a sustained vowel (MPT, jitter), a glissando (F0 high), the softest phonation and a calibrated microphone.".into());
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(task: &str, dur: f32, t: u64) -> Recording {
        Recording { id: format!("r{t}"), task: task.into(), duration: dur, created: t, sample_rate: 48000, source: "in".into(), ..Default::default() }
    }

    #[test]
    fn dsi_and_sz_from_tasks() {
        let a = rec("Produženi vokal /a/", 20.0, 1);
        let s = rec("Produženi /s/", 18.0, 2);
        let z = rec("Produženi /z/", 15.0, 3);
        let g = rec("Glasovni raspon / glisando", 6.0, 4);
        let soft = rec("Najtiša fonacija /a/", 4.0, 5);
        let va = VoiceReport { max_voiced_s: 18.0, jitter_local: Some(0.5), ..Default::default() };
        let vg = VoiceReport { f0_max: Some(800.0), f0_min: Some(110.0), ..Default::default() };
        let vs = VoiceReport { intensity_mean_db: Some(50.0), ..Default::default() };
        let none = VoiceReport::default();
        let items = [(&a, &va), (&s, &none), (&z, &none), (&g, &vg), (&soft, &vs)];
        let p = voice_profile(&items, Some(0.0));
        assert_eq!(p.mpt_s, Some(18.0));
        assert!((p.sz_ratio.unwrap() - 1.2).abs() < 1e-6);
        // 0.13·18 + 0.0053·800 − 0.26·50 − 1.18·0.5 + 12.4 = 5.39
        assert!((p.dsi.unwrap() - 5.39).abs() < 1e-3, "{:?}", p.dsi);
        let p = voice_profile(&items, None);
        assert!(p.dsi.is_none() && p.i_low_db_spl.is_none());
    }
}
