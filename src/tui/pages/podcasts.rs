use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::time::{Duration, Instant};
use std::collections::HashMap;

use crate::api::{PinepodsClient, Podcast, PodcastEpisode, Episode};
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
    Episodes,
}

pub struct PodcastsPage {
    client: PinepodsClient,
    
    // Data
    podcasts: Vec<Podcast>,
    selected_podcast_episodes: Vec<PodcastEpisode>,
    
    // UI State
    podcast_list_state: ListState,
    episode_list_state: ListState,
    focused_panel: FocusPanel,
    loading: bool,
    loading_episodes: bool,
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

impl PodcastsPage {
    pub fn new(client: PinepodsClient) -> Self {
        let mut podcast_list_state = ListState::default();
        podcast_list_state.select(Some(0));
        
        let settings_manager = SettingsManager::new().unwrap_or_else(|e| {
            log::error!("Failed to create settings manager: {}", e);
            SettingsManager::new().expect("Failed to create fallback settings manager")
        });
        
        Self {
            client,
            podcasts: Vec::new(),
            selected_podcast_episodes: Vec::new(),
            podcast_list_state,
            episode_list_state: ListState::default(),
            focused_panel: FocusPanel::Podcasts,
            loading: false,
            loading_episodes: false,
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
        self.load_podcasts().await
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.load_podcasts().await
    }

    async fn load_podcasts(&mut self) -> Result<()> {
        self.loading = true;
        self.error_message = None;

        match self.client.get_podcasts().await {
            Ok(podcasts) => {
                log::info!("Loaded {} podcasts", podcasts.len());
                self.podcasts = podcasts;
                self.loading = false;
                
                // Auto-select first podcast and load its episodes
                if !self.podcasts.is_empty() {
                    self.podcast_list_state.select(Some(0));
                    if let Some(podcast) = self.podcasts.get(0) {
                        self.load_podcast_episodes(podcast.podcastid).await?;
                    }
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load podcasts: {}", e));
                self.loading = false;
                return Err(e);
            }
        }
        
        Ok(())
    }

    async fn load_podcast_episodes(&mut self, podcast_id: i64) -> Result<()> {
        self.loading_episodes = true;
        self.selected_podcast_id = Some(podcast_id);
        self.episode_list_state = ListState::default();
        self.episode_list_state.select(Some(0));

        match self.client.get_podcast_episodes(podcast_id).await {
            Ok(episodes) => {
                log::info!("Loaded {} episodes for podcast {}", episodes.len(), podcast_id);
                self.selected_podcast_episodes = episodes;
                self.loading_episodes = false;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load episodes: {}", e));
                self.loading_episodes = false;
                return Err(e);
            }
        }
        
        Ok(())
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                self.focused_panel = FocusPanel::Podcasts;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if !self.selected_podcast_episodes.is_empty() {
                    self.focused_panel = FocusPanel::Episodes;
                }
            }
            KeyCode::Tab => {
                self.focused_panel = match self.focused_panel {
                    FocusPanel::Podcasts => {
                        if !self.selected_podcast_episodes.is_empty() {
                            FocusPanel::Episodes
                        } else {
                            FocusPanel::Podcasts
                        }
                    }
                    FocusPanel::Episodes => FocusPanel::Podcasts,
                };
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match self.focused_panel {
                    FocusPanel::Podcasts => {
                        if let Some(selected) = self.podcast_list_state.selected() {
                            if selected < self.podcasts.len().saturating_sub(1) {
                                self.podcast_list_state.select(Some(selected + 1));
                                
                                // Auto-load episodes if setting is enabled
                                if self.settings_manager.auto_load_episodes() {
                                    if let Some(podcast) = self.podcasts.get(selected + 1) {
                                        let _ = self.load_podcast_episodes(podcast.podcastid).await;
                                    }
                                }
                            }
                        }
                    }
                    FocusPanel::Episodes => {
                        if let Some(selected) = self.episode_list_state.selected() {
                            if selected < self.selected_podcast_episodes.len().saturating_sub(1) {
                                self.episode_list_state.select(Some(selected + 1));
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
                                
                                // Auto-load episodes if setting is enabled
                                if self.settings_manager.auto_load_episodes() {
                                    if let Some(podcast) = self.podcasts.get(selected - 1) {
                                        let _ = self.load_podcast_episodes(podcast.podcastid).await;
                                    }
                                }
                            }
                        }
                    }
                    FocusPanel::Episodes => {
                        if let Some(selected) = self.episode_list_state.selected() {
                            if selected > 0 {
                                self.episode_list_state.select(Some(selected - 1));
                            }
                        }
                    }
                }
            }
            KeyCode::Enter => {
                match self.focused_panel {
                    FocusPanel::Podcasts => {
                        if let Some(selected) = self.podcast_list_state.selected() {
                            if let Some(podcast) = self.podcasts.get(selected) {
                                self.load_podcast_episodes(podcast.podcastid).await?;
                                self.focused_panel = FocusPanel::Episodes;
                            }
                        }
                    }
                    FocusPanel::Episodes => {
                        self.play_selected_episode().await?;
                    }
                }
            }
            KeyCode::Char('r') => {
                self.refresh().await?;
            }
            _ => {}
        }
        
        Ok(())
    }

