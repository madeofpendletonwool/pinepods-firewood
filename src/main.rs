use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{Clear, ClearType};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

use pinepods_firewood::api::http::{build_client, build_media_client};
use pinepods_firewood::api::{HttpApi, PinepodsApi};
use pinepods_firewood::art::Artwork;
use pinepods_firewood::auth::{LoginTui, SessionInfo};
use pinepods_firewood::mediakeys;
use pinepods_firewood::nowplaying::{self, NowPlaying};
use pinepods_firewood::paths;
use pinepods_firewood::playback::{Playback, PlaybackConfig};
use pinepods_firewood::settings::SettingsManager;
use pinepods_firewood::stderr::StderrRedirect;
use pinepods_firewood::tui::App;
use pinepods_firewood::tui::ui::cover::{self, Covers};
use ratatui_image::picker::Picker;

const LOG_FILES_KEPT: usize = 7;

/// Logs go to a daily file. `FIREWOOD_LOG` takes an `EnvFilter` directive
/// (`debug`, `pinepods_firewood=trace`, …); the default is `info`, minus the MP3
/// decoder's warnings about resyncing, which every seek into an MP3 stream produces.
fn setup_logging() -> Result<WorkerGuard> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("firewood")
        .filename_suffix("log")
        .max_log_files(LOG_FILES_KEPT)
        .build(paths::log_dir()?)
        .context("couldn't open the log file")?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_env("FIREWOOD_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,symphonia_bundle_mp3=error"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .init();
    Ok(guard)
}

fn main() -> Result<()> {
    // On macOS the media keys need the main thread, so Firewood runs on another.
    mediakeys::run_with_main_loop(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("couldn't start the async runtime")?
            .block_on(app_main())
    })
}

async fn app_main() -> Result<()> {
    let _log_guard = setup_logging()?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting");

    let log_dir = paths::log_dir()?;
    let crash_note = format!("Firewood crashed. Details are in {}", log_dir.display());
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        let _ = execute!(
            std::io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste
        );
        // stderr is redirected while the TUI runs, so say it on stdout.
        println!("{crash_note}");
    }));

    // Installs a panic hook of its own that restores the terminal before ours runs.
    let mut terminal = ratatui::try_init()?;
    // The terminal answers on stdin, so ask before anything else reads it.
    let picker = cover::detect();
    // A terminal that doesn't understand a query may have printed it.
    execute!(std::io::stdout(), Clear(ClearType::All))?;
    // Mouse capture means selecting text needs Shift (Option in some macOS
    // terminals), so it can be turned off.
    let mouse = std::env::var_os("FIREWOOD_NO_MOUSE").is_none();
    if mouse {
        execute!(std::io::stdout(), EnableMouseCapture)?;
    }
    execute!(std::io::stdout(), EnableBracketedPaste)?;
    let stderr = StderrRedirect::to_file(&log_dir.join("stderr.log"));
    let result = run(&mut terminal, picker).await;
    drop(stderr);
    let _ = execute!(
        std::io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste
    );
    ratatui::restore();

    if let Err(err) = &result {
        tracing::error!("exiting with error: {err:#}");
    }
    result
}

async fn run(terminal: &mut DefaultTerminal, picker: Picker) -> Result<()> {
    let Some(session) = authenticate(terminal).await? else {
        return Ok(());
    };

    let auth = &session.auth_state;
    let api: Arc<dyn PinepodsApi> = Arc::new(HttpApi::new(
        build_client()?,
        &auth.server_name,
        auth.api_key.clone(),
        i64::from(auth.user_details.UserID),
    ));

    let settings = SettingsManager::new().unwrap_or_else(|e| {
        tracing::warn!("couldn't load settings, using defaults: {e:#}");
        SettingsManager::with_defaults()
    });
    let playback = Playback::start(
        api.clone(),
        build_media_client()?,
        PlaybackConfig {
            device: settings.selected_audio_device(),
            volume: settings.default_volume(),
        },
    )
    .context("couldn't start audio")?;
    let artwork = Artwork::new(build_client()?, &auth.server_name);
    if settings.media_keys() {
        mediakeys::start(
            playback.clone(),
            mediakeys::Config {
                skip: Duration::from_secs(settings.skip_interval()),
                artwork: Some(artwork.clone()),
            },
        );
    }
    let remote = NowPlaying::start(
        nowplaying::Config {
            server: auth.server_name.clone(),
            api_key: auth.api_key.clone(),
            user_id: i64::from(auth.user_details.UserID),
            device_name: settings.device_name(),
        },
        api.clone(),
        playback.clone(),
    );

    let covers = Covers::new(artwork, picker);
    let app = App::new(
        &session,
        api,
        playback.clone(),
        remote.clone(),
        settings,
        covers,
    );
    let terminated = Arc::new(AtomicBool::new(false));
    watch_for_termination(terminated.clone(), playback.clone(), remote.clone());
    let result = run_main_app(terminal, app, &terminated).await;
    tokio::join!(remote.shutdown(), playback.shutdown());
    result
}

