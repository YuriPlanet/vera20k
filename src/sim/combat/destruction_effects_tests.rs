//! Rust regression tests for the death-anim producers. Expected draws are
//! replayed on a clone of the Scenario stream in the native order recorded on
//! each producer; they are not native goldens.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::house_state::HouseState;
use crate::sim::rng::SimRng;

const RULES: &str = "\
[InfantryTypes]
[VehicleTypes]
0=PLAIN
1=APOC
2=VETBOOM
3=AMMOBOOM
4=EMPTYBOOM
[AircraftTypes]
0=JET
[BuildingTypes]
0=PLANT
1=HALL
2=SHED
[Warheads]
0=KILLWH
[PLAIN]
Strength=100
Explosion=EXPA,EXPB,EXPC
DestroyAnim=DESTA
[APOC]
Strength=100
Explodes=yes
Explosion=EXPA,EXPB,EXPC
[VETBOOM]
Strength=100
VeteranAbilities=EXPLODES
Explosion=EXPA,EXPB,EXPC
[AMMOBOOM]
Strength=100
Explodes=yes
Ammo=3
Explosion=EXPA,EXPB,EXPC
[EMPTYBOOM]
Strength=100
Explodes=yes
Ammo=0
Explosion=EXPA,EXPB,EXPC
[JET]
Strength=100
Explosion=EXPA,EXPB
DestroyAnim=DESTA
[PLANT]
Strength=100
Foundation=2x2
Explosion=EXPA,EXPB
DestroyAnim=DESTA
[HALL]
Strength=100
Foundation=1x1
DestroyAnim=DESTA
[SHED]
Strength=100
Foundation=1x1
[KILLWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

const ART: &str = "\
[EXPA]
End=10
[EXPB]
End=10
[EXPC]
End=10
[DESTA]
End=10
";

fn rules() -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("death anim rules");
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(ART));
    for name in ["EXPA", "EXPB", "EXPC", "DESTA"] {
        art.bind_anim_frame_count_for_test(name, 10);
    }
    rules.art_registry = art;
    rules
}

fn sim(seed: u64) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    sim.session.house_order.push(owner);
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, "Americans", rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

/// Every anim in creation order: (type, coordinate, delay, flags, zAdjust).
fn anims(sim: &Simulation) -> Vec<(String, AnimWorldCoord, u16, u32, i32)> {
    sim.substrate
        .anims
        .iter()
        .map(|(_, anim)| {
            (
                sim.interner.resolve(anim.type_id).to_string(),
                anim.world_coord,
                anim.runtime.delay_remaining,
                anim.draw_flags,
                anim.z_adjust,
            )
        })
        .collect()
}

/// The consequence boundary's admission of the recorded death anims.
fn admit(sim: &mut Simulation, rules: &RuleSet, recorded: Vec<ExplosionEffect>) {
    for effect in recorded {
        let spawn = effect.death.expect("a death producer's record");
        sim.admit_death_anim(rules, effect.shp_name, spawn);
    }
}

fn pick(rng: &mut SimRng, list: &[&str]) -> String {
    list[(rng.next_u32() % list.len() as u32) as usize].to_string()
}

