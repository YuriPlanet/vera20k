//! Ares `Type=EMPulse` through the ordered Rules reader, the launch command
//! and the cannons' Missile mission, and the `Type=DropPod` readers.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::command::Command;
use crate::sim::mission::MissionType;
use crate::sim::projectile::ProjectileTarget;
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::world::SimSoundEvent;

const RULES: &str = "\
[SuperWeaponTypes]
0=Pulse
1=Volley
[Pulse]
Type=EMPulse
RechargeTime=1
IsPowered=no
SW.MaxCount=2
EMPulse.PulseDelay=7
EMPulse.PulseBall=BALL
[Volley]
Type=EMPulse
RechargeTime=1
IsPowered=no
EMPulse.Cannons=GUN,NOSUCHTYPE
[BuildingTypes]
0=CANNON
1=GUN
2=DEPOT
[CANNON]
Strength=500
EMPulseCannon=yes
SuperWeapon=Pulse
Primary=PulseGun
[GUN]
Strength=500
Primary=GunShot
[DEPOT]
Strength=500
SuperWeapon=Pulse
[PulseGun]
Damage=50
Range=10
Speed=40
Projectile=PulseShell
Warhead=PulseWH
Report=PulseSound
[PulseShell]
Arcing=yes
[GunShot]
Damage=50
Range=10
Speed=40
Projectile=GunShell
Warhead=PulseWH
[GunShell]
Speed=40
[Warheads]
0=PulseWH
[PulseWH]
CellSpread=2
PercentAtMax=1
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[Animations]
0=BALL
1=PULSBALL
";
const ART: &str = "[CANNON]\nFoundation=1x1\n[GUN]\nFoundation=1x1\n[DEPOT]\nFoundation=1x1\n\
    [BALL]\n[PULSBALL]\n";

fn rules(extra: &str) -> RuleSet {
    // The fixture's `Type=EMPulse` data is SWTypeExt (Ares) read the Ares way.
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
    ini.merge(&IniFile::from_str(extra));
    let art = IniFile::from_str(ART);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules
}

fn scene(rules: &RuleSet) -> (Simulation, InternedId) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 17);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.intern("Russians");
    for name in ["Pulse", "Volley"] {
        let sw_type = sim.interner.intern(name);
        let mut instance = SuperWeaponInstance::new(sw_type, owner, 0);
        instance.is_active = true;
        instance.is_ready = true;
        instance.charge_start_tick = sim.session.binary_frame() as i32;
        instance.charge_duration = 900;
        sim.super_weapons
            .map_mut_for_test()
            .entry(owner)
            .or_default()
            .insert(sw_type, instance);
    }
    (sim, owner)
}

fn building(sim: &mut Simulation, rules: &RuleSet, kind: &str, cell: (u16, u16)) -> u64 {
    sim.spawn_object_at_height(kind, "Russians", cell.0, cell.1, 0, 0, rules)
        .unwrap()
}

fn launch(sim: &mut Simulation, rules: &RuleSet, sw: &str, cell: (u16, u16)) -> bool {
    let sw_type_id = sim.interner.intern(sw);
    sim.apply_command_with_overlays(
        None,
        "Russians",
        &Command::LaunchSuperWeapon {
            sw_type_id,
            target_rx: cell.0,
            target_ry: cell.1,
        },
        rules,
        None,
        FrameEffects::default(),
    )
}

fn mission(sim: &Simulation, id: u64) -> Option<MissionType> {
    sim.substrate.entities.get(id)?.mission.effective().known()
}

fn step(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), None, None, 33);
}

