# A1 "order admission" — examination against gamemd.exe

Scope: criterion A1 of `docs/plans/2026-09-15-movement-retail-acceptance.md` —
*"when a player issues a move order, does VERA20k accept and refuse exactly the
destinations retail accepts and refuses?"*

Binary: Ghidra project `testProsjekt`, program `gamemd.exe` (the project's retail
image). Every native address below was read in this session by
`decompile_function` / `disassemble_function` / `disassemble_bytes` /
`read_memory`; nothing is quoted from a label alone. Rust was read directly in
this worktree (`feature/vera20k-drive-residual-budget`, e933da49).

Evidence classes used: **[N]** native-established (body/caller/data read here),
**[R]** Rust-read, **[P]** parity-demonstrated (native-vs-Rust executable
comparison). No item below is **[P]** unless it says so.

---

## 0. Summary verdict

**No.** Two of the three things the A1 row names are wrong as written, and the
production Rust path for *vehicles* is a different mechanism from the native one,
not a port of it.

1. The A1 row says refusals happen where `Can_Enter_Cell +0x1AC` returns **code
   7**. Native's `+0x1AC` result at the click target does **not** refuse the
   order at all. It selects the **cursor** (`ACTION_MOVE` vs `ACTION_NOMOVE`),
   and the threshold is **`code <= 1` accepts, `code >= 2` is NOMOVE** — not
   "code 7". **[N]** `0x00700B75..0x00700B7E`.
2. `ACTION_NOMOVE` still issues a Move mission. Actions 1 and 2 land on the same
   handler in `FootClass::ClickedAction_Cell`; NOMOVE only skips the destination
   flash animation. **[N]** jump tables `0x004D8244` / `0x004D8270`.
3. Native's zone check (`MapClass::Can_Reach_Zone 0x0056D100`) never refuses a
   move order either. On failure the engine **retargets into the mover's own
   zone** — VERA's behaviour is right in kind — but it picks the substitute with
   **`g_CurrentFrameCounter % candidateCount`**, not nearest-to-click, because
   the selection reference it passes is the zeroed null CellStruct. **[N]**
   `0x004DE4D7`/`0x004DE4DC` zero the reference; `0x0056DC20` tail selects on
   `reference == DAT_00abd480` (= `{0,0}`, read).

VERA already contains a faithful port of the native resolver
(`resolve_walk_cell_click`, `src/sim/world/move_cell_input.rs`), but it is gated
to ordinary grounded **Walk** movers. **[R]** Every vehicle (Drive/Hover/Ship/…)
order takes a different, three-stage substitution path with no shroud gate, no
height gate, no per-candidate playfield gate, a different selection rule and a
different evaluation order.

---

## 1. The native order-admission spine (all **[N]**)

### 1.1 Dispatch

| Step | Address | What it is |
|---|---|---|
| Cursor/action for a cell | `UnitClass::What_Action_OnCell 0x007404B0` (slot `+0x70`, read at `0x007F5CE0`); `InfantryClass 0x0051F800`; `FootClass 0x004DDDE0`; core `TechnoClass 0x00700600` | decides `ACTION_MOVE`(1) / `ACTION_NOMOVE`(2) / `ACTION_NONE`(0) |
| Cell click entry | `UnitClass::Active_Click_With(cell) 0x00738910` (slot `+0x140`, read at `0x007F5DB0`); `InfantryClass 0x0051F250` (`0x007EB198`) | per-class click receiver |
| Shared cell handler | `FootClass::ClickedAction_Cell 0x004D7D50` | gates, then the resolver, then the mission |
| Destination resolver | `FUN_004DE1D0` (called at `0x004D806F`) | the thing `resolve_reachable_move_goal` stands in for |
| Mission emission | `Player_Assign_Mission` via vtable `+0x378`, mission **2** | issued with the **resolved** cell |

