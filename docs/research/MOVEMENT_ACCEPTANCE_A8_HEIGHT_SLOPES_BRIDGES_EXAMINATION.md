# A8 examination: height, slopes and bridges

Criterion A8 of [the movement retail acceptance ledger](../plans/2026-09-15-movement-retail-acceptance.md):
*"Ground/deck height, ramp tilt, slope transitions; under-span and over-span routing."*

Examined 2026-09-16 against `gamemd.exe` in Ghidra project `testProsjekt`
(image base `0x00400000`). Every native address below was read this session from
the binary — disassembly where operand order or control flow is load-bearing,
decompilation only for shape. Ghidra labels were treated as leads: the two
vtable slots used here are proved by base + offset arithmetic against the
slot addresses in `get_xrefs_to`, not by their names.

Evidence words used exactly as the contract defines them:

- **Native-established** — read from the body / callers / retail data in this session.
- **Rust-read** — read from the working tree in this session.
- **Parity-demonstrated** — a saved native-execution artifact backs it, within
  that artifact's own stated coverage.

No Cargo suite was run for this examination (it is a read, and the A8 retail
tests are `#[ignore]`d and need a retail install). Nothing outside this file
was edited.

---

## 1. Verdict

**NOT ACCEPTED.** Seven divergences, of which one (D1) changes a mover's speed
on retail ground, one (D3) is the same defect the A3 examination recorded as its
D7, and one (D4) is a substrate split that also blinds A8's own retail
instrument.

The core of A8 is in better shape than A3, A5 or A6: the *height producer* — the
per-track-point ground/deck commit, its cadence, the ground evaluator, the
on/off-bridge predicate and the height-legality gate — is a faithful port, and
part of it is parity-demonstrated by an executed native oracle. What is wrong is
around the edges: which *input* decides a slope's direction, what the brake band
measures, and a second height field VERA carries that gamemd does not have.

---

## 2. The native model, as read

### 2.1 Who writes a ground mover's Z, and when

`DriveLocomotionClass::Process_Drive_Track 0x004B0F20` never stores to
`Object+0xA4` by literal offset (`search_instructions`, operand `0xa4`, 1655
instructions scanned: zero matches); neither does
`DriveLocomotionClass::Process_Movement 0x004B2630` (2600 scanned, zero).
The coordinate write is `[vtable+0x1B4]` → `TechnoClass::Set_Coords_With_Cloak
0x004DB810`, which copies X/Y/Z **verbatim** (its body is a raw-coords copy
bracketed by Mark REMOVE/PUT). The per-point coordinate it is handed carries the
mover's **previous** Z unchanged.

The height is then written by the next call:

```
004b17cc  CALL [EAX+0x124]         ; Mark(REMOVE)
004b17dc  CALL [EDX+0x1b4]         ; Set_Coords (raw copy, Z carried)
   ... bridge-flag update, crush list, Mark(PUT) ...
004b1a7a  MOV  CL,[EAX+0x90]       ; IsOnMap gate
004b1a88  MOV  BL,[EAX+0x74]       ; save IsMarked
004b1a8b  MOV  byte [EAX+0x74],0   ; clear it
004b1a92  PUSH 0
004b1a96  CALL [EDX+0x1cc]         ; FootClass::Set_Height_On_Bridge(0)
004b1aa8  MOV  byte [EAX+0x74],BL  ; restore IsMarked
```

`+0x1CC` is `FootClass::Set_Height_On_Bridge 0x005F5FA0`: the `get_xrefs_to`
DATA list for `0x005F5FA0` contains `0x007F5E3C` and `0x007EB224`; the UnitClass
vtable base is `0x007F5E1C − 0x1AC = 0x007F5C70` and `0x007F5C70 + 0x1CC =
0x007F5E3C`, the InfantryClass base is `0x007EB058` and `0x007EB058 + 0x1CC =
0x007EB224`. Slot proved, both classes.

The setter body: `Z = GetGroundHeight(own already-committed XY) + arg +
(Object+0x8C OnBridge ? g_nFootOnBridgeDeckOffsetLeptons : 0)`, with the marked
branch bracketing the write in `[vtable+0x124]` REMOVE/PUT. `GetGroundHeight
0x00578080` passes the **sub-cell low bytes of X and Y** through to
`CellClass::ComputeGroundHeightAtCoord 0x0047B3A0`, so the value is the
interpolated ramp surface at the mover's exact position, not `level × 104`.

So the native cadence is: **commit XY, resolve any crossing's OnBridge state,
then resample the surface under the new XY.** The residual (sub-point)
interpolation at the tail of `Process_Drive_Track` (`0x004B2543..0x004B25A7`)
ends at `[vtable+0x124](1)` and makes **no** `+0x1CC` call — the residual keeps
the previous raw Z.

### 2.2 The on/off-bridge predicate

Disassembly `0x004B1807..0x004B1847` (ESI = source cell from the pre-move
coordinate at `0x004B17EF`, EBX = destination cell at `0x004B1800`):

