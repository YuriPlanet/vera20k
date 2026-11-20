//! Phobos `DroppodTypeClass` per-type keys through the production rules
//! reader and the DropPod locomotor hooks (`0x4B5B70` Process, `0x4B607D`
//! MoveTo).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::sim::command::Command;
use crate::sim::components::DriveCoord;
use crate::sim::house_state::HouseState;

const RULES: &str = "[General]\nDropPodAngle=0.79\nDropPodHeight=1500\nDropPodSpeed=40\n\
 [Countries]\n0=Americans\n1=Russians\n\
 [Sides]\nAllied=Americans\nSoviet=Russians\n\
 [VehicleTypes]\n0=MTNK\n\
 [MTNK]\nStrength=300\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\n\
 Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
 [Animations]\n0=TRAIL\n1=ENTRY\n2=PUFF\n3=LAND\n4=SMOKEY\n\
 [SuperWeaponTypes]\n0=Drop\n[Drop]\nType=DropPod\nRechargeTime=1\nIsPowered=no\n\
 DropPod.Minimum=1\nDropPod.Maximum=1\nDropPod.Types=MTNK\n";

const ART: &str = "[TRAIL]\nRate=100000\nEnd=99\n[ENTRY]\nRate=100000\nEnd=99\n\
 [PUFF]\nRate=100000\nEnd=99\n[LAND]\nRate=100000\nEnd=99\n[SMOKEY]\nRate=100000\nEnd=99\n";

fn rules(extra: &str) -> RuleSet {
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
    ini.merge(&IniFile::from_str(extra));
    let art = IniFile::from_str(ART);
    let processed = RulesLayerStack::new(ini)
        .process_with_fixed_art(&art)
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    for name in ["TRAIL", "ENTRY", "PUFF", "LAND", "SMOKEY"] {
        registry.bind_anim_frame_count_for_test(name, 100);
    }
    rules.install_art_data(registry);
    rules
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
        .filter(|entity| entity.ares_drop_pod().is_some())
        .map(|entity| entity.stable_id())
        .collect()
}

fn tick(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    let _ = sim.advance_tick(&commands, Some(rules), None, None, 33);
}

fn landing(sim: &Simulation, id: u64) -> Option<DriveCoord> {
    sim.substrate
        .entities
        .get(id)
        .and_then(|entity| entity.ares_drop_pod())
        .and_then(|pod| pod.destination())
}

fn flight_height(sim: &Simulation, id: u64) -> i32 {
    let entity = sim.substrate.entities.get(id).unwrap();
    crate::sim::movement::air_movement::current_fly_height(entity, sim.resolved_terrain.as_ref())
}

fn anims_named(sim: &Simulation, name: &str) -> Vec<crate::sim::anim_class::AnimId> {
    sim.substrate
        .anims
        .iter()
        .filter(|(_, anim)| sim.interner.resolve(anim.type_id()) == name)
        .map(|(id, _)| *id)
        .collect()
}

/// `DropPod.Height=` places the pod, and `DropPod.AtmosphereEntry=` plays the
/// entry anim over it (hook `0x4B607D`).
#[test]
fn height_and_atmosphere_entry_place_the_pod() {
    let rules = rules("[MTNK]\nDropPod.Height=600\nDropPod.AtmosphereEntry=ENTRY\n");
    let mut sim = world(&rules);
    fire(&mut sim, &rules, (12, 10));
    let ids = pods(&sim, "Americans");
    assert_eq!(ids.len(), 1);
    assert_eq!(flight_height(&sim, ids[0]), 600);
    let entries = anims_named(&sim, "ENTRY");
    assert_eq!(entries.len(), 1);
    let owner = sim.interner.get("Americans").unwrap();
    assert_eq!(
        sim.substrate.anims.get(entries[0]).unwrap().owner_house(),
        Some(owner)
    );
}

