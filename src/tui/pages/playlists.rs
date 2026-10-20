//! Playlists. Each is a query the server runs over your podcasts (unplayed and
//! under an hour, say), so opening one lists whatever fits it right now. Playing
//! from a playlist carries on through it when an episode ends.

use ratatui::{
    Frame,
    crossterm::event::{KeyEvent, MouseEvent},
    layout::{Constraint, Rect},
    text::{Line, Span, Text},
    widgets::Cell,
};

use super::episodes::EpisodeListPage;
use super::{Ctx, Outcome, Page};
use crate::api::Playlist;
use crate::tui::keymap::Action;
use crate::tui::task::{Remote, Resource};
use crate::tui::text::truncate;
use crate::tui::ui::{self, list::Click, list::ListView};

pub struct PlaylistsPage {
    data: Remote<Vec<Playlist>>,
    list: ListView,
}

impl Default for PlaylistsPage {
    fn default() -> Self {
        Self {
            data: Remote::default(),
            list: ListView::default().with_row_height(2),
        }
    }
}

fn search_text(p: &Playlist) -> String {
    format!("{} {}", p.name, p.description)
}

/// A glyph for the web app's Phosphor icon name, by what it shows.
pub fn icon(name: &str) -> &'static str {
    let name = name.trim_start_matches("ph-");
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&["fast-forward", "skip"]) {
        "»"
    } else if has(&["play"]) && !name.contains("list") {
        "▶"
    } else if has(&["sparkle", "star"]) {
        "✦"
    } else if has(&["hourglass", "clock", "timer"]) {
        "⧗"
    } else if has(&["heart"]) {
        "♥"
    } else if has(&["bookmark"]) {
        "★"
    } else if has(&["car", "train", "bus", "bicycle", "airplane"]) {
        "→"
    } else {
        "♫"
    }
}

impl PlaylistsPage {
    fn load(&mut self, cx: &Ctx) {
        let api = cx.api.clone();
        self.data.load(async move { api.get_playlists().await });
    }

    fn items(&self) -> &[Playlist] {
        self.data.data.as_deref().unwrap_or_default()
    }

    fn open(&self) -> Outcome {
        match self.list.selected_index().and_then(|i| self.items().get(i)) {
            Some(p) => Outcome::Push(Box::new(EpisodeListPage::playlist(p.clone()))),
            None => Outcome::Handled,
        }
    }
}

impl Page for PlaylistsPage {
    fn title(&self) -> String {
        "Playlists".into()
    }

    fn hints(&self) -> &'static [Action] {
        &[Action::Activate, Action::Filter, Action::Refresh]
    }

    fn show(&mut self, cx: &Ctx) {
        if self.data.needs_load() {
            self.load(cx);
        }
    }

    fn tick(&mut self, cx: &Ctx) {
        if self.data.poll(cx.bus) {
            let items = self.data.data.as_deref().unwrap_or_default();
            self.list.sync(items, search_text);
        }
    }

    fn refresh(&mut self, cx: &Ctx) {
        self.load(cx);
    }

    fn changed(&mut self, resource: Resource) {
        // Counts move as episodes are played, saved and added.
        if matches!(resource, Resource::Progress | Resource::Podcasts) {
            self.data.mark_stale();
        }
    }

    fn busy(&self) -> bool {
        self.data.is_loading()
    }

    fn typing(&self) -> bool {
        self.list.is_typing()
    }

    fn key(&mut self, key: KeyEvent, _cx: &mut Ctx) -> Option<Outcome> {
        if !self.list.is_typing() {
            return None;
        }
        if self.list.filter_key(key) {
            let items = self.data.data.as_deref().unwrap_or_default();
            self.list.sync(items, search_text);
        }
        Some(Outcome::Handled)
    }

    fn action(&mut self, action: Action, _cx: &mut Ctx) -> Outcome {
        if self.list.navigate(action) {
            return Outcome::Handled;
        }
        match action {
            Action::Activate | Action::Open => return self.open(),
            Action::Filter => self.list.start_filter(),
            Action::Back if self.list.is_filtered() => {
                self.list.clear_filter();
                let items = self.data.data.as_deref().unwrap_or_default();
                self.list.sync(items, search_text);
            }
            _ => return Outcome::Ignored,
        }
        Outcome::Handled
    }

    fn mouse(&mut self, event: MouseEvent, _cx: &mut Ctx) -> Outcome {
        match self.list.mouse(event) {
            Click::Activated => self.open(),
            Click::Moved => Outcome::Handled,
            Click::Missed => Outcome::Ignored,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        if ui::load_state(
            frame,
            area,
            theme,
            self.data.is_empty_loading(),
            self.data.error(),
            "playlists",
        ) {
            return;
        }
        if self.list.is_empty() && !self.list.is_typing() {
            let text = if self.list.is_filtered() {
                "Nothing matches."
            } else {
                "No playlists. Make one in the web app."
            };
            ui::message(
                frame,
                area,
                vec![Line::styled(text, theme.muted())],
                theme.base(),
            );
            return;
        }
        let playing = cx.player.playlist;
        let items = self.data.data.as_deref().unwrap_or_default();
        self.list.render(
            frame,
            area,
            theme,
            &[
                Constraint::Length(1),
                Constraint::Fill(1),
                Constraint::Length(12),
            ],
            None,
            |i, w| {
                let p = &items[i];
                let mark = if playing == Some(p.playlist_id) {
                    Span::styled("▶", theme.accent())
                } else {
                    Span::styled(icon(&p.icon_name), theme.accent())
                };
                let count = ui::episode_count(p.episode_count);
                vec![
                    Cell::from(mark),
                    Cell::from(Text::from(vec![
                        Line::raw(truncate(&p.name, usize::from(w[1]))),
                        Line::styled(truncate(&p.description, usize::from(w[1])), theme.faint()),
                    ])),
                    Cell::from(Span::styled(count, theme.muted())),
                ]
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_follow_the_web_names() {
        assert_eq!(icon("ph-car"), "→");
        assert_eq!(icon("ph-fast-forward"), "»");
        assert_eq!(icon("ph-play"), "▶");
        assert_eq!(icon("ph-playlist"), "♫");
        assert_eq!(icon(""), "♫");
    }
}
