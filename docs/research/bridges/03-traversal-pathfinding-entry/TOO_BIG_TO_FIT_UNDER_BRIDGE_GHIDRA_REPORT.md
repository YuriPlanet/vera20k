# TooBigToFitUnderBridge — Ghidra Research Report

> **Rendering claims superseded 2026-09-08:** use the live assembly and
> caller reconstruction in [Unit composite bridge split](../06-render-presentation-audio/UNIT_COMPOSITE_BRIDGE_SPLIT_73B140_GHIDRA_REPORT.md).
> The historical text below contains consequential errors: Unit vtable base
> is `0x7F5C70`, final composite slot is `+0x55C`, the bottom strip is full
> width by 16 (not 16 by 16), and Foot GetZAdjust takes no gradient/mode
> argument. This path composites ordinary voxel bodies too; it is not a
> shadow split. `+0x8C` is OnBridge, `0x703E70` returns 0..=2, and the
> alternate building gate is WeaponsFactory. Do not use the historical
> rendering pseudocode or its broad confidence statement as an implementation
> contract. Movement claims were not re-audited in that bounded follow-up.

**Address(es):**
- `0x00845DC8` — the INI key string `"TooBigToFitUnderBridge"`
- `0x00747749` / `0x0074777A` — read/write at `UnitTypeClass+0xE16` inside `UnitTypeClass::ReadINI` (0x0074774E xref)
- `0x00747143` — constructor zero-init (`MOV byte ptr [ESI+0xE16], BL` with BL=0)
- `0x0073B1B0` — reader inside `FUN_0073B140` (UnitClass virtual method, vtable slot 0x1CC of vtable at 0x007F6000)
- `0x0073CE0D` — reader inside `FUN_0073C5F0` (UnitClass virtual method, vtable slot 0x1C8 of vtable at 0x007F6000)
- `0x00703B10` — `TechnoClass::IsOnBridge_ForFiring` (helper)
- `0x00703E70` — bridge-piece neighbor counter (helper)
- `0x00703CC0` — adjacent low-bridge ramp counter (helper, alternate)
- `0x00487950` — `CellClass::IsShrouded` wrapper (helper)

**Confidence:** HIGH (every claim is from live decompilation of the addresses above)

**Active in YR:** **Conditional — but broader than initially thought.** The flag has
NO movement effect at all. Its only effect is a sprite-Z fudge during rendering. The
shadow/sprite split-blit applies to ANY SHP-rendered unit on a bridge edge; the
main-draw Z bias applies only to units with `Turret=no` on a bridge edge. Per
`ini/rulesmd.ini`, the `Turret=no` set in YR includes most capital ships and several
big land units (see Section 4.2).

> **2026-05-13 Update:** A follow-up `/re-investigate` resolved the previously-uncertain
> `TypeClass+0xCA1` dispatch byte. It is the **`Turret` bool**, not "SHP-vs-voxel".
> See [TECHNOTYPECLASS_TURRET_FIELD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOTYPECLASS_TURRET_FIELD_GHIDRA_REPORT.md).
> The corrections are inlined below; the original framing is struck through where
> wrong.

---

## 1. TL;DR — what the flag actually does in gamemd.exe

`TooBigToFitUnderBridge=yes` is a **rendering-only** flag in gamemd.exe.
- It is **never read** by `UnitClass::Can_Enter_Cell`, locomotor passability, A* costing,
  cell-occupancy logic, `Mission_Move`, or any path/movement helper.
- It is read in **exactly two** virtual methods of `UnitClass`, both in the **drawing
  pipeline**:
  1. `UnitClass::Draw_Sprite_With_BridgeFudge` (formerly `FUN_0073B140`, renamed
     2026-05-13) — SHP sprite/shadow blitter (vtable slot 0x1CC). When the flag is set
     AND the unit is on a HighBridge cell AND no neighboring cell has bridge-piece
     tiles AND the sprite is taller than 16 pixels, the sprite is drawn in **two
     slices** (upper full width × `h - 16`, then a `16 × 16` lower strip) each with
     different Z parameters. **Applies to all unit types regardless of Turret.**
  2. `UnitClass::Draw_Body_And_Turret` (formerly `FUN_0073C5F0`, renamed 2026-05-13) —
     the main UnitClass draw method (vtable slot 0x1C8). The check sits inside the
     **no-turret** branch (taken when `TypeClass->Turret == 0`, i.e. `+0xCA1 == 0`).
     When triggered, the draw call to vtable+0x50C is issued with a Z bias of **-16**
     (0xFFFFFFF0 in the stack arg) and the normal vtable+0x2F0 pre-call is skipped.
     **Applies only to units with `Turret=no`.**
