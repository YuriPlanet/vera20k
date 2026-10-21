//! `HouseClass::NotifyUnderAttack @ 0x004F93E0` ends each call by raising
//! event 6 on its House's Tags (`0x004F95D4..0x004F95FD`). Both building
//! callers run here through their production entries: the damage commit
//! (`0x0044297B`) and the Temporal warp start (`0x0071B0CA`).

use super::tests::{
    event_ingress_damage, event_ingress_fixture_with_rules, install_event_ingress_map,
    spawn_event_ingress_unit,
};
use super::*;

/// A building, an `Insignificant=` one, and a Temporal firer.
const RULES: &str = "[BuildingTypes]\n1=TAGHQ\n2=TAGPOST\n\
    [TAGHQ]\nStrength=1000\n[TAGPOST]\nStrength=1000\nInsignificant=yes\n\
    [InfantryTypes]\n2=TAGCLEG\n\
    [TAGCLEG]\nStrength=125\nSpeed=4\nPrimary=TagBeam\n\
    Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
    [TagBeam]\nDamage=8\nROF=120\nRange=5\nProjectile=TagInviso\nWarhead=TagChrono\n\
    [TagInviso]\nInviso=yes\n[Warheads]\n2=TagChrono\n[TagChrono]\nTemporal=yes\n";

/// Two House0 Tags (FIRST, then SECOND) and one House1 Tag. Each Trigger
/// joins event 6 to event 8 (any event), a House event that attaches the Tag
/// to its owner's House; only an event-6 raise satisfies both. Each action
/// sets its own global.
fn install(sim: &mut Simulation, rules: &crate::rules::ruleset::RuleSet, first_repeat: i32) {
    install_event_ingress_map(
        sim,
        rules,
        &format!(
            "[Triggers]\nFIRST=Country0,<none>,First,0,1,1,1,0\n\
             SECOND=Country0,<none>,Second,0,1,1,1,0\nOTHER=Country7,<none>,Other,0,1,1,1,0\n\
             [Tags]\nTAG1={first_repeat},First,FIRST\nTAG2=2,Second,SECOND\nTAG3=2,Other,OTHER\n\
             [Events]\nFIRST=2,6,0,0,8,0,0\nSECOND=2,6,0,0,8,0,0\nOTHER=2,6,0,0,8,0,0\n\
             [Actions]\nFIRST=1,28,0,11,0,0,0,0,A\nSECOND=1,28,0,12,0,0,0,0,A\n\
             OTHER=1,28,0,13,0,0,0,0,A\n"
        ),
    );
}

fn spawn(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    kind: &str,
    house: crate::sim::intern::InternedId,
    cell: (u16, u16),
) -> u64 {
    let owner = sim.interner.resolve(house).to_owned();
    sim.spawn_object_at_height(kind, &owner, cell.0, cell.1, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn raised(tags: &[usize]) -> Vec<(usize, i32)> {
    tags.iter().map(|&tag| (tag, 6)).collect()
}

/// A hit with a source on a significant building raises event 6 on each of
/// its owner's Tags, first to last, and on no other House's; an
/// `Insignificant=` building notifies nobody (`0x0044295C..0x00442964`).
#[test]
fn a_building_hit_raises_event6_on_its_house_tags_in_order() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture_with_rules(RULES);
    let hq = spawn(&mut sim, &rules, "TAGHQ", houses[0], (15, 15));
    let post = spawn(&mut sim, &rules, "TAGPOST", houses[0], (19, 15));
    let source = spawn_event_ingress_unit(&mut sim, &rules, houses[1], (17, 18));
    install(&mut sim, &rules, 2);
    let own = sim.trigger_runtime.house_tags[&houses[0]].clone();
    assert_eq!(own.len(), 2);
    assert_eq!(sim.trigger_runtime.house_tags[&houses[1]].len(), 1);

    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        post,
        source,
        1,
        FrameEffects::default(),
    );
    assert!(sim.trigger_runtime.dispatch_trace.is_empty());

    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        hq,
        source,
        1,
        FrameEffects::default(),
    );
    assert_eq!(sim.trigger_runtime.dispatch_trace, raised(&own));
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([11, 12]));
}

