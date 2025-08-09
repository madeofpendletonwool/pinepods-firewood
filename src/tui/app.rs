use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Tabs},
    Frame,
};
use std::time::{Duration, Instant};

use crate::api::PinepodsClient;
use crate::auth::SessionInfo;
use crate::audio::AudioPlayer;
use crate::settings::SettingsManager;
use crate::theme::ThemeManager;
use super::pages::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppTab {
    Home = 0,
    Player = 1,
    Episodes = 2,
    Podcasts = 3,
    Queue = 4,
    Saved = 5,
    Downloads = 6,
    Search = 7,
    Settings = 8,
}

impl AppTab {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Home,
            1 => Self::Player,
            2 => Self::Episodes,
            3 => Self::Podcasts,
            4 => Self::Queue,
            5 => Self::Saved,
            6 => Self::Downloads,
            7 => Self::Search,
            8 => Self::Settings,
            _ => Self::Home,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Podcasts => "Podcasts",
            Self::Episodes => "Feed",
            Self::Player => "Player",
            Self::Queue => "Queue",
            Self::Saved => "Saved",
            Self::Downloads => "Downloads",
            Self::Search => "Search",
            Self::Settings => "Settings",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Home => "🏠",
            Self::Podcasts => "🎙️",
            Self::Episodes => "📻",
            Self::Player => "🎵",
            Self::Queue => "📝",
            Self::Saved => "⭐",
            Self::Downloads => "📥",
            Self::Search => "🔍",
            Self::Settings => "⚙️",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Home => Self::Player,
            Self::Player => Self::Episodes,
            Self::Episodes => Self::Podcasts,
            Self::Podcasts => Self::Queue,
            Self::Queue => Self::Saved,
            Self::Saved => Self::Downloads,
            Self::Downloads => Self::Search,
            Self::Search => Self::Settings,
            Self::Settings => Self::Home,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Home => Self::Settings,
            Self::Player => Self::Home,
            Self::Episodes => Self::Player,
            Self::Podcasts => Self::Episodes,
            Self::Queue => Self::Podcasts,
            Self::Saved => Self::Queue,
            Self::Downloads => Self::Saved,
            Self::Search => Self::Downloads,
            Self::Settings => Self::Search,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Home" => Some(Self::Home),
            "Player" => Some(Self::Player),
            "Feed" => Some(Self::Episodes),
            "Podcasts" => Some(Self::Podcasts),
            "Queue" => Some(Self::Queue),
            "Saved" => Some(Self::Saved),
            "Downloads" => Some(Self::Downloads),
            "Search" => Some(Self::Search),
            "Settings" => Some(Self::Settings),
            _ => None,
        }
    }

    pub fn all_tabs() -> Vec<Self> {
        vec![
            Self::Home,
            Self::Player,
            Self::Episodes,
            Self::Podcasts,
            Self::Queue,
            Self::Saved,
            Self::Downloads,
            Self::Search,
            Self::Settings,
        ]
    }
    
    pub fn ordered_tabs(settings_manager: &crate::settings::SettingsManager) -> Vec<Self> {
        let tab_order = settings_manager.tab_order();
        let mut ordered = Vec::new();
        
        // Add tabs according to user's preferred order
        for tab_name in tab_order {
            if let Some(tab) = Self::from_name(tab_name) {
                ordered.push(tab);
            }
        }
        
        // Add any missing tabs (in case settings are incomplete)
        for tab in Self::all_tabs() {
            if !ordered.contains(&tab) {
                ordered.push(tab);
            }
        }
        
        ordered
    }
}

pub struct TuiApp {
    // Core state
    client: PinepodsClient,
    session_info: SessionInfo,
    active_tab: AppTab,
    should_quit: bool,
    
    // Audio player
    audio_player: AudioPlayer,
    
    // Pages
    home_page: HomePage,
    podcasts_page: PodcastsPage,
    episodes_page: EpisodesPage,
    player_page: PlayerPage,
    queue_page: QueuePage,
    saved_page: SavedPage,
    downloads_page: DownloadsPage,
    search_page: SearchPage,
    settings_page: SettingsPage,
    
    // UI state
    loading: bool,
    error_message: Option<String>,
    success_message: Option<String>,
    message_timeout: Option<Instant>,
    
    // Tab scrolling state
    tab_scroll_offset: usize,
    
    // Theme management
    theme_manager: ThemeManager,
    
