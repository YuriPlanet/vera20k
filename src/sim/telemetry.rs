//! Dashboard telemetry: the read-only snapshot schema consumed by the remote
//! debugger frontend (the browser visualization client).
//!
//! `TelemetrySnapshot` is the wire contract between the engine and the
//! dashboard. It is deliberately *derived data*: `Simulation::telemetry_snapshot`
//! walks live state on demand and returns a plain, versioned,
//! serde-serializable struct. Nothing in this module is stored on
//! `Simulation`, serialized into authoritative state, or hashed. Sampling
//! changes no gameplay state, saves or replay inputs.
//!
//! ## Design notes
//!
//! - **Pull model.** The capture is one pass over the entity store, sampled
//!   by the app at dashboard rate (5–10 Hz), never per tick. Aggregation runs
//!   only on demand; separate frame profiling is opt-in and defaults off.
//! - **Wire format.** The app serializes with `serde_json` (text WebSocket
//!   frames) and the browser parses directly; the payload is aggregate data
//!   plus capped spots. Byte and collection limits bound the 20k-object case.
//! - **Spots.** `spots` is the dashboard's map view: one entry per entity,
//!   flattened to f32 world cells (cell + sub-cell). `owner_index` addresses
//!   `houses` (u16::MAX = no house row). Capped at `TELEMETRY_MAX_SPOTS` in
//!   store order; viewport culling and LOD are a later, camera-aware step.
//!   Constructor-held (limbo) entities sit at (0,0) by native semantics —
//!   `store_spawned_limbo` resets the CoordStruct to the off-map default —
//!   so the map view shows them piled at the origin.
//! - **Measured data only.** No event-history or pathfinding-health counters
//!   are invented where the current simulation has no observation source. The
//!   `path_batch` block (v24) is the batch's own accounting: it already makes
//!   those counts to enforce its caps and its budget, so it reports them.
//! - **Wire stability.** Houses and spots are `Vec`s in store/interned-id
//!   order — deterministic across peers and runs. New fields arrive as
//!   `Option` or new sections under a bumped `TELEMETRY_SCHEMA_VERSION`.
//! - **Debug panel.** `debug` carries cursor cell and walkability, entities at
//!   the cursor, selected movers' paths and miner state.
//!   The app layer fills it from cursor and selection state; the
//!   simulation itself never does. All fields in this protocol are read-only;
//!   the schema does not define requests that mutate the game.
//!
//! ## Dependency rules
//! - Part of sim/; no render/ui/audio/net dependencies.

use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::sim::intern::InternedId;
use crate::sim::production::PRODUCTION_STEPS;
use crate::sim::world::Simulation;

/// Wire schema version. Bump on any breaking change to `TelemetrySnapshot`.
///
/// v13: `profile` gained `phases_ms`, the per-subsystem frame ledger the report
/// table lists. The four pie buckets are derived from it, not added alongside.
/// v14: `render` — the per-phase breakdown of one in-game render.
/// v15: `logs` — incremental log records for the dashboard's log view.
/// v17: houses carry their live power reading.
/// v21: houses carry `production` (what each factory is building, its queue and
/// its ready-to-place stock), `squads` (the computer's live `TeamScriptState`
/// teams with their members and script step) and `base_plan` (its planned base
/// positions).
/// v22: read-only schema; trainer state is absent and collections are bounded.
/// v23: optional developer-tools capability and bounded selected-object history.
/// v24: `path_batch` — the path batch pass's own counts and durations.
/// v25: `history` — 1 Hz match summaries recorded whether or not a dashboard is
/// attached; sent once, to a client that attached since the last publish.
/// v26: `report_points` — report-grade samples (5 s) carrying exactly the
/// fields the dashboard's match report reads, so a past point can render the
/// full report. Spots, logs, debug and production detail stay out.
pub const TELEMETRY_SCHEMA_VERSION: u32 = 26;

/// One house row of a [`ReportPoint`]: just what the report's party table and
/// pie captions read.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReportHouseRow {
    pub name: String,
    pub is_human: bool,
    pub passive: bool,
    pub structures: u32,
    pub credits: i32,
    pub score: i32,
}

