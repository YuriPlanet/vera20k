//! Phobos `BuildingSell` / `BuildingUndeploy` through the production
//! detonation path (`Ext/WarheadType/Detonate.cpp:266-352`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::attack_target::TargetKind;
use crate::sim::combat::building_sell;
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

/// A foundation per building plus the Buildup SHP `Sell_Back @ 0x00447110`
/// insists on before any of the four arms runs. `BOTH` is unsellable *and*
/// undeployable, so only a fall-through from the sell arm could reach it.
const ART: &str = "[BLDG]\nFoundation=2x2\n\
    [UNSELL]\nFoundation=2x2\n\
    [STRANGE]\nFoundation=1x1\n\
    [BOTH]\nFoundation=2x2\n";

/// `wh_ini` fills `[WH]`, `bldg_keys` fills `[BLDG]`; `UNSELL` carries
/// `Unsellable=yes`, `STRANGE` is the 1x1 `UndeploysInto=` type
/// `is_1x1_with_undeploy` calls strange, and `BOTH` is unsellable with an
/// `UndeploysInto=` to catch any fall-through from the sell arm.
fn rules_with(wh_ini: &str, bldg_keys: &str) -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=SMIN\n\
         [BuildingTypes]\n0=BLDG\n1=UNSELL\n2=STRANGE\n3=BOTH\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
         [Warheads]\n0=WH\n\
         [WH]\nCellSpread=0\n{wh_ini}\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nCost=100\nPrimary=GUN\n\
         [SMIN]\nStrength=2000\nArmor=none\nSpeed=3\n\
         [BLDG]\nStrength=1000\nArmor=none\n{bldg_keys}\
         [UNSELL]\nStrength=1000\nArmor=none\nUnsellable=yes\n\
         [STRANGE]\nStrength=1000\nArmor=none\nUndeploysInto=SMIN\n\
         [BOTH]\nStrength=1000\nArmor=none\nUnsellable=yes\nUndeploysInto=SMIN\n"
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(ART))
        .expect("sell/undeploy test ruleset");
    for name in ["BLDG", "UNSELL", "STRANGE", "BOTH"] {
        rules.set_buildup_control_for_test(name, [0, 29, 1]);
    }
    rules
}

/// Two rival houses on the flat arena; `target_type` names the building the
/// warhead lands on and `target_house` its owner.
fn scene(
    wh_ini: &str,
    bldg_keys: &str,
    target_type: &str,
    target_house: &str,
) -> (Simulation, RuleSet, u64, u64) {
    let rules = rules_with(wh_ini, bldg_keys);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
        .expect("firer");
    let building = sim
        .spawn_object(target_type, target_house, 22, 20, 0, &rules)
        .expect("building");
    (sim, rules, firer, building)
}

/// One `WarheadTypeExt::DetonateOnOneUnit` pass of `WH` at `target`, the same
/// production path a bullet's detonation takes.
fn detonate_once(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64) {
    let warhead = rules.warhead("WH").expect("warhead");
    let warhead_ref = sim.interner.intern("WH");
    let source_house = sim.entities().get(firer).map(|entity| entity.owner());
    let impact = {
        let coords = crate::sim::movement::ground_pose::object_get_coords(
            sim.entities().get(target).expect("target"),
            sim.resolved_terrain.as_ref(),
        );
        crate::sim::projectile::ProjectileCoord::new(coords.x, coords.y, coords.z)
    };
    let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
    let mut effects = crate::sim::combat::effects::DeathEffects::default();
    crate::sim::combat::crit::detonate(
        sim,
        &mut run,
        rules,
        None,
        warhead,
        warhead_ref,
        firer,
        source_house,
        impact,
        Some(target),
        0,
        false,
        &mut effects,
    );
    run.finish();
}

fn mission(sim: &Simulation, id: u64) -> Option<MissionType> {
    sim.entities()
        .get(id)
        .and_then(|entity| entity.mission.effective().known())
}

fn archive(sim: &Simulation, id: u64) -> Option<TargetKind> {
    sim.entities()
        .get(id)
        .and_then(|entity| entity.archive_target())
}

