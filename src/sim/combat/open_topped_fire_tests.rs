//! Retail Dustbowl runs of a Battle Fortress's riders firing from it: the
//! only retail `OpenTopped=yes` type, loaded with five GIs through the
//! production boarding.

use crate::headless_scenario::HeadlessScenario;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::mission::MissionType;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::passenger::PassengerRole;
use crate::sim::world::{SimFrameOutput, TickLane};

/// A loaded Battle Fortress on retail Dustbowl.
struct Fortress {
    scenario: HeadlessScenario,
    bfrt: u64,
    /// The GIs, in boarding order; the last is the cargo head.
    gis: Vec<u64>,
    cell: (u16, u16),
}

/// Retail Dustbowl with an Americans Battle Fortress on open level ground at
/// `cell`, with open level ground from two cells west to twelve cells east of
/// it, one cell north and four south, and five GIs boarded through the
/// production boarding. Americans and Russians are both human (no AI orders) and both
/// allied with the map's own `Player` house, whose start forces would
/// otherwise join the fight; each keeps a power plant out of the fight, so
/// neither is defeated under the Battle mode's ShortGame.
fn retail_dustbowl_battle_fortress() -> Fortress {
    retail_dustbowl_battle_fortress_boarded(5)
}

/// [`retail_dustbowl_battle_fortress`] with `riders` GIs boarded (at most 5).
fn retail_dustbowl_battle_fortress_boarded(riders: u16) -> Fortress {
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
    let rules = &resources.rules;
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let house = sim.interner.intern(name);
        sim.houses
            .entry(house)
            .or_insert_with(|| HouseState::new(house, side, None, true, 10_000, 10));
        if !sim.session.house_order.contains(&house) {
            sim.session.house_order.push(house);
        }
    }
    let (bfrt, x, y) = (40..100_u16)
        .flat_map(|y| (40..100_u16).map(move |x| (x, y)))
        .find_map(|(x, y)| {
            let grid = sim.path_grid()?;
            let terrain = sim.resolved_terrain.as_ref()?;
            let level = terrain.cell(x, y)?.level;
            let open = (x - 2..=x + 12).all(|cx| {
                (y - 1..=y + 4).all(|cy| {
                    terrain.cell(cx, cy).is_some_and(|cell| cell.level == level)
                        && grid.cell(cx, cy).is_some_and(|cell| cell.ground_walkable)
                })
            });
            if !open {
                return None;
            }
            let bfrt = sim.spawn_object("BFRT", "Americans", x, y, 0, rules)?;
            Some((bfrt, x, y))
        })
        .expect("open level ground for the fight");
    // A power plant each, sixteen or more cells from the fight.
    for (plant, owner) in [("GAPOWR", "Americans"), ("NAPOWR", "Russians")] {
        (20..120_u16)
            .flat_map(|py| (20..120_u16).map(move |px| (px, py)))
            .filter(|&(px, py)| px.abs_diff(x).max(py.abs_diff(y)) >= 16)
            .find_map(|(px, py)| sim.spawn_object(plant, owner, px, py, 0, rules))
            .unwrap_or_else(|| panic!("room for {plant}"));
    }
    for (house, ally) in [
        ("AMERICANS", "PLAYER"),
        ("PLAYER", "AMERICANS"),
        ("RUSSIANS", "PLAYER"),
        ("PLAYER", "RUSSIANS"),
    ] {
        sim.house_alliances
            .entry(house.to_string())
            .or_default()
            .insert(ally.to_string());
    }
    let gis: Vec<u64> = (0..riders)
        .map(|i| {
            let id = sim
                .spawn_object(
                    "E1",
                    "Americans",
                    x - 1 + i % 3,
                    if i < 3 { y + 1 } else { y - 1 },
                    0,
                    rules,
                )
                .expect("GI spawns");
            sim.substrate.entities.get_mut(id).unwrap().passenger_role = PassengerRole::Boarding {
                target_transport_id: bfrt,
            };
            id
        })
        .collect();
    sim.resolve_type_handles(rules);
    crate::sim::passenger::tick_passenger_system(sim, rules);
    Fortress {
        scenario,
        bfrt,
        gis,
        cell: (x, y),
    }
}

