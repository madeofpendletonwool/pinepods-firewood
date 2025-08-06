use anyhow::{anyhow, Result};
use reqwest::Client;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use super::models::*;

pub struct AuthManager {
    client: Client,
}

#[derive(Debug, Clone)]
pub enum AuthManagerResult {
    Success(GetUserDetails, LoginServerRequest, GetApiDetails),
    MfaRequired {
        server_name: String,
        username: String,
        user_id: i32,
        mfa_session_token: String,
    },
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    // Step 1: Verify if server is a Pinepods instance
    pub async fn verify_pinepods_instance(&self, server_name: &str) -> Result<PinepodsCheckResponse> {
        let server_name = server_name.trim_end_matches('/');
        let url = format!("{}/api/pinepods_check", server_name);
        
        let response = self.client.get(&url).send().await?;

        if response.status().is_success() {
            let check_data: PinepodsCheckResponse = response.json().await?;
            if check_data.pinepods_instance {
                Ok(check_data)
            } else {
                Err(anyhow!("Server is not a Pinepods instance"))
            }
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(anyhow!("Failed to verify Pinepods instance ({}): {}", status, error_text))
        }
    }

    // Main login function that handles secure MFA flow
    pub async fn login_new_server_secure(&self, server_name: String, username: String, password: String) -> Result<AuthManagerResult> {
        let credentials = STANDARD.encode(format!("{}:{}", username, password).as_bytes());
        let auth_header = format!("Basic {}", credentials);
        let url = format!("{}/api/data/get_key", server_name);
        
        // Step 1: Verify Server
        let check_data = self.verify_pinepods_instance(&server_name).await?;
        if !check_data.pinepods_instance {
            return Err(anyhow!("Pinepods instance not found at specified server"));
        }
        
        // Step 2: Get API key or MFA session token
        let response = self.client
            .get(&url)
            .header("Authorization", &auth_header)
            .send()
            .await?;
            
        if !response.status().is_success() {
            return Err(anyhow!("Failed to authenticate user. Incorrect credentials?"));
        }
        
        let login_response: LoginResponse = response.json().await?;
        
        // Check if MFA is required
        if login_response.status == "mfa_required" && login_response.mfa_required.unwrap_or(false) {
            return Ok(AuthManagerResult::MfaRequired {
                server_name,
                username,
                user_id: login_response.user_id.unwrap(),
                mfa_session_token: login_response.mfa_session_token.unwrap(),
            });
        }
        
        // Normal flow - MFA not required, proceed with existing logic
        let api_key = login_response.retrieved_key.ok_or_else(|| {
            anyhow!("No API key returned from server")
        })?;
        
        // Continue with existing verification steps
        let result = self.complete_login_flow(server_name, username, password, api_key).await?;
        Ok(AuthManagerResult::Success(result.0, result.1, result.2))
    }

    // Complete MFA verification during login and get full login data
    pub async fn complete_mfa_login(&self, server_name: String, username: String, mfa_session_token: String, mfa_code: String) -> Result<(GetUserDetails, LoginServerRequest, GetApiDetails)> {
        // Verify MFA and get API key
        let mfa_response = self.call_verify_mfa_and_get_key(&server_name, mfa_session_token, mfa_code).await?;
        
        if !mfa_response.verified || mfa_response.status != "success" {
            return Err(anyhow!("MFA verification failed"));
        }
        
        let api_key = mfa_response.retrieved_key.ok_or_else(|| {
            anyhow!("No API key returned after MFA verification")
        })?;
        
        // Complete the login flow with the verified API key
        self.complete_login_flow(server_name, username, "".to_string(), api_key).await
    }

    // Extracted common login completion logic
    pub async fn complete_login_flow(&self, server_name: String, username: String, password: String, api_key: String) -> Result<(GetUserDetails, LoginServerRequest, GetApiDetails)> {
        // Step 1: Verify the API key
        let verify_response = self.call_verify_key(&server_name, &api_key).await?;
        if verify_response.status != "success" {
            return Err(anyhow!("API key verification failed"));
        }
        
        // Step 2: Get user ID
        let user_id_response = self.call_get_user_id(&server_name, &api_key).await?;
        if user_id_response.status != "success" {
            return Err(anyhow!("Failed to get user ID"));
        }
        
        let login_request = LoginServerRequest {
            server_name: server_name.clone(),
            username: Some(username.clone()),
            password: if password.is_empty() { None } else { Some(password) },
            api_key: Some(api_key.clone()),
        };
        
        // Step 3: Get user details
        let user_details = self.call_get_user_details(
            &server_name,
            &api_key,
            &user_id_response.retrieved_id.unwrap(),
        ).await?;
        
        if user_details.Username.is_none() {
            return Err(anyhow!("Failed to get user details"));
        }
        
        // Step 4: Get server details
        let server_details = self.call_get_api_config(&server_name, &api_key).await?;
        if server_details.api_url.is_none() {
            return Err(anyhow!("Failed to get server details"));
        }
        
        Ok((user_details, login_request, server_details))
    }

