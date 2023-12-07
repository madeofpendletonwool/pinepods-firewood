use std::{
    fs::File,
    io::{BufReader, Cursor},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use lofty::{AudioFile, Probe};
use log::error;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use crate::requests::PinepodsEpisodes;

use super::gen_funcs;

pub struct MusicHandle {
    music_output: Arc<(OutputStream, OutputStreamHandle)>,
    sink: Arc<Sink>,
    song_length: u16,
    time_played: Arc<Mutex<u16>>,
    currently_playing: String,
}

impl Default for MusicHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicHandle {
    pub fn new() -> Self {
        Self {
            music_output: Arc::new(OutputStream::try_default().unwrap()),
            sink: Arc::new(Sink::new_idle().0), // more efficient way, shouldnt have to do twice?
            song_length: 0,
            time_played: Arc::new(Mutex::new(0)),
            currently_playing: "CURRENT SONG".to_string(),
        }
    }

    pub fn currently_playing(&self) -> String {
        self.currently_playing.clone()
    }

    pub fn song_length(&self) -> u16 {
        self.song_length
    }

    pub fn time_played(&self) -> u16 {
        *self.time_played.lock().unwrap()
    }

    pub fn sink_empty(&self) -> bool {
        self.sink.empty()
    }

    pub fn set_time_played(&mut self, t: u16) {
        *self.time_played.lock().unwrap() = t;
    }
    // set currently playing song
    pub fn set_currently_playing(&mut self, path: &PinepodsEpisodes) {
        self.currently_playing = gen_funcs::audio_display(path);
    }

    // update current song and play
    pub fn play(&mut self, episode: &PinepodsEpisodes) {
        // if song already playing, need to be able to restart tho
        // println!("Playing: {}", episode.EpisodeURL.clone());
        error!("Playing: {}", episode.EpisodeURL.clone());
        self.sink.stop();
        *self.time_played.lock().unwrap() = 0;

        // set currently playing
        self.currently_playing = episode.EpisodeTitle.clone();
        self.set_currently_playing(episode);
        self.update_song_length(episode);

        // reinitialize due to rodio crate
        self.sink = Arc::new(Sink::try_new(&self.music_output.1).unwrap());

        // clone sink for thread
        let sclone = self.sink.clone();

        let tpclone = self.time_played.clone();

        let episode_url = episode.EpisodeURL.clone();
        let episode_title = episode.EpisodeTitle.clone();

        let _t1 = thread::spawn(move || {

            // can send in through function
            // get file
            let resp = reqwest::blocking::get(episode_url).unwrap();
            let mut cursor = Cursor::new(resp.bytes().unwrap()); // Adds Read and Seek to the bytes via Cursor
            // let file = BufReader::new(File::open(episode).unwrap());
            let source = Decoder::new(cursor).unwrap();

            // Arc inside a thread inside a thread. BOOM, INCEPTION
            let sink_clone_2 = sclone.clone();
            let tpclone2 = tpclone.clone();

            sclone.append(source);

            let _ = thread::spawn(move || {
                // sleep for 1 second then increment count
                while sink_clone_2.len() == 1 {
                    thread::sleep(Duration::from_secs(1));

                    if !sink_clone_2.is_paused() {
                        *tpclone2.lock().unwrap() += 1;
                    }
                }
            });
            // if sink.stop, thread destroyed.
            sclone.sleep_until_end();
        });
    }

    pub fn play_pause(&mut self) {
        if self.sink.is_paused() {
            self.sink.play()
        } else {
            self.sink.pause()
        }
    }

    pub fn skip(&self) {
        self.sink.stop();
    }

    /// Update `self.song_length` with the provided file.
    pub fn update_song_length(&mut self, episode: &PinepodsEpisodes) {
        // update song length, currently playing
        self.song_length = episode.EpisodeDuration as u16;
    }
}
