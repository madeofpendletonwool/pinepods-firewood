//! "Play on ▸": a popup for choosing which of the user's devices plays an episode.

use ratatui::{
    Frame,
    crossterm::event::MouseEvent,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Cell, Paragraph},
};

use super::keymap::Action;
use super::text::truncate;
use super::ui::{self, list::Click, list::ListView};
use crate::api::Episode;
use crate::nowplaying::protocol::Device;
use crate::theme::Theme;

pub struct DevicePicker {
    episode: Episode,
    list: ListView,
}

impl DevicePicker {
    pub fn new(episode: Episode) -> Self {
        Self {
            episode,
            list: ListView::default(),
        }
    }

    pub fn episode(&self) -> &Episode {
        &self.episode
    }

    /// Devices come and go while the popup is open.
    pub fn sync(&mut self, devices: &[Device]) {
        self.list.sync(devices, |d| d.label().to_string());
    }

    pub fn navigate(&mut self, action: Action) {
        self.list.navigate(action);
    }

    pub fn mouse(&mut self, event: MouseEvent) -> Click {
        self.list.mouse(event)
    }

    pub fn selected(&self) -> Option<usize> {
        self.list.selected_index()
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme, devices: &[Device]) {
        let height = (devices.len() as u16).clamp(1, 8) + 4;
        let title = format!("Play on ▸ {}", truncate(&self.episode.episode_title, 40));
        let inner = ui::popup(frame, ui::centered(area, 64, height), &title, theme);
        let [rows, _, hint] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        self.list.render(
            frame,
            rows,
            theme,
            &[Constraint::Fill(1), Constraint::Length(20)],
            None,
            |i, _| {
                let device = &devices[i];
                let state = match (device.episode(), device.playing) {
                    (None, _) => "idle",
                    (Some(_), true) => "playing",
                    (Some(_), false) => "paused",
                };
                vec![
                    Cell::from(device.label().to_string()),
                    Cell::from(Span::styled(
                        format!("{} · {state}", device.device_type),
                        theme.muted(),
                    )),
                ]
            },
        );
        frame.render_widget(
            Paragraph::new(Line::styled("Enter play there · Esc cancel", theme.faint())),
            hint,
        );
    }
}
