use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState,
        Paragraph, Wrap
    },
    Frame,
};
use std::time::{Duration, Instant};
use std::collections::HashMap;

use crate::api::{PinepodsClient, HomeOverview, Episode, Playlist};
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

pub struct HomePage {
    client: PinepodsClient,
    
    // Data
    overview: Option<HomeOverview>,
    
    // UI State
    selected_section: usize,
    selected_item: usize,
    focused_panel: FocusPanel,
    item_list_state: ListState,
    loading: bool,
    error_message: Option<String>,
    
    // Sections
    sections: Vec<HomeSection>,
    
    // Theme management
    theme_manager: ThemeManager,
    
    // Title scrolling animation
    scroll_states: HashMap<String, ScrollState>,
    last_scroll_update: Instant,
    
    // Animation
    last_update: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FocusPanel {
    Sections,
    Items,
}

#[derive(Debug, Clone)]
struct HomeSection {
    title: String,
    items: Vec<HomeItem>,
    icon: String,
}

#[derive(Debug, Clone)]
enum HomeItem {
    Episode(Episode),
    Playlist(Playlist),
    Action(String, String), // title, description
    Stat(String), // statistic text
}

impl HomePage {
    pub fn new(client: PinepodsClient) -> Self {
        let mut item_list_state = ListState::default();
        item_list_state.select(Some(0));
        
        Self {
            client,
            overview: None,
            selected_section: 0,
            selected_item: 0,
            focused_panel: FocusPanel::Sections,
            item_list_state,
            loading: false,
            error_message: None,
            sections: Vec::new(),
            theme_manager: ThemeManager::new(),
            scroll_states: HashMap::new(),
            last_scroll_update: Instant::now(),
            last_update: Instant::now(),
        }
    }

    pub async fn initialize(&mut self) -> Result<()> {
        self.refresh().await
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.loading = true;
        self.error_message = None;

        match self.client.get_home_overview().await {
            Ok(overview) => {
                self.overview = Some(overview.clone());
                self.build_sections(overview);
                self.loading = false;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load home data: {}", e));
                self.loading = false;
                return Err(e);
            }
        }

        Ok(())
    }