/// A report-grade sample of a match at one tick.
///
/// It carries exactly the readings the dashboard's match report consumes, so
/// the report renderer can be pointed at a past sample unchanged. `timing`
/// and `render` are filled by the app (their owners are the render loop).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReportPoint {
    pub tick: u64,
    pub entities_total: u32,
    pub map_width: u16,
    pub map_height: u16,
    pub houses: Vec<ReportHouseRow>,
    pub timing: Option<TimingSample>,
    pub profile: Option<crate::sim::frame_profile::FrameProfileReport>,
    pub render: Option<crate::sim::telemetry::RenderTimingReport>,
    pub path_batch: Option<crate::sim::movement::PathBatchStats>,
}

/// One 1 Hz summary of a match, recorded by the app for the whole match.
///
/// The dashboard draws these as the match-long timeline; a client that
/// attaches mid-match receives the ring once so the time before it is not a
/// hole. Render and phase-ledger fields are deliberately absent: those are
/// measured only while a client is attached.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MatchHistorySample {
    pub tick: u64,
    /// Mean and slowest simulation-advance wall time over the sample's ticks.
    pub tick_ms: f32,
    pub peak_ms: f32,
    pub entities: u32,
    pub houses: u32,
    /// The path batch's own counts at the sample's last tick; `None` while the
    /// simulation searches serially.
    pub path_batch: Option<crate::sim::movement::PathBatchStats>,
}

/// Spot ceiling per snapshot: the project's headline unit scale. Beyond it
/// the capture keeps the first N in store order (deterministic) and the
/// dashboard renders a truncated world rather than an unbounded payload.
pub const TELEMETRY_MAX_SPOTS: usize = 20_000;

/// Per-house roster ceiling. The secondary screen lists what a house actually
/// fields; a long tail of one-off types belongs on a drill-down, not the wall.
pub const TELEMETRY_MAX_ROSTER: usize = 16;
/// Thirty participants plus the ordinary neutral/special house slots.
pub const TELEMETRY_MAX_HOUSES: usize = 32;
pub const TELEMETRY_MAX_PRODUCTION: usize = 8;
pub const TELEMETRY_MAX_QUEUE: usize = 8;
pub const TELEMETRY_MAX_SQUADS: usize = 16;
pub const TELEMETRY_MAX_COMPOSITION: usize = 8;
pub const TELEMETRY_MAX_BASE_PLAN: usize = 32;
/// Byte limit, shortened only at UTF-8 character boundaries.
pub const TELEMETRY_MAX_NAME_BYTES: usize = 64;

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;

/// Live population of one entity category.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CategoryCounts {
    pub unit: u32,
    pub infantry: u32,
    pub aircraft: u32,
    pub structure: u32,
}

impl CategoryCounts {
    /// All four categories summed.
    pub fn total(&self) -> u32 {
        self.unit
            .saturating_add(self.infantry)
            .saturating_add(self.aircraft)
            .saturating_add(self.structure)
    }

    /// Count one entity into its category bucket.
    pub(crate) fn add(&mut self, category: EntityCategory) {
        match category {
            EntityCategory::Unit => self.unit = self.unit.saturating_add(1),
            EntityCategory::Infantry => self.infantry = self.infantry.saturating_add(1),
            EntityCategory::Aircraft => self.aircraft = self.aircraft.saturating_add(1),
            EntityCategory::Structure => self.structure = self.structure.saturating_add(1),
        }
    }
}

