//! Phobos' gattling selection hook at `0x6F3432`
//! (`Ext/Techno/Hooks.Firing.cpp:204-283`): `PickWeaponIndex` over the stage
//! pair with both fallback gates on, then the four `isWeaponValid` rows, the
//! naval selector and `LandTargeting`. The stock ladder only answers
//! `2*stage + (slot-1 projectile AA && high flier)`.

use super::TargetKind;
use super::combat_weapon;
use crate::rules::ruleset::RuleSet;
use crate::sim::arena_fixture;
use crate::sim::world::Simulation;

/// Four-slot gattling type `GAT`, a plain heavy-armed `PLAIN`, a shielded
/// `SHIELDED` and an aircraft `PLANE`. `weapons` overrides the `WeaponN=` block
/// and `extra` appends type keys such as `LandTargeting=`.
fn gattling_rules(weapons: &str, extra: &str) -> RuleSet {
    let ini = arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=GAT\n1=PLAIN\n2=SHIELDED\n\
         [AircraftTypes]\n0=PLANE\n\
         [ShieldTypes]\n0=Weak\n\
         [Weak]\nStrength=500\nArmor=none\n\
         [GAT]\nStrength=200\nCost=300\nSpeed=6\nArmor=heavy\n\
         IsGattling=yes\nTurretCount=1\nWeaponCount=4\n{weapons}{extra}\
         [PLAIN]\nStrength=200\nCost=300\nSpeed=6\nArmor=heavy\n\
         [SHIELDED]\nStrength=200\nCost=300\nSpeed=6\nArmor=heavy\nShieldType=Weak\n\
         [PLANE]\nStrength=200\nArmor=light\nSpeed=10\nSight=8\n\
         [GroundGun]\nDamage=10\nROF=50\nRange=6\nProjectile=GroundShot\nWarhead=NarrowWH\n\
         [AirGun]\nDamage=10\nROF=50\nRange=6\nProjectile=AirShot\nWarhead=AnyWH\n\
         [AnyGun]\nDamage=10\nROF=50\nRange=6\nProjectile=GroundShot\nWarhead=AnyWH\n\
         [GroundShot]\nAA=no\nAG=yes\n[AirShot]\nAA=yes\nAG=no\n\
         [NarrowWH]\nVerses=0%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n\
         [AnyWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ));
    RuleSet::from_ini(&ini).expect("gattling selection rules")
}

const STAGE_PAIR: &str = "Weapon1=GroundGun\nWeapon2=AirGun\nWeapon3=AirGun\nWeapon4=GroundGun\n";
const ALL_VALID: &str = "Weapon1=AnyGun\nWeapon2=AnyGun\nWeapon3=AirGun\nWeapon4=GroundGun\n";

fn selected_index(sim: &Simulation, rules: &RuleSet, attacker: u64, target: u64) -> i32 {
    let entity = sim.entities().get(attacker).expect("attacker entity");
    let object = rules.object("GAT").expect("gattling type");
    combat_weapon::select_weapon_against(
        rules,
        entity,
        object,
        &combat_weapon::attacker_facts(entity, object),
        Some(&TargetKind::Entity(target)),
        sim,
        &sim.interner,
        sim.resolved_terrain.as_ref(),
        Some(&sim.house_alliances),
    )
    .expect("a weapon index")
    .0
}

fn arena(rules: &RuleSet, target_type: &str, seed: u64) -> (Simulation, u64, u64) {
    let mut sim = arena_fixture::rival_arena(rules, seed);
    let attacker = sim
        .spawn_object("GAT", "Americans", 8, 10, 0, rules)
        .expect("gattling attacker spawns");
    let target = sim
        .spawn_object(target_type, "Russians", 12, 10, 0, rules)
        .expect("target spawns");
    (sim, attacker, target)
}

/// `CurrentGattlingStage = 1` asks the pair 2/3. The stock arm would answer 3
/// because slot 1's projectile is AA and the target is airborne; the hook's AA
/// row refuses slot 3 (`AA=no`) and answers the AA-carrying slot 2.
#[test]
fn the_gattling_arm_reads_the_stage_pair_not_the_slot1_projectile() {
    let rules = gattling_rules(STAGE_PAIR, "");
    let (mut sim, attacker, target) = arena(&rules, "PLANE", 31);
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(1024));
    sim.substrate
        .entities
        .get_mut(attacker)
        .unwrap()
        .gattling
        .set_stage(1);
    assert_eq!(
        selected_index(&sim, &rules, attacker, target),
        2,
        "slot 3 lacks AA, so the stage pair's AA slot answers"
    );
}

/// The shield row: slot 0's warhead is refused by the target's active shield,
/// so the otherwise ordinary slot 1 answers.
#[test]
fn the_gattling_arm_hands_the_shot_to_the_slot_the_shield_accepts() {
    let rules = gattling_rules(STAGE_PAIR, "");
    let (sim, attacker, target) = arena(&rules, "SHIELDED", 32);
    assert!(
        sim.entities().get(target).unwrap().shield().is_some(),
        "the shielded type carries a live shield"
    );
    assert_eq!(selected_index(&sim, &rules, attacker, target), 1);
}

/// The raw-armor Verses row, with no shield involved: slot 0's warhead does
/// nothing to the target's own heavy armor.
#[test]
fn the_gattling_arm_refuses_a_zero_verses_slot() {
    let rules = gattling_rules(STAGE_PAIR, "");
    let (sim, attacker, target) = arena(&rules, "PLAIN", 33);
    assert!(sim.entities().get(target).unwrap().shield().is_none());
    assert_eq!(selected_index(&sim, &rules, attacker, target), 1);
}

/// `LandTargeting=Land_Secondary` answers the even slot once both slots pass
/// the validity rows.
#[test]
fn land_secondary_answers_the_gattling_even_slot() {
    let rules = gattling_rules(ALL_VALID, "LandTargeting=2\n");
    let (sim, attacker, target) = arena(&rules, "PLAIN", 34);
    assert_eq!(selected_index(&sim, &rules, attacker, target), 1);
}
