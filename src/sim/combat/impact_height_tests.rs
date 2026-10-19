use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::sim::intern::test_interner;
use crate::sim::world::Simulation;

const TEST_GRID: u16 = 16;
/// Terrain floor used by every raised-ground case here. Chosen because two
/// levels is what the reported screenshot showed: one whole tile of
/// vertical error.
const RAISED_LEVEL: u8 = 2;

#[test]
fn gsi_04_11_refinery_survivor_cells_follow_native_sentinel_offsets() {
    let SmudgeSpawnRequest::BuildingCenter {
        foundation_w,
        foundation_h,
        ..
    } = building_center_smudge_request(10, 20, 3, "3x3Refinery")
    else {
        panic!("the destruction-center mark");
    };
    assert_eq!((foundation_w, foundation_h), (3, 3));
    let survivor_cells = crate::sim::crew_survival::foundation_cells(10, 20, "3x3Refinery");
    assert_eq!(
        survivor_cells,
        vec![
            (10, 20),
            (11, 20),
            (12, 20),
            (10, 21),
            (11, 21),
            (10, 22),
            (11, 22),
            (12, 22),
        ]
    );
    assert!(!survivor_cells.contains(&(12, 21)));
}

/// `TechnoClass::ReceiveDamage`'s metallic debris loop
/// (`0x007024E0..0x0070256B`, `tools/spatial_oracle/bridge_debris_producer.py`,
/// executed with the whole AnimClass constructor per piece) on retail
/// rules and art: each piece's `MetallicDebris=` pick, then that piece's
/// constructor draws, before the next pick. The budget is pinned (equal
/// bounds take no draw) so the rows start at the loop.
#[test]
fn debris_pieces_construct_between_picks_as_the_original() {
    let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let Some(mut rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_debris_producer.json",
    ))
    .unwrap();
    // Use the exact retained image-header inputs supplied to the original
    // constructor corpus; the old draw-only test omitted asset binding.
    let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    for native in golden["retail_anim_types"].as_array().unwrap() {
        if native["source"]["art_body_read"] == true {
            registry.bind_anim_frame_count_for_test(
                native["name"].as_str().unwrap(),
                native["frames"].as_i64().unwrap() as i32,
            );
        }
    }
    rules.install_art_data(registry);
    let rows = golden["death_loop"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    for row in rows {
        let input = &row["input"];
        let pieces = input["pieces"].as_i64().unwrap() as i32;
        let mut object_type = rules.object("MTNK").unwrap().clone();
        object_type.max_debris = pieces + 1;
        object_type.min_debris = pieces;
        object_type.debris_types.clear();
        object_type.debris_anims.clear();
        let coord = input["coord"].as_array().unwrap();
        let at = |i: usize| coord[i].as_i64().unwrap() as i32;
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap());
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_before"].as_str().unwrap()
        );
        let mut voxels = Vec::new();
        let mut effects = Vec::new();
        throw_debris_for_death(
            &mut sim,
            &object_type,
            &rules,
            owner,
            glam::IVec3::new(at(0), at(1), at(2)),
            &mut voxels,
            &mut effects,
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "{input}"
        );
        let names: Vec<String> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["call"] == "anim_ctor")
            .map(|event| event["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            sim.substrate
                .anims
                .iter()
                .map(|(_, anim)| sim.interner.resolve(anim.type_id).to_string())
                .collect::<Vec<_>>(),
            names,
            "{input}"
        );
        assert!(
            effects.is_empty() && voxels.is_empty(),
            "live constructors leave no deferred packets"
        );
        assert_eq!(sim.logic_order().len(), names.len());
        for ((_, anim), native) in sim
            .substrate
            .anims
            .iter()
            .zip(row["anims"].as_array().unwrap())
        {
            let location = native["location"].as_array().unwrap();
            assert_eq!(
                [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
                std::array::from_fn(|i| location[i].as_i64().unwrap() as i32),
                "{input}"
            );
            if native["is_bouncing"].as_u64().unwrap() == 0 {
                assert!(
                    anim.bounce.is_none(),
                    "unread retail D is not a Bouncer: {input}"
                );
                assert_eq!(native["type"], "D");
                continue;
            }
            let body = anim.bounce.expect("an ART-read bouncing chunk");
            let bits = |key: &str, i: usize| native["bounce"][key][i].as_u64().unwrap() as u32;
            for axis in 0..3 {
                assert_eq!(body.position[axis].bits(), bits("position_bits", axis));
                assert_eq!(body.velocity[axis].bits(), bits("velocity_bits", axis));
            }
        }
    }
}

fn terrain_cell(rx: u16, ry: u16, level: u8) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        level,
        filled_clear: true,
        accepts_smudge: true,
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
    }
}

pub(super) fn terrain_at_level(level: u8) -> ResolvedTerrainGrid {
    crate::map::resolved_terrain::test_grid(TEST_GRID, TEST_GRID, |rx, ry| {
        terrain_cell(rx, ry, level)
    })
}

