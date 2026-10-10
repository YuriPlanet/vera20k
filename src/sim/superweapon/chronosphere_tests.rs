//! The Chronosphere and the Chrono Warp on retail rules through production
//! frames: Launch cases 3 and 4 (`chronosphere`), the Teleport's
//! Chronosphere states (`movement::teleport_chrono`) and the end of the
//! piggyback.
//!
//! The frame-by-frame states, owner bytes, timers and the end frame are
//! pinned against `tools/superweapon_oracle.json` `chrono_process`, where
//! `TeleportLocomotionClass::Process @ 0x007192F0`, its TimerCheck
//! (`0x00719BF0`) and Is_Ok_To_End (`0x00719F30`) run in Unicorn through
//! the class AI's frame flow (the prologue's extra Process, the frozen
//! return, FootClass::AI's Process and its end of the piggyback). VERA runs
//! commands at the frame's tail, so the warp's first Process frame is the
//! one after the destination click.

use super::SuperWeaponInstance;
use crate::map::resolved_terrain::test_grid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::superweapon::cell_receiver_tests::test_terrain_cell;
use crate::sim::world::{SimSoundEvent, Simulation};

pub(crate) const SOURCE: (u16, u16) = (21, 21);
const TARGET: (u16, u16) = (40, 40);

fn retail_rules() -> Option<RuleSet> {
    retail_rules_binding(&[
        ("CHRONOAR", 20),
        ("CHRONOTG", 20),
        ("CHRONOFD", 20),
        ("WARPOUT", 20),
        ("CHRONOSK", 20),
    ])
}

/// Retail rules with `anims`' SHP frame counts bound: the lib suite loads no
/// SHP.
pub(crate) fn retail_rules_binding(anims: &[(&str, i32)]) -> Option<RuleSet> {
    let (rules_ini, art_ini) = crate::rules::retail_ini_fixture::retail_rules_and_art()?;
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    for &(name, frames) in anims {
        rules.bind_anim_frame_count_for_test(name, frames);
    }
    Some(rules)
}

/// Retail rules and a flat 64x64 map ([`world_with`]).
pub(crate) fn retail_world() -> Option<(RuleSet, Simulation, InternedId)> {
    Some(world_with(retail_rules()?, 64, &[]))
}

/// `rules` and a `size`-square map, flat but for `levels`, with its zones,
/// Map Size and path grid: Americans (human) and Russians, in a
/// campaign-mode session (no multiplayer defeat gate: neither house holds a
/// building).
pub(crate) fn world_with(
    rules: RuleSet,
    size: u16,
    levels: &[((u16, u16), u8)],
) -> (RuleSet, Simulation, InternedId) {
    world_with_cells(rules, size, |x, y| {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            level: levels
                .iter()
                .find(|(at, _)| *at == (x, y))
                .map_or(0, |&(_, level)| level),
            ..test_terrain_cell(x, y)
        }
    })
}

/// [`world_with`] over the cells `cell` builds.
fn world_with_cells(
    rules: RuleSet,
    size: u16,
    cell: impl FnMut(u16, u16) -> crate::map::resolved_terrain::ResolvedTerrainCell,
) -> (RuleSet, Simulation, InternedId) {
    let mut sim = Simulation::with_seed(17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let americans = sim.interner.intern("Americans");
    let russians = sim.interner.intern("Russians");
    for (id, side, human) in [(americans, 0, true), (russians, 1, false)] {
        sim.houses
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
    }
    sim.session.game_options.super_weapons = true;
    sim.session.map_width = size;
    sim.session.map_height = size;
    sim.resolved_terrain = Some(test_grid(size, size, cell));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    (rules, sim, americans)
}

/// Retail rules and a flat 64x64 map of open water.
fn retail_water_world() -> Option<(RuleSet, Simulation, InternedId)> {
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
    let float = SpeedCostProfile {
        float: Some(100),
        ..SpeedCostProfile::default()
    };
    let water = LandType::Water.as_index();
    Some(world_with_cells(retail_rules()?, 64, |x, y| {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            land_type: water,
            yr_cell_land_type: water,
            base_land_type: water,
            base_yr_cell_land_type: water,
            terrain_class: TerrainClass::Water,
            base_terrain_class: TerrainClass::Water,
            speed_costs: float,
            base_speed_costs: float,
            is_water: true,
            zone_type: 4,
            ground_walk_blocked: true,
            base_ground_walk_blocked: true,
            base_build_blocked: true,
            ..crate::map::resolved_terrain::test_flat_cell(x, y)
        }
    }))
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, at: (u16, u16)) -> u64 {
    sim.spawn_object_at_height(kind, owner, at.0, at.1, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} stands at {at:?}"))
}

pub(crate) fn step(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), None, None, 33);
}

