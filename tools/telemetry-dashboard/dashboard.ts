/**
 * VERA20k telemetry dashboard — browser client.
 *
 * Connects to the engine's telemetry server (same origin, /ws), receives
 * `TelemetrySnapshot` JSON at ~10 Hz and renders:
 *  - a 3D battlefield overview (three.js InstancedMesh),
 *  - a streaming tick-cost chart (canvas),
 *  - read-only terrain, selected paths and miner state,
 *  - per-house population and roster panels.
 *
 * Schema mirror of `src/sim/telemetry.rs`; keep both in step.
 */

import {
  applyStaticTranslations,
  Locale,
  onLocaleChange,
  setLocale,
  syncSwitcher,
  t,
} from "./i18n.js";
import { createMap3D, Map3D } from "./map3d.js";
import { setupDebugTools, updateDebugTools } from "./debug-tools.js";
import {
  DebugPanel,
  EntityDesc,
  FrameProfileReport,
  HousePopulation,
  LogBatch,
  LogRecord,
  PathBatchStats,
  ProcessStats,
  RenderTimingReport,
  ReportPoint,
  SCHEMA_VERSION,
  SelectedDebug,
  TelemetrySnapshot,
  TypeCount,
  TimingSample,
} from "./types.js";

/** Chart window: the ring keeps the last N samples. */
const CHART_WINDOW = 240;

/** The four views the header tabs pick between. */
type AppView = "wall" | "detail" | "report" | "logs";

let map3d: Map3D | null = null;
/** Latest snapshot, kept so a language switch can re-render every panel. */
let lastSnapshot: TelemetrySnapshot | null = null;
/** The authenticated telemetry socket; commands require a separate capability. */
let socket: WebSocket | null = null;

// ---------------------------------------------------------------- map

/** Latched once WebGL proves unavailable, so a retry on every snapshot does not
 *  rebuild a renderer that cannot work or append the notice repeatedly. */
let map3dUnavailable = false;

function ensureMap3D(): Map3D | null {
  if (map3d || map3dUnavailable) return map3d;
  const canvas = document.getElementById("map3d") as HTMLCanvasElement | null;
  if (!canvas) return null;
  const callouts = document.getElementById("map-callouts");
  map3d = createMap3D(canvas, callouts);
  if (!map3d) {
    map3dUnavailable = true;
    const note = document.createElement("div");
    note.className = "map-fallback";
    note.textContent = t("map.noWebGL");
    canvas.parentElement?.append(note);
  }
  return map3d;
}

function drawMapLegend(snapshot: TelemetrySnapshot): void {
  const legend = document.getElementById("map-legend");
  if (!legend) return;
  const palette = ["#8a94a3", "#b04aff", "#e8c34a", "#37e0b8", "#ff5555", "#4a9eff"];
  legend.textContent = "";
  snapshot.houses.forEach((house, index) => {
    const swatch = document.createElement("span");
    const dot = document.createElement("i");
    const color = house.color;
    dot.style.background = color
      ? `rgb(${color[0]},${color[1]},${color[2]})`
      : palette[index % palette.length];
    swatch.append(dot, document.createTextNode(house.name));
    legend.append(swatch);
  });
}

// ---------------------------------------------------------------- chart

const chartSamples: { tick: number; p99: number }[] = [];

function drawChart(timing: TimingSample | null): void {
  const canvas = document.getElementById("chart") as HTMLCanvasElement;
  const dpr = Math.min(window.devicePixelRatio, 2);
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  if (canvas.width !== width * dpr || canvas.height !== height * dpr) {
    canvas.width = width * dpr;
    canvas.height = height * dpr;
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);

  const budget = timing?.budget_ms ?? null;
  const peak = Math.max((budget ?? 0) * 1.4, ...chartSamples.map((s) => s.p99), 1);
  const y = (ms: number) => height - 4 - (ms / peak) * (height - 10);

  // Palette matches the tech-wall chrome: cyan traces, amber budget.
  ctx.strokeStyle = "rgba(0, 190, 255, 0.10)";
  ctx.lineWidth = 1;
  for (let fraction = 0; fraction <= 1; fraction += 0.25) {
    const gy = 4 + fraction * (height - 10);
    ctx.beginPath();
    ctx.moveTo(0, gy);
    ctx.lineTo(width, gy);
    ctx.stroke();
  }

  if (budget !== null) {
    ctx.save();
    ctx.strokeStyle = "#ffb648";
    ctx.shadowColor = "rgba(255, 182, 72, 0.8)";
    ctx.shadowBlur = 6;
    ctx.setLineDash([4, 4]);
    ctx.beginPath();
    ctx.moveTo(0, y(budget));
    ctx.lineTo(width, y(budget));
    ctx.stroke();
    ctx.restore();
    ctx.setLineDash([]);
    ctx.fillStyle = "#ffb648";
    ctx.font = "10px monospace";
    ctx.fillText(t("chart.budget", { ms: budget.toFixed(1) }), 6, y(budget) - 3);
  }

  if (chartSamples.length > 1) {
    const step = width / (CHART_WINDOW - 1);
    const base = width - (chartSamples.length - 1) * step;
    ctx.save();
    ctx.strokeStyle = "#28f0ff";
    ctx.lineWidth = 1.6;
    ctx.shadowColor = "rgba(40, 240, 255, 0.75)";
    ctx.shadowBlur = 8;
    ctx.beginPath();
    chartSamples.forEach((sample, index) => {
      const px = base + index * step;
      const py = y(sample.tick);
      if (index === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    });
    ctx.stroke();
    ctx.restore();
    ctx.strokeStyle = "rgba(255, 90, 110, 0.55)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    chartSamples.forEach((sample, index) => {
      const px = base + index * step;
      const py = y(sample.p99);
      if (index === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    });
    ctx.stroke();
  }
}

// ---------------------------------------------------------------- panels

function fmt(value: number, digits = 2): string {
  return value.toFixed(digits);
}

/** Group thousands so a long match's tick count stays readable. Both callers
 *  pass counts (ticks, seconds), so rounding first is honest. */
function thousands(value: number): string {
  return Math.round(value).toLocaleString("en-US");
}

/** House names come from the rules INI, so they are text, not markup — escape
 *  before any of them reaches an `innerHTML` template. */
function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function budgetClass(utilization: number): string {
  if (utilization >= 1) return "bad";
  if (utilization >= 0.7) return "warn";
  return "good";
}

/** One `label: value` row of a key/value panel. */
function kv(label: string, value: string, cls?: string): HTMLDivElement {
  const row = document.createElement("div");
  row.className = "kv";
  const key = document.createElement("span");
  key.className = "k";
  key.textContent = label;
  const val = document.createElement("span");
  val.className = cls ? `v ${cls}` : "v";
  val.textContent = value;
  row.append(key, val);
  return row;
}

function updateHeader(snapshot: TelemetrySnapshot): void {
  document.getElementById("version")!.textContent = String(snapshot.version);
  document.getElementById("tick")!.textContent = String(snapshot.tick);
  document.getElementById("entities")!.textContent = String(snapshot.entities_total);
  document.getElementById("mapsize")!.textContent =
    `${snapshot.map_width}x${snapshot.map_height}`;
}

function updateCost(snapshot: TelemetrySnapshot): void {
  const timing = snapshot.timing;
  document.getElementById("last")!.textContent = timing ? fmt(timing.tick_ms) : "-";
  document.getElementById("p50")!.textContent = timing ? fmt(timing.p50_ms) : "-";
  document.getElementById("p99")!.textContent = timing ? fmt(timing.p99_ms) : "-";
  const budgetCell = document.getElementById("budget")!;
  if (timing && timing.budget_utilization !== null) {
    budgetCell.textContent = `${Math.round(timing.budget_utilization * 100)}`;
    budgetCell.className = `value ${budgetClass(timing.budget_utilization)}`;
    budgetCell.title = "";
  } else {
    budgetCell.textContent = "—";
    budgetCell.className = "value";
    budgetCell.title = timing ? t("cost.unbounded") : "";
  }
  // Two different clocks: presentation frames vs committed sim ticks. While
  // paused the tick rate is a true 0, not missing data — the hyphen is
  // reserved for "no sample at all".
  document.getElementById("fps")!.textContent = timing
    ? `${Math.round(timing.fps)}`
    : "-";
  document.getElementById("tickhz")!.textContent = timing
    ? fmt(timing.tick_hz, 1)
    : "-";
}

/** Terrain inspection data from the snapshot's debug block. */
function updateTerrain(debug: DebugPanel | null): void {
  const panel = document.getElementById("terrain")!;
  panel.textContent = "";
  if (!debug) {
    // The synthetic demo streams population but no cursor/selection capture;
    // the real game fills the whole panel.
    panel.append(kv(t("terr.status"), t("terr.noData"), "dim"));
    return;
  }
  panel.append(
    kv(t("terr.grid"), debug.grid ? `${debug.grid[0]}x${debug.grid[1]}` : t("terr.none")),
    kv(t("terr.overlay"), debug.active_speed_type),
    kv(
      t("terr.vxl"),
      debug.vxl_sprites === null ? "-" : `${debug.vxl_sprites}`,
    ),
  );
  const cursor = debug.cursor_cell;
  panel.append(
    kv(
      t("terr.cursor"),
      cursor ? `(${cursor[0]}, ${cursor[1]})  z=${debug.cursor_elevation}` : t("terr.offMap"),
    ),
    kv(
      t("terr.pathgrid"),
      debug.cursor_walkable === null
        ? "-"
        : debug.cursor_walkable
          ? t("terr.walkable")
          : t("terr.blocked"),
      debug.cursor_walkable === false ? "bad" : debug.cursor_walkable ? "good" : undefined,
    ),
  );
  if (debug.land_rows.length > 0) {
    const rows = debug.land_rows
      .map(
        (row) =>
          `${row.speed_type} ${row.cost === 0 ? t("terr.blocked") : `row=${row.cost}%`}`,
      )
      .join("   ");
    panel.append(kv(t("terr.landRows"), rows));
  }
  if (debug.footprint) {
    panel.append(kv(t("terr.footprint"), debug.footprint, "warn"));
  }

  const at = document.createElement("div");
  at.className = "sub";
  at.textContent = t("terr.atCursor", { n: debug.entities_at_cursor.length });
  panel.append(at);
  if (debug.entities_at_cursor.length === 0) {
    panel.append(kv("", t("terr.none"), "dim"));
  }
  for (const entity of debug.entities_at_cursor) {
    panel.append(kv("", describeEntity(entity), "mono"));
  }

  const sel = document.createElement("div");
  sel.className = "sub";
  sel.textContent = t("terr.selected", { n: debug.selected.length });
  panel.append(sel);
  if (debug.selected.length === 0) {
    panel.append(kv("", t("terr.none"), "dim"));
  }
  for (const selected of debug.selected) {
    panel.append(...selectedRows(selected));
  }
}

function describeEntity(entity: EntityDesc): string {
  return `${entity.type_name}(${entity.category}) [${entity.owner}] hp=${entity.hp}/${entity.max_hp}`;
}

/** One selected entity's rows: identity, path and miner state. */
function selectedRows(selected: SelectedDebug): HTMLDivElement[] {
  const rows: HTMLDivElement[] = [
    kv(
      "",
      `${selected.type_name} @ (${selected.cell[0]},${selected.cell[1]}) sub=(${selected.sub[0]},${selected.sub[1]})`,
      "mono",
    ),
  ];
  if (selected.moving) {
    rows.push(
      kv(
        t("sel.path"),
        t("sel.stepsLeft", { n: selected.path_steps_left, b: String(selected.path_blocked) }),
        selected.path_blocked ? "warn" : undefined,
      ),
    );
    if (selected.goal) {
      rows.push(kv(t("sel.goal"), `(${selected.goal[0]},${selected.goal[1]})`, "mono"));
    }
    for (const step of selected.path_preview) {
      rows.push(
        kv(
          "",
          `${step.first ? ">" : " "}[${step.cell[0]},${step.cell[1]}]${step.walkable ? "" : ` ${t("terr.blocked")}`}`,
          step.walkable ? "mono" : "mono bad",
        ),
      );
    }
  } else {
    rows.push(kv(t("sel.path"), t("sel.idle"), "dim"));
  }
  if (selected.miner) {
    const miner = selected.miner;
    rows.push(
      kv(t("sel.miner"), `${miner.kind} — ${miner.state}`),
      kv(t("sel.cargo"), `${miner.cargo}/${miner.capacity}`),
      kv(
        t("sel.stage"),
        `${miner.stage} (rate ${miner.rate}) ${t("sel.harvesting")}: ${miner.harvesting}`,
      ),
    );
    if (miner.archived_ore) {
      rows.push(kv(t("sel.ore"), `(${miner.archived_ore[0]},${miner.archived_ore[1]})`, "mono"));
    }
    rows.push(
      kv(
        t("sel.movement"),
        `${selected.moving ? t("sel.active") : t("sel.idleWord")} · ${t("sel.teleport")} ${miner.teleport_active ? t("sel.active") : t("sel.idleWord")}`,
      ),
    );
  }
  return rows;
}

/** Rule and telemetry strings are text even when they contain markup. */
function renderStatGrid(container: HTMLElement, cells: [string, string][]): void {
  const rows = cells.map(([key, value]) => {
    const row = document.createElement("div");
    const label = document.createElement("span");
    label.className = "k";
    label.textContent = key;
    const text = document.createElement("b");
    text.textContent = value;
    row.append(label, text);
    return row;
  });
  container.replaceChildren(...rows);
}

function renderHouseTags(container: HTMLElement, house: HousePopulation): void {
  const tags: HTMLElement[] = [];
  const tag = (className: string, label: string): void => {
    const node = document.createElement("span");
    node.className = `tag ${className}`;
    node.textContent = label;
    tags.push(node);
  };
  if (house.is_human) tag("human", t("tag.human"));
  else {
    const difficulty = ["easy", "normal", "hard"].includes(house.difficulty)
      ? house.difficulty
      : "normal";
    tag(`ai ${difficulty}`, t(`diff.${difficulty}`));
  }
  if (isPassiveHouse(house)) tag("passive", t("dp.passive"));
  if (house.defeated) tag("dead", t("dp.defeated"));
  container.replaceChildren(...tags);
}

/** A house's side, from the loaded rules' own `[Sides]` key.
 *
 * The numeric index is only a position in that section (retail order is not
 * the test-fixture order), so the engine spelling is authoritative; known
 * names are localised, unknown ones are shown as-is. */
function sideName(house: HousePopulation): string {
  const known: Record<string, string> = {
    allied: "side.allied",
    allies: "side.allied",
    gdi: "side.allied",
    soviet: "side.soviet",
    soviets: "side.soviet",
    nod: "side.soviet",
    thirdside: "side.yuri",
    yuri: "side.yuri",
    yuricountry: "side.yuri",
    civilian: "side.civilian",
    civilians: "side.civilian",
    mutant: "side.mutant",
  };
  const raw = house.side_name ?? "";
  const key = known[raw.toLowerCase()];
  if (key) return t(key);
  return raw || t("side.unknown", { n: house.side_index });
}

function updatePopulation(snapshot: TelemetrySnapshot): void {
  const population = document.getElementById("population")!;
  population.textContent = "";
  for (const house of snapshot.houses) {
    const counts = house.counts;
    const total = counts.unit + counts.infantry + counts.aircraft + counts.structure;
    const row = document.createElement("div");
    row.className = "pop-row";
    const segments: [number, string][] = [
      [counts.infantry, "#3f8f6f"],
      [counts.unit, "#2f6fb0"],
      [counts.aircraft, "#8a5fc0"],
      [counts.structure, "#b0763a"],
    ];
    const bar = document.createElement("div");
    bar.className = "bar";
    for (const [value, css] of segments) {
      if (value <= 0) continue;
      const segment = document.createElement("div");
      segment.className = "seg";
      segment.style.width = `${(value / Math.max(total, 1)) * 100}%`;
      segment.style.background = css;
      bar.append(segment);
    }
    // The dot repeats the battlefield's team colour, so a row and its units
    // read as the same house at a glance. Segments stay category-coloured:
    // they encode composition, which is this panel's whole point.
    const dot = document.createElement("i");
    dot.className = "house-dot";
    const css = house.color
      ? `rgb(${house.color[0]},${house.color[1]},${house.color[2]})`
      : "#6b7a8c";
    dot.style.background = css;
    dot.style.boxShadow = `0 0 8px ${css}`;
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = house.name;
    name.title = house.name;
    const totalCell = document.createElement("span");
    totalCell.className = "total";
    totalCell.textContent = String(total);
    row.append(dot, name, bar, totalCell);
    population.append(row);
  }
}

