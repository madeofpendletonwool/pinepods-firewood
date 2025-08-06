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
use super::pages::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppTab {
    Home = 0,
    Podcasts = 1,
    Episodes = 2,
    Player = 3,
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
            1 => Self::Podcasts,
            2 => Self::Episodes,
            3 => Self::Player,
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
            Self::Episodes => "Episodes",
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
            Self::Home => Self::Podcasts,
            Self::Podcasts => Self::Episodes,
            Self::Episodes => Self::Player,
            Self::Player => Self::Queue,
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
            Self::Podcasts => Self::Home,
            Self::Episodes => Self::Podcasts,
            Self::Player => Self::Episodes,
            Self::Queue => Self::Player,
            Self::Saved => Self::Queue,
            Self::Downloads => Self::Saved,
            Self::Search => Self::Downloads,
            Self::Settings => Self::Search,
        }
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
    
    // Animation
    animation_frame: usize,
    last_animation_update: Instant,
}

impl TuiApp {
    pub fn new(session_info: SessionInfo) -> Result<Self> {
        let client = PinepodsClient::new(session_info.auth_state.clone());
        
        // Create audio player
        let audio_player = AudioPlayer::new(client.clone())?;
        
        // Create episodes page and set up audio player
        let mut episodes_page = EpisodesPage::new(client.clone());
        episodes_page.set_audio_player(audio_player.clone());
        
        Ok(Self {
            client: client.clone(),
            session_info,
            active_tab: AppTab::Home,
            should_quit: false,
            
            audio_player: audio_player.clone(),
            
            home_page: HomePage::new(client.clone()),
            podcasts_page: PodcastsPage::new(client.clone()),
            episodes_page,
            player_page: PlayerPage::new(audio_player),
            queue_page: QueuePage::new(client.clone()),
            saved_page: SavedPage::new(client.clone()),
            downloads_page: DownloadsPage::new(client.clone()),
            search_page: SearchPage::new(client.clone()),
            settings_page: SettingsPage::new(client.clone()),
            
            loading: false,
            error_message: None,
            success_message: None,
            message_timeout: None,
            
            animation_frame: 0,
            last_animation_update: Instant::now(),
        })
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
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
                self.handle_tab_switch().await?;
                return Ok(());
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.active_tab = self.active_tab.previous();
                self.clear_messages();
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
            AppTab::Settings => self.settings_page.handle_input(key).await?,
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
        // Auto-load episodes when switching to Episodes tab
        if self.active_tab == AppTab::Episodes {
            if let Err(e) = self.episodes_page.refresh().await {
                self.show_error_message(&format!("Failed to load episodes: {}", e));
            }
        }
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Header with tabs
                Constraint::Min(0),     // Content
                Constraint::Length(3),  // Micro-player
                Constraint::Length(3),  // Footer with shortcuts
            ])
            .split(frame.size());

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
        self.render_messages_overlay(frame, frame.size());
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let tabs = [
            AppTab::Home,
            AppTab::Podcasts,
            AppTab::Episodes,
            AppTab::Player,
            AppTab::Queue,
            AppTab::Saved,
            AppTab::Downloads,
            AppTab::Search,
            AppTab::Settings,
        ];

        let titles: Vec<Line> = tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let icon = tab.icon();
                let title = tab.title();
                let shortcut = format!(" {}", i + 1);
                
                Line::from(vec![
                    Span::raw(icon),
                    Span::raw(" "),
                    Span::raw(title),
                    Span::styled(shortcut, Style::default().fg(Color::DarkGray)),
                ])
            })
            .collect();

        let tabs_widget = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .style(Style::default().fg(Color::Green))
                    .title(format!(" 🌲 Pinepods Firewood - {} ", 
                                  self.session_info.auth_state.user_details.Username
                                      .as_deref().unwrap_or("User")))
            )
            .select(self.active_tab as usize)
            .style(Style::default().fg(Color::Gray))
            .highlight_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            );

        frame.render_widget(tabs_widget, area);
    }

    fn render_micro_player(&self, frame: &mut Frame, area: Rect) {
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
                    Span::styled("🎵 ", Style::default().fg(Color::Green)),
                    Span::styled(title_text, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("by ", Style::default().fg(Color::Gray)),
                    Span::styled(podcast_name, Style::default().fg(Color::Cyan)),
                ]),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
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
                )
                .gauge_style(Style::default().fg(Color::Blue))
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
                    Span::styled(status_icon, Style::default().fg(Color::Yellow)),
                    Span::raw(" "),
                    Span::styled(format!("{}/{}", current_time, total_time), Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("Space", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::raw(" Play/Pause "),
                    Span::styled("4", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::raw(" Player"),
                ]),
            ])
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
            );
            frame.render_widget(controls, layout[2]);
        } else {
            // No episode playing
            let no_player = ratatui::widgets::Paragraph::new("No episode playing")
                .style(Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title("🎵 Player")
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
                    Span::styled(*key, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {}", desc), Style::default().fg(Color::Gray)),
                ];
                
                if i < shortcuts.len() - 1 {
                    spans.push(Span::raw("  "));
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
                    .border_style(Style::default().fg(Color::Blue))
            );

        frame.render_widget(footer, area);
    }

    fn render_messages_overlay(&self, frame: &mut Frame, area: Rect) {
        let message = if let Some(error) = &self.error_message {
            Some((format!("❌ {}", error), Color::Red))
        } else if let Some(success) = &self.success_message {
            Some((format!("✅ {}", success), Color::Green))
        } else {
            None
        };

        if let Some((text, color)) = message {
            let message_area = Rect {
                x: area.x + 4,
                y: area.y + 1,
                width: area.width.saturating_sub(8),
                height: 1,
            };

            let message_widget = ratatui::widgets::Paragraph::new(text)
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
                .alignment(ratatui::layout::Alignment::Center);

            frame.render_widget(message_widget, message_area);
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
        // Initialize all pages
        self.home_page.initialize().await?;
        self.podcasts_page.initialize().await?;
        self.queue_page.initialize().await?;
        self.saved_page.initialize().await?;
        self.downloads_page.initialize().await?;
        
        Ok(())
    }
}