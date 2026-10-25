//! A Factory can retain a transferred limbo product after Action36 compacts
//! the Techno registry and skips its remaining producer. Original6E0B60,
//! 6F463B/63F000 and6E0C18 establish this reached ownership split.

use super::tests::install_event_ingress_map;
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::production::{PRODUCTION_STEPS, ProductionCategory};
use crate::sim::snapshot::GameSnapshot;

fn frame(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, command: Option<Command>) {
    let commands =
        command.map(|command| CommandEnvelope::new(owner, sim.session.tick() + 1, command));
    sim.advance_tick(commands.as_slice(), Some(rules), None, None, 67);
}

fn held(sim: &Simulation, owner: InternedId, category: ProductionCategory) -> u64 {
    sim.production
        .factories()
        .view(owner, category)
        .expect("real queue")
        .object
        .expect("constructed head")
        .entity_id
        .expect("held identity")
}

fn paid_place(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    name: &str,
    rx: u16,
    ry: u16,
) -> u64 {
    let type_id = sim.interner.get(name).unwrap();
    frame(
        sim,
        rules,
        owner,
        Some(Command::QueueProduction { type_id }),
    );
    let id = held(sim, owner, ProductionCategory::Building);
    for _ in 0..20_000 {
        let view = sim
            .production
            .factories()
            .view(owner, ProductionCategory::Building)
            .expect("building queue retained");
        if view.progress == PRODUCTION_STEPS {
            break;
        }
        frame(sim, rules, owner, None);
    }
    assert_eq!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Building)
            .unwrap()
            .progress,
        PRODUCTION_STEPS,
        "{name} paid completion"
    );
    let owner_name = sim.interner.resolve(owner);
    let preview = crate::sim::production::placement_preview_for_owner_with_overlays(
        sim, rules, owner_name, name, rx, ry, None,
    )
    .unwrap();
    assert_eq!(preview.reason, None, "{name} placement site");
    frame(
        sim,
        rules,
        owner,
        Some(Command::PlaceReadyBuilding { type_id, rx, ry }),
    );
    assert!(
        !sim.substrate.entities.get(id).unwrap().lifecycle.in_limbo(),
        "{name} placed actual product"
    );
    for _ in 0..120 {
        frame(sim, rules, owner, None);
    }
    id
}

fn queue_hold(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    name: &str,
    category: ProductionCategory,
) -> u64 {
    let type_id = sim.interner.get(name).unwrap();
    frame(
        sim,
        rules,
        owner,
        Some(Command::QueueProduction { type_id }),
    );
    let id = held(sim, owner, category);
    frame(
        sim,
        rules,
        owner,
        Some(Command::SuspendProduction { category }),
    );
    let view = sim.production.factories().view(owner, category).unwrap();
    assert!(
        view.suspended && view.progress < PRODUCTION_STEPS,
        "{name} genuinely held"
    );
    id
}

fn stock_world(with_plant: bool) -> Option<(Simulation, RuleSet, InternedId, InternedId)> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let rules = retail.rules;
    let mut sim = Simulation::with_seed(0x4ca6_e036);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    let owner = sim.interner.intern("Americans");
    let target = sim.interner.intern("Russians");
    for (house, slot, human) in [(owner, 0, true), (target, 1, false)] {
        let mut state = HouseState::new(house, slot, Some(house), human, 10_000, 10);
        state.project_country_mults(&rules, &sim.interner);
        state.set_current_iq_for_test(if human {
            0
        } else {
            rules.general.max_iq_levels
        });
        sim.houses.register(state);
    }
    sim.session.set_current_house_for_test(Some(owner));
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    sim.fog.ensure_viewer(owner);
    sim.fog.ensure_viewer(target);
    // Completed map-scene priors all precede the ordered production interval.
    for (name, country, x, y) in [
        ("GACNST", "Americans", 13, 18),
        ("GAPOWR", "Americans", 10, 11),
        ("GAPOWR", "Americans", 14, 10),
        ("GAREFN", "Americans", 10, 14),
        ("NAPOWR", "Russians", 26, 26),
    ] {
        assert!(
            sim.map_cell_in_bounds((x as i16, y as i16)),
            "{name}: native map bounds"
        );
        sim.spawn_object_at_height(name, country, x, y, 0, 0, &rules)
            .expect("completed scene prior");
    }
    if with_plant {
        for (name, x, y) in [("NAPOWR", 26, 22), ("NAINDP", 22, 26)] {
            assert!(sim.map_cell_in_bounds((x as i16, y as i16)));
            sim.spawn_object_at_height(name, "Russians", x, y, 0, 0, &rules)
                .unwrap();
        }
    }
    for _ in 0..120 {
        frame(&mut sim, &rules, owner, None);
    }
    assert!(
        sim.fog.is_cell_revealed(owner, 13, 18),
        "base revealed through real sight"
    );
    Some((sim, rules, owner, target))
}

