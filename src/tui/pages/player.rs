//! What's playing, full screen: progress you can click to seek, the transport,
//! volume, speed and skips, and the show notes, chapters and transcript underneath.

use std::time::Duration;

use ratatui::{
    Frame,
    crossterm::event::{MouseButton, MouseEvent, MouseEventKind},
    layout::{Alignment, Constraint, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::{Ctx, Outcome, Page};
use crate::audio::skip::SkipKind;
use crate::playback::{PlaybackState, PlayerState};
use crate::theme::Theme;
use crate::tui::keymap::{Action, Context};
use crate::tui::text::truncate;
use crate::tui::ui::details::{self, Details};
use crate::tui::ui::{self, clock};

/// How long "Skipped …" stays on screen after a jump.
const SKIP_NOTICE_FOR: Duration = Duration::from_secs(5);
const VOLUME_STEP: f32 = 0.05;

#[derive(Default)]
pub struct PlayerPage {
    details: Details,
    /// Clickable parts, from the last frame.
    progress_area: Rect,
    buttons: [Rect; 3],
}

/// Volume as a row of blocks, like `▮▮▮▮▮▯▯▯▯▯ 50%`.
fn volume_meter<'a>(volume: f32, theme: &Theme) -> Vec<Span<'a>> {
    let filled = (volume.clamp(0.0, 1.0) * 10.0).round() as usize;
    vec![
        Span::styled("▮".repeat(filled), Style::new().fg(theme.gauge)),
        Span::styled("▯".repeat(10 - filled), theme.faint()),
        Span::styled(format!(" {:.0}%", volume * 100.0), theme.muted()),
    ]
}

fn status_line<'a>(state: &PlayerState, theme: &Theme) -> Line<'a> {
    let warn = Style::new().fg(theme.warning);
    match &state.playback_state {
        PlaybackState::Error(msg) => Line::styled(
            format!("Playback failed: {msg}"),
            Style::new().fg(theme.error),
        ),
        PlaybackState::Loading => Line::styled(
            match state.reconnecting {
                Some(attempt) => format!(
                    "{} Connection lost, reconnecting (attempt {attempt})…",
                    ui::spinner()
                ),
                None => format!("{} Loading…", ui::spinner()),
            },
            warn,
        ),
        _ if state.buffering => Line::styled(format!("{} Buffering…", ui::spinner()), warn),
        _ => match state.skips.last {
            Some(skip) if skip.at.elapsed() < SKIP_NOTICE_FOR => {
                let what = match skip.kind {
                    SkipKind::Ad => "an ad",
                    SkipKind::Silence => "silence",
                };
                Line::styled(
                    format!("Skipped {what} ({}s)", skip.length.as_secs()),
                    theme.accent(),
                )
            }
            _ => Line::default(),
        },
    }
}

fn skip_summary(state: &PlayerState) -> String {
    let skips = &state.skips;
    let trim = match skips.trim_silence {
        Some(true) if skips.silences > 0 => format!("trimming silence ({})", skips.silences),
        Some(true) => "trim silence on".to_string(),
        Some(false) => "trim silence off".to_string(),
        None => String::new(),
    };
    let ads = match skips.ads {
        0 => String::new(),
        1 => "skipping 1 ad".to_string(),
        n => format!("skipping {n} ads"),
    };
    [ads, trim]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

impl Page for PlayerPage {
    fn title(&self) -> String {
        "Now Playing".into()
    }

    fn contexts(&self) -> &'static [Context] {
        &[Context::Player, Context::Details, Context::List]
    }

    fn hints(&self) -> &'static [Action] {
        &[
            Action::TogglePlay,
            Action::SeekBackward,
            Action::SeekForward,
            Action::CycleView,
            Action::NextChapter,
            Action::SpeedUp,
            Action::TrimSilence,
        ]
    }

    fn tick(&mut self, cx: &Ctx) {
        if let Some(episode) = &cx.player.current_episode {
            self.details.set_episode(episode, cx);
        }
        self.details.tick(cx);
    }

    fn busy(&self) -> bool {
        self.details.busy()
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome {
        let state = cx.player;
        let position = state.current_position.as_secs_f64();
        let result = match action {
            Action::CycleView => {
                if !self.details.cycle(Some(position), cx) {
                    cx.bus.info("This episode has no chapters or transcript");
                }
                Ok(())
            }
            Action::NextChapter | Action::PrevChapter => {
                let forward = action == Action::NextChapter;
                match self.details.chapter_jump(position, forward) {
                    Some(start) => cx.playback.seek(Duration::from_secs_f64(start)),
                    None if self.details.has_chapters() => Ok(()),
                    None => {
                        cx.bus.info("This episode has no chapters");
                        Ok(())
                    }
                }
            }
            Action::Stop => cx.playback.stop(),
            Action::ResetSpeed => cx.playback.set_speed(1.0),
            Action::TrimSilence => {
                if state.skips.trim_silence.is_none() {
                    cx.bus.info("The server doesn't support trimming silence");
                    Ok(())
                } else {
                    cx.playback.toggle_trim_silence()
                }
            }
            Action::VolumeUp => cx
                .playback
                .set_volume((state.volume + VOLUME_STEP).min(1.0)),
            Action::VolumeDown => cx
                .playback
                .set_volume((state.volume - VOLUME_STEP).max(0.0)),
            other => match self.details.action(other) {
                details::Outcome::Seek(start) => cx.playback.seek(Duration::from_secs_f64(start)),
                details::Outcome::Handled => Ok(()),
                details::Outcome::Ignored => return Outcome::Ignored,
            },
        };
        if result.is_err() {
            cx.bus
                .error("Playback has stopped working; restart Firewood");
        }
        Outcome::Handled
    }

    fn mouse(&mut self, event: MouseEvent, cx: &mut Ctx) -> Outcome {
        let at = Position::new(event.column, event.row);
        match event.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                self.details.mouse(event);
            }
            MouseEventKind::Down(MouseButton::Left) if self.progress_area.contains(at) => {
                let total = cx.player.total_duration;
                let area = self.progress_area;
                if total.is_zero() || area.width == 0 {
                    return Outcome::Handled;
                }
                let ratio = f64::from(event.column - area.x) / f64::from(area.width);
                let _ = cx.playback.seek(total.mul_f64(ratio.clamp(0.0, 1.0)));
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let skip = Duration::from_secs(cx.settings.skip_interval());
                let result = if self.buttons[0].contains(at) {
                    cx.playback.skip_backward(skip)
                } else if self.buttons[1].contains(at) {
                    cx.playback.toggle_play_pause()
                } else if self.buttons[2].contains(at) {
                    cx.playback.skip_forward(skip)
                } else {
                    return match self.details.mouse(event) {
                        details::Outcome::Seek(start) => {
                            let _ = cx.playback.seek(Duration::from_secs_f64(start));
                            Outcome::Handled
                        }
                        details::Outcome::Handled => Outcome::Handled,
                        details::Outcome::Ignored => Outcome::Ignored,
                    };
                };
                if result.is_err() {
                    cx.bus
                        .error("Playback has stopped working; restart Firewood");
                }
            }
            _ => return Outcome::Ignored,
        }
        Outcome::Handled
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx) {
        let theme = cx.theme;
        let state = cx.player;
        self.progress_area = Rect::default();
        self.buttons = [Rect::default(); 3];
        let Some(episode) = &state.current_episode else {
            ui::message(
                frame,
                area,
                vec![
                    Line::styled("Nothing playing", theme.heading()),
                    Line::styled(
                        "Press Enter on an episode in any list to start it.",
                        theme.muted(),
                    ),
                ],
                theme.base(),
            );
            return;
        };

        let width = area.width.min(90);
        let column = Rect {
            x: area.x + (area.width - width) / 2,
            width,
            ..area
        };
        let [
            title,
            podcast,
            _,
            progress,
            times,
            _,
            controls,
            _,
            settings,
            status,
            _,
            notes_area,
        ] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(column);

        frame.render_widget(
            Paragraph::new(Span::styled(
                episode.episode_title.clone(),
                theme.heading().add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            title,
        );
        let mut byline = vec![Span::styled(
            episode.podcast_name.clone().unwrap_or_default(),
            theme.muted(),
        )];
        if let Some(chapter) = self
            .details
            .chapter_at(state.current_position.as_secs_f64())
        {
            byline.push(Span::styled("  ·  ", theme.faint()));
            byline.push(Span::styled(
                format!("▸ {}", truncate(chapter, usize::from(podcast.width) / 2)),
                theme.accent(),
            ));
        }
        frame.render_widget(
            Paragraph::new(Line::from(byline)).alignment(Alignment::Center),
            podcast,
        );

        let total = state.total_duration;
        let ratio = if total.is_zero() {
            0.0
        } else {
            state.current_position.as_secs_f64() / total.as_secs_f64()
        };
        frame.render_widget(
            Paragraph::new(Line::from(ui::bar(ratio, progress.width, theme))),
            progress,
        );
        self.progress_area = progress;
        let remaining = total.saturating_sub(state.current_position);
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(times);
        frame.render_widget(
            Paragraph::new(Span::styled(clock(state.current_position), theme.muted())),
            left,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("-{}  {}", clock(remaining), clock(total)),
                theme.muted(),
            ))
            .alignment(Alignment::Right),
            right,
        );

        let skip = cx.settings.skip_interval();
        let play = match state.playback_state {
            PlaybackState::Playing => "⏸  Pause",
            PlaybackState::Loading => "…  Loading",
            _ => "▶  Play",
        };
        let labels = [
            format!("«  {skip}s"),
            play.to_string(),
            format!("{skip}s  »"),
        ];
        let widths: Vec<u16> = labels
            .iter()
            .map(|l| l.chars().count() as u16 + 4)
            .collect();
        let total_width: u16 = widths.iter().sum::<u16>() + 4;
        let mut x = controls.x + controls.width.saturating_sub(total_width) / 2;
        for (i, label) in labels.iter().enumerate() {
            let rect = Rect {
                x,
                y: controls.y,
                width: widths[i].min(controls.right().saturating_sub(x)),
                height: 1,
            };
            let style = if i == 1 { theme.active } else { theme.selected };
            frame.render_widget(
                Paragraph::new(Span::styled(format!("  {label}  "), style)),
                rect,
            );
            self.buttons[i] = rect;
            x += widths[i] + 2;
        }

        let mut line = vec![Span::styled("Volume ", theme.faint())];
        line.extend(volume_meter(state.volume, theme));
        line.push(Span::styled("    Speed ", theme.faint()));
        line.push(Span::styled(ui::speed(state.speed), theme.accent()));
        let skips = skip_summary(state);
        if !skips.is_empty() {
            line.push(Span::styled(format!("    {skips}"), theme.muted()));
        }
        frame.render_widget(
            Paragraph::new(Line::from(line)).alignment(Alignment::Center),
            settings,
        );
        frame.render_widget(
            Paragraph::new(status_line(state, theme)).alignment(Alignment::Center),
            status,
        );

        let position = state.current_position.as_secs_f64();
        self.details.render(
            frame,
            notes_area,
            episode,
            Some(position),
            theme,
            theme.muted(),
        );
    }
}
