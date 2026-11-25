//! Retail Engineer/FV cargo selection composed with normal friendly repair input.
//! Supplied flat-map placement and target damage; no mouse picking or window capture.

use super::*;
use crate::app::input::cursor::{capability_cursor_for_hover, cursor_id_for_feedback};
use crate::app::types::{CursorFeedbackKind, CursorId};
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::world::UnitRepairAction;
use crate::sim::world::{FrameEffects, Simulation};

const OWNER: &str = "Americans";

pub(in crate::app) fn boarded_ifv(rules: &RuleSet, passenger_type: &str) -> (Simulation, u64, u64) {
    let mut sim = Simulation::new();
    for (name, side) in [(OWNER, 0), ("Russians", 1)] {
        let owner = sim.intern(name);
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, side, None, true, 0, 10));
    }
    let house = sim.interner().get(OWNER);
    sim.session_mut_for_test().set_current_house_for_test(house);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    // The arena supplies terrain/navigation. Real Spark ground-contact
    // queries also need the map's mutable overlay owner, even on clear cells.
    *sim.overlay_grid_mut_for_test() = Some(crate::sim::overlay_grid::OverlayGrid::new(
        sim.session().map_width(),
        sim.session().map_height(),
    ));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let ifv = sim.spawn_object("FV", OWNER, 16, 16, 64, rules).unwrap();
    let engineer = sim
        .spawn_object(passenger_type, OWNER, 16, 15, 64, rules)
        .unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::clicked_mission(
            engineer,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(ifv)
        ),
        Some(rules)
    ));
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..240 {
        if sim
            .entities()
            .get(engineer)
            .unwrap()
            .passenger_role()
            .is_inside_transport()
        {
            break;
        }
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
    }
    assert!(
        sim.entities()
            .get(engineer)
            .unwrap()
            .passenger_role()
            .inside_transport_id()
            == Some(ifv)
    );
    let tank = sim.spawn_object("MTNK", OWNER, 17, 16, 64, rules).unwrap();
    sim.entity_mut_for_test(tank)
        .unwrap()
        .health
        .set_for_test(100);
    (sim, ifv, tank)
}

pub(in crate::app) fn repair_ifv(rules: &RuleSet) -> (Simulation, u64, u64) {
    let (sim, ifv, tank) = boarded_ifv(rules, "ENGINEER");
    assert_eq!(sim.entities().get(ifv).unwrap().current_weapon_number(), 1);
    assert_eq!(
        crate::sim::combat::combat_weapon::weapon_damage_value(
            sim.entities().get(ifv).unwrap(),
            rules.object("FV").unwrap(),
            sim.entities(),
            rules,
            sim.interner(),
        ),
        -50
    );
    (sim, ifv, tank)
}

/// UnitClass::What_Action_OnCell (`0x007404FD..0x00740522`): an IFV carrying
/// an Engineer heals, so Ctrl on the ground answers AttackSupport (0x3B), not
/// Attack, and Foot's ClickedAction_Cell (`0x004D7D50`, default case
/// `0x004D8237`) sends nothing for it. With a GI aboard it fires at the ground.
#[test]
fn retail_engineer_ifv_ctrl_on_the_ground_orders_nothing() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let ground = crate::sim::combat::TargetKind::Cell(16, 19);
    let (sim, ifv, _) = repair_ifv(rules);
    assert_eq!(
        attack_action(&sim, rules, ifv, &ground, true, Some(&registry)),
        AttackAction::AttackSupport
    );
    let prior = sim.state_hash();
    let click = attack_branch_orders(
        &sim,
        rules,
        &[ifv],
        ground,
        true,
        OrderMode::Move,
        &registry,
    );
    assert!(click.consumed && !click.select_fallback);
    assert_eq!(click.receivers, [ifv].into_iter().collect());
    assert!(click.commands.is_empty(), "{:?}", click.commands);
    assert_eq!(
        sim.state_hash(),
        prior,
        "input must not mutate gameplay/RNG"
    );

    let (sim, ifv, _) = boarded_ifv(rules, "E1");
    assert_eq!(
        attack_action(&sim, rules, ifv, &ground, true, Some(&registry)),
        AttackAction::Attack
    );
    assert_eq!(
        attack_branch_orders(
            &sim,
            rules,
            &[ifv],
            ground,
            true,
            OrderMode::Move,
            &registry
        )
        .commands,
        [Command::ForceAttackCell {
            attacker_id: ifv,
            target_rx: 16,
            target_ry: 19,
        }]
    );
}

/// UnitClass::What_Action_OnObject's healer arm (`0x0073FD7C`):
/// Is_Ally_ByObject (`0x004F9A90`) refuses a tree's null House, so an
/// Engineer IFV's Ctrl over it answers 0, which the Unit tail makes NoMove
/// (`0x007403EB`): the click moves it near the tree. A GI's IFV attacks it.
#[test]
fn retail_engineer_ifv_ctrl_on_a_tree_moves_near_it() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let with_tree = |(mut sim, ifv, _): (Simulation, u64, u64)| {
        assert_eq!(
            crate::sim::terrain_spawn::construct_terrain_objects(
                &mut sim,
                &[crate::map::overlay::TerrainObject {
                    rx: 16,
                    ry: 20,
                    name: "TREE01".into(),
                }],
                rules,
                false,
            ),
            1
        );
        let tree = sim.terrain_objects().id_at((16, 20)).unwrap();
        (sim, ifv, crate::sim::combat::TargetKind::Entity(tree))
    };
    let (sim, ifv, tree) = with_tree(repair_ifv(rules));
    assert_eq!(
        attack_action(&sim, rules, ifv, &tree, true, Some(&registry)),
        AttackAction::NoMove
    );
    let click = attack_branch_orders(&sim, rules, &[ifv], tree, true, OrderMode::Move, &registry);
    assert!(click.consumed);
    assert!(
        matches!(
            click.commands.as_slice(),
            [Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. }]
                if *entity_id == ifv
        ),
        "{:?}",
        click.commands
    );

    let (sim, ifv, tree) = with_tree(boarded_ifv(rules, "E1"));
    assert_eq!(
        attack_action(&sim, rules, ifv, &tree, true, Some(&registry)),
        AttackAction::Attack
    );
}