/// SW_EMPulse's Initialize (10071440) and LoadFromINI (10071510): MaxCount
/// 1 and ranges (0, -1) until read, PulseDelay 32, Find-only references
/// (an unknown cannon name is skipped), and Action none under TargetSelf.
#[test]
fn ares_emp_pulse_reads_its_handler_keys() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules("[Volley]\nEMPulse.TargetSelf=yes\nEMPulse.PulseBall=NOSUCHANIM\n");
    let pulse = rules.super_weapon("Pulse").unwrap();
    assert_eq!(pulse.kind, SuperWeaponKind::EMPulse);
    assert_eq!(pulse.action.as_deref(), Some("Attack"));
    let launch = rules.super_weapon_metadata("Pulse").unwrap().launch();
    let emp = launch.emp_pulse();
    assert_eq!(
        (emp.max_count, emp.pulse_delay, emp.pulse_ball.clone()),
        (2, 7, Some(Some("BALL".to_owned())))
    );
    let volley = rules.super_weapon_metadata("Volley").unwrap();
    let emp = volley.launch().emp_pulse();
    assert_eq!(
        (emp.max_count, emp.pulse_delay, emp.pulse_ball.clone()),
        (1, 32, None)
    );
    assert_eq!(emp.cannons, ["GUN"]);
    assert!(emp.target_self);
    assert_eq!(volley.targeting().range(), (0.0, -1.0));
    assert!(rules.super_weapon("Volley").unwrap().action.is_none());
}

/// SW_EMPulse::Activate (100711C0) with IsLaunchSite (10071490) and
/// GetLaunchSiteRange (10071380): of the providing `EMPulseCannon=`
/// buildings within weapon 0's `Range=` of the cell, the first
/// `SW.MaxCount` in the House's order take Missile with the type's index
/// and the cell; the House's EMPTarget is the cell.
#[test]
fn ares_emp_pulse_sends_the_cannons_in_range_up_to_its_count() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules("");
    let (mut sim, owner) = scene(&rules);
    let depot = building(&mut sim, &rules, "DEPOT", (24, 23));
    let far = building(&mut sim, &rules, "CANNON", (31, 2));
    let first = building(&mut sim, &rules, "CANNON", (20, 20));
    let second = building(&mut sim, &rules, "CANNON", (22, 20));
    let third = building(&mut sim, &rules, "CANNON", (20, 22));
    assert!(launch(&mut sim, &rules, "Pulse", (26, 26)));
    for id in [first, second] {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(mission(&sim, id), Some(MissionType::Missile));
        assert_eq!(
            entity
                .mission_leaf
                .as_building()
                .unwrap()
                .firing_super_weapon(),
            0
        );
        assert_eq!(entity.ares_super_target(), Some((26, 26)));
    }
    for id in [depot, far, third] {
        assert_ne!(mission(&sim, id), Some(MissionType::Missile));
    }
    assert_eq!(sim.houses[&owner].emp_target(), (26, 26));
}

/// gamemd Mission_Missile's EMP arm (0x0044CD42..0x0044D535) for a fired
/// cannon: it aims, shows the Super's `EMPulse.PulseBall` and waits its
/// `EMPulse.PulseDelay` (Ares hook 44CE46), then launches weapon 0's
/// shell at the cell from its turret pivot with its report sound, and
/// goes back to Guard.
#[test]
fn ares_emp_cannon_pulses_then_fires_its_shell_at_the_cell() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules("[Pulse]\nSW.MaxCount=1\n");
    let (mut sim, _) = scene(&rules);
    let cannon = building(&mut sim, &rules, "CANNON", (20, 20));
    assert!(launch(&mut sim, &rules, "Pulse", (26, 26)));
    let ball = |sim: &Simulation| {
        sim.substrate
            .anims
            .iter()
            .any(|(_, anim)| sim.interner.resolve(anim.type_id()) == "BALL")
    };
    let mut ball_frame = None;
    let mut shot_frame = None;
    for _ in 0..200 {
        step(&mut sim, &rules);
        let frame = sim.session.binary_frame();
        if ball_frame.is_none() && ball(&sim) {
            ball_frame = Some(frame);
        }
        if sim.projectiles.iter().next().is_some() {
            shot_frame = Some(frame);
            break;
        }
    }
    let (ball_frame, shot_frame) = (ball_frame.unwrap(), shot_frame.unwrap());
    assert_eq!(shot_frame - ball_frame, 7);
    let (_, shell) = sim.projectiles.iter().next().unwrap();
    assert_eq!(shell.source_id(), cannon);
    assert_eq!(shell.target(), ProjectileTarget::Cell { rx: 26, ry: 26 });
    let velocity = shell.velocity().native().map(|v| f64::from_bits(v.bits()));
    assert!(velocity[0] > 0.0 && velocity[1] > 0.0 && velocity[2] > 0.0);
    assert!(sim.sound_events.iter().any(|event| matches!(
        event,
        SimSoundEvent::VocAt { sound_id, .. } if sound_id == "PulseSound"
    )));
    for _ in 0..3 {
        step(&mut sim, &rules);
    }
    assert_eq!(mission(&sim, cannon), Some(MissionType::Guard));
}

