//! UpdateAnimation's depot arm (`0x00450A86..0x00450B34`) on retail GADEPT.
use super::*;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

/// A depot off its Repair mission that still holds SpecialAnimTwo (slot 11,
/// GADEPT_B, the lowered arm) raises it on its next Building AI:
/// SpecialAnimThree into slot 12 (GADEPT_C, or GADEPT_CD at or below
/// ConditionYellow), and slot 11 is cleared. Get_Mission (`0x005B3040`) reads
/// the current mission, else the queued one: a depot on Repair, or with no
/// current mission and Repair queued, keeps the arm; one on Guard with Repair
/// queued raises it. A Naval shipyard (GAYARD) keeps its ProductionAnim.
#[test]
fn a_depot_off_its_repair_mission_raises_its_arm() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    // Asset binding gives each anim its frame count; without one the store
    // refuses the type. The damaged depot also lights its damage fires.
    let mut rules = retail.rules.clone();
    let fires: Vec<String> = (rules.general.damage_fire_types.iter())
        .map(|anim| anim.name.clone())
        .collect();
    for name in [
        "GADEPT_B",
        "GADEPT_BD",
        "GADEPT_C",
        "GADEPT_CD",
        "GAYARD_D",
        "GAYARD_S3",
    ]
    .into_iter()
    .chain(fires.iter().map(String::as_str))
    {
        rules.bind_anim_frame_count_for_test(name, 100);
    }
    let rules = &rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x450A86);
    crate::sim::arena_fixture::flood(
        &mut sim,
        rules,
        32,
        (0..10).flat_map(|y| (20..=27).map(move |x| (x, y))),
    );
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let mut lowered = |name, x, y, slot| {
        let id = sim
            .spawn_object(name, "Americans", x, y, 0, rules)
            .expect("building");
        sim.clear_building_anim_slot(id, 12);
        assert!(
            sim.set_building_anim_slot(id, slot, false, false, 0, rules)
                .is_some()
        );
        id
    };
    let healthy = lowered("GADEPT", 2, 4, 11);
    let damaged = lowered("GADEPT", 8, 4, 11);
    let servicing = lowered("GADEPT", 14, 4, 11);
    let queued = lowered("GADEPT", 2, 12, 11);
    let waiting = lowered("GADEPT", 8, 12, 11);
    let shipyard = lowered("GAYARD", 21, 2, 8);
    let strength = rules.object("GADEPT").unwrap().strength;
    sim.substrate
        .entities
        .get_mut(damaged)
        .unwrap()
        .health
        .set(strength / 4);
    for (id, current, next) in [
        (
            servicing,
            MissionId::from_known(MissionType::Repair),
            MissionId::NONE,
        ),
        (
            queued,
            MissionId::from_known(MissionType::Guard),
            MissionId::from_known(MissionType::Repair),
        ),
        (
            waiting,
            MissionId::NONE,
            MissionId::from_known(MissionType::Repair),
        ),
        (
            shipyard,
            MissionId::from_known(MissionType::Guard),
            MissionId::NONE,
        ),
    ] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .mission
            .apply_test_fixture(MissionTestFixture {
                current,
                suspended: MissionId::NONE,
                queued: next,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(0),
            });
    }
    let anim = |sim: &Simulation, id: u64, slot: usize| {
        sim.entities().get(id).unwrap().building_art.anim_slots()[slot].map(|anim| {
            sim.interner()
                .resolve(sim.substrate.anims.get(anim).unwrap().type_id())
                .to_owned()
        })
    };
    let slots = |sim: &Simulation, id: u64| (anim(sim, id, 11), anim(sim, id, 12));
    let lowered_arm = (Some("GADEPT_B".to_owned()), None);
    let raised_arm = (None, Some("GADEPT_C".to_owned()));
    // These rows ask the UpdateAnimation owner directly: the Repair mission's
    // own exits, once the Building AI promotes or runs it, also restore it.
    for id in [servicing, waiting] {
        sim.update_depot_raise_anim(id, rules);
        assert_eq!(slots(&sim, id), lowered_arm);
    }
    sim.update_depot_raise_anim(queued, rules);
    assert_eq!(slots(&sim, queued), raised_arm);
    sim.update_depot_raise_anim(shipyard, rules);
    assert_eq!(anim(&sim, shipyard, 8).as_deref(), Some("GAYARD_D"));

    sim.advance_tick(&[], Some(rules), None, None, 67);
    assert_eq!(slots(&sim, healthy), raised_arm);
    assert_eq!(slots(&sim, damaged), (None, Some("GADEPT_CD".to_owned())));
}
