// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Source adapters and how they send signals to the engine (03, 01 "Threads").

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::Arc;

use uca_types::{Signal, SourceId};

/// Capacity of the engine's bounded channel (01, decided).
pub const ENGINE_CHANNEL_CAPACITY: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Pointer,
    Trigger,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("the source could not start: {0}")]
    Start(String),
}

/// Every input device is wrapped by an adapter implementing this (03).
pub trait InputSource: Send {
    fn id(&self) -> SourceId;
    fn kind(&self) -> SourceKind;
    fn start(&mut self, tx: SignalSender) -> Result<(), SourceError>;
    fn stop(&mut self);
}

/// The engine side has gone; the adapter should stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the engine channel is closed")]
pub struct Disconnected;

trait Sink: Send + Sync {
    fn try_send(&self, s: Signal) -> Result<(), TrySendError<()>>;
    fn send(&self, s: Signal) -> Result<(), Disconnected>;
}

struct ChannelSink<M> {
    tx: SyncSender<M>,
    wrap: fn(Signal) -> M,
}

impl<M: Send + 'static> Sink for ChannelSink<M> {
    fn try_send(&self, s: Signal) -> Result<(), TrySendError<()>> {
        self.tx.try_send((self.wrap)(s)).map_err(|e| match e {
            TrySendError::Full(_) => TrySendError::Full(()),
            TrySendError::Disconnected(_) => TrySendError::Disconnected(()),
        })
    }

    fn send(&self, s: Signal) -> Result<(), Disconnected> {
        self.tx.send((self.wrap)(s)).map_err(|_| Disconnected)
    }
}

/// Sends signals into the engine channel with the decided policy (01):
/// when full, `Point` samples are dropped and counted; everything else waits.
/// Do not call from an OS hook callback: hand off to the adapter's thread first.
#[derive(Clone)]
pub struct SignalSender {
    sink: Arc<dyn Sink>,
    dropped: Arc<AtomicU64>,
}

impl SignalSender {
    /// `wrap` turns a signal into the engine's message type.
    pub fn new<M: Send + 'static>(tx: SyncSender<M>, wrap: fn(Signal) -> M) -> Self {
        Self {
            sink: Arc::new(ChannelSink { tx, wrap }),
            dropped: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn send(&self, signal: Signal) -> Result<(), Disconnected> {
        if matches!(signal, Signal::Point(_)) {
            return match self.sink.try_send(signal) {
                Ok(()) => Ok(()),
                Err(TrySendError::Full(())) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
                Err(TrySendError::Disconnected(())) => Err(Disconnected),
            };
        }
        self.sink.send(signal)
    }

    /// Point samples dropped because the channel was full, since the last call.
    pub fn take_dropped(&self) -> u64 {
        self.dropped.swap(0, Ordering::Relaxed)
    }
}

impl std::fmt::Debug for SignalSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalSender")
            .field("dropped", &self.dropped.load(Ordering::Relaxed))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use uca_types::{Clock, FakeClock, Phase, PhysPoint, PointSample, TriggerEvent, TriggerId};

    use super::*;

    #[test]
    fn full_channel_drops_points_but_not_triggers() {
        let (tx, rx) = mpsc::sync_channel::<Signal>(1);
        let sender = SignalSender::new(tx, |s| s);
        let clock = FakeClock::new();
        let point = || {
            Signal::Point(PointSample {
                t: clock.now(),
                source: SourceId("mouse".into()),
                pos: PhysPoint::new(1.0, 1.0),
                monitor: None,
                confidence: 1.0,
                presence: true,
            })
        };
        sender.send(point()).expect("first fits");
        sender.send(point()).expect("dropped, not an error");
        assert_eq!(sender.take_dropped(), 1);
        assert_eq!(sender.take_dropped(), 0);

        let trigger = Signal::Trigger(TriggerEvent {
            t: clock.now(),
            source: SourceId("keyboard".into()),
            trigger: TriggerId::parse("key:F13").expect("t"),
            phase: Phase::Up,
        });
        let s2 = sender.clone();
        let handle = std::thread::spawn(move || s2.send(trigger));
        // The trigger waits for room instead of being dropped.
        assert!(matches!(rx.recv(), Ok(Signal::Point(_))));
        assert!(matches!(rx.recv(), Ok(Signal::Trigger(_))));
        assert_eq!(handle.join().ok(), Some(Ok(())));

        drop(rx);
        assert_eq!(sender.send(point()), Err(Disconnected));
    }
}
