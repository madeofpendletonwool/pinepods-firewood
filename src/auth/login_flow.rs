use anyhow::Result;
use super::{AuthManager, models::*};

#[derive(Debug, Clone)]
pub enum LoginState {
    ServerEntry,
    ServerVerification,
    SelfServiceCheck,
    FirstAdminSetup,
    CredentialEntry,
    Authentication,
    MfaRequired {
        server_name: String,
        username: String,
        user_id: i32,
        mfa_session_token: String,
    },
    MfaEntry,
    FirstTimeSetup {
        auth_state: AuthState,
    },
    TimeZoneSetup {
        auth_state: AuthState,
    },
    Complete {
        session_info: SessionInfo,
    },
    Error(String),
}

#[derive(Debug, Clone)]
pub enum LoginResult {
    Success(SessionInfo),
    ServerVerified,
    MfaRequired {
        server_name: String,
        username: String,
        user_id: i32,
        mfa_session_token: String,
    },
    FirstAdminRequired {
        server_name: String,
    },
    FirstTimeSetup {
        auth_state: AuthState,
    },
    Error(String),
}

pub struct LoginFlow {
    pub auth_manager: AuthManager,
    state: LoginState,
}

impl LoginFlow {
    pub fn new() -> Self {
        Self {
            auth_manager: AuthManager::new(),
            state: LoginState::ServerEntry,
        }
    }

    pub fn state(&self) -> &LoginState {
        &self.state
    }

    pub async fn check_existing_auth(&mut self) -> Result<Option<SessionInfo>> {
        match self.auth_manager.load_auth_state().await {
            Ok(auth_state) => {
                // Check if it's a first login and timezone setup
                let first_login_done = self.auth_manager
                    .call_first_login_done(auth_state.server_name.clone(), auth_state.api_key.clone(), 
                                     &auth_state.user_details.UserID)
                    .await
                    .unwrap_or(false);
                
                let timezone_configured = self.auth_manager
                    .get_time_info(&auth_state.server_name, &auth_state.api_key, 
                                 auth_state.user_details.UserID)
                    .await
                    .is_ok();

                let session_info = SessionInfo {
                    auth_state,
                    is_first_login: !first_login_done, // Inverted: if first_login_done is false, it IS first login
                    timezone_configured,
                };

                Ok(Some(session_info))
            }
            Err(_) => Ok(None),
        }
    }

    pub async fn verify_server(&mut self, server_name: &str) -> Result<LoginResult> {
        self.state = LoginState::ServerVerification;
        
        // Step 1: Verify Pinepods instance
        match self.auth_manager.verify_pinepods_instance(server_name).await {
            Ok(_) => {
                // Step 2: Check self-service status
                match self.auth_manager.check_self_service_status(server_name).await {
                    Ok((self_service_enabled, first_admin_created)) => {
                        if self_service_enabled && !first_admin_created {
                            self.state = LoginState::FirstAdminSetup;
                            Ok(LoginResult::FirstAdminRequired {
                                server_name: server_name.to_string(),
                            })
                        } else {
                            self.state = LoginState::CredentialEntry;
                            // Server verified, but authentication not complete yet
                            Ok(LoginResult::ServerVerified)
                        }
                    }
                    Err(e) => {
                        self.state = LoginState::Error(format!("Self-service check failed: {}", e));
                        Ok(LoginResult::Error(format!("Self-service check failed: {}", e)))
                    }
                }
            }
            Err(e) => {
                self.state = LoginState::Error(format!("Server verification failed: {}", e));
                Ok(LoginResult::Error(format!("Server verification failed: {}", e)))
            }
        }
    }

    pub async fn create_first_admin(&mut self, server_name: &str, username: String, 
        password: String, email: String, fullname: String) -> Result<LoginResult> {
        
        // Hash the password (you'll need to implement password hashing)
        let hashed_password = hash_password(&password)?;
        
        let request = CreateFirstAdminRequest {
            username: username.clone(),
            password: hashed_password,
            email,
            fullname,
        };

        match self.auth_manager.create_first_admin(server_name, request).await {
            Ok(_) => {
                // Now proceed with normal login
                self.authenticate(server_name, &username, &password).await
            }
            Err(e) => {
                self.state = LoginState::Error(format!("Failed to create admin: {}", e));
                Ok(LoginResult::Error(format!("Failed to create admin: {}", e)))
            }
        }
    }

