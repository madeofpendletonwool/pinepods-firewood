//! Now-playing socket: tells the server what this player is doing, applies remote
//! control from the user's other devices, and lets Firewood control them in turn.
//!
//! One task owns the socket. It reads the player's state from [`Playback`] and sends
//! it commands, the same as a key press would; playback never waits on the socket,
//! so a slow or dead server costs remote control and nothing else. The protocol is
//! documented in the server's `rust-api/src/handlers/nowplaying.rs`.

pub mod device;
pub mod protocol;
mod report;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

use crate::api::http::normalize_server_url;
use crate::api::{Api, Episode, EpisodeKey};
use crate::playback::Playback;
use device::DeviceId;
use protocol::{Action, CAPABILITIES, CLOSE_REPLACED, Device, Incoming, Outgoing};
use report::{Periodic, ReportClock, report_of};

/// Set to `1` or `true` to run without the socket.
pub const DISABLE_ENV: &str = "FIREWOOD_REMOTE_DISABLED";

const DEVICE_TYPE: &str = "Firewood";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A send that can't get through in this long means the connection is dead.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
/// The server pings every 30 s, so this long without a single frame means the
/// connection is dead even though TCP hasn't noticed (a frozen server, a dropped NAT
/// mapping, a laptop waking up). It's the server's own rule for its clients.
const SERVER_SILENCE: Duration = Duration::from_secs(75);
const BACKOFF_MIN: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// After the server hands our id to another connection, take a new id and come
/// back quickly; there's nothing wrong with the server.
const REPLACED_DELAY: Duration = Duration::from_secs(1);
/// Commands awaiting an ack. Acks come back in well under a second, so this only
/// bounds the list if a server never answers.
const MAX_PENDING: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Connection {
    /// Turned off with [`DISABLE_ENV`].
    Disabled,
    Connecting,
    Connected,
    /// Not connected; trying again in a while.
    Offline {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct Notice {
    pub text: String,
    pub is_error: bool,
    pub at: Instant,
}

/// What the UI shows about remote control.
#[derive(Debug, Clone)]
pub struct RemoteState {
    pub connection: Connection,
    pub device_name: String,
    /// The user's other devices; never this one.
    pub devices: Vec<Device>,
    /// When `devices` arrived, to advance the positions of the ones playing.
    pub received: Instant,
    /// The outcome of the last thing the user asked another device to do.
    pub notice: Option<Notice>,
}

impl RemoteState {
    /// Where a device is now, given that it reported `position_sec` when the list
    /// arrived.
    pub fn live_position(&self, device: &Device, now: Instant) -> Duration {
        let mut secs = device.position_sec.max(0.0);
        if device.playing {
            let speed = if device.speed > 0.0 {
                device.speed
            } else {
                1.0
            };
            secs += now.saturating_duration_since(self.received).as_secs_f64() * speed;
        }
        if device.duration_sec > 0.0 {
            secs = secs.min(device.duration_sec);
        }
        Duration::from_secs_f64(secs)
    }
}

pub struct Config {
    pub server: String,
    pub api_key: String,
    pub user_id: i64,
    /// `None` for the default, `Firewood · <hostname>`.
    pub device_name: Option<String>,
}

enum Request {
    Command {
        target: String,
        action: Action,
    },
    /// Hand what's playing here to `target`, pausing here once it has it.
    SendPlayback {
        target: String,
    },
    /// Take over what `source` is playing, from where it is now.
    PlayHere {
        source: Device,
        position: Duration,
    },
    Shutdown(oneshot::Sender<()>),
}

/// The handle the UI holds. Every method returns immediately.
#[derive(Clone)]
pub struct NowPlaying {
    requests: Option<mpsc::UnboundedSender<Request>>,
    state: watch::Receiver<RemoteState>,
}

impl NowPlaying {
    /// Starts the socket task, unless [`DISABLE_ENV`] turns it off. Must be called
    /// inside a Tokio runtime.
    pub fn start(config: Config, api: Api, playback: Playback) -> Self {
        let device_name = config
            .device_name
            .clone()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(device::default_name);
        if disabled_by_env() {
            tracing::info!("now-playing socket disabled by {DISABLE_ENV}");
            return Self::disabled(device_name);
        }

        let (state_tx, state) = watch::channel(RemoteState {
            connection: Connection::Connecting,
            device_name: device_name.clone(),
            devices: Vec::new(),
            received: Instant::now(),
            notice: None,
        });
        let (requests, requests_rx) = mpsc::unbounded_channel();
        let service = Service {
            config,
            device_name,
            device_id: DeviceId::load_or_create(),
            api,
            playback,
            state: state_tx,
            requests: requests_rx,
            tls: tls_config(),
            next_cmd: 0,
            pending: VecDeque::new(),
        };
        tokio::spawn(service.run());
        Self {
            requests: Some(requests),
            state,
        }
    }

    fn disabled(device_name: String) -> Self {
        let (_, state) = watch::channel(RemoteState {
            connection: Connection::Disabled,
            device_name,
            devices: Vec::new(),
            received: Instant::now(),
            notice: None,
        });
        Self {
            requests: None,
            state,
        }
    }

    pub fn state(&self) -> RemoteState {
        self.state.borrow().clone()
    }

    /// Sends a transport command to another device.
    pub fn command(&self, target: &str, action: Action) {
        self.send(Request::Command {
            target: target.to_string(),
            action,
        });
    }

    /// Starts an episode on another device from the position the server has saved.
    pub fn play_on(&self, target: &str, episode: EpisodeKey) {
        self.command(
            target,
            Action::PlayEpisode {
                episode,
                position: None,
            },
        );
    }

    /// Moves what's playing here to `target`. This player pauses only once the
    /// server confirms delivery, so an offline target doesn't leave you with silence.
    pub fn send_playback_to(&self, target: &str) {
        self.send(Request::SendPlayback {
            target: target.to_string(),
        });
    }

    /// Pauses `source` and continues its episode here from where it is now.
    pub fn play_here(&self, source: &Device) {
        let state = self.state.borrow();
        let position = state.live_position(source, Instant::now());
        drop(state);
        self.send(Request::PlayHere {
            source: source.clone(),
            position,
        });
    }

    /// Closes the socket so the server drops this device at once rather than after
    /// its timeout. Bounded, so quitting never hangs on a dead server.
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if let Some(requests) = &self.requests
            && requests.send(Request::Shutdown(tx)).is_ok()
        {
            let _ = tokio::time::timeout(CLOSE_TIMEOUT * 2, rx).await;
        }
    }

    fn send(&self, request: Request) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(request);
        }
    }
}

