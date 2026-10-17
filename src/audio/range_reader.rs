//! A remote file the decoder can read and seek like a local one.
//!
//! A background task downloads into a temp file and records which byte ranges have
//! arrived. Reads that land on downloaded bytes return at once; reads that don't block
//! the calling (decoder) thread until they do. When a read needs bytes far from where
//! the download is, the task drops its connection and starts a range request there, so
//! seeking an hour into an episode costs one request rather than the whole hour.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use reqwest::header::{CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::{Client, Response, StatusCode};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// A read this close ahead of the download just waits for it; further away, the
/// download restarts at the read. About 15 s of a typical episode.
const LOOKAHEAD: u64 = 256 * 1024;

/// A connection that delivers nothing for this long is treated as dropped.
const STALL_TIMEOUT: Duration = Duration::from_secs(15);

/// Waits before reconnecting after a dropped connection. When these run out the
/// reader reports the failure, and the player decides what to do.
const RECONNECT_DELAYS: [Duration; 3] = [
    Duration::from_millis(500),
    Duration::from_secs(2),
    Duration::from_secs(5),
];

/// How long a blocked read sleeps between checks of whether it's been cancelled.
const READ_WAIT: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub enum OpenFailure {
    /// The request never got an answer.
    Network(reqwest::Error),
    /// The server answered with an error status.
    Status(StatusCode),
    Storage(io::Error),
}

/// What the server told us when the download started.
pub struct Opened {
    pub reader: RangeReader,
    pub len: Option<u64>,
    pub mime_type: Option<String>,
    pub handle: DownloadHandle,
}

/// Starts downloading `url`. Returns once the server has answered.
pub async fn open(client: Client, url: reqwest::Url) -> Result<Opened, OpenFailure> {
    // Asking for a range from the start is the reliable way to learn whether the
    // server honors them: some (PinePods' own `/stream` among them) advertise
    // `Accept-Ranges: bytes` and then answer every range request with the whole file.
    let response = client
        .get(url.clone())
        .header(RANGE, "bytes=0-")
        .send()
        .await
        .map_err(OpenFailure::Network)?;
    let status = response.status();
    if !status.is_success() {
        return Err(OpenFailure::Status(status));
    }
    let ranges = status == StatusCode::PARTIAL_CONTENT && content_range_start(&response) == Some(0);
    let len = if ranges {
        content_range_total(&response)
    } else {
        response.content_length()
    };
    let mime_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().to_ascii_lowercase());

    let file = tempfile::tempfile().map_err(OpenFailure::Storage)?;
    let reader_file = file.try_clone().map_err(OpenFailure::Storage)?;
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            ranges: RangeSet::default(),
            wanted: None,
            failed: None,
            len,
            closed: false,
        }),
        arrived: Condvar::new(),
        wake: Notify::new(),
    });

    let task = tokio::spawn(
        Download {
            client,
            url,
            file,
            shared: shared.clone(),
            ranges_supported: AtomicBool::new(ranges && len.is_some()),
        }
        .run(response),
    );
    Ok(Opened {
        reader: RangeReader {
            shared: shared.clone(),
            file: reader_file,
            pos: 0,
        },
        len,
        mime_type,
        handle: DownloadHandle { shared, task },
    })
}

