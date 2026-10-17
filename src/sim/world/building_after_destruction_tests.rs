//! Rust regression tests for `BuildingClass::AfterDestruction`'s rubble arm
//! and crate tail, driven through the production damage receiver.
//! Expectations follow the instruction reading recorded in
//! `building_after_destruction`; they are not native goldens.

use std::fmt::Write as _;

use crate::rules::building_rubble::RubbleArt;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::overlay_grid::{OverlayGrid, RUBBLE_OVERLAY_INDEX};
use crate::sim::world::Simulation;

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
            shadow_frame: 7,
        },
    );
    rules.set_building_rubble_art_for_test(
        "RUIN",
        RubbleArt {
            shp: "RUIN.SHP".into(),
            body_frame: 3,
            shadow_frame: 7,
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

/// Original440042..4400EA controls retain exact-zero timer expiry, including
/// negative paused durations and signed frame wrap. The native terminal callees
/// are observation sinks; occupancy/rubble below are Rust integration checks.
#[test]
fn zero_health_building_ai_matches_original_terminal_gate() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_zero_health.json",
    ))
    .unwrap();
    let rules = rules();
    let registry = registry();
    for row in corpus.as_array().unwrap() {
        let input = &row["input"];
        let mut sim = sim();
        sim.session.binary_frame = input["constructor_frame"].as_i64().unwrap() as u32;
        let id = sim
            .spawn_object_at_height("PLAIN", "Americans", 10, 12, 0, 0, &rules)
            .unwrap();
        let building = sim.substrate.entities.get_mut(id).unwrap();
        building
            .health
            .set(input["health"].as_i64().unwrap() as i32);
        if let Some(timer) = input["timer_override"].as_array() {
            building.pending_c4_detonation.set_for_test(Some(
                crate::sim::c4_plant::PendingC4Detonation::new(
                    crate::sim::timer::CdTimer::from_raw(
                        timer[0].as_i64().unwrap() as i32,
                        timer[1].as_i64().unwrap() as i32,
                    ),
                    None,
                ),
            ));
        }
        sim.session.binary_frame = input["frame"].as_i64().unwrap() as u32;
        sim.object_ai_visit_one(
            id,
            Some(&rules),
            super::techno_ai::ObjectAiCtx {
                overlay_registry: Some(&registry),
                ..Default::default()
            },
        );
        let terminal = !row["output"]["terminal_callouts"]
            .as_array()
            .unwrap()
            .is_empty();
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .is_none_or(|e| e.lifecycle.in_limbo()),
            terminal,
            "{}",
            row["name"]
        );
        assert_eq!(
            sim.occupancy()
                .first_building_on_layer(
                    10,
                    12,
                    crate::sim::movement::locomotor::MovementLayer::Ground
                )
                .is_none(),
            terminal,
            "{}",
            row["name"]
        );
    }
}
