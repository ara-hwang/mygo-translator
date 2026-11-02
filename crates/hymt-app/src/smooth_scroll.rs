//! Smooth mouse-wheel scrolling for `overflow_y_scroll` containers.
//!
//! GPUI applies wheel deltas to a scroll container instantly, one event at a time.
//! [`SmoothScroller`] wraps a `ScrollHandle` so wheel input only moves an eased
//! target and a per-frame loop eases the handle's offset toward it. The pure
//! easing math lives in [`SmoothScroll`] and is unit-tested without GPUI.

use gpui_kit::{point, px, ScrollEvent, ScrollHandle, Window};

/// Share of the remaining distance the chase covers per frame (tuned for 60 fps).
const EASE_FACTOR: f32 = 0.22;
/// Remaining distance, in px, below which the chase counts as settled.
const SETTLE_PX: f32 = 0.5;

/// Eased scroll bookkeeping. `current` chases `target`; both are stored as the
/// distance scrolled away from the top in px (0 = top, larger = further down),
/// which is the opposite sign of GPUI's native offset (negative scrolling down).
#[derive(Debug, Clone, PartialEq, Default)]
struct SmoothScroll {
    /// Distance currently shown.
    current: f32,
    /// Distance the wheel has asked for.
    target: f32,
}

impl SmoothScroll {
    /// Adds one wheel delta; a positive delta scrolls further down.
    fn push_delta(&mut self, delta: f32) {
        self.target += delta;
    }

    /// Runs one animation step clamped to `max` (the container's max scroll
    /// distance) and returns the position to show plus whether more frames
    /// are needed.
    fn tick(&mut self, max: f32) -> (f32, bool) {
        let max = max.max(0.0);
        self.target = self.target.clamp(0.0, max);
        let difference = self.target - self.current;
        if difference.abs() <= SETTLE_PX {
            self.current = self.target;
            return (self.current, false);
        }
        self.current = (self.current + difference * EASE_FACTOR).clamp(0.0, max);
        (self.current, true)
    }
}

/// GPUI glue: a scroll handle plus the eased target attached to it. One instance
/// per scrollable container; the frame loop that drives it lives in `root.rs`.
#[derive(Default, Debug, Clone)]
pub struct SmoothScroller {
    anim: SmoothScroll,
    /// True while the eased chase already has frames scheduled.
    chasing: bool,
    handle: ScrollHandle,
}

impl SmoothScroller {
    /// Handle to attach to a container via `.track_scroll()`.
    pub fn handle(&self) -> &ScrollHandle {
        &self.handle
    }

    /// Feeds one wheel event into the eased target and rewinds the instant jump
    /// the container's native scroll handler already applied, so the only motion
    /// on screen comes from the eased chase.
    pub fn on_wheel(&mut self, event: &ScrollEvent, window: &Window) {
        let delta = event.delta.pixel_delta(window.line_height()).y.as_f32();
        if delta == 0.0 {
            return;
        }
        // Native offset decreases as the user scrolls down, so invert the sign.
        self.anim.push_delta(-delta);
        self.handle
            .set_offset(point(self.handle.offset().x, px(-self.anim.current)));
    }

    /// Whether the chase currently has frames scheduled.
    pub fn is_chasing(&self) -> bool {
        self.chasing
    }

    /// Starts or stops scheduling the per-frame chase.
    pub fn set_chasing(&mut self, chasing: bool) {
        self.chasing = chasing;
    }

    /// Runs one eased chase step against the handle's freshly painted max
    /// offset and applies the result to the handle. Returns whether another
    /// frame is needed.
    pub fn chase_step(&mut self) -> bool {
        let max = self.handle.max_offset().y.as_f32();
        let (position, more) = self.anim.tick(max);
        self.handle
            .set_offset(point(self.handle.offset().x, px(-position)));
        more
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chase_runs_until_settled() {
        let mut anim = SmoothScroll::default();
        anim.push_delta(3000.0);

        let mut frames = 0usize;
        let mut previous = anim.current;
        let mut more = true;
        while more && frames < 1000 {
            let (position, keep) = anim.tick(10_000.0);
            assert!(position > previous, "chase must move forward monotonically");
            previous = position;
            more = keep;
            frames += 1;
        }
        assert!(!more, "chase must settle");
        assert_eq!(previous, 3000.0);
        assert!(frames > 1, "the chase must take multiple frames");
    }

    #[test]
    fn push_delta_accumulates_while_chasing() {
        let mut anim = SmoothScroll::default();
        anim.push_delta(200.0);
        let (_, more) = anim.tick(10_000.0);
        assert!(more, "one wheel push starts a chase");

        // A user pushing more mid-chase extends the target from the same origin.
        anim.push_delta(100.0);
        let mut more = true;
        while more {
            let (_, keep) = anim.tick(10_000.0);
            more = keep;
        }
        assert_eq!(anim.current, 300.0);
    }

    #[test]
    fn ticks_are_clamped_to_max() {
        let mut anim = SmoothScroll::default();
        anim.push_delta(5000.0);
        let mut more = true;
        while more {
            let (position, keep) = anim.tick(1200.0);
            assert!(position <= 1200.0);
            more = keep;
        }
        assert_eq!(anim.current, 1200.0);
        assert_eq!(anim.target, 1200.0);
        // Scrolling back up after clamping still works.
        anim.push_delta(-800.0);
        let mut more = true;
        while more {
            let (position, keep) = anim.tick(1200.0);
            assert!(position <= 1200.0);
            more = keep;
        }
        assert_eq!(anim.current, 400.0);
    }

    #[test]
    fn negative_max_is_treated_as_zero() {
        let mut anim = SmoothScroll::default();
        anim.push_delta(300.0);
        let (position, more) = anim.tick(-1.0);
        assert_eq!(position, 0.0);
        assert!(!more);
    }

    #[test]
    fn chase_handles_multiple_directions() {
        let mut anim = SmoothScroll::default();
        anim.push_delta(400.0);
        let mut more = true;
        while more {
            let (position, keep) = anim.tick(10_000.0);
            assert!(position >= 0.0);
            more = keep;
        }
        assert_eq!(anim.current, 400.0);
        // Scrolling down further, partly cancelling the first delta with an up
        // scroll, lands on the net distance without ever going negative.
        anim.push_delta(300.0);
        anim.push_delta(-200.0);
        let mut more = true;
        while more {
            let (position, keep) = anim.tick(10_000.0);
            assert!(position >= 0.0);
            more = keep;
        }
        assert_eq!(anim.current, 500.0);
    }
}
