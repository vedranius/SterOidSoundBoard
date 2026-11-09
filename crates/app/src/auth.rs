//! Login for the desktop app: users with a password or PIN (Argon2id hashes in
//! `users.json`), cookie sessions held in memory, a per-address brake on wrong
//! guesses, and the audit log (`audit.jsonl`: who changed what, when, from where).
//!
//! Until the first user is created the app behaves as before on this computer,
//! but other devices on the network only reach the audio/pedalboard part: patient
//! data, AI and MCP settings and backups need this computer or a login.
use anyhow::{anyhow, bail, Result};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const COOKIE: &str = "ssb_session";
/// A session ends after this long without requests …
const IDLE: Duration = Duration::from_secs(12 * 3600);
/// … and at the latest after this long.
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
/// Wrong guesses from one address before it has to wait.
const FREE_TRIES: u32 = 5;

#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    #[serde(default)]
    pub display: String,
    /// "admin" (users, audit log, backups, AI/MCP settings) or "clinician".
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    hash: String,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub disabled: bool,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
    /// The user without the hash, for the UI.
    pub fn public(&self) -> serde_json::Value {
        serde_json::json!({"name": self.name, "display": self.display, "role": self.role, "created": self.created, "disabled": self.disabled})
    }
}

#[derive(Default, Serialize, Deserialize)]
struct UsersFile {
    users: Vec<User>,
}

struct Session {
    user: String,
    created: Instant,
    last: Instant,
}

pub struct Auth {
    path: PathBuf,
    audit_path: PathBuf,
    users: Mutex<Vec<User>>,
    sessions: Mutex<HashMap<String, Session>>,
    /// Wrong guesses per address: (count, time of the last one).
    fails: Mutex<HashMap<IpAddr, (u32, Instant)>>,
    audit_lock: Mutex<()>,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::getrandom(&mut b).expect("system random source");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hash_secret(secret: &str) -> Result<String> {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| anyhow!("random: {e}"))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| anyhow!("{e}"))?;
    Ok(Argon2::default().hash_password(secret.as_bytes(), &salt).map_err(|e| anyhow!("{e}"))?.to_string())
}

fn verify(secret: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(secret.as_bytes(), &h).is_ok())
}

/// User names: 1–32 of a–z, 0–9, `.`, `_`, `-` (stored in lower case).
pub fn clean_name(n: &str) -> Result<String> {
    let n = n.trim().to_lowercase();
    if n.is_empty() || n.len() > 32 || !n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')) {
        bail!(tr!("Korisničko ime: 1–32 znaka (a–z, 0–9, . _ -).", "User name: 1–32 characters (a–z, 0–9, . _ -)."));
    }
    Ok(n)
}

fn check_secret(s: &str) -> Result<()> {
    if s.chars().count() < 4 || s.len() > 256 {
        bail!(tr!("Lozinka ili PIN mora imati barem 4 znaka.", "The password or PIN needs at least 4 characters."));
    }
    Ok(())
}

impl Auth {
    pub fn load(data: &std::path::Path) -> Auth {
        let path = data.join("users.json");
        let f: UsersFile = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Auth {
            path,
            audit_path: data.join("audit.jsonl"),
            users: Mutex::new(f.users),
            sessions: Mutex::new(HashMap::new()),
            fails: Mutex::new(HashMap::new()),
            audit_lock: Mutex::new(()),
        }
    }

    fn save(&self, users: &[User]) -> Result<()> {
        let s = serde_json::to_string_pretty(&serde_json::json!({ "users": users }))?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, s)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// Login is switched on as soon as one active user exists.
    pub fn enabled(&self) -> bool {
        self.users.lock().unwrap().iter().any(|u| !u.disabled)
    }

    pub fn list(&self) -> Vec<serde_json::Value> {
        self.users.lock().unwrap().iter().map(User::public).collect()
    }

    /// Creates a user. The first one is always an administrator.
    pub fn create(&self, name: &str, display: &str, role: &str, secret: &str) -> Result<User> {
        let name = clean_name(name)?;
        check_secret(secret)?;
        let hash = hash_secret(secret)?;
        let mut users = self.users.lock().unwrap();
        if users.iter().any(|u| u.name == name) {
            bail!(tr!("Korisnik s tim imenom već postoji.", "A user with that name already exists."));
        }
        let role = if users.iter().all(|u| u.disabled) || role == "admin" { "admin" } else { "clinician" };
        let display = display.trim().chars().take(80).collect::<String>();
        let u = User { display: if display.is_empty() { name.clone() } else { display }, name, role: role.into(), hash, created: now(), disabled: false };
        users.push(u.clone());
        self.save(&users)?;
        Ok(u)
    }

