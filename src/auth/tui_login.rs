//! The login screen: server address, then a password or an API key, then a second
//! factor if the account has one. A server with no accounts yet asks for its first
//! administrator instead, and an account that hasn't been set up picks its time zone
//! and formats.
//!
//! Every step runs in the background (see [`Pending`]), so the screen keeps drawing
//! and shows a spinner while the server answers. Every printable key goes into the
//! fields, so the screen's own keys are Tab, arrows, Enter, Esc and function keys,
//! and only Ctrl+C quits.

use std::collections::HashMap;

use anyhow::Result;
use chrono_tz::TZ_VARIANTS;
use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::login_flow::{LoginFlow, LoginResult, MfaSession, NewAdmin, Restored};
use super::{AuthState, SessionInfo};
use crate::api::http::normalize_server_url;
use crate::settings::SettingsManager;
use crate::theme::{ColorDepth, DEFAULT_THEME, Theme};
use crate::tui::task::Pending;
use crate::tui::text::{truncate, wrap};
use crate::tui::ui;

const CARD_WIDTH: u16 = 62;
const MFA_DIGITS: usize = 6;
/// Time zones listed under the search box.
const TZ_ROWS: usize = 4;
/// What PinePods stores (three letters at most) and how each looks.
const DATE_FORMATS: [(&str, &str); 5] = [
    ("MDY", "MM-DD-YYYY"),
    ("DMY", "DD-MM-YYYY"),
    ("YMD", "YYYY-MM-DD"),
    ("USA", "MM/DD/YYYY"),
    ("EUR", "DD.MM.YYYY"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    /// Checking the saved session.
    Restoring,
    Server,
    SignIn,
    Mfa,
    FirstAdmin,
    Setup,
}

impl Screen {
    fn title(self) -> &'static str {
        match self {
            Self::Restoring => "Signing in",
            Self::Server => "Connect to PinePods",
            Self::SignIn => "Sign in",
            Self::Mfa => "Two-factor code",
            Self::FirstAdmin => "Create the admin account",
            Self::Setup => "Set up your account",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Field {
    Server,
    Username,
    Password,
    ApiKey,
    Code,
    AdminUsername,
    AdminPassword,
    AdminEmail,
    AdminName,
    Timezone,
    HourFormat,
    DateFormat,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Self::Server => "Server address",
            Self::Username | Self::AdminUsername => "Username",
            Self::Password | Self::AdminPassword => "Password",
            Self::ApiKey => "API key",
            Self::Code => "Code",
            Self::AdminEmail => "Email",
            Self::AdminName => "Full name",
            Self::Timezone => "Time zone",
            Self::HourFormat => "Clock",
            Self::DateFormat => "Dates",
        }
    }

    fn secret(self) -> bool {
        matches!(self, Self::Password | Self::ApiKey | Self::AdminPassword)
    }

    /// Picked with ←/→ rather than typed.
    fn is_choice(self) -> bool {
        matches!(self, Self::HourFormat | Self::DateFormat)
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::Server => "https://pods.example.com",
            Self::ApiKey => "Paste a key from the web app",
            Self::Timezone => "Type to search",
            _ => "",
        }
    }
}

/// How the sign-in screen asks for proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Password,
    ApiKey,
}

enum Request {
    Restore(Pending<Restored>),
    Step(Pending<LoginResult>),
    Mfa(Pending<(LoginResult, Option<MfaSession>)>),
}

pub struct LoginTui {
    flow: LoginFlow,
    theme: Theme,
    screen: Screen,
    method: Method,
    values: HashMap<Field, String>,
    focus: usize,
    reveal: bool,
    /// An error, shown under the field it's about.
    error: Option<(Field, String)>,
    /// Good news from the last step, shown at the bottom of the card.
    notice: Option<String>,
    request: Option<(Request, String)>,
    /// Carried from the password step to the code step.
    mfa: Option<MfaSession>,
    /// Logged in, waiting on first-run setup.
    setup: Option<Box<AuthState>>,
    timezones: Vec<&'static str>,
    /// Indexes into `timezones` that match what's typed.
    tz_matches: Vec<usize>,
    tz_selected: usize,
    hour_24: bool,
    date_format: usize,
}

