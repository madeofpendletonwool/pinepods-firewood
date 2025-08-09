use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};
use std::time::{Duration, Instant};
use std::collections::HashMap;

use crate::api::{PinepodsClient, Episode};
use crate::audio::AudioPlayer;
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
pub enum EpisodesFilter {
    All,
    Completed,
    InProgress,
    Unplayed,
}

impl EpisodesFilter {
    pub fn title(&self) -> &'static str {
        match self {
            Self::All => "All Feed",
            Self::Completed => "Completed",
            Self::InProgress => "In Progress", 
            Self::Unplayed => "Unplayed",
        }
    }

    pub fn matches(&self, episode: &Episode) -> bool {
        match self {
            Self::All => true,
            Self::Completed => episode.completed.unwrap_or(false),
            Self::InProgress => episode.listen_duration.unwrap_or(0) > 0 && !episode.completed.unwrap_or(false),
            Self::Unplayed => episode.listen_duration.unwrap_or(0) == 0,
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::All => Self::Completed,
            Self::Completed => Self::InProgress,
            Self::InProgress => Self::Unplayed,
            Self::Unplayed => Self::All,
        }
    }

    pub fn previous(&self) -> Self {
        match self {
            Self::All => Self::Unplayed,
            Self::Completed => Self::All,
            Self::InProgress => Self::Completed,
            Self::Unplayed => Self::InProgress,
        }
    }
}

pub struct EpisodesPage {
    client: PinepodsClient,
    
    // Data
    episodes: Vec<Episode>,
    filtered_episodes: Vec<Episode>,
    
    // UI State
    list_state: ListState,
    current_filter: EpisodesFilter,
    search_query: String,
    search_mode: bool,
    loading: bool,
    error_message: Option<String>,
    
    // Animation
    last_update: Instant,
    
    // Title scrolling animation
    scroll_states: HashMap<String, ScrollState>,
    last_scroll_update: Instant,
    
    // Audio player
    audio_player: Option<AudioPlayer>,
    
    // Theme management
    theme_manager: ThemeManager,
}