/// `DropPod.Angle=`/`DropPod.Speed=` replace the `[General]` descent: the
/// per-frame X run is `int(cos(angle) * max(speed, height / 10 + 2))` and the
/// Z drop `int(sin(angle) * speed)`. A flatter, faster angle runs farther and
/// sinks less than the global `0.79` / `40` (hook `0x4B5B70`).
#[test]
fn per_type_angle_and_speed_shape_the_descent() {
    let globals = rules("");
    let mut baseline = world(&globals);
    fire(&mut baseline, &globals, (12, 10));
    let base_pod = pods(&baseline, "Americans")[0];
    let base_x0 = crate::sim::movement::ground_pose::position_world_xy(
        &baseline.substrate.entities.get(base_pod).unwrap().position,
    )[0];
    tick(&mut baseline, &globals);
    let base_x1 = crate::sim::movement::ground_pose::position_world_xy(
        &baseline.substrate.entities.get(base_pod).unwrap().position,
    )[0];
    let base_drop = 1500 - flight_height(&baseline, base_pod);

    let tuned = rules("[MTNK]\nDropPod.Angle=0.45\nDropPod.Speed=300\n");
    let mut sim = world(&tuned);
    fire(&mut sim, &tuned, (12, 10));
    let pod = pods(&sim, "Americans")[0];
    let x0 = crate::sim::movement::ground_pose::position_world_xy(
        &sim.substrate.entities.get(pod).unwrap().position,
    )[0];
    tick(&mut sim, &tuned);
    let x1 = crate::sim::movement::ground_pose::position_world_xy(
        &sim.substrate.entities.get(pod).unwrap().position,
    )[0];
    let drop = 1500 - flight_height(&sim, pod);

    assert!(base_x1 < base_x0, "a plain pod descends toward -X");
    assert!(base_drop > 0);
    assert!(
        x1 < x0,
        "a plain pod still moves toward -X with the tuned type"
    );
    assert!(
        x0 - x1 > base_x0 - base_x1,
        "the tuned angle/speed runs farther along X: {} vs {}",
        x0 - x1,
        base_x0 - base_x1
    );
    assert!(
        drop > base_drop,
        "speed 300 sinks faster than the global 40: {drop} vs {base_drop}"
    );
}

/// A pod whose entry point lies outside the playfield is OutOfMap
/// (`0x004B607D`): it starts on the -X side of the destination and Process
/// moves it +X (hook `0x4B5B70`), the reverse of a plain pod. The narrow
/// playfield diamond makes the plain +X entry point leave it while the
/// destination and the flipped entry point stay inside.
#[test]
fn out_of_map_pods_descend_toward_positive_x() {
    let rules = rules("");
    let mut sim = world(&rules);
    sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -128,
        off_100: -128,
        off_104: 130,
        off_108: 256,
    }));
    fire(&mut sim, &rules, (12, 10));
    let pod = pods(&sim, "Americans")[0];
    let landing = landing(&sim, pod).unwrap();
    let x0 = crate::sim::movement::ground_pose::position_world_xy(
        &sim.substrate.entities.get(pod).unwrap().position,
    )[0];
    assert!(
        x0 < landing.x,
        "an OutOfMap pod enters from -X (start {x0}, landing {})",
        landing.x
    );
    tick(&mut sim, &rules);
    let x1 = crate::sim::movement::ground_pose::position_world_xy(
        &sim.substrate.entities.get(pod).unwrap().position,
    )[0];
    assert!(x1 > x0, "and keeps moving +X while it falls: {x1} vs {x0}");
}

/// The trailer falls back to `SMOKEY` when the type names none, and
/// `DropPod.Trailer.SpawnDelay=` sets its cadence (elapsed frames, not the
/// stock every-sixth-frame rule).
#[test]
fn trailer_uses_the_smokey_default_on_the_configured_cadence() {
    let rules = rules("[MTNK]\nDropPod.Trailer.SpawnDelay=3\n");
    let mut sim = world(&rules);
    fire(&mut sim, &rules, (12, 10));
    let pod = pods(&sim, "Americans")[0];
    let mut spawns = Vec::new();
    let mut seen = 0;
    for _ in 0..20 {
        tick(&mut sim, &rules);
        let count = anims_named(&sim, "SMOKEY").len();
        if count > seen {
            spawns.push(sim.session.binary_frame());
            seen = count;
        }
        if sim
            .substrate
            .entities
            .get(pod)
            .unwrap()
            .ares_drop_pod()
            .is_none()
        {
            break;
        }
    }
    assert!(
        spawns.len() >= 2,
        "several trailers before landing: {spawns:?}"
    );
    for pair in spawns.windows(2) {
        assert_eq!(
            pair[1] - pair[0],
            3,
            "the configured SpawnDelay spaces the spawns: {spawns:?}"
        );
    }
    assert!(
        sim.substrate
            .anims
            .iter()
            .all(|(_, anim)| anim.owner_entity().is_none()),
        "Trailer.Attached unset leaves the trail unowned"
    );
}