```
MOVSX EAX,[ESI+0x11b]   ; src.Level (signed)
MOVSX ECX,[EBX+0x11b]   ; dst.Level (signed)
SUB   EAX,4
CMP   ECX,EAX           ; dst.Level == src.Level - 4 ?
MOV   EAX,0x100
JNZ   004b1837
TEST  [EBX+0x140],EAX   ; dst has bridge stamp ?
JZ    004b183f
MOV   byte [EDX+0x8c],1 ; ENTER: OnBridge = 1
004b1837: TEST [EBX+0x140],EAX
          JNZ  004b1851
004b183f: TEST [ESI+0x140],EAX ; src has bridge stamp ?
          JZ   004b1851
004b1847: MOV  byte [..+0x8c],0 ; EXIT: OnBridge = 0
```

Enter ⇔ `dst.level == src.level − 4 && dst & 0x100`.
Exit ⇔ `!(dst & 0x100) && src & 0x100`. The twin runs at the terminal point
(`0x004B2543..0x004B25B1`).

### 2.3 Height legality: `CheckBridgeTraversal 0x004D9C60`

Body read this session. With `candidate` = the cell being entered and `source`
the predecessor (derived from `(facing − 4) & 7` when not supplied), and
`path_height` the mover's carried numeric height:

- `path_height == −1` and source stamped `0x100` → `path_height = source.level + 4`,
  and the candidate must carry `0x200` or the step is refused (7).
- `selected = source stamped ? source.level : path_height`; `diff = selected − candidate.level`.
- `|diff| == 0` → refused only when (candidate lacks `0x100`, or lacks `0x200`, or
  source lacks `0x100`) **and** `path_height != −1` **and** `path_height != candidate.level`.
- `|diff| == 1` → allowed **iff the lower of the two cells has a nonzero raw TMP
  slope byte `+0x11C`**. This is the whole ramp rule: a one-level step is only
  legal across a ramp tile.
- `|diff| == 4` → the bridge arm. Candidate 4 above: needs `path_height ==
  candidate.level` and a stamped source. Candidate 4 below: needs candidate
  `0x100` **and** `0x200`, then sets the deck-list output and allows.
- 2, 3 and > 4 → refused (7).

### 2.4 Slope speed

`Process_Movement 0x004B3C84..0x004B3DFA`:

```
004b3c98  MOV EDX,[EAX+0x67c]              ; SpeedType
004b3c9e  LEA ECX,[ESI+ESI*8]              ; 9 * land index
004b3ca3  FLD float ptr [EDX*4 + 0x89ea40] ; land x speed-type table
004b3cae  FCOMP [0x007e1718] (= 1.0)       ; factor = min(factor, 1.0)
004b3cda  CALL 0x005657a0                  ; Get_CellClass (destination)
004b3ce1  CALL 0x00480a30                  ; CellClass::Get_Center_Coords
004b3cec  CALL 0x00578080                  ; ESI = destination CENTRE ground Z
004b3cf6..004b3d16                          ; build coord from owner+0x9C
004b3d1a  CALL 0x00578080                  ; EDI = ground Z under the MOVER's exact XY
004b3d21  CMP ESI,EDI / JLE                ; uphill vs downhill
   uphill : What_Am_I()==1 and SpeedType==1 -> *= [Rules+0x768] else *= [Rules+0x778]
   downhill: What_Am_I()==1 and SpeedType==1 -> *= [Rules+0x770] else *= [Rules+0x780]
004b3db4  FCOMP [0x007e2800] (= 0.0)       ; exact-zero -> substitute 0.5
004b3dd4  CALL 0x005f5c60 ; FCOMP [Rules+0x1700] ; <= ConditionYellow -> *= [0x007e7fc0] (= 0.75)
004b3dfa  CMP [EBP+0x58],0x40              ; selector gate, then store to drive+0x50
```

`0x007E2800` reads `00 00 00 00 00 00 00 00` = 0.0 and `0x007E7FC0` reads
`00 00 00 00 00 00 e8 3f` = 0.75 (both read this session). The four Rules
doubles are `TrackedUphill` / `TrackedDownhill` / `WheeledUphill` /
`WheeledDownhill`; stock `RULESMD.INI` `[General]` lines 401-404 give
`1.0 / 1.2 / 1.0 / 1.2`.

### 2.5 The three deck/height constants

They are three separate globals, not one:

| Global | Value | Set by | Used by |
|---|---|---|---|
| `g_nFootOnBridgeDeckOffsetLeptons` (`0x00AC13BC`, read at `0x005F5FB5` `ADD EDI,[0x00ac13bc]` behind the `Object+0x8C` test) | 416 | runtime | the mover's own Z, `0x005F5FA0` |
| `g_BridgeZOffset_Drive` (`0x008A07C4`) | `ftol(4 × g_DriveHeightStep + 0.5)` at `0x004AF4A0` | `0x004AF4C0` | brake distance `0x004B0FE7`, crush-list pick `0x004B18CC`, `Set_Destination 0x004AFDE2` |
| `g_DriveHeightStep` (`0x008A07D0`) | `ftol(SinLookup(g_A − g_B) × g_C × 0.5)` at `0x004AF400` | `0x004AF42B` | `×2` occupier threshold `0x004B2BB7` / `0x004B30A2`, `×3` bridge test `0x004B324F`, arrival Z tolerance, divisor |

