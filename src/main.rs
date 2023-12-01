mod app;
mod config;

use std::{
    error::Error,
    io,
    time,
    time::{Duration, Instant},
};
use app::{App, AppTab, InputMode, SelectedItem, BrowserItem};
use std::fmt::format;
use std::thread::sleep;
use serde::Deserialize;
use crossterm::{
    event::{self, DisableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand
};
use ratatui::{
    prelude::{CrosstermBackend, Stylize, Terminal, Backend},
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Span, Line, Text},
    widgets::{Block, BorderType, Borders, Cell, Gauge, List, ListItem, Row, Table, Tabs, Paragraph},
    Frame
};
// use app::{App, AppTab, InputMode};
use config::Config;
use pinepods_firewood::gen_funcs;
use std::ops::Not;
use std::io::{Write, stderr, Result};
use serde_derive::Serialize;
use serde_json::to_string;
use std::sync::{Arc, Mutex};
use log::{info, debug, warn, error};


#[derive(Debug, Deserialize)]
struct PinepodsCheck {
    status_code: u16,
    pinepods_instance: bool
}

#[derive(Debug, Serialize, Deserialize)]
struct PinepodsConfig {
    url: String,
    api_key: String
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let mut shared_values = Arc::new(Mutex::new(pinepods_firewood::helpers::requests::ReqwestValues {
        url: String::new(),
        api_key: String::new(),
        user_id: 2,
    }));

    // let mut pinepods_values = shared_values.lock().unwrap();

    let mut error_check = true;
    let mut hostname: String = String::new();
    let mut web_protocol: String = String::new();
    let mut api_key: String = String::new();

