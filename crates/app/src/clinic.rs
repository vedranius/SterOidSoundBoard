//! DigiLingua clinical records: patients, therapy sessions, recordings.
//! Stored as one JSON file next to the recordings in the data directory.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
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
            c.sessions.push(Session { id: format!("s{id}"), patient_id: id.into(), start: 0, end: None, notes: String::new(), board: None });
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
            });
        }
        assert_eq!(c.remove_patient("a"), vec!["ra".to_string()]);
        assert_eq!((c.patients.len(), c.sessions.len(), c.recordings.len()), (1, 1, 1));
    }
}
