//! The user's other players, with handoff and transport controls for each.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    crossterm::event::MouseEvent,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Cell, Paragraph, Wrap},
};

use super::{Ctx, Outcome, Page};
use crate::audio::speed;
use crate::nowplaying::protocol::{Action as Remote, Device};
use crate::nowplaying::{Connection, DISABLE_ENV};
use crate::theme::Theme;
use crate::tui::keymap::{Action, Context};
use crate::tui::ui::list::{Click, ListView};
use crate::tui::ui::{self, clock};

const SPEED_STEP: f32 = 0.25;
const SKIP_FORWARD: Duration = Duration::from_secs(30);
const SKIP_BACK: Duration = Duration::from_secs(15);

pub struct DevicesPage {
    list: ListView,
    /// Follows the device rather than the row, since the list reorders as devices
    /// come and go.
    selected_id: Option<String>,
    devices: Vec<Device>,
}

impl Default for DevicesPage {
    fn default() -> Self {
        Self {
            list: ListView::default().with_row_height(3),
            selected_id: None,
            devices: Vec::new(),
        }
    }
}

impl DevicesPage {
    fn selected(&self) -> Option<&Device> {
        self.list.selected_index().and_then(|i| self.devices.get(i))
    }
}

impl Page for DevicesPage {
    fn title(&self) -> String {
        "Devices".into()
    }

    fn contexts(&self) -> &'static [Context] {
        &[Context::Devices, Context::List]
    }

    fn hints(&self) -> &'static [Action] {
        &[
            Action::PlayHere,
            Action::SendHere,
            Action::RemoteToggle,
            Action::RemoteForward,
            Action::RemoteFaster,
        ]
    }

    fn tick(&mut self, cx: &Ctx) {
        self.devices = cx.remote.state().devices;
        self.list.sync(&self.devices, |d| d.label().to_string());
        let position = self
            .selected_id
            .as_deref()
            .and_then(|id| self.devices.iter().position(|d| d.device_id == id));
        if let Some(position) = position {
            self.list.select_index(position);
        }
        self.selected_id = self.selected().map(|d| d.device_id.clone());
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        if self.list.navigate(action) {
            self.selected_id = self.selected().map(|d| d.device_id.clone());
            return Outcome::Handled;
        }
        let Some(device) = self.selected().cloned() else {
            return Outcome::Ignored;
        };
        let target = device.device_id.as_str();
        let has_episode = device.episode().is_some();
        match action {
            Action::PlayHere if has_episode => cx.remote.play_here(&device),
            Action::PlayHere => cx
                .bus
                .info(format!("{} isn't playing anything", device.label())),
            Action::SendHere => {
                if cx.player.current_episode.is_none() {
                    cx.bus.info("Nothing is playing here to send");
                } else {
                    cx.remote.send_playback_to(target);
                }
            }
            Action::RemoteToggle if has_episode => {
                let command = if device.playing {
                    Remote::Pause
                } else {
                    Remote::Play
                };
                cx.remote.command(target, command);
            }
            Action::RemoteBack if has_episode => {
                cx.remote.command(target, Remote::SkipBack(SKIP_BACK));
            }
            Action::RemoteForward if has_episode => {
                cx.remote.command(target, Remote::SkipForward(SKIP_FORWARD));
            }
            Action::RemoteSlower | Action::RemoteFaster if has_episode => {
                let current = if device.speed > 0.0 {
                    device.speed as f32
                } else {
                    1.0
                };
                let step = if action == Action::RemoteFaster {
                    SPEED_STEP
                } else {
                    -SPEED_STEP
                };
                cx.remote
                    .command(target, Remote::SetSpeed(speed::clamp(current + step)));
            }
            Action::RemoteToggle
            | Action::RemoteBack
            | Action::RemoteForward
            | Action::RemoteSlower
            | Action::RemoteFaster => {
                cx.bus
                    .info(format!("{} isn't playing anything", device.label()));
            }
            _ => return Outcome::Ignored,
        }
        Outcome::Handled
    }

    fn mouse(&mut self, event: MouseEvent, cx: &mut Ctx) -> Outcome {
        match self.list.mouse(event) {
            Click::Activated => self.action(Action::PlayHere, cx),
            Click::Moved => {
                self.selected_id = self.selected().map(|d| d.device_id.clone());
                Outcome::Handled
            }
            Click::Missed => Outcome::Ignored,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        let state = cx.remote.state();
        let [status, _, body] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(area);

        let (dot, color, text) = match &state.connection {
            Connection::Connected => (
                "●",
                theme.success,
                format!("Connected as {}", state.device_name),
            ),
            Connection::Connecting => {
                ("○", theme.warning, format!("{} Connecting…", ui::spinner()))
            }
            Connection::Offline { reason } => (
                "○",
                theme.error,
                format!("Not connected: {reason}. Retrying…"),
            ),
            Connection::Disabled => (
                "○",
                theme.muted,
                format!("Remote control is off ({DISABLE_ENV} is set)"),
            ),
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{dot} "), Style::new().fg(color)),
                Span::styled(text, theme.base()),
            ]))
            .wrap(Wrap { trim: true }),
            status,
        );

        if self.devices.is_empty() {
            let text = match state.connection {
                Connection::Connected => {
                    "No other devices are connected. Open PinePods in a browser or the mobile \
                     app and it shows up here."
                }
                _ => "Devices appear here once Firewood is connected to the server.",
            };
            ui::message(
                frame,
                body,
                vec![Line::styled(text, theme.muted())],
                theme.base(),
            );
            return;
        }

        let now = Instant::now();
        let devices = &self.devices;
        let width = body.width.saturating_sub(4);
        self.list.render(
            frame,
            body,
            theme,
            &[Constraint::Length(2), Constraint::Fill(1)],
            None,
            |i, _| {
                let device = &devices[i];
                let position = state.live_position(device, now);
                vec![
                    Cell::from(Span::styled(type_icon(&device.device_type), theme.accent())),
                    Cell::from(Text::from(card(device, position, width, theme))),
                ]
            },
        );
    }
}

