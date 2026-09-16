# A5 "Blocked recovery" — read-only examination

Scope: `Foot+0x64C` retry counter, `CloseEnough`, `Foot+0x640/+0x648` and
`Foot+0x668/+0x670` timers, the `Foot+0x6B7` latch, and blocker scatter via
`CellClass::Scatter_Objects 0x00481670`.

Binary: `gamemd.exe`, Ghidra project `testProsjekt`, image base 0x00400000,
10073 functions. All native statements below are from disassembly read in this
session (`disassemble_bytes` dry-run, `search_instructions`, `read_memory`,
one decompile of `0x00481670`). Rust statements are from the worktree's current
source, not from `docs/research/traces/`.

**Verdict: A5 is NOT accepted.** Ten divergences established, eight with a
player-visible effect; eight sub-behaviours match with evidence; six items
remain UNCHECKED.

---

## 0. Native ground truth (established this session)

### 0.1 Field identities

| Field | Native evidence |
|---|---|
| `+0x640` start / `+0x644` / `+0x648` duration | Movement-delay (PathDelay) timer. Constructor `0x004D3320` (= Frame) / `0x004D3326` (= 0). Read `0x004B2825..0x004B2845`, `0x004B3699..0x004B36B6`, Walk `0x0075B8EC..0x0075B909`. Re-armed `0x004B2865..0x004B286D` and `0x004B3A84..0x004B3A8C` with `ftol(Rules+0x1760 × 900.0)`. |
| `+0x64C` | Retry/attempt budget. Constructor `0x004D32DB MOV EAX,0xA` → `0x004D332C`. |
| `+0x668` start / `+0x66C` / `+0x670` duration | Blockage (escalation) timer. Constructor `0x004D335B` (= Frame) / `0x004D3361` (= 0). Duration source `Rules+0x1768`. |
| `+0x6B7` | Blocked latch, byte. Constructor `0x004D3451` = 0. |
| `Rules+0x1718` | `CloseEnough`, leptons. `rulesmd.ini` `CloseEnough=2.25` cells = 576. |
| `Rules+0x1760` | `PathDelay`, **double**, minutes. `rulesmd.ini` `PathDelay=.01`. Scale constant `0x007E27F8` = `0x408C200000000000` = **900.0** → 9 frames. |
| `Rules+0x1768` | `BlockagePathDelay`, dword. `RulesClass__Constructor 0x0066761E` writes 0x3C = **60**; `rulesmd.ini` `BlockagePathDelay=60`. |

### 0.2 Timer arithmetic (identical shape at every read site)

```
0x004B36A5  CMP ESI,-1        ; start == -1 -> remaining = duration verbatim
0x004B36A8  JZ  ...
0x004B36AA  MOV EDX,EDI / SUB EDX,ESI   ; elapsed = Frame - start
0x004B36AE  CMP EDX,EAX
0x004B36B0  JGE ...           ; elapsed >= duration -> expired
0x004B36B2  SUB EAX,EDX       ; remaining
0x004B36B4  TEST EAX,EAX
0x004B36B6  JNZ ...           ; nonzero remaining -> still running
```

### 0.3 The Drive code-2 arm, `0x004B364D..0x004B36EF`

```
004B364D  CMP  EAX,0x2
004B3650  JNZ  0x004B3A97          ; not code 2 -> wall/other dispatch
004B3659  MOV  CL,[EAX+0x6B7]
004B365F  TEST CL,CL
004B3661  JNZ  0x004B3690          ; ALREADY LATCHED -> skip the stamp entirely
004B3663  MOV  byte [EAX+0x6B7],1
004B366A..368D                     ; +0x668 = Frame, +0x66C = [ESP+0x54], +0x670 = Rules+0x1768
004B3690..36B6                     ; movement-delay gate (+0x640/+0x648); running -> JNZ 0x004B3AA1
004B36BC  MOV  AL,[ECX+0x6B7]
004B36C4  JZ   0x004B39D1          ; latch clear -> BL = 0
004B36CA..36E7                     ; blockage timer (+0x668/+0x670); running -> 0x004B39D1 (BL = 0)
004B36ED  MOV  BL,1                ; expired -> BL = 1
004B36EF  JMP  0x004B39D3
...
004B39FB  TEST BL,BL / SETNZ DL / INC EDX   ; urgency = 1 or 2
004B3A0E  CALL 0x004D3920                    ; Find_Path(cell, 0, urgency)
004B3A2F  JNZ  0x004B3A59                    ; success -> re-arm +0x640 with PathDelay, return TRUE
004B3A47  CALL [vtable+0x480](0,1)           ; both failed -> stop, return FALSE
```

