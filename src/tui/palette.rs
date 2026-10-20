//! The command palette (`:` or Ctrl+P): one fuzzy-matched list of what the page's
//! keys do, the sidebar entries, your podcasts, episodes already on screen somewhere
//! and recent searches. The app builds the entries; this keeps the query, ranks
//! the matches and draws them.

use std::collections::HashSet;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent},
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Cell, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::keymap::{Action, Context, Keymap};
use super::pages::Section;
use super::pages::search::RecentSearch;
use super::task::{Bus, Pending};
use super::text::truncate;
use super::ui::{self, list::Click, list::ListView};
use crate::api::{Api, ApiResult, Episode, Podcast};
use crate::theme::Theme;

const WIDTH: u16 = 84;
const MAX_ROWS: u16 = 14;
/// Past this many matches the rest aren't worth drawing.
const MAX_RESULTS: usize = 200;

/// What an entry does when run.
#[derive(Debug, Clone)]
pub enum Command {
    Action(Action),
    Go(Section),
    Search(RecentSearch),
    Podcast(Podcast),
    Play(Episode),
}

impl Command {
    /// Breaks ties between equally good matches, and orders the list before
    /// anything is typed.
    fn rank(&self) -> u8 {
        match self {
            // Moving the cursor is what keys are for; it's here to be complete.
            Self::Action(
                Action::Up
                | Action::Down
                | Action::PageUp
                | Action::PageDown
                | Action::Top
                | Action::Bottom,
            ) => 1,
            Self::Action(_) => 0,
            Self::Go(_) => 2,
            Self::Search(_) => 3,
            Self::Podcast(_) => 4,
            Self::Play(_) => 5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// What's matched and shown.
    pub label: String,
    /// Shown faintly on the right: a key, or where the entry comes from.
    pub detail: String,
    pub command: Command,
}

/// What the palette offers, gathered from the app when it opens.
#[derive(Default)]
pub struct Sources<'a> {
    /// The open page's contexts, most specific first.
    pub contexts: &'a [Context],
    /// Sidebar entries in the order shown.
    pub sections: &'a [Section],
    pub searches: &'a [RecentSearch],
    pub podcasts: Vec<&'a Podcast>,
    pub episodes: Vec<&'a Episode>,
}

/// Every action that does something on a page with `contexts`, with the key that
/// does it there, if any.
pub fn actions(keymap: &Keymap, contexts: &[Context]) -> Vec<(Action, Option<String>)> {
    let mut seen = HashSet::new();
    contexts
        .iter()
        .chain(std::iter::once(&Context::Global))
        .flat_map(|c| keymap.bindings(*c))
        .filter(|(action, _)| *action != Action::Palette && seen.insert(*action))
        .map(|(action, _)| {
            let key = keymap.key_for(*action, contexts).map(|k| k.to_string());
            (*action, key)
        })
        .collect()
}

pub fn entries(keymap: &Keymap, sources: Sources) -> Vec<Entry> {
    let mut out: Vec<Entry> = actions(keymap, sources.contexts)
        .into_iter()
        .map(|(action, key)| Entry {
            label: action.description().to_string(),
            detail: key.unwrap_or_default(),
            command: Command::Action(action),
        })
        .collect();
    for (i, section) in sources.sections.iter().enumerate() {
        let digit = match i {
            0..=8 => (i + 1).to_string(),
            9 => "0".into(),
            _ => String::new(),
        };
        out.push(Entry {
            label: format!("Go to {}", section.title()),
            detail: digit,
            command: Command::Go(*section),
        });
    }
    for search in sources.searches {
        out.push(Entry {
            label: search.label(),
            detail: "recent search".into(),
            command: Command::Search(search.clone()),
        });
    }
    let mut seen = HashSet::new();
    for podcast in sources.podcasts {
        if seen.insert(podcast.podcastid) {
            out.push(podcast_entry(podcast));
        }
    }
    let mut seen = HashSet::new();
    for episode in sources.episodes {
        if seen.insert(episode.key()) {
            out.push(Entry {
                label: format!("Play {}", episode.episode_title),
                detail: episode.podcast_name.clone().unwrap_or_default(),
                command: Command::Play(episode.clone()),
            });
        }
    }
    out
}

fn podcast_entry(podcast: &Podcast) -> Entry {
    Entry {
        label: podcast.podcastname.clone(),
        detail: "podcast".into(),
        command: Command::Podcast(podcast.clone()),
    }
}

/// A match: the entry's index and which characters of its label matched.
#[derive(Debug, Clone, PartialEq)]
struct Hit {
    index: usize,
    chars: Vec<u32>,
}

/// Ranks `entries` against `query`: best match first, ties broken by kind and then
/// by the order they came in. An empty query keeps them all, in that order.
///
/// The scoring is nucleo's (as in Helix): the query's characters must appear in
/// order, with bonuses for landing on word starts and for running together, and
/// each space-separated word is matched on its own.
fn rank(matcher: &mut Matcher, entries: &[Entry], query: &str) -> Vec<Hit> {
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, Hit)> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let haystack = Utf32Str::new(&entry.label, &mut buf);
            let mut chars = Vec::new();
            let score = pattern.indices(haystack, matcher, &mut chars)?;
            chars.sort_unstable();
            chars.dedup();
            Some((score, Hit { index, chars }))
        })
        .collect();
    scored.sort_by(|(a_score, a), (b_score, b)| {
        b_score
            .cmp(a_score)
            .then_with(|| {
                entries[a.index]
                    .command
                    .rank()
                    .cmp(&entries[b.index].command.rank())
            })
            .then_with(|| a.index.cmp(&b.index))
    });
    scored
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(_, hit)| hit)
        .collect()
}

