//! Ares `Type=DropPod` through the ordered Rules reader, the launch command
//! and the DropPod locomotor (`superweapon::drop_pod`).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::sim::command::Command;
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;

const RULES: &str = "[General]\nDropPodAngle=0.79\n[Countries]\n0=Americans\n1=Russians\n\
 [Sides]\nAllied=Americans\nSoviet=Russians\n\
 [VehicleTypes]\n0=MTNK\n\
 [MTNK]\nStrength=300\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\n\
 Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
 [SuperWeaponTypes]\n0=Drop\n[Drop]\nType=DropPod\nRechargeTime=1\nIsPowered=no\n\
 DropPod.Minimum=2\nDropPod.Maximum=2\nDropPod.Types=MTNK\n";

fn rules(extra: &str) -> RuleSet {
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
    ini.merge(&IniFile::from_str(extra));
    let processed = RulesLayerStack::new(ini)
        .process_with_fixed_art(&IniFile::empty())
        .unwrap();
    RuleSet::from_processed_rules(&processed).unwrap()
}

fn world(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    for (name, human) in [("Americans", true), ("Russians", false)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, 0, Some(id), human, 2000, 10));
        construct_house_supers(&mut sim, rules, id, false);
    }
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().super_weapons = true;
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    sim
}

/// `owner`'s DropPod granted and charged.
fn charge(sim: &mut Simulation, rules: &RuleSet, owner: &str) -> InternedId {
    let owner = sim.interner.get(owner).unwrap();
    let sw = sim.interner.get("Drop").unwrap();
    assert!(trigger_grant_super(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &sim.houses,
        &mut sim.super_weapons,
        rules,
        owner,
        sw,
        false
    ));
    let instance = sim
        .super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&sw)
        .unwrap();
    instance.is_ready = true;
    instance.charge_start_tick = sim.session.binary_frame() as i32;
    instance.charge_duration = 1;
    sw
}

fn fire(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16)) {
    let sw = charge(sim, rules, "Americans");
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &Command::LaunchSuperWeapon {
            sw_type_id: sw,
            target_rx: cell.0,
            target_ry: cell.1,
        },
        rules,
        None,
        FrameEffects::default(),
    ));
}

fn pods(sim: &Simulation, owner: &str) -> Vec<u64> {
    sim.substrate
        .entities
        .values()
        .filter(|entity| sim.interner.resolve(entity.owner()) == owner)
        .filter(|entity| sim.interner.resolve(entity.type_ref()) == "MTNK")
        .map(|entity| entity.stable_id())
        .collect()
}

/// Run frames until every pod of `ids` has landed, and answer the cell each
/// stood on in the frame it landed.
fn land(sim: &mut Simulation, rules: &RuleSet, ids: &[u64]) -> Vec<(u16, u16)> {
    let mut landed = vec![None; ids.len()];
    for _ in 0..200 {
        let commands = sim.take_due_commands();
        let _ = sim.advance_tick(&commands, Some(rules), None, None, 33);
        for (slot, &id) in landed.iter_mut().zip(ids) {
            let entity = sim.substrate.entities.get(id).expect("pod unit survives");
            if slot.is_none() && entity.ares_drop_pod().is_none() {
                assert!(!entity.lifecycle.in_limbo());
                assert_eq!(
                    crate::sim::movement::air_movement::current_fly_height(
                        entity,
                        sim.resolved_terrain.as_ref()
                    ),
                    0
                );
                *slot = Some((entity.position.rx(), entity.position.ry()));
            }
        }
        if landed.iter().all(Option::is_some) {
            return landed.into_iter().flatten().collect();
        }
    }
    panic!("the pods never landed");
}

