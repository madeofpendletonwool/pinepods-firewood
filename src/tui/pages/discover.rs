//! Finding podcasts to follow: Search's directory modes and a result's own page.
//!
//! Podcast Index and iTunes go through the server's `proxy_search`; YouTube channels
//! through its yt-dlp search, which takes a few seconds. Both search when Enter is
//! pressed rather than as you type, since each search reaches an outside service.

use std::collections::HashSet;

use ratatui::{
    Frame,
    crossterm::event::MouseEvent,
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span, Text},
    widgets::{Cell, Paragraph, Wrap},
};

use super::{Ctx, Outcome, Page};
use crate::api::{Directory, DirectoryPodcast, EpisodeKey, Podcast, YouTubeChannel};
use crate::tui::keymap::{Action, Context};
use crate::tui::task::{Remote, Resource};
use crate::tui::text::truncate;
use crate::tui::ui::list::{Click, ListView};
use crate::tui::ui::notes::NotesView;
use crate::tui::ui::{self};

/// Where Search looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Directory(Directory),
    YouTube,
}

/// A search result.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    Podcast(DirectoryPodcast),
    Channel(YouTubeChannel),
}

impl Found {
    pub fn name(&self) -> &str {
        match self {
            Self::Podcast(p) => &p.title,
            Self::Channel(c) => &c.name,
        }
    }

    fn byline(&self) -> String {
        match self {
            Self::Podcast(p) => p.author.clone(),
            Self::Channel(c) => c
                .recent_videos
                .first()
                .map(|v| format!("Latest: {}", v.title))
                .unwrap_or_default(),
        }
    }

    /// Whether `podcast` is this result, already followed.
    fn is(&self, podcast: &Podcast) -> bool {
        match self {
            Self::Podcast(p) => {
                same_feed(&podcast.feedurl, &p.feed_url)
                    || (p.index_id.is_some() && podcast.podcastindexid == p.index_id)
            }
            Self::Channel(c) => podcast.is_youtube && podcast.feedurl.contains(&c.channel_id),
        }
    }

    /// What identifies it for "subscribed this session".
    fn id(&self) -> String {
        match self {
            Self::Podcast(p) => normalize_feed(&p.feed_url),
            Self::Channel(c) => c.channel_id.clone(),
        }
    }
}

/// Feed URLs as written in different places differ in scheme and trailing slash.
fn normalize_feed(url: &str) -> String {
    url.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_lowercase()
}

fn same_feed(a: &str, b: &str) -> bool {
    !a.is_empty() && normalize_feed(a) == normalize_feed(b)
}

/// Subscribes, then tells every page the podcast list changed. The server fetches
/// the episodes in the background, so they appear over the next minute or so.
pub fn subscribe(found: &Found, cx: &Ctx) {
    let api = cx.api.clone();
    let name = truncate(found.name(), 40);
    let found = found.clone();
    cx.bus.run(
        format!("Subscribed to {name}; its episodes are on their way"),
        "Couldn't subscribe",
        &[Resource::Podcasts],
        async move {
            match &found {
                Found::Podcast(p) => api.add_podcast(p).await.map(drop),
                Found::Channel(c) => api.subscribe_youtube(&c.channel_id).await.map(drop),
            }
        },
    );
}

/// Search's results in a directory mode.
pub struct Discover {
    results: Remote<Vec<Found>>,
    list: ListView,
    /// What you follow already, to mark it.
    following: Remote<Vec<Podcast>>,
    /// Subscribed to from here, before the podcast list has caught up.
    added: HashSet<String>,
    /// The search the results are for, to say what found nothing.
    searched: Option<(Source, String)>,
}

impl Default for Discover {
    fn default() -> Self {
        Self {
            results: Remote::default(),
            list: ListView::default().with_row_height(2),
            following: Remote::default(),
            added: HashSet::new(),
            searched: None,
        }
    }
}

impl Discover {
    pub fn search(&mut self, source: Source, query: &str, cx: &Ctx) {
        let query = query.trim().to_string();
        if query.is_empty() || self.searched == Some((source, query.clone())) {
            return;
        }
        self.searched = Some((source, query.clone()));
        let api = cx.api.clone();
        // A new search replaces whatever was running.
        self.results = Remote::default();
        self.results.load(async move {
            Ok(match source {
                Source::Directory(directory) => api
                    .search_directory(&query, directory)
                    .await?
                    .into_iter()
                    .map(Found::Podcast)
                    .collect(),
                Source::YouTube => api
                    .search_youtube(&query)
                    .await?
                    .into_iter()
                    .map(Found::Channel)
                    .collect(),
            })
        });
        self.list = ListView::default().with_row_height(2);
    }

