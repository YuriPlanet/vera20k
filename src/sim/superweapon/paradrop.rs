//! ParaDrop / AmerParaDrop superweapon launch handler.
//!
//! Mirrors gamemd.exe SuperClass::Launch cases 5 (ParaDrop, side-branched on
//! HouseClass.Side) and 6 (AmerParaDrop, always-American config).
//!
//! Per-side branch picks an infantry list from rules; for each (inf_type, num)
//! entry, spawns one PDPLANE at the house's waypoint edge with `num` limbo
//! infantry loaded as cargo. The carrier's initial Rust mission is
//! ParaDropApproach, which models the stock Mission_Open superweapon path.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/aircraft, sim/movement,
//!   sim/passenger, sim/pathfinding, sim/world.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::AircraftMission;
use crate::sim::intern::InternedId;
use crate::sim::passenger::PassengerRole;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::edge_cell::{Edge, find_paradrop_edge_cell};
use crate::sim::world::{PlacementEvidence, SimSoundEvent, Simulation};
use crate::util::fixed_math::{SimFixed, ra2_speed_to_leptons_per_second};

#[derive(Debug, Clone, Copy)]
pub enum ParaDropKind {
    /// Type=ParaDrop — side-branched on HouseClass.side_index.
    Generic,
    /// Type=AmerParaDrop — always uses the AmerParaDropList.
    American,
}

/// Launch entry point. Returns true if at least one carrier was spawned.
pub fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    target_rx: u16,
    target_ry: u16,
    kind: ParaDropKind,
    sw_type: InternedId,
    _path_grid: Option<&PathGrid>,
) -> bool {
    // Bridge rejection deferred — map system does not yet expose is_bridge_cell.
    let (target_rx, target_ry) = (target_rx, target_ry);

    // Pick the per-side infantry list.
    let list: Vec<(String, u32)> = match kind {
        ParaDropKind::American => rules.general.amer_paradrop_list.clone(),
        ParaDropKind::Generic => {
            let side = sim.houses.get(&owner).map_or(0, |h| h.side_index);
            match side {
                0 => rules.general.ally_paradrop_list.clone(),
                2 => rules.general.yuri_paradrop_list.clone(),
                _ => rules.general.sov_paradrop_list.clone(), // Soviet fallback
            }
        }
    };

    if list.is_empty() {
        log::warn!(
            "Paradrop launch by '{}': per-side list is empty; aborting",
            sim.interner.resolve(owner),
        );
        return false;
    }

    // Resolve the carrier's spawn edge cell.
    let waypoint_edge_idx = sim.houses.get(&owner).map_or(0, |h| h.waypoint_edge);
    let (edge, resolved_edge_idx) = match Edge::from_index(waypoint_edge_idx) {
        Some(e) => (e, waypoint_edge_idx),
        None => {
            log::warn!(
                "Paradrop launch: invalid waypoint_edge {}; falling back to north edge",
                waypoint_edge_idx
            );
            (Edge::North, 0)
        }
    };

    // Launch notification for the app layer. `SuperClass::Launch @ 0x006CC390`
    // cases 5 (`ParaDrop`) and 6 (`AmerParaDrop`) play **no** cue and **no**
    // EVA line — neither case body reaches a `VocClass` or `VoxClass` call —
    // so the app resolves this to silence. The event is still emitted so every
    // launch reaches one mapping site.
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx: target_rx,
        ry: target_ry,
    });

    // YR linked-aircraft paradrop dispatch derives its facing from edge*2 << 13.
    let launch_facing_word = crate::sim::aircraft::runtime_contract::paradrop_edge_facing_word(
        resolved_edge_idx.into(),
        false,
    );
    let launch_facing = (launch_facing_word >> 8) as u8;

    // Spawn one PDPLANE per (inf_type, num) entry.
    let mut spawned_any = false;
    for (inf_type_name, num) in list {
        if spawn_pdplane(
            sim,
            rules,
            owner,
            edge,
            launch_facing,
            target_rx,
            target_ry,
            &inf_type_name,
            num,
        ) {
            spawned_any = true;
        }
    }
    spawned_any
}

