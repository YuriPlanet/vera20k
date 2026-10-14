//! Retail Dustbowl runs of a Battle Fortress's riders firing from it: the
//! only retail `OpenTopped=yes` type, loaded with five GIs through the
//! production boarding.

use crate::headless_scenario::HeadlessScenario;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::mission::MissionType;
use crate::sim::movement::ground_pose::position_world_coord;
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
    sim.resolve_type_handles(rules);
    let mut fortress = Fortress {
        scenario,
        bfrt,
        gis: Vec::new(),
        cell: (x, y),
    };
    for _ in 0..riders {
        let rider = board_infantry(&mut fortress, "E1");
        fortress.gis.push(rider);
    }
    fortress
}

/// Exercise the player's EnterTransport, class setter, Walk/Teleport arrival
/// and OpenTopped registration, rather than fabricating a Boarding role.
fn board_infantry(fortress: &mut Fortress, kind: &str) -> u64 {
    let (x, y) = fortress.cell;
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut fortress.scenario.runtime;
    let rider = sim
        .spawn_object_with_overlay_registry(
            kind,
            "Americans",
            x - 2,
            y + 2,
            0,
            &resources.rules,
            &resources.overlay_registry,
        )
        .unwrap_or_else(|| panic!("{kind} spawns"));
    sim.resolve_type_handles(&resources.rules);
    let enter = order(
        fortress,
        Command::EnterTransport {
            passenger_id: rider,
            transport_id: fortress.bfrt,
        },
    );
    retail_frame(&mut fortress.scenario, vec![enter]);
    for _ in 0..300 {
        if fortress
            .scenario
            .sim()
            .substrate
            .entities
            .get(rider)
            .is_some_and(|entity| entity.passenger_role.open_transport_id() == Some(fortress.bfrt))
        {
            return rider;
        }
        retail_frame(&mut fortress.scenario, Vec::new());
    }
    panic!("{kind} must finish player-ordered boarding");
}