impl EpisodesPage {
    pub fn new(client: PinepodsClient) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        Self {
            client,
            episodes: Vec::new(),
            filtered_episodes: Vec::new(),
            list_state,
            current_filter: EpisodesFilter::All,
            search_query: String::new(),
            search_mode: false,
            loading: false,
            error_message: None,
            last_update: Instant::now(),
            audio_player: None,
            theme_manager: ThemeManager::new(),
            scroll_states: HashMap::new(),
            last_scroll_update: Instant::now(),
        }
    }

    pub fn set_audio_player(&mut self, audio_player: AudioPlayer) {
        self.audio_player = Some(audio_player);
    }

    pub async fn initialize(&mut self) -> Result<()> {
        self.load_episodes().await
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.load_episodes().await
    }

    async fn load_episodes(&mut self) -> Result<()> {
        self.loading = true;
        self.error_message = None;

        match self.client.get_recent_episodes().await {
            Ok(episodes) => {
                log::info!("Loaded {} episodes", episodes.len());
                self.episodes = episodes;
                self.apply_filters();
                self.loading = false;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load episodes: {}", e));
                self.loading = false;
                return Err(e);
            }
        }
        
        Ok(())
    }

    fn apply_filters(&mut self) {
        self.filtered_episodes = self.episodes
            .iter()
            .filter(|episode| {
                // Apply current filter
                if !self.current_filter.matches(episode) {
                    return false;
                }
                
                // Apply search query
                if !self.search_query.is_empty() {
                    let query = self.search_query.to_lowercase();
                    let title_matches = episode.episode_title.to_lowercase().contains(&query);
                    let podcast_matches = episode.podcast_name.as_deref().unwrap_or("").to_lowercase().contains(&query);
                    let desc_matches = episode.episode_description.to_lowercase().contains(&query);
                    
                    if !(title_matches || podcast_matches || desc_matches) {
                        return false;
                    }
                }
                
                true
            })
            .cloned()
            .collect();
            
        // Reset selection if needed
        if self.filtered_episodes.is_empty() {
            self.list_state.select(None);
        } else if self.list_state.selected().is_none() || self.list_state.selected().unwrap_or(0) >= self.filtered_episodes.len() {
            self.list_state.select(Some(0));
        }
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        if self.search_mode {
            match key.code {
                KeyCode::Esc => {
                    self.search_mode = false;
                    self.search_query.clear();
                    self.apply_filters();
                }
                KeyCode::Enter => {
                    self.search_mode = false;
                    self.apply_filters();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.apply_filters();
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.apply_filters();
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(selected) = self.list_state.selected() {
                        if selected < self.filtered_episodes.len().saturating_sub(1) {
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
                KeyCode::Enter => {
                    self.play_selected_episode().await?;
                }
                KeyCode::Char('/') => {
                    self.search_mode = true;
                    self.search_query.clear();
                }
                KeyCode::Char('f') => {
                    self.current_filter = self.current_filter.next();
                    self.apply_filters();
                }
                KeyCode::Char('F') => {
                    self.current_filter = self.current_filter.previous();
                    self.apply_filters();
                }
                KeyCode::Char('r') => {
                    self.refresh().await?;
                }
                KeyCode::Char('s') => {
                    if let Some(selected) = self.list_state.selected() {
                        if let Some(episode) = self.filtered_episodes.get(selected) {
                            if let Some(episode_id) = episode.episode_id {
                                log::debug!("Save key pressed for episode: {} (id: {})", episode.episode_title, episode_id);
                                self.toggle_saved(episode_id as i64).await?;
                            } else {
                                log::error!("No episode ID found for episode: {}", episode.episode_title);
                                self.error_message = Some("❌ Cannot save episode: missing ID".to_string());
                            }
                        }
                    }
                }
                KeyCode::Char('a') => {
                    log::debug!("Queue key 'a' pressed - starting handler");
                    if let Some(selected) = self.list_state.selected() {
                        if let Some(episode) = self.filtered_episodes.get(selected) {
                            if let Some(episode_id) = episode.episode_id {
                                log::debug!("Queue key pressed for episode: {} (id: {})", episode.episode_title, episode_id);
                                self.toggle_queued(episode_id as i64).await?;
                            } else {
                                log::error!("No episode ID found for episode: {}", episode.episode_title);
                                self.error_message = Some("❌ Cannot queue episode: missing ID".to_string());
                            }
                        }
                    }
                }
                KeyCode::Char('d') => {
                    log::debug!("Download/Delete key 'd' pressed - starting handler");
                    if let Some(selected) = self.list_state.selected() {
                        if let Some(episode) = self.filtered_episodes.get(selected) {
                            if let Some(episode_id) = episode.episode_id {
                                let is_downloaded = episode.downloaded.unwrap_or(false);
                                if is_downloaded {
                                    log::debug!("Delete download key pressed for episode: {} (id: {})", episode.episode_title, episode_id);
                                    let is_youtube = episode.is_youtube.unwrap_or(false);
                                    self.delete_download(episode_id as i64, is_youtube).await?;
                                } else {
                                    log::debug!("Download key pressed for episode: {} (id: {})", episode.episode_title, episode_id);
                                    self.download_episode(episode_id as i64).await?;
                                }
                            } else {
                                log::error!("No episode ID found for episode: {}", episode.episode_title);
                                self.error_message = Some("❌ Cannot process episode: missing ID".to_string());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        
        Ok(())
    }

    async fn play_selected_episode(&mut self) -> Result<()> {
        if let Some(selected) = self.list_state.selected() {
            if let Some(episode) = self.filtered_episodes.get(selected) {
                log::info!("Playing episode: {}", episode.episode_title);
                
                if let Some(ref mut audio_player) = self.audio_player {
                    audio_player.play_episode(episode.clone())?;
                    // Suggest switching to player tab in the UI
                    self.error_message = Some("🎵 Episode started! Switch to Player tab (4) to control playback".to_string());
                } else {
                    log::warn!("No audio player available");
                    self.error_message = Some("Audio player not available".to_string());
                }
            }
        }
        Ok(())
    }

    async fn toggle_saved(&mut self, episode_id: i64) -> Result<()> {
        // Find the episode in our data
        if let Some(episode) = self.episodes.iter_mut().find(|e| e.episode_id.unwrap_or(0) == episode_id) {
            let is_saved = episode.saved.unwrap_or(false);
            let is_youtube = episode.is_youtube.unwrap_or(false);
            
            if is_saved {
                match self.client.unsave_episode(episode_id, is_youtube).await {
                    Ok(_) => {
                        episode.saved = Some(false);
                        self.error_message = Some("📌 Episode removed from saved".to_string());
                    }
                    Err(e) => {
                        log::error!("Failed to unsave episode: {}", e);
                        self.error_message = Some(format!("❌ Failed to unsave episode: {}", e));
                        return Ok(());
                    }
                }
            } else {
                match self.client.save_episode(episode_id, is_youtube).await {
                    Ok(_) => {
                        episode.saved = Some(true);
                        self.error_message = Some("⭐ Episode saved successfully!".to_string());
                    }
                    Err(e) => {
                        log::error!("Failed to save episode: {}", e);
                        self.error_message = Some(format!("❌ Failed to save episode: {}", e));
                        return Ok(());
                    }
                }
            }
            
            self.apply_filters();
        }
        Ok(())
    }

    async fn toggle_queued(&mut self, episode_id: i64) -> Result<()> {
        log::debug!("toggle_queued called with episode_id: {}", episode_id);
        // Find the episode in our data  
        if let Some(episode) = self.episodes.iter_mut().find(|e| e.episode_id.unwrap_or(0) == episode_id) {
            let is_queued = episode.queued.unwrap_or(false);
            let is_youtube = episode.is_youtube.unwrap_or(false);
            log::debug!("Episode found - is_queued: {}, is_youtube: {}", is_queued, is_youtube);
            
            if is_queued {
                log::debug!("Calling remove_from_queue for episode {}", episode_id);
                match self.client.remove_from_queue(episode_id, is_youtube).await {
                    Ok(_) => {
                        episode.queued = Some(false);
                        self.error_message = Some("📝 Episode removed from queue".to_string());
                    }
                    Err(e) => {
                        log::error!("Failed to remove from queue: {}", e);
                        self.error_message = Some(format!("❌ Failed to remove from queue: {}", e));
                        return Ok(());
                    }
                }
            } else {
                log::debug!("Calling add_to_queue for episode {}", episode_id);
                match self.client.add_to_queue(episode_id, is_youtube).await {
                    Ok(_) => {
                        episode.queued = Some(true);
                        self.error_message = Some("📝 Episode queued successfully!".to_string());
                    }
                    Err(e) => {
                        log::error!("Failed to add to queue: {}", e);
                        self.error_message = Some(format!("❌ Failed to add to queue: {}", e));
                        return Ok(());
                    }
                }
            }
            
            self.apply_filters();
        }
        Ok(())
    }

    async fn download_episode(&mut self, episode_id: i64) -> Result<()> {
        log::debug!("download_episode called with episode_id: {}", episode_id);
        match self.client.download_episode(episode_id).await {
            Ok(_) => {
                self.error_message = Some("📥 Episode download queued successfully!".to_string());
                // Refresh to update the UI with new download status
                self.refresh().await?;
            }
            Err(e) => {
                log::error!("Failed to download episode: {}", e);
                self.error_message = Some(format!("❌ Failed to download episode: {}", e));
            }
        }
        Ok(())
    }

    async fn delete_download(&mut self, episode_id: i64, is_youtube: bool) -> Result<()> {
        log::debug!("delete_download called with episode_id: {}, is_youtube: {}", episode_id, is_youtube);
        match self.client.delete_download(episode_id, is_youtube).await {
            Ok(_) => {
                self.error_message = Some("🗑️ Episode download deleted successfully!".to_string());
                // Refresh to update the UI with new download status
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
        // Update animation timer
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
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Header with filter and search
                Constraint::Min(5),     // Episodes list
                Constraint::Length(3),  // Footer with controls
            ])
            .split(area);

        // Header
        self.render_header(frame, chunks[0]);
        
        // Episodes list
        self.render_episodes_list(frame, chunks[1]);
        
        // Footer
        self.render_footer(frame, chunks[2]);
        
        // Loading overlay
        if self.loading {
            self.render_loading_overlay(frame, area);
        }
    }

    fn render_header(&mut self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let header_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Filter info
        let filter_text = format!(
            "Filter: {} ({} episodes)", 
            self.current_filter.title(), 
            self.filtered_episodes.len()
        );
        
        let filter = Paragraph::new(filter_text)
            .style(Style::default().fg(theme_colors.accent))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("📱 Feed")
                    .border_style(Style::default().fg(theme_colors.border))
                    .title_style(Style::default().fg(theme_colors.accent))
            );
        frame.render_widget(filter, header_chunks[0]);

        // Search box
        let search_text = if self.search_mode {
            format!("Search: {}|", self.search_query)
        } else if !self.search_query.is_empty() {
            format!("Search: {}", self.search_query)
        } else {
            "Press / to search".to_string()
        };
        
        let search_style = if self.search_mode {
            Style::default().fg(theme_colors.warning).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme_colors.text_secondary)
        };

        let search = Paragraph::new(search_text)
            .style(search_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("🔍 Search")
                    .border_style(Style::default().fg(theme_colors.border))
                    .title_style(Style::default().fg(theme_colors.accent))
            );
        frame.render_widget(search, header_chunks[1]);
    }

    fn render_episodes_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.filtered_episodes.is_empty() {
            let theme_colors = self.theme_manager.get_colors();
            let empty_msg = if self.episodes.is_empty() {
                "No episodes found. Press 'r' to refresh."
            } else if !self.search_query.is_empty() {
                "No episodes match your search."
            } else {
                "No episodes match the current filter."
            };

            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme_colors.text_secondary))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(theme_colors.border))
                );
            frame.render_widget(empty, area);
            return;
        }

        // Pre-calculate scrolled titles to avoid borrowing issues
        let mut scrolled_titles = Vec::new();
        let available_width = area.width as usize;
        
        // First collect the data we need to avoid borrowing conflicts
        let episode_data: Vec<(usize, String, String, Option<i64>)> = self.filtered_episodes
            .iter()
            .enumerate()
            .map(|(index, episode)| {
                let podcast_name = episode.podcast_name.as_deref().unwrap_or("Unknown Podcast").to_string();
                let title = episode.episode_title.clone();
                let episode_id = episode.episode_id;
                (index, podcast_name, title, episode_id)
            })
            .collect();
        
        // Now we can safely call mutable methods
        for (index, podcast_name, title, episode_id) in episode_data {
            // Calculate available width for title (approximate)
            let podcast_width = podcast_name.chars().count();
            let status_width = 15; // Approximate width for status indicators
            let other_chars = 10; // " • " and padding
            let title_max_width = available_width.saturating_sub(podcast_width + status_width + other_chars).max(20);
            
            // Use scrolling for long titles
            let scroll_key = format!("episode_{}_{}", index, episode_id.unwrap_or(0));
            let displayed_title = self.get_scrolled_text(scroll_key, &title, title_max_width);
            scrolled_titles.push(displayed_title);
        }

        // Get theme colors after mutable operations
        let theme_colors = self.theme_manager.get_colors();
        
        let items: Vec<ListItem> = self.filtered_episodes
            .iter()
            .enumerate()
            .map(|(index, episode)| {
                let podcast_name = episode.podcast_name.as_deref().unwrap_or("Unknown Podcast");
                let duration = format_duration(episode.episode_duration as i64);
                let listen_progress = episode.listen_duration.unwrap_or(0) as f64 / episode.episode_duration as f64;
                
                // Get pre-calculated scrolled title
                let displayed_title = scrolled_titles.get(index).cloned().unwrap_or_else(|| episode.episode_title.clone());
                
                // Status indicators
                let mut indicators = Vec::new();
                if episode.completed.unwrap_or(false) {
                    indicators.push("✅");
                } else if episode.listen_duration.unwrap_or(0) > 0 {
                    indicators.push("▶️");
                }
                if episode.saved.unwrap_or(false) {
                    indicators.push("⭐");
                }
                if episode.queued.unwrap_or(false) {
                    indicators.push("📝");
                }
                if episode.downloaded.unwrap_or(false) {
                    indicators.push("📥");
                }

                let status = if indicators.is_empty() {
                    String::new()
                } else {
                    format!(" {}", indicators.join(" "))
                };

                let progress_bar = if listen_progress > 0.0 && listen_progress < 1.0 {
                    let progress_width = 20;
                    let filled = (listen_progress * progress_width as f64) as usize;
                    let bar = "█".repeat(filled) + &"░".repeat(progress_width - filled);
                    format!(" [{}]", bar)
                } else {
                    String::new()
                };

                let line1 = Line::from(vec![
                    Span::styled(podcast_name, Style::default().fg(theme_colors.accent)),
                    Span::raw(" • "),
                    Span::styled(displayed_title, Style::default().fg(theme_colors.text).add_modifier(Modifier::BOLD)),
                    Span::styled(status, Style::default().fg(theme_colors.success)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(duration, Style::default().fg(theme_colors.text_secondary)),
                    Span::styled(progress_bar, Style::default().fg(theme_colors.primary)),
                ]);

                ListItem::new(Text::from(vec![line1, line2]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Feed")
                    .border_style(Style::default().fg(theme_colors.border))
                    .title_style(Style::default().fg(theme_colors.accent))
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
        let controls = if self.search_mode {
            vec![
                ("Esc", "Cancel search"),
                ("Enter", "Apply search"),
                ("Backspace", "Delete char"),
            ]
        } else {
            // Get dynamic controls based on selected episode status
            let mut base_controls = vec![
                ("↑↓/jk", "Navigate"),
                ("Enter", "Play"),
                ("f", "Filter"),
                ("/", "Search"),
            ];

            // Add dynamic controls based on current selection
            if let Some(selected) = self.list_state.selected() {
                if let Some(episode) = self.filtered_episodes.get(selected) {
                    // Save/Unsave
                    if episode.saved.unwrap_or(false) {
                        base_controls.push(("s", "Unsave"));
                    } else {
                        base_controls.push(("s", "Save"));
                    }
                    
                    // Queue/Unqueue
                    if episode.queued.unwrap_or(false) {
                        base_controls.push(("a", "Unqueue"));
                    } else {
                        base_controls.push(("a", "Queue"));
                    }
                    
                    // Download/Delete Download
                    if episode.downloaded.unwrap_or(false) {
                        base_controls.push(("d", "Delete download"));
                    } else {
                        base_controls.push(("d", "Download"));
                    }
                }
            } else {
                // Default controls when no episode selected
                base_controls.push(("s", "Save"));
                base_controls.push(("a", "Queue"));
                base_controls.push(("d", "Download"));
            }
            
            base_controls.push(("r", "Refresh"));
            base_controls
        };

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

        // Show error message if present
        if let Some(error) = &self.error_message {
            let error_area = Rect {
                x: area.x + 2,
                y: area.y + 1,
                width: area.width.saturating_sub(4),
                height: 1,
            };
            
            let error_msg = Paragraph::new(format!("❌ {}", error))
                .style(Style::default().fg(theme_colors.error).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center);
            frame.render_widget(error_msg, error_area);
        }
    }

    fn render_loading_overlay(&self, frame: &mut Frame, area: Rect) {
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
                Constraint::Percentage(30),
                Constraint::Percentage(40),
                Constraint::Percentage(30),
            ])
            .split(popup_area)[1];

        frame.render_widget(Clear, popup_area);

        let loading = Paragraph::new("🔄 Loading episodes...")
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