All three are BSS; a cold `read_memory 0x008A07C4` returns `00000000`.

### 2.6 Destination Z and the deck

`DriveLocomotionClass::Set_Destination 0x004AFD40` stores the caller's XYZ and,
when the destination cell carries `0x100`, adds `g_BridgeZOffset_Drive`
(`0x004AFDE2`) — unconditionally, whether the mover is routing over or under the
span.

---

## 3. What matches

| # | Behaviour | Native evidence | Rust |
|---|---|---|---|
| M1 | Height cadence: paid track point and arrival snap resample the surface; the residual interpolation does not | `0x004B1A96` `CALL [vtable+0x1CC]` reached from both the same-cell and cell-changed arms; tail `0x004B2543..0x004B25A7` has no such call | `track_host.rs:371-378` commits after every paid point, `:690-699` on `terminal`, and the residual call site `:452-461` passes `terminal = false`. It even mirrors the `Object+0x74` save/clear/restore around the call (`:369-380`). |
| M2 | The height value itself: `ground(exact XY) + (OnBridge ? deck : 0)` | `0x005F5FA0` body; `0x00578080` passes the sub-cell low bytes through | `ground_pose.rs::ground_surface_z_at` / `commit_ground_height` |
| M3 | The ground evaluator (signed level × 104, 21-record slope table, clamp before base, chop) | `CellClass::ComputeGroundHeightAtCoord 0x0047B3A0` | `util/lepton.rs::ground_height_leptons`. **Parity-demonstrated** within its stated coverage: `tools/ramp_height_vectors.json` carries 158 Unicorn-executed fixtures over the setter/getter/ground-lookup/evaluator regions, pins `native_fpcw 3711` (`0x0E7F`), `ground_level_leptons 104`, `bridge_deck_leptons 416`, and is consumed by `ground_pose_tests.rs:634` and `bridge_batch_native_tests.rs:419`. Its own coverage note excludes the movement loops, Mark callbacks and rendering. |
| M4 | On/off-bridge transition predicate | §2.2 above, `0x004B1807..0x004B1847` | `movement_bridge.rs::compute_bridge_transition` (`:201-220`) — identical, including the independent evaluation of the two conditions |
| M5 | Height legality including the ramp rule | `CheckBridgeTraversal 0x004D9C60`, §2.3 | `pathfinding/core.rs::check_bridge_traversal` (`:623-710`) — arm for arm: the `−1` facing arm, the null-parent allow, the `path_height == −1` promotion with the `0x200` refusal, the 0/1/4 arms with the lower-cell slope-byte rule, the `_ => false` tail |
| M6 | Ramp tilt state: 3-frame transition on a change of the containing cell's `+0x11C` | `DriveLocomotionClass::Process 0x004B0500` reads `[vtable+0x1BC]` → `cell+0x11C`, compares against `loco+0x1C`, shifts previous→current and starts a literal 3 | `slope_transition.rs` (`SLOPE_TRANSITION_FRAMES = 3`, `sample_process_entry`), driven from `movement_tick.rs:3496-3511` and consumed by the renderer at `units.rs:184-212` |
| M7 | Speed chain order and constants: land row capped at 1.0, slope multiply, exact-zero → 0.5 substitution on the **combined** value, then ×0.75 at or below `ConditionYellow` | §2.4 | `terrain_speed.rs:150-260` — same order, same constants, and its header states the ordering is load-bearing for exactly the reason the binary shows |
| M8 | Destination deck bump on a `0x100` cell | `0x004AFD40` / `0x004AFDE2` | `navcom.rs::adjusted_destination` (`:98-116`), wired through `refresh_drive_destination_coord` at `movement_tick.rs:3525`. Walk's separate 414 destination offset is modelled independently at `navcom.rs:18-44`. |
| M9 | Ground-plane routing beneath an intact high span (I1) | `UnitClass::Can_Enter_Cell 0x0073F0A0` reads the ground land row on the ground branch | `cell_entry.rs:678-689` + `TerrainCostGrid::ground_cost_at`; covered on retail maps by `tank/infantry/hover_tank_ordered_under_{hills,bay_of_pigs}_high_bridge_crosses_all_four_ground_lanes` and `deck_and_ground_under_one_high_bridge_cell_are_separate_occupancy_planes` |

---

## 4. Divergences

### D1 — the slope's direction is decided from level bytes, not ground Z

