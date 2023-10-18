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
    pub(crate) url: String,
    pub(crate) api_key: String
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
    pub(crate) url: String,
    pub(crate) api_key: String,
    pub(crate) user_id: i64,
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
            let config_path = app_path.join("pinepods_config.json");

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

    pub async fn return_pods(&self) -> Result<Vec<Value>> {
        let client = reqwest::Client::new();
        let response = client
            .get(&format!("{}/api/data/return_pods/{}", &self.url, &self.user_id)) // Format the URL
            .header("Api-Key", &self.api_key.trim().to_string()) // Add the API key to the headers
            .send()
            .await?;

        if response.status().is_success() {
            let json: Value = response.json().await?;
            Ok(json["pods"].as_array().cloned().unwrap_or_default())
        } else {
            eprintln!(
                "Error fetching podcasts: {}",
                response.status()
            );
            Err(anyhow!("Error Fetching pods"))
        }
    }
}