The object-click sibling is `FootClass::ClickedAction_Object 0x004D74E0`
(`UnitClass 0x00738890` at slot `+0x144`, `0x007F5DB4`; base `0x00417BD0` in
vtable `0x007E22A4`, RTTI pointer at `0x007E22A0`). It carries a structurally
identical resolver inline at `0x004D7A31..0x004D7C08`. This report follows the
**cell** path, which is the move order.

Vtable identities the task named, verified by direct slot reads:

* `0x007F5C70 + 0x1AC` = `0x007F5E1C` holds `0x0073F0A0` (`UnitClass::Can_Enter_Cell`). ✔
* `0x007EB058 + 0x1AC` = `0x007EB204` holds `0x0051BF90` (`InfantryClass`). ✔
* `Can_Enter_Cell` ends `RET 0x14` at `0x0073FD43` → five stack args.

### 1.2 What `+0x1AC` actually decides

`TechnoClass::What_Action(cell,…) 0x00700600`, tail:

```
00700AF6  CALL 0x00578460            ; Is_Cell_In_Playfield(clicked, 1)
00700AFD  JZ   0x00700C17            ;   not in playfield -> ACTION_NOMOVE (2)
00700B5F  PUSH 1 / PUSH 0 / PUSH -1 / PUSH -1 / PUSH EDI
00700B6D  CALL 0x005657A0            ; Get_CellClass(clicked)
00700B75  CALL dword ptr [EBX+0x1AC] ; Can_Enter_Cell(cell, -1, -1, 0, 1)
00700B7B  CMP  EAX, 1
00700B7E  JLE  0x00700C08            ;   code <= 1 -> ACTION_MOVE (1)
          ; code >= 2: BuildingClass+0x408/+0x5EC arm -> 1;
          ;            TypeClass+0xD2C == 0 -> return 2;
          ;            else retry Can_Enter_Cell(cell,-1,-1,0,0); >1 -> 2 else 1
```

So the admission-relevant threshold is `>= 2`, which includes the
column-following code 2 and the destroyable-wall codes 4/5, not only 7.

`FootClass::What_Action_OnCell 0x004DDDE0` wraps it: a **shrouded** destination
returns 2 unless `TypeClass+0xC8D` (`MoveToShroud`) is set and the cell is in the
playfield.

### 1.3 The gates that really refuse

`FootClass::ClickedAction_Cell 0x004D7D50`:

```
004D7D58  this+0x298 != 0                                   -> return false
004D7D6D  this+0x2A8 != 0 && TypeClass+0x692 == 0           -> return false
004D7DA6  action = this->+0x70(clicked, 0, flag)            ; RECOMPUTED here
004D7DB3  Is_Cell_In_Playfield(clicked, 1)
004D7DBC    if !inPlayfield && action != 2                  -> return false
004D7DCC  switch(action-1)  [jump 0x004D8244, bytes 0x004D8270]
004D806F  CALL 0x004DE1D0                                   ; resolver
004D808F  resolved == {0,0}                                 -> return false (ORDER DROPPED)
          else Player_Assign_Mission(2, 0, CellClass(resolved), CellClass(clicked))
```

Jump/byte tables read directly: byte table index is `action-1`; index 0
(action 1) → `0x004D7EB5` (destination-flash anim, then straight-line fall into
`0x004D8065`), index 1 (**action 2**) → `0x004D8065` — the same
`CALL 0x004DE1D0` site. **ACTION_NOMOVE issues a Move.** Action `0x3E` shares
action 1's entry.

`UnitClass::Active_Click_With(cell) 0x00738910` adds one more refusal:

```
0073891D  AL = this+0x6E0
00738925  JZ 0x0073893D                    ; skip when clear
0073892D  CALL [vtable+0x70]               ; What_Action_OnCell
00738930  CMP EAX, 2 ; JNZ ...             ; == 2 -> return false
0073893D  this+0x298 != 0 -> return false
0073895D  CALL 0x004D7D50
```

`UnitClass+0x6E0` is written by `UnitClass::Constructor 0x00735422`,
`FUN_00739AC0` (`0x00739B6A`, `0x00739C6A`) and `FUN_00739CD0` (`0x00739D21`,
`0x00739E4F`), and read by `Mission_Unload`, `Scatter`, `Set_Destination`,
`Facing_Update`, `IsFalling`, `DrawVoxelBody`. Its INI identity is **UNCHECKED**
(instrument: `TechnoTypeClass`/`UnitClass` ReadINI key-string xrefs, or a live
breakpoint on `0x00739D21` while deploying).

### 1.4 The resolver `FUN_004DE1D0` — what native does instead of VERA's retarget

Signature (from the frame): `(outCellStruct*, clickedCellStruct*, flag)`, `RET 0xC`.

```
004DE1E9  action  = this->+0x70(clicked, flag, 0)
004DE1F8  inPlay  = Is_Cell_In_Playfield(clicked, 1)
004DE26B  TypeClass+0xC8D (MoveToShroud) == 0
004DE27D    && IsShrouded(clickedCentre)            -> 004DE562: return {0,0}   (ORDER DROPPED)
004DE2B7  mz = TypeClass+0x5B4 ;  mz==6 -> 0 ;  mz==9 && 0x0053A130(owner) -> 7
004DE2E3  special = ObjectClass::IsHighFlying(0x004DE620)
            || TypeClass+0xD2C
            || (What_Am_I()==0x0F && this[0x6C0]+0xD94 && !0x0053A130)
            || What_Am_I()==2
004DE33F  ownZoneBit = this->+0xBC(0)
004DE3B5  shrouded = IsShrouded(clickedCentre)

004DE3BE  if special && action == 2:
004DE43B      FNPC(out, clicked, SpeedType, zone = -1, mz, clickedBridge, 1,1,0,1, amph, 1,
                   ref = {0,0}, 0, 0)
004DE455  else:
004DE459      if !inPlay                      -> retarget (004DE4BD)
004DE45D      if shrouded                     -> retarget
004DE461      if special                      -> 004DE535: clicked cell VERBATIM
004DE471      if TypeClass+0xCD4 (Teleporter) && action != 2 -> VERBATIM
004DE4B4      Can_Reach_Zone(&ownCell+0x24, &clicked, mz, ownZoneBit, clickedBridge, 0)
004DE4BB          true  -> VERBATIM
                  false -> retarget:
004DE4D7/DC           zero a local CellStruct  -> the selection reference is {0,0}
004DE506              ownZone = MapClass::GetZoneID(&ownCell+0x24, mz, ownZoneBit)
004DE528              FNPC(out, clicked, SpeedType, ownZone, mz, clickedBridge,
                           1,1,0,1,0,1, ref = {0,0}, 0, 0)
004DE539  result == {0,0} -> return {0,0}     (caller drops the order)
```

So **native does retarget an unreachable goal into the mover's own zone** — that
part of VERA's model is correct. What differs is *how the substitute is chosen*
and *what the search admits*.

`MapClass::Find_Nearby_Passable_Cell 0x0056DC20`, selection tail:

```
if (*ref == (short)DAT_00abd480 && ref[1] == DAT_00abd480._2_2_) {
    *out = (directCount == 0) ? indirect[g_CurrentFrameCounter % indirectCount]
                              : direct  [g_CurrentFrameCounter % directCount];
} else {
    *out = argmin over the pool of Sqrt_Approx((cx-refx)^2 + (cy-refy)^2);
}
```

`read_memory 0x00ABD480` = `00 00 00 00 00 00 00 00`, and the reference passed by
both resolver branches was just zeroed. **The frame-counter branch is the live
one.** (`read_memory 0x008B3D88` = zero likewise: the null-cell sentinel is
`{0,0}`.)

Other FNPC arguments the call sites fix, read from the pushes:

| FNPC arg | Native value here | Meaning in the body |
|---|---|---|
| footprint w,h | `1, 1` | `CellRect::CheckPassability 0x0056E7C0` rectangle |
| `param_11` | **`1`** | height gate: `abs(seedLevel − 4·candidateBridge − candidateLevel) < 2` |
| `param_12` | `0` (`amph` in the special/NOMOVE arm) | `Is_Current_Cell_Obstacle_Free` |
| `param_13` | `1` | bridge cells allowed |
| `param_16` | `0` | no `CellRect::CheckOccupancy` |
| anchor gate | always | `MapClass::Is_Cell_In_Playfield_CellClass 0x00578540` per candidate |
| radius cap | `min(MapClass+0xF4 + MapClass+0xF8, 32)` | `0x0056DCE1..` |
| pool cap | 24 (`0x18`) | early-exit on equality |

`MapClass::Can_Reach_Zone 0x0056D100` is **strict zone-id equality**
(`GetZoneID(B) == GetZoneID(A)`) plus two asymmetric off-playfield escapes
(source outside the playfield rect but inside the `[Map] Size` diamond → true;
with `param_6` set, source inside / destination outside-but-in-diamond → true).
No adjacency or super-zone widening exists in the body.

---

## 2. What VERA does (all **[R]**)

Two disjoint production paths, split at
`src/app/input/commands.rs:830 ordinary_cell_move_goal`:

* `native_ground_walk == true` (plain Move, not queued, `ordinary_cell_receiver`)
  **and** `LocomotorKind::Walk` **and** not Teleporter/Jumpjet/Subterranean →
  `Simulation::ordinary_ground_walk_cell_input`
  (`src/sim/world/move_cell_input.rs:13`) → `resolve_walk_cell_click`.
* everything else → the compatibility adapter at
  `src/app/input/commands.rs:852-858`:
  `nearest_walkable_any_layer(clicked, radius 12)`, then the sim's
  `try_start_move` (`src/sim/movement/movement_commands.rs`) applies
  `resolve_requested_move_goal(…, 10)` at line 691, runs A\*, and only on A\*
  failure calls `resolve_reachable_move_goal`
  (`src/sim/movement/movement_path.rs:309`).

### 2.1 What matches

| Item | Evidence |
|---|---|
| `resolve_walk_cell_click` reproduces `FUN_004DE1D0`'s arm order: shroud drop, special/high-flying verbatim, Teleporter verbatim, `Can_Reach_Zone`, own-zone FNPC. | **[R]** `move_cell_input.rs:222-334` vs **[N]** `0x004DE26B..0x004DE528`. |
| On that path FNPC is called with `target_cell: None` (frame-counter selection), `check_height: true`, `anchor_gate: NativeHeightAware`, `allow_bridge_cells: true`, `check_occupancy: false`, `footprint: SINGLE`, `radius_cap: map_owned_radius_cap(size)`. | **[R]** `move_cell_input.rs:336-357`; matches the pushes at **[N]** `0x004DE4BD..0x004DE528`. |
| `ZoneGrid::can_reach_native` (`zone_map.rs:565`) implements `0x0056D100` including both off-playfield escapes and the target-then-source query order. | **[R]/[N]** |
| `find_nearby_passable_cell` implements the ring order, 24-candidate cap, per-ring early-out, direct/indirect pool split, `frame % pool.len()` selection, `min(w+h, 32)` cap. | **[R]** `src/sim/find_nearby_cell.rs:36-285`; **[N]** `0x0056DC20`. |
| `MoveToShroud` is `TechnoTypeClass+0xC8D`, default `true` for non-aircraft. | **[R]** `src/rules/object_type.rs:757-760, 1905-1907`; **[N]** the `0x004DE26B` read. |
| A move order is emitted with the **resolved** cell, before event encoding, and the synchronized executor does not re-resolve. | **[R]** `commands.rs:862-885 roundtrip_ordinary_local_move`; **[N]** `0x004D806F` precedes the `+0x378` emission. |

The `resolve_walk_cell_click` path is **[P]-adjacent but not [P] here**: its own
plate comment at `0x004DE1D0` cites a 20-case native corpus
(`tools/spatial_oracle/walk_move_admission.py`) with a recorded gamemd SHA1 and
freeze hash. I did not re-run that harness, so I report it as a prior claim, not
as parity demonstrated in this examination.

### 2.2 Divergences

Frequencies below are stated only where a code path or stock INI establishes
them; otherwise UNCHECKED with the instrument named.

**D1 — Vehicles never reach the native resolver.**
Native address: `0x004DE1D0` (reached for every Foot cell click via
`0x004D806F`). Rust: `move_cell_input.rs:22-27` returns `None` for any
`active_kind() != LocomotorKind::Walk`.
Trigger: any cell-click move order on a `[VehicleTypes]` unit.
Frequency: **every vehicle move order** (established from the Rust gate; every
stock vehicle is Drive/Hover/Ship/Tunnel, none is Walk).
Player-visible effect: the whole of D2–D7 below applies to vehicles.

**D2 — Substitute selection rule.**
Native: `candidates[g_CurrentFrameCounter % N]` (`0x0056DC20` tail, reference
`{0,0}` zeroed at `0x004DE4D7`/`0x004DE4DC`).
Rust: `resolve_reachable_move_goal` passes `target_cell: Some(goal)` and
`frame_counter = 0` (`movement_path.rs:355, 371`), taking the nearest-distance
branch; its own doc comment calls the choice UNCHECKED.
Trigger: any order whose goal fails the zone test with more than one surviving
candidate.
Frequency: UNCHECKED — needs a candidate-count distribution from a retail map
(instrument: instrument `0x0056DC20`'s `local_1c8`/`local_1c4` at
`0x0056E5B3` under the debugger, or the `spatial_oracle` FNPC lane over a retail
map's water/cliff clicks).
Effect: clicking across a river sends the unit to a *different* bank cell than
retail, and retail's choice changes with the frame the click lands on while
VERA's never does. Both are deterministic post-encoding, so this is a placement
difference, not a desync.

**D3 — Height gate dropped.**
Native: FNPC `param_11 = 1` at `0x004DE4C9` (and `0x004DE40D`) → a candidate is
admitted only while `abs(seedLevel − 4·candidateBridge − candidateLevel) < 2`
(`0x0056DE0x` block, and `find_nearby_cell.rs:69-80` documents the same
arithmetic).
Rust: `movement_path.rs:349` sets `check_height: false`.
Trigger: a refused goal whose ring neighbourhood spans a cliff edge or a ramp.
Frequency: UNCHECKED (instrument: count clicks whose 32-ring candidate set
contains a >1-level candidate on a retail map — `spatial_oracle` FNPC lane).
Effect: VERA can park the unit on a plateau two levels above the clicked cell
where retail would keep searching outward and land it on the near bank.

**D4 — Per-candidate playfield gate bypassed.**
Native: `MapClass::Is_Cell_In_Playfield_CellClass(candidate, 1)` runs before
`CheckPassability` on every candidate (four call sites inside `0x0056DC20`).
Rust: `movement_path.rs:350` sets
`anchor_gate: NearbyAnchorGate::UnverifiedCompatibilityBypass`; the enum's own
doc (`find_nearby_cell.rs:117-130`) says the gate is mandatory natively.
Trigger: a retarget whose candidate ring reaches the map border.
Frequency: map-edge clicks only; UNCHECKED as a rate.
Effect: VERA can retarget onto a border cell retail refuses.

**D5 — Zone test widened.**
Native: `0x0056D100` returns `GetZoneID(A) == GetZoneID(B)` — exact equality.
Rust: `resolve_reachable_move_goal` calls `ZoneGrid::can_reach`
(`zone_map.rs:967`), which falls back to `super_zones.are_connected` /
`zone_graph_connected` adjacency BFS when the ids differ.
Trigger: a goal in a zone adjacent-but-not-equal to the mover's.
Frequency: UNCHECKED (instrument: compare `can_reach` against
`can_reach_native` over a retail map's zone pairs — both already exist in
`zone_map.rs`, so this is a cheap Rust-side differential test).
Effect: VERA accepts the clicked cell verbatim where retail retargets.
The same function's doc already records the two off-playfield escapes it does
**not** implement (`zone_map.rs:955-966`), which refuse map-border orders retail
accepts.

**D6 — Evaluation order and substitution count.**
Native: exactly one substitution, at click time, before the mission is built
(`0x004DE1D0` → `0x004D806F`).
Rust (vehicle path): three, in sequence —
(a) `nearest_walkable_any_layer(clicked, 12)` at input time, no zone
requirement, no height gate, no playfield gate (`commands.rs:852-858`);
(b) `resolve_requested_move_goal(…, max_radius = 10)` inside `try_start_move`
(`movement_commands.rs:691`), which **refuses the order outright** when it finds
nothing ("No walkable cell near … cannot issue move");
(c) `resolve_reachable_move_goal` only after A\* has already failed
(`movement_commands.rs:828-849`).
Trigger: any vehicle move order onto a non-walkable cell.
Frequency: every such order (Rust-established).
Effect: the goal that reaches the sim is already a substitute chosen by a rule
with no native counterpart, and VERA's radius-10 stage can refuse an order that
native's radius-32 FNPC would have answered.

**D7 — Shroud drop absent on the vehicle path.**
Native: `0x004DE26B`/`0x004DE284` return `{0,0}` — the click is consumed and no
mission is emitted — when `TypeClass+0xC8D == 0` and the destination is
shrouded; and for `MoveToShroud=yes` movers a shrouded destination still forces
the own-zone FNPC retarget rather than the verbatim cell (`0x004DE45D`).
Rust: the vehicle path has no shroud term at all; `coordinate_is_shrouded` is
only reached from `resolve_walk_cell_click`.
Trigger: clicking a shrouded cell.
Frequency of the *drop*: low — in this worktree's `ini/rulesmd.ini`,
`MoveToShroud=no` appears on exactly `[CLEG]` (4140), `[CCOMAND]` (4222) and
`[CIVAN]` (4705); no `[VehicleTypes]` entry sets it, and the non-aircraft default
is yes. Frequency of the *forced retarget*: UNCHECKED, but it usually resolves
to the clicked cell anyway because that cell is the ring-0 candidate.
Effect: mostly none in stock; the Chrono Legionnaire case is a Teleport
locomotor and is outside both paths today.

**D8 — `is_bridge_only_goal` refuses where retail searches (already recorded).**
Native: `FootClass::Find_Path 0x004D3920` has one clear-and-return-0 exit,
`vtable+0x2CC` at `0x004D3989`; nothing gates the search on a bridge layer.
Rust: `movement_path.rs:179-186` drops the order when the goal's ground plane is
impassable but its bridge deck is walkable.
Trigger: clicking a bridge deck over water or a cliff.
Frequency: the existing comment says "a few times a match on any map with a high
bridge" — I did not re-establish that; treat the rate as UNCHECKED.
Effect: VERA refuses the order outright; retail runs the search.

**D9 — `+0x1AC` never consulted at admission on either VERA path.**
Native: `0x00700B75` runs `Can_Enter_Cell(cell,-1,-1,0,1)` and the `<=1`
threshold decides the **cursor**, plus (via `0x00738930`) an outright refusal
when `UnitClass+0x6E0` is set.
Rust: `ordinary_cell_move_goal` and `resolve_walk_cell_click` contain no
`Can_Enter_Cell` call; the cursor and the `+0x6E0` refusal have no counterpart I
found.
Trigger: a NOMOVE-cursor click by a unit in the `+0x6E0` state.
Frequency: UNCHECKED — depends on `+0x6E0`'s identity, itself UNCHECKED.
Effect: for ordinary units none (NOMOVE issues a Move anyway); for the `+0x6E0`
state VERA would accept an order retail silently drops.

**D10 — The Move mission's second cell is not carried.**
Native: `Player_Assign_Mission(2, 0, CellClass(resolved), CellClass(clicked))` —
the *clicked* cell is passed alongside the resolved one (`0x004D8095..` then the
`+0x378` call).
Rust: `Command::Move` carries one `(target_rx, target_ry)`.
Trigger: every cell-click move order.
Frequency: every order (Rust-established).
Effect: UNCHECKED — whether any consumer of mission 2 reads the fourth argument
is unexamined (instrument: decompile the `+0x378` implementation
`TechnoClass::Player_Assign_Mission` and follow argument 4).

---

## 3. UNCHECKED items and the instrument for each

| # | Item | Instrument that would settle it |
|---|---|---|
| U1 | Identity of `UnitClass+0x6E0` (the `0x00738930` refusal condition) | ReadINI key-string xrefs on the `UnitTypeClass` writers, or a breakpoint on `0x00739D21`/`0x00739B6A` while deploying a stock deployer |
| U2 | Whether any mission-2 consumer reads the fourth (`clicked`) argument | decompile `TechnoClass::Player_Assign_Mission` (`vtable+0x378`) and trace argument 4 |
| U3 | `FUN_0053A130(owner)` — the house predicate that gates `mz 9 → 7` and the Infantry `+0xD94` arm | decompile `0x0053A130`; it is a `HouseClass` method (`ECX = this+0x21C`) |
| U4 | Candidate-count distribution behind D2 (how often frame-modulo vs nearest actually differ) | debugger watch on `0x0056E5B3` (`local_1c8`/`local_1c4`), or a `spatial_oracle` FNPC lane over retail-map water/cliff clicks |
| U5 | Rate of D3/D4 (height gate, anchor gate) on retail maps | same FNPC lane, comparing the admitted candidate sets with `param_11`/anchor on and off |
| U6 | Whether `ZoneGrid::can_reach`'s adjacency widening ever disagrees with `can_reach_native` on a retail map | pure Rust differential: both functions already exist in `zone_map.rs`; iterate cell pairs on a loaded retail map |
| U7 | Re-validation of the `walk_move_admission` corpus the `0x004DE1D0` plate comment cites | rerun `tools/spatial_oracle/walk_move_admission.py` against the current binary and record the SHA1/freeze hash |
| U8 | Whether the `0x004D7DBC` out-of-playfield refusal (`!inPlayfield && action != 2`) is reachable for a player click at all | breakpoint at `0x004D7DC1` during a border click, or trace whether the display can ever hand `ClickedAction_Cell` an off-playfield cell |

---

## 4. Proposed ledger row

Replace the current A1 row's "Retail behaviour to match" text, which misstates
the mechanism, and add the criterion's status line:

> | A1 | Order admission | A cell click always emits Mission 2 with a **resolved** cell. `TechnoClass::What_Action 0x00700600` picks the cursor only (`Can_Enter_Cell +0x1AC (cell,-1,-1,0,1) <= 1` → MOVE, `>= 2` → NOMOVE); both actions reach `FootClass::ClickedAction_Cell 0x004D7D50` → `FUN_004DE1D0` (tables `0x004D8244`/`0x004D8270`). The resolver drops the order only for shroud without `MoveToShroud` (`+0xC8D`, `0x004DE284`), an off-playfield cell with action != 2 (`0x004D7DBC`), the `+0x298`/`+0x2A8` limbo gates, `UnitClass+0x6E0` with NOMOVE (`0x00738930`), or an empty FNPC result (`0x004D808F`). A failed `Can_Reach_Zone 0x0056D100` (**exact** zone-id equality) never refuses: it retargets via `FNPC 0x0056DC20` seeded at the clicked cell, required zone = `GetZoneID(ownCell)` (`0x004DE506`), height gate on, per-candidate playfield gate on, and selects `candidates[g_CurrentFrameCounter % N]` because the selection reference is the zeroed null cell (`0x004DE4D7`). | `tools/spatial_oracle/walk_move_admission` native corpus; Ghidra `0x00700600`, `0x004D7D50`, `0x004DE1D0`, `0x0056DC20`, `0x0056D100` |

> **A1 status: PARTIAL.** Walk movers run a faithful port
> (`resolve_walk_cell_click`, `src/sim/world/move_cell_input.rs`); the prior
> corpus behind it is cited, not re-demonstrated here. **Every non-Walk mover —
> all stock vehicles — takes a different mechanism** (`ordinary_cell_move_goal`
> compatibility adapter → `resolve_requested_move_goal` → post-A\*
> `resolve_reachable_move_goal`) that diverges in selection rule (D2), height
> gate (D3), candidate playfield gate (D4), zone-test strictness (D5),
> substitution count and evaluation order (D6), shroud handling (D7) and the
> bridge-only refusal (D8). Closing A1 requires extending the native receiver to
> Drive/Hover — not further tuning of `resolve_reachable_move_goal`.

---

## 5. Native addresses used (index)

`0x00417BD0` base `Active_Click_With(object)` · `0x004D3810`
`FootClass::CanReachDestination` (Find_Path's `+0x2CC` gate) · `0x004D3920`
`FootClass::Find_Path` (exit `0x004D3989`) · `0x004D74E0`
`ClickedAction_Object` · `0x004D7D50` `ClickedAction_Cell`
(`0x004D7D58`, `0x004D7D6D`, `0x004D7DA6`, `0x004D7DB3`, `0x004D7DBC`,
`0x004D7DCC`, `0x004D806F`, `0x004D808F`) · `0x004D8244`/`0x004D8270` switch
tables · `0x004DDDE0` `FootClass::What_Action_OnCell` · `0x004DE1D0` cell-click
destination resolver (`0x004DE26B`, `0x004DE284`, `0x004DE2B7`, `0x004DE2E3`,
`0x004DE33F`, `0x004DE3BE`, `0x004DE455`, `0x004DE471`, `0x004DE4B4`,
`0x004DE4BD`, `0x004DE4D7`, `0x004DE506`, `0x004DE528`, `0x004DE562`) ·
`0x004DE620` `ObjectClass::IsHighFlying` · `0x0051BF90`
`InfantryClass::Can_Enter_Cell` · `0x0051F190`/`0x0051F250` Infantry click
receivers · `0x0051F800` `InfantryClass::What_Action_OnCell` · `0x0053A130`
house predicate (unidentified) · `0x0056D100` `MapClass::Can_Reach_Zone` ·
`0x0056D230` `MapClass::GetZoneID` · `0x0056DC20`
`Find_Nearby_Passable_Cell` (`0x0056DCE1` radius, `0x0056E5B3` selection) ·
`0x0056E7C0` `CellRect::CheckPassability` · `0x00578460`
`Is_Cell_In_Playfield` · `0x00578540` `Is_Cell_In_Playfield_CellClass` ·
`0x00586360` `IsShrouded` · `0x00700600` `TechnoClass::What_Action(cell)`
(`0x00700AF6`, `0x00700B75`, `0x00700B7B`) · `0x00738890`/`0x00738910`
`UnitClass` click receivers · `0x0073F0A0` `UnitClass::Can_Enter_Cell`
(`RET 0x14` at `0x0073FD43`) · `0x0073FD50` `UnitClass::What_Action_OnObject` ·
`0x007404B0` `UnitClass::What_Action_OnCell`.
Data: `0x007F5C70`/`0x007EB058`/`0x007E22A4` vtables; `0x008B3D88` and
`0x00ABD480` both read as `{0,0}`.