#[test]
fn transferred_held_infantry_uses_its_owner_for_rates_and_survives_restore() {
    let Some((mut sim, rules, owner, target)) = stock_world(false) else {
        return;
    };
    let pile1 = paid_place(&mut sim, &rules, owner, "GAPILE", 18, 18);
    let weap1 = paid_place(&mut sim, &rules, owner, "GAWEAP", 14, 12);
    let tank = queue_hold(&mut sim, &rules, owner, "MTNK", ProductionCategory::Vehicle);
    let infantry = queue_hold(&mut sim, &rules, owner, "E1", ProductionCategory::Infantry);
    let weap2 = paid_place(&mut sim, &rules, owner, "GAWEAP", 14, 15);
    let pile2 = paid_place(&mut sim, &rules, owner, "GAPILE", 14, 7);
    let registry: Vec<_> = (0..sim.substrate.entities.techno_registry_len())
        .map(|i| sim.substrate.entities.techno_registry_at(i).unwrap())
        .collect();
    let suffix = [pile1, weap1, tank, infantry, weap2, pile2];
    let start = registry.iter().position(|&id| id == pile1).unwrap();
    assert_eq!(
        &registry[start..],
        &suffix,
        "actual Techno construction suffix"
    );
    transfer(&mut sim, &rules, owner, target);
    assert!(
        sim.substrate.entities.get(tank).is_none(),
        "last vehicle producer abandons earlier held tank"
    );
    assert_eq!(
        sim.substrate.entities.get(pile2).unwrap().owner(),
        owner,
        "compaction skips last barracks"
    );
    assert_eq!(
        sim.substrate.entities.get(infantry).unwrap().owner(),
        target
    );
    assert!(
        sim.substrate
            .entities
            .get(infantry)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert_eq!(held(&sim, owner, ProductionCategory::Infantry), infantry);
    for _ in 0..60 {
        frame(&mut sim, &rules, owner, None);
    }
    let view = sim
        .production
        .factories()
        .view(owner, ProductionCategory::Infantry)
        .expect("skipped producer retains queue after normal assessment");
    let cached = sim
        .production
        .factories()
        .iter_insertion_ordered()
        .into_iter()
        .find(|f| f.owner() == owner && f.category() == ProductionCategory::Infantry)
        .unwrap();
    let cached_before = cached.step_rate_frames();
    let live_before = view.build_step_rate(&sim, &rules);
    assert_eq!(sim.substrate.entities.get(pile2).unwrap().owner(), owner);
    assert!(
        !sim.substrate
            .entities
            .get(pile2)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert!(view.suspended);
    let bytes = GameSnapshot::save(&sim, 0, rules.simulation_config_hash(), "held-owner", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
    restored.restore_after_snapshot_load().unwrap();
    restored.resolve_type_handles(&rules);
    crate::sim::production::validate_restored_factory_state(&restored, &rules)
        .expect("admit actually reachable dual-owner Factory");
    assert_eq!(restored.production.factories(), sim.production.factories());
    assert_eq!(
        restored.substrate.entities.get(infantry).unwrap().owner(),
        target
    );
    let mut sim = restored;
    let type_id = sim.interner.get("E1").unwrap();
    let f_spent = sim.houses[&owner].economy().spent_credits();
    let t_spent = sim.houses[&target].economy().spent_credits();
    frame(
        &mut sim,
        &rules,
        owner,
        Some(Command::QueueProduction { type_id }),
    );
    let resumed = sim
        .production
        .factories()
        .iter_insertion_ordered()
        .into_iter()
        .find(|f| f.owner() == owner && f.category() == ProductionCategory::Infantry)
        .unwrap()
        .step_rate_frames();
    for _ in 0..8 {
        frame(&mut sim, &rules, owner, None);
    }
    assert_eq!(resumed, live_before);
    assert!(sim.houses[&owner].economy().spent_credits() > f_spent);
    assert_eq!(sim.houses[&target].economy().spent_credits(), t_spent);
    // GoOffline452403 supplies a system hold; GoOnline452337 reaches the
    // automatic Resume509287 without changing the product or its debt.
    let balance = sim
        .production
        .factories()
        .iter_insertion_ordered()
        .into_iter()
        .find(|f| f.owner() == owner && f.category() == ProductionCategory::Infantry)
        .unwrap()
        .balance();
    assert!(sim.go_building_offline(pile2, &rules, FrameEffects::default()));
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap()
            .suspended
    );
    assert!(sim.go_building_online(pile2, &rules, FrameEffects::default()));
    let resumed = sim
        .production
        .factories()
        .iter_insertion_ordered()
        .into_iter()
        .find(|f| f.owner() == owner && f.category() == ProductionCategory::Infantry)
        .unwrap();
    assert!(!resumed.suspended());
    assert_eq!(resumed.step_rate_frames(), live_before);
    assert_eq!(resumed.balance(), balance);
    assert_eq!(held(&sim, owner, ProductionCategory::Infantry), infantry);
    frame(
        &mut sim,
        &rules,
        owner,
        Some(Command::CancelProductionByType { type_id, all: true }),
    );
    assert!(sim.substrate.entities.get(infantry).is_none());
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .is_none()
    );
    assert_eq!(
        cached_before, live_before,
        "native4CA6E0 calls held Techno6F47A0 with its actual owner"
    );
}

fn transfer(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, target: InternedId) {
    let selector = i32::from(rules.trigger_house_type_index("Russians").unwrap().0);
    assert_eq!(sim.house_by_trigger_selector(selector, rules), Some(target));
    install_event_ingress_map(
        sim,
        rules,
        &format!(
            "[Triggers]\nSHIFT=Americans,<none>,Shift,0,1,1,1,0\n[Tags]\nTAG=2,Shift,SHIFT\n[Events]\nSHIFT=1,27,0,7\n[Actions]\nSHIFT=1,36,0,{selector},0,0,0,0,A\n"
        ),
    );
    let tag_type = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
    let tag = sim.shared_trigger_tag(tag_type);
    let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
    let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
    let action = sim.trigger_runtime.definitions.trigger_types[typ].actions[0].clone();
    assert_eq!((action.entry.kind, action.value), (36, Some(selector)));
    let context = sim.trigger_action_context(typ, Some(live), TagEventInput::default(), rules);
    assert_eq!(context.incoming_house, Some(owner));
    assert!(sim.apply_live_trigger_action(
        &action,
        context,
        &mut Vec::new(),
        rules,
        None,
        FrameEffects::default()
    ));
}

#[test]
fn transferred_held_tank_refund_values_its_owner_but_credits_factory_house() {
    let Some((mut sim, rules, owner, target)) = stock_world(true) else {
        return;
    };
    let _early_pile = paid_place(&mut sim, &rules, owner, "GAPILE", 18, 18);
    let weap1 = paid_place(&mut sim, &rules, owner, "GAWEAP", 14, 12);
    let pile1 = paid_place(&mut sim, &rules, owner, "GAPILE", 14, 7);
    let infantry = queue_hold(&mut sim, &rules, owner, "E1", ProductionCategory::Infantry);
    let tank = queue_hold(&mut sim, &rules, owner, "MTNK", ProductionCategory::Vehicle);
    let pile2 = paid_place(&mut sim, &rules, owner, "GAPILE", 16, 5);
    let weap2 = paid_place(&mut sim, &rules, owner, "GAWEAP", 14, 15);
    let registry: Vec<_> = (0..sim.substrate.entities.techno_registry_len())
        .map(|i| sim.substrate.entities.techno_registry_at(i).unwrap())
        .collect();
    let start = registry.iter().position(|&id| id == weap1).unwrap();
    let suffix = [weap1, pile1, infantry, tank, pile2, weap2];
    assert_eq!(&registry[start..], &suffix);
    transfer(&mut sim, &rules, owner, target);
    assert!(sim.substrate.entities.get(infantry).is_none());
    assert_eq!(sim.substrate.entities.get(weap2).unwrap().owner(), owner);
    assert_eq!(sim.substrate.entities.get(tank).unwrap().owner(), target);
    assert_eq!(held(&sim, owner, ProductionCategory::Vehicle), tank);
    for _ in 0..60 {
        frame(&mut sim, &rules, owner, None);
    }
    let f = sim
        .production
        .factories()
        .iter_insertion_ordered()
        .into_iter()
        .find(|f| f.owner() == owner && f.category() == ProductionCategory::Vehicle)
        .unwrap();
    let balance = f.balance();
    let f_cost = sim.cost_of(owner, rules.object("MTNK").unwrap(), &rules);
    let t_cost = sim.cost_of(target, rules.object("MTNK").unwrap(), &rules);
    assert!(
        t_cost < f_cost,
        "stock Industrial Plant supplies the owner cost contrast"
    );
    assert!(
        t_cost < balance,
        "the retained debt makes this a negative refund"
    );
    let f_money = sim.houses[&owner].economy().credits();
    let t_money = sim.houses[&target].economy().credits();
    assert!(crate::sim::production::cancel_production_for_owner(
        &mut sim,
        &rules,
        "Americans",
        crate::sim::production::ProductionCancellation::Type {
            type_id: "MTNK",
            naval: false
        },
        true,
        FrameEffects::default()
    ));
    let actual = sim.houses[&owner].economy().credits().wrapping_sub(f_money);
    assert!(sim.substrate.entities.get(tank).is_none());
    assert_eq!(sim.houses[&target].economy().credits(), t_money);
    assert_eq!(
        actual,
        t_cost.wrapping_sub(balance),
        "native4CA02E values held-owner CostOf but4CA040 credits Factory owner"
    );
}