/// Stops the download when dropped or cancelled, waking any blocked read.
pub struct DownloadHandle {
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl DownloadHandle {
    pub fn cancel(&self) {
        self.task.abort();
        self.shared.lock().closed = true;
        self.shared.arrived.notify_all();
    }
}

impl Drop for DownloadHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct Shared {
    state: Mutex<State>,
    /// Signals readers that bytes arrived, or that the download failed or stopped.
    arrived: Condvar,
    /// Signals the download task that a reader wants somewhere new.
    wake: Notify,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

struct State {
    ranges: RangeSet,
    /// Where a blocked reader is waiting.
    wanted: Option<u64>,
    failed: Option<String>,
    len: Option<u64>,
    closed: bool,
}

pub struct RangeReader {
    shared: Arc<Shared>,
    file: File,
    pos: u64,
}

impl Read for RangeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut state = self.shared.lock();
        let available = loop {
            if state.len.is_some_and(|len| self.pos >= len) {
                return Ok(0);
            }
            if let Some(end) = state.ranges.end_of_range_at(self.pos) {
                break end - self.pos;
            }
            if let Some(message) = &state.failed {
                return Err(io::Error::other(message.clone()));
            }
            if state.closed {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "download cancelled",
                ));
            }
            if state.wanted != Some(self.pos) {
                state.wanted = Some(self.pos);
                self.shared.wake.notify_one();
            }
            state = self
                .shared
                .arrived
                .wait_timeout(state, READ_WAIT)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        if state.wanted == Some(self.pos) {
            state.wanted = None;
        }
        drop(state);

        let n = buf
            .len()
            .min(usize::try_from(available).unwrap_or(usize::MAX));
        let n = read_at(&self.file, &mut buf[..n], self.pos)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for RangeReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
            SeekFrom::End(delta) => match self.shared.lock().len {
                Some(len) => len.checked_add_signed(delta),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "the server didn't say how long the file is",
                    ));
                }
            },
        };
        // Moving is free; the next read fetches what it needs.
        self.pos = target
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"))?;
        Ok(self.pos)
    }
}

struct Download {
    client: Client,
    url: reqwest::Url,
    file: File,
    shared: Arc<Shared>,
    /// Cleared if the server turns out to ignore range requests after all.
    ranges_supported: AtomicBool,
}

/// Why a connection ended.
enum Ended {
    /// Reached the end of what was asked for.
    Done,
    /// A reader wants bytes elsewhere.
    Moved(u64),
    Failed(String),
}

