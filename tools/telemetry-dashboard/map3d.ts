/**
 * 3D battlefield overview (three.js).
 *
 * The target is the approved mockup, so its projection and shading model are
 * reproduced rather than approximated:
 *
 *  - **Parallel projection, 2:1 diamond.** The mockup maps cell (gx, gy) to
 *    screen ((gx-gy)*25, (gx+gy)*12.5). That ratio is `sin(elevation) = 0.5`, so
 *    the camera sits at 30° and an orthographic frustum — a perspective cone
 *    cannot hold parallel edges and the diamond goes crooked at the far corner.
 *  - **Terraced relief, RA2's own height quantum.** The plate is the map's own
 *    elevation grid, but stacked the way the game stacks it: one discrete level
 *    per cell, 15 px of rise against a 30 px tile (`HEIGHT_STEP` / `TILE_HEIGHT`
 *    in `src/map/terrain.rs`), so every cell keeps a flat top of its own and a
 *    boundary whose two cells differ drops a step face between them. A raised
 *    area therefore reads as a plateau with cliff edges instead of a smooth 3D
 *    hill. The grid, its every-fourth emphasis and its boundary are baked once
 *    into a canvas texture applied to those flat tops, so the glowing cell lines
 *    follow the terraces. Three land features ride the same grid: a ramp cell
 *    (`slope_type != 0`) tilts its own top toward its lower neighbours instead
 *    of dropping a step face; a water cell (`LandType::Water`) takes a solid
 *    blue top with no grid; and a structural bridge cell lifts a steel deck
 *    `deck_rises` levels above its ground, with side faces down to it.
 *  - **Unlit, per-category solids.** Entities are flat three-tone solids, not
 *    shaded geometry, so their boxes carry baked per-face vertex colours and an
 *    unlit material. Each category gets its own silhouette (post, hull+turret,
 *    wing, plinth+tower) so a glance can tell infantry from armour from air from
 *    buildings. The house colour still multiplies in through `instanceColor`.
 *
 * The callout layer is a DOM overlay, not sprites: it has to carry the panel's
 * own text and stay crisp when the map is dragged. Every chip is anchored to a
 * cell the engine already reports — a selected unit, a house's base centre — so
 * nothing here invents a position. Hover and click are DOM + raycast: hovering
 * an entity outlines it and shows a chip, clicking re-centres the camera on it.
 *
 * Orientation note: azimuth `pi/4` is 45° off the radar axes, which is what makes
 * the diamond. It is not the in-game minimap reading (that is azimuth `pi/2`,
 * where +rx runs right and +ry runs down); drag-rotate is live, so the check
 * against the game is one drag away. Right-drag pans, wheel zooms, click focuses.
 *
 * Schema mirror of `src/sim/telemetry.rs`; keep both in step.
 */

import * as THREE from "./vendor/three.module.js";
import { t } from "./i18n.js";
import { DebugPanel, EntityCategory, EntitySpot, HousePopulation, TelemetrySnapshot } from "./types.js";

/** InstancedMesh capacity per category: the project's 20k-unit target, so a
 *  single-category world can still field the whole capped spot list. */
const MAX_SPOTS = 20000;
/** One elevation level's visual offset, in world units. Shared by the terrain
 *  surface and the entity boxes, so a unit stands exactly on its cell. RA2's
 *  own quantum: a level raises a cell by 15 px against a 30 px tile, which is
 *  this half cell — a discrete step, not a height field to interpolate. */
const LEVEL_STEP = 0.5;
/** Step faces between terraces are rock in RA2, not floor: a dark tone the same
 *  sun shades, so a cliff reads as a wall instead of glowing grid. */
const CLIFF_TONE = 0x141c2e;
/** Water is a land type, not a height: `LandType::Water` (`2`) in
 *  `src/rules/terrain_rules.rs`. Water tops carry their own colour band and no
 *  land grid, so a coastline reads at a glance. */
const WATER_LAND_TYPE = 2;
type Rgb = readonly [number, number, number];
/** Land elevation ramp: dark low ground to mossy high ground. The texture is
 *  multiplied by the Lambert sun, so these stay desaturated. */
const LAND_RAMP: readonly Rgb[] = [
  [9, 20, 21],
  [14, 31, 28],
  [21, 44, 38],
  [30, 60, 50],
  [41, 78, 63],
  [54, 95, 76],
];
/** Deep, mid and shallow water, by distance from land. */
const WATER_RAMP: readonly Rgb[] = [
  [7, 28, 45],
  [15, 58, 82],
  [30, 96, 120],
];
/** One-cell beach and ramp tint. */
const SHORE_SAND: Rgb = [78, 72, 48];
const RAMP_TINT: Rgb = [70, 92, 70];
const NEIGHBORS = [[1, 0], [-1, 0], [0, 1], [0, -1]] as const;
/** A structural bridge deck is lifted `deck_rises` levels above its ground and
 *  drawn in steel, so a high bridge reads as a raised span over its own
 *  ground instead of flat terrain. Low, ground-level bridges have no rise and
 *  no steel top. */
const DECK_TONE = 0x4b5560;
/** Per-category box scale (world units, one cell = 1). Height is not here: the
 *  geometry's own base offset sets where the solid meets the ground. */
const CATEGORY_SCALE: Record<EntityCategory, number> = {
  Infantry: 0.3,
  Unit: 0.42,
  Aircraft: 0.36,
  Structure: 2.2,
};
/** Distance from a geometry's origin to its base, so the solid sits on the
 *  plate instead of half-sinking into it. */
const CATEGORY_HEIGHT: Record<EntityCategory, number> = {
  Infantry: 0.45,
  Unit: 0.45,
  Aircraft: 0.175,
  Structure: 0.5,
};
/** Structures are washed toward their house colour so they read as buildings;
 *  a heavy wash turned every neutral building into a white blob. */
const STRUCTURE_WASH = 0.28;
/** Chips are capped: a 20k-unit selection would bury the map. */
const MAX_CALLOUTS = 8;
/** Camera elevation, from the mockup's 2:1 ground ratio. */
const ISO_ELEVATION = Math.asin(0.5);
/** 45° off the radar axes: the diamond. */
const ISO_AZIMUTH = Math.PI / 4;
/** Fixed stand-off. An orthographic frustum ignores distance for size, so this
 *  only has to clear the map for the near and far planes. */
const CAMERA_DISTANCE = 600;
/** Fraction of the stage the diamond may fill; the rest is breathing room. */
const FIT = 0.88;
/** An entity marker keeps at least this many screen pixels; a 0.42-cell unit
 *  is sub-pixel when the whole 200x200 map fills the panel. */
const MIN_MARKER_PX = 8.0;
/** Fallback palette for houses the engine gave no colour (neutral, orphan). */
const HOUSE_PALETTE = ["#8a94a3", "#b04aff", "#e8c34a", "#37e0b8", "#ff5555", "#4a9eff"];
/** Entities with no house row (owner_index sentinel). */
const ORPHAN_COLOR = "#6b7a8c";
/** Every category, in a stable order for the per-category meshes. */
const CATEGORIES: EntityCategory[] = ["Infantry", "Unit", "Aircraft", "Structure"];

/** `[r, g, b]` (0-255) -> a CSS colour three.js converts from sRGB. */
function houseColor(
  house: { color: [number, number, number] | null } | undefined,
  index: number,
): string {
  if (house?.color) {
    const [r, g, b] = house.color;
    return `rgb(${r},${g},${b})`;
  }
  return HOUSE_PALETTE[index % HOUSE_PALETTE.length];
}

/** Mix an `rgb(...)`/`#rrggbb` colour toward white, for the structure wash. */
function wash(color: string, amount: number): string {
  const m = /rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(color) ||
    /^#([\da-f]{2})([\da-f]{2})([\da-f]{2})$/i.exec(color);
  if (!m) return color;
  const [r, g, b] = [1, 2, 3].map((group) => {
    const raw = m[group].length === 2 ? parseInt(m[group], 16) : Number(m[group]);
    return Math.round(raw + (255 - raw) * amount);
  });
  return `rgb(${r},${g},${b})`;
}

function clampInt(value: number, low: number, high: number): number {
  return Math.min(Math.max(Math.floor(value), low), high);
}

function mixRgb(a: Rgb, b: Rgb, t: number): Rgb {
  const k = Math.min(1, Math.max(0, t));
  return [
    Math.round(a[0] + (b[0] - a[0]) * k),
    Math.round(a[1] + (b[1] - a[1]) * k),
    Math.round(a[2] + (b[2] - a[2]) * k),
  ];
}

function rampRgb(stops: readonly Rgb[], t: number): Rgb {
  if (stops.length === 0) return [0, 0, 0];
  if (stops.length === 1) return stops[0];
  const x = Math.min(1, Math.max(0, t)) * (stops.length - 1);
  const index = Math.min(stops.length - 2, Math.floor(x));
  return mixRgb(stops[index], stops[index + 1], x - index);
}

function cssRgb(color: Rgb): string {
  return `rgb(${color[0]},${color[1]},${color[2]})`;
}