pub fn disabled_by_env() -> bool {
    std::env::var(DISABLE_ENV).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How a connection ended.
enum End {
    Shutdown,
    /// Another connection took our device id.
    Replaced,
    Lost(String),
}

#[derive(Debug, Clone, Copy)]
enum Pending {
    Transport,
    PlayOn,
    Handoff,
}

struct Service {
    config: Config,
    device_name: String,
    device_id: DeviceId,
    api: Api,
    playback: Playback,
    state: watch::Sender<RemoteState>,
    requests: mpsc::UnboundedReceiver<Request>,
    tls: Option<Arc<rustls::ClientConfig>>,
    next_cmd: u64,
    /// Commands sent and not yet acked, oldest first.
    pending: VecDeque<(String, String, Pending)>,
}

impl Service {
    async fn run(mut self) {
        let mut failures = 0u32;
        let mut last_reason = String::new();
        loop {
            self.state
                .send_modify(|s| s.connection = Connection::Connecting);
            let end = match self.connect().await {
                Ok(socket) => {
                    failures = 0;
                    last_reason.clear();
                    tracing::info!(host = %host_of(&self.config.server), "now-playing socket connected");
                    self.session(socket).await
                }
                Err(reason) => End::Lost(reason),
            };
            self.pending.clear();
            let delay = match end {
                End::Shutdown => return,
                End::Replaced => {
                    tracing::info!(
                        "another connection took this device id; switching to a new one"
                    );
                    self.device_id.rotate();
                    self.set_offline("reconnecting with a new device id".into());
                    REPLACED_DELAY
                }
                End::Lost(reason) => {
                    let delay = backoff(failures, random_unit());
                    failures = failures.saturating_add(1);
                    // Say it once at warn; a server that stays down shouldn't fill the log.
                    if reason != last_reason {
                        tracing::warn!(
                            "now-playing socket: {reason}; retrying in {}s",
                            delay.as_secs()
                        );
                    } else {
                        tracing::debug!("now-playing socket: {reason}");
                    }
                    self.set_offline(reason.clone());
                    last_reason = reason;
                    delay
                }
            };
            if !self.wait(delay).await {
                return;
            }
        }
    }

    fn set_offline(&self, reason: String) {
        self.state.send_modify(|s| {
            s.connection = Connection::Offline { reason };
            s.devices.clear();
        });
    }

    /// Waits out a reconnect delay while still answering the UI. False on shutdown.
    async fn wait(&mut self, delay: Duration) -> bool {
        let deadline = tokio::time::sleep(delay);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => return true,
                request = self.requests.recv() => match request {
                    None => return false,
                    Some(Request::Shutdown(done)) => {
                        let _ = done.send(());
                        return false;
                    }
                    Some(_) => self.notify("Not connected to the server", true),
                },
            }
        }
    }

    async fn connect(&self) -> Result<Socket, String> {
        let url = socket_url(
            &self.config.server,
            self.config.user_id,
            &self.config.api_key,
            self.device_id.as_str(),
            &self.device_name,
        );
        let connector = self.tls.clone().map(Connector::Rustls);
        let attempt = tokio_tungstenite::connect_async_tls_with_config(url, None, false, connector);
        match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
            Ok(Ok((socket, _))) => Ok(socket),
            Ok(Err(err)) => Err(self.redact(&describe_connect_error(&err))),
            Err(_) => Err("timed out connecting".into()),
        }
    }

    /// Runs one connection until it ends.
    async fn session(&mut self, socket: Socket) -> End {
        let (mut sink, mut stream) = socket.split();
        self.state
            .send_modify(|s| s.connection = Connection::Connected);

        let mut player = self.playback.subscribe();
        let mut player_alive = true;
        let mut clock = ReportClock::new(Instant::now());
        let report = report_of(&player.borrow_and_update());
        // The server already lists a new connection as idle.
        if report.episode_id == 0 {
            clock.assume(report, Instant::now());
        } else if let Err(end) = send(&mut sink, Outgoing::Report(report.clone())).await {
            return end;
        } else {
            clock.sent(report, Instant::now());
        }

        let mut heard = tokio::time::Instant::now();
        loop {
            let due = tokio::time::Instant::from_std(clock.due());
            tokio::select! {
                message = stream.next() => {
                    if let Some(Ok(_)) = &message {
                        heard = tokio::time::Instant::now();
                    }
                    if let Some(end) = self.on_frame(message) {
                        return end;
                    }
                }
                _ = tokio::time::sleep_until(heard + SERVER_SILENCE) => {
                    return End::Lost(format!(
                        "no word from the server in {}s",
                        SERVER_SILENCE.as_secs()
                    ));
                }
                changed = player.changed(), if player_alive => {
                    if changed.is_err() {
                        player_alive = false;
                        continue;
                    }
                    let report = report_of(&player.borrow_and_update());
                    let now = Instant::now();
                    if clock.is_urgent(&report, now) {
                        if let Err(end) = send(&mut sink, Outgoing::Report(report.clone())).await {
                            return end;
                        }
                        clock.sent(report, now);
                    }
                }
                _ = tokio::time::sleep_until(due) => {
                    let now = Instant::now();
                    match clock.periodic() {
                        Periodic::Report => {
                            let report = report_of(&player.borrow());
                            if let Err(end) = send(&mut sink, Outgoing::Report(report.clone())).await {
                                return end;
                            }
                            clock.sent(report, now);
                        }
                        Periodic::Heartbeat => {
                            if let Err(end) = send(&mut sink, Outgoing::Heartbeat).await {
                                return end;
                            }
                            clock.heartbeat_sent(now);
                        }
                    }
                }
                request = self.requests.recv() => {
                    let request = match request {
                        Some(Request::Shutdown(done)) => {
                            close(&mut sink).await;
                            let _ = done.send(());
                            return End::Shutdown;
                        }
                        None => {
                            close(&mut sink).await;
                            return End::Shutdown;
                        }
                        Some(request) => request,
                    };
                    if let Some(message) = self.on_request(request)
                        && let Err(end) = send(&mut sink, message).await
                    {
                        return end;
                    }
                }
            }
        }
    }

    /// Handles one frame from the server; `Some` once the connection is over.
    fn on_frame(&mut self, frame: Option<Result<Message, tungstenite::Error>>) -> Option<End> {
        match frame {
            Some(Ok(Message::Text(text))) => {
                self.on_message(&text);
                None
            }
            Some(Ok(Message::Close(frame))) => Some(match frame {
                Some(f) if u16::from(f.code) == CLOSE_REPLACED => End::Replaced,
                Some(f) => End::Lost(format!("the server closed the connection ({})", f.code)),
                None => End::Lost("the server closed the connection".into()),
            }),
            // Pings are answered by tungstenite as part of reading.
            Some(Ok(_)) => None,
            Some(Err(err)) => Some(End::Lost(format!("connection lost: {err}"))),
            None => Some(End::Lost("connection lost".into())),
        }
    }

    fn on_message(&mut self, text: &str) {
        let message = match serde_json::from_str::<Incoming>(text) {
            Ok(message) => message,
            Err(err) => {
                tracing::debug!("ignoring a now-playing message that didn't parse: {err}");
                return;
            }
        };
        match message {
            Incoming::Devices { devices } => {
                let me = self.device_id.as_str();
                let others: Vec<Device> =
                    devices.into_iter().filter(|d| d.device_id != me).collect();
                self.state.send_modify(|s| {
                    s.devices = others;
                    s.received = Instant::now();
                });
            }
            Incoming::Command {
                from_device_id,
                action,
                args,
            } => match Action::parse(&action, &args) {
                Some(parsed) => {
                    let from = self.label_of(&from_device_id);
                    tracing::info!(%from, action, "remote command");
                    self.apply(parsed);
                }
                None => tracing::debug!(
                    action,
                    "ignoring a remote command this player doesn't support"
                ),
            },
            Incoming::Ack {
                ok,
                detail,
                cmd_id,
                target_device_id,
            } => self.on_ack(ok, &detail, cmd_id.as_deref(), &target_device_id),
            Incoming::Unknown => {}
        }
    }

    fn on_ack(&mut self, ok: bool, detail: &str, cmd_id: Option<&str>, target: &str) {
        let pending = cmd_id.and_then(|id| {
            let index = self.pending.iter().position(|(pid, _, _)| pid == id)?;
            self.pending.remove(index)
        });
        let Some((_, name, kind)) = pending else {
            return;
        };
        if !ok {
            tracing::info!(target, detail, "command not delivered");
            self.notify(
                &format!("{name} is offline, so the command wasn't delivered"),
                true,
            );
            return;
        }
        match kind {
            Pending::Handoff => {
                // Only now is it safe to stop here: the other device has the episode.
                let _ = self.playback.pause();
                self.notify(&format!("Sent to {name}"), false);
            }
            Pending::PlayOn => self.notify(&format!("Playing on {name}"), false),
            Pending::Transport => {}
        }
    }

    /// Applies a command from another device to the local player.
    fn apply(&self, action: Action) {
        let playback = &self.playback;
        let result = match action {
            Action::Play => playback.resume(),
            Action::Pause => playback.pause(),
            Action::Seek(to) => playback.seek(to),
            Action::SkipForward(by) => playback.skip_forward(by),
            Action::SkipBack(by) => playback.skip_backward(by),
            Action::SetSpeed(speed) => playback.set_speed(speed),
            Action::PlayEpisode { episode, position } => {
                self.start_episode(
                    episode,
                    position,
                    "Couldn't start the episode another device sent",
                );
                Ok(())
            }
        };
        if result.is_err() {
            tracing::warn!("couldn't apply a remote command: the player has stopped");
        }
    }

    /// Looks the episode up (another device only knows its id) and plays it here.
    fn start_episode(&self, key: EpisodeKey, position: Option<Duration>, failure: &'static str) {
        let api = self.api.clone();
        let playback = self.playback.clone();
        let state = self.state.clone();
        tokio::spawn(async move {
            let result = match api.get_episode_metadata(key).await {
                Ok(episode) => play(&playback, episode, position),
                Err(err) => Err(err.to_string()),
            };
            if let Err(err) = result {
                tracing::warn!(episode_id = key.episode_id, "{failure}: {err}");
                state.send_modify(|s| s.notice = Some(notice(format!("{failure}: {err}"), true)));
            }
        });
    }

    /// Turns a UI request into the message to send, if any.
    fn on_request(&mut self, request: Request) -> Option<Outgoing> {
        match request {
            Request::Command { target, action } => {
                let kind = match action {
                    Action::PlayEpisode { .. } => Pending::PlayOn,
                    _ => Pending::Transport,
                };
                Some(self.command(target, &action, kind))
            }
            Request::SendPlayback { target } => {
                let state = self.playback.get_state();
                let Some(episode) = state.current_episode else {
                    self.notify("Nothing is playing here to send", true);
                    return None;
                };
                let action = Action::PlayEpisode {
                    episode: episode.key(),
                    position: Some(state.current_position),
                };
                Some(self.command(target, &action, Pending::Handoff))
            }
            Request::PlayHere { source, position } => {
                let Some(episode) = source.episode() else {
                    self.notify(&format!("{} isn't playing anything", source.label()), true);
                    return None;
                };
                self.start_episode(episode, Some(position), "Couldn't play that episode here");
                // Pause the other device so it doesn't keep going in two places.
                source
                    .playing
                    .then(|| self.command(source.device_id, &Action::Pause, Pending::Transport))
            }
            Request::Shutdown(_) => unreachable!("handled by the caller"),
        }
    }

    fn command(&mut self, target: String, action: &Action, kind: Pending) -> Outgoing {
        self.next_cmd += 1;
        let cmd_id = self.next_cmd.to_string();
        if self.pending.len() >= MAX_PENDING {
            self.pending.pop_front();
        }
        self.pending
            .push_back((cmd_id.clone(), self.label_of(&target), kind));
        let (action, args) = action.to_wire();
        Outgoing::Command {
            target_device_id: target,
            action,
            args,
            cmd_id,
        }
    }

    fn label_of(&self, device_id: &str) -> String {
        self.state
            .borrow()
            .devices
            .iter()
            .find(|d| d.device_id == device_id)
            .map_or_else(|| "That device".to_string(), |d| d.label().to_string())
    }

    fn notify(&self, text: &str, is_error: bool) {
        self.state
            .send_modify(|s| s.notice = Some(notice(text.to_string(), is_error)));
    }

    /// Connection errors can quote the URL, which carries the API key.
    fn redact(&self, message: &str) -> String {
        let key = &self.config.api_key;
        if key.is_empty() {
            return message.to_string();
        }
        message
            .replace(urlencoding::encode(key).as_ref(), "***")
            .replace(key.as_str(), "***")
    }
}

