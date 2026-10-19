//! How an episode looks in a list, and what the episode keys do to it, the same
//! on every page.

use chrono::NaiveDate;
use ratatui::{
    layout::Constraint,
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Row},
};

use super::{date, length};
use crate::api::{Episode, EpisodeKey};
use crate::theme::Theme;
use crate::tui::keymap::Action;
use crate::tui::pages::Ctx;
use crate::tui::task::Resource;
use crate::tui::text::truncate;

/// Columns for a list `width` wide. The podcast name drops out first on a narrow
/// terminal, then the date.
#[derive(Debug, Clone, Copy)]
pub struct Columns {
    podcast: bool,
    date: bool,
}

impl Columns {
    pub fn fit(width: u16, with_podcast: bool) -> Self {
        Self {
            podcast: with_podcast && width >= 80,
            date: width >= 56,
        }
    }

    pub fn widths(self) -> Vec<Constraint> {
        let mut widths = vec![Constraint::Length(1), Constraint::Fill(1)];
        if self.podcast {
            widths.push(Constraint::Percentage(25));
        }
        if self.date {
            widths.push(Constraint::Length(11));
        }
        widths.push(Constraint::Length(11));
        widths.push(Constraint::Length(3));
        widths
    }

    pub fn header(self, theme: &Theme) -> Row<'static> {
        let mut cells = vec!["", "Episode"];
        if self.podcast {
            cells.push("Podcast");
        }
        if self.date {
            cells.push("Published");
        }
        cells.push("Length");
        cells.push("");
        Row::new(cells).style(theme.faint())
    }

    pub fn cells(
        self,
        episode: &Episode,
        playing: Option<EpisodeKey>,
        theme: &Theme,
        today: NaiveDate,
        widths: &[u16],
    ) -> Vec<Cell<'static>> {
        let fit = |text: &str, column: usize| {
            truncate(
                text,
                usize::from(widths.get(column).copied().unwrap_or(u16::MAX)),
            )
        };
        let is_playing = playing == Some(episode.key());
        let position = episode.listen_duration.unwrap_or(0);
        let (mark, mark_style) = if is_playing {
            ("▶", theme.accent())
        } else if episode.completed {
            ("✓", Style::new().fg(theme.success))
        } else if position > 0 {
            ("◐", theme.accent())
        } else {
            (" ", theme.faint())
        };
        let title_style = if is_playing {
            theme.accent()
        } else if episode.completed {
            theme.muted()
        } else {
            Style::new()
        };

        let mut cells = vec![
            Cell::from(Span::styled(mark, mark_style)),
            Cell::from(Span::styled(fit(&episode.episode_title, 1), title_style)),
        ];
        if self.podcast {
            cells.push(Cell::from(Span::styled(
                fit(episode.podcast_name.as_deref().unwrap_or_default(), 2),
                theme.muted(),
            )));
        }
        if self.date {
            cells.push(Cell::from(Span::styled(
                date(&episode.episode_pub_date, today),
                theme.muted(),
            )));
        }
        let duration = episode.episode_duration;
        let time = if !episode.completed && position > 0 && duration > position {
            format!("{} left", length(duration - position))
        } else if duration > 0 {
            length(duration)
        } else {
            String::new()
        };
        cells.push(Cell::from(Span::styled(time, theme.muted())));
        cells.push(Cell::from(flags(episode, theme)));
        cells
    }
}

/// ★ saved, ≡ queued, ↓ downloaded on the server.
pub fn flags(episode: &Episode, theme: &Theme) -> Line<'static> {
    let flag = |on: bool, symbol: &'static str| {
        Span::styled(if on { symbol } else { " " }, theme.accent())
    };
    Line::from(vec![
        flag(episode.saved, "★"),
        flag(episode.queued, "≡"),
        flag(episode.downloaded, "↓"),
    ])
}

/// What the filter matches an episode on.
pub fn search_text(episode: &Episode) -> String {
    format!(
        "{} {}",
        episode.episode_title,
        episode.podcast_name.as_deref().unwrap_or_default()
    )
}

pub fn play(episode: &Episode, cx: &Ctx) {
    if cx.playback.play_episode(episode.clone()).is_err() {
        cx.bus
            .error("Playback has stopped working; restart Firewood");
    }
}

/// The episode keys every list shares: queue, play next, save, download, played.
/// Changes show on screen at once and are put back if the server refuses them.
/// Returns false for any other action.
pub fn apply(action: Action, episode: &mut Episode, cx: &Ctx) -> bool {
    let api = cx.api.clone();
    let key = episode.key();
    let name = truncate(&episode.episode_title, 40);
    match action {
        Action::Queue => {
            episode.queued = !episode.queued;
            if episode.queued {
                cx.bus.run(
                    format!("Queued {name}"),
                    "Couldn't add to the queue",
                    &[Resource::Queue],
                    async move { api.queue_episode(key, None).await },
                );
            } else {
                cx.bus.run(
                    format!("Removed {name} from the queue"),
                    "Couldn't remove from the queue",
                    &[Resource::Queue],
                    async move { api.remove_from_queue(key).await },
                );
            }
        }
        Action::PlayNext => {
            let playing = cx.player.current_episode.as_ref().map(Episode::key);
            episode.queued = true;
            cx.bus.run(
                format!("{name} plays next"),
                "Couldn't add to the queue",
                &[Resource::Queue],
                async move { api.queue_episode(key, playing).await },
            );
        }
        Action::Save => {
            episode.saved = !episode.saved;
            if episode.saved {
                cx.bus.run(
                    format!("Saved {name}"),
                    "Couldn't save",
                    &[Resource::Saved],
                    async move { api.save_episode(key).await },
                );
            } else {
                cx.bus.run(
                    format!("Unsaved {name}"),
                    "Couldn't unsave",
                    &[Resource::Saved],
                    async move { api.unsave_episode(key).await },
                );
            }
        }
        Action::Download => {
            episode.downloaded = !episode.downloaded;
            if episode.downloaded {
                cx.bus.run(
                    format!("The server is downloading {name}"),
                    "Couldn't start the download",
                    &[Resource::Downloads],
                    async move { api.download_episode(key).await },
                );
            } else {
                cx.bus.run(
                    format!("Deleted the server's copy of {name}"),
                    "Couldn't delete the download",
                    &[Resource::Downloads],
                    async move { api.delete_download(key).await },
                );
            }
        }
        Action::MarkPlayed => {
            episode.completed = !episode.completed;
            if episode.completed {
                cx.bus.run(
                    format!("Marked {name} played"),
                    "Couldn't mark it played",
                    &[Resource::Progress],
                    async move { api.mark_completed(key).await },
                );
            } else {
                cx.bus.run(
                    format!("Marked {name} unplayed"),
                    "Couldn't mark it unplayed",
                    &[Resource::Progress],
                    async move { api.mark_uncompleted(key).await },
                );
            }
        }
        _ => return false,
    }
    true
}