/// Ares hook 44CCE7 (10036570): an `EMPulse.Cannons` building without
/// `EMPulseCannon=` fires weapon 0 at the Super's cell and queues Guard;
/// under `EMPulse.TargetSelf` (Activate's other arm) it detonates its weapon
/// on itself instead.
#[test]
fn ares_emp_pulse_listed_cannons_fire_at_the_cell_or_at_themselves() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules("");
    let (mut sim, _) = scene(&rules);
    let gun = building(&mut sim, &rules, "GUN", (20, 20));
    assert!(launch(&mut sim, &rules, "Volley", (26, 26)));
    assert_eq!(mission(&sim, gun), Some(MissionType::Missile));
    step(&mut sim, &rules);
    let shells: Vec<_> = sim
        .projectiles
        .iter()
        .filter(|(_, shell)| shell.source_id() == gun)
        .collect();
    assert_eq!(shells.len(), 1);
    assert_eq!(
        shells[0].1.target(),
        ProjectileTarget::Cell { rx: 26, ry: 26 }
    );
    assert_eq!(mission(&sim, gun), Some(MissionType::Guard));

    let rules = self::rules("[Volley]\nEMPulse.TargetSelf=yes\n");
    let (mut sim, _) = scene(&rules);
    let gun = building(&mut sim, &rules, "GUN", (20, 20));
    let victim = sim
        .spawn_object_at_height("DEPOT", "Americans", 21, 20, 0, 0, &rules)
        .unwrap();
    assert!(launch(&mut sim, &rules, "Volley", (0, 0)));
    assert_ne!(mission(&sim, gun), Some(MissionType::Missile));
    assert!(sim.projectiles.iter().next().is_none());
    assert_eq!(
        sim.substrate.entities.get(victim).unwrap().health.current(),
        450
    );
}

/// SW_DropPod's Initialize (100710F0) and LoadFromINI (10071140) over two
/// passes: Nullable counts, Veterancy 2.0 until read, and Find-only types
/// (an unknown name is skipped, a later list replaces).
#[test]
fn ares_drop_pod_reads_its_handler_keys() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
    ini.merge(&IniFile::from_str(
        "[SuperWeaponTypes]\n2=Drop\n[Drop]\nType=DropPod\nDropPod.Minimum=2\n\
         DropPod.Types=NOSUCHTYPE,GUN\n",
    ));
    let mut layers = RulesLayerStack::new(ini);
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Drop]\nDropPod.Veterancy=1.5\nDropPod.Types=CANNON\n"),
    );
    let processed = layers
        .process_with_fixed_art(&IniFile::from_str(ART))
        .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let metadata = rules.super_weapon_metadata("Drop").unwrap();
    let drop_pod = metadata.launch().drop_pod();
    assert_eq!((drop_pod.minimum, drop_pod.maximum), (Some(2), None));
    assert_eq!(f64::from_bits(drop_pod.veterancy.bits()), 1.5);
    let types: Vec<_> = drop_pod.types.iter().map(|ty| ty.id.as_str()).collect();
    assert_eq!(types, ["CANNON"]);
    assert_eq!(
        metadata.targeting().mode(),
        crate::rules::superweapon_targeting::AiTargetingMode::DropPod
    );
}