- The trigger predicate in both readers is essentially the same:
  ```
  Type->TooBigToFitUnderBridge != 0
    && IsOnBridge_ForFiring(unit) != 0
    && bridge_piece_neighbor_count(unit) == 0
  ```
- The flag's effect is purely cosmetic: it nudges Z so that the sprite renders
  consistently when a big SHP unit is positioned at the **edge** of a bridge cell.

**The Rust implementation in `src/sim/movement/movement_path.rs` treating this as an
A\* hard-block for non-water-mover units is a parity bug** — see Section 6.

---

## 2. Class layout / key offsets

### UnitTypeClass

| Offset | Type | INI key | Default | Notes |
|---|---|---|---|---|
| `+0xE16` | byte (bool) | `TooBigToFitUnderBridge=` | `0` (false) | Constructor zero-inits at `0x00747143`. Parsed via `CCINIClass::ReadBool` in `UnitTypeClass::ReadINI` (the field is its own default, so retains constructor value when key absent). |

### TechnoClass (instance)

| Offset | Field | Notes |
|---|---|---|
| `+0x6C4` | `Type` pointer | Loaded as `[ESI+0x6C4]` before reading `+0xE16` |
| `+0x8C` | byte | Tested by `IsOnBridge_ForFiring` (decompiles as `param_1[0x23] == '\0'`). Likely "InLimbo" or equivalent — function early-exits if non-zero. |

### CellClass

| Offset | Field | Notes |
|---|---|---|
| `+0x38` | int (tile index) | Used by neighbor counter to detect bridge-piece tile IDs |
| `+0x140` | uint (flags) | Bit `0x100` = HighBridge; bit `0x400` = LowBridge/ramp; bit `0x800` = directional/corner discriminator |
| `+0x11B` | byte | HighBridge deck level (z-level the bridge sits at). Used by `Can_Enter_Cell` for the height comparison (`|param_4 - cell+0x11B| < 2` → treat as ground). |

### Vtable at 0x007F6000 (UnitClass)

| Slot offset | Function | Role |
|---|---|---|
| `+0x1C8` | `FUN_0073C5F0` | Main draw (SHP or voxel-matrix, dispatched on `Type+0xCA1`) |
| `+0x1CC` | `FUN_0073B140` | SHP blit helper (sprite + shadow) |

Other vtable slots invoked by these two: `+0x48` GetCoords, `+0x68` draw-mode-getter,
`+0xC4` IsHumanOwned, `+0x1B0` passability (called from `Can_Enter_Cell`), `+0x2EC`
Z-blit-helper, `+0x2F0` Z-pre-helper, `+0x50C` actual draw, `+0x510` voxel main draw.

### The `TypeClass+0xCA1` dispatch byte — RESOLVED (2026-05-13)

`TypeClass+0xCA1` is the **`Turret`** bool (INI key `Turret=`, default false).
Fully verified in [TECHNOTYPECLASS_TURRET_FIELD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOTYPECLASS_TURRET_FIELD_GHIDRA_REPORT.md).
Decisive proof: `BuildingClass::HasTurret` at `0x004527D0` literally reads
`[Type+0xCA1]` as its turret check, and `TechnoTypeClass::ReadINI` writes
`AL = ReadBool("Turret")` into this offset at `0x007133C2`.

- Read at `0x0073C725` (`MOV AL, [ECX+0xCA1]; TEST AL, AL; JZ 0x73CE0D`) — JZ
  (Turret=no) takes the simple body draw where TooBig is checked; fall-through
  (Turret=yes) takes the two-pass body+turret draw where TooBig is NOT checked.
- **What this means for the units that actually hit each branch:**
  - The main-draw Z-bias applies to ANY unit type with `Turret=no` AND
    `TooBigToFitUnderBridge=yes`. In YR `rulesmd.ini` this includes most capital
    ships (DEST, CDEST, CARRIER, DRED, SUB, BSUB, SQD, CRUISE, TUG) and several
    big land units (MGTK, V3, HOWI, TNKD, SAPC, VLAD, LCRF, YHVR).
  - The reason most ships are `Turret=no` is a content quirk documented in the
    INI itself: `;can't have a turrett and a NoSpawnAlt (both go in AuxVoxel)`.
