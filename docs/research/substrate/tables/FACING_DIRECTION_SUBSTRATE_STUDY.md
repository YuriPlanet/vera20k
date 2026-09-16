# Facing / Direction Tables — Substrate Service Study

**Date:** 2026-06-04
**Scope:** The gamemd.exe "which-way / where-next" table family — 8-direction cell-offset
table, 8-direction lepton-offset table, the drive-track turn/raw/point tables, the
DRAGON 32-way rotating-SHP frame table, the facing-quantization formula, and the
FacingClass shortest-arc turn helpers. Goal: design ONE Rust-native, pure/read-only,
deterministic substrate service for this family ("Rust-native structure, gamemd-native
semantics"), enumerate every DRIFT vs current Rust, and plan migration slices with
exact-equality acceptance tests against the gamemd dump.

**Authority:** binary → Ghidra → docs. All binary facts below cite the exact Ghidra MCP
call made during the Stage-1 decode pass (carried into this doc) or this Stage-2 pass.
Burden of proof defaults to **DRIFT**; equivalence is only downgraded with algebraic
proof, a bit-identical test across the full input space (incl. boundaries), or exhaustive
caller verification.

**Confidence legend (per claim):** `[V]` verified-from-binary this/Stage-1 session (Ghidra
call cited); `[D]` doc-sourced (cross-ref cited), not re-verified this pass; `[R]`
read-from-current-Rust this pass (file:line cited); `[U]` UNCHECKED / unknown.

**Render-side out of scope (deferred to render audit):** `g_VXL_FacingMatrices @ 0x00B45188`
caller bucketing `[U]`.

---

## (1) Active-YR responsibilities of this table family

This family answers every "which direction / where is the next cell / what sub-cell vector
/ what facing now" question in a stock YR skirmish. Player-visible outputs it drives `[V/D]`:

- **Adjacent-cell stepping** — A* neighbor expansion, bridge passability, wall auto-connect,
  ore/anim neighbor scans, tube jumps. `g_DirectionOffsets @ 0x0089F688` alone has 500+
  xrefs. (Stage-1 get_xrefs_to 0x0089F688; [DRIVE_PROCESS_MOVEMENT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_PROCESS_MOVEMENT_GHIDRA_REPORT.md) "All
  Global Data Addresses".)
- **Sub-cell (lepton) movement vectors** — EVERY locomotor's per-tick body translation:
  Drive, Walk (infantry), Hover, Ship, Tube, ObjectClass UpdatePosition. Reads
  `g_DirectionDeltaX_Table @ 0x0089F6D8` / `g_DirectionDeltaY_Table @ 0x0089F6DC`.
  (Stage-1 get_xrefs_to 0x0089F6D8/0x0089F6DC.)
- **Body/turret facing math** — `ROT=` turn-rate clamping/shift and the homing-projectile
  shortest-arc turn (FacingClass trio). (Stage-1 decompile_function 0x004C9680;
  0x005B2950/90/C0.)
- **Drive-track curve selection** (vehicles) — which of 16 raw curves / 72 turn entries a
  turning vehicle animates through, plus the post-turn facing it lands on.
  (Stage-1 read_memory 0x007E7A28 / 0x007E7B28.)
- **Render quantization** — 8-way directional muzzle-flash anim in Fire_At; 32-way
  rotating-SHP (DRAGON) frame selection. (Stage-1 disassemble Fire_At 0x006FF2D1..2FF;
  read_memory 0x007F4890.)
- **Startup init** — `Foundation_direction_table_init @ 0x0049F2F0` populates both the cell
  and lepton tables before gameplay; consumed live thereafter. (Stage-1 decompile_function
  0x0049F2F0.)

---

## (2) Full inventory (every line cited)

Static tables are **runtime-populated** — a static PE read returns zeros; the values below
are the initializer's writes, decoded from the init routine's instruction stream.

| Symbol | Address | Type | Contents / role | Cite |
|---|---|---|---|---|
| `g_DirectionOffsets` / `g_DirectionCellOffsets` | `0x0089F688` | `short[8][2]` | 8 cell-deltas, compass order N..NW | `[V]` Stage-1 decompile_function 0x0049F2F0; read_memory 0x0089F688 = zeros (runtime storage) |
| `g_DirectionDeltaX_Table` / `g_DirectionDeltaY_Table` (alias `g_DirectionLeptonOffsets`) | `0x0089F6D8` / `0x0089F6DC` | `i32[8][2]`, stride 8B | 8 lepton-deltas = cell-deltas ×256 | `[V]` Stage-1 read_memory 0x0049F330 len160 + 0x0049F3C0 len96; WRITE xref 0x0049F3A7; read_memory 0x0089F6D8 = zeros |
| `g_DriveTrackData_Array` (RawTrack) | `0x007E7A28` | 16 × 16B `{points*, chain_key+4, entry_index+8, jump_index+0xC}` | base curve metadata | `[V]` Stage-1 read_memory 0x007E7A28 len256 |
| `g_DriveTrackIndex_Table`(+0) / `g_DriveTrackDirection_Table`(+4) / `g_DriveTrackFlags_Table`(+8) | `0x007E7B28` | 72 × 12B TurnTrack `{u8 normal, u8 short, pad[2], u8 target_facing, pad[3], u8 flags, pad[3]}` | turn → curve mapping, idx = `from_dir*8 + to_dir` | `[V]` Stage-1 read_memory 0x007E7B28/0x007E7C48/0x007E7D68 864B |
| TrackPoint arrays | `0x007E64F8` (e.g. entry3) and neighbors | per-curve `{x,y,facing}` lepton points | the actual curve geometry | `[V]` Stage-1 RawTrack ptr column; `[D]` DRIVE_TRACK_TABLES doc |
| `g_ShadowDirectionLookup` / `DWORD_TABLE_007F4890` | `0x007F4890` | `i32[32]` = `(28-index)&31` | DRAGON 32-way frame map | `[V]` Stage-1 read_memory 0x007F4890 len128 |
| `g_VXL_FacingMatrices` | `0x00B45188` | VXL render matrix array | render facing matrices | `[U]` caller bucket count UNCHECKED (render) |

**Initializer (CORRECTED — two adjacent functions, not one):** `Foundation_direction_table_init
@ 0x0049F2F0` writes ONLY the **cell** table (`0x0089F688..6A4`) and `RET`s at `0x0049F39B`
(`POP ESI; POP ECX; RET`). The **lepton** table (`0x0089F6D8..714`) is initialized by a
**separate, adjacent function starting at `0x0049F3A0`** (after 4 bytes of `0x90` NOP padding at
`0x0049F39C`), whose first lepton WRITE is the `MOV [0x0089F6DC],ECX` at `0x0049F3A7` and which
`RET`s at `0x0049F40A`. The Stage-2 claim that "the lepton writes live physically in the same
routine past the recorded boundary" was **WRONG** — they are two distinct functions separated by a
`RET` + NOP pad. Both tables are still runtime-populated before gameplay; the values are unchanged.
`[V]` Adversarial re-check 2026-06-04: disassemble_function 0x0049F2F0 (shows cell writes →
RET@0x0049F39B); read_memory 0x0049F39C (NOP pad + new func bytes); read_memory 0x0049F3A0 len200
(decoded all 8 lepton imm writes: ECX/EAX = ±256/0 to 0x0089F6D8..714 → N(0,-256)…NW(-256,-256),
RET `0xC3`).

**Reader/consumer fns (all live in YR):** `[V]` Stage-1
- `MapCoord_Step_By_Direction @ 0x0042D490` — generic step; non-8 indexes cell table directly (NO mask/bounds); dir 8 = tube.
- `CheckBridgeTraversal @ 0x004D9C60` — opposite = `(dir-4)&7`.
- `WalkLocomotionClass__ProcessMovement @ 0x0075B5C4` — infantry steps by lepton table `[(dir&7)*8]`; dir 8 = tube; NO drive-track.
- `DriveLocomotionClass__Can_Use_Track @ 0x004B4B00` (vtable slot 0x007E7F54) — facing→dir `((f16>>12)+1)>>1 & 7`; track idx `to_dir + from_dir*8`.
- `DriveLocomotionClass__Process_Movement @ 0x004B2630` — same quantization; lepton reads at 0x004B32BF/333F/40C4.
- `UnitClass__Constructor @ 0x007353C0` — reads `TechnoTypeClass+0x71C` (ROT), clamp/shift twice (body+turret).
- `FUN_004C9680` (ROT-rate setter) — clamp `>0x7E→0x7F`, store `(byte)rot<<8` to `+0x14`.
- `BulletClass__HomingTrack @ 0x005B20F0` — yaw via atan2; sole caller of the FacingClass trio.
- `TechnoClass::Fire_At @ 0x006FDD50` (muzzle block 0x006FF2D1..2FF) — 8-way anim.
- `bullet_class_draw_shp_frame_facing_mapped @ 0x00468000` — DRAGON 32-way.

**FacingClass turn helpers (homing path only):** `Facing__GetTurnDelta @ 0x005B2950`,
`Facing__IsWithinROT @ 0x005B2990`, `Facing__ClampToROT @ 0x005B29C0`. `[V]` Stage-1
decompile each; get_function_callers 0x005B29C0 = ONLY BulletClass__HomingTrack.

**Cross-doc anchors:** `[D]` [SPATIAL_PRIMITIVES_LAYER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPATIAL_PRIMITIVES_LAYER_GHIDRA_REPORT.md) §8;
[DRIVE_PROCESS_MOVEMENT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_PROCESS_MOVEMENT_GHIDRA_REPORT.md) "All Global Data Addresses";
[FACING_BYTE_VS_DIRECTION_INDEX_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FACING_BYTE_VS_DIRECTION_INDEX_GHIDRA_REPORT.md) §§5,8,9.

### Verified table values (gamemd dump)

**Cell-delta table `0x0089F688` (compass order, `+X=east,+Y=south`):** `[V]` Stage-1
decompile_function 0x0049F2F0
```
idx0 N  (0,-1)   idx1 NE (1,-1)   idx2 E  (1,0)   idx3 SE (1,1)
idx4 S  (0,1)    idx5 SW (-1,1)   idx6 W  (-1,0)  idx7 NW (-1,-1)
```
**Lepton-delta table `0x0089F6D8`:** same compass order, each entry ×256. `[V]` Stage-1
raw read_memory 0x0049F330/0x0049F3C0
```
idx0 N (0,-256)  idx1 NE (256,-256)  idx2 E (256,0)  idx3 SE (256,256)
idx4 S (0,256)   idx5 SW (-256,256)  idx6 W (-256,0) idx7 NW (-256,-256)
```
**DRAGON 32-way table `0x007F4890`:** `i32[32] = [28,27,...,1,0,31,30,29]` = `(28-index)&31`.
`[V]` Stage-1 read_memory 0x007F4890 len128.

---

## (3) Active vs legacy/dormant TS split

| Item | Status | Trigger / gate | Cite |
|---|---|---|---|
| Cell table `0x0089F688` | **ACTIVE** | 500+ xrefs; A*, bridges, walls, anim, path read live | `[V/D]` |
| Lepton table `0x0089F6D8` | **ACTIVE** | every locomotor per tick | `[V]` |
| TurnTrack/RawTrack standard entries 0-63, raw 1-6 | **ACTIVE** | standard Drive locomotor every turning tick | `[V]` |
| `short_track` column + raw tracks 7-10 | **DEAD in YR** | gated on `loco+0x60`, written once (ctor), never set at runtime | `[D]` DRIVE_TRACK doc, exhaustive byte-pattern search |
| TurnTrack entries 64-71 + raw tracks 11-15 | **DORMANT-UNTIL-PROVEN** | reachable ONLY via `Force_Track` (vtable+0x70), ZERO static call sites; edge-retreat verified NOT to use it; refinery-exit `0x47` IS a live Force_Track caller | `[D]` DRIVE_TRACK doc; `[D]` CHRONO_MINER_FORCE_TRACK_0X47 / DOCK_ARRIVAL_PIVOT docs (exit path) |
| 8-way muzzle anim (Fire_At) | **ACTIVE** | gated on `WeaponType+0x104 == 8` (anim count exactly 8) | `[V]` Stage-1 disassemble |
| DRAGON 32-way table | **ACTIVE** | stock `[DRAGON] Rotates=yes`, `[AAHeatSeeker2] Image=DRAGON` | `[V]` Stage-1 |
| FacingClass trio `0x005B29xx` | **ACTIVE but NARROW** | sole caller = BulletClass HomingTrack — the homing-projectile turn path, NOT general body/turret turn | `[V]` Stage-1 get_function_callers |
| VXL facing matrices `0x00B45188` | **render; UNCHECKED** | caller bucket math deferred | `[U]` |
| Direction value 8 | **ACTIVE (tube)** | tube-jump branch in MapCoord_Step / WalkLoco / A* / Path replay; NOT a 9th compass dir. Tunnel/subterranean proper is TS-only and out of scope | `[V/D]` |

> Note on the refinery-exit Force_Track `0x47`: this is the ONE live caller into the
> "dormant" 64-71 / raw-11-15 region (entry 0x47 = 71, which selects raw track 15). So the
> region is not fully dead — but it is reached only by hardcoded undock/exit calls, never by
> the standard `from_dir*8 + to_dir` turn formula. Treat it as a special-entry path, not part
> of the general turn contract. `[D]` DOCK_ARRIVAL_PIVOT_SEQUENCE §5.2 (Force_Track 0x47 →
> raw track 15).

---

## (4) Compare vs current Rust — table-by-table, helper-by-helper

### 4.1 Cell-delta table — `DIRECTION_DELTAS`
`[R]` `src/util/direction.rs:12-21`:
```
(0,-1) (1,-1) (1,0) (1,1) (0,1) (-1,1) (-1,0) (-1,-1)
```
**Verdict: PARITY (exact).** Byte-for-byte identical to gamemd `0x0089F688` `[V]` for all 8
entries, same compass order. `Ra2Direction::facing_byte() = index*32` (`direction.rs:59-61`)
matches the `0,32,...,224` facing-byte mapping `[V]` FACING_BYTE doc §9.

### 4.2 Lepton-delta table — **MISSING in Rust (DRIFT)**
gamemd `g_DirectionDeltaX/Y_Table @ 0x0089F6D8` is the per-tick locomotor step vector =
cell-delta ×256, used as a **direct integer table lookup** `pos += lepton_table[(dir&7)]`.
`[V]` Stage-1.

Rust has **no integer lepton-delta table.** Sub-cell movement vectors are instead derived
from **float sin/cos**:
- `facing_table.rs:88 facing_to_movement(facing,speed) = (sin*speed, -cos*speed)` `[R]` —
  used by `air_movement.rs:369` and `drop_payload.rs:65`.
- The 256-entry `SIN_TABLE`/`COS_TABLE` are compile-time fixed-point from a quarter-wave
  table (`facing_table.rs:20-77`) `[R]`.

**Verdict: DRIFT.** Two distinct issues:
1. **Wrong primitive for the 8-direction step.** gamemd's locomotor cardinal/diagonal step
   is an exact integer lookup of `(±256, 0)` / `(±256, ±256)`. Rust's diagonal step via
   `facing_to_movement` would give `(sin(45°), -cos(45°))*speed ≈ (0.707·speed,
   0.707·speed)` — a *normalized* diagonal, NOT gamemd's `(±256,±256)` per-cell delta
   (which is √2 longer). For a unit moving NE one cell, gamemd advances 256 leptons on each
   axis; a sin/cos vector advances ~181 each. This is an observable speed/path difference on
   diagonals unless the consumer is only the cell-table path (see §4.7). **No algebraic or
   empirical proof of equivalence exists — DRIFT.**
2. **Float in lockstep sim.** `facing_to_movement` itself uses fixed-point tables (OK), but
   the *facing it consumes* is produced by float atan2 (§4.6), and there is no integer
   lepton-table fast path at all. The substrate must provide the exact integer table so the
   8-direction locomotor step is bit-identical to gamemd.

### 4.3 Facing → direction quantization
gamemd: `dir = ((f16>>12)+1)>>1 & 7` (16-bit) / `((f8>>4)+1)>>1 & 7` (8-bit). `[V]` Stage-1.
Rust: `direction_from_facing(f8) = (f8.wrapping_add(16)/32) & 7` `[R]` `direction.rs:81-84`.

**Verdict: PARITY (PROVEN-EQUAL).** Stage-1 proved `((f8>>4)+1)>>1 & 7 == (f8+16)/32 & 7`
bit-identical across all 256 bytes (0 mismatches), and the low byte of a 16-bit facing is
irrelevant (only bits ≥12 feed the quantization). Rust test
`facing_quantization_matches_drive_locomotor_formula` (`direction.rs:140-165`) checks the
boundary samples. **Downgraded from default-DRIFT to PARITY by the Stage-1 full-input-space
proof.** (One residual: the substrate should ALSO expose the 16-bit form `((f16>>12)+1)>>1`
directly so 16-bit FacingClass values quantize without a lossy `>>8` first.)

### 4.4 Opposite direction
gamemd: `(dir-4)&7`. `[V]` Stage-1 CheckBridgeTraversal.
Rust: `opposite_direction(d) = (d+4)&7` `[R]` `direction.rs:90-91` (only for valid 0-7,
returns None otherwise).

**Verdict: PARITY.** `(d-4)&7 == (d+4)&7` for all `d` (mod 8, since `-4 ≡ +4`). Algebraic
proof: `(d-4) mod 8 = (d+4) mod 8` because `8 | 8`. Exact for all 8 directions. (Rust's
`None` for `d>7` differs from binary's unmasked read — see §4.7 row (a).)

### 4.5 TurnTrack[72] + RawTrack[16] + TrackPoint arrays
`[R]` `src/sim/movement/drive_track.rs:194-836`. Spot-checked the Stage-1-cited values against
the Rust table:

| TurnTrack idx | gamemd `[V]` (normal/short/target/flags) | Rust `[R]` | Match |
|---|---|---|---|
| 0 | 1 / 0 / 0x00 / 0x00 | 1/0/0x00/0 (L196) | ✓ |
| 1 | 3 / 7 / 0x20 / 0x08 | 3/7/0x20/8 (L203) | ✓ |
| 18 | 1 / 0 / 0x40 / 0x03 | 1/0/0x40/3 (L322) | ✓ |
| 64 | 11 / 11 / 0xA0 / 0x00 | 11/11/0xA0/0 (L644) | ✓ |
| 71 | 15 / 15 / 0xC0 / 0x00 | 15/15/0xC0/0 (L693) | ✓ |

RawTrack metadata spot-check vs Stage-1 (`{chain_key/entry_index/jump_index}` ↔ Rust
`{chain_index/entry_index/cell_cross_index}`):

| Raw idx | gamemd `[V]` key/entry/jump | Rust `[R]` chain/entry/cross | Match |
|---|---|---|---|
| 3 | 37 / 12 / 22 | 37 / 12 / 22 (L733-739) | ✓ |
| 4 | 26 / 11 / 19 | 26 / 11 / 19 (L741-747) | ✓ |
| 5 | 45 / 15 / 31 | 45 / 15 / 31 (L749-755) | ✓ |
| 6 | 44 / 16 / 27 | 44 / 16 / 27 (L757-763) | ✓ |
| 1,2,7-15 | key=-1, jump=-1 | chain=-1, cross=-1 (L717-835) | ✓ |
| 0 | null sentinel | points_count=0, entry=192 (L709-715) | ✓ (sentinel) |

**Verdict: PARITY on the spot-checked entries (exact).** BUT — see DRIFT Ledger D5:
- the FULL 72-entry / 16-entry / ~492-point arrays were NOT exhaustively diffed against the
  gamemd dump this pass (only the 5 TurnTrack + 6 RawTrack Stage-1-cited rows). The remaining
  67 TurnTrack rows, the TrackPoint arrays (`drive_track.rs:842-3393`), and the precise
  `chain_index→raw_track_index` semantics are **UNCHECKED for exact byte-equality** and
  default to **DRIFT/UNCHECKED** until a generated equality test diffs every byte against
  `read_memory 0x007E7A28/0x007E7B28` + each TrackPoint pointer. The Rust struct field
  `cell_cross_index` is named for the `+0x0C` "jump_index" — the Stage-1 contract calls
  `+0xC` the **jump_index**; whether Rust's "cell cross" semantics exactly equal gamemd's
  jump-index semantics is `[U]` (naming overlap, not proven behavioral identity).
- `transform_track_point` (`drive_track.rs:44-62`) flag math (swap_xy / negate_x / negate_y +
  paired facing transforms) is **UNCHECKED vs the binary's Transform_Track_Coords** this
  pass — DRIFT/UNCHECKED until verified.

### 4.6 Facing-from-delta — **float atan2 (DRIFT / determinism risk)**
Rust `facing_from_delta_int(dx,dy)` and `_u16` use `f32::atan2` then quantize `[R]`
`fixed_math.rs:280-311`. Consumers: `movement/mod.rs:208`, `movement_path.rs:21`,
`movement_step.rs:33`, `movement_commands.rs:564`, `turret.rs:64`
(`facing_toward_lepton`). `[R]` grep.

gamemd derives the equivalent facing several ways depending on the path:
- **Vehicle (Drive) movement / drive-track** uses the **integer** quantization
  `((f16>>12)+1)>>1 & 7` over facings that themselves come from the integer track tables — NOT
  atan2. `[V]` Stage-1 + 2026-06-04 (Can_Use_Track 0x004B4B00 shows the quantization on a
  `(byte)g_DriveTrackDirection_Table[]<<8` facing).
- **Homing yaw** (BulletClass HomingTrack) DOES use atan2 with specific constants
  (`0x007E2820 = π/2`, `0x007E2818 ≈ -32768/π`) and `ftol`. `[V]` Stage-1.
- **CORRECTION (infantry):** gamemd's **WalkLocomotion** (infantry) DOES compute body facing via
  `Math__atan2(dest.y - cur.y, cur.x - dest.x)` → `ftol` in two branches (the sub-cell-destination
  branch and the final per-cell block), then sets facing. So the Stage-2 framing "gamemd uses
  integer table-derived facing; atan2 only for homing/DRAGON" was **OVERSTATED** — infantry walk
  facing is genuinely atan2-derived in gamemd. The DRIFT for Rust infantry facing is therefore NOT
  "Rust uses float where gamemd uses integer," it is "Rust must reproduce gamemd's EXACT atan2 +
  `ftol` form (same operand order, sign, and truncation) so the same byte results" — still DRIFT
  (unproven bit-identity), but the corrective action differs (match the atan2, don't replace it).
  `[V]` Adversarial re-check 2026-06-04: decompile_function 0x0075B5C4 (two `Math__atan2(...)` →
  `Math__ftol()` → facing-set callsites; cardinal *position* step still uses integer lepton table
  `g_DirectionDeltaX/Y_Table[(dir&7)*8]`).

**Verdict: DRIFT (two faces).**
1. For body/turret/movement facing, gamemd does NOT compute facing from a float atan2 of a
   cell/lepton delta — it reads facings from integer tables and quantizes with integer
   shifts. Rust's `f32::atan2` is a **different mechanism with no proof of bit-identical
   output**; cross-platform f32 (1 ULP) differences can land on a different u8/u16 bucket at
   a boundary. The doc comment at `fixed_math.rs:276-279` *asserts* "1 ULP maps to the same
   bucket" but provides **no proof across the input space** — this is exactly the
   "probably equivalent" downgrade CLAUDE.md forbids. **DRIFT / lockstep-determinism risk.**
2. Where gamemd genuinely uses atan2 (homing yaw, DRAGON frame), Rust must match gamemd's
   exact constant set and `ftol` rounding, not a generic `atan2/TAU*256`. **UNCHECKED**
   whether `fixed_math.rs` atan2 matches gamemd's `(atan2(-VelY,VelX)-π/2)*(-32768/π)` form
   and `ftol` truncation.

### 4.7 Invalid-direction handling
gamemd `MapCoord_Step_By_Direction`: for `dir` in 9..255, indexes the 8-entry cell table
**out of bounds with NO mask** (reads adjacent garbage). `[V]` Stage-1.
Rust `direction_delta(d)` returns `None` for `d>7`, and `delta_from_facing`/
`direction_from_facing` mask with `&7`. `[R]` `direction.rs:77-92, 81-84`.

**Verdict: DRIFT-in-principle, but lower-risk.** Rust's masking/`None` for `dir>8` differs
from the binary's unmasked OOB read. This only matters if a caller ever passes `dir>8` (other
than the tube sentinel 8). Downgrade to PARITY requires **exhaustive caller verification**
that no Rust caller passes `dir>8` to these helpers — NOT done this pass, so it stays DRIFT.
(In practice gamemd's valid callers sanitize upstream too, so the OOB read is itself a
never-hit branch — but "never hit" is not proof and the surfaces differ.)

### 4.8 FacingClass trio (shortest-arc turn)
gamemd `[V]` Stage-1: GetTurnDelta `(short)(current-target)`; IsWithinROT
`abs((short)(a-b)) <= abs(rot)`; ClampToROT snap-if-within else ±rot along shortest signed
arc, equal-distance (0x8000) resolved by `(target-current)<0`.
Rust `FacingClass` `[R]` `facing_class.rs`: `set_rot` clamp `>0x7E→0x7F`, `<<8`
(`L57-60`) — matches `[V]` 0x004C9680. `current()` interpolation uses signed-short diff
`current.wrapping_sub(prev) as i16` (`L100`) — shortest signed arc. `set()` snapshots
animated into prev (`L129`).

**Verdict: PARITY on rate clamp/shift (exact, matches 0x004C9680).** BUT — the trio is a
**turn-step primitive** (per-frame add/subtract ROT toward target), whereas Rust's
`FacingClass` is a **timer-based interpolator** (`current = current - per_step*remaining`).
These are different internal mechanisms; the OBSERVABLE per-frame facing sequence is the
parity bar. Whether the timer model reproduces gamemd's exact per-frame ClampToROT sequence
(including the equal-distance `0x8000` tiebreak and the snap-within-ROT behavior) is
**UNCHECKED for bit-identity across the arc space** this pass → **DRIFT/UNCHECKED** until a
per-frame equality test compares Rust `current(f)` against an emulated ClampToROT sequence.
Also note: gamemd's trio is **homing-only** (sole caller HomingTrack); Rust uses
`FacingClass` for general turret/body turning (`turret.rs`). Using the homing-arc helper's
contract as the general body-turn contract is itself an assumption to verify — the general
vehicle body turn is governed by the **drive-track tables**, not the trio.

### 4.9 Duplications across Rust files
- `path_smooth.rs:31 DIR_DELTAS = crate::util::direction::DIRECTION_DELTAS` `[R]` — a
  re-export alias (acceptable; single source). NOT a copy, but a second name for the same
  table; the substrate should make this the canonical accessor and drop the alias.
- `dir_to_cell_delta` (`fixed_math.rs:330-332`) is a thin forwarder to
  `direction::delta_from_facing` `[R]` — redundant indirection across modules
  (`util/fixed_math` → `util/direction`).
- `body_facing_to_turret` (`turret.rs:70-72` = `(body as u16)<<8`) and the FACING_BYTE doc's
  `facing8<<8` are the same primitive expressed twice. `[R]`

---

## (5) gamemd-native behavior contract (exact I/O a Rust replacement must reproduce)

**Canonical compass order (single source of truth for BOTH cell + lepton tables):**
`0=N,1=NE,2=E,3=SE,4=S,5=SW,6=W,7=NW`, clockwise from map-north, `+X=east,+Y=south`. `[V]`

**Cell-delta step (`MapCoord_Step_By_Direction`):** `[V]`
- `dir != 8` → `out = (x + cell[dir].dx, y + cell[dir].dy)`. NO mask, NO bounds — `dir` in
  9..255 reads OOB. A faithful substrate exposes a checked accessor for sim use AND documents
  that the gamemd helper is unchecked (callers sanitize upstream).
- `dir == 8` → tube branch: read `Cell+0x116` tube idx; `!= -1` → `out = TubeClass[idx]+0x28`
  (packed dest), else `out = packed(0,0)`. Tube idx only checked `== -1`.

**Lepton-delta step:** `pos += lepton[(dir&7)]` (X at base, Y at base+4); 256 leptons = 1
cell; cell-center = `cell*256+128`. Lepton→cell = `(v + (v>>31 & 0xFF)) >> 8` (signed /256,
round toward zero). `[V]`

**Opposite direction:** `(dir-4)&7`. `[V]`

**8↔16-bit facing:** 16-bit = `facing8<<8` (high byte authoritative). ROT-rate field =
`(byte)rot<<8` after clamp `rot>0x7E→0x7F` (max stored = `0x7F00`). `[V]`

**Facing→direction quantization:** 16-bit `((f16>>12)+1)>>1 & 7`; 8-bit `((f8>>4)+1)>>1 & 7`.
**PROVEN bit-identical** to Rust `((f+16)/32)&7` across all 256 bytes. Rounds UP at `16+32n`;
240..255 wrap → N. Low byte of a 16-bit facing irrelevant. `[V]`

**FacingClass turn (homing path):** GetTurnDelta `(short)(current-target)`; IsWithinROT
`abs((short)(a-b)) <= abs(rot)`; ClampToROT snap if within else `current ± rot` along shortest
signed arc, equal-distance via `(target-current)<0`. `[V]`

**Drive-track selection:** `track_index = next_dir + current_dir*8` (0-63); if
`TurnTrack[idx].normal_track == 0` → fallback `track_index = current_dir*9` (straight
diagonal). `target_facing` byte (0x00..0xE0 step 0x20) = post-turn facing. Flags bits 0-2 =
swap_xy/negate_x/negate_y (each with paired facing transform); bit 3 = cell-crossing gate.
Chain-key (RawTrack+4) must match in-progress chain group (groups: -1, 26, 37, 44, 45) to
chain. Special idx 64-71 (raw 11-15) NOT reachable by formula — Force_Track only. Infantry
(WalkLocomotion) does NOT use TurnTrack — pure lepton stepping + atan2 facing. `[V/D]`

**8-way muzzle anim (anim count == 8 only):** `bucket = ((f16>>12)+1)>>1 & 7`;
`anim_index = (bucket+1) & 7`. The `+1` rotation is real. `[V]`

**DRAGON 32-way frame (Rotates=yes):** `bam = ftol((atan2(-VelY,VelX)-π/2)*(-32768/π))`;
`index = (((u16)bam>>10)+1)>>1 & 0x1F`; `frame = table_007F4890[index] = (28-index)&31`.
Uses live BulletClass velocity. Constants: `0x007E2820=π/2`, `0x007E2818≈-32768/π`,
`0x007E2810` = dir-unit→radian scale. `[V]`

**Boundary summary:** dir>8 unmasked OOB; dir 8 = tube (no tube → zero); tube idx unchecked
beyond `==-1`; quantization wraps 240..255→N; ROT clamps at 0x7F; ClampToROT never takes the
long way; track fallback turns a blocked turn into a straight step. `[V]`

---

## (6) DESIGN — the Rust-native substrate service boundary

**One service:** `sim/substrate/direction_tables` — a pure, read-only, deterministic facade
over the whole "which-way / where-next" family. **Rust-native structure** (const tables +
free functions + a small typed enum), **gamemd-native semantics** (every output bit-identical
to the gamemd dump). No `render/ui/audio/net` dependency; the service depends only on
`util/fixed_math` (SimFixed/lepton helpers) and `std`. This satisfies the `sim/` layering
invariant and is consumable by the existing locomotors, turret, pathfinding, and combat.

### 6.1 Module placement & data ownership
```
src/sim/substrate/direction_tables/
  mod.rs            // re-exports + //! header (purpose + deps)
  cell.rs           // cell-delta table + stepping accessors
  lepton.rs         // lepton-delta table + sub-cell step
  quantize.rs       // facing<->direction quantization, opposite, 8<->16
  drive_track.rs    // MOVED from sim/movement: TurnTrack/RawTrack/TrackPoint + selection
  dragon.rs         // 32-way rotating-SHP frame table + index formula (sim-side data only)
```
The service **OWNS** the const tables (the single source of truth). `util/direction.rs`'s
`DIRECTION_DELTAS`, `util/facing_table.rs`'s movement vectors, the scattered
`direction_from_facing`/`opposite_direction`/`dir_to_cell_delta`, and
`sim/movement/drive_track.rs`'s tables all collapse into this service (retire list §7).

> Layering note: this service lives under `sim/` (not `util/`) because it carries
> game-behavior tables (drive tracks, DRAGON frames) that are sim-authoritative. The pure
> arithmetic helpers (quantize, opposite, 8↔16) could also live in `util/`; placing them in
> the sim substrate keeps ONE home and avoids the current `util↔util` cross-forwarding.
> Pathfinding/locomotor/turret import from the substrate.

### 6.2 Construction source
- Cell + lepton tables: **const tables embedded from the gamemd dump** (the §5 values). NOT
  INI-parsed, NOT map-derived — they are engine constants. The lepton table is literally the
  cell table ×256, so it can be `const`-derived from the cell table at compile time (proven
  identity, so no drift risk) OR embedded verbatim and asserted equal in a test.
- TurnTrack/RawTrack/TrackPoint: **const tables embedded from the gamemd dump** (already in
  Rust; moved, not regenerated).
- DRAGON table: **const** `[i32;32]`, or `const`-derived as `(28-i)&31` with a test asserting
  it equals the dumped `0x007F4890` bytes.
- Quantization/opposite/clamp: **pure functions**, no data.

### 6.3 API surface (signatures)
```rust
// Canonical direction enum (re-home Ra2Direction here or keep in util and re-export).
pub enum Direction8 { N, NE, E, SE, S, SW, W, NW } // 0..7

// --- cell.rs ---
/// gamemd cell-delta. PANIC-free checked variant for sim callers.
pub fn cell_delta(dir: u8) -> Option<(i32, i32)>;          // None for dir>7
/// Faithful unchecked gamemd helper (documents the OOB-read contract);
/// debug-asserts dir<=7. Use only when mirroring MapCoord_Step exactly.
pub fn cell_delta_unchecked(dir: u8) -> (i32, i32);
pub const CELL_DELTAS: [(i32, i32); 8];

// --- lepton.rs ---
/// gamemd per-tick locomotor step vector (= cell ×256). The integer table
/// gamemd actually uses — NOT sin/cos.
pub fn lepton_delta(dir: u8) -> Option<(i32, i32)>;        // None for dir>7
pub const LEPTON_DELTAS: [(i32, i32); 8];                  // const-derived ×256
/// Signed lepton/256 toward zero, matching (v + (v>>31 & 0xFF)) >> 8.
pub fn lepton_to_cell(v: i32) -> i32;

// --- quantize.rs ---
pub fn dir_from_facing8(f: u8) -> u8;                       // ((f>>4)+1)>>1 & 7
pub fn dir_from_facing16(f: u16) -> u8;                     // ((f>>12)+1)>>1 & 7
pub fn facing8_to_16(f: u8) -> u16;                         // (f as u16)<<8
pub fn opposite_dir(dir: u8) -> u8;                         // (dir-4)&7
/// 8-way muzzle-anim index: (dir_from_facing16(f)+1)&7.
pub fn muzzle_anim_index_8way(f16: u16) -> u8;

// --- drive_track.rs (moved) ---
pub const TURN_TRACKS: [TurnTrack; 72];
pub const RAW_TRACKS:  [RawTrack; 16];
pub fn select_turn_track(from_dir: u8, to_dir: u8) -> u8;   // to_dir + from_dir*8, w/ fallback
pub fn transform_track_point(x: i16, y: i16, facing: u8, flags: u8) -> (i16, i16, u8);

// --- dragon.rs ---
pub const DRAGON_FRAME_TABLE: [i32; 32];                    // (28-i)&31
pub fn dragon_frame_index(bam: u16) -> usize;               // (((bam)>>10)+1)>>1 & 0x1F
```

### 6.4 Determinism guarantees
- **No float anywhere in the integer-table paths.** Cell/lepton stepping, quantization,
  opposite, drive-track selection, muzzle bucket, and DRAGON index are pure integer.
- The float atan2 facing (`facing_from_delta_int*`) is **removed from the movement/turret
  hot path** in favor of integer table stepping; any genuinely-atan2 gamemd path (homing
  yaw, DRAGON bam) is isolated into a single clearly-marked function that reproduces gamemd's
  exact `(atan2(-VelY,VelX)-π/2)*(-32768/π)` + `ftol` form, kept out of the general facing
  API (so callers can't accidentally introduce float facing into lockstep movement).
- All tables `const`; the service is stateless (no `&mut self`, no interior mutability), so it
  is trivially reentrant and replay-safe.

---

## (7) Retire list (every ad hoc / duplicated / approximated Rust table or helper)

| Rust file:line | Item | Disposition |
|---|---|---|
| `src/util/direction.rs:12-21` | `DIRECTION_DELTAS` | MOVE → `substrate/direction_tables/cell.rs::CELL_DELTAS` (canonical) |
| `src/util/direction.rs:81-92` | `direction_from_facing`, `delta_from_facing`, `opposite_direction` | MOVE → `quantize.rs` (`dir_from_facing8`, cell accessor, `opposite_dir`) |
| `src/util/direction.rs:94-96` | `is_tube_step_direction` / `TUBE_STEP_DIRECTION` | MOVE → substrate (tube sentinel constant) |
| `src/util/facing_table.rs:20-77` | `QUARTER_SIN`, `SIN_TABLE`, `COS_TABLE` | KEEP for genuine trig consumers (aircraft/payload), but REMOVE as the source of 8-direction *step vectors*; replace those callers with `lepton_delta` |
| `src/util/facing_table.rs:88-92` | `facing_to_movement` | NARROW: keep only for non-cardinal continuous-heading movers (aircraft); DROP for cardinal/diagonal cell steps |
| `src/util/fixed_math.rs:280-311` | `facing_from_delta_int`, `_u16` (float atan2) | DRIFT — split: remove from movement/turret facing (replace with integer quantization of table-derived facing); keep ONLY an isolated gamemd-exact atan2 for homing/DRAGON |
| `src/util/fixed_math.rs:330-332` | `dir_to_cell_delta` (forwarder) | DELETE — redundant `util→util` indirection; callers use `substrate::cell_delta` |
| `src/sim/movement/drive_track.rs:194-836` | `TURN_TRACKS`, `RAW_TRACKS` | MOVE → `substrate/direction_tables/drive_track.rs` |
| `src/sim/movement/drive_track.rs:842-3393` | TrackPoint arrays | MOVE → substrate (after full byte-equality verification, §8 slice 5) |
| `src/sim/movement/drive_track.rs:44-62` | `transform_track_point` | MOVE → substrate; verify flag math vs binary (§8 slice 5) |
| `src/sim/movement/turret.rs:70-72` | `body_facing_to_turret` (`<<8`) | MOVE → `quantize.rs::facing8_to_16` (single primitive) |
| `src/sim/pathfinding/path_smooth.rs:31` | `DIR_DELTAS` alias | DELETE alias; import `substrate::CELL_DELTAS` directly |
| (no Rust yet) | lepton-delta table `0x0089F6D8` | ADD — `LEPTON_DELTAS` (the MISSING gamemd table) |
| (no Rust yet) | DRAGON 32-way table `0x007F4890` | ADD — `DRAGON_FRAME_TABLE` (currently `app_fire_effects` uses a wrong cell-delta formula per FACING_BYTE doc §9) |

---

## (8) Migration slices + acceptance tests

Each slice is independently shippable. **Pure-data-parity** slices add the substrate
table + an exact-equality test and re-point callers; **stateful** slices (S8) touch
per-entity timer/track state. Map to the substrate program convention: foundational
helper-service slices first, behavior re-pointing after.

**S1 — Cell-delta table (pure data).** Add `substrate/direction_tables/cell.rs` with
`CELL_DELTAS` + checked/unchecked accessors; re-export from `util::direction`.
- Test `cell_delta_table_equals_gamemd_dump`: assert all 8 entries == the §5 cell table
  (N(0,-1)…NW(-1,-1)). Input space: all `dir` 0..7 plus 8 (tube sentinel → None) plus
  255 (→ None for checked).
- Test `opposite_dir_matches_subtract4`: for all `dir` 0..7, `opposite_dir(dir) ==
  (dir.wrapping_sub(4))&7` AND `== (dir+4)&7`.

**S2 — Lepton-delta table (pure data, NEW).** Add `LEPTON_DELTAS` const-derived as
`CELL_DELTAS[i] * 256`.
- Test `lepton_delta_table_equals_gamemd_dump`: all 8 entries == §5 lepton table
  (N(0,-256)…NW(-256,-256)). Boundary: each entry exactly ±256/0, never ±181 (the sin/cos
  diagonal) — explicitly assert `lepton_delta(1) == (256,-256)`.
- Test `lepton_delta_is_cell_times_256`: for all 8, `lepton == (cell.0*256, cell.1*256)`.

**S3 — Quantization + 8↔16 (pure fn).** Move `dir_from_facing8/16`, `facing8_to_16`.
- Test `dir_from_facing8_full_input_space`: for ALL 256 bytes, `dir_from_facing8(f) ==
  ((f>>4)+1)>>1 & 7` AND `== (f.wrapping_add(16)/32)&7` (re-prove the Stage-1 equivalence in
  CI). Boundaries: 15→0, 16→1, 47→1, 240→0, 255→0.
- Test `dir_from_facing16_ignores_low_byte`: for all 256 high bytes × sampled low bytes,
  `dir_from_facing16((hi<<8)|lo) == dir_from_facing8(hi)` (proves low byte irrelevant).

**S4 — Muzzle-anim + DRAGON (pure data + fn).** Add `DRAGON_FRAME_TABLE`,
`muzzle_anim_index_8way`, `dragon_frame_index`.
- Test `dragon_frame_table_equals_gamemd_dump`: all 32 == `(28-i)&31` == the dumped
  `0x007F4890` sequence `[28,27,…,0,31,30,29]`.
- Test `muzzle_anim_8way_rotated_bucket`: for sampled 16-bit facings,
  `muzzle_anim_index_8way(f) == (dir_from_facing16(f)+1)&7`; assert the `+1` rotation
  (e.g. f=0x0000 → bucket 0 → anim 1).
- Test `dragon_frame_index_formula`: `(((bam)>>10)+1)>>1 & 0x1F` over boundary bam values.

**S5 — Drive-track tables (pure data, full byte-equality).** MOVE TurnTrack/RawTrack/
TrackPoint + `transform_track_point` into the substrate. THIS is the slice that closes the
§4.5 UNCHECKED gap.
- Test `turn_track_table_equals_gamemd_dump`: ALL 72 entries' `{normal,short,target,flags}`
  == bytes from `read_memory 0x007E7B28/0x007E7C48/0x007E7D68` (generate the expected array
  from the dump, not from the Rust source, to catch self-consistent drift).
- Test `raw_track_table_equals_gamemd_dump`: ALL 16 entries' `{points_count, entry_index,
  chain_key, jump_index}` == `read_memory 0x007E7A28` (resolve each TrackPoint pointer; verify
  `points_count` from the sentinel terminator).
- Test `track_point_arrays_equal_gamemd_dump`: every `{x,y,facing}` of all ~492 points ==
  the dumped point arrays (per RawTrack pointer). Input space: every point of every used raw
  track (1-6, 11-15).
- Test `select_turn_track_indexing`: `select_turn_track(from,to) == to + from*8` for all
  64 standard pairs; fallback `normal_track==0 → from*9` for the zero entries.
- Test `transform_track_point_flag_math`: for each flag bit (1/2/4) and combinations, output
  `(tx,ty,tf)` == the binary Transform_Track_Coords result (verify against a Ghidra emulation
  of the transform — UNCHECKED until done; this test BLOCKS the slice).

**S6 — Re-point pathfinding/locomotor cell stepping.** Replace `path_smooth.rs:31` alias
and any `direction_delta` callers with `substrate::cell_delta`; delete the alias and
`dir_to_cell_delta` forwarder.
- Test `path_neighbor_steps_use_substrate_cell_table`: A* neighbor expansion for a fixture
  cell produces the same 8 neighbor coords as `CELL_DELTAS`. (Cross-family: coordinate with
  the path-neighbor lane — shared `CELL_DELTAS`.)

**S7 — Re-point locomotor sub-cell step to lepton table.** Replace the sin/cos-derived
cardinal/diagonal step in the ground locomotors with `lepton_delta` for the 8-direction
case; keep `facing_to_movement` only for aircraft/continuous heading.
- Test `cardinal_step_advances_256_leptons`: a unit stepping E advances exactly +256
  leptons X / 0 Y per full cell; NE advances +256/-256 (NOT +181/-181).
- Test `lockstep_no_float_in_ground_step`: (review-level) ground-step path contains no
  `f32`/atan2; integer-only.

**S8 — FacingClass / turret turn parity (STATEFUL).** This is the only genuinely stateful
slice. Verify the timer-interpolator `FacingClass` reproduces the gamemd per-frame
ClampToROT sequence, OR (if it cannot) replace `current()` with a gamemd-faithful per-frame
add/subtract-ROT step for the body/turret path. Keep `set_rot` clamp/shift (already exact).
- Test `clamp_to_rot_per_frame_matches_gamemd`: for sampled `(current,target,rot)` incl.
  boundaries (equal-distance 0x8000 apart, within-ROT snap, wrap across 0), the per-frame
  facing sequence == an emulated ClampToROT sequence (snap-if-within; else ±rot shortest arc;
  equal-distance via `(target-current)<0`).
- Test `set_rot_clamp_shift_exact`: `set_rot(r)` stores `min(r,0x7F)<<8` (already passing,
  keep). Boundary: 0x7E→0x7E00, 0x7F→0x7F00, 0xFF→0x7F00.
- Test `homing_facing_isolated_from_general_turn`: (review-level) the atan2 yaw path is the
  ONLY caller of the float-facing helper; general turret/body turning uses integer
  quantization + tables.

**S9 — Remove float `facing_from_delta_int*` from movement/turret.** After S6/S7/S8,
delete or quarantine `facing_from_delta_int`/`_u16` to a single homing/DRAGON-only function
with gamemd-exact constants + `ftol`.
- Test `homing_yaw_matches_gamemd_atan2_form`: `ftol((atan2(-VelY,VelX)-π/2)*(-32768/π))`
  over sampled velocities == gamemd's bam (verify the constant set 0x007E2820/2818/2810 and
  truncation). UNCHECKED until the constants are wired — this test gates the slice.

---

## Anchors & Evidence

| Address | Ghidra call cited | Doc cross-ref |
|---|---|---|
| `0x0089F688` cell table | `[V]` decompile_function 0x0049F2F0; read_memory 0x0089F688 | SPATIAL_PRIMITIVES §8; DRIVE_PROCESS_MOVEMENT globals; GDIRECTIONOFFSETS_0089F688 |
| `0x0089F6D8/6DC` lepton table | `[V]` read_memory 0x0049F330/0x0049F3C0; WRITE xref 0x0049F3A7 | DRIVE_PROCESS_MOVEMENT globals; SPATIAL_PRIMITIVES §8 |
| `0x0049F2F0` init | `[V]` decompile_function 0x0049F2F0 | FACING_BYTE §5,9 |
| `0x007E7A28` RawTrack | `[V]` read_memory 0x007E7A28 len256 | DRIVE_TRACK_TABLES; DRIVE_APPLY_TRACK_DELTA |
| `0x007E7B28` TurnTrack | `[V]` read_memory 0x007E7B28/0x007E7C48/0x007E7D68 | DRIVE_TRACK_TABLES |
| `0x007F4890` DRAGON | `[V]` read_memory 0x007F4890 len128 | DRAGON_BULLET_DRAW_SLOT_FRAME_MAPPING; FACING_BYTE §8 OQ-5 |
| `0x0042D490` MapCoord_Step | `[V]` decompile_function 0x0042D490 | — |
| `0x004D9C60` CheckBridgeTraversal | `[V]` decompile_function 0x004D9C60 | — |
| `0x0075B5C4` WalkLoco ProcessMovement | `[V]` decompile_function 0x0075B5C4 | — |
| `0x004B4B00` Can_Use_Track (vtable 0x007E7F54) | `[V]` decompile_function 0x004B4B00 | DRIVE_PROCESS_MOVEMENT |
| `0x004B2630` Drive Process_Movement | `[V]` decompile_function 0x004B2630 | DRIVE_PROCESS_MOVEMENT |
| `0x004C9680` ROT-rate setter | `[V]` decompile_function 0x004C9680 | FACING_BYTE §9 OQ-2 |
| `0x005B20F0` HomingTrack | `[V]` disassemble_function 0x005B20F0 | FACING_BYTE §5 |
| `0x005B2950/90/C0` FacingClass trio | `[V]` decompile each; get_function_callers 0x005B29C0 | FACING_BYTE §8 OQ-3 |
| `0x006FDD50` Fire_At (0x006FF2D1..2FF) | `[V]` disassemble Fire_At muzzle block | FACING_BYTE §8 OQ-4 |
| `0x00468000` DRAGON draw | `[V]` (per prior report; table dumped) | DRAGON_BULLET_DRAW_SLOT_FRAME_MAPPING |
| `0x007E2820/2818/2810` yaw constants | `[V]` read_memory (Stage-1) | FACING_BYTE §8 OQ-5 |
| `0x00B45188` VXL matrices | `[U]` caller bucketing UNCHECKED | FACING_BYTE §9 (deferred OQ-9) |

---

## DRIFT Ledger

| # | Rust file:line | Current | gamemd-correct | Severity (+ trigger frequency) |
|---|---|---|---|---|
| D1 | (missing) — no lepton-delta table; `facing_table.rs:88` `facing_to_movement` (sin/cos) used for steps | diagonal step ≈ `(0.707·speed, 0.707·speed)` normalized | exact integer `lepton[dir] = (±256,0)/(±256,±256)` per cell | **HIGH** — fires every tick any ground unit moves diagonally (constant in normal play); √2 speed/path difference on diagonals |
| D2 | `fixed_math.rs:280-311` `facing_from_delta_int`/`_u16` (f32 atan2) feeding `movement/mod.rs:208`, `movement_path.rs:21`, `movement_step.rs:33`, `turret.rs:64`, `movement_commands.rs:564` | float atan2 → quantize; "1 ULP same bucket" asserted, unproven | **Vehicle/drive-track:** integer table-derived facing + integer quantization (NOT atan2). **Infantry (WalkLoco):** gamemd ITSELF uses `atan2`+`ftol` — Rust must match the EXACT atan2/operand-order/`ftol` form, not replace it. atan2 only for homing/DRAGON/infantry. `[V]` 2026-06-04 corrected (see §4.6) | **HIGH** — facing is derived every move/turret tick; float in lockstep sim is a cross-platform desync risk (fires whenever a boundary case lands differently) |
| D3 | `app_fire_effects.rs` DRAGON frame (per FACING_BYTE §9 "mismatch") + missing `0x007F4890` table | origin→target cell-delta formula | `(28-index)&31` lookup over `bam = ftol((atan2(-VelY,VelX)-π/2)*(-32768/π))`, `index=(((u16)bam>>10)+1)>>1&0x1F` | **MEDIUM** — fires whenever a `Rotates=yes`/DRAGON projectile (e.g. Aegis AA missile) is in flight; visible wrong projectile sprite frame |
| D4 | `direction.rs:77-92` checked/`&7`-masked accessors | `None`/`&7`-wrap for `dir>8` | gamemd indexes unmasked (OOB read) — different surface | **LOW** — only differs if a caller passes `dir>8`; not proven impossible, so DRIFT, but not observed to fire in normal play (callers sanitize) |
| D5 | `drive_track.rs:194-836` (67 unspot-checked TurnTrack rows), `:842-3393` (TrackPoint arrays), `:44-62` `transform_track_point` | extracted, only 5 TurnTrack + 6 RawTrack rows re-verified this pass | full byte-equality vs `0x007E7A28/0x007E7B28` + TrackPoint pointers; flag-transform math vs Transform_Track_Coords | **MEDIUM/UNCHECKED** — drives every vehicle turn (constant in play); spot-checks pass but full-table + transform equality is unproven |
| D6 | `facing_class.rs:85-116` timer interpolator used for general body/turret turn (`turret.rs`) | timer model `current - per_step*remaining` | gamemd general body turn = drive-track tables; trio is homing-only; per-frame ClampToROT sequence unproven-equal | **MEDIUM/UNCHECKED** — turret tracking is visible every combat tick; observable per-frame facing sequence not proven bit-identical |
| D7 | `fixed_math.rs:330-332` `dir_to_cell_delta`; `path_smooth.rs:31` `DIR_DELTAS` alias; `turret.rs:70-72` `body_facing_to_turret` | scattered forwarders/aliases of one primitive | single substrate source | **LOW** — no behavior change; maintenance/duplication only (does not fire as a player-visible drift) |

---

## Cross-family hooks for the synthesis stage to reconcile

- **`CELL_DELTAS` is shared with the path-neighbor / pathfinding lane** (`path_smooth.rs:31`
  alias, A* neighbor expansion). The substrate must be the single owner; the path lane must
  import from it, not keep its own `NEIGHBORS`/`DIR_DELTAS`.
- **The facing-quantization formula is shared with the combat lane** (Fire_At muzzle 8-way)
  and the **render lane** (DRAGON 32-way, and the deferred VXL bucket). Synthesis must ensure
  one quantization implementation feeds all three, and must resolve the VXL caller-bucket
  `[U]` before the render atlas claims parity.
- **The lepton-delta table is shared with every locomotor lane** (Drive/Walk/Hover/Ship/Tube/
  ObjectClass). Whichever lane owns locomotor stepping must consume `LEPTON_DELTAS`, not
  sin/cos, for the 8-direction case — coordinate so the substrate lands before locomotor
  re-pointing.
- **`facing_to_movement` (sin/cos) stays for aircraft/continuous-heading movers** — synthesis
  must NOT delete it wholesale; it is the correct primitive for non-cardinal headings, and
  only the cardinal/diagonal *cell-step* callers move to `LEPTON_DELTAS`.
- **Drive-track tables overlap the miner/dock lane** (Force_Track `0x47` refinery exit uses
  TurnTrack entry 71 / raw track 15). The substrate owns the tables; the miner-exit lane
  keeps the Force_Track *invocation* logic but reads track data from the substrate.

---

## Verification Log (adversarial re-check, 2026-06-04)

All checks below were re-run LIVE against gamemd.exe this session (read-only Ghidra MCP). Default
verdict was DRIFT/UNVERIFIED; a claim is marked VERIFIED only on bytes/decompile read this pass.

| # | Claim | Verdict | Evidence (this-session Ghidra call) |
|---|---|---|---|
| 1 | Cell-delta table `0x0089F688` = N(0,-1)…NW(-1,-1), compass order, runtime-populated | **VERIFIED** | disassemble_function 0x0049F2F0 → 8 imm writes 0x0089F688..6A4 decoded as packed i16 pairs = N(0,-1),NE(1,-1),E(1,0),SE(1,1),S(0,1),SW(-1,1),W(-1,0),NW(-1,-1) |
| 2 | Lepton table `0x0089F6D8` = cell ×256, stride 8B, N(0,-256)…NW(-256,-256) | **VERIFIED** | read_memory 0x0049F3A0 len200 → all 8 imm writes (ECX/EAX = 0/±256) to 0x0089F6D8..714 decoded; FUN_0049f550 confirms `g_DirectionDeltaX_Table + dir*8` (X@base, Y@base+4) |
| 3 | Init: `Foundation_direction_table_init @ 0x0049F2F0` writes BOTH tables, lepton "past recorded boundary" of same routine | **WRONG → corrected** | 0x0049F2F0 RETs at 0x0049F39B (cell only); read_memory 0x0049F39C shows 4× `0x90` NOP pad + a NEW function at 0x0049F3A0 (first lepton WRITE 0x0049F3A7, RET 0xC3 at 0x0049F40A). Two adjacent functions, not one. Corrected inline at "Initializer (CORRECTED…)". Values unchanged. |
| 4 | DRAGON 32-way table `0x007F4890` = `(28-i)&31` = `[28,27,…,1,0,31,30,29]` | **VERIFIED** | read_memory 0x007F4890 len128 → i32[32] = 28,27,…,1,0,31,30,29 exactly |
| 5 | Facing→dir quantization `((f16>>12)+1)>>1 & 7` | **VERIFIED** | decompile_function 0x004B4B00 (Can_Use_Track): `((param_1 >> 0xc) + 1 >> 1 & 7)` on a `(byte)g_DriveTrackDirection_Table[]<<8` facing |
| 6 | Opposite / step direction `(dir-4)&7`, cell table stride 4B (i16 pairs) | **VERIFIED** | decompile_function 0x004D9C60 (CheckBridgeTraversal): `uVar1 = param_2 - 4U & 7`; reads `&g_DirectionOffsets + uVar1*4` (+0 X, +2 Y) |
| 7 | MapCoord_Step OOB contract: dir≠8 indexes cell table unmasked/unbounded; dir=8 tube (idx checked only `==-1`, dest = TubeArray[idx]+0x28, else 0) | **VERIFIED** | decompile_function 0x0042D490: no mask/bounds on `g_DirectionOffsets[param_3]`; tube branch checks `(*(short*)(cell+0x116) != -1)` → `*(TubeArray[idx*4]+0x28)` else 0 |
| 8 | FacingClass trio contract (GetTurnDelta `current-target`; IsWithinROT `abs(a-b)<=abs(rot)`; ClampToROT snap-if-within else ±rot shortest arc, equal-dist via `(target-current)<0`) | **VERIFIED** | decompile_function 0x005B2950 (`*p2 = *p1 - *p3`); 0x005B2990 (`abs(short)(*p1-*p2) <= abs(*p3)`); 0x005B29C0 (`abs(cur-tgt)<=abs(rot)`→snap; else `(tgt-cur)<0 ? cur-rot : cur+rot`) |
| 9 | FacingClass trio is HOMING-ONLY (sole caller HomingTrack) | **VERIFIED** | get_function_callers 0x005B29C0 → only `BulletClass__HomingTrack @ 0x005B20F0` |
| 10 | ROT-rate setter `>0x7E→0x7F`, store `(byte)rot<<8` to `+0x14` | **VERIFIED** | decompile_function 0x004C9680: `if (0x7e < param_2) param_2 = 0x7f; *(ushort*)(p1+0x14) = (byte)param_2 << 8` |
| 11 | TurnTrack `0x007E7B28` (12B stride) spot rows: 0=1/0/0x00/0, 1=3/7/0x20/8, 18=1/0/0x40/3, 64=11/11/0xA0/0, 71=15/15/0xC0/0 | **VERIFIED** | read_memory 0x007E7B28, 0x007E7C00 (idx18), 0x007E7E28 (idx64), 0x007E7E7C (idx71) — all 5 byte-exact |
| 12 | RawTrack `0x007E7A28` (16B stride) spot rows: 0=sentinel/entry192, 1-2=key/jump=-1, 3=37/12/22, 4=26/11/19, 5=45/15/31 | **VERIFIED** | read_memory 0x007E7A28 len96 — all 6 byte-exact; entry3 TrackPoint ptr = 0x007E64F8 (matches doc) |
| 13 | Cell table "500+ xrefs", consumed live (A*, bridges, walls, anim, locomotors, AI) | **VERIFIED (approx count)** | get_xrefs_to 0x0089F688 limit 600 → hundreds of READ+DATA refs across Path_*, MapClass bridge/ramp fns, BuildingClass wall fns, *Locomotion*, HouseClass AI, etc. "500+" is order-of-magnitude correct, not an exact test target |
| 14 | "gamemd uses integer table-derived facing; atan2 only for homing/DRAGON" (D2/§4.6/§5) | **WRONG (overstated) → corrected** | decompile_function 0x0075B5C4 (WalkLocomotion): infantry body facing set via `Math__atan2(dest.y-cur.y, cur.x-dest.x)` → `Math__ftol()` in 2 branches. Position step still uses integer lepton table `g_DirectionDeltaX/Y_Table[(dir&7)*8]`. Corrected §4.6 + D2: infantry facing is genuinely atan2 in gamemd; Rust must MATCH the atan2, not replace it. |
| 15 | Infantry (WalkLoco) does NOT use TurnTrack — pure lepton position stepping (§5) | **VERIFIED** | decompile_function 0x0075B5C4: no `g_DriveTrackIndex_Table`/TurnTrack read; cardinal step is `g_DirectionDeltaX/Y_Table[(dir&7)*8]` |
| 16 | 8-way muzzle `+1` rotation (`anim=(bucket+1)&7`); DRAGON index `(((bam)>>10)+1)>>1 & 0x1F`; homing yaw constants `0x007E2820/2818/2810` | **UNVERIFIABLE this pass** | Fire_At (0x006FDD50) disassembly exceeds tool token limit; not re-disassembled. DRAGON *table* bytes ARE verified (#4). These formula/constant claims remain Stage-1-sourced `[V]` (not re-proven 2026-06-04). |
| 17 | Full 72-row TurnTrack / 16-row RawTrack / ~492 TrackPoints / `transform_track_point` flag math byte-equality | **UNVERIFIABLE this pass (matches doc's own D5)** | Only the 11 spot rows (#11,#12) re-checked; the remaining rows + all TrackPoint arrays + Transform_Track_Coords not exhaustively diffed. Doc already marks these DRIFT/UNCHECKED (D5) — verdict stands. |

### Summary of corrections
- **#3 (init attribution)** and **#14 (infantry facing mechanism)** were WRONG and are corrected
  inline. Neither changes a TABLE VALUE; both correct the *narrative/mechanism* and #14 changes the
  prescribed FIX.
- All table values, the quantization/opposite formulas, the FacingClass-trio contract + homing-only
  scope, the MapCoord_Step OOB/tube contract, and the ROT setter are VERIFIED byte/decompile-exact.

### Effect on Stage-2 recommendations (for synthesis to down-weight)
- **#14 invalidates part of D2 / Retire-list row `fixed_math.rs:280-311` / Slice S9's premise.**
  The recommendation "remove the float `facing_from_delta_int*` from the movement/turret path and
  replace with integer quantization" is correct for **vehicle** facing but WRONG for **infantry**:
  gamemd's WalkLocomotion computes infantry facing with `atan2`+`ftol` itself. S9 must therefore
  KEEP an atan2 facing path for infantry (and homing/DRAGON), matching gamemd's exact
  operand-order/`ftol`, rather than eliminating atan2 from all movement. The S9 acceptance test
  `homing_yaw_matches_gamemd_atan2_form` should be extended to cover the infantry-walk atan2 form.
- **#3 corrects the init narrative only** — no slice depends on the two tables being written by one
  vs two functions; S1/S2's const-embed plan is unaffected (the dumped values are confirmed).
- No correction invalidates S1-S5's pure-data const-embed plan or the verified formula slices.
