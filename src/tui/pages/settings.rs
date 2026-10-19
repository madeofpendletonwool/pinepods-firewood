//! Preferences kept on this machine, and where Firewood keeps its files.

use std::path::PathBuf;

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::{Ctx, Outcome, Page, Section};
use crate::settings::get_available_audio_devices;
use crate::theme::Theme;
use crate::tui::keymap::{self, Action, Context, Keymap};
use crate::tui::task::Pending;
use crate::tui::text::truncate;
use crate::tui::ui::{self, list::Click, list::ListView};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Theme,
    FollowServer,
    Volume,
    SkipInterval,
    Output,
    DeviceName,
    SidebarOrder,
    Keymap,
    Logs,
    Account,
    Version,
}

impl Item {
    const ALL: [Item; 11] = [
        Self::Theme,
        Self::FollowServer,
        Self::Volume,
        Self::SkipInterval,
        Self::Output,
        Self::DeviceName,
        Self::SidebarOrder,
        Self::Keymap,
        Self::Logs,
        Self::Account,
        Self::Version,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::FollowServer => "Use the web app's theme",
            Self::Volume => "Starting volume",
            Self::SkipInterval => "Skip by",
            Self::Output => "Audio output",
            Self::DeviceName => "Name on other devices",
            Self::SidebarOrder => "Sidebar order",
            Self::Keymap => "Key bindings",
            Self::Logs => "Logs",
            Self::Account => "Signed in as",
            Self::Version => "Version",
        }
    }

    fn help(self) -> &'static str {
        match self {
            Self::Theme => "Enter to choose. The same themes as the web app.",
            Self::FollowServer => {
                "Enter to toggle. When on, Firewood matches the web app at startup."
            }
            Self::Volume => "←/→ to change. Where volume starts each time Firewood opens.",
            Self::SkipInterval => "←/→ to change. How far the skip keys jump.",
            Self::Output => "Enter to choose. Applies the next time Firewood starts.",
            Self::DeviceName => "Enter to edit. Applies the next time Firewood starts.",
            Self::SidebarOrder => "Enter to rearrange. Digit keys follow this order.",
            Self::Keymap => "Enter to create the file with every binding, ready to edit.",
            Self::Logs => "Enter to read today's log.",
            Self::Account => "",
            Self::Version => "",
        }
    }
}

/// A popup the page is showing.
enum Popup {
    /// Picking from a list. Themes preview as the cursor moves.
    Choose {
        what: Item,
        options: Vec<(String, String)>,
        list: ListView,
        /// The value to go back to on Esc.
        original: String,
    },
    /// Output devices are being listed (it can take a moment).
    LoadingDevices(Pending<Vec<(String, String)>>),
    Edit {
        what: Item,
        text: String,
    },
    Order {
        sections: Vec<Section>,
        list: ListView,
    },
}

#[derive(Default)]
pub struct SettingsPage {
    list: ListView,
    popup: Option<Popup>,
}

fn keymap_path() -> Option<PathBuf> {
    Keymap::path()
}

/// `~/…` for paths under the home directory.
fn home_relative(path: &std::path::Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(PathBuf::from)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

fn latest_log_file() -> Option<PathBuf> {
    std::fs::read_dir(crate::paths::log_dir().ok()?)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("firewood"))
        .max_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok())
        .map(|entry| entry.path())
}

/// Every binding, commented out, for the user to uncomment and change.
fn keymap_template() -> String {
    let keymap = Keymap::default();
    let mut out = String::from(
        "# Firewood key bindings. Uncomment a line and change its keys to rebind an\n\
         # action; an action lists every key that should trigger it. Keys look like\n\
         # \"q\", \"G\", \"space\", \"enter\", \"ctrl+r\", \"shift+up\", \"f5\".\n",
    );
    for context in [
        Context::Global,
        Context::List,
        Context::Episode,
        Context::Queue,
        Context::Player,
        Context::Devices,
    ] {
        out.push_str(&format!("\n[{}]\n", context_name(context)));
        for (action, keys) in keymap.bindings(context) {
            let keys: Vec<String> = keys.iter().map(|k| format!("{:?}", key_spec(*k))).collect();
            out.push_str(&format!(
                "# {} = [{}]  # {}\n",
                action_name(*action),
                keys.join(", "),
                action.description()
            ));
        }
    }
    out
}