**Native (established).** `0x004B3CDA..0x004B3D21`: the uphill/downhill branch
compares `GetGroundHeight(destination cell **centre**)` against
`GetGroundHeight(the mover's own committed coordinate)` — two interpolated
surface samples, the second taken at the mover's exact sub-cell XY.

**Rust (read).** `terrain_speed.rs::slope_factor_for` (`:238-258`) compares
`cell_level(current_cell)` with `cell_level(next_cell)` — the two `u8` `level`
bytes, via `cell_level` at `:205-207`. Wired to production through
`compute_cell_speed_modifier` → `compute_drive_target_speed_fraction`
(`drive_locomotion.rs:106-123`) → `movement_tick.rs:2228`.

**Trigger.** Every step where the two cells' level bytes are equal but the
surface under the mover is not at the destination centre's height. The
systematic case is leaving a ramp cell onto the flat cell at the ramp's own
level: the mover stands at `L×104 + ε` and the destination centre is `L×104`, so
gamemd takes the downhill arm and VERA takes neither.

**Player-visible effect.** That cell is crossed at the level coefficient (1.0)
where gamemd uses `TrackedDownhill` / `WheeledDownhill` = 1.2. The bonus only
survives where the destination land row is below about 0.83, because
`TechnoClass::SetSpeedFraction 0x004D3710` clamps the owner's fraction at 1.0 —
a point VERA's own `drive_locomotion.rs:262-278` already records. So on a 100%
Clear row the divergence produces no pixel; on a sub-83% row (rough, sand,
tiberium, and any modded road below 83) the mover leaves a ramp about 17 percent
slower than retail for one cell.

**Frequency.** UNCHECKED — it needs the land-row distribution of the ramp-adjacent
cells on the retail maps, which no instrument reports.

### D2 — the slope coefficients have no `What_Am_I()` gate

**Native (established).** Both arms gate the multiply on the owner's RTTI id:
`0x004B3D2A` `CALL [EDX+0x2c]; CMP EAX,1; JNZ 0x004B3D68` (uphill) and
`0x004B3D71` `CALL [EAX+0x2c]; CMP EAX,1; JNZ 0x004B3DB0` (downhill). Id 1 is
`UnitClass` (the `Process 0x004B0500` plate comment carries the byte-verified id
table). A non-UnitClass Drive owner gets no coefficient at all.

**Rust (read).** `slope_factor_for` applies the coefficients to every mover that
reaches the chain; `uses_land_type_speed_chain` filters by locomotor, not by class.

**Trigger.** A Drive- or Ship-locomotor owner that is not a `UnitClass`.

**Frequency.** UNCHECKED. Every stock Drive/Ship owner is a `UnitClass`, so the
expected value is zero; that has not been checked against the type list.

**Effect.** None established.

### D3 — the brake band measures a 2-D cell distance (same defect as A3's D7)

**Native (established).** `0x004B0FB0..0x004B1015`, the head of
`Process_Drive_Track`:

```
004b0fc0 ..  ; copy drive+0x34/0x38/0x3c (the locomotor destination)
004b0fdc  CALL 0x00565730                 ; cell at the destination
004b0fe1  MOV  ESI,[EAX+0x140]
004b0fe7  MOV  EDI,[0x008a07c4]           ; g_BridgeZOffset_Drive
004b0fed  AND  ESI,0x100 / NEG / SBB / AND ESI,EDI
004b1003  CALL 0x00578080                 ; ground Z at the destination XY
004b1015  ADD  EAX,ESI                    ; destination Z = ground + (stamped ? deck : 0)
004b1024..004b1055                        ; dx, dy, dz against owner+0x9C/0xA0/0xA4
          Sqrt_Approx(dx*dx + dy*dy + dz*dz) ; ftol; CMP against type+0x2F8
```

Three facts matter: the term is **3-D**; the destination Z is **recomputed** from
the destination's ground plus the deck offset rather than read from the stored
destination Z; and the XY is the locomotor's **destination coordinate**, not a
cell centre.

**Rust (read).** `movement_tick.rs::distance_to_goal_leptons` (`:275-282`) is a
2-D Euclidean distance from the mover's XY to `goal × 256 + 128`, the goal
**cell centre**. `movement_tick.rs:2260-2270` then adds a flat
`BRIDGE_Z_OFFSET` (416) for a water mover whose **current** cell carries a deck.

**This is the same defect as A3's D7, not a separate one** — one site, one fix,
and A3 already owns it. Two things the A3 row does not say, which belong to A8:

1. The destination Z is `ground(destXY) + deck`, resampled, so the fix cannot
   just read the stored `drive+0x3C`.
2. VERA's water-mover `+416` is a VERA-invented partial substitute for the same
   Z term, and it is wrong in three ways: additive instead of a Euclidean
   component, keyed on the **source** cell where native uses the destination, and
   applied only to water movers where native applies the Z term to everyone. The
   unit test `gsi_04_03b_water_mover_bridge_clearance_crosses_braking_boundary`
   (`movement_bridge.rs:380-405`) pins that substitute: at 100 leptons planar it
   asserts 516 and "outside braking range", where native's
   `sqrt(100² + 416²) ≈ 428` is **inside** a 500 `SlowdownDistance`.

