//! Rust regression tests for the death-anim producers. Expected draws are
//! replayed on a clone of the Scenario stream in the native order recorded on
//! each producer; they are not native goldens.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::house_state::HouseState;
use crate::sim::rng::SimRng;

const RULES: &str = "\
[General]
AlliedCrew=E1
Engineer=ENGINEER
AlliedSurvivorDivisor=500
RefundPercent=50%
[InfantryTypes]
0=E1
1=ENGINEER
[VehicleTypes]
0=PLAIN
1=APOC
2=VETBOOM
3=AMMOBOOM
4=EMPTYBOOM
5=SHIP
6=SKIFF
7=SUB
[AircraftTypes]
0=JET
[BuildingTypes]
0=PLANT
1=HALL
2=SHED
3=DEPOT
[Warheads]
0=KILLWH
[E1]
Strength=125
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[ENGINEER]
Strength=75
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
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
[SHIP]
Strength=100
Naval=yes
Speed=6
SpeedType=Float
MovementZone=Water
Locomotor={2BEA74E1-7CCA-11d3-BE14-00104B62A16C}
Weight=4
Explosion=EXPA,EXPB,EXPC
[SKIFF]
Strength=100
Naval=yes
Speed=6
SpeedType=Float
MovementZone=Water
Locomotor={2BEA74E1-7CCA-11d3-BE14-00104B62A16C}
Weight=1
Explosion=EXPA,EXPB,EXPC
[SUB]
Strength=100
Naval=yes
Speed=6
SpeedType=Float
MovementZone=Water
Locomotor={2BEA74E1-7CCA-11d3-BE14-00104B62A16C}
Underwater=yes
Weight=4
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
[DEPOT]
Strength=100
Cost=800
Crewed=yes
Foundation=2x2
Explosion=EXPA,EXPB
DestroyAnim=DESTA
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

/// A one-entry DestroyAnim list still spends its pick (`0x00441CCA` draws
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

/// Through the production receiver: DestructionEffects draws each foundation
/// cell's jitter, delay and pick and then the DestroyAnim pick before
/// SpawnSurvivors rolls the same cells (`0x00442665` precedes `0x00441F1B`).
/// The mapless fixture has no smudge candidates, so neither the centre mark
/// nor the per-cell marks draw, and the survivor's Scatter stops after its
/// first draw.
#[test]
fn a_killed_building_draws_its_death_anims_before_its_survivors() {
    let rules = rules();
    let mut survivors = 0;
    for seed in 1..=6 {
        let mut sim = sim(seed);
        let depot = spawn(&mut sim, &rules, "DEPOT", 10, 20);
        let location = position_world_coord(&sim.substrate.entities.get(depot).unwrap().position);
        let before = sim.substrate.entities.keys_sorted();
        let mut replay = sim.scenario_rng.clone();
        kill(&mut sim, &rules, depot);

        let cells = crate::sim::crew_survival::foundation_cells(10, 20, "2x2");
        let mut expected = Vec::new();
        for &(rx, ry) in &cells {
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
        let corner = AnimWorldCoord {
            x: location.x - 0x80,
            y: location.y - 0x80,
            z: location.z,
        };
        expected.push((pick(&mut replay, &["DESTA"]), corner, 0, 0x600, 0));
        // SpawnSurvivors Phase B for the one owed survivor: per cell the
        // `RandomRanged(0, 2)` roll; a hit spends the Engineer roll (a depot
        // is no yard), the constructor word, the centre-row placement, the
        // health roll and Scatter's first draw.
        let mut owed = 1;
        let mut healths = Vec::new();
        for _ in &cells {
            if owed > 0 && replay.next_range_u32_inclusive(0, 2) == 1 {
                let _engineer = replay.next_range_u32_inclusive(0, 99);
                let _constructor = replay.next_u32();
                let _row = replay.next_range_u32(4);
                healths.push(replay.next_range_i32_inclusive(5, 125));
                let _scatter = replay.next_range_u32_inclusive(0, 4);
                owed -= 1;
            }
        }

        assert_eq!(anims(&sim), expected, "seed {seed}");
        assert_eq!(sim.scenario_rng.state(), replay.state(), "seed {seed}");
        let crew: Vec<_> = sim
            .substrate
            .entities
            .keys_sorted()
            .into_iter()
            .filter(|id| !before.contains(id))
            .map(|id| sim.substrate.entities.get(id).unwrap().health.current)
            .collect();
        assert_eq!(crew, healths, "seed {seed}");
        survivors += crew.len();
    }
    assert!(survivors > 0, "the seeds exercise a survivor");
}

/// `0x00737DE2..0x00737E5E`: a surface naval unit at least
/// `ShipSinkingWeight=` heavy (default 3.0) dying on a water cell sinks and
/// never reaches `Death_Explosion`: no pick, no anim. On a cell whose LandType
/// is not water, below the weight or `Underwater=`, it explodes.
#[test]
fn a_heavy_ship_dying_on_water_sinks_without_its_explosion() {
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile};
    let rules = rules();
    let open = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    for (kind, water, sinks) in [
        ("SHIP", true, true),
        ("SHIP", false, false),
        ("SKIFF", true, false),
        ("SUB", true, false),
    ] {
        let mut sim = sim(9);
        let cells = (0..20_u16)
            .flat_map(|y| (0..20_u16).map(move |x| (x, y)))
            .map(|(x, y)| {
                let mut cell = test_flat_cell(x, y);
                cell.speed_costs = open;
                cell.base_speed_costs = open;
                if (x, y) == (12, 12) {
                    cell.land_type = LandType::Water.as_index();
                    cell.yr_cell_land_type = LandType::Water.as_index();
                    cell.is_water = true;
                }
                cell
            })
            .collect();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(20, 20, cells));
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 20,
            off_fc: -128,
            off_100: -128,
            off_104: 256,
            off_108: 256,
        });
        sim.playfield_size_height = Some(20);
        assert!(sim.rebuild_dynamic_navigation(&rules));
        let ship = spawn(&mut sim, &rules, kind, 12, 12);
        if !water {
            // Only the gate's `+0xEC` read changes; the ship stays placed.
            let cell = sim.resolved_terrain.as_mut().unwrap().cell_mut(12, 12);
            cell.unwrap().yr_cell_land_type = LandType::Clear.as_index();
        }
        let mut replay = sim.scenario_rng.clone();
        kill(&mut sim, &rules, ship);
        let played: Vec<_> = anims(&sim).into_iter().map(|anim| anim.0).collect();
        if sinks {
            assert!(played.is_empty(), "{kind}: {played:?}");
        } else {
            assert_eq!(
                played,
                vec![pick(&mut replay, &["EXPA", "EXPB", "EXPC"])],
                "{kind} water {water}"
            );
        }
        assert_eq!(
            sim.scenario_rng.state(),
            replay.state(),
            "{kind} water {water}"
        );
    }
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
        plant_anims
            .iter()
            .all(|(name, delay, z)| in_list("GAPOWR", name) && *delay <= 3 && *z == 0)
    );
    // This seed's four picks: two art-less `gtpowexp` (nothing constructed)
    // and these two.
    let names: Vec<_> = plant_anims.iter().map(|anim| anim.0.as_str()).collect();
    assert_eq!(names, ["S_CLSN58", "S_TUMU60"], "{plant_anims:?}");
    let mcv_anims = &per_kill[1];
    assert_eq!(mcv_anims.len(), 1, "{mcv_anims:?}");
    assert!(in_list("AMCV", &mcv_anims[0].0) && mcv_anims[0].1 == 0);
}
