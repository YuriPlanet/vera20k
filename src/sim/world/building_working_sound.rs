//! A building's working sound: the type's `WorkingSound=` (`+0xE80`) or
//! `NotWorkingSound=` (`+0xE84`) on the building's `+0x6B4` handle.
//!
//! - `BuildingClass::Update @ 0x0043FB20` samples the operational state that
//!   [`Simulation::visit_building_operational`] owns (`0x0043FB29..0x0043FB5F`)
//!   and compares it with `+0x6C8`.
//! - On a change, if the type names either sound (`0x0043FB6D..0x0043FB80`),
//!   it detaches the handle (`0x00405FD0`). It then plays the new state's
//!   sound with `VocClass::PlayAt @ 0x007509E0` at `Location` (`+0x9C`) on
//!   that handle (`0x0043FB82..0x0043FBE3`), where `-1` plays nothing. The
//!   operational dispatcher (`0x004549B0`) and the `+0x6C8` store follow.
//! - On an unchanged visit with either sound, `UpdateLoopingSound @
//!   0x00750D40` re-drives the handle at `Location` (`0x0043FBF7..0x0043FC34`).
//!   That is the app's per-frame owner pass, which asks
//!   [`Simulation::looping_sound_owner_coord`].
//! - The destructor releases the handle (`0x00406060` at `0x0043BD1A`). Limbo
//!   leaves it alone, and Load starts it empty (`0x00405BE0` at `0x00453FAB`)
//!   while `+0x6C8` is restored, so a loaded loop waits for the next edge.
//!
//! Stock authors: seven defences play `PowerOn`/`PowerOff`; `CAOILD` and
//! `CASLAB` play `OilDerrickLoop` while operational and nothing otherwise.
//!
//! The handle is the app/audio owner's, keyed by [`working_sound_owner`]. The
//! detach and the PlayAt go out on the ordered sound channel and are flushed
//! before the dispatcher runs.
//!
//! Evidence: instruction-level reading of the addresses above, and the stock
//! authors through the production rules owner. The sim side draws no RNG and
//! writes no timer.

use super::{FrameEffects, SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::intern::StringInterner;
use crate::sim::world::FrameChannel;
use crate::sim::world::ObjectSubstrate;

/// The tag keeps this handle apart from the object's own sound handle (bare
/// id) and from the other owner families (`1 << 58` to `1 << 62`).
const WORKING_SOUND_TAG: u64 = 1 << 57;

/// The owner of a building's `+0x6B4` handle.
pub(crate) const fn working_sound_owner(building: u64) -> u64 {
    building | WORKING_SOUND_TAG
}

/// The building whose `+0x6B4` handle `owner` names.
pub(crate) const fn working_sound_building(owner: u64) -> Option<u64> {
    if owner & WORKING_SOUND_TAG != 0 {
        Some(owner & !WORKING_SOUND_TAG)
    } else {
        None
    }
}

impl Simulation {
    /// `0x0043FB6D..0x0043FBE3`: an operational edge's detach and PlayAt,
    /// before the dispatcher.
    pub(super) fn building_working_sound_edge(
        &mut self,
        id: u64,
        operational: bool,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        if object.working_sound.is_none() && object.not_working_sound.is_none() {
            return;
        }
        let sound = if operational {
            &object.working_sound
        } else {
            &object.not_working_sound
        };
        let world = Self::movement_sound_world(entity);
        let owner = working_sound_owner(id);
        self.sound_events
            .emit(SimSoundEvent::ObjectSoundDetached { owner });
        if let Some(sound_id) = sound.clone() {
            self.sound_events.emit(SimSoundEvent::AnimationStarted {
                anim_id: owner,
                sound_id,
                world,
            });
        }
        effects.flush_sound(self, Some(rules));
    }

    /// The `0x00750D40` re-drive coordinate of a `+0x6B4` owner: the
    /// building's `Location` for as long as it exists.
    pub(super) fn working_sound_owner_coord(&self, owner: u64) -> Option<AnimWorldCoord> {
        self.substrate
            .entities
            .get(working_sound_building(owner)?)
            .map(Self::movement_sound_world)
    }

    /// The destructor's Release (`0x0043BD1A`), for a building whose type
    /// can have given the handle a sound.
    pub(super) fn release_building_working_sound(
        interner: &StringInterner,
        substrate: &ObjectSubstrate,
        sound_events: &mut FrameChannel<SimSoundEvent>,
        id: u64,
        rules: Option<&RuleSet>,
    ) {
        let Some(entity) = substrate.entities.get(id) else {
            return;
        };
        if entity.category() != EntityCategory::Structure {
            return;
        }
        let authored = rules
            .and_then(|rules| interner.object_type(entity.type_ref(), rules))
            .is_some_and(|object| {
                object.working_sound.is_some() || object.not_working_sound.is_some()
            });
        if authored {
            sound_events.emit(SimSoundEvent::ObjectSoundReleased {
                owner: working_sound_owner(id),
            });
        }
    }
}

#[cfg(test)]
#[path = "building_working_sound_tests.rs"]
mod tests;