- Turreted units (HTNK, MTNK, LTNK, UTNK, TTNK, YTNK, ROBO, TELE, MIND, DISK,
  XCOMET, FV) that ALSO carry `TooBigToFitUnderBridge=yes` skip this main-draw
  path — but their **shadow** still gets the split-blit treatment via the
  sibling vtable slot at `0x0073B140`.

**Original (incorrect) framing this report previously held:** that `+0xCA1` was a
"SHP-vs-voxel" discriminator and so ships (all voxel) would universally skip the
flag. That framing was wrong. Corrected here and in the implications section
(Section 6).

---

## 3. Core logic — pseudocode

### 3.1 Helper: `TechnoClass::IsOnBridge_ForFiring` (`0x00703B10`)

```
bool IsOnBridge_ForFiring(this):
    coords = this->vtable[0x1B8]()    // get cell coords
    centerCell = Map.Get_CellClass(coords)
    if centerCell == NULL: return 0
    if this->byte[0x8C] != 0: return 0    // early-out (limbo?)

    // Sample 4 specific neighbor offsets stored in globals 0x89F698, 0x89F68A,
    // 0x89F690, 0x89F6A0 (each is a (dx,dy) packed in 32-bit).
    nNE = Map.Get_CellClass(coords + DAT_0089F698)   // also requires bit 0x800 SET
    nN  = Map.Get_CellClass(coords + DAT_0089F68A)
    nSW = Map.Get_CellClass(coords + DAT_0089F690)   // requires bit 0x800 CLEAR
    nSE = Map.Get_CellClass(coords + DAT_0089F6A0)   // requires bit 0x800 CLEAR

    return 1 if any of:
        - centerCell.Flags & 0x100  (HighBridge on this cell)
        - nNE != NULL && nNE.Flags & 0x100 && nNE.Flags & 0x800
        - nSE != NULL && nSE.Flags & 0x100 && !(nSE.Flags & 0x800)
        - nSW != NULL && nSW.Flags & 0x100 && !(nSW.Flags & 0x800)
        - nN  != NULL && nN.Flags  & 0x100 && nN.Flags  & 0x800
    else 0
```

**Tiny detail:** the 0x800 bit is checked **inverted** for two of the four
neighbor directions. The function asymmetrically treats NE and N (require 0x800=1)
vs SE and SW (require 0x800=0). This is the "bridge orientation" disambiguator.

### 3.2 Helper: `FUN_00703E70` — bridge-piece neighbor counter

```
int CountBridgePieceNeighbors(this):
    coords = this->vtable[0x1B8]()
    if Map.Get_CellClass(coords) == NULL: return 0

    onBridge = IsOnBridge_ForFiring(this)
    onRamp   = FUN_00703CC0(this)             // identical to IsOnBridge but checks bit 0x400
    if !onBridge && !onRamp: return 0

    count = 0
    for offset in [DAT_0089F698, DAT_0089F690, DAT_0089F694]:    // only 3 neighbors
        c = Map.Get_CellClass(coords + offset)
        if c != NULL:
            tile = c->Tile_Index_at_+0x38
            if tile != 0xFFFF && tile != 0xFF:
                idx = tile - DAT_00AA0E28 + 1   // DAT_00AA0E28 is the bridge-tile base ID
                if 6 < idx && idx < 0x11:        // strict both sides — tile IDs 7..16
                    count++                       // 3rd loop is `count = count + 1` (same effect)
    return count    // 0..3
```

**Tiny details that matter for parity:**
- The valid tile-id range is **strictly between 6 and 0x11** (i.e. `idx ∈ {7,8,9,10,11,12,13,14,15,16}`, 10 distinct IDs). Both bounds are exclusive in the binary's `6 < idx && idx < 0x11` check.
- Only **3** neighbor offsets are tested (vs the 4 in `IsOnBridge_ForFiring`). The
  offset `0x89F694` is unique to this function and is NOT one of the four used
  for the bridge bit test.
- `DAT_00AA0E28` is the runtime-discovered base tile ID for bridge pieces in the
  current theater — the range check is **theater-relative**, not absolute. Parity
  consumers need to compute the same base ID at map-load.

