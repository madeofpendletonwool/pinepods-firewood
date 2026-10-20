//! MPRIS over D-Bus, which desktop media widgets, `playerctl` and the media keys
//! of most Linux desktops talk to.
//!
//! The server is a single-threaded future, so it runs on this thread's own small
//! runtime rather than Firewood's.

use std::time::Duration;

use mpris_server::{Metadata, PlaybackStatus, Player, Time, TrackId};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{Control, Snapshot, Status, Update};

const IDENTITY: &str = "Firewood";
const MIN_RATE: f64 = 0.5;
const MAX_RATE: f64 = 3.0;

pub fn serve(
    controls: UnboundedSender<Control>,
    updates: UnboundedReceiver<Update>,
    skip: Duration,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            tracing::warn!("media keys are off: couldn't start their runtime ({e})");
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, run(controls, updates, skip));
}

async fn run(
    controls: UnboundedSender<Control>,
    mut updates: UnboundedReceiver<Update>,
    _skip: Duration,
) {
    // One name per process, as MPRIS suggests for players that can run twice.
    let name = format!("firewood.instance{}", std::process::id());
    let player = match Player::builder(&name)
        .identity(IDENTITY)
        .can_play(true)
        .can_pause(true)
        .can_seek(true)
        .can_go_next(true)
        .can_go_previous(true)
        .can_control(true)
        .minimum_rate(MIN_RATE)
        .maximum_rate(MAX_RATE)
        .build()
        .await
    {
        Ok(player) => player,
        Err(e) => {
            tracing::warn!("media keys are off: couldn't register with D-Bus ({e})");
            return;
        }
    };
    tracing::info!("media keys: registered as org.mpris.MediaPlayer2.{name}");

    let send = move |control: Control| {
        let controls = controls.clone();
        move || {
            let _ = controls.send(control);
        }
    };
    let on = |control| {
        let send = send(control);
        move |_: &Player| send()
    };
    player.connect_play(on(Control::Play));
    player.connect_pause(on(Control::Pause));
    player.connect_play_pause(on(Control::Toggle));
    player.connect_stop(on(Control::Stop));
    player.connect_next(on(Control::Next));
    player.connect_previous(on(Control::Previous));
    {
        let send = send.clone();
        player.connect_seek(move |_, offset| {
            let by = Duration::from_micros(offset.as_micros().unsigned_abs());
            let control = if offset.is_negative() {
                Control::Backward(Some(by))
            } else {
                Control::Forward(Some(by))
            };
            send(control)();
        });
    }
    player.connect_set_position(move |_, _track, at| {
        if !at.is_negative() {
            send(Control::SetPosition(Duration::from_micros(
                at.as_micros() as u64
            )))();
        }
    });
    tokio::task::spawn_local(player.run());

    let mut warned = false;
    while let Some(update) = updates.recv().await {
        if let Err(e) = show(&player, &update).await {
            if warned {
                tracing::debug!("couldn't update MPRIS: {e}");
            } else {
                tracing::warn!("couldn't update MPRIS: {e}");
                warned = true;
            }
        }
    }
}

async fn show(player: &Player, update: &Update) -> mpris_server::zbus::Result<()> {
    let now = &update.now;
    // Clients read the position whenever they like, so it's always kept current.
    player.set_position(time(now.position));
    if update.changed {
        player.set_metadata(metadata(now)).await?;
        player.set_rate(now.rate.clamp(MIN_RATE, MAX_RATE)).await?;
        player
            .set_playback_status(match now.status {
                Status::Playing => PlaybackStatus::Playing,
                Status::Paused => PlaybackStatus::Paused,
                Status::Stopped => PlaybackStatus::Stopped,
            })
            .await?;
    }
    if update.jumped || update.changed {
        player.seeked(time(now.position)).await?;
    }
    Ok(())
}

fn metadata(now: &Snapshot) -> Metadata {
    let Some(track) = &now.track else {
        return Metadata::builder().trackid(TrackId::NO_TRACK).build();
    };
    let trackid = TrackId::try_from(format!("/org/pinepods/firewood/episode/{}", track.id))
        .unwrap_or(TrackId::NO_TRACK);
    let mut builder = Metadata::builder()
        .trackid(trackid)
        .title(track.title.clone())
        .artist([track.podcast.clone()])
        .album(track.podcast.clone())
        .length(time(track.duration));
    let art = match &track.artwork_file {
        Some(file) => Some(file_url(file)),
        None if !track.artwork_url.is_empty() => Some(track.artwork_url.clone()),
        None => None,
    };
    if let Some(art) = art {
        builder = builder.art_url(art);
    }
    builder.build()
}

fn time(duration: Duration) -> Time {
    Time::from_micros(i64::try_from(duration.as_micros()).unwrap_or(i64::MAX))
}

/// A `file://` URL for `path`, with each part percent-encoded.
fn file_url(path: &std::path::Path) -> String {
    let encoded: Vec<String> = path
        .to_string_lossy()
        .split('/')
        .map(|part| urlencoding::encode(part).into_owned())
        .collect();
    format!("file://{}", encoded.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Needs a D-Bus session and `playerctl`:
    /// `dbus-run-session -- cargo test -- --ignored mpris`.
    #[test]
    #[ignore]
    fn playerctl_sees_the_episode_and_controls_it() {
        let (controls_tx, mut controls) = tokio::sync::mpsc::unbounded_channel();
        let (updates, updates_rx) = tokio::sync::mpsc::unbounded_channel();
        std::thread::spawn(move || serve(controls_tx, updates_rx, Duration::from_secs(15)));
        let now = Snapshot {
            track: Some(super::super::Track {
                id: "42".into(),
                title: "Pilot".into(),
                podcast: "Show".into(),
                artwork_url: "https://example.com/a.jpg".into(),
                artwork_file: None,
                duration: Duration::from_secs(600),
            }),
            status: Status::Playing,
            position: Duration::from_secs(90),
            rate: 1.5,
        };
        updates
            .send(Update {
                now,
                changed: true,
                jumped: false,
            })
            .unwrap();
        std::thread::sleep(Duration::from_secs(1));

        let playerctl = |args: &[&str]| {
            let out = Command::new("playerctl")
                .args(["-p", "firewood"])
                .args(args)
                .output()
                .expect("playerctl runs");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        assert_eq!(playerctl(&["metadata", "title"]), "Pilot");
        assert_eq!(playerctl(&["metadata", "artist"]), "Show");
        assert_eq!(playerctl(&["status"]), "Playing");
        assert_eq!(playerctl(&["position"]).split('.').next(), Some("90"));
        assert_eq!(
            playerctl(&["metadata", "mpris:artUrl"]),
            "https://example.com/a.jpg"
        );

        playerctl(&["play-pause"]);
        playerctl(&["next"]);
        playerctl(&["position", "30+"]);
        playerctl(&["position", "120"]);
        let mut got = Vec::new();
        while got.len() < 4 {
            got.push(controls.blocking_recv().expect("a control"));
        }
        assert_eq!(
            got,
            [
                Control::Toggle,
                Control::Next,
                Control::Forward(Some(Duration::from_secs(30))),
                Control::SetPosition(Duration::from_secs(120)),
            ]
        );
    }

    #[test]
    fn file_urls_are_encoded() {
        assert_eq!(
            file_url(std::path::Path::new("/home/a b/c.jpg")),
            "file:///home/a%20b/c.jpg"
        );
    }
}