/** A measured phase within one sample population: simulation ticks or renders.
 *  The two populations have independent clocks, totals and sample counts. */
interface Slice {
  key: string;
  ms: number;
  color: string;
}

const PHASE_COLORS: Record<string, string> = {
  objects: "#2f6fb0",
  tail: "#d94f5c",
  vision: "#4fb8a8",
  logic: "#3f9f5f",
  unmarked: "#9a9a9a",
};

/** Render one pie: inline SVG wedges with in-slice percentages, plus a
 *  legend carrying the millisecond values. */
function drawPie(
  canvasId: string,
  legendId: string,
  slices: Slice[],
  total: number,
): void {
  const canvas = document.getElementById(canvasId);
  const legend = document.getElementById(legendId);
  if (!canvas || !legend) return;
  canvas.textContent = "";
  legend.textContent = "";

  const size = 132;
  const center = size / 2;
  const radius = size / 2 - 2;
  /** A ring rather than a full disc: the hollow centre carries the headline
   *  number, and a band shows a 6% wedge beside an 80% one far better than two
   *  wedges sharing a point at the middle. */
  const inner = radius * 0.58;
  /** Angular gap between wedges, applied only where the wedge is wide enough to
   *  spare it, so a sliver is never eaten by its own separator. */
  const gap = 1.8;
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", `0 0 ${size} ${size}`);
  /** `scale` shrinks the radius: 1 is the outer arc, `inner / radius` the hole. */
  const polar = (degrees: number, scale = 1): [number, number] => {
    const radians = (degrees * Math.PI) / 180;
    const reach = radius * scale;
    return [center + reach * Math.cos(radians), center + reach * Math.sin(radians)];
  };
  let angle = -90;
  let drew = false;
  let dominant = 0;
  for (const slice of slices) {
    if (slice.ms <= 0 || total <= 0) continue;
    drew = true;
    const fraction = Math.min(1, slice.ms / total);
    dominant = Math.max(dominant, fraction);
    const sweep = fraction * 360;
    const pad = sweep > gap * 3 ? gap / 2 : 0;
    const a0 = angle + pad;
    const a1 = angle + sweep - pad;
    const arc = a1 - a0 > 180 ? 1 : 0;
    const [ox0, oy0] = polar(a0);
    const [ox1, oy1] = polar(a1);
    const [ix1, iy1] = polar(a1, inner / radius);
    const [ix0, iy0] = polar(a0, inner / radius);
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute(
      "d",
      `M ${ox0} ${oy0} A ${radius} ${radius} 0 ${arc} 1 ${ox1} ${oy1}` +
        ` L ${ix1} ${iy1} A ${inner} ${inner} 0 ${arc} 0 ${ix0} ${iy0} Z`,
    );
    path.setAttribute("fill", slice.color);
    path.setAttribute("stroke", "rgba(4, 8, 20, 0.5)");
    path.setAttribute("stroke-width", "0.6");
    const title = document.createElementNS("http://www.w3.org/2000/svg", "title");
    title.textContent = `${t(slice.key)} ${fmt(slice.ms, 2)} ms`;
    path.append(title);
    svg.append(path);
    // Only label wedges wide enough to hold the text: a sliver's label would
    // spill across its neighbours, and the legend already carries the number.
    if (fraction >= 0.05) {
      // The band's own midline, so the text sits inside the wedge it names.
      const [lx, ly] = polar((a0 + a1) / 2, (inner / radius + 1) / 2);
      const label = document.createElementNS("http://www.w3.org/2000/svg", "text");
      label.setAttribute("x", String(lx.toFixed(2)));
      label.setAttribute("y", String(ly.toFixed(2)));
      label.setAttribute("text-anchor", "middle");
      label.setAttribute("dominant-baseline", "central");
      label.setAttribute("font-size", "10.5");
      label.setAttribute("font-weight", "600");
      label.setAttribute("fill", "#ffffff");
      // A dark halo rather than a colour gamble: the wedge palette comes from
      // the engine, so both a pale and a saturated fill must stay readable.
      label.setAttribute("stroke", "rgba(4, 8, 20, 0.6)");
      label.setAttribute("stroke-width", "2.6");
      label.setAttribute("paint-order", "stroke");
      label.setAttribute("pointer-events", "none");
      label.textContent = `${fmt(fraction * 100, 0)}%`;
      svg.append(label);
    }
    angle += sweep;
  }
  if (drew) {
    // The dominant share stays within this pie's sample population. Simulation
    // tick costs and measured render costs are never added or divided together.
    const head = document.createElementNS("http://www.w3.org/2000/svg", "text");
    head.setAttribute("x", String(center));
    head.setAttribute("y", String(center));
    head.setAttribute("text-anchor", "middle");
    head.setAttribute("dominant-baseline", "central");
    head.setAttribute("class", "pie-dominant");
    head.textContent = `${fmt(dominant * 100, 0)}%`;
    svg.append(head);
  }
  // An empty pie would still be a 200px box on the report wall; leave the
  // container empty instead so the panel collapses to its text.
  if (drew) canvas.append(svg);

  for (const slice of slices) {
    const row = document.createElement("li");
    const swatch = document.createElement("span");
    swatch.className = "sw";
    swatch.style.background = slice.color;
    swatch.style.color = slice.color;
    const label = document.createElement("span");
    label.className = "lb";
    label.textContent = t(slice.key);
    const value = document.createElement("span");
    value.className = "ms";
    const percent = total > 0 ? (slice.ms / total) * 100 : 0;
    value.textContent = `${fmt(slice.ms, 2)} ms · ${fmt(percent, 1)}%`;
    row.append(swatch, label, value);
    legend.append(row);
  }
}

/** Build each distribution from its own measured population. Render spans do
 *  not include a guessed remainder from FPS, and never share a denominator with
 *  simulation ticks. These distributions also feed both export formats. */
function buildTimingSlices(
  profile: FrameProfileReport | null,
  render: RenderTimingReport | null,
): { sim: Slice[]; render: Slice[]; simTotal: number; renderTotal: number } {
  const sim: Slice[] = profile && profile.frames > 0 ? [
    { key: "rep.objects", ms: profile.objects_ms, color: PHASE_COLORS.objects },
    { key: "rep.tail", ms: profile.tail_ms, color: PHASE_COLORS.tail },
    { key: "rep.vision", ms: profile.vision_ms, color: PHASE_COLORS.vision },
    { key: "rep.logic", ms: profile.logic_ms, color: PHASE_COLORS.logic },
  ] : [];
  if (profile && profile.frames > 0 && profile.unmarked_ms > 0) {
    sim.push({ key: "rep.unmarked", ms: profile.unmarked_ms, color: PHASE_COLORS.unmarked });
  }
  const rendered: Slice[] = render && render.frames > 0 ? [
    { key: "rnd.world", ms: render.world_ms, color: "#e08b2a" },
    { key: "rnd.debug", ms: render.debug_ms, color: "#a884ff" },
    { key: "rnd.shroud", ms: render.shroud_ms, color: "#4fb8a8" },
    { key: "rnd.ui", ms: render.ui_ms, color: "#2f6fb0" },
    { key: "rnd.sidebar", ms: render.sidebar_ms, color: "#3f9f5f" },
    { key: "rnd.upload", ms: render.upload_ms, color: "#d94f5c" },
    { key: "rnd.other", ms: render.other_ms, color: "#9a9a9a" },
  ] : [];
  return {
    sim,
    render: rendered,
    simTotal: sim.reduce((sum, slice) => sum + slice.ms, 0),
    renderTotal: rendered.reduce((sum, slice) => sum + slice.ms, 0),
  };
}

/** A zero-duration measurement is valid; no samples is unknown. */
function measuredCost(total: number, samples: number): string {
  return samples > 0 ? `${fmt(total, 2)} ms` : "—";
}

/** Per-subsystem ledger keys, in the engine's `PHASE_KEYS` order (schema v13).
 *  The report table lists them in full; the pies only sum four slices of them. */
const PHASE_KEYS = [
  "commands_ingress",
  "commands_admit",
  "triggers",
  "shroud",
  "lighting",
  "ore_growth",
  "bombs",
  "superweapons",
  "radiation",
  "team_ai",
  "objects",
  "wall_crush",
  "wakes",
  "vision",
  "combat",
  "passengers",
  "ion_blasts",
  "crates",
  "production",
  "docks",
  "spawn_fog",
  "late_ai",
  "overlays",
  "debug_asserts",
  "anims",
  "score",
  "hash",
] as const;

/** A phase's milliseconds: three decimals while it is under a tenth, so the
 *  small ones do not all collapse into `0.00`. */
function fmtPhase(ms: number): string {
  if (ms <= 0) return "—";
  if (ms < 0.01) return "<0.01";
  if (ms < 0.1) return fmt(ms, 3);
  return fmt(ms, 2);
}

/** Independent simulation and measured-render ledgers. Percentages are only
 *  within their own group; no whole-frame total or inferred idle cost exists. */
function renderPhaseTable(
  table: HTMLElement | null,
  profile: FrameProfileReport | null,
  render: RenderTimingReport | null,
  batch: PathBatchStats | null,
): void {
  if (!table) return;
  const scroller = table.closest(".table-wrap") as HTMLElement | null;
  const scrollTop = scroller?.scrollTop ?? 0;
  const { render: rendered, simTotal, renderTotal } = buildTimingSlices(profile, render);
  const share = (ms: number, total: number): number =>
    total > 0 ? Math.min(100, (ms / total) * 100) : 0;
  const rowsFor = (
    rows: { label: string; ms: number }[], total: number, kind: "sim" | "render",
  ): string => rows.map(({ label, ms }) => {
    const percent = share(ms, total);
    const text = ms > 0 && total > 0 ? `${fmt(percent, 1)}%` : "—";
    return `<tr><td class="ph-name">${escapeHtml(label)}</td>` +
      `<td class="ph-bar" style="--share:${percent.toFixed(2)}%"><span class="ph-fill"></span></td>` +
      `<td class="ph-ms"><b>${escapeHtml(fmtPhase(ms))}</b></td>` +
      `<td class="ph-pct">${kind === "sim" ? text : "—"}</td>` +
      `<td class="ph-pct">${kind === "render" ? text : "—"}</td></tr>`;
  }).join("");
  const totalRow = (label: string, total: number, kind: "sim" | "render"): string =>
    `<tr class="ph-total"><td class="ph-name">${escapeHtml(label)}</td><td class="ph-bar"></td>` +
    `<td class="ph-ms"><b>${escapeHtml(fmtPhase(total))}</b></td>` +
    `<td class="ph-pct">${kind === "sim" && total > 0 ? "100%" : "—"}</td>` +
    `<td class="ph-pct">${kind === "render" && total > 0 ? "100%" : "—"}</td></tr>`;
  const countRow = (label: string, value: string): string =>
    `<tr><td class="ph-name">${escapeHtml(label)}</td><td class="ph-bar"></td>` +
    `<td class="ph-ms"><b>${escapeHtml(value)}</b></td>` +
    `<td class="ph-pct">—</td><td class="ph-pct">—</td></tr>`;
  let body = "";
  if (profile && profile.frames > 0) {
    body += `<tbody><tr class="ph-group"><td colspan="5">${escapeHtml(t("rep.simTick"))}` +
      ` · ${thousands(profile.frames)} ${escapeHtml(t("rep.frames"))}</td></tr>`;
    const values = profile.phases_ms;
    if (values.length === PHASE_KEYS.length) {
      const rows = PHASE_KEYS.map((key, index) => ({
        label: t(`phase.${key}`), ms: values[index] ?? 0, order: index,
      }));
      rows.push({ label: t("rep.unmarked"), ms: profile.unmarked_ms, order: PHASE_KEYS.length });
      rows.sort((a, b) => a.ms < 0.01 && b.ms < 0.01 ? a.order - b.order : b.ms - a.ms);
      body += rowsFor(rows, simTotal, "sim");
      body += totalRow(t("rep.phaseTotal"), simTotal, "sim");
    } else {
      body += `<tr><td colspan="5">phase ledger mismatch: ${values.length} phases on the wire, ` +
        `${PHASE_KEYS.length} expected</td></tr>`;
    }
    body += "</tbody>";
  }
  if (batch && batch.samples > 0) {
    body += `<tbody><tr class="ph-group"><td colspan="5">${escapeHtml(t("rep.pathBatch"))}` +
      ` · ${thousands(batch.samples)} ${escapeHtml(t("rep.pathPasses"))}</td></tr>`;
    body += rowsFor([
      { label: t("rep.pathMean"), ms: batch.mean_ms },
      { label: t("rep.pathPeak"), ms: batch.peak_ms },
    ], simTotal, "sim");
    body += countRow(t("rep.pathSearched"), `${fmt(batch.searched, 0)} / ${fmt(batch.waiting, 0)}`);
    body += countRow(t("rep.pathLeftOut"), fmt(batch.left_out, 0));
    body += countRow(t("rep.pathSteps"), fmt(batch.steps, 0));
    body += "</tbody>";
  }
  if (render && render.frames > 0) {
    body += `<tbody><tr class="ph-group"><td colspan="5">${escapeHtml(t("rep.renderSample"))}` +
      ` · ${thousands(render.frames)} ${escapeHtml(t("rep.renders"))}</td></tr>`;
    body += rowsFor(rendered.map((slice) => ({ label: t(slice.key), ms: slice.ms }))
      .sort((a, b) => b.ms - a.ms), renderTotal, "render");
    body += totalRow(t("rep.renderSample"), renderTotal, "render") + "</tbody>";
  }
  table.innerHTML = body ?
    `<thead><tr><th>${t("th.phase")}</th><th class="ph-bar-col"></th><th>${t("th.ms")}</th>` +
    `<th>${t("th.shareSim")}</th><th>${t("th.shareRender")}</th></tr></thead>${body}` : "";
  if (scroller) scroller.scrollTop = scrollTop;
}

const EXPORT_FONT = "'Microsoft YaHei', 'PingFang SC', 'Segoe UI', sans-serif";