/// A charged Chronosphere for `owner`: its recharge timer has run out.
pub(crate) fn charge_chronosphere(sim: &mut Simulation, owner: InternedId) -> InternedId {
    charge_super(sim, owner, "ChronoSphereSpecial")
}

/// Constructs `kind` for the Americans on `cell`'s ground or bridge list at
/// `level` (a deck object 416 leptons over the ground), holding the
/// Chronosphere's warp latch when `latch` (on its Teleport), and reveals it.
pub(crate) fn place(
    sim: &mut Simulation,
    rules: &RuleSet,
    kind: &str,
    (x, y): (u16, u16),
    level: u8,
    bridge: bool,
    latch: bool,
) -> u64 {
    let deck = if bridge { 4 } else { 0 };
    let id = sim
        .construct_object_limbo_at_height(kind, "Americans", x, y, 0, level + deck, rules)
        .unwrap_or_else(|| panic!("{kind} constructs at {x},{y}"));
    let frame = sim.session.binary_frame;
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    crate::sim::movement::ground_pose::put_location(
        &mut entity.position,
        DriveCoord {
            x: i32::from(x) * 256 + 128,
            y: i32::from(y) * 256 + 128,
            z: i32::from(level + deck) * 104,
        },
    );
    entity.position.z = level + deck;
    entity.on_bridge = bridge;
    if latch {
        let house = entity.owner();
        entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.teleport_runtime_mut())
            .expect("a latched Foot on its Teleport")
            .arm_chrono(crate::sim::movement::teleport_movement::ChronoWarp::new(
                DriveCoord { x: 0, y: 0, z: 0 },
                house,
                frame,
            ));
    }
    assert!(matches!(
        sim.reveal(id),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().chrono_warp_latch(),
        latch
    );
    id
}

/// A charged Super of type `name` for `owner`, replacing any it has: its
/// recharge timer has run out.
pub(crate) fn charge_super(sim: &mut Simulation, owner: InternedId, name: &str) -> InternedId {
    let sw_type = sim.interner.intern(name);
    let mut instance = SuperWeaponInstance::new(sw_type, owner, 0);
    instance.activate(6300, sim.session.binary_frame);
    instance.charge_start_tick -= 6300;
    instance.is_ready = true;
    sim.super_weapons
        .entry(owner)
        .or_default()
        .insert(sw_type, instance);
    sw_type
}

/// One click as its SPECIAL_PLACE event, run at the next frame's tail.
pub(crate) fn click(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    name: &str,
    at: (u16, u16),
) {
    let sw_type_id = sim.interner.intern(name);
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::LaunchSuperWeapon {
            sw_type_id,
            target_rx: at.0,
            target_ry: at.1,
        },
    ));
    step(sim, rules);
}

fn cell(sim: &Simulation, id: u64) -> (u16, u16) {
    let entity = sim.substrate.entities.get(id).expect("alive");
    (entity.position.rx, entity.position.ry)
}

fn locomotors(sim: &Simulation, id: u64) -> (LocomotorKind, Option<LocomotorKind>) {
    let locomotor = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.locomotor.as_ref())
        .expect("a locomotor");
    (
        locomotor.active_kind(),
        locomotor.piggyback.as_ref().map(|stash| stash.kind),
    )
}

fn dead(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .is_none_or(|entity| entity.health.current == 0 || entity.dying)
}

/// The keys the chain reads, through the production reader on the retail
/// INIs.
#[test]
fn retail_chrono_rules() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let sphere = rules.super_weapon("ChronoSphereSpecial").unwrap();
    assert_eq!(
        (sphere.kind, sphere.pre_click, sphere.post_click),
        (SuperWeaponKind::ChronoSphere, true, false)
    );
    let warp = rules.super_weapon("ChronoWarpSpecial").unwrap();
    assert_eq!(
        (
            warp.kind,
            warp.pre_click,
            warp.post_click,
            warp.pre_dependent
        ),
        (SuperWeaponKind::ChronoWarp, false, true, 3)
    );
    assert_eq!(rules.super_weapon_order[3], "ChronoSphereSpecial");
    assert_eq!(
        rules.super_weapon_order[super::CHRONO_WARP_SELECTION_INDEX],
        "ChronoWarpSpecial"
    );
    let general = &rules.general;
    assert_eq!(general.chrono_delay, 60);
    assert_eq!(
        (
            general.chrono_placement_anim.as_str(),
            general.chrono_blast_anim.as_str(),
            general.chrono_blast_dest_anim.as_str(),
            general.warp_out.name.as_str()
        ),
        ("CHRONOAR", "CHRONOFD", "CHRONOTG", "WARPOUT")
    );
    assert_eq!(rules.bridge_warheads.c4_name, "Super");
    // Organic Infantry unless a type says otherwise; the Chrono infantry are
    // Teleporters.
    assert!(rules.object("E1").unwrap().organic);
    let cleg = rules.object("CLEG").unwrap();
    assert!(cleg.organic && cleg.teleporter);
    assert!(!rules.object("MTNK").unwrap().organic);
}

