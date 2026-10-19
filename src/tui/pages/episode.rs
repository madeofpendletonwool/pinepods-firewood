//! One episode: what it is, where you are in it, and its show notes.

use ratatui::{
    Frame,
    crossterm::event::MouseEvent,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use super::{Ctx, Outcome, Page};
use crate::api::Episode;
use crate::tui::keymap::{Action, Context};
use crate::tui::task::{Remote, Resource};
use crate::tui::ui::notes::NotesView;
use crate::tui::ui::{self, episodes};

pub struct EpisodePage {
    episode: Episode,
    /// The full record, fetched when the list's copy has no description.
    details: Remote<Episode>,
    notes: NotesView,
}

impl EpisodePage {
    pub fn new(episode: Episode) -> Self {
        Self {
            episode,
            details: Remote::default(),
            notes: NotesView::default(),
        }
    }
}

impl Page for EpisodePage {
    fn title(&self) -> String {
        self.episode.episode_title.clone()
    }

    fn contexts(&self) -> &'static [Context] {
        &[Context::Episode, Context::List]
    }

    fn hints(&self) -> &'static [Action] {
        &[
            Action::Activate,
            Action::Queue,
            Action::PlayNext,
            Action::Save,
            Action::Download,
            Action::MarkPlayed,
            Action::Back,
        ]
    }

    fn show(&mut self, cx: &Ctx) {
        if self.episode.episode_description.trim().is_empty() && self.details.needs_load() {
            let (api, key) = (cx.api.clone(), self.episode.key());
            self.details
                .load(async move { api.get_episode_metadata(key).await });
        }
    }

    fn tick(&mut self, cx: &Ctx) {
        if self.details.poll(cx.bus)
            && let Some(full) = self.details.data.take()
        {
            self.episode = Episode {
                podcast_name: full.podcast_name.or(self.episode.podcast_name.take()),
                ..full
            };
            self.notes = NotesView::default();
        }
    }

    fn changed(&mut self, _resource: Resource) {}

    fn busy(&self) -> bool {
        self.details.is_loading()
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        match action {
            Action::Activate => episodes::play(&self.episode, cx),
            Action::Back => return Outcome::Pop,
            other if self.notes.action(other) => {}
            other => {
                if !episodes::apply(other, &mut self.episode, cx) {
                    return Outcome::Ignored;
                }
            }
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
        let e = &self.episode;
        let [head, _, notes_area] = Layout::vertical([
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(area);

        let mut facts = vec![Span::styled(
            ui::date(&e.episode_pub_date, ui::today()),
            theme.muted(),
        )];
        if e.episode_duration > 0 {
            facts.push(Span::styled(
                format!(" · {}", ui::length(e.episode_duration)),
                theme.muted(),
            ));
        }
        let position = e.listen_duration.unwrap_or(0);
        if e.completed {
            facts.push(Span::styled(" · played", Style::new().fg(theme.success)));
        } else if position > 0 {
            facts.push(Span::styled(
                format!(
                    " · stopped at {}",
                    ui::clock(std::time::Duration::from_secs(position as u64))
                ),
                theme.accent(),
            ));
        }
        let mut flags = Vec::new();
        if e.saved {
            flags.push("★ saved");
        }
        if e.queued {
            flags.push("≡ queued");
        }
        if e.downloaded {
            flags.push("↓ on the server");
        }
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    e.episode_title.clone(),
                    theme.heading().add_modifier(Modifier::BOLD),
                ),
                Line::styled(e.podcast_name.clone().unwrap_or_default(), theme.accent()),
                Line::from(facts),
                Line::styled(flags.join("   "), theme.muted()),
            ])
            .wrap(Wrap { trim: true }),
            head,
        );

        if self.details.is_empty_loading() && e.episode_description.trim().is_empty() {
            ui::load_state(frame, notes_area, theme, true, None, "show notes");
            return;
        }
        if e.episode_description.trim().is_empty() {
            ui::message(
                frame,
                notes_area,
                vec![Line::styled("No show notes.", theme.muted())],
                theme.base(),
            );
            return;
        }
        let (key, html) = (e.key(), e.episode_description.clone());
        self.notes
            .render(frame, notes_area, key, &html, theme, theme.base());
    }

    fn selected_episode(&self) -> Option<Episode> {
        Some(self.episode.clone())
    }
}
