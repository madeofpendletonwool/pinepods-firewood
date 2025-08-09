use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::api::{PinepodsClient, DownloadItem};
use crate::audio::AudioPlayer;
use crate::settings::SettingsManager;
use crate::theme::ThemeManager;

#[derive(Debug, Clone)]
struct ScrollState {
    offset: usize,
    direction: ScrollDirection,
    pause_until: Instant,
    text_width: usize,
    display_width: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum ScrollDirection {
    Right,
    Left,
    Paused,
}

impl ScrollState {
    fn new(text_width: usize, display_width: usize) -> Self {
        Self {
            offset: 0,
            direction: ScrollDirection::Paused,
            pause_until: Instant::now() + Duration::from_millis(1000), // Initial pause
            text_width,
            display_width,
        }
    }
    
    fn update(&mut self, now: Instant) -> bool {
        // If text fits in display width, no scrolling needed
        if self.text_width <= self.display_width {
            return false;
        }
        
        // Check if we're in a pause period
        if now < self.pause_until {
            return false;
        }
        
        match self.direction {
            ScrollDirection::Paused => {
                self.direction = ScrollDirection::Right;
                true
            }
            ScrollDirection::Right => {
                self.offset += 1;
                if self.offset >= self.text_width - self.display_width + 3 {
                    self.direction = ScrollDirection::Left;
                    self.pause_until = now + Duration::from_millis(1500); // Pause at end
                }
                true
            }
            ScrollDirection::Left => {
                if self.offset > 0 {
                    self.offset -= 1;
                } else {
                    self.direction = ScrollDirection::Paused;
                    self.pause_until = now + Duration::from_millis(2000); // Pause at beginning
                }
                true
            }
        }
    }
    
