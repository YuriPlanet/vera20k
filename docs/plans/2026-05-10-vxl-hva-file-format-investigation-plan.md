---
title: VXL + HVA File Format & Consumer Pipeline — Investigation Plan
status: awaiting approval
---

# VXL + HVA File Format & Consumer Pipeline — Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass. Execute it by running
> `/re-investigate` with this plan loaded as context, OR dispatch the function inventory
> to subagents in batches of 5–8 (see Section 10). The deliverable is
> [ra2-rust-game-docs/VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md) (split into two files if
> length warrants).

**Topic:** On-disk byte layout of VXL/HVA, the loader → drawer pipeline, per-section
matrix composition, and TS-legacy code that must NOT be ported.

**Scope Size:** **Large** — 35 functions in inventory, ~10 INI keys touch the surface.
This is at the upper end of single-investigation scope; the recommended execution
strategy (Section 10) is **batched subagents in 3 phases**.

**Est. Effort:** Anchored at ~15–30 min per FULL function, ~5–10 min per MEDIUM, ~2–5 min
per LIGHT. Inventory: 14 FULL + 13 MEDIUM + 8 LIGHT → roughly **8–12 hours** of
`/re-investigate` work, with a Phase-1 checkpoint at ~3–4 hours.

**Prior Research:** Four reports cover *runtime* voxel rendering — none cover the
*on-disk* byte layout. See Section 2.

**Expected Output:**
[docs/research/VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md)
(or split into `VXL_FILE_FORMAT_GHIDRA_REPORT.md` + `HVA_FILE_FORMAT_GHIDRA_REPORT.md`
if combined doc exceeds ~1500 lines).

**Next Pipeline Step:** `/brainstorm` if the disparity surface is large; otherwise
`/write-plan` for a parser-fix patch. Likely outcome: parity audit findings → small
targeted fixes to `src/assets/vxl_file.rs`, `src/assets/hva_file.rs`,
`src/assets/vxl_decode.rs`, `src/render/vxl_normals.rs`, `src/render/vxl_raster.rs`.

---

## 1. Goal

Produce a verified specification of how gamemd.exe parses a VXL file and a paired HVA
file, how it composes per-section transforms (HVA × facing × tilt × turret/barrel
override) per draw, and which fields/flags/code paths are dormant TS legacy.

The resulting document must let a Rust implementer answer, for any byte in a `.vxl`
or `.hva` file: "What does gamemd do with this byte, and is the answer the same in
YR as in TS?" — without re-opening Ghidra.

The five concrete questions the report must answer:
1. **VXL on-disk layout** — exact offsets/sizes of header, palette, limb headers, body
   span tables, voxel-run encoding, tailers; endianness; alignment; what differs from
   the in-memory layout already documented at 0x755DB0.
2. **HVA on-disk layout** — exact byte structure of filename header, frame/section
   counts, section names, matrix block ordering (frame-major vs section-major),
   matrix layout (3×4 or 4×3, row- vs column-major, translation column index).
3. **Pairing semantics** — what happens when HVA `section_count != VXL limb_count`,
   when section names mismatch, when frame_count == 0 or 1; how gamemd selects the
   HVA frame for the current sim tick.
4. **Per-section composition** — concrete matrix multiplication order including
   turret/barrel override, body tilt, facing, sub-cell offset; which limb is "body"
   vs "turret" vs "barrel" and how the engine identifies them.
5. **TS-legacy register** — list every flag, field, code path, and normals-table
   entry that exists in the format/binary but is never reached or always-default in
   stock YR. Active-in-YR: Yes/No/Conditional for each.

---

## 2. Prior Research Inventory

| Report | Scope | Confidence | Known Gaps |
|---|---|---|---|
| [VOXEL_RENDERING_ANALYSIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VOXEL_RENDERING_ANALYSIS.md) | Software rasterization pipeline; in-memory layout post-load; references loader at `0x00755DB0` | HIGH on rendering, MEDIUM on loader | **No on-disk byte layout** — describes loaded memory, not file bytes |
| `VOXEL_SLOPE_TILT_SYSTEM.md` | 20 slope types, tilt angles, body-vs-turret rotation, matrix `Rz×Rx×Rz⁻¹` construction | HIGH | Does not touch file format |
| [VXL_DRAW_MATRIX_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_DRAW_MATRIX_GHIDRA_REPORT.md) | 4×3 row-major matrix, facing lookup `0x7559B0`, SLERP `0x755A40`, table `0xB45188`, quaternions `0xB43188` | HIGH | Does not touch file format |
| [VOXELANIMCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VOXELANIMCLASS_GHIDRA_REPORT.md) | Bouncing voxel debris physics, `BounceClass`, `VoxelAnimTypeClass` INI parsing | HIGH | Does not touch file format |