There is **no `Scatter_Objects` call in the arm.** The four sites in
`Process_Movement` are `0x004B2DC0`, `0x004B327D`, `0x004B393A`, `0x004B4437`;
none lies between `0x004B3656` and `0x004B3A94`. Walk's `0x0075B891` scatter
sits in a sibling arm that `RET 0x4` at `0x0075B89D`, i.e. **before** the code-2
test `CMP ESI,0x2` at `0x0075B8A0`. Ship `0x006A2CA8..` and Hover
`0x00515D30..` repeat the Drive shape.

### 0.4 The retry counter's single decrement, `0x004B2D68` block

```
004B2C34/0x004B2C51  CoordStruct helper + Distance3D 0x0041C380
004B2C5C  CMP  EAX,[Rules+0x1718]
004B2C62  JGE  0x004B2D68          ; NOT close enough -> blocked handling
...                                 ; fall-through (dist < CloseEnough) -> stop / arrive
004B2D68  MOV  EAX,[EDI+0x140] / TEST AH,1   ; bridge-layer select for the scatter
004B2DC0  CALL 0x00481670                    ; Scatter_Objects(NullCoord, 1, 1, altlist)
004B2DC8  MOV  EAX,[ECX+0x64C]
004B2DCE  TEST EAX,EAX
004B2DD0  JLE  0x004B2DDE          ; EXHAUSTED -> give-up ladder
004B2DD2  DEC  EAX
004B2DD3  MOV  [ECX+0x64C],EAX
004B2DD9  JMP  0x004B2E77          ; ... and CONTINUE this pass
```

Exhausted ladder `0x004B2DDE..0x004B2E77`: if `Foot+0x598 == 0` (`0x004B2E1C`)
call `vtable+0x480(0,1)` (stop); then if `Foot+0x68A` is set play the blocked
voice through `Rules+0x700` (`0x004B2E51..0x004B2E68`) and clear `+0x68A`.
If `Foot+0x598 != 0` it jumps to `0x004B2F1D` and does **not** give up.

The mirror block `0x004B3090..0x004B3225` is structurally identical but its
tail **resets** the counter: `0x004B327D` `Scatter_Objects`, `0x004B3285`
`MOV dword [EAX+0x64C],0xA`.

Both blocks gate the `CloseEnough` compare behind three tests:

```
CALL 0x0047C3D0 (blocking-object lookup); EAX == 0 -> JZ (skip scatter + compare)
CALL 0x004F9A90 Is_Ally_ByObject;        AL == 0 -> JZ
CALL [vtable+0x84] -> [EAX+0xC94];       nonzero -> JNZ
```
(`0x004B2BD4/0x004B2BEB/0x004B2C04`, `0x004B30B9/0x004B30D0/0x004B30E9`).

Five `Rules+0x1718` compares exist in Drive: `0x004B297F`, `0x004B2C5C`,
`0x004B3141`, `0x004B37BE`, `0x004B42DB`. All five have the same polarity
`JGE → keep going`, i.e. `dist < CloseEnough → stop`.

### 0.5 `Set_Destination_Internal 0x004D96B0..0x004D9707`

`+0x6B7 = 0`; `+0x668 = Frame`, `+0x670 = Rules+0x1768` (60);
`+0x640 = Frame`, `+0x648 = 0` (ECX zeroed at `0x004D96F9`).
**`+0x64C` is not written.** (26-site program-wide `0x64c]` scan: no `0x004D96xx`
entry; the only writers are the constructor, the six reset sites, and the six
per-locomotor decrements.)

