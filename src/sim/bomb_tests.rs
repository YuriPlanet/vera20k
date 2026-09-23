//! Tests for the bomb owner. `native_bomb_corpus` compares against
//! `tools/spatial_oracle/bomb_class.json`, produced by running the original
//! BombClass/BombListClass code under Unicorn; the rest are Rust regression
//! tests of the production paths.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::util::fixed_math::SimFixed;

const RULES: &str = "\
[AudioVisual]
BombAttachSound=CrazyIvanAttack
BombTickingSound=CrazyIvanBombTick
[CombatDamage]
IvanWarhead=IvanWH
IvanDamage=450
IvanTimedDelay=450
C4Warhead=Super
[InfantryTypes]
0=IVAN
1=ENGINEER
2=SCOUT3
[VehicleTypes]
0=HTNK
1=LTNK
[AircraftTypes]
[BuildingTypes]
0=GAPOWR
1=CAGARR
2=CABHUT
[Warheads]
0=IvanBomb
1=IvanWH
2=BombDisarm
3=Super
[IVAN]
Strength=125
Speed=4
Sight=6
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
Primary=IvanBomber
Explodes=yes
Ivan=yes
[ENGINEER]
Strength=75
Speed=4
Sight=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
Primary=DefuseKit
Engineer=yes
BombSight=4
[SCOUT3]
Strength=50
Speed=4
Sight=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
BombSight=3
[HTNK]
Strength=1000
Speed=4
ROT=5
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[LTNK]
Strength=300
Speed=4
ROT=5
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[GAPOWR]
Strength=750
Foundation=2x2
Power=100
[CAGARR]
Strength=750
Foundation=2x2
CanBeOccupied=yes
[CABHUT]
Strength=1000
Foundation=1x1
BridgeRepairHut=yes
[IvanBomber]
Damage=400
ROF=50
Range=1.5
Projectile=Invisible
Warhead=IvanBomb
[DefuseKit]
Damage=1
ROF=20
Range=1.5
Projectile=InvisibleAll
Warhead=BombDisarm
[Invisible]
Inviso=yes
[InvisibleAll]
Inviso=yes
[IvanBomb]
IvanBomb=yes
[BombDisarm]
BombDisarm=yes
[IvanWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
CellSpread=1.5
PercentAtMax=.25
[Super]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

fn rules() -> RuleSet {
    rules_from(RULES)
}

fn rules_from(text: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(text)).expect("bomb rules")
}

/// Americans (human) and Russians (computer), in that house order.
fn sim(seed: u64) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    for (name, side, human) in [("Americans", 0, true), ("Russians", 1, false)] {
        let id = sim.interner.intern(name);
        sim.houses
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
    }
    sim
}

fn arena(seed: u64, rules: &RuleSet) -> (Simulation, crate::sim::pathfinding::PathGrid) {
    let mut sim = sim(seed);
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    (sim, grid)
}

/// Spawn one object; a vehicle needs a cell without other objects.
fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn entity(sim: &Simulation, id: u64) -> &GameEntity {
    sim.substrate.entities.get(id).expect("entity")
}

/// Stand `id` at the ground Location `(x, y)` in leptons, inside the cell it
/// was spawned in.
fn place(sim: &mut Simulation, id: u64, (x, y): (i32, i32)) {
    let position = &mut sim.substrate.entities.get_mut(id).unwrap().position;
    assert_eq!(
        (i32::from(position.rx), i32::from(position.ry)),
        (x / 256, y / 256)
    );
    position.sub_x = SimFixed::from_num(x % 256);
    position.sub_y = SimFixed::from_num(y % 256);
}

/// Kill `id` with one sourceless hit.
fn kill(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let strength = i32::from(entity(sim, id).health.current);
    let super_wh = sim.interner.intern("Super");
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        id,
        strength,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        super_wh,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.commit_direct_damage_receiver(rules, None, hit);
    assert_eq!(entity(sim, id).health.current, 0);
}

fn bomb(sim: &Simulation, id: u64) -> Option<Bomb> {
    sim.substrate
        .entities
        .get(id)
        .and_then(|entity| entity.bomb)
}

fn attack(sim: &mut Simulation, attacker: u64, target: u64) {
    let owner = entity(sim, attacker).owner();
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::Attack {
            attacker_id: attacker,
            target_id: target,
        },
    ));
}

/// One frame; returns the native frame it ran as.
fn step(sim: &mut Simulation, rules: &RuleSet, grid: &crate::sim::pathfinding::PathGrid) -> i32 {
    let frame = sim.session.binary_frame as i32;
    let commands = sim.take_due_commands();
    sim.advance_tick(
        &commands,
        Some(rules),
        &std::collections::BTreeMap::new(),
        Some(grid),
        None,
        33,
    );
    frame
}

/// Run until `done` holds after a frame; the frame it held after.
fn run_until(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &crate::sim::pathfinding::PathGrid,
    frames: usize,
    mut done: impl FnMut(&Simulation) -> bool,
) -> Option<i32> {
    for _ in 0..frames {
        let frame = step(sim, rules, grid);
        if done(sim) {
            return Some(frame);
        }
    }
    None
}