fn repair_click(sim: &Simulation, rules: &RuleSet, ifv: u64, tank: u64) -> Command {
    let owner = sim.entities().get(ifv).unwrap().owner();
    let prior = sim.state_hash();
    let dispatch = unit_repair_branch_orders(
        sim,
        rules,
        &[ifv],
        &[ifv],
        &HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: tank,
        },
        owner,
        OrderModifier::Normal,
        OrderMode::Move,
        false,
    );
    assert!(dispatch.consumed && !dispatch.select_fallback);
    assert_eq!(dispatch.receivers, [ifv].into_iter().collect());
    assert_eq!(dispatch.commands.len(), 1);
    assert!(
        matches!(dispatch.commands[0], Command::Attack { attacker_id, target_id }
        if attacker_id == ifv && target_id == tank)
    );
    assert_eq!(
        sim.state_hash(),
        prior,
        "input must not mutate gameplay/RNG"
    );
    dispatch.commands.into_iter().next().unwrap()
}

/// Under the attack-move chord, ClickedMission promotes the repair click's
/// Attack too (`0x006FFC09..0x006FFC1E`) and keeps the friend as the event
/// target, which `Assign_Target_Command` stores as MegaTarget.
#[test]
fn retail_repair_click_under_the_attack_move_chord_keeps_the_friend_as_target() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (sim, ifv, tank) = repair_ifv(rules);
    let owner = sim.entities().get(ifv).unwrap().owner();
    let dispatch = unit_repair_branch_orders(
        &sim,
        rules,
        &[ifv],
        &[ifv],
        &HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: tank,
        },
        owner,
        OrderModifier::Normal,
        OrderMode::AttackMove,
        false,
    );
    assert_eq!(
        dispatch.commands,
        [Command::AttackMove {
            entity_id: ifv,
            target: Some(TargetKind::Entity(tank)),
            destination: None,
        }]
    );
}

#[test]
fn retail_engineer_ifv_offers_normal_repair_cursor_on_damaged_friend() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (sim, ifv, tank) = repair_ifv(&retail.rules);
    let prior = sim.state_hash();
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyUnit,
        stable_id: tank,
    };
    let feedback = capability_cursor_for_hover(
        &sim,
        &[ifv],
        Some(ifv),
        &hover,
        Some(&retail.rules),
        None,
        crate::app::input::context_order::OrderModifier::Normal,
        false,
        false,
        &mut Default::default(),
    );
    assert_eq!(feedback, CursorFeedbackKind::RepairAction(true));
    assert_eq!(cursor_id_for_feedback(feedback), Some(CursorId::Repair));
    assert_eq!(
        sim.state_hash(),
        prior,
        "a hover must not mutate gameplay or RNG"
    );
}

#[test]
fn retail_berserk_engineer_ifv_keeps_cursor_but_refuses_click() {
    for ares_way in [false, true] {
        let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules;
        let (mut sim, ifv, tank) = repair_ifv(rules);
        let already_sent = repair_click(&sim, rules, ifv, tank);
        sim.entity_mut_for_test(ifv)
            .unwrap()
            .berserk
            .set_active_for_test(true);
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/bunker_attack_action.json",
        ))
        .unwrap();
        let control = native["repair_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "berserk_friend")
            .unwrap();
        assert_eq!(control["action"], 29);
        assert_eq!(control["cursor"]["output"]["cursor_row"], 33);
        assert!(!control["trace"].to_string().contains("command_sink"));
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: tank,
        };
        let prior = sim.state_hash();
        let feedback = capability_cursor_for_hover(
            &sim,
            &[ifv],
            Some(ifv),
            &hover,
            Some(rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
            &mut Default::default(),
        );
        assert_eq!(feedback, CursorFeedbackKind::RepairAction(true));
        assert_eq!(cursor_id_for_feedback(feedback), Some(CursorId::Repair));
        // DetermineAction still answers Repair29, which dispatches (the click is
        // consumed and the action-line timer starts), but Unit738890 refuses the
        // berserk IFV, so it sends nothing.
        assert!(
            sim.click_order_receivers(&[ifv], ares_way).is_empty(),
            "native Unit738890 refuses berserk clicks"
        );
        let owner = sim.entities().get(ifv).unwrap().owner();
        assert_eq!(
            berserk_selection_click(&sim, rules, &[ifv], Some(&hover), (0, 0), owner),
            LeftRelease::Dispatch
        );
        // Over itself the healer arm answers Select (`0x0073FE3B`), since the
        // berserk base answer (0) skips the self action.
        let over_itself = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: ifv,
        };
        assert_eq!(
            berserk_selection_click(&sim, rules, &[ifv], Some(&over_itself), (0, 0), owner),
            LeftRelease::Select
        );
        assert_eq!(sim.state_hash(), prior);
        // The refusal belongs to ClickedAction, not Event::Execute. An event
        // already published before the actor became berserk is still received.
        assert!(sim.apply_command(OWNER, &already_sent, Some(rules)));
        assert_eq!(
            sim.entities().get(ifv).unwrap().attack_target.target(),
            Some(TargetKind::Entity(tank))
        );
    }
}

/// A repair IFV being erased by a Chrono Legionnaire keeps the Repair cursor,
/// and native's click still sends the order: the selection loop
/// (`0x004AE829..0x004AE844`), Unit ClickedAction_Object (`0x00738890`),
/// Foot `0x004D74E0`'s Attack case (`0x004D769F`), ClickedMission
/// (`0x006FFBE0`) and the MEGAMISSION arm of Event::Execute
/// (`0x004C71F6..0x004C74BA`, with Queue_Mission `0x005B35E0` and SetTarget
/// `0x006FCDB0`) never read BeingWarpedOut (vt+0x1D4). The event queues
/// Attack and sets the target; the next frozen Unit AI frame drops it
/// (`0x00736321..0x00736331`, vt+0x3C8(0)) and returns (`0x0073635A`) before
/// MissionClass::AI, so the IFV never repairs and Attack stays queued.
#[test]
fn retail_warped_engineer_ifv_repair_click_is_accepted_then_dropped() {
    for ares_way in [false, true] {
        let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules;
        let (mut sim, ifv, tank) = repair_ifv(rules);
        sim.entity_mut_for_test(ifv)
            .unwrap()
            .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(99));
        assert!(sim.entities().get(ifv).unwrap().is_warped_out());
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: tank,
        };
        let feedback = capability_cursor_for_hover(
            &sim,
            &[ifv],
            Some(ifv),
            &hover,
            Some(rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
            &mut Default::default(),
        );
        assert_eq!(feedback, CursorFeedbackKind::RepairAction(true));
        assert_eq!(sim.click_order_receivers(&[ifv], ares_way), [ifv]);
        let command = repair_click(&sim, rules, ifv, tank);
        assert!(sim.apply_command(OWNER, &command, Some(rules)));
        let attack =
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Attack);
        let ifv_state = |sim: &Simulation| {
            let ifv = sim.entities().get(ifv).unwrap();
            (ifv.attack_target.target(), ifv.mission.queued())
        };
        assert_eq!(ifv_state(&sim), (Some(TargetKind::Entity(tank)), attack));
        let grid = sim.path_grid_snapshot().unwrap();
        let before = sim.entities().get(tank).unwrap().health.current();
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        assert_eq!(
            ifv_state(&sim),
            (None, attack),
            "the frozen prologue drops TarCom; MissionClass::AI never runs"
        );
        for _ in 0..120 {
            sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        }
        assert!(sim.entities().get(ifv).unwrap().is_warped_out());
        assert_eq!(ifv_state(&sim), (None, attack));
        assert_eq!(
            sim.entities().get(tank).unwrap().health.current(),
            before,
            "a warped-out IFV must not repair"
        );
    }
}

