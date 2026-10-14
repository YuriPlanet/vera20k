//! Building turret rotation sound: the sustained positional loop a `Turret=yes`
//! building plays while its turret is turning.
//!
//! All of the state is TechnoClass's: `+0x4A0` the rotation sample, `+0x49C`
//! the loop latch and `+0x488` the loop's `VocHandle`; the cue
//! (`TurretRotateSound=`) is the type's `VocClass` index at `+0x560`.
//!
//! Addresses in this module are for the historical reference/Steam code layout.
//! The corpus sidecar records the actual executed binary and entry points.
//!
//! Per frame, for a type whose `Turret` byte (`+0xCA1`) is set:
//! - `BuildingClass::Update @ 0x0043FB20` stores `FacingClass::Is_Rotating @
//!   0x004C9480` of the building's own turret facing (`+0x388`) into `+0x4A0`
//!   (`0x0043FCC7..0x0043FCF9`), or zero for a type without the byte. The
//!   sample is taken **before** `TemporalClass::Update` and before the
//!   `TechnoClass::AI_Update @ 0x006F9E50` call at `0x0043FE56`, so it is the
//!   state left by the previous frame's own aim.
//! - `TechnoClass::AI_Update`, `0x006F9FA9..0x006FA04F`, then:
//!   - sample nonzero and the latch set → `VocHandle::StopAndClear @
//!     0x00405D40` on `+0x488`, `VocClass::PlayAt @ 0x007509E0` of
//!     `Type+0x560` at the object's own `Location` (`+0x9C`) on that handle,
//!     then the latch is cleared;
//!   - sample zero → `SoundEvent::Release @ 0x00406060` on `+0x488`, then the
//!     latch is set;
//!   - every visit ends by re-driving the handle's coordinate (`0x006FA04F`).
//!
//! After a reached turret-control visit, the latch is the inverse of that
//! visit's sample. An armed rotating visit requests playback and clears the
//! latch even if playback is refused. Constructor, Unlimbo and Limbo arm it;
//! Load retains it without restoring a playing source. Releasing an already
//! empty handle is omitted. Current facing cannot reconstruct this control
//! history, so the bit is retained rather than recomputed.
//!
//! Instruction reading indicates that non-buildings leave `+0x4A0` zero:
//! `Turret=` vehicles use the separate `+0x3A0` barrel facing, and a whole-image
//! operand scan finds no other nonzero writer. On that basis, VERA omits their
//! empty-handle operations and runs this state machine for structures only.
//! Non-building execution has not been compared against native.
//!
//! Evidence: instruction-level reading of `gamemd.exe` (`BuildingClass::Update`,
//! `TechnoClass::AI_Update`, `FacingClass::Is_Rotating`, `TechnoClass::Limbo`'s
//! `+0x488` release, the `TurretRotateSound` reader) plus both stock values
//! through the production rules owner. Sampling, control and Limbo addresses
//! were also checked against the supported Steam executable; this is static
//! instruction evidence. The bounded Steam comparison executes Building
//! sampling, Techno control and Limbo sound cleanup with the audio device
//! disabled. Rust consumes that native
//! corpus in `turret_rotate_sound_tests`; accepted playback, audio RNG,
//! attack/decay timing and whole native Load are outside its coverage.
//! `tools/spatial_oracle/turret_rotate_sound.py` regenerates the corpus using
//! the selected supported executable, with identity and limits in its sidecar.
//!
//! The handle itself is the app/audio owner's, keyed by
//! [`turret_rotate_sound_owner`] and re-driven by `app`, exactly as the
//! Gattling report loop and `FootClass`'s `MoveSound` are. VERA publishes the
//! stop and the start on the ordered `sound_events` channel and leaves the
//! native `0x00750D40` coordinate update to the app's per-frame owner pass.
//! Each published Stop/Play/Release reaches `FrameEffects::flush_sound` before
//! the latch write and later object work. This preserves request ordering;
//! it does not establish native parity for accepted sources or their RNG draws.
//! Load reconstructs the external handle and retains the saved latch: an
//! interrupted rotation stays silent until its idle visit re-arms the latch.

