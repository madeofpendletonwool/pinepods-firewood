//! Up next. Reordering shows at once and reaches the server shortly after the last
//! move, as one request.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    crossterm::event::{KeyEvent, MouseEvent},
    layout::Rect,
    text::Line,
};

use super::episode::EpisodePage;
use super::{Ctx, Outcome, Page};
use crate::api::{Episode, EpisodeKey, QueueItem};
use crate::tui::keymap::{Action, Context};
use crate::tui::task::{Remote, Resource};
use crate::tui::text::truncate;
use crate::tui::ui::episodes::{self, Columns};
use crate::tui::ui::list::{Click, ListView};
use crate::tui::ui::{self, today};

/// Moves made within this long of each other go to the server together.
const REORDER_SETTLE: Duration = Duration::from_millis(700);

/// Pressing Clear again within this long empties the queue.
const CONFIRM_WINDOW: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct QueuePage {
    data: Remote<Vec<QueueItem>>,
    list: ListView,
    /// When the order last changed on screen without being sent.
    reordered: Option<Instant>,
    /// When Clear was first pressed.
    clear_armed: Option<Instant>,
}

fn search_text(item: &QueueItem) -> String {
    episodes::search_text(item)
}

impl QueuePage {
    fn load(&mut self, cx: &Ctx) {
        let api = cx.api.clone();
        self.data.load(async move {
            let mut queue = api.get_queue().await?;
            queue.sort_by_key(|q| q.queue_position.unwrap_or(i64::MAX));
            for item in &mut queue {
                item.episode.queued = true;
            }
            Ok(queue)
        });
    }

    fn items(&self) -> &[QueueItem] {
        self.data.data.as_deref().unwrap_or_default()
    }

    fn sync(&mut self) {
        let items = self.data.data.as_deref().unwrap_or_default();
        self.list.sync(items, search_text);
    }

    /// Moves the selected item to `to` (clamped), keeping it selected.
    fn move_selected(&mut self, cx: &Ctx, to: impl FnOnce(usize, usize) -> usize) {
        if self.list.is_filtered() {
            cx.bus.info("Clear the filter to reorder");
            return;
        }
        let Some(from) = self.list.selected_index() else {
            return;
        };
        let Some(items) = self.data.data.as_mut() else {
            return;
        };
        let to = to(from, items.len()).min(items.len().saturating_sub(1));
        if to == from {
            return;
        }
        let item = items.remove(from);
        items.insert(to, item);
        self.reordered = Some(Instant::now());
        self.sync();
        self.list.select_index(to);
    }

    fn send_order(&mut self, cx: &Ctx) {
        self.reordered = None;
        let order: Vec<EpisodeKey> = self.items().iter().map(|q| q.key()).collect();
        let api = cx.api.clone();
        cx.bus.run(
            "",
            "Couldn't reorder the queue",
            &[Resource::Queue],
            async move { api.reorder_queue(&order).await },
        );
    }

    fn remove_selected(&mut self, cx: &Ctx) {
        let Some(index) = self.list.selected_index() else {
            return;
        };
        let Some(items) = self.data.data.as_mut() else {
            return;
        };
        let item = items.remove(index);
        self.sync();
        let api = cx.api.clone();
        let key = item.key();
        cx.bus.run(
            format!("Removed {}", truncate(&item.episode_title, 40)),
            "Couldn't remove it",
            &[Resource::Queue],
            async move { api.remove_from_queue(key).await },
        );
    }

    fn clear(&mut self, cx: &Ctx) {
        if self.items().is_empty() {
            return;
        }
        if self
            .clear_armed
            .is_none_or(|at| at.elapsed() > CONFIRM_WINDOW)
        {
            self.clear_armed = Some(Instant::now());
            let key = cx
                .keymap
                .key_for(Action::ClearQueue, self.contexts())
                .map(|k| k.to_string())
                .unwrap_or_default();
            cx.bus
                .info(format!("Press {key} again to clear the whole queue"));
            return;
        }
        self.clear_armed = None;
        if let Some(items) = self.data.data.as_mut() {
            items.clear();
        }
        self.sync();
        let api = cx.api.clone();
        cx.bus.run(
            "Cleared the queue",
            "Couldn't clear the queue",
            &[Resource::Queue],
            async move { api.clear_queue().await },
        );
    }

    fn shuffle(&mut self, cx: &Ctx) {
        if self.list.is_filtered() {
            cx.bus.info("Clear the filter to reorder");
            return;
        }
        if let Some(items) = self.data.data.as_mut() {
            shuffle(items);
        }
        self.sync();
        self.list.navigate(Action::Top);
        self.send_order(cx);
    }
}