fn context_name(context: Context) -> String {
    toml_name(&context)
}

fn action_name(action: Action) -> String {
    toml_name(&action)
}

/// The snake_case name serde reads a unit variant by.
fn toml_name<T: std::fmt::Debug>(value: &T) -> String {
    let debug = format!("{value:?}");
    let mut out = String::new();
    for (i, c) in debug.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A key the way the keymap file spells it.
fn key_spec(key: keymap::Key) -> String {
    let shown = key.to_string();
    let mut parts: Vec<&str> = shown.split('+').collect();
    if shown.ends_with("++") || shown == "+" {
        parts.truncate(parts.len().saturating_sub(2));
        parts.push("+");
    }
    let name = parts.pop().unwrap_or_default();
    let name = match name {
        "↑" => "up",
        "↓" => "down",
        "←" => "left",
        "→" => "right",
        "Bksp" => "backspace",
        "Del" => "delete",
        "PgUp" => "pageup",
        "PgDn" => "pagedown",
        "Space" => "space",
        "Enter" => "enter",
        "Esc" => "esc",
        "Tab" => "tab",
        "Home" => "home",
        "End" => "end",
        other => other,
    };
    let control_letter = parts.contains(&"Ctrl") && name.len() == 1;
    let function_key = name.starts_with('F') && name.len() > 1;
    let name = if control_letter || function_key {
        name.to_lowercase()
    } else {
        name.to_string()
    };
    parts
        .iter()
        .map(|m| m.to_lowercase())
        .chain(std::iter::once(name))
        .collect::<Vec<_>>()
        .join("+")
}

impl SettingsPage {
    fn selected(&self) -> Item {
        self.list
            .selected_index()
            .map_or(Item::Theme, |i| Item::ALL[i])
    }

    fn value(&self, item: Item, cx: &Ctx) -> String {
        let s = cx.settings.get();
        match item {
            Item::Theme => cx.theme.name.clone(),
            Item::FollowServer => if s.theme.follow_server { "on" } else { "off" }.into(),
            Item::Volume => format!("{:.0}%", s.default_volume * 100.0),
            Item::SkipInterval => format!("{} seconds", s.skip_interval),
            Item::Output => s
                .audio
                .selected_device
                .clone()
                .unwrap_or_else(|| "System default".into()),
            Item::DeviceName => s
                .remote
                .device_name
                .clone()
                .unwrap_or_else(|| cx.remote.state().device_name),
            Item::SidebarOrder => Section::ordered(&s.ui.tab_order)
                .iter()
                .map(|s| s.title())
                .collect::<Vec<_>>()
                .join(" · "),
            Item::Keymap => keymap_path()
                .map(|p| {
                    let exists = if p.exists() { "" } else { " (not created)" };
                    format!("{}{exists}", home_relative(&p))
                })
                .unwrap_or_default(),
            Item::Logs => crate::paths::log_dir()
                .map(|p| home_relative(&p))
                .unwrap_or_default(),
            Item::Account => cx.account.to_string(),
            Item::Version => format!("Firewood {VERSION}"),
        }
    }

    fn save(cx: &mut Ctx, change: impl FnOnce(&mut crate::settings::AppSettings)) {
        if let Err(e) = cx.settings.update(change) {
            tracing::warn!("couldn't save settings: {e:#}");
            cx.bus.error(format!("Couldn't save settings: {e}"));
        }
    }

    fn adjust(&mut self, cx: &mut Ctx, step: i32) {
        match self.selected() {
            Item::Volume => Self::save(cx, |s| {
                s.default_volume = (s.default_volume + step as f32 * 0.05).clamp(0.0, 1.0);
            }),
            Item::SkipInterval => Self::save(cx, |s| {
                s.skip_interval =
                    (s.skip_interval as i64 + i64::from(step) * 5).clamp(5, 300) as u64;
            }),
            Item::FollowServer => {
                Self::save(cx, |s| s.theme.follow_server = !s.theme.follow_server)
            }
            _ => {}
        }
    }

    fn activate(&mut self, cx: &mut Ctx) -> Outcome {
        let item = self.selected();
        match item {
            Item::Theme => {
                let options: Vec<(String, String)> = Theme::names()
                    .map(|n| (n.to_string(), n.to_string()))
                    .collect();
                let mut list = ListView::default();
                list.sync(&options, |(_, label)| label.clone());
                if let Some(i) = options.iter().position(|(n, _)| *n == cx.theme.name) {
                    list.select_index(i);
                }
                self.popup = Some(Popup::Choose {
                    what: item,
                    options,
                    list,
                    original: cx.settings.theme_name().to_string(),
                });
            }
            Item::FollowServer => self.adjust(cx, 1),
            Item::Output => {
                self.popup = Some(Popup::LoadingDevices(Pending::spawn(async {
                    tokio::task::spawn_blocking(get_available_audio_devices)
                        .await
                        .unwrap_or_default()
                })));
            }
            Item::DeviceName => {
                self.popup = Some(Popup::Edit {
                    what: item,
                    text: cx.settings.device_name().unwrap_or_default(),
                });
            }
            Item::SidebarOrder => {
                let sections = Section::ordered(&cx.settings.get().ui.tab_order);
                let mut list = ListView::default();
                list.sync(&sections, |s| s.title().to_string());
                self.popup = Some(Popup::Order { sections, list });
            }
            Item::Keymap => match keymap_path() {
                Some(path) if path.exists() => {
                    cx.bus
                        .info(format!("Edit {} and restart Firewood", path.display()));
                }
                Some(path) => match std::fs::write(&path, keymap_template()) {
                    Ok(()) => cx.bus.success(format!(
                        "Created {}. Edit it and restart Firewood",
                        path.display()
                    )),
                    Err(e) => cx
                        .bus
                        .error(format!("Couldn't create {}: {e}", path.display())),
                },
                None => cx.bus.error("There's no config directory"),
            },
            Item::Logs => {
                let Some(path) = latest_log_file() else {
                    cx.bus.info("No log file yet");
                    return Outcome::Handled;
                };
                return match std::fs::read_to_string(&path) {
                    Ok(text) => Outcome::Push(Box::new(LogPage::new(path, &text))),
                    Err(e) => {
                        cx.bus.error(format!("Couldn't read the log: {e}"));
                        Outcome::Handled
                    }
                };
            }
            Item::Volume | Item::SkipInterval | Item::Account | Item::Version => {}
        }
        Outcome::Handled
    }

    /// Keys while a popup is open. Always handled.
    fn popup_key(&mut self, key: KeyEvent, cx: &mut Ctx) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        let action = cx.keymap.action(key.into(), &[Context::List]);
        match popup {
            Popup::LoadingDevices(_) => {
                if key.code == KeyCode::Esc {
                    self.popup = None;
                }
            }
            Popup::Choose {
                what,
                options,
                list,
                original,
            } => {
                let what = *what;
                if key.code == KeyCode::Esc {
                    if what == Item::Theme {
                        let original = original.clone();
                        cx.settings.get_mut().theme.theme_name = original;
                    }
                    self.popup = None;
                    return;
                }
                if key.code == KeyCode::Enter {
                    let chosen = list.selected_index().map(|i| options[i].0.clone());
                    self.popup = None;
                    let Some(chosen) = chosen else {
                        return;
                    };
                    match what {
                        Item::Theme => Self::save(cx, |s| {
                            s.theme.theme_name = chosen;
                            s.theme.follow_server = false;
                        }),
                        Item::Output => {
                            Self::save(cx, |s| {
                                s.audio.selected_device = (chosen != "default").then_some(chosen);
                            });
                            cx.bus
                                .info("The new output is used the next time Firewood starts");
                        }
                        _ => {}
                    }
                    return;
                }
                if let Some(action) = action {
                    list.navigate(action);
                }
                if what == Item::Theme
                    && let Some(i) = list.selected_index()
                {
                    // Preview without saving.
                    cx.settings.get_mut().theme.theme_name = options[i].0.clone();
                }
            }
            Popup::Edit { what, text } => match key.code {
                KeyCode::Esc => self.popup = None,
                KeyCode::Enter => {
                    let (what, value) = (*what, text.trim().to_string());
                    self.popup = None;
                    if what == Item::DeviceName {
                        Self::save(cx, |s| {
                            s.remote.device_name = (!value.is_empty()).then_some(value);
                        });
                        cx.bus
                            .info("The new name shows the next time Firewood starts");
                    }
                }
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    text.clear();
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && text.chars().count() < 60 =>
                {
                    text.push(c);
                }
                _ => {}
            },
            Popup::Order { sections, list } => {
                let shift = |list: &mut ListView, sections: &mut Vec<Section>, down: bool| {
                    let Some(i) = list.selected_index() else {
                        return;
                    };
                    let j = if down { i + 1 } else { i.wrapping_sub(1) };
                    if j < sections.len() {
                        sections.swap(i, j);
                        list.sync(sections, |s| s.title().to_string());
                        list.select_index(j);
                    }
                };
                let moved = cx
                    .keymap
                    .action(key.into(), &[Context::Queue, Context::List]);
                match (key.code, moved) {
                    (KeyCode::Esc, _) => self.popup = None,
                    (KeyCode::Enter, _) => {
                        let order: Vec<String> = sections
                            .iter()
                            .map(|s| s.setting_name().to_string())
                            .collect();
                        self.popup = None;
                        Self::save(cx, |s| s.ui.tab_order = order);
                    }
                    (_, Some(Action::MoveUp)) => shift(list, sections, false),
                    (_, Some(Action::MoveDown)) => shift(list, sections, true),
                    (_, Some(action)) => {
                        list.navigate(action);
                    }
                    _ => {}
                }
            }
        }
    }

    fn render_popup(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        let Some(popup) = &mut self.popup else {
            return;
        };
        match popup {
            Popup::LoadingDevices(_) => {
                let inner = ui::popup(frame, ui::centered(area, 40, 3), "Audio output", theme);
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        format!("{} Looking for devices…", ui::spinner()),
                        theme.muted(),
                    )),
                    inner,
                );
            }
            Popup::Choose {
                what,
                options,
                list,
                ..
            } => {
                let height = (options.len() as u16 + 3).min(area.height.saturating_sub(2));
                let width = options
                    .iter()
                    .map(|(_, l)| l.width() as u16)
                    .max()
                    .unwrap_or(20)
                    .clamp(30, area.width.saturating_sub(4));
                let title = match what {
                    Item::Theme => "Theme",
                    _ => "Audio output",
                };
                let inner = ui::popup(frame, ui::centered(area, width + 6, height), title, theme);
                let [rows, hint] =
                    Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
                list.render(frame, rows, theme, &[Constraint::Fill(1)], None, |i, _| {
                    vec![Cell::from(options[i].1.clone())]
                });
                frame.render_widget(
                    Paragraph::new(Span::styled("Enter choose · Esc cancel", theme.faint())),
                    hint,
                );
            }
            Popup::Edit { text, .. } => {
                let inner = ui::popup(
                    frame,
                    ui::centered(area, 50, 4),
                    "Name on other devices",
                    theme,
                );
                let [field, hint] =
                    Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);
                frame.render_widget(Paragraph::new(text.as_str()).style(theme.base()), field);
                frame.set_cursor_position((
                    (field.x + text.width() as u16).min(field.right().saturating_sub(1)),
                    field.y,
                ));
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        "Enter save · Esc cancel · empty for the default",
                        theme.faint(),
                    )),
                    hint,
                );
            }
            Popup::Order { sections, list } => {
                let height = sections.len() as u16 + 3;
                let inner = ui::popup(
                    frame,
                    ui::centered(area, 40, height),
                    "Sidebar order",
                    theme,
                );
                let [rows, hint] =
                    Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
                list.render(
                    frame,
                    rows,
                    theme,
                    &[Constraint::Length(3), Constraint::Fill(1)],
                    None,
                    |i, _| {
                        let digit = match i {
                            0..=8 => (i + 1).to_string(),
                            9 => "0".into(),
                            _ => String::new(),
                        };
                        vec![
                            Cell::from(Span::styled(digit, theme.faint())),
                            Cell::from(format!("{} {}", sections[i].icon(), sections[i].title())),
                        ]
                    },
                );
                let keys = |a| {
                    cx.keymap
                        .key_for(a, &[Context::Queue])
                        .map(|k| k.to_string())
                        .unwrap_or_default()
                };
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        format!(
                            "{}/{} move · Enter save · Esc cancel",
                            keys(Action::MoveUp),
                            keys(Action::MoveDown)
                        ),
                        theme.faint(),
                    )),
                    hint,
                );
            }
        }
    }
}

