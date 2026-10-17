//! The login state machine behind the login screen.

use anyhow::Result;

use super::auth_manager::{AuthManager, LoginOutcome, hash_password};
use super::models::*;
use super::session_storage::SessionStorage;
use crate::api::http::normalize_server_url;

#[derive(Debug, Clone)]
pub enum LoginResult {
    Success(Box<SessionInfo>),
    ServerVerified,
    MfaRequired,
    FirstAdminRequired,
    /// Logged in, but the account hasn't been through first-run setup yet.
    FirstTimeSetup,
    Error(String),
}

/// What the flow is waiting on between screens.
enum Pending {
    None,
    Mfa {
        server: String,
        mfa_session_token: String,
    },
    Setup(Box<AuthState>),
}

pub struct LoginFlow {
    auth: AuthManager,
    pending: Pending,
}

impl LoginFlow {
    pub fn new() -> Result<Self> {
        Ok(Self {
            auth: AuthManager::new()?,
            pending: Pending::None,
        })
    }

    pub async fn verify_server(&mut self, server: &str) -> LoginResult {
        let server = normalize_server_url(server);
        if let Err(e) = self.auth.verify_pinepods_instance(&server).await {
            return LoginResult::Error(format!("Couldn't verify the server: {e:#}"));
        }
        match self.auth.check_self_service_status(&server).await {
            Ok((true, false)) => LoginResult::FirstAdminRequired,
            Ok(_) => LoginResult::ServerVerified,
            Err(e) => LoginResult::Error(format!("Couldn't check the server's setup: {e:#}")),
        }
    }

    pub async fn create_first_admin(
        &mut self,
        server: &str,
        username: String,
        password: String,
        email: String,
        fullname: String,
    ) -> LoginResult {
        let server = normalize_server_url(server);
        let request = match hash_password(&password) {
            Ok(hashed) => CreateFirstAdminRequest {
                username: username.clone(),
                password: hashed,
                email,
                fullname,
            },
            Err(e) => return LoginResult::Error(e.to_string()),
        };
        if let Err(e) = self.auth.create_first_admin(&server, request).await {
            return LoginResult::Error(format!("{e:#}"));
        }
        self.authenticate(&server, &username, &password).await
    }

    pub async fn authenticate(
        &mut self,
        server: &str,
        username: &str,
        password: &str,
    ) -> LoginResult {
        let server = normalize_server_url(server);
        match self.auth.login(&server, username, password).await {
            Ok(LoginOutcome::Key(api_key)) => self.finish(server, api_key).await,
            Ok(LoginOutcome::MfaRequired {
                mfa_session_token, ..
            }) => {
                self.pending = Pending::Mfa {
                    server,
                    mfa_session_token,
                };
                LoginResult::MfaRequired
            }
            Err(e) => LoginResult::Error(format!("Login failed: {e:#}")),
        }
    }

    pub async fn verify_mfa(&mut self, code: &str) -> LoginResult {
        let Pending::Mfa {
            server,
            mfa_session_token,
        } = std::mem::replace(&mut self.pending, Pending::None)
        else {
            return LoginResult::Error("Log in again to get a new MFA prompt".into());
        };
        match self
            .auth
            .verify_mfa(&server, mfa_session_token.clone(), code.to_string())
            .await
        {
            Ok(api_key) => self.finish(server, api_key).await,
            Err(e) => {
                // Keep the MFA session so the user can retype the code.
                self.pending = Pending::Mfa {
                    server,
                    mfa_session_token,
                };
                LoginResult::Error(format!("MFA failed: {e:#}"))
            }
        }
    }

    pub async fn setup_timezone(
        &mut self,
        timezone: String,
        hour_pref: i32,
        date_format: String,
    ) -> LoginResult {
        let Pending::Setup(auth_state) = &self.pending else {
            return LoginResult::Error("Log in first".into());
        };
        let info = TimeZoneInfo {
            user_id: auth_state.user_details.UserID,
            timezone,
            hour_pref,
            date_format,
        };
        if let Err(e) = self
            .auth
            .setup_timezone_info(&auth_state.server_name, &auth_state.api_key, info)
            .await
        {
            return LoginResult::Error(format!("Couldn't save the settings: {e:#}"));
        }
        let Pending::Setup(auth_state) = std::mem::replace(&mut self.pending, Pending::None) else {
            unreachable!("checked above");
        };
        Self::complete(*auth_state)
    }

    async fn finish(&mut self, server: String, api_key: String) -> LoginResult {
        let (user_details, server_details) = match self.auth.load_account(&server, &api_key).await {
            Ok(account) => account,
            Err(e) => return LoginResult::Error(format!("Couldn't load the account: {e:#}")),
        };
        let auth_state = AuthState {
            server_name: server,
            api_key,
            user_details,
            server_details,
        };
        let setup_done = self
            .auth
            .first_login_done(
                &auth_state.server_name,
                &auth_state.api_key,
                auth_state.user_details.UserID,
            )
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("first-login check failed, assuming setup is done: {e:#}");
                true
            });
        if !setup_done {
            self.pending = Pending::Setup(Box::new(auth_state));
            return LoginResult::FirstTimeSetup;
        }
        Self::complete(auth_state)
    }

    fn complete(auth_state: AuthState) -> LoginResult {
        if let Err(e) = SessionStorage::save_session(&auth_state) {
            tracing::warn!("couldn't save the session; you'll need to log in next time: {e:#}");
        }
        LoginResult::Success(Box::new(SessionInfo { auth_state }))
    }

    /// Restores the saved session. A revoked key clears it; an unreachable server keeps
    /// it, so Firewood still starts when the server is down or the network is out.
    pub async fn restore_session(&self) -> Option<SessionInfo> {
        let auth_state = match SessionStorage::load_session() {
            Ok(Some(auth)) => auth,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!("couldn't read the saved session: {e:#}");
                return None;
            }
        };
        match self
            .auth
            .verify_key(&auth_state.server_name, &auth_state.api_key)
            .await
        {
            Ok(()) => {}
            Err(e)
                if matches!(
                    e.downcast_ref::<crate::api::ApiError>(),
                    Some(crate::api::ApiError::Unauthorized)
                ) =>
            {
                tracing::warn!("the saved API key was rejected; logging out");
                if let Err(e) = SessionStorage::clear_session() {
                    tracing::warn!("couldn't clear the session: {e:#}");
                }
                return None;
            }
            Err(e) => tracing::warn!(
                "couldn't reach the server to check the session; continuing offline: {e:#}"
            ),
        }
        Some(SessionInfo { auth_state })
    }
}
