//! The login steps behind the login screen.
//!
//! Each step is a future that owns everything it needs, so the screen can run it in
//! the background and keep drawing. What a later step needs from an earlier one (the
//! MFA session, an account waiting on first-run setup) comes back in the
//! [`LoginResult`] and is handed back in by the screen.

use anyhow::Result;

use super::auth_manager::{AuthManager, LoginOutcome, hash_password};
use super::models::*;
use super::session_storage::SessionStorage;
use crate::api::ApiError;
use crate::api::http::normalize_server_url;

#[derive(Debug)]
pub enum LoginResult {
    Success(Box<SessionInfo>),
    ServerVerified,
    FirstAdminRequired,
    MfaRequired(MfaSession),
    /// Logged in, but the account hasn't been through first-run setup yet.
    FirstTimeSetup(Box<AuthState>),
    Error(String),
}

/// A login waiting on its second factor.
pub struct MfaSession {
    server: String,
    token: String,
}

impl std::fmt::Debug for MfaSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MfaSession")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

/// What starting up found in the saved session.
#[derive(Debug)]
pub enum Restored {
    Session(Box<SessionInfo>),
    /// Nothing saved.
    None,
    /// The saved key was revoked; the server it was for, to sign in again.
    Rejected {
        server: String,
    },
}

/// The fields of a new administrator account.
pub struct NewAdmin {
    pub username: String,
    pub password: String,
    pub email: String,
    pub fullname: String,
}

/// Cheap to clone: each step runs on its own copy.
#[derive(Clone)]
pub struct LoginFlow {
    auth: AuthManager,
}

impl LoginFlow {
    pub fn new() -> Result<Self> {
        Ok(Self {
            auth: AuthManager::new()?,
        })
    }

    pub async fn verify_server(self, server: String) -> LoginResult {
        let server = normalize_server_url(&server);
        if let Err(e) = self.auth.verify_pinepods_instance(&server).await {
            return LoginResult::Error(format!("Couldn't verify the server: {e:#}"));
        }
        match self.auth.check_self_service_status(&server).await {
            Ok((true, false)) => LoginResult::FirstAdminRequired,
            Ok(_) => LoginResult::ServerVerified,
            Err(e) => LoginResult::Error(format!("Couldn't check the server's setup: {e:#}")),
        }
    }

    pub async fn create_first_admin(self, server: String, admin: NewAdmin) -> LoginResult {
        let server = normalize_server_url(&server);
        let request = match hash_password(&admin.password) {
            Ok(hashed) => CreateFirstAdminRequest {
                username: admin.username.clone(),
                password: hashed,
                email: admin.email,
                fullname: admin.fullname,
            },
            Err(e) => return LoginResult::Error(e.to_string()),
        };
        if let Err(e) = self.auth.create_first_admin(&server, request).await {
            return LoginResult::Error(format!("{e:#}"));
        }
        self.sign_in(server, admin.username, admin.password).await
    }

    pub async fn sign_in(self, server: String, username: String, password: String) -> LoginResult {
        let server = normalize_server_url(&server);
        match self.auth.login(&server, &username, &password).await {
            Ok(LoginOutcome::Key(api_key)) => self.finish(server, api_key).await,
            Ok(LoginOutcome::MfaRequired {
                mfa_session_token, ..
            }) => LoginResult::MfaRequired(MfaSession {
                server,
                token: mfa_session_token,
            }),
            Err(e) => LoginResult::Error(format!("{e:#}")),
        }
    }

    /// Signs in with a key made in the web app, for accounts that log in through
    /// OIDC and so have no password.
    pub async fn sign_in_with_key(self, server: String, api_key: String) -> LoginResult {
        let server = normalize_server_url(&server);
        self.finish(server, api_key.trim().to_string()).await
    }

    /// Loads the account behind a key given from outside (headless mode's environment)
    /// without saving it as the session or running first-run setup: the key's owner
    /// stays in charge of where it lives, and there's nobody to ask setup questions.
    pub async fn account_for_key(self, server: String, api_key: String) -> Result<AuthState> {
        let server = normalize_server_url(&server);
        let api_key = api_key.trim().to_string();
        let (user_details, server_details) = self.auth.load_account(&server, &api_key).await?;
        Ok(AuthState {
            server_name: server,
            api_key,
            user_details,
            server_details,
        })
    }

    /// On failure the session comes back so the code can be typed again.
    pub async fn verify_mfa(
        self,
        session: MfaSession,
        code: String,
    ) -> (LoginResult, Option<MfaSession>) {
        match self
            .auth
            .verify_mfa(&session.server, session.token.clone(), code)
            .await
        {
            Ok(api_key) => (self.finish(session.server, api_key).await, None),
            Err(e) => (LoginResult::Error(format!("{e:#}")), Some(session)),
        }
    }

    pub async fn setup_timezone(
        self,
        auth_state: AuthState,
        timezone: String,
        hour_pref: i32,
        date_format: String,
    ) -> LoginResult {
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
        Self::complete(auth_state)
    }

    async fn finish(&self, server: String, api_key: String) -> LoginResult {
        let (user_details, server_details) = match self.auth.load_account(&server, &api_key).await {
            Ok(account) => account,
            Err(e) if matches!(e.downcast_ref::<ApiError>(), Some(ApiError::Unauthorized)) => {
                return LoginResult::Error("The server didn't accept that API key".into());
            }
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
            return LoginResult::FirstTimeSetup(Box::new(auth_state));
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
    pub async fn restore_session(self) -> Restored {
        let auth_state = match SessionStorage::load_session() {
            Ok(Some(auth)) => auth,
            Ok(None) => return Restored::None,
            Err(e) => {
                tracing::warn!("couldn't read the saved session: {e:#}");
                return Restored::None;
            }
        };
        match self
            .auth
            .verify_key(&auth_state.server_name, &auth_state.api_key)
            .await
        {
            Ok(()) => {}
            Err(e) if matches!(e.downcast_ref::<ApiError>(), Some(ApiError::Unauthorized)) => {
                tracing::warn!("the saved API key was rejected; logging out");
                if let Err(e) = SessionStorage::clear_session() {
                    tracing::warn!("couldn't clear the session: {e:#}");
                }
                return Restored::Rejected {
                    server: auth_state.server_name,
                };
            }
            Err(e) => tracing::warn!(
                "couldn't reach the server to check the session; continuing offline: {e:#}"
            ),
        }
        Restored::Session(Box::new(SessionInfo { auth_state }))
    }
}
