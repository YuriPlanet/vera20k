# gamemd.exe Z-Buffer Depth System

Reverse-engineered via Ghidra MCP (live decompilation of Yuri's Revenge `gamemd.exe`).
Documents the per-pixel Z-buffer system used for depth ordering between sprites.
Overview, sections 1, 3, 7 and 10 corrected 2026-09-07 after reading the live
leaf blitters; see the correction note in the Overview.

---

## 2026-09-08 implementation correction

The [depth continuation](../plans/2026-09-08-depth-occlusion-continuation.md)
supersedes this document's old Rust-status section and these native claims:
ordinary walls/overlays use SHP47F6A0 (flags4E00), not TMP547CF0; BUILDNGZ
uses a constant raw seed from the body/shape intersection, not the ordinary
scanline gradient. Raw shape frames clip without consuming shape values.
See the continuation for addresses, implementation and bounded validation.
Remaining historical sections are research context, not current source status.

## Overview

> **Correction (2026-09-07).** Earlier revisions claimed that SHP sprites drawn
> through `TechnoClass::DrawSHP` never read or write the Z-buffer per pixel, that
> the Z-writing blitters were dead code, and that BUILDNGZ.SHA was loaded but
> ignored. All three claims were wrong. They came from tracing the vtable slots at
> 0x74/0x78/0x130, which are the cloak/warp translucency path (z-mode 1-4), not the
> normal-object path. The live leaf blitters for normal objects were read from
> their bodies on 2026-09-07 (Ghidra, `gamemd.exe`) and do per-pixel Z work.
> VERA code and docs written against the old claim (the sprite "passthrough"
> depth contract, the BUILDNGZ removal in section 10) are DRIFT, not parity.

The original engine uses a **16-bit per-pixel Z-buffer** (`g_ZBuffer`,
`DAT_00887644`). Three blit families touch it:

```c
// TMP tiles (FUN_00547cf0) - per-pixel Z from tile Z-data, read + write, <= test:
pixel_z = z_shape_value + base_z;
if (pixel_z <= zbuffer[pixel]) { zbuffer[pixel] = pixel_z; screen[pixel] = color; }

// Building bodies (flags 0x6E00 -> 0x004990e0 / 0x004958d0) - read + write, < test,
// per-pixel Z-shape from BUILDNGZ.SHA:
if (base_z - zshape[x] < zbuffer[x]) { screen[x] = remap[...]; zbuffer[x] = base_z - zshape[x]; }

// Normal objects (flags 0x2800 -> 0x00494b60 / 0x00497fd0) - read + test only, no write:
if (base_z - zshape[x] < zbuffer[x]) { screen[x] = color; }   // zshape row is all-zero without a Z-shape
```

**Established from bodies (2026-09-07):**

- `TechnoClass_DrawSHP` (`0x00705e00`, `RET 0x40`, 16 args) builds the flag word
  as: `0x2000` when arg 8 != -1 (`0x00706129`), `0x4000` when byte arg 9 != 0
  (`0x00706136`), `0x800` unconditionally (`0x00706148`), `&= ~arg16`
  (`0x00706175`), then `|= 0x600` immediately before `CC_Draw_Shape`
  (`0x0070643b`). Z-mode bits 2/4/6 come only from the `vtable+0x68`
  visual-state result 1-4 (cloak/warp); jump table at `0x00706620`, case 0 ->
  `0x007063ff` (body draw, then a shadow pass with `(flags & ~6) | 0x601`).
- `BuildingClass_DrawBody` (`0x0043d290`, body site `0x0043d85f`) passes arg 8 = 2
  (gradient type), arg 9 = 1 (Z-write request), arg 12 = `g_BUILDNGZ_SHA`
  (`0x0089ddbc`; nulled only when the foundation width >= 8), arg 13 = 0
  (Z-shape frame), args 14/15 = `Type+0x1530/+0x1534` (`ZShapePointMove`) +
  (0xc6, 0x1be) - CellToPixel(foundation). **Building body flags = 0x6E00.**
  Bit 0x10 is never set: `CC_Draw_Shape` ORs it only when its stack arg 6 != 0
  (`0x004af101..0x004af115`) and DrawSHP pushes literal 0 (`0x0070643e`).
- `Blitter_selector` (`0x00490b90`) and `Blitter_selector_extended`
  (`0x00490e50`), with 0x10 clear and bits 1/2/4/0x20 clear:

| flags | standard slot -> vtable -> leaf | extended slot -> vtable -> leaf | Z behaviour |
|---|---|---|---|
| `0x6E00` (building body) | `+0xbc` -> `0x007e5630` -> `0x004958d0` | `+0x158` -> `0x007e53a0` -> `0x004990e0` | read, `<` test, **write** |
| `0x2E00` (`TechnoClass_DrawSHP` non-building callers: a8 != -1, a9 = 0, `0x800`, `0x600` ORed at `0x0070643b`) and `0x2800` (`TechnoClass__Draw` `0x00706640`; VXL cache blit) | `+0x98` -> `0x007e56f0` -> `0x00494b60` | `+0x138` -> `0x007e5420` -> `0x00497fd0` | read, `<` test, no write |

Both selectors (`0x00490b90`, `0x00490e50`) reach these slots through
`TEST [0x0081dc24]/[0x0081dc28], EAX` with both globals equal to `0x3000`
(critic re-read 2026-09-07); `0x200` and `0x400` are never tested, so `0x2E00`
and `0x2800` route identically. `Blitter_init 0x0048ebf0` fills the four slots
only on its 16-bpp branch, which is why two of the leaves lack Ghidra function
bodies.

- `0x004990e0` (`ExtendedBlitter__RLEZero_RemapIntensity_ZReadWrite`) is the
  live building path: building SHPs such as GACNST.SHP are format 3 on every
  frame, so `SHP_GetFrameCompressionFlag` (`0x0069e900`) selects the extended
  blitter. Its inner loop subtracts the Z-shape byte (`param_11`: the BUILDNGZ
  row, or the 32-byte all-zero table at `0x0089c568` when there is no Z-shape)
  per pixel, tests `< *zbuf`, and stores `base_z - zshape` on success.
- `0x004958d0` (format-1 fallback, undefined in Ghidra, read from bytes at
  `0x00495930-0x0049596d`): `MOV BX,[EDX]; CMP ECX,EBX; JGE skip; ...
  MOV [EDI],CX; MOV [EDX],CX` with `EDX += 2` per pixel and Z-buffer wrap.
  Z read, strict-less test, Z write.
- `0x00494b60` (0x2800, standard): `MOV AX,[EDX]; ADD EDX,2; CMP EBX,EAX; JGE
  skip; ... MOV [EDI],AX`. Z read + test, no store.
- `0x00497fd0` (0x2800, extended, also the VXL cache blit): `MOVSX EDX,[ECX]`
  (Z-shape byte), `EDI = base_z - EDX`, `CMP EDI,[EBX-2]; JGE skip`. Z read +
  test with per-pixel Z-shape, no store.
- Both walkers (`SHP_StandardBlitter` `0x004373b0`, `SHP_ExtendedBlitter`
  `0x00437a10`) fetch `g_ZBuffer` globally, hand the leaf a per-row 16-bit
  pointer, and step base Z per scanline from the gradient table (section 4).
  The gradient is per row; the test is per pixel.
- The `0x74/0x78/0x130` slots (cloak z-modes) and the `0x10c` slot
  (`0x007e54d0` -> `0x00497100`, the extended selector's `0x4000`-set /
  `0x800`-clear arm at `0x00490f45`; the true fallthrough arms are
  `+0x124`/`+0xd0`) exist but are not the normal-object path. The old "0x124 opaque, no Z" claim for z-mode
  0 was also wrong: z-mode 0 reaches the slots in the table above.

**Inferred (not re-traced in the same pass):**

- A unit drawn after a building in the Ground layer is hidden wherever the
  building wrote a nearer Z (its tall parts) and visible beside its base. Units
  do not write Z, so units never occlude each other through the Z-buffer;
  unit-vs-unit order is the layer Y-sort (section 7).
- Infantry: `InfantryClass Draw_It 0x00518F90` calls `vtable+0x50c` at
  `0x005195E2`; Infantry `+0x50c` = `0x0041c090`, which forwards all 16 args to
  `0x00705e00` (`RET 0x40`). Pushes: a8 = 2, a9 = 0 (`0x005195b5`), a7 =
  `[0x00825500]`, a12..a16 = 0, so no `0x4000` (section 4 table; critic
  re-read 2026-09-07).
- `vtable+0x43c` (Building `0x007e3ebc+0x43c` and Infantry alike) is
  `TechnoClass__ModifyCloakDrawFlags 0x0070ED80`, gated by
  `HouseClass__IsHumanPlayer` and `+0xc4` (`0x0041C010`, returns byte
  `this+0x1d8`). It returns `flags`, `flags|2` or `flags|4` only: it can move a
  human-owned object into the translucent slot family but cannot strip
  `0x4000`/`0x2000`/`0x800`. Closed.
- OpenTS (Tiberian Sun 2.03 reconstruction, `Documents\OpenTS`) has the same
  design: `BuildingClass::Draw_It` passes `zwrite = true` with the
  `BUILDNGZ.SHP` z-shape (`building.cpp:873-890`) and lands in
  `RLEBlitTransXlatAlphaZReadWrite` (`rlerle.h:240-306`); units and infantry
  pass `zwrite = false` and land in `RLEBlitTransXlatAlphaZRead`. Supporting
  evidence only; gamemd is the authority.

**Depth ordering for SHP sprites therefore relies on:**
1. **Terrain Z** written per pixel by `TMP_TileBlitter` in Phase 1
2. **Building Z** written per pixel (BUILDNGZ-shaped) by building body draws
3. **Per-pixel Z read** by every other object draw (test, no write)
4. **Screen-Y sort** within a layer and the **5-layer order**, which decide
   everything the Z-buffer does not (units vs units, equal depth)

Lower Z = closer to camera. Tiles test `<=`; sprite leaves test `<`.

**Visual-state z-mode** (from `vtable+0x68` / `GetVisualState` dispatch in
`TechnoClass::DrawSHP`). These bits select the cloak/warp translucency path;
they do not decide Z behaviour for normal objects:

| Return | Z-mode bits | Visual state | Path |
|--------|-------------|-------------|------|
| **0** | none | **Normal (opaque)** | `0x007063ff`; slots per the flags table above (Z read, and Z write for buildings) |
| 1 | `0x02` | Uncloaking (early) | `0x007064d4`; translucent slot family (0x74/0x78/0x12c) |
| 2 | `0x04` | Uncloaking (late) | `0x007065a3`; 50% blend slot family (0x130) |
| 3 | `0x04` | Cloaking (visible) | Same as case 2 |
| 4 | varies | Cloaking (depends) | `param_1[0x89]` flag -> 0x02 or 0x04 |
| 5 | - | Fully cloaked | Draw skipped |

**ALL normal (non-cloaked) objects return 0** - buildings, infantry, vehicles, and
aircraft. The dispatch chain: `BuildingClass_GetVisualState` (`0x004544a0`,
delegates when `+0x6ED = 0`) or `FootClass::GetVisualState` (`0x004da4e0`,
asks locomotor first) -> `TechnoClass_GetVisualState` (`0x00703860`) -> returns 0
when CloakState (`this[0x88]`) is 0. The only non-zero returns come from:
- **Cloaking/uncloaking** (CloakState != 0) -> returns 1-5 based on progress
- **TunnelLocomotionClass** in burrowed state -> returns 4 or 5

The Z-buffer is cleared to `0xFFFF` per dirty rect (not the whole surface)
via `FUN_007bcfb0`, called from `FUN_006d2b60` which iterates the dirty rect
list at `DAT_00b0ce7c`. Three separate subsystems provide the Z-shape depth data:

1. **TMP Z-data** - per-pixel depth baked into terrain tile files
2. **BUILDNGZ.SHA** - per-pixel depth overlay for building sprites
3. **Per-scanline Z-gradient table** - row-by-row depth accumulator


---

## 1. Z-Buffer Infrastructure

### Z-Buffer Surface

Allocated as a 0x30-byte object via `operator_new`, constructed by `FUN_007bc970`,
stored in global `DAT_00887644`. Created during game init in `FUN_006bb9a0`.

| Field | Address/Value | Purpose |
|-------|--------------|---------|
| Global pointer | `DAT_00887644` | 16-bit per-pixel depth surface |
| Origin X | `+0x00` | Screen/map origin X |
| Y origin | `+0x04` | Top of viewport in Z-buffer coords |
| Width | `+0x08` | Buffer width |
| Height | `+0x0C` | Buffer height |
| State/flags | `+0x10` | Initialized to 0 |
| Surface ptr | `+0x14` | Inner BSurface object (0x20 bytes) |
| Buffer start | `+0x18` | Pointer to pixel data start |
| Buffer end | `+0x1C` | Pointer to pixel data end (wrap check) |
| Buffer wrap | `+0x20` | Wrap-around size for circular buffer |
| Default Z | `+0x24` = `0x8000` (-32768 as signed short) | Base Z value for depth computation |
| Stride | `+0x28` | Width in pixels (uint16 elements per row) |
| Stride height | `+0x2C` | Buffer height (copy) |

### A-Buffer (Alpha/Auxiliary)

| Field | Address/Value | Purpose |
|-------|--------------|---------|
| Global pointer | `DAT_0087e8a4` | 16-bit auxiliary surface (intensity/remap overlay) |

### Base Z Formula

The base Z depends on the rendering path. Three variants exist:

**Tile renderer** (`FUN_00547cf0` — terrain/walls/ore):
```c
base_z = (ushort)(DefaultZ + YOrigin - screenY - spriteHeight)
         - (spriteHeight * heightLevel) / 2;
```

**SHP blitter, gradient field[5]=1** (entries 0, 1 — standard objects):
```c
raw = (ushort)(DefaultZ + YOrigin - screenY_offset) + z_height;
base_z = (raw / field[1]) * field[1];   // quantized (entry 0: field[1]=1, no-op)
```

**SHP blitter, gradient field[5]=0** (entry 2 — buildings):
```c
step = field[3] / field[2];             // entry 2: 3/1 = 3
raw = (ushort)(DefaultZ + YOrigin - spriteHeight - screenY_offset + 1) + z_height;
base_z = (raw / step) * step - spriteHeight / step;
accum_init = field[3] - spriteHeight % step;
if (accum_init == field[3]) { accum_init = 0; base_z += step_dir; }
```

- `DefaultZ` = `0x8000` (-32768 as signed short; set in constructor `FUN_007bc970`)
- `YOrigin` = Z-buffer `+0x04` (viewport top)
- `screenY` / `screenY_offset` = sprite's screen position components
- `spriteHeight` = draw rect height in pixels
- `z_height` = **CC_Draw_Shape stack slot 7 = DrawSHP a7 (z-adjust, pixels) − 2**
  (corrected 2026-09-07: forwarded at `0x004AF227` to `SHP_StandardBlitter`
  `[ESP+0x74]`, added at `0x0043753E`; brightness (slot 9) and tint (slot 10)
  never enter the Z block `0x004374BB–0x004375A7`). The older "CC param_10"
  naming was Ghidra's off-by-one. Building body a7 = `NormalZAdjust −
  AdjustForZ(Location.Z)`; see section 4.
- Building formula subtracts spriteHeight and quantizes to 3-unit boundaries
- **Tile `heightLevel`**: `CellClass+0x11B` (signed char, typically 0–14). Moves tile
  upward by `heightLevel * 15` pixels **and is the tile Z parameter**: both
  `TMP_TileBlitter` callers push `MOVSX [cell+0x11B]` as slot 10 and the base Z
  is `(DefaultZ + YOrigin − y − tileH) − tileH * heightLevel / 2`
  (`DAT_00AA1104`). `CellClass+0x10C` is pushed as slot 11 but used only as the
  palette-LUT brightness row (`0x00547D9C`, `*0x105 >> 11`, `<< 9`); it is a
  lighting value (section 8 correction, 2026-09-07).
- **Animation `z_height`**: `YDrawOffset + ZAdjust - AdjustForZ() - 2` (non-flat)
  or `- 3` (flat). `ZAdjust` from art.ini stored at `AnimTypeClass+0x348`,
  instance copy at `AnimClass+0x100`.

Objects further DOWN the screen get LOWER Z values (closer to camera).

### Z-Buffer Flags in Draw Calls

Corrected 2026-09-07. Z behaviour is **not** selected by bits 1-2. Those bits
(`0x02`/`0x04`/`0x06`) are the cloak/warp translucency modes from `vtable+0x68`.
For normal objects (z-mode 0) `Blitter_selector` (`0x00490b90`) and
`Blitter_selector_extended` (`0x00490e50`) dispatch on:

| Bit(s) | Value | Meaning |
|-----|-------|---------|
| 4 | `0x10` | Set in `CC_Draw_Shape` only when its stack arg 6 != 0 (`0x004af101..0x004af115`); `TechnoClass_DrawSHP` pushes literal 0 (`0x0070643e`), so object draws never carry it |
| 9 | `0x200` | Sprite centering (subtracts half frame width/height; `0x004af002`: `TEST AH, 0x2`). ORed by DrawSHP as part of `0x600` |
| 11 | `0x800` | Always set by `TechnoClass::DrawSHP` (`0x00706148`); selects the intensity-aware blitter families |
| 12-13 | `0x3000` | `0x2000` set when DrawSHP arg 8 (gradient type) != -1 (`0x00706129`) |
| 14 | `0x4000` | Set when DrawSHP byte arg 9 != 0 (`0x00706136`): Z-write request. `BuildingClass_DrawBody` passes 1 |

Resulting slots: `0x6E00` -> `+0xbc` / `+0x158` (Z read + write, buildings);
`0x2800` -> `+0x98` / `+0x138` (Z read only, other objects). Bit 10 (`0x400`)
is not tested in `CC_Draw_Shape` or the selectors.


---

## 2. TMP Z-Data (Terrain Tiles and Overlays)

### Source

Z-data is stored directly in the **TMP file format**, inside each sub-tile's cell header.

### TMP Cell Header (relevant fields)

| Offset | Size | Field |
|--------|------|-------|
| 12 | u32 | Relative byte offset from cell header to Z-data block |
| 36 | u8 | Flags — bit 1 (`0x02`) = has Z-data |

### Renderer: `FUN_00547cf0` at `0x00547cf0`

This is the per-cell tile renderer for 60×29 isometric diamond sprites (terrain tiles,
wall overlays, ore overlays). It renders TMP/overlay pixel data within the isometric
diamond defined by three mask tables:

| Address | Table | Purpose |
|---------|-------|---------|
| `DAT_00abb120` | Pixel data offsets | Per-scanline offset into tile frame data |
| `DAT_00aa074c` | Screen buffer offsets | Per-scanline offset into screen/zbuf scanline |
| `DAT_00aa154c` | Scanline widths | Number of pixels per scanline row |

### Z-Data Access

```c
piVar4 = frame_pointer;
if ((*(byte *)(piVar4 + 9) & 2) != 0) {       // flag bit 1 at byte 36
    z_shape_ptr = (byte *)(piVar4[3] + (int)piVar4);  // Z-data at offset 12
}
```

### Per-Pixel Z Test (inner loop at ~line 275)

```c
uint16_t pixel_z = (uint16_t)*z_shape_ptr + (int16_t)base_z;
if (pixel_z <= *zbuffer_ptr) {
    *zbuffer_ptr = pixel_z;                    // update Z-buffer
    *screen_ptr = remap[abuf_pixel | pixel];   // draw remapped pixel
}
z_shape_ptr++;
zbuffer_ptr++;
screen_ptr++;
```

### Status in Rust Engine

Our TMP parser (`src/assets/tmp_decode.rs`) reads Z-data with `FLAG_HAS_Z_DATA`
(`0x02`). The Z-data is used in the GPU depth pipeline via `zdepth_shader.wgsl`
which samples an R8 depth atlas for per-pixel terrain depth (cliff occlusion).

---

## 3. BUILDNGZ.SHA (Building Z-Shape Overlay)

### Source

A **separate SHP file** called `BUILDNGZ.SHA` stored in the MIX archives provides
per-pixel depth offsets for building sprites. Runtime-verified dimensions:
**396×477 pixels**, 169,488 non-zero pixels. This is a single large depth gradient
shared across all buildings (centered and bottom-aligned per building sprite).

### Loading: `FUN_0045e8f0` at `0x0045e8f0`

1. Loaded from MIX via `LoadFileFromMIX()` (`0x005b40b0`) into global `DAT_0089ddbc`
2. Gets frame 0 data via `FUN_0069e740(0)`, then for every non-zero pixel
   in `width × height` bytes (SHP header dimensions), the value is adjusted:
   ```c
   pixel -= 0x41;  // subtract 65 (byte arithmetic)
   ```
3. This converts raw bytes centered around 65 into signed depth offsets.
   The result is stored as a byte but read as `char` (signed) by the Z-test
   blitter, so the effective range is `[-64..+127]` for input bytes `[1..192]`.
   Input bytes above 192 wrap to negative values due to signed char overflow.

### Usage in Building Rendering

Buildings are rendered via:
```
BuildingClass_DrawBody (0x0043d290)
  → TechnoClass_DrawSHP (0x00705e00, RET 0x40 = 17 params)
    → CC_Draw_Shape (0x004aed70, RET 0x38 = 16 params)
      → SHP_StandardBlitter (0x004373b0) or SHP_ExtendedBlitter (0x00437a10)
```

The blitter choice is per-frame: `SHP_GetFrameCompressionFlag()` (`0x0069e900`) checks bit 1 of the SHP frame's
flag byte (offset 8 in the 24-byte frame header). Bit 1 set → extended blitter;
bit 1 clear → standard blitter (typical for building SHPs). Both blitters use
the gradient table, so gradient entry 2 applies in either path.

In `CC_Draw_Shape` (`FUN_004aed70`), when the Z-shape SHP pointer (`param_13`,
actually the BUILDNGZ.SHA pointer itself) is non-null:

```c
// Ghidra shows param_12 but real param is CC param_13 (BUILDNGZ.SHA pointer)
// due to 16-param function with Ghidra showing only 15.
// Assembly: TEST EBX, EBX at 0x004aef1c where EBX = [entry+0x2C] = CC param_13
if (buildngz_ptr != 0) {
    // ECX = BUILDNGZ.SHA SHP pointer (for SHP_Resolve inside)
    // param_14 = frame index (0 for buildings)
    piVar3 = (int *)SHP_GetFrameRect(&iStack_6c, frame_index);
    puStack_94 = operator_new(0x20);   // Z-shape draw context
    PixelBuffer_Init(uVar4, iStack_74 * iStack_70);  // prepare frame data
}
```

**Reachable, and used (corrected 2026-09-07).** The Z-shape surface built here
is passed only to `SHP_ExtendedBlitter` (`0x00437a10`, call at `0x004af1bf`);
`SHP_StandardBlitter` (`0x004373b0`) has no Z-shape parameter. Building SHPs are
format 3 on every frame (GACNST.SHP checked via `asset_info`), so the extended
path is the normal one. With building flags `0x6E00` the extended selector
returns slot `+0x158` -> vtable `0x007e53a0` -> leaf `0x004990e0`
(`ExtendedBlitter__RLEZero_RemapIntensity_ZReadWrite`):

```c
if (param_6 - *param_11 < (int)*param_7) {        // base_z - zshape[x] < zbuf[x]
    *dest = remap[...];
    *param_7 = (short)param_6 - (short)*param_11;  // write Z
}
param_7++; param_11++;                             // per pixel
```

`param_11` is the BUILDNGZ row pointer (or the 32-byte all-zero table at
`0x0089c568` when there is no Z-shape). The format-1 fallback leaf `0x004958d0`
(slot `+0xbc`) does the same strict-less test and Z write without a Z-shape.
The earlier text naming slot `0x130` / `0x00497cf0` described the cloak-mode
path (z-mode bits set), not normal building rendering. The `-0x41` signed remap
from the loader applies: the leaf subtracts the remapped byte.

### Building Draw Call Parameters

From `BuildingClass::DrawBody`:

```c
FUN_00705e00(
    shape,          // param_2: building SHP
    frame,          // param_3: frame index
    ...,
    z_height,       // param_8: base Z from screen position
    2,              // param_9: gradient type (entry 2 = buildings)
    1,              // param_10: consumed in TechnoClass, NOT forwarded to CC_Draw_Shape
    ...,
    DAT_0089ddbc,   // param_13: BUILDNGZ.SHA pointer (→ CC param_13, null check = Z-shape flag)
    0,              // param_14: Z-shape frame index (→ CC param_14 → FUN_0069e7e0)
    ...
);
```

Verified via full assembly trace: the 14-push sequence at `0x007065a3` maps
TechnoClass param_9 (=2) → CC_Draw_Shape **param_11** (gradient type), and
TechnoClass param_8 (z_height) → CC_Draw_Shape **param_10** (z_height).
TechnoClass param_13 (=DAT_0089ddbc) → CC_Draw_Shape param_13 (Z-shape SHP
pointer, tested as `EBX != 0`). TechnoClass param_14 (=0) → CC param_14
(frame index for `FUN_0069e7e0`). The gradient/z_height mapping was verified
from `FUN_004373b0` assembly: `ESP+0x78` (CC param_11) indexes the gradient
table, while `ESP+0x74` (CC param_10) is added to the Z formula as z_height.

### SpecialZOverlay (Gates)

Individual building types can have their own Z-overlay SHP files via the art.ini
key `SpecialZOverlay` (stored at type offset `+0x1510`). A companion key
`SpecialZOverlayZAdjust` (string at `0x0081a67c`) adjusts the Z-position.
Only gate buildings use this:

- `GAGATEZA` / `GAGATEZB` (Allied gates)
- `NAGATEZA` / `NAGATEZB` (Soviet gates)

### Status in Rust Engine

**Not implemented (DRIFT).** BUILDNGZ per-pixel depth was implemented once via
the zdepth atlas, then removed (section 10) on the strength of the old, wrong
"loaded but ignored" claim. No `buildngz` reference remains in `src/` as of
2026-09-07. Building bodies draw through the passthrough pipeline (no depth read
or write), so a unit is never partially hidden behind a building's tall part.
Reinstating it needs:

- frame 0 of BUILDNGZ.SHA with the `-0x41` signed remap (the removed version
  used raw unsigned bytes, see the difference noted above)
- placement at `(0xc6, 0x1be) + ZShapePointMove - CellToPixel(foundation)`
  relative to the building draw point, dropped when the foundation width >= 8
- a building-body pipeline that tests and writes depth, with the section 4
  gradient for base Z, and a depth-tested no-write pipeline for other objects

---

## 4. Per-Scanline Z-Gradient Table

### Table: `DAT_00817710` at `0x00817710`

Three 24-byte entries defining how Z changes per scanline row. Used by both SHP
blitters (`FUN_004373b0` standard, `FUN_00437a10` extended) as a Bresenham-style
accumulator:

| Entry | Fields (6 × i32) | Behavior |
|-------|-------------------|----------|
| 0 | `1, 1, 1, 1, -1, 1` | Standard: Z decreases by 1 every row (rate 1.0/row) |
| 1 | `2, 3, 2, 3, -1, 1` | Moderate: Z decreases by 2 every 3 rows (rate 0.67/row) |
| 2 | `1, 3, 1, 3, +1, 0` | Buildings: Z increases by 1 every 3 rows (rate 0.33/row) |

The gradient type flows from `BuildingClass::DrawBody` (param_9 = 2) through
`TechnoClass::DrawSHP` to `CC_Draw_Shape` **param_11** (not param_10), then
to `FUN_004373b0` param_9 (at `ESP+0x78` after stack setup). Verified from
assembly at `0x00437415`: `LEA EDX,[EAX*0x8 + 0x817710]` where EAX comes
from `ESP+0x78` = CC param_11. CC param_10 carries z_height (separate value). (Ghidra's `param_N` here is
stack slot N-1; section 1 names the same values "slot 7" (z) and "slot 8"
(gradient).)
**Buildings pass gradient type 2.**

Entry 2 (used by buildings): going DOWN the sprite (top-to-bottom rendering),
Z **increases** (further from camera). This gives the roof (top scanlines)
**lower Z** (closer) and the base (bottom scanlines) **higher Z** (further).
Entry 2 also triggers a different Z initialization path in the blitter
(field[5]=0) that includes sprite height in the base Z formula.

### Blitter Dispatch Mechanism

The per-scanline loop in `SHP_StandardBlitter` calls the **Blitter object's**
vtable+4 method per scanline. The Blitter is selected by `Blitter_selector`
based on draw flags. `Blitter_ClipAndSetup` (`0x007bc040`) does NOT touch the
Blitter — it only clips rectangles and locks source/dest surfaces, returning
pixel pointers. The Blitter object stays in the outer loop and controls the
per-scanline rendering behavior (opaque, translucent, Z-aware, etc.).

### Accumulator Logic

```c
z_gradient_entry = &DAT_00817710[gradient_type * 6];
// Fields read by the blitter (verified from assembly at 0x004374fa):
z_increment = z_gradient_entry[2];   // field[2] at offset +0x08: per-row accumulator step
z_threshold = z_gradient_entry[3];   // field[3] at offset +0x0C: step threshold
z_step_dir  = z_gradient_entry[4];   // field[4] at offset +0x10: +1 or -1

// Initial accumulator depends on rendering path:
// field[5]=1 (entries 0,1): z_accum = 0 (set at 0x004374f2)
// field[5]=0 (entry 2):    z_accum = field[3] - (spriteHeight % step)
//   Special case: if z_accum == field[3] → z_accum = 0, base_z += step_dir

// Per scanline row (assembly at 0x00437921):
z_accum += z_increment;              // [ESP+0x28] = field[2]
if (z_accum >= z_threshold) {        // [ESP+0x2c] = field[3]
    z_accum -= z_threshold;
    base_z += z_step_dir;            // field[4] at [gradient_ptr + 0x10]
}
```

Note: field[0]==field[2] and field[1]==field[3] for all three gradient entries,
so older docs referencing field[0]/field[1] produce correct results numerically.
The code actually reads offsets +0x08 and +0x0C (fields 2 and 3).

### Per-class Z policy and gradient (established 2026-09-07, read-only Ghidra pass)

Table bytes at `0x00817710` re-read: `{1,1,1,1,-1,1}`, `{2,3,2,3,-1,1}`,
`{1,3,1,3,+1,0}` — confirmed exactly.

`TechnoClass_DrawSHP` (`0x00705E00`) stack args: a7 z-adjust px, a8 gradient
index (`-1` suppresses `0x2000`), a9 Z-write byte, **a10 z-height** (fed to
`vtable+0x464` at `0x00706328`), a12 Z-shape ptr, a14/a15 Z-shape offset,
a16 clear-mask. VXL draws go through `TechnoClass__Draw` (`0x00706640`,
`vtable+0x444`), whose a8 is the z-height and a10 the clear-mask.

| Draw site | a8 gradient / a9 write / a12 z-shape | flags | slot | Z | status |
|---|---|---|---|---|---|
| Building body `0x0043D85F` (`0x0043D683` alt) | 2 / 1 / `g_BUILDNGZ_SHA` (null if `GetFoundationWidth() > 7`) | `0x6E00` | `+0xbc`/`+0x158` | read + write | body |
| Building bib `0x0043D9C9` (`Type+0x1518`, gated `this+0x534 != 0`) | **0** / 1 / 0 (a7 = `-1 - AdjustForZ`) | `0x6E00` | same | read + write, gradient entry 0 | body |
| Building damaged extras `0x0043D8E9` / `0x0043DA67` (`Type+0x14EC` / `+0x1504`) | 0 / 1 / 0 | `0x6E00` | same | read + write | body |
| Infantry body (`InfantryClass Draw_It 0x00518F90`, call `0x005195E2`; locomotor path `0x00519286`) | **2** / 0 / 0, a16 = 0 | `0x2E00` (`+2`/`+4` when `ModifyCloakDrawFlags` selects the translucent family) | `+0x98`/`+0x138` | read, no write | body (a8/a9, `+0x50c` = `0x0041c090`); leaf per Overview |
| Unit SHP, no turret (`0x0073CE0D`, call `0x0073CEAD`) | `vtable+0x2F0` result = locomotor `Z_Gradient` (`0x004DB0A0` → `loco+0x3C`, **default 2**) / 0 / 0; TooBig branch a8 = 0, a7 = -16 | `0x2E00` | `+0x98` | read, no write | body |
| Unit SHP with turret (`0x0073CC36` body, `0x0073CD08` turret) | 0 / 0 / 0 with a16 = `0x2800`/`0x2820` → flags `0x620`/`0x600` | into temp surface `DAT_00B1D13C` (`g_PrimarySurface` swapped at `0x0073C7C3`) | `+0x08`/`+0x0C` | no Z while compositing; the composite blit (`vtable+0x55C` = `0x0073B140`) uses `0x2800` → `+0x98` | body |
| Unit VXL body/turret (`UnitClass__DrawVoxelBody 0x0073B470` → `+0x510` → `0x00706640`) | a10 mask `0x2800` → Draw flags 0 (or 4/6) | temp surface | `+0x0C` | Z applied only by the composite `0x0073B140`: `0x2800` → `+0x98`, read, no write | body; leftover push count inferred |
| Aircraft (`AircraftClass__Draw_It 0x004144B0` → `+0x510`) | Draw a8 = `cell+0x10A + Rules+0x17DC + height term`, a9 = 0, mask 0 | `0x2800` (+state) → `VXL_CacheBlit` p5 / `Render` p8, `flags & ~0x10` | `+0x98` | read, no write, direct to screen | body |
| Building VXL turret (`FUN_0043DA80` → `+0x444`, `0x0043E2FF`) | a8 = `cell+0x10A`, a9 = remap bits, a10 = 0 | `0x2800` | `+0x98` | read, no write | decompile only |
| Anim (`AnimClass__DrawIt 0x00422CA0`, normal call `0x004236F7`) | CC 8th arg gradient = **2** (0 in the Flat branch `0x0042389E`; 2 in the tiled loop `0x00423827`); no Z-write arg; no z-shape | `this+0x190 OR trans(2/4/6) OR 0x800 (unless bit 0) OR 0x2000` = `0x2800` normally | `+0x98` | read, no write; `0x4000` only if `+0x190` carries it (source not traced) | flags body; `+0x190` inferred |
| Anim shadow | `0x2601` (`0x004233E9`), second pass `& ~6, OR 0x601` when `Type+0x372` | shadow family | `+0x10`/`+0x2C` | — | body |

VXL walkers: `VXL_CacheBlit 0x00707480` calls `Blitter_selector_extended(flags & ~0x10)`,
gradient = `this->+0x2F0()` (locomotor `Z_Gradient`, default 2), z-adjust =
`+0x2EC()`, and passes `0, 0, 0` in the Z-shape/offset positions.
The pending gradient push is an argument to the subsequent blitter, not to
Foot GetZAdjust (no-argument return at `0x004DB09E`). The unit final-composite
bridge split uses independently seeded full-width upper/bottom rectangles;
see the [2026-09-08 assembly correction](bridges/06-render-presentation-audio/UNIT_COMPOSITE_BRIDGE_SPLIT_73B140_GHIDRA_REPORT.md).
`TechnoClass__Render 0x00706ED0` calls `Blitter_selector(flags & ~0x10)` with the
same pair and no Z-shape argument at all.

`FootClass +0x510` (`0x004DAF10`) adds the locomotor `+0x2C` draw-point offset
and forwards W.arg8 → Draw a8 (z-height), W.arg10 → a9, W.arg9 → a10.

**DrawSHP a10 is brightness, not Z (established 2026-09-07, second pass).**
`Rules+0x17D8` is written by `[AudioVisual] ExtraInfantryLight` in
`RulesClass__ReadAudioVisual` (`0x006691E0`; key string `0x0083A23C`, PUSH
`0x0066B6E7`, store `0x0066B6FF`, ctor default 0 at `0x00665650`; stored as
value x 1000). Neighbours: `+0x17D4` `ExtraUnitLight`, `+0x17DC`
`ExtraAircraftLight`. Rules instance pointer is `0x008871E0`. At
`InfantryClass Draw_It 0x00518F90` the sum `cell+0x10A + height x lighting
Level + Rules+0x17D8` (built `0x00519444-0x00519457`) is pushed as a10
(`0x005195B4`) and `TechnoClass_DrawSHP` feeds it to `vtable+0x464`
(`0x0070631F` → `0x00706328`), then through the temporal/warp-in visual-phase
scalers, into `CC_Draw_Shape`'s intensity parameter. So `vtable+0x464`
(Building `0x00456F80`, Techno `0x0070D190`: `if (this+0xF0 & 2) v = (v > 1500)
? v - 500 : v + 500`) is a brightness adjuster, `cell+0x10A` is the cell light
scalar, and VERA's `src/map/lighting.rs::infantry_tint_at` citation of this site
stands. The first pass's "z-height" reading of a10 / `+0x464` is withdrawn.

The Z term of a `DrawSHP` call is therefore **a7** (z-adjust in pixels): bib
`-1 - AdjustForZ`, unit TooBig branch `-16`, VXL walkers `+0x2EC()`
(locomotor Z_Adjust), infantry the global `[0x00825500]`. This matches the
OpenTS `Techno_Draw_Object` argument order (`zadjust - 2, zgrad, brightness,
zshapefile, ...`).

**Building body a7 and the walker Z source (established from bodies, third
pass 2026-09-07).** `BuildingClass_DrawBody` loads `Type+0x1520` =
`NormalZAdjust` (INI read `0x004613A6`, store `0x004613B6`) into a local at
`0x0043D31C/0x0043D324`, overridden to −20 (`0x0043D6D3`) for the rubble image
`Type+0x14e4` and −40 (`0x0043D7D6`) for `Type+0x14fc`. At `0x0043D836` it
calls `vtable+0x1D0` (`0x005F5F30`, `Location.Z`), then
`Tactical__AdjustForZ 0x006D20E0`, and pushes `NormalZAdjust − AdjustForZ(Z)`
as a7 (`0x0043D847/0x0043D84D`). Site `0x0043D683` pushes the same a7; it
differs only in a10 (`cell+0x10A` without `Type+0x1548` = `ExtraLight`, INI
read `0x004613F1`). Inside DrawSHP a7 is forwarded untouched for buildings and
becomes CC slot 7 as `a7 − 2` (`0x0070641E`, `0x00706430`). In
`CC_Draw_Shape` slot 7 → `SHP_StandardBlitter` `[ESP+0x74]` (`0x004AF227`),
slot 8 → `[ESP+0x78]` gradient index, slot 9 (brightness) → `[ESP+0x7c]`
leaf-only, slot 10 (tint) → `[ESP+0x80]` leaf selection. The Z block
`0x004374BB–0x004375A7` reads only `[ESP+0x74]`, the gradient table, rect and
`g_ZBuffer`. The shadow call pushes literal 1000 in slot 9, confirming slot 9
is intensity. Retail YR sets `NormalZAdjust` only on GAFSDF (−10).

**`cell+0x10A/+0x10C/+0x10E` are lighting, not Z (established from
`0x00484680`).** `[+0x10A] = [+0x10E] = Scenario[+0x352c]*10 + cell[+0x108]`,
then `+= Level * int8(cell+0x11B) − Ground` (Level/Ground word pairs selected
by the ion-storm / dominator / nuke globals: `+0x355C/+0x3558`,
`+0x3590/+0x358C`, `+0x3574/+0x3570`, default `+0x3544/+0x3540`); `+0x10E`
uses `level + 4`; `[+0x10C] = ([+0x10A] * cell[+0x104]) >> 16`, all clamped
0..2000. DrawSHP itself loads `cell+0x10C` into the a10 brightness slot at
`0x00705F52` and `0x007060FF`. The tile blitter derives Z from the height level
alone (section 1). Section 8's `Cell_ComputeZAdjust` / `cellZAdjust` naming
is wrong; the addresses and formulas there are right, the interpretation is
lighting.

Foundation subtraction at the building body site (before `0x0043D85F`):
`x = W*0x100 - 0x100`, `y = H*0x100 - 0x100` (leptons of `(W-1, H-1)`) →
`TacticalClass__CellToPixel(&out, &{x, y})`; `off.x = (Type+0x1530 + 0xC6) - out.x`,
`off.y = (Type+0x1534 + 0x1BE) - out.y`. The `Type+0x1530/+0x1534` terms are
omitted for building types `0x12`/`0x13` (not identified).

Withdrawn: the earlier "Building `+0x50C` = `0x0070F020`" identity was
mis-based (`0x0070F020` sits at `vtable_BuildingClass+0x4AC` and is a
`XOR AL,AL; RET` stub; Building `+0x50c` holds `0x0045AAB0`). Buildings call
`0x00705e00` directly, so nothing depends on it. Not found: the Anim `+0x190`
flag source; `Extended_SHP_blitter` / `FUN_004AF2A0` parameter semantics (the
null Z-shape is positional inference); the `+0x2EC` locomotor slot number.

### Status in Rust Engine

Not implemented. Could be approximated in the vertex/fragment shader by computing
a per-row Z offset from the fragment's Y position within the sprite.

---

## 5. VXL (Voxel) Z-Buffer Pipeline

VXL units (tanks, ships, etc.) do **NOT** directly access `g_ZBuffer`. They use a
two-stage pipeline where Z-buffer interaction happens only at blit time.

### Stage 1: Software 3D Rasterization

Voxel models are rasterized into two private 256×256-byte intermediate buffers:
- `g_VXL_VisibilityMap` (`0x00b2ff78`) — per-pixel palette color indices
- `g_VXL_DepthMap` (`0x00b1d5e0`) — per-pixel depth (mirror mode only)

Depth ordering between voxel sections uses **painter's algorithm**: sections are
bubble-sorted by minimum Z depth (float at box record +0x24), then drawn
front-to-back (normal) or back-to-front (mirror mode).

Mirror rasterizers (`0x00757120`) have per-pixel depth test within the VXL's own
depth buffer:
```c
if ((ushort)g_VXL_DepthMap[pixel] < (depth >> 8)) {
    g_VXL_DepthMap[pixel] = (byte)(depth >> 8);
    g_VXL_VisibilityMap[pixel] = voxel_color;
}
```
Non-mirror rasterizers simply overwrite — the section sort ensures correctness.

### Stage 2: Blit to Screen (same SHP blitter)

The 256×256 buffer is RLE-encoded and blitted via the **same blitter pipeline**
as SHP sprites:
- Cached path (`0x00707480`): → `SHP_ExtendedBlitter` (`0x00437a10`)
- Uncached path (`0x00706ed0`): → `SHP_StandardBlitter` (`0x004373b0`)

Z-mode flags are computed via `vtable+0x68` in `TechnoClass__Draw` (`0x00706640`),
identical to SHP objects. The gradient table applies during the blit phase.

**Exception — VXL turrets** (`0x00706bd0`): hardcoded flags `0x2001` (no Z-test,
no Z-write), z_height = 1000, gradient type = 0.

### Key VXL Data

| Address | Name | Purpose |
|---------|------|---------|
| `0x00b2ff78` | g_VXL_VisibilityMap | 256×256 intermediate color buffer |
| `0x00b1d5e0` | g_VXL_DepthMap | 256×256 intermediate depth (mirror only) |
| `0x00846840` | Rasterizer function table | 16 entries, dispatched by 4-bit flags |

---

## 6. Animation Z-Buffer Interaction

`AnimClass::DrawIt` (`0x00422ca0`) computes Z-height from art.ini keys:

```c
// Non-flat animations (Flat=false, gradient type 2):
z_height = YDrawOffset + ZAdjust - AdjustForZ() - 2;

// Flat animations (Flat=true, gradient type 0):
z_height = YDrawOffset + ZAdjust - AdjustForZ() - 3;
```

- `YDrawOffset`: `AnimTypeClass+0x344` (from art.ini `YDrawOffset=`)
- `ZAdjust`: `AnimTypeClass+0x348` (from art.ini `ZAdjust=`), instance copy at
  `AnimClass+0x100` (can be overridden per-instance)

### Z-mode flags

Animations use `flags | 0x2000` (part of the 0x3000 blitter mask). The actual
Z-read/write comes from bits 1–2:

| Condition | Z-flags | Notes |
|-----------|---------|-------|
| Default (no 0x119 flag) | `0x00` | No Z interaction |
| 0x119 set, Scorch=false | `0x04` | Z-WRITE only |
| 0x119 set, Scorch=true | `0x06` | Z-READ + WRITE |
| Translucent (DetailLevel based) | `0x02`/`0x04`/`0x06` | Varies by translucency % |

Resolved 2026-09-07 (section 4 per-class table): a normal anim carries `0x2800` (`this+0x190 | trans | 0x800 | 0x2000`) with gradient entry 2 (0 in the Flat branch), reaching the read-only slot `+0x98`: per-pixel Z-test, no write. The "Z-flags" column above reads the cloak-mode bits, which the selectors do not use for Z; whether `+0x190` can carry `0x4000` is still untraced.

**Key differences from TechnoClass::DrawSHP:**
- Animations DO add `0x800` **unless bit 0 (shadow mode) is set**:
  `if ((flags & 1) == 0) { flags |= 0x800; }` — shadow draws skip 0x800
- Animations do **NOT** add `0x200` (no sprite centering)
- Non-flat animations use gradient type **2** (same as buildings: roof closer)
- Flat animations use gradient type **0** (standard: Z decreases 1/row)

---

## 7. Draw Order and Layer System

### Layer Rendering Order

Objects are sorted into 5 display layers. The layer array is at `DAT_008a0360`
(5 × `DynamicVectorClass<ObjectClass*>`, 24 bytes each, total 120 bytes).
`DAT_008a0364` is the buffer pointer of layer 0. Layer names verified from
string table at `0x0081da78`; name↔index conversion via `FUN_0048e050`/`FUN_0048e090`:

| Layer | Index | INI Name | Contents | Z-Buffer |
|-------|-------|----------|----------|----------|
| Underground | 0 | `Underground` | Tunnel locomotor effects | Varies |
| Surface | 1 | `Surface` | Below-ground-level objects (e.g. submerged subs) | Varies |
| Ground | 2 | `Ground` | Buildings, infantry, vehicles | Varies |
| Air | 3 | `Air` | Aircraft, projectiles | Varies |
| Top | 4 | `Top` | Parachutes, top-layer effects | Varies |

Z behaviour is set per draw by the flag word `TechnoClass::DrawSHP` builds
(section 1), not per layer. Building bodies request Z write (arg 9 = 1 ->
`0x4000`) and carry the BUILDNGZ Z-shape, so they read, test and write Z per
pixel. Other objects drawn with `0x2800` read and test per pixel without
writing. The `vtable+0x68` visual state only switches to the cloak/warp
translucent slot families.

After layer 2 (Ground), building turrets are drawn from `g_BuildingClass_Array`.

**Walls are NOT layer objects.** They are rendered as cell overlays via
`FUN_006d7560` → `FUN_00480350` → `FUN_00547cf0` (tile renderer).
`TacticalClass::Draw` (`FUN_006d3f50`) uses a two-phase rendering system
controlled by `param_4`: Phase 1 (`param_4 == 1`) for terrain, Phase 2
(`param_4 == 2`) for objects, Phase 3 (`param_4 == 3`) for both sequentially.

**Phase 1 — Terrain pass** (8 steps):

1. `FUN_006d2b60` — Z-buffer dirty rect clear
2. `FUN_006d3660` — Shroud edges and icons
3. `FUN_006d2de0` — Terrain shadows
4. `FUN_006d3470` — Base terrain cells
5. `FUN_006d3290` — Smudges and craters
6. `FUN_006d3ac0` — Building overlays
7. `FUN_006d3040` — Overlays (**walls, ore, other cell overlays**)
8. `FUN_006d3870` — Animations

**Phase 2 — Object pass:**

9. `FUN_006d8db0` — Layer object rendering (buildings, units, aircraft, etc.)

**Corrected 2026-09-07.** Building pixels are Z-tested per pixel and write Z
(leaves `0x004990e0` / `0x004958d0`, section 3); units and voxel cache blits are
Z-tested per pixel without writing (`0x00494b60` / `0x00497fd0`). Depth
ordering between walls, buildings and units therefore works as follows:

1. Wall pixels (Phase 1, step 7) write `wall_z` via `TMP_TileBlitter`
   (`pixel_z <= zbuffer`).
2. Building pixels (Phase 2) test `base_z - zshape < zbuffer` against
   terrain/wall Z and write their own Z where they win. A building can lose
   pixels to a nearer wall or cliff.
3. Unit, infantry (UNCHECKED flag word, see Overview) and aircraft pixels test
   against everything written so far and never write. A unit behind a
   building's tall part is hidden per pixel; a unit beside its base is drawn.
4. Units vs units: no Z interaction. Order is the layer Y-sort.

**Implication for our engine:** the sprite "passthrough" contract (no depth read
or write for SHP sprites, building bodies included) is DRIFT. Native parity
needs building depth writes shaped by BUILDNGZ and a depth-tested sprite
pipeline. The earlier "painter's algorithm, buildings always overwrite walls"
conclusion is withdrawn.

---

## 8. Bridge Rendering and Z-Buffer Interaction (verified)

Bridges achieve correct depth ordering through a combination of heightLevel
manipulation, pre-computed Z-adjust fields, and overlay rendering with the
standard TMP_TileBlitter Z pipeline. There is NO special bridge Z-buffer
handling — bridges use the same per-pixel Z-test as all other terrain tiles.

### Bridge Height Model

Bridge deck cells have `heightLevel` (CellClass+0x11B) set to `ground_height + 4`
during map load. This +4 is applied in two ways:

1. **ApplyBridgeTile** (`0x0057b440`): Sets `cell.heightLevel = sub_tile_height + reference_ground_height`
2. **Map init** (`FUN_0059e740`): Adds +4 to heightLevel for cells whose IsoTileTypeIndex matches the bridge tile set: `cell.heightLevel += 4`

The constant +4 means bridge decks are always 4 height levels (60 pixels) above
the ground beneath them. `GetEffectiveHeight` (`0x00487d50`) uses the same formula
for unit positioning: `heightLevel + ((cell_flags >> 7) & 1) * 4`.

### Three Pre-Computed Cell Lighting Fields (formerly "Z-Adjust")

> Correction 2026-09-07: `0x00484680` computes per-cell **lighting
> intensities** (1000 = normal; Scenario Ambient/Level/Ground, scaled by the
> 16.16 factor at `+0x104`), not depths. `TMP_TileBlitter` uses `+0x10C` as its
> palette brightness row and takes Z from the height level `+0x11B`; the
> bridge body's `+0x10E` push below lands in `CC_Draw_Shape` slot 9, the
> intensity slot. Formulas and offsets in this subsection are correct; read
> "Z-adjust" as "light". See section 4.

`0x00484680` computes three per-cell values:

| Field | Offset | Formula | Used By |
|-------|--------|---------|---------|
| cellZAdjust_top | +0x10A | `base + gradient * heightLevel - offset` | Buildings, normal overlays |
| cellZAdjust | +0x10C | `(+0x10A * intensityFactor) >> 16` | TMP_TileBlitter (tile Z), normal overlay CC_Draw_Shape |
| cellZAdjust_bottom | +0x10E | `base + gradient * (heightLevel + 4) - offset`, scaled | **Bridge overlay body** |

The +4 in the `+0x10E` formula is hardcoded — every cell pre-computes a
"bridge-level" Z-adjust regardless of whether it actually has a bridge.
The gradient and offset come from theater-specific tables at offsets from
the Rules class instance (e.g., `+0x3544` gradient, `+0x3540` offset for
the default theater).

### Bridge Tile Rendering (TMP_TileBlitter Path)

Bridge terrain tiles are drawn via:
```
Tactical_layer_terrain_shadows → iso_to_screen → CellOverlay_TileDraw → TMP_TileBlitter
```

In `CellOverlay_TileDraw` (`0x00480350`):
- **Y position**: `screenY + heightLevel * -15` (shifts tile UP by 15px per level)
- **Z-enable**: Always `1` (Z-test + Z-write active)
- **heightLevel**: Raw `CellClass+0x11B` value (bridge cells already have +4 baked in)
- **cellZAdjust**: `CellClass+0x10C` (pre-computed, includes heightLevel contribution)

TMP_TileBlitter Z formula:
```c
base_z = (DefaultZ + YOrigin - screenY - spriteHeight) - (spriteHeight * heightLevel) / 2;
// heightLevel = cell+0x11B (slot 10); cell+0x10C (slot 11) is the brightness row, not Z
// Per pixel: if (z_shape_value + base_z <= zbuffer[pixel]) { write; }
```

Since bridge cells have heightLevel = ground+4, their `cellZAdjust` (+0x10C) is
larger, producing a LOWER base_z (closer to camera). This makes bridge tile pixels
occlude ground-level pixels in the Z-buffer.

### Bridge Overlay Rendering (SHP Path via CC_Draw_Shape)

Bridge overlays (deck surface graphics with SHP overlays, not TMP tiles) are
drawn through `CellClass__DrawOverlay_Body` (`0x0047f6a0`) and
`CellClass__DrawOverlay_Shadow` (`0x0047f510`), called from `FUN_004d1890`
(base terrain renderer, case 0x14).

**Bridge body** (when `cell_flags & 0x80` is set):
```c
effective_height = heightLevel + ((cell_flags >> 7) & 1) * 4;  // +4 for bridges
CC_Draw_Shape(shp, frame, &pos, clip_rect,
    0x4E00,                    // flags: Z-buffered, centered, palette
    0,
    effective_height * -15 - 2, // Y-adjust
    0,                          // gradient type 0
    cell.light_bottom,          // +0x10E: CC slot 9 = brightness at level+4 (not Z; corrected 2026-09-07)
    0, 0, 0, 0, 0);
```

**Bridge shadow**:
```c
CC_Draw_Shape(shp, shadow_frame, &pos, clip_rect,
    0x4601,                    // flags: Z-buffered, centered, palette, darken
    0,
    heightLevel * -15 - 2,     // Y-adjust (NO +4, draws at ground level)
    0,                          // gradient type 0
    1000,                       // Z-height = default (no special depth)
    0, 0, 0, 0, 0);
```

Key distinction: body uses `+0x10E` (bridge-level Z) while shadow uses Z=1000
(default terrain Z). Shadow renders at ground height, body renders elevated.

### Bridge Railing/Pavement Overlay

`FUN_004802a0` → `FUN_00547230` draws bridge railings/pavement in the overlay
pass (Phase 1 step 7). Called for every cell in `FUN_006d7c00` (Tactical_layer_overlays
inner iterator). Renders using `CC_Draw_Shape` with flag `0x4601` and Y-adjust
`heightLevel * -15 + 0x3A` (58 decimal, approximately 2 tiles up from the base).

### Depth Ordering Summary

Units and objects sort correctly relative to bridges because:

1. **Units ON bridge**: Their cell has `heightLevel = ground + 4`, so their screen-Y
   position (used for draw-order sorting in Phase 2) is shifted up by 60 pixels.
   They sort in front of bridge surface tiles which were Z-written in Phase 1.

2. **Units UNDER bridge**: Their cell heightLevel is the raw ground height.
   Bridge tile Z-values (from heightLevel+4) are LOWER (closer to camera),
   so the bridge terrain pixels occlude the ground beneath. Units at ground
   height have screen-Y positions that sort them behind the bridge body.

3. **Bridge shadow**: Drawn at ground-level Z (=1000, default), so it appears
   on the ground surface beneath the bridge without interfering with bridge
   body depth.

4. **No special rendering pass**: Bridges are NOT separated into a special
   rendering pass. They go through the same Phase 1 terrain pipeline as all
   other tiles, using the standard TMP_TileBlitter Z-test.

### DAT_00B0782C (g_BridgeZ_Offset)

Value: **0** at static analysis time (runtime-initialized).
This is used exclusively by `ShipLocomotionClass` for ships navigating under
bridges. It adjusts the Z-coordinate of ship destinations when the destination
cell has flag `0x100` (bridge structural cell), ensuring ships render at the
correct depth beneath bridges. It is NOT used by the rendering pipeline directly.

Xrefs:
- `ShipLocomotionClass__InitBridgeZOffset` (`0x0069ebd0`) — WRITE (sets the value)
- `FUN_0069f450` (`0x0069f450`) — READ (applies to ship Z when cell has bridge)
- `ShipLocomotionClass__Process_Drive_Track` (`0x006a0000`) — READ (2 locations)

---

## 9. Key Addresses

### Functions

| Address | Ghidra Label | Purpose |
|---------|-------------|---------|
| `0x00547cf0` | TMP_TileBlitter | 60×29 diamond renderer with TMP Z-data |
| `0x004aed70` | CC_Draw_Shape | Main SHP draw entry point (50+ call sites) |
| `0x004373b0` | SHP_StandardBlitter | Per-scanline loop with Z-gradient |
| `0x00437a10` | SHP_ExtendedBlitter | Shadow/Z-overlay variant |
| `0x0045e8f0` | LoadBuildingZShape | Loads BUILDNGZ.SHA, applies -65 remap |
| `0x00490b90` | Blitter_selector | Picks blitter from draw flags (164 lines) |
| `0x00490e50` | Blitter_selector_extended | Same for extended blitter path |
| `0x006d8db0` | Tactical_ObjectRenderingLoop | Iterates 5 display layers, calls Draw_It |
| `0x007bcf50` | ZBuffer_RectClear | Thin wrapper, calls virtual fill method |
| `0x007bcfb0` | ZBuffer_Clear | Row-by-row fill with 0xFFFF (the actual clear) |
| `0x007bd130` | ZBuffer_GetScanlinePtr | Gets pointer to Z-buffer row |
| `0x0043d290` | BuildingClass_DrawBody | Building body draw dispatch |
| `0x00705e00` | TechnoClass_DrawSHP | Shape draw with Z params |
| `0x00480350` | CellOverlay_TileDraw | Wall/overlay draw, calls tile renderer |
| `0x007bc970` | ZBuffer_Constructor | Creates 0x30-byte Z-buffer surface object |
| `0x006bb9a0` | WinMain | Creates Z-buffer, sets DefaultZ = 0x8000 |
| `0x00497100` | Blitter_ZClip_Plain16_WritesZ | Per-pixel Z-shape blitter: `base_z - z_shape` |
| `0x00495bc0` | Blitter_ZBuf_Intensity25pct_WritesZ | Per-pixel Z R+W with 25% blend |
| `0x004990e0` | ExtendedBlitter__RLEZero_RemapIntensity_ZReadWrite | Live building body leaf (flags 0x6E00, slot +0x158): per-pixel `base_z - zshape < zbuf`, writes Z (2026-09-07) |
| `0x004958d0` | (undefined in Ghidra) | Building body leaf, format-1 frames (slot +0xbc): per-pixel Z read, `<` test, write (2026-09-07) |
| `0x00494b60` | (standard, slot +0x98) | Normal-object leaf (flags 0x2800): per-pixel Z read + test, no write (2026-09-07) |
| `0x00497fd0` | (extended, slot +0x138) | Normal-object / VXL cache leaf (flags 0x2800): per-pixel Z read + test with Z-shape byte, no write (2026-09-07) |
| `0x0048ebf0` | Blitter_Init_All | Creates all blitter objects with vtables |
| `0x00456f80` | BuildingClass_AdjustZHeight | vtable+0x464: ±500 (threshold 1500) |
| `0x006d3f50` | TacticalClass_Draw | Two-phase renderer (1=terrain, 2=objects, 3=both) |
| `0x006d2b60` | Tactical_ZBufferDirtyClear | Phase 1 step 1: dirty rect Z-clear |
| `0x006d2de0` | Tactical_TerrainShadows | Phase 1 step 3: terrain shadows |
| `0x006d3470` | Tactical_BaseTerrainCells | Phase 1 step 4: base terrain cells |
| `0x006d3040` | Tactical_Overlays | Phase 1 step 7: walls, ore, cell overlays |
| `0x0069e900` | SHP_GetFrameCompressionFlag | Bit 1 of frame byte 8: std vs extended |
| `0x0069e7e0` | SHP_GetFrameRect | Returns frame x/y/w/h for given frame index |
| `0x0069e740` | SHP_GetFrameData | Returns decompressed frame pixel data ptr |
| `0x0069e580` | SHP_Resolve | Resolve SHP reference, ensure loaded |
| `0x007bc040` | Blitter_ClipAndSetup | Clip rects + configure source surface |
| `0x004114b0` | CircBuf_GetScanlinePtr | Generic circular buffer row pointer |
| `0x0043ad00` | PixelBuffer_Init | Init buffer descriptor, optional alloc |
| `0x0043ae50` | PixelBuffer_Free | Free if owned, zero descriptor |
| `0x005b40b0` | LoadFileFromMIX | Generic file loader from MIX archives |
| `0x00706640` | TechnoClass__Draw | Sets Z-mode flags for VXL (same as SHP) |
| `0x00706ed0` | TechnoClass__Render | VXL uncached → SHP_StandardBlitter |
| `0x00707480` | VXL_CacheBlit | VXL cached → SHP_ExtendedBlitter |
| `0x00706bd0` | VXL turret draw | Hardcoded `0x2001` (no Z-test/write) |
| `0x00754510` | VXL_Sort_Rasterize | Bubble-sort sections by Z, then rasterize |
| `0x00757120` | VXL_Rasterizer_Mirror | Per-pixel depth test in g_VXL_DepthMap |
| `0x00422ca0` | AnimClass::DrawIt | Anim draw with ZAdjust + gradient type 0 or 2 |
| `0x00484680` | Cell lighting init | Computes per-cell light (+0x10A/+0x10C/+0x10E) from Scenario Ambient/Level/Ground and heightLevel (not Z) |

### Global Data

| Address | Type | Purpose |
|---------|------|---------|
| `DAT_00887644` | `ZBuffer*` | 16-bit per-pixel depth surface |
| `DAT_0087e8a4` | `ABuffer*` | 16-bit intensity/remap overlay surface |
| `DAT_0089ddbc` | `SHP*` | Loaded BUILDNGZ.SHA data |
| `DAT_00817710` | `int[3][6]` | Z-gradient parameter table |
| `DAT_008a0360` | `DynamicVectorClass[5]` | Display layer array (24 bytes/entry, base) |
| `DAT_008a0364` | `ObjectClass**` | Buffer pointer of layer 0 (inside first entry) |
| `DAT_0081da78` | `char*[5]` | Layer name string table (Underground/Surface/Ground/Air/Top) |
| `DAT_0081dc24` | `uint` | Blitter flag mask = `0x3000` (tests bits 12-13) |
| `DAT_00abb120` | `byte[29×60]` | Isometric diamond pixel offset table (runtime-initialized) |
| `DAT_00aa074c` | `byte[29×60]` | Isometric diamond screen offset table (runtime-initialized) |
| `DAT_00aa154c` | `byte[]` | Isometric diamond scanline width table (stride `0x6CC` = 60×29, runtime-initialized) |
| `0x00b2ff78` | `byte[256×256]` | g_VXL_VisibilityMap (intermediate voxel color buffer) |
| `0x00b1d5e0` | `byte[256×256]` | g_VXL_DepthMap (intermediate voxel depth, mirror only) |
| `0x00846840` | `func_ptr[16]` | VXL rasterizer function table (4-bit dispatch) |

---

## 10. Historical implementation status (superseded 2026-09-08)

### How Our Engine Renders Depth

Our engine uses painter's algorithm (draw order) for sprite-vs-sprite layering.
**This does not match gamemd.exe** (correction 2026-09-07, see Overview): native
building bodies write per-pixel Z and every other object reads it. The GPU depth
buffer currently handles terrain occlusion only:

- **Terrain tiles** — `zdepth_shader.wgsl` samples per-pixel TMP Z-data from an R8
  depth atlas, writes `@builtin(frag_depth)`. Depth write ON. Matches the original's
  per-pixel terrain Z-write via TMP_TileBlitter.
- **Wall overlays** — `zdepth_shader.wgsl` with depth write ON (`LessEqual`). Walls
  write depth so sprites behind them are occluded. Matches gamemd.exe where walls
  write Z via TMP_TileBlitter in terrain pass step 7.
- **Non-wall overlays (ore, terrain objects)** — `zdepth_shader.wgsl` with depth
  write OFF. Read terrain depth for cliff occlusion only.
- **All sprites (buildings, units, infantry, damage fires)** — as of 2026-09-07
  the SHP sprite pipeline is `overlay_passthrough_pipeline` (`src/render/batch.rs`,
  compare `Always`, no depth write); voxel sprites test `LessEqual` without
  writing. Sprite-vs-sprite ordering is pure draw order. **DRIFT**: native
  building bodies write Z (BUILDNGZ-shaped) and other objects Z-test per pixel.
- **Unified Y-sorted object pass** — VXL units and SHP entities are merged into a
  single Y-sorted draw pass via multi-way merge, matching gamemd.exe Layer 2 only
  if each class uses its native virtual `GetYSort` key. Base `ObjectClass::GetYSort`
  is `X + Y`; `AnimClass` adds `YSortAdjust`; `BuildingClass` adds `+32` for
  `BuildingType+0x16C5` and `-16` for `BuildingType+0x16B7`. No class/id
  tiebreakers were found in the 2026-05-28 YSort override census.
- **Building turrets** — drawn in a separate pass after all layer-2 objects, matching
  gamemd.exe's turret pass after the ground layer.
- **Damage fires** — Y-sorted with buildings in the object pass (not a separate
  terrain pass), matching gamemd.exe where FIRE anims are Layer 2 objects.
- **Cliff occlusion** — terrain is drawn once through the zdepth pipeline with
  depth write ON; there is no separate cliff redraw pass any more
  (`src/app/presentation/render/draw_passes.rs`, checked 2026-09-07). Cliff
  pixels occlude voxel sprites (which depth-test) but not SHP sprites (which do
  not).
- **Depth function** — single `compute_sprite_depth()` for all sprites. No per-type
  bias constants. Depth only determines terrain occlusion, not sprite ordering.

Source note: the per-class `GetYSort` details above come from
[docs/research/PERCLASS_VTABLE_B8_YSORT_OVERRIDE_CENSUS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PERCLASS_VTABLE_B8_YSORT_OVERRIDE_CENSUS_GHIDRA_REPORT.md).

### BUILDNGZ.SHA — Removed (removal was a mistake)

BUILDNGZ.SHA was removed from the pipeline on the strength of the old claim that
the original never used it. That claim was wrong (section 3): building bodies
consume it per pixel through `0x004990e0`. The earlier implementation also used
raw unsigned bytes instead of the `-0x41` signed remap, which is a plausible
cause of the wall edge artifacts it showed. Reinstating it, with the signed
remap and `ZShapePointMove` placement, is required for parity.

### Draw Pass Order

```
1. Terrain          — zdepth pipeline (depth write ON)
2. Wall overlays    — passthrough (no depth test, painted over terrain)
3. Non-wall overlays — passthrough pipeline (depth compare Always, no test)
4. UNIFIED MERGE    — VXL + SHP + damage fires, Y-sorted, interleaved draw calls
5. Building turrets — zdepth overlay no-write (after all layer-2 objects)
6. (removed)        — the cliff redraw pass no longer exists (2026-09-07)
7. Debug/fog/UI     — overlay pipeline
```

### Comparison with Original Engine

| Aspect | Original (gamemd.exe) | Our engine | Match? |
|--------|----------------------|------------|--------|
| Terrain per-pixel Z | TMP tile blitter, Z R+W | zdepth shader, frag_depth | Match |
| Wall Z | TMP blitter Z R+W; sprites Z-test against it | passthrough (no depth test) | **DRIFT** (sprites should lose pixels to nearer walls) |
| Non-wall overlay Z | TMP blitter skips Z (flag 0x02 clear) | passthrough pipeline (Always compare) | Match |
| SHP sprite Z | Buildings read+write Z (BUILDNGZ-shaped); other objects read-only | passthrough (no depth test, painter's alg) | **DRIFT** |
| Object sort | Layer 2 sorted by virtual `GetYSort`: base `X+Y`, plus AnimClass and BuildingClass overrides | depth-sorted (iso_row based), multi-way merge | Partial: missing proven per-class `GetYSort` deltas |
| Building turrets | Separate pass after layer 2 | Separate pass after merged objects | Match |
| Damage fires | AnimClass in Layer 2, Y-sorted | In SHP pass, Y-sorted with buildings | Match |
| Building sort key | `BuildingClass::GetYSort = ObjectClass::GetYSort` plus conditional `+32` / `-16` type flags | screen_y from foundation | Partial: missing conditional deltas |
| BUILDNGZ per-pixel | Consumed per pixel by building body blitter | Removed | **DRIFT** |
| Cliff occlusion | Buildings and units Z-test against tile Z | zdepth terrain write; only voxel sprites test | Partial |
| Per-scanline gradient | 3-entry Bresenham table | Not implemented | Missing (cosmetic) |
| Shadows | Drawn before sprite per object | Not implemented | Missing |
| Flat anims | Terrain pass step 8 (below objects) | Mixed into SHP pass | Missing |
| Smudges | Terrain pass step 5 | Not implemented | Missing |

### Intentional Improvements Over Original
- (withdrawn 2026-09-07) The "cliff occlusion the original lacks" entry rested on
  the wrong no-Z-test claim; the original does Z-test buildings and units
  against tile Z.
