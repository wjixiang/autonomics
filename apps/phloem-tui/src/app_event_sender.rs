//! A cloneable handle for sending `AppEvent`s into the main loop's channel.
//!
//! Uses `std::sync::mpsc` because the main loop is synchronous. The standard
//! channel is unbounded (no capacity limit). If the app later moves to an
//! async loop (as codex does), switch to `tokio::sync::mpsc`.

use std::sync::mpsc::Sender;

use crate::app_event::AppEvent;

#[derive(Clone, Debug)]
pub(crate) struct AppEventSender {
    pub tx: Sender<AppEvent>,
}

impl AppEventSender {
    pub fn new(tx: Sender<AppEvent>) -> Self {
        Self { tx }
    }

    /// Send an event. Errors are logged but not propagated — a disconnected
    /// receiver means the app is shutting down.
    pub fn send(&self, event: AppEvent) {
        if let Err(e) = self.tx.send(event) {
            tracing::error!("app event send failed (receiver likely dropped): {e}");
        }
    }
}