impl LoginTui {
    /// Starts checking the saved session straight away; the screen shows a spinner
    /// until it's known whether there's anything to ask.
    pub fn new() -> Result<Self> {
        // The saved theme, if there is one from an earlier session; the server's
        // can't be fetched before signing in.
        let theme_name = SettingsManager::new()
            .map(|s| s.theme_name().to_string())
            .unwrap_or_else(|_| DEFAULT_THEME.to_string());
        let mut login = Self::blank(LoginFlow::new()?, &theme_name);
        let restore = Pending::spawn(login.flow.clone().restore_session());
        login.request = Some((
            Request::Restore(restore),
            "Checking the saved sign-in…".into(),
        ));
        Ok(login)
    }

    fn blank(flow: LoginFlow, theme_name: &str) -> Self {
        let timezones: Vec<&'static str> = TZ_VARIANTS.iter().map(|tz| tz.name()).collect();
        let mut login = Self {
            flow,
            theme: Theme::named(theme_name, ColorDepth::detect()),
            screen: Screen::Restoring,
            method: Method::Password,
            values: HashMap::new(),
            focus: 0,
            reveal: false,
            error: None,
            notice: None,
            request: None,
            mfa: None,
            setup: None,
            tz_matches: (0..timezones.len()).collect(),
            timezones,
            tz_selected: 0,
            hour_24: false,
            date_format: 0,
        };
        login.select_system_timezone();
        login
    }

