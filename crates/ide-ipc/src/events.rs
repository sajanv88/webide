//! Messages from the backend to the webview, and the guard that keeps the
//! window open while it has unsaved changes.
//!
//! The webview gives us a Tauri `Channel` through `subscribe_events`, so it
//! needs no event permission: it can only hear what we send it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use tauri::ipc::Channel;

use crate::dto::{BackendEventDto, WindowStateDto};

/// The page's channel, and whether closing now would lose unsaved changes.
#[derive(Default)]
pub struct EventHub {
    channel: Mutex<Option<Channel<BackendEventDto>>>,
    unsaved: AtomicBool,
    /// The window's state as last sent, so resizing sends only changes.
    window: Mutex<WindowStateDto>,
}

impl std::fmt::Debug for EventHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventHub")
            .field("unsaved", &self.unsaved.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl EventHub {
    /// A page subscribed: it replaces the previous page's channel, and starts
    /// with nothing unsaved.
    pub fn subscribe(&self, channel: Channel<BackendEventDto>) {
        *self.lock() = Some(channel);
        self.unsaved.store(false, Ordering::Release);
    }

    /// A page started loading: the old one is gone, with its channel and its edits.
    pub fn reset(&self) {
        *self.lock() = None;
        self.unsaved.store(false, Ordering::Release);
    }

    /// Sends to the page, if one is listening. Returns whether it was sent.
    pub fn send(&self, event: BackendEventDto) -> bool {
        let channel = self.lock().clone();
        channel.is_some_and(|channel| channel.send(event).is_ok())
    }

    /// Keeps the window's `state`; returns whether it differs from before.
    pub fn window_state_changed(&self, state: WindowStateDto) -> bool {
        let mut kept = self.window.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = *kept != state;
        *kept = state;
        changed
    }

    pub fn set_unsaved(&self, unsaved: bool) {
        self.unsaved.store(unsaved, Ordering::Release);
    }

    /// The window is about to close. Returns `true` to keep it open: there are
    /// unsaved changes and the page was asked what to do with them. When no
    /// page is listening, nothing could answer, so it closes.
    pub fn hold_close(&self) -> bool {
        self.unsaved.load(Ordering::Acquire) && self.send(BackendEventDto::CloseRequested)
    }

    /// The page has dealt with its unsaved changes and closes the window.
    pub fn release_close(&self) {
        self.unsaved.store(false, Ordering::Release);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Channel<BackendEventDto>>> {
        self.channel.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
