//! A plain list of episodes: the Feed, Saved, and one podcast's episodes.

use ratatui::{
    Frame,
    crossterm::event::{KeyEvent, MouseEvent},
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::episode::EpisodePage;
use super::{Ctx, Outcome, Page};
use crate::api::{Episode, Podcast};
use crate::tui::keymap::{Action, Context};
use crate::tui::task::{Remote, Resource};
use crate::tui::ui::episodes::{self, Columns};
use crate::tui::ui::list::{Click, ListView};
use crate::tui::ui::{self, today};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Unplayed,
    InProgress,
    Played,
}

impl Filter {
    const ALL: [Filter; 4] = [Self::All, Self::Unplayed, Self::InProgress, Self::Played];

    fn title(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Unplayed => "Unplayed",
            Self::InProgress => "In progress",
            Self::Played => "Played",
        }
    }

    fn matches(self, episode: &Episode) -> bool {
        let started = episode.listen_duration.unwrap_or(0) > 0;
        match self {
            Self::All => true,
            Self::Unplayed => !episode.completed && !started,
            Self::InProgress => !episode.completed && started,
            Self::Played => episode.completed,
        }
    }

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

enum Source {
    /// Recent episodes from every subscription.
    Feed,
    Saved,
    /// Episodes the server has a copy of.
    Downloads,
    Podcast(Box<Podcast>),
}

pub struct EpisodeListPage {
    source: Source,
    data: Remote<Vec<Episode>>,
    /// Episodes passing the played/unplayed filter, which the list then filters by text.
    shown: Vec<Episode>,
    list: ListView,
    filter: Filter,
}

impl EpisodeListPage {
    fn new(source: Source) -> Self {
        Self {
            source,
            data: Remote::default(),
            shown: Vec::new(),
            list: ListView::default(),
            filter: Filter::All,
        }
    }

    pub fn feed() -> Self {
        Self::new(Source::Feed)
    }

    pub fn saved() -> Self {
        Self::new(Source::Saved)
    }

    pub fn downloads() -> Self {
        Self::new(Source::Downloads)
    }

    pub fn podcast(podcast: Podcast) -> Self {
        Self::new(Source::Podcast(Box::new(podcast)))
    }

    fn load(&mut self, cx: &Ctx) {
        let api = cx.api.clone();
        match &self.source {
            Source::Feed => self
                .data
                .load(async move { api.get_recent_episodes().await }),
            Source::Saved => self.data.load(async move {
                let mut episodes = api.get_saved().await?;
                // The list doesn't always carry the flag it's a list of.
                for e in &mut episodes {
                    e.saved = true;
                }
                Ok(episodes)
            }),
            Source::Downloads => self.data.load(async move {
                let mut episodes: Vec<Episode> = api
                    .get_downloads()
                    .await?
                    .into_iter()
                    .map(|d| d.episode)
                    .collect();
                for e in &mut episodes {
                    e.downloaded = true;
                }
                Ok(episodes)
            }),
            Source::Podcast(p) => {
                let (id, name) = (p.podcastid, p.podcastname.clone());
                self.data.load(async move {
                    let mut episodes = api.get_podcast_episodes(id).await?;
                    for e in &mut episodes {
                        e.podcast_name.get_or_insert_with(|| name.clone());
                    }
                    Ok(episodes)
                })
            }
        }
    }

    fn rebuild(&mut self) {
        let filter = self.filter;
        self.shown = self
            .data
            .data
            .iter()
            .flatten()
            .filter(|e| filter.matches(e))
            .cloned()
            .collect();
        self.list.sync(&self.shown, episodes::search_text);
    }

    fn selected_mut(&mut self) -> Option<&mut Episode> {
        let index = self.list.selected_index()?;
        self.shown.get_mut(index)
    }

    /// Mirrors a change made to a shown episode back into the loaded data, so it
    /// survives the next filter change.
    fn write_back(&mut self) {
        let Some(changed) = self.list.selected_index().and_then(|i| self.shown.get(i)) else {
            return;
        };
        if let Some(original) = self
            .data
            .data
            .iter_mut()
            .flatten()
            .find(|e| e.key() == changed.key())
        {
            *original = changed.clone();
        }
    }

    fn empty_text(&self) -> &'static str {
        if self.list.is_filtered() || self.filter != Filter::All {
            return "Nothing matches.";
        }
        match self.source {
            Source::Feed => "No recent episodes. Subscribe to podcasts in the web app.",
            Source::Saved => "Nothing saved yet. Press s on an episode to save it.",
            Source::Downloads => {
                "The server has no downloads. Press d on an episode to have it download one."
            }
            Source::Podcast(_) => "This podcast has no episodes yet.",
        }
    }
}

impl Page for EpisodeListPage {
    fn title(&self) -> String {
        match &self.source {
            Source::Feed => "Feed".into(),
            Source::Saved => "Saved".into(),
            Source::Downloads => "Downloads".into(),
            Source::Podcast(p) => p.podcastname.clone(),
        }
    }

    fn contexts(&self) -> &'static [Context] {
        &[Context::Episode, Context::List]
    }

    fn hints(&self) -> &'static [Action] {
        &[
            Action::Activate,
            Action::Open,
            Action::Queue,
            Action::Save,
            Action::CycleFilter,
            Action::Filter,
        ]
    }

    fn show(&mut self, cx: &Ctx) {
        if self.data.needs_load() {
            self.load(cx);
        }
    }

    fn tick(&mut self, cx: &Ctx) {
        if self.data.poll(cx.bus) {
            self.rebuild();
        }
    }

    fn refresh(&mut self, cx: &Ctx) {
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
            self.list.sync(&self.shown, episodes::search_text);
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
                self.list.sync(&self.shown, episodes::search_text);
            }
            Action::CycleFilter => {
                self.filter = self.filter.next();
                self.rebuild();
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
            other => {
                let Some(episode) = self.selected_mut() else {
                    return Outcome::Ignored;
                };
                if !episodes::apply(other, episode, cx) {
                    return Outcome::Ignored;
                }
                self.write_back();
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
        let [tabs, body] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
        let mut spans = Vec::new();
        for filter in Filter::ALL {
            let style = if filter == self.filter {
                theme.active
            } else {
                theme.muted()
            };
            spans.push(Span::styled(format!(" {} ", filter.title()), style));
            spans.push(Span::raw(" "));
        }
        if let Some(all) = &self.data.data {
            spans.push(Span::styled(
                format!(" {} of {}", self.shown.len(), all.len()),
                theme.faint(),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), tabs);

        if ui::load_state(
            frame,
            body,
            theme,
            self.data.is_empty_loading(),
            self.data.error(),
            "episodes",
        ) {
            return;
        }
        if self.list.is_empty() && !self.list.is_typing() {
            ui::message(
                frame,
                body,
                vec![Line::styled(self.empty_text(), theme.muted())],
                theme.base(),
            );
            return;
        }

        let with_podcast = !matches!(self.source, Source::Podcast(_));
        let columns = Columns::fit(body.width, with_podcast);
        let playing = cx.player.current_episode.as_ref().map(Episode::key);
        let today = today();
        let shown = &self.shown;
        self.list.render(
            frame,
            body,
            theme,
            &columns.widths(),
            Some(columns.header(theme)),
            |i, w| columns.cells(&shown[i], playing, theme, today, w),
        );
    }

    fn selected_episode(&self) -> Option<Episode> {
        self.list
            .selected_index()
            .and_then(|i| self.shown.get(i))
            .cloned()
    }
}