impl Download {
    async fn run(self, first: Response) {
        let mut first = Some(first);
        let mut start = 0u64;
        let mut attempt = 0usize;
        loop {
            let response = match first.take() {
                Some(response) => Ok((response, start)),
                None => self.request(start).await,
            };
            let (ended, reached) = match response {
                Ok((response, from)) => {
                    start = from;
                    self.stream(response, from).await
                }
                Err(message) => (Ended::Failed(message), start),
            };
            if reached > start {
                attempt = 0;
            }
            match ended {
                Ended::Moved(to) => start = to,
                Ended::Done => match self.next_start().await {
                    Some(to) => start = to,
                    None => return,
                },
                Ended::Failed(message) => {
                    match RECONNECT_DELAYS
                        .get(attempt)
                        .filter(|_| self.ranges_supported())
                    {
                        Some(delay) => {
                            tracing::warn!("{message}; reconnecting");
                            attempt += 1;
                            tokio::time::sleep(*delay).await;
                            // A reader may have moved on while we waited.
                            let wanted = self.shared.lock().wanted.unwrap_or(reached);
                            start = self.resume_point(wanted);
                        }
                        None => {
                            self.shared.lock().failed = Some(message);
                            self.shared.arrived.notify_all();
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Asks for everything from `start`. Returns the response and where its body
    /// actually begins.
    async fn request(&self, start: u64) -> Result<(Response, u64), String> {
        let response = self
            .client
            .get(self.url.clone())
            .header(RANGE, format!("bytes={start}-"))
            .send()
            .await
            .map_err(|err| format!("couldn't reach the episode audio: {}", err.without_url()))?;
        match response.status() {
            StatusCode::PARTIAL_CONTENT if content_range_start(&response) == Some(start) => {
                Ok((response, start))
            }
            // The whole file after all. Take it from the top and stop asking for
            // ranges; reads further in wait for it to get there.
            StatusCode::OK => {
                tracing::warn!("the audio server ignored a range request; seeking will be slow");
                self.ranges_supported.store(false, Ordering::Relaxed);
                Ok((response, 0))
            }
            status => Err(format!("the episode audio request failed ({status})")),
        }
    }

    fn ranges_supported(&self) -> bool {
        self.ranges_supported.load(Ordering::Relaxed)
    }

    /// Copies one response into the file from `start` until it ends, fails, or a
    /// reader needs to be served from somewhere else. Also returns how far it got.
    async fn stream(&self, mut response: Response, start: u64) -> (Ended, u64) {
        let mut pos = start;
        loop {
            let chunk = tokio::select! {
                chunk = tokio::time::timeout(STALL_TIMEOUT, response.chunk()) => chunk,
                () = self.shared.wake.notified() => {
                    match self.redirect(pos) {
                        Some(to) => return (Ended::Moved(to), pos),
                        None => continue,
                    }
                }
            };
            let bytes = match chunk {
                Ok(Ok(Some(bytes))) => bytes,
                Ok(Ok(None)) => return (Ended::Done, pos),
                Ok(Err(err)) => {
                    let message = format!(
                        "lost the connection to the episode audio: {}",
                        err.without_url()
                    );
                    return (Ended::Failed(message), pos);
                }
                Err(_) => {
                    let message = "the episode audio stopped arriving".to_string();
                    return (Ended::Failed(message), pos);
                }
            };
            if let Err(err) = write_at(&self.file, &bytes, pos) {
                let message = format!("couldn't cache the episode audio: {err}");
                return (Ended::Failed(message), pos);
            }
            let end = pos + bytes.len() as u64;
            self.shared.lock().ranges.insert(pos..end);
            self.shared.arrived.notify_all();
            pos = end;
            if let Some(to) = self.redirect(pos) {
                return (Ended::Moved(to), pos);
            }
        }
    }

    /// Where to restart, if a waiting reader is too far from `pos` to wait for it.
    fn redirect(&self, pos: u64) -> Option<u64> {
        if !self.ranges_supported() {
            return None;
        }
        let state = self.shared.lock();
        let wanted = state.wanted?;
        if state.ranges.contains(wanted) || (pos..pos + LOOKAHEAD).contains(&wanted) {
            return None;
        }
        Some(self.resume_point_locked(&state, wanted))
    }

    /// After reaching the end of a request: keep going if the file has holes a reader
    /// is waiting on, otherwise park until one asks for something.
    async fn next_start(&self) -> Option<u64> {
        if !self.ranges_supported() {
            // Without ranges there's no way to fetch anything we don't have.
            let mut state = self.shared.lock();
            if state.len.is_none() {
                state.len = Some(state.ranges.end_of_range_at(0).unwrap_or(0));
            }
            drop(state);
            self.shared.arrived.notify_all();
            return None;
        }
        loop {
            {
                let state = self.shared.lock();
                if let Some(wanted) = state.wanted
                    && !state.ranges.contains(wanted)
                {
                    return Some(self.resume_point_locked(&state, wanted));
                }
            }
            self.shared.wake.notified().await;
        }
    }

    fn resume_point(&self, wanted: u64) -> u64 {
        let state = self.shared.lock();
        self.resume_point_locked(&state, wanted)
    }

    /// Skips bytes already downloaded from `wanted` onward.
    fn resume_point_locked(&self, state: &State, wanted: u64) -> u64 {
        state.ranges.end_of_range_at(wanted).unwrap_or(wanted)
    }
}

/// The first byte of a `Content-Range: bytes START-END/TOTAL` answer.
fn content_range_start(response: &Response) -> Option<u64> {
    let value = response.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    let range = value.strip_prefix("bytes ")?;
    range.split('-').next()?.trim().parse().ok()
}

/// The file's full length from `Content-Range`, when the server knows it.
fn content_range_total(response: &Response) -> Option<u64> {
    let value = response.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    value.rsplit_once('/')?.1.trim().parse().ok()
}

/// Sorted, non-overlapping byte ranges.
#[derive(Debug, Default, Clone, PartialEq)]
struct RangeSet(Vec<Range<u64>>);

impl RangeSet {
    fn insert(&mut self, range: Range<u64>) {
        if range.is_empty() {
            return;
        }
        let ranges = &mut self.0;
        // Everything touching `range` merges into it.
        let first = ranges.partition_point(|r| r.end < range.start);
        let last = ranges.partition_point(|r| r.start <= range.end);
        let merged = if first < last {
            ranges[first].start.min(range.start)..ranges[last - 1].end.max(range.end)
        } else {
            range
        };
        ranges.splice(first..last, [merged]);
    }

    fn contains(&self, pos: u64) -> bool {
        self.end_of_range_at(pos).is_some()
    }

    /// The end of the downloaded range holding `pos`.
    fn end_of_range_at(&self, pos: u64) -> Option<u64> {
        let idx = self.0.partition_point(|r| r.start <= pos);
        let range = self.0.get(idx.checked_sub(1)?)?;
        (pos < range.end).then_some(range.end)
    }
}

#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, offset)
}

#[cfg(unix)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, offset)
}

#[cfg(windows)]
fn write_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    while !buf.is_empty() {
        let n = std::os::windows::fs::FileExt::seek_write(file, buf, offset)?;
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    /// Serves `body`, honoring `Range: bytes=N-` like a CDN would.
    struct Ranged(Vec<u8>);

    impl Respond for Ranged {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let len = self.0.len();
            let start = request
                .headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.trim_end_matches('-').parse::<usize>().ok());
            match start {
                Some(start) => ResponseTemplate::new(206)
                    .insert_header("Content-Range", format!("bytes {start}-{}/{len}", len - 1))
                    .insert_header("Content-Type", "audio/mpeg")
                    .set_body_bytes(self.0[start..].to_vec()),
                None => ResponseTemplate::new(200)
                    .insert_header("Accept-Ranges", "bytes")
                    .insert_header("Content-Type", "audio/mpeg; charset=binary")
                    .set_body_bytes(self.0.clone()),
            }
        }
    }

    fn body(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 % 251) as u8).collect()
    }

