use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph},
};
use std::time::Instant;

use crate::api::PinepodsClient;
use crate::settings::{SettingsManager, get_available_audio_devices};
use crate::theme::ThemeManager;

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingItem {
    AutoLoadEpisodes,
    DefaultVolume,
    SkipInterval,
    AudioDevice,
    RemoteControlEnabled,
    RemoteControlPort,
    Theme,
    UseBorders,
    TabOrder,
    ViewLogs,
}

impl SettingItem {
    fn title(&self) -> &'static str {
        match self {
            Self::AutoLoadEpisodes => "Auto-load Episodes",
            Self::DefaultVolume => "Default Volume",
            Self::SkipInterval => "Skip Interval (seconds)",
            Self::AudioDevice => "Audio Output Device",
            Self::RemoteControlEnabled => "Remote Control",
            Self::RemoteControlPort => "Remote Control Port",
            Self::Theme => "Theme",
            Self::UseBorders => "Use Borders",
            Self::TabOrder => "Tab Order",
            Self::ViewLogs => "View Application Logs",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::AutoLoadEpisodes => "Auto-load episodes when navigating podcasts/downloads",
            Self::DefaultVolume => "Default volume level (0-100%)",
            Self::SkipInterval => "Number of seconds to skip forward/backward",
            Self::AudioDevice => "Select audio output device",
            Self::RemoteControlEnabled => "Enable remote control HTTP server",
            Self::RemoteControlPort => "Port for remote control server",
            Self::Theme => "Application theme (Nordic, Abyss, Light)",
            Self::UseBorders => "Display borders around UI elements",
            Self::TabOrder => "Customize tab order and appearance",
            Self::ViewLogs => "View application log file contents",
        }
    }
}

const ALL_SETTINGS: &[SettingItem] = &[
    SettingItem::AutoLoadEpisodes,
    SettingItem::DefaultVolume,
    SettingItem::SkipInterval,
    SettingItem::AudioDevice,
    SettingItem::RemoteControlEnabled,
    SettingItem::RemoteControlPort,
    SettingItem::Theme,
    SettingItem::UseBorders,
    SettingItem::TabOrder,
    SettingItem::ViewLogs,
];

pub struct SettingsPage {
    client: PinepodsClient,
    session_info: Option<crate::auth::SessionInfo>,
    
    // Settings management
    settings_manager: SettingsManager,
    
    // UI State
    list_state: ListState,
    error_message: Option<String>,
    success_message: Option<String>,
    
    // Audio device management
    available_audio_devices: Vec<(String, String)>,
    
    // Theme management
    theme_manager: crate::theme::ThemeManager,
    theme_selector_open: bool,
    theme_selector_state: ListState,
    
    // Audio device selector  
    audio_selector_open: bool,
    audio_selector_state: ListState,
    
    // Tab order selector
    tab_order_selector_open: bool,
    tab_order_selector_state: ListState,
    tab_order_editing: Vec<String>, // Working copy while editing
    
    // Log viewer
    log_viewer_open: bool,
    log_viewer_scroll: usize,
    log_contents: Vec<String>, // Cached log lines
    
    // Animation
    last_update: Instant,
}

impl SettingsPage {
    pub fn new(client: PinepodsClient) -> Self {
        // Note: session_info will be set via set_session_info method
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        let settings_manager = SettingsManager::new().unwrap_or_else(|e| {
            log::error!("Failed to create settings manager: {}", e);
            // Create a fallback - this will use default settings
            SettingsManager::new().expect("Failed to create fallback settings manager")
        });
        
        let available_audio_devices = vec![("default".to_string(), "System Default".to_string())]; // Lazy load when needed
        
        // Initialize theme manager with current theme from settings
        let mut theme_manager = crate::theme::ThemeManager::new();
        theme_manager.set_theme(settings_manager.theme_name());
        
        // Initialize theme selector state
        let mut theme_selector_state = ListState::default();
        theme_selector_state.select(Some(0));
        
        // Initialize audio selector state
        let mut audio_selector_state = ListState::default();
        audio_selector_state.select(Some(0));
        
        // Initialize tab order selector state
        let mut tab_order_selector_state = ListState::default();
        tab_order_selector_state.select(Some(0));
        
        Self {
            client,
            session_info: None,
            settings_manager,
            list_state,
            error_message: None,
            success_message: None,
            available_audio_devices,
            theme_manager,
            theme_selector_open: false,
            theme_selector_state,
            audio_selector_open: false,
            audio_selector_state,
            tab_order_selector_open: false,
            tab_order_selector_state,
            tab_order_editing: Vec::new(),
            log_viewer_open: false,
            log_viewer_scroll: 0,
            log_contents: Vec::new(),
            last_update: Instant::now(),
        }
    }

