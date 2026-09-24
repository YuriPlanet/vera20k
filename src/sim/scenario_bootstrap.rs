//! Simulation-owned offline-skirmish world bootstrap.
//!
//! Active-stock offline startup resolves both House passes and both selected-mode
//! start callbacks before terrain Fill, then carries the same Scenario RNG cursor
//! into the live world. Final House projection, opening forces, shroud, AI
//! credits, and alliances remain behind the same simulation authority boundary.

use std::collections::{BTreeMap, HashMap};

use crate::map::entities::EntityCategory;
use crate::map::houses::HouseRoster;
use crate::map::map_file::{MapFile, MapHeader};
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::waypoints::Waypoint;
use crate::rng_continuation::MapGenRngContinuation;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::sim::ai::AiPlayerState;
#[cfg(test)]
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::find_nearby_cell::{
    NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
    map_owned_radius_cap,
};
use crate::sim::house_state::{HouseDifficulty, HouseState, determine_waypoint_edge};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::rng::{SimRng, SimRngLogicalState};
use crate::sim::scenario_session::ScenarioDescriptor;
use crate::sim::world::{PlacementEvidence, Simulation};
use crate::skirmish_launch::{
    LaunchCountry, LaunchStartPosition, LaunchTeam, PreFillHouseRoster, SkirmishLaunchSession,
};
use crate::util::native_x87::{NativeF64Bits, X87Chop53, sqrt_approx_f32};

#[derive(Debug, Clone, Copy)]
pub(crate) struct NativeStartBounds {
    pub(crate) min_rx: u16,
    pub(crate) min_ry: u16,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

impl NativeStartBounds {
    /// Construct the post-Resize cell-array rectangle directly from parsed
    /// `[Map] Size`, before Fill has produced a resolved terrain grid.
    pub(crate) fn from_map_header(header: &MapHeader) -> Option<Self> {
        if header.width == 0 || header.height == 0 {
            return None;
        }
        let extent = header.width.checked_add(header.height)?;
        if extent > 512 {
            return None;
        }
        let extent = u16::try_from(extent).ok()?;
        (extent > 1).then_some(Self {
            min_rx: 1,
            min_ry: 1,
            width: extent - 1,
            height: extent - 1,
        })
    }

    /// gamemd-derived: active YR start placement is bounded by the MapClass
    /// CELL-ARRAY rect, never by `LocalSize=`. `MapClass::Resize @ 0x00565C10` writes
    /// `MapClass+0x124..0x130` as `(1, 1, SizeW+SizeH-1, SizeW+SizeH-1)`, and
    /// both `ScenarioClass__Gather_Start_Positions @ 0x00688380` (deficient
    /// seed block `0x00688528..0x0068857C`) and
    /// `Try_Unlimbo_Object_At_Or_Near_Cell @ 0x00688ED0` (fallback probe clamp)
    /// read exactly those four fields. Playable-area
    /// acceptance is the separate isometric-diamond predicate
    /// (`cell_rect::cell_is_in_playfield_height_aware`); `LocalSize=` feeds that test's
    /// band constants only, never an axis-aligned bound. Treating LocalSize
    /// as a cell rectangle here previously rejected most authored starts and
    /// clamped every displaced MCV onto the false edge.
    pub(crate) fn from_session(sim: &Simulation, terrain: &ResolvedTerrainGrid) -> Self {
        // `session.map_width/height` hold the canonical cell-array extent
        // (~SizeW+SizeH), so the native rect side is that extent minus one.
        // Sessions without header data (headless fixtures) fall back to the
        // terrain grid extent — VERA-internal, same shape.
        let (extent_x, extent_y) = if sim.session.map_width != 0 && sim.session.map_height != 0 {
            (sim.session.map_width, sim.session.map_height)
        } else {
            (terrain.width(), terrain.height())
        };
        Self {
            min_rx: 1,
            min_ry: 1,
            width: extent_x.saturating_sub(1),
            height: extent_y.saturating_sub(1),
        }
    }

    fn max_rx(self) -> u16 {
        self.min_rx.saturating_add(self.width.saturating_sub(1))
    }

    fn max_ry(self) -> u16 {
        self.min_ry.saturating_add(self.height.saturating_sub(1))
    }

    pub(crate) fn clamp(self, rx: i32, ry: i32) -> (u16, u16) {
        (
            rx.clamp(i32::from(self.min_rx), i32::from(self.max_rx())) as u16,
            ry.clamp(i32::from(self.min_ry), i32::from(self.max_ry())) as u16,
        )
    }
}

/// Build the native multiplayer-start vector before assigning houses.
///
/// Only waypoint indices below the computed target count are examined. When
/// authored starts are deficient, each retry consumes the two asymmetric
/// ranged draws and runs the 8x8 nearby-passable search; invalid searches do
/// not advance the vector and have no artificial retry cap.
#[cfg(test)]
pub(crate) fn native_gather_start_positions(
    waypoints: &HashMap<u32, Waypoint>,
    participant_count: usize,
    terrain: &ResolvedTerrainGrid,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    bounds: NativeStartBounds,
    playfield_bounds: Option<PlayfieldBounds>,
    map_size_height: Option<i32>,
    binary_frame: u32,
    rng: &mut SimRng,
) -> Vec<Waypoint> {
    gather_start_positions_with_search(
        waypoints,
        participant_count,
        bounds,
        rng,
        |seed_rx, seed_ry| {
            find_nearby_start_rect(
                terrain,
                occupancy,
                playfield_bounds,
                map_size_height,
                binary_frame,
                seed_rx,
                seed_ry,
            )
        },
    )
}

/// Run Gather in the exact pre-Fill cell lifetime: MapClass has resized and
/// normalized Size/LocalSize, but every CellClass still has constructor
/// defaults and no Iso/overlay/Terrain/Techno/occupation authority exists.
pub(crate) fn native_gather_pre_fill_start_positions(
    waypoints: &HashMap<u32, Waypoint>,
    required_start_count: usize,
    header: &MapHeader,
    rng: &mut SimRng,
) -> Option<Vec<Waypoint>> {
    let bounds = NativeStartBounds::from_map_header(header)?;
    Some(gather_start_positions_with_search(
        waypoints,
        required_start_count,
        bounds,
        rng,
        |seed_rx, seed_ry| find_nearby_pre_fill_start_rect(header, seed_rx, seed_ry),
    ))
}

fn gather_start_positions_with_search(
    waypoints: &HashMap<u32, Waypoint>,
    participant_count: usize,
    bounds: NativeStartBounds,
    rng: &mut SimRng,
    mut find_nearby: impl FnMut(u16, u16) -> Option<(u16, u16)>,
) -> Vec<Waypoint> {
    let authored_prefix = (0..8u32)
        .take_while(|index| waypoints.contains_key(index))
        .count();
    let target_count = authored_prefix.max(participant_count);
    let mut starts = Vec::with_capacity(target_count);

    for index in 0..target_count as u32 {
        if let Some(waypoint) = waypoints.get(&index) {
            starts.push(*waypoint);
        }
    }

    while starts.len() < target_count {
        // gamemd-derived: active YR `ScenarioClass__Gather_Start_Positions
        // @ 0x00688380`, seed block `0x00688528..0x0068857C`: the first draw
        // `RandomRanged(0, rect.h - 10)` lands on the Y axis
        // (`+ 10 + rect.y`), the second `RandomRanged(10, rect.w - 10)` on the
        // X axis (`+ rect.x`). The rect is the MapClass cell-array rect (see
        // `NativeStartBounds::from_session`), which is square, so a transposed
        // mapping is RNG-neutral — but the seeded POINT mirrors across the
        // diagonal, so the axes are kept exactly as native writes them.
        let y_span = u32::from(bounds.height.saturating_sub(10));
        let x_high = u32::from(bounds.width.saturating_sub(10));
        let seed_ry = rng
            .next_range_u32_inclusive(0, y_span)
            .wrapping_add(10)
            .wrapping_add(u32::from(bounds.min_ry)) as u16;
        let seed_rx = rng
            .next_range_u32_inclusive(10, x_high)
            .wrapping_add(u32::from(bounds.min_rx)) as u16;

        let Some((rx, ry)) = find_nearby(seed_rx, seed_ry) else {
            continue;
        };
        starts.push(Waypoint {
            index: starts.len() as u32,
            rx,
            ry,
        });
    }

    starts
}

fn find_nearby_pre_fill_start_rect(
    header: &MapHeader,
    seed_rx: u16,
    seed_ry: u16,
) -> Option<(u16, u16)> {
    let playfield_bounds = crate::map::playfield::PlayfieldBounds::from_map_header(header);
    let query = NearbyQuery {
        native_cells: None,
        raw_occupation: None,
        passability: PassabilityArgs {
            speed_type: SpeedType::Track,
            required_zone_id: None,
            movement_zone: MovementZone::Normal,
            bridge_aware_zone: false,
        },
        footprint: NearbyFootprint::new(
            i32::from(DEFICIENT_START_RECT_W),
            i32::from(DEFICIENT_START_RECT_H),
        ),
        anchor_gate: NearbyAnchorGate::NativeHeightAware,
        allow_bridge_cells: true,
        check_height: false,
        check_occupancy: false,
        radius_cap: map_owned_radius_cap(playfield_bounds.base, header.height as i32),
        target_cell: None,
        path_grid: None,
        resolved_terrain: None,
        overlay_grid: None,
        occupancy: None,
        entities: None,
        zone_grid: None,
        playfield_bounds: Some(playfield_bounds),
    };
    find_nearby_passable_cell((i32::from(seed_rx), i32::from(seed_ry)), &query, 0)
}

/// Exact deficient-start adapter over the shared MapClass FNPC mechanism.
///
/// The caller's `8,8` are top-left CellRect dimensions, not a search radius.
/// Radius comes from the separately retained MapClass Size pair, and a missing
/// Size/playfield authority rejects the query instead of approximating it.
// gamemd-derived: `ScenarioClass::Gather_Start_Positions @ 0x00688380`, call
// `0x006885B5`, passes `(Track, -1, Normal, bridge-aware=0, 8x8,
// reject-overlay=0, height=0, obstacle=0, allow-bridge=1, null-reference,
// param15=0, final-occupancy=0)` to
// `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20`.
#[cfg(test)]
pub(crate) fn find_nearby_start_rect(
    terrain: &ResolvedTerrainGrid,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    playfield_bounds: Option<PlayfieldBounds>,
    map_size_height: Option<i32>,
    binary_frame: u32,
    seed_rx: u16,
    seed_ry: u16,
) -> Option<(u16, u16)> {
    let bounds = playfield_bounds?;
    let size_height = map_size_height?;
    let query = NearbyQuery {
        native_cells: None,
        raw_occupation: None,
        passability: PassabilityArgs {
            speed_type: SpeedType::Track,
            required_zone_id: None,
            movement_zone: MovementZone::Normal,
            bridge_aware_zone: false,
        },
        footprint: NearbyFootprint::new(
            i32::from(DEFICIENT_START_RECT_W),
            i32::from(DEFICIENT_START_RECT_H),
        ),
        anchor_gate: NearbyAnchorGate::NativeHeightAware,
        allow_bridge_cells: true,
        check_height: false,
        check_occupancy: false,
        radius_cap: map_owned_radius_cap(bounds.base, size_height),
        target_cell: None,
        path_grid: None,
        resolved_terrain: Some(terrain),
        overlay_grid: None,
        // CellRect passability reads the native occupation plane even though the
        // caller disables the later, distinct CheckOccupancy rectangle gate.
        occupancy: Some(occupancy),
        entities: None,
        zone_grid: None,
        playfield_bounds: Some(bounds),
    };
    find_nearby_passable_cell(
        (i32::from(seed_rx), i32::from(seed_ry)),
        &query,
        binary_frame,
    )
}

/// Retain the native start-table ownership and resolved placement for one
/// stock-offline assignment callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeStartAssignment {
    pub(crate) placements: Vec<(usize, Waypoint)>,
    pub(crate) start_table: Vec<Option<usize>>,
}

pub(crate) const HOUSE_CONSTRUCTOR_TIMER_MIN: u32 = 450;
pub(crate) const HOUSE_CONSTRUCTOR_TIMER_MAX: u32 = 1800;

/// The selected active-retail start callback used by offline noncampaign Full Init.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StockOfflineStartCallbackFamily {
    Battle,
    Cooperative,
}

/// Immutable stock-offline Scenario prefix prepared before terrain Fill.
///
/// This plan is the sole owner of the disposable first House pass, both
/// selected-mode Gather callbacks, the final chooser, the zero-draw reset, and
/// the second House pass. Only the second Gather vector and its retained
/// assignment are projected later; projection is deliberately draw-free.
#[derive(Debug)]
pub(crate) struct PreFillScenarioPrefixPlan {
    projection: StockOfflinePrefixProjection,
    first_gathered_starts: Vec<Waypoint>,
    first_house_timers: Vec<u32>,
    second_house_timers: Vec<u32>,
    scenario_rng_before: SimRngLogicalState,
    scenario_rng_before_fingerprint: u64,
    scenario_rng_after: SimRngLogicalState,
    scenario_rng_after_cursor: SimRng,
    native_resize_width: u32,
    native_resize_height: u32,
    #[cfg(test)]
    rng_checkpoints: ScenarioPrefixRngCheckpoints,
}

/// Paired Scenario-RNG and native numeric-ID prefix for one supported fresh
/// stock-offline Full_Init. Neither half can reach production independently.
#[derive(Debug)]
pub(crate) struct BoundStockOfflineFreshPrefixPlan {
    scenario: PreFillScenarioPrefixPlan,
    native_ids: crate::sim::native_identity::NativeFreshIdPrefixReceipt,
}

/// Draw-free stock-offline state retained after the prefix cursor receipt is
/// consumed.  It can place the final Houses and expose loading markers, but it
/// owns no RNG and cannot replay the prefix.
#[derive(Debug)]
pub(crate) struct StockOfflinePrefixProjection {
    /// Raw active Scenario waypoint table after the authored read or accepted
    /// RMG staging copy. Gather may derive fallback cells from this table, but
    /// loading markers and the live Scenario/session owner retain these exact
    /// entries rather than the Gather result or regenerated `.SED` table.
    active_scenario_waypoints: HashMap<u32, Waypoint>,
    final_gathered_starts: Vec<Waypoint>,
    assignment: NativeStartAssignment,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScenarioPrefixRngCheckpoints {
    pub(crate) after_first_house_pass: SimRngLogicalState,
    pub(crate) after_first_gather: SimRngLogicalState,
    pub(crate) after_second_gather_and_chooser: SimRngLogicalState,
    pub(crate) after_zero_draw_reset: SimRngLogicalState,
    pub(crate) after_second_house_pass: SimRngLogicalState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PreFillScenarioPrefixPlanError {
    #[error(
        "pre-Fill Scenario prefix expected RNG fingerprint {expected:#018x}, got {actual:#018x}"
    )]
    ScenarioRngPrestateMismatch { expected: u64, actual: u64 },
    #[error("launch mode id {id} is not the validated active-retail stock row")]
    UnsupportedStockMode { id: i32 },
    #[error(
        "pre-Fill roster requires {roster_start_count} starts but the compact launch session has {launch_participant_count} participants"
    )]
    RosterParticipantMismatch {
        roster_start_count: usize,
        launch_participant_count: usize,
    },
    #[error("map Size does not produce a valid resized pre-Fill cell rectangle")]
    InvalidMapCellExtent,
}

impl PreFillScenarioPrefixPlan {
    pub(crate) fn projection(&self) -> &StockOfflinePrefixProjection {
        &self.projection
    }

    #[cfg(test)]
    pub(crate) fn first_gathered_starts(&self) -> &[Waypoint] {
        &self.first_gathered_starts
    }

    #[cfg(test)]
    pub(crate) fn final_gathered_starts(&self) -> &[Waypoint] {
        self.projection.final_gathered_starts()
    }

    #[cfg(test)]
    pub(crate) fn assignment(&self) -> &NativeStartAssignment {
        self.projection.assignment()
    }

    #[cfg(test)]
    pub(crate) fn first_house_timers(&self) -> &[u32] {
        &self.first_house_timers
    }

    #[cfg(test)]
    pub(crate) fn second_house_timers(&self) -> &[u32] {
        &self.second_house_timers
    }

    #[cfg(test)]
    pub(crate) fn rng_checkpoints(&self) -> &ScenarioPrefixRngCheckpoints {
        &self.rng_checkpoints
    }

    /// Bind the move-only E/P Rules chronology to the already-validated House,
    /// Resize, and Scenario prefix. Type/House/Cell constructors advance only
    /// the independent native-ID cursor; Scenario RNG remains byte-for-byte
    /// owned by this plan.
    pub(crate) fn bind_native_rules_receipt(
        self,
        receipt: crate::rules::process_owner::NativeScenarioRulesReceipt,
    ) -> BoundStockOfflineFreshPrefixPlan {
        let (early, rebuilt) = receipt.into_parts();
        let (early_events, first_super_weapon_type_count) = early.into_parts();
        let (rebuilt_events, final_super_weapon_type_count) = rebuilt.into_parts();
        let native_ids = crate::sim::native_identity::build_noncampaign_fresh_id_prefix(
            early_events.len(),
            first_super_weapon_type_count,
            self.first_house_timers.len(),
            rebuilt_events.len(),
            final_super_weapon_type_count,
            self.second_house_timers.len(),
            self.native_resize_width,
            self.native_resize_height,
        );
        BoundStockOfflineFreshPrefixPlan {
            scenario: self,
            native_ids,
        }
    }

    /// Validate and transfer the one pre-loading RNG prefix to the stream that
    /// later terrain Fill and Simulation construction will continue.
    fn install_before_terrain(
        self,
        scenario_rng: &mut SimRng,
    ) -> Result<StockOfflinePrefixProjection, PreFillScenarioPrefixPlanError> {
        if scenario_rng.logical_state() != self.scenario_rng_before {
            return Err(
                PreFillScenarioPrefixPlanError::ScenarioRngPrestateMismatch {
                    expected: self.scenario_rng_before_fingerprint,
                    actual: scenario_rng.state(),
                },
            );
        }
        *scenario_rng = self.scenario_rng_after_cursor;
        debug_assert_eq!(scenario_rng.logical_state(), self.scenario_rng_after);
        Ok(self.projection)
    }
}

impl BoundStockOfflineFreshPrefixPlan {
    pub(crate) fn projection(&self) -> &StockOfflinePrefixProjection {
        self.scenario.projection()
    }