    /// Searches again next time, as with a changed query.
    pub fn forget(&mut self) {
        self.searched = None;
    }

    pub fn show(&mut self, cx: &Ctx) {
        if self.following.needs_load() {
            let api = cx.api.clone();
            self.following.load(async move { api.get_podcasts().await });
        }
    }

    pub fn tick(&mut self, cx: &Ctx) {
        self.following.poll(cx.bus);
        if self.results.poll(cx.bus) {
            let items = self.results.data.as_deref().unwrap_or_default();
            self.list.sync(items, |f| f.name().to_string());
        }
    }

    pub fn changed(&mut self, resource: Resource) {
        if resource == Resource::Podcasts {
            self.following.mark_stale();
        }
    }

    pub fn busy(&self) -> bool {
        self.results.is_loading()
    }

    pub fn has_results(&self) -> bool {
        !self.list.is_empty()
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.list.set_focused(focused);
    }

    fn follows(&self, found: &Found) -> bool {
        self.added.contains(&found.id())
            || self.following.data.iter().flatten().any(|p| found.is(p))
    }

    fn selected(&self) -> Option<&Found> {
        let i = self.list.selected_index()?;
        self.results.data.as_ref()?.get(i)
    }

    pub fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        if self.list.navigate(action) {
            return Outcome::Handled;
        }
        let Some(found) = self.selected().cloned() else {
            return Outcome::Ignored;
        };
        match action {
            Action::Activate | Action::Open => {
                let follows = self.follows(&found);
                Outcome::Push(Box::new(FoundPage::new(found, follows)))
            }
            Action::Subscribe => {
                if self.follows(&found) {
                    cx.bus
                        .info(format!("You follow {} already", truncate(found.name(), 40)));
                } else {
                    self.added.insert(found.id());
                    subscribe(&found, cx);
                }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }

    pub fn mouse(&mut self, event: MouseEvent, cx: &mut Ctx) -> Outcome {
        match self.list.mouse(event) {
            Click::Activated => self.action(Action::Activate, cx),
            Click::Moved => Outcome::Handled,
            Click::Missed => Outcome::Ignored,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        let Some((source, query)) = self.searched.clone() else {
            let text = "Type a name and press Enter to search. f switches where to look.";
            ui::message(
                frame,
                area,
                vec![Line::styled(text, theme.muted())],
                theme.base(),
            );
            return;
        };
        let what = match source {
            Source::YouTube => "channels (this takes a few seconds)",
            Source::Directory(_) => "podcasts",
        };
        if ui::load_state(
            frame,
            area,
            theme,
            self.results.is_empty_loading(),
            self.results.error(),
            what,
        ) {
            return;
        }
        if self.list.is_empty() {
            ui::message(
                frame,
                area,
                vec![Line::styled(
                    format!("Nothing found for “{query}”."),
                    theme.muted(),
                )],
                theme.base(),
            );
            return;
        }
        let items = self.results.data.clone().unwrap_or_default();
        let follows: Vec<bool> = items.iter().map(|f| self.follows(f)).collect();
        self.list.render(
            frame,
            area,
            theme,
            &[Constraint::Fill(1), Constraint::Length(14)],
            None,
            |i, w| {
                let found = &items[i];
                let right = if follows[i] {
                    Span::styled("✓ following", theme.accent())
                } else {
                    match found {
                        Found::Podcast(p) if p.episode_count > 0 => {
                            Span::styled(ui::episode_count(p.episode_count), theme.muted())
                        }
                        _ => Span::raw(""),
                    }
                };
                vec![
                    Cell::from(Text::from(vec![
                        Line::raw(truncate(found.name(), usize::from(w[0]))),
                        Line::styled(truncate(&found.byline(), usize::from(w[0])), theme.faint()),
                    ])),
                    Cell::from(right),
                ]
            },
        );
    }
}

/// One search result: what it is, and Subscribe.
pub struct FoundPage {
    found: Found,
    following: bool,
    notes: NotesView,
}

impl FoundPage {
    fn new(found: Found, following: bool) -> Self {
        Self {
            found,
            following,
            notes: NotesView::default(),
        }
    }
}

impl Page for FoundPage {
    fn title(&self) -> String {
        self.found.name().to_string()
    }

    fn contexts(&self) -> &'static [Context] {
        &[Context::Discover, Context::List]
    }

    fn hints(&self) -> &'static [Action] {
        &[Action::Subscribe, Action::Back]
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        match action {
            Action::Subscribe if self.following => {
                cx.bus.info("You follow this already");
            }
            Action::Subscribe => {
                self.following = true;
                subscribe(&self.found, cx);
            }
            Action::Back => return Outcome::Pop,
            other if self.notes.action(other) => {}
            _ => return Outcome::Ignored,
        }
        Outcome::Handled
    }

