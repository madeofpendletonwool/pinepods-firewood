use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::{CrosstermBackend, Terminal},
};
use std::{
    io,
    time::{Duration, Instant},
};

use pinepods_firewood::auth::{LoginTui, SessionInfo};
use pinepods_firewood::tui::TuiApp;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, DisableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the application
    let result = run_app(&mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Handle any errors
    if let Err(err) = result {
        eprintln!("Application error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::prelude::Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    // First, try to authenticate
    let session_info = authenticate(terminal).await?;
    
    // Create and run the main TUI application
    let mut app = TuiApp::new(session_info)?;
    app.initialize().await?;
    
    run_main_app(terminal, app).await
}

async fn authenticate<B: ratatui::prelude::Backend>(terminal: &mut Terminal<B>) -> Result<SessionInfo> {
    let mut login_tui = LoginTui::new();
    
    // Check for existing authentication
    if let Some(session_info) = login_tui.check_existing_auth().await? {
        return Ok(session_info);
    }
    
    // Run login flow
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();
    
    loop {
        terminal.draw(|f| login_tui.render(f, f.size()))?;
        
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
            
        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Handle quit
                    match key.code {
                        crossterm::event::KeyCode::Char('q') => {
                            return Err(anyhow::anyhow!("User quit during login"));
                        }
                        crossterm::event::KeyCode::Esc => {
                            return Err(anyhow::anyhow!("User cancelled login"));
                        }
                        _ => {}
                    }
                    
                    // Handle login input
                    if let Some(session_info) = login_tui.handle_input(key).await? {
                        return Ok(session_info);
                    }
                }
            }
        }
        
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
}

async fn run_main_app<B: ratatui::prelude::Backend>(
    terminal: &mut Terminal<B>, 
    mut app: TuiApp
) -> Result<()> {
    let tick_rate = Duration::from_millis(50);
    let mut last_tick = Instant::now();
    
    loop {
        terminal.draw(|f| app.render(f))?;
        
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
            
        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_input(key).await?;
                    
                    if app.should_quit() {
                        break;
                    }
                }
            }
        }
        
        if last_tick.elapsed() >= tick_rate {
            app.update().await?;
            last_tick = Instant::now();
        }
    }
    
    Ok(())
}