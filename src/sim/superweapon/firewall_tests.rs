//! Ares `Type=Firestorm` (SW_Firewall, `src/Misc/SWTypes/Firewall.cpp`)
//! through the production rules reader, Grant, ClickFire and the building
//! receiver.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::command::Command;

fn rules() -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n\
         [BuildingTypes]\n0=FSWALL\n\
         [FSWALL]\nStrength=500\nFirestorm.Wall=yes\nTurret=no\nSight=0\n\
         [Warheads]\n0=C4WH\n\
         [C4WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [General]\nDamageToFirestormDamageCoefficient=2.5\n\
         [SuperWeaponTypes]\n0=Fire\n\
         [Fire]\nType=Firestorm\nRechargeTime=1\nIsPowered=no\nAction=Nuke\n\
         SW.CreateRadarEvent=yes\n",
    );
    let art = IniFile::from_str("[FSWALL]\nFoundation=1x1\n");
    let processed = RulesLayerStack::new(ini)
        .process_with_fixed_art(&art)
        .unwrap();
    RuleSet::from_processed_rules(&processed).unwrap()
}

fn click(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, sw: InternedId) {
    let name = sim.interner.resolve(owner).to_owned();
    sim.apply_command_with_overlays(
        &name,
        &Command::LaunchSuperWeapon {
            sw_type_id: sw,
            target_rx: 0,
            target_ry: 0,
        },
        rules,
        None,
        FrameEffects::default(),
    );
}

/// SW_Firewall::LoadFromINI (`10071620`) forces Action=None, UseChargeDrain
/// and no radar event. Its Activate (`100715C0`) raises the House's
/// firestorm while the drain runs, its Deactivate (`100715F0`) on the next
/// click drops it; a hit on a powered section (hook `0x4423E7`, `10014A90`)
/// leaves it whole and takes `F2I(damage * 2.5)` frames off the drain.
#[test]
fn a_firestorm_super_powers_the_walls_while_it_drains_and_hits_shorten_the_drain() {
    let rules = rules();
    let fire = rules.super_weapon("Fire").unwrap();
    assert_eq!(fire.kind, SuperWeaponKind::Firestorm);
    assert!(fire.use_charge_drain && fire.action.is_none());
    assert!(
        !rules
            .super_weapon_metadata("Fire")
            .unwrap()
            .launch()
            .radar_event()
    );
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x100715C0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.get("Americans").unwrap();
    for house in ["Americans", "Russians"] {
        let house = sim.interner.get(house).unwrap();
        construct_house_supers(&mut sim, &rules, house, false);
    }
    let sw = sim.interner.get("Fire").unwrap();
    sim.session.set_binary_frame_for_test(100);
    assert!(grant_super(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &sim.houses,
        &mut sim.super_weapons,
        &rules,
        owner,
        sw,
        false,
        false
    ));
    sim.session.set_binary_frame_for_test(1000);
    tick_house_superweapon_instances(&mut sim, &rules, owner);
    assert_eq!(sim.super_weapons[&owner][&sw].charge_drain_state(), 1);
    let wall = sim
        .spawn_object("FSWALL", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let wall_frame = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(wall)
            .unwrap()
            .ares
            .ares_firestorm_wall
            .frame()
    };

    click(&mut sim, &rules, owner, sw);
    assert_eq!(sim.super_weapons[&owner][&sw].charge_drain_state(), 2);
    assert!(sim.ares_firestorm.is_active(owner));
    assert_eq!(wall_frame(&sim), 32);
    let left = sim.super_weapons[&owner][&sw].charge_remaining(1000);
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        wall,
        40,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        sim.interner.intern("C4WH"),
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    sim.commit_direct_damage_receiver(&rules, None, hit, FrameEffects::default());
    assert_eq!(
        sim.substrate.entities.get(wall).unwrap().health.current(),
        500
    );
    assert_eq!(
        sim.super_weapons[&owner][&sw].charge_remaining(1000),
        left - 100
    );

    click(&mut sim, &rules, owner, sw);
    assert_eq!(sim.super_weapons[&owner][&sw].charge_drain_state(), 1);
    assert!(!sim.ares_firestorm.is_active(owner));
    assert_eq!(wall_frame(&sim), 0);
}
