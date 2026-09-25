//! Tests for the rocking system. Populated incrementally by Tasks 7–10.

#![cfg(test)]

use crate::sim::components::RockingState;
use crate::sim::rocking::impulse::apply_rocker_impulse;
use crate::sim::rocking::rocking_system::{
    IMPULSE_VEL_CAP, NORMAL_RANGE_PI2, SATURATION_PI4, SATURATION_PI10, TILT_DEADBAND,
    advance_axis, advance_ship_rocking,
};
use crate::util::fixed_math::SimFixed;

const DEFAULT_WEIGHT: SimFixed = SimFixed::lit("2.0");

const FALLBACK: SimFixed = SimFixed::lit("0.1");

#[test]
fn zero_velocity_force_zeros_angle() {
    let mut a = SimFixed::lit("0.5");
    let mut v = SimFixed::ZERO;
    advance_axis(&mut a, &mut v, SATURATION_PI4, false, FALLBACK);
    assert_eq!(a, SimFixed::ZERO);
}

#[test]
fn integrate_simple() {
    let mut a = SimFixed::lit("0.1");
    let mut v = SimFixed::lit("0.01");
    advance_axis(&mut a, &mut v, SATURATION_PI4, false, FALLBACK);
    // Integrate: a = 0.1 + 0.01 = 0.11; below cap, stationary + in-range so
    // saturation skipped. Dampen by BASE_DECAY_RATE: v = 0.01 - 0.002 = 0.008.
    let eps = SimFixed::lit("0.0001");
    assert!((a - SimFixed::lit("0.11")).abs() < eps);
    assert!((v - SimFixed::lit("0.008")).abs() < eps);
}

#[test]
fn saturation_clamps_when_stationary_and_crossing() {
    let mut a = SimFixed::lit("0.78"); // just below π/4 ≈ 0.7854
    let mut v = SimFixed::lit("0.05");
    advance_axis(&mut a, &mut v, SATURATION_PI4, false, FALLBACK);
    // new = 0.83; crosses π/4 from below; stationary + in_range → clamp.
    assert_eq!(a, SATURATION_PI4);
    assert_eq!(v, SimFixed::ZERO);
}

#[test]
fn saturation_does_not_clamp_when_moving() {
    let mut a = SimFixed::lit("0.78");
    let mut v = SimFixed::lit("0.05");
    advance_axis(
        &mut a,
        &mut v,
        SATURATION_PI4,
        true, /* moving */
        FALLBACK,
    );
    // Moving → saturation skipped; angle drifts past π/4.
    assert!(a > SATURATION_PI4);
}

#[test]
fn pi10_cap_is_supported_via_parameter() {
    // ±π/10 cap is the L8 forwards-during-crush path. The crush gate is
    // currently DEFERRED at the caller; this test pins that the math itself
    // still clamps correctly when a tighter cap is supplied. Re-enabling L8
    // later then becomes a one-line change at the caller.
    let mut a = SimFixed::lit("0.30"); // just below π/10 ≈ 0.3142
    let mut v = SimFixed::lit("0.05");
    advance_axis(&mut a, &mut v, SATURATION_PI10, false, FALLBACK);
    assert_eq!(a, SATURATION_PI10);
    assert_eq!(v, SimFixed::ZERO);
}

#[test]
fn deadband_snaps_to_zero() {
    // For the deadband branch to fire, the integrated angle must stay inside
    // TILT_DEADBAND after the integrate step. Start a=0 with the tiniest
    // possible nonzero velocity so v != 0 (L10 doesn't fire) but angle stays
    // <= deadband after integrate.
    let mut a = SimFixed::ZERO;
    let mut v = SimFixed::DELTA;
    advance_axis(&mut a, &mut v, SATURATION_PI4, false, FALLBACK);
    assert_eq!(a, SimFixed::ZERO);
    assert_eq!(v, SimFixed::ZERO);
}