    fn into_parts(
        self,
    ) -> (
        PreFillScenarioPrefixPlan,
        crate::sim::native_identity::NativeFreshIdPrefixReceipt,
    ) {
        (self.scenario, self.native_ids)
    }
}

impl StockOfflinePrefixProjection {
    pub(crate) fn active_scenario_waypoints(&self) -> &HashMap<u32, Waypoint> {
        &self.active_scenario_waypoints
    }

    pub(crate) fn final_gathered_starts(&self) -> &[Waypoint] {
        &self.final_gathered_starts
    }

    pub(crate) fn start_table(&self) -> &[Option<usize>] {
        &self.assignment.start_table
    }

    pub(crate) fn assignment(&self) -> &NativeStartAssignment {
        &self.assignment
    }
}

/// Prepare the complete active-stock offline prefix exactly once.
///
/// `start_waypoints` is an explicit source selected by the app boundary:
/// authored maps pass the parsed map table, while accepted Battle/FFA `.SED`
/// launches pass the provenance-bearing setup staging. Both Gather callbacks
/// operate only on the resized default-cell view derived from `[Map]`.
pub(crate) fn prepare_stock_offline_scenario_prefix_plan(
    descriptor: &MatchLaunchDescriptor,
    map_data: &MapFile,
    start_waypoints: &HashMap<u32, Waypoint>,
    launch_seed: u32,
) -> Result<PreFillScenarioPrefixPlan, PreFillScenarioPrefixPlanError> {
    let session = descriptor.session();
    let family = stock_offline_start_callback_family(session)?;
    let launch_participant_count = 1usize + session.opponents.len();
    let roster_start_count = session.pre_fill_house_roster.required_start_count();
    if roster_start_count != launch_participant_count {
        return Err(PreFillScenarioPrefixPlanError::RosterParticipantMismatch {
            roster_start_count,
            launch_participant_count,
        });
    }

    let mut scenario_rng = SimRng::new(u64::from(launch_seed));
    let scenario_rng_before = scenario_rng.logical_state();
    let scenario_rng_before_fingerprint = scenario_rng.state();

    let first_house_timers =
        advance_pre_fill_house_constructor_pass(&session.pre_fill_house_roster, &mut scenario_rng);
    #[cfg(test)]
    let after_first_house_pass = scenario_rng.logical_state();
    let first_gathered_starts = native_gather_pre_fill_start_positions(
        start_waypoints,
        roster_start_count,
        &map_data.header,
        &mut scenario_rng,
    )
    .ok_or(PreFillScenarioPrefixPlanError::InvalidMapCellExtent)?;
    #[cfg(test)]
    let after_first_gather = scenario_rng.logical_state();
    let preassignment = native_preassign_launch_start_table(session, first_gathered_starts.len());

    let final_gathered_starts = native_gather_pre_fill_start_positions(
        start_waypoints,
        roster_start_count,
        &map_data.header,
        &mut scenario_rng,
    )
    .ok_or(PreFillScenarioPrefixPlanError::InvalidMapCellExtent)?;
    let assignment = match family {
        StockOfflineStartCallbackFamily::Battle => native_assign_launch_starts_from_preassignment(
            session,
            &final_gathered_starts,
            &preassignment,
            &mut scenario_rng,
        ),
        StockOfflineStartCallbackFamily::Cooperative => {
            let human_start_spots = map_data
                .ini
                .section("Header")
                .and_then(|header| header.get_i32("NumCoopHumanStartSpots"))
                .unwrap_or(0)
                .max(0) as usize;
            native_assign_cooperative_starts_from_preassignment(
                session,
                &final_gathered_starts,
                &preassignment,
                human_start_spots,
                session.pre_fill_house_roster.nonobserver_human_count(),
                &mut scenario_rng,
            )
        }
    };
    #[cfg(test)]
    let after_second_gather_and_chooser = scenario_rng.logical_state();

    // Native deletes every disposable first-pass House here. Destruction and
    // the following rules/basic reset consume no Scenario draw, so the second
    // pass begins at the chooser's exact cursor.
    #[cfg(test)]
    let after_zero_draw_reset = scenario_rng.logical_state();
    let second_house_timers =
        advance_pre_fill_house_constructor_pass(&session.pre_fill_house_roster, &mut scenario_rng);
    let scenario_rng_after = scenario_rng.logical_state();
    #[cfg(test)]
    let after_second_house_pass = scenario_rng_after.clone();

    Ok(PreFillScenarioPrefixPlan {
        projection: StockOfflinePrefixProjection {
            active_scenario_waypoints: start_waypoints.clone(),
            final_gathered_starts,
            assignment,
        },
        first_gathered_starts,
        first_house_timers,
        second_house_timers,
        scenario_rng_before,
        scenario_rng_before_fingerprint,
        scenario_rng_after,
        scenario_rng_after_cursor: scenario_rng,
        native_resize_width: map_data.header.width,
        native_resize_height: map_data.header.height,
        #[cfg(test)]
        rng_checkpoints: ScenarioPrefixRngCheckpoints {
            after_first_house_pass,
            after_first_gather,
            after_second_gather_and_chooser,
            after_zero_draw_reset,
            after_second_house_pass,
        },
    })
}

fn advance_pre_fill_house_constructor_pass(
    roster: &PreFillHouseRoster,
    rng: &mut SimRng,
) -> Vec<u32> {
    let mut timers = Vec::with_capacity(roster.created_house_count());
    for _human in roster.human_nodes() {
        timers.push(
            rng.next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX),
        );
    }
    for _slot in roster.ai_slots().iter().filter(|slot| slot.valid) {
        timers.push(
            rng.next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX),
        );
    }
    for _fixed in roster.fixed_tail() {
        timers.push(
            rng.next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX),
        );
    }
    timers
}

pub(crate) fn stock_offline_start_callback_family(
    session: &SkirmishLaunchSession,
) -> Result<StockOfflineStartCallbackFamily, PreFillScenarioPrefixPlanError> {
    let mode = &session.mode;
    let expected = match mode.id {
        1 => (
            "GUI:Battle",
            "STT:ModeBattle",
            "MPBattleMD.ini",
            "standard",
            true,
            true,
            false,
            StockOfflineStartCallbackFamily::Battle,
        ),
        2 => (
            "GUI:FreeForAll",
            "STT:ModeFreeForAll",
            "MPFreeForAllMD.ini",
            "standard",
            true,
            false,
            false,
            StockOfflineStartCallbackFamily::Battle,
        ),
        3 => (
            "GUI:Cooperative",
            "STT:ModeCooperative",
            "MPCoopMD.ini",
            "cooperative",
            false,
            false,
            false,
            StockOfflineStartCallbackFamily::Cooperative,
        ),
        4 => (
            "GUI:UnholyAlliance",
            "STT:ModeUnholyAlliance",
            "MPUnholyMD.ini",
            "standard",
            false,
            true,
            false,
            StockOfflineStartCallbackFamily::Battle,
        ),
        5 => (
            "GUI:Megawealth",
            "STT:ModeMegawealth",
            "MPMWMD.ini",
            "megawealth",
            false,
            true,
            false,
            StockOfflineStartCallbackFamily::Battle,
        ),
        6 => (
            "GUI:Duel",
            "STT:ModeDuel",
            "MPDuelMD.ini",
            "duel",
            false,
            true,
            false,
            StockOfflineStartCallbackFamily::Battle,
        ),
        7 => (
            "GUI:MeatGrind",
            "STT:ModeMeatGrind",
            "MPMeatMD.ini",
            "meatgrind",
            false,
            true,
            false,
            StockOfflineStartCallbackFamily::Battle,
        ),
        8 => (
            "GUI:NavalWar",
            "STT:ModeNavalWar",
            "MPNavalMD.ini",
            "navalwar",
            false,
            true,
            false,
            StockOfflineStartCallbackFamily::Battle,
        ),
        9 => (
            "GUI:TeamGame",
            "STT:ModeTeamGame",
            "MPTeamMD.ini",
            "teamgame",
            false,
            true,
            true,
            StockOfflineStartCallbackFamily::Battle,
        ),
        _ => {
            return Err(PreFillScenarioPrefixPlanError::UnsupportedStockMode { id: mode.id });
        }
    };
    let (ui_name, tooltip, override_file, map_filter, random_maps, allies, must_ally, family) =
        expected;
    if mode.ui_name_key.eq_ignore_ascii_case(ui_name)
        && mode.tooltip_key.eq_ignore_ascii_case(tooltip)
        && mode.override_file.eq_ignore_ascii_case(override_file)
        && mode.map_filter.eq_ignore_ascii_case(map_filter)
        && mode.random_maps_allowed == random_maps
        && mode.allies_allowed == allies
        && mode.must_ally == must_ally
    {
        Ok(family)
    } else {
        Err(PreFillScenarioPrefixPlanError::UnsupportedStockMode { id: mode.id })
    }
}

#[cfg(test)]
pub(crate) fn native_assign_launch_starts(
    session: &SkirmishLaunchSession,
    starts: &[Waypoint],
    rng: &mut SimRng,
) -> NativeStartAssignment {
    let preassignment = native_preassign_launch_start_table(session, starts.len());
    native_assign_launch_starts_from_preassignment(session, starts, &preassignment, rng)
}

fn launch_start_requests(session: &SkirmishLaunchSession) -> Vec<LaunchStartPosition> {
    std::iter::once(session.local.start_position)
        .chain(
            session
                .opponents
                .iter()
                .map(|opponent| opponent.start_position),
        )
        .collect()
}

pub(crate) fn native_preassign_launch_start_table(
    session: &SkirmishLaunchSession,
    start_count: usize,
) -> Vec<Option<usize>> {
    let requested = launch_start_requests(session);
    let mut explicit_owner = vec![None; start_count];
    for (slot, request) in requested.iter().enumerate() {
        let LaunchStartPosition::Position(index) = request else {
            continue;
        };
        if let Some(owner) = explicit_owner.get_mut(usize::from(*index)) {
            *owner = Some(slot);
        }
    }
    explicit_owner
}

fn native_assign_launch_starts_from_preassignment(
    session: &SkirmishLaunchSession,
    starts: &[Waypoint],
    preassignment: &[Option<usize>],
    rng: &mut SimRng,
) -> NativeStartAssignment {
    if starts.is_empty() {
        return NativeStartAssignment {
            placements: Vec::new(),
            start_table: Vec::new(),
        };
    }

    assert_eq!(starts.len(), preassignment.len());
    let requested = launch_start_requests(session);
    let mut explicit_owner = preassignment.to_vec();

    // Battle +0x84 builds its occupied-byte array from the complete
    // Scenario+0x1180 table before its HouseClass pass begins. The selected
    // mode's +0x80 callback populated that table in HouseClass order, so
    // duplicate explicit starts have already resolved last-writer-wins.
    let mut occupied: Vec<bool> = explicit_owner.iter().map(Option::is_some).collect();
    let mut assigned = vec![None; requested.len()];

    // Unlike generic AssignStartingPoints, standard Battle walks every
    // non-special House once and honors table ownership for AI houses too.
    for slot in 0..requested.len() {
        let explicit = explicit_owner
            .iter()
            .rposition(|owner| *owner == Some(slot));
        let start_index =
            explicit.unwrap_or_else(|| choose_battle_automatic_start(starts, &occupied, rng));
        occupied[start_index] = true;
        explicit_owner[start_index] = Some(slot);
        assigned[slot] = Some(starts[start_index]);
    }

    NativeStartAssignment {
        placements: assigned
            .into_iter()
            .enumerate()
            .filter_map(|(slot, start)| start.map(|start| (slot, start)))
            .collect(),
        start_table: explicit_owner,
    }
}

/// Cooperative's custom start callback partitions authored positions into a
/// human prefix and an AI suffix. The explicit Scenario start table reserves
/// all of its entries before HouseClass iteration; automatic human placement
/// draws within the prefix and probes forward, while the remaining houses
/// take the first free suffix entry without a draw.
#[cfg(test)]
pub(crate) fn native_assign_cooperative_launch_starts(
    session: &SkirmishLaunchSession,
    starts: &[Waypoint],
    human_start_spots: usize,
    rng: &mut SimRng,
) -> NativeStartAssignment {
    let preassignment = native_preassign_launch_start_table(session, starts.len());
    native_assign_cooperative_starts_from_preassignment(
        session,
        starts,
        &preassignment,
        human_start_spots,
        1,
        rng,
    )
}

fn native_assign_cooperative_starts_from_preassignment(
    session: &SkirmishLaunchSession,
    starts: &[Waypoint],
    preassignment: &[Option<usize>],
    human_start_spots: usize,
    human_house_count: usize,
    rng: &mut SimRng,
) -> NativeStartAssignment {
    if starts.is_empty() {
        return NativeStartAssignment {
            placements: Vec::new(),
            start_table: Vec::new(),
        };
    }

    assert_eq!(starts.len(), preassignment.len());
    let requested = launch_start_requests(session);
    let mut explicit_owner = preassignment.to_vec();

    let mut occupied: Vec<bool> = explicit_owner.iter().map(Option::is_some).collect();
    let human_start_spots = human_start_spots.min(starts.len());
    let mut assigned = vec![None; requested.len()];

    for slot in 0..requested.len() {
        let explicit = explicit_owner.iter().position(|owner| *owner == Some(slot));
        let start_index = if let Some(explicit) = explicit {
            explicit
        } else {
            let occupied_count = occupied.iter().filter(|used| **used).count();
            if occupied_count < human_house_count && human_start_spots != 0 {
                let mut candidate =
                    rng.next_range_u32_inclusive(0, human_start_spots as u32 - 1) as usize;
                while occupied[candidate] {
                    candidate = (candidate + 1) % human_start_spots;
                }
                candidate
            } else {
                (human_start_spots..starts.len())
                    .find(|candidate| !occupied[*candidate])
                    .or_else(|| occupied.iter().position(|used| !*used))
                    .expect("participant count cannot exceed gathered starts")
            }
        };
        occupied[start_index] = true;
        explicit_owner[start_index] = Some(slot);
        assigned[slot] = Some(starts[start_index]);
    }

    NativeStartAssignment {
        placements: assigned
            .into_iter()
            .enumerate()
            .filter_map(|(slot, start)| start.map(|start| (slot, start)))
            .collect(),
        start_table: explicit_owner,
    }
}

pub(crate) fn choose_battle_automatic_start(
    starts: &[Waypoint],
    occupied: &[bool],
    rng: &mut SimRng,
) -> usize {
    let used_count = occupied.iter().filter(|used| **used).count();
    if used_count == 0 {
        return rng.next_range_u32_inclusive(0, starts.len() as u32 - 1) as usize;
    }

    let free: Vec<usize> = occupied
        .iter()
        .enumerate()
        .filter_map(|(index, used)| (!*used).then_some(index))
        .collect();
    let distance_sum = |candidate: usize| -> i32 {
        occupied
            .iter()
            .enumerate()
            .filter(|(_, used)| **used)
            .map(|(other, _)| native_start_distance(starts[candidate], starts[other]))
            .sum()
    };
    let mut iter = free.into_iter();
    let mut selected = iter
        .next()
        .expect("participant count cannot exceed gathered starts");
    let mut selected_sum = distance_sum(selected);
    for candidate in iter {
        let sum = distance_sum(candidate);
        if sum > selected_sum {
            selected = candidate;
            selected_sum = sum;
        }
    }
    selected
}

pub(crate) fn native_start_distance(left: Waypoint, right: Waypoint) -> i32 {
    let dx = i32::from((left.rx as i16).wrapping_sub(right.rx as i16));
    let dy = i32::from((left.ry as i16).wrapping_sub(right.ry as i16));
    let squared_distance = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
    let root = sqrt_approx_f32(X87Chop53::load_i32(squared_distance))
        .expect("signed squared start distance stays in the verified finite x87 domain");
    f32::from_bits(root.bits()) as i32
}

const DEFICIENT_START_RECT_W: u16 = 8;
const DEFICIENT_START_RECT_H: u16 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkirmishLaunchApplyResult {
    pub(crate) local_owner: Option<String>,
    pub(crate) spawned_mcvs: u32,
    pub(crate) active_slots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedSkirmishSlot {
    pub(crate) owner_name: String,
    pub(crate) country: LaunchCountry,
    pub(crate) color_index: u8,
    pub(crate) start_position: LaunchStartPosition,
    pub(crate) team: LaunchTeam,
    pub(crate) is_human: bool,
    pub(crate) difficulty: HouseDifficulty,
}

/// Sim-owned behavioral launch descriptor (F09).
///
/// Wraps a `SkirmishLaunchSession` whose shell-random choices — country and
/// color, for the local slot and every AI slot — are proven resolved. The
/// validating constructor is the only way in, so sim entry points cannot
/// receive an unresolved frontend session and silently launch with the
/// placeholder country/color a random slot still carries. Start positions
/// remain potentially random by design: gamemd assigns them at scenario load
/// with the gameplay Scenario RNG (see
/// `prepare_stock_offline_scenario_prefix_plan`), not in the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchLaunchDescriptor {
    session: SkirmishLaunchSession,
}

/// A launch slot still carried an unresolved random shell choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnresolvedShellChoice {
    /// `None` = the local player slot; `Some(i)` = AI opponent index `i`.
    pub ai_slot: Option<usize>,
    /// Which choice was left random: `"country"` or `"color"`.
    pub choice: &'static str,
}

impl std::fmt::Display for UnresolvedShellChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.ai_slot {
            None => write!(f, "local slot still has a random {}", self.choice),
            Some(index) => write!(f, "AI slot {index} still has a random {}", self.choice),
        }
    }
}

impl MatchLaunchDescriptor {
    /// Validate that the app's shell close transaction resolved every
    /// random choice before the session crosses into sim.
    pub fn from_resolved(session: SkirmishLaunchSession) -> Result<Self, UnresolvedShellChoice> {
        if session.local.country_random {
            return Err(UnresolvedShellChoice {
                ai_slot: None,
                choice: "country",
            });
        }
        if session.local.color_random {
            return Err(UnresolvedShellChoice {
                ai_slot: None,
                choice: "color",
            });
        }
        for (index, opponent) in session.opponents.iter().enumerate() {
            if opponent.country_random {
                return Err(UnresolvedShellChoice {
                    ai_slot: Some(index),
                    choice: "country",
                });
            }
            if opponent.color_random {
                return Err(UnresolvedShellChoice {
                    ai_slot: Some(index),
                    choice: "color",
                });
            }
        }
        Ok(Self { session })
    }

    /// The resolved session's gameplay facts.
    pub(crate) fn session(&self) -> &SkirmishLaunchSession {
        &self.session
    }
}