/** One design token off `:root`, or the fallback it was introduced with. */
function token(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

/** `#rrggbb` at reduced strength — canvas has no `color-mix()` to soften a token. */
function soften(color: string, alpha: number): string {
  const hex = /^#([\da-f]{2})([\da-f]{2})([\da-f]{2})$/i.exec(color);
  if (!hex) return color;
  const [r, g, b] = [1, 2, 3].map((group) => parseInt(hex[group], 16));
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/**
 * The export canvases cannot inherit the wall's stylesheet, so they read the same
 * tokens off `:root` instead of carrying a second copy of the palette. That copy
 * is exactly why a generated PNG kept the old cyan wall after the glass restyle.
 */
interface ExportPalette {
  top: string;
  bottom: string;
  title: string;
  strong: string;
  ink: string;
  dim: string;
  accent: string;
  violet: string;
}

function exportPalette(): ExportPalette {
  return {
    top: token("--bg-hi", "#101d3e"),
    bottom: token("--bg", "#070b18"),
    title: token("--title", "#9fd8ff"),
    strong: token("--strong", "#f0f6ff"),
    ink: token("--text", "#dce7f8"),
    dim: token("--dim", "#8ea6c6"),
    accent: token("--cyan", "#6fb7ff"),
    violet: token("--blue", "#a884ff"),
  };
}

/**
 * The whole ledger as a canvas: every row, including the ones the panel scrolls
 * out of view. Drawn by hand rather than via an HTML-to-image library so the
 * numbers, bars and rules land on exact pixel columns at any row count.
 */
/**
 * Exactly what the match report reads. `TelemetrySnapshot` and a stored
 * `ReportPoint` both satisfy it, so one renderer serves the live view and a
 * past point.
 */
interface ReportView {
  tick: number;
  entities_total: number;
  map_width: number;
  map_height: number;
  timing: TimingSample | null;
  profile: FrameProfileReport | null;
  render: RenderTimingReport | null;
  path_batch: PathBatchStats | null;
  houses: {
    name: string;
    is_human: boolean;
    passive: boolean;
    counts: { structure: number };
    credits: number;
    score: number;
  }[];
}

function reportViewOf(point: ReportPoint): ReportView {
  return {
    tick: point.tick,
    entities_total: point.entities_total,
    map_width: point.map_width,
    map_height: point.map_height,
    timing: point.timing,
    profile: point.profile,
    render: point.render,
    path_batch: point.path_batch,
    houses: point.houses.map((house) => ({
      name: house.name,
      is_human: house.is_human,
      passive: house.passive,
      counts: { structure: house.structures },
      credits: house.credits,
      score: house.score,
    })),
  };
}

interface LedgerRow {
  name: string;
  ms: string;
  sim: string;
  render: string;
  bar: number;
  group: boolean;
  foot: boolean;
}

/** Read a rendered ledger table's rows. One formatter fills the table, so the
 *  live report and a stored point cannot list different rows. */
function readRows(table: HTMLElement): LedgerRow[] {
  const rows: LedgerRow[] = [];
  table.querySelectorAll("tr").forEach((tr) => {
    if (tr.closest("thead")) return;
    const cells = tr.querySelectorAll("td");
    if (cells.length === 0) return;
    const barCell = tr.querySelector("td.ph-bar") as HTMLElement | null;
    const bar = barCell ? Number.parseFloat(barCell.style.getPropertyValue("--share")) : 0;
    const offset = cells.length === 1 ? 0 : 1;
    rows.push({
      name: (cells[0]?.textContent ?? "").trim(),
      ms: (cells[1 + offset]?.textContent ?? "").trim(),
      sim: (cells[2 + offset]?.textContent ?? "").trim(),
      render: (cells[3 + offset]?.textContent ?? "").trim(),
      bar: Number.isFinite(bar) ? bar : 0,
      group: tr.classList.contains("ph-group"),
      foot: tr.classList.contains("ph-total"),
    });
  });
  return rows;
}

/** The ledger for a view, rendered into a detached table. */
function ledgerRowsFor(view: ReportView): LedgerRow[] {
  const table = document.createElement("table");
  renderPhaseTable(table, view.profile, view.render, view.path_batch);
  return readRows(table);
}

function buildPhaseLedgerCanvas(): HTMLCanvasElement | null {
  if (!lastSnapshot) return null;
  const rows = ledgerRowsFor(lastSnapshot);
  if (rows.length === 0) return null;

  const dpr = Math.min(3, Math.max(2, window.devicePixelRatio || 1));
  const width = 900;
  const pad = 22;
  const barX = 300;
  const barW = 240;
  const msRight = 650;
  const simRight = 780;
  const renderRight = width - pad;
  const titleH = 54;
  const headH = 24;
  const rowH = 23;
  const groupH = 30;
  const noteH = 28;
  const height =
    titleH + headH + rows.reduce((sum, row) => sum + (row.group ? groupH : rowH), 0) + noteH;

  const canvas = document.createElement("canvas");
  canvas.width = Math.round(width * dpr);
  canvas.height = Math.round(height * dpr);
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.scale(dpr, dpr);
  ctx.textBaseline = "middle";

  const pal = exportPalette();
  const background = ctx.createLinearGradient(0, 0, 0, height);
  background.addColorStop(0, pal.top);
  background.addColorStop(1, pal.bottom);
  ctx.fillStyle = background;
  ctx.fillRect(0, 0, width, height);

  const rule = (y: number, color: string, weight = 1): void => {
    ctx.strokeStyle = color;
    ctx.lineWidth = weight;
    ctx.beginPath();
    ctx.moveTo(pad, y);
    ctx.lineTo(width - pad, y);
    ctx.stroke();
  };

  // Title block: what this is, and which tick it was measured on.
  ctx.textAlign = "left";
  ctx.fillStyle = pal.title;
  ctx.font = `600 15px ${EXPORT_FONT}`;
  ctx.fillText(t("rep.phases"), pad, 24);
  ctx.fillStyle = pal.dim;
  ctx.font = `11.5px ${EXPORT_FONT}`;
  ctx.fillText(
    `${t("rep.tickNow")} ${thousands(lastSnapshot?.tick ?? 0)} · ${t("rep.phaseHint")}`,
    pad,
    43,
  );

  let y = titleH;
  ctx.fillStyle = pal.dim;
  ctx.font = `600 10px ${EXPORT_FONT}`;
  ctx.textAlign = "left";
  ctx.fillText(t("th.phase"), pad, y + headH / 2);
  ctx.textAlign = "right";
  ctx.fillText(t("th.ms").toUpperCase(), msRight, y + headH / 2);
  ctx.fillText(t("th.shareSim").toUpperCase(), simRight, y + headH / 2);
  ctx.fillText(t("th.shareRender").toUpperCase(), renderRight, y + headH / 2);
  y += headH;
  rule(y, soften(pal.accent, 0.34));

  for (const row of rows) {
    const h = row.group ? groupH : rowH;
    const mid = y + h / 2;

    if (row.group) {
      // Group header: a rule above and the label sitting on the lower edge.
      rule(y, soften(pal.accent, 0.24));
      ctx.textAlign = "left";
      ctx.fillStyle = pal.dim;
      ctx.font = `600 11px ${EXPORT_FONT}`;
      ctx.fillText(row.name, pad, y + groupH - 8);
      y += h;
      continue;
    }

    // No minimum width: a phase at 0.01 % should leave no bar rather than a
    // row of identical 2 px stubs. The fill matches `.ph-fill` in the wall.
    const barWidth = (row.bar / 100) * barW;
    if (barWidth >= 1) {
      const barPaint = ctx.createLinearGradient(barX, 0, barX + barW, 0);
      barPaint.addColorStop(0, soften(pal.accent, 0.5));
      barPaint.addColorStop(1, soften(pal.violet, 0.22));
      ctx.fillStyle = barPaint;
      if (typeof ctx.roundRect === "function") {
        ctx.beginPath();
        ctx.roundRect(barX, y + 4, barWidth, h - 8, 3);
        ctx.fill();
      } else {
        ctx.fillRect(barX, y + 4, barWidth, h - 8);
      }
    }

    ctx.textAlign = "left";
    ctx.font = `${row.foot ? "600 " : ""}12px ${EXPORT_FONT}`;
    ctx.fillStyle = row.foot ? pal.strong : pal.ink;
    ctx.fillText(row.name, pad, mid);

    ctx.textAlign = "right";
    ctx.fillStyle = row.foot ? pal.strong : pal.ink;
    ctx.fillText(row.ms, msRight, mid);
    ctx.fillStyle = pal.dim;
    ctx.fillText(row.sim, simRight, mid);
    ctx.fillText(row.render, renderRight, mid);

    rule(y + h, soften(pal.accent, row.foot ? 0.28 : 0.09));
    y += h;
  }

  ctx.textAlign = "left";
  ctx.font = `11px ${EXPORT_FONT}`;
  ctx.fillStyle = soften(pal.dim, 0.8);
  ctx.fillText(
    `VERA20k · schema v${SCHEMA_VERSION} · ${new Date().toLocaleString()}`,
    pad,
    y + noteH / 2 + 2,
  );

  return canvas;
}

/** The PNG behind the preview, kept so copy and save act on exactly the bytes
 *  the user is looking at. */
let shotBlob: Blob | null = null;
let shotUrl: string | null = null;
let shotTick = 0;

function fitCanvasText(ctx: CanvasRenderingContext2D, text: string, width: number): string {
  if (ctx.measureText(text).width <= width) return text;
  const characters = Array.from(text);
  while (characters.length && ctx.measureText(characters.join("") + "…").width > width) {
    characters.pop();
  }
  return characters.join("") + "…";
}

/** Build the ledger and show it in the preview dialog. Nothing is downloaded
 *  until the user asks — the table is taller than its panel, so seeing the
 *  whole thing is the point. */
/** One pie wedge as an SVG path — the same geometry the canvas pie uses. */
function pieWedge(cx: number, cy: number, r: number, start: number, sweep: number): string {
  const at = (deg: number): [number, number] => {
    const rad = (deg * Math.PI) / 180;
    return [cx + r * Math.cos(rad), cy + r * Math.sin(rad)];
  };
  const [x0, y0] = at(start);
  const [x1, y1] = at(start + sweep);
  if (sweep >= 360) {
    const [oppositeX, oppositeY] = at(start + 180);
    return `M ${cx} ${cy} L ${x0} ${y0} A ${r} ${r} 0 0 1 ${oppositeX} ${oppositeY}` +
      ` A ${r} ${r} 0 0 1 ${x0} ${y0} Z`;
  }
  return (
    `M ${cx} ${cy} L ${x0.toFixed(2)} ${y0.toFixed(2)} ` +
    `A ${r} ${r} 0 ${sweep > 180 ? 1 : 0} 1 ${x1.toFixed(2)} ${y1.toFixed(2)} Z`
  );
}

/** The report's own palette: light, for pasting into a document or an issue. */
const REPORT_INK = "#1d2733";
const REPORT_DIM = "#5b6b7d";
const REPORT_RULE = "#e2e8f0";

/**
 * The whole report as one image: header facts, both pies, the phase ledger and
 * the participants.
 *
 * Light theme on purpose — this is a document to paste into an issue, not a
 * screenshot of the dashboard. The live panel keeps its dark styling.
 */
function buildReportCanvas(view: ReportView | null): HTMLCanvasElement | null {
  const snapshot = view;
  const profile = snapshot?.profile ?? null;
  const render = snapshot?.render ?? null;
  if (!snapshot || !(profile?.frames || render?.frames)) return null;

  const { sim: simSlices, render: renderSlices, simTotal, renderTotal } =
    buildTimingSlices(profile, render);
  const ledger = ledgerRowsFor(snapshot);
  const parties = snapshot.houses.filter((house) => !house.passive);

  const width = 1000;
  const pad = 34;
  const headerH = 142;
  const pieH = 280;
  const tableHeadH = 28;
  const rowH = 23;
  const groupH = 30;
  const ledgerH = ledger.reduce((sum, row) => sum + (row.group ? groupH : rowH), 0);
  const partiesH = parties.length === 0 ? 0 : 48 + parties.length * 26;
  // Leaves room between the last participant row and the colophon.
  const footerH = 52;
  const height = headerH + pieH + tableHeadH + ledgerH + 26 + partiesH + footerH;

  const dpr = Math.min(3, Math.max(2, window.devicePixelRatio || 1));
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(width * dpr);
  canvas.height = Math.round(height * dpr);
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.scale(dpr, dpr);
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";

  ctx.fillStyle = "#f7f9fc";
  ctx.fillRect(0, 0, width, height);

  const rule = (y: number, color = REPORT_RULE, weight = 1): void => {
    ctx.strokeStyle = color;
    ctx.lineWidth = weight;
    ctx.beginPath();
    ctx.moveTo(pad, y);
    ctx.lineTo(width - pad, y);
    ctx.stroke();
  };

  // ---------------------------------------------------------------- header
  ctx.fillStyle = REPORT_INK;
  ctx.font = `700 22px ${EXPORT_FONT}`;
  ctx.fillText(t("rep.title"), pad, 40);
  ctx.fillStyle = REPORT_DIM;
  ctx.font = `12.5px ${EXPORT_FONT}`;
  const tickHz = snapshot.timing?.tick_hz ?? 0;
  ctx.fillText(
    `tick ${thousands(snapshot.tick)} · ` +
      `${t("rep.fps")} ${fmt(snapshot.timing?.fps ?? 0, 0)} fps · ${fmt(tickHz, 1)} tick/s · ` +
      `${t("hdr.map")} ${snapshot.map_width}x${snapshot.map_height}`,
    pad,
    68,
  );
  ctx.fillText(
    `${t("rep.sim")} ${measuredCost(profile?.mean_frame_ms ?? 0, profile?.frames ?? 0)} · ` +
      `${t("rep.peak")} ${measuredCost(profile?.peak_frame_ms ?? 0, profile?.frames ?? 0)}`,
    pad,
    88,
  );
  ctx.font = `10px ${EXPORT_FONT}`;
  ctx.fillText(t("rep.note"), pad, 110, width - pad * 2);
  rule(headerH - 14);

  // ------------------------------------------------------------------ pies
  const radius = 88;
  const paintPie = (
    cx: number,
    cy: number,
    slices: Slice[],
    total: number,
    caption: string,
    samples: number,
    countLabel: string,
  ): void => {
    ctx.textAlign = "left";
    ctx.fillStyle = REPORT_DIM;
    ctx.font = `600 12px ${EXPORT_FONT}`;
    ctx.fillText(`${caption}  ${measuredCost(total, samples)}`, cx - radius, cy - radius - 24);
    ctx.font = `10.5px ${EXPORT_FONT}`;
    ctx.fillText(`${thousands(samples)} ${countLabel}`, cx - radius, cy - radius - 8);

    let angle = -90;
    for (const slice of slices) {
      if (slice.ms <= 0 || total <= 0) continue;
      const sweep = Math.min(360, (slice.ms / total) * 360);
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.arc(cx, cy, radius, (angle * Math.PI) / 180, ((angle + sweep) * Math.PI) / 180);
      ctx.closePath();
      ctx.fillStyle = slice.color;
      ctx.fill();
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 1.5;
      ctx.stroke();
      if (sweep >= 20) {
        const mid = ((angle + sweep / 2) * Math.PI) / 180;
        ctx.fillStyle = "#ffffff";
        ctx.font = `600 10.5px ${EXPORT_FONT}`;
        ctx.textAlign = "center";
        ctx.fillText(
          `${fmt((slice.ms / total) * 100, 0)}%`,
          cx + radius * 0.62 * Math.cos(mid),
          cy + radius * 0.62 * Math.sin(mid),
        );
      }
      angle += sweep;
    }

    // Legend beside the pie, in the same order the wedges were drawn.
    let ly = cy - radius + 6;
    ctx.textAlign = "left";
    for (const slice of slices) {
      ctx.fillStyle = slice.color;
      ctx.fillRect(cx + radius + 22, ly - 4, 9, 9);
      ctx.fillStyle = "#3d4f63";
      ctx.font = `11px ${EXPORT_FONT}`;
      ctx.fillText(
        `${t(slice.key)}  ${fmt(slice.ms, 2)} ms · ${fmt(total > 0 ? (slice.ms / total) * 100 : 0, 1)}%`,
        cx + radius + 37,
        ly,
      );
      ly += 17;
    }
  };

  const pieTop = headerH + 46;
  paintPie(pad + radius + 4, pieTop + radius, renderSlices, renderTotal,
    t("rep.renderSample"), render?.frames ?? 0, t("rep.renders"));
  paintPie(width / 2 + radius + 40, pieTop + radius, simSlices, simTotal,
    t("rep.simTick"), profile?.frames ?? 0, t("rep.frames"));

  // ---------------------------------------------------------------- ledger
  let y = headerH + pieH;
  ctx.fillStyle = REPORT_INK;
  ctx.font = `600 14px ${EXPORT_FONT}`;
  ctx.fillText(t("rep.phases"), pad, y + 8);
  y += 22;

  const nameX = pad;
  const barX = 330;
  const barW = 250;
  const msRight = 690;
  const simRight = 820;
  const renderRight = width - pad;

  ctx.fillStyle = "#6b7b8d";
  ctx.font = `600 10.5px ${EXPORT_FONT}`;
  ctx.textAlign = "left";
  ctx.fillText(t("th.phase"), nameX, y + 10);
  ctx.textAlign = "right";
  ctx.fillText(t("th.ms").toUpperCase(), msRight, y + 10);
  ctx.fillText(t("th.shareSim"), simRight, y + 10);
  ctx.fillText(t("th.shareRender"), renderRight, y + 10);
  y += tableHeadH;
  rule(y, "#cbd5e1");

  for (const row of ledger) {
    const h = row.group ? groupH : rowH;
    const mid = y + h / 2;
    if (row.group) {
      rule(y, "#d8e0ea");
      ctx.textAlign = "left";
      ctx.fillStyle = "#4a5b6e";
      ctx.font = `600 11.5px ${EXPORT_FONT}`;
      ctx.fillText(row.name, nameX, y + groupH - 8);
      y += h;
      continue;
    }
    const barWidth = (row.bar / 100) * barW;
    if (barWidth >= 1) {
      ctx.fillStyle = "rgba(47, 111, 176, 0.18)";
      ctx.fillRect(barX, y + 4, barWidth, h - 8);
    }
    ctx.textAlign = "left";
    ctx.font = `${row.foot ? "600 " : ""}11.5px ${EXPORT_FONT}`;
    ctx.fillStyle = row.foot ? REPORT_INK : "#2f3f52";
    ctx.fillText(row.name, nameX, mid);
    ctx.textAlign = "right";
    ctx.fillText(row.ms, msRight, mid);
    ctx.fillStyle = REPORT_DIM;
    ctx.fillText(row.sim, simRight, mid);
    ctx.fillText(row.render, renderRight, mid);
    rule(y + h, row.foot ? "#cbd5e1" : "#edf1f6");
    y += h;
  }

  // ------------------------------------------------------------ participants
  y += 26;
  if (parties.length > 0) {
    ctx.textAlign = "left";
    ctx.fillStyle = REPORT_INK;
    ctx.font = `600 14px ${EXPORT_FONT}`;
    ctx.fillText(t("rep.houses"), pad, y);
    y += 24;
    ctx.fillStyle = "#6b7b8d";
    ctx.font = `600 10.5px ${EXPORT_FONT}`;
    ctx.fillText(t("th.house"), nameX, y);
    ctx.fillText(t("rep.kind"), 330, y);
    ctx.textAlign = "right";
    ctx.fillText(t("rep.buildings"), 640, y);
    ctx.fillText(t("rep.credits"), 800, y);
    ctx.fillText(t("rep.score"), renderRight, y);
    y += 16;
    rule(y, "#cbd5e1");
    y += 8;
    for (const house of parties) {
      ctx.textAlign = "left";
      ctx.fillStyle = "#2f3f52";
      ctx.font = `11.5px ${EXPORT_FONT}`;
      ctx.fillText(fitCanvasText(ctx, house.name, 306 - nameX), nameX, y + 8);
      ctx.fillText(house.is_human ? t("tag.human") : t("tag.ai"), 330, y + 8);
      ctx.textAlign = "right";
      ctx.fillText(fmt(house.counts.structure, 0), 640, y + 8);
      ctx.fillStyle = REPORT_DIM;
      ctx.fillText(fmt(house.credits, 0), 800, y + 8);
      ctx.fillText(fmt(house.score, 0), renderRight, y + 8);
      y += 26;
    }
  }

  // ---------------------------------------------------------------- footer
  ctx.textAlign = "left";
  ctx.font = `10.5px ${EXPORT_FONT}`;
  ctx.fillStyle = "#8b9aab";
  ctx.fillText(
    `VERA20k · schema v${SCHEMA_VERSION} · ${new Date().toLocaleString()}`,
    pad,
    height - 18,
  );

  return canvas;
}

/**
 * The same report as a self-contained HTML file: inline SVG pies, no scripts,
 * no network — it opens years later and can be attached to a bug report.
 */
function buildReportHtml(view: ReportView | null): string | null {
  const profile = view?.profile ?? null;
  const render = view?.render ?? null;
  if (!view || !(profile?.frames || render?.frames)) return null;

  const { sim: simSlices, render: renderSlices, simTotal, renderTotal } =
    buildTimingSlices(profile, render);
  const ledger = ledgerRowsFor(view);
  const parties = view.houses.filter((house) => !house.passive);
  const tickHz = view.timing?.tick_hz ?? 0;

  const pie = (slices: Slice[], total: number, caption: string, samples: number, countLabel: string): string => {
    const radius = 88;
    const size = 180;
    let angle = -90;
    const wedges = slices
      .filter((slice) => slice.ms > 0 && total > 0)
      .map((slice) => {
        const sweep = Math.min(360, (slice.ms / total) * 360);
        const path = pieWedge(size / 2, size / 2, radius, angle, sweep);
        angle += sweep;
        return `<path d="${path}" fill="${slice.color}" stroke="#fff" stroke-width="1.5"/>`;
      })
      .join("");
    const legend = slices
      .map(
        (slice) =>
          `<li><span class="sw" style="background:${slice.color}"></span>` +
          `<span class="lb">${escapeHtml(t(slice.key))}</span>` +
          `<span class="ms">${fmt(slice.ms, 2)} ms · ` +
          `${fmt(total > 0 ? (slice.ms / total) * 100 : 0, 1)}%</span></li>`,
      )
      .join("");
    return (
      `<figure class="pie"><figcaption>${escapeHtml(caption)} <b>${measuredCost(total, samples)}</b>` +
      ` · ${thousands(samples)} ${escapeHtml(countLabel)}</figcaption>` +
      `<div class="body"><svg viewBox="0 0 ${size} ${size}" width="${size}" height="${size}">${wedges}</svg>` +
      `<ul>${legend}</ul></div></figure>`
    );
  };

  const rows = ledger
    .map((row) =>
      row.group
        ? `<tr class="group"><td colspan="4">${escapeHtml(row.name)}</td></tr>`
        : `<tr class="${row.foot ? "foot" : ""}"><td>${escapeHtml(row.name)}</td>` +
          `<td class="n">${escapeHtml(row.ms)}</td><td class="n">${escapeHtml(row.sim)}</td>` +
          `<td class="n">${escapeHtml(row.render)}</td></tr>`,
    )
    .join("");

  const partyRows = parties
    .map(
      (house) =>
        `<tr><td>${escapeHtml(house.name)}</td>` +
        `<td>${house.is_human ? t("tag.human") : t("tag.ai")}</td>` +
        `<td class="n">${fmt(house.counts.structure, 0)}</td>` +
        `<td class="n">${fmt(house.credits, 0)}</td>` +
        `<td class="n">${fmt(house.score, 0)}</td></tr>`,
    )
    .join("");

  return `<!doctype html>
<html lang="${escapeHtml(document.documentElement.lang || "en")}">
<head><meta charset="utf-8"><title>${escapeHtml(t("rep.title"))} · tick ${thousands(view.tick)}</title>
<style>
*{box-sizing:border-box;margin:0}
body{font:14px/1.5 -apple-system,'Segoe UI','PingFang SC','Microsoft YaHei',sans-serif;color:#1d2733;background:#f7f9fc;padding:28px 32px}
h1{font-size:24px;font-weight:700}
h2{font-size:14px;font-weight:600;color:#4a5b6e;margin:26px 0 10px}
.sub{color:#5b6b7d;font-size:13px;margin-top:6px}
.pies{display:flex;gap:34px;flex-wrap:wrap;align-items:flex-start}
figure.pie{flex:1 1 430px;min-width:0;background:#fff;border:1px solid #e2e8f0;border-radius:8px;padding:18px 20px;box-shadow:0 1px 3px rgba(16,24,40,.06)}
figcaption{font-size:13px;color:#4a5b6e;margin-bottom:10px}
.body{display:flex;align-items:center;gap:22px}
ul{list-style:none}
li{display:flex;align-items:center;gap:8px;margin:5px 0;font-size:12.5px}
.sw{width:10px;height:10px;border-radius:2px;flex:none}
.lb{min-width:118px}
.ms{color:#5b6b7d;font-variant-numeric:tabular-nums}
table{border-collapse:collapse;font-variant-numeric:tabular-nums;width:100%}
th,td{text-align:left;padding:4px 22px 4px 0;border-bottom:1px solid #e8edf3;font-size:13px}
th{color:#6b7b8d;font-weight:600;font-size:12px}
th.n,td.n{text-align:right;padding-right:0}
.houses{table-layout:fixed}
.houses th:first-child{width:40%}
.houses td:first-child{overflow-wrap:anywhere}
tr.group td{color:#4a5b6e;font-weight:600;padding-top:14px}
tr.foot td{font-weight:600;color:#1d2733;border-bottom:none}
.note{margin-top:22px;color:#7b8b9d;font-size:12px}
</style></head>
<body>
<h1>${escapeHtml(t("rep.title"))}</h1>
<div class="sub">tick ${thousands(view.tick)} · ${t("rep.fps")} ${fmt(view.timing?.fps ?? 0, 0)} fps · ${fmt(tickHz, 1)} tick/s · ${t("hdr.map")} ${fmt(view.map_width, 0)}x${fmt(view.map_height, 0)}<br>
${t("rep.sim")} ${measuredCost(profile?.mean_frame_ms ?? 0, profile?.frames ?? 0)} · ${t("rep.peak")} ${measuredCost(profile?.peak_frame_ms ?? 0, profile?.frames ?? 0)}</div>
<p class="note">${escapeHtml(t("rep.note"))}</p>
<div class="pies">${pie(renderSlices, renderTotal, t("rep.renderSample"), render?.frames ?? 0, t("rep.renders"))}${pie(simSlices, simTotal, t("rep.simTick"), profile?.frames ?? 0, t("rep.frames"))}</div>
<h2>${escapeHtml(t("rep.phases"))}</h2>
<table><thead><tr><th>${escapeHtml(t("th.phase"))}</th><th class="n">${escapeHtml(t("th.ms").toUpperCase())}</th><th class="n">${escapeHtml(t("th.shareSim"))}</th><th class="n">${escapeHtml(t("th.shareRender"))}</th></tr></thead><tbody>${rows}</tbody></table>
${parties.length > 0 ? `<h2>${escapeHtml(t("rep.houses"))}</h2><table class="houses"><thead><tr><th>${escapeHtml(t("th.house"))}</th><th>${escapeHtml(t("rep.kind"))}</th><th class="n">${escapeHtml(t("rep.buildings"))}</th><th class="n">${escapeHtml(t("rep.credits"))}</th><th class="n">${escapeHtml(t("rep.score"))}</th></tr></thead><tbody>${partyRows}</tbody></table>` : ""}
<p class="note">VERA20k · schema v${SCHEMA_VERSION} · ${new Date().toLocaleString()}</p>
</body></html>`;
}

function openShotPreview(
  kind: "ledger" | "report" = "ledger",
  view: ReportView | null = lastSnapshot,
): void {
  const tick = view?.tick ?? 0;
  const canvas = kind === "report" ? buildReportCanvas(view) : buildPhaseLedgerCanvas();
  if (!canvas) return;
  // The report can also be saved as HTML; the ledger snapshot cannot.
  shotHtml = kind === "report" ? buildReportHtml(view) : null;
  canvas.toBlob((blob) => {
    if (!blob) return;
    if (shotUrl) URL.revokeObjectURL(shotUrl);
    shotBlob = blob;
    shotTick = tick;
    shotUrl = URL.createObjectURL(blob);
    const image = document.getElementById("shot-img") as HTMLImageElement | null;
    if (image) image.src = shotUrl;
    const title = document.getElementById("shot-title");
    if (title) title.textContent = t(kind === "report" ? "rep.reportShot" : "rep.shotTitle");
    const htmlButton = document.getElementById("shot-html");
    if (htmlButton) htmlButton.hidden = shotHtml === null;
    const modal = document.getElementById("shot-modal");
    if (modal) modal.hidden = false;
    shotNote("", "ok");
  }, "image/png");
}

function closeShot(): void {
  const modal = document.getElementById("shot-modal");
  if (modal) modal.hidden = true;
  const image = document.getElementById("shot-img") as HTMLImageElement | null;
  if (image) image.removeAttribute("src");
  if (shotUrl) {
    URL.revokeObjectURL(shotUrl);
    shotUrl = null;
  }
  shotBlob = null;
  shotHtml = null;
}

function shotNote(text: string, tone: "ok" | "bad"): void {
  const note = document.getElementById("shot-note");
  if (!note) return;
  note.textContent = text;
  note.style.color = tone === "bad" ? "var(--amber)" : "var(--green)";
}

/** Put the image on the clipboard. Needs a secure context; 127.0.0.1 has one. */
async function copyShot(): Promise<void> {
  if (!shotBlob) return;
  try {
    await navigator.clipboard.write([new ClipboardItem({ "image/png": shotBlob })]);
    shotNote(t("rep.copied"), "ok");
  } catch {
    // A denied clipboard is not fatal: the download path still works.
    shotNote(t("rep.copyFailed"), "bad");
  }
}

function saveShot(): void {
  if (!shotUrl) return;
  const link = document.createElement("a");
  link.href = shotUrl;
  link.download = `phase-ledger-tick${shotTick}.png`;
  link.click();
}

/** Save the report as a self-contained HTML file, built in this tab. */
function saveReportHtml(): void {
  if (!shotHtml) return;
  const blob = new Blob([shotHtml], { type: "text/html;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `vera20k-report-tick${shotTick}.html`;
  link.click();
  URL.revokeObjectURL(url);
}

/** The HTML behind a report preview, when the preview is a report. */
let shotHtml: string | null = null;

function updateFrameProfile(
  profile: FrameProfileReport | null,
  render: RenderTimingReport | null,
): void {
  const summary = document.getElementById("frame-summary");
  if (!summary) return;
  const { sim, render: rendered, simTotal, renderTotal } = buildTimingSlices(profile, render);
  drawPie("pie-frame", "pie-frame-legend", rendered, renderTotal);
  drawPie("pie-sim", "pie-sim-legend", sim, simTotal);
  document.getElementById("pie-frame-total")!.textContent = measuredCost(renderTotal, render?.frames ?? 0);
  document.getElementById("pie-sim-total")!.textContent = measuredCost(simTotal, profile?.frames ?? 0);
  const rows: [string, string][] = [
    [t("rep.sim"), measuredCost(profile?.mean_frame_ms ?? 0, profile?.frames ?? 0)],
    [t("rep.peak"), measuredCost(profile?.peak_frame_ms ?? 0, profile?.frames ?? 0)],
    [t("rep.frames"), thousands(profile?.frames ?? 0)],
    [t("rep.renderSample"), measuredCost(renderTotal, render?.frames ?? 0)],
    [t("rep.renders"), thousands(render?.frames ?? 0)],
  ];
  summary.innerHTML = rows
    .map(([key, value]) => `<div class='row'><span>${key}</span><b>${value}</b></div>`)
    .join("");
}

// ---------------------------------------------------- process page (帧耗时 panel, page 2)

/**
 * Host resources: CPU share of the whole machine, resident and peak working
 * set, live thread count, and the adapter plus this process's own resident
 * atlas textures. Hardware GPU utilization is deliberately not claimed here —
 * it needs vendor driver counters this build does not read.
 */
function updateProcess(process: ProcessStats | null): void {
  const value = (id: string, text: string, cls = "proc-value"): void => {
    const cell = document.getElementById(id);
    if (!cell) return;
    cell.textContent = text;
    cell.className = cls;
  };
  const meter = (id: string, fraction: number, hot: boolean): void => {
    const bar = document.getElementById(id);
    if (!bar) return;
    bar.style.width = `${Math.min(100, Math.max(0, fraction * 100))}%`;
    bar.classList.toggle("hot", hot);
  };
  if (!process) {
    for (const id of [
      "proc-cpu",
      "proc-mem",
      "proc-threads",
      "proc-gpu",
      "proc-cores",
      "proc-peak",
      "proc-texture",
    ]) {
      value(id, "-");
    }
    meter("proc-cpu-bar", 0, false);
    meter("proc-mem-bar", 0, false);
    return;
  }
  // 80% of the machine is the interesting line: below it the host has slack,
  // above it the pacer is competing with everything else on the box.
  const cpuHot = process.cpu_percent >= 80;
  value("proc-cpu", `${fmt(process.cpu_percent, 1)}%`, `proc-value${cpuHot ? " hot" : ""}`);
  meter("proc-cpu-bar", process.cpu_percent / 100, cpuHot);
  value("proc-cores", `${process.cpu_cores} ${t("proc.cores")}`, "proc-sub");

  const memHot =
    process.peak_working_set_mb > 0 && process.working_set_mb >= process.peak_working_set_mb * 0.9;
  value("proc-mem", `${fmt(process.working_set_mb, 0)} MB`, `proc-value${memHot ? " hot" : ""}`);
  meter(
    "proc-mem-bar",
    process.peak_working_set_mb > 0
      ? process.working_set_mb / process.peak_working_set_mb
      : 0,
    memHot,
  );
  value(
    "proc-peak",
    `${t("proc.peak")} ${fmt(process.peak_working_set_mb, 0)} MB`,
    "proc-sub",
  );

  value("proc-threads", `${process.threads}`);
  value("proc-gpu", process.gpu_adapter ?? "-", "proc-value small");
  value("proc-texture", `${t("proc.texture")} ${fmt(process.gpu_texture_mb, 0)} MB`, "proc-sub");
}

/** Two panels paginate: the cost column switches between tick cost and host
 *  resources, the measured-cost panel between renders and simulation ticks. Each
 *  panel carries its own `data-page`, so switching one never hides the other's
 *  content. A hidden page's canvas has zero size, so returning to the cost
 *  chart redraws it; the pies are inline SVG and need no redraw. */
function setupPager(): void {
  for (const panel of document.querySelectorAll<HTMLElement>(".panel[data-page]")) {
    for (const tab of panel.querySelectorAll<HTMLElement>(".pager-tab")) {
      tab.addEventListener("click", () => {
        const page = tab.dataset.page;
        if (!page) return;
        panel.dataset.page = page;
        for (const sibling of panel.querySelectorAll<HTMLElement>(".pager-tab")) {
          sibling.classList.toggle("on", sibling === tab);
        }
        if (panel.id === "cost" && page === "cost" && lastSnapshot) {
          drawChart(lastSnapshot.timing);
        }
      });
    }
  }
}

// ------------------------------------------------------- detail wall (副屏)

/**
 * The player-detail view: economy over time, live cash, spend, score, base,
 * force composition and the unit-type roster — one card per house. It reads
 * the same snapshots as the wall; its per-house history accumulates even while
 * the wall view is showing, so switching over shows a continuous chart.
 */

/** Samples retained per house for the sparkline and the economy chart. */
const DETAIL_HISTORY = 240;
/** Per-house sampled series, keyed by house name (stable across a match). */
const houseHistories = new Map<string, { credits: number[] }>();
/** Cached card element handles, rebuilt only when the house set changes. */
let detailCardKeys: string[] = [];
/** Which wall is showing; the header tabs drive it. */
let currentView: AppView = "wall";

/** Category colours, shared with the main wall's composition bars. */
const DETAIL_CATEGORY_COLOR: Record<string, string> = {
  Infantry: "#3f8f6f",
  Unit: "#2f6fb0",
  Aircraft: "#8a5fc0",
  Structure: "#b0763a",
};
const DETAIL_ORPHAN_COLOR = "#6b7a8c";

function houseCss(house: HousePopulation): string {
  return house.color
    ? `rgb(${house.color[0]},${house.color[1]},${house.color[2]})`
    : DETAIL_ORPHAN_COLOR;
}

/** A house that never fights: Neutral / Special (MultiplayPassive in rules). */
function isPassiveHouse(house: HousePopulation): boolean {
  return house.passive;
}

function detailTotal(counts: HousePopulation["counts"]): number {
  return counts.unit + counts.infantry + counts.aircraft + counts.structure;
}

function detailCategoryKey(category: string): string {
  switch (category) {
    case "Infantry":
      return "cat.infantry";
    case "Unit":
      return "cat.vehicles";
    case "Aircraft":
      return "cat.aircraft";
    default:
      return "cat.structures";
  }
}

/** Push one sample per house; drop series whose house left the match. */
function sampleHouseEconomy(snapshot: TelemetrySnapshot): void {
  const live = new Set<string>();
  for (const house of snapshot.houses) {
    live.add(house.name);
    let history = houseHistories.get(house.name);
    if (!history) {
      history = { credits: [] };
      houseHistories.set(house.name, history);
    }
    history.credits.push(house.credits);
    if (history.credits.length > DETAIL_HISTORY) history.credits.shift();
  }
  for (const name of [...houseHistories.keys()]) {
    if (!live.has(name)) houseHistories.delete(name);
  }
}

function fitDetailCanvas(canvas: HTMLCanvasElement): CanvasRenderingContext2D | null {
  const ratio = window.devicePixelRatio || 1;
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  if (width === 0 || height === 0) return null;
  if (canvas.width !== Math.round(width * ratio) || canvas.height !== Math.round(height * ratio)) {
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, width, height);
  return ctx;
}

/** Multi-line credits chart: one coloured trace per fighting house. */
function drawEconomyChart(snapshot: TelemetrySnapshot): void {
  const canvas = document.getElementById("economy") as HTMLCanvasElement | null;
  if (!canvas) return;
  const ctx = fitDetailCanvas(canvas);
  if (!ctx) return; // hidden view: zero-sized, redrawn on activation
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;

  const series = snapshot.houses
    .filter((house) => !isPassiveHouse(house))
    .map((house) => ({ house, values: houseHistories.get(house.name)?.credits ?? [] }))
    .filter((entry) => entry.values.length > 0);
  const peak = Math.max(1, ...series.flatMap((entry) => entry.values));

  ctx.strokeStyle = "rgba(0, 190, 255, 0.10)";
  ctx.lineWidth = 1;
  for (let band = 0; band <= 4; band++) {
    const y = 6 + (band / 4) * (height - 18);
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
    ctx.stroke();
  }

  const span = DETAIL_HISTORY - 1;
  for (const { house, values } of series) {
    const color = houseCss(house);
    ctx.save();
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.8;
    ctx.shadowColor = color;
    ctx.shadowBlur = 8;
    ctx.beginPath();
    values.forEach((value, index) => {
      // Newest sample sits at the right edge; a short series grows leftward.
      const x = width - ((values.length - 1 - index) / span) * width;
      const y = 6 + (1 - value / peak) * (height - 18);
      if (index === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.stroke();
    ctx.restore();
  }

  ctx.fillStyle = "rgba(160, 210, 235, 0.75)";
  ctx.font = "10px monospace";
  ctx.fillText(fmt(peak), 4, 12);
  ctx.fillText("0", 4, height - 6);

  const legend = document.getElementById("economy-legend");
  if (legend) {
    legend.textContent = "";
    for (const { house } of series) {
      const item = document.createElement("span");
      const dot = document.createElement("i");
      dot.style.background = houseCss(house);
      item.append(dot, document.createTextNode(house.name));
      legend.append(item);
    }
  }
}

function drawSparkline(canvas: HTMLCanvasElement, values: number[], color: string): void {
  const ctx = fitDetailCanvas(canvas);
  if (!ctx || values.length === 0) return;
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  const peak = Math.max(1, ...values);
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.4;
  ctx.shadowColor = color;
  ctx.shadowBlur = 6;
  ctx.beginPath();
  values.forEach((value, index) => {
    const x = (index / Math.max(1, values.length - 1)) * width;
    const y = 4 + (1 - value / peak) * (height - 8);
    if (index === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  });
  ctx.stroke();
  ctx.restore();
}

interface DetailCard {
  root: HTMLElement;
  dot: HTMLElement;
  tags: HTMLElement;
  credits: HTMLElement;
  delta: HTMLElement;
  spark: HTMLCanvasElement;
  stats: HTMLElement;
  bar: HTMLElement;
  compLegend: HTMLElement;
  rosterPeek: HTMLElement;
}

function buildDetailCard(house: HousePopulation): DetailCard {
  const root = document.createElement("section");
  root.className = "panel house-card";

  const head = document.createElement("div");
  head.className = "card-head";
  const dot = document.createElement("i");
  dot.className = "house-dot";
  const name = document.createElement("span");
  name.className = "card-name";
  name.textContent = house.name;
  name.title = house.name;
  const spacer = document.createElement("span");
  spacer.className = "spacer";
  const tags = document.createElement("span");
  head.append(dot, name, spacer, tags);

  const econ = document.createElement("div");
  econ.className = "card-econ";
  const big = document.createElement("div");
  big.className = "big";
  const credits = document.createElement("div");
  credits.className = "value";
  const creditsLabel = document.createElement("div");
  creditsLabel.className = "label";
  creditsLabel.textContent = t("dp.credits");
  const delta = document.createElement("div");
  delta.className = "delta";
  big.append(credits, creditsLabel, delta);
  const spark = document.createElement("canvas");
  spark.className = "spark";
  econ.append(big, spark);

  const stats = document.createElement("div");
  stats.className = "stat-grid";

  const forceTitle = document.createElement("div");
  forceTitle.className = "sub";
  forceTitle.textContent = t("sec.pop");
  const bar = document.createElement("div");
  bar.className = "comp-bar";
  const compLegend = document.createElement("div");
  compLegend.className = "comp-legend";

  const rosterTitle = document.createElement("div");
  rosterTitle.className = "sub";
  rosterTitle.textContent = t("dp.roster");
  // The full list lives in the click-through modal. What stays on the card is
  // one line of evidence that there is something to open: the top types as
  // chips, and the roster length, so a house that fields many types reads as
  // "worth opening" without anyone having to discover the scroll box first.
  const rosterPeek = document.createElement("button");
  rosterPeek.type = "button";
  rosterPeek.className = "roster-peek";

  root.append(head, econ, stats, forceTitle, bar, compLegend, rosterTitle, rosterPeek);
  root.tabIndex = 0;
  root.setAttribute("role", "button");
  root.title = t("dp.openDetail");
  return { root, dot, tags, credits, delta, spark, stats, bar, compLegend, rosterPeek };
}

function renderDetailCards(snapshot: TelemetrySnapshot): void {
  const container = document.getElementById("cards");
  if (!container) return;

  // Human players first, then AI, then the passive houses; name breaks ties.
  const houses = [...snapshot.houses].sort((a, b) => {
    const rank = (house: HousePopulation): number =>
      house.is_human ? 0 : isPassiveHouse(house) ? 2 : 1;
    return rank(a) - rank(b) || a.name.localeCompare(b.name);
  });

  const keys = houses.map((house) => house.name);
  const views = new Map<string, DetailCard>();
  if (keys.join("\u0000") !== detailCardKeys.join("\u0000")) {
    container.textContent = "";
    for (const house of houses) {
      const view = buildDetailCard(house);
      // The whole card is the target, and the roster line inside it is a real
      // button — so it also works by keyboard, and the peek line keeps the
      // same affordance for anyone who looks for it first.
      view.root.addEventListener("click", () => openHouseModal(house.name));
      view.root.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          openHouseModal(house.name);
        }
      });
      view.rosterPeek.addEventListener("click", (event) => {
        event.stopPropagation();
        openHouseModal(house.name);
      });
      container.append(view.root);
      views.set(house.name, view);
    }
    detailCardKeys = keys;
  } else {
    const existing = container.querySelectorAll<HTMLElement>(".house-card");
    houses.forEach((house, index) => {
      const root = existing[index];
      if (!root) return;
      views.set(house.name, {
        root,
        dot: root.querySelector(".house-dot") as HTMLElement,
        tags: root.querySelector(".card-head span:last-child") as HTMLElement,
        credits: root.querySelector(".card-econ .value") as HTMLElement,
        delta: root.querySelector(".card-econ .delta") as HTMLElement,
        spark: root.querySelector(".spark") as HTMLCanvasElement,
        stats: root.querySelector(".stat-grid") as HTMLElement,
        bar: root.querySelector(".comp-bar") as HTMLElement,
        compLegend: root.querySelector(".comp-legend") as HTMLElement,
        rosterPeek: root.querySelector(".roster-peek") as HTMLElement,
      });
    });
  }

  for (const house of houses) {
    const view = views.get(house.name);
    if (!view) continue;
    const color = houseCss(house);
    const history = houseHistories.get(house.name)?.credits ?? [];
    const previous = history.length > 1 ? history[history.length - 2] : house.credits;
    const delta = house.credits - previous;

    view.root.classList.toggle("passive", isPassiveHouse(house));
    view.root.classList.toggle("defeated", house.defeated);
    view.dot.style.background = color;
    view.dot.style.boxShadow = `0 0 8px ${color}`;

    renderHouseTags(view.tags, house);

    view.credits.textContent = fmt(house.credits, 0);
    view.delta.className = `delta ${delta > 0 ? "up" : delta < 0 ? "down" : ""}`;
    view.delta.textContent =
      delta === 0 ? "" : `${delta > 0 ? "▲" : "▼"} ${fmt(Math.abs(delta), 0)}`;

    const cells: [string, string][] = [
      [t("dp.spent"), fmt(house.spent, 0)],
      [t("dp.score"), fmt(house.score, 0)],
      [t("dp.buildings"), fmt(house.counts.structure, 0)],
      [t("dp.base"), house.base_center ? `${house.base_center[0]}, ${house.base_center[1]}` : "-"],
      [t("dp.side"), sideName(house)],
      [t("dp.units"), fmt(detailTotal(house.counts), 0)],
    ];
    renderStatGrid(view.stats, cells);

    const total = Math.max(1, detailTotal(house.counts));
    view.bar.textContent = "";
    const segments: [keyof HousePopulation["counts"], string][] = [
      ["infantry", DETAIL_CATEGORY_COLOR.Infantry],
      ["unit", DETAIL_CATEGORY_COLOR.Unit],
      ["aircraft", DETAIL_CATEGORY_COLOR.Aircraft],
      ["structure", DETAIL_CATEGORY_COLOR.Structure],
    ];
    for (const [key, css] of segments) {
      const count = house.counts[key];
      if (count === 0) continue;
      const seg = document.createElement("span");
      seg.className = "seg";
      seg.style.width = `${(count / total) * 100}%`;
      seg.style.background = css;
      view.bar.append(seg);
    }
    view.compLegend.replaceChildren(...segments
      .filter(([key]) => house.counts[key] > 0)
      .map(([key, css]) => {
        const legend = document.createElement("span");
        const dot = document.createElement("i");
        dot.style.background = css;
        const category = key === "infantry" ? "Infantry" : key === "unit" ? "Unit"
          : key === "aircraft" ? "Aircraft" : "Structure";
        legend.append(dot, document.createTextNode(`${t(detailCategoryKey(category))} ${house.counts[key]}`));
        return legend;
      }));

    // One line of evidence, not the list itself: the full roster is a click
    // away in the house modal, where the whole list can scroll.
    view.rosterPeek.textContent = "";
    if (house.roster.length === 0) {
      const empty = document.createElement("span");
      empty.className = "peek-empty";
      empty.textContent = t("dp.none");
      view.rosterPeek.append(empty);
    } else {
      // The wire caps the roster at TELEMETRY_MAX_ROSTER types, so "N types"
      // is the reported length; the engine truncates the tail, it does not
      // page it, and the modal is where that cap becomes visible.
      for (const entry of house.roster.slice(0, HOUSE_PEEK_TYPES)) {
        const chip = document.createElement("span");
        chip.className = "peek-chip";
        const dot = document.createElement("i");
        dot.style.background = DETAIL_CATEGORY_COLOR[entry.category] ?? DETAIL_ORPHAN_COLOR;
        const name = document.createElement("b");
        name.textContent = entry.type_name;
        const count = document.createElement("em");
        count.textContent = fmt(entry.count, 0);
        chip.append(dot, name, count);
        chip.title = `${entry.type_name} · ${t(detailCategoryKey(entry.category))}`;
        view.rosterPeek.append(chip);
      }
      const more = document.createElement("span");
      more.className = "peek-more";
      const hidden = house.roster.length - HOUSE_PEEK_TYPES;
      more.textContent =
        hidden > 0
          ? `${t("dp.peekMore", { n: hidden })}`
          : t("dp.peekOpen");
      view.rosterPeek.append(more);
    }

    drawSparkline(view.spark, history, color);
  }

  // A change of house set rebuilds the cards; the open modal follows the same
  // key, so it closes rather than showing a house that is no longer listed.
  if (houseModalName && !keys.includes(houseModalName)) closeHouseModal();
}

/** How many roster types the card line shows before it counts the rest. */
const HOUSE_PEEK_TYPES = 3;

/** The house the detail modal is showing, or null when it is closed. */
let houseModalName: string | null = null;

/**
 * Open the house detail modal. The card is the only entry point, and the
 * content is rebuilt from the snapshot rather than cloned from the card, so
 * the modal can hold what the card has no room for: the whole roster, the
 * power reading and the per-category totals.
 */
function openHouseModal(name: string): void {
  const snapshot = lastSnapshot;
  const house = snapshot?.houses.find((row) => row.name === name);
  const modal = document.getElementById("house-modal");
  if (!snapshot || !house || !modal) return;
  houseModalName = name;
  renderHouseModal();
  modal.hidden = false;
}

function closeHouseModal(): void {
  const modal = document.getElementById("house-modal");
  if (modal) modal.hidden = true;
  houseModalName = null;
}

/** Repaint the open modal from the live snapshot. Cheap; runs on every frame
 *  the snapshot arrives, which is what makes the modal a live view rather
 *  than a snapshot of the click. */
function renderHouseModal(): void {
  if (!houseModalName) return;
  const snapshot = lastSnapshot;
  const house = snapshot?.houses.find((row) => row.name === houseModalName);
  if (!snapshot || !house) return;
  const color = houseCss(house);

  const dot = document.getElementById("house-modal-dot");
  if (dot) {
    dot.style.background = color;
    dot.style.boxShadow = `0 0 10px ${color}`;
  }
  const title = document.getElementById("house-modal-title");
  if (title) title.textContent = house.name;

  const tags = document.getElementById("house-modal-tags");
  if (tags) renderHouseTags(tags, house);

  // The economy strip and the composition bar are the same drawings the card
  // makes, so the modal cannot quote a different number than the card does.
  const stats = document.getElementById("house-modal-stats");
  if (stats) {
    const powerState = house.low_power
      ? t("dp.powerLow")
      : t("dp.powerOk");
    const cells: [string, string][] = [
      [t("dp.credits"), fmt(house.credits, 0)],
      [t("dp.spent"), fmt(house.spent, 0)],
      [t("dp.score"), fmt(house.score, 0)],
      [t("dp.power"), `${thousands(house.power_output)} / ${thousands(house.power_drain)}`],
      [t("dp.units"), fmt(detailTotal(house.counts), 0)],
      [t("dp.base"), house.base_center ? `${house.base_center[0]}, ${house.base_center[1]}` : "-"],
      [t("dp.side"), sideName(house)],
      [t("hdr.tick"), thousands(snapshot.tick)],
    ];
    renderStatGrid(stats, cells);
    const power = document.getElementById("house-modal-power");
    if (power) power.textContent = powerState;
  }

  const total = Math.max(1, detailTotal(house.counts));
  const bar = document.getElementById("house-modal-bar");
  if (bar) {
    bar.textContent = "";
    const segments: [keyof HousePopulation["counts"], string][] = [
      ["infantry", DETAIL_CATEGORY_COLOR.Infantry],
      ["unit", DETAIL_CATEGORY_COLOR.Unit],
      ["aircraft", DETAIL_CATEGORY_COLOR.Aircraft],
      ["structure", DETAIL_CATEGORY_COLOR.Structure],
    ];
    for (const [key, css] of segments) {
      if (house.counts[key] === 0) continue;
      const seg = document.createElement("span");
      seg.className = "seg";
      seg.style.width = `${(house.counts[key] / total) * 100}%`;
      seg.style.background = css;
      bar.append(seg);
    }
  }

  // What the house is building. A factory that has finished and is waiting for
  // a placement click is the interesting state, so it reads as its own line
  // rather than as a full progress bar that stopped at the end.
  const production = document.getElementById("house-modal-production");
  if (production) {
    production.textContent = "";
    if (house.production.length === 0) {
      production.appendChild(emptyRow(t("dp.noProduction")));
    } else {
      for (const line of house.production) {
        const row = document.createElement("div");
        row.className = "prod-row";
        const cat = document.createElement("span");
        cat.className = "prod-cat";
        cat.textContent = t(`prod.${line.category.toLowerCase()}`);
        const name = document.createElement("span");
        name.className = "prod-name";
        name.textContent = line.active_type ?? "—";
        name.title = line.active_type ?? "";

        // One bar per factory, scaled by the engine's own 54-step progress so
        // the reading matches what the sidebar cameo shows.
        const gauge = document.createElement("span");
        gauge.className = "prod-gauge";
        const fill = document.createElement("i");
        fill.style.width = `${Math.min(100, (line.progress / TELEMETRY_PRODUCTION_STEPS) * 100)}%`;
        fill.style.background = color;
        gauge.append(fill);

        const state = document.createElement("span");
        state.className = "prod-state";
        if (line.suspended) state.textContent = t("prod.suspended");
        else if (line.on_hold) state.textContent = t("prod.onHold");
        else if (line.ready_to_place.length > 0) state.textContent = t("prod.ready");
        else if (line.queue.length > 0) state.textContent = t("prod.queued", { n: line.queue.length });
        else state.textContent = "";

        row.append(cat, name, gauge, state);
        if (line.ready_to_place.length > 0) {
          row.classList.add("ready");
          const pending = document.createElement("em");
          pending.className = "prod-pending";
          pending.textContent = line.ready_to_place.join(" · ");
          pending.title = line.ready_to_place.join(", ");
          row.append(pending);
        }
        production.append(row);
      }
    }
  }

  const squads = document.getElementById("house-modal-squads");
  if (squads) {
    squads.textContent = "";
    if (house.squads.length === 0) {
      const empty = document.createElement("li");
      empty.className = "empty";
      empty.textContent = t("dp.noSquads");
      squads.append(empty);
    } else {
      for (const squad of house.squads) {
        const head = document.createElement("li");
        head.className = "roster-group";
        const label =
          squad.team_type ??
          (squad.task_force ? squad.task_force : t("dp.squadUnnamed", { n: squad.id }));
        // The forming/suspended flag rides at the right of the team's own line
        // and the member count stays with its name: as a row beneath the header
        // the flag read as a separate entry, and after the count it read as
        // part of the number.
        const flag = squad.suspended
          ? t("prod.suspended")
          : !squad.formed
            ? t("dp.squadForming")
            : "";
        const name = document.createElement("span");
        name.textContent = label;
        const members = document.createElement("em");
        members.textContent = String(squad.members);
        head.append(name, members);
        if (flag) {
          const status = document.createElement("i");
          status.className = "squad-flag";
          status.textContent = flag;
          head.append(status);
        }
        squads.append(head);
        for (const entry of squad.composition) {
          const row = document.createElement("li");
          const count = document.createElement("span");
          count.className = "cnt";
          count.textContent = fmt(entry.count, 0);
          const type = document.createElement("span");
          type.className = "tname";
          type.textContent = entry.type_name;
          type.title = entry.type_name;
          const category = document.createElement("span");
          category.className = "cat";
          category.style.color = DETAIL_CATEGORY_COLOR[entry.category] ?? "var(--dim)";
          category.textContent = t(detailCategoryKey(entry.category));
          row.append(count, type, category);
          squads.append(row);
        }
      }
    }
  }

  const basePlan = document.getElementById("house-modal-baseplan");
  if (basePlan) {
    basePlan.textContent = "";
    const outstanding = house.base_plan.filter((node) => node.is_base);
    if (house.base_plan.length === 0) {
      basePlan.appendChild(emptyRow(t("dp.noBasePlan")));
    } else {
      const note = document.createElement("p");
      note.className = "note baseplan-note";
      // The engine's node carries a BuildingTypes registry index, not a name,
      // so the panel reports how much of the plan is outstanding and where —
      // and says so rather than inventing building names.
      note.textContent = t("dp.basePlanCount", {
        n: outstanding.length,
        m: house.base_plan.length,
      });
      basePlan.append(note);
      const wrap = document.createElement("div");
      wrap.className = "baseplan-cells";
      for (const node of house.base_plan) {
        const cell = document.createElement("span");
        cell.className = node.is_base ? "cell pending" : "cell";
        cell.textContent = `${node.x},${node.y}`;
        cell.title = node.is_base ? t("dp.basePlanPending") : t("dp.basePlanDone");
        wrap.append(cell);
      }
      basePlan.append(wrap);
    }
  }

  // The whole roster, grouped by category so a long list reads as four short
  // blocks instead of one sorted column. The engine caps the list at 16 types;
  // saying so here is better than letting a reader assume 16 is the whole
  // army.
  const list = document.getElementById("house-modal-roster");
  if (list) {
    list.textContent = "";
    if (house.roster.length === 0) {
      const empty = document.createElement("li");
      empty.className = "empty";
      empty.textContent = t("dp.none");
      list.append(empty);
    } else {
      const groups = new Map<string, TypeCount[]>();
      for (const entry of house.roster) {
        const bucket = groups.get(entry.category);
        if (bucket) bucket.push(entry);
        else groups.set(entry.category, [entry]);
      }
      for (const [category, entries] of groups) {
        const head = document.createElement("li");
        head.className = "roster-group";
        const dot = document.createElement("i");
        dot.className = "cat-dot";
        dot.style.background = DETAIL_CATEGORY_COLOR[category] ?? DETAIL_ORPHAN_COLOR;
        const label = document.createElement("span");
        label.textContent = t(detailCategoryKey(category));
        const total = document.createElement("em");
        total.textContent = String(entries.reduce((sum, entry) => sum + entry.count, 0));
        head.append(dot, label, total);
        list.append(head);
        for (const entry of entries) {
          const row = document.createElement("li");
          const count = document.createElement("span");
          count.className = "cnt";
          count.textContent = fmt(entry.count, 0);
          const type = document.createElement("span");
          type.className = "tname";
          type.textContent = entry.type_name;
          type.title = entry.type_name;
          row.append(count, type);
          list.append(row);
        }
      }
    }
    const note = document.getElementById("house-modal-note");
    if (note) {
      const types = house.roster.length;
      note.textContent =
        types >= TELEMETRY_ROSTER_CAP
          ? t("dp.rosterCapped", { n: types })
          : t("dp.rosterFull", { n: types });
    }
  }

  const spark = document.getElementById("house-modal-spark") as HTMLCanvasElement | null;
  if (spark) {
    const history = houseHistories.get(house.name)?.credits ?? [];
    drawSparkline(spark, history, color);
  }
}

/** The engine's per-house roster ceiling (`TELEMETRY_MAX_ROSTER`). Mirrored
 *  here because the modal has to say when a house is sitting at the cap. */
const TELEMETRY_ROSTER_CAP = 16;

/** The engine's production steps (`PRODUCTION_STEPS`): a factory's progress
 *  runs 0..=54. Mirrored so the gauge scales the way the sidebar cameo does. */
const TELEMETRY_PRODUCTION_STEPS = 54;

/** A "nothing here" line for the modal's block-level sections. */
function emptyRow(text: string): HTMLElement {
  const row = document.createElement("p");
  row.className = "empty-row";
  row.textContent = text;
  return row;
}

/** Full detail-wall refresh. Drawing only — `onmessage` owns the sampling,
 *  so a switch-time redraw never injects a phantom history point. */
function renderDetail(snapshot: TelemetrySnapshot): void {
  drawEconomyChart(snapshot);
  renderDetailCards(snapshot);
  // An open house modal is a live view, not a picture of the click: it repaints
  // from the same snapshot the cards just drew.
  renderHouseModal();
}


/** Header tabs: show one view, hide the others. A switch into the detail view
 *  redraws immediately because its canvases were zero-sized while hidden. */
function setupViews(): void {
  const show = (view: AppView): void => {
    currentView = view;
    document.body.dataset.view = view;
    document.getElementById("view-wall")?.classList.toggle("on", view === "wall");
    document.getElementById("view-detail")?.classList.toggle("on", view === "detail");
    document.getElementById("view-report")?.classList.toggle("on", view === "report");
    document.getElementById("view-logs")?.classList.toggle("on", view === "logs");
    if (view === "detail" && lastSnapshot) renderDetail(lastSnapshot);
    if (view === "report") renderReport();
    if (view === "logs") rebuildLogView();
  };
  document.getElementById("view-wall")?.addEventListener("click", () => show("wall"));
  document.getElementById("view-detail")?.addEventListener("click", () => show("detail"));
  document.getElementById("view-report")?.addEventListener("click", () => show("report"));
  document.getElementById("view-logs")?.addEventListener("click", () => show("logs"));
  document.getElementById("log-pause")?.addEventListener("click", toggleLogPause);
  document.getElementById("log-clear")?.addEventListener("click", clearLogs);
  document.getElementById("log-follow")?.addEventListener("change", (event) => {
    logFollow = (event.target as HTMLInputElement).checked;
    if (logFollow) rebuildLogView();
  });
  document.getElementById("log-level")?.addEventListener("change", (event) => {
    logLevel = Number((event.target as HTMLSelectElement).value) || 3;
    rebuildLogView();
  });
  const logSearch = document.getElementById("log-search") as HTMLInputElement | null;
  if (logSearch) {
    logSearch.placeholder = t("log.search");
    logSearch.addEventListener("input", () => {
      if (logSearchTimer !== null) window.clearTimeout(logSearchTimer);
      logSearchTimer = window.setTimeout(() => {
        logSearchTimer = null;
        logQuery = logSearch.value.trim().toLowerCase();
        rebuildLogView();
      }, 160);
    });
  }
  document.getElementById("make-report")?.addEventListener("click", () => openShotPreview("report"));
  document.getElementById("export-phases")?.addEventListener("click", () => openShotPreview("ledger"));
  // The map already answers the wheel and a drag; these exist because a trackpad
  // user has no way to discover how to get back to the framing it started in.
  document
    .getElementById("map-zoom-in")
    ?.addEventListener("click", () => ensureMap3D()?.zoomBy(1.25));
  document
    .getElementById("map-zoom-out")
    ?.addEventListener("click", () => ensureMap3D()?.zoomBy(1 / 1.25));
  document.getElementById("map-zoom-reset")?.addEventListener("click", () => ensureMap3D()?.resetView());
  document.getElementById("shot-close")?.addEventListener("click", closeShot);
  document.getElementById("shot-copy")?.addEventListener("click", () => void copyShot());
  document.getElementById("shot-save")?.addEventListener("click", saveShot);
  document.getElementById("shot-html")?.addEventListener("click", saveReportHtml);
  // Clicking the dimmed backdrop closes the preview; clicking its card does not.
  document.getElementById("shot-modal")?.addEventListener("click", (event) => {
    if (event.target === event.currentTarget) closeShot();
  });
  document.getElementById("house-modal-close")?.addEventListener("click", closeHouseModal);
  document.getElementById("house-modal")?.addEventListener("click", (event) => {
    if (event.target === event.currentTarget) closeHouseModal();
  });
  window.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    // Both dialogs are modal, so the topmost one is the only one Escape
    // should close — otherwise Escape behind the house detail would also drop
    // the screenshot preview underneath it.
    if (houseModalName) closeHouseModal();
    else closeShot();
  });
}

// ------------------------------------------------------ match report wall

/**
 * The report wall: the match being played right now, not a file the engine
 * freezes at the end. It draws the same independent measured populations as
 * the main wall, built by `buildTimingSlices`, and pairs them with the house table so the numbers
 * can be read against who is on the map while they are being measured.
 */
// ------------------------------------------------------------ log console

/** Rows kept in memory and in the DOM. The engine's ring holds more, but this
 *  is what the browser renders without stuttering. */
const LOG_LIMIT = 3000;
const LOG_LEVEL_NAMES = ["", "ERR", "WRN", "INF", "DBG", "TRC"];

let logLines: LogRecord[] = [];
let logPaused = false;
let logFollow = true;
let logLevel = 3;
let logQuery = "";
let logDropped = 0;
let logSearchTimer: number | null = null;

function logVisible(line: LogRecord): boolean {
  if (line.level > logLevel) return false;
  if (logQuery.length === 0) return true;
  return (
    line.message.toLowerCase().includes(logQuery) ||
    line.target.toLowerCase().includes(logQuery)
  );
}

function logRow(line: LogRecord): HTMLElement {
  const row = document.createElement("div");
  row.className = `log-row lv${line.level}`;
  const time = document.createElement("span");
  time.className = "t";
  time.textContent = `${(line.t_ms / 1000).toFixed(3)}s`;
  const level = document.createElement("span");
  level.className = "lv";
  level.textContent = LOG_LEVEL_NAMES[line.level] ?? "?";
  const target = document.createElement("span");
  target.className = "tg";
  target.textContent = line.target;
  target.title = line.target;
  const message = document.createElement("span");
  message.className = "msg";
  message.textContent = line.message;
  message.title = line.message;
  row.append(time, level, target, message);
  return row;
}

function updateLogCount(): void {
  const count = document.getElementById("log-count");
  if (!count) return;
  const list = document.getElementById("log-list");
  count.textContent = t("log.count", {
    shown: thousands(list ? list.childElementCount : 0),
    total: thousands(logLines.length),
  });
}

function appendLogRows(lines: LogRecord[]): void {
  const list = document.getElementById("log-list");
  if (!list) return;
  const fragment = document.createDocumentFragment();
  for (const line of lines) {
    if (logVisible(line)) fragment.append(logRow(line));
  }
  if (fragment.childElementCount === 0) return;
  list.append(fragment);
  // Trim from the top: the tail is what a reader is watching.
  while (list.childElementCount > LOG_LIMIT) list.firstElementChild?.remove();
  if (logFollow) list.scrollTop = list.scrollHeight;
  updateLogCount();
}

/** Redraw every visible row from the buffer: filter changes, view entry, resume. */
function rebuildLogView(): void {
  const list = document.getElementById("log-list");
  if (!list) return;
  const fragment = document.createDocumentFragment();
  for (const line of logLines) {
    if (logVisible(line)) fragment.append(logRow(line));
  }
  list.replaceChildren(fragment);
  if (logFollow) list.scrollTop = list.scrollHeight;
  updateLogCount();
}

/** Take one snapshot's worth of records. Paused only stops the drawing; the
 *  buffer keeps filling so resuming shows what arrived meanwhile. */
function acceptLogs(batch: LogBatch | null): void {
  if (!batch) return;
  if (batch.dropped > 0) {
    logDropped += batch.dropped;
    const marker = document.getElementById("log-dropped");
    if (marker) marker.textContent = t("log.dropped", { n: thousands(logDropped) });
  }
  if (batch.lines.length === 0) return;
  logLines.push(...batch.lines);
  if (logLines.length > LOG_LIMIT) {
    logLines.splice(0, logLines.length - LOG_LIMIT);
  }
  if (!logPaused && currentView === "logs") appendLogRows(batch.lines);
  else updateLogCount();
}

function toggleLogPause(): void {
  logPaused = !logPaused;
  const button = document.getElementById("log-pause");
  if (button) button.textContent = t(logPaused ? "log.resume" : "log.pause");
  if (!logPaused) rebuildLogView();
}

function clearLogs(): void {
  logLines = [];
  logDropped = 0;
  const marker = document.getElementById("log-dropped");
  if (marker) marker.textContent = "";
  rebuildLogView();
}

/** The stream arrives ~11×/s. Rebuilding a forty-row table that often reads as
 *  flicker rather than as live data, so the wall redraws at most four times a
 *  second and coalesces whatever arrived in between. */
const REPORT_RENDER_INTERVAL_MS = 250;
let reportRenderedAt = 0;
let reportRenderTimer: number | null = null;

// ---------------------------------------------------- whole-match timeline

/** Ticks the engine packs into one history sample (15 = one second at 15 Hz). */
const HISTORY_TICKS = 15;
/** Client-side ceiling; the engine ring keeps the same window. */
const HISTORY_LIMIT = 7200;
const HISTORY_W = 1000;
const HISTORY_H = 220;
const HISTORY_PAD = { l: 44, r: 12, t: 12, b: 18 };

interface HistoryPoint {
  tick: number;
  tick_ms: number;
  peak_ms: number;
  entities: number;
  houses: number;
  batch: PathBatchStats | null;
}

let historyPoints: HistoryPoint[] = [];
let historyLive: {
  firstTick: number;
  sum: number;
  n: number;
  peak: number;
  entities: number;
  houses: number;
  batch: PathBatchStats | null;
} | null = null;
let historyLo = 0;
let historyHi = -1;
let historySelection: number | null = null;
let historyDrag: [number, number] | null = null;

/** Report-grade samples: seeded by the backfill, extended by the newest one. */
let reportPoints: ReportPoint[] = [];
const REPORT_POINT_LIMIT = 1440;

function ingestReportPoints(snapshot: TelemetrySnapshot): void {
  if (snapshot.report_points && snapshot.report_points.length > 0) {
    reportPoints = snapshot.report_points.slice(-REPORT_POINT_LIMIT);
  }
  const latest = snapshot.report_point;
  if (latest && (!reportPoints.length || latest.tick > reportPoints[reportPoints.length - 1].tick)) {
    reportPoints.push(latest);
    if (reportPoints.length > REPORT_POINT_LIMIT) reportPoints.shift();
  }
}

/** The stored report-grade sample closest to `tick`, or null when none exist. */
function nearestReportPoint(tick: number): ReportPoint | null {
  let best: ReportPoint | null = null;
  for (const point of reportPoints) {
    if (!best || Math.abs(point.tick - tick) < Math.abs(best.tick - tick)) best = point;
  }
  return best;
}

/** Full match report for the sample nearest `tick`; null when none exists. */
function pointReportHtml(tick: number): string | null {
  const point = nearestReportPoint(tick);
  return point ? buildReportHtml(reportViewOf(point)) : null;
}

function pushHistoryPoint(point: HistoryPoint): void {
  historyPoints.push(point);
  if (historyPoints.length > HISTORY_LIMIT) historyPoints.shift();
  if (historyHi < 0) {
    historyLo = 0;
    historyHi = historyPoints.length - 1;
  } else if (historyHi >= historyPoints.length - 2) {
    historyHi = historyPoints.length - 1;
  }
}

/** Seed from the engine's ring on the first publish, then keep appending. */
function ingestHistory(snapshot: TelemetrySnapshot): void {
  if (snapshot.history && snapshot.history.length > 0) {
    historyPoints = snapshot.history.map((sample) => ({
      tick: sample.tick,
      tick_ms: sample.tick_ms,
      peak_ms: sample.peak_ms,
      entities: sample.entities,
      houses: sample.houses,
      batch: sample.path_batch,
    }));
    historyLive = null;
    historyLo = 0;
    historyHi = historyPoints.length - 1;
    historySelection = null;
  }
  const ms = snapshot.timing?.tick_ms;
  if (ms === undefined) return;
  if (!historyLive) {
    historyLive = {
      firstTick: snapshot.tick,
      sum: 0,
      n: 0,
      peak: 0,
      entities: snapshot.entities_total,
      houses: snapshot.houses.length,
      batch: snapshot.path_batch,
    };
  }
  historyLive.sum += ms;
  historyLive.n += 1;
  historyLive.peak = Math.max(historyLive.peak, ms);
  historyLive.entities = snapshot.entities_total;
  historyLive.houses = snapshot.houses.length;
  historyLive.batch = snapshot.path_batch;
  if (snapshot.tick - historyLive.firstTick + 1 >= HISTORY_TICKS) {
    pushHistoryPoint({
      tick: snapshot.tick,
      tick_ms: historyLive.sum / historyLive.n,
      peak_ms: historyLive.peak,
      entities: historyLive.entities,
      houses: historyLive.houses,
      batch: historyLive.batch,
    });
    historyLive = null;
  }
}

function drawHistoryChart(): void {
  const host = document.getElementById("history-chart");
  const detail = document.getElementById("history-detail");
  if (!host) return;
  const points = historyPoints.slice(historyLo, historyHi + 1);
  if (points.length === 0) {
    host.innerHTML = `<p class="hint">${escapeHtml(t("rep.historyEmpty"))}</p>`;
    if (detail) detail.textContent = "";
    return;
  }
  const max = Math.max(1, ...points.map((p) => Math.max(p.peak_ms, p.batch?.mean_ms ?? 0))) * 1.1;
  const x = (index: number): number =>
    HISTORY_PAD.l + (HISTORY_W - HISTORY_PAD.l - HISTORY_PAD.r) * (points.length < 2 ? 0.5 : index / (points.length - 1));
  const y = (ms: number): number =>
    HISTORY_H - HISTORY_PAD.b - (HISTORY_H - HISTORY_PAD.t - HISTORY_PAD.b) * (ms / max);
  let body = "";
  for (let step = 0; step <= 4; step += 1) {
    const yy = HISTORY_PAD.t + (HISTORY_H - HISTORY_PAD.t - HISTORY_PAD.b) * step / 4;
    body += `<line class="hgrid" x1="${HISTORY_PAD.l}" x2="${HISTORY_W - HISTORY_PAD.r}" y1="${yy.toFixed(1)}" y2="${yy.toFixed(1)}"/>`;
    body += `<text class="haxis" x="4" y="${(yy + 3).toFixed(1)}">${fmt(max * (1 - step / 4), 0)}</text>`;
  }
  for (let step = 0; step <= 5; step += 1) {
    const index = Math.round((points.length - 1) * step / 5);
    body += `<text class="haxis" x="${(x(index) - 16).toFixed(1)}" y="${HISTORY_H - 4}">t${thousands(points[index].tick)}</text>`;
  }
  // The peak band fills mean..peak, so one spike cannot rescale the mean line.
  const up = points.map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(p.tick_ms).toFixed(1)}`).join("");
  const down = points
    .map((p, i) => ({ p, i }))
    .reverse()
    .map(({ p, i }) => `L${x(i).toFixed(1)},${y(p.peak_ms).toFixed(1)}`)
    .join("");
  body += `<path class="hband" d="${up}${down}Z"/>`;
  const line = (value: (point: HistoryPoint) => number, color: string): string =>
    `<path class="hline" stroke="${color}" d="${points
      .map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(value(p)).toFixed(1)}`)
      .join("")}"/>`;
  // A dot on the axis marks every tick that has a full report-grade sample.
  const firstTick = points[0].tick;
  const lastTick = points[points.length - 1].tick;
  const span = Math.max(1, lastTick - firstTick);
  for (const mark of reportPoints) {
    if (mark.tick < firstTick || mark.tick > lastTick) continue;
    const px = HISTORY_PAD.l + (HISTORY_W - HISTORY_PAD.l - HISTORY_PAD.r) * ((mark.tick - firstTick) / span);
    body += `<circle class="hmark" cx="${px.toFixed(1)}" cy="${HISTORY_H - HISTORY_PAD.b - 2}" r="1.7"/>`;
  }
  body += line((p) => p.tick_ms, "#49b6ff");
  body += line((p) => p.batch?.mean_ms ?? 0, "#ff5d73");
  if (historySelection !== null) {
    const index = historySelection - historyLo;
    if (index >= 0 && index < points.length) {
      const point = points[index];
      body += `<line class="hsel" x1="${x(index).toFixed(1)}" x2="${x(index).toFixed(1)}" y1="${HISTORY_PAD.t}" y2="${HISTORY_H - HISTORY_PAD.b}"/>`;
      body += `<circle class="hdot" cx="${x(index).toFixed(1)}" cy="${y(point.tick_ms).toFixed(1)}" fill="#49b6ff"/>`;
    }
  }
  host.innerHTML = `<svg viewBox="0 0 ${HISTORY_W} ${HISTORY_H}" preserveAspectRatio="none">${body}</svg>`;
  if (detail) {
    const selected = historySelection === null ? historyPoints.length - 1 : historySelection;
    const point = points[Math.min(Math.max(selected - historyLo, 0), points.length - 1)];
    const rows: [string, string][] = [
      [t("rep.histTick"), `${fmt(point.tick_ms, 2)} ms`],
      [t("rep.histPeak"), `${fmt(point.peak_ms, 2)} ms`],
      [t("rep.histBatch"), point.batch ? `${fmt(point.batch.mean_ms, 2)} ms` : "—"],
      [t("rep.histEntities"), thousands(point.entities)],
      [t("rep.histPoints"), thousands(historyPoints.length)],
    ];
    // One line of facts: five short pairs stacked vertically pushed the chart's
    // own panel taller than the data it describes.
    detail.innerHTML =
      `<div class="history-facts">` +
      rows
        .map(([key, value]) =>
          `<span class="hf"><span class="hk">${escapeHtml(key)}</span><b>${escapeHtml(value)}</b></span>`)
        .join("") +
      `</div><p class="note">${escapeHtml(t("rep.histBatchNote"))}</p>`;
  }
}

let bubbleTimer: number | null = null;

/** A short-lived bubble under an anchor: used where a control is not ready. */
function showBubble(anchor: HTMLElement, text: string): void {
  let bubble = document.getElementById("ui-bubble");
  if (!bubble) {
    bubble = document.createElement("div");
    bubble.id = "ui-bubble";
    bubble.className = "ui-bubble";
    document.body.appendChild(bubble);
  }
  bubble.textContent = text;
  bubble.hidden = false;
  const rect = anchor.getBoundingClientRect();
  bubble.style.left = `${Math.min(window.innerWidth - 220, Math.max(8, rect.left))}px`;
  bubble.style.top = `${rect.bottom + 6}px`;
  if (bubbleTimer !== null) window.clearTimeout(bubbleTimer);
  bubbleTimer = window.setTimeout(() => {
    bubble!.hidden = true;
    bubbleTimer = null;
  }, 2600);
}

function setupHistoryChart(): void {
  const host = document.getElementById("history-chart");
  if (!host) return;
  const tip = document.createElement("div");
  tip.className = "history-tip";
  tip.hidden = true;
  // The chart redraws through `innerHTML`; the tip must live outside it.
  (host.parentElement ?? host).appendChild(tip);
  const px = (event: PointerEvent): number => {
    const svg = host.querySelector("svg");
    if (!svg) return 0;
    return (event.clientX - svg.getBoundingClientRect().left) / svg.getBoundingClientRect().width * HISTORY_W;
  };
  const indexAt = (value: number): number => {
    const count = historyHi - historyLo + 1;
    if (count <= 0) return 0;
    return Math.max(0, Math.min(count - 1,
      Math.round((value - HISTORY_PAD.l) / (HISTORY_W - HISTORY_PAD.l - HISTORY_PAD.r) * (count - 1))));
  };
  host.addEventListener("pointerdown", (event) => {
    const value = px(event);
    historyDrag = [value, value];
  });
  host.addEventListener("pointermove", (event) => {
    if (historyDrag) {
      historyDrag[1] = px(event);
      return;
    }
    if (historyHi < historyLo) return;
    const index = historyLo + indexAt(px(event));
    const point = historyPoints[index];
    if (!point) return;
    tip.innerHTML =
      `<b>t${thousands(point.tick)}</b> · ${escapeHtml(t("rep.histTick"))} ${fmt(point.tick_ms, 2)} ms` +
      ` · ${escapeHtml(t("rep.histPeak"))} ${fmt(point.peak_ms, 2)} ms<br>` +
      `${escapeHtml(t("rep.histBatch"))} ${point.batch ? `${fmt(point.batch.mean_ms, 2)} ms` : "—"}` +
      ` · ${escapeHtml(t("rep.histEntities"))} ${thousands(point.entities)}`;
    const panelRect = (host.parentElement ?? host).getBoundingClientRect();
    let left = event.clientX - panelRect.left + 14;
    if (left + 240 > panelRect.width) left = event.clientX - panelRect.left - 250;
    tip.style.left = `${Math.max(4, left)}px`;
    tip.style.top = `${Math.max(4, event.clientY - panelRect.top - 44)}px`;
    tip.hidden = false;
  });
  host.addEventListener("pointerleave", () => {
    tip.hidden = true;
  });
  host.addEventListener("pointerup", (event) => {
    const value = historyDrag ? px(event) : 0;
    if (historyDrag && Math.abs(historyDrag[1] - historyDrag[0]) > 14) {
      const from = indexAt(Math.min(historyDrag[0], historyDrag[1]));
      const to = indexAt(Math.max(historyDrag[0], historyDrag[1]));
      if (to > from) {
        const lo = historyLo;
        historyHi = lo + to;
        historyLo = lo + from;
        historySelection = null;
      }
    } else {
      historySelection = historyLo + indexAt(value);
    }
    historyDrag = null;
    drawHistoryChart();
  });
  host.addEventListener("dblclick", () => {
    historyLo = 0;
    historyHi = historyPoints.length - 1;
    historySelection = null;
    drawHistoryChart();
  });
  document.getElementById("report-point")?.addEventListener("click", reportSelectedPoint);
  document.getElementById("export-history")?.addEventListener("click", (event) =>
    showBubble(event.currentTarget as HTMLElement, t("rep.notImplemented")));
}

/** A one-sample report: the 1 Hz summary plus its neighbours.
 *
 *  It deliberately is not the full report: houses, production and the phase
 *  ledger are measured live only, so history holds just these numbers. */
/** Download the report for the selected point: the full match report when a
 *  report-grade sample exists near it, else the 1 Hz summary. */
function reportSelectedPoint(): void {
  if (historyPoints.length === 0) return;
  const index = Math.max(0, Math.min(
    historySelection === null ? historyPoints.length - 1 : historySelection,
    historyPoints.length - 1));
  const tick = historyPoints[index].tick;
  const sample = nearestReportPoint(tick);
  if (sample) {
    // Same preview as the live report: PNG, clipboard and HTML save.
    openShotPreview("report", reportViewOf(sample));
    return;
  }
  const button = document.getElementById("report-point");
  if (button) showBubble(button, t("rep.noReportPoint"));
}


function renderReportSoon(): void {
  const wait = REPORT_RENDER_INTERVAL_MS - (performance.now() - reportRenderedAt);
  if (wait <= 0) {
    reportRenderedAt = performance.now();
    renderReport();
    return;
  }
  if (reportRenderTimer !== null) return;
  reportRenderTimer = window.setTimeout(() => {
    reportRenderTimer = null;
    reportRenderedAt = performance.now();
    renderReport();
  }, wait);
}

function renderReport(): void {
  const facts = document.getElementById("report-facts");
  const table = document.getElementById("report-houses");
  const foot = document.getElementById("report-foot");
  if (!facts || !table || !foot) return;
  const snapshot = lastSnapshot;
  const profile = snapshot?.profile ?? null;
  const render = snapshot?.render ?? null;
  const headLine = document.getElementById("report-head");
  const note = document.getElementById("report-note");
  const housesPanel = table.closest("section");
  const phasesTable = document.getElementById("report-phases");
  const phasesPanel = phasesTable?.closest("section");
  const { sim, render: rendered, simTotal, renderTotal } = buildTimingSlices(profile, render);
  drawPie("rep-pie-frame", "rep-frame-legend", rendered, renderTotal);
  drawPie("rep-pie-sim", "rep-sim-legend", sim, simTotal);
  document.getElementById("rep-frame-total")!.textContent = measuredCost(renderTotal, render?.frames ?? 0);
  document.getElementById("rep-sim-total")!.textContent = measuredCost(simTotal, profile?.frames ?? 0);
  const hasSamples = !!(profile?.frames || render?.frames);
  if (housesPanel) housesPanel.hidden = !snapshot;
  if (phasesPanel) phasesPanel.hidden = !hasSamples;
  document.getElementById("report")?.classList.toggle("only-pies", !snapshot);
  renderPhaseTable(phasesTable, profile, render, snapshot?.path_batch ?? null);
  drawHistoryChart();
  if (!snapshot) {
    facts.innerHTML = `<div class='row'><span>${t("rep.waiting")}</span></div>`;
    table.textContent = "";
    foot.textContent = "";
    if (headLine) headLine.textContent = "";
    if (note) note.textContent = "";
    return;
  }
  const rows: [string, string][] = [
    [t("hdr.map"), `${fmt(snapshot.map_width, 0)}x${fmt(snapshot.map_height, 0)}`],
    [t("rep.tickP50"), snapshot.timing ? `${fmt(snapshot.timing.p50_ms, 2)} ms` : "—"],
    [t("rep.sim"), measuredCost(profile?.mean_frame_ms ?? 0, profile?.frames ?? 0)],
    [t("rep.peak"), measuredCost(profile?.peak_frame_ms ?? 0, profile?.frames ?? 0)],
    [t("rep.frames"), thousands(profile?.frames ?? 0)],
    [t("rep.renderSample"), measuredCost(renderTotal, render?.frames ?? 0)],
    [t("rep.renders"), thousands(render?.frames ?? 0)],
  ];
  facts.innerHTML = rows
    .map(([key, value]) => `<div class='row'><span>${key}</span><b>${value}</b></div>`)
    .join("");
  const head =
    `<tr><th>${t("th.house")}</th><th>${t("rep.kind")}</th>` +
    `<th>${t("rep.buildings")}</th><th>${t("rep.credits")}</th><th>${t("rep.score")}</th></tr>`;
  table.innerHTML = head + snapshot.houses
    .filter((house) => !isPassiveHouse(house))
    .map((house) => `<tr><td>${escapeHtml(house.name)}</td>` +
      `<td>${house.is_human ? t("tag.human") : t("tag.ai")}</td>` +
      `<td>${fmt(house.counts.structure, 0)}</td>` +
      `<td>${fmt(house.credits, 0)}</td><td>${fmt(house.score, 0)}</td></tr>`)
    .join("");
  if (headLine) {
    const parts = [
      `tick <b>${thousands(snapshot.tick)}</b>`,
      `${t("rep.fps")} <b>${fmt(snapshot.timing?.fps ?? 0, 0)}</b> fps`,
      `<b>${fmt(snapshot.timing?.tick_hz ?? 0, 1)}</b> tick/s`,
    ];
    headLine.innerHTML = parts.join(`<span class="sep">·</span>`);
  }
  if (note) note.textContent = t("rep.note");
  foot.textContent = t("rep.source");
}

// ---------------------------------------------------------------- socket

let statusOpen = false;
let statusOverride: { key: string; vars?: Record<string, string | number> } | null = null;
let sessionToken: string | null = null;
let reconnectTimer: number | null = null;
let connectionTimer: number | null = null;
let reconnectAttempts = 0;
let disposed = false;
const SOCKET_PROTOCOL = "vera-telemetry.v1";
const MAX_RECONNECT_ATTEMPTS = 6;
let everConnected = false;
/** Snapshots seen since the last second closed, and the rate that second made. */
let snapTicks = 0;
let snapHz = 0;

function renderStatus(): void {
  // The pill's colour is the connection state, so the state class goes on the
  // pill itself and not only on the dot inside it.
  document.getElementById("status")!.classList.toggle("open", statusOpen);
  document.getElementById("dot")!.classList.toggle("open", statusOpen);
  document.getElementById("statustext")!.textContent =
    (statusOverride ? t(statusOverride.key, statusOverride.vars) : null) ??
    (statusOpen
      ? snapHz > 0
        ? t("status.liveHz", { n: snapHz })
        : t("status.live")
      : everConnected
        ? t("status.reconnecting")
        : t("status.connecting"));
}

function setStatus(open: boolean, key?: string, vars?: Record<string, string | number>): void {
  statusOpen = open;
  if (open) everConnected = true;
  statusOverride = key ? { key, vars } : null;
  renderStatus();
}

/** The link keeps the credential in this tab so a reload reconnects. */
const SESSION_TOKEN_KEY = "vera20k.session";

/**
 * Consume the session link and keep its credential for this tab only.
 *
 * The fragment is removed from the address bar, and the credential never
 * enters a request, history or another tab: `sessionStorage` is per-tab and
 * dies with it. A link without a token (a reload, or a bare URL) falls back
 * to the stored credential; a stale one from a previous game simply fails to
 * connect, and the game's printed link replaces it.
 */
function consumeSessionToken(): string | null {
  const valid = (value: string | null): value is string =>
    value !== null && /^[a-f0-9]{64}$/i.test(value);
  const url = new URL(location.href);
  const tokens = new URLSearchParams(url.hash.slice(1)).getAll("token");
  if (url.hash) {
    url.hash = "";
    try {
      history.replaceState(history.state, "", url.pathname + url.search);
    } catch {
      // Do not connect if the credential could not be removed from history.
      return null;
    }
  }
  if (tokens.length === 1 && valid(tokens[0])) {
    try {
      sessionStorage.setItem(SESSION_TOKEN_KEY, tokens[0]);
    } catch {
      // Storage denied: the link still works for this load.
    }
    return tokens[0];
  }
  try {
    const stored = sessionStorage.getItem(SESSION_TOKEN_KEY);
    return valid(stored) ? stored : null;
  } catch {
    return null;
  }
}

function clearConnectionTimer(): void {
  if (connectionTimer !== null) window.clearTimeout(connectionTimer);
  connectionTimer = null;
}

function scheduleReconnect(): void {
  if (disposed || !sessionToken || reconnectTimer !== null) return;
  if (reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) {
    setStatus(false, "status.connectionFailed");
    return;
  }
  const delay = Math.min(30_000, 1000 * 2 ** reconnectAttempts);
  reconnectAttempts += 1;
  setStatus(false, "status.reconnecting");
  reconnectTimer = window.setTimeout(() => {
    reconnectTimer = null;
    connect();
  }, delay);
}

function connect(): void {
  if (disposed) return;
  if (!sessionToken || !["http:", "https:"].includes(location.protocol)) {
    setStatus(false, "status.sessionRequired");
    return;
  }
  if (socket && (socket.readyState === WebSocket.CONNECTING || socket.readyState === WebSocket.OPEN)) {
    return;
  }
  const endpoint = new URL("/ws", location.href);
  endpoint.protocol = location.protocol === "https:" ? "wss:" : "ws:";
  let next: WebSocket;
  try {
    next = new WebSocket(endpoint, [SOCKET_PROTOCOL, `vera-auth.${sessionToken}`]);
  } catch {
    // Never retry without authentication or with a different host.
    setStatus(false, "status.connectionFailed");
    return;
  }
  socket = next;
  let terminalFailure = false;
  connectionTimer = window.setTimeout(() => {
    connectionTimer = null;
    if (socket === next && next.readyState === WebSocket.CONNECTING) next.close();
  }, 10_000);
  next.onopen = () => {
    if (disposed || socket !== next) return;
    clearConnectionTimer();
    if (next.protocol !== SOCKET_PROTOCOL) {
      terminalFailure = true;
      setStatus(false, "status.sessionRejected");
      next.close(1002, "Unexpected protocol");
      return;
    }
    setStatus(true);
  };
  next.onclose = (event) => {
    if (socket !== next) return;
    clearConnectionTimer();
    socket = null;
    updateDebugTools(null, false);
    if (disposed) return;
    if (terminalFailure || event.code === 1008 || event.code === 1002) {
      setStatus(false, "status.sessionRejected");
      return;
    }
    scheduleReconnect();
  };
  next.onerror = () => {
    if (socket === next && !disposed) next.close();
  };
  next.onmessage = (event) => {
    if (disposed || socket !== next || typeof event.data !== "string") return;
    let snapshot: TelemetrySnapshot;
    try {
      snapshot = JSON.parse(event.data) as TelemetrySnapshot;
    } catch {
      return;
    }
    if (!snapshot || typeof snapshot !== "object") return;
    if (snapshot.version !== SCHEMA_VERSION) {
      updateDebugTools(null, false);
      setStatus(true, "status.unsupported", { v: String(snapshot.version) });
      return;
    }
    if (!Array.isArray(snapshot.houses) || !Array.isArray(snapshot.spots)) return;
    reconnectAttempts = 0;
    lastSnapshot = snapshot;
    updateDebugTools(snapshot, true);
    snapTicks += 1;
    acceptLogs(snapshot.logs);
    setStatus(true);
    // The detail view's history must keep flowing while the data wall shows,
    // or its charts restart at every switch.
    sampleHouseEconomy(snapshot);
    // The wall draws both of its pieces per snapshot. The chart only ran from
    // `renderDetail`, which is a view switch or a resize, so the cards moved
    // under a frozen trace.
    if (currentView === "detail") {
      drawEconomyChart(snapshot);
      renderDetailCards(snapshot);
    }
    // The modal repaints on its own, not through `renderDetail`: that path only
    // runs on a view switch or a resize, which froze the production gauges and
    // the squad list until the modal was reopened. It self-guards on an open
    // modal, so this costs nothing while none is showing.
    renderHouseModal();
    ensureMap3D()?.draw(snapshot);
    drawMapLegend(snapshot);
    if (snapshot.timing) {
      chartSamples.push({ tick: snapshot.timing.tick_ms, p99: snapshot.timing.p99_ms });
      if (chartSamples.length > CHART_WINDOW) chartSamples.shift();
    }
    updateHeader(snapshot);
    updateCost(snapshot);
    updateProcess(snapshot.process);
    updateTerrain(snapshot.debug);
    updatePopulation(snapshot);
    updateFrameProfile(snapshot.profile, snapshot.render);
    ingestHistory(snapshot);
    ingestReportPoints(snapshot);
    drawChart(snapshot.timing);
    // Live snapshots, so the report wall just rides the stream: no polling,
    // and no waiting for a match to end before it has anything to show.
    if (currentView === "report") renderReportSoon();
  };
}

// ------------------------------------------------------------ bootstrap

applyStaticTranslations();
syncSwitcher();
renderStatus();
setupViews();
setupHistoryChart();
setupPager();
setupDebugTools((envelope) => {
  if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(envelope));
});

