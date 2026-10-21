//! Rust regression tests for the death-anim producers. Expected draws are
//! replayed on a clone of the Scenario stream in the native order recorded on
//! each producer; they are not native goldens. The retail Dustbowl test is the
//! exception: it compares a building death against native execution
//! (`tools/spatial_oracle/building_death_anims.py`).

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
4=STATUE
[Warheads]
0=KILLWH
1=Super
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
Explosion=EXPA,EXPB
DestroyAnim=DESTA
[HALL]
Strength=100
DestroyAnim=DESTA
[SHED]
Strength=100
[STATUE]
Strength=100
DestroyAnim=DESTA
[DEPOT]
Strength=100
Cost=800
Crewed=yes
Explosion=EXPA,EXPB
DestroyAnim=DESTA
[KILLWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[Super]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
Tiberium=yes
ProneDamage=100%
Sparky=no
InfDeath=2
PenetratesBunker=yes
AnimList=XGRYSML1,XGRYSML2,EXPLOSML,XGRYMED1,XGRYMED2,EXPLOMED,EXPLOLRG,TWLT070
";

const ART: &str = "\
[PLANT]
Foundation=2x2
[HALL]
Foundation=1x1
[SHED]
Foundation=1x1
[STATUE]
Foundation=1x1
Palette=lib
[DEPOT]
Foundation=2x2
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
    rules_from(RULES)
}

fn rules_from(text: &str) -> RuleSet {
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(text),
        &IniFile::from_str(ART),
    )
    .expect("death anim rules");
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(ART));
    for name in ["EXPA", "EXPB", "EXPC", "DESTA"] {
        art.bind_anim_frame_count_for_test(name, 10);
    }
    rules.install_art_fixture(art);
    rules
}

fn sim(seed: u64) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, false, 0, 10));
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
                sim.interner.resolve(anim.type_id()).to_string(),
                anim.world_coord(),
                anim.runtime().delay_remaining,
                anim.draw_flags(),
                anim.z_adjust(),
            )
        })
        .collect()
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
        sim.building_destruction_effects(&rules, plant, false, &mut recorded, |_, request| {
            marks.push(request)
        });

        let mut expected = Vec::new();
        for (rx, ry) in crate::sim::crew_survival::foundation_cells(10, 20, "2x2") {
            let (x, y) = super::super::inviso_scatter::random_direction_coord(
                &mut replay,
                i32::from(rx) * 256 + 0x80,
                i32::from(ry) * 256 + 0x80,
                0x40,
                super::super::inviso_scatter::RandomDirectionSnap::Preserve,
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
    sim.building_destruction_effects(&rules, hall, false, &mut recorded, |_, _| {});
    let _pick = replay.next_u32();
    assert_eq!(sim.scenario_rng.state(), replay.state());
    assert_eq!(anims(&sim).len(), 1);

    let before = sim.scenario_rng.state();
    let mut recorded = Vec::new();
    sim.building_destruction_effects(&rules, shed, false, &mut recorded, |_, _| {});
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
                sim.substrate
                    .entities
                    .get_mut(unit)
                    .unwrap()
                    .veterancy
                    .set_veteran();
            }
            let mut replay = sim.scenario_rng.clone();
            let mut recorded = Vec::new();
            sim.unit_death_explosion(&rules, unit, &mut recorded);
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
    let explosion = pick(&mut replay, &["EXPA", "EXPB"]);
    let played: Vec<_> = anims(&sim).into_iter().map(|anim| anim.0).collect();
    assert_eq!(played, vec![explosion]);
    assert_eq!(sim.scenario_rng.state(), replay.state());
}

fn kill(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    kill_with(sim, rules, id, "KILLWH", RAD_NO_ATTACKER, None);
}

fn kill_with(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    warhead: &str,
    attacker: u64,
    house: Option<InternedId>,
) {
    let warhead = sim.interner.intern(warhead);
    let hit = EntityDamageEvent::direct_receiver(
        id,
        100_000,
        0,
        attacker,
        house,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(
        rules,
        None,
        &[hit],
        crate::sim::world::FrameEffects::default(),
    );
}

/// A 20x20 open map whose cell (12, 12) stands two levels up, on Water or
/// Clear.
fn water_cell_sim(rules: &RuleSet, water: bool) -> Simulation {
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
    use crate::rules::terrain_rules::SpeedCostProfile;

    let open = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    let mut sim = sim(9);
    let cells = (0..20_u16)
        .flat_map(|y| (0..20_u16).map(move |x| (x, y)))
        .map(|(x, y)| {
            let mut cell = test_flat_cell(x, y);
            cell.speed_costs = open;
            cell.base_speed_costs = open;
            if (x, y) == (12, 12) {
                cell.level = 2;
                cell.land_type = if water {
                    LandType::Water.as_index()
                } else {
                    LandType::Clear.as_index()
                };
                cell.base_land_type = cell.land_type;
            }
            cell
        })
        .collect();
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(20, 20, cells));
    sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 20,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    }));
    sim.set_playfield_size_height_for_test(Some(20));
    assert!(sim.rebuild_dynamic_navigation(rules));
    sim
}

/// Puts `id` `height` leptons above cell (12, 12)'s two ground levels and,
/// for a bomb, lands it from a DropAsBomb fall (the +8F byte persists).
fn drop_at_height(sim: &mut Simulation, id: u64, bomb: bool, height: i32) {
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    // Two ground levels exercise a raw world Z rather than a floor-Z
    // substitute. Native GetHeight still sees the given height.
    entity.position.set_exact_z_leptons_for_test(Some(
        2 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS + height,
    ));
    if bomb {
        entity.begin_bomb_fall();
        entity.finish_fall();
        assert!(entity.was_dropped_as_bomb(), "landing retains +8F");
    }
}

