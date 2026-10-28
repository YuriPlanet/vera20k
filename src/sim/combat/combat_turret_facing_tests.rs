//! Integration tests for turret rotation + fire decision parity.
//!
//! Verifies the FacingClass-driven combat behavior end-to-end through
//! `Simulation::advance_tick`, covering 1-tick acquisition latency,
//! mid-rotation retarget and slow vs fast ROT alignment timing, with each
//! turret turning and firing in its own LogicClass visit.

use std::collections::BTreeMap;

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::movement::FacingClass;
use crate::sim::movement::turret::{body_facing_to_turret, facing_update};
use crate::sim::power_system::PowerState;
use crate::sim::world::Simulation;

/// Unit736F78..736FAC re-aims a turret on FACING before Facing_Update,
/// including when +6AF blocks that later routine's ordinary aiming arm.
/// A retail IFV's homing projectile bypasses the ROTATING refusal.
#[test]
fn retail_ifv_retargets_its_turret_during_an_unfinished_turn() {
    use crate::sim::command::{Command, CommandEnvelope};
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x736f78);
    sim.session.game_options_mut_for_test().short_game = false;
    let ifv = sim
        .spawn_object("FV", "Americans", 12, 12, 0, rules)
        .unwrap();
    let first = sim
        .spawn_object("MTNK", "Russians", 15, 12, 0, rules)
        .unwrap();
    let second = sim
        .spawn_object("MTNK", "Russians", 9, 12, 0, rules)
        .unwrap();
    let owner = sim.substrate.entities.get(ifv).unwrap().owner();
    sim.refresh_object_sight(ifv, rules, 0, crate::sim::world::FrameEffects::default());
    let actor = sim.substrate.entities.get(ifv).unwrap();
    let obj = rules.object("FV").unwrap();
    let weapon = super::combat_weapon::current_weapon(
        actor,
        obj,
        &sim.substrate.entities,
        rules,
        &sim.interner,
    )
    .unwrap();
    let projectile = weapon
        .projectile
        .as_deref()
        .and_then(|id| rules.projectile(id))
        .unwrap();
    assert!(projectile.rot > 0, "the real selected IFV weapon is homing");

    let advance = |sim: &mut Simulation, order: Option<Command>| {
        let orders: Vec<_> = order
            .into_iter()
            .map(|payload| CommandEnvelope::new(owner, sim.session.tick() + 1, payload))
            .collect();
        let grid = sim.path_grid_snapshot();
        let output = sim.advance_tick(&orders, Some(rules), grid.as_deref(), None, 67);
        assert!(output.frame_committed, "the ordinary frame advances");
        output
    };
    // Orders execute at the frame tail; the next visit starts the turn.
    advance(
        &mut sim,
        Some(Command::Attack {
            attacker_id: ifv,
            target_id: first,
        }),
    );
    advance(&mut sim, None);
    let actor = sim.substrate.entities.get(ifv).unwrap();
    let first_aim =
        crate::sim::movement::turret::facing_toward_target(actor, &TargetKind::Entity(first), &sim)
            .unwrap();
    assert_eq!(
        actor.barrel_facing.as_ref().unwrap().destination(),
        first_aim
    );
    assert!(actor.turret_rotation_latch.is_set());
    assert!(
        actor
            .barrel_facing
            .as_ref()
            .unwrap()
            .is_rotating(sim.session.binary_frame())
    );

    advance(
        &mut sim,
        Some(Command::Attack {
            attacker_id: ifv,
            target_id: second,
        }),
    );
    let actor = sim.substrate.entities.get(ifv).unwrap();
    assert_eq!(
        actor.attack_target.target(),
        Some(TargetKind::Entity(second))
    );
    assert!(
        actor.turret_rotation_latch.is_set(),
        "retargeting did not clear the live latch"
    );
    assert!(
        actor
            .barrel_facing
            .as_ref()
            .unwrap()
            .is_rotating(sim.session.binary_frame())
    );
    let expected = crate::sim::movement::turret::facing_toward_target(
        actor,
        &TargetKind::Entity(second),
        &sim,
    )
    .unwrap();
    assert_ne!(expected, first_aim);
    assert_eq!(
        super::fire_error_world::FireSubject {
            world: &sim,
            rules,
            overlay_registry: None,
            fog: None,
            firer: actor,
            obj,
            target: Some(TargetKind::Entity(second)),
            weapon_index: 0,
        }
        .fire_error(true),
        super::fire_error::FireError::Facing
    );
    advance(&mut sim, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(ifv)
            .unwrap()
            .barrel_facing
            .as_ref()
            .unwrap()
            .destination(),
        expected,
        "Fire_At_Target re-aims before Facing_Update's latched aiming arm",
    );
}