/// Construct the active offline-skirmish House array before any map object.
///
/// Active YR `ScenarioClass__Full_Init @ 0x00686B20` calls
/// `ScenarioClass__Create_Houses @ 0x00687F10` before terrain and Techno map
/// sections. The standard offline order is the human participant, AI slots,
/// then Neutral and Special. Object construction and Reveal can therefore
/// commit the house tracking counts and house-indexed base reservations
/// directly, without a repair pass.
pub(crate) fn initialize_skirmish_launch_houses(
    sim: &mut Simulation,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    descriptor: &MatchLaunchDescriptor,
) {
    let session = descriptor.session();
    assert!(
        sim.houses.is_empty()
            && sim.session.house_order.is_empty()
            && sim.ai_players.is_empty()
            && sim.entities().is_empty()
            && sim.production.terrain_objects.is_empty(),
        "skirmish houses must be initialized before map objects"
    );
    let slots = normalized_launch_slots(session);
    sim.session.game_options = session
        .options
        .to_game_options(session.opponents.len() as i32);
    populate_launch_houses(sim, &slots, rules);
    // 688109 installs the original participant-array index0. These normalized
    // slots retain that launch identity; later MCV placement/viewer choice
    // cannot clear or replace the current house.
    sim.session.current_house = slots.first().map(|slot| {
        let id = sim
            .interner
            .get(&slot.owner_name)
            .expect("created launch house");
        assert!(sim.houses.contains_key(&id));
        id
    });
    populate_special_houses(sim, house_roster, rules);
}

/// Commit the explicit launch-session diplomacy graph at the post-crate boundary.
pub(crate) fn apply_skirmish_launch_alliances(
    sim: &mut Simulation,
    house_roster: &HouseRoster,
    session: &SkirmishLaunchSession,
) {
    let slots = normalized_launch_slots(session);
    sim.house_alliances = launch_alliance_map(house_roster, &slots, &session.mode);
}

/// Test-only compatibility path for direct post-Fill launch fixtures.
#[cfg(test)]
pub(crate) fn apply_explicit_skirmish_launch_session(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    resolved_terrain: &ResolvedTerrainGrid,
    descriptor: &MatchLaunchDescriptor,
) -> SkirmishLaunchApplyResult {
    apply_resolved_skirmish_launch_session(
        sim,
        map_data,
        house_roster,
        rules,
        height_map,
        resolved_terrain,
        descriptor,
        None,
        LaunchStartResolution::PostFillTestCompatibility,
    )
}

