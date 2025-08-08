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
use crate::settings::SettingsManager;

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingItem {
    AutoLoadEpisodes,
    DefaultVolume,
    SkipInterval,
    RemoteControlEnabled,
    RemoteControlPort,
    ThemeAccentColor,
    UseBorders,
}

impl SettingItem {
    fn title(&self) -> &'static str {
        match self {
            Self::AutoLoadEpisodes => "Auto-load Episodes",
            Self::DefaultVolume => "Default Volume",
            Self::SkipInterval => "Skip Interval (seconds)",
            Self::RemoteControlEnabled => "Remote Control",
            Self::RemoteControlPort => "Remote Control Port",
            Self::ThemeAccentColor => "Accent Color",
            Self::UseBorders => "Use Borders",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::AutoLoadEpisodes => "Auto-load episodes when navigating podcasts/downloads",
            Self::DefaultVolume => "Default volume level (0-100%)",
            Self::SkipInterval => "Number of seconds to skip forward/backward",
            Self::RemoteControlEnabled => "Enable remote control HTTP server",
            Self::RemoteControlPort => "Port for remote control server",
            Self::ThemeAccentColor => "Theme accent color",
            Self::UseBorders => "Display borders around UI elements",
        }
    }
}

const ALL_SETTINGS: &[SettingItem] = &[
    SettingItem::AutoLoadEpisodes,
    SettingItem::DefaultVolume,
    SettingItem::SkipInterval,
    SettingItem::RemoteControlEnabled,
    SettingItem::RemoteControlPort,
    SettingItem::ThemeAccentColor,
    SettingItem::UseBorders,
];

pub struct SettingsPage {
    client: PinepodsClient,
    
    // Settings management
    settings_manager: SettingsManager,
    
    // UI State
    list_state: ListState,
    error_message: Option<String>,
    success_message: Option<String>,
    
    // Animation
    last_update: Instant,
}

impl SettingsPage {
    pub fn new(client: PinepodsClient) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        let settings_manager = SettingsManager::new().unwrap_or_else(|e| {
            log::error!("Failed to create settings manager: {}", e);
            // Create a fallback - this will use default settings
            SettingsManager::new().expect("Failed to create fallback settings manager")
        });
        
        Self {
            client,
            settings_manager,
            list_state,
            error_message: None,
            success_message: None,
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
                Constraint::Length(3), // Footer/status
            ])
            .split(area);

        // Header
        let header = Paragraph::new("⚙️  Application Settings")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan))
            );
        frame.render_widget(header, main_layout[0]);

        // Settings list
        self.render_settings_list(frame, main_layout[1]);

        // Footer with controls
        self.render_footer(frame, main_layout[2]);
    }

    fn render_settings_list(&mut self, frame: &mut Frame, area: Rect) {
        let settings = self.settings_manager.get();

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
                    SettingItem::RemoteControlEnabled => {
                        if settings.remote_control.enabled { "Enabled" } else { "Disabled" }.to_string()
                    }
                    SettingItem::RemoteControlPort => {
                        settings.remote_control.preferred_port.to_string()
                    }
                    SettingItem::ThemeAccentColor => {
                        settings.theme.accent_color.clone()
                    }
                    SettingItem::UseBorders => {
                        if settings.theme.use_borders { "Enabled" } else { "Disabled" }.to_string()
                    }
                };

                let value_color = match setting {
                    SettingItem::AutoLoadEpisodes | SettingItem::RemoteControlEnabled | SettingItem::UseBorders => {
                        if value == "Enabled" { Color::Green } else { Color::Red }
                    }
                    _ => Color::Yellow,
                };

                let line1 = Line::from(vec![
                    Span::styled(title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" → {}", value), Style::default().fg(value_color).add_modifier(Modifier::BOLD)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(description, Style::default().fg(Color::Gray)),
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
                    .border_style(Style::default().fg(Color::Gray))
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(70), // Controls
                Constraint::Percentage(30), // Status message
            ])
            .split(area);

        // Controls
        let controls = Paragraph::new("Enter/→: Toggle • ←/→: Adjust • +/-: Adjust • r: Reload • s: Save")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Left)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Controls")
                    .border_style(Style::default().fg(Color::DarkGray))
            );
        frame.render_widget(controls, layout[0]);

        // Status message
        let (message, color) = if let Some(ref error) = self.error_message {
            (format!("❌ {}", error), Color::Red)
        } else if let Some(ref success) = self.success_message {
            (format!("✅ {}", success), Color::Green)
        } else {
            ("Ready".to_string(), Color::Gray)
        };

        let status = Paragraph::new(message)
            .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Status")
                    .border_style(Style::default().fg(Color::DarkGray))
            );
        frame.render_widget(status, layout[1]);
    }

    // Getter for settings manager (to be used by other parts of the app)
    pub fn settings_manager(&self) -> &SettingsManager {
        &self.settings_manager
    }
}