#[test]
fn convergence_decays_to_zero_over_time() {
    let mut a = SimFixed::ZERO;
    // 0.01 is a clean multiple of BASE_DECAY_RATE in I16F16 fixed-point, so
    // velocity decays to exactly ZERO in 5 dampening ticks; the L10
    // short-circuit then forces angle to 0 on the next tick. Larger seed
    // values that aren't clean multiples leave a few DELTA units of residual
    // velocity that oscillates around 0 forever and takes hundreds of ticks
    // to drain through the integrate-step toward deadband.
    let mut v = SimFixed::lit("0.01");
    for _ in 0..100 {
        advance_axis(&mut a, &mut v, SATURATION_PI4, false, FALLBACK);
    }
    assert!(a.abs() <= TILT_DEADBAND);
    assert!(v.abs() <= TILT_DEADBAND);
}

#[test]
fn moving_dampens_more_slowly_than_stationary() {
    // FALLBACK = 0.1 → moving decay is 0.1 * BASE_DECAY_RATE = 0.0002/tick,
    // 10× slower than stationary's 0.002/tick. After 100 ticks a moving unit
    // should still be carrying noticeably more velocity than a stationary one.
    let mut a_moving = SimFixed::lit("0.1");
    let mut v_moving = SimFixed::lit("0.04");
    let mut a_still = SimFixed::lit("0.1");
    let mut v_still = SimFixed::lit("0.04");
    for _ in 0..50 {
        advance_axis(&mut a_moving, &mut v_moving, SATURATION_PI4, true, FALLBACK);
        advance_axis(&mut a_still, &mut v_still, SATURATION_PI4, false, FALLBACK);
    }
    assert!(
        v_moving.abs() > v_still.abs(),
        "moving v={:?} should retain more velocity than still v={:?}",
        v_moving,
        v_still
    );
}

#[test]
fn ship_rocking_integrates_without_damping() {
    let mut r = RockingState::default();
    r.vel_sideways = SimFixed::lit("0.01");
    advance_ship_rocking(&mut r, true);
    assert_eq!(r.angle_sideways, SimFixed::lit("0.01"));
    // Velocity is unchanged — no damping in this path.
    assert_eq!(r.vel_sideways, SimFixed::lit("0.01"));
}

#[test]
fn ship_rocking_clamps_upper_sideways() {
    let mut r = RockingState::default();
    r.angle_sideways = SATURATION_PI4;
    r.vel_sideways = SimFixed::lit("0.1");
    advance_ship_rocking(&mut r, true);
    assert_eq!(r.angle_sideways, SATURATION_PI4);
}

#[test]
fn ship_rocking_clamps_lower_both_axes() {
    let mut r = RockingState::default();
    r.angle_forwards = -SATURATION_PI4 + SimFixed::lit("0.001");
    r.vel_forwards = SimFixed::lit("-0.01");
    r.angle_sideways = -SATURATION_PI4 + SimFixed::lit("0.001");
    r.vel_sideways = SimFixed::lit("-0.01");
    advance_ship_rocking(&mut r, true);
    assert_eq!(r.angle_forwards, -SATURATION_PI4);
    assert_eq!(r.angle_sideways, -SATURATION_PI4);
}

#[test]
fn ship_rocking_forwards_upper_is_unclamped() {
    // Asymmetric: forwards upper is NOT clamped (only sideways upper).
    let mut r = RockingState::default();
    r.angle_forwards = SATURATION_PI4;
    r.vel_forwards = SimFixed::lit("0.1");
    advance_ship_rocking(&mut r, true);
    assert!(
        r.angle_forwards > SATURATION_PI4,
        "forwards should drift past +π/4 unclamped, got {:?}",
        r.angle_forwards,
    );
}