/// The player's two clicks: the source click holds the Chronosphere's
/// charge-free cell and anim, the destination click warps the source block.
/// The GI dies at once; the Grizzly and the Chrono Legionnaire stay frozen
/// at the source, land at the target plus their offsets from the source
/// and then regain their own locomotors.
#[test]
fn retail_chrono_warp_moves_the_source_block() {
    let Some((rules, mut sim, americans)) = retail_world() else {
        return;
    };
    let tank = spawn(&mut sim, &rules, "MTNK", "Americans", (20, 20));
    let gi = spawn(&mut sim, &rules, "E1", "Americans", (21, 20));
    let legionnaire = spawn(&mut sim, &rules, "CLEG", "Americans", (20, 21));
    let sphere = charge_chronosphere(&mut sim, americans);

    // Case 3: the cell and the placement anim, the charge cleared without a
    // recharge; the next Super AI finds the timer run out and readies it.
    click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
    let instance = &sim.super_weapons[&americans][&sphere];
    assert_eq!(instance.chrono_cell(), SOURCE);
    let placement: Vec<_> = sim.super_placement_anims();
    assert_eq!(placement.len(), 1);
    let anim = sim.substrate.anims.get(placement[0].1).unwrap();
    assert_eq!(sim.interner.resolve(anim.type_id), "CHRONOAR");
    // Case 3 lifts the cell 5 leptons and CreateChronoAnim 5 more
    // (`0x006CC423`, `0x006CB43B`).
    assert_eq!(anim.world_coord.z, super::deck_coords(&sim, SOURCE)[2] + 10);
    step(&mut sim, &rules);
    assert!(sim.super_weapons[&americans][&sphere].is_ready);

    // Case 4.
    click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
    assert!(dead(&sim, gi), "an Organic non-Teleporter takes C4");
    for id in [tank, legionnaire] {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert!(entity.chrono_warp_latch(), "{id} latched");
    }
    assert_eq!(
        locomotors(&sim, tank),
        (LocomotorKind::Teleport, Some(LocomotorKind::Drive))
    );
    assert_eq!(
        locomotors(&sim, legionnaire),
        (LocomotorKind::Teleport, Some(LocomotorKind::Teleport))
    );
    let radar: Vec<_> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::SuperWeaponRadarEvent { radar } => Some((radar.rx, radar.ry)),
            _ => None,
        })
        .collect();
    assert_eq!(radar, vec![SOURCE, TARGET]);
    // Fire_SW's pairing restarted the Chronosphere's recharge and let its
    // anim go.
    let instance = &sim.super_weapons[&americans][&sphere];
    assert!(!instance.is_ready);
    assert_eq!(instance.charge_duration, 6300);
    assert!(sim.super_placement_anims().is_empty());

    // Frozen while warped out; the landing; the end of the warp.
    let mut landed = false;
    let mut ended = false;
    for frame in 1..=70 {
        step(&mut sim, &rules);
        let entity = sim.substrate.entities.get(tank).unwrap();
        if !landed && cell(&sim, tank) == (39, 39) {
            landed = true;
            assert_eq!(cell(&sim, legionnaire), (39, 40));
            assert!(entity.is_warping_in());
        }
        if !landed {
            assert_eq!(cell(&sim, tank), (20, 20), "frame {frame}");
            assert_eq!(entity.ai_frozen(), frame >= 1, "frame {frame}");
        }
        if landed && !ended && entity.chrono_warp().is_none() {
            ended = true;
            assert_eq!(locomotors(&sim, tank), (LocomotorKind::Drive, None));
            assert_eq!(
                locomotors(&sim, legionnaire),
                (LocomotorKind::Teleport, None)
            );
        }
    }
    assert!(landed && ended);
    let entity = sim.substrate.entities.get(tank).unwrap();
    assert!(!entity.is_warping_in() && !entity.is_warped_out());
    assert!(entity.navigation.nav_com.is_none());
    assert!(!dead(&sim, tank) && !dead(&sim, legionnaire));
}

/// The locomotor chain from the active object down.
fn chain(sim: &Simulation, id: u64) -> Vec<LocomotorKind> {
    let mut kinds = Vec::new();
    let mut at = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.locomotor.as_ref());
    while let Some(locomotor) = at {
        kinds.push(locomotor.active_kind());
        at = locomotor.piggyback.as_deref();
    }
    kinds
}

