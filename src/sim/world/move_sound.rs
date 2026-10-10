//! FootClass MoveSound latch, countdown and post-Process selection.
//! Native FootClass::AI 0x4DA80C / 0x4DAA01..0x4DAB3C; executable controls:
//! tools/spatial_oracle/fv_cell_attack/foot_move_sound.json.

use super::{SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;

/// Foot+0x53C and signed dword+0x540. The sound may have finished (or playback
/// may have been rejected) while the latch remains set. Device state must
/// never rewrite this state or decide whether simulation spends a Main draw.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct MoveSoundState {
    active: bool,
    countdown: i32,
}

impl MoveSoundState {
    pub(crate) fn is_active(self) -> bool {
        self.active
    }

    pub(crate) fn countdown(self) -> i32 {
        self.countdown
    }

    #[cfg(test)]
    pub(crate) fn from_raw_for_test(active: bool, countdown: i32) -> Self {
        Self { active, countdown }
    }
}

impl Simulation {
    /// FootClass::Load 0x4DB60D..0x4DB624 reconstructs the handle and clears both fields
    /// of logical sound state, regardless of sinking/crash state. The next
    /// ordinary paid Foot visit may start a new sound and spends its own draw.
    pub(crate) fn restore_move_sound_state_after_load(&mut self, id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(id)
            && entity.category != EntityCategory::Structure
        {
            entity.move_sound = MoveSoundState::default();
        }
    }

    /// The before value is the same Foot+538 captured before Process, not
    /// position, facing or a locomotor cursor. The short-circuit changed arm
    /// does not ask IsMovingNow again. Stock idle SQD changes every four
    /// frames, refreshing countdown 3 before lapse, and plays one one-shot.
    ///
    /// RESIDUAL: 0x4DAA4A..0x4DAA62 suppresses a qualifying sound for a nonnull
    /// DirectRocker reciprocal Techno+2A8 link when Type+692 Pushy is set.
    /// The type reader exists, but the reciprocal link mechanism is absent.
    /// Ordinary unlinked SQD is covered; linked actors can retain/start a
    /// sound and spend a Main draw wrongly.
    /// Held locomotor routes retain their existing moving-now/cadence limits.
    pub(super) fn tick_move_sound_after_process(
        &mut self,
        stable_id: u64,
        body_frame_before_process: u32,
        rules: Option<&RuleSet>,
    ) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if entity.category == EntityCategory::Structure || entity.locomotor.is_none() {
            return;
        }
        let changed_or_moving = body_frame_before_process != entity.body_frame_counter
            || crate::sim::movement::motion_query::is_moving_now(
                entity,
                rules.map(|rules| {
                    crate::sim::movement::SpeedRules::new(
                        rules,
                        &self.interner,
                        &self.type_handles,
                        &self.houses,
                    )
                }),
                self.session.binary_frame,
            );
        // Native +8D/+425. A retained parachute object alone is not this gate.
        let falling_or_crashing = entity.is_falling_down() || entity.crashing;
        let state = entity.move_sound;
        if changed_or_moving && !falling_or_crashing {
            let sounds = rules
                .and_then(|rules| self.object_type(entity.type_ref(), rules))
                .map_or(&[][..], |object| object.move_sound.as_slice());
            let mut started = false;
            if !state.active && !sounds.is_empty() {
                let world = Self::movement_sound_world(entity);
                // 0x4DAA91 hard-stops any prior sound on the shared Foot handle,
                // then 0x4DAACB draws Main once even for a singleton vector.
                self.sound_events.push(SimSoundEvent::AnimationStopped {
                    anim_id: stable_id,
                    stop_sound_id: None,
                    world,
                });
                let index = self.main_rng.next_u32() as usize % sounds.len();
                self.sound_events.push(SimSoundEvent::AnimationStarted {
                    anim_id: stable_id,
                    sound_id: sounds[index].clone(),
                    world,
                });
                // 0x4DAAE7 does not depend on PlayAt's device/admission result.
                started = true;
            }
            let entity = self.substrate.entities.get_mut(stable_id).unwrap();
            entity.move_sound.active = state.active || started;
            // 0x4DAA87 reaches this even when the configured vector is empty.
            entity.move_sound.countdown = 3;
        } else if state.active {
            let entity = self.substrate.entities.get_mut(stable_id).unwrap();
            if state.countdown == 0 || falling_or_crashing {
                // 0x4DAB2B Release (0x406060) lets a one-shot play out. It is distinct
                // from the Stop405D40 before a new selection and Limbo405FD0.
                self.sound_events
                    .push(SimSoundEvent::ObjectSoundReleased { owner: stable_id });
                entity.move_sound = MoveSoundState::default();
            } else {
                entity.move_sound.countdown = state.countdown.wrapping_sub(1);
            }
        }
    }
}