### 0.6 `CellClass::Scatter_Objects 0x00481670`

Decompile + the five locomotor call sites:

- list head `this+0xE4` (FirstObject) or `this+0xE8` (AltObject) by `param_5`;
  walk via `+0x30`.
- if `param_4 == 0`: pre-scan for an elite in-map object (`bVar1`).
- second pass collects up to **ten** entries (cap check `FUN_0040CE50(10,0)`) —
  the criterion's ten-entry cap is correct as written.
- dispatch in collected order: `(**(obj_vtable + 0x174))(param_2, param_3, param_4)`
  when `bVar1 || param_4 || Rules+0x17ED || HasWeaponAbility(3) ||
  Owner->IQ >= Rules+0x144C`.

Every locomotor site pushes `param_2 = 0x008A0790`, `param_3 = 1`,
`param_4 = 1` (force), `param_5 =` alt-layer flag. `read_memory 0x008A0790` = 16
zero bytes, so it is the NullCoord. With `force = 1` the eligibility disjunction
short-circuits true and the elite/IQ/PlayerScatter gate never applies at these
sites. `param_5` is computed from `[obj+0x140] & 0x100` and
`|Z/256 − cell_height| > 2` (`0x004B2D68..0x004B2DAB`, `0x004B3225..0x004B326D`,
`0x004B38F5..0x004B391B`, `0x004B4411..0x004B4429`, Walk `0x0075B862..0x0075B881`).

---

## 1. Divergences

### D1 — VERA scatters the blocker in the code-2 arm; native does not
- **Native:** §0.3. No `Scatter_Objects` between `0x004B3656` and `0x004B3A94`
  (Drive) or after `CMP ESI,2` at `0x0075B8A0` (Walk).
- **VERA:** `src/sim/movement/movement_occupancy.rs:1177` calls
  `bump_crush::scatter_blocker` under
  `CellEntryResult::TemporaryBlock | TemporaryOccupation`, whose
  `yr_code()` is 2 (`src/sim/pathfinding/cell_entry.rs:260`).
- **Trigger:** any Drive/Ship mover whose next cell holds a moving friendly.
- **Frequency:** continuous in ordinary skirmish — every convoy, rally-point
  queue and harvester crossing.
- **Player effect:** the blocker is nudged one cell where retail leaves it
  alone; the pair "shuffles" instead of one waiting. Also spends a scenario RNG
  draw per nudge, so it is a determinism surface, not only a cosmetic one.
- **Internal contradiction:** VERA's *other* production code-2 handler,
  `movement_tick.rs:1047 handle_deferred_drive_selection_block`
  (called from `movement_tick.rs:3152`), documents this same native finding in
  its doc comment and deliberately does **not** scatter. Two production code-2
  arms disagree with each other.

### D2 — `POST_SCATTER_WAIT_FRAMES = 10` is a misidentified `Foot+0x64C`
- **Native:** `0x004B3285` writes `10` into `[EAX+0x64C]`, the retry counter —
  the same field the constructor initialises to 10 (`0x004D32DB`/`0x004D332C`)
  and `0x004B2DD2` decrements. It is not a frame wait: the decrement branch
  `JMP 0x004B2E77` continues the same pass, so nothing "skips the move while it
  is positive". The genuine code-2 wait is `Rules+0x1768` = 60.
- **VERA:** `src/sim/movement/bump_crush.rs:1030-1038` documents it as a
  hardcoded 10-frame wait explicitly contrasted with `BlockagePathDelay`, and
  `movement_occupancy.rs:1209` writes it into `path_runtime.blocked_timer`.
- **Trigger/frequency:** as D1.
- **Player effect:** urgency escalates to 2 (the 1000x route-around) after 10
  frames instead of 60. Units abandon a short queue and take a long detour
  around a jam retail waits out — six times too early.
