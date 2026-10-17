use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

use pinepods_firewood::api::http::{build_client, build_media_client};
use pinepods_firewood::api::{HttpApi, PinepodsApi};
use pinepods_firewood::auth::{LoginTui, SessionInfo};
use pinepods_firewood::nowplaying::{self, NowPlaying};
use pinepods_firewood::paths;
use pinepods_firewood::playback::{Playback, PlaybackConfig};
use pinepods_firewood::settings::SettingsManager;
use pinepods_firewood::stderr::StderrRedirect;
use pinepods_firewood::tui::TuiApp;

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

#[tokio::main]
async fn main() -> Result<()> {
    let _log_guard = setup_logging()?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting");

    let log_dir = paths::log_dir()?;
    let crash_note = format!("Firewood crashed. Details are in {}", log_dir.display());
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        // stderr is redirected while the TUI runs, so say it on stdout.
        println!("{crash_note}");
    }));

    // Installs a panic hook of its own that restores the terminal before ours runs.
    let mut terminal = ratatui::try_init()?;
    let stderr = StderrRedirect::to_file(&log_dir.join("stderr.log"));
    let result = run(&mut terminal).await;
    drop(stderr);
    ratatui::restore();

    if let Err(err) = &result {
        tracing::error!("exiting with error: {err:#}");
    }
    result
}

async fn run(terminal: &mut DefaultTerminal) -> Result<()> {
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

    let mut app = TuiApp::new(session, api, playback.clone(), remote.clone(), settings);
    if let Err(e) = app.initialize().await {
        tracing::warn!("couldn't load the library: {e:#}");
        app.show_error_message(&format!("Couldn't load your library: {e}"));
    }
    let terminated = Arc::new(AtomicBool::new(false));
    watch_for_termination(terminated.clone(), playback.clone(), remote.clone());
    let result = run_main_app(terminal, app, &terminated).await;
    tokio::join!(remote.shutdown(), playback.shutdown());
    result
}

/// Returns `None` if the user quits from the login screen.
async fn authenticate(terminal: &mut DefaultTerminal) -> Result<Option<SessionInfo>> {
    let mut login = LoginTui::new()?;
    if let Some(session) = login.check_existing_auth().await {
        return Ok(Some(session));
    }

    loop {
        terminal.draw(|f| login.render(f, f.area()))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        // Every printable key belongs to the input fields, so quitting is Ctrl+C only.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Ok(None);
        }
        if let Some(session) = login.handle_input(key).await? {
            return Ok(Some(session));
        }
    }
}

async fn run_main_app(
    terminal: &mut DefaultTerminal,
    mut app: TuiApp,
    terminated: &AtomicBool,
) -> Result<()> {
    let tick_rate = Duration::from_millis(50);
    let mut last_tick = Instant::now();

    while !terminated.load(Ordering::Relaxed) {
        terminal.draw(|f| app.render(f))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.handle_input(key).await?;
            if app.should_quit() {
                break;
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.update().await?;
            last_tick = Instant::now();
        }
    }
    Ok(())
}

/// Signals should still save the position rather than kill the process outright.
///
/// SIGTERM (`kill`, a service manager) ends the main loop so the terminal is restored
/// normally. SIGHUP means the terminal is gone: crossterm then spins on a dead tty
/// instead of returning from `poll`, so the loop would never notice a flag. Save and
/// exit from here instead.
fn watch_for_termination(flag: Arc<AtomicBool>, playback: Playback, remote: NowPlaying) {
    #[cfg(unix)]
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
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
            }
            _ = hup.recv() => {
                tracing::info!("terminal closed; saving and exiting");
                tokio::join!(remote.shutdown(), playback.shutdown());
                std::process::exit(0);
            }
        }
    });
    #[cfg(not(unix))]
    let _ = (flag, playback, remote);
}
