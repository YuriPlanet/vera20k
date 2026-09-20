//! Timer-based 16-bit facing interpolator, mirroring gamemd's FacingClass primitive.
//!
//! At any binary frame, the animated value is a pure function of state +
//! frame: `current(frame) = current - (diff / step_size) * remaining`, where
//! `step_size = abs(diff) / rot_per_frame` and `remaining = duration - elapsed`.
//! The per-step rate spreads the full signed arc evenly across the rotation's
//! frame count using integer division; elapsed=0 can differ slightly from `prev`
//! when the arc is not divisible by the frame count. Setting a new target snapshots the current animated value into
//! `prev` so rotations retarget smoothly without snap-back.
//!
//! Verified against gamemd.exe — see
//! ra2-rust-game-docs/UNITCLASS_TURRET_TRACKING_AND_FIRE_TIMING_GHIDRA_REPORT.md
//! §1.3 (24-byte byte layout), §2.1 (Current interpolation), §2.4 (Set
//! semantics), §2.7 (SetROT clamp + shift).
//!
//! ## Dependency rules
//! - Part of sim/ — depends only on serde and std.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FacingClass {
    /// Destination — where the rotation will end up. 16-bit DirStruct.
    current: u16,
    /// Where the current rotation began. Updated on `set` to the animated
    /// value at the moment of the new request, so retargets continue
    /// smoothly from the visible position.
    prev: u16,
    /// Binary frame when the rotation began. None = never started.
    start_frame: Option<u32>,
    /// Total binary frames needed to complete the rotation. When this is
    /// 0, `current()` returns `current` immediately (snap-on-step<1).
    duration_frames: u16,
    /// Per-frame step in 16-bit facing units. Stored as `(rot_byte << 8)`.
    /// Zero means instant rotator (no interpolation).
    rot_per_frame: u16,
}

impl FacingClass {
    /// Construct a new FacingClass at the given initial facing with the
    /// given ROT byte. ROT byte is the value from rules.ini (e.g. 5 for
    /// War Miner, 10 for Harvester) before the binary's <<8 shift.
    pub fn new(initial: u16, rot_byte: u8) -> Self {
        let mut fc = Self {
            current: initial,
            prev: initial,
            start_frame: None,
            duration_frames: 0,
            rot_per_frame: 0,
        };
        fc.set_rot(rot_byte);
        fc
    }

    /// Update the rate of turn. Mirrors gamemd's SetROT (`FacingClass::Set_ROT` @ `0x004C9680`):
    /// clamps input > 126 to 127, then stores `(byte << 8)`.
    pub fn set_rot(&mut self, rot_byte: u8) {
        let clamped: u8 = if rot_byte > 0x7E { 0x7F } else { rot_byte };
        self.rot_per_frame = (clamped as u16) << 8;
    }

    /// Destination facing — where the rotation will end (regardless of
    /// where the animation currently is).
    pub fn destination(&self) -> u16 {
        self.current
    }

    /// Per-frame step value, exposed for tests and for callers that need
    /// to know the rotation rate.
    pub fn rot_per_frame(&self) -> u16 {
        self.rot_per_frame
    }

    #[cfg(test)]
    pub(crate) fn timer_start_frame(&self) -> Option<u32> {
        self.start_frame
    }

    /// Animated facing at the given binary frame. Pure function of state.
    ///
    /// Returns `current` when:
    /// - rot_per_frame == 0 (instant rotator)
    /// - start_frame is None (no rotation initiated)
    /// - elapsed >= duration_frames (rotation complete)
    /// - step_size < 1 (rotation request smaller than one frame's ROT — snaps)
    ///
    /// Otherwise interpolates linearly along the shortest signed arc from
    /// prev to current at the per-step rate `diff / step_size` (the arc spread
    /// evenly across the rotation's frame count).
    pub fn current(&self, binary_frame: u32) -> u16 {
        if self.rot_per_frame == 0 {
            return self.current;
        }
        let Some(start) = self.start_frame else {
            return self.current;
        };
        let elapsed = (binary_frame as i32).wrapping_sub(start as i32);
        let remaining = i32::from(self.duration_frames).wrapping_sub(elapsed).max(0);
        if remaining == 0 {
            return self.current;
        }

        // Signed short subtraction gives shortest signed delta.
        // 0xFFE0 → 0x0010 wraps to +0x30, not -0xFFD0.
        let diff: i16 = self.current.wrapping_sub(self.prev) as i16;

        // step_size < 1 snaps (research doc §2.2).
        let step_size: u16 = diff.unsigned_abs() / self.rot_per_frame;
        if step_size < 1 {
            return self.current;
        }

        // Per-step is the full signed arc spread evenly across the rotation's
        // frame count (`diff / step_size`, integer-truncated toward zero), not a
        // fixed `rot_per_frame`. When abs(diff) is not an exact multiple of
        // rot_per_frame, the second division can leave a sub-byte remainder:
        // 0x4000 -> 0xC000 at ROT5 samples 0x3FEE at elapsed0, not 0x4000.
        // Original sample: tools/spatial_oracle/drive_fresh_turn.json.
        let per_step: i32 = (diff as i32) / (step_size as i32);
        let delta = per_step.wrapping_mul(remaining);
        i32::from(self.current).wrapping_sub(delta) as u16
    }