/// One house's identity and live population, as the dashboard sees it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HousePopulation {
    /// House name resolved through the interner (e.g. "Americans").
    pub name: String,
    /// Rules-owned side index: the position in the loaded `[Sides]` section.
    /// Not a fixed enum — retail order differs from test fixtures, so the app
    /// also sends `side_name`, the authoritative spelling.
    pub side_index: i32,
    /// The `[Sides]` key this house belongs to ("Allied", "ThirdSide",
    /// "Civilian", "Mutant", …), resolved from the loaded rules. `None` when
    /// no rule set is available.
    pub side_name: Option<String>,
    /// The house's AI difficulty ("hard" / "normal" / "easy"). Human houses
    /// keep Normal; the dashboard only shows this for computer houses.
    pub difficulty: String,
    pub is_human: bool,
    /// True for the non-contending houses the engine keeps for map objects:
    /// `Neutral` and `Special` own scenery and civilians but never take part
    /// in the match, so the dashboard filters them out of any lineup.
    pub passive: bool,
    pub counts: CategoryCounts,
    /// The house's own team colour, RGB, from the rules `[Colors]` scheme the
    /// player picked — the colour the game draws its units in. `None` for
    /// houses without a player colour (Neutral, Special) and until the app
    /// fills it: the simulation does not own colour schemes, so the app
    /// resolves this from the launch session's house-colour map.
    pub color: Option<[u8; 3]>,
    /// Live cash balance (native house `+0x30C`).
    pub credits: i32,
    /// Health-scaled power output this house's buildings produce.
    pub power_output: i32,
    /// Full-rated power drain they consume.
    pub power_drain: i32,
    /// Whether the house is currently in low power (native `House+577B`).
    pub low_power: bool,
    /// Running total taken by `Spend_Money` (native house `+0x2DC`).
    pub spent: i32,
    /// House score (native house `+0x54E8`): deposits, kills and captures.
    pub score: i32,
    /// Whether this house has been eliminated.
    pub defeated: bool,
    /// Historical primary base cell (native `HouseClass+0x5490`); `None` until
    /// a building lifecycle boundary publishes it.
    pub base_center: Option<(u16, u16)>,
    /// Live unit-type roster for this house, largest first. Capped at
    /// `TELEMETRY_MAX_ROSTER`: the secondary screen lists the bulk, not the
    /// long tail.
    pub roster: Vec<TypeCount>,
    /// What this house is building right now, and what it has finished but not
    /// yet placed. Empty for a house with no factory.
    pub production: Vec<ProductionLine>,
    /// The computer's live teams. Empty for a human house — a human's units are
    /// not scripted into teams.
    pub squads: Vec<AiSquad>,
    /// The computer's planned base positions. Empty for a human house.
    pub base_plan: Vec<BasePlan>,
}

/// One object type held by a house, by live count. The secondary screen's
/// roster line ("GRIZZLY ×6"); derived from the entity store, never stored.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeCount {
    /// Object type as the rules spell it ("GRIZZLY", "GAPOWR").
    pub type_name: String,
    pub category: EntityCategory,
    pub count: u32,
}

/// One entity's spot for the dashboard's map view. Presentation-derived:
/// sub-cell precision flattened to f32 world cells.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EntitySpot {
    /// World cell X = cell rx + sub-cell offset (0..256) / 256.
    pub x: f32,
    /// World cell Y = cell ry + sub-cell offset (0..256) / 256.
    pub y: f32,
    /// Elevation level (0 = ground).
    pub z: u8,
    pub category: EntityCategory,
    /// Index into `TelemetrySnapshot::houses`; u16::MAX when the entity's
    /// owner has no house row.
    pub owner_index: u16,
}

/// Frame-timing sample, filled by the app layer (the simulation never
/// measures wall time). `budget_utilization` is `tick_ms` over `budget_ms`;
/// values above 1.0 mean the tick cost exceeded its budget and the pacer can
/// no longer hold the tick rate.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TimingSample {
    /// The most recent tick's cost, in milliseconds.
    pub tick_ms: f32,
    /// Median tick cost over the app's sampling window.
    pub p50_ms: f32,
    /// 99th-percentile tick cost over the app's sampling window.
    pub p99_ms: f32,
    /// The pacer's tick interval. None for uncapped pacing.
    pub budget_ms: Option<f32>,
    /// `tick_ms` divided by the paced budget; None when uncapped.
    pub budget_utilization: Option<f32>,
    /// Presentation frames per second, from the app's render-frame timer.
    /// 0 while no sample has been taken (menu, or before the first redraw).
    pub fps: f32,
    /// Committed simulation ticks per second, measured over the app's
    /// wall-clock window. 0 while the runtime is paused or frozen.
    pub tick_hz: f32,
}

/// One entity's compact description for the dashboard's cursor list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EntityDesc {
    pub type_name: String,
    /// "Unit" / "Inf" / "Bld" / "Air" — the egui panel's short labels.
    pub category: String,
    pub owner: String,
    pub hp: i32,
    pub max_hp: i32,
}

/// One SpeedType's land row at the cursor cell; cost 0 means impassable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LandRow {
    pub speed_type: String,
    pub cost: u8,
}

/// One step of a selected mover's remaining path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PathStep {
    pub cell: (u16, u16),
    pub walkable: bool,
    /// The mover's next cell (the egui panel's `>` marker).
    pub first: bool,
}