### 3.3 Helper: `FUN_00703CC0` — low-bridge ramp probe

Same structure as `IsOnBridge_ForFiring` but tests cell flag **`0x400`** instead of
`0x100`. The 0x800 inversion pattern across the four directions is identical.

### 3.4 Reader A — `FUN_0073C5F0` tail at `0x0073CE0D` (vtable slot 0x1C8)

This is `UnitClass::DrawIt` (or near-equivalent). Reduced pseudocode of just the
relevant tail:

```
function FUN_0073C5F0(this):
    ... compute frame index uVar15 from Type->FiringFrames, WalkFrames, etc. ...
    Type = this->Type     // [this + 0x6C4], stored as iVar12

    if Type->byte[0xCA1] != 0:        // voxel-matrix draw path
        ... CC_Draw_Shape (shadow) ...
        ... Matrix3D ops (Matrix3x4_Copy, RotateZ, shear, rotate_y_axis) ...
        ... vtable[0x510] (voxel body draw, twice) ...
        return    // TooBigToFitUnderBridge IS NOT CHECKED ON THIS PATH

    // SHP draw path — Type+0xCA1 == 0
    if Type->TooBigToFitUnderBridge != 0:          // [Type + 0xE16]
        if IsOnBridge_ForFiring(this) != 0:
            if CountBridgePieceNeighbors(this) == 0:
                // SPECIAL DRAW: skip the vtable+0x2F0 pre-call and call
                // vtable+0x50C with stack args set such that the Z-bias slot
                // (uStack_150) holds 0xFFFFFFF0 (= -16) instead of 0.
                this->vtable[0x50C](
                    shape         = pfStack_160,
                    frame         = pfStack_158,
                    ...,
                    z_bias        = -16,           // 0xFFFFFFF0
                    palette_offset= 0x100,
                    ...
                )
                return

    // NORMAL SHP DRAW
    pre = this->vtable[0x2F0]()                    // computes Z adjust
    this->vtable[0x50C](
        shape, frame, ..., z_bias = 0, palette_offset = 0x100, pre, ...
    )
```

**Tiny details:**
- The Z bias on the special path is **exactly -16** (0xFFFFFFF0). Not -8, not -32.
- The "skip vtable+0x2F0" detail matters: vtable+0x2F0 is the per-frame Z-pre-helper
  whose return is normally fed into vtable+0x50C as the `pre` slot. On the special
  path this slot is **zero** (uninitialized stack — `uStack_14C` is left as 0 from
  the initial layout). A clean Rust implementation must produce the same draw
  ordering as "vtable+0x50C with pre=0, z_bias=-16, palette_offset=0x100" — not
  "skip the pre-call but still feed its result".
- `palette_offset = 0x100` is the same on both paths — only `z_bias` and `pre` differ.

### 3.5 Reader B — `FUN_0073B140` (vtable slot 0x1CC)

This is an SHP sprite/shadow blitter. Reduced pseudocode:

```
function FUN_0073B140(this):
    coords = this->vtable[0x48](&local_30)        // returns CoordStruct*
    cellPos = (coords.x / 0x100, coords.y / 0x100)
    cell = Map.Get_CellClass(cellPos)
    isShrouded = CellClass::IsShrouded(cell)      // FUN_00487950
    // The TEST AL,AL after IsShrouded zeroes [ESP+0x6C] only when AL == 0
    // — the slot is consumed later by the blit setup but has limited side-effect.

    bVar1 = false
    if Type->TooBigToFitUnderBridge != 0:         // [this->Type + 0xE16]
        cond_A = (IsOnBridge_ForFiring(this) != 0 && CountBridgePieceNeighbors(this) == 0)
        cond_B = false
        if cond_A == false:
            obj = FUN_0065AD40(this, 0)           // returns this->[0xE4][0]
            if this->[0x169] != 0                  // [this + 0x5A4] — "destination"-ish
               && obj != NULL
               && obj->vtable[0x2C]() == 6        // RTTI_ID == 6 (BuildingClass family)
               && obj->[0x148]->byte[0x16BD] != 0 // building TypeClass flag at +0x16BD
            :
                cond_B = true
        if cond_A || cond_B:
            bVar1 = true

    // Compute blit flags from a draw-mode switch on vtable+0x68(0,0)
    flags = 0x2800
    mode = this->vtable[0x68](0, 0)
    switch mode:
        case 1: flags = 0x2802
        case 2,3: flags = 0x2804
        case 4: flags = 0x280A
                if this->[0x89] != 0: flags = 0x280C
                unaff_ESI = FUN_0070BE50()
    if this->vtable[0x1D4]() || this->vtable[0x1D8]():
        flags |= 4
    if HouseClass::IsHumanPlayer() && this->vtable[0xC4]():
        flags = this->vtable[0x43C](flags)
    blitter = Blitter_selector(flags)

    rect_x = DAT_00B1CFC0 - 0x80 + iStack_4       // 0x80 = 128 px sprite half-width
    rect_y = retaddr - 0x80 + DAT_00B1CFC4         // 0x80 sprite half-height
    rect_w = DAT_00B1CFC8
    rect_h = DAT_00B1CFCC

    if bVar1 && rect_h > 0x10:                    // sprite > 16 px tall
        // SPLIT BLIT — upper slice (full width × h-16) + lower strip (16 × 16)
        upper_h = rect_h - 0x10
        z_upper = this->vtable[0x2EC](mode=0, ...) - 5   // -5 priority bias
        color_upper = g_PrimarySurface->[0x78](upper_rect, ..., blitter, z_upper)
        Standard_SHP_blitter(upper_rect, g_PrimarySurface, color_upper,
                             ..., blitter, z_upper, sub_index = 0, ...)

        lower_rect.y     = rect_y + upper_h
        lower_rect.w     = 0x10
        lower_rect.h     = 0x10
        z_lower = this->vtable[0x2EC](mode=2, ...)        // mode 2, no -5 bias
        color_lower = g_PrimarySurface->[0x78](lower_rect_ptr, ..., blitter, z_lower)
        Standard_SHP_blitter(lower_rect, ..., blitter, z_lower, sub_index = 2, ...)
        return

    // DEFAULT — single full-sprite blit
    pre = this->vtable[0x2F0]()
    z   = this->vtable[0x2EC](pre)
    color = g_PrimarySurface->[0x78](sprite_rect, ..., blitter, z)
    Standard_SHP_blitter(sprite_rect, g_PrimarySurface, color, ..., blitter, z, pre, ...)
```

**Tiny details:**
- The lower-strip width is **0x10 (16) NOT the sprite's actual width** — the
  decompiled code assigns `iStack_34 = 0x10` for the lower slice. So only a
  16×16 square at the bottom-center of the original sprite rectangle is blitted
  in the second pass. This may be wrong (or it may be intentional — gamemd's
  bridge graphic only obscures a small footprint).
- `iStack_2C = iStack_2C + iStack_24` shifts the lower-slice Y by the **original
  iStack_24 value**, which is `DAT_00B1CFC8` (sprite width, not height — likely
  a bug/typo in gamemd that we should reproduce verbatim). After this shift
  `iStack_24` is then reassigned to `0x10`. Worth verifying again under a live
  in-game scenario before assuming gamemd uses sprite-width for the y-shift.
- Mode argument differs: upper slice uses `vtable[0x2EC](0, ...) - 5`, lower
  slice uses `vtable[0x2EC](2, ...)` (no -5).
- `Blitter_selector(flags)` decides whether translucency/team-color/cloak gets
  applied. The flag set `0x2800 / 2802 / 2804 / 280A / 280C` corresponds to
  unit-draw blitter variants (likely opaque vs translucent vs team-tinted).
- The second OR-clause (cond_B) — a destination targeting a Building whose
  TypeClass has byte +0x16BD set — is an **alternate trigger** independent of
  IsOnBridge. We did not chase what +0x16BD identifies on BuildingTypeClass;
  it is likely "is a bridge repair hut" or "is a bridge piece building". Worth
  a follow-up if this code path matters for parity. Documented here so it is
  not lost.

### 3.6 Helper: `CellClass::IsShrouded` (`FUN_00487950` @ `0x00487950`)

```
bool IsShrouded(CellClass* cell):
    coord = cell->[0x24]                // packed (x:short, y:short)
    y = (short)((uint)coord >> 0x10)
    local_14 = 0x80 / 0x80              // setup for FUN_0047B3A0
    local_4 = FUN_0047B3A0(&local_14)
    local_c = (short)coord * 0x100 + 0x80
    local_8 = y * 0x100 + 0x80
    return IsShrouded(&local_c)         // returns bool — TEST AL,AL at the call site
```

