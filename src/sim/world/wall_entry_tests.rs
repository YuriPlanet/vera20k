//! The class `Can_Enter_Cell` wall arm (Infantry `0x0051C17C..0x0051C223`,
//! Unit `0x0073F3D7..0x0073F51F`) reads the live overlay cell: the overlay
//! index `CellClass+0x44`, data `+0x11E` and wall owner `+0x50` that
//! `OverlayClass::Mark` (`0x005FC570`) and `CellClass::DestroyOverlay`
//! (`0x00480CB0`) write during play.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::overlay_grid::damage_wall_overlay_with_terrain;
use crate::sim::rng::SimRng;
use crate::sim::world::entry_test_fixture;

/// Wall overlays `O2` (stone, `DamageLevels=3`) and `O3` (wooden), the wall
/// BuildingType `WALLB`, and movers armed with a `Wall=` or a `Wood=` warhead.
const RULES: &str = "[BuildingTypes]\n1=WALLB\n[WALLB]\nWall=yes\nFoundation=1x1\nStrength=300\n\
     [InfantryTypes]\n2=WALLI\n3=WOODI\n\
     [WALLI]\nSpeedType=Foot\nStrength=100\nPrimary=WALLGUN\n\
     Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
     [WOODI]\nSpeedType=Foot\nStrength=100\nPrimary=WOODGUN\n\
     Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
     [VehicleTypes]\n0=TRUCK\n1=WALLV\n2=WOODV\n\
     [TRUCK]\nSpeedType=Wheel\nStrength=100\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [WALLV]\nSpeedType=Wheel\nStrength=100\nPrimary=WALLGUN\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [WOODV]\nSpeedType=Wheel\nStrength=100\nPrimary=WOODGUN\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [WALLGUN]\nDamage=10\nROF=20\nRange=5\nProjectile=TESTPROJ\nWarhead=WALLWH\n\
     [WOODGUN]\nDamage=10\nROF=20\nRange=5\nProjectile=TESTPROJ\nWarhead=WOODWH\n\
     [TESTPROJ]\nAG=yes\n\
     [Warheads]\n2=WALLWH\n3=WOODWH\n\
     [WALLWH]\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
     [WOODWH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
     [O2]\nWall=yes\nStrength=300\n\
     [O3]\nWall=yes\nArmor=wood\nStrength=100\n";

const ENGINEER: u64 = 90;
const TRUCK: u64 = 91;
const WALLI: u64 = 92;
const WOODI: u64 = 93;
const WALLV: u64 = 94;
const WOODV: u64 = 95;

/// The entry fixture with every mover at (15, 15), owned by `Americans`;
/// ENGINEER and TRUCK are unarmed.
fn wall_fixture() -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    let (mut sim, rules, registry) = entry_test_fixture::fixture_with_rules_and_fixed_art(
        RULES,
        &IniFile::from_str("[WALLB]\nToOverlay=O2\n[O2]\nDamageLevels=3\n"),
    );
    let owner = sim.intern("Americans");
    for (id, name, category, kind) in [
        (
            ENGINEER,
            "ENGINEER",
            EntityCategory::Infantry,
            LocomotorKind::Walk,
        ),
        (TRUCK, "TRUCK", EntityCategory::Unit, LocomotorKind::Drive),
        (
            WALLI,
            "WALLI",
            EntityCategory::Infantry,
            LocomotorKind::Walk,
        ),
        (
            WOODI,
            "WOODI",
            EntityCategory::Infantry,
            LocomotorKind::Walk,
        ),
        (WALLV, "WALLV", EntityCategory::Unit, LocomotorKind::Drive),
        (WOODV, "WOODV", EntityCategory::Unit, LocomotorKind::Drive),
    ] {
        let mut mover = GameEntity::test_default(id, name, "Americans", 15, 15);
        mover.set_owner_for_test(owner);
        mover.set_type_ref_for_test(sim.intern(name));
        mover.set_category_for_test(category);
        mover.in_playfield = true;
        mover.locomotor = Some(LocomotorState::for_test_kind(kind));
        sim.substrate.entities.insert(mover);
    }
    (sim, rules, registry)
}

/// A wall as the map loader leaves it: the overlay cell and the bridge facts.
fn map_wall(sim: &mut Simulation, overlay: u8, data: u8) {
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(16, 15, overlay, data);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(16, 15)
        .unwrap()
        .bridge_facts
        .overlay_id = Some(overlay);
}

