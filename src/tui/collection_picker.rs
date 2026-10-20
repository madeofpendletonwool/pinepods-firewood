//! "Add to a collection": a popup listing your collections with a check by the
//! ones that hold the episode. Enter adds it to or takes it out of the one selected.

use std::collections::HashSet;

use ratatui::{
    Frame,
    crossterm::event::MouseEvent,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Cell, Paragraph},
};

use super::keymap::Action;
use super::task::{Bus, Remote, Resource};
use super::text::truncate;
use super::ui::{self, list::Click, list::ListView};
use crate::api::{Api, Collection, Episode};
use crate::theme::Theme;

/// The collections, and which of them hold the episode.
type Membership = (Vec<Collection>, HashSet<i64>);

pub struct CollectionPicker {
    episode: Episode,
    data: Remote<Membership>,
    list: ListView,
}

impl CollectionPicker {
    pub fn new(episode: Episode, api: &Api) -> Self {
        let mut data = Remote::default();
        let (api, key) = (api.clone(), episode.key());
        data.load(async move {
            let (collections, holding) =
                tokio::join!(api.get_collections(), api.get_episode_collections(key));
            let mut collections = collections?;
            collections.sort_by_key(|c| !c.is_default);
            Ok((collections, holding?.into_iter().collect()))
        });
        Self {
            episode,
            data,
            list: ListView::default(),
        }
    }

    pub fn tick(&mut self, bus: &Bus) {
        if self.data.poll(bus) {
            self.sync();
        }
    }

    fn sync(&mut self) {
        let collections = self.data.data.as_ref().map(|(c, _)| c.as_slice());
        self.list
            .sync(collections.unwrap_or_default(), |c| c.name.clone());
    }

    pub fn navigate(&mut self, action: Action) {
        self.list.navigate(action);
    }

    pub fn mouse(&mut self, event: MouseEvent) -> Click {
        self.list.mouse(event)
    }

    /// Adds the episode to the selected collection, or takes it out. Shows at once;
    /// a refusal from the server comes back as a toast.
    pub fn toggle(&mut self, api: &Api, bus: &Bus) {
        let Some(index) = self.list.selected_index() else {
            return;
        };
        let Some((collections, holding)) = self.data.data.as_mut() else {
            return;
        };
        let Some(collection) = collections.get(index).cloned() else {
            return;
        };
        let id = collection.collection_id;
        let adding = !holding.contains(&id);
        if adding {
            holding.insert(id);
        } else {
            holding.remove(&id);
        }
        // Saved is the default collection, so lists of saved episodes change too.
        let affects: &[Resource] = if collection.is_default {
            &[Resource::Collections, Resource::Saved]
        } else {
            &[Resource::Collections]
        };
        let (api, key) = (api.clone(), self.episode.key());
        let name = truncate(&collection.name, 30);
        if adding {
            bus.run(
                format!("Added to {name}"),
                format!("Couldn't add to {name}"),
                affects,
                async move { api.add_to_collection(id, key).await },
            );
        } else {
            bus.run(
                format!("Removed from {name}"),
                format!("Couldn't remove from {name}"),
                affects,
                async move { api.remove_from_collection(id, key).await },
            );
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let count = self.data.data.as_ref().map_or(1, |(c, _)| c.len());
        let height = (count as u16).clamp(1, 10) + 4;
        let title = format!(
            "Collections ▸ {}",
            truncate(&self.episode.episode_title, 40)
        );
        let inner = ui::popup(frame, ui::centered(area, 64, height), &title, theme);
        let [rows, _, hint] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        if ui::load_state(
            frame,
            rows,
            theme,
            self.data.is_empty_loading(),
            self.data.error(),
            "collections",
        ) {
            return;
        }
        let Some((collections, holding)) = &self.data.data else {
            return;
        };
        self.list.render(
            frame,
            rows,
            theme,
            &[
                Constraint::Length(3),
                Constraint::Fill(1),
                Constraint::Length(12),
            ],
            None,
            |i, w| {
                let c = &collections[i];
                let check = if holding.contains(&c.collection_id) {
                    Span::styled("[✓]", theme.accent())
                } else {
                    Span::styled("[ ]", theme.faint())
                };
                vec![
                    Cell::from(check),
                    Cell::from(truncate(&c.name, usize::from(w[1]))),
                    Cell::from(Span::styled(
                        ui::episode_count(c.episode_count),
                        theme.muted(),
                    )),
                ]
            },
        );
        frame.render_widget(
            Paragraph::new(Line::styled(
                "Enter add or remove · Esc done",
                theme.faint(),
            )),
            hint,
        );
    }
}