Used only as a side-effect query in `FUN_0073B140`. The return value gates a
single stack-slot zeroing and has no further influence on the TooBig check.

---

## 4. INI keys & usage

| Key | Type | Section | Default | Effect |
|---|---|---|---|---|
| `TooBigToFitUnderBridge=` | bool | `[UNITID]` (UnitType) | `no` | Triggers the rendering-only Z fudge described in Section 3 when the unit is at a bridge edge cell, **only on the SHP draw path**. No movement effect. |

### Units that set it in the shipped INI (revised 2026-05-13 after Turret resolution)

`rules.ini`: 27 entries `=true` plus 3 commented (`CARRIER`, `DLPH`, `DRED`).
`rulesmd.ini`: 40 entries `=true` plus 3 commented.

The relevant axis for the **main-draw** Z bias is `Turret=` (which controls
whether the unit hits the body+turret draw path or the simple no-turret path):

**`TooBig=yes` AND `Turret=no`** — main-draw Z bias DOES fire (in addition to
shadow split-blit):
- **Naval:** `DEST`, `CDEST`, `CARRIER`, `DRED`, `SUB`, `BSUB`, `SQD`, `CRUISE`, `TUG`
- **Big land:** `MGTK`, `V3`, `HOWI`, `TNKD`, `SAPC`, `VLAD`, `LCRF`, `YHVR`
- **Drones:** `DNOA`, `DNOB`, `DRON`

(Most of the naval entries are `Turret=no` because of the gamemd content
constraint `Turret and NoSpawnAlt can't both be set; both use AuxVoxel`.)

**`TooBig=yes` AND `Turret=yes`** — main-draw Z bias is SKIPPED (still gets
shadow split-blit at the sibling vtable slot):
- **Tanks:** `MTNK`, `HTNK` (Apocalypse), `LTNK`, `UTNK`, `TTNK`, `YTNK`,
  `ROBO`, `TELE`, `MIND`, `DISK`, `XCOMET`, `FV`
- **Misc:** `SREF`, `SCHP`, `SCHD`, `HARV`, `HTK`, `SMIN`

**`TooBig=yes` with no explicit Turret (default = no):**
- `BFRT`, `DLPH` (Dolphin), `SHAD` (Black Hawk).

**Implication for parity:** treating this as a path block (as the current Rust
port does) **changes behavior the original game does not exhibit** — gamemd
lets the entire `Turret=no` capital-ship fleet path freely through bridge cells
and renders the sprite with a -16 Z bias to keep the visual layering correct.

---

## 5. Integration points

- **Parse:** `UnitTypeClass::ReadINI` @ `0x0074774E` — reads `"TooBigToFitUnderBridge"`
  via `CCINIClass::ReadBool` with the current `+0xE16` value as default (and the
  constructor at `0x007470D0` zero-inits it). Order in ReadINI: between `CarriesCrate`
  (which reads `+0xE1A`) and `HalfDamageSmokeLocation`. Same `param_1` is `int` —
  direct byte offsets (no `int*` × 4 trap).
- **Read at runtime (only places in the entire binary):**
  - `FUN_0073B140` (vtable slot `+0x1CC` on UnitClass-related vtable at `0x007F6000`) —
    SHP blit helper.
  - `FUN_0073C5F0` (vtable slot `+0x1C8` on the same vtable) — main draw, SHP branch only.
- **Not read by:**
  - `UnitClass::Can_Enter_Cell` (`0x0073F0A0`) — passability/A* costing
  - `FootClass::LocomotorPassabilityCheck`
  - `CellClass`-side passability or speed-modifier lookups
  - `Mission_Move`, `Mission_Hunt`, `Mission_Harvest`
  - Pathfinding (`Pathfinding_update_continued`)
  - Tube/tunnel logic, scatter logic, retaliation logic
- **Tick ordering:** these are draw-pipeline reads. They occur once per visible unit
  per frame during the render phase, well after sim tick logic. Not in the
  command/movement loop.

### Byte-pattern audit (confirmation of completeness)

Searching the binary for the displacement bytes `16 0E 00 00`:

| Address | What it is |
|---|---|
| `0x00407CF6` | False positive — inside a `CALL` immediate in `StreamPlayer__PlayFile` (audio code, unrelated) |
| `0x0073B1B2` | Real reader — `MOV AL, [ECX+0xE16]` inside `FUN_0073B140` |
| `0x0073CE0F` | Real reader — `MOV AL, [ECX+0xE16]` inside `FUN_0073C5F0` tail |
| `0x00747145` | `MOV byte ptr [ESI+0xE16], BL` — constructor zero-init |
| `0x00747749` | Read for ReadBool default inside `UnitTypeClass::ReadINI` |
| `0x0074777A` | Write of ReadBool result inside `UnitTypeClass::ReadINI` |

For comparison, sibling fields show similar small counts: `+0xE15` (UseTurretShadow):
6 hits → 5 real readers; `+0xE17` (CanBeach): 4 hits → 3 real readers. The two
real readers for `+0xE16` is the complete set. (Per-byte loads through computed
offsets — e.g. `ADD reg, 0xE16; MOV AL, [reg]` — would still embed the same
4-byte displacement; only an instruction-pair using `ADD reg, 0x800; MOV AL,
[reg + 0x616]` could hide it, and the compiler does not emit such code for a
simple field access here.)

---

## 6. Current Rust implementation status

Per the parallel scan, the Rust port currently treats this flag as a movement
gate, which **does not match gamemd**:

- `src/rules/object_type.rs:392` — `pub too_big_to_fit_under_bridge: bool` field.
  Parsed at `object_type.rs:892` via `section.get_bool("TooBigToFitUnderBridge")`.
  Default `false`. ✅ Matches gamemd's INI parsing.
- `src/sim/game_entity.rs:170` — entity component carries the flag.
- `src/sim/movement/mod.rs:132` — movement snapshot carries the flag.
- `src/sim/movement/movement_path.rs:27-45`:
  ```
  fn merge_path_blocks(..., too_big_to_fit_under_bridge: bool) -> BTreeSet<(u16,u16)> {
      if too_big_to_fit_under_bridge && movement_zone.is_some_and(|mz| !mz.is_water_mover()) {
          // Hard A* block on every under-bridge cell
          for cell in terrain.iter().filter(is_under_bridge_blocked_cell) {
              blocks.insert((cell.rx, cell.ry));
          }
      }
      blocks
  }
  ```
  **gamemd does not do this.** The flag has no effect on cell-passability or A*.
- `src/sim/movement/movement_path.rs:497-501` — test asserts water movers stay
  unblocked. The water-mover carve-out is unnecessary because **no** unit
  type is supposed to be blocked by this flag.
- `src/sim/movement/movement_blocked.rs:42, 118` — flag threaded into repath
  handling. Same problem: it shouldn't be gating movement at all.
- `src/sim/movement/movement_bridge.rs:50` — `BRIDGE_Z_OFFSET = 360` for ship
  braking-distance under bridges, **unrelated** to this flag.

**Gap vs gamemd:** the *actual* rendering effect (Z-bias / split blit on the SHP
draw path) is **not implemented**. Whether implementing it is worth the effort
depends on whether any visible SHP unit (Dolphin, possibly SHAD) routinely
crosses a bridge in normal play. The original code path is shipped but
practically dormant.

**Suggested rectification (research finding — not a directive):**
1. Remove the A* block in `movement_path.rs`. Tanks and ships should be free
   to path under/through bridge cells subject only to normal cell passability
   rules (HighBridge flag + height-level matching) — which gamemd handles
   uniformly in `UnitClass::Can_Enter_Cell` independent of unit type.
2. Drop the water-mover carve-out — it papered over the bigger error.
3. Optionally implement the rendering Z-bias for SHP units at bridge edge cells.
   Low priority: applies only to Dolphin/Black-Hawk-Transport scale of units.

This finding directly contradicts several existing docs:
- `BRIDGE_SYSTEM.md:1161` — "Most ships have `TooBigToFitUnderBridge=true` → cannot
  enter bridge cells" — **WRONG**. gamemd does not gate cell entry on this flag.
- `NAVAL_SYSTEM_RESEARCH.md:231` — same incorrect claim. **WRONG**.
- `BRIDGE_RENDERING_GHIDRA_REPORT.md:751` — "prevents unit from going under
  bridges entirely" — **WRONG**.
