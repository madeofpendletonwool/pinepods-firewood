//! Search: the episodes of your subscriptions, as you type, or new podcasts to
//! follow from Podcast Index, iTunes and YouTube. `f` switches between them.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent},
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::discover::{self, Discover};
use super::episode::EpisodePage;
use super::{Ctx, Outcome, Page};
use crate::api::{Directory, Episode, Podcast};
use crate::tui::keymap::{Action, Context};
use crate::tui::task::{Remote, Resource};
use crate::tui::ui::episodes::{self, Columns};
use crate::tui::ui::list::{Click, ListView};
use crate::tui::ui::{self, today};

/// Typing pauses this long before a search goes out.
const DEBOUNCE: Duration = Duration::from_millis(350);
const MIN_QUERY: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Episodes of the podcasts you follow, from the server.
    Episodes,
    Find(discover::Source),
}

impl Mode {
    const ALL: [Mode; 4] = [
        Self::Episodes,
        Self::Find(discover::Source::Directory(Directory::PodcastIndex)),
        Self::Find(discover::Source::Directory(Directory::Itunes)),
        Self::Find(discover::Source::YouTube),
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Episodes => "Your podcasts",
            Self::Find(discover::Source::Directory(Directory::PodcastIndex)) => "Podcast Index",
            Self::Find(discover::Source::Directory(Directory::Itunes)) => "iTunes",
            Self::Find(discover::Source::YouTube) => "YouTube",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::Episodes => "Episode or podcast…",
            Self::Find(discover::Source::YouTube) => "Channel name, then Enter…",
            Self::Find(_) => "Podcast name, then Enter…",
        }
    }

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// A search that went out, for the command palette to offer again.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentSearch {
    mode: Mode,
    pub query: String,
}

impl RecentSearch {
    pub fn label(&self) -> String {
        match self.mode {
            Mode::Episodes => format!("Search for {}", self.query),
            mode => format!("Search {} for {}", mode.title(), self.query),
        }
    }

    /// Whether `self` was a step on the way to typing `later`, so only the
    /// finished query is kept.
    pub fn leads_to(&self, later: &RecentSearch) -> bool {
        self.mode == later.mode && later.query.starts_with(&self.query)
    }
}

pub struct SearchPage {
    mode: Mode,
    query: String,
    editing: bool,
    /// When the query last changed without being searched for.
    edited: Option<Instant>,
    /// The query the results are for.
    searched: String,
    results: Remote<Vec<Episode>>,
    list: ListView,
    discover: Discover,
    /// Search as soon as the page shows; set when it's opened to repeat a search.
    search_on_show: bool,
}

impl Default for SearchPage {
    fn default() -> Self {
        Self {
            mode: Mode::Episodes,
            query: String::new(),
            editing: true,
            edited: None,
            searched: String::new(),
            results: Remote::default(),
            list: ListView::default(),
            discover: Discover::default(),
            search_on_show: false,
        }
    }
}

impl SearchPage {
    /// The page with `recent` searched for again.
    pub fn repeat(recent: &RecentSearch) -> Self {
        Self {
            mode: recent.mode,
            query: recent.query.clone(),
            editing: false,
            search_on_show: true,
            ..Self::default()
        }
    }