/// A Chrono Miner driving on the Drive its setter installed warps with the
/// block: the warp's fresh Teleport suspends the Drive with the miner's
/// Teleport inside it (`0x006CCB4A`; Begin_Piggyback tests only its own
/// slot, `0x00719EA9`), and the warp's END hands the Drive back. That
/// Drive's refused own-cell search clears its destination, so the Foot AI
/// tail ends it at the landing. Its next order drives it, and it ends back
/// on its Teleport at the stop.
#[test]
fn retail_chrono_warp_carries_a_driving_chrono_miner() {
    use LocomotorKind::{Drive, Teleport};
    let Some((rules, mut sim, americans)) = retail_world() else {
        return;
    };
    let miner = spawn(&mut sim, &rules, "CMIN", "Americans", (20, 20));
    let order = |sim: &mut Simulation, (rx, ry): (u16, u16)| {
        let command = Command::Move {
            entity_id: miner,
            target_rx: rx,
            target_ry: ry,
            queue: false,
        };
        assert!(sim.apply_command_with_overlays(
            "Americans",
            &command,
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default()
        ));
    };
    order(&mut sim, (30, 20));
    step(&mut sim, &rules);
    assert_eq!(chain(&sim, miner), [Drive, Teleport]);
    charge_chronosphere(&mut sim, americans);
    click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
    step(&mut sim, &rules);
    let from = cell(&sim, miner);
    click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
    assert_eq!(chain(&sim, miner), [Teleport, Drive, Teleport]);

    let landing = (TARGET.0 + from.0 - SOURCE.0, TARGET.1 + from.1 - SOURCE.1);
    let mut chains = vec![(chain(&sim, miner), from)];
    let warping = |sim: &Simulation| {
        let entity = sim.substrate.entities.get(miner).unwrap();
        entity.chrono_warp().is_some()
    };
    for _ in 0..80 {
        step(&mut sim, &rules);
        let now = (chain(&sim, miner), cell(&sim, miner));
        let done = !warping(&sim) && now.0 == [Teleport];
        if chains.last() != Some(&now) {
            chains.push(now);
        }
        if done {
            break;
        }
    }
    assert_eq!(
        chains,
        [
            (vec![Teleport, Drive, Teleport], from),
            (vec![Teleport, Drive, Teleport], landing),
            (vec![Drive, Teleport], landing),
            (vec![Teleport], landing),
        ]
    );
    let entity = sim.substrate.entities.get(miner).unwrap();
    assert!(!warping(&sim) && entity.navigation.nav_com.is_none());

    let next = (landing.0 + 4, landing.1);
    order(&mut sim, next);
    let mut drove = false;
    for _ in 0..200 {
        step(&mut sim, &rules);
        drove |= chain(&sim, miner)[0] == Drive && cell(&sim, miner) != landing;
    }
    assert!(drove);
    assert_eq!(
        (chain(&sim, miner), cell(&sim, miner)),
        (vec![Teleport], next)
    );
}

/// A destroyer under way when the warp arms has its Ship forced onto its
/// landing (`ShipLocomotionClass::Force_Track @ 0x006A0310`, Drive's twin):
/// no track (-1), its head and destination at the landing cell's deck
/// coordinate. The warp's END hands that Ship back. Its next Process runs
/// Process_Movement (`0x006A0142`), whose Find_Path for the landing cell
/// refuses (AStar returns no route for a goal in its start cell,
/// `0x00429BF3..0x00429C0A`): the destination clears, the head stays, and
/// the ship stays where it landed.
#[test]
fn retail_chrono_warp_forces_a_sailing_ships_track_to_its_landing() {
    use LocomotorKind::{Ship, Teleport};
    let Some((rules, mut sim, americans)) = retail_water_world() else {
        return;
    };
    let ship = spawn(&mut sim, &rules, "DEST", "Americans", (20, 20));
    let command = Command::Move {
        entity_id: ship,
        target_rx: 30,
        target_ry: 20,
        queue: false,
    };
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &command,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default()
    ));
    charge_chronosphere(&mut sim, americans);
    click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
    step(&mut sim, &rules);
    let track = |sim: &Simulation| {
        let locomotor = sim.substrate.entities.get(ship)?.locomotor.as_ref()?;
        let ship = locomotor.selected_ship_runtime()?.retained()?;
        Some((ship.track().turn_index, ship.head_to(), ship.destination()))
    };
    // The destroyer turns in place before its first track.
    for _ in 0..30 {
        if track(&sim).is_some_and(|(turn, _, _)| turn != -1) {
            break;
        }
        step(&mut sim, &rules);
    }
    let (turn, old_head, _) = track(&sim).expect("a sailing destroyer's Ship");
    assert!(
        turn != -1 && old_head.is_some(),
        "on a track when the warp arms"
    );
    let from = cell(&sim, ship);
    click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
    assert_eq!(chain(&sim, ship), [Teleport, Ship]);

    let landing = (TARGET.0 + from.0 - SOURCE.0, TARGET.1 + from.1 - SOURCE.1);
    let [x, y, z] = super::deck_coords(&sim, landing);
    let at_landing = Some(DriveCoord { x, y, z });
    let mut ended = false;
    for frame in 0..200 {
        step(&mut sim, &rules);
        let warping = sim
            .substrate
            .entities
            .get(ship)
            .unwrap()
            .chrono_warp()
            .is_some();
        if !ended && !warping {
            ended = true;
            assert_eq!(chain(&sim, ship), [Ship]);
            assert_eq!(track(&sim), Some((-1, at_landing, at_landing)));
            continue;
        }
        if ended {
            assert_eq!(track(&sim), Some((-1, at_landing, None)), "frame {frame}");
            let entity = sim.substrate.entities.get(ship).unwrap();
            assert!(entity.movement_target.is_none(), "frame {frame}");
            assert_eq!(cell(&sim, ship), landing, "frame {frame}");
        }
    }
    assert!(ended);
}