/// Shared ordinary input fixture: real boarding, normal repair dispatch and
/// committed Logic create the first live Welding children. No particle, timer,
/// Display or effect field is installed by this fixture.
pub(in crate::app) struct OrdinaryWeldingScene {
    pub sim: Simulation,
    pub ifv: u64,
    pub target: u64,
    pub system: u64,
    pub first_repair_health: i32,
}

/// The WeldingSys's sparks: ParticleClass children, or under Ares the Spark
/// records the system keeps itself (hooks 0x62DF05, 0x62FD60).
fn welding_has_children(system: &crate::sim::particles::ParticleSystem) -> bool {
    if system
        .ares()
        .is_some_and(|ares| !ares.particles().is_empty())
    {
        return true;
    }
    !system.particles().is_empty()
}

pub(in crate::app) fn ordinary_welding_children(rules: &RuleSet) -> OrdinaryWeldingScene {
    let (mut sim, ifv, tank) = repair_ifv(rules);
    // No Ctrl: the actual normal-click producer uses the existing friendly
    // Attack-event alias. It is one native Attack mission, not force-fire state.
    let command = repair_click(&sim, rules, ifv, tank);
    assert!(sim.apply_command(OWNER, &command, Some(rules)));
    assert_eq!(
        sim.entities().get(ifv).unwrap().attack_target.target(),
        Some(TargetKind::Entity(tank))
    );
    let grid = sim.path_grid_snapshot().unwrap();
    let before = sim.entities().get(tank).unwrap().health.current();
    for _ in 0..120 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        if sim.entities().get(tank).unwrap().health.current() > before {
            break;
        }
    }
    let after = sim.entities().get(tank).unwrap().health.current();
    assert!(
        after > before && after <= rules.object("MTNK").unwrap().strength,
        "the ordinary selected repair weapon must raise target health"
    );
    // FireAt6FF1AD..6FF1F5 constructs the selected RepairBullet's retained
    // WeldingSys before GetROF/rearm. Healing alone does not exercise that
    // connected effect: its actual system must enter ordinary Logic/Display.
    let welding = rules.ps_type_id_by_name("WeldingSys").unwrap();
    let systems: Vec<_> = sim
        .particle_systems()
        .iter()
        .filter(|(_, system)| system.type_id() == welding)
        .collect();
    let [(_, system)] = systems.as_slice() else {
        panic!("the first ordinary repair must create one WeldingSys");
    };
    assert_eq!(system.owner_entity(), Some(ifv));
    assert_eq!(
        system.target_ref(),
        Some(crate::sim::combat::TargetKind::Entity(tank))
    );
    assert!(system.in_logic_vector());
    assert!(
        sim.display_layers()
            .ordered_ids()
            .any(|&id| id == system.stable_id()),
        "the constructed system must retain its ordinary Display membership"
    );
    let first_system = system.stable_id();
    for _ in 0..120 {
        if sim
            .particle_systems()
            .get(first_system)
            .is_some_and(welding_has_children)
        {
            break;
        }
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
    }
    let system = sim
        .particle_systems()
        .get(first_system)
        .expect("the first system still lives");
    assert!(
        welding_has_children(system),
        "ordinary Logic must create real Welding children"
    );
    assert_eq!(
        sim.entities().get(tank).unwrap().health.current(),
        after,
        "the fixture stops before the next repair"
    );
    OrdinaryWeldingScene {
        sim,
        ifv,
        target: tank,
        system: first_system,
        first_repair_health: after,
    }
}

#[test]
fn retail_engineer_ifv_plain_repair_click_reaches_attack_mission_and_heals() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let OrdinaryWeldingScene {
        mut sim,
        ifv,
        target: tank,
        system: first_system,
        first_repair_health: after,
    } = ordinary_welding_children(rules);
    let grid = sim.path_grid_snapshot().unwrap();
    let welding = rules.ps_type_id_by_name("WeldingSys").unwrap();
    let mut drained_after_emission = false;
    let mut retired = false;
    for _ in 0..300 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        match sim.particle_systems().get(first_system) {
            Some(system) => {
                drained_after_emission |= system.spark_spawn_frames() <= 0
                    && system.done_spawning()
                    && welding_has_children(system);
            }
            None => retired = true,
        }
        if sim.entities().get(tank).unwrap().health.current() > after {
            break;
        }
    }
    assert!(
        drained_after_emission,
        "the real children outlive the emission countdown"
    );
    assert!(retired, "the first system must retire through ordinary AI");
    assert!(
        sim.entities().get(tank).unwrap().health.current() > after,
        "the next ordinary repair must resume after actual system expiry and rearm"
    );
    assert!(sim.particle_systems().iter().any(|(&id, system)| {
        id != first_system && system.type_id() == welding && system.owner_entity() == Some(ifv)
    }));
}

#[test]
fn retail_far_repair_click_drives_into_range_and_heals_without_ctrl() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, ifv, _) = repair_ifv(rules);
    let far = sim.spawn_object("MTNK", OWNER, 25, 16, 64, rules).unwrap();
    sim.entity_mut_for_test(far)
        .unwrap()
        .health
        .set_for_test(100);
    let start = sim.entities().get(ifv).unwrap().position;
    let command = repair_click(&sim, rules, ifv, far);
    assert!(sim.apply_command(OWNER, &command, Some(rules)));
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..900 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        if sim.entities().get(far).unwrap().health.current() > 100 {
            break;
        }
    }
    let current = sim.entities().get(ifv).unwrap().position;
    assert_ne!(
        (current.rx(), current.ry()),
        (start.rx(), start.ry()),
        "the ordered repair must approach a distant ally"
    );
    assert!(
        sim.entities().get(far).unwrap().health.current() > 100,
        "a normal repair click must reach its damaged target and fire"
    );
}

