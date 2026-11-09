//! Backup and restore of the data directory as a .zip.
//!
//! Entries are stored (not compressed) with the data directory's own layout, the
//! same format the browser edition writes and reads, so a backup moves freely
//! between the two editions. Login data (`users.json`), device settings, the AI
//! key and the MCP token are never part of a backup.
use crate::state::App;
use anyhow::{anyhow, bail, Result};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

/// Folders of the data directory that go into a backup …
const DIRS: [&str; 4] = ["recordings", "presets", "clinic_presets", "boards"];
/// … and single files.
const FILES: [&str; 2] = ["clinic.json", "audit.jsonl"];
/// Largest single entry accepted on restore.
const MAX_ENTRY: u64 = 4 << 30;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<_> = rd.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for e in items {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out),
            Ok(t) if t.is_file() => out.push(p),
            _ => {} // symbolic links are skipped
        }
    }
}

/// Files of the data directory that belong in a backup, as (zip name, path).
fn backup_files(data: &Path) -> Vec<(String, PathBuf)> {
    let mut paths: Vec<PathBuf> = FILES.iter().map(|f| data.join(f)).filter(|p| p.is_file()).collect();
    for d in DIRS {
        walk(&data.join(d), &mut paths);
    }
    paths
        .into_iter()
        .filter(|p| !p.extension().is_some_and(|e| e == "tmp"))
        .filter_map(|p| {
            let rel = p.strip_prefix(data).ok()?;
            let name = rel.components().map(|c| c.as_os_str().to_str()).collect::<Option<Vec<_>>>()?.join("/");
            Some((name, p))
        })
        .collect()
}

/// Writes a backup of `data` to `dest`; returns the number of files.
pub fn write_zip(data: &Path, dest: &Path) -> Result<usize> {
    let files = backup_files(data);
    let tmp = dest.with_extension("part");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(false);
    let mut n = 0;
    for (name, path) in &files {
        let Ok(mut f) = std::fs::File::open(path) else { continue };
        let big = f.metadata().map(|m| m.len() >= u32::MAX as u64).unwrap_or(false);
        z.start_file(name.as_str(), opts.large_file(big))?;
        std::io::copy(&mut f, &mut z)?;
        n += 1;
    }
    z.finish()?;
    std::fs::rename(&tmp, dest)?;
    Ok(n)
}

/// Zip entry name → safe relative path inside the data directory, or `None`
/// for anything a restore must not write (other files, `..`, absolute paths).
pub fn restore_target(name: &str) -> Option<PathBuf> {
    if name.ends_with('/') || name.contains('\\') {
        return None;
    }
    let p = Path::new(name);
    let mut parts = vec![];
    for c in p.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_str()?),
            _ => return None,
        }
    }
    let ok = match parts.as_slice() {
        [f] => *f == "clinic.json",
        [d, rest @ ..] if !rest.is_empty() => DIRS.contains(d) && rest.iter().all(|s| !s.starts_with('.')),
        _ => false,
    };
    ok.then(|| parts.iter().collect())
}

impl App {
    /// The backups folder (`<data>/backups`).
    pub fn backups_dir(&self) -> PathBuf {
        let d = self.paths.data.join("backups");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    /// Makes a backup file for download (replaces the previous download file).
    pub fn backup_for_download(&self) -> Result<(PathBuf, usize)> {
        self.flush_clinic()?;
        let dest = self.backups_dir().join("download.zip");
        let n = write_zip(&self.paths.data, &dest)?;
        Ok((dest, n))
    }

    /// Restores a backup .zip (desktop or browser edition). The current data is
    /// saved first to `backups/before-restore-<time>.zip`. Returns the number of
    /// files restored and the name of that safety copy.
    pub fn restore_zip(&self, bytes: &[u8]) -> Result<(usize, String)> {
        if self.inner.lock().unwrap().recording.is_some() {
            bail!(tr!("Najprije zaustavite snimanje.", "Stop the recording first."));
        }
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| anyhow!(tr!("To nije ispravna .zip datoteka.", "This is not a valid .zip file.")))?;
        let names: Vec<String> = (0..z.len()).filter_map(|i| z.by_index(i).ok().map(|f| f.name().to_string())).collect();
        if !names.iter().any(|n| n == "clinic.json") {
            bail!(tr!("To nije sigurnosna kopija SterOidSoundBoarda (nema clinic.json).", "This is not a SterOidSoundBoard backup (no clinic.json)."));
        }
        // the restored clinic.json must be readable before anything is replaced
        {
            let mut f = z.by_name("clinic.json")?;
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            serde_json::from_str::<crate::clinic::Clinic>(&s).map_err(|e| anyhow!(tr!("clinic.json iz kopije nije ispravan: {e}", "clinic.json in the backup is not valid: {e}")))?;
        }
        self.flush_clinic()?;
        let safety = format!("before-restore-{}.zip", crate::auth::now());
        write_zip(&self.paths.data, &self.backups_dir().join(&safety))?;
        let mut n = 0;
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            let Some(rel) = restore_target(f.name()) else { continue };
            if f.size() > MAX_ENTRY {
                continue;
            }
            let dest = self.paths.data.join(&rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tmp = dest.with_extension("restore-tmp");
            {
                let mut out = std::fs::File::create(&tmp)?;
                std::io::copy(&mut (&mut f).take(MAX_ENTRY), &mut out)?;
                out.flush()?;
            }
            std::fs::rename(&tmp, &dest)?;
            n += 1;
        }
        self.reload_clinic();
        Ok((n, safety))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_only_writes_data_files() {
        assert_eq!(restore_target("clinic.json"), Some(PathBuf::from("clinic.json")));
        assert_eq!(restore_target("recordings/r1.wav"), Some(PathBuf::from("recordings").join("r1.wav")));
        assert!(restore_target("presets/My board.json").is_some());
        for bad in ["../clinic.json", "/etc/passwd", "recordings/../users.json", "users.json", "ai.json", "mcp.json", "audio.json", "recordings/", "recordings/.hidden", "C:\\x", "boards"] {
            assert!(restore_target(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn zip_round_trip_is_stored_and_skips_secrets() {
        let d = std::env::temp_dir().join(format!("ssb-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("recordings")).unwrap();
        std::fs::write(d.join("clinic.json"), "{}").unwrap();
        std::fs::write(d.join("recordings/r1.wav"), b"RIFF....").unwrap();
        std::fs::write(d.join("users.json"), "hashes").unwrap();
        std::fs::write(d.join("ai.json"), "key").unwrap();
        let out = d.join("b.zip");
        assert_eq!(write_zip(&d, &out).unwrap(), 2);
        let bytes = std::fs::read(&out).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut names = vec![];
        for i in 0..z.len() {
            let f = z.by_index(i).unwrap();
            assert_eq!(f.compression(), zip::CompressionMethod::Stored, "the browser edition reads stored entries only");
            names.push(f.name().to_string());
        }
        names.sort();
        assert_eq!(names, ["clinic.json", "recordings/r1.wav"]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