    {
        let mut pinepods_values = shared_values.lock().unwrap();
        let config_test = pinepods_firewood::helpers::requests::test_existing_config();
        match config_test.await {
            Ok(data) => {
                println!("Heres the url {}", data.url);
                pinepods_values.url = String::from(data.url);
                pinepods_values.api_key = data.api_key;

                match pinepods_values.get_userid().await {
                    Ok(id) => {
                        println!("Podcasts: {:?}", &id);
                        pinepods_values.user_id = id;
                    }
                    Err(e) => eprintln!("Request failed: {:?}", e),
                }
            }
            Err(data) => {
                let firewood = "
       (
        )
       (  (
           )
     (    (  ,,           _/_/_/    _/                                                _/
      ) /\\ -((          _/    _/      _/_/_/      _/_/    _/_/_/      _/_/      _/_/_/    _/_/_/
    (  // | (`'         _/_/_/    _/  _/    _/  _/_/_/_/  _/    _/  _/    _/  _/    _/  _/_/
  _ -.;_/ \\--._       _/        _/  _/    _/  _/        _/    _/  _/    _/  _/    _/      _/_/
 (_;-// | \\ \\-'.    _/        _/  _/    _/    _/_/_/  _/_/_/      _/_/      _/_/_/  _/_/_/
 ( `.__ _  ___,')                                    _/
  `'(_ )_)(_)_)'                                    _/
    ";
                println!("{}", firewood);
                println!("Hello! Welcome to Pinepods Firewood!");
                println!("This appears to be your first time starting the app. We'll first need to connect you to your Pinepods Server. Please enter your hostname below:");
                while error_check {
                    println!("Is your server HTTP or HTTPS?");
                    loop {
                        web_protocol.clear();
                        std::io::stdin().read_line(&mut web_protocol).unwrap();

                        let trimmed_protocol = web_protocol.trim().to_lowercase();

                        if trimmed_protocol == "http" || trimmed_protocol == "https" {
                            break
                        } else {
                            println!("Invalid protocol. Please enter HTTP or HTTPS.");
                        }
                    }


                    println!("Please enter your hostname/ip without the http protocol below:");
                    println!("EX. pinepods.online, 10.0.0.10:8040");

                    io::stdin().read_line(&mut hostname).unwrap();
                    let url_build = String::from((format!("{}{}{}", web_protocol.to_lowercase().trim(), "://", hostname.trim())));
                    pinepods_values.url = url_build;
                    match pinepods_values.make_request().await {
                        Ok(data) => {
                            if data.status_code == 200 {
                                loop {
                                    println!("Connection Successful! Now please enter your api key to login:");
                                    println!("If you aren't sure how to add an api key you can consult the docs here: https://www.pinepods.online/docs/tutorial-basics/adding-an-api-key");
                                    io::stdin().read_line(&mut api_key).unwrap();
                                    pinepods_values.api_key = api_key.clone();
                                    let return_verify_login = pinepods_values.verify_key();
                                    match return_verify_login.await {
                                        Ok(data) => {
                                            println!("Login Successful! Saving configuration and starting application!:");
                                            let file_result = pinepods_values.store_pinepods_info();
                                            loop {
                                                match file_result.await {
                                                    Ok(data) => { break }
                                                    Err(e) => panic!("Unable to save configuration! Maybe you don't have permission to config location, {}", e)
                                                }
                                            }
                                            break
                                        }
                                        Err(e) => println!("API Key is not valid: {:?}", e)
                                    }
                                    println!("Please try again");
                                }
                                let temp_time = time::Duration::from_secs(2);
                                tokio::time::sleep(temp_time).await;
                                error_check = false;
                            } else {
                                println!("Problem with Connection: Not a valid Pinepods Instance")
                            }
                        },
                        Err(e) => println!("Problem with Connection: {:?}", e)
                    };
                }
                match pinepods_values.get_userid().await {
                    Ok(id) => {
                        pinepods_values.user_id = id;
                    }
                    Err(e) => eprintln!("Request failed: {:?}", e),
                }
            }
        }
    }
    {
    let mut pinepods_values = shared_values.lock().unwrap();
    match pinepods_values.return_pods().await {
        Ok(pods) => println!("Podcasts: {:?}", pods),
        Err(e) => eprintln!("Request failed: {:?}", e),
    }
        }
    error!("Setting up terminal...");
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, DisableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    error!("creating app...");
    let tick_rate = Duration::from_secs(1);
    let app = App::new(shared_values.clone());
    let cfg = Config::new();
    error!("running app...");
    let res = run_app(&mut terminal, app.await, cfg, tick_rate).await;

    // restore terminal
    error!("shutdown app...");
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("{:?}", err)
    }

    Ok(())
}



async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App<'_>,
    cfg: Config,
    tick_rate: Duration,
) -> io::Result<()> {
    let mut last_tick = Instant::now();
    loop {
        terminal.draw(|f| ui::<B>(f, &mut app, &cfg))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
        if crossterm::event::poll(timeout)? {
            // different keys depending on which browser tab
            if let Event::Key(key) = event::read()? {
                match app.input_mode() {
                    // error!("setting key press...");
                    InputMode::Browser => match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('p') | KeyCode::Char(' ') => app.music_handle.play_pause(),
                        KeyCode::Char('g') => app.music_handle.skip(),
                        KeyCode::Char('a') => {
                            if let Some(SelectedItem::Episode(episode)) = app.selected_item() {
                                app.queue_items.add(episode, episode.EpisodeDuration);
                            }
                        }
                        KeyCode::Enter => app.evaluate().await,
                        KeyCode::Backspace => app.backpedal().await,
                        KeyCode::Down | KeyCode::Char('j') => app.browser_items.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.browser_items.previous(),
                        KeyCode::Right | KeyCode::Char('l') => {
                            app.browser_items.unselect();
                            app.set_input_mode(InputMode::Queue);
                            app.queue_items.next();
                        }
                        KeyCode::Tab => {
                            app.next();
                            match app.input_mode() {
                                InputMode::Controls => app.set_input_mode(InputMode::Browser),
                                _ => app.set_input_mode(InputMode::Controls),
                            };
                        }
                        _ => {}
                    },
                    InputMode::Queue => match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('p') => app.music_handle.play_pause(),
                        KeyCode::Char('g') => app.music_handle.skip(),
                        KeyCode::Enter => {
                            if let Some(i) = app.queue_items.item() {
                                app.music_handle.play(i.clone());
                            };
                        }
                        KeyCode::Down | KeyCode::Char('j') => app.queue_items.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.queue_items.previous(),
                        KeyCode::Char('r') => app.queue_items.remove(),
                        KeyCode::Left | KeyCode::Char('h') => {
                            app.queue_items.unselect();
                            app.set_input_mode(InputMode::Browser);
                            app.browser_items.next();
                        }
                        KeyCode::Tab => {
                            app.next();
                            match app.input_mode() {
                                InputMode::Controls => app.set_input_mode(InputMode::Browser),
                                _ => app.set_input_mode(InputMode::Controls),
                            };
                        }
                        _ => {}
                    },
                    InputMode::Controls => match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('p') => app.music_handle.play_pause(),
                        KeyCode::Char('g') => app.music_handle.skip(),
                        KeyCode::Down | KeyCode::Char('j') => app.control_table.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.control_table.previous(),
                        KeyCode::Tab => {
                            app.next();
                            match app.input_mode() {
                                InputMode::Controls => app.set_input_mode(InputMode::Browser),
                                _ => app.set_input_mode(InputMode::Controls),
                            };
                        }
                        _ => {}
                    },
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
}

fn ui<B: Backend>(f: &mut Frame, app: &mut App, cfg: &Config) {
    // Total Size
    let size = f.size();

    // chunking from top to bottom, 3 gets tabs displayed, the rest goes to item layouts
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
        .split(size);

    // Main Background block, covers entire screen
    let block = Block::default().style(Style::default().bg(cfg.background()));
    f.render_widget(block, size);

    // Tab Title items collected
    let titles = app
        .titles
        .iter()
        .map(|t| {
            let (first, rest) = t.split_at(1);
            Line::from(vec![
                Span::styled(first, Style::default().fg(cfg.highlight_background())), // CHANGE FOR CUSTOMIZATION
                Span::styled(rest, Style::default().fg(cfg.highlight_background())), // These are tab highlights, first vs rest diff colors
            ])
        })
        .collect();

    // Box Around Tab Items
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("Tabs"))
        .select(app.active_tab as usize)
        .style(Style::default().fg(cfg.foreground()))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(cfg.background()),
        );
    f.render_widget(tabs, chunks[0]);

    match app.active_tab {
        AppTab::Music => music_tab::<B>(f, app, chunks[1], cfg),
        AppTab::Controls => instructions_tab::<B>(f, app, chunks[1], cfg),
    };
}

fn music_tab<B: Backend>(f: &mut Frame, app: &mut App, chunks: Rect, cfg: &Config) {
    // split into left / right
    let browser_queue = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)].as_ref())
        .split(chunks);
    // f.size()

    // queue and playing sections (sltdkh)
    let queue_playing = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage(100 - cfg.progress_bar()),
                Constraint::Percentage(cfg.progress_bar()),
            ]
                .as_ref(),
        )
        .split(browser_queue[1]);

    // convert app items to text
    let items: Vec<ListItem> = app
        .browser_items
        .items()
        .iter()
        .map(|browser_item| {
            let text = match browser_item {
                BrowserItem::Podcast(podcast) => {
                    // Create a string representation for the podcast
                    // For example, using the podcast title
                    podcast.PodcastName.clone()
                },
                BrowserItem::Episode(episode) => {
                    // Create a string representation for the episode
                    // For example, using the episode title
                    episode.EpisodePubDate.clone() + " - " + &
                    episode.EpisodeTitle.clone()
                }
            };

            // Convert the string to Text
            ListItem::new(Text::from(text))
        })
        .collect();

    // Create a List from all list items and highlight the currently selected one // RENDER 1
    let items = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Browser")
                .title_alignment(Alignment::Left)
                .border_type(BorderType::Rounded),
        )
        .style(Style::default().fg(cfg.foreground()))
        .highlight_style(
            Style::default()
                .bg(cfg.highlight_background())
                .fg(cfg.highlight_foreground())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    f.render_stateful_widget(items, browser_queue[0], &mut app.browser_items.state());

    let queue_items: Vec<ListItem> = app
        .queue_items
        .items()
        .iter()
        .map(|i| ListItem::new(Text::from(gen_funcs::audio_display(i))))
        .collect();

    let queue_title = format!(
        "| Queue: {queue_items} Songs |{total_time}",
        queue_items = app.queue_items.length(),
        total_time = app.queue_items.total_time(),
    );

    let queue_items = List::new(queue_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(queue_title)
                .title_alignment(Alignment::Left)
                .border_type(BorderType::Rounded),
        )
        .style(Style::default().fg(cfg.foreground()))
        .highlight_style(
            Style::default()
                .bg(cfg.highlight_background())
                .fg(cfg.highlight_foreground())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    f.render_stateful_widget(queue_items, queue_playing[0], &mut app.queue_items.state());

    let playing_title = format!("| {current_song} |", current_song = app.current_song());

    // Note Gauge is using background color for progress
    let playing = Gauge::default()
        .block(
            Block::default()
                .title(playing_title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title_alignment(Alignment::Center),
        )
        .style(Style::default().fg(cfg.foreground()))
        .gauge_style(Style::default().fg(cfg.highlight_background()))
        .percent(app.song_progress());
    f.render_widget(playing, queue_playing[1]);
}

fn instructions_tab<B: Backend>(f: &mut Frame, app: &mut App, chunks: Rect, cfg: &Config) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([Constraint::Percentage(100)].as_ref())
        .split(chunks);

    // map header to tui object
    let header = app
        .control_table
        .header
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(cfg.highlight_foreground())));

    // Header and first row
    let header = Row::new(header)
        .style(Style::default().bg(cfg.background()).fg(cfg.foreground()))
        .height(1)
        .bottom_margin(1);

    // map items from table to Row items
    let rows = app.control_table.items.iter().map(|item| {
        let height = item
            .iter()
            .map(|content| content.chars().filter(|c| *c == '\n').count())
            .max()
            .unwrap_or(0)
            + 1;
        let cells = item.iter().map(|c| Cell::from(*c));
        Row::new(cells).height(height as u16).bottom_margin(1)
    });

    let t = Table::new(rows)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Controls"))
        .style(Style::default().fg(cfg.foreground()).bg(cfg.background()))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(cfg.highlight_background())
                .fg(cfg.highlight_foreground()),
        )
        // .highlight_symbol(">> ")
        .widths(&[
            Constraint::Percentage(50),
            Constraint::Length(30),
            Constraint::Min(10),
        ]);
    f.render_stateful_widget(t, chunks[0], &mut app.control_table.state);
}


