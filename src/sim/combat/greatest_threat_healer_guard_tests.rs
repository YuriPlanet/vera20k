//! The healer-on-Guard radius (`0x006F9024..0x006F903E`): a scanner whose
//! `GetWeaponDamageValue(-1)` (`0x006F3970`) is negative scans a hard 0x200
//! leptons while its mission is Guard, through the production scan owner on
//! retail rules. Retail's healer is the IFV (`[FV]`) carrying an Engineer:
//! the Engineer's `IFVMode=` selects `Weapon2=RepairBullet` (`Damage=-50`,
//! `Range=1.8`).

use super::*;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::mission::{MissionId, MissionType};
use crate::util::fixed_math::SimFixed;

/// A repair IFV at (16, 16) on `mission` and a damaged allied Grizzly
/// `offset` leptons (east, south) from it. Answers what the IFV's Guard-mask
/// scan picks and the Grizzly.
fn repair_scan(rules: &RuleSet, offset: (i32, i32), mission: MissionType) -> (Option<u64>, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let ifv = sim
        .spawn_object("FV", "Americans", 16, 16, 64, rules)
        .expect("IFV");
    let (east, south) = (128 + offset.0, 128 + offset.1);
    let tank = sim
        .spawn_object(
            "MTNK",
            "Americans",
            16 + (east / 256) as u16,
            16 + (south / 256) as u16,
            64,
            rules,
        )
        .expect("Grizzly");
    let mode = rules.object("ENGINEER").expect("ENGINEER").ifv_mode;
    let ifv_type = rules.object("FV").expect("FV");
    sim.substrate
        .entities
        .get_mut(ifv)
        .unwrap()
        .set_gunner_weapon(mode, ifv_type);
    let grizzly = sim.substrate.entities.get_mut(tank).unwrap();
    grizzly
        .position
        .set_sub_x_for_test(SimFixed::from_num(east % 256));
    grizzly
        .position
        .set_sub_y_for_test(SimFixed::from_num(south % 256));
    grizzly.health.set_for_test(grizzly.health.current() / 2);
    let frame = sim.session.binary_frame();
    sim.substrate
        .mission_assign_exact(ifv, MissionId::from_known(mission), frame)
        .unwrap();
    let pick = sim.greatest_threat_represented(rules, None, ifv, ScanMission::Guard, None);
    (pick, tank)
}

/// 0x200 leptons truncate to a two-cell walk bound, and the ring walk stops
/// before the bound (`INC EDI; CMP EDI,ECX; JGE` at `0x006F9B37..0x006F9B3E`):
/// on Guard the IFV sees only rings 0 and 1. A Grizzly 384 leptons east
/// stands in ring 2, inside RepairBullet's 1.8 cells, and is not taken. One
/// in the diagonal ring-1 cell 489 leptons away, past RepairBullet's range
/// but inside 0x200, is: a positive radius skips InRange (`JNZ 0x006F81BE`
/// at `0x006F8178`). Off Guard (Move, whose AI scan pushes the same mask 1)
/// the walk bound is RepairBullet's cell, AirRangeBonus and one more, and
/// InRange decides.
#[test]
fn a_guarding_repair_ifv_walks_only_the_adjacent_cells() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (pick, tank) = repair_scan(rules, (256, 0), MissionType::Guard);
    assert_eq!(pick, Some(tank), "Guard: ring 1");
    let (pick, _) = repair_scan(rules, (384, 0), MissionType::Guard);
    assert_eq!(pick, None, "Guard: ring 2 is past the walk");
    let (pick, tank) = repair_scan(rules, (346, 346), MissionType::Guard);
    assert_eq!(pick, Some(tank), "Guard: the cutoff, not InRange");
    let (pick, _) = repair_scan(rules, (346, 346), MissionType::Move);
    assert_eq!(pick, None, "Move: InRange");
    let (pick, tank) = repair_scan(rules, (384, 0), MissionType::Move);
    assert_eq!(pick, Some(tank), "Move: inside RepairBullet's range");
    let (pick, _) = repair_scan(rules, (480, 0), MissionType::Move);
    assert_eq!(pick, None, "Move: past RepairBullet's range");
}
