//! Phobos `BulletClass_Logics_Debris` (`0x469D1A`,
//! `Ext/Bullet/Hooks.DetonateLogics.cpp:171-242`) through the production
//! `emit_warhead_debris` owner: the `Debris.Conventional` water skip, the
//! `RandomRanged(MinDebris, MaxDebris)` budget, the restored typed loop with
//! the warhead `DebrisMinimums`, and the warhead `DebrisAnims` fallback.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::projectile::{ProjectileDetonationReason, ProjectilePayload, ProjectileTarget};

const LISTS: &str = "[Animations]\n0=DBRI1\n1=DBRI2\n2=DBRI3\n\
    [VoxelAnims]\n1=TIRE\n\
    [TIRE]\nElasticity=0.8\nMinAngularVelocity=12.0\nMaxAngularVelocity=24.0\n\
    MinZVel=28.0\nMaxZVel=32.0\nMaxXYVel=10.0\nDuration=150\n";

fn rules(general: &str, warhead: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\n{general}MetallicDebris=DBRI1,DBRI2\n\
         [WeaponTypes]\n0=GUN\n[GUN]\nDamage=0\nWarhead=WH\n\
         [VehicleTypes]\n0=TANK\n[TANK]\nStrength=100\nPrimary=GUN\n\
         [Warheads]\n0=WH\n{LISTS}[WH]\n{warhead}"
    )))
    .expect("warhead debris fixture parses")
}