#[derive(serde::Deserialize)]
struct NativeBombCase {
    input: serde_json::Value,
    #[serde(default)]
    bombs: Vec<serde_json::Value>,
    #[serde(default)]
    bomb: Option<serde_json::Value>,
    #[serde(default)]
    events: Vec<serde_json::Value>,
    #[serde(default)]
    technos: Vec<serde_json::Value>,
    #[serde(default)]
    expired: Option<u64>,
    #[serde(default)]
    frame_index: Option<i64>,
    #[serde(default)]
    list: Option<serde_json::Value>,
}

fn native_bomb(fields: &serde_json::Value) -> Bomb {
    let int = |key: &str| fields[key].as_i64().unwrap_or(0) as i32;
    Bomb {
        planter: fields["planter"].as_u64(),
        planter_house: InternedId::default(),
        start_frame: int("start"),
        end_frame: int("end"),
        seen_by: 0,
    }
}

/// The original BombClass bodies, case by case, against VERA's: Attach's
/// gates and stores (end = start + IvanTimedDelay as a plain 32-bit add, the
/// planter's house, the attach sound for that house's player only), the
/// fuse boundary (IsTimerExpired: signed `Frame > end`), the clock frame,
/// Detonate's silent limbo branch and its source, Defuse, and the AI_Update
/// fuse check. Death bombs (`+0x30 = 1`) and spent or carrier-less records
/// have no VERA state and are skipped. The DetonateAtCoord arm rows reduce to
/// Attach's and Defuse's own gates (a non-Techno or missing target or firer
/// reaches Attach as null) and are exercised end to end by the production
/// tests below; the GetFireError rows are compared in `combat_weapon`.
#[test]
fn native_bomb_corpus() {
    let cases: Vec<NativeBombCase> =
        serde_json::from_str(include_str!("../../tools/spatial_oracle/bomb_class.json")).unwrap();
    assert_eq!(cases.len(), 97);
    let mut compared = 0;
    for case in cases {
        let input = &case.input;
        let name = input["name"].as_str().unwrap();
        let section = input["section"].as_str().unwrap();
        let native_rules = &input["rules"];
        let delay = native_rules["delay"].as_i64().unwrap_or(450) as i32;
        let flicker = native_rules["flicker"].as_i64().unwrap_or(8) as i32;
        let preset: Vec<&serde_json::Value> = input["bombs"]
            .as_array()
            .map(|bombs| bombs.iter().collect())
            .unwrap_or_default();
        let skipped = preset.iter().any(|fields| {
            fields["kind"].as_i64().unwrap_or(0) != 0
                || fields["spent"].as_i64().unwrap_or(0) != 0
                || fields["carrier"].is_null()
        });
        match section {
            "timer" if !skipped => {
                let frame = input["frame"].as_i64().unwrap() as i32;
                let expected = case.expired == Some(1);
                assert_eq!(native_bomb(preset[0]).expired(frame), expected, "{name}");
            }
            "clock" if !skipped => {
                let frame = input["frame"].as_i64().unwrap() as i32;
                assert_eq!(
                    native_bomb(preset[0]).clock_frame(frame, delay, flicker),
                    case.frame_index.map(|index| index as i32),
                    "{name}"
                );
            }
            "attach" | "detonate" | "defuse" | "expiry" if !skipped => {
                check_world_case(&case, name, section, delay);
            }
            _ => continue,
        }
        compared += 1;
    }
    // 55 state-machine rows less the six VERA cannot hold (death bombs,
    // spent or carrier-less records); the GetFireError rows are compared by
    // `combat_weapon::tests::bomb_fire_error_gates_match_native`.
    assert_eq!(compared, 49);
}

/// A corpus case's world: house `n` is `house{n}`, each techno stands at its
/// native Location, and a bomb is held only by the carrier whose `+0x38`
/// points at it (VERA keeps the record on its carrier; a record nothing points
/// at is no bomb). A carrier drawn as `visible` starts seen by the local house.
struct NativeWorld {
    sim: Simulation,
    ids: Vec<u64>,
    houses: Vec<InternedId>,
}

