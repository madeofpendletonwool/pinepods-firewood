//! Puts the terminal back when a fatal signal kills the process.
//!
//! A panic hook never sees an illegal instruction or a segfault, so without this the
//! shell is left in raw mode on the alternate screen, printing mouse reports as text.
//! The signal still kills the process afterwards, so a core dump still points at the
//! real fault.

#[cfg(unix)]
mod imp {
    use std::mem::MaybeUninit;
    use std::sync::OnceLock;

    use libc::c_int;

    const SIGNALS: [c_int; 5] = [
        libc::SIGILL,
        libc::SIGSEGV,
        libc::SIGBUS,
        libc::SIGFPE,
        libc::SIGABRT,
    ];

    /// Mouse capture and bracketed paste off, off the alternate screen, cursor shown.
    const RESTORE: &[u8] =
        b"\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[?1049l\x1b[?25h";
    const NOTE: &[u8] = b"Firewood crashed on a fatal signal. It couldn't write a log entry, \
        but a core dump, if your system keeps them, has the details.\n";

    /// The terminal's settings from before raw mode.
    static TERMINAL: OnceLock<libc::termios> = OnceLock::new();

    /// Runs inside the signal, so it only makes async-signal-safe calls: no allocating,
    /// locking or logging.
    extern "C" fn on_fatal_signal(signal: c_int) {
        // SAFETY: signal, tcsetattr, write and raise are async-signal-safe, and both
        // pointers and lengths come from live statics.
        unsafe {
            // Not SA_RESETHAND: systems may refuse to auto-reset SIGILL, and macOS does,
            // which would bring every raise below straight back here.
            libc::signal(signal, libc::SIG_DFL);
            if let Some(terminal) = TERMINAL.get() {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, terminal);
            }
            let _ = libc::write(libc::STDOUT_FILENO, RESTORE.as_ptr().cast(), RESTORE.len());
            let _ = libc::write(libc::STDOUT_FILENO, NOTE.as_ptr().cast(), NOTE.len());
            // This signal is blocked until the handler returns, and then the raised one
            // ends the process, including when the signal came from `kill` and no
            // instruction would fault again.
            libc::raise(signal);
        }
    }

    pub fn install() {
        // SAFETY: tcgetattr fills the struct, which is only read if it succeeds.
        // sigaction gets a zeroed, then fully set, struct whose handler has the
        // signature it expects without SA_SIGINFO.
        unsafe {
            let mut terminal = MaybeUninit::<libc::termios>::uninit();
            if libc::tcgetattr(libc::STDIN_FILENO, terminal.as_mut_ptr()) == 0 {
                let _ = TERMINAL.set(terminal.assume_init());
            }

            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_fatal_signal as *const () as libc::sighandler_t;
            // The alternate stack is what lets this run after a stack overflow.
            action.sa_flags = libc::SA_ONSTACK;
            libc::sigemptyset(&mut action.sa_mask);
            for signal in SIGNALS {
                if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                    tracing::warn!(signal, "couldn't install a fatal signal handler");
                }
            }
        }
    }
}

/// Call before the terminal enters raw mode, which is the state to return to.
#[cfg(unix)]
pub fn install() {
    imp::install();
}

#[cfg(not(unix))]
pub fn install() {}