fn play(playback: &Playback, episode: Episode, position: Option<Duration>) -> Result<(), String> {
    match position {
        Some(start) => playback.play_episode_from(episode, start),
        None => playback.play_episode(episode),
    }
    .map_err(|e| e.to_string())
}

fn notice(text: String, is_error: bool) -> Notice {
    Notice {
        text,
        is_error,
        at: Instant::now(),
    }
}

type Sink = futures_util::stream::SplitSink<Socket, Message>;

async fn send(sink: &mut Sink, message: Outgoing) -> Result<(), End> {
    match tokio::time::timeout(SEND_TIMEOUT, sink.send(Message::text(message.to_json()))).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(End::Lost(format!("couldn't send: {err}"))),
        Err(_) => Err(End::Lost("timed out sending".into())),
    }
}

async fn close(sink: &mut Sink) {
    let frame = CloseFrame {
        code: CloseCode::Normal,
        reason: "quitting".into(),
    };
    let closing = async {
        sink.send(Message::Close(Some(frame))).await?;
        sink.close().await
    };
    match tokio::time::timeout(CLOSE_TIMEOUT, closing).await {
        Ok(Ok(())) => tracing::debug!("now-playing socket closed"),
        Ok(Err(err)) => tracing::debug!("couldn't close the now-playing socket cleanly: {err}"),
        Err(_) => tracing::debug!("timed out closing the now-playing socket"),
    }
}

