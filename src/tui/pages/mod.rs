//! The screens. Each sidebar entry is a [`Section`] with a stack of [`Page`]s: a
//! podcast's episodes open on top of the podcast list, an episode's details on top
//! of that, and Back pops them.

pub mod devices;
pub mod episode;
pub mod episodes;
pub mod home;
pub mod player;
pub mod podcasts;
pub mod queue;
pub mod search;
pub mod settings;

use ratatui::{
    Frame,
    crossterm::event::{KeyEvent, MouseEvent},
    layout::Rect,
};

use super::keymap::{Action, Context, Keymap};
use super::task::{Bus, Resource};
use crate::api::{Api, Episode};
use crate::nowplaying::NowPlaying;
use crate::playback::{Playback, PlayerState};
use crate::settings::SettingsManager;
use crate::theme::Theme;

/// What pages get to work with.
pub struct Ctx<'a> {
    pub api: &'a Api,
    pub playback: &'a Playback,
    pub remote: &'a NowPlaying,
    pub bus: &'a Bus,
    pub theme: &'a Theme,
    pub keymap: &'a Keymap,
    /// The player as of this frame.
    pub player: &'a PlayerState,
    pub settings: &'a mut SettingsManager,
    /// `user on server`, for Settings.
    pub account: &'a str,
}

/// What handling an input led to.
pub enum Outcome {
    /// Not this page's key; the app tries its global meaning.
    Ignored,
    Handled,
    /// Open a page on top of this one.
    Push(Box<dyn Page>),
    /// Close this page.
    Pop,
}

pub trait Page {
    /// The heading, and the breadcrumb when stacked.
    fn title(&self) -> String;

    /// Keymap contexts, most specific first; [`Context::Global`] is implied.
    fn contexts(&self) -> &'static [Context] {
        &[Context::List]
    }

    /// The actions worth a hint in the footer.
    fn hints(&self) -> &'static [Action] {
        &[]
    }

    /// The page has come into view: load whatever is missing or stale.
    fn show(&mut self, _cx: &Ctx) {}

    /// Called every frame while the page is open, to take in finished loads.
    fn tick(&mut self, _cx: &Ctx) {}

    fn refresh(&mut self, _cx: &Ctx) {}

    /// Data of this kind changed somewhere; reload it next time it's shown.
    fn changed(&mut self, _resource: Resource) {}

    /// A load is running (for the spinner beside the title).
    fn busy(&self) -> bool {
        false
    }

    /// A text field has the keyboard, so single-key shortcuts are off.
    fn typing(&self) -> bool {
        false
    }

    /// Sees each key before the keymap does; `Some` means it was dealt with.
    fn key(&mut self, _key: KeyEvent, _cx: &mut Ctx) -> Option<Outcome> {
        None
    }

    fn action(&mut self, action: Action, cx: &mut Ctx) -> Outcome;

    fn mouse(&mut self, _event: MouseEvent, _cx: &mut Ctx) -> Outcome {
        Outcome::Ignored
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, cx: &Ctx);

    /// The episode under the cursor, for "Play on device".
    fn selected_episode(&self) -> Option<Episode> {
        None
    }
}

/// A sidebar entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Home,
    Feed,
    Podcasts,
    Queue,
    Saved,
    Downloads,
    Search,
    Devices,
    Player,
    Settings,
}

impl Section {
    pub const ALL: [Section; 10] = [
        Self::Home,
        Self::Feed,
        Self::Podcasts,
        Self::Queue,
        Self::Saved,
        Self::Downloads,
        Self::Search,
        Self::Devices,
        Self::Player,
        Self::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Feed => "Feed",
            Self::Podcasts => "Podcasts",
            Self::Queue => "Queue",
            Self::Saved => "Saved",
            Self::Downloads => "Downloads",
            Self::Search => "Search",
            Self::Devices => "Devices",
            Self::Player => "Now Playing",
            Self::Settings => "Settings",
        }
    }

    /// The name `ui.tab_order` in the settings file uses.
    pub fn setting_name(self) -> &'static str {
        match self {
            Self::Player => "Player",
            other => other.title(),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Home => "⌂",
            Self::Feed => "≋",
            Self::Podcasts => "◉",
            Self::Queue => "≡",
            Self::Saved => "★",
            Self::Downloads => "↓",
            Self::Search => "⌕",
            Self::Devices => "⇄",
            Self::Player => "♪",
            Self::Settings => "⚙",
        }
    }

    /// Sections in the user's order; any the setting leaves out go at the end.
    pub fn ordered(names: &[String]) -> Vec<Section> {
        let mut order: Vec<Section> = Vec::new();
        let named = names
            .iter()
            .filter_map(|n| Self::ALL.into_iter().find(|s| s.setting_name() == n));
        for section in named.chain(Self::ALL) {
            if !order.contains(&section) {
                order.push(section);
            }
        }
        order
    }

    pub fn root(self) -> Box<dyn Page> {
        match self {
            Self::Home => Box::new(home::HomePage::default()),
            Self::Feed => Box::new(episodes::EpisodeListPage::feed()),
            Self::Podcasts => Box::new(podcasts::PodcastsPage::default()),
            Self::Queue => Box::new(queue::QueuePage::default()),
            Self::Saved => Box::new(episodes::EpisodeListPage::saved()),
            Self::Downloads => Box::new(episodes::EpisodeListPage::downloads()),
            Self::Search => Box::new(search::SearchPage::default()),
            Self::Devices => Box::new(devices::DevicesPage::default()),
            Self::Player => Box::new(player::PlayerPage::default()),
            Self::Settings => Box::new(settings::SettingsPage::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_follows_the_setting_and_keeps_every_section() {
        let names = ["Queue", "Player", "Nonsense", "Queue"].map(String::from);
        let order = Section::ordered(&names);
        assert_eq!(&order[..2], &[Section::Queue, Section::Player]);
        assert_eq!(order.len(), Section::ALL.len());
    }
}
