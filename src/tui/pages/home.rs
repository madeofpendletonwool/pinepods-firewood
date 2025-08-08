use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, 
        Paragraph, Wrap
    },
    Frame,
};
use std::time::Instant;

use crate::api::{PinepodsClient, HomeOverview, Episode, Playlist};

pub struct HomePage {
    client: PinepodsClient,
    
    // Data
    overview: Option<HomeOverview>,
    
    // UI State
    selected_section: usize,
    selected_item: usize,
    loading: bool,
    error_message: Option<String>,
    
    // Sections
    sections: Vec<HomeSection>,
    
    // Animation
    last_update: Instant,
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
        Self {
            client,
            overview: None,
            selected_section: 0,
            selected_item: 0,
            loading: false,
            error_message: None,
            sections: Vec::new(),
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
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.next_item();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.previous_item();
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                self.next_section();
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
                self.previous_section();
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
        // Auto-refresh every 5 minutes
        if self.last_update.elapsed().as_secs() > 300 {
            self.refresh().await?;
            self.last_update = Instant::now();
        }

        Ok(())
    }

    fn next_item(&mut self) {
        if let Some(section) = self.sections.get(self.selected_section) {
            if !section.items.is_empty() {
                self.selected_item = (self.selected_item + 1) % section.items.len();
            }
        }
    }

    fn previous_item(&mut self) {
        if let Some(section) = self.sections.get(self.selected_section) {
            if !section.items.is_empty() {
                if self.selected_item == 0 {
                    self.selected_item = section.items.len() - 1;
                } else {
                    self.selected_item -= 1;
                }
            }
        }
    }

    fn next_section(&mut self) {
        if !self.sections.is_empty() {
            self.selected_section = (self.selected_section + 1) % self.sections.len();
            self.selected_item = 0;
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
        let items: Vec<ListItem> = self.sections
            .iter()
            .enumerate()
            .map(|(i, section)| {
                let style = if i == self.selected_section {
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
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
                    .border_style(Style::default().fg(Color::Blue))
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            );

        frame.render_widget(list, area);
    }

    fn render_section_content(&self, frame: &mut Frame, area: Rect) {
        if let Some(section) = self.sections.get(self.selected_section) {
            let items: Vec<ListItem> = section.items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let style = if i == self.selected_item {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let text = match item {
                        HomeItem::Episode(episode) => {
                            let podcast_name = episode.podcast_name
                                .as_deref()
                                .unwrap_or("Unknown Podcast");
                            
                            format!("🎵 {} - {}", podcast_name, episode.episode_title)
                        }
                        HomeItem::Playlist(playlist) => {
                            let count = playlist.episode_count.unwrap_or(0);
                            format!("📋 {} ({} episodes)", playlist.name, count)
                        }
                        HomeItem::Action(title, description) => {
                            format!("⚡ {} - {}", title, description)
                        }
                        HomeItem::Stat(text) => {
                            format!("📊 {}", text)
                        }
                    };

                    ListItem::new(Text::from(text)).style(style)
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(format!("{} {}", section.icon, section.title))
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
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let controls = vec![
            ("←→/hl", "Navigate sections"),
            ("↑↓/jk", "Navigate items"),
            ("Tab", "Switch panels"),
            ("Enter", "Activate"),
            ("r", "Refresh"),
        ];

        let footer_text: Vec<Span> = controls
            .iter()
            .enumerate()
            .flat_map(|(i, (key, desc))| {
                let mut spans = vec![
                    Span::styled(*key, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {}", desc), Style::default().fg(Color::Gray)),
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
                    .border_style(Style::default().fg(Color::Blue))
            );

        frame.render_widget(footer, area);
    }

    fn render_loading(&self, frame: &mut Frame, area: Rect) {
        let loading_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Loading Home...")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(Color::Yellow));

        let loading_text = Paragraph::new("🔄 Loading your personalized home page...")
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(loading_block);

        frame.render_widget(loading_text, area);
    }

    fn render_error(&self, frame: &mut Frame, area: Rect, error: &str) {
        let error_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Error")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(Color::Red));

        let error_text = Paragraph::new(format!("❌ {}\n\nPress 'r' to retry", error))
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(error_block);

        frame.render_widget(error_text, area);
    }

    fn render_empty(&self, frame: &mut Frame, area: Rect) {
        let welcome_text = vec![
            Line::from(vec![
                Span::styled("🌲 Welcome to Pinepods Firewood! 🌲", 
                           Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
            Line::from("It looks like you don't have any recent activity yet."),
            Line::from("Here are some things you can do to get started:"),
            Line::from(""),
            Line::from("• Subscribe to podcasts in the Podcasts tab"),
            Line::from("• Search for episodes in the Search tab"),
            Line::from("• Import your subscriptions in Settings"),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::Gray)),
                Span::styled("Tab", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" to navigate between tabs", Style::default().fg(Color::Gray)),
            ]),
        ];

        let welcome_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Welcome")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(Color::Blue));

        let welcome_paragraph = Paragraph::new(welcome_text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(welcome_block);

        frame.render_widget(welcome_paragraph, area);
    }
}