//! Show notes: feed HTML as wrapped, styled terminal text.
//!
//! Links show inline in the accent color and are listed once each at the end, so
//! their addresses are there to copy without footnote markers in the prose.

use html2text::render::RichAnnotation;
use ratatui::{
    Frame,
    crossterm::event::{MouseEvent, MouseEventKind},
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::api::EpisodeKey;
use crate::theme::Theme;
use crate::tui::keymap::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Link,
    Strong,
    Emphasis,
    Code,
    Heading,
}

pub type Notes = Vec<Vec<(String, Kind)>>;

/// Lays out `html` for a column `width` wide.
pub fn layout(html: &str, width: u16) -> Notes {
    let width = usize::from(width.max(20));
    let Ok(tagged) = html2text::config::rich()
        .link_footnotes(false)
        .lines_from_read(html.as_bytes(), width)
    else {
        return html
            .lines()
            .map(|l| vec![(l.to_string(), Kind::Plain)])
            .collect();
    };

    let mut links: Vec<String> = Vec::new();
    let mut lines: Notes = tagged
        .iter()
        .map(|line| {
            line.tagged_strings()
                .map(|ts| {
                    let mut kind = Kind::Plain;
                    for tag in &ts.tag {
                        kind = match tag {
                            RichAnnotation::Link(url) => {
                                if !url.is_empty() && !links.contains(url) {
                                    links.push(url.clone());
                                }
                                Kind::Link
                            }
                            RichAnnotation::Strong => Kind::Strong,
                            RichAnnotation::Emphasis => Kind::Emphasis,
                            RichAnnotation::Code | RichAnnotation::Preformat(_) => Kind::Code,
                            _ => kind,
                        };
                    }
                    (ts.s.clone(), kind)
                })
                .collect()
        })
        .collect();

    // Feeds pad their notes with empty paragraphs.
    let blank = |l: &Vec<(String, Kind)>| l.iter().all(|(s, _)| s.trim().is_empty());
    lines.dedup_by(|a, b| blank(a) && blank(b));
    while lines.first().is_some_and(blank) {
        lines.remove(0);
    }
    while lines.last().is_some_and(blank) {
        lines.pop();
    }
    if !links.is_empty() {
        lines.push(Vec::new());
        lines.push(vec![("Links".to_string(), Kind::Heading)]);
        lines.extend(links.into_iter().map(|url| vec![(url, Kind::Link)]));
    }
    lines
}

pub fn styled(notes: &Notes, theme: &Theme, base: Style) -> Vec<Line<'static>> {
    notes
        .iter()
        .map(|line| {
            Line::from(
                line.iter()
                    .map(|(text, kind)| {
                        let style = match kind {
                            Kind::Plain => base,
                            Kind::Link => theme.accent().add_modifier(Modifier::UNDERLINED),
                            Kind::Strong => base.add_modifier(Modifier::BOLD),
                            Kind::Emphasis => base.add_modifier(Modifier::ITALIC),
                            Kind::Code => theme.accent(),
                            Kind::Heading => theme.heading(),
                        };
                        Span::styled(text.clone(), style)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// A scrolling view of one episode's show notes, laid out again only when the
/// episode or the width changes.
#[derive(Default)]
pub struct NotesView {
    laid_out: Option<(EpisodeKey, u16, Notes)>,
    scroll: usize,
    height: usize,
}

impl NotesView {
    fn scroll_by(&mut self, step: isize) {
        let total = self.laid_out.as_ref().map_or(0, |(_, _, n)| n.len());
        let max = total.saturating_sub(self.height) as isize;
        self.scroll = (self.scroll as isize + step).clamp(0, max) as usize;
    }

    /// Scrolls for movement actions; false for anything else.
    pub fn action(&mut self, action: Action) -> bool {
        let page = self.height.max(1) as isize;
        match action {
            Action::Up => self.scroll_by(-1),
            Action::Down => self.scroll_by(1),
            Action::PageUp => self.scroll_by(-page),
            Action::PageDown => self.scroll_by(page),
            Action::Top => self.scroll = 0,
            Action::Bottom => self.scroll_by(isize::MAX / 2),
            _ => return false,
        }
        true
    }

    pub fn mouse(&mut self, event: MouseEvent) -> bool {
        match event.kind {
            MouseEventKind::ScrollDown => self.scroll_by(3),
            MouseEventKind::ScrollUp => self.scroll_by(-3),
            _ => return false,
        }
        true
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        episode: EpisodeKey,
        html: &str,
        theme: &Theme,
        base: Style,
    ) {
        // Leave a column for the scrollbar.
        let width = area.width.saturating_sub(2);
        let stale = self
            .laid_out
            .as_ref()
            .is_none_or(|(key, w, _)| *key != episode || *w != width);
        if stale {
            if self
                .laid_out
                .as_ref()
                .is_none_or(|(key, _, _)| *key != episode)
            {
                self.scroll = 0;
            }
            self.laid_out = Some((episode, width, layout(html, width)));
        }
        self.height = usize::from(area.height);
        self.scroll_by(0);
        let Some((_, _, notes)) = &self.laid_out else {
            return;
        };
        let lines: Vec<Line> = styled(notes, theme, base)
            .into_iter()
            .skip(self.scroll)
            .take(self.height)
            .collect();
        frame.render_widget(Paragraph::new(lines), area);
        super::scrollbar(frame, area, notes.len(), self.scroll, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(notes: &Notes) -> String {
        notes
            .iter()
            .map(|l| l.iter().map(|(s, _)| s.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn links_are_inline_and_listed_once_at_the_end() {
        let html = "<p>Hello <b>world</b>.</p><p></p><p></p>\
                    <p>See <a href=\"https://example.com\">this</a> and \
                    <a href=\"https://example.com\">that</a>.</p>";
        let notes = layout(html, 40);
        let all = text(&notes);
        assert!(all.starts_with("Hello world."), "{all}");
        assert!(all.contains("See this and that."), "{all}");
        assert_eq!(all.matches("https://example.com").count(), 1, "{all}");
        assert!(!all.contains("\n\n\n"), "{all}");
        assert!(
            notes
                .iter()
                .flatten()
                .any(|(s, k)| s == "world" && *k == Kind::Strong)
        );
        assert!(
            notes
                .iter()
                .flatten()
                .any(|(s, k)| s == "this" && *k == Kind::Link)
        );
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(text(&layout("Just words", 40)), "Just words");
    }
}