#[test]
fn retail_repair_click_preserves_native_modifier_and_target_controls() {
    for ares_way in [false, true] {
        let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules;
        let (mut sim, ifv, tank) = repair_ifv(rules);
        let owner = sim.entities().get(ifv).unwrap().owner();
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: tank,
        };
        let packet: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/bunker_attack_action.json",
        ))
        .unwrap();
        for (modifier, name) in [
            (OrderModifier::Normal, "damaged_friend"),
            (OrderModifier::ForceFire, "ctrl_friend"),
            (OrderModifier::ForceMove, "alt_friend"),
            (OrderModifier::GuardArea, "ctrl_alt_friend"),
            (OrderModifier::Shift, "shift_friend"),
        ] {
            let native = packet["repair_rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["name"] == name)
                .unwrap();
            let dispatch = unit_repair_branch_orders(
                &sim,
                rules,
                &[ifv],
                &[ifv],
                &hover,
                owner,
                modifier,
                OrderMode::Move,
                false,
            );
            let native_orders: Vec<_> = native["trace"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["kind"] == "command_sink")
                .collect();
            // Ares 1004E5A0 (hook 0x73FDBD): Alt answers Select and Ctrl+Alt's
            // GuardArea keeps its own path, so neither repairs.
            let expected = if ares_way && matches!(name, "alt_friend" | "ctrl_alt_friend") {
                0
            } else {
                native_orders.len()
            };
            assert_eq!(dispatch.commands.len(), expected, "{name}");
            if expected != 0 {
                assert_eq!(native_orders[0]["mission"], 1);
                assert!(
                    matches!(dispatch.commands[0], Command::Attack { .. }),
                    "{name}"
                );
            }
        }
        assert_eq!(
            sim.unit_repair_action(
                ifv,
                tank,
                owner,
                crate::sim::world::HealArmKeys::default(),
                rules
            ),
            Some(UnitRepairAction::Repair)
        );
        assert_eq!(
            sim.unit_repair_action(
                ifv,
                ifv,
                owner,
                crate::sim::world::HealArmKeys::default(),
                rules
            ),
            Some(UnitRepairAction::Select)
        );
        let wrong_owner = sim.interner().get("Russians").unwrap();
        assert_eq!(
            sim.unit_repair_action(
                ifv,
                tank,
                wrong_owner,
                crate::sim::world::HealArmKeys::default(),
                rules
            ),
            None
        );
        sim.entity_mut_for_test(tank)
            .unwrap()
            .health
            .set_for_test(rules.object("MTNK").unwrap().strength);
        assert!(
            unit_repair_branch_orders(
                &sim,
                rules,
                &[ifv],
                &[ifv],
                &hover,
                owner,
                OrderModifier::Normal,
                OrderMode::Move,
                false
            )
            .commands
            .is_empty()
        );
        sim.entity_mut_for_test(tank)
            .unwrap()
            .health
            .set_for_test(100);
        let enemy = sim
            .spawn_object("MTNK", "Russians", 23, 16, 64, rules)
            .unwrap();
        sim.entity_mut_for_test(enemy)
            .unwrap()
            .health
            .set_for_test(100);
        assert_eq!(
            sim.unit_repair_action(
                ifv,
                enemy,
                owner,
                crate::sim::world::HealArmKeys::default(),
                rules
            ),
            Some(UnitRepairAction::Move)
        );
        let enemy_hover = HoverTargetKindWithId {
            kind: HoverTargetKind::EnemyUnit,
            stable_id: enemy,
        };
        let move_dispatch = unit_repair_branch_orders(
            &sim,
            rules,
            &[ifv],
            &[ifv],
            &enemy_hover,
            owner,
            OrderModifier::Normal,
            OrderMode::Move,
            false,
        );
        assert_eq!(move_dispatch.commands.len(), 1);
        assert!(
            matches!(move_dispatch.commands[0], Command::Move { entity_id, .. } if entity_id == ifv)
        );
        sim.house_alliances_mut_for_test()
            .entry(OWNER.to_ascii_uppercase())
            .or_default()
            .insert("RUSSIANS".to_string());
        assert_eq!(
            sim.unit_repair_action(
                ifv,
                enemy,
                owner,
                crate::sim::world::HealArmKeys::default(),
                rules
            ),
            Some(UnitRepairAction::Repair),
            "the actor's one-way alliance is the native query"
        );
        let empty = sim.spawn_object("FV", OWNER, 20, 20, 64, rules).unwrap();
        assert_eq!(
            sim.unit_repair_action(
                empty,
                tank,
                owner,
                crate::sim::world::HealArmKeys::default(),
                rules
            ),
            None
        );
        let (sim, gi_ifv, tank) = boarded_ifv(rules, "E1");
        assert_eq!(
            sim.unit_repair_action(
                gi_ifv,
                tank,
                sim.entities().get(gi_ifv).unwrap().owner(),
                crate::sim::world::HealArmKeys::default(),
                rules
            ),
            None,
            "a different passenger must not inherit Engineer repair admission"
        );
    }
}

#[test]
fn issued_repair_attack_is_not_readmitted_when_health_or_cargo_changes() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, ifv, tank) = repair_ifv(rules);
    let command = repair_click(&sim, rules, ifv, tank);
    sim.entity_mut_for_test(tank)
        .unwrap()
        .health
        .set_for_test(rules.object("MTNK").unwrap().strength);
    assert!(sim.apply_command(OWNER, &command, Some(rules)));
    assert_eq!(
        sim.entities().get(ifv).unwrap().attack_target.target(),
        Some(TargetKind::Entity(tank)),
        "the event still commits its original target"
    );
    let (mut sim, ifv, tank) = repair_ifv(rules);
    let command = repair_click(&sim, rules, ifv, tank);
    use crate::sim::passenger::{
        DepartureRoute, depart_cargo_head, exit_cell_position, reveal_unloaded_passenger,
    };
    depart_cargo_head(
        &mut sim,
        rules,
        ifv,
        DepartureRoute::Vehicle,
        |sim, passenger, _| {
            let position = exit_cell_position(sim, passenger, 16, 15, 0);
            reveal_unloaded_passenger(
                sim,
                rules,
                ifv,
                passenger,
                position,
                crate::sim::world::PlacementEvidence::MarkSucceeded,
                None,
                &mut FrameEffects::default(),
            )
        },
        &mut FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.entities().get(ifv).unwrap().current_weapon_number(), 0);
    assert!(sim.apply_command(OWNER, &command, Some(rules)));
    assert_eq!(
        sim.entities().get(ifv).unwrap().attack_target.target(),
        Some(TargetKind::Entity(tank)),
        "cargo changes do not replace the issued event"
    );
}

