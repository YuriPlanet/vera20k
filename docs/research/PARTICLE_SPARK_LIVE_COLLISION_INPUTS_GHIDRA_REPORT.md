# Particle Spark Live Collision Inputs — Ghidra Report

**Date:** 2026-07-18

**Target:** active Yuri's Revenge `gamemd.exe`, image base `0x00400000`, x86 32-bit

**Primary consumer:** Spark particle AI `0x0062C6E0`

**Investigation mode:** exhaustive-slice

**Confidence:** HIGH for the declared valid-stock-cell slice; the slope matrix bit table is an exact replay of the retail executable's deterministic initialization code and lookup tables, not a runtime memory capture

**Implementation scope:** none; this report is a research handoff and intentionally changes no Rust code

**Post-implementation status (2026-08-27):** Spark shared-dummy routing and
invalid/unallocated-cell continuation are implemented. The current authority is
the [Phase 3 Spark shared-dummy routing contract](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_SPARK_SHARED_DUMMY_ROUTING_GHIDRA_REPORT.md).
Implementation and review repairs are `4c71b488`, `72bf8e15`, `96779c16`, and
`0054549e`. Any statement below that says Spark still returns typed unavailable,
has not integrated the dummy, or leaves invalid-cell parity open is a historical
pre-`4c71b488` Rust baseline, not current behavior. The native evidence remains
valid; only those dated Rust-state conclusions are superseded.

**2026-08-27 numeric correction:** active runtime capture in
[PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md)
supersedes this report's original 90/360/340 interpretation. Cell ground is
104; Particle's independently owned structural offset is 416; Spark's
ascending commit is ground +396. The text below incorporates that correction.

## Verdict

The live-input prerequisites for the approved Spark collision adapter are closed for valid stock map cells.

The adapter must not approximate these inputs:

- ground height uses a signed cell conversion, a flattened 512-stride lookup, the active-retail 104-lepton Cell scalar, and one of 20 slope records;
- Spark queries a 3x4 `f32` matrix by the candidate cell's slope byte, including identity for slope 0 and slopes 17-20;
- the structural bridge test is the live `CellClass+0x140 & 0x100` bit, not the presence of a bridge overlay or a generic deck height;
- high-bridge collapse clears `0x100`, and engineer repair does **not** restore it—an active native quirk;
- `Gravity` is an `i32` rule with constructor default 3 and stock override 6, while `ColorSpeed` is stored as the exact `f64` widening of an INI-parsed `f32`;
- the stock LaserFence exception is compiled and reachable only when a type enables it; stock rules do not enable it.

At the 2026-07-18 Rust baseline, most source facts already had owners, while
precision and activation work remained. In that baseline the shared mutable
dummy substrate existed in `src/sim/cell_rect.rs`, but Spark had not yet routed
through it and still returned typed unavailable. That specific historical gap
is closed by `4c71b488` and the three review-repair commits listed above.

## 1. Scope, duplication check, and source order

### Included

1. World-lepton X/Y to `CellClass` selection used by the ground query.
2. Exact ground height for valid cells, including signed levels and slope types 0-20.
3. Exact Spark slope matrices for slope types 0-20.
4. Map-load, collapse, and repair lifecycle of structural bridge bit `0x100`.
5. `Gravity` and `ColorSpeed` parse, defaults, widths, and stock values.
6. Rust ownership of ground, bridge, occupancy, wall-overlay, and rule facts.
7. Stock reachability of the LaserFence special case.

### Explicitly outside this slice

- Spark movement/collision/color arithmetic and the pixel compositor, already covered by [PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md).
- Spawn, public dispatch, rendering activation, Railgun behavior, and final retail display masks.
- A complete taxonomy of all mutations to the shared off-map dummy `CellClass`.
- Mod-only LaserFence construction and connectivity semantics.

The duplicate-work check read the existing Spark report, bridge flag/stamping/collapse/repair reports, ground/deck-height reports, and VXL slope reports before opening new binary paths. This report extends only the unresolved live-input boundary. It also supersedes two stale claims identified below; it does not use those claims as evidence.

Binary findings came from a Ghidra MCP session opened on `/gamemd.exe` before this investigation. Load-bearing operations were fresh `decompile_function`/`disassemble_function`, `get_xrefs_to`, and `read_memory` calls at the addresses named inline. When the legacy bridge later became unstable, exact arithmetic constants and lookup entries were independently reread from the same retail PE image; no Ghidra database changes were saved.

## 2. Opening questions log

This log was captured before the focused pass. The final disposition is in §10.

