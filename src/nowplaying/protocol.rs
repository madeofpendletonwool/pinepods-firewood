//! Messages on the now-playing socket. The server's module doc
//! (`rust-api/src/handlers/nowplaying.rs`) is the source of truth.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::EpisodeKey;

/// Close code the server sends when a newer connection took over our `device_id`.
pub const CLOSE_REPLACED: u16 = 4000;

/// Action names this player applies, sent as the `capabilities` query parameter so
/// other clients can hide controls a device lacks.
pub const CAPABILITIES: &[&str] = &[
    "play",
    "pause",
    "seek",
    "skip_forward",
    "skip_back",
    "set_speed",
    "play_episode",
];

const DEFAULT_SKIP_FORWARD: f64 = 30.0;
const DEFAULT_SKIP_BACK: f64 = 15.0;

/// One of the user's players, as the server last heard from it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Device {
    pub device_id: String,
    pub device_name: String,
    pub device_type: String,
    /// 0 when the device is idle.
    pub episode_id: i64,
    pub is_youtube: bool,
    pub title: String,
    pub artwork_url: String,
    pub position_sec: f64,
    pub duration_sec: f64,
    pub playing: bool,
    pub speed: f64,
    /// Unix seconds, stamped by the server.
    pub updated_at: i64,
}

impl Device {
    /// What it's playing, if anything.
    pub fn episode(&self) -> Option<EpisodeKey> {
        (self.episode_id > 0).then_some(EpisodeKey {
            episode_id: self.episode_id,
            is_youtube: self.is_youtube,
        })
    }

    /// A name to show, falling back to the type for clients that send no name.
    pub fn label(&self) -> &str {
        [&self.device_name, &self.device_type]
            .into_iter()
            .find(|s| !s.is_empty())
            .map_or("Unnamed device", |s| s.as_str())
    }
}

/// This player's state, sent on every change and periodically while playing.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Report {
    pub episode_id: i64,
    pub is_youtube: bool,
    pub title: String,
    pub artwork_url: String,
    pub position_sec: f64,
    pub duration_sec: f64,
    pub playing: bool,
    pub speed: f64,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outgoing {
    Report(Report),
    Heartbeat,
    Command {
        target_device_id: String,
        action: &'static str,
        args: Value,
        cmd_id: String,
    },
}

impl Outgoing {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("outgoing messages always serialize")
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Incoming {
    Devices {
        #[serde(default)]
        devices: Vec<Device>,
    },
    Command {
        #[serde(default)]
        from_device_id: String,
        action: String,
        #[serde(default)]
        args: Value,
    },
    /// Whether the server could deliver a command we sent. Delivery isn't execution.
    Ack {
        ok: bool,
        #[serde(default)]
        detail: String,
        #[serde(default)]
        cmd_id: Option<String>,
        #[serde(default)]
        target_device_id: String,
    },
    /// Newer servers may add message types; ignore them.
    #[serde(other)]
    Unknown,
}

/// A remote-control action, in either direction.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Play,
    Pause,
    Seek(Duration),
    SkipForward(Duration),
    SkipBack(Duration),
    SetSpeed(f32),
    /// Start an episode; with a position, resume exactly there (a handoff),
    /// otherwise from the position the server has saved.
    PlayEpisode {
        episode: EpisodeKey,
        position: Option<Duration>,
    },
}

impl Action {
    /// `None` for actions this player doesn't know and for arguments that make no
    /// sense, which other clients may send and which are ignored.
    pub fn parse(action: &str, args: &Value) -> Option<Self> {
        let num = |name: &str| args.get(name).and_then(Value::as_f64);
        Some(match action {
            "play" => Self::Play,
            "pause" => Self::Pause,
            "seek" => Self::Seek(secs(num("position_sec")?)?),
            "skip_forward" => {
                Self::SkipForward(secs(num("seconds").unwrap_or(DEFAULT_SKIP_FORWARD))?)
            }
            "skip_back" => Self::SkipBack(secs(num("seconds").unwrap_or(DEFAULT_SKIP_BACK))?),
            "set_speed" => {
                let speed = num("speed").filter(|s| s.is_finite() && *s > 0.0)?;
                Self::SetSpeed(speed as f32)
            }
            "play_episode" => {
                let episode_id = args.get("episode_id").and_then(Value::as_i64)?;
                if episode_id <= 0 {
                    return None;
                }
                let is_youtube = args
                    .get("is_youtube")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Self::PlayEpisode {
                    episode: EpisodeKey {
                        episode_id,
                        is_youtube,
                    },
                    // 0 is what a sender with no position would mean anyway.
                    position: num("position_sec").and_then(secs).filter(|p| !p.is_zero()),
                }
            }
            _ => return None,
        })
    }

    pub fn to_wire(&self) -> (&'static str, Value) {
        match self {
            Self::Play => ("play", json!({})),
            Self::Pause => ("pause", json!({})),
            Self::Seek(to) => ("seek", json!({ "position_sec": to.as_secs_f64() })),
            Self::SkipForward(by) => ("skip_forward", json!({ "seconds": by.as_secs_f64() })),
            Self::SkipBack(by) => ("skip_back", json!({ "seconds": by.as_secs_f64() })),
            Self::SetSpeed(speed) => ("set_speed", json!({ "speed": f64::from(*speed) })),
            Self::PlayEpisode { episode, position } => {
                let mut args = json!({
                    "episode_id": episode.episode_id,
                    "is_youtube": episode.is_youtube,
                });
                if let Some(position) = position {
                    args["position_sec"] = json!(position.as_secs_f64());
                }
                ("play_episode", args)
            }
        }
    }
}

