//! Key bindings, with overrides from `keymap.toml` in the config directory.
//!
//! Bindings live in contexts. A page names the contexts it's in, most specific first,
//! and a key means the first action bound to it along that list; `global` comes last
//! everywhere. So `s` can send playback to a device on the Devices page and save an
//! episode in a list.
//!
//! An override replaces every key for that action in that context:
//!
//! ```toml
//! [global]
//! quit = ["q", "ctrl+q"]
//!
//! [queue]
//! move_up = ["K", "shift+up"]
//! ```

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

pub const FILE_NAME: &str = "keymap.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Context {
    Global,
    /// Any list: movement, filtering, opening.
    List,
    /// A list of episodes.
    Episode,
    Queue,
    Player,
    Devices,
    /// A podcast you follow, in a list or open.
    Podcast,
    /// Podcasts from the directories, in Search.
    Discover,
    /// An episode's notes, chapters and transcript.
    Details,
}

impl Context {
    pub const ALL: [Context; 9] = [
        Self::Global,
        Self::List,
        Self::Episode,
        Self::Podcast,
        Self::Details,
        Self::Queue,
        Self::Player,
        Self::Discover,
        Self::Devices,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Global => "Everywhere",
            Self::List => "Lists",
            Self::Episode => "Episodes",
            Self::Queue => "Queue",
            Self::Player => "Now playing",
            Self::Devices => "Devices",
            Self::Podcast => "Podcasts",
            Self::Discover => "Discover",
            Self::Details => "Notes, chapters and transcript",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    // Global
    Quit,
    Help,
    NextPage,
    PrevPage,
    Refresh,
    TogglePlay,
    SeekForward,
    SeekBackward,
    VolumeUp,
    VolumeDown,
    SpeedUp,
    SpeedDown,
    NowPlaying,
    PlayOn,
    // List
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Activate,
    Open,
    Back,
    Filter,
    CycleFilter,
    // Episode
    Queue,
    PlayNext,
    Save,
    Download,
    MarkPlayed,
    AddToCollection,
    // Podcast
    Unsubscribe,
    // Discover
    Subscribe,
    // Details
    CycleView,
    // Queue
    MoveUp,
    MoveDown,
    MoveToTop,
    MoveToBottom,
    Remove,
    ClearQueue,
    Shuffle,
    // Player
    Stop,
    ResetSpeed,
    TrimSilence,
    NextChapter,
    PrevChapter,
    // Devices
    PlayHere,
    SendHere,
    RemoteToggle,
    RemoteBack,
    RemoteForward,
    RemoteSlower,
    RemoteFaster,
}

impl Action {
    pub fn description(self) -> &'static str {
        match self {
            Self::Quit => "Quit",
            Self::Help => "Show keys",
            Self::NextPage => "Next page",
            Self::PrevPage => "Previous page",
            Self::Refresh => "Refresh",
            Self::TogglePlay => "Play/pause",
            Self::SeekForward => "Skip forward",
            Self::SeekBackward => "Skip back",
            Self::VolumeUp => "Volume up",
            Self::VolumeDown => "Volume down",
            Self::SpeedUp => "Faster",
            Self::SpeedDown => "Slower",
            Self::NowPlaying => "Now playing",
            Self::PlayOn => "Play on device",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::PageUp => "Page up",
            Self::PageDown => "Page down",
            Self::Top => "First",
            Self::Bottom => "Last",
            Self::Activate => "Play/open",
            Self::Open => "Details",
            Self::Back => "Back",
            Self::Filter => "Filter",
            Self::CycleFilter => "Next filter",
            Self::Queue => "Add to queue",
            Self::PlayNext => "Play next",
            Self::Save => "Save/unsave",
            Self::Download => "Download/delete on server",
            Self::MarkPlayed => "Mark played/unplayed",
            Self::AddToCollection => "Add to a collection",
            Self::Unsubscribe => "Unsubscribe",
            Self::Subscribe => "Subscribe",
            Self::CycleView => "Notes/chapters/transcript",
            Self::MoveUp => "Move up",
            Self::MoveDown => "Move down",
            Self::MoveToTop => "Move to top",
            Self::MoveToBottom => "Move to bottom",
            Self::Remove => "Remove",
            Self::ClearQueue => "Clear queue",
            Self::Shuffle => "Shuffle",
            Self::Stop => "Stop",
            Self::ResetSpeed => "Normal speed",
            Self::TrimSilence => "Trim silence on/off",
            Self::NextChapter => "Next chapter",
            Self::PrevChapter => "Previous chapter",
            Self::PlayHere => "Play here",
            Self::SendHere => "Send playback there",
            Self::RemoteToggle => "Play/pause there",
            Self::RemoteBack => "Skip back there",
            Self::RemoteForward => "Skip forward there",
            Self::RemoteSlower => "Slower there",
            Self::RemoteFaster => "Faster there",
        }
    }
}