| ID | Opening question |
|---|---|
| S01 | Which Spark stage owns and consumes the collision fact bundle? |
| G01 | How are signed world X/Y values converted to cell coordinates? |
| G02 | What is the exact base-height formula and rounding? |
| G03 | What is the exact slope contribution formula, clamp order, and rounding? |
| G04 | What are all 20 ground-slope records? |
| G05 | What happens at flattened-index boundaries and on dummy-cell lookup? |
| M01 | Who initializes the VXL slope matrix table and which slots are written? |
| M02 | What are the exact matrices for slopes 0-20? |
| M03 | Does lookup clamp, substitute identity, or special-case dummy cells? |
| B01 | Which cells receive structural bit `0x100` at map load? |
| B02 | What does high-bridge collapse do to `0x100`? |
| B03 | Does engineer repair restore `0x100` directly or through `RecalcAttributes`? |
| B04 | Do forward-3 and direction-6 extra cells count as structural deck cells? |
| B05 | Which Rust state must be combined to reproduce the live bit? |
| R01 | How is `Gravity` parsed and what is its missing-key default? |
| R02 | How is `ColorSpeed` parsed and what is its missing-key default? |
| R03 | What are the exact stock numeric bits supplied to Spark? |
| R04 | Does current Rust retain those native widths? |
| T01 | When must facts be gathered relative to mutable RNG use and cleanup? |
| L01 | Is the LaserFence branch active in stock YR data? |
| E01 | Do these facts require adapter-local state across pause/save/load? |

## 3. Coordinate and ground-height mechanism

### 3.1 Cell lookup

`CellClass::GetGroundHeight` wrapper `0x00578080` and `MapClass::Get_CellClass_At_Coord` `0x00565730` perform the same signed conversion for each world/lepton axis:

```text
cell_axis = (world_axis + ((world_axis >> 31) & 0xFF)) >> 8
```

This is signed truncation toward zero by 256. Concrete fixtures:

| World/lepton axis | Cell axis |
|---:|---:|
| `255` | `0` |
| `256` | `1` |
| `-1` | `0` |
| `-255` | `0` |
| `-256` | `-1` |

The lookup then forms `linear = cell_y * 0x200 + cell_x`. It validates the flattened result and pointer, not X and Y independently. Consequently, an individually out-of-range axis can alias an in-range flattened slot. A Rust helper that bounds-checks X/Y separately is not equivalent at this boundary.

On failure or null pointer, the wrapper uses the shared dummy cell at `0x00ABDC50`, writes the requested packed cell coordinate at dummy `+0x24` (`0x00ABDC74`), and calls the same inner ground function. `CellClass::Constructor @ 0x0047BBF0`, reached for the dummy at the end of the `MapClass::Resize` path around `0x005670E7`, initializes dummy level `+0x11B` and slope `+0x11C` to zero. Other off-map helpers can mutate shared dummy state later. Rust models that shared mutable substrate in `cell_rect`, and Spark now consumes it through the native-ordered routing implemented in `4c71b488`.

Evidence: fresh Ghidra `disassemble_function(address="0x00578080")`, `decompile_function(address="0x00565730")`, `decompile_function(address="0x0047BBF0")`, and disassembly around `0x005670E7`; the coordinate conversion and dummy writes are explicit instructions, not inferred names.

### 3.2 Base height

The inner ground function `0x0047B3A0` receives `ECX = CellClass*` and a pointer to the original world/lepton coordinate. It lazily initializes:

- ground `LevelHeight = 104`;
- `LevelHeight / 256 = 0.40625`;
- the 20 ground-slope records in §3.3.

The ground evaluator owns only the 104-based floor and slope result. Cell's
416 structural-deck offset is initialized separately. Spark does not consume
that Cell-owned offset: it reads Particle's independently initialized 416 and
composes `ground + 416` in its collision path. See the active-runtime ownership
census in
[PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_CELL_GROUND_HEIGHT_104_DOMAIN_CONSUMER_CENSUS_GHIDRA_REPORT.md).

Base height is:

```text
base = ftol_chop(sign_extend_i8(cell.level) * 104 + 0.5)
```

For ordinary nonnegative map levels this is exactly `level * 104`. The signed load matters for corrupted or synthetic negative levels: level `-1` produces `ftol(-103.5) = -103`, not `-104`.

Evidence: fresh Ghidra `decompile_function(address="0x0047B3A0")` plus full `disassemble_function(address="0x0047B3A0")`; the body uses `MOVSX`, `FILD`, adds the `0.5` constant at `0x007E1738`, and calls `Math__ftol @ 0x007C5F00`.

### 3.3 Slope contribution

Slope zero returns `base`. For nonzero `s`, the function reads record `(s - 1) * 0x28` from `0x0081C900`. Only the low unsigned byte of world X and Y participates:

```text
x = world_x & 0xFF
y = world_y & 0xFF
raw = y * coeff_y * (104 / 256)
    + x * coeff_x * (104 / 256)
    + bias_a
    + bias_b
clamped = min(max(raw, 0.0), max_value)
height = ftol_chop(base + clamped)
```

The clamp occurs before adding base; final conversion chops toward zero. Let `L = 104`:

| Slope | `coeff_x` | `coeff_y` | `bias_a` | `max` | `bias_b` |
|---:|---:|---:|---:|---:|---:|
| 1 | `1` | `0` | `0` | `L` | `0` |
| 2 | `0` | `1` | `0` | `L` | `0` |
| 3 | `-1` | `0` | `L` | `L` | `0` |
| 4 | `0` | `-1` | `L` | `L` | `0` |
| 5 | `1` | `1` | `-L` | `L` | `0` |
| 6 | `-1` | `1` | `0` | `L` | `0` |
| 7 | `-1` | `-1` | `L` | `L` | `0` |
| 8 | `1` | `-1` | `0` | `L` | `0` |
| 9 | `1` | `1` | `0` | `L` | `0` |
| 10 | `-1` | `1` | `L` | `L` | `0` |
| 11 | `-1` | `-1` | `2L` | `L` | `0` |
| 12 | `1` | `-1` | `L` | `L` | `0` |
| 13 | `1` | `1` | `0` | `2L` | `0` |
| 14 | `-1` | `1` | `L` | `2L` | `0` |
| 15 | `-1` | `-1` | `2L` | `2L` | `0` |
| 16 | `1` | `-1` | `L` | `2L` | `0` |
| 17 | `0` | `0` | `0` | `L/2` | `L/2` |
| 18 | `0` | `0` | `L` | `L/2` | `-L/2` |
| 19 | `0` | `0` | `0` | `L/2` | `L/2` |
| 20 | `0` | `0` | `L` | `L/2` | `-L/2` |

Ground slopes 17-20 therefore all add exactly 52 leptons, independent of local X/Y. Their VXL/Spark reflection matrices are independently initialized to identity (§4).

Evidence: Ghidra disassembly `0x0047B3ED..0x0047BA8E` for record initialization and `0x0047BA94..0x0047BB58` for the evaluation/clamp/return path. `FUN_006D6AD0` independently confirms that slope is read as the unsigned byte at `CellClass+0x11C` with no clamp.

## 4. Exact Spark slope matrices

### 4.1 Initialization and lookup

`VXL_MasterLighting_Init @ 0x00754CB0` is the sole active startup owner. It initializes entry 0 to identity and calls `Matrix3x4_BuildFromRotateXAndFacing @ 0x005AE6F0` for entries 1-16. The initializer tail at `0x00755852..0x00755875` calls identity builder `0x005AE860` on `0x00B454B8`, `0x00B454E8`, `0x00B45518`, and `0x00B45548`, initializing entries 17-20 to identity.

`VXL_GetFacingMatrix @ 0x007559B0` computes `0x00B45188 + slope * 0x30` and copies 12 dwords. It has no range check, remap, flat-cell early-out, or fallback. Spark passes the candidate terrain slope byte directly, so:

- slope 0 returns the initialized identity entry;
- slopes 1-16 return initialized rotations;
- slopes 17-20 return initialized identity matrices.

Evidence: fresh Ghidra `decompile_function` and `disassemble_function` on `0x00754CB0`, `0x005AE6F0`, and `0x007559B0`; the complete original initializer, including its tail, is executed by `tools/projectile_oracle/ordinary_collision.py`. Static PE zero bytes do not describe the initialized runtime table.

### 4.2 VXL height and tilt derivation

Cell ground and VXL use independently owned and initialized globals, but active runtime capture proves that both `LevelHeight` values are 104. Independent ownership does not create different numeric domains.

`VXL_Init_CellHeightRatio @ 0x007549E0` uses the tangent lookup `0x004CAD50`, not an analytic sine. Its `pi/6` sample selects table index 341, whose retail `f32` is `0.5766686797142029`; multiplying by half the 256-cell diagonal and chopping yields VXL level height 104.

The two initialized tilt values are lookup-quantized `f32` values:

| Tilt | Input to atan lookup | Table index | `f32` value | Bits |
|---|---:|---:|---:|---:|
| corner | `104 / 256` | `16` | `0.3723885416984558` | `0x3EBEA9B6` |
| edge | `208 / sqrt(2*256²)` | `23` | `0.5116347670555115` | `0x3F02FA7F` |

These values supersede analytic `atan(...)` approximations in older VXL prose. The builder starts from identity and applies `RotateZ(compass)`, `RotateX(tilt)`, `RotateZ(-compass)`. The rotation helpers call the retail lookup functions `0x004CACB0` and `0x004CAD00`; their results are stored as `f32` after each matrix element update.

Evidence: Ghidra `disassemble_function` on `0x007549E0`, `0x00754A20`, `0x00754A50`, `0x004CAD50`, `0x004CADE0`, `0x004CACB0`, `0x004CAD00`, `0x005AEF60`, and `0x005AF1A0`. Exact table values and constants were cold-reread from the retail PE at virtual addresses `0x0085D0A4`, `0x008610B4`, `0x008650B8`, and `0x008223B0`.

### 4.3 Exact 3x4 `f32` bit table

Rows below are row-major 3x4 matrices. Zero translation columns are retained because Spark copies and transforms all 12 fields. Small off-axis values are real consequences of the executable's quantized compass/trig lookups; do not normalize them away.