/// What a key in the palette led to.
pub enum Outcome {
    Open,
    Close,
    Run(Box<Command>),
}

pub struct Palette {
    query: String,
    entries: Vec<Entry>,
    hits: Vec<Hit>,
    list: ListView,
    matcher: Matcher,
    /// Your podcasts, when no page had them loaded yet.
    podcasts: Pending<ApiResult<Vec<Podcast>>>,
    /// Where the popup was drawn, so a click on it doesn't count as outside.
    area: Rect,
}

impl Palette {
    /// `load_podcasts` fetches your podcasts in the background, for when no page has
    /// them; they join the list when they arrive.
    pub fn new(entries: Vec<Entry>, load_podcasts: Option<&Api>) -> Self {
        let podcasts = match load_podcasts {
            Some(api) => {
                let api = api.clone();
                Pending::spawn(async move { api.get_podcasts().await })
            }
            None => Pending::default(),
        };
        let mut palette = Self {
            query: String::new(),
            entries,
            hits: Vec::new(),
            list: ListView::default(),
            matcher: Matcher::new(Config::DEFAULT),
            podcasts,
            area: Rect::default(),
        };
        palette.rematch();
        palette
    }

    pub fn tick(&mut self, bus: &Bus) {
        match self.podcasts.poll() {
            Some(Ok(podcasts)) => {
                let known: HashSet<i64> = self
                    .entries
                    .iter()
                    .filter_map(|e| match &e.command {
                        Command::Podcast(p) => Some(p.podcastid),
                        _ => None,
                    })
                    .collect();
                // Keep podcasts ahead of episodes, so the order without a query holds.
                let at = self
                    .entries
                    .iter()
                    .position(|e| matches!(e.command, Command::Play(_)))
                    .unwrap_or(self.entries.len());
                let new: Vec<Entry> = podcasts
                    .iter()
                    .filter(|p| !known.contains(&p.podcastid))
                    .map(podcast_entry)
                    .collect();
                self.entries.splice(at..at, new);
                let selected = self.selected().map(|e| e.label.clone());
                self.rematch();
                if let Some(label) = selected
                    && let Some(i) = self
                        .hits
                        .iter()
                        .position(|h| self.entries[h.index].label == label)
                {
                    self.list.select_index(i);
                }
            }
            Some(Err(e)) => {
                tracing::warn!("couldn't load podcasts for the palette: {e}");
                bus.error(format!("Couldn't load your podcasts: {e}"));
            }
            None => {}
        }
    }

    fn rematch(&mut self) {
        self.hits = rank(&mut self.matcher, &self.entries, &self.query);
        self.list.sync(&self.hits, |_| String::new());
        self.list.navigate(Action::Top);
    }

    fn selected(&self) -> Option<&Entry> {
        let hit = self.hits.get(self.list.selected_index()?)?;
        self.entries.get(hit.index)
    }

    fn run_selected(&self) -> Outcome {
        match self.selected() {
            Some(entry) => Outcome::Run(Box::new(entry.command.clone())),
            None => Outcome::Open,
        }
    }

