//! DriveLocomotion runtime helpers.
//!
//! This module owns Drive-specific state updates that should not leak into the
//! generic `MovementTarget` path. Detailed DriveTrack consumption remains in
//! `drive_track`; this file handles the Drive-local speed fraction scaffold.

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{LocomotorKind, SpeedType};
#[cfg(test)]
use crate::sim::components::DriveCoord;
use crate::sim::components::{
    DriveLocomotionRuntime, FootSpeedState, NavTargetRef, ShipLocomotionRuntime,
};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::pathfinding::terrain_speed::{self, TerrainSpeedConfig};
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed};

const DRIVE_DESTINATION_BRAKE_FLOOR: SimFixed = SimFixed::lit("0.3");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DriveProcessOutcome {
    NotDrive,
    Processed,
}

pub(crate) fn process_drive_locomotion_shell(entity: &GameEntity) -> DriveProcessOutcome {
    if entity.drive_locomotion.is_none() {
        return DriveProcessOutcome::NotDrive;
    }
    DriveProcessOutcome::Processed
}

pub(super) fn drive_requires_native_step(
    drive: &DriveLocomotionRuntime,
    path_replay: &crate::sim::components::FootPathQueue,
) -> bool {
    !path_replay.remaining_directions().is_empty() || drive.track.residual != 0
}

/// `ILocomotion::Is_Moving` (slot 4) for the Drive locomotor.
///
/// gamemd reads the locomotor's OWN coordinates, not the owner's path queue: a
/// non-null destination is moving; otherwise a null head-to is not moving; a
/// head-to whose X and Y already equal the owner's exact lepton position is not
/// moving; anything else is. Z is deliberately not compared.
///
/// This is the predicate `Drive::Is_Ok_To_End` consults before a piggyback may
/// be unwound. It is a different function from `Is_Moving_Now` (slot 32), which
/// additionally folds in hull rotation and the live per-frame speed — a Drive
/// unit with a destination but zero speed is `Is_Moving`, not `Is_Moving_Now`.
pub(crate) fn drive_locomotor_is_moving(entity: &GameEntity) -> bool {
    let Some(drive) = entity.drive_locomotion.as_ref() else {
        return false;
    };
    if drive.destination.is_some() {
        return true;
    }
    let Some(head) = drive.head_to else {
        return false;
    };
    let owner_x = i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>();
    let owner_y = i32::from(entity.position.ry) * 256 + entity.position.sub_y.to_num::<i32>();
    head.x != owner_x || head.y != owner_y
}

/// Unit -> Infantry NavCom refreshes among `candidates`, visited in ascending
/// stable-id order. Callers pass the objects of the current pass rather than
/// the whole store; the answer for any object does not depend on the others.
pub(super) fn drive_entity_nav_targets(
    entities: &EntityStore,
    candidates: &[u64],
) -> Vec<(u64, NavTargetRef)> {
    let mut ids: Vec<u64> = candidates.to_vec();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| {
            let entity = entities.get(id)?;
            let target = entity.navigation.nav_com?;
            let NavTargetRef::Entity { id: target_id } = target else {
                return None;
            };
            // Native4B05D0: only Unit -> Infantry, on the active Drive instance.
            (entity.category == crate::map::entities::EntityCategory::Unit
                && entity
                    .locomotor
                    .as_ref()
                    .is_some_and(|l| l.kind == LocomotorKind::Drive)
                && entities.get(target_id).is_some_and(|target| {
                    target.category == crate::map::entities::EntityCategory::Infantry
                }))
            .then_some((id, target))
        })
        .collect()
}