#[test]
fn unit_repair_vehicle_class_matches_fire_error_including_undeploy_building() {
    for ares_way in [false, true] {
        let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &crate::sim::arena_fixture::clear_ground_ini(
                "[VehicleTypes]\n0=REPAIRER\n1=TANK\n2=ORGANIC\n\
             [BuildingTypes]\n0=DEPLOYED\n1=ORDINARY\n\
             [REPAIRER]\nStrength=100\nPrimary=Fix\nSpeed=5\n\
             [TANK]\nStrength=100\nArmor=heavy\nSpeed=5\n\
             [ORGANIC]\nStrength=100\nArmor=heavy\nNonVehicle=yes\nSpeed=5\n\
             [DEPLOYED]\nStrength=100\nArmor=heavy\nUndeploysInto=TANK\n\
             [ORDINARY]\nStrength=100\nArmor=heavy\n\
             [Fix]\nDamage=-50\nRange=5\nOmniFire=yes\nProjectile=Invisible\nWarhead=Mechanical\n\
             [Invisible]\nInviso=yes\n\
             [Mechanical]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            ),
            &crate::rules::ini_parser::IniFile::from_str(
                "[DEPLOYED]\nFoundation=1x1\n[ORDINARY]\nFoundation=1x1\n",
            ),
        )
        .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.intern(OWNER);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let actor = sim
            .spawn_object("REPAIRER", OWNER, 10, 10, 64, &rules)
            .unwrap();
        let mut targets = Vec::new();
        for (kind, cell) in [
            ("TANK", (11, 10)),
            ("ORGANIC", (11, 11)),
            ("DEPLOYED", (12, 10)),
            ("ORDINARY", (12, 11)),
        ] {
            let target = sim
                .spawn_object(kind, OWNER, cell.0, cell.1, 64, &rules)
                .unwrap();
            sim.entity_mut_for_test(target)
                .unwrap()
                .health
                .set_for_test(50);
            targets.push(target);
        }
        assert_eq!(
            sim.unit_repair_action(
                actor,
                targets[0],
                owner,
                crate::sim::world::HealArmKeys::default(),
                &rules
            ),
            Some(UnitRepairAction::Repair)
        );
        assert_eq!(
            sim.unit_repair_action(
                actor,
                targets[1],
                owner,
                crate::sim::world::HealArmKeys::default(),
                &rules
            ),
            // Ares 1004E5A0 (hook 0x73FDBD) drops the vt+80 vehicle test: any
            // damaged ally the warhead reaches is repaired.
            Some(if ares_way {
                UnitRepairAction::Repair
            } else {
                UnitRepairAction::Select
            })
        );
        assert_eq!(
            sim.unit_repair_action(
                actor,
                targets[2],
                owner,
                crate::sim::world::HealArmKeys::default(),
                &rules
            ),
            Some(UnitRepairAction::Repair)
        );
        assert_eq!(
            sim.unit_repair_action(
                actor,
                targets[3],
                owner,
                crate::sim::world::HealArmKeys::default(),
                &rules
            ),
            ares_way.then_some(UnitRepairAction::Repair)
        );
        use crate::sim::combat::fire_error::FireError;
        assert_eq!(
            sim.selected_weapon_fire_error(&rules, actor, TargetKind::Entity(targets[2]), true,),
            FireError::Ok,
            "the same native Building vt+80 fact must reach UnitFireError"
        );
        // Ares 3.0 UnitClass_CanFire_Heal (1004E540, hook 0x741113) replaces the
        // vt+80 call with the health-ratio test alone, so a damaged NonVehicle
        // Unit may be repaired.
        assert_eq!(
            sim.selected_weapon_fire_error(&rules, actor, TargetKind::Entity(targets[1]), true,),
            if ares_way {
                FireError::Ok
            } else {
                FireError::Illegal
            }
        );
    }
}

/// Ares 1004E5A0 (hook 0x73FDBD): the repair IFV still repairs a damaged
/// ally, but a GuardArea base action keeps its own path, a held ForceMove key
/// answers Select, and so does a sound ally, where the stock arm kept the base
/// action.
#[test]
fn ares_repair_ifv_selects_a_sound_ally_and_under_alt() {
    use crate::sim::world::HealArmKeys;
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, ifv, tank) = repair_ifv(rules);
    let owner = sim.interner().get(OWNER).unwrap();
    let action = |sim: &Simulation, keys| sim.unit_repair_action(ifv, tank, owner, keys, rules);
    assert_eq!(
        action(&sim, HealArmKeys::default()),
        Some(UnitRepairAction::Repair)
    );
    let alt = HealArmKeys {
        force_move: true,
        guard_area: false,
        moves: true,
    };
    assert_eq!(action(&sim, alt), Some(UnitRepairAction::Select));
    let guard = HealArmKeys {
        force_move: true,
        guard_area: true,
        moves: false,
    };
    assert_eq!(action(&sim, guard), None);
    let strength = rules.object("MTNK").unwrap().strength;
    sim.entity_mut_for_test(tank)
        .unwrap()
        .health
        .set_for_test(strength);
    assert_eq!(
        action(&sim, HealArmKeys::default()),
        Some(UnitRepairAction::Select)
    );
}

/// Ares 1004E2E0 (hook 0x51E710): an Infantryman with a healing weapon heals
/// a damaged ally its warhead's Verses reach, with InfantryHeal (0x5A) over an
/// Infantry and UnitRepair (0x5B) over a vehicle, unless Alt is held. Here two
/// unarmed civilians carry `Heal` (Organic, which no retail type allocates
/// before the map pass, so the patch restates it) and `RepairBullet`.
#[test]
fn ares_healer_infantry_heal_what_their_warhead_reaches() {
    use crate::sim::world::{HealArmKeys, InfantryHealAction};
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &crate::rules::ini_parser::IniFile::from_str(
            "[CIV1]\nPrimary=Heal\nElitePrimary=Heal\n\
             [CIV2]\nPrimary=RepairBullet\nElitePrimary=RepairBullet\n\
             [Heal]\nDamage=-50\nProjectile=Invisible\nWarhead=Organic\nRange=2\n\
             [Organic]\nVerses=100%,100%,100%,0%,0%,0%,0%,0%,0%,0%,100%\n",
        ),
    ) else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _ifv, tank) = boarded_ifv(rules, "ENGINEER");
    let medic = sim.spawn_object("CIV1", OWNER, 18, 18, 64, rules).unwrap();
    let mechanic = sim.spawn_object("CIV2", OWNER, 19, 18, 64, rules).unwrap();
    let hurt = sim.spawn_object("E1", OWNER, 20, 18, 64, rules).unwrap();
    sim.entity_mut_for_test(hurt)
        .unwrap()
        .health
        .set_for_test(50);
    let owner = sim.interner().get(OWNER).unwrap();
    let heal = |sim: &Simulation, actor, target, keys| {
        sim.infantry_heal_action(actor, target, owner, keys, rules)
    };
    let keys = HealArmKeys::default();
    assert_eq!(
        heal(&sim, medic, hurt, keys),
        Some(InfantryHealAction::Heal { organic: true })
    );
    assert_eq!(
        heal(&sim, medic, tank, keys),
        Some(InfantryHealAction::Select),
        "Organic is 0% on heavy"
    );
    assert_eq!(
        heal(&sim, mechanic, tank, keys),
        Some(InfantryHealAction::Heal { organic: false })
    );
    let alt = HealArmKeys {
        force_move: true,
        guard_area: false,
        moves: true,
    };
    assert_eq!(heal(&sim, mechanic, tank, alt), None);
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyUnit,
        stable_id: tank,
    };
    assert_eq!(
        capability_cursor_for_hover(
            &sim,
            &[mechanic],
            Some(mechanic),
            &hover,
            Some(rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
            &mut Default::default(),
        ),
        CursorFeedbackKind::AresAction(0x5B)
    );
}

