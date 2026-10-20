//! An episode's show notes, chapters and transcript in one pane, switched with
//! `v`. Now Playing and an episode's details page both show it.
//!
//! Chapters and the list of transcripts come from the feed's Podcasting 2.0 tags,
//! which the server reads (and whose chapters file it fetches). A transcript itself
//! is only fetched when its tab is opened. With a playback position, the current
//! chapter is marked and a timed transcript follows along; Enter jumps to the
//! selected chapter or line.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    crossterm::event::{MouseButton, MouseEvent, MouseEventKind},
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph},
};

use super::list::{Click, ListView};
use super::notes::NotesView;
use super::transcript::{self, Cue, Transcript};
use super::{clock, message, spinner};
use crate::api::{Chapter, Episode, EpisodeKey, Podcasting2, StoredTranscript, TranscriptLink};
use crate::theme::Theme;
use crate::tui::keymap::Action;
use crate::tui::pages::Ctx;
use crate::tui::task::Remote;
use crate::tui::text::{truncate, wrap};

/// Moving through a transcript by hand stops it following playback for this long.
const MANUAL_FOR: Duration = Duration::from_secs(8);
/// Going back within this long of a chapter's start goes to the one before.
const RESTART_WITHIN: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Notes,
    Chapters,
    Transcript,
}

impl View {
    fn title(self) -> &'static str {
        match self {
            Self::Notes => "Notes",
            Self::Chapters => "Chapters",
            Self::Transcript => "Transcript",
        }
    }
}

/// What an input did.
pub enum Outcome {
    Ignored,
    Handled,
    /// Jump to this many seconds in.
    Seek(f64),
}

#[derive(Default)]
struct Extras {
    chapters: Vec<Chapter>,
    link: Option<TranscriptLink>,
    generated: Option<StoredTranscript>,
}

impl Extras {
    fn has_transcript(&self) -> bool {
        self.link.is_some() || self.generated.is_some()
    }
}

#[derive(Default)]
pub struct Details {
    episode: Option<EpisodeKey>,
    extras: Remote<Extras>,
    transcript: Remote<Transcript>,
    view: Option<View>,
    notes: NotesView,
    chapters: ListView,
    cues: CueView,
    /// An untimed transcript, laid out like notes.
    transcript_notes: NotesView,
}

impl Details {
    /// Points the pane at `episode`, loading its chapters and transcript list the
    /// first time. YouTube videos have neither.
    pub fn set_episode(&mut self, episode: &Episode, cx: &Ctx) {
        let key = episode.key();
        if self.episode == Some(key) {
            return;
        }
        *self = Self {
            episode: Some(key),
            ..Self::default()
        };
        if episode.is_youtube {
            return;
        }
        let (api, id) = (cx.api.clone(), episode.episode_id);
        self.extras.load(async move {
            let (feed, generated) =
                tokio::join!(api.get_podcasting2(id), api.get_generated_transcript(id));
            // Both are extras: a feed that can't be read or a server without AI
            // features just means there's nothing more to show.
            let feed = feed.unwrap_or_else(|e| {
                tracing::warn!("couldn't get the episode's chapters and transcripts: {e}");
                Podcasting2::default()
            });
            let generated = generated.ok().flatten().filter(|t| {
                t.segments.as_deref().is_some_and(|s| !s.trim().is_empty())
                    || t.full_text.as_deref().is_some_and(|s| !s.trim().is_empty())
            });
            Ok(Extras {
                chapters: feed.chapters.into_iter().filter(|c| c.toc).collect(),
                link: transcript::best_link(&feed.transcripts).cloned(),
                generated,
            })
        });
    }

    pub fn tick(&mut self, cx: &Ctx) {
        if self.extras.poll(cx.bus) {
            let chapters = self.chapters_list().to_vec();
            self.chapters.sync(&chapters, |c| c.title.clone());
        }
        self.transcript.poll(cx.bus);
    }

    pub fn busy(&self) -> bool {
        self.extras.is_loading() || self.transcript.is_loading()
    }

