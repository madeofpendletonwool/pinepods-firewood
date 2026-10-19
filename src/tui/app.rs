//! The frame around the pages: sidebar, now-playing bar, key hints, toasts and
//! overlays, and where input goes.
//!
//! Nothing here waits on the network. Pages load in the background (see
//! [`task`](super::task)), so the loop in `main` can draw every frame on time.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    layout::{Alignment, Constraint, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthStr;

use super::device_picker::DevicePicker;
use super::keymap::{Action, Context, Keymap};
use super::pages::{Ctx, Outcome, Page, Section};
use super::task::{Bus, Event, Pending, Resource, Toast, ToastKind};
use super::text::truncate;
use super::ui::{self, list::Click};
use crate::api::{Api, ApiResult};
use crate::auth::SessionInfo;
use crate::nowplaying::{Connection, NowPlaying};
use crate::playback::{Playback, PlaybackState, PlayerState};
use crate::settings::SettingsManager;
use crate::theme::{ColorDepth, Theme};

const SIDEBAR_WIDTH: u16 = 22;
/// Digits and icons only.
const SIDEBAR_COMPACT_WIDTH: u16 = 7;
/// Narrower than this, the sidebar drops its labels.
const SIDEBAR_FULL_TERMINAL: u16 = 110;
/// Narrower than this, it folds away entirely.
const SIDEBAR_MIN_TERMINAL: u16 = 60;
const VOLUME_STEP: f32 = 0.05;
const SPEED_STEP: f32 = 0.1;
/// How long a volume or speed change shows in the now-playing bar.
const FLASH_FOR: Duration = Duration::from_millis(1500);
const MAX_TOASTS: usize = 4;

pub struct App {
    api: Api,
    playback: Playback,
    remote: NowPlaying,
    settings: SettingsManager,
    account: String,

    theme: Theme,
    depth: ColorDepth,
    keymap: Keymap,
    bus: Bus,
    events: mpsc::UnboundedReceiver<Event>,
    server_theme: Pending<ApiResult<String>>,

    order: Vec<Section>,
    /// One stack of pages per section, in [`Section::ALL`] order.
    stacks: Vec<Vec<Box<dyn Page>>>,
    current: Section,
    player: PlayerState,

    toasts: Vec<Toast>,
    flash: Option<(String, Instant)>,
    help: Option<u16>,
    picker: Option<DevicePicker>,
    /// The last remote-control notice already shown, so each shows once.
    notice_seen: Option<Instant>,
    quit: bool,

    // Where things were drawn, for the mouse.
    sidebar_rows: Vec<(Rect, Section)>,
    content: Rect,
    play_button: Rect,
    progress: Rect,
    toast_area: Option<Rect>,
}