/// DestructionEffects steps 7, 8 and 13: the centre mark first, then per
/// foundation cell (list order) the 0x40 jitter draw, the `RandomRanged(0, 3)`
/// delay and the type pick, each constructed at once with `0x600`/0; then the
/// DestroyAnim pick at the origin cell's corner.
#[test]
fn a_destroyed_building_explodes_per_foundation_cell_then_plays_its_destroy_anim() {
    let rules = rules();
    for seed in 1..=4 {
        let mut sim = sim(seed);
        let plant = spawn(&mut sim, &rules, "PLANT", 10, 20);
        let location = position_world_coord(&sim.substrate.entities.get(plant).unwrap().position);
        let mut replay = sim.scenario_rng.clone();
        let mut marks = Vec::new();
        let mut recorded = Vec::new();
        sim.building_destruction_anims(&rules, plant, &mut recorded, |_, request| {
            marks.push(request)
        });
        admit(&mut sim, &rules, recorded);

        let mut expected = Vec::new();
        for (rx, ry) in crate::sim::crew_survival::foundation_cells(10, 20, "2x2") {
            let (x, y) = super::super::inviso_scatter::random_direction_coord(
                &mut replay,
                i32::from(rx) * 256 + 0x80,
                i32::from(ry) * 256 + 0x80,
                0x40,
            );
            let delay = replay.next_range_u32_inclusive(0, 3) as u16;
            let anim = pick(&mut replay, &["EXPA", "EXPB"]);
            let coord = AnimWorldCoord {
                x,
                y,
                z: location.z,
            };
            expected.push((anim, coord, delay, 0x600, 0));
        }
        let destroy = pick(&mut replay, &["DESTA"]);
        let corner = AnimWorldCoord {
            x: location.x - 0x80,
            y: location.y - 0x80,
            z: location.z,
        };
        expected.push((destroy, corner, 0, 0x600, 0));

        assert_eq!(anims(&sim), expected, "seed {seed}");
        assert_eq!(sim.scenario_rng.state(), replay.state(), "seed {seed}");
        assert!(matches!(
            marks.as_slice(),
            [SmudgeSpawnRequest::BuildingCenter {
                rx: 10,
                ry: 20,
                foundation_w: 2,
                foundation_h: 2,
                ..
            }]
        ));
    }
}

/// A one-entry DestroyAnim list still spends its pick (`0x0073881D` draws
/// before the modulo); a building with neither list draws nothing.
#[test]
fn a_single_destroy_anim_still_draws_and_no_lists_draw_nothing() {
    let rules = rules();
    let mut sim = sim(7);
    let hall = spawn(&mut sim, &rules, "HALL", 10, 20);
    let shed = spawn(&mut sim, &rules, "SHED", 20, 20);

    let mut replay = sim.scenario_rng.clone();
    let mut recorded = Vec::new();
    sim.building_destruction_anims(&rules, hall, &mut recorded, |_, _| {});
    admit(&mut sim, &rules, recorded);
    let _pick = replay.next_u32();
    assert_eq!(sim.scenario_rng.state(), replay.state());
    assert_eq!(anims(&sim).len(), 1);

    let before = sim.scenario_rng.state();
    let mut recorded = Vec::new();
    sim.building_destruction_anims(&rules, shed, &mut recorded, |_, _| {});
    assert!(recorded.is_empty());
    assert_eq!(sim.scenario_rng.state(), before);
    assert_eq!(anims(&sim).len(), 1);
}