    fn fields(&self) -> &'static [Field] {
        match (self.screen, self.method) {
            (Screen::Restoring, _) => &[],
            (Screen::Server, _) => &[Field::Server],
            (Screen::SignIn, Method::Password) => &[Field::Username, Field::Password],
            (Screen::SignIn, Method::ApiKey) => &[Field::ApiKey],
            (Screen::Mfa, _) => &[Field::Code],
            (Screen::FirstAdmin, _) => &[
                Field::AdminUsername,
                Field::AdminPassword,
                Field::AdminEmail,
                Field::AdminName,
            ],
            (Screen::Setup, _) => &[Field::Timezone, Field::HourFormat, Field::DateFormat],
        }
    }

    fn focused(&self) -> Option<Field> {
        self.fields().get(self.focus).copied()
    }

    fn value(&self, field: Field) -> &str {
        self.values.get(&field).map(String::as_str).unwrap_or("")
    }

    fn server(&self) -> String {
        normalize_server_url(self.value(Field::Server))
    }

    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.focus = 0;
        self.reveal = false;
        self.error = None;
    }

    /// Takes in a finished request. Returns the session once signed in.
    pub fn tick(&mut self) -> Option<SessionInfo> {
        let (request, _) = self.request.as_mut()?;
        let result = match request {
            Request::Restore(pending) => {
                let restored = pending.poll()?;
                self.request = None;
                return self.restored(restored);
            }
            Request::Step(pending) => pending.poll()?,
            Request::Mfa(pending) => {
                let (result, session) = pending.poll()?;
                self.mfa = session;
                result
            }
        };
        self.request = None;
        self.finished(result)
    }

    fn restored(&mut self, restored: Restored) -> Option<SessionInfo> {
        match restored {
            Restored::Session(session) => return Some(*session),
            Restored::None => self.go(Screen::Server),
            Restored::Rejected { server } => {
                self.values.insert(Field::Server, server);
                self.go(Screen::Server);
                self.error = Some((
                    Field::Server,
                    "The server no longer accepts the saved sign-in. Sign in again.".into(),
                ));
            }
        }
        None
    }

    fn finished(&mut self, result: LoginResult) -> Option<SessionInfo> {
        match result {
            LoginResult::Success(session) => {
                self.forget_secrets();
                return Some(*session);
            }
            LoginResult::ServerVerified => {
                self.go(Screen::SignIn);
                self.notice = Some(format!("Found PinePods at {}", self.server()));
            }
            LoginResult::FirstAdminRequired => {
                self.go(Screen::FirstAdmin);
                self.notice = Some("This server has no accounts yet".into());
            }
            LoginResult::MfaRequired(session) => {
                self.mfa = Some(session);
                self.values.remove(&Field::Code);
                self.go(Screen::Mfa);
            }
            LoginResult::FirstTimeSetup(auth) => {
                self.setup = Some(auth);
                self.forget_secrets();
                self.go(Screen::Setup);
                self.notice = Some("Signed in".into());
            }
            LoginResult::Error(message) => {
                let field = self.fields().last().copied().unwrap_or(Field::Server);
                self.focus = self.fields().len().saturating_sub(1);
                self.error = Some((field, sentence(&message)));
            }
        }
        None
    }

    /// Passwords and keys aren't needed once the server has given us a session.
    fn forget_secrets(&mut self) {
        for field in [Field::Password, Field::ApiKey, Field::AdminPassword] {
            self.values.remove(&field);
        }
    }

    pub fn paste(&mut self, text: &str) {
        if self.request.is_some() {
            return;
        }
        for c in text.chars().filter(|c| !c.is_control()) {
            self.type_char(c);
        }
    }

    pub fn key(&mut self, key: KeyEvent) {
        if self.request.is_some() || self.screen == Screen::Restoring {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let field = self.focused();
        match key.code {
            KeyCode::Tab => self.move_focus(1),
            KeyCode::BackTab => self.move_focus(-1),
            KeyCode::Up if field == Some(Field::Timezone) => self.step_timezone(-1),
            KeyCode::Down if field == Some(Field::Timezone) => self.step_timezone(1),
            KeyCode::Up => self.move_focus(-1),
            KeyCode::Down => self.move_focus(1),
            KeyCode::Left | KeyCode::Right if field.is_some_and(Field::is_choice) => {
                self.change_choice(key.code == KeyCode::Right);
            }
            KeyCode::Enter => self.enter(),
            KeyCode::Esc => self.back(),
            KeyCode::F(1) => self.reveal = !self.reveal,
            KeyCode::Char('r') if ctrl => self.reveal = !self.reveal,
            KeyCode::F(2) => self.switch_method(),
            KeyCode::Char('k') if ctrl => self.switch_method(),
            KeyCode::Backspace => {
                if let Some(value) = self.text_mut() {
                    value.pop();
                }
                self.edited();
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(value) = self.text_mut() {
                    value.clear();
                }
                self.edited();
            }
            KeyCode::Char('w') if ctrl => {
                if let Some(value) = self.text_mut() {
                    let kept = value.trim_end().rfind(' ').map_or(0, |i| i + 1);
                    value.truncate(kept);
                }
                self.edited();
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.type_char(c);
            }
            _ => {}
        }
    }

    /// The focused field's text, if it's one you type into.
    fn text_mut(&mut self) -> Option<&mut String> {
        let field = self.focused().filter(|f| !f.is_choice())?;
        Some(self.values.entry(field).or_default())
    }

    fn type_char(&mut self, c: char) {
        let field = self.focused();
        if field == Some(Field::Code)
            && (!c.is_ascii_digit() || self.value(Field::Code).len() >= MFA_DIGITS)
        {
            return;
        }
        if let Some(value) = self.text_mut() {
            value.push(c);
        }
        self.edited();
    }

    fn edited(&mut self) {
        if self.error.as_ref().map(|(f, _)| Some(*f)) == Some(self.focused()) {
            self.error = None;
        }
        self.notice = None;
        if self.focused() == Some(Field::Timezone) {
            self.filter_timezones();
        }
    }

    fn move_focus(&mut self, step: isize) {
        let len = self.fields().len() as isize;
        if len > 0 {
            self.focus = (self.focus as isize + step).rem_euclid(len) as usize;
        }
    }

    /// Enter moves on to the next empty field, and submits once there's none.
    fn enter(&mut self) {
        let next_empty = self.fields()[self.focus.min(self.fields().len())..]
            .iter()
            .skip(1)
            .position(|f| !f.is_choice() && self.value(*f).trim().is_empty());
        match next_empty {
            Some(offset) => self.focus += offset + 1,
            None => self.submit(),
        }
    }

    fn back(&mut self) {
        let to = match self.screen {
            Screen::SignIn | Screen::FirstAdmin => Screen::Server,
            Screen::Mfa => Screen::SignIn,
            // Signed in already; there's nothing to go back to.
            Screen::Setup | Screen::Server | Screen::Restoring => return,
        };
        self.notice = None;
        self.go(to);
    }

    fn switch_method(&mut self) {
        if self.screen != Screen::SignIn {
            return;
        }
        self.method = match self.method {
            Method::Password => Method::ApiKey,
            Method::ApiKey => Method::Password,
        };
        self.focus = 0;
        self.error = None;
        self.reveal = false;
    }

    /// The first empty field among `fields`, as an error under it.
    fn require(&mut self, fields: &[(Field, &str)]) -> bool {
        for (field, message) in fields {
            if self.value(*field).trim().is_empty() {
                self.focus = self.fields().iter().position(|f| f == field).unwrap_or(0);
                self.error = Some((*field, (*message).to_string()));
                return false;
            }
        }
        true
    }

    fn submit(&mut self) {
        self.error = None;
        self.notice = None;
        let flow = self.flow.clone();
        let server = self.server();
        let (request, busy) = match (self.screen, self.method) {
            (Screen::Restoring, _) => return,
            (Screen::Server, _) => {
                if !self.require(&[(Field::Server, "Enter the address of your PinePods server")]) {
                    return;
                }
                let busy = format!("Looking for PinePods at {server}…");
                (
                    Request::Step(Pending::spawn(flow.verify_server(server))),
                    busy,
                )
            }
            (Screen::SignIn, Method::Password) => {
                if !self.require(&[
                    (Field::Username, "Enter your username"),
                    (Field::Password, "Enter your password"),
                ]) {
                    return;
                }
                let username = self.value(Field::Username).trim().to_string();
                let password = self.value(Field::Password).to_string();
                (
                    Request::Step(Pending::spawn(flow.sign_in(server, username, password))),
                    "Signing in…".to_string(),
                )
            }
            (Screen::SignIn, Method::ApiKey) => {
                if !self.require(&[(Field::ApiKey, "Paste an API key")]) {
                    return;
                }
                let key = self.value(Field::ApiKey).to_string();
                (
                    Request::Step(Pending::spawn(flow.sign_in_with_key(server, key))),
                    "Checking the key…".to_string(),
                )
            }
            (Screen::Mfa, _) => {
                let code = self.value(Field::Code).to_string();
                if code.len() != MFA_DIGITS {
                    self.error = Some((Field::Code, format!("Enter all {MFA_DIGITS} digits")));
                    return;
                }
                let Some(session) = self.mfa.take() else {
                    self.go(Screen::SignIn);
                    self.error = Some((Field::Password, "Sign in again for a new code".into()));
                    return;
                };
                (
                    Request::Mfa(Pending::spawn(flow.verify_mfa(session, code))),
                    "Checking the code…".to_string(),
                )
            }
            (Screen::FirstAdmin, _) => {
                if !self.require(&[
                    (Field::AdminUsername, "Choose a username"),
                    (Field::AdminPassword, "Choose a password"),
                    (Field::AdminEmail, "Enter an email address"),
                    (Field::AdminName, "Enter your name"),
                ]) {
                    return;
                }
                if !self.value(Field::AdminEmail).contains('@') {
                    self.focus = 2;
                    self.error = Some((Field::AdminEmail, "That isn't an email address".into()));
                    return;
                }
                let admin = NewAdmin {
                    username: self.value(Field::AdminUsername).trim().to_string(),
                    password: self.value(Field::AdminPassword).to_string(),
                    email: self.value(Field::AdminEmail).trim().to_string(),
                    fullname: self.value(Field::AdminName).trim().to_string(),
                };
                (
                    Request::Step(Pending::spawn(flow.create_first_admin(server, admin))),
                    "Creating the account…".to_string(),
                )
            }
            (Screen::Setup, _) => {
                let Some(timezone) = self.selected_timezone() else {
                    self.focus = 0;
                    self.error = Some((Field::Timezone, "Pick a time zone from the list".into()));
                    return;
                };
                let Some(auth) = self.setup.clone() else {
                    return;
                };
                let hour = if self.hour_24 { 24 } else { 12 };
                let format = DATE_FORMATS[self.date_format].0.to_string();
                (
                    Request::Step(Pending::spawn(flow.setup_timezone(
                        *auth,
                        timezone.to_string(),
                        hour,
                        format,
                    ))),
                    "Saving…".to_string(),
                )
            }
        };
        self.request = Some((request, busy));
    }

    // Time zones

    fn select_system_timezone(&mut self) {
        let name = system_timezone().unwrap_or_else(|| "UTC".into());
        if let Some(i) = self.timezones.iter().position(|tz| *tz == name) {
            self.tz_selected = i;
        }
    }

    fn filter_timezones(&mut self) {
        let previous = self.selected_timezone();
        let words: Vec<String> = self
            .value(Field::Timezone)
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect();
        self.tz_matches = self
            .timezones
            .iter()
            .enumerate()
            .filter(|(_, tz)| {
                let name = tz.to_lowercase().replace('_', " ");
                words.iter().all(|w| name.contains(w.as_str()))
            })
            .map(|(i, _)| i)
            .collect();
        let keep =
            previous.and_then(|p| self.tz_matches.iter().position(|&i| self.timezones[i] == p));
        self.tz_selected = keep.map_or(0, |pos| self.tz_matches[pos]);
        if keep.is_none()
            && let Some(&first) = self.tz_matches.first()
        {
            self.tz_selected = first;
        }
    }

    fn selected_timezone(&self) -> Option<&'static str> {
        self.tz_matches
            .contains(&self.tz_selected)
            .then(|| self.timezones[self.tz_selected])
    }

    fn step_timezone(&mut self, step: isize) {
        let Some(pos) = self.tz_matches.iter().position(|&i| i == self.tz_selected) else {
            return;
        };
        let last = self.tz_matches.len() as isize - 1;
        self.tz_selected = self.tz_matches[(pos as isize + step).clamp(0, last) as usize];
    }

    fn change_choice(&mut self, forward: bool) {
        match self.focused() {
            Some(Field::HourFormat) => self.hour_24 = !self.hour_24,
            Some(Field::DateFormat) => {
                let len = DATE_FORMATS.len();
                self.date_format = if forward {
                    (self.date_format + 1) % len
                } else {
                    (self.date_format + len - 1) % len
                };
            }
            _ => {}
        }
    }

    // Drawing

    pub fn render(&mut self, frame: &mut Frame) {
        let theme = &self.theme;
        let area = frame.area();
        frame.render_widget(Block::new().style(theme.base()), area);
        let [body, footer] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
        self.render_footer(frame, footer);

        let width = CARD_WIDTH.min(body.width.saturating_sub(4));
        // Inside the border and a column of padding each side.
        let text_width = usize::from(width.saturating_sub(6));
        let (lines, cursor) = self.card_lines(text_width);
        let height = (lines.len() as u16 + 4).min(body.height.saturating_sub(2));
        let [logo, _, card] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(height),
        ])
        .areas(ui::centered(body, width, height + 2));

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("▲ ", Style::new().fg(theme.success)),
                Span::styled("Firewood", theme.heading()),
                Span::styled("  for PinePods", theme.faint()),
            ])),
            logo,
        );
        let inner = ui::popup(frame, card, self.screen.title(), theme);
        let inner = Rect {
            x: inner.x + 2,
            y: inner.y + 1,
            width: inner.width.saturating_sub(4),
            height: inner.height.saturating_sub(2),
        };
        // Keep the focused field in view on a short terminal.
        let scroll = cursor
            .map_or(0, |(row, _)| {
                (row + 3).saturating_sub(usize::from(inner.height))
            })
            .min(lines.len().saturating_sub(usize::from(inner.height)));
        frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), inner);
        if let Some((row, col)) = cursor
            && self.request.is_none()
            && row >= scroll
            && row - scroll < usize::from(inner.height)
        {
            let x = inner.x + (col as u16).min(inner.width.saturating_sub(1));
            frame.set_cursor_position((x, inner.y + (row - scroll) as u16));
        }
    }

    /// The card's lines, and where the cursor goes (line, column).
    fn card_lines(&self, width: usize) -> (Vec<Line<'static>>, Option<(usize, usize)>) {
        let theme = &self.theme;
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut cursor = None;
        for text in self.intro() {
            lines.extend(
                wrap(&text, width)
                    .into_iter()
                    .map(|l| Line::styled(l, theme.muted())),
            );
        }
        if !lines.is_empty() {
            lines.push(Line::default());
        }

        for (i, field) in self.fields().iter().copied().enumerate() {
            let focused = i == self.focus;
            let label_style = if focused {
                theme.accent().add_modifier(Modifier::BOLD)
            } else {
                theme.muted()
            };
            lines.push(Line::styled(field.label(), label_style));

            let (text, style) = self.shown_value(field, width);
            if focused && !field.is_choice() {
                let typed = self.value(field);
                let col = if typed.is_empty() { 0 } else { text.width() };
                cursor = Some((lines.len(), col));
            }
            lines.push(Line::styled(text, style));
            if field == Field::Timezone {
                lines.extend(self.timezone_rows(width));
            }
            lines.push(Line::styled("─".repeat(width), theme.border(focused)));

            match &self.error {
                Some((f, message)) if *f == field => lines.extend(
                    wrap(message, width)
                        .into_iter()
                        .map(|l| Line::styled(l, Style::new().fg(theme.error))),
                ),
                _ => lines.push(Line::styled(
                    if focused { self.field_hint(field) } else { "" },
                    theme.faint(),
                )),
            }
        }

        match (&self.request, &self.notice) {
            (Some((_, busy)), _) => lines.push(Line::styled(
                format!(
                    "{} {}",
                    ui::spinner(),
                    truncate(busy, width.saturating_sub(2))
                ),
                theme.accent(),
            )),
            (None, Some(notice)) => lines.push(Line::styled(
                format!("✓ {}", truncate(notice, width.saturating_sub(2))),
                Style::new().fg(theme.success),
            )),
            (None, None) => {}
        }
        if lines.last().is_some_and(|l| l.width() == 0) {
            lines.pop();
        }
        (lines, cursor)
    }

    fn intro(&self) -> Vec<String> {
        match (self.screen, self.method) {
            (Screen::Restoring, _) => Vec::new(),
            (Screen::Server, _) => {
                vec!["The address you open PinePods at in a browser.".into()]
            }
            (Screen::SignIn, Method::Password) => vec![format!(
                "Your PinePods account on {}. Sign in through OIDC? Press F2 to use an API key instead.",
                host(&self.server())
            )],
            (Screen::SignIn, Method::ApiKey) => vec![
                "Make a key in the web app under Settings → API Keys, then paste it here."
                    .into(),
            ],
            (Screen::Mfa, _) => {
                vec!["Enter the code from your authenticator app.".into()]
            }
            (Screen::FirstAdmin, _) => vec![
                "The first account on a new server is its administrator. You can add more people from the web app later."
                    .into(),
            ],
            (Screen::Setup, _) => vec![
                "How PinePods shows times and dates. You can change these in the web app."
                    .into(),
            ],
        }
    }

    /// A field's text as drawn: masked if secret, a placeholder if empty, the
    /// current pick for a choice.
    fn shown_value(&self, field: Field, width: usize) -> (String, Style) {
        let theme = &self.theme;
        let choice = |text: String| (format!("‹ {text} ›"), theme.base().bg(theme.surface));
        match field {
            Field::HourFormat => {
                return choice(if self.hour_24 { "24-hour" } else { "12-hour" }.into());
            }
            Field::DateFormat => {
                let (_, example) = DATE_FORMATS[self.date_format];
                return choice(example.into());
            }
            _ => {}
        }
        let value = self.value(field);
        if value.is_empty() {
            return (field.placeholder().to_string(), theme.faint());
        }
        let text = if field.secret() && !self.reveal {
            "•".repeat(value.chars().count())
        } else {
            value.to_string()
        };
        // Keep the end in view: that's where typing happens.
        let text = if text.width() > width.saturating_sub(1) {
            let tail: String = text.chars().rev().take(width.saturating_sub(2)).collect();
            format!("…{}", tail.chars().rev().collect::<String>())
        } else {
            text
        };
        (text, theme.base().bg(theme.surface))
    }

    fn timezone_rows(&self, width: usize) -> Vec<Line<'static>> {
        let theme = &self.theme;
        if self.tz_matches.is_empty() {
            return vec![Line::styled("  No time zone matches", theme.faint())];
        }
        let pos = self
            .tz_matches
            .iter()
            .position(|&i| i == self.tz_selected)
            .unwrap_or(0);
        let start = pos
            .saturating_sub(TZ_ROWS / 2)
            .min(self.tz_matches.len().saturating_sub(TZ_ROWS));
        self.tz_matches
            .iter()
            .skip(start)
            .take(TZ_ROWS)
            .map(|&i| {
                let name = truncate(
                    &self.timezones[i].replace('_', " "),
                    width.saturating_sub(2),
                );
                if i == self.tz_selected {
                    Line::styled(
                        format!("› {name}"),
                        theme.accent().add_modifier(Modifier::BOLD),
                    )
                } else {
                    Line::styled(format!("  {name}"), theme.muted())
                }
            })
            .collect()
    }

    fn field_hint(&self, field: Field) -> &'static str {
        match field {
            Field::Password | Field::AdminPassword if self.reveal => "F1 hides it",
            Field::Password | Field::AdminPassword => "F1 shows it as you type",
            Field::ApiKey => "Paste with your terminal's paste shortcut",
            Field::Timezone => "↑↓ pick from the list",
            Field::HourFormat | Field::DateFormat => "←→ change",
            _ => "",
        }
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let theme = &self.theme;
        let area = Rect {
            x: area.x + 2,
            width: area.width.saturating_sub(4),
            ..area
        };
        let mut hints: Vec<(&str, &str)> = Vec::new();
        if self.request.is_none() && self.screen != Screen::Restoring {
            hints.push(("Enter", "continue"));
            if self.fields().len() > 1 {
                hints.push(("Tab", "next field"));
            }
            match self.screen {
                Screen::SignIn if self.method == Method::Password => {
                    hints.push(("F2", "use an API key"));
                }
                Screen::SignIn => hints.push(("F2", "use a password")),
                _ => {}
            }
            if matches!(
                self.screen,
                Screen::SignIn | Screen::Mfa | Screen::FirstAdmin
            ) {
                hints.push(("Esc", "back"));
            }
        }
        let quit = [("Ctrl+C", "quit")];
        let spans = |pairs: &[(&str, &str)]| -> Vec<Span<'static>> {
            let mut spans = Vec::new();
            for (key, what) in pairs {
                if !spans.is_empty() {
                    spans.push(Span::raw("  "));
                }
                spans.push(Span::styled(key.to_string(), theme.key()));
                spans.push(Span::styled(format!(" {what}"), theme.muted()));
            }
            spans
        };
        let right = spans(&quit);
        let right_width: u16 = right.iter().map(|s| s.content.width() as u16).sum();
        let room = usize::from(area.width.saturating_sub(right_width + 2));
        while hints.len() > 1
            && spans(&hints)
                .iter()
                .map(|s| s.content.width())
                .sum::<usize>()
                > room
        {
            hints.pop();
        }
        let [left_area, right_area] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(area);
        frame.render_widget(Paragraph::new(Line::from(spans(&hints))), left_area);
        frame.render_widget(Paragraph::new(Line::from(right)), right_area);
    }
}