    /// The palette has the keyboard while it's open: typing edits the query, and
    /// movement keys that aren't characters move the selection.
    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Enter => return self.run_selected(),
            KeyCode::Up => self.list.move_by(-1),
            KeyCode::Down | KeyCode::Tab => self.list.move_by(1),
            KeyCode::BackTab => self.list.move_by(-1),
            KeyCode::Char('p' | 'k') if ctrl => self.list.move_by(-1),
            KeyCode::Char('n' | 'j') if ctrl => self.list.move_by(1),
            KeyCode::PageUp => {
                self.list.navigate(Action::PageUp);
            }
            KeyCode::PageDown => {
                self.list.navigate(Action::PageDown);
            }
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.rematch();
            }
            KeyCode::Backspace => {
                // Backspace on an empty query closes, as in most palettes.
                if self.query.pop().is_none() {
                    return Outcome::Close;
                }
                self.rematch();
            }
            KeyCode::Char(c) if !ctrl => {
                self.query.push(c);
                self.rematch();
            }
            _ => {}
        }
        Outcome::Open
    }

    pub fn paste(&mut self, text: &str) {
        self.query.extend(text.chars().filter(|c| !c.is_control()));
        self.rematch();
    }

    /// A click outside closes; clicking the selected row runs it.
    pub fn mouse(&mut self, event: MouseEvent, click: bool) -> Outcome {
        let inside = self
            .area
            .contains(ratatui::layout::Position::new(event.column, event.row));
        match self.list.mouse(event) {
            Click::Activated => self.run_selected(),
            Click::Missed if click && !inside => Outcome::Close,
            _ => Outcome::Open,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let rows = (self.hits.len() as u16).clamp(1, MAX_ROWS);
        let height = rows + 4;
        let width = WIDTH.min(area.width.saturating_sub(4));
        // High on the screen, so the list grows down from where you're looking.
        let rect = Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + (area.height.saturating_sub(height) / 4).min(4),
            width,
            height: height.min(area.height),
        };
        self.area = rect;
        let inner = ui::popup(frame, rect, "Commands", theme);
        let inner = Rect {
            x: inner.x + 1,
            width: inner.width.saturating_sub(2),
            ..inner
        };
        let [input, _, body, _] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(0),
        ])
        .areas(inner);

        let prompt = Span::styled("› ", theme.accent());
        let typed = if self.query.is_empty() {
            Span::styled("Type a command, page, podcast or episode…", theme.faint())
        } else {
            Span::styled(self.query.clone(), theme.base())
        };
        let count = if self.query.is_empty() {
            Span::default()
        } else {
            Span::styled(self.hits.len().to_string(), theme.faint())
        };
        let [text, counter] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(count.content.width() as u16),
        ])
        .areas(input);
        frame.render_widget(Paragraph::new(Line::from(vec![prompt, typed])), text);
        frame.render_widget(Paragraph::new(count), counter);
        let cursor = text.x + 2 + self.query.width() as u16;
        frame.set_cursor_position((cursor.min(text.right().saturating_sub(1)), text.y));

        if self.hits.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled("Nothing matches.", theme.muted())),
                body,
            );
            return;
        }
        let detail_width = self
            .hits
            .iter()
            .map(|h| self.entries[h.index].detail.width())
            .max()
            .unwrap_or(0)
            .clamp(4, 28) as u16;
        let (entries, hits) = (&self.entries, &self.hits);
        let highlight = theme.accent().add_modifier(Modifier::BOLD);
        self.list.render(
            frame,
            body,
            theme,
            &[Constraint::Fill(1), Constraint::Length(detail_width)],
            None,
            |i, w| {
                let hit = &hits[i];
                let entry = &entries[hit.index];
                let style = match entry.command {
                    Command::Action(_) => theme.key(),
                    _ => theme.faint(),
                };
                vec![
                    Cell::from(highlighted(
                        &entry.label,
                        &hit.chars,
                        usize::from(w[0]),
                        highlight,
                    )),
                    Cell::from(
                        Line::styled(truncate(&entry.detail, usize::from(w[1])), style)
                            .right_aligned(),
                    ),
                ]
            },
        );
    }
}

