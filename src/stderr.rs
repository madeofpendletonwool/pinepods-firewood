//! Keeps stray stderr output off the TUI.
//!
//! ALSA, CoreAudio and some decoders write diagnostics straight to file descriptor 2,
//! which would draw over the interface. While the TUI is up, fd 2 points at a file in
//! the log directory instead.

#[cfg(unix)]
pub struct StderrRedirect {
    saved: Option<std::os::fd::OwnedFd>,
}

#[cfg(unix)]
impl StderrRedirect {
    pub fn to_file(path: &std::path::Path) -> Self {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

        let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        else {
            return Self { saved: None };
        };
        // SAFETY: dup/dup2 on fd 2 and a file we hold open. dup returns a new descriptor
        // that we own, or -1, which is checked before wrapping it.
        let saved = unsafe {
            let fd = libc::dup(2);
            if fd < 0 || libc::dup2(file.as_raw_fd(), 2) < 0 {
                if fd >= 0 {
                    libc::close(fd);
                }
                return Self { saved: None };
            }
            OwnedFd::from_raw_fd(fd)
        };
        Self { saved: Some(saved) }
    }
}

#[cfg(unix)]
impl Drop for StderrRedirect {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        if let Some(saved) = self.saved.take() {
            // SAFETY: restores fd 2 from a descriptor we own; `saved` closes on drop.
            unsafe {
                libc::dup2(saved.as_raw_fd(), 2);
            }
        }
    }
}

#[cfg(not(unix))]
pub struct StderrRedirect;

#[cfg(not(unix))]
impl StderrRedirect {
    pub fn to_file(_path: &std::path::Path) -> Self {
        Self
    }
}
