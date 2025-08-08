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
use std::time::Instant;

use crate::api::{PinepodsClient, DownloadItem};
use crate::audio::AudioPlayer;
use crate::settings::SettingsManager;

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
    
    // Audio player
    audio_player: Option<AudioPlayer>,
    
    // Settings
    settings_manager: SettingsManager,
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
            audio_player: None,
            settings_manager,
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

    pub async fn update(&mut self) -> Result<()> {
        self.last_update = Instant::now();
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let main_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40),  // Podcasts list
                Constraint::Percentage(60),  // Downloads list
            ])
            .split(area);

        // Render podcasts list
        self.render_podcasts_list(frame, main_layout[0]);
        
        // Render downloads list
        self.render_downloads_list(frame, main_layout[1]);
    }

    fn render_podcasts_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.podcasts_with_downloads.is_empty() && !self.loading {
            let empty_msg = "No downloads found. Press 'r' to refresh.";
            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Gray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title("📥 Podcasts with Downloads")
                        .border_style(if self.focused_panel == FocusPanel::Podcasts {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Gray)
                        })
                );
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self.podcasts_with_downloads
            .iter()
            .map(|podcast| {
                let download_count = podcast.download_count;
                
                let line1 = Line::from(vec![
                    Span::styled(&podcast.podcast_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(format!("{} downloads", download_count), Style::default().fg(Color::Cyan)),
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
                    .border_style(if self.focused_panel == FocusPanel::Podcasts {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Gray)
                    })
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
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
            let empty_msg = if self.podcasts_with_downloads.is_empty() {
                "Select a podcast to view downloads"
            } else {
                "No downloads found for this podcast"
            };

            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Gray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(title)
                        .border_style(if self.focused_panel == FocusPanel::Downloads {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Gray)
                        })
                );
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self.selected_podcast_downloads
            .iter()
            .map(|download| {
                let duration = format_duration(download.episode_duration);
                let pub_date = format_pub_date(&download.episode_pub_date);
                
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
                    Span::styled(&download.episode_title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(status, Style::default().fg(Color::Green)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(pub_date, Style::default().fg(Color::Yellow)),
                    Span::styled(format!(" • {}", duration), Style::default().fg(Color::Gray)),
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
                    .border_style(if self.focused_panel == FocusPanel::Downloads {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Gray)
                    })
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
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
            
            let error_msg = Paragraph::new(format!("❌ {}", error))
                .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center);
            frame.render_widget(error_msg, error_area);
        }
    }

    fn render_loading_overlay(&self, frame: &mut Frame, area: Rect, message: &str) {
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
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan))
                    .style(Style::default().bg(Color::Black))
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