    fn get_display_text(&self, text: &str) -> String {
        if self.text_width <= self.display_width {
            return text.to_string();
        }
        
        let chars: Vec<char> = text.chars().collect();
        let end_pos = (self.offset + self.display_width).min(chars.len());
        
        if self.offset < chars.len() {
            chars[self.offset..end_pos].iter().collect()
        } else {
            String::new()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FocusPanel {
    Podcasts,
    Downloads,
}

pub struct DownloadsPage {
    client: PinepodsClient,
    
    // Data
    all_downloads: Vec<DownloadItem>,
    podcasts_with_downloads: Vec<PodcastWithDownloads>,
    selected_podcast_downloads: Vec<DownloadItem>,
    
    // UI State
    podcast_list_state: ListState,
    download_list_state: ListState,
    focused_panel: FocusPanel,
    loading: bool,
    loading_downloads: bool,
    error_message: Option<String>,
    selected_podcast_id: Option<i64>,
    
    // Animation
    last_update: Instant,
    
    // Title scrolling animation
    scroll_states: HashMap<String, ScrollState>,
    last_scroll_update: Instant,
    
    // Audio player
    audio_player: Option<AudioPlayer>,
    
    // Settings
    settings_manager: SettingsManager,
    
    // Theme management
    theme_manager: ThemeManager,
}

#[derive(Debug, Clone)]
struct PodcastWithDownloads {
    podcast_id: i64,
    podcast_name: String,
    artwork_url: String,
    download_count: usize,
}

impl DownloadsPage {
    pub fn new(client: PinepodsClient) -> Self {
        let mut podcast_list_state = ListState::default();
        podcast_list_state.select(Some(0));
        
        let settings_manager = SettingsManager::new().unwrap_or_else(|e| {
            log::error!("Failed to create settings manager: {}", e);
            SettingsManager::new().expect("Failed to create fallback settings manager")
        });
        
        Self {
            client,
            all_downloads: Vec::new(),
            podcasts_with_downloads: Vec::new(),
            selected_podcast_downloads: Vec::new(),
            podcast_list_state,
            download_list_state: ListState::default(),
            focused_panel: FocusPanel::Podcasts,
            loading: false,
            loading_downloads: false,
            error_message: None,
            selected_podcast_id: None,
            last_update: Instant::now(),
            scroll_states: HashMap::new(),
            last_scroll_update: Instant::now(),
            audio_player: None,
            settings_manager,
            theme_manager: ThemeManager::new(),
        }
    }

    pub fn set_audio_player(&mut self, audio_player: AudioPlayer) {
        self.audio_player = Some(audio_player);
    }

    pub async fn initialize(&mut self) -> Result<()> {
        self.load_downloads().await
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.load_downloads().await
    }

    async fn load_downloads(&mut self) -> Result<()> {
        self.loading = true;
        self.error_message = None;

        match self.client.get_downloads().await {
            Ok(downloads) => {
                log::info!("Loaded {} downloads", downloads.len());
                self.all_downloads = downloads;
                self.loading = false;
                
                // Group downloads by podcast
                self.group_downloads_by_podcast();
                
                // Auto-select first podcast and load its downloads
                if !self.podcasts_with_downloads.is_empty() {
                    self.podcast_list_state.select(Some(0));
                    if let Some(podcast) = self.podcasts_with_downloads.get(0) {
                        self.load_podcast_downloads(podcast.podcast_id).await?;
                    }
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load downloads: {}", e));
                self.loading = false;
                return Err(e);
            }
        }
        
        Ok(())
    }

    fn group_downloads_by_podcast(&mut self) {
        let mut podcast_map: HashMap<i64, PodcastWithDownloads> = HashMap::new();
        
        for download in &self.all_downloads {
            let entry = podcast_map.entry(download.podcast_id).or_insert_with(|| {
                PodcastWithDownloads {
                    podcast_id: download.podcast_id,
                    podcast_name: download.podcast_name.clone(),
                    artwork_url: download.artwork_url.clone(),
                    download_count: 0,
                }
            });
            entry.download_count += 1;
        }
        
        self.podcasts_with_downloads = podcast_map.into_values().collect();
        // Sort by podcast name
        self.podcasts_with_downloads.sort_by(|a, b| a.podcast_name.cmp(&b.podcast_name));
    }

    async fn load_podcast_downloads(&mut self, podcast_id: i64) -> Result<()> {
        self.loading_downloads = true;
        self.selected_podcast_id = Some(podcast_id);
        self.download_list_state = ListState::default();
        self.download_list_state.select(Some(0));

        // Filter downloads for this podcast
        self.selected_podcast_downloads = self.all_downloads
            .iter()
            .filter(|d| d.podcast_id == podcast_id)
            .cloned()
            .collect();
        
        // Sort by episode pub date (newest first)
        self.selected_podcast_downloads.sort_by(|a, b| b.episode_pub_date.cmp(&a.episode_pub_date));
        
        log::info!("Loaded {} downloads for podcast {}", self.selected_podcast_downloads.len(), podcast_id);
        self.loading_downloads = false;
        
        Ok(())
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                self.focused_panel = FocusPanel::Podcasts;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if !self.selected_podcast_downloads.is_empty() {
                    self.focused_panel = FocusPanel::Downloads;
                }
            }
            KeyCode::Tab => {
                self.focused_panel = match self.focused_panel {
                    FocusPanel::Podcasts => {
                        if !self.selected_podcast_downloads.is_empty() {
                            FocusPanel::Downloads
                        } else {
                            FocusPanel::Podcasts
                        }
                    }
                    FocusPanel::Downloads => FocusPanel::Podcasts,
                };
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match self.focused_panel {
                    FocusPanel::Podcasts => {
                        if let Some(selected) = self.podcast_list_state.selected() {
                            if selected < self.podcasts_with_downloads.len().saturating_sub(1) {
                                self.podcast_list_state.select(Some(selected + 1));
                                
                                // Auto-load downloads if setting is enabled
                                if self.settings_manager.auto_load_episodes() {
                                    if let Some(podcast) = self.podcasts_with_downloads.get(selected + 1) {
                                        let _ = self.load_podcast_downloads(podcast.podcast_id).await;
                                    }
                                }
                            }
                        }
                    }
                    FocusPanel::Downloads => {
                        if let Some(selected) = self.download_list_state.selected() {
                            if selected < self.selected_podcast_downloads.len().saturating_sub(1) {
                                self.download_list_state.select(Some(selected + 1));
                            }
                        }
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                match self.focused_panel {
                    FocusPanel::Podcasts => {
                        if let Some(selected) = self.podcast_list_state.selected() {
                            if selected > 0 {
                                self.podcast_list_state.select(Some(selected - 1));
                                
                                // Auto-load downloads if setting is enabled
                                if self.settings_manager.auto_load_episodes() {
                                    if let Some(podcast) = self.podcasts_with_downloads.get(selected - 1) {
                                        let _ = self.load_podcast_downloads(podcast.podcast_id).await;
                                    }
                                }
                            }
                        }
                    }
                    FocusPanel::Downloads => {
                        if let Some(selected) = self.download_list_state.selected() {
                            if selected > 0 {
                                self.download_list_state.select(Some(selected - 1));
                            }
                        }
                    }
                }
            }
            KeyCode::Enter => {
                match self.focused_panel {
                    FocusPanel::Podcasts => {
                        if let Some(selected) = self.podcast_list_state.selected() {
                            if let Some(podcast) = self.podcasts_with_downloads.get(selected) {
                                self.load_podcast_downloads(podcast.podcast_id).await?;
                                self.focused_panel = FocusPanel::Downloads;
                            }
                        }
                    }
                    FocusPanel::Downloads => {
                        self.play_selected_download().await?;
                    }
                }
            }
            KeyCode::Char('r') => {
                self.refresh().await?;
            }
            KeyCode::Char('d') => {
                if self.focused_panel == FocusPanel::Downloads {
                    if let Some(selected) = self.download_list_state.selected() {
                        if let Some(download) = self.selected_podcast_downloads.get(selected) {
                            log::debug!("Delete key pressed for download: {} (id: {})", download.episode_title, download.episode_id);
                            self.delete_download(download.episode_id, download.is_youtube).await?;
                        }
                    }
                }
            }
            _ => {}
        }
        
        Ok(())
    }

    async fn play_selected_download(&mut self) -> Result<()> {
        if let Some(selected) = self.download_list_state.selected() {
            if let Some(download) = self.selected_podcast_downloads.get(selected) {
                log::info!("Playing downloaded episode: {}", download.episode_title);
                
                if let Some(ref mut audio_player) = self.audio_player {
                    // Convert DownloadItem to Episode for the audio player
                    let episode = crate::api::Episode {
                        episode_id: Some(download.episode_id),
                        podcast_id: Some(download.podcast_id),
                        podcast_name: Some(download.podcast_name.clone()),
                        episode_title: download.episode_title.clone(),
                        episode_pub_date: download.episode_pub_date.clone(),
                        episode_description: download.episode_description.clone(),
                        episode_artwork: download.episode_artwork.clone(),
                        episode_url: download.episode_url.clone(),
                        episode_duration: download.episode_duration,
                        listen_duration: download.listen_duration,
                        completed: Some(download.completed),
                        saved: Some(download.saved),
                        queued: Some(download.queued),
                        downloaded: Some(download.downloaded),
                        is_youtube: Some(download.is_youtube),
                    };
                    
                    audio_player.play_episode(episode)?;
                    self.error_message = Some("🎵 Download started! Switch to Player tab (4) to control playback".to_string());
                } else {
                    log::warn!("No audio player available");
                    self.error_message = Some("Audio player not available".to_string());
                }
            }
        }
        Ok(())
    }

    async fn delete_download(&mut self, episode_id: i64, is_youtube: bool) -> Result<()> {
        log::debug!("delete_download called with episode_id: {}, is_youtube: {}", episode_id, is_youtube);
        match self.client.delete_download(episode_id, is_youtube).await {
            Ok(_) => {
                self.error_message = Some("🗑️ Episode download deleted successfully!".to_string());
                // Refresh to update the UI
                self.refresh().await?;
            }
            Err(e) => {
                log::error!("Failed to delete download: {}", e);
                self.error_message = Some(format!("❌ Failed to delete download: {}", e));
            }
        }
        Ok(())
    }

    pub async fn update(&mut self) -> Result<()> {
        self.last_update = Instant::now();
        
        // Update scrolling animation every 150ms
        let now = Instant::now();
        if now.duration_since(self.last_scroll_update) >= Duration::from_millis(150) {
            self.update_scroll_states(now);
            self.last_scroll_update = now;
        }
        
        Ok(())
    }
    
    fn update_scroll_states(&mut self, now: Instant) {
        // Update all scroll states
        for scroll_state in self.scroll_states.values_mut() {
            scroll_state.update(now);
        }
    }
    
    fn get_or_create_scroll_state(&mut self, key: String, text: &str, display_width: usize) -> &mut ScrollState {
        let text_width = text.chars().count();
        
        self.scroll_states.entry(key).or_insert_with(|| {
            ScrollState::new(text_width, display_width)
        })
    }
    
    fn get_scrolled_text(&mut self, key: String, text: &str, display_width: usize) -> String {
        let scroll_state = self.get_or_create_scroll_state(key, text, display_width);
        
        // Update scroll state dimensions if text OR display width changed
        let text_width = text.chars().count();
        if scroll_state.text_width != text_width || scroll_state.display_width != display_width {
            scroll_state.text_width = text_width;
            scroll_state.display_width = display_width;
            scroll_state.offset = 0;
            scroll_state.direction = ScrollDirection::Paused;
            scroll_state.pause_until = Instant::now() + Duration::from_millis(1000);
        }
        
        scroll_state.get_display_text(text)
    }
    
    // Method to update theme from external source (like server sync)
    pub fn update_theme(&mut self, theme_name: &str) {
        self.theme_manager.set_theme(theme_name);
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),         // Main content
                Constraint::Length(3),      // Help/controls footer
            ])
            .split(area);

        let content_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40),  // Podcasts list
                Constraint::Percentage(60),  // Downloads list
            ])
            .split(main_layout[0]);