| Slope | Twelve `f32` bit patterns |
|---:|---|
| 0 | `3F800000 00000000 00000000 00000000 00000000 3F800000 00000000 00000000 00000000 00000000 3F800000 00000000` |
| 1 | `3F5F3968 BAAF51EE BEFAA753 00000000 3AC90FD4 3F7FFFEC 250A3D28 00000000 3EFAA73F BA44DCE0 3F5F397A 00000000` |
| 2 | `3F7FFFEC BAC90FD4 248A3D28 00000000 3AAF51EE 3F5F3968 3EFAA753 00000000 BA44DCE0 BEFAA73F 3F5F397A 00000000` |
| 3 | `3F5F3968 BAAF51EE 3EFAA753 00000000 3AC90FD4 3F7FFFEC A48A3D28 00000000 BEFAA73F 3A44DCE0 3F5F397A 00000000` |
| 4 | `3F800000 00000000 00000000 00000000 00000000 3F5F397A BEFAA753 00000000 00000000 3EFAA753 3F5F397A 00000000` |
| 5 | `3F773916 3D06934A BE83D955 00000000 3D12B5D0 3F77322F 3E83D955 00000000 3E83A584 BE840D12 3F6E6B6D 00000000` |
| 6 | `3F77322F BD12B5D0 3E83D955 00000000 BD06934A 3F773916 3E83D955 00000000 BE840D12 BE83A584 3F6E6B6D 00000000` |
| 7 | `3F773916 3D06934A 3E83D955 00000000 3D12B5D0 3F77322F BE83D955 00000000 BE83A584 3E840D12 3F6E6B6D 00000000` |
| 8 | `3F77322F BD12B5D0 BE83D955 00000000 BD06934A 3F773916 BE83D955 00000000 3E840D12 3E83A584 3F6E6B6D 00000000` |
| 9 | `3F773916 3D06934A BE83D955 00000000 3D12B5D0 3F77322F 3E83D955 00000000 3E83A584 BE840D12 3F6E6B6D 00000000` |
| 10 | `3F77322F BD12B5D0 3E83D955 00000000 BD06934A 3F773916 3E83D955 00000000 BE840D12 BE83A584 3F6E6B6D 00000000` |
| 11 | `3F773916 3D06934A 3E83D955 00000000 3D12B5D0 3F77322F BE83D955 00000000 BE83A584 3E840D12 3F6E6B6D 00000000` |
| 12 | `3F77322F BD12B5D0 BE83D955 00000000 BD06934A 3F773916 BE83D955 00000000 3E840D12 3E83A584 3F6E6B6D 00000000` |
| 13 | `3F6FA319 3D80294A BEB13D26 00000000 3D860AD0 3F6F963A 3EB13D26 00000000 3EB0F77E BEB182B2 3F5F397A 00000000` |
| 14 | `3F6F963A BD860AD0 3EB13D26 00000000 BD80294A 3F6FA319 3EB13D26 00000000 BEB182B2 BEB0F77E 3F5F397A 00000000` |
| 15 | `3F6FA319 3D80294A 3EB13D26 00000000 3D860AD0 3F6F963A BEB13D26 00000000 BEB0F77E 3EB182B2 3F5F397A 00000000` |
| 16 | `3F6F963A BD860AD0 BEB13D26 00000000 BD80294A 3F6FA319 BEB13D26 00000000 3EB182B2 3EB0F77E 3F5F397A 00000000` |
| 17 | `3F800000 00000000 00000000 00000000 00000000 3F800000 00000000 00000000 00000000 00000000 3F800000 00000000` |
| 18 | `3F800000 00000000 00000000 00000000 00000000 3F800000 00000000 00000000 00000000 00000000 3F800000 00000000` |
| 19 | `3F800000 00000000 00000000 00000000 00000000 3F800000 00000000 00000000 00000000 00000000 3F800000 00000000` |
| 20 | `3F800000 00000000 00000000 00000000 00000000 3F800000 00000000 00000000 00000000 00000000 3F800000 00000000` |

The table is captured by executing the original CRT precision setup, WinMain rounding control, cached-control-word initializer, VXL scalar/tilt initializers, and complete `0x00754CB0` body. `WinMain 0x006BBFB7..0x006BBFC9` selects chop rounding and caches it at `0x00822D80`; the CRT selects 53-bit precision. The ambient arithmetic mode is `0x0E7F` (reserved bit 6 does not affect the captured words). Earlier replay values with a different rounding chronology are superseded. It is suitable as an implementation golden for the native-derived table builder or as the direct constant table. It is not a Rust-vs-Rust parity certificate; the reference bytes come from `gamemd.exe`.

## 5. Structural bridge bit lifecycle

### 5.1 Map load and collapse

`CellClass::SetBridgeDirection_NESW @ 0x0047E040` and `_NWSE @ 0x0047E470` are the authoritative structural-bit writers for this slice.

For intact state 1, structural bit `0x100` is set on four slots: anchor, forward-1, forward-2, and opposite. Forward-3 and the direction-6 extra cell do not receive `0x100`. For destroyed state 0, the same four slots clear `0x100` and set destroyed bit `0x400`.