/// Seconds from another client, which may be negative or not a number.
fn secs(value: f64) -> Option<Duration> {
    value
        .is_finite()
        .then(|| Duration::from_secs_f64(value.max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_matches_the_server_format() {
        let report = Outgoing::Report(Report {
            episode_id: 123,
            is_youtube: false,
            title: "Ep".into(),
            artwork_url: "https://a/b.jpg".into(),
            position_sec: 812.5,
            duration_sec: 3600.0,
            playing: true,
            speed: 1.25,
        });
        let value: Value = serde_json::from_str(&report.to_json()).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "report", "episode_id": 123, "is_youtube": false, "title": "Ep",
                "artwork_url": "https://a/b.jpg", "position_sec": 812.5,
                "duration_sec": 3600.0, "playing": true, "speed": 1.25
            })
        );
        assert_eq!(Outgoing::Heartbeat.to_json(), r#"{"type":"heartbeat"}"#);
    }

    #[test]
    fn command_carries_its_id_and_args() {
        let (action, args) = Action::Pause.to_wire();
        let msg = Outgoing::Command {
            target_device_id: "web-1".into(),
            action,
            args,
            cmd_id: "7".into(),
        };
        let value: Value = serde_json::from_str(&msg.to_json()).unwrap();
        assert_eq!(
            value,
            json!({"type": "command", "target_device_id": "web-1", "action": "pause",
                   "args": {}, "cmd_id": "7"})
        );
    }

    #[test]
    fn parses_server_messages() {
        let devices: Incoming = serde_json::from_str(
            r#"{"type":"devices","devices":[{"device_id":"a","device_name":"Kitchen",
                "device_type":"Web","episode_id":5,"is_youtube":false,"title":"T",
                "artwork_url":"","position_sec":10.5,"duration_sec":60.0,
                "playing":true,"speed":1.0,"updated_at":1760000000}]}"#,
        )
        .unwrap();
        let Incoming::Devices { devices } = devices else {
            panic!("expected devices");
        };
        assert_eq!(devices[0].label(), "Kitchen");
        assert_eq!(devices[0].episode().unwrap().episode_id, 5);

        let ack: Incoming = serde_json::from_str(
            r#"{"type":"ack","ok":false,"detail":"device offline","cmd_id":"3",
                "target_device_id":"b"}"#,
        )
        .unwrap();
        assert_eq!(
            ack,
            Incoming::Ack {
                ok: false,
                detail: "device offline".into(),
                cmd_id: Some("3".into()),
                target_device_id: "b".into(),
            }
        );

        let command: Incoming = serde_json::from_str(
            r#"{"type":"command","from_device_id":"a","action":"seek","args":{"position_sec":90}}"#,
        )
        .unwrap();
        let Incoming::Command { action, args, .. } = command else {
            panic!("expected command");
        };
        assert_eq!(
            Action::parse(&action, &args),
            Some(Action::Seek(Duration::from_secs(90)))
        );
    }

    #[test]
    fn tolerates_unknown_and_sparse_messages() {
        let unknown: Incoming = serde_json::from_str(r#"{"type":"hello","x":1}"#).unwrap();
        assert_eq!(unknown, Incoming::Unknown);

        // An idle device from an older client with only the required fields.
        let devices: Incoming = serde_json::from_str(
            r#"{"type":"devices","devices":[{"device_id":"x","device_name":"",
                "device_type":"Android","episode_id":0}]}"#,
        )
        .unwrap();
        let Incoming::Devices { devices } = devices else {
            panic!("expected devices");
        };
        assert_eq!(devices[0].episode(), None);
        assert_eq!(devices[0].label(), "Android");
    }

    #[test]
    fn skips_default_like_the_other_clients() {
        let none = json!({});
        assert_eq!(
            Action::parse("skip_forward", &none),
            Some(Action::SkipForward(Duration::from_secs(30)))
        );
        assert_eq!(
            Action::parse("skip_back", &none),
            Some(Action::SkipBack(Duration::from_secs(15)))
        );
        assert_eq!(
            Action::parse("skip_back", &json!({"seconds": 10})),
            Some(Action::SkipBack(Duration::from_secs(10)))
        );
    }

    #[test]
    fn rejects_unknown_actions_and_nonsense_args() {
        assert_eq!(Action::parse("stop", &json!({})), None);
        assert_eq!(Action::parse("seek", &json!({})), None);
        assert_eq!(Action::parse("seek", &json!({"position_sec": "10"})), None);
        assert_eq!(Action::parse("set_speed", &json!({"speed": 0})), None);
        assert_eq!(
            Action::parse("play_episode", &json!({"episode_id": 0})),
            None
        );
        assert_eq!(
            Action::parse("seek", &json!({"position_sec": -5})),
            Some(Action::Seek(Duration::ZERO))
        );
    }

    #[test]
    fn play_episode_round_trips_with_and_without_a_position() {
        let episode = EpisodeKey {
            episode_id: 42,
            is_youtube: true,
        };
        for position in [None, Some(Duration::from_secs_f64(812.5))] {
            let action = Action::PlayEpisode { episode, position };
            let (name, args) = action.to_wire();
            assert_eq!(Action::parse(name, &args), Some(action));
        }
        // Mobile sends whole seconds as integers.
        assert_eq!(
            Action::parse(
                "play_episode",
                &json!({"episode_id": 42, "is_youtube": true, "position_sec": 0})
            ),
            Some(Action::PlayEpisode {
                episode,
                position: None
            })
        );
    }
}
