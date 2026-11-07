#![cfg(unix)]

use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use pinepods_firewood::fatal;

const CHILD_SIGNAL: &str = "FIREWOOD_TEST_FATAL_SIGNAL";

/// The handler ends the process, so each signal gets a copy of this test to kill.
#[test]
fn restores_the_terminal_and_still_dies_of_the_signal() {
    if let Some(signal) = std::env::var_os(CHILD_SIGNAL) {
        let signal: libc::c_int = signal.to_str().unwrap().parse().unwrap();
        let no_core = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        fatal::install();
        // SAFETY: plain libc calls with a live struct, in a process about to die.
        unsafe {
            libc::setrlimit(libc::RLIMIT_CORE, &no_core);
            libc::raise(signal);
        }
        unreachable!("the signal should have ended the process");
    }

    for signal in [
        libc::SIGILL,
        libc::SIGSEGV,
        libc::SIGBUS,
        libc::SIGFPE,
        libc::SIGABRT,
    ] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "restores_the_terminal_and_still_dies_of_the_signal",
                "--nocapture",
            ])
            .env(CHILD_SIGNAL, signal.to_string())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.signal(), Some(signal), "{stdout}");
        for sequence in ["\x1b[?1000l", "\x1b[?2004l", "\x1b[?1049l", "\x1b[?25h"] {
            assert!(stdout.contains(sequence), "{signal}: {stdout:?}");
        }
        assert!(stdout.contains("Firewood crashed"), "{signal}: {stdout:?}");
    }
}