`ProcessBridgeDamageStateMachine_High @ 0x00576BA0` calls the direction setter with state 0 before clearing the terminal bridge overlay/state. Spark therefore sees the high bridge plane only while the queried cell's live `0x100` remains set.

Evidence: fresh Ghidra decompile/disassembly of `0x0047E040`, `0x0047E470`, and `0x00576BA0`, including the state-0 callsite at `0x0057778A..0x00577795`. The four structural slots agree with the existing bridge stamping and collapse reports.

### 5.2 Engineer repair quirk

High repair dispatch `0x0057F440` and high repair walkers `0x005800D0`/`0x00580600` rewrite the overlay triples and call `CellClass::RecalcAttributes @ 0x0047D2B0`. They do not call either `SetBridgeDirection` function and do not directly write `CellClass+0x140`.

The complete `RecalcAttributes` body does not re-establish high structural bit `0x100`. Its only nearby flag write ORs unrelated bit `0x10000` into neighboring cells under an animation-list path. Therefore engineer repair does **not** restore the Spark-visible bridge bit after a full collapse.

This corrects [BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md) lines 1477-1481, which say `RecalcAttributes` re-derives `0x80/0x100/0x400`. Replace that claim conceptually with:

> Repair walkers rewrite overlays and recalculate terrain attributes, but neither they nor `RecalcAttributes @ 0x0047D2B0` restore structural bit `0x100`; a collapsed-and-repaired high bridge remains non-structural to readers of `CellClass+0x140 & 0x100`.

The later [REPAIRBRIDGEWALKER_BODIES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/REPAIRBRIDGEWALKER_BODIES_GHIDRA_REPORT.md) write audit already supports this correction.

### 5.3 Rust mapping

`ResolvedTerrainCell.bridge_facts.has_structural_bridge()` is a static map-load fact. `BridgeRuntimeCell.deck_present` starts true, collapse sets it false, and the current repair walker does not set it true. That apparent omission matches the native quirk for Spark's `0x100` reader.

The adapter's structural query must be:

```text
if static structural bit is false: false
if static structural bit is true and matching runtime cell exists:
    runtime.deck_present
if static structural bit is true but authoritative runtime state/cell is unavailable:
    typed unavailable/error
```

Do not use `static_structural || runtime.deck_present`; do not infer structural presence merely from a repaired overlay; and do not treat every generic runtime deck/bridgehead cell as native bit `0x100`.

Relevant Rust owners: `src/map/bridge_facts.rs`, `src/map/resolved_terrain.rs`, `src/sim/bridge_state/mod.rs`, `src/sim/bridge_state/walker.rs`, and `src/sim/world/bridge_orchestrator.rs`.

## 6. Rules numeric semantics

### 6.1 Gravity

The `Gravity` string has one active rules-reader xref: `RulesClass::ReadAudioVisual @ 0x006691E0`. It calls `CCINIClass::ReadInt @ 0x005276D0` and stores a signed 32-bit integer at `RulesClass+0x16B8`. `RulesClass` constructor `0x00665650` initializes that field to 3 (`0x006674D6`). Stock `rules.ini:615` and `rulesmd.ini:756` both override it with 6.

Spark `0x0062C6E0` uses `FILD dword [Rules+0x16B8]` at `0x0062C705` and again at `0x0062C762`; the motion path rounds/stores through `f32`. The adapter input for stock is therefore native `f32` `6.0`, bits `0x40C00000`, derived from signed integer 6—not parsed as an arbitrary float or fixed point.

Current `GeneralRules` has no `Gravity` field. That is a blocking Rust delta.

### 6.2 ColorSpeed

The `ColorSpeed` string has one active xref: `ParticleTypeClass::ReadINI @ 0x00644F50`. Around `0x006451F8`, it calls `CCINIClass::ReadDouble @ 0x005283D0` using the existing 8-byte value and stores the returned `f64` at `ParticleTypeClass+0x2B0`. `ParticleTypeClass` constructor `0x00644BE0` initializes this field to `0.0`.

`ReadDouble` parses through `sscanf("%f")` into `f32`, widens that exact `f32` to `f64`, and applies `* 0.01` only if a percent marker exists. Stock `.13` therefore becomes:

| Stage | Value/bits |
|---|---|
| parsed `f32` | `0.12999999523162842`, bits `0x3E051EB8` |
| widened stored `f64` | bits `0x3FC0A3D700000000` |

Current `src/rules/ini_value.rs::read_double` already implements this parse contract, but `src/rules/particle_type.rs` currently parses `ColorSpeed` with `get_f32()` and quantizes it into `SimFixed`. That field is DRIFT for Spark. It must retain `NativeF64Bits` or an exact `f64` representation through the rule boundary.

Evidence: Ghidra `get_xrefs_to` on the two strings, `decompile_function`/`disassemble_function` for `0x006691E0`, `0x005276D0`, `0x00665650`, `0x00644F50`, `0x005283D0`, and `0x00644BE0`, plus direct stock INI reads.