/// What a lone selected healer clicked on itself shows and sends
/// (`InfantryClass::What_Action_OnObject` 0x51E748): the cursor, the click
/// its ClickedAction makes of the answer, and the lone selection's release.
fn healer_self_click(
    rules: &RuleSet,
    healer_type: &str,
) -> (
    CursorFeedbackKind,
    Option<Command>,
    Option<Option<Command>>,
    u64,
) {
    use crate::app::input::cursor::{ActionDistanceTarget, select_best_for_action};
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x0051_E748);
    let owner = sim.interner().get(OWNER).unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let healer = sim
        .spawn_object(healer_type, OWNER, 9, 9, 0, rules)
        .unwrap();
    sim.entity_mut_for_test(healer)
        .unwrap()
        .seldom
        .selection
        .set_selected_for_test(true);
    let best = select_best_for_action(
        &sim,
        &[healer],
        ActionDistanceTarget::Object(healer),
        Some(rules),
    );
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyUnit,
        stable_id: healer,
    };
    let cursor = capability_cursor_for_hover(
        &sim,
        &[healer],
        best,
        &hover,
        Some(rules),
        None,
        OrderModifier::Normal,
        false,
        false,
        &mut Default::default(),
    );
    let click = object_attack_click_command(
        &sim,
        rules,
        &crate::rules::overlay_types::OverlayTypeRegistry::empty(),
        owner,
        OrderMode::Move,
        OrderModifier::Normal,
        false,
        healer,
        healer,
    );
    let release = lone_self_healer_guard(&sim, &sim.control_projection(), rules, &[healer], healer);
    (cursor, click, release, healer)
}

const HEALER_PATCH: &str = "[CIV1]\nPrimary=Heal\nElitePrimary=Heal\n\
     [Heal]\nDamage=-50\nProjectile=Invisible\nWarhead=Organic\nRange=2\n";

/// A healer infantryman clicked on itself answers GuardArea (0x51E748) and
/// guards its own area through Foot's AreaGuard case (0x4D7543). No retail
/// infantryman heals, so a civilian carries `Heal` here.
#[test]
fn a_healer_infantryman_clicked_on_itself_guards_its_area() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &crate::rules::ini_parser::IniFile::from_str(HEALER_PATCH),
    ) else {
        return;
    };
    assert!(!retail.rules.ares());
    let (cursor, click, release, healer) = healer_self_click(&retail.rules, "CIV1");
    let guard = Command::Guard {
        entity_id: healer,
        target: Some(crate::sim::combat::TargetKind::Entity(healer)),
    };
    assert_eq!(cursor, CursorFeedbackKind::GuardArea);
    assert_eq!(click, Some(guard.clone()));
    assert_eq!(release, Some(Some(guard)));
}

/// Ares' heal hook (0x51E710, 1004E2E0) reaches the same GuardArea for the
/// healer itself; `NoSelfGuardArea=yes` (hook 0x51E748, 10049710) skips it,
/// so that healer answers NoMove over itself as an unarmed infantryman does.
#[test]
fn ares_no_self_guard_area_keeps_a_healer_off_its_own_guard() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &crate::rules::ini_parser::IniFile::from_str(&format!(
            "{HEALER_PATCH}[CIV2]\nPrimary=Heal\nElitePrimary=Heal\nNoSelfGuardArea=yes\n"
        )),
    ) else {
        return;
    };
    let rules = &retail.rules;
    assert!(rules.ares());
    let (cursor, click, release, _) = healer_self_click(rules, "CIV1");
    assert_eq!(cursor, CursorFeedbackKind::GuardArea);
    assert!(matches!(click, Some(Command::Guard { .. })), "{click:?}");
    assert!(
        matches!(release, Some(Some(Command::Guard { .. }))),
        "{release:?}"
    );
    let (cursor, click, release, _) = healer_self_click(rules, "CIV2");
    assert_eq!(cursor, CursorFeedbackKind::Invalid);
    assert!(
        matches!(
            click,
            Some(Command::Move { .. } | Command::MoveToObject { .. })
        ),
        "{click:?}"
    );
    assert_eq!(release, None);
}

/// The player's and a rival house on open skirmish ground, with what a healer
/// infantryman shows and sends over a target (InfantryClass What_Action's
/// healer arm).
struct HealerArena {
    sim: Simulation,
    owner: crate::sim::intern::InternedId,
}

impl HealerArena {
    fn new(rules: &RuleSet) -> Self {
        let mut sim = Simulation::new();
        for (name, side) in [(OWNER, 0), ("Russians", 1)] {
            let house = sim.intern(name);
            sim.houses_mut_for_test()
                .register(HouseState::new(house, side, None, true, 0, 10));
        }
        let owner = sim.interner().get(OWNER).unwrap();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        // Outside a campaign Foot's shroud rung leaves the base action alone.
        sim.session_mut_for_test()
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        crate::sim::arena_fixture::flat_arena(&mut sim, rules);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        Self { sim, owner }
    }

    fn cursor(
        &self,
        rules: &RuleSet,
        selected: &[u64],
        target: u64,
        kind: HoverTargetKind,
        modifier: OrderModifier,
    ) -> CursorFeedbackKind {
        capability_cursor_for_hover(
            &self.sim,
            selected,
            Some(selected[0]),
            &HoverTargetKindWithId {
                kind,
                stable_id: target,
            },
            Some(rules),
            None,
            modifier,
            false,
            false,
            &mut Default::default(),
        )
    }