fn native_world(input: &serde_json::Value, rules: &RuleSet) -> NativeWorld {
    let mut sim = sim(9);
    let houses: Vec<InternedId> = (0..4)
        .map(|n| {
            let id = sim.interner.intern(&format!("house{n}"));
            sim.houses
                .insert(id, HouseState::new(id, 0, None, false, 0, 10));
            sim.session.house_order.push(id);
            id
        })
        .collect();
    sim.session.binary_frame = input["frame"].as_i64().unwrap_or(1000) as u32;
    let technos = input["technos"].as_array().unwrap();
    let ids: Vec<u64> = technos
        .iter()
        .enumerate()
        .map(|(n, techno)| {
            let kind = match techno["rtti"].as_u64().unwrap_or(0x0F) {
                0x0F => match techno["bomb_sight"].as_i64().unwrap_or(0) {
                    4 => "ENGINEER",
                    3 => "SCOUT3",
                    _ => "IVAN",
                },
                0x06 if techno["bridge_hut"].as_bool() == Some(true) => "CABHUT",
                0x06 => "GAPOWR",
                _ => "HTNK",
            };
            let owner = format!("house{}", techno["house"].as_u64().unwrap_or(0));
            let id = spawn(&mut sim, rules, kind, &owner, 10 + 4 * n as u16, 10);
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            if let Some(location) = techno["location"].as_array() {
                let axis = |i: usize| location[i].as_i64().unwrap() as i32;
                entity.position.rx = (axis(0) / 256) as u16;
                entity.position.sub_x = SimFixed::from_num(axis(0) % 256);
                entity.position.ry = (axis(1) / 256) as u16;
                entity.position.sub_y = SimFixed::from_num(axis(1) % 256);
                entity.position.exact_z_leptons = Some(axis(2));
            }
            // Native's detector list holds exactly the objects on the map with
            // a BombSight (Unlimbo adds, Limbo removes), so an unlisted one is
            // one in limbo.
            let unlisted_detector = techno["bomb_sight"].as_i64().unwrap_or(0) != 0
                && !input["detectors"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|listed| listed.as_u64() == Some(n as u64));
            entity.lifecycle.in_limbo =
                techno["in_limbo"].as_bool() == Some(true) || unlisted_detector;
            id
        })
        .collect();
    let local = houses[input["local_house"].as_u64().unwrap_or(0) as usize];
    for (n, fields) in input["bombs"].as_array().into_iter().flatten().enumerate() {
        let slot = fields["carrier"].as_u64().unwrap() as usize;
        if technos[slot]["bomb"].as_u64() != Some(n as u64) {
            continue;
        }
        let mut bomb = native_bomb(fields);
        bomb.planter = fields["planter"].as_u64().map(|n| ids[n as usize]);
        bomb.planter_house = houses[fields["house"].as_u64().unwrap_or(0) as usize];
        if technos[slot]["visible"].as_u64() == Some(1) {
            bomb.seen_by = house_bit(&sim.session.house_order, local);
        }
        sim.substrate.entities.get_mut(ids[slot]).unwrap().bomb = Some(bomb);
        sim.bombs.carriers.insert(ids[slot]);
    }
    sim.sound_events.clear();
    NativeWorld { sim, ids, houses }
}

/// Build the case's technos and bombs in a world and run VERA's side.
fn check_world_case(case: &NativeBombCase, name: &str, section: &str, delay: i32) {
    let input = &case.input;
    let mut text = RULES.replace("IvanTimedDelay=450", &format!("IvanTimedDelay={delay}"));
    if input["rules"]["attach_sound"].as_i64() == Some(-1) {
        text = text.replace("BombAttachSound=CrazyIvanAttack\n", "");
    }
    let rules = rules_from(&text);
    let NativeWorld {
        mut sim,
        ids,
        houses: _,
    } = native_world(input, &rules);
    let index = |value: &serde_json::Value| value.as_u64().map(|n| ids[n as usize]);
    match section {
        "attach" => {
            let planter = index(&input["planter"]).unwrap_or(u64::MAX);
            let target = index(&input["target"]);
            let carrier_before = target.and_then(|id| bomb(&sim, id));
            sim.bomb_attach(planter, target, &rules);
            let native = case.bombs.first();
            match native {
                Some(native) => {
                    let carrier = index(&input["target"]).unwrap();
                    let planted = bomb(&sim, carrier).expect("planted");
                    assert_eq!(planted.planter, Some(planter), "{name}");
                    let house = native["house"].as_str().unwrap();
                    assert_eq!(sim.interner.resolve(planted.planter_house), house, "{name}");
                    assert_eq!(
                        (planted.start_frame, planted.end_frame),
                        (
                            native["start"].as_i64().unwrap() as i32,
                            native["end"].as_i64().unwrap() as i32
                        ),
                        "{name}: the fuse"
                    );
                    assert!(sim.bombs.carriers.contains(&carrier), "{name}");
                    let countdown = case.list.as_ref().unwrap()["countdown"].as_i64();
                    assert_eq!(
                        Some(i64::from(sim.bombs.visibility_countdown)),
                        countdown,
                        "{name}: 0x00438FCA"
                    );
                    // The sound is the planter's player's only: VERA names
                    // the planter's house; the app matches the local player.
                    let local = format!("house{}", input["local_house"].as_u64().unwrap_or(0));
                    let heard = sim.sound_events.iter().any(|event| {
                        matches!(event, SimSoundEvent::VocAt { audible_to: Some(houses), .. }
                            if sim.interner.resolve(houses[0]) == local)
                    });
                    let native_heard = case.events.iter().any(|event| event[0] == "play_at");
                    assert_eq!(heard, native_heard, "{name}: the attach sound");
                }
                None => {
                    assert_eq!(
                        target.and_then(|id| bomb(&sim, id)),
                        carrier_before,
                        "{name}: nothing planted"
                    );
                    assert!(sim.sound_events.is_empty(), "{name}");
                    let countdown = case.list.as_ref().unwrap()["countdown"].as_i64();
                    assert_eq!(
                        Some(i64::from(sim.bombs.visibility_countdown)),
                        countdown,
                        "{name}: the countdown is left alone"
                    );
                }
            }
        }
        "detonate" => {
            let carrier = ids[1];
            let native_blast = case.events.iter().any(|event| event[0] == "area_damage");
            let blast = sim.take_bomb_blast(carrier, &rules);
            assert!(
                bomb(&sim, carrier).is_none(),
                "{name}: the record goes first"
            );
            assert_eq!(blast.is_some(), native_blast, "{name}: silent in limbo");
            if let Some(blast) = blast {
                let event = case
                    .events
                    .iter()
                    .find(|event| event[0] == "area_damage")
                    .unwrap();
                let native_source = event[3]
                    .as_str()
                    .map(|name| ids[name["techno".len()..].parse::<usize>().unwrap()]);
                assert_eq!(
                    blast.source,
                    native_source.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
                    "{name}: the source"
                );
                // Native always ends a hut's 5x5 scan in 0x00574C20 or
                // 0x00574000; which one is the shared hut dispatcher's choice
                // (bridge_orchestrator), not the bomb's.
                let hut = case
                    .events
                    .iter()
                    .any(|event| event[0] == "bridge_low" || event[0] == "bridge_high");
                assert_eq!(blast.bridge_hut, hut, "{name}: the hut");
            }
        }
        "defuse" => {
            let carrier = ids[1];
            sim.bomb_defuse(carrier);
            assert!(bomb(&sim, carrier).is_none(), "{name}");
            assert!(!sim.bombs.carriers.contains(&carrier), "{name}");
        }
        "expiry" => {
            let carrier = ids[0];
            let had = bomb(&sim, carrier).is_some();
            sim.bomb_fuse_step(carrier, &rules, None);
            let detonated = had && bomb(&sim, carrier).is_none();
            let native = case.events.iter().any(|event| event[0] == "detonate");
            assert_eq!(detonated, native, "{name}");
        }
        _ => unreachable!(),
    }
    let _ = &case.technos;
    let _ = &case.bomb;
}

