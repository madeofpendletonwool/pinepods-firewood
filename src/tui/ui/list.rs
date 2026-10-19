//! A scrolling, filterable list that pages keep their rows in.
//!
//! The page owns the items; [`ListView`] owns the selection, scroll position and
//! filter, and draws whatever rows the page builds for the visible slice. Typing `/`
//! opens the filter: every word typed must appear in an item's text, in any order.

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{Constraint, Flex, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
};

use crate::theme::Theme;
use crate::tui::keymap::Action;

/// Columns between table cells.
const COLUMN_SPACING: u16 = 2;

/// What a click on the list did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Selected a row (or scrolled).
    Moved,
    /// Clicked the row that was already selected.
    Activated,
    Missed,
}

#[derive(Debug, Default)]
pub struct ListView {
    /// Index into the visible (filtered) rows.
    selected: usize,
    offset: usize,
    /// Indexes of the items that pass the filter, in order.
    visible: Vec<usize>,
    query: String,
    /// The filter box has the keyboard.
    typing: bool,
    /// Where the rows were last drawn, for mouse clicks and page jumps.
    rows_area: Rect,
    /// Another list on the same page has the keyboard, so don't show a selection.
    unfocused: bool,
    /// Lines per row, less one: rows are one line unless set.
    extra_lines: u16,
}

