//! Now Playing (Control Center, the lock screen) and the media keys on macOS,
//! through `MPNowPlayingInfoCenter` and `MPRemoteCommandCenter`.
//!
//! Both belong to the main thread: commands are delivered on the main queue, and
//! that's where updates go too. [`run_with_main_loop`] keeps the main thread free to
//! serve it.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, Message};
use objc2_app_kit::NSImage;
use objc2_core_foundation::{CFRunLoop, CGSize, kCFRunLoopDefaultMode};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle,
    MPMediaItemPropertyArtist, MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration,
    MPMediaItemPropertyTitle, MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyDefaultPlaybackRate,
    MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate,
    MPNowPlayingPlaybackState, MPRemoteCommand, MPRemoteCommandCenter, MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus, MPSkipIntervalCommandEvent,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{Control, Snapshot, Status, Update};

/// How long the main thread waits in its run loop before checking whether the app
/// has finished. Stopping the loop wakes it sooner; this covers a stop that
/// arrives before the loop has started.
const RUN_LOOP_SLICE: f64 = 0.5;

pub fn run_with_main_loop<T: Send + 'static>(app: impl FnOnce() -> T + Send + 'static) -> T {
    let done = Arc::new(AtomicBool::new(false));
    let finished = done.clone();
    let handle = std::thread::Builder::new()
        .name("firewood".into())
        // As much stack as the main thread would have had.
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let result = app();
            finished.store(true, Ordering::SeqCst);
            if let Some(main) = CFRunLoop::main() {
                main.stop();
            }
            result
        })
        .expect("couldn't start the app thread");
    while !done.load(Ordering::SeqCst) {
        // SAFETY: the default mode is a constant CFString provided by the system.
        let mode = unsafe { kCFRunLoopDefaultMode };
        CFRunLoop::run_in_mode(mode, RUN_LOOP_SLICE, false);
    }
    match handle.join() {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Registers for the commands, then hands each change to the main thread. Media
/// keys only reach the app the system considers to be playing: whichever last
/// published Now Playing info.
pub fn serve(
    controls: UnboundedSender<Control>,
    mut updates: UnboundedReceiver<Update>,
    skip: Duration,
) {
    DispatchQueue::main().exec_async(move || register(controls, skip));
    while let Some(update) = updates.blocking_recv() {
        if update.due() {
            DispatchQueue::main().exec_async(move || publish(&update.now));
        }
    }
}

fn register(controls: UnboundedSender<Control>, skip: Duration) {
    // SAFETY (throughout): plain MediaPlayer calls on the main thread, with
    // handlers of the signature the API documents.
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        let simple: [(Retained<MPRemoteCommand>, Control); 6] = [
            (center.playCommand(), Control::Play),
            (center.pauseCommand(), Control::Pause),
            (center.togglePlayPauseCommand(), Control::Toggle),
            (center.stopCommand(), Control::Stop),
            (center.nextTrackCommand(), Control::Next),
            (center.previousTrackCommand(), Control::Previous),
        ];
        for (command, control) in simple {
            let controls = controls.clone();
            add_handler(&command, move |_| Some(control), controls);
        }
        // The skip buttons Control Center shows, labelled with the skip interval.
        let intervals = NSArray::from_retained_slice(&[NSNumber::new_f64(skip.as_secs_f64())]);
        let forward = center.skipForwardCommand();
        forward.setPreferredIntervals(&intervals);
        add_handler(
            &forward,
            |event| {
                let event = event.cast::<MPSkipIntervalCommandEvent>();
                let interval = event.as_ref().interval();
                Some(Control::Forward(secs(interval)))
            },
            controls.clone(),
        );
        let backward = center.skipBackwardCommand();
        backward.setPreferredIntervals(&intervals);
        add_handler(
            &backward,
            |event| {
                let event = event.cast::<MPSkipIntervalCommandEvent>();
                let interval = event.as_ref().interval();
                Some(Control::Backward(secs(interval)))
            },
            controls.clone(),
        );
        add_handler(
            &center.changePlaybackPositionCommand(),
            |event| {
                let event = event.cast::<MPChangePlaybackPositionCommandEvent>();
                secs(event.as_ref().positionTime()).map(Control::SetPosition)
            },
            controls,
        );
    }
}