/** Bake a flat tone per vertex from its normal, for the unlit material: a top
 *  face reads bright, a bottom face dark, the sides in between. This is the
 *  same three-tone cue the original box carried, now for any silhouette. */
function toneByNormal(source: THREE.BufferGeometry, tone: number): THREE.BufferGeometry {
  const geo = source.index ? source.toNonIndexed() : source;
  const normal = geo.getAttribute("normal");
  const count = normal.count;
  const colors = new Float32Array(count * 3);
  for (let i = 0; i < count; i++) {
    const ny = normal.getY(i);
    const shade = ny > 0.5 ? Math.min(1, tone + 0.22) : ny < -0.5 ? Math.max(0.12, tone - 0.34) : tone;
    colors[i * 3] = shade;
    colors[i * 3 + 1] = shade;
    colors[i * 3 + 2] = shade;
  }
  geo.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  return geo;
}

/** One translated unit box for a composite silhouette. */
function boxPart(w: number, h: number, d: number, y: number, tone: number): THREE.BufferGeometry {
  const geo = new THREE.BoxGeometry(w, h, d);
  geo.translate(0, y, 0);
  return toneByNormal(geo, tone);
}

/** A box offset on all three axes, for tracks, barrels and annexes. */
function boxAt(
  w: number,
  h: number,
  d: number,
  x: number,
  y: number,
  z: number,
  tone: number,
): THREE.BufferGeometry {
  const geo = new THREE.BoxGeometry(w, h, d);
  geo.translate(x, y, z);
  return toneByNormal(geo, tone);
}

/** Low-poly cylinder: turrets, helmet bands, masts. */
function cylinderPart(
  radiusTop: number,
  radiusBottom: number,
  height: number,
  y: number,
  tone: number,
  segments = 6,
): THREE.BufferGeometry {
  const geo = new THREE.CylinderGeometry(radiusTop, radiusBottom, height, segments);
  geo.translate(0, y, 0);
  return toneByNormal(geo, tone);
}

/** Low-poly head. */
function spherePart(radius: number, y: number, tone: number): THREE.BufferGeometry {
  const geo = new THREE.IcosahedronGeometry(radius, 0);
  geo.translate(0, y, 0);
  return toneByNormal(geo, tone);
}

/** Four-sided hip roof; the 45° turn puts the pyramid's ridges over the walls. */
function roofPart(radius: number, height: number, y: number, tone: number): THREE.BufferGeometry {
  const geo = new THREE.ConeGeometry(radius, height, 4);
  geo.rotateY(Math.PI / 4);
  geo.translate(0, y, 0);
  return toneByNormal(geo, tone);
}

/** Concatenate toned parts into one non-indexed geometry for a category. */
function mergeParts(parts: THREE.BufferGeometry[]): THREE.BufferGeometry {
  const position: number[] = [];
  const normal: number[] = [];
  const color: number[] = [];
  for (const part of parts) {
    const p = part.getAttribute("position");
    const n = part.getAttribute("normal");
    const c = part.getAttribute("color");
    for (let i = 0; i < p.count; i++) {
      position.push(p.getX(i), p.getY(i), p.getZ(i));
      normal.push(n.getX(i), n.getY(i), n.getZ(i));
      color.push(c.getX(i), c.getY(i), c.getZ(i));
    }
  }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute("position", new THREE.Float32BufferAttribute(position, 3));
  geo.setAttribute("normal", new THREE.Float32BufferAttribute(normal, 3));
  geo.setAttribute("color", new THREE.Float32BufferAttribute(color, 3));
  return geo;
}

/** One distinct silhouette per category, each centred on the origin and sized so
 *  its base sits at `-CATEGORY_HEIGHT` — a unit box scaled and placed by the
 *  same rule still stands on its cell. */
function categoryGeometry(category: EntityCategory): THREE.BufferGeometry {
  switch (category) {
    case "Infantry":
      // Legs, torso, head with a dark helmet band, and a slung rifle.
      return mergeParts([
        boxAt(0.09, 0.2, 0.1, -0.07, -0.34, 0.02, 0.58),
        boxAt(0.09, 0.2, 0.1, 0.07, -0.34, 0.02, 0.58),
        boxPart(0.24, 0.46, 0.2, -0.22, 0.78),
        spherePart(0.16, 0.12, 0.95),
        cylinderPart(0.18, 0.18, 0.08, 0.22, 0.5, 8),
        boxAt(0.06, 0.06, 0.46, 0.16, 0.02, 0.2, 0.4),
      ]);
    case "Unit":
      // Tracked hull, hexagonal turret, gun barrel and a hatch.
      return mergeParts([
        boxAt(0.24, 0.26, 1.5, -0.42, -0.32, 0, 0.2),
        boxAt(0.24, 0.26, 1.5, 0.42, -0.32, 0, 0.2),
        boxPart(0.94, 0.32, 1.25, -0.13, 0.82),
        cylinderPart(0.32, 0.36, 0.28, 0.19, 0.98, 6),
        boxAt(0.1, 0.1, 0.8, 0, 0.22, 0.58, 0.5),
        boxAt(0.16, 0.08, 0.16, 0.18, 0.35, -0.14, 0.7),
      ]);
    case "Aircraft":
      // Fuselage, swept wing pair, tailplane and fin: a plane silhouette.
      return mergeParts([
        boxPart(0.34, 0.24, 1.5, -0.055, 0.9),
        boxPart(1.8, 0.07, 0.42, -0.02, 0.68),
        boxAt(0.7, 0.06, 0.3, 0, -0.02, -0.55, 0.62),
        boxAt(0.08, 0.4, 0.3, 0, 0.16, -0.55, 0.82),
        boxAt(0.2, 0.16, 0.22, 0, -0.02, 0.8, 0.95),
      ]);
    case "Structure":
      // Foundation, walls, hip roof, side annex, door and rooftop vent.
      return mergeParts([
        boxPart(1.15, 0.24, 1.15, -0.38, 0.35),
        boxPart(0.92, 0.5, 0.92, -0.01, 0.8),
        roofPart(0.74, 0.32, 0.4, 0.6),
        boxAt(0.36, 0.35, 0.5, -0.34, -0.085, 0.26, 0.68),
        boxAt(0.3, 0.28, 0.04, 0, -0.12, 0.47, 0.22),
        boxAt(0.12, 0.24, 0.12, 0.26, 0.44, -0.26, 0.5),
      ]);
  }
}

interface CellBounds {
  minX: number;
  minZ: number;
  maxX: number;
  maxZ: number;
}

export interface Map3D {
  draw(snapshot: TelemetrySnapshot): void;
  /** Wheel/button zoom: `factor` > 1 enlarges the map. */
  zoomBy(factor: number): void;
  /** Back to the framing and orientation this map size was laid out for. */
  resetView(): void;
  dispose(): void;
}

