//! Phobos `SpyEffect.*` through the production Per_Cell spy entry
//! (`0x00519FF8..0x0051A00B`), the call site Phobos wraps with
//! `BuildingClass_InfiltratedBy_Wrapper` (`Ext/Building/Body.cpp:662-671`).
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::superweapon::SuperWeaponInstance;

const RULES_HEAD: &str = "[General]\nSpyMoneyStealPercent=0\n\
    [InfantryTypes]\n0=SPY\n\
    [SPY]\nStrength=100\nSpeed=4\nAgent=yes\n\
    [VehicleTypes]\n[AircraftTypes]\n\
    [BuildingTypes]\n0=LAB\n\
    [LAB]\nStrength=1000\nFoundation=1x1\nSpyable=yes\n";
const RULES_TAIL: &str = "[SuperWeaponTypes]\n0=VSHIELD\n1=ISTORM\n\
    [VSHIELD]\nType=ForceShield\nRechargeTime=1\n\
    [ISTORM]\nType=LightningStorm\nRechargeTime=1\n";
const FRAME: u32 = 100;

/// The scene's building type carries `lab_keys`; the spy belongs to
/// `spy_house` (a house `rival_arena` registered).
struct Scene {
    sim: Simulation,
    rules: RuleSet,
    owner: InternedId,
    spy_house: InternedId,
    building: u64,
    spy: u64,
}

fn scene(lab_keys: &str, spy_house: &str) -> Scene {
    let text = format!("{RULES_HEAD}{lab_keys}{RULES_TAIL}");
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&text))
        .expect("spy effect rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x0051_A00B);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(FRAME);
    sim.session.game_options_mut_for_test().super_weapons = true;
    let owner = sim.interner.intern("Russians");
    let spy_house = sim.interner.intern(spy_house);
    for house in [owner, spy_house] {
        for name in ["VSHIELD", "ISTORM"] {
            let sw = sim.interner.intern(name);
            let mut instance = SuperWeaponInstance::new(sw, house, 0);
            instance.set_is_active_for_test(true);
            instance.set_charge_start_tick_for_test(7);
            instance.set_charge_duration_for_test(900);
            sim.super_weapons
                .map_mut_for_test()
                .entry(house)
                .or_default()
                .insert(sw, instance);
        }
    }
    let building = sim
        .spawn_object("LAB", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let spy_house_name = sim.interner.resolve(spy_house).to_owned();
    // The building's own cell is not passable for Unlimbo; the spy walks in
    // later through `foot_set_location_marked`.
    let spy = sim
        .spawn_object("SPY", &spy_house_name, 8, 10, 0, &rules)
        .unwrap();
    Scene {
        sim,
        rules,
        owner,
        spy_house,
        building,
        spy,
    }
}

/// Arrive in the building's cell on a Capture order; answers whether the
/// Per_Cell receiver took the spy in (`return_before_foot`).
fn infiltrate(scene: &mut Scene) -> bool {
    let Scene {
        sim,
        rules,
        building,
        spy,
        ..
    } = scene;
    sim.substrate
        .mission_assign_exact(*spy, MissionId::from_known(MissionType::Capture), 0)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(*spy)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::Object { id: *building }));
    let at = sim.substrate.entities.get(*building).unwrap().position;
    sim.foot_set_location_marked(
        *spy,
        DriveCoord {
            x: i32::from(at.rx()) * 256 + 128,
            y: i32::from(at.ry()) * 256 + 128,
            z: 0,
        },
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    sim.infantry_per_cell_building_entry(
        *spy,
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap()
    .return_before_foot
}

fn launched_supers(sim: &mut Simulation) -> Vec<(InternedId, InternedId, i32, i32)> {
    sim.take_sound_events_for_test()
        .into_iter()
        .filter_map(|event| match event {
            SimSoundEvent::SuperWeaponLaunched {
                owner,
                sw_type,
                world,
            } => Some((owner, sw_type, world.x, world.y)),
            _ => None,
        })
        .collect()
}

/// Victim index first on the victim house, then the infiltrator index on the
/// spy's house; both at the building's centre cell and both charge timers
/// restored to their pre-launch `StartTime`/`TimeLeft`.
#[test]
fn spy_effect_launches_both_configured_supers_and_keeps_the_recharge() {
    let keys = "SpyEffect.Custom=yes\nSpyEffect.VictimSuperWeapon=VSHIELD\n\
                SpyEffect.InfiltratorSuperWeapon=ISTORM\n";
    let mut scene = scene(keys, "Americans");
    assert!(infiltrate(&mut scene));
    let Scene {
        sim,
        owner,
        spy_house,
        ..
    } = &mut scene;
    let vshield = sim.interner.intern("VSHIELD");
    let istorm = sim.interner.intern("ISTORM");
    let launched = launched_supers(sim);
    assert_eq!(
        launched
            .iter()
            .map(|(house, sw, x, y)| (*house, *sw, x / 256, y / 256))
            .collect::<Vec<_>>(),
        vec![(*owner, vshield, 10, 10), (*spy_house, istorm, 10, 10),],
        "victim then infiltrator, each at the building's centre cell"
    );
    for (house, sw) in [(*owner, &vshield), (*spy_house, &istorm)] {
        let instance = &sim.super_weapons[&house][sw];
        assert!(!instance.is_ready(), "Reset clears the forced readiness");
        assert_eq!(
            (instance.charge_start_tick(), instance.charge_duration()),
            (7, 900),
            "the saved RechargeTimer StartTime/TimeLeft are written back"
        );
    }
}

/// `SpyEffect.Custom` off, a same-house entry, and unresolved superweapon
/// names all launch nothing through the same entry.
#[test]
fn spy_effect_stays_quiet_without_custom_same_house_or_unknown_names() {
    for (keys, spy_house) in [
        ("", "Americans"),
        (
            "SpyEffect.Custom=yes\nSpyEffect.VictimSuperWeapon=VSHIELD\n\
             SpyEffect.InfiltratorSuperWeapon=ISTORM\n",
            "Russians",
        ),
        (
            "SpyEffect.Custom=yes\nSpyEffect.VictimSuperWeapon=NOTASW\n\
             SpyEffect.InfiltratorSuperWeapon=NOSUCH\n",
            "Americans",
        ),
    ] {
        let mut scene = scene(keys, spy_house);
        assert!(infiltrate(&mut scene), "keys {keys:?} house {spy_house}");
        assert!(
            launched_supers(&mut scene.sim).is_empty(),
            "keys {keys:?} house {spy_house}"
        );
    }
}