fn warhead_detonation(rules: &RuleSet) -> (Simulation, ProjectileDetonation) {
    let mut sim = Simulation::with_seed(7);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    // A fixture world collects the anim packets instead of admitting them to
    // the consequence queue (the same policy the receiver fixtures use).
    *sim.receiver_fixture = Some(crate::sim::combat::receiver_fixture::FixturePolicy {
        current_tick: 0,
        require_playfield_membership: false,
        fog_enabled: false,
        radiation_enabled: false,
        sound_enabled: false,
        terrain_collection: false,
        trace: Default::default(),
        deferred_tiberium: Vec::new(),
        tail_effects: Vec::new(),
        admitted_spawns: Vec::new(),
    });
    let payload = ProjectilePayload::new(0, sim.interner.intern("WH"), None, false);
    let detonation = ProjectileDetonation {
        projectile_id: 1,
        source_id: 0,
        target: ProjectileTarget::None,
        impact: ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
        payload,
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    (sim, detonation)
}

/// Set the impact cell's LandType. Water is index 2 in `[LandTypes]` order,
/// Beach index 6.
fn set_impact_land(sim: &mut Simulation, land: u8) {
    let identity = sim
        .resolved_terrain
        .as_ref()
        .expect("fixture terrain")
        .native_cell_identity((5, 5));
    let crate::map::cell_index::NativeCellIdentity::Real(index) = identity else {
        panic!("fixture impact cell is real");
    };
    sim.resolved_terrain
        .as_mut()
        .expect("fixture terrain")
        .cells_mut_for_test()[index]
        .land_type = land;
}

fn emit(
    sim: &mut Simulation,
    rules: &RuleSet,
    detonation: &ProjectileDetonation,
) -> (Vec<String>, usize) {
    let warhead = rules.warhead("WH").expect("fixture warhead").clone();
    let mut out = CombatEmit::default();
    emit_warhead_debris(sim, rules, detonation, &warhead, &mut out);
    let names = out
        .effects
        .explosion_effects
        .iter()
        .map(|effect| sim.interner.resolve(effect.shp_name).to_owned())
        .collect();
    (names, out.effects.voxel_debris.len())
}

/// Phobos's budget asks `RandomRanged(MinDebris, MaxDebris)` — the vanilla
/// `MaxDebris - 1` top is not used — and a resolved `Debris.Conventional`
/// skips the whole block over a Water cell only.
#[test]
fn the_bullet_budget_is_min_to_max_and_the_water_key_skips_it() {
    let dry = rules("", "MinDebris=2\nMaxDebris=2\n");
    let (mut sim, detonation) = warhead_detonation(&dry);
    let (names, _) = emit(&mut sim, &dry, &detonation);
    assert_eq!(
        names.len(),
        2,
        "the hook's top is MaxDebris, not MaxDebris - 1: {names:?}"
    );
    assert!(names.iter().all(|name| name.starts_with("DBRI")));
    // One index draw per metallic piece, and no other draw.
    let mut expected = SimRng::new(7);
    for _ in 0..2 {
        let _ = expected.next_range_i32_inclusive(0, 1);
    }
    assert_eq!(sim.scenario_rng.logical_view(), expected.logical_view());

    let keyed = rules("", "MinDebris=2\nMaxDebris=3\nDebris.Conventional=yes\n");
    let (mut sim, detonation) = warhead_detonation(&keyed);
    set_impact_land(&mut sim, 2);
    let (names, voxels) = emit(&mut sim, &keyed, &detonation);
    assert!(names.is_empty() && voxels == 0, "the water cell skips");
    assert_eq!(
        sim.scenario_rng.logical_view(),
        SimRng::new(7).logical_view(),
        "the skip takes no draw"
    );

    let global = rules("Debris.Conventional=yes\n", "MinDebris=2\nMaxDebris=3\n");
    let (mut sim, detonation) = warhead_detonation(&global);
    set_impact_land(&mut sim, 2);
    let (names, _) = emit(&mut sim, &global, &detonation);
    assert!(names.is_empty(), "the global fallback skips the water cell");

    let blocked = rules(
        "Debris.Conventional=yes\n",
        "MinDebris=2\nMaxDebris=3\nDebris.Conventional=no\n",
    );
    let (mut sim, detonation) = warhead_detonation(&blocked);
    set_impact_land(&mut sim, 2);
    let (names, _) = emit(&mut sim, &blocked, &detonation);
    assert!(
        !names.is_empty(),
        "an explicit no beats the global: {names:?}"
    );

    // A Beach cell is not the Water arm.
    let beach = rules("", "MinDebris=2\nMaxDebris=3\nDebris.Conventional=yes\n");
    let (mut sim, detonation) = warhead_detonation(&beach);
    set_impact_land(&mut sim, 6);
    let (names, _) = emit(&mut sim, &beach, &detonation);
    assert!(!names.is_empty(), "Beach is not Water");
}

/// Phobos's restored loop reads the warhead's own `DebrisMinimums`: a floor at
/// or above the budget throws the whole budget through its slot.
#[test]
fn the_warhead_debris_minimums_drive_the_typed_loop() {
    let floored = rules(
        "",
        "MinDebris=3\nMaxDebris=3\nDebrisTypes=TIRE\nDebrisMaximums=4\n\
         DebrisMinimums=3\nDebrisTypes.Limit=yes\n",
    );
    let (mut sim, detonation) = warhead_detonation(&floored);
    let (names, voxels) = emit(&mut sim, &floored, &detonation);
    assert_eq!(voxels, 3, "the minimum of 3 fills the pinned budget");
    assert!(names.is_empty(), "the typed loop never reaches an SHP arm");
    let mut expected = SimRng::new(7);
    let pick = expected.next_range_i32_inclusive(3, 4);
    assert_eq!(pick, 3, "the slot's floor");
    for _ in 0..3 {
        for _ in 0..4 {
            expected.next_u32();
        }
        for _ in 0..3 {
            expected.next_range_u32_inclusive(0, (0xFFFF - -0xFFFF) as u32);
        }
    }
    assert_eq!(sim.scenario_rng.logical_view(), expected.logical_view());
}

/// Phobos's bullet tail throws the warhead's own `DebrisAnims` list when the
/// typed count is zero, falling back to `[General] MetallicDebris=`.
#[test]
fn the_warhead_debris_anims_list_precedes_the_metallic_fallback() {
    let own = rules("", "MinDebris=1\nMaxDebris=1\nDebrisAnims=DBRI3\n");
    let (mut sim, detonation) = warhead_detonation(&own);
    let (names, voxels) = emit(&mut sim, &own, &detonation);
    assert_eq!(voxels, 0);
    assert_eq!(names, ["DBRI3"], "the warhead's own anim list wins");

    let metallic = rules("", "MinDebris=1\nMaxDebris=1\n");
    let (mut sim, detonation) = warhead_detonation(&metallic);
    let (names, _) = emit(&mut sim, &metallic, &detonation);
    assert_eq!(names.len(), 1);
    assert!(
        names[0] == "DBRI1" || names[0] == "DBRI2",
        "the metallic fallback: {names:?}"
    );
}
