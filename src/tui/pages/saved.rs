//! Saved episodes, and your other collections.
//!
//! The server keeps saved episodes as the default collection. With no other
//! collections (or a server too old to have them) this is just the saved list; once
//! there are more, it lists the collections, Saved first, and Enter opens one.

use ratatui::{
    Frame,
    crossterm::event::{KeyEvent, MouseEvent},
    layout::{Constraint, Rect},
    text::{Line, Span, Text},
    widgets::Cell,
};

use super::episodes::EpisodeListPage;
use super::playlists::icon;
use super::{Ctx, Outcome, Page};
use crate::api::{Collection, Episode};
use crate::tui::keymap::{Action, Context};
use crate::tui::task::{Remote, Resource};
use crate::tui::text::truncate;
use crate::tui::ui::{self, list::Click, list::ListView};

pub struct SavedPage {
    saved: EpisodeListPage,
    collections: Remote<Vec<Collection>>,
    list: ListView,
}

impl Default for SavedPage {
    fn default() -> Self {
        Self {
            saved: EpisodeListPage::saved(),
            collections: Remote::default(),
            list: ListView::default().with_row_height(2),
        }
    }
}

fn search_text(c: &Collection) -> String {
    format!(
        "{} {}",
        c.name,
        c.description.as_deref().unwrap_or_default()
    )
}

impl SavedPage {
    /// More than the default collection to show.
    fn listing(&self) -> bool {
        self.collections
            .data
            .as_ref()
            .is_some_and(|c| c.iter().any(|c| !c.is_default))
    }

    fn items(&self) -> &[Collection] {
        self.collections.data.as_deref().unwrap_or_default()
    }

    fn load(&mut self, cx: &Ctx) {
        let api = cx.api.clone();
        self.collections.load(async move {
            let mut collections = api.get_collections().await?;
            collections.sort_by_key(|c| !c.is_default);
            Ok(collections)
        });
    }

    fn open(&self) -> Outcome {
        let Some(c) = self.list.selected_index().and_then(|i| self.items().get(i)) else {
            return Outcome::Handled;
        };
        let page = if c.is_default {
            EpisodeListPage::saved()
        } else {
            EpisodeListPage::collection(c.clone())
        };
        Outcome::Push(Box::new(page))
    }
}

impl Page for SavedPage {
    fn title(&self) -> String {
        if self.listing() {
            "Collections".into()
        } else {
            self.saved.title()
        }
    }

    fn contexts(&self) -> &'static [Context] {
        if self.listing() {
            &[Context::List]
        } else {
            self.saved.contexts()
        }
    }

    fn hints(&self) -> &'static [Action] {
        if self.listing() {
            &[Action::Activate, Action::Filter, Action::Refresh]
        } else {
            self.saved.hints()
        }
    }

    fn show(&mut self, cx: &Ctx) {
        if self.collections.needs_load() {
            self.load(cx);
        }
        if !self.listing() {
            self.saved.show(cx);
        }
    }

    fn tick(&mut self, cx: &Ctx) {
        if self.collections.poll(cx.bus) {
            let items = self.collections.data.as_deref().unwrap_or_default();
            self.list.sync(items, search_text);
        }
        self.saved.tick(cx);
    }

    fn refresh(&mut self, cx: &Ctx) {
        self.load(cx);
        if !self.listing() {
            self.saved.refresh(cx);
        }
    }

    fn changed(&mut self, resource: Resource) {
        if matches!(resource, Resource::Collections | Resource::Saved) {
            self.collections.mark_stale();
        }
        self.saved.changed(resource);
    }

    fn busy(&self) -> bool {
        self.collections.is_loading() || (!self.listing() && self.saved.busy())
    }

    fn typing(&self) -> bool {
        if self.listing() {
            self.list.is_typing()
        } else {
            self.saved.typing()
        }
    }

    fn key(&mut self, key: KeyEvent, cx: &mut Ctx) -> Option<Outcome> {
        if !self.listing() {
            return self.saved.key(key, cx);
        }
        if !self.list.is_typing() {
            return None;
        }
        if self.list.filter_key(key) {
            let items = self.collections.data.as_deref().unwrap_or_default();
            self.list.sync(items, search_text);
        }
        Some(Outcome::Handled)
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        if !self.listing() {
            return self.saved.action(action, cx);
        }
        if self.list.navigate(action) {
            return Outcome::Handled;
        }
        match action {
            Action::Activate | Action::Open => return self.open(),
            Action::Filter => self.list.start_filter(),
            Action::Back if self.list.is_filtered() => {
                self.list.clear_filter();
                let items = self.collections.data.as_deref().unwrap_or_default();
                self.list.sync(items, search_text);
            }
            _ => return Outcome::Ignored,
        }
        Outcome::Handled
    }

    fn mouse(&mut self, event: MouseEvent, cx: &mut Ctx) -> Outcome {
        if !self.listing() {
            return self.saved.mouse(event, cx);
        }
        match self.list.mouse(event) {
            Click::Activated => self.open(),
            Click::Moved => Outcome::Handled,
            Click::Missed => Outcome::Ignored,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        if !self.listing() {
            // Until the collections arrive, or if there are none to list.
            self.saved.render(frame, area, cx);
            return;
        }
        let theme = cx.theme;
        if self.list.is_empty() && !self.list.is_typing() {
            ui::message(
                frame,
                area,
                vec![Line::styled("Nothing matches.", theme.muted())],
                theme.base(),
            );
            return;
        }
        let items = self.collections.data.as_deref().unwrap_or_default();
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
                let c = &items[i];
                let mark = if c.is_default { "★" } else { icon(&c.icon) };
                let description = match (&c.description, c.is_default) {
                    (Some(d), _) if !d.trim().is_empty() => d.clone(),
                    (_, true) => "Episodes you've saved".to_string(),
                    _ => String::new(),
                };
                let count = ui::episode_count(c.episode_count);
                vec![
                    Cell::from(Span::styled(mark, theme.accent())),
                    Cell::from(Text::from(vec![
                        Line::raw(truncate(&c.name, usize::from(w[1]))),
                        Line::styled(truncate(&description, usize::from(w[1])), theme.faint()),
                    ])),
                    Cell::from(Span::styled(count, theme.muted())),
                ]
            },
        );
    }

    fn selected_episode(&self) -> Option<Episode> {
        if self.listing() {
            None
        } else {
            self.saved.selected_episode()
        }
    }

    fn episodes(&self) -> Vec<&Episode> {
        self.saved.episodes()
    }
}