/// `BombListClass::UpdateAll` against the corpus's `update_all` rows: the
/// countdown after the call, each carrier's BombVisible for the local player
/// (the planter's house, or a listed detector of that player inside
/// `BombSight << 8`, through Sqrt_Approx), and where the ticking loop plays
/// (at the Location while out of limbo, stopped in limbo). Two row families
/// have no VERA counterpart: the purge (VERA drops a record the moment it goes
/// off or is defused) and the campaign rows (game mode 0: the recorded
/// campaign-visibility residual).
#[test]
fn native_update_all_corpus() {
    let cases: Vec<NativeBombCase> =
        serde_json::from_str(include_str!("../../tools/spatial_oracle/bomb_class.json")).unwrap();
    let rules = rules();
    let mut compared = 0;
    for case in cases
        .iter()
        .filter(|case| case.input["section"] == "update_all")
    {
        let input = &case.input;
        let name = input["name"].as_str().unwrap();
        if input["game_mode"].as_i64() == Some(0) || name.starts_with("update_purges") {
            continue;
        }
        let NativeWorld {
            mut sim,
            ids,
            houses,
        } = native_world(input, &rules);
        sim.bombs.visibility_countdown = input["countdown"].as_i64().unwrap() as i32;
        sim.bomb_list_update(&rules);
        let list = case.list.as_ref().unwrap();
        assert_eq!(
            i64::from(sim.bombs.visibility_countdown),
            list["countdown"].as_i64().unwrap(),
            "{name}: countdown"
        );
        let local = houses[input["local_house"].as_u64().unwrap_or(0) as usize];
        for (slot, native) in case.technos.iter().enumerate() {
            if bomb(&sim, ids[slot]).is_some() {
                assert_eq!(
                    sim.bomb_seen_by(ids[slot], local),
                    native["visible"] == 1,
                    "{name}: techno{slot} BombVisible"
                );
            }
        }
        let technos = input["technos"].as_array().unwrap();
        let carrier_of = |bomb_name: &serde_json::Value| {
            let n: u64 = bomb_name.as_str().unwrap()["bomb".len()..].parse().unwrap();
            let slot = technos
                .iter()
                .position(|techno| techno["bomb"].as_u64() == Some(n))
                .unwrap();
            ids[slot]
        };
        for event in &case.events {
            match event[0].as_str().unwrap() {
                "play_at" | "update_loop" => {
                    let (coords, owner) = if event[0] == "play_at" {
                        (&event[2], &event[3])
                    } else {
                        (&event[1], &event[2])
                    };
                    let at = sim
                        .bomb_ticking_coord(carrier_of(owner))
                        .unwrap_or_else(|| panic!("{name}: the loop plays"));
                    let native: Vec<i32> = coords
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_i64().unwrap() as i32)
                        .collect();
                    assert_eq!(vec![at.x, at.y, at.z], native, "{name}: at the Location");
                }
                "voc_release" => {
                    assert!(
                        sim.bomb_ticking_coord(carrier_of(&event[1])).is_none(),
                        "{name}"
                    );
                }
                other => panic!("{name}: unexpected {other}"),
            }
        }
        // A bomb attached without BombTickingSound= keeps silent: VERA reads the
        // rules key where native copied it (`Rules+0x20C` at 0x00438EFD).
        if input["bombs"][0]["ticking_sound"].as_i64() == Some(-1) {
            assert!(case.events.is_empty(), "{name}");
            let silent = rules_from(&RULES.replace("BombTickingSound=CrazyIvanBombTick\n", ""));
            assert_eq!(silent.general.bomb_ticking_sound, None, "{name}");
        }
        compared += 1;
    }
    // 27 rows less three campaign rows and two purge rows.
    assert_eq!(compared, 22);
}