impl App {
    pub fn new(
        session: &SessionInfo,
        api: Api,
        playback: Playback,
        remote: NowPlaying,
        settings: SettingsManager,
    ) -> Self {
        let auth = &session.auth_state;
        let user = auth
            .user_details
            .Username
            .clone()
            .unwrap_or_else(|| "you".into());
        let server = auth
            .server_name
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_string();
        let depth = ColorDepth::detect();
        let (bus, events) = Bus::new();

        let (keymap, problems) = Keymap::load(Keymap::path().as_deref());
        for problem in problems {
            tracing::warn!("{problem}");
            bus.error(problem);
        }

        let server_theme = if settings.get().theme.follow_server {
            let api = api.clone();
            Pending::spawn(async move { api.get_theme().await })
        } else {
            Pending::default()
        };

        let order = Section::ordered(&settings.get().ui.tab_order);
        let current = order[0];
        let mut app = Self {
            theme: Theme::named(settings.theme_name(), depth),
            depth,
            keymap,
            bus,
            events,
            server_theme,
            stacks: Section::ALL.iter().map(|s| vec![s.root()]).collect(),
            order,
            current,
            player: playback.get_state(),
            api,
            playback,
            remote,
            settings,
            account: format!("{user} on {server}"),
            toasts: Vec::new(),
            flash: None,
            help: None,
            picker: None,
            notice_seen: None,
            quit: false,
            sidebar_rows: Vec::new(),
            content: Rect::default(),
            play_button: Rect::default(),
            progress: Rect::default(),
            toast_area: None,
        };
        app.with_page(|page, cx| page.show(cx));
        app
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    fn stack(&self, section: Section) -> &Vec<Box<dyn Page>> {
        let i = Section::ALL.iter().position(|s| *s == section).unwrap_or(0);
        &self.stacks[i]
    }

    fn stack_mut(&mut self, section: Section) -> &mut Vec<Box<dyn Page>> {
        let i = Section::ALL.iter().position(|s| *s == section).unwrap_or(0);
        &mut self.stacks[i]
    }

    fn page(&self) -> &dyn Page {
        self.stack(self.current)
            .last()
            .expect("every section has a root page")
            .as_ref()
    }

    /// Runs `f` on the open page with a context borrowed from the rest of the app.
    fn with_page<R>(&mut self, f: impl FnOnce(&mut dyn Page, &mut Ctx) -> R) -> R {
        let i = Section::ALL
            .iter()
            .position(|s| *s == self.current)
            .unwrap_or(0);
        let page = self.stacks[i]
            .last_mut()
            .expect("every section has a root page");
        let mut cx = Ctx {
            api: &self.api,
            playback: &self.playback,
            remote: &self.remote,
            bus: &self.bus,
            theme: &self.theme,
            keymap: &self.keymap,
            player: &self.player,
            settings: &mut self.settings,
            account: &self.account,
        };
        f(page.as_mut(), &mut cx)
    }

    /// Once per frame: take in what the background work produced.
    pub fn tick(&mut self) {
        let previous = self.player.current_episode.as_ref().map(|e| e.key());
        self.player = self.playback.get_state();
        let current = self.player.current_episode.as_ref().map(|e| e.key());
        if current != previous && previous.is_some() {
            // The last episode's position was saved; lists showing it are out of date.
            self.bus.changed(Resource::Progress);
        }

        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Toast(toast) => self.push_toast(toast),
                Event::Changed(resource) => {
                    for page in self.stacks.iter_mut().flatten() {
                        page.changed(resource);
                    }
                }
            }
        }
        let now = Instant::now();
        self.toasts
            .retain(|t| now.duration_since(t.at) < toast_life(t.kind));
        if self
            .flash
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > FLASH_FOR)
        {
            self.flash = None;
        }

        self.show_remote_notice();
        self.apply_server_theme();
        self.apply_settings();

        self.with_page(|page, cx| {
            page.tick(cx);
            page.show(cx);
        });
    }

    fn push_toast(&mut self, toast: Toast) {
        // The same message again just restarts its timer.
        self.toasts.retain(|t| t.text != toast.text);
        self.toasts.push(toast);
        if self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
        }
    }

    fn show_remote_notice(&mut self) {
        let Some(notice) = self.remote.state().notice else {
            return;
        };
        if self.notice_seen == Some(notice.at) {
            return;
        }
        self.notice_seen = Some(notice.at);
        let kind = if notice.is_error {
            ToastKind::Error
        } else {
            ToastKind::Success
        };
        self.bus.toast(kind, notice.text);
    }

    fn apply_server_theme(&mut self) {
        let Some(result) = self.server_theme.poll() else {
            return;
        };
        match result {
            Ok(name) if Theme::exists(&name) => {
                if self.settings.get().theme.follow_server
                    && name != self.settings.theme_name()
                    && let Err(e) = self.settings.update(|s| s.theme.theme_name = name)
                {
                    tracing::warn!("couldn't save the theme: {e:#}");
                }
            }
            // A custom theme from the web app; Firewood only has the built-in ones.
            Ok(name) => tracing::info!("the web app's theme {name:?} isn't built in; keeping ours"),
            Err(e) => tracing::warn!("couldn't fetch the theme: {e}"),
        }
    }

    /// Picks up settings the Settings page changed.
    fn apply_settings(&mut self) {
        let name = self.settings.theme_name();
        if !name.eq_ignore_ascii_case(&self.theme.name) && Theme::exists(name) {
            self.theme = Theme::named(name, self.depth);
        }
        let order = Section::ordered(&self.settings.get().ui.tab_order);
        if order != self.order {
            self.order = order;
        }
    }

    fn switch_to(&mut self, section: Section) {
        if self.current != section {
            self.current = section;
            self.with_page(|page, cx| page.show(cx));
        }
    }

    fn step_section(&mut self, step: isize) {
        let len = self.order.len() as isize;
        let i = self
            .order
            .iter()
            .position(|s| *s == self.current)
            .unwrap_or(0) as isize;
        self.switch_to(self.order[(i + step).rem_euclid(len) as usize]);
    }

    fn apply_outcome(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Push(mut page) => {
                let cx = Ctx {
                    api: &self.api,
                    playback: &self.playback,
                    remote: &self.remote,
                    bus: &self.bus,
                    theme: &self.theme,
                    keymap: &self.keymap,
                    player: &self.player,
                    settings: &mut self.settings,
                    account: &self.account,
                };
                page.show(&cx);
                self.stack_mut(self.current).push(page);
            }
            Outcome::Pop => self.pop(),
            Outcome::Handled | Outcome::Ignored => {}
        }
    }

    fn pop(&mut self) {
        let stack = self.stack_mut(self.current);
        if stack.len() > 1 {
            stack.pop();
            self.with_page(|page, cx| page.show(cx));
        }
    }

    pub fn key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        // Ctrl+C always quits, even mid-typing.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.help.is_some() {
            self.help_key(key);
            return;
        }
        if self.picker.is_some() {
            self.picker_key(key);
            return;
        }

        if let Some(outcome) = self.with_page(|page, cx| page.key(key, cx)) {
            self.apply_outcome(outcome);
            return;
        }
        if self.page().typing() {
            return;
        }

        let contexts = self.page().contexts();
        let action = self.keymap.action(key.into(), contexts);
        if let Some(action) = action {
            let outcome = self.with_page(|page, cx| page.action(action, cx));
            if !matches!(outcome, Outcome::Ignored) {
                self.apply_outcome(outcome);
                return;
            }
            self.global(action);
            return;
        }
        // Digits go to sidebar entries in the order shown; 0 is the tenth.
        if let KeyCode::Char(c @ '0'..='9') = key.code
            && key.modifiers.is_empty()
        {
            let index = (c as usize + 10 - '1' as usize) % 10;
            if let Some(&section) = self.order.get(index) {
                self.switch_to(section);
            }
        }
    }

    /// Text pasted into the terminal goes to whatever field has focus.
    pub fn paste(&mut self, text: &str) {
        if !self.page().typing() {
            return;
        }
        for c in text.chars().filter(|c| !c.is_control()) {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            self.with_page(|page, cx| page.key(key, cx));
        }
    }

    /// What an action means when the page doesn't use it.
    fn global(&mut self, action: Action) {
        let skip = Duration::from_secs(self.settings.skip_interval());
        let state = &self.player;
        let has_episode = state.current_episode.is_some();
        let result = match action {
            Action::Quit => {
                self.quit = true;
                Ok(())
            }
            Action::Help => {
                self.help = Some(0);
                Ok(())
            }
            Action::NextPage => {
                self.step_section(1);
                Ok(())
            }
            Action::PrevPage => {
                self.step_section(-1);
                Ok(())
            }
            Action::Refresh => {
                self.with_page(|page, cx| page.refresh(cx));
                Ok(())
            }
            Action::Back => {
                self.pop();
                Ok(())
            }
            Action::NowPlaying => {
                self.switch_to(Section::Player);
                Ok(())
            }
            Action::PlayOn => {
                self.open_picker();
                Ok(())
            }
            Action::TogglePlay if has_episode => self.playback.toggle_play_pause(),
            Action::SeekForward if has_episode => self.playback.skip_forward(skip),
            Action::SeekBackward if has_episode => self.playback.skip_backward(skip),
            Action::VolumeUp | Action::VolumeDown => {
                let step = if action == Action::VolumeUp {
                    VOLUME_STEP
                } else {
                    -VOLUME_STEP
                };
                let volume = (state.volume + step).clamp(0.0, 1.0);
                self.flash = Some((format!("Volume {:.0}%", volume * 100.0), Instant::now()));
                self.playback.set_volume(volume)
            }
            Action::SpeedUp | Action::SpeedDown if has_episode => {
                let step = if action == Action::SpeedUp {
                    SPEED_STEP
                } else {
                    -SPEED_STEP
                };
                let speed = ui::step_speed(state.speed, step);
                self.flash = Some((format!("Speed {}", ui::speed(speed)), Instant::now()));
                self.playback.set_speed(speed)
            }
            Action::TogglePlay
            | Action::SeekForward
            | Action::SeekBackward
            | Action::SpeedUp
            | Action::SpeedDown => {
                self.bus.info("Nothing is playing");
                Ok(())
            }
            _ => Ok(()),
        };
        if result.is_err() {
            self.bus
                .error("Playback has stopped working; restart Firewood");
        }
    }

    fn help_key(&mut self, key: KeyEvent) {
        let action = self.keymap.action(key.into(), &[Context::List]);
        let scroll = self.help.unwrap_or(0);
        self.help = match (key.code, action) {
            (KeyCode::Esc, _) | (_, Some(Action::Help | Action::Quit | Action::Back)) => None,
            (_, Some(Action::Down)) => Some(scroll.saturating_add(1)),
            (_, Some(Action::Up)) => Some(scroll.saturating_sub(1)),
            (_, Some(Action::PageDown)) => Some(scroll.saturating_add(10)),
            (_, Some(Action::PageUp)) => Some(scroll.saturating_sub(10)),
            (_, Some(Action::Top)) => Some(0),
            _ => Some(scroll),
        };
    }

    fn open_picker(&mut self) {
        let Some(episode) = self.page().selected_episode() else {
            self.bus.info("Select an episode first");
            return;
        };
        let devices = self.remote.state().devices;
        if devices.is_empty() {
            let reason = match self.remote.state().connection {
                Connection::Connected => "No other devices are connected",
                _ => "Not connected to the server's remote control",
            };
            self.bus.info(reason);
            return;
        }
        let mut picker = DevicePicker::new(episode);
        picker.sync(&devices);
        self.picker = Some(picker);
    }

    fn picker_key(&mut self, key: KeyEvent) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        let devices = self.remote.state().devices;
        picker.sync(&devices);
        let action = self.keymap.action(key.into(), &[Context::List]);
        match (key.code, action) {
            (KeyCode::Esc, _) | (_, Some(Action::Back | Action::Quit)) => self.picker = None,
            (KeyCode::Enter, _) => self.play_on_selected(),
            (_, Some(action)) => picker.navigate(action),
            _ => {}
        }
    }

    fn play_on_selected(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        let devices = self.remote.state().devices;
        if let Some(device) = picker.selected().and_then(|i| devices.get(i)) {
            self.remote
                .play_on(&device.device_id, picker.episode().key());
        }
    }

    pub fn mouse(&mut self, event: MouseEvent) {
        let at = Position::new(event.column, event.row);
        let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));
        if self.help.is_some() {
            if click {
                self.help = None;
            } else if event.kind == MouseEventKind::ScrollDown {
                self.help = self.help.map(|s| s.saturating_add(3));
            } else if event.kind == MouseEventKind::ScrollUp {
                self.help = self.help.map(|s| s.saturating_sub(3));
            }
            return;
        }
        if let Some(picker) = &mut self.picker {
            match picker.mouse(event) {
                Click::Activated => self.play_on_selected(),
                Click::Missed if click => self.picker = None,
                _ => {}
            }
            return;
        }
        if click {
            if let Some(&(_, section)) = self.sidebar_rows.iter().find(|(r, _)| r.contains(at)) {
                self.switch_to(section);
                return;
            }
            if self.play_button.contains(at) {
                self.global(Action::TogglePlay);
                return;
            }
            if self.progress.contains(at) {
                let total = self.player.total_duration;
                if !total.is_zero() && self.progress.width > 0 {
                    let ratio =
                        f64::from(event.column - self.progress.x) / f64::from(self.progress.width);
                    let _ = self.playback.seek(total.mul_f64(ratio.clamp(0.0, 1.0)));
                }
                return;
            }
            if let Some(i) = self.toast_at(at) {
                self.toasts.remove(i);
                return;
            }
        }
        if self.content.contains(at) {
            let outcome = self.with_page(|page, cx| page.mouse(event, cx));
            self.apply_outcome(outcome);
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(Block::new().style(self.theme.base()), area);

        let [main, nowbar, footer] = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(area);

        let content_area = if area.width >= SIDEBAR_MIN_TERMINAL {
            let compact = area.width < SIDEBAR_FULL_TERMINAL;
            let width = if compact {
                SIDEBAR_COMPACT_WIDTH
            } else {
                SIDEBAR_WIDTH
            };
            let [sidebar, content] =
                Layout::horizontal([Constraint::Length(width), Constraint::Min(0)]).areas(main);
            self.render_sidebar(frame, sidebar, compact);
            content
        } else {
            self.sidebar_rows.clear();
            main
        };

        let [header, _, body] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(pad(content_area, 2, 1));
        self.render_header(frame, header);
        self.content = body;
        self.with_page(|page, cx| page.render(frame, body, cx));

        self.render_nowbar(frame, nowbar);
        self.render_footer(frame, footer);
        self.render_toasts(frame, main);

        if let Some(picker) = &mut self.picker {
            let devices = self.remote.state().devices;
            picker.sync(&devices);
            picker.render(frame, area, &self.theme, &devices);
        }
        if self.help.is_some() {
            self.render_help(frame, area);
        }
    }

    fn render_sidebar(&mut self, frame: &mut Frame, area: Rect, compact: bool) {
        let theme = &self.theme;
        let block = Block::new()
            .borders(Borders::RIGHT)
            .border_style(theme.border(false));
        let inner = pad(block.inner(area), 1, 0);
        frame.render_widget(block, area);

        let [logo, _, entries, status] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .areas(Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..inner
        });
        let name = if compact { "" } else { "Firewood" };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("▲ ", Style::new().fg(theme.success)),
                Span::styled(name, theme.heading()),
            ])),
            logo,
        );

        self.sidebar_rows.clear();
        for (i, section) in self.order.iter().enumerate() {
            if i as u16 >= entries.height {
                break;
            }
            let row = Rect {
                y: entries.y + i as u16,
                height: 1,
                ..entries
            };
            let digit = match i {
                0..=8 => (i + 1).to_string(),
                9 => "0".to_string(),
                _ => " ".to_string(),
            };
            let current = *section == self.current;
            let label = if compact {
                format!(" {}", section.icon())
            } else {
                format!(" {} {}", section.icon(), section.title())
            };
            let pad_to = usize::from(row.width.saturating_sub(3));
            let label = format!("{label:<pad_to$}");
            let line = Line::from(vec![
                Span::styled(format!("{digit} "), theme.faint()),
                Span::styled(label, if current { theme.active } else { theme.muted() }),
            ]);
            frame.render_widget(Paragraph::new(line), row);
            self.sidebar_rows.push((row, *section));
        }

        let remote = self.remote.state();
        let (dot, color, text) = match remote.connection {
            Connection::Connected => ("●", theme.success, "Remote on".to_string()),
            Connection::Connecting => ("○", theme.warning, "Connecting…".to_string()),
            Connection::Offline { .. } => ("○", theme.error, "Remote offline".to_string()),
            Connection::Disabled => ("○", theme.faint, "Remote off".to_string()),
        };
        let others = remote.devices.len();
        if compact {
            let count = if others > 0 {
                others.to_string()
            } else {
                String::new()
            };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!("{dot} "), Style::new().fg(color)),
                    Span::styled(count, theme.faint()),
                ])),
                status,
            );
            return;
        }
        let devices = match others {
            0 => String::new(),
            1 => " · 1 device".into(),
            n => format!(" · {n} devices"),
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(format!("{dot} "), Style::new().fg(color)),
                    Span::styled(format!("{text}{devices}"), theme.faint()),
                ]),
                Line::styled(
                    truncate(&self.account, usize::from(status.width)),
                    theme.faint(),
                ),
            ]),
            status,
        );
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let theme = &self.theme;
        let stack = self.stack(self.current);
        let mut spans = Vec::new();
        let titles: Vec<String> = stack.iter().map(|p| p.title()).collect();
        let last = titles.len() - 1;
        let budget = usize::from(area.width.saturating_sub(4));
        let crumb_width = budget / titles.len().max(1);
        for (i, title) in titles.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" › ", theme.faint()));
            }
            let style = if i == last {
                theme.heading()
            } else {
                theme.muted()
            };
            let width = if i == last {
                budget
            } else {
                crumb_width.min(24)
            };
            spans.push(Span::styled(truncate(title, width), style));
        }
        if self.page().busy() {
            spans.push(Span::styled(format!("  {}", ui::spinner()), theme.accent()));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_nowbar(&mut self, frame: &mut Frame, area: Rect) {
        let theme = &self.theme;
        let block = Block::new()
            .borders(Borders::TOP)
            .border_style(theme.border(false));
        let inner = pad(block.inner(area), 2, 0);
        frame.render_widget(block, area);
        self.play_button = Rect::default();
        self.progress = Rect::default();

        let state = &self.player;
        let Some(episode) = &state.current_episode else {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "Nothing playing. Press Enter on an episode to start it.",
                    theme.faint(),
                )),
                inner,
            );
            return;
        };
        let [top, bottom] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

        let (icon, icon_style) = match &state.playback_state {
            PlaybackState::Playing if state.buffering => (ui::spinner(), theme.accent()),
            PlaybackState::Playing => ("▶", theme.accent()),
            PlaybackState::Paused => ("⏸", theme.muted()),
            PlaybackState::Loading => (ui::spinner(), theme.accent()),
            PlaybackState::Stopped => ("■", theme.muted()),
            PlaybackState::Error(_) => ("!", Style::new().fg(theme.error)),
        };
        let mut right: Vec<Span> = Vec::new();
        match (&self.flash, &state.playback_state) {
            (Some((text, _)), _) => right.push(Span::styled(text.clone(), theme.accent())),
            (_, PlaybackState::Error(msg)) => right.push(Span::styled(
                truncate(msg, 40),
                Style::new().fg(theme.error),
            )),
            (_, PlaybackState::Loading) if state.reconnecting.is_some() => {
                right.push(Span::styled(
                    "Reconnecting…",
                    Style::new().fg(theme.warning),
                ));
            }
            _ if state.buffering => {
                right.push(Span::styled("Buffering…", Style::new().fg(theme.warning)));
            }
            _ => {
                if (state.speed - 1.0).abs() > 0.01 {
                    right.push(Span::styled(ui::speed(state.speed), theme.accent()));
                    right.push(Span::raw("  "));
                }
                right.push(Span::styled(
                    format!("vol {:.0}%", state.volume * 100.0),
                    theme.faint(),
                ));
            }
        }
        let right_width: u16 = right.iter().map(|s| s.content.width() as u16).sum();
        let [play, title_area, right_area] = Layout::horizontal([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(right_width + 2),
        ])
        .areas(top);
        self.play_button = play;
        frame.render_widget(Paragraph::new(Span::styled(icon, icon_style)), play);
        let podcast = episode.podcast_name.clone().unwrap_or_default();
        let title_budget = usize::from(title_area.width);
        let title = truncate(&episode.episode_title, title_budget * 2 / 3);
        let rest = title_budget.saturating_sub(title.width() + 3);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(title, theme.base().add_modifier(Modifier::BOLD)),
                Span::styled("  ", theme.faint()),
                Span::styled(truncate(&podcast, rest), theme.muted()),
            ])),
            title_area,
        );
        frame.render_widget(
            Paragraph::new(Line::from(right)).alignment(Alignment::Right),
            right_area,
        );

        let elapsed = ui::clock(state.current_position);
        let total = ui::clock(state.total_duration);
        let [_, start, bar, end] = Layout::horizontal([
            Constraint::Length(3),
            Constraint::Length(elapsed.width() as u16 + 1),
            Constraint::Min(0),
            Constraint::Length(total.width() as u16 + 1),
        ])
        .areas(bottom);
        let ratio = if state.total_duration.is_zero() {
            0.0
        } else {
            state.current_position.as_secs_f64() / state.total_duration.as_secs_f64()
        };
        frame.render_widget(Paragraph::new(Span::styled(elapsed, theme.muted())), start);
        frame.render_widget(
            Paragraph::new(Line::from(ui::bar(ratio, bar.width, theme))),
            bar,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(total, theme.muted())).alignment(Alignment::Right),
            end,
        );
        self.progress = bar;
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let theme = &self.theme;
        let area = pad(area, 2, 0);
        let page = self.page();
        let contexts = page.contexts();
        let (left, right) = if page.typing() {
            (
                vec![
                    Span::styled("Enter", theme.key()),
                    Span::styled(" done  ", theme.muted()),
                    Span::styled("Esc", theme.key()),
                    Span::styled(" cancel", theme.muted()),
                ],
                Vec::new(),
            )
        } else {
            let mut right = ui::hints(&[Action::Help, Action::Quit], contexts, &self.keymap, theme);
            if self.stack(self.current).len() > 1 {
                let mut back = ui::hints(&[Action::Back], contexts, &self.keymap, theme);
                back.push(Span::styled("  ", theme.faint()));
                back.append(&mut right);
                right = back;
            }
            (
                ui::hints(page.hints(), contexts, &self.keymap, theme),
                right,
            )
        };
        let width = |spans: &[Span]| spans.iter().map(|s| s.content.width() as u16).sum::<u16>();
        let right_width = width(&right);
        // Drop hints from the end until the rest fit; past the first, each hint is a
        // gap, a key and a description.
        let room = area.width.saturating_sub(right_width + 2);
        let mut left = left;
        while width(&left) > room && left.len() > 3 {
            left.truncate(left.len() - 3);
        }
        let [left_area, right_area] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(area);
        frame.render_widget(Paragraph::new(Line::from(left)), left_area);
        frame.render_widget(Paragraph::new(Line::from(right)), right_area);
    }

    /// Where each toast is drawn: stacked up from the bottom right of `area`.
    fn toast_rects(&self, area: Rect) -> Vec<Rect> {
        let mut y = area.bottom().saturating_sub(1);
        let max_width = area.width.saturating_sub(4).min(64);
        self.toasts
            .iter()
            .rev()
            .map(|t| {
                let width = (t.text.width() as u16 + 4).min(max_width);
                let rect = Rect {
                    x: area.right().saturating_sub(width + 2),
                    y,
                    width,
                    height: 1,
                };
                y = y.saturating_sub(1);
                rect
            })
            .collect()
    }

    fn toast_at(&self, at: Position) -> Option<usize> {
        let area = self.toast_area?;
        let count = self.toasts.len();
        self.toast_rects(area)
            .iter()
            .position(|r| r.contains(at))
            .map(|i| count - 1 - i)
    }

    fn render_toasts(&mut self, frame: &mut Frame, area: Rect) {
        self.toast_area = Some(area);
        let theme = &self.theme;
        for (toast, rect) in self.toasts.iter().rev().zip(self.toast_rects(area)) {
            let (mark, color) = match toast.kind {
                ToastKind::Info => ("•", theme.accent),
                ToastKind::Success => ("✓", theme.success),
                ToastKind::Error => ("!", theme.error),
            };
            frame.render_widget(Clear, rect);
            let text = truncate(&toast.text, usize::from(rect.width.saturating_sub(4)));
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!(" {mark} "),
                        Style::new().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(text),
                    Span::raw(" "),
                ]))
                .style(theme.surface()),
                rect,
            );
        }
    }

    fn render_help(&mut self, frame: &mut Frame, area: Rect) {
        let theme = &self.theme;
        let page_contexts = self.page().contexts();
        let mut contexts: Vec<Context> = page_contexts.to_vec();
        contexts.push(Context::Global);

        let mut lines: Vec<Line> = Vec::new();
        for context in &contexts {
            let bindings = self.keymap.bindings(*context);
            if bindings.is_empty() {
                continue;
            }
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.push(Line::styled(context.title(), theme.heading()));
            for (action, keys) in bindings {
                // A binding the page's own context overrides doesn't apply here.
                let live: Vec<String> = keys
                    .iter()
                    .filter(|k| self.keymap.action(**k, page_contexts) == Some(*action))
                    .map(ToString::to_string)
                    .collect();
                if live.is_empty() {
                    continue;
                }
                lines.push(Line::from(vec![
                    Span::styled(format!("  {:<16}", live.join(" ")), theme.key()),
                    Span::styled(action.description(), theme.base()),
                ]));
            }
        }
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<16}", "1–9 0"), theme.key()),
            Span::styled("Go to a sidebar entry", theme.base()),
        ]));
        lines.push(Line::default());
        let path = Keymap::path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "keymap.toml".into());
        lines.push(Line::styled(
            format!("Rebind keys in {path}"),
            theme.faint(),
        ));
        lines.push(Line::styled(
            "Mouse: click to select, click again to open, wheel to scroll",
            theme.faint(),
        ));

        let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
        let rect = ui::centered(area, 64, height);
        let inner = ui::popup(frame, rect, "Keys", theme);
        let total = lines.len();
        let [body, hint] = if total > usize::from(inner.height) {
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner)
        } else {
            [inner, Rect::default()]
        };
        let max_scroll = (total as u16).saturating_sub(body.height);
        let scroll = self.help.unwrap_or(0).min(max_scroll);
        self.help = Some(scroll);
        frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), body);
        ui::scrollbar(frame, body, total, usize::from(scroll), theme);
        frame.render_widget(
            Paragraph::new(Line::styled("j/k scroll · Esc close", theme.faint()))
                .alignment(Alignment::Right),
            hint,
        );
    }
}

fn toast_life(kind: ToastKind) -> Duration {
    match kind {
        ToastKind::Error => Duration::from_secs(8),
        _ => Duration::from_secs(4),
    }
}

/// `area` shrunk by `x` columns each side and `y` rows top and bottom.
fn pad(area: Rect, x: u16, y: u16) -> Rect {
    Rect {
        x: area.x + x.min(area.width / 2),
        y: area.y + y.min(area.height / 2),
        width: area.width.saturating_sub(2 * x),
        height: area.height.saturating_sub(2 * y),
    }
}
