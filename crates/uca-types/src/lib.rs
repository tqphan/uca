// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared types: geometry, signals, ids, Clock trait.

pub mod clock;
pub mod geometry;
pub mod ids;
pub mod key;
pub mod signal;
pub mod suggest;
pub mod trigger;

pub use clock::{Clock, FakeClock, SystemClock};
pub use geometry::{LogPoint, LogPx, MonitorId, MonitorTransform, PhysPoint, PhysPx, RectPhys};
pub use ids::{ItemId, SourceId};
pub use key::{Key, KeyError};
pub use signal::{
    GestureEvent, HeadPoseSample, Phase, PointSample, Signal, SourceStatus, SourceStatusEvent,
    TriggerEvent,
};
pub use trigger::{TriggerId, TriggerIdError, TriggerKind};
