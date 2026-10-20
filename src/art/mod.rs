//! Cover art: fetched in the background, decoded and shrunk off the render loop,
//! and kept in memory and on disk.
//!
//! Nothing here knows about terminals, so the system now-playing controls (and a
//! headless Firewood) can use the same images as the TUI. Images come through the
//! server's `/api/proxy/image`, which caches them and spares this machine from
//! contacting every podcast host; if the server refuses (an old server, or a host on
//! a private network, which the proxy won't reach), they're fetched directly.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use image::{DynamicImage, ImageFormat, imageops::FilterType};

use crate::api::http::normalize_server_url;

/// Covers are shrunk to fit this many pixels square: enough for a terminal of any
/// size, and small to keep and redraw.
const MAX_SIDE: u32 = 512;
/// Decoded covers kept in memory (about 1 MB each).
const MEMORY_ENTRIES: usize = 48;
/// The cache folder is trimmed to this, oldest first, at startup.
const DISK_BYTES: u64 = 64 * 1024 * 1024;
/// Downloads bigger than this aren't cover art worth showing.
const MAX_DOWNLOAD: usize = 12 * 1024 * 1024;
/// How long a cover that couldn't be fetched isn't tried again.
const RETRY_AFTER: Duration = Duration::from_secs(300);

/// A cover, ready to draw.
pub struct Cover {
    pub image: DynamicImage,
    /// The shrunk cover as a JPEG on disk, for the system's now-playing controls.
    pub file: Option<PathBuf>,
}

/// Where a cover is.
#[derive(Clone)]
pub enum Art {
    Loading,
    Ready(Arc<Cover>),
    /// No art, or it couldn't be fetched or read.
    Missing,
}

enum Entry {
    Loading,
    Ready(Arc<Cover>),
    Failed(Instant),
}

#[derive(Default)]
struct Store {
    entries: HashMap<String, Entry>,
    /// Ready covers, least recently used first.
    recent: VecDeque<String>,
}

impl Store {
    fn touch(&mut self, url: &str) {
        if let Some(i) = self.recent.iter().position(|u| u == url) {
            let url = self.recent.remove(i).expect("position is in range");
            self.recent.push_back(url);
        }
    }

    fn insert_ready(&mut self, url: String, cover: Arc<Cover>) {
        self.entries.insert(url.clone(), Entry::Ready(cover));
        self.recent.retain(|u| *u != url);
        self.recent.push_back(url);
        while self.recent.len() > MEMORY_ENTRIES {
            if let Some(old) = self.recent.pop_front() {
                self.entries.remove(&old);
            }
        }
    }
}

/// The cover art cache. Cheap to clone.
#[derive(Clone)]
pub struct Artwork {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    server: String,
    dir: Option<PathBuf>,
    store: Mutex<Store>,
}

impl Artwork {
    /// Must be called inside a Tokio runtime. `server` is the PinePods server, for its
    /// image proxy.
    pub fn new(http: reqwest::Client, server: &str) -> Self {
        let dir = crate::paths::cache_dir().ok().map(|d| d.join("art"));
        if let Some(dir) = dir.clone() {
            tokio::task::spawn_blocking(move || {
                if let Err(e) = fs::create_dir_all(&dir) {
                    tracing::warn!("couldn't create the art cache {}: {e}", dir.display());
                    return;
                }
                trim_dir(&dir, DISK_BYTES);
            });
        }
        Self {
            inner: Arc::new(Inner {
                http,
                server: normalize_server_url(server),
                dir,
                store: Mutex::new(Store::default()),
            }),
        }
    }

    /// The cover at `url`, starting to fetch it if it isn't here yet. Never waits.
    pub fn get(&self, url: &str) -> Art {
        let url = url.trim();
        if url.is_empty() {
            return Art::Missing;
        }
        let mut store = self.inner.store.lock().expect("art store lock");
        match store.entries.get(url) {
            Some(Entry::Ready(cover)) => {
                let cover = cover.clone();
                store.touch(url);
                return Art::Ready(cover);
            }
            Some(Entry::Loading) => return Art::Loading,
            Some(Entry::Failed(at)) if at.elapsed() < RETRY_AFTER => return Art::Missing,
            _ => {}
        }
        store.entries.insert(url.to_string(), Entry::Loading);
        drop(store);

        let (inner, url) = (self.inner.clone(), url.to_string());
        tokio::spawn(async move {
            let entry = match inner.load(&url).await {
                Ok(cover) => {
                    let cover = Arc::new(cover);
                    inner
                        .store
                        .lock()
                        .expect("art store lock")
                        .insert_ready(url, cover);
                    return;
                }
                Err(e) => {
                    tracing::debug!("no cover from {url}: {e}");
                    Entry::Failed(Instant::now())
                }
            };
            inner
                .store
                .lock()
                .expect("art store lock")
                .entries
                .insert(url, entry);
        });
        Art::Loading
    }
}