    fn click(
        &self,
        rules: &RuleSet,
        healer: u64,
        target: u64,
        kind: HoverTargetKind,
        modifier: OrderModifier,
    ) -> AttackDispatch {
        unit_repair_branch_orders(
            &self.sim,
            rules,
            &[healer],
            &[healer],
            &HoverTargetKindWithId {
                kind,
                stable_id: target,
            },
            self.owner,
            modifier,
            OrderMode::Move,
            false,
        )
    }
}

/// Without Ares a healer infantryman heals a damaged Infantryman
/// (`0x0051E73C`): Heal shows SetCursorFromAction's row 0x3C, and Infantry
/// ClickedAction (`0x0051F190`) sends it as Attack. Over any other ally, Ctrl
/// included, the arm answers Select (`0x0051E78B`), and the release selects.
#[test]
fn a_stock_healer_infantryman_heals_a_hurt_infantryman_and_selects_the_rest() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &crate::rules::ini_parser::IniFile::from_str(HEALER_PATCH),
    ) else {
        return;
    };
    let rules = &retail.rules;
    assert!(!rules.ares());
    let mut arena = HealerArena::new(rules);
    let medic = arena
        .sim
        .spawn_object("CIV1", OWNER, 9, 9, 0, rules)
        .unwrap();
    let hurt = arena
        .sim
        .spawn_object("E1", OWNER, 10, 9, 0, rules)
        .unwrap();
    let tank = arena
        .sim
        .spawn_object("MTNK", OWNER, 11, 11, 0, rules)
        .unwrap();
    arena
        .sim
        .entity_mut_for_test(hurt)
        .unwrap()
        .health
        .set_for_test(50);
    let friendly = HoverTargetKind::FriendlyUnit;
    assert_eq!(
        arena.cursor(rules, &[medic], hurt, friendly, OrderModifier::Normal),
        CursorFeedbackKind::Heal
    );
    assert_eq!(
        cursor_id_for_feedback(CursorFeedbackKind::Heal),
        Some(CursorId::NoTogglePower)
    );
    let heal = arena.click(rules, medic, hurt, friendly, OrderModifier::Normal);
    assert!(
        matches!(heal.commands.as_slice(), [Command::Attack { attacker_id, target_id }]
            if *attacker_id == medic && *target_id == hurt),
        "{:?}",
        heal.commands
    );
    assert_eq!(
        arena.cursor(rules, &[medic], tank, friendly, OrderModifier::ForceFire),
        CursorFeedbackKind::FriendlyUnit
    );
    let select = arena.click(rules, medic, tank, friendly, OrderModifier::ForceFire);
    assert!(select.selects && select.commands.is_empty());
    // Its Select ends the rows before the Grinder's (`0x0051F0AF`).
    let grinder = arena
        .sim
        .spawn_object("YAGRND", OWNER, 14, 14, 0, rules)
        .unwrap();
    let structure = HoverTargetKind::FriendlyStructure;
    assert_eq!(
        arena.cursor(rules, &[medic], grinder, structure, OrderModifier::Normal),
        CursorFeedbackKind::FriendlyUnit
    );
    assert!(
        arena
            .click(rules, medic, grinder, structure, OrderModifier::Normal)
            .selects
    );
    // A berserk healer's arm still runs over its base action 0.
    arena
        .sim
        .entity_mut_for_test(medic)
        .unwrap()
        .berserk
        .set_active_for_test(true);
    let over_tank = HoverTargetKindWithId {
        kind: friendly,
        stable_id: tank,
    };
    assert_eq!(
        berserk_selection_click(
            &arena.sim,
            rules,
            &[medic],
            Some(&over_tank),
            (11, 11),
            arena.owner
        ),
        LeftRelease::Select
    );
}

/// The healer arm's tail keeps a Move base action (`0x0051E77F`): in a
/// campaign Foot's shroud rung (`0x004DDED0`) makes the base action over an
/// ally whose coordinate the player's map shrouds Move, so the healer moves
/// there, cursor and click alike, where in a skirmish it selects.
#[test]
fn a_healer_keeps_the_campaign_shroud_move_over_an_ally() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &crate::rules::ini_parser::IniFile::from_str(HEALER_PATCH),
    ) else {
        return;
    };
    let rules = &retail.rules;
    let mut arena = HealerArena::new(rules);
    let medic = arena
        .sim
        .spawn_object("CIV1", OWNER, 9, 9, 0, rules)
        .unwrap();
    let tank = arena
        .sim
        .spawn_object("MTNK", OWNER, 11, 11, 0, rules)
        .unwrap();
    let centre = arena.sim.object_coords_cell(tank).unwrap();
    *arena.sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    arena.sim.fog_mut_for_test().reveal_cells_for_owner(
        arena.owner,
        (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .filter(|&cell| cell != centre),
    );
    let friendly = HoverTargetKind::FriendlyUnit;
    assert_eq!(
        arena.cursor(rules, &[medic], tank, friendly, OrderModifier::Normal),
        CursorFeedbackKind::FriendlyUnit
    );

    arena
        .sim
        .session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Campaign);
    assert_eq!(
        arena.cursor(rules, &[medic], tank, friendly, OrderModifier::Normal),
        CursorFeedbackKind::Move
    );
    // The arm leaves the receiver to the later click branches, which move it.
    let healer_branch = arena.click(rules, medic, tank, friendly, OrderModifier::Normal);
    assert!(!healer_branch.selects && healer_branch.receivers.is_empty());
    let click = object_attack_click_command(
        &arena.sim,
        rules,
        &crate::rules::overlay_types::OverlayTypeRegistry::empty(),
        arena.owner,
        OrderMode::Move,
        OrderModifier::Normal,
        false,
        medic,
        tank,
    );
    assert!(
        matches!(
            click,
            Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
                if entity_id == medic
        ),
        "{click:?}"
    );
}