/// One production frame of the retail runtime.
fn retail_frame(scenario: &mut HeadlessScenario, orders: Vec<CommandEnvelope>) -> SimFrameOutput {
    scenario
        .runtime
        .advance_frame(
            &orders,
            crate::headless_scenario::SIM_TICK_MS,
            TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
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
/// - Boarding runs `SetInOpenTransport 0x00710470`: each GI holds `+0x82`
///   and takes Guard (`ResetOrdersToGuard 0x0070F850`). Native51A441 keeps
///   its arrival subcell; carrier SetLocation later publishes7104F0.
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
            cargo.passengers(),
            gis.iter().rev().copied().collect::<Vec<_>>()
        );
        for &id in &gis {
            let gi = sim.substrate.entities.get(id).unwrap();
            assert_eq!(gi.passenger_role.inside_transport(), Some((bfrt, true)));
            assert!(gi.lifecycle.in_limbo());
            assert_eq!(gi.mission.current().known(), Some(MissionType::Guard));
            assert!(gi.attack_target.target().is_none());
            // Native51A441..51A46E preserves the arrival subcell; a later
            // carrier SetLocation copies its coordinate through4DB88A/7104F0.
            assert_eq!(
                (gi.position.rx, gi.position.ry),
                (transport.position.rx, transport.position.ry),
                "GI {id} boards in the Fortress's cell"
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
            first_volley = Some((output.fire_events.len(), 200 - target.health.current()));
        }
        if target.health.current() == 0 || !target.lifecycle.object_alive() {
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
    let mut firing_ports = std::collections::BTreeSet::new();
    for &(frame, rider, coord) in &shots {
        let cargo = sim
            .substrate
            .entities
            .get(bfrt)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap();
        let port = cargo
            .passengers()
            .iter()
            .position(|&id| id == rider)
            .unwrap();
        // Real sequential boarding retains each rider's own scan/animation
        // cadence; it does not synchronize their first shots into one frame.
        firing_ports.insert(port);
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
        firing_ports,
        (0..5).collect(),
        "all five boarded riders fire from their own ports"
    );
    assert!(
        sim.substrate
            .entities
            .get(enemy)
            .is_none_or(|target| !target.lifecycle.object_alive() || target.health.current() == 0),
        "the riders kill Tanya"
    );
    // The experience (`+0x150`), not yet a rank: Tanya's 1000 over the
    // Fortress's 2000 at `VeteranRatio=3` is a sixth of a rank.
    let experience = |id: u64| {
        f32::from_bits(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .veterancy
                .raw()
                .bits(),
        )
    };
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
                entities.get(id).unwrap().attack_target.target()
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
        let moving = crate::sim::movement::motion_query::is_moving(&transport) == Some(true);
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
                && gi.attack_target.target() == Some(TargetKind::Entity(apocalypse))
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
    assert!(transport.attack_target.target().is_none());
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
                entities.get(id).unwrap().attack_target.target()
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
            .spawn_object_with_overlay_registry(
                "E1",
                "Americans",
                x - 2,
                y + 2,
                0,
                &resources.rules,
                &resources.overlay_registry,
            )
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
        fortress
            .scenario
            .sim()
            .substrate
            .entities
            .get(gi)
            .unwrap()
            .passenger_role
            .inside_transport()
            .is_some_and(|(_, open)| open == true)
    };
    let mut frames = 0;
    while !inside(&fortress) {
        assert!(frames < 300, "the GI boards");
        retail_frame(&mut fortress.scenario, Vec::new());
        frames += 1;
    }
    println!("boarded after {frames} frames");
    let arrival = position_world_coord(
        &fortress
            .scenario
            .sim()
            .substrate
            .entities
            .get(gi)
            .unwrap()
            .position,
    );
    for frame in 0..60 {
        retail_frame(&mut fortress.scenario, Vec::new());
        let sim = fortress.scenario.sim();
        let [rider, transport] =
            [gi, bfrt].map(|id| sim.substrate.entities.get(id).expect("object lives"));
        assert!(inside(&fortress), "frame {frame}: still inside");
        assert!(rider.lifecycle.in_limbo(), "frame {frame}: in limbo");
        assert_eq!(
            position_world_coord(&rider.position),
            arrival,
            "frame {frame}: a stationary carrier retains the arrival subcell"
        );
        assert_eq!(
            rider.passenger_role.open_transport_id(),
            Some(transport.stable_id())
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

/// Original Walk75BE20..75BE3C stops the last path step before PerCell can
/// conceal an open-topped rider. Native Infantry fire admission reads that
/// speed fraction; being hidden must not strand a moving-speed fire refusal.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_battle_fortress_player_boarding_retires_walking_speed() {
    for kind in ["E1", "GGI", "GHOST", "CLEG"] {
        let mut fortress = retail_dustbowl_battle_fortress_boarded(0);
        let rider = board_infantry(&mut fortress, kind);
        let actor = fortress
            .scenario
            .sim()
            .substrate
            .entities
            .get(rider)
            .unwrap();
        println!(
            "{kind} speed={:?} nav={:?} doing={:?}",
            actor.foot_speed.applied_fraction(),
            actor.navigation.nav_com,
            actor.mission_leaf.as_infantry().map(|leaf| leaf.doing())
        );
        assert_eq!(
            actor.foot_speed.applied_fraction(),
            crate::util::fixed_math::SIM_ZERO,
            "{kind} must stop before boarding conceals its ordinary movement tail"
        );
    }
}

/// Stock passenger weapon choices and permissions were checked with original
/// Infantry/Techno selection (521917/6F3330), GetFireError's FireInTransport
/// gate6FC363 and retail RULESMD. Values come through the production reader.
/// Every armed, non-balloon, Size<=2 stock Infantry family with a permitted
/// undeployed weapon is exercised, including campaign/civilian aliases.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_battle_fortress_permitted_stock_riders_emit_their_weapons() {
    let cases = [
        ("E1", "Para"),
        ("E2", "M1Carbine"),
        ("SHK", "ElectricBolt"),
        ("GHOST", "MP5"),
        ("YURI", "MindControl"),
        ("DESO", "RadBeamWeapon"),
        ("CTECH", "Pistola"),
        ("WEEDGUY", "NukeCarrier"),
        ("CLEG", "NeutronRifle"),
        ("CCOMAND", "ChronoMP5"),
        ("PTROOP", "MindControl"),
        ("YURIPR", "SuperPsiWave"),
        ("SNIPE", "AWP"),
        ("ALL", "AlligatorBite"),
        ("TANY", "DoublePistols"),
        ("FLAKT", "FlakGuyGun"),
        ("VLADIMIR", "Pistola"),
        ("PENTGEN", "Pistola"),
        ("SSRV", "Pistola"),
        ("CIVA", "Pistola"),
        ("CIVB", "Pistola"),
        ("CIVC", "Pistola"),
        ("POLARB", "BearBite"),
        ("JOSH", "ChimpBite"),
        ("GGI", "MissileLauncher"),
        ("INIT", "PsychicJab"),
        ("BORIS", "AKM"),
        ("VIRUS", "Virusgun"),
        ("CLNT", "CLINTGUN"),
        ("ARND", "TERMIGUN"),
        ("STLN", "STALGUN"),
        ("SLAV", "SHOVEL"),
        ("MUMY", "Mummysmash"),
    ];
    for (kind, expected_weapon) in cases {
        let mut fortress = retail_dustbowl_battle_fortress_boarded(0);
        let rider = board_infantry(&mut fortress, kind);
        let (x, y) = fortress.cell;
        let target = if kind == "YURIPR" {
            // Native I8: AreaFire uses the firer's own cell. This is PsiWave,
            // not Yuri Prime's outside-transport SuperMindControl.
            TargetKind::Cell(x, y)
        } else {
            let enemy = spawn_enemy(
                &mut fortress,
                if matches!(kind, "YURI" | "PTROOP" | "CLEG" | "MUMY") {
                    "MTNK"
                } else {
                    "E2"
                },
                (
                    x + if matches!(kind, "YURI" | "PTROOP" | "CLEG") {
                        6
                    } else {
                        1
                    },
                    y,
                ),
            );
            TargetKind::Entity(enemy)
        };
        let command = match target {
            TargetKind::Entity(target_id) => Command::Attack {
                attacker_id: fortress.bfrt,
                target_id,
            },
            TargetKind::Cell(rx, ry) => Command::ForceAttackCell {
                attacker_id: fortress.bfrt,
                target_rx: rx,
                target_ry: ry,
            },
        };
        let envelope = order(&fortress, command);
        let mut output = retail_frame(&mut fortress.scenario, vec![envelope]);
        let mut fired = None;
        for _ in 0..180 {
            if let Some(event) = output
                .fire_events
                .iter()
                .find(|event| event.attacker_id == rider)
            {
                fired = Some(
                    fortress
                        .scenario
                        .sim()
                        .interner
                        .resolve(event.weapon_id)
                        .to_string(),
                );
                break;
            }
            output = retail_frame(&mut fortress.scenario, Vec::new());
        }
        assert_eq!(
            fired.as_deref(),
            Some(expected_weapon),
            "{kind} must fire its own weapon from BFRT"
        );
        if let TargetKind::Entity(target_id) = target {
            if matches!(kind, "YURI" | "PTROOP") {
                for _ in 0..20 {
                    if fortress
                        .scenario
                        .sim()
                        .substrate
                        .entities
                        .get(target_id)
                        .is_some_and(|victim| victim.mind_control.controller() == Some(rider))
                    {
                        break;
                    }
                    retail_frame(&mut fortress.scenario, Vec::new());
                }
                let sim = fortress.scenario.sim();
                let victim = sim.substrate.entities.get(target_id).unwrap();
                assert_eq!(
                    victim.mind_control.controller(),
                    Some(rider),
                    "{kind} captures through its own manager"
                );
                assert_eq!(
                    victim.owner(),
                    sim.substrate.entities.get(rider).unwrap().owner()
                );
            }
        }
    }
}

/// A freshly teleported rider must finish materializing inside, acquire and
/// freeze a target outside the hull's range. Native carrier Stop710550 clears
/// TarCom but keeps a Guard rider's beam (not ordinary Attack->Idle->LetGo).
/// Moving beyond OpenToppedWarpDistance releases it; otherwise it erases.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_battle_fortress_chrono_rider_freezes_releases_and_erases() {
    let control: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/temporal_stop.open_topped_guard.json",
    ))
    .unwrap();
    assert_eq!(
        control["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    let native_stop = &control["rows"][0]["output"]["after_stop"];
    for release_by_moving in [true, false] {
        let mut fortress = retail_dustbowl_battle_fortress_boarded(0);
        let rider = board_infantry(&mut fortress, "CLEG");
        assert!(
            fortress
                .scenario
                .sim()
                .substrate
                .entities
                .get(rider)
                .unwrap()
                .is_warping_in()
        );
        let (x, y) = fortress.cell;
        let target = spawn_enemy(&mut fortress, "MTNK", (x + 5, y + 3));
        assert!((1408..1792).contains(&distance(&fortress, fortress.bfrt, target)));
        fortress
            .scenario
            .runtime
            .simulation
            .substrate
            .entities
            .get_mut(target)
            .unwrap()
            .mission
            .set_current_for_test(
                crate::sim::mission::MissionId::from_known(MissionType::Sleep),
                0,
            );
        let original_health = fortress
            .scenario
            .sim()
            .substrate
            .entities
            .get(target)
            .unwrap()
            .health
            .current();
        let mut frozen = false;
        let mut stop_issued = false;
        let mut stop_observed = false;
        let mut move_issued = false;
        let mut completed = false;
        let mut pending = None;
        for frame in 0..1000 {
            // Observe range-triggered release before another Guard scan.
            if move_issued
                && !fortress
                    .scenario
                    .sim()
                    .substrate
                    .entities
                    .get(rider)
                    .unwrap()
                    .temporal
                    .is_warping_someone()
            {
                assert!(
                    distance(&fortress, rider, target)
                        > fortress
                            .scenario
                            .runtime
                            .resources
                            .rules
                            .combat_damage
                            .open_topped_warp_distance
                            .wrapping_mul(256),
                    "release must follow the production retail distance limit"
                );
                let victim = fortress
                    .scenario
                    .sim()
                    .substrate
                    .entities
                    .get(target)
                    .unwrap();
                assert!(!victim.ai_frozen());
                assert_eq!(victim.health.current(), original_health);
                completed = true;
                break;
            }
            retail_frame(&mut fortress.scenario, pending.take().into_iter().collect());
            let sim = fortress.scenario.sim();
            let actor = sim.substrate.entities.get(rider).unwrap();
            if actor.temporal.warp_target() == Some(target) {
                assert!(!actor.is_warping_in());
                let victim = sim.substrate.entities.get(target).unwrap();
                assert!(victim.ai_frozen());
                assert_eq!(victim.health.current(), original_health);
                frozen = true;
                if !stop_issued {
                    pending = Some(order(
                        &fortress,
                        Command::Stop {
                            entity_id: fortress.bfrt,
                        },
                    ));
                    stop_issued = true;
                }
                if stop_issued && actor.attack_target.target().is_none() && !stop_observed {
                    assert_eq!(
                        actor.attack_target.target().is_none(),
                        native_stop["target_is_null"].as_bool().unwrap()
                    );
                    assert_eq!(
                        i64::from(actor.mission.current().raw()),
                        native_stop["mission"].as_i64().unwrap()
                    );
                    assert_eq!(
                        actor.temporal.warp_target().is_some(),
                        native_stop["temporal_target"].as_u64().unwrap() != 0
                    );
                    assert_eq!(
                        victim.ai_frozen(),
                        native_stop["victim_warped"].as_bool().unwrap()
                    );
                    stop_observed = true;
                    if release_by_moving {
                        // The fixture's two-cell western corridor is flat and
                        // puts the carrier >7 cells from the stationary victim.
                        pending = Some(order(
                            &fortress,
                            Command::Move {
                                entity_id: fortress.bfrt,
                                target_rx: x - 2,
                                target_ry: y,
                            },
                        ));
                        move_issued = true;
                    }
                }
            }
            if sim
                .substrate
                .entities
                .get(target)
                .is_none_or(|victim| !victim.lifecycle.object_alive())
            {
                assert!(
                    !release_by_moving,
                    "the range-release scenario must preserve a living victim"
                );
                assert!(!actor.temporal.is_warping_someone());
                completed = true;
                break;
            }
            if matches!(frame, 120 | 300 | 999) {
                println!(
                    "chrono frame {frame}: warp={} mission={:?} queued={:?} target={:?} beam={:?} fraction={:?} doing={:?}",
                    actor.is_warping_in(),
                    actor.mission.current(),
                    actor.mission.queued(),
                    actor.attack_target,
                    actor.temporal.warp_target(),
                    actor.foot_speed.applied_fraction(),
                    actor.mission_leaf.as_infantry().map(|leaf| leaf.doing())
                );
            }
        }
        assert!(frozen, "rider freezes the target after hidden cooldown");
        assert!(
            stop_observed,
            "carrier Stop clears TarCom and retains the native Guard beam"
        );
        assert!(
            completed,
            "carrier distance releases it, or the complete Temporal chain erases it"
        );
    }
}

#[test]
fn retail_battle_fortress_forbidden_weapons_do_not_gain_transport_permission() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    for weapon in [
        "DefuseKit",
        "MakeupKit",
        "BadTeeth",
        "GoodTeeth",
        "Punch",
        "Smash",
        "TerrorBomb",
        "IvanBomber",
        "Mummypunch",
    ] {
        assert!(
            !retail.rules.weapon(weapon).unwrap().fire_in_transport,
            "{weapon}"
        );
    }
    // Native NeverUse selection excludes the scanner. Permission alone must
    // not turn an Engineer into a repair gunner in an OpenTopped carrier.
    let scanner = retail.rules.weapon("VirtualScanner").unwrap();
    assert!(scanner.fire_in_transport);
    assert!(scanner.never_use);
}