    /// Changes a user; `None` keeps a field. Refuses to leave no active administrator.
    pub fn update(&self, name: &str, display: Option<&str>, role: Option<&str>, secret: Option<&str>, disabled: Option<bool>) -> Result<User> {
        if let Some(s) = secret {
            check_secret(s)?;
        }
        let hash = secret.map(hash_secret).transpose()?;
        let mut users = self.users.lock().unwrap();
        let mut next = users.clone();
        let u = next.iter_mut().find(|u| u.name == name).ok_or_else(|| anyhow!(tr!("Nema tog korisnika.", "No such user.")))?;
        if let Some(d) = display {
            let d = d.trim().chars().take(80).collect::<String>();
            u.display = if d.is_empty() { u.name.clone() } else { d };
        }
        if let Some(r) = role {
            u.role = if r == "admin" { "admin" } else { "clinician" }.into();
        }
        if let Some(h) = hash {
            u.hash = h;
        }
        if let Some(d) = disabled {
            u.disabled = d;
        }
        let out = u.clone();
        if !next.iter().any(|u| u.is_admin() && !u.disabled) {
            bail!(tr!("Mora ostati barem jedan aktivni administrator.", "At least one active administrator must remain."));
        }
        self.save(&next)?;
        *users = next;
        drop(users);
        if secret.is_some() || out.disabled {
            self.end_sessions_of(name, None);
        }
        Ok(out)
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        let mut users = self.users.lock().unwrap();
        let next: Vec<User> = users.iter().filter(|u| u.name != name).cloned().collect();
        if next.len() == users.len() {
            bail!(tr!("Nema tog korisnika.", "No such user."));
        }
        if !next.iter().any(|u| u.is_admin() && !u.disabled) {
            bail!(tr!("Mora ostati barem jedan aktivni administrator.", "At least one active administrator must remain."));
        }
        self.save(&next)?;
        *users = next;
        drop(users);
        self.end_sessions_of(name, None);
        Ok(())
    }

    /// Ends the user's sessions, except `keep` (the one changing its own password).
    pub fn end_sessions_of(&self, name: &str, keep: Option<&str>) {
        self.sessions.lock().unwrap().retain(|t, s| s.user != name || Some(t.as_str()) == keep);
    }

    /// How long `ip` still has to wait after too many wrong guesses.
    pub fn wait_for(&self, ip: IpAddr) -> Option<Duration> {
        let f = self.fails.lock().unwrap();
        let (n, last) = *f.get(&ip)?;
        if n < FREE_TRIES {
            return None;
        }
        // 30 s after the 5th wrong guess, doubling up to 15 min
        let wait = Duration::from_secs((30u64 << (n - FREE_TRIES).min(5)).min(900));
        let gone = last.elapsed();
        (gone < wait).then(|| wait - gone)
    }

    /// Checks name + secret; on success returns the user and a new session token.
    /// Slow on purpose (Argon2id), so call it from a blocking thread.
    pub fn login(&self, name: &str, secret: &str, ip: IpAddr) -> Result<(User, String)> {
        if let Some(w) = self.wait_for(ip) {
            bail!(tr!("Previše pogrešnih pokušaja. Pokušajte ponovno za {} s.", "Too many wrong attempts. Try again in {} s.", w.as_secs() + 1));
        }
        let name = name.trim().to_lowercase();
        let user = self.users.lock().unwrap().iter().find(|u| u.name == name && !u.disabled).cloned();
        // same work whether or not the user exists
        static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let dummy = DUMMY.get_or_init(|| hash_secret("not a real secret").unwrap_or_default());
        let ok = verify(secret, user.as_ref().map(|u| u.hash.as_str()).unwrap_or(dummy)) && user.is_some();
        let mut fails = self.fails.lock().unwrap();
        if !ok {
            let e = fails.entry(ip).or_insert((0, Instant::now()));
            e.0 += 1;
            e.1 = Instant::now();
            bail!(tr!("Pogrešno korisničko ime ili lozinka.", "Wrong user name or password."));
        }
        fails.remove(&ip);
        drop(fails);
        let user = user.unwrap();
        let token = random_hex(32);
        let mut s = self.sessions.lock().unwrap();
        s.retain(|_, v| v.last.elapsed() < IDLE && v.created.elapsed() < MAX_AGE);
        s.insert(token.clone(), Session { user: user.name.clone(), created: Instant::now(), last: Instant::now() });
        Ok((user, token))
    }