/// Phobos `EMPulse.WeaponIndex` (`Ext/SWType/Body.cpp:179`, hook
/// `0x44CEEC` in `Ext/Building/Hooks.cpp:132-189`): slot 1 takes the
/// cannon's Secondary, while a negative index asks SelectWeapon against the
/// EMPTarget cell. PulseGun has `Report=PulseSound`; GunShot has none.
#[test]
fn phobos_empulse_weapon_index_selects_the_indexed_weapon() {
    let slot1 =
        rules("[Pulse]\nSW.MaxCount=1\nEMPulse.WeaponIndex=1\n[CANNON]\nSecondary=GunShot\n");
    let (mut sim, _) = scene(&slot1);
    building(&mut sim, &slot1, "CANNON", (20, 20));
    assert!(launch(&mut sim, &slot1, "Pulse", (26, 26)));
    for _ in 0..200 {
        step(&mut sim, &slot1);
        if sim.projectiles.iter().next().is_some() {
            break;
        }
    }
    assert!(
        sim.projectiles.iter().next().is_some(),
        "slot 1 still fires a bullet"
    );
    assert!(!sim.sound_events.iter().any(|event| matches!(
        event,
        SimSoundEvent::VocAt { sound_id, .. } if sound_id == "PulseSound"
    )));

    let reverse = rules("[Pulse]\nSW.MaxCount=1\nEMPulse.WeaponIndex=-1\n");
    let (mut sim, _) = scene(&reverse);
    building(&mut sim, &reverse, "CANNON", (20, 20));
    assert!(launch(&mut sim, &reverse, "Pulse", (26, 26)));
    for _ in 0..200 {
        step(&mut sim, &reverse);
        if sim.projectiles.iter().next().is_some() {
            break;
        }
    }
    assert!(
        sim.projectiles.iter().next().is_some(),
        "SelectWeapon against the cell still fires weapon 0"
    );
}

/// `EMPulse.Cannons` (`Ext/SWType/Body.cpp:181`, `SWHelpers.cpp:232-268`):
/// the listed type is a launch site, an unlisted provider is not, and an
/// empty list leaves the Ares `EMPulseCannon=` sites unrestricted.
#[test]
fn phobos_empulse_cannons_restrict_the_firing_source() {
    let listed = rules("");
    let (mut sim, _) = scene(&listed);
    let gun = building(&mut sim, &listed, "GUN", (20, 20));
    let depot = building(&mut sim, &listed, "DEPOT", (22, 22));
    let cannon = building(&mut sim, &listed, "CANNON", (24, 24));
    assert!(launch(&mut sim, &listed, "Volley", (26, 26)));
    assert_eq!(mission(&sim, gun), Some(MissionType::Missile));
    assert_ne!(mission(&sim, depot), Some(MissionType::Missile));
    assert_ne!(mission(&sim, cannon), Some(MissionType::Missile));

    let unrestricted = rules("");
    let pulse = unrestricted
        .super_weapon_metadata("Pulse")
        .unwrap()
        .launch()
        .emp_pulse();
    assert!(pulse.cannons.is_empty(), "an empty list is no restriction");
    let (mut sim, _) = scene(&unrestricted);
    let cannon = building(&mut sim, &unrestricted, "CANNON", (20, 20));
    assert!(launch(&mut sim, &unrestricted, "Pulse", (26, 26)));
    assert_eq!(mission(&sim, cannon), Some(MissionType::Missile));
}

/// `EMPulse.SuspendOthers` (`Ext/SWType/Body.cpp:180`,
/// `Ext/SWType/FireSuperWeapon.cpp:380-404`): the other EMPulse Super is
/// held while the missile flies, the residue is real save state, and
/// Mission_Missile's select-weapon hook (`0x44CEEC`) resumes and erases it.
#[test]
fn phobos_empulse_suspend_others_holds_then_resumes() {
    // The two SW types share a cannon type, which is Phobos's intersection
    // condition for holding the other Super (`FireSuperWeapon.cpp:384-392`).
    // GUN is also an `EMPulseCannon=`: Mission_Missile only runs the cannon
    // statuses (and so the `0x44CEEC` select-weapon hook that resumes the
    // held Supers) for such a type; a merely listed non-cannon fires weapon 0
    // and queues Guard without reaching the hook.
    let rules = rules(
        "[Pulse]\nSW.MaxCount=1\nEMPulse.SuspendOthers=yes\nEMPulse.Cannons=GUN\n\
         [Volley]\nEMPulse.Cannons=GUN\n[GUN]\nEMPulseCannon=yes\n",
    );
    let (mut sim, owner) = scene(&rules);
    building(&mut sim, &rules, "GUN", (20, 20));
    let volley = sim.interner.intern("Volley");
    assert!(launch(&mut sim, &rules, "Pulse", (26, 26)));
    assert!(sim.super_weapons[&owner][&volley].is_suspended());
    assert!(!sim.houses[&owner].suspended_emp_pulse_sws().is_empty());

    let saved = serde_json::to_string(&sim.houses[&owner]).expect("house serializes");
    let restored: crate::sim::house_state::HouseState =
        serde_json::from_str(&saved).expect("house restores");
    assert_eq!(
        restored.suspended_emp_pulse_sws(),
        sim.houses[&owner].suspended_emp_pulse_sws()
    );

    for _ in 0..200 {
        step(&mut sim, &rules);
        if !sim.super_weapons[&owner][&volley].is_suspended() {
            break;
        }
    }
    assert!(!sim.super_weapons[&owner][&volley].is_suspended());
    assert!(sim.houses[&owner].suspended_emp_pulse_sws().is_empty());
}