    // Helper function to verify MFA and get API key
    pub async fn call_verify_mfa_and_get_key(&self, server_name: &str, mfa_session_token: String, mfa_code: String) -> Result<VerifyMfaLoginResponse> {
        let url = format!("{}/api/data/verify_mfa_and_get_key", server_name);
        let body = VerifyMfaLoginRequest { mfa_session_token, mfa_code };

        let response = self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            let response_body: VerifyMfaLoginResponse = response.json().await?;
            Ok(response_body)
        } else {
            Err(anyhow!("MFA verification failed"))
        }
    }

    // Helper function to verify API key
    async fn call_verify_key(&self, server_name: &str, api_key: &str) -> Result<KeyVerification> {
        let url = format!("{}/api/data/verify_key", server_name);
        
        let response = self.client
            .get(&url)
            .header("Api-Key", api_key)
            .send()
            .await?;

        if response.status().is_success() {
            let key_verify: KeyVerification = response.json().await?;
            Ok(key_verify)
        } else {
            Err(anyhow!("API key verification failed"))
        }
    }

    // Helper function to get user ID
    async fn call_get_user_id(&self, server_name: &str, api_key: &str) -> Result<GetUserIdResponse> {
        let url = format!("{}/api/data/get_user", server_name);

        let response = self.client
            .get(&url)
            .header("Api-Key", api_key)
            .send()
            .await?;

        if response.status().is_success() {
            let user_id_response: GetUserIdResponse = response.json().await?;
            Ok(user_id_response)
        } else {
            Err(anyhow!("Failed to get user ID"))
        }
    }

    // Helper function to get user details
    async fn call_get_user_details(&self, server_name: &str, api_key: &str, user_id: &i32) -> Result<GetUserDetails> {
        let url = format!("{}/api/data/user_details_id/{}", server_name, user_id);
        
        let response = self.client
            .get(&url)
            .header("Api-Key", api_key)
            .send()
            .await?;

        if response.status().is_success() {
            let user_details: GetUserDetails = response.json().await?;
            Ok(user_details)
        } else {
            Err(anyhow!("Failed to get user details"))
        }
    }

    // Helper function to get API configuration
    async fn call_get_api_config(&self, server_name: &str, api_key: &str) -> Result<GetApiDetails> {
        let url = format!("{}/api/data/config", server_name);
        
        let response = self.client
            .get(&url)
            .header("Api-Key", api_key)
            .send()
            .await?;

        if response.status().is_success() {
            let config: GetApiDetails = response.json().await?;
            Ok(config)
        } else {
            Err(anyhow!("Failed to get API configuration"))
        }
    }

    // Check if this is the first login for the user
    pub async fn call_first_login_done(&self, server_name: String, api_key: String, user_id: &i32) -> Result<bool> {
        let url = format!("{}/api/data/first_login_done/{}", server_name, user_id);
        let response = self.client
            .get(&url)
            .header("Content-Type", "application/json")
            .header("Api-Key", &api_key)
            .send()
            .await?;
            
        if response.status().is_success() {
            let response_body: FirstLoginResponse = response.json().await?;
            Ok(response_body.FirstLogin)
        } else {
            Err(anyhow!("Failed to check first login status"))
        }
    }

    // Check self-service status (needed for first admin setup)
    pub async fn check_self_service_status(&self, server_name: &str) -> Result<(bool, bool)> {
        let server_name = server_name.trim_end_matches('/');
        let url = format!("{}/api/data/self_service_status", server_name);
        
        let response = self.client.get(&url).send().await?;

        if response.status().is_success() {
            let status_response: SelfServiceStatusResponse = response.json().await?;
            Ok((status_response.status, status_response.first_admin_created))
        } else {
            Err(anyhow!("Failed to check self-service status"))
        }
    }

    // Create first admin (for new installations)
    pub async fn create_first_admin(&self, server_name: &str, request: CreateFirstAdminRequest) -> Result<CreateFirstAdminResponse> {
        let url = format!("{}/api/data/create_first", server_name);
        
        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;
            
        if response.status().is_success() {
            let admin_response: CreateFirstAdminResponse = response.json().await?;
            Ok(admin_response)
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(anyhow!("Failed to create first admin: {}", error_text))
        }
    }

    // Get timezone information
    pub async fn get_time_info(&self, server_name: &str, api_key: &str, user_id: i32) -> Result<TimeInfo> {
        let url = format!("{}/api/data/get_time_info", server_name);
        
        let response = self.client
            .get(&url)
            .header("Api-Key", api_key)
            .query(&[("user_id", user_id.to_string())])
            .send()
            .await?;
            
        if response.status().is_success() {
            let time_info: TimeInfo = response.json().await?;
            Ok(time_info)
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(anyhow!("Failed to get time info: {}", error_text))
        }
    }

    // Setup timezone information
    pub async fn setup_timezone_info(&self, server_name: &str, api_key: &str, timezone_info: TimeZoneInfo) -> Result<()> {
        let url = format!("{}/api/data/setup_time_info", server_name);
        
        let response = self.client
            .post(&url)
            .header("Api-Key", api_key)
            .json(&timezone_info)
            .send()
            .await?;
            
        if response.status().is_success() {
            Ok(())
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(anyhow!("Failed to setup timezone: {}", error_text))
        }
    }

    // Save authentication state
    pub async fn save_auth_state(&self, auth_state: &AuthState) -> Result<()> {
        // Implementation for saving to config file
        // This would typically save to ~/.config/pinepods-firewood/auth.json or similar
        Ok(())
    }

    // Load authentication state
    pub async fn load_auth_state(&self) -> Result<AuthState> {
        // Implementation for loading from config file
        // This would typically load from ~/.config/pinepods-firewood/auth.json or similar
        Err(anyhow!("No saved authentication state"))
    }
}