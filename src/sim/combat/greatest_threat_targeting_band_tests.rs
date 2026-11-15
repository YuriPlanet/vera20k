//! `[General] DisableOveroptimizationInTargeting=` (`Ext/Rules/Body.cpp:531`,
//! `Body.h:440,951`, constructed `false`), through the production Guard scan.
//!
//! Native's cell ring walk returns as soon as a ring that held a candidate is
//! the `radius/4` or the `radius/2` band (`MOV EAX,[ESP+0x38] ; CDQ ; AND
//! EDX,0x3 ; ... ; SAR EAX,0x2`, then the halved form, `0x006F9AE2..0x006F9B0A`).
//! Yes jumps past the whole band test to `0x006F9B1B`
//! (`Ext/Techno/Hooks.Targeting.cpp:88-92`), so a ring walk reaches its radius
//! even when a band held a worse candidate first.

use crate::sim::combat::TargetKind;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::house_state::HouseState;

/// A `SCOUTCAR` guarding from (8, 8) with a 12-cell weapon — the walk bound is
/// 13 cells, so the bands are rings 3 and 6 — a nearly harmless `WEAKCAR` on
/// ring 3 and a heavier `STRONGCAR` on ring 4. The one `[General]` coefficient
/// makes the armed car score above the unarmed one: the fixture's own rules
/// leave every threat coefficient at 0, which would tie every candidate.
const EXTRA: &str = "[General]\nTargetEffectivenessCoefficientDefault=1\n\
     [VehicleTypes]\n0=SCOUTCAR\n1=WEAKCAR\n2=STRONGCAR\n\
     [SCOUTCAR]\nStrength=500\nSpeed=6\nArmor=none\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nPrimary=GuardGun\n\
     [GuardGun]\nDamage=10\nRange=12\nROF=60\nProjectile=TestShot\nWarhead=TestWH\n\
     [TestShot]\nROT=0\n[TestWH]\nVerses=100%\n\
     [WEAKCAR]\nStrength=60\nSpeed=6\nArmor=none\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [STRONGCAR]\nStrength=600\nSpeed=6\nArmor=none\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nPrimary=HugeGun\n\
     [HugeGun]\nDamage=400\nRange=6\nROF=60\nProjectile=TestShot\nWarhead=TestWH\n";

/// What the guarding car holds with the switch on or off, plus both targets.
fn guard_scene(disable_optimization: bool) -> (Option<u64>, u64, u64) {
    let (mut sim, mut rules, _) = crate::sim::world::entry_test_fixture::fixture_with_rules(EXTRA);
    rules.phobos_globals.disable_overoptimization_in_targeting = disable_optimization;
    for (index, name) in ["Americans", "Russians"].into_iter().enumerate() {
        let owner = sim.intern(name);
        sim.houses.get_or_register_for_test(owner, || {
            HouseState::new(owner, index as u8, None, false, 0, 10)
        });
    }
    let scanner = sim
        .spawn_object("SCOUTCAR", "Americans", 8, 8, 0, &rules)
        .expect("scanner");
    let weak = sim
        .spawn_object("WEAKCAR", "Russians", 11, 8, 0, &rules)
        .expect("weak car on ring 3");
    let strong = sim
        .spawn_object("STRONGCAR", "Russians", 12, 8, 0, &rules)
        .expect("strong car on ring 4");
    let held = sim
        .greatest_threat_represented(&rules, None, scanner, ScanMission::Guard, None)
        .and_then(TargetKind::object_id);
    (held, weak, strong)
}

/// Stock stops at the `radius/4` band holding the nearly harmless car; the
/// switch walks on to ring 4 and takes the heavy one.
#[test]
fn the_band_return_stops_the_ring_walk_unless_disabled() {
    let (held, weak, _) = guard_scene(false);
    assert_eq!(held, Some(weak), "ring 3 is the radius/4 band return");

    let (held, _, strong) = guard_scene(true);
    assert_eq!(held, Some(strong), "the walk continues past ring 3");
}