/// A Chrono Miner on its own Teleport warps on a tank's frames. Arming
/// drops its NavCom (the warp's Teleport has no Marked coordinate), so the
/// NULL destinations of states 5 and 7 return before the Unit setter's
/// Teleporter arm (`0x00741A80..0x00741A9C`), and no Drive comes between.
/// The tank's frames are the native `chrono_process` rows'
/// (`retail_chrono_warp_frames_match_native_process`).
#[test]
fn retail_chrono_warp_runs_an_idle_chrono_miner_on_a_tanks_frames() {
    use LocomotorKind::Teleport;
    let Some((rules, mut sim, americans)) = retail_world() else {
        return;
    };
    let miner = spawn(&mut sim, &rules, "CMIN", "Americans", SOURCE);
    let beside = (SOURCE.0 + 1, SOURCE.1);
    let tank = spawn(&mut sim, &rules, "MTNK", "Americans", beside);
    charge_chronosphere(&mut sim, americans);
    click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
    click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
    let state = |sim: &Simulation, id| {
        let entity = sim.substrate.entities.get(id).unwrap();
        entity.chrono_warp().map(|warp| warp.state())
    };
    let mut ended = false;
    for frame in 1..=80 {
        step(&mut sim, &rules);
        let tank_state = state(&sim, tank);
        assert_eq!(state(&sim, miner), tank_state, "frame {frame}");
        let entity = sim.substrate.entities.get(miner).unwrap();
        assert!(entity.navigation.nav_com.is_none(), "frame {frame}");
        let expected = if tank_state.is_some() {
            vec![Teleport, Teleport]
        } else {
            vec![Teleport]
        };
        assert_eq!(chain(&sim, miner), expected, "frame {frame}");
        ended |= tank_state.is_none();
    }
    assert!(ended);
}

/// A destination given once the warp has landed (WarpingIn up, the latch
/// down) runs the Unit setter's Teleporter arm. The warp's Teleport may not
/// end mid-warp (`0x00719F30`), so a fresh Drive suspends it (`0x0074276F`),
/// but the Drive refuses the destination while its owner warps
/// (`0x004AFD40` reads `+0x1D4`/`+0x1D8`) and ends at the Foot AI tail. The
/// warp then finishes on the landing cell, and state 5's NULL destination
/// drops the NavCom.
#[test]
fn retail_destination_while_warping_in_is_refused_by_the_drive() {
    use LocomotorKind::{Drive, Teleport};
    let Some((rules, mut sim, americans)) = retail_world() else {
        return;
    };
    let miner = spawn(&mut sim, &rules, "CMIN", "Americans", SOURCE);
    charge_chronosphere(&mut sim, americans);
    click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
    click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
    let warping_in = |sim: &Simulation| {
        let entity = sim.substrate.entities.get(miner).unwrap();
        entity.is_warping_in() && !entity.chrono_warp_latch()
    };
    for _ in 0..80 {
        if warping_in(&sim) {
            break;
        }
        step(&mut sim, &rules);
    }
    assert!(warping_in(&sim));
    let next = NavTargetRef::cell(TARGET.0 + 4, TARGET.1);
    sim.set_unit_destination(
        miner,
        next,
        &rules,
        true,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(chain(&sim, miner), [Drive, Teleport, Teleport]);
    assert!(warping_in(&sim));
    let entity = sim.substrate.entities.get(miner).unwrap();
    assert_eq!(entity.navigation.nav_com, Some(next));
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(entity),
        Some(false)
    );

    let mut chains = vec![chain(&sim, miner)];
    for _ in 0..80 {
        step(&mut sim, &rules);
        assert_eq!(cell(&sim, miner), TARGET);
        let now = chain(&sim, miner);
        if chains.last() != Some(&now) {
            chains.push(now);
        }
    }
    assert_eq!(
        chains,
        [
            vec![Drive, Teleport, Teleport],
            vec![Teleport, Teleport],
            vec![Teleport],
        ]
    );
    let entity = sim.substrate.entities.get(miner).unwrap();
    assert!(entity.chrono_warp().is_none() && !entity.is_warping_in());
    assert!(entity.navigation.nav_com.is_none() && entity.movement_target.is_none());
}

