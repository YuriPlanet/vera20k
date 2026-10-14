//! Promotion announcement and self-heal pulse at the per-object AI head.

use super::*;
use crate::map::playfield::PlayfieldBounds;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::veterancy::{self, LEVEL_ELITE, LEVEL_ROOKIE, LEVEL_UNSAMPLED};
use crate::sim::world::SimSoundEvent;
use crate::util::fixed_math::SimFixed;

/// `[MTNK]` carries the stock Grizzly's own lines verbatim (`rulesmd.ini`:
/// `Strength=300`, `Speed=7`, `Cost=700`,
/// `VeteranAbilities=STRONGER,FIREPOWER,SIGHT,FASTER`,
/// `EliteAbilities=SELF_HEAL,STRONGER,FIREPOWER,ROF`) and `[General] RepairRate`
/// and `VeteranSpeed` are the stock values, so the cadence and speed pins below
/// are stock-type pins, not synthetic ones. `[HTNK]` is the negative control:
/// stock authors `FASTER` on it too, so the ability list here is deliberately
/// trimmed to prove the gate fires on the list and not on the rank alone.
fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\nRepairRate=.016\nVeteranSpeed=1.2\n\
         [AudioVisual]\nUpgradeVeteranSound=UpgradeVeteran\nUpgradeEliteSound=UpgradeElite\nEliteFlashTimer=150\n\
         [VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nSpeed=7\nCost=700\nROT=5\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\nVeteranAbilities=STRONGER,FIREPOWER,SIGHT,FASTER\nEliteAbilities=SELF_HEAL,STRONGER,FIREPOWER,ROF\n\
         [HTNK]\nStrength=400\nSpeed=6\nCost=900\nROT=5\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\nVeteranAbilities=STRONGER,FIREPOWER,SIGHT\nEliteAbilities=STRONGER,FIREPOWER,ROF\n",
    ))
    .expect("veterancy AI rules")
}

fn spawned_grizzly() -> (Simulation, RuleSet, u64) {
    spawned_vehicle("MTNK")
}

fn spawned_vehicle(type_id: &str) -> (Simulation, RuleSet, u64) {
    let rules = rules();
    let mut sim = Simulation::with_seed(0x5E7E_2A4C);
    sim.fog.width = 64;
    sim.fog.height = 64;
    sim.playfield_bounds = Some(PlayfieldBounds::from_normalized_local_size(
        64, 2, 2, 56, 52,
    ));
    let bounds = sim.playfield_bounds.unwrap();
    let (rx, ry) = (8u16..56)
        .flat_map(|ry| (8u16..56).map(move |rx| (rx, ry)))
        .find(|&(rx, ry)| bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
        .expect("interior mode-one cell");
    let id = sim
        .spawn_object_at_height(type_id, "Soviet", rx, ry, 0, 0, &rules)
        .unwrap();
    (sim, rules, id)
}

/// `AI_Update @ 0x006FA054..0x006FA145`: silent first sample, one cue per
/// crossing carrying the matching `[AudioVisual]` sound, the elite flash seed
/// on the elite crossing only, and the countdown afterwards.
#[test]
fn gsi_08_12_promotion_step_announces_each_crossing_once() {
    let (mut sim, rules, id) = spawned_grizzly();
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().veterancy_rank_cache,
        LEVEL_UNSAMPLED
    );
    veterancy_promotion_step(&mut sim, id, &rules);
    assert!(
        sim.sound_events.is_empty(),
        "the first sample caches silently"
    );
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().veterancy_rank_cache,
        LEVEL_ROOKIE
    );

    veterancy::set_veteran(sim.substrate.entities.get_mut(id).unwrap());
    veterancy_promotion_step(&mut sim, id, &rules);
    let owner = sim.substrate.entities.get(id).unwrap().owner;
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::UnitPromoted {
            owner: o,
            sound_id: Some(sound),
            elite: false,
            ..
        }] if *o == owner && sim.interner.resolve(*sound) == "UpgradeVeteran"
    ));
    assert_eq!(sim.substrate.entities.get(id).unwrap().flash_countdown(), 0);
    veterancy_promotion_step(&mut sim, id, &rules);
    assert_eq!(sim.sound_events.len(), 1, "no repeat while the rank holds");

    veterancy::set_elite(sim.substrate.entities.get_mut(id).unwrap());
    veterancy_promotion_step(&mut sim, id, &rules);
    assert!(matches!(
        sim.sound_events.last(),
        Some(SimSoundEvent::UnitPromoted {
            sound_id: Some(sound),
            elite: true,
            ..
        }) if sim.interner.resolve(*sound) == "UpgradeElite"
    ));
    let entity = sim.substrate.entities.get(id).unwrap();
    assert_eq!(entity.veterancy_rank_cache, LEVEL_ELITE);
    assert_eq!(entity.flash_countdown(), 150);
    veterancy_promotion_step(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().flash_countdown(),
        150
    );
    //Techno6FAC4D runs the countdown in the later shared AI slot.
    elite_flash_step(&mut sim, id);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().flash_countdown(),
        149
    );
}

/// Issue the ordinary player `Command::Move` and read its order-speed stamp.
/// Native keeps no such stamp: Drive queries the live getter each Process.
fn move_order_speed(sim: &mut Simulation, rules: &RuleSet, id: u64) -> SimFixed {
    let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
    sim.install_fixture_path_grid(Some(&grid));
    let (rx, ry) = {
        let e = sim.substrate.entities.get(id).expect("mover");
        (e.position.rx, e.position.ry)
    };
    let issued = sim.apply_command(
        "Soviet",
        &crate::sim::command::Command::Move {
            entity_id: id,
            target_rx: rx + 6,
            target_ry: ry + 6,
            queue: false,
        },
        Some(rules),
    );
    assert!(issued, "the ordinary move command must issue");
    sim.substrate
        .entities
        .get(id)
        .and_then(|e| e.movement_target.as_ref())
        .expect("the move order attached a movement target")
        .speed
}