/// Name and state, then what's playing, then where it is.
fn card<'a>(device: &Device, position: Duration, width: u16, theme: &Theme) -> Vec<Line<'a>> {
    let (badge, badge_color) = match (device.episode(), device.playing) {
        (None, _) => ("idle", theme.muted),
        (Some(_), true) => ("▶ playing", theme.success),
        (Some(_), false) => ("⏸ paused", theme.warning),
    };
    let header = Line::from(vec![
        Span::styled(
            device.label().to_string(),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  {}  ", device.device_type), theme.faint()),
        Span::styled(badge, Style::new().fg(badge_color)),
    ]);
    if device.episode().is_none() {
        return vec![header, Line::styled("Nothing playing", theme.muted())];
    }
    let mut time = if device.duration_sec > 0.0 {
        format!(
            " {} / {}",
            clock(position),
            clock(Duration::from_secs_f64(device.duration_sec))
        )
    } else {
        format!(" {}", clock(position))
    };
    if device.speed > 0.0 && (device.speed - 1.0).abs() > 0.01 {
        time.push_str(&format!("  {}×", device.speed));
    }
    let bar_width = width
        .saturating_sub(time.chars().count() as u16 + 1)
        .min(40);
    let ratio = if device.duration_sec > 0.0 {
        (position.as_secs_f64() / device.duration_sec).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut progress = ui::bar(ratio, bar_width, theme);
    progress.push(Span::styled(time, theme.muted()));
    vec![
        header,
        Line::styled(device.title.clone(), theme.accent()),
        Line::from(progress),
    ]
}

fn type_icon(device_type: &str) -> &'static str {
    match device_type.to_ascii_lowercase().as_str() {
        "firewood" => "▣",
        "ios" | "android" | "mobile" => "▯",
        "web" => "◍",
        _ => "▭",
    }
}
