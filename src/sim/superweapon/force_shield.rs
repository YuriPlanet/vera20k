//! The Force Shield's launch: `SuperClass::Launch @ 0x006CC390` case 10
//! (`0x006CD072..0x006CD2EB`).
//!
//! A charged Super builds `[General] ForceShieldInvokeAnim=` over the cell,
//! arms its fade countdown (`SuperClass+0x50`, `ForceShieldDuration=` less
//! `ForceShieldPlayFadeSoundTime=`) at the cell's deck coordinate (`+0x54`),
//! plays its type's `StartSound=`, blacks out its house for
//! `ForceShieldBlackoutDuration=`, then walks `BuildingClass::Array` from its
//! end. Each building whose owner counts the launching house as an ally
//! (`HouseClass::IsAlliedWith @ 0x004F9A50`, asked of the owner,
//! `0x006CD221..0x006CD235`) and that lies [`within_radius`] gets
//! BuildingClass's IronCurtain (`0x00457C90`, which also defuses a planted
//! C4) with the duration, the house and the Force Shield byte. Neither its
//! health nor its limbo is tested. SuperClass::AI plays the
//! type's `SpecialSound=` where the countdown was armed once it runs out
//! (`super::SuperWeaponInstance::step_fade`). The local player's tail
//! (`0x006CD2B7..0x006CD2DC`) clears the selected Super and drops the queued
//! `EVA_ForceShieldReady` line; the app does both from the launch event.
//!
//! Evidence: `tools/superweapon_oracle.py` sections `force_shield_launch` and
//! `super_fade` (Unicorn on gamemd.exe), replayed in `force_shield_tests.rs`.
//!
//! RESIDUAL: `StartSound=` plays at the launch cell's centre at height 0
//! (`SuperWeaponLaunched` carries only the cell), natively at the deck
//! coordinate (`0x006CD172`), which carries the cell's ground height and,
//! over a bridge, 416 more leptons. Trigger: a Force Shield on a raised,
//! sloped or bridge cell. Effect: the cue's position on screen, so its pan.
//!
//! Ledger: no RNG draws and no detach calls. Timer writes: the house's
//! blackout (`0x0050BC90`); each shielded building's curtain
//! (`TechnoClass::IronCurtain @ 0x0070E2B0`, `+0x18C`) and the C4 timer a
//! planted building's IronCurtain resets (`+0x528`); the Super's countdown
//! (`+0x50`).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/houses, sim/superweapon/invulnerability,
//!   sim/movement/ground_pose, sim/power_system, sim/game_entity, sim/world,
//!   util/native_x87.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

#[cfg(test)]
#[path = "force_shield_tests.rs"]
mod tests;

use crate::map::entities::EntityCategory;
use crate::map::houses::is_allied_with;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::superweapon::invulnerability::{InvulnKind, apply_invulnerability};
use crate::sim::world::{SimSoundEvent, Simulation};

/// The coordinates the walk skips (`0x006CD1C8..0x006CD1FF`): `0x00B0C020`,
/// zeroed by its static initializer `0x006CADC0`, and `0x00B0C070`, which
/// `0x006CADE0` sets to cell (0, 0)'s centre at level 0.
const WALK_SENTINELS: [[i32; 3]; 2] = [[0, 0, 0], [128, 128, 0]];

/// Launch the Force Shield at (target_rx, target_ry) for `owner`'s Super of
/// type `sw_type`. ClickFire has admitted the charged Super (`+0x6F`,
/// `0x006CD072`).
pub fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    target_rx: u16,
    target_ry: u16,
    sw_type: InternedId,
) -> bool {
    let general = &rules.general;
    let frame = sim.session.binary_frame;

    // `0x006CD07D..0x006CD130`: the anim 5 leptons over the deck coordinate.
    super::spawn_cell_anim(
        sim,
        rules,
        &general.force_shield_invoke_anim,
        target_rx,
        target_ry,
    );

    // `0x006CD135..0x006CD162`: the countdown and where its sound plays.
    let deck = super::deck_coords(sim, (target_rx, target_ry));
    if let Some(instance) = sim
        .super_weapons
        .get_mut(&owner)
        .and_then(|weapons| weapons.get_mut(&sw_type))
    {
        instance.arm_fade(
            general
                .force_shield_duration
                .wrapping_sub(general.force_shield_fade_sound_time),
            deck,
        );
    }

    // `0x006CD17B..0x006CD18B` calls the shared House50BC90 replacement
    // setter, even when its duration is shorter than the previous outage.
    // The native owner House already exists; Rust's derived power entry may
    // still be lazy. The setter keeps the signed duration's bits.
    if sim.houses.contains_key(&owner) {
        sim.power_states
            .entry(owner)
            .or_default()
            .start_blackout(frame, general.force_shield_blackout_duration as u32);
    }

    // BuildingClass::Array holds the buildings in construction order.
    let centre = super::fire::cell_coords(sim, (target_rx, target_ry));
    let launcher = sim.interner.resolve(owner);
    let terrain = sim.resolved_terrain.as_ref();
    let (ids, buildings): (Vec<u64>, Vec<(bool, [i32; 3])>) = sim
        .substrate
        .entities
        .values()
        .filter(|building| building.category == EntityCategory::Structure)
        .map(|building| {
            let allied = is_allied_with(
                &sim.house_alliances,
                sim.interner.resolve(building.owner()),
                launcher,
            );
            let coords = crate::sim::movement::ground_pose::object_get_coords(building, terrain);
            (
                building.stable_id(),
                (allied, [coords.x, coords.y, coords.z]),
            )
        })
        .unzip();
    let shielded = walk(centre, general.force_shield_radius, &buildings);
    for &index in &shielded {
        if let Some(building) = sim.substrate.entities.get_mut(ids[index]) {
            apply_invulnerability(
                building,
                frame,
                general.force_shield_duration,
                InvulnKind::ForceShield,
            );
        }
    }

    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx: target_rx,
        ry: target_ry,
    });

    log::info!(
        "ForceShield launched at ({}, {}) by '{}', {} buildings shielded",
        target_rx,
        target_ry,
        sim.interner.resolve(owner),
        shielded.len()
    );

    true
}

/// The walk (`0x006CD190..0x006CD2B1`) from the cell's centre (its
/// GetCoords, vt+0x48, without the deck) over BuildingClass::Array, given
/// for each building whether its owner counts the launching house as an ally
/// and its GetCoords: the indexes it shields, from the array's end. A centre
/// among [`WALK_SENTINELS`] shields nothing.
fn walk(centre: [i32; 3], radius_cells: i32, buildings: &[(bool, [i32; 3])]) -> Vec<usize> {
    if WALK_SENTINELS.contains(&centre) {
        return Vec::new();
    }
    (0..buildings.len())
        .rev()
        .filter(|&index| {
            let (allied, coords) = buildings[index];
            allied && within_radius(centre, coords, radius_cells)
        })
        .collect()
}

/// The walk's distance test (`0x006CD1C2..0x006CD289`):
/// `CoordStruct::Distance3D @ 0x0041C380` of the centre less a building's
/// GetCoords (`0x0041C230`) below `ForceShieldRadius=` cells (`SHL 8`; a
/// signed `JGE` skips the building).
fn within_radius(centre: [i32; 3], coords: [i32; 3], radius_cells: i32) -> bool {
    crate::util::native_x87::distance_3d_leptons(centre, coords) < radius_cells.wrapping_shl(8)
}
