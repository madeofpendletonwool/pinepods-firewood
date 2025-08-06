use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::time::Instant;

use crate::api::{PinepodsClient, Podcast, PodcastEpisode, Episode};
use crate::audio::AudioPlayer;

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
    
    // Audio player
    audio_player: Option<AudioPlayer>,
}

impl PodcastsPage {
    pub fn new(client: PinepodsClient) -> Self {
        let mut podcast_list_state = ListState::default();
        podcast_list_state.select(Some(0));
        
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
            audio_player: None,
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
                if !self.podcasts.is_empty() && self.podcast_list_state.selected().is_none() {
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
        Ok(())
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
            let empty_msg = "No podcasts found. Press 'r' to refresh.";
            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Gray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title("🎙️ Podcasts")
                        .border_style(if self.focused_panel == FocusPanel::Podcasts {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Gray)
                        })
                );
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self.podcasts
            .iter()
            .map(|podcast| {
                let episode_count = podcast.episodecount;
                let author = &podcast.author;
                
                let line1 = Line::from(vec![
                    Span::styled(&podcast.podcastname, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(format!("by {}", author), Style::default().fg(Color::Cyan)),
                    Span::styled(format!(" • {} episodes", episode_count), Style::default().fg(Color::Gray)),
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
            let empty_msg = if self.podcasts.is_empty() {
                "Select a podcast to view episodes"
            } else {
                "No episodes found for this podcast"
            };

            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Gray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(title)
                        .border_style(if self.focused_panel == FocusPanel::Episodes {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Gray)
                        })
                );
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self.selected_podcast_episodes
            .iter()
            .map(|episode| {
                let duration = format_duration(episode.episode_duration);
                let pub_date = format_pub_date(&episode.episode_pub_date);
                
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
                    Span::styled(&episode.episode_title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
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
                    .border_style(if self.focused_panel == FocusPanel::Episodes {
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