    pub async fn authenticate(&mut self, server_name: &str, username: &str, password: &str) 
        -> Result<LoginResult> {
        self.state = LoginState::Authentication;
        
        match self.auth_manager.login_new_server_secure(
            server_name.to_string(), 
            username.to_string(), 
            password.to_string()
        ).await {
            Ok(super::auth_manager::AuthManagerResult::Success(user_details, login_request, server_details)) => {
                // Create auth state
                let api_key = login_request.api_key.clone().unwrap();
                let auth_state = AuthState {
                    server_name: server_name.to_string(),
                    api_key: api_key.clone(),
                    user_details: user_details.clone(),
                    server_details,
                    login_request,
                };

                // Check if it's first time login - if first_login_done is TRUE, user has already completed setup
                let first_login_done = self.auth_manager
                    .call_first_login_done(server_name.to_string(), auth_state.api_key.clone(), &user_details.UserID)
                    .await
                    .unwrap_or(false);

                if !first_login_done {
                    // User has NOT completed first login setup yet
                    self.state = LoginState::FirstTimeSetup { 
                        auth_state: auth_state.clone() 
                    };
                    return Ok(LoginResult::FirstTimeSetup { auth_state });
                }

                // Check timezone configuration
                let timezone_configured = self.auth_manager
                    .get_time_info(&server_name, &auth_state.api_key, user_details.UserID)
                    .await
                    .is_ok();

                if !timezone_configured {
                    self.state = LoginState::TimeZoneSetup { 
                        auth_state: auth_state.clone() 
                    };
                }

                let session_info = SessionInfo {
                    auth_state,
                    is_first_login: !first_login_done,
                    timezone_configured,
                };

                // Save auth state to both old system and new session storage
                if let Err(e) = self.auth_manager.save_auth_state(&session_info.auth_state).await {
                    log::warn!("Failed to save auth state: {}", e);
                }
                
                // Save to session storage for persistence
                if let Err(e) = super::SessionStorage::save_session(&session_info.auth_state) {
                    log::warn!("Failed to save session: {}", e);
                }

                self.state = LoginState::Complete { 
                    session_info: session_info.clone() 
                };
                
                Ok(LoginResult::Success(session_info))
            }
            Ok(super::auth_manager::AuthManagerResult::MfaRequired { server_name, username, user_id, mfa_session_token }) => {
                let mfa_state = LoginState::MfaRequired {
                    server_name: server_name.clone(),
                    username: username.clone(),
                    user_id,
                    mfa_session_token: mfa_session_token.clone(),
                };
                self.state = mfa_state;
                
                Ok(LoginResult::MfaRequired {
                    server_name,
                    username,
                    user_id,
                    mfa_session_token,
                })
            }
            Err(e) => {
                self.state = LoginState::Error(format!("Authentication failed: {}", e));
                Ok(LoginResult::Error(format!("Authentication failed: {}", e)))
            }
        }
    }

    pub async fn verify_mfa(&mut self, mfa_code: &str) -> Result<LoginResult> {
        if let LoginState::MfaRequired { server_name, username, user_id: _, mfa_session_token } = self.state.clone() {
            self.state = LoginState::MfaEntry;
            
            match self.auth_manager.call_verify_mfa_and_get_key(
                &server_name, 
                mfa_session_token, 
                mfa_code.to_string()
            ).await {
                Ok(mfa_response) => {
                    if !mfa_response.verified || mfa_response.status != "success" {
                        self.state = LoginState::Error("MFA verification failed".to_string());
                        return Ok(LoginResult::Error("MFA verification failed".to_string()));
                    }
                    
                    let api_key = mfa_response.retrieved_key
                        .ok_or_else(|| anyhow::anyhow!("No API key returned after MFA"))?;
                    
                    // Complete login flow with verified API key
                    self.complete_login(server_name, username, 
                                      String::new(), api_key).await
                }
                Err(e) => {
                    self.state = LoginState::Error(format!("MFA verification failed: {}", e));
                    Ok(LoginResult::Error(format!("MFA verification failed: {}", e)))
                }
            }
        } else {
            Ok(LoginResult::Error("Invalid state for MFA verification".to_string()))
        }
    }

