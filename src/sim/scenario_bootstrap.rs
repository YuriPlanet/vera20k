//! Simulation-owned fresh Scenario bootstrap.
//!
//! Fresh families share one staged Simulation before terrain Fill. Active-stock
//! offline startup resolves its two House/start-callback passes; campaign startup
//! constructs and reads its authored House/Super array once. The same Scenario
//! RNG and native-ID owners then continue through terrain and map objects.
//! Opening forces, shroud, AI credits and diplomacy retain their shared owners.

#[cfg(test)]
#[path = "campaign_house_bootstrap_tests.rs"]
mod campaign_house_bootstrap_tests;

#[cfg(test)]
#[path = "scenario_house_name_retail_tests.rs"]
mod scenario_house_name_retail_tests;

#[cfg(test)]
#[path = "scenario_launch_identity_retail_tests.rs"]
mod scenario_launch_identity_retail_tests;

#[cfg(test)]
#[path = "scenario_fillsilos_retail_tests.rs"]
mod scenario_fillsilos_retail_tests;

#[cfg(test)]
#[path = "scenario_special_money_retail_tests.rs"]
mod scenario_special_money_retail_tests;

use std::collections::HashMap;

use crate::map::entities::EntityCategory;
use crate::map::houses::{HouseRoster, normalize_house_name};
use crate::map::map_file::{MapFile, MapHeader};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::waypoints::Waypoint;
use crate::rng_continuation::MapGenRngContinuation;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::find_nearby_cell::{
    NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell, map_owned_radius_cap,
};
use crate::sim::house_state::{HouseDifficulty, HouseState};
use crate::sim::houses::Houses;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::rng::{SimRng, SimRngLogicalState};
use crate::sim::scenario_session::ScenarioDescriptor;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::{PlacementEvidence, Simulation};
use crate::skirmish_launch::{
    LaunchCountry, LaunchStartPosition, LaunchTeam, PreFillHouseRoster, SkirmishLaunchSession,
};
use crate::util::direction::DIRECTION_DELTAS;
use crate::util::native_x87::{NativeF64Bits, X87Chop53, sqrt_approx_f32};

#[derive(Debug, Clone, Copy)]
pub struct NativeStartBounds {
    pub min_rx: u16,
    pub min_ry: u16,
    pub width: u16,
    pub height: u16,
}