        // Render podcasts list
        self.render_podcasts_list(frame, content_layout[0]);
        
        // Render downloads list
        self.render_downloads_list(frame, content_layout[1]);
        
        // Render help/controls footer
        self.render_controls_footer(frame, main_layout[1]);
    }

    fn render_podcasts_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.podcasts_with_downloads.is_empty() && !self.loading {
            let theme_colors = self.theme_manager.get_colors();
            let empty_msg = "No downloads found. Press 'r' to refresh.";
            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme_colors.text_secondary))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title("📥 Podcasts with Downloads")
                        .title_style(Style::default().fg(theme_colors.accent))
                        .border_style(if self.focused_panel == FocusPanel::Podcasts {
                            Style::default().fg(theme_colors.primary)
                        } else {
                            Style::default().fg(theme_colors.border)
                        })
                );
            frame.render_widget(empty, area);
            return;
        }

        // Pre-calculate scrolled podcast names to avoid borrowing issues
        let mut scrolled_names = Vec::new();
        let available_width = area.width as usize;
        
        // First collect the data we need to avoid borrowing conflicts
        let podcast_data: Vec<(usize, String, i64)> = self.podcasts_with_downloads
            .iter()
            .enumerate()
            .map(|(index, podcast)| {
                (index, podcast.podcast_name.clone(), podcast.podcast_id)
            })
            .collect();
        
        // Now we can safely call mutable methods
        for (index, name, podcast_id) in podcast_data {
            // Use most of the available width for podcast names, leaving room for download count
            let name_max_width = available_width.saturating_sub(20).max(30);
            
            // Use scrolling for long podcast names
            let scroll_key = format!("downloads_podcast_{}_{}", index, podcast_id);
            let displayed_name = self.get_scrolled_text(scroll_key, &name, name_max_width);
            scrolled_names.push(displayed_name);
        }
        
        // Get theme colors after mutable operations
        let theme_colors = self.theme_manager.get_colors();

        let items: Vec<ListItem> = self.podcasts_with_downloads
            .iter()
            .enumerate()
            .map(|(index, podcast)| {
                let download_count = podcast.download_count;
                
                // Get pre-calculated scrolled name
                let displayed_name = scrolled_names.get(index)
                    .cloned()
                    .unwrap_or_else(|| podcast.podcast_name.clone());
                
                let line1 = Line::from(vec![
                    Span::styled(displayed_name, Style::default().fg(theme_colors.text).add_modifier(Modifier::BOLD)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(format!("{} downloads", download_count), Style::default().fg(theme_colors.accent)),
                ]);

                ListItem::new(Text::from(vec![line1, line2]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("📥 Podcasts with Downloads")
                    .title_style(Style::default().fg(theme_colors.accent))
                    .border_style(if self.focused_panel == FocusPanel::Podcasts {
                        Style::default().fg(theme_colors.primary)
                    } else {
                        Style::default().fg(theme_colors.border)
                    })
            )
            .highlight_style(
                Style::default()
                    .bg(theme_colors.highlight)
                    .fg(theme_colors.background)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_stateful_widget(list, area, &mut self.podcast_list_state);

        // Show loading overlay for podcasts
        if self.loading {
            self.render_loading_overlay(frame, area, "Loading downloads...");
        }
    }

    fn render_downloads_list(&mut self, frame: &mut Frame, area: Rect) {
        let title = if let Some(selected) = self.podcast_list_state.selected() {
            if let Some(podcast) = self.podcasts_with_downloads.get(selected) {
                format!("📥 Downloads - {}", podcast.podcast_name)
            } else {
                "📥 Downloads".to_string()
            }
        } else {
            "📥 Downloads".to_string()
        };

        if self.selected_podcast_downloads.is_empty() && !self.loading_downloads {
            let theme_colors = self.theme_manager.get_colors();
            let empty_msg = if self.podcasts_with_downloads.is_empty() {
                "Select a podcast to view downloads"
            } else {
                "No downloads found for this podcast"
            };

            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme_colors.text_secondary))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(title)
                        .title_style(Style::default().fg(theme_colors.accent))
                        .border_style(if self.focused_panel == FocusPanel::Downloads {
                            Style::default().fg(theme_colors.primary)
                        } else {
                            Style::default().fg(theme_colors.border)
                        })
                );
            frame.render_widget(empty, area);
            return;
        }

        // Pre-calculate scrolled episode titles to avoid borrowing issues
        let mut scrolled_titles = Vec::new();
        let available_width = area.width as usize;
        
        // First collect the data we need to avoid borrowing conflicts
        let download_data: Vec<(usize, String, i64)> = self.selected_podcast_downloads
            .iter()
            .enumerate()
            .map(|(index, download)| {
                (index, download.episode_title.clone(), download.episode_id)
            })
            .collect();
        
        // Now we can safely call mutable methods
        for (index, title, episode_id) in download_data {
            // Use most of the available width for episode titles, leaving room for status indicators
            let title_max_width = available_width.saturating_sub(26).max(40);
            
            // Use scrolling for long episode titles
            let scroll_key = format!("downloads_episode_{}_{}", index, episode_id);
            let displayed_title = self.get_scrolled_text(scroll_key, &title, title_max_width);
            scrolled_titles.push(displayed_title);
        }
        
        // Get theme colors after mutable operations
        let theme_colors = self.theme_manager.get_colors();

        let items: Vec<ListItem> = self.selected_podcast_downloads
            .iter()
            .enumerate()
            .map(|(index, download)| {
                let duration = format_duration(download.episode_duration);
                let pub_date = format_pub_date(&download.episode_pub_date);
                
                // Get pre-calculated scrolled title
                let displayed_title = scrolled_titles.get(index)
                    .cloned()
                    .unwrap_or_else(|| download.episode_title.clone());
                
                // Status indicators
                let mut indicators = Vec::new();
                if download.completed {
                    indicators.push("✅");
                } else if download.listen_duration.unwrap_or(0) > 0 {
                    indicators.push("▶️");
                }
                if download.saved {
                    indicators.push("⭐");
                }
                if download.queued {
                    indicators.push("📝");
                }
                indicators.push("📥"); // Always show download indicator

                let status = format!(" {}", indicators.join(" "));

                let line1 = Line::from(vec![
                    Span::styled(displayed_title, Style::default().fg(theme_colors.text).add_modifier(Modifier::BOLD)),
                    Span::styled(status, Style::default().fg(theme_colors.success)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(pub_date, Style::default().fg(theme_colors.warning)),
                    Span::styled(format!(" • {}", duration), Style::default().fg(theme_colors.text_secondary)),
                ]);

                ListItem::new(Text::from(vec![line1, line2]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(title)
                    .title_style(Style::default().fg(theme_colors.accent))
                    .border_style(if self.focused_panel == FocusPanel::Downloads {
                        Style::default().fg(theme_colors.primary)
                    } else {
                        Style::default().fg(theme_colors.border)
                    })
            )
            .highlight_style(
                Style::default()
                    .bg(theme_colors.highlight)
                    .fg(theme_colors.background)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_stateful_widget(list, area, &mut self.download_list_state);

        // Show loading overlay for downloads
        if self.loading_downloads {
            self.render_loading_overlay(frame, area, "Loading downloads...");
        }

        // Show error message if present
        if let Some(error) = &self.error_message {
            let error_area = Rect {
                x: area.x + 2,
                y: area.y + area.height - 3,
                width: area.width.saturating_sub(4),
                height: 1,
            };
            
            let color = if error.starts_with("🎵") {
                theme_colors.success
            } else {
                theme_colors.error
            };
            let error_msg = Paragraph::new(format!("❌ {}", error))
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center);
            frame.render_widget(error_msg, error_area);
        }
    }

    fn render_loading_overlay(&self, frame: &mut Frame, area: Rect, message: &str) {
        let theme_colors = self.theme_manager.get_colors();
        let popup_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Length(3),
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

        frame.render_widget(Clear, popup_area);

        let loading = Paragraph::new(format!("🔄 {}", message))
            .style(Style::default().fg(theme_colors.accent).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme_colors.accent))
                    .style(Style::default().bg(theme_colors.container))
            );
        frame.render_widget(loading, popup_area);
    }

    fn render_controls_footer(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let controls = vec![
            ("↑↓/jk", "Navigate"),
            ("←→/hl", "Switch panel"),
            ("Tab", "Switch panel"),
            ("Enter", "Play/Select"),
            ("d", "Delete download"),
            ("r", "Refresh"),
        ];

        let footer_text: Vec<Span> = controls
            .iter()
            .enumerate()
            .flat_map(|(i, (key, desc))| {
                let mut spans = vec![
                    Span::styled(*key, Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {}", desc), Style::default().fg(theme_colors.text_secondary)),
                ];
                
                if i < controls.len() - 1 {
                    spans.push(Span::raw("  "));
                }
                
                spans
            })
            .collect();

        let footer = Paragraph::new(Line::from(footer_text))
            .alignment(Alignment::Center)
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
}

fn format_duration(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{}:{:02}", minutes, secs)
    }
}

fn format_pub_date(date_str: &str) -> String {
    // Simple date formatting - you could use chrono for more sophisticated parsing
    if let Some(date_part) = date_str.split('T').next() {
        date_part.to_string()
    } else {
        date_str.to_string()
    }
}