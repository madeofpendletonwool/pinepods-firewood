use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use std::time::Instant;

use crate::api::{PinepodsClient, Episode};
use crate::audio::AudioPlayer;
use crate::theme::ThemeManager;

pub struct SavedPage {
    client: PinepodsClient,
    
    // Data
    saved_episodes: Vec<Episode>,
    
    // UI State
    list_state: ListState,
    loading: bool,
    error_message: Option<String>,
    
    // Animation
    last_update: Instant,
    
    // Audio player
    audio_player: Option<AudioPlayer>,
    
    // Theme management
    theme_manager: ThemeManager,
}

impl SavedPage {
    pub fn new(client: PinepodsClient) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        Self {
            client,
            saved_episodes: Vec::new(),
            list_state,
            loading: false,
            error_message: None,
            last_update: Instant::now(),
            audio_player: None,
            theme_manager: ThemeManager::new(),
        }
    }

    pub fn set_audio_player(&mut self, audio_player: AudioPlayer) {
        self.audio_player = Some(audio_player);
    }

    pub async fn initialize(&mut self) -> Result<()> {
        self.load_saved_episodes().await
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.load_saved_episodes().await
    }

    async fn load_saved_episodes(&mut self) -> Result<()> {
        self.loading = true;
        self.error_message = None;

        match self.client.get_saved_episodes().await {
            Ok(episodes) => {
                log::info!("Loaded {} saved episodes", episodes.len());
                self.saved_episodes = episodes;
                self.loading = false;
                
                // Auto-select first episode if none selected
                if !self.saved_episodes.is_empty() && self.list_state.selected().is_none() {
                    self.list_state.select(Some(0));
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load saved episodes: {}", e));
                self.loading = false;
                return Err(e);
            }
        }
        
        Ok(())
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(selected) = self.list_state.selected() {
                    if selected < self.saved_episodes.len().saturating_sub(1) {
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
            KeyCode::Char('r') => {
                self.refresh().await?;
            }
            KeyCode::Char('u') => {
                // Unsave selected episode
                self.unsave_selected_episode().await?;
            }
            _ => {}
        }
        
        Ok(())
    }

    async fn play_selected_episode(&mut self) -> Result<()> {
        if let Some(selected) = self.list_state.selected() {
            if let Some(episode) = self.saved_episodes.get(selected) {
                log::info!("Playing saved episode: {}", episode.episode_title);
                
                if let Some(ref mut audio_player) = self.audio_player {
                    audio_player.play_episode(episode.clone())?;
                    self.error_message = Some("🎵 Episode started! Switch to Player tab (4) to control playback".to_string());
                } else {
                    log::warn!("No audio player available");
                    self.error_message = Some("Audio player not available".to_string());
                }
            }
        }
        Ok(())
    }

    async fn unsave_selected_episode(&mut self) -> Result<()> {
        if let Some(selected) = self.list_state.selected() {
            if let Some(episode) = self.saved_episodes.get(selected) {
                if let Some(episode_id) = episode.episode_id {
                    log::info!("Removing episode from saved: {}", episode.episode_title);
                    
                    let is_youtube = episode.is_youtube.unwrap_or(false);
                    match self.client.unsave_episode(episode_id, is_youtube).await {
                        Ok(_) => {
                            // Remove from local list
                            self.saved_episodes.remove(selected);
                            
                            // Adjust selection
                            if self.saved_episodes.is_empty() {
                                self.list_state.select(None);
                            } else if selected >= self.saved_episodes.len() {
                                self.list_state.select(Some(self.saved_episodes.len() - 1));
                            }
                            
                            self.error_message = Some("✅ Episode removed from saved".to_string());
                        }
                        Err(e) => {
                            self.error_message = Some(format!("Failed to remove episode: {}", e));
                        }
                    }
                } else {
                    self.error_message = Some("Cannot remove episode: missing ID".to_string());
                }
            }
        }
        Ok(())
    }

    pub async fn update(&mut self) -> Result<()> {
        self.last_update = Instant::now();
        Ok(())
    }
    
    // Method to update theme from external source (like server sync)
    pub fn update_theme(&mut self, theme_name: &str) {
        self.theme_manager.set_theme(theme_name);
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),     // Episodes list
                Constraint::Length(3),  // Footer with controls
            ])
            .split(area);

        // Render episodes list
        self.render_episodes_list(frame, chunks[0]);
        
        // Render footer
        self.render_footer(frame, chunks[1]);
        
        // Loading overlay
        if self.loading {
            self.render_loading_overlay(frame, area, "Loading saved episodes...");
        }
    }

    fn render_episodes_list(&mut self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        if self.saved_episodes.is_empty() && !self.loading {
            let empty_msg = "No saved episodes found.\nPress 'r' to refresh.";
            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme_colors.text_secondary))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title("⭐ Saved Episodes")
                        .border_style(Style::default().fg(theme_colors.warning))
                        .title_style(Style::default().fg(theme_colors.warning))
                );
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self.saved_episodes
            .iter()
            .map(|episode| {
                let duration = format_duration(episode.episode_duration);
                let pub_date = format_pub_date(&episode.episode_pub_date);
                let podcast_name = episode.podcast_name.as_deref().unwrap_or("Unknown Podcast");
                
                // Status indicators
                let mut indicators = Vec::new();
                if episode.completed.unwrap_or(false) {
                    indicators.push("✅");
                } else if episode.listen_duration.unwrap_or(0) > 0 {
                    indicators.push("▶️");
                }
                indicators.push("⭐"); // Always show saved indicator
                if episode.queued.unwrap_or(false) {
                    indicators.push("📝");
                }
                if episode.downloaded.unwrap_or(false) {
                    indicators.push("📥");
                }

                let status = format!(" {}", indicators.join(" "));

                let line1 = Line::from(vec![
                    Span::styled(&episode.episode_title, Style::default().fg(theme_colors.text).add_modifier(Modifier::BOLD)),
                    Span::styled(status, Style::default().fg(theme_colors.success)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(podcast_name, Style::default().fg(theme_colors.accent)),
                    Span::styled(format!(" • {}", pub_date), Style::default().fg(theme_colors.warning)),
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
                    .title("⭐ Saved Episodes")
                    .border_style(Style::default().fg(theme_colors.warning))
                    .title_style(Style::default().fg(theme_colors.warning))
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
        let controls = vec![
            ("↑↓/jk", "Navigate"),
            ("Enter", "Play"),
            ("u", "Unsave"),
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

        // Show error message if present
        if let Some(error) = &self.error_message {
            let error_area = Rect {
                x: area.x + 2,
                y: area.y + 1,
                width: area.width.saturating_sub(4),
                height: 1,
            };
            
            let color = if error.starts_with("✅") || error.starts_with("🎵") {
                theme_colors.success
            } else {
                theme_colors.error
            };
            
            let error_msg = Paragraph::new(error.as_str())
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