/// One production frame of the retail runtime.
fn retail_frame(scenario: &mut HeadlessScenario, orders: Vec<CommandEnvelope>) -> SimFrameOutput {
    scenario
        .runtime
        .advance_frame(
            &orders,
            crate::headless_scenario::SIM_TICK_MS,
            TickLane::Ordinary,
        )
        .expect("retail frame")
}

fn spawn_enemy(fortress: &mut Fortress, kind: &str, cell: (u16, u16)) -> u64 {
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut fortress.scenario.runtime;
    let id = sim
        .spawn_object(kind, "Russians", cell.0, cell.1, 0, &resources.rules)
        .expect("enemy spawns");
    sim.resolve_type_handles(&resources.rules);
    id
}

fn order(fortress: &Fortress, payload: Command) -> CommandEnvelope {
    let sim = fortress.scenario.sim();
    CommandEnvelope::new(
        sim.interner.get("Americans").expect("Americans house"),
        sim.session.tick + 1,
        payload,
    )
}

/// The 3-D lepton distance between two objects' coordinates.
fn distance(fortress: &Fortress, a: u64, b: u64) -> i32 {
    let entities = &fortress.scenario.sim().substrate.entities;
    let [a, b] = [a, b].map(|id| {
        let coord = position_world_coord(&entities.get(id).expect("object lives").position);
        [coord.x, coord.y, coord.z]
    });
    crate::util::native_x87::distance_3d_leptons(a, b)
}