/// The class `Can_Enter_Cell` code of mover `id` for the wall cell (16, 15).
fn entry(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    id: u64,
) -> Result<u8, String> {
    let cell = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .native_cell_identity((16, 15));
    sim.foot_can_enter(id, cell, InfantryEntryArgs::REPAIR, rules, Some(registry))
}

#[test]
fn a_wall_built_in_play_refuses_a_mover_that_cannot_shoot_it() {
    let (mut sim, rules, registry) = wall_fixture();
    for id in [ENGINEER, TRUCK] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(0));
    }
    let owner = sim.intern("Americans");
    let wall = rules.object("WALLB").unwrap();
    assert!(crate::sim::production::stamp_wall_with_autofill(
        &mut sim,
        &rules,
        &registry,
        wall,
        (16, 15),
        owner
    ));
    // Unarmed: Infantry `0x0051C1EB`, Unit `0x0073F48F`.
    for id in [ENGINEER, TRUCK] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(7), "{id}");
    }
}

#[test]
fn a_map_wall_destroyed_in_play_admits_movers_again() {
    let (mut sim, rules, registry) = wall_fixture();
    map_wall(&mut sim, 2, 0);
    for id in [ENGINEER, TRUCK] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(7), "{id}");
    }
    let destroyed = damage_wall_overlay_with_terrain(
        sim.overlay_grid.as_mut().unwrap(),
        &registry,
        sim.resolved_terrain.as_mut(),
        16,
        15,
        -1,
        &mut SimRng::new(1),
    );
    assert_eq!(destroyed.destroyed_cells, vec![(16, 15)]);
    for id in [ENGINEER, TRUCK] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(0), "{id}");
    }
}

#[test]
fn a_mover_that_can_shoot_a_wall_prices_it_by_its_owner() {
    let (mut sim, rules, registry) = wall_fixture();
    map_wall(&mut sim, 2, 0);
    // Unowned: `Is_Ally_ByIndex` (`0x004F9A10`) rejects index -1.
    for id in [WALLI, WALLV] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(5), "{id}");
    }
    let owner = sim.intern("Americans");
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .set_wall_owner(16, 15, owner);
    for id in [WALLI, WALLV] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(4), "{id}");
    }
    // A house that names the mover's House an ally: only the mover's own
    // Allies bits count.
    let other = sim.intern("Russians");
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .set_wall_owner(16, 15, other);
    sim.house_alliances
        .entry("RUSSIANS".into())
        .or_default()
        .insert("AMERICANS".into());
    for id in [WALLI, WALLV] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(5), "{id}");
    }
    sim.house_alliances
        .entry("AMERICANS".into())
        .or_default()
        .insert("RUSSIANS".into());
    for id in [WALLI, WALLV] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(4), "{id}");
    }
}

#[test]
fn only_a_unit_takes_the_wood_route_against_a_wooden_wall() {
    let (mut sim, rules, registry) = wall_fixture();
    map_wall(&mut sim, 3, 0);
    // Unit `0x0073F4B3..0x0073F4C4`: `Wood=` against `Armor=wood`.
    assert_eq!(entry(&sim, &rules, &registry, WOODV), Ok(5));
    // Infantry asks `0x00772AC0`, which tests `Wall=` alone.
    assert_eq!(entry(&sim, &rules, &registry, WOODI), Ok(7));
    map_wall(&mut sim, 2, 0);
    assert_eq!(entry(&sim, &rules, &registry, WOODV), Ok(7));
}

#[test]
fn infantry_ignores_a_wall_at_its_last_damage_stage() {
    let (mut sim, rules, registry) = wall_fixture();
    // Damage stage 3 of `DamageLevels=3` (`0x0051C1C9..0x0051C1DC`).
    map_wall(&mut sim, 2, 0x30);
    for id in [WALLI, ENGINEER] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(0), "{id}");
    }
    // The Unit arm has no such test.
    assert_eq!(entry(&sim, &rules, &registry, WALLV), Ok(5));
    map_wall(&mut sim, 2, 0x20);
    assert_eq!(entry(&sim, &rules, &registry, ENGINEER), Ok(7));
}

#[test]
fn the_land_row_refuses_after_the_wall_arm() {
    let (mut sim, rules, registry) = wall_fixture();
    map_wall(&mut sim, 2, 0);
    assert_eq!(entry(&sim, &rules, &registry, WALLI), Ok(5));
    // The arm accumulates 5, then the ground row read after the object list
    // (Infantry `0x0051C750`) refuses whatever it accumulated.
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(16, 15)
        .unwrap()
        .speed_costs
        .foot = Some(0);
    assert_eq!(entry(&sim, &rules, &registry, WALLI), Ok(7));
}
