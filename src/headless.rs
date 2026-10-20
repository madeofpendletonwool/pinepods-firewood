//! `--headless`: the player and the now-playing socket with no terminal, so a
//! Raspberry Pi or a speaker box can sit there as a device other PinePods clients
//! play on and control.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::api::ApiError;
use crate::auth::{AuthState, LoginFlow, Restored};
use crate::nowplaying;
use crate::services::Services;

pub const SERVER_ENV: &str = "FIREWOOD_SERVER";
pub const API_KEY_ENV: &str = "FIREWOOD_API_KEY";
pub const API_KEY_FILE_ENV: &str = "FIREWOOD_API_KEY_FILE";

/// A box that boots before its network, or before the server, keeps trying.
const RETRY_MIN: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(60);

pub async fn run() -> Result<()> {
    if nowplaying::disabled_by_env() {
        bail!(
            "{} turns off remote control, which is all a headless player is for",
            nowplaying::DISABLE_ENV
        );
    }
    let stop = shutdown_signal();
    tokio::pin!(stop);

    let auth = tokio::select! {
        auth = login() => auth?,
        _ = &mut stop => return Ok(()),
    };
    let services = Services::start(&auth)?;
    tracing::info!(
        user_id = auth.user_details.UserID,
        device = %services.remote.state().device_name,
        "headless player running"
    );

    stop.await;
    services.shutdown().await;
    Ok(())
}

/// Credentials in the environment win over the saved session, so a service file can
/// say exactly which account the box plays for.
async fn login() -> Result<AuthState> {
    let flow = LoginFlow::new()?;
    if let Some((server, api_key)) = env_credentials()? {
        tracing::info!("logging in with the key from the environment");
        return account_with_retry(flow, server, api_key).await;
    }
    match flow.restore_session().await {
        Restored::Session(session) => {
            tracing::info!("using the saved session");
            Ok(session.auth_state)
        }
        Restored::None => bail!(
            "not logged in: run pinepods_firewood once without --headless to log in, \
             or set {SERVER_ENV} and {API_KEY_ENV}"
        ),
        Restored::Rejected { .. } => bail!(
            "the server rejected the saved API key: log in again without --headless, \
             or set {SERVER_ENV} and {API_KEY_ENV}"
        ),
    }
}

async fn account_with_retry(flow: LoginFlow, server: String, api_key: String) -> Result<AuthState> {
    let mut delay = RETRY_MIN;
    loop {
        match flow
            .clone()
            .account_for_key(server.clone(), api_key.clone())
            .await
        {
            Ok(auth) => return Ok(auth),
            // A refused key, or an answer that isn't PinePods', won't change on retry.
            // Anything else (no network yet, a proxy's 502 while the server starts)
            // might.
            Err(e)
                if e.downcast_ref::<ApiError>()
                    .is_some_and(|e| !e.is_transient()) =>
            {
                return Err(e.context("couldn't log in"));
            }
            Err(e) => {
                tracing::warn!("couldn't log in, retrying in {}s: {e:#}", delay.as_secs());
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(RETRY_MAX);
            }
        }
    }
}

fn env_credentials() -> Result<Option<(String, String)>> {
    let server = env_value(SERVER_ENV);
    let api_key = match (env_value(API_KEY_ENV), env_value(API_KEY_FILE_ENV)) {
        (Some(key), _) => Some(key),
        (None, Some(path)) => Some(read_key_file(Path::new(&path))?),
        (None, None) => None,
    };
    match (server, api_key) {
        (Some(server), Some(api_key)) => Ok(Some((server, api_key))),
        (None, None) => Ok(None),
        (Some(_), None) => {
            bail!("{SERVER_ENV} is set, but neither {API_KEY_ENV} nor {API_KEY_FILE_ENV} is")
        }
        (None, Some(_)) => bail!("an API key is set, but {SERVER_ENV} isn't"),
    }
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn read_key_file(path: &Path) -> Result<String> {
    let key = std::fs::read_to_string(path)
        .with_context(|| format!("couldn't read {API_KEY_FILE_ENV} ({})", path.display()))?;
    let key = key.trim();
    if key.is_empty() {
        bail!("{API_KEY_FILE_ENV} ({}) is empty", path.display());
    }
    Ok(key.to_string())
}

/// SIGTERM is how a service manager stops us, SIGINT is Ctrl+C when run by hand, and
/// SIGHUP is the terminal going away. There's nothing to reload, so each one means
/// save and exit.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
            signal(SignalKind::hangup()),
        ) {
            (Ok(mut term), Ok(mut int), Ok(mut hup)) => {
                let name = tokio::select! {
                    _ = term.recv() => "SIGTERM",
                    _ = int.recv() => "SIGINT",
                    _ = hup.recv() => "SIGHUP",
                };
                tracing::info!("received {name}; saving and exiting");
                return;
            }
            _ => tracing::warn!("couldn't install signal handlers; only Ctrl+C will stop"),
        }
    }
    if tokio::signal::ctrl_c().await.is_err() {
        // Nothing can stop us, so wait for the process to be killed.
        std::future::pending::<()>().await;
    }
    tracing::info!("interrupted; saving and exiting");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_file_is_trimmed() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "  abc123\n").unwrap();
        assert_eq!(read_key_file(file.path()).unwrap(), "abc123");
    }

    #[test]
    fn empty_key_file_is_an_error() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "\n").unwrap();
        assert!(read_key_file(file.path()).is_err());
    }
}