/// Miner state for a selected harvester, mirroring the egui panel.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinerDebug {
    pub kind: String,
    pub state: String,
    pub cargo: u32,
    pub capacity: u32,
    pub stage: i32,
    pub rate: i32,
    pub harvesting: bool,
    pub archived_ore: Option<(u16, u16)>,
    pub teleport_active: bool,
}

/// One selected entity's read-only identity, position, path and miner state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SelectedDebug {
    pub stable_id: u64,
    pub type_name: String,
    pub cell: (u16, u16),
    /// Sub-cell offsets, 0..256.
    pub sub: (u16, u16),
    pub moving: bool,
    pub path_steps_left: u32,
    pub path_blocked: bool,
    pub goal: Option<(u16, u16)>,
    /// The first steps of the remaining path, the mover's next cell first.
    pub path_preview: Vec<PathStep>,
    pub miner: Option<MinerDebug>,
    #[cfg(feature = "debug-tools")]
    #[serde(default)]
    pub inspector_active: bool,
    #[cfg(feature = "debug-tools")]
    #[serde(default)]
    pub events: Vec<DebugEventReport>,
}

#[cfg(feature = "debug-tools")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DebugEventReport {
    pub tick: u32,
    pub text: String,
}

/// The dashboard's read-only terrain and selection observations.
/// Filled by the app layer from live cursor, selection and
/// overlay-switch state; `None` when no match is running.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DebugPanel {
    /// Cell under the cursor, when the cursor maps to the map.
    pub cursor_cell: Option<(u16, u16)>,
    /// Elevation level at the cursor cell.
    pub cursor_elevation: u8,
    /// PathGrid dimensions, when one exists.
    pub grid: Option<(u16, u16)>,
    /// Whether the cursor cell is walkable.
    pub cursor_walkable: Option<bool>,
    /// Land rows for the five locomotor SpeedTypes at the cursor.
    pub land_rows: Vec<LandRow>,
    /// The terrain-cost overlay's active SpeedType (`[` `]` cycle it).
    pub active_speed_type: String,
    /// VXL sprites rendered by the last atlas build.
    pub vxl_sprites: Option<u32>,
    /// Entities standing on the cursor cell.
    pub entities_at_cursor: Vec<EntityDesc>,
    /// The structure whose footprint covers the cursor, if any.
    pub footprint: Option<String>,
    /// Selected entities, in store order.
    pub selected: Vec<SelectedDebug>,
    /// Debug-overlay switch states, so the dashboard's switches show the
    /// engine's real state (they may also be toggled by the game hotkeys).
    pub overlays: OverlayFlags,
}

/// Debug-overlay switch states — the dashboard's readback for its switches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OverlayFlags {
    /// Terrain-cost / PathGrid wash (`^+P` / F9).
    pub terrain_costs: bool,
    /// Heightmap overlay (`^+K`).
    pub heightmap: bool,
    /// Cell grid overlay (`^+L`).
    pub cell_grid: bool,
    /// Fog of war (the engine stores the inverse: `sandbox_full_visibility`).
    pub fog: bool,
    /// Foot-path step overlay (`^+O`).
    pub foot_path: bool,
    /// Debug pause (`^+J`).
    pub paused: bool,
    #[cfg(feature = "debug-tools")]
    #[serde(default)]
    pub unit_inspector: bool,
}

/// Process-level resources, filled by the app layer: the simulation has no
/// business knowing how much CPU or memory the host spends on it. `None` on
/// platforms the app cannot sample.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProcessStats {
    /// Share of the whole machine's CPU capacity, 0..100 across all cores.
    pub cpu_percent: f32,
    /// Logical cores the percentage is measured against.
    pub cpu_cores: u32,
    /// Resident working set in MiB.
    pub working_set_mb: f64,
    /// High-water working set in MiB.
    pub peak_working_set_mb: f64,
    /// Live thread count.
    pub threads: u32,
    /// The adapter selected at startup (`None` before the GPU is up).
    pub gpu_adapter: Option<String>,
    /// Palette-index texture bytes this process keeps resident (sprite and
    /// unit atlas pages) in MiB. This is **our own** texture footprint, not the
    /// card's total usage: swap-chain, driver and other allocations are not
    /// visible from inside the process, and hardware utilization percentages
    /// need vendor driver counters that this build does not read.
    pub gpu_texture_mb: f64,
}