- Note `mod.rs:235` (`PATH_STUCK_INIT`) identifies `+0x64C` correctly, so the
  tree currently binds one native field to two incompatible meanings.

### D3 — the blockage timer is re-armed on every pass, not only on the latch transition
- **Native:** `0x004B3661 JNZ 0x004B3690` jumps over the `+0x668/+0x670` store
  at `0x004B366A..0x004B368D`, so the stamp happens only on the `0 → 1`
  transition of `+0x6B7`. Same branch in Walk (`0x0075B8B4 JNZ 0x0075B8E3`),
  Ship (`0x006A2CA8..`), Hover (`0x00515D30..`).
- **VERA:** `movement_occupancy.rs:1206-1213` arms it under
  `first_block || grace_expired`, i.e. again at every expiry. The comment there
  and in `movement_blocked.rs:285-298` asserts the store "sits straight-line
  after the scatter with no branch between them" — the branch is `0x004B3661`,
  and the straight-line store those comments are describing is the *different*
  one at `0x004B3A84`, which re-arms `+0x640`, not `+0x668`.
- **Player effect:** retail's code-2 grace is one-shot — after 60 frames the
  mover stays at urgency 2 for the rest of the block. VERA oscillates
  1→2→1→2 on a 10-frame cycle, so a jammed unit visibly flip-flops between
  queuing and detouring.

### D4 — `Foot+0x64C` is reset by VERA on events native never touches
- **Native:** the six reset sites are `0x004B3285` (Drive), `0x0075B2E2` /
  `0x0075BA27` (Walk), `0x006A28D5` (Ship), `0x00516BCD` (Hover),
  `0x005B05E0` / `0x005B0DAA`. `Set_Destination_Internal` does not write it
  (§0.5).
- **VERA:** `movement_commands.rs:436` and `:1210` replace the whole
  `FootPathRuntime` with `default()` on every accepted non-Walk destination,
  restoring `retries_left = 10`; `movement_tick.rs:590` does the same at each
  24-step segment repath; `movement_blocked.rs:254` resets on every successful
  repath.
- **Trigger:** any new order, any segment boundary, any successful repath.
- **Frequency:** every long move order (segments are 24 steps) and every player
  re-click.
- **Player effect:** the retail give-up — stop plus the "unable to comply"
  voice (`0x004B2E51..0x004B2E68`, `Foot+0x68A`) — is effectively unreachable in
  VERA for a long order. Retail units that have burned their budget stop and
  complain; VERA units keep grinding.

### D5 — what decrements `Foot+0x64C`, and the off-by-one
- **Native:** exactly one decrement site per locomotor (`0x004B2DD2` Drive,
  `0x0075B07A` Walk, `0x006A2423` Ship, `0x005167CD` Hover, `0x005B0383`),
  reached on any pass that lands on the allied-blocking-object label
  `0x004B2D68` / its scatter tail `0x004B2DC5`. Guard `TEST/JLE` → decrement
  only while `> 0`; the give-up is taken on the *next* pass at 0, i.e. eleven
  passes total.
- **VERA:** `movement_blocked.rs:270-292` decrements once per **urgency-2
  repath failure** only, and aborts as soon as the counter reaches 0 — ten
  failures.
- **Player effect:** different counted event and a one-pass-early abort. A VERA
  mover that repaths *successfully* but stays blocked never counts down at all;
  retail counts every blocked pass. A VERA mover inside its grace window
  (urgency 1) also never counts down.
- Native's exhausted ladder is additionally conditioned on `Foot+0x598 == 0`
  (`0x004B2E1C`); VERA's abort is unconditional.

### D6 — `CloseEnough` runs on block classes native never gates it on
- **Native:** the two compares inside the blocked ladder (`0x004B2C5C`,
  `0x004B3141`) are reachable only after a blocking object is found
  (`0x0047C3D0`), is an ally (`Is_Ally_ByObject 0x004F9A90`) and has
  `[type+0xC94] == 0` (§0.4). A cell that is simply impassable terrain returns
  `EAX == 0` from `0x0047C3D0` and jumps straight past both the scatter and the
  compare.
