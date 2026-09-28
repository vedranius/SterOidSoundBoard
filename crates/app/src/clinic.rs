//! DigiLingua clinical records: patients, therapy sessions, recordings.
//! Stored as one JSON file next to the recordings in the data directory.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use steroid_engine::analysis::AnalysisSettings;
use steroid_engine::Board;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Unique, filesystem-safe id: prefix + unix ms + counter.
pub fn new_id(prefix: &str) -> String {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    format!("{prefix}{}-{}", now_ms(), SEQ.fetch_add(1, Relaxed) % 1000)
}

/// Patient record as entered by the clinician. Every field but `name` is
/// optional so older `clinic.json` files keep loading.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PatientIn {
    pub name: String,
    pub code: String,
    /// YYYY-MM-DD
    pub birth: String,
    /// "M", "Ž", "drugo" or empty
    pub sex: String,
    /// Diagnosis / ICD-10
    pub diagnosis: String,
    /// Current problem, reason for referral
    pub complaint: String,
    /// History of the problem: onset, duration, course, previous therapy
    pub history: String,
    /// Medical history: surgery, neurological, reflux, hormonal…
    pub medical: String,
    pub medications: String,
    /// Occupation and vocal load
    pub voice_use: String,
    /// "nepušač", "bivši pušač", "pušač" or empty
    pub smoking: String,
    pub hearing: String,
    pub language: String,
    pub goals: String,
    pub notes: String,
    /// Patient consented to processing by an external AI service.
    pub ai_consent: bool,
}

impl PatientIn {
    /// (field, max length) for validation.
    pub fn text_fields(&self) -> [(&str, usize); 15] {
        [
            (&self.name, 200),
            (&self.code, 100),
            (&self.birth, 40),
            (&self.sex, 20),
            (&self.diagnosis, 2000),
            (&self.complaint, 5000),
            (&self.history, 20_000),
            (&self.medical, 20_000),
            (&self.medications, 5000),
            (&self.voice_use, 5000),
            (&self.smoking, 50),
            (&self.hearing, 2000),
            (&self.language, 100),
            (&self.goals, 5000),
            (&self.notes, 20_000),
        ]
    }
}

/// Whole years between a YYYY-MM-DD birth date and a unix-ms instant.
pub fn age_at(birth: &str, at_ms: u64) -> Option<u32> {
    let mut it = birth.trim().split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    let (cy, cm, cd) = civil_from_days((at_ms / 86_400_000) as i64);
    let mut age = cy - y;
    if (cm, cd) < (m, d) {
        age -= 1;
    }
    (0..=130).contains(&age).then_some(age as u32)
}

