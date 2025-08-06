use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, List, ListItem, Paragraph, Wrap
    },
    Frame,
};

use crate::api::{PinepodsClient, QueueItem};

pub struct QueuePage {
    client: PinepodsClient,
    
    // Data
    queue_items: Vec<QueueItem>,
    
    // UI State
    selected_item: usize,
    loading: bool,
    error_message: Option<String>,
    
    // Actions
    show_actions: bool,
    selected_action: usize,
}

impl QueuePage {
    pub fn new(client: PinepodsClient) -> Self {
        Self {
            client,
            queue_items: Vec::new(),
            selected_item: 0,
            loading: false,
            error_message: None,
            show_actions: false,
            selected_action: 0,
        }
    }

    pub async fn initialize(&mut self) -> Result<()> {
        self.refresh().await
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.loading = true;
        self.error_message = None;

        match self.client.get_queue().await {
            Ok(items) => {
                self.queue_items = items;
                self.queue_items.sort_by_key(|item| item.queue_position);
                
                // Reset selection if needed
                if self.selected_item >= self.queue_items.len() {
                    self.selected_item = 0;
                }
                
                self.loading = false;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load queue: {}", e));
                self.loading = false;
                return Err(e);
            }
        }

        Ok(())
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        if self.show_actions {
            return self.handle_action_input(key).await;
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.queue_items.is_empty() {
                    self.selected_item = (self.selected_item + 1) % self.queue_items.len();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.queue_items.is_empty() {
                    if self.selected_item == 0 {
                        self.selected_item = self.queue_items.len() - 1;
                    } else {
                        self.selected_item -= 1;
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(item) = self.queue_items.get(self.selected_item) {
                    // TODO: Play selected episode
                    println!("Playing: {}", item.episode_title);
                }
            }
            KeyCode::Char(' ') => {
                self.show_actions = true;
                self.selected_action = 0;
            }
            KeyCode::Char('r') => {
                if let Some(item) = self.queue_items.get(self.selected_item) {
                    self.remove_from_queue(item.episode_id).await?;
                }
            }
            KeyCode::Char('c') => {
                self.clear_queue().await?;
            }
            KeyCode::Char('s') => {
                self.shuffle_queue().await?;
            }
            KeyCode::Char('t') => {
                if let Some(item) = self.queue_items.get(self.selected_item) {
                    self.move_to_top(item.episode_id).await?;
                }
            }
            KeyCode::Char('b') => {
                if let Some(item) = self.queue_items.get(self.selected_item) {
                    self.move_to_bottom(item.episode_id).await?;
                }
            }
            KeyCode::F(5) => {
                self.refresh().await?;
            }
            _ => {}
        }

        Ok(())
    }

    async fn handle_action_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_action = (self.selected_action + 1) % 6;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_action == 0 {
                    self.selected_action = 5;
                } else {
                    self.selected_action -= 1;
                }
            }
            KeyCode::Enter => {
                if let Some(item) = self.queue_items.get(self.selected_item) {
                    match self.selected_action {
                        0 => { // Play
                            println!("Playing: {}", item.episode_title);
                        }
                        1 => { // Remove
                            self.remove_from_queue(item.episode_id).await?;
                        }
                        2 => { // Move to top
                            self.move_to_top(item.episode_id).await?;
                        }
                        3 => { // Move to bottom
                            self.move_to_bottom(item.episode_id).await?;
                        }
                        4 => { // Save episode
                            self.save_episode(item.episode_id).await?;
                        }
                        5 => { // Download
                            self.download_episode(item.episode_id).await?;
                        }
                        _ => {}
                    }
                }
                self.show_actions = false;
            }
            KeyCode::Esc => {
                self.show_actions = false;
            }
            _ => {}
        }

