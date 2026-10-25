//! Stock SCHP Move -> landing -> real SCHPDEPL -> reverse completion.
//! UnitUndeploy739D4F..739D62 consumes even an exhausted Nearby search.

use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainCell, test_grid};
use crate::rules::terrain_rules::LandType;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::scenario_session::ScenarioGameMode;

const START: (u16, u16) = (48, 56);

fn frame(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    assert!(
        sim.advance_tick(&commands, Some(rules), None, None, 33)
            .frame_committed
    );
}

fn altitude(sim: &Simulation, id: u64) -> i32 {
    crate::sim::movement::air_movement::current_fly_height(
        sim.substrate.entities.get(id).expect("live stock SCHP"),
        sim.resolved_terrain.as_ref(),
    )
}

fn until(
    sim: &mut Simulation,
    rules: &RuleSet,
    budget: usize,
    label: &str,
    reached: impl Fn(&Simulation) -> bool,
) {
    for _ in 0..budget {
        if reached(sim) {
            return;
        }
        frame(sim, rules);
    }
    panic!(
        "did not reach {label}; frame={} actors={:?}",
        sim.session.binary_frame(),
        sim.substrate
            .entities
            .values()
            .map(|e| (
                sim.interner.resolve(e.type_ref()),
                e.position,
                e.mission.current(),
                e.is_fully_deployed(),
                e.unit_deploying(),
                e.landing_for_deploy(),
                e.navigation.nav_com(),
                e.attack_target.target()
            ))
            .collect::<Vec<_>>()
    );
}

fn order(sim: &mut Simulation, rules: &RuleSet, id: u64, command: Command) {
    let owner = sim.substrate.entities.get(id).unwrap().owner();
    sim.queue_command(CommandEnvelope::new(owner, sim.session.tick() + 1, command));
    frame(sim, rules);
}

fn scene(extra_clear: bool) -> Option<(Simulation, RuleSet, u64)> {
    let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
    let anim = rules
        .object("SCHP")
        .unwrap()
        .deploying_anim
        .clone()
        .unwrap();
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_scenario_assets().unwrap();
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    rules.bind_anim_class_assets(&[anim.clone()], &assets, "TEM", "TEMPERATE");
    let config = rules.art().anim_runtime_config(&anim).unwrap();
    assert_eq!(
        (
            config.raw_shp_frame_count,
            config.end,
            config.rate_logic_frames
        ),
        (Some(22), 11, 9)
    );
    let object = rules.object("SCHP").unwrap();
    assert!(object.is_simple_deployer && object.deploy_to_land);
    assert!(
        object.passengers == 0
            && !object.harvester
            && !object.weeder
            && object.deploys_into.is_none()
    );
    let clear = rules
        .terrain_rules
        .semantics_for_land_type(LandType::Clear.as_index())
        .unwrap();
    let rock = rules
        .terrain_rules
        .semantics_for_land_type(LandType::Rock.as_index())
        .unwrap();
    assert_eq!(rock.speed_costs.hover, Some(0));
    assert!(clear.speed_costs.hover.unwrap() > 0);
    let mut sim = Simulation::with_seed(0x739d62);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().short_game = false;
    sim.session.set_map_width_for_test(96);
    sim.session.set_map_height_for_test(96);
    for name in ["Russians", "Americans"] {
        let owner = sim.interner.intern(name);
        let side = rules.country_side_index(name).unwrap().0;
        let mut house = HouseState::new(owner, side, Some(owner), true, 0, 10);
        house.project_country_mults(&rules, &sim.interner);
        sim.houses.register(house);
    }
    sim.session
        .set_current_house_for_test(sim.interner.get("Russians"));
    sim.fog = crate::sim::vision::FogState::new(96, 96);
    sim.overlay_grid = Some(OverlayGrid::new(96, 96));
    let clear_cells = [START, (84, 84), (86, 86), (88, 84)];
    sim.install_resolved_terrain_for_new_map(test_grid(96, 96, |x, y| {
        let is_clear = clear_cells.contains(&(x, y)) || (extra_clear && (x, y) == (49, 56));
        let land = if is_clear {
            LandType::Clear
        } else {
            LandType::Rock
        };
        let semantics = if is_clear { clear } else { rock };
        ResolvedTerrainCell {
            land_type: land.as_index(),
            base_land_type: land.as_index(),
            speed_costs: semantics.speed_costs,
            base_speed_costs: semantics.speed_costs,
            ground_walk_blocked: semantics.ground_blocked,
            base_ground_walk_blocked: semantics.ground_blocked,
            base_build_blocked: !semantics.buildable,
            zone_type: crate::map::resolved_terrain::recalc_zone_type(
                false,
                None,
                land.as_index(),
                semantics.speed_costs.wheel,
                None,
            ),
            ..crate::map::resolved_terrain::test_flat_cell(x, y)
        }
    }));
    crate::sim::arena_fixture::supply_native_map_keyed(
        &mut sim,
        ("retail SCHP boxed undeploy", extra_clear),
    );
    sim.set_input_delay_ticks(0);
    assert!(crate::map::playfield::size_diamond_contains(
        96,
        96,
        (48, 56)
    ));
    let id = sim
        .spawn_object("SCHP", "Russians", START.0, START.1, 0, &rules)
        .unwrap();
    sim.spawn_object("SMCV", "Russians", 86, 86, 0, &rules)
        .unwrap();
    sim.spawn_object("AMCV", "Americans", 88, 84, 0, &rules)
        .unwrap();
    assert_eq!(altitude(&sim, id), 0);
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_bits(START.0, START.1)
            & 0x20,
        0x20
    );
    Some((sim, rules, id))
}

