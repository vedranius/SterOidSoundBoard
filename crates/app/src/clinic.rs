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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PatientIn {
    pub name: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub birth: String,
    #[serde(default)]
    pub notes: String,
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
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Clinic {
    #[serde(default)]
    pub patients: Vec<Patient>,
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub recordings: Vec<Recording>,
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
            });
        }
        assert_eq!(c.remove_patient("a"), vec!["ra".to_string()]);
        assert_eq!((c.patients.len(), c.sessions.len(), c.recordings.len()), (1, 1, 1));
    }
}