/// Five GIs board a Battle Fortress on retail Dustbowl and fire from it at
/// Tanya four cells east and four south: 5.66 cells away, out of the
/// Fortress's own 5.5-cell 20mm but inside the riders' Para
/// (`OpenTransportWeapon=1`, `Range=5`) plus `OpenToppedRangeBonus=2`.
///
/// - Boarding runs `SetInOpenTransport 0x00710470`: each GI holds `+0x82`,
///   takes Guard (`ResetOrdersToGuard 0x0070F850`) and the Fortress's
///   coordinate (`0x007104F0`).
/// - The riders acquire the GI from their Guard scan and fire Para; the
///   Fortress does not fire. Each shot leaves from its rider's port,
///   `AlternateFLH` of the rider's 1-based cargo index from the head
///   (`InfantryClass::GetFLH 0x00523250`).
/// - Each shot of the first volley costs `ftol(25 * 1.2f)` = 30 (`FireAt
///   0x006FE43B`, pinned by `tools/spatial_oracle/damage_build`) at SSA's
///   100% against `Armor=flak`.
/// - The kill pays the Fortress, not the rider (`0x00702E9D`).
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_battle_fortress_riders_fire_from_its_ports() {
    let mut fortress = retail_dustbowl_battle_fortress();
    let (x, y) = fortress.cell;
    let (bfrt, gis) = (fortress.bfrt, fortress.gis.clone());
    {
        let sim = fortress.scenario.sim();
        let transport = sim.substrate.entities.get(bfrt).unwrap();
        let cargo = transport.passenger_role.cargo().expect("a transport");
        assert_eq!(
            cargo.passengers,
            gis.iter().rev().copied().collect::<Vec<_>>()
        );
        for &id in &gis {
            let gi = sim.substrate.entities.get(id).unwrap();
            assert!(matches!(
                gi.passenger_role,
                PassengerRole::Inside {
                    transport_id,
                    open_topped: true,
                } if transport_id == bfrt
            ));
            assert!(gi.lifecycle.in_limbo);
            assert_eq!(gi.mission.current().known(), Some(MissionType::Guard));
            assert!(gi.attack_target.is_none());
            assert_eq!(
                (
                    gi.position.rx,
                    gi.position.ry,
                    gi.position.sub_x,
                    gi.position.sub_y
                ),
                (
                    transport.position.rx,
                    transport.position.ry,
                    transport.position.sub_x,
                    transport.position.sub_y
                ),
                "GI {id} rides at the Fortress's coordinate"
            );
        }
    }
    let enemy = spawn_enemy(&mut fortress, "TANY", (x + 4, y + 4));
    let reach = distance(&fortress, bfrt, enemy);
    assert!((1408..1792).contains(&reach), "{reach}");

    let (ports, bfrt_z) = {
        let crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources,
        } = &fortress.scenario.runtime;
        let rules = &resources.rules;
        let object = rules.object("BFRT").unwrap();
        let art = rules
            .art()
            .get(&object.image)
            .or_else(|| rules.art().get(&object.id))
            .expect("BFRT art");
        let transport = sim.substrate.entities.get(bfrt).unwrap();
        let ports: [crate::rules::flh::Flh; 5] = std::array::from_fn(|port| {
            art.open_topped_port_flh(port, object.turret_count, object.weapon_count)
        });
        (
            ports,
            crate::sim::movement::ground_pose::object_world_z_leptons(
                transport,
                sim.resolved_terrain.as_ref(),
            ),
        )
    };
    let mut first_volley = None;
    let mut shots = Vec::new();
    for frame in 0..300 {
        let output = retail_frame(&mut fortress.scenario, Vec::new());
        let sim = fortress.scenario.sim();
        for event in &output.fire_events {
            assert_ne!(event.attacker_id, bfrt, "the Fortress is out of range");
            assert!(gis.contains(&event.attacker_id));
            assert_eq!(sim.interner.resolve(event.weapon_id), "Para");
            shots.push((frame, event.attacker_id, event.fire_coord));
        }
        let Some(target) = sim.substrate.entities.get(enemy) else {
            break;
        };
        if first_volley.is_none() && !output.fire_events.is_empty() {
            first_volley = Some((output.fire_events.len(), 200 - target.health.current));
        }
        if target.health.current == 0 || !target.lifecycle.object_alive {
            break;
        }
    }
    // Para is instant, so a volley lands in its own frame, before Tanya goes
    // prone (SSA's `ProneDamage=80%`, one ulp below 0.8, then cuts later hits
    // to 23).
    let (volley, damage) = first_volley.expect("the riders fire");
    println!("first volley: {volley} shots, {damage} damage");
    assert_eq!(
        damage,
        30 * volley as i32,
        "ftol(25 * 1.2f) a shot at SSA 100%"
    );
    let sim = fortress.scenario.sim();
    let first_frame = shots.first().map(|&(frame, ..)| frame);
    let mut first_ports = std::collections::BTreeSet::new();
    for &(frame, rider, coord) in &shots {
        let cargo = sim
            .substrate
            .entities
            .get(bfrt)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap();
        let port = cargo.passengers.iter().position(|&id| id == rider).unwrap();
        if Some(frame) == first_frame {
            first_ports.insert(port);
        }
        let flh = ports[port];
        let gi = sim.substrate.entities.get(rider).unwrap();
        let own = crate::sim::movement::ground_pose::position_world_coord(&gi.position);
        println!("frame {frame}: rider {rider} port {port} FLH {flh:?} fires from {coord:?}");
        assert_eq!(coord.z, bfrt_z + flh.height, "port {port} height");
        assert_ne!(
            (coord.x, coord.y),
            (own.x, own.y),
            "port {port} is off-centre"
        );
    }
    assert_eq!(
        first_ports,
        (0..5).collect(),
        "the first volley leaves from all five ports"
    );
    assert!(
        sim.substrate
            .entities
            .get(enemy)
            .is_none_or(|target| !target.lifecycle.object_alive || target.health.current == 0),
        "the riders kill Tanya"
    );
    // The experience (`+0x150`), not yet a rank: Tanya's 1000 over the
    // Fortress's 2000 at `VeteranRatio=3` is a sixth of a rank.
    let experience =
        |id: u64| f32::from_bits(sim.substrate.entities.get(id).unwrap().veterancy_raw.bits());
    assert!(experience(bfrt) > 0.0, "the kill pays the Fortress");
    for &id in &gis {
        assert_eq!(experience(id), 0.0, "not the rider");
    }
}