- `BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md:563` — confidence "TBD (locomotor)" — at
  least correctly flagged as unverified.

The first three appear to be inferred from the field name rather than from the
binary. Recommend running `/verify-doc` on each.

---

## 7. Open questions

1. ~~**What is `TypeClass+0xCA1`?**~~ **RESOLVED 2026-05-13.** It is the **`Turret`**
   bool. See [TECHNOTYPECLASS_TURRET_FIELD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOTYPECLASS_TURRET_FIELD_GHIDRA_REPORT.md).

2. **`BuildingTypeClass+0x16BD`** — the second-OR-clause condition in `FUN_0073B140`
   triggers the split-blit if the unit's destination is a Building whose TypeClass
   has byte +0x16BD set. Likely a "is bridge piece" or "is bridge repair hut" flag.
   Not investigated here.

3. **Lower-slice dimensions in `FUN_0073B140`** — the decompilation shows the lower
   slice as `16 × 16` (width = 0x10, height = 0x10). This looks suspicious because
   the upper slice spans `rect_w × (rect_h - 16)`. Worth a live in-game check to
   verify what the lower strip actually contains in pixels.

4. **The Y-shift of the lower slice** — `iStack_2C = iStack_2C + iStack_24` where
   `iStack_24 = DAT_00B1CFC8` (sprite width). Is gamemd actually shifting the
   lower-slice Y by the sprite's width, or did the decompiler mis-track a swap
   somewhere? Re-disassembling around `0x0073B1E0`-`0x0073B260` would settle it.

5. **Aircraft draw path** — `SHAD` (Black Hawk Transport) carries the flag.
   Aircraft typically use a different draw method on `AircraftClass`. Whether the
   vtable inheritance reaches `FUN_0073B140` for aircraft is unconfirmed.

6. **Does any other byte field beyond +0xE16 piggyback on bridge-edge rendering?**
   `ZFudgeBridge=` (TechnoTypeClass+0xDCC) is separate, but `+0xE15`
   (UseTurretShadow) has 5 readers and one is in the rendering range
   (`0x00662217`) — could interact with the same code path. Not chased.

---

## Sources

**Ghidra decompilation (live, this session):**
- `0x00845DC8` — INI string `"TooBigToFitUnderBridge"`
- `0x0074774E` — `UnitTypeClass::ReadINI`
- `0x007470D0` — `UnitTypeClass::Constructor`
- `0x0073C5F0` — main draw (created in this session, formerly unnamed)
- `0x0073B140` — SHP blit helper (created in this session, formerly unnamed)
- `0x0073CE0D` — branch tail (created in this session)
- `0x00703B10` — `TechnoClass::IsOnBridge_ForFiring`
- `0x00703CC0` — adjacent low-bridge ramp probe
- `0x00703E70` — bridge-piece neighbor counter
- `0x00487950` — `CellClass::IsShrouded` wrapper
- `0x0073F0A0` — `UnitClass::Can_Enter_Cell` (verified NO read of `+0xE16`)
- `0x005F92D0` — `ObjectTypeClass::ReadINI` (Voxel key at `+0x236`, confirming
  0xCA1 is NOT Voxel)
- Byte-pattern audit at `16 0E 00 00` (6 hits, 2 real readers)

**Existing reports cross-checked:**
- `BRIDGE_SYSTEM.md`, [BRIDGE_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_GHIDRA_REPORT.md), `BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md`,
  [NAVAL_SYSTEM_RESEARCH.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVAL_SYSTEM_RESEARCH.md), [SUBMARINE_AND_SINKING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUBMARINE_AND_SINKING_GHIDRA_REPORT.md),
  [MCV_DEPLOY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MCV_DEPLOY_GHIDRA_REPORT.md), [DRIVE_LOCOMOTION_CLASS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_LOCOMOTION_CLASS.md),
  `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md`,
  [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md)

**INI files:**
- `ini/rulesmd.ini`, `ini/rules.ini` (artmd.ini and art.ini do not contain the key)

**Rust source surveyed (not modified):**
- `src/rules/object_type.rs:392, 892`
- `src/sim/game_entity.rs:170`
- `src/sim/movement/mod.rs:132`
- `src/sim/movement/movement_path.rs:27-45, 497-501`
- `src/sim/movement/movement_blocked.rs:42, 118`
- `src/sim/movement/movement_bridge.rs:50`
