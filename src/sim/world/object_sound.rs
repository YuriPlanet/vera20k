//! `ObjectClass`'s two positional sound handles: `+0x3C` plays the type's
//! `AmbientSound=` (`ObjectTypeClass+0x1F4`) and `+0x50` the custom sound
//! (`+0x64`) that trigger action 99 assigns.
//!
//! - `ObjectClass::AI @ 0x005F3E70` (`0x005F3E78..0x005F3F0C`): unless the
//!   object is in Limbo (`+0x81`), it plays the type's AmbientSound and then
//!   the custom sound through `VocClass::PlayAt @ 0x007509E0` at its
//!   `Location` (`+0x9C`), on `+0x3C` and `+0x50`. It does this on every
//!   visit. PlayAt keeps a live event of the same sound and starts a new one
//!   once the last has ended, so the entry's AMBIENT predelay spaces the
//!   repeats. It stops the event while the listener is out of range.
//! - Callers: `TechnoClass::AI_Update` reaches it through `MissionClass::AI @
//!   0x005B3060`, whose first call (`0x005B3067`) it is, right after the
//!   `+0xC4` count (`0x006FA646..0x006FA655`); `TerrainClass::AI @ 0x0071C730`
//!   calls it first (`0x0071C738`). Anims (`0x00423B1E`), bullets and waves
//!   reach it too, but no retail type of theirs sets AmbientSound and action
//!   99 only assigns Buildings and Terrain, so VERA does not visit them here.
//! - `ObjectClass::Set_Custom_Sound @ 0x005F6CB0` writes `+0x64`; `-1` also
//!   detaches `+0x50` (`0x00405FD0`).
//! - `ObjectClass::Limbo @ 0x005F4D30` hard-stops `+0x3C` (`0x00405D40` at
//!   `0x005F4D81`) and detaches `+0x50` (`0x005F4D89`). `+0x64` survives, so
//!   an object returning from Limbo resumes its custom sound.
//!   `ObjectClass::UnInit @ 0x005F65F0` reaches the same Limbo (`vt+0xD4`),
//!   which is how a destroyed tree detaches its custom sound.
//! - TAction 99 (`TActionClass::Execute @ 0x006DD8B0`, case at `0x006DE845`):
//!   the waypoint's `ScenarioClass::GetWaypointLocation @ 0x0068BD00`, then
//!   `FUN_006E1830`. That takes the coordinate's cell (`/256`, truncating) and
//!   rejects the zero cell (`0x00B0E680`). Otherwise it returns the cell's
//!   first Building (`CellClass::Look_up_building_in_cell @ 0x0047C520`), else
//!   its first Terrain on the ground list (`0x0047C550`). An object takes the
//!   action's sound as its custom sound. With none, `VocClass::PlayAtCoord @
//!   0x00750E20` plays it at the coordinate with flags 1.
//! - TAction 116 (`0x006E1980`) resolves the same waypoint and ground-list
//!   object, then calls `Set_Custom_Sound(-1)`. With no object (including
//!   the zero cell), `VocClass::StopAtCoord @ 0x00750F40` detaches every
//!   retained request whose full XYZ equals the waypoint and whose flags
//!   intersect 1. The audio request owner retains those coordinates/flags;
//!   request retirement remains in its existing `0x00750EC0` service.
//!
//! The handles belong to the app/audio owner, keyed by
//! [`ambient_sound_owner`] and [`custom_sound_owner`]. Each PlayAt is
//! published as `AnimationStarted` on the ordered sound channel and flushed
//! before later object work. No `UpdateLoopingSound @ 0x00750D40` runs on
//! these handles, so the app's owner re-drive passes over them
//! ([`Simulation::object_sound_owner_alive`]): its reallocation would skip a
//! restarted loop's attack (`0x00750DB3`), and PlayAt plays it.
//!
//! Evidence: instruction-level reading of `gamemd.exe` at the addresses above
//! and the retail data: the AmbientSound authors are all TechnoTypes, and the
//! 161 retail skirmish maps author action 99 in 151 maps.
//!
//! RESIDUALS:
//! - A waypoint index the action reader rejects (native `-1`) reads the slot
//!   before the waypoint array; VERA plays nothing.
//! - Anim, Bullet, Wave and Terrain types do not bind AmbientSound
//!   (dormant: no retail one sets it).

use super::{FrameEffects, SimSoundEvent, Simulation};
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::DriveCoord;
use crate::sim::movement::locomotor::MovementLayer;

/// The handle tags. They keep these owners apart from each other, from the
/// object's own sound handle (bare id) and from the bomb, Gattling and turret
/// families (`1 << 62`, `1 << 61`, `1 << 60`):
/// [`Simulation::object_sound_owner_alive`] decodes an owner by its tag.
const AMBIENT_SOUND_TAG: u64 = 1 << 59;
const CUSTOM_SOUND_TAG: u64 = 1 << 58;