for (const button of document.querySelectorAll<HTMLElement>("#lang [data-locale]")) {
  button.addEventListener("click", () => {
    setLocale(button.dataset.locale as Locale);
  });
}

// A language switch re-renders every dynamic panel from the latest snapshot;
// the 3D scene is language-free and keeps running.
onLocaleChange(() => {
  syncSwitcher();
  renderStatus();
  // The detail cards bake labels into their DOM; force a structural rebuild.
  detailCardKeys = [];
  if (currentView === "detail" && lastSnapshot) renderDetailCards(lastSnapshot);
  renderHouseModal();
  if (currentView === "report") renderReportSoon();
  if (!lastSnapshot) return;
  updateTerrain(lastSnapshot.debug);
  updatePopulation(lastSnapshot);
  updateFrameProfile(lastSnapshot.profile, lastSnapshot.render);
  drawChart(lastSnapshot.timing);
});

window.addEventListener("resize", () => {
  if (currentView === "detail" && lastSnapshot) renderDetail(lastSnapshot);
});

// One-second window on the push rate. A 1s period is already past every
// hidden-tab timer clamp, so the reading stays true when the tab is in the
// background.
const statusTimer = window.setInterval(() => {
  snapHz = snapTicks;
  snapTicks = 0;
  renderStatus();
}, 1000);

