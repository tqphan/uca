// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Normalized input signals (01).

use std::time::Instant;

use smol_str::SmolStr;

use crate::geometry::{LogPoint, MonitorId, PhysPoint};
use crate::ids::SourceId;
use crate::trigger::TriggerId;

#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    Point(PointSample),
    Trigger(TriggerEvent),
    Gesture(GestureEvent),
    /// Head orientation and position, for mouse-look (12).
    HeadPose(HeadPoseSample),
    SourceStatus(SourceStatusEvent),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PointSample {
    pub t: Instant,
    pub source: SourceId,
    pub pos: PhysPoint,
    pub monitor: Option<MonitorId>,
    /// 0..1
    pub confidence: f32,
    /// User detected; `false` pauses dwell (SAFETY-03).
    pub presence: bool,
}

/// A pointer sample after the PointPipeline (03): valid, smoothed and mapped to a monitor.
#[derive(Debug, Clone, PartialEq)]
pub struct FilteredPoint {
    pub t: Instant,
    pub phys: PhysPoint,
    pub monitor: MonitorId,
    pub logical: LogPoint,
    pub presence: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Down,
    Up,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TriggerEvent {
    pub t: Instant,
    pub source: SourceId,
    pub trigger: TriggerId,
    pub phase: Phase,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GestureEvent {
    pub t: Instant,
    pub source: SourceId,
    pub gesture: SmolStr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeadPoseSample {
    pub t: Instant,
    pub source: SourceId,
    /// Radians.
    pub yaw: f64,
    /// Radians.
    pub pitch: f64,
    /// Radians.
    pub roll: f64,
    /// Metres, `[x, y, z]`; all three or none.
    pub position: Option<[f64; 3]>,
    /// 0..1
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceStatus {
    Connected,
    Disconnected,
    UserLost,
    UserFound,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceStatusEvent {
    pub t: Instant,
    pub source: SourceId,
    pub status: SourceStatus,
}

impl Signal {
    pub fn time(&self) -> Instant {
        match self {
            Signal::Point(s) => s.t,
            Signal::Trigger(s) => s.t,
            Signal::Gesture(s) => s.t,
            Signal::HeadPose(s) => s.t,
            Signal::SourceStatus(s) => s.t,
        }
    }

    pub fn source(&self) -> &SourceId {
        match self {
            Signal::Point(s) => &s.source,
            Signal::Trigger(s) => &s.source,
            Signal::Gesture(s) => &s.source,
            Signal::HeadPose(s) => &s.source,
            Signal::SourceStatus(s) => &s.source,
        }
    }
}