    async fn play_selected_episode(&mut self) -> Result<()> {
        if let Some(selected) = self.episode_list_state.selected() {
            if let Some(episode) = self.selected_podcast_episodes.get(selected) {
                log::info!("Playing episode: {}", episode.episode_title);
                
                if let Some(ref mut audio_player) = self.audio_player {
                    // Convert PodcastEpisode to Episode for the audio player
                    let episode_for_player: Episode = episode.clone().into();
                    audio_player.play_episode(episode_for_player)?;
                    self.error_message = Some("🎵 Episode started! Switch to Player tab (4) to control playback".to_string());
                } else {
                    log::warn!("No audio player available");
                    self.error_message = Some("Audio player not available".to_string());
                }
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
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40),  // Podcasts list
                Constraint::Percentage(60),  // Episodes list
            ])
            .split(area);

        // Render podcasts list
        self.render_podcasts_list(frame, main_layout[0]);
        
        // Render episodes list
        self.render_episodes_list(frame, main_layout[1]);
    }

    fn render_podcasts_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.podcasts.is_empty() && !self.loading {
            let theme_colors = self.theme_manager.get_colors();
            let empty_msg = "No podcasts found. Press 'r' to refresh.";
            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme_colors.text_secondary))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title("🎙️ Podcasts")
                        .border_style(if self.focused_panel == FocusPanel::Podcasts {
                            Style::default().fg(theme_colors.primary)
                        } else {
                            Style::default().fg(theme_colors.border)
                        })
                );
            frame.render_widget(empty, area);
            return;
        }

        // Pre-calculate scrolled titles to avoid borrowing issues
        let mut scrolled_titles = Vec::new();
        let available_width = area.width as usize;
        
        // First collect the data we need to avoid borrowing conflicts
        let podcast_data: Vec<(usize, String, String, i64)> = self.podcasts
            .iter()
            .enumerate()
            .map(|(index, podcast)| {
                let name = podcast.podcastname.clone();
                let author = podcast.author.clone();
                let podcast_id = podcast.podcastid;
                (index, name, author, podcast_id)
            })
            .collect();
        
        // Now we can safely call mutable methods
        for (index, name, author, podcast_id) in podcast_data {
            // Use much more of the available width for podcast name
            // The second line has author info, so the first line can use most of the width
            let name_max_width = available_width.saturating_sub(8).max(30); // Just leave room for borders/padding
            
            // Use scrolling for long podcast names
            let scroll_key = format!("podcast_{}_{}", index, podcast_id);
            let displayed_name = self.get_scrolled_text(scroll_key, &name, name_max_width);
            scrolled_titles.push(displayed_name);
        }

        // Get theme colors after mutable operations
        let theme_colors = self.theme_manager.get_colors();

        let items: Vec<ListItem> = self.podcasts
            .iter()
            .enumerate()
            .map(|(index, podcast)| {
                let episode_count = podcast.episodecount;
                let author = &podcast.author;
                
                // Get pre-calculated scrolled title
                let displayed_name = scrolled_titles.get(index).cloned().unwrap_or_else(|| podcast.podcastname.clone());
                
                let line1 = Line::from(vec![
                    Span::styled(displayed_name, Style::default().fg(theme_colors.text).add_modifier(Modifier::BOLD)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(format!("by {}", author), Style::default().fg(theme_colors.accent)),
                    Span::styled(format!(" • {} episodes", episode_count), Style::default().fg(theme_colors.text_secondary)),
                ]);

                ListItem::new(Text::from(vec![line1, line2]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("🎙️ Podcasts")
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
            self.render_loading_overlay(frame, area, "Loading podcasts...");
        }
    }

    fn render_episodes_list(&mut self, frame: &mut Frame, area: Rect) {
        let title = if let Some(selected) = self.podcast_list_state.selected() {
            if let Some(podcast) = self.podcasts.get(selected) {
                format!("📻 Episodes - {}", podcast.podcastname)
            } else {
                "📻 Episodes".to_string()
            }
        } else {
            "📻 Episodes".to_string()
        };

        if self.selected_podcast_episodes.is_empty() && !self.loading_episodes {
            let theme_colors = self.theme_manager.get_colors();
            let empty_msg = if self.podcasts.is_empty() {
                "Select a podcast to view episodes"
            } else {
                "No episodes found for this podcast"
            };

            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme_colors.text_secondary))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(title)
                        .border_style(if self.focused_panel == FocusPanel::Episodes {
                            Style::default().fg(theme_colors.primary)
                        } else {
                            Style::default().fg(theme_colors.border)
                        })
                );
            frame.render_widget(empty, area);
            return;
        }

        // Pre-calculate scrolled titles to avoid borrowing issues
        let mut scrolled_titles = Vec::new();
        let available_width = area.width as usize;
        
        // First collect the data we need to avoid borrowing conflicts
        let episode_data: Vec<(usize, String, Option<i64>)> = self.selected_podcast_episodes
            .iter()
            .enumerate()
            .map(|(index, episode)| {
                let title = episode.episode_title.clone();
                let episode_id = episode.episode_id;
                (index, title, episode_id)
            })
            .collect();
        
        // Now we can safely call mutable methods
        for (index, title, episode_id) in episode_data {
            // Use much more of the available width for episode title  
            // Status indicators are on the same line, so we need to be a bit more conservative
            // but still use most of the width
            let status_width = 20; // Conservative estimate for status indicators
            let title_max_width = available_width.saturating_sub(status_width + 6).max(40); // Leave room for status + padding
            
            // Use scrolling for long episode titles
            let scroll_key = format!("episode_{}_{}", index, episode_id.unwrap_or(0));
            let displayed_title = self.get_scrolled_text(scroll_key, &title, title_max_width);
            scrolled_titles.push(displayed_title);
        }

        // Get theme colors after mutable operations
        let theme_colors = self.theme_manager.get_colors();

        let items: Vec<ListItem> = self.selected_podcast_episodes
            .iter()
            .enumerate()
            .map(|(index, episode)| {
                let duration = format_duration(episode.episode_duration);
                let pub_date = format_pub_date(&episode.episode_pub_date);
                
                // Get pre-calculated scrolled title
                let displayed_title = scrolled_titles.get(index).cloned().unwrap_or_else(|| episode.episode_title.clone());
                
                // Status indicators
                let mut indicators = Vec::new();
                if episode.completed {
                    indicators.push("✅");
                } else if episode.listen_duration.unwrap_or(0) > 0 {
                    indicators.push("▶️");
                }
                if episode.saved {
                    indicators.push("⭐");
                }
                if episode.queued {
                    indicators.push("📝");
                }
                if episode.downloaded {
                    indicators.push("📥");
                }

                let status = if indicators.is_empty() {
                    String::new()
                } else {
                    format!(" {}", indicators.join(" "))
                };

                let line1 = Line::from(vec![
                    Span::styled(displayed_title, Style::default().fg(theme_colors.text).add_modifier(Modifier::BOLD)),
                    Span::styled(status, Style::default().fg(theme_colors.success)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(pub_date, Style::default().fg(theme_colors.accent)),
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
                    .border_style(if self.focused_panel == FocusPanel::Episodes {
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

        frame.render_stateful_widget(list, area, &mut self.episode_list_state);

        // Show loading overlay for episodes
        if self.loading_episodes {
            self.render_loading_overlay(frame, area, "Loading episodes...");
        }

        // Show error message if present
        if let Some(error) = &self.error_message {
            let error_area = Rect {
                x: area.x + 2,
                y: area.y + area.height - 3,
                width: area.width.saturating_sub(4),
                height: 1,
            };
            
            let error_msg = Paragraph::new(format!("❌ {}", error))
                .style(Style::default().fg(theme_colors.error).add_modifier(Modifier::BOLD))
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