//! Colors, matched to the PinePods web app's themes.
//!
//! Each web theme is a set of CSS variables ([`Tokens`]). A terminal needs a few
//! distinctions a web page doesn't (a selected row, a dim hint, a border that shows
//! on its background), so [`Theme`] derives those roles from the tokens. Where a token
//! is too close to the background to read in a terminal, it falls back to the next
//! candidate: Github Light's progress bar is #F1F2F3 on white, and most dark themes
//! draw borders in black.

mod builtin;

use ratatui::style::{Color, Modifier, Style};

use builtin::BUILTIN;

/// The theme to use until the server says otherwise; also the web app's default.
pub const DEFAULT_THEME: &str = "Nordic";

/// Uses the terminal's own colors instead of a PinePods palette.
pub const TERMINAL_THEME: &str = "Terminal";

/// One web-app theme, as 0xRRGGBB values named after its CSS variables. Only the
/// variables Firewood uses are kept.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tokens {
    pub name: &'static str,
    pub background: u32,
    pub container: u32,
    pub button: u32,
    pub button_text: u32,
    pub text: u32,
    pub text_secondary: u32,
    pub border: u32,
    pub prog_bar: u32,
    pub error: u32,
    pub standout: u32,
    pub hover: u32,
    pub link: u32,
}

/// What the UI paints with. Every color here is readable on `bg`.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub bg: Color,
    /// Popups and the now-playing bar: a step off the background.
    pub surface: Color,
    pub text: Color,
    /// Secondary text: podcast names, dates.
    pub muted: Color,
    /// Hints, separators and the unplayed part of a progress bar.
    pub faint: Color,
    /// Headings, the current page, links.
    pub accent: Color,
    pub border: Color,
    pub gauge: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    /// The highlighted row of a list.
    pub selected: Style,
    /// The current page in the sidebar, styled like the web app's buttons.
    pub active: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self::named(DEFAULT_THEME, ColorDepth::detect())
    }
}

impl Theme {
    /// Every theme a user can pick, in the web app's order, then the terminal's own.
    pub fn names() -> impl Iterator<Item = &'static str> {
        BUILTIN
            .iter()
            .map(|t| t.name)
            .chain(std::iter::once(TERMINAL_THEME))
    }

    /// Whether `name` is a theme Firewood has. The server also stores users' custom
    /// themes by name, which aren't built in.
    pub fn exists(name: &str) -> bool {
        Self::names().any(|n| n.eq_ignore_ascii_case(name))
    }

    /// The theme called `name` (any case), or the default for a name Firewood
    /// doesn't know.
    pub fn named(name: &str, depth: ColorDepth) -> Self {
        if name.eq_ignore_ascii_case(TERMINAL_THEME) {
            return Self::terminal();
        }
        let tokens = BUILTIN
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .or_else(|| BUILTIN.iter().find(|t| t.name == DEFAULT_THEME))
            .expect("the default theme is built in");
        Self::from_tokens(tokens, depth)
    }

    fn from_tokens(t: &Tokens, depth: ColorDepth) -> Self {
        let bg = Rgb::from(t.background);
        let dark = bg.luminance() < 0.2;
        let first = |candidates: &[Rgb], min: f64| readable(bg, candidates, min);

        // Gruvbox Dark's olive text is 3.4:1 on its background; lift it toward white
        // (or black, on a light theme) until it reads, keeping its hue.
        let toward = if dark { Rgb::WHITE } else { Rgb::BLACK };
        let base_text = Rgb::from(t.text);
        let text = first(
            &[
                base_text,
                t.text_secondary.into(),
                base_text.mix(toward, 0.7),
                base_text.mix(toward, 0.5),
            ],
            4.0,
        );
        let muted = if t.text_secondary == t.text {
            text.mix(bg, 0.7)
        } else {
            first(&[t.text_secondary.into(), text.mix(bg, 0.7)], 3.0)
        };
        let faint = text.mix(bg, 0.45);
        let accent = first(
            &[
                t.link.into(),
                t.standout.into(),
                t.hover.into(),
                t.prog_bar.into(),
                text,
            ],
            2.4,
        );
        let gauge = first(&[t.prog_bar.into(), t.link.into(), accent], 2.0);
        let border = first(&[t.border.into(), text.mix(bg, 0.3)], 1.6);
        let container = Rgb::from(t.container);
        let surface = if container.contrast(bg) >= 1.08 {
            container
        } else {
            text.mix(bg, 0.08)
        };
        let selection = text.mix(bg, 0.14);
        let button = Rgb::from(t.button);
        let button_text = readable(
            button,
            &[t.button_text.into(), bg, text, Rgb::WHITE, Rgb::BLACK],
            4.5,
        );
        let (warning, success) = if dark {
            (Rgb::from(0xFFCC00), Rgb::from(0x30A46C))
        } else {
            (Rgb::from(0xB58900), Rgb::from(0x1E7F4F))
        };
        let error = first(&[t.error.into(), text], 3.0);

        let c = |rgb: Rgb| rgb.to_color(depth);
        Self {
            name: t.name.to_string(),
            bg: c(bg),
            surface: c(surface),
            text: c(text),
            muted: c(muted),
            faint: c(faint),
            accent: c(accent),
            border: c(border),
            gauge: c(gauge),
            error: c(error),
            warning: c(first(&[warning, text], 3.0)),
            success: c(first(&[success, text], 3.0)),
            selected: Style::new()
                .bg(c(selection))
                .fg(c(text))
                .add_modifier(Modifier::BOLD),
            active: Style::new()
                .bg(c(button))
                .fg(c(button_text))
                .add_modifier(Modifier::BOLD),
        }
    }

    /// The terminal's own palette: works on any background and color depth.
    fn terminal() -> Self {
        Self {
            name: TERMINAL_THEME.to_string(),
            bg: Color::Reset,
            surface: Color::Reset,
            text: Color::Reset,
            muted: Color::Gray,
            faint: Color::DarkGray,
            accent: Color::Cyan,
            border: Color::DarkGray,
            gauge: Color::Cyan,
            error: Color::Red,
            warning: Color::Yellow,
            success: Color::Green,
            selected: Style::new().add_modifier(Modifier::REVERSED),
            active: Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        }
    }

    pub fn base(&self) -> Style {
        Style::new().fg(self.text).bg(self.bg)
    }

    pub fn muted(&self) -> Style {
        Style::new().fg(self.muted)
    }

    pub fn faint(&self) -> Style {
        Style::new().fg(self.faint)
    }

    pub fn accent(&self) -> Style {
        Style::new().fg(self.accent)
    }

    pub fn heading(&self) -> Style {
        Style::new().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    pub fn border(&self, focused: bool) -> Style {
        Style::new().fg(if focused { self.accent } else { self.border })
    }

    /// A popup's body.
    pub fn surface(&self) -> Style {
        Style::new().fg(self.text).bg(self.surface)
    }

    /// The key in a "key description" hint.
    pub fn key(&self) -> Style {
        Style::new().fg(self.accent).add_modifier(Modifier::BOLD)
    }
}

