# A4 "Column following" — examination against gamemd.exe

Criterion under examination: acceptance row **A4** of
[2026-09-15-movement-retail-acceptance.md](../plans/2026-09-15-movement-retail-acceptance.md) —
*"Followers chain onto an occupant in transit (`0x0073FA2C..FA7C`, code 2)
instead of stopping."*

Question put: when a group of units moves along one route, does VERA20k chain
followers onto an occupant in transit the way retail does, instead of stopping
or scattering?

Binary: `gamemd.exe`, Ghidra project `testProsjekt`, program `/gamemd.exe`
(the only open program on the TCP bridge at `127.0.0.1:8089`, PID 29484).
Tree: worktree `vera20k-yuris-revenge-movement-bc5040`, branch
`feature/vera20k-drive-residual-budget` at `e933da49`.

Evidence classes used below, per the project contract:

* **Native established** — read from the disassembly of `gamemd.exe` in this
  session (address ranges cited per claim).
* **Rust read** — read from the tree in this session.
* **Parity demonstrated** — an executable native/Rust comparison, cited with
  its harness and coverage bound.

Nothing in this report is a parity certificate for A4. No native execution or
capture was run in this session; every native claim is a static read.

---

## 1. The native mechanism, as read

`UnitClass::Can_Enter_Cell` is `0x0073F0A0..0x0073FD45`. Its object-list walk
loops `ESI = [ESI+0x30]` and re-enters at `0x0073F528` (`0x0073FA87..FA8C`).
The ally arm of that walk has four layers, in this order:

### 1.1 Ally gate — `0x0073F8C0..0x0073F8CE`

```
0073f8c0  MOV  ECX,[EBX+0x21c]        ; mover's Owner house
0073f8c6  PUSH ESI                    ; the occupant
0073f8c7  CALL 0x004f9a90             ; HouseClass::Is_Ally_ByObject
0073f8cc  TEST AL,AL
0073f8ce  JZ   0x0073faff             ; not allied -> the enemy arm
```

Everything below runs only for an **allied** occupant. A non-ally leaves at
`0x0073FAFF` (`CALL 0x0040DD20`, then `CMP [EAX+0x220],2`), which is a
different arm and outside A4.

### 1.2 "Is the occupant moving?" — `0x0073F865..0x0073F8BF`

```
0073f865  TEST byte ptr [ESI+0x14],0x4 ; occupant is a FootClass
0073f869  JZ   0x0073f8b3              ;   no -> [ESP+0x28]=0, [ESP+0x16]=0
0073f86b  MOV  EAX,[ESI+0x5a4]         ; Destination
0073f871  MOV  [ESP+0x28],ESI          ; remember the occupant
0073f877  JNZ  0x0073f8ac              ;   Destination != 0      -> moving
0073f879  LEA  ECX,[ESI+0x388]
0073f87f  CALL 0x004c9480              ; FacingClass::Is_Rotating
0073f886  JNZ  0x0073f8ac              ;   rotating              -> moving
0073f888  MOV  EAX,[ESI+0x674]         ; the occupant's ILocomotion
0073f8a2  PUSH EAX
0073f8a3  MOV  ECX,[EAX]
0073f8a5  CALL dword ptr [ECX+0x10]    ; locomotor slot +0x10
0073f8aa  JZ   0x0073f8bb              ;   false                 -> not moving
0073f8ac  MOV  byte ptr [ESP+0x16],0x1
```

So native's "moving" is a **three-term disjunction**:
`Destination != 0 || PrimaryFacing.Is_Rotating() || Locomotor->slot+0x10`.
`FacingClass::Is_Rotating` was corroborated from its body (`0x004C9480..94AC`:
a nonzero rotation amount at `+0x14` plus a `{start@+0x8, duration@+0x10}`
timer still live against the Frame global `0x00A8ED84`), not from its label.
The `+0x10` slot is the same locomotor slot the I9b row reads as `Is_Moving`
at `0x004B264D`; that binding is in-tree and was **not** re-proved here.

