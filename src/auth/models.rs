use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginResponse {
    pub status: String,
    pub retrieved_key: Option<String>,
    pub mfa_required: Option<bool>,
    pub user_id: Option<i32>,
    pub mfa_session_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PinepodsCheckResponse {
    pub status_code: u16,
    pub pinepods_instance: bool,
}

#[derive(Debug, Deserialize)]
pub struct KeyVerification {
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct GetUserIdResponse {
    pub status: String,
    pub retrieved_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct FirstLoginResponse {
    pub FirstLogin: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TimeInfo {
    pub timezone: Option<String>,
    pub hour_pref: Option<i32>,
    pub date_format: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(non_snake_case)]
pub struct GetUserDetails {
    pub UserID: i32,
    pub Fullname: Option<String>,
    pub Username: Option<String>,
    pub Email: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct GetApiDetails {
    pub api_url: Option<String>,
    pub proxy_url: Option<String>,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<String>,
    pub proxy_protocol: Option<String>,
    pub reverse_proxy: Option<String>,
    pub people_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VerifyMfaLoginRequest {
    pub mfa_session_token: String,
    pub mfa_code: String,
}

#[derive(Deserialize)]
pub struct VerifyMfaLoginResponse {
    pub status: String,
    pub retrieved_key: Option<String>,
    pub verified: bool,
}

#[derive(Debug, Deserialize)]
pub struct SelfServiceStatusResponse {
    pub status: bool,
    pub first_admin_created: bool,
}

#[derive(Serialize)]
pub struct CreateFirstAdminRequest {
    pub username: String,
    pub password: String,
    pub email: String,
    pub fullname: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateFirstAdminResponse {
    pub message: String,
    pub user_id: i32,
}


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimeZoneInfo {
    pub user_id: i32,
    pub timezone: String,
    pub hour_pref: i32,
    pub date_format: String,
}

#[derive(Debug, Deserialize)]
pub struct SetupTimeZoneInfoResponse {
    pub success: bool,
}

/// A logged-in session. `Debug` leaves out the API key so it can't end up in a log.
#[derive(Clone)]
pub struct AuthState {
    pub server_name: String,
    pub api_key: String,
    pub user_details: GetUserDetails,
    pub server_details: GetApiDetails,
}

impl std::fmt::Debug for AuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthState")
            .field("server_name", &self.server_name)
            .field("user_id", &self.user_details.UserID)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub auth_state: AuthState,
}