## 7. Remaining collision fact owners

The parent Spark report already verifies the building predicate, wall helper, and ordering. This pass only maps them to current Rust read owners:

| Native fact | Rust owner | Adapter rule |
|---|---|---|
| Candidate ground and slope byte | `ResolvedTerrainCell` / resolved terrain | Use the exact wrapper semantics for cell selection; valid cell supplies level/slope to §3 and §4. |
| Old structural bit | static `bridge_facts` plus `BridgeRuntimeState` | Query old coordinate before candidate decision using §5.3. |
| Candidate structural bit | same | Query candidate coordinate independently; do not reuse old-cell result. |
| Candidate accepted building | `OccupancyGrid` plus entity/type facts | Preserve occupancy list order and the parent report's exact `WhatAmI==6`/exception predicates. |
| Candidate wall overlay | `OverlayGrid` and overlay type/rules data | Supply exact overlay ID/presence; out-of-bounds is not silently “no wall” if the ground cell itself is unavailable. |
| Gravity | `GeneralRules` after adding native integer field | Convert signed `i32` to native `f32` bits at the Spark boundary. |
| ColorSpeed | `ParticleType` after precision correction | Supply stored native `f64` bits. |

`OccupancyGrid` rebuild sorts by `(occupancy_enter_order, stable_id)` and reproduces the native prepend/append order between non-building objects and structures. The adapter must scan the resulting cell list using the already-verified building acceptance predicate; it must not replace the scan with an unordered “any building” set.

All facts must be gathered into an owned `SparkCollisionFacts` value before borrowing the mutable simulation RNG for color progression. The kernel itself consumes no RNG while gathering collision facts. Particle ticks run forward; dead-particle cleanup runs in reverse afterward. The adapter should own no state across ticks, pause, or save/load—its authorities already serialize their own state.

## 8. LaserFence stock reachability

The Spark wall helper contains a conditional LaserFence-building exception. The type byte is parsed from `LaserFence=` and the building instance contributes a connectivity/frame field. The code is active, but neither stock `rules.ini` nor `rulesmd.ini` contains an active `LaserFence=` assignment. Stock Spark behavior therefore needs ordinary wall-overlay facts only.

Do not hardcode the conditional as always true or invent a stock LaserFence object. Mod-capable LaserFence support remains outside this valid-stock slice and must not block a stock-only adapter. Public Spark dispatch remains disabled independently until its approved activation work is complete.

## 9. Exhaustion and adversarial checks

### 9.1 Zero-additional-information pass

After resolving the opening ledger, a second pass revisited all direct callees and writers for the ground wrapper, slope-table initializer/lookup, bridge structural bit, and the two rules fields. It added no new in-scope mechanism. The last newly material fact before that zero-add pass was the initialized slope-table values. A later complete initializer audit corrected the former zero-row claim and floating-point chronology; the prior zero-add pass did not establish those claims.

### 9.2 Cold spot-checks

1. **Ground cold check:** re-read `0x00578080` and `0x0047B3A0` from assembly rather than prior prose. This reconfirmed signed truncation, flattened aliasing, dummy routing, low-byte local coordinates, clamp-before-base, and the negative-level `+0.5` quirk.
2. **Bridge cold check:** independently re-read both high repair walkers and full `RecalcAttributes @ 0x0047D2B0`. This disproved the stale claim that repair recalculates bit `0x100` and confirmed the native “not restored” quirk.

### 9.3 Five adversarial questions

| Question | Answer |
|---|---|
| What if a candidate X/Y axis is negative or individually outside 0-511? | Native conversion truncates toward zero and only the flattened index is validated; current Rust reproduces valid aliases and routes misses through the shared dummy. |
| What if a collapsed high bridge is repaired by an engineer? | Its overlays/attributes are repaired, but structural `0x100` is not restored; Spark no longer applies the 416-lepton bridge plane there. |
| What if the terrain slope is 0 or 17-20? | Slope 0 returns identity. Slopes 17-20 return identity matrices while their independent ground contribution is 52. |
| What if `.13` is stored through `SimFixed` because the visual result seems close? | That changes the native `f64` input and is DRIFT; retain `0x3FC0A3D700000000`. |
| What if independently owned Cell/VXL globals are assumed numerically different? | Wrong: active runtime capture proves both level scalars are 104; Spark composes its independently owned 416 structural offset over Cell ground. |
| What if stock maps never use a discovered edge? | Trigger frequency affects priority, not parity. Every valid stock slope 0-20 and bridge lifecycle state must retain the listed mechanism. |

## 10. Final questions disposition

