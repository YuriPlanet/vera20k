//! Rust regression tests for `BuildingClass::AfterDestruction`'s rubble arm
//! and crate tail, driven through the production damage receiver.
//! Expectations follow the instruction reading recorded in
//! `building_after_destruction`; they are not native goldens.

use std::fmt::Write as _;

use crate::rules::building_rubble::RubbleArt;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TargetKind};
use crate::sim::house_state::HouseState;
use crate::sim::overlay_grid::{OverlayGrid, RUBBLE_OVERLAY_INDEX};
use crate::sim::world::{FrameEffects, Simulation};

const RULES: &str = "\
[InfantryTypes]
[VehicleTypes]
[AircraftTypes]
[BuildingTypes]
0=RUIN
1=PLAIN
2=SHORT
3=CASH
4=GIFT
5=BOTH
[Warheads]
0=KILLWH
[RUIN]
Strength=100
LeaveRubble=yes
[PLAIN]
Strength=100
[SHORT]
Strength=100
LeaveRubble=yes
[CASH]
Strength=100
CrateBeneath=yes
CrateBeneathIsMoney=yes
[GIFT]
Strength=100
CrateBeneath=yes
[BOTH]
Strength=100
LeaveRubble=yes
CrateBeneath=yes
CrateBeneathIsMoney=yes
[CrateRules]
CrateImg=SILVER
WoodCrateImg=WOOD
WaterCrateImg=WATER
[KILLWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

const ART: &str = "\
[RUIN]
Foundation=2x2
[PLAIN]
Foundation=2x2
[SHORT]
Foundation=2x2
[CASH]
Foundation=2x2
[GIFT]
Foundation=2x2
[BOTH]
Foundation=2x2
";

fn rules() -> RuleSet {
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str(ART),
    )
    .expect("rubble rules");
    // RUIN's image carries rubble; SHORT's is too short (no art bound).
    rules.set_building_rubble_art_for_test(
        "BOTH",
        RubbleArt {
            shp: "BOTH.SHP".into(),
            body_frame: 3,
        },
    );
    rules.set_building_rubble_art_for_test(
        "RUIN",
        RubbleArt {
            shp: "RUIN.SHP".into(),
            body_frame: 3,
        },
    );
    rules
}

/// Dense table whose index 0xEF is an `IsRubble=yes` type, as in retail,
/// with the three crate images at 1..=3.
fn registry() -> OverlayTypeRegistry {
    let mut ini = String::from("[OverlayTypes]\n");
    for id in 0..=u16::from(RUBBLE_OVERLAY_INDEX) {
        let name = match id {
            1 => "SILVER".to_owned(),
            2 => "WOOD".to_owned(),
            3 => "WATER".to_owned(),
            _ if id == u16::from(RUBBLE_OVERLAY_INDEX) => "RUBBLE_OVERLAY".to_owned(),
            _ => format!("OV{id:03}"),
        };
        writeln!(ini, "{id}={name}").unwrap();
    }
    ini.push_str("[RUBBLE_OVERLAY]\nIsRubble=yes\nDrawFlat=true\nNoUseTileLandType=true\n");
    ini.push_str("[SILVER]\nCrate=yes\n[WOOD]\nCrate=yes\n[WATER]\nCrate=yes\n");
    OverlayTypeRegistry::from_ini(&IniFile::from_str(&ini), None)
}

fn sim() -> Simulation {
    let mut sim = Simulation::with_seed(7);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    sim.session.house_order.push(owner);
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
        32,
        32,
        crate::map::resolved_terrain::test_clear_cell,
    ));
    // A playfield and map size the crate placer reads (as the crate tests'
    // fixture installs them).
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 20,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    });
    sim.playfield_size_height = Some(20);
    sim
}