    pub async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }

    pub async fn refresh(&mut self) -> Result<()> {
        // Reload settings from disk
        self.settings_manager = SettingsManager::new()?;
        self.success_message = Some("Settings reloaded".to_string());
        Ok(())
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        // Clear messages on any input
        self.error_message = None;
        self.success_message = None;

        // Handle popup inputs first
        if self.theme_selector_open {
            return self.handle_theme_selector_input(key).await;
        }
        
        if self.audio_selector_open {
            return self.handle_audio_selector_input(key).await;
        }
        
        if self.tab_order_selector_open {
            return self.handle_tab_order_selector_input(key).await;
        }
        
        if self.log_viewer_open {
            return self.handle_log_viewer_input(key).await;
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(selected) = self.list_state.selected() {
                    if selected < ALL_SETTINGS.len().saturating_sub(1) {
                        self.list_state.select(Some(selected + 1));
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(selected) = self.list_state.selected() {
                    if selected > 0 {
                        self.list_state.select(Some(selected - 1));
                    }
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                self.toggle_setting().await?;
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.decrease_setting().await?;
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.increase_setting().await?;
            }
            KeyCode::Char('-') => {
                self.decrease_setting().await?;
            }
            KeyCode::Char('r') => {
                self.refresh().await?;
            }
            KeyCode::Char('s') => {
                // Save settings manually
                match self.settings_manager.save() {
                    Ok(_) => self.success_message = Some("Settings saved!".to_string()),
                    Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                }
            }
            _ => {}
        }
        
        Ok(())
    }

    async fn toggle_setting(&mut self) -> Result<()> {
        if let Some(selected) = self.list_state.selected() {
            if let Some(setting) = ALL_SETTINGS.get(selected) {
                match setting {
                    SettingItem::AutoLoadEpisodes => {
                        let result = self.settings_manager.update(|s| {
                            s.auto_load_episodes = !s.auto_load_episodes;
                        });
                        match result {
                            Ok(_) => self.success_message = Some(format!("Auto-load episodes: {}", 
                                if self.settings_manager.auto_load_episodes() { "Enabled" } else { "Disabled" })),
                            Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                        }
                    }
                    SettingItem::RemoteControlEnabled => {
                        let result = self.settings_manager.update(|s| {
                            s.remote_control.enabled = !s.remote_control.enabled;
                        });
                        match result {
                            Ok(_) => self.success_message = Some(format!("Remote control: {}", 
                                if self.settings_manager.remote_control_enabled() { "Enabled" } else { "Disabled" })),
                            Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                        }
                    }
                    SettingItem::AudioDevice => {
                        self.open_audio_selector();
                    }
                    SettingItem::Theme => {
                        self.open_theme_selector();
                    }
                    SettingItem::UseBorders => {
                        let result = self.settings_manager.update(|s| {
                            s.theme.use_borders = !s.theme.use_borders;
                        });
                        match result {
                            Ok(_) => self.success_message = Some(format!("Borders: {}", 
                                if self.settings_manager.get().theme.use_borders { "Enabled" } else { "Disabled" })),
                            Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                        }
                    }
                    SettingItem::TabOrder => {
                        self.open_tab_order_selector();
                    }
                    SettingItem::ViewLogs => {
                        self.open_log_viewer().await?;
                    }
                    _ => {
                        // For non-toggle settings, increase the value
                        self.increase_setting().await?;
                    }
                }
            }
        }
        Ok(())
    }

    async fn increase_setting(&mut self) -> Result<()> {
        if let Some(selected) = self.list_state.selected() {
            if let Some(setting) = ALL_SETTINGS.get(selected) {
                match setting {
                    SettingItem::DefaultVolume => {
                        let result = self.settings_manager.update(|s| {
                            s.default_volume = (s.default_volume + 0.1).min(1.0);
                        });
                        match result {
                            Ok(_) => self.success_message = Some(format!("Volume: {}%", 
                                (self.settings_manager.default_volume() * 100.0) as u32)),
                            Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                        }
                    }
                    SettingItem::SkipInterval => {
                        let result = self.settings_manager.update(|s| {
                            s.skip_interval = (s.skip_interval + 5).min(120);
                        });
                        match result {
                            Ok(_) => self.success_message = Some(format!("Skip interval: {}s", 
                                self.settings_manager.skip_interval())),
                            Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                        }
                    }
                    SettingItem::RemoteControlPort => {
                        let result = self.settings_manager.update(|s| {
                            s.remote_control.preferred_port = s.remote_control.preferred_port.saturating_add(1).min(65535);
                        });
                        match result {
                            Ok(_) => self.success_message = Some(format!("Port: {}", 
                                self.settings_manager.remote_control_port())),
                            Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                        }
                    }
                    SettingItem::AudioDevice => {
                        self.open_audio_selector();
                    }
                    SettingItem::Theme => {
                        self.open_theme_selector();
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    async fn decrease_setting(&mut self) -> Result<()> {
        if let Some(selected) = self.list_state.selected() {
            if let Some(setting) = ALL_SETTINGS.get(selected) {
                match setting {
                    SettingItem::DefaultVolume => {
                        let result = self.settings_manager.update(|s| {
                            s.default_volume = (s.default_volume - 0.1).max(0.0);
                        });
                        match result {
                            Ok(_) => self.success_message = Some(format!("Volume: {}%", 
                                (self.settings_manager.default_volume() * 100.0) as u32)),
                            Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                        }
                    }
                    SettingItem::SkipInterval => {
                        let result = self.settings_manager.update(|s| {
                            s.skip_interval = s.skip_interval.saturating_sub(5).max(5);
                        });
                        match result {
                            Ok(_) => self.success_message = Some(format!("Skip interval: {}s", 
                                self.settings_manager.skip_interval())),
                            Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                        }
                    }
                    SettingItem::RemoteControlPort => {
                        let result = self.settings_manager.update(|s| {
                            s.remote_control.preferred_port = s.remote_control.preferred_port.saturating_sub(1).max(1024);
                        });
                        match result {
                            Ok(_) => self.success_message = Some(format!("Port: {}", 
                                self.settings_manager.remote_control_port())),
                            Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                        }
                    }
                    SettingItem::AudioDevice => {
                        self.open_audio_selector();
                    }
                    SettingItem::Theme => {
                        self.open_theme_selector();
                    }
                    SettingItem::TabOrder => {
                        self.open_tab_order_selector();
                    }
                    SettingItem::ViewLogs => {
                        self.open_log_viewer().await?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub async fn update(&mut self) -> Result<()> {
        self.last_update = Instant::now();
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(5),    // Settings list
                Constraint::Length(3), // Current server info
                Constraint::Length(4), // Connection info
                Constraint::Length(3), // Web UI message
                Constraint::Length(3), // Footer/status
            ])
            .split(area);

        // Header
        let theme_colors = self.theme_manager.get_colors();
        let header = Paragraph::new("⚙️  Application Settings")
            .style(Style::default().fg(theme_colors.accent).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme_colors.accent))
            );
        frame.render_widget(header, main_layout[0]);

        // Settings list
        self.render_settings_list(frame, main_layout[1]);

        // Current server info
        self.render_current_server_info(frame, main_layout[2]);

        // Connection info
        self.render_connection_info(frame, main_layout[3]);

        // Web UI message
        self.render_web_ui_message(frame, main_layout[4]);

        // Footer with controls
        self.render_footer(frame, main_layout[5]);

        // Render popups if open
        if self.theme_selector_open {
            self.render_theme_selector(frame, area);
        }
        
        if self.audio_selector_open {
            self.render_audio_selector(frame, area);
        }
        
        if self.tab_order_selector_open {
            self.render_tab_order_selector(frame, area);
        }
        
        if self.log_viewer_open {
            self.render_log_viewer(frame, area);
        }
    }

    fn render_settings_list(&mut self, frame: &mut Frame, area: Rect) {
        let settings = self.settings_manager.get();
        let theme_colors = self.theme_manager.get_colors();

        let items: Vec<ListItem> = ALL_SETTINGS
            .iter()
            .map(|setting| {
                let title = setting.title();
                let description = setting.description();
                
                let value = match setting {
                    SettingItem::AutoLoadEpisodes => {
                        if settings.auto_load_episodes { "Enabled" } else { "Disabled" }.to_string()
                    }
                    SettingItem::DefaultVolume => {
                        format!("{}%", (settings.default_volume * 100.0) as u32)
                    }
                    SettingItem::SkipInterval => {
                        format!("{}s", settings.skip_interval)
                    }
                    SettingItem::AudioDevice => {
                        if let Some(device) = &settings.audio.selected_device {
                            self.available_audio_devices.iter()
                                .find(|(name, _)| name == device)
                                .map(|(_, display)| display.clone())
                                .unwrap_or_else(|| device.clone())
                        } else {
                            "System Default".to_string()
                        }
                    }
                    SettingItem::RemoteControlEnabled => {
                        if settings.remote_control.enabled { "Enabled" } else { "Disabled" }.to_string()
                    }
                    SettingItem::RemoteControlPort => {
                        settings.remote_control.preferred_port.to_string()
                    }
                    SettingItem::Theme => {
                        settings.theme.theme_name.clone()
                    }
                    SettingItem::UseBorders => {
                        if settings.theme.use_borders { "Enabled" } else { "Disabled" }.to_string()
                    }
                    SettingItem::TabOrder => {
                        format!("{} tabs configured", settings.ui.tab_order.len())
                    }
                    SettingItem::ViewLogs => {
                        "Click to view".to_string()
                    }
                };

                let value_color = match setting {
                    SettingItem::AutoLoadEpisodes | SettingItem::RemoteControlEnabled | SettingItem::UseBorders => {
                        if value == "Enabled" { theme_colors.success } else { theme_colors.error }
                    }
                    _ => theme_colors.primary,
                };

                let line1 = Line::from(vec![
                    Span::styled(title, Style::default().fg(theme_colors.text).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" → {}", value), Style::default().fg(value_color).add_modifier(Modifier::BOLD)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(description, Style::default().fg(theme_colors.text_secondary)),
                ]);

                ListItem::new(Text::from(vec![line1, line2]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Settings")
                    .border_style(Style::default().fg(theme_colors.border))
            )
            .highlight_style(
                Style::default()
                    .bg(theme_colors.highlight)
                    .fg(theme_colors.background)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(70), // Controls
                Constraint::Percentage(30), // Status message
            ])
            .split(area);

        // Controls
        let controls = if self.theme_selector_open || self.audio_selector_open {
            Paragraph::new("↑/↓: Navigate • Enter: Select • Esc: Cancel")
        } else if self.tab_order_selector_open {
            Paragraph::new("↑/↓: Navigate • u: Move up • d: Move down • r: Reset • Enter: Save • Esc: Cancel")
        } else if self.log_viewer_open {
            Paragraph::new("↑/↓: Scroll • r: Refresh • Esc: Close")
        } else {
            Paragraph::new("Enter: Select • ←/→: Adjust • +/-: Adjust • r: Reload • s: Save")
        }
            .style(Style::default().fg(theme_colors.text_secondary))
            .alignment(Alignment::Left)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Controls")
                    .border_style(Style::default().fg(theme_colors.border))
            );
        frame.render_widget(controls, layout[0]);

        // Status message
        let (message, color) = if let Some(ref error) = self.error_message {
            (format!("❌ {}", error), theme_colors.error)
        } else if let Some(ref success) = self.success_message {
            (format!("✅ {}", success), theme_colors.success)
        } else {
            ("Ready".to_string(), theme_colors.text_secondary)
        };

        let status = Paragraph::new(message)
            .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Status")
                    .border_style(Style::default().fg(theme_colors.border))
            );
        frame.render_widget(status, layout[1]);
    }

    fn render_connection_info(&self, frame: &mut Frame, area: Rect) {
        let settings = self.settings_manager.get();
        let theme_colors = self.theme_manager.get_colors();
        let port = settings.remote_control.preferred_port;
        
        // Try to get local IP address
        let ip_address = match local_ip_address::local_ip() {
            Ok(ip) => ip.to_string(),
            Err(_) => "127.0.0.1".to_string(),
        };
        
        let connection_text = format!(
            "Add this Firewood server to your PinePods instance:\nIP Address: {}  •  Port: {}",
            ip_address, port
        );
        
        let connection_info = Paragraph::new(connection_text)
            .style(Style::default().fg(theme_colors.warning))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("🔗 PinePods Integration")
                    .border_style(Style::default().fg(theme_colors.primary))
            );
        frame.render_widget(connection_info, area);
    }

    fn render_theme_selector(&mut self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let available_themes = ThemeManager::available_themes();
        
        // Create a popup in the center
        let popup_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Length(available_themes.len() as u16 + 4), // +4 for borders and title
                Constraint::Percentage(20),
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
        
        // Clear the background
        frame.render_widget(ratatui::widgets::Clear, popup_area);
        
        let items: Vec<ListItem> = available_themes
            .iter()
            .map(|&theme_name| {
                ListItem::new(theme_name)
            })
            .collect();
        
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("🎨 Select Theme (Enter to select, Esc to cancel)")
                    .border_style(Style::default().fg(theme_colors.accent))
                    .style(Style::default().bg(theme_colors.container))
            )
            .highlight_style(
                Style::default()
                    .bg(theme_colors.primary)
                    .fg(theme_colors.background)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_stateful_widget(list, popup_area, &mut self.theme_selector_state);
    }

    fn render_audio_selector(&mut self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        
        // Create a popup in the center
        let popup_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Length(self.available_audio_devices.len() as u16 + 4), // +4 for borders and title
                Constraint::Percentage(30),
            ])
            .split(area)[1];
            
        let popup_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(15),
                Constraint::Percentage(70),
                Constraint::Percentage(15),
            ])
            .split(popup_area)[1];
        
        // Clear the background
        frame.render_widget(ratatui::widgets::Clear, popup_area);
        
        let items: Vec<ListItem> = self.available_audio_devices
            .iter()
            .map(|(_, display_name)| {
                ListItem::new(display_name.as_str())
            })
            .collect();
        
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("🔊 Select Audio Device (Enter to select, Esc to cancel)")
                    .border_style(Style::default().fg(theme_colors.accent))
                    .style(Style::default().bg(theme_colors.container))
            )
            .highlight_style(
                Style::default()
                    .bg(theme_colors.primary)
                    .fg(theme_colors.background)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_stateful_widget(list, popup_area, &mut self.audio_selector_state);
    }

    fn render_web_ui_message(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        // Get server name from client if available
        let server_name = "your PinePods server"; // Could be enhanced to get actual server name
        
        let message_text = format!(
            "Additional server settings available on the web UI at {}",
            server_name
        );
        
        let web_ui_message = Paragraph::new(message_text)
            .style(Style::default().fg(theme_colors.accent))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("🌐 Web Interface")
                    .border_style(Style::default().fg(theme_colors.accent))
            );
        frame.render_widget(web_ui_message, area);
    }

    fn open_theme_selector(&mut self) {
        self.theme_selector_open = true;
        // Set the current theme as selected in the dropdown
        let available_themes = ThemeManager::available_themes();
        let current_theme = self.settings_manager.theme_name();
        if let Some(index) = available_themes.iter().position(|&theme| theme == current_theme) {
            self.theme_selector_state.select(Some(index));
        }
    }
    
    fn open_audio_selector(&mut self) {
        // Load audio devices only when opening the selector (lazy loading)
        self.available_audio_devices = get_available_audio_devices();
        self.audio_selector_open = true;
        // Set the current device as selected in the dropdown
        let current_device = self.settings_manager.selected_audio_device();
        if let Some(ref device) = current_device {
            if let Some(index) = self.available_audio_devices.iter().position(|(name, _)| name == device) {
                self.audio_selector_state.select(Some(index));
            }
        } else {
            self.audio_selector_state.select(Some(0)); // Default device
        }
    }

    async fn handle_theme_selector_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.theme_selector_open = false;
            }
            KeyCode::Enter => {
                if let Some(selected) = self.theme_selector_state.selected() {
                    let available_themes = ThemeManager::available_themes();
                    if let Some(&theme_name) = available_themes.get(selected) {
                        let result = self.settings_manager.update(|s| {
                            s.theme.theme_name = theme_name.to_string();
                        });
                        
                        match result {
                            Ok(_) => {
                                self.theme_manager.set_theme(theme_name);
                                self.success_message = Some(format!("Theme: {}", theme_name));
                            }
                            Err(e) => {
                                self.error_message = Some(format!("Failed to save: {}", e));
                            }
                        }
                    }
                }
                self.theme_selector_open = false;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(selected) = self.theme_selector_state.selected() {
                    let available_themes = ThemeManager::available_themes();
                    let new_index = if selected > 0 {
                        selected - 1
                    } else {
                        available_themes.len() - 1
                    };
                    self.theme_selector_state.select(Some(new_index));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(selected) = self.theme_selector_state.selected() {
                    let available_themes = ThemeManager::available_themes();
                    let new_index = (selected + 1) % available_themes.len();
                    self.theme_selector_state.select(Some(new_index));
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_audio_selector_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.audio_selector_open = false;
            }
            KeyCode::Enter => {
                if let Some(selected) = self.audio_selector_state.selected() {
                    if let Some((device_name, display_name)) = self.available_audio_devices.get(selected) {
                        let result = self.settings_manager.update(|s| {
                            s.audio.selected_device = if device_name == "default" {
                                None
                            } else {
                                Some(device_name.clone())
                            };
                        });
                        
                        match result {
                            Ok(_) => {
                                self.success_message = Some(format!("Audio device: {}", display_name));
                            }
                            Err(e) => {
                                self.error_message = Some(format!("Failed to save: {}", e));
                            }
                        }
                    }
                }
                self.audio_selector_open = false;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(selected) = self.audio_selector_state.selected() {
                    let new_index = if selected > 0 {
                        selected - 1
                    } else {
                        self.available_audio_devices.len() - 1
                    };
                    self.audio_selector_state.select(Some(new_index));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(selected) = self.audio_selector_state.selected() {
                    let new_index = (selected + 1) % self.available_audio_devices.len();
                    self.audio_selector_state.select(Some(new_index));
                }
            }
            _ => {}
        }
        Ok(())
    }

    // Getter for settings manager (to be used by other parts of the app)
    pub fn settings_manager(&self) -> &SettingsManager {
        &self.settings_manager
    }

    // Method to update theme from external source (like server sync)
    pub fn update_theme(&mut self, theme_name: &str) {
        self.theme_manager.set_theme(theme_name);
        // Also reload the settings manager to get the updated theme name
        if let Ok(new_settings_manager) = SettingsManager::new() {
            self.settings_manager = new_settings_manager;
        }
    }
    
    fn open_tab_order_selector(&mut self) {
        self.tab_order_selector_open = true;
        // Copy current tab order for editing
        self.tab_order_editing = self.settings_manager.tab_order().clone();
        self.tab_order_selector_state.select(Some(0));
    }
    
    async fn handle_tab_order_selector_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.tab_order_selector_open = false;
                self.tab_order_editing.clear();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(selected) = self.tab_order_selector_state.selected() {
                    let new_index = if selected > 0 {
                        selected - 1
                    } else {
                        self.tab_order_editing.len() - 1
                    };
                    self.tab_order_selector_state.select(Some(new_index));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(selected) = self.tab_order_selector_state.selected() {
                    let new_index = (selected + 1) % self.tab_order_editing.len();
                    self.tab_order_selector_state.select(Some(new_index));
                }
            }
            KeyCode::Char('u') => {
                // Move selected tab up
                if let Some(selected) = self.tab_order_selector_state.selected() {
                    if selected > 0 && selected < self.tab_order_editing.len() {
                        self.tab_order_editing.swap(selected, selected - 1);
                        self.tab_order_selector_state.select(Some(selected - 1));
                    }
                }
            }
            KeyCode::Char('d') => {
                // Move selected tab down
                if let Some(selected) = self.tab_order_selector_state.selected() {
                    if selected < self.tab_order_editing.len() - 1 {
                        self.tab_order_editing.swap(selected, selected + 1);
                        self.tab_order_selector_state.select(Some(selected + 1));
                    }
                }
            }
            KeyCode::Enter => {
                // Save current order and close
                let result = self.settings_manager.set_tab_order(self.tab_order_editing.clone());
                match result {
                    Ok(_) => {
                        self.success_message = Some("Tab order saved!".to_string());
                        self.tab_order_selector_open = false;
                        self.tab_order_editing.clear();
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to save: {}", e));
                    }
                }
            }
            KeyCode::Char('r') => {
                // Reset to default order
                use crate::settings::UiSettings;
                self.tab_order_editing = UiSettings::default().tab_order;
                self.tab_order_selector_state.select(Some(0));
            }
            _ => {}
        }
        Ok(())
    }
    
    fn render_tab_order_selector(&mut self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        
        // Create a popup in the center with better sizing
        let popup_height = (self.tab_order_editing.len() as u16 + 7).min(area.height - 4); // +7 for borders, title, 2-line help
        let popup_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length((area.height - popup_height) / 2),
                Constraint::Length(popup_height),
                Constraint::Min(0),
            ])
            .split(area)[1];
            
        let popup_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(15),
                Constraint::Percentage(70),
                Constraint::Percentage(15),
            ])
            .split(popup_area)[1];
        
        // Clear the background
        frame.render_widget(ratatui::widgets::Clear, popup_area);
        
        // Split the popup into list and help sections
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3), // List area
                Constraint::Length(4), // Help text (2 lines + borders)
            ])
            .split(popup_area);
        
        let items: Vec<ListItem> = self.tab_order_editing
            .iter()
            .enumerate()
            .map(|(index, tab_name)| {
                let prefix = format!("{}. ", index + 1);
                ListItem::new(format!("{}{}", prefix, tab_name))
            })
            .collect();
        
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("📑 Customize Tab Order")
                    .border_style(Style::default().fg(theme_colors.accent))
                    .style(Style::default().bg(theme_colors.container))
            )
            .highlight_style(
                Style::default()
                    .bg(theme_colors.primary)
                    .fg(theme_colors.background)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_stateful_widget(list, popup_layout[0], &mut self.tab_order_selector_state);
        
        // Help text at the bottom (two lines)
        let help_lines = vec![
            Line::from("↑↓: Navigate • u: Move up • d: Move down • r: Reset"),
            Line::from("Enter: Save and close • Esc: Cancel")
        ];
        let help_text = Paragraph::new(help_lines)
            .style(Style::default().fg(theme_colors.text_secondary))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme_colors.border))
                    .style(Style::default().bg(theme_colors.container))
            );
        
        frame.render_widget(help_text, popup_layout[1]);
    }
    
    fn render_current_server_info(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        
        // Get current server info from auth state
        let server_info = if let Some(ref session) = self.session_info {
            let server_name = &session.auth_state.server_name;
            let username = session.auth_state.user_details.Username.as_deref().unwrap_or("Unknown");
            format!("Connected to: {} as {}", server_name, username)
        } else {
            "No server connection info available".to_string()
        };
        
        let server_info_widget = Paragraph::new(server_info)
            .style(Style::default().fg(theme_colors.success))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("🌍 Current Server")
                    .border_style(Style::default().fg(theme_colors.success))
            );
        frame.render_widget(server_info_widget, area);
    }
    
    pub fn set_session_info(&mut self, session_info: crate::auth::SessionInfo) {
        self.session_info = Some(session_info);
    }
    
    async fn open_log_viewer(&mut self) -> Result<()> {
        // Load log file contents
        let log_file = std::path::PathBuf::from(
            dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("pinepods")
                .join("logs")
                .join("pinepods.log")
        );
        match std::fs::read_to_string(&log_file) {
            Ok(contents) => {
                self.log_contents = contents.lines().map(|s| s.to_string()).collect();
                self.log_viewer_open = true;
                self.log_viewer_scroll = if self.log_contents.len() > 20 {
                    self.log_contents.len().saturating_sub(20) // Start at bottom
                } else {
                    0
                };
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to read log file: {}", e));
            }
        }
        Ok(())
    }
    
    async fn handle_log_viewer_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.log_viewer_open = false;
                self.log_contents.clear();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.log_viewer_scroll = self.log_viewer_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max_scroll = self.log_contents.len().saturating_sub(1);
                if self.log_viewer_scroll < max_scroll {
                    self.log_viewer_scroll += 1;
                }
            }
            KeyCode::PageUp => {
                self.log_viewer_scroll = self.log_viewer_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                let max_scroll = self.log_contents.len().saturating_sub(1);
                self.log_viewer_scroll = (self.log_viewer_scroll + 10).min(max_scroll);
            }
            KeyCode::Home => {
                self.log_viewer_scroll = 0;
            }
            KeyCode::End => {
                self.log_viewer_scroll = self.log_contents.len().saturating_sub(20);
            }
            KeyCode::Char('r') => {
                // Refresh log contents
                self.open_log_viewer().await?;
            }
            _ => {}
        }
        Ok(())
    }
    
    fn render_log_viewer(&mut self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        
        // Create a large popup for log viewing
        let popup_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(5),
                Constraint::Percentage(90),
                Constraint::Percentage(5),
            ])
            .split(area)[1];
            
        let popup_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(5),
                Constraint::Percentage(90),
                Constraint::Percentage(5),
            ])
            .split(popup_area)[1];
        
        // Clear the background
        frame.render_widget(ratatui::widgets::Clear, popup_area);
        
        // Calculate visible lines
        let visible_height = popup_area.height.saturating_sub(4) as usize; // Account for borders and title
        let start_line = self.log_viewer_scroll;
        let end_line = (start_line + visible_height).min(self.log_contents.len());
        
        let visible_lines: Vec<Line> = self.log_contents[start_line..end_line]
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let line_num = start_line + i + 1;
                let display_line = if line.len() > popup_area.width.saturating_sub(10) as usize {
                    format!("{}: {}...", line_num, &line[..(popup_area.width.saturating_sub(15) as usize)])
                } else {
                    format!("{}: {}", line_num, line)
                };
                
                // Color code log levels
                if line.contains("ERROR") {
                    Line::from(Span::styled(display_line, Style::default().fg(theme_colors.error)))
                } else if line.contains("WARN") {
                    Line::from(Span::styled(display_line, Style::default().fg(theme_colors.warning)))
                } else if line.contains("DEBUG") {
                    Line::from(Span::styled(display_line, Style::default().fg(theme_colors.text_secondary)))
                } else {
                    Line::from(Span::styled(display_line, Style::default().fg(theme_colors.text)))
                }
            })
            .collect();
        
        let log_paragraph = Paragraph::new(visible_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(format!("📝 Application Logs ({}/{} lines) - Esc: Close, r: Refresh", 
                           end_line, self.log_contents.len()))
                    .border_style(Style::default().fg(theme_colors.accent))
                    .style(Style::default().bg(theme_colors.container))
            );
        
        frame.render_widget(log_paragraph, popup_area);
    }
}