/// `ws(s)://<server>/ws/api/nowplaying/<user>?…`. It carries the API key (browsers
/// can't set headers on a WebSocket, so the server takes it in the query); never log it.
fn socket_url(
    server: &str,
    user_id: i64,
    api_key: &str,
    device_id: &str,
    device_name: &str,
) -> String {
    let server = normalize_server_url(server);
    let (scheme, rest) = match server.strip_prefix("http://") {
        Some(rest) => ("ws", rest),
        None => ("wss", server.strip_prefix("https://").unwrap_or(&server)),
    };
    let enc = |s: &str| urlencoding::encode(s).into_owned();
    format!(
        "{scheme}://{rest}/ws/api/nowplaying/{user_id}?api_key={}&device_id={}&device_name={}&device_type={DEVICE_TYPE}&capabilities={}",
        enc(api_key),
        enc(device_id),
        enc(device_name),
        enc(&CAPABILITIES.join(",")),
    )
}

fn host_of(server: &str) -> String {
    let server = normalize_server_url(server);
    let rest = server.split_once("://").map_or(server.as_str(), |(_, r)| r);
    rest.split('/').next().unwrap_or(rest).to_string()
}

fn describe_connect_error(err: &tungstenite::Error) -> String {
    match err {
        tungstenite::Error::Http(response) => match response.status().as_u16() {
            401 | 403 => "the server rejected the API key".into(),
            // A 200 is a reverse proxy's fallback page answering instead of the API.
            200 | 404 => "the server has no now-playing socket at /ws/api/nowplaying (an \
                          older PinePods, or a reverse proxy that doesn't pass /ws/api/ \
                          through)"
                .into(),
            status => format!("the server refused the connection (HTTP {status})"),
        },
        other => format!("couldn't connect: {other}"),
    }
}