/// A Crazy Ivan's shot plants a bomb and deals nothing; the bomb goes off in
/// the tank's own AI turn IvanTimedDelay + 1 frames after the attach, for
/// IvanDamage with IvanWarhead.
#[test]
fn a_crazy_ivan_bombs_a_tank() {
    let rules = rules();
    let (mut sim, grid) = arena(21, &rules);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 11, 10);
    attack(&mut sim, ivan, tank);
    run_until(&mut sim, &rules, &grid, 200, |sim| {
        bomb(sim, tank).is_some()
    })
    .expect("planted");
    let planted = bomb(&sim, tank).unwrap();
    assert_eq!(planted.planter, Some(ivan));
    assert_eq!(sim.interner.resolve(planted.planter_house), "Russians");
    assert_eq!(
        entity(&sim, tank).health.current,
        1000,
        "the shot deals nothing"
    );

    let went_off = run_until(&mut sim, &rules, &grid, 600, |sim| {
        bomb(sim, tank).is_none()
    })
    .expect("the bomb goes off");
    assert_eq!(went_off, planted.start_frame + 451, "IvanTimedDelay + 1");
    assert_eq!(entity(&sim, tank).health.current, 1000 - 450);
    assert!(
        sim.substrate
            .entities
            .values()
            .all(|entity| entity.bomb.is_none()),
        "an Ivan cannot bomb a bombed target, and fires no second bomb"
    );
}

/// Frames in limbo do not extend the fuse, and a carrier in limbo does not
/// set it off: it goes off on its first AI visit back on the map.
#[test]
fn the_fuse_waits_while_the_carrier_is_in_limbo() {
    let rules = rules();
    let mut sim = sim(22);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 11, 10);
    sim.session.binary_frame = 100;
    sim.bomb_attach(ivan, Some(tank), &rules);
    sim.session.binary_frame = 100 + 451;
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .lifecycle
        .in_limbo = true;
    sim.bomb_fuse_step(tank, &rules, None);
    assert!(bomb(&sim, tank).is_some(), "waits in limbo");
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .lifecycle
        .in_limbo = false;
    sim.bomb_fuse_step(tank, &rules, None);
    assert!(bomb(&sim, tank).is_none());
    assert_eq!(entity(&sim, tank).health.current, 1000 - 450);
}

/// A bomb outlives its planter (PointerGotInvalid only nulls it) and then
/// credits no one.
#[test]
fn a_bomb_outlives_its_planter_and_credits_no_one() {
    let rules = rules();
    let mut sim = sim(23);
    let tank = spawn(&mut sim, &rules, "LTNK", "Americans", 12, 10);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 11, 10);
    sim.session.binary_frame = 100;
    sim.bomb_attach(ivan, Some(tank), &rules);
    sim.uninit_with_rules(ivan, &rules);
    sim.flush_pending_delete();
    assert_eq!(bomb(&sim, tank).unwrap().planter, None);

    sim.session.binary_frame = 100 + 451;
    sim.bomb_fuse_step(tank, &rules, None);
    let tank = entity(&sim, tank);
    assert_eq!(tank.health.current, 0, "450 kills the 300-strength tank");
    assert!(tank.killed_by.is_none(), "no kill credit");
}

/// UnInit defuses silently (`0x005F65F3`); a building changing hands loses
/// its bomb unless it is `CanBeOccupied=` (`0x00448277`); mind control keeps
/// a unit's.
#[test]
fn removal_and_capture_defuse_silently() {
    let rules = rules();
    let mut sim = sim(24);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 30, 10);
    let sold = spawn(&mut sim, &rules, "GAPOWR", "Americans", 10, 10);
    let captured = spawn(&mut sim, &rules, "GAPOWR", "Americans", 14, 10);
    let garrison = spawn(&mut sim, &rules, "CAGARR", "Americans", 18, 10);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 22, 10);
    for carrier in [sold, captured, garrison, tank] {
        sim.bomb_attach(ivan, Some(carrier), &rules);
    }
    let russians = sim.interner.intern("Russians");

    sim.uninit_with_rules(sold, &rules);
    assert!(bomb(&sim, sold).is_none());
    sim.change_owner_with_rules(captured, russians, &rules);
    assert!(
        bomb(&sim, captured).is_none(),
        "an engineer's capture defuses"
    );
    sim.change_owner_with_rules(garrison, russians, &rules);
    assert!(
        bomb(&sim, garrison).is_some(),
        "an occupied building keeps it"
    );
    sim.change_owner_with_rules(tank, russians, &rules);
    assert!(bomb(&sim, tank).is_some(), "a unit changing hands keeps it");
    assert_eq!(
        sim.bomb_carriers().iter().copied().collect::<Vec<_>>(),
        vec![garrison, tank]
    );
    assert_eq!(entity(&sim, sold).health.current, 750, "silently");
}

