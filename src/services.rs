//! Everything a logged-in Firewood runs, with or without the TUI on top: the API
//! client, the player, media keys and the now-playing socket.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::api::http::{build_client, build_media_client};
use crate::api::{HttpApi, PinepodsApi};
use crate::art::Artwork;
use crate::auth::AuthState;
use crate::mediakeys;
use crate::nowplaying::{self, NowPlaying};
use crate::playback::{Playback, PlaybackConfig};
use crate::settings::SettingsManager;

pub struct Services {
    pub api: Arc<dyn PinepodsApi>,
    pub playback: Playback,
    pub remote: NowPlaying,
    pub artwork: Artwork,
    pub settings: SettingsManager,
}

impl Services {
    /// Opens the audio device and starts the background tasks. Must be called inside
    /// a Tokio runtime.
    pub fn start(auth: &AuthState) -> Result<Self> {
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

        Ok(Self {
            api,
            playback,
            remote,
            artwork,
            settings,
        })
    }

    /// Saves the position and closes the socket so other devices drop this one now.
    pub async fn shutdown(&self) {
        tokio::join!(self.remote.shutdown(), self.playback.shutdown());
    }
}
