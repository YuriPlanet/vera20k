<!--
Provenance: /brainstorm 2026-06-02 for Slice 7b of the Mission/Radio Substrate plan
  (docs/plans/2026-06-01-mission-radio-substrate-implementation-plan.md §Slice 7b).
Supersedes the thin 7b stub in that plan (which only added `bunker_host` + link helpers
  and wrongly assumed a bunker lifecycle already existed). 7b = the WHOLE lifecycle.
Load-bearing research (verified this session against the binary, see Ledger):
  - [TANK_BUNKER_ENTRY_EXIT_VISIBLE_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TANK_BUNKER_ENTRY_EXIT_VISIBLE_LIFECYCLE_GHIDRA_REPORT.md)
  - [BUNKER_0X2E4_LIFECYCLE_EXIT_CLEAR_PATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUNKER_0X2E4_LIFECYCLE_EXIT_CLEAR_PATH_GHIDRA_REPORT.md)
  - [BUNKER_SERVICEDEPOT_0X2E4_RECIPROCAL_LINK_TEARDOWN_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUNKER_SERVICEDEPOT_0X2E4_RECIPROCAL_LINK_TEARDOWN_GHIDRA_REPORT.md) (2026-06-02)
Status: DESIGN — approved approach (A). Not a plan, not code. Feed to /write-plan next.
-->

# Tank Bunker Lifecycle (Slice 7b) — Design

## Goal

Build the complete stock-`NATBNK` tank-bunker lifecycle — entry admission, the
6-state install machine, hide, the three distinct exit/teardown paths, the reciprocal
`+0x2E4` link, and wall sounds/anims — on top of the Slice 0–7 mission/radio substrate.

## Architecture Context

**Bunkers today are data-only.** `ObjectType.bunker` (Bunker=yes) and `ObjectType.bunkerable`
parse (`object_type.rs:687/643`); `GameEntity.bunker_occupant: Option<u64>` exists
(`game_entity.rs:427`) but is **written nowhere** — only *read* by the passability gate
(`movement_occupancy.rs:332`, `bump_crush.rs:147`, `pathfinding/cell_entry.rs:345`).
`RulesGeneral.bunker_walls_down_sound` parses; `BunkerWallsUpSound` does not.
`SimSoundEvent::RefineryExitSfx` maps to the down-sound (stale naming) but has **no producer**.
There is no entry, install, exit, or teardown code anywhere.

**Substrate 7b builds on (Slices 0–7a/c landed):**
- **Radio bus** — `radio::transmit(sim, sender, target, msg, payload) -> RadioResponse`
  (intercepts `Hello`/`Break` for contact bookkeeping; all other opcodes → `receive_radio`).
  `receive_radio` dispatches by category; `Structure` currently → `refinery_receive` only.
  `RadioMessage` has `CanEnter=0x0F`, `DockNow=0x15`, `Hello=0x02`, `Break=0x03`. `Contacts`
  slot store on `radio_contacts`; `dock_entered_with: Option<u64>`.
- **Mission verbs** — `MissionType` (Guard=5, Move=2, Enter=7, …); `assign_mission_with_teardown(id, mission, DockTeardown)`; `MissionCom` (written in parallel, authoritative only in a later
  slice); `MissionTimer` (frame-anchored, `defer`/`due`/`clear`, no per-tick drift).
- **Lifecycle helpers** — `conceal(id)`/`reveal(id)` (logic-vector limbo + presence),
  `remove_entity_occupancy`/`add_entity_occupancy`, `uninit(id)` teardown (already calls
  `clear_radio_contacts_for(id)` at the BREAK point, `world/mod.rs:~955`).
- **Approach precedent** — `Command::EnterTransport` sets `MissionType::Enter` +
  `PassengerRole::Boarding{..Approach}` + an approach move (`world_commands.rs:845`); the
  passenger tick boards on arrival via `conceal`. We **mirror this shape but do NOT reuse
  `PassengerRole`/cargo** — bunkers are a reciprocal single-slot link, not cargo (all three
  research docs forbid the collapse).
