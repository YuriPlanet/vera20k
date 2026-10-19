/**
 * Schema mirror of `src/sim/telemetry.rs` (v23); keep both in step.
 */

export const SCHEMA_VERSION = 23;

export type EntityCategory = "Unit" | "Infantry" | "Aircraft" | "Structure";

export interface CategoryCounts {
  unit: number;
  infantry: number;
  aircraft: number;
  structure: number;
}

export interface HousePopulation {
  name: string;
  /** Position in the loaded [Sides] section — not a fixed enum. */
  side_index: number;
  /** The [Sides] key this house belongs to ("Allied", "ThirdSide", …). */
  side_name: string | null;
  /** AI difficulty ladder: "hard" (brutal) | "normal" | "easy". */
  difficulty: string;
  is_human: boolean;
  /** Non-contending house (`Neutral`, `Special`): owns map scenery and
   *  civilians but never plays. Lineups filter it out. */
  passive: boolean;
  counts: CategoryCounts;
  /** The house's own team colour from the rules [Colors] scheme (RGB 0-255);
   *  null for houses without a player colour. */
  color: [number, number, number] | null;
  /** Live cash balance. */
  credits: number;
  power_output: number;
  power_drain: number;
  low_power: boolean;
  /** Running total spent. */
  spent: number;
  /** House score: deposits, kills and captures. */
  score: number;
  /** Whether this house has been eliminated. */
  defeated: boolean;
  /** Historical primary base cell; null until a building publishes it. */
  base_center: [number, number] | null;
  /** Live roster, largest first, capped at 16 types. */
  roster: TypeCount[];
  /** What this house is building right now (schema v21). */
  production: ProductionLine[];
  /** The computer's live teams; empty for a human house. */
  squads: AiSquad[];
  /** The computer's planned base positions; empty for a human house. */
  base_plan: BasePlanNodeReport[];
}

/** One factory's line, projected from the registry. An AI house has one entry
 *  per factory, so several may share a category. */
export interface ProductionLine {
  category: string;
  /** The type being built now; null when idle or finished-but-unplaced. */
  active_type: string | null;
  /** 0..54; equals 54 when the build has completed. */
  progress: number;
  on_hold: boolean;
  suspended: boolean;
  queue: string[];
  /** Completed and waiting for a placement click. */
  ready_to_place: string[];
}

/** One AI team (`TeamScriptState`). */
export interface AiSquad {
  id: number;
  team_type: string | null;
  task_force: string | null;
  members: number;
  composition: TypeCount[];
  script_step: number;
  formed: boolean;
  under_strength: boolean;
  suspended: boolean;
}

/** One planned base position. The engine's node holds a registry index, not a
 *  name, so only the cell and whether it is still outstanding are reported. */
export interface BasePlanNodeReport {
  x: number;
  y: number;
  building: string | null;
  is_base: boolean;
}

/** One object type held by a house, by live count. */
export interface TypeCount {
  type_name: string;
  category: EntityCategory;
  count: number;
}

export interface EntitySpot {
  x: number;
  y: number;
  z: number;
  category: EntityCategory;
  owner_index: number;
}

export interface TimingSample {
  tick_ms: number;
  p50_ms: number;
  p99_ms: number;
  /** No finite tick deadline at uncapped game speed. */
  budget_ms: number | null;
  budget_utilization: number | null;
  /** Presentation frames per second (0 when no sample has been taken). */
  fps: number;
  /** Committed simulation ticks per second (0 while paused or frozen). */
  tick_hz: number;
}

export interface EntityDesc {
  type_name: string;
  category: string;
  owner: string;
  hp: number;
  max_hp: number;
}

export interface LandRow {
  speed_type: string;
  cost: number;
}

export interface PathStep {
  cell: [number, number];
  walkable: boolean;
  first: boolean;
}

export interface MinerDebug {
  kind: string;
  state: string;
  cargo: number;
  capacity: number;
  stage: number;
  rate: number;
  harvesting: boolean;
  archived_ore: [number, number] | null;
  teleport_active: boolean;
}