/// Rolling means of one in-game render, in milliseconds.
///
/// App-filled: the simulation owns no render, so this rides the snapshot the
/// same way `timing` and `process` do. Order matches `RenderPhases::spans`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RenderTimingReport {
    /// Renders averaged.
    pub frames: u32,
    /// Phase 1 — terrain, overlays, entities.
    pub world_ms: f32,
    /// Phase 2 — pathgrid / cell grid / heightmap overlays.
    pub debug_ms: f32,
    /// Phase 3 — shroud ABuffer rebuild.
    pub shroud_ms: f32,
    /// Phase 4 — minimap update + UI instances.
    pub ui_ms: f32,
    /// Phase 5 — sidebar instances.
    pub sidebar_ms: f32,
    /// Phase 6 — instance upload to the GPU buffer pool.
    pub upload_ms: f32,
    /// Prep before phase 1 and everything after phase 6.
    pub other_ms: f32,
}

/// One house's production line, projected from its `FactoryRegistry`.
///
/// The registry is keyed by holder, so an AI house's queues live under its
/// factory *buildings* rather than under the house slot. The projection walks
/// the keyed factories instead of asking per `(owner, category)`, because an AI
/// can have several factories of one category at once and the house slot would
/// answer with only one of them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProductionLine {
    /// The sidebar category, serialized by its own name so the panel can label
    /// it without a second table.
    pub category: String,
    /// The type the factory is actively building, and its display name.
    pub active_type: Option<String>,
    /// `0..=PRODUCTION_STEPS`; equals `PRODUCTION_STEPS` when finished.
    pub progress: u16,
    /// Whether the factory is stopped (`OnHold`) or suspended (a base-defense
    /// response), so a stalled progress bar is distinguishable from a slow one.
    pub on_hold: bool,
    pub suspended: bool,
    /// Queued, not-yet-active builds in queue order.
    pub queue: Vec<String>,
    /// Completed and waiting to be placed — the ones a click on the sidebar
    /// consumes.
    pub ready_to_place: Vec<String>,
}

/// One AI team (`TeamScriptState`), as the dashboard lists it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AiSquad {
    /// The team's construction id, stable for the match.
    pub id: u64,
    /// The `TeamTypes` entry's name, or `None` for a team with no TeamType.
    pub team_type: Option<String>,
    /// The `TaskForce` the team is filling.
    pub task_force: Option<String>,
    /// Retained member-reference count and present per-type breakdown, largest first.
    pub members: u32,
    pub composition: Vec<TypeCount>,
    /// `+0x2C`, the script action the team is running; -1 before the first.
    pub script_step: i32,
    /// `+0x7F`: the team has formed and is running its script.
    pub formed: bool,
    /// `+0x7A`: it is below its TaskForce's requested strength.
    pub under_strength: bool,
    /// Suspended by the base-defense response, so the panel can say why a team
    /// is standing still.
    pub suspended: bool,
}

/// One planned base position in the computer's expansion (`ai_base_site`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BasePlan {
    /// The cell the site is anchored at.
    pub x: u16,
    pub y: u16,
    /// The type the site wants built there, when one is chosen.
    pub building: Option<String>,
    /// Legacy wire label: true while this retained node is not yet filled.
    pub is_base: bool,
}

/// One log record on the wire.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LogRecord {
    /// Monotonic record number; the dashboard uses it to drop duplicates.
    pub seq: u64,
    /// 1 = error, 2 = warn, 3 = info, 4 = debug, 5 = trace.
    pub level: u8,
    pub target: String,
    pub message: String,
    /// Milliseconds since logging started.
    pub t_ms: u64,
}

/// Log records captured since the previous snapshot.
///
/// Incremental on purpose: the file log runs to hundreds of megabytes during
/// map load, so the wire carries only what the reader has not seen.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LogBatch {
    /// Records the ring evicted before they could be sent.
    pub dropped: u64,
    pub lines: Vec<LogRecord>,
}

/// The optional developer build's capability and live trainer readback.
#[cfg(feature = "debug-tools")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DebugToolsReport {
    pub enabled: bool,
    pub unavailable_reason: Option<String>,
    /// App-owned session epoch; no commands are accepted without one.
    pub session: Option<u64>,
    pub cheats: Option<CheatReport>,
}

