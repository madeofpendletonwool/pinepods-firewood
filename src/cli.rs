//! Command-line arguments. There are few enough that a parser crate isn't worth it.

use std::ffi::OsString;

pub const USAGE: &str = "\
Usage: pinepods_firewood [--headless]

A terminal client for PinePods.

Options:
      --headless  Run without the TUI: just the player and remote control, so other
                  PinePods devices can play on this one. Logs go to stderr as well as
                  the log file.
  -V, --version   Print the version and exit
  -h, --help      Print this help and exit

Headless login comes from the session saved by the TUI, or from the environment:
  FIREWOOD_SERVER         The server's URL
  FIREWOOD_API_KEY        An API key made in the web app (Settings → API Keys)
  FIREWOOD_API_KEY_FILE   A file holding the key, instead of FIREWOOD_API_KEY

Other environment variables:
  FIREWOOD_LOG               Log filter, such as `debug` (default `info`)
  FIREWOOD_REMOTE_DISABLED   `1` turns off remote control
  FIREWOOD_MEDIA_KEYS        `0` turns off media keys
  FIREWOOD_NO_MOUSE          Set to leave the mouse to the terminal
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Tui,
    Headless,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(Mode),
    Version,
    Help,
}

/// Parses the arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut mode = Mode::Tui;
    for arg in args {
        match arg.to_str() {
            Some("--headless") => mode = Mode::Headless,
            Some("-V" | "--version") => return Ok(Command::Version),
            Some("-h" | "--help") => return Ok(Command::Help),
            _ => return Err(format!("unknown argument: {}", arg.to_string_lossy())),
        }
    }
    Ok(Command::Run(mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_runs_the_tui() {
        assert_eq!(parse_strs(&[]), Ok(Command::Run(Mode::Tui)));
    }

    #[test]
    fn headless() {
        assert_eq!(
            parse_strs(&["--headless"]),
            Ok(Command::Run(Mode::Headless))
        );
    }

    #[test]
    fn version_and_help_win_over_other_flags() {
        assert_eq!(parse_strs(&["--headless", "-V"]), Ok(Command::Version));
        assert_eq!(parse_strs(&["--version"]), Ok(Command::Version));
        assert_eq!(parse_strs(&["-h", "--headless"]), Ok(Command::Help));
    }

    #[test]
    fn unknown_argument_is_an_error() {
        assert!(parse_strs(&["--headles"]).is_err());
        assert!(parse_strs(&["--headless", "extra"]).is_err());
    }
}
