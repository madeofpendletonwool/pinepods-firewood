//! Where you left off, what's new, and your most-played podcasts.
//!
//! Three lists stacked on one screen. Moving past the end of one carries on into
//! the next, so it reads as a single list.

use ratatui::{
    Frame,
    crossterm::event::MouseEvent,
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Cell, Paragraph},
};

use super::episode::EpisodePage;
use super::episodes::EpisodeListPage;
use super::{Ctx, Outcome, Page};
use crate::api::{Episode, HomeOverview};
use crate::tui::keymap::{Action, Context};
use crate::tui::task::{Remote, Resource};
use crate::tui::text::truncate;
use crate::tui::ui::episodes::{self, Columns};
use crate::tui::ui::list::{Click, ListView};
use crate::tui::ui::{self, today};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Continue,
    Recent,
    Podcasts,
}

impl Part {
    const ALL: [Part; 3] = [Self::Continue, Self::Recent, Self::Podcasts];

    fn title(self) -> &'static str {
        match self {
            Self::Continue => "Continue listening",
            Self::Recent => "New episodes",
            Self::Podcasts => "Your top podcasts",
        }
    }
}

#[derive(Default)]
pub struct HomePage {
    data: Remote<HomeOverview>,
    lists: [ListView; 3],
    focus: usize,
}

impl HomePage {
    fn load(&mut self, cx: &Ctx) {
        let api = cx.api.clone();
        self.data.load(async move { api.get_home_overview().await });
    }

    fn sync(&mut self) {
        let Some(home) = &self.data.data else {
            return;
        };
        self.lists[0].sync(&home.in_progress_episodes, episodes::search_text);
        self.lists[1].sync(&home.recent_episodes, episodes::search_text);
        self.lists[2].sync(&home.top_podcasts, |p| p.podcastname.clone());
        // Start on the first part that has anything in it.
        if self.lists[self.focus].is_empty()
            && let Some(first) = self.lists.iter().position(|l| !l.is_empty())
        {
            self.focus = first;
        }
    }

    fn part_episodes(&self, part: Part) -> &[Episode] {
        match (&self.data.data, part) {
            (Some(h), Part::Continue) => &h.in_progress_episodes,
            (Some(h), Part::Recent) => &h.recent_episodes,
            _ => &[],
        }
    }

    fn part_episodes_mut(&mut self, part: Part) -> Option<&mut Vec<Episode>> {
        match (&mut self.data.data, part) {
            (Some(h), Part::Continue) => Some(&mut h.in_progress_episodes),
            (Some(h), Part::Recent) => Some(&mut h.recent_episodes),
            _ => None,
        }
    }

    /// Moves within the focused part, spilling into the next or previous one.
    fn step(&mut self, down: bool) {
        let list = &mut self.lists[self.focus];
        let at_edge = match (down, list.selected_index()) {
            (_, None) => true,
            (true, Some(_)) => list.visible().last() == list.selected_index().as_ref(),
            (false, Some(_)) => list.visible().first() == list.selected_index().as_ref(),
        };
        if !at_edge {
            list.move_by(if down { 1 } else { -1 });
            return;
        }
        let order: Vec<usize> = if down {
            (self.focus + 1..3).collect()
        } else {
            (0..self.focus).rev().collect()
        };
        if let Some(next) = order.into_iter().find(|&i| !self.lists[i].is_empty()) {
            self.focus = next;
            let action = if down { Action::Top } else { Action::Bottom };
            self.lists[next].navigate(action);
        }
    }
}

impl Page for HomePage {
    fn title(&self) -> String {
        "Home".into()
    }