/// Rust regression through the production damage receiver. Instruction
/// evidence: Techno702232..70227B (retail gamemd) jumps to 0x702672 for a
/// low dropped bomb over water, past KillPassengers (0x702603..0x702667) and
/// Fire_Death_Weapon (0x70266D) as well as the debris. An `Explodes=` unit
/// that falls off a bridge into water sets off no death weapon; high or dry,
/// it still does. Its passenger dies either way: skipped by KillPassengers,
/// it meets the unit tail's escape, which kills every passenger of a dropped
/// bomb (+0x8F, `0x007380AF` -> RecordKill and UnInit at `0x00738188`).
#[test]
fn a_low_dropped_bomb_over_water_fires_no_death_weapon() {
    let text = RULES
        .replace("1=ENGINEER\n", "1=ENGINEER\n2=BYSTANDER\n")
        .replace(
            "[APOC]\nStrength=100\nExplodes=yes\n",
            "[APOC]\nStrength=100\nExplodes=yes\nDeathWeapon=BOOMW\nPassengers=1\n",
        )
        + "\n[BYSTANDER]\nStrength=125\nSpeed=4\n\
           Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
           [BOOMW]\nDamage=50\nWarhead=BOOMWH\n\
           [BOOMWH]\nCellSpread=2\nPercentAtMax=100%\n\
           Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
    let rules = rules_from(&text);
    for (bomb, height, water, fires) in [
        (true, 0, true, false),
        (true, 10, true, false),
        (true, 11, true, true),
        (false, 0, true, true),
        (true, 0, false, true),
    ] {
        let case = format!("bomb={bomb} height={height} water={water}");
        let mut sim = water_cell_sim(&rules, water);
        let apoc = spawn(&mut sim, &rules, "APOC", 12, 12);
        let bystander = spawn(&mut sim, &rules, "BYSTANDER", 13, 12);
        let rider = spawn(&mut sim, &rules, "E1", 11, 12);
        assert!(crate::sim::passenger::board_passenger(
            &mut sim,
            &rules,
            rider,
            apoc,
            crate::sim::passenger::BoardingArm::InfantryTransport,
            None,
            crate::sim::world::FrameEffects::default(),
        ));
        drop_at_height(&mut sim, apoc, bomb, height);

        kill(&mut sim, &rules, apoc);

        assert_eq!(
            sim.substrate
                .entities
                .get(bystander)
                .unwrap()
                .health
                .current()
                < 125,
            fires,
            "death weapon: {case}"
        );
        assert!(
            sim.substrate
                .entities
                .get(rider)
                .is_none_or(|rider| !rider.lifecycle.object_alive()),
            "passenger: {case}"
        );
    }
}

/// Rust regression through the production damage receiver. Instruction
/// evidence: Techno702232, Unit737E78 and Infantry5181B5 (retail gamemd).
/// Both water branches retain Location Z and construct Wake before the
/// selected splash, with no selection draws. The low bomb gate alone skips
/// Techno's debris; high bombs and ordinary water deaths still consume it.
#[test]
fn low_dropped_bombs_use_class_water_splashes_without_debris() {
    let text = RULES
        .replace("[General]\n", "[General]\nWake=EXPA\n")
        .replace("1=ENGINEER\n", "1=ENGINEER\n2=ROCKET\n")
        .replace(
            "[PLAIN]\nStrength=100\nExplosion=EXPA,EXPB,EXPC\nDestroyAnim=DESTA",
            "[PLAIN]\nStrength=100\nExplosion=DESTA\nMaxDebris=2\nMinDebris=1\nDebrisAnims=DESTA",
        )
        + "\n[CombatDamage]\nSplashList=EXPC,EXPB\n\
           [ROCKET]\nStrength=100\nJumpJet=yes\nCrashable=yes\nSpeed=4\n\
           Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n";
    let rules = rules_from(&text);
    for (kind, bomb, height, water, splash) in [
        ("PLAIN", true, 10, true, Some(("EXPB", 5))),
        ("PLAIN", true, 0, true, Some(("EXPB", 5))),
        ("PLAIN", false, 0, true, None),
        ("PLAIN", true, 11, true, None),
        ("PLAIN", true, 0, false, None),
        ("E1", true, 10, true, Some(("EXPC", 3))),
        ("E1", true, 0, true, Some(("EXPC", 3))),
        ("ROCKET", true, 0, true, Some(("EXPC", 3))),
        ("ROCKET", true, 10, true, Some(("EXPC", 3))),
        ("SHIP", true, 0, true, None),
    ] {
        let mut sim = water_cell_sim(&rules, water);
        let id = spawn(&mut sim, &rules, kind, 12, 12);
        drop_at_height(&mut sim, id, bomb, height);
        let raw = position_world_coord(&sim.substrate.entities.get(id).unwrap().position);
        let before = sim.scenario_rng.state();
        kill(&mut sim, &rules, id);
        let played = anims(&sim);
        if let Some((name, lift)) = splash {
            assert_eq!(
                played,
                vec![
                    (
                        "EXPA".to_string(),
                        AnimWorldCoord {
                            x: raw.x,
                            y: raw.y,
                            z: raw.z
                        },
                        0,
                        0x600,
                        0,
                    ),
                    (
                        name.to_string(),
                        AnimWorldCoord {
                            x: raw.x,
                            y: raw.y,
                            z: raw.z + lift
                        },
                        0,
                        0x600,
                        0,
                    ),
                ],
                "{kind} bomb={bomb} height={height} water={water}",
            );
            // Jumpjet Stop_Driver legitimately consumes placement draws
            // before this concrete death arm. Plain foot/vehicle controls
            // isolate the splash selections' absence of RNG draws.
            if kind != "ROCKET" {
                assert_eq!(sim.scenario_rng.state(), before, "no selection draw");
            }
            if kind == "ROCKET" && height > 0 {
                let entity = sim.substrate.entities.get(id).unwrap();
                assert!(
                    entity.crash.is_crashing(),
                    "common Crashable tail follows splash"
                );
                assert!(entity.lifecycle.object_alive());
                assert_eq!(entity.health.current(), 0);
                assert!(!entity.death.is_dying());
                assert!(!sim.substrate.pending_delete.contains_object(&id));
                continue;
            }
            assert!(
                !sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .lifecycle
                    .object_alive()
            );
            assert!(sim.substrate.pending_delete.contains_object(&id));
        } else if kind == "SHIP" {
            assert!(played.is_empty(), "ship sinking precedes water splash");
            let ship = sim.substrate.entities.get(id).unwrap();
            assert!(ship.sinking.is_active());
            assert_eq!(ship.health.current(), 1);
        } else {
            assert!(
                played.len() >= 2,
                "debris plus ordinary explosion: {played:?}"
            );
            assert!(played.iter().all(|anim| anim.0 == "DESTA"));
            assert_ne!(sim.scenario_rng.state(), before, "debris/explosion draw");
        }
    }
}

