//! Scenario-start crate placement — gamemd `ScenarioClass::Post_Map_Init`.
//!
//! When the lobby "Crates" option is on (stock default), the scenario clamps the
//! player count between `[CrateRules] CrateMinimum` and `CrateMaximum` and calls
//! the map's random-cell crate placer exactly that many times. Each placer call
//! first looks for a free entry in the map's 256-entry crate-slot array and
//! returns immediately — spending no draws — if there is none. Otherwise it walks
//! a bounded retry loop; every attempt draws a random X then a random Y inside
//! the active Map rectangle, snaps the result to a nearby passable cell, and
//! submits the destination to the crate-specific native Mark transaction.
//!
//! The drawn cell decides the snap movement: water uses *float*, anything else
//! uses *track*. After snapping, the destination cell independently chooses
//! `WaterCrateImg` or `WoodCrateImg` from its own land type.
//!
//! This module owns startup placement and its persistent slot/timer result.
//! The [`runtime`] submodule owns the live slot clear, the identity-specific
//! overlay removal, and the per-tick `CrateRegen` scan. [`pickup`] owns
//! `CellClass::PickupCrate @ 0x00481A00`, the pickup the movement hosts call
//! when a Foot commits to a crate cell; [`effects`] and [`speed`] own its
//! effect arms.
//!
//! [`construct_runtime_overlay`] is the runtime `OverlayClass::OverlayClass
//! @ 0x005FC380` owner; its Mark is the one port in [`crate::map::overlay_mark`].
//! Meteor landing calls it with its own selected type and scope; crate
//! validation still owns its clearing bracket.
//!
//! ## Dependency rules
//! Part of `sim/` — depends on `rules/`, `map/` grid types and other `sim/`
//! modules only. Never on render/, ui/, audio/, net/.

mod effects;
mod pickup;
mod runtime;
mod speed;
mod state;

pub(crate) use effects::CrateMultipliers;
pub(crate) use pickup::pickup_crate;
pub(crate) use runtime::tick_crate_regeneration;
pub use state::{CrateAuthority, CrateSlot};

use crate::map::lighting::LightingProfileUnits;
#[cfg(any(test, feature = "test-support"))]
use crate::map::lighting::ParsedLightingProfiles;
use crate::map::overlay_mark::{
    MarkCrateImages, OverlayMarkOutcome, OverlayMarkRequest, mark_overlay,
};
use crate::rules::crate_rules::CrateRules;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::cell_rect::cell_is_in_playfield_height_aware;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::overlay_mark_host::SimulationMarkHost;
use crate::sim::world::Simulation;

/// Retry budget inside one placer call. Every attempt spends its two draws
/// whether or not the cell works out; after this many failures the call gives up
/// and no crate is placed.
const MAX_PLACEMENT_ATTEMPTS: u32 = 1000;

/// Outcome of one scenario-start crate pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CratePlacement {
    /// Signed attempts selected by native minimum/human/maximum comparisons.
    pub requested: i32,
    /// Accepted calls, including timed ghosts whose overlay Mark failed.
    pub accepted: u32,
    /// Accepted calls that installed a visible overlay.
    pub visible: u32,
}

/// `min(max(CrateMinimum, player_count), CrateMaximum)`.
///
/// gamemd applies the floor first and the ceiling second, so a `CrateMaximum`
/// below `CrateMinimum` wins — the clamp is not symmetric and must not be
/// rewritten as a single `clamp()` call with the arguments in rules order.
pub fn scenario_start_crate_count(rules: &CrateRules, player_count: i32) -> i32 {
    rules.maximum.min(rules.minimum.max(player_count))
}