- **Anims** — `BuildingAnimOverlays`/`AnimOverlayState` (`components.rs:617/640`);
  `art_data.rs` parses `SpecialAnim..SpecialAnimFour` + `…Damaged` (`BuildingAnimKind::Special`).
  `ConditionRed`/`Yellow` in `RulesGeneral`.
- **Sound** — sim emits `SimSoundEvent` into `sim.sound_events`; `app_sim_tick.rs` drains,
  resolves the rules sound name, screen-converts, and plays positionally.

**Tick order** (`advance_tick`): commands → ground move → air/special → vision → power →
turrets+combat → retaliation+passengers (`passenger::tick_passenger_system`, ~2263) →
scatter+production+repairs+**docks** (`building_dock::tick_building_docks`, ~2289)+ore →
AI → defeat → building anims+cleanup → state hash.

## Impact Analysis

**New surface:**
- Unit: `bunker_link: BunkerLink` (enum `None`/`Approaching(u64)`/`Installed(u64)` — folds
  the plan's `bunker_host` plus the approach state into one field; mirrors `PassengerRole`'s
  enum shape). Hashed.
- Building: `bunker_runtime: Option<BunkerRuntime>` (the 6-state install machine + installing
  unit + inter-state timer; mirrors `building_gate: Option<BuildingGateRuntime>`). `Some` on
  Bunker=yes buildings from spawn. Hashed. `bunker_occupant` (exists) becomes the authoritative
  installed-occupant pointer, written at install / cleared at every teardown.
- `src/sim/docking/bunker_link.rs` — `install_bunker_link`, `break_bunker_link` (core: clear
  both sides + BREAK), the three release helpers (`release_normal`/`release_sell_destroy`/
  `release_clear`), and `break_links_on_despawn` (safety net).
- `radio/receive.rs` — a Bunker branch in the `Structure` path (`CanEnter`/`DockNow`).
- `command.rs` + `world_commands.rs` — `EnterBunker { unit_id, bunker_id }` and
  `EjectBunker { bunker_id }` (eject targets the **bunker**, not the hidden unit — see Decision D3).
- Sounds — parse `BunkerWallsUpSound`; add `SimSoundEvent::BunkerWallsUp/Down`; retire the
  `RefineryExitSfx`-as-bunker-down mapping (it had no producer); map in `app_sim_tick.rs`.
- A building-side `tick_bunker_install` in the docks phase; teardown hooks at five sites.

**Touched / blast radius:**
- **The passability gate goes live.** `bunker_occupant` was read but never set; install/release
  now set/clear it, so an occupied bunker starts blocking cells as gamemd does. Must confirm no
  test asserts it stays `None` (`cell_entry.rs:948` `empty_vs_occupied_bunker_…` already covers both).
- **Five teardown trigger sites** must each call the right release helper or a unit is orphaned
  (stuck/ghost): building Sell (`production_sell.rs`), building death (`ReceiveDamage`), unit
  death, superweapon launch, temporal. Plus the `uninit` safety net (~`world/mod.rs:955`).