**Conflicts between reports:** None found. All three matrix-touching reports agree on
**3×4 row-major, 12 floats, translation in column index 3 of each row (linear indices
3, 7, 11)** — this is the same convention `src/assets/hva_file.rs` already uses.

**Gap confirmed:** The on-disk byte layout has **never been documented from the
binary**. Existing Rust constants (`VXL_HEADER_SIZE = 802`, `SECTION_HEADER_SIZE = 28`,
`SECTION_TAILER_SIZE = 92`, `HVA_MIN_SIZE = 24`, `SECTION_NAME_SIZE = 16`) are
**unverified against gamemd.exe**.

---

## 3. Function Inventory

35 functions across 3 phases. **Phase 1 must be completed and checkpointed before
Phase 2 begins** — Phase 1 alone produces a usable file-format skeleton.

### Phase 1 — Core (14 functions)

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Risk |
|---|---|---|---|---|---|---|
| 1 | 1 | `0x00755DB0` | `VXL_Load_File` | **Primary VXL loader** — reads "Voxel Animation\0" magic, header, palette, limbs, bodies, tailers. Master spec for VXL on-disk format. | FULL | Low |
| 2 | 1 | **TBD — must locate** | *(unlabeled)* `HVA_Load_File` | **Primary HVA loader** — Ghidra has not labeled it. Likely a sibling of #1, reachable via `MotionLibClass` / `VoxLibClass` vtable, or via `TechnoTypeClass` voxel asset slot xref (offsets near `+0xA4` / `+0xE00`). **Locating this function is the first Phase-1 task.** | FULL | Low — but verify before assuming |
| 3 | 1 | `0x00758950` | `FUN_00758950` | Palette/section setup helper called from #1 before tailer loop — likely populates global decode buffers. | FULL | Low |
| 4 | 1 | `0x00758B70` | `VXL_BuildRemapTable` | Builds VXL palette remap; called for every loaded VXL. Ties palette block layout to runtime. | FULL | Low |
| 5 | 1 | `0x007559B0` | `VXL_GetFacingMatrix` | Returns precomputed facing matrix for an exact facing index. Anchor for facing-→matrix mapping. | FULL | Low |
| 6 | 1 | `0x00755A40` | `VXL_InterpolatedFacing` | SLERPs between two facing matrices for sub-tick smoothing. Verifies how facing interpolation feeds into composition. | FULL | Low |
| 7 | 1 | `0x00458810` | `BuildVXLTurretMatrix` | Composes turret/barrel pivot translates + Z-rotate + Y-rotate + body tilt for unit voxel. **Per-section composition entry.** | FULL | Low |
| 8 | 1 | `0x00453C98` | `GetTurretDrawPosition` | Caller of #7 — determines per-section transform for unit turret/barrel. | FULL | Low |
| 9 | 1 | `0x004AFF60` | `DriveLocomotionClass__Draw_Matrix` | Composes ground-vehicle body matrix (facing + slope tilt). Already partially in `VOXEL_SLOPE_TILT_SYSTEM.md` — extend, don't redo. | MEDIUM | Low |
| 10 | 1 | `0x0069F670` | `ShipLocomotionClass__Draw_Matrix` | Ship body matrix; differs from drive in tilt handling. | MEDIUM | Low |
| 11 | 1 | `0x004CFB00` | `FlyLocomotionClass__Render_Matrix` | Airborne body matrix; verify whether it ignores slope. | MEDIUM | Medium — confirm air units actually use this in YR |
| 12 | 1 | `0x00756590` | `VXL_Section_Rasterizer` | Per-limb rasterizer dispatch. Closes the loop from loader → composer → renderer. | MEDIUM | Low |
| 13 | 1 | `0x00754CB0` | `VXL_MasterLighting_Init` | Reads light dir/diffuse/ambient — touches normals-table selection. | MEDIUM | Low |
| 14 | 1 | `0x00758670` | `VXL_SimpleLighting` | Default lighting path; accesses RA2-vs-TS normals table via `tailer.normals_mode`. **Identifies the two normals-table data symbols.** | FULL | Low |

**Phase 1 checkpoint deliverable:** A draft VXL header / limb-header / body / tailer
spec, a draft HVA header / matrix-block spec, and an answer to "row-major or
column-major" verified from at least two reading sites in the binary.