/// A Foot standing on the destination is killed when the warp tests the
/// cell (Update_Position `0x007183C8`), and the warp lands there.
#[test]
fn retail_chrono_warp_telefrags_the_unit_at_the_destination() {
    let Some((rules, mut sim, americans)) = retail_world() else {
        return;
    };
    let tank = spawn(&mut sim, &rules, "MTNK", "Americans", SOURCE);
    let victim = spawn(&mut sim, &rules, "HTNK", "Russians", TARGET);
    charge_chronosphere(&mut sim, americans);
    click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
    click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
    for _ in 0..61 {
        step(&mut sim, &rules);
    }
    assert!(!dead(&sim, victim));
    step(&mut sim, &rules);
    assert!(dead(&sim, victim), "killed on the landing frame");
    assert_eq!(cell(&sim, tank), TARGET);
}

/// An object under the Iron Curtain on the destination kills the warping
/// object instead (`0x0071840F`).
#[test]
fn retail_chrono_warp_onto_the_iron_curtain_kills_the_warper() {
    let Some((rules, mut sim, americans)) = retail_world() else {
        return;
    };
    let tank = spawn(&mut sim, &rules, "MTNK", "Americans", SOURCE);
    let curtained = spawn(&mut sim, &rules, "HTNK", "Russians", TARGET);
    let frame = sim.session.binary_frame;
    crate::sim::superweapon::invulnerability::apply_invulnerability(
        sim.substrate.entities.get_mut(curtained).unwrap(),
        frame,
        10_000,
        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
    );
    charge_chronosphere(&mut sim, americans);
    click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
    click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
    for _ in 0..62 {
        step(&mut sim, &rules);
    }
    assert!(dead(&sim, tank));
    assert!(!dead(&sim, curtained));
}

/// A building on the destination blocks it: the warp lands on a nearby
/// cell instead, keeping its offset from the cell (Update_Position's
/// blocked arm `0x007184FA..0x00718658`), and waits out `[General]
/// ChronoDelay=` there.
#[test]
fn retail_blocked_chrono_warp_lands_beside_the_building() {
    let Some((rules, mut sim, americans)) = retail_world() else {
        return;
    };
    let tank = spawn(&mut sim, &rules, "MTNK", "Americans", SOURCE);
    spawn(&mut sim, &rules, "GAPOWR", "Russians", TARGET);
    charge_chronosphere(&mut sim, americans);
    click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
    click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
    while sim
        .substrate
        .entities
        .get(tank)
        .is_some_and(|entity| entity.chrono_warp().is_some())
    {
        step(&mut sim, &rules);
    }
    let entity = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(entity.chrono_warp_delay(), 60);
    let at = cell(&sim, tank);
    let footprint = TARGET.0..TARGET.0 + 2;
    assert!(!(footprint.contains(&at.0) && (TARGET.1..TARGET.1 + 2).contains(&at.1)));
    assert!(
        at.0.abs_diff(TARGET.0) <= 2 && at.1.abs_diff(TARGET.1) <= 2,
        "{at:?}"
    );
    assert!(!dead(&sim, tank));
}