- **VERA:** `close_enough_abort = true` at
  `movement_step.rs:2513` (bridge/traversal refusal), `:2705` (terrain/wall),
  `:2787` (cliff), `movement_occupancy.rs:702`, `:907` (allied building),
  `:1086` (enemy blocker), `:1285` (wall/impassable). Only the code-2 sites
  (`movement_occupancy.rs:1236`, `movement_tick.rs:1073`) pass `false`.
- **Trigger:** a mover stopped by terrain, a cliff, a building footprint or an
  enemy within 2.25 cells of its goal.
- **Frequency:** very common — any right-click at or next to a building, cliff
  edge or contested cell.
- **Player effect:** VERA declares arrival and drops the order where retail
  keeps pushing and repathing; the unit stops short of the commanded cell.
- **Metric residuals** (already noted in `movement_blocked.rs:111-140`, both
  confirmed against the native site): VERA measures
  `|Δcell| × 256` with Z dropped; native measures `Distance3D 0x0041C380` over
  real lepton coords fetched through `vtable+0x48`. Bites on bridge approaches
  and on any sub-cell offset.

### D7 — VERA scatters one blocker; native scatters the whole cell list
- **Native:** §0.6 — the full object list from `+0xE4`/`+0xE8`, up to ten, each
  dispatched `vtable+0x174` in collected order.
- **VERA:** `movement_occupancy.rs:1177` scatters only the single classified
  `blocker_id`, once per blocker per tick (`already_scattered`).
- **Trigger:** a vehicle blocked by a cell holding more than one occupant —
  most often a full five-man infantry cell.
- **Frequency:** every time armour meets massed infantry, i.e. constantly in
  mid-game skirmish.
- **Player effect:** the jam clears one man per pass instead of all at once, so
  a tank pushing through infantry takes several times longer than retail.
- The ten-entry cap itself is **not** a defect and VERA carries no contradicting
  constant; what is unmodelled is the list (all occupants, cell-list order, and
  the `+0xE4` vs `+0xE8` layer selection).
- `src/sim/movement/scatter.rs` is not in this path at all:
  `scatter_units_from_cell` has no production caller and the only
  `tick_idle_scatter` reference (`src/sim/world/mod.rs:6512`) is commented out.
  Its header retraction stands; the ledger's I6 row should point at
  `bump_crush::scatter_blocker`, not `scatter.rs`.

### D8 — Drive's PathDelay limiter is left open
- **Native:** `+0x640/+0x648` is re-armed with `ftol(Rules+0x1760 × 900.0)` =
  9 frames after a successful `Find_Path` in the code-2 arm
  (`0x004B3A59..0x004B3A8C`) and in the no-path arm
  (`0x004B2850..0x004B286D`). `FootClass::Find_Path 0x004D4022` reads the same
  field.
- **VERA:** `handle_blocked_tick` re-arms `movement_timer` only on the
  urgency-1 *failure* branch (`movement_blocked.rs:308`) and, for Walk, at the
  tail (`:316`). A Drive mover that repaths successfully leaves the limiter open
  and repaths again next tick; two call sites additionally force it open first
  (`movement_step.rs:2511`, `:2701`: `start_movement(frame, 0, walk)`).
- The comments asserting "the limiter is permanently open for Drive movers ...
  the only writers of the tick field are the FootClass constructor and
  Set_Destination_Internal, both of which store zero"
  (`movement_occupancy.rs:1158-1167`) and "Walk restarts PathDelay after every
  actual FindPath attempt ... Drive does not" (`movement_blocked.rs:300-306`)
  are both contradicted by `0x004B2865` and `0x004B3A84`.
- **Frequency:** every blocked Drive mover, every tick.
- **Player effect:** ~9x the A* rate while blocked plus visible route flapping
  tick-to-tick where retail holds a route for 9 frames. At the 20k-unit target
  this is also a throughput risk.