    async fn complete_login(&mut self, server_name: String, username: String, 
        password: String, api_key: String) -> Result<LoginResult> {
        
        match self.auth_manager.complete_login_flow(
            server_name.clone(), username, password, api_key.clone()
        ).await {
            Ok((user_details, login_request, server_details)) => {
                let auth_state = AuthState {
                    server_name: server_name.clone(),
                    api_key: api_key.clone(),
                    user_details: user_details.clone(),
                    server_details,
                    login_request,
                };

                // Check if it's first time login
                let first_login_done = self.auth_manager
                    .call_first_login_done(server_name.clone(), api_key.clone(), &user_details.UserID)
                    .await
                    .unwrap_or(false);

                if !first_login_done {
                    self.state = LoginState::FirstTimeSetup { 
                        auth_state: auth_state.clone() 
                    };
                    return Ok(LoginResult::FirstTimeSetup { auth_state });
                }

                // Check timezone configuration
                let timezone_configured = self.auth_manager
                    .get_time_info(&server_name, &api_key, user_details.UserID)
                    .await
                    .is_ok();

                if !timezone_configured {
                    self.state = LoginState::TimeZoneSetup { 
                        auth_state: auth_state.clone() 
                    };
                }

                let session_info = SessionInfo {
                    auth_state,
                    is_first_login: !first_login_done,
                    timezone_configured,
                };

                // Save auth state to both old system and new session storage
                if let Err(e) = self.auth_manager.save_auth_state(&session_info.auth_state).await {
                    log::warn!("Failed to save auth state: {}", e);
                }
                
                // Save to session storage for persistence
                if let Err(e) = super::SessionStorage::save_session(&session_info.auth_state) {
                    log::warn!("Failed to save session: {}", e);
                }

                self.state = LoginState::Complete { 
                    session_info: session_info.clone() 
                };
                
                Ok(LoginResult::Success(session_info))
            }
            Err(e) => {
                self.state = LoginState::Error(format!("Login completion failed: {}", e));
                Ok(LoginResult::Error(format!("Login completion failed: {}", e)))
            }
        }
    }

    pub async fn setup_timezone(&mut self, timezone: String, hour_pref: i32, 
        date_format: String) -> Result<LoginResult> {
        
        if let LoginState::TimeZoneSetup { auth_state } = &self.state {
            let time_zone_info = TimeZoneInfo {
                user_id: auth_state.user_details.UserID,
                timezone,
                hour_pref,
                date_format,
            };

            match self.auth_manager.setup_timezone_info(
                &auth_state.server_name, 
                &auth_state.api_key, 
                time_zone_info
            ).await {
                Ok(_) => {
                    let mut session_info = SessionInfo {
                        auth_state: auth_state.clone(),
                        is_first_login: false,
                        timezone_configured: true,
                    };

                    // Update auth state
                    if let Err(e) = self.auth_manager.save_auth_state(&session_info.auth_state).await {
                        log::warn!("Failed to save auth state: {}", e);
                    }

                    self.state = LoginState::Complete { 
                        session_info: session_info.clone() 
                    };
                    
                    Ok(LoginResult::Success(session_info))
                }
                Err(e) => {
                    self.state = LoginState::Error(format!("Timezone setup failed: {}", e));
                    Ok(LoginResult::Error(format!("Timezone setup failed: {}", e)))
                }
            }
        } else {
            Ok(LoginResult::Error("Invalid state for timezone setup".to_string()))
        }
    }
}

// Placeholder for password hashing - implement according to your needs
fn hash_password(password: &str) -> Result<String> {
    // This should match the password hashing used by the web client
    // For now, returning the password as-is, but you should implement proper hashing
    Ok(password.to_string())
}