/// Through the production receiver: an infantryman's InfDeath anim is built
/// inline at its Location with the death producers' `0x600`/0 arguments
/// (InfDeath 3 at `0x00518693`), and only InfDeath 8's `InfantryVirus=`
/// takes an owner (`0x0051887B..0x005188A9`): the killing source's house,
/// else the receiver's house argument.
#[test]
fn infantry_death_anims_are_built_inline_and_the_virus_takes_the_killers_house() {
    let text = RULES
        .replace(
            "[General]\n",
            "[General]\nInfantryExplode=EXPA\nInfantryVirus=EXPB\n",
        )
        .replace("1=Super\n", "1=Super\n2=BLASTWH\n3=VIRUSWH\n")
        + "[BLASTWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nInfDeath=3\n\
[VIRUSWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nInfDeath=8\n";
    let rules = rules_from(&text);
    let owners = |sim: &Simulation| -> Vec<Option<String>> {
        sim.substrate
            .anims
            .iter()
            .map(|(_, anim)| {
                anim.owner_house()
                    .map(|house| sim.interner.resolve(house).to_string())
            })
            .collect()
    };
    let world = || {
        let mut sim = sim(5);
        let russians = sim.interner.intern("Russians");
        sim.houses
            .register(HouseState::new(russians, 1, None, false, 0, 10));
        (sim, russians)
    };

    let (mut sim, _) = world();
    let victim = spawn(&mut sim, &rules, "E1", 12, 12);
    let [x, y] = crate::sim::movement::ground_pose::position_world_xy(
        &sim.substrate.entities.get(victim).unwrap().position,
    );
    kill_with(&mut sim, &rules, victim, "BLASTWH", RAD_NO_ATTACKER, None);
    let location = AnimWorldCoord { x, y, z: 0 };
    assert_eq!(
        anims(&sim),
        vec![("EXPA".to_string(), location, 0, 0x600, 0)]
    );
    assert_eq!(owners(&sim), vec![None]);

    let (mut sim, _) = world();
    let victim = spawn(&mut sim, &rules, "E1", 12, 12);
    let killer = sim
        .spawn_object_at_height("E1", "Russians", 14, 12, 0, 0, &rules)
        .unwrap();
    kill_with(&mut sim, &rules, victim, "VIRUSWH", killer, None);
    assert_eq!(
        anims(&sim),
        vec![("EXPB".to_string(), location, 0, 0x600, 0)]
    );
    assert_eq!(owners(&sim), vec![Some("Russians".to_string())]);

    let (mut sim, russians) = world();
    let victim = spawn(&mut sim, &rules, "E1", 12, 12);
    kill_with(
        &mut sim,
        &rules,
        victim,
        "VIRUSWH",
        RAD_NO_ATTACKER,
        Some(russians),
    );
    assert_eq!(owners(&sim), vec![Some("Russians".to_string())]);
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

/// `0x00441D2F..0x00441D61`: a custom-palette type (`+0xDF0`) gives its
/// DestroyAnim GetRemapColour's scheme from that palette for its owner; the
/// same anim from an ordinary type keeps no remap.
#[test]
fn a_custom_palette_building_gives_its_destroy_anim_the_palette_scheme() {
    use crate::assets::asset_manager::{AssetManager, test_support::TestDirectory};
    use crate::sim::anim_class::AnimRemap;
    let mut rules = rules();
    let palette_root = TestDirectory::new("destroy-anim-palette");
    std::fs::write(palette_root.path().join("LIBTEM.PAL"), [0u8; 768]).unwrap();
    let assets = AssetManager::from_loose_root_for_test(palette_root.path());
    rules.bind_effect_assets(&assets, "tem", "TEMPERATE");
    let remaps = |kind: &str| {
        let mut sim = sim(5);
        let building = spawn(&mut sim, &rules, kind, 10, 20);
        kill(&mut sim, &rules, building);
        let owner = sim.interner.intern("Americans");
        let remaps: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| {
                (
                    sim.interner.resolve(anim.type_id()).to_string(),
                    anim.remap(),
                )
            })
            .collect();
        (remaps, owner)
    };
    let (statue, owner) = remaps("STATUE");
    assert_eq!(
        statue,
        [(
            "DESTA".to_string(),
            Some(AnimRemap::ExtraBuildingScheme(owner))
        )]
    );
    let (hall, _) = remaps("HALL");
    assert_eq!(hall, [("DESTA".to_string(), None)]);
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
                super::super::inviso_scatter::RandomDirectionSnap::Preserve,
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
        // Whole original Building442230 -> Techno701900 -> 4415F0 creates
        // each debris/footprint Anim before SpawnSurvivors442D90 and the
        // Infantry517A50 constructor. Full RNG consumption alone cannot
        // establish this native identity/Logic order.
        let death_anim_ids: Vec<_> = sim.substrate.anims.iter().map(|(id, _)| *id).collect();
        let logic = sim.logic_order();
        for survivor in sim.substrate.entities.values().filter(|entity| {
            !before.contains(&entity.stable_id())
                && entity.category() == crate::map::entities::EntityCategory::Infantry
        }) {
            let survivor_slot = logic
                .iter()
                .position(|id| *id == survivor.stable_id())
                .unwrap();
            assert!(
                death_anim_ids.iter().all(|id| {
                    *id < survivor.stable_id()
                        && logic.iter().position(|live| live == id).unwrap() < survivor_slot
                }),
                "seed {seed}: death Anim constructors precede survivor {}",
                survivor.stable_id()
            );
        }
        let crew: Vec<_> = sim
            .substrate
            .entities
            .keys_sorted()
            .into_iter()
            .filter(|id| !before.contains(id))
            .map(|id| sim.substrate.entities.get(id).unwrap().health.current())
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
    let controls: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_lifetime_controls.json",
    ))
    .unwrap();
    let repeat = &controls["repeat_fatal"];
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
                }
                cell
            })
            .collect();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(20, 20, cells));
        sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 20,
            off_fc: -128,
            off_100: -128,
            off_104: 256,
            off_108: 256,
        }));
        sim.set_playfield_size_height_for_test(Some(20));
        assert!(sim.rebuild_dynamic_navigation(&rules));
        let ship = spawn(&mut sim, &rules, kind, 12, 12);
        if !water {
            // Only the gate's `+0xEC` read changes; the ship stays placed.
            let cell = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut_for_test(12, 12);
            cell.unwrap().land_type = LandType::Clear.as_index();
        }
        let mut replay = sim.scenario_rng.clone();
        kill(&mut sim, &rules, ship);
        let played: Vec<_> = anims(&sim).into_iter().map(|anim| anim.0).collect();
        if sinks {
            assert!(played.is_empty(), "{kind}: {played:?}");
            // Original Unit ReceiveDamage737E33..737E5E restores the ship
            // after the fatal receiver and retains it for Unit AI's sinking
            // suffix. Native executions: naval_occupants, water-neighbor rows.
            let retained = sim.substrate.entities.get(ship).expect("retained hull");
            assert_eq!(
                retained.health.current(),
                1,
                "a sinking hull retains native Health 1 until its terminal AI visit"
            );
            assert!(retained.lifecycle.object_alive());
            assert!(!retained.lifecycle.in_limbo());
            assert!(retained.in_logic_vector.is_member());
            assert!(!sim.substrate.pending_delete.contains_object(&ship));
            assert!(
                !retained.lifecycle.cell_marked(),
                "native737F7A still unmarks the hull"
            );
            let owner = retained.owner();
            assert_eq!(sim.houses[&owner].stats().units_lost(), 1);
            // An area dispatch rejects the now-unmarked hull (489A80).
            // The native repeated-hit packet is a direct Unit737C90 call.
            let before_area = sim.rng_state();
            kill(&mut sim, &rules, ship);
            assert_eq!(sim.houses[&owner].stats().units_lost(), 1);
            assert_eq!(sim.rng_state(), before_area);
            assert_eq!(sim.scenario_rng.state(), replay.state());

            // Match the native control's complete ambient streams after
            // setup. The physical Super section above is parsed by the same
            // production reader; ignoreDefenses bypasses armor calculation.
            sim.main_rng = SimRng::new(1).into();
            sim.scenario_rng = SimRng::new(1);
            sim.mapgen_rng = SimRng::new(1);
            replay = sim.scenario_rng.clone();
            for (stream, rng) in [
                ("main", sim.main_rng.native_state_hex()),
                ("scenario", sim.scenario_rng.native_state_hex()),
                ("mapgen", sim.mapgen_rng.native_state_hex()),
            ] {
                assert_eq!(rng, repeat["rng_before"][stream].as_str().unwrap());
            }
            let input = &repeat["input"];
            let retained = sim.substrate.entities.get(ship).unwrap();
            assert_eq!(
                i64::from(retained.health.current()),
                input["initial_health"]
            );
            assert_eq!(
                u64::from(retained.sinking.is_active()),
                input["initial_sinking"]
            );
            assert!(input["attacker"].is_null());
            assert!(input["house"].is_null());
            let warhead = sim
                .interner
                .intern(repeat["calls"][0]["warhead"].as_str().unwrap());
            let direct_hit = EntityDamageEvent::direct_receiver(
                ship,
                i32::try_from(input["c4_damage"].as_i64().unwrap()).unwrap(),
                i32::try_from(input["distance"].as_i64().unwrap()).unwrap(),
                RAD_NO_ATTACKER,
                None,
                warhead,
                ReceiverCallFlags {
                    ignore_defenses: input["ignore_defenses"].as_bool().unwrap(),
                    arg6: input["arg6"].as_bool().unwrap(),
                },
            );
            sim.commit_direct_damage_receiver(
                &rules,
                None,
                direct_hit,
                crate::sim::world::FrameEffects::default(),
            );
            let retained = sim.substrate.entities.get(ship).unwrap();
            assert_eq!(i64::from(retained.health.current()), repeat["health"]);
            assert_eq!(
                u64::from(retained.lifecycle.object_alive()),
                repeat["alive"]
            );
            assert_eq!(u64::from(retained.sinking.is_active()), repeat["sinking"]);
            assert_eq!(
                u64::from(sim.houses[&owner].stats().units_lost()),
                repeat["losses"]
            );
            assert!(!retained.lifecycle.cell_marked());
            assert!(retained.in_logic_vector.is_member());
            for (stream, rng) in [
                ("main", sim.main_rng.native_state_hex()),
                ("scenario", sim.scenario_rng.native_state_hex()),
                ("mapgen", sim.mapgen_rng.native_state_hex()),
            ] {
                assert_eq!(rng, repeat["rng_after"][stream].as_str().unwrap());
            }
            // Terminal UnitAI records a null-source loss before shared
            // UnInit. Its later once-only recorder must not count it twice.
            sim.substrate
                .entities
                .get_mut(ship)
                .unwrap()
                .position
                .set_exact_z_leptons_for_test(Some(-400));
            assert!(sim.tick_ship_sinking(
                ship,
                &rules,
                None,
                crate::sim::world::FrameEffects::default()
            ));
            assert_eq!(sim.houses[&owner].stats().units_lost(), 3);
            assert!(sim.substrate.pending_delete.contains_object(&ship));
            assert!(
                !sim.substrate
                    .entities
                    .get(ship)
                    .unwrap()
                    .in_logic_vector
                    .is_member()
            );
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

/// Retail Dustbowl runtime against native execution: a power plant killed
/// through the production receiver, with retail rules and art bound, throws
/// its `DebrisAnims=` chunks, rolls for and places its centre mark and plays
/// one `Explosion=` anim per foundation cell as the original does from the
/// same Scenario state (`tools/spatial_oracle/building_death_anims.py`: the
/// ReceiveDamage debris block, then DestructionEffects steps 7 and 8 with
/// CanPlace over the map's own cells and Place's writes; the stream is
/// reseeded at the kill so the scenario's earlier draws cannot move the
/// comparison). At the fixture's plant the origin cells carry ore, which
/// CanPlace's overlay check (`0x006B6002`) rejects for every candidate, so
/// no mark lands; on the clean ground at (73, 116) the placer picks among the
/// 1x1, 2x1 and 1x2 types (no 2x2 fits: (74, 117) is not Morphable), and at
/// (81, 123) among the preferred 2x2 types; the mark's type, cells and
/// SmudgeData are compared. An art-less `gtpowexp` pick constructs an End=0
/// anim as native does. At the fixture's plant an MCV then
/// plays one of its own `Explosion=` anims and throws `MetallicDebris=`
/// chunks, all with `0x600`/0. Ignored: needs the retail install (`RA2_DIR`
/// or `config.toml`).
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_death_anims_use_the_types_lists() {
    let dir = std::env::var("RA2_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::test_fixture::game_config()
                .expect("set RA2_DIR or provide config.toml for this ignored test")
                .paths
                .ra2_dir
        });
    let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_death_anims.json",
    ))
    .unwrap();
    let rows = golden["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 30);
    let map_spec = &golden["map"];
    let tile_lookup = crate::sim::smudge_grid::oracle_fixture::tile_lookup(map_spec);
    let smudge_names: Vec<&str> = golden["smudge_types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|def| def["name"].as_str().unwrap())
        .collect();
    let ints = |value: &serde_json::Value| -> Vec<i64> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect()
    };
    for row in rows {
        let input = &row["input"];
        let mut scenario = crate::headless_scenario::load(&dir, "Dustbowl.mmx", 0x00C0_FFEE)
            .expect("Dustbowl loads");
        let crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources,
        } = &mut scenario.runtime;
        let rules = &resources.rules;
        let owner = sim.interner.intern("Americans");
        sim.houses
            .get_or_register_for_test(owner, || HouseState::new(owner, 0, None, false, 10_000, 10));
        let location_input = ints(&input["location"]);
        let (cell_x, cell_y) = (
            (location_input[0] / 256) as u16,
            (location_input[1] / 256) as u16,
        );
        let map_cell = |x: u16, y: u16| {
            map_spec["cells"].as_array().unwrap().iter().find(|cell| {
                cell["x"].as_u64() == Some(u64::from(x)) && cell["y"].as_u64() == Some(u64::from(y))
            })
        };
        let ore = map_cell(cell_x, cell_y).unwrap()["overlay"].is_i64();
        let (plant, mcv) = if !ore {
            let plant = sim
                .spawn_object("GAPOWR", "Americans", cell_x, cell_y, 0, rules)
                .expect("a power plant on the clean cells");
            (plant, None)
        } else {
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
                    let mcv = sim.spawn_object("AMCV", "Americans", x, y, 0, rules)?;
                    let plant = sim.spawn_object("GAPOWR", "Americans", x - 3, y, 0, rules)?;
                    Some((plant, mcv))
                })
                .expect("an MCV cell with room for a power plant");
            (plant, Some(mcv))
        };
        sim.resolve_type_handles(rules);

        // The native row's inputs are this plant's: its retail lists, its
        // Location, and the cells its CanPlace reads.
        let gapowr = rules.object("GAPOWR").unwrap();
        let names = |value: &serde_json::Value| -> Vec<String> {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(gapowr.explosion_anims, names(&input["explosion"]));
        assert_eq!(gapowr.debris_anims, names(&input["debris_anims"]));
        assert!(gapowr.debris_types.is_empty());
        assert_eq!(
            [gapowr.max_debris, gapowr.min_debris],
            [&input["max_debris"], &input["min_debris"]].map(|v| v.as_i64().unwrap() as i32)
        );
        assert_eq!(
            crate::rules::foundation::foundation_dimensions(&gapowr.foundation),
            (2, 2)
        );
        let flagged = |burn: bool, crater: bool, w: u8, h: u8| {
            (burn || crater).then_some((burn, crater, w, h))
        };
        assert_eq!(
            rules
                .smudge_types
                .iter_with_id()
                .filter_map(|(_, def)| {
                    flagged(def.burn, def.crater, def.width, def.height)
                        .map(|flags| (def.name.clone(), flags))
                })
                .collect::<Vec<_>>(),
            golden["smudge_types"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|def| {
                    let field = |key: &str| def[key].as_u64().unwrap();
                    flagged(
                        field("burn") == 1,
                        field("crater") == 1,
                        field("width") as u8,
                        field("height") as u8,
                    )
                    .map(|flags| (def["name"].as_str().unwrap().to_string(), flags))
                })
                .collect::<Vec<_>>()
        );
        let entity = sim.substrate.entities.get(plant).unwrap();
        let (rx, ry) = (entity.position.rx(), entity.position.ry());
        let location = position_world_coord(&entity.position);
        assert_eq!(
            vec![location.x, location.y, location.z]
                .into_iter()
                .map(i64::from)
                .collect::<Vec<_>>(),
            ints(&input["location"])
        );
        assert_eq!(
            sim.map_size_diamond()
                .map(|(w, h)| vec![i64::from(w), i64::from(h)]),
            Some(ints(&map_spec["size"]))
        );
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let overlay = sim.overlay_grid.as_ref().unwrap();
        let smudges = sim.smudge_grid.as_ref().unwrap();
        for (x, y) in (ry..ry + 2).flat_map(|y| (rx..rx + 2).map(move |x| (x, y))) {
            let spec = map_cell(x, y).expect("the native table holds every footprint cell");
            let field = |key: &str| spec[key].as_i64();
            let cell = terrain.cell(x, y).unwrap();
            assert_eq!(Some(i64::from(cell.final_tile_index)), field("tile"));
            assert_eq!(i64::from(cell.slope_type), field("slope").unwrap_or(0));
            assert_eq!(
                cell.accepts_smudge,
                crate::map::resolved_terrain::current_tile_permissions(
                    &tile_lookup,
                    cell.final_tile_index
                )
                .0
            );
            let slot = overlay.cell(x, y);
            assert_eq!(slot.overlay_id.map(i64::from), field("overlay"));
            if slot.overlay_id.is_some() {
                assert_eq!(Some(i64::from(slot.overlay_data)), field("overlay_data"));
            }
            assert!(smudges.cell(x, y).type_id.is_none() && field("smudge").is_none());
        }
        let marks_before = smudges.clone();
        sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap());
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_before"].as_str().unwrap()
        );

        let warhead = sim.interner.intern("Super");
        let kill = |sim: &mut Simulation, id: u64| {
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
            sim.commit_noncombat_aoe_hits(
                rules,
                Some(&resources.overlay_registry),
                &[hit],
                crate::sim::world::FrameEffects::default(),
            );
            // (type, coordinate, delay, flags, zAdjust, launch bits), in
            // construction order.
            sim.substrate
                .anims
                .iter()
                .filter(|(id, _)| !before.contains(id))
                .map(|(_, anim)| {
                    (
                        sim.interner.resolve(anim.type_id()).to_string(),
                        [
                            anim.world_coord().x,
                            anim.world_coord().y,
                            anim.world_coord().z,
                        ]
                        .map(i64::from)
                        .to_vec(),
                        anim.runtime().delay_remaining,
                        anim.draw_flags(),
                        anim.z_adjust(),
                        anim.bounce().as_ref().map(|body| {
                            (
                                body.position().map(|v| i64::from(v.bits())).to_vec(),
                                body.velocity().map(|v| i64::from(v.bits())).to_vec(),
                            )
                        }),
                    )
                })
                .collect::<Vec<_>>()
        };
        let seed = input["seed"].as_u64().unwrap();
        let (debris, explosions): (Vec<_>, Vec<_>) = kill(sim, plant)
            .into_iter()
            .partition(|anim| anim.5.is_some());

        let native_debris = row["debris"].as_array().unwrap();
        assert_eq!(debris.len(), native_debris.len(), "seed {seed:#x}");
        for (anim, native) in debris.iter().zip(native_debris) {
            let ctor = &native["ctor"];
            assert_eq!(anim.0, ctor["type"].as_str().unwrap(), "seed {seed:#x}");
            assert_eq!(anim.1, ints(&ctor["coord"]), "seed {seed:#x}");
            assert_eq!(
                (anim.2, anim.3, anim.4),
                (
                    ctor["delay"].as_u64().unwrap() as u16,
                    ctor["flags"].as_u64().unwrap() as u32,
                    0
                ),
                "seed {seed:#x}"
            );
            let (position, velocity) = anim.5.clone().unwrap();
            assert_eq!(
                position,
                ints(&native["bounce"]["position_bits"]),
                "seed {seed:#x}"
            );
            assert_eq!(
                velocity,
                ints(&native["bounce"]["velocity_bits"]),
                "seed {seed:#x}"
            );
        }

        // The mark's footprint from the Location cell, the plant's origin:
        // Place's (cell, type, SmudgeData) writes in its y-outer order.
        let events = row["events"].as_array().unwrap();
        let native_marks: Vec<_> = row["marked"]
            .as_array()
            .unwrap()
            .iter()
            .map(|mark| {
                assert_eq!(mark["dummy"], false);
                let cell = ints(&mark["cell"]);
                (
                    (cell[1] as u16, cell[0] as u16),
                    smudge_names[mark["type"].as_u64().unwrap() as usize].to_string(),
                    mark["data"].as_u64().unwrap() as u8,
                )
            })
            .collect();
        let mut marks: Vec<_> = sim
            .smudge_grid
            .as_ref()
            .unwrap()
            .iter_occupied()
            .filter(|&(x, y, cell)| marks_before.cell(x, y) != cell)
            .map(|(x, y, cell)| {
                let def = rules.smudge_types.get(cell.type_id.unwrap()).unwrap();
                ((y, x), def.name.clone(), cell.frame_offset)
            })
            .collect();
        marks.sort();
        assert_eq!(marks, native_marks, "seed {seed:#x}");
        assert_eq!(native_marks.is_empty(), ore);

        let native_explosions: Vec<_> = events
            .iter()
            .filter(|event| event["call"] == "anim_ctor")
            .skip(native_debris.len())
            .map(|event| {
                (
                    event["type"].as_str().unwrap().to_string(),
                    ints(&event["coord"]),
                    event["delay"].as_u64().unwrap() as u16,
                    event["flags"].as_u64().unwrap() as u32,
                )
            })
            .collect();
        assert_eq!(
            explosions
                .iter()
                .map(|anim| (anim.0.clone(), anim.1.clone(), anim.2, anim.3))
                .collect::<Vec<_>>(),
            native_explosions,
            "seed {seed:#x}"
        );
        assert!(explosions.iter().all(|anim| anim.4 == 0));

        let Some(mcv) = mcv else {
            continue;
        };
        // ReadGeneral's 128-byte buffer leaves a literal final type D in
        // retail MetallicDebris. It is unread/non-Bouncer but still debris;
        // the selected type list, not presence of a Bounce body, classifies it.
        let (mcv_debris, mcv_explosions): (Vec<_>, Vec<_>) =
            kill(sim, mcv).into_iter().partition(|anim| {
                rules
                    .general
                    .metallic_debris
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&anim.0))
            });
        let amcv = rules.object("AMCV").unwrap();
        assert!(
            matches!(mcv_explosions.as_slice(), [(name, _, 0, 0x600, 0, None)]
                if amcv.explosion_anims.iter().any(|entry| entry.eq_ignore_ascii_case(name))),
            "{mcv_explosions:?}"
        );
        assert!(mcv_debris.len() < amcv.max_debris as usize);
        assert!(mcv_debris.iter().all(|anim| {
            rules
                .general
                .metallic_debris
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&anim.0))
                && (anim.2, anim.3, anim.4) == (0, 0x600, 0)
        }));
    }
}