| ID | Status | Resolution |
|---|---|---|
| S01 | RESOLVED | Spark AI consumes one owned bundle per forward particle tick; reverse cleanup follows. |
| G01 | RESOLVED | Signed truncation toward zero by 256, then flattened 512-stride index. |
| G02 | RESOLVED | Signed level times 104, add 0.5, x87 chop. |
| G03 | RESOLVED | Low-byte X/Y affine record, clamp to `[0,max]`, add base, chop. |
| G04 | RESOLVED | All 20 records are listed in §3.3. |
| G05 | RESOLVED | Native dummy routing is known and current Rust routes Spark through the shared substrate; the former typed-unavailable gap was closed by `4c71b488`. |
| M01 | RESOLVED | `VXL_MasterLighting_Init` writes identity 0 and rotations 1-16; 17-20 are initialized to identity at755852..755875. |
| M02 | RESOLVED | Exact `f32` bits are listed in §4.3. |
| M03 | RESOLVED | Direct `slope*0x30` copy; no clamp or fallback. |
| B01 | RESOLVED | Structural slots are anchor, forward-1, forward-2, opposite. |
| B02 | RESOLVED | State-0 direction stamp clears `0x100` and sets `0x400`. |
| B03 | RESOLVED | Repair and `RecalcAttributes` do not restore `0x100`. |
| B04 | RESOLVED | Forward-3 and direction-6 extra are non-structural for this bit. |
| B05 | RESOLVED | Static structural fact AND authoritative runtime `deck_present`, with typed unavailable when runtime authority is missing. |
| R01 | RESOLVED | Signed `ReadInt`, default 3, stock 6. |
| R02 | RESOLVED | `ReadDouble`, default 0.0, `f32` parse widened to `f64`. |
| R03 | RESOLVED | Gravity `0x40C00000`; ColorSpeed `0x3FC0A3D700000000`. |
| R04 | RESOLVED | Current Rust lacks Gravity and quantizes ColorSpeed; both require changes. |
| T01 | RESOLVED | Gather owned facts, release borrows, then consume color RNG; forward ticks and reverse cleanup. |
| L01 | RESOLVED | Compiled conditional is stock-data-inert; mod-only support is outside scope. |
| E01 | RESOLVED | Adapter is pure/read-only and carries no cross-tick or save state. |

No in-scope question remains deferred. The generic shared-dummy mutation taxonomy and mod-only LaserFence system are declared non-scope rather than silently treated as resolved parity.

## 11. Function coverage ledger

| # | Address | Verified role | Directly relevant |
|---:|---:|---|---|
| 1 | `0x0062C6E0` | Spark AI consumer and rule loads | Yes |
| 2 | `0x00578080` | world coordinate to cell/dummy ground wrapper | Yes |
| 3 | `0x00565730` | matching map cell lookup conversion | Yes |
| 4 | `0x0047B3A0` | base and sloped ground height | Yes |
| 5 | `0x0047BBF0` | dummy/cell level and slope initialization | Yes |
| 6 | `0x006D6AD0` | unsigned slope-byte reader | Yes |
| 7 | `0x00754CB0` | VXL slope table master initializer | Yes |
| 8 | `0x007549E0` | VXL cell-height ratio/104 derivation | Yes |
| 9 | `0x00754A20` | corner tilt lookup initializer | Yes |
| 10 | `0x00754A50` | edge tilt lookup initializer | Yes |
| 11 | `0x007559B0` | direct 12-dword slope matrix copy | Yes |
| 12 | `0x005AE6F0` | identity → Z/X/-Z matrix builder | Yes |
| 13 | `0x005AEF60` | X rotation with retail trig lookup | Yes |
| 14 | `0x005AF1A0` | Z rotation with retail trig lookup | Yes |
| 15 | `0x004CACB0` | sine lookup used by rotation helpers | Yes |
| 16 | `0x004CAD00` | cosine lookup used by rotation helpers | Yes |
| 17 | `0x004CAD50` | tangent lookup used by VXL level initialization | Yes |
| 18 | `0x004CADE0` | atan lookup used by tilt initialization | Yes |
| 19 | `0x0047E040` | NESW structural bridge stamp/clear | Yes |
| 20 | `0x0047E470` | NWSE structural bridge stamp/clear | Yes |
| 21 | `0x00576BA0` | active high-bridge collapse state machine | Yes |
| 22 | `0x0057F440` | high bridge repair dispatcher | Yes |
| 23 | `0x005800D0` | high NS repair walker | Yes |
| 24 | `0x00580600` | high EW repair walker | Yes |
| 25 | `0x0047D2B0` | terrain attribute recalc; no structural-bit restore | Yes |
| 26 | `0x006691E0` | `Gravity` rules reader | Yes |
| 27 | `0x005276D0` | `ReadInt` | Yes |
| 28 | `0x00665650` | Rules constructor/default owner | Yes |
| 29 | `0x00644F50` | ParticleType INI reader | Yes |
| 30 | `0x005283D0` | native `ReadDouble` | Yes |
| 31 | `0x00644BE0` | ParticleType constructor/default owner | Yes |

## 12. Historical implementation handoff

This table records the 2026-07-18 implementation baseline. Its Spark
shared-dummy row is completed by `4c71b488`; later review coverage is in
`72bf8e15`, `96779c16`, and `0054549e`.