/// Apply the retained stock-offline House/start projection without repeating
/// any prefix draw. Existing starting-force and later initialization draws
/// remain after this projection.
#[cfg(test)]
pub(crate) fn apply_pre_fill_scenario_prefix_launch_session(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    resolved_terrain: &ResolvedTerrainGrid,
    descriptor: &MatchLaunchDescriptor,
    plan: &StockOfflinePrefixProjection,
) -> SkirmishLaunchApplyResult {
    apply_resolved_skirmish_launch_session(
        sim,
        map_data,
        house_roster,
        rules,
        height_map,
        resolved_terrain,
        descriptor,
        None,
        LaunchStartResolution::Prefix(plan),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_pre_fill_scenario_prefix_launch_session_with_overlay_registry(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    resolved_terrain: &ResolvedTerrainGrid,
    descriptor: &MatchLaunchDescriptor,
    overlay_registry: &OverlayTypeRegistry,
    plan: &StockOfflinePrefixProjection,
) -> SkirmishLaunchApplyResult {
    apply_resolved_skirmish_launch_session(
        sim,
        map_data,
        house_roster,
        rules,
        height_map,
        resolved_terrain,
        descriptor,
        Some(overlay_registry),
        LaunchStartResolution::Prefix(plan),
    )
}

enum LaunchStartResolution<'a> {
    Prefix(&'a StockOfflinePrefixProjection),
    #[cfg(test)]
    PostFillTestCompatibility,
}

fn apply_resolved_skirmish_launch_session(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    resolved_terrain: &ResolvedTerrainGrid,
    descriptor: &MatchLaunchDescriptor,
    overlay_registry: Option<&OverlayTypeRegistry>,
    start_resolution: LaunchStartResolution<'_>,
) -> SkirmishLaunchApplyResult {
    // Direct frontend/unit-test launch paths can enter before the shared
    // scenario construction funnel. Retail has already completed
    // `0x00654490 -> MapClass::Set_Clipped_LocalSize @ 0x00567230` before
    // Battle start gathering and placement consume the playfield fields.
    sim.install_playfield_from_map_header(&map_data.header);

    let session = descriptor.session();
    let slots = normalized_launch_slots(session);
    if sim.houses.is_empty() {
        // Direct unit-level callers may enter before the shared app load funnel.
        // The initializer rejects any already-constructed map object, so this
        // fallback cannot recreate the former object-before-house production path.
        initialize_skirmish_launch_houses(sim, house_roster, rules, descriptor);
    }
    assert_eq!(
        sim.session.house_order.len(),
        slots.len() + 2,
        "launch House array must be complete before start assignment"
    );
    for (registered, slot) in sim.session.house_order.iter().zip(&slots) {
        assert_eq!(
            sim.interner.resolve(*registered),
            slot.owner_name,
            "participant House order changed after map construction"
        );
    }
    for (registered, expected) in sim.session.house_order[slots.len()..]
        .iter()
        .zip(["Neutral", "Special"])
    {
        assert!(
            sim.interner
                .resolve(*registered)
                .eq_ignore_ascii_case(expected),
            "special House order changed after map construction"
        );
    }

    let bounds = NativeStartBounds::from_session(sim, resolved_terrain);
    let (_starts, start_assignment) = match start_resolution {
        LaunchStartResolution::Prefix(plan) => {
            // The same immutable table already drove the first loading markers.
            // Its complete House/Gather/assignment prefix was installed before
            // Fill, so projecting it here cannot draw or replace the live cursor.
            (
                plan.final_gathered_starts().to_vec(),
                plan.assignment().clone(),
            )
        }
        #[cfg(test)]
        LaunchStartResolution::PostFillTestCompatibility => {
            let cooperative = session
                .mode
                .override_file
                .eq_ignore_ascii_case("MPCoopMD.ini");
            // Direct unit fixtures historically enter with a constructed map.
            // Keep their compatibility resolver outside production compilation,
            // while still modeling two independent Gather callbacks.
            let _first_gather = sim.gather_native_start_positions(
                &map_data.waypoints,
                slots.len(),
                resolved_terrain,
                bounds,
            );
            let starts = sim.gather_native_start_positions(
                &map_data.waypoints,
                slots.len(),
                resolved_terrain,
                bounds,
            );
            let assignment = if cooperative {
                let human_start_spots = map_data
                    .ini
                    .section("Header")
                    .and_then(|header| header.get_i32("NumCoopHumanStartSpots"))
                    .unwrap_or(0)
                    .max(0) as usize;
                sim.assign_native_cooperative_starts(session, &starts, human_start_spots)
            } else {
                sim.assign_native_battle_starts(session, &starts)
            };
            (starts, assignment)
        }
    };
    project_pre_fill_start_assignment(sim, &slots, &start_assignment);
    let assignments = &start_assignment.placements;
    let mut spawned_mcvs = 0;
    let mut local_owner = slots.first().map(|slot| slot.owner_name.clone());

    if session.options.bases {
        for (slot_idx, waypoint) in assignments {
            let Some(slot) = slots.get(*slot_idx) else {
                continue;
            };
            let mcv_type = launch_mcv_type_for_country(slot.country, rules);
            if place_starting_mcv(
                sim,
                mcv_type,
                &slot.owner_name,
                waypoint.rx,
                waypoint.ry,
                bounds,
                rules,
                height_map,
                resolved_terrain,
                overlay_registry,
            )
            .is_some()
            {
                spawned_mcvs += 1;
            } else {
                log::warn!(
                    "Failed to seed session MCV '{}' for {} at waypoint {} ({},{})",
                    mcv_type,
                    slot.owner_name,
                    waypoint.index,
                    waypoint.rx,
                    waypoint.ry
                );
                if slot.is_human {
                    local_owner = None;
                }
            }
        }

        if spawned_mcvs > 0 {
            log::info!(
                "Seeded {} session skirmish MCV(s) for {} active slot(s)",
                spawned_mcvs,
                slots.len()
            );
        }
    }

    seed_starting_extra_units_with_overlay_registry(
        sim,
        &slots,
        rules,
        height_map,
        resolved_terrain,
        bounds,
        session.options.unit_count,
        map_data
            .special_flags
            .initial_veteran
            .unwrap_or(rules.initial_veteran),
        overlay_registry,
    );

    // The selected-mode starting-force orchestrator advances Scenario RNG once
    // after the complete house pass, including zero-budget runs.
    let _ = sim.scenario_rng.next_range_u32_inclusive(0, 0xffff);

    let local_human_owner = slots
        .iter()
        .find(|slot| slot.is_human)
        .map(|slot| slot.owner_name.as_str());
    apply_launch_shroud_option(sim, local_human_owner);

    SkirmishLaunchApplyResult {
        local_owner,
        spawned_mcvs,
        active_slots: slots.len(),
    }
}

/// Project the already-resolved start table and base centers. This boundary is
/// intentionally incapable of drawing: the selected-mode chooser completed in
/// the pre-Fill plan, while starting Technos and the force tail remain later.
fn project_pre_fill_start_assignment(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    start_assignment: &NativeStartAssignment,
) {
    // Session start-slot -> house table: filled after the random-assignment
    // draws, before tick 0 — lockstep state (hashed + serialized).
    sim.session.start_slot_houses.clear();
    for (start_idx, slot_idx) in start_assignment.start_table.iter().enumerate() {
        let Some(slot_idx) = *slot_idx else {
            continue;
        };
        let Some(slot) = slots.get(slot_idx) else {
            continue;
        };
        let owner = sim.interner.intern(&slot.owner_name);
        sim.session
            .start_slot_houses
            .insert(start_idx as u32, owner);
    }
    assign_launch_base_centers(sim, slots, &start_assignment.placements);
}

/// Apply the lobby's one-shot unexplored-shroud choice to the local human
/// viewer plane. AI houses keep their own unexplored knowledge and the reveal
/// never grants VERA's current-sight flag.
pub(crate) fn apply_launch_shroud_option(sim: &mut Simulation, local_owner: Option<&str>) {
    if sim.session.game_options.shroud {
        return;
    }
    let Some(owner) = local_owner.and_then(|owner| sim.interner.get(owner)) else {
        return;
    };
    if !sim.houses.get(&owner).is_some_and(|house| house.is_human) {
        return;
    }
    if let Some(terrain) = sim.resolved_terrain.as_ref() {
        sim.fog
            .reveal_cells_for_owner(owner, terrain.iter().map(|cell| (cell.rx, cell.ry)));
    } else {
        sim.fog.reveal_all_for_owner(owner);
    }
}

fn assign_launch_base_centers(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    assignments: &[(usize, Waypoint)],
) {
    for (slot_idx, waypoint) in assignments {
        let Some(slot) = slots.get(*slot_idx) else {
            continue;
        };
        let waypoint_edge = sim
            .playfield_bounds
            .map(|bounds| determine_waypoint_edge((waypoint.rx, waypoint.ry), bounds));
        if let Some(house) = crate::sim::house_state::house_state_for_owner_mut(
            &mut sim.houses,
            &slot.owner_name,
            &sim.interner,
        ) {
            house.base_center = Some((waypoint.rx, waypoint.ry));
            if let Some(waypoint_edge) = waypoint_edge {
                house.waypoint_edge = waypoint_edge;
            }
        }
    }
}

pub(crate) fn normalized_launch_slots(
    session: &SkirmishLaunchSession,
) -> Vec<NormalizedSkirmishSlot> {
    // Native 687F10 allocates a distinct House before copying its names;
    // 688092..6880A4 stores the participant handle separately from +15FF4.
    // Our shared owner-key projection must likewise avoid merging the local
    // House with a generated AI or special House. Keep the display handle in
    // session.player_name and preserve every noncolliding compatibility key.
    // `Player` cannot match Neutral/Special or any generated ComputerN key.
    // Evidence: docs/research/PHASE3_CURRENT_HOUSE_IDENTITY_GHIDRA_REPORT.md.
    let name_collides = ["Neutral", "Special"]
        .iter()
        .any(|name| session.player_name.eq_ignore_ascii_case(name))
        || (1..=session.opponents.len()).any(|index| {
            session
                .player_name
                .eq_ignore_ascii_case(&format!("Computer{index}"))
        });
    let local_owner_name = if name_collides {
        "Player"
    } else {
        &session.player_name
    };
    let mut slots = Vec::with_capacity(1 + session.opponents.len());
    slots.push(NormalizedSkirmishSlot {
        owner_name: local_owner_name.to_string(),
        country: session.local.country,
        color_index: session.local.color_index,
        start_position: session.local.start_position,
        team: session.local.team,
        is_human: true,
        difficulty: HouseDifficulty::Normal,
    });
    for (idx, opponent) in session.opponents.iter().enumerate() {
        slots.push(NormalizedSkirmishSlot {
            owner_name: format!("Computer{}", idx + 1),
            country: opponent.country,
            color_index: opponent.color_index,
            start_position: opponent.start_position,
            team: opponent.team,
            is_human: false,
            difficulty: HouseDifficulty::from_native(opponent.difficulty.as_i32())
                .expect("AiDifficulty uses native HouseClass discriminants"),
        });
    }
    slots
}

pub(crate) fn populate_special_houses(
    sim: &mut Simulation,
    house_roster: &HouseRoster,
    rules: &RuleSet,
) {
    for special_name in ["Neutral", "Special"] {
        let definition = house_roster
            .houses
            .iter()
            .find(|house| house.name.eq_ignore_ascii_case(special_name));
        let name = definition.map_or(special_name, |house| house.name.as_str());
        let name_id = sim.interner.intern(name);
        let country_name = definition
            .and_then(|house| house.country.as_deref())
            .unwrap_or(special_name);
        let country_id = Some(sim.interner.intern(country_name));
        let declared_side = definition.and_then(|house| house.side.as_deref());
        let side_idx = crate::sim::house_state::resolve_house_side_index(
            rules,
            Some(country_name),
            declared_side,
            crate::sim::house_state::side_index_from_name(declared_side),
        );
        let mut house = HouseState::new(
            name_id,
            side_idx,
            country_id,
            false,
            sim.session.game_options.starting_credits,
            sim.session.game_options.tech_level,
        );
        // Stock Neutral/Special are MultiplayPassive, which keeps them out of
        // defeat evaluation and out of the game-over alive scan. `country_name`
        // is always concrete here — the roster's `Country=` when the map named
        // one, otherwise the special house's own `[Countries]` entry, which is
        // what this house is being built as.
        house.multiplay_passive =
            crate::sim::house_state::resolve_multiplay_passive(Some(rules), Some(country_name));
        sim.houses.insert(name_id, house);
        sim.session.house_order.push(name_id);
    }
}

pub(crate) fn populate_launch_houses(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    rules: &RuleSet,
) {
    for slot in slots {
        let name_id = sim.interner.intern(&slot.owner_name);
        let country_name = slot.country.country_name();
        let country_id = sim.interner.intern(country_name);
        let side_index = crate::sim::house_state::resolve_house_side_index(
            rules,
            Some(country_name),
            None,
            slot.country.side_index(),
        );
        let mut house = HouseState::new(
            name_id,
            side_index,
            Some(country_id),
            slot.is_human,
            sim.session.game_options.starting_credits,
            sim.session.game_options.tech_level,
        );
        // `ScenarioClass::Create_Houses` passes every launch house through
        // `HouseClass::SetDifficulty` (humans with 1, `0x00688132`; computer
        // slots with their own, `0x006882B9`); Neutral and Special are never
        // passed and keep the constructor's values.
        house.set_difficulty(
            slot.difficulty,
            &rules.general.difficulty_rof,
            rules.country_rof(country_name),
            sim.session.game_mode_nonzero,
        );
        // ScenarioClass::Create_Houses leaves generated human CurrentIQ at
        // the constructor value zero and stamps generated computer slots with
        // Rules.MaxIQLevels in non-campaign sessions.
        if !slot.is_human {
            house.current_iq = rules.general.max_iq_levels;
        }
        house.multiplay_passive =
            crate::sim::house_state::resolve_multiplay_passive(Some(rules), Some(country_name));
        sim.houses.insert(name_id, house);
        sim.session.house_order.push(name_id);
        if !slot.is_human {
            sim.ai_players.push(AiPlayerState::new(name_id));
            log::info!("AI player registered: {}", slot.owner_name);
        }
    }
}

/// Bit pattern of the `0.01` double at `0x007E3808` read by
/// `ScenarioClass__Post_Map_Init @ 0x00686A52` (`FMUL qword [0x007E3808]`).
const NATIVE_AI_CM_PERCENT_MULTIPLIER_BITS: u64 = 0x3F84_7AE1_47AE_147B;

/// gamemd-derived AI opening grant: `ftol(MultiplayerAICM[diff] * 0.01 * money)`.
///
/// Active YR `ScenarioClass__Post_Map_Init @ 0x00686A52..0x00686A6B`:
/// `FILD MultiplayerAICM[house+0x184]; FMUL qword [0x007E3808] (0.01);
/// FIMUL money; CALL Math__ftol @ 0x007C5F00`. The process control word is
/// `0x0E7F` (RC = chop, PC = 53): `WinMain @ 0x006BBFC1` calls
/// `_controlfp(0x300, 0x300)` (`_RC_CHOP`) and `0x007C5EE4` captures the word
/// at `0x00822D80` for `ftol`. Both multiplies therefore truncate toward zero at
/// 53 bits and the final conversion truncates; plain f64 round-to-nearest
/// differs (e.g. `3 * 0.01 * 100` chops to 2, not 3). `Math__ftol` returns the
/// low dword of `FISTP qword`, so the i64 result is narrowed with `as i32`; an
/// out-of-range operand stores the x87 integer indefinite (low dword 0).
pub(crate) fn native_ai_opening_grant(coefficient: i32, money: i32) -> i32 {
    let percent = X87Chop53::load_f64(NativeF64Bits::from_bits(
        NATIVE_AI_CM_PERCENT_MULTIPLIER_BITS,
    ))
    .expect("0.01 is a finite normal double");
    let scaled = X87Chop53::mul(X87Chop53::load_i32(coefficient), percent);
    let product = X87Chop53::mul(scaled, X87Chop53::load_i32(money));
    X87Chop53::ftol_i32_low_masked(product)
}

/// Apply the generated skirmish AI opening-credit grant at the Post_Map_Init handoff.
///
/// Active YR `ScenarioClass__Post_Map_Init @ 0x00686890`, loop `0x006869BE..
/// 0x00686A7E`: for each house with `+0x1EC == 0` (not human) whose HouseType
/// `+0x1A6 == 0` (not `MultiplayPassive`), it calls the house secondary vslot
/// `+0x18` (`0x004F6990`: `ftol(storage_total * factor + credits)`, storage is
/// empty for generated Battle houses so this is the current balance), then
/// computes [`native_ai_opening_grant`] and passes the result to
/// `HouseClass__Add_Credits @ 0x004F9950`. Stock `MultiplayerAICM=400,0,0`
/// therefore gives a Hard AI 5x its balance and leaves Normal/Easy unchanged.
///
/// Sequencing: `Generate_Starting_Units @ 0x005D6D80` is called earlier in the
/// same function (`0x00686937`), so the balance multiplied here already carries
/// the leftover starting-unit budget credited by `seed_starting_extra_units`
/// (pure integer arithmetic on the native side: `CDQ/SAR/IDIV` budget thirds
/// and integer cost subtraction, no x87 involvement).
pub(crate) fn apply_skirmish_ai_opening_credits(sim: &mut Simulation, rules: &RuleSet) {
    for ai in &sim.ai_players {
        let Some(house) = sim.houses.get_mut(&ai.owner) else {
            continue;
        };
        if house.is_human || house.multiplay_passive {
            continue;
        }
        let coefficient = rules
            .general
            .multiplayer_ai_cm
            .get(house.difficulty.table_index())
            .copied()
            .unwrap_or(0);
        let grant = native_ai_opening_grant(coefficient, house.economy.credits);
        house.economy.credits = house.economy.credits.wrapping_add(grant);
    }
}

fn normalize_house_key(name: &str) -> String {
    name.trim().to_ascii_uppercase()
}

pub(crate) fn launch_alliance_map(
    house_roster: &HouseRoster,
    slots: &[NormalizedSkirmishSlot],
    mode: &crate::skirmish_launch::SkirmishLaunchMode,
) -> crate::map::houses::HouseAllianceMap {
    let mut alliances = house_roster.alliance_map();
    for slot in slots {
        alliances
            .entry(normalize_house_key(&slot.owner_name))
            .or_default();
    }
    for left in slots {
        let LaunchTeam::Team(team) = left.team else {
            continue;
        };
        for right in slots {
            if left.owner_name == right.owner_name || right.team != LaunchTeam::Team(team) {
                continue;
            }
            let left_key = normalize_house_key(&left.owner_name);
            let right_key = normalize_house_key(&right.owner_name);
            alliances
                .entry(left_key.clone())
                .or_default()
                .insert(right_key.clone());
            alliances.entry(right_key).or_default().insert(left_key);
        }
    }
    if mode.override_file.eq_ignore_ascii_case("MPCoopMD.ini") {
        for left in slots {
            for right in slots {
                if left.owner_name == right.owner_name || left.is_human != right.is_human {
                    continue;
                }
                let left_key = normalize_house_key(&left.owner_name);
                let right_key = normalize_house_key(&right.owner_name);
                alliances
                    .entry(left_key.clone())
                    .or_default()
                    .insert(right_key.clone());
                alliances.entry(right_key).or_default().insert(left_key);
            }
        }
    }
    alliances
}

pub(crate) const STARTING_MCV_FACING: u8 = 64;
pub(crate) const STARTING_MCV_FALLBACK_MAX_RADIUS: i32 = 31;
pub(crate) const STARTING_EXTRA_UNIT_FALLBACK_START_RADIUS: i32 = 4;
pub(crate) const STARTING_MCV_FALLBACK_DIRECTIONS: &[(i32, i32)] = &[
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

fn place_starting_mcv(
    sim: &mut Simulation,
    mcv_type: &str,
    owner: &str,
    base_rx: u16,
    base_ry: u16,
    bounds: NativeStartBounds,
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    resolved_terrain: &ResolvedTerrainGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<u64> {
    place_starting_object_near_base(
        sim,
        mcv_type,
        owner,
        base_rx,
        base_ry,
        STARTING_MCV_FACING,
        1,
        bounds,
        rules,
        height_map,
        resolved_terrain,
        overlay_registry,
    )
}

fn place_starting_object_near_base(
    sim: &mut Simulation,
    type_id: &str,
    owner: &str,
    base_rx: u16,
    base_ry: u16,
    facing: u8,
    start_radius: i32,
    bounds: NativeStartBounds,
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    resolved_terrain: &ResolvedTerrainGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<u64> {
    let category = rules.object(type_id)?.category;
    let initial_z = height_map.get(&(base_rx, base_ry)).copied().unwrap_or(0);
    // Active retail constructs one Techno before exact/fallback Unlimbo. The
    // one constructor draw therefore precedes every placement-search draw and
    // remains spent even when every attempt fails.
    let stable_id = sim.construct_object_limbo_at_height(
        type_id, owner, base_rx, base_ry, facing, initial_z, rules,
    )?;
    if starting_object_cell_placeable(sim, resolved_terrain, base_rx, base_ry, category) {
        if sim
            .reveal_constructed_object_at_height_with_unit_context(
                stable_id,
                base_rx,
                base_ry,
                facing,
                initial_z,
                PlacementEvidence::EvaluateMark,
                rules,
                overlay_registry,
                stable_id,
            )
            .is_some()
        {
            return Some(stable_id);
        }
    }

    for radius in start_radius..=STARTING_MCV_FALLBACK_MAX_RADIUS {
        let start_direction = sim.scenario_rng.next_range_u32_inclusive(0, 7) as usize;
        for jitter_pass in 0..2 {
            for offset in 0..8 {
                let direction = (start_direction + offset) & 7;
                let (dx, dy) = STARTING_MCV_FALLBACK_DIRECTIONS[direction];
                let (mut rx, mut ry) = bounds.clamp(
                    i32::from(base_rx) + dx * radius,
                    i32::from(base_ry) + dy * radius,
                );

                if jitter_pass != 0 {
                    let x_amount = sim.scenario_rng.next_range_u32_inclusive(0, 1) as i32;
                    let x_sign = sim.scenario_rng.next_range_u32_inclusive(0, 99);
                    let y_amount = sim.scenario_rng.next_range_u32_inclusive(0, 1) as i32;
                    let y_sign = sim.scenario_rng.next_range_u32_inclusive(0, 99);
                    let jittered = bounds.clamp(
                        i32::from(rx) + if x_sign < 50 { x_amount } else { -x_amount },
                        i32::from(ry) + if y_sign < 50 { y_amount } else { -y_amount },
                    );
                    rx = jittered.0;
                    ry = jittered.1;
                }

                if (rx, ry) == (base_rx, base_ry)
                    || !starting_object_cell_placeable(sim, resolved_terrain, rx, ry, category)
                {
                    continue;
                }
                let z = height_map.get(&(rx, ry)).copied().unwrap_or(0);
                if sim
                    .reveal_constructed_object_at_height_with_unit_context(
                        stable_id,
                        rx,
                        ry,
                        facing,
                        z,
                        PlacementEvidence::EvaluateMark,
                        rules,
                        overlay_registry,
                        stable_id,
                    )
                    .is_some()
                {
                    return Some(stable_id);
                }
            }
        }
    }

    sim.discard_constructed_limbo(stable_id);
    None
}

#[derive(Debug, Clone)]
pub(crate) struct StartingUnitCandidate {
    pub(crate) type_id: String,
    pub(crate) cost: i32,
}

pub(crate) const STARTING_EXTRA_UNIT_MAX_PLACEMENT_FAILURES: u8 = 20;

pub(crate) fn starting_unit_prefers_vehicle(spent: i32, budget: i32) -> bool {
    // gamemd 0x005D7337..0x005D7349 compares remaining budget against
    // trunc(initial_budget / 3). Expressed through `spent`, the strict
    // boundary is initial_budget - trunc(initial_budget / 3).
    spent < budget.wrapping_sub(budget / 3)
}

#[cfg(test)]
pub(crate) fn seed_starting_extra_units(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    resolved_terrain: &ResolvedTerrainGrid,
    bounds: NativeStartBounds,
    unit_count: i32,
    initial_veteran: bool,
) -> u32 {
    seed_starting_extra_units_with_overlay_registry(
        sim,
        slots,
        rules,
        height_map,
        resolved_terrain,
        bounds,
        unit_count,
        initial_veteran,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn seed_starting_extra_units_with_overlay_registry(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    resolved_terrain: &ResolvedTerrainGrid,
    bounds: NativeStartBounds,
    unit_count: i32,
    initial_veteran: bool,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> u32 {
    if unit_count <= 0 {
        return 0;
    }
    let budget = starting_unit_budget(
        rules,
        slots,
        sim.session.game_options.tech_level,
        unit_count,
    );
    if budget <= 0 {
        return 0;
    }
    let mut spawned = 0;

    for slot in slots {
        let Some(base_center) = crate::sim::house_state::house_state_for_owner(
            &sim.houses,
            &slot.owner_name,
            &sim.interner,
        )
        .and_then(|house| house.base_center) else {
            continue;
        };
        let candidates = starting_unit_candidates_for_country(
            rules,
            slot.country,
            sim.session.game_options.tech_level,
        );
        if candidates.vehicles.is_empty() && candidates.infantry.is_empty() {
            continue;
        }
        let mut spent = 0i32;
        let mut placement_failures_left = STARTING_EXTRA_UNIT_MAX_PLACEMENT_FAILURES;
        while spent < budget && placement_failures_left != 0 {
            let prefer_vehicle = starting_unit_prefers_vehicle(spent, budget);
            let preferred = if prefer_vehicle {
                &candidates.vehicles
            } else {
                &candidates.infantry
            };
            let pool = if preferred.is_empty() {
                if prefer_vehicle {
                    &candidates.infantry
                } else {
                    break;
                }
            } else {
                preferred
            };
            let candidate_index =
                sim.scenario_rng
                    .next_range_u32_inclusive(0, pool.len() as u32 - 1) as usize;
            let candidate = &pool[candidate_index];
            let Some(stable_id) = place_starting_object_near_base(
                sim,
                &candidate.type_id,
                &slot.owner_name,
                base_center.0,
                base_center.1,
                STARTING_MCV_FACING,
                STARTING_EXTRA_UNIT_FALLBACK_START_RADIUS,
                bounds,
                rules,
                height_map,
                resolved_terrain,
                overlay_registry,
            ) else {
                placement_failures_left -= 1;
                continue;
            };

            // gamemd-derived: the starting-unit generator tests SpecialFlags
            // bit 9 (`InitialVeteran`, `TEST AH,0x2` on `[g_Scenario]` at
            // `0x005D73FD`) and calls `VeterancyStruct::SetElite(1) @
            // 0x007500B0` on the unit — 2.0f, ELITE, with no `Trainable=`
            // gate. Seeding the raw accumulator too is what keeps the unit
            // elite through its first kill instead of demoting it.
            if initial_veteran {
                if let Some(entity) = sim.entities_mut().get_mut(stable_id) {
                    crate::sim::combat::veterancy::set_elite(entity);
                }
            }
            let mission = if slot.is_human {
                MissionType::Guard
            } else {
                MissionType::AreaGuard
            };
            let _ = sim.mission_assign_exact(
                stable_id,
                MissionId::from_known(mission),
                sim.session.binary_frame,
            );
            spawned += 1;
            spent = spent.wrapping_add(candidate.cost);
        }
        // gamemd-derived: `MultiplayerGameMode__Generate_Starting_Units @
        // 0x005D6D80` hands each non-MultiplayPassive house its own copy of the
        // budget by reference; the extra-unit callback (`0x005D70F0`) subtracts
        // each placed unit's cost at `0x005D73EF..0x005D73F3`, and the caller
        // then reads the remainder at `0x005D6F91` and, when it is still > 0,
        // calls `HouseClass__Add_Credits @ 0x004F9950` (`0x005D6F99..0x005D6F9C`).
        // Human houses are included; the starting MCV is not charged.
        let remaining = budget.wrapping_sub(spent);
        if remaining > 0
            && let Some(house) = crate::sim::house_state::house_state_for_owner_mut(
                &mut sim.houses,
                &slot.owner_name,
                &sim.interner,
            )
            && !house.multiplay_passive
        {
            house.economy.credits = house.economy.credits.wrapping_add(remaining);
        }
    }

    spawned
}

pub(crate) fn starting_unit_budget(
    rules: &RuleSet,
    slots: &[NormalizedSkirmishSlot],
    tech_level: i32,
    unit_count: i32,
) -> i32 {
    if unit_count <= 0 {
        return 0;
    }
    let mut total_cost: i32 = 0;
    let mut eligible_count: i32 = 0;
    for id in &rules.vehicle_ids {
        let Some(object) = rules.object(id) else {
            continue;
        };
        if !object.allowed_to_start_in_multiplayer
            || object.tech_level > tech_level
            || !slots
                .iter()
                .any(|slot| launch_country_can_own_object(slot.country, object))
            || rules
                .general
                .base_unit_types
                .iter()
                .any(|base_unit| base_unit.eq_ignore_ascii_case(&object.id))
        {
            continue;
        }
        eligible_count += 1i32;
        total_cost = total_cost.wrapping_add(object.cost);
    }
    for id in &rules.infantry_ids {
        let Some(object) = rules.object(id) else {
            continue;
        };
        if !object.allowed_to_start_in_multiplayer
            || object.tech_level > tech_level
            || !slots
                .iter()
                .any(|slot| launch_country_can_own_object(slot.country, object))
        {
            continue;
        }
        eligible_count += 1i32;
        total_cost = total_cost.wrapping_add(object.cost);
    }
    eligible_count = eligible_count.max(1);
    (eligible_count / 2)
        .wrapping_add(total_cost)
        .wrapping_div(eligible_count)
        .wrapping_mul(unit_count)
}

#[derive(Debug, Default)]
pub(crate) struct StartingUnitCandidates {
    pub(crate) vehicles: Vec<StartingUnitCandidate>,
    pub(crate) infantry: Vec<StartingUnitCandidate>,
}

pub(crate) fn starting_unit_candidates_for_country(
    rules: &RuleSet,
    country: LaunchCountry,
    tech_level: i32,
) -> StartingUnitCandidates {
    let vehicles = rules
        .vehicle_ids
        .iter()
        .filter_map(|id| {
            let object = rules.object(id)?;
            if !starting_unit_vehicle_allowed(rules, object, tech_level)
                || !launch_country_can_own_object(country, object)
            {
                return None;
            }
            Some(StartingUnitCandidate {
                type_id: id.clone(),
                cost: object.cost,
            })
        })
        .collect();
    let infantry = rules
        .infantry_ids
        .iter()
        .filter_map(|id| {
            let object = rules.object(id)?;
            if !starting_unit_infantry_allowed(object, tech_level)
                || !launch_country_can_own_object(country, object)
            {
                return None;
            }
            Some(StartingUnitCandidate {
                type_id: id.clone(),
                cost: object.cost,
            })
        })
        .collect();
    StartingUnitCandidates { vehicles, infantry }
}

fn starting_unit_vehicle_allowed(
    rules: &RuleSet,
    object: &crate::rules::object_type::ObjectType,
    tech_level: i32,
) -> bool {
    object.allowed_to_start_in_multiplayer
        && object.tech_level <= tech_level
        && !rules
            .general
            .base_unit_types
            .iter()
            .any(|base_unit| base_unit.eq_ignore_ascii_case(&object.id))
}

fn starting_unit_infantry_allowed(
    object: &crate::rules::object_type::ObjectType,
    tech_level: i32,
) -> bool {
    object.allowed_to_start_in_multiplayer && object.tech_level <= tech_level
}

fn starting_object_cell_placeable(
    sim: &Simulation,
    resolved_terrain: &ResolvedTerrainGrid,
    rx: u16,
    ry: u16,
    category: crate::rules::object_type::ObjectCategory,
) -> bool {
    // Acceptance is the retail isometric-diamond predicate: the mode's
    // starting-unit creator (vtable `+0xC8` @ `0x005D7030`) unlimbos at the
    // start coord, and its scan-place fallback @ `0x00688ED0` gates the seed
    // cell and every ring candidate with `MapClass::Is_Cell_In_Playfield(cell,
    // 1)`. The cell-array rect only clamps probe coordinates — it is never an
    // acceptance test.
    if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
        (i32::from(rx), i32::from(ry)),
        sim.playfield_bounds,
        Some(resolved_terrain),
    ) {
        return false;
    }
    if let Some(occupancy) = sim.occupancy().get(rx, ry) {
        let compatible_infantry = category == crate::rules::object_type::ObjectCategory::Infantry
            && occupancy
                .occupants
                .first()
                .and_then(|occupant| sim.entities().get(occupant.entity_id))
                .is_some_and(|entity| entity.category == EntityCategory::Infantry)
            && occupancy
                .occupants
                .iter()
                .filter(|occupant| occupant.sub_cell.is_some())
                .count()
                < 5;
        if !compatible_infantry {
            return false;
        }
    }
    let Some(cell) = resolved_terrain.cell(rx, ry) else {
        return false;
    };
    if cell.overlay_blocks || cell.terrain_object_blocks {
        return false;
    }
    if cell.has_bridge_deck && cell.bridge_walkable {
        return true;
    }
    cell.zone_type == crate::map::resolved_terrain::zone_class::GROUND
}

pub(crate) fn launch_mcv_type_for_country<'a>(
    country: LaunchCountry,
    rules: &'a RuleSet,
) -> &'a str {
    rules
        .general
        .base_unit_types
        .iter()
        .find_map(|id| {
            let object = rules.object(id)?;
            launch_country_can_own_object(country, object).then_some(id.as_str())
        })
        .or_else(|| {
            rules
                .general
                .base_unit_types
                .iter()
                .find(|id| rules.object(id).is_some())
                .map(String::as_str)
        })
        .unwrap_or("AMCV")
}

fn launch_country_can_own_object(
    country: LaunchCountry,
    object: &crate::rules::object_type::ObjectType,
) -> bool {
    let country_name = country.country_name();
    if !object.owner.iter().any(|owner| owner == country_name) {
        return false;
    }
    if !object.required_houses.is_empty()
        && !object
            .required_houses
            .iter()
            .any(|required| required == country_name)
    {
        return false;
    }
    !object
        .forbidden_houses
        .iter()
        .any(|forbidden| forbidden == country_name)
}

/// Opaque owner for the RNG cursors used while a map becomes a world.
///
/// The app may drive content-loading algorithms through the two narrow draw
/// wrappers and may transport the launch-generated MapGen continuation, but it
/// cannot replace, extract, or draw from those cursors. Consuming this owner is
/// the only production handoff into a freshly constructed Simulation.
pub(crate) struct ScenarioBootstrapRng {
    scenario: SimRng,
    main: SimRng,
    mapgen: Option<SimRng>,
}

/// Scenario-stream access granted only to the terrain Fill callback.
pub(crate) struct ScenarioFillRng<'a> {
    rng: &'a mut SimRng,
}

impl ScenarioFillRng<'_> {
    pub(crate) fn next_range_u32_inclusive(&mut self, low: u32, high: u32) -> u32 {
        self.rng.next_range_u32_inclusive(low, high)
    }
}

/// Main-stream access granted only to one-time TMP variant-table generation.
pub(crate) struct VariantMainRng<'a> {
    rng: &'a mut SimRng,
}

impl VariantMainRng<'_> {
    pub(crate) fn next_u32(&mut self) -> u32 {
        self.rng.next_u32()
    }
}

impl ScenarioBootstrapRng {
    pub(crate) fn new(seed: u32) -> Self {
        let seed = u64::from(seed);
        Self {
            scenario: SimRng::new(seed),
            main: SimRng::new(seed),
            mapgen: None,
        }
    }

    /// Install the accepted random map's process-global cursor exactly once.
    pub(crate) fn install_generated_mapgen_continuation(
        &mut self,
        continuation: MapGenRngContinuation,
    ) {
        assert!(
            self.mapgen.is_none(),
            "generated MapGen continuation may only be installed once"
        );
        self.mapgen = Some(SimRng::from_mapgen_continuation(continuation));
    }

    /// Install the already-resolved stock-offline pre-Fill prefix exactly once.
    #[cfg(test)]
    pub(crate) fn install_pre_fill_scenario_prefix_plan(
        &mut self,
        plan: PreFillScenarioPrefixPlan,
    ) -> Result<StockOfflinePrefixProjection, PreFillScenarioPrefixPlanError> {
        plan.install_before_terrain(&mut self.scenario)
    }

    /// Consume the paired stock-offline RNG/native-ID prefix into the one staged
    /// Simulation before terrain Fill.
    pub(crate) fn into_stock_offline_staged_simulation(
        mut self,
        descriptor: &ScenarioDescriptor,
        bound_prefix: BoundStockOfflineFreshPrefixPlan,
    ) -> Result<(Simulation, StockOfflinePrefixProjection), PreFillScenarioPrefixPlanError> {
        let (scenario_prefix, native_id_prefix) = bound_prefix.into_parts();
        let projection = scenario_prefix.install_before_terrain(&mut self.scenario)?;
        let mut simulation = self.into_simulation(descriptor);
        simulation.native_unique_ids = Some(native_id_prefix.into_cursor());
        Ok((simulation, projection))
    }

    #[cfg(test)]
    pub(crate) fn logical_states_for_test(
        &self,
    ) -> (
        crate::sim::rng::SimRngLogicalState,
        crate::sim::rng::SimRngLogicalState,
        Option<crate::sim::rng::SimRngLogicalState>,
    ) {
        (
            self.scenario.logical_state(),
            self.main.logical_state(),
            self.mapgen.as_ref().map(SimRng::logical_state),
        )
    }

    /// Finish the app-to-sim construction handoff with every bound cursor.
    pub(crate) fn into_simulation(self, descriptor: &ScenarioDescriptor) -> Simulation {
        let mut sim = Simulation::from_descriptor(descriptor);
        sim.install_terrain_load_advanced_scenario_rng(self.scenario);
        sim.install_variant_advanced_main_rng(self.main);
        if let Some(mapgen) = self.mapgen {
            sim.install_generated_mapgen_rng(mapgen);
        }
        sim
    }
}

fn replay_generated_construction_trace_with_rng(
    scenario: &mut SimRng,
    trace: &crate::map::construction_trace::RmgConstructionTrace,
) -> Result<crate::sim::world::GeneratedTechnoInitTable, crate::sim::world::GeneratedTechnoInitError>
{
    let mut emitted = Vec::new();
    for (expected_ordinal, event) in trace.events.iter().enumerate() {
        if event.ordinal != expected_ordinal {
            return Err(
                crate::sim::world::GeneratedTechnoInitError::TraceOrdinalMismatch {
                    expected: expected_ordinal,
                    found: event.ordinal,
                },
            );
        }
        let techno_ctor_random_word = (scenario.next_u32() & 0xFFFF) as u16;
        if let crate::map::construction_trace::RmgConstructionOutcome::Emitted {
            entity_index,
            cell,
        } = &event.outcome
        {
            emitted.push(crate::sim::game_entity::GeneratedTechnoInit {
                entity_index: *entity_index,
                techno_type: event.techno_type.clone(),
                cell: *cell,
                techno_ctor_random_word,
            });
        }
    }
    crate::sim::world::GeneratedTechnoInitTable::try_new(emitted)
}

impl Simulation {
    /// Borrow the two load-time streams from the already-staged gameplay owner.
    /// No second bootstrap owner can exist after `into_simulation` consumes it.
    pub(crate) fn terrain_load_draws(&mut self) -> (ScenarioFillRng<'_>, VariantMainRng<'_>) {
        (
            ScenarioFillRng {
                rng: &mut self.scenario_rng,
            },
            VariantMainRng {
                rng: &mut self.main_rng,
            },
        )
    }

    /// Replay accepted launch-generation constructor effects on the staged
    /// Scenario stream after Fill, before authored object-section projection.
    ///
    /// Consume the launch generation's ordered Building constructor trace on
    /// the same Scenario owner that already passed through the stock-offline
    /// prefix and terrain Fill. Successful rows retain their low word for later
    /// projection; discarded rows spend the word and deliberately bind none.
    ///
    /// gamemd provenance: TechnoClass constructor 0x006F3254 consumes one raw
    /// Scenario word before RMG placement can succeed or fail. Launch reader
    /// 0x00684620 regenerates the accepted `.SED` after match RNG reseeding.
    pub(crate) fn replay_staged_generated_construction_trace(
        &mut self,
        trace: &crate::map::construction_trace::RmgConstructionTrace,
    ) -> Result<
        crate::sim::world::GeneratedTechnoInitTable,
        crate::sim::world::GeneratedTechnoInitError,
    > {
        replay_generated_construction_trace_with_rng(&mut self.scenario_rng, trace)
    }

    #[cfg(test)]
    pub(crate) fn gather_native_start_positions(
        &mut self,
        waypoints: &HashMap<u32, Waypoint>,
        participant_count: usize,
        terrain: &ResolvedTerrainGrid,
        bounds: NativeStartBounds,
    ) -> Vec<Waypoint> {
        let map_size_height = self.playfield_size_height;
        let binary_frame = self.session.binary_frame;
        native_gather_start_positions(
            waypoints,
            participant_count,
            terrain,
            &self.substrate.occupancy,
            bounds,
            self.playfield_bounds,
            map_size_height,
            binary_frame,
            &mut self.scenario_rng,
        )
    }

    #[cfg(test)]
    pub(crate) fn assign_native_battle_starts(
        &mut self,
        session: &SkirmishLaunchSession,
        starts: &[Waypoint],
    ) -> NativeStartAssignment {
        native_assign_launch_starts(session, starts, &mut self.scenario_rng)
    }

    #[cfg(test)]
    pub(crate) fn assign_native_cooperative_starts(
        &mut self,
        session: &SkirmishLaunchSession,
        starts: &[Waypoint],
        human_start_spots: usize,
    ) -> NativeStartAssignment {
        native_assign_cooperative_launch_starts(
            session,
            starts,
            human_start_spots,
            &mut self.scenario_rng,
        )
    }
}

impl Simulation {
    /// Register an AI player for every playable roster house except the local
    /// owner (F10 boundary method: the app names the local owner, sim owns
    /// the writes). Neutral/civilian/special houses never receive AI.
    pub(crate) fn register_ai_players_from_roster(
        &mut self,
        house_roster: &HouseRoster,
        local_owner: &str,
    ) {
        use crate::sim::ai::AiPlayerState;

        for house in &house_roster.houses {
            let up = house.name.to_ascii_uppercase();
            if matches!(
                up.as_str(),
                "NEUTRAL" | "SPECIAL" | "CIVILIAN" | "GOODGUY" | "BADGUY" | "JP"
            ) {
                continue;
            }
            if house.name.eq_ignore_ascii_case(local_owner) {
                continue;
            }
            self.ai_players
                .push(AiPlayerState::new(self.interner.intern(&house.name)));
            log::info!("AI player registered: {}", house.name);
        }
    }

    /// Mark the named house human-controlled (F10 boundary method). Returns
    /// false when the owner is not interned or owns no house.
    pub(crate) fn mark_house_human(&mut self, owner: &str) -> bool {
        let Some(owner_id) = self.interner.get(owner) else {
            return false;
        };
        let Some(house) = self.houses.get_mut(&owner_id) else {
            return false;
        };
        house.is_human = true;
        true
    }
}

/// Campaign68ACAA chooses Basic.Player with a 20-byte ReadString buffer,
/// byte-exact House name lookup50C170, and index0 fallback on a miss.
/// Evidence: docs/research/PHASE3_CURRENT_HOUSE_IDENTITY_GHIDRA_REPORT.md.
/// Empty dev substrates retain unavailable identity; this is not a claim that
/// native can launch an empty House array.
pub(crate) fn initialize_campaign_current_house(
    sim: &mut Simulation,
    roster: &HouseRoster,
    ini: &crate::rules::ini_parser::IniFile,
) {
    let requested = ini
        .section("Basic")
        .map(|section| section.read_string("Player", "", 20))
        .unwrap_or_default();
    // Interning folds lookup case and preserves the first spelling it saw.
    // The native strcmp reads the original House definition instead.
    sim.session.current_house = sim
        .session
        .house_order
        .iter()
        .copied()
        .find(|id| {
            roster.houses.iter().any(|house| {
                house.name.as_bytes() == requested.as_bytes()
                    && sim.interner.get(&house.name) == Some(*id)
            })
        })
        .or_else(|| sim.session.house_order.first().copied());
    if let Some(owner) = sim.session.current_house {
        // Original 68AD0C/68AD18 set the selected House's +1EC/+1ED.
        // This is scenario construction, never a snapshot/viewer rebind.
        let house = sim
            .houses
            .get_mut(&owner)
            .expect("registered current House");
        house.is_human = true;
        house.player_control = true;
    }
}

/// Map-roster house construction shared by app and headless (F09):
/// native order requires houses before every object section.
#[cfg(test)]
pub(crate) fn initialize_map_roster_houses(
    sim: &mut Simulation,
    house_roster: &HouseRoster,
    rules: Option<&RuleSet>,
) {
    assert!(
        sim.houses.is_empty()
            && sim.session.house_order.is_empty()
            && sim.entities().is_empty()
            && sim.production.terrain_objects.is_empty(),
        "scenario houses must be initialized before map objects"
    );
    for house in &house_roster.houses {
        let fallback_side = crate::sim::house_state::side_index_from_name(house.side.as_deref());
        let side_idx = rules.map_or(fallback_side, |rules| {
            crate::sim::house_state::resolve_house_side_index(
                rules,
                house.country.as_deref(),
                house.side.as_deref(),
                fallback_side,
            )
        });
        let player_control = house.player_control == Some(true);
        let name_id = sim.interner.intern(&house.name);
        let country_id = house.country.as_deref().map(|c| sim.interner.intern(c));
        let mut house_state = crate::sim::house_state::HouseState::new(
            name_id,
            side_idx,
            country_id,
            false,
            sim.session.game_options.starting_credits,
            sim.session.game_options.tech_level,
        );
        house_state.player_control = player_control;
        house_state.base_plan.percent_built = house.base_plan.percent_built;
        house_state.base_plan.nodes = house
            .base_plan
            .nodes
            .iter()
            .map(|node| crate::sim::base_plan::BasePlanNode {
                type_or_control: node.type_or_control,
                packed_cell: node.packed_cell,
                filled: node.filled,
                retry_count: node.retry_count,
            })
            .collect();
        // HouseClass::Read_Scenario_INI reads `IQ=` from this exact named
        // house section, defaults it to zero, and changes a value above
        // MaxIQLevels to literal one before storing CurrentIQ (+0x24C).
        house_state.current_iq = rules.map_or_else(
            || house.iq.unwrap_or(0),
            |rules| house.scenario_current_iq(rules.general.max_iq_levels),
        );
        // MultiplayPassive lives on the country/house type. A roster section
        // with no `Country=` resolves through `[Countries]` entry zero.
        house_state.multiplay_passive =
            crate::sim::house_state::resolve_multiplay_passive(rules, house.country.as_deref());
        sim.houses.insert(name_id, house_state);
        sim.session.house_order.push(name_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campaign_current_house_uses_exact_case_and_registry_fallback() {
        let mut sim = Simulation::new();
        // Earlier interning must not become the original House name spelling.
        sim.interner.intern("alpha");
        let map = IniFile::from_str(
            "[Houses]\n7=Zulu\n2=Alpha\n9=1234567890123456789\n[Zulu]\n[Alpha]\n[1234567890123456789]\n",
        );
        let roster = crate::map::houses::parse_house_roster(&map, &[], None);
        initialize_map_roster_houses(&mut sim, &roster, None);
        let first = sim.interner.get("Zulu").unwrap();
        let second = sim.interner.get("Alpha").unwrap();
        let capped = sim.interner.get("1234567890123456789").unwrap();
        for (input, expected) in [
            ("", first),
            ("Player=missing", first),
            ("Player=alpha", first),
            ("Player=Alpha", second),
            ("Player=1234567890123456789suffix", capped),
            // The first 19 bytes end in whitespace that only the post-copy
            // trim removes; trimming the uncut source cannot remove it.
            ("Player=Alpha              suffix", second),
        ] {
            initialize_campaign_current_house(
                &mut sim,
                &roster,
                &IniFile::from_str(&format!("[Basic]\n{input}\n")),
            );
            assert_eq!(sim.session.current_house, Some(expected));
            assert!(sim.houses[&expected].is_human);
            assert!(sim.houses[&expected].player_control);
        }
        assert!(
            sim.houses[&first].is_human && sim.houses[&first].player_control,
            "selecting another House does not clear previously set flags"
        );
    }

    /// x87 chop (cw `0x0E7F`) versus f64 round-to-nearest for the AI opening
    /// grant: `3 * 0.01` chops just below `0.03`, `* 100` chops just below `3.0`,
    /// and `Math__ftol` truncates to 2 where `((3.0 * 0.01) * 100.0) as i32`
    /// gives 3. The stock `400,0,0 x 10000` case agrees under both modes.
    #[test]
    fn native_ai_opening_grant_chops_where_round_to_nearest_differs() {
        assert_eq!(
            ((3.0_f64 * 0.01) * 100.0) as i32,
            3,
            "round-to-nearest baseline"
        );
        assert_eq!(
            native_ai_opening_grant(3, 100),
            2,
            "chop at 53 bits then ftol"
        );
        assert_eq!(native_ai_opening_grant(3, 200), 5);
        assert_eq!(
            native_ai_opening_grant(400, 10_000),
            40_000,
            "stock Hard AI"
        );
        assert_eq!(native_ai_opening_grant(0, 10_000), 0, "stock Normal/Easy");
        assert_eq!(native_ai_opening_grant(7, 12_345), 864);
        assert_eq!(native_ai_opening_grant(400, 0), 0);
        assert_eq!(
            native_ai_opening_grant(-100, 10_000),
            -10_000,
            "negative chops toward zero"
        );
    }
    use crate::map::bridge_facts::BridgeCellFacts;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, zone_class};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};

    fn descriptor(seed: u32) -> ScenarioDescriptor {
        ScenarioDescriptor {
            seed,
            ..ScenarioDescriptor::default()
        }
    }

    fn one_player_battle_launch(selected_map_file: &str) -> MatchLaunchDescriptor {
        use crate::skirmish_launch::{
            PreFillHouseRoster, SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLocalSlot,
        };

        MatchLaunchDescriptor::from_resolved(SkirmishLaunchSession {
            mode: SkirmishLaunchMode {
                id: 1,
                ui_name_key: "GUI:Battle".to_string(),
                tooltip_key: "STT:ModeBattle".to_string(),
                override_file: "MPBattleMD.ini".to_string(),
                map_filter: "standard".to_string(),
                random_maps_allowed: true,
                allies_allowed: true,
                must_ally: false,
            },
            selected_map_file: Some(selected_map_file.to_string()),
            player_name: "Player".to_string(),
            local: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: false,
                color_index: 0,
                color_random: false,
                start_position: LaunchStartPosition::Auto,
                team: LaunchTeam::None,
            },
            opponents: Vec::new(),
            pre_fill_house_roster: PreFillHouseRoster::from_compact_skirmish(0),
            options: SkirmishLaunchOptions::default(),
        })
        .expect("one-player Battle fixture is fully resolved")
    }

    fn prefix_map_with_starts(starts: &[Waypoint]) -> MapFile {
        MapFile {
            header: crate::map::map_file::MapHeader {
                theater: "TEMPERATE".to_string(),
                fill: "Clear".to_string(),
                level: 0,
                width: 44,
                height: 52,
                local_left: 2,
                local_top: 5,
                local_width: 40,
                local_height: 40,
            },
            basic: Default::default(),
            briefing: Default::default(),
            preview: Default::default(),
            cells: Vec::new(),
            iso_map_pack_lookups: Vec::new(),
            entities: Vec::new(),
            overlays: Vec::new(),
            authored_overlay_packs: crate::map::map_file::AuthoredOverlayPackSlot::empty(),
            smudges: Vec::new(),
            terrain_objects: Vec::new(),
            waypoints: starts
                .iter()
                .copied()
                .map(|start| (start.index, start))
                .collect(),
            cell_tags: Default::default(),
            tags: Default::default(),
            triggers: Default::default(),
            events: Default::default(),
            actions: Default::default(),
            local_variables: Default::default(),
            trigger_graph: Default::default(),
            special_flags: Default::default(),
            explicit_tubes: Vec::new(),
            ini: IniFile::from_str(""),
        }
    }

    fn one_start_prefix_map(start: Waypoint) -> MapFile {
        prefix_map_with_starts(&[start])
    }

    #[test]
    fn current_house_precedes_map_objects_and_survives_failed_starting_mcv() {
        // Exercise the same staged population and pre-Fill launch projection
        // used by the offline app. Blocking every candidate after map-object
        // creation supplies a deterministic total placement failure; it does
        // not replace the production MCV constructor/search/result branch.
        let rules_ini = IniFile::from_str(
            "[General]\nBaseUnit=MTNK\n[VehicleTypes]\n0=MTNK\n\
             [BuildingTypes]\n0=CABHUT\n\
             [MTNK]\nStrength=300\nSpeed=6\nCost=100\nOwner=Americans\n\
             [CABHUT]\nStrength=100\nFoundation=1x1\nOwner=Neutral\n",
        );
        let start = Waypoint {
            index: 0,
            rx: 40,
            ry: 40,
        };
        let mut map = one_start_prefix_map(start);
        map.entities = crate::map::entities::parse_map_entities(&IniFile::from_str(
            "[Structures]\n0=Neutral,CABHUT,256,30,30,0,None\n",
        ));
        let roster = HouseRoster::default();
        let overlays = OverlayTypeRegistry::empty();
        for (handle, with_ai, owner_key) in [
            ("Player", false, "Player"),
            ("Commander", false, "Commander"),
            ("Neutral", false, "Player"),
            ("sPeCiAl", true, "Player"),
            ("COMPUTER1", true, "Player"),
            ("Computer2", true, "Computer2"),
            ("Computer01", true, "Computer01"),
        ] {
            let mut launch = one_player_battle_launch("current-house.map");
            launch.session.player_name = handle.to_string();
            launch.session.options.bases = true;
            launch.session.options.unit_count = 0;
            if with_ai {
                launch
                    .session
                    .opponents
                    .push(crate::skirmish_launch::SkirmishAiSlot {
                        country: LaunchCountry::Russia,
                        country_random: false,
                        color_index: 1,
                        color_random: false,
                        start_position: LaunchStartPosition::Auto,
                        team: LaunchTeam::None,
                        difficulty: crate::skirmish_launch::AiDifficulty::Easy,
                    });
                launch.session.pre_fill_house_roster = PreFillHouseRoster::from_compact_skirmish(1);
                map.waypoints.insert(
                    1,
                    Waypoint {
                        index: 1,
                        rx: 50,
                        ry: 50,
                    },
                );
            } else {
                map.waypoints.remove(&1);
            }
            for block_placement in [false, true] {
                let mut scenario = descriptor(0xA83D4C);
                scenario.game_mode_nonzero = true;
                scenario.map_width = 96;
                scenario.map_height = 96;
                scenario.mp_start_waypoints.insert(0, (40, 40));
                let plan = prepare_stock_offline_scenario_prefix_plan(
                    &launch,
                    &map,
                    &map.waypoints,
                    scenario.seed,
                )
                .expect("admitted offline launch prefix");
                let mut rules_owner =
                    crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
                        rules_ini.clone(),
                        None,
                        IniFile::from_str(""),
                    )
                    .expect("cold process Rules authority");
                let (rules, _, _, rules_receipt) = rules_owner
                    .load_noncampaign_scenario(None, &map.ini)
                    .expect("production noncampaign Rules reset/rebuild")
                    .into_parts();
                let bound_prefix = plan.bind_native_rules_receipt(rules_receipt);
                let (mut sim, projection) = ScenarioBootstrapRng::new(scenario.seed)
                    .into_stock_offline_staged_simulation(&scenario, bound_prefix)
                    .expect("production admission installs the paired RNG/native-ID prefix");
                assert!(sim.native_unique_ids.is_some());
                let mut terrain = techno_constructor_flat_start_terrain(96);
                let overlay_grid = crate::sim::overlay_grid::OverlayGrid::new(96, 96);
                crate::sim::runtime::populate_staged_scenario_with_generated_inits(
                    &mut sim,
                    &map,
                    &terrain,
                    "TEMPERATE",
                    Some(&rules),
                    &BTreeMap::new(),
                    Some(&overlays),
                    Some(&overlay_grid),
                    crate::map::basic::BridgeDestroyabilityMode::SkirmishOrMultiplayer {
                        bridge_destruction: true,
                    },
                    &scenario,
                    None,
                    |sim| {
                        initialize_skirmish_launch_houses(sim, &roster, &rules, &launch);
                        assert!(
                            sim.entities().is_empty(),
                            "binding precedes every map object"
                        );
                        let player = sim.interner.get(owner_key).unwrap();
                        assert_eq!(sim.session.current_house, Some(player));
                        assert_eq!(sim.session.house_order[0], player);
                        assert!(sim.houses[&player].is_human && sim.houses[&player].player_control);
                    },
                )
                .expect("shared staged object construction");
                assert_eq!(sim.entities().len(), 1, "map object actually constructed");
                let player = sim.interner.get(owner_key).unwrap();
                assert_eq!(sim.session.current_house, Some(player));
                if block_placement {
                    let mut cells = terrain.cells().to_vec();
                    for cell in &mut cells {
                        cell.overlay_blocks = true;
                    }
                    terrain = ResolvedTerrainGrid::from_cells(96, 96, cells);
                    sim.install_resolved_terrain_for_new_map(terrain.clone());
                }
                let result = apply_pre_fill_scenario_prefix_launch_session_with_overlay_registry(
                    &mut sim,
                    &map,
                    &roster,
                    &rules,
                    &BTreeMap::new(),
                    &terrain,
                    &launch,
                    &overlays,
                    &projection,
                );
                assert_eq!(
                    result.spawned_mcvs,
                    if block_placement {
                        0
                    } else {
                        1 + u32::from(with_ai)
                    }
                );
                assert_eq!(
                    result.local_owner.as_deref(),
                    (!block_placement).then_some(owner_key)
                );
                assert_eq!(
                    sim.entities().len(),
                    if block_placement {
                        1
                    } else {
                        2 + usize::from(with_ai)
                    }
                );
                assert_eq!(sim.session.current_house, Some(player));
                assert!(sim.houses[&player].is_human && sim.houses[&player].player_control);
                assert_eq!(launch.session.player_name, handle);
                let mut distinct = sim.session.house_order.clone();
                distinct.sort();
                distinct.dedup();
                assert_eq!(distinct.len(), 3 + usize::from(with_ai));
                assert_eq!(sim.houses.len(), distinct.len());
                for name in ["Neutral", "Special"]
                    .into_iter()
                    .chain(with_ai.then_some("Computer1"))
                {
                    let other = sim.interner.get(name).unwrap();
                    assert_ne!(player, other);
                    assert!(!sim.houses[&other].is_human && !sim.houses[&other].player_control);
                }
                let neutral = sim.interner.get("Neutral").unwrap();
                let mut expected_mcv_owners = Vec::new();
                if !block_placement {
                    expected_mcv_owners.push(player);
                    if with_ai {
                        expected_mcv_owners.push(sim.interner.get("Computer1").unwrap());
                    }
                }
                expected_mcv_owners.sort();
                let assert_entity_owners = |world: &Simulation| {
                    let hut_owners: Vec<_> = world
                        .entities()
                        .values()
                        .filter(|entity| world.interner.resolve(entity.type_ref) == "CABHUT")
                        .map(|entity| entity.owner)
                        .collect();
                    assert_eq!(
                        hut_owners,
                        [neutral],
                        "authored hut keeps its Neutral owner"
                    );
                    let mut mcv_owners: Vec<_> = world
                        .entities()
                        .values()
                        .filter(|entity| world.interner.resolve(entity.type_ref) == "MTNK")
                        .map(|entity| entity.owner)
                        .collect();
                    mcv_owners.sort();
                    assert_eq!(
                        mcv_owners, expected_mcv_owners,
                        "each successful MCV belongs to its distinct participant; failures leave none"
                    );
                };
                assert_entity_owners(&sim);
                let bytes =
                    crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "owner collision", 0);
                let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
                    .unwrap()
                    .sim;
                restored
                    .restore_after_snapshot_load()
                    .expect("valid distinct House registry");
                assert_eq!(restored.session.current_house, Some(player));
                assert_eq!(restored.session.house_order, sim.session.house_order);
                assert!(
                    restored.houses[&player].is_human && restored.houses[&player].player_control
                );
                assert_entity_owners(&restored);
            }
        }
    }

    fn techno_constructor_start_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n\n\
             [VehicleTypes]\n0=MTNK\n1=HTNK\n\n\
             [AircraftTypes]\n\n\
             [BuildingTypes]\n\n\
             [MTNK]\nStrength=300\nSpeed=6\nCost=100\nTechLevel=1\nOwner=Americans\nAllowedToStartInMultiplayer=yes\n\n\
             [HTNK]\nStrength=400\nSpeed=5\nCost=100\nTechLevel=1\nOwner=Americans\nAllowedToStartInMultiplayer=yes\n",
        ))
        .expect("starting-object constructor rules")
    }

    fn techno_constructor_flat_start_terrain(size: u16) -> ResolvedTerrainGrid {
        let land_type = LandType::Clear.as_index();
        let speed_costs = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            float: Some(100),
            amphibious: Some(100),
            float_beach: Some(100),
            hover: Some(100),
        };
        let mut cells = Vec::with_capacity(usize::from(size) * usize::from(size));
        for ry in 0..size {
            for rx in 0..size {
                cells.push(ResolvedTerrainCell {
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
                    land_type,
                    yr_cell_land_type: land_type,
                    slope_type: 0,
                    template_height: 0,
                    height_in_pixels: 0,
                    render_offset_x: 0,
                    render_offset_y: 0,
                    terrain_class: TerrainClass::Clear,
                    speed_costs,
                    is_water: false,
                    is_cliff_like: false,
                    is_rough: false,
                    is_road: false,
                    accepts_smudge: true,
                    allows_tiberium: true,
                    variant: 0,
                    has_ramp: false,
                    canonical_ramp: None,
                    ground_walk_blocked: false,
                    terrain_object_blocks: false,
                    terrain_object_occupation: None,
                    overlay_blocks: false,
                    overlay_zone_type: None,
                    outside_playfield: false,
                    zone_type: zone_class::GROUND,
                    base_ground_walk_blocked: false,
                    base_build_blocked: false,
                    base_land_type: land_type,
                    base_yr_cell_land_type: land_type,
                    base_terrain_class: TerrainClass::Clear,
                    base_speed_costs: speed_costs,
                    build_blocked: false,
                    has_bridge_deck: false,
                    bridge_walkable: false,
                    bridge_transition: false,
                    bridge_deck_level: 0,
                    bridge_layer: None,
                    bridge_facts: BridgeCellFacts::default(),
                    tube_index: None,
                    radar_left: [0; 3],
                    radar_right: [0; 3],
                    has_damaged_data: false,
                    bridgehead_anchor_class_at_load: None,
                });
            }
        }
        ResolvedTerrainGrid::from_cells(size, size, cells)
    }

    fn techno_constructor_start_sim(seed: u64, size: u16) -> Simulation {
        let mut sim = Simulation::with_seed(seed);
        sim.session.map_width = size;
        sim.session.map_height = size;
        sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
            base: i32::from(size),
            off_fc: 0,
            off_100: 0,
            off_104: i32::from(size),
            off_108: i32::from(size),
        });
        sim
    }

    #[test]
    fn techno_constructor_postmap_exact_and_fallback_place_one_constructed_identity() {
        let seed = 0xC701_1001;
        let rules = techno_constructor_start_rules();
        let terrain = techno_constructor_flat_start_terrain(10);
        let bounds = NativeStartBounds {
            min_rx: 1,
            min_ry: 1,
            width: 9,
            height: 9,
        };

        let mut exact = techno_constructor_start_sim(seed, 10);
        let mut exact_expected = SimRng::new(seed);
        let exact_word = (exact_expected.next_u32() & 0xFFFF) as u16;
        let exact_id = place_starting_object_near_base(
            &mut exact,
            "MTNK",
            "Americans",
            6,
            5,
            STARTING_MCV_FACING,
            1,
            bounds,
            &rules,
            &BTreeMap::new(),
            &terrain,
            None,
        )
        .expect("exact PostMap placement");
        let exact_entity = exact.substrate.entities.get(exact_id).unwrap();
        assert_eq!(exact_entity.techno_ctor_random_word, exact_word);
        assert_eq!((exact_entity.position.rx, exact_entity.position.ry), (6, 5));
        assert_eq!(
            exact.scenario_rng.logical_state(),
            exact_expected.logical_state()
        );

        let mut fallback = techno_constructor_start_sim(seed, 10);
        let mut fallback_expected = SimRng::new(seed);
        let blocker_word = (fallback_expected.next_u32() & 0xFFFF) as u16;
        let blocker = fallback
            .spawn_object("MTNK", "Americans", 6, 5, 0, &rules, &BTreeMap::new())
            .unwrap();
        assert_eq!(
            fallback
                .substrate
                .entities
                .get(blocker)
                .unwrap()
                .techno_ctor_random_word,
            blocker_word
        );
        let fallback_word = (fallback_expected.next_u32() & 0xFFFF) as u16;
        let _start_direction = fallback_expected.next_range_u32_inclusive(0, 7);
        let fallback_id = place_starting_object_near_base(
            &mut fallback,
            "MTNK",
            "Americans",
            6,
            5,
            STARTING_MCV_FACING,
            1,
            bounds,
            &rules,
            &BTreeMap::new(),
            &terrain,
            None,
        )
        .expect("fallback PostMap placement");
        let fallback_entity = fallback.substrate.entities.get(fallback_id).unwrap();
        assert_eq!(fallback_entity.techno_ctor_random_word, fallback_word);
        assert_ne!(
            (fallback_entity.position.rx, fallback_entity.position.ry),
            (6, 5)
        );
        assert_eq!(
            fallback.scenario_rng.logical_state(),
            fallback_expected.logical_state()
        );
    }

    #[test]
    fn techno_constructor_postmap_total_failure_deletes_object_but_keeps_draw_first() {
        let seed = 0xC701_1002;
        let rules = techno_constructor_start_rules();
        let terrain = ResolvedTerrainGrid::from_cells(10, 10, Vec::new());
        let bounds = NativeStartBounds {
            min_rx: 1,
            min_ry: 1,
            width: 9,
            height: 9,
        };
        let mut sim = techno_constructor_start_sim(seed, 10);
        let mut expected = SimRng::new(seed);
        let _constructor_word = expected.next_u32();
        for _radius in 1..=STARTING_MCV_FALLBACK_MAX_RADIUS {
            let _ = expected.next_range_u32_inclusive(0, 7);
            for jitter_pass in 0..2 {
                for _offset in 0..8 {
                    if jitter_pass != 0 {
                        let _ = expected.next_range_u32_inclusive(0, 1);
                        let _ = expected.next_range_u32_inclusive(0, 99);
                        let _ = expected.next_range_u32_inclusive(0, 1);
                        let _ = expected.next_range_u32_inclusive(0, 99);
                    }
                }
            }
        }

        assert!(
            place_starting_object_near_base(
                &mut sim,
                "MTNK",
                "Americans",
                5,
                5,
                STARTING_MCV_FACING,
                1,
                bounds,
                &rules,
                &BTreeMap::new(),
                &terrain,
                None,
            )
            .is_none()
        );
        assert!(sim.substrate.entities.is_empty());
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    }

    #[test]
    fn techno_constructor_starting_unit_uses_raw_unit_admission_before_fallback() {
        let seed = 0xC701_1004;
        let rules = techno_constructor_start_rules();
        let terrain = techno_constructor_flat_start_terrain(10);
        let bounds = NativeStartBounds {
            min_rx: 1,
            min_ry: 1,
            width: 9,
            height: 9,
        };
        let base = (6, 5);
        let mut sim = techno_constructor_start_sim(seed, 10);
        sim.install_resolved_terrain_for_new_map(terrain.clone());
        sim.substrate.raw_cell_occupation.mark_ground(
            base.0,
            base.1,
            crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        );

        let mut expected = SimRng::new(seed);
        let expected_word = (expected.next_u32() & 0xFFFF) as u16;
        let _fallback_start_direction = expected.next_range_u32_inclusive(0, 7);
        let stable_id = place_starting_object_near_base(
            &mut sim,
            "MTNK",
            "Americans",
            base.0,
            base.1,
            STARTING_MCV_FACING,
            1,
            bounds,
            &rules,
            &BTreeMap::new(),
            &terrain,
            None,
        )
        .expect("raw-bit rejection must retry the same constructed Unit");

        let placed = sim.substrate.entities.get(stable_id).unwrap();
        assert_eq!(placed.techno_ctor_random_word, expected_word);
        assert_ne!((placed.position.rx, placed.position.ry), base);
        assert_eq!(stable_id, 1, "fallback must retain the first identity");
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    }

    #[test]
    fn techno_constructor_extra_unit_selection_precedes_constructor_word() {
        let seed = 0xC701_1003;
        let rules = techno_constructor_start_rules();
        let terrain = techno_constructor_flat_start_terrain(10);
        let bounds = NativeStartBounds {
            min_rx: 1,
            min_ry: 1,
            width: 9,
            height: 9,
        };
        let mut sim = techno_constructor_start_sim(seed, 10);
        sim.session.game_options.tech_level = 10;
        let owner = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, None, true, 0, 10);
        house.base_center = Some((6, 5));
        sim.houses.insert(owner, house);
        let slots = [NormalizedSkirmishSlot {
            owner_name: "Americans".to_string(),
            country: LaunchCountry::America,
            color_index: 0,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
            is_human: true,
            difficulty: HouseDifficulty::Normal,
        }];
        let mut expected = SimRng::new(seed);
        let blocker_word = (expected.next_u32() & 0xFFFF) as u16;
        let blocker = sim
            .spawn_object("MTNK", "Americans", 6, 5, 0, &rules, &BTreeMap::new())
            .expect("starting-cell blocker");
        assert_eq!(
            sim.substrate
                .entities
                .get(blocker)
                .unwrap()
                .techno_ctor_random_word,
            blocker_word
        );
        let candidate_index = expected.next_range_u32_inclusive(0, 1) as usize;
        let expected_type = ["MTNK", "HTNK"][candidate_index];
        let expected_word = (expected.next_u32() & 0xFFFF) as u16;
        let _fallback_start_direction = expected.next_range_u32_inclusive(0, 7);

        assert_eq!(
            seed_starting_extra_units(
                &mut sim,
                &slots,
                &rules,
                &BTreeMap::new(),
                &terrain,
                bounds,
                1,
                false,
            ),
            1
        );
        let entity = sim.substrate.entities.get(2).unwrap();
        assert_eq!(sim.interner.resolve(entity.type_ref), expected_type);
        assert_eq!(entity.techno_ctor_random_word, expected_word);
        assert_ne!((entity.position.rx, entity.position.ry), (6, 5));
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    }

    #[test]
    fn nearoref_native_start_bounds_use_inclusive_full_cell_array_endpoints() {
        let mut sim = Simulation::new();
        sim.session.map_width = 138;
        sim.session.map_height = 138;
        sim.session.local_left = 2;
        sim.session.local_top = 4;
        sim.session.local_width = 76;
        sim.session.local_height = 48;
        let terrain = ResolvedTerrainGrid::from_cells(138, 138, Vec::new());

        let bounds = NativeStartBounds::from_session(&sim, &terrain);

        assert_eq!(
            (
                bounds.min_rx,
                bounds.min_ry,
                bounds.width,
                bounds.height,
                bounds.max_rx(),
                bounds.max_ry(),
            ),
            (1, 1, 137, 137, 137, 137)
        );
        assert_eq!(bounds.clamp(0, 0), (1, 1));
        assert_eq!(bounds.clamp(1, 1), (1, 1));
        assert_eq!(bounds.clamp(100, 75), (100, 75));
        assert_eq!(bounds.clamp(137, 137), (137, 137));
        assert_eq!(bounds.clamp(138, 138), (137, 137));
        assert_eq!(bounds.clamp(i32::MIN, i32::MAX), (1, 137));
    }

    #[test]
    fn untouched_bootstrap_cursors_match_fresh_descriptor_construction() {
        let seed = 0x51C0_1001;
        let expected = Simulation::from_descriptor(&descriptor(seed)).rng_state();
        let actual = ScenarioBootstrapRng::new(seed)
            .into_simulation(&descriptor(seed))
            .rng_state();

        assert_eq!(actual, expected);
    }

    #[test]
    fn main_variant_draws_do_not_advance_scenario_cursor() {
        let seed = 0x51C0_1002;
        let mut sim = ScenarioBootstrapRng::new(seed).into_simulation(&descriptor(seed));
        {
            let (scenario, mut main) = sim.terrain_load_draws();
            let _ = main.next_u32();
            drop(scenario);
        }
        let actual = sim.rng_state();
        let mut expected_main = SimRng::new(u64::from(seed));
        let _ = expected_main.next_u32();

        assert_eq!(
            actual.scenario,
            SimRng::new(u64::from(seed)).logical_state()
        );
        assert_eq!(actual.main, expected_main.logical_state());
    }

    #[test]
    fn terrain_scenario_draws_do_not_advance_main() {
        let seed = 0x51C0_1003;
        let mut sim = ScenarioBootstrapRng::new(seed).into_simulation(&descriptor(seed));
        {
            let (mut scenario, main) = sim.terrain_load_draws();
            let _ = scenario.next_range_u32_inclusive(5, 17);
            drop(main);
        }
        let actual = sim.rng_state();
        let mut expected_scenario = SimRng::new(u64::from(seed));
        let _ = expected_scenario.next_range_u32_inclusive(5, 17);

        assert_eq!(actual.scenario, expected_scenario.logical_state());
        assert_eq!(actual.main, SimRng::new(u64::from(seed)).logical_state());
    }

    #[test]
    fn generated_mapgen_continuation_replaces_only_mapgen_cursor() {
        let match_seed = 0x51C0_1005;
        let map_seed: u16 = 0xBEEF;
        let mut expected_mapgen = SimRng::new(u64::from(map_seed));
        for _ in 0..353 {
            let _ = expected_mapgen.next_u32();
        }
        let generated = expected_mapgen.logical_state();

        let mut owner = ScenarioBootstrapRng::new(match_seed);
        owner.install_generated_mapgen_continuation(MapGenRngContinuation::from_native_parts(
            generated.words,
            usize::try_from(generated.index_a).expect("test MapGen cursor A is non-negative"),
            usize::try_from(generated.index_b).expect("test MapGen cursor B is non-negative"),
        ));
        let actual = owner.into_simulation(&descriptor(match_seed)).rng_state();

        assert_eq!(
            actual.scenario,
            SimRng::new(u64::from(match_seed)).logical_state()
        );
        assert_eq!(
            actual.main,
            SimRng::new(u64::from(match_seed)).logical_state()
        );
        assert_eq!(actual.mapgen, expected_mapgen.logical_state());
    }

    #[test]
    fn scenario_prefix_runs_two_identical_house_passes_before_fill() {
        use crate::skirmish_launch::{
            AiDifficulty, PreFillAiHouseSlot, PreFillHouseRoster, PreFillHumanHouse, SkirmishAiSlot,
        };

        let seed = (0..100_000u32)
            .find(|seed| {
                let mut rng = SimRng::new(u64::from(*seed));
                let before = rng.logical_state().index_a;
                let _ = rng.next_range_u32_inclusive(
                    HOUSE_CONSTRUCTOR_TIMER_MIN,
                    HOUSE_CONSTRUCTOR_TIMER_MAX,
                );
                (rng.logical_state().index_a - before).rem_euclid(250) > 1
            })
            .expect("the native ranged domain has a rejection seed");
        let roster = PreFillHouseRoster::new(
            vec![
                PreFillHumanHouse {
                    priority: 4,
                    source_order: 1,
                    observer: false,
                },
                PreFillHumanHouse {
                    priority: -7,
                    source_order: 0,
                    observer: true,
                },
            ],
            vec![
                PreFillAiHouseSlot {
                    slot_index: 3,
                    valid: true,
                },
                PreFillAiHouseSlot {
                    slot_index: 1,
                    valid: false,
                },
                PreFillAiHouseSlot {
                    slot_index: 0,
                    valid: true,
                },
            ],
        );
        assert_eq!(roster.human_nodes()[0].priority, -7);
        assert_eq!(
            roster
                .ai_slots()
                .iter()
                .map(|slot| slot.slot_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 3]
        );
        assert_eq!(roster.required_start_count(), 3);
        assert_eq!(roster.created_house_count(), 6);

        let mut session = one_player_battle_launch("prefix-roster.mmx")
            .session()
            .clone();
        session.local.start_position = LaunchStartPosition::Position(0);
        session.opponents = (0..2)
            .map(|index| SkirmishAiSlot {
                country: LaunchCountry::Russia,
                country_random: false,
                color_index: index + 1,
                color_random: false,
                start_position: LaunchStartPosition::Position(index + 1),
                team: LaunchTeam::None,
                difficulty: AiDifficulty::Easy,
            })
            .collect();
        session.pre_fill_house_roster = roster;
        let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let starts = [
            Waypoint {
                index: 0,
                rx: 30,
                ry: 30,
            },
            Waypoint {
                index: 1,
                rx: 40,
                ry: 40,
            },
            Waypoint {
                index: 2,
                rx: 50,
                ry: 50,
            },
        ];
        let map = prefix_map_with_starts(&starts);
        let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
            .unwrap();

        let native_plan =
            prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
                .unwrap();
        let mut native_rules =
            crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
                IniFile::from_bytes(b"").unwrap(),
                None,
                IniFile::from_bytes(b"").unwrap(),
            )
            .unwrap();
        let native_load = native_rules
            .load_noncampaign_scenario(None, &map.ini)
            .unwrap();
        let (_, _, _, receipt) = native_load.into_parts();
        let early_count = receipt.pre_reset().event_count() as u32;
        let first_super_count = receipt.pre_reset().allocated_super_weapon_type_count() as u32;
        let rebuilt_count = receipt.post_reset().event_count() as u32;
        let final_super_count = receipt.post_reset().allocated_super_weapon_type_count() as u32;
        let bound_native = native_plan.bind_native_rules_receipt(receipt);
        let native_checkpoints = bound_native.native_ids.checkpoints();
        let resize_count = map
            .header
            .height
            .wrapping_mul(map.header.width.wrapping_mul(2).wrapping_sub(1))
            .wrapping_add(1);
        assert_eq!(
            native_checkpoints.after_early_types,
            1_000_000 + early_count
        );
        assert_eq!(
            native_checkpoints.after_first_house_generation,
            native_checkpoints
                .after_early_types
                .wrapping_add(6 * (1 + first_super_count)),
            "observer, both valid AI rows, ordinary human, Neutral, and Special all construct"
        );
        assert_eq!(
            native_checkpoints.after_first_resize,
            native_checkpoints
                .after_first_house_generation
                .wrapping_add(resize_count),
            "the first native Cell generation uses [Map] Size and includes the dummy"
        );
        assert_eq!(
            native_checkpoints.after_rebuilt_types,
            native_checkpoints
                .after_first_resize
                .wrapping_add(rebuilt_count)
        );
        assert_eq!(
            native_checkpoints.after_final_house_generation,
            native_checkpoints
                .after_rebuilt_types
                .wrapping_add(6 * (1 + final_super_count))
        );
        assert_eq!(
            native_checkpoints.after_final_resize,
            native_checkpoints
                .after_final_house_generation
                .wrapping_add(resize_count),
            "the same live Cell identities reconstruct in the second Resize generation"
        );

        let mut reference = SimRng::new(u64::from(seed));
        let first_timers: Vec<_> = (0..6)
            .map(|_| {
                reference.next_range_u32_inclusive(
                    HOUSE_CONSTRUCTOR_TIMER_MIN,
                    HOUSE_CONSTRUCTOR_TIMER_MAX,
                )
            })
            .collect();
        let after_first = reference.logical_state();
        let second_timers: Vec<_> = (0..6)
            .map(|_| {
                reference.next_range_u32_inclusive(
                    HOUSE_CONSTRUCTOR_TIMER_MIN,
                    HOUSE_CONSTRUCTOR_TIMER_MAX,
                )
            })
            .collect();
        let checkpoints = plan.rng_checkpoints();
        assert_eq!(plan.first_house_timers(), first_timers);
        assert_eq!(plan.second_house_timers(), second_timers);
        assert_eq!(checkpoints.after_first_house_pass, after_first);
        assert_eq!(checkpoints.after_first_gather, after_first);
        assert_eq!(
            checkpoints.after_second_gather_and_chooser, after_first,
            "complete explicit starts make both callbacks draw-free"
        );
        assert_eq!(
            checkpoints.after_zero_draw_reset,
            checkpoints.after_second_gather_and_chooser
        );
        assert_eq!(
            checkpoints.after_second_house_pass,
            reference.logical_state()
        );

        let mut owner = ScenarioBootstrapRng::new(seed);
        owner.install_pre_fill_scenario_prefix_plan(plan).unwrap();
        let sim = owner.into_simulation(&descriptor(seed));
        assert!(
            sim.houses.is_empty(),
            "prefix emulation retains no disposable first-pass House state"
        );
    }

    #[test]
    fn scenario_prefix_sparse_waypoints_preserve_only_entries_below_target() {
        let map = prefix_map_with_starts(&[]);
        let starts = HashMap::from([
            (
                0,
                Waypoint {
                    index: 0,
                    rx: 30,
                    ry: 30,
                },
            ),
            (
                2,
                Waypoint {
                    index: 2,
                    rx: 50,
                    ry: 50,
                },
            ),
        ]);
        let mut target_two_rng = SimRng::new(0x5200);
        let target_two =
            native_gather_pre_fill_start_positions(&starts, 2, &map.header, &mut target_two_rng)
                .unwrap();
        assert_eq!(target_two.len(), 2);
        assert_eq!((target_two[0].rx, target_two[0].ry), (30, 30));
        assert!(
            !target_two
                .iter()
                .any(|start| (start.rx, start.ry) == (50, 50))
        );

        let mut target_three_rng = SimRng::new(0x5200);
        let target_three =
            native_gather_pre_fill_start_positions(&starts, 3, &map.header, &mut target_three_rng)
                .unwrap();
        assert_eq!(target_three.len(), 3);
        assert_eq!((target_three[0].rx, target_three[0].ry), (30, 30));
        assert_eq!((target_three[1].rx, target_three[1].ry), (50, 50));
    }

    #[test]
    fn scenario_prefix_all_stock_mode_families_gather_twice() {
        let map = prefix_map_with_starts(&[]);
        let modes = crate::skirmish_modes::stock_skirmish_modes();
        assert_eq!(modes.len(), 9);
        for mode in modes {
            let mut session = one_player_battle_launch("stock-mode.mmx").session().clone();
            session.mode = crate::skirmish_launch::SkirmishLaunchMode::from_game_mode(&mode);
            let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
            let expected_family = if mode.id == 3 {
                StockOfflineStartCallbackFamily::Cooperative
            } else {
                StockOfflineStartCallbackFamily::Battle
            };
            assert_eq!(
                stock_offline_start_callback_family(launch.session()).unwrap(),
                expected_family
            );
            let plan = prepare_stock_offline_scenario_prefix_plan(
                &launch,
                &map,
                &map.waypoints,
                0x5300 + mode.id as u32,
            )
            .unwrap_or_else(|err| panic!("stock mode {} prefix failed: {err}", mode.id));
            assert_eq!(plan.first_gathered_starts().len(), 1);
            assert_eq!(plan.final_gathered_starts().len(), 1);
            assert_ne!(
                plan.rng_checkpoints().after_first_gather,
                plan.rng_checkpoints().after_second_gather_and_chooser,
                "stock mode {} must execute its second deficient Gather",
                mode.id
            );
        }
    }

    #[test]
    fn scenario_prefix_rejects_mutated_stock_mode_metadata() {
        let session = one_player_battle_launch("mutated-mode.mmx")
            .session()
            .clone();
        let mutations: [fn(&mut crate::skirmish_launch::SkirmishLaunchMode); 7] = [
            |mode| mode.ui_name_key = "GUI:Cooperative".to_string(),
            |mode| mode.tooltip_key = "STT:ModeCooperative".to_string(),
            |mode| mode.override_file = "MPCoopMD.ini".to_string(),
            |mode| mode.map_filter = "cooperative".to_string(),
            |mode| mode.random_maps_allowed = false,
            |mode| mode.allies_allowed = false,
            |mode| mode.must_ally = true,
        ];
        let mut cases = Vec::new();
        for mutate in mutations {
            let mut mutated = session.clone();
            mutate(&mut mutated.mode);
            cases.push(mutated);
        }
        let mut unknown_id = session;
        unknown_id.mode.id = 10;
        cases.push(unknown_id);

        for mutated in cases {
            let id = mutated.mode.id;
            assert_eq!(
                stock_offline_start_callback_family(&mutated).unwrap_err(),
                PreFillScenarioPrefixPlanError::UnsupportedStockMode { id }
            );
        }
    }

    #[test]
    fn scenario_prefix_rejects_invalid_map_cell_extents() {
        let launch = one_player_battle_launch("invalid-size.mmx");
        for (width, height) in [(0, 64), (64, 0), (400, 200), (u32::MAX, 1)] {
            let mut map = one_start_prefix_map(Waypoint {
                index: 0,
                rx: 30,
                ry: 30,
            });
            map.header.width = width;
            map.header.height = height;
            assert_eq!(
                prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, 0x5320,)
                    .unwrap_err(),
                PreFillScenarioPrefixPlanError::InvalidMapCellExtent,
                "Size={width},{height}"
            );
        }

        let mut boundary = one_start_prefix_map(Waypoint {
            index: 0,
            rx: 30,
            ry: 30,
        });
        boundary.header.width = 256;
        boundary.header.height = 256;
        prepare_stock_offline_scenario_prefix_plan(&launch, &boundary, &boundary.waypoints, 0x5320)
            .expect("Size sum 512 is the inclusive native cell-array boundary");
    }

    #[test]
    fn scenario_prefix_ignores_later_map_payload() {
        use crate::map::entities::{EntityCategory, MapEntity};
        use crate::map::map_file::MapCell;
        use crate::map::overlay::{OverlayEntry, TerrainObject};

        let start = Waypoint {
            index: 0,
            rx: 35,
            ry: 35,
        };
        let map_a = one_start_prefix_map(start);
        let mut map_b = one_start_prefix_map(start);
        map_b.header.fill = "Water".to_string();
        map_b.header.level = 9;
        map_b.cells.push(MapCell {
            rx: 36,
            ry: 36,
            tile_index: 777,
            sub_tile: 3,
            z: 9,
        });
        map_b.overlays.push(OverlayEntry {
            rx: 36,
            ry: 36,
            overlay_id: 24,
            frame: 7,
        });
        map_b.terrain_objects.push(TerrainObject {
            rx: 37,
            ry: 37,
            name: "TREE01".to_string(),
        });
        map_b.entities.push(MapEntity {
            owner: "Neutral".to_string(),
            type_id: "CAOILD".to_string(),
            health: 256,
            cell_x: 38,
            cell_y: 38,
            facing: 0,
            category: EntityCategory::Structure,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        });
        let launch = one_player_battle_launch("payload.mmx");
        let plan_a =
            prepare_stock_offline_scenario_prefix_plan(&launch, &map_a, &map_a.waypoints, 0x5400)
                .unwrap();
        let plan_b =
            prepare_stock_offline_scenario_prefix_plan(&launch, &map_b, &map_b.waypoints, 0x5400)
                .unwrap();
        assert_eq!(
            plan_a.first_gathered_starts(),
            plan_b.first_gathered_starts()
        );
        assert_eq!(
            plan_a.final_gathered_starts(),
            plan_b.final_gathered_starts()
        );
        assert_eq!(plan_a.assignment(), plan_b.assignment());
        assert_eq!(plan_a.scenario_rng_after, plan_b.scenario_rng_after);
    }

    #[test]
    fn scenario_prefix_projection_is_draw_free() {
        let seed = 0x5500;
        let start = Waypoint {
            index: 0,
            rx: 40,
            ry: 40,
        };
        let map = one_start_prefix_map(start);
        let mut session = one_player_battle_launch("projection.mmx").session().clone();
        session.local.start_position = LaunchStartPosition::Position(0);
        let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
            .unwrap();
        let mut owner = ScenarioBootstrapRng::new(seed);
        let projection = owner.install_pre_fill_scenario_prefix_plan(plan).unwrap();
        let mut sim = owner.into_simulation(&descriptor(seed));
        let rules = techno_constructor_start_rules();
        initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
        assert_eq!(sim.session.current_house, Some(sim.session.house_order[0]));
        let before = sim.scenario_rng.logical_state();
        let slots = normalized_launch_slots(launch.session());
        project_pre_fill_start_assignment(&mut sim, &slots, projection.assignment());
        assert_eq!(sim.scenario_rng.logical_state(), before);
        assert_eq!(
            sim.houses
                .get(&sim.session.house_order[0])
                .and_then(|house| house.base_center),
            Some((40, 40))
        );
    }

    #[test]
    fn scenario_prefix_cursor_reaches_base_plan_consumer() {
        use crate::sim::base_plan::BasePlanState;
        use crate::sim::base_plan_generation::recalc_base_plan;

        let seed = 0x5600;
        let start = Waypoint {
            index: 0,
            rx: 40,
            ry: 40,
        };
        let map = one_start_prefix_map(start);
        let mut session = one_player_battle_launch("base-plan.mmx").session().clone();
        session.local.start_position = LaunchStartPosition::Position(0);
        let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
            .unwrap();
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\n\
             HarvesterUnit=HARV\n\
             AIExtraRefineries=1,1,1\n\
             AISlaveMinerNumber=1,1,1\n\
             AlliedBaseDefenseCounts=1,1,1\n\
             SovietBaseDefenseCounts=0,0,0\n\
             ThirdBaseDefenseCounts=0,0,0\n\
             [AI]\n\
             BuildConst=CON\nBuildPower=POW\nBuildRefinery=REF\n\
             BuildBarracks=BAR\nBuildWeapons=WEAP\nBuildRadar=RAD\nBuildTech=TECH\n\
             [Countries]\n0=Americans\n[Sides]\nAllied=Americans\n\
             [Americans]\nSide=Allied\n\
             [VehicleTypes]\n0=HARV\n[HARV]\nOwner=Americans\nStrength=1\n\
             [BuildingTypes]\n0=CON\n1=POW\n2=REF\n3=BAR\n4=WEAP\n5=RAD\n6=TECH\n\
             [CON]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [POW]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [REF]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [BAR]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [WEAP]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [RAD]\nOwner=Americans\nAIBuildThis=no\nFoundation=1x1\n\
             [TECH]\nOwner=Americans\nAIBuildThis=no\nFoundation=1x1\n",
        ))
        .unwrap();

        let mut owner = ScenarioBootstrapRng::new(seed);
        owner.install_pre_fill_scenario_prefix_plan(plan).unwrap();
        let mut sim = owner.into_simulation(&descriptor(seed));
        let mut expected_rng = SimRng::new(u64::from(seed));
        for _ in 0..6 {
            let _ = expected_rng
                .next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX);
        }
        let mut actual_plan = BasePlanState::default();
        let mut expected_plan = BasePlanState::default();
        recalc_base_plan(
            &mut actual_plan,
            &rules,
            "Americans",
            0,
            HouseDifficulty::Normal,
            10,
            true,
            &mut sim.scenario_rng,
        );
        recalc_base_plan(
            &mut expected_plan,
            &rules,
            "Americans",
            0,
            HouseDifficulty::Normal,
            10,
            true,
            &mut expected_rng,
        );
        assert_eq!(actual_plan.nodes, expected_plan.nodes);
        assert_eq!(actual_plan.percent_built, expected_plan.percent_built);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
    }

    #[test]
    fn scenario_prefix_transfers_once_before_terrain() {
        let seed = 0x51C0_1004;
        let before = SimRng::new(u64::from(seed));
        let start = Waypoint {
            index: 0,
            rx: 40,
            ry: 40,
        };
        let map = one_start_prefix_map(start);
        let launch = one_player_battle_launch("mp01t4.map");
        let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
            .expect("stock Battle prefix");
        let after = plan.scenario_rng_after_cursor.clone();
        let mut owner = ScenarioBootstrapRng::new(seed);

        owner
            .install_pre_fill_scenario_prefix_plan(plan)
            .expect("fresh bootstrap owner accepts the plan prefix");
        let actual = owner.into_simulation(&descriptor(seed)).rng_state();

        assert_eq!(actual.scenario, after.logical_state());
        assert_eq!(actual.main, before.logical_state());
    }

    #[test]
    fn scenario_prefix_rejects_a_tampered_prestate_before_transfer() {
        let seed = 0x51C0_1006;
        let start = Waypoint {
            index: 0,
            rx: 40,
            ry: 40,
        };
        let map = one_start_prefix_map(start);
        let launch = one_player_battle_launch("mp01t4.map");
        let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
            .expect("stock Battle prefix");
        let mut owner = ScenarioBootstrapRng::new(seed ^ 1);

        assert!(matches!(
            owner.install_pre_fill_scenario_prefix_plan(plan),
            Err(PreFillScenarioPrefixPlanError::ScenarioRngPrestateMismatch { .. })
        ));
    }

    #[test]
    fn staged_simulation_owns_fill_and_generated_replay_without_reconstruction() {
        use crate::map::construction_trace::{RmgConstructionPhase, RmgConstructionTrace};

        let seed = 0x51C0_1005;
        let mut reference = SimRng::new(u64::from(seed));
        let owner = ScenarioBootstrapRng::new(seed);
        let mut sim = owner.into_simulation(&descriptor(seed));
        sim.session.binary_frame = 77;
        let retained_handle = sim.allocate_stable_id();

        {
            let (mut scenario_fill, mut variant_main) = sim.terrain_load_draws();
            assert_eq!(
                scenario_fill.next_range_u32_inclusive(5, 17),
                reference.next_range_u32_inclusive(5, 17),
            );
            let _ = variant_main.next_u32();
        }

        let mut trace = RmgConstructionTrace::default();
        trace.push_emitted(
            RmgConstructionPhase::BridgeRepairHut,
            "CABHUT".to_string(),
            0,
            (10, 11),
        );
        let expected_word = (reference.next_u32() & 0xFFFF) as u16;
        let bindings = sim
            .replay_staged_generated_construction_trace(&trace)
            .expect("staged owner replays the accepted constructor event");

        assert_eq!(
            bindings
                .entry(0)
                .expect("emitted constructor binding")
                .techno_ctor_random_word,
            expected_word,
        );
        assert_eq!(sim.rng_state().scenario, reference.logical_state());
        assert_eq!(sim.session.binary_frame, 77);
        assert_eq!(retained_handle, 1);
        assert_eq!(sim.allocate_stable_id(), 2);
    }

    #[test]
    fn gsi_04_12_prefix_fill_and_generated_trace_share_one_scenario_owner() {
        use crate::map::construction_trace::{RmgConstructionPhase, RmgConstructionTrace};

        let seed = 0x51C0_1006;
        let before = SimRng::new(u64::from(seed));
        let start = Waypoint {
            index: 0,
            rx: 40,
            ry: 40,
        };
        let map = one_start_prefix_map(start);
        let launch = one_player_battle_launch("RandMap.SED");
        let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
            .expect("generated stock prefix");
        let mut reference = plan.scenario_rng_after_cursor.clone();
        let mut owner = ScenarioBootstrapRng::new(seed);
        owner
            .install_pre_fill_scenario_prefix_plan(plan)
            .expect("fresh launch owner accepts the Full-Init prefix");
        // The match load stages the Simulation here, before Fill.
        let mut sim = owner.into_simulation(&descriptor(seed));
        {
            let (mut scenario_fill, main) = sim.terrain_load_draws();
            let actual = scenario_fill.next_range_u32_inclusive(5, 17);
            let expected = reference.next_range_u32_inclusive(5, 17);
            assert_eq!(actual, expected);
            drop(main);
        }

        let mut trace = RmgConstructionTrace::default();
        trace.push_emitted(
            RmgConstructionPhase::BridgeRepairHut,
            "CABHUT".to_string(),
            0,
            (10, 11),
        );
        trace.push_discarded(RmgConstructionPhase::NeutralTech, "CAOILD".to_string());
        trace.push_emitted(
            RmgConstructionPhase::NeutralTech,
            "CATHOSP".to_string(),
            2,
            (12, 13),
        );
        let first_word = (reference.next_u32() & 0xFFFF) as u16;
        let _discarded_word = reference.next_u32();
        let second_word = (reference.next_u32() & 0xFFFF) as u16;

        let bindings = sim
            .replay_staged_generated_construction_trace(&trace)
            .expect("ordered trace binds emitted constructors");
        assert_eq!(
            bindings
                .entry(0)
                .expect("CABHUT binding")
                .techno_ctor_random_word,
            first_word
        );
        assert_eq!(
            bindings
                .entry(2)
                .expect("neutral-tech binding")
                .techno_ctor_random_word,
            second_word
        );
        assert!(bindings.entry(1).is_none(), "discarded row binds no entity");

        let actual = sim.rng_state();
        assert_eq!(actual.scenario, reference.logical_state());
        assert_eq!(actual.main, before.logical_state());
    }

    #[test]
    fn gsi_04_12_generated_trace_continues_into_starting_techno_and_post_map_crate() {
        use crate::map::construction_trace::{RmgConstructionPhase, RmgConstructionTrace};
        use crate::sim::scenario_post_map::ScenarioPostMapInput;
        use crate::skirmish_launch::{
            SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLocalSlot,
        };

        const SIZE: u16 = 80;
        let seed = 0x51C0_100B;
        let ini = IniFile::from_str(
            "[General]\n\
             BaseUnit=MTNK\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=CABHUT\n\
             1=CATHOSP\n\
             [MTNK]\n\
             Strength=300\n\
             Speed=6\n\
             Cost=100\n\
             TechLevel=1\n\
             Owner=Americans\n\
             [CABHUT]\n\
             Strength=100\n\
             Foundation=1x1\n\
             Owner=Neutral\n\
             [CATHOSP]\n\
             Strength=100\n\
             Foundation=1x1\n\
             Owner=Neutral\n\
             [OverlayTypes]\n\
             0=WOOD\n\
             1=WATER\n\
             [WOOD]\n\
             Crate=yes\n\
             [WATER]\n\
             Crate=yes\n\
             [CrateRules]\n\
             CrateImg=WOOD\n\
             WoodCrateImg=WOOD\n\
             WaterCrateImg=WATER\n\
             CrateMinimum=1\n\
             CrateMaximum=1\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("single-owner launch rules");
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);
        let map = MapFile::from_bytes(
            b"[Map]\nTheater=TEMPERATE\nSize=0,0,40,40\nLocalSize=2,2,36,32\n\
              [Basic]\nName=GSI 04.12 owner fixture\n\
              [Structures]\n\
              0=Neutral,CABHUT,256,30,30,0,None\n\
              1=Neutral,CATHOSP,256,32,32,0,None\n\
              [IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n",
        )
        .expect("minimal generated-map-shaped input");
        let session = SkirmishLaunchSession {
            mode: SkirmishLaunchMode {
                id: 1,
                ui_name_key: "GUI:Battle".to_string(),
                tooltip_key: "STT:ModeBattle".to_string(),
                override_file: "MPBattleMD.ini".to_string(),
                map_filter: "standard".to_string(),
                random_maps_allowed: true,
                allies_allowed: true,
                must_ally: false,
            },
            selected_map_file: Some("RandMap.SED".to_string()),
            player_name: "Player".to_string(),
            local: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: false,
                color_index: 0,
                color_random: false,
                start_position: LaunchStartPosition::Position(1),
                team: LaunchTeam::None,
            },
            opponents: Vec::new(),
            pre_fill_house_roster:
                crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(0),
            options: SkirmishLaunchOptions {
                unit_count: 0,
                bases: true,
                crates: true,
                ..SkirmishLaunchOptions::default()
            },
        };
        let launch = MatchLaunchDescriptor::from_resolved(session)
            .expect("fixture contains no unresolved shell choices");

        let start = Waypoint {
            index: 0,
            rx: 40,
            ry: 40,
        };
        let before = SimRng::new(u64::from(seed));
        let staged_starts = HashMap::from([(0, start)]);
        let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &staged_starts, seed)
            .expect("accepted generated prefix");
        let mut reference = plan.scenario_rng_after_cursor.clone();
        let mut owner = ScenarioBootstrapRng::new(seed);
        let projection = owner
            .install_pre_fill_scenario_prefix_plan(plan)
            .expect("one authoritative Full-Init prefix");
        // The match load stages the Simulation here, before Fill.
        let mut scenario = descriptor(seed);
        scenario.map_name = "RandMap.SED".to_string();
        scenario.theater = "TEMPERATE".to_string();
        scenario.game_mode_nonzero = true;
        scenario.map_width = SIZE;
        scenario.map_height = SIZE;
        scenario.local_left = 2;
        scenario.local_top = 2;
        scenario.local_width = 36;
        scenario.local_height = 32;
        scenario.mp_start_waypoints.insert(0, (start.rx, start.ry));
        let mut sim = owner.into_simulation(&scenario);
        {
            let (mut scenario_fill, main) = sim.terrain_load_draws();
            let actual = scenario_fill.next_range_u32_inclusive(5, 17);
            let expected = reference.next_range_u32_inclusive(5, 17);
            assert_eq!(actual, expected, "terrain Fill continues the prefix");
            drop(main);
        }

        let mut trace = RmgConstructionTrace::default();
        trace.push_emitted(
            RmgConstructionPhase::BridgeRepairHut,
            "CABHUT".to_string(),
            0,
            (30, 30),
        );
        trace.push_discarded(RmgConstructionPhase::NeutralTech, "CAOILD".to_string());
        trace.push_emitted(
            RmgConstructionPhase::NeutralTech,
            "CATHOSP".to_string(),
            1,
            (32, 32),
        );
        let first_trace_word = (reference.next_u32() & 0xFFFF) as u16;
        let _discarded_trace_word = reference.next_u32();
        let second_trace_word = (reference.next_u32() & 0xFFFF) as u16;
        let bindings = sim
            .replay_staged_generated_construction_trace(&trace)
            .expect("ordered trace binds emitted constructors");
        assert_eq!(
            bindings.entry(0).unwrap().techno_ctor_random_word,
            first_trace_word
        );
        assert_eq!(
            bindings.entry(1).unwrap().techno_ctor_random_word,
            second_trace_word
        );
        assert!(bindings.entry(2).is_none());
        assert_eq!(
            sim.rng_state().scenario,
            reference.logical_state(),
            "the independently walked intermediate cursor must match after replay"
        );

        let terrain = techno_constructor_flat_start_terrain(SIZE);
        sim.install_resolved_terrain_for_new_map(terrain.clone());
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(SIZE, SIZE));
        sim.install_playfield_from_map_header(&map.header);
        let house_roster = HouseRoster::default();
        initialize_skirmish_launch_houses(&mut sim, &house_roster, &rules, &launch);
        let before_projection = sim.scenario_rng.logical_state();
        assert_eq!(
            sim.spawn_generated_from_map_with_resolved(
                &map.entities,
                &rules,
                &BTreeMap::new(),
                Some(&terrain),
                &bindings,
            )
            .expect("generated projection validates the replay bindings"),
            2
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            before_projection,
            "generated projection must install preconsumed words without drawing"
        );
        let installed_words = [("CABHUT", first_trace_word), ("CATHOSP", second_trace_word)];
        for (type_id, expected_word) in installed_words {
            let entity = sim
                .entities()
                .values()
                .find(|entity| sim.interner.resolve(entity.type_ref) == type_id)
                .unwrap_or_else(|| panic!("generated {type_id} reached Simulation"));
            assert_eq!(entity.techno_ctor_random_word, expected_word);
        }

        let starting_techno_word = (reference.next_u32() & 0xFFFF) as u16;
        let _starting_force_tail = reference.next_range_u32_inclusive(0, 0xFFFF);
        let launch_result = apply_pre_fill_scenario_prefix_launch_session_with_overlay_registry(
            &mut sim,
            &map,
            &house_roster,
            &rules,
            &BTreeMap::new(),
            &terrain,
            &launch,
            &overlays,
            &projection,
        );
        assert_eq!(launch_result.spawned_mcvs, 1);
        let starting_techno = sim
            .entities()
            .values()
            .find(|entity| sim.interner.resolve(entity.type_ref) == "MTNK")
            .expect("the production launch constructs its starting MCV");
        assert_eq!(
            starting_techno.techno_ctor_random_word, starting_techno_word,
            "the first post-trace Techno constructor continues the same cursor"
        );

        let expected_crate_cell = (
            reference.next_range_u32_inclusive(1, u32::from(SIZE - 1)) as u16,
            reference.next_range_u32_inclusive(1, u32::from(SIZE - 1)) as u16,
        );
        assert!(
            crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (
                    i32::from(expected_crate_cell.0),
                    i32::from(expected_crate_cell.1),
                ),
                sim.playfield_bounds,
                Some(&terrain),
            ),
            "controlled first crate draw must need no FNPC displacement"
        );
        let _crate_timer = reference.next_range_u32_inclusive(0, 0x7FFF_FFFE);
        // The queues are built where the load builds them, before the post-map
        // tail, which leaves them alone.
        let overlay_grid_for_queues = sim.overlay_grid.clone();
        let _ = crate::sim::runtime::initialize_native_tiberium_queues(
            &mut sim,
            &map.basic,
            &map.special_flags,
            &rules,
            &overlays,
            overlay_grid_for_queues.as_ref(),
            (SIZE, SIZE),
        );
        let output = sim.finalize_scenario_post_map(ScenarioPostMapInput {
            map_width: SIZE,
            map_height: SIZE,
            basic: &map.basic,
            special_flags: &map.special_flags,
            normal_lighting: crate::map::lighting::parse_lighting_profiles(&map.ini).normal,
            rules: &rules,
            overlay_registry: &overlays,
            house_roster: &house_roster,
            skirmish_session: Some(&launch),
        });
        assert_eq!(
            output.crates,
            Some(crate::sim::crates::CratePlacement {
                requested: 1,
                accepted: 1,
                visible: 1,
            })
        );
        let wood = overlays.id_for_name("WOOD").expect("WOOD crate overlay");
        let crate_cells = sim
            .overlay_grid
            .as_ref()
            .expect("post-map overlay authority")
            .iter_occupied()
            .filter_map(|(rx, ry, cell)| (cell.overlay_id == Some(wood)).then_some((rx, ry)))
            .collect::<Vec<_>>();
        assert_eq!(crate_cells, vec![expected_crate_cell]);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            reference.logical_state(),
            "prefix, Fill, trace, starting Techno, and crate draws must form one stream"
        );
        assert_eq!(sim.main_rng.logical_state(), before.logical_state());
    }

    #[test]
    fn generated_trace_rejects_bad_ordinal_before_spending_scenario() {
        let seed = 0x51C0_1007;
        let mut sim = ScenarioBootstrapRng::new(seed).into_simulation(&descriptor(seed));
        let mut trace = crate::map::construction_trace::RmgConstructionTrace::default();
        trace.push_discarded(
            crate::map::construction_trace::RmgConstructionPhase::NeutralTech,
            "CAOILD".to_string(),
        );
        trace.events[0].ordinal = 4;

        assert!(matches!(
            sim.replay_staged_generated_construction_trace(&trace),
            Err(
                crate::sim::world::GeneratedTechnoInitError::TraceOrdinalMismatch {
                    expected: 0,
                    found: 4,
                }
            )
        ));
        assert_eq!(
            sim.rng_state().scenario,
            SimRng::new(u64::from(seed)).logical_state()
        );
    }
}
