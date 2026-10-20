//! Pieces every page draws with.

pub mod details;
pub mod episodes;
pub mod list;
pub mod notes;
pub mod transcript;

use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};

use crate::theme::Theme;
use crate::tui::keymap::{Action, Context, Keymap};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// A braille spinner frame that advances with wall-clock time, so every spinner on
/// screen turns together however often the screen redraws.
pub fn spinner() -> &'static str {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let elapsed = START.get_or_init(Instant::now).elapsed();
    SPINNER[(elapsed.as_millis() / 80) as usize % SPINNER.len()]
}

/// A slim scrollbar down the right edge of `area`, when `total` lines don't fit.
pub fn scrollbar(frame: &mut Frame, area: Rect, total: usize, offset: usize, theme: &Theme) {
    let shown = usize::from(area.height);
    if total <= shown {
        return;
    }
    let mut state = ScrollbarState::new(total - shown).position(offset);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(" "))
            .thumb_symbol("▐")
            .thumb_style(theme.faint()),
        area,
        &mut state,
    );
}

/// A thin progress bar `width` cells wide: played in the gauge color, the rest faint.
pub fn bar<'a>(ratio: f64, width: u16, theme: &Theme) -> Vec<Span<'a>> {
    let width = usize::from(width);
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    vec![
        Span::styled("━".repeat(filled), Style::new().fg(theme.gauge)),
        Span::styled("─".repeat(width - filled), theme.faint()),
    ]
}

/// Steps speed by `delta`, landing on tenths so repeated presses don't drift.
pub fn step_speed(current: f32, delta: f32) -> f32 {
    crate::audio::speed::clamp(((current + delta) * 10.0).round() / 10.0)
}

/// `1×`, `1.5×`, `1.25×`.
pub fn speed(speed: f32) -> String {
    let text = format!("{speed:.2}");
    format!("{}×", text.trim_end_matches('0').trim_end_matches('.'))
}

/// `1:02:03` or `2:03`.
pub fn clock(duration: Duration) -> String {
    let secs = duration.as_secs();
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `1 episode`, `12 episodes`.
pub fn episode_count(n: i64) -> String {
    if n == 1 {
        "1 episode".into()
    } else {
        format!("{n} episodes")
    }
}

/// `1h 5m`, `45m`, or `30s`, for lengths in a list.
pub fn length(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let (h, m) = (seconds / 3600, (seconds % 3600) / 60);
    match (h, m) {
        (0, 0) => format!("{seconds}s"),
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// `Oct 3` this year, `Oct 3 2023` before. The server sends `2026-01-02T03:04:05`
/// or a bare date; anything else is shown as it came.
pub fn date(published: &str, today: NaiveDate) -> String {
    let parsed = NaiveDateTime::parse_from_str(published, "%Y-%m-%dT%H:%M:%S")
        .map(|dt| dt.date())
        .or_else(|_| NaiveDate::parse_from_str(published.get(..10).unwrap_or(""), "%Y-%m-%d"));
    match parsed {
        Ok(d) if d.year() == today.year() => d.format("%b %-d").to_string(),
        Ok(d) => d.format("%b %-d %Y").to_string(),
        Err(_) => published.to_string(),
    }
}

pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// A rectangle `width` × `height` centred in `area`, clamped to fit.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [rect] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    rect
}

/// A bordered popup with `title`, cleared and filled with the surface color.
/// Returns the area inside the border.
pub fn popup(frame: &mut Frame, area: Rect, title: &str, theme: &Theme) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.border(true))
        .title(Span::styled(format!(" {title} "), theme.heading()))
        .style(theme.surface());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// "key description" pairs for `actions`, skipping any that have no key.
pub fn hints<'a>(
    actions: &[Action],
    contexts: &[Context],
    keymap: &Keymap,
    theme: &Theme,
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    for action in actions {
        let Some(key) = keymap.key_for(*action, contexts) else {
            continue;
        };
        if !spans.is_empty() {
            spans.push(Span::styled("  ", theme.faint()));
        }
        spans.push(Span::styled(key.to_string(), theme.key()));
        spans.push(Span::styled(
            format!(" {}", action.description()),
            theme.muted(),
        ));
    }
    spans
}

/// A centred message filling `area`: an empty list, a failed load, a spinner.
pub fn message(frame: &mut Frame, area: Rect, lines: Vec<Line<'_>>, style: Style) {
    let height = lines.len() as u16;
    let [middle] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    frame.render_widget(
        Paragraph::new(lines)
            .style(style)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true }),
        middle,
    );
}

/// What a page shows in place of its content while loading or after a failure.
pub fn load_state(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    loading: bool,
    error: Option<&str>,
    what: &str,
) -> bool {
    if let Some(error) = error {
        message(
            frame,
            area,
            vec![
                Line::styled(format!("Couldn't load {what}"), theme.heading()),
                Line::styled(error.to_string(), Style::new().fg(theme.error)),
                Line::default(),
                Line::styled("r to try again", theme.faint()),
            ],
            theme.base(),
        );
        return true;
    }
    if loading {
        message(
            frame,
            area,
            vec![Line::styled(
                format!("{} Loading {what}…", spinner()),
                theme.muted(),
            )],
            theme.base(),
        );
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times() {
        assert_eq!(clock(Duration::from_secs(62)), "1:02");
        assert_eq!(clock(Duration::from_secs(3723)), "1:02:03");
        assert_eq!(length(45 * 60), "45m");
        assert_eq!(length(3600 + 5 * 60), "1h 5m");
        assert_eq!(length(7200), "2h");
        assert_eq!(length(30), "30s");
    }

    #[test]
    fn speed_steps_land_on_tenths_and_stay_in_range() {
        assert_eq!(step_speed(1.0, 0.1), 1.1);
        assert_eq!(step_speed(1.25, -0.1), 1.2);
        assert_eq!(step_speed(3.0, 0.1), 3.0);
        assert_eq!(step_speed(0.5, -0.1), 0.5);
    }

    #[test]
    fn speed_is_shown_without_trailing_zeros() {
        assert_eq!(speed(1.0), "1×");
        assert_eq!(speed(1.5), "1.5×");
        assert_eq!(speed(1.25), "1.25×");
    }

    #[test]
    fn formats_dates_relative_to_this_year() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        assert_eq!(date("2026-10-03T08:00:00", today), "Oct 3");
        assert_eq!(date("2023-01-02", today), "Jan 2 2023");
        assert_eq!(date("whenever", today), "whenever");
    }
}