impl ListView {
    /// Recomputes which items pass the filter. Call whenever the items change; the
    /// selection stays on the same item where it can.
    pub fn sync<T>(&mut self, items: &[T], text: impl Fn(&T) -> String) {
        let previous = self.selected_index();
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.visible = items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if words.is_empty() {
                    return true;
                }
                let haystack = text(item).to_lowercase();
                words.iter().all(|w| haystack.contains(w.as_str()))
            })
            .map(|(i, _)| i)
            .collect();
        self.selected = previous
            .and_then(|p| self.visible.iter().position(|&i| i == p))
            .unwrap_or(self.selected)
            .min(self.visible.len().saturating_sub(1));
    }

    /// The selected item's index into the page's items.
    pub fn selected_index(&self) -> Option<usize> {
        self.visible.get(self.selected).copied()
    }

    pub fn select_index(&mut self, index: usize) {
        if let Some(pos) = self.visible.iter().position(|&i| i == index) {
            self.selected = pos;
        }
    }

    pub fn visible(&self) -> &[usize] {
        &self.visible
    }

    pub fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    pub fn is_filtered(&self) -> bool {
        !self.query.is_empty()
    }

    pub fn is_typing(&self) -> bool {
        self.typing
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Rows `lines` tall, for lists of cards.
    pub fn with_row_height(mut self, lines: u16) -> Self {
        self.extra_lines = lines.saturating_sub(1);
        self
    }

    fn row_height(&self) -> u16 {
        self.extra_lines + 1
    }

    /// How many rows fit on screen.
    fn rows_shown(&self) -> usize {
        usize::from(self.rows_area.height / self.row_height())
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.unfocused = !focused;
    }

    pub fn start_filter(&mut self) {
        self.typing = true;
    }

    pub fn clear_filter(&mut self) {
        self.query.clear();
        self.typing = false;
    }

    /// Handles a key while the filter box has focus. Returns whether the query
    /// changed, so the page can [`sync`](Self::sync).
    pub fn filter_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.clear_filter();
                true
            }
            KeyCode::Enter | KeyCode::Down | KeyCode::Up | KeyCode::Tab => {
                // Keep the filter, give the keys back to the list.
                self.typing = false;
                if key.code == KeyCode::Down {
                    self.move_by(1);
                } else if key.code == KeyCode::Up {
                    self.move_by(-1);
                }
                false
            }
            KeyCode::Backspace => {
                if self.query.pop().is_none() {
                    self.typing = false;
                }
                true
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.clear();
                true
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let trimmed = self.query.trim_end().len();
                self.query.truncate(
                    self.query[..trimmed]
                        .rfind(char::is_whitespace)
                        .map_or(0, |i| i + 1),
                );
                true
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.push(c);
                self.selected = 0;
                true
            }
            _ => false,
        }
    }

    pub fn move_by(&mut self, step: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() as isize - 1;
        self.selected = (self.selected as isize + step).clamp(0, last) as usize;
    }

    /// Applies a movement action; returns false for anything else.
    pub fn navigate(&mut self, action: Action) -> bool {
        let page = self.rows_shown().max(1) as isize;
        match action {
            Action::Up => self.move_by(-1),
            Action::Down => self.move_by(1),
            Action::PageUp => self.move_by(-page),
            Action::PageDown => self.move_by(page),
            Action::Top => self.selected = 0,
            Action::Bottom => self.selected = self.visible.len().saturating_sub(1),
            _ => return false,
        }
        true
    }

    pub fn mouse(&mut self, event: MouseEvent) -> Click {
        let area = self.rows_area;
        let inside = event.column >= area.x
            && event.column < area.right()
            && event.row >= area.y
            && event.row < area.bottom();
        match event.kind {
            MouseEventKind::ScrollDown if inside => {
                self.move_by(3);
                Click::Moved
            }
            MouseEventKind::ScrollUp if inside => {
                self.move_by(-3);
                Click::Moved
            }
            MouseEventKind::Down(MouseButton::Left) if inside => {
                let row = self.offset + usize::from((event.row - area.y) / self.row_height());
                if row >= self.visible.len() {
                    return Click::Missed;
                }
                if row == self.selected {
                    Click::Activated
                } else {
                    self.selected = row;
                    Click::Moved
                }
            }
            _ => Click::Missed,
        }
    }

    /// Draws the filter line (when there is one) and the rows. `row` builds the cells
    /// for an item index; `header` labels the columns.
    pub fn render<'a>(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        widths: &[Constraint],
        header: Option<Row<'a>>,
        mut row: impl FnMut(usize, &[u16]) -> Vec<Cell<'a>>,
    ) {
        let mut area = area;
        if self.typing || self.is_filtered() {
            let [filter, rest] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
            self.render_filter(frame, filter, theme);
            area = rest;
        }
        let (head, rows_area) = if header.is_some() {
            let [head, rest] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
            (Some(head), rest)
        } else {
            (None, area)
        };
        self.rows_area = rows_area;

        let height = self.rows_shown();
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if height > 0 && self.selected >= self.offset + height {
            self.offset = self.selected + 1 - height;
        }
        self.offset = self.offset.min(self.visible.len().saturating_sub(height));

        // Keep the last column clear of the scrollbar.
        let scrolls = self.visible.len() > height;
        let narrow = |r: Rect| Rect {
            width: r.width.saturating_sub(u16::from(scrolls)),
            ..r
        };
        let table = |rows: Vec<Row<'a>>| {
            Table::new(rows, widths.to_vec())
                .column_spacing(COLUMN_SPACING)
                .flex(Flex::Start)
        };
        let column_widths: Vec<u16> = Layout::horizontal(widths.to_vec())
            .spacing(COLUMN_SPACING)
            .flex(Flex::Start)
            .split(narrow(rows_area))
            .iter()
            .map(|r| r.width)
            .collect();

        if let (Some(header), Some(head)) = (header, head) {
            frame.render_widget(
                table(vec![header]).style(theme.faint().add_modifier(Modifier::BOLD)),
                narrow(head),
            );
        }
        let rows: Vec<Row> = self
            .visible
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(height)
            .map(|(pos, &index)| {
                let r = Row::new(row(index, &column_widths)).height(self.row_height());
                if pos == self.selected && !self.unfocused {
                    r.style(theme.selected)
                } else {
                    r
                }
            })
            .collect();
        frame.render_widget(table(rows).style(theme.base()), narrow(rows_area));

        if scrolls {
            super::scrollbar(
                frame,
                Rect {
                    height: height as u16 * self.row_height(),
                    ..rows_area
                },
                self.visible.len() * usize::from(self.row_height()),
                self.offset * usize::from(self.row_height()),
                theme,
            );
        }
    }

    fn render_filter(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut spans = vec![
            Span::styled("/", theme.key()),
            Span::styled(self.query.clone(), theme.base()),
        ];
        if self.typing {
            spans.push(Span::styled("▏", theme.accent()));
        }
        spans.push(Span::styled(
            format!(
                "  {} match{}",
                self.visible.len(),
                if self.visible.len() == 1 { "" } else { "es" }
            ),
            theme.faint(),
        ));
        if self.typing {
            spans.push(Span::styled("  Enter keep · Esc clear", theme.faint()));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        if self.typing {
            let x = area.x + 1 + unicode_width::UnicodeWidthStr::width(self.query.as_str()) as u16;
            frame.set_cursor_position((x.min(area.right().saturating_sub(1)), area.y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn typed(list: &mut ListView, items: &[&str], text: &str) {
        list.start_filter();
        for c in text.chars() {
            list.filter_key(key(KeyCode::Char(c)));
        }
        list.sync(items, |s| s.to_string());
    }

    #[test]
    fn filters_on_every_word_in_any_order() {
        let items = [
            "The Daily",
            "Daily Tech News",
            "Hardcore History",
            "Tech Talk",
        ];
        let mut list = ListView::default();
        list.sync(&items, |s| s.to_string());
        assert_eq!(list.visible(), &[0, 1, 2, 3]);

        typed(&mut list, &items, "tech daily");
        assert_eq!(list.visible(), &[1]);
        assert_eq!(list.selected_index(), Some(1));
    }

    #[test]
    fn selection_survives_filtering_and_shrinking() {
        let mut items = vec!["a", "b", "c"];
        let mut list = ListView::default();
        list.sync(&items, |s| s.to_string());
        list.move_by(2);
        typed(&mut list, &items, "c");
        list.filter_key(key(KeyCode::Esc));
        list.sync(&items, |s| s.to_string());
        assert_eq!(list.selected_index(), Some(2));
        items.truncate(1);
        list.sync(&items, |s| s.to_string());
        assert_eq!(list.selected_index(), Some(0));
    }

    #[test]
    fn escape_clears_and_enter_keeps_the_filter() {
        let items = ["alpha", "beta"];
        let mut list = ListView::default();
        typed(&mut list, &items, "al");
        list.filter_key(key(KeyCode::Enter));
        assert!(!list.is_typing());
        assert!(list.is_filtered());
        list.start_filter();
        list.filter_key(key(KeyCode::Esc));
        list.sync(&items, |s| s.to_string());
        assert!(!list.is_filtered());
        assert_eq!(list.visible().len(), 2);
    }

    #[test]
    fn navigation_stays_in_bounds() {
        let items = ["a", "b", "c"];
        let mut list = ListView::default();
        list.sync(&items, |s| s.to_string());
        list.navigate(Action::Bottom);
        assert_eq!(list.selected_index(), Some(2));
        list.navigate(Action::Down);
        assert_eq!(list.selected_index(), Some(2));
        list.navigate(Action::Top);
        list.navigate(Action::Up);
        assert_eq!(list.selected_index(), Some(0));
    }
}