### D9 — fresh-order blockage grace is 0 for non-Walk movers
- **Native:** `0x004D96CF..0x004D96ED` writes `+0x668 = Frame`,
  `+0x670 = Rules+0x1768` = 60.
- **VERA:** `movement_commands.rs:436` gives non-Walk movers
  `FootPathRuntime::default()`, whose `blocked_timer` duration is **0**. Walk
  gets the correct 60 via `DestinationTiming::accept_walk`
  (`movement_commands.rs:203-207`), which already cites the right address.
- **Trigger:** a Drive/Ship mover blocked on the first tick after an order.
- **Frequency:** every order issued into a congested area.
- **Player effect:** the first block escalates straight to urgency 2 — a long
  route-around instead of retail's 60-frame wait.

### D10 — the `+0x6B7` latch is raised for non-code-2 blocks
- **Native:** set only in the code-2 arm (`0x004B3663`, Walk `0x0075B8B6`,
  Ship `0x006A2CB2`, Hover `0x00515D3A`); cleared by actual movement
  (`Process_Drive_Track 0x004B1624`/`0x004B1FFF`, Ship `0x006A0CE4`/`0x006A1642`,
  Walk `0x0075BE11`/`0x0075BFD1`, Hover `0x00514528`/`0x005147DF`), by the
  constructor `0x004D3451` and by `Set_Destination_Internal 0x004D96C2`.
- **VERA:** `movement_blocked.rs:78-96` sets `path_runtime.path_blocked = true`
  on every blocked tick, including code-7 terrain, cliff and wall refusals.
- **Effect:** a terrain-blocked mover carries the "blocked by a moving friendly"
  state retail never gives it. The field is hashed (`world_hash.rs:1585` region),
  and it gates the blockage-timer arming, so the behavioural effect surfaces
  through D3 rather than on its own. Recorded as a state-authority divergence.

---

## 2. Where VERA matches, with evidence

- **M1 Timer arithmetic.** `src/sim/timer.rs:68-86` reproduces §0.2 exactly,
  including `PAUSED_START_FRAME = -1` against native `CMP ESI,-1` and expiry at
  the exact zero boundary.
- **M2 Constructor state.** `FootPathRuntime::at_frame`
  (`components.rs:344-356`): both timers `(frame, 0)`, `retries_left = 10`,
  `path_blocked = false` — matches `0x004D3320`, `0x004D3326`, `0x004D332C`
  (`EAX = 0xA` at `0x004D32DB`), `0x004D335B`, `0x004D3361`, `0x004D3451`.
- **M3 Urgency selection.** `movement_blocked.rs:170-174`
  (`if !blocked_timer.expired {1} else {2}`) matches
  `0x004B36BC..0x004B36EF` plus `SETNZ DL / INC EDX` at
  `0x004B39FD..0x004B3A00`.
- **M4 CloseEnough polarity and value.** All five native compares stop on
  `dist < CloseEnough`; VERA's predicate has the same polarity, and its
  Euclidean `isqrt` form (replacing an earlier Manhattan sum) is the right
  metric family. Stock `CloseEnough = 2.25` cells = 576 leptons.
- **M5 Walk code-2 timers are natively pinned.** `movement_blocked.rs:334-470`
  (`walk_code_two_repath_timing_matches_native_vectors`) runs 30 cases from
  `tools/infantry_scatter_oracle.json` (`"source": "unicorn/gamemd.exe"`)
  covering grace initialisation/preservation, the movement-delay gate and
  urgency selection. This is the only native-vector coverage A5 has; it does
  **not** cover the retry counter, `CloseEnough`, or scatter.
- **M6 Rules constants.** `ruleset.rs:1369/1371` defaults 9 and 60 match
  `ftol(0.01 × 900.0)` and `Rules+0x1768 = 0x3C`.
- **M7 Ten-entry cap.** Real and correctly stated in the criterion
  (`FUN_0040CE50(10,0)` inside `0x00481670`). Nothing in the tree contradicts it.
- **M8 NullCoord premise.** `bump_crush.rs:1020-1028`'s "force=1 with a
  NullCoord" is correct: `0x008A0790` reads 16 zero bytes and all five locomotor
  sites push it with `param_4 = 1`.