#[test]
fn ship_rocking_no_clamp_when_type_doesnt_support() {
    let mut r = RockingState::default();
    r.angle_sideways = SATURATION_PI4 + SimFixed::lit("0.5");
    r.vel_sideways = SimFixed::lit("0.01");
    advance_ship_rocking(&mut r, false);
    assert!(r.angle_sideways > SATURATION_PI4);
}

#[test]
fn impulse_caps_at_005_per_axis() {
    let mut r = RockingState::default();
    apply_rocker_impulse(
        &mut r,
        SimFixed::from_num(100),
        DEFAULT_WEIGHT,
        SimFixed::ONE,
        SimFixed::ZERO,
    );
    assert!(r.vel_sideways.abs() <= IMPULSE_VEL_CAP);
    assert!(r.vel_forwards.abs() <= IMPULSE_VEL_CAP);
}

#[test]
fn impulse_direction_from_x_axis_writes_sideways_only() {
    let mut r = RockingState::default();
    apply_rocker_impulse(
        &mut r,
        SimFixed::lit("4.0"),
        DEFAULT_WEIGHT,
        SimFixed::ONE,
        SimFixed::ZERO,
    );
    // Force=4, Weight=2 → force_scaled = 0.04 × 4 / 2 = 0.08 → clamps to 0.05.
    // nx = +1 → vel_side = -0.05; ny = 0 → vel_fwd = 0.
    assert!(r.vel_sideways < SimFixed::ZERO);
    assert_eq!(r.vel_forwards, SimFixed::ZERO);
}

#[test]
fn impulse_zero_distance_does_nothing() {
    let mut r = RockingState::default();
    apply_rocker_impulse(
        &mut r,
        SimFixed::ONE,
        DEFAULT_WEIGHT,
        SimFixed::ZERO,
        SimFixed::ZERO,
    );
    assert_eq!(r.vel_sideways, SimFixed::ZERO);
    assert_eq!(r.vel_forwards, SimFixed::ZERO);
}

#[test]
fn impulse_stacks_additively_until_cap() {
    let mut r = RockingState::default();
    // force=1.0, weight=2 → force_scaled = 0.04 × 1 / 2 = 0.02 (passes 0.01 floor).
    apply_rocker_impulse(
        &mut r,
        SimFixed::lit("1.0"),
        DEFAULT_WEIGHT,
        SimFixed::ONE,
        SimFixed::ZERO,
    );
    let v1 = r.vel_sideways;
    apply_rocker_impulse(
        &mut r,
        SimFixed::lit("1.0"),
        DEFAULT_WEIGHT,
        SimFixed::ONE,
        SimFixed::ZERO,
    );
    let v2 = r.vel_sideways;
    assert!(v2.abs() > v1.abs() || v2.abs() == IMPULSE_VEL_CAP);
}

#[test]
fn impulse_too_weak_dropped_by_floor_gate() {
    let mut r = RockingState::default();
    // force=0.1, weight=2 → force_scaled = 0.04 × 0.1 / 2 = 0.002 < 0.01 gate.
    apply_rocker_impulse(
        &mut r,
        SimFixed::lit("0.1"),
        DEFAULT_WEIGHT,
        SimFixed::ONE,
        SimFixed::ZERO,
    );
    assert_eq!(r.vel_sideways, SimFixed::ZERO);
    assert_eq!(r.vel_forwards, SimFixed::ZERO);
}

#[test]
fn impulse_heavier_unit_rocks_less_per_equal_force() {
    // L12c: Weight=5 (Aircraft Carrier) rocks 2.5× less than Weight=2 default.
    // Pick a force in the linear regime (below the 0.05 cap):
    //   light: 0.04 × 1.5 / 2.0 = 0.03
    //   heavy: 0.04 × 1.5 / 5.0 = 0.012
    //   ratio: 0.03 / 0.012 = 2.5
    let force = SimFixed::lit("1.5");
    let mut light = RockingState::default();
    apply_rocker_impulse(
        &mut light,
        force,
        SimFixed::lit("2.0"),
        SimFixed::ONE,
        SimFixed::ZERO,
    );
    let mut heavy = RockingState::default();
    apply_rocker_impulse(
        &mut heavy,
        force,
        SimFixed::lit("5.0"),
        SimFixed::ONE,
        SimFixed::ZERO,
    );
    let ratio = light.vel_sideways.abs() / heavy.vel_sideways.abs();
    assert!(
        ratio > SimFixed::lit("2.4") && ratio < SimFixed::lit("2.6"),
        "expected ~2.5× ratio (light/heavy), got {:?}",
        ratio
    );
}