const DEFAULTS: &[(Context, Action, &[&str])] = &[
    (Context::Global, Action::Quit, &["q"]),
    (Context::Global, Action::Help, &["?"]),
    (Context::Global, Action::NextPage, &["tab"]),
    (Context::Global, Action::PrevPage, &["shift+tab"]),
    (Context::Global, Action::Refresh, &["r", "ctrl+r"]),
    (Context::Global, Action::TogglePlay, &["space"]),
    (Context::Global, Action::SeekForward, &["right"]),
    (Context::Global, Action::SeekBackward, &["left"]),
    (Context::Global, Action::VolumeUp, &["+"]),
    (Context::Global, Action::VolumeDown, &["-"]),
    (Context::Global, Action::SpeedUp, &["]"]),
    (Context::Global, Action::SpeedDown, &["["]),
    (Context::Global, Action::NowPlaying, &["n"]),
    (Context::Global, Action::Back, &["esc", "backspace", "h"]),
    (Context::List, Action::Up, &["k", "up"]),
    (Context::List, Action::Down, &["j", "down"]),
    (Context::List, Action::PageUp, &["pageup", "ctrl+u"]),
    (Context::List, Action::PageDown, &["pagedown", "ctrl+d"]),
    (Context::List, Action::Top, &["g", "home"]),
    (Context::List, Action::Bottom, &["G", "end"]),
    (Context::List, Action::Activate, &["enter"]),
    (Context::List, Action::Open, &["l"]),
    (Context::List, Action::Filter, &["/"]),
    (Context::List, Action::CycleFilter, &["f"]),
    (Context::Episode, Action::Open, &["i", "l"]),
    (Context::Episode, Action::Queue, &["a"]),
    (Context::Episode, Action::PlayNext, &["A"]),
    (Context::Episode, Action::Save, &["s"]),
    (Context::Episode, Action::Download, &["d"]),
    (Context::Episode, Action::MarkPlayed, &["m"]),
    (Context::Episode, Action::PlayOn, &["o"]),
    (Context::Episode, Action::AddToCollection, &["c"]),
    (Context::Podcast, Action::Unsubscribe, &["U"]),
    (Context::Discover, Action::Subscribe, &["s"]),
    (Context::Details, Action::CycleView, &["v"]),
    (Context::Queue, Action::MoveUp, &["K", "shift+up"]),
    (Context::Queue, Action::MoveDown, &["J", "shift+down"]),
    (Context::Queue, Action::MoveToTop, &["t"]),
    (Context::Queue, Action::MoveToBottom, &["b"]),
    (Context::Queue, Action::Remove, &["x", "delete"]),
    (Context::Queue, Action::ClearQueue, &["C"]),
    (Context::Queue, Action::Shuffle, &["S"]),
    (Context::Player, Action::Stop, &["x"]),
    (Context::Player, Action::ResetSpeed, &["="]),
    (Context::Player, Action::TrimSilence, &["t"]),
    (Context::Player, Action::VolumeUp, &["+", "up"]),
    (Context::Player, Action::VolumeDown, &["-", "down"]),
    (Context::Player, Action::NextChapter, &["."]),
    (Context::Player, Action::PrevChapter, &[","]),
    (Context::Devices, Action::PlayHere, &["enter"]),
    (Context::Devices, Action::SendHere, &["s"]),
    (Context::Devices, Action::RemoteToggle, &["p"]),
    (Context::Devices, Action::RemoteBack, &["left", "h"]),
    (Context::Devices, Action::RemoteForward, &["right", "l"]),
    (Context::Devices, Action::RemoteSlower, &["["]),
    (Context::Devices, Action::RemoteFaster, &["]"]),
];