/// How many colors the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    /// The xterm 256-color palette. 24-bit colors sent to such a terminal come out
    /// wrong (macOS Terminal before 26 draws them in a handful of basic colors), so
    /// themes are mapped to the nearest palette entry instead.
    Ansi256,
}

impl ColorDepth {
    /// `FIREWOOD_COLOR=truecolor|256` overrides the guess, which goes by the
    /// variables terminals set to advertise 24-bit color.
    pub fn detect() -> Self {
        let var = |name| std::env::var(name).unwrap_or_default().to_ascii_lowercase();
        match var("FIREWOOD_COLOR").as_str() {
            "truecolor" | "24bit" => return Self::TrueColor,
            "256" => return Self::Ansi256,
            _ => {}
        }
        let colorterm = var("COLORTERM");
        let term = var("TERM");
        let program = var("TERM_PROGRAM");
        let truecolor = colorterm.contains("truecolor")
            || colorterm.contains("24bit")
            || term.contains("direct")
            || ["kitty", "alacritty", "ghostty", "wezterm", "foot"]
                .iter()
                .any(|t| term.contains(t))
            || ["iterm.app", "wezterm", "ghostty", "vscode", "warpterminal"]
                .contains(&program.as_str());
        if truecolor {
            Self::TrueColor
        } else {
            Self::Ansi256
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rgb(u8, u8, u8);

impl From<u32> for Rgb {
    fn from(hex: u32) -> Self {
        Self((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }
}

impl Rgb {
    const WHITE: Rgb = Rgb(255, 255, 255);
    const BLACK: Rgb = Rgb(0, 0, 0);

    /// Relative luminance, as WCAG defines it.
    fn luminance(self) -> f64 {
        let channel = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(self.0) + 0.7152 * channel(self.1) + 0.0722 * channel(self.2)
    }

    /// WCAG contrast ratio, from 1 (identical) to 21 (black on white).
    fn contrast(self, other: Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// `amount` of the way from `other` to `self`.
    fn mix(self, other: Rgb, amount: f64) -> Rgb {
        let blend =
            |a: u8, b: u8| (f64::from(a) * amount + f64::from(b) * (1.0 - amount)).round() as u8;
        Rgb(
            blend(self.0, other.0),
            blend(self.1, other.1),
            blend(self.2, other.2),
        )
    }

    fn to_color(self, depth: ColorDepth) -> Color {
        match depth {
            ColorDepth::TrueColor => Color::Rgb(self.0, self.1, self.2),
            ColorDepth::Ansi256 => Color::Indexed(self.nearest_256()),
        }
    }

    /// The closest entry in the xterm palette's color cube or gray ramp (indexes
    /// 16–255; the first 16 vary by terminal).
    fn nearest_256(self) -> u8 {
        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let nearest_level = |c: u8| {
            (0..6)
                .min_by_key(|&i| (i32::from(LEVELS[i]) - i32::from(c)).abs())
                .unwrap_or(0)
        };
        let (r, g, b) = (
            nearest_level(self.0),
            nearest_level(self.1),
            nearest_level(self.2),
        );
        let cube = Rgb(LEVELS[r], LEVELS[g], LEVELS[b]);
        let cube_index = 16 + 36 * r + 6 * g + b;

        let average = (u32::from(self.0) + u32::from(self.1) + u32::from(self.2)) / 3;
        let step = (average.saturating_sub(3) / 10).min(23) as u8;
        let level = 8 + 10 * step;
        let gray = Rgb(level, level, level);

        if self.distance(gray) < self.distance(cube) {
            232 + step
        } else {
            cube_index as u8
        }
    }

    fn distance(self, other: Rgb) -> u32 {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2) as u32;
        d(self.0, other.0) + d(self.1, other.1) + d(self.2, other.2)
    }
}

/// The first candidate with at least `min` contrast against `bg`, or else whichever
/// contrasts most.
fn readable(bg: Rgb, candidates: &[Rgb], min: f64) -> Rgb {
    candidates
        .iter()
        .copied()
        .find(|c| c.contrast(bg) >= min)
        .or_else(|| {
            candidates
                .iter()
                .copied()
                .max_by(|a, b| a.contrast(bg).total_cmp(&b.contrast(bg)))
        })
        .unwrap_or(Rgb::WHITE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(color: Color) -> Rgb {
        match color {
            Color::Rgb(r, g, b) => Rgb(r, g, b),
            other => panic!("expected an RGB color, got {other:?}"),
        }
    }

    #[test]
    fn every_web_theme_is_readable() {
        for tokens in BUILTIN {
            let theme = Theme::from_tokens(tokens, ColorDepth::TrueColor);
            let bg = rgb(theme.bg);
            let name = tokens.name;
            assert!(rgb(theme.text).contrast(bg) >= 4.0, "{name}: text");
            assert!(rgb(theme.muted).contrast(bg) >= 2.5, "{name}: muted");
            assert!(rgb(theme.accent).contrast(bg) >= 2.0, "{name}: accent");
            assert!(rgb(theme.gauge).contrast(bg) >= 2.0, "{name}: gauge");
            assert!(rgb(theme.border).contrast(bg) >= 1.3, "{name}: border");
            let active = theme.active;
            assert!(
                rgb(active.fg.unwrap()).contrast(rgb(active.bg.unwrap())) >= 3.0,
                "{name}: active page"
            );
        }
    }

    #[test]
    fn keeps_the_web_colors_where_they_work() {
        let nordic = Theme::named("nordic", ColorDepth::TrueColor);
        assert_eq!(nordic.name, "Nordic");
        assert_eq!(nordic.bg, Color::Rgb(0x3C, 0x42, 0x52));
        assert_eq!(nordic.text, Color::Rgb(0xF6, 0xF5, 0xF4));
        assert_eq!(nordic.accent, Color::Rgb(0x5D, 0x80, 0xAA));
    }

    #[test]
    fn replaces_a_progress_bar_that_vanishes_into_the_background() {
        let github = Theme::named("Github Light", ColorDepth::TrueColor);
        assert_ne!(github.gauge, Color::Rgb(0xF1, 0xF2, 0xF3));
    }

    #[test]
    fn unknown_names_fall_back_to_the_default() {
        assert_eq!(
            Theme::named("My Custom Theme", ColorDepth::TrueColor).name,
            DEFAULT_THEME
        );
        assert!(Theme::exists("hot dog stand - my eyes"));
        assert!(Theme::exists("terminal"));
        assert!(!Theme::exists("My Custom Theme"));
    }

    #[test]
    fn maps_to_the_256_color_palette() {
        assert_eq!(Rgb(0, 0, 0).nearest_256(), 16);
        assert_eq!(Rgb(255, 255, 255).nearest_256(), 231);
        assert_eq!(Rgb(255, 0, 0).nearest_256(), 196);
        // A mid gray lands on the gray ramp rather than the coarser cube.
        assert_eq!(Rgb(0x80, 0x80, 0x80).nearest_256(), 244);
        let nordic = Theme::named("Nordic", ColorDepth::Ansi256);
        assert!(matches!(nordic.bg, Color::Indexed(_)));
    }
}