/// The walk re-reads the list and its count for every slot (`0x004F95E3`,
/// `0x004F95F7`). A one-shot Tag that springs leaves the House's list at once
/// (`HouseClass::PointerExpired @ 0x004FB9B0` closes the gap), so the next
/// Tag moves into the slot just passed and waits for the next notice.
#[test]
fn a_tag_that_leaves_on_its_raise_passes_its_successor_by() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture_with_rules(RULES);
    let hq = spawn(&mut sim, &rules, "TAGHQ", houses[0], (15, 15));
    let source = spawn_event_ingress_unit(&mut sim, &rules, houses[1], (17, 18));
    install(&mut sim, &rules, 0);
    let [first, second] = sim.trigger_runtime.house_tags[&houses[0]][..] else {
        panic!("two House0 Tags");
    };

    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        hq,
        source,
        1,
        FrameEffects::default(),
    );
    assert_eq!(sim.trigger_runtime.dispatch_trace, raised(&[first]));
    assert_eq!(sim.trigger_runtime.house_tags[&houses[0]], [second]);
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([11]));

    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        hq,
        source,
        1,
        FrameEffects::default(),
    );
    assert_eq!(sim.trigger_runtime.dispatch_trace, raised(&[first, second]));
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([11, 12]));
}

/// The raise also meets the events whose predicate ignores the raised kind.
/// A House Trigger of plain conditions (event 12, funds of at least 0) that
/// the House's own event-8 raise skips on this frame (`Frame % 8 != 0`)
/// springs on the hit, as on stock campaign maps.
#[test]
fn a_hit_springs_a_house_condition_trigger_between_house_raises() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture_with_rules(RULES);
    let hq = spawn(&mut sim, &rules, "TAGHQ", houses[0], (15, 15));
    let source = spawn_event_ingress_unit(&mut sim, &rules, houses[1], (17, 18));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nFUNDS=Country0,<none>,Funds,0,1,1,1,0\n[Tags]\nTAG=0,Funds,FUNDS\n\
         [Events]\nFUNDS=1,12,0,0\n[Actions]\nFUNDS=1,28,0,21,0,0,0,0,A\n",
    );
    let [tag] = sim.trigger_runtime.house_tags[&houses[0]][..] else {
        panic!("one House0 Tag");
    };
    sim.session.set_binary_frame_for_test(3);
    sim.raise_house_tags(houses[0], &rules, Some(&overlays), FrameEffects::default());
    assert!(sim.trigger_runtime.dispatch_trace.is_empty());

    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        hq,
        source,
        1,
        FrameEffects::default(),
    );
    assert_eq!(sim.trigger_runtime.dispatch_trace, raised(&[tag]));
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([21]));
    assert!(sim.trigger_runtime.house_tags[&houses[0]].is_empty());
}

/// A Temporal warp that starts on a significant building notifies its House
/// (`0x0071B064..0x0071B0CA`); one on a Unit that is no harvester notifies
/// nobody.
#[test]
fn a_temporal_warp_start_on_a_building_raises_event6_on_its_house_tags() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture_with_rules(RULES);
    let hq = spawn(&mut sim, &rules, "TAGHQ", houses[0], (15, 15));
    let tank = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (19, 15));
    let [first, second] =
        [(15, 18), (17, 18)].map(|cell| spawn(&mut sim, &rules, "TAGCLEG", houses[1], cell));
    install(&mut sim, &rules, 2);
    let own = sim.trigger_runtime.house_tags[&houses[0]].clone();

    sim.temporal_initiate_warp(
        first,
        Some(tank),
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .temporal
            .chain_head(),
        Some(first)
    );
    assert!(sim.trigger_runtime.dispatch_trace.is_empty());

    sim.temporal_initiate_warp(
        second,
        Some(hq),
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(hq)
            .unwrap()
            .temporal
            .chain_head(),
        Some(second)
    );
    assert_eq!(sim.trigger_runtime.dispatch_trace, raised(&own));
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([11, 12]));
}