/// `BuildingSell=` sells a building that `CanBeSold()` and is not strange
/// (`Detonate.cpp:270-279`): the arm clears the archive target and takes the
/// sale either way, so the building leaves its mission for `Selling`.
#[test]
fn building_sell_sells_the_hit_building() {
    let (mut sim, rules, firer, building) = scene("BuildingSell=yes\n", "", "BLDG", "Russians");
    assert_ne!(mission(&sim, building), Some(MissionType::Selling));
    detonate_once(&mut sim, &rules, firer, building);
    assert_eq!(
        mission(&sim, building),
        Some(MissionType::Selling),
        "BuildingSell=yes sells a plain building"
    );
    assert_eq!(
        archive(&sim, building),
        None,
        "the sell arm resets the archive target before Sell(1)"
    );
}

/// `CanBeSold()` refuses an `Unsellable=` type, and without
/// `BuildingSell.IgnoreUnsellable=` the arm does nothing at all — it never
/// falls through to the undeploy arm.
#[test]
fn an_unsellable_building_is_left_alone_without_the_override() {
    let (mut sim, rules, firer, building) = scene("BuildingSell=yes\n", "", "UNSELL", "Russians");
    detonate_once(&mut sim, &rules, firer, building);
    assert_ne!(
        mission(&sim, building),
        Some(MissionType::Selling),
        "an Unsellable= type keeps its mission"
    );
}

/// `BuildingSell.IgnoreUnsellable=` overrides every gate but the missing
/// Buildup (`:271-273`), so the sale goes through.
#[test]
fn ignore_unsellable_overrides_the_gate() {
    let (mut sim, rules, firer, building) = scene(
        "BuildingSell=yes\nBuildingSell.IgnoreUnsellable=yes\n",
        "",
        "UNSELL",
        "Russians",
    );
    detonate_once(&mut sim, &rules, firer, building);
    assert_eq!(
        mission(&sim, building),
        Some(MissionType::Selling),
        "IgnoreUnsellable forces the sale"
    );
}

/// `!pBuilding->IsStrange()` is the 1x1 `UndeploysInto=` predicate
/// (`ObjectType::is_1x1_with_undeploy`): such a building is "strange" and
/// only the override sells it.
#[test]
fn a_strange_building_needs_the_override_too() {
    let (mut sim, rules, firer, building) = scene("BuildingSell=yes\n", "", "STRANGE", "Russians");
    detonate_once(&mut sim, &rules, firer, building);
    assert_ne!(
        mission(&sim, building),
        Some(MissionType::Selling),
        "IsStrange() refuses the plain sell"
    );

    let (mut sim, rules, firer, building) = scene(
        "BuildingSell=yes\nBuildingSell.IgnoreUnsellable=yes\n",
        "",
        "STRANGE",
        "Russians",
    );
    detonate_once(&mut sim, &rules, firer, building);
    assert_eq!(mission(&sim, building), Some(MissionType::Selling));
}

/// `BuildingSell` **returns either way** (`:279`), so a building that fails
/// the gates is never sold *and* never undeployed even when the warhead also
/// asks for `BuildingUndeploy`. `BOTH` is unsellable but undeployable, so a
/// fall-through would show up as a pack-up.
#[test]
fn a_refused_sell_never_falls_through_to_the_undeploy_arm() {
    let (mut sim, rules, firer, building) = scene(
        "BuildingSell=yes\nBuildingUndeploy=yes\n",
        "",
        "BOTH",
        "Russians",
    );
    detonate_once(&mut sim, &rules, firer, building);
    assert_ne!(
        mission(&sim, building),
        Some(MissionType::Selling),
        "the sell arm's early return blocks the undeploy arm too"
    );
}

/// A `BuildingUndeploy=` hit on a type with `UndeploysInto=` packs the
/// building up into that unit (`:284-304`), which runs as the undeploy sale.
#[test]
fn building_undeploy_packs_the_building_into_its_unit() {
    let (mut sim, rules, firer, building) = scene(
        "BuildingUndeploy=yes\n",
        "UndeploysInto=SMIN\n",
        "BLDG",
        "Russians",
    );
    detonate_once(&mut sim, &rules, firer, building);
    assert_eq!(
        mission(&sim, building),
        Some(MissionType::Selling),
        "BuildingUndeploy=yes starts the pack-up"
    );
}

/// A type with no `UndeploysInto=` cannot pack up (`:291-292`).
#[test]
fn a_building_without_undeploys_into_stays() {
    let (mut sim, rules, firer, building) = scene("BuildingUndeploy=yes\n", "", "BLDG", "Russians");
    detonate_once(&mut sim, &rules, firer, building);
    assert_ne!(mission(&sim, building), Some(MissionType::Selling));
}