/// Minimal rules with MTNK at the given ROT byte.
fn rules_with_mtnk_rot(rot: u32) -> RuleSet {
    let ini_str: String = format!(
        "[VehicleTypes]\n0=MTNK\n\n\
[InfantryTypes]\n0=ENGI\n\n\
[BuildingTypes]\n0=GAPILE\n\n\
[AircraftTypes]\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\nROT={rot}\nTurret=yes\n\n\
[ENGI]\nStrength=75\nArmor=none\nSpeed=4\n\n\
[GAPILE]\nStrength=300\nArmor=heavy\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n"
    );
    let ini: IniFile = IniFile::from_str(&ini_str);
    RuleSet::from_ini(&ini).expect("rules_with_mtnk_rot should parse")
}

/// Spawn a turreted attacker at (rx, ry) facing north (0) with the given ROT byte.
fn spawn_turreted(sim: &mut Simulation, stable_id: u64, rx: u16, ry: u16, rot_byte: u8) {
    let mut entity = GameEntity::test_default(stable_id, "MTNK", "Americans", rx, ry);
    entity.barrel_facing = Some(FacingClass::new(body_facing_to_turret(0), rot_byte));
    sim.substrate.entities.insert(entity);
    use_test_interner(sim);
    assert!(matches!(
        sim.reveal(stable_id),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
}

/// Spawn a passive target at (rx, ry).
fn spawn_target(sim: &mut Simulation, stable_id: u64, rx: u16, ry: u16) {
    let entity = GameEntity::test_default(stable_id, "GAPILE", "Soviet", rx, ry);
    sim.substrate.entities.insert(entity);
    use_test_interner(sim);
    assert!(matches!(
        sim.reveal(stable_id),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
}

/// Replace sim's interner with the thread-local test interner so entity
/// type_ref / owner IDs from `GameEntity::test_default` (which uses
/// `test_intern`) resolve correctly inside sim functions. Bind after entity
/// construction and before Reveal/Unlimbo, which resolves identities now;
/// cloning the interner only after reveal is too late.
fn use_test_interner(sim: &mut Simulation) {
    sim.interner = crate::sim::intern::test_interner();
}

// --- L2 unit_post shadow acceptance tests ---

#[test]
fn unit_authoritative_fire_kills_target_via_advance_tick() {
    // Drive a turreted Unit through acquisition, alignment, repeated fire, and the
    // target's death via advance_tick — exercising the authoritative path end-to-end:
    // shared-body fire + the per-object unit_post facing pass (incl. the retarget after
    // the kill) + the deferred-death batch. A facing/fire break would show as the
    // target never dying or a panic; the per-tick state_hash gate is the stronger net
    // for hash-affecting drift.
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 100); // fast ROT — aligns within a frame
    spawn_target(&mut sim, 2, 5, 8); // 3 cells south, in range (Range=6)
    let rules = rules_with_mtnk_rot(100);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(2)));

    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mark_live_contact_with(2);
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .mark_live_contact_with(1);
    let start_hp = sim.substrate.entities.get(2).unwrap().health.current();
    let mut fired = false;
    let mut target_gone = false;
    for _ in 0..400 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        match sim.substrate.entities.get(2) {
            Some(t) => {
                if t.health.current() < start_hp {
                    fired = true;
                }
            }
            None => {
                target_gone = true;
                break;
            }
        }
    }
    assert!(fired, "the Unit should have fired and damaged the target");
    assert!(
        target_gone,
        "repeated fire should have killed and despawned the target"
    );
    let survivor = sim.substrate.entities.get(1).unwrap();
    assert!(survivor.attack_target.target().is_none());
    assert!(!survivor.has_live_contact_with(2));
}

// --- S3 per-object facing-destination tests (read in the P2 window) ---

