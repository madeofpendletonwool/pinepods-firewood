use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Gauge, Paragraph, Wrap},
};
use std::time::Duration;

use crate::audio::{AudioPlayer, AudioPlayerState, PlaybackState};
use crate::api::Episode;
use crate::theme::ThemeManager;

pub struct PlayerPage {
    audio_player: AudioPlayer,
    last_state: AudioPlayerState,
    
    // Theme management
    theme_manager: ThemeManager,
}

impl PlayerPage {
    pub fn new(audio_player: AudioPlayer) -> Self {
        Self {
            last_state: audio_player.get_state(),
            audio_player,
            theme_manager: ThemeManager::new(),
        }
    }

    pub async fn handle_input(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Char(' ') => {
                self.audio_player.toggle_play_pause()?;
            }
            KeyCode::Right => {
                self.audio_player.skip_forward(Duration::from_secs(15))?;
            }
            KeyCode::Left => {
                self.audio_player.skip_backward(Duration::from_secs(15))?;
            }
            KeyCode::Up => {
                let current_volume = self.last_state.volume;
                self.audio_player.set_volume((current_volume + 0.1).min(1.0))?;
            }
            KeyCode::Down => {
                let current_volume = self.last_state.volume;
                self.audio_player.set_volume((current_volume - 0.1).max(0.0))?;
            }
            KeyCode::Char('s') => {
                self.audio_player.stop()?;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                return Ok(false); // Close player
            }
            _ => {}
        }
        Ok(true)
    }

    pub async fn play_episode(&mut self, episode: Episode) -> Result<()> {
        self.audio_player.play_episode(episode)?;
        Ok(())
    }

    pub fn update(&mut self) -> Result<()> {
        self.last_state = self.audio_player.get_state();
        Ok(())
    }
    
    // Method to update theme from external source (like server sync)
    pub fn update_theme(&mut self, theme_name: &str) {
        self.theme_manager.set_theme(theme_name);
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let current_state = self.audio_player.get_state();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Title bar
                Constraint::Length(6),  // Episode info
                Constraint::Length(3),  // Progress bar
                Constraint::Length(3),  // Time info
                Constraint::Length(4),  // Controls
                Constraint::Min(3),     // Status/debug info
            ])
            .split(area);

        // Title bar
        self.render_title(frame, chunks[0]);
        
        // Episode info
        self.render_episode_info(frame, chunks[1], &current_state);
        
        // Progress bar
        self.render_progress(frame, chunks[2], &current_state);
        
        // Time info
        self.render_time_info(frame, chunks[3], &current_state);
        
        // Controls
        self.render_controls(frame, chunks[4], &current_state);
        
        // Status
        self.render_status(frame, chunks[5], &current_state);
    }

    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let theme_colors = self.theme_manager.get_colors();
        let title = Paragraph::new("🎵 Audio Player")
            .style(Style::default().fg(theme_colors.accent).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme_colors.accent))
            );
        frame.render_widget(title, area);
    }

    fn render_episode_info(&self, frame: &mut Frame, area: Rect, state: &AudioPlayerState) {
        let theme_colors = self.theme_manager.get_colors();
        let content = if let Some(ref episode) = state.current_episode {
            let podcast_name = episode.podcast_name.as_deref().unwrap_or("Unknown Podcast");
            let episode_title = &episode.episode_title;
            
            vec![
                Line::from(vec![
                    Span::styled("Podcast: ", Style::default().fg(theme_colors.text_secondary)),
                    Span::styled(podcast_name, Style::default().fg(theme_colors.accent).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("Episode: ", Style::default().fg(theme_colors.text_secondary)),
                    Span::styled(episode_title, Style::default().fg(theme_colors.text).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(""), // Spacer
                Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(theme_colors.text_secondary)),
                    Span::styled(self.format_playback_state(&state.playback_state), self.get_status_style(&state.playback_state)),
                ]),
            ]
        } else {
            vec![
                Line::from(Span::styled(
                    "No episode loaded",
                    Style::default().fg(theme_colors.text_secondary).add_modifier(Modifier::ITALIC),
                )),
            ]
        };

        let episode_info = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("📻 Now Playing")
                    .border_style(Style::default().fg(theme_colors.border))
                    .title_style(Style::default().fg(theme_colors.accent))
            )
            .wrap(Wrap { trim: true });

        frame.render_widget(episode_info, area);
    }

    fn render_progress(&self, frame: &mut Frame, area: Rect, state: &AudioPlayerState) {
        let theme_colors = self.theme_manager.get_colors();
        let progress = if state.total_duration.as_secs() > 0 {
            state.current_position.as_secs_f64() / state.total_duration.as_secs_f64()
        } else {
            0.0
        };

        let progress_bar = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("Progress")
                    .border_style(Style::default().fg(theme_colors.border))
                    .title_style(Style::default().fg(theme_colors.accent))
            )
            .gauge_style(Style::default().fg(theme_colors.primary).bg(theme_colors.container))
            .ratio(progress)
            .label(format!("{:.1}%", progress * 100.0));

        frame.render_widget(progress_bar, area);
    }

    fn render_time_info(&self, frame: &mut Frame, area: Rect, state: &AudioPlayerState) {
        let theme_colors = self.theme_manager.get_colors();
        let current_time = Self::format_duration(state.current_position);
        let total_time = Self::format_duration(state.total_duration);
        let remaining_time = Self::format_duration(state.total_duration.saturating_sub(state.current_position));

        let time_info = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Time: ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled(format!("{} / {}", current_time, total_time), Style::default().fg(theme_colors.text)),
                Span::styled(format!(" (-{})", remaining_time), Style::default().fg(theme_colors.warning)),
            ]),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme_colors.border))
        );

        frame.render_widget(time_info, area);
    }

    fn render_controls(&self, frame: &mut Frame, area: Rect, state: &AudioPlayerState) {
        let theme_colors = self.theme_manager.get_colors();
        let controls_text = vec![
            Line::from(vec![
                Span::styled("Space", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Play/Pause  ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("←/→", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" ±15s  ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("↑/↓", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Volume  ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled("S", Style::default().fg(theme_colors.primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Stop", Style::default().fg(theme_colors.text_secondary)),
            ]),
            Line::from(vec![
                Span::styled("Volume: ", Style::default().fg(theme_colors.text_secondary)),
                Span::styled(format!("{:.0}%", state.volume * 100.0), Style::default().fg(theme_colors.warning)),
                Span::raw("  "),
                Span::styled("Q/Esc", Style::default().fg(theme_colors.error).add_modifier(Modifier::BOLD)),
                Span::styled(" Close Player", Style::default().fg(theme_colors.text_secondary)),
            ]),
        ];

        let controls = Paragraph::new(controls_text)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("🔧 Controls")
                    .border_style(Style::default().fg(theme_colors.border))
                    .title_style(Style::default().fg(theme_colors.accent))
            );

        frame.render_widget(controls, area);
    }

    fn render_status(&self, frame: &mut Frame, area: Rect, state: &AudioPlayerState) {
        let theme_colors = self.theme_manager.get_colors();
        let status_text = match &state.playback_state {
            PlaybackState::Error(msg) => vec![
                Line::from(vec![
                    Span::styled("❌ Error: ", Style::default().fg(theme_colors.error).add_modifier(Modifier::BOLD)),
                    Span::styled(msg, Style::default().fg(theme_colors.error)),
                ]),
            ],
            PlaybackState::Loading => vec![
                Line::from(Span::styled("🔄 Loading audio...", Style::default().fg(theme_colors.warning).add_modifier(Modifier::BOLD))),
            ],
            _ => vec![
                Line::from(vec![
                    Span::styled("Ready", Style::default().fg(theme_colors.success)),
                    Span::raw(" - "),
                    Span::styled("Press Space to play/pause, ←/→ to skip", Style::default().fg(theme_colors.text_secondary)),
                ]),
            ],
        };

        let status = Paragraph::new(status_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title("📊 Status")
                    .border_style(Style::default().fg(theme_colors.border))
                    .title_style(Style::default().fg(theme_colors.accent))
            )
            .wrap(Wrap { trim: true });

        frame.render_widget(status, area);
    }

    fn format_playback_state(&self, state: &PlaybackState) -> String {
        match state {
            PlaybackState::Stopped => "Stopped".to_string(),
            PlaybackState::Playing => "Playing".to_string(),
            PlaybackState::Paused => "Paused".to_string(),
            PlaybackState::Loading => "Loading...".to_string(),
            PlaybackState::Error(_) => "Error".to_string(),
        }
    }

    fn get_status_style(&self, state: &PlaybackState) -> Style {
        let theme_colors = self.theme_manager.get_colors();
        match state {
            PlaybackState::Stopped => Style::default().fg(theme_colors.text_secondary),
            PlaybackState::Playing => Style::default().fg(theme_colors.success).add_modifier(Modifier::BOLD),
            PlaybackState::Paused => Style::default().fg(theme_colors.warning),
            PlaybackState::Loading => Style::default().fg(theme_colors.accent),
            PlaybackState::Error(_) => Style::default().fg(theme_colors.error).add_modifier(Modifier::BOLD),
        }
    }

    fn format_duration(duration: Duration) -> String {
        let total_seconds = duration.as_secs();
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if hours > 0 {
            format!("{}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{}:{:02}", minutes, seconds)
        }
    }
}