/// A key with its modifiers, comparable regardless of how the terminal reported
/// Shift on a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Key {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        let mut modifiers =
            modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        let code = match code {
            // "G" already says Shift; terminals differ on whether they also set it.
            KeyCode::Char(c) => {
                modifiers.remove(KeyModifiers::SHIFT);
                KeyCode::Char(c)
            }
            KeyCode::BackTab => {
                modifiers.insert(KeyModifiers::SHIFT);
                KeyCode::Tab
            }
            other => other,
        };
        Self { code, modifiers }
    }

    /// Parses `"ctrl+r"`, `"G"`, `"space"`, `"shift+up"`, `"f5"` and the like.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut modifiers = KeyModifiers::NONE;
        let mut parts: Vec<&str> = text.split('+').collect();
        // "+" alone, or ending in "++", is the plus key itself.
        let name = if text.ends_with('+') && (text == "+" || text.ends_with("++")) {
            parts.truncate(parts.len().saturating_sub(2));
            "+"
        } else {
            parts.pop().unwrap_or_default()
        };
        for modifier in parts {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
                "alt" | "option" | "meta" => modifiers |= KeyModifiers::ALT,
                "shift" => modifiers |= KeyModifiers::SHIFT,
                other => return Err(format!("unknown modifier {other:?} in {text:?}")),
            }
        }
        let mut chars = name.chars();
        let code = match (chars.next(), chars.next()) {
            (Some(c), None) => KeyCode::Char(c),
            _ => match name.to_ascii_lowercase().as_str() {
                "space" => KeyCode::Char(' '),
                "enter" | "return" => KeyCode::Enter,
                "esc" | "escape" => KeyCode::Esc,
                "tab" => KeyCode::Tab,
                "backtab" => KeyCode::BackTab,
                "backspace" => KeyCode::Backspace,
                "delete" | "del" => KeyCode::Delete,
                "insert" => KeyCode::Insert,
                "up" => KeyCode::Up,
                "down" => KeyCode::Down,
                "left" => KeyCode::Left,
                "right" => KeyCode::Right,
                "pageup" | "pgup" => KeyCode::PageUp,
                "pagedown" | "pgdn" => KeyCode::PageDown,
                "home" => KeyCode::Home,
                "end" => KeyCode::End,
                f if f.starts_with('f') => f[1..]
                    .parse()
                    .map(KeyCode::F)
                    .map_err(|_| format!("unknown key {name:?}"))?,
                _ => return Err(format!("unknown key {name:?}")),
            },
        };
        Ok(Self::new(code, modifiers))
    }
}