/// `DropPod.Trailer.Attached=yes` sets the pod as the trailer's owner object
/// (`SetOwnerObject`), so the anim follows it.
#[test]
fn attached_trailers_own_the_pod() {
    let rules = rules("[MTNK]\nDropPod.Trailer=TRAIL\nDropPod.Trailer.Attached=yes\n");
    let mut sim = world(&rules);
    fire(&mut sim, &rules, (12, 10));
    let pod = pods(&sim, "Americans")[0];
    let mut trailers = Vec::new();
    for _ in 0..5 {
        tick(&mut sim, &rules);
        trailers = anims_named(&sim, "TRAIL");
        if !trailers.is_empty() {
            break;
        }
    }
    assert!(
        !trailers.is_empty(),
        "the first trailer spawns at elapsed 1"
    );
    for trailer in trailers {
        assert_eq!(
            sim.substrate.anims.get(trailer).unwrap().owner_entity(),
            Some(pod)
        );
    }
}

/// `DropPod.Weapon=` is a `Read<true>`: a weapon the INI never lists in
/// `[WeaponTypes]` is constructed by the rules pass and strafes the
/// destination on `max(ROF, 3)`-frame cadence.
#[test]
fn drop_pod_weapon_is_allocated_and_strafes() {
    let rules = rules(
        "[MTNK]\nDropPod.Weapon=PodGun\n\
         [PodGun]\nDamage=10\nWarhead=PodWH\n\
         [Warheads]\n0=PodWH\n\
         [PodWH]\nCellSpread=2\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    assert!(
        rules.weapon("PodGun").is_some(),
        "DropPod.Weapon.Read<true> constructs the weapon"
    );
    let mut sim = world(&rules);
    let victim = sim
        .spawn_object("MTNK", "Russians", 13, 10, 0, &rules)
        .unwrap();
    fire(&mut sim, &rules, (12, 10));
    for _ in 0..3 {
        tick(&mut sim, &rules);
    }
    assert!(
        sim.substrate.entities.get(victim).unwrap().health.current() < 300,
        "the per-type weapon strafes while the pod is still in the air"
    );
}

/// `DropPod.Weapon.HitLandOnly=yes` suppresses the air strafe and detonates
/// the weapon once at the landing point (`WeaponTypeExt::DetonateAt`).
#[test]
fn hit_land_only_defers_the_weapon_to_the_landing() {
    let rules = rules(
        "[MTNK]\nDropPod.Weapon=PodGun\nDropPod.Weapon.HitLandOnly=yes\n\
         [PodGun]\nDamage=10\nWarhead=PodWH\n\
         [Warheads]\n0=PodWH\n\
         [PodWH]\nCellSpread=2\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let mut sim = world(&rules);
    let victim = sim
        .spawn_object("MTNK", "Russians", 13, 10, 0, &rules)
        .unwrap();
    fire(&mut sim, &rules, (12, 10));
    let pod = pods(&sim, "Americans")[0];
    for _ in 0..3 {
        tick(&mut sim, &rules);
    }
    assert!(
        sim.substrate
            .entities
            .get(pod)
            .unwrap()
            .ares_drop_pod()
            .is_some(),
        "still airborne, so no landing detonation yet"
    );
    assert_eq!(
        sim.substrate.entities.get(victim).unwrap().health.current(),
        300,
        "HitLandOnly refuses the air strafe"
    );
    for _ in 0..80 {
        tick(&mut sim, &rules);
        if sim
            .substrate
            .entities
            .get(pod)
            .unwrap()
            .ares_drop_pod()
            .is_none()
        {
            break;
        }
    }
    assert!(
        sim.substrate.entities.get(victim).unwrap().health.current() < 300,
        "the landing detonation damages the victim"
    );
}

/// `DropPod.Puff=` and `DropPod.GroundAnim=` play at the landing point, both
/// owned by the pod's house (`SetAnimOwnerHouseKind`).
#[test]
fn puff_and_ground_anim_play_on_landing() {
    let rules = rules("[MTNK]\nDropPod.Puff=PUFF\nDropPod.GroundAnim=LAND\n");
    let mut sim = world(&rules);
    fire(&mut sim, &rules, (12, 10));
    let pod = pods(&sim, "Americans")[0];
    for _ in 0..80 {
        tick(&mut sim, &rules);
        if sim
            .substrate
            .entities
            .get(pod)
            .unwrap()
            .ares_drop_pod()
            .is_none()
        {
            break;
        }
    }
    assert!(
        sim.substrate
            .entities
            .get(pod)
            .unwrap()
            .ares_drop_pod()
            .is_none()
    );
    let owner = sim.interner.get("Americans").unwrap();
    for name in ["PUFF", "LAND"] {
        let anims = anims_named(&sim, name);
        assert_eq!(anims.len(), 1, "{name} plays exactly once");
        assert_eq!(
            sim.substrate.anims.get(anims[0]).unwrap().owner_house(),
            Some(owner),
            "{name} takes the pod's house"
        );
    }
}