/// Compute the Drive-local target speed fraction from currently modeled runtime
/// modifiers. This is the `DriveLocomotion` owner value; raw `Speed=` remains a
/// separate top-speed input.
///
/// `below_condition_yellow` carries the owner's damaged state into the fraction:
/// gamemd reads the health ratio inside `Process_Movement` itself and multiplies
/// the fraction by 0.75 when it is at or below `[AudioVisual] ConditionYellow`.
#[allow(clippy::too_many_arguments)]
pub(super) fn compute_drive_target_speed_fraction(
    speed_type: SpeedType,
    locomotor_kind: LocomotorKind,
    mover_world: (i32, i32),
    next_cell: (u16, u16),
    terrain: &ResolvedTerrainGrid,
    config: &TerrainSpeedConfig,
    below_condition_yellow: bool,
) -> SimFixed {
    terrain_speed::compute_cell_speed_modifier(
        speed_type,
        locomotor_kind,
        mover_world,
        next_cell,
        terrain,
        config,
        below_condition_yellow,
    )
}

/// Update Drive target/current speed fractions before budget consumption.
///
/// Gamemd keeps the target fraction on DriveLocomotion and the applied/current
/// fraction on the owner through `SetSpeedFraction`4D3710. A controller swap
/// must leave that live owner value available to the retained invocation.
pub(super) fn update_drive_speed_fraction(
    drive: &mut DriveLocomotionRuntime,
    owner_speed: &mut FootSpeedState,
    target_fraction: SimFixed,
    accelerates: bool,
    raw_speed_per_frame: SimFixed,
    accel_factor: SimFixed,
    decel_factor: SimFixed,
    slowdown_distance: SimFixed,
    distance_to_goal: SimFixed,
) {
    update_vehicle_speed_fraction(
        &mut drive.target_speed_fraction,
        &mut owner_speed.applied_fraction,
        target_fraction,
        accelerates,
        raw_speed_per_frame,
        accel_factor,
        decel_factor,
        slowdown_distance,
        distance_to_goal,
    );
}

/// Update Ship's class-owned target fraction and owner-applied fraction.
///
/// The active Ship `Process_Drive_Track` body uses these same transitions
/// before calling the owner's `SetSpeedFraction` slot. The target belongs to
/// Ship; the applied value belongs to the live Foot owner.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_ship_speed_fraction(
    ship: &mut ShipLocomotionRuntime,
    owner_speed: &mut FootSpeedState,
    target_fraction: SimFixed,
    accelerates: bool,
    raw_speed_per_frame: SimFixed,
    accel_factor: SimFixed,
    decel_factor: SimFixed,
    slowdown_distance: SimFixed,
    distance_to_goal: SimFixed,
) {
    update_vehicle_speed_fraction(
        &mut ship.target_speed_fraction,
        &mut owner_speed.applied_fraction,
        target_fraction,
        accelerates,
        raw_speed_per_frame,
        accel_factor,
        decel_factor,
        slowdown_distance,
        distance_to_goal,
    );
}

/// Ship normally recomputes its requested fraction in `Process_Movement`.
/// `Stop_Moving` is the active exception: after it clears destination while a
/// committed head remains, `Process_Drive_Track` consumes the class-owned
/// clamped target without running a fresh terrain request over it.
pub(super) fn ship_process_target_speed_fraction(
    ship: &ShipLocomotionRuntime,
    movement_target_fraction: SimFixed,
) -> SimFixed {
    if ship.destination.is_none() && ship.head_to.is_some() {
        ship.target_speed_fraction
    } else {
        movement_target_fraction
    }
}

