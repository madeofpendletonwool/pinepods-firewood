use std::{
    env,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};
use std::sync::{Arc, Mutex};

use pinepods_firewood::gen_funcs;
use pinepods_firewood::music_handler::MusicHandle;
use pinepods_firewood::queue::Queue;
use pinepods_firewood::stateful_list::StatefulList;
use pinepods_firewood::stateful_table::StatefulTable;
use crate::helpers::requests::ReqwestValues;

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

pub struct App<'a> {
    pub browser_items: StatefulList<String>,
    pub queue_items: Queue,
    pub control_table: StatefulTable<'a>,
    pub music_handle: MusicHandle,
    input_mode: InputMode,
    pub titles: Vec<&'a str>,
    pub active_tab: AppTab,
    pub pinepods_values: Arc<Mutex<super::helpers::requests::ReqwestValues>>,
}

impl<'a> App<'a> {
    pub async fn new(pinepods_values: Arc<Mutex<ReqwestValues>>) -> Self {
        Self {
            browser_items: StatefulList::with_items(gen_funcs::scan_folder(gen_funcs::scan_folder(&pinepods_values))),
            queue_items: Queue::with_items(),
            control_table: StatefulTable::new(),
            music_handle: MusicHandle::new(),
            input_mode: InputMode::Browser,
            titles: vec!["Music", "Controls"],
            active_tab: AppTab::Music,
            pinepods_values
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
    pub fn evaluate(&mut self) {
        let join = self.selected_item();
        // if folder enter, else play song
        if join.is_dir() {
            env::set_current_dir(join).unwrap();
            self.browser_items = StatefulList::with_items(gen_funcs::scan_folder(&self.pinepods_values));
            self.browser_items.next();
        } else {
            self.music_handle.play(join);
        }
    }

    // cd into selected directory
    pub fn backpedal(&mut self) {
        env::set_current_dir("../").unwrap();
        self.browser_items = StatefulList::with_items(gen_funcs::scan_folder(&self.pinepods_values));
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
    pub fn selected_item(&self) -> PathBuf {
        let current_dir = env::current_dir().unwrap();
        if self.browser_items.empty() {
            Path::new(&current_dir).into()
        } else {
            let join = Path::join(&current_dir, Path::new(&self.browser_items.item()));
            join
        }
    }
}