    async fn serve(responder: impl Respond + 'static) -> (MockServer, reqwest::Url) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(responder)
            .mount(&server)
            .await;
        let url = format!("{}/ep.mp3", server.uri()).parse().unwrap();
        (server, url)
    }

    /// Runs blocking reader work off the runtime, as the decoder thread does.
    async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        tokio::task::spawn_blocking(f).await.unwrap()
    }

    #[tokio::test]
    async fn reads_the_whole_file_and_reports_its_headers() {
        let data = body(300_000);
        let (_server, url) = serve(Ranged(data.clone())).await;
        let opened = open(Client::new(), url).await.unwrap();
        assert_eq!(opened.len, Some(300_000));
        assert_eq!(opened.mime_type.as_deref(), Some("audio/mpeg"));
        let mut reader = opened.reader;
        let read = blocking(move || {
            let mut out = Vec::new();
            reader.read_to_end(&mut out).unwrap();
            out
        })
        .await;
        assert_eq!(read, data);
    }

    #[tokio::test]
    async fn seeking_reads_the_right_bytes() {
        let data = body(2_000_000);
        let (server, url) = serve(Ranged(data.clone())).await;
        let opened = open(Client::new(), url).await.unwrap();
        let mut reader = opened.reader;
        let expected = data.clone();
        blocking(move || {
            for offset in [1_900_000u64, 10, 1_000_000, 1_999_990] {
                reader.seek(SeekFrom::Start(offset)).unwrap();
                let mut buf = [0u8; 10];
                reader.read_exact(&mut buf).unwrap();
                let at = offset as usize;
                assert_eq!(buf, expected[at..at + 10], "at {offset}");
            }
            reader.seek(SeekFrom::End(0)).unwrap();
            assert_eq!(reader.read(&mut [0u8; 4]).unwrap(), 0);
        })
        .await;
        drop(opened.handle);
        // Every request was either the first one or a well-formed range request.
        for request in server.received_requests().await.unwrap() {
            if let Some(range) = request.headers.get("range") {
                assert!(range.to_str().unwrap().starts_with("bytes="));
            }
        }
    }

    #[tokio::test]
    async fn works_without_range_support() {
        let data = body(500_000);
        let (_server, url) = serve(
            ResponseTemplate::new(200)
                .insert_header("Accept-Ranges", "none")
                .set_body_bytes(data.clone()),
        )
        .await;
        let opened = open(Client::new(), url).await.unwrap();
        let mut reader = opened.reader;
        blocking(move || {
            reader.seek(SeekFrom::Start(400_000)).unwrap();
            let mut buf = [0u8; 16];
            reader.read_exact(&mut buf).unwrap();
            assert_eq!(buf, data[400_000..400_016]);
        })
        .await;
    }

    #[tokio::test]
    async fn copes_with_a_server_that_advertises_ranges_but_ignores_them() {
        let data = body(500_000);
        let (server, url) = serve(
            ResponseTemplate::new(200)
                .insert_header("Accept-Ranges", "bytes")
                .set_body_bytes(data.clone()),
        )
        .await;
        let opened = open(Client::new(), url).await.unwrap();
        assert_eq!(opened.len, Some(500_000));
        let mut reader = opened.reader;
        blocking(move || {
            reader.seek(SeekFrom::Start(450_000)).unwrap();
            let mut buf = [0u8; 16];
            reader.read_exact(&mut buf).unwrap();
            assert_eq!(buf, data[450_000..450_016]);
        })
        .await;
        // Learned from the first answer, so no doomed range requests follow it.
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn error_status_fails_the_open() {
        let (_server, url) = serve(ResponseTemplate::new(404)).await;
        match open(Client::new(), url).await {
            Err(OpenFailure::Status(status)) => assert_eq!(status, StatusCode::NOT_FOUND),
            other => panic!("expected a 404, got {:?}", other.err()),
        }
    }

    #[tokio::test]
    async fn cancelling_wakes_a_blocked_read() {
        let data = body(100);
        let (_server, url) = serve(Ranged(data)).await;
        let opened = open(Client::new(), url).await.unwrap();
        let mut reader = opened.reader;
        let handle = opened.handle;
        // Past the end of a known-length file reads as EOF, so ask the reader for a
        // position it has to wait on by pretending the length is larger.
        reader.shared.lock().len = Some(10_000);
        let read = tokio::task::spawn_blocking(move || {
            reader.seek(SeekFrom::Start(5_000)).unwrap();
            reader.read(&mut [0u8; 8])
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        handle.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), read)
            .await
            .expect("the read should wake up")
            .unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn range_set_merges_touching_and_overlapping_ranges() {
        let mut set = RangeSet::default();
        set.insert(10..20);
        set.insert(30..40);
        set.insert(20..25);
        assert_eq!(set.0, vec![10..25, 30..40]);
        set.insert(24..31);
        assert_eq!(set.0, vec![10..40]);
        set.insert(0..5);
        set.insert(50..60);
        set.insert(5..55);
        assert_eq!(set.0, vec![0..60]);
    }

    #[test]
    fn range_set_lookup() {
        let mut set = RangeSet::default();
        set.insert(10..20);
        set.insert(30..40);
        assert_eq!(set.end_of_range_at(9), None);
        assert_eq!(set.end_of_range_at(10), Some(20));
        assert_eq!(set.end_of_range_at(19), Some(20));
        assert_eq!(set.end_of_range_at(20), None);
        assert_eq!(set.end_of_range_at(35), Some(40));
        assert!(!set.contains(45));
    }
}