/// The Teleport's Chronosphere states frame by frame against the native
/// `chrono_process` rows: from the first frame after Launch case 4, each
/// frame's state, owner bytes (`+0x270`, `+0x271`, `+0x27C`), timer and
/// ChronoDelay (`+0x284`), the landing frame (Update_Position's placement)
/// and the frame the piggyback ends. A building on the destination makes
/// one blocked Update_Position; the oracle's twice-blocked row needs a
/// nearby cell that blocks too, which this map cannot build.
#[test]
fn retail_chrono_warp_frames_match_native_process() {
    if retail_rules().is_none() {
        return;
    }
    let oracle: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    let int = |value: &serde_json::Value| value.as_i64().unwrap();
    let mut replayed = 0;
    for row in oracle["chrono_process"].as_array().unwrap() {
        let blocks = int(&row["blocks"]);
        if blocks > 1 {
            continue;
        }
        let (mut rules, mut sim, americans) = retail_world().unwrap();
        rules.general.chrono_delay = int(&row["chrono_delay"]) as i32;
        let tank = spawn(&mut sim, &rules, "MTNK", "Americans", SOURCE);
        if blocks == 1 {
            spawn(&mut sim, &rules, "GAPOWR", "Russians", TARGET);
        }
        sim.substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .set_chrono_warp_delay(int(&row["stale_delay"]) as i32);
        charge_chronosphere(&mut sim, americans);
        click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
        click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
        let origin = sim.session.binary_frame as i64;
        let frames = row["frames"].as_array().unwrap();
        let landing = frames
            .iter()
            .find(|frame| {
                frame["calls"].as_array().unwrap().iter().any(|call| {
                    call[3]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|event| event[0] == "set_location")
                })
            })
            .map(|frame| int(&frame["frame"]))
            .unwrap();
        let ended = int(&row["ended"]);
        let mut expected = &frames[0];
        let mut landed = None;
        for frame in 0..=ended {
            step(&mut sim, &rules);
            if let Some(next) = frames.iter().find(|row| int(&row["frame"]) == frame) {
                expected = next;
            }
            let entity = sim.substrate.entities.get(tank).unwrap();
            if landed.is_none() && cell(&sim, tank) != SOURCE {
                landed = Some(frame);
            }
            if frame == ended {
                assert!(entity.chrono_warp().is_none(), "{row}: ended at {frame}");
                break;
            }
            let warp = entity.chrono_warp().expect("still warping");
            let timer = warp.timer();
            let actual = serde_json::json!({
                "state": warp.state(),
                "warped_out": u8::from(entity.is_warped_out()),
                "warping_in": u8::from(entity.is_warping_in()),
                "latched": u8::from(entity.chrono_warp_latch()),
                "timer": [i64::from(timer.start_frame()) - origin, timer.duration()],
                "delay": entity.chrono_warp_delay(),
            });
            for key in [
                "state",
                "warped_out",
                "warping_in",
                "latched",
                "timer",
                "delay",
            ] {
                assert_eq!(
                    actual[key], expected[key],
                    "{key} at frame {frame} of {row}"
                );
            }
        }
        assert_eq!(landed, Some(landing), "{row}");
        replayed += 1;
    }
    assert_eq!(replayed, 4);
}

/// Update_Position (`Simulation::chrono_update_position`) against the native
/// `chrono_update_position` rows, each rebuilt on the retail map: the owner
/// armed by the two clicks, the row's levels, bridge flags, victims, Iron
/// Curtain, OnBridge, Marked and `+0x288`; then the answer, who died, the
/// new `+0x288`, Marked and OnBridge. A blocked row's nearby cell is the one
/// VERA's search picks in that world on this frame (the search draws from
/// its pool by frame), which the oracle's stub answers. Rows this map
/// cannot build are left out: an object on the bridge list, two objects in
/// one cell, a blocked bridge cell's search, and the MovementZone remap
/// (the search's input).
#[test]
fn retail_chrono_update_position_matches_native_rows() {
    if retail_rules().is_none() {
        return;
    }
    let oracle: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    let int = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
    let coord = |value: &serde_json::Value| DriveCoord {
        x: int(&value[0]),
        y: int(&value[1]),
        z: int(&value[2]),
    };
    let mut replayed = 0;
    for row in oracle["chrono_update_position"].as_array().unwrap() {
        let place = row["place"].as_bool().unwrap();
        let objects = row["objects"].as_array().unwrap();
        let cells = row["cells"].as_array().unwrap();
        let mut occupied = std::collections::BTreeSet::new();
        let buildable = int(&row["mz"]) == 0
            && objects.iter().all(|object| object[2] == "ground")
            && objects
                .iter()
                .all(|object| occupied.insert(object[1].to_string()))
            && (place || cells.iter().all(|cell| int(&cell[3]) == 0));
        if !buildable {
            continue;
        }
        let (rules, mut sim, americans) = retail_world().unwrap();
        if let Some(terrain) = sim.resolved_terrain.as_mut() {
            for cell in cells {
                let index = terrain
                    .index(int(&cell[0]) as u16, int(&cell[1]) as u16)
                    .unwrap();
                terrain.cells[index].level = int(&cell[2]) as u8;
                terrain.cells[index].bridge_facts.raw_flags = int(&cell[3]) as u32;
            }
        }
        sim.zone_grid = None;
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        let owner_type = if row["owner"] == "infantry" {
            "CLEG"
        } else {
            "MTNK"
        };
        let owner = spawn(&mut sim, &rules, owner_type, "Americans", SOURCE);
        let mut victims = Vec::new();
        for object in objects {
            let name = object[0].as_str().unwrap();
            let at = (int(&object[1][0]) as u16, int(&object[1][1]) as u16);
            let id = spawn(&mut sim, &rules, name, "Russians", at);
            // Only an infantryman's coordinate enters the test.
            if int(&object[3]) == 0xF {
                let entity = sim.substrate.entities.get(id).unwrap();
                assert_eq!(
                    crate::sim::movement::ground_pose::object_get_coords(
                        entity,
                        sim.resolved_terrain.as_ref()
                    ),
                    coord(&object[6]),
                    "{name} of {row}"
                );
            }
            if object[5].as_bool().unwrap() {
                let frame = sim.session.binary_frame;
                crate::sim::superweapon::invulnerability::apply_invulnerability(
                    sim.substrate.entities.get_mut(id).unwrap(),
                    frame,
                    10_000,
                    crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
                );
            }
            victims.push((name, id));
        }
        charge_chronosphere(&mut sim, americans);
        click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
        click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
        let target = coord(&row["coord"]);
        let entity = sim.substrate.entities.get_mut(owner).unwrap();
        entity.on_bridge = row["on_bridge"].as_bool().unwrap();
        let runtime = entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.teleport_runtime_mut())
            .unwrap();
        runtime.chrono_mut().unwrap().set_destination(target);
        if !row["marked"].is_null() {
            runtime.set_resolved_destination(coord(&row["marked"]));
        }

        let answer = sim.chrono_update_position(
            owner,
            target,
            place,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );

        assert_eq!(u8::from(answer), int(&row["result"]) as u8, "{row}");
        let killed: Vec<&str> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event[0] == "damage")
            .map(|event| event[1].as_str().unwrap())
            .collect();
        assert_eq!(
            dead(&sim, owner),
            killed.contains(&"owner"),
            "owner of {row}"
        );
        for (name, id) in victims {
            assert_eq!(dead(&sim, id), killed.contains(&name), "{name} of {row}");
        }
        let entity = sim.substrate.entities.get(owner).unwrap();
        let runtime = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.teleport_runtime())
            .unwrap();
        assert_eq!(
            runtime.chrono().unwrap().destination(),
            coord(&row["destination_after"]),
            "+0x288 of {row}"
        );
        assert_eq!(
            runtime
                .resolved_destination()
                .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 }),
            coord(&row["marked_after"]),
            "Marked of {row}"
        );
        assert_eq!(
            u8::from(entity.on_bridge),
            int(&row["on_bridge_after"]) as u8,
            "OnBridge of {row}"
        );
        replayed += 1;
    }
    assert_eq!(replayed, 14);
}

