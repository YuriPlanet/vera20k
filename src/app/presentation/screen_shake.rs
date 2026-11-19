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

    /// Whether the next Input event at `binary_frame` moves the offset: an
    /// even frame decays any offset left, an odd one holds it. Every service
    /// pass sends one, so a paused odd frame keeps a shake standing.
    pub(crate) fn decays_at(&self, binary_frame: u32) -> bool {
        binary_frame & 1 == 0 && self.offset_px != [0, 0]
    }

    /// Derived presentation cache; updated only after the corresponding GPU
    /// frame is presented, never from a later Logic write.
    pub(crate) fn record_presented(&mut self, offset_px: [i32; 2]) {
        self.last_presented_offset_px = offset_px;
    }

    /// Apply Logic's native-ordered detonation writes after the render's
    /// pre-Logic offset snapshot has been taken.
    ///
    /// `admit` is the client-side `ShakeIsLocal` gate
    /// (`TacticalClass::CoordsToClient`, `Ext/Bullet/Hooks.DetonateLogics.cpp:36-39`):
    /// a local-only update the client cannot see is dropped here, after the
    /// simulation has already committed the (deterministic) draw.
    pub(crate) fn apply_updates_with_hide(
        &mut self,
        updates: &[crate::sim::combat::ScreenShakeUpdate],
        hide: bool,
        admit: impl Fn(&crate::sim::combat::ScreenShakeUpdate) -> bool,
    ) {
        if hide {
            return;
        }
        for update in updates {
            if update.local_only && !admit(update) {
                continue;
            }
            if let Some(x) = update.x {
                self.offset_px[0] = x;
            }
            if let Some(y) = update.y {
                self.offset_px[1] = y;
            }
        }
    }
}

/// The client's `CoordsToClient` admission for a local-only shake write: the
/// detonation's projected point inside the tactical rectangle, with the native
/// padding [`super::instances::projection_admitted`] documents.
pub(crate) fn update_is_on_screen(
    update: &crate::sim::combat::ScreenShakeUpdate,
    camera: [f32; 2],
    viewport: [f32; 2],
) -> bool {
    let (x, y) = crate::util::lepton::absolute_leptons_to_screen(
        update.coord.x,
        update.coord.y,
        update.coord.z,
    );
    super::instances::projection_admitted([x, y], camera, viewport)
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

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::ScreenShakeState;
    use crate::sim::combat::ScreenShakeUpdate;

    #[test]
    fn screen_shake_input_snapshot_matches_native_prelogic_phase() {
        let mut shake = ScreenShakeState::default();

        // The current Input(N) event is the render snapshot; Logic's later
        // detonation writes belong to the next Input/render service.
        assert_eq!(shake.service_offset_snapshot(Some(11), false), [0, 0]);
        shake.apply_updates_with_hide(
            &[
                ScreenShakeUpdate {
                    x: Some(10),
                    y: Some(-4),
                    local_only: false,
                    coord: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
                },
                // A later same-Logic Y-only detonation wins that enabled axis;
                // the retained owner before the next Input is therefore [10, 5].
                ScreenShakeUpdate {
                    x: None,
                    y: Some(5),
                    local_only: false,
                    coord: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
                },
            ],
            false,
            |_| true,
        );
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

    #[test]
    fn a_paused_odd_frame_holds_the_shake_and_an_even_one_decays_it() {
        let mut shake = ScreenShakeState::default();
        assert!(!shake.decays_at(12));
        shake.apply_updates_with_hide(
            &[ScreenShakeUpdate {
                x: Some(3),
                y: None,
                local_only: false,
                coord: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
            }],
            false,
            |_| true,
        );
        assert!(!shake.decays_at(13));
        assert_eq!(shake.service_offset_snapshot(Some(13), true), [3, 0]);
        assert_eq!(shake.service_offset_snapshot(Some(13), true), [3, 0]);
        assert!(shake.decays_at(12));
        assert_eq!(shake.service_offset_snapshot(Some(12), true), [3, 0]);
        assert_eq!(shake.service_offset_snapshot(Some(12), true), [-2, 0]);
    }

    /// Phobos `[Phobos] HideShakeEffects=` (read default false,
    /// `Phobos.INI.cpp:113`): the detonation writes land only when the switch
    /// is off, and a standing offset still decays through the stock path.
    #[test]
    fn hide_shake_effects_drops_the_detonation_writes() {
        let update = [ScreenShakeUpdate {
            x: Some(6),
            y: Some(-3),
            local_only: false,
            coord: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
        }];
        let mut shown = ScreenShakeState::default();
        shown.apply_updates_with_hide(&update, false, |_| true);
        assert_eq!(shown.service_offset_snapshot(Some(11), false), [6, -3]);

        let mut hidden = ScreenShakeState::default();
        hidden.apply_updates_with_hide(&update, true, |_| true);
        assert_eq!(hidden.service_offset_snapshot(Some(11), false), [0, 0]);
        assert_eq!(hidden.offset_px, [0, 0]);
        assert!(!hidden.decays_at(12));
    }

    /// Phobos `[Warhead] ShakeIsLocal=` (`Ext/WarheadType/Body.cpp:184`,
    /// fallback `Ext/Rules/Body.cpp:491`): the client drops a detonation its
    /// tactical view cannot see (`Ext/Bullet/Hooks.DetonateLogics.cpp:36-39`),
    /// and an ordinary (non-local-only) write is never gated.
    #[test]
    fn a_local_only_write_outside_the_view_is_dropped() {
        let update = [ScreenShakeUpdate {
            x: Some(4),
            y: None,
            local_only: true,
            coord: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
        }];
        let mut offscreen = ScreenShakeState::default();
        offscreen.apply_updates_with_hide(&update, false, |_| false);
        assert_eq!(offscreen.offset_px, [0, 0]);

        let mut onscreen = ScreenShakeState::default();
        onscreen.apply_updates_with_hide(&update, false, |_| true);
        assert_eq!(onscreen.offset_px, [4, 0]);

        let ordinary = [ScreenShakeUpdate {
            x: Some(4),
            y: None,
            local_only: false,
            coord: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
        }];
        let mut always = ScreenShakeState::default();
        always.apply_updates_with_hide(&ordinary, false, |_| false);
        assert_eq!(always.offset_px, [4, 0]);
    }

    /// `update_is_on_screen` projects the detonation point into the same
    /// world-pixel frame `projection_admitted` compares against the camera.
    #[test]
    fn on_screen_admission_uses_the_projected_point_and_the_view_padding() {
        let inside = ScreenShakeUpdate {
            x: Some(1),
            y: None,
            local_only: true,
            coord: crate::sim::projectile::ProjectileCoord::new(2688, 2688, 0),
        };
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(2688, 2688, 0);
        assert!(super::update_is_on_screen(&inside, [x, y], [800.0, 600.0]));
        // Far outside the padded rectangle.
        let outside = ScreenShakeUpdate {
            coord: crate::sim::projectile::ProjectileCoord::new(2688 + 256 * 40, 2688, 0),
            ..inside
        };
        assert!(!super::update_is_on_screen(
            &outside,
            [x, y],
            [800.0, 600.0]
        ));
    }
}