/// `UnitClass::Death_Explosion`: one Explosion= pick then one DestroyAnim=
/// pick, both at the unit's coordinate with `0x600`/0 and no delay.
#[test]
fn a_dying_vehicle_plays_its_explosion_then_its_destroy_anim() {
    let rules = rules();
    for seed in 1..=4 {
        let mut sim = sim(seed);
        let unit = spawn(&mut sim, &rules, "PLAIN", 12, 12);
        let location = position_world_coord(&sim.substrate.entities.get(unit).unwrap().position);
        let coord = AnimWorldCoord {
            x: location.x,
            y: location.y,
            z: location.z,
        };
        let mut replay = sim.scenario_rng.clone();
        let mut recorded = Vec::new();
        sim.unit_death_explosion(&rules, unit, &mut recorded);
        admit(&mut sim, &rules, recorded);
        let explosion = pick(&mut replay, &["EXPA", "EXPB", "EXPC"]);
        let destroy = pick(&mut replay, &["DESTA"]);
        assert_eq!(
            anims(&sim),
            vec![
                (explosion, coord, 0, 0x600, 0),
                (destroy, coord, 0, 0x600, 0)
            ],
            "seed {seed}"
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }
}

/// `0x007386C3..0x007386FF`: an `Explodes=` vehicle (the Apocalypse, the
/// Demolition Truck) or one with the EXPLODES veteran ability dies with its
/// last `Explosion=` entry when it has ammo left; the pick is still drawn.
#[test]
fn an_exploding_vehicle_with_ammo_dies_with_its_last_explosion() {
    let rules = rules();
    for (kind, veteran, last) in [
        ("APOC", false, true),
        ("AMMOBOOM", false, true),
        ("EMPTYBOOM", false, false),
        ("VETBOOM", false, false),
        ("VETBOOM", true, true),
    ] {
        for seed in 1..=3 {
            let mut sim = sim(seed);
            let unit = spawn(&mut sim, &rules, kind, 12, 12);
            if veteran {
                crate::sim::combat::veterancy::set_veteran(
                    sim.substrate.entities.get_mut(unit).unwrap(),
                );
            }
            let mut replay = sim.scenario_rng.clone();
            let mut recorded = Vec::new();
            sim.unit_death_explosion(&rules, unit, &mut recorded);
            admit(&mut sim, &rules, recorded);
            let picked = pick(&mut replay, &["EXPA", "EXPB", "EXPC"]);
            let expected = if last { "EXPC".to_string() } else { picked };
            let played: Vec<_> = anims(&sim).into_iter().map(|anim| anim.0).collect();
            assert_eq!(
                played,
                vec![expected],
                "{kind} veteran {veteran} seed {seed}"
            );
            assert_eq!(sim.scenario_rng.state(), replay.state());
        }
    }
}

/// The Aircraft death arm plays one Explosion= anim and no DestroyAnim=.
#[test]
fn a_dying_aircraft_plays_one_explosion_and_no_destroy_anim() {
    let rules = rules();
    let mut sim = sim(3);
    let jet = spawn(&mut sim, &rules, "JET", 12, 12);
    let mut replay = sim.scenario_rng.clone();
    let mut recorded = Vec::new();
    sim.aircraft_death_explosion(&rules, jet, &mut recorded);
    admit(&mut sim, &rules, recorded);
    let explosion = pick(&mut replay, &["EXPA", "EXPB"]);
    let played: Vec<_> = anims(&sim).into_iter().map(|anim| anim.0).collect();
    assert_eq!(played, vec![explosion]);
    assert_eq!(sim.scenario_rng.state(), replay.state());
}

fn kill(sim: &mut Simulation, rules: &RuleSet, id: u64) {
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
    sim.commit_noncombat_aoe_hits(rules, None, &[hit]);
}

/// Through the production receiver: a killed vehicle plays its own
/// `Explosion=` and then its `DestroyAnim=` as live anims, and a killed
/// building one explosion per foundation cell before its DestroyAnim, all
/// with the death producers' `0x600`/0 constructor arguments.
#[test]
fn gsi_08_11_killing_hits_play_the_types_death_anims() {
    let rules = rules();
    let mut sim = sim(5);
    let unit = spawn(&mut sim, &rules, "PLAIN", 12, 12);
    kill(&mut sim, &rules, unit);
    let played: Vec<_> = anims(&sim).into_iter().map(|anim| anim.0).collect();
    assert_eq!(played.len(), 2, "{played:?}");
    assert!(["EXPA", "EXPB", "EXPC"].contains(&played[0].as_str()));
    assert_eq!(played[1], "DESTA");

    let mut sim = self::sim(5);
    let plant = spawn(&mut sim, &rules, "PLANT", 10, 20);
    kill(&mut sim, &rules, plant);
    let played: Vec<_> = anims(&sim).into_iter().map(|anim| anim.0).collect();
    assert_eq!(played.len(), 5, "{played:?}");
    assert!(
        played[..4]
            .iter()
            .all(|name| name == "EXPA" || name == "EXPB")
    );
    assert_eq!(played[4], "DESTA");
    assert!(
        anims(&sim)[..4].iter().all(|anim| anim.2 <= 3),
        "per-cell delays come from RandomRanged(0, 3)"
    );
    assert!(
        anims(&sim)
            .iter()
            .all(|anim| (anim.3, anim.4) == (0x600, 0))
    );
}

/// Retail Dustbowl runtime: a power plant killed through the production
/// receiver with retail art bound plays one `Explosion=` anim per foundation
/// cell (an art-less `gtpowexp` pick constructs nothing) with delays in 0..=3,
/// and an MCV its own explosion, all with `0x600`/0. Ignored: needs the retail
/// install (`RA2_DIR` or `config.toml`).
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_death_anims_use_the_types_lists() {
    let dir = std::env::var("RA2_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::util::config::GameConfig::load()
                .expect("set RA2_DIR or provide config.toml for this ignored test")
                .paths
                .ra2_dir
        });
    let mut scenario =
        crate::headless_scenario::load(&dir, "Dustbowl.mmx", 0x00C0_FFEE).expect("Dustbowl loads");
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let owner = sim.interner.intern("Americans");
    sim.houses
        .entry(owner)
        .or_insert_with(|| HouseState::new(owner, 0, None, false, 10_000, 10));
    if !sim.session.house_order.contains(&owner) {
        sim.session.house_order.push(owner);
    }
    let (plant, mcv) = (40..100_u16)
        .flat_map(|y| (40..100_u16).map(move |x| (x, y)))
        .find_map(|(x, y)| {
            let grid = sim.path_grid()?;
            let terrain = sim.resolved_terrain.as_ref()?;
            let level = terrain.cell(x.checked_sub(3)?, y)?.level;
            let (x0, y0) = (x.checked_sub(4)?, y.checked_sub(1)?);
            let open = (x0..=x + 1).all(|cx| {
                (y0..=y + 2).all(|cy| {
                    terrain.cell(cx, cy).is_some_and(|cell| cell.level == level)
                        && grid.cell(cx, cy).is_some_and(|cell| cell.ground_walkable)
                })
            });
            if !open {
                return None;
            }
            let mcv = sim.spawn_object(
                "AMCV",
                "Americans",
                x,
                y,
                0,
                &resources.rules,
                &resources.height_map,
            )?;
            let plant = sim.spawn_object(
                "GAPOWR",
                "Americans",
                x - 3,
                y,
                0,
                &resources.rules,
                &resources.height_map,
            )?;
            Some((plant, mcv))
        })
        .expect("an MCV cell with room for a power plant");
    sim.resolve_type_handles(&resources.rules);
    let warhead = sim.interner.intern("Super");
    let mut per_kill = Vec::new();
    for id in [plant, mcv] {
        let before: Vec<_> = sim.substrate.anims.iter().map(|(id, _)| *id).collect();
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
        sim.commit_noncombat_aoe_hits(&resources.rules, Some(&resources.overlay_registry), &[hit]);
        let deaths: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .filter(|(id, anim)| !before.contains(id) && anim.draw_flags == 0x600)
            .map(|(_, anim)| {
                (
                    sim.interner.resolve(anim.type_id).to_string(),
                    anim.runtime.delay_remaining,
                    anim.z_adjust,
                )
            })
            .collect();
        println!("death anims of {id}: {deaths:?}");
        per_kill.push(deaths);
    }
    let rules = &resources.rules;
    let in_list = |kind: &str, name: &str| {
        rules
            .object(kind)
            .unwrap()
            .explosion_anims
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(name))
    };
    let plant_anims = &per_kill[0];
    assert!(
        plant_anims.len() <= 4,
        "one per foundation cell, less art-less picks: {plant_anims:?}"
    );
    assert!(
        plant_anims
            .iter()
            .all(|(name, delay, z)| in_list("GAPOWR", name) && *delay <= 3 && *z == 0)
    );
    let mcv_anims = &per_kill[1];
    assert_eq!(mcv_anims.len(), 1, "{mcv_anims:?}");
    assert!(in_list("AMCV", &mcv_anims[0].0) && mcv_anims[0].1 == 0);
}
