//! Where Firewood keeps its files.
//!
//! Config lives in `$XDG_CONFIG_HOME/pinepods-firewood` (`~/.config/…` by default) on
//! every platform, where settings have always been. Logs go to the platform's local
//! data directory, out of the way.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const APP_DIR: &str = "pinepods-firewood";

pub fn config_dir() -> io::Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))?
            .join(".config"),
    };
    let dir = base.join(APP_DIR);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn log_dir() -> io::Result<PathBuf> {
    let base = dirs::data_local_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no data directory"))?;
    let dir = base.join(APP_DIR).join("logs");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Writes `contents` so that only the current user can read it, replacing the file
/// atomically so a crash mid-write can't leave a truncated session behind.
pub fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    // `mode` only applies on creation; tighten a leftover temp file too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents)?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}