---

## 3. UNCHECKED

| # | Open question | Instrument that would settle it |
|---|---|---|
| U1 | Identity of `Foot+0x598`, which must be null before the exhausted counter actually stops the mover (`0x004B2E1C`, `0x004B31FF`). | `get_field_access_context` / xref sweep on `+0x598` across FootClass, or a debugger breakpoint at `0x004B2E1C`. |
| U2 | Which coordinate the ladder's `CloseEnough` measures against (the `EBX` CoordStruct at `0x004B2C1A` / `0x004B310F`) — destination cell centre, NavCom, or the Find_Path target. | Unicorn `emulate_function` on `0x004B2630` with a synthetic Foot, or a debugger watch on `[EBX]` at `0x004B2C0A`. |
| U3 | Whether the terrain/impassable (code 7) path reaches a `CloseEnough` compare at all. Only the two ladder compares were traced to their guards; `0x004B297F`, `0x004B37BE`, `0x004B42DB` were read at the compare only. | `analyze_control_flow` over `Process_Movement`, or breakpoints on the three compares during a retail terrain-block. |
| U4 | Whether the decrement label `0x004B2D68` is reachable for anything other than an allied blocking object (this decides whether D5's "different counted event" is also a *narrower* event). | Same instrument as U3. |
| U5 | All frequency claims above are reasoned from stock rules and the code paths, not measured. **No retail blocked run exists**, and no fixture drives a blocked recovery with `close_enough > 0` — the path-test harness zeroes it, which disables the branch entirely (`movement_blocked.rs:131-136`). | The A5 retail-map blocked run named in the criterion: two vehicles plus a full infantry cell on a retail map, retail `-win` capture against the same order in VERA. A Δ(2,1) diagonal approach at `close_enough = 576` would pin D6's metric half on its own. |
| U6 | Whether `advance_compatibility_process` (`movement_tick.rs:1878`, once per movement tick, non-Walk only, and gated behind `movement_target.is_some()`) diverges from native wall-frame aging for a mover between orders, and whether the Drive same-tick re-entries (`0x004B397F`, `0x004B4552`) double-age it. | A Rust regression that orders → aborts → re-orders across 60 ticks and compares timer remainders against a frame-anchored reference. |

---

## 4. Suggested ledger wording for the A5 row

> **A5 Blocked recovery — NOT ACCEPTED (examined 2026-09-16).** Ten divergences
> established against `DriveLocomotionClass::Process_Movement 0x004B2630`,
> `WalkLocomotionClass::ProcessMovement` (the Walk family at `0x0075AEC0`; this
> line carried a placeholder digit when the report was written and it was
> replaced rather than reproduced),
> `FootClass::Set_Destination_Internal 0x004D96B0` and
> `CellClass::Scatter_Objects 0x00481670`: the code-2 arm scatters where native
> does not (D1); `POST_SCATTER_WAIT_FRAMES = 10` misidentifies the `+0x64C`
> retry counter as a frame wait and displaces `BlockagePathDelay = 60` (D2);
> the blockage timer re-arms per pass instead of per latch transition (D3);
> `+0x64C` is reset on orders and segment repaths that native leaves alone (D4)
> and decremented on a different event with an off-by-one (D5); `CloseEnough`
> is consulted on block classes native gates behind an allied blocking object
> (D6); scatter reaches one blocker instead of the cell's list (D7); Drive's
> 9-frame PathDelay limiter is left open (D8); a fresh non-Walk order gets a
> zero-length grace instead of 60 (D9); and the `+0x6B7` latch is raised for
> non-code-2 blocks (D10). Matching: timer arithmetic, constructor state,
> urgency selection, `CloseEnough` polarity, the Walk code-2 native oracle
> (30 cases), the rules constants, the ten-entry cap, and the NullCoord/force=1
> premise. Six items UNCHECKED, including every frequency estimate — the
> criterion's retail blocked run still does not exist.