/// A dying Crazy Ivan's death weapon is his own bomber: it plants a bomb on
/// him that goes off at once, IvanDamage at his Location with himself as the
/// planter. A tank at that Location takes the whole 450 once (the death
/// weapon's own shot deals nothing), and what the blast kills is credited to
/// his house.
#[test]
fn a_dying_crazy_ivan_explodes() {
    let rules = rules();
    let (mut sim, _grid) = arena(25, &rules);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 11, 12);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 11, 12);
    let engineer = spawn(&mut sim, &rules, "ENGINEER", "Americans", 11, 12);
    for id in [ivan, tank, engineer] {
        place(&mut sim, id, (3000, 3100));
    }
    kill(&mut sim, &rules, ivan);
    assert!(
        sim.bomb_carriers().is_empty(),
        "planted and gone off at once"
    );
    assert_eq!(entity(&sim, tank).health.current, 1000 - 450);
    let engineer = entity(&sim, engineer);
    assert_eq!(engineer.health.current, 0);
    assert_eq!(engineer.killed_by, sim.interner.get("Russians"));
}

/// A bombed Crazy Ivan who dies blows up once: his death weapon cannot plant
/// on a bombed target (Attach's gate at `0x00438EA3`), so only the bomb he
/// carries goes off (`0x00702672`), credited to its planter.
#[test]
fn a_bombed_crazy_ivan_blows_up_once() {
    let rules = rules();
    let (mut sim, _grid) = arena(33, &rules);
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", 11, 12);
    let carrier = spawn(&mut sim, &rules, "IVAN", "Russians", 11, 12);
    let engineer = spawn(&mut sim, &rules, "ENGINEER", "Russians", 11, 12);
    let planter = spawn(&mut sim, &rules, "IVAN", "Americans", 25, 25);
    for id in [carrier, tank, engineer] {
        place(&mut sim, id, (3000, 3100));
    }
    sim.bomb_attach(planter, Some(carrier), &rules);
    kill(&mut sim, &rules, carrier);
    assert!(sim.bomb_carriers().is_empty());
    assert_eq!(entity(&sim, tank).health.current, 1000 - 450, "one blast");
    let engineer = entity(&sim, engineer);
    assert_eq!(engineer.health.current, 0);
    assert_eq!(engineer.killed_by, sim.interner.get("Americans"));
}

/// The blast is Apply_area_damage at the carrier's Location with IvanDamage
/// and IvanWarhead, the planter as its source and no house (the corpus's
/// `detonate_blast` row: 450 at (3000, 3100, 0) from `techno0`). A carrier off
/// its cell's center takes the whole 450, and what the blast kills is credited
/// to the planter's house.
#[test]
fn the_blast_is_centred_on_the_carrier() {
    let rules = rules();
    let (mut sim, _grid) = arena(31, &rules);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 11, 12);
    let engineer = spawn(&mut sim, &rules, "ENGINEER", "Americans", 11, 12);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 25, 25);
    for id in [tank, engineer] {
        place(&mut sim, id, (3000, 3100));
    }
    sim.session.binary_frame = 100;
    sim.bomb_attach(ivan, Some(tank), &rules);
    sim.session.binary_frame = 100 + 451;
    sim.bomb_fuse_step(tank, &rules, None);
    assert_eq!(entity(&sim, tank).health.current, 1000 - 450);
    let engineer = entity(&sim, engineer);
    assert_eq!(engineer.health.current, 0);
    assert_eq!(engineer.killed_by, sim.interner.get("Russians"));
}

/// A blast that kills another carrier sets that bomb off in its death
/// (`0x00702672`), within the first blast's damage: the first carrier, at the
/// same Location, takes both.
#[test]
fn a_blast_sets_off_the_bombs_it_kills() {
    let rules = rules();
    let (mut sim, _grid) = arena(32, &rules);
    let first = spawn(&mut sim, &rules, "HTNK", "Americans", 11, 12);
    let second = spawn(&mut sim, &rules, "ENGINEER", "Americans", 11, 12);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 25, 25);
    for id in [first, second] {
        place(&mut sim, id, (3000, 3100));
    }
    sim.session.binary_frame = 100;
    sim.bomb_attach(ivan, Some(first), &rules);
    sim.session.binary_frame = 200;
    sim.bomb_attach(ivan, Some(second), &rules);
    sim.session.binary_frame = 100 + 451;
    sim.bomb_fuse_step(first, &rules, None);
    assert_eq!(entity(&sim, second).health.current, 0);
    assert!(sim.bomb_carriers().is_empty(), "both went off");
    assert_eq!(entity(&sim, first).health.current, 1000 - 2 * 450);
}

/// GetFireError's bomb gates (`0x006FCB8D`, `0x006FCBAD`) and the BombDisarm
/// arm (`0x004699C4`): an Engineer's DefuseKit fires only at a bombed object,
/// and defuses it; the Ivan fires only at an unbombed one.
#[test]
fn an_engineer_defuses_a_bomb() {
    let rules = rules();
    let (mut sim, grid) = arena(26, &rules);
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", 12, 10);
    let clean = spawn(&mut sim, &rules, "HTNK", "Russians", 12, 14);
    let engineer = spawn(&mut sim, &rules, "ENGINEER", "Americans", 11, 14);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 30, 10);
    sim.bomb_attach(ivan, Some(tank), &rules);

    // Against an unbombed tank the kit is illegal: nothing happens.
    attack(&mut sim, engineer, clean);
    for _ in 0..60 {
        step(&mut sim, &rules, &grid);
    }
    assert_eq!(entity(&sim, clean).health.current, 1000);

    attack(&mut sim, engineer, tank);
    run_until(&mut sim, &rules, &grid, 300, |sim| {
        bomb(sim, tank).is_none()
    })
    .expect("the engineer defuses it");
    assert_eq!(entity(&sim, tank).health.current, 1000, "no damage");
}