use super::{FrameEffects, SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;

/// The loop-handle owner of a techno's turret rotation sound
/// (`TechnoClass+0x488`). A tag bit keeps it apart from the object's own sound
/// handle (`FootClass+0x544`, keyed by the bare object id), from
/// [`crate::sim::combat::gattling`]'s `+0x4A4` report loop and from
/// [`crate::sim::bomb::ticking_sound_owner`]'s `+0x3C` tick: the app re-drive
/// asks [`crate::sim::world::Simulation::looping_sound_owner_coord`] which
/// family an owner belongs to, and any shared bit would resolve the wrong one.
const TURRET_ROTATE_SOUND_TAG: u64 = 1 << 60;

pub(crate) const fn turret_rotate_sound_owner(techno: u64) -> u64 {
    techno | TURRET_ROTATE_SOUND_TAG
}

pub(crate) const fn turret_rotate_sound_techno(owner: u64) -> Option<u64> {
    if owner & TURRET_ROTATE_SOUND_TAG != 0 {
        Some(owner & !TURRET_ROTATE_SOUND_TAG)
    } else {
        None
    }
}

impl Simulation {
    /// `BuildingClass::Update`'s sample block (`0x0043FCC7..0x0043FCF9`):
    /// whether this building's own turret facing is rotating at this frame.
    /// Non-building callers return `false`, following the instruction-reading
    /// evidence described above; their execution is outside the native corpus.
    pub(crate) fn sample_building_turret_rotation(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.category() != EntityCategory::Structure {
            return false;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return false;
        };
        object.has_turret && entity.body_facing.is_rotating(self.session.binary_frame)
    }

    /// `TechnoClass::AI_Update`'s turret block (`0x006F9FA9..0x006FA04F`) for
    /// one structure visit, with the sample `BuildingClass::Update` stored for
    /// it. `sample` false is the ordinary idle frame.
    ///
    /// The sample arrives as an argument rather than being recomputed here
    /// because native freezes it before `TemporalClass::Update` and before this
    /// frame's own aim: by the time the block runs, `body_facing` may already
    /// hold a newer aim, and only the earlier value decides the edge.
    ///
    /// The block's second gate is the type's `Turret` byte, so a type without
    /// it never reaches the handle at all.
    pub(crate) fn visit_turret_rotate_sound(
        &mut self,
        id: u64,
        sample: bool,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some((has_turret, armed)) = self.substrate.entities.get(id).and_then(|entity| {
            let object = self.object_type(entity.type_ref(), rules)?;
            Some((object.has_turret, entity.turret_rotate_latch))
        }) else {
            return;
        };
        if !has_turret {
            return;
        }
        if !sample {
            if !armed {
                // The prior rotating visit cleared the latch, even without playback.
                // Release any source and re-arm for the next rotating visit.
                self.sound_events.push(SimSoundEvent::ObjectSoundReleased {
                    owner: turret_rotate_sound_owner(id),
                });
                effects.flush_sound(self, Some(rules));
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.turret_rotate_latch = true;
                }
            }
            return;
        }
        if !armed {
            // Still turning: do not request playback again, even if no source exists.
            return;
        }
        // An armed rotating visit requests playback. Native reaches
        // StopAndClear and PlayAt whatever the type's index is, and PlayAt
        // refuses the -1 sentinel. The latch still clears on an unresolved
        // cue: it records the rotation edge, not audio admission or playback.
        let owner = turret_rotate_sound_owner(id);
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("turret actor remains resident");
        let sound_id = self
            .object_type(entity.type_ref(), rules)
            .and_then(|object| object.turret_rotate_sound.clone());
        let world = Self::movement_sound_world(entity);
        self.sound_events.push(SimSoundEvent::AnimationStopped {
            anim_id: owner,
            stop_sound_id: None,
            world,
        });
        effects.flush_sound(self, Some(rules));
        if let Some(sound_id) = sound_id {
            self.sound_events.push(SimSoundEvent::AnimationStarted {
                anim_id: owner,
                sound_id,
                world,
            });
            effects.flush_sound(self, Some(rules));
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.turret_rotate_latch = false;
        }
    }

    /// The `+0x488` release `TechnoClass::Limbo`'s sound cleanup makes, in the
    /// same block that releases the Gattling report handle and re-arms the
    /// latch (Release call `0x006F6C50`, latch write `0x006F6C5B`). An
    /// unarmed latch requires cleanup even if no source was accepted or Load
    /// reconstructed an empty handle; an armed latch needs no release event.
    pub(crate) fn turret_rotate_sound_limbo(
        &mut self,
        id: u64,
        effects: FrameEffects<'_>,
        rules: Option<&RuleSet>,
    ) {
        if !self.turret_rotate_loop_live(id) {
            return;
        }
        self.sound_events.push(SimSoundEvent::ObjectSoundReleased {
            owner: turret_rotate_sound_owner(id),
        });
        effects.flush_sound(self, rules);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.turret_rotate_latch = true;
        }
    }

    // This queries the control latch, not whether the audio owner has a source.
    fn turret_rotate_loop_live(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|entity| !entity.turret_rotate_latch)
    }
}

#[cfg(test)]
#[path = "turret_rotate_sound_tests.rs"]
mod tests;
