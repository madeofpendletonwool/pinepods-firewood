//! One episode: what it is, where you are in it, and its show notes, chapters and
//! transcript.

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
use crate::tui::ui::details::{self, Details};
use crate::tui::ui::{self, episodes};

pub struct EpisodePage {
    episode: Episode,
    /// The full record, fetched when the list's copy has no description.
    full: Remote<Episode>,
    details: Details,
}

impl EpisodePage {
    pub fn new(episode: Episode) -> Self {
        Self {
            episode,
            full: Remote::default(),
            details: Details::default(),
        }
    }
}

impl EpisodePage {
    /// Where playback is in this episode, if it's the one playing.
    fn position(&self, cx: &Ctx) -> Option<f64> {
        let playing = cx.player.current_episode.as_ref()?;
        (playing.key() == self.episode.key()).then_some(cx.player.current_position.as_secs_f64())
    }

    /// Jumps there if this episode is playing, or starts it from there.
    fn play_from(&self, start: f64, cx: &Ctx) {
        let start = std::time::Duration::from_secs_f64(start.max(0.0));
        let result = if self.position(cx).is_some() {
            cx.playback.seek(start)
        } else {
            cx.playback.play_episode_from(self.episode.clone(), start)
        };
        if result.is_err() {
            cx.bus
                .error("Playback has stopped working; restart Firewood");
        }
    }
}

impl Page for EpisodePage {
    fn title(&self) -> String {
        self.episode.episode_title.clone()
    }

    fn contexts(&self) -> &'static [Context] {
        &[Context::Episode, Context::Details, Context::List]
    }

    fn hints(&self) -> &'static [Action] {
        &[
            Action::Activate,
            Action::CycleView,
            Action::Queue,
            Action::PlayNext,
            Action::Save,
            Action::Download,
            Action::MarkPlayed,
            Action::Back,
        ]
    }

    fn show(&mut self, cx: &Ctx) {
        if self.episode.episode_description.trim().is_empty() && self.full.needs_load() {
            let (api, key) = (cx.api.clone(), self.episode.key());
            self.full
                .load(async move { api.get_episode_metadata(key).await });
        }
        self.details.set_episode(&self.episode, cx);
    }

    fn tick(&mut self, cx: &Ctx) {
        if self.full.poll(cx.bus)
            && let Some(full) = self.full.data.take()
        {
            self.episode = Episode {
                podcast_name: full.podcast_name.or(self.episode.podcast_name.take()),
                ..full
            };
        }
        self.details.tick(cx);
    }

    fn changed(&mut self, _resource: Resource) {}

    fn busy(&self) -> bool {
        self.full.is_loading() || self.details.busy()
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        match action {
            Action::CycleView => {
                if !self.details.cycle(self.position(cx), cx) {
                    cx.bus.info("This episode has no chapters or transcript");
                }
                return Outcome::Handled;
            }
            Action::Back => return Outcome::Pop,
            _ => {}
        }
        match self.details.action(action) {
            details::Outcome::Seek(start) => {
                self.play_from(start, cx);
                return Outcome::Handled;
            }
            details::Outcome::Handled => return Outcome::Handled,
            details::Outcome::Ignored => {}
        }
        match action {
            Action::Activate => episodes::play(&self.episode, cx),
            other => {
                if !episodes::apply(other, &mut self.episode, cx) {
                    return Outcome::Ignored;
                }
            }
        }
        Outcome::Handled
    }

    fn mouse(&mut self, event: MouseEvent, cx: &mut Ctx) -> Outcome {
        match self.details.mouse(event) {
            details::Outcome::Seek(start) => {
                self.play_from(start, cx);
                Outcome::Handled
            }
            details::Outcome::Handled => Outcome::Handled,
            details::Outcome::Ignored => Outcome::Ignored,
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

        if self.full.is_empty_loading() && e.episode_description.trim().is_empty() {
            ui::load_state(frame, notes_area, theme, true, None, "show notes");
            return;
        }
        let position = self.position(cx);
        let episode = self.episode.clone();
        self.details
            .render(frame, notes_area, &episode, position, theme, theme.base());
    }

    fn selected_episode(&self) -> Option<Episode> {
        Some(self.episode.clone())
    }

    fn episodes(&self) -> Vec<&Episode> {
        vec![&self.episode]
    }
}