sessionToken = consumeSessionToken();
connect();

window.addEventListener("pagehide", () => {
  disposed = true;
  updateDebugTools(null, false);
  sessionToken = null;
  window.clearInterval(statusTimer);
  if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
  reconnectTimer = null;
  clearConnectionTimer();
  if (reportRenderTimer !== null) window.clearTimeout(reportRenderTimer);
  reportRenderTimer = null;
  if (logSearchTimer !== null) window.clearTimeout(logSearchTimer);
  logSearchTimer = null;
  const previous = socket;
  socket = null;
  previous?.close(1000, "Dashboard closed");
});

window.addEventListener("pageshow", (event) => {
  if (event.persisted && disposed) setStatus(false, "status.sessionRequired");
});

/** Read-only handle for the screenshot harness: headless browsers refuse
 *  downloads, so this is the only way to assert on the generated report. It
 *  exposes no state the panel does not already render. */
(window as unknown as { veraReport?: () => string | null }).veraReport = () =>
  buildReportHtml(lastSnapshot);

/** Read-only handle for the screenshot harness, same contract as `veraReport`:
 *  the full report for the stored point nearest `tick`. */
(window as unknown as { veraPointReport?: (tick: number) => string | null }).veraPointReport =
  (tick: number) => pointReportHtml(tick);
