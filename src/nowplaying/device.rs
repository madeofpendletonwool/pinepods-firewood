//! Who this player is to the server: a stable id per install and a display name.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DEVICE_ID_FILE: &str = "device_id";

/// The id is persisted so the server (and the user's other devices) see the same
/// player across restarts.
#[derive(Debug)]
pub struct DeviceId {
    id: String,
    /// `None` when there's nowhere to save it; the id then lasts for this run.
    path: Option<PathBuf>,
}

impl DeviceId {
    /// Reads the saved id, or makes and saves one.
    pub fn load_or_create() -> Self {
        match crate::paths::config_dir() {
            Ok(dir) => Self::load_or_create_at(dir.join(DEVICE_ID_FILE)),
            Err(err) => {
                tracing::warn!(
                    "no config directory for the device id, using a temporary one: {err}"
                );
                Self {
                    id: generate(),
                    path: None,
                }
            }
        }
    }

    pub fn load_or_create_at(path: PathBuf) -> Self {
        let saved = fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| is_valid(s));
        match saved {
            Some(id) => Self {
                id,
                path: Some(path),
            },
            None => {
                let mut device = Self {
                    id: String::new(),
                    path: Some(path),
                };
                device.rotate();
                device
            }
        }
    }

    pub fn as_str(&self) -> &str {
        &self.id
    }

    /// Takes a fresh id. Used when the server says another live connection holds
    /// ours (a copied config directory, two instances at once), so the two stop
    /// evicting each other.
    pub fn rotate(&mut self) {
        self.id = generate();
        if let Some(path) = &self.path
            && let Err(err) = save(path, &self.id)
        {
            tracing::warn!("couldn't save the device id (it will change on restart): {err}");
        }
    }
}

fn save(path: &Path, id: &str) -> io::Result<()> {
    crate::paths::write_private(path, format!("{id}\n").as_bytes())
}

/// 128 random bits as hex, like the web and mobile clients.
fn generate() -> String {
    let mut bytes = [0u8; 16];
    if let Err(err) = getrandom::fill(&mut bytes) {
        // Not secret, only needs to be unlikely to collide; fall back to the clock.
        tracing::warn!("no OS randomness for the device id: {err}");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        bytes = (nanos ^ (u128::from(std::process::id()) << 64)).to_le_bytes();
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Anything sane someone might have written by hand; it goes in a URL query.
fn is_valid(id: &str) -> bool {
    (8..=128).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `Firewood · <hostname>`, or just `Firewood` if the hostname is unknown.
pub fn default_name() -> String {
    match hostname() {
        Some(host) => format!("Firewood · {host}"),
        None => "Firewood".to_string(),
    }
}

/// The short hostname: `studio` rather than `studio.local`.
fn hostname() -> Option<String> {
    let full = raw_hostname()?;
    let short = full.split('.').next().unwrap_or(&full).trim();
    (!short.is_empty()).then(|| short.to_string())
}

#[cfg(unix)]
fn raw_hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its whole length, and gethostname writes at
    // most that many bytes.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..len].to_vec()).ok()
}

#[cfg(not(unix))]
fn raw_hostname() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_once_then_reuses_and_rotates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DEVICE_ID_FILE);

        let first = DeviceId::load_or_create_at(path.clone());
        assert_eq!(first.as_str().len(), 32);
        assert!(is_valid(first.as_str()));

        let again = DeviceId::load_or_create_at(path.clone());
        assert_eq!(again.as_str(), first.as_str());

        let mut rotated = again;
        rotated.rotate();
        assert_ne!(rotated.as_str(), first.as_str());
        let reloaded = DeviceId::load_or_create_at(path);
        assert_eq!(reloaded.as_str(), rotated.as_str());
    }

    #[test]
    fn replaces_a_garbled_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DEVICE_ID_FILE);
        fs::write(&path, "not a valid id!\n").unwrap();
        let device = DeviceId::load_or_create_at(path.clone());
        assert!(is_valid(device.as_str()));
        assert_eq!(fs::read_to_string(path).unwrap().trim(), device.as_str());
    }

    #[test]
    fn default_name_says_firewood() {
        assert!(default_name().starts_with("Firewood"));
    }
}