        Ok(())
    }

    pub async fn update(&mut self) -> Result<()> {
        // Auto-refresh every minute
        // TODO: Implement auto-refresh logic
        Ok(())
    }

    async fn remove_from_queue(&mut self, episode_id: i64) -> Result<()> {
        match self.client.remove_from_queue(episode_id).await {
            Ok(_) => {
                self.refresh().await?;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to remove from queue: {}", e));
            }
        }
        Ok(())
    }

    async fn clear_queue(&mut self) -> Result<()> {
        match self.client.clear_queue().await {
            Ok(_) => {
                self.refresh().await?;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to clear queue: {}", e));
            }
        }
        Ok(())
    }

    async fn shuffle_queue(&mut self) -> Result<()> {
        match self.client.shuffle_queue().await {
            Ok(_) => {
                self.refresh().await?;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to shuffle queue: {}", e));
            }
        }
        Ok(())
    }

    async fn move_to_top(&mut self, episode_id: i64) -> Result<()> {
        match self.client.move_to_top_of_queue(episode_id).await {
            Ok(_) => {
                self.refresh().await?;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to move to top: {}", e));
            }
        }
        Ok(())
    }

    async fn move_to_bottom(&mut self, episode_id: i64) -> Result<()> {
        match self.client.move_to_bottom_of_queue(episode_id).await {
            Ok(_) => {
                self.refresh().await?;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to move to bottom: {}", e));
            }
        }
        Ok(())
    }

    async fn save_episode(&mut self, episode_id: i64) -> Result<()> {
        match self.client.save_episode(episode_id).await {
            Ok(_) => {
                // Show success message
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to save episode: {}", e));
            }
        }
        Ok(())
    }

    async fn download_episode(&mut self, episode_id: i64) -> Result<()> {
        match self.client.download_episode(episode_id).await {
            Ok(_) => {
                // Show success message
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to download episode: {}", e));
            }
        }
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        if self.loading {
            self.render_loading(frame, area);
            return;
        }

        if let Some(error) = &self.error_message {
            self.render_error(frame, area, error);
            return;
        }

        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),     // Queue list
                Constraint::Length(4),  // Help text
            ])
            .split(area);

        // Render queue list
        self.render_queue_list(frame, main_layout[0]);

        // Render help text
        self.render_help(frame, main_layout[1]);

        // Render actions modal if shown
        if self.show_actions {
            self.render_actions_modal(frame, area);
        }
    }

    fn render_queue_list(&self, frame: &mut Frame, area: Rect) {
        if self.queue_items.is_empty() {
            self.render_empty_queue(frame, area);
            return;
        }

        let items: Vec<ListItem> = self.queue_items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let style = if i == self.selected_item {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let duration = format_duration(item.episode_duration);
                let progress = item.listen_duration.unwrap_or(0);
                let progress_indicator = if progress > 0 {
                    let percent = (progress * 100) / item.episode_duration.max(1);
                    format!(" ({}%)", percent)
                } else {
                    String::new()
                };

                let text = format!("{}. {} - {}{} [{}]",
                                 item.queue_position,
                                 item.podcast_name,
                                 item.episode_title,
                                 progress_indicator,
                                 duration);

                ListItem::new(Text::from(text)).style(style)
            })
            .collect();

        let total_duration: i64 = self.queue_items.iter().map(|item| item.episode_duration).sum();
        let title = format!("Queue ({} episodes, {})", 
                          self.queue_items.len(),
                          format_duration(total_duration));

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(Color::Green))
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Green)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_widget(list, area);
    }

    fn render_empty_queue(&self, frame: &mut Frame, area: Rect) {
        let empty_text = vec![
            Line::from("📝 Your queue is empty"),
            Line::from(""),
            Line::from("Add episodes to your queue from:"),
            Line::from("• Episode lists"),
            Line::from("• Search results"),
            Line::from("• Podcast pages"),
            Line::from(""),
            Line::from("Press 'a' on any episode to add it to your queue"),
        ];

        let empty_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Empty Queue")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(Color::Blue));

        let empty_paragraph = Paragraph::new(empty_text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(empty_block);

        frame.render_widget(empty_paragraph, area);
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let help_text = vec![
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Play  "),
                Span::styled("Space", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Actions  "),
                Span::styled("r", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Remove  "),
                Span::styled("c", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Clear  "),
                Span::styled("s", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Shuffle"),
            ]),
            Line::from(vec![
                Span::styled("t", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Move to top  "),
                Span::styled("b", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Move to bottom  "),
                Span::styled("F5", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Refresh"),
            ]),
        ];

        let help_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Controls")
            .border_style(Style::default().fg(Color::Blue));

        let help_paragraph = Paragraph::new(help_text)
            .alignment(Alignment::Left)
            .block(help_block);

        frame.render_widget(help_paragraph, area);
    }

    fn render_actions_modal(&self, frame: &mut Frame, area: Rect) {
        let popup_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Length(10),
                Constraint::Percentage(30),
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

        frame.render_widget(ratatui::widgets::Clear, popup_area);

        let actions = [
            "▶️  Play Episode",
            "🗑️  Remove from Queue",
            "⬆️  Move to Top",
            "⬇️  Move to Bottom",
            "⭐ Save Episode",
            "📥 Download Episode",
        ];

        let items: Vec<ListItem> = actions
            .iter()
            .enumerate()
            .map(|(i, action)| {
                let style = if i == self.selected_action {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Text::from(*action)).style(style)
            })
            .collect();

        let actions_list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Actions")
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(Color::Cyan))
                    .style(Style::default().bg(Color::Black))
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Cyan)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_widget(actions_list, popup_area);
    }

    fn render_loading(&self, frame: &mut Frame, area: Rect) {
        let loading_text = Paragraph::new("🔄 Loading queue...")
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Loading")
                    .border_style(Style::default().fg(Color::Yellow))
            );

        frame.render_widget(loading_text, area);
    }

    fn render_error(&self, frame: &mut Frame, area: Rect, error: &str) {
        let error_text = Paragraph::new(format!("❌ {}\n\nPress F5 to retry", error))
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Error")
                    .border_style(Style::default().fg(Color::Red))
            );

        frame.render_widget(error_text, area);
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