/// "YYYY-MM-DD" for a unix-ms instant (UTC).
pub fn date_str(ms: u64) -> String {
    let (y, m, d) = civil_from_days((ms / 86_400_000) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 → (year, month, day) (H. Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patient {
    pub id: String,
    #[serde(flatten)]
    pub data: PatientIn,
    pub created: u64,
    pub updated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub patient_id: String,
    pub start: u64,
    #[serde(default)]
    pub end: Option<u64>,
    #[serde(default)]
    pub notes: String,
    /// Board (all settings) at session start.
    #[serde(default)]
    pub board: Option<Board>,
    /// Name of the clinical preset loaded when the session started.
    #[serde(default)]
    pub preset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    pub id: String,
    #[serde(default)]
    pub patient_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub label: String,
    pub created: u64,
    pub duration: f32,
    pub sample_rate: u32,
    /// "in" = dry microphone, "out" = processed output.
    pub source: String,
    /// Speech task, e.g. "Produženi vokal /a/", "Čitanje teksta".
    #[serde(default)]
    pub task: String,
    /// Clinician's observations for this recording.
    #[serde(default)]
    pub notes: String,
    /// Time-aligned labels (disfluencies, events) — exported as a Praat TextGrid.
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    /// Syllable count entered by the clinician (overrides the automatic
    /// syllable-nuclei estimate for %SS).
    #[serde(default)]
    pub syllables: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub id: String,
    pub start: f32,
    pub end: f32,
    /// One of ANNOTATION_KINDS (ids).
    pub kind: String,
    #[serde(default)]
    pub text: String,
}

/// (id, Croatian label, stuttering-like disfluency?)
pub const ANNOTATION_KINDS: &[(&str, &str, bool)] = &[
    ("blok", "Blok", true),
    ("produljenje", "Produljenje glasa", true),
    ("ponavljanje_glasa", "Ponavljanje glasa", true),
    ("ponavljanje_sloga", "Ponavljanje sloga", true),
    ("ponavljanje_rijeci", "Ponavljanje jednosložne riječi", true),
    ("umetak", "Umetak / poštapalica", false),
    ("revizija", "Revizija / prekinuta riječ", false),
    ("tvrdi_pocetak", "Tvrdi početak fonacije", false),
    ("prekid_glasa", "Prekid / pucanje glasa", false),
    ("sapat", "Šapat / afonija", false),
    ("pratece", "Prateće ponašanje", false),
    ("ostalo", "Ostalo", false),
];

pub fn kind_label(kind: &str) -> &str {
    ANNOTATION_KINDS.iter().find(|k| k.0 == kind).map(|k| k.1).unwrap_or(kind)
}

/// Disfluency summary of a recording's annotations.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AnnotationSummary {
    pub total: usize,
    /// Stuttering-like disfluencies (blocks, prolongations, sound/syllable/word repetitions).
    pub sld: usize,
    pub per_minute: f32,
    /// % syllables stuttered (SLD / syllables × 100), when a syllable count is known.
    pub pct_ss: Option<f32>,
    pub syllables: Option<u32>,
    pub syllables_source: &'static str,
    /// Mean duration of the three longest SLD, s (SSI-4 duration component).
    pub longest3_mean_s: Option<f32>,
    pub by_kind: Vec<(String, usize)>,
}

pub fn summarize(rec: &Recording, auto_syllables: Option<usize>) -> AnnotationSummary {
    let mut s = AnnotationSummary { total: rec.annotations.len(), ..Default::default() };
    let is_sld = |k: &str| ANNOTATION_KINDS.iter().any(|x| x.0 == k && x.2);
    let mut durs: Vec<f32> = rec.annotations.iter().filter(|a| is_sld(&a.kind)).map(|a| a.end - a.start).collect();
    s.sld = durs.len();
    if rec.duration > 0.0 {
        s.per_minute = s.total as f32 / rec.duration * 60.0;
    }
    let (syl, src) = match (rec.syllables, auto_syllables) {
        (Some(n), _) if n > 0 => (Some(n), "rehabilitator"),
        (_, Some(n)) if n > 0 => (Some(n as u32), "automatska procjena (slogovne jezgre)"),
        _ => (None, ""),
    };
    s.syllables = syl;
    s.syllables_source = src;
    s.pct_ss = syl.map(|n| s.sld as f32 / n as f32 * 100.0);
    durs.sort_by(|a, b| b.total_cmp(a));
    if !durs.is_empty() {
        let k = durs.len().min(3);
        s.longest3_mean_s = Some(durs[..k].iter().sum::<f32>() / k as f32);
    }
    for (id, label, _) in ANNOTATION_KINDS {
        let n = rec.annotations.iter().filter(|a| a.kind == *id).count();
        if n > 0 {
            s.by_kind.push((label.to_string(), n));
        }
    }
    s
}

fn tg_escape(t: &str) -> String {
    t.replace('"', "\"\"")
}

/// Praat TextGrid (long text format). Overlapping labels go to extra tiers.
pub fn textgrid(rec: &Recording) -> String {
    use std::fmt::Write;
    let dur = rec.duration.max(rec.annotations.iter().fold(0.0, |m, a| m.max(a.end))) as f64;
    let mut anns: Vec<&Annotation> = rec.annotations.iter().filter(|a| a.end > a.start).collect();
    anns.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut tiers: Vec<Vec<&Annotation>> = vec![];
    for a in anns {
        match tiers.iter_mut().find(|t| t.last().is_none_or(|l| l.end <= a.start)) {
            Some(t) => t.push(a),
            None => tiers.push(vec![a]),
        }
    }
    if tiers.is_empty() {
        tiers.push(vec![]);
    }
    let mut s = String::new();
    let _ = write!(
        s,
        "File type = \"ooTextFile\"\nObject class = \"TextGrid\"\n\nxmin = 0 \nxmax = {dur} \ntiers? <exists> \nsize = {} \nitem []: \n",
        tiers.len()
    );
    for (ti, tier) in tiers.iter().enumerate() {
        // fill gaps with empty intervals
        let mut iv: Vec<(f64, f64, String)> = vec![];
        let mut t = 0.0f64;
        for a in tier {
            let (a0, a1) = (a.start as f64, (a.end as f64).min(dur));
            if a0 > t {
                iv.push((t, a0, String::new()));
            }
            let text = if a.text.is_empty() { kind_label(&a.kind).to_string() } else { format!("{}: {}", kind_label(&a.kind), a.text) };
            iv.push((a0.max(t), a1, text));
            t = a1;
        }
        if t < dur {
            iv.push((t, dur, String::new()));
        }
        let name = if ti == 0 { "disfluencije".to_string() } else { format!("disfluencije {}", ti + 1) };
        let _ = write!(
            s,
            "    item [{}]:\n        class = \"IntervalTier\" \n        name = \"{name}\" \n        xmin = 0 \n        xmax = {dur} \n        intervals: size = {} \n",
            ti + 1,
            iv.len()
        );
        for (k, (a, b, text)) in iv.iter().enumerate() {
            let _ = write!(
                s,
                "        intervals [{}]:\n            xmin = {a} \n            xmax = {b} \n            text = \"{}\" \n",
                k + 1,
                tg_escape(text)
            );
        }
    }
    s
}

/// Clinic-wide settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ClinicSettings {
    /// Shown in report headers.
    pub clinic_name: String,
    pub clinician: String,
    /// Default acoustic-analysis parameters.
    pub analysis: AnalysisSettings,
    /// dB SPL = Praat dB + offset (microphone calibration); None = uncalibrated.
    pub calibration_db: Option<f32>,
}

/// An AI opinion on a recording, kept with the patient's records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiReport {
    pub id: String,
    pub recording_id: String,
    #[serde(default)]
    pub patient_id: Option<String>,
    pub created: u64,
    pub provider: String,
    pub model: String,
    /// Analysed range in seconds.
    pub start: f32,
    pub end: f32,
    #[serde(default)]
    pub question: String,
    pub text: String,
    /// True if the model stopped at the output limit.
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Clinic {
    #[serde(default)]
    pub patients: Vec<Patient>,
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub recordings: Vec<Recording>,
    #[serde(default)]
    pub ai_reports: Vec<AiReport>,
    #[serde(default)]
    pub settings: ClinicSettings,
}

impl Clinic {
    pub fn load(path: &Path) -> Clinic {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn active_session(&self) -> Option<&Session> {
        self.sessions.iter().find(|s| s.end.is_none())
    }

    /// Remove a patient with all their sessions and recordings; returns the
    /// recording ids whose files must be deleted.
    pub fn remove_patient(&mut self, id: &str) -> Vec<String> {
        self.patients.retain(|p| p.id != id);
        self.sessions.retain(|s| s.patient_id != id);
        let gone = self.recordings.iter().filter(|r| r.patient_id.as_deref() == Some(id)).map(|r| r.id.clone()).collect();
        self.recordings.retain(|r| r.patient_id.as_deref() != Some(id));
        self.ai_reports.retain(|a| a.patient_id.as_deref() != Some(id));
        gone
    }
}

pub fn wav_path(dir: &Path, rec_id: &str) -> PathBuf {
    dir.join(format!("{rec_id}.wav"))
}

/// Preset names become file names: keep letters (incl. č ć ž š đ), digits, space, - _ .
pub fn safe_name(name: &str) -> Option<String> {
    let s: String = name.trim().chars().filter(|c| c.is_alphanumeric() || " -_.".contains(*c)).collect();
    let s = s.trim_matches('.').trim().to_string();
    if s.is_empty() || s.len() > 80 { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_names() {
        assert_eq!(safe_name("  Govor — fokus ").as_deref(), Some("Govor  fokus"));
        assert_eq!(safe_name("../../etc/passwd").as_deref(), Some("etcpasswd"));
        assert_eq!(safe_name("čćžšđ 1").as_deref(), Some("čćžšđ 1"));
        assert!(safe_name("..").is_none());
    }

    fn rec_with(anns: Vec<(f32, f32, &str)>) -> Recording {
        Recording {
            id: "r".into(),
            patient_id: None,
            session_id: None,
            label: String::new(),
            created: 0,
            duration: 30.0,
            sample_rate: 48000,
            source: "in".into(),
            task: String::new(),
            notes: String::new(),
            annotations: anns
                .into_iter()
                .enumerate()
                .map(|(i, (a, b, k))| Annotation { id: format!("a{i}"), start: a, end: b, kind: k.into(), text: String::new() })
                .collect(),
            syllables: None,
        }
    }

    #[test]
    fn stuttering_summary() {
        let r = rec_with(vec![(1.0, 2.5, "blok"), (4.0, 4.4, "ponavljanje_sloga"), (6.0, 6.3, "umetak"), (9.0, 10.0, "produljenje")]);
        let s = summarize(&r, Some(100));
        assert_eq!((s.total, s.sld), (4, 3));
        assert!((s.pct_ss.unwrap() - 3.0).abs() < 1e-6);
        assert!((s.longest3_mean_s.unwrap() - (1.5 + 1.0 + 0.4) / 3.0).abs() < 1e-6);
        assert!((s.per_minute - 8.0).abs() < 1e-6);
        let mut r2 = r.clone();
        r2.syllables = Some(150);
        assert!((summarize(&r2, Some(100)).pct_ss.unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn textgrid_splits_overlaps() {
        let r = rec_with(vec![(1.0, 3.0, "blok"), (2.0, 2.5, "pratece"), (5.0, 6.0, "umetak")]);
        let tg = textgrid(&r);
        assert!(tg.starts_with("File type = \"ooTextFile\""));
        assert!(tg.contains("size = 2"), "{tg}");
        assert!(tg.contains("text = \"Blok\""));
        assert!(tg.contains("name = \"disfluencije 2\""));
        // first tier: [0,1] "" [1,3] Blok [3,5] "" [5,6] Umetak [6,30] ""
        assert!(tg.contains("intervals: size = 5"));
    }

    #[test]
    fn ages() {
        let d = |y: i64, m: u32, dd: u32| {
            // unix ms for a date via days-from-civil inverse check
            let mut ms = 0u64;
            while civil_from_days((ms / 86_400_000) as i64) != (y, m as i64, dd as i64) {
                ms += 86_400_000;
            }
            ms
        };
        let at = d(2026, 9, 28);
        assert_eq!(age_at("1980-09-28", at), Some(46));
        assert_eq!(age_at("1980-09-29", at), Some(45));
        assert_eq!(age_at("2020-01-01", at), Some(6));
        assert_eq!(age_at("", at), None);
        assert_eq!(age_at("kriv", at), None);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn old_patient_json_still_loads() {
        let p: PatientIn = serde_json::from_str(r#"{"name":"A","code":"1","birth":"","notes":"x"}"#).unwrap();
        assert_eq!((p.name.as_str(), p.sex.as_str(), p.ai_consent), ("A", "", false));
    }

    #[test]
    fn remove_patient_cascades() {
        let mut c = Clinic::default();
        for id in ["a", "b"] {
            c.patients.push(Patient { id: id.into(), data: PatientIn { name: id.into(), ..Default::default() }, created: 0, updated: 0 });
            c.sessions.push(Session { id: format!("s{id}"), patient_id: id.into(), start: 0, end: None, notes: String::new(), board: None, preset: None });
            c.recordings.push(Recording {
                id: format!("r{id}"),
                patient_id: Some(id.into()),
                session_id: None,
                label: String::new(),
                created: 0,
                duration: 1.0,
                sample_rate: 48000,
                source: "in".into(),
                task: String::new(),
                notes: String::new(),
                annotations: vec![],
                syllables: None,
            });
        }
        assert_eq!(c.remove_patient("a"), vec!["ra".to_string()]);
        assert_eq!((c.patients.len(), c.sessions.len(), c.recordings.len()), (1, 1, 1));
    }
}
