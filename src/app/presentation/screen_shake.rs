//! The client-owned screen offset retained by native GScreenClass.

/// MatchPresentationState owns this one offset across scenario transitions.
#[derive(Debug, Default)]
pub(crate) struct ScreenShakeState {
    offset_px: [i32; 2],
    /// Cache of the offset used by the last successful present. Focus-frozen
    /// redraws reuse it because native sleeps without presenting a new frame.
    last_presented_offset_px: [i32; 2],
}

impl ScreenShakeState {
    /// `GScreenClass::vt_entry_28 @ 0x004F4BB0` reads, but does not advance,
    /// `g_CurrentFrameCounter`; on even Input events it updates X then Y.
    fn input_event(&mut self, binary_frame: u32) -> [i32; 2] {
        if binary_frame & 1 == 0 {
            self.offset_px[0] = decay_axis(self.offset_px[0]);
            self.offset_px[1] = decay_axis(self.offset_px[1]);
        }
        self.offset_px
    }

    /// Choose the one render snapshot for this in-game service. `None` means
    /// focus freeze withheld native Input, so redraw the last presented value.
    /// Paused Main_Tick renders first at 0x0055D84F and reaches Throttle/Input
    /// at 0x0055E253; snapshot before consuming that event, independently of
    /// whether the host later acquires or presents a GPU frame.
    pub(crate) fn service_offset_snapshot(
        &mut self,
        input_frame: Option<u32>,
        render_before_input: bool,
    ) -> [i32; 2] {
        match input_frame {
            Some(frame) if render_before_input => {
                let snapshot = self.offset_px;
                self.input_event(frame);
                snapshot
            }
            Some(frame) => self.input_event(frame),
            None => self.last_presented_offset_px,
        }
    }

    /// Derived presentation cache; updated only after the corresponding GPU
    /// frame is presented, never from a later Logic write.
    pub(crate) fn record_presented(&mut self, offset_px: [i32; 2]) {
        self.last_presented_offset_px = offset_px;
    }

    /// Apply Logic's native-ordered detonation writes after the render's
    /// pre-Logic offset snapshot has been taken.
    pub(crate) fn apply_updates(&mut self, updates: &[crate::sim::combat::ScreenShakeUpdate]) {
        for update in updates {
            if let Some(x) = update.x {
                self.offset_px[0] = x;
            }
            if let Some(y) = update.y {
                self.offset_px[1] = y;
            }
        }
    }
}

/// The native branches preserve sign alternation while reducing magnitude:
/// positive `v` becomes `1-v`; negative `v` becomes `-1-v`.
fn decay_axis(value: i32) -> i32 {
    if value > 0 {
        1 - value
    } else if value < 0 {
        -1 - value
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::ScreenShakeState;
    use crate::sim::combat::ScreenShakeUpdate;

    #[test]
    fn screen_shake_input_snapshot_matches_native_prelogic_phase() {
        let mut shake = ScreenShakeState::default();

        // The current Input(N) event is the render snapshot; Logic's later
        // detonation writes belong to the next Input/render service.
        assert_eq!(shake.service_offset_snapshot(Some(11), false), [0, 0]);
        shake.apply_updates(&[
            ScreenShakeUpdate {
                x: Some(10),
                y: Some(-4),
            },
            // A later same-Logic Y-only detonation wins that enabled axis;
            // the retained owner before the next Input is therefore [10, 5].
            ScreenShakeUpdate {
                x: None,
                y: Some(5),
            },
        ]);
        // The paused native path renders the current owner first, then the
        // same-frame Input event decays it. A later Y-only write won above.
        let first_paused_snapshot = shake.service_offset_snapshot(Some(12), true);
        assert_eq!(first_paused_snapshot, [10, 5]);
        assert_eq!(shake.offset_px, [-9, -4]);
        shake.record_presented(first_paused_snapshot);

        // Wait/Throttle services can issue another Input at the same native
        // frame; parity is not a draw counter, but each even Input still decays.
        let repeated_paused_snapshot = shake.service_offset_snapshot(Some(12), true);
        assert_eq!(repeated_paused_snapshot, [-9, -4]);
        assert_eq!(shake.offset_px, [8, 3]);
        shake.record_presented(repeated_paused_snapshot);

        // Focus freeze does not consume Input; redraws reuse the image's last
        // presented offset rather than a newer unpresented simulation value.
        assert_eq!(shake.service_offset_snapshot(None, false), [-9, -4]);
        assert_eq!(shake.offset_px, [8, 3]);

        // The first ordinary service after unpausing resumes pre-render Input.
        assert_eq!(shake.service_offset_snapshot(Some(12), false), [-7, -2]);
    }
}
