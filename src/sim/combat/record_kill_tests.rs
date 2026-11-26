//! `TechnoClass::Record_The_Kill @ 0x00702D40` experience awarding through the
//! world entry ([`Simulation::record_the_kill`]).

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::{HouseCostFactors, RuleSet};
use crate::sim::combat::veterancy::{accumulate, kill_award_points};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;
use crate::util::native_x87::NativeF32Bits;

/// NPatch (NPExt bug-fix changelog item 55, issue #2981): a kill's veterancy
/// price reads each side's OWN house cost factors. Native Record_The_Kill
/// prices the recipient's divisor through the victim's house too
/// (`0x00702ED1`, `0x00702F13`, `0x00702F77`, `0x00702FD4`), so a victim
/// under cost modifiers — an industrial plant or a country discount — skews
/// every award made against it. Here the victim's country runs
/// `CostUnitsMult=0.5` while the killer's is neutral: the same TEST type must
/// divide the award by 1000 (the killer's own price), not 500 (the victim's).
#[test]
fn npatch_kill_award_prices_the_recipient_with_its_own_house() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TEST\n[TEST]\nCost=1000\n\
         [Countries]\n0=AMER\n1=RUSS\n[AMER]\nCostUnitsMult=0.5\n[RUSS]\n",
    ))
    .unwrap();
    rules.set_npatch_for_test(true);
    let mut sim = Simulation::new();
    let victim_owner = sim.interner.intern("AMER");
    let killer_owner = sim.interner.intern("RUSS");
    for (name, side) in [(victim_owner, 0), (killer_owner, 1)] {
        let mut house = HouseState::new(name, side, None, true, 0, 10);
        house.project_country_mults(&rules, &sim.interner);
        sim.houses.register(house);
    }
    let type_ref = sim.interner.intern("TEST");
    for (stable_id, owner) in [(1, victim_owner), (2, killer_owner)] {
        sim.substrate
            .entities
            .insert(GameEntity::new_at_frame_zero_for_test(
                stable_id,
                2,
                3,
                0,
                0,
                owner,
                Health::new(100),
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                true,
            ));
    }
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .health
        .set_for_test(0);
    sim.record_the_kill(
        1,
        KillSource::Techno(Some(2)),
        KillCallback::Terminal,
        crate::sim::world::UninitContext::new(&rules),
    );

    // The award base stays the victim-house price: 1000 * 0.5 = 500. Under
    // npatch the divisor is the killer-house price 1000, not the victim's 500.
    let house_cost = |owner| {
        let object = sim.interner.object_type(type_ref, &rules).unwrap();
        let factors: HouseCostFactors = sim.houses.get(&owner).unwrap().cost_factors();
        rules.cost_of(object, Some(&factors))
    };
    let victim_cost = house_cost(victim_owner);
    assert_eq!(victim_cost, 500, "the victim's country halves its price");
    let points = kill_award_points(victim_cost, victim_rank(&sim, 1), false);
    let expected = accumulate(
        NativeF32Bits::POSITIVE_ZERO,
        house_cost(killer_owner),
        points,
        rules.general.veteran_ratio,
        rules.general.veteran_cap,
    );
    let killer = sim.substrate.entities.get(2).unwrap();
    assert_eq!(
        killer.veterancy.raw(),
        expected,
        "the divisor is priced by the killer's own house"
    );
}

/// The rank the victim died at, sampled from its accumulator.
fn victim_rank(sim: &Simulation, victim_id: u64) -> crate::sim::combat::veterancy::VeterancyRank {
    sim.substrate
        .entities
        .get(victim_id)
        .unwrap()
        .veterancy
        .rank_kind()
}
