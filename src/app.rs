use std::{
    env,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};
use std::sync::{Arc, Mutex};
use log::{info, debug, warn, error};

use pinepods_firewood::gen_funcs;
use pinepods_firewood::music_handler::MusicHandle;
use pinepods_firewood::queue::Queue;
use pinepods_firewood::stateful_list::StatefulList;
use pinepods_firewood::stateful_table::StatefulTable;
use pinepods_firewood::helpers::requests::ReqwestValues;
use pinepods_firewood::requests::{PinepodsEpisodes, PinepodsPodcasts};

#[derive(Clone, Copy)]
pub enum InputMode {
    Browser,
    Queue,
    Controls,
}

/// Represents the active tab state.
#[derive(Debug, Clone, Copy)]
pub enum AppTab {
    Music = 0,
    Controls,
}

impl AppTab {
    /// Get the next tab in the list.
    pub fn next(&self) -> Self {
        match self {
            Self::Music => Self::Controls,
            // Wrap around to the first tab.
            Self::Controls => Self::Music,
        }
    }
}

pub enum ContentState {
    PodcastMode { feed_url: String },
    EpisodeMode { podcast_id: i64 },
    PlayingEpisode { ep_url: String }
}

pub enum BrowserItem {
    Podcast(PinepodsPodcasts),
    Episode(PinepodsEpisodes),
}

pub enum SelectedItem<'a> {
    Podcast(&'a PinepodsPodcasts),
    Episode(&'a PinepodsEpisodes),
}


pub struct App<'a> {
    pub browser_items: StatefulList<BrowserItem>,
    pub queue_items: Queue,
    pub control_table: StatefulTable<'a>,
    pub music_handle: MusicHandle,
    input_mode: InputMode,
    pub titles: Vec<&'a str>,
    pub active_tab: AppTab,
    pub pinepods_values: Arc<Mutex<ReqwestValues>>,
    pub content_state: ContentState,
}

impl<'a> App<'a> {
    pub async fn new(pinepods_values: Arc<Mutex<ReqwestValues>>) -> App<'a> {
        let podcasts = gen_funcs::scan_folder(&pinepods_values).await;
        error!("Fetched podcasts: {:?}", podcasts);
        let podcast_items = podcasts.into_iter()
            .map(BrowserItem::Podcast)
            .collect();


        App {
            browser_items: StatefulList::with_items(podcast_items),
            queue_items: Queue::with_items(),
            control_table: StatefulTable::new(),
            music_handle: MusicHandle::new(),
            input_mode: InputMode::Browser,
            titles: vec!["Podcasts", "Controls"],
            active_tab: AppTab::Music,
            pinepods_values,
            content_state: ContentState::PodcastMode {
                feed_url: String::from("some_feed_url"), // Replace with an actual URL or appropriate default value
            },
        }
    }

    pub fn next(&mut self) {
        self.active_tab = self.active_tab.next();
    }

    pub fn input_mode(&self) -> InputMode {
        self.input_mode
    }

    pub fn set_input_mode(&mut self, in_mode: InputMode) {
        self.input_mode = in_mode
    }

    pub fn current_song(&self) -> String {
        if self.music_handle.sink_empty() && self.queue_items.is_empty() {
            "CURRENT SONG".to_string()
        } else {
            self.music_handle.currently_playing()
        }
    }

    // if item selected is folder, enter folder, else play record.
    pub async fn evaluate(&mut self) {
        match &self.content_state {
            ContentState::PodcastMode {feed_url} => {
                let selected_podcast = match self.browser_items.item() {
                    BrowserItem::Podcast(p) => p,
                    _ => return, // or handle error if necessary
                };
                let podcast_id = selected_podcast.PodcastID.clone();
                self.content_state = ContentState::EpisodeMode { podcast_id: podcast_id.clone() };

                let mut pinepods_values = self.pinepods_values.lock().unwrap();
                match pinepods_values.return_eps(selected_podcast).await {
                    Ok(episodes) => {
                        let episode_items = episodes.into_iter()
                            .map(BrowserItem::Episode)
                            .collect();
                        self.browser_items = StatefulList::with_items(episode_items);
                    },
                    Err(e) => eprintln!("Error fetching episodes: {:?}", e),
                }
            },
            ContentState::EpisodeMode { podcast_id } => {
                let selected_episode = match self.browser_items.item() {
                    BrowserItem::Episode(e) => e,
                    _ => return, // or handle error if necessary
                };
                // Logic to handle episode selection and playback
                // For example, change state to PlayingEpisode or perform other actions
            },
            _ => {
                // Handle other states, like PlayingEpisode
            }
        }
    }
    pub async fn backpedal(&mut self) {

        // Fetch the podcasts and wrap them as BrowserItem
        self.content_state = ContentState::PodcastMode {
            feed_url: String::from("some_feed_url"), // Replace with an actual URL or appropriate default value
        };

        let podcasts = gen_funcs::scan_folder(&self.pinepods_values).await;
        let podcast_items = podcasts.into_iter()
            .map(BrowserItem::Podcast)
            .collect();

        // Update the browser_items with the new list
        self.browser_items = StatefulList::with_items(podcast_items);
        self.browser_items.next();
    }

    // if queue has items and nothing playing, auto play
    pub fn auto_play(&mut self) {
        thread::sleep(Duration::from_millis(250));
        if self.music_handle.sink_empty() && !self.queue_items.is_empty() {
            self.music_handle.set_time_played(0);
            self.music_handle.play(self.queue_items.pop());
        }
    }

    // if playing and
    pub fn song_progress(&mut self) -> u16 {
        let progress = || {
            let percentage =
                (self.music_handle.time_played() * 100) / self.music_handle.song_length();
            if percentage >= 100 {
                100
            } else {
                percentage
            }
        };

        // edge case if nothing queued or playing
        if self.music_handle.sink_empty() && self.queue_items.is_empty() {
            0

            // if something playing, calculate progress
        } else if !self.music_handle.sink_empty() {
            progress()
            // if nothing playing keep rolling
        } else {
            self.auto_play();
            0
        }
    }


    // get file path
    pub fn selected_item(&self) -> Option<SelectedItem> {
        match self.browser_items.item() {
            BrowserItem::Podcast(podcast) => Some(SelectedItem::Podcast(podcast)),
            BrowserItem::Episode(episode) => Some(SelectedItem::Episode(episode)),
        }
    }
}