/// Original contributor's trainer readback, bounded to the local house.
#[cfg(feature = "debug-tools")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CheatReport {
    pub infinite_money: bool,
    pub money_target: i32,
    pub infinite_power: bool,
    pub instant_build: bool,
    pub invincible: bool,
    pub damage_scale: u16,
    pub reveal_map: bool,
    pub tech_all: bool,
    pub tech_boost: bool,
    pub build_anywhere: bool,
    pub owner: Option<String>,
    pub power_forced: bool,
}

/// One dashboard sample: the whole wire payload per snapshot.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TelemetrySnapshot {
    pub version: u32,
    /// Authoritative tick the sample was taken on.
    pub tick: u64,
    /// Wrapping frame counter committed with that tick.
    pub binary_frame: u32,
    /// Map dimensions in cells; frames the dashboard's ground plane.
    pub map_width: u16,
    pub map_height: u16,
    /// Every entity stored in the entity store, limbo (cargo, paradrop)
    /// included. The sum of house counts is lower only when an entity names
    /// an owner with no house row — a data anomaly the dashboard should
    /// surface, not hide.
    pub entities_total: u32,
    /// In `BTreeMap` (interned-id) order, capped at `TELEMETRY_MAX_HOUSES`.
    pub houses: Vec<HousePopulation>,
    /// One spot per entity in store order, capped at `TELEMETRY_MAX_SPOTS`.
    pub spots: Vec<EntitySpot>,
    /// App-filled frame timing; `None` until the app has sampled a frame.
    pub timing: Option<TimingSample>,
    /// App-filled terrain and selection observations; `None`
    /// until the app layer fills it.
    pub debug: Option<DebugPanel>,
    /// App-filled process resources (CPU, memory, threads, GPU texture).
    pub process: Option<ProcessStats>,
    /// Mean cost per simulation tick in the current observation window.
    pub profile: Option<crate::sim::frame_profile::FrameProfileReport>,
    /// The path batch's own accounting of its latest pass; `None` while the
    /// simulation searches serially, since then no pass runs.
    pub path_batch: Option<crate::sim::movement::PathBatchStats>,
    /// Whole-match 1 Hz summaries; app-filled, and only on the first publish
    /// after a client attached.
    pub history: Option<Vec<MatchHistorySample>>,
    /// Report-grade 5 s samples; app-filled, and only on the first publish
    /// after a client attached.
    pub report_points: Option<Vec<ReportPoint>>,
    /// The newest report-grade sample; app-filled on every snapshot, so a long
    /// session keeps gaining reportable points after the initial backfill.
    pub report_point: Option<ReportPoint>,
    /// App-filled per-phase render breakdown; `None` until the app has rendered
    /// a game frame.
    pub render: Option<RenderTimingReport>,
    /// App-filled log records since the previous snapshot.
    pub logs: Option<LogBatch>,
    #[cfg(feature = "debug-tools")]
    pub debug_tools: DebugToolsReport,
}