/// Call `tick_combat_with_fog` directly (mirrors the combat_tests direct-call
/// pattern) so the shot's fire events are observable.
fn run_combat_direct(
    sim: &mut Simulation,
    rules: &RuleSet,
) -> crate::sim::combat::CombatTickResult {
    let live_order = sim.live_object_order_snapshot();
    crate::sim::combat::tick_combat_with_fog(
        &mut sim.substrate.entities,
        &mut sim.substrate.occupancy,
        rules,
        &mut sim.interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        None,
        None,
        None,
        sim.session.tick(),
        67,
        sim.session.binary_frame(),
        &live_order,
        None, // radiation state — not under test here
        &mut sim.scenario_rng,
    )
}

#[test]
fn save_load_round_trip_on_kill_tick() {
    // A save taken on the kill tick — where the barrel destination still holds
    // a now-dead target's facing and the hashed mission value reflects the
    // dispatch-time machines — must restore an identical state hash (barrel
    // FacingClass and MissionCom both round-trip via serde; load trusts the
    // serialized values, no post-load re-derive).
    use crate::sim::snapshot::GameSnapshot;
    let mut sim = Simulation::new();
    spawn_turreted(&mut sim, 1, 5, 5, 100);
    spawn_target(&mut sim, 2, 5, 8);
    let rules = rules_with_mtnk_rot(100);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(2)));
    let toward_target = {
        let e = sim.substrate.entities.get(1).unwrap();
        facing_update(e, &sim, &rules, &sim.interner, sim.session.binary_frame())
            .turret_destination
            .expect("turreted")
    };
    sim.substrate.entities.get_mut(1).unwrap().barrel_facing =
        Some(FacingClass::new(toward_target, 100));
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .health
        .set_for_test(10);

    sim.advance_tick(&[], Some(&rules), None, None, 67); // kill tick

    // Native in-scenario load restarts Scenario RNG from Seed0; isolate the
    // kill-tick/turret persistence hash on that same post-load cursor.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash_before = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("load").sim;
    restored.rebuild_logic_membership(); // the real post-deserialize step
    assert_eq!(
        restored.state_hash(),
        hash_before,
        "kill-tick save/load must restore an identical state hash"
    );
}

// --- M4: the turretless body gate, the latch, the dwell and the widened
//     tolerance (rows 126 GSI-08.14 / 124 GSI-08.04) ---

/// The exact 16-bit facing from cell (5,5) to cell (5,9) that the fire gate
/// computes. `atan2`+`ftol` does not land on a round 0x8000, so offsets in the
/// tolerance tests are taken from this value rather than from a constant.
fn facing_from_5_5_to_5_9() -> u16 {
    crate::sim::movement::turret::facing_toward_lepton(
        5,
        5,
        crate::util::fixed_math::SimFixed::from_num(128),
        crate::util::fixed_math::SimFixed::from_num(128),
        5,
        9,
        crate::util::fixed_math::SimFixed::from_num(128),
        crate::util::fixed_math::SimFixed::from_num(128),
    )
}

/// Rules with an `MTNK` whose 105mm fires a HOMING projectile (`ROT=` non-zero
/// on the `[Projectile]` section) - the input that widens the fire tolerance.
fn rules_with_homing_projectile(projectile_rot: u32) -> RuleSet {
    let ini_str: String = format!(
        "[VehicleTypes]\n0=MTNK\n\n\
[InfantryTypes]\n\n\
[BuildingTypes]\n0=GAPILE\n\n\
[AircraftTypes]\n\n\
[Projectiles]\n0=Homer\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\nROT=5\nTurret=yes\n\n\
[GAPILE]\nStrength=300\nArmor=heavy\n\n\
[Homer]\nROT={projectile_rot}\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\nProjectile=Homer\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n"
    );
    let ini: IniFile = IniFile::from_str(&ini_str);
    RuleSet::from_ini(&ini).expect("homing fixture should parse")
}

