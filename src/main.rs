use std::process::ExitCode;
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
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use pinepods_firewood::auth::{LoginTui, SessionInfo};
use pinepods_firewood::cli::{self, Command, Mode};
use pinepods_firewood::headless;
use pinepods_firewood::mediakeys;
use pinepods_firewood::nowplaying::NowPlaying;
use pinepods_firewood::paths;
use pinepods_firewood::playback::Playback;
use pinepods_firewood::services::Services;
use pinepods_firewood::stderr::StderrRedirect;
use pinepods_firewood::tui::App;
use pinepods_firewood::tui::ui::cover::{self, Covers};
use ratatui_image::picker::Picker;

const LOG_FILES_KEPT: usize = 7;

/// Logs go to a daily file. `FIREWOOD_LOG` takes an `EnvFilter` directive
/// (`debug`, `pinepods_firewood=trace`, …); the default is `info`, minus the MP3
/// decoder's warnings about resyncing, which every seek into an MP3 stream produces.
/// Headless, they go to stderr too, where a service manager's journal picks them up,
/// and a log file that can't be opened leaves just stderr.
fn setup_logging(mode: Mode) -> Result<Option<WorkerGuard>> {
    let headless = mode == Mode::Headless;
    let appender = paths::log_dir()
        .map_err(anyhow::Error::from)
        .and_then(|dir| {
            RollingFileAppender::builder()
                .rotation(Rotation::DAILY)
                .filename_prefix("firewood")
                .filename_suffix("log")
                .max_log_files(LOG_FILES_KEPT)
                .build(dir)
                .map_err(anyhow::Error::from)
        })
        .context("couldn't open the log file");
    let (file, guard, file_error) = match appender {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let layer = tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false);
            (Some(layer), Some(guard), None)
        }
        Err(err) if headless => (None, None, Some(err)),
        Err(err) => return Err(err),
    };
    let filter = EnvFilter::try_from_env("FIREWOOD_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,symphonia_bundle_mp3=error"));
    let stderr = headless.then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(false)
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(file)
        .with(stderr)
        .init();
    if let Some(err) = file_error {
        tracing::warn!("logging to stderr only: {err:#}");
    }
    Ok(guard)
}

fn main() -> Result<ExitCode> {
    let mode = match cli::parse(std::env::args_os().skip(1)) {
        Ok(Command::Run(mode)) => mode,
        Ok(Command::Version) => {
            println!("pinepods_firewood {}", env!("CARGO_PKG_VERSION"));
            return Ok(ExitCode::SUCCESS);
        }
        Ok(Command::Help) => {
            print!("{}", cli::USAGE);
            return Ok(ExitCode::SUCCESS);
        }
        Err(err) => {
            eprint!("{err}\n\n{}", cli::USAGE);
            return Ok(ExitCode::from(2));
        }
    };
    // On macOS the media keys need the main thread, so Firewood runs on another.
    let result = mediakeys::run_with_main_loop(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("couldn't start the async runtime")?
            .block_on(app_main(mode))
    });
    match result {
        // Headless logs go to stderr, so the error is there already.
        Err(_) if mode == Mode::Headless => Ok(ExitCode::FAILURE),
        result => result.map(|()| ExitCode::SUCCESS),
    }
}

async fn app_main(mode: Mode) -> Result<()> {
    let _log_guard = setup_logging(mode)?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), ?mode, "starting");

    if mode == Mode::Headless {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            tracing::error!("panic: {info}");
            default_hook(info);
        }));
        let result = headless::run().await;
        if let Err(err) = &result {
            tracing::error!("exiting with error: {err:#}");
        }
        return result;
    }

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

    let Services {
        api,
        playback,
        remote,
        artwork,
        settings,
    } = Services::start(&session.auth_state)?;

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
