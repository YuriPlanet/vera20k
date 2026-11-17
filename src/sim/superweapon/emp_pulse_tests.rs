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
