use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginServerRequest {
    pub server_name: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
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
    pub timezone: String,
    pub hour_pref: i32,
    pub date_format: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(non_snake_case)]
pub struct GetUserDetails {
    pub UserID: i32,
    pub Fullname: Option<String>,
    pub Username: Option<String>,
    pub Email: Option<String>,
    pub Hashed_PW: Option<String>,
    pub Salt: Option<String>,
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

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Deserialize)]
pub struct TimeInfoResponse {
    pub timezone: String,
    pub hour_pref: i16,
    pub date_format: String,
}

// Authentication state for storage
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthState {
    pub server_name: String,
    pub api_key: String,
    pub user_details: GetUserDetails,
    pub server_details: GetApiDetails,
    pub login_request: LoginServerRequest,
}

// Session information
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub auth_state: AuthState,
    pub is_first_login: bool,
    pub timezone_configured: bool,
}