impl From<KeyEvent> for Key {
    fn from(event: KeyEvent) -> Self {
        Self::new(event.code, event.modifiers)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            f.write_str("Ctrl+")?;
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            f.write_str("Alt+")?;
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            f.write_str("Shift+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("Space"),
            KeyCode::Char(c) if self.modifiers.contains(KeyModifiers::CONTROL) => {
                write!(f, "{}", c.to_ascii_uppercase())
            }
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Enter => f.write_str("Enter"),
            KeyCode::Esc => f.write_str("Esc"),
            KeyCode::Tab => f.write_str("Tab"),
            KeyCode::Backspace => f.write_str("Bksp"),
            KeyCode::Delete => f.write_str("Del"),
            KeyCode::Up => f.write_str("↑"),
            KeyCode::Down => f.write_str("↓"),
            KeyCode::Left => f.write_str("←"),
            KeyCode::Right => f.write_str("→"),
            KeyCode::PageUp => f.write_str("PgUp"),
            KeyCode::PageDown => f.write_str("PgDn"),
            KeyCode::Home => f.write_str("Home"),
            KeyCode::End => f.write_str("End"),
            KeyCode::F(n) => write!(f, "F{n}"),
            other => write!(f, "{other:?}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Keymap {
    /// Per context, each action's keys in the order they were listed.
    bindings: HashMap<Context, Vec<(Action, Vec<Key>)>>,
}

impl Default for Keymap {
    fn default() -> Self {
        let mut bindings: HashMap<Context, Vec<(Action, Vec<Key>)>> = HashMap::new();
        for (context, action, keys) in DEFAULTS {
            let keys = keys
                .iter()
                .map(|k| Key::parse(k).expect("default bindings parse"))
                .collect();
            bindings.entry(*context).or_default().push((*action, keys));
        }
        Self { bindings }
    }
}

type Overrides = HashMap<Context, HashMap<Action, OneOrMany>>;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl Keymap {
    pub fn path() -> Option<PathBuf> {
        crate::paths::config_dir().ok().map(|d| d.join(FILE_NAME))
    }

    /// The defaults with the user's overrides applied. Problems in the file are
    /// returned alongside a usable keymap so they can be shown, not fatal.
    pub fn load(path: Option<&Path>) -> (Self, Vec<String>) {
        let mut keymap = Self::default();
        let Some(path) = path else {
            return (keymap, Vec::new());
        };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (keymap, Vec::new()),
            Err(e) => {
                return (
                    keymap,
                    vec![format!("couldn't read {}: {e}", path.display())],
                );
            }
        };
        let problems = keymap.apply(&text);
        (keymap, problems)
    }

    fn apply(&mut self, toml_text: &str) -> Vec<String> {
        let overrides: Overrides = match toml::from_str(toml_text) {
            Ok(o) => o,
            Err(e) => return vec![format!("{FILE_NAME}: {}", e.message())],
        };
        let mut problems = Vec::new();
        for (context, actions) in overrides {
            for (action, keys) in actions {
                let keys = match keys {
                    OneOrMany::One(k) => vec![k],
                    OneOrMany::Many(ks) => ks,
                };
                let parsed: Vec<Key> = keys
                    .iter()
                    .filter_map(|k| {
                        Key::parse(k)
                            .map_err(|e| problems.push(format!("{FILE_NAME}: {e}")))
                            .ok()
                    })
                    .collect();
                let list = self.bindings.entry(context).or_default();
                match list.iter_mut().find(|(a, _)| *a == action) {
                    Some((_, existing)) => *existing = parsed,
                    None => list.push((action, parsed)),
                }
            }
        }
        problems
    }

    /// What `key` does in `contexts` (most specific first; `global` is implied last).
    pub fn action(&self, key: Key, contexts: &[Context]) -> Option<Action> {
        contexts
            .iter()
            .chain(std::iter::once(&Context::Global))
            .filter_map(|c| self.bindings.get(c))
            .find_map(|list| {
                list.iter()
                    .find(|(_, keys)| keys.contains(&key))
                    .map(|(action, _)| *action)
            })
    }

    /// The first key for `action` in `contexts`, for a hint like "a Add to queue".
    pub fn key_for(&self, action: Action, contexts: &[Context]) -> Option<Key> {
        contexts
            .iter()
            .chain(std::iter::once(&Context::Global))
            .filter_map(|c| self.bindings.get(c))
            .find_map(|list| {
                list.iter()
                    .find(|(a, _)| *a == action)
                    .and_then(|(_, keys)| keys.first().copied())
            })
    }

    /// Every binding in `context`, for the help overlay.
    pub fn bindings(&self, context: Context) -> &[(Action, Vec<Key>)] {
        self.bindings
            .get(&context)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Key {
        Key::parse(text).unwrap()
    }

    #[test]
    fn parses_and_prints_keys() {
        assert_eq!(key("ctrl+r").to_string(), "Ctrl+R");
        assert_eq!(key("space").to_string(), "Space");
        assert_eq!(key("+").to_string(), "+");
        assert_eq!(key("ctrl++").to_string(), "Ctrl++");
        assert_eq!(key("shift+tab").to_string(), "Shift+Tab");
        assert_eq!(key("f5").to_string(), "F5");
        assert!(Key::parse("hyper+x").is_err());
        assert!(Key::parse("banana").is_err());
    }

    #[test]
    fn shift_on_a_letter_doesnt_matter() {
        let reported = Key::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(reported, key("G"));
        assert_eq!(
            Key::new(KeyCode::BackTab, KeyModifiers::SHIFT),
            key("shift+tab")
        );
    }

    #[test]
    fn specific_contexts_win_over_global() {
        let keymap = Keymap::default();
        let s = key("s");
        assert_eq!(
            keymap.action(s, &[Context::Devices]),
            Some(Action::SendHere)
        );
        assert_eq!(
            keymap.action(s, &[Context::Episode, Context::List]),
            Some(Action::Save)
        );
        assert_eq!(
            keymap.action(key("q"), &[Context::List]),
            Some(Action::Quit)
        );
        assert_eq!(keymap.action(key("s"), &[]), None);
    }

    #[test]
    fn overrides_replace_an_actions_keys() {
        let mut keymap = Keymap::default();
        let problems = keymap.apply(
            r#"
            [global]
            quit = "ctrl+q"
            [queue]
            move_up = ["U", "nonsense+u"]
            "#,
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(keymap.action(key("q"), &[]), None);
        assert_eq!(keymap.action(key("ctrl+q"), &[]), Some(Action::Quit));
        assert_eq!(
            keymap.action(key("U"), &[Context::Queue]),
            Some(Action::MoveUp)
        );
        assert_eq!(keymap.action(key("K"), &[Context::Queue]), None);
    }

    #[test]
    fn a_broken_file_keeps_the_defaults() {
        let mut keymap = Keymap::default();
        let problems = keymap.apply("[global\nquit = 1");
        assert_eq!(problems.len(), 1);
        assert_eq!(keymap.action(key("q"), &[]), Some(Action::Quit));
    }

    #[test]
    fn defaults_have_no_clashes_within_a_context() {
        let keymap = Keymap::default();
        for (context, list) in &keymap.bindings {
            let mut seen = HashMap::new();
            for (action, keys) in list {
                for k in keys {
                    if let Some(other) = seen.insert(*k, *action) {
                        panic!("{context:?}: {k} is bound to {other:?} and {action:?}");
                    }
                }
            }
        }
    }
}