export interface SelectedDebug {
  stable_id: number;
  type_name: string;
  cell: [number, number];
  sub: [number, number];
  moving: boolean;
  path_steps_left: number;
  path_blocked: boolean;
  goal: [number, number] | null;
  path_preview: PathStep[];
  miner: MinerDebug | null;
  /** Present only in a debug-tools build. */
  inspector_active?: boolean;
  events?: DebugEventEntry[];
}

export interface DebugEventEntry {
  tick: number;
  text: string;
}

export interface DebugPanel {
  cursor_cell: [number, number] | null;
  cursor_elevation: number;
  grid: [number, number] | null;
  cursor_walkable: boolean | null;
  land_rows: LandRow[];
  active_speed_type: string;
  vxl_sprites: number | null;
  entities_at_cursor: EntityDesc[];
  footprint: string | null;
  selected: SelectedDebug[];
  overlays: OverlayFlags;
}

/** Read-only debug-overlay states, mirrored from the engine. */
export interface OverlayFlags {
  terrain_costs: boolean;
  heightmap: boolean;
  cell_grid: boolean;
  fog: boolean;
  foot_path: boolean;
  paused: boolean;
  unit_inspector?: boolean;
}

/** Trainer state is reported only by the separately enabled debug-tools build. */
export interface CheatReport {
  infinite_money: boolean;
  money_target: number;
  infinite_power: boolean;
  instant_build: boolean;
  invincible: boolean;
  damage_scale: number;
  reveal_map: boolean;
  tech_all: boolean;
  tech_boost: boolean;
  build_anywhere: boolean;
  owner: string | null;
  power_forced: boolean;
}

export interface DebugToolsReport {
  /** True only for an eligible offline match, not merely a compiled feature. */
  enabled: boolean;
  unavailable_reason: string | null;
  /** Changes when the authoritative match changes; old commands are rejected. */
  session: number | null;
  cheats: CheatReport | null;
}

/** Process-level resources, filled by the app layer. */
export interface ProcessStats {
  /** Share of the whole machine's CPU capacity, 0..100 across all cores. */
  cpu_percent: number;
  cpu_cores: number;
  /** Resident working set, MiB. */
  working_set_mb: number;
  /** High-water working set, MiB. */
  peak_working_set_mb: number;
  threads: number;
  gpu_adapter: string | null;
  /** Our own resident atlas texture bytes, MiB — not the card's total usage. */
  gpu_texture_mb: number;
}

/** Measured simulation phases averaged per profiled tick, independent of renders. */
export interface FrameProfileReport {
  /** Profiled simulation ticks, including a terminal call. */
  frames: number;
  mean_frame_ms: number;
  peak_frame_ms: number;
  logic_ms: number;
  objects_ms: number;
  vision_ms: number;
  tail_ms: number;
  unmarked_ms: number;
  /** Per-subsystem ledger, in the engine's `PHASE_KEYS` order (schema v13).
   *  The four buckets above are sums over this array. */
  phases_ms: number[];
}

/** Per-phase means of one in-game render, in milliseconds (schema v14). */
export interface RenderTimingReport {
  frames: number;
  world_ms: number;
  debug_ms: number;
  shroud_ms: number;
  ui_ms: number;
  sidebar_ms: number;
  upload_ms: number;
  other_ms: number;
}

/** One log record on the wire (schema v15). */
export interface LogRecord {
  seq: number;
  /** 1 = error, 2 = warn, 3 = info, 4 = debug, 5 = trace. */
  level: number;
  target: string;
  message: string;
  t_ms: number;
}

/** Records captured since the previous snapshot. */
export interface LogBatch {
  /** Records the ring evicted before they could be sent. */
  dropped: number;
  lines: LogRecord[];
}

export interface TelemetrySnapshot {
  version: number;
  tick: number;
  binary_frame: number;
  map_width: number;
  map_height: number;
  entities_total: number;
  houses: HousePopulation[];
  spots: EntitySpot[];
  timing: TimingSample | null;
  debug: DebugPanel | null;
  process: ProcessStats | null;
  profile: FrameProfileReport | null;
  render: RenderTimingReport | null;
  logs: LogBatch | null;
  /** Absent in ordinary builds, including the read-only telemetry dashboard. */
  debug_tools?: DebugToolsReport | null;
}
