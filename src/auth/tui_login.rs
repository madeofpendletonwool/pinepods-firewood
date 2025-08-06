use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph, 
        Tabs, Wrap
    },
    Frame,
};
use std::time::{Duration, Instant};
use chrono_tz::{TZ_VARIANTS, Tz};

use super::{LoginFlow, LoginState, SessionInfo};
use super::login_flow::LoginResult;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoginTab {
    Server,
    Credentials,
    MFA,
    FirstAdmin,
    Setup,
}

impl LoginTab {
    pub fn next(self) -> Self {
        match self {
            Self::Server => Self::Credentials,
            Self::Credentials => Self::MFA,
            Self::MFA => Self::Setup,
            Self::FirstAdmin => Self::Credentials,
            Self::Setup => Self::Server,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Server => Self::Setup,
            Self::Credentials => Self::Server,
            Self::MFA => Self::Credentials,
            Self::FirstAdmin => Self::Server,
            Self::Setup => Self::MFA,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Server => "Server Connection",
            Self::Credentials => "Login",
            Self::MFA => "Two-Factor Authentication",
            Self::FirstAdmin => "Initial Setup",
            Self::Setup => "First Time Setup",
        }
    }
}

pub struct LoginTui {
    login_flow: LoginFlow,
    active_tab: LoginTab,
    
    // Input fields
    server_input: String,
    username_input: String,
    password_input: String,
    mfa_input: String,
    
    // First admin fields
    admin_username: String,
    admin_password: String,
    admin_email: String,
    admin_fullname: String,
    
    // Setup fields
    timezone: String,
    timezone_index: usize,
    hour_preference: i32,
    hour_preference_index: usize,  // 0 = 12 hour, 1 = 24 hour
    date_format: String,
    date_format_index: usize,      // 0 = MDY, 1 = DMY, 2 = YMD
    
    // UI state
    current_field: usize,
    show_password: bool,
    loading: bool,
    error_message: Option<String>,
    success_message: Option<String>,
    progress: u16,
    
    // Animation
    animation_frame: usize,
    last_animation_update: Instant,
}

impl LoginTui {
    pub fn new() -> Self {
        Self {
            login_flow: LoginFlow::new(),
            active_tab: LoginTab::Server,
            
            server_input: String::new(),
            username_input: String::new(),
            password_input: String::new(),
            mfa_input: String::new(),
            
            admin_username: String::new(),
            admin_password: String::new(),
            admin_email: String::new(),
            admin_fullname: String::new(),
            
            timezone: "UTC".to_string(),
            timezone_index: 0,
            hour_preference: 12,
            hour_preference_index: 0,
            date_format: "MDY".to_string(),
            date_format_index: 0,
            
            current_field: 0,
            show_password: false,
            loading: false,
            error_message: None,
            success_message: None,
            progress: 0,
            
            animation_frame: 0,
            last_animation_update: Instant::now(),
        }
    }

    pub async fn check_existing_auth(&mut self) -> Result<Option<SessionInfo>> {
        // First try to load from session storage (new persistent storage)
        if let Ok(Some(stored_session)) = super::SessionStorage::load_session() {
            log::info!("Found stored session for user: {}", stored_session.username);
            
            match stored_session.to_auth_state() {
                Ok(auth_state) => {
                    // Verify the stored session is still valid by making a test API call
                    if let Err(e) = self.login_flow.auth_manager.verify_pinepods_instance(&auth_state.server_name).await {
                        log::warn!("Stored session server verification failed: {}, clearing session", e);
                        let _ = super::SessionStorage::clear_session();
                    } else {
                        // Create session info from stored session
                        let session_info = SessionInfo {
                            auth_state,
                            is_first_login: false, // If we have a stored session, first login was completed
                            timezone_configured: true, // Assume configured if we have a session
                        };
                        
                        log::info!("Successfully restored session for user: {}", stored_session.username);
                        return Ok(Some(session_info));
                    }
                }
                Err(e) => {
                    log::warn!("Failed to restore auth state from stored session: {}, clearing session", e);
                    let _ = super::SessionStorage::clear_session();
                }
            }
        }
        
        // Fallback to old auth check method
        self.login_flow.check_existing_auth().await
    }