#[test]
fn gsi_08_03_homing_projectile_widens_the_fire_tolerance_to_0x1000() {
    // `0x007412BC`..`0x007412CC`: the tolerance byte is built from the
    // PROJECTILE's `ROT=` (`WeaponType+0xA0` -> `BulletTypeClass+0x2DC`) with
    // `NEG`/`SBB`/`AND 8`/`ADD 8` - 8 when that ROT is zero, 0x10 when it is
    // not - then `MOV CH,BL` shifts it into the high byte. A missile turret
    // therefore shoots up to 1/16 of a turn off-axis.
    fn fires_at_offset(rules: &RuleSet, offset: u16) -> bool {
        let mut sim = Simulation::new();
        spawn_turreted(&mut sim, 1, 5, 5, 5);
        spawn_target(&mut sim, 2, 5, 9); // due south -> 0x8000
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .attack_target
            .set_for_test(Some(TargetKind::Entity(2)));
        sim.substrate.entities.get_mut(1).unwrap().barrel_facing = Some(FacingClass::new(
            facing_from_5_5_to_5_9().wrapping_add(offset),
            5,
        ));
        !run_combat_direct(&mut sim, rules)
            .consequences
            .fire_events()
            .is_empty()
    }

    let straight = rules_with_homing_projectile(0);
    let homing = rules_with_homing_projectile(30);
    assert!(
        fires_at_offset(&straight, 0x0800),
        "0x0800 itself passes - the native JGE skips the error return"
    );
    assert!(
        !fires_at_offset(&straight, 0x0801),
        "a straight-flying projectile is refused one unit past 0x0800"
    );
    assert!(
        fires_at_offset(&homing, 0x0801),
        "a homing projectile widens the slack"
    );
    assert!(
        fires_at_offset(&homing, 0x1000),
        "all the way to 0x1000 inclusive"
    );
    assert!(!fires_at_offset(&homing, 0x1001), "and no further");
}

#[test]
fn gsi_08_04_voxel_turret_building_needs_an_exact_match_then_snaps_within_one_rot_step() {
    // `BuildingClass::GetFireError @ 0x00448010`..`0x00448045` narrows the
    // tolerance to `0x0000` when `BuildingTypeClass+0x16C5` (`TurretAnimIsVoxel=`)
    // is set, and `BuildingClass::Mission_Attack @ 0x0044B068`..`0x0044B0B3`
    // gives it a second chance in the SAME visit: within one `ROT=` step it
    // snaps `+0x388` with `UpdateFacing` and re-runs the error check.
    fn tower_rules(voxel: bool, rot: i32) -> RuleSet {
        let ini_str: String = format!(
            "[VehicleTypes]\n\n\
[InfantryTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GTGCAN\n1=TGT\n\n\
[GTGCAN]\nStrength=900\nArmor=concrete\nPrimary=105mm\nROT={rot}\nTurret=yes\n\
TurretAnimIsVoxel={}\n\n\
[TGT]\nStrength=300\nArmor=heavy\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
            if voxel { "true" } else { "false" }
        );
        RuleSet::from_ini(&IniFile::from_str(&ini_str)).expect("tower fixture should parse")
    }
    fn fires_at_offset(voxel: bool, rot: i32, offset: u16) -> bool {
        let rules = tower_rules(voxel, rot);
        let mut sim = Simulation::new();
        let mut tower = GameEntity::test_default_of_category(
            1,
            "GTGCAN",
            "Americans",
            5,
            5,
            crate::map::entities::EntityCategory::Structure,
        );
        tower.lifecycle.set_in_limbo_for_test(false);
        tower.body_facing = FacingClass::new(facing_from_5_5_to_5_9().wrapping_add(offset), rot);
        tower
            .attack_target
            .set_for_test(Some(TargetKind::Entity(2)));
        sim.substrate.entities.insert(tower);
        let target = GameEntity::test_default(2, "TGT", "Soviet", 5, 9); // due south -> 0x8000
        sim.substrate.entities.insert(target);
        use_test_interner(&mut sim);
        sim.reveal(1);
        sim.reveal(2);
        !run_combat_direct(&mut sim, &rules)
            .consequences
            .fire_events()
            .is_empty()
    }

    // ROT=1 means one step is 0x0100.
    assert!(fires_at_offset(true, 1, 0), "an exact match always passes");
    assert!(
        fires_at_offset(true, 1, 0x0100),
        "within one ROT step the voxel turret snaps and fires the same visit"
    );
    assert!(
        !fires_at_offset(true, 1, 0x0101),
        "one unit further and it must turn gradually instead"
    );
    // An SHP turret keeps the ordinary 0x0800 slack and never snaps.
    assert!(
        fires_at_offset(false, 1, 0x0800),
        "an SHP-turret building keeps the 0x0800 tolerance"
    );
    assert!(
        !fires_at_offset(false, 1, 0x0801),
        "and is refused past it, with no snap-and-retry"
    );
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_fire_turn.json",
    ))
    .unwrap();
    for row in native.as_array().unwrap() {
        let rot = row["rot"].as_i64().unwrap() as i32;
        let delta = row["delta"].as_u64().unwrap() as u16;
        assert_eq!(
            fires_at_offset(true, rot, delta),
            row["retry"].as_bool().unwrap(),
            "{row}"
        );
    }
}