**Trigger.** Every braking approach whose destination ground Z differs from the
mover's Z: every arrival on a deck, under a span, or at a different terrain level.

**Frequency.** UNCHECKED numerically — the retail harness records cell, z and
on-bridge per tick but not the brake band.

**Player-visible effect.** Slope and bridge arrivals brake late (the 2-D distance
is always ≤ the 3-D one, so the band opens later); the water-under-span case
brakes later still because of the additive term.

### D4 — two height fields where gamemd has one

**Native (established).** One number: `Object+0xA4`, absolute leptons. There is
no coarse level on the object; `ObjectClass::GetHeight 0x005F5F40` derives the
height-above-surface by subtracting the ground lookup, and `+0x1CC` derives the
absolute Z by adding it back.

**Rust (read).** `components.rs::Position` carries both `z: u8` (a level byte)
and `exact_z_leptons: Option<i32>`. They have different writers:

- `exact_z_leptons` ← `ground_pose::commit_ground_height`, the faithful `+0x1CC`
  analogue (M1/M2 above).
- `z` ← only `movement_bridge.rs:273-278`, inside
  `resolve_cell_transition_bridge_state`, as
  `dst_cell.signed_level() + (on_bridge_after ? 4 : 0)` — level granularity, cell
  centre, no ramp interpolation.

The drive-track crossing arm (`track_host.rs:672-690`) calls
`compute_bridge_transition` directly and updates `on_bridge` and
`bridge_occupancy` but **not** `position.z`; the shared step path
(`movement_step.rs:2904`, `movement_tick.rs:2656` and `:2767`, `walk_host.rs:302`)
calls `resolve_cell_transition_bridge_state` and does update it. Which of the two
step paths last ran therefore decides whether `position.z` is fresh.

**Player-visible effect.** None established today: `world_z_leptons`
(`render/locomotor_visual.rs:108-114`) and `foot_depth.rs:248-258` both prefer
`exact_z_leptons` and fall back to `z × 104` only when it is absent, which
ordinary ground movers are not. The risk is that `position.z` is a live,
serialized, hash-visible field with a partial writer.

**Frequency.** UNCHECKED — it needs a run that records both fields for a
track-driven mover across a ramp and a deck.

**Instrument consequence (this is the sharper half).** A8's retail harness
asserts on the **fallback** field: `TickRow.z = entity.position.z`
(`movement_bridge_retail_tests.rs:169`, built at `:1008`, `:1751`, `:2412`,
`:3684`), and `TickRow::expected_z` (`:201-208`) is
`terrain_level + BRIDGE_DECK_LEVEL_DELTA` in **levels**. So the 30 retail
crossings prove a level-granular height on the field the renderer does not use,
and cannot see a wrong lepton Z, a wrong ramp interpolation, or a missing 416 in
`exact_z_leptons`.

### D5 — the bridge-traversal gate is skipped on a class of edges

**Native (established).** `AStar_main_loop` calls the `+0x1AC` slot
unconditionally at `0x00429F54`; `UnitClass::Can_Enter_Cell 0x0073F0A0` reaches
`CheckBridgeTraversal 0x004D9C60` on every candidate.

**Rust (read).** `core.rs::needs_bridge_traversal_for_edge` (`:561-583`) returns
false — i.e. skips the gate — for structural→structural edges where the parent is
not at its own `level + 4` and the candidate lacks `0x200`; the caller then uses
"its local height-difference rule". `movement_occupancy.rs:186-204` mirrors the
same skip in the runtime crossing so the runtime accepts what the plan produced.
The function's own doc-comment states it is a VERA adapter and not a native skip
predicate, which is the right disclosure.

**Trigger.** Deck-interior driving (ramp→body, body→body) on a span whose body
cells do not carry `0x200`.

**Effect.** On those edges the one-level ramp rule and the 2/3-level refusal
never run.

**Frequency.** UNCHECKED — it needs a count of such edges in the retail bridge
inventory, which `retail_high_bridge_inventory` could produce but does not.

### D6 — the runtime `+0x1AC` leaf carries no numeric path height

**Native (established).** `UnitClass::Can_Enter_Cell 0x0073F0A0` clears its deck
flag by comparing the mover's carried path height against the cell's signed level
(`0x0073F0B7..F0E8`), and only then selects the ground list `+0xE4` at
`0x0073F51A` and reads the land row at `0x0073FAB5`. The height is an input to the
slot, not a layer enum.

**Rust (read).** `cell_entry.rs::evaluate_shared_cell_leaf` returns on the
structural/transition gate at `:740-760` with the comment *"Until +0x1AC threads
its numeric path height, retain the prior structural gate"*. Under/over-span
plane selection on that seam therefore rests on `ctx.terrain_layer` plus the
structural stamp. (The parallel `movement_occupancy` runtime evaluation *does*
carry `args.height` and derives `object_list_layer` from
`|height − level| >= 2`, so the two seams disagree about what decides the plane.)