/// A retail skirmish arena with `kind` for the Russians at (24, 24).
fn retail_building(
    kind: &str,
    seed: u64,
) -> Option<(Simulation, RuleSet, crate::sim::pathfinding::PathGrid, u64)> {
    let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
    // The damage fires an Update lights at Health 0 (`0x0043FC39`) and the
    // death weapon's anims need their art.
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let mut roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    roots.extend(
        rules
            .general
            .damage_fire_types
            .iter()
            .map(|anim| anim.name.clone()),
    );
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, seed, 48);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .unwrap();
    let id = sim
        .spawn_object(kind, "Russians", 24, 24, 0, &rules)
        .unwrap_or_else(|| panic!("{kind} spawns"));
    Some((sim, rules, grid, id))
}

/// A lethal direct hit with the retail `Super` warhead and no attacker.
fn kill_retail(sim: &mut Simulation, rules: &RuleSet, id: u64, ignore_defenses: bool) {
    let warhead = sim.interner.intern("Super");
    assert!(rules.warhead("Super").is_some());
    let hit = EntityDamageEvent::direct_receiver(
        id,
        100_000,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(
        rules,
        None,
        &[hit],
        crate::sim::world::FrameEffects::default(),
    );
}

fn infantry_ids(sim: &Simulation) -> Vec<u64> {
    sim.substrate
        .entities
        .keys_sorted()
        .into_iter()
        .filter(|&id| {
            sim.substrate.entities.get(id).is_some_and(|entity| {
                entity.category() == crate::map::entities::EntityCategory::Infantry
            })
        })
        .collect()
}

/// Alive at Health 0, not dying and still in the store.
fn outlives(sim: &Simulation, id: u64) -> bool {
    sim.substrate.entities.get(id).is_some_and(|building| {
        building.lifecycle.object_alive()
            && building.health.current() == 0
            && !building.death.is_dying()
    })
}

fn retired(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .is_none_or(|building| building.death.is_dying())
}

fn tick(sim: &mut Simulation, rules: &RuleSet, grid: &crate::sim::pathfinding::PathGrid) {
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
}

/// DestructionEffects arms a zero death fuse for an `Explodes=` type
/// (`0x00441C43..0x00441C8C`), so ReceiveDamage leaves the retail Nuclear
/// Reactor alive at Health 0 (`0x0044266B..0x00442690`). Its next Update
/// limbos it and runs SpawnSurvivors a second time before its UnInit
/// (`0x004400C5..0x004400E5`): more crew leave a frame after the blast.
#[test]
fn a_nuclear_reactor_outlives_its_killing_blow_and_sends_a_second_crew_round() {
    let mut second_round = 0;
    for seed in 1..=4 {
        let Some((mut sim, rules, grid, reactor)) = retail_building("NANRCT", seed) else {
            return;
        };
        let stage = sim
            .substrate
            .entities
            .get(reactor)
            .unwrap()
            .native_stage()
            .value();
        kill_retail(&mut sim, &rules, reactor, false);
        assert!(outlives(&sim, reactor), "seed {seed}: alive at Health 0");
        // `0x00441C8F..0x00441CA9`: its StageClass clock stops, Value kept.
        let clock = sim.substrate.entities.get(reactor).unwrap().native_stage();
        assert_eq!((clock.value(), clock.rate()), (stage, 0));
        assert!(
            !sim.substrate
                .entities
                .get(reactor)
                .unwrap()
                .no_survivor
                .is_set()
        );
        let before = infantry_ids(&sim);
        tick(&mut sim, &rules, &grid);
        assert!(retired(&sim, reactor), "seed {seed}: its Update retires it");
        second_round += infantry_ids(&sim)
            .iter()
            .filter(|id| !before.contains(id))
            .count();
    }
    assert!(second_round > 0, "the seeds exercise a second crew round");
}

/// The killing call's IgnoreDefenses becomes NoSurvivor (`0x00441F0B`) and
/// holds for the second round as well (How_Many_Survivors `0x00451334`).
#[test]
fn an_ignore_defenses_kill_leaves_a_reactor_no_survivor_in_either_round() {
    for seed in 1..=4 {
        let Some((mut sim, rules, grid, reactor)) = retail_building("NANRCT", seed) else {
            return;
        };
        let before = infantry_ids(&sim);
        kill_retail(&mut sim, &rules, reactor, true);
        assert!(outlives(&sim, reactor));
        assert!(
            sim.substrate
                .entities
                .get(reactor)
                .unwrap()
                .no_survivor
                .is_set()
        );
        tick(&mut sim, &rules, &grid);
        assert!(retired(&sim, reactor));
        assert_eq!(infantry_ids(&sim), before, "seed {seed}: no crew");
    }
}

/// Any other building gets an 8-frame fuse and ReceiveDamage UnInits it at
/// once (`0x0044269A`).
#[test]
fn a_power_plant_dies_with_its_killing_blow() {
    let Some((mut sim, rules, _, plant)) = retail_building("NAPOWR", 1) else {
        return;
    };
    kill_retail(&mut sim, &rules, plant, false);
    assert!(retired(&sim, plant));
}

/// A building whose Get_Mission is Selling gets the zero fuse too
/// (`0x00441C43`), whatever its type.
#[test]
fn a_power_plant_killed_while_selling_outlives_the_blow_until_its_update() {
    let Some((mut sim, rules, grid, plant)) = retail_building("NAPOWR", 1) else {
        return;
    };
    let now = sim.session.binary_frame();
    sim.substrate
        .mission_assign_exact(
            plant,
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Selling),
            now,
        )
        .unwrap();
    kill_retail(&mut sim, &rules, plant, false);
    assert!(outlives(&sim, plant));
    tick(&mut sim, &rules, &grid);
    assert!(retired(&sim, plant));
}

