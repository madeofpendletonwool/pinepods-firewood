//! "Play on ▸": a popup for choosing which of the user's devices plays an episode.

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState},
};

use crate::api::Episode;
use crate::nowplaying::NowPlaying;
use crate::theme::ThemeColors;

pub struct DevicePicker {
    episode: Episode,
    list_state: ListState,
}

impl DevicePicker {
    pub fn new(episode: Episode) -> Self {
        Self {
            episode,
            list_state: ListState::default().with_selected(Some(0)),
        }
    }

    pub fn episode(&self) -> &Episode {
        &self.episode
    }

    pub fn selected(&self) -> usize {
        self.list_state.selected().unwrap_or(0)
    }

    pub fn move_by(&mut self, step: isize, len: usize) {
        if len == 0 {
            return;
        }
        let next = (self.selected() as isize + step).clamp(0, len as isize - 1);
        self.list_state.select(Some(next as usize));
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        colors: &ThemeColors,
        remote: &NowPlaying,
    ) {
        let devices = remote.state().devices;
        // Devices can leave while the popup is open.
        if self.selected() >= devices.len() {
            self.list_state
                .select(Some(devices.len().saturating_sub(1)));
        }

        let height = (devices.len() as u16).clamp(1, 8) + 4;
        let [popup] = Layout::horizontal([Constraint::Percentage(60)])
            .flex(Flex::Center)
            .areas(area);
        let [popup] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(popup);

        let items: Vec<ListItem> = devices
            .iter()
            .map(|device| {
                let state = match (device.episode(), device.playing) {
                    (None, _) => "idle",
                    (Some(_), true) => "playing",
                    (Some(_), false) => "paused",
                };
                ListItem::new(Line::from(vec![
                    Span::styled(device.label().to_string(), Style::default().fg(colors.text)),
                    Span::styled(
                        format!("  {} · {state}", device.device_type),
                        Style::default().fg(colors.text_secondary),
                    ),
                ]))
            })
            .collect();

        let title = format!(
            " Play on ▸ {} ",
            crate::tui::text::truncate(&self.episode.episode_title, 40)
        );
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(colors.accent))
                    .title(title)
                    .title_bottom(" Enter play there · Esc cancel ")
                    .style(Style::default().bg(colors.container)),
            )
            .highlight_style(
                Style::default()
                    .bg(colors.highlight)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("► ");

        frame.render_widget(Clear, popup);
        frame.render_stateful_widget(list, popup, &mut self.list_state);
    }
}