impl NativeStartBounds {
    /// Construct the post-Resize cell-array rectangle directly from parsed
    /// `[Map] Size`, before Fill has produced a resolved terrain grid.
    pub fn from_map_header(header: &MapHeader) -> Option<Self> {
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
    pub fn from_session(sim: &Simulation, terrain: &ResolvedTerrainGrid) -> Self {
        // `session.map_width/height` hold the canonical cell-array extent
        // (~SizeW+SizeH), so the native rect side is that extent minus one.
        // Sessions without header data (headless fixtures) fall back to the
        // terrain grid extent — VERA-internal, same shape.
        let (extent_x, extent_y) = if sim.session.map_width() != 0 && sim.session.map_height() != 0
        {
            (sim.session.map_width(), sim.session.map_height())
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

    /// The inclusive test native walks make against `+0x124..+0x130`:
    /// `left <= x <= left + width`, and the same for y. Both CABHUT tails
    /// walk inside it (repair `0x005738B1`, death `0x005744F7..0x00574531`).
    pub(crate) fn contains(self, at: (i16, i16)) -> bool {
        let (left, top) = (i32::from(self.min_rx), i32::from(self.min_ry));
        (left..=left + i32::from(self.width)).contains(&i32::from(at.0))
            && (top..=top + i32::from(self.height)).contains(&i32::from(at.1))
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
#[cfg(any(test, feature = "test-support"))]
pub fn native_gather_start_positions(
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
#[cfg(any(test, feature = "test-support"))]
pub fn find_nearby_start_rect(
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
pub struct NativeStartAssignment {
    pub placements: Vec<(usize, Waypoint)>,
    pub start_table: Vec<Option<usize>>,
}

pub const HOUSE_CONSTRUCTOR_TIMER_MIN: u32 = 450;
pub const HOUSE_CONSTRUCTOR_TIMER_MAX: u32 = 1800;

/// The selected active-retail start callback used by offline noncampaign Full Init.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockOfflineStartCallbackFamily {
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
pub struct PreFillScenarioPrefixPlan {
    start_header: crate::map::preview::ScenarioStartHeader,
    projection: StockOfflinePrefixProjection,
    #[cfg(any(test, feature = "test-support"))]
    first_gathered_starts: Vec<Waypoint>,
    first_house_timers: Vec<u32>,
    second_house_timers: Vec<u32>,
    scenario_rng_before: SimRngLogicalState,
    scenario_rng_before_fingerprint: u64,
    scenario_rng_after: SimRngLogicalState,
    scenario_rng_after_cursor: SimRng,
    native_resize_width: u32,
    native_resize_height: u32,
    #[cfg(any(test, feature = "test-support"))]
    rng_checkpoints: ScenarioPrefixRngCheckpoints,
}

/// Paired Scenario-RNG and native numeric-ID prefix for one supported fresh
/// stock-offline Full_Init. Neither half can reach production independently.
#[derive(Debug)]
pub struct BoundStockOfflineFreshPrefixPlan {
    scenario: PreFillScenarioPrefixPlan,
    native_ids: crate::sim::native_identity::NativeFreshIdPrefixReceipt,
}

/// Family-specific inputs to the one pre-Fill Simulation handoff. Campaigns
/// construct the authored House array on this owner; they never synthesize an
/// offline start assignment or a draw-free projection of that assignment.
#[allow(
    clippy::large_enum_variant,
    reason = "built once per scenario load and consumed immediately"
)]
pub enum FreshScenarioPrefix<'a> {
    StockOffline(BoundStockOfflineFreshPrefixPlan),
    Campaign {
        native_rules_receipt: crate::rules::process_owner::NativeScenarioRulesReceipt,
        rules: &'a RuleSet,
        map_data: &'a MapFile,
        house_roster: &'a HouseRoster,
        difficulty: i32,
        mission_counter: i32,
        game_options: Option<&'a crate::sim::game_options::GameOptions>,
        campaign_index: i32,
        restart_carry_over: crate::sim::campaign_carry_over::CampaignCarryOver,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum FreshScenarioPrefixError {
    #[error(transparent)]
    Offline(#[from] PreFillScenarioPrefixPlanError),
    #[error(transparent)]
    Campaign(#[from] crate::sim::scenario_session::CampaignScenarioStartupError),
    #[error(transparent)]
    Identity(#[from] crate::sim::native_identity::NativeIdentityError),
    #[error("campaign House construction requires an authored [Houses] roster")]
    MissingCampaignHouseRoster,
    #[error("campaign House construction requires a registered entry0 Country")]
    MissingCampaignCountryRegistry,
    #[error("campaign House {house} requires unsupported Country constructor {country}")]
    UnsupportedCampaignCountryConstructor { house: String, country: String },
    #[error(
        "campaign Super constructors have {allocated} native types but {projected} registry identities"
    )]
    SuperWeaponRegistryMismatch { allocated: usize, projected: usize },
}

/// Draw-free stock-offline state retained after the prefix cursor receipt is
/// consumed.  It can place the final Houses and expose loading markers, but it
/// owns no RNG and cannot replay the prefix.
#[derive(Debug)]
pub struct StockOfflinePrefixProjection {
    /// Raw active Scenario waypoint table after the authored read or accepted
    /// RMG staging copy. Gather may derive fallback cells from this table, but
    /// loading markers and the live Scenario/session owner retain these exact
    /// entries rather than the Gather result or regenerated `.SED` table.
    active_scenario_waypoints: HashMap<u32, Waypoint>,
    final_gathered_starts: Vec<Waypoint>,
    assignment: NativeStartAssignment,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScenarioPrefixRngCheckpoints {
    pub(crate) after_first_house_pass: SimRngLogicalState,
    pub(crate) after_first_gather: SimRngLogicalState,
    pub(crate) after_second_gather_and_chooser: SimRngLogicalState,
    pub(crate) after_zero_draw_reset: SimRngLogicalState,
    pub(crate) after_second_house_pass: SimRngLogicalState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PreFillScenarioPrefixPlanError {
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
    #[error("accepted launch Country index {index} is absent after scenario Rules reset")]
    MissingLaunchCountry { index: u16 },
}

impl PreFillScenarioPrefixPlan {
    pub fn projection(&self) -> &StockOfflinePrefixProjection {
        &self.projection
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn first_gathered_starts(&self) -> &[Waypoint] {
        &self.first_gathered_starts
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn final_gathered_starts(&self) -> &[Waypoint] {
        self.projection.final_gathered_starts()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn assignment(&self) -> &NativeStartAssignment {
        self.projection.assignment()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn first_house_timers(&self) -> &[u32] {
        &self.first_house_timers
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn second_house_timers(&self) -> &[u32] {
        &self.second_house_timers
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn rng_checkpoints(&self) -> &ScenarioPrefixRngCheckpoints {
        &self.rng_checkpoints
    }

    /// Bind the move-only E/P Rules chronology to the already-validated House,
    /// Resize, and Scenario prefix. Type/House/Cell constructors advance only
    /// the independent native-ID cursor; Scenario RNG remains byte-for-byte
    /// owned by this plan.
    pub fn bind_native_rules_receipt(
        self,
        receipt: crate::rules::process_owner::NativeScenarioRulesReceipt,
        launch: &MatchLaunchDescriptor,
        rules: &RuleSet,
    ) -> Result<BoundStockOfflineFreshPrefixPlan, PreFillScenarioPrefixPlanError> {
        launch.validate_countries(rules)?;
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
        Ok(BoundStockOfflineFreshPrefixPlan {
            scenario: self,
            native_ids,
        })
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
    pub fn projection(&self) -> &StockOfflinePrefixProjection {
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
    pub fn active_scenario_waypoints(&self) -> &HashMap<u32, Waypoint> {
        &self.active_scenario_waypoints
    }

    pub fn final_gathered_starts(&self) -> &[Waypoint] {
        &self.final_gathered_starts
    }

    pub fn start_table(&self) -> &[Option<usize>] {
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
pub fn prepare_stock_offline_scenario_prefix_plan(
    descriptor: &MatchLaunchDescriptor,
    map_data: &MapFile,
    start_waypoints: &HashMap<u32, Waypoint>,
    launch_seed: u32,
) -> Result<PreFillScenarioPrefixPlan, PreFillScenarioPrefixPlanError> {
    let session = descriptor.session();
    let family = stock_offline_start_callback_family(session)?;
    let mut start_header = crate::map::preview::ScenarioStartHeader::default();
    start_header.read_ini(&map_data.ini);
    let launch_participant_count = (session
        .humans
        .iter()
        .filter(|human| !human.is_observer())
        .count() as i32)
        .wrapping_add(session.ai_player_count)
        .max(0) as usize;
    let roster_start_count = session.required_start_count();
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
    #[cfg(any(test, feature = "test-support"))]
    let after_first_house_pass = scenario_rng.logical_state();
    let first_gathered_starts = native_gather_pre_fill_start_positions(
        start_waypoints,
        roster_start_count,
        &map_data.header,
        &mut scenario_rng,
    )
    .ok_or(PreFillScenarioPrefixPlanError::InvalidMapCellExtent)?;
    #[cfg(any(test, feature = "test-support"))]
    let after_first_gather = scenario_rng.logical_state();
    let preassignment = native_preassign_launch_start_table(session, first_gathered_starts.len());

    let final_gathered_starts = native_gather_pre_fill_start_positions(
        start_waypoints,
        roster_start_count,
        &map_data.header,
        &mut scenario_rng,
    )
    .ok_or(PreFillScenarioPrefixPlanError::InvalidMapCellExtent)?;
    // 0x0068754B..0x00687581: inside GameMode != 0, WOL Quick Co-op
    // ([0xA8B244] == 2) runs AssignStartingPoints 0x005EE9D0 in place of the
    // mode's +0x84, whatever the mode.
    let assignment = if descriptor.quick_coop_starts {
        native_assign_quick_coop_starts_from_preassignment(
            session,
            &final_gathered_starts,
            &preassignment,
            &mut scenario_rng,
        )
    } else {
        match family {
            StockOfflineStartCallbackFamily::Battle => {
                native_assign_launch_starts_from_preassignment(
                    session,
                    &final_gathered_starts,
                    &preassignment,
                    &mut scenario_rng,
                )
            }
            StockOfflineStartCallbackFamily::Cooperative => {
                let human_start_spots = start_header.coop_human_start_spots().max(0) as usize;
                native_assign_cooperative_starts_from_preassignment(
                    session,
                    &final_gathered_starts,
                    &preassignment,
                    human_start_spots,
                    session.pre_fill_house_roster.nonobserver_human_count(),
                    &mut scenario_rng,
                )
            }
        }
    };
    #[cfg(any(test, feature = "test-support"))]
    let after_second_gather_and_chooser = scenario_rng.logical_state();

    // Native deletes every disposable first-pass House here. Destruction and
    // the following rules/basic reset consume no Scenario draw, so the second
    // pass begins at the chooser's exact cursor.
    #[cfg(any(test, feature = "test-support"))]
    let after_zero_draw_reset = scenario_rng.logical_state();
    let second_house_timers =
        advance_pre_fill_house_constructor_pass(&session.pre_fill_house_roster, &mut scenario_rng);
    let scenario_rng_after = scenario_rng.logical_state();
    #[cfg(any(test, feature = "test-support"))]
    let after_second_house_pass = scenario_rng_after.clone();

    Ok(PreFillScenarioPrefixPlan {
        start_header,
        projection: StockOfflinePrefixProjection {
            active_scenario_waypoints: start_waypoints.clone(),
            final_gathered_starts,
            assignment,
        },
        #[cfg(any(test, feature = "test-support"))]
        first_gathered_starts,
        first_house_timers,
        second_house_timers,
        scenario_rng_before,
        scenario_rng_before_fingerprint,
        scenario_rng_after,
        scenario_rng_after_cursor: scenario_rng,
        native_resize_width: map_data.header.width,
        native_resize_height: map_data.header.height,
        #[cfg(any(test, feature = "test-support"))]
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
        timers.push(draw_house_constructor_timer(rng));
    }
    for _slot in roster.ai_slots().iter().filter(|slot| slot.valid) {
        timers.push(draw_house_constructor_timer(rng));
    }
    for _fixed in roster.fixed_tail() {
        timers.push(draw_house_constructor_timer(rng));
    }
    timers
}

fn draw_house_constructor_timer(rng: &mut SimRng) -> u32 {
    rng.next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX)
}

pub fn stock_offline_start_callback_family(
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

#[cfg(any(test, feature = "test-support"))]
pub fn native_assign_launch_starts(
    session: &SkirmishLaunchSession,
    starts: &[Waypoint],
    rng: &mut SimRng,
) -> NativeStartAssignment {
    let preassignment = native_preassign_launch_start_table(session, starts.len());
    native_assign_launch_starts_from_preassignment(session, starts, &preassignment, rng)
}

fn launch_start_requests(session: &SkirmishLaunchSession) -> Vec<LaunchStartPosition> {
    normalized_launch_slots(session)
        .iter()
        .map(|slot| slot.start_position)
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

    let slots = normalized_launch_slots(session);
    let mut observer_reserved_first = false;
    let mut observer_claim_complete = false;
    // Battle5D6C70 visits House order; chooser5D6890 shares an observer's
    // viewpoint without reserving another start. ABFDD4/5 distinguish a first
    // observer reservation from an observer joining an already occupied view.
    for (slot, assigned_start) in assigned.iter_mut().enumerate() {
        let explicit = explicit_owner
            .iter()
            .rposition(|owner| *owner == Some(slot));
        let first_occupied = explicit_owner.iter().position(Option::is_some);
        let mut write_owner = true;
        let start_index = if let Some(explicit) = explicit {
            explicit
        } else if let Some(first) = first_occupied {
            if observer_reserved_first && !observer_claim_complete {
                observer_claim_complete = true;
                first
            } else if slots[slot].observer {
                observer_reserved_first = true;
                observer_claim_complete = true;
                write_owner = false;
                first
            } else {
                choose_battle_automatic_start(starts, &occupied, rng)
            }
        } else {
            observer_reserved_first = slots[slot].observer;
            observer_claim_complete = false;
            choose_battle_automatic_start(starts, &occupied, rng)
        };
        if write_owner {
            occupied[start_index] = true;
            explicit_owner[start_index] = Some(slot);
        }
        *assigned_start = Some(starts[start_index]);
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
#[cfg(any(test, feature = "test-support"))]
pub fn native_assign_cooperative_launch_starts(
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
        session
            .humans
            .iter()
            .filter(|human| !human.is_observer())
            .count(),
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

    for (slot, assigned_start) in assigned.iter_mut().enumerate() {
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
        *assigned_start = Some(starts[start_index]);
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

/// WOL Quick Co-op's start callback, `ScenarioClass__AssignStartingPoints`
/// (`0x005EE9D0..0x005EEBAA`). It snapshots `used` from the Scenario start
/// table once, then makes two passes over the Houses in order: humans
/// (`+0x1EC`) keep the last table entry naming them or pick with the human
/// flag; then every AI picks with no table check, so a preset AI start stays
/// reserved but unused. The `[Basic] Official` argument only reaches Gather,
/// which never reads it. It writes no node or AI-slot start back.
fn native_assign_quick_coop_starts_from_preassignment(
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
    let slots = normalized_launch_slots(session);
    let mut start_table = preassignment.to_vec();
    let mut used: Vec<bool> = start_table.iter().map(Option::is_some).collect();
    let mut assigned = vec![None; slots.len()];

    for human in [true, false] {
        for (slot, normalized) in slots.iter().enumerate() {
            if normalized.is_human != human {
                continue;
            }
            let preset = if human {
                start_table.iter().rposition(|owner| *owner == Some(slot))
            } else {
                None
            };
            let start_index = preset.unwrap_or_else(|| {
                let picked = choose_quick_coop_start(starts, &used, human, rng);
                used[picked] = true;
                start_table[picked] = Some(slot);
                picked
            });
            assigned[slot] = Some(starts[start_index]);
        }
    }

    NativeStartAssignment {
        placements: assigned
            .into_iter()
            .enumerate()
            .filter_map(|(slot, start)| start.map(|start| (slot, start)))
            .collect(),
        start_table,
    }
}

/// AssignStartingPoints' picker `0x005EE6F0`, with `k` the used starts. No
/// start used: a human draws `RandomRanged(0, N-1)` on the Scenario RNG; an
/// AI takes the farthest rule, which is the first start. Two used and an AI:
/// draw `r = RandomRanged(0, N-3)` and take the `(r+1)`-th free start in
/// index order. One used: the free start nearest the used one. Otherwise the
/// free start farthest from the used ones; ties keep the lowest index.
fn choose_quick_coop_start(
    starts: &[Waypoint],
    used: &[bool],
    human: bool,
    rng: &mut SimRng,
) -> usize {
    let used_count = used.iter().filter(|used| **used).count();
    if used_count == 0 && human {
        return rng.next_range_u32_inclusive(0, starts.len() as u32 - 1) as usize;
    }
    if used_count == 2 && !human {
        let r = rng.next_range_u32_inclusive(0, starts.len() as u32 - 3) as usize;
        if let Some(free) = (0..starts.len()).filter(|index| !used[*index]).nth(r) {
            return free;
        }
    }
    // Each distance is truncated to 16 bits before the 32-bit sum.
    let distance_sum = |candidate: usize| -> i32 {
        (0..starts.len())
            .filter(|other| used[*other])
            .map(|other| i32::from(native_start_distance(starts[candidate], starts[other]) as i16))
            .sum()
    };
    let free = (0..starts.len()).filter(|index| !used[*index]);
    let picked = if used_count == 1 {
        // Strict `<`, so the first minimum wins.
        free.fold(None::<(usize, i32)>, |best, candidate| {
            let sum = distance_sum(candidate);
            match best {
                Some((_, best_sum)) if sum >= best_sum => best,
                _ => Some((candidate, sum)),
            }
        })
    } else {
        // Strict `>` from -1, so ties keep the lowest index.
        free.fold(None::<(usize, i32)>, |best, candidate| {
            let sum = distance_sum(candidate);
            match best {
                Some((_, best_sum)) if sum <= best_sum => best,
                _ => Some((candidate, sum)),
            }
        })
    };
    picked
        .expect("participant count cannot exceed gathered starts")
        .0
}

pub fn choose_battle_automatic_start(
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

pub fn native_start_distance(left: Waypoint, right: Waypoint) -> i32 {
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
pub struct SkirmishLaunchApplyResult {
    pub local_owner: Option<String>,
    pub spawned_mcvs: u32,
    pub active_slots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedSkirmishSlot {
    pub owner_name: String,
    pub(crate) country: LaunchCountry,
    pub color_index: u8,
    pub(crate) start_position: LaunchStartPosition,
    pub(crate) team: LaunchTeam,
    pub is_human: bool,
    pub observer: bool,
    pub source_human_index: Option<usize>,
    /// A human node's handle, CreateHouses' source for House+0x1602A.
    pub handle: Option<String>,
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
    /// WOL Quick Co-op (`[0xA8B244] == 2`) at scenario load: the start
    /// callback is AssignStartingPoints `0x005EE9D0`.
    quick_coop_starts: bool,
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
        for human in &session.humans {
            if human.slot.country_random {
                return Err(UnresolvedShellChoice {
                    ai_slot: None,
                    choice: "country",
                });
            }
            if human.slot.color_random {
                return Err(UnresolvedShellChoice {
                    ai_slot: None,
                    choice: "color",
                });
            }
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
        Ok(Self {
            session,
            quick_coop_starts: false,
        })
    }

    /// Assign starts as WOL Quick Co-op does (`0x0068755E`).
    pub fn with_quick_coop_starts(mut self) -> Self {
        self.quick_coop_starts = true;
        self
    }

    /// Full_Init6876AC deletes all HouseTypes (6689C3..6689E5), then
    /// rebuilds them before CreateHouses687FBD indexes the final array without
    /// a bounds gate. Reject a disappeared shell allocation rather than
    /// reconstructing it or entering the native undefined-pointer domain.
    pub fn validate_countries(
        &self,
        rules: &RuleSet,
    ) -> Result<(), PreFillScenarioPrefixPlanError> {
        for slot in normalized_launch_slots(&self.session) {
            if rules.country_name(slot.country.index()).is_none() {
                return Err(PreFillScenarioPrefixPlanError::MissingLaunchCountry {
                    index: slot.country.raw_index(),
                });
            }
        }
        Ok(())
    }

    /// The resolved session's gameplay facts.
    pub fn session(&self) -> &SkirmishLaunchSession {
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
pub fn initialize_skirmish_launch_houses(
    sim: &mut Simulation,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    descriptor: &MatchLaunchDescriptor,
) {
    let session = descriptor.session();
    assert!(
        sim.houses.is_empty()
            && sim.houses.order().is_empty()
            && sim.entities().is_empty()
            && sim.terrain_objects.objects().is_empty(),
        "skirmish houses must be initialized before map objects"
    );
    let slots = normalized_launch_slots(session);
    sim.session
        .install_game_options(session.options.to_game_options(session.ai_player_count));
    populate_launch_houses(sim, &slots, rules);
    // 688109 installs the original participant-array index0. These normalized
    // slots retain that launch identity; later MCV placement/viewer choice
    // cannot clear or replace the current house.
    let current_house = slots
        .iter()
        .find(|slot| slot.source_human_index == Some(session.local_player_index))
        .map(|slot| {
            let id = sim
                .interner
                .get(&slot.owner_name)
                .expect("created launch house");
            assert!(sim.houses.contains_key(&id));
            id
        });
    sim.session.install_current_house(current_house);
    // Full_Init68782D/687938 takes the local participant's Country.Side,
    // before the color-ordered House array is consulted. Native source node0
    // is the current local node (CreateHouses6880F8..68810F); VERA represents
    // that identity by local_player_index, independently of host node order.
    let scenario_side = session
        .humans
        .get(session.local_player_index)
        .map(|human| {
            let country = if human.is_observer() {
                LaunchCountry::America
            } else {
                human.slot.country
            };
            let name = rules
                .country_name(country.index())
                .expect("accepted local Country exists");
            rules.country_side_index_raw(name).unwrap_or(0)
        })
        .unwrap_or(0);
    sim.session.install_scenario_side(scenario_side);
    let observer = slots
        .iter()
        .rev()
        .find(|slot| slot.observer)
        .and_then(|slot| sim.interner.get(&slot.owner_name));
    sim.session
        .install_multiplayer_house_identity(session.mode.class, observer);
    // 688109..10F sets player-control only for the local source node;
    // remote human Houses remain human without acquiring local control.
    for slot in &slots {
        if let Some(id) = sim.interner.get(&slot.owner_name) {
            sim.houses
                .get_mut(&id)
                .expect("created launch House")
                .assign_player_control(slot.source_human_index == Some(session.local_player_index));
        }
    }
    populate_special_houses(sim, house_roster, rules);
}

/// Commit the explicit launch-session diplomacy graph at the post-crate boundary.
pub(crate) fn apply_skirmish_launch_alliances(
    sim: &mut Simulation,
    house_roster: &HouseRoster,
    session: &SkirmishLaunchSession,
    rules: &RuleSet,
) {
    let slots = normalized_launch_slots(session);
    sim.install_house_alliances(
        crate::sim::house_diplomacy::HouseAllianceAdmission::Admitted(launch_alliance_map(
            house_roster,
            &slots,
            &session.mode,
        )),
        rules,
    );
}

/// Test-only compatibility path for direct post-Fill launch fixtures.
#[cfg(any(test, feature = "test-support"))]
pub fn apply_explicit_skirmish_launch_session(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    descriptor: &MatchLaunchDescriptor,
) -> SkirmishLaunchApplyResult {
    apply_resolved_skirmish_launch_session(
        sim,
        map_data,
        house_roster,
        rules,
        resolved_terrain,
        descriptor,
        None,
        LaunchStartResolution::PostFillTestCompatibility,
    )
}

/// Apply the retained stock-offline House/start projection without repeating
/// any prefix draw. Existing starting-force and later initialization draws
/// remain after this projection.
#[cfg(any(test, feature = "test-support"))]
pub fn apply_pre_fill_scenario_prefix_launch_session(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    descriptor: &MatchLaunchDescriptor,
    plan: &StockOfflinePrefixProjection,
) -> SkirmishLaunchApplyResult {
    apply_resolved_skirmish_launch_session(
        sim,
        map_data,
        house_roster,
        rules,
        resolved_terrain,
        descriptor,
        None,
        LaunchStartResolution::Prefix(plan),
    )
}

pub fn apply_pre_fill_scenario_prefix_launch_session_with_overlay_registry(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
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
        resolved_terrain,
        descriptor,
        Some(overlay_registry),
        LaunchStartResolution::Prefix(plan),
    )
}

enum LaunchStartResolution<'a> {
    Prefix(&'a StockOfflinePrefixProjection),
    #[cfg(any(test, feature = "test-support"))]
    PostFillTestCompatibility,
}

fn apply_resolved_skirmish_launch_session(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
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
        sim.houses.order().len(),
        slots.len() + 2,
        "launch House array must be complete before start assignment"
    );
    for (registered, slot) in sim.houses.order().iter().zip(&slots) {
        assert_eq!(
            sim.interner.resolve(*registered),
            slot.owner_name,
            "participant House order changed after map construction"
        );
    }
    for (registered, expected) in sim.houses.order()[slots.len()..]
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
        #[cfg(any(test, feature = "test-support"))]
        LaunchStartResolution::PostFillTestCompatibility => {
            let cooperative = session.mode.class == crate::skirmish_modes::MpModeClass::Cooperative;
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
                let mut header = crate::map::preview::ScenarioStartHeader::default();
                header.read_ini(&map_data.ini);
                sim.session.install_start_header(header);
                let human_start_spots =
                    sim.session.start_header().coop_human_start_spots().max(0) as usize;
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
    let mut local_owner = slots
        .iter()
        .find(|slot| slot.source_human_index == Some(session.local_player_index))
        .map(|slot| slot.owner_name.clone());

    if session.options.bases {
        for (slot_idx, waypoint) in assignments {
            let Some(slot) = slots.get(*slot_idx) else {
                continue;
            };
            let Some(country) = crate::sim::house_state::house_state_for_owner(
                &sim.houses,
                &slot.owner_name,
                &sim.interner,
            )
            .and_then(|house| house.country())
            .and_then(|name| rules.country_index(sim.interner.resolve(name))) else {
                continue;
            };
            // GenerateStartingUnits5D6EF0..5D6EF8 skips passive Countries
            // before either the MCV (+C8) or extra-unit (+CC) callback.
            if slot.observer
                || rules
                    .country_name(country)
                    .is_some_and(|name| rules.country_multiplay_passive(name))
            {
                continue;
            }
            // Battle5D7062 skips Unit construction and its RNG when505310
            // finds no Owner-compatible BaseUnit. The House still exists.
            let Some(mcv_type) = launch_mcv_type_for_country(
                LaunchCountry::from_country_index(u32::from(country.0)),
                rules,
            ) else {
                continue;
            };
            if place_starting_mcv(
                sim,
                mcv_type,
                &slot.owner_name,
                waypoint.rx,
                waypoint.ry,
                bounds,
                rules,
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
                if slot.source_human_index == Some(session.local_player_index) {
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

    apply_launch_shroud_option(
        sim,
        crate::sim::world::UninitContext::new(rules, overlay_registry),
    );

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
    let mut table = Vec::new();
    for (start_idx, slot_idx) in start_assignment.start_table.iter().enumerate() {
        let Some(slot_idx) = *slot_idx else {
            continue;
        };
        let Some(slot) = slots.get(slot_idx) else {
            continue;
        };
        table.push((start_idx as u32, sim.interner.intern(&slot.owner_name)));
    }
    sim.session.install_start_slot_houses(table);
    assign_launch_base_centers(sim, slots, &start_assignment.placements);
}

/// `Read_Scenario`'s `Shroud=no` start (`0x006849D0..0x006849E7`): with
/// Rules+0x14AE clear, `RevealEntireMap(NULL)` explores the whole map for the
/// current house ([`Simulation::reveal_entire_map`]). AI houses keep their own
/// unexplored planes; no sight, gap pass or map-clear byte is written.
///
/// RESIDUAL: native reveals after `Post_Map_Init` (`0x00684990`), so after
/// the startup crates, AI credits and alliances that VERA's
/// `finalize_scenario_post_map` runs later. The reveal reads none of them,
/// and a noncampaign CellPUT already discovered every object for the current
/// house (`discover_cell_put_object`), so no effect is known.
pub fn apply_launch_shroud_option(
    sim: &mut Simulation,
    context: crate::sim::world::UninitContext<'_>,
) {
    if sim.session.game_options().shroud {
        return;
    }
    sim.reveal_entire_map(None, context);
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
        // The House edge (+0x577C) keeps the constructor's North (0x004F5C79)
        // until a yard's DetermineEdge (0x0050DB00); a start does not set it.
        if let Some(house) = crate::sim::house_state::house_state_for_owner_mut(
            &mut sim.houses,
            &slot.owner_name,
            &sim.interner,
        ) {
            house.set_base_center((waypoint.rx, waypoint.ry));
        }
    }
}

pub fn normalized_launch_slots(session: &SkirmishLaunchSession) -> Vec<NormalizedSkirmishSlot> {
    // CreateHouses687F66..84 compares signed DWORD node+53; equal colors
    // retain source order. 688137 writes the House index back to each node.
    // Internal owner keys must stay distinct even for duplicate handles or
    // handles colliding with generated AI/fixed Houses (688092..6880A4).
    let mut used_names: Vec<String> = ["Neutral", "Special"]
        .into_iter()
        .map(normalize_house_name)
        .collect();
    used_names.extend(
        (1..=session.opponents.len())
            .map(|index| normalize_house_name(&format!("Computer{index}"))),
    );
    let mut humans: Vec<_> = session.humans.iter().enumerate().collect();
    humans.sort_by_key(|(index, human)| (human.slot.color_index, *index));
    let mut slots = Vec::with_capacity(humans.len() + session.opponents.len());
    for (index, human) in humans {
        let mut owner_name = human.name.clone();
        let mut suffix = 0;
        while used_names.contains(&normalize_house_name(&owner_name)) {
            owner_name = if suffix == 0 {
                "Player".into()
            } else {
                format!("Player{suffix}")
            };
            suffix += 1;
        }
        used_names.push(normalize_house_name(&owner_name));
        slots.push(NormalizedSkirmishSlot {
            owner_name,
            country: if human.is_observer() {
                LaunchCountry::America
            } else {
                human.slot.country
            },
            color_index: human.slot.color_index,
            start_position: human.slot.start_position,
            team: human.slot.team,
            is_human: true,
            observer: human.is_observer(),
            source_human_index: Some(index),
            handle: Some(human.name.clone()),
            difficulty: HouseDifficulty::Normal,
        });
    }
    for (index, opponent) in session.opponents.iter().enumerate() {
        slots.push(NormalizedSkirmishSlot {
            owner_name: format!("Computer{}", index + 1),
            country: opponent.country,
            color_index: opponent.color_index,
            start_position: opponent.start_position,
            team: opponent.team,
            is_human: false,
            observer: false,
            source_human_index: None,
            handle: None,
            difficulty: HouseDifficulty::from_native(opponent.difficulty.as_i32())
                .expect("native AI difficulty"),
        });
    }
    slots
}

pub fn populate_special_houses(sim: &mut Simulation, house_roster: &HouseRoster, rules: &RuleSet) {
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
        // Original gamemd CreateHouses6882C2..68837E constructs this fixed
        // tail without participant SetCreditsAndColor4FCE00. House ctor
        // 4F58D8 leaves cash0; launch credits belong to participants.
        let mut house = HouseState::new(
            name_id,
            side_idx,
            country_id,
            false,
            0,
            sim.session.game_options().tech_level,
        );
        // The House constructor builds its whole Supers vector.
        crate::sim::superweapon::construct_house_supers(sim, rules, name_id, false);
        // House4F62BA copies its HouseType's ID. These fixed-tail Houses do
        // not pass the generated participant-name writers below.
        house.initialize_scenario_name(
            rules
                .scenario_country_name(Some(country_name))
                .unwrap_or(country_name),
        );
        // Stock Neutral/Special are MultiplayPassive, which keeps them out of
        // defeat evaluation and out of the game-over alive scan. `country_name`
        // is always concrete here — the roster's `Country=` when the map named
        // one, otherwise the special house's own `[Countries]` entry, which is
        // what this house is being built as.
        house.resolve_multiplay_passive(rules, Some(country_name));
        house.project_country_mults(rules, &sim.interner);
        sim.houses.register(house);
    }
}

pub fn populate_launch_houses(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    rules: &RuleSet,
) {
    let human_count = slots.iter().filter(|slot| slot.is_human).count();
    for slot in slots {
        let name_id = sim.interner.intern(&slot.owner_name);
        let country_name = rules
            .country_name(slot.country.index())
            .expect("accepted launch Country must exist in the canonical Rules registry");
        let country_id = sim.interner.intern(country_name);
        let side_index =
            crate::sim::house_state::resolve_house_side_index(rules, Some(country_name), None, 0);
        let mut house = HouseState::new(
            name_id,
            side_index,
            Some(country_id),
            slot.is_human,
            sim.session.game_options().starting_credits,
            sim.session.game_options().tech_level,
        );
        // The House constructor builds its whole Supers vector.
        crate::sim::superweapon::construct_house_supers(sim, rules, name_id, false);
        // CreateHouses overwrites the constructor name independently of the
        // display handle: GameMode 4 gives a human its node's name narrowed
        // to 20 bytes (0x687FCE..0x688043), other modes "<human player>"
        // (0x68804A..0x688092); every computer gets "Computer"
        // (0x688210..0x688252). Every computer keeps the same native name;
        // its internal key stays distinct, so FromName can select the first
        // array entry.
        // RESIDUAL: the node name's wide-to-narrow conversion (0x7B7F90) is
        // taken as the handle's first 20 characters; WOL nicknames are ASCII.
        let wol_handle = slot
            .handle
            .as_deref()
            .filter(|_| {
                slot.is_human
                    && sim.session.game_mode()
                        == crate::sim::scenario_session::ScenarioGameMode::Wol
            })
            .map(|handle| handle.chars().take(20).collect::<String>());
        house.initialize_scenario_name(match (&wol_handle, slot.is_human) {
            (Some(handle), _) => handle,
            (None, true) => "<human player>",
            (None, false) => "Computer",
        });
        // The same writers give a human its own node's handle and every
        // computer TXT_COMPUTER as its UI name, in every mode.
        if slot.is_human {
            if let Some(handle) = &slot.handle {
                house.set_ui_name(crate::sim::house_state::HouseUiName::handle(handle));
            }
        } else {
            house.set_ui_name(crate::sim::house_state::HouseUiName::Computer);
        }
        // `ScenarioClass::Create_Houses` passes every launch house through
        // `HouseClass::SetDifficulty` (humans with 1, `0x00688132`; computer
        // slots with their own, `0x006882B9`); Neutral and Special are never
        // passed and keep the constructor's values.
        // CreateHouses688293..6882B9 counts every human node, including
        // observers, before applying Rules[AI].CompEasyBonus to positive AI
        // difficulty values. The unchanged House owner applies its biases.
        let difficulty = if !slot.is_human && human_count > 1 && rules.general.comp_easy_bonus {
            HouseDifficulty::from_native((slot.difficulty as i32 - 1).max(0))
                .expect("native AI difficulty")
        } else {
            slot.difficulty
        };
        house.set_difficulty(
            difficulty,
            &rules.general,
            rules.country_difficulty_biases(country_name),
            sim.session.game_mode_nonzero(),
            sim.houses.order().len() as i32,
            sim.session.binary_frame() as i32,
        );
        // ScenarioClass::Create_Houses leaves generated human CurrentIQ at
        // the constructor value zero and stamps generated computer slots with
        // Rules.MaxIQLevels in non-campaign sessions.
        if !slot.is_human {
            house.stamp_computer_iq(rules.general.max_iq_levels);
            // `0x006881F5..0x00688209`: a computer slot's team marks the
            // Scenario, which turns Computer_Paranoid off.
            if slot.team != LaunchTeam::None {
                sim.session.set_computer_slot_has_team();
            }
        }
        house.resolve_multiplay_passive(rules, Some(country_name));
        house.project_country_mults(rules, &sim.interner);
        sim.houses.register(house);
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
pub fn apply_skirmish_ai_opening_credits(houses: &mut crate::sim::houses::Houses, rules: &RuleSet) {
    for house in houses.values_mut() {
        if house.is_human() || house.multiplay_passive() {
            continue;
        }
        let coefficient = rules
            .general
            .multiplayer_ai_cm
            .get(house.difficulty().table_index())
            .copied()
            .unwrap_or(0);
        let grant = native_ai_opening_grant(coefficient, house.economy().available_money());
        house.economy_mut().add_credits(grant);
    }
}

pub fn launch_alliance_map(
    house_roster: &HouseRoster,
    slots: &[NormalizedSkirmishSlot],
    mode: &crate::skirmish_launch::SkirmishLaunchMode,
) -> crate::map::houses::HouseAllianceMap {
    let mut alliances = house_roster.alliance_map();
    for slot in slots {
        alliances
            .entry(normalize_house_name(&slot.owner_name))
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
            let left_key = normalize_house_name(&left.owner_name);
            let right_key = normalize_house_name(&right.owner_name);
            alliances
                .entry(left_key.clone())
                .or_default()
                .insert(right_key.clone());
            alliances.entry(right_key).or_default().insert(left_key);
        }
    }
    if mode.class == crate::skirmish_modes::MpModeClass::Cooperative {
        for left in slots {
            for right in slots {
                if left.owner_name == right.owner_name || left.is_human != right.is_human {
                    continue;
                }
                let left_key = normalize_house_name(&left.owner_name);
                let right_key = normalize_house_name(&right.owner_name);
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

pub const STARTING_MCV_FACING: u8 = 64;
pub const STARTING_MCV_FALLBACK_MAX_RADIUS: i32 = 31;
pub const STARTING_EXTRA_UNIT_FALLBACK_START_RADIUS: i32 = 4;

fn place_starting_mcv(
    sim: &mut Simulation,
    mcv_type: &str,
    owner: &str,
    base_rx: u16,
    base_ry: u16,
    bounds: NativeStartBounds,
    rules: &RuleSet,
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
    resolved_terrain: &ResolvedTerrainGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<u64> {
    let category = rules.object(type_id)?.category;
    let initial_z = resolved_terrain
        .cell(base_rx, base_ry)
        .map_or(0, |cell| cell.level);
    // Active retail constructs one Techno before exact/fallback Unlimbo. The
    // one constructor draw therefore precedes every placement-search draw and
    // remains spent even when every attempt fails.
    let stable_id = sim.construct_object_limbo_at_height(
        type_id,
        owner,
        base_rx,
        base_ry,
        facing,
        initial_z,
        rules,
        crate::sim::world::FrameEffects::default(),
    )?;
    if starting_object_cell_placeable(sim, resolved_terrain, base_rx, base_ry, category)
        && sim
            .reveal_constructed_object_at_height_with_overlay_context(
                stable_id,
                base_rx,
                base_ry,
                facing,
                initial_z,
                PlacementEvidence::EvaluateMark,
                rules,
                overlay_registry,
                crate::sim::world::FrameEffects::default(),
            )
            .is_some()
    {
        return Some(stable_id);
    }

    for radius in start_radius..=STARTING_MCV_FALLBACK_MAX_RADIUS {
        let start_direction = sim.scenario_rng.next_range_u32_inclusive(0, 7) as usize;
        for jitter_pass in 0..2 {
            for offset in 0..8 {
                let direction = (start_direction + offset) & 7;
                let (dx, dy) = DIRECTION_DELTAS[direction];
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
                let z = resolved_terrain.cell(rx, ry).map_or(0, |cell| cell.level);
                if sim
                    .reveal_constructed_object_at_height_with_overlay_context(
                        stable_id,
                        rx,
                        ry,
                        facing,
                        z,
                        PlacementEvidence::EvaluateMark,
                        rules,
                        overlay_registry,
                        crate::sim::world::FrameEffects::default(),
                    )
                    .is_some()
                {
                    return Some(stable_id);
                }
            }
        }
    }

    sim.discard_constructed_limbo(stable_id, rules, crate::sim::world::FrameEffects::default());
    None
}

#[derive(Debug, Clone)]
pub struct StartingUnitCandidate {
    pub type_id: String,
    pub(crate) cost: i32,
}

pub const STARTING_EXTRA_UNIT_MAX_PLACEMENT_FAILURES: u8 = 20;

pub fn starting_unit_prefers_vehicle(spent: i32, budget: i32) -> bool {
    // gamemd 0x005D7337..0x005D7349 compares remaining budget against
    // trunc(initial_budget / 3). Expressed through `spent`, the strict
    // boundary is initial_budget - trunc(initial_budget / 3).
    spent < budget.wrapping_sub(budget / 3)
}

#[cfg(any(test, feature = "test-support"))]
pub fn seed_starting_extra_units(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    bounds: NativeStartBounds,
    unit_count: i32,
    initial_veteran: bool,
) -> u32 {
    seed_starting_extra_units_with_overlay_registry(
        sim,
        slots,
        rules,
        resolved_terrain,
        bounds,
        unit_count,
        initial_veteran,
        None,
    )
}

fn seed_starting_extra_units_with_overlay_registry(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    rules: &RuleSet,
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
        sim.houses.values().filter_map(|house| {
            house
                .country()
                .and_then(|name| rules.country_index(sim.interner.resolve(name)))
        }),
        sim.session.game_options().tech_level,
        unit_count,
    );
    if budget <= 0 {
        return 0;
    }
    let mut spawned = 0;

    for slot in slots {
        // 5D6EF0..F8 and5D6F4A..53 precede both starting-force callbacks.
        if slot.observer {
            continue;
        }
        let Some((base_center, tech_level, country_name)) =
            crate::sim::house_state::house_state_for_owner(
                &sim.houses,
                &slot.owner_name,
                &sim.interner,
            )
            .and_then(|house| {
                Some((
                    house.base_center()?,
                    house.tech_level(),
                    sim.interner.resolve(house.country()?),
                ))
            })
        else {
            continue;
        };
        if rules.country_multiplay_passive(country_name) {
            continue;
        }
        let Some(country) = rules.country_index(country_name) else {
            continue;
        };
        //5D70F0 compares the House's +1D4, not the global budget level.
        let candidates = starting_unit_candidates_for_country(
            rules,
            LaunchCountry::from_country_index(u32::from(country.0)),
            tech_level,
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
            if initial_veteran && let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
                entity.veterancy.set_elite();
            }
            let mission = if slot.is_human {
                MissionType::Guard
            } else {
                MissionType::AreaGuard
            };
            let _ = sim.substrate.mission_assign_exact(
                stable_id,
                MissionId::from_known(mission),
                sim.session.binary_frame(),
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
            && !house.multiplay_passive()
        {
            house.economy_mut().add_credits(remaining);
        }
    }

    spawned
}

pub fn starting_unit_budget(
    rules: &RuleSet,
    countries: impl IntoIterator<Item = crate::rules::ruleset::CountryIdx>,
    tech_level: i32,
    unit_count: i32,
) -> i32 {
    if unit_count <= 0 {
        return 0;
    }
    // GenerateStartingUnits5D6D80:5D6DAF..5D6DCB ORs actual Country+B4
    // bits from non-passive Countries. Observer filtering is later; neither
    // ParentCountry nor production's Required/ForbiddenHouses enters this
    // budget or5D70F0's candidate lists. Executed controls:
    // tools/rules_oracle/session_country_starting_ownership.{json,md}.
    let country_mask = countries.into_iter().fold(0u32, |mask, country| {
        let Some(name) = rules.country_name(country) else {
            return mask;
        };
        if rules.country_multiplay_passive(name) {
            mask
        } else {
            mask | crate::sim::ai_buildable::country_bit(country)
        }
    });
    let mut total_cost: i32 = 0;
    let mut eligible_count: i32 = 0;
    for id in &rules.vehicle_ids {
        let Some(object) = rules.object(id) else {
            continue;
        };
        if !starting_unit_vehicle_allowed(rules, object, tech_level)
            || !crate::sim::ai_buildable::owner_allows(object, country_mask, rules)
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
        if !starting_unit_infantry_allowed(object, tech_level)
            || !crate::sim::ai_buildable::owner_allows(object, country_mask, rules)
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
pub struct StartingUnitCandidates {
    pub vehicles: Vec<StartingUnitCandidate>,
    pub infantry: Vec<StartingUnitCandidate>,
}

pub fn starting_unit_candidates_for_country(
    rules: &RuleSet,
    country: LaunchCountry,
    tech_level: i32,
) -> StartingUnitCandidates {
    if rules.country_name(country.index()).is_none() {
        return StartingUnitCandidates::default();
    }
    // GenerateStartingUnitsForHouse5D70F0 uses the actual Country+B4 bit,
    // unlike the MCV's separate505310 ParentCountry lookup. Reuse the one
    // shared Owner-mask decision; production admission is not called here.
    let country_mask = crate::sim::ai_buildable::country_bit(country.index());
    let vehicles = rules
        .vehicle_ids
        .iter()
        .filter_map(|id| {
            let object = rules.object(id)?;
            if !starting_unit_vehicle_allowed(rules, object, tech_level)
                || !crate::sim::ai_buildable::owner_allows(object, country_mask, rules)
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
                || !crate::sim::ai_buildable::owner_allows(object, country_mask, rules)
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
        sim.playfield_bounds(),
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
                .is_some_and(|entity| entity.category() == EntityCategory::Infantry)
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

/// Battle5D7030 uses505310's Owner-only lookup, not production gates.
pub fn launch_mcv_type_for_country(country: LaunchCountry, rules: &RuleSet) -> Option<&str> {
    let name = rules.country_name(country.index())?;
    crate::sim::ai_buildable::first_owner_compatible(
        rules,
        &rules.general.base_unit_types,
        crate::rules::object_type::ObjectCategory::Vehicle,
        rules.country_owner_mask(name),
    )
    .map(|object| object.id.as_str())
}

/// Opaque owner for the RNG cursors used while a map becomes a world.
///
/// The app may drive content-loading algorithms through the two narrow draw
/// wrappers and may transport the launch-generated MapGen continuation, but it
/// cannot replace, extract, or draw from those cursors. Consuming this owner is
/// the only production handoff into a freshly constructed Simulation.
pub struct ScenarioBootstrapRng {
    scenario: SimRng,
    main: SimRng,
    mapgen: Option<SimRng>,
}

/// Scenario-stream access granted only to the terrain Fill callback.
pub struct ScenarioFillRng<'a> {
    rng: &'a mut SimRng,
}

impl ScenarioFillRng<'_> {
    pub fn next_range_u32_inclusive(&mut self, low: u32, high: u32) -> u32 {
        self.rng.next_range_u32_inclusive(low, high)
    }
}

impl ScenarioBootstrapRng {
    pub fn new(seed: u32) -> Self {
        let seed = u64::from(seed);
        Self {
            scenario: SimRng::new(seed),
            main: SimRng::new(seed),
            mapgen: None,
        }
    }

    /// Install the previous process-global cursor exactly once. Authored
    /// loads retain it; a completed .SED generation has already published its
    /// newer cursor through the app's one process-retention owner.
    pub fn install_process_mapgen_continuation(&mut self, continuation: MapGenRngContinuation) {
        assert!(
            self.mapgen.is_none(),
            "process MapGen continuation may only be installed once"
        );
        self.mapgen = Some(SimRng::from_mapgen_continuation(continuation));
    }

    /// Install the already-resolved stock-offline pre-Fill prefix exactly once.
    #[cfg(any(test, feature = "test-support"))]
    pub fn install_pre_fill_scenario_prefix_plan(
        &mut self,
        plan: PreFillScenarioPrefixPlan,
    ) -> Result<StockOfflinePrefixProjection, PreFillScenarioPrefixPlanError> {
        plan.install_before_terrain(&mut self.scenario)
    }

    /// Consume the paired stock-offline RNG/native-ID prefix into the one staged
    /// Simulation before terrain Fill.
    pub fn into_stock_offline_staged_simulation(
        mut self,
        descriptor: &ScenarioDescriptor,
        bound_prefix: BoundStockOfflineFreshPrefixPlan,
    ) -> Result<(Simulation, StockOfflinePrefixProjection), PreFillScenarioPrefixPlanError> {
        let (scenario_prefix, native_id_prefix) = bound_prefix.into_parts();
        let start_header = scenario_prefix.start_header;
        let projection = scenario_prefix.install_before_terrain(&mut self.scenario)?;
        let mut simulation = self.into_simulation(descriptor);
        simulation.session.install_start_header(start_header);
        simulation.install_native_unique_id_cursor(native_id_prefix.into_cursor());
        Ok((simulation, projection))
    }

    /// Consume every fresh family through the same staged Simulation, before
    /// terrain Fill can draw or construct any map object. The campaign arm
    /// follows original Full_Init686B20: E/P types, one actual House/Super
    /// generation, current-House selection, then one Resize checkpoint.
    pub fn into_fresh_staged_simulation(
        self,
        descriptor: &ScenarioDescriptor,
        prefix: FreshScenarioPrefix<'_>,
    ) -> Result<(Simulation, Option<StockOfflinePrefixProjection>), FreshScenarioPrefixError> {
        match prefix {
            FreshScenarioPrefix::StockOffline(bound_prefix) => {
                let (simulation, projection) =
                    self.into_stock_offline_staged_simulation(descriptor, bound_prefix)?;
                Ok((simulation, Some(projection)))
            }
            FreshScenarioPrefix::Campaign {
                native_rules_receipt,
                rules,
                map_data,
                house_roster,
                difficulty,
                mission_counter,
                game_options,
                campaign_index,
                restart_carry_over,
            } => {
                NativeStartBounds::from_map_header(&map_data.header)
                    .ok_or(PreFillScenarioPrefixPlanError::InvalidMapCellExtent)?;
                if house_roster.houses.is_empty() {
                    return Err(FreshScenarioPrefixError::MissingCampaignHouseRoster);
                }
                // ReadHouseType475540 can allocate an unknown nonempty
                // Country at4755B8. That path must extend the live Rules
                // registry/ID owner; the selected stock route has only known
                // or missing tokens. Reject it before any House IDs/RNG,
                // rather than silently binding another Country's defaults.
                for house in &house_roster.houses {
                    if rules
                        .scenario_country_name(house.country.as_deref())
                        .is_none()
                    {
                        return Err(match &house.country {
                            Some(country) => {
                                FreshScenarioPrefixError::UnsupportedCampaignCountryConstructor {
                                    house: house.name.clone(),
                                    country: country.clone(),
                                }
                            }
                            None => FreshScenarioPrefixError::MissingCampaignCountryRegistry,
                        });
                    }
                }
                let (early, rebuilt) = native_rules_receipt.into_parts();
                let (early_events, _) = early.into_parts();
                let (rebuilt_events, super_weapon_type_count) = rebuilt.into_parts();
                if super_weapon_type_count != rules.super_weapon_order.len() {
                    return Err(FreshScenarioPrefixError::SuperWeaponRegistryMismatch {
                        allocated: super_weapon_type_count,
                        projected: rules.super_weapon_order.len(),
                    });
                }
                // House4F6202..4F6290 constructs every registered Super via
                // 6CAF90, even when Type+B4 is -1 or has no implemented
                // dispatch body. The instance owner already uses this order;
                // launch support is not a prerequisite for House admission.
                // Evidence: input_oracle/campaign_start.py --houses (native
                // SuperType registration without its later body reader).
                let mut simulation = self.into_simulation(descriptor);
                let mut start_header = crate::map::preview::ScenarioStartHeader::default();
                start_header.read_ini(&map_data.ini);
                simulation.session.install_start_header(start_header);
                simulation.session.initialize_campaign_startup(
                    difficulty,
                    mission_counter,
                    game_options,
                    campaign_index,
                    restart_carry_over,
                )?;
                simulation.install_native_unique_id_cursor(
                    crate::sim::native_identity::NativeUniqueIdCursor::begin_campaign_prefix(
                        early_events.len(),
                        rebuilt_events.len(),
                    ),
                );
                initialize_map_roster_houses(
                    &mut simulation,
                    house_roster,
                    rules,
                    Some(&map_data.ini),
                );
                initialize_campaign_current_house(
                    &mut simulation.session,
                    &mut simulation.houses,
                    &map_data.ini,
                );
                initialize_campaign_scenario_side(&mut simulation, &map_data.ini, rules);
                simulation
                    .native_unique_ids
                    .as_mut()
                    .expect("campaign prefix installed the sole native-ID cursor")
                    .finish_campaign_prefix_after_resize(
                        map_data.header.width,
                        map_data.header.height,
                    )?;
                Ok((simulation, None))
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn logical_states_for_test(
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
    pub fn into_simulation(self, descriptor: &ScenarioDescriptor) -> Simulation {
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
    native_ids: &mut Option<crate::sim::native_identity::NativeUniqueIdCursor>,
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
        // Building43BA15 assigns after base Techno6F3254, before placement.
        // Discarded construction attempts spend both effects, like live rows.
        let native_unique_id =
            crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(native_ids);
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
                native_unique_id,
            });
        }
    }
    crate::sim::world::GeneratedTechnoInitTable::try_new(emitted)
}

impl Simulation {
    /// Borrow the two load-time streams from the already-staged gameplay owner.
    /// No second bootstrap owner can exist after `into_simulation` consumes it.
    pub fn terrain_load_draws(&mut self) -> (ScenarioFillRng<'_>, crate::sim::rng::MainRngDraws) {
        (
            ScenarioFillRng {
                rng: &mut self.scenario_rng,
            },
            self.main_rng.draws(),
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
    pub fn replay_staged_generated_construction_trace(
        &mut self,
        trace: &crate::map::construction_trace::RmgConstructionTrace,
    ) -> Result<
        crate::sim::world::GeneratedTechnoInitTable,
        crate::sim::world::GeneratedTechnoInitError,
    > {
        replay_generated_construction_trace_with_rng(
            &mut self.scenario_rng,
            &mut self.native_unique_ids,
            trace,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn gather_native_start_positions(
        &mut self,
        waypoints: &HashMap<u32, Waypoint>,
        participant_count: usize,
        terrain: &ResolvedTerrainGrid,
        bounds: NativeStartBounds,
    ) -> Vec<Waypoint> {
        let map_size_height = self.playfield_size_height();
        let binary_frame = self.session.binary_frame();
        native_gather_start_positions(
            waypoints,
            participant_count,
            terrain,
            &self.substrate.occupancy,
            bounds,
            self.playfield_bounds(),
            map_size_height,
            binary_frame,
            &mut self.scenario_rng,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn assign_native_battle_starts(
        &mut self,
        session: &SkirmishLaunchSession,
        starts: &[Waypoint],
    ) -> NativeStartAssignment {
        native_assign_launch_starts(session, starts, &mut self.scenario_rng)
    }

    #[cfg(any(test, feature = "test-support"))]
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

/// Campaign68ACAA chooses Basic.Player with a 20-byte ReadString buffer,
/// byte-exact House name lookup50C170, and index0 fallback on a miss.
/// Source: original68ACAA..68AD18 and the consumed campaign_start_houses corpus.
/// Empty dev substrates retain unavailable identity; this is not a claim that
/// native can launch an empty House array.
pub fn initialize_campaign_current_house(
    session: &mut ScenarioSession,
    houses: &mut Houses,
    ini: &crate::rules::ini_parser::IniFile,
) {
    let requested = ini
        .section("Basic")
        .map(|section| section.read_string("Player", "", 20))
        .unwrap_or_default();
    // Interning folds lookup case and preserves the first spelling it saw.
    // The native strcmp reads the retained raw House name instead.
    let current_house = Simulation::scenario_house_named(houses, &requested)
        .or_else(|| houses.order().first().copied());
    session.install_current_house(current_house);
    if let Some(owner) = session.current_house() {
        // Original 68AD0C/68AD18 set the selected House's +1EC/+1ED.
        // This is scenario construction, never a snapshot/viewer rebind.
        let house = houses.get_mut(&owner).expect("registered current House");
        house.make_campaign_player();
    }
}

/// Full_Init687874..6878C5/68792A reads Basic.Player independently of the
/// current-House reader68ACAA: capacity32, default literal Americans, miss0.
/// House.Find50C170 and the retained Country reader remain their sole owners.
fn initialize_campaign_scenario_side(
    sim: &mut Simulation,
    ini: &crate::rules::ini_parser::IniFile,
    rules: &RuleSet,
) {
    let requested = ini.section("Basic").map_or_else(
        || "Americans".to_owned(),
        |section| section.read_string("Player", "Americans", 32),
    );
    let side = Simulation::scenario_house_named(&sim.houses, &requested)
        .and_then(|owner| sim.houses.get(&owner))
        .and_then(|house| rules.country_side_index_raw(sim.interner.resolve(house.house_type_id())))
        .unwrap_or(0);
    sim.session.install_scenario_side(side);
}

/// Map-roster house construction shared by app and headless (F09):
/// native order requires houses before every object section.
pub fn initialize_map_roster_houses(
    sim: &mut Simulation,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    map_ini: Option<&crate::rules::ini_parser::IniFile>,
) {
    assert!(
        sim.houses.is_empty()
            && sim.houses.order().is_empty()
            && sim.entities().is_empty()
            && sim.terrain_objects.objects().is_empty(),
        "scenario houses must be initialized before map objects"
    );
    let campaign_difficulties = sim.session.campaign_house_difficulties();
    if campaign_difficulties.is_some() {
        assert!(
            map_ini.is_some(),
            "campaign Houses require admitted map/rules"
        );
        assert!(
            sim.super_weapons.is_empty(),
            "campaign House Supers are constructed once"
        );
    }
    let frame = sim.session.binary_frame() as i32;
    let mission_counter = sim.session.campaign_mission_counter().unwrap_or(1);
    for house in &house_roster.houses {
        let country_name = rules
            .scenario_country_name(house.country.as_deref())
            .or(house.country.as_deref());
        let fallback_side = crate::sim::house_state::side_index_from_name(house.side.as_deref());
        let side_idx = crate::sim::house_state::resolve_house_side_index(
            rules,
            country_name,
            house.side.as_deref(),
            fallback_side,
        );
        let name_id = sim.interner.intern(&house.name);
        let country_id = country_name.map(|c| sim.interner.intern(c));
        let mut house_state = crate::sim::house_state::HouseState::new(
            name_id,
            side_idx,
            country_id,
            false,
            sim.session.game_options().starting_credits,
            sim.session.game_options().tech_level,
        );
        let campaign_house_id = campaign_difficulties.map(|_| {
            sim.next_native_load_id()
                .expect("admitted campaign native-ID cursor")
        });
        // Every native House constructs its whole Supers vector before map
        // objects. Only the admitted campaign prefix has a measured ID cursor.
        crate::sim::superweapon::construct_house_supers(
            sim,
            rules,
            name_id,
            campaign_house_id.is_some(),
        );
        if let Some((_, computer_difficulty)) = campaign_difficulties {
            let draw = draw_house_constructor_timer(&mut sim.scenario_rng);
            house_state.bind_native_construction(
                campaign_house_id.expect("admitted campaign House ID"),
                computer_difficulty,
                frame,
                draw,
                rules.general.attack_delay,
            );
        }
        // HouseReadINI500A62..500A7B overwrites the constructor's country ID
        // with the bounded roster entry before any object reader runs.
        house_state.initialize_scenario_name(&house.name);
        // Read_Scenario_INI 500C5D..500D05: a non-empty `UIName=` names the
        // House through the string table, else the type's UIName stays.
        // Residual: native copies with wcsncpy(.., 0x20), whose zero padding
        // runs past the 20-character field through +0x16069 (the colour
        // index +0x16054 that the later `Color=` read defaults from, and the
        // constructor's +0x16058..+0x16064) before terminating +0x16052.
        // VERA keeps the visible 20 characters and none of that overrun;
        // whether retail campaign maps set House `UIName=` is unchecked.
        if let Some(key) = &house.ui_name {
            house_state.set_ui_name(crate::sim::house_state::HouseUiName::Csf(key.clone()));
        }
        house_state.resolve_multiplay_passive(rules, country_name);
        house_state.project_country_mults(rules, &sim.interner);
        // Registration admits distinct House names only.
        sim.houses.register(house_state);
    }

    // Original5009B0 constructs the whole House/Super array before its
    // second loop calls ReadScenarioINI500B40 and SetDifficulty4F6EC0.
    // Cross-House map reads must therefore see every constructor at once.
    for (index, house) in house_roster.houses.iter().enumerate() {
        let name_id = sim.houses.order()[index];
        let house_state = sim
            .houses
            .get_mut(&name_id)
            .expect("constructed scenario House");
        let country_name = house_state
            .country()
            .map(|country| sim.interner.resolve(country));
        #[cfg(feature = "ares")]
        house_state.read_scenario_factory_plan(country_name.and_then(|country| {
            crate::sim::factory_owners::parent_or_self_country(rules, country)
        }));
        house_state.assign_player_control(house.player_control == Some(true));
        if let Some(ini) = map_ini {
            house_state.initialize_scenario_parameters(
                house.read_scenario_parameters(ini, mission_counter),
                campaign_difficulties.map(|(player, _)| player),
                Some(&rules.general),
            );
            house_state
                .team_creation_mut()
                .read_scenario_ratio(ini.section_or_empty(&house.name));
            let temporary_delay = house_state
                .difficulty_value(&rules.general.team_delays)
                .wrapping_add((index as i32).wrapping_mul(175));
            house_state
                .team_creation_mut()
                .restart(frame, temporary_delay);
        }
        house_state.base_plan_mut().read_scenario(
            house.base_plan.percent_built,
            house
                .base_plan
                .nodes
                .iter()
                .map(|node| crate::sim::base_plan::BasePlanNode {
                    type_or_control: node.type_or_control,
                    packed_cell: node.packed_cell,
                    filled: node.filled,
                    retry_count: node.retry_count,
                }),
        );
        // HouseClass::Read_Scenario_INI reads `IQ=` from this exact named
        // house section, defaults it to zero, and changes a value above
        // MaxIQLevels to literal one before storing it as both the authored
        // IQ (+0x1D0) and CurrentIQ (+0x24C) (0x00500DBA..0x00500DC0).
        house_state.install_scenario_iq(house.scenario_current_iq(rules.general.max_iq_levels));
        if let Some((player, computer)) = campaign_difficulties {
            let difficulty = if house_state.is_controlled_by_human(false) {
                player
            } else {
                computer
            };
            house_state.set_difficulty(
                difficulty,
                &rules.general,
                country_name.map_or_else(Default::default, |name| {
                    rules.country_difficulty_biases(name)
                }),
                false,
                index as i32,
                frame,
            );
        }
    }
    if campaign_difficulties.is_some() {
        sim.install_house_alliances(
            crate::sim::house_diplomacy::HouseAllianceAdmission::ScenarioInitialization {
                roster: house_roster,
            },
            rules,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each stock Country reaches its section, so its ParentCountry defaults to
    // its own ID as in retail; a sectionless Country keeps ctor-empty
    // ParentCountry and owns nothing.
    const CONTROLLED_STOCK_COUNTRIES: &str = "[Countries]\n0=Americans\n1=Alliance\n2=French\n3=Germans\n4=British\n5=Africans\n6=Arabs\n7=Confederation\n8=Russians\n9=YuriCountry\n[Sides]\nAllied=Americans,Alliance,French,Germans,British\nSoviet=Africans,Arabs,Confederation,Russians\nThirdSide=YuriCountry\n[Americans]\nParentCountry=Americans\n[Alliance]\nParentCountry=Alliance\n[French]\nParentCountry=French\n[Germans]\nParentCountry=Germans\n[British]\nParentCountry=British\n[Africans]\nParentCountry=Africans\n[Arabs]\nParentCountry=Arabs\n[Confederation]\nParentCountry=Confederation\n[Russians]\nParentCountry=Russians\n[YuriCountry]\nParentCountry=YuriCountry\n";

    /// Layer a test's source over the Countries fixture, so its own Country
    /// keys join the fixture's sections; a fresh load would keep only the
    /// first of two same-named sections.
    fn launch_fixture_ini(source: &str) -> IniFile {
        let mut ini = IniFile::from_str(CONTROLLED_STOCK_COUNTRIES);
        ini.merge(&IniFile::from_str(source));
        ini
    }

    fn starting_country_native_fixture(passive: bool) -> (serde_json::Value, RuleSet) {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/session_country_starting_ownership.json",
        ))
        .unwrap();
        let mut source = String::from("[General]\nBaseUnit=V_BASE\n");
        for (section, prefix) in [("VehicleTypes", "V_"), ("InfantryTypes", "I_")] {
            source.push_str(&format!("[{section}]\n"));
            for (index, row) in corpus["type_inputs"].as_array().unwrap().iter().enumerate() {
                let id = row["id"].as_str().unwrap();
                if id.starts_with(prefix) {
                    source.push_str(&format!("{index}={id}\n"));
                }
            }
        }
        source.push_str(corpus["root"].as_str().unwrap());
        let mut ini = IniFile::from_str(&source);
        if passive {
            ini.merge(&IniFile::from_str(
                corpus["passive_layer"].as_str().unwrap(),
            ));
        }
        let rules = RuleSet::from_ini(&ini).unwrap();
        (corpus, rules)
    }

    #[test]
    fn starting_country_candidate_lists_match_original_owner_only_admission() {
        let (corpus, rules) = starting_country_native_fixture(false);
        for row in corpus["type_inputs"].as_array().unwrap() {
            let object = rules.object(row["id"].as_str().unwrap()).unwrap();
            assert_eq!(
                rules.house_type_mask(&object.owner),
                row["owner_mask"].as_u64().unwrap() as u32,
                "{} Owner mask",
                row["id"],
            );
        }
        for country in corpus["country_inputs"].as_array().unwrap() {
            let id = country["id"].as_str().unwrap();
            let candidates = starting_unit_candidates_for_country(
                &rules,
                LaunchCountry::from_country_index(country["actual_index"].as_u64().unwrap() as u32),
                corpus["tech_level"].as_i64().unwrap() as i32,
            );
            for (category, actual) in [
                ("vehicles", candidates.vehicles),
                ("infantry", candidates.infantry),
            ] {
                let ids: Vec<_> = actual
                    .iter()
                    .map(|candidate| candidate.type_id.as_str())
                    .collect();
                let expected: Vec<_> = corpus["candidate_lists"][id][category]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect();
                assert_eq!(ids, expected, "{id}/{category}");
            }
        }
        for row in corpus["candidate_house_tech_controls"].as_array().unwrap() {
            let index = corpus["country_inputs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|country| country["id"] == row["country"])
                .unwrap()["actual_index"]
                .as_u64()
                .unwrap();
            let candidates = starting_unit_candidates_for_country(
                &rules,
                LaunchCountry::from_country_index(index as u32),
                row["house_tech_level"].as_i64().unwrap() as i32,
            );
            for (category, actual) in [
                ("vehicles", candidates.vehicles),
                ("infantry", candidates.infantry),
            ] {
                let expected: Vec<_> = row[category]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect();
                assert_eq!(
                    actual
                        .iter()
                        .map(|candidate| candidate.type_id.as_str())
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
    }

    #[test]
    fn starting_country_budgets_match_original_actual_nonpassive_mask() {
        let (corpus, active_rules) = starting_country_native_fixture(false);
        let (_, passive_rules) = starting_country_native_fixture(true);
        let template =
            normalized_launch_slots(one_player_battle_launch("budget.map").session()).remove(0);
        for row in corpus["budget_rows"].as_array().unwrap() {
            let rules = if row["alliance_passive"].as_bool().unwrap() {
                &passive_rules
            } else {
                &active_rules
            };
            let slots: Vec<_> = row["participants"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| {
                    let index = corpus["country_inputs"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|country| country["id"] == *name)
                        .unwrap()["actual_index"]
                        .as_u64()
                        .unwrap();
                    let mut slot = template.clone();
                    slot.country = LaunchCountry::from_country_index(index as u32);
                    slot
                })
                .collect();
            assert_eq!(
                starting_unit_budget(
                    rules,
                    slots.iter().map(|slot| slot.country.index()),
                    corpus["tech_level"].as_i64().unwrap() as i32,
                    corpus["unit_count"].as_i64().unwrap() as i32
                ),
                row["budget"].as_i64().unwrap() as i32,
                "{}",
                row["name"],
            );
        }
    }

    #[test]
    fn starting_country_native_qualifier_sentinels_reach_shared_consumers() {
        let corpus =
            crate::test_fixture::json("tools/rules_oracle/session_country_starting_ownership.json");
        let gate = &corpus["qualifier_gates"];
        let mut source = String::from("[Countries]\n");
        for (index, id) in gate["sentinel_registry_ids"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            source.push_str(&format!("{index}={}\n", id.as_str().unwrap()));
        }
        // The native House's Country reads its retained ParentCountry, which
        // supplies the Owner-side bit; its actual index supplies the rest.
        let actual_country = gate["actual_country"].as_str().unwrap();
        source.push_str(&format!(
            "[{actual_country}]\nParentCountry={}\n",
            gate["owner_parent"].as_str().unwrap()
        ));
        let type_id = gate["type_id"].as_str().unwrap();
        source.push_str(&format!(
            "[VehicleTypes]\n0={type_id}\n[{type_id}]\nTechLevel=1\n"
        ));
        for row in gate["rows"].as_array().unwrap() {
            let mut ini = IniFile::from_str(&source);
            ini.merge(&IniFile::from_str(gate["owner_layer"].as_str().unwrap()));
            ini.merge(&IniFile::from_str(row["physical_layer"].as_str().unwrap()));
            let rules = RuleSet::from_ini(&ini).unwrap();
            let object = rules.object(type_id).unwrap();
            assert_eq!(
                rules.house_type_mask(&object.owner),
                gate["owner_mask"].as_u64().unwrap() as u32
            );
            assert_eq!(
                crate::sim::ai_buildable::house_country_bit(&rules, actual_country),
                1 << gate["actual_index"].as_u64().unwrap()
            );
            assert_eq!(
                crate::sim::ai_buildable::candidate_allowed(
                    object,
                    crate::sim::ai_buildable::CountryBuildMasks::for_country(
                        &rules,
                        actual_country
                    ),
                    gate["planned_side"].as_i64().unwrap() as i32,
                    true,
                    &rules,
                ),
                row["first_buildable_result"].as_str() == Some(type_id),
                "{} FirstBuildable",
                row["name"],
            );
            // Only the original qualifier region is a native CanBuild
            // golden. Other production gates are open in this Rust scene.
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("QualifierHouse");
            let country = sim.interner.intern(actual_country);
            sim.houses
                .register(HouseState::new(owner, -1, Some(country), false, 0, 10));
            assert_eq!(
                crate::sim::production::can_build(&sim, &rules, owner, object, false, false)
                    == crate::sim::production::CanBuild::Yes,
                row["can_build_qualifier_gate"].as_bool().unwrap(),
                "{} CanBuild qualifier integration",
                row["name"],
            );
        }
    }

    #[test]
    fn starting_country_passive_blocks_both_mcv_and_extra_unit_callbacks() {
        // Original5D6EF8 skips both callbacks. Exercise the shared launch
        // consumer; this is a Rust integration check, not placement parity.
        for (passive, house_tech_level, changed_house_country) in [
            (false, 10, false),
            (false, 0, false),
            (true, 10, false),
            (false, 10, true),
        ] {
            let rules = RuleSet::from_ini(&launch_fixture_ini(&format!(
                "[General]\nBaseUnit=MTNK\n[Americans]\nMultiplayPassive={}\n[Alliance]\nMultiplayPassive=yes\n\
                 [VehicleTypes]\n0=MTNK\n1=HTNK\n\
                 [MTNK]\nStrength=300\nSpeed=6\nCost=100\nTechLevel=1\nOwner=Americans\nAllowedToStartInMultiplayer=yes\n\
                 [HTNK]\nStrength=300\nSpeed=6\nCost=100\nTechLevel=1\nOwner=Americans\nAllowedToStartInMultiplayer=yes\n",
                if passive { "yes" } else { "no" },
            ))).unwrap();
            let mut session = one_player_battle_launch("passive.map").session().clone();
            session.options.bases = true;
            session.options.unit_count = 1;
            session.options.tech_level = 10;
            let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
            let map = one_start_prefix_map(Waypoint {
                index: 0,
                rx: 44,
                ry: 48,
            });
            let terrain = techno_constructor_flat_start_terrain(96);
            let mut sim = techno_constructor_start_sim(0xC701_1005, 96);
            sim.install_resolved_terrain_for_new_map(terrain.clone());
            initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
            let owner = sim.session.current_house().unwrap();
            if changed_house_country {
                let country = sim.interner.intern("Alliance");
                sim.houses
                    .get_mut(&owner)
                    .unwrap()
                    .set_country_for_test(Some(country));
            }
            sim.houses
                .get_mut(&owner)
                .unwrap()
                .set_tech_level_for_test(house_tech_level);
            let result = apply_explicit_skirmish_launch_session(
                &mut sim,
                &map,
                &HouseRoster::default(),
                &rules,
                &terrain,
                &launch,
            );
            let skips_forces = passive || changed_house_country;
            assert_eq!(result.spawned_mcvs, u32::from(!skips_forces));
            assert_eq!(
                sim.entities().len(),
                if skips_forces {
                    0
                } else {
                    1 + usize::from(house_tech_level > 0)
                },
            );
        }
    }

    #[test]
    fn registered_nonstock_country_reaches_house_without_stock_remapping() {
        let mut ini = String::from(CONTROLLED_STOCK_COUNTRIES);
        // A second physical Countries section is not a registry extension;
        // append the new ID inside the original master list instead.
        ini = ini.replace("[Sides]", "10=Allocated\n[Sides]");
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let mut session = one_player_battle_launch("allocated.map").session().clone();
        session.local_slot_mut().country = LaunchCountry::from_country_index(10);
        let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let mut sim = Simulation::new();
        initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
        let local = sim.session.current_house().unwrap();
        let house = &sim.houses[&local];
        assert_eq!(sim.interner.resolve(house.country().unwrap()), "Allocated");
        assert_eq!(
            house.side_index(),
            -1,
            "Country ctor's absent Side remains native-1"
        );
    }

    #[test]
    fn multiple_humans_including_observer_apply_layered_comp_easy_bonus_before_house_biases() {
        let mut session = one_player_battle_launch("bonus.map").session().clone();
        let mut observer = session.humans[0].clone();
        observer.name = "Observer".into();
        observer.raw_slot_index = Some(1);
        observer.set_observer(true);
        observer.slot.color_index = 8;
        session.humans.push(observer);
        session
            .opponents
            .push(crate::skirmish_launch::SkirmishAiSlot {
                raw_slot_index: 2,
                country: LaunchCountry::Russia,
                country_random: false,
                color_index: 1,
                color_random: false,
                start_position: LaunchStartPosition::Auto,
                team: LaunchTeam::None,
                difficulty: crate::skirmish_launch::AiDifficulty::Normal,
            });
        session.rebuild_house_roster();
        let descriptor = MatchLaunchDescriptor::from_resolved(session).unwrap();
        for (setting, expected) in [
            ("", HouseDifficulty::Hard),
            ("[AI]\nCompEasyBonus=no\n", HouseDifficulty::Normal),
        ] {
            let rules = RuleSet::from_ini(&launch_fixture_ini(setting)).unwrap();
            let mut sim = Simulation::new();
            initialize_skirmish_launch_houses(
                &mut sim,
                &HouseRoster::default(),
                &rules,
                &descriptor,
            );
            let ai = sim.interner.get("Computer1").unwrap();
            assert_eq!(sim.houses[&ai].difficulty(), expected);
        }
    }

    #[test]
    fn observer_start_views_match_executed_native_chooser_and_rng() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        for case in corpus["observer_starts"].as_array().unwrap() {
            let mut session = one_player_battle_launch("observer.map").session().clone();
            let template = session.humans[0].clone();
            session.humans = case["observers"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, observer)| {
                    let mut human = template.clone();
                    human.name = format!("Human{index}");
                    human.raw_slot_index = Some(index as u8);
                    human.set_observer(observer.as_bool().unwrap());
                    human.slot.color_index = if human.is_observer() { 8 } else { index as u8 };
                    human.slot.start_position = case["initial_table"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .position(|owner| owner.as_i64() == Some(index as i64))
                        .map_or(LaunchStartPosition::Auto, |start| {
                            LaunchStartPosition::Position(start as u8)
                        });
                    human
                })
                .collect();
            session.rebuild_house_roster();
            let starts: Vec<_> = case["starts"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, cell)| Waypoint {
                    index: index as u32,
                    rx: cell[0].as_u64().unwrap() as u16,
                    ry: cell[1].as_u64().unwrap() as u16,
                })
                .collect();
            let mut rng =
                SimRng::from_native_state_hex_for_test(case["initial_rng_hex"].as_str().unwrap());
            let result = native_assign_launch_starts(&session, &starts, &mut rng);
            assert_eq!(
                result
                    .start_table
                    .iter()
                    .map(|owner| owner.map_or(-1, |owner| owner as i64))
                    .collect::<Vec<_>>(),
                case["table"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_i64().unwrap())
                    .collect::<Vec<_>>()
            );
            for (index, position) in &result.placements {
                assert_eq!(
                    [u64::from(position.rx), u64::from(position.ry)],
                    [
                        case["placements"][*index][0].as_u64().unwrap(),
                        case["placements"][*index][1].as_u64().unwrap()
                    ]
                );
            }
            assert_eq!(
                rng.native_state_hex(),
                case["rng_hex"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn ordered_humans_keep_local_identity_after_color_sort_and_observer_viewpoint() {
        let mut session = one_player_battle_launch("multiplayer.map")
            .session()
            .clone();
        session.humans[0].name = "Host".into();
        session.humans[0].slot.color_index = 6;
        session.humans[0].slot.start_position = LaunchStartPosition::Position(0);
        let mut local = session.humans[0].clone();
        local.name = "Guest".into();
        local.raw_slot_index = Some(1);
        local.slot.color_index = 1;
        local.slot.start_position = LaunchStartPosition::Position(1);
        session.humans.push(local);
        let mut observer = session.humans[0].clone();
        observer.name = "Observer".into();
        observer.raw_slot_index = Some(2);
        observer.set_observer(true);
        observer.slot.color_index = 8;
        observer.slot.start_position = LaunchStartPosition::Auto;
        session.humans.push(observer);
        session.local_player_index = 1;
        session.rebuild_house_roster();
        assert!(session.house_roster_is_consistent());
        assert_eq!(session.required_start_count(), 2);
        assert_eq!(session.pre_fill_house_roster.created_house_count(), 5);
        let descriptor = MatchLaunchDescriptor::from_resolved(session.clone()).unwrap();
        let rules = techno_constructor_start_rules();
        let mut sim = Simulation::new();
        initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &descriptor);
        let guest = sim.interner.get("Guest").unwrap();
        let host = sim.interner.get("Host").unwrap();
        assert_eq!(sim.session.current_house(), Some(guest));
        assert_eq!(sim.houses.order()[0], guest);
        assert!(sim.houses[&guest].is_human() && sim.houses[&guest].player_control());
        assert!(sim.houses[&host].is_human() && !sim.houses[&host].player_control());
        let starts = [
            Waypoint {
                index: 0,
                rx: 20,
                ry: 20,
            },
            Waypoint {
                index: 1,
                rx: 40,
                ry: 40,
            },
        ];
        let map = prefix_map_with_starts(&starts);
        let full_prefix =
            prepare_stock_offline_scenario_prefix_plan(&descriptor, &map, &map.waypoints, 73)
                .expect("ordered multi-human launch enters the real pre-Fill prefix");
        assert_eq!(full_prefix.first_house_timers.len(), 5);
        assert_eq!(full_prefix.second_house_timers.len(), 5);
        assert_eq!(full_prefix.projection.assignment.placements.len(), 3);
        let mut rng = SimRng::new(73);
        let before = rng.logical_state();
        let assigned = native_assign_launch_starts(&session, &starts, &mut rng);
        assert_eq!(assigned.start_table, vec![Some(1), Some(0)]);
        assert_eq!(
            assigned.placements,
            vec![(0, starts[1]), (1, starts[0]), (2, starts[0])]
        );
        assert_eq!(
            rng.logical_state(),
            before,
            "observer shares an occupied view without a draw"
        );
        let mut unresolved_remote = session;
        unresolved_remote.humans[0].slot.country_random = true;
        assert!(MatchLaunchDescriptor::from_resolved(unresolved_remote).is_err());
    }

    #[test]
    fn campaign_current_house_uses_exact_case_and_registry_fallback() {
        let mut sim = Simulation::new();
        // Earlier interning must not become the original House name spelling.
        sim.interner.intern("alpha");
        let map = IniFile::from_str(
            "[Houses]\n7=Zulu\n2=Alpha\n9=1234567890123456789\n[Zulu]\n[Alpha]\n[1234567890123456789]\n",
        );
        let roster = crate::map::houses::parse_house_roster(&map, &[], None);
        initialize_map_roster_houses(
            &mut sim,
            &roster,
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
        );
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
                &mut sim.session,
                &mut sim.houses,
                &IniFile::from_str(&format!("[Basic]\n{input}\n")),
            );
            assert_eq!(sim.session.current_house(), Some(expected));
            assert!(sim.houses[&expected].is_human());
            assert!(sim.houses[&expected].player_control());
        }
        assert!(
            sim.houses[&first].is_human() && sim.houses[&first].player_control(),
            "selecting another House does not clear previously set flags"
        );
    }

    #[test]
    fn scenario_side_campaign_uses_its_own_basic_player_reader() {
        let rules = techno_constructor_start_rules();
        // ReadHousesList first reads each roster name with capacity20, so
        // authored names retain19 bytes before House's own20-byte copy.
        // A suffix is cut by the current-House reader but retained by this
        // independent32-byte Player reader, whose exact lookup must miss.
        let long_name = "1234567890123456789";
        let map = IniFile::from_str(&format!(
            "[Houses]\n0=First\n1=Americans\n2={long_name}\n[First]\nCountry=YuriCountry\n[Americans]\nCountry=Russians\n[{long_name}]\nCountry=YuriCountry\n"
        ));
        let roster =
            crate::map::houses::parse_house_roster(&map, &rules.color_schemes, Some(&rules));
        let mut sim = Simulation::new();
        initialize_map_roster_houses(&mut sim, &roster, &rules, None);
        let rng = sim.rng_state();
        for (basic, expected) in [
            (String::new(), 1),
            ("[Basic]\n".to_owned(), 1),
            // Physical INI loading omits empty values, so this also uses
            // the literal Americans default rather than a stored-empty key.
            ("[Basic]\nPlayer=\n".to_owned(), 1),
            ("[Basic]\nPlayer=First\n".to_owned(), 2),
            ("[Basic]\nPlayer=americans\n".to_owned(), 0),
            (format!("[Basic]\nPlayer={long_name}\n"), 2),
            (format!("[Basic]\nPlayer={long_name}suffix\n"), 0),
        ] {
            let ini = IniFile::from_str(&basic);
            initialize_campaign_current_house(&mut sim.session, &mut sim.houses, &ini);
            let current = sim.session.current_house();
            if basic.ends_with("suffix\n") {
                assert_eq!(current, sim.interner.get(long_name));
            }
            initialize_campaign_scenario_side(&mut sim, &ini, &rules);
            assert_eq!(sim.session.scenario_side(), expected, "{basic}");
            assert_eq!(sim.session.current_house(), current);
            assert_eq!(sim.rng_state(), rng);
        }
    }

    #[test]
    fn scenario_side_skirmish_uses_local_source_identity_before_house_color_order() {
        let mut session = one_player_battle_launch("side.map").session().clone();
        let mut second = session.humans[0].clone();
        second.name = "LocalSecond".into();
        second.raw_slot_index = Some(1);
        second.slot.country = LaunchCountry::Russia;
        second.slot.color_index = 1;
        session.humans[0].slot.color_index = 0;
        session.humans.push(second);
        session.local_player_index = 1;
        session.rebuild_house_roster();
        let descriptor = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let rules = techno_constructor_start_rules();
        let mut sim = Simulation::new();
        initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &descriptor);
        assert_eq!(sim.houses[&sim.houses.order()[0]].side_index(), 0);
        assert_eq!(sim.session.scenario_side(), 1);
        let hash = sim.state_hash();
        let rng = sim.rng_state();
        sim.session
            .install_current_house(Some(sim.houses.order()[0]));
        assert_eq!(sim.session.scenario_side(), 1);
        assert_eq!(sim.state_hash(), hash);
        assert_eq!(sim.rng_state(), rng);
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
    use crate::map::resolved_terrain::ResolvedTerrainCell;
    use crate::rules::ini_parser::IniFile;

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
                class: crate::skirmish_modes::MpModeClass::Battle,
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
            humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: "Player".to_string(),
                slot: SkirmishLocalSlot {
                    country: LaunchCountry::America,
                    country_random: false,
                    color_index: 0,
                    color_random: false,
                    start_position: LaunchStartPosition::Auto,
                    team: LaunchTeam::None,
                },
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
            }],
            local_player_index: 0,
            ai_player_count: 0,
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
            ranking: Default::default(),
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
            tags: Default::default(),
            triggers: Default::default(),
            events: Default::default(),
            actions: Default::default(),
            local_variables: Default::default(),
            special_flags: Default::default(),
            ini: IniFile::from_str(""),
        }
    }

    fn one_start_prefix_map(start: Waypoint) -> MapFile {
        prefix_map_with_starts(&[start])
    }

    #[test]
    fn shell_allocated_country_is_readmitted_against_post_reset_process_rules() {
        use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
        let root = "[Countries]\n0=Alpha\n[Alpha]\nName=Alpha\n[Orphan]\nName=Orphan\n";
        for replacement in [None, Some("Replacement")] {
            let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
                IniFile::from_str(root),
                None,
                IniFile::empty(),
                std::sync::Arc::default(),
                Default::default(),
            )
            .unwrap();
            let index = owner
                .read_multiplayer_country(&IniFile::from_str("[MultiPlayer]\nSide=Orphan\n"), -1);
            assert_eq!(index, 1);
            owner.refresh_multiplayer_country_bodies();
            let mut shell_rules = RuleSet::from_ini(&IniFile::from_str(root)).unwrap();
            owner.project_multiplayer_countries(&mut shell_rules);
            let mut launch = one_player_battle_launch("country-reset.map");
            launch.session.local_slot_mut().country =
                LaunchCountry::from_country_index(index as u32);
            launch.validate_countries(&shell_rules).unwrap();
            let mut map = one_start_prefix_map(Waypoint {
                index: 0,
                rx: 40,
                ry: 40,
            });
            if let Some(name) = replacement {
                map.ini =
                    IniFile::from_str(&format!("[Countries]\n1={name}\n[{name}]\nName={name}\n"));
            }
            let plan =
                prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, 1234)
                    .unwrap();
            let (rules, _, _, receipt) = owner
                .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &map.ini)
                .unwrap()
                .into_parts();
            assert!(rules.country_index("Orphan").is_none());
            let admitted = plan.bind_native_rules_receipt(receipt, &launch, &rules);
            if replacement.is_none() {
                assert!(matches!(
                    admitted,
                    Err(PreFillScenarioPrefixPlanError::MissingLaunchCountry { index: 1 })
                ));
                continue;
            }
            let (mut sim, _) = ScenarioBootstrapRng::new(1234)
                .into_stock_offline_staged_simulation(
                    &crate::sim::scenario_session::ScenarioDescriptor {
                        seed: 1234,
                        ..Default::default()
                    },
                    admitted.unwrap(),
                )
                .unwrap();
            initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
            let house = sim
                .houses
                .get(&sim.session.current_house().unwrap())
                .unwrap();
            assert_eq!(
                house.country().map(|id| sim.interner.resolve(id)),
                Some("Replacement")
            );
        }
    }

    #[test]
    fn current_house_precedes_map_objects_and_survives_failed_starting_mcv() {
        // Exercise the same staged population and pre-Fill launch projection
        // used by the offline app. Blocking every candidate after map-object
        // creation supplies a deterministic total placement failure; it does
        // not replace the production MCV constructor/search/result branch.
        let rules_ini = launch_fixture_ini(
            "[General]\nBaseUnit=MTNK\n[VehicleTypes]\n0=MTNK\n\
             [BuildingTypes]\n0=CABHUT\n\
             [MTNK]\nStrength=300\nSpeed=6\nCost=100\nOwner=Americans,Russians\n\
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
            launch.session.local_player_mut().name = handle.to_string();
            launch.session.options.bases = true;
            launch.session.options.unit_count = 0;
            if with_ai {
                launch
                    .session
                    .opponents
                    .push(crate::skirmish_launch::SkirmishAiSlot {
                        raw_slot_index: 1,
                        country: LaunchCountry::Russia,
                        country_random: false,
                        color_index: 1,
                        color_random: false,
                        start_position: LaunchStartPosition::Auto,
                        team: LaunchTeam::None,
                        difficulty: crate::skirmish_launch::AiDifficulty::Easy,
                    });
                launch.session.ai_player_count = 1;
                launch.session.rebuild_house_roster();
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
                        std::sync::Arc::default(),
                        Default::default(),
                    )
                    .expect("cold process Rules authority");
                let (rules, _, _, rules_receipt) = rules_owner
                    .load_scenario(
                        crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                        &map.ini,
                    )
                    .expect("production noncampaign Rules reset/rebuild")
                    .into_parts();
                let bound_prefix = plan
                    .bind_native_rules_receipt(rules_receipt, &launch, &rules)
                    .unwrap();
                let (mut sim, projection) = ScenarioBootstrapRng::new(scenario.seed)
                    .into_stock_offline_staged_simulation(&scenario, bound_prefix)
                    .expect("production admission installs the paired RNG/native-ID prefix");
                assert!(sim.native_unique_ids.is_some());
                let native_before_map = sim.native_unique_ids.as_ref().unwrap().current_raw();
                let mut terrain = techno_constructor_flat_start_terrain(96);
                let overlay_grid = crate::sim::overlay_grid::OverlayGrid::new(96, 96);
                crate::sim::runtime::populate_staged_scenario_with_generated_inits(
                    &mut sim,
                    &map,
                    &terrain,
                    "TEMPERATE",
                    &rules,
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
                        assert_eq!(sim.session.current_house(), Some(player));
                        assert_eq!(sim.houses.order()[0], player);
                        assert!(
                            sim.houses[&player].is_human() && sim.houses[&player].player_control()
                        );
                    },
                )
                .expect("shared staged object construction");
                assert_eq!(sim.entities().len(), 1, "map object actually constructed");
                assert_eq!(
                    sim.entities().values().next().unwrap().native_unique_id(),
                    native_before_map.wrapping_add(1) as i32,
                    "production admission retains its concrete native constructor ID",
                );
                let player = sim.interner.get(owner_key).unwrap();
                assert_eq!(sim.session.current_house(), Some(player));
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
                assert_eq!(sim.session.current_house(), Some(player));
                assert!(sim.houses[&player].is_human() && sim.houses[&player].player_control());
                assert_eq!(launch.session.local_player().name, handle);
                let mut distinct = sim.houses.order().to_vec();
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
                    assert!(!sim.houses[&other].is_human() && !sim.houses[&other].player_control());
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
                        .filter(|entity| world.interner.resolve(entity.type_ref()) == "CABHUT")
                        .map(|entity| entity.owner())
                        .collect();
                    assert_eq!(
                        hut_owners,
                        [neutral],
                        "authored hut keeps its Neutral owner"
                    );
                    let mut mcv_owners: Vec<_> = world
                        .entities()
                        .values()
                        .filter(|entity| world.interner.resolve(entity.type_ref()) == "MTNK")
                        .map(|entity| entity.owner())
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
                assert_eq!(restored.session.current_house(), Some(player));
                assert_eq!(restored.houses.order(), sim.houses.order());
                assert!(
                    restored.houses[&player].is_human()
                        && restored.houses[&player].player_control()
                );
                assert_entity_owners(&restored);
            }
        }
    }

    fn techno_constructor_start_rules() -> RuleSet {
        RuleSet::from_ini(&launch_fixture_ini(
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
        crate::map::resolved_terrain::test_grid(size, size, |rx, ry| ResolvedTerrainCell {
            speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            ..crate::map::resolved_terrain::test_tiberium_cell(rx, ry)
        })
    }

    fn techno_constructor_start_sim(seed: u64, size: u16) -> Simulation {
        let mut sim = Simulation::with_seed(seed);
        sim.session.set_map_width_for_test(size);
        sim.session.set_map_height_for_test(size);
        sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
            base: i32::from(size),
            off_fc: 0,
            off_100: 0,
            off_104: i32::from(size),
            off_108: i32::from(size),
        }));
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
            &terrain,
            None,
        )
        .expect("exact PostMap placement");
        let exact_entity = exact.substrate.entities.get(exact_id).unwrap();
        assert_eq!(exact_entity.techno_ctor_random_word(), exact_word);
        assert_eq!(
            (exact_entity.position.rx(), exact_entity.position.ry()),
            (6, 5)
        );
        assert_eq!(
            exact.scenario_rng.logical_state(),
            exact_expected.logical_state()
        );

        let mut fallback = techno_constructor_start_sim(seed, 10);
        let mut fallback_expected = SimRng::new(seed);
        let blocker_word = (fallback_expected.next_u32() & 0xFFFF) as u16;
        let blocker = fallback
            .spawn_object("MTNK", "Americans", 6, 5, 0, &rules)
            .unwrap();
        assert_eq!(
            fallback
                .substrate
                .entities
                .get(blocker)
                .unwrap()
                .techno_ctor_random_word(),
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
            &terrain,
            None,
        )
        .expect("fallback PostMap placement");
        let fallback_entity = fallback.substrate.entities.get(fallback_id).unwrap();
        assert_eq!(fallback_entity.techno_ctor_random_word(), fallback_word);
        assert_ne!(
            (fallback_entity.position.rx(), fallback_entity.position.ry()),
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
            &terrain,
            None,
        )
        .expect("raw-bit rejection must retry the same constructed Unit");

        let placed = sim.substrate.entities.get(stable_id).unwrap();
        assert_eq!(placed.techno_ctor_random_word(), expected_word);
        assert_ne!((placed.position.rx(), placed.position.ry()), base);
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
        sim.session.game_options_mut_for_test().tech_level = 10;
        let owner = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, Some(owner), true, 0, 10);
        house.set_base_center_for_test(Some((6, 5)));
        sim.houses.register(house);
        let slots = [NormalizedSkirmishSlot {
            owner_name: "Americans".to_string(),
            country: LaunchCountry::America,
            color_index: 0,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
            is_human: true,
            observer: false,
            source_human_index: Some(0),
            handle: None,
            difficulty: HouseDifficulty::Normal,
        }];
        let mut expected = SimRng::new(seed);
        let blocker_word = (expected.next_u32() & 0xFFFF) as u16;
        let blocker = sim
            .spawn_object("MTNK", "Americans", 6, 5, 0, &rules)
            .expect("starting-cell blocker");
        assert_eq!(
            sim.substrate
                .entities
                .get(blocker)
                .unwrap()
                .techno_ctor_random_word(),
            blocker_word
        );
        let candidate_index = expected.next_range_u32_inclusive(0, 1) as usize;
        let expected_type = ["MTNK", "HTNK"][candidate_index];
        let expected_word = (expected.next_u32() & 0xFFFF) as u16;
        let _fallback_start_direction = expected.next_range_u32_inclusive(0, 7);

        assert_eq!(
            seed_starting_extra_units(&mut sim, &slots, &rules, &terrain, bounds, 1, false,),
            1
        );
        let entity = sim.substrate.entities.get(2).unwrap();
        assert_eq!(sim.interner.resolve(entity.type_ref()), expected_type);
        assert_eq!(entity.techno_ctor_random_word(), expected_word);
        assert_ne!((entity.position.rx(), entity.position.ry()), (6, 5));
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    }

    #[test]
    fn nearoref_native_start_bounds_use_inclusive_full_cell_array_endpoints() {
        let mut sim = Simulation::new();
        sim.session.set_map_width_for_test(138);
        sim.session.set_map_height_for_test(138);
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

    /// Retail starts sit outside `[Map] Size=`: Dustbowl's Size is 70x76 and
    /// its starts are at rx 70 and 79. Start bounds and the session's cell
    /// extent therefore come from the Resize cell array, which must hold every
    /// authored start without the fallback clamp moving it.
    #[test]
    fn retail_dustbowl_starts_need_the_cell_array_not_the_size_rect() {
        let Some((root, _assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let map = crate::map::map_file::load_from_path(&root.join("Dustbowl.mmx"))
            .expect("retail Dustbowl.mmx parses");
        let bounds = NativeStartBounds::from_map_header(&map.header).expect("valid Size=");
        let starts = crate::map::waypoints::multiplayer_start_waypoints(&map.waypoints);
        assert!(!starts.is_empty());
        for start in &starts {
            assert_eq!(
                bounds.clamp(i32::from(start.rx), i32::from(start.ry)),
                (start.rx, start.ry),
                "start {}",
                start.index
            );
        }
        assert!(
            starts
                .iter()
                .any(|start| u32::from(start.rx) >= map.header.width
                    || u32::from(start.ry) >= map.header.height),
            "some authored start lies outside the Size= rect"
        );
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
    fn generated_mapgen_continuation_replaces_only_mapgen_cursor() {
        let match_seed = 0x51C0_1005;
        let map_seed: u16 = 0xBEEF;
        let mut expected_mapgen = SimRng::new(u64::from(map_seed));
        for _ in 0..353 {
            let _ = expected_mapgen.next_u32();
        }
        let generated = expected_mapgen.logical_state();

        let mut owner = ScenarioBootstrapRng::new(match_seed);
        owner.install_process_mapgen_continuation(MapGenRngContinuation::from_native_parts(
            generated.disabled,
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
        assert_eq!(roster.nonobserver_house_count(), 3);
        assert_eq!(roster.created_house_count(), 6);

        let mut session = one_player_battle_launch("prefix-roster.mmx")
            .session()
            .clone();
        session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
        session.opponents = (0..2)
            .map(|index| SkirmishAiSlot {
                raw_slot_index: (index + 1) as u8,
                country: LaunchCountry::Russia,
                country_random: false,
                color_index: index + 1,
                color_random: false,
                start_position: LaunchStartPosition::Position(index + 1),
                team: LaunchTeam::None,
                difficulty: AiDifficulty::Easy,
            })
            .collect();
        session.ai_player_count = session.opponents.len() as i32;
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
                IniFile::from_str(CONTROLLED_STOCK_COUNTRIES),
                None,
                IniFile::from_bytes(b"").unwrap(),
                std::sync::Arc::default(),
                Default::default(),
            )
            .unwrap();
        let native_load = native_rules
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &map.ini,
            )
            .unwrap();
        let (rules, _, _, receipt) = native_load.into_parts();
        let early_count = receipt.pre_reset().event_count() as u32;
        let first_super_count = receipt.pre_reset().allocated_super_weapon_type_count() as u32;
        let rebuilt_count = receipt.post_reset().event_count() as u32;
        let final_super_count = receipt.post_reset().allocated_super_weapon_type_count() as u32;
        let bound_native = native_plan
            .bind_native_rules_receipt(receipt, &launch, &rules)
            .unwrap();
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
            veterancy: None,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
            structure_ai_sellable: false,
            structure_ai_repairable: false,
            structure_online: true,
            trigger_tag: None,
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
    fn map_houses_take_a_ui_name_key_or_keep_the_type_name() {
        use crate::sim::house_state::HouseUiName;
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[Houses]\n0=Allies\n1=Soviets\n\
             [Allies]\nUIName=Name:AlliedHouse\n[Soviets]\nUIName=\n",
        );
        let roster = crate::map::houses::parse_house_roster(&ini, &[], None);
        let mut sim = Simulation::new();
        initialize_map_roster_houses(
            &mut sim,
            &roster,
            crate::rules::ruleset::RuleSet::empty_for_test(),
            Some(&ini),
        );
        let name = |house: &str| {
            let id = sim.interner.get(house).unwrap();
            sim.houses[&id].ui_name().clone()
        };
        assert_eq!(name("Allies"), HouseUiName::Csf("Name:AlliedHouse".into()));
        assert_eq!(name("Soviets"), HouseUiName::TypeName);
    }

    #[test]
    fn launch_houses_take_each_humans_handle_and_computer_ui_names() {
        use crate::sim::house_state::HouseUiName;
        // CreateHouses 688092..6880A4: each human its own node's handle (at
        // most 20 wide chars), even a duplicate; 688252..688270: computers
        // TXT_COMPUTER. Owner keys stay distinct.
        let mut session = one_player_battle_launch("names.mmx").session().clone();
        session.humans[0].name = "Twentyone-characters".to_string() + "X";
        let mut peer = session.humans[0].clone();
        peer.raw_slot_index = Some(1);
        peer.slot.color_index = 1;
        peer.name = "Remote".to_string();
        let mut twin = peer.clone();
        twin.raw_slot_index = Some(2);
        twin.slot.color_index = 2;
        session.humans.push(peer);
        session.humans.push(twin);
        session.rebuild_house_roster();
        let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let mut sim = Simulation::new();
        initialize_skirmish_launch_houses(
            &mut sim,
            &HouseRoster::default(),
            &techno_constructor_start_rules(),
            &launch,
        );
        let names: Vec<_> = normalized_launch_slots(launch.session())
            .iter()
            .map(|slot| {
                let owner = sim.interner.get(&slot.owner_name).unwrap();
                (
                    slot.owner_name.clone(),
                    sim.houses[&owner].ui_name().clone(),
                )
            })
            .collect();
        assert_eq!(
            names[0].1,
            HouseUiName::Handle("Twentyone-characters".into())
        );
        assert_eq!(
            names[1],
            ("Remote".into(), HouseUiName::Handle("Remote".into()))
        );
        assert_eq!(names[2].1, HouseUiName::Handle("Remote".into()));
        assert_ne!(names[2].0, "Remote");
        assert!(
            names[3..]
                .iter()
                .all(|(_, name)| *name == HouseUiName::Computer)
        );
        assert_eq!(
            HouseUiName::bounded("\u{1F600}".repeat(11).as_str())
                .chars()
                .count(),
            10
        );
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
        session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
        let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
            .unwrap();
        let mut owner = ScenarioBootstrapRng::new(seed);
        let projection = owner.install_pre_fill_scenario_prefix_plan(plan).unwrap();
        let mut sim = owner.into_simulation(&descriptor(seed));
        let rules = techno_constructor_start_rules();
        initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
        assert_eq!(sim.session.current_house(), Some(sim.houses.order()[0]));
        let before = sim.scenario_rng.logical_state();
        let slots = normalized_launch_slots(launch.session());
        project_pre_fill_start_assignment(&mut sim, &slots, projection.assignment());
        assert_eq!(sim.scenario_rng.logical_state(), before);
        assert_eq!(
            sim.houses
                .get(&sim.houses.order()[0])
                .and_then(|house| house.base_center()),
            Some((40, 40))
        );
    }

    /// The start becomes the base centre only: the House edge (+0x577C) keeps
    /// the constructor's North (`0x004F5C79`) until a yard's DetermineEdge
    /// (`0x0050DB00`), here where the start lies nearest East.
    #[test]
    fn scenario_prefix_start_leaves_the_house_edge_north() {
        let seed = 0x5501;
        let start = Waypoint {
            index: 0,
            rx: 60,
            ry: 30,
        };
        let map = one_start_prefix_map(start);
        let mut session = one_player_battle_launch("projection.mmx").session().clone();
        session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
        let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
            .unwrap();
        let mut owner = ScenarioBootstrapRng::new(seed);
        let projection = owner.install_pre_fill_scenario_prefix_plan(plan).unwrap();
        let mut sim = owner.into_simulation(&descriptor(seed));
        let bounds = crate::map::playfield::PlayfieldBounds::from_map_header(&map.header);
        sim.set_playfield_bounds_for_test(Some(bounds));
        let rules = techno_constructor_start_rules();
        initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
        let slots = normalized_launch_slots(launch.session());
        project_pre_fill_start_assignment(&mut sim, &slots, projection.assignment());
        let house = &sim.houses[&sim.houses.order()[0]];
        assert_eq!(house.base_center(), Some((60, 30)));
        assert_eq!(
            crate::sim::house_state::determine_waypoint_edge((60, 30), bounds),
            1
        );
        assert_eq!(house.waypoint_edge(), 0);
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
        session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
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
            &crate::sim::world::TypeTechLevels::default(),
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
            &crate::sim::world::TypeTechLevels::default(),
            "Americans",
            0,
            HouseDifficulty::Normal,
            10,
            true,
            &mut expected_rng,
        );
        assert_eq!(actual_plan.nodes(), expected_plan.nodes());
        assert_eq!(actual_plan.percent_built(), expected_plan.percent_built());
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
        // This isolated RNG fixture omits the full native Rules prefix.
        sim.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        sim.session.set_binary_frame_for_test(77);
        let retained_handle = sim.substrate.allocate_stable_id();

        {
            let (mut scenario_fill, variant_main) = sim.terrain_load_draws();
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
        assert_eq!(sim.session.binary_frame(), 77);
        assert_eq!(retained_handle, 1);
        assert_eq!(sim.substrate.allocate_stable_id(), 2);
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
        // This isolated RNG fixture omits the full native Rules prefix.
        sim.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
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
        let ini = launch_fixture_ini(
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
                class: crate::skirmish_modes::MpModeClass::Battle,
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
            humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: "Player".to_string(),
                slot: SkirmishLocalSlot {
                    country: LaunchCountry::America,
                    country_random: false,
                    color_index: 0,
                    color_random: false,
                    start_position: LaunchStartPosition::Position(1),
                    team: LaunchTeam::None,
                },
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
            }],
            local_player_index: 0,
            ai_player_count: 0,
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
        scenario.mp_start_waypoints.insert(0, (start.rx, start.ry));
        let mut sim = owner.into_simulation(&scenario);
        // This isolated RNG fixture omits the full native Rules prefix.
        sim.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
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
                .find(|entity| sim.interner.resolve(entity.type_ref()) == type_id)
                .unwrap_or_else(|| panic!("generated {type_id} reached Simulation"));
            assert_eq!(entity.techno_ctor_random_word(), expected_word);
        }

        let starting_techno_word = (reference.next_u32() & 0xFFFF) as u16;
        let _starting_force_tail = reference.next_range_u32_inclusive(0, 0xFFFF);
        let launch_result = apply_pre_fill_scenario_prefix_launch_session_with_overlay_registry(
            &mut sim,
            &map,
            &house_roster,
            &rules,
            &terrain,
            &launch,
            &overlays,
            &projection,
        );
        assert_eq!(launch_result.spawned_mcvs, 1);
        let starting_techno = sim
            .entities()
            .values()
            .find(|entity| sim.interner.resolve(entity.type_ref()) == "MTNK")
            .expect("the production launch constructs its starting MCV");
        assert_eq!(
            starting_techno.techno_ctor_random_word(),
            starting_techno_word,
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
                sim.playfield_bounds(),
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
            fill_silos: false,
            rules: &rules,
            overlay_registry: &overlays,
            house_roster: &house_roster,
            skirmish_session: Some(&launch),
            campaign_map: None,
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

    /// Two humans then two AIs, in the House order CreateHouses builds.
    fn quick_coop_session(
        humans: [LaunchStartPosition; 2],
        ais: [LaunchStartPosition; 2],
    ) -> SkirmishLaunchSession {
        let mut session = one_player_battle_launch("quick-coop.map").session().clone();
        let mut partner = session.humans[0].clone();
        partner.raw_slot_index = Some(1);
        partner.name = "Partner".to_string();
        partner.slot.color_index = 1;
        session.humans.push(partner);
        for (human, start) in session.humans.iter_mut().zip(humans) {
            human.slot.start_position = start;
        }
        session.opponents = ais
            .into_iter()
            .enumerate()
            .map(
                |(index, start_position)| crate::skirmish_launch::SkirmishAiSlot {
                    raw_slot_index: index as u8 + 2,
                    country: LaunchCountry::Russia,
                    country_random: false,
                    color_index: index as u8 + 2,
                    color_random: false,
                    start_position,
                    team: LaunchTeam::None,
                    difficulty: crate::skirmish_launch::AiDifficulty::Easy,
                },
            )
            .collect();
        session.ai_player_count = 2;
        use crate::skirmish_launch::{PreFillAiHouseSlot, PreFillHouseRoster, PreFillHumanHouse};
        session.pre_fill_house_roster = PreFillHouseRoster::new(
            (0..2)
                .map(|order| PreFillHumanHouse {
                    priority: i32::from(order),
                    source_order: order,
                    observer: false,
                })
                .collect(),
            (0..8)
                .map(|slot_index| PreFillAiHouseSlot {
                    slot_index,
                    valid: matches!(slot_index, 2 | 3),
                })
                .collect(),
        );
        session
    }

    /// Starts on one row at x = 10, 20, 40, 80, 100.
    fn quick_coop_row() -> Vec<Waypoint> {
        [10, 20, 40, 80, 100]
            .into_iter()
            .enumerate()
            .map(|(index, rx)| Waypoint {
                index: index as u32,
                rx,
                ry: 10,
            })
            .collect()
    }

    fn quick_coop_assign(session: &SkirmishLaunchSession, rng: &mut SimRng) -> Vec<(u16, usize)> {
        let starts = quick_coop_row();
        let preassignment = native_preassign_launch_start_table(session, starts.len());
        let assignment = native_assign_quick_coop_starts_from_preassignment(
            session,
            &starts,
            &preassignment,
            rng,
        );
        assignment
            .placements
            .iter()
            .map(|(slot, start)| (start.rx, *slot))
            .collect()
    }

    #[test]
    fn quick_coop_starts_seat_humans_first_and_ignore_preset_ai_starts() {
        use LaunchStartPosition::{Auto, Position};
        // Human A keeps preset start 2. With 2 and AI 1's preset 4 used,
        // human B takes the farthest free start (x 10). AI 1 then picks as
        // if it had no preset: farthest from 10, 40 and 100 is x 80. AI 2
        // takes the last free start.
        let session = quick_coop_session([Position(2), Auto], [Position(4), Auto]);
        let mut rng = SimRng::new(0x5EE9);
        let before = rng.state();
        let mut placements = quick_coop_assign(&session, &mut rng);
        placements.sort_by_key(|(_, slot)| *slot);
        assert_eq!(placements, vec![(40, 0), (10, 1), (80, 2), (20, 3)]);
        assert_eq!(rng.state(), before, "no picker branch here draws");

        let starts = quick_coop_row();
        let preassignment = native_preassign_launch_start_table(&session, starts.len());
        let assignment = native_assign_quick_coop_starts_from_preassignment(
            &session,
            &starts,
            &preassignment,
            &mut rng,
        );
        assert_eq!(
            assignment.start_table,
            vec![Some(1), Some(3), Some(0), Some(2), Some(2)]
        );
    }

    #[test]
    fn quick_coop_starts_draw_for_the_first_human_and_the_first_ai() {
        use LaunchStartPosition::Auto;
        let session = quick_coop_session([Auto, Auto], [Auto, Auto]);
        let nearest = |rx: u16| match rx {
            10 => 20,
            20 => 10,
            40 => 20,
            80 => 100,
            100 => 80,
            _ => unreachable!(),
        };
        for seed in 0..32 {
            let mut rng = SimRng::new(seed);
            let mut expected = rng.clone();
            let first = expected.next_range_u32_inclusive(0, 4) as usize;
            let ai_draw = expected.next_range_u32_inclusive(0, 2) as usize;
            let placements = quick_coop_assign(&session, &mut rng);
            assert_eq!(rng.state(), expected.state(), "seed {seed}: two draws");

            let row: Vec<u16> = quick_coop_row().iter().map(|start| start.rx).collect();
            let start_of = |slot: usize| placements.iter().find(|(_, s)| *s == slot).unwrap().0;
            assert_eq!(start_of(0), row[first]);
            assert_eq!(start_of(1), nearest(row[first]));
            let free: Vec<u16> = row
                .iter()
                .copied()
                .filter(|rx| *rx != start_of(0) && *rx != start_of(1))
                .collect();
            assert_eq!(start_of(2), free[ai_draw]);
        }
    }

    #[test]
    fn quick_coop_picker_third_house_of_three_takes_the_last_start_without_a_draw() {
        // N = 3, k = 2, AI: RandomRanged(0, 0) returns at min == max
        // (0x0065C7EF) without consuming the stream.
        let starts = &quick_coop_row()[..3];
        let mut rng = SimRng::new(9);
        let before = rng.state();
        assert_eq!(
            choose_quick_coop_start(starts, &[true, true, false], false, &mut rng),
            2
        );
        assert_eq!(rng.state(), before);
    }

    #[test]
    fn quick_coop_picker_gives_an_ai_on_an_empty_table_the_first_start() {
        let starts = quick_coop_row();
        let mut rng = SimRng::new(7);
        let before = rng.state();
        assert_eq!(
            choose_quick_coop_start(&starts, &[false; 5], false, &mut rng),
            0
        );
        assert_eq!(rng.state(), before);
    }

    #[test]
    fn quick_coop_starts_replace_the_mode_chooser_in_the_prefix() {
        use LaunchStartPosition::Auto;
        let session = quick_coop_session([Auto, Auto], [Auto, Auto]);
        let row: Vec<Waypoint> = [6, 12, 20, 30, 38]
            .into_iter()
            .enumerate()
            .map(|(index, rx)| Waypoint {
                index: index as u32,
                rx,
                ry: 20,
            })
            .collect();
        let map = prefix_map_with_starts(&row);
        let battle = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let quick_coop = battle.clone().with_quick_coop_starts();
        for seed in 0x5EE0..0x5EF0 {
            for (launch, partner_nearest) in [(&battle, false), (&quick_coop, true)] {
                let plan =
                    prepare_stock_offline_scenario_prefix_plan(launch, &map, &map.waypoints, seed)
                        .unwrap();
                let starts = plan.final_gathered_starts();
                let placements = &plan.projection.assignment.placements;
                let start_of = |slot: usize| {
                    placements
                        .iter()
                        .find(|(placed, _)| *placed == slot)
                        .unwrap()
                        .1
                };
                let first = start_of(0);
                let others = || {
                    starts
                        .iter()
                        .copied()
                        .filter(move |start| *start != first)
                        .map(move |start| (native_start_distance(first, start), start))
                };
                // Battle seats the second house farthest from the first;
                // AssignStartingPoints seats the partner nearest.
                let expected = if partner_nearest {
                    others().min_by_key(|(distance, _)| *distance)
                } else {
                    others().max_by_key(|(distance, _)| *distance)
                };
                assert_eq!(
                    native_start_distance(first, start_of(1)),
                    expected.unwrap().0,
                    "seed {seed:#x}, quick co-op {partner_nearest}"
                );
            }
        }
    }
}