**Effect.** This is the standing boundary of I1 rather than a new wrong pixel; it
is what keeps D5's local rule necessary.

**Frequency.** UNCHECKED.

### D7 — the three native height constants are collapsed into two

**Native (established).** §2.5: the mover's own Z uses the Foot global (416,
oracle-confirmed); the brake distance, the crush-list pick and `Set_Destination`
use `g_BridgeZOffset_Drive = ftol(4 × g_DriveHeightStep + 0.5)`; three separate
thresholds are built from `g_DriveHeightStep` itself — `×2` for the occupier-list
pick (`0x004B2BB7`, `0x004B30A2`), `×3` for the "am I on the span above this
cell" test (`0x004B324F`, and the same shape in the scatter arm), and `×2` again
for the arrival Z tolerance.

**Rust (read).** `BRIDGE_HEIGHT_DELTA_LEPTONS = 416` serves both the Foot setter
role (`ground_pose.rs`) and the Drive-locomotor role (`navcom.rs`,
`movement_tick.rs`); `GROUND_LEVEL_HEIGHT_LEPTONS = 104` serves the height-step
role. `lepton.rs:129-131` explicitly says the two 416s are "kept separate … even
though both active retail values are 416 leptons", so the conflation is knowing.

**Effect.** None while `g_DriveHeightStep == 104`. The identity is the exposure:
if it is not 104 the brake band, the crush-list pick, the `Set_Destination` bump
and the arrival tolerance all move together and VERA's would not.

**Frequency.** Not applicable until the identity is settled.

---

## 5. What increment I1 did and did not cover

**Did.** It moved the ground-plane land-row read from the Infantry-only arm to
every ground-layer mover, so a Drive or Hover mover under an intact high span
reads the terrain's own row instead of the deck's override
(`cell_entry.rs:678-689`, `TerrainCostGrid::ground_cost_at`). Retail coverage is
real and specific: four ground lanes under the Hills and Bay of Pigs spans for
tank, infantry and hover tank; entry admitted on every lane; deck and ground
under one cell held as separate occupancy planes; the riverbed beside the Bay of
Pigs span still refused.

**Did not.**

- It did not thread the numeric path height into the `+0x1AC` leaf (D6), so the
  plane is still chosen by `MovementLayer` plus the stamp on that seam.
- It did not touch the over-span side. `Set_Destination 0x004AFDE2` raises **any**
  destination Z on a `0x100` cell to the deck, including a destination the mover
  is routing *under*; VERA ports that faithfully (`navcom.rs::adjusted_destination`)
  but nothing then consumes the destination Z, because the brake band uses the
  goal cell centre instead (D3). The two halves of the over-span contract are
  each present and not connected.
- It did not touch height *legality*: the ramp and 4-level arms live in
  `check_bridge_traversal`, which the I1 seam does not call.
- Its damaged-span consequence is still unrun — the ledger's own residual
  (plan lines 93-97) says no retail damaged-span crossing has been driven for Drive.

---

## 5a. Documentation corrections owed

- `src/sim/movement/movement_bridge_retail_tests.rs:199` attributes
  `TickRow::expected_z` to "`ObjectClass::GetHeight` @ `0x005F5F30`". The
  function entry is `0x005F5F40` (`get_function_by_address`, body
  `0x005F5F40..0x005F5F91`). The same comment claims the value is "the only Z the
  native model can produce for this cell and this OnBridge state", which is not
  true at level granularity: the native model produces
  `ComputeGroundHeightAtCoord(exact XY) + (OnBridge ? 416 : 0)` in leptons, which
  on a ramp cell is not `level × 104`. The comment should say what the field
  actually is — a coarse-level proxy.
- `src/sim/movement/ground_pose.rs:3-6` says "Callers own cadence: Drive/Ship
  residual movement does not call this setter and retains the last raw
  coordinate Z." That is correct and now has its address: the residual tail
  `0x004B2543..0x004B25A7` ends at `[vtable+0x124](1)` with no `+0x1CC` call,
  while the paid/terminal path calls it at `0x004B1A96`. Worth carrying the two
  addresses in the header so the next reader does not have to re-derive them.

---

## 6. UNCHECKED, with the instrument each needs