    fn build_sections(&mut self, overview: HomeOverview) {
        self.sections.clear();

        // Recent Episodes
        if !overview.recent_episodes.is_empty() {
            let items = overview.recent_episodes
                .into_iter()
                .map(HomeItem::Episode)
                .collect();
            
            self.sections.push(HomeSection {
                title: "Recent Episodes".to_string(),
                items,
                icon: "🆕".to_string(),
            });
        }

        // In Progress Episodes
        if !overview.in_progress_episodes.is_empty() {
            let items = overview.in_progress_episodes
                .into_iter()
                .map(HomeItem::Episode)
                .collect();
            
            self.sections.push(HomeSection {
                title: "Continue Listening".to_string(),
                items,
                icon: "▶️".to_string(),
            });
        }

        // Quick Stats - using the count values from the API
        let stats = vec![
            HomeItem::Stat(format!("Saved Episodes: {}", overview.saved_count)),
            HomeItem::Stat(format!("Downloaded Episodes: {}", overview.downloaded_count)),
            HomeItem::Stat(format!("Queued Episodes: {}", overview.queue_count)),
            HomeItem::Stat(format!("Top Podcasts: {}", overview.top_podcasts.len())),
        ];

        self.sections.push(HomeSection {
            title: "Statistics".to_string(),
            items: stats,
            icon: "📊".to_string(),
        });


        // Reset selection if needed
        if self.selected_section >= self.sections.len() {
            self.selected_section = 0;
        }
        self.selected_item = 0;
        
        // Update item list state for the first section
        if !self.sections.is_empty() {
            if let Some(section) = self.sections.get(self.selected_section) {
                if !section.items.is_empty() {
                    self.item_list_state.select(Some(0));
                } else {
                    self.item_list_state.select(None);
                }
            }
        } else {
            self.item_list_state.select(None);
        }
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                match self.focused_panel {
                    FocusPanel::Sections => {
                        self.next_section();
                    }
                    FocusPanel::Items => {
                        self.next_item();
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                match self.focused_panel {
                    FocusPanel::Sections => {
                        self.previous_section();
                    }
                    FocusPanel::Items => {
                        self.previous_item();
                    }
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                // Move to items panel if current section has items
                if let Some(section) = self.sections.get(self.selected_section) {
                    if !section.items.is_empty() {
                        self.focused_panel = FocusPanel::Items;
                    }
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                // Move back to sections panel
                self.focused_panel = FocusPanel::Sections;
            }
            KeyCode::Tab => {
                // Toggle between panels
                self.focused_panel = match self.focused_panel {
                    FocusPanel::Sections => {
                        if let Some(section) = self.sections.get(self.selected_section) {
                            if !section.items.is_empty() {
                                FocusPanel::Items
                            } else {
                                FocusPanel::Sections
                            }
                        } else {
                            FocusPanel::Sections
                        }
                    }
                    FocusPanel::Items => FocusPanel::Sections,
                };
            }
            KeyCode::Enter => {
                self.activate_selected().await?;
            }
            KeyCode::Char('r') => {
                self.refresh().await?;
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
        
        // Auto-refresh every 5 minutes
        if self.last_update.elapsed().as_secs() > 300 {
            self.refresh().await?;
            self.last_update = Instant::now();
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

    fn next_item(&mut self) {
        if let Some(section) = self.sections.get(self.selected_section) {
            if !section.items.is_empty() {
                if let Some(selected) = self.item_list_state.selected() {
                    if selected < section.items.len().saturating_sub(1) {
                        self.item_list_state.select(Some(selected + 1));
                        self.selected_item = selected + 1;
                    }
                } else {
                    self.item_list_state.select(Some(0));
                    self.selected_item = 0;
                }
            }
        }
    }

    fn previous_item(&mut self) {
        if let Some(section) = self.sections.get(self.selected_section) {
            if !section.items.is_empty() {
                if let Some(selected) = self.item_list_state.selected() {
                    if selected > 0 {
                        self.item_list_state.select(Some(selected - 1));
                        self.selected_item = selected - 1;
                    }
                } else {
                    self.item_list_state.select(Some(0));
                    self.selected_item = 0;
                }
            }
        }
    }

    fn next_section(&mut self) {
        if !self.sections.is_empty() {
            self.selected_section = (self.selected_section + 1) % self.sections.len();
            self.selected_item = 0;
            
            // Update item list state for new section
            if let Some(section) = self.sections.get(self.selected_section) {
                if !section.items.is_empty() {
                    self.item_list_state.select(Some(0));
                } else {
                    self.item_list_state.select(None);
                }
            }
        }
    }

    fn previous_section(&mut self) {
        if !self.sections.is_empty() {
            if self.selected_section == 0 {
                self.selected_section = self.sections.len() - 1;
            } else {
                self.selected_section -= 1;
            }
            self.selected_item = 0;
            
            // Update item list state for new section
            if let Some(section) = self.sections.get(self.selected_section) {
                if !section.items.is_empty() {
                    self.item_list_state.select(Some(0));
                } else {
                    self.item_list_state.select(None);
                }
            }
        }
    }

    async fn activate_selected(&mut self) -> Result<()> {
        if let Some(section) = self.sections.get(self.selected_section) {
            if let Some(item) = section.items.get(self.selected_item) {
                match item {
                    HomeItem::Episode(episode) => {
                        // TODO: Play episode or show episode details
                        println!("Playing episode: {}", episode.episode_title);
                    }
                    HomeItem::Playlist(playlist) => {
                        // TODO: Navigate to playlist
                        println!("Opening playlist: {}", playlist.name);
                    }
                    HomeItem::Action(title, _) => {
                        // TODO: Handle quick actions
                        println!("Executing action: {}", title);
                    }
                    HomeItem::Stat(_) => {
                        // Stats are not actionable
                    }
                }
            }
        }
        Ok(())
    }

    // Method to update theme from external source (like server sync)
    pub fn update_theme(&mut self, theme_name: &str) {
        self.theme_manager.set_theme(theme_name);
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

        if self.sections.is_empty() {
            self.render_empty(frame, area);
            return;
        }

        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),     // Main content
                Constraint::Length(3),  // Footer
            ])
            .split(area);

        let content_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30), // Section list
                Constraint::Percentage(70), // Content
            ])
            .split(main_layout[0]);

        // Render section list
        self.render_section_list(frame, content_layout[0]);

        // Render selected section content
        self.render_section_content(frame, content_layout[1]);

        // Render footer
        self.render_footer(frame, main_layout[1]);
    }

