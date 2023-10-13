use reqwest;
use tokio;
use anyhow::{Result, anyhow};
use serde_json::Value;
use std::fs::create_dir_all;
use directories::{ProjectDirs};
use std::path::{Display, Path, PathBuf};
use std::fs;
use serde::Deserialize;
use serde_derive::Serialize;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Deserialize)]
pub struct PinepodsCheck {
    pub(crate) status_code: u16,
    pub(crate) pinepods_instance: bool
}

#[derive(Debug, Serialize, Deserialize)]
struct PinepodsConfig {
    url: String,
    api_key: String
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

pub fn test_existing_config () -> std::io::Result<()> {
    if let Some(app_path) = get_app_path() {
        let mut config_path = app_path.join("pinepods_config.json");
        let config_data: String = fs::read_to_string(&config_path)?;
        let config_json: serde_json::Value = serde_json::from_str(&config_data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        dbg!(&config_json);
        let pinepods_config: PinepodsConfig = serde_json::from_value(config_json)
            .expect("Failed to convert JSON value to Config Struct");
        let return_verify_login = pinepods_config.verify_key();
        match return_verify_login {
            Ok(data) => {}
            Err(data) => {}
        };
    } else { return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "App Path not found"));
    }
    return Ok(())

}

pub struct ReqwestValues<'a> {
    pub(crate) url: &'a str,
    pub(crate) api_key: &'a str,
    pub(crate) user_id: i16,
}

impl ReqwestValues<'_> {

    pub async fn make_request(&self) -> Result<PinepodsCheck, reqwest::Error> {
        let client = reqwest::Client::new();
        println!("{}", self.url);
        let response = client
        .get(&self.url)
        .header("pinepods_api", "not_needed")
        .send()?;

        let parsed_data: PinepodsCheck = response.json()?;
        Ok(parsed_data)
        }

    pub async fn verify_key(&self) -> Result<PinepodsCheck, reqwest::Error> {

        let key_verify_url = &format!("{}{}", self.url, "/api/pinepods_check");
        println!("{}", &key_verify_url);
        println!("{}", &self.api_key);
        let client = reqwest::Client::new();
        let response = client
            .get(key_verify_url)
            .header("Api-Key", &self.api_key.trim().to_string())
            .send()?;

        let parsed_data: PinepodsCheck = response.json()?;
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
    pub async fn return_pods(&self) -> Result<Vec<Value>> {
        let client = reqwest::Client::new();
        let response = client
            .get(&format!("{}/return_pods/{}", &self.url, &self.user_id)) // Format the URL
            .header("Api-Key", self.api_key) // Add the API key to the headers
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
