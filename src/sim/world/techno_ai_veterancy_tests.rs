//! Promotion announcement and self-heal pulse at the per-object AI head.

use super::*;
use crate::map::playfield::PlayfieldBounds;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::veterancy::{LEVEL_ELITE, LEVEL_ROOKIE, LEVEL_UNSAMPLED};
use crate::sim::world::SimSoundEvent;

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
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .veterancy
            .rank_cache(),
        LEVEL_UNSAMPLED
    );
    veterancy_promotion_step(&mut sim, id, &rules);
    assert!(
        sim.sound_events.is_empty(),
        "the first sample caches silently"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .veterancy
            .rank_cache(),
        LEVEL_ROOKIE
    );

    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .veterancy
        .set_veteran();
    veterancy_promotion_step(&mut sim, id, &rules);
    let owner = sim.substrate.entities.get(id).unwrap().owner();
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::UnitPromoted {
            owner: o,
            sound_id: Some(sound),
            elite: false,
            ..
        }] if *o == owner && sim.interner.resolve(*sound) == "UpgradeVeteran"
    ));
    assert_eq!(sim.substrate.entities.get(id).unwrap().flashing.frames(), 0);
    veterancy_promotion_step(&mut sim, id, &rules);
    assert_eq!(sim.sound_events.len(), 1, "no repeat while the rank holds");

    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .veterancy
        .set_elite();
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
    assert_eq!(entity.veterancy.rank_cache(), LEVEL_ELITE);
    assert_eq!(entity.flashing.frames(), 150);
    veterancy_promotion_step(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().flashing.frames(),
        150
    );
    //Techno6FAC4D runs the countdown in the later shared AI slot.
    flash_step(&mut sim, id);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().flashing.frames(),
        149
    );
}

/// Issue the ordinary player `Command::Move`.
fn order_move(sim: &mut Simulation, rules: &RuleSet, id: u64) {
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
        },
        Some(rules),
    );
    assert!(issued, "the ordinary move command must issue");
}

/// The mover's live GetCurrentSpeed (`0x004DB1A0`) at full Foot+578. Native
/// keeps no order speed: Drive queries the getter each Process.
fn full_speed(sim: &mut Simulation, rules: &RuleSet, id: u64) -> i32 {
    sim.substrate
        .entities
        .get_mut(id)
        .expect("mover")
        .foot_speed
        .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
    sim.current_speed_for_test(id, rules)
}

/// A mover's live speed carries `FASTER`. gamemd asks the getter for the
/// speed on every frame a ground mover steps (vtable slot `+0x538` from
/// `DriveLocomotionClass::Process_Drive_Track @ 0x004B1274`), so a `FASTER`
/// holder moves 20% faster at stock `VeteranSpeed=1.2`. Stock Grizzly
/// `Speed=7` → per-frame 17 → `ftol(17 * 1.2)` = 20; stock Rhino-shaped
/// control `Speed=6` → 15, unchanged because its ability list omits `FASTER`.
/// Native getter and retained order/prefix evidence: track_speed_native.json.
#[test]
fn gsi_08_12_faster_reaches_the_movers_speed() {
    let (mut sim, rules, id) = spawned_grizzly();
    assert_eq!(
        full_speed(&mut sim, &rules, id),
        17,
        "a rookie Grizzly moves at its plain Speed=7"
    );

    let (mut sim, rules, id) = spawned_grizzly();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .veterancy
        .set_veteran();
    assert_eq!(
        full_speed(&mut sim, &rules, id),
        20,
        "the veteran list carries FASTER, so the mover takes ftol(17 * 1.2) = 20"
    );

    let (mut sim, rules, id) = spawned_grizzly();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .veterancy
        .set_elite();
    assert_eq!(
        full_speed(&mut sim, &rules, id),
        20,
        "HasWeaponAbility @ 0x0070D0D0: elite inherits the veteran list"
    );

    // Negative control: same rank, no FASTER in either list.
    let (mut sim, rules, control_id) = spawned_vehicle("HTNK");
    sim.substrate
        .entities
        .get_mut(control_id)
        .unwrap()
        .veterancy
        .set_elite();
    assert_eq!(
        full_speed(&mut sim, &rules, control_id),
        15,
        "no FASTER token means no multiply, whatever the rank"
    );
}

/// A promotion mid-move speeds the mover up on its next query, with no new
/// order: the order caches no speed, and Drive and Ship read the live getter
/// every Process.
#[test]
fn gsi_08_12_promotion_mid_move_reaches_the_live_speed() {
    let (mut sim, rules, id) = spawned_grizzly();
    order_move(&mut sim, &rules, id);
    assert_eq!(full_speed(&mut sim, &rules, id), 17);

    // Silent first sample, then the veteran crossing while the path is live.
    veterancy_promotion_step(&mut sim, id, &rules);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .veterancy
        .set_veteran();
    veterancy_promotion_step(&mut sim, id, &rules);

    assert_eq!(
        full_speed(&mut sim, &rules, id),
        20,
        "the promoted mover's getter carries FASTER without a new order"
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
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .health
        .set_for_test(100);
    sim.session.binary_frame = 14;
    self_heal_step(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().health.current(),
        100,
        "a rookie without SelfHealing= never pulses"
    );

    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .veterancy
        .set_elite();
    self_heal_step(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().health.current(),
        101
    );
    sim.session.binary_frame = 15;
    self_heal_step(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().health.current(),
        101,
        "off-cadence frames do nothing"
    );
    sim.session.binary_frame = 28;
    self_heal_step(&mut sim, id, &rules);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().health.current(),
        102
    );
}