/// Launch case 4's destination (`+0x288`) against the native
/// `chrono_destination` rows: a Grizzly (Unit) or a Chrono Legionnaire in
/// the source block, on the row's levels and bridge flags, warped from
/// (21, 21) to (40, 40).
#[test]
fn retail_chrono_warp_destinations_match_native_rows() {
    if retail_rules().is_none() {
        return;
    }
    let oracle: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    let int = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
    let coord = |value: &serde_json::Value| DriveCoord {
        x: int(&value[0]),
        y: int(&value[1]),
        z: int(&value[2]),
    };
    let rows = oracle["chrono_destination"].as_array().unwrap();
    for row in rows {
        assert_eq!(
            (int(&row["src"][0]), int(&row["src"][1])),
            (i32::from(SOURCE.0), i32::from(SOURCE.1))
        );
        assert_eq!(
            (int(&row["target"][0]), int(&row["target"][1])),
            (i32::from(TARGET.0), i32::from(TARGET.1))
        );
        let (rules, mut sim, americans) = retail_world().unwrap();
        if let Some(terrain) = sim.resolved_terrain.as_mut() {
            for cell in row["cells"].as_array().unwrap() {
                let index = terrain
                    .index(int(&cell[0]) as u16, int(&cell[1]) as u16)
                    .unwrap();
                terrain.cells[index].level = int(&cell[2]) as u8;
                terrain.cells[index].bridge_facts.raw_flags = int(&cell[3]) as u32;
            }
        }
        sim.zone_grid = None;
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        let infantry = int(&row["what"]) == 0xF;
        let at = (
            (i32::from(SOURCE.0) + int(&row["offset"][0])) as u16,
            (i32::from(SOURCE.1) + int(&row["offset"][1])) as u16,
        );
        let id = spawn(
            &mut sim,
            &rules,
            if infantry { "CLEG" } else { "MTNK" },
            "Americans",
            at,
        );
        // Only a non-Unit's own coordinate enters the destination.
        if infantry {
            let entity = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                crate::sim::movement::ground_pose::object_get_coords(
                    entity,
                    sim.resolved_terrain.as_ref()
                ),
                coord(&row["coords"]),
                "{row}"
            );
        }
        charge_chronosphere(&mut sim, americans);
        click(&mut sim, &rules, americans, "ChronoSphereSpecial", SOURCE);
        click(&mut sim, &rules, americans, "ChronoWarpSpecial", TARGET);
        let warp = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.chrono_warp())
            .unwrap_or_else(|| panic!("warping: {row}"));
        assert_eq!(warp.destination(), coord(&row["destination"]), "{row}");
    }
    assert_eq!(rows.len(), 8);
}
