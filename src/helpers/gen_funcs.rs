use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use std::sync::{Arc, Mutex};

use glob::{glob_with, MatchOptions};
use lofty::{Accessor, Probe, TaggedFileExt};

use log::error;
use crate::requests::{PinepodsEpisodes, PinepodsPodcasts};

// converts queue items to what's displayed for user
pub fn audio_display(episode: &PinepodsEpisodes) -> String {
    return format!("{:?} - {}", episode.PodcastName, episode.EpisodeTitle);
}

// scans folder for valid files, returns matches
pub async fn scan_folder(pinepods_values: &Arc<Mutex<super::requests::ReqwestValues>>) -> Vec<PinepodsPodcasts> {
    error!("before lock...");

    let result = {
        let pinepods_locked = pinepods_values.lock().expect("Lock is poisoned!");
        pinepods_locked.return_pods().await
    };

    match result {
        Ok(podcasts) => {
            error!("pods return finished...");
            podcasts
        },
        Err(e) => {
            eprintln!("Request failed: {:?}", e);
            Vec::new() // return empty list on error
        }
    }
}

pub fn display_podcast_details(podcast: &serde_json::Value) {
    if let Some(podcast_name) = podcast["PodcastName"].as_str() {
        println!("Podcast Name: {}", podcast_name);
    }
    if let Some(author) = podcast["Author"].as_str() {
        println!("Author: {}", author);
    }
    // ... add other fields similarly
}

// Example of usage:

// let podcast_names = list_podcasts().await;
// for (index, name) in podcast_names.iter().enumerate() {
// println!("{}: {}", index + 1, name);
// }
//
// println!("Enter the number of the podcast you want to explore:");
// let mut input = String::new();
// io::stdin().read_line(&mut input).unwrap();
// let choice: usize = input.trim().parse().unwrap_or(0);
//
// if choice > 0 && choice <= podcast_names.len() {
// let selected_podcast = &podcasts[choice - 1];
// display_podcast_details(&selected_podcast);
// } else {
// println!("Invalid choice!");
// }


// scans folder for valid files, returns matches
// need to set current dir
pub fn bulk_add(selected: &PathBuf) -> Vec<PathBuf> {
    let mut items = Vec::new();
    env::set_current_dir(selected).unwrap();

    for item in glob::glob("./*")
        .expect("Failed to read glob pattern")
        .flatten()
    {
        let current_dir = env::current_dir().unwrap();
        let join = Path::join(&current_dir, Path::new(&item));
        let ext = Path::new(&item).extension().and_then(OsStr::to_str);
        if ext.is_some()
            && (item.extension().unwrap() == "mp3"
            || item.extension().unwrap() == "mp4"
            || item.extension().unwrap() == "m4a"
            || item.extension().unwrap() == "wav"
            || item.extension().unwrap() == "flac"
            || item.extension().unwrap() == "ogg"
            || item.extension().unwrap() == "aac")
        {
            items.push(join);
        }
    }
    env::set_current_dir("../").unwrap();
    items
}