/// Fresh ordinary `Command::Move` orders stamp the adjusted type speed via
/// `Simulation::resolve_move_info` and the canonical Unit destination setter.
/// Each rank starts without a NavCom: Unit741970 returns without any write
/// for a repeated destination unless forced. That separate case is replayed
/// by `repeated_move_orders_keep_the_stamp_but_sample_live_native_speed`.
///
/// gamemd asks the getter for the speed on every frame a ground mover steps
/// (vtable slot `+0x538` from `DriveLocomotionClass::Process_Drive_Track
/// @ 0x004B1274`), so an ordered move by a `FASTER` holder is 20% faster at
/// stock `VeteranSpeed=1.2`. Stock Grizzly `Speed=7` → per-frame 17 →
/// `ftol(17 * 1.2)` = 20; stock Rhino-shaped control `Speed=6` → 15, unchanged
/// because its ability list omits `FASTER`.
/// Native getter and retained order/prefix evidence: track_speed_native.json.
#[test]
fn gsi_08_12_faster_reaches_the_ordinary_move_order() {
    const FRAMES_PER_SECOND: i32 = 15;
    let (mut sim, rules, id) = spawned_grizzly();

    let rookie = move_order_speed(&mut sim, &rules, id);
    assert_eq!(
        rookie,
        SimFixed::from_num(17 * FRAMES_PER_SECOND),
        "a rookie Grizzly moves at its plain Speed=7"
    );

    let (mut sim, rules, id) = spawned_grizzly();
    veterancy::set_veteran(sim.substrate.entities.get_mut(id).unwrap());
    let veteran = move_order_speed(&mut sim, &rules, id);
    assert_eq!(
        veteran,
        SimFixed::from_num(20 * FRAMES_PER_SECOND),
        "the veteran list carries FASTER, so the ordered move takes ftol(17 * 1.2) = 20"
    );

    let (mut sim, rules, id) = spawned_grizzly();
    veterancy::set_elite(sim.substrate.entities.get_mut(id).unwrap());
    let elite = move_order_speed(&mut sim, &rules, id);
    assert_eq!(
        elite,
        SimFixed::from_num(20 * FRAMES_PER_SECOND),
        "HasWeaponAbility @ 0x0070D0D0: elite inherits the veteran list"
    );

    // Negative control: same rank, no FASTER in either list.
    let (mut sim, rules, control_id) = spawned_vehicle("HTNK");
    veterancy::set_elite(sim.substrate.entities.get_mut(control_id).unwrap());
    assert_eq!(
        move_order_speed(&mut sim, &rules, control_id),
        SimFixed::from_num(15 * FRAMES_PER_SECOND),
        "no FASTER token means no multiply, whatever the rank"
    );
}

/// The promotion detector also refreshes VERA's scheduling adapter stamp.
/// Drive and Ship query their speed independently every Process; the native
/// repeated-order history tests that live getter without this AI refresh.
#[test]
fn gsi_08_12_promotion_mid_move_refreshes_the_order_speed_stamp() {
    const FRAMES_PER_SECOND: i32 = 15;
    let (mut sim, rules, id) = spawned_grizzly();
    assert_eq!(
        move_order_speed(&mut sim, &rules, id),
        SimFixed::from_num(17 * FRAMES_PER_SECOND)
    );

    // Silent first sample, then the veteran crossing while the path is live.
    veterancy_promotion_step(&mut sim, id, &rules);
    veterancy::set_veteran(sim.substrate.entities.get_mut(id).unwrap());
    veterancy_promotion_step(&mut sim, id, &rules);

    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .and_then(|e| e.movement_target.as_ref())
            .expect("still moving")
            .speed,
        SimFixed::from_num(20 * FRAMES_PER_SECOND),
        "the promotion detector refreshed the adapter without a new order"
    );
}

/// `AI_Update @ 0x006FA743..0x006FA757`: `+1` health on the shared
/// `frame % ftol(RepairRate * 900)` pulse, for an eligible object only.
///
/// Pinned on a stock type with stock data: `[MTNK]` authors
/// `EliteAbilities=SELF_HEAL,...` and `[General] RepairRate=.016`, so the
/// cadence is `ftol(0.016 * 900)` = **14 frames** — the interval every one of
/// the 63 `SELF_HEAL` authors and 22 `SelfHealing=yes` types now regenerates
/// on. A rookie Grizzly never pulses; an elite one gains 1 HP every 14 frames.
#[test]
fn gsi_08_12_self_heal_pulses_one_point_on_the_repair_rate_cadence() {
    let (mut sim, rules, id) = spawned_grizzly();
    sim.substrate.entities.get_mut(id).unwrap().health.current = 100;
    sim.session.binary_frame = 14;
    self_heal_step(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().health.current,
        100,
        "a rookie without SelfHealing= never pulses"
    );

    veterancy::set_elite(sim.substrate.entities.get_mut(id).unwrap());
    self_heal_step(&mut sim, id, &rules);
    assert_eq!(sim.substrate.entities.get(id).unwrap().health.current, 101);
    sim.session.binary_frame = 15;
    self_heal_step(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().health.current,
        101,
        "off-cadence frames do nothing"
    );
    sim.session.binary_frame = 28;
    self_heal_step(&mut sim, id, &rules);
    assert_eq!(sim.substrate.entities.get(id).unwrap().health.current, 102);
}