    /// A new session without a password check (right after creating the first user).
    pub fn session_for(&self, name: &str) -> String {
        let token = random_hex(32);
        self.sessions.lock().unwrap().insert(token.clone(), Session { user: name.into(), created: Instant::now(), last: Instant::now() });
        token
    }

    /// The signed-in user of a session token (and keeps the session alive).
    pub fn check(&self, token: &str) -> Option<User> {
        let mut s = self.sessions.lock().unwrap();
        let sess = s.get_mut(token)?;
        if sess.last.elapsed() >= IDLE || sess.created.elapsed() >= MAX_AGE {
            s.remove(token);
            return None;
        }
        sess.last = Instant::now();
        let name = sess.user.clone();
        drop(s);
        self.users.lock().unwrap().iter().find(|u| u.name == name && !u.disabled).cloned()
    }

    pub fn logout(&self, token: &str) {
        self.sessions.lock().unwrap().remove(token);
    }

    // ------------------------------------------------------------ audit log

    /// Appends one line to `audit.jsonl`. Never carries patient names or other
    /// clinical content: only who, from where, what kind of change, and ids.
    pub fn audit(&self, user: &str, ip: &str, action: &str, target: &str) {
        let line = serde_json::json!({"at": now(), "user": user, "ip": ip, "action": action, "target": target}).to_string();
        let _g = self.audit_lock.lock().unwrap();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.audit_path) {
            let _ = writeln!(f, "{line}");
        }
    }

    /// The newest `n` entries, newest first.
    pub fn audit_tail(&self, n: usize) -> Vec<serde_json::Value> {
        let _g = self.audit_lock.lock().unwrap();
        let s = std::fs::read_to_string(&self.audit_path).unwrap_or_default();
        s.lines().rev().filter_map(|l| serde_json::from_str(l).ok()).take(n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ssb-auth-{}", random_hex(6)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn users_login_sessions() {
        let d = tmp();
        let a = Auth::load(&d);
        assert!(!a.enabled());
        let first = a.create("Ana", "Ana A.", "clinician", "1234").unwrap();
        assert_eq!(first.role, "admin", "the first user is an administrator");
        assert_eq!(first.name, "ana");
        assert!(a.enabled());
        assert!(a.create("ana", "", "clinician", "abcd").is_err(), "duplicate");
        assert!(a.create("bo b", "", "clinician", "abcd").is_err(), "bad name");
        assert!(a.create("bob", "", "clinician", "12").is_err(), "short secret");
        a.create("bob", "Bob", "clinician", "secret").unwrap();
        // the file never holds the secret, and reloads
        let raw = std::fs::read_to_string(d.join("users.json")).unwrap();
        assert!(!raw.contains("secret") && raw.contains("$argon2id$"));
        let a = Auth::load(&d);
        let ip: IpAddr = "192.168.1.5".parse().unwrap();
        let (u, tok) = a.login("BOB", "secret", ip).unwrap();
        assert_eq!(u.name, "bob");
        assert_eq!(a.check(&tok).unwrap().name, "bob");
        assert!(a.check("nope").is_none());
        // the last administrator stays
        assert!(a.update("ana", None, Some("clinician"), None, None).is_err());
        assert!(a.delete("ana").is_err());
        // a new password ends the user's sessions
        a.update("bob", None, None, Some("newsecret"), None).unwrap();
        assert!(a.check(&tok).is_none());
        assert!(a.login("bob", "secret", ip).is_err());
        a.login("bob", "newsecret", ip).unwrap();
        a.delete("bob").unwrap();
        assert!(a.login("bob", "newsecret", ip).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn brake_on_wrong_guesses() {
        let d = tmp();
        let a = Auth::load(&d);
        a.create("ana", "", "admin", "1234").unwrap();
        let ip: IpAddr = "10.0.0.9".parse().unwrap();
        for _ in 0..FREE_TRIES {
            assert!(a.login("ana", "0000", ip).is_err());
        }
        assert!(a.wait_for(ip).is_some());
        // even the right PIN waits now; other addresses do not
        assert!(a.login("ana", "1234", ip).is_err());
        assert!(a.login("ana", "1234", "10.0.0.10".parse().unwrap()).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn audit_log_newest_first() {
        let d = tmp();
        let a = Auth::load(&d);
        a.audit("ana", "127.0.0.1", "PUT", "/api/patients/p1");
        a.audit("bob", "10.0.0.2", "DELETE", "/api/notes/n1");
        let t = a.audit_tail(10);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0]["user"], "bob");
        assert_eq!(t[1]["target"], "/api/patients/p1");
        let _ = std::fs::remove_dir_all(&d);
    }
}