/// `UnitClass::Facing_Update @ 0x00736990` executed frame after frame
/// (`tools/spatial_oracle/turret_cadence.py`, every callee native): the aim
/// `Set` and its latch, re-aims at a moving target only once an arc ends, the
/// idle return exactly `GuardAreaTargetingDelay + 5` frames after the last
/// shot, hull turns, and FacingClass interpolation — turret current and
/// desired facing and the `+0x6AF` latch, frame for frame, through VERA's
/// `facing_update` and `apply_unit_facing`. Rows exercising arms VERA does not
/// model on this path (OmniFire, TurretLocked, a Battle Bunker, no weapon)
/// are left out, as are
/// targets off the map (negative world coordinates), which VERA's cell
/// positions cannot hold.
#[test]
fn turret_cadence_matches_the_original() {
    use crate::sim::movement::turret::facing_update;
    use crate::util::fixed_math::SimFixed;

    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/turret_cadence.json",
    ))
    .unwrap();
    let off_map = |input: &serde_json::Value| {
        std::iter::once(&input["target_coord"])
            .chain(
                input["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|event| &event["value"]),
            )
            .filter_map(|value| value.as_array())
            .any(|coord| coord.iter().take(2).any(|axis| axis.as_i64().unwrap() < 0))
    };
    let mut compared = 0;
    let mut issues = Vec::new();
    for row in &rows {
        let input = &row["input"];
        let int = |key: &str| input[key].as_i64().unwrap_or(0);
        let turret_spins = int("turret_spins") != 0;
        let weapon = &input["weapon"];
        if weapon["present"] != 1
            || weapon["omni_fire"] != 0
            || weapon["turret_locked"] != 0
            || int("bunkered") != 0
            || (int("hull_rot") != int("turret_rot") && !turret_spins)
            || off_map(input)
        {
            continue;
        }
        let rot = int("turret_rot") as i32;
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nGuardAreaTargetingDelay={}\n\
             [VehicleTypes]\n0=MTNK\n[InfantryTypes]\n[BuildingTypes]\n0=GAPILE\n[AircraftTypes]\n\
             [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\nROT={rot}\nTurret=yes\n\
             TurretSpins={}\nIsSimpleDeployer={}\n\
             [GAPILE]\nStrength=300\nArmor=heavy\n\
             [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
            int("delay"),
            if turret_spins { "yes" } else { "no" },
            if int("simple_deployer") != 0 {
                "yes"
            } else {
                "no"
            }
        )))
        .unwrap();
        let coord = |value: &serde_json::Value| {
            let at = |i: usize| value[i].as_i64().unwrap() as i32;
            (at(0), at(1))
        };
        let place = |entity: &mut GameEntity, (x, y): (i32, i32)| {
            entity.position.set_rx_for_test((x / 256) as u16);
            entity.position.set_ry_for_test((y / 256) as u16);
            entity
                .position
                .set_sub_x_for_test(SimFixed::from_num(x % 256));
            entity
                .position
                .set_sub_y_for_test(SimFixed::from_num(y % 256));
        };
        let mut sim = Simulation::new();
        let mut unit = GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
        place(&mut unit, coord(&input["unit_coord"]));
        let hull_init = int("hull_init") as u16;
        unit.body_facing = FacingClass::new(hull_init, rot);
        unit.barrel_facing = Some(FacingClass::new(int("turret_init") as u16, rot));
        unit.last_fire_frame.set_for_test(int("last_fire"));
        unit.turret_rotation_latch
            .set_for_test(int("latch_init") != 0);
        unit.set_unit_simple_deploy_for_test(int("deployed") != 0, false, false);
        unit.lift.set_swap_active_for_test(int("magnetron") != 0);
        let mut navcom = GameEntity::test_default(3, "MTNK", "Americans", 0, 0);
        if !input["navcom_coord"].is_null() {
            place(&mut navcom, coord(&input["navcom_coord"]));
            unit.navigation
                .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::object(3)));
        }
        let mut target = GameEntity::test_default(2, "GAPILE", "Soviet", 0, 0);
        if !input["target_coord"].is_null() {
            place(&mut target, coord(&input["target_coord"]));
            unit.attack_target.set_for_test(Some(TargetKind::Entity(2)));
        }
        sim.substrate.entities.insert(unit);
        sim.substrate.entities.insert(target);
        sim.substrate.entities.insert(navcom);
        use_test_interner(&mut sim);
        let start = int("start") as u32;
        let events = input["events"].as_array().unwrap();
        for (index, native) in row["frames"].as_array().unwrap().iter().enumerate() {
            let frame = start + index as u32;
            for event in events.iter().filter(|event| event["at"] == index) {
                let value = &event["value"];
                match event["kind"].as_str().unwrap() {
                    "target_coord" => {
                        place(sim.substrate.entities.get_mut(2).unwrap(), coord(value));
                        sim.substrate
                            .entities
                            .get_mut(1)
                            .unwrap()
                            .attack_target
                            .set_for_test(Some(TargetKind::Entity(2)));
                    }
                    "clear_target" => {
                        sim.substrate
                            .entities
                            .get_mut(1)
                            .unwrap()
                            .attack_target
                            .set_for_test(None);
                    }
                    "navcom_coord" => {
                        place(sim.substrate.entities.get_mut(3).unwrap(), coord(value));
                        sim.substrate
                            .entities
                            .get_mut(1)
                            .unwrap()
                            .navigation
                            .set_nav_com_for_test(Some(
                                crate::sim::components::NavTargetRef::object(3),
                            ));
                    }
                    "clear_navcom" => {
                        sim.substrate
                            .entities
                            .get_mut(1)
                            .unwrap()
                            .navigation
                            .set_nav_com_for_test(None);
                    }
                    "last_fire" => {
                        sim.substrate
                            .entities
                            .get_mut(1)
                            .unwrap()
                            .last_fire_frame
                            .set_for_test(value.as_i64().unwrap());
                    }
                    "hull_set" => {
                        let unit = sim.substrate.entities.get_mut(1).unwrap();
                        unit.body_facing.set(value.as_u64().unwrap() as u16, frame);
                    }
                    other => panic!("unmodelled event {other}"),
                }
            }
            let update = facing_update(
                sim.substrate.entities.get(1).unwrap(),
                &sim,
                &rules,
                &sim.interner,
                frame,
            );
            crate::sim::world::unit_post::apply_unit_facing(
                &mut sim.substrate.entities,
                &[crate::sim::combat::UnitFacingUpdate::from_facing_update(
                    1, update,
                )],
                &rules,
                &sim.interner,
                frame,
            );
            let unit = sim.substrate.entities.get(1).unwrap();
            let barrel = unit.barrel_facing.as_ref().unwrap();
            let actual = (
                barrel.current(frame),
                barrel.destination(),
                unit.turret_rotation_latch.is_set(),
            );
            let turret = native["turret"].as_array().unwrap();
            let expected = (
                turret[0].as_u64().unwrap() as u16,
                turret[1].as_u64().unwrap() as u16,
                native["latch"].as_u64().unwrap() != 0,
            );
            if actual != expected {
                issues.push(format!(
                    "{} frame {frame}: {actual:?} != {expected:?}",
                    input["name"]
                ));
                break;
            }
            if turret_spins {
                let actual_raw = serde_json::json!([
                    barrel.current(frame),
                    barrel.destination(),
                    barrel.start_word(),
                    barrel.timer_start_frame().unwrap_or(0),
                    barrel.timer_duration(),
                    barrel.rot_per_frame(),
                ]);
                assert_eq!(
                    actual_raw, native["turret"],
                    "{} frame {frame}: full turret facing state",
                    input["name"]
                );
            }
        }
        compared += 1;
    }
    assert!(compared >= 52, "only {compared} rows compared");
    assert!(issues.is_empty(), "{}", issues.join("\n"));
}
