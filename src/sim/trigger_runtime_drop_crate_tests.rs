//! Trigger action 600 (`TActionExt::SetDropCrate`,
//! `Ext/TAction/Body.cpp:690-720`) through the production action dispatch and
//! the production destruction transaction. `TechnoExt::GetDropCrateIndex`
//! (`Ext/Techno/Body.Internal.cpp:275-299`) reads the instance fields the
//! action writes.
use super::*;

const RULES: &str = "\
[InfantryTypes]
[VehicleTypes]
0=GIFT
[AircraftTypes]
[BuildingTypes]
[CrateRules]
CrateImg=SILVER
WoodCrateImg=WOOD
WaterCrateImg=WATER
[GIFT]
Name=Gift
Strength=100
";

fn fixture(gift_keys: &str, actions: &str) -> (Simulation, crate::rules::ruleset::RuleSet) {
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::sim::arena_fixture::clear_ground_ini(&format!("{RULES}{gift_keys}\n")),
    )
    .expect("drop-crate rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 11);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    super::tests::install_event_ingress_map(
        &mut sim,
        &rules,
        &format!(
            "[Triggers]\nT=Neutral,<none>,T,0,1,1,1,0\n[Actions]\nT={actions}\n[Tags]\nTAG=2,Tag,T\n"
        ),
    );
    (sim, rules)
}

/// Apply the single action the fixture's trigger `T` carries.
fn fire(sim: &mut Simulation, rules: &crate::rules::ruleset::RuleSet) {
    let Some(trigger_type) = sim
        .trigger_runtime
        .definitions
        .trigger_types
        .iter()
        .position(|typ| typ.id == "T")
    else {
        panic!("the fixture map has trigger T");
    };
    let Some(live) = sim
        .trigger_runtime
        .trigger_order
        .iter()
        .copied()
        .find(|&live| {
            sim.trigger_runtime
                .trigger(live)
                .is_some_and(|trigger| trigger.type_index == trigger_type)
        })
    else {
        panic!("trigger T has a live instance");
    };
    let action = sim.trigger_runtime.definitions.trigger_types[trigger_type].actions[0].clone();
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: Some(live),
            incoming_house: None,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        rules,
        None,
        FrameEffects::default(),
    );
}

fn crates(sim: &Simulation) -> Vec<(u16, u16, u8)> {
    let grid = sim.overlay_grid.as_ref().unwrap();
    let mut placed = Vec::new();
    for x in 0..32u16 {
        for y in 0..32u16 {
            let cell = grid.cell(x, y);
            if cell.overlay_id.is_some() {
                placed.push((x, y, cell.overlay_data));
            }
        }
    }
    placed
}

fn registry() -> crate::rules::overlay_types::OverlayTypeRegistry {
    crate::sim::crates::tests::crate_registry()
}

fn destroy(sim: &mut Simulation, rules: &crate::rules::ruleset::RuleSet, id: u64) {
    let registry = registry();
    sim.record_the_kill(
        id,
        crate::sim::combat::KillSource::Techno(None),
        crate::sim::combat::KillCallback::Terminal,
        crate::sim::world::UninitContext::new(rules, Some(&registry)),
    );
}

/// `TActionExt::SetDropCrate`: the action writes `DropCrate = 1` plus
/// `DropCrateType = Param3` on every techno whose tag chain lists a trigger
/// typed like the firing one, so a type without `DropCrate=` still leaves the
/// action's crate.
#[test]
fn action_600_retypes_the_crate_of_a_tagged_techno() {
    let powerup = crate::rules::powerups::POWERUP_HEAL_BASE as i32;
    let (mut sim, rules) = fixture("", &format!("1,600,0,1,{powerup},0,0,0,0,A"));
    let victim = sim
        .spawn_object("GIFT", "Americans", 10, 10, 0, &rules)
        .expect("victim");
    sim.attach_object_trigger_tag(victim, Some("TAG"));

    fire(&mut sim, &rules);

    assert_eq!(
        sim.substrate
            .entities
            .get(victim)
            .unwrap()
            .phobos_drop_crate(),
        1
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(victim)
            .unwrap()
            .phobos_drop_crate_type(),
        powerup
    );

    destroy(&mut sim, &rules, victim);

    let placed = crates(&sim);
    assert_eq!(placed.len(), 1, "one crate: {placed:?}");
    assert_eq!(
        placed[0].2, powerup as u8,
        "the crate carries the action's slot: {placed:?}"
    );
}

/// `DropCrate = 0` is the action's "disable" arm: a type that authors
/// `DropCrate=` leaves nothing once its tag's trigger fires that value.
#[test]
fn action_600_with_zero_suppresses_the_type_key() {
    let (mut sim, rules) = fixture("DropCrate=Money", "1,600,0,0,0,0,0,0,0,A");
    let victim = sim
        .spawn_object("GIFT", "Americans", 10, 10, 0, &rules)
        .expect("victim");
    sim.attach_object_trigger_tag(victim, Some("TAG"));

    // The type key alone would place a crate.
    assert!(
        rules
            .object_in_category(crate::rules::object_type::ObjectCategory::Vehicle, "GIFT")
            .expect("GIFT type")
            .phobos_techno_extras
            .drop_crate
            .is_some(),
        "the rules reader saw DropCrate=Money"
    );

    fire(&mut sim, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(victim)
            .unwrap()
            .phobos_drop_crate(),
        0
    );

    destroy(&mut sim, &rules, victim);

    let placed = crates(&sim);
    assert!(
        placed.is_empty(),
        "the override suppresses the crate: {placed:?}"
    );
}

/// A techno outside the firing trigger's tag chain keeps its type key: the
/// action's scan is the `AttachedTag` chain, not every techno.
#[test]
fn action_600_leaves_an_untagged_techno_on_its_type_key() {
    let (mut sim, rules) = fixture("DropCrate=Money", "1,600,0,1,2,0,0,0,0,A");
    let tagged = sim
        .spawn_object("GIFT", "Americans", 10, 10, 0, &rules)
        .expect("tagged victim");
    let plain = sim
        .spawn_object("GIFT", "Americans", 20, 20, 0, &rules)
        .expect("plain victim");
    sim.attach_object_trigger_tag(tagged, Some("TAG"));

    fire(&mut sim, &rules);

    assert_eq!(
        sim.substrate
            .entities
            .get(tagged)
            .unwrap()
            .phobos_drop_crate(),
        1
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(plain)
            .unwrap()
            .phobos_drop_crate(),
        -1,
        "the untagged object keeps the unset instance value"
    );
}
