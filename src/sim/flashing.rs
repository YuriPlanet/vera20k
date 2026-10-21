//! A Techno's flash countdown (`TechnoClass+0xF0`). Its native writers: the
//! elite promotion (`EliteFlashTimer`, `0x006FA0DC`); `TechnoClass::Flash`
//! (`0x006F9DFC`), which `ObjectClass::ReceiveDamage`'s heal branch also
//! reaches (`0x005F5486`, `Flash(7)`); a building's Flash (vt+0x148,
//! `0x00456E00`, from a C4 plant); team script action 50 (`0x006E9D0A`);
//! `TeamClass::Flash_Members` (`0x006EF5FB`); and
//! `TActionClass::FlashBuildingsOfType` (`0x006E45CB`). The heal flash and
//! the MegaMission event's order-target flash (`0x004C739F`) reach vt+0x148
//! through [`Simulation::flash_object`]. ToggleRepair447091 reuses the
//! Building Flash owner for a local repair start.
//! The shared late TechnoAI
//! slot steps it (`0x004CC770`, called at `0x006FAC4D`). Presentation reads
//! its phase bit; there is no separate flash clock.

/// The signed native flash dword `TechnoClass+0xF0`, owned by this module.
/// Each writer's value is kept with all its bits. `+0xF4` is a parity cache
/// for drawing, not a separate gameplay timer.
use crate::sim::house_diplomacy::HouseAlliances;
use crate::sim::intern::StringInterner;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::vision::FogState;
use crate::sim::world::ObjectSubstrate;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Flashing {
    frames: i32,
}

impl Flashing {
    /// The countdown's remaining frames.
    pub fn frames(&self) -> i32 {
        self.frames
    }

    /// Bit 1 of the countdown: the phase in which the object draws flashed.
    pub fn flashed(&self) -> bool {
        self.frames & 2 != 0
    }

    /// Seed the countdown, as each native writer stores its operand.
    pub fn start(&mut self, frames: i32) {
        self.frames = frames;
    }

    /// Countdown `0x004CC770`: DEC runs for every nonzero signed dword,
    /// negative values included; zero is idle. Executed signed controls are
    /// in tools/spatial_oracle/team_creation (elite_flash rows). Returns
    /// whether it counted.
    pub(crate) fn tick(&mut self) -> bool {
        if self.frames == 0 {
            return false;
        }
        self.frames = self.frames.wrapping_sub(1);
        true
    }
}

impl crate::sim::world::Simulation {
    /// vt+0x148 `Flash(frames)` on a represented object. Every Foot class
    /// keeps `TechnoClass::Flash @ 0x006F9DD0`; `BuildingClass::Flash @
    /// 0x00456E00` wraps the same body and relights its slot anims
    /// ([`ObjectSubstrate::flash_building`]). TerrainClass's
    /// slot (`0x005F4370`) is an empty `RET 4`, so a terrain object needs no
    /// call.
    ///
    /// The Techno body skips the write when the object is disguised to the
    /// current player with no house behind it
    /// ([`crate::sim::game_entity::GameEntity::disguise_hides_house_from`]):
    /// a Mirage worn as terrain stays dark to an enemy that cannot see
    /// through it; a Spy always flashes.
    pub(crate) fn flash_object(
        interner: &StringInterner,
        session: &ScenarioSession,
        fog: &FogState,
        house_alliances: &HouseAlliances,
        substrate: &mut ObjectSubstrate,
        id: u64,
        frames: i32,
    ) {
        use crate::map::entities::EntityCategory;
        let Some(entity) = substrate.entities.get(id) else {
            return;
        };
        match entity.category() {
            EntityCategory::Structure => {
                substrate.flash_building(id, frames, session.binary_frame());
                return;
            }
            EntityCategory::Aircraft => {}
            EntityCategory::Infantry | EntityCategory::Unit => {
                let hidden = session.current_house().is_some_and(|player| {
                    entity.disguise_hides_house_from(
                        player,
                        Some(fog),
                        Some(house_alliances.as_map()),
                        interner,
                    )
                });
                if !hidden {
                    substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .flashing
                        .start(frames);
                }
                return;
            }
        }
        substrate.entities.get_mut(id).unwrap().flash_base(frames);
    }
}

#[cfg(test)]
#[path = "flashing_tests.rs"]
mod tests;
