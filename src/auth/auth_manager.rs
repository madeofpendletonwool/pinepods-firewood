//! The HTTP calls that happen before there's a session: server check, login, MFA,
//! first-admin creation and first-run setup.

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;

use super::models::*;
use crate::api::ApiError;
use crate::api::http::build_client;

pub struct AuthManager {
    client: Client,
}

pub enum LoginOutcome {
    /// Logged in; holds the API key.
    Key(String),
    MfaRequired {
        user_id: i32,
        mfa_session_token: String,
    },
}

impl AuthManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: build_client()?,
        })
    }

    async fn json<T: DeserializeOwned>(&self, req: RequestBuilder, what: &str) -> Result<T> {
        let resp = req.send().await.map_err(ApiError::from)?;
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(ApiError::Unauthorized.into());
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(str::to_string))
                .unwrap_or_else(|| status.to_string());
            bail!("{what} failed: {detail}");
        }
        resp.json()
            .await
            .map_err(ApiError::from)
            .with_context(|| format!("{what}: unexpected response"))
    }

    pub async fn verify_pinepods_instance(&self, server: &str) -> Result<()> {
        let check: PinepodsCheckResponse = self
            .json(
                self.client.get(format!("{server}/api/pinepods_check")),
                "server check",
            )
            .await?;
        if !check.pinepods_instance {
            bail!("that server isn't a PinePods instance");
        }
        Ok(())
    }

    /// Returns `(self_service_enabled, first_admin_created)`.
    pub async fn check_self_service_status(&self, server: &str) -> Result<(bool, bool)> {
        let status: SelfServiceStatusResponse = self
            .json(
                self.client
                    .get(format!("{server}/api/data/self_service_status")),
                "self-service check",
            )
            .await?;
        Ok((status.status, status.first_admin_created))
    }

    pub async fn login(
        &self,
        server: &str,
        username: &str,
        password: &str,
    ) -> Result<LoginOutcome> {
        let credentials = STANDARD.encode(format!("{username}:{password}"));
        let req = self
            .client
            .get(format!("{server}/api/data/get_key"))
            .header("Authorization", format!("Basic {credentials}"));
        let resp: LoginResponse = match self.json(req, "login").await {
            Ok(r) => r,
            Err(e) if matches!(e.downcast_ref::<ApiError>(), Some(ApiError::Unauthorized)) => {
                bail!("incorrect username or password")
            }
            Err(e) => return Err(e),
        };

        if resp.status == "mfa_required" && resp.mfa_required.unwrap_or(false) {
            return match (resp.user_id, resp.mfa_session_token) {
                (Some(user_id), Some(mfa_session_token)) => Ok(LoginOutcome::MfaRequired {
                    user_id,
                    mfa_session_token,
                }),
                _ => Err(anyhow!(
                    "the server asked for MFA but didn't start an MFA session"
                )),
            };
        }
        resp.retrieved_key
            .map(LoginOutcome::Key)
            .ok_or_else(|| anyhow!("the server didn't return an API key"))
    }

    pub async fn verify_mfa(
        &self,
        server: &str,
        mfa_session_token: String,
        mfa_code: String,
    ) -> Result<String> {
        let body = VerifyMfaLoginRequest {
            mfa_session_token,
            mfa_code,
        };
        let req = self
            .client
            .post(format!("{server}/api/data/verify_mfa_and_get_key"))
            .json(&body);
        let resp: VerifyMfaLoginResponse = self.json(req, "MFA verification").await?;
        if !resp.verified || resp.status != "success" {
            bail!("that code wasn't accepted");
        }
        resp.retrieved_key
            .ok_or_else(|| anyhow!("the server didn't return an API key"))
    }

    /// Fetches everything a session needs once we hold a key.
    pub async fn load_account(
        &self,
        server: &str,
        api_key: &str,
    ) -> Result<(GetUserDetails, GetApiDetails)> {
        self.verify_key(server, api_key).await?;
        let user: GetUserIdResponse = self
            .json(
                self.authed(server, "/api/data/get_user", api_key),
                "user lookup",
            )
            .await?;
        let user_id = user
            .retrieved_id
            .ok_or_else(|| anyhow!("the server didn't return a user id"))?;
        let details: GetUserDetails = self
            .json(
                self.authed(
                    server,
                    &format!("/api/data/user_details_id/{user_id}"),
                    api_key,
                ),
                "user details",
            )
            .await?;
        let config: GetApiDetails = self
            .json(
                self.authed(server, "/api/data/config", api_key),
                "server config",
            )
            .await?;
        Ok((details, config))
    }

    /// Fails with [`ApiError::Unauthorized`] if the key has been revoked.
    pub async fn verify_key(&self, server: &str, api_key: &str) -> Result<()> {
        let resp: KeyVerification = self
            .json(
                self.authed(server, "/api/data/verify_key", api_key),
                "key check",
            )
            .await?;
        if resp.status != "success" {
            return Err(ApiError::Unauthorized.into());
        }
        Ok(())
    }

    pub async fn first_login_done(
        &self,
        server: &str,
        api_key: &str,
        user_id: i32,
    ) -> Result<bool> {
        let resp: FirstLoginResponse = self
            .json(
                self.authed(
                    server,
                    &format!("/api/data/first_login_done/{user_id}"),
                    api_key,
                ),
                "first-login check",
            )
            .await?;
        Ok(resp.FirstLogin)
    }

    pub async fn create_first_admin(
        &self,
        server: &str,
        request: CreateFirstAdminRequest,
    ) -> Result<()> {
        let req = self
            .client
            .post(format!("{server}/api/data/create_first"))
            .json(&request);
        let _: CreateFirstAdminResponse = self.json(req, "creating the admin account").await?;
        Ok(())
    }

    pub async fn setup_timezone_info(
        &self,
        server: &str,
        api_key: &str,
        info: TimeZoneInfo,
    ) -> Result<()> {
        let req = self
            .client
            .post(format!("{server}/api/data/setup_time_info"))
            .header("Api-Key", api_key)
            .json(&info);
        let _: serde_json::Value = self.json(req, "saving time settings").await?;
        Ok(())
    }

    fn authed(&self, server: &str, path: &str, api_key: &str) -> RequestBuilder {
        self.client
            .get(format!("{server}{path}"))
            .header("Api-Key", api_key)
    }
}

/// Hashes a new account's password the way the web client does; the server stores
/// what it's sent as the hash.
pub fn hash_password(password: &str) -> Result<String> {
    use argon2::{Argon2, PasswordHasher};
    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|e| anyhow!("couldn't hash the password: {e}"))?;
    Ok(hash.to_string())
}
