use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::audio::speed;
use crate::nowplaying::protocol::{Action, Device};
use crate::nowplaying::{Connection, NowPlaying, RemoteState};
use crate::theme::ThemeManager;

const SPEED_STEP: f32 = 0.25;
const SKIP_FORWARD: Duration = Duration::from_secs(30);
const SKIP_BACK: Duration = Duration::from_secs(15);

/// The user's other players, with handoff and transport controls for each.
pub struct DevicesPage {
    remote: NowPlaying,
    theme_manager: ThemeManager,
    list_state: ListState,
    /// Follows the device rather than the row, since the list reorders as devices
    /// come and go.
    selected_id: Option<String>,
}

impl DevicesPage {
    pub fn new(remote: NowPlaying) -> Self {
        Self {
            remote,
            theme_manager: ThemeManager::new(),
            list_state: ListState::default(),
            selected_id: None,
        }
    }

    pub fn update_theme(&mut self, theme_name: &str) {
        self.theme_manager.set_theme(theme_name);
    }

    fn selected(&self, state: &RemoteState) -> Option<Device> {
        let id = self.selected_id.as_deref()?;
        state.devices.iter().find(|d| d.device_id == id).cloned()
    }

    fn move_selection(&mut self, state: &RemoteState, step: isize) {
        if state.devices.is_empty() {
            return;
        }
        let current = self
            .selected_id
            .as_deref()
            .and_then(|id| state.devices.iter().position(|d| d.device_id == id))
            .unwrap_or(0) as isize;
        let next = (current + step).clamp(0, state.devices.len() as isize - 1) as usize;
        self.selected_id = Some(state.devices[next].device_id.clone());
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        let state = self.remote.state();
        let step = match key.code {
            KeyCode::Down | KeyCode::Char('j') => Some(1),
            KeyCode::Up | KeyCode::Char('k') => Some(-1),
            _ => None,
        };
        if let Some(step) = step {
            self.move_selection(&state, step);
            return Ok(());
        }
        let Some(device) = self.selected(&state) else {
            return Ok(());
        };
        let target = device.device_id.as_str();
        match key.code {
            KeyCode::Enter => self.remote.play_here(&device),
            KeyCode::Char('s') => self.remote.send_playback_to(target),
            KeyCode::Char('p') if device.episode().is_some() => {
                let action = if device.playing {
                    Action::Pause
                } else {
                    Action::Play
                };
                self.remote.command(target, action);
            }
            KeyCode::Left | KeyCode::Char('h') if device.episode().is_some() => {
                self.remote.command(target, Action::SkipBack(SKIP_BACK));
            }
            KeyCode::Right | KeyCode::Char('l') if device.episode().is_some() => {
                self.remote
                    .command(target, Action::SkipForward(SKIP_FORWARD));
            }
            KeyCode::Char(c @ ('[' | ']')) if device.episode().is_some() => {
                let current = if device.speed > 0.0 {
                    device.speed as f32
                } else {
                    1.0
                };
                let step = if c == ']' { SPEED_STEP } else { -SPEED_STEP };
                let speed = speed::clamp(current + step);
                self.remote.command(target, Action::SetSpeed(speed));
            }
            _ => {}
        }
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let state = self.remote.state();
        // Keep a selection whenever there's something to select.
        if self.selected(&state).is_none() {
            self.selected_id = state.devices.first().map(|d| d.device_id.clone());
        }
        let index = self
            .selected_id
            .as_deref()
            .and_then(|id| state.devices.iter().position(|d| d.device_id == id));
        self.list_state.select(index);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(3),
            ])
            .split(area);
        self.render_status(frame, chunks[0], &state);
        self.render_devices(frame, chunks[1], &state);
        self.render_footer(frame, chunks[2]);
    }

    fn render_status(&self, frame: &mut Frame, area: Rect, state: &RemoteState) {
        let colors = self.theme_manager.get_colors();
        let (dot, color, text) = match &state.connection {
            Connection::Connected => (
                "●",
                colors.success,
                format!("Connected as {}", state.device_name),
            ),
            Connection::Connecting => ("○", colors.warning, "Connecting…".to_string()),
            Connection::Offline { reason } => (
                "○",
                colors.error,
                format!("Not connected: {reason}. Retrying…"),
            ),
            Connection::Disabled => (
                "○",
                colors.text_secondary,
                format!(
                    "Remote control is off ({} is set)",
                    crate::nowplaying::DISABLE_ENV
                ),
            ),
        };
        let status = Paragraph::new(Line::from(vec![
            Span::styled(format!("{dot} "), Style::default().fg(color)),
            Span::styled(text, Style::default().fg(colors.text)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(colors.border))
                .title("📡 Devices")
                .title_style(Style::default().fg(colors.accent)),
        );
        frame.render_widget(status, area);
    }

    fn render_devices(&mut self, frame: &mut Frame, area: Rect, state: &RemoteState) {
        let colors = self.theme_manager.get_colors();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(colors.border))
            .title(format!("Your other devices ({})", state.devices.len()))
            .title_style(Style::default().fg(colors.text));

        if state.devices.is_empty() {
            let text = match state.connection {
                Connection::Connected => {
                    "No other devices are connected. Open PinePods in a browser or the mobile \
                     app and it will show up here."
                }
                _ => "Devices appear here once Firewood is connected to the server.",
            };
            let empty = Paragraph::new(text)
                .style(
                    Style::default()
                        .fg(colors.text_secondary)
                        .add_modifier(Modifier::ITALIC),
                )
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(block);
            frame.render_widget(empty, area);
            return;
        }

        let now = Instant::now();
        let items: Vec<ListItem> = state
            .devices
            .iter()
            .map(|device| {
                let (badge, badge_color) = match (device.episode(), device.playing) {
                    (None, _) => ("Idle", colors.text_secondary),
                    (Some(_), true) => ("▶ Playing", colors.success),
                    (Some(_), false) => ("⏸ Paused", colors.warning),
                };
                let header = Line::from(vec![
                    Span::raw(format!("{} ", type_icon(&device.device_type))),
                    Span::styled(
                        device.label().to_string(),
                        Style::default()
                            .fg(colors.text)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {}  ", device.device_type),
                        Style::default().fg(colors.text_secondary),
                    ),
                    Span::styled(badge, Style::default().fg(badge_color)),
                ]);
                let detail = match device.episode() {
                    None => Line::from(Span::styled(
                        "     Nothing playing",
                        Style::default().fg(colors.text_secondary),
                    )),
                    Some(_) => {
                        let mut spans = vec![
                            Span::styled(
                                format!("     {}  ", crate::tui::text::truncate(&device.title, 60)),
                                Style::default().fg(colors.primary),
                            ),
                            Span::styled(
                                progress(state.live_position(device, now), device.duration_sec),
                                Style::default().fg(colors.text_secondary),
                            ),
                        ];
                        if device.speed > 0.0 && (device.speed - 1.0).abs() > 0.01 {
                            spans.push(Span::styled(
                                format!("  {}×", device.speed),
                                Style::default().fg(colors.accent),
                            ));
                        }
                        Line::from(spans)
                    }
                };
                ListItem::new(vec![header, detail])
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(colors.highlight)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("► ");
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let colors = self.theme_manager.get_colors();
        let controls = [
            ("Enter", "Play here"),
            ("s", "Send what's playing here"),
            ("p", "Play/Pause"),
            ("←→", "Skip"),
            ("[ ]", "Speed"),
        ];
        let mut spans = Vec::new();
        for (i, (key, desc)) in controls.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(
                *key,
                Style::default()
                    .fg(colors.primary)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" {desc}"),
                Style::default().fg(colors.text_secondary),
            ));
        }
        let footer = Paragraph::new(Line::from(spans))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(colors.border))
                    .title("🔧 Controls")
                    .title_style(Style::default().fg(colors.accent)),
            );
        frame.render_widget(footer, area);
    }
}

fn type_icon(device_type: &str) -> &'static str {
    match device_type.to_ascii_lowercase().as_str() {
        "firewood" => "🌲",
        "ios" | "android" | "mobile" => "📱",
        "web" => "🌐",
        _ => "💻",
    }
}

fn progress(position: Duration, duration_sec: f64) -> String {
    let clock = |secs: u64| {
        let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
        if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m}:{s:02}")
        }
    };
    if duration_sec > 0.0 {
        format!(
            "{} / {}",
            clock(position.as_secs()),
            clock(duration_sec as u64)
        )
    } else {
        clock(position.as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_reads_like_the_player() {
        assert_eq!(progress(Duration::from_secs(65), 3725.0), "1:05 / 1:02:05");
        assert_eq!(progress(Duration::from_secs(5), 0.0), "0:05");
    }
}