#[test]
fn impulse_zero_weight_falls_back_to_default() {
    // Defensive: malformed INI section with Weight=0 must not panic or
    // produce inf — should behave identically to Weight=2.0.
    let mut r = RockingState::default();
    apply_rocker_impulse(
        &mut r,
        SimFixed::lit("4.0"),
        SimFixed::ZERO,
        SimFixed::ONE,
        SimFixed::ZERO,
    );
    let mut reference = RockingState::default();
    apply_rocker_impulse(
        &mut reference,
        SimFixed::lit("4.0"),
        DEFAULT_WEIGHT,
        SimFixed::ONE,
        SimFixed::ZERO,
    );
    assert_eq!(r.vel_sideways, reference.vel_sideways);
}

#[test]
fn impulse_forwards_axis_halved_relative_to_sideways() {
    // L12b asymmetry: pure +Y direction → vel_fwd = +ny × force_scaled × 0.5;
    // pure +X direction → vel_side = -nx × force_scaled (no halving). Same
    // force magnitude → forwards-axis impulse is exactly half the sideways one.
    let force = SimFixed::lit("4.0");
    let mut y_only = RockingState::default();
    apply_rocker_impulse(
        &mut y_only,
        force,
        DEFAULT_WEIGHT,
        SimFixed::ZERO,
        SimFixed::ONE,
    );
    let mut x_only = RockingState::default();
    apply_rocker_impulse(
        &mut x_only,
        force,
        DEFAULT_WEIGHT,
        SimFixed::ONE,
        SimFixed::ZERO,
    );
    // Sideways component (from +X impulse) is the full force-scaled
    // value (clamped at 0.05); forwards component (from +Y) is half.
    assert!(
        (x_only.vel_sideways.abs() - SimFixed::lit("0.05")).abs() < SimFixed::lit("0.001"),
        "x-only sideways should be ≈0.05, got {:?}",
        x_only.vel_sideways,
    );
    assert!(
        (y_only.vel_forwards.abs() - SimFixed::lit("0.025")).abs() < SimFixed::lit("0.001"),
        "y-only forwards should be ≈0.025 (halved), got {:?}",
        y_only.vel_forwards,
    );
}

#[test]
fn out_of_range_velocity_runs_away_not_inward() {
    // Out of normal range (|angle| > π/2), dampening sign flips so velocity
    // grows in its own direction — this is the runaway path the L30
    // self-destruct catches at ±π.
    let mut a = NORMAL_RANGE_PI2 + SimFixed::lit("0.1");
    let mut v = SimFixed::lit("0.01");
    let v_initial = v;
    advance_axis(&mut a, &mut v, SATURATION_PI4, false, FALLBACK);
    // Integrate pushed angle further past π/2; out-of-range branch adds
    // BASE_DECAY_RATE to a positive velocity → grows.
    assert!(
        v > v_initial,
        "out-of-range velocity should grow, was {:?} now {:?}",
        v_initial,
        v
    );
}