/// A retained request must not give DestroyAnim a house remap when the
/// native group loader would return null (441D25 /6276AA).
#[test]
fn palette_binding_missing_file_keeps_destroy_anim_unremapped() {
    use crate::assets::asset_manager::{AssetManager, test_support::TestDirectory};
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let art = IniFile::from_str(ART);
    let mut layers = RulesLayerStack::new(IniFile::from_str(RULES));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[STATUE]\nImage=EMPTY\n"),
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("DESTA", 10);
    rules.install_art_data(registry);
    let root = TestDirectory::new("destroy-anim-missing-palette");
    let assets = AssetManager::from_loose_root_for_test(root.path());
    rules.bind_effect_assets(&assets, "tem", "TEMPERATE");
    assert_eq!(
        rules.object("STATUE").unwrap().custom_palette_name(),
        Some("lib")
    );
    let mut sim = sim(5);
    let building = spawn(&mut sim, &rules, "STATUE", 10, 20);
    kill(&mut sim, &rules, building);
    let remaps: Vec<_> = sim
        .substrate
        .anims
        .iter()
        .map(|(_, anim)| (sim.interner.resolve(anim.type_id()), anim.remap()))
        .collect();
    assert_eq!(remaps, [("DESTA", None)]);
}

/// Legal original one-successful-group case: A's failed request is retained
/// through an empty later ART read, while B's valid group must still bind.
/// A must neither shadow B's body/MAKE palette nor inherit B's DestroyAnim
/// scheme (717820 null-store,626F47..626F62 loaded-group count,441D25).
#[test]
fn palette_binding_failed_request_does_not_shadow_or_inherit_loaded_group() {
    use crate::assets::asset_manager::{AssetManager, test_support::TestDirectory};
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::sim::anim_class::AnimRemap;
    let root_rules = RULES.replace(
        "[HALL]\nStrength=100\nDestroyAnim=DESTA",
        "[HALL]\nStrength=100\nDestroyAnim=DESTA\nImage=MISS",
    );
    let art = IniFile::from_str(&format!("{ART}\n[MISS]\nPalette=missing\n[EMPTY]\n"));
    let mut layers = RulesLayerStack::new(IniFile::from_str(&root_rules));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[HALL]\nImage=EMPTY\n"),
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("DESTA", 10);
    rules.install_art_data(registry);
    let root = TestDirectory::new("retained-missing-plus-valid-palette");
    std::fs::write(root.path().join("LIBTEM.PAL"), [0u8; 768]).unwrap();
    let assets = AssetManager::from_loose_root_for_test(root.path());
    assert_eq!(rules.object("HALL").unwrap().image, "EMPTY");
    assert_eq!(
        rules.object("HALL").unwrap().custom_palette_name(),
        Some("missing")
    );
    assert_eq!(
        rules.object("STATUE").unwrap().custom_palette_name(),
        Some("lib")
    );
    // The public body/MAKE resolver skips A's missing file and names B's
    // actual group, instead of a first-request marker shared by every type.
    let (name, _palette) = rules.load_extra_building_palette(&assets, "tem").unwrap();
    assert_eq!(name, "lib");
    rules.bind_effect_assets(&assets, "tem", "TEMPERATE");
    for (kind, loaded) in [("HALL", false), ("STATUE", true)] {
        assert_eq!(
            rules.has_loaded_custom_palette(rules.object(kind).unwrap()),
            loaded
        );
        let mut sim = sim(5);
        let owner = sim.interner.intern("Americans");
        let building = spawn(&mut sim, &rules, kind, 10, 20);
        kill(&mut sim, &rules, building);
        let remaps: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| (sim.interner.resolve(anim.type_id()), anim.remap()))
            .collect();
        assert_eq!(
            remaps,
            [(
                "DESTA",
                loaded.then_some(AnimRemap::ExtraBuildingScheme(owner))
            )],
            "{kind}"
        );
    }
}