    fn chapters_list(&self) -> &[Chapter] {
        self.extras
            .data
            .as_ref()
            .map_or(&[], |e| e.chapters.as_slice())
    }

    pub fn has_chapters(&self) -> bool {
        !self.chapters_list().is_empty()
    }

    fn views(&self) -> Vec<View> {
        let mut views = vec![View::Notes];
        if let Some(extras) = &self.extras.data {
            if !extras.chapters.is_empty() {
                views.push(View::Chapters);
            }
            if extras.has_transcript() {
                views.push(View::Transcript);
            }
        }
        views
    }

    fn view(&self) -> View {
        self.view
            .filter(|v| self.views().contains(v))
            .unwrap_or(View::Notes)
    }

    /// The next tab. Returns false when there's only the notes.
    pub fn cycle(&mut self, position: Option<f64>, cx: &Ctx) -> bool {
        let views = self.views();
        if views.len() < 2 {
            return false;
        }
        let i = views.iter().position(|v| *v == self.view()).unwrap_or(0);
        let next = views[(i + 1) % views.len()];
        self.view = Some(next);
        match next {
            View::Chapters => {
                if let Some(i) = position.and_then(|p| self.current_chapter(p)) {
                    self.chapters.select_index(i);
                }
            }
            View::Transcript => self.load_transcript(cx),
            View::Notes => {}
        }
        true
    }

    fn load_transcript(&mut self, cx: &Ctx) {
        if !self.transcript.needs_load() {
            return;
        }
        let Some(extras) = &self.extras.data else {
            return;
        };
        let generated = extras.generated.clone();
        let link = extras
            .link
            .clone()
            .filter(|l| generated.is_none() || transcript::prefer_link(l));
        let api = cx.api.clone();
        self.transcript.load(async move {
            if let Some(link) = link {
                match api.fetch_transcript(&link.url).await {
                    Ok(content) => return Ok(transcript::parse(&content, &link.mime_type)),
                    Err(e) if generated.is_some() => {
                        tracing::warn!(
                            "couldn't fetch the feed's transcript, using the server's: {e}"
                        );
                    }
                    Err(e) => return Err(e),
                }
            }
            Ok(generated
                .as_ref()
                .and_then(transcript::from_stored)
                .unwrap_or_else(|| Transcript::Html(String::new())))
        });
    }

    fn current_chapter(&self, position: f64) -> Option<usize> {
        self.chapters_list()
            .iter()
            .rposition(|c| c.start <= position + 0.5)
    }

    /// Where the next (or previous) chapter starts, from `position`.
    pub fn chapter_jump(&self, position: f64, forward: bool) -> Option<f64> {
        let chapters = self.chapters_list();
        let current = self.current_chapter(position);
        if forward {
            return chapters
                .iter()
                .find(|c| c.start > position + 0.5)
                .map(|c| c.start);
        }
        let i = current?;
        if position - chapters[i].start > RESTART_WITHIN || i == 0 {
            Some(chapters[i].start)
        } else {
            Some(chapters[i - 1].start)
        }
    }

    /// The chapter playing at `position`, for a heading.
    pub fn chapter_at(&self, position: f64) -> Option<&str> {
        let i = self.current_chapter(position)?;
        Some(self.chapters_list()[i].title.as_str())
    }

    pub fn action(&mut self, action: Action) -> Outcome {
        match self.view() {
            View::Notes => {
                if self.notes.action(action) {
                    Outcome::Handled
                } else {
                    Outcome::Ignored
                }
            }
            View::Chapters => {
                if self.chapters.navigate(action) {
                    return Outcome::Handled;
                }
                match (action, self.chapters.selected_index()) {
                    (Action::Activate, Some(i)) => Outcome::Seek(self.chapters_list()[i].start),
                    _ => Outcome::Ignored,
                }
            }
            View::Transcript => match &self.transcript.data {
                Some(Transcript::Timed(cues)) => self.cues.action(action, cues),
                Some(Transcript::Html(_)) if self.transcript_notes.action(action) => {
                    Outcome::Handled
                }
                _ => Outcome::Ignored,
            },
        }
    }