- Determinism: `bunker_link` + `bunker_runtime` fold into `world_hash` → **one re-baseline**
  (the plan's hash ledger already reserves "7b adds bunker_host"). `MissionTimer` is
  frame-anchored; `conceal`/`reveal`/`Find_Nearby_Passable_Cell` are deterministic; the
  state-0 blocker-shove reuses the existing scatter (deterministic sim RNG — same draw count).

## Chosen Approach — A: Radio-bus admission + dedicated `BunkerRuntime` 6-state machine + `bunker_link.rs` teardown trio

Entry routes through the Slice-4 bus (gamemd's actual mechanism: case `0x0F` admission, case
`0x15` commit). The unit's `EnterBunker` command runs the handshake and, if admitted, starts the
building's `BunkerRuntime` and issues the approach move. The building ticks the faithful 6-state
machine in the docks phase. Teardown is the three distinct `bunker_link.rs` helpers, each ending
in a BREAK over the bus. Rejected alternatives in the last section.

### The hide model (load-bearing decision, documented divergence)

gamemd's install hide (`vtable+0x150`) is a **light** hide: the unit stays a live, ticking
object (it fires from inside — the bunker combat surface — and `UndockUnit @ 0x004593A0`
issues locomotor commands on it *without* an Unlimbo). **7b uses a full limbo instead**
(`conceal` + `remove_entity_occupancy`), because combat/render is explicitly out of 7b scope
(Decision D3) so the unit needs neither to tick, fire, nor render. Each release **reveals +
places** the unit to reproduce the same *visible* end position gamemd produces. This is an
output-equivalent mechanism divergence, justified by the deferred combat/render scope; the
**combat/render slice must revisit the hide model** to the live-but-hidden form so the bunkered
tank can fire and draw. Recorded as a known follow-up, not silent drift.

## Tiny-Detail Ledger (parity constraints — carried to /write-plan)

Verified this session unless marked. `[GHIDRA …]` = decompiled/confirmed live this session.

1. **Admission gate.** `Bunker=yes` building + own-owner + not-occupied/installing + `CanAutoDeployHere`:
   `Bunkerable`(type+0xD2E) ∧ deploy-compat(type+0xCA1) ∧ has-primary-weapon(vtable+0x3F4 ≠ null/[0]≠0)
   ∧ movement-zone(type+0x67C) ≠ 3 ∧ ¬(busy-guard: this.field_0x14&4 && this[1].field_0x174≠0).
   [GHIDRA 0x0070FB50 CanAutoDeployHere; 0x0043C2D0 case 0x0F]
2. **Handshake → install start.** case `0x0F` = admission reply; case `0x15` sets building
   field_0x6DD=1 + `MissionSet(0x14,0)` → `MissionRepairAndProduce` (the *sole* caller of the
   install machine, Bunker-gated) → `0x00458E50`. [GHIDRA 0x0043C2D0; callers(0x00458E50)=0x0044B780]
   HELLO(0x02) precedes 0x0F — **confirmed** (`RADIO_INFERRED_CODES_AND_DOCK_HANDSHAKE_ORDER` §9.4:
   bunker admission = HELLO then CAN_ENTER). The unit-side *sender function* of 0x0F/0x15 is
   unverified but not needed for the command-driven Rust admission.
3. **Install machine = 6 states on `+0x718`** (run while building on mission 0x14). [GHIDRA 0x00458E50]
   - Preflight (every entry): candidate = `building+0x2E4` else `GetDestination(0)`; if null or
     candidate `WhatAmI()≠Unit` → `+0x718=0`, building `MissionSet(Guard=5)`, return.
   - **0** wait until candidate is on the building cell AND locomotor stopped; then shove every
     non-candidate object off the 2×2 foundation (per-cell `Find_Nearest_Object`, range 0x80;
     `obj.vtable+0x174` scatter); → 1.
   - **1** re-scan foundation; while any non-candidate object is on it (hard blocker = `obj+0x14&4
     ∧ obj+0x5a4==0`), stay; once clear, set the unit body **FacingClass (`unit+0x388`)** desired
     facing = `atan2(Δy=building.y−unit.y, Δx=unit.x−building.x)` → `ftol` → `+0x7fff`; → 2.
   - **2** once the unit finishes turning (FacingClass not rotating): octant = `(facing>>7)+1 &0x1FE`
     → force-track index (0x40 NE→**0x43**, 0xC0 SE→**0x44**, 0x1C0 NW→**0x46**, else SW→**0x45**);
     locomotor `+0x70` head-to(**track, building-coord — no ±0x80 offset**); unit `+0x544(0,1.0)`; → 3.
   - **3** wait until candidate on cell AND locomotor stopped; set `unit+0x388` desired facing =
     **South (`0x8000`)**; → 4.
   - **4** once the turn-to-South completes: create entry anims (ledger 5); → 5.
   - **5** install (ledger 6); → 6 (occupied).
   - **RESOLVED this session** ([GHIDRA `0x00458E50` disasm + `0x004c9220/80/d0`]): the "timers" are the
     unit body `FacingClass @ unit+0x388` — the waits are **turn-completion** (`|Δfacing|/ROT`); there
     are **no magic frame durations**, and `0x8000` is a desired **facing (South)**, not a duration.
     No `DockingOffset` is read (entry cell = wherever the unit's own move lands it on the footprint;
     `Look_up_building_in_cell` only checks `WhatAmI()==6`). Install tracks `0x43–0x46` are the diagonal
     entry curves (siblings of exit track `0x47`/track 15). Remaining minor: `unit+0x214`→-1 reader and
     the COL-verified `+0x150`/`+0x480` decompile (both deferred; behaviors confirmed from call shape).
     Full detail: `TANK_BUNKER_INSTALL_MICROSTATES_GHIDRA_REPORT.md`.
4. **CreateAnimForSlot anim slots.** entry → slots **10/11**; exit → slots **12/13**.
   release/clear clear slots 10/11 before creating 12/13. [GHIDRA 0x00458E50 / 0x004595C0]
5. **Entry anims (state 4), health-gated.** ratio > ConditionRed (`RulesClass+0x1700`) →
   `SpecialAnim`(+0x11F4) + `SpecialAnimTwo`(+0x1238); else `SpecialAnimDamaged`(+0x1204) +
   `…TwoDamaged`(+0x1248). Skip a slot whose name is empty. [GHIDRA 0x00458E50 case 4;
   art `[NATBNK]` `NATBNK_A/AD`, `NATBNK_B/BD`]
6. **Install state-5 writes, in order.** `building+0x2E4=unit` → `unit+0x2E4=building` →
   `unit+0x214=-1` → hide (Limbo `vtable+0x150`) → `building+0x718=6` → unit `MissionSet(Guard=5,1)`
   → BunkerWallsUpSound at building location if `RulesClass+0x240 ≠ -1`. [GHIDRA 0x00458E50 case 5]
   (`unit+0x214 = -1` companion field — exact role UNKNOWN; closest Rust analogue is clearing the
   unit's nav/destination. Mark for confirmation.)
7. **Normal exit `release_normal`** (deploy/eject). [GHIDRA 0x004595C0]
   clear slots 10/11 → down-sound at building loc if `+0x244 ≠ -1` → exit anims health-gated:
   `SpecialAnimThree`(+0x127C)/`Four`(+0x12C0) else `…Damaged`(+0x128C/+0x12D0), slots 12/13 →
   clear `unit+0x2E4` → loco Stop(+0x58) + Head_To(+0x70) track **0x47** offset (building −0x80, +0x80)
   → unit `vtable+0x544(0,0x3ff00000)` → `Find_Nearby_Passable_Cell` from building-NW **(−1,+1)** →
   Unlimbo(`vtable+0x480`, cell, dir) → unit `MissionSet(Move=2)` → clear `building+0x2E4` +
   `+0x718=0` → building `MissionSet(Guard=5)` → BREAK (`vtable+0x274(3)`).
8. **Sell/destroy/temporal-building `release_sell_destroy`** (= `UndockUnit`). [GHIDRA 0x004593A0]
   loco Stop(+0x58) + Head_To(+0x70) track **0x47** offset (building −0x80, +0x80) → unit
   `vtable+0x544(0,0x3ff00000)` → clear **both** `+0x2E4` → BREAK(`vtable+0x274(3)`). **No sound,
   no anims, no `Find_Nearby_Passable_Cell`, no Unlimbo, no mission set, does NOT clear `+0x718`.**
   In gamemd the unit was never full-limbo'd so it just relocates to the building cell ±half-cell.
   *7b adaptation (full-limbo model):* reveal + place at the building cell (SE facing, the −0x80,+0x80
   half-cell offset), no Move mission → same visible result (unit at building cell, idle).
   Triggers: Sell `0x0044AA00`, ReceiveDamage death-case-4 `0x004424EA`, Temporal(bldg) `0x0071A760`.
9. **Super/temporal-non-bldg/unit-death `release_clear`** (= `FUN_00459470`). [GHIDRA 0x00459470, doc 2026-06-02]
   clear anim slots → down-sound if occupied (`+0x244 ≠ -1`) → exit anims (health-gated, slots 12/13) →
   BREAK(`vtable+0x274(3)`) **before** side-clear → clear unit-side `+0x2E4` then building-side →
   `+0x718=0` → building `MissionSet(Guard=5)`. **No reveal/place/mission on the unit** (it is dead or
   warped). Triggers: SuperClass::Launch `0x006CC390` (area scan), Temporal(non-bldg) `0x0071A760`,
   UnitClass::ReceiveDamage `0x00737C90`.
10. **Both-sides-clear invariant + sound matrix.** Every teardown clears BOTH `+0x2E4` before the
    building resets its mission; BREAK fires before/at the side-clear. Up-sound: install only.
    Down-sound: `release_normal` + `release_clear` only — **NOT** `release_sell_destroy`. [doc 2026-06-02 §3/§5]
11. **Empty-bunker preflight.** install preflight with no candidate → `+0x718=0`, Guard, return;
    `release_normal` with null occupant → `+0x718=0`, Guard, return. [GHIDRA 0x00458E50 / 0x004595C0]
12. **Sounds.** `BunkerWallsUpSound`→`RulesClass+0x240` (stock `TankBunkerUp`),
    `BunkerWallsDownSound`→`+0x244` (stock `TankBunkerDown`); play only if id ≠ -1; positional at
    building location. [GHIDRA 0x00669E20; rulesmd.ini:719/720]
13. **Stock activation.** `NATBNK` only (`Bunker=yes`, rulesmd.ini:13732); `NABNKR` does NOT
    (rulesmd.ini:12979); single occupant (`NumberOfDocks=1`); `Foundation=2x2`, `Strength=1000`,
    `Armor=steel`. Do NOT string-special-case NATBNK — gate on the parsed `Bunker` flag.

## Design

### Components

**`BunkerLink` (unit, `game_entity.rs`)**
```text
enum BunkerLink { None, Approaching(u64), Installed(u64) }   // Default None; hashed
```
`Approaching` = en route under `EnterBunker` (abort signal: cleared on any retask, which lets the
building preflight reset). `Installed` = the reciprocal of `building.bunker_occupant`.

**`BunkerRuntime` (building, `game_entity.rs`, `Option<>` like `building_gate`)**
```text
struct BunkerRuntime {
    state: BunkerState,            // Idle, ArriveWait, ClearWait, Turn, TurnWait, EntryAnim, Occupied
    installing_unit: Option<u64>,  // candidate during ArriveWait..EntryAnim; None when Idle/Occupied
    // States 1->2 and 3->4 advance on the UNIT's facing-turn completion (set desired facing on the
    // unit's existing FacingClass, then gate on "not rotating") — NOT a countdown. No MissionTimer
    // needed for the install waits (keep one only as an optional safety cap). See RE report.
}
```
`BunkerState` maps 1:1 to `+0x718` (Idle=0 pre-arrival, …, Occupied=6). `Some(BunkerRuntime::idle())`
created at spawn on every `Bunker=yes` building. `bunker_occupant` is set only at `EntryAnim→Occupied`.

**`src/sim/docking/bunker_link.rs`** — pure functions on `&mut Simulation`:
- `install_bunker_link(sim, building, unit, rules)` — ledger 6: write both sides, `unit+0x214`
  analogue clear, `conceal`+`remove_entity_occupancy`(unit), unit `Guard`, state→Occupied,
  emit `BunkerWallsUp`. (Entry anims are created by the machine in `EntryAnim`, just before.)
- `break_bunker_link(sim, building) -> Option<u64>` — core: read occupant, `transmit(building,
  unit, Break)`, clear `unit.bunker_link` and `building.bunker_occupant`, return the unit id.
- `release_normal(sim, building, rules)` — ledger 7 (full ejection).
- `release_sell_destroy(sim, building, rules)` — ledger 8 (place-at-building-cell, no sound/anims/Move).
- `release_clear(sim, building, rules)` — ledger 9 (clear-only, no reveal/place).
- `break_links_on_despawn(sim, id)` — safety net: if `id` is a bunker with an occupant or a unit
  with `Installed(b)`, clear the reciprocal side (no anims/sound). Called from `uninit`.

**Radio (`radio/receive.rs`)** — `Structure` branch becomes:
```text
Structure => if object_type(target).bunker { bunker_receive(...) } else { refinery_receive(...) }
```
`bunker_receive`: `CanEnter(0x0F)` → own-owner ∧ alive ∧ `bunker_occupant.is_none()` ∧
`bunker_runtime.state==Idle` ∧ `can_auto_deploy_here(sender)` ⇒ `Roger`, else `Negatory`;
`DockNow(0x15)` → set `bunker_runtime` = `ArriveWait{installing_unit: sender}` (building "mission 0x14"),
reply `Roger`; `Break` → handled by the bus contact bookkeeping + helpers.
New `can_auto_deploy_here(sim, unit, rules) -> bool` (ledger 1) lives in `bunker_link.rs` or a small
`docking/bunker_admit.rs`.

**Commands (`command.rs` / `world_commands.rs`)**
- `EnterBunker { unit_id, bunker_id }` — validate owner + `bunker_receive(CanEnter)` admission; on
  `Roger`: `transmit(DockNow)`, set `unit.bunker_link=Approaching(bunker_id)`,
  `assign_mission_with_teardown(unit, Enter, DockTeardown::None)`, issue approach move to the bunker
  cell (mirror `EnterTransport`'s move issuance). On `Negatory`: no-op.
- `EjectBunker { bunker_id }` — validate owner + occupied; call `release_normal`. (Targets the
  bunker because the occupant is not rendered/selectable in 7b — Decision D3.)

**Sounds** — parse `BunkerWallsUpSound` into `RulesGeneral` beside `bunker_walls_down_sound`; add
`SimSoundEvent::BunkerWallsUp{rx,ry}` / `BunkerWallsDown{rx,ry}`; map both in `app_sim_tick.rs`
(positional, skip when the rules string is empty/absent — matches the `≠ -1` guard). Remove the
`RefineryExitSfx`-as-bunker-down provisional mapping.

### Interfaces / Contracts

- **Entry:** `EnterBunker` → bus admission → `BunkerRuntime` starts → unit approaches → building
  machine detects arrival (state 0) → states 0–5 → `install_bunker_link`. The handshake (0x0F/0x15)
  is sent at command time; the *machine* waits for physical arrival (state 0/3), matching gamemd.
- **Abort:** any retask of an `Approaching` unit clears its `bunker_link`; the building preflight
  finds no candidate → resets to `Idle`/`Guard` (ledger 11).
- **Exit/teardown dispatch (the contract that must not be collapsed):**
  | Trigger | Helper | Reveal+place | Sound | Anims | Mission |
  |---|---|---|---|---|---|
  | Eject / deploy | `release_normal` | nearby passable cell + Move | down | 12/13 | Move |
  | Sell / building death / temporal(bldg) | `release_sell_destroy` | building cell, no Move | — | — | — |
  | Super / temporal(non-bldg) / unit death | `release_clear` | none | down | 12/13 | — |
- **Despawn safety net:** `uninit` → `break_links_on_despawn` after `clear_radio_contacts_for`.

### Data Flow

`EnterBunker` (cmd phase) → bus → `BunkerRuntime=ArriveWait` + unit `Enter`+move. Ground-move phase
drives the unit onto the bunker cell. Docks phase `tick_bunker_install` advances ArriveWait→…→Occupied,
calling `install_bunker_link` at the EntryAnim→Occupied edge. `EjectBunker` (cmd phase) →
`release_normal`. Death/sell/super/temporal handlers (their own phases) → the matching release.
`uninit` (cleanup phase) → safety net.

### Error Handling

Helpers no-op on a missing entity (`get`/`get_mut` guards, the established pattern). One-sided links
are an invariant violation: `break_bunker_link` always clears both, and `break_links_on_despawn`
catches a straggler. `EnterBunker` on a full/ineligible bunker is a silent no-op (Negatory).
`Find_Nearby_Passable_Cell` failure in `release_normal` falls back to the building cell (never lose
the unit) — flag if gamemd retries instead (UNKNOWN, low frequency).

### Testing Strategy

- `bunker_link` unit tests: install sets both sides + conceals + Guard + up-sound; each release clears
  both sides; sound matrix (up=install; down=normal/clear, not sell_destroy); anim slots 10/11→12/13.
- State-machine test: ArriveWait→…→Occupied across `MissionTimer` advances; abort on retask resets.
- Admission test: Bunkerable vehicle accepted; infantry / `Bunkerable=no` / enemy-owner / occupied /
  `NABNKR` rejected. (`bunker_entry_requires_bunker_flag_and_bunkerable_vehicle`.)
- Teardown-dispatch test: sell→sell_destroy (building cell, no Move/sound); building death→sell_destroy;
  unit death→clear (no reveal); super→clear. Both sides cleared in all.
- Passability-gate-goes-live test: occupied bunker blocks the row helper, empty does not.
- Despawn safety-net test: limbo a bunker with an occupant → back-link cleared.
- Re-baseline the global parity hash once (documented one-line reason in the same commit).

### Determinism

`bunker_link` + `bunker_runtime` fold into `world_hash` (one re-baseline). `MissionTimer` is
frame-anchored (`sim.binary_frame`), wrapping `u32`, no per-tick decrement. `conceal`/`reveal` and
`BTreeMap` iteration are deterministic. The state-0 blocker-shove reuses existing scatter (same RNG
draw count/order). No new `HashMap`, no float in the tick path.

## Architectural Decisions

- **Follows** the `building_gate` runtime pattern (`BunkerRuntime`), the Slice-4 radio-bus pattern
  (admission + BREAK over `transmit`), the `EnterTransport` approach-issuance shape, the
  `SimSoundEvent`/`BuildingAnimOverlays` patterns.
- **Deviates (documented):** (D1) reciprocal `Option`-link enum, NOT `PassengerRole`/cargo — required
  by the research. (D2) full-limbo hide in 7b vs gamemd's light hide — forced by combat/render being
  out of scope; output-equivalent; combat/render slice must revisit. (D3) eject targets the bunker,
  not the hidden unit — forced by the unit not being rendered; behavior (`release_normal`) is identical
  to gamemd's unit-deploy trigger.
- **Tech debt / follow-ups:** the combat/render slice owns the live-hidden hide + bunkered-unit draw
  (z-sorted between SpecialAnim front/back) + firing (BunkerDamage/ROF/Range multipliers, already
  parsed in `RulesGeneral`). Until it lands the player sees walls up/down but no tank inside.

## Open RE gaps — RESOLVED (2026-06-02 re-investigation)

The full-6-state RE gaps are closed by [TANK_BUNKER_INSTALL_MICROSTATES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TANK_BUNKER_INSTALL_MICROSTATES_GHIDRA_REPORT.md):
- **Inter-state waits are facing-turn completions, not timers.** The install drives the unit's body
  `FacingClass @ unit+0x388`; no magic frame durations exist. `0x8000` = desired facing South.
- **No `DockingOffset`/entry cell math** — the unit reaches the footprint via its own move;
  `Look_up_building_in_cell` only verifies a `WhatAmI()==6` building in the unit's cell.
- **Force-track:** install = diagonal entry curves `0x43`(NE)/`0x44`(SE)/`0x45`(SW)/`0x46`(NW) by
  octant, target = building coord (no offset); exit = `0x47` (TurnTrack[71]/RawTrack[15], already in
  `drive_track.rs`, `±0x80` lepton offset). Confirm `0x43–0x46` point tables exist before wiring.
- **Hide `+0x150`** is a *light* hide (keeps the unit's coordinate + live-object status; `UndockUnit`
  re-uses position without an Unlimbo). 7b's full-conceal model is output-equivalent; the combat/render
  slice revisits it.
- **Handshake** HELLO→CAN_ENTER confirmed.

Still deferred (neither blocks 7b): `unit+0x214`→-1 reader (clear the unit's pending-nav/target on
hide as the behavioral analogue); the COL-verified decompile of UnitClass `+0x150`/`+0x480` (RTTI
backlink not analyzed in this Ghidra DB). `/write-plan` may proceed.

## Alternatives Considered

- **B — Direct (no-bus) admission + same machine/helpers.** Simpler call graph, but DRIFTS: gamemd's
  admission *is* radio 0x0F/0x15 and teardown *is* BREAK, which also clears the unit↔bunker radio
  contact; bypassing the bus hand-rolls that bookkeeping and diverges from the established Slice-4
  pattern. Rejected.
- **C — Reuse `PassengerRole::Inside` + cargo-of-1.** Explicitly forbidden by all three docs: collapses
  the reciprocal `+0x2E4` link into the garrison cargo vector, gives the wrong (single, garrison-style)
  teardown, and violates "do not collapse the three helpers." Rejected (anti-pattern).