/// A loaded Battle Fortress ordered onto an Apocalypse twelve cells east:
///
/// - The order hands every rider the target (`SetTargetForPassengers
///   0x00710550` from MEGAMISSION `0x004C749D`), and each drops it on its next
///   AI pass because it cannot fire at it and, on Guard, may not chase it
///   (`Approach_Target 0x004D5690` from `0x006FAAEF`).
/// - The Fortress drives on past its own 20mm's 5.5 cells and stops at the
///   first cell centre where its 3-D distance is under GetWeaponRange, which
///   caps its range at the riders' M60 (4 cells, `0x004D885C..0x004D88F4`).
/// - A Stop clears the riders' targets with the Fortress's (`0x004C7650`).
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_battle_fortress_orders_reach_its_riders() {
    let mut fortress = retail_dustbowl_battle_fortress();
    let (x, y) = fortress.cell;
    let (bfrt, gis) = (fortress.bfrt, fortress.gis.clone());
    let apocalypse = spawn_enemy(&mut fortress, "APOC", (x + 12, y));
    let targets = |fortress: &Fortress| -> Vec<Option<TargetKind>> {
        gis.iter()
            .map(|&id| {
                let entities = &fortress.scenario.sim().substrate.entities;
                entities
                    .get(id)
                    .unwrap()
                    .attack_target
                    .as_ref()
                    .map(|attack| attack.target)
            })
            .collect()
    };

    let attack = order(
        &fortress,
        Command::Attack {
            attacker_id: bfrt,
            target_id: apocalypse,
        },
    );
    retail_frame(&mut fortress.scenario, vec![attack]);
    assert_eq!(
        targets(&fortress),
        vec![Some(TargetKind::Entity(apocalypse)); 5],
        "the order reaches every rider"
    );
    retail_frame(&mut fortress.scenario, Vec::new());
    assert_eq!(targets(&fortress), vec![None; 5], "out of reach on Guard");
    for &id in &gis {
        let gi = fortress.scenario.sim().substrate.entities.get(id).unwrap();
        assert_eq!(gi.mission.current().known(), Some(MissionType::Guard));
    }

    let mut previous = distance(&fortress, bfrt, apocalypse);
    let (mut moved, mut passed_own_range, mut stop) = (false, false, None);
    for frame in 0..600 {
        retail_frame(&mut fortress.scenario, Vec::new());
        let now = distance(&fortress, bfrt, apocalypse);
        let transport = fortress
            .scenario
            .sim()
            .substrate
            .entities
            .get(bfrt)
            .unwrap();
        let moving = transport.movement_target.is_some();
        moved |= moving;
        passed_own_range |= moving && now < 1408;
        if moved && !moving {
            stop = Some((frame, previous, now));
            break;
        }
        previous = now;
    }
    let (frame, before, at) = stop.expect("the Fortress stops");
    println!("stopped at frame {frame}: {before} -> {at} leptons");
    assert!(passed_own_range, "drives on inside its own 5.5 cells");
    // Straight along the row, the first centre under 4 cells is 3 cells out.
    let rest = fortress
        .scenario
        .sim()
        .substrate
        .entities
        .get(bfrt)
        .map(|transport| position_world_coord(&transport.position))
        .unwrap();
    assert_eq!(
        (rest.x % 256, rest.y % 256),
        (128, 128),
        "rests on a centre"
    );
    assert!(
        (1024 - 256..1024).contains(&at),
        "stops at the riders' M60 range: {at}"
    );

    // In reach, each rider's own Guard scan takes the Apocalypse again
    // (`+0x50C`, `0x006FA6EE`), so the Stop below has targets to clear.
    let passive = |fortress: &Fortress| {
        gis.iter().all(|&id| {
            let gi = fortress.scenario.sim().substrate.entities.get(id).unwrap();
            gi.passively_acquired_target
                && gi.attack_target.as_ref().map(|attack| attack.target)
                    == Some(TargetKind::Entity(apocalypse))
        })
    };
    for _ in 0..120 {
        if passive(&fortress) {
            break;
        }
        retail_frame(&mut fortress.scenario, Vec::new());
    }
    assert!(passive(&fortress), "the riders re-acquire it in reach");

    let stop_order = order(&fortress, Command::Stop { entity_id: bfrt });
    retail_frame(&mut fortress.scenario, vec![stop_order]);
    assert_eq!(targets(&fortress), vec![None; 5], "Stop clears the riders");
    let transport = fortress
        .scenario
        .sim()
        .substrate
        .entities
        .get(bfrt)
        .unwrap();
    assert!(transport.attack_target.is_none());
}

