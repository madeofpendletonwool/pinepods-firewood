use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::api::PinepodsClient;

pub struct SearchPage {
    client: PinepodsClient,
}

impl SearchPage {
    pub fn new(client: PinepodsClient) -> Self {
        Self { client }
    }

    pub async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }

    pub async fn refresh(&mut self) -> Result<()> {
        Ok(())
    }

    pub async fn handle_input(&mut self, _key: KeyEvent) -> Result<()> {
        Ok(())
    }

    pub async fn update(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let placeholder = ratatui::widgets::Paragraph::new("Search page - Coming soon!")
            .block(ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title("Search"));
        frame.render_widget(placeholder, area);
    }
}