/// Bound copied identifiers before allocating wire strings. Never splits UTF-8.
fn bounded_name(value: &str) -> String {
    let mut end = value.len().min(TELEMETRY_MAX_NAME_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn category_order(category: EntityCategory) -> u8 {
    match category {
        EntityCategory::Unit => 0,
        EntityCategory::Infantry => 1,
        EntityCategory::Aircraft => 2,
        EntityCategory::Structure => 3,
    }
}

fn type_counts(
    counts: &BTreeMap<(u8, InternedId), (EntityCategory, u32)>,
    interner: &crate::sim::intern::StringInterner,
    limit: usize,
) -> Vec<TypeCount> {
    let mut rows: Vec<_> = counts.iter().collect();
    rows.sort_by(
        |((category_a, type_a), (_, count_a)), ((category_b, type_b), (_, count_b))| {
            count_b
                .cmp(count_a)
                .then_with(|| interner.resolve(*type_a).cmp(interner.resolve(*type_b)))
                .then_with(|| category_a.cmp(category_b))
        },
    );
    rows.into_iter()
        .take(limit)
        .map(|((_, id), (category, count))| TypeCount {
            type_name: bounded_name(interner.resolve(*id)),
            category: *category,
            count: *count,
        })
        .collect()
}

impl Simulation {
    /// Capture the retained owners without changing simulation state. Rows and
    /// strings are bounded before wire allocation; totals include all entities.
    pub fn telemetry_snapshot(&self) -> TelemetrySnapshot {
        let mut owner_index = BTreeMap::new();
        let mut houses: Vec<HousePopulation> =
            Vec::with_capacity(self.houses.len().min(TELEMETRY_MAX_HOUSES));
        for (index, (owner, house)) in self.houses.iter().take(TELEMETRY_MAX_HOUSES).enumerate() {
            owner_index.insert(*owner, index);
            let difficulty = match house.difficulty() {
                crate::sim::house_state::HouseDifficulty::Hard => "hard",
                crate::sim::house_state::HouseDifficulty::Normal => "normal",
                crate::sim::house_state::HouseDifficulty::Easy => "easy",
            };
            houses.push(HousePopulation {
                name: bounded_name(self.interner.resolve(house.name())),
                side_index: house.side_index(),
                side_name: None,
                difficulty: difficulty.to_owned(),
                is_human: house.is_human(),
                passive: house.multiplay_passive(),
                counts: CategoryCounts::default(),
                color: None,
                credits: house.economy().credits(),
                power_output: self
                    .power_states
                    .get(owner)
                    .map_or(0, |power| power.total_output()),
                power_drain: self
                    .power_states
                    .get(owner)
                    .map_or(0, |power| power.total_drain()),
                low_power: self
                    .power_states
                    .get(owner)
                    .is_some_and(|power| power.is_low_power()),
                spent: house.economy().spent_credits(),
                score: house.economy().score(),
                defeated: house.is_defeated(),
                base_center: house.base_center(),
                roster: Vec::new(),
                production: Vec::new(),
                squads: Vec::new(),
                base_plan: Vec::new(),
            });
        }
        let mut rosters = vec![BTreeMap::new(); houses.len()];
        let mut spots = Vec::with_capacity(self.entities().len().min(TELEMETRY_MAX_SPOTS));
        let mut entities_total = 0u32;
        for entity in self.entities().values() {
            entities_total = entities_total.saturating_add(1);
            let index = owner_index.get(&entity.owner()).copied();
            let category = entity.category();
            if let Some(index) = index {
                houses[index].counts.add(category);
                let row = rosters[index]
                    .entry((category_order(category), entity.type_ref()))
                    .or_insert((category, 0u32));
                row.1 = row.1.saturating_add(1);
            }
            if spots.len() < TELEMETRY_MAX_SPOTS {
                let position = &entity.position;
                spots.push(EntitySpot {
                    x: f32::from(position.rx()) + position.sub_x().to_num::<f32>() / SUB_CELL_UNITS,
                    y: f32::from(position.ry()) + position.sub_y().to_num::<f32>() / SUB_CELL_UNITS,
                    z: position.z(),
                    category,
                    owner_index: index.map_or(u16::MAX, |index| index as u16),
                });
            }
        }
        for (house, counts) in houses.iter_mut().zip(&rosters) {
            house.roster = type_counts(counts, &self.interner, TELEMETRY_MAX_ROSTER);
        }

        for factory in self.production.factories().iter_insertion_ordered() {
            let Some(&index) = owner_index.get(&factory.owner()) else {
                continue;
            };
            let rows = &mut houses[index].production;
            if rows.len() == TELEMETRY_MAX_PRODUCTION {
                continue;
            }
            let type_name = |id| bounded_name(self.interner.resolve(id));
            let (active_type, ready_to_place) = match factory.object() {
                Some(object) if factory.progress() >= PRODUCTION_STEPS => {
                    (None, vec![type_name(object.type_id)])
                }
                Some(object) => (Some(type_name(object.type_id)), Vec::new()),
                None => (None, Vec::new()),
            };
            rows.push(ProductionLine {
                category: factory.category().label().to_owned(),
                active_type,
                progress: factory.progress(),
                on_hold: factory.on_hold(),
                suspended: factory.suspended(),
                queue: factory
                    .queue()
                    .iter()
                    .take(TELEMETRY_MAX_QUEUE)
                    .map(|entry| type_name(entry.type_id))
                    .collect(),
                ready_to_place,
            });
        }

        for team in self.team_script_vm.teams_in_order() {
            let Some(&index) = owner_index.get(&team.owner()) else {
                continue;
            };
            let rows = &mut houses[index].squads;
            if rows.len() == TELEMETRY_MAX_SQUADS {
                continue;
            }
            let mut counts = BTreeMap::new();
            let mut members = 0u32;
            for member in team.members() {
                members = members.saturating_add(1);
                if let Some(entity) = self.entities().get(member) {
                    let category = entity.category();
                    let row = counts
                        .entry((category_order(category), entity.type_ref()))
                        .or_insert((category, 0u32));
                    row.1 = row.1.saturating_add(1);
                }
            }
            rows.push(AiSquad {
                id: team.id(),
                team_type: team
                    .team_type_id()
                    .map(|id| bounded_name(self.interner.resolve(id))),
                task_force: team
                    .task_force_id()
                    .map(|id| bounded_name(self.interner.resolve(id))),
                members,
                composition: type_counts(&counts, &self.interner, TELEMETRY_MAX_COMPOSITION),
                script_step: team.cursor(),
                formed: team.formed(),
                under_strength: team.under_strength(),
                suspended: team.is_suspended(),
            });
        }

        for (owner, index) in &owner_index {
            let source = &self.houses[owner];
            houses[*index].base_plan = source
                .base_plan()
                .nodes()
                .iter()
                .filter(|node| node.packed_cell != 0)
                .take(TELEMETRY_MAX_BASE_PLAN)
                .map(|node| BasePlan {
                    x: (node.packed_cell & 0xffff) as u16,
                    y: (node.packed_cell >> 16) as u16,
                    building: None,
                    is_base: !node.filled,
                })
                .collect();
        }

        TelemetrySnapshot {
            version: TELEMETRY_SCHEMA_VERSION,
            tick: self.session.tick(),
            binary_frame: self.session.binary_frame(),
            map_width: self.session.map_width(),
            map_height: self.session.map_height(),
            entities_total,
            houses,
            spots,
            timing: None,
            debug: None,
            process: None,
            profile: self.frame_profile().report(),
            path_batch: self.path_batch_stats(),
            history: None,
            report_points: None,
            report_point: None,
            render: None,
            logs: None,
            #[cfg(feature = "debug-tools")]
            debug_tools: {
                let flags = self.cheat_flags();
                DebugToolsReport {
                    enabled: self.trainer_available(),
                    unavailable_reason: self.trainer_unavailable_reason().map(str::to_owned),
                    session: None,
                    cheats: self.trainer_available().then(|| CheatReport {
                        infinite_money: flags.infinite_money,
                        money_target: flags.money_target,
                        infinite_power: flags.infinite_power,
                        instant_build: flags.instant_build,
                        invincible: flags.invincible,
                        damage_scale: flags.damage_scale,
                        reveal_map: flags.reveal_map,
                        tech_all: flags.tech_all,
                        tech_boost: flags.tech_boost,
                        build_anywhere: flags.build_anywhere,
                        owner: self
                            .trainer_owner()
                            .map(|owner| bounded_name(self.interner.resolve(owner))),
                        power_forced: self.trainer_power_forced(),
                    }),
                }
            },
        }
    }

    /// Build a report-grade sample of the current state. `timing` and `render`
    /// stay `None`: the render loop owns those readings, and the app fills them
    /// where it captures the point.
    pub fn report_point(&self) -> ReportPoint {
        let rows: Vec<ReportHouseRow> = self
            .houses
            .iter()
            .take(TELEMETRY_MAX_HOUSES)
            .map(|(_, house)| ReportHouseRow {
                name: bounded_name(self.interner.resolve(house.name())),
                is_human: house.is_human(),
                passive: house.multiplay_passive(),
                structures: 0,
                credits: house.economy().credits(),
                score: house.economy().score(),
            })
            .collect();
        let index: BTreeMap<InternedId, usize> = self
            .houses
            .keys()
            .take(TELEMETRY_MAX_HOUSES)
            .enumerate()
            .map(|(row, owner)| (*owner, row))
            .collect();
        let mut rows = rows;
        let mut entities_total = 0u32;
        for entity in self.entities().values() {
            entities_total = entities_total.saturating_add(1);
            if entity.category() == EntityCategory::Structure
                && let Some(&row) = index.get(&entity.owner())
            {
                rows[row].structures = rows[row].structures.saturating_add(1);
            }
        }
        ReportPoint {
            tick: self.session.tick(),
            entities_total,
            map_width: self.session.map_width(),
            map_height: self.session.map_height(),
            houses: rows,
            timing: None,
            profile: self.frame_profile().report(),
            render: None,
            path_batch: self.path_batch_stats(),
        }
    }
}

/// Sub-cell units per cell: the `Position` sub-cell range is 0..256 with 128
/// at the cell centre.
const SUB_CELL_UNITS: f32 = 256.0;