| Verified requirement | Current Rust state | Required delta | Acceptance test |
|---|---|---|---|
| Exact valid-cell coordinate selection and ground formula | Historical pre-`4c71b488`: `cell_rect` owned the substrate while Spark returned typed unavailable at the dummy boundary | Completed in `4c71b488` without changing valid flattened aliases | Retained boundary/alias fixtures plus shared-dummy level, slope, coordinate, transcript, and hash coverage |
| Exact candidate slope matrix | No Spark world table; renderer table is not authoritative for collision and treats unsupported slopes differently | Add native-derived matrix source using §4.3 bits or exact table builder; 0 and17-20 identity | Compare all 21×12 raw `f32` bits to §4.3 |
| Live structural bit | Static bridge facts and runtime `deck_present` exist separately | Query static structural AND runtime state exactly as §5.3 | Intact true; collapsed false; repaired-after-collapse remains false; forward-3/extra false |
| Candidate building/wall facts | Occupancy and overlay grids exist | Read in verified list order; preserve typed failure at unavailable terrain boundary | Multiple-object list ordering; accepted building; rejected non-building; wall overlay ID |
| Gravity width/default | Missing from `GeneralRules` | Parse signed `i32`, fallback 3, stock 6; supply native `f32` bits | Missing key gives 3; stock gives bits `0x40C00000` |
| ColorSpeed width/default | `SimFixed` via `get_f32()` | Preserve native `ReadDouble` result/`NativeF64Bits` at particle type boundary | `.13` gives bits `0x3FC0A3D700000000`; missing key gives zero |
| Borrow/RNG ordering | Pure Spark kernel exists; production dispatch disabled | Gather all facts into owned input, release world borrows, then call kernel with authoritative RNG | Fact-query failure consumes no RNG; successful tick consumes the parent-report sequence only |
| Activation safety | Spark/Railgun public dispatch/render remain disabled | Keep disabled until adapter integration and focused tests pass; no fallback path | Unsupported/unavailable input reports error and never silently activates approximation |

Historical implementation surface and integration seam:

- preserve the existing read-only `src/sim/particles/spark_world.rs` valid-cell ground/matrix/bridge/occupancy/overlay fact gathering;
- the planned routing through the existing shared real-or-dummy substrate in `src/sim/cell_rect.rs` was completed by `4c71b488`;
- retain the focused rule ownership in `src/rules/ruleset.rs` and `src/rules/particle_type.rs`;
- owner wiring only after owned facts are complete and all borrows are released;
- no change to public Spark spawn/render activation in the same patch.

## 13. Stale-document corrections

1. `VOXEL_SLOPE_TILT_SYSTEM.md`:
   - replace analytic corner/edge tilt constants with §4.2's lookup-derived values;
   - replace “entry 0 is BSS zero and never read” with “master init writes identity to entry 0; Spark directly queries it for flat candidate cells”;
   - record the complete initializer tail that sets entries17-20 to identity.
2. [BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md) lines 1477-1481:
   - replace the claim that `RecalcAttributes` re-derives `0x80/0x100/0x400` with §5.2's no-restore result.

These are corrections to prior prose, not changes to active binary evidence.

## 14. Sources

- Live read-only Ghidra MCP session on `/gamemd.exe`: `decompile_function`, `disassemble_function`, `get_xrefs_to`, and `read_memory` at the addresses enumerated in §11.
- Retail executable bytes: `<ra2-install>/gamemd.exe`, used to cold-reread PE constants, lookup-table entries, and exact matrix initialization arithmetic after the live bridge became unstable.
- Stock INIs: `ini/rules.ini`, `ini/rulesmd.ini`.
- Existing research read before and cross-checked during this pass:
  - [PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARTICLE_SPARK_COLLISION_AND_PIXEL_COMPOSITOR_GHIDRA_REPORT.md)
  - `GATE_BRIDGE_DECK_HEIGHT_RESOLUTION_GHIDRA_REPORT.md`
  - `VXL_SLOPE_MATRIX_SIGN_GHIDRA_REPORT.md`
  - `VOXEL_SLOPE_TILT_SYSTEM.md`
  - [bridges/01-assets-map-load-overlay/BRIDGE_SETBRIDGEDIRECTION_STAMPING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/BRIDGE_SETBRIDGEDIRECTION_STAMPING_GHIDRA_REPORT.md)
  - `bridges/05-damage-collapse-repair-cabhut/HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md`
  - [bridges/05-damage-collapse-repair-cabhut/REPAIRBRIDGEWALKER_BODIES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/REPAIRBRIDGEWALKER_BODIES_GHIDRA_REPORT.md)
  - [bridges/05-damage-collapse-repair-cabhut/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md)
- Current Rust owners inspected read-only: `src/map/bridge_facts.rs`, `src/map/resolved_terrain.rs`, `src/sim/bridge_state/`, `src/sim/occupancy.rs`, `src/sim/particles/spark.rs`, `src/rules/ini_value.rs`, `src/rules/particle_type.rs`, and `src/rules/ruleset.rs`.