impl Inner {
    async fn load(&self, url: &str) -> Result<Cover, String> {
        let file = self.dir.as_ref().map(|d| d.join(file_name(url)));
        if let Some(path) = file.clone()
            && let Ok(Ok(image)) = tokio::task::spawn_blocking(move || read_cached(&path)).await
        {
            return Ok(Cover { image, file });
        }

        let proxied = format!(
            "{}/api/proxy/image?url={}",
            self.server,
            urlencoding::encode(url)
        );
        let bytes = match self.download(&proxied).await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::debug!(
                    "the server's image proxy didn't serve {url} ({e}); fetching it directly"
                );
                self.download(url).await?
            }
        };
        tokio::task::spawn_blocking(move || {
            let image = decode(&bytes)?;
            let file = file.filter(|path| match write_cached(path, &image) {
                Ok(()) => true,
                Err(e) => {
                    tracing::debug!("couldn't cache a cover at {}: {e}", path.display());
                    false
                }
            });
            Ok(Cover { image, file })
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn download(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut response = self.http.get(url).send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_DOWNLOAD as u64)
        {
            return Err("too big".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MAX_DOWNLOAD {
                return Err("too big".into());
            }
        }
        Ok(bytes)
    }
}

/// Decodes `bytes` and shrinks the result to [`MAX_SIDE`].
fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    Ok(shrink(image))
}

fn shrink(image: DynamicImage) -> DynamicImage {
    if image.width() <= MAX_SIDE && image.height() <= MAX_SIDE {
        return image;
    }
    image.resize(MAX_SIDE, MAX_SIDE, FilterType::Triangle)
}

fn read_cached(path: &Path) -> Result<DynamicImage, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let image = image::load_from_memory_with_format(&bytes, ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    // Mark it used, so trimming the folder drops the covers nobody looks at.
    if let Ok(file) = fs::File::options().append(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
    Ok(image)
}

fn write_cached(path: &Path, image: &DynamicImage) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    // JPEG has no alpha channel.
    image
        .to_rgb8()
        .save_with_format(&tmp, ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// A stable file name for `url`: FNV-1a, which unlike `DefaultHasher` doesn't
/// change between Rust releases.
fn file_name(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}.jpg")
}

/// Deletes the least recently used files in `dir` until it holds at most `limit` bytes.
fn trim_dir(dir: &Path, limit: u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| {
                (
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    meta.len(),
                    e.path(),
                )
            })
        })
        .collect();
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    files.sort_by_key(|(modified, _, _)| *modified);
    for (_, len, path) in files {
        if total <= limit {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_stable_and_distinct() {
        assert_eq!(file_name("https://a/x.jpg"), file_name("https://a/x.jpg"));
        assert_ne!(file_name("https://a/x.jpg"), file_name("https://a/y.jpg"));
        assert_eq!(file_name("").len(), 20);
    }

    #[test]
    fn big_covers_are_shrunk_keeping_their_shape() {
        let image = shrink(DynamicImage::new_rgb8(3000, 1500));
        assert_eq!((image.width(), image.height()), (MAX_SIDE, MAX_SIDE / 2));
        let small = shrink(DynamicImage::new_rgb8(100, 100));
        assert_eq!(small.width(), 100);
    }

    #[test]
    fn the_memory_cache_drops_the_least_recently_used() {
        let mut store = Store::default();
        let cover = || {
            Arc::new(Cover {
                image: DynamicImage::new_rgb8(1, 1),
                file: None,
            })
        };
        for i in 0..MEMORY_ENTRIES {
            store.insert_ready(i.to_string(), cover());
        }
        store.touch("0");
        store.insert_ready("new".into(), cover());
        assert!(store.entries.contains_key("0"));
        assert!(!store.entries.contains_key("1"));
        assert_eq!(store.recent.len(), MEMORY_ENTRIES);
    }

    #[test]
    fn trimming_the_folder_keeps_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        for (i, name) in ["old", "mid", "new"].iter().enumerate() {
            let path = dir.path().join(name);
            fs::write(&path, [0u8; 100]).unwrap();
            let file = fs::File::options().append(true).open(&path).unwrap();
            file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(i as u64 * 60))
                .unwrap();
        }
        trim_dir(dir.path(), 200);
        assert!(!dir.path().join("old").exists());
        assert!(dir.path().join("mid").exists());
        assert!(dir.path().join("new").exists());
    }

    #[test]
    fn cached_covers_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.jpg");
        write_cached(&path, &DynamicImage::new_rgba8(8, 4)).unwrap();
        let back = read_cached(&path).unwrap();
        assert_eq!((back.width(), back.height()), (8, 4));
    }
}
