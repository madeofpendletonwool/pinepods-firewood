//! Cover art on screen, through whichever graphics protocol the terminal speaks
//! (kitty, sixel or iTerm2), or as half-block characters when it speaks none.
//!
//! Turning a cover into the protocol's data for a given size is slow (resizing,
//! then PNG or sixel encoding), so it happens in the background; until it's done
//! the last cover drawn stays up, or a placeholder.

use std::time::Duration;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect, Size},
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph},
};
use ratatui_image::picker::cap_parser::QueryStdioOptions;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::{Image, Resize};

use crate::art::{Art, Artwork};
use crate::settings::CoverArt;
use crate::theme::Theme;
use crate::tui::task::Pending;

/// How long to wait for the terminal to say what it supports.
const QUERY_TIMEOUT: Duration = Duration::from_millis(800);

/// `blocks` skips asking the terminal and draws covers in half blocks; `kitty`,
/// `sixel` or `iterm2` use that protocol whatever the terminal says (useful in tmux,
/// which hides the terminal outside it).
pub const GRAPHICS_ENV: &str = "FIREWOOD_GRAPHICS";

/// Asks the terminal which graphics protocol it supports and how big its cells are.
/// Call it once, after the terminal is in raw mode and before reading any input:
/// the answer arrives on stdin. A terminal that doesn't answer gets half blocks (and
/// loses the first key typed, which the unanswered query reads).
pub fn detect() -> Picker {
    let forced = std::env::var(GRAPHICS_ENV)
        .ok()
        .map(|v| v.to_ascii_lowercase());
    let forced = match forced.as_deref() {
        None | Some("") | Some("auto") => None,
        Some("blocks" | "halfblocks") => return Picker::halfblocks(),
        Some("kitty") => Some(ProtocolType::Kitty),
        Some("sixel") => Some(ProtocolType::Sixel),
        Some("iterm2" | "iterm") => Some(ProtocolType::Iterm2),
        Some(other) => {
            tracing::warn!(
                "{GRAPHICS_ENV}={other:?} isn't one of auto, blocks, kitty, sixel or iterm2"
            );
            None
        }
    };
    let options = QueryStdioOptions {
        timeout: QUERY_TIMEOUT,
        ..QueryStdioOptions::default()
    };
    match Picker::from_query_stdio_with_options(options) {
        Ok(mut picker) => {
            if let Some(protocol) = forced {
                picker.set_protocol_type(protocol);
            }
            tracing::info!(
                protocol = ?picker.protocol_type(),
                font = ?picker.font_size(),
                tmux = picker.tmux_detected(),
                "graphics",
            );
            picker
        }
        Err(e) => {
            tracing::warn!("couldn't ask the terminal about graphics: {e}");
            Picker::halfblocks()
        }
    }
}

/// What every cover on screen shares: the image cache and how to draw.
pub struct Covers {
    pub artwork: Artwork,
    picker: Picker,
}

impl Covers {
    pub fn new(artwork: Artwork, picker: Picker) -> Self {
        Self { artwork, picker }
    }

    /// A real image protocol, not half blocks.
    pub fn has_graphics(&self) -> bool {
        self.picker.protocol_type() != ProtocolType::Halfblocks
    }

    /// How the setting reads: `auto (kitty)`.
    pub fn describe(&self, setting: CoverArt) -> String {
        match setting {
            CoverArt::Off => "off".into(),
            CoverArt::Auto => {
                let how = match self.picker.protocol_type() {
                    ProtocolType::Kitty => "kitty",
                    ProtocolType::Sixel => "sixel",
                    ProtocolType::Iterm2 => "iTerm2",
                    ProtocolType::Halfblocks => "blocks",
                };
                format!("auto ({how})")
            }
        }
    }

    /// Whether drawing over a cover can leave pieces of it behind. Kitty images are
    /// placed with characters, so they come and go with the text; sixel and iTerm2
    /// images are painted once and stay until something overwrites every pixel.
    pub fn leaves_traces(&self) -> bool {
        matches!(
            self.picker.protocol_type(),
            ProtocolType::Sixel | ProtocolType::Iterm2
        )
    }
}

/// One place a cover is drawn.
#[derive(Default)]
pub struct CoverView {
    /// The cover on screen: its url, the area it was made for, and the data.
    shown: Option<(String, Size, Protocol)>,
    making: Option<(String, Size, Pending<Option<Protocol>>)>,
}

impl CoverView {
    /// Draws the cover at `url` fitted and centered in `area`.
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        url: &str,
        covers: &Covers,
        theme: &Theme,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let size = area.as_size();
        if let Some((_, _, pending)) = &mut self.making
            && let Some(made) = pending.poll()
        {
            let (url, size, _) = self.making.take().expect("checked above");
            self.shown = made.map(|protocol| (url, size, protocol));
        }

        let cover = match covers.artwork.get(url) {
            Art::Ready(cover) => cover,
            Art::Loading | Art::Missing => {
                self.shown = None;
                placeholder(frame, area, theme);
                return;
            }
        };
        let current = self
            .shown
            .as_ref()
            .is_some_and(|(u, s, _)| u == url && *s == size);
        let underway = self
            .making
            .as_ref()
            .is_some_and(|(u, s, _)| u == url && *s == size);
        if !current && !underway {
            let (picker, image) = (covers.picker.clone(), cover.image.clone());
            let pending = Pending::spawn(async move {
                tokio::task::spawn_blocking(move || {
                    picker
                        .new_protocol(image, size, Resize::Fit(None))
                        .map_err(|e| tracing::warn!("couldn't prepare a cover: {e}"))
                        .ok()
                })
                .await
                .ok()
                .flatten()
            });
            self.making = Some((url.to_string(), size, pending));
        }

        // An older size of the same cover is fine while the new one is made, as
        // long as it fits.
        match &self.shown {
            Some((u, _, protocol)) if u == url => {
                let fits = protocol.size();
                if fits.width <= area.width && fits.height <= area.height {
                    frame.render_widget(Image::new(protocol), centered(area, fits));
                } else {
                    placeholder(frame, area, theme);
                }
            }
            _ => placeholder(frame, area, theme),
        }
    }
}

fn centered(area: Rect, size: Size) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(size.height)])
        .flex(Flex::Center)
        .areas(area);
    let [rect] = Layout::horizontal([Constraint::Length(size.width)])
        .flex(Flex::Center)
        .areas(row);
    rect
}

/// A quiet frame with a note in it, where a cover would go.
fn placeholder(frame: &mut Frame, area: Rect, theme: &Theme) {
    if area.width < 5 || area.height < 3 {
        frame.render_widget(
            Paragraph::new(Line::styled("♪", theme.faint())).centered(),
            centered(area, Size::new(area.width, 1)),
        );
        return;
    }
    // Square-ish, like a cover: cells are about twice as tall as they're wide.
    let width = area.width.min(area.height * 2);
    let rect = centered(area, Size::new(width, area.height.min(width.div_ceil(2))));
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.faint());
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(Line::styled("♪", theme.faint())).centered(),
        centered(inner, Size::new(inner.width, 1)),
    );
}