| Item | Instrument that would settle it |
|---|---|
| D1's frequency (how many retail ramp exits sit on a sub-83% land row) | Extend `retail_high_bridge_inventory` / a new ramp inventory over `Hills.mmx`, `BayOPigs.mmx`, `Deadman.mmx` to report each ramp-adjacent cell's land row per SpeedType. Cheap; needs only the harness. |
| D3's frequency and magnitude | A brake-band column in the retail harness `TickRow` (native distance term vs VERA's), or a Unicorn harness over `0x004B0FB0..0x004B1015`. Neither exists. |
| D4: whether `position.z` actually goes stale for a track-driven mover | Add both fields to `TickRow` and re-run the ignored retail crossings. One-line instrument change. |
| D5's edge count | `retail_high_bridge_inventory` already walks the spans; it would need to print the `0x200` flag per body cell. |
| D7: is `g_DriveHeightStep` 104, and is `g_BridgeZOffset_Drive` 416? | Both are BSS (cold read is zero). Needs a live/attached read of `0x008A07D0` and `0x008A07C4` in a running `gamemd.exe` — `debugger_read_memory` after attach, or a Unicorn run of the two init leaves `0x004AF400` / `0x004AF4A0` with the isometric globals seeded. The Foot-side 416 is already oracle-confirmed and needs nothing. |
| Whether the land-row table index at `0x0089EA40` reads the destination or the source cell's land type (VERA reads the destination) | `ESI` at `0x004B3C9E` is set by predecessors not walked here. A short dataflow read of the block feeding `0x004B3C84`. The prior verification of 2026-08-04 recorded in `terrain_speed.rs` says destination; it was not re-derived this session. |
| Whether the Ship twin (`0x0069FC10` / `0x006A0500` / `0x006A2xxx`) carries the same slope, brake and height arms instruction-for-instruction | Diff the two functions. The xref list shows Ship calls `GetGroundHeight` at the mirrored offsets, which is consistent but not proof. |
| Ramp tilt *geometry* (the matrix, not the timer) | Out of this examination's reach: it is a render question and the memory record says it is settled by retail `-win` capture vs `asset render`, not by a body read. |
| Whether a runtime (`--release`) crossing of a ramp and a deck looks right | No in-game pass was made. The ledger's standing residual (plan lines 83-86) applies here too. |

---

## 7. Proposed ledger row

Replacing A8's row in the "Criteria without an increment" table of
`docs/plans/2026-09-15-movement-retail-acceptance.md`:

> | A8 Height, slopes, bridges | **EXAMINED 2026-09-16 — NOT ACCEPTED.** Seven divergences against `FootClass::Set_Height_On_Bridge 0x005F5FA0` (vtable `+0x1CC`, slot proved at `0x007F5E3C`/`0x007EB224`), `Process_Drive_Track 0x004B0F20`, `Process_Movement 0x004B2630`, `CheckBridgeTraversal 0x004D9C60` and `Set_Destination 0x004AFD40`. **Matching (native read, Rust read):** the height cadence — paid point and arrival resample the surface at the mover's exact XY (`0x004B1A96`), the residual keeps the previous raw Z (`0x004B2543..0x004B25A7`) — mirrored by `track_host.rs:371/452/690` down to the `Object+0x74` save/restore; the ground evaluator `0x0047B3A0` (**parity-demonstrated** by 158 Unicorn fixtures in `tools/ramp_height_vectors.json`, `bridge_deck_leptons 416`, `ground_level_leptons 104`, FPCW `0x0E7F`, coverage excludes the movement loops); the on/off-bridge predicate `0x004B1807..0x004B1847` (`dst.level == src.level − 4 && dst&0x100` to enter, `!dst&0x100 && src&0x100` to exit); `check_bridge_traversal` arm for arm including the one-level ramp rule and the 4-level bridge arm; the 3-frame ramp-tilt timer off `cell+0x11C` (`0x004B050B`); the speed chain's order and constants (cap 1.0, slope, exact-zero → 0.5, ×0.75 at `ConditionYellow`); the `Set_Destination` deck bump. **D1 (worst) — the slope direction is read from level bytes.** Native compares `GetGroundHeight(destination centre)` with `GetGroundHeight(the mover's own coordinate)` at `0x004B3CEC`/`0x004B3D1A`, decided at `0x004B3D21`; `terrain_speed.rs:238-258` compares the two cells' `u8` levels. Trigger: every ramp exit onto the flat cell at the ramp's own level. Effect: 1.0 where gamemd uses `TrackedDownhill`/`WheeledDownhill` = 1.2, visible only on land rows below ~0.83 because `SetSpeedFraction 0x004D3710` clamps the rest away. Frequency UNCHECKED. **D2** — the coefficients lack native's `What_Am_I()==1` gate (`0x004B3D2A`, `0x004B3D71`); zero expected frequency, UNCHECKED. **D3 — the brake band is A3's D7, one defect, one owner.** A8 adds that native recomputes the destination Z as `ground(destXY) + (stamped ? g_BridgeZOffset_Drive : 0)` at `0x004B1003`/`0x004B1015` rather than reading the stored destination, and that VERA's water-mover `+416` (`movement_tick.rs:2262`) is an invented substitute — additive, keyed on the source cell, water-only — pinned by `gsi_04_03b_water_mover_bridge_clearance_crosses_braking_boundary`, which asserts 516/"outside" where native gives ≈428/"inside" at 100 planar. **D4 — two height fields.** `Position.exact_z_leptons` (the faithful `+0x1CC` analogue) and `Position.z` (levels, written only by `movement_bridge.rs:278`, which the drive-track crossing arm does not call). No wrong pixel today — the renderer prefers the exact field — but A8's own retail harness asserts `position.z` in **levels** (`movement_bridge_retail_tests.rs:169/201`), so the 30 retail crossings cannot see a wrong lepton Z or a wrong ramp interpolation. **D5** — `needs_bridge_traversal_for_edge` (`core.rs:561`) skips `0x004D9C60` on structural→structural edges where native's `AStar_main_loop 0x00429F54` calls the slot unconditionally. **D6** — the `+0x1AC` leaf carries no numeric path height (`cell_entry.rs:740-760`), so the plane comes from `MovementLayer` plus the stamp, not from native's `0x0073F0B7..F0E8` comparison; this is I1's standing boundary. **D7** — the three native constants (`g_nFootOnBridgeDeckOffsetLeptons 0x00AC13BC`, `g_BridgeZOffset_Drive 0x008A07C4 = ftol(4 × g_DriveHeightStep + 0.5)` at `0x004AF4A0`, `g_DriveHeightStep 0x008A07D0`) are collapsed into 416 and 104; inert while the step is 104, and the step is BSS so it is UNCHECKED. I1 covered under-span ground routing for Drive and Hover with real retail coverage; it did not touch over-span destination consumption, height legality, or the numeric-height seam. Report: [A8 examination](../research/MOVEMENT_ACCEPTANCE_A8_HEIGHT_SLOPES_BRIDGES_EXAMINATION.md). | `ramp_height_oracle` exists and is executed-native (158 fixtures, consumed by `ground_pose_tests.rs:634`); the retail bridge matrix exists as 30 ignored retail-map crossings but asserts the coarse level field, so **no instrument on either side observes the exact lepton Z, the brake band, or the slope coefficient actually applied**. |