/// Armed tank plus a warhead that emits an impact animation, so a
/// force-fire produces an observable `ExplosionEffect`.
fn impact_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "\
[VehicleTypes]\n0=MTNK\n\n\
[InfantryTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[Warheads]\n0=AP\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nAnimList=TWLT070\n",
    );
    RuleSet::from_ini(&ini).expect("impact rules should parse")
}

#[test]
fn ordinary_cell_launch_uses_terrain_ground_leptons() {
    let world = Simulation::new();
    let flat = terrain_at_level(0);
    let raised = terrain_at_level(RAISED_LEVEL);

    assert_eq!(
        attack_world_z_leptons(TargetKind::Cell(7, 9), &world, Some(&raised)),
        208,
        "a non-bridge Cell+58 aim is its ground surface in leptons"
    );
    assert_eq!(
        attack_world_z_leptons(TargetKind::Cell(7, 9), &world, Some(&flat)),
        0,
        "level-0 ground is still zero — that is the value, not the fallback"
    );
    assert_eq!(
        attack_world_z_leptons(TargetKind::Cell(7, 9), &world, None),
        0,
        "no loaded map means no cell to read"
    );
    assert_eq!(
        attack_world_z_leptons(
            TargetKind::Cell(TEST_GRID + 5, TEST_GRID + 5),
            &world,
            Some(&raised)
        ),
        0,
        "mapless test boundary; production fallback targets retain the shared CellClass"
    );
}

/// The impact byte is a signed level count on both sides of the sim/app
/// boundary, because the projection decodes it with `as i8`.
///
/// Catches the two-narrowings shape error: clamping into `u8` range lets
/// 200 through, which the projection reads back as -56 levels and draws
/// 840 px away, while clamping into `i8` range saturates at the top of the
/// domain the reader actually decodes.
#[test]
fn impact_z_byte_saturates_in_the_signed_domain_the_projection_decodes() {
    for level in [0_i32, 1, 2, 14, 127] {
        assert_eq!(
            i32::from(impact_z_byte(level) as i8),
            level,
            "every reachable map height must survive the round trip"
        );
    }
    assert_eq!(
        impact_z_byte(200) as i8,
        i8::MAX,
        "an over-range height saturates at the top of the signed domain, it \
         does not wrap to a large negative one"
    );
    assert_eq!(impact_z_byte(-40) as i8, -40, "below-ground z stays signed");
    assert_eq!(impact_z_byte(-9000) as i8, i8::MIN);
}

#[test]
fn entity_launch_uses_the_object_coordinate() {
    let mut world = Simulation::new();
    let mut on_deck = GameEntity::test_default(1, "MTNK", "Americans", 7, 9);
    on_deck.position.set_z_for_test(6);
    on_deck.set_on_bridge(true);
    world.substrate.entities.insert(on_deck);
    let terrain = terrain_at_level(RAISED_LEVEL);

    assert_eq!(
        attack_world_z_leptons(TargetKind::Entity(1), &world, Some(&terrain)),
        624,
        "the object-owned bridge layer raises its ground coordinate"
    );
    assert_eq!(
        attack_world_z_leptons(TargetKind::Entity(1), &world, None),
        624,
        "the mapless fallback already includes the object layer"
    );
    assert_eq!(
        attack_world_z_leptons(TargetKind::Entity(404), &world, Some(&terrain)),
        0,
        "a vanished target contributes nothing"
    );
}

#[test]
fn force_fire_on_raised_ground_places_the_explosion_at_the_terrain_height() {
    let mut rules = impact_rules();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str(""),
    ));
    let mut terrain = terrain_at_level(RAISED_LEVEL);
    let mut store = EntityStore::new();
    // `test_interner` snapshots the thread-local, so the entity's type and
    // owner strings must be interned before the snapshot is taken.
    let mut firer = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    // This hand-placed combat fixture represents a revealed actor. The
    // live Unit firing host correctly skips constructor-only limbo state.
    firer.lifecycle.set_in_limbo_for_test(false);
    // The fixture `MTNK` authors no `Turret=`, so its HULL is what the
    // native body gate compares (`UnitClass::GetFireError @ 0x00740FD0`
    // step 17). Face it south at the force-fire cell so this test measures
    // impact height, not turn-to-fire.
    firer.body_facing.snap(0x8000, 0);
    store.insert(firer);
    let mut interner = test_interner();
    assert!(
        install_cell_attack_target_for_test(&mut store, 1, 5, 6, Some(&rules), &interner),
        "armed tank should accept a force-fire order on an adjacent cell"
    );

    let mut scenario_rng = SimRng::new(1);
    let result = tick_combat_with_fog(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        None,
        None,
        Some(&mut terrain),
        0,
        100,
        0,
        &[1],
        None,
        &mut scenario_rng,
    );

    let effect = result
        .fixture_anims
        .first()
        .expect("force-fire should construct the warhead's impact animation");
    let (rx, ry, _, _, _) = effect.world_coord.to_cell_sub_z();
    assert_eq!((rx, ry), (5, 6));
    assert_eq!(
        effect.world_coord.z,
        i32::from(RAISED_LEVEL) * 104,
        "the impact animation is placed at the impact height; a constant 0 \
         draws it 15 screen pixels per level below the ground it hit"
    );
}
