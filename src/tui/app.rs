use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Tabs},
};
use std::time::{Duration, Instant};

use super::device_picker::DevicePicker;
use super::pages::*;
use crate::api::Api;
use crate::auth::SessionInfo;
use crate::nowplaying::NowPlaying;
use crate::playback::Playback;
use crate::settings::SettingsManager;
use crate::theme::ThemeManager;

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
    Devices = 9,
}

impl AppTab {
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
            Self::Devices => "Devices",
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
            Self::Devices => "📡",
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
            "Devices" => Some(Self::Devices),
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
            Self::Devices,
        ]
    }

    /// Tabs that list episodes, where "Play on ▸" applies.
    fn lists_episodes(self) -> bool {
        matches!(
            self,
            Self::Home
                | Self::Episodes
                | Self::Podcasts
                | Self::Queue
                | Self::Saved
                | Self::Downloads
                | Self::Search
        )
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
    client: Api,
    session_info: SessionInfo,
    active_tab: AppTab,
    should_quit: bool,

    // Audio player
    audio_player: Playback,
    remote: NowPlaying,
    /// The "Play on ▸" device picker, while open.
    picker: Option<DevicePicker>,
    /// The last remote-control notice already shown, so each shows once.
    notice_seen: Option<Instant>,

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
    devices_page: DevicesPage,

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
    pub fn new(
        session_info: SessionInfo,
        client: Api,
        audio_player: Playback,
        remote: NowPlaying,
        settings_manager: SettingsManager,
    ) -> Self {
        let mut theme_manager = ThemeManager::new();
        theme_manager.set_theme(settings_manager.theme_name());
        let skip_interval = Duration::from_secs(settings_manager.skip_interval());

        let mut episodes_page = EpisodesPage::new(client.clone());
        episodes_page.set_audio_player(audio_player.clone());

        let mut podcasts_page = PodcastsPage::new(client.clone());
        podcasts_page.set_audio_player(audio_player.clone());

        let mut settings_page = SettingsPage::new();
        settings_page.set_session_info(session_info.clone());

        let mut home_page = HomePage::new(client.clone());
        home_page.set_audio_player(audio_player.clone());

        let mut queue_page = QueuePage::new(client.clone());
        queue_page.set_audio_player(audio_player.clone());

        let mut saved_page = SavedPage::new(client.clone());
        saved_page.set_audio_player(audio_player.clone());

        let mut downloads_page = DownloadsPage::new(client.clone());
        downloads_page.set_audio_player(audio_player.clone());

        let mut search_page = SearchPage::new(client.clone());
        search_page.set_audio_player(audio_player.clone());

        Self {
            client,
            session_info,
            active_tab: AppTab::Home,
            should_quit: false,

            audio_player: audio_player.clone(),
            remote: remote.clone(),
            picker: None,
            notice_seen: None,

            home_page,
            podcasts_page,
            episodes_page,
            player_page: PlayerPage::new(audio_player, skip_interval),
            queue_page,
            saved_page,
            downloads_page,
            search_page,
            settings_page,
            devices_page: DevicesPage::new(remote),

            loading: false,
            error_message: None,
            success_message: None,
            message_timeout: None,

            tab_scroll_offset: 0,

            theme_manager,
            settings_manager,

            animation_frame: 0,
            last_animation_update: Instant::now(),
        }
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn update_theme(&mut self, theme_name: &str) {
        self.theme_manager.set_theme(theme_name);
    }

    pub async fn handle_input(&mut self, key: ratatui::crossterm::event::KeyEvent) -> Result<()> {
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }

        // Ctrl+C always quits, even mid-typing.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Ok(());
        }

        if self.picker.is_some() {
            self.handle_picker_input(key);
            return Ok(());
        }

        // While a text field has focus every other key is text, so skip the
        // single-key shortcuts (q, Space, digits).
        let typing = match self.active_tab {
            AppTab::Search => self.search_page.is_editing(),
            AppTab::Episodes => self.episodes_page.is_editing(),
            _ => false,
        };

        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Char('q')) if !typing => {
                self.should_quit = true;
                return Ok(());
            }
            (KeyModifiers::NONE, KeyCode::Char(' '))
                if !typing && self.active_tab != AppTab::Player =>
            {
                if let Err(e) = self.audio_player.toggle_play_pause() {
                    self.show_error_message(&format!("Failed to control playback: {}", e));
                }
                return Ok(());
            }
            (KeyModifiers::NONE, KeyCode::Tab) if !typing => {
                self.switch_tab(self.next_tab(1)).await?;
                return Ok(());
            }
            (_, KeyCode::BackTab) if !typing => {
                self.switch_tab(self.next_tab(-1)).await?;
                return Ok(());
            }
            (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                self.refresh_current_page().await?;
                return Ok(());
            }
            (KeyModifiers::NONE, KeyCode::Char('o'))
                if !typing && self.active_tab.lists_episodes() =>
            {
                self.open_picker();
                return Ok(());
            }
            (KeyModifiers::NONE, KeyCode::Char(c @ '0'..='9')) if !typing => {
                // Numbers follow the tab order as displayed, which the user can change;
                // 0 is the tenth.
                let index = (c as usize + 10 - '1' as usize) % 10;
                if let Some(&tab) = AppTab::ordered_tabs(&self.settings_manager).get(index) {
                    self.switch_tab(tab).await?;
                }
                return Ok(());
            }
            _ => {}
        }

        // Forward input to active page
        match self.active_tab {
            AppTab::Home => self.home_page.handle_input(key).await?,
            AppTab::Podcasts => self.podcasts_page.handle_input(key).await?,
            AppTab::Episodes => self.episodes_page.handle_input(key).await?,
            AppTab::Player => {
                self.player_page.handle_input(key).await?;
            }
            AppTab::Queue => self.queue_page.handle_input(key).await?,
            AppTab::Saved => self.saved_page.handle_input(key).await?,
            AppTab::Downloads => self.downloads_page.handle_input(key).await?,
            AppTab::Search => self.search_page.handle_input(key).await?,
            AppTab::Devices => self.devices_page.handle_input(key).await?,
            AppTab::Settings => {
                self.settings_page.handle_input(key).await?;
                // Check if theme changed and sync it
                let current_theme = self
                    .settings_page
                    .settings_manager()
                    .theme_name()
                    .to_string();
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
                    self.devices_page.update_theme(&current_theme);
                }
                // Check if tab order changed and sync it
                let current_tab_order = self.settings_page.settings_manager().tab_order().clone();
                if current_tab_order != self.settings_manager.tab_order().clone() {
                    // Reload settings to get updated tab order
                    if let Ok(updated_settings) = crate::settings::SettingsManager::new() {
                        self.settings_manager = updated_settings;
                    }
                }
            }
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
        if let Some(timeout) = self.message_timeout
            && timeout.elapsed() >= Duration::from_secs(3)
        {
            self.error_message = None;
            self.success_message = None;
            self.message_timeout = None;
        }

        self.show_remote_notice();

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
            AppTab::Devices => {}
        }

        Ok(())
    }

    /// The tab `step` places away in the displayed order, wrapping around.
    fn next_tab(&self, step: isize) -> AppTab {
        let tabs = AppTab::ordered_tabs(&self.settings_manager);
        let len = tabs.len() as isize;
        let current = tabs.iter().position(|t| *t == self.active_tab).unwrap_or(0) as isize;
        tabs[(current + step).rem_euclid(len) as usize]
    }

    async fn switch_tab(&mut self, tab: AppTab) -> Result<()> {
        self.active_tab = tab;
        self.clear_messages();
        self.handle_tab_switch().await
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
                Constraint::Length(3), // Header with tabs
                Constraint::Min(5),    // Content (minimum height to prevent overlap)
                Constraint::Length(3), // Micro-player
                Constraint::Length(3), // Footer with shortcuts
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
            AppTab::Devices => self.devices_page.render(frame, main_layout[1]),
        }

        // Render micro-player
        self.render_micro_player(frame, main_layout[2]);

        // Render footer
        self.render_footer(frame, main_layout[3]);

        if let Some(picker) = &mut self.picker {
            picker.render(
                frame,
                frame.area(),
                self.theme_manager.get_colors(),
                &self.remote,
            );
        }

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
        let active_index = all_tabs
            .iter()
            .position(|t| *t == self.active_tab)
            .unwrap_or(0);

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
            format!(
                "◀ 🌲 Pinepods Firewood - {} ▶",
                self.session_info
                    .auth_state
                    .user_details
                    .Username
                    .as_deref()
                    .unwrap_or("User")
            )
        } else if show_left_arrow {
            format!(
                "◀ 🌲 Pinepods Firewood - {} ",
                self.session_info
                    .auth_state
                    .user_details
                    .Username
                    .as_deref()
                    .unwrap_or("User")
            )
        } else if show_right_arrow {
            format!(
                " 🌲 Pinepods Firewood - {} ▶",
                self.session_info
                    .auth_state
                    .user_details
                    .Username
                    .as_deref()
                    .unwrap_or("User")
            )
        } else {
            format!(
                " 🌲 Pinepods Firewood - {} ",
                self.session_info
                    .auth_state
                    .user_details
                    .Username
                    .as_deref()
                    .unwrap_or("User")
            )
        };

        // Find which visible tab is the active one
        let active_idx = active_index;
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
                    .title_style(Style::default().fg(theme_colors.text)),
            )
            .select(selected_index)
            .style(Style::default().fg(theme_colors.text_secondary))
            .highlight_style(
                Style::default()
                    .fg(theme_colors.primary)
                    .bg(theme_colors.container)
                    .add_modifier(Modifier::BOLD),
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
                    Constraint::Percentage(40), // Episode info
                    Constraint::Percentage(30), // Progress bar
                    Constraint::Percentage(30), // Controls and time
                ])
                .split(area);

            // Episode info
            let podcast_name = episode.podcast_name.as_deref().unwrap_or("Unknown");
            let episode_title = &episode.episode_title;
            let title_text = super::text::truncate(episode_title, 30);

            let episode_info = ratatui::widgets::Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("🎵 ", Style::default().fg(theme_colors.accent)),
                    Span::styled(
                        title_text.clone(),
                        Style::default()
                            .fg(theme_colors.text)
                            .add_modifier(Modifier::BOLD),
                    ),
                    // Show loading indicator in title area if loading
                    if let Some(note) = match player_state.playback_state {
                        crate::playback::PlaybackState::Loading
                            if player_state.reconnecting.is_some() =>
                        {
                            Some(" (Reconnecting...)")
                        }
                        crate::playback::PlaybackState::Loading => Some(" (Loading...)"),
                        crate::playback::PlaybackState::Playing if player_state.buffering => {
                            Some(" (Buffering...)")
                        }
                        _ => None,
                    } {
                        Span::styled(
                            note,
                            Style::default()
                                .fg(theme_colors.warning)
                                .add_modifier(Modifier::ITALIC),
                        )
                    } else {
                        Span::raw("")
                    },
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
                    .border_style(Style::default().fg(theme_colors.border)),
            );
            frame.render_widget(episode_info, layout[0]);

            // Progress bar
            let progress = if player_state.total_duration.as_secs() > 0 {
                player_state.current_position.as_secs_f64()
                    / player_state.total_duration.as_secs_f64()
            } else {
                0.0
            };

            let progress_bar = ratatui::widgets::Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(theme_colors.border)),
                )
                .gauge_style(
                    Style::default()
                        .fg(theme_colors.primary)
                        .bg(theme_colors.container),
                )
                .ratio(progress);
            frame.render_widget(progress_bar, layout[1]);

            // Controls and time
            let current_time = Self::format_duration_short(player_state.current_position);
            let total_time = Self::format_duration_short(player_state.total_duration);
            let status_icon = match player_state.playback_state {
                crate::playback::PlaybackState::Playing => "▶️",
                crate::playback::PlaybackState::Paused => "⏸️",
                crate::playback::PlaybackState::Loading => {
                    // Animated loading spinner
                    let spinners = ["🔄", "🔃", "⟳", "⟲"];
                    spinners[self.animation_frame % spinners.len()]
                }
                crate::playback::PlaybackState::Stopped => "⏹️",
                crate::playback::PlaybackState::Error(_) => "❌",
            };

            let controls = ratatui::widgets::Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(status_icon, Style::default().fg(theme_colors.warning)),
                    Span::raw(" "),
                    Span::styled(
                        format!("{}/{}", current_time, total_time),
                        Style::default().fg(theme_colors.text),
                    ),
                    if player_state.speed == 1.0 {
                        Span::raw("")
                    } else {
                        Span::styled(
                            format!(" {}×", player_state.speed),
                            Style::default().fg(theme_colors.accent),
                        )
                    },
                ]),
                Line::from(vec![
                    Span::styled(
                        "Space",
                        Style::default()
                            .fg(theme_colors.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Play/Pause ",
                        Style::default().fg(theme_colors.text_secondary),
                    ),
                    Span::styled(
                        "4",
                        Style::default()
                            .fg(theme_colors.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" Player", Style::default().fg(theme_colors.text_secondary)),
                ]),
            ])
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme_colors.border)),
            );
            frame.render_widget(controls, layout[2]);
        } else {
            // No episode playing
            let no_player = ratatui::widgets::Paragraph::new("No episode playing")
                .style(
                    Style::default()
                        .fg(theme_colors.text_secondary)
                        .add_modifier(Modifier::ITALIC),
                )
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(theme_colors.border))
                        .title("🎵 Player")
                        .title_style(Style::default().fg(theme_colors.accent)),
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
        let shortcuts = [
            ("Tab", "Switch tabs"),
            ("1-0", "Quick tab"),
            ("o", "Play on…"),
            ("Ctrl+R", "Refresh"),
            ("Ctrl+C/Q", "Quit"),
        ];

        let footer_text: Vec<Span> = shortcuts
            .iter()
            .enumerate()
            .flat_map(|(i, (key, desc))| {
                let mut spans = vec![
                    Span::styled(
                        *key,
                        Style::default()
                            .fg(theme_colors.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {}", desc),
                        Style::default().fg(theme_colors.text_secondary),
                    ),
                ];

                if i < shortcuts.len() - 1 {
                    spans.push(Span::styled(
                        "  ",
                        Style::default().fg(theme_colors.text_secondary),
                    ));
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
                    .title_style(Style::default().fg(theme_colors.accent)),
            );

        frame.render_widget(footer, area);
    }

    fn render_messages_overlay(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let message = if let Some(error) = &self.error_message {
            Some((format!("❌ {}", error), theme_colors.error, "Error"))
        } else {
            self.success_message
                .as_ref()
                .map(|success| (format!("✅ {}", success), theme_colors.success, "Success"))
        };

        if let Some((text, color, title)) = message {
            // Create a larger popup for better error display
            let popup_area = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Length(8), // Fixed height popup
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
                        .style(Style::default().bg(theme_colors.container)),
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
            AppTab::Devices => Ok(()), // Devices are pushed by the server
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

    fn selected_episode(&self) -> Option<crate::api::Episode> {
        match self.active_tab {
            AppTab::Home => self.home_page.selected_episode(),
            AppTab::Episodes => self.episodes_page.selected_episode(),
            AppTab::Podcasts => self.podcasts_page.selected_episode(),
            AppTab::Queue => self.queue_page.selected_episode(),
            AppTab::Saved => self.saved_page.selected_episode(),
            AppTab::Downloads => self.downloads_page.selected_episode(),
            AppTab::Search => self.search_page.selected_episode(),
            AppTab::Player | AppTab::Settings | AppTab::Devices => None,
        }
    }

    fn open_picker(&mut self) {
        let Some(episode) = self.selected_episode() else {
            self.show_error_message("Select an episode first");
            return;
        };
        if self.remote.state().devices.is_empty() {
            self.show_error_message("No other devices are connected");
            return;
        }
        self.picker = Some(DevicePicker::new(episode));
    }

    fn handle_picker_input(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        let devices = self.remote.state().devices;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.picker = None,
            KeyCode::Down | KeyCode::Char('j') => picker.move_by(1, devices.len()),
            KeyCode::Up | KeyCode::Char('k') => picker.move_by(-1, devices.len()),
            KeyCode::Enter => {
                if let Some(device) = devices.get(picker.selected()) {
                    self.remote
                        .play_on(&device.device_id, picker.episode().key());
                }
                self.picker = None;
            }
            _ => {}
        }
    }

    /// Shows what came of a remote command (offline target, a handoff landing) as a
    /// toast, whichever tab is open.
    fn show_remote_notice(&mut self) {
        let Some(notice) = self.remote.state().notice else {
            return;
        };
        if self.notice_seen == Some(notice.at) {
            return;
        }
        self.notice_seen = Some(notice.at);
        if notice.is_error {
            self.show_error_message(&notice.text);
        } else {
            self.show_success_message(&notice.text);
        }
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
        match self.client.get_theme().await {
            Ok(server_theme) => {
                tracing::info!("Fetched theme from server: {}", server_theme);
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
                    self.devices_page.update_theme(&server_theme);
                } else {
                    tracing::warn!("Could not update local settings with server theme");
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch theme from server, using local settings: {}",
                    e
                );
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
}