/// Fisher–Yates with std's randomly seeded hasher, to avoid a dependency for one call.
fn shuffle<T>(items: &mut [T]) {
    use std::hash::{BuildHasher, Hasher};
    let state = std::collections::hash_map::RandomState::new();
    for i in (1..items.len()).rev() {
        let mut hasher = state.build_hasher();
        hasher.write_usize(i);
        let j = (hasher.finish() % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

impl Page for QueuePage {
    fn title(&self) -> String {
        "Queue".into()
    }

    fn contexts(&self) -> &'static [Context] {
        &[Context::Queue, Context::Episode, Context::List]
    }

    fn hints(&self) -> &'static [Action] {
        &[
            Action::Activate,
            Action::MoveUp,
            Action::MoveDown,
            Action::Remove,
            Action::Open,
            Action::Filter,
        ]
    }

    fn show(&mut self, cx: &Ctx) {
        // Don't reload over moves that haven't been sent.
        if self.data.needs_load() && self.reordered.is_none() {
            self.load(cx);
        }
    }

    fn tick(&mut self, cx: &Ctx) {
        if self.data.poll(cx.bus) {
            self.sync();
        }
        if self
            .reordered
            .is_some_and(|at| at.elapsed() >= REORDER_SETTLE)
        {
            self.send_order(cx);
        }
    }

    fn refresh(&mut self, cx: &Ctx) {
        if self.reordered.is_some() {
            self.send_order(cx);
        }
        self.load(cx);
    }

    fn changed(&mut self, resource: Resource) {
        if resource != Resource::Podcasts {
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
            self.sync();
        }
        Some(Outcome::Handled)
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        if self.list.navigate(action) {
            return Outcome::Handled;
        }
        match action {
            Action::Filter => self.list.start_filter(),
            Action::Back if self.list.is_filtered() => {
                self.list.clear_filter();
                self.sync();
            }
            Action::Activate => {
                if let Some(e) = self.selected_episode() {
                    episodes::play(&e, cx);
                }
            }
            Action::Open => {
                if let Some(e) = self.selected_episode() {
                    return Outcome::Push(Box::new(EpisodePage::new(e)));
                }
            }
            Action::MoveUp => self.move_selected(cx, |i, _| i.saturating_sub(1)),
            Action::MoveDown => self.move_selected(cx, |i, _| i + 1),
            Action::MoveToTop => self.move_selected(cx, |_, _| 0),
            Action::MoveToBottom => self.move_selected(cx, |_, len| len),
            Action::Remove => self.remove_selected(cx),
            Action::ClearQueue => self.clear(cx),
            Action::Shuffle => self.shuffle(cx),
            Action::Queue => self.remove_selected(cx),
            other => {
                let Some(index) = self.list.selected_index() else {
                    return Outcome::Ignored;
                };
                let Some(item) = self.data.data.as_mut().and_then(|d| d.get_mut(index)) else {
                    return Outcome::Ignored;
                };
                if !episodes::apply(other, &mut item.episode, cx) {
                    return Outcome::Ignored;
                }
            }
        }
        Outcome::Handled
    }

    fn mouse(&mut self, event: MouseEvent, cx: &mut Ctx) -> Outcome {
        match self.list.mouse(event) {
            Click::Activated => self.action(Action::Activate, cx),
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
            "the queue",
        ) {
            return;
        }
        if self.list.is_empty() && !self.list.is_typing() {
            let text = if self.list.is_filtered() {
                "Nothing matches."
            } else {
                "The queue is empty. Press a on any episode to add it."
            };
            ui::message(
                frame,
                area,
                vec![Line::styled(text, theme.muted())],
                theme.base(),
            );
            return;
        }
        let columns = Columns::fit(area.width, true);
        let playing = cx.player.current_episode.as_ref().map(Episode::key);
        let today = today();
        let items = self.data.data.as_deref().unwrap_or_default();
        self.list.render(
            frame,
            area,
            theme,
            &columns.widths(),
            Some(columns.header(theme)),
            |i, w| columns.cells(&items[i], playing, theme, today, w),
        );
    }

    fn selected_episode(&self) -> Option<Episode> {
        self.list
            .selected_index()
            .and_then(|i| self.items().get(i))
            .map(|q| q.episode.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_keeps_every_item() {
        let mut items: Vec<u32> = (0..50).collect();
        shuffle(&mut items);
        let mut sorted = items.clone();
        sorted.sort();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
    }
}