/// The ordinary (non-`Leave`) arm leaves the archive target alone, so the
/// resulting unit stands where the building stood; `finish_undeploy` only
/// sends it somewhere when an archive was set.
#[test]
fn the_ordinary_undeploy_sets_no_destination() {
    let (mut sim, rules, firer, building) = scene(
        "BuildingUndeploy=yes\n",
        "UndeploysInto=SMIN\n",
        "BLDG",
        "Russians",
    );
    detonate_once(&mut sim, &rules, firer, building);
    assert_eq!(
        archive(&sim, building),
        None,
        "no Leave means no archive target"
    );
}

/// `BuildingUndeploy.Leave=` picks the cell by a 16-direction threat
/// weighting and hands it to the pack-up as the archive target
/// (`Detonate.cpp:304-349`), which `finish_undeploy` turns into the new
/// unit's destination (`world_spawn.rs:3233-3247`).
#[test]
fn leave_relocates_the_destination_away_from_the_scan() {
    use crate::sim::movement::ground_pose::object_center_xy;
    use crate::util::direction_tables::facing16_between;

    let (mut sim, rules, firer, building) = scene(
        "BuildingUndeploy=yes\nBuildingUndeploy.Leave=yes\n",
        "UndeploysInto=SMIN\n",
        "BLDG",
        "Americans",
    );
    // One armed, non-allied unit to the east of the building, so the record
    // has exactly one nonzero bin. The firer shares the building's house and
    // is therefore never counted.
    let threat = sim
        .spawn_object("FIRER", "Russians", 26, 20, 0, &rules)
        .expect("threat");
    detonate_once(&mut sim, &rules, firer, building);

    let Some(TargetKind::Cell(x, y)) = archive(&sim, building) else {
        panic!("Leave= hands the pack-up a cell archive target");
    };
    let cell = (x as i16, y as i16);
    let base = {
        let target = sim.entities().get(building).expect("building");
        // `GetMapCoords()`, with the `(1,1)` offset a foundation over 2 adds.
        let (w, h) = rules
            .object(sim.interner.resolve(target.type_ref()))
            .expect("type")
            .foundation
            .dimensions();
        let bias = i16::from(w > 2 || h > 2);
        (
            (target.position.rx() as i16).wrapping_add(bias),
            (target.position.ry() as i16).wrapping_add(bias),
        )
    };
    assert_ne!(cell, base, "Leave= does not leave the unit where it stood");

    // The pick is the recorded bin's radian step, stated from the two
    // objects' coordinates rather than from the scan that is under test.
    let [bx, by] = object_center_xy(sim.entities().get(building).expect("building"));
    let [tx, ty] = object_center_xy(sim.entities().get(threat).expect("threat"));
    let bin =
        crate::sim::combat::building_sell_leave::threat_bin(facing16_between([bx, by], [tx, ty]));
    assert_eq!(
        cell,
        crate::sim::combat::building_sell_leave::leave_target_cell(base, bin),
        "the archive cell is the threat bin's 14-cell step"
    );
}

/// A non-building target is never touched (`:268`).
#[test]
fn a_non_building_target_is_ignored() {
    let rules = rules_with("BuildingSell=yes\nBuildingUndeploy=yes\n", "");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
        .expect("firer");
    let unit = sim
        .spawn_object("SMIN", "Russians", 22, 20, 0, &rules)
        .expect("unit");
    detonate_once(&mut sim, &rules, firer, unit);
    assert_ne!(
        mission(&sim, unit),
        Some(MissionType::Selling),
        "the arms only ever act on a BuildingClass"
    );
}

/// `relocation_cell` refuses a target that is no longer there, and the arm
/// then leaves the archive alone rather than panicking.
#[test]
fn a_missing_target_sets_no_destination() {
    let mut rules = rules_with("BuildingUndeploy=yes\nBuildingUndeploy.Leave=yes\n", "");
    rules.set_buildup_control_for_test("BLDG", [0, 29, 1]);
    let mut sim = Simulation::with_seed(0x5A_AA_1E);
    assert_eq!(
        building_sell::relocation_cell(&mut sim, &rules, 0x9999),
        None,
        "a missing entity answers None"
    );
}
