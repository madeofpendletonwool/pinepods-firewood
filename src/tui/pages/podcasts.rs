//! Subscriptions. Opening one lists its episodes.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    crossterm::event::{KeyEvent, MouseEvent},
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Cell, Row},
};

use super::episodes::{self, EpisodeListPage};
use super::{Ctx, Outcome, Page};
use crate::api::Podcast;
use crate::tui::keymap::{Action, Context};
use crate::tui::task::{Remote, Resource};
use crate::tui::text::truncate;
use crate::tui::ui::{self, list::Click, list::ListView};

/// Pressing Unsubscribe again within this long goes ahead.
const CONFIRM_WINDOW: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct PodcastsPage {
    data: Remote<Vec<Podcast>>,
    list: ListView,
    /// The podcast Unsubscribe was first pressed on, and when.
    unsubscribe_armed: Option<(i64, Instant)>,
}

fn search_text(p: &Podcast) -> String {
    format!("{} {}", p.podcastname, p.author)
}

impl PodcastsPage {
    fn load(&mut self, cx: &Ctx) {
        let api = cx.api.clone();
        self.data.load(async move {
            let mut pods = api.get_podcasts().await?;
            pods.sort_by_key(|p| p.podcastname.to_lowercase());
            Ok(pods)
        });
    }

    fn items(&self) -> &[Podcast] {
        self.data.data.as_deref().unwrap_or_default()
    }

    fn open(&self) -> Outcome {
        let Some(podcast) = self.list.selected_index().and_then(|i| self.items().get(i)) else {
            return Outcome::Handled;
        };
        Outcome::Push(Box::new(EpisodeListPage::podcast(podcast.clone())))
    }

    /// The first press asks for a second, on the same podcast.
    fn unsubscribe(&mut self, cx: &Ctx) {
        let Some(index) = self.list.selected_index() else {
            return;
        };
        let Some(podcast) = self.items().get(index).cloned() else {
            return;
        };
        let armed = self
            .unsubscribe_armed
            .is_some_and(|(id, at)| id == podcast.podcastid && at.elapsed() <= CONFIRM_WINDOW);
        if !armed {
            self.unsubscribe_armed = Some((podcast.podcastid, Instant::now()));
            let key = cx
                .keymap
                .key_for(Action::Unsubscribe, self.contexts())
                .map(|k| k.to_string())
                .unwrap_or_default();
            cx.bus.info(format!(
                "Press {key} again to unsubscribe from {}",
                truncate(&podcast.podcastname, 40)
            ));
            return;
        }
        self.unsubscribe_armed = None;
        episodes::unsubscribe(&podcast, cx);
        if let Some(items) = self.data.data.as_mut() {
            items.retain(|p| p.podcastid != podcast.podcastid);
        }
        let items = self.data.data.as_deref().unwrap_or_default();
        self.list.sync(items, search_text);
    }
}

impl Page for PodcastsPage {
    fn title(&self) -> String {
        "Podcasts".into()
    }

    fn contexts(&self) -> &'static [Context] {
        &[Context::Podcast, Context::List]
    }

    fn hints(&self) -> &'static [Action] {
        &[Action::Activate, Action::Filter, Action::Unsubscribe]
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
        if resource == Resource::Podcasts {
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

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        if self.list.navigate(action) {
            return Outcome::Handled;
        }
        match action {
            Action::Activate | Action::Open => return self.open(),
            Action::Unsubscribe => self.unsubscribe(cx),
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
            "podcasts",
        ) {
            return;
        }
        if self.list.is_empty() && !self.list.is_typing() {
            let text = if self.list.is_filtered() {
                "Nothing matches."
            } else {
                "No subscriptions yet. Find podcasts to follow in Search (press f there)."
            };
            ui::message(
                frame,
                area,
                vec![Line::styled(text, theme.muted())],
                theme.base(),
            );
            return;
        }
        let wide = area.width >= 70;
        let mut widths = vec![Constraint::Fill(2)];
        if wide {
            widths.push(Constraint::Fill(1));
        }
        widths.push(Constraint::Length(10));
        let header = if wide {
            Row::new(["Podcast", "By", "Episodes"])
        } else {
            Row::new(["Podcast", "Episodes"])
        };
        let items = self.data.data.as_deref().unwrap_or_default();
        self.list
            .render(frame, area, theme, &widths, Some(header), |i, w| {
                let p = &items[i];
                let tag = if p.is_youtube { "  YouTube" } else { "" };
                let name_width = usize::from(w[0]).saturating_sub(tag.len());
                let mut cells = vec![Cell::from(Line::from(vec![
                    Span::raw(truncate(&p.podcastname, name_width)),
                    Span::styled(tag, theme.faint()),
                ]))];
                if wide {
                    cells.push(Cell::from(Span::styled(
                        truncate(&p.author, usize::from(w[1])),
                        theme.muted(),
                    )));
                }
                cells.push(Cell::from(Span::styled(
                    p.episodecount.to_string(),
                    theme.muted(),
                )));
                cells
            });
    }

    fn podcasts(&self) -> Vec<&Podcast> {
        self.items().iter().collect()
    }
}