    fn contexts(&self) -> &'static [Context] {
        if Part::ALL[self.focus] == Part::Podcasts {
            &[Context::List]
        } else {
            &[Context::Episode, Context::List]
        }
    }

    fn hints(&self) -> &'static [Action] {
        &[Action::Activate, Action::Open, Action::Queue, Action::Save]
    }

    fn show(&mut self, cx: &Ctx) {
        if self.data.needs_load() {
            self.load(cx);
        }
    }

    fn tick(&mut self, cx: &Ctx) {
        if self.data.poll(cx.bus) {
            self.sync();
        }
    }

    fn refresh(&mut self, cx: &Ctx) {
        self.load(cx);
    }

    fn changed(&mut self, _resource: Resource) {
        self.data.mark_stale();
    }

    fn busy(&self) -> bool {
        self.data.is_loading()
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        let part = Part::ALL[self.focus];
        match action {
            Action::Down => self.step(true),
            Action::Up => self.step(false),
            Action::Top => {
                if let Some(first) = self.lists.iter().position(|l| !l.is_empty()) {
                    self.focus = first;
                    self.lists[first].navigate(Action::Top);
                }
            }
            Action::Bottom => {
                if let Some(last) = self.lists.iter().rposition(|l| !l.is_empty()) {
                    self.focus = last;
                    self.lists[last].navigate(Action::Bottom);
                }
            }
            Action::PageUp | Action::PageDown => {
                self.lists[self.focus].navigate(action);
            }
            Action::Activate | Action::Open if part == Part::Podcasts => {
                let podcast = self.lists[2]
                    .selected_index()
                    .and_then(|i| self.data.data.as_ref().and_then(|h| h.top_podcasts.get(i)));
                if let Some(p) = podcast {
                    return Outcome::Push(Box::new(EpisodeListPage::podcast(p.clone())));
                }
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
                let index = self.lists[self.focus].selected_index();
                let Some(episode) = index.and_then(|i| {
                    self.part_episodes_mut(part)
                        .and_then(|list| list.get_mut(i))
                }) else {
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
        for i in 0..3 {
            match self.lists[i].mouse(event) {
                Click::Missed => continue,
                Click::Moved => {
                    self.focus = i;
                    return Outcome::Handled;
                }
                Click::Activated => {
                    self.focus = i;
                    return self.action(Action::Activate, cx);
                }
            }
        }
        Outcome::Ignored
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        if ui::load_state(
            frame,
            area,
            theme,
            self.data.is_empty_loading(),
            self.data.error(),
            "your home page",
        ) {
            return;
        }
        let Some(home) = self.data.data.clone() else {
            return;
        };

        let [stats, body] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
        let stat = |n: i64, what: &str| {
            vec![
                Span::styled(n.to_string(), theme.heading()),
                Span::styled(format!(" {what}   "), theme.muted()),
            ]
        };
        frame.render_widget(
            Paragraph::new(Line::from(
                [
                    stat(home.queue_count, "in the queue"),
                    stat(home.saved_count, "saved"),
                    stat(home.downloaded_count, "downloaded"),
                ]
                .concat(),
            )),
            stats,
        );

        if self.lists.iter().all(ListView::is_empty) {
            ui::message(
                frame,
                body,
                vec![Line::styled(
                    "Nothing here yet. Subscribe to a podcast in the web app to get started.",
                    theme.muted(),
                )],
                theme.base(),
            );
            return;
        }

        // Each part gets its rows plus a title line, up to a cap; New episodes takes
        // whatever is left.
        let len = |i: usize| self.lists[i].visible().len() as u16;
        let constraints = [
            Constraint::Length(if len(0) == 0 { 0 } else { len(0).min(6) + 3 }),
            Constraint::Min(if len(1) == 0 { 0 } else { 5 }),
            Constraint::Length(if len(2) == 0 { 0 } else { len(2).min(6) + 2 }),
        ];
        let areas = Layout::vertical(constraints).split(body);
        let today = today();
        let playing = cx.player.current_episode.as_ref().map(Episode::key);
        let focus = self.focus;

        for (i, part) in Part::ALL.into_iter().enumerate() {
            let area = areas[i];
            if area.height < 2 || self.lists[i].is_empty() {
                continue;
            }
            let [title, rows] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
            let style = if i == focus {
                theme.heading()
            } else {
                theme.muted().add_modifier(Modifier::BOLD)
            };
            frame.render_widget(Paragraph::new(Span::styled(part.title(), style)), title);
            let rows = Rect {
                height: rows.height.saturating_sub(u16::from(i < 2)),
                ..rows
            };
            // Only the focused part shows a selection.
            self.lists[i].set_focused(i == focus);

            if part == Part::Podcasts {
                let pods = &home.top_podcasts;
                self.lists[i].render(
                    frame,
                    rows,
                    theme,
                    &[Constraint::Fill(1), Constraint::Length(14)],
                    None,
                    |j, w| {
                        vec![
                            Cell::from(truncate(&pods[j].podcastname, usize::from(w[0]))),
                            Cell::from(Span::styled(
                                format!("{} episodes", pods[j].episodecount),
                                theme.muted(),
                            )),
                        ]
                    },
                );
            } else {
                let list = self.part_episodes(part).to_vec();
                let columns = Columns::fit(rows.width, true);
                self.lists[i].render(frame, rows, theme, &columns.widths(), None, |j, w| {
                    columns.cells(&list[j], playing, theme, today, w)
                });
            }
        }
    }

    fn selected_episode(&self) -> Option<Episode> {
        let part = Part::ALL[self.focus];
        let index = self.lists[self.focus].selected_index()?;
        self.part_episodes(part).get(index).cloned()
    }
}