/// Over an object of a house it is not allied with, a tree's null House
/// included, a healer infantryman answers AttackSupport (`0x0051E797`): the
/// arrow, and Foot's case 0x3B (`0x004D74E0`) puts the healer on Area Guard
/// of itself, as its Primary heals.
#[test]
fn a_healer_infantryman_guards_itself_over_an_enemy_or_a_tree() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &crate::rules::ini_parser::IniFile::from_str(HEALER_PATCH),
    ) else {
        return;
    };
    let rules = &retail.rules;
    let mut arena = HealerArena::new(rules);
    let medic = arena
        .sim
        .spawn_object("CIV1", OWNER, 9, 9, 0, rules)
        .unwrap();
    let enemy = arena
        .sim
        .spawn_object("E1", "Russians", 11, 9, 0, rules)
        .unwrap();
    assert_eq!(
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut arena.sim,
            &[crate::map::overlay::TerrainObject {
                rx: 9,
                ry: 12,
                name: "TREE01".into(),
            }],
            rules,
            false,
        ),
        1
    );
    let tree = arena.sim.terrain_objects().id_at((9, 12)).unwrap();
    let guard = Command::Guard {
        entity_id: medic,
        target: Some(crate::sim::combat::TargetKind::Entity(medic)),
    };
    let enemy_kind = HoverTargetKind::EnemyUnit;
    assert_eq!(
        arena.cursor(rules, &[medic], enemy, enemy_kind, OrderModifier::Normal),
        CursorFeedbackKind::Default
    );
    let click = arena.click(rules, medic, enemy, enemy_kind, OrderModifier::Normal);
    assert!(click.consumed);
    assert_eq!(click.commands, [guard.clone()]);
    let tree = crate::sim::combat::TargetKind::Entity(tree);
    assert_eq!(
        attack_action(&arena.sim, rules, medic, &tree, true, None),
        AttackAction::AttackSupport
    );
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    let click = attack_branch_orders(
        &arena.sim,
        rules,
        &[medic],
        tree,
        true,
        OrderMode::Move,
        &registry,
    );
    assert_eq!(click.commands, [guard]);
}

/// A healer whose Secondary heals but whose Primary does not (their average
/// is negative, `0x006F3970`) answers AttackSupport too, and Foot's case
/// 0x3B puts it on Guard of itself, not Area Guard (`0x004D7C93`). Over a
/// tree the arm (`0x0051E797`) replaces Alt's Move and Ctrl+Alt's
/// GuardArea, the arrow and the same Guard; Shift's ToggleSelect returns
/// before it.
#[test]
fn a_secondary_healer_guards_itself_and_its_arm_outranks_alt_over_a_tree() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &crate::rules::ini_parser::IniFile::from_str(
            "[CIV1]\nPrimary=Poke\nElitePrimary=Poke\nSecondary=Heal\nEliteSecondary=Heal\n\
             [Heal]\nDamage=-50\nProjectile=Invisible\nWarhead=Organic\nRange=2\n\
             [Poke]\nDamage=10\nProjectile=Invisible\nWarhead=SA\nRange=2\n",
        ),
    ) else {
        return;
    };
    let rules = &retail.rules;
    let mut arena = HealerArena::new(rules);
    let medic = arena
        .sim
        .spawn_object("CIV1", OWNER, 9, 9, 0, rules)
        .unwrap();
    let enemy = arena
        .sim
        .spawn_object("E1", "Russians", 11, 9, 0, rules)
        .unwrap();
    assert_eq!(
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut arena.sim,
            &[crate::map::overlay::TerrainObject {
                rx: 9,
                ry: 12,
                name: "TREE01".into(),
            }],
            rules,
            false,
        ),
        1
    );
    let tree = arena.sim.terrain_objects().id_at((9, 12)).unwrap();
    let guard = Command::MegaMission {
        entity_id: medic,
        mission: crate::sim::mission::MissionType::Guard,
        target: Some(crate::sim::combat::TargetKind::Entity(medic)),
        destination: None,
    };
    let click = arena.click(
        rules,
        medic,
        enemy,
        HoverTargetKind::EnemyUnit,
        OrderModifier::Normal,
    );
    assert_eq!(click.commands, [guard.clone()]);

    let selected = [medic];
    for (modifier, alt, cursor) in [
        (OrderModifier::ForceMove, true, CursorFeedbackKind::Default),
        (OrderModifier::GuardArea, true, CursorFeedbackKind::Default),
        (OrderModifier::Shift, false, CursorFeedbackKind::Select),
    ] {
        let query = crate::app::input::cursor::ActionQuery {
            sim: &arena.sim,
            rules: Some(rules),
            overlays: None,
            viewer: Some(arena.owner),
            selected: &selected,
            modifier,
            force_move_key: alt,
            force_fire_key: modifier == OrderModifier::GuardArea,
            order_mode: OrderMode::Move,
            planning: false,
            mouse_over: true,
            full_visibility: true,
        };
        assert_eq!(
            query
                .selection_action((9, 12), Some(HoverObject::Terrain(tree)))
                .0,
            Some(cursor),
            "{modifier:?}"
        );
        assert_eq!(
            healer_terrain_key_order(
                &arena.sim,
                &arena.sim.control_projection(),
                rules,
                medic,
                tree,
                modifier,
                alt,
            ),
            (modifier != OrderModifier::Shift).then(|| guard.clone()),
            "{modifier:?}"
        );
    }
}

/// A healing Engineer among several selected objects answers NoMove under
/// Ctrl over itself: Ctrl forces Techno's attack rung, which the Engineer
/// refuses (`0x0051E6B4..0x0051E6D5`) before the healer's self arm. Alone it
/// answers Self over itself first (`0x007000B9`), so the arm guards.
#[test]
fn a_healing_engineer_in_a_group_refuses_ctrl_over_itself() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &crate::rules::ini_parser::IniFile::from_str(&format!(
            "{HEALER_PATCH}[ENGINEER]\nPrimary=Heal\nElitePrimary=Heal\n"
        )),
    ) else {
        return;
    };
    let rules = &retail.rules;
    let mut arena = HealerArena::new(rules);
    let engineer = arena
        .sim
        .spawn_object("ENGINEER", OWNER, 9, 9, 0, rules)
        .unwrap();
    let gi = arena
        .sim
        .spawn_object("E1", OWNER, 10, 9, 0, rules)
        .unwrap();
    let own = HoverTargetKind::FriendlyUnit;
    let ctrl = OrderModifier::ForceFire;
    assert_eq!(
        arena.cursor(rules, &[engineer, gi], engineer, own, ctrl),
        CursorFeedbackKind::Invalid
    );
    let click = object_attack_click_command(
        &arena.sim,
        rules,
        &crate::rules::overlay_types::OverlayTypeRegistry::empty(),
        arena.owner,
        OrderMode::Move,
        ctrl,
        false,
        engineer,
        engineer,
    );
    assert!(
        matches!(
            click,
            Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
                if entity_id == engineer
        ),
        "{click:?}"
    );
    assert_eq!(
        arena.cursor(rules, &[engineer], engineer, own, ctrl),
        CursorFeedbackKind::GuardArea
    );
}