/// Returns `None` if the user quits from the login screen. Requests run in the
/// background, so this draws on time while the server is slow to answer.
async fn authenticate(terminal: &mut DefaultTerminal) -> Result<Option<SessionInfo>> {
    const FRAME: Duration = Duration::from_millis(50);
    let mut login = LoginTui::new()?;
    loop {
        if let Some(session) = login.tick() {
            return Ok(Some(session));
        }
        terminal.draw(|f| login.render(f))?;
        if !tokio::task::block_in_place(|| event::poll(FRAME))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                // Every printable key belongs to the input fields, so quitting is
                // Ctrl+C only.
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(None);
                }
                login.key(key);
            }
            Event::Paste(text) => login.paste(&text),
            _ => {}
        }
    }
}

/// Draws every frame on time: nothing in the app waits on the network, so a frame
/// is due every [`FRAME`] whether or not input arrived.
async fn run_main_app(
    terminal: &mut DefaultTerminal,
    mut app: App,
    terminated: &AtomicBool,
) -> Result<()> {
    const FRAME: Duration = Duration::from_millis(50);
    let mut next_frame = Instant::now();

    while !terminated.load(Ordering::Relaxed) && !app.should_quit() {
        if Instant::now() >= next_frame {
            app.tick();
            draw(terminal, &mut app)?;
            next_frame = Instant::now() + FRAME;
        }
        // Waiting here blocks this worker thread; the runtime has others for the
        // network and the socket.
        let timeout = next_frame.saturating_duration_since(Instant::now());
        if !tokio::task::block_in_place(|| event::poll(timeout))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => app.key(key),
            Event::Mouse(mouse) => app.mouse(mouse),
            Event::Paste(text) => app.paste(&text),
            // Redraw at the new size right away.
            Event::Resize(..) => next_frame = Instant::now(),
            _ => {}
        }
        // Show the effect of input without waiting for the next frame.
        if Instant::now() + Duration::from_millis(10) < next_frame {
            app.tick();
            draw(terminal, &mut app)?;
        }
    }
    Ok(())
}

fn draw(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    terminal.draw(|f| app.render(f))?;
    if app.take_repaint() {
        // Everything again, not just what changed. `Terminal::clear` would do this
        // too, but it first asks the terminal where the cursor is and waits for the
        // answer.
        execute!(terminal.backend_mut(), Clear(ClearType::All))?;
        terminal.swap_buffers();
        terminal.draw(|f| app.render(f))?;
    }
    Ok(())
}

/// Signals should still save the position rather than kill the process outright.
///
/// SIGTERM (`kill`, a service manager) ends the main loop so the terminal is restored
/// normally. SIGHUP means the terminal is gone: crossterm then spins on a dead tty
/// instead of returning from `poll`, so the loop would never notice a flag. Save and
/// exit from here instead. That also covers a terminal that closes right after
/// SIGTERM, and a loop that hasn't wound down a few seconds after it.
fn watch_for_termination(flag: Arc<AtomicBool>, playback: Playback, remote: NowPlaying) {
    #[cfg(unix)]
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
        const GRACE: Duration = Duration::from_secs(5);
        let (Ok(mut term), Ok(mut hup)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::hangup()),
        ) else {
            tracing::warn!("couldn't install signal handlers");
            return;
        };
        tokio::select! {
            _ = term.recv() => {
                tracing::info!("received SIGTERM");
                flag.store(true, Ordering::Relaxed);
                tokio::select! {
                    _ = hup.recv() => tracing::info!("terminal closed while exiting"),
                    _ = tokio::time::sleep(GRACE) => tracing::warn!("still running after SIGTERM"),
                }
            }
            _ = hup.recv() => tracing::info!("terminal closed; saving and exiting"),
        }
        tokio::join!(remote.shutdown(), playback.shutdown());
        std::process::exit(0);
    });
    #[cfg(not(unix))]
    let _ = (flag, playback, remote);
}
