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
 *  - **Unlit, per-face tones.** The blocks are flat three-tone solids, not
 *    shaded geometry, so the boxes carry baked per-face vertex colours and the
 *    material is unlit. The house colour still multiplies in through
 *    `instanceColor`, so ownership reads exactly as before.
 *  - **Glowing grid.** WebGL lines are one pixel wide and cannot glow, so the
 *    plate's grid, its every-fourth emphasis and its boundary are baked once
 *    into a canvas texture with a real shadow blur.
 *
 * The callout layer is a DOM overlay, not sprites: it has to carry the panel's
 * own text and stay crisp when the map is dragged. Every chip is anchored to a
 * cell the engine already reports — a selected unit, a house's base centre — so
 * nothing here invents a position.
 *
 * Orientation note: azimuth `pi/4` is 45° off the radar axes, which is what makes
 * the diamond. It is not the in-game minimap reading (that is azimuth `pi/2`,
 * where +rx runs right and +ry runs down); drag-rotate is live, so the check
 * against the game is one drag away.
 *
 * Schema mirror of `src/sim/telemetry.rs`; keep both in step.
 */

import * as THREE from "./vendor/three.module.js";
import { t } from "./i18n.js";
import { DebugPanel, EntityCategory, EntitySpot, HousePopulation, TelemetrySnapshot } from "./types.js";

/** InstancedMesh capacity: the project's 20k-unit target. */
const MAX_SPOTS = 20000;
/** One elevation level's visual offset, in world units. */
const LEVEL_STEP = 0.5;
/** Per-category box scale and base height (world units, one cell = 1).
 *  The centre is lifted by `height × scale`, which sets a structure's footprint
 *  just above the plate instead of half sunk into it. */
const CATEGORY_SCALE: Record<EntityCategory, number> = {
  Infantry: 0.3,
  Unit: 0.42,
  Aircraft: 0.36,
  Structure: 2.2,
};
const CATEGORY_HEIGHT: Record<EntityCategory, number> = {
  Infantry: 0.3,
  Unit: 0.5,
  Aircraft: 0.45,
  Structure: 0.55,
};
/** Structures are washed toward white to read as buildings; units keep the full
 *  team colour so ownership stays legible. */
const STRUCTURE_WASH = 0.55;
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
/** BoxGeometry face order: +X, -X, +Y, -Y, +Z, -Z, four vertices each. */
const FACE_TONES = [0.78, 0.52, 1.0, 0.45, 0.68, 0.6];
/** Fallback palette for houses the engine gave no colour (neutral, orphan). */
const HOUSE_PALETTE = ["#8a94a3", "#b04aff", "#e8c34a", "#37e0b8", "#ff5555", "#4a9eff"];
/** Entities with no house row (owner_index sentinel). */
const ORPHAN_COLOR = "#6b7a8c";

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