### Phase 2 — Depth (13 functions)

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Risk |
|---|---|---|---|---|---|---|
| 15 | 2 | `0x00758EA0` | `VXL_NearestColorMatch` | Helper for #4. Confirms palette format. | LIGHT | Low |
| 16 | 2 | `0x00756860` | `VXL_Quad_Rasterizer` | Inner loop of #12; reveals how per-voxel `(x,y,z,color,normal)` is consumed. | MEDIUM | Low |
| 17 | 2 | `0x007542F0` | `VXL_Sort_Rasterize` (entry A) | Sort + rasterize pipeline. Verify which entry is live. | MEDIUM | **Medium** — duplicate suggests TS-era split |
| 18 | 2 | `0x00754510` | `VXL_Sort_Rasterize` (entry B) | Second entry; possibly shadow vs main. | MEDIUM | **Medium** — same |
| 19 | 2 | `0x00753D00` | `VXL_Init_BlinnPhong` | Init Phong tables. Pairs with #20. | MEDIUM | **Medium** — verify YR ever selects Phong |
| 20 | 2 | `0x007586F0` | `VXL_BlinnPhongLighting` | Blinn-Phong shader path. May be TS-only. | MEDIUM | **High** — flag for TS register if unreached in YR |
| 21 | 2 | `0x007DF9C0` | `VXL_Rasterizer_RenderMode` | Dispatcher across rasterizer variants. Reveals which variants are reachable. | MEDIUM | **Medium** — variant set may include TS |
| 22 | 2 | `0x00753E00` | `VXL_Clear_TileMap` | Per-frame bookkeeping; touches draw ordering. | LIGHT | Low |
| 23 | 2 | `0x00753F90` | `VXL_Submit_Billboard` | Submits billboard quad to render queue. | LIGHT | Low |
| 24 | 2 | `0x005AE750` | `Matrix3x4_BuildAxisAngleRotation` | Axis-angle rotation primitive used through pipeline. Anchors matrix conventions. | MEDIUM | Low |
| 25 | 2 | `0x005AF980` | `Locomotion_Matrix` | Generic locomotion-matrix dispatcher. | MEDIUM | Low |
| 26 | 2 | `0x00754C00` | `VXL_LightDirection_Setup` | Light vector built per-section. | MEDIUM | Low |
| 27 | 2 | `0x00749F30` | `VoxelAnimClass__AI` | Per-tick voxel-anim update; **reveals HVA frame-selection cadence** for animated voxels. Cross-check against [VOXELANIMCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VOXELANIMCLASS_GHIDRA_REPORT.md). | MEDIUM | Low |

### Phase 3 — Context & Edges (8 functions)

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Risk |
|---|---|---|---|---|---|---|
| 28 | 3 | `0x00757120` | `VXL_Rasterizer_Mirror` | Mirrored variant — water reflection or shadow. Verify reachable in YR. | LIGHT | **High** — likely TS-era mirror code |
| 29 | 3 | `0x007540F0` | `VXL_Submit_BoundingBox` | Debug/dev geometry. | LIGHT | **High** — likely dev-only, never live in YR |
| 30 | 3 | `0x005F5B90` | `ObjectClass__DrawVoxelShadow` | Voxel shadow draw; uses `Shadow_Matrix` from locomotors. | LIGHT | Low |
| 31 | 3 | `0x0046B0C0` | `VoxelAnim__Draw` | High-level VoxelAnim draw entry. | LIGHT | Low |
| 32 | 3 | `0x00748AF0` | `VoxelDebris__Render` | Debris consumer of #14/#17. | LIGHT | **Medium** — debris cosmetic, but YR-active; confirm |
| 33 | 3 | `0x0043E63E` | `FUN_0043DA80` (BuildingClass turret matrix) | BuildingClass voxel-turret matrix path; mirrors #8. Building voxels use a separate path. | LIGHT | Low |
| 34 | 3 | `0x007493B0` | `VoxelAnimClass__Constructor` | Spawn entry — reveals param contract for animated voxels. | LIGHT | Low |
| 35 | 3 | `0x0074B050` | `VoxelAnimTypeClass__ReadINI` | `[VoxelAnimTypes]` parser — verifies which INI keys map to per-anim frame logic. | LIGHT | Low |

**Total: 35 functions (14/13/8 across phases). Sizing class: Large.**

---

## 4. Detail Checklist

### 4.1 VXL on-disk byte layout