/// `EMPulse.TargetSelf` (`Ext/SWType/FireSuperWeapon.cpp:37`) skips
/// `HandleEMPulseLaunch` entirely: no cannon latch, no suspension, no
/// residue. `SWHelpers.cpp:272` also makes the cannon range unbounded.
#[test]
fn phobos_empulse_target_self_skips_the_launch_tail() {
    // Without the skip, the shared cannon list would suspend Volley.
    let rules = rules(
        "[Pulse]\nEMPulse.TargetSelf=yes\nEMPulse.SuspendOthers=yes\nEMPulse.Cannons=GUN\n\
         [Volley]\nEMPulse.Cannons=GUN\n",
    );
    let (mut sim, owner) = scene(&rules);
    building(&mut sim, &rules, "GUN", (20, 20));
    let volley = sim.interner.intern("Volley");
    assert!(launch(&mut sim, &rules, "Pulse", (26, 26)));
    assert!(!sim.super_weapons[&owner][&volley].is_suspended());
    assert!(sim.houses[&owner].suspended_emp_pulse_sws().is_empty());
}

/// With none of the four Phobos keys set, the constructor values keep the
/// Ares EMPulse path: weapon slot 0, no suspension, no residue.
#[test]
fn stock_empulse_keys_keep_the_ares_launch() {
    let rules = rules("");
    let emp = rules
        .super_weapon_metadata("Pulse")
        .unwrap()
        .launch()
        .emp_pulse();
    assert_eq!(emp.empulse_weapon_index, 0);
    assert!(!emp.empulse_suspend_others);
    let (mut sim, owner) = scene(&rules);
    let cannon = building(&mut sim, &rules, "CANNON", (20, 20));
    let volley = sim.interner.intern("Volley");
    assert!(launch(&mut sim, &rules, "Pulse", (26, 26)));
    assert_eq!(mission(&sim, cannon), Some(MissionType::Missile));
    assert!(!sim.super_weapons[&owner][&volley].is_suspended());
    assert!(sim.houses[&owner].suspended_emp_pulse_sws().is_empty());
}

/// `BuildingExt::CurrentEMPulseSW` (`Ext/Building/Hooks.cpp:132-189`) is real
/// state across ticks: two otherwise identical worlds that differ only in the
/// latch take different weapon slots and resume sets, so the state hash must
/// tell them apart.
#[test]
fn the_empulse_cannon_latch_changes_the_state_hash() {
    let rules = rules("");
    let (mut first, _) = scene(&rules);
    let cannon = building(&mut first, &rules, "CANNON", (20, 20));
    let (mut second, _) = scene(&rules);
    building(&mut second, &rules, "CANNON", (20, 20));
    assert_eq!(
        first.state_hash(),
        second.state_hash(),
        "identical worlds start with the same hash"
    );
    let pulse = first.interner.intern("Pulse");
    first
        .substrate
        .entities
        .get_mut(cannon)
        .unwrap()
        .set_phobos_current_empulse_sw(pulse);
    let second_pulse = second.interner.intern("Pulse");
    assert_eq!(pulse, second_pulse);
    assert_ne!(
        first.state_hash(),
        second.state_hash(),
        "the latch is hashed"
    );
}