fn kill(sim: &mut Simulation, rules: &RuleSet, registry: &OverlayTypeRegistry, id: u64) {
    let warhead = sim.interner.intern("KILLWH");
    let hit = EntityDamageEvent::direct_receiver(
        id,
        100_000,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(rules, Some(registry), &[hit], FrameEffects::default());
}

fn overlay(sim: &Simulation, cell: (u16, u16)) -> crate::sim::overlay_grid::OverlayCell {
    *sim.overlay_grid.as_ref().unwrap().cell(cell.0, cell.1)
}

/// Every foundation cell takes overlay 0xEF with its data byte kept; only the
/// north-west cell records the type the overlay draws read.
#[test]
fn a_destroyed_leave_rubble_building_leaves_rubble_on_its_foundation() {
    let rules = rules();
    let registry = registry();
    let mut sim = sim();
    let ruin = sim
        .spawn_object_at_height("RUIN", "Americans", 10, 12, 0, 0, &rules)
        .expect("RUIN spawns");
    // A listener force-firing a foundation cell, and installed navigation so
    // the per-cell publish and zone repair run.
    let listener = sim
        .spawn_object_at_height("PLAIN", "Americans", 20, 20, 0, 0, &rules)
        .expect("PLAIN spawns");
    sim.substrate
        .entities
        .get_mut(listener)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Cell(11, 13)));
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .write_literal_bridge_state(sim.resolved_terrain.as_mut().unwrap(), 11, 13, 5);
    assert!(sim.rebuild_dynamic_navigation(&rules));

    kill(&mut sim, &rules, &registry, ruin);

    assert!(
        sim.substrate
            .entities
            .get(ruin)
            .is_none_or(|entity| !entity.lifecycle.object_alive()),
        "the building is gone"
    );
    let ruin_type = sim.interner.get("RUIN").expect("interned type");
    for cell in [(10, 12), (11, 12), (10, 13), (11, 13)] {
        let live = overlay(&sim, cell);
        assert_eq!(live.overlay_id, Some(RUBBLE_OVERLAY_INDEX), "{cell:?}");
        assert_eq!(
            live.rubble_building_type,
            (cell == (10, 12)).then_some(ruin_type),
            "{cell:?}"
        );
    }
    assert_eq!(
        overlay(&sim, (11, 13)).overlay_data,
        5,
        "+0x11E is not written"
    );
    assert_eq!(
        overlay(&sim, (12, 12)).overlay_id,
        None,
        "outside the foundation"
    );
    assert!(
        sim.substrate
            .entities
            .get(listener)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "the cell detach sweep (0x0070D4A0) clears a target on a foundation cell"
    );
    assert!(sim.zone_grid.is_some() && sim.path_grid().is_some());
}

/// No rubble without `LeaveRubble=yes`, and none when the type's image is too
/// short to carry a rubble frame (`0x0045F160` returns false).
#[test]
fn no_rubble_without_leave_rubble_or_rubble_art() {
    let rules = rules();
    let registry = registry();
    for kind in ["PLAIN", "SHORT"] {
        let mut sim = sim();
        let id = sim
            .spawn_object_at_height(kind, "Americans", 10, 12, 0, 0, &rules)
            .expect("building spawns");
        kill(&mut sim, &rules, &registry, id);
        for cell in [(10, 12), (11, 12), (10, 13), (11, 13)] {
            assert_eq!(overlay(&sim, cell).overlay_id, None, "{kind} {cell:?}");
        }
    }
}

/// The crate tail places one crate at the north-west cell, whether or not
/// the type leaves rubble: Money (data 0) for `CrateBeneathIsMoney=`, else the
/// crate Mark's byte for a random powerup at pickup. Its only draw is the
/// slot timer's.
#[test]
fn a_crate_beneath_building_leaves_a_crate_at_its_origin() {
    let rules = rules();
    let registry = registry();
    let wood = registry.id_for_name("WOOD").unwrap();
    for (kind, data) in [("CASH", 0u8), ("GIFT", u8::MAX)] {
        let mut sim = sim();
        let id = sim
            .spawn_object_at_height(kind, "Americans", 10, 12, 0, 0, &rules)
            .expect("building spawns");
        assert!(sim.rebuild_dynamic_navigation(&rules));

        kill(&mut sim, &rules, &registry, id);

        let cell = overlay(&sim, (10, 12));
        assert_eq!(
            (cell.overlay_id, cell.overlay_data),
            (Some(wood), data),
            "{kind}"
        );
        let occupied: Vec<_> = sim
            .crate_authority
            .slots()
            .iter()
            .filter(|slot| !slot.is_empty())
            .map(|slot| (slot.cell_x, slot.cell_y))
            .collect();
        assert_eq!(occupied, [(10, 12)], "{kind}");
    }
}

/// Every retail `CrateBeneath=` type also leaves rubble, and the rubble arm
/// runs first: the origin cell then holds overlay 0xEF, the passable rubble
/// cell is where Find_Nearby_Passable_Cell lands on this flat fixture, and
/// the crate commit's no-overlay check
/// (`CrateSlot__ValidateCellAndCreateOverlay`, reached from `0x004A17C0`)
/// refuses it before the slot is taken or its timer drawn.
/// Composed from the two ported owners; not compared against native
/// execution.
#[test]
fn rubble_on_the_origin_cell_refuses_the_crate_beneath() {
    let rules = rules();
    let registry = registry();
    let mut sim = sim();
    let id = sim
        .spawn_object_at_height("BOTH", "Americans", 10, 12, 0, 0, &rules)
        .expect("BOTH spawns");
    assert!(sim.rebuild_dynamic_navigation(&rules));

    kill(&mut sim, &rules, &registry, id);

    assert_eq!(
        overlay(&sim, (10, 12)).overlay_id,
        Some(RUBBLE_OVERLAY_INDEX)
    );
    assert!(
        sim.crate_authority
            .slots()
            .iter()
            .all(|slot| slot.is_empty())
    );
    // The refusal spends no Scenario draw. Death effects share the stream, so
    // check the placer alone on the rubble-covered origin.
    let rng_before = sim.scenario_rng.state();
    assert!(!crate::sim::crates::place_crate(
        &mut sim,
        &rules,
        &registry,
        (10, 12),
        Some(0)
    ));
    assert_eq!(sim.scenario_rng.state(), rng_before);
}