/** A unit box with a flat tone baked into each face, for the unlit material. */
function tonedBox(): THREE.BoxGeometry {
  const geometry = new THREE.BoxGeometry(1, 1, 1);
  const colors = new Float32Array(24 * 3);
  for (let vertex = 0; vertex < 24; vertex++) {
    const tone = FACE_TONES[Math.floor(vertex / 4)];
    colors[vertex * 3] = tone;
    colors[vertex * 3 + 1] = tone;
    colors[vertex * 3 + 2] = tone;
  }
  geometry.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  return geometry;
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
  let extent = { w: 64, h: 64 };
  let aspect = 1;

  // --- ground: one plate whose texture is the glowing cell grid ------------
  let plate: THREE.Mesh<THREE.PlaneGeometry, THREE.MeshBasicMaterial> | null = null;

  /** The map's grid, boundary and every-fourth emphasis, baked with a glow.
   *  WebGL lines are a fixed one pixel and cannot bloom, which is what makes the
   *  mockup's grid read as lit rather than drawn. */
  function makeGridTexture(w: number, h: number): THREE.CanvasTexture {
    const px = Math.max(2, Math.min(16, Math.floor(2048 / Math.max(w, h))));
    const sprite = document.createElement("canvas");
    sprite.width = Math.max(1, w * px);
    sprite.height = Math.max(1, h * px);
    const g = sprite.getContext("2d");
    if (g) {
      // The base has to stay very dark: the plate is seen at 30°, so minification
      // averages a cell down to roughly its mean colour, and a lighter fill turns
      // the whole grid into a flat pale wash.
      g.fillStyle = "#040a1c";
      g.fillRect(0, 0, sprite.width, sprite.height);
      g.lineWidth = 1;
      for (let i = 0; i <= w; i++) {
        const major = i % 4 === 0;
        g.strokeStyle = major ? "rgba(150,210,255,0.46)" : "rgba(110,170,255,0.11)";
        g.shadowColor = major ? "rgba(120,190,255,0.9)" : "transparent";
        g.shadowBlur = major ? px * 0.5 : 0;
        g.beginPath();
        g.moveTo(i * px + 0.5, 0);
        g.lineTo(i * px + 0.5, sprite.height);
        g.stroke();
      }
      for (let j = 0; j <= h; j++) {
        const major = j % 4 === 0;
        g.strokeStyle = major ? "rgba(150,210,255,0.46)" : "rgba(110,170,255,0.11)";
        g.shadowColor = major ? "rgba(120,190,255,0.9)" : "transparent";
        g.shadowBlur = major ? px * 0.5 : 0;
        g.beginPath();
        g.moveTo(0, j * px + 0.5);
        g.lineTo(sprite.width, j * px + 0.5);
        g.stroke();
      }
      g.shadowColor = "rgba(160,225,255,1)";
      g.shadowBlur = px;
      g.strokeStyle = "rgba(190,235,255,0.85)";
      g.lineWidth = Math.max(1.5, px * 0.25);
      g.strokeRect(0, 0, sprite.width, sprite.height);
    }
    return new THREE.CanvasTexture(sprite);
  }

  function buildGround(w: number, h: number): void {
    if (plate) {
      scene.remove(plate);
      plate.geometry.dispose();
      plate.material.map?.dispose();
      plate.material.dispose();
    }
    const texture = makeGridTexture(w, h);
    texture.colorSpace = THREE.SRGBColorSpace;
    // The plate is seen at a grazing angle: without anisotropic filtering the mip
    // chain jumps to a level far coarser than a cell and the grid averages away.
    texture.anisotropy = renderer.capabilities.getMaxAnisotropy();
    plate = new THREE.Mesh(
      new THREE.PlaneGeometry(w, h),
      new THREE.MeshBasicMaterial({ map: texture }),
    );
    plate.rotation.x = -Math.PI / 2;
    plate.position.set(w / 2, 0, h / 2);
    scene.add(plate);
  }

  // --- entities: unlit, per-face toned boxes -------------------------------
  const entities = new THREE.InstancedMesh(
    tonedBox(),
    new THREE.MeshBasicMaterial({ vertexColors: true }),
    MAX_SPOTS,
  );
  entities.count = 0;
  entities.frustumCulled = false;
  scene.add(entities);
  const dummy = new THREE.Object3D();
  const color = new THREE.Color();

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

  const plateGlow = makeGlow(0x63b3ff, 0.5);
  const baseGlows = [makeGlow(0xffffff, 0.55), makeGlow(0xffffff, 0.55), makeGlow(0xffffff, 0.55), makeGlow(0xffffff, 0.55)];
  const scratchColor = new THREE.Color();

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

  /** Camera position and frustum. The diamond's screen extents follow from the
   *  azimuth and elevation, so the fit is solved against the stage rather than
   *  tuned by eye: at 45° it is `(w + h) * cos45` wide and that times
   *  `sin(elevation)` tall, and either edge can bind depending on the panel. */
  function applyCamera(): void {
    orbitTarget.set(extent.w / 2, 0, extent.h / 2);
    camera.position.set(
      orbitTarget.x + CAMERA_DISTANCE * Math.cos(orbit.azimuth) * Math.cos(orbit.elevation),
      orbitTarget.y + CAMERA_DISTANCE * Math.sin(orbit.elevation),
      orbitTarget.z + CAMERA_DISTANCE * Math.sin(orbit.azimuth) * Math.cos(orbit.elevation),
    );
    camera.lookAt(orbitTarget);

    const spread = (extent.w + extent.h) * Math.SQRT1_2;
    const halfFromHeight = (spread * Math.sin(orbit.elevation)) / 2;
    const halfFromWidth = spread / 2 / Math.max(0.2, aspect);
    const halfHeight = (Math.max(halfFromHeight, halfFromWidth) * 2) / FIT / 2;
    camera.top = halfHeight * orbit.zoom;
    camera.bottom = -camera.top;
    camera.right = camera.top * aspect;
    camera.left = -camera.right;
    camera.updateProjectionMatrix();
  }

  function updateSpots(snapshot: TelemetrySnapshot): void {
    const spots: EntitySpot[] = snapshot.spots;
    const count = Math.min(spots.length, MAX_SPOTS);
    // One colour per house row, from the engine's own [Colors] scheme.
    const houseColors = snapshot.houses.map((house, index) => houseColor(house, index));
    for (let i = 0; i < count; i++) {
      const spot = spots[i];
      const scale = CATEGORY_SCALE[spot.category] ?? 0.6;
      // Map (rx, ry) -> world (X, Z); elevation -> Y.
      dummy.position.set(
        spot.x,
        spot.z * LEVEL_STEP + CATEGORY_HEIGHT[spot.category] * scale,
        spot.y,
      );
      dummy.scale.set(scale, scale, scale);
      dummy.rotation.set(0, 0, 0);
      dummy.updateMatrix();
      entities.setMatrixAt(i, dummy.matrix);
      const owner = spot.owner_index;
      const base = owner === 0xffff ? ORPHAN_COLOR : (houseColors[owner] ?? ORPHAN_COLOR);
      color.setStyle(spot.category === "Structure" ? wash(base, STRUCTURE_WASH) : base);
      entities.setColorAt(i, color);
    }
    entities.count = count;
    entities.instanceMatrix.needsUpdate = true;
    if (entities.instanceColor) entities.instanceColor.needsUpdate = true;
  }

  function updateCursor(debug: DebugPanel | null): void {
    const cell = debug?.cursor_cell;
    if (!cell) {
      cursorMarker.visible = false;
      cursorHalo.visible = false;
      return;
    }
    cursorMarker.visible = true;
    cursorHalo.visible = true;
    cursorMarker.position.set(cell[0] + 0.5, 0.06, cell[1] + 0.5);
    cursorHalo.position.set(cell[0] + 0.5, 0.02, cell[1] + 0.5);
  }

  function updateGlows(snapshot: TelemetrySnapshot): void {
    const reach = Math.max(extent.w, extent.h) * 1.6;
    plateGlow.visible = true;
    plateGlow.position.set(extent.w / 2, -0.06, extent.h / 2);
    plateGlow.scale.set(reach, reach, 1);

    const bases = snapshot.houses.filter(
      (house: HousePopulation) => !house.passive && !house.defeated && house.base_center,
    );
    const size = Math.max(extent.w, extent.h) * 0.36;
    for (let i = 0; i < baseGlows.length; i++) {
      const glow = baseGlows[i];
      const house = bases[i];
      if (!house?.base_center) {
        glow.visible = false;
        continue;
      }
      scratchColor.setStyle(houseColor(house, snapshot.houses.indexOf(house)));
      glow.material.color.copy(scratchColor);
      glow.visible = true;
      glow.position.set(house.base_center[0], 0.03, house.base_center[1]);
      glow.scale.set(size, size, 1);
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
        array[s * 3] = steps[s].cell[0] + 0.5;
        array[s * 3 + 1] = 0.7;
        array[s * 3 + 2] = steps[s].cell[1] + 0.5;
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
    projected.set(gx, 0, gy).project(camera);
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
  }

  // --- orbit: drag rotate, wheel zoom --------------------------------------
  function zoom(factor: number): void {
    orbit.zoom = Math.min(3, Math.max(0.35, orbit.zoom / factor));
    applyCamera();
  }

  function bindOrbit(): void {
    let dragging = false;
    let lastX = 0;
    let lastY = 0;
    canvas.addEventListener("pointerdown", (event) => {
      dragging = true;
      lastX = event.clientX;
      lastY = event.clientY;
      canvas.setPointerCapture(event.pointerId);
    });
    canvas.addEventListener("pointerup", (event) => {
      dragging = false;
      canvas.releasePointerCapture(event.pointerId);
    });
    canvas.addEventListener("pointermove", (event) => {
      if (!dragging) return;
      orbit.azimuth -= (event.clientX - lastX) * 0.008;
      orbit.elevation = Math.min(
        1.45,
        Math.max(0.12, orbit.elevation + (event.clientY - lastY) * 0.006),
      );
      lastX = event.clientX;
      lastY = event.clientY;
      applyCamera();
    });
    canvas.addEventListener(
      "wheel",
      (event) => {
        event.preventDefault();
        zoom(1 / (1 + Math.sign(event.deltaY) * 0.12));
      },
      { passive: false },
    );
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
    renderer.render(scene, camera);
  }

  buildGround(extent.w, extent.h);
  bindOrbit();
  new ResizeObserver(resize).observe(canvas.parentElement as HTMLElement);
  resize();
  render();

  return {
    draw(snapshot: TelemetrySnapshot): void {
      const w = snapshot.map_width || 64;
      const h = snapshot.map_height || 64;
      if (w !== extent.w || h !== extent.h) {
        extent = { w, h };
        buildGround(w, h);
        applyCamera();
      }
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
      applyCamera();
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
      if (plate) {
        plate.material.map?.dispose();
        plate.material.dispose();
        plate.geometry.dispose();
      }
      entities.geometry.dispose();
      (entities.material as THREE.Material).dispose();
      renderer.dispose();
    },
  };
}