fn deployed(extra_clear: bool) -> Option<(Simulation, RuleSet, u64, u64)> {
    let (mut sim, rules, id) = scene(extra_clear)?;
    order(
        &mut sim,
        &rules,
        id,
        Command::Move {
            entity_id: id,
            target_rx: 84,
            target_ry: 84,
        },
    );
    until(
        &mut sim,
        &rules,
        120,
        "takeoff clearing the initial ground cell",
        |sim| {
            altitude(sim, id) > 0
                && sim
                    .substrate
                    .raw_cell_occupation
                    .ground_bits(START.0, START.1)
                    & 0x20
                    == 0
        },
    );
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(
        (i32::from(e.position.rx()) - i32::from(START.0)).abs() < 32
            && (i32::from(e.position.ry()) - i32::from(START.1)).abs() < 32,
        "the vacated Clear cell must remain in the actual Stop search"
    );
    eprintln!(
        "takeoff frame={} cell=({}, {}) height={}",
        sim.session.binary_frame(),
        e.position.rx(),
        e.position.ry(),
        altitude(&sim, id)
    );
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_bits(START.0, START.1)
            & 0x20,
        0
    );
    order(&mut sim, &rules, id, Command::DeployMcv { entity_id: id });
    until(&mut sim, &rules, 120, "ordinary landing request", |sim| {
        sim.substrate.entities.get(id).unwrap().landing_for_deploy()
    });
    until(
        &mut sim,
        &rules,
        400,
        "forward deployment animation",
        |sim| {
            let e = sim.substrate.entities.get(id).unwrap();
            e.unit_deploying() && e.deploy_anim().is_some()
        },
    );
    let forward = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .deploy_anim()
        .unwrap();
    assert_eq!(altitude(&sim, id), 0);
    assert_eq!(sim.anim(forward).unwrap().owner_entity(), Some(id));
    assert!(!sim.anim(forward).unwrap().runtime().constructor_reverse);
    until(&mut sim, &rules, 400, "fully deployed", |sim| {
        let e = sim.substrate.entities.get(id).unwrap();
        e.is_fully_deployed() && !e.unit_deploying()
    });
    until(&mut sim, &rules, 400, "forward attachment expiry", |sim| {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .deploy_anim()
            .is_none()
    });
    assert!(sim.anim(forward).is_none());
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(e.lifecycle.object_alive() && !e.lifecycle.in_limbo());
    assert_eq!(altitude(&sim, id), 0);
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_bits(e.position.rx(), e.position.ry())
            & 0x20,
        0x20
    );
    assert_eq!(e.navigation.nav_com(), None);
    assert_eq!(e.attack_target.target(), None);
    let nearby = sim.techno_nearby_location(id, Some(id), &rules);
    assert_eq!(
        nearby.is_some(),
        extra_clear,
        "actual stock Hover search result {nearby:?}"
    );
    Some((sim, rules, id, forward))
}

fn assert_reverse_lifts(extra_clear: bool) {
    let Some((mut sim, rules, id, forward)) = deployed(extra_clear) else {
        return;
    };
    order(&mut sim, &rules, id, Command::DeployMcv { entity_id: id });
    until(
        &mut sim,
        &rules,
        120,
        "reverse deployment attachment",
        |sim| {
            let e = sim.substrate.entities.get(id).unwrap();
            e.unit_deploying() && e.deploy_anim().is_some()
        },
    );
    let reverse = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .deploy_anim()
        .unwrap();
    assert_ne!(reverse, forward);
    assert!(sim.anim(reverse).unwrap().runtime().constructor_reverse);
    assert_eq!(sim.anim(reverse).unwrap().owner_entity(), Some(id));
    until(
        &mut sim,
        &rules,
        400,
        "first completed reverse frame",
        |sim| {
            let e = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                e.attack_target.target(),
                None,
                "Guard must not mask the missing handoff"
            );
            !e.is_fully_deployed() && !e.unit_deploying()
        },
    );
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(e.lifecycle.object_alive() && !e.lifecycle.in_limbo());
    assert!(sim.houses.values().all(|house| !house.is_defeated()));
    let jumpjet = e.locomotor.as_ref().unwrap().jumpjet_runtime().unwrap();
    eprintln!(
        "reverse completion frame={} extra_clear={extra_clear} height={} nav={:?} jumpjet={jumpjet:?}",
        sim.session.binary_frame(),
        altitude(&sim, id),
        e.navigation.nav_com()
    );
    assert!(
        jumpjet.moving(),
        "ordinary reverse completion must consume the real Nearby result, including failure"
    );
    until(
        &mut sim,
        &rules,
        120,
        "positive height after reverse completion",
        |sim| {
            assert_eq!(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .attack_target
                    .target(),
                None
            );
            altitude(sim, id) > 0
        },
    );
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .lifecycle
            .object_alive()
    );
}

#[test]
fn retail_siege_chopper_lifts_after_animated_undeploy_with_no_nearby_cell() {
    assert_reverse_lifts(false);
}

#[test]
fn retail_siege_chopper_lifts_after_animated_undeploy_with_a_free_nearby_cell() {
    assert_reverse_lifts(true);
}
