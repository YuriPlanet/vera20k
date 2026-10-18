//! One process speech bubble: original70F120 and TechnoExtras6F5E8D.
//! The deterministic simulation owns immediate reveal. This owner consumes
//! ordered cues and reads the existing system clock only for presentation.

use crate::sim::timer::CdTimer;

#[derive(Debug, Default)]
pub(crate) struct TalkBubble {
    actor: Option<u64>,
    value: i32,
    timer: CdTimer,
}

impl TalkBubble {
    pub(crate) fn set(&mut self, actor: Option<u64>, value: i32, duration: i32, wall_ms: u64) {
        //70F126/12A tests nonzero, not positivity. A negative value still
        //replaces the old bubble and reveals; only DrawExtras rejects it.
        self.actor = actor.filter(|_| value != 0);
        self.value = if self.actor.is_some() { value } else { 0 };
        self.timer = CdTimer::started(
            ((wall_ms as u32) >> 4) as i32,
            if self.actor.is_some() { duration } else { 0 },
        );
    }

    pub(crate) fn frame_for(&self, actor: u64, wall_ms: u64) -> Option<usize> {
        (self.actor == Some(actor)
            && self.value > 0
            && self.timer.remaining(((wall_ms as u32) >> 4) as i32) > 0)
            .then(|| (self.value - 1) as usize)
    }

    /// Native stores the actor in process globalB0EB38. Its setter/draw
    /// reader are established here; exhaustive cross-load reset/swizzling
    /// behavior remains outside this coverage. VERA retires the local effect
    /// at world replacement so a new world cannot reuse the old target ID.
    pub(crate) fn clear_on_load(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_requests_use_the_wall_clock_and_preserve_signed_gates() {
        let mut b = TalkBubble::default();
        b.set(Some(7), 3, 300, 1600);
        assert_eq!(b.frame_for(7, 1600), Some(2));
        assert_eq!(b.frame_for(8, 1600), None);
        assert_eq!(b.frame_for(7, 1600 + 299 * 16), Some(2));
        assert_eq!(b.frame_for(7, 1600 + 300 * 16), None);
        b.set(Some(8), -1, 300, 2000);
        assert_eq!(b.actor, Some(8));
        assert_eq!(b.frame_for(7, 2000), None);
        assert_eq!(b.frame_for(8, 2000), None);
        b.set(Some(7), 1, 0, 2000);
        assert_eq!(b.frame_for(7, 2000), None);
        b.set(Some(7), 1, 300, 2000);
        b.set(None, 1, 300, 2000);
        assert_eq!(b.actor, None);
        b.set(Some(7), 1, 300, 2000);
        b.clear_on_load();
        assert_eq!(b.frame_for(7, 2000), None);
    }
}
