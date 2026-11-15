//! `[General] AIAirTargetingFix=` (`Ext/Rules/Body.cpp:452`,
//! `Body.h:362,867`, constructed `false`), through the production Hunt scan.
//!
//! Native's flat `SelectAutoTarget` walk gates airborne objects twice:
//!
//! - the aircraft pass runs only when the flags word already carries bit 4
//!   (`MOV AL,[ESP+0x14] ; TEST AL,0x4 ; JZ 0x006F9C56` at `0x006F9B7E`), and
//!   that bit is only ORed into the word later, at `0x006F9C5D`, from the
//!   scanner's weapon class bits, so a weapon whose class bits carry `0x10`
//!   but not `0x4` (anti-ground `0xB8`) never enters the aircraft pass;
//! - the object gate admits only `LastLayer == Ground`
//!   (`CMP [EDI+0x94],0x2 ; JNZ 0x006F9D93` at `0x006F9D13`).
//!
//! Between the two, an AI unit with an anti-ground weapon cannot hold an
//! aircraft at all. Yes remaps `0x006F9B7E` onto `0x006F9C56`
//! (`Misc/Hooks.BugFixes.cpp:3074-3077`) and replaces the object gate with
//! `canTargetRtti & 4 ? LastLayer != Underground : LastLayer == Ground`
//! (`:3079-3097`), so the single Techno walk carries the airborne target.

use crate::sim::combat::TargetKind;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;
use crate::sim::world::display_layers::DisplayLayer;

/// A hunter whose weapon carries neither AA nor AG — `PLAINHUNT` — reaches
/// the same widened word, because VERA's class-bit reader treats a weapon
/// whose `Projectile` section does not resolve as anti-ground
/// (`combat_weapon::weapon_class_bits` defaults to `(false, true)`). The
/// native clear-bit arm (`canTargetRtti & 4` false, stock's Ground-only test)
/// is therefore not reachable from these fixtures; see the doc's residuals.
const EXTRA: &str = "[VehicleTypes]\n1=AGHUNT\n\
     [AGHUNT]\nStrength=200\nSpeed=6\nArmor=none\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nPrimary=AGGun\n\
     [AGGun]\nDamage=20\nRange=8\nROF=60\nProjectile=TestShot\nWarhead=TestWH\nAG=yes\n\
     [TestShot]\nROT=0\n[TestWH]\nVerses=100%\n";

/// A flat arena with the hunter on the ground at (8, 10) and a hostile
/// `HORNET` airborne at Z 64 three cells east, inside the weapon's range.
fn hunt_scene(fix: bool, hunter: &str) -> (Option<u64>, u64, Option<DisplayLayer>) {
    let (mut sim, mut rules, _) = crate::sim::world::entry_test_fixture::fixture_with_rules(EXTRA);
    rules.phobos_globals.ai_air_targeting_fix = fix;
    for (index, name) in ["Americans", "Russians"].into_iter().enumerate() {
        let owner = sim.intern(name);
        sim.houses.get_or_register_for_test(owner, || {
            HouseState::new(owner, index as u8, None, false, 0, 10)
        });
    }
    let hunter_id = sim
        .spawn_object(hunter, "Americans", 8, 10, 0, &rules)
        .expect("hunter on the ground");
    let aircraft = sim
        .spawn_object_at_height("HORNET", "Russians", 11, 10, 0, 64, &rules)
        .expect("aircraft");
    // A freshly constructed aircraft is still on the Ground layer; its takeoff
    // makes the Display re-submit it to Air. Stand in for that re-submission:
    // the gate under test reads `LastLayer`, which the Display owns.
    Simulation::submit_overhead_display(&mut sim.substrate, aircraft, DisplayLayer::AIR);
    let layer = sim.display_layers().layer_of(aircraft);
    let held = sim
        .greatest_threat_represented(&rules, None, hunter_id, ScanMission::Hunt, None)
        .and_then(TargetKind::object_id);
    (held, aircraft, layer)
}

/// The airborne aircraft is a non-Ground object, as the gate reads it.
#[test]
fn the_fixture_aircraft_is_not_on_the_ground_layer() {
    let (_, _, layer) = hunt_scene(false, "AGHUNT");
    assert!(
        matches!(layer, Some(DisplayLayer::AIR | DisplayLayer::TOP)),
        "an airborne aircraft is not on Ground, got {layer:?}"
    );
}

/// The replacement gate turns on `canTargetRtti & 4`; the anti-ground hunter's
/// class bits (`0xB8`) carry `0x10` but not `4`, so only the fix's widening
/// reaches the airborne aircraft.
#[test]
fn an_anti_ground_hunter_holds_an_airborne_target_only_with_the_fix() {
    let (stock, _, _) = hunt_scene(false, "AGHUNT");
    assert_eq!(stock, None, "stock refuses every non-Ground LastLayer");

    let (fixed, aircraft, _) = hunt_scene(true, "AGHUNT");
    assert_eq!(
        fixed,
        Some(aircraft),
        "the widened word admits the aircraft"
    );
}