    pub async fn handle_input(&mut self, key: crossterm::event::KeyEvent) -> Result<Option<SessionInfo>> {
        if key.kind != KeyEventKind::Press {
            return Ok(None);
        }

        // Clear messages on new input
        if matches!(key.code, KeyCode::Char(_) | KeyCode::Backspace) {
            self.error_message = None;
            self.success_message = None;
        }

        match key.code {
            KeyCode::Tab => {
                self.next_field();
            }
            KeyCode::BackTab => {
                self.previous_field();
            }
            KeyCode::Enter => {
                return self.handle_submit().await;
            }
            KeyCode::Esc => {
                self.error_message = None;
                self.success_message = None;
            }
            KeyCode::F(1) => {
                self.show_password = !self.show_password;
            }
            KeyCode::Char(c) => {
                self.handle_char_input(c);
            }
            KeyCode::Backspace => {
                self.handle_backspace();
            }
            KeyCode::Up => {
                if self.active_tab == LoginTab::Setup {
                    self.handle_selection_up();
                }
            }
            KeyCode::Down => {
                if self.active_tab == LoginTab::Setup {
                    self.handle_selection_down();
                }
            }
            _ => {}
        }

        Ok(None)
    }

    fn handle_char_input(&mut self, c: char) {
        match self.active_tab {
            LoginTab::Server => {
                if self.current_field == 0 {
                    self.server_input.push(c);
                }
            }
            LoginTab::Credentials => {
                match self.current_field {
                    0 => self.username_input.push(c),
                    1 => self.password_input.push(c),
                    _ => {}
                }
            }
            LoginTab::MFA => {
                if self.current_field == 0 && self.mfa_input.len() < 6 {
                    if c.is_ascii_digit() {
                        self.mfa_input.push(c);
                    }
                }
            }
            LoginTab::FirstAdmin => {
                match self.current_field {
                    0 => self.admin_username.push(c),
                    1 => self.admin_password.push(c),
                    2 => self.admin_email.push(c),
                    3 => self.admin_fullname.push(c),
                    _ => {}
                }
            }
            LoginTab::Setup => {
                // Setup tab uses arrow keys for selection, no character input
            }
        }
    }

    fn handle_backspace(&mut self) {
        match self.active_tab {
            LoginTab::Server => {
                if self.current_field == 0 {
                    self.server_input.pop();
                }
            }
            LoginTab::Credentials => {
                match self.current_field {
                    0 => { self.username_input.pop(); }
                    1 => { self.password_input.pop(); }
                    _ => {}
                }
            }
            LoginTab::MFA => {
                if self.current_field == 0 {
                    self.mfa_input.pop();
                }
            }
            LoginTab::FirstAdmin => {
                match self.current_field {
                    0 => { self.admin_username.pop(); }
                    1 => { self.admin_password.pop(); }
                    2 => { self.admin_email.pop(); }
                    3 => { self.admin_fullname.pop(); }
                    _ => {}
                }
            }
            LoginTab::Setup => {
                // Setup tab uses arrow keys for selection, no backspace handling
            }
        }
    }