    fn search(&mut self, cx: &Ctx) {
        self.edited = None;
        let term = self.query.trim().to_string();
        if let Mode::Find(source) = self.mode {
            if !term.is_empty() {
                self.remember(term, cx);
            }
            self.discover.search(source, &self.query, cx);
            return;
        }
        if term.chars().count() < MIN_QUERY || term == self.searched && !self.results.needs_load() {
            return;
        }
        self.remember(term.clone(), cx);
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

    fn remember(&self, query: String, cx: &Ctx) {
        cx.bus.searched(RecentSearch {
            mode: self.mode,
            query,
        });
    }

    fn items(&self) -> &[Episode] {
        self.results.data.as_deref().unwrap_or_default()
    }

    fn has_results(&self) -> bool {
        match self.mode {
            Mode::Episodes => !self.list.is_empty(),
            Mode::Find(_) => self.discover.has_results(),
        }
    }

    fn switch_mode(&mut self, cx: &Ctx) {
        self.mode = self.mode.next();
        if let Mode::Find(_) = self.mode {
            self.discover.show(cx);
            self.discover.forget();
            if !self.query.trim().is_empty() {
                self.search(cx);
            }
        } else {
            self.searched.clear();
            self.search(cx);
        }
    }

    fn render_modes(&self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        let mut spans = Vec::new();
        for mode in Mode::ALL {
            let style = if mode == self.mode {
                theme.active
            } else {
                theme.muted()
            };
            spans.push(Span::styled(format!(" {} ", mode.title()), style));
            spans.push(Span::raw(" "));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

impl Page for SearchPage {
    fn title(&self) -> String {
        "Search".into()
    }

    fn contexts(&self) -> &'static [Context] {
        match self.mode {
            Mode::Episodes => &[Context::Episode, Context::List],
            Mode::Find(_) => &[Context::Discover, Context::List],
        }
    }

    fn hints(&self) -> &'static [Action] {
        match self.mode {
            Mode::Episodes => &[
                Action::Filter,
                Action::Activate,
                Action::Open,
                Action::Queue,
                Action::CycleFilter,
            ],
            Mode::Find(_) => &[
                Action::Filter,
                Action::Subscribe,
                Action::Activate,
                Action::CycleFilter,
            ],
        }
    }

    fn show(&mut self, cx: &Ctx) {
        if let Mode::Find(_) = self.mode {
            self.discover.show(cx);
        }
        if std::mem::take(&mut self.search_on_show) {
            self.search(cx);
        }
    }

    fn tick(&mut self, cx: &Ctx) {
        if self.mode == Mode::Episodes && self.edited.is_some_and(|at| at.elapsed() >= DEBOUNCE) {
            self.search(cx);
        }
        if self.results.poll(cx.bus) {
            let items = self.results.data.as_deref().unwrap_or_default();
            self.list.sync(items, episodes::search_text);
            self.list.navigate(Action::Top);
        }
        self.discover.tick(cx);
    }

    fn refresh(&mut self, cx: &Ctx) {
        self.results.mark_stale();
        self.searched.clear();
        self.discover.forget();
        self.search(cx);
    }

    fn changed(&mut self, resource: Resource) {
        self.discover.changed(resource);
    }

    fn busy(&self) -> bool {
        self.results.is_loading() || self.discover.busy()
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
                if self.mode == Mode::Episodes || key.code == KeyCode::Enter {
                    self.search(cx);
                }
                if self.has_results() || key.code == KeyCode::Enter {
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
        match action {
            Action::Filter => {
                self.editing = true;
                return Outcome::Handled;
            }
            Action::CycleFilter => {
                self.switch_mode(cx);
                return Outcome::Handled;
            }
            _ => {}
        }
        if let Mode::Find(_) = self.mode {
            return self.discover.action(action, cx);
        }
        if self.list.navigate(action) {
            return Outcome::Handled;
        }
        match action {
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
        if let Mode::Find(_) = self.mode {
            let outcome = self.discover.mouse(event, cx);
            if !matches!(outcome, Outcome::Ignored) {
                self.editing = false;
            }
            return outcome;
        }
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
        let [input, modes, _, body] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
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
            Line::styled("Press / to search", theme.faint())
        } else if self.query.is_empty() {
            Line::styled(self.mode.placeholder(), theme.faint())
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
        self.render_modes(frame, modes, cx);

        if let Mode::Find(_) = self.mode {
            self.discover.set_focused(!self.editing);
            self.discover.render(frame, body, cx);
            return;
        }
        if self.searched.is_empty() {
            ui::message(
                frame,
                body,
                vec![Line::styled(
                    "Results come from the episodes of every podcast you follow. \
                     f finds new podcasts instead.",
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
        if self.mode != Mode::Episodes {
            return None;
        }
        self.list
            .selected_index()
            .and_then(|i| self.items().get(i))
            .cloned()
    }

    fn episodes(&self) -> Vec<&Episode> {
        self.items().iter().collect()
    }

    fn podcasts(&self) -> Vec<&Podcast> {
        self.discover.following()
    }
}