/// A loaded Battle Fortress force-fired at the ground: the order hands the
/// riders the cell (`SetTargetForPassengers 0x00710550` takes a cell TarCom
/// too). A cell six cells east is inside their Para plus the open-topped
/// bonus, which `InRange` adds for a cell target as for an object
/// (`0x006F72C8`, pinned by `tools/spatial_oracle/walk_cell_range`), so they
/// keep it and fire at it; a cell eight cells east is beyond it, so each
/// rider drops it on its next AI pass (`0x006FAAEF`).
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_battle_fortress_riders_force_fire_at_the_ground() {
    let mut fortress = retail_dustbowl_battle_fortress();
    let (x, y) = fortress.cell;
    let (bfrt, gis) = (fortress.bfrt, fortress.gis.clone());
    let targets = |fortress: &Fortress| -> Vec<Option<TargetKind>> {
        gis.iter()
            .map(|&id| {
                let entities = &fortress.scenario.sim().substrate.entities;
                entities
                    .get(id)
                    .unwrap()
                    .attack_target
                    .as_ref()
                    .map(|attack| attack.target)
            })
            .collect()
    };
    for (cells, kept) in [(8, false), (6, true)] {
        let cell = TargetKind::Cell(x + cells, y);
        let force = order(
            &fortress,
            Command::ForceAttackCell {
                attacker_id: bfrt,
                target_rx: x + cells,
                target_ry: y,
            },
        );
        retail_frame(&mut fortress.scenario, vec![force]);
        assert_eq!(targets(&fortress), vec![Some(cell); 5], "{cells} cells");
        let output = retail_frame(&mut fortress.scenario, Vec::new());
        let expected = if kept { Some(cell) } else { None };
        assert_eq!(targets(&fortress), vec![expected; 5], "{cells} cells");
        if kept {
            let mut fired = output
                .fire_events
                .iter()
                .any(|event| gis.contains(&event.attacker_id) && event.target == cell);
            for _ in 0..60 {
                if fired {
                    break;
                }
                let output = retail_frame(&mut fortress.scenario, Vec::new());
                fired = output
                    .fire_events
                    .iter()
                    .any(|event| gis.contains(&event.attacker_id) && event.target == cell);
            }
            assert!(fired, "the riders fire at the ground {cells} cells away");
        }
        let stop = order(&fortress, Command::Stop { entity_id: bfrt });
        retail_frame(&mut fortress.scenario, vec![stop]);
    }
}

/// A GI ordered into an empty Battle Fortress from two cells away walks in
/// (production `EnterTransport`) and boards. Inside, it keeps the Fortress's
/// coordinate frame after frame and leaves no infantry occupation on the cells
/// it crossed: its walk does not carry on from inside the transport.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_battle_fortress_rider_walks_in_and_stays_put() {
    let mut fortress = retail_dustbowl_battle_fortress_boarded(0);
    let (x, y) = fortress.cell;
    let bfrt = fortress.bfrt;
    let gi = {
        let crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources,
        } = &mut fortress.scenario.runtime;
        let id = sim
            .spawn_object("E1", "Americans", x - 2, y + 2, 0, &resources.rules)
            .expect("GI spawns");
        sim.resolve_type_handles(&resources.rules);
        id
    };
    let enter = order(
        &fortress,
        Command::EnterTransport {
            passenger_id: gi,
            transport_id: bfrt,
        },
    );
    retail_frame(&mut fortress.scenario, vec![enter]);
    let inside = |fortress: &Fortress| {
        matches!(
            fortress
                .scenario
                .sim()
                .substrate
                .entities
                .get(gi)
                .unwrap()
                .passenger_role,
            PassengerRole::Inside {
                open_topped: true,
                ..
            }
        )
    };
    let mut frames = 0;
    while !inside(&fortress) {
        assert!(frames < 300, "the GI boards");
        retail_frame(&mut fortress.scenario, Vec::new());
        frames += 1;
    }
    println!("boarded after {frames} frames");
    for frame in 0..60 {
        retail_frame(&mut fortress.scenario, Vec::new());
        let sim = fortress.scenario.sim();
        let [rider, transport] =
            [gi, bfrt].map(|id| sim.substrate.entities.get(id).expect("object lives"));
        assert!(inside(&fortress), "frame {frame}: still inside");
        assert!(rider.lifecycle.in_limbo, "frame {frame}: in limbo");
        assert_eq!(
            position_world_coord(&rider.position),
            position_world_coord(&transport.position),
            "frame {frame}: the rider holds the Fortress's coordinate"
        );
        for cx in x - 3..=x + 1 {
            for cy in y - 1..=y + 3 {
                assert!(
                    !sim.substrate.occupancy.contains_entity(cx, cy, gi),
                    "frame {frame}: the rider is listed in ({cx}, {cy})"
                );
                assert_eq!(
                    sim.substrate.raw_cell_occupation.ground_bits(cx, cy) & 0x1F,
                    0,
                    "frame {frame}: infantry occupation left at ({cx}, {cy})"
                );
            }
        }
    }
}
