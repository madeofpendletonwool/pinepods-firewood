use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use crate::auth::{AuthState, GetUserDetails, LoginServerRequest, GetApiDetails};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub server_name: String,
    pub api_key: String,
    pub user_id: i32,
    pub username: String,
    pub full_name: String,
    pub email: String,
    pub server_details: serde_json::Value,
}

impl From<&AuthState> for StoredSession {
    fn from(auth_state: &AuthState) -> Self {
        Self {
            server_name: auth_state.server_name.clone(),
            api_key: auth_state.api_key.clone(),
            user_id: auth_state.user_details.UserID,
            username: auth_state.user_details.Username.clone().unwrap_or_default(),
            full_name: auth_state.user_details.Fullname.clone().unwrap_or_default(),
            email: auth_state.user_details.Email.clone().unwrap_or_default(),
            server_details: serde_json::to_value(&auth_state.server_details).unwrap_or_default(),
        }
    }
}

impl StoredSession {
    pub fn to_auth_state(&self) -> Result<AuthState> {
        let user_details = GetUserDetails {
            UserID: self.user_id,
            Username: Some(self.username.clone()),
            Fullname: Some(self.full_name.clone()),
            Email: Some(self.email.clone()),
            Hashed_PW: None, // We don't store passwords
            Salt: None, // We don't store salts
        };

        let login_request = LoginServerRequest {
            server_name: self.server_name.clone(),
            username: Some(self.username.clone()),
            password: None, // We don't store passwords
            api_key: Some(self.api_key.clone()),
        };

        let server_details: GetApiDetails = serde_json::from_value(self.server_details.clone())
            .unwrap_or_default();

        Ok(AuthState {
            server_name: self.server_name.clone(),
            api_key: self.api_key.clone(),
            user_details,
            server_details,
            login_request,
        })
    }
}

pub struct SessionStorage;

impl SessionStorage {
    fn get_session_file_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
        let app_dir = config_dir.join("pinepods-firewood");
        fs::create_dir_all(&app_dir)?;
        Ok(app_dir.join("session.json"))
    }

    pub fn save_session(auth_state: &AuthState) -> Result<()> {
        let stored_session = StoredSession::from(auth_state);
        let session_path = Self::get_session_file_path()?;
        let session_data = serde_json::to_string_pretty(&stored_session)?;
        fs::write(session_path, session_data)?;
        log::info!("Session saved successfully");
        Ok(())
    }

    pub fn load_session() -> Result<Option<StoredSession>> {
        let session_path = Self::get_session_file_path()?;
        
        if !session_path.exists() {
            return Ok(None);
        }

        let session_data = fs::read_to_string(session_path)?;
        let stored_session: StoredSession = serde_json::from_str(&session_data)?;
        log::info!("Session loaded successfully for user: {}", stored_session.username);
        Ok(Some(stored_session))
    }

    pub fn clear_session() -> Result<()> {
        let session_path = Self::get_session_file_path()?;
        
        if session_path.exists() {
            fs::remove_file(session_path)?;
            log::info!("Session cleared successfully");
        }
        
        Ok(())
    }

    pub fn session_exists() -> bool {
        Self::get_session_file_path()
            .map(|path| path.exists())
            .unwrap_or(false)
    }
}