/// The `Accelerates=` ramp, `DriveLocomotionClass::Process_Drive_Track` @
/// `0x004B0F20`. Three of its arms are **not** modelled, recorded here:
///
/// - **The second brake band.** Outside `SlowdownDistance`, when `owner+0x3CD`
///   is set, native brakes by `rawSpeed × 0.0015` with a floor of `0.1`
///   (`0x004B10FF`) — much gentler, to a much lower floor, than the arrival
///   band. `+0x3CD` is written by `UnitClass::ReceiveDamage` `0x00737E51`,
///   `TemporalClass::AI` `0x00629C69`, the teleport post-warp validation and
///   the jumpjet touchdown, and `Process` @ `0x004B0500` also zeroes the target
///   speed on it — the two writes there are `[recv+0x4C]` and `[recv+0x50]` on
///   the **ILocomotion** receiver, i.e. complete-object `+0x50`/`+0x54`, the two
///   halves of one `double`. The integer movement residual at complete `+0x4C`
///   is untouched. So it is load-bearing in two places. Its identity is
///   UNCHECKED, which is why this carries no frequency clause yet: that is the
///   gap to close before ranking it.
/// - **The crush clamp.** While `owner+0x6B5` is set (raised at `0x004B1A2F`
///   when the mover drives over a crushable, cleared in
///   `UnitClass::PerCellProcess`), native replaces the whole ramp with
///   `min(target, 0.2)` and writes it back to `drive+0x50` (`0x004B1146`).
///   Trigger: a crusher mid-crush. Player effect: retail slows to a fifth speed
///   over the victim. Frequency: **common, not rare.** It lives inside the
///   `Accelerates` branch, and of the 29 `Crusher=yes` types in stock
///   `rulesmd.ini` only 14 carry `Accelerates=false` — the 15 that accelerate
///   include `[APOC]`, both MCVs, `[V3]`, every ore miner and the amphibious
///   transports. An Apocalypse driving over infantry is ordinary Soviet play.
///   Downstream risk: it writes the locomotor-owned slot, not just the owner's.
/// - **The `drive+0x58 >= 0x40` bypass.** Both `Process_Movement` @
///   `0x004B2630` and this function gate on `*(int*)(drive+0x58) < 0x40`; above
///   it native skips the `drive+0x50` write *and* the whole ramp, setting the
///   owner fraction directly. VERA always writes and always ramps. Trigger:
///   a live `Force_Track` curve. `Force_Track` @ `0x004B0C40` is the only writer
///   that can push the selector past 0x3F — both ordinary writers cap at 63 —
///   and the three call sites that pass one are the **Yuri Tank Bunker**
///   occupant lifecycle: the installer at `0x00458E50` (`0x43`..`0x46`, chosen
///   by facing at `0x00459132`-`0x0045915C`), `BuildingClass::UndockUnit` @
///   `0x004593A0` (`0x47`, pushed at `0x0045942C`) and
///   `BuildingClass::ReleaseDockedHarvester` @ `0x004595C0` (`0x47`, pushed at
///   `0x00459751`). The latter two early-return unless the building's `+0x2E4`
///   dock link is set, and the installer is its only setter, so the whole family
///   needs a garrisoned bunker. Stock `rulesmd.ini` has exactly one
///   `Bunker=yes`: `[NATBNK]`.
///
///   So the gate means: while on a bunker install or eject curve, skip the ramp
///   and drive at the 1.0 `Force_Track` installed. Player effect: VERA ramps
///   where native jumps straight to full speed. Frequency: **zero in any match
///   without a garrisoned Tank Bunker**, occasional in Yuri matchups — not the
///   factory door. Downstream risk: `ForcedDriveTrackState` already carries a
///   full-speed constant, so the forced arm is approximated; what is missing is
///   the same `< 0x40` gate inside `Process_Movement` @ `0x004B2630` (gates read
///   at `0x004B0FA8` and `0x004B3DFA`).
///
///   `Force_Track` has three further callers, all passing `-1`:
///   `TechnoClass::PerformDeploy` @ `0x007101B3`, `SuperClass::Launch` @
///   `0x006CCAA2`, and `0x0062AB24`.
/// - **The `Passive=` skip.** `UnitTypeClass+0xE0C` (stored at `0x0074783D`)
///   disables the entire ramp for a UnitClass mover. Trigger: a `Passive=yes`
///   type. Player effect: none observed. Frequency: zero in skirmish — stock
///   `Passive=yes` is civilian traffic. Downstream risk: one `if`.
#[allow(clippy::too_many_arguments)]
fn update_vehicle_speed_fraction(
    target_slot: &mut SimFixed,
    current_slot: &mut SimFixed,
    target_fraction: SimFixed,
    accelerates: bool,
    raw_speed_per_frame: SimFixed,
    accel_factor: SimFixed,
    decel_factor: SimFixed,
    slowdown_distance: SimFixed,
    distance_to_goal: SimFixed,
) {
    // The locomotor-owned target fraction is **not** clamped in gamemd.
    // `Process_Movement` @ `0x004B2630` writes `drive+0x50` raw, so a healthy
    // tracked mover going downhill on a 100% land row legitimately carries 1.2
    // — the terrain chain's own tests assert the combined value exceeds 1.0.
    // The only native clamp is inside `TechnoClass::SetSpeedFraction` @
    // `0x004D3710`, on the owner's `+0x578`.
    //
    // That clamp is still reached: **every** arm of `Process_Drive_Track` @
    // `0x004B0F20` terminates in `SetSpeedFraction` through vtable `+0x544`, so
    // gamemd discards the above-1.0 portion one step later and the mover does
    // not actually go faster downhill. Keeping this slot unclamped matches
    // where the native clamp lives, and matters only to anything that reads the
    // *target* fraction rather than the owner's; it is not a speed change.
    *target_slot = target_fraction;
    if !accelerates {
        *current_slot = target_fraction.clamp(SIM_ZERO, SIM_ONE);
        return;
    }

    let target = *target_slot;
    let mut current = *current_slot;
    if slowdown_distance > SIM_ZERO && distance_to_goal < slowdown_distance {
        current -= raw_speed_per_frame * decel_factor;
        if current < DRIVE_DESTINATION_BRAKE_FLOOR {
            current = DRIVE_DESTINATION_BRAKE_FLOOR;
        }
    } else if current < target {
        current += accel_factor;
        if current > target {
            current = target;
        }
    } else if target < current {
        current -= raw_speed_per_frame * decel_factor;
        if current < target {
            current = target;
        }
    }
    *current_slot = current.clamp(SIM_ZERO, SIM_ONE);
}