    fn mouse(&mut self, event: MouseEvent, _cx: &mut Ctx) -> Outcome {
        if self.notes.mouse(event) {
            Outcome::Handled
        } else {
            Outcome::Ignored
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        let mut head = vec![Line::styled(
            self.found.name().to_string(),
            theme.heading().add_modifier(Modifier::BOLD),
        )];
        let description = match &self.found {
            Found::Podcast(p) => {
                let mut facts = Vec::new();
                if p.episode_count > 0 {
                    facts.push(ui::episode_count(p.episode_count));
                }
                let categories: Vec<&str> = p.categories.values().map(String::as_str).collect();
                if !categories.is_empty() {
                    facts.push(categories.join(", "));
                }
                if p.explicit {
                    facts.push("explicit".into());
                }
                head.push(Line::styled(p.author.clone(), theme.accent()));
                head.push(Line::styled(facts.join(" · "), theme.muted()));
                head.push(Line::styled(p.feed_url.clone(), theme.faint()));
                p.description.clone()
            }
            Found::Channel(c) => {
                head.push(Line::styled("YouTube channel", theme.accent()));
                head.push(Line::styled(c.url.clone(), theme.faint()));
                let mut text = c.description.clone();
                if !c.recent_videos.is_empty() {
                    text.push_str("<p><b>Recent videos</b></p><ul>");
                    for video in &c.recent_videos {
                        text.push_str(&format!("<li>{}</li>", html_escape(&video.title)));
                    }
                    text.push_str("</ul>");
                }
                text
            }
        };
        head.push(if self.following {
            Line::styled("✓ You follow this", theme.accent())
        } else {
            let key = cx
                .keymap
                .key_for(Action::Subscribe, self.contexts())
                .map(|k| k.to_string())
                .unwrap_or_default();
            Line::from(vec![
                Span::styled(key, theme.key()),
                Span::styled(" to subscribe", theme.muted()),
            ])
        });

        let height = head.len() as u16;
        let [head_area, _, notes_area] = Layout::vertical([
            Constraint::Length(height),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(head).wrap(Wrap { trim: true }), head_area);
        if description.trim().is_empty() {
            ui::message(
                frame,
                notes_area,
                vec![Line::styled("No description.", theme.muted())],
                theme.base(),
            );
            return;
        }
        // The notes view caches by episode; any fixed key does for one page.
        let key = EpisodeKey {
            episode_id: 0,
            is_youtube: false,
        };
        self.notes
            .render(frame, notes_area, key, &description, theme, theme.base());
    }
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_followed_feed_matches_despite_scheme_and_slash() {
        let found = Found::Podcast(DirectoryPodcast {
            feed_url: "https://Feeds.example.com/show/".into(),
            ..Default::default()
        });
        let podcast = Podcast {
            feedurl: "http://feeds.example.com/show".into(),
            ..Default::default()
        };
        assert!(found.is(&podcast));
        assert!(!found.is(&Podcast::default()));
    }

    #[test]
    fn a_channel_matches_its_podcast_by_id() {
        let found = Found::Channel(YouTubeChannel {
            channel_id: "UC123".into(),
            name: String::new(),
            description: String::new(),
            url: String::new(),
            recent_videos: Vec::new(),
        });
        let podcast = Podcast {
            feedurl: "https://www.youtube.com/channel/UC123".into(),
            is_youtube: true,
            ..Default::default()
        };
        assert!(found.is(&podcast));
    }
}
