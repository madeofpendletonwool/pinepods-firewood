//! Network work off the render loop.
//!
//! A page starts a request with [`Pending::spawn`] and checks it each frame with
//! [`Pending::poll`], so a slow server never freezes drawing or input. Changes (save,
//! queue, delete) go through [`Bus::run`], which reports the outcome as a toast and
//! tells every page what kind of data changed so the stale ones reload.

use std::future::Future;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};

use crate::api::ApiResult;

/// A request in flight.
pub struct Pending<T> {
    rx: Option<oneshot::Receiver<T>>,
    started: Instant,
}

impl<T: Send + 'static> Pending<T> {
    pub fn spawn(future: impl Future<Output = T> + Send + 'static) -> Self {
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let _ = tx.send(future.await);
        });
        Self {
            rx: Some(rx),
            started: Instant::now(),
        }
    }
}

impl<T> Default for Pending<T> {
    fn default() -> Self {
        Self {
            rx: None,
            started: Instant::now(),
        }
    }
}

impl<T> Pending<T> {
    /// The result, once, when it has arrived.
    pub fn poll(&mut self) -> Option<T> {
        let rx = self.rx.as_mut()?;
        match rx.try_recv() {
            Ok(value) => {
                self.rx = None;
                Some(value)
            }
            Err(oneshot::error::TryRecvError::Empty) => None,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.rx = None;
                None
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// Data a page shows, loaded in the background. Old data stays on screen while a
/// reload runs, so refreshing doesn't flash an empty page.
pub struct Remote<T> {
    pub data: Option<T>,
    pending: Pending<ApiResult<T>>,
    error: Option<String>,
    stale: bool,
}

impl<T> Default for Remote<T> {
    fn default() -> Self {
        Self {
            data: None,
            pending: Pending::default(),
            error: None,
            stale: false,
        }
    }
}

impl<T: Send + 'static> Remote<T> {
    /// Starts loading unless a load is already running.
    pub fn load(&mut self, future: impl Future<Output = ApiResult<T>> + Send + 'static) {
        if !self.pending.is_running() {
            self.pending = Pending::spawn(future);
            self.stale = false;
        }
    }
}

impl<T> Remote<T> {
    /// Takes in a finished load. True when new data arrived. A failed reload of data
    /// already on screen is reported as a toast; a failed first load shows in place
    /// of the page through [`error`](Self::error).
    pub fn poll(&mut self, bus: &Bus) -> bool {
        match self.pending.poll() {
            Some(Ok(data)) => {
                self.data = Some(data);
                self.error = None;
                true
            }
            Some(Err(e)) => {
                tracing::warn!("load failed: {e}");
                if self.data.is_some() {
                    bus.error(format!("Couldn't refresh: {e}"));
                }
                self.error = Some(e.to_string());
                false
            }
            None => false,
        }
    }

    pub fn is_loading(&self) -> bool {
        self.pending.is_running()
    }

    /// Loading with nothing to show yet.
    pub fn is_empty_loading(&self) -> bool {
        self.data.is_none() && self.pending.is_running()
    }

    /// The last load's error, while there's no data to show instead and no retry
    /// running.
    pub fn error(&self) -> Option<&str> {
        if self.data.is_some() || self.pending.is_running() {
            return None;
        }
        self.error.as_deref()
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Nothing loaded yet, or the data went stale.
    pub fn needs_load(&self) -> bool {
        (self.data.is_none() && self.error.is_none()) || self.stale
    }
}

/// Kinds of server data, so a change in one page can refresh the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Queue,
    Saved,
    Downloads,
    Podcasts,
    /// Which collections hold which episodes. Saved is one, so changes to it come
    /// with [`Resource::Saved`] too.
    Collections,
    /// Played state and positions.
    Progress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    pub at: Instant,
}

pub enum Event {
    Toast(Toast),
    Changed(Resource),
}

/// The pages' way to tell the app things. Cheap to clone into spawned tasks.
#[derive(Clone)]
pub struct Bus {
    tx: mpsc::UnboundedSender<Event>,
}

impl Bus {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Event>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    pub fn toast(&self, kind: ToastKind, text: impl Into<String>) {
        let _ = self.tx.send(Event::Toast(Toast {
            kind,
            text: text.into(),
            at: Instant::now(),
        }));
    }

    pub fn info(&self, text: impl Into<String>) {
        self.toast(ToastKind::Info, text);
    }

    pub fn success(&self, text: impl Into<String>) {
        self.toast(ToastKind::Success, text);
    }

    pub fn error(&self, text: impl Into<String>) {
        self.toast(ToastKind::Error, text);
    }

    pub fn changed(&self, resource: Resource) {
        let _ = self.tx.send(Event::Changed(resource));
    }

    /// Runs a change in the background. Success shows `done` and marks `affects`
    /// stale; failure shows "`failed`: <error>" and also marks it stale, so an
    /// optimistic edit on screen is put back the way the server has it.
    pub fn run(
        &self,
        done: impl Into<String>,
        failed: impl Into<String>,
        affects: &[Resource],
        future: impl Future<Output = ApiResult<()>> + Send + 'static,
    ) {
        let bus = self.clone();
        let (done, failed, affects) = (done.into(), failed.into(), affects.to_vec());
        tokio::spawn(async move {
            match future.await {
                Ok(()) => {
                    if !done.is_empty() {
                        bus.success(done);
                    }
                }
                Err(e) => {
                    tracing::warn!("{failed}: {e}");
                    bus.error(format!("{failed}: {e}"));
                }
            }
            for resource in affects {
                bus.changed(resource);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_hands_over_its_result_once() {
        let mut pending = Pending::spawn(async { 7 });
        assert!(pending.is_running());
        let value = loop {
            if let Some(v) = pending.poll() {
                break v;
            }
            tokio::task::yield_now().await;
        };
        assert_eq!(value, 7);
        assert!(!pending.is_running());
        assert_eq!(pending.poll(), None);
    }

    #[tokio::test]
    async fn run_reports_and_invalidates_on_failure_too() {
        let (bus, mut rx) = Bus::new();
        bus.run("Saved", "Couldn't save", &[Resource::Saved], async {
            Err(crate::api::ApiError::Unauthorized)
        });
        let Some(Event::Toast(toast)) = rx.recv().await else {
            panic!("expected a toast");
        };
        assert_eq!(toast.kind, ToastKind::Error);
        assert!(toast.text.starts_with("Couldn't save"));
        assert!(matches!(
            rx.recv().await,
            Some(Event::Changed(Resource::Saved))
        ));
    }
}