// ---- Integration tests (Task 12) -----------------------------------------
//
// End-to-end exercises of the full `Simulation::advance_tick` pipeline with
// one vehicle that has rocking enabled. Validates:
//   1. A finite impulse decays back to neutral over O(60) ticks.
//   2. Two seeded runs given identical impulses produce bit-identical
//      `state_hash()` outputs every tick (lockstep / replay invariant).
//
// The helper builds the minimum scaffolding `advance_tick` needs: a flat
// 10×10 terrain, a path grid, a `RuleSet` with one vehicle type, and the
// spawned MTNK with `rocking = Some(_)` set (production-side spawn doesn't
// yet initialize that field; see plan §"Open Questions" — vehicles will get
// it by default once combat lands).

use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;
use std::collections::BTreeMap;

const GRID_W: u16 = 10;
const GRID_H: u16 = 10;
const TICK_MS: u32 = 33;

fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    let mut cells = Vec::with_capacity((width as usize) * (height as usize));
    let speed_costs = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    for y in 0..height {
        for x in 0..width {
            cells.push(ResolvedTerrainCell {
                rx: x,
                ry: y,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: true,
                tileset_index: Some(0),
                land_type: 0,
                yr_cell_land_type: 0,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs,
                is_water: false,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: speed_costs,
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

fn minimal_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n",
    );
    RuleSet::from_ini(&ini).expect("minimal rules should parse")
}

/// Build a simulation with a single MTNK at (5, 5), flat terrain, and the
/// entity's `RockingState` initialized to default. Returns `(sim, rules,
/// path_grid)` so each test can drive `advance_tick` directly.
fn make_test_simulation_with_one_vehicle() -> (Simulation, RuleSet, PathGrid) {
    let rules = minimal_rules();
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -10,
        off_100: -1,
        off_104: 20,
        off_108: 11,
    });
    sim.resolved_terrain = Some(flat_terrain(GRID_W, GRID_H));
    let path_grid = PathGrid::new(GRID_W, GRID_H);

    let id = sim
        .spawn_object("MTNK", "Americans", 5, 5, 64, &rules, &BTreeMap::new())
        .expect("spawn MTNK");
    // Production-side spawn doesn't initialize `rocking` yet (sim-side only
    // path lands with combat in Task 19); flip it on here so the rocking
    // pipeline actually runs against this entity.
    sim.substrate
        .entities
        .get_mut(id)
        .expect("spawned entity present")
        .rocking = Some(RockingState::default());
    (sim, rules, path_grid)
}

fn advance(sim: &mut Simulation, rules: &RuleSet, path_grid: &PathGrid) {
    let _ = sim.advance_tick(
        &[],
        Some(rules),
        &BTreeMap::new(),
        Some(path_grid),
        None,
        TICK_MS,
    );
}

#[test]
fn integration_impulse_decays_to_neutral_over_60_ticks() {
    let (mut sim, rules, path_grid) = make_test_simulation_with_one_vehicle();

    // Apply a finite sideways impulse; default Weight=2.0 gives a scaled
    // velocity of 0.04 * 1.0 / 2.0 = 0.02 rad/tick — well within the
    // ±IMPULSE_VEL_CAP envelope, and large enough to actually settle visibly.
    let weight = SimFixed::lit("2.0");
    {
        let e = sim
            .substrate
            .entities
            .values_mut()
            .next()
            .expect("one entity");
        let r = e.rocking.as_mut().expect("rocking present");
        apply_rocker_impulse(r, SimFixed::ONE, weight, SimFixed::ONE, SimFixed::ZERO);
        assert!(
            r.vel_sideways.abs() > SimFixed::ZERO,
            "impulse should set nonzero sideways velocity, got {:?}",
            r.vel_sideways,
        );
    }

    // 60 ticks ≈ 1 second of sim time. Base decay 0.002 rad/tick against an
    // initial 0.02 rad/tick velocity zeroes out well before this horizon.
    for _ in 0..60 {
        advance(&mut sim, &rules, &path_grid);
    }

    let e = sim.substrate.entities.values().next().expect("one entity");
    let r = e.rocking.as_ref().expect("rocking present");
    assert!(
        r.is_neutral(),
        "rocking should have decayed to neutral after 60 ticks; \
         angles=({:?},{:?}) vels=({:?},{:?})",
        r.angle_sideways,
        r.angle_forwards,
        r.vel_sideways,
        r.vel_forwards,
    );
}