    /// Smooth setter — initiates a new rotation toward `new_target`.
    /// Snapshots the current animated position into `prev` (research doc
    /// §2.4) so retargets continue smoothly from the visible position.
    ///
    /// No-op when `new_target == current`. Returns true if state changed.
    pub fn set(&mut self, new_target: u16, binary_frame: u32) -> bool {
        if new_target == self.current {
            return false;
        }
        if self.rot_per_frame > 0 {
            // Snapshot animated value into prev BEFORE writing new target.
            self.prev = self.current(binary_frame);
        } else {
            self.prev = self.current;
        }
        self.current = new_target;
        if self.rot_per_frame > 0 {
            let diff: i16 = self.current.wrapping_sub(self.prev) as i16;
            self.duration_frames = diff.unsigned_abs() / self.rot_per_frame;
            self.start_frame = Some(binary_frame);
        }
        true
    }

    /// Snap setter — writes target to both current and prev, resets the
    /// timer. Mirrors `FacingClass::UpdateFacing` @ `0x004C9300`.
    ///
    /// Its primary consumers are not spawn paths: `DriveLocomotionClass::
    /// Process_Drive_Track` @ `0x004B1AC1` and its Ship twin @ `0x006A10FD`
    /// snap the **body facing to each consumed track point's facing byte**,
    /// so a Drive unit's hull facing is snapped rather than interpolated for
    /// the whole duration of a curve. Spawn, takeoff and deploy use it too.
    /// Returns true if the destination changed.
    pub fn snap(&mut self, new_target: u16, binary_frame: u32) -> bool {
        let animated = self.current(binary_frame);
        if animated == new_target && self.current == new_target {
            self.start_frame = Some(binary_frame);
            self.duration_frames = 0;
            return false;
        }
        self.current = new_target;
        self.prev = new_target;
        self.start_frame = Some(binary_frame);
        self.duration_frames = 0;
        true
    }