/// The owner of an object's `+0x3C` AmbientSound handle.
pub(crate) const fn ambient_sound_owner(object: u64) -> u64 {
    object | AMBIENT_SOUND_TAG
}

/// The owner of an object's `+0x50` custom-sound handle.
pub(crate) const fn custom_sound_owner(object: u64) -> u64 {
    object | CUSTOM_SOUND_TAG
}

/// The object whose `+0x3C` or `+0x50` handle `owner` names.
pub(crate) const fn object_sound_object(owner: u64) -> Option<u64> {
    if owner & (AMBIENT_SOUND_TAG | CUSTOM_SOUND_TAG) != 0 {
        Some(owner & !(AMBIENT_SOUND_TAG | CUSTOM_SOUND_TAG))
    } else {
        None
    }
}

impl Simulation {
    /// `ObjectClass::AI`'s sound block for one visit of a Techno or Terrain.
    pub(crate) fn object_ai_sounds(&mut self, id: u64, rules: &RuleSet, effects: FrameEffects<'_>) {
        // `+0x1F4` and `+0x64` are read before `Location`, so an object
        // with neither costs no coordinate.
        let ambient = self.object_ambient_sound(id, rules);
        if ambient.is_none() && !self.object_custom_sounds.contains_key(&id) {
            return;
        }
        let Some(world) = self.object_sound_location(id) else {
            return;
        };
        if let Some(sound_id) = ambient {
            self.sound_events.emit(SimSoundEvent::AnimationStarted {
                anim_id: ambient_sound_owner(id),
                sound_id: sound_id.to_owned(),
                world,
            });
            effects.flush_sound(self, rules);
        }
        if let Some(sound_id) = self.object_custom_sounds.get(&id).cloned() {
            self.sound_events.emit(SimSoundEvent::AnimationStarted {
                anim_id: custom_sound_owner(id),
                sound_id,
                world,
            });
            effects.flush_sound(self, rules);
        }
    }

    /// Execute98 at6DE7A5..6DE842: scan702 slots in order, retaining at most
    ///400 nonempty pairs. The signed Scenario range request runs even for no
    ///candidates; a singleton consumes no draw. Cell642740 uses Z=0, without
    ///Map/ground/bridge or custom-object lookup. Voc750E20 receives flags0.
    pub(crate) fn trigger_play_sound_at_random_waypoint(
        &mut self,
        sound: Option<&str>,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let mut candidates = Vec::new();
        for index in 0..crate::map::waypoints::WAYPOINT_SLOT_COUNT as i32 {
            if self.session.waypoint_is_valid(index) {
                candidates.push(self.session.waypoint_cell(index).expect("valid slot"));
                if candidates.len() == 400 {
                    break;
                }
            }
        }
        let selected = self
            .scenario_rng
            .next_range_i32_inclusive(0, candidates.len() as i32 - 1);
        let cell = if candidates.is_empty() && selected == 0 {
            //4068F0 initializes the original100 CellStruct scratch entries.
            (0, 0)
        } else if let Some(cell) = usize::try_from(selected)
            .ok()
            .and_then(|index| candidates.get(index))
        {
            *cell
        } else {
            //RESIDUAL: no-waypoint result-1 reads before native's scratch
            //array. Keep the real RNG request and Execute's unconditional
            //AL1 (6DE836), while skipping audio with an undefined coordinate.
            //A Vec also avoids native stack corruption when its
            //400-entry admission exceeds that100-entry scratch allocation.
            return true;
        };
        let coordinate = crate::sim::cell_kernel::cell_center(
            crate::sim::cell_kernel::CellCoordinate {
                x: i32::from(cell.0),
                y: i32::from(cell.1),
            },
            0,
        );
        if let Some(sound_id) = sound {
            self.sound_events.emit(SimSoundEvent::VocAtCoord {
                sound_id: sound_id.to_owned(),
                coordinate: DriveCoord {
                    x: coordinate.x,
                    y: coordinate.y,
                    z: 0,
                },
                flags: 0,
            });
            effects.flush_sound(self, rules);
        }
        true
    }

    /// TAction 99's body: see the module doc. `sound` is the action's
    /// resolved `+0x90` (`None` for -1).
    pub(crate) fn trigger_play_sound_at_waypoint(
        &mut self,
        sound: Option<&str>,
        waypoint: Option<u32>,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some([x, y, z]) =
            waypoint.and_then(|index| self.trigger_waypoint_location(index as i32))
        else {
            return false;
        };
        let cell = ((x / 256) as i16, (y / 256) as i16);
        if cell != (0, 0)
            && let Some(object) = self.waypoint_sound_object((cell.0 as u16, cell.1 as u16))
        {
            self.set_custom_sound(object, sound);
            effects.flush_sound(self, rules);
            return true;
        }
        if let Some(sound_id) = sound {
            self.sound_events.emit(SimSoundEvent::VocAtCoord {
                sound_id: sound_id.to_owned(),
                coordinate: DriveCoord { x, y, z },
                flags: 1,
            });
            effects.flush_sound(self, rules);
        }
        true
    }