#[test]
fn integration_determinism_same_impulse_same_hash() {
    let (mut a, rules_a, grid_a) = make_test_simulation_with_one_vehicle();
    let (mut b, rules_b, grid_b) = make_test_simulation_with_one_vehicle();

    // Identical impulses on both sims, applied before any tick advances.
    let weight = SimFixed::lit("2.0");
    {
        let ea = a.substrate.entities.values_mut().next().unwrap();
        apply_rocker_impulse(
            ea.rocking.as_mut().unwrap(),
            SimFixed::lit("0.5"),
            weight,
            SimFixed::ONE,
            SimFixed::ZERO,
        );
        let eb = b.substrate.entities.values_mut().next().unwrap();
        apply_rocker_impulse(
            eb.rocking.as_mut().unwrap(),
            SimFixed::lit("0.5"),
            weight,
            SimFixed::ONE,
            SimFixed::ZERO,
        );
    }

    // Hashes must agree at every tick boundary, not just at the end. Catching
    // the divergence tick is what makes this useful for replay debugging.
    assert_eq!(a.state_hash(), b.state_hash(), "pre-tick hashes diverge");
    for tick in 0..200 {
        advance(&mut a, &rules_a, &grid_a);
        advance(&mut b, &rules_b, &grid_b);
        assert_eq!(
            a.state_hash(),
            b.state_hash(),
            "state hash diverged at tick {}",
            tick + 1,
        );
    }
}

/// `RockingUpdate`'s crashing branch against the executable: 40-frame spins of
/// both signs, a `BalloonHover=` clamp and a start from nonzero angles
/// (`tools/spatial_oracle/aircraft_crash.json`, `rocking`). Native keeps f32;
/// each SimFixed step may differ by one quantum, so the tolerance grows with
/// the frame count.
#[test]
fn crash_spin_matches_native_rocking_update() {
    use crate::sim::rocking::rocking_system::advance_crash_spin;
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_crash.json"
    ))
    .unwrap();
    let rows = oracle["rocking"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    let native = |hex: &serde_json::Value| {
        f64::from(f32::from_bits(
            u32::from_str_radix(hex.as_str().unwrap(), 16).unwrap(),
        ))
    };
    for row in rows {
        let input = &row["input"];
        let rates = input["rates"].as_array().unwrap();
        let angles = input["angles"].as_array();
        // The fixture writes the rates and angles as f32; start from those
        // exact values.
        let f32_of = |v: &serde_json::Value| f64::from(v.as_f64().unwrap() as f32);
        let mut state = RockingState {
            vel_sideways: SimFixed::from_num(f32_of(&rates[0])),
            vel_forwards: SimFixed::from_num(f32_of(&rates[1])),
            angle_sideways: angles.map_or(SimFixed::ZERO, |a| SimFixed::from_num(f32_of(&a[0]))),
            angle_forwards: angles.map_or(SimFixed::ZERO, |a| SimFixed::from_num(f32_of(&a[1]))),
            is_ship_rocking: false,
        };
        let balloon = input["balloon_hover"].as_i64() == Some(1);
        for (frame, expected) in row["history"].as_array().unwrap().iter().enumerate() {
            advance_crash_spin(&mut state, balloon);
            let tolerance = (frame as f64 + 3.0) / 65536.0;
            for (actual, index) in [(state.angle_sideways, 0), (state.angle_forwards, 1)] {
                let expected = native(&expected[index]);
                assert!(
                    (actual.to_num::<f64>() - expected).abs() <= tolerance,
                    "{} frame {frame} axis {index}: {actual} vs {expected}",
                    input["name"]
                );
            }
        }
    }
}
