// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Geometry in two coordinate spaces (05):
//! - physical pixels on the virtual desktop ([`PhysPx`], [`PhysPoint`], [`RectPhys`]);
//! - logical pixels on one monitor ([`LogPx`], [`LogPoint`]).
//!
//! The two never mix implicitly. The only conversion is [`MonitorTransform`]:
//! `logical = (phys - monitor.origin_phys) / monitor.scale_factor`.

/// Physical pixels, virtual-desktop space.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct PhysPx(pub f64);

/// Logical pixels on a specific monitor.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct LogPx(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MonitorId(pub u32);

/// A point on the virtual desktop, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PhysPoint {
    pub x: f64,
    pub y: f64,
}

impl PhysPoint {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A rectangle on the virtual desktop, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RectPhys {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl RectPhys {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn origin(&self) -> PhysPoint {
        PhysPoint::new(self.x, self.y)
    }

    /// Half-open: the left and top edges are inside, the right and bottom edges are not,
    /// so adjacent monitors never both contain a point.
    pub fn contains(&self, p: PhysPoint) -> bool {
        p.x >= self.x && p.x < self.x + self.width && p.y >= self.y && p.y < self.y + self.height
    }
}

/// A point in one monitor's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogPoint {
    pub monitor: MonitorId,
    pub x: LogPx,
    pub y: LogPx,
}

/// Converts between physical and one monitor's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorTransform {
    monitor: MonitorId,
    origin_phys: PhysPoint,
    scale_factor: f64,
}

impl MonitorTransform {
    /// Returns `None` if `scale_factor` is not a finite number above zero.
    pub fn new(monitor: MonitorId, origin_phys: PhysPoint, scale_factor: f64) -> Option<Self> {
        (scale_factor.is_finite() && scale_factor > 0.0).then_some(Self {
            monitor,
            origin_phys,
            scale_factor,
        })
    }

    pub fn monitor(&self) -> MonitorId {
        self.monitor
    }

    pub fn origin_phys(&self) -> PhysPoint {
        self.origin_phys
    }

    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    pub fn to_logical(&self, p: PhysPoint) -> LogPoint {
        LogPoint {
            monitor: self.monitor,
            x: LogPx((p.x - self.origin_phys.x) / self.scale_factor),
            y: LogPx((p.y - self.origin_phys.y) / self.scale_factor),
        }
    }

    /// Returns `None` if `p` belongs to another monitor.
    pub fn to_physical(&self, p: LogPoint) -> Option<PhysPoint> {
        (p.monitor == self.monitor).then(|| {
            PhysPoint::new(
                p.x.0 * self.scale_factor + self.origin_phys.x,
                p.y.0 * self.scale_factor + self.origin_phys.y,
            )
        })
    }

    /// Logical length to physical length (no origin offset).
    pub fn len_to_physical(&self, l: LogPx) -> PhysPx {
        PhysPx(l.0 * self.scale_factor)
    }

    /// Physical length to logical length (no origin offset).
    pub fn len_to_logical(&self, l: PhysPx) -> LogPx {
        LogPx(l.0 / self.scale_factor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform(origin: (f64, f64), scale: f64) -> MonitorTransform {
        MonitorTransform::new(MonitorId(1), PhysPoint::new(origin.0, origin.1), scale)
            .expect("valid scale")
    }

    #[test]
    fn primary_monitor_at_100_percent_is_identity() {
        let t = transform((0.0, 0.0), 1.0);
        let l = t.to_logical(PhysPoint::new(960.0, 540.0));
        assert_eq!((l.x, l.y), (LogPx(960.0), LogPx(540.0)));
    }

    #[test]
    fn scaled_monitor_divides_by_scale() {
        let t = transform((0.0, 0.0), 1.5);
        let l = t.to_logical(PhysPoint::new(300.0, 150.0));
        assert_eq!((l.x, l.y), (LogPx(200.0), LogPx(100.0)));
    }

    #[test]
    fn monitor_left_of_primary_has_negative_origin() {
        // 2560 px wide monitor at 200 % placed left of and above the primary.
        let t = transform((-2560.0, -400.0), 2.0);
        let l = t.to_logical(PhysPoint::new(-2560.0, -400.0));
        assert_eq!((l.x, l.y), (LogPx(0.0), LogPx(0.0)));
        let l = t.to_logical(PhysPoint::new(-1.0, 0.0));
        assert_eq!((l.x, l.y), (LogPx(1279.5), LogPx(200.0)));
    }

    #[test]
    fn round_trip_restores_physical_point() {
        let t = transform((-1920.0, 120.0), 1.25);
        let p = PhysPoint::new(-777.0, 999.0);
        assert_eq!(t.to_physical(t.to_logical(p)), Some(p));
    }

    #[test]
    fn to_physical_rejects_point_from_other_monitor() {
        let t = transform((0.0, 0.0), 1.0);
        let other = LogPoint {
            monitor: MonitorId(2),
            x: LogPx(1.0),
            y: LogPx(1.0),
        };
        assert_eq!(t.to_physical(other), None);
    }

    #[test]
    fn lengths_scale_without_offset() {
        let t = transform((-1000.0, -1000.0), 1.5);
        assert_eq!(t.len_to_physical(LogPx(10.0)), PhysPx(15.0));
        assert_eq!(t.len_to_logical(PhysPx(15.0)), LogPx(10.0));
    }

    #[test]
    fn invalid_scale_is_rejected() {
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(MonitorTransform::new(MonitorId(0), PhysPoint::default(), scale).is_none());
        }
    }

    #[test]
    fn rect_contains_is_half_open() {
        let r = RectPhys::new(-1920.0, 0.0, 1920.0, 1080.0);
        assert!(r.contains(PhysPoint::new(-1920.0, 0.0)));
        assert!(r.contains(PhysPoint::new(-0.5, 1079.5)));
        assert!(!r.contains(PhysPoint::new(0.0, 10.0)));
        assert!(!r.contains(PhysPoint::new(-10.0, 1080.0)));
    }
}