/// The human seat count the crate clamp is applied to.
///
/// gamemd's pregame setup assigns the crate-count global straight from the value
/// it logs as "Pregame setup for N players", and the sibling `Post_Map_Init`
/// budget gate adds the AI count to that same value — so it is human seats only.
/// In a 1v3 skirmish this is 1, not 4. Passive houses (Neutral, Special) are
/// never seats.
pub fn human_player_count(sim: &Simulation) -> i32 {
    i32::try_from(
        sim.houses
            .values()
            .filter(|house| house.is_human && !house.multiplay_passive)
            .count(),
    )
    .unwrap_or(i32::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OneCrateResult {
    HardRejected,
    AcceptedGhost,
    AcceptedVisible,
}

/// Native allocation, Unlimbo, and Mark failures all collapse to the same
/// gameplay result after the two hard prechecks: an accepted timed ghost.
/// Production invariants select `None`; focused tests inject the other cases
/// without importing gamemd's object allocator into simulation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ForcedPostPrecheckFailure {
    #[default]
    None,
    #[cfg(any(test, feature = "test-support"))]
    Allocation,
    #[cfg(any(test, feature = "test-support"))]
    Unlimbo,
    #[cfg(any(test, feature = "test-support"))]
    Mark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcceptedCellResult {
    Ghost,
    Visible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CrateRandomFrame {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

/// Place the scenario-start crates.
///
/// `player_count` is the lobby *human* player count. gamemd clamps a session
/// global that the pregame-setup routine assigns from the same value it logs as
/// "Pregame setup for N players", and the sibling budget gate in `Post_Map_Init`
/// adds the AI count to it separately — so it counts human seats only, not
/// human + AI. Callers pass VERA's human, non-passive house count.
///
/// ## RNG
/// Both coordinates are drawn from the scenario stream — the placer loads the
/// scenario instance pointer and adds `0x218` before each `RandomRanged` call,
/// the same member `Gather_Start_Positions` binds and the one VERA models as
/// `scenario_rng`. Failed attempts spend X then Y. A successful overlay spends
/// one additional `RandomRanged(0, 0x7fff_fffe)` draw for its crate slot timer.
#[cfg(any(test, feature = "test-support"))]
pub fn place_scenario_start_crates(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    player_count: i32,
) -> CratePlacement {
    place_scenario_start_crates_with_lighting(
        sim,
        rules,
        overlay_registry,
        player_count,
        ParsedLightingProfiles::default().normal,
    )
}

/// Production entry carrying the already parsed ordinary ScenarioClass
/// lighting profile. `OverlayClass::Mark` copies each target CellClass's
/// `+0x10A` value into a spawned CellAnim after construction. The runtime
/// regeneration rung reaches the same Mark path every tick, so the shared
/// helpers below are no longer scenario-start-only.
pub(crate) fn place_scenario_start_crates_with_lighting(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    player_count: i32,
    lighting_profile: LightingProfileUnits,
) -> CratePlacement {
    place_scenario_start_crates_with_failure(
        sim,
        rules,
        overlay_registry,
        player_count,
        lighting_profile,
        ForcedPostPrecheckFailure::None,
    )
}

fn place_scenario_start_crates_with_failure(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    player_count: i32,
    lighting_profile: LightingProfileUnits,
    forced_failure: ForcedPostPrecheckFailure,
) -> CratePlacement {
    if !sim.session.game_options.crates {
        return CratePlacement {
            requested: 0,
            accepted: 0,
            visible: 0,
        };
    }
    let requested = scenario_start_crate_count(&rules.crate_rules, player_count);
    if requested <= 0 {
        return CratePlacement {
            requested,
            accepted: 0,
            visible: 0,
        };
    }

    let mut accepted = 0u32;
    let mut visible = 0u32;
    for _ in 0..requested {
        match place_one_random_crate(
            sim,
            rules,
            overlay_registry,
            lighting_profile,
            forced_failure,
        ) {
            OneCrateResult::HardRejected => {}
            OneCrateResult::AcceptedGhost => accepted = accepted.wrapping_add(1),
            OneCrateResult::AcceptedVisible => {
                accepted = accepted.wrapping_add(1);
                visible = visible.wrapping_add(1);
            }
        }
    }
    if visible != 0 {
        // Post_Map_Init publishes navigation before crate placement. Native
        // Mark mutates live CellClass land/zone/bridge state synchronously;
        // refresh Rust's derived bridge and path projections once after the
        // ordered crate batch, without introducing another RNG boundary.
        sim.refresh_bridge_runtime_after_crate_mark();
        let _ = sim.rebuild_dynamic_navigation(rules);
    }
    log::info!(
        "Scenario-start crates: requested {requested}, accepted {accepted}, visible {visible}"
    );
    CratePlacement {
        requested,
        accepted,
        visible,
    }
}

/// Native water-vs-land classification, evaluated independently at the drawn
/// cell for movement and at the snapped destination for image selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrateSurface {
    Land,
    Water,
}

/// One `MapClass__PlaceCrateAtRandomCell @ 0x0056BD40` call.
fn place_one_random_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    lighting_profile: LightingProfileUnits,
    forced_failure: ForcedPostPrecheckFailure,
) -> OneCrateResult {
    let Some(slot_index) = sim.crate_authority.first_empty_index() else {
        // The native full-table return precedes every random draw.
        return OneCrateResult::HardRejected;
    };
    let Some(frame) = crate_random_frame(sim) else {
        return OneCrateResult::HardRejected;
    };
    for _ in 0..MAX_PLACEMENT_ATTEMPTS {
        let drawn = draw_crate_candidate(&mut sim.scenario_rng, frame);

        // The drawn cell's own land type selects the snap, before any snapping
        // happens. A draw outside the grid takes the land branch, matching the
        // native fallback cell.
        let movement_surface = crate_surface_at_packed(sim, drawn);

        let Some(cell) = snap_to_passable(sim, drawn, movement_surface) else {
            continue;
        };
        if let Some(accepted) = place_overlay_and_init_timer(
            sim,
            rules,
            overlay_registry,
            slot_index,
            cell,
            lighting_profile,
            forced_failure,
        ) {
            return match accepted {
                AcceptedCellResult::Ghost => OneCrateResult::AcceptedGhost,
                AcceptedCellResult::Visible => OneCrateResult::AcceptedVisible,
            };
        }
    }
    OneCrateResult::HardRejected
}

/// `CrateSlot__PlaceOverlayAndInitTimer @ 0x004A17C0` for the empty slot
/// `slot_index`: `CrateSlot__ValidateCellAndCreateOverlay` at `cell` (playfield,
/// no overlay, then the crate Mark), and on acceptance the slot coordinate and
/// its one Scenario timer draw. `None` when the cell is refused, before any
/// draw. Shared by the random placer (`0x0056BD40`) and `MapClass::Place_Crate
/// @ 0x0056BEC0`.
fn place_overlay_and_init_timer(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    slot_index: usize,
    cell: (u16, u16),
    lighting_profile: LightingProfileUnits,
    forced_failure: ForcedPostPrecheckFailure,
) -> Option<AcceptedCellResult> {
    if cell == (0, 0)
        || !cell_is_in_playfield_height_aware(
            (i32::from(cell.0), i32::from(cell.1)),
            sim.playfield_bounds,
            sim.resolved_terrain.as_ref(),
        )
    {
        return None;
    }
    // A cell that already carries ore, a wall, a bridge piece or another
    // crate cannot take one.
    if sim
        .overlay_grid
        .as_ref()
        .is_some_and(|grid| grid.cell(cell.0, cell.1).overlay_id.is_some())
    {
        return None;
    }
    let accepted = validate_and_stamp_candidate_with_rules_and_lighting(
        sim,
        rules,
        overlay_registry,
        cell,
        lighting_profile,
        forced_failure,
    );

    // `CrateSlot__PlaceOverlayAndInitTimer @ 0x004A17C0`: accepted ghosts
    // and visible overlays both write the packed coordinate before drawing
    // and installing start/aux/duration timer words.
    {
        let slot = sim.crate_authority.slot_mut(slot_index);
        slot.cell_x = cell.0 as i16;
        slot.cell_y = cell.1 as i16;
    }
    let timer_draw = sim.scenario_rng.next_range_u32_inclusive(0, 0x7fff_fffe);
    let (start_frame, aux, duration) = state::crate_timer_words(
        rules.crate_rules.regen,
        timer_draw,
        sim.session.binary_frame as i32,
    );
    let slot = sim.crate_authority.slot_mut(slot_index);
    slot.start_frame = start_frame;
    slot.aux = aux;
    slot.duration = duration;
    Some(accepted)
}

/// `MapClass::Place_Crate @ 0x0056BEC0` at `cell` with a fixed powerup slot,
/// or `None` for the random one (`0x14`). The requested cell's land type
/// selects the snap (Float on water, else Track) through the same
/// Find_Nearby_Passable_Cell arguments as the random placer (`0x0056BF08..
/// 0x0056BF69`); the first empty slot then takes one
/// [`place_overlay_and_init_timer`] attempt (`0x0056BF7B..0x0056BFBC`). On
/// acceptance a fixed powerup is written to the snapped cell's overlay data
/// byte (`0x0056BFC2..0x0056BFF9`), which the pickup reads as its stored
/// selection. No retry and no draw before the commit.
pub(crate) fn place_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    cell: (i16, i16),
    powerup: Option<u8>,
) -> bool {
    let requested = (i32::from(cell.0), i32::from(cell.1));
    let surface = crate_surface_at_packed(sim, requested);
    let Some(snapped) = snap_to_passable(sim, requested, surface) else {
        return false;
    };
    let Some(slot_index) = sim.crate_authority.first_empty_index() else {
        return false;
    };
    let lighting = sim
        .session
        .lighting
        .profile(crate::sim::scenario_session::ScenarioLightingProfile::Normal)
        .into();
    if place_overlay_and_init_timer(
        sim,
        rules,
        overlay_registry,
        slot_index,
        snapped,
        lighting,
        ForcedPostPrecheckFailure::None,
    )
    .is_none()
    {
        return false;
    }
    if let Some(powerup) = powerup {
        write_crate_mark_raw_data(sim, (snapped.0 as i16, snapped.1 as i16), powerup);
    }
    true
}

/// Active Map rectangle used by random placement ([`Simulation::map_rect`]);
/// LocalSize and the canonical Rust cell-array extent do not own these draws.
fn crate_random_frame(sim: &Simulation) -> Option<CrateRandomFrame> {
    let [left, top, width, height] = sim.map_rect()?;
    Some(CrateRandomFrame {
        left,
        top,
        width,
        height,
    })
}

/// `MapClass__PlaceCrateAtRandomCell @ 0x0056BD8B..0x0056BDD3` always draws
/// X then Y, adds the signed rectangle origin with dword wrapping, and stores
/// each result through a 16-bit word before the later MOVSX reads it back.
fn draw_crate_candidate(rng: &mut crate::sim::rng::SimRng, frame: CrateRandomFrame) -> (i32, i32) {
    let x = frame
        .left
        .wrapping_add(rng.next_range_i32_inclusive(0, frame.width.wrapping_sub(1)));
    let y = frame
        .top
        .wrapping_add(rng.next_range_i32_inclusive(0, frame.height.wrapping_sub(1)));
    (x as i16 as i32, y as i16 as i32)
}

#[cfg(any(test, feature = "test-support"))]
fn validate_and_stamp_candidate(
    sim: &mut Simulation,
    rules: &CrateRules,
    overlay_registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    forced_failure: ForcedPostPrecheckFailure,
) -> AcceptedCellResult {
    validate_and_stamp_candidate_inner(
        sim,
        rules,
        None,
        overlay_registry,
        cell,
        ParsedLightingProfiles::default().normal,
        forced_failure,
    )
}

#[cfg(any(test, feature = "test-support"))]
fn validate_and_stamp_candidate_with_rules(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    forced_failure: ForcedPostPrecheckFailure,
) -> AcceptedCellResult {
    validate_and_stamp_candidate_with_rules_and_lighting(
        sim,
        rules,
        overlay_registry,
        cell,
        ParsedLightingProfiles::default().normal,
        forced_failure,
    )
}

fn validate_and_stamp_candidate_with_rules_and_lighting(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    lighting_profile: LightingProfileUnits,
    forced_failure: ForcedPostPrecheckFailure,
) -> AcceptedCellResult {
    validate_and_stamp_candidate_inner(
        sim,
        &rules.crate_rules,
        Some(rules),
        overlay_registry,
        cell,
        lighting_profile,
        forced_failure,
    )
}

fn validate_and_stamp_candidate_inner(
    sim: &mut Simulation,
    rules: &CrateRules,
    full_rules: Option<&RuleSet>,
    overlay_registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    lighting_profile: LightingProfileUnits,
    forced_failure: ForcedPostPrecheckFailure,
) -> AcceptedCellResult {
    // CrateValidate4A18F0 clears A8E7AC at4A190B, then restores it on
    // both returns. The constructor/Mark below also serves non-crate callers.
    sim.without_object_placement_scope(|sim| {
        let selected = match crate_surface_at(sim, cell) {
            CrateSurface::Water => rules.water_crate_img.as_deref(),
            CrateSurface::Land => rules.wood_crate_img.as_deref(),
        }
        .and_then(|name| overlay_registry.id_for_name(name));
        let Some(selected_id) = selected else {
            return AcceptedCellResult::Ghost;
        };
        if forced_failure != ForcedPostPrecheckFailure::None {
            return AcceptedCellResult::Ghost;
        }
        construct_runtime_overlay(
            sim,
            rules,
            full_rules,
            overlay_registry,
            cell,
            selected_id,
            lighting_profile,
        )
    })
}

/// Runtime `OverlayClass::OverlayClass @ 0x005FC380` with constructor
/// argument -1, revealed into the one `OverlayClass::Mark @ 0x005FC570` port
/// ([`crate::map::overlay_mark`]) on the live Simulation. CrateValidate owns
/// its own ScenarioInit bracket; meteor landing keeps the current scope.
/// LoadObjectLifecycle keeps the transient Overlay's registries and identity.
/// Visible when Mark left a new identity on the cell.
pub(crate) fn construct_runtime_overlay(
    sim: &mut Simulation,
    rules: &CrateRules,
    full_rules: Option<&RuleSet>,
    overlay_registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    selected_id: u8,
    lighting_profile: LightingProfileUnits,
) -> AcceptedCellResult {
    let Some(selected_flags) = overlay_registry.flags(selected_id).cloned() else {
        return AcceptedCellResult::Ghost;
    };
    // Original5FC380 constructs Object/Abstract and joins all five registries
    // before its Terrain/Unlimbo/Mark gates. Reuse the authored/runtime owner:
    // an ordinary crate spends an ID even though Mark immediately UnInit's
    // this transient object. Evidence: trigger_crate money.lifecycle and the
    // shared bridge_constructor corpus. Allocate before a nested CellAnim.
    let stable_id = sim.allocate_stable_id();
    let handle = sim
        .load_objects
        .construct_overlay(stable_id, cell, || {
            crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(
                &mut sim.native_unique_ids,
            )
        })
        .expect("crate Overlay constructor must join its owned registries");
    // `OverlayClass::OverlayClass @ 0x005FC380` performs the TerrainClass scan
    // before calling Unlimbo. A hit skips Unlimbo, so Mark never runs.
    if sim.production.terrain_object_cells.contains_key(&cell) {
        sim.load_objects
            .finish_unrevealed_survivor(handle)
            .expect("Terrain-refused crate retains its constructed Overlay");
        return AcceptedCellResult::Ghost;
    }
    if sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .is_none()
    {
        sim.load_objects
            .finish_unrevealed_survivor(handle)
            .expect("an unavailable crate cell cannot reveal its Overlay");
        return AcceptedCellResult::Ghost;
    }
    let dirty_cell = sim
        .load_objects
        .begin_mark(handle, None)
        .expect("crate Overlay enters Mark once");
    sim.tactical_dirty_cells.push(dirty_cell);

    let identity_before = crate_cell_identity(sim, cell);
    let request = OverlayMarkRequest {
        overlay_id: selected_id,
        flags: &selected_flags,
        cell: (cell.0 as i16, cell.1 as i16),
        location: [
            i32::from(cell.0).wrapping_mul(256).wrapping_add(128),
            i32::from(cell.1).wrapping_mul(256).wrapping_add(128),
            0,
        ],
        registry: overlay_registry,
        tiberium_types: full_rules.map(|rules| &rules.tiberium_types),
        crate_images: MarkCrateImages::resolve(rules, overlay_registry),
    };
    let outcome = match mark_overlay(
        &mut SimulationMarkHost::new(sim, full_rules, overlay_registry, lighting_profile),
        &request,
    ) {
        Ok(outcome) => outcome,
        Err(never) => match never {},
    };
    match outcome {
        OverlayMarkOutcome::SlopeRefused => {
            sim.load_objects
                .finish_slope_survivor(handle)
                .expect("steep-slope crate retains its live limbo Overlay");
            return AcceptedCellResult::Ghost;
        }
        OverlayMarkOutcome::WallRefused => {
            sim.load_objects
                .finish_wall_reject(handle, &mut sim.substrate.pending_delete)
                .expect("refused wall crate completes its shared Overlay lifecycle");
            return AcceptedCellResult::Ghost;
        }
        OverlayMarkOutcome::Marked => sim
            .load_objects
            .finish_common(handle, &mut sim.substrate.pending_delete)
            .expect("crate Mark completes its shared Overlay lifecycle"),
    }
    let identity_after = crate_cell_identity(sim, cell);
    if identity_after.is_some() && identity_after != identity_before {
        AcceptedCellResult::Visible
    } else {
        AcceptedCellResult::Ghost
    }
}

fn crate_cell_identity(sim: &Simulation, cell: (u16, u16)) -> Option<u8> {
    sim.overlay_grid
        .as_ref()
        .and_then(|grid| grid.cell(cell.0, cell.1).overlay_id)
}

#[derive(Debug, Clone)]
enum CrateMarkCellRef {
    Real(u16, u16),
    Dummy(crate::map::resolved_terrain::SharedCellDummy),
}

fn resolve_crate_mark_cell(sim: &Simulation, cell: (i16, i16)) -> CrateMarkCellRef {
    let x = i32::from(cell.0);
    let y = i32::from(cell.1);
    if let Some((rx, ry)) = crate::map::cell_index::canonical_cell_coord(x, y)
        && sim
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(rx, ry))
            .is_some()
        && sim
            .overlay_grid
            .as_ref()
            .is_some_and(|grid| rx < grid.width() && ry < grid.height())
    {
        return CrateMarkCellRef::Real(rx, ry);
    }
    let dummy = sim.effective_shared_cell_dummy();
    dummy.stamp_coord(x, y);
    CrateMarkCellRef::Dummy(dummy)
}

fn read_crate_mark_fields(sim: &Simulation, cell: (i16, i16)) -> (Option<u8>, u8) {
    match resolve_crate_mark_cell(sim, cell) {
        CrateMarkCellRef::Real(rx, ry) => {
            let cell = sim
                .overlay_grid
                .as_ref()
                .expect("real crate Mark cell requires OverlayGrid")
                .cell(rx, ry);
            (cell.overlay_id, cell.overlay_data)
        }
        CrateMarkCellRef::Dummy(dummy) => dummy.overlay_fields(),
    }
}

fn write_crate_mark_raw_data(sim: &mut Simulation, cell: (i16, i16), overlay_data: u8) {
    match resolve_crate_mark_cell(sim, cell) {
        CrateMarkCellRef::Real(rx, ry) => {
            let (Some(grid), Some(terrain)) =
                (sim.overlay_grid.as_mut(), sim.resolved_terrain.as_mut())
            else {
                return;
            };
            let _ = grid.write_crate_mark_data_field(terrain, rx, ry, overlay_data);
        }
        CrateMarkCellRef::Dummy(dummy) => {
            let (overlay_id, _) = dummy.overlay_fields();
            dummy.set_overlay_fields(overlay_id, overlay_data);
        }
    }
}

fn crate_surface_at(sim: &Simulation, cell: (u16, u16)) -> CrateSurface {
    if sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .is_some_and(|cell| cell.land_type() == LandType::Water.as_index())
    {
        CrateSurface::Water
    } else {
        CrateSurface::Land
    }
}

fn crate_surface_at_packed(sim: &Simulation, cell: (i32, i32)) -> CrateSurface {
    let Some((rx, ry)) = crate::map::cell_index::canonical_cell_coord(cell.0, cell.1) else {
        return CrateSurface::Land;
    };
    crate_surface_at(sim, (rx, ry))
}