/// Exponential from 2 s to 60 s, scaled by `unit` in `[0, 1)` to between 80% and 120%
/// so devices that lost the same server don't all come back at the same instant.
fn backoff(failures: u32, unit: f64) -> Duration {
    let base = BACKOFF_MIN.as_secs_f64() * 2f64.powi(failures.min(10) as i32);
    let jittered = base.min(BACKOFF_MAX.as_secs_f64()) * (0.8 + 0.4 * unit);
    Duration::from_secs_f64(jittered.clamp(BACKOFF_MIN.as_secs_f64(), BACKOFF_MAX.as_secs_f64()))
}

fn random_unit() -> f64 {
    let mut bytes = [0u8; 4];
    if getrandom::fill(&mut bytes).is_err() {
        return 0.5;
    }
    f64::from(u32::from_le_bytes(bytes)) / (f64::from(u32::MAX) + 1.0)
}

/// Same TLS setup as the HTTP client: the OS certificate store and its policies.
fn tls_config() -> Option<Arc<rustls::ClientConfig>> {
    use rustls_platform_verifier::ConfigVerifierExt;
    match rustls::ClientConfig::with_platform_verifier() {
        Ok(config) => Some(Arc::new(config)),
        Err(err) => {
            tracing::warn!(
                "couldn't load the system certificates for the now-playing socket: {err}"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_follows_the_server_scheme_and_encodes_params() {
        let url = socket_url(
            "https://pods.example.com/",
            2,
            "k&y=",
            "abc123",
            "Firewood · studio",
        );
        assert_eq!(
            url,
            "wss://pods.example.com/ws/api/nowplaying/2?api_key=k%26y%3D&device_id=abc123\
             &device_name=Firewood%20%C2%B7%20studio&device_type=Firewood\
             &capabilities=play%2Cpause%2Cseek%2Cskip_forward%2Cskip_back%2Cset_speed%2Cplay_episode"
        );
        assert!(
            socket_url("http://10.0.0.5:8040", 1, "k", "d", "n")
                .starts_with("ws://10.0.0.5:8040/ws/api/")
        );
        // A bare host gets https, as at login.
        assert!(
            socket_url("pods.example.com", 1, "k", "d", "n")
                .starts_with("wss://pods.example.com/ws/")
        );
        // A server under a path prefix keeps it.
        assert!(
            socket_url("https://example.com/pinepods", 1, "k", "d", "n")
                .starts_with("wss://example.com/pinepods/ws/api/nowplaying/1?")
        );
    }

    #[test]
    fn host_is_safe_to_log() {
        assert_eq!(host_of("https://pods.example.com/x"), "pods.example.com");
        assert_eq!(host_of("http://10.0.0.5:8040"), "10.0.0.5:8040");
    }

    #[test]
    fn backoff_grows_to_a_minute_with_jitter() {
        assert_eq!(backoff(0, 0.5), Duration::from_secs(2));
        assert_eq!(
            backoff(0, 0.0),
            Duration::from_secs(2),
            "never under the minimum"
        );
        assert_eq!(backoff(2, 0.5), Duration::from_secs(8));
        assert_eq!(backoff(30, 0.5), Duration::from_secs(60));
        assert_eq!(
            backoff(30, 0.99),
            Duration::from_secs(60),
            "never over the maximum"
        );
        assert!(backoff(3, 0.0) < backoff(3, 0.99));
    }

    #[test]
    fn live_position_advances_only_while_playing() {
        let received = Instant::now();
        let state = RemoteState {
            connection: Connection::Connected,
            device_name: String::new(),
            devices: Vec::new(),
            received,
            notice: None,
        };
        let mut device = Device {
            episode_id: 1,
            position_sec: 100.0,
            duration_sec: 105.0,
            playing: true,
            speed: 2.0,
            ..Device::default()
        };
        let later = received + Duration::from_secs(2);
        assert_eq!(
            state.live_position(&device, later),
            Duration::from_secs(104)
        );
        assert_eq!(
            state.live_position(&device, received + Duration::from_secs(10)),
            Duration::from_secs(105),
            "capped at the end"
        );
        device.playing = false;
        assert_eq!(
            state.live_position(&device, later),
            Duration::from_secs(100)
        );
    }
}