export function createMap3D(
  canvas: HTMLCanvasElement,
  calloutLayer?: HTMLElement | null,
): Map3D | null {
  let renderer: THREE.WebGLRenderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
  } catch {
    // No WebGL (headless preview, blocklisted GPU): the data panels still work.
    return null;
  }
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  // Transparent: the panel's CSS gradient is the sky.
  renderer.setClearColor(0x000000, 0);

  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.5, 4000);

  const orbit = { azimuth: ISO_AZIMUTH, elevation: ISO_ELEVATION, zoom: 1 };
  const orbitTarget = new THREE.Vector3();
  /** The map's own size; the plate always spans this. */
  let extent = { w: 64, h: 64 };
  /** Framed window inside the map: what Reset and auto-fit bring into view. */
  const view = { w: 64, h: 64, cx: 32, cz: 32 };
  let aspect = 1;
  /** Follow the action until the player drags, zooms or clicks; Reset re-arms it. */
  let latestSnapshot: TelemetrySnapshot | null = null;
  /** The default frame follows the action once, then stays put until Reset. */
  let framedToAction = false;

  // Terrain: the decoded per-cell grids and the lit surface built from them.
  // The grids arrive once per map, so they are cached and the surface is
  // rebuilt only when they change.
  interface DecodedTerrain {
    width: number;
    height: number;
    /** Ground elevation level per cell. */
    levels: Uint8Array;
    /** `LandType` index per cell (`2` = Water). */
    land: Uint8Array;
    /** TMP slope type per cell (`0` = flat, `1..=16` = ramp). */
    slope: Uint8Array;
    /** Bridge deck rise in levels per cell (`0` = no raised deck). */
    deck: Uint8Array;
  }
  let terrain: DecodedTerrain | null = null;
  let minLevel = 0;

  // Lights only affect the terrain's Lambert materials — the flat tops and the
  // step faces between them; the entity boxes stay unlit so their baked tones
  // are exact.
  scene.add(new THREE.AmbientLight(0xffffff, 0.82));
  // Cool sky over a near-black ground gives the coloured terrain a readable
  // gradient instead of a flat ambient wash; the unlit entity solids ignore it.
  scene.add(new THREE.HemisphereLight(0x9fd4ff, 0x071018, 0.32));
  const sun = new THREE.DirectionalLight(0xffffff, 1.2);
  sun.position.set(0.5, 1.0, 0.35);
  scene.add(sun);

  /** One decoded grid's value at a cell, clamped to the map and 0 off it. */
  function sample(grid: Uint8Array | undefined, rx: number, ry: number): number {
    if (!terrain || !grid) return 0;
    const cx = clampInt(rx, 0, terrain.width - 1);
    const cy = clampInt(ry, 0, terrain.height - 1);
    return grid[cy * terrain.width + cx];
  }

  /** Ground elevation level at a cell, 0 when no terrain has arrived. */
  function levelAt(rx: number, ry: number): number {
    return sample(terrain?.levels, rx, ry);
  }

  /** `LandType` index at a cell. */
  function landAt(rx: number, ry: number): number {
    return sample(terrain?.land, rx, ry);
  }

  /** TMP slope type at a cell (`0` = flat). */
  function slopeAt(rx: number, ry: number): number {
    return sample(terrain?.slope, rx, ry);
  }

  /** Bridge deck rise in levels at a cell (`0` = no raised deck). */
  function deckAt(rx: number, ry: number): number {
    return sample(terrain?.deck, rx, ry);
  }

  /** World height a cell's top sits at: ground plus any bridge deck rise. Every
   *  mark that has to sit on the terrain — entities, cursor, glows, paths,
   *  callouts — reads this instead of the bare ground level. */
  function surfaceAt(rx: number, ry: number): number {
    return (levelAt(rx, ry) + deckAt(rx, ry)) * LEVEL_STEP;
  }

  /** Decode one wire hex grid into a byte per cell. */
  function decodeHex(hex: string, count: number): Uint8Array {
    const out = new Uint8Array(count);
    for (let i = 0; i < count; i++) {
      out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16) || 0;
    }
    return out;
  }

  // --- ground: the terraced elevation surface, carrying the glowing grid ----
  let plate: THREE.Mesh | null = null;
  /** The plate's four materials, in geometry group order: floor (glowing
   *  grid), rock (step faces and bridge piers), water, bridge deck. */
  let plateMaterials: THREE.MeshLambertMaterial[] = [];

  /** The map's terrain, coastline and grid, baked once per map. Land cells are
   *  tinted by their own elevation level, water by its distance from land; level
   *  breaks get contour lines and the coast gets a glowing shoreline. The floor
   *  and water materials share this texture, so the coast has no material seam. */
  function makeGridTexture(
    w: number,
    h: number,
    focus: CellBounds | null,
  ): THREE.CanvasTexture {
    const px = Math.max(2, Math.min(16, Math.floor(2048 / Math.max(w, h))));
    const sprite = document.createElement("canvas");
    sprite.width = Math.max(1, w * px);
    sprite.height = Math.max(1, h * px);
    const g = sprite.getContext("2d");
    if (!g) return new THREE.CanvasTexture(sprite);

    const waterAt = (rx: number, ry: number): boolean => landAt(rx, ry) === WATER_LAND_TYPE;
    const inside = (rx: number, ry: number): boolean => rx >= 0 && ry >= 0 && rx < w && ry < h;
    const hasLandNeighbor = (rx: number, ry: number): boolean => {
      for (const [dx, dy] of NEIGHBORS) {
        const nx = rx + dx;
        const ny = ry + dy;
        if (inside(nx, ny) && !waterAt(nx, ny)) return true;
      }
      return false;
    };
    const hasWaterNeighbor = (rx: number, ry: number): boolean => {
      for (const [dx, dy] of NEIGHBORS) {
        const nx = rx + dx;
        const ny = ry + dy;
        if (inside(nx, ny) && waterAt(nx, ny)) return true;
      }
      return false;
    };
    /** 0 deep, 1 one cell off the shore, 2 shallow. */
    const waterBand = (rx: number, ry: number): number => {
      for (const [dx, dy] of NEIGHBORS) {
        const nx = rx + dx;
        const ny = ry + dy;
        if (inside(nx, ny) && !waterAt(nx, ny)) return 2;
      }
      for (const [dx, dy] of NEIGHBORS) {
        const nx = rx + dx;
        const ny = ry + dy;
        if (inside(nx, ny) && waterAt(nx, ny) && hasLandNeighbor(nx, ny)) return 1;
      }
      return 0;
    };
    const inFocus = (rx: number, ry: number): boolean =>
      !focus ||
      (rx >= focus.minX && rx <= focus.maxX && ry >= focus.minZ && ry <= focus.maxZ);

    let minLand = 255;
    let maxLand = 0;
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        if (waterAt(rx, ry)) continue;
        const level = levelAt(rx, ry);
        if (level < minLand) minLand = level;
        if (level > maxLand) maxLand = level;
      }
    }
    const flat = maxLand <= minLand;

    // Cell fills. The texture covers the whole map, so every cell is opaque.
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        let color: Rgb;
        if (waterAt(rx, ry)) {
          color = WATER_RAMP[waterBand(rx, ry)];
        } else {
          const t = flat ? 0.5 : (levelAt(rx, ry) - minLand) / (maxLand - minLand);
          color = rampRgb(LAND_RAMP, t);
          if (hasWaterNeighbor(rx, ry)) color = mixRgb(color, SHORE_SAND, 0.36);
          if (slopeAt(rx, ry) !== 0) color = mixRgb(color, RAMP_TINT, 0.28);
        }
        g.fillStyle = cssRgb(color);
        g.fillRect(rx * px, ry * px, px, px);
      }
    }

    // Land grid; water keeps its own surface (no tactical grid).
    g.lineWidth = 1;
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        if (waterAt(rx, ry) || !inFocus(rx, ry)) continue;
        if (rx + 1 < w && !waterAt(rx + 1, ry) && inFocus(rx + 1, ry)) {
          const major = (rx + 1) % 4 === 0;
          g.strokeStyle = major ? "rgba(150,220,255,0.26)" : "rgba(120,190,255,0.07)";
          g.beginPath();
          g.moveTo((rx + 1) * px + 0.5, ry * px);
          g.lineTo((rx + 1) * px + 0.5, (ry + 1) * px);
          g.stroke();
        }
        if (ry + 1 < h && !waterAt(rx, ry + 1) && inFocus(rx, ry + 1)) {
          const major = (ry + 1) % 4 === 0;
          g.strokeStyle = major ? "rgba(150,220,255,0.26)" : "rgba(120,190,255,0.07)";
          g.beginPath();
          g.moveTo(rx * px, (ry + 1) * px + 0.5);
          g.lineTo((rx + 1) * px, (ry + 1) * px + 0.5);
          g.stroke();
        }
      }
    }

    // Contour lines where land levels meet.
    g.strokeStyle = "rgba(150,225,205,0.22)";
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        if (waterAt(rx, ry) || !inFocus(rx, ry)) continue;
        const level = levelAt(rx, ry);
        if (rx + 1 < w && !waterAt(rx + 1, ry) && inFocus(rx + 1, ry) && levelAt(rx + 1, ry) !== level) {
          g.beginPath();
          g.moveTo((rx + 1) * px + 0.5, ry * px);
          g.lineTo((rx + 1) * px + 0.5, (ry + 1) * px);
          g.stroke();
        }
        if (ry + 1 < h && !waterAt(rx, ry + 1) && inFocus(rx, ry + 1) && levelAt(rx, ry + 1) !== level) {
          g.beginPath();
          g.moveTo(rx * px, (ry + 1) * px + 0.5);
          g.lineTo((rx + 1) * px, (ry + 1) * px + 0.5);
          g.stroke();
        }
      }
    }

    // Glowing shoreline on the water side of every coast edge.
    g.strokeStyle = "rgba(135,225,255,0.7)";
    g.shadowColor = "rgba(80,200,255,0.85)";
    g.shadowBlur = px * 0.7;
    g.lineWidth = Math.max(1, px * 0.12);
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        if (!waterAt(rx, ry)) continue;
        if (rx > 0 && !waterAt(rx - 1, ry)) {
          g.beginPath();
          g.moveTo(rx * px + 0.5, ry * px);
          g.lineTo(rx * px + 0.5, (ry + 1) * px);
          g.stroke();
        }
        if (rx + 1 < w && !waterAt(rx + 1, ry)) {
          g.beginPath();
          g.moveTo((rx + 1) * px + 0.5, ry * px);
          g.lineTo((rx + 1) * px + 0.5, (ry + 1) * px);
          g.stroke();
        }
        if (ry > 0 && !waterAt(rx, ry - 1)) {
          g.beginPath();
          g.moveTo(rx * px, ry * px + 0.5);
          g.lineTo((rx + 1) * px, ry * px + 0.5);
          g.stroke();
        }
        if (ry + 1 < h && !waterAt(rx, ry + 1)) {
          g.beginPath();
          g.moveTo(rx * px, (ry + 1) * px + 0.5);
          g.lineTo((rx + 1) * px, (ry + 1) * px + 0.5);
          g.stroke();
        }
      }
    }
    g.shadowBlur = 0;
    g.shadowColor = "transparent";

    // Sparse glints give the flat water a surface without a tiled pattern.
    g.strokeStyle = "rgba(170,235,255,0.09)";
    g.lineWidth = 1;
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        if (!waterAt(rx, ry) || (rx * 31 + ry * 17) % 23 !== 0) continue;
        const x = rx * px + px * 0.22;
        const y = ry * px + px * (0.35 + ((rx + ry) % 3) * 0.12);
        g.beginPath();
        g.moveTo(x, y);
        g.lineTo(x + px * 0.5, y);
        g.stroke();
      }
    }

    g.shadowColor = "rgba(160,225,255,1)";
    g.shadowBlur = px;
    g.strokeStyle = "rgba(190,235,255,0.55)";
    g.lineWidth = Math.max(1.5, px * 0.25);
    g.strokeRect(0, 0, sprite.width, sprite.height);
    g.shadowBlur = 0;
    g.shadowColor = "transparent";

    return new THREE.CanvasTexture(sprite);
  }

  /** The terraced plate. Ground tops are flat quads, merged along a row where
   *  level and land type agree; a ramp cell's top is one tilted quad instead,
   *  its corners dropping toward whichever neighbours are lower. A step face
   *  fills every boundary whose two ground levels differ, unless the higher
   *  cell is a ramp — its slope already reaches down. A water cell's top takes
   *  its own material, and a high bridge cell adds a lifted deck quad plus the
   *  side faces carrying it down to its neighbours. Emitted non-indexed, so
   *  each face keeps its own normal and the corners stay hard, in four
   *  contiguous material groups: floor (grid), rock, water, deck. */
  function buildPlateGeometry(w: number, h: number): THREE.BufferGeometry {
    interface Surface {
      pos: number[];
      uv: number[];
    }
    const floor: Surface = { pos: [], uv: [] };
    const rock: Surface = { pos: [], uv: [] };
    const water: Surface = { pos: [], uv: [] };
    const deck: Surface = { pos: [], uv: [] };

    const push = (dst: Surface, x: number, y: number, z: number, u: number, v: number): void => {
      dst.pos.push(x, y, z);
      dst.uv.push(u, v);
    };
    /** One quad, counter-clockwise seen from above, carrying the cell box's
     *  texture corners. `a` is the cell's min-x/min-z corner. */
    const quad = (
      dst: Surface,
      ax: number, ay: number, az: number,
      bx: number, by: number, bz: number,
      cx: number, cy: number, cz: number,
      dx: number, dy: number, dz: number,
      u0: number, v0: number, u1: number, v1: number,
    ): void => {
      push(dst, ax, ay, az, u0, v1);
      push(dst, bx, by, bz, u0, v0);
      push(dst, cx, cy, cz, u1, v0);
      push(dst, ax, ay, az, u0, v1);
      push(dst, cx, cy, cz, u1, v0);
      push(dst, dx, dy, dz, u1, v1);
    };

    const isRamp = (rx: number, ry: number): boolean => slopeAt(rx, ry) !== 0;
    const isDeck = (rx: number, ry: number): boolean => deckAt(rx, ry) > 0;
    const isWater = (rx: number, ry: number): boolean => landAt(rx, ry) === WATER_LAND_TYPE;
    /** A cell whose ground top is a plain quad: everything but a ramp's sloped
     *  tile. A bridge cell still has ground under it, so it counts as flat. */
    const flatGround = (rx: number, ry: number): boolean => isDeck(rx, ry) || !isRamp(rx, ry);

    // The lowest terrace, for the ground bloom that hangs under the plate.
    minLevel = 255;
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        const level = levelAt(rx, ry);
        if (level < minLevel) minLevel = level;
      }
    }

    // Ground tops. Merging a row's equal-level, equal-land-type cells into one
    // quad keeps a plateau-heavy — or flat — map close to free; only the
    // terraces' own corners and every ramp cost vertices.
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        if (!flatGround(rx, ry)) continue;
        const level = levelAt(rx, ry);
        const wet = isWater(rx, ry);
        // A cell whose west neighbour shares its run is inside it already.
        if (
          rx > 0 && flatGround(rx - 1, ry) &&
          levelAt(rx - 1, ry) === level && isWater(rx - 1, ry) === wet
        ) continue;
        let end = rx + 1;
        while (
          end < w && flatGround(end, ry) &&
          levelAt(end, ry) === level && isWater(end, ry) === wet
        ) end++;
        const y = level * LEVEL_STEP;
        quad(
          wet ? water : floor,
          rx, y, ry,
          rx, y, ry + 1,
          end, y, ry + 1,
          end, y, ry,
          rx / w, 1 - (ry + 1) / h, end / w, 1 - ry / h,
        );
      }
    }

    // Ramps: one tilted quad per cell, each corner dropping by the larger of
    // its two edges' drops. A full-edge ramp then tilts on a single axis and a
    // corner ramp on both, without overshooting either neighbour.
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        if (!isRamp(rx, ry) || isDeck(rx, ry)) continue;
        const level = levelAt(rx, ry);
        const west = rx > 0 ? levelAt(rx - 1, ry) : level;
        const east = rx + 1 < w ? levelAt(rx + 1, ry) : level;
        const north = ry > 0 ? levelAt(rx, ry - 1) : level;
        const south = ry + 1 < h ? levelAt(rx, ry + 1) : level;
        const dropWest = Math.max(0, level - west);
        const dropEast = Math.max(0, level - east);
        const dropNorth = Math.max(0, level - north);
        const dropSouth = Math.max(0, level - south);
        // Corner (0, 0) is the cell's min-x/min-z corner; cx picks the x-side
        // (0 = west, 1 = east), cz the z-side (0 = north, 1 = south).
        const corner = (cx: number, cz: number): number =>
          (level - Math.max(cx === 0 ? dropWest : dropEast, cz === 0 ? dropNorth : dropSouth)) *
          LEVEL_STEP;
        quad(
          isWater(rx, ry) ? water : floor,
          rx, corner(0, 0), ry,
          rx, corner(0, 1), ry + 1,
          rx + 1, corner(1, 1), ry + 1,
          rx + 1, corner(1, 0), ry,
          rx / w, 1 - (ry + 1) / h, (rx + 1) / w, 1 - ry / h,
        );
      }
    }

    // Step faces. Each boundary is reached once, from its west or north cell,
    // and drawn from the higher level down to the lower one; the material is
    // double-sided, so the wall is solid from whichever side the camera stands.
    // A ramp on the higher side already slopes to its neighbour: no face.
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        const level = levelAt(rx, ry);
        if (rx + 1 < w) {
          const east = levelAt(rx + 1, ry);
          const higherRamp = level > east ? isRamp(rx, ry) : isRamp(rx + 1, ry);
          if (east !== level && !higherRamp) {
            const low = Math.min(level, east) * LEVEL_STEP;
            const high = Math.max(level, east) * LEVEL_STEP;
            quad(rock, rx + 1, low, ry, rx + 1, low, ry + 1, rx + 1, high, ry + 1, rx + 1, high, ry, 0, 0, 0, 0);
          }
        }
        if (ry + 1 < h) {
          const south = levelAt(rx, ry + 1);
          const higherRamp = level > south ? isRamp(rx, ry) : isRamp(rx, ry + 1);
          if (south !== level && !higherRamp) {
            const low = Math.min(level, south) * LEVEL_STEP;
            const high = Math.max(level, south) * LEVEL_STEP;
            quad(rock, rx, low, ry + 1, rx + 1, low, ry + 1, rx + 1, high, ry + 1, rx, high, ry + 1, 0, 0, 0, 0);
          }
        }
      }
    }

    // Bridge decks: a lifted steel top over the cell's own ground, plus a side
    // face wherever the neighbour's surface sits lower — the span's flanks and
    // its abutments. Adjacent equal decks share a top, so no face is drawn
    // between them and the deck reads as one continuous span.
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        if (!isDeck(rx, ry)) continue;
        const top = (levelAt(rx, ry) + deckAt(rx, ry)) * LEVEL_STEP;
        const neighbourTop = (nx: number, ny: number): number =>
          isDeck(nx, ny)
            ? (levelAt(nx, ny) + deckAt(nx, ny)) * LEVEL_STEP
            : levelAt(nx, ny) * LEVEL_STEP;
        quad(
          deck,
          rx, top, ry,
          rx, top, ry + 1,
          rx + 1, top, ry + 1,
          rx + 1, top, ry,
          rx / w, 1 - (ry + 1) / h, (rx + 1) / w, 1 - ry / h,
        );
        const sides: { nx: number; ny: number; ax: number; az: number; bx: number; bz: number }[] = [];
        if (rx > 0) sides.push({ nx: rx - 1, ny: ry, ax: rx, az: ry, bx: rx, bz: ry + 1 });
        if (rx + 1 < w) sides.push({ nx: rx + 1, ny: ry, ax: rx + 1, az: ry, bx: rx + 1, bz: ry + 1 });
        if (ry > 0) sides.push({ nx: rx, ny: ry - 1, ax: rx, az: ry, bx: rx + 1, bz: ry });
        if (ry + 1 < h) sides.push({ nx: rx, ny: ry + 1, ax: rx, az: ry + 1, bx: rx + 1, bz: ry + 1 });
        for (const side of sides) {
          const base = neighbourTop(side.nx, side.ny);
          if (base >= top) continue;
          quad(deck, side.ax, base, side.az, side.bx, base, side.bz, side.bx, top, side.bz, side.ax, top, side.az, 0, 0, 0, 0);
        }
      }
    }

    const geometry = new THREE.BufferGeometry();
    const position = [...floor.pos, ...rock.pos, ...water.pos, ...deck.pos];
    const uv = [...floor.uv, ...rock.uv, ...water.uv, ...deck.uv];
    geometry.setAttribute("position", new THREE.Float32BufferAttribute(position, 3));
    geometry.setAttribute("uv", new THREE.Float32BufferAttribute(uv, 2));
    geometry.computeVertexNormals();
    // The four grids are concatenated in material order, so each is a
    // contiguous range — empty ones simply get no group.
    let start = 0;
    const groups: [number, number][] = [
      [floor.pos.length / 3, 0],
      [rock.pos.length / 3, 1],
      [water.pos.length / 3, 2],
      [deck.pos.length / 3, 3],
    ];
    for (const [count, material] of groups) {
      if (count > 0) geometry.addGroup(start, count, material);
      start += count;
    }
    return geometry;
  }

  function buildGround(w: number, h: number): void {
    if (plate) {
      scene.remove(plate);
      plate.geometry.dispose();
    }
    const oldMaps = new Set<THREE.Texture>();
    for (const material of plateMaterials) {
      if (material.map) oldMaps.add(material.map);
      material.dispose();
    }
    for (const map of oldMaps) map.dispose();
    const focus = terrainContentBounds(w, h);
    const texture = makeGridTexture(w, h, focus);
    texture.colorSpace = THREE.SRGBColorSpace;
    // The plate is seen at a grazing angle: without anisotropic filtering the mip
    // chain jumps to a level far coarser than a cell and the grid averages away.
    texture.anisotropy = renderer.capabilities.getMaxAnisotropy();

    // The floor and water share the terrain texture: the coast is painted into
    // it, so neither material needs its own flat colour. Rock and the bridge
    // deck stay solid, shaded by the same sun.
    const shaded = { flatShading: true, side: THREE.DoubleSide };
    const floorMaterial = new THREE.MeshLambertMaterial({ map: texture, flatShading: true });
    const rockMaterial = new THREE.MeshLambertMaterial({ color: CLIFF_TONE, ...shaded });
    const waterMaterial = new THREE.MeshLambertMaterial({ map: texture, color: 0xffffff, ...shaded });
    const deckMaterial = new THREE.MeshLambertMaterial({ color: DECK_TONE, ...shaded });
    plateMaterials = [floorMaterial, rockMaterial, waterMaterial, deckMaterial];
    // The geometry is already in world space — X and Z are the cell axes and Y
    // is the level — so the plate needs no transform of its own.
    plate = new THREE.Mesh(buildPlateGeometry(w, h), plateMaterials);
    scene.add(plate);
  }

  // --- entities: unlit, per-category toned solids --------------------------
  const entityMaterial = new THREE.MeshBasicMaterial({ vertexColors: true });
  const entities = new Map<EntityCategory, THREE.InstancedMesh>();
  const meshCategory = new Map<THREE.InstancedMesh, EntityCategory>();
  for (const category of CATEGORIES) {
    const mesh = new THREE.InstancedMesh(
      categoryGeometry(category),
      entityMaterial,
      MAX_SPOTS,
    );
    mesh.count = 0;
    mesh.frustumCulled = false;
    scene.add(mesh);
    entities.set(category, mesh);
    meshCategory.set(mesh, category);
  }
  const dummy = new THREE.Object3D();
  const color = new THREE.Color();
  // Instance id -> the spot it came from, per category, for hover/click lookups.
  const entityLookup: Record<EntityCategory, EntitySpot[]> = {
    Infantry: [],
    Unit: [],
    Aircraft: [],
    Structure: [],
  };
  let lastHouses: HousePopulation[] = [];

  // --- cursor marker (the Terrain Debug panel's cell) ----------------------
  const cursorMarker = new THREE.Mesh(
    new THREE.BoxGeometry(1.02, 0.12, 1.02),
    new THREE.MeshBasicMaterial({ color: 0xff7cc9, transparent: true, opacity: 0.95 }),
  );
  cursorMarker.visible = false;
  scene.add(cursorMarker);
  // A flat halo: at 30° elevation a thin box is a few pixels tall, and the
  // cursor cell is the one thing that always has to be findable.
  const cursorHalo = new THREE.Mesh(
    new THREE.PlaneGeometry(3.4, 3.4),
    new THREE.MeshBasicMaterial({ color: 0xff7cc9, transparent: true, opacity: 0.22 }),
  );
  cursorHalo.rotation.x = -Math.PI / 2;
  cursorHalo.visible = false;
  scene.add(cursorHalo);

  // --- bloom ---------------------------------------------------------------
  /** One soft radial sprite reused for every glow: the diamond's halo and each
   *  base's ground bloom are the same gradient, tinted and scaled. */
  function makeGlowTexture(): THREE.CanvasTexture {
    const side = 128;
    const sprite = document.createElement("canvas");
    sprite.width = side;
    sprite.height = side;
    const g = sprite.getContext("2d");
    if (g) {
      const grad = g.createRadialGradient(side / 2, side / 2, 0, side / 2, side / 2, side / 2);
      grad.addColorStop(0, "rgba(255,255,255,1)");
      grad.addColorStop(0.35, "rgba(255,255,255,0.42)");
      grad.addColorStop(1, "rgba(255,255,255,0)");
      g.fillStyle = grad;
      g.fillRect(0, 0, side, side);
    }
    return new THREE.CanvasTexture(sprite);
  }
  const glowTexture = makeGlowTexture();

  function makeGlow(hex: number, opacity: number): THREE.Mesh<THREE.PlaneGeometry, THREE.MeshBasicMaterial> {
    const mesh = new THREE.Mesh(
      new THREE.PlaneGeometry(1, 1),
      new THREE.MeshBasicMaterial({
        map: glowTexture,
        color: hex,
        transparent: true,
        opacity,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
      }),
    );
    mesh.rotation.x = -Math.PI / 2;
    mesh.visible = false;
    scene.add(mesh);
    return mesh;
  }

  const plateGlow = makeGlow(0x63b3ff, 0.32);
  const baseGlows = [makeGlow(0xffffff, 0.45), makeGlow(0xffffff, 0.45), makeGlow(0xffffff, 0.45), makeGlow(0xffffff, 0.45)];
  const scratchColor = new THREE.Color();

  /** A crisp ring with a soft inner halo, tinted by each base's house colour. */
  function makeRingTexture(): THREE.CanvasTexture {
    const side = 256;
    const sprite = document.createElement("canvas");
    sprite.width = side;
    sprite.height = side;
    const g = sprite.getContext("2d");
    if (g) {
      const mid = side / 2;
      const grad = g.createRadialGradient(mid, mid, side * 0.24, mid, mid, mid);
      grad.addColorStop(0, "rgba(255,255,255,0)");
      grad.addColorStop(0.56, "rgba(255,255,255,0)");
      grad.addColorStop(0.70, "rgba(255,255,255,0.10)");
      grad.addColorStop(0.80, "rgba(255,255,255,0.85)");
      grad.addColorStop(0.84, "rgba(255,255,255,0.16)");
      grad.addColorStop(0.92, "rgba(255,255,255,0.04)");
      grad.addColorStop(1, "rgba(255,255,255,0)");
      g.fillStyle = grad;
      g.fillRect(0, 0, side, side);
    }
    return new THREE.CanvasTexture(sprite);
  }
  const ringTexture = makeRingTexture();
  ringTexture.colorSpace = THREE.SRGBColorSpace;

  function makeBaseRing(): THREE.Mesh<THREE.PlaneGeometry, THREE.MeshBasicMaterial> {
    const mesh = new THREE.Mesh(
      new THREE.PlaneGeometry(1, 1),
      new THREE.MeshBasicMaterial({
        map: ringTexture,
        color: 0xffffff,
        transparent: true,
        opacity: 0.75,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
        side: THREE.DoubleSide,
      }),
    );
    mesh.rotation.x = -Math.PI / 2;
    mesh.visible = false;
    scene.add(mesh);
    return mesh;
  }
  const baseRings = [makeBaseRing(), makeBaseRing(), makeBaseRing(), makeBaseRing()];

  // --- path preview --------------------------------------------------------
  const MAX_PATH_POINTS = 128;
  interface PathLine {
    line: THREE.Line;
    material: THREE.LineDashedMaterial;
    positions: THREE.BufferAttribute;
  }
  const pathLines: PathLine[] = [];
  for (let i = 0; i < baseGlows.length; i++) {
    const positions = new THREE.BufferAttribute(new Float32Array(MAX_PATH_POINTS * 3), 3);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", positions);
    const material = new THREE.LineDashedMaterial({
      color: 0x41ecff,
      dashSize: 2.4,
      gapSize: 1.4,
      transparent: true,
      opacity: 1,
    });
    const line = new THREE.Line(geometry, material);
    line.visible = false;
    line.frustumCulled = false;
    scene.add(line);
    pathLines.push({ line, material, positions });
  }

  // --- hover highlight -----------------------------------------------------
  const highlight = new THREE.LineSegments(
    new THREE.EdgesGeometry(new THREE.BoxGeometry(1, 1, 1)),
    new THREE.LineBasicMaterial({ color: 0xaee8ff, transparent: true, opacity: 0.95 }),
  );
  highlight.visible = false;
  highlight.frustumCulled = false;
  scene.add(highlight);

  // The hover chip is a DOM element outside the callout host, which `updateCallouts`
  // clears each snapshot. It shares the stage box so its offsets are in pixels.
  const stage = (canvas.parentElement ?? document.body) as HTMLElement;
  const tip = document.createElement("div");
  tip.className = "map-tip";
  tip.style.display = "none";
  stage.append(tip);

  /** Camera position and frustum. The diamond's screen extents follow from the
   *  azimuth and elevation, so the fit is solved against the stage rather than
   *  tuned by eye: at 45° it is `(w + h) * cos45` wide and that times
   *  `sin(elevation)` tall, and either edge can bind depending on the panel. */
  function applyCamera(): void {
    camera.position.set(
      orbitTarget.x + CAMERA_DISTANCE * Math.cos(orbit.azimuth) * Math.cos(orbit.elevation),
      orbitTarget.y + CAMERA_DISTANCE * Math.sin(orbit.elevation),
      orbitTarget.z + CAMERA_DISTANCE * Math.sin(orbit.azimuth) * Math.cos(orbit.elevation),
    );
    camera.lookAt(orbitTarget);

    const spread = (view.w + view.h) * Math.SQRT1_2;
    const halfFromHeight = (spread * Math.sin(orbit.elevation)) / 2;
    const halfFromWidth = spread / 2 / Math.max(0.2, aspect);
    const halfHeight = (Math.max(halfFromHeight, halfFromWidth) * 2) / FIT / 2;
    camera.top = halfHeight * orbit.zoom;
    camera.bottom = -camera.top;
    camera.right = camera.top * aspect;
    camera.left = -camera.right;
    camera.updateProjectionMatrix();
  }

  /** Re-aim at the current framed window. */
  function recenter(): void {
    orbitTarget.set(view.cx, 0, view.cz);
    applyCamera();
  }

  /** Frame a content box; the plate's terrain still extends around it. */
  function fitBounds(bounds: CellBounds): void {
    view.w = Math.min(extent.w, Math.max(1, bounds.maxX - bounds.minX + 1));
    view.h = Math.min(extent.h, Math.max(1, bounds.maxZ - bounds.minZ + 1));
    const cx = (bounds.minX + bounds.maxX) / 2;
    const cz = (bounds.minZ + bounds.maxZ) / 2;
    view.cx = Math.min(Math.max(cx, view.w / 2), extent.w - view.w / 2);
    view.cz = Math.min(Math.max(cz, view.h / 2), extent.h - view.h / 2);
    orbit.zoom = 1;
    recenter();
  }

  function frameWholeMap(): void {
    view.w = extent.w;
    view.h = extent.h;
    view.cx = extent.w / 2;
    view.cz = extent.h / 2;
    orbit.zoom = 1;
    recenter();
  }

  /** Cells the current snapshot asks to see. Quantiles drop a handful of
   *  off-map/neutral outliers; base centres anchor houses with no live spots. */
  function actionBounds(snapshot: TelemetrySnapshot): CellBounds | null {
    const xs: number[] = [];
    const zs: number[] = [];
    for (const spot of snapshot.spots) {
      xs.push(spot.x);
      zs.push(spot.y);
    }
    for (const house of snapshot.houses) {
      if (!house.passive && !house.defeated && house.base_center) {
        xs.push(house.base_center[0]);
        zs.push(house.base_center[1]);
      }
    }
    if (xs.length === 0) return null;
    const pct = (values: number[], p: number): number => {
      const sorted = [...values].sort((a, b) => a - b);
      return sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * p))];
    };
    // A few unset/orphan spots sit at (0,0); 6% trims them without cutting a
    // real march's leading or trailing edge.
    const low = xs.length >= 12 ? 0.06 : 0;
    const high = xs.length >= 12 ? 0.94 : 1;
    return {
      minX: Math.max(0, pct(xs, low)),
      maxX: Math.min(extent.w, pct(xs, high)),
      minZ: Math.max(0, pct(zs, low)),
      maxZ: Math.min(extent.h, pct(zs, high)),
    };
  }

  /** Real terrain content: water, shore, elevation, ramps, bridges, plus the
   *  cells where units or bases actually stand. The unused flat border is what
   *  the panel's big empty grid was made of. */
  function terrainContentBounds(w: number, h: number, pad = 12): CellBounds | null {
    let minX = w;
    let minZ = h;
    let maxX = -1;
    let maxZ = -1;
    const take = (x: number, y: number): void => {
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minZ) minZ = y;
      if (y > maxZ) maxZ = y;
    };
    for (let ry = 0; ry < h; ry++) {
      for (let rx = 0; rx < w; rx++) {
        const water = landAt(rx, ry) === WATER_LAND_TYPE;
        let shore = false;
        if (!water) {
          for (const [dx, dy] of NEIGHBORS) {
            const nx = rx + dx;
            const ny = ry + dy;
            if (nx >= 0 && ny >= 0 && nx < w && ny < h && landAt(nx, ny) === WATER_LAND_TYPE) {
              shore = true;
              break;
            }
          }
        }
        if (water || shore || levelAt(rx, ry) !== 0 || slopeAt(rx, ry) !== 0 || deckAt(rx, ry) > 0) {
          take(rx, ry);
        }
      }
    }
    const action = latestSnapshot ? actionBounds(latestSnapshot) : null;
    if (action) {
      take(action.minX, action.minZ);
      take(action.maxX, action.maxZ);
    }
    if (maxX < 0) return null;
    return {
      minX: Math.max(0, minX - pad),
      minZ: Math.max(0, minZ - pad),
      maxX: Math.min(w - 1, maxX + pad),
      maxZ: Math.min(h - 1, maxZ + pad),
    };
  }

  /** Default camera framing: the action plus a small rim, so the whole
   *  battlefield content fills the panel instead of sitting inside the unused
   *  plate border. Before any unit exists, fall back to the terrain content. */
  function fitDefault(snapshot: TelemetrySnapshot, force = false): void {
    const action = actionBounds(snapshot);
    if (action) {
      const pad = 10;
      fitBounds({
        minX: Math.max(0, action.minX - pad),
        minZ: Math.max(0, action.minZ - pad),
        maxX: Math.min(extent.w - 1, action.maxX + pad),
        maxZ: Math.min(extent.h - 1, action.maxZ + pad),
      });
      framedToAction = true;
      return;
    }
    if (!framedToAction || force) {
      const content = terrainContentBounds(
        snapshot.map_width || 64,
        snapshot.map_height || 64,
        4,
      );
      if (content) {
        fitBounds(content);
      } else {
        frameWholeMap();
      }
    }
  }

  /** Effective marker scale: category size with a screen-pixel floor. */
  function markerScale(category: EntityCategory): number {
    const base = CATEGORY_SCALE[category] ?? 0.6;
    const worldPerPixel = (camera.top - camera.bottom) / Math.max(1, canvas.clientHeight);
    return Math.min(base * 8, Math.max(base, MIN_MARKER_PX * worldPerPixel));
  }

  /** Centre the camera on a map cell, keeping the current orientation and zoom. */
  function focusOn(gx: number, gz: number): void {
    orbitTarget.set(gx, surfaceAt(gx, gz), gz);
    applyCamera();
  }

  function updateSpots(snapshot: TelemetrySnapshot): void {
    const spots: EntitySpot[] = snapshot.spots;
    lastHouses = snapshot.houses;
    for (const category of CATEGORIES) {
      entities.get(category)!.count = 0;
      entityLookup[category].length = 0;
    }
    // One colour per house row, from the engine's own [Colors] scheme.
    const houseColors = snapshot.houses.map((house, index) => houseColor(house, index));
    for (const spot of spots) {
      const mesh = entities.get(spot.category);
      if (!mesh || mesh.count >= MAX_SPOTS) continue;
      const index = mesh.count;
      mesh.count = index + 1;
      entityLookup[spot.category].push(spot);

      const scale = markerScale(spot.category);
      // Map (rx, ry) -> world (X, Z); elevation -> Y.
      dummy.position.set(
        spot.x,
        spot.z * LEVEL_STEP + CATEGORY_HEIGHT[spot.category] * scale,
        spot.y,
      );
      dummy.scale.set(scale, scale, scale);
      dummy.rotation.set(0, 0, 0);
      dummy.updateMatrix();
      mesh.setMatrixAt(index, dummy.matrix);
      const owner = spot.owner_index;
      const base = owner === 0xffff ? ORPHAN_COLOR : (houseColors[owner] ?? ORPHAN_COLOR);
      color.setStyle(spot.category === "Structure" ? wash(base, STRUCTURE_WASH) : base);
      mesh.setColorAt(index, color);
    }
    for (const category of CATEGORIES) {
      const mesh = entities.get(category)!;
      mesh.instanceMatrix.needsUpdate = true;
      if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
    }
  }

  function updateCursor(debug: DebugPanel | null): void {
    const cell = debug?.cursor_cell;
    if (!cell) {
      cursorMarker.visible = false;
      cursorHalo.visible = false;
      return;
    }
    const ground = surfaceAt(cell[0], cell[1]);
    cursorMarker.visible = true;
    cursorHalo.visible = true;
    cursorMarker.position.set(cell[0] + 0.5, ground + 0.06, cell[1] + 0.5);
    cursorHalo.position.set(cell[0] + 0.5, ground + 0.02, cell[1] + 0.5);
  }

  function updateGlows(snapshot: TelemetrySnapshot): void {
    const reach = Math.max(view.w, view.h) * 2.0;
    plateGlow.visible = true;
    plateGlow.position.set(view.cx, minLevel * LEVEL_STEP - 0.15, view.cz);
    plateGlow.scale.set(reach, reach, 1);

    const bases = snapshot.houses.filter(
      (house: HousePopulation) => !house.passive && !house.defeated && house.base_center,
    );
    const size = Math.min(48, Math.max(14, Math.max(view.w, view.h) * 0.18));
    const ringSize = Math.min(44, Math.max(16, Math.max(view.w, view.h) * 0.2));
    for (let i = 0; i < baseGlows.length; i++) {
      const glow = baseGlows[i];
      const ring = baseRings[i];
      const house = bases[i];
      if (!house?.base_center) {
        glow.visible = false;
        ring.visible = false;
        continue;
      }
      scratchColor.setStyle(houseColor(house, snapshot.houses.indexOf(house)));
      glow.material.color.copy(scratchColor);
      glow.visible = true;
      ring.material.color.copy(scratchColor);
      ring.visible = true;
      const [bx, bz] = house.base_center;
      const ground = surfaceAt(bx, bz);
      glow.position.set(bx, ground + 0.03, bz);
      glow.scale.set(size, size, 1);
      ring.position.set(bx, ground + 0.045, bz);
      ring.scale.set(ringSize, ringSize, 1);
    }
  }

  /** The engine already sends the plan for every inspected unit, so the dashed
   *  line is the sim's own path, not a guess. A step it marks unwalkable turns
   *  the whole line amber — the same reading as the chip's "path blocked". */
  function updatePaths(debug: DebugPanel | null): void {
    const selected = debug?.selected ?? [];
    for (let i = 0; i < pathLines.length; i++) {
      const entry = pathLines[i];
      const steps = selected[i]?.path_preview ?? [];
      if (steps.length < 2) {
        entry.line.visible = false;
        continue;
      }
      const count = Math.min(steps.length, MAX_PATH_POINTS);
      const array = entry.positions.array as Float32Array;
      let blocked = false;
      for (let s = 0; s < count; s++) {
        const [cx, cy] = steps[s].cell;
        array[s * 3] = cx + 0.5;
        array[s * 3 + 1] = surfaceAt(cx, cy) + 0.7;
        array[s * 3 + 2] = cy + 0.5;
        blocked = blocked || !steps[s].walkable;
      }
      entry.positions.needsUpdate = true;
      entry.line.geometry.setDrawRange(0, count);
      entry.material.color.set(blocked ? 0xffca74 : 0x41ecff);
      // Dashes are measured along the polyline, so they only come out right
      // once the positions are final.
      entry.line.computeLineDistances();
      entry.line.visible = true;
    }
  }

  // --- callouts: world-anchored DOM chips over the canvas ------------------
  const projected = new THREE.Vector3();
  const calloutHost: HTMLElement | null = calloutLayer ?? null;

  function chip(stem: number, title: string, sub: string, tint: string | null): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "map-callout";
    wrap.style.setProperty("--stem", `${stem}px`);
    if (tint) wrap.style.setProperty("--tint", tint);
    const box = document.createElement("div");
    box.className = "map-callout-box";
    const head = document.createElement("b");
    head.textContent = title;
    box.append(head);
    if (sub) {
      const detail = document.createElement("em");
      detail.textContent = sub;
      box.append(detail);
    }
    const line = document.createElement("i");
    line.className = "map-callout-stem";
    const foot = document.createElement("s");
    foot.className = "map-callout-foot";
    wrap.append(box, line, foot);
    return wrap;
  }

  /** Place one chip above the world point its cell names; false when it is
   *  behind the camera or off the visible canvas. */
  function place(el: HTMLElement, gx: number, gy: number): boolean {
    projected.set(gx, surfaceAt(gx, gy), gy).project(camera);
    if (projected.z > 1) return false;
    const left = (projected.x * 0.5 + 0.5) * canvas.clientWidth;
    const top = (-projected.y * 0.5 + 0.5) * canvas.clientHeight;
    if (left < 0 || top < 0 || left > canvas.clientWidth || top > canvas.clientHeight) return false;
    el.style.left = `${left.toFixed(1)}px`;
    el.style.top = `${top.toFixed(1)}px`;
    return true;
  }

  function updateCallouts(snapshot: TelemetrySnapshot): void {
    if (!calloutHost) return;
    calloutHost.textContent = "";
    const debug = snapshot.debug;
    const built: { el: HTMLElement; gx: number; gy: number }[] = [];

    // Selected units first: they are what the inspector panel is about, so they
    // keep the slots if there are more than fit.
    for (const unit of debug?.selected.slice(0, MAX_CALLOUTS) ?? []) {
      const state = unit.path_blocked
        ? t("map.pathBlocked")
        : unit.moving
          ? `${t("map.moving")} · ${t("map.steps", { n: unit.path_steps_left })}`
          : t("map.standby");
      built.push({
        el: chip(34 + built.length * 15, unit.type_name, `${state} · #${unit.stable_id}`, null),
        gx: unit.cell[0] + unit.sub[0],
        gy: unit.cell[1] + unit.sub[1],
      });
    }
    // Then each contending house's base, the only other place on the map the
    // engine reports a named position for a player.
    const room = MAX_CALLOUTS - built.length;
    if (room > 0) {
      const bases = snapshot.houses.filter(
        (house: HousePopulation) => !house.passive && !house.defeated && house.base_center,
      );
      for (const house of bases.slice(0, room)) {
        const index = snapshot.houses.indexOf(house);
        built.push({
          el: chip(34 + built.length * 15, house.name, t("map.base"), houseColor(house, index)),
          gx: house.base_center![0],
          gy: house.base_center![1],
        });
      }
    }

    for (const item of built) {
      calloutHost.append(item.el);
      if (!place(item.el, item.gx, item.gy)) item.el.remove();
    }
    avoidCalloutOverlaps(built.map((item) => item.el).filter((el) => el.isConnected));
  }

  /** Two bases can sit close enough that their chips cover each other. Lift the
   *  later chip's stem until its box clears every box already placed; the foot
   *  stays on the named cell. */
  function avoidCalloutOverlaps(elements: HTMLElement[]): void {
    const placed: DOMRect[] = [];
    const gap = 7;
    for (const el of elements) {
      const box = el.querySelector<HTMLElement>(".map-callout-box");
      if (!box) continue;
      let stem = Number.parseFloat(el.style.getPropertyValue("--stem")) || 34;
      const anchorTop = Number.parseFloat(el.style.top) || 0;
      const limit = Math.max(stem, anchorTop - box.offsetHeight - 8);
      for (let pass = 0; pass < 4; pass++) {
        const rect = box.getBoundingClientRect();
        const hit = placed.find(
          (other) =>
            rect.right > other.left - gap &&
            rect.left < other.right + gap &&
            rect.bottom > other.top - gap &&
            rect.top < other.bottom + gap,
        );
        if (!hit) break;
        const lift = hit.bottom + gap - rect.top;
        if (lift <= 0) break;
        stem = Math.min(limit, stem + lift);
        el.style.setProperty("--stem", `${stem.toFixed(1)}px`);
        if (stem >= limit) break;
      }
      placed.push(box.getBoundingClientRect());
    }
  }

  // --- interaction: hover, click-to-focus, drag rotate/pan, wheel zoom ------
  const raycaster = new THREE.Raycaster();
  const ndc = new THREE.Vector2();
  const hoverMatrix = new THREE.Matrix4();
  const hoverPos = new THREE.Vector3();
  const hoverQuat = new THREE.Quaternion();
  const hoverScale = new THREE.Vector3();
  let pointerX = 0;
  let pointerY = 0;
  let pointerInside = false;

  /** Owner name for a spot, or the orphan label when it has no house row. */
  function ownerName(spot: EntitySpot): string {
    if (spot.owner_index === 0xffff) return t("map.orphan");
    return lastHouses[spot.owner_index]?.name ?? t("map.orphan");
  }

  /** Once per frame: outline the entity under the pointer and show its chip.
   *  Bounded to a frame so a 20k-instance world does not raycast per mousemove. */
  function updateHover(): void {
    if (!pointerInside || drag.mode) {
      highlight.visible = false;
      tip.style.display = "none";
      return;
    }
    const rect = canvas.getBoundingClientRect();
    ndc.set(
      ((pointerX - rect.left) / rect.width) * 2 - 1,
      -(((pointerY - rect.top) / rect.height) * 2 - 1),
    );
    raycaster.setFromCamera(ndc, camera);
    const hits = raycaster.intersectObjects([...entities.values()], false);
    const hit = hits[0];
    const id = hit?.instanceId;
    const mesh = hit?.object as THREE.InstancedMesh | undefined;
    const category = mesh ? meshCategory.get(mesh) : undefined;
    if (!mesh || category === undefined || id === undefined) {
      highlight.visible = false;
      tip.style.display = "none";
      return;
    }
    mesh.getMatrixAt(id, hoverMatrix);
    hoverMatrix.decompose(hoverPos, hoverQuat, hoverScale);
    const scale = markerScale(category) * 1.5;
    highlight.position.copy(hoverPos);
    highlight.scale.set(scale, scale, scale);
    highlight.visible = true;

    const spot = entityLookup[category][id];
    tip.textContent = spot ? `${spot.category} · ${ownerName(spot)}` : "";
    tip.style.display = spot ? "block" : "none";
    const stageRect = stage.getBoundingClientRect();
    tip.style.left = `${pointerX - stageRect.left + 14}px`;
    tip.style.top = `${pointerY - stageRect.top + 14}px`;
  }

  /** The entity or terrain point under a screen position, as a focus target. */
  function pick(gx: number, gy: number): { gx: number; gz: number } | null {
    const rect = canvas.getBoundingClientRect();
    ndc.set(((gx - rect.left) / rect.width) * 2 - 1, -(((gy - rect.top) / rect.height) * 2 - 1));
    raycaster.setFromCamera(ndc, camera);
    const hits = raycaster.intersectObjects([...entities.values()], false);
    if (hits.length > 0) {
      const hit = hits[0];
      const category = meshCategory.get(hit.object as THREE.InstancedMesh);
      const spot = category !== undefined && hit.instanceId !== undefined
        ? entityLookup[category][hit.instanceId]
        : undefined;
      if (spot) return { gx: spot.x, gz: spot.y };
    }
    // Nothing solid hit: fall back to the ground plane so a click anywhere
    // re-centres the view rather than doing nothing.
    if (plate) {
      const ground = raycaster.intersectObject(plate, false);
      const point = ground[0]?.point;
      if (point) return { gx: point.x, gz: point.z };
    }
    return null;
  }

  const drag: { mode: "rotate" | "pan" | null; x: number; y: number; moved: boolean } = {
    mode: null,
    x: 0,
    y: 0,
    moved: false,
  };

  function bindInteractions(): void {
    canvas.addEventListener("pointerdown", (event) => {
      drag.mode = event.button === 2 || event.shiftKey ? "pan" : "rotate";
      drag.x = event.clientX;
      drag.y = event.clientY;
      drag.moved = false;
      canvas.setPointerCapture(event.pointerId);
    });
    canvas.addEventListener("pointerup", (event) => {
      const wasClick = drag.mode === "rotate" && !drag.moved;
      drag.mode = null;
      canvas.releasePointerCapture(event.pointerId);
      // A click (not a drag) re-centres on what is under it.
      if (wasClick) {
        const target = pick(event.clientX, event.clientY);
        if (target) focusOn(target.gx, target.gz);
      }
    });
    canvas.addEventListener("pointermove", (event) => {
      pointerX = event.clientX;
      pointerY = event.clientY;
      pointerInside = true;
      if (!drag.mode) return;
      const dx = event.clientX - drag.x;
      const dy = event.clientY - drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 2) drag.moved = true;
      if (drag.mode === "rotate") {
        orbit.azimuth -= dx * 0.008;
        orbit.elevation = Math.min(1.45, Math.max(0.12, orbit.elevation + dy * 0.006));
      } else {
        // Pan: move the view target opposite the drag, one world unit per the
        // pixels the frustum covers, along the camera's own screen axes.
        const worldPerPixel = (camera.top - camera.bottom) / canvas.clientHeight;
        const right = new THREE.Vector3().setFromMatrixColumn(camera.matrix, 0);
        const up = new THREE.Vector3().setFromMatrixColumn(camera.matrix, 1);
        orbitTarget.addScaledVector(right, -dx * worldPerPixel);
        orbitTarget.addScaledVector(up, dy * worldPerPixel);
      }
      drag.x = event.clientX;
      drag.y = event.clientY;
      applyCamera();
    });
    canvas.addEventListener("pointerleave", () => {
      pointerInside = false;
    });
    canvas.addEventListener("contextmenu", (event) => event.preventDefault());
    canvas.addEventListener(
      "wheel",
      (event) => {
        event.preventDefault();
        zoom(1 / (1 + Math.sign(event.deltaY) * 0.12));
      },
      { passive: false },
    );
  }

  function zoom(factor: number): void {
    orbit.zoom = Math.min(3, Math.max(0.35, orbit.zoom / factor));
    applyCamera();
  }

  function resize(): void {
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    if (width === 0 || height === 0) return;
    renderer.setSize(width, height, false);
    aspect = width / height;
    // The frustum depends on the aspect, so a resize changes the framing too.
    applyCamera();
  }

  function render(): void {
    requestAnimationFrame(render);
    updateHover();
    // Base rings breathe slowly; this is presentation only and never touches
    // simulation state or the snapshot pipeline.
    const now = performance.now();
    for (let i = 0; i < baseRings.length; i++) {
      const ring = baseRings[i];
      if (!ring.visible) continue;
      ring.material.opacity = 0.58 + 0.22 * (0.5 + 0.5 * Math.sin(now * 0.0016 + i * 1.9));
    }
    renderer.render(scene, camera);
  }

  buildGround(extent.w, extent.h);
  recenter();
  bindInteractions();
  new ResizeObserver(resize).observe(canvas.parentElement as HTMLElement);
  resize();
  render();

  return {
    draw(snapshot: TelemetrySnapshot): void {
      const w = snapshot.map_width || 64;
      const h = snapshot.map_height || 64;
      // The elevation grid arrives once per map; cache it and rebuild the
      // surface only when the grid or the map size changes.
      if (snapshot.terrain) {
        const grid = snapshot.terrain;
        const count = grid.width * grid.height;
        terrain = {
          width: grid.width,
          height: grid.height,
          levels: decodeHex(grid.levels, count),
          land: decodeHex(grid.land_types, count),
          slope: decodeHex(grid.slope_types, count),
          deck: decodeHex(grid.deck_rises, count),
        };
      }
      latestSnapshot = snapshot;
      if (w !== extent.w || h !== extent.h) {
        extent = { w, h };
        buildGround(w, h);
        framedToAction = false;
        fitDefault(snapshot);
      } else if (terrain && snapshot.terrain) {
        buildGround(w, h);
        framedToAction = false;
        fitDefault(snapshot);
      }
      if (!framedToAction) fitDefault(snapshot);
      updateSpots(snapshot);
      updateCursor(snapshot.debug);
      updateGlows(snapshot);
      updatePaths(snapshot.debug);
      // Last: the chips are positioned against the camera this snapshot left the
      // scene in, so they must be projected after every move.
      updateCallouts(snapshot);
    },
    zoomBy(factor: number): void {
      zoom(factor);
    },
    resetView(): void {
      orbit.azimuth = ISO_AZIMUTH;
      orbit.elevation = ISO_ELEVATION;
      orbit.zoom = 1;
      framedToAction = false;
      if (latestSnapshot) {
        fitDefault(latestSnapshot, true);
      } else {
        frameWholeMap();
      }
    },
    dispose(): void {
      glowTexture.dispose();
      for (const entry of pathLines) {
        entry.line.geometry.dispose();
        entry.material.dispose();
      }
      for (const glow of [plateGlow, ...baseGlows]) {
        glow.geometry.dispose();
        glow.material.dispose();
      }
      ringTexture.dispose();
      for (const ring of baseRings) {
        ring.geometry.dispose();
        ring.material.dispose();
      }
      if (plate) plate.geometry.dispose();
      const maps = new Set<THREE.Texture>();
      for (const material of plateMaterials) {
        if (material.map) maps.add(material.map);
        material.dispose();
      }
      for (const map of maps) map.dispose();
      for (const category of CATEGORIES) {
        entities.get(category)!.geometry.dispose();
      }
      entityMaterial.dispose();
      highlight.geometry.dispose();
      (highlight.material as THREE.Material).dispose();
      cursorMarker.geometry.dispose();
      (cursorMarker.material as THREE.Material).dispose();
      cursorHalo.geometry.dispose();
      (cursorHalo.material as THREE.Material).dispose();
      tip.remove();
      renderer.dispose();
    },
  };
}