    // Settings management
    settings_manager: SettingsManager,
    
    // Animation
    animation_frame: usize,
    last_animation_update: Instant,
}

impl TuiApp {
    pub fn new(session_info: SessionInfo) -> Result<Self> {
        let client = PinepodsClient::new(session_info.auth_state.clone());
        
        // Load settings to get audio device preference
        let settings_manager = SettingsManager::new().unwrap_or_else(|e| {
            log::warn!("Failed to load settings, using defaults: {}", e);
            SettingsManager::new().expect("Failed to create fallback settings")
        });
        
        let selected_device = settings_manager.selected_audio_device();
        
        // Initialize theme manager with current theme from settings
        let mut theme_manager = ThemeManager::new();
        theme_manager.set_theme(settings_manager.theme_name());
        
        // Create audio player with selected device
        let audio_player = AudioPlayer::new_with_device(client.clone(), selected_device)?;
        
        // Create episodes page and set up audio player
        let mut episodes_page = EpisodesPage::new(client.clone());
        episodes_page.set_audio_player(audio_player.clone());
        
        // Create podcasts page and set up audio player
        let mut podcasts_page = PodcastsPage::new(client.clone());
        podcasts_page.set_audio_player(audio_player.clone());
        
        // Create settings page and set session info
        let mut settings_page = SettingsPage::new(client.clone());
        settings_page.set_session_info(session_info.clone());
        
        Ok(Self {
            client: client.clone(),
            session_info,
            active_tab: AppTab::Home,
            should_quit: false,
            
            audio_player: audio_player.clone(),
            
            home_page: HomePage::new(client.clone()),
            podcasts_page,
            episodes_page,
            player_page: PlayerPage::new(audio_player),
            queue_page: QueuePage::new(client.clone()),
            saved_page: SavedPage::new(client.clone()),
            downloads_page: DownloadsPage::new(client.clone()),
            search_page: SearchPage::new(client.clone()),
            settings_page,
            
            loading: false,
            error_message: None,
            success_message: None,
            message_timeout: None,
            
            tab_scroll_offset: 0,
            
            theme_manager,
            settings_manager,
            
            animation_frame: 0,
            last_animation_update: Instant::now(),
        })
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn client(&self) -> &PinepodsClient {
        &self.client
    }

    pub fn audio_player(&self) -> &AudioPlayer {
        &self.audio_player
    }

    pub fn update_theme(&mut self, theme_name: &str) {
        self.theme_manager.set_theme(theme_name);
    }

    pub async fn handle_input(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }

        // Global shortcuts
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) | 
            (KeyModifiers::NONE, KeyCode::Char('q')) => {
                self.should_quit = true;
                return Ok(());
            }
            (KeyModifiers::NONE, KeyCode::Char(' ')) => {
                // Global play/pause unless we're in Player tab (let player handle it)
                if self.active_tab != AppTab::Player {
                    if let Err(e) = self.audio_player.toggle_play_pause() {
                        self.show_error_message(&format!("Failed to control playback: {}", e));
                    }
                    return Ok(());
                }
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.active_tab = self.active_tab.next();
                self.clear_messages();
                self.update_tab_scroll();
                self.handle_tab_switch().await?;
                return Ok(());
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.active_tab = self.active_tab.previous();
                self.clear_messages();
                self.update_tab_scroll();
                self.handle_tab_switch().await?;
                return Ok(());
            }
            (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                self.refresh_current_page().await?;
                return Ok(());
            }
            _ => {}
        }

        // Number key shortcuts for tabs
        if let KeyCode::Char(c) = key.code {
            if let Some(digit) = c.to_digit(10) {
                if digit >= 1 && digit <= 9 {
                    self.active_tab = AppTab::from_index((digit - 1) as usize);
                    self.clear_messages();
                    self.update_tab_scroll();
                    self.handle_tab_switch().await?;
                    return Ok(());
                }
            }
        }

        // Forward input to active page
        match self.active_tab {
            AppTab::Home => self.home_page.handle_input(key).await?,
            AppTab::Podcasts => self.podcasts_page.handle_input(key).await?,
            AppTab::Episodes => self.episodes_page.handle_input(key).await?,
            AppTab::Player => { self.player_page.handle_input(key).await?; },
            AppTab::Queue => self.queue_page.handle_input(key).await?,
            AppTab::Saved => self.saved_page.handle_input(key).await?,
            AppTab::Downloads => self.downloads_page.handle_input(key).await?,
            AppTab::Search => self.search_page.handle_input(key).await?,
            AppTab::Settings => {
                self.settings_page.handle_input(key).await?;
                // Check if theme changed and sync it
                let current_theme = self.settings_page.settings_manager().theme_name().to_string();
                if current_theme != self.theme_manager.get_theme_name() {
                    self.update_theme(&current_theme);
                    // Also update all other pages
                    self.home_page.update_theme(&current_theme);
                    self.podcasts_page.update_theme(&current_theme);
                    self.episodes_page.update_theme(&current_theme);
                    self.player_page.update_theme(&current_theme);
                    self.queue_page.update_theme(&current_theme);
                    self.saved_page.update_theme(&current_theme);
                    self.downloads_page.update_theme(&current_theme);
                    self.search_page.update_theme(&current_theme);
                }
                // Check if tab order changed and sync it
                let current_tab_order = self.settings_page.settings_manager().tab_order().clone();
                if current_tab_order != self.settings_manager.tab_order().clone() {
                    // Reload settings to get updated tab order
                    if let Ok(updated_settings) = crate::settings::SettingsManager::new() {
                        self.settings_manager = updated_settings;
                    }
                }
            },
        }

        Ok(())
    }

    pub async fn update(&mut self) -> Result<()> {
        // Update animation
        if self.last_animation_update.elapsed() >= Duration::from_millis(100) {
            self.animation_frame = (self.animation_frame + 1) % 8;
            self.last_animation_update = Instant::now();
        }

        // Clear expired messages
        if let Some(timeout) = self.message_timeout {
            if timeout.elapsed() >= Duration::from_secs(3) {
                self.error_message = None;
                self.success_message = None;
                self.message_timeout = None;
            }
        }

        // Update active page
        match self.active_tab {
            AppTab::Home => self.home_page.update().await?,
            AppTab::Podcasts => self.podcasts_page.update().await?,
            AppTab::Episodes => self.episodes_page.update().await?,
            AppTab::Player => self.player_page.update()?,
            AppTab::Queue => self.queue_page.update().await?,
            AppTab::Saved => self.saved_page.update().await?,
            AppTab::Downloads => self.downloads_page.update().await?,
            AppTab::Search => self.search_page.update().await?,
            AppTab::Settings => self.settings_page.update().await?,
        }

        Ok(())
    }

    async fn handle_tab_switch(&mut self) -> Result<()> {
        // Auto-load data when switching to specific tabs
        match self.active_tab {
            AppTab::Episodes => {
                if let Err(e) = self.episodes_page.refresh().await {
                    self.show_error_message(&format!("Failed to load episodes: {}", e));
                }
            }
            AppTab::Podcasts => {
                if let Err(e) = self.podcasts_page.refresh().await {
                    self.show_error_message(&format!("Failed to load podcasts: {}", e));
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Header with tabs
                Constraint::Min(5),     // Content (minimum height to prevent overlap)
                Constraint::Length(3),  // Micro-player
                Constraint::Length(3),  // Footer with shortcuts
            ])
            .split(frame.area());

        // Render header with tabs
        self.render_header(frame, main_layout[0]);

        // Render active page content
        match self.active_tab {
            AppTab::Home => self.home_page.render(frame, main_layout[1]),
            AppTab::Podcasts => self.podcasts_page.render(frame, main_layout[1]),
            AppTab::Episodes => self.episodes_page.render(frame, main_layout[1]),
            AppTab::Player => self.player_page.render(frame, main_layout[1]),
            AppTab::Queue => self.queue_page.render(frame, main_layout[1]),
            AppTab::Saved => self.saved_page.render(frame, main_layout[1]),
            AppTab::Downloads => self.downloads_page.render(frame, main_layout[1]),
            AppTab::Search => self.search_page.render(frame, main_layout[1]),
            AppTab::Settings => self.settings_page.render(frame, main_layout[1]),
        }

        // Render micro-player
        self.render_micro_player(frame, main_layout[2]);
        
        // Render footer
        self.render_footer(frame, main_layout[3]);

        // Render messages overlay if needed
        self.render_messages_overlay(frame, frame.area());
    }

    fn render_header(&mut self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let all_tabs = AppTab::ordered_tabs(&self.settings_manager);

        // Calculate how many tabs can fit based on terminal width
        let available_width = area.width.saturating_sub(2) as usize; // Account for borders only
        
        // Calculate approximate width per tab (icon + space + title + shortcut + padding)
        let avg_tab_width = 14; // More accurate estimate: icon(2) + space(1) + title(6-8) + shortcut(2) + padding(2)
        let max_visible_tabs = (available_width / avg_tab_width).max(1).min(all_tabs.len());
        
        // Simple scrolling: show a window of tabs based on scroll offset
        let visible_count = max_visible_tabs;
        let active_index = self.active_tab as usize;
        
        // Adjust scroll offset to ensure active tab is visible
        let mut scroll_offset = self.tab_scroll_offset;
        let start_idx = scroll_offset.min(all_tabs.len().saturating_sub(visible_count));
        let end_idx = (start_idx + visible_count).min(all_tabs.len());
        
        // If active tab is outside the visible window, adjust scroll offset
        if active_index < start_idx {
            scroll_offset = active_index;
        } else if active_index >= end_idx {
            scroll_offset = active_index.saturating_sub(visible_count - 1);
        }
        
        // Ensure scroll offset doesn't go beyond valid range
        let max_scroll = all_tabs.len().saturating_sub(visible_count);
        if scroll_offset > max_scroll {
            scroll_offset = max_scroll;
        }
        
        // Update the stored scroll offset and recalculate
        self.tab_scroll_offset = scroll_offset;
        let start_idx = scroll_offset.min(all_tabs.len().saturating_sub(visible_count));
        let end_idx = (start_idx + visible_count).min(all_tabs.len());
        
        let visible_tabs = &all_tabs[start_idx..end_idx];
        let show_left_arrow = start_idx > 0;
        let show_right_arrow = end_idx < all_tabs.len();

        let titles: Vec<Line> = visible_tabs
            .iter()
            .enumerate()
            .map(|(visible_idx, tab)| {
                let actual_idx = start_idx + visible_idx;
                let icon = tab.icon();
                let title = tab.title();
                let shortcut = format!(" {}", actual_idx + 1);
                
                Line::from(vec![
                    Span::styled(icon, Style::default().fg(theme_colors.accent)),
                    Span::raw(" "),
                    Span::styled(title, Style::default().fg(theme_colors.text)),
                    Span::styled(shortcut, Style::default().fg(theme_colors.text_secondary)),
                ])
            })
            .collect();

        // Create tabs widget with arrow indicators in title
        let tab_title = if show_left_arrow && show_right_arrow {
            format!("◀ 🌲 Pinepods Firewood - {} ▶", 
                   self.session_info.auth_state.user_details.Username.as_deref().unwrap_or("User"))
        } else if show_left_arrow {
            format!("◀ 🌲 Pinepods Firewood - {} ", 
                   self.session_info.auth_state.user_details.Username.as_deref().unwrap_or("User"))
        } else if show_right_arrow {
            format!(" 🌲 Pinepods Firewood - {} ▶", 
                   self.session_info.auth_state.user_details.Username.as_deref().unwrap_or("User"))
        } else {
            format!(" 🌲 Pinepods Firewood - {} ", 
                   self.session_info.auth_state.user_details.Username.as_deref().unwrap_or("User"))
        };

        // Find which visible tab is the active one
        let active_idx = self.active_tab as usize;
        let selected_index = if active_idx >= start_idx && active_idx < end_idx {
            active_idx - start_idx
        } else {
            0
        };

        let tabs_widget = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .style(Style::default().fg(theme_colors.accent))
                    .border_style(Style::default().fg(theme_colors.accent))
                    .title(tab_title)
                    .title_style(Style::default().fg(theme_colors.text))
            )
            .select(selected_index)
            .style(Style::default().fg(theme_colors.text_secondary))
            .highlight_style(
                Style::default()
                    .fg(theme_colors.primary)
                    .bg(theme_colors.container)
                    .add_modifier(Modifier::BOLD)
            );

        frame.render_widget(tabs_widget, area);
    }

    fn render_micro_player(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let player_state = self.audio_player.get_state();
        
        if let Some(ref episode) = player_state.current_episode {
            let layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(40),  // Episode info
                    Constraint::Percentage(30),  // Progress bar
                    Constraint::Percentage(30),  // Controls and time
                ])
                .split(area);

            // Episode info
            let podcast_name = episode.podcast_name.as_deref().unwrap_or("Unknown");
            let episode_title = &episode.episode_title;
            let title_text = if episode_title.len() > 30 {
                format!("{}...", &episode_title[..27])
            } else {
                episode_title.clone()
            };

            let episode_info = ratatui::widgets::Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("🎵 ", Style::default().fg(theme_colors.accent)),
                    Span::styled(title_text, Style::default().fg(theme_colors.text).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("by ", Style::default().fg(theme_colors.text_secondary)),
                    Span::styled(podcast_name, Style::default().fg(theme_colors.primary)),
                ]),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme_colors.border))
            );
            frame.render_widget(episode_info, layout[0]);

            // Progress bar
            let progress = if player_state.total_duration.as_secs() > 0 {
                player_state.current_position.as_secs_f64() / player_state.total_duration.as_secs_f64()
            } else {
                0.0
            };

            let progress_bar = ratatui::widgets::Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(theme_colors.border))
                )
                .gauge_style(Style::default().fg(theme_colors.primary).bg(theme_colors.container))
                .ratio(progress);
            frame.render_widget(progress_bar, layout[1]);

            // Controls and time
            let current_time = Self::format_duration_short(player_state.current_position);
            let total_time = Self::format_duration_short(player_state.total_duration);
            let status_icon = match player_state.playback_state {
                crate::audio::PlaybackState::Playing => "▶️",
                crate::audio::PlaybackState::Paused => "⏸️",
                crate::audio::PlaybackState::Loading => "🔄",
                crate::audio::PlaybackState::Stopped => "⏹️",
                crate::audio::PlaybackState::Error(_) => "❌",
            };

            let controls = ratatui::widgets::Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(status_icon, Style::default().fg(theme_colors.warning)),
                    Span::raw(" "),
                    Span::styled(format!("{}/{}", current_time, total_time), Style::default().fg(theme_colors.text)),
                ]),
                Line::from(vec![
                    Span::styled("Space", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                    Span::styled(" Play/Pause ", Style::default().fg(theme_colors.text_secondary)),
                    Span::styled("4", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                    Span::styled(" Player", Style::default().fg(theme_colors.text_secondary)),
                ]),
            ])
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme_colors.border))
            );
            frame.render_widget(controls, layout[2]);
        } else {
            // No episode playing
            let no_player = ratatui::widgets::Paragraph::new("No episode playing")
                .style(Style::default().fg(theme_colors.text_secondary).add_modifier(Modifier::ITALIC))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(theme_colors.border))
                        .title("🎵 Player")
                        .title_style(Style::default().fg(theme_colors.accent))
                );
            frame.render_widget(no_player, area);
        }
    }

    fn format_duration_short(duration: std::time::Duration) -> String {
        let total_seconds = duration.as_secs();
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{}:{:02}", minutes, seconds)
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let shortcuts = vec![
            ("Tab", "Switch tabs"),
            ("1-9", "Quick tab"),
            ("Ctrl+R", "Refresh"),
            ("Ctrl+C/Q", "Quit"),
        ];

        let footer_text: Vec<Span> = shortcuts
            .iter()
            .enumerate()
            .flat_map(|(i, (key, desc))| {
                let mut spans = vec![
                    Span::styled(*key, Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {}", desc), Style::default().fg(theme_colors.text_secondary)),
                ];
                
                if i < shortcuts.len() - 1 {
                    spans.push(Span::styled("  ", Style::default().fg(theme_colors.text_secondary)));
                }
                
                spans
            })
            .collect();

        let footer = ratatui::widgets::Paragraph::new(Line::from(footer_text))
            .alignment(ratatui::layout::Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme_colors.border))
                    .title("🔧 Controls")
                    .title_style(Style::default().fg(theme_colors.accent))
            );

        frame.render_widget(footer, area);
    }

    fn render_messages_overlay(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let message = if let Some(error) = &self.error_message {
            Some((format!("❌ {}", error), theme_colors.error, "Error"))
        } else if let Some(success) = &self.success_message {
            Some((format!("✅ {}", success), theme_colors.success, "Success"))
        } else {
            None
        };

        if let Some((text, color, title)) = message {
            // Create a larger popup for better error display
            let popup_area = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Length(8),  // Fixed height popup
                    Constraint::Percentage(20),
                ])
                .split(area)[1];

            let popup_area = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(10),
                    Constraint::Percentage(80),
                    Constraint::Percentage(10),
                ])
                .split(popup_area)[1];

            frame.render_widget(ratatui::widgets::Clear, popup_area);

            let message_widget = ratatui::widgets::Paragraph::new(text)
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
                .alignment(ratatui::layout::Alignment::Left)
                .wrap(ratatui::widgets::Wrap { trim: true })
                .block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .border_type(ratatui::widgets::BorderType::Rounded)
                        .title(format!("{} (Press any key to dismiss)", title))
                        .border_style(Style::default().fg(color))
                        .style(Style::default().bg(theme_colors.container))
                );

            frame.render_widget(message_widget, popup_area);
        }
    }

    async fn refresh_current_page(&mut self) -> Result<()> {
        self.loading = true;
        
        let result = match self.active_tab {
            AppTab::Home => self.home_page.refresh().await,
            AppTab::Podcasts => self.podcasts_page.refresh().await,
            AppTab::Episodes => self.episodes_page.refresh().await,
            AppTab::Player => Ok(()), // Player doesn't need refresh
            AppTab::Queue => self.queue_page.refresh().await,
            AppTab::Saved => self.saved_page.refresh().await,
            AppTab::Downloads => self.downloads_page.refresh().await,
            AppTab::Search => self.search_page.refresh().await,
            AppTab::Settings => self.settings_page.refresh().await,
        };

        self.loading = false;

        match result {
            Ok(_) => {
                self.show_success_message("Page refreshed successfully");
            }
            Err(e) => {
                self.show_error_message(&format!("Failed to refresh: {}", e));
            }
        }

        Ok(())
    }

    pub fn show_error_message(&mut self, message: &str) {
        self.error_message = Some(message.to_string());
        self.success_message = None;
        self.message_timeout = Some(Instant::now());
    }

    pub fn show_success_message(&mut self, message: &str) {
        self.success_message = Some(message.to_string());
        self.error_message = None;
        self.message_timeout = Some(Instant::now());
    }

    fn clear_messages(&mut self) {
        self.error_message = None;
        self.success_message = None;
        self.message_timeout = None;
    }

    pub async fn initialize(&mut self) -> Result<()> {
        // Fetch theme from server and sync with local settings
        let user_id = self.session_info.auth_state.user_details.UserID;
        match crate::theme::ThemeManager::fetch_theme_from_server(&self.client, user_id).await {
            Ok(server_theme) => {
                log::info!("Fetched theme from server: {}", server_theme);
                // Update local settings with server theme
                if let Ok(mut settings_manager) = crate::settings::SettingsManager::new() {
                    let _ = settings_manager.update(|s| {
                        s.theme.theme_name = server_theme.clone();
                    });
                    // Update theme manager with server theme
                    self.theme_manager.set_theme(&server_theme);
                    // Update all page theme managers
                    self.settings_page.update_theme(&server_theme);
                    self.home_page.update_theme(&server_theme);
                    self.podcasts_page.update_theme(&server_theme);
                    self.episodes_page.update_theme(&server_theme);
                    self.player_page.update_theme(&server_theme);
                    self.queue_page.update_theme(&server_theme);
                    self.saved_page.update_theme(&server_theme);
                    self.downloads_page.update_theme(&server_theme);
                    self.search_page.update_theme(&server_theme);
                } else {
                    log::warn!("Could not update local settings with server theme");
                }
            }
            Err(e) => {
                log::warn!("Failed to fetch theme from server, using local settings: {}", e);
                // Continue with local theme - this is not a fatal error
            }
        }
        
        // Initialize all pages
        self.home_page.initialize().await?;
        self.podcasts_page.initialize().await?;
        self.queue_page.initialize().await?;
        self.saved_page.initialize().await?;
        self.downloads_page.initialize().await?;
        
        Ok(())
    }


    fn update_tab_scroll(&mut self) {
        // This method will be called during tab switching, but we need the terminal width
        // to calculate visible_count. For now, we'll use a simple approach that works
        // with the render method's calculations. The actual scroll adjustment happens
        // in render_header when we know the available width.
        
        let total_tabs: usize = 9; // Total number of tabs
        let active_index = self.active_tab as usize;
        
        // Basic bounds checking - the render method will do the real work
        if self.tab_scroll_offset > total_tabs.saturating_sub(1) {
            self.tab_scroll_offset = total_tabs.saturating_sub(1);
        }
    }
}