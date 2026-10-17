//! An episode's audio as something the decoder can read and seek.
//!
//! Over HTTP it's a [`range_reader`](super::range_reader): playback starts as soon as
//! the first bytes arrive, and seeking past what's downloaded starts a new request at
//! that offset instead of waiting for everything before it.

use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};

use super::range_reader::{self, OpenFailure};

trait ReadSeek: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> ReadSeek for T {}

/// Why opening failed, and whether trying again might help.
#[derive(Debug, Clone)]
pub struct OpenError {
    pub message: String,
    /// False for answers that won't change, like a 404 or a malformed URL.
    pub retryable: bool,
}

pub struct Media {
    pub reader: MediaReader,
    pub byte_len: Option<u64>,
    pub mime_type: Option<String>,
    /// File extension from the URL, as a format hint for the decoder.
    pub extension: Option<String>,
    cancel: Option<Box<dyn Fn() + Send + Sync>>,
}

impl Media {
    /// Starts streaming `url`. Returns once the server has answered, before the body
    /// arrives.
    pub async fn open_http(client: &reqwest::Client, url: &str) -> Result<Self, OpenError> {
        let parsed = reqwest::Url::parse(url).map_err(|_| OpenError {
            message: "the episode's audio URL isn't valid".into(),
            retryable: false,
        })?;
        let extension = parsed
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .and_then(|name| name.rsplit_once('.'))
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .filter(|ext| ext.len() <= 4);

        let opened = range_reader::open(client.clone(), parsed)
            .await
            .map_err(open_error)?;
        let handle = opened.handle;
        Ok(Self {
            reader: MediaReader::new(opened.reader),
            byte_len: opened.len,
            mime_type: opened.mime_type,
            extension,
            cancel: Some(Box::new(move || handle.cancel())),
        })
    }

    /// Audio already in memory.
    pub fn from_bytes(data: Vec<u8>, extension: Option<&str>) -> Self {
        Self {
            byte_len: Some(data.len() as u64),
            reader: MediaReader::new(Cursor::new(data)),
            mime_type: None,
            extension: extension.map(str::to_string),
            cancel: None,
        }
    }

    /// Stops the background download, so a decoder blocked on it gives up promptly.
    pub fn canceller(&mut self) -> Option<Box<dyn Fn() + Send + Sync>> {
        self.cancel.take()
    }
}

fn open_error(failure: OpenFailure) -> OpenError {
    match failure {
        OpenFailure::Network(err) => OpenError {
            message: format!("couldn't reach the episode audio: {}", err.without_url()),
            retryable: true,
        },
        OpenFailure::Status(status) => OpenError {
            message: format!("the episode audio request failed ({status})"),
            // Server errors are often a restart or an overloaded proxy.
            retryable: status.is_server_error(),
        },
        OpenFailure::Storage(err) => OpenError {
            message: format!("couldn't create a cache file for the episode: {err}"),
            retryable: false,
        },
    }
}

/// Remembers the last I/O error, because the decoder reports a failed read the same
/// way as the end of the file.
pub struct MediaReader {
    inner: Box<dyn ReadSeek>,
    error: Arc<Mutex<Option<String>>>,
}

impl MediaReader {
    fn new(inner: impl Read + Seek + Send + Sync + 'static) -> Self {
        Self {
            inner: Box::new(inner),
            error: Arc::default(),
        }
    }

    pub fn error_slot(&self) -> Arc<Mutex<Option<String>>> {
        self.error.clone()
    }

    fn record<T>(&self, result: io::Result<T>) -> io::Result<T> {
        if let Err(err) = &result
            && let Ok(mut slot) = self.error.lock()
        {
            *slot = Some(err.to_string());
        }
        result
    }
}

impl Read for MediaReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let result = self.inner.read(buf);
        self.record(result)
    }
}

impl Seek for MediaReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let result = self.inner.seek(pos);
        self.record(result)
    }
}