fn spawn_pdplane(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    edge: Edge,
    launch_facing: u8,
    target_rx: u16,
    target_ry: u16,
    inf_type: &str,
    num: u32,
) -> bool {
    let owner_str = sim.interner.resolve(owner).to_string();
    let pdplane_type = rules.general.paradrop_aircraft_type.clone();

    // Active FUN_0065E660 constructs each carrier before its own edge-helper
    // draw, then Unlimbos that retained identity. Standard list entries must
    // not share one precomputed edge or reverse constructor/RNG ordering.
    let pdplane_id = match sim.construct_object_limbo_at_height(
        &pdplane_type,
        &owner_str,
        0,
        0,
        launch_facing,
        /*z*/ 0,
        rules,
    ) {
        Some(id) => id,
        None => {
            log::warn!(
                "Paradrop spawn: failed to construct carrier '{}'",
                pdplane_type,
            );
            return false;
        }
    };

    let edge_cell = find_paradrop_edge_cell(
        sim.playfield_bounds,
        sim.resolved_terrain.as_ref(),
        edge,
        &mut sim.scenario_rng,
    );
    let Some(edge_cell) = edge_cell else {
        let _ = sim.discard_constructed_limbo(pdplane_id);
        log::warn!("Paradrop spawn: native edge helper lacks MapClass authority");
        return false;
    };

    // Jump straight to cruise altitude AND install a cargo hold sized for the
    // paradrop payload. Vanilla PDPLANE has no Passengers= key, so spawn_object
    // doesn't initialize a Transport cargo — gamemd's spawner builds the
    // CargoClass linked list directly. Mirror that here.
    // Aircraft Unlimbo414383 uses Type virtual+BC, including a per-type
    // override. Resolve the rule itself rather than copying mutable flight
    // controller state (whose target can also represent a dive or landing).
    let flight_level = rules
        .object(&pdplane_type)
        .expect("constructed paradrop carrier has a rules type")
        .flight_level(rules.general.flight_level);
    if let Some(entity) = sim.substrate.entities.get_mut(pdplane_id) {
        if let Some(loco) = entity.locomotor.as_mut() {
            loco.altitude = SimFixed::saturating_from_num(flight_level);
            loco.set_fly_target_height(flight_level);
        }
        if !entity.passenger_role.is_transport() {
            entity.passenger_role = PassengerRole::Transport {
                cargo: crate::sim::passenger::PassengerCargo::new(num, /*size_limit*/ 0),
            };
        }
        entity.aircraft_mission = Some(AircraftMission::ParaDropApproach {
            target_rx,
            target_ry,
            has_revealed_fog: false,
        });
    }

    // FUN_0065E660 installs the carrier mission and destination before
    // AircraftClass::Unlimbo. This mutates only the retained carrier's
    // MovementTarget/Fly locomotor target; passenger constructors remain
    // strictly after successful Unlimbo.
    // No FASTER stage: the carrier flies, and the fly locomotor never calls the
    // `FootClass::GetCurrentSpeed` slot (`veterancy::locomotor_consults_current_speed`).
    let speed = rules
        .object(&pdplane_type)
        .map(|o| ra2_speed_to_leptons_per_second(o.speed.max(1)))
        .unwrap_or(SimFixed::from_num(8));
    sim.issue_air_cell_destination(pdplane_id, (target_rx, target_ry), speed, Some(rules));

    // Criterion 4 intentionally chooses the first cell just outside the
    // isometric playfield. AircraftClass Unlimbo accepts that map-edge spawn;
    // the ordinary Object Reveal inside-playfield gate is not the admission
    // oracle for this verified call path.
    if sim
        .reveal_constructed_object_at_height(
            pdplane_id,
            edge_cell.0,
            edge_cell.1,
            launch_facing,
            0,
            PlacementEvidence::MarkSucceeded,
            rules,
        )
        .is_none()
    {
        let _ = sim.discard_constructed_limbo(pdplane_id);
        log::warn!(
            "Paradrop spawn: carrier '{}' rejected edge ({},{})",
            pdplane_type,
            edge_cell.0,
            edge_cell.1,
        );
        return false;
    }

    // Load N limbo-created infantry into cargo as Inside passengers.
    let inf_size = rules.object(inf_type).map(|o| o.size).unwrap_or(1);
    let mut loaded = 0u32;
    for _ in 0..num {
        let pax_id = match sim.spawn_object_limbo_at_height(
            inf_type,
            &owner_str,
            edge_cell.0,
            edge_cell.1,
            /*facing*/ 0,
            /*z*/ 0,
            rules,
        ) {
            Some(id) => id,
            None => break,
        };
        if let Some(pax) = sim.substrate.entities.get_mut(pax_id) {
            pax.passenger_role = PassengerRole::Inside {
                transport_id: pdplane_id,
            };
        }
        let boarded = sim
            .substrate
            .entities
            .get_mut(pdplane_id)
            .and_then(|a| a.passenger_role.cargo_mut())
            .map(|c| {
                c.board_forced(pax_id, inf_size);
                true
            })
            .unwrap_or(false);
        if !boarded {
            // No hold - give up; the partial cargo flies.
            break;
        }
        loaded += 1;
    }

    if loaded == 0 {
        // No passengers loaded — kill the empty carrier rather than fly empty.
        let infantry_terminal = sim.begin_raw_infantry_death(pdplane_id, None);
        if !infantry_terminal && let Some(entity) = sim.substrate.entities.get_mut(pdplane_id) {
            entity.health.current = 0;
            entity.dying = true;
        }
        return false;
    }

    log::info!(
        "Paradrop: spawned '{}' for '{}' carrying {} '{}' at edge ({},{}) → target ({},{})",
        pdplane_type,
        owner_str,
        loaded,
        inf_type,
        edge_cell.0,
        edge_cell.1,
        target_rx,
        target_ry,
    );
    true
}