    async fn handle_submit(&mut self) -> Result<Option<SessionInfo>> {
        self.loading = true;
        self.progress = 0;
        
        let result = match self.active_tab {
            LoginTab::Server => {
                if self.server_input.is_empty() {
                    self.error_message = Some("Please enter a server URL".to_string());
                    self.loading = false;
                    return Ok(None);
                }
                
                self.progress = 25;
                let result = self.login_flow.verify_server(&self.server_input).await?;
                self.progress = 50;
                result
            }
            LoginTab::Credentials => {
                if self.username_input.is_empty() || self.password_input.is_empty() {
                    self.error_message = Some("Please enter both username and password".to_string());
                    self.loading = false;
                    return Ok(None);
                }
                
                self.progress = 25;
                let result = self.login_flow.authenticate(
                    &self.server_input, 
                    &self.username_input, 
                    &self.password_input
                ).await?;
                self.progress = 75;
                result
            }
            LoginTab::MFA => {
                if self.mfa_input.len() != 6 {
                    self.error_message = Some("Please enter a 6-digit MFA code".to_string());
                    self.loading = false;
                    return Ok(None);
                }
                
                self.progress = 50;
                let result = self.login_flow.verify_mfa(&self.mfa_input).await?;
                self.progress = 90;
                result
            }
            LoginTab::FirstAdmin => {
                if self.admin_username.is_empty() || self.admin_password.is_empty() || 
                   self.admin_email.is_empty() || self.admin_fullname.is_empty() {
                    self.error_message = Some("Please fill in all fields".to_string());
                    self.loading = false;
                    return Ok(None);
                }
                
                self.progress = 25;
                let result = self.login_flow.create_first_admin(
                    &self.server_input,
                    self.admin_username.clone(),
                    self.admin_password.clone(),
                    self.admin_email.clone(),
                    self.admin_fullname.clone(),
                ).await?;
                self.progress = 75;
                result
            }
            LoginTab::Setup => {
                if self.timezone.is_empty() {
                    self.error_message = Some("Please enter a timezone".to_string());
                    self.loading = false;
                    return Ok(None);
                }
                
                self.progress = 50;
                let result = self.login_flow.setup_timezone(
                    self.get_selected_timezone().to_string(),
                    self.get_selected_hour_format(),
                    self.get_selected_date_format().to_string(),
                ).await?;
                self.progress = 100;
                result
            }
        };

        self.loading = false;
        
        match result {
            LoginResult::Success(session_info) => {
                self.success_message = Some("Login successful!".to_string());
                Ok(Some(session_info))
            }
            LoginResult::ServerVerified => {
                self.active_tab = LoginTab::Credentials;
                self.current_field = 0;
                self.success_message = Some("Server verified! Please enter your credentials".to_string());
                Ok(None)
            }
            LoginResult::MfaRequired { .. } => {
                self.active_tab = LoginTab::MFA;
                self.current_field = 0;
                self.mfa_input.clear();
                self.success_message = Some("MFA code required".to_string());
                Ok(None)
            }
            LoginResult::FirstAdminRequired { .. } => {
                self.active_tab = LoginTab::FirstAdmin;
                self.current_field = 0;
                self.success_message = Some("Server requires initial admin setup".to_string());
                Ok(None)
            }
            LoginResult::FirstTimeSetup { .. } => {
                self.active_tab = LoginTab::Setup;
                self.current_field = 0;
                self.success_message = Some("Please complete your profile setup".to_string());
                Ok(None)
            }
            LoginResult::Error(msg) => {
                self.error_message = Some(msg);
                Ok(None)
            }
        }
    }

    fn next_field(&mut self) {
        let max_fields = match self.active_tab {
            LoginTab::Server => 1,
            LoginTab::Credentials => 2,
            LoginTab::MFA => 1,
            LoginTab::FirstAdmin => 4,
            LoginTab::Setup => 3,
        };
        
        self.current_field = (self.current_field + 1) % max_fields;
    }

    fn previous_field(&mut self) {
        let max_fields = match self.active_tab {
            LoginTab::Server => 1,
            LoginTab::Credentials => 2,
            LoginTab::MFA => 1,
            LoginTab::FirstAdmin => 4,
            LoginTab::Setup => 3,
        };
        
        if self.current_field == 0 {
            self.current_field = max_fields - 1;
        } else {
            self.current_field -= 1;
        }
    }

    pub fn update_animation(&mut self) {
        if self.last_animation_update.elapsed() >= Duration::from_millis(200) {
            self.animation_frame = (self.animation_frame + 1) % 4;
            self.last_animation_update = Instant::now();
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        // Update animation
        self.update_animation();

        // Main layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Header
                Constraint::Min(10),    // Content
                Constraint::Length(3),  // Footer
            ])
            .split(area);

        // Render header
        self.render_header(frame, chunks[0]);

        // Render content based on current tab
        match self.active_tab {
            LoginTab::Server => self.render_server_tab(frame, chunks[1]),
            LoginTab::Credentials => self.render_credentials_tab(frame, chunks[1]),
            LoginTab::MFA => self.render_mfa_tab(frame, chunks[1]),
            LoginTab::FirstAdmin => self.render_first_admin_tab(frame, chunks[1]),
            LoginTab::Setup => self.render_setup_tab(frame, chunks[1]),
        }

        // Render footer
        self.render_footer(frame, chunks[2]);