/// `text` cut to `width` columns, with the characters at `chars` (indexes into
/// its chars, sorted) in `style`.
fn highlighted(
    text: &str,
    chars: &[u32],
    width: usize,
    style: ratatui::style::Style,
) -> Line<'static> {
    let text = truncate(text, width);
    let mut spans: Vec<Span> = Vec::new();
    let mut run = String::new();
    let mut run_hit = false;
    let mut next = chars.iter().peekable();
    for (i, c) in text.chars().enumerate() {
        let hit = next.peek().is_some_and(|&&n| n as usize == i);
        if hit {
            next.next();
        }
        if hit != run_hit && !run.is_empty() {
            let done = std::mem::take(&mut run);
            spans.push(if run_hit {
                Span::styled(done, style)
            } else {
                Span::raw(done)
            });
        }
        run_hit = hit;
        run.push(c);
    }
    if !run.is_empty() {
        spans.push(if run_hit {
            Span::styled(run, style)
        } else {
            Span::raw(run)
        });
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &str, command: Command) -> Entry {
        Entry {
            label: label.into(),
            detail: String::new(),
            command,
        }
    }

    fn ranked(labels: &[&str], query: &str) -> Vec<String> {
        let entries: Vec<Entry> = labels
            .iter()
            .map(|l| entry(l, Command::Action(Action::Quit)))
            .collect();
        let mut matcher = Matcher::new(Config::DEFAULT);
        rank(&mut matcher, &entries, query)
            .into_iter()
            .map(|h| entries[h.index].label.clone())
            .collect()
    }

    #[test]
    fn matches_need_every_character_in_order() {
        let found = ranked(&["Add to queue", "Clear queue", "Quit"], "adq");
        assert_eq!(found, ["Add to queue"]);
        assert!(ranked(&["Quit"], "tiuq").is_empty());
    }

    #[test]
    fn word_starts_and_runs_beat_scattered_letters() {
        // "pn" lands on the starts of "Play next"; in "Open in a new tab" the
        // letters are scattered mid-word.
        let found = ranked(&["Open in a new tab", "Play next"], "pn");
        assert_eq!(found[0], "Play next");
        // A contiguous run beats the same letters spread out.
        let found = ranked(&["Search history", "Shuffle"], "shuf");
        assert_eq!(found[0], "Shuffle");
    }

    #[test]
    fn words_match_in_any_order_and_case_is_smart() {
        assert_eq!(ranked(&["Go to Queue"], "queue go"), ["Go to Queue"]);
        assert_eq!(ranked(&["Go to Queue"], "QUEUE"), Vec::<String>::new());
        assert_eq!(ranked(&["Go to Queue"], "Queue"), ["Go to Queue"]);
    }

    #[test]
    fn an_empty_query_keeps_everything_with_actions_first() {
        let entries = vec![
            entry("Go to Home", Command::Go(Section::Home)),
            entry("Quit", Command::Action(Action::Quit)),
        ];
        let mut matcher = Matcher::new(Config::DEFAULT);
        let hits = rank(&mut matcher, &entries, "");
        let order: Vec<usize> = hits.iter().map(|h| h.index).collect();
        assert_eq!(order, [1, 0]);
    }

    #[test]
    fn matched_characters_are_reported_for_highlighting() {
        let entries = vec![entry("Play next", Command::Action(Action::PlayNext))];
        let mut matcher = Matcher::new(Config::DEFAULT);
        let hits = rank(&mut matcher, &entries, "pn");
        assert_eq!(hits[0].chars, [0, 5]);
        let line = highlighted("Play next", &hits[0].chars, 40, Default::default());
        let pieces: Vec<&str> = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(pieces, ["P", "lay ", "n", "ext"]);
    }

    /// Every action can be run from the palette on some page, except opening the
    /// palette itself.
    #[test]
    fn every_action_is_reachable() {
        let keymap = Keymap::default();
        let mut reachable = HashSet::new();
        for context in Context::ALL {
            for (action, _) in actions(&keymap, &[context]) {
                reachable.insert(action);
            }
        }
        for action in Action::ALL {
            if action == Action::Palette {
                assert!(!reachable.contains(&action));
            } else {
                assert!(reachable.contains(&action), "{action:?} isn't offered");
            }
        }
    }

    #[test]
    fn actions_show_the_key_that_works_on_the_page() {
        let keymap = Keymap::default();
        let on_devices = actions(&keymap, &[Context::Devices]);
        let send = on_devices
            .iter()
            .find(|(a, _)| *a == Action::SendHere)
            .unwrap();
        assert_eq!(send.1.as_deref(), Some("s"));
        // Saving isn't something the Devices page does.
        assert!(on_devices.iter().all(|(a, _)| *a != Action::Save));
        let in_lists = actions(&keymap, &[Context::Episode, Context::List]);
        let open = in_lists.iter().find(|(a, _)| *a == Action::Open).unwrap();
        assert_eq!(open.1.as_deref(), Some("i"));
    }

    #[test]
    fn duplicates_are_listed_once() {
        let podcast = Podcast {
            podcastid: 1,
            podcastname: "Show".into(),
            ..Default::default()
        };
        let episode = Episode {
            episode_id: 7,
            episode_title: "Pilot".into(),
            ..Default::default()
        };
        let list = entries(
            &Keymap::default(),
            Sources {
                podcasts: vec![&podcast, &podcast],
                episodes: vec![&episode, &episode],
                ..Default::default()
            },
        );
        assert_eq!(
            list.iter()
                .filter(|e| matches!(e.command, Command::Podcast(_)))
                .count(),
            1
        );
        assert_eq!(
            list.iter()
                .filter(|e| matches!(e.command, Command::Play(_)))
                .count(),
            1
        );
    }
}
