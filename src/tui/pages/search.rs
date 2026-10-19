//! Searches the episodes of your subscriptions on the server as you type.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent},
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::episode::EpisodePage;
use super::{Ctx, Outcome, Page};
use crate::api::Episode;
use crate::tui::keymap::{Action, Context};
use crate::tui::task::{Remote, Resource};
use crate::tui::ui::episodes::{self, Columns};
use crate::tui::ui::list::{Click, ListView};
use crate::tui::ui::{self, today};

/// Typing pauses this long before a search goes out.
const DEBOUNCE: Duration = Duration::from_millis(350);
const MIN_QUERY: usize = 2;

pub struct SearchPage {
    query: String,
    editing: bool,
    /// When the query last changed without being searched for.
    edited: Option<Instant>,
    /// The query the results are for.
    searched: String,
    results: Remote<Vec<Episode>>,
    list: ListView,
}

impl Default for SearchPage {
    fn default() -> Self {
        Self {
            query: String::new(),
            editing: true,
            edited: None,
            searched: String::new(),
            results: Remote::default(),
            list: ListView::default(),
        }
    }
}

impl SearchPage {
    fn search(&mut self, cx: &Ctx) {
        self.edited = None;
        let term = self.query.trim().to_string();
        if term.chars().count() < MIN_QUERY || term == self.searched && !self.results.needs_load() {
            return;
        }
        self.searched = term.clone();
        let api = cx.api.clone();
        self.results.load(async move {
            // The server returns an episode once per match, so the same one can come
            // back several times.
            let mut seen = std::collections::HashSet::new();
            Ok(api
                .search(&term)
                .await?
                .into_iter()
                .map(|r| r.episode)
                .filter(|e| seen.insert(e.key()))
                .collect())
        });
    }

    fn items(&self) -> &[Episode] {
        self.results.data.as_deref().unwrap_or_default()
    }
}

impl Page for SearchPage {
    fn title(&self) -> String {
        "Search".into()
    }

    fn contexts(&self) -> &'static [Context] {
        &[Context::Episode, Context::List]
    }

    fn hints(&self) -> &'static [Action] {
        &[
            Action::Filter,
            Action::Activate,
            Action::Open,
            Action::Queue,
            Action::Save,
        ]
    }

    fn tick(&mut self, cx: &Ctx) {
        if self.edited.is_some_and(|at| at.elapsed() >= DEBOUNCE) {
            self.search(cx);
        }
        if self.results.poll(cx.bus) {
            let items = self.results.data.as_deref().unwrap_or_default();
            self.list.sync(items, episodes::search_text);
            self.list.navigate(Action::Top);
        }
    }

    fn refresh(&mut self, cx: &Ctx) {
        self.results.mark_stale();
        self.searched.clear();
        self.search(cx);
    }

    fn changed(&mut self, _resource: Resource) {}

    fn busy(&self) -> bool {
        self.results.is_loading()
    }

    fn typing(&self) -> bool {
        self.editing
    }

    fn key(&mut self, key: KeyEvent, cx: &mut Ctx) -> Option<Outcome> {
        if !self.editing {
            return None;
        }
        match key.code {
            KeyCode::Esc => {
                // First Esc clears, the second leaves the box.
                if self.query.is_empty() {
                    self.editing = false;
                }
                self.query.clear();
                self.edited = None;
            }
            KeyCode::Enter | KeyCode::Down | KeyCode::Tab => {
                self.search(cx);
                if !self.list.is_empty() || key.code == KeyCode::Enter {
                    self.editing = false;
                }
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.edited = Some(Instant::now());
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.clear();
            }
            // `/` is how you get here; don't start a search with it.
            KeyCode::Char('/') if self.query.is_empty() => {}
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.push(c);
                self.edited = Some(Instant::now());
            }
            _ => return None,
        }
        Some(Outcome::Handled)
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        if self.list.navigate(action) {
            return Outcome::Handled;
        }
        match action {
            Action::Filter => self.editing = true,
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
            other => {
                let index = self.list.selected_index();
                let Some(episode) =
                    index.and_then(|i| self.results.data.as_mut().and_then(|d| d.get_mut(i)))
                else {
                    return Outcome::Ignored;
                };
                if !episodes::apply(other, episode, cx) {
                    return Outcome::Ignored;
                }
            }
        }
        Outcome::Handled
    }

    fn mouse(&mut self, event: MouseEvent, cx: &mut Ctx) -> Outcome {
        match self.list.mouse(event) {
            Click::Activated => self.action(Action::Activate, cx),
            Click::Moved => {
                self.editing = false;
                Outcome::Handled
            }
            Click::Missed => Outcome::Ignored,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        let [input, _, body] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(area);

        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme.border(self.editing));
        let inner = block.inner(input);
        let text = if self.query.is_empty() && !self.editing {
            Line::styled("Press / to search your podcasts", theme.faint())
        } else if self.query.is_empty() {
            Line::styled("Episode or podcast…", theme.faint())
        } else {
            Line::from(Span::raw(self.query.clone()))
        };
        frame.render_widget(
            Paragraph::new(Line::from(
                [vec![Span::styled("⌕ ", theme.accent())], text.spans].concat(),
            ))
            .block(block),
            input,
        );
        if self.editing {
            let x = inner.x + 2 + self.query.width() as u16;
            frame.set_cursor_position((x.min(inner.right().saturating_sub(1)), inner.y));
        }

        if self.searched.is_empty() {
            ui::message(
                frame,
                body,
                vec![Line::styled(
                    "Results come from the episodes of every podcast you follow.",
                    theme.muted(),
                )],
                theme.base(),
            );
            return;
        }
        if ui::load_state(
            frame,
            body,
            theme,
            self.results.is_empty_loading(),
            self.results.error(),
            "results",
        ) {
            return;
        }
        if self.list.is_empty() {
            ui::message(
                frame,
                body,
                vec![Line::styled(
                    format!("Nothing matches “{}”.", self.searched),
                    theme.muted(),
                )],
                theme.base(),
            );
            return;
        }
        let columns = Columns::fit(body.width, true);
        let playing = cx.player.current_episode.as_ref().map(Episode::key);
        let today = today();
        let items = self.results.data.as_deref().unwrap_or_default();
        self.list.set_focused(!self.editing);
        self.list.render(
            frame,
            body,
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
            .cloned()
    }
}