/// Reproduce the positive/zero value returned by owner slot `+0x538` for the
/// active stock vehicle speed path.
///
/// `FootClass::GetCurrentSpeed` first truncates the type/owner-adjusted raw
/// speed, then multiplies by owner `+0x578` and truncates again. Rust keeps the
/// adjusted speed in leptons/second, so dividing by the 15-Hz native baseline
/// recovers the first integer before applying the Foot-owned fraction.
///
/// Two native terms are **not** modelled, recorded rather than guessed:
/// - the elite multiply, `HasWeaponAbility(0)` -> `x Rules+0x678`, which sits
///   *between* the two truncations (`0x004DB1E8`-`0x004DB205`). Trigger: an
///   elite unit. Player effect: elites move at their veteran speed instead of
///   their elite one. Frequency: every elite vehicle, which an active player
///   accumulates over a long match. Downstream risk: none - one factor in the
///   middle of a chain VERA already reproduces exactly.
/// - the halving at `0x004DB226`: RTTI 1 (UnitClass) with `owner+0x6CC != -1`
///   -> `speed / 2` via `CDQ/SUB/SAR 1`. This is the active-YR CTF flag owner
///   index, normally inactive in stock multiplayer; it is not a TS-only gate.
///   See FOOTCLASS_GET_CURRENT_SPEED_EXACT_GHIDRA_REPORT.md for the retained
///   speed-crate input and staged getter proof needed by the live Process host.
pub(crate) fn owner_current_speed_from_fraction(
    adjusted_speed_per_second: SimFixed,
    current_speed_fraction: SimFixed,
) -> i32 {
    let adjusted_type_speed = (adjusted_speed_per_second / SimFixed::from_num(15)).to_num::<i32>();
    (SimFixed::from_num(adjusted_type_speed) * current_speed_fraction).to_num::<i32>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::util::fixed_math::{SIM_HALF, SIM_ONE, SIM_ZERO};

    #[test]
    fn gsi_13_06_ship_speed_fraction_uses_locomotor_owned_state() {
        let mut owner_speed = FootSpeedState::default();
        let mut ship = ShipLocomotionRuntime::default();

        update_ship_speed_fraction(
            &mut ship,
            &mut owner_speed,
            SIM_HALF,
            false,
            SimFixed::from_num(10),
            SimFixed::lit("0.03"),
            SimFixed::lit("0.002"),
            SimFixed::from_num(500),
            SimFixed::from_num(1000),
        );
        assert_eq!(ship.target_speed_fraction, SIM_HALF);
        assert_eq!(owner_speed.applied_fraction, SIM_HALF);

        owner_speed.applied_fraction = SIM_ZERO;
        update_ship_speed_fraction(
            &mut ship,
            &mut owner_speed,
            SIM_ONE,
            true,
            SimFixed::from_num(10),
            SimFixed::lit("0.03"),
            SimFixed::lit("0.002"),
            SimFixed::from_num(500),
            SimFixed::from_num(1000),
        );
        assert_eq!(ship.target_speed_fraction, SIM_ONE);
        assert_eq!(owner_speed.applied_fraction, SimFixed::lit("0.03"));
    }

    #[test]
    fn gsi_13_06_stock_shp_current_speed_preserves_native_truncation() {
        use crate::util::fixed_math::ra2_speed_to_leptons_per_second;

        let stock_ship_speed = ra2_speed_to_leptons_per_second(8);
        assert_eq!(
            owner_current_speed_from_fraction(stock_ship_speed, SimFixed::lit("0.03")),
            0,
            "DLPH/SQD first acceleration step truncates to zero"
        );
        assert_eq!(
            owner_current_speed_from_fraction(stock_ship_speed, SimFixed::lit("0.06")),
            1
        );
        let stock_dron_speed = ra2_speed_to_leptons_per_second(10);
        assert_eq!(
            owner_current_speed_from_fraction(stock_dron_speed, SimFixed::lit("0.03")),
            0,
            "DRON's raw stage 25 still truncates 0.75 to zero"
        );
        assert_eq!(
            owner_current_speed_from_fraction(stock_dron_speed, SIM_ONE),
            25,
            "Accelerates=false DRON uses its full converted type speed"
        );
    }

    #[test]
    fn gsi_13_06_nonterminal_ship_post_stop_process_preserves_clamped_target() {
        use crate::util::fixed_math::ra2_speed_to_leptons_per_second;

        let speed = ra2_speed_to_leptons_per_second(8);
        let mut owner_speed = FootSpeedState {
            applied_fraction: SIM_HALF,
            cached_current_speed: 10,
        };
        let mut ship = ShipLocomotionRuntime {
            destination: None,
            head_to: Some(DriveCoord::cell(4, 3, 0)),
            target_speed_fraction: SimFixed::lit("0.3"),
            ..Default::default()
        };

        for _ in 0..10 {
            let requested = ship_process_target_speed_fraction(&ship, SIM_ONE);
            assert_eq!(requested, SimFixed::lit("0.3"));
            update_ship_speed_fraction(
                &mut ship,
                &mut owner_speed,
                requested,
                true,
                speed / SimFixed::from_num(15),
                SimFixed::lit("0.03"),
                SimFixed::lit("0.002"),
                SIM_ZERO,
                SimFixed::from_num(256),
            );
            owner_speed.cached_current_speed =
                owner_current_speed_from_fraction(speed, owner_speed.applied_fraction);
            assert_eq!(ship.target_speed_fraction, SimFixed::lit("0.3"));
            assert!(owner_speed.cached_current_speed > 0);
        }

        assert_eq!(ship.destination, None);
        assert_eq!(ship.head_to, Some(DriveCoord::cell(4, 3, 0)));
        assert_eq!(owner_speed.applied_fraction, SimFixed::lit("0.3"));
        assert_eq!(owner_speed.cached_current_speed, 6);
    }

    fn terrain_cell(rx: u16, ry: u16, speed_costs: SpeedCostProfile) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
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
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            height_in_pixels: 0,
            variant: 0,
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
            base_speed_costs: Default::default(),
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
        }
    }

    #[test]
    fn drive_target_speed_fraction_uses_terrain_modifier() {
        let terrain = ResolvedTerrainGrid::from_cells(
            2,
            1,
            vec![
                terrain_cell(0, 0, SpeedCostProfile::default()),
                terrain_cell(
                    1,
                    0,
                    SpeedCostProfile {
                        track: Some(50),
                        ..Default::default()
                    },
                ),
            ],
        );

        let fraction = compute_drive_target_speed_fraction(
            SpeedType::Track,
            LocomotorKind::Drive,
            (0, 0),
            (1, 0),
            &terrain,
            &TerrainSpeedConfig::default(),
            false,
        );

        assert_eq!(fraction, SIM_HALF);
    }

    /// Two flat `[Clear]` cells, `Track=100%`, so the terrain × slope product is
    /// exactly 1.0 and the only thing under test is the damaged-mover factor.
    fn flat_clear_pair() -> ResolvedTerrainGrid {
        let clear = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            ..Default::default()
        };
        ResolvedTerrainGrid::from_cells(
            2,
            1,
            vec![terrain_cell(0, 0, clear), terrain_cell(1, 0, clear)],
        )
    }

    fn fraction_for(kind: LocomotorKind, damaged: bool) -> SimFixed {
        compute_drive_target_speed_fraction(
            SpeedType::Track,
            kind,
            (0, 0),
            (1, 0),
            &flat_clear_pair(),
            &TerrainSpeedConfig::default(),
            damaged,
        )
    }

    /// GSI-06.04 G1: gamemd multiplies the per-cell speed fraction by 0.75 when
    /// the owner's health ratio is at or below `[AudioVisual] ConditionYellow`.
    /// A healthy mover on the same cells keeps the full fraction.
    #[test]
    fn gsi_06_04_damaged_drive_mover_slows_to_three_quarters() {
        assert_eq!(fraction_for(LocomotorKind::Drive, false), SIM_ONE);
        assert_eq!(
            fraction_for(LocomotorKind::Drive, true),
            SimFixed::lit("0.75")
        );
    }

    /// The same factor is read by Ship `Process_Movement`; a damaged destroyer
    /// slows exactly like a damaged tank.
    #[test]
    fn gsi_06_04_damaged_ship_mover_slows_to_three_quarters() {
        assert_eq!(fraction_for(LocomotorKind::Ship, false), SIM_ONE);
        assert_eq!(
            fraction_for(LocomotorKind::Ship, true),
            SimFixed::lit("0.75")
        );
    }

    /// The damaged factor is NOT read by any other locomotor — a wounded GI, a
    /// wounded Robot Tank and a wounded Rocketeer all keep full speed. Guards
    /// against over-applying the factor once it exists.
    #[test]
    fn gsi_06_04_damaged_non_drive_movers_keep_full_speed() {
        for kind in [
            LocomotorKind::Walk,
            LocomotorKind::Hover,
            LocomotorKind::Jumpjet,
            LocomotorKind::Mech,
            LocomotorKind::Teleport,
            LocomotorKind::Tunnel,
            LocomotorKind::Fly,
        ] {
            assert_eq!(fraction_for(kind, true), SIM_ONE, "damaged {kind:?}");
            assert_eq!(fraction_for(kind, false), SIM_ONE, "healthy {kind:?}");
        }
    }

    /// GSI-06.04 G2: gamemd routes the land-type table through Drive and Ship
    /// only. A hover mover standing on `[Clear] Hover=50%` therefore runs at full
    /// speed on land — its locomotor is a pure throttle that never reads the
    /// table — while a Drive mover on the same cell is halved.
    #[test]
    fn gsi_06_04_land_type_table_reaches_drive_and_ship_only() {
        let terrain = ResolvedTerrainGrid::from_cells(
            2,
            1,
            vec![
                terrain_cell(0, 0, SpeedCostProfile::default()),
                terrain_cell(
                    1,
                    0,
                    SpeedCostProfile {
                        hover: Some(50),
                        track: Some(50),
                        ..Default::default()
                    },
                ),
            ],
        );
        let sample = |kind: LocomotorKind, st: SpeedType| {
            compute_drive_target_speed_fraction(
                st,
                kind,
                (0, 0),
                (1, 0),
                &terrain,
                &TerrainSpeedConfig::default(),
                false,
            )
        };
        assert_eq!(sample(LocomotorKind::Drive, SpeedType::Track), SIM_HALF);
        assert_eq!(sample(LocomotorKind::Ship, SpeedType::Track), SIM_HALF);
        assert_eq!(sample(LocomotorKind::Hover, SpeedType::Hover), SIM_ONE);
        assert_eq!(sample(LocomotorKind::Walk, SpeedType::Foot), SIM_ONE);
    }

    /// GSI-06.04 G2 (slope half): the four `[General]` slope coefficients are
    /// applied at the same two sites as the table, so infantry get no downhill
    /// boost on a ramp.
    #[test]
    fn gsi_06_04_slope_coefficients_reach_drive_and_ship_only() {
        let flat = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            ..Default::default()
        };
        let mut high = terrain_cell(0, 0, flat);
        high.level = 2;
        let terrain = ResolvedTerrainGrid::from_cells(2, 1, vec![high, terrain_cell(1, 0, flat)]);
        let sample = |kind: LocomotorKind, st: SpeedType| {
            compute_drive_target_speed_fraction(
                st,
                kind,
                (0, 0),
                (1, 0),
                &terrain,
                &TerrainSpeedConfig::default(),
                false,
            )
        };
        // Downhill: the Drive mover takes the 1.2x coefficient...
        assert_eq!(
            sample(LocomotorKind::Drive, SpeedType::Track),
            SimFixed::lit("1.2")
        );
        // ...and the walking infantryman does not.
        assert_eq!(sample(LocomotorKind::Walk, SpeedType::Foot), SIM_ONE);
    }

    fn drive_entity_at(rx: u16, ry: u16) -> GameEntity {
        let mut entity = GameEntity::test_default(1, "HARV", "Americans", rx, ry);
        entity.drive_locomotion = Some(DriveLocomotionRuntime::default());
        entity
    }

    /// GSI-06.12 GAP 3: `Drive::Is_Ok_To_End` reads ILocomotion slot 4
    /// (`Is_Moving`) on the ACTIVE locomotor, and that predicate looks at the
    /// Drive locomotor's own destination and head-to coords — not at the
    /// owner's path queue. A non-null destination alone means "moving".
    #[test]
    fn gsi_06_12_drive_is_moving_reads_its_own_destination() {
        let mut entity = drive_entity_at(3, 3);
        assert!(!drive_locomotor_is_moving(&entity));

        entity.drive_locomotion.as_mut().expect("drive").destination =
            Some(DriveCoord::cell(9, 3, 0));
        assert!(drive_locomotor_is_moving(&entity));
    }

    /// With no destination, a null head-to is not moving and a head-to that
    /// already equals the owner's exact lepton X/Y is not moving either. Z is
    /// deliberately not part of the comparison.
    #[test]
    fn gsi_06_12_drive_is_moving_compares_head_to_against_the_owner_position() {
        let mut entity = drive_entity_at(3, 3);
        entity.position.sub_x = SimFixed::from_num(128);
        entity.position.sub_y = SimFixed::from_num(128);

        let drive = entity.drive_locomotion.as_mut().expect("drive");
        drive.head_to = Some(DriveCoord::cell(3, 3, 0));
        assert!(
            !drive_locomotor_is_moving(&entity),
            "head-to at the owner's own cell centre is not moving"
        );

        let drive = entity.drive_locomotion.as_mut().expect("drive");
        drive.head_to = Some(DriveCoord::cell(3, 3, 7));
        assert!(
            !drive_locomotor_is_moving(&entity),
            "a Z-only difference does not make it moving"
        );

        let drive = entity.drive_locomotion.as_mut().expect("drive");
        drive.head_to = Some(DriveCoord::cell(4, 3, 0));
        assert!(drive_locomotor_is_moving(&entity));
    }

    #[test]
    fn a_downhill_target_stays_above_one_but_the_owner_fraction_does_not() {
        // `Process_Movement` @ 0x004B2630 writes `drive+0x50` unclamped, so a
        // 1.2 downhill product survives on the locomotor-owned slot; the only
        // native clamp is `TechnoClass::SetSpeedFraction` @ 0x004D3710, which
        // every arm of `Process_Drive_Track` reaches, so the owner's fraction
        // never exceeds 1.
        let mut owner_speed = FootSpeedState {
            applied_fraction: SIM_ZERO,
            ..Default::default()
        };
        let mut drive = DriveLocomotionRuntime::default();

        update_drive_speed_fraction(
            &mut drive,
            &mut owner_speed,
            SimFixed::lit("1.2"),
            false,
            SimFixed::from_num(10),
            SimFixed::lit("0.03"),
            SimFixed::lit("0.002"),
            SimFixed::from_num(500),
            SimFixed::from_num(1000),
        );

        assert_eq!(drive.target_speed_fraction, SimFixed::lit("1.2"));
        assert_eq!(owner_speed.applied_fraction, SIM_ONE);
    }

    #[test]
    fn accelerates_false_assigns_current_fraction_directly() {
        let mut owner_speed = FootSpeedState {
            applied_fraction: SIM_ZERO,
            ..Default::default()
        };
        let mut drive = DriveLocomotionRuntime::default();

        update_drive_speed_fraction(
            &mut drive,
            &mut owner_speed,
            SIM_HALF,
            false,
            SimFixed::from_num(10),
            SimFixed::lit("0.03"),
            SimFixed::lit("0.002"),
            SimFixed::from_num(500),
            SimFixed::from_num(1000),
        );

        assert_eq!(drive.target_speed_fraction, SIM_HALF);
        assert_eq!(owner_speed.applied_fraction, SIM_HALF);
    }

    #[test]
    fn accelerates_true_ramps_current_fraction_upward() {
        let mut owner_speed = FootSpeedState {
            applied_fraction: SIM_ZERO,
            ..Default::default()
        };
        let mut drive = DriveLocomotionRuntime::default();

        update_drive_speed_fraction(
            &mut drive,
            &mut owner_speed,
            SIM_ONE,
            true,
            SimFixed::from_num(10),
            SimFixed::lit("0.03"),
            SimFixed::lit("0.002"),
            SimFixed::from_num(500),
            SimFixed::from_num(1000),
        );

        assert_eq!(drive.target_speed_fraction, SIM_ONE);
        assert_eq!(owner_speed.applied_fraction, SimFixed::lit("0.03"));
    }

    #[test]
    fn accelerates_true_brakes_by_raw_speed_scaled_decel_with_floor() {
        let mut owner_speed = FootSpeedState {
            applied_fraction: SIM_HALF,
            ..Default::default()
        };
        let mut drive = DriveLocomotionRuntime::default();

        update_drive_speed_fraction(
            &mut drive,
            &mut owner_speed,
            SIM_ONE,
            true,
            SimFixed::from_num(10),
            SimFixed::lit("0.03"),
            SimFixed::lit("0.002"),
            SimFixed::from_num(500),
            SimFixed::from_num(499),
        );

        assert_eq!(
            owner_speed.applied_fraction,
            SIM_HALF - SimFixed::from_num(10) * SimFixed::lit("0.002")
        );
    }

    #[test]
    fn accelerates_true_braking_uses_strict_slowdown_distance() {
        let mut owner_speed = FootSpeedState {
            applied_fraction: SIM_HALF,
            ..Default::default()
        };
        let mut drive = DriveLocomotionRuntime::default();

        update_drive_speed_fraction(
            &mut drive,
            &mut owner_speed,
            SIM_ONE,
            true,
            SimFixed::from_num(10),
            SimFixed::lit("0.03"),
            SimFixed::lit("0.002"),
            SimFixed::from_num(500),
            SimFixed::from_num(500),
        );

        assert_eq!(owner_speed.applied_fraction, SimFixed::lit("0.53"));
    }
}
