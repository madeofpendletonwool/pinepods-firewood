use std::collections::HashMap;
use reqwest;
use tokio;
use anyhow::{Result, anyhow};
use serde_json::Value;
use std::fs::create_dir_all;
use directories::{ProjectDirs};
use std::path::{Display, Path, PathBuf};
use std::fs;
use std::pin::pin;
use serde::Deserialize;
use serde_derive::Serialize;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use super::models;
use log::error;
use std::error::Error;

#[derive(Debug)]
pub enum PinepodsError {
    Reqwest(reqwest::Error),
    Serde(serde_json::Error),
}

impl From<reqwest::Error> for PinepodsError {
    fn from(err: reqwest::Error) -> PinepodsError {
        PinepodsError::Reqwest(err)
    }
}

impl From<serde_json::Error> for PinepodsError {
    fn from(err: serde_json::Error) -> PinepodsError {
        PinepodsError::Serde(err)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PinepodsConfig {
    pub url: String,
    pub api_key: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EpisodeRequest {
    pub user_id: i64,
    pub podcast_id: i64
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PinepodsPodcasts {
    pub PodcastID: i64,  // Assuming integers, change to i32 if the range is smaller
    pub PodcastName: String,
    pub ArtworkURL: String,
    pub Author: String,
    pub Categories: String, // Change to Vec<String> if it's actually a JSON array
    pub EpisodeCount: u32, // Assuming integers
    pub FeedURL: String,
    pub WebsiteURL: String,
    pub Description: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PinepodsEpisodes {
    pub PodcastName: Option<String>, // Optional if it might not always be present
    pub EpisodeTitle: String,
    pub EpisodePubDate: String,
    pub EpisodeDescription: String,
    pub EpisodeArtwork: String,
    pub EpisodeURL: String,
    pub EpisodeDuration: i64, // Assuming this is an integer value for duration in seconds
    // Optional fields
    pub ListenDuration: Option<i64>, // Assuming this is an integer value for listened duration in seconds
    // If you still need EpisodeID and PodcastID, you can include them as optional
    pub EpisodeID: Option<String>,
    pub PodcastID: Option<String>,
}

// Temporary struct to match the JSON response
#[derive(Debug, Deserialize)]
struct TempPodcast {
    PodcastName: String,
    ArtworkURL: String,
    Description: String, // Add this field if needed in your struct
    EpisodeCount: u32,
    WebsiteURL: String,
    FeedURL: String,
    Author: String,
    Categories: String,
    PodcastID: i64,
}

async fn verify_existing_key(hostname: &String, api_key: &String) -> Result<models::PinepodsUserResponse, PinepodsError> {

    let key_verify_url = &format!("{}{}", &hostname, "/api/data/get_user");
    let client = reqwest::Client::new();
    let response = client
        .get(key_verify_url)
        .header("Api-Key", api_key.trim().to_string())
        .send().await?;

    // Read the response body as a string
    let raw_response = response.text().await?;

    // Print the raw response
    println!("Raw Response: {}", raw_response);

    // Now parse the raw response into your desired structure
    let parsed_data: models::PinepodsUserResponse = serde_json::from_str(&raw_response)?;

    Ok(parsed_data)
}

fn get_app_path() -> Option<PathBuf> {
    if let Some(proj_dirs) = ProjectDirs::from("org", "Gooseberry Development",  "Pinepods") {
        Some(proj_dirs.config_dir().to_path_buf())
    } else {
        None
    }
}

fn create_app_path(app_path: &Display) -> std::io::Result<()> {
    create_dir_all(app_path.to_string())?;
    Ok(())
}

pub async fn test_existing_config () -> std::io::Result<PinepodsConfig> {
    return if let Some(app_path) = get_app_path() {
        let mut config_path = app_path.join("pinepods_config.json");
        let config_data: String = fs::read_to_string(&config_path)?;
        let config_json: serde_json::Value = serde_json::from_str(&config_data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        dbg!(&config_json);
        let pinepods_config: PinepodsConfig = serde_json::from_value(config_json)
            .expect("Failed to convert JSON value to Config Struct");
        let return_verify_login = verify_existing_key(&pinepods_config.url, &pinepods_config.api_key).await;
        match return_verify_login {
            Ok(data) => {}
            Err(data) => {}
        };
        Ok(pinepods_config)
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "App Path not found"))
    }
}

pub struct ReqwestValues {
    pub url: String,
    pub api_key: String,
    pub user_id: i64,
}

impl ReqwestValues {

    pub async fn make_request(&self) -> Result<models::PinepodsCheck, PinepodsError> {
    let client = reqwest::Client::new();
        let make_request_url = &format!("{}{}", &*self.url, "/api/pinepods_check");
        let response = client.get(make_request_url).send().await?;

        let raw_response = response.text().await?;

        let parsed_data: models::PinepodsCheck = serde_json::from_str(&raw_response)?;

        Ok(parsed_data)
    }

    pub async fn verify_key(&self) -> Result<models::PinepodsUserResponse, PinepodsError> {
        let key_verify_url = &format!("{}{}", self.url, "/api/data/get_user");
        let client = reqwest::Client::new();
        let response = client
            .get(key_verify_url)
            .header("Api-Key", &self.api_key.trim().to_string())
            .send().await?;

        // Read the response body as a string
        let raw_response = response.text().await?;

        // Print the raw response

        // Now parse the raw response into your desired structure
        let parsed_data: models::PinepodsUserResponse = serde_json::from_str(&raw_response)?;

        Ok(parsed_data)
    }

    pub async fn store_pinepods_info(&self) -> std::io::Result<()> {
        if let Some(app_path) = get_app_path() {
            use std::path::Path;
            use dirs::home_dir;
            if let Some(home_directory) = home_dir() {
                let config_path = home_directory.join(".config");
                let pinepods_path = config_path.join("pinepods");

                if config_path.exists() && pinepods_path.exists() {
                    println!("Both .config and .config/pinepods exist");
                } else if config_path.exists() {
                    println!("Just the pinepods config folder is missing");
                    fs::create_dir(pinepods_path)?;
                } else {
                    println!("One or both of the directories do not exist");
                    println!("{:?}", pinepods_path);
                    fs::create_dir(config_path)?;
                    fs::create_dir(pinepods_path)?;
                }
            } else {
                println!("Could not determine the home directory");
            }
            let config_path = app_path.join("pinepods_config.json");
            println!("{:?}", &config_path);

            let login_info = PinepodsConfig {
                url: (&self.url.clone()).parse().unwrap(),
                api_key: (&self.api_key.clone()).parse().unwrap()
            };
            let json = serde_json::to_string(&login_info)?;

            let mut file = File::create(config_path).await?;
            file.write_all(json.as_bytes()).await?;
        } else {
            println!("Failed to get Config Path");
        }
        Ok(())
    }

    pub async fn get_userid(&self) -> Result<i64> {
        let client = reqwest::Client::new();
        let response = client
            .get(&format!("{}/api/data/get_user", &self.url)) // Format the URL
            .header("Api-Key", &self.api_key.trim().to_string()) // Add the API key to the headers
            .send()
            .await?;

        if response.status().is_success() {
            let json: Value = response.json().await?;
            Ok(json["retrieved_id"].as_i64().unwrap_or_default())
        } else {
            eprintln!(
                "Error fetching podcasts: {}",
                response.status()
            );
            Err(anyhow!("Error Fetching pods"))
        }
    }

    pub async fn return_pods(&self) -> anyhow::Result<Vec<PinepodsPodcasts>> {
        let client = reqwest::Client::new();
        let response = client
            .get(&format!("{}/api/data/return_pods/{}", &self.url, &self.user_id))
            .header("Api-Key", &self.api_key.trim().to_string())
            .send()
            .await?;

        if response.status().is_success() {
            let temp_response: HashMap<String, Vec<TempPodcast>> = response.json().await?;
            // Bind the empty vector to a variable
            let empty_vec = vec![];
            let temp_podcasts = temp_response.get("pods").unwrap_or(&empty_vec);

            let podcasts = temp_podcasts.iter().map(|temp_pod| {
                PinepodsPodcasts {
                    PodcastID: temp_pod.PodcastID.clone(),
                    PodcastName: temp_pod.PodcastName.clone(),
                    ArtworkURL: temp_pod.ArtworkURL.clone(),
                    Author: temp_pod.Author.clone(),
                    Categories: temp_pod.Categories.clone(),
                    EpisodeCount: temp_pod.EpisodeCount,
                    FeedURL: temp_pod.FeedURL.clone(),
                    WebsiteURL: temp_pod.WebsiteURL.clone(),
                    Description: temp_pod.Description.clone(), // Map this if you added it
                }
            }).collect();

            Ok(podcasts)
        } else {
            Err(anyhow!("Error Fetching pods"))
        }
    }


    pub async fn return_eps(&self, podcast_data: &PinepodsPodcasts) -> anyhow::Result<Vec<PinepodsEpisodes>> {
        let client = reqwest::Client::new();
        let request_body = EpisodeRequest {
            podcast_id: podcast_data.PodcastID,  // Assuming PodcastID is of type i64
            user_id: self.user_id,
        };
        let response = client
            .post(&format!("{}/api/data/podcast_episodes", &self.url))
            .header("Api-Key", &self.api_key.trim().to_string())
            .json(&request_body)
            .send()
            .await?;

        if response.status().is_success() {
            let json: HashMap<String, Vec<PinepodsEpisodes>> = response.json().await?;
            let episodes = json.get("episode_info").cloned().unwrap_or_else(Vec::new);

            Ok(episodes)
        } else {
            Err(anyhow!("Error fetching episodes"))
        }
    }


}
