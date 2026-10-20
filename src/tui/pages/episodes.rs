//! A plain list of episodes: the Feed, Saved, History, Downloads, a playlist, a
//! collection, and one podcast's episodes.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    crossterm::event::{KeyEvent, MouseEvent},
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::episode::EpisodePage;
use super::{Ctx, Outcome, Page};
use crate::api::{Collection, Episode, Playlist, Podcast};
use crate::tui::keymap::{Action, Context};
use crate::tui::task::{Remote, Resource};
use crate::tui::text::truncate;
use crate::tui::ui::episodes::{self, Columns};
use crate::tui::ui::list::{Click, ListView};
use crate::tui::ui::{self, today};

/// Pressing Unsubscribe again within this long goes ahead.
const CONFIRM_WINDOW: Duration = Duration::from_secs(3);

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
    /// What you've played, newest first.
    History,
    /// Episodes the server has a copy of.
    Downloads,
    Podcast(Box<Podcast>),
    /// Playing one carries on through the playlist rather than the queue.
    Playlist(Box<Playlist>),
    Collection(Box<Collection>),
}

pub struct EpisodeListPage {
    source: Source,
    data: Remote<Vec<Episode>>,
    /// Episodes passing the played/unplayed filter, which the list then filters by text.
    shown: Vec<Episode>,
    list: ListView,
    filter: Filter,
    /// When Unsubscribe was first pressed.
    unsubscribe_armed: Option<Instant>,
}

impl EpisodeListPage {
    fn new(source: Source) -> Self {
        Self {
            source,
            data: Remote::default(),
            shown: Vec::new(),
            list: ListView::default(),
            filter: Filter::All,
            unsubscribe_armed: None,
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

    pub fn history() -> Self {
        Self::new(Source::History)
    }

    pub fn podcast(podcast: Podcast) -> Self {
        Self::new(Source::Podcast(Box::new(podcast)))
    }

    pub fn playlist(playlist: Playlist) -> Self {
        Self::new(Source::Playlist(Box::new(playlist)))
    }

    pub fn collection(collection: Collection) -> Self {
        Self::new(Source::Collection(Box::new(collection)))
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
            Source::History => self.data.load(async move { api.get_history().await }),
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
                // A podcast's downloads together, newest first.
                episodes.sort_by(|a, b| {
                    let podcast =
                        |e: &Episode| e.podcast_name.clone().unwrap_or_default().to_lowercase();
                    podcast(a)
                        .cmp(&podcast(b))
                        .then_with(|| b.episode_pub_date.cmp(&a.episode_pub_date))
                });
                Ok(episodes)
            }),
            Source::Playlist(p) => {
                let id = p.playlist_id;
                self.data
                    .load(async move { api.get_playlist_episodes(id).await });
            }
            Source::Collection(c) => {
                let id = c.collection_id;
                self.data
                    .load(async move { api.get_collection_episodes(id).await });
            }
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
            Source::Feed => "No recent episodes. Find podcasts to follow in Search.",
            Source::Saved => "Nothing saved yet. Press s on an episode to save it.",
            Source::History => "Nothing played yet.",
            Source::Downloads => {
                "The server has no downloads. Press d on an episode to have it download one."
            }
            Source::Podcast(_) => "This podcast has no episodes yet.",
            Source::Playlist(_) => "Nothing in your podcasts fits this playlist right now.",
            Source::Collection(_) => {
                "This collection is empty. Press c on an episode to add it to one."
            }
        }
    }

    fn play(&self, episode: &Episode, cx: &Ctx) {
        match &self.source {
            Source::Playlist(p) => {
                if cx
                    .playback
                    .play_from_playlist(episode.clone(), p.playlist_id)
                    .is_err()
                {
                    cx.bus
                        .error("Playback has stopped working; restart Firewood");
                }
            }
            _ => episodes::play(episode, cx),
        }
    }

    /// The first press asks for a second; the second unsubscribes and closes the page.
    fn unsubscribe(&mut self, cx: &Ctx) -> Outcome {
        let Source::Podcast(podcast) = &self.source else {
            return Outcome::Ignored;
        };
        if self
            .unsubscribe_armed
            .is_none_or(|at| at.elapsed() > CONFIRM_WINDOW)
        {
            self.unsubscribe_armed = Some(Instant::now());
            let key = cx
                .keymap
                .key_for(Action::Unsubscribe, self.contexts())
                .map(|k| k.to_string())
                .unwrap_or_default();
            cx.bus.info(format!(
                "Press {key} again to unsubscribe from {}",
                truncate(&podcast.podcastname, 40)
            ));
            return Outcome::Handled;
        }
        self.unsubscribe_armed = None;
        unsubscribe(podcast, cx);
        Outcome::Pop
    }
}

/// Removes a subscription and everything the server keeps for it.
pub fn unsubscribe(podcast: &Podcast, cx: &Ctx) {
    let api = cx.api.clone();
    let podcast = podcast.clone();
    cx.bus.run(
        format!("Unsubscribed from {}", truncate(&podcast.podcastname, 40)),
        "Couldn't unsubscribe",
        &[Resource::Podcasts, Resource::Queue, Resource::Saved],
        async move { api.remove_podcast(&podcast).await },
    );
}

impl Page for EpisodeListPage {
    fn title(&self) -> String {
        match &self.source {
            Source::Feed => "Feed".into(),
            Source::Saved => "Saved".into(),
            Source::History => "History".into(),
            Source::Downloads => "Downloads".into(),
            Source::Podcast(p) => p.podcastname.clone(),
            Source::Playlist(p) => p.name.clone(),
            Source::Collection(c) => c.name.clone(),
        }
    }

    fn contexts(&self) -> &'static [Context] {
        match self.source {
            Source::Podcast(_) => &[Context::Podcast, Context::Episode, Context::List],
            _ => &[Context::Episode, Context::List],
        }
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
        let affected = match self.source {
            Source::Collection(_) => resource != Resource::Podcasts,
            _ => !matches!(resource, Resource::Podcasts | Resource::Collections),
        };
        if affected {
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
                    self.play(&e, cx);
                }
            }
            Action::Open => {
                if let Some(e) = self.selected_episode() {
                    return Outcome::Push(Box::new(EpisodePage::new(e)));
                }
            }
            Action::Unsubscribe => return self.unsubscribe(cx),
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
        let mut lines = vec![Line::from(spans)];
        // A playlist says what it picks, since that's not obvious from its name.
        if let Source::Playlist(p) = &self.source {
            lines.push(Line::styled(
                truncate(&p.description, usize::from(tabs.width)),
                theme.faint(),
            ));
        }
        frame.render_widget(Paragraph::new(lines), tabs);

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