fn secs(value: f64) -> Option<Duration> {
    (value.is_finite() && value > 0.0).then(|| Duration::from_secs_f64(value))
}

/// # Safety
///
/// `read` gets the command's event, which it may cast to the event class the
/// command is documented to send.
unsafe fn add_handler(
    command: &MPRemoteCommand,
    read: impl Fn(NonNull<MPRemoteCommandEvent>) -> Option<Control> + 'static,
    controls: UnboundedSender<Control>,
) {
    let handler = RcBlock::new(
        move |event: NonNull<MPRemoteCommandEvent>| match read(event) {
            Some(control) if controls.send(control).is_ok() => {
                MPRemoteCommandHandlerStatus::Success
            }
            _ => MPRemoteCommandHandlerStatus::CommandFailed,
        },
    );
    // SAFETY: the caller's promise, and a handler of the documented signature.
    unsafe {
        command.setEnabled(true);
        command.addTargetWithHandler(&handler);
    }
}

thread_local! {
    /// The cover Now Playing shows, so it's made once per episode, not per update.
    static ARTWORK: RefCell<Option<(PathBuf, Retained<MPMediaItemArtwork>)>> =
        const { RefCell::new(None) };
}

fn publish(now: &Snapshot) {
    // SAFETY: MediaPlayer calls on the main thread, with values of the types each
    // key is documented to hold.
    unsafe {
        let center = MPNowPlayingInfoCenter::defaultCenter();
        let Some(track) = &now.track else {
            center.setNowPlayingInfo(None);
            center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
            return;
        };
        let rate = if now.status == Status::Playing {
            now.rate
        } else {
            0.0
        };
        let mut keys: Vec<&NSString> = vec![
            MPMediaItemPropertyTitle,
            MPMediaItemPropertyArtist,
            MPMediaItemPropertyAlbumTitle,
            MPMediaItemPropertyPlaybackDuration,
            MPNowPlayingInfoPropertyElapsedPlaybackTime,
            MPNowPlayingInfoPropertyPlaybackRate,
            MPNowPlayingInfoPropertyDefaultPlaybackRate,
        ];
        let mut values: Vec<Retained<AnyObject>> = vec![
            any(NSString::from_str(&track.title)),
            any(NSString::from_str(&track.podcast)),
            any(NSString::from_str(&track.podcast)),
            any(NSNumber::new_f64(track.duration.as_secs_f64())),
            any(NSNumber::new_f64(now.position.as_secs_f64())),
            any(NSNumber::new_f64(rate)),
            any(NSNumber::new_f64(1.0)),
        ];
        if let Some(artwork) = track.artwork_file.as_deref().and_then(artwork) {
            keys.push(MPMediaItemPropertyArtwork);
            values.push(any(artwork));
        }
        let info = NSDictionary::from_retained_objects(&keys, &values);
        center.setNowPlayingInfo(Some(&info));
        center.setPlaybackState(match now.status {
            Status::Playing => MPNowPlayingPlaybackState::Playing,
            Status::Paused => MPNowPlayingPlaybackState::Paused,
            Status::Stopped => MPNowPlayingPlaybackState::Stopped,
        });
    }
}

fn artwork(file: &Path) -> Option<Retained<MPMediaItemArtwork>> {
    ARTWORK.with_borrow_mut(|cached| {
        if let Some((path, artwork)) = cached.as_ref()
            && path == file
        {
            return Some(artwork.clone());
        }
        let image = NSImage::initWithContentsOfFile(
            NSImage::alloc(),
            &NSString::from_str(&file.to_string_lossy()),
        )?;
        let size = image.size();
        let handler = RcBlock::new(move |_: CGSize| NonNull::from(&*image));
        // SAFETY: the handler returns an image it keeps alive for as long as the
        // artwork holds the handler.
        let artwork = unsafe {
            MPMediaItemArtwork::initWithBoundsSize_requestHandler(
                MPMediaItemArtwork::alloc(),
                CGSize::new(size.width, size.height),
                &handler,
            )
        };
        *cached = Some((file.to_path_buf(), artwork.clone()));
        Some(artwork)
    })
}

fn any<T: Message>(object: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an `AnyObject`.
    unsafe { Retained::cast_unchecked(object) }
}