- [ ] **File header**: confirm magic = `"Voxel Animation\0\0"` (16 or 32 bytes? Rust uses 16); `palette_count` field offset/size; `limb_count` and `tailer_count` (relationship — always equal?); `body_size` (header value vs computed body extent); `unknown1`, `unknown2` u32 fields. Verify total header size against Rust's `VXL_HEADER_SIZE = 802`.
- [ ] **Palette block**: 256 × 3 RGB entries (768 bytes)? Verify the `+2` remap bytes (first/last remap indices) Rust currently reads.
- [ ] **Limb header** (`SECTION_HEADER_SIZE = 28` per Rust): name (32 vs 16 bytes? Rust's split — verify), `limb_number` u32, two unknowns, `hva_section_index` correlation field.
- [ ] **Body data per limb**: `span_start_off`, `span_end_off`, `data_span_off` u32 triplet — confirm.
- [ ] **Span pointer table layout** — `size_x × size_y` entries of i32 offsets into span data; `-1` = empty column. Rust assumption at `vxl_decode.rs:56–65` — verify.
- [ ] **Voxel run encoding** — `[z_skip: u8, count: u8, (color: u8, normal: u8) × count, dup_count: u8]`. **The trailing `dup_count` byte is unverified** — Rust comment at `vxl_decode.rs:143` says "validation/padding — unused by us." Confirm gamemd's behavior: does it validate equality, ignore it, or use it for something else? **This byte's purpose is a parity-critical question.**
- [ ] **Tailer per limb** (`SECTION_TAILER_SIZE = 92`): `bounds[6]` (min_x, min_y, min_z, max_x, max_y, max_z floats), `scale` f32, `transform[12]` (3×4 row-major), `size_x`/`size_y`/`size_z` u8s, `normals_mode` u8. Verify exact field order — Rust's order is an assumption.
- [ ] **`normals_mode` semantics**: confirm mode 2 = TS (36 normals), mode 4 = RA2 (256 normals). Are modes 0, 1, 3 valid? What does gamemd do with an unknown mode?
- [ ] **`limb_count` vs `tailer_count`**: Rust assumes equal. What does gamemd do if they differ?
- [ ] **Endianness**: confirm everywhere (Rust uses little-endian; spot-check at least 3 reads).

### 4.2 HVA on-disk byte layout

- [ ] **File header**: 16-byte filename string (ASCII, null-padded?) — Rust assumes 16. Verify against the loader's first read.
- [ ] **`frame_count`, `section_count` u32s** — confirm offsets and sign.
- [ ] **Section name table**: `section_count × 16` bytes (Rust assumption) or `× 24`? Check.
- [ ] **Matrix block ordering**: frame-major (`for f in frames: for s in sections: matrix`) vs section-major. **Rust assumes frame-major** — confirm.
- [ ] **Matrix layout per section**: 12 floats — row-major 3×4 (3 rows, 4 cols, translation in col 3) per Rust. Cross-check against [VXL_DRAW_MATRIX_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_DRAW_MATRIX_GHIDRA_REPORT.md) claim and a fresh read of the loader. Verify translation indices (Rust uses 3, 7, 11).
- [ ] **Section-name → VXL-limb pairing rule**: by name match, by index, by both? What if HVA names don't match VXL limb names?
- [ ] **`section_count != limb_count`** behavior: extra sections ignored? Missing sections → identity matrix? Crash?
- [ ] **`frame_count == 0` or `frame_count == 1`** edge cases.

### 4.3 VXL ↔ HVA per-frame composition

- [ ] **Frame selection per sim tick**: rate-limit (frames per game-tick), wrap policy (loop, hold, ping-pong?), special "idle" vs "moving" frame (does gamemd have either?).
- [ ] **Composition order** for unit voxel draw — exact multiplication sequence including: locomotor body matrix, slope tilt, world→isometric, HVA section transform, optional turret/barrel override, optional sub-cell offset. Cross-reference against Rust `vxl_raster.rs:332–334`.
- [ ] **Turret/barrel override**: how gamemd identifies which limb is turret vs barrel vs body. By name? By index? By INI? Confirm via #7/#8.
- [ ] **Per-section facing override**: how `body_facing` and `barrel_facing` (separate game-state values) get applied to specific sections (Rust uses `UNIT_FACING_STEP=4`, `TURRET_FACING_STEP=2`).
- [ ] **`TurretCount=`, `BarrelLength=`, `BarrelThickness=`, `TurretOffset=`** INI keys — how each one feeds into composition.
- [ ] **Sub-cell offset** (the half-cell offset that drove the slope-tilt fixes) — verify whether it lives in the locomotor matrix, the HVA matrix, or a separate post-multiply.

### 4.4 Voxel rendering math

- [ ] **Normals tables**: locate the two global data symbols (RA2 mode 4, TS mode 2). Verify entry counts (256 vs 36) and value layout (xyz f32 triplets? quantized?). Check if any other modes are referenced.
- [ ] **Normals mode 2 entries 252–255**: Rust at `vxl_normals.rs:22` claims they are duplicates of 251. Verify.
- [ ] **VPL palette lookup**: confirm page is selected by normal-vs-light-dir dot product, indexed within page by voxel color. What goes into the z-buffer? (Rust uses 16.16 fixed-point truncation.)
- [ ] **Voxel-to-screen projection**: scale factor, isometric matrix, the per-side half-cell offset. Already covered in `VOXEL_SLOPE_TILT_SYSTEM.md` — extend only if discrepancies surface.
- [ ] **Camera constants**: `CAMERA_PITCH_DEG=60°`, `WORLD_YAW_OFFSET_DEG=45°` (Rust). Verify against gamemd's hardcoded values.
- [ ] **Edge tilt ≈ 29.88°**, **corner tilt ≈ 22.10°** — Rust constants `EDGE_TILT_RAD ≈ 0.5215`, `CORNER_TILT_RAD ≈ 0.3859`. Already verified in prior docs; cross-check rather than re-derive.

### 4.5 TS-Legacy Risk Register (live during planning, expand during execution)

- [ ] **Blinn-Phong path** (#20 `0x007586F0`) — does any YR-reachable code path select this over `VXL_SimpleLighting`? If never, mark NOT-LIVE-IN-YR.
- [ ] **`VXL_Rasterizer_Mirror`** (#28 `0x00757120`) — water reflection? Or TS-era visual? Trace callers; if only debug or TS-tagged, mark NOT-LIVE-IN-YR.
- [ ] **`VXL_Submit_BoundingBox`** (#29 `0x007540F0`) — almost certainly dev-only. Confirm.
- [ ] **Two `VXL_Sort_Rasterize` entries** (#17, #18) — if one is shadow-only and one is main, document; if one is dead, mark.
- [ ] **`SpecialFlags`-gated voxel features** — grep gamemd for any `SpecialFlags &` near voxel pipeline functions.
- [ ] **Tailer fields beyond `bounds + scale + transform + size + normals_mode`** — if the 92-byte tailer has unused trailing fields, list them and mark NOT-LIVE-IN-YR if the loader copies them but no consumer reads them.
- [ ] **TS normals mode (mode 2)** — is it ever selected by a stock YR `.vxl`? If only legacy assets use it, document but don't remove.
- [ ] **Vein/Tiberium-tagged voxel anim flags** in `VoxelAnimTypeClass` — already partially documented; ensure none gate the file-format readers.

---

## 5. INI Keys in Scope

VXL/HVA on-disk format is data, not INI — the INI surface is small. Investigation
must verify how each key feeds into the composition pipeline (Section 4.3), not how
it's parsed (parsing is already documented elsewhere).

| Key | Section | Default | Suspected Purpose | Currently Parsed in Rust? |
|---|---|---|---|---|
| `Voxel=` | `[UnitTypes]/[AircraftTypes]` per-unit | `no` | Selects VXL render path over SHP | Yes |
| `Image=` | per-unit | (defaults to unit name) | Asset basename → `<basename>.vxl` + `<basename>.hva` filename derivation | Yes |
| `TurretCount=` | per-unit | `0` | Number of turret sections; affects which HVA sections are turret-facing-driven | Yes |
| `TurretOffset=` | per-unit | `0` | Z translation of turret pivot — composition input | Yes |
| `BarrelLength=` | per-WeaponType | `0` | Affects barrel section pivot | Yes |
| `BarrelThickness=` | per-WeaponType | `0` | Visual offset for barrel section | Partial (verify) |
| `Bouncer=` (VoxelAnimType) | `[VoxelAnimTypes]` | varies | Selects bouncing physics for VoxelAnim — only relevant for #27/#34 | Yes |
| `AlphaImage=` | per-unit | none | Optional translucent voxel asset (rare) | Verify |
| `Tilts=` | (vehicle locomotor) | `yes` | Enables slope tilt for body matrix | Yes |
| `DefaultToLookDown=` | per-unit | `no` | Affects HVA frame selection convention | Verify |

The investigation does **not** need to re-document parser code for these keys — it
needs to confirm which struct offset each one ends up at and how that offset is
read by the composition functions in Phase 1.

---

## 6. Caller & Integration Map

| Caller | Calls Into | When Invoked | Should Executor Decompile? |
|---|---|---|---|
| Asset preload (game start) | `VXL_Load_File` (#1) | Each unique VXL filename referenced by `Image=` | YES — confirm filename canonicalization |
| Asset preload (game start) | HVA loader (#2, TBD) | Paired with #1 for each `Image=` | YES — find this function (Phase-1 priority) |
| `TechnoClass::Draw` (already documented) | Locomotor matrix (#9/#10/#11) → `VXL_Section_Rasterizer` (#12) | Per draw call per unit per frame | LIGHT — context is already in [VOXEL_RENDERING_ANALYSIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VOXEL_RENDERING_ANALYSIS.md) |
| `BuildingClass::Draw` | `FUN_0043DA80` (#33) | Per building voxel turret per frame | LIGHT — confirm path is reachable in YR |
| `VoxelAnimClass::AI` (#27) | HVA frame index for animated voxels | Per game tick per voxel-anim | MEDIUM — frame-selection cadence is parity-critical |
| INI loader (`VoxelAnimTypeClass::ReadINI` #35) | Sets type fields | Once per match | LIGHT — only verify offsets |

**Rust integration anchors** (verify against, do not change):
- `src/assets/vxl_file.rs` — parser; constants `VXL_HEADER_SIZE = 802`,
  `SECTION_HEADER_SIZE = 28`, `SECTION_TAILER_SIZE = 92`.
- `src/assets/hva_file.rs` — parser; constants `HVA_MIN_SIZE = 24`,
  `SECTION_NAME_SIZE = 16`.
- `src/assets/vxl_decode.rs` — span/run decoder; trailing `dup_count` purpose
  flagged unverified.
- `src/render/vxl_raster.rs:332–334` — composition order:
  `rotate_to_world × slope_matrix × section_translate × hva_bone_matrix × section_scale`.
- `src/render/vxl_normals.rs:15–22` — `normals_mode` 2/4 selection; entries 252–255 dedup.
- `src/render/vxl_compute.rs` — GPU-compute alternative renderer; consumes the same
  composed matrix.

**Callers NOT in scope** (justification): the upstream `MIX` archive locator, the
asset cache, and the network-replay save/load path. These are independent of the
file format itself.

---

## 7. TS-Legacy Risk Register (Pre-Execution)

Consolidated from the function inventory and Rust source notes. Each item must be
resolved by execution with explicit "Live in YR: Yes/No/Conditional".

1. **Blinn-Phong lighting (#19, #20)** — High risk. Confirm whether any
   stock-YR-reachable code selects it over `VXL_SimpleLighting`. If never selected,
   the pipeline can ignore it.
2. **`VXL_Rasterizer_Mirror` (#28)** — High risk. Likely TS-era. Verify by tracing
   callers. May still be live for water reflection.
3. **`VXL_Submit_BoundingBox` (#29)** — High risk. Likely dev/debug only.
4. **Two `VXL_Sort_Rasterize` entries (#17, #18)** — Medium risk. Document live vs
   dead; do not assume.
5. **`normals_mode` values other than 2 and 4** — what does gamemd do with mode 0,
   1, 3? Possibly TS-only or invalid. Document the dispatch.
6. **Trailing `dup_count` byte in voxel-run encoding** — flagged in
   `vxl_decode.rs:143` as unverified. May be a TS-era validation byte.
7. **Tailer trailing bytes** — if `SECTION_TAILER_SIZE = 92` is correct, and the
   field set Rust knows of doesn't add up to 92, the difference is unread bytes.
   List them as TS-legacy field candidates.
8. **`SpecialFlags`-gated voxel paths** — none currently known. Grep during
   execution to be sure.
9. **HVA matrix translation interpretation** — `VOXEL_SLOPE_TILT_SYSTEM.md`
   mentions multi-step rotation; verify gamemd doesn't apply a TS-era "extra
   transform" pass that we'd silently drop.
10. **Voxel-shadow path (#30)** — possibly different in YR than TS. Verify.

---

## 8. Current Rust Implementation Surface

Files the future audit will check findings against (do **not** modify in this
session — research only):

- [vxl_file.rs](src/assets/vxl_file.rs) — VXL parser, `from_bytes`, header/limbs/tailers.
  Constants on lines 1–30. Comment at line 59 ("Section tailer contains default
  transform from VXL file") flagged unverified.
- [hva_file.rs](src/assets/hva_file.rs) — HVA parser, frame×section matrix layout.
- [vxl_decode.rs](src/assets/vxl_decode.rs) — span/run decoder. Comment at line 143
  ("dup_count is validation/padding — unused by us") flagged unverified.
- [vpl_file.rs](src/assets/vpl_file.rs) — VPL palette lookup file (touched only at
  rendering, but file-format adjacent).
- [vxl_normals.rs](src/render/vxl_normals.rs) — normals tables and `normals_mode`
  selection. Lines 15–22.
- [vxl_raster.rs](src/render/vxl_raster.rs) — composition pipeline at lines 332–334;
  facing/slope inputs at 255–286; HVA translation scaling at 606–616.
- [unit_atlas.rs](src/render/unit_atlas.rs) — facing quantization
  (`UNIT_FACING_STEP=4`, `TURRET_FACING_STEP=2`).
- [vxl_compute.rs](src/render/vxl_compute.rs) — GPU compute renderer, consumes
  prepared `LimbRenderData`.

---

## 9. Deferred Open Questions

These the scoping pass could not answer; they become the executor's explicit list:

1. **What is the address of the HVA loader?** Ghidra has not labeled it. First
   Phase-1 task is to find it. Suggested entry points: xrefs from string `.hva`,
   xrefs from `MotionLibClass` / `VoxLibClass` vtable methods, or a sibling of
   `VXL_Load_File` (`0x00755DB0`). If still not found, search for the read pattern
   `read 16 bytes filename → read u32 frames → read u32 sections`.
2. **Are the two `VXL_Sort_Rasterize` entries (`0x007542F0`, `0x00754510`) main +
   shadow, or main + dead?**
3. **What is the `dup_count` byte's purpose at the end of each voxel span?**
4. **Does `normals_mode` accept values other than 2 and 4? What's the dispatch?**
5. **Is the Blinn-Phong path ever selected in stock YR?**
6. **Section-name → limb pairing rule** in HVA: by-name, by-index, or by-both?
7. **What does gamemd do when HVA `section_count != VXL limb_count`?**
8. **What is the tailer's byte breakdown?** `bounds(24) + scale(4) + transform(48)
   + size(3) + normals_mode(1) = 80`. What are the remaining 12 bytes (if
   `SECTION_TAILER_SIZE = 92` is correct)?
9. **Building voxel turret path (#33)** — does it use the same HVA pairing as units?
10. **Frame-selection cadence for `VoxelAnimClass`** — is it sim-tick-rate or a
    separate animation rate?

---

## 10. Execution Strategy

**Recommendation: Batched subagents, three phases, with checkpoint between.**

**Why batched:** 35 functions × 15–30 min each is 8–12 hours of single-threaded work.
Batched subagents (5–8 functions per batch, dispatched in parallel) cuts wall-clock
time substantially. Each batch returns a structured fragment; the executor
synthesizes between phases.

**Why phased:** Phase 1 alone yields a usable file-format skeleton. If Phase 1 reveals
the scope is wrong (e.g., `0x00755DB0` turns out to be a thin wrapper around the
real loader), the plan is revised before burning Phase 2/3 effort.

### Execution sequence

**Step 0 — Locate HVA loader (deferred Q1).**
Single targeted Ghidra MCP pass: trace xrefs from `.hva` strings; if dry, look
for sibling functions of `VXL_Load_File`; if still dry, search for the read
pattern. Add the discovered address to the inventory as #2.

**Phase 1 batch (14 functions including #2 once located):**
- Batch 1A: VXL loader chain — #1, #3, #4 (FULL each)
- Batch 1B: HVA loader + facing matrix — #2, #5, #6 (FULL each)
- Batch 1C: Composition + section dispatch — #7, #8, #12 (FULL/MEDIUM)
- Batch 1D: Locomotor matrices — #9, #10, #11 (MEDIUM each)
- Batch 1E: Lighting init + simple — #13, #14 (FULL on #14)

**Phase 1 checkpoint (mandatory):**
- Draft Sections 1–3 of the deliverable doc.
- Verify `SECTION_HEADER_SIZE`, `SECTION_TAILER_SIZE`, `VXL_HEADER_SIZE`,
  `HVA_MIN_SIZE`, `SECTION_NAME_SIZE` against gamemd reads.
- Resolve "row-major / column-major / 3×4 / 4×3" with two independent reading
  sites cited.
- If any constant disagrees with Rust, **stop and flag** before Phase 2.

**Phase 2 batch (13 functions):**
- Batch 2A: Rasterizer variants — #16, #17, #18, #21 (variant inventory + TS gating)
- Batch 2B: Lighting alternates — #19, #20, #26 (Phong path TS-gating)
- Batch 2C: Pipeline plumbing — #22, #23, #24, #25, #15
- Batch 2D: Voxel-anim AI — #27 (frame cadence)

**Phase 3 batch (8 functions):**
- Batch 3A: Edge paths — #28, #29, #30 (TS register)
- Batch 3B: Building/voxel-anim consumers — #31, #32, #33, #34, #35

**Document split criterion:** if combined doc exceeds ~1500 lines or 100KB, split
into `VXL_FILE_FORMAT_GHIDRA_REPORT.md` (Phase 1 batches 1A, 1C, 1D, 1E + Phase 2A)
and `HVA_FILE_FORMAT_GHIDRA_REPORT.md` (Phase 1 batch 1B + Phase 2D + relevant
parts of Phase 3).

---

## 11. Success Criteria

The executed research document(s) must:

- Answer every question in **Section 1** (the five concrete questions).
- Include every function from **Section 3** (or explicitly justify omission).
- Resolve every **deferred question from Section 9**, or re-document it as
  unresolved with a specific reason.
- State **"Active in YR: Yes / No / Conditional"** for every TS-legacy risk in
  Section 7.
- Cite **Ghidra address + function name** for every HIGH-confidence claim.
- Differentiate **verified-from-binary** vs **inferred** vs **assumed** for each
  finding.
- Include **a byte-for-byte VXL header diagram** and an **HVA header diagram**
  drawn from the actual loader reads (not from Rust assumptions).
- Include a **disparity list** for the Rust parsers in Section 8 — every constant,
  field order, and assumption that disagrees with gamemd, with severity
  (player-visible / internal-only).

The doc is **not done** if any of: row-major/column-major is hand-waved; the
`dup_count` byte is left unexplained; the HVA loader address is "TBD"; or any
function in Section 3 is silently dropped without a written justification.

---

## Sources

- **Ghidra addresses sampled (light scoping):** `0x00755DB0`, `0x00758950`,
  `0x00758B70`, `0x00758EA0`, `0x007559B0`, `0x00755A40`, `0x00458810`,
  `0x00453C98`, `0x004AFF60`, `0x004CFB00`, `0x0069F670`, `0x00756590`,
  `0x00756860`, `0x00757120`, `0x007DF9C0`, `0x007542F0`, `0x00754510`,
  `0x00753E00`, `0x00753F90`, `0x007540F0`, `0x00754CB0`, `0x00754C00`,
  `0x00753D00`, `0x007586F0`, `0x00758670`, `0x005F5B90`, `0x004AFF60`,
  `0x0069F670`, `0x004CFB00`, `0x005AF980`, `0x005AE750`, `0x00749F30`,
  `0x007493B0`, `0x0074AD80`, `0x0074B050`, `0x00748AF0`, `0x0046B0C0`,
  `0x0043E63E`. **Note:** HVA loader address is TBD — first task in execution.
- **Docs searched:** [ra2-rust-game-docs/VOXEL_RENDERING_ANALYSIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VOXEL_RENDERING_ANALYSIS.md),
  `VOXEL_SLOPE_TILT_SYSTEM.md`, [VXL_DRAW_MATRIX_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_DRAW_MATRIX_GHIDRA_REPORT.md),
  [VOXELANIMCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VOXELANIMCLASS_GHIDRA_REPORT.md), [OBJECTCLASS_DRAW_LIMBO_CELLLIST.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECTCLASS_DRAW_LIMBO_CELLLIST.md),
  [TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md), [UNIT_DRAW_EXTRAS_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNIT_DRAW_EXTRAS_REPORT.md),
  [RENDERING_PARITY_CHECKLIST.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RENDERING_PARITY_CHECKLIST.md).
- **INI files checked:** `ini/rulesmd.ini`, `ini/artmd.ini`.
- **Rust source surveyed:** `src/assets/{vxl_file,hva_file,vxl_decode,vpl_file}.rs`,
  `src/render/{vxl_raster,vxl_normals,vxl_compute,unit_atlas}.rs`,
  `src/bin/audit-assets.rs`.
- **Related plans:** `2026-05-10-vxl-slope-tilt-constants-plan.md`,
  `2026-05-10-vxl-slopes-9-20-investigation-plan.md` (slope-tilt-only — no
  format coverage).