    fn render_section_list(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let items: Vec<ListItem> = self.sections
            .iter()
            .enumerate()
            .map(|(i, section)| {
                let style = if i == self.selected_section {
                    Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme_colors.text)
                };

                let text = format!("{} {} ({})", 
                                 section.icon, 
                                 section.title, 
                                 section.items.len());
                
                ListItem::new(Text::from(text)).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Sections")
                    .title_alignment(Alignment::Center)
                    .border_style(if self.focused_panel == FocusPanel::Sections {
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
            );

        frame.render_widget(list, area);
    }

    fn render_section_content(&mut self, frame: &mut Frame, area: Rect) {
        // First collect all the section data to avoid borrowing conflicts
        let section_data = if let Some(section) = self.sections.get(self.selected_section) {
            Some((section.items.clone(), section.title.clone(), section.icon.clone()))
        } else {
            None
        };
        
        if let Some((items, section_title, section_icon)) = section_data {
            // Pre-calculate scrolled titles for episodes to avoid borrowing issues
            let mut scrolled_titles = Vec::new();
            let available_width = area.width as usize;
            
            // First collect episode data that needs scrolling
            let episode_data: Vec<(usize, String, Option<i64>)> = items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    if let HomeItem::Episode(episode) = item {
                        Some((index, episode.episode_title.clone(), episode.episode_id))
                    } else {
                        None
                    }
                })
                .collect();
            
            // Now we can safely call mutable methods for scrolling
            for (index, title, episode_id) in episode_data {
                // Use most of the available width for episode titles
                // Status indicators are on the same line, so leave some room
                let status_width = 20; // Conservative estimate for status indicators
                let title_max_width = available_width.saturating_sub(status_width + 6).max(40);
                
                // Use scrolling for long episode titles
                let scroll_key = format!("home_episode_{}_{}", index, episode_id.unwrap_or(0));
                let displayed_title = self.get_scrolled_text(scroll_key, &title, title_max_width);
                scrolled_titles.push((index, displayed_title));
            }
            
            // Get theme colors after mutable operations
            let theme_colors = self.theme_manager.get_colors();
            
            let list_items: Vec<ListItem> = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let style = if i == self.selected_item {
                        Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme_colors.text)
                    };

                    match item {
                        HomeItem::Episode(episode) => {
                            let podcast_name = episode.podcast_name
                                .as_deref()
                                .unwrap_or("Unknown Podcast");
                            
                            let duration = format_duration(episode.episode_duration);
                            let listen_progress = episode.listen_duration.unwrap_or(0) as f64 / episode.episode_duration as f64;
                            
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
                                let progress_width = 15; // Smaller for home page
                                let filled = (listen_progress * progress_width as f64) as usize;
                                let bar = "█".repeat(filled) + &"░".repeat(progress_width - filled);
                                format!(" [{}]", bar)
                            } else {
                                String::new()
                            };

                            // Get pre-calculated scrolled title if available
                            let displayed_title = scrolled_titles.iter()
                                .find(|(index, _)| *index == i)
                                .map(|(_, title)| title.clone())
                                .unwrap_or_else(|| episode.episode_title.clone());
                            
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
                        }
                        HomeItem::Playlist(playlist) => {
                            let count = playlist.episode_count.unwrap_or(0);
                            let text = format!("📋 {} ({} episodes)", playlist.name, count);
                            ListItem::new(Text::from(text)).style(style)
                        }
                        HomeItem::Action(title, description) => {
                            let text = format!("⚡ {} - {}", title, description);
                            ListItem::new(Text::from(text)).style(style)
                        }
                        HomeItem::Stat(text) => {
                            let text = format!("📊 {}", text);
                            ListItem::new(Text::from(text)).style(style)
                        }
                    }
                })
                .collect();

            let list = List::new(list_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(format!("{} {}", section_icon, section_title))
                        .title_alignment(Alignment::Center)
                        .border_style(if self.focused_panel == FocusPanel::Items {
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

            frame.render_stateful_widget(list, area, &mut self.item_list_state);
        }
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let controls = vec![
            ("↑↓/jk", "Navigate"),
            ("←→/hl", "Switch panels"),
            ("Tab", "Switch panels"),
            ("Enter", "Activate"),
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

    fn render_loading(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let loading_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Loading Home...")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme_colors.accent));

        let loading_text = Paragraph::new("🔄 Loading your personalized home page...")
            .style(Style::default().fg(theme_colors.accent))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(loading_block);

        frame.render_widget(loading_text, area);
    }

    fn render_error(&self, frame: &mut Frame, area: Rect, error: &str) {
        let theme_colors = self.theme_manager.get_colors();
        let error_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Error")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme_colors.error));

        let error_text = Paragraph::new(format!("❌ {}\n\nPress 'r' to retry", error))
            .style(Style::default().fg(theme_colors.error))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(error_block);

        frame.render_widget(error_text, area);
    }

    fn render_empty(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let welcome_text = vec![
            Line::from(vec![
                Span::styled("🌲 Welcome to Pinepods Firewood! 🌲", 
                           Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled("It looks like you don't have any recent activity yet.", Style::default().fg(theme_colors.text))]),
            Line::from(vec![Span::styled("Here are some things you can do to get started:", Style::default().fg(theme_colors.text))]),
            Line::from(""),
            Line::from(vec![Span::styled("• Subscribe to podcasts in the Podcasts tab", Style::default().fg(theme_colors.text_secondary))]),
            Line::from(vec![Span::styled("• Search for episodes in the Search tab", Style::default().fg(theme_colors.text_secondary))]),
            Line::from(vec![Span::styled("• Import your subscriptions in Settings", Style::default().fg(theme_colors.text_secondary))]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("Tab", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" to navigate between tabs", Style::default().fg(theme_colors.text_secondary)),
            ]),
        ];

        let welcome_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Welcome")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme_colors.accent));

        let welcome_paragraph = Paragraph::new(welcome_text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(welcome_block);

        frame.render_widget(welcome_paragraph, area);
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