    pub fn mouse(&mut self, event: MouseEvent) -> Outcome {
        match self.view() {
            View::Notes if self.notes.mouse(event) => Outcome::Handled,
            View::Chapters => match self.chapters.mouse(event) {
                Click::Activated => self.action(Action::Activate),
                Click::Moved => Outcome::Handled,
                Click::Missed => Outcome::Ignored,
            },
            View::Transcript => match &self.transcript.data {
                Some(Transcript::Timed(cues)) => self.cues.mouse(event, cues),
                Some(Transcript::Html(_)) if self.transcript_notes.mouse(event) => Outcome::Handled,
                _ => Outcome::Ignored,
            },
            _ => Outcome::Ignored,
        }
    }

    /// Draws the tabs (when there's more than notes) and the open view. `position`
    /// is where playback is, when this episode is the one playing.
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        episode: &Episode,
        position: Option<f64>,
        theme: &Theme,
        base: Style,
    ) {
        let views = self.views();
        let area = if views.len() > 1 || self.extras.is_loading() {
            let [tabs, _, rest] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .areas(area);
            self.render_tabs(frame, tabs, &views, theme);
            rest
        } else {
            area
        };
        match self.view() {
            View::Notes => {
                if episode.episode_description.trim().is_empty() {
                    message(
                        frame,
                        area,
                        vec![Line::styled("No show notes.", theme.muted())],
                        theme.base(),
                    );
                    return;
                }
                let html = episode.episode_description.clone();
                self.notes
                    .render(frame, area, episode.key(), &html, theme, base);
            }
            View::Chapters => self.render_chapters(frame, area, position, theme),
            View::Transcript => {
                self.render_transcript(frame, area, episode.key(), position, theme, base)
            }
        }
    }

    fn render_tabs(&self, frame: &mut Frame, area: Rect, views: &[View], theme: &Theme) {
        let current = self.view();
        let mut spans = Vec::new();
        for view in views {
            let style = if *view == current {
                theme.active
            } else {
                theme.muted()
            };
            let label = match view {
                View::Chapters => format!(" {} {} ", view.title(), self.chapters_list().len()),
                _ => format!(" {} ", view.title()),
            };
            spans.push(Span::styled(label, style));
            spans.push(Span::raw(" "));
        }
        if self.extras.is_loading() {
            spans.push(Span::styled(format!("{} ", spinner()), theme.accent()));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_chapters(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        position: Option<f64>,
        theme: &Theme,
    ) {
        let chapters = self.chapters_list().to_vec();
        let current = position.and_then(|p| self.current_chapter(p));
        self.chapters.render(
            frame,
            area,
            theme,
            &[
                Constraint::Length(1),
                Constraint::Length(8),
                Constraint::Fill(1),
            ],
            None,
            |i, w| {
                let chapter = &chapters[i];
                let playing = current == Some(i);
                let (mark, title_style) = if playing {
                    ("▶", theme.accent().add_modifier(Modifier::BOLD))
                } else {
                    (" ", Style::new())
                };
                vec![
                    Cell::from(Span::styled(mark, theme.accent())),
                    Cell::from(Span::styled(
                        clock(Duration::from_secs_f64(chapter.start.max(0.0))),
                        theme.muted(),
                    )),
                    Cell::from(Span::styled(
                        truncate(&chapter.title, usize::from(w[2])),
                        title_style,
                    )),
                ]
            },
        );
    }

    fn render_transcript(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        key: EpisodeKey,
        position: Option<f64>,
        theme: &Theme,
        base: Style,
    ) {
        if super::load_state(
            frame,
            area,
            theme,
            self.transcript.is_empty_loading(),
            self.transcript.error(),
            "the transcript",
        ) {
            return;
        }
        match &self.transcript.data {
            Some(Transcript::Timed(cues)) if !cues.is_empty() => {
                self.cues.render(frame, area, cues, position, theme);
            }
            Some(Transcript::Html(html)) if !html.trim().is_empty() => {
                let html = html.clone();
                self.transcript_notes
                    .render(frame, area, key, &html, theme, base);
            }
            _ => message(
                frame,
                area,
                vec![Line::styled("The transcript is empty.", theme.muted())],
                theme.base(),
            ),
        }
    }
}

/// A transcript line: its cue, its text, and whether it's a speaker's name.
type CueLine = (usize, String, bool);

/// A timed transcript: one cue selected, following playback until you move.
#[derive(Default)]
struct CueView {
    selected: usize,
    scroll: usize,
    /// Moved by hand; don't follow playback until then.
    manual_until: Option<Instant>,
    /// Cues wrapped for a width.
    laid_out: Option<(u16, Vec<CueLine>)>,
    /// Which cue each visible row shows, for clicks.
    rows: Vec<(u16, usize)>,
    height: usize,
}

impl CueView {
    fn following(&self) -> bool {
        self.manual_until.is_none_or(|t| Instant::now() >= t)
    }

    fn moved(&mut self) {
        self.manual_until = Some(Instant::now() + MANUAL_FOR);
    }

    fn action(&mut self, action: Action, cues: &[Cue]) -> Outcome {
        let last = cues.len().saturating_sub(1);
        let page = (self.height / 3).max(1);
        self.selected = match action {
            Action::Up => self.selected.saturating_sub(1),
            Action::Down => (self.selected + 1).min(last),
            Action::PageUp => self.selected.saturating_sub(page),
            Action::PageDown => (self.selected + page).min(last),
            Action::Top => 0,
            Action::Bottom => last,
            Action::Activate => {
                self.manual_until = None;
                return cues
                    .get(self.selected)
                    .map_or(Outcome::Handled, |c| Outcome::Seek(c.start));
            }
            _ => return Outcome::Ignored,
        };
        self.moved();
        Outcome::Handled
    }

    fn mouse(&mut self, event: MouseEvent, cues: &[Cue]) -> Outcome {
        match event.kind {
            MouseEventKind::ScrollDown => self.action(Action::Down, cues),
            MouseEventKind::ScrollUp => self.action(Action::Up, cues),
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(&(_, cue)) = self.rows.iter().find(|(y, _)| *y == event.row) else {
                    return Outcome::Ignored;
                };
                if cue == self.selected {
                    return self.action(Action::Activate, cues);
                }
                self.selected = cue;
                self.moved();
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }

    fn layout(cues: &[Cue], width: u16) -> Vec<CueLine> {
        // "1:02:03  " in front of each cue; continuation lines line up after it.
        const GUTTER: usize = 9;
        let text_width = usize::from(width).saturating_sub(GUTTER + 1).max(10);
        let mut lines = Vec::new();
        let mut speaker: Option<&str> = None;
        for (i, cue) in cues.iter().enumerate() {
            if cue.speaker.is_some() && cue.speaker.as_deref() != speaker {
                speaker = cue.speaker.as_deref();
                lines.push((
                    i,
                    format!("{:GUTTER$}{}", "", speaker.unwrap_or_default()),
                    true,
                ));
            }
            for (n, text) in wrap(&cue.text, text_width).into_iter().enumerate() {
                let time = if n == 0 {
                    clock(Duration::from_secs_f64(cue.start.max(0.0)))
                } else {
                    String::new()
                };
                lines.push((i, format!("{time:>7}  {text}"), false));
            }
        }
        lines
    }

    fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        cues: &[Cue],
        position: Option<f64>,
        theme: &Theme,
    ) {
        let width = area.width.saturating_sub(1);
        if self.laid_out.as_ref().is_none_or(|(w, _)| *w != width) {
            self.laid_out = Some((width, Self::layout(cues, width)));
        }
        let Some((_, lines)) = &self.laid_out else {
            return;
        };
        self.height = usize::from(area.height);
        let current = position.and_then(|p| Transcript::cue_at(cues, p));
        if self.following()
            && let Some(current) = current
        {
            self.selected = current;
        }
        self.selected = self.selected.min(cues.len().saturating_sub(1));

        let first = lines
            .iter()
            .position(|(c, _, _)| *c == self.selected)
            .unwrap_or(0);
        let last = lines
            .iter()
            .rposition(|(c, _, _)| *c == self.selected)
            .unwrap_or(0);
        if self.following() && current.is_some() {
            // Keep the line being spoken a third of the way down.
            self.scroll = first.saturating_sub(self.height / 3);
        } else if first < self.scroll {
            self.scroll = first;
        } else if last >= self.scroll + self.height {
            self.scroll = last + 1 - self.height;
        }
        self.scroll = self.scroll.min(lines.len().saturating_sub(self.height));

        self.rows.clear();
        let mut shown = Vec::new();
        for (row, (cue, text, is_speaker)) in
            lines.iter().skip(self.scroll).take(self.height).enumerate()
        {
            self.rows.push((area.y + row as u16, *cue));
            let style = if *is_speaker {
                theme.muted().add_modifier(Modifier::BOLD)
            } else if *cue == self.selected && (!self.following() || current.is_none()) {
                theme.selected
            } else if Some(*cue) == current {
                theme.accent().add_modifier(Modifier::BOLD)
            } else {
                theme.base()
            };
            shown.push(Line::styled(text.clone(), style));
        }
        frame.render_widget(Paragraph::new(shown), area);
        super::scrollbar(frame, area, lines.len(), self.scroll, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapters(starts: &[f64]) -> Details {
        let mut details = Details::default();
        details.extras.data = Some(Extras {
            chapters: starts
                .iter()
                .map(|s| Chapter {
                    start: *s,
                    title: format!("At {s}"),
                    toc: true,
                })
                .collect(),
            ..Extras::default()
        });
        details
    }

    #[test]
    fn next_and_previous_chapter() {
        let d = chapters(&[0.0, 60.0, 120.0]);
        assert_eq!(d.chapter_jump(10.0, true), Some(60.0));
        assert_eq!(d.chapter_jump(130.0, true), None);
        // Well into a chapter, back goes to its start; near the start, to the one before.
        assert_eq!(d.chapter_jump(90.0, false), Some(60.0));
        assert_eq!(d.chapter_jump(61.0, false), Some(0.0));
        assert_eq!(d.chapter_jump(1.0, false), Some(0.0));
        assert_eq!(d.chapter_at(65.0), Some("At 60"));
    }

    #[test]
    fn tabs_appear_only_for_what_there_is() {
        let mut d = Details::default();
        assert_eq!(d.views(), [View::Notes]);
        d.extras.data = Some(Extras {
            generated: Some(StoredTranscript {
                status: "complete".into(),
                full_text: Some("Hi".into()),
                segments: None,
            }),
            ..Extras::default()
        });
        assert_eq!(d.views(), [View::Notes, View::Transcript]);
    }

    #[test]
    fn moving_through_a_transcript_stops_following() {
        let cues: Vec<Cue> = (0..5)
            .map(|i| Cue {
                start: f64::from(i) * 10.0,
                speaker: None,
                text: format!("Line {i}"),
            })
            .collect();
        let mut view = CueView::default();
        assert!(view.following());
        assert!(matches!(view.action(Action::Down, &cues), Outcome::Handled));
        assert!(!view.following());
        assert!(matches!(view.action(Action::Activate, &cues), Outcome::Seek(s) if s == 10.0));
        assert!(view.following());
    }

    #[test]
    fn speakers_get_a_line_when_they_change() {
        let cue = |speaker: &str, text: &str| Cue {
            start: 0.0,
            speaker: Some(speaker.into()),
            text: text.into(),
        };
        let lines = CueView::layout(&[cue("A", "one"), cue("A", "two"), cue("B", "three")], 60);
        let texts: Vec<&str> = lines.iter().map(|(_, t, _)| t.trim()).collect();
        assert_eq!(texts, ["A", "0:00  one", "0:00  two", "B", "0:00  three"]);
    }
}
