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
use pinepods_firewood::remote::RemoteControlServer;
use pinepods_firewood::config::{get_preferred_remote_port, is_remote_control_enabled};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();

    // Set up panic hook to capture panic messages
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("PANIC: {:?}", panic_info);
        if let Some(location) = panic_info.location() {
            eprintln!("PANIC LOCATION: {}:{}:{}", location.file(), location.line(), location.column());
        }
        if let Some(msg) = panic_info.payload().downcast_ref::<&str>() {
            eprintln!("PANIC MESSAGE: {}", msg);
        }
        std::process::exit(1);
    }));

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
    
    // Try to start the remote control server (optional)
    let remote_handle = if is_remote_control_enabled() {
        let client = app.client().clone();
        let audio_player = app.audio_player().clone();
        let preferred_port = get_preferred_remote_port();
        
        match RemoteControlServer::new(
            Some(audio_player),
            Some(client),
            Some(preferred_port),
        ) {
        Ok(mut remote_server) => {
            let allocated_port = remote_server.get_port();
            // Spawn the remote control server in the background
            let handle = tokio::spawn(async move {
                if let Err(e) = remote_server.start().await {
                    log::error!("Remote control server failed: {}", e);
                }
            });
            log::info!("Remote control server started on port {}", allocated_port);
            Some(handle)
        }
        Err(e) => {
            let error_msg = format!("Failed to create remote control server: {}", e);
            log::warn!("{}", error_msg);
            app.show_error_message(&error_msg);
            log::info!("Continuing without remote control functionality");
            None
        }
        }
    } else {
        log::info!("Remote control server disabled via configuration");
        None
    };
    
    log::info!("Use Ctrl+C or 'q' to quit");
    
    // Main TUI loop
    let main_result = async {
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
        Ok::<(), anyhow::Error>(())
    }.await;
    
    // Gracefully stop the remote control server if it was started
    if let Some(handle) = remote_handle {
        log::info!("Shutting down remote control server...");
        // Give it a moment to clean up
        if tokio::time::timeout(Duration::from_secs(2), async {
            handle.abort();
            // The abort will trigger cleanup in the Drop impl
        }).await.is_err() {
            log::warn!("Remote control server shutdown timed out");
        }
    }
    
    main_result
}