/// `pods.example.com` from `https://pods.example.com`.
fn host(server: &str) -> &str {
    server
        .trim_start_matches("https://")
        .trim_start_matches("http://")
}

/// Server and library errors start lower-case; the screen shows them as sentences.
fn sentence(message: &str) -> String {
    let mut chars = message.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The machine's IANA time zone: `$TZ`, then `/etc/timezone` (Debian), then where
/// `/etc/localtime` points (most Linux and macOS).
fn system_timezone() -> Option<String> {
    let known = |name: &str| TZ_VARIANTS.iter().any(|tz| tz.name() == name);
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim_start_matches(':');
        if known(tz) {
            return Some(tz.to_string());
        }
    }
    if let Ok(text) = std::fs::read_to_string("/etc/timezone")
        && known(text.trim())
    {
        return Some(text.trim().to_string());
    }
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let target = target.to_string_lossy();
    let name = target.split("zoneinfo/").nth(1)?;
    known(name).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEvent;

    /// Never [`LoginTui::new`]: that reads the real saved session.
    fn login(screen: Screen) -> LoginTui {
        let mut login = LoginTui::blank(LoginFlow::new().unwrap(), DEFAULT_THEME);
        login.go(screen);
        login
    }

    fn press(login: &mut LoginTui, code: KeyCode) {
        login.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn typed(login: &mut LoginTui, text: &str) {
        for c in text.chars() {
            press(login, KeyCode::Char(c));
        }
    }

    #[test]
    fn every_printable_key_is_typed() {
        let mut login = login(Screen::SignIn);
        typed(&mut login, "q?/ jk");
        assert_eq!(login.value(Field::Username), "q?/ jk");
    }

    #[test]
    fn enter_moves_to_the_next_empty_field_then_complains() {
        let mut login = login(Screen::SignIn);
        typed(&mut login, "alice");
        press(&mut login, KeyCode::Enter);
        assert_eq!(login.focused(), Some(Field::Password));
        assert!(login.request.is_none());
        press(&mut login, KeyCode::BackTab);
        login.values.remove(&Field::Username);
        press(&mut login, KeyCode::Tab);
        typed(&mut login, "secret");
        press(&mut login, KeyCode::Enter);
        assert_eq!(login.focused(), Some(Field::Username));
        assert!(matches!(login.error, Some((Field::Username, _))));
        assert!(login.request.is_none());
    }

    #[test]
    fn passwords_are_masked_until_revealed() {
        let mut login = login(Screen::SignIn);
        press(&mut login, KeyCode::Tab);
        typed(&mut login, "hunter2");
        assert_eq!(login.shown_value(Field::Password, 40).0, "•••••••");
        press(&mut login, KeyCode::F(1));
        assert_eq!(login.shown_value(Field::Password, 40).0, "hunter2");
    }

    #[test]
    fn f2_switches_to_an_api_key_and_back() {
        let mut login = login(Screen::SignIn);
        press(&mut login, KeyCode::F(2));
        assert_eq!(login.fields(), &[Field::ApiKey]);
        login.paste("abc\n123");
        assert_eq!(login.value(Field::ApiKey), "abc123");
        assert_eq!(login.shown_value(Field::ApiKey, 40).0, "••••••");
        press(&mut login, KeyCode::F(2));
        assert_eq!(login.fields(), &[Field::Username, Field::Password]);
    }

    #[test]
    fn codes_take_six_digits_only() {
        let mut login = login(Screen::Mfa);
        typed(&mut login, "12a34567");
        assert_eq!(login.value(Field::Code), "123456");
    }

    #[test]
    fn time_zones_filter_as_you_type() {
        let mut login = login(Screen::Setup);
        typed(&mut login, "new york");
        assert_eq!(login.selected_timezone(), Some("America/New_York"));
        typed(&mut login, "zzz");
        assert_eq!(login.selected_timezone(), None);
        press(&mut login, KeyCode::Enter);
        assert!(matches!(login.error, Some((Field::Timezone, _))));
    }

    #[test]
    fn secrets_go_once_signed_in() {
        let mut login = login(Screen::SignIn);
        login.values.insert(Field::Password, "hunter2".into());
        login.values.insert(Field::ApiKey, "key".into());
        login.forget_secrets();
        assert_eq!(login.value(Field::Password), "");
        assert_eq!(login.value(Field::ApiKey), "");
    }

    #[test]
    fn date_formats_fit_the_column() {
        assert!(DATE_FORMATS.iter().all(|(code, _)| code.len() <= 3));
    }
}