/// Snap a drawn cell onto a nearby passable cell of the matching surface.
/// A nonpositive map Size sum scans no rings ([`map_owned_radius_cap`]), after
/// the X/Y draws have occurred.
///
/// [`map_owned_radius_cap`]: crate::sim::find_nearby_cell::map_owned_radius_cap
fn snap_to_passable(
    sim: &Simulation,
    drawn: (i32, i32),
    surface: CrateSurface,
) -> Option<(u16, u16)> {
    // gamemd-derived: every FNPC candidate path, including crate placement,
    // calls Is_Cell_In_Playfield_CellClass(cell, 1) @ 0x00578540 before
    // CellRect passability (caller PlaceCrateAtRandomCell @ 0x0056BD40), and
    // PlaceCrateAtRandomCell initializes the reference to the engine zero
    // cell, which FNPC @ 0x0056DC20 reads as "no target": the plain shape.
    sim.nearby_passable_cell(
        drawn,
        NearbyArgs::plain(
            PassabilityArgs {
                // Verified: the placer passes native speed type 5 (float) when
                // the drawn cell is water and 1 (track) otherwise.
                speed_type: match surface {
                    CrateSurface::Water => SpeedType::Float,
                    CrateSurface::Land => SpeedType::Track,
                },
                required_zone_id: None,
                movement_zone: MovementZone::Normal,
                bridge_aware_zone: false,
            },
            NearbyFootprint::SINGLE,
        ),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::sim::cell_rect::PlayfieldBounds;
    use crate::sim::occupancy::OBJECT_OCCUPATION_BIT;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::pathfinding::PathGrid;

    const MAP: u16 = 40;

    /// The land row of every [`uniform_terrain`] cell: each SpeedType at
    /// 100%. A crate overlay's `Land=` (Clear) reads it from the registry's
    /// rules, and the grid's land rows from the same keys.
    const OPEN_LAND_KEYS: &str = "Foot=100%\nTrack=100%\nWheel=100%\nFloat=100%\n\
        Hover=100%\nAmphibious=100%\nFloatBeach=100%\n";

    pub(crate) fn crate_registry() -> OverlayTypeRegistry {
        let ini = IniFile::from_str(&format!(
            "[OverlayTypes]\n0=TIB01\n1=SILVER\n2=WOOD\n3=WATER\n\
             [TIB01]\nTiberium=yes\n[SILVER]\nCrate=yes\n\
             [WOOD]\nCrate=yes\n[WATER]\nCrate=yes\n[Clear]\n{OPEN_LAND_KEYS}",
        ));
        OverlayTypeRegistry::from_ini(&ini, None)
    }

    pub(crate) fn crate_ruleset(extra: &str) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [CrateRules]\nCrateImg=SILVER\nWoodCrateImg=WOOD\nWaterCrateImg=WATER\n{extra}",
        ));
        RuleSet::from_ini(&ini).expect("rules")
    }

    pub(crate) fn crate_ruleset_with_images(
        wood: &str,
        common: &str,
        water: &str,
        extra: &str,
    ) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [CrateRules]\nCrateImg={common}\nWoodCrateImg={wood}\nWaterCrateImg={water}\n{extra}",
        ));
        RuleSet::from_ini(&ini).expect("rules")
    }

    fn crate_registry_with_raw_b2() -> OverlayTypeRegistry {
        let mut ini_text = String::from("[OverlayTypes]\n");
        for index in 0..=0xB2u16 {
            let name = if index == 0xB2 {
                "STEEPCRATE".to_owned()
            } else {
                format!("OV{index:03}")
            };
            writeln!(&mut ini_text, "{index}={name}").expect("string write");
        }
        ini_text.push_str("[STEEPCRATE]\nCrate=yes\n");
        OverlayTypeRegistry::from_ini(&IniFile::from_str(&ini_text), None)
    }

    fn dense_registry(last: u8, overrides: &[(u8, &str, &str)]) -> OverlayTypeRegistry {
        let mut names: Vec<String> = (0..=last).map(|id| format!("OV{id:03}")).collect();
        for &(id, name, _) in overrides {
            names[usize::from(id)] = name.to_owned();
        }
        let mut ini_text = String::from("[OverlayTypes]\n");
        for (id, name) in names.iter().enumerate() {
            writeln!(&mut ini_text, "{id}={name}").unwrap();
        }
        for (id, name) in names.iter().enumerate() {
            let override_section = overrides.iter().find_map(|(candidate, _, section)| {
                (usize::from(*candidate) == id).then_some(*section)
            });
            let default_low_bridge = crate::rules::overlay_types::is_bridge_overlay_index(id as u8)
                && !crate::rules::overlay_types::is_high_bridge_index(id as u8);
            let section = override_section.or(default_low_bridge.then_some("Land=Road\n"));
            if let Some(section) = section {
                writeln!(&mut ini_text, "[{name}]").unwrap();
                ini_text.push_str(section);
            }
        }
        OverlayTypeRegistry::from_ini(&IniFile::from_str(&ini_text), None)
    }

    /// `seed` positions the scenario cursor; the placer draws from there.
    pub(crate) fn sim_with_grid(seed: u64) -> Simulation {
        let mut sim = Simulation::new();
        sim.session.seed = seed;
        sim.scenario_rng = crate::sim::rng::SimRng::new(seed);
        sim.session.map_width = MAP;
        sim.session.map_height = MAP;
        // Production installs MapClass authority before Post_Map_Init reaches
        // crate placement. Keep this focused fixture intentionally broad while
        // exercising the same required mode-one gate.
        sim.playfield_bounds = Some(PlayfieldBounds {
            base: 20,
            off_fc: -128,
            off_100: -128,
            off_104: 256,
            off_108: 256,
        });
        sim.playfield_size_height = Some(20);
        sim.session.game_options.crates = true;
        sim.overlay_grid = Some(OverlayGrid::new(MAP, MAP));
        sim.resolved_terrain = Some(uniform_terrain(false));
        sim
    }

    fn cells_with_overlay(
        sim: &Simulation,
        registry: &OverlayTypeRegistry,
        name: &str,
    ) -> Vec<(u16, u16)> {
        let id = registry.id_for_name(name).expect("overlay type");
        let grid = sim.overlay_grid.as_ref().expect("overlay grid");
        let mut cells = Vec::new();
        for ry in 0..grid.height() {
            for rx in 0..grid.width() {
                if grid.cell(rx, ry).overlay_id == Some(id) {
                    cells.push((rx, ry));
                }
            }
        }
        cells
    }

    pub(crate) fn crate_cells(sim: &Simulation, registry: &OverlayTypeRegistry) -> Vec<(u16, u16)> {
        cells_with_overlay(sim, registry, "WOOD")
    }

    fn first_random_cell(sim: &Simulation) -> ((u16, u16), crate::sim::rng::SimRng) {
        let frame = crate_random_frame(sim).expect("valid crate random frame");
        let mut replay = sim.scenario_rng.clone();
        let cell =
            (
                frame.left.wrapping_add(
                    replay.next_range_u32_inclusive(0, (frame.width - 1) as u32) as i32,
                ) as u16,
                frame.top.wrapping_add(
                    replay.next_range_u32_inclusive(0, (frame.height - 1) as u32) as i32,
                ) as u16,
            );
        (cell, replay)
    }

    #[test]
    fn gsi_01_04_crate_count_applies_minimum_then_maximum() {
        let rules = CrateRules {
            minimum: 1,
            maximum: 255,
            ..CrateRules::default()
        };
        // Stock shape: one crate per player, never below CrateMinimum.
        assert_eq!(scenario_start_crate_count(&rules, 0), 1);
        assert_eq!(scenario_start_crate_count(&rules, 1), 1);
        assert_eq!(scenario_start_crate_count(&rules, 4), 4);
        assert_eq!(scenario_start_crate_count(&rules, 900), 255);

        let floored = CrateRules {
            minimum: 6,
            maximum: 255,
            ..CrateRules::default()
        };
        assert_eq!(scenario_start_crate_count(&floored, 2), 6);

        // The ceiling is applied after the floor, so a maximum under the
        // minimum wins outright.
        let inverted = CrateRules {
            minimum: 10,
            maximum: 3,
            ..CrateRules::default()
        };
        assert_eq!(scenario_start_crate_count(&inverted, 1), 3);

        let signed = CrateRules {
            minimum: -9,
            maximum: -3,
            ..CrateRules::default()
        };
        assert_eq!(scenario_start_crate_count(&signed, -7), -7);
        assert_eq!(scenario_start_crate_count(&signed, 4), -3);

        let uncapped_slot_count = CrateRules {
            minimum: 300,
            maximum: i32::MAX,
            ..CrateRules::default()
        };
        assert_eq!(scenario_start_crate_count(&uncapped_slot_count, 1), 300);
    }

    #[test]
    fn gsi_01_04_crate_rules_read_all_three_image_keys() {
        let rules = crate_ruleset("CrateMinimum=2\nCrateMaximum=9\n");
        assert_eq!(rules.crate_rules.minimum, 2);
        assert_eq!(rules.crate_rules.maximum, 9);
        assert_eq!(rules.crate_rules.crate_img.as_deref(), Some("SILVER"));
        assert_eq!(rules.crate_rules.wood_crate_img.as_deref(), Some("WOOD"));
        assert_eq!(rules.crate_rules.water_crate_img.as_deref(), Some("WATER"));

        let defaults = crate_ruleset("");
        assert_eq!(defaults.crate_rules.minimum, 1);
        assert_eq!(defaults.crate_rules.maximum, 255);
    }

    #[test]
    fn gsi_01_04_crate_placement_respects_clamped_count_and_distinct_cells() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=3\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x1234_5678);
        sim.install_fixture_path_grid(Some(&grid));

        // Five players, ceiling 3.
        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 5);

        assert_eq!(result.requested, 3);
        assert_eq!((result.accepted, result.visible), (3, 3));
        let cells = crate_cells(&sim, &registry);
        assert_eq!(cells.len(), 3);
        let unique: BTreeSet<(u16, u16)> = cells.iter().copied().collect();
        assert_eq!(unique.len(), 3, "no two crates share a cell");
        for (rx, ry) in cells {
            assert!(rx < MAP && ry < MAP, "crate inside the map rectangle");
        }
    }

    #[test]
    fn gsi_01_04_crate_minimum_lifts_single_human_count() {
        let rules = crate_ruleset("CrateMinimum=4\nCrateMaximum=255\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(99);
        sim.install_fixture_path_grid(Some(&grid));

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 1);

        assert_eq!(result.requested, 4);
        assert_eq!((result.accepted, result.visible), (4, 4));
        assert_eq!(crate_cells(&sim, &registry).len(), 4);
    }

    #[test]
    fn gsi_01_04_crate_disabled_spends_no_rng_and_places_nothing() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=255\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(7);
        sim.install_fixture_path_grid(Some(&grid));
        sim.session.game_options.crates = false;
        let rng_before = sim.scenario_rng.state();

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 4);

        assert_eq!(
            result,
            CratePlacement {
                requested: 0,
                accepted: 0,
                visible: 0,
            }
        );
        assert!(crate_cells(&sim, &registry).is_empty());
        assert_eq!(sim.scenario_rng.state(), rng_before);
    }

    #[test]
    fn scenario_start_crate_missing_map_size_authority_invents_no_rectangle() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0xBAD5_1E00);
        sim.install_fixture_path_grid(Some(&grid));
        sim.playfield_size_height = None;
        let before = sim.scenario_rng.state();

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 1);

        assert_eq!((result.accepted, result.visible), (0, 0));
        assert_eq!(sim.scenario_rng.state(), before);
        assert!(sim.crate_authority.slots()[0].is_empty());
    }

    /// A successful request spends X, Y, then the crate-slot timer draw.
    #[test]
    fn gsi_01_04_crate_success_spends_x_y_then_timer() {
        let rules = crate_ruleset("CrateMinimum=3\nCrateMaximum=255\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        let mut first = sim_with_grid(0xABCD);
        first.install_fixture_path_grid(Some(&grid));
        let mut replay = first.scenario_rng.clone();
        place_scenario_start_crates(&mut first, &rules, &registry, 3);

        // Three successful first attempts: X, Y, timer for each crate.
        for _ in 0..3 {
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1));
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1));
            replay.next_range_u32_inclusive(0, 0x7fff_fffe);
        }
        assert_eq!(
            first.scenario_rng.state(),
            replay.state(),
            "successful crates must leave the cursor after X, Y, timer"
        );

        let mut same_cursor = sim_with_grid(0xABCD);
        same_cursor.install_fixture_path_grid(Some(&grid));
        place_scenario_start_crates(&mut same_cursor, &rules, &registry, 3);
        let mut other_cursor = sim_with_grid(0x1111);
        other_cursor.install_fixture_path_grid(Some(&grid));
        place_scenario_start_crates(&mut other_cursor, &rules, &registry, 3);

        assert_eq!(
            crate_cells(&first, &registry),
            crate_cells(&same_cursor, &registry),
            "the same scenario cursor must scatter crates identically"
        );
        assert_ne!(
            crate_cells(&first, &registry),
            crate_cells(&other_cursor, &registry),
            "a different scenario cursor must scatter crates differently"
        );
    }

    /// `MapClass::Place_Crate @ 0x0056BEC0`: one commit at the snapped
    /// requested cell with only the slot timer's draw; a fixed powerup lands
    /// in the overlay data byte, the random one (`0x14`) leaves the Mark's.
    #[test]
    fn place_crate_commits_at_the_requested_cell_with_one_timer_draw() {
        let rules = crate_ruleset("");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let wood = registry.id_for_name("WOOD").unwrap();
        for (powerup, data) in [(Some(0u8), 0u8), (None, u8::MAX)] {
            let mut sim = sim_with_grid(0x5EED);
            sim.install_fixture_path_grid(Some(&grid));
            let mut replay = sim.scenario_rng.clone();

            assert!(place_crate(&mut sim, &rules, &registry, (12, 14), powerup));

            replay.next_range_u32_inclusive(0, 0x7fff_fffe);
            assert_eq!(sim.scenario_rng.state(), replay.state(), "{powerup:?}");
            let cell = *sim.overlay_grid.as_ref().unwrap().cell(12, 14);
            assert_eq!((cell.overlay_id, cell.overlay_data), (Some(wood), data));
            let slot = sim.crate_authority.slots()[0];
            assert_eq!((slot.cell_x, slot.cell_y), (12, 14));
        }
    }

    /// A full slot table refuses after the snap, before any draw or write.
    #[test]
    fn place_crate_with_a_full_slot_table_places_nothing() {
        let rules = crate_ruleset("");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x5EED);
        sim.install_fixture_path_grid(Some(&grid));
        for index in 0..sim.crate_authority.slots().len() {
            let slot = sim.crate_authority.slot_mut(index);
            slot.cell_x = 1;
            slot.cell_y = 1;
        }
        let before = sim.scenario_rng.state();

        assert!(!place_crate(&mut sim, &rules, &registry, (12, 14), Some(0)));

        assert_eq!(sim.scenario_rng.state(), before);
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(12, 14).overlay_id,
            None
        );
    }

    #[test]
    fn gsi_01_04_crate_failed_attempt_spends_only_x_y_before_success_timer() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0xA11CE);
        sim.install_fixture_path_grid(Some(&grid));
        let mut replay = sim.scenario_rng.clone();

        let first = (
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
        );
        let second = (
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
        );
        assert_ne!(first, second, "fixture needs a distinct second attempt");
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);

        let blocker = registry.id_for_name("TIB01").expect("overlay type");
        sim.overlay_grid
            .as_mut()
            .expect("overlay grid")
            .place_overlay(first.0, first.1, blocker, 0);

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 1);

        assert_eq!((result.accepted, result.visible), (1, 1));
        assert_eq!(crate_cells(&sim, &registry), vec![second]);
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }

    #[test]
    fn gsi_01_04_crate_second_request_sees_first_overlay_immediately() {
        let rules = crate_ruleset("CrateMinimum=2\nCrateMaximum=2\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        // Seed 1671 yields (37,4), timer, (37,4), then retry (5,8).
        let mut sim = sim_with_grid(1671);
        sim.install_fixture_path_grid(Some(&grid));
        let mut replay = sim.scenario_rng.clone();
        let first = (
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
        );
        assert_eq!(first, (37, 4));
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);
        assert_eq!(
            (
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            ),
            first
        );
        let retry = (
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
        );
        assert_eq!(retry, (5, 8));
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 2);

        assert_eq!((result.accepted, result.visible), (2, 2));
        assert_eq!(
            crate_cells(&sim, &registry)
                .into_iter()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([first, retry])
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }

    #[test]
    fn gsi_01_04_crate_one_thousand_failures_spend_exactly_two_thousand_draws() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0xF411ED);
        sim.install_fixture_path_grid(Some(&grid));
        // Empty diamond: every FNPC candidate fails the mandatory anchor gate
        // (and the independent final playfield gate would reject it too).
        sim.playfield_bounds = Some(PlayfieldBounds {
            base: 20,
            off_fc: 0,
            off_100: 0,
            off_104: 0,
            off_108: 0,
        });
        let mut replay = sim.scenario_rng.clone();
        for _ in 0..MAX_PLACEMENT_ATTEMPTS {
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1));
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1));
        }

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 1);

        assert_eq!((result.accepted, result.visible), (0, 0));
        assert!(crate_cells(&sim, &registry).is_empty());
        assert_eq!(sim.crate_authority.slots()[0], CrateSlot::default());
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }

    #[test]
    fn gsi_01_04_crate_full_slot_array_stops_before_more_rng() {
        let rules = crate_ruleset("CrateMinimum=300\nCrateMaximum=300\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x5107_0256);
        sim.install_fixture_path_grid(Some(&grid));
        let mut replay = sim.scenario_rng.clone();
        let mut occupied = BTreeSet::new();
        while occupied.len() < state::CRATE_SLOT_CAPACITY {
            let cell = (
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            );
            if occupied.insert(cell) {
                replay.next_range_u32_inclusive(0, 0x7fff_fffe);
            }
        }

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 300);

        assert_eq!(result.requested, 300);
        assert_eq!(
            (result.accepted, result.visible),
            (
                state::CRATE_SLOT_CAPACITY as u32,
                state::CRATE_SLOT_CAPACITY as u32,
            )
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }

    #[test]
    fn scenario_start_crate_preexisting_full_slot_table_spends_no_rng() {
        let rules = crate_ruleset("CrateMinimum=4\nCrateMaximum=4\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x5107_F011);
        sim.install_fixture_path_grid(Some(&grid));
        for index in 0..state::CRATE_SLOT_CAPACITY {
            sim.crate_authority.slot_mut(index).cell_x = (index as i16).wrapping_add(1);
        }
        let before = sim.scenario_rng.state();

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 1);

        assert_eq!(result.requested, 4);
        assert_eq!((result.accepted, result.visible), (0, 0));
        assert_eq!(sim.scenario_rng.state(), before);
    }

    /// The scenario-start snap reads the live raw occupation byte. FNPC's
    /// `CellRect::CheckPassability @ 0x0056E7C0` pushes both ignore flags 0
    /// (`0x0056E854`, `0x0056E856`), so `CellClass::CheckCellPassability @
    /// 0x004834A0` refuses any nonzero `+0x124` byte (`0x00483552..
    /// 0x0048357A`). A drawn Cell holding any bit sends the crate, visible, to
    /// the first ring, with no draw beyond X, Y and the slot timer.
    #[test]
    fn scenario_start_crate_snap_skips_any_raw_ground_occupation_bit() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        for bit in [1, 2, 4, 8, 16, 32, 64, 128] {
            let mut sim = sim_with_grid(0x0CC0_0000 + u64::from(bit));
            sim.install_fixture_path_grid(Some(&grid));
            let (drawn, mut replay) = first_random_cell(&sim);
            replay.next_range_u32_inclusive(0, 0x7fff_fffe);
            sim.substrate
                .raw_cell_occupation
                .mark_ground(drawn.0, drawn.1, bit);

            let result = place_scenario_start_crates(&mut sim, &rules, &registry, 1);

            assert_eq!(
                (result.accepted, result.visible),
                (1, 1),
                "tested {bit:#04x}"
            );
            let placed = crate_cells(&sim, &registry);
            assert_eq!(placed.len(), 1, "tested {bit:#04x}");
            let placed = placed[0];
            assert_eq!(
                placed.0.abs_diff(drawn.0).max(placed.1.abs_diff(drawn.1)),
                1,
                "the snap skips the occupied drawn Cell; tested {bit:#04x}"
            );
            let slot = sim.crate_authority.slots()[0];
            assert_eq!(
                (slot.cell_x, slot.cell_y),
                (placed.0 as i16, placed.1 as i16)
            );
            assert_eq!(sim.scenario_rng.state(), replay.state());
        }
    }

    /// `CrateSlot__PlaceOverlayAndInitTimer @ 0x004A17C0` on a Cell whose raw
    /// ground occupation holds object bit 0x40: Mark refuses after the slot
    /// precheck, and the slot still takes the Cell and its timer draw. The
    /// scenario-start snap never returns such a Cell
    /// (`scenario_start_crate_snap_skips_any_raw_ground_occupation_bit`), so
    /// this enters at the slot.
    #[test]
    fn crate_slot_object_ground_occupation_accepts_a_timed_ghost() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x47C550);
        sim.install_fixture_path_grid(Some(&grid));
        let (rx, ry) = (12, 13);
        let mut replay = sim.scenario_rng.clone();
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);
        sim.substrate
            .raw_cell_occupation
            .mark_ground(rx, ry, OBJECT_OCCUPATION_BIT);

        let accepted = place_overlay_and_init_timer(
            &mut sim,
            &rules,
            &registry,
            0,
            (rx, ry),
            ParsedLightingProfiles::default().normal,
            ForcedPostPrecheckFailure::None,
        );

        assert_eq!(accepted, Some(AcceptedCellResult::Ghost));
        assert!(crate_cells(&sim, &registry).is_empty());
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .expect("overlay grid")
                .cell(rx, ry)
                .overlay_id,
            None,
            "a post-precheck Mark failure leaves the Cell unchanged",
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
        let slot = sim.crate_authority.slots()[0];
        assert_eq!((slot.cell_x, slot.cell_y), (rx as i16, ry as i16));
        assert_eq!(slot.start_frame, sim.session.binary_frame as i32);
        assert_eq!(
            slot.aux, 0x40d1_9400,
            "constructor regen 10 owns upper=18000"
        );
        assert!(slot.duration > 0);
    }

    /// Mark5FD065..5FD070 passes both ignore flags0 for the crate pointers.
    ///483558..483570 therefore refuses every nonzero raw occupation bit.
    /// The scenario-start snap never returns an occupied Cell, so this enters
    /// at the slot.
    #[test]
    fn crate_slot_any_raw_ground_occupation_bit_ghosts() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let cell = (12, 13);

        for bit in [1, 2, 4, 8, 16, 32, 64, 128] {
            let mut sim = sim_with_grid(0x0CC0_0000 + u64::from(bit));
            sim.install_fixture_path_grid(Some(&grid));
            sim.substrate
                .raw_cell_occupation
                .mark_ground(cell.0, cell.1, bit);
            let accepted = place_overlay_and_init_timer(
                &mut sim,
                &rules,
                &registry,
                0,
                cell,
                ParsedLightingProfiles::default().normal,
                ForcedPostPrecheckFailure::None,
            );
            assert_eq!(
                accepted,
                Some(AcceptedCellResult::Ghost),
                "crate Mark passes neither ignore flag; tested {bit:#04x}"
            );
            assert!(sim.crate_authority.slots()[0].cell_x != 0);
            assert_eq!(
                crate_cells(&sim, &registry).contains(&cell),
                false,
                "occupied crate Mark preserves the timed ghost"
            );
        }
    }

    /// Non-crate5FD03D uses both ignore flags1; SceneInit5FCFE7 skips
    /// passability and5FD079 skips Overrides. Exercise the actual shared
    /// constructor and lifecycle, without the crate caller's scope bracket.
    #[test]
    fn ordinary_overlay_constructor_keeps_its_own_ignore_and_scope_policy() {
        let rules = crate_ruleset("");
        let registry = crate_registry();
        let ordinary = registry.id_for_name("TIB01").unwrap();
        let cell = (12, 13);
        for bit in [0, 1, 2, 4, 8, 16, 32, 64, 128] {
            let mut sim = sim_with_grid(77);
            sim.substrate
                .raw_cell_occupation
                .mark_ground(cell.0, cell.1, bit);
            let before = sim.scenario_rng.native_state_hex();
            let result = construct_runtime_overlay(
                &mut sim,
                &rules.crate_rules,
                Some(&rules),
                &registry,
                cell,
                ordinary,
                ParsedLightingProfiles::default().normal,
            );
            assert_eq!(
                result,
                if bit == 0x40 {
                    AcceptedCellResult::Ghost
                } else {
                    AcceptedCellResult::Visible
                }
            );
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                before,
                "ordinary Clear/CellAnim-free input draws nothing"
            );
        }
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=BLOCK\n1=WOOD\n[BLOCK]\nOverrides=yes\n[WOOD]\nCrate=yes\n",
            ),
            None,
        );
        let old = registry.id_for_name("BLOCK").unwrap();
        let incoming = registry.id_for_name("WOOD").unwrap();
        for (initial_scope, occupation) in [(false, 0), (false, 0xFF), (true, 0), (true, 0xFF)] {
            let mut sim = sim_with_grid(78);
            sim.overlay_grid
                .as_mut()
                .unwrap()
                .place_overlay(cell.0, cell.1, old, 7);
            sim.substrate
                .raw_cell_occupation
                .mark_ground(cell.0, cell.1, occupation);
            let apply = |sim: &mut Simulation| {
                construct_runtime_overlay(
                    sim,
                    &rules.crate_rules,
                    Some(&rules),
                    &registry,
                    cell,
                    incoming,
                    ParsedLightingProfiles::default().normal,
                )
            };
            let result = if initial_scope {
                sim.with_object_placement_scope(apply)
            } else {
                apply(&mut sim)
            };
            assert_eq!(
                result,
                if initial_scope {
                    AcceptedCellResult::Visible
                } else {
                    AcceptedCellResult::Ghost
                }
            );
            let fields = sim.overlay_grid.as_ref().unwrap().cell(cell.0, cell.1);
            assert_eq!(
                (fields.overlay_id, fields.overlay_data),
                if initial_scope {
                    (Some(incoming), 255)
                } else {
                    (Some(old), 7)
                }
            );
        }
    }

    #[test]
    fn scenario_start_crate_null_image_and_terrain_object_are_accepted_ghosts() {
        let null_rules = crate_ruleset_with_images(
            "none",
            "SILVER",
            "WATER",
            "CrateMinimum=1\nCrateMaximum=1\nCrateRegen=3\n",
        );
        let ordinary_rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\nCrateRegen=3\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        let mut null_sim = sim_with_grid(0xA011_0001);
        null_sim.install_fixture_path_grid(Some(&grid));
        null_sim.session.binary_frame = u32::MAX;
        let (null_cell, mut null_replay) = first_random_cell(&null_sim);
        let timer_draw = null_replay.next_range_u32_inclusive(0, 0x7fff_fffe);
        let expected_timer = state::crate_timer_words(
            null_rules.crate_rules.regen,
            timer_draw,
            null_sim.session.binary_frame as i32,
        );
        let null_result = place_scenario_start_crates(&mut null_sim, &null_rules, &registry, 1);
        assert_eq!((null_result.accepted, null_result.visible), (1, 0));
        assert!(
            null_sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .iter_occupied()
                .next()
                .is_none()
        );
        let null_slot = null_sim.crate_authority.slots()[0];
        assert_eq!(
            (null_slot.cell_x, null_slot.cell_y),
            (null_cell.0 as i16, null_cell.1 as i16)
        );
        assert_eq!(
            (null_slot.start_frame, null_slot.aux, null_slot.duration),
            expected_timer
        );
        assert_eq!(null_sim.scenario_rng.state(), null_replay.state());

        let mut terrain_sim = sim_with_grid(0xA011_0002);
        terrain_sim.install_fixture_path_grid(Some(&grid));
        let (cell, _) = first_random_cell(&terrain_sim);
        terrain_sim.production.terrain_object_cells.insert(cell, 77);
        let terrain_result =
            place_scenario_start_crates(&mut terrain_sim, &ordinary_rules, &registry, 1);
        assert_eq!((terrain_result.accepted, terrain_result.visible), (1, 0));
        assert_eq!(
            (
                terrain_sim.crate_authority.slots()[0].cell_x,
                terrain_sim.crate_authority.slots()[0].cell_y
            ),
            (cell.0 as i16, cell.1 as i16)
        );
    }

    #[test]
    fn crate_placement_forced_post_precheck_failures_are_timed_ghosts() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\nCrateRegen=3\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        for failure in [
            ForcedPostPrecheckFailure::Allocation,
            ForcedPostPrecheckFailure::Unlimbo,
            ForcedPostPrecheckFailure::Mark,
        ] {
            let mut sim = sim_with_grid(0xFA11_0000 + failure as u64);
            sim.install_fixture_path_grid(Some(&grid));
            let result = place_scenario_start_crates_with_failure(
                &mut sim,
                &rules,
                &registry,
                1,
                ParsedLightingProfiles::default().normal,
                failure,
            );
            assert_eq!((result.accepted, result.visible), (1, 0));
            let slot = sim.crate_authority.slots()[0];
            assert!(!slot.is_empty());
            assert_eq!(slot.aux, 0x40b5_1800);
            assert!(
                sim.overlay_grid
                    .as_ref()
                    .unwrap()
                    .iter_occupied()
                    .next()
                    .is_none()
            );
        }
    }

    #[test]
    fn gsi_01_04_crate_water_track_zero_commits_and_spends_timer_draw() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0xF10A_7000);
        sim.install_fixture_path_grid(Some(&grid));
        sim.resolved_terrain = Some(uniform_terrain(true));
        for cell in &mut sim.resolved_terrain.as_mut().expect("terrain").cells {
            cell.speed_costs.track = Some(0);
            cell.speed_costs.float = Some(100);
        }
        let mut replay = sim.scenario_rng.clone();
        let destination = (
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
        );
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 1);

        assert_eq!((result.accepted, result.visible), (1, 1));
        assert_eq!(
            cells_with_overlay(&sim, &registry, "WATER"),
            vec![destination]
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }

    #[test]
    fn crate_placement_mark_uses_water_first_numeric_identity_alias() {
        let rules =
            crate_ruleset_with_images("WOOD", "SILVER", "WOOD", "CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let mut sim = sim_with_grid(0xA11A_5001);
        let cell = (7, 9);
        let terrain_cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        terrain_cell.speed_costs.track = Some(0);
        terrain_cell.speed_costs.float = Some(100);

        let result = validate_and_stamp_candidate(
            &mut sim,
            &rules.crate_rules,
            &registry,
            cell,
            ForcedPostPrecheckFailure::None,
        );

        assert_eq!(result, AcceptedCellResult::Visible);
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_id,
            registry.id_for_name("WOOD")
        );

        let mut zero_float = sim_with_grid(0xA11A_5002);
        let zero_cell = zero_float
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        zero_cell.speed_costs.track = Some(100);
        zero_cell.speed_costs.float = Some(0);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut zero_float,
                &rules.crate_rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost,
            "the aliased selected identity must use Water/Float, not Wood/Track"
        );
    }

    #[test]
    fn crate_placement_bridge_selects_full_deck_byte_and_bypasses_speed_zero() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let cell = (8, 11);

        let mut visible = sim_with_grid(0xB21D_0001);
        let terrain_cell = visible
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        terrain_cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        terrain_cell.speed_costs.track = Some(0);
        visible
            .substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, 0x80);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut visible,
                &rules.crate_rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible,
            "structural bridge chooses empty deck and bypasses ground speed zero"
        );

        let mut non_object_deck = sim_with_grid(0xB21D_0002);
        let terrain_cell = non_object_deck
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        terrain_cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        terrain_cell.speed_costs.track = Some(100);
        non_object_deck
            .substrate
            .raw_cell_occupation
            .mark_deck(cell.0, cell.1, 0x01);
        // The crate Mark ignores neither occupant class (`0x005FD065..
        // 0x005FD070` push EDI = 0 for both), so an infantry bit in the
        // selected deck byte refuses too.
        assert_eq!(
            validate_and_stamp_candidate(
                &mut non_object_deck,
                &rules.crate_rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost,
            "crate Mark passes ignore0/0: any selected deck occupation bit refuses"
        );

        let mut object_deck = sim_with_grid(0xB21D_0003);
        let terrain_cell = object_deck
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        terrain_cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        terrain_cell.speed_costs.track = Some(100);
        object_deck
            .substrate
            .raw_cell_occupation
            .mark_deck(cell.0, cell.1, OBJECT_OCCUPATION_BIT);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut object_deck,
                &rules.crate_rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost,
            "selected deck object bit 0x40 rejects Mark"
        );
    }

    #[test]
    fn crate_placement_steep_slope_uses_exact_raw_b2_exception() {
        let ordinary_rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let ordinary_registry = crate_registry();
        let cell = (12, 12);
        let mut ordinary = sim_with_grid(0xB200_0001);
        ordinary
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .slope_type = 5;
        assert_eq!(
            validate_and_stamp_candidate(
                &mut ordinary,
                &ordinary_rules.crate_rules,
                &ordinary_registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );

        let b2_registry = crate_registry_with_raw_b2();
        assert_eq!(b2_registry.id_for_name("STEEPCRATE"), Some(0xB2));
        let b2_rules = CrateRules {
            wood_crate_img: Some("STEEPCRATE".to_owned()),
            crate_img: Some("STEEPCRATE".to_owned()),
            water_crate_img: Some("STEEPCRATE".to_owned()),
            ..CrateRules::default()
        };
        let mut exempt = sim_with_grid(0xB200_0002);
        exempt
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .slope_type = u8::MAX;
        assert_eq!(
            validate_and_stamp_candidate(
                &mut exempt,
                &b2_rules,
                &b2_registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
    }

    #[test]
    fn place_crate_overlay_preserves_wall_owner_and_publishes_one_dirty_cell() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let mut sim = sim_with_grid(0xD117_0001);
        let cell = (6, 13);
        let owner = sim.interner.intern("Owner");
        let grid = sim.overlay_grid.as_mut().unwrap();
        grid.cell_mut(cell.0, cell.1).wall_owner = Some(owner);
        assert!(grid.take_dirty_cells().is_empty());

        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules.crate_rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let grid = sim.overlay_grid.as_mut().unwrap();
        let written = grid.cell(cell.0, cell.1);
        assert_eq!(written.overlay_id, registry.id_for_name("WOOD"));
        assert_eq!(written.overlay_data, u8::MAX);
        assert_eq!(written.wall_owner, Some(owner));
        assert_eq!(grid.take_dirty_cells(), vec![cell]);
    }

    #[test]
    fn configured_noncrate_images_keep_native_mark_zero_and_land_five_data() {
        // OverlayClass::Mark @ 0x005FD0D9 stores one for Land 5 (Tiberium),
        // not Road; a type without Tiberium=yes keeps it after germination.
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=PLAIN\n1=ROADBOX\n2=TIBBOX\n[PLAIN]\n\
                 [ROADBOX]\nLand=Road\n[TIBBOX]\nLand=Tiberium\n",
            ),
            None,
        );
        for (index, (name, expected_data)) in [("PLAIN", 0), ("ROADBOX", 0), ("TIBBOX", 1)]
            .into_iter()
            .enumerate()
        {
            let rules = CrateRules {
                wood_crate_img: Some(name.to_owned()),
                crate_img: Some(name.to_owned()),
                water_crate_img: Some(name.to_owned()),
                ..CrateRules::default()
            };
            let mut sim = sim_with_grid(0xDA7A_0000 + index as u64);
            let cell = (6, 13);

            assert_eq!(
                validate_and_stamp_candidate(
                    &mut sim,
                    &rules,
                    &registry,
                    cell,
                    ForcedPostPrecheckFailure::None,
                ),
                AcceptedCellResult::Visible
            );
            let written = sim.overlay_grid.as_ref().unwrap().cell(cell.0, cell.1);
            assert_eq!(written.overlay_id, registry.id_for_name(name));
            assert_eq!(written.overlay_data, expected_data, "configured {name}");
        }
    }

    #[test]
    fn crate_mark_publishes_navigation_before_next_reader_and_frame_delivery() {
        use crate::sim::world::TickLane;
        use std::sync::Arc;

        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=ROADBOX\n[ROADBOX]\nLand=Road\n[Road]\nFoot=37%\nTrack=100%\n",
            ),
            None,
        );
        let rules = crate_ruleset_with_images("ROADBOX", "ROADBOX", "ROADBOX", "CrateMinimum=0\n");
        let mut sim = sim_with_grid(0xDA7A_0020);
        sim.hash_state_every(std::num::NonZeroU64::MIN);
        // Mark is invoked below; autonomous crate spawning is outside this fixture.
        sim.session.game_options.crates = false;
        sim.production.ore_growth_config.grows = false;
        sim.production.ore_growth_config.spreads = false;
        assert!(sim.rebuild_dynamic_navigation(&rules));
        let cells = [(12, 13), (6, 13)];
        for cell in cells {
            assert_eq!(
                validate_and_stamp_candidate(
                    &mut sim,
                    &rules.crate_rules,
                    &registry,
                    cell,
                    ForcedPostPrecheckFailure::None,
                ),
                AcceptedCellResult::Visible
            );
            let projected = sim
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .unwrap();
            assert_eq!(projected.land_type, LandType::Road.as_index());
            assert_eq!(projected.speed_costs.foot, Some(37));
        }
        let repeated = sim.overlay_grid.as_mut().unwrap().recalculate_runtime_cell(
            sim.resolved_terrain.as_mut().unwrap(),
            &registry,
            cells[0],
        );
        assert!(!repeated.navigation_changed);
        let mut pending = sim.overlay_grid.as_ref().unwrap().clone();
        assert_eq!(pending.take_synchronous_navigation_cells(), cells);
        assert_eq!(pending.take_dirty_cells(), cells);

        // Even an empty receiver batch executes the production consequence settlement.
        // It must publish pending crate terrain before its next navigation reader.
        sim.commit_noncombat_aoe_receivers(
            &rules,
            Some(&registry),
            &[],
            crate::sim::world::FrameEffects::default(),
        );
        let mut pending = sim.overlay_grid.as_ref().unwrap().clone();
        assert!(pending.take_synchronous_navigation_cells().is_empty());
        assert_eq!(
            pending.take_dirty_cells(),
            cells,
            "next-reader settlement does not consume presentation dirtiness"
        );
        let settled = sim.path_grid_snapshot().unwrap();
        let first = sim
            .advance_app_frame(
                &[],
                Some(&rules),
                Some(&registry),
                67,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        assert_eq!(
            first
                .overlay_updates
                .iter()
                .map(|entry| (entry.rx, entry.ry))
                .collect::<Vec<_>>(),
            cells
        );
        assert_eq!(first.tick.state_hash, Some(sim.state_hash()));
        let published = sim.path_grid_snapshot().unwrap();
        assert!(
            Arc::ptr_eq(&settled, &published),
            "the next-reader settlement already published the Recalc"
        );
        let second = sim
            .advance_app_frame(
                &[],
                Some(&rules),
                Some(&registry),
                67,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        assert!(second.overlay_updates.is_empty());
        assert!(Arc::ptr_eq(&published, &sim.path_grid_snapshot().unwrap()));
        assert_eq!(second.tick.state_hash, Some(sim.state_hash()));
    }

    #[test]
    fn railroad_crate_mark_bypasses_ordinary_blockers_and_forces_data_zero() {
        let registry = dense_registry(0, &[(0, "RAILCRATE", "Land=Railroad\nCrate=yes\n")]);
        let rules = CrateRules {
            wood_crate_img: Some("RAILCRATE".to_owned()),
            crate_img: Some("RAILCRATE".to_owned()),
            water_crate_img: Some("RAILCRATE".to_owned()),
            ..CrateRules::default()
        };
        let mut sim = sim_with_grid(0x14090001);
        let cell = (12, 13);
        sim.substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, 0xFF);
        sim.substrate
            .raw_cell_occupation
            .mark_deck(cell.0, cell.1, 0xFF);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .speed_costs
            .track = Some(0);

        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let written = sim.overlay_grid.as_ref().unwrap().cell(cell.0, cell.1);
        assert_eq!(written.overlay_id, Some(0));
        assert_eq!(written.overlay_data, 0, "Railroad wins over Crate=yes");
    }

    #[test]
    fn wall_crate_mark_uses_building_passability_and_crate_never_overrides_data() {
        let registry = dense_registry(0, &[(0, "WALLCRATE", "Wall=yes\nLand=Wall\nCrate=yes\n")]);
        // The ground's `[Clear]` row decides.
        let rules = crate_ruleset_with_images(
            "WALLCRATE",
            "WALLCRATE",
            "WALLCRATE",
            "[Clear]\nTrack=100%\n",
        );
        let cell = (12, 13);
        let mut visible = sim_with_grid(0x14090002);
        visible
            .substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, 0xFF);
        visible
            .overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(cell.0 + 1, cell.1, 0, 0);

        assert_eq!(
            validate_and_stamp_candidate_with_rules(
                &mut visible,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let grid = visible.overlay_grid.as_ref().unwrap();
        assert_eq!(grid.cell(cell.0, cell.1).overlay_data, 0x02);
        assert_eq!(grid.cell(cell.0 + 1, cell.1).overlay_data, 0x08);

        let mut blocked = sim_with_grid(0x14090003);
        let trackless =
            crate_ruleset_with_images("WALLCRATE", "WALLCRATE", "WALLCRATE", "[Clear]\nTrack=0%\n");
        assert_eq!(
            validate_and_stamp_candidate_with_rules(
                &mut blocked,
                &trackless,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        assert_eq!(
            blocked
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_id,
            None
        );
    }

    #[test]
    fn wall_crate_mark_preserves_cell_metadata_and_publishes_retained_neighbors() {
        let registry = dense_registry(0, &[(0, "WALLCRATE", "Wall=yes\nLand=Wall\n")]);
        let rules = crate_ruleset_with_images(
            "WALLCRATE",
            "WALLCRATE",
            "WALLCRATE",
            "[Clear]\nTrack=100%\n",
        );
        let mut sim = sim_with_grid(0x1409_0004);
        let cell = (12, 13);
        let retained_owner = sim.interner.intern("RetainedOwner");
        let rubble = sim.interner.intern("RetainedRubble");
        {
            let grid = sim.overlay_grid.as_mut().unwrap();
            let target = grid.cell_mut(cell.0, cell.1);
            target.wall_owner = Some(retained_owner);
            target.rubble_building_type = Some(rubble);
            target.overlay_data = u8::MAX;
        }

        let result = sim.with_object_placement_scope(|sim| {
            let result = validate_and_stamp_candidate_with_rules(
                sim,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            );
            assert!(sim.object_placement_scope_active());
            result
        });
        assert_eq!(result, AcceptedCellResult::Visible);
        assert!(!sim.object_placement_scope_active());
        let grid = sim.overlay_grid.as_ref().unwrap();
        let marked = grid.cell(cell.0, cell.1);
        assert_eq!((marked.overlay_id, marked.overlay_data), (Some(0), 0));
        assert_eq!(marked.wall_owner, Some(retained_owner));
        assert_eq!(marked.rubble_building_type, Some(rubble));
        //5FC75A..775 contributes to eight neighboring Cell+122 bytes,
        //not the source; later Foot/wall cleanup consumes this retained plane.
        let counts = grid.retained_neighbor_counts();
        let at = |x: u16, y: u16| counts[usize::from(y) * usize::from(MAP) + usize::from(x)];
        assert_eq!(at(cell.0 - 1, cell.1), 1);
        assert_eq!(at(cell.0 + 1, cell.1 - 1), 1);
        assert_eq!(at(cell.0, cell.1), 0);
        assert_eq!(
            counts
                .iter()
                .map(|&count| usize::from(count))
                .sum::<usize>(),
            8
        );
        assert!(sim.radar_terrain_dirty_cells.contains(&cell));
        assert!(sim.tactical_dirty_cells.contains(&(cell.0 - 1, cell.1)));
    }

    #[test]
    fn wall_crate_mark_clears_and_restores_nested_placement_scope_on_refusal() {
        let registry = dense_registry(0, &[(0, "WALLCRATE", "Wall=yes\nLand=Wall\n")]);
        let rules = crate_ruleset_with_images(
            "WALLCRATE",
            "WALLCRATE",
            "WALLCRATE",
            "[Clear]\nBuildable=yes\nTrack=0%\n",
        );
        let mut sim = sim_with_grid(0x1409_0005);
        let cell = (12, 13);
        sim.with_object_placement_scope(|sim| {
            sim.with_object_placement_scope(|sim| {
                assert_eq!(
                    validate_and_stamp_candidate_with_rules(
                        sim,
                        &rules,
                        &registry,
                        cell,
                        ForcedPostPrecheckFailure::None,
                    ),
                    AcceptedCellResult::Ghost,
                    "4A190B's cleared scope must not bypass Track admission"
                );
                assert!(sim.object_placement_scope_active());
            });
            assert!(sim.object_placement_scope_active());
        });
        assert!(!sim.object_placement_scope_active());
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_id,
            None
        );
    }

    #[test]
    fn land_tiberium_crate_mark_germinates_from_same_type_neighbors() {
        // OverlayClass::Mark @ 0x005FD0D9 germinates for Land 5 (Tiberium)
        // only; a Land=Road ore type keeps the ordinary zero.
        for (land, expected) in [("Tiberium", 6), ("Road", 0)] {
            let ini = IniFile::from_str(&format!(
                "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
                 [OverlayTypes]\n0=LANDORE\n\
                 [LANDORE]\nTiberium=yes\nLand={land}\n\
                 [Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\n\
                 [CrateRules]\nCrateImg=LANDORE\nWoodCrateImg=LANDORE\nWaterCrateImg=LANDORE\n",
            ));
            let registry = OverlayTypeRegistry::from_ini(&ini, None);
            let rules = RuleSet::from_ini(&ini).unwrap();
            let mut sim = sim_with_grid(0x14090004);
            let cell = (12, 13);
            for neighbor in [(11, 12), (12, 12), (13, 13), (12, 14)] {
                sim.overlay_grid
                    .as_mut()
                    .unwrap()
                    .place_overlay(neighbor.0, neighbor.1, 0, 1);
            }

            assert_eq!(
                validate_and_stamp_candidate_with_rules(
                    &mut sim,
                    &rules,
                    &registry,
                    cell,
                    ForcedPostPrecheckFailure::None,
                ),
                AcceptedCellResult::Visible
            );
            assert_eq!(
                sim.overlay_grid
                    .as_ref()
                    .unwrap()
                    .cell(cell.0, cell.1)
                    .overlay_data,
                expected,
                "Land={land}: four matching neighbors index the retail [0,1,3,4,6,...] table"
            );
        }
    }

    fn cell_anim_crate_rules_and_registry() -> (RuleSet, OverlayTypeRegistry) {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [Colors]\nGold=43,239,255\nNeonGreen=104,241,195\n\
             [Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\nColor=NeonGreen\n\
             [Animations]\n0=SPARK\n\
             [OverlayTypes]\n0=ANIMBOX\n[ANIMBOX]\nTiberium=yes\nCellAnim=SPARK\n\
             [CrateRules]\nCrateImg=ANIMBOX\nWoodCrateImg=ANIMBOX\nWaterCrateImg=ANIMBOX\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
            "[SPARK]\nRate=1\nEnd=2\nLoopCount=1\n",
        ));
        art.bind_anim_frame_count_for_test("SPARK", 2);
        rules.replace_art_registry_for_test(art);
        (rules, registry)
    }

    #[test]
    fn restored_crate_regeneration_uses_incoming_normal_lighting() {
        use crate::sim::scenario_session::{ScenarioLightingProfile, ScenarioLightingState};
        use crate::sim::snapshot::GameSnapshot;
        use crate::sim::world::SnapshotProcessState;

        let (rules, registry) = cell_anim_crate_rules_and_registry();
        let profiles = crate::map::lighting::parse_lighting_profiles(&IniFile::from_str(
            "[Lighting]\nAmbient=0.80\nRed=0.91\nGreen=0.72\nBlue=0.63\n\
             Ground=0.025\nLevel=0.005\nIonAmbient=1.20\nIonGround=0.040\n",
        ));
        let mut incoming = sim_with_grid(0x14_09_0007);
        incoming.session.game_mode_nonzero = true;
        incoming.session.binary_frame = 100;
        incoming.session.lighting = ScenarioLightingState::from_map(&profiles);
        incoming.session.lighting.selected_profile = ScenarioLightingProfile::Ion;
        incoming.session.lighting.current_ambient = profiles.ion.ambient_percent;
        incoming.session.lighting.target_ambient = profiles.ion.ambient_percent;
        let terrain = incoming.resolved_terrain.take().unwrap();
        incoming.install_resolved_terrain_for_new_map(terrain.clone());
        incoming
            .overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(12, 13, 0, 1);
        *incoming.crate_authority.slot_mut(0) = state::CrateSlot {
            start_frame: 0,
            aux: 0,
            duration: 0,
            cell_x: 12,
            cell_y: 13,
        };
        let bytes = GameSnapshot::save(&incoming, 0, 0, "incoming-lighting.map", 0);
        let mut outgoing = sim_with_grid(0x14_09_0008);
        outgoing.session.lighting.normal.ambient_percent = 150;
        outgoing.session.lighting.normal.ground_units = 50;
        let cold_main = crate::sim::rng::MainRng::new(0x14_09_0009);

        for in_game in [false, true] {
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            restored.retain_process_state_for_load(if in_game {
                SnapshotProcessState::Live(&outgoing)
            } else {
                SnapshotProcessState::Cold(&cold_main)
            });
            restored.restore_after_snapshot_load().unwrap();
            restored.rebuild_caches_after_load(terrain.clone(), Default::default(), &rules);
            restored
                .restore_map_authority_after_snapshot_load(&rules, &registry, None)
                .unwrap();
            assert_eq!(restored.session.lighting.normal, profiles.normal.into());
            assert_eq!(
                restored.session.lighting.selected_profile,
                ScenarioLightingProfile::Ion
            );

            restored.advance_tick(&[], Some(&rules), None, Some(&registry), 67);

            let anims: Vec<_> = restored
                .substrate
                .anims
                .iter()
                .map(|(_, anim)| anim)
                .collect();
            assert_eq!(
                anims.len(),
                1,
                "the expired slot must construct its replacement CellAnim"
            );
            assert_eq!(
                anims[0].z_adjust, 775,
                "cold/in-game restore must use incoming ordinary Cell10A, not defaults, outgoing, or selected Ion lighting"
            );
        }
    }

    #[test]
    fn ordinary_cell_anim_spawns_after_visible_or_failed_ordinary_mark() {
        let (rules, registry) = cell_anim_crate_rules_and_registry();
        let cell = (12, 13);

        let mut visible = sim_with_grid(0x14090005);
        let lighting = LightingProfileUnits {
            ambient_percent: 80,
            red_percent: 100,
            green_percent: 100,
            blue_percent: 100,
            ground_units: 25,
            level_units: 5,
        };
        assert_eq!(
            validate_and_stamp_candidate_with_rules_and_lighting(
                &mut visible,
                &rules,
                &registry,
                cell,
                lighting,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let visible_anim = visible.substrate.anims.iter().next().unwrap().1;
        assert_eq!(
            visible_anim.world_coord,
            crate::sim::anim_class::AnimWorldCoord {
                x: i32::from(cell.0) * 256 + 128 + 0x180,
                y: i32::from(cell.1) * 256 + 128 + 0x180,
                z: 0,
            }
        );
        assert_eq!(visible_anim.draw_flags, 0x600);
        assert_eq!(
            visible_anim.remap(),
            Some(crate::sim::anim_class::AnimRemap::ColorScheme(
                crate::rules::house_colors::HouseColorIndex(1)
            )),
            "the constructed CellAnim uses the live tiberium Color scheme"
        );
        assert_eq!(
            visible_anim.z_adjust, 775,
            "CellClass +0x10A is ambient*10 + level*height - ground"
        );
        let encoded =
            crate::util::save_codec::serialize(&visible).expect("serialize CellAnim remap state");
        let restored: Simulation =
            crate::util::save_codec::deserialize(&encoded).expect("restore CellAnim remap state");
        let restored_anim = restored.substrate.anims.iter().next().unwrap().1;
        assert_eq!(restored_anim.remap(), visible_anim.remap());
        assert_eq!(restored_anim.z_adjust, visible_anim.z_adjust);

        let mut ghost = sim_with_grid(0x14090006);
        ghost
            .substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, OBJECT_OCCUPATION_BIT);
        assert_eq!(
            validate_and_stamp_candidate_with_rules(
                &mut ghost,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        let ghost_anim = ghost.substrate.anims.iter().next().unwrap().1;
        assert_eq!(ghost_anim.remap(), None);
        assert_eq!(
            ghost_anim.z_adjust, 0,
            "failed Mark leaves the Cell without tiberium, so native skips both post-writes"
        );
        assert_eq!(
            ghost
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_id,
            None
        );

        let mut terrain_object = sim_with_grid(0x14090007);
        terrain_object
            .production
            .terrain_object_cells
            .insert(cell, 99);
        assert_eq!(
            validate_and_stamp_candidate_with_rules(
                &mut terrain_object,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        assert_eq!(terrain_object.substrate.anims.iter().count(), 0);
        assert_eq!(
            terrain_object
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_id,
            None
        );
    }

    #[test]
    fn high_bridge_crate_mark_writes_setter_data_then_falls_through_crate_override() {
        let registry = dense_registry(0x18, &[(0x18, "HIGHCRATE", "Crate=yes\n")]);
        let rules = CrateRules {
            wood_crate_img: Some("HIGHCRATE".to_owned()),
            crate_img: Some("HIGHCRATE".to_owned()),
            water_crate_img: Some("HIGHCRATE".to_owned()),
            ..CrateRules::default()
        };
        let cell = (12, 13);
        let prefilled = |seed| {
            let mut sim = sim_with_grid(seed);
            for target in [(12, 13), (12, 12), (12, 11), (12, 10), (12, 14)] {
                write_crate_mark_raw_data(&mut sim, (target.0 as i16, target.1 as i16), 7);
            }
            let _ = sim.overlay_grid.as_mut().unwrap().take_dirty_cells();
            sim
        };

        // The crate-image comparisons precede the high-bridge exemption
        // (0x005FCFF9..0x005FD035): a high bridge configured as the water
        // crate image asks CheckCellPassability with Float and no ignores,
        // after its setter already stored the span's data.
        let mut blocked = prefilled(0x14090008);
        blocked
            .substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, 0xFF);
        blocked
            .substrate
            .raw_cell_occupation
            .mark_deck(cell.0, cell.1, 0xFF);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut blocked,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        let grid = blocked.overlay_grid.as_ref().unwrap();
        assert_eq!(grid.cell(12, 13).overlay_id, None);
        for target in [(12, 13), (12, 12), (12, 11), (12, 14)] {
            assert_eq!(grid.cell(target.0, target.1).overlay_data, 0, "{target:?}");
        }

        let mut sim = prefilled(0x14090008);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let grid = sim.overlay_grid.as_ref().unwrap();
        assert_eq!(
            (grid.cell(12, 13).overlay_id, grid.cell(12, 13).overlay_data),
            (Some(0x18), u8::MAX),
            "Crate=yes runs after the direction-0 setter writes zero"
        );
        for target in [(12, 12), (12, 11), (12, 14)] {
            assert_eq!(grid.cell(target.0, target.1).overlay_data, 0, "{target:?}");
        }
        assert_eq!(grid.cell(12, 10).overlay_data, 7, "F3 gets flags, not data");

        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let anchor = terrain.cell(12, 13).unwrap().bridge_facts;
        assert!(anchor.is_anchor_self());
        assert!(anchor.has_structural_bridge());
        assert_eq!(anchor.state_byte, u8::MAX);
        for target in [(12, 12), (12, 11), (12, 14)] {
            assert_eq!(
                terrain
                    .cell(target.0, target.1)
                    .unwrap()
                    .bridge_facts
                    .state_byte,
                0,
                "derived state follows raw setter data at {target:?}"
            );
        }
        assert_eq!(terrain.cell(12, 10).unwrap().bridge_facts.state_byte, 7);

        let mut terrain_blocked = sim_with_grid(0x14090009);
        write_crate_mark_raw_data(&mut terrain_blocked, (12, 13), 7);
        let _ = terrain_blocked
            .overlay_grid
            .as_mut()
            .unwrap()
            .take_dirty_cells();
        terrain_blocked
            .production
            .terrain_object_cells
            .insert(cell, 100);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut terrain_blocked,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        assert_eq!(
            terrain_blocked
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_data,
            7,
            "TerrainClass constructor gate runs before the high setter"
        );
        assert_eq!(
            terrain_blocked
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .unwrap()
                .bridge_facts
                .raw_flags
                & BRIDGE_FLAG_STRUCTURAL,
            0
        );
        assert!(
            terrain_blocked
                .overlay_grid
                .as_ref()
                .unwrap()
                .pending_dirty_cells()
                .is_empty()
        );
    }

    #[test]
    fn direction_six_high_bridge_setter_data_precedes_land_five_override() {
        // Mark keeps the setter's nine for a high bridge (0x005FD0B4..0x005FD0CC)
        // unless Land 5 (0x005FD0D9) then stores one.
        for (land, anchor_data) in [("Road", 9), ("Tiberium", 1)] {
            let section = format!("Land={land}\n");
            let registry = dense_registry(0x19, &[(0x19, "HIGHLAND", section.as_str())]);
            let rules = CrateRules {
                wood_crate_img: Some("HIGHLAND".to_owned()),
                crate_img: Some("HIGHLAND".to_owned()),
                water_crate_img: Some("HIGHLAND".to_owned()),
                ..CrateRules::default()
            };
            let cell = (12, 13);
            let mut sim = sim_with_grid(0x1409000A);

            assert_eq!(
                validate_and_stamp_candidate(
                    &mut sim,
                    &rules,
                    &registry,
                    cell,
                    ForcedPostPrecheckFailure::None,
                ),
                AcceptedCellResult::Visible
            );
            let grid = sim.overlay_grid.as_ref().unwrap();
            assert_eq!(
                (grid.cell(12, 13).overlay_id, grid.cell(12, 13).overlay_data),
                (Some(0x19), anchor_data),
                "Land={land} after the direction-6 setter writes nine"
            );
            for target in [(11, 13), (10, 13), (13, 13)] {
                assert_eq!(grid.cell(target.0, target.1).overlay_data, 9, "{target:?}");
                assert_eq!(
                    sim.resolved_terrain
                        .as_ref()
                        .unwrap()
                        .cell(target.0, target.1)
                        .unwrap()
                        .bridge_facts
                        .state_byte,
                    9,
                    "derived state follows raw setter data at {target:?}"
                );
            }
        }
    }

    #[test]
    fn low_bridge_crate_mark_uses_exact_table_raw_draws_and_success_noop() {
        let registry = dense_registry(0x7A, &[(0x7A, "LOWTRIGGER", "Land=Road\n")]);
        let rules = CrateRules {
            wood_crate_img: Some("LOWTRIGGER".to_owned()),
            crate_img: Some("LOWTRIGGER".to_owned()),
            water_crate_img: Some("LOWTRIGGER".to_owned()),
            ..CrateRules::default()
        };
        let origin = (20, 20);
        let mut sim = sim_with_grid(0x14090008);
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(15, 20, 0x5E, 1);
        let mut replay = sim.scenario_rng.clone();
        let body_draws: Vec<u8> = (0..12).map(|_| (replay.next_u32() & 3) as u8).collect();

        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                origin,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
        let grid = sim.overlay_grid.as_ref().unwrap();
        assert_eq!(
            (grid.cell(20, 20).overlay_id, grid.cell(20, 20).overlay_data),
            (Some(0x5C), 1)
        );
        assert_eq!(grid.cell(16, 19).overlay_id, Some(0x4A + body_draws[0]));
        assert_eq!(grid.cell(16, 19).overlay_data, 0);
        assert_eq!(grid.cell(19, 21).overlay_id, Some(0x4A + body_draws[11]));
        assert_eq!(grid.cell(19, 21).overlay_data, 2);
        let origin_terrain = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(origin.0, origin.1)
            .unwrap();
        assert!(origin_terrain.has_bridge_deck);
        assert!(!origin_terrain.bridge_walkable);
        assert_eq!(
            origin_terrain
                .bridge_layer
                .as_ref()
                .map(|layer| layer.direction),
            Some(crate::map::resolved_terrain::BridgeDirection::Low)
        );

        let mut occupied = sim_with_grid(0x14090009);
        occupied
            .overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(20, 19, 0, 0);
        let before = occupied.scenario_rng.state();
        assert_eq!(
            validate_and_stamp_candidate(
                &mut occupied,
                &rules,
                &registry,
                origin,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        assert_eq!(occupied.scenario_rng.state(), before);
        assert_eq!(
            occupied
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(origin.0, origin.1)
                .overlay_id,
            None
        );
    }

    #[test]
    fn low_bridge_crate_mark_preserves_shared_dummy_overlay_alias() {
        let registry = dense_registry(0x7C, &[(0x7C, "EDGELOW", "Land=Road\n")]);
        let rules = CrateRules {
            wood_crate_img: Some("EDGELOW".to_owned()),
            crate_img: Some("EDGELOW".to_owned()),
            water_crate_img: Some("EDGELOW".to_owned()),
            ..CrateRules::default()
        };
        let mut sim = sim_with_grid(0x14090012);
        let before = sim.scenario_rng.state();

        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                (0, 1),
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let dummy = sim.effective_shared_cell_dummy();
        assert_eq!(dummy.overlay_fields(), (Some(0x60), 0));
        assert_eq!(dummy.snapshot().coord, (-1, 1));

        // A different missing coordinate aliases that same native fallback
        // cell. Its retained overlay makes the later three-probe row occupied,
        // so the real trigger cell remains an accepted timed ghost.
        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                (0, 2),
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        assert_eq!(dummy.overlay_fields(), (Some(0x60), 0));
        assert_eq!(dummy.snapshot().coord, (-1, 2));
        assert_eq!(sim.scenario_rng.state(), before);
    }

    #[test]
    fn ts_legacy_veins_crate_ids_are_explicit_accepted_ghosts() {
        for (id, name) in [(0x7E, "VEINS"), (0xA7, "VEINHOLE")] {
            let registry = dense_registry(id, &[(id, name, "Crate=yes\n")]);
            let rules = CrateRules {
                wood_crate_img: Some(name.to_owned()),
                crate_img: Some(name.to_owned()),
                water_crate_img: Some(name.to_owned()),
                ..CrateRules::default()
            };
            let mut sim = sim_with_grid(0x14090010 + u64::from(id));
            let before = sim.scenario_rng.state();
            assert_eq!(
                validate_and_stamp_candidate(
                    &mut sim,
                    &rules,
                    &registry,
                    (12, 13),
                    ForcedPostPrecheckFailure::None,
                ),
                AcceptedCellResult::Ghost
            );
            assert_eq!(sim.scenario_rng.state(), before);
            assert_eq!(
                sim.overlay_grid.as_ref().unwrap().cell(12, 13).overlay_id,
                None
            );
        }
    }

    #[test]
    fn crate_random_rectangle_draws_signed_reversed_ranges_then_narrows_to_i16() {
        let mut sum_one = crate::sim::rng::SimRng::new(0x1401);
        let mut sum_one_expected = sum_one.clone();
        let sum_one_frame = CrateRandomFrame {
            left: 1,
            top: 1,
            width: 0,
            height: 0,
        };
        let sum_one_cell = draw_crate_candidate(&mut sum_one, sum_one_frame);
        let expected_sum_one = (
            sum_one_expected.next_range_u32_inclusive(0, 1) as i32,
            sum_one_expected.next_range_u32_inclusive(0, 1) as i32,
        );
        assert_eq!(sum_one_cell, expected_sum_one);
        assert_eq!(sum_one.state(), sum_one_expected.state());

        let mut negative = crate::sim::rng::SimRng::new(0x1402);
        let mut negative_expected = negative.clone();
        let negative_frame = CrateRandomFrame {
            left: i32::MAX,
            top: i32::MIN,
            width: -2,
            height: -2,
        };
        let negative_cell = draw_crate_candidate(&mut negative, negative_frame);
        let expected_negative = (
            i32::MAX.wrapping_add(-3 + negative_expected.next_range_u32_inclusive(0, 3) as i32)
                as i16 as i32,
            i32::MIN.wrapping_add(-3 + negative_expected.next_range_u32_inclusive(0, 3) as i32)
                as i16 as i32,
        );
        assert_eq!(negative_cell, expected_negative);
        assert_eq!(negative.state(), negative_expected.state());

        let mut wide = crate::sim::rng::SimRng::new(0x1403);
        let mut wide_expected = wide.clone();
        let wide_frame = CrateRandomFrame {
            left: 30_000,
            top: -30_000,
            width: 70_000,
            height: 70_000,
        };
        let wide_cell = draw_crate_candidate(&mut wide, wide_frame);
        let expected_wide = (
            30_000i32.wrapping_add(wide_expected.next_range_u32_inclusive(0, 69_999) as i32) as i16
                as i32,
            (-30_000i32).wrapping_add(wide_expected.next_range_u32_inclusive(0, 69_999) as i32)
                as i16 as i32,
        );
        assert_eq!(wide_cell, expected_wide);
        assert_eq!(wide.state(), wide_expected.state());
    }

    #[test]
    fn crate_random_frame_retains_nonpositive_signed_size_for_rng_before_empty_snap() {
        let mut sim = sim_with_grid(1);
        sim.playfield_bounds.as_mut().unwrap().base = 1;
        sim.playfield_size_height = Some(0);
        assert_eq!(
            crate_random_frame(&sim),
            Some(CrateRandomFrame {
                left: 1,
                top: 1,
                width: 0,
                height: 0,
            })
        );

        sim.playfield_bounds.as_mut().unwrap().base = -3;
        sim.playfield_size_height = Some(-2);
        assert_eq!(
            crate_random_frame(&sim),
            Some(CrateRandomFrame {
                left: 1,
                top: 1,
                width: -6,
                height: -6,
            })
        );
    }

    /// With a uniform surface, destination image selection follows the
    /// native LandType byte.
    #[test]
    fn gsi_01_04_crate_landtype_two_uses_water_otherwise_wood() {
        let rules = crate_ruleset("CrateMinimum=4\nCrateMaximum=255\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        for water in [false, true] {
            let mut sim = sim_with_grid(0x5150);
            sim.install_fixture_path_grid(Some(&grid));
            sim.resolved_terrain = Some(uniform_terrain(water));

            let result = place_scenario_start_crates(&mut sim, &rules, &registry, 4);
            assert_eq!((result.accepted, result.visible), (4, 4));

            let silver_crates = cells_with_overlay(&sim, &registry, "SILVER");
            let land_crates = cells_with_overlay(&sim, &registry, "WOOD");
            let water_crates = cells_with_overlay(&sim, &registry, "WATER");
            assert!(silver_crates.is_empty(), "startup never uses CrateImg");
            if water {
                assert_eq!(water_crates.len(), 4, "water draws take WaterCrateImg");
                assert!(land_crates.is_empty());
            } else {
                assert_eq!(land_crates.len(), 4, "land draws take WoodCrateImg");
                assert!(water_crates.is_empty());
            }
        }
    }

    #[test]
    fn gsi_01_04_crate_mixed_surface_uses_drawn_movement_and_destination_image() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        for drawn_is_water in [true, false] {
            let mut sim = sim_with_grid(0x1234_5678);
            sim.install_fixture_path_grid(Some(&grid));
            let mut replay = sim.scenario_rng.clone();
            let drawn = (
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            );
            assert_eq!(drawn, (14, 13));

            let destination = (15, 13);
            let wrong_movement_decoy = (13, 12);
            let mut terrain = uniform_terrain(false);
            for cell in &mut terrain.cells {
                cell.speed_costs.float = Some(0);
                cell.speed_costs.track = Some(0);
            }
            let drawn_cell = terrain.cell_mut(drawn.0, drawn.1).expect("drawn cell");
            drawn_cell.land_type = if drawn_is_water {
                LandType::Water.as_index()
            } else {
                LandType::Clear.as_index()
            };

            // The real destination admits the drawn-cell FNPC movement and the
            // independently selected image's Mark movement. The nearer-to-(0,0)
            // decoy admits only the opposite FNPC type, so choosing the search
            // movement from anywhere but the draw lands on the decoy.
            let destination_cell = terrain
                .cell_mut(destination.0, destination.1)
                .expect("destination cell");
            destination_cell.speed_costs.float = Some(100);
            destination_cell.speed_costs.track = Some(100);
            destination_cell.land_type = if drawn_is_water {
                LandType::Clear.as_index()
            } else {
                LandType::Water.as_index()
            };
            let decoy = terrain
                .cell_mut(wrong_movement_decoy.0, wrong_movement_decoy.1)
                .expect("wrong-movement decoy");
            if drawn_is_water {
                decoy.speed_costs.track = Some(100);
            } else {
                decoy.speed_costs.float = Some(100);
            }
            sim.resolved_terrain = Some(terrain);

            let result = place_scenario_start_crates(&mut sim, &rules, &registry, 1);

            assert_eq!((result.accepted, result.visible), (1, 1));
            let expected_image = if drawn_is_water { "WOOD" } else { "WATER" };
            let drawn_image = if drawn_is_water { "WATER" } else { "WOOD" };
            assert_eq!(
                cells_with_overlay(&sim, &registry, expected_image),
                vec![destination],
                "image must follow the snapped destination surface"
            );
            assert!(
                cells_with_overlay(&sim, &registry, drawn_image).is_empty(),
                "image must not follow the drawn surface"
            );
            assert!(
                sim.overlay_grid
                    .as_ref()
                    .expect("overlay grid")
                    .cell(wrong_movement_decoy.0, wrong_movement_decoy.1)
                    .overlay_id
                    .is_none(),
                "movement type must follow the drawn surface"
            );
        }
    }

    #[test]
    fn gsi_01_04_crate_snap_zero_reference_uses_live_frame_modulo() {
        let registry = crate_registry();
        let mut sim = sim_with_grid(1);
        let mut terrain = uniform_terrain(false);
        for cell in &mut terrain.cells {
            cell.speed_costs.track = Some(0);
        }
        // Both preferred candidates are on ring 9. Engine ring order encounters
        // (29,11) first and (11,20) second, while nearest-to-origin would choose
        // (11,20). Native's zero reference is a sentinel, not a real target.
        terrain
            .cell_mut(29, 11)
            .expect("candidate A")
            .speed_costs
            .track = Some(100);
        let selected = terrain.cell_mut(11, 20).expect("candidate B");
        selected.speed_costs.track = Some(100);
        selected.bridge_facts.raw_flags |= BRIDGE_FLAG_STRUCTURAL;
        sim.resolved_terrain = Some(terrain);

        // An ordinary overlay would fail occupancy-rect checking, but this
        // caller passes occupancy_rect_check=false. Crate placement itself
        // performs its separate no-existing-overlay check after FNPC.
        let ore = registry.id_for_name("TIB01").expect("overlay type");
        sim.overlay_grid
            .as_mut()
            .expect("overlay grid")
            .place_overlay(11, 20, ore, 0);
        let grid = PathGrid::test_all_passable(MAP, MAP);
        sim.install_fixture_path_grid(Some(&grid));

        sim.session.binary_frame = 0;
        assert_eq!(
            snap_to_passable(&sim, (20, 20), CrateSurface::Land),
            Some((29, 11)),
            "frame zero selects the first preferred survivor, not nearest to (0,0)"
        );

        sim.session.binary_frame = 1;
        assert_eq!(
            snap_to_passable(&sim, (20, 20), CrateSurface::Land),
            Some((11, 20)),
            "the live frame advances modulo the preferred pool"
        );
    }

    #[test]
    fn gsi_01_04_crate_snap_requires_native_candidate_anchor_diamond() {
        let mut sim = sim_with_grid(1);
        let mut terrain = uniform_terrain(false);
        for cell in &mut terrain.cells {
            cell.speed_costs.track = Some(0);
        }
        // Both candidates are valid cells inside the 40x40 terrain rectangle.
        // Ring order visits (8,3) first, but the native diamond below admits
        // (8,5) only: on flat terrain it requires 12 < x+y <= 26,
        // x-y < 14, and y-x < 6.
        terrain
            .cell_mut(8, 3)
            .expect("rectangular-only candidate")
            .speed_costs
            .track = Some(100);
        terrain
            .cell_mut(8, 5)
            .expect("in-diamond candidate")
            .speed_costs
            .track = Some(100);
        sim.resolved_terrain = Some(terrain);
        sim.playfield_bounds = Some(PlayfieldBounds {
            base: 10,
            off_fc: 2,
            off_100: 1,
            off_104: 10,
            off_108: 6,
        });
        sim.session.binary_frame = 0;
        let grid = PathGrid::test_all_passable(MAP, MAP);
        sim.install_fixture_path_grid(Some(&grid));

        assert_eq!(
            snap_to_passable(&sim, (9, 4), CrateSurface::Land),
            Some((8, 5)),
            "the earlier rectangularly valid but off-diamond anchor must be rejected"
        );

        sim.playfield_bounds = None;
        assert_eq!(
            snap_to_passable(&sim, (9, 4), CrateSurface::Land),
            None,
            "missing MapClass playfield authority must reject, not bypass"
        );
    }

    /// Every cell the same land type, every speed type passable, so the only
    /// thing the water flag can change is which crate image is chosen. Its
    /// Clear and Water land rows are [`OPEN_LAND_KEYS`], bound as a loaded map
    /// binds its rules.
    fn uniform_terrain(water: bool) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
        use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile};

        let speed_costs = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            float: Some(100),
            amphibious: Some(100),
            float_beach: Some(100),
            hover: Some(100),
        };
        let land_type = if water {
            LandType::Water.as_index()
        } else {
            LandType::Clear.as_index()
        };
        let template = ResolvedTerrainCell {
            land_type,
            speed_costs,
            allows_tiberium: true,
            zone_type: zone_class::GROUND,
            base_land_type: land_type,
            base_speed_costs: speed_costs,
            ..crate::map::resolved_terrain::test_flat_cell(0, 0)
        };
        let mut cells = Vec::with_capacity(MAP as usize * MAP as usize);
        for ry in 0..MAP {
            for rx in 0..MAP {
                let mut cell = template.clone();
                cell.rx = rx;
                cell.ry = ry;
                cells.push(cell);
            }
        }
        let mut grid = ResolvedTerrainGrid::from_cells(MAP, MAP, cells);
        grid.set_land_speed_rules_for_test(&crate::rules::terrain_rules::TerrainRules::from_ini(
            &IniFile::from_str(&format!(
                "[Clear]\n{OPEN_LAND_KEYS}[Water]\n{OPEN_LAND_KEYS}"
            )),
        ));
        grid
    }

    /// The count the loader feeds the clamp: human seats only. A 1v3 skirmish
    /// asks for one crate, not four.
    #[test]
    fn gsi_01_04_crate_one_human_and_seven_ai_requests_one() {
        use crate::sim::house_state::HouseState;

        let mut sim = sim_with_grid(0x1A17_0001);
        let mut add = |name: &str, is_human: bool, passive: bool| {
            let id = sim.interner.intern(name);
            let mut house = HouseState::new(id, 0, None, is_human, 10_000, 10);
            house.multiplay_passive = passive;
            sim.houses.insert(id, house);
        };
        add("Local", true, false);
        for index in 1..=7 {
            add(&format!("Computer{index}"), false, false);
        }
        add("Neutral", false, true);
        add("Special", false, true);

        let player_count = human_player_count(&sim);
        assert_eq!(player_count, 1);
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=255\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        sim.install_fixture_path_grid(Some(&grid));
        let result = place_scenario_start_crates(&mut sim, &rules, &registry, player_count);
        assert_eq!(result.requested, 1);
        assert_eq!((result.accepted, result.visible), (1, 1));
    }

    #[test]
    fn scenario_start_crate_draws_exact_active_rectangle_not_session_extent_or_localsize() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x2468);
        sim.install_fixture_path_grid(Some(&grid));
        sim.playfield_bounds.as_mut().unwrap().base = 14;
        sim.playfield_size_height = Some(9);
        sim.session.map_width = 37;
        let mut replay = sim.scenario_rng.clone();
        let expected = (
            1 + replay.next_range_u32_inclusive(0, 21) as u16,
            1 + replay.next_range_u32_inclusive(0, 21) as u16,
        );
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, 1);

        assert_eq!((result.accepted, result.visible), (1, 1));
        assert_eq!(crate_cells(&sim, &registry), vec![expected]);
        assert!((1..=22).contains(&expected.0) && (1..=22).contains(&expected.1));
        assert_ne!(expected, (0, 0), "draw lies outside the 1x1 LocalSize");
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }
}