    /// Whether a rotation is currently in progress at the given binary
    /// frame. Equivalent to gamemd's CDTimerClass::Remaining test on
    /// FacingClass (research doc §5.5, §1.5). Computed on demand —
    /// not cached.
    pub fn is_rotating(&self, binary_frame: u32) -> bool {
        if self.rot_per_frame == 0 {
            return false;
        }
        let Some(start) = self.start_frame else {
            return false;
        };
        let elapsed = (binary_frame as i32).wrapping_sub(start as i32);
        i32::from(self.duration_frames).wrapping_sub(elapsed).max(0) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_initializes_at_given_facing() {
        let fc = FacingClass::new(12345, 5);
        assert_eq!(fc.destination(), 12345);
        assert_eq!(fc.rot_per_frame(), 5 * 256); // 5 << 8 = 1280
    }

    #[test]
    fn set_rot_clamps_at_0x7f() {
        let mut fc = FacingClass::new(0, 0);
        fc.set_rot(0x7E);
        assert_eq!(fc.rot_per_frame(), 0x7E00); // 126 << 8
        fc.set_rot(0x7F);
        assert_eq!(fc.rot_per_frame(), 0x7F00); // 127 << 8
        fc.set_rot(0xFF);
        assert_eq!(fc.rot_per_frame(), 0x7F00); // clamped to 127, 127 << 8
        fc.set_rot(200);
        assert_eq!(fc.rot_per_frame(), 0x7F00); // clamped
    }

    #[test]
    fn set_rot_zero_means_instant() {
        let mut fc = FacingClass::new(100, 5);
        fc.set_rot(0);
        assert_eq!(fc.rot_per_frame(), 0);
    }

    #[test]
    fn gsi_05_07_same_target_snap_restarts_timer_epoch() {
        let mut fc = mid_rotation(0, 0x4000, 10, 16, 4);
        assert_eq!(fc.current(26), 0x4000);

        assert!(!fc.snap(0x4000, 77));

        assert_eq!(fc.start_frame, Some(77));
        assert_eq!(fc.duration_frames, 0);
        assert!(!fc.is_rotating(77));
        assert_eq!(fc.current(77), 0x4000);
    }

    /// Helper: construct a FacingClass mid-rotation (skips set() so we can
    /// test current() in isolation).
    fn mid_rotation(
        prev: u16,
        current: u16,
        start: u32,
        duration: u16,
        rot_byte: u8,
    ) -> FacingClass {
        let mut fc = FacingClass::new(current, rot_byte);
        fc.prev = prev;
        fc.start_frame = Some(start);
        fc.duration_frames = duration;
        fc
    }

    #[test]
    fn current_returns_destination_when_rot_zero() {
        let mut fc = mid_rotation(0, 1000, 0, 10, 5);
        fc.set_rot(0);
        assert_eq!(fc.current(0), 1000);
        assert_eq!(fc.current(5), 1000);
        assert_eq!(fc.current(100), 1000);
    }

    #[test]
    fn current_returns_destination_when_no_start_frame() {
        let fc = FacingClass::new(1000, 5);
        // start_frame = None
        assert_eq!(fc.current(0), 1000);
        assert_eq!(fc.current(50), 1000);
    }

    #[test]
    fn current_returns_destination_when_elapsed_exceeds_duration() {
        // prev=0, current=12800 (10 frames at ROT=5 → 1280/frame), duration=10.
        let fc = mid_rotation(0, 12800, 0, 10, 5);
        assert_eq!(fc.current(10), 12800); // exactly at end
        assert_eq!(fc.current(15), 12800); // past end
        assert_eq!(fc.current(100), 12800);
    }

    #[test]
    fn current_interpolates_linearly() {
        // prev=0, current=12800 (= 10 frames * 1280), duration=10.
        // At elapsed=5, animated = 0 + 5 * 1280 = 6400.
        // Equivalently: animated = current - 5 * 1280 = 12800 - 6400 = 6400.
        let fc = mid_rotation(0, 12800, 0, 10, 5);
        assert_eq!(fc.current(0), 0); // remaining=10, animated = 12800 - 1280*10 = 0
        assert_eq!(fc.current(1), 1280);
        assert_eq!(fc.current(5), 6400);
        assert_eq!(fc.current(9), 11520);
    }

    #[test]
    fn current_non_multiple_arc_lands_on_prev_at_start() {
        // Regression: when abs(diff) is NOT an exact multiple of rot_per_frame,
        // the per-step rate must absorb the remainder (per_step = diff/step_size)
        // so elapsed=0 lands exactly on prev. The old fixed-rot_per_frame step
        // left a `diff % rot_per_frame` gap (here 12900 % 1280 = 100 units).
        // prev=0, current=12900, ROT byte 5 (rot_per_frame=1280).
        // step_size = 12900/1280 = 10; per_step = 12900/10 = 1290.
        let fc = mid_rotation(0, 12900, 0, 10, 5);
        // elapsed=0, remaining=10: animated = 12900 - 1290*10 = 0 == prev.
        assert_eq!(fc.current(0), 0);
        // The old (wrong) fixed-step formula gave 12900 - 1280*10 = 100 here.
        assert_ne!(fc.current(0), 100);
        // Endpoint reached exactly at duration.
        assert_eq!(fc.current(10), 12900);
        // Mid-rotation interpolates at the spread per-step rate (1290/frame).
        assert_eq!(fc.current(5), 12900 - 1290 * 5); // 6450
    }

    #[test]
    fn current_non_multiple_arc_negative_diff_lands_on_prev() {
        // Same as above for a negative (counter-clockwise) arc. prev=12900,
        // current=0 → diff = -12900. step_size = 12900/1280 = 10;
        // per_step = -12900/10 = -1290 (truncates toward zero).
        let fc = mid_rotation(12900, 0, 0, 10, 5);
        // elapsed=0, remaining=10: animated = 0 - (-1290*10) = 12900 == prev.
        assert_eq!(fc.current(0), 12900);
        assert_eq!(fc.current(10), 0);
        assert_eq!(fc.current(5), 1290 * 5); // 6450
    }

    #[test]
    fn current_snaps_when_step_size_below_one() {
        // diff = current - prev = 100; rot_per_frame = 1280; step_size = 100/1280 = 0.
        // Should snap to current immediately.
        let fc = mid_rotation(0, 100, 0, 0, 5);
        assert_eq!(fc.current(0), 100);
    }

    #[test]
    fn current_handles_wrap_around_short_path() {
        // 0xFFE0 → 0x0010: shortest signed delta is +0x30 (48 units), not -0xFFD0.
        // ROT=1 byte → 256/frame; duration = 48/256 = 0 → snaps. Use larger arc.
        // Let's go 0xFF00 → 0x0100: signed diff = 0x0100 - 0xFF00 (as i16) = 0x0200 = 512.
        // ROT=1, rot_per_frame=256; duration = 512/256 = 2.
        let fc = mid_rotation(0xFF00, 0x0100, 0, 2, 1);
        // At elapsed=0: animated = current - sign(+) * 256 * 2 = 0x0100 - 512 = 0xFF00.
        assert_eq!(fc.current(0), 0xFF00);
        // At elapsed=1: animated = 0x0100 - 256 = 0 (i.e., 0x0000, just past wrap).
        assert_eq!(fc.current(1), 0x0000);
        // At elapsed=2: complete.
        assert_eq!(fc.current(2), 0x0100);
    }

    #[test]
    fn current_handles_wrap_around_short_path_negative_diff() {
        // 0x0100 → 0xFF00: signed diff = (0xFF00 - 0x0100) as i16 = 0xFE00 = -512.
        // shortest path is COUNTER-clockwise by 512 units (back through 0).
        let fc = mid_rotation(0x0100, 0xFF00, 0, 2, 1);
        // At elapsed=0: animated = 0xFF00 - sign(-) * 256 * 2 = 0xFF00 + 512 = 0x0100.
        assert_eq!(fc.current(0), 0x0100);
        // At elapsed=1: animated = 0xFF00 + 256 = 0x0000.
        assert_eq!(fc.current(1), 0x0000);
        // At elapsed=2: complete.
        assert_eq!(fc.current(2), 0xFF00);
    }

    #[test]
    fn current_uses_signed_wrapping_frame_elapsed() {
        let fc = mid_rotation(0, 12_800, u32::MAX - 1, 10, 5);

        assert_eq!(fc.current(1), 3_840);
        assert!(fc.is_rotating(1));
        assert_eq!(fc.current(8), 12_800);
        assert!(!fc.is_rotating(8));
    }

    #[test]
    fn non_multiple_arc_preserves_start_remainder() {
        let mut fc = FacingClass::new(0, 5);
        fc.set(0x4000, 100);

        assert_eq!(fc.duration_frames, 12);
        assert_eq!(fc.current(100), 4);
        assert_eq!(fc.current(101), 1_369);
        assert_eq!(fc.current(111), 15_019);
        assert_eq!(fc.current(112), 0x4000);
    }

    #[test]
    fn set_no_op_when_target_equals_current() {
        let mut fc = FacingClass::new(1000, 5);
        let changed = fc.set(1000, 0);
        assert!(!changed);
        assert_eq!(fc.destination(), 1000);
        assert!(fc.start_frame.is_none());
    }

    // --- Native frame / tick contract acceptance tests ---

    #[test]
    fn set_and_check_same_frame_yields_zero_elapsed() {
        // Acceptance: a timer started and checked in the SAME update sees
        // elapsed 0 (binary_frame is constant within a tick). Retarget at
        // frame 100, then query at the same frame 100 — animated sits at the
        // start orientation and rotation is in progress.
        let mut fc = FacingClass::new(0, 5); // rot_per_frame = 1280
        let changed = fc.set(12800, 100); // 12800 / 1280 = exactly 10 frames
        assert!(changed);
        assert_eq!(fc.start_frame, Some(100));
        assert_eq!(fc.duration_frames, 10);
        // elapsed = 100 - 100 = 0 → animated == prev (start), still rotating.
        assert_eq!(fc.current(100), 0);
        assert!(fc.is_rotating(100));
    }

    #[test]
    fn retarget_captures_supplied_frame_and_progresses_relative_to_it() {
        // Acceptance: a facing retarget captures the frame it is given (the
        // pre-increment frame visible during the tick) and animates relative
        // to it. Invariant to a uniform frame offset — only relative elapsed
        // matters.
        let mut fc = FacingClass::new(0, 5); // rot_per_frame = 1280
        fc.set(12800, 100); // duration 10, start_frame 100
        assert_eq!(fc.start_frame, Some(100));
        // 5 frames later: animated = 0 + 5 * 1280 = 6400, still rotating.
        assert_eq!(fc.current(105), 6400);
        assert!(fc.is_rotating(105));
        // Retarget at frame 105: snapshots visible 6400 into prev, captures
        // start_frame = 105.
        fc.set(0, 105);
        assert_eq!(fc.start_frame, Some(105));
        assert_eq!(fc.prev, 6400);
        // Same-frame after retarget: elapsed 0 → still at 6400.
        assert_eq!(fc.current(105), 6400);
    }

    #[test]
    fn set_initiates_rotation_when_target_differs() {
        let mut fc = FacingClass::new(0, 5);
        let changed = fc.set(12800, 0);
        assert!(changed);
        assert_eq!(fc.destination(), 12800);
        assert_eq!(fc.start_frame, Some(0));
        // duration = abs(12800 - 0) / 1280 = 10.
        assert_eq!(fc.duration_frames, 10);
    }

    #[test]
    fn set_snapshots_animated_into_prev_mid_rotation() {
        let mut fc = FacingClass::new(0, 5);
        fc.set(12800, 0); // start rotation: 0 → 12800 over 10 frames.

        // At frame 5, animated = 6400. Now retarget.
        assert_eq!(fc.current(5), 6400);
        fc.set(25600, 5);

        // After re-set, prev should be 6400 (the animated position at the
        // moment of the new request), NOT 0 (the old prev).
        assert_eq!(fc.prev, 6400);
        assert_eq!(fc.destination(), 25600);
        assert_eq!(fc.start_frame, Some(5));
        // New duration = abs(25600 - 6400) / 1280 = 19200 / 1280 = 15.
        assert_eq!(fc.duration_frames, 15);
    }

    #[test]
    fn set_with_zero_rot_writes_destination_without_timer() {
        let mut fc = FacingClass::new(0, 0);
        let changed = fc.set(1000, 5);
        assert!(changed);
        assert_eq!(fc.destination(), 1000);
        // No timer state set when rot is 0.
        assert!(fc.start_frame.is_none());
    }

    #[test]
    fn set_handles_wrap_around_shortest_path() {
        // From 0x0100 to 0xFF00, shortest path is COUNTER-clockwise (-512 units).
        let mut fc = FacingClass::new(0x0100, 1);
        fc.set(0xFF00, 0);
        // duration = abs((-512 as i16).unsigned_abs()) / 256 = 512/256 = 2.
        assert_eq!(fc.duration_frames, 2);
    }

    #[test]
    fn snap_writes_both_current_and_prev() {
        let mut fc = FacingClass::new(0, 5);
        fc.snap(12800, 10);
        assert_eq!(fc.current, 12800);
        assert_eq!(fc.prev, 12800);
        assert_eq!(fc.duration_frames, 0);
        assert_eq!(fc.current(10), 12800); // animated = destination immediately
    }

    #[test]
    fn snap_no_op_when_already_at_target() {
        let mut fc = FacingClass::new(1000, 5);
        let changed = fc.snap(1000, 0);
        assert!(!changed);
    }

    #[test]
    fn is_rotating_false_when_rot_zero() {
        let fc = FacingClass::new(0, 0);
        assert!(!fc.is_rotating(0));
        assert!(!fc.is_rotating(100));
    }

    #[test]
    fn is_rotating_false_when_no_start_frame() {
        let fc = FacingClass::new(0, 5);
        assert!(!fc.is_rotating(0));
    }

    #[test]
    fn is_rotating_true_during_rotation_false_after() {
        let mut fc = FacingClass::new(0, 5);
        fc.set(12800, 0); // duration = 10
        assert!(fc.is_rotating(0));
        assert!(fc.is_rotating(5));
        assert!(fc.is_rotating(9));
        assert!(!fc.is_rotating(10)); // exactly at end
        assert!(!fc.is_rotating(100));
    }

    #[test]
    fn turret_spins_formula_smoke_test() {
        // Forward-test the deferred Floating Disk permaspin formula:
        // each frame, set target = ((current(f) >> 7 + 1) >> 1 + 8) << 8.
        // ROT byte = 100 (Disk's ROT), per-frame step = 25600 in 16-bit units.
        // Per-frame target advance = 8 << 8 = 2048 < 25600, so step_size = 0
        // → snaps every frame to the new target. After 32 frames, full revolution.
        let mut fc = FacingClass::new(0, 100);
        for f in 0..32u32 {
            let animated = fc.current(f);
            let rounded_8bit: u16 = (((animated >> 7) + 1) >> 1) & 0xFF;
            let next_target: u16 = ((rounded_8bit + 8) & 0xFF) << 8;
            fc.set(next_target, f);
        }
        // After 32 frames of advancing by 8 8-bit units, we should be back at
        // (8 * 32) % 256 = 256 % 256 = 0 (in 8-bit), so 0 << 8 = 0 in 16-bit.
        assert_eq!(fc.current(32), 0);
    }
}