impl Page for SettingsPage {
    fn title(&self) -> String {
        "Settings".into()
    }

    fn hints(&self) -> &'static [Action] {
        &[Action::Activate]
    }

    fn tick(&mut self, _cx: &Ctx) {
        if self.list.is_empty() {
            self.list.sync(&Item::ALL, |i| i.label().to_string());
        }
        if let Some(Popup::LoadingDevices(pending)) = &mut self.popup
            && let Some(options) = pending.poll()
        {
            let mut list = ListView::default();
            list.sync(&options, |(_, l)| l.clone());
            self.popup = Some(Popup::Choose {
                what: Item::Output,
                options,
                list,
                original: String::new(),
            });
        }
    }

    fn typing(&self) -> bool {
        self.popup.is_some()
    }

    fn key(&mut self, key: KeyEvent, cx: &mut Ctx) -> Option<Outcome> {
        if self.popup.is_some() {
            self.popup_key(key, cx);
            return Some(Outcome::Handled);
        }
        let step = match key.code {
            KeyCode::Left | KeyCode::Char('h') => -1,
            KeyCode::Right | KeyCode::Char('l') => 1,
            _ => return None,
        };
        if matches!(
            self.selected(),
            Item::Volume | Item::SkipInterval | Item::FollowServer
        ) {
            self.adjust(cx, step);
            return Some(Outcome::Handled);
        }
        None
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        if self.list.navigate(action) {
            return Outcome::Handled;
        }
        match action {
            Action::Activate | Action::Open => self.activate(cx),
            _ => Outcome::Ignored,
        }
    }

    fn mouse(&mut self, event: MouseEvent, cx: &mut Ctx) -> Outcome {
        if self.popup.is_some() {
            if let Some(Popup::Choose { list, .. } | Popup::Order { list, .. }) = &mut self.popup
                && list.mouse(event) == Click::Activated
            {
                self.popup_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), cx);
            }
            return Outcome::Handled;
        }
        if matches!(event.kind, MouseEventKind::Moved) {
            return Outcome::Ignored;
        }
        match self.list.mouse(event) {
            Click::Activated => self.activate(cx),
            Click::Moved => Outcome::Handled,
            Click::Missed => Outcome::Ignored,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        let [rows, _, help] = Layout::vertical([
            Constraint::Length(Item::ALL.len() as u16),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .areas(area);
        let values: Vec<String> = Item::ALL.iter().map(|i| self.value(*i, cx)).collect();
        self.list.render(
            frame,
            rows,
            theme,
            &[Constraint::Length(24), Constraint::Fill(1)],
            None,
            |i, w| {
                vec![
                    Cell::from(Span::styled(Item::ALL[i].label(), theme.muted())),
                    Cell::from(Span::styled(
                        truncate(&values[i], usize::from(w[1])),
                        theme.base().add_modifier(Modifier::BOLD),
                    )),
                ]
            },
        );
        frame.render_widget(
            Paragraph::new(Line::styled(self.selected().help(), theme.faint()))
                .wrap(Wrap { trim: true }),
            help,
        );
        self.render_popup(frame, area, cx);
    }
}

/// Today's log, newest at the bottom.
pub struct LogPage {
    path: PathBuf,
    lines: Vec<String>,
    /// Lines scrolled up from the bottom.
    from_bottom: usize,
    height: usize,
}

impl LogPage {
    fn new(path: PathBuf, text: &str) -> Self {
        Self {
            path,
            lines: text.lines().map(str::to_string).collect(),
            from_bottom: 0,
            height: 0,
        }
    }

    fn scroll_up(&mut self, by: isize) {
        let max = self.lines.len().saturating_sub(self.height) as isize;
        self.from_bottom = (self.from_bottom as isize + by).clamp(0, max) as usize;
    }
}

impl Page for LogPage {
    fn title(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Log".into())
    }

    fn hints(&self) -> &'static [Action] {
        &[Action::Up, Action::Down, Action::Refresh, Action::Back]
    }

    fn refresh(&mut self, cx: &Ctx) {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => self.lines = text.lines().map(str::to_string).collect(),
            Err(e) => cx.bus.error(format!("Couldn't read the log: {e}")),
        }
    }

    fn action(&mut self, action: Action, _cx: &mut Ctx) -> Outcome {
        let page = self.height.max(1) as isize;
        match action {
            Action::Up => self.scroll_up(1),
            Action::Down => self.scroll_up(-1),
            Action::PageUp => self.scroll_up(page),
            Action::PageDown => self.scroll_up(-page),
            Action::Top => self.scroll_up(isize::MAX / 2),
            Action::Bottom => self.from_bottom = 0,
            _ => return Outcome::Ignored,
        }
        Outcome::Handled
    }

    fn mouse(&mut self, event: MouseEvent, _cx: &mut Ctx) -> Outcome {
        match event.kind {
            MouseEventKind::ScrollUp => self.scroll_up(3),
            MouseEventKind::ScrollDown => self.scroll_up(-3),
            _ => return Outcome::Ignored,
        }
        Outcome::Handled
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        self.height = usize::from(area.height);
        let end = self.lines.len().saturating_sub(self.from_bottom);
        let start = end.saturating_sub(self.height);
        let lines: Vec<Line> = self.lines[start..end]
            .iter()
            .map(|l| {
                let style = if l.contains(" ERROR ") {
                    ratatui::style::Style::new().fg(theme.error)
                } else if l.contains(" WARN ") {
                    ratatui::style::Style::new().fg(theme.warning)
                } else {
                    theme.muted()
                };
                Line::styled(l.clone(), style)
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), area);
        let total = self.lines.len();
        ui::scrollbar(frame, area, total, start, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keymap_template_loads_back_to_the_defaults() {
        let template = keymap_template();
        // Uncomment every binding: it must parse and change nothing.
        let uncommented: String = template
            .lines()
            .map(|l| {
                let l = l
                    .strip_prefix("# ")
                    .filter(|l| l.contains(" = ["))
                    .unwrap_or(l);
                // Drop the trailing description comment.
                match l.find("]  #") {
                    Some(i) => format!("{}\n", &l[..=i]),
                    None => format!("{l}\n"),
                }
            })
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(keymap::FILE_NAME);
        std::fs::write(&path, uncommented).unwrap();
        let (loaded, problems) = Keymap::load(Some(&path));
        assert!(problems.is_empty(), "{problems:?}");
        let defaults = Keymap::default();
        for context in [
            Context::Global,
            Context::List,
            Context::Episode,
            Context::Queue,
        ] {
            assert_eq!(
                format!("{:?}", loaded.bindings(context)),
                format!("{:?}", defaults.bindings(context)),
                "{context:?}"
            );
        }
    }
}