/// `FireSuperWeapon.cpp:400-403` pushes a second hold into the existing entry;
/// the map must keep both lists until the resume hook clears them.
#[test]
fn a_second_empulse_hold_appends_to_the_house_entry() {
    let (mut sim, owner) = scene(&rules(""));
    let pulse = sim.interner.intern("Pulse");
    let first = sim.interner.intern("Volley");
    let second = sim.interner.intern("Pulse2");
    let house = sim.houses.get(&owner).unwrap();
    let mut house = house.clone();
    house.hold_emp_pulse_sws(pulse, vec![first]);
    house.hold_emp_pulse_sws(pulse, vec![second]);
    assert_eq!(
        house.suspended_emp_pulse_sws()[&pulse],
        [first, second],
        "the later hold is appended, not replacing the earlier one"
    );
    assert_eq!(house.release_emp_pulse_sws(pulse), [first, second]);
    assert!(house.suspended_emp_pulse_sws().is_empty());
}
/// Mental Omega's Iron Guard shape: an auto-firing, self-targeting EMPulse
/// whose cannon's warhead curtains its House's nearby units.
const IRON_GUARD: &str = "\
[VehicleTypes]
0=TANK
[TANK]
Strength=300
[BuildingTypes]
0=GUARD
1=TOWER
2=COIL
[GUARD]
Strength=700
EMPulseCannon=yes
Primary=GuardWeapon
SuperWeapon=GuardSpecial
[TOWER]
Strength=700
EMPulseCannon=yes
Primary=GuardWeapon
Upgrades=1
[COIL]
Strength=100
PowersUpBuilding=TOWER
PowersUpToLevel=1
SuperWeapon=GuardSpecial
[SuperWeaponTypes]
0=GuardSpecial
[GuardSpecial]
Type=EMPulse
IsPowered=no
RechargeTime=1
SW.InitialReady=yes
SW.AutoFire=yes
SW.ManualFire=no
SW.AITargeting=Self
EMPulse.TargetSelf=yes
[WeaponTypes]
0=GuardWeapon
[GuardWeapon]
Damage=-1
Warhead=GuardWH
[Warheads]
0=GuardWH
[GuardWH]
CellSpread=3
IronCurtain.Duration=125
";

/// `SW.AITargeting=Self` picks the providing building's own cell
/// (PickTarget 100389C9), so the human House's automatic launch fires the
/// Iron Guard and its warhead curtains the tank beside it.
#[test]
fn ares_self_targeting_auto_fire_curtains_the_tank_beside_its_building() {
    assert_iron_guard_protects("GUARD", None);
}

/// HasSuperWeapon, the House grant pass and IsLaunchSite all admit the
/// installed upgrade when the cannon's base type has no Super of its own.
#[test]
fn ares_self_targeting_auto_fire_uses_a_super_from_an_installed_upgrade() {
    assert_iron_guard_protects("TOWER", Some("COIL"));
}

fn assert_iron_guard_protects(building: &str, upgrade: Option<&str>) {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let ini = crate::sim::arena_fixture::clear_ground_ini(IRON_GUARD);
    let art = IniFile::from_str("[GUARD]\nFoundation=1x1\n[TOWER]\nFoundation=1x1\n");
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    super::construct_house_supers(&mut sim, &rules, owner, false);
    let sw = sim.interner.intern("GuardSpecial");
    let host = sim
        .spawn_object_at_height(building, "Americans", 12, 15, 0, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object_at_height("TANK", "Americans", 14, 15, 0, 0, &rules)
        .unwrap();
    if let Some(upgrade) = upgrade {
        assert!(!sim.super_weapons[&owner][&sw].is_active());
        let temporary = sim
            .construct_object_limbo_at_height(
                upgrade,
                "Americans",
                12,
                15,
                0,
                0,
                &rules,
                FrameEffects::default(),
            )
            .unwrap();
        assert!(sim.try_install_building_upgrade(
            temporary,
            host,
            &rules,
            crate::sim::world::UninitContext::new(&rules),
        ));
    } else {
        super::grant_provided_supers(&mut sim, &rules, owner);
    }
    assert!(sim.super_weapons[&owner][&sw].is_ready());
    crate::sim::house_strategy::update_strategy(
        &mut sim,
        &rules,
        owner,
        None,
        FrameEffects::default(),
    );
    let tank = sim.substrate.entities.get(tank).unwrap();
    assert!(crate::sim::superweapon::invulnerability::is_invulnerable(
        tank.invulnerability.as_ref(),
        sim.session.binary_frame(),
    ));
}