        // Render loading overlay if needed
        if self.loading {
            self.render_loading_overlay(frame, area);
        }
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let title = format!("🌲 Pinepods Firewood - {}", self.active_tab.title());
        let header = Paragraph::new(title)
            .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .style(Style::default().fg(Color::Green))
            );
        
        frame.render_widget(header, area);
    }

    fn render_server_tab(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),  // Instructions
                Constraint::Length(3),  // Input
                Constraint::Min(2),     // Help text
            ])
            .split(area);

        // Instructions
        let instructions = Paragraph::new("Enter your Pinepods server URL (e.g., https://pinepods.example.com)")
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        frame.render_widget(instructions, chunks[0]);

        // Server input
        let input_style = if self.current_field == 0 {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let server_input = Paragraph::new(self.server_input.as_str())
            .style(input_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Server URL")
                    .border_type(BorderType::Rounded)
                    .border_style(if self.current_field == 0 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Gray)
                    })
            );
        frame.render_widget(server_input, chunks[1]);

        // Help text
        let help_text = vec![
            Line::from(vec![
                Span::styled("Examples:", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ]),
            Line::from("  • https://pinepods.example.com"),
            Line::from("  • http://192.168.1.100:8040"),
            Line::from("  • https://my-pinepods-server.com"),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::Gray)),
                Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" to continue", Style::default().fg(Color::Gray)),
            ]),
        ];

        let help = Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Help")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Blue))
            );
        frame.render_widget(help, chunks[2]);
    }

    fn render_credentials_tab(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),  // Instructions
                Constraint::Length(3),  // Username
                Constraint::Length(3),  // Password
                Constraint::Min(2),     // Help
            ])
            .split(area);

        // Instructions
        let instructions = Paragraph::new("Enter your Pinepods login credentials")
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Center);
        frame.render_widget(instructions, chunks[0]);

        // Username input
        let username_style = if self.current_field == 0 {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let username_input = Paragraph::new(self.username_input.as_str())
            .style(username_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Username")
                    .border_type(BorderType::Rounded)
                    .border_style(if self.current_field == 0 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Gray)
                    })
            );
        frame.render_widget(username_input, chunks[1]);

        // Password input
        let password_style = if self.current_field == 1 {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let password_display = if self.show_password {
            self.password_input.clone()
        } else {
            "*".repeat(self.password_input.len())
        };

        let password_input = Paragraph::new(password_display.as_str())
            .style(password_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Password")
                    .border_type(BorderType::Rounded)
                    .border_style(if self.current_field == 1 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Gray)
                    })
            );
        frame.render_widget(password_input, chunks[2]);

        // Help text
        let help_text = vec![
            Line::from(vec![
                Span::styled("Controls:", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("Tab", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" - Switch fields", Style::default().fg(Color::Gray)),
            ]),
            Line::from(vec![
                Span::styled("F1", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" - Toggle password visibility", Style::default().fg(Color::Gray)),
            ]),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" - Login", Style::default().fg(Color::Gray)),
            ]),
        ];

        let help = Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Help")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Blue))
            );
        frame.render_widget(help, chunks[3]);
    }

    fn render_mfa_tab(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),  // Instructions
                Constraint::Length(3),  // MFA input
                Constraint::Min(2),     // Help
            ])
            .split(area);

        // Instructions
        let instructions = Paragraph::new("Enter the 6-digit code from your authenticator app")
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Center);
        frame.render_widget(instructions, chunks[0]);

        // MFA input with visual formatting
        let mfa_display = format!("{:0<6}", self.mfa_input)
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if i < self.mfa_input.len() {
                    c.to_string()
                } else {
                    "_".to_string()
                }
            })
            .collect::<Vec<String>>()
            .join(" ");

        let mfa_input = Paragraph::new(mfa_display)
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("MFA Code")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Yellow))
            );
        frame.render_widget(mfa_input, chunks[1]);

        // Help text
        let help_text = vec![
            Line::from("Enter the 6-digit verification code from your"),
            Line::from("two-factor authentication app (Google Authenticator,"),
            Line::from("Authy, etc.)"),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::Gray)),
                Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" when complete", Style::default().fg(Color::Gray)),
            ]),
        ];

        let help = Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Two-Factor Authentication")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Blue))
            );
        frame.render_widget(help, chunks[2]);
    }

    fn render_first_admin_tab(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),  // Instructions
                Constraint::Length(3),  // Username
                Constraint::Length(3),  // Password
                Constraint::Length(3),  // Email
                Constraint::Length(3),  // Full name
                Constraint::Min(1),     // Help
            ])
            .split(area);

        // Instructions
        let instructions = Paragraph::new("Create the first administrator account for this Pinepods server")
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Center);
        frame.render_widget(instructions, chunks[0]);

        // Input fields
        let fields = [
            ("Username", &self.admin_username),
            ("Password", &self.admin_password),
            ("Email", &self.admin_email),
            ("Full Name", &self.admin_fullname),
        ];

        for (i, (label, value)) in fields.iter().enumerate() {
            let is_active = self.current_field == i;
            let display_value = if label == &"Password" && !self.show_password {
                "*".repeat(value.len())
            } else {
                value.to_string()
            };

            let input = Paragraph::new(display_value.as_str())
                .style(if is_active {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(*label)
                        .border_type(BorderType::Rounded)
                        .border_style(if is_active {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default().fg(Color::Gray)
                        })
                );
            frame.render_widget(input, chunks[i + 1]);
        }

        // Help text
        let help_text = vec![
            Line::from("This will create the first administrator account."),
            Line::from("You'll be able to create additional users later."),
        ];

        let help = Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Initial Setup")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Blue))
            );
        frame.render_widget(help, chunks[5]);
    }

    fn render_setup_tab(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),  // Instructions
                Constraint::Length(3),  // Timezone
                Constraint::Length(3),  // Hour preference
                Constraint::Length(3),  // Date format
                Constraint::Min(1),     // Help
            ])
            .split(area);

        // Instructions
        let instructions = Paragraph::new("Configure your preferences. Use ↑/↓ arrows to change selections.")
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Center);
        frame.render_widget(instructions, chunks[0]);

        // Timezone selection
        let timezone_text = format!("⌚ {} ({})", self.get_selected_timezone(), 
                                  if self.current_field == 0 { "↑↓ to change" } else { "" });
        let timezone_input = Paragraph::new(timezone_text)
            .style(if self.current_field == 0 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("🌍 Timezone")
                    .border_type(BorderType::Rounded)
                    .border_style(if self.current_field == 0 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Gray)
                    })
            );
        frame.render_widget(timezone_input, chunks[1]);

        // Hour format selection
        let hour_format_text = format!("🕐 {} Hour Format ({})", 
                                     self.get_selected_hour_format(),
                                     if self.current_field == 1 { "↑↓ to change" } else { "" });
        let hour_pref_input = Paragraph::new(hour_format_text)
            .style(if self.current_field == 1 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("🕒 Time Format")
                    .border_type(BorderType::Rounded)
                    .border_style(if self.current_field == 1 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Gray)
                    })
            );
        frame.render_widget(hour_pref_input, chunks[2]);

        // Date format selection
        let date_format_text = format!("📅 {} ({})", 
                                     self.get_selected_date_format(),
                                     if self.current_field == 2 { "↑↓ to change" } else { "" });
        let date_format_input = Paragraph::new(date_format_text)
            .style(if self.current_field == 2 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("📆 Date Format")
                    .border_type(BorderType::Rounded)
                    .border_style(if self.current_field == 2 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Gray)
                    })
            );
        frame.render_widget(date_format_input, chunks[3]);

        // Help text
        let help_text = vec![
            Line::from("🎯 These settings can be changed later in your profile."),
            Line::from(""),
            Line::from("Use Tab/Shift+Tab to navigate fields, ↑/↓ arrows to change selections"),
            Line::from("Press Enter when ready to complete setup"),
        ];

        let help = Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("ℹ️  Setup Guide")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Blue))
            );
        frame.render_widget(help, chunks[4]);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let mut footer_text = vec![
            Span::styled("Tab", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("/", Style::default().fg(Color::Gray)),
            Span::styled("Shift+Tab", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(" Navigate  ", Style::default().fg(Color::Gray)),
            Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(" Submit  ", Style::default().fg(Color::Gray)),
            Span::styled("Esc", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(" Clear messages", Style::default().fg(Color::Gray)),
        ];

        if matches!(self.active_tab, LoginTab::Credentials | LoginTab::FirstAdmin) {
            footer_text.extend_from_slice(&[
                Span::styled("  F1", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" Toggle password", Style::default().fg(Color::Gray)),
            ]);
        }

        let footer = Paragraph::new(Line::from(footer_text))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Blue))
            );

        frame.render_widget(footer, area);

        // Render messages if present
        if let Some(error) = &self.error_message {
            let error_area = Rect {
                x: area.x + 2,
                y: area.y + 1,
                width: area.width.saturating_sub(4),
                height: 1,
            };
            
            let error_msg = Paragraph::new(format!("❌ {}", error))
                .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center);
            frame.render_widget(error_msg, error_area);
        }

        if let Some(success) = &self.success_message {
            let success_area = Rect {
                x: area.x + 2,
                y: area.y + 1,
                width: area.width.saturating_sub(4),
                height: 1,
            };
            
            let success_msg = Paragraph::new(format!("✅ {}", success))
                .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center);
            frame.render_widget(success_msg, success_area);
        }
    }

    fn render_loading_overlay(&self, frame: &mut Frame, area: Rect) {
        // Create a centered overlay
        let popup_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Length(7),
                Constraint::Percentage(40),
            ])
            .split(area)[1];

        let popup_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(50),
                Constraint::Percentage(25),
            ])
            .split(popup_area)[1];

        // Clear the area
        frame.render_widget(Clear, popup_area);

        // Loading animation
        let spinner_chars = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
        let spinner = spinner_chars[self.animation_frame % spinner_chars.len()];

        // Create loading content
        let loading_content = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Progress bar
                Constraint::Length(2),  // Spinner and text
            ])
            .margin(1)
            .split(popup_area);

        // Progress bar
        let progress_bar = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Processing...")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan))
            )
            .gauge_style(Style::default().fg(Color::Cyan))
            .percent(self.progress);
        frame.render_widget(progress_bar, loading_content[0]);

        // Spinner and loading text
        let loading_text = Paragraph::new(format!("{} Please wait...", spinner))
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
        frame.render_widget(loading_text, loading_content[1]);

        // Outer border
        let border = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));
        frame.render_widget(border, popup_area);
    }

    // Helper methods for selection options
    fn get_timezone_options() -> Vec<&'static str> {
        TZ_VARIANTS.iter().map(|tz| tz.name()).collect()
    }

    fn get_hour_format_options() -> Vec<&'static str> {
        vec!["12 Hour", "24 Hour"]
    }

    fn get_date_format_options() -> Vec<&'static str> {
        vec!["MM-DD-YYYY", "DD-MM-YYYY", "YYYY-MM-DD"]
    }

    fn get_selected_timezone(&self) -> &str {
        let options = Self::get_timezone_options();
        if self.timezone_index < options.len() {
            options[self.timezone_index]
        } else {
            "UTC"
        }
    }

    fn get_selected_hour_format(&self) -> i32 {
        if self.hour_preference_index == 0 { 12 } else { 24 }
    }

    fn get_selected_date_format(&self) -> &str {
        let options = Self::get_date_format_options();
        if self.date_format_index < options.len() {
            options[self.date_format_index]
        } else {
            "MDY"
        }
    }

    fn handle_selection_up(&mut self) {
        match self.current_field {
            0 => {
                // Timezone selection
                if self.timezone_index > 0 {
                    self.timezone_index -= 1;
                } else {
                    self.timezone_index = Self::get_timezone_options().len() - 1;
                }
                self.timezone = self.get_selected_timezone().to_string();
            }
            1 => {
                // Hour format selection
                self.hour_preference_index = if self.hour_preference_index == 0 { 1 } else { 0 };
                self.hour_preference = self.get_selected_hour_format();
            }
            2 => {
                // Date format selection
                if self.date_format_index > 0 {
                    self.date_format_index -= 1;
                } else {
                    self.date_format_index = Self::get_date_format_options().len() - 1;
                }
                self.date_format = self.get_selected_date_format().to_string();
            }
            _ => {}
        }
    }

    fn handle_selection_down(&mut self) {
        match self.current_field {
            0 => {
                // Timezone selection
                let max_len = Self::get_timezone_options().len();
                if self.timezone_index < max_len - 1 {
                    self.timezone_index += 1;
                } else {
                    self.timezone_index = 0;
                }
                self.timezone = self.get_selected_timezone().to_string();
            }
            1 => {
                // Hour format selection
                self.hour_preference_index = if self.hour_preference_index == 0 { 1 } else { 0 };
                self.hour_preference = self.get_selected_hour_format();
            }
            2 => {
                // Date format selection
                let max_len = Self::get_date_format_options().len();
                if self.date_format_index < max_len - 1 {
                    self.date_format_index += 1;
                } else {
                    self.date_format_index = 0;
                }
                self.date_format = self.get_selected_date_format().to_string();
            }
            _ => {}
        }
    }
}