    /// TAction 116 (`0x006E1980`): clear the first ground Building's custom
    /// sound, else the first Terrain's; with neither, stop coordinate requests
    /// through `0x00750F40` using flags 1. `+0x64` remains the object's sole
    /// custom-sound authority, and the app/audio owner holds request handles.
    pub(crate) fn trigger_stop_sound_at_waypoint(
        &mut self,
        waypoint: Option<u32>,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some([x, y, z]) =
            waypoint.and_then(|index| self.trigger_waypoint_location(index as i32))
        else {
            return false;
        };
        let cell = ((x / 256) as i16, (y / 256) as i16);
        if cell != (0, 0)
            && let Some(object) = self.waypoint_sound_object((cell.0 as u16, cell.1 as u16))
        {
            self.set_custom_sound(object, None);
        } else {
            self.sound_events.emit(SimSoundEvent::VocStopAtCoord {
                coordinate: DriveCoord { x, y, z },
                flags: 1,
            });
        }
        // Original detach completes before the next action or Object AI can
        // submit a new sound on the same handle/coordinate in this frame.
        effects.flush_sound(self, rules);
        true
    }

    /// `ObjectClass::Limbo`'s handle teardown for a Techno
    /// (`0x005F4D81`, `0x005F4D89`).
    pub(crate) fn object_sound_conceal(&mut self, id: u64, rules: &RuleSet) {
        if self.object_ambient_sound(id, rules).is_some()
            && let Some(world) = self.object_sound_location(id)
        {
            self.sound_events.emit(SimSoundEvent::AnimationStopped {
                anim_id: ambient_sound_owner(id),
                stop_sound_id: None,
                world,
            });
        }
        self.detach_custom_sound(id);
    }

    /// `+0x50`'s detach (`0x00405FD0`) as Limbo or `Set_Custom_Sound(-1)`
    /// makes it. Only an assigned sound can have given the handle an event.
    pub(crate) fn detach_custom_sound(&mut self, id: u64) {
        if self.object_custom_sounds.contains_key(&id) {
            self.sound_events.emit(SimSoundEvent::ObjectSoundDetached {
                owner: custom_sound_owner(id),
            });
        }
    }

    /// For an owner naming an object's `+0x3C` or `+0x50` handle, whether
    /// the object still stands out of Limbo; `None` for any other owner.
    pub fn object_sound_owner_alive(&self, owner: u64) -> Option<bool> {
        Some(
            self.object_sound_location(object_sound_object(owner)?)
                .is_some(),
        )
    }

    /// The object's destruction drops its `+0x64` with it.
    pub(crate) fn drop_object_custom_sound(&mut self, id: u64) {
        self.object_custom_sounds.remove(&id);
    }

    /// `ObjectClass::Set_Custom_Sound @ 0x005F6CB0`.
    fn set_custom_sound(&mut self, object: u64, sound: Option<&str>) {
        match sound {
            Some(sound_id) => {
                self.object_custom_sounds
                    .insert(object, sound_id.to_owned());
            }
            None => {
                self.detach_custom_sound(object);
                self.object_custom_sounds.remove(&object);
            }
        }
    }

    /// `FUN_006E1830`'s object: the cell's first Building on the ground list,
    /// else its Terrain.
    fn waypoint_sound_object(&self, cell: (u16, u16)) -> Option<u64> {
        self.substrate
            .occupancy
            .first_building_on_layer(cell.0, cell.1, MovementLayer::Ground)
            .or_else(|| self.terrain_objects.cell_index().get(&cell).copied())
    }

    /// `Location` (`+0x9C`) of a Techno, or of a Terrain object while it is
    /// not in Limbo. `None` also stands for a Techno in Limbo.
    /// `ObjectClass::AI @ 0x005F3E70`'s InLimbo (`+0x81`) gate and PlayAt coordinate
    /// (`0x005F3E78..0x005F3F0C`).
    fn object_sound_location(&self, id: u64) -> Option<AnimWorldCoord> {
        if let Some(entity) = self.substrate.entities.get(id) {
            return (!entity.lifecycle.in_limbo()).then(|| Self::movement_sound_world(entity));
        }
        let terrain = self
            .terrain_objects
            .objects()
            .get(&id)
            .filter(|terrain| terrain.is_live())?;
        let coord = terrain.world_coord();
        Some(AnimWorldCoord {
            x: coord.x,
            y: coord.y,
            z: coord.z,
        })
    }

    fn object_ambient_sound<'r>(&self, id: u64, rules: &'r RuleSet) -> Option<&'r str> {
        let entity = self.substrate.entities.get(id)?;
        self.object_type(entity.type_ref(), rules)?
            .ambient_sound
            .as_deref()
    }
}

#[cfg(test)]
#[path = "object_sound_tests.rs"]
mod tests;