/// `TechnoClass::CanAcquireTarget` (`0x0070924D`): a human player's Engineer
/// never picks its own target, so a bomb on an enemy beside it stays until it
/// is ordered; a computer player's Engineer defuses one on its own, as native
/// lets it.
#[test]
fn only_a_computer_engineer_defuses_unordered() {
    let rules = rules();
    let (mut sim, grid) = arena(30, &rules);
    let russian_tank = spawn(&mut sim, &rules, "HTNK", "Russians", 12, 10);
    let american_tank = spawn(&mut sim, &rules, "HTNK", "Americans", 22, 10);
    let american_ivan = spawn(&mut sim, &rules, "IVAN", "Americans", 40, 40);
    let russian_ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 5, 45);
    sim.bomb_attach(american_ivan, Some(russian_tank), &rules);
    sim.bomb_attach(russian_ivan, Some(american_tank), &rules);
    spawn(&mut sim, &rules, "ENGINEER", "Americans", 12, 11);
    spawn(&mut sim, &rules, "ENGINEER", "Russians", 22, 11);
    run_until(&mut sim, &rules, &grid, 300, |sim| {
        bomb(sim, american_tank).is_none()
    })
    .expect("the computer's Engineer defuses the bomb on its enemy");
    assert!(
        bomb(&sim, russian_tank).is_some(),
        "the player's Engineer waits for an order"
    );
}

/// The bomb and the carrier list survive a snapshot and fold into the hash.
#[test]
fn a_bomb_survives_a_snapshot() {
    let rules = rules();
    let mut sim = sim(27);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 11, 10);
    let unbombed = sim.state_hash();
    sim.bomb_attach(ivan, Some(tank), &rules);
    assert_ne!(sim.state_hash(), unbombed, "the bomb is hashed");
    // Two UpdateAll calls after the attach: the planter's house sees it.
    sim.bomb_list_update(&rules);
    sim.bomb_list_update(&rules);
    let russians = sim.interner.get("Russians").unwrap();
    assert!(sim.bomb_seen_by(tank, russians));
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "bomb", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    // The envelope reseeds Scenario RNG; that policy is not under test here.
    restored.scenario_rng = sim.scenario_rng.clone();
    assert_eq!(
        bomb(&restored, tank),
        bomb(&sim, tank),
        "BombVisible is saved"
    );
    assert_eq!(restored.bomb_carriers(), sim.bomb_carriers());
    assert_eq!(restored.state_hash(), sim.state_hash());
    // `BombListClass::Clear` runs before a load and Save skips the countdown.
    assert_eq!(restored.bombs.visibility_countdown, 45);
}

/// The clock shows for the planter's player from the second UpdateAll after
/// the attach (Attach sets the countdown to 1), then refreshes every 46th
/// frame: an Engineer of the carrier's own player standing by starts seeing
/// it at the next refresh, and neither answer is part of the world hash.
#[test]
fn who_sees_a_bomb_follows_the_native_refresh() {
    let rules = rules();
    let (mut sim, grid) = arena(28, &rules);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 11, 10);
    let (americans, russians) = (
        sim.interner.get("Americans").unwrap(),
        sim.interner.get("Russians").unwrap(),
    );
    attack(&mut sim, ivan, tank);
    run_until(&mut sim, &rules, &grid, 200, |sim| {
        bomb(sim, tank).is_some()
    })
    .expect("planted");
    assert!(!sim.bomb_seen_by(tank, russians), "not in the attach frame");
    step(&mut sim, &rules, &grid);
    assert!(
        !sim.bomb_seen_by(tank, russians),
        "the countdown's 1 runs out first"
    );
    step(&mut sim, &rules, &grid);
    assert!(
        sim.bomb_seen_by(tank, russians),
        "the second UpdateAll refreshes"
    );
    assert!(
        !sim.bomb_seen_by(tank, americans),
        "the carrier's owner has no detector"
    );
    assert_eq!(sim.bombs.visibility_countdown, 45);

    // An American Engineer (BombSight=4) one cell away is seen at the next
    // refresh, 46 frames on. The Ivan leaves first: bombing the Engineer
    // would attach again, and every attach forces a refresh.
    sim.uninit_with_rules(ivan, &rules);
    spawn(&mut sim, &rules, "ENGINEER", "Americans", 12, 11);
    for _ in 0..45 {
        step(&mut sim, &rules, &grid);
        if bomb(&sim, tank).is_none() {
            break;
        }
    }
    assert!(bomb(&sim, tank).is_some(), "the fuse outlasts the refresh");
    assert!(!sim.bomb_seen_by(tank, americans), "not before the refresh");
    step(&mut sim, &rules, &grid);
    assert!(
        sim.bomb_seen_by(tank, americans),
        "the Engineer's player sees it"
    );
    assert!(sim.bomb_seen_by(tank, russians));

    // The refresh writes no hashed state.
    let before = sim.state_hash();
    sim.bombs.visibility_countdown = 0;
    if let Some(bomb) = sim
        .substrate
        .entities
        .get_mut(tank)
        .and_then(|entity| entity.bomb.as_mut())
    {
        bomb.seen_by = 0;
    }
    sim.bomb_list_update(&rules);
    assert!(sim.bomb_seen_by(tank, americans));
    assert_eq!(sim.state_hash(), before, "BombVisible is presentation");
}