/// SW_DropPod::Activate (10070E40) with two MTNK: CreateWithDroppod
/// (10044030) lands the first on the open target cell and the second on the
/// neighbour the walk then picked. Each pod's Move_To (0x004B6040) starts
/// its unit DropPodHeight (constructor 1500) above the ground on the +X side
/// of its cell, with the cell as NavCom; its Process (0x004B5B70) lands it
/// on that cell, out of limbo.
#[test]
fn drop_pod_lands_its_units_on_the_target_and_a_neighbour() {
    let rules = rules("");
    let mut sim = world(&rules);
    fire(&mut sim, &rules, (12, 10));
    let ids = pods(&sim, "Americans");
    assert_eq!(ids.len(), 2);
    let height = rules.ares_drop_pod().locomotor.height;
    let mut cells = Vec::new();
    for &id in &ids {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert!(!entity.lifecycle.in_limbo());
        let landing = entity
            .ares_drop_pod()
            .and_then(|pod| pod.destination())
            .expect("a pod with a destination");
        let cell = ((landing.x / 256) as u16, (landing.y / 256) as u16);
        assert_eq!(
            entity.navigation.nav_com(),
            Some(NavTargetRef::cell(cell.0, cell.1))
        );
        assert_eq!(
            crate::sim::movement::air_movement::current_fly_height(
                entity,
                sim.resolved_terrain.as_ref()
            ),
            height
        );
        assert!(
            crate::sim::movement::ground_pose::position_world_xy(&entity.position)[0] > landing.x
        );
        cells.push(cell);
    }
    assert!(cells.contains(&(12, 10)));
    let other = cells.iter().find(|&&cell| cell != (12, 10)).unwrap();
    assert!(other.0.abs_diff(12) <= 1 && other.1.abs_diff(10) <= 1);
    assert_eq!(land(&mut sim, &rules, &ids), cells);
}

/// The pod's `DropPodWeapon=` strafes a point 0x55 leptons from its
/// destination every third frame while the destination's cell holds no ally
/// (0x004B5F29..0x004B602D); without `Report=` Ares 1005C890 (hook
/// 0x4B5F9E) skips the sound instead of dividing by zero.
#[test]
fn drop_pod_weapon_strafes_around_the_destination() {
    let rules = rules(
        "[General]\nDropPodWeapon=PodGun\n[Drop]\nDropPod.Minimum=1\nDropPod.Maximum=1\n\
         [PodGun]\nDamage=10\nWarhead=PodWH\n[Warheads]\n0=PodWH\n\
         [PodWH]\nCellSpread=2\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    assert!(
        rules
            .ares_drop_pod()
            .locomotor
            .weapon
            .as_deref()
            .is_some_and(|weapon| weapon.eq_ignore_ascii_case("PodGun"))
    );
    let mut sim = world(&rules);
    let victim = sim
        .spawn_object("MTNK", "Russians", 11, 10, 0, &rules)
        .unwrap();
    fire(&mut sim, &rules, (12, 10));
    let ids = pods(&sim, "Americans");
    assert_eq!(ids.len(), 1);
    assert_eq!(land(&mut sim, &rules, &ids), [(12, 10)]);
    assert!(sim.substrate.entities.get(victim).unwrap().health.current() < 300);
}

/// Ares PickTarget (10038640, case 0x13): a computer House with an enemy
/// aims its DropPod at the nearby Foot cell of House501AC0's random cell in
/// a random zone (`RandomRanged(1, 4)`) around its base. With the radius at
/// its 0x300 floor that cell lies 0x300..0x600 leptons off the base, two to
/// six cells, and the first unit's pod is sent there. Without the arm the
/// AI finds no target and drops nothing.
#[test]
fn drop_pod_ai_targets_a_cell_in_a_zone_around_its_base() {
    let rules = rules("");
    let mut sim = world(&rules);
    let russians = sim.interner.get("Russians").unwrap();
    let americans = sim.interner.get("Americans").unwrap();
    let house = sim.houses.get_mut(&russians).unwrap();
    house.retain_base_geometry(Some((16, 16)), 0x300);
    house.set_enemy_house_for_test(Some(americans));
    charge(&mut sim, &rules, "Russians");
    super::ai_fire::try_fire(&mut sim, &rules, russians, None, FrameEffects::default());
    let ids = pods(&sim, "Russians");
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().any(|&id| {
        let landing = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.ares_drop_pod())
            .and_then(|pod| pod.destination())
            .unwrap();
        let cell = ((landing.x / 256) as u16, (landing.y / 256) as u16);
        (2..=6).contains(&cell.0.abs_diff(16).max(cell.1.abs_diff(16)))
    }));
}
