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

use crate::api::{PinepodsClient, SearchResultItem};
use crate::audio::AudioPlayer;

#[derive(Debug, Clone, Copy, PartialEq)]
enum InputMode {
    Normal,
    Editing,
}

pub struct SearchPage {
    client: PinepodsClient,
    
    // Search state
    search_input: String,
    input_mode: InputMode,
    search_results: Vec<SearchResultItem>,
    
    // UI State
    list_state: ListState,
    loading: bool,
    error_message: Option<String>,
    success_message: Option<String>,
    
    // Animation
    last_update: Instant,
    
    // Audio player
    audio_player: Option<AudioPlayer>,
}

impl SearchPage {
    pub fn new(client: PinepodsClient) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        Self {
            client,
            search_input: String::new(),
            input_mode: InputMode::Normal,
            search_results: Vec::new(),
            list_state,
            loading: false,
            error_message: None,
            success_message: None,
            last_update: Instant::now(),
            audio_player: None,
        }
    }

    pub fn set_audio_player(&mut self, audio_player: AudioPlayer) {
        self.audio_player = Some(audio_player);
    }

    pub async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }

    pub async fn refresh(&mut self) -> Result<()> {
        if !self.search_input.is_empty() {
            self.perform_search().await?;
        }
        Ok(())
    }

    async fn perform_search(&mut self) -> Result<()> {
        if self.search_input.trim().is_empty() {
            self.search_results.clear();
            self.list_state.select(None);
            return Ok(());
        }

        self.loading = true;
        self.error_message = None;
        self.success_message = None;

        match self.client.search_data(&self.search_input).await {
            Ok(results) => {
                log::info!("Search found {} results for '{}'", results.len(), self.search_input);
                self.search_results = results;
                self.loading = false;
                
                if !self.search_results.is_empty() {
                    self.list_state.select(Some(0));
                    self.success_message = Some(format!("Found {} results", self.search_results.len()));
                } else {
                    self.list_state.select(None);
                    self.success_message = Some("No results found".to_string());
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Search failed: {}", e));
                self.loading = false;
                return Err(e);
            }
        }
        
        Ok(())
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        // Clear messages on input (except when typing)
        if !matches!(self.input_mode, InputMode::Editing) {
            self.error_message = None;
            self.success_message = None;
        }

        match self.input_mode {
            InputMode::Normal => {
                match key.code {
                    KeyCode::Char('/') | KeyCode::Char('s') => {
                        self.input_mode = InputMode::Editing;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Some(selected) = self.list_state.selected() {
                            if selected < self.search_results.len().saturating_sub(1) {
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
                        self.play_selected_result().await?;
                    }
                    KeyCode::Char('r') => {
                        self.refresh().await?;
                    }
                    KeyCode::Char('c') => {
                        // Clear search
                        self.search_input.clear();
                        self.search_results.clear();
                        self.list_state.select(None);
                        self.success_message = Some("Search cleared".to_string());
                    }
                    _ => {}
                }
            }
            InputMode::Editing => {
                match key.code {
                    KeyCode::Enter => {
                        self.input_mode = InputMode::Normal;
                        self.perform_search().await?;
                    }
                    KeyCode::Char(c) => {
                        self.search_input.push(c);
                    }
                    KeyCode::Backspace => {
                        self.search_input.pop();
                    }
                    KeyCode::Esc => {
                        self.input_mode = InputMode::Normal;
                    }
                    _ => {}
                }
            }
        }
        
        Ok(())
    }

    async fn play_selected_result(&mut self) -> Result<()> {
        if let Some(selected) = self.list_state.selected() {
            if let Some(result) = self.search_results.get(selected) {
                log::info!("Playing search result: {}", result.episode_title);
                
                if let Some(ref mut audio_player) = self.audio_player {
                    // Convert SearchResultItem to Episode for the audio player
                    let episode = result.clone().into();
                    audio_player.play_episode(episode)?;
                    self.success_message = Some("🎵 Episode started! Switch to Player tab (4) to control playback".to_string());
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
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Search input
                Constraint::Min(5),    // Results
                Constraint::Length(3), // Status/help
            ])
            .split(area);

        // Search input
        self.render_search_input(frame, main_layout[0]);
        
        // Results list
        self.render_results_list(frame, main_layout[1]);
        
        // Status/help
        self.render_status(frame, main_layout[2]);
    }

    fn render_search_input(&self, frame: &mut Frame, area: Rect) {
        let input_style = match self.input_mode {
            InputMode::Normal => Style::default().fg(Color::Gray),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        };

        let border_style = match self.input_mode {
            InputMode::Normal => Style::default().fg(Color::Gray),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        };

        let title = match self.input_mode {
            InputMode::Normal => "🔍 Search (press '/' or 's' to edit)",
            InputMode::Editing => "🔍 Search (press Enter to search, Esc to cancel)",
        };

        let display_text = if self.search_input.is_empty() && self.input_mode == InputMode::Normal {
            "Enter search term..."
        } else {
            &self.search_input
        };

        let input = Paragraph::new(display_text)
            .style(input_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(title)
                    .border_style(border_style)
            );

        frame.render_widget(input, area);

        // Show cursor when editing
        if self.input_mode == InputMode::Editing {
            frame.set_cursor(
                area.x + self.search_input.len() as u16 + 1,
                area.y + 1,
            );
        }
    }

    fn render_results_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.search_results.is_empty() && !self.loading {
            let empty_msg = if self.search_input.is_empty() {
                "Enter a search term to find episodes and podcasts"
            } else {
                "No results found for your search"
            };

            let empty = Paragraph::new(empty_msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Gray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title("Search Results")
                        .border_style(Style::default().fg(Color::Cyan))
                );
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self.search_results
            .iter()
            .map(|result| {
                let duration = format_duration(result.episode_duration);
                let pub_date = format_pub_date(&result.episode_pub_date);
                
                // Status indicators
                let mut indicators = Vec::new();
                if result.completed {
                    indicators.push("✅");
                } else if result.listen_duration > 0 {
                    indicators.push("▶️");
                }
                if result.saved {
                    indicators.push("⭐");
                }
                if result.queued {
                    indicators.push("📝");
                }
                if result.downloaded {
                    indicators.push("📥");
                }

                let status = if indicators.is_empty() {
                    String::new()
                } else {
                    format!(" {}", indicators.join(" "))
                };

                let line1 = Line::from(vec![
                    Span::styled(&result.episode_title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(status, Style::default().fg(Color::Green)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(&result.podcast_name, Style::default().fg(Color::Cyan)),
                    Span::styled(format!(" • {}", pub_date), Style::default().fg(Color::Yellow)),
                    Span::styled(format!(" • {}", duration), Style::default().fg(Color::Gray)),
                ]);

                // Third line with description (truncated)
                let description = truncate_text(&result.episode_description, 80);
                let line3 = Line::from(vec![
                    Span::styled(description, Style::default().fg(Color::DarkGray)),
                ]);

                ListItem::new(Text::from(vec![line1, line2, line3]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(format!("🔍 Search Results ({})", self.search_results.len()))
                    .border_style(Style::default().fg(Color::Cyan))
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_stateful_widget(list, area, &mut self.list_state);

        // Show loading overlay
        if self.loading {
            self.render_loading_overlay(frame, area, "Searching...");
        }
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(60), // Help text
                Constraint::Percentage(40), // Status message
            ])
            .split(area);

        // Help text
        let help_text = match self.input_mode {
            InputMode::Normal => "Enter: Play • /,s: Search • r: Refresh • c: Clear",
            InputMode::Editing => "Enter: Search • Esc: Cancel • Backspace: Delete",
        };

        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Left)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Controls")
                    .border_style(Style::default().fg(Color::DarkGray))
            );
        frame.render_widget(help, layout[0]);

        // Status message
        let (message, color) = if let Some(ref error) = self.error_message {
            (format!("❌ {}", error), Color::Red)
        } else if let Some(ref success) = self.success_message {
            (format!("✅ {}", success), Color::Green)
        } else {
            ("Ready to search".to_string(), Color::Gray)
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

fn truncate_text(text: &str, max_len: usize) -> String {
    // Remove HTML tags and truncate
    let clean_text = text.replace(['<', '>'], "");
    if clean_text.len() <= max_len {
        clean_text
    } else {
        format!("{}...", &clean_text[..max_len.saturating_sub(3)])
    }
}