`[ESP+0x16] == 0` falls to `0x0073FADF`: `CALL [ESI vtable+0x2C]`,
`CMP EAX,6 / JZ 0x0073FCD0` (return 7), else the running code is raised to 6
(`0x0073FAF4..FAF9`) and the walk continues.

### 1.3 Head-on exit — `0x0073F8D4..0x0073FA26`

Reached only when the ally is moving. The mover's facing octant comes from
`[EBX+0x388]` via `0x004C93D0` and `((w>>12)+1>>1)&7`; the occupant's facing
word is read the same way and **reversed by `ADD CX,0x7FFF`** at `0x0073F914`
before its octant is taken. `CMP EBP,EDI / JNZ 0x0073FA2C` at `0x0073F98F`
escapes unless the two octants match (they face each other). Then a 3-D
lepton distance (`FILD/FMUL/FADDP` at `0x0073F9DA..F9FA`, `CALL 0x004CAC40`
sqrt, `CALL 0x007C5F00` ftol) is compared `CMP EAX,0x1FF / JG 0x0073FA2C` at
`0x0073FA10`; and finally the bearing word computed at `0x0073F976..F98A`
(`CALL 0x004CAE30`, `FSUB [0x007E2820]`, `FMUL [0x007E2818]`, ftol) must fall
in the mover's own octant — `CMP ECX,EBP / JZ 0x0073FCD0` at `0x0073FA24`,
which returns **7** from `0x0073FCD0..FCDF`.

### 1.4 The in-transit / infantry skip — `0x0073FA2C..0x0073FA7C`

```
0073fa2c  MOV  EDI,[ESP+0x28]          ; the ally occupant
0073fa30  MOV  AL,[EDI+0x6b6]          ; occupation-enabled byte
0073fa36  TEST AL,AL
0073fa38  JZ   0x0073fa46              ; in transit -> ask the locomotor
0073fa3a  MOV  EDX,[EDI]
0073fa3e  CALL dword ptr [EDX+0x2c]    ; RTTI id
0073fa41  CMP  EAX,0xf
0073fa44  JNZ  0x0073fa6d              ; not 0xF -> raise to 2
0073fa46  MOV  EAX,[EDI+0x674]         ; ILocomotion (null -> E_POINTER throw)
0073fa60  PUSH EAX
0073fa61  MOV  ECX,[EAX]
0073fa63  CALL dword ptr [ECX+0xa4]    ; Can_Use_Track
0073fa69  TEST AL,AL
0073fa6b  JZ   0x0073fa7c              ; FALSE -> SKIP the occupant entirely
0073fa6d  CMP  dword ptr [ESP+0x18],0x2
0073fa72  JGE  0x0073fa7c              ; running MAX, not a store
0073fa74  MOV  dword ptr [ESP+0x18],0x2
0073fa7c  MOV  EBP,[ESP+0x18]          ; continue the walk
```

**This is the A4 mechanism.** A moving ally that is in transit
(`Foot+0x6B6 == 0`) — or is RTTI `0xF` — is asked `Can_Use_Track` on
ILocomotion slot `+0xA4` (COM convention: `this` is pushed, `ECX` holds the
vtable). A **false** answer removes the occupant from the walk entirely; only
a true answer raises the running code to 2. A moving ally that is *not* in
transit and *not* `0xF` goes straight to the raise.

`Can_Use_Track` is `DriveLocomotionClass 0x004B4B00` / `ShipLocomotionClass
0x006A4130`; every other locomotor inherits the base slot `0x004B6640`
(`XOR AL,AL / RET 4`), so Walk, Hover, Teleport and friends always answer
false and are always skipped by this arm.

### 1.5 What the Drive body does with code 2 — `0x004B364D..0x004B36EF`

`DriveLocomotionClass::Process_Movement` is `0x004B2630..0x004B4766`. It calls
`+0x1AC` at four sites: `0x004B2B17`, `0x004B2FF9`, `0x004B34C0`, `0x004B4120`.
The dispatch named by the criterion belongs to the `0x004B34C0` site, whose
result is stored into the frame slot later read as `[ESP+0x18]`
(`MOV [ESP+0x1C],EAX` at `0x004B34CB`, after the `PUSH 0x1` at `0x004B34C9`).
Routing: `0x004B35E3 CMP EDX,3` → `0x004B36F4 CMP EDX,6` → `0x004B3944
CMP EDX,1` → everything else `JNZ 0x004B3607` → `0x004B3649` →

```
004b364d  CMP  EAX,0x2
004b3650  JNZ  0x004b3a97               ; other codes (4/5/... per I9b)
004b3656  MOV  EAX,[EBP+0xc]            ; the Foot
004b3659  MOV  CL,[EAX+0x6b7]
004b3661  JNZ  0x004b3690               ; latch already set -> do not re-arm
004b3663  MOV  byte ptr [EAX+0x6b7],0x1
004b367e  MOV  ECX,[ECX+0x1768]         ; Rules -> BlockagePathDelay
004b3684  MOV  [EDX],EAX                ; Foot+0x668 timer start = Frame
004b368d  MOV  [EDX+0x8],ECX            ; Foot+0x670 duration
004b3699  MOV  ESI,[ECX+0x640]          ; the path-delay timer
004b36b6  JNZ  0x004b3aa1               ; still running -> bail this tick
004b36ca  MOV  EDX,[ECX+0x668]          ; the just-armed timer
004b36e7  JNZ  0x004b39d1               ; still running -> BL = 0
004b36ed  MOV  BL,0x1                   ; expired        -> BL = 1
004b36ef  JMP  0x004b39d3
...
004b39fb  TEST BL,BL / SETNZ DL / INC EDX
004b3a0e  CALL 0x004d3920               ; FootClass::Find_Path(dest, 0, BL?2:1)
```

Three facts follow, all from the bytes above:

* **The code-2 arm calls no scatter.** `CellClass::Scatter_Objects
  0x00481670` has exactly four call sites in the whole of
  `Process_Movement` — `0x004B2DC0`, `0x004B327D`, `0x004B393A`,
  `0x004B4437` (exhaustive instruction search over the 2600 instructions of
  the function). None is inside `0x004B364D..0x004B36EF` or
  `0x004B39D1..0x004B3A0E`. And code 2 cannot fall into any of them by
  another route at either dispatch: at the `0x004B2FF9` site every non-6 code
  takes `JNZ 0x004B3282` at `0x004B3027` (or `JMP 0x004B3282` at `0x004B301F`
  for code 3), jumping **past** the scatter at `0x004B327D`; at the
  `0x004B34C0` site code 2 leaves through `0x004B36F4 CMP EDX,6 / JNZ`, which
  skips the code-6 ladder that ends at the scatter `0x004B393A`.
* **The wait is `[General] BlockagePathDelay`, armed once per episode.**
  `Rules+0x1768` is that key — the string at `0x0083D314` is
  `"BlockagePathDelay"`, read at `0x00673A0C..0x00673A31`, constructor
  default `0x3C` = 60 at `0x0066761E`. The `+0x6B7` latch suppresses
  re-arming (`JNZ 0x004B3690`), and is cleared only by actual motion
  (`Process_Drive_Track 0x004B1624` / `0x004B1FFF`, Ship `0x006A0CE4` /
  `0x006A1642`, Walk `0x0075BE11` / `0x0075BFD1`, Hover `0x00514528` /
  `0x005147DF`) or by `FootClass::Set_Destination_Internal 0x004D96C2`.
  So urgency goes 1 → 2 **once**, 60 frames after the first refusal, and
  stays 2 for the rest of the block episode.
