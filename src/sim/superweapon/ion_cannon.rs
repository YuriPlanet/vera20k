//! Tiberian Sun's Ion Cannon superweapon, `Type=IonCannon`
//! (docs/ts-ion-cannon.md), a VERA feature ported from Firestorm 2.0
//! GAME.EXE. gamemd kept the blast (IonBlastClass, `sim::world::ion_blast`)
//! and dropped the superweapon, so only a rules or map INI that lists a type
//! of it in `[SuperWeaponTypes]` switches it on; retail RULESMD.INI's
//! `[IonCannonSpecial]` is listed nowhere.
//!
//! The click reaches this case through gamemd's Fire_SW and ClickFire, which
//! do what Firestorm's HouseClass 0x4BE960 and `SuperClass::Discharge @
//! 0x60B730` do around it: Discharge calls Place and then clears the charge
//! (`+0x2B`). The computer's target is `ai_fire`'s Ion Cannon arm.

#[cfg(all(test, tests_sim_superweapon))]
#[path = "ion_cannon_tests.rs"]
mod tests;

use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;

/// Firestorm 2.0 `SuperClass::Place @ 0x60BDC0` case 3 for `owner`'s Super of
/// `sw_type` at `(rx, ry)`:
/// 1. a Super that is not charged (`+0x2B`) does nothing (`0x60BE21`);
/// 2. the Map lookup of the cell (`0x50F1F0`) gives its Level (`+0x93`,
///    signed), and the blast's coordinate is the cell's centre, from the
///    signed 16-bit cell, at Level times the level height (`[0x80CC90]`, 104)
///    (`0x60BE3E..0x60BE64`);
/// 3. a coordinate equal to the static at `0x80CC68` returns
///    (`0x60BE6A..0x60BE7C`); static init `0x60B0F0` leaves it zero and X is
///    128 more than a multiple of 256, so it never does;
/// 4. the player's selection clears (`0x7494E8 = -1`, `0x60BE8D`), which the
///    client does on [`SimSoundEvent::SuperWeaponLaunched`];
/// 5. House `+0xD2` is set (`0x60BEA0`), which the player's HouseClass::AI
///    turns into a sidebar recalculation (`0x4BD128..0x4BD148`, `0x5F3C70`).
///    RESIDUAL: not ported; gamemd's ClickFire already owns the Super's own
///    cameo after a launch, so the recalculation finds nothing else to change
///    from this case;
/// 6. `new IonBlast(coord)` (`0x60BEC6`, ctor `0x4EDB90`).
///
/// The case plays no sound and no EVA line.
pub(super) fn launch(
    sim: &mut Simulation,
    owner: InternedId,
    sw_type: InternedId,
    (rx, ry): (u16, u16),
) -> bool {
    if !super::is_charged(sim, owner, sw_type) {
        return false;
    }
    let (x, y) = (i32::from(rx as i16), i32::from(ry as i16));
    let level = crate::sim::cell_rect::get_cellclass_fallback(sim.resolved_terrain.as_ref(), x, y)
        .signed_level();
    let coord = [
        x * 256 + 128,
        y * 256 + 128,
        i32::from(level) * GROUND_LEVEL_HEIGHT_LEPTONS,
    ];
    sim.add_ion_cannon_blast(coord);
    let world = crate::sim::components::DriveCoord::cell(rx, ry, 0).into();
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        world,
    });
    true
}
