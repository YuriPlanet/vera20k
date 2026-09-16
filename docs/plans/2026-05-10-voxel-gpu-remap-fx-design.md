---
title: Voxel Render — GPU-Side House Remap & Special-Effects Pipeline
status: awaiting approval
---

# Voxel Render — GPU-Side House Remap & Special-Effects Pipeline

## Goal

Move voxel sprite **house remap** and **per-instance visual effects** (cloak, EMP,
iron curtain, warp, mirror) out of the atlas-bake step and into the fragment
shader. Drop the house dimension from the atlas cache key. Convert atlas tiles
from RGBA to u8 palette indices. The result is one atlas per game session
(rather than per house × per game), shader-driven FX (rather than re-baked
atlas variants per effect state), and a memory budget that scales independent
of player count.

## Architecture Context

### How voxel rendering works today

[vxl_raster.rs](src/render/vxl_raster.rs) — CPU rasterizer. Takes
`(VxlFile, HvaFile, Palette, VplFile, VxlRenderParams)` and produces a
`VxlSprite { rgba: Vec<[u8; 4]>, depth: Vec<f32>, ... }`. House remap is
applied **before rasterization** by `palette.with_house_colors(ramp)` at
[vxl_raster.rs:614](src/render/vxl_raster.rs#L614). Each house gets its own
rasterization pass.

[unit_atlas.rs](src/render/unit_atlas.rs) — atlas builder. Pre-renders all
unique sprite keys at game load and shelf-packs them into a single GPU
texture. Current key:
```rust
UnitSpriteKey { type_id, facing, house_color, layer, frame, slope_type }
```
The `house_color` field is what multiplies memory by the player count.
Caching is incremental — only new keys get rendered.

[vxl_compute.rs](src/render/vxl_compute.rs) — GPU compute alternative. Splat +
resolve passes for the Composite layer only. Output is RGBA.

[vxl_normals.rs](src/render/vxl_normals.rs) — normal vector tables and
Blinn-Phong page mapping.

Per-frame, [app_instances/units.rs](src/app_instances/units.rs) builds a
`UnitSpriteKey` from each entity's `(type_ref, facing, owner→house, slope, frame)`,
calls `atlas.get(key)`, and emits a `SpriteInstance` for the GPU instanced
draw. Voxel sprites use **passthrough depth** (don't write Z); only terrain
writes depth. Cliff redraw uses `zdepth+Less`.

### Why we're changing it

Two reports
([VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md),
[VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md))
established that gamemd applies house remap **at blit time**, not at rasterize
time, and stores **pre-remap palette indices** in its visibility-map
intermediate. Our current atlas inverts that: it bakes house remap in.
Combined with the project's 30-player / 20k-unit scale target
(`memory/project_scale_target.md`), this puts the atlas memory cost on a
trajectory that scales linearly with player count when it shouldn't.

Per CLAUDE.md, parity is on observable output, not internals. We are not
porting gamemd's visibility-map data flow verbatim — we are matching its
output (same pixels, same FX behavior) with a clean Rust GPU pipeline that
happens to defer remap and FX to fragment-shader time.

## Impact Analysis

| Touched | Change |
|---|---|
| [src/render/vxl_raster.rs](src/render/vxl_raster.rs) | Output: `Vec<[u8;4]>` → `Vec<u8>`. Remove house-remap step (`palette.with_house_colors`). Color 0 reserved as transparent sentinel. |
| [src/render/vxl_compute.rs](src/render/vxl_compute.rs) | Resolve pass writes u8 instead of RGBA. |
| [src/render/unit_atlas.rs](src/render/unit_atlas.rs) | Drop `house_color` from `UnitSpriteKey`. Texture format → R8Uint. Per-house render loop removed. |
| [src/render/vxl_normals.rs](src/render/vxl_normals.rs) | `SPECULAR_STRENGTH = 3.4 → 3.0`. Iterate normals `0..245` for mode 4. Add modes 1 (16 entries) and 3 (64 entries) tables (low priority). |
| [src/assets/vxl_file.rs](src/assets/vxl_file.rs) | `VXL_HEADER_SIZE = 802 → 32`. Parse variable palette section (`palette_count × 770 bytes`). Tailer `+0x0C` re-typed `f32 scale`. |
| New: `src/render/palette_textures.rs` | Owns palette texture + remap LUT texture as GPU resources. Per-theater palette upload. |
| New / extended: `src/render/sprite_shader.wgsl` | Fragment shader: `byte → remap_lut[house_idx][byte] → palette[idx] → fx_apply()`. |
| [src/render/batch_renderer.rs](src/render/batch_renderer.rs) | Per-instance uniform buffer for `(house_color_idx, fx_flags, fx_params)`. Add bind groups for palette + remap LUT. |
| [src/app_instances/units.rs](src/app_instances/units.rs) | Drop `house_color` from key build. Pass `house_color_idx` and `fx_state` per-instance. |
| New / extended: sim components | `Cloak`, `EmpEffect`, `IronCurtain`, `WarpEffect`, `Mirror` (per-entity FX state). |
| [src/bin/audit-assets.rs](src/bin/audit-assets.rs) | Verify VXL header parser fix. |

**Determinism:** None affected. The render path does not feed sim. Per
CLAUDE.md the architectural invariant `sim/ NEVER depends on render/` is
preserved.

**Migration:** None. Atlas is rebuilt at every game start; no on-disk format.
No save-game schema impact.

**Blast radius:** All voxel-rendered units (vehicles, ships, aircraft, voxel
buildings) re-render through the new path. SHP-rendered entities (infantry,
most buildings, animations) are untouched. Audit during Phase 1: any
SHP→VXL fallback paths must keep working with the unchanged SHP pipeline.

## Tiny-Detail Ledger

Parity-relevant items the implementation must preserve. Each cites its source.
Items already correct in Rust are noted; items being fixed by this design are
flagged.

### Output format & color

- **Color 0 = transparent**, hard-coded everywhere [doc: VXL_RASTERIZER_DISPATCH §8].
  Atlas tile must reserve byte 0 as the "no voxel" sentinel. Rasterizer must
  not write index 0 for any actual voxel. Rust currently ✓ at
  [vxl_raster.rs:672](src/render/vxl_raster.rs#L672) (`Color::transparent()`)
  but the new u8 atlas needs a hard invariant: byte 0 ↔ no voxel.
- **VPL output formula**: `vpl_pages[(g_VXL_NormalLUT[normal] << 8) | color]`
  [doc: VXL_RASTERIZER_DISPATCH §2.1, §5.1]. Atlas tile pixel value = the
  VPL-shaded palette index. **No house remap at this stage.**
- **House remap formula**: `palette[house_remap_lut[src_byte]]` [doc:
  VXL_RASTERIZER_DISPATCH §10.1]. Applied at draw time in fragment shader.
- **Source-byte transparency test before remap**: `if (b != 0)` on the raw
  byte before any palette lookup [doc: VXL_RASTERIZER_DISPATCH §8]. Shader
  must `discard` (or write α=0) on byte 0, BEFORE looking up the remap LUT.

### Lighting constants

- **Specular strength = 3.0**, NOT 3.4 [doc: VXL_HVA §6.4, GHIDRA `0x40400000`
  passed to `VXL_Init_BlinnPhong`]. **Rust currently wrong at
  [vxl_normals.rs](src/render/vxl_normals.rs)** — fix in Phase 0.
- **Light scale ×16.0** at `0x007F6960` [doc: VXL_HVA §6.3]. Diffuse byte =
  `ftol(dot × 16.0)` clamped at 0 from below.
- **Ambient byte = 0x10 (16)** at `g_VXL_NormalLUT[253..255]` [doc: VXL_HVA
  §6.3, §6.4]. Hardcoded ambient triple.
- **RA2 normals = 245 entries** (entries 245-249 byte-duplicate of 244, no
  entries 250-255) [doc: VXL_HVA §6.2]. **Rust currently iterates 0..256** —
  fix in Phase 0.
- **Light direction = (-0.7071, -0.7071, 0)** yawed by world angle [doc:
  VXL_HVA §6.5]. Initialized once at startup, NOT per-section per-frame.

### Geometry

- **Slope tilt: edge ≈29.88° (0.5215 rad), corner ≈22.10° (0.3859 rad).
  Identity at slope 0** [doc: VXL_HVA §5.4]. Rust ✓ at
  [vxl_raster.rs:800-826](src/render/vxl_raster.rs#L800) (`EDGE_TILT_RAD`,
  `CORNER_TILT_RAD`).
- **32-step facing convention**, per-step angle `-PI/16` [doc: VXL_HVA §5.2,
  GHIDRA const `0x007E4408`]. Rust uses 64-bucket body / 128-bucket turret
  quantization in `unit_atlas.rs` — that's an OVERSAMPLING of 32 facings,
  acceptable per CLAUDE.md (smoother is fine).
- **Aircraft body matrix has NO banking, NO climb/dive tilt, NO slope tilt**
  when `ConsideredAircraft=true` [doc: VXL_HVA §5.5]. Verify Rust honors this.
- **Sub-tick facing SLERP** for Drive/Ship only, NOT Fly/Turret [doc: VXL_HVA
  §7.4]. Rust currently quantizes to discrete buckets — sub-tick smoothing is
  a Phase-N follow-up if visible drift is observed.
- **Body matrix composition order**: `(rx_ry) × facing_rot × shear × slope`
  [doc: VXL_HVA §5.3].

### File format (atlas-build correctness)

- **VXL header is 32 bytes + variable palette (`palette_count × 770`)** [doc:
  VXL_HVA §2.1, §2.2]. **Rust `VXL_HEADER_SIZE = 802` is wrong** — fix in
  Phase 0.
- **Tailer `+0x0C` is `f32 scale`**, NOT `u32 limb_identifier` [doc: VXL_HVA
  §2.5]. Fix in Phase 0.
- **Voxel run encoding `[skip:u8, count:u8, (color, normal) × count, trailer:u8]`**
  with trailer byte CONSUMED but value IGNORED [doc: VXL_HVA §8]. Rust ✓ at
  [vxl_decode.rs:143](src/assets/vxl_decode.rs#L143).
- **HVA header 24 bytes: 16 filename + 4 frame_count + 4 section_count** [doc:
  VXL_HVA §3]. Rust ✓.
- **HVA section names are seeked-past** by gamemd (positional pairing)
  [doc: VXL_HVA §3.3]. Rust reads them but doesn't gate logic on them — ✓.
- **HVA matrix index = `frame × section_count + section`** [doc: VXL_HVA §3].
  Rust ✓.

### Animation

- **Frame index = `FootClass+0x538 % HVA->frame_count`** per draw [doc:
  VXL_HVA §5.7]. Frame counter increments gated on `WalkRate` (when moving)
  or `IdleRate` (when idle/firing), with `g_CurrentFrameCounter % rate == 0`.
- **WalkRate** = `TechnoTypeClass+0x294`, **IdleRate** = `+0x298` [doc:
  VXL_HVA §5.7]. Rust component layer needs this — verify or add.
- **Most YR voxels have `frame_count == 1`** [doc: VXL_HVA §5.7]. Static is
  the common case.

### Rasterizer dispatch (rendering correctness)

- **4 live rasterizer dispatch entries** in YR: idx 4/5 (lit, no-mirror, OBB
  corner halves) and idx 6/7 (lit, mirror+z-test) [doc:
  VXL_RASTERIZER_DISPATCH §2.1, §2.2]. Other slots are TS-legacy dead.
- **Mirror rasterizer = z-test enabled, sweep with reversed direction** [doc:
  VXL_RASTERIZER_DISPATCH §2.2]. Phase 6 (water reflections) — not
  immediately needed.
- **`g_VXL_RenderMode` is constant 1 in YR** [doc: VXL_RASTERIZER_DISPATCH
  §5]. Rust always-lit assumption is correct. No-op for the design.
- **`tailer.normals_mode` is `+0xA3`** byte; the same byte is read by the
  rasterizer as `byte == 0 → transparent` [doc: VXL_RASTERIZER_DISPATCH §7].
  Rust must treat the byte consistently in both reads. Mode 0 → use unlit
  alpha rasterizer; modes 1-4 → use lit opaque rasterizer.

## Chosen Approach

### High-level shape

```
Atlas tile (R8Uint)         per-instance uniform           per-theater asset
┌──────────────┐             ┌───────────────────┐         ┌───────────────┐
│ byte = 0     │             │ house_color_idx   │         │ palette[256]  │
│   = transparent             │ fx_flags          │         │ remap_lut[H][256]│
│ byte = N     │             │ fx_params (vec4)  │         │ (H = N houses)│
│   = palette idx             │                   │         │               │
└──────┬───────┘             └────────┬──────────┘         └───┬───────────┘
       │                              │                        │
       └────────► fragment shader ◄───┴────────────────────────┘
                       │
                       ▼
                final RGBA pixel

fragment_shader(uv):
    byte = texture(atlas, uv).r;
    if (byte == 0u) discard;
    byte = remap_lut[house_color_idx][byte];   // house remap
    color = palette[byte];                      // palette → rgb
    color = apply_fx(color, fx_flags, fx_params);
    return color;
```

### Atlas key (after change)

```rust
struct UnitSpriteKey {
    type_id: u32,
    facing: u8,       // body or composite facing (64-bucket quantization)
    layer: VxlLayer,  // Composite | Body | Turret | Barrel
    frame: u8,        // HVA frame % HVA::frame_count
    slope_type: u8,   // 0..=8
}
```

The `house_color` field is **gone**. Memory shape:

| Dimension | Cardinality | Notes |
|---|---|---|
| type_id | ~30-50 | unit types in stock YR; capped per-game |
| facing | 64 (body) or 128 (turret) | quantized; only exercised facings stored |
| layer | 4 max | Composite, Body, Turret, Barrel |
| frame | 1-8 typical | most YR voxels have `frame_count = 1` |
| slope_type | 1-9 | mostly 0 (flat); 1-8 sampled per terrain |

Saturated single-game upper bound: `30 × 64 × 4 × 8 × 9 = 553k` keys × ~4KB
average tile (in u8 storage) = **~2.2 GB worst-case**, but realistically
much smaller (frames=1, layers=Composite-only, slopes mostly 0). Real
expected size: **~50-200 MB** for a 30-player saturated session.

For comparison, current Rust atlas multiplied by 30 houses → **~1.5-6 GB**
expected, **~66 GB** worst-case. The new design is 30× smaller in
expectation.

### Atlas tile format

- Texture: `wgpu::TextureFormat::R8Uint` (single-channel byte texture, no
  filtering — point sampling).
- Tile pixel: byte value `0..255`.
  - `0` = transparent (no voxel rasterized at this pixel).
  - `1..=15` = palette indices (non-remappable; team-neutral).
  - `16..=31` = palette indices in the **remap range** (these get
    house-translated by `remap_lut`).
  - `32..=255` = palette indices (non-remappable).

The rasterizer's job: produce the VPL-shaded palette index per voxel,
without any house remap. Verified VPL formula: `vpl_pages[(g_VXL_NormalLUT[normal] << 8) | color]`.

### Per-instance uniform

```rust
#[repr(C)]
struct UnitSpriteInstance {
    // (existing fields: position, size, uv, depth, ...)
    house_color_idx: u32,   // u8 actually, padded
    fx_flags: u32,
    fx_params: [f32; 4],    // packed: cloak_alpha, emp_dim, ic_phase, warp_phase
    ic_tint: [f32; 4],      // RGB tint + intensity for iron curtain
}
```

`fx_flags` bits:

```
bit 0: CLOAK         (use fx_params[0] as alpha)
bit 1: EMP           (use fx_params[1] as desaturate amount)
bit 2: IRON_CURTAIN  (use ic_tint[0..3] + ic_tint[3] as intensity)
bit 3: WARP_FADE     (use fx_params[3] as scanline phase)
bit 4: MIRROR        (water reflection — flip Y or alpha-fade)
bit 5..31: reserved
```

### Palette + remap LUT GPU resources

Per-theater (Temperate, Snow, Urban, Desert, etc.):

```rust
struct PaletteSet {
    palette_tex: wgpu::Texture,        // 256×1, RGBA, point-sampled
    remap_lut_tex: wgpu::Texture,      // 256×N_houses, R8Uint, point-sampled
}
```

`remap_lut_tex[house][byte]` → translated byte. Identity outside the remap
range, house-band substitution for indices 16-31.

Theater palette is selected per map. House count is fixed at game start
(`30` for the upper bound); LUT is rebuilt only on theater swap (rare) or
house-list change (game start).

### Fragment shader (WGSL, abbreviated)

```wgsl
@fragment
fn fs_voxel_sprite(in: VertexOut) -> @location(0) vec4<f32> {
    let byte = textureSample(atlas, atlas_sampler, in.uv).r * 255.0;
    let byte_u = u32(byte);
    if (byte_u == 0u) { discard; }

    let remapped = remap_lookup(in.house_color_idx, byte_u);
    var color = palette_lookup(remapped);

    color = apply_fx(color, in.fx_flags, in.fx_params, in.ic_tint);
    return color;
}

fn remap_lookup(house_idx: u32, byte: u32) -> u32 {
    let coords = vec2<i32>(i32(byte), i32(house_idx));
    return u32(textureLoad(remap_lut, coords, 0).r);
}

fn palette_lookup(byte: u32) -> vec4<f32> {
    return textureLoad(palette, vec2<i32>(i32(byte), 0), 0);
}

fn apply_fx(color: vec4<f32>, flags: u32, params: vec4<f32>, ic: vec4<f32>) -> vec4<f32> {
    var c = color;
    if ((flags & 1u) != 0u) { c.a *= params.x; }                          // cloak
    if ((flags & 2u) != 0u) { c.rgb = mix(c.rgb, vec3(luminance(c.rgb)), params.y); } // EMP desaturate
    if ((flags & 4u) != 0u) { c.rgb = mix(c.rgb, ic.rgb, ic.a); }         // IC tint
    if ((flags & 8u) != 0u) { c.a *= warp_pattern(in.uv, params.w); }     // warp scanline
    return c;
}
```

Branches are uniform-controlled; GPU branch predictor handles them well at
the per-instance granularity (all fragments of one sprite share the same
flags).

### Special-effects integration

| FX | Sim component | Uniform field | Shader branch |
|---|---|---|---|
| Cloak | `Cloak { state, transition_phase }` | `fx_params[0]` (alpha) | alpha-blend, optional shimmer noise |
| EMP | `EmpEffect { remaining_ticks }` | `fx_params[1]` (desat) | desaturate to luma |
| Iron Curtain | `IronCurtain { remaining_ticks }` | `ic_tint[0..3]` + intensity | RGB tint overlay |
| Warp / Chrono | `WarpEffect { phase, in_or_out }` | `fx_params[3]` | scanline-noise alpha fade |
| Mirror (water) | `Mirror` (computed at render) | bit 4 + same atlas | flip-Y blit + alpha-fade |

Each effect is a per-entity component; the render layer reads it when
building the SpriteInstance and packs it into the per-instance uniform.

## Design

### Components

```
src/render/
  ├─ vxl_raster.rs          (output u8, no house remap)
  ├─ vxl_compute.rs         (output u8, no house remap)
  ├─ vxl_normals.rs         (specular 3.0, normals 0..245)
  ├─ unit_atlas.rs          (key without house_color, R8Uint texture)
  ├─ palette_textures.rs    [NEW] (palette + remap LUT GPU resources)
  ├─ batch_renderer.rs      (per-instance uniform with FX, palette/remap bindings)
  └─ sprite_shader.wgsl     (fragment shader: byte → remap → palette → fx)

src/assets/
  └─ vxl_file.rs            (header 32 bytes, palette section skip, scale f32)

src/sim/
  ├─ components/
  │   ├─ cloak.rs            [NEW or extend existing]
  │   ├─ emp.rs              [NEW]
  │   ├─ iron_curtain.rs     [NEW]
  │   ├─ warp.rs             [NEW]
  │   └─ mirror.rs           [NEW or computed-at-render]
```

### Interfaces / contracts

```rust
// src/render/palette_textures.rs
pub struct PaletteSet {
    pub palette_tex: wgpu::Texture,
    pub remap_lut_tex: wgpu::Texture,
    pub bind_group: wgpu::BindGroup,
}

impl PaletteSet {
    pub fn new(device, queue, palette: &Palette, houses: &[HouseColor]) -> Self;
    pub fn rebuild_remap(&mut self, queue, houses: &[HouseColor]);
}

// src/render/unit_atlas.rs
pub struct UnitSpriteKey {
    pub type_id: u32,
    pub facing: u8,
    pub layer: VxlLayer,
    pub frame: u8,
    pub slope_type: u8,
}
// note: house_color removed

// src/render/batch_renderer.rs (added fields on SpriteInstance)
pub struct SpriteInstance {
    // existing: pos, size, uv_origin, uv_size, depth, ...
    pub house_color_idx: u32,
    pub fx_flags: u32,
    pub fx_params: [f32; 4],
    pub ic_tint: [f32; 4],
}
```

### Data flow

```
Boot:
  PaletteSet::new(palette, houses) — uploads palette + remap_lut_tex once
  UnitAtlas::build() — rasterizes each (type, facing, layer, frame, slope) → u8 tile
                       NO house_color in the loop. NO house_remap pass.

Per-tick (sim):
  components Cloak/EMP/IC/Warp updated by sim.

Per-frame (render):
  For each unit entity:
    key = UnitSpriteKey { type, facing, layer, frame, slope }
    entry = atlas.get(key)
    instance = SpriteInstance {
        ...positional...,
        house_color_idx: house_map[owner],
        fx_flags:       compute_fx_flags(entity),
        fx_params:      compute_fx_params(entity),
        ic_tint:        compute_ic_tint(entity, current_tick),
    }
    push to instance buffer

Single instanced draw:
  bind_groups: [atlas_tex, palette_set]
  fragment shader: byte → remap → palette → fx
  output: RGBA to framebuffer
```

### Error handling

- **Atlas miss** (key not found): warn-log per
  `feedback_silent_render_failures` memory, render with a magenta-key
  sentinel sprite. Don't silently skip.
- **Palette/remap_lut size mismatch**: assert at boot; fail-fast.
- **Unknown house_color_idx in shader**: shader bound-checks via clamp; falls
  back to identity remap for safety.
- **Theater palette swap**: `PaletteSet::rebuild_remap` invalidates GPU
  resources; atlas does NOT need rebuild (atlas is theater-independent
  because it stores palette indices).

### Testing strategy

- **Pixel-comparison vs gamemd screenshots** for: stationary Grizzly (no FX),
  cloaked Grizzly, EMP'd Grizzly, IC'd Grizzly, warping IFV, mirror reflection
  on water tile. One screenshot per FX per house color.
- **Atlas memory assertion**: at 30-player saturation (run a 30-player AI
  skirmish, let the atlas fill for ~5 minutes), assert
  `atlas.gpu_memory_bytes() < 200 MB`.
- **Specular highlight pixel test**: rasterize a Grizzly with the new s=3.0,
  pick 3 known-position highlight pixels, compare to gamemd capture.
- **Color-0 transparency invariant**: rasterize 100 random voxel data
  arrangements, assert no atlas tile contains a non-zero byte at a "should
  be transparent" position AND no zero byte at a "should be opaque" position.
- **Determinism unit test**: render the same entity twice with same key, assert
  identical sprite output (atlas is content-addressed by key).

### Determinism considerations

This is a render-only change. The sim layer is unchanged except for the
addition of FX components, which are per-entity state machines updated by
sim ticks. The new components must:
- Hash into `World::state_hash` (per CLAUDE.md determinism contract).
- Update deterministically (no `f32` in update logic — use `fixed`-point or
  integer ticks).
- Have explicit tick-order placement in `World::advance_tick` (probably in
  the existing post-combat updates phase, not a new phase).

Render does not affect determinism; the sim hash already excludes render
state.

## Architectural Decisions

- **Single atlas across houses** (vs gamemd's per-instance cache): different
  internal data flow, same observable output. Per CLAUDE.md, internals are
  ours. Justified by 30-player scale target and GPU pipeline.
- **Fragment-shader remap + FX**: keeps GPU pipeline; avoids CPU blit
  regression. Shader branches are uniform-controlled (one branch per sprite,
  not per fragment).
- **R8Uint atlas tiles**: 4× storage win over RGBA. Trade-off: must use
  `textureLoad` (no filtering) — fine because voxel sprites are pixel-art.
- **FX as per-instance uniforms**: extensible without atlas re-renders. Adding
  a new FX is a shader branch + uniform field, not a key dimension.
- **Color 0 is the transparency sentinel**, hard-invariant matching gamemd.
  Rasterizer must never write byte 0 to atlas; clear must zero-fill. Already
  consistent with gamemd's visibility-map convention.
- **Per-theater palette set**: theater swap is rare; palette+LUT rebuild is
  cheap (a few KB of texture upload).

### Patterns followed
- Existing module hierarchy (sim/render boundary preserved).
- Existing batch_renderer instance-buffer pattern (just adds fields).
- Existing `feedback_silent_render_failures` memory: warn on atlas miss,
  don't silently skip.
- Existing `feedback_no_engine_refs_in_comments` memory: no gamemd addresses
  in Rust source comments.

### Patterns deviated from
- Atlas tile format changes from RGBA to R8Uint. New format requires
  shader-side decode. Documented here and in the sprite_shader.wgsl header.
- New component cluster for visual FX (Cloak, EMP, IC, Warp, Mirror). Aligns
  with existing per-entity component pattern; no new architectural concept.

### Tech debt introduced
- Mirror rasterizer (Phase 6) requires z-test variant which we don't have
  yet. Deferred.
- Sub-tick facing SLERP (gamemd does this for Drive/Ship) — we currently
  quantize. If visible drift surfaces, follow-up plan.

## Alternatives Considered

- **Approach A — keep direct-atlas with house dimension.** Rejected: memory
  scales O(houses × ...), exceeds 30-player budget. The bug-fixes alone
  (header, specular, normals) wouldn't address the architectural cap.
- **Approach B — match gamemd literally with CPU visibility-map intermediate.**
  Rejected: contradicts the Vulkan/wgpu pipeline; reverts to CPU per-pixel
  blit; would tank performance vs current GPU-batched approach.
- **Approach D — per-instance LRU cache like gamemd.** Rejected: less
  GPU-friendly batching, harder to predict memory usage, complicates the
  drawing loop. Gamemd's choice was sensible for software rendering on a
  1GHz CPU; we have a GPU.
- **Approach E — multi-level atlas (base + per-house overlay).** Rejected:
  more complex than C without enabling shader-driven FX more cheaply. The
  shader-remap path subsumes overlay completely.

## Phasing / Write-Plan Sequence

This design is not a single PR. It splits into 7 phases, each independently
shippable. Phase 0 is independent of the rest; Phases 1-5 build on Phase 1.

### Phase 0 — Trivial parity fixes (independent)
*~50 LOC, can ship anytime; no architectural change.*

- `vxl_file.rs`: `VXL_HEADER_SIZE = 32`; parse variable palette section.
- `vxl_file.rs`: tailer `+0x0C` re-typed `f32 scale`.
- `vxl_normals.rs`: `SPECULAR_STRENGTH = 3.0`; iterate 0..245; fix the stale
  comment "252-255 dup of 251" → "245-249 dup of 244".
- (Optional) Add modes 1 (16 entries) and 3 (64 entries) tables. Low retail
  impact; defer if not blocking.

### Phase 1 — Atlas format change + shader remap (no FX yet)
*Largest single change. Foundational for all FX phases.*

- Convert `vxl_raster.rs` output to `Vec<u8>` instead of `Vec<[u8; 4]>`.
- Convert `vxl_compute.rs` resolve pass to write u8.
- Drop `house_color` from `UnitSpriteKey`.
- Atlas texture: `R8Uint`.
- New: `palette_textures.rs` with `PaletteSet`.
- Fragment shader: byte → remap → palette → output.
- Per-instance uniform: `house_color_idx`.
- Verify visual parity with current renderer (pixel test).
- Verify atlas memory drops at 30-player saturation.

### Phase 2 — Cloak FX
- Sim component: `Cloak { state: enum, transition_phase: f32 }`.
- Sim integration: existing cloak-eligible classes (Mirage Tank, Yuri's IFV,
  spy on certain conditions). Ticks update transition_phase.
- Shader branch: alpha multiplied by `fx_params[0]`. Optional shimmer noise.
- FX uniform population in `app_instances/units.rs`.

### Phase 3 — EMP FX
- Sim component: `EmpEffect { remaining_ticks: u32 }`.
- Shader branch: desaturate to luminance, clamp dim.

### Phase 4 — Iron Curtain FX
- Sim component: `IronCurtain { remaining_ticks: u32 }`.
- Shader branch: RGB tint overlay (red→white pulse over duration).

### Phase 5 — Warp / Chrono fade
- Sim component: `WarpEffect { phase: f32, in_or_out: enum }`.
- Shader branch: scanline-noise alpha fade.

### Phase 6 — Mirror rasterizer + water reflections
- Atlas key gains `mirror: bool` (or treat mirror as a fragment-shader Y-flip
  with separate sub-render).
- New rasterizer variant with z-test (matches gamemd dispatch slots 6/7).
- Water-reflection sub-pass.

### Phase 7 — Sub-tick facing SLERP (if visible drift)
- Replace facing-bucket lookup with SLERP between adjacent buckets.
- Drive/Ship only (per gamemd).
- Defer until pixel test reveals visible aliasing.

## Success Criteria

The design ships when:
- Atlas memory at 30-player saturation is under 200 MB (vs current ~1.5+ GB
  expected at scale).
- Specular highlights pixel-match gamemd within 1 LSB on a Grizzly capture.
- Color-0-transparency invariant holds for all rasterized atlas tiles.
- All 5 FX (cloak/EMP/IC/warp/mirror) render with player-observable parity to
  gamemd captures.
- The `sim/ NEVER depends on render/` invariant is preserved.
- The new FX components hash deterministically into `World::state_hash`.

---

## Hand-off

Design is ready. Suggested next step: **`/write-plan`** scoped to Phase 0 + 1.
Phase 0 is bite-sized parity fixes that can ship independently. Phase 1 is the
largest piece — atlas format change + shader remap, no FX wiring yet.
Subsequent phases (2-7) each get their own write-plan when ready, since they
are independently shippable.

Park, refine further, or proceed to `/write-plan` for Phase 0 + 1?
