use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap
    },
    Frame,
};
use std::time::{Duration, Instant};
use std::collections::HashMap;

use crate::api::{PinepodsClient, QueueItem};
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

pub struct QueuePage {
    client: PinepodsClient,
    
    // Data
    queue_items: Vec<QueueItem>,
    
    // UI State
    list_state: ListState,
    selected_item: usize,
    loading: bool,
    error_message: Option<String>,
    
    // Actions
    show_actions: bool,
    selected_action: usize,
    
    // Title scrolling animation
    scroll_states: HashMap<String, ScrollState>,
    last_scroll_update: Instant,
    
    // Theme management
    theme_manager: ThemeManager,
}

impl QueuePage {
    pub fn new(client: PinepodsClient) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        Self {
            client,
            queue_items: Vec::new(),
            list_state,
            selected_item: 0,
            loading: false,
            error_message: None,
            show_actions: false,
            selected_action: 0,
            scroll_states: HashMap::new(),
            last_scroll_update: Instant::now(),
            theme_manager: ThemeManager::new(),
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
                    self.list_state.select(Some(0));
                } else if self.queue_items.is_empty() {
                    self.list_state.select(None);
                } else {
                    self.list_state.select(Some(self.selected_item));
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
                if let Some(selected) = self.list_state.selected() {
                    if selected < self.queue_items.len().saturating_sub(1) {
                        self.list_state.select(Some(selected + 1));
                        self.selected_item = selected + 1;
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(selected) = self.list_state.selected() {
                    if selected > 0 {
                        self.list_state.select(Some(selected - 1));
                        self.selected_item = selected - 1;
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
            KeyCode::Char('u') => {
                if let Some(item) = self.queue_items.get(self.selected_item) {
                    self.move_up_one_position(item.episode_id).await?;
                }
            }
            KeyCode::Char('d') => {
                if let Some(item) = self.queue_items.get(self.selected_item) {
                    self.move_down_one_position(item.episode_id).await?;
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
                self.selected_action = (self.selected_action + 1) % 8;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_action == 0 {
                    self.selected_action = 7;
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
                        6 => { // Move up one position
                            self.move_up_one_position(item.episode_id).await?;
                        }
                        7 => { // Move down one position
                            self.move_down_one_position(item.episode_id).await?;
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
        // Update scrolling animation every 150ms
        let now = Instant::now();
        if now.duration_since(self.last_scroll_update) >= Duration::from_millis(150) {
            self.update_scroll_states(now);
            self.last_scroll_update = now;
        }
        
        // Auto-refresh every minute
        // TODO: Implement auto-refresh logic
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

    async fn remove_from_queue(&mut self, episode_id: i64) -> Result<()> {
        let is_youtube = self.queue_items.iter()
            .find(|item| item.episode_id == episode_id)
            .map(|item| item.is_youtube)
            .unwrap_or(false);
            
        match self.client.remove_from_queue(episode_id, is_youtube).await {
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

    async fn move_up_one_position(&mut self, episode_id: i64) -> Result<()> {
        // Check if we can move up (not at the top)
        if let Some(current_selected) = self.list_state.selected() {
            if current_selected > 0 {
                match self.client.move_up_one_position(episode_id).await {
                    Ok(_) => {
                        self.refresh().await?;
                        // Move cursor up to follow the episode that moved up
                        let new_position = current_selected - 1;
                        self.list_state.select(Some(new_position));
                        self.selected_item = new_position;
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to move up: {}", e));
                    }
                }
            }
        }
        Ok(())
    }

    async fn move_down_one_position(&mut self, episode_id: i64) -> Result<()> {
        // Check if we can move down (not at the bottom)
        if let Some(current_selected) = self.list_state.selected() {
            if current_selected < self.queue_items.len().saturating_sub(1) {
                match self.client.move_down_one_position(episode_id).await {
                    Ok(_) => {
                        self.refresh().await?;
                        // Move cursor down to follow the episode that moved down
                        let new_position = current_selected + 1;
                        self.list_state.select(Some(new_position));
                        self.selected_item = new_position;
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to move down: {}", e));
                    }
                }
            }
        }
        Ok(())
    }

    async fn save_episode(&mut self, episode_id: i64) -> Result<()> {
        let is_youtube = self.queue_items.iter()
            .find(|item| item.episode_id == episode_id)
            .map(|item| item.is_youtube)
            .unwrap_or(false);
            
        match self.client.save_episode(episode_id, is_youtube).await {
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

    fn render_queue_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.queue_items.is_empty() {
            self.render_empty_queue(frame, area);
            return;
        }

        // Pre-calculate scrolled titles to avoid borrowing issues
        let mut scrolled_titles = Vec::new();
        let available_width = area.width as usize;
        
        // First collect the data we need to avoid borrowing conflicts
        let item_data: Vec<(usize, String, String, i64)> = self.queue_items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let podcast_name = item.podcast_name.clone();
                let title = item.episode_title.clone();
                let episode_id = item.episode_id;
                (index, podcast_name, title, episode_id)
            })
            .collect();
        
        // Now we can safely call mutable methods
        for (index, podcast_name, title, episode_id) in item_data {
            // Calculate available width for title (approximate)
            let podcast_width = podcast_name.chars().count();
            let status_width = 20; // Approximate width for status indicators
            let other_chars = 10; // " • " and padding
            let title_max_width = available_width.saturating_sub(podcast_width + status_width + other_chars).max(20);
            
            // Use scrolling for long titles
            let scroll_key = format!("queue_{}_{}", index, episode_id);
            let displayed_title = self.get_scrolled_text(scroll_key, &title, title_max_width);
            scrolled_titles.push(displayed_title);
        }

        let items: Vec<ListItem> = self.queue_items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let duration = format_duration(item.episode_duration);
                let pub_date = format_pub_date(&item.episode_pub_date);
                
                // Get pre-calculated scrolled title
                let displayed_title = scrolled_titles.get(index).cloned().unwrap_or_else(|| item.episode_title.clone());
                
                // Status indicators
                let mut indicators = Vec::new();
                indicators.push(format!("#{}", item.queue_position)); // Queue position
                if item.completed {
                    indicators.push("✅".to_string());
                } else if item.listen_duration.unwrap_or(0) > 0 {
                    indicators.push("▶️".to_string());
                }
                if item.saved {
                    indicators.push("⭐".to_string());
                }
                indicators.push("📝".to_string()); // Always show queued indicator
                if item.downloaded {
                    indicators.push("📥".to_string());
                }

                let status = format!(" {}", indicators.join(" "));

                let theme_colors = self.theme_manager.get_colors();
                let line1 = Line::from(vec![
                    Span::styled(displayed_title, Style::default().fg(theme_colors.text).add_modifier(Modifier::BOLD)),
                    Span::styled(status, Style::default().fg(theme_colors.success)),
                ]);

                let line2 = Line::from(vec![
                    Span::styled(&item.podcast_name, Style::default().fg(theme_colors.accent)),
                    Span::styled(format!(" • {}", pub_date), Style::default().fg(theme_colors.warning)),
                    Span::styled(format!(" • {}", duration), Style::default().fg(theme_colors.text_secondary)),
                ]);

                ListItem::new(Text::from(vec![line1, line2]))
            })
            .collect();

        let total_duration: i64 = self.queue_items.iter().map(|item| item.episode_duration).sum();
        let title = format!("Queue ({} episodes, {})", 
                          self.queue_items.len(),
                          format_duration(total_duration));

        let theme_colors = self.theme_manager.get_colors();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(format!("📝 {}", title))
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(theme_colors.primary))
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

    fn render_empty_queue(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let empty_text = vec![
            Line::from(vec![Span::styled("📝 Your queue is empty", Style::default().fg(theme_colors.accent).add_modifier(Modifier::BOLD))]),
            Line::from(""),
            Line::from(vec![Span::styled("Add episodes to your queue from:", Style::default().fg(theme_colors.text))]),
            Line::from(vec![Span::styled("• Episode lists", Style::default().fg(theme_colors.text_secondary))]),
            Line::from(vec![Span::styled("• Search results", Style::default().fg(theme_colors.text_secondary))]),
            Line::from(vec![Span::styled("• Podcast pages", Style::default().fg(theme_colors.text_secondary))]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("'a'", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" on any episode to add it to your queue", Style::default().fg(theme_colors.text_secondary)),
            ]),
        ];

        let empty_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("📝 Empty Queue")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme_colors.border))
            .title_style(Style::default().fg(theme_colors.accent));

        let empty_paragraph = Paragraph::new(empty_text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(empty_block);

        frame.render_widget(empty_paragraph, area);
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let help_text = vec![
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Play  ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("Space", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Actions  ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("r", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Remove  ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("c", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Clear  ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("s", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Shuffle", Style::default().fg(theme_colors.text_secondary)),
            ]),
            Line::from(vec![
                Span::styled("t", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Move to top  ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("b", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Move to bottom  ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("u", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Move up one  ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("d", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Move down one", Style::default().fg(theme_colors.text_secondary)),
            ]),
            Line::from(vec![
                Span::styled("F5", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Refresh", Style::default().fg(theme_colors.text_secondary)),
            ]),
        ];

        let help_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("🔧 Controls")
            .border_style(Style::default().fg(theme_colors.border))
            .title_style(Style::default().fg(theme_colors.accent));

        let help_paragraph = Paragraph::new(help_text)
            .alignment(Alignment::Left)
            .block(help_block);

        frame.render_widget(help_paragraph, area);
    }

    fn render_actions_modal(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
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
            "🔼 Move Up One",
            "🔽 Move Down One",
        ];

        let items: Vec<ListItem> = actions
            .iter()
            .enumerate()
            .map(|(i, action)| {
                let style = if i == self.selected_action {
                    Style::default().fg(theme_colors.warning).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme_colors.text)
                };
                ListItem::new(Text::from(*action)).style(style)
            })
            .collect();

        let actions_list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("⚡ Actions")
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(theme_colors.accent))
                    .title_style(Style::default().fg(theme_colors.accent))
                    .style(Style::default().bg(theme_colors.container))
            )
            .highlight_style(
                Style::default()
                    .bg(theme_colors.primary)
                    .fg(theme_colors.background)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("► ");

        frame.render_widget(actions_list, popup_area);
    }

    fn render_loading(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let loading_text = Paragraph::new("🔄 Loading queue...")
            .style(Style::default().fg(theme_colors.accent))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Loading")
                    .border_style(Style::default().fg(theme_colors.accent))
                    .title_style(Style::default().fg(theme_colors.accent))
            );

        frame.render_widget(loading_text, area);
    }

    fn render_error(&self, frame: &mut Frame, area: Rect, error: &str) {
        let theme_colors = self.theme_manager.get_colors();
        let error_text = Paragraph::new(format!("❌ {}\n\nPress F5 to retry", error))
            .style(Style::default().fg(theme_colors.error))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("❌ Error")
                    .border_style(Style::default().fg(theme_colors.error))
                    .title_style(Style::default().fg(theme_colors.error))
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

fn format_pub_date(date_str: &str) -> String {
    // Simple date formatting - you could use chrono for more sophisticated parsing
    if let Some(date_part) = date_str.split('T').next() {
        date_part.to_string()
    } else {
        date_str.to_string()
    }
}