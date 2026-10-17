//! The saved login.
//!
//! The API key goes in the OS credential store (Keychain, Secret Service). The rest of
//! the session, which isn't secret, goes in `session.json`. Where no credential store
//! is available (a headless box without Secret Service), the key falls back into
//! `session.json`, which is always written readable by the owner only.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::auth::{AuthState, GetApiDetails, GetUserDetails};
use crate::paths;

const KEYRING_SERVICE: &str = "pinepods-firewood";

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub server_name: String,
    pub user_id: i32,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub email: String,
    /// Present only when the credential store couldn't be used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default)]
    pub server_details: GetApiDetails,
}

pub struct SessionStorage;

impl SessionStorage {
    fn session_path() -> Result<PathBuf> {
        Ok(paths::config_dir()?.join("session.json"))
    }

    /// Before 0.2 the session lived in the platform config dir (`~/Library/Application
    /// Support` on macOS) with the key in plain JSON.
    fn legacy_session_path() -> Option<PathBuf> {
        let path = dirs::config_dir()?
            .join("pinepods-firewood")
            .join("session.json");
        let current = Self::session_path().ok()?;
        (path != current).then_some(path)
    }

    pub fn save_session(auth: &AuthState) -> Result<()> {
        let key_in_keyring = match keyring_entry(&auth.server_name)
            .and_then(|e| e.set_password(&auth.api_key))
        {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(
                    "credential store unavailable ({err}); keeping the API key in the session file"
                );
                false
            }
        };
        let stored = StoredSession {
            server_name: auth.server_name.clone(),
            user_id: auth.user_details.UserID,
            username: auth.user_details.Username.clone().unwrap_or_default(),
            full_name: auth.user_details.Fullname.clone().unwrap_or_default(),
            email: auth.user_details.Email.clone().unwrap_or_default(),
            api_key: (!key_in_keyring).then(|| auth.api_key.clone()),
            server_details: auth.server_details.clone(),
        };
        let json = serde_json::to_vec_pretty(&stored)?;
        paths::write_private(&Self::session_path()?, &json)
            .context("couldn't write the session file")?;
        tracing::info!("session saved");
        Ok(())
    }

    pub fn load_session() -> Result<Option<AuthState>> {
        let path = Self::session_path()?;
        if !path.exists() {
            return Self::migrate_legacy();
        }
        let stored: StoredSession =
            serde_json::from_slice(&fs::read(&path)?).context("session file is corrupt")?;
        let api_key = match stored.api_key.clone() {
            Some(key) => key,
            None => match keyring_entry(&stored.server_name).and_then(|e| e.get_password()) {
                Ok(key) => key,
                Err(err) => {
                    tracing::warn!("couldn't read the API key from the credential store: {err}");
                    return Ok(None);
                }
            },
        };
        Ok(Some(stored.into_auth_state(api_key)))
    }

    fn migrate_legacy() -> Result<Option<AuthState>> {
        let Some(legacy) = Self::legacy_session_path().filter(|p| p.exists()) else {
            return Ok(None);
        };
        let stored: StoredSession = match fs::read(&legacy)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
        {
            Some(s) => s,
            None => return Ok(None),
        };
        let Some(api_key) = stored.api_key.clone() else {
            return Ok(None);
        };
        let auth = stored.into_auth_state(api_key);
        Self::save_session(&auth)?;
        if let Err(err) = fs::remove_file(&legacy) {
            tracing::warn!(
                "couldn't remove the old session file at {}: {err}",
                legacy.display()
            );
        }
        tracing::info!("moved the session from {}", legacy.display());
        Ok(Some(auth))
    }

    pub fn clear_session() -> Result<()> {
        let path = Self::session_path()?;
        if path.exists() {
            if let Ok(stored) = serde_json::from_slice::<StoredSession>(&fs::read(&path)?)
                && let Ok(entry) = keyring_entry(&stored.server_name)
            {
                let _ = entry.delete_credential();
            }
            fs::remove_file(&path)?;
            tracing::info!("session cleared");
        }
        Ok(())
    }
}

impl StoredSession {
    fn into_auth_state(self, api_key: String) -> AuthState {
        AuthState {
            server_name: self.server_name,
            api_key,
            user_details: GetUserDetails {
                UserID: self.user_id,
                Username: Some(self.username),
                Fullname: Some(self.full_name),
                Email: Some(self.email),
            },
            server_details: self.server_details,
        }
    }
}

fn keyring_entry(server: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, server)
}