/// The ticking loop plays at the carrier's Location while it is on the map
/// and stops in limbo (`BombListClass::UpdateAll`, 0x00438C84..0x00438CFE),
/// under an owner key apart from the carrier's own sounds.
#[test]
fn the_ticking_loop_follows_the_carrier() {
    let rules = rules();
    let mut sim = sim(29);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let ivan = spawn(&mut sim, &rules, "IVAN", "Russians", 11, 10);
    assert_eq!(sim.bomb_ticking_coord(tank), None, "no bomb, no loop");
    sim.bomb_attach(ivan, Some(tank), &rules);
    let owner = ticking_sound_owner(tank);
    assert_ne!(owner, tank);
    assert_eq!(ticking_sound_carrier(owner), Some(tank));
    assert_eq!(ticking_sound_carrier(tank), None);
    let expected = Simulation::movement_sound_world(entity(&sim, tank));
    assert_eq!(sim.bomb_ticking_coord(tank), Some(expected));
    assert_eq!(sim.looping_sound_owner_coord(owner), Some(expected));
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .lifecycle
        .in_limbo = true;
    assert_eq!(
        sim.looping_sound_owner_coord(owner),
        None,
        "stopped in limbo"
    );
}

/// Retail `rulesmd.ini` through the production reader (skipped without it):
/// the `[CombatDamage]` and `[AudioVisual]` keys the bomb reads, the two
/// Crazy Ivans and three Engineers that use it, and the stock data the
/// cursor and click rules lean on: no object opts out of `Bombable=`, and
/// only the Ivans have `AttackCursorOnFriendlies=`.
#[test]
fn retail_rules_arm_the_stock_bomb() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let rules = RuleSet::from_ini(&ini).unwrap();
    let damage = &rules.combat_damage;
    assert_eq!(damage.ivan_warhead.as_deref(), Some("IvanWH"));
    assert_eq!(
        (
            damage.ivan_damage,
            damage.ivan_timed_delay,
            damage.ivan_icon_flicker_rate
        ),
        (450, 450, 8)
    );
    assert_eq!(
        rules.general.bomb_ticking_sound.as_deref(),
        Some("CrazyIvanBombTick")
    );
    assert_eq!(
        rules.general.bomb_attach_sound.as_deref(),
        Some("CrazyIvanAttack")
    );
    let primary_warhead = |kind: &str| {
        let object = rules.object(kind).unwrap();
        let weapon = rules.weapon(object.primary.as_deref().unwrap()).unwrap();
        rules.warhead(weapon.warhead.as_deref().unwrap()).unwrap()
    };
    for ivan in ["IVAN", "CIVAN"] {
        let object = rules.object(ivan).unwrap();
        assert!(object.ivan && object.attack_cursor_on_friendlies, "{ivan}");
        assert!(primary_warhead(ivan).ivan_bomb, "{ivan}");
    }
    for engineer in ["ENGINEER", "SENGINEER", "YENGINEER"] {
        let object = rules.object(engineer).unwrap();
        assert!(object.engineer, "{engineer}");
        assert_eq!(object.bomb_sight, 4, "{engineer}");
        assert!(primary_warhead(engineer).bomb_disarm, "{engineer}");
    }
    assert!(rules.all_objects().all(|object| object.bombable));
    let mut friendly_cursor: Vec<&str> = rules
        .all_objects()
        .filter(|object| object.attack_cursor_on_friendlies)
        .map(|object| object.id.as_str())
        .collect();
    friendly_cursor.sort_unstable();
    assert_eq!(friendly_cursor, ["CIVAN", "IVAN"]);
}

/// The rules keys and their constructor defaults.
#[test]
fn bomb_rules_keys() {
    let rules = rules();
    assert_eq!(rules.combat_damage.ivan_warhead.as_deref(), Some("IvanWH"));
    assert_eq!(
        (
            rules.combat_damage.ivan_damage,
            rules.combat_damage.ivan_timed_delay,
            rules.combat_damage.ivan_icon_flicker_rate,
        ),
        (450, 450, 8)
    );
    assert_eq!(
        rules.general.bomb_attach_sound.as_deref(),
        Some("CrazyIvanAttack")
    );
    let ivan = rules.object("IVAN").unwrap();
    assert!(ivan.ivan && ivan.bombable);
    assert_eq!(rules.object("ENGINEER").unwrap().bomb_sight, 4);
    let bare = rules_from("[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n");
    assert_eq!(bare.combat_damage.ivan_warhead, None);
    assert_eq!(
        (
            bare.combat_damage.ivan_damage,
            bare.combat_damage.ivan_timed_delay,
            bare.combat_damage.ivan_icon_flicker_rate,
        ),
        (100, 450, 8),
        "RulesClass constructor 0x00666B88..0x00666BA8"
    );
}