---

## 8. Provenance of every native address cited

Read this session from `testProsjekt` / `gamemd.exe`, image base `0x00400000`:

- `0x0047B3A0` `CellClass::ComputeGroundHeightAtCoord` — decompiled.
- `0x00480A30` `CellClass::Get_Center_Coords` — identified via `get_function_by_address`.
- `0x004AF400` `InitHeightStep_A`, `0x004AF4A0` `ComputeBridgeZOffset` — disassembled.
- `0x004AFCA0` `Head_To_Coord`, `0x004AFD40` `Set_Destination` — decompiled.
- `0x004B0500` `Process`, `0x004B0C40` `Force_Track` — decompiled.
- `0x004B0F20` `Process_Drive_Track` — decompiled, plus disassembly at
  `0x004B0FC0..0x004B1060`, `0x004B13F0..0x004B14C0`, `0x004B17C8..0x004B1848`,
  `0x004B1860..0x004B1930`, `0x004B1A77..0x004B1AB0`.
- `0x004B2630` `Process_Movement` — disassembly at `0x004B2B70..0x004B2C10`,
  `0x004B2CC0..0x004B2D20`, `0x004B3040..0x004B3110`, `0x004B31A0..0x004B3260`,
  `0x004B32B0..0x004B32F8`, `0x004B3C30..0x004B3E10`, `0x004B4090..0x004B40E0`,
  `0x004B41A0..0x004B41F0`, `0x004B4550..0x004B46E0`; instruction searches for
  `0xa4`, `0x9c`, `[EBX`, `0x40], E`, `0x48], E`, `CALL 0x0048`.
- `0x004D9C60` `CheckBridgeTraversal` — decompiled.
- `0x004DB810` `TechnoClass::Set_Coords_With_Cloak` — decompiled.
- `0x00578080` `CellClass::GetGroundHeight` — decompiled; `get_xrefs_to` walked in
  full (160 entries over two pages) to place the Drive/Ship/Walk/Hover/Jumpjet
  consumers.
- `0x005F5FA0` `FootClass::Set_Height_On_Bridge` — decompiled and disassembled
  (`0x005F5FA0..0x005F5FC0`, giving the `Object+0x8C` test and
  `ADD EDI,[0x00ac13bc]`); `get_xrefs_to` gave the vtable DATA slots used for the
  `+0x1CC` proof.
- `0x005F5F40` `ObjectClass::GetHeight` — entry and body bounds from
  `get_function_by_address` (the tree cites `0x005F5F30`; see §5a).
- `0x005F60A0` `ObjectClass::Mark_Put` — decompiled (ruled out as a Z writer).
- Data: `0x007E1718` (1.0), `0x007E1738` (0.5), `0x007E2800` (0.0),
  `0x007E7FC0` (0.75), `0x008A07C4` / `0x008A07D0` (BSS, cold zero),
  `0x007F5E24` (vtable `+0x1B4`).
- Retail INI: `RULESMD.INI` `[General]` lines 401-404 for the four slope coefficients.