* **The retry budget `Foot+0x64C` is not a wait timer.** `0x004B3285`
  stores the literal `0xA` into it, immediately after the `0x004B327D`
  scatter — but that store is on a tail reached by *every* code from the
  `0x004B2FF9` site, code 0 included (`CMP EAX,3 / JNZ`, `CMP EAX,6 / JNZ
  0x004B3282`). Its consumer is `0x004B2DC8..0x004B2DD3`: after the
  `0x004B2DC0` scatter, `if (Foot+0x64C > 0) { --Foot+0x64C; bail; }`. It is
  a decrement-per-pass retry counter, reloaded to 10, not a frame span.

---

## 2. What increment I3 established, and where its edges are

I3 (PR #373) landed the in-transit skip and the head-on exit; I3b (PR #376)
landed the Hover `+0x6B6` lifecycle. Read against the binary, the parts of
§1 they cover are §1.3 and §1.4, plus the `+0x6B6` producers. §1.1 is covered
by the alliance gate. **§1.2 and §1.5 are the parts I3 did not cover**, and
they are where every divergence below sits.

---

## 3. What matches

| # | Claim | Evidence |
|---|---|---|
| M1 | The skip predicate and its ordering: in transit **or** infantry → ask `+0xA4`; false skips with no code, true raises to 2; otherwise raise to 2 directly. | Native established `0x0073FA2C..FA7C`. Rust read: `cell_entry::classify_blocker` (`src/sim/pathfinding/cell_entry.rs:1696-1712`) and `bump_crush::build_entity_block_sets` (`src/sim/movement/bump_crush.rs:166-207`) implement the same two-branch shape over `foot_occupation_enabled` and `EntityCategory::Infantry`. |
| M2 | `Can_Use_Track` itself. | Parity demonstrated, per the ledger's I3 row: `tools.spatial_oracle.locomotor_can_use_track`, 12,320 cases, pinned by `drive_track_tests::occupant_can_use_track_matches_native_oracle`. Not re-run in this session; the coverage bound is that oracle's case set, not the whole locomotor. |
| M3 | Only an **allied** occupant reaches the arm. | Native `0x0073F8C7` (`HouseClass::Is_Ally_ByObject 0x004F9A90`). Rust: `classify_blocker` returns `OccupiedEnemy` before the moving branch. |
| M4 | The running code is a **max**, taken over the whole cell list, not the first occupant. | Native `0x0073FA6D CMP [ESP+0x18],2 / JGE`. Rust: `classify_occupied_cell_with_slave_query` walks every occupant into `worst` (`cell_entry.rs:1347-1441`). |
| M5 | Head-on exit: reversal by `+0x7FFF`, 3-D `Sqrt_Approx` + `ftol`, `> 0x1FF` escape, bearing in the mover's octant, Unit-only, and evaluated **before** the locomotor question. | Native `0x0073F8D4..FA26`. Rust: `cell_entry::head_on_exit` (`cell_entry.rs:1602-1640`) reproduces each step including the `X87Chop53` chain; ordering pinned by `classify_blocker_head_on_exit_precedes_the_transit_skip_for_unit_movers`. |
| M6 | A follower may not *install a curve* on code 2 — only code 0 or a crush admits. | Native: code 2 leaves the dispatch through the wait/repath arm and never reaches an admission. Rust: `drive_track_chain_entry_allows_track_install` (`movement_tick.rs:1287-1292`). |
| M7 | The code-2 repath is **per-tick**, with the timer selecting urgency and never suppressing the repath. | Native `0x004B39FB..0x004B3A0E` (`SETNZ DL / INC EDX`, then `Find_Path`). Rust prose and code at `movement_occupancy.rs:1159-1166` agree. |
| M8 | The leader releases its cell claim while in transit, so the mask arm does not block the follower either. | Rust read: `track_host.rs:495-508` gates the raw mark on the enable; `CellOccupationGrid::rebuild` (`src/sim/occupancy.rs:824-831`) skips a mover whose enable is clear. Native producers cited by the I3/I3b rows (`0x004B161A`/`0x004B1FEF`, `0x006A0CDA`/`0x006A1632`, `0x005147D5`/`0x0051451E`); not re-read here. |

---

## 4. Divergences

### D1 — the "moving" test is one term where native has three

* **Native:** `0x0073F86B` (`Destination`), `0x0073F87F` →
  `FacingClass::Is_Rotating 0x004C9480`, `0x0073F8A5` (locomotor slot
  `+0x10`), all gated on `TEST byte [ESI+0x14],0x4` at `0x0073F865`.
* **Rust:** `classify_blocker` uses `blocker.movement_target.is_some()` alone
  (`cell_entry.rs:1696`); `build_entity_block_sets` the same
  (`bump_crush.rs:175`).
* **Trigger:** an allied occupant with no movement target that is still
  rotating, or whose locomotor still reports motion — a tank turning in place
  on a Guard order, a unit finishing its committed curve after a Stop, a
  hover gliding to a dropped Head_To.
* **Frequency:** UNCHECKED. The trigger is reachable in ordinary play, but
  how often a *follower* evaluates such an occupant was not measured.
* **Player-visible effect:** VERA classifies that ally as stationary and
  takes the code-6 arm, which **scatters the blocker**
  (`movement_occupancy.rs:845-880`). Retail takes the moving arm: head-on
  test, then skip-or-code-2, and no scatter. The visible symptom is a unit
  being shoved out of formation while it is merely turning.

### D2 — a VERA-only `next_cell != pos` gate on the moving arm

* **Native:** no such term anywhere in `0x0073F865..0x0073FA7C`.
* **Rust:** `bump_crush.rs:177-179` requires `mt.path.get(mt.next_index)` to
  exist **and** to differ from the occupant's own cell before the moving arm
  is entered; otherwise the occupant falls through to the code-6 stationary
  insert at `bump_crush.rs:210-217`.
* **Trigger:** a moving ally whose path head is its own cell — the state a
  repath leaves behind when it re-targets the cell the mover stands in.
* **Frequency:** UNCHECKED.
* **Player-visible effect:** same shape as D1 — a moving ally is treated as
  stationary and scattered.

### D3 — VERA scatters on code 2; retail does not

* **Native:** established in §1.5. Four `Scatter_Objects` sites in
  `Process_Movement`, none reachable from either dispatch's code-2 path.
* **Rust:** the `TemporaryBlock | TemporaryOccupation` arm of
  `handle_deferred_occupancy` calls `bump_crush::scatter_blocker`
  (`movement_occupancy.rs:1177-1195`) for any non-Walk mover on the first
  block and again on every grace expiry.
* **Trigger:** any Drive/Ship mover refused by a moving ally that raised
  code 2 — i.e. exactly the case A4 is about, once the leader is *not* in
  transit (stopped at a cell centre with a live destination, or a Walk/Hover
  ally whose enable is set).
* **Frequency:** structural — every code-2 refusal on the runtime crossing
  path takes this arm.
* **Player-visible effect:** the column's leader gets a scatter order and
  steps off the route; retail leaves it alone and repaths the follower. This
  is the "scatters instead of chaining" symptom A4 names, arriving through
  the *response* rather than the *classification*.
* **Note:** the tree already records the correct native fact — the doc
  comment on `drive_track_chain_code2_refuses_track_without_scattering`
  (`movement_tick.rs:4897-4915`) says the code-2 arm has no `Scatter_Objects`
  — so production and documentation currently contradict each other. That
  same comment cites `0x004B3225` as a scatter site in the blocked block;
  the exhaustive search says the site there is **`0x004B327D`**, and
  `0x004B3225` is not a call to `0x00481670`.

### D4 — the code-2 wait: wrong span, wrong constant, wrong re-arm rule

* **Native:** `+0x6B7` latch + `+0x668` timer, duration
  `Rules+0x1768 = [General] BlockagePathDelay` (string `0x0083D314`, ctor
  default 60 at `0x0066761E`), armed **once** per block episode
  (`0x004B3661 JNZ 0x004B3690`), cleared only by motion or a new
  destination. Urgency latches at 2 after one span.
* **Rust:** `POST_SCATTER_WAIT_FRAMES = 10` (`bump_crush.rs:1037`) re-armed
  on every pass (`movement_occupancy.rs:1203-1212`), producing the 10-frame
  sawtooth its own test name advertises
  (`code_two_post_scatter_wait_rearms_on_every_pass_while_the_block_holds`).
* **Source of the 10:** `Foot+0x64C = 0xA` at `0x004B3285`. §1.5 shows that
  store is a **retry-counter reload** on a tail every code reaches, code 0
  included, consumed by the decrement at `0x004B2DC8..0x004B2DD3`. It is not
  a wait span, and it is not on the code-2 arm.
* **Trigger:** every code-2 block episode.
* **Frequency:** structural.
* **Player-visible effect:** repath urgency oscillates 1→2→1→2 every 10
  frames where retail escalates once at 60 frames and holds, so a blocked
  follower re-solicits a different route six times as often; combined with
  D3 it also re-scatters the leader every 10 frames.

### D5 — the Drive selection lane discards every infantry entry

* **Native:** only a **moving** infantry ally is skipped, and only because
  Walk's `+0xA4` inherits the always-false base (`0x004B6640`). A
  **stationary** ally infantryman still raises code 6 (`0x0073FAF4`), and an
  enemy infantryman goes down the enemy arm at `0x0073FAFF`.
* **Rust:** `DriveCellAdmission::refusal_code`
  (`movement_step.rs:1386-1388`) applies
  `.filter(|entry| !entry.blocker_is_infantry)` to the whole block-map
  lookup, so codes 5 and 6 carried by infantry never refuse a fresh curve.
* **Trigger:** a vehicle selecting a fresh Drive/Ship curve into a cell
  holding infantry.
* **Frequency:** UNCHECKED.
* **Player-visible effect:** vehicles commit curves into infantry-occupied
  cells that retail would price or refuse. Recorded rather than proposed as
  a fix: sub-cell infantry occupancy and the crush latch both bear on this
  arm, and narrowing the filter without reading them would be the kind of
  single-gate change the I9b row warns about.

### D6 — the Drive selection lane answers from a cached, stale snapshot

* **Native:** the walk evaluates every occupant live, inside the same call.
* **Rust:** `refresh_owner_block_set_if_stale` (`movement_tick.rs:392-406`,
  called at `:1827`) rebuilds the per-owner block map — which carries both
  the code map and the `MovingAllyOccupant` facing/world record used by
  `head_on_exit` — only when `occupancy.generation()` advances. A mover that
  rotates, translates within its cell, or flips `foot_occupation_enabled`
  does none of that.
* **Trigger:** any ally whose facing, sub-cell position or transit state
  changes without a cell change — i.e. most of every crossing.
* **Frequency:** structural on the Drive selection lane. The live deferred
  walk (`handle_deferred_occupancy` → `classify_blocker`) reads entities
  fresh and is **not** affected.
* **Player-visible effect:** a head-on refusal or a code-2 refusal decided
  from facings and transit states up to a cell-crossing old; a follower
  admitted or refused a tick or two off.

### D7 — the non-Foot allied occupant

* **Native:** `TEST byte [ESI+0x14],0x4` at `0x0073F865` gates the whole
  "moving" computation; a non-Foot ally lands at `0x0073FADF`, where
  `vtable+0x2C == 6` returns **7** and anything else raises code 6.
* **Rust:** `classify_blocker` maps `EntityCategory::Structure` to
  `Impassable` and every other non-moving ally to `FriendlyStationary`
  (`cell_entry.rs:1677-1713`).
* **Trigger:** an allied non-Foot object sharing the target cell.
* **Frequency:** UNCHECKED.
* **Player-visible effect:** depends entirely on U1 below; if `6` is not
  BuildingClass the Structure mapping is wrong in both directions.

---

## 5. UNCHECKED, with the instrument that would settle each

| # | Open question | Instrument |
|---|---|---|
| U1 | The RTTI ids: is `vtable+0x2C == 0xF` InfantryClass, and `== 6` BuildingClass? Both gate arms VERA already ports. | Read the `+0x2C` bodies off the InfantryClass and BuildingClass vtables located from the RTTI/COL substrate (project memory records that substrate as reliable), or set a debugger breakpoint at `0x0073FA3E` and `0x0073FAE3` with a known occupant of each class. A brute instruction search for `MOV EAX,0xF` is too noisy to settle it. |
| U2 | Does a retail column actually exercise the §1.4 skip, or does the mask release alone carry it? A4's whole claim rests on the answer. | A retail `-win` capture of four or more tanks on one waypoint route, measuring hull separation per frame, against the same order in VERA. `derived_min_transit_separation_leptons` (`world_tests.rs:8668-8692`) is a VERA-side ratchet derived from the shipped curve tables — it is not a native measurement and cannot settle this. |
| U3 | Frequency for D1, D2, D5, D7. | Count each arm over a scripted convoy run in VERA, and count the native side with breakpoints at `0x0073F8AC` / `0x0073F8BB` (which of the three "moving" terms fired) and `0x0073FAF4`. |
| U4 | The `0x004B3060..0x004B3260` range of the `0x004B2FF9` dispatch was not read. The no-scatter-on-code-2 conclusion does not depend on it (every non-6 code jumps past the scatter at `0x004B3027`/`0x004B301F`), but the `+0x64C` semantics on that lane are only partly traced. | Read the range, or breakpoint `0x004B3285` and log the incoming code. |
| U5 | Ship, Walk and Hover code-2 arms. They were seen only as readers of the same `+0x6B7` latch and `Rules+0x1768` (`0x006A2CA8`/`0x006A2CCD`, `0x0075B8AC`/`0x0075B8D1`, `0x00515D30`/`0x00515D55`); their dispatch bodies are unread. VERA runs one shared arm for all of them. | Disassemble `0x006A2CA8..`, `0x0075B8AC..` and `0x00515D30..` the way §1.5 does for Drive. |
| U6 | Whether D6's staleness changes an outcome in practice. | A debug-only comparison of the cached block map against a fresh build at each Drive selection, run over a convoy fixture; it must not be a `debug_assert!` with a side effect (see the release-only-bug memory). |
| U7 | Whether `Find_Path`'s third argument is genuinely an urgency/threat level. The call shape `Find_Path(cell, 0, BL ? 2 : 1)` is established; the parameter's meaning is not. `ECX` is the Foot, carried from `[EBP+0xC]` through both entries into `0x004B39D3`. | Read `FootClass::Find_Path 0x004D3920..0x004D41F2`'s use of its third parameter. |

---

## 6. Verdict on A4

**Not accepted.** The classification half of the criterion is in good shape:
the in-transit skip, its ordering against the head-on exit, the running-max
raise and the alliance gate all match the binary, and `Can_Use_Track` is the
one piece with a demonstrated parity result behind it. A follower does chain
onto a leader that is genuinely in transit.

The criterion still fails on two counts, both outside what I3 scoped:

1. **The predicate that decides whether an ally is "moving" at all** is one
   term where the binary has three (D1), with a VERA-only fourth condition
   bolted on in the A\* lane (D2). Misclassifying a moving ally as stationary
   sends it down the scatter arm — the exact behaviour A4 says must not
   happen.
2. **The code-2 response scatters and waits on a wrong, re-armed span**
   (D3, D4). Retail's code-2 arm only repaths, and escalates urgency once at
   `BlockagePathDelay`. VERA's scatters the leader and re-scatters it every
   ten frames.

D3 and D4 are the ones with structural frequency and a direct visible
symptom, and D4's constant rests on a store the binary shows to be a retry
counter, not a wait. They should lead any follow-up increment.

---

## 7. Proposed ledger row

Add to the increment table of
`docs/plans/2026-09-15-movement-retail-acceptance.md`:

> | I3c Moving-ally predicate and the Drive code-2 response | A4, A5 | OPEN (examined 2026-09-16) | Examination:
> [MOVEMENT_ACCEPTANCE_A4_COLUMN_FOLLOWING_EXAMINATION.md](../research/MOVEMENT_ACCEPTANCE_A4_COLUMN_FOLLOWING_EXAMINATION.md).
> Native established: the "is the occupant moving" gate is a three-term
> disjunction — `Destination` (`0x0073F86B`), `FacingClass::Is_Rotating`
> (`0x0073F87F` → `0x004C9480`, body read) and locomotor slot `+0x10`
> (`0x0073F8A5`) — behind a Foot test (`0x0073F865`); VERA uses
> `movement_target` alone (`cell_entry.rs:1696`, `bump_crush.rs:175`) plus a
> VERA-only `next_cell != pos` gate (`bump_crush.rs:177`), so a rotating or
> coasting ally is classified stationary and **scattered**. Native established:
> the Drive code-2 arm (`0x004B364D..0x004B36EF` → `0x004B39D3` →
> `Find_Path 0x004D3920` at `0x004B3A0E`) calls no `Scatter_Objects` — the
> four sites in `Process_Movement` are `0x004B2DC0`, `0x004B327D`,
> `0x004B393A`, `0x004B4437` and code 2 jumps past all of them
> (`0x004B3027`, `0x004B301F`, `0x004B36F4`) — and arms its `+0x668` timer
> **once** per episode with `Rules+0x1768 = [General] BlockagePathDelay`
> (string `0x0083D314`, ctor default 60 at `0x0066761E`), the `+0x6B7` latch
> (`0x004B3663`) suppressing re-arming until motion (`0x004B1624`/
> `0x004B1FFF`) or `Set_Destination_Internal 0x004D96C2` clears it, so
> urgency latches at 2. VERA scatters on code 2
> (`movement_occupancy.rs:1177`) and re-arms a 10-frame span every pass; that
> 10 comes from `Foot+0x64C = 0xA` at `0x004B3285`, which the binary shows to
> be a **retry-counter reload** on a tail every code reaches (code 0
> included) and consumed by the decrement at `0x004B2DC8..0x004B2DD3` — not a
> wait span, and not on the code-2 arm. Also recorded: the Drive selection
> lane drops every infantry block entry (`movement_step.rs:1387`) where
> native skips only *moving* infantry, and answers the head-on exit and the
> code map from a snapshot rebuilt only on `occupancy.generation()`
> (`movement_tick.rs:392`, `:1827`), which no rotation or in-cell translation
> advances. Matching and unchanged: the skip predicate and its ordering, the
> running-max raise, the alliance gate, the head-on arithmetic, and
> `Can_Use_Track` (parity demonstrated, `tools.spatial_oracle.locomotor_can_use_track`,
> 12,320 cases). Documentation drift found: the comment on
> `drive_track_chain_code2_refuses_track_without_scattering`
> (`movement_tick.rs:4897`) states the correct native fact that the
> production code-2 arm violates, and cites `0x004B3225` for a scatter site
> that is actually `0x004B327D`. UNCHECKED: RTTI ids `0xF`/`6`; whether a
> retail column exercises the skip rather than the mask release alone
> (needs a `-win` capture — `derived_min_transit_separation_leptons` is a
> VERA ratchet); frequencies for the moving-predicate and infantry-filter
> divergences; the Ship/Walk/Hover code-2 arms (`0x006A2CA8`, `0x0075B8AC`,
> `0x00515D30`), which VERA serves with one shared arm. |

And amend the A4 acceptance row's status to cite this examination rather than
resting on I3/I3b alone.
