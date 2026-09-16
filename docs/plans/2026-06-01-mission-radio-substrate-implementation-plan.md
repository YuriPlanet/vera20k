<!--
Provenance: assembled 2026-06-01 by workflow wf_1b44ffd1-c27
  (5 binary verifiers V1–V5 + 7 per-slice planners P0–P6 + synthesizer)
  from [docs/research/MISSION_RADIO_SUBSTRATE_SERVICE_DESIGN.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSION_RADIO_SUBSTRATE_SERVICE_DESIGN.md) (the design).
Companion: [docs/research/MISSION_RADIO_SUBSTRATE_BINARY_VERIFICATIONS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSION_RADIO_SUBSTRATE_BINARY_VERIFICATIONS.md) (the §9 Ghidra resolutions).
Status: DRAFTED, not approved or executed. Review before implementing; execute
  slice-by-slice (§D linearized path 0→{1,2,3}→{4,5}→{6,7}→8), build + test gated.
-->

# Mission/Radio Substrate — Consolidated Implementation Plan

> Status: ready for review, then `docs/plans/`. Assembled from 5 §9 verification results (V1–V5) and 7 per-slice drafts (P0–P6). Every Ghidra address and `file:line` citation from the source drafts is preserved. The default verdict for any unproven equivalence is **DRIFT** (CLAUDE.md); items that remain unproven are tagged STILL-UNCHECKED, never silently downgraded.
>
> **#1 invariant preserved throughout:** `sim/` (incl. the new `sim/mission/` and `sim/radio/`) NEVER depends on `render/`, `ui/`, `sidebar/`, `audio/`, `net/`. The `rules → sim::intern`/`sim::mission` reference already exists in `ruleset.rs` and is the established pattern, not a new violation.
>
> **advance_tick phase order preserved:** no slice collapses or reorders the tick phases. The pre/post-combat order-intent split and Phase-6 retaliation slot stay exactly where they are (`world/mod.rs:1943/1947/2187/2189`); only state *representation* and *teardown call sites* change. There is no monolithic dispatch rewrite.

---

## A. Verified preconditions (folding in the §9 resolutions)

| # | Item | Verdict | Resolution the plan adopts |
|---|---|---|---|
| **V1** | `ReadyToCommence` (vtable +0x200) per-subclass overrides — the commence gate | **RESOLVED (per-type hook required)** | All four leaf entity types (Building `0x00454250`, Unit `0x00744270`, Infantry `0x00521B60`, Aircraft `0x0041B5E0`) override +0x200 with a real predicate; the base `0x004E0140` is `return 1`. `MissionClass__Queue_Mission @ 0x005B35E0` calls `(**(*this+0x200))()` and skips `Commence (+0x1EC)` when it returns false. → The verb API **must** carry a per-`EntityCategory` `ready_to_commence()` hook (Slice 6). `assign_mission` bypasses it (force-promote); only `queue_mission(commence=true)` consults it. |
| **V1-residue** | Byte-flag semantics (`+0x6DD` building, `+0x6D2/+0x6D4` aircraft, `+0x6E1/+0x6E2/+0x6D1/+0x68D/+0x8D` unit/infantry busy flags), locomotor `+0x80` idle predicate, mission-ID table-vs-enum off-by-one (table 15="Ambush" vs design enum 14) | **STILL-UNCHECKED** | Slice 6 encodes the verified predicate *structure*; the exact excluded-mission set + flag setters are traced before any *live* commence path relies on the gate. Reconcile the mission-ID numbering against the canonical `MissionType` enum (§C.0) before hardcoding any mission constant into the gate. |
| **V2** | AircraftClass `+0x294` radio-deaf latch — the SETTER | **RESOLVED (it is a back-pointer, not a bool)** | `+0x294` is a back-pointer from an airstrike-summoned aircraft to its controlling AirstrikeClass. Set by `FUN_0041d860` (launch) / `FUN_0041da20` (lead-swap incoming); cleared/reassigned by `FUN_0041da20` (outgoing→0) / `FUN_0041db40` (teardown, rescan global registry `DAT_00889fbc`). Constructor `0x00413D20` does NOT write it (calloc-zeroed). → Model as `airstrike_owner: Option<EntityId>`; radio-deaf gate = `airstrike_owner.is_some() && mission ∈ {Retreat, ParaDropApproach, ParaDropOverfly, SpyplaneApproach, SpyplaneOverfly}`. Owned by a future `sim/airstrike` service, **deferred to Slice 7-aircraft**; not on the refinery/airfield critical path. |
| **V2-residue** | Exact `AircraftClass::AI` read-site; whether >1 AirstrikeClass can claim one aircraft | **STILL-UNCHECKED** | Does not affect the SET/CLEAR contract; resolve when `sim/airstrike` is implemented. |
| **V3** | Dock eviction / who-docks-next order | **RESOLVED (FIFO is a proven DRIFT)** | gamemd has **no stored wait-queue**. A saturated refinery/airfield replies NEGATORY(10) to every HELLO/CAN_ENTER; the next docker is whichever waiting unit re-probes and wins, **distance-then-deterministic-order** (`Find_Docking_Bay 0x004DF040`, refinery building-search nearest pick). Receiver never evicts (`Receive_Radio 0x0065A820`); only a full SENDER self-evicts its own slot-0 (`Transmit_Radio_Impl 0x0065A970`). → Remove `RefineryDockContacts.waiting_retry_queue` (Slice 4) and `AirfieldDocks.queues` (Slice 7a); replace with on-demand re-probe + distance-biased winner. Delete the `airfield_docks_release_pad_1_promotes_into_pad_1` test — it locks the wrong FIFO-pin behavior. |
| **V4-Rescue** | Mission 21 (Rescue) YR-reachable? | **RESOLVED — YES (live AI behavior)** | Real handlers (`FootClass__Mission_Rescue 0x004ddf90`, `AircraftClass__Mission_Rescue 0x00415960`); live assigner `FUN_00708080` via `ReceiveDamage` family (gated `IsPlayerControl()==0`, AI-only). Fires every AI skirmish. → Include `MissionType::Rescue(21)` with a real handler; AI-only, scope into the AI threat-response path (out of the Slice-set's player-command scope but **must exist in the enum**). Correct design §3.1 "no live trigger" for Rescue. |
| **V4-Ambush** | Mission 14 (Ambush) YR-reachable? | **RESOLVED — NO (dead stub, TS-legacy)** | No handler (base `Mission_Default 0x005B2E10` = `return 0x1C2`), no live assigner. → Keep `Ambush(14)` as an **inert no-op enum variant** for map-INI name round-trip only; implement no logic. |
| **V5-RadioHistory** | RadioHistory (+0xD4/+0xD8/+0xDC) readers | **RESOLVED — OMIT-SAFE (HIGH)** | Sole writer is base `RadioClass::Receive_Radio 0x0065A820` (write-only push-down); no subclass override reads it; binary-wide read scan found only other classes' +0xD4/D8/DC. InfantryClass has no own `Receive_Radio` (inherits FootClass `0x004D8FB0`). → **Omit RadioHistory** from the Rust port. (Downgraded from "proven-inert" to "omit-safe HIGH" — exotic encodings not 100% excluded; STILL-UNCHECKED at that residual level only.) |
| **V5-override-map** | Per-subclass mission-handler override map (+0x204..+0x270) | **RESOLVED (full map enumerated)** | Per-category real-handler sets verified (see §C.0 table). DRIFT correction: design §3.1's claim "AircraftClass overrides the +0x204 Sleep slot with a QMove handler" is **WRONG** — `AircraftClass__Mission_QMove 0x00415A50` is installed at slot **+0x230 (Retreat, mission 4)**; Aircraft Sleep(0) is the base stub. Mission 3 (QMove) routes to the Sleep slot (+0x204) for *all* classes. → Carry this correction into the dispatch slice (Slice 6+); do not "preserve a +0x204 override quirk" that does not exist. |

---

## B. Canonical substrate API (consistency reconciliation)

The 7 drafts diverged on the substrate's surface API. This section is the **single canonical shape** every slice below uses. Where a draft used a different spelling, the divergence and the merge are recorded; **no draft's content is dropped** — divergent methods are folded into the canonical set as aliases or unified names.

### B.1 `MissionTimer` (reconciled)

Drafts diverged: P0 `start_frame`/`duration` (public) + `defer`/`due`/`elapsed`/`remaining`; P1/P3 `start`/`duration` (private) + `arm`/`due`/`clear`/`remaining`/`is_armed`; P6 referenced `reset`. **Canonical decision:** public fields `start_frame: u32, duration: u32` (P0/P6 spelling — the gate adopter in Slice 1 needs field access for `reverse_transition`), and the **union of all methods** so every consumer compiles unchanged:

| Canonical method | Semantics | Drafts that used it (alias merged) |
|---|---|---|
| `due(now) -> bool` | `start_frame == SENTINEL \|\| now.wrapping_sub(start_frame) >= duration` (inclusive) | P0, P1, P3, P5, P6 |
| `defer(&mut, now, n)` | `start_frame=now; duration=n` | P0, P2 (P3/P5 spelled this `arm`) |
| `arm(&mut, now, n)` | **alias of `defer`** (kept so P3/P5 call sites compile) | P3, P5 |
| `armed(now, n) -> Self` | constructor (the gate's `armed(0,0)` default needs this) | P0, P1 |
| `clear(&mut)` | `start_frame=SENTINEL; duration=0` (→ always due) | P3, P5 |
| `reset(now)` | **alias of `defer(now, 0)`** (Slice 6 `assign_mission` calls `reset`) | P6 |
| `elapsed(now) -> u32` | `0` if sentinel else `now.wrapping_sub(start_frame)` (gate reversal needs this) | P0 |
| `remaining(now) -> u32` | `0` if sentinel else `duration.saturating_sub(elapsed)` | P0, P3, P5 |
| `is_armed() -> bool` | `start_frame != SENTINEL` | P3, P5 |

`SENTINEL = u32::MAX`. `Default` → `armed(SENTINEL, 0)` (always due). **Exception:** `BuildingGateRuntime::Default` seeds `armed(0,0)` (NOT sentinel) — load-bearing for the gate's `wrapping_sub(0)` arithmetic (Slice 1, §C.1). Derives `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize`.

### B.2 `MissionType` (reconciled)

Drafts diverged hardest here. P0: 32 variants, no `None`, `Wait`(28) + `AttackMove`(29), `repr(u8)`. P1: adds `None=0xFF`, renames 28→`Deliberate`, drops `AttackMove`, `repr(u8)`. P6: needs `repr(u16)` for the hash cast. V4: `Rescue(21)` real, `Ambush(14)` inert. **Canonical decision:**

- `#[repr(u16)]` (P6's hash-cast requirement; the discriminant still equals the gamemd mission id for 0–31; cast as `u16` is the stable hash fold).
- Include all 32 dispatched ids **plus** a `None = 0xFF` sentinel (P1) — `Default = None`.
- Index 28 named **`Deliberate`** with a doc note "Wait in the mission-name table; Deliberate in the FOOTCLASS report" (merges P0's `Wait` + P1's `Deliberate` — same field). Provide `pub const Wait: MissionType = MissionType::Deliberate;`-style aliasing in docs only; one variant.
- Keep **`AttackMove = 29`** (P0) as a representable-but-never-dispatched selector with a doc note "no gamemd dispatch case; resolved upstream as a queued command; dispatcher must skip it." (P1 dropped it; we keep it because `world_commands.rs` AttackMove needs a selector, and the dispatcher skip is the parity requirement.)
- `Ambush = 14` present but **inert** (V4): round-trips for map-INI name fidelity, executes as Sleep/Default no-op.
- `Rescue = 21` present **with a real (AI-only) handler** (V4).
- `Eaten = 9` retained (TS-legacy index shift origin).
- Derives `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize` (P0's determinism note: `Ord` required for the `BTreeMap<MissionType,_>` key in `MissionControl`).
- `from_id(u8) -> Option<Self>` is an explicit match (no transmute — lockstep forbids UB on a malformed map byte ≥ 32). `id()/dispatch_id() -> u8` (both names provided; `dispatch_id` aliases `id`). `ini_section() -> &'static str`. `all()` iterates 0..32.

### B.3 `Contacts` (reconciled)

Drafts diverged: P1 `insert`/`insert_evicting`/`remove`/`find_slot`/`slot`/`clear_all`/`iter_live`/`with_capacity`; P2 `insert_first_free`/`break_with`/`set_capacity`/`contains`/`find_slot`/`iter`/`hash_fold`/`len`. **Canonical decision — the union, with one name per behavior:**

| Canonical method | Semantics | Merged from |
|---|---|---|
| `with_capacity(n) -> Self` | capacity `n.max(1)`, all `None` | P1 |
| `set_capacity(&mut, n)` | **grow-only** resize to `n.max(1)` (never shrinks) | P2 |
| `capacity() -> usize` | slot count | P1, P2 |
| `contains(id) -> bool` | membership (the sole load-bearing `Can_Enter_Cell` reader) | P1, P2 |
| `find_slot(id) -> Option<usize>` | slot index = pad-index basis (§5.2.11) | P1, P2 |
| `slot(i) -> Option<u64>` | indexed read (for hash fold) | P1 |
| `len() / is_empty()` | filled-slot count / all-empty | P1, P2 |
| `insert(id) -> Option<usize>` | **receiver-side** first-null insert; `None` when full; idempotent → existing slot. **NO eviction** (V3) | P1 (P2 spelled this `insert_first_free -> bool`; canonical returns `Option<usize>`, and an `insert_first_free(id)->bool` wrapper is kept for P2/P4 call sites) |
| `insert_evicting(id) -> (usize, Option<u64>)` | **sender-side** slot-0 self-evict when full (`Transmit_Radio_Impl`) | P1 |
| `remove(id) -> Option<usize>` | BREAK: null first matching slot, no compaction | P1 (P2 spelled `break_with -> bool`; canonical `remove` returns the slot, `break_with(id)->bool` wrapper kept) |
| `clear_all(&mut)` | null every slot, preserve capacity (teardown/limbo) | P1 |
| `iter_live() -> impl Iterator<Item=u64>` | live ids in slot order (broadcast-BREAK) | P1 (P2 `iter`; both names provided) |
| `hash_fold<H>(&self, &mut H)` | capacity + per-slot `Option` by index (slot position is hash-relevant — V3) | P2 |

`Default` = capacity-1, one empty slot (gamemd default ctor). Derives `Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize`. **GameEntity field name reconciliation (P1 vs P2):** the field is named **`radio_contacts`** and its **type changes** `Vec<u64> → Contacts` (P1's rename-free approach — preserves the 25+ call sites that go through helper methods). P2's "add `radio` beside the Vec" approach is **rejected** as the canonical end-state, but P2's *staging* (keep the Vec live through Slice 4, migrate the type in Slice 3) is honored by **slice ordering**: Slice 3 performs the `Vec→Contacts` type swap; Slice 4 then consumes `radio_contacts: Contacts` directly. (If Slice 4 must land before Slice 3 in a given session, it uses P2's interim "add a parallel field" shim — documented as the fallback, not the target.)

### B.4 `RadioMessage` / `RadioResponse` (reconciled)

P0 and P2/P4 both define these; canonical = the **superset** (P0's full table for scaffolding; P2/P4's refinery subset is a strict subset of it). `RadioMessage` is `#[repr(u8)]` with the gamemd opcode as discriminant (`Hello=0x02, Break=0x03, … DockNow=0x15, … IsOccupied=0x23`), `0x24 WantRide` omitted (dormant). `RadioResponse` `#[repr(u8)]` (`None=0, Roger=1, Negatory=0x0A, CellAccepted=0x14, Queued=0x17, InsufficientFunds=0x20, RepairComplete=0x21`). Both derive `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize` + `code() -> u8`. `RadioPayload { cell: Option<(u16,u16)> }` (P4).

### B.5 `now` source (reconciled)

Drafts split: P0/P1/P2/P3/P5/P7 use `sim.binary_frame` (the `g_CurrentFrameCounter` analogue, committed late at `world/mod.rs:1655–1663`); P6 used `self.tick as u32`. **Canonical: `sim.binary_frame`** everywhere. P6's `self.tick as u32` is corrected to `self.binary_frame` in the Slice-6 verb call sites (the verbs take `now: u32`; the caller passes `binary_frame`). This is the single time base; mixing `tick` and `binary_frame` would reintroduce the Risk-2 drift the whole migration removes.

---

## C. The nine slices, in dependency order

Linearized critical path: **0 → {1, 2, 3} → {4, 5} → {6, 7} → 8**.

### Slice 0 — Substrate scaffolding (types only, no consumer)

**Goal.** Add the canonical mission vocabulary (`MissionType`, §B.2), radio vocabularies (`RadioMessage`/`RadioResponse`, §B.4), and the `MissionControl` INI table parsed from the `[MissionName]` sections. No consumer reads these yet → **state hash unchanged by construction** (no `GameEntity` field, no `world_hash` fold).

**Files.**
- NEW `src/sim/mission/mod.rs` — `MissionType` (§B.2), module exports, `MISSION_COUNT=32`.
- NEW `src/sim/mission/control.rs` — `MissionControl` / `MissionControlEntry`.
- NEW `src/sim/mission/timer.rs` — `MissionTimer` (§B.1) (file created here so the module tree compiles; the gate adopter that *consumes* it is Slice 1).
- NEW `src/sim/radio/mod.rs` — `RadioMessage`/`RadioResponse`/`RadioPayload` (§B.4) + module exports.
- EDIT `src/sim/mod.rs` — after `pub mod world;` (verified line 35), add `pub mod mission;` + `pub mod radio;`.
- EDIT `src/rules/ruleset.rs` — struct field after `c4_warhead_id` (verified line 1381, struct closes 1382): `pub mission_control: crate::sim::mission::MissionControl,`. Parse in `from_ini` after the `[CombatDamage]` block (insert after verified line 1550): `let mission_control = crate::sim::mission::MissionControl::from_ini(ini);`. Struct construction after `c4_warhead_id: None,` (verified line 1662): `mission_control,`.

**Full code — `MissionType`** (canonical §B.2; the explicit `from_id`/`ini_section`/`all` bodies are exactly P0's, with `repr(u16)`, the added `None=0xFF`/`Default`, `Deliberate`(28) doc-merge, retained `AttackMove`(29), and `PartialOrd,Ord` added):

```rust
//! Mission scheduler substrate — vocabulary + components.
//! Models the gamemd MissionClass *contract* as a Rust-native service (single
//! current-mission selector + frame-anchored dispatch timer + verb API), NOT the
//! C++ class tree. `timer` owns the deferral primitive; `control` the INI table;
//! verbs + dispatch land in later slices. Depends on rules/ (MissionControl
//! parses from IniFile); sim/ only — never render/ui/sidebar/audio/net.

pub mod control;
pub mod timer;
pub use control::{MissionControl, MissionControlEntry};
pub use timer::MissionTimer;

pub const MISSION_COUNT: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
         serde::Serialize, serde::Deserialize)]
#[repr(u16)]
pub enum MissionType {
    /// No committed mission (gamemd CurrentMission == -1 / idle). Sentinel 0xFF.
    #[default]
    None = 0xFF,
    Sleep = 0, Attack = 1, Move = 2, QMove = 3, Retreat = 4, Guard = 5, Sticky = 6,
    Enter = 7, Capture = 8,
    /// TS-legacy; occupies index 9 and shifts the rest vs the "clean" YRpp enum.
    Eaten = 9,
    Harvest = 10, AreaGuard = 11, Return = 12, Stop = 13,
    /// Dead stub in YR (base Mission_Default, no live assigner — V4). Round-trips
    /// for map-INI name fidelity; executes as Sleep-equivalent no-op.
    Ambush = 14,
    Hunt = 15, Unload = 16, Sabotage = 17, Construction = 18, Selling = 19,
    Repair = 20,
    /// Live AI-only behavior (V4): AI tasks idle teammates to converge on an
    /// attacker via ReceiveDamage. Real handler required; never player-assigned.
    Rescue = 21,
    Missile = 22, Harmless = 23, Open = 24, Patrol = 25,
    ParadropApproach = 26, ParadropOverfly = 27,
    /// gamemd 0x1C. "Wait" in the mission-name table, "Deliberate" in the
    /// FOOTCLASS report — same field. Guard-protected interrupt mission.
    Deliberate = 28,
    /// No gamemd dispatch case — resolved upstream as a queued command, never
    /// executed as a CurrentMission. Present so the selector can represent the
    /// command; the dispatcher MUST skip it (parity requirement).
    AttackMove = 29,
    SpyplaneApproach = 30, SpyplaneOverfly = 31,
}

impl MissionType {
    #[inline] pub fn id(self) -> u8 { self as u8 }
    #[inline] pub fn dispatch_id(self) -> u8 { self as u8 } // alias

    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            0 => Self::Sleep, 1 => Self::Attack, 2 => Self::Move, 3 => Self::QMove,
            4 => Self::Retreat, 5 => Self::Guard, 6 => Self::Sticky, 7 => Self::Enter,
            8 => Self::Capture, 9 => Self::Eaten, 10 => Self::Harvest, 11 => Self::AreaGuard,
            12 => Self::Return, 13 => Self::Stop, 14 => Self::Ambush, 15 => Self::Hunt,
            16 => Self::Unload, 17 => Self::Sabotage, 18 => Self::Construction,
            19 => Self::Selling, 20 => Self::Repair, 21 => Self::Rescue, 22 => Self::Missile,
            23 => Self::Harmless, 24 => Self::Open, 25 => Self::Patrol,
            26 => Self::ParadropApproach, 27 => Self::ParadropOverfly, 28 => Self::Deliberate,
            29 => Self::AttackMove, 30 => Self::SpyplaneApproach, 31 => Self::SpyplaneOverfly,
            _ => return None,
        })
    }

    pub fn ini_section(self) -> &'static str {
        match self {
            Self::Sleep => "Sleep", Self::Attack => "Attack", Self::Move => "Move",
            Self::QMove => "QMove", Self::Retreat => "Retreat", Self::Guard => "Guard",
            Self::Sticky => "Sticky", Self::Enter => "Enter", Self::Capture => "Capture",
            Self::Eaten => "Eaten", Self::Harvest => "Harvest", Self::AreaGuard => "Area Guard",
            Self::Return => "Return", Self::Stop => "Stop", Self::Ambush => "Ambush",
            Self::Hunt => "Hunt", Self::Unload => "Unload", Self::Sabotage => "Sabotage",
            Self::Construction => "Construction", Self::Selling => "Selling",
            Self::Repair => "Repair", Self::Rescue => "Rescue", Self::Missile => "Missile",
            Self::Harmless => "Harmless", Self::Open => "Open", Self::Patrol => "Patrol",
            Self::ParadropApproach => "ParadropApproach", Self::ParadropOverfly => "ParadropOverfly",
            Self::Deliberate => "Wait", Self::AttackMove => "AttackMove",
            Self::SpyplaneApproach => "SpyplaneApproach", Self::SpyplaneOverfly => "SpyplaneOverfly",
            Self::None => "None",
        }
    }

    /// Iterate all 32 dispatched missions in index order (table builds, round-trip).
    pub fn all() -> impl Iterator<Item = MissionType> {
        (0u8..MISSION_COUNT as u8).filter_map(MissionType::from_id)
    }
}
```

**Full code — `MissionControl`** (exactly P0's `control.rs`: `FRAMES_PER_MINUTE=900.0`, carry-forward accumulator, `AARate` absent/0 copies Rate, integer-frames pre-converted at parse). Reproduced verbatim from draft P0 §0.NEW `control.rs` — including the `MissionControlEntry` struct, `Default`, `rate_to_frames`, `from_ini` with the running accumulator, `entry`/`rate_frames`. **One reconciliation:** `MissionType` now derives `Ord`, so the `BTreeMap<MissionType, MissionControlEntry>` key compiles unchanged.

> **STILL-UNCHECKED (carried from P0):** whether gamemd's `Read_INI` carries the previous entry's value forward on an omitted key (the running accumulator) or resets each entry to header defaults. P0 models the carry-forward (the "model the primitive" choice). If a later session proves reset-per-entry via `Read_INI @ 0x005B3760`, swap `running` for a fresh `MissionControlEntry::default()` per iteration. Flagged here, not silently assumed correct.

**Full code — `RadioMessage`/`RadioResponse`/`RadioPayload`** (canonical §B.4 — P0's full table is the superset; reproduced verbatim from P0 §0.NEW `radio/mod.rs`, plus `RadioPayload` from P4).

**Full code — `MissionTimer`** (canonical §B.1, union of all draft methods):

```rust
//! `MissionTimer` — the single frame-anchored deferral primitive.
//! gamemd's mission/CDTimer throttle snapshots the global frame counter and tests
//! a delta — it never decrements, so skipped ticks never drift the cadence. This
//! generalizes the building-gate's already-correct (last_frame + ticks_remaining)
//! model. Pure integer u32, wrapping arithmetic, no float. sim/ only.
use serde::{Deserialize, Serialize};

/// "Unarmed / always due" (gamemd -1 start). u32::MAX: the live counter starts at
/// 0 and would take ~3.3 years at 15fps to reach it, so it is never a live value.
pub const SENTINEL: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MissionTimer { pub start_frame: u32, pub duration: u32 }

impl Default for MissionTimer {
    fn default() -> Self { Self { start_frame: SENTINEL, duration: 0 } }
}

impl MissionTimer {
    #[inline] pub fn armed(start_frame: u32, duration: u32) -> Self { Self { start_frame, duration } }
    #[inline] pub fn due(self, now: u32) -> bool {
        self.start_frame == SENTINEL || now.wrapping_sub(self.start_frame) >= self.duration
    }
    #[inline] pub fn defer(&mut self, now: u32, n: u32) { self.start_frame = now; self.duration = n; }
    #[inline] pub fn arm(&mut self, now: u32, n: u32) { self.defer(now, n); }     // P3/P5 alias
    #[inline] pub fn reset(&mut self, now: u32) { self.defer(now, 0); }           // P6 alias
    #[inline] pub fn clear(&mut self) { self.start_frame = SENTINEL; self.duration = 0; }
    #[inline] pub fn is_armed(self) -> bool { self.start_frame != SENTINEL }
    #[inline] pub fn elapsed(self, now: u32) -> u32 {
        if self.start_frame == SENTINEL { 0 } else { now.wrapping_sub(self.start_frame) }
    }
    #[inline] pub fn remaining(self, now: u32) -> u32 {
        if self.start_frame == SENTINEL { 0 }
        else { self.duration.saturating_sub(now.wrapping_sub(self.start_frame)) }
    }
}
```

**Acceptance.** `cargo test -p vera20k mission::` — round-trip all 32 ids; verified-binary spot indices (Sleep=0, Guard=5, Enter=7, Eaten=9, Harvest=10, AreaGuard=11, Selling=19, Rescue=21, AttackMove=29, SpyplaneOverfly=31); Rate→frames via 900 (`1.0→900`, `.016→14`, `.030→27`, `.040→36`); AARate absent copies Rate, present overrides; bools with documented defaults; table fully populated. `MissionTimer` unit tests: unarmed always-due, inclusive boundary, defer-zero due-next, elapsed/remaining, wraparound. (Test bodies verbatim from P0 §0.ACCEPTANCE and §1 `timer.rs` tests.)

**Determinism.** No float in any tick path (`f64` only in `rate_to_frames` at parse). `BTreeMap<MissionType,_>` deterministic (Ord). No `world_hash` fold → hash unchanged by construction.

**Dependencies.** None prior. Produces every substrate type the later slices consume.

---

### Slice 1 — `MissionTimer` first consumer: building gate runtime

**Goal.** Migrate the building gate runtime — the only existing correct frame-delta adopter — onto `MissionTimer`. Lowest-risk first consumer (it already uses the exact `(last_frame, ticks_remaining)` pair). Closes the gate's bespoke transition/hold timers (part of §7 #6). **Bit-identical cadence is the acceptance bar** — the existing gate test suite must pass unchanged (modulo field-name renames).

**Files.**
- EDIT `src/sim/game_entity.rs` `BuildingGateRuntime` (struct lines 88–106, Default 108–121): replace `transition_ticks_remaining`+`transition_last_frame` with `transition_timer: MissionTimer`; replace `hold_ticks_remaining`+`hold_last_frame` with `hold_timer: MissionTimer`; keep `transition_total_ticks: u32` (nominal total, not a live timer). Default seeds **`armed(0,0)`** for both timers (NOT sentinel — load-bearing).
- EDIT `src/sim/gate_runtime.rs`: re-express `seed_hold_timer` (17–20), `start_opening` (22–37), `start_closing` (39–54), `reverse_transition` (56–71), `advance_remaining`→`advance_hold` (73–78) + its `OpenHold` call site (248–258), `advance_transition` (80–97) — each a **representation swap preserving identical arithmetic**, not a logic rewrite.
- EDIT `src/sim/gate_runtime.rs` tests: `closed_or_closing_request_restarts_mission_setup` (312–329), `closing_rerequest_preserves_native_start_frame_baseline` (332–355), `hold_reseeds_while_obstructed_then_closes_after_clear_delay` (291–310) — field-name updates only (`transition_timer.duration`/`.start_frame`, `hold_timer.duration`).

**Edit shapes** (verbatim from P0 §1 Edits A/B1–B6/C1–C3). The load-bearing one is `reverse_transition`: `elapsed = transition_timer.elapsed(frame); live_remaining = transition_timer.duration.saturating_sub(elapsed); transition_timer.duration = transition_total_ticks.saturating_sub(live_remaining)` — `start_frame` untouched (preserves the native baseline the `closing_rerequest…` test asserts: armed(100,39)@110 → duration 10, start 100). `advance_transition` uses `!transition_timer.due(frame)` (the exact complement of the old `elapsed < remaining` early-out). `advance_hold` re-anchors start to `now` and saturating-decrements (reproduces the old `advance_remaining` field math exactly).

**Acceptance.** `cargo test -p vera20k gate_runtime::` (the full existing gate suite is the bit-identical gate) + `cargo test -p vera20k mission::timer` (the `reversal_arithmetic_matches_gate_reverse` unit test pins armed(100,39)@110 → duration 10, start 100).

**Determinism.** Wrapping arithmetic + inclusive due-gate match the gate's existing `wrapping_sub` convention. No `world_hash` fold change — the two `MissionTimer` fields hold the same numeric values the old `u32` pairs held (regrouped into structs).

> **STILL-UNCHECKED / save-compat caveat (P0):** an OLD serialized gate (with `transition_ticks_remaining`/`transition_last_frame`/`hold_*` keys) would deserialize the new `#[serde(default)]` fields to `MissionTimer::default()` (SENTINEL), not `armed(0,0)`. Save format is internal/non-cross-version-stable here (harness re-baselines per slice), so acceptable; if cross-version save-compat is later required, add `#[serde(default = "…")]` returning `armed(0,0)`. Flagged, not silently assumed.

**Dependencies.** Slice 0 (`MissionTimer` exported). Consumes only `MissionTimer` — not `MissionCom`/`MissionControl`/radio.

---

### Slice 2 — `MissionCom` in shadow mode

**Goal.** Add the `MissionCom` component to `GameEntity` in **shadow mode**, mirroring the `Presence` shadow precedent (`game_entity.rs:129–151`, `world/mod.rs:840–852`). Each tick *derive* `current`/`substate` from the existing `Option<T>` machines; a `#[cfg(debug_assertions)]` assert proves agreement. **Not read by any system, NOT hashed, NOT serialized** (`#[serde(skip)]`). Carrier for §7 #1–#5 (retirements land in Slice 6).

**Files.**
- EDIT `src/sim/mission/mod.rs` — add `MissionCom` struct (the shadow component). Fields: `current: MissionType`, `queued: Option<MissionType>`, `suspended: Option<MissionType>`, `substate: u8`, `timer: MissionTimer`, `tick_counter: u32`. Derives `Debug, Clone, Copy, PartialEq, Eq, Default`. Add `MissionCom::idle()` ctor (= `default()`, current=`None`).
- EDIT `src/sim/game_entity.rs` — field `#[serde(skip)] pub mission_com: MissionCom` inserted before `debug_log` (after `rocking`, verified lines 437–442); init in `new()` after `rocking: None,` (verified lines 580–581); add `derived_mission(&self) -> (MissionType, u8)` after `derived_presence` (ends line 457).
- EDIT `src/sim/world/mod.rs` — `refresh_mission_shadow()` + `#[cfg(debug_assertions)] debug_assert_mission_shadow_consistent()` after `debug_assert_presence_consistent` (ends line 852); call `refresh_mission_shadow()` in `advance_tick` tail before `state_hash()` (verified lines 2309–2313); merge the mission rebuild into the existing `rebuild_logic_membership` `presence =` loop (lines 1133–1148).

**Edit shapes.** `MissionCom`, `derived_mission`, `refresh_mission_shadow`, the assert, and the rebuild merge are verbatim from P1 §2.2–2.3. **One reconciliation vs P1:** P1's `MissionType` had `Default = None=0xFF` and no `Deliberate`/`AttackMove` distinction — the canonical enum (§B.2) already supplies `Default=None` and both variants, so `derived_mission`'s `_ =>` arms and the `MissionType::None` returns compile unchanged. The field is named `mission_com` in P1; Slice 6 (P6) names it `mission`. **Canonical: `mission_com` in Slice 2** (the shadow field), **renamed to `mission` when it becomes authoritative in Slice 6** (Slice 6's edits explicitly do this rename as part of making it authoritative — see consistency-audit note §E.1). The derive priority in `derived_mission` (miner → aircraft_mission → dock_state → attack_target → movement_target → None) is P1's; the coder confirms the `MinerState`/`AircraftMission` variant lists at `miner/mod.rs:63`/`aircraft/mod.rs:35` and extends the match (never widens the assert tolerance).

**Acceptance.** `cargo test -p vera20k mission` + `cargo test -p vera20k mission_shadow`: `mission_com` defaults to `None` and is `#[serde(skip)]` (absent in serialized form); `derived_mission` tracks attack_target; `mission_shadow_does_not_change_state_hash` (snapshot before/after `refresh_mission_shadow()` equal). Test bodies verbatim from P1 §2.4.

**Determinism.** `#[serde(skip)]` + omitted from `world_hash` → cannot perturb the lockstep hash (proven by the no-change test). `values_mut()` = BTreeMap ascending order. `MissionTimer` integer math; `tick_counter` uses `wrapping_add`. Scheduler draws zero RNG.

**Dependencies.** Slice 0 (`MissionType`) + Slice 1 (`MissionTimer`). Independent of Slice 3.

---

### Slice 3 — `Contacts` slot model (replace `radio_contacts: Vec<u64>`)

**Goal.** Replace `radio_contacts: Vec<u64>` (`game_entity.rs:240`) with the sparse, capacity-bounded `Contacts` component (§B.3): first-null insert, null-hole removal (no compaction), slot-0 sender self-evict, capacity `max(NumberOfDocks,1)` for buildings else 1. Closes §7 #8. Keeps the sole load-bearing reader (`Can_Enter_Cell` membership skip, `movement_occupancy.rs:326`) working unchanged. **Does NOT add `transmit`/`receive_radio`** (Slice 4) and **does NOT touch** the FIFO dock registries (Slice 4/7).

**Files.**
- NEW `src/sim/radio/contacts.rs` — `Contacts` (canonical §B.3, full method set). Body verbatim from P1 §3.2 + P2's `set_capacity`/`hash_fold`/`break_with`/`insert_first_free` wrappers merged in.
- EDIT `src/sim/radio/mod.rs` — `pub mod contacts; pub use contacts::Contacts;`.
- EDIT `src/sim/game_entity.rs` — field type swap `radio_contacts: Vec<u64> → Contacts` (line 240, **name preserved**); init `Contacts::default()` (line 516); rewrite the 3 helpers `mark_live_contact_with`/`has_live_contact_with`/`clear_live_contact_with` (lines 592–610) to delegate to `insert`/`contains`/`remove`.
- EDIT `src/sim/entity_store.rs` — `clear_radio_contacts_for` (lines 80–87) line 84 `radio_contacts.clear()` → `radio_contacts.clear_all()`.
- EDIT `src/sim/world/world_hash.rs` — replace the `radio_contacts.len()`+loop fold (lines 482–485) with the slot-indexed `Contacts::hash_fold` (capacity + per-slot `Option`). **Intended hash change** at this behavior-bearing boundary — re-baseline.
- EDIT `src/sim/world/world_spawn.rs` — mcv_redeploy gate (line 813) `!entity.radio_contacts.is_empty()` compiles unchanged (`Contacts::is_empty` exists).
- EDIT building-spawn finalizer (locate via `obj.number_of_docks`, e.g. near `production_spawn.rs:716`): widen a building's capacity `radio_contacts = Contacts::with_capacity(obj.number_of_docks.max(1) as usize)`. If the site is not yet wired, capacity-1 default is parity-correct for every stock refinery (`NumberOfDocks=1`); only multi-pad airfields (Slice 7) need >1.
- EDIT test call sites asserting raw-`Vec` equality (`entity_store.rs:293,294,309,310,323,338`, `game_entity.rs:851`, `passenger.rs:1831`, `world_tests.rs:157`, `world_hash.rs`): rewrite `== vec![..]` to per-id `has_live_contact_with` + count (slot order ≠ insertion order now).

**Acceptance.** `cargo test -p vera20k contacts` (slot model: default cap-1 empty; first-null insert returns slot; idempotent; receiver-full→`None` no-evict; sender `insert_evicting` slot-0 evict; remove nulls in place; `iter_live` skips holes in slot order). Plus existing suites green after the membership-assert rewrites: `cargo test -p vera20k radio_contacts`, `miner_dock`, `entity_store`. Test bodies verbatim from P1 §3.4.

**Determinism.** `Contacts` round-trips serde + folds into `world_hash` by slot index (null holes + pad-bearing positions are hash-relevant — V3). Index/`Option` arithmetic only — no float/RNG. Capacity fixed at construction, preserved by `clear_all` (grow-only, never shrinks).

**Dependencies.** Slice 0 (radio module exists). Independent of Slices 1/2. **Blocks Slice 4** (refinery bus needs the slot model) and **Slice 7** (airfield pad-index = slot index). Per V3, the FIFO registries (#9) are NOT removed here — deferred to 4/7.

---

### Slice 4 — RadioBus: refinery dock idiom

**Goal.** Stand up the synchronous `transmit()`/`receive_radio()` bus (§6.2) and route the refinery inbound dock choreography (HELLO→CAN_DOCK→accepted-cell→ENTER_DOCK→TIMING_SYNC→DOCK_NOW→deposit→BREAK) through it, replacing the ad-hoc `RefineryDockContacts` admission. Closes §7 #8 for the refinery consumer and the refinery half of #9. **Removes the V3-proven DRIFT** `RefineryDockContacts.waiting_retry_queue` (no gamemd counterpart) → on-demand re-probe admission.

**Hard preservation (verified):** 14.4-tick whole-slot deposit cadence (`miner/mod.rs:174,204-208`; `miner_dock_sequence.rs:1054-1141`); ore-then-gem order (`SLOT_ORDER`, `miner_dock_sequence.rs:1064`); zero-link refinery (`+0x2E4==0`, verified `0x0043C2D0` case `0x15` — only `Type[0x16b3]` tank-bunker queues `0x10` on the sender; Refinery branch does not); capacity-1 no-evict 2nd HELLO (`0x0043C2D0` case `0x0E`, receiver never evicts); CAN_DOCK accepted cell = building NW + (3,1) (`0x0043C2D0`, `*psVar5+3, psVar5[1]+1`; Rust `miner_dock_sequence.rs:199-201`).

**Files.**
- NEW `src/sim/radio/receive.rs` — `receive_radio(sim, target_sid, sender_sid, msg, payload) -> RadioResponse`; the zero-link refinery branch (`CanDock`/`EnterDock`/`LeaveDock`/`TimingSync`/`DockNow`/`MoveToCell`/`Break`). `REFINERY_ACCEPTED_DX=3`/`DY=1`; `refinery_accepted_cell(rx,ry)`. Body verbatim from P4 §2.
- EDIT `src/sim/radio/mod.rs` — add `pub mod receive; pub use receive::receive_radio;` + the `transmit()` free function (HELLO/BREAK contact bookkeeping centralized: ally + alive + idempotent + first-null insert, NO eviction; other opcodes dispatch straight to the receiver). RTTI sender filter. Body verbatim from P4 §2 `transmit`/`hello`.
- EDIT `src/sim/game_entity.rs` — **`dock_entered_with: Option<u64>`** field (models the `+0x418` dock-entered flag), `#[serde(default)]`, init `None`. (P4 also added a parallel `radio: Contacts` field — **rejected**: canonical uses `radio_contacts: Contacts` from Slice 3. Slice 4 consumes `radio_contacts` directly. If Slice 3 has NOT landed in this session, fall back to P4's interim `radio: Contacts` shim and reconcile at Slice 3 merge — see §E.2.)
- EDIT `src/sim/world/world_hash.rs` — fold `dock_entered_with` (after the Slice-3 `Contacts::hash_fold`): `0u8`/`1u8`+sid. **Intended hash change** — re-baseline.
- EDIT `src/sim/miner/miner_dock_sequence.rs` — route the dock handshake through `transmit()`: `phase_approach` (783–792) HELLO; `phase_mission_enter` (819–894) HELLO + EnterDock + `dock_entered_with` read; `phase_face_sync` (931–933) EnterDock; `phase_departing` (1170–1176) BREAK. Keep the physical pad (`on_pad`) and the phase FSM cadence exactly as-is. Abort/interrupt helpers also clear `dock_entered_with` + `radio_contacts.remove(entity_id)`. Edit shapes verbatim from P4 §3.4a–e.
- EDIT `src/sim/miner/miner_dock.rs` — mark `waiting_retry_queue`, `is_waiting`, `remove_waiter`, `next_waiter` for deletion (the V3 DRIFT fix); keep `on_pad` + `try_reserve`/`release`/`cancel` test-compat helpers.

**Edit shapes.** `transmit`/`hello`/`receive_radio`/`refinery_receive` verbatim from P4. **Reconciliation vs §B.3:** P4's `hello()` calls `target.radio.set_capacity(...)`/`.contains`/`.insert_first_free`; canonical `Contacts` provides `set_capacity`/`contains`/`insert_first_free` (the `bool` wrapper over `insert`), so P4's bus code compiles against the canonical `radio_contacts: Contacts` field by replacing `target.radio` → `target.radio_contacts`. The ally gate is owner-equality (no ally graph yet, confirmed — no `is_ally` in `src/sim`); swap for `is_ally()` when the ally graph lands (matches stock skirmish: own-owner refinery only).

**Acceptance.** `cargo test -p vera20k miner::` (the existing miner-dock suite is the primary gate, with the one `is_waiting` assertion in `dock_queuing_one_at_a_time @ miner_tests.rs:680` rewritten to "m2 is NOT in the refinery's `radio_contacts` and re-sends HELLO next tick"). Locked behaviors: `credits_arrive_per_slot_during_unload` (755), `unloading_emits_one_event_per_slot_drain` (3475), `dock_first_slot_drain_waits_one_unload_interval` (4599), `full_dock_cycle_war_miner` (4235), `departing_handoff_releases_dock_and_returns_to_search` (3977). Plus new module tests `cargo test -p vera20k radio::` (cap-1 2nd HELLO Negatory no-evict; enemy HELLO Negatory) and the two new integration tests `refinery_cycle_over_radio_bus_matches_registry_cadence` / `full_unload_credits_unchanged_over_bus`. Bodies verbatim from P4 §1/§4.

**Determinism.** Bus is pure integer/enum; only dock-path RNG (`miner_jitter_rng`) stays in the phase handlers, unchanged (scheduler/bus draws zero RNG). `Contacts` hash-folded in stable slot order; HELLO first-null scan deterministic; no `HashMap` introduced. Miner dispatch over `keys_sorted()` (BTreeMap) unchanged → removing the FIFO introduces no new ordering source. **Hash baseline changes once** (`dock_entered_with` + slot-folded contacts) — re-baseline.

**Dependencies.** Slice 0 (radio enums) + **Slice 3** (`Contacts` slot model under the bus). Does NOT consume `MissionCom`/`MissionTimer` (the miner keeps its existing frame-anchored `dock_enter_retry_*`/`mission_deploy_*` timers — Slice 5 migrates them). The V2 aircraft latch is NOT exercised by the refinery idiom — deferred to Slice 7-aircraft.

---

### Slice 5 — Migrate bespoke per-subsystem timers onto `MissionTimer`

**Goal.** Replace every per-sim-tick decrement countdown in miner/dock/deploy/aircraft with the frame-anchored `MissionTimer`, and delete the duplicated fields. Closes §7 #6 (per-subsystem bespoke timers) and the active-timer half of #7 (dead `unload_timer: i16` / `deposit_cooldown_ticks` mirrors).

**The split (P3, re-read this session — do NOT trust a flat "decrement vs frame" split):**
- *Already frame-anchored* (`sim.binary_frame`-delta, pure type-rename, cadence trivially preserved): `dock_enter_retry_start_frame`/`_duration` (`miner_dock_sequence.rs:84-92`), `mission_deploy_start_frame`/`_duration` (104-111), `unload_cluster_start_frame`/`_duration`/`_repeat` (157-170, 988-991).
- *Per-sim-tick decrement* (the ONLY cadence-driftable ones — Risk 2): `harvest_timer: u8` (`miner_system.rs:547-548`), `rescan_cooldown: u8` (754-755), `service_timer`/`no_funds_ticks: u32` (`building_dock.rs:214,246`), `DeployPhase::{Deploying,Undeploying}{ticks_remaining}` (`deploy.rs:87-104`), `Docking.reload_timer: u32` (`aircraft/mod.rs:495`), `ParaDropOverfly.drop_cooldown: u16` (`paradrop_mission.rs:152`).
- *Dead in the active path* (delete with serde-default shims): `unload_timer: i16`, `deposit_cooldown_ticks: u16` (written `0`-only / read only in legacy `phase_deposit_cooldown @ miner_dock_sequence.rs:1146-1153`).

**Files & edit shapes.** Verbatim from P3 §3.1–3.7:
- `src/sim/miner/mod.rs` — fold `*_start_frame`/`*_duration` pairs into `MissionTimer` fields (`dock_enter_retry`, `mission_deploy_timer`, `unload_cluster_timer` + keep `unload_cluster_repeat: u32`); convert `harvest_timer`/`rescan_cooldown` `u8 → MissionTimer`; delete `unload_timer`/`deposit_cooldown_ticks`; init in `Miner::new`; `use crate::sim::mission::MissionTimer;`.
- `src/sim/miner/miner_system.rs` — `harvest_timer` gate `!due(sim.binary_frame)` + `arm(sim.binary_frame, …)` seeds (547-548, 592, 497); `rescan_cooldown` likewise (754-755, 426, 790).
- `src/sim/miner/miner_dock_sequence.rs` — `schedule_enter_retry`/`enter_retry_due`/`clear_enter_retry`/`schedule_mission_deploy_delay`/`mission_deploy_due`/`clear_mission_deploy_delay`/`tick_unload_accumulator` become thin `arm`/`due`/`clear` wrappers (no cadence change); jitter constants unchanged (`ENTER_RETRY_BASE_FRAMES=14`, `_JITTER_MAX=2`, etc.); delete `phase_deposit_cooldown` + `deposit_cooldown_ticks` sites.
- `src/sim/docking/building_dock.rs` — `service_timer: u32 → MissionTimer`; `no_funds_ticks` up-counter inverts to a `no_funds_grace: MissionTimer` (clear on funded tick, arm(now,30) on first failure, exit on `due`); `DockSnapshot`/`DockMutation` carry `MissionTimer`.
- `src/sim/deploy.rs` — `DeployPhase` → unit-like markers `{Deploying, Deployed, Undeploying}`; new `deploy_timer: MissionTimer` on `GameEntity`; `tick_deploy_state(entities, now)` gains `now: u32` (call site passes `self.binary_frame`).
- `src/sim/aircraft/mod.rs` — `Docking.reload_timer: u32 → MissionTimer`; `tick_aircraft_missions` adds `let now = sim.binary_frame;`.
- `src/sim/aircraft/paradrop_mission.rs` — `ParaDropOverfly.drop_cooldown: u16 → drop_timer: MissionTimer`; `tick_overfly(now)`.

**Acceptance.** `cargo test -p vera20k slice5_timer`: harvest cadence bit-identical (one bale per `harvest_tick_interval` frames, full-fill frame matches); unload deposit cadence + RNG cursor unchanged (jitter draw count/order identical → no desync); deploy completes on the same tick (inclusive boundary); aircraft reload + depot service/grace + paradrop drop cadence; cross-slice hash stable over the recorded skirmish (= the pre-Slice-5 baseline, since Slice 5 is behavior-preserving — only representation changed). Bodies verbatim from P3 §4.

**Determinism.** All `u32` frame math, `wrapping_sub`; time base = `binary_frame` (committed late). Deleting the already-hashed `unload_timer`/`deposit_cooldown_ticks` **changes the hash layout** → re-baseline once as an *intended layout change* (not a behavior change). **If any acceptance test shows a 1-tick shift, that is the real Risk-2 drift** — re-baseline it as a documented intentional change, do NOT paper over it. Jitter RNG count/range/order unchanged.

**Dependencies.** Slice 1 (`MissionTimer`). Does NOT depend on MissionCom/Contacts/radio. **Sequence after Slice 4** — Edit 3.3's unload-cluster fold sits in `miner_dock_sequence.rs` which Slice 4 also edits (avoid the merge collision; matches design §8 order 4→5).

---

### Slice 6 — Verb API + dispatch adoption (fold teardown, busy-predicates, order_intent resume, retaliation)

**Goal.** Replace the ~9 manual per-command teardown blocks (`world_commands.rs`) with one `assign_mission_with_teardown()`, and the scattered "is busy/idle" predicates with `get_current_mission()`/`is_busy()`. Fold the `order_intent` resume side-channel into the `suspended` stack (`override_mission`/`restore_mission`); re-express retaliation as a mission-gated transition. Closes §7 #1/#2/#3/#4 (to the extent bit-identical allows — see residue). **Does NOT collapse tick phases** (§9 Risk 1) — pre/post-combat split (`world/mod.rs:1943/1947`) and Phase-6 retaliation (`2187/2189`) stay put. **Makes `MissionCom` authoritative for the retask/resume/retaliation paths only**, renaming the Slice-2 field `mission_com → mission` as part of the authority flip.

Consumes **V1 ReadyToCommence**: `assign_mission` force-promotes (no gate); only `queue_mission(commence=true)` consults the per-`EntityCategory` `ready_to_commence()` hook. None of the Slice-6 retasking commands use commence-now, so the hook is added + unit-tested here but exercised live only by the dock slices' `Queue_Mission(0x10)`.

**Files.**
- NEW `src/sim/mission/verb.rs` — the six pure verbs over `&mut MissionCom` (`get_current_mission`, `is_busy`, `assign_mission`, `queue_mission`, `commence_queued`, `override_mission`, `restore_mission`) + the two hardcoded interrupt guards (`is_transition_blocked`: Selling blocks all; Deliberate blocks override/queue→Guard) + `ready_to_commence(ReadySnapshot)` per-category hook + `ReadyCategory`/`ReadySnapshot`. Body verbatim from P6 §2a.
- NEW `src/sim/mission/retask.rs` — `Simulation::assign_mission_with_teardown(id, mission, DockTeardown)` and `assign_mission_keep_fields(id, mission, DockTeardown)`. The `DockTeardown { All, Depot, AircraftOnly, IdleOnly, None }` enum is **load-bearing** (the 9 old sites cancelled different reservation subsets — a single fixed teardown is NOT bit-identical). Body adapted from P6 §2b with the `DockTeardown` parameter wired through (see consistency note §E.3).
- EDIT `src/sim/world/world_commands.rs` — the 9 sites (Move 144–155, Stop 288–297, Attack 329–334, ForceAttack 354–357, ForceAttackCell 375–379, AttackMove 405–408/473–480, RepairAtDepot 777–786, EnterTransport 871–874, PlantC4 1018–1026, CaptureBuilding 1114–1119), each routed through the helper with the correct `DockTeardown` variant (Move=`All`, Stop=`Depot`, Attack=`AircraftOnly`/the keep-fields variant, ForceAttack/ForceAttackCell=`IdleOnly`, RepairAtDepot/EnterTransport/PlantC4/CaptureBuilding=`None` keeping their inline field set/clear). Edit shapes verbatim from P6 §3.1.
- EDIT `src/sim/world/world_orders.rs` — `tick_order_intents_pre_combat` (51): keep `order_intent.is_some()` selector (it carries the AttackMove/Guard *coords* `MissionType` can't encode); documented "retired in spirit, unchanged in code" (full retirement at Slice 8). `tick_order_intents_post_combat` (91–112): doc/comment change only — resume coords stay on `OrderIntent`.
- EDIT `src/sim/combat/combat_targeting.rs` — `tick_retaliation` (346): the gate stays the bit-identical `entity.order_intent.is_some()` suppression (because `is_busy(Guard)==false` would make Guarding units retaliate — a proven DRIFT). The retirement is *architectural/documented*; the literal predicate is unchanged. Encoded with the `get_current_mission` read as the conceptual gate per the residue note. Edit shape verbatim from P6 §3.3.
- EDIT `src/sim/components.rs` — `OrderIntent` (487–495): **no deletion**; doc note that it now carries only goal/anchor coords + Unloading flag (busy-signalling role answered by `MissionCom`).
- EDIT `src/sim/game_entity.rs` — rename the Slice-2 `mission_com` field → `mission` and remove `#[serde(skip)]` so it round-trips (Slice 6 is the first reader-as-authority). Keep the Slice-2 shadow refresh + assert running for all OTHER paths (Slice 8 drops them).

**Acceptance.** `cargo test -p vera20k slice6` (the pure verb tests: assign forces/clears/resets-timer; Selling blocks all; Deliberate blocks only Guard target; override-with-queued discards-current-saves-queued; override-without-queued saves-current + restore; ready_to_commence base always-true, Unit not-ready-while-driving; get_current falls back to queued). Bodies verbatim from P6 §4. Integration: `move_command_retasks_via_mission_substrate_and_clears_state`; `retaliation_still_suppressed_for_guarding_unit` (the DRIFT tripwire). Full regression `cargo test -p vera20k -- world_commands combat_targeting world_orders`. **Hard gate:** `cargo test -p vera20k replay_hash_stable_through_slice6` — retask + guard→attack→resume + retaliation must produce the **same end-of-match `state_hash()`** as the pre-slice baseline (Slice 6 is behavior-preserving; legacy `Option<T>` fields still authoritative, `MissionCom` written in parallel via the verbs). If the hash moves, the `DockTeardown` parameterization or the retaliation gate diverged.

**Determinism.** Verbs are pure functions of `(MissionCom, arg, now=binary_frame)` — no clock/RNG/float. `MissionTimer` frame-anchored. Iteration unchanged (retaliation walks `live_order`; commands walk `keys_sorted()`). `MissionCom` already serde + folded once authoritative; `DockTeardown` is a stack-local control value, never persisted.

**Dependencies.** Slices 0/1/2. First *reader-as-authority* of `MissionCom`. V1 hook consumed but not live-exercised here.

**RESIDUE / DRIFT (surface, do not treat as resolved):**
1. `OrderIntent` **not retired** in Slice 6 — carries AttackMove goal / Guard anchor / Unloading, which `MissionCom` has no field for. Full #3 retirement needs a goal field on `MissionCom`/NavCom — deferred to Slice 8.
2. Retaliation #4 retirement is **partial** — literal predicate stays `order_intent.is_some()`; the clean `is_busy`-only gate is a Slice-8 follow-up (encoding `is_busy` directly now is a proven DRIFT — Guard begins retaliating).
3. `DockTeardown` parameterization is the highest-risk detail — the replay-hash test exists to catch a wrong subset.
4. `ready_to_commence` excluded-mission set / byte-flag semantics carry V1-UNCHECKED residue; reconcile the mission-ID table-vs-enum off-by-one against §B.2 before any live commence path relies on the gate.

---

### Slice 7 — Per-idiom radio adoption (airfield, tank bunker, service depot, war-factory exit)

**Goal.** Migrate the four remaining dock idioms onto the bus, each as a *distinct flow* (design §3.3 "do not conflate"). Four independently-shippable sub-slices. All line anchors re-read this session per P5.

**7a — Airfield/helipad** (contact-slot = pad-index, `CachedDock` 0x0F revalidation, NO FIFO). Closes §7 #9 airfield half. Per **V3**: remove `AirfieldDocks.queues` (`aircraft_dock.rs:113`); `try_reserve` becomes admission-only (no enqueue); `release` frees-only (drop the FIFO `pop_front`/promote-into-freed-pad); add the **nearest-contender distance gate** in `WaitForDock` (523–567) so the distance-then-deterministic-order winner matches `Find_Docking_Bay 0x004DF040`. **Delete** `airfield_docks_release_pad_1_promotes_into_pad_1` (746–762) — it locks the wrong FIFO-pin behavior; replace with `airfield_release_does_not_pin_freed_pad_index` + `airfield_full_returns_negatory_no_queue`. Consumes **V2**: an airstrike-summoned aircraft is radio-deaf (`airstrike_owner.is_some()` && mission ∈ the 5 flight missions); 7a's `CanEnter(0x0F)` revalidation respects that gate (a deaf aircraft drops the 0x0F). The latch is written by a future `sim/airstrike` service, read by 7a. Edit points + tests verbatim from P5 §7a.

**7b — Tank bunker** (`+0x2E4` reciprocal two-sided link + three teardown helpers). Names a home for the §9.2 bunker UNCHECKED item; closes part of §7 #8. Add `bunker_host: Option<u64>` on `game_entity.rs:417` (reciprocal of the existing `bunker_occupant`, init `None` at 572). NEW `src/sim/docking/bunker_link.rs`: `install_bunker_link` (two-sided write), `break_bunker_link` (core, clears both + BREAK), `release_normal`/`release_sell_destroy`/`release_super_damage` (the three teardown reasons), `break_links_on_despawn`. Wire `break_links_on_despawn` into `world/mod.rs:955` (after `clear_radio_contacts_for`, in `uninit` 940–976). Route the install command through `install_bunker_link`, undeploy through `release_normal`. Edit points + tests verbatim from P5 §7b.

**7c — Service depot** (`0x1C REPAIR_TICK` money/heal trichotomy). Closes §7 #6 for the depot timer. Re-express the `Servicing` arm (`building_dock.rs:209-258`) as `repair_tick(...) -> RepairResponse` returning `Roger`/`InsufficientFunds`/`RepairComplete`, preserving all money/heal math + grace logic byte-identical (`cost_per_step = max(1, total*repair_step/max_hp)`, `NO_FUNDS_GRACE_TICKS=30`). The `service_timer: u32 → MissionTimer` re-home is **coupled to Slice 5** (ship the trichotomy first in 7c; finalize the timer in Slice 5's `building_dock.rs` edit — already covered there). Edit points + tests verbatim from P5 §7c.

**7d — War-factory exit** (transient reciprocal contact gating `Can_Enter_Cell`). Closes §7 #8 factory-exit half. Slot-bound the contact: `production_spawn.rs:213` `mark_live_contact_with` → `radio_contacts.insert` on the building's `Contacts`; `movement_occupancy.rs:326` `has_live_contact_with` → `radio_contacts.contains(building_id)` (the sole load-bearing `Can_Enter_Cell` reader — must stay bit-identical). Make it transient (BREAK once the vehicle clears the footprint) — or keep the despawn-time clear as the documented interim if the cell-crossing BREAK is out of 7d scope. **Note (P5):** `Queue_Mission(0x10)` is a verb arg (RESERVE_DOCK), NOT a radio message — do not model 0x10 as a radio code. Edit points + tests verbatim from P5 §7d.

**Acceptance.** `cargo test -p vera20k airfield_` (full→Negatory no-queue; release doesn't pin freed pad; distance gate integration), `bunker_link` (reciprocal break both ways; despawn clears back-link), `depot_repair` (trichotomy + grace reset), `war_factory` + `exit_contact` (footprint passability bit-identical; capacity bounded). Bodies verbatim from P5.

**Determinism.** 7a: `BTreeMap` slots/`aircraft_to_pad`, integer Chebyshev `cell_distance`, no `queues` field (hash shrinks → re-baseline). 7b: `Option<u64>` back-links, O(1) teardown (no scan). 7c: integer money/HP, frame-anchored timer. 7d: `Contacts` slot membership, integer footprint cell math.

**Dependencies.** 7a/7d depend on **Slice 3** (`Contacts` slot model); 7c-Edit1 (timer) on **Slice 1/5**; all read `MissionCom` (Slice 6) before any selector drop. 7c-trichotomy/7b/7a-core are shippable before Slice 3 with interim shims. Despawn broadcast-BREAK (§5.2.8) is the shared safety net for 7b + 7d.

---

### Slice 8 — Make `MissionCom` authoritative + global parity harness

**Goal.** Close out the migration: (1) **hash `MissionCom`** into `state_hash()` (it was `#[serde(skip)]`/unhashed shadow through 2–7); (2) **drop the shadow agreement asserts** (Slice 2); (3) **retire the redundant `Option<T>` mission *selector* storage** — the parts of `aircraft_mission`/`order_intent`/`dock_state`/`deploy_state`/`building_gate.mission_state`/miner FSM that merely encode "which mission." Genuinely-richer **substate** (attack sub_state, reload_timer, pad_index, cargo, drop_cooldown) **stays**. Closes §7 #1–#5 to final form; finishes #6/#7. **Behavior-neutral by contract** — the state hash must not change except by the documented addition of `MissionCom` bytes (re-baseline once). If dropping a selector changes any observable cadence, that selector was NOT redundant — STOP and keep it.

> **CORRECTION (applied at Slice 8 implementation, 2026-06-03).** Two V5
> assumptions in this section were STALE against current code and were corrected:
> (1) `order_intent` is **NOT** a pure selector — it is load-bearing substate and
> is **KEPT** (Slice 8 deletes no fields); (2) the `dock_reservations`
> `hash_production` fold is **NOT** dead — `RefineryDockContacts` is a live
> transitional mirror still written by the production miner-dock path, so its
> hashing is **retained**. Net: the MissionCom fold is the SOLE hash change in
> Slice 8. The bullets below are kept for provenance; see the corrected map row
> and ledger.

**Files.** Verbatim from P6 §3 + the new tests:
- EDIT `src/sim/world/world_hash.rs` — add the `MissionCom` fold block (explicit `hash_mission_com` helper) at the end of the per-entity loop: `current as u16`, `Option` queued/suspended as `0u8`/`1u8`+`u16`, `substate`, `timer.start_frame`, `timer.duration`, `tick_counter`. ~~Delete the refinery-registry hashing in `hash_production()`~~ **CORRECTED: retain it** — `dock_reservations.{contacts,contact_entered,on_pad}` is a live transitional mirror (still written by `miner_dock_sequence.rs`/`miner_system.rs`), not dead; deleting it would change the hash mid-dock and blind the desync detector. Mirror retirement is a later slice.
- EDIT `src/sim/snapshot.rs` — bump `SNAPSHOT_VERSION` (line 22) to Slice-7's value +1 (re-read at impl time); comment the MissionCom-authoritative layout change.
- EDIT `src/sim/replay.rs` — no format edit; add the re-baseline guard comment (pre-Slice-8 replays carry pre-MissionCom hashes and will mismatch — header.version bump is the gate).
- EDIT `src/sim/game_entity.rs` — ~~delete `order_intent`~~ **CORRECTED: `order_intent` is KEPT** (load-bearing substate, not a selector — see corrected map row). The `mission` field rename + un-skip already happened in Slice 6. Delete the Slice-2 shadow asserts (`debug_assert_mission_shadow_consistent` + call site). Keep `last_attacker_id`. No fields are deleted in Slice 8.
- NEW `src/sim/world/mission_authoritative_tests.rs` — `mission_current_changes_state_hash`, `mission_timer_and_substate_change_state_hash` (+ a `mission_queued_and_suspended_change_state_hash` for the Option fold). **No** `no_standalone_order_intent_selector_remains` tripwire (order_intent is retained). Wire `#[cfg(test)] mod mission_authoritative_tests;` in `world/mod.rs`.
- NEW `src/sim/world/global_parity_harness_tests.rs` — the GLOBAL harness (below).

**Selector-retirement map (V5 — what's redundant vs richer substate):**

| Field | Verdict | Action |
|---|---|---|
| `navigation: NavigationState` | KEEP (NavCom is a separate primitive) | unchanged |
| `radio_contacts` | already `Contacts` (Slice 3) | hashed via `hash_fold` |
| `last_attacker_id` | KEEP data, retire convention (#4) | field stays; coordinated-by-convention logic deleted |
| `miner: Option<Miner>` | KEEP slimmed | `Miner.state` selector defers to `MissionCom`; cargo/refinery/ore stay |
| `order_intent: Option<OrderIntent>` | **KEEP (corrected — NOT a pure selector)** | retained. Load-bearing substate: sole store of AttackMove goal / Guard anchor resume coords (`world_orders.rs` post-combat resume), the `Unloading` transport-unload flag (`passenger.rs`), and the retaliation gate `is_busy` provably cannot replace (a Guarding unit has `mission.current == None`, so `is_busy` would let it retaliate — proven DRIFT, `combat_targeting.rs:346`). V5's "pure selector" verdict was stale. |
| `dock_state: Option<DockState>` | KEEP slimmed | Enter selector defers; `service_timer`/`no_funds_grace` substate stays |
| `aircraft_mission: Option<AircraftMission>` | KEEP slimmed | variant selector → `MissionCom.current`; rich payload stays |
| `building_gate: Option<BuildingGateRuntime>` | KEEP slimmed | `mission_state` selector defers; `MissionTimer`s stay |
| `deploy_state` / `deploy_timer` | KEEP | phase markers + timer stay (Slice 5 already split them) |

Per-category real-handler sets (V5, for the dispatch authority — informs which selectors are non-redundant): Vehicles {Sleep, Attack, Move, Retreat, Guard/Sticky, Enter, Capture/Sabotage, Harvest, AreaGuard, Hunt, Unload, Repair, Patrol, Rescue, Eaten}; Infantry same minus Repair/Patrol-override plus Harvest(0x524E70); Aircraft {Attack, Move, Retreat(=QMove-named handler at +0x230), Guard, AreaGuard, Hunt, Unload, Enter, Patrol, Paradrop A/O, Spyplane A/O}; Buildings {Attack, Capture/Sabotage, Guard, AreaGuard, Harvest, Unload, Construction, Selling, Repair, Missile, Open}. **Carry the V5 DRIFT correction:** Aircraft Sleep(0) = base stub; `Mission_QMove 0x00415A50` is at +0x230 (Retreat); QMove(3) routes to the Sleep slot for all classes.

**GLOBAL parity harness** (the project-wide lockstep regression guard). `src/sim/world/global_parity_harness_tests.rs` — body verbatim from P6 §4c: records a deterministic multi-faction skirmish (`HARNESS_SEED=0xC0FFEE_1234`, `HARNESS_TICKS=600`) as a `ReplayLog` via the same `ReplayLog`/`ReplayHeader`/`ReplayRunner::run` path the live game uses (`replay.rs:36–92`), seeds ≥2 houses with a refinery+miner (exercises Slice 4), airfield+aircraft (Slice 7a), war factory (Slice 3 exit-contact), and move/attack commands at known ticks (Slice 6). `global_skirmish_replay_is_deterministic_and_baseline_stable` asserts (1) every tick's live hash == recorded hash (intra-run determinism), (2) final hash == recorded == `GLOBAL_HARNESS_FINAL_HASH` (committed baseline). **The baseline constant is the re-baseline anchor:** unchanged through Slices 0–3, edited exactly once per behavior-bearing Slice 4–8 *in the same commit with a one-line documented reason*.

**Acceptance.** `cargo test -p vera20k mission_authoritative`, `cargo test -p vera20k round_trip_preserves_state_hash` (after version bump), `cargo test -p vera20k global_skirmish_replay_is_deterministic_and_baseline_stable`.

**Determinism.** `MissionCom` hashed inside the BTreeMap-ordered `hash_entities` loop (inherits stable order). `MissionType as u16` discriminant fold (cross-platform stable, matching the file's `category as u8` idiom); `Option<MissionType>` as presence-tag + discriminant. No float added to the hash pre-image. Harness uses seeded RNG + scripted commands; `sim/` never reads the clock. Folding `MissionCom` is the *single* intended hash change — the "Slices 0–3 must not change baseline" harness assertion is the tripwire for any accidental earlier behavior leak.

**Dependencies.** Terminal slice — in-edges from 6 and 7 (transitively all). Consumes `MissionCom`/`MissionType`/`MissionTimer`/`Contacts` + the verb API + all four radio idioms. **One concern (stated, proceeding):** the `hash_production()` deletion assumes Slice 4 fully removed `dock_reservations`; re-grep `dock_reservations` at impl time and gate the deletion on Slice 4 being merged.

---

## D. Cross-slice dependency graph

```
Slice 0 (enums + MissionTimer + MissionControl + radio vocab)  ── blocks ──▶ all
   ├─▶ Slice 1 (MissionTimer first consumer: gate runtime)
   │       └─▶ Slice 5 (migrate bespoke timers onto MissionTimer)
   ├─▶ Slice 2 (MissionCom shadow mode)
   │       └─▶ Slice 6 (verb API + dispatch adoption; first authority reader)
   │               └─▶ Slice 8
   └─▶ Slice 3 (Contacts slot model)
           └─▶ Slice 4 (RadioBus: refinery idiom)
                   ├─▶ Slice 5 (sequence after 4 — shared miner_dock_sequence.rs)
                   └─▶ Slice 7 (airfield / bunker / depot / war-factory)
                           └─▶ Slice 8
Slice 5 ──▶ Slice 6   (timers on MissionTimer before verb-API retasking)
Slice 6 ──▶ Slice 8   (selector reads route through get_current_mission)
Slice 7 ──▶ Slice 8   (all radio idioms read MissionCom before selector drop)
```

**Linearized critical path:** `0 → {1, 2, 3} → {4, 5} → {6, 7} → 8`. Slice 8 is terminal (no downstream). Independent parallelizable lanes: {1→5} (timer lane) and {3→4→7} (radio lane) can proceed concurrently with {2→6} (mission-state lane) until they converge at 6/7→8.

**Hash-baseline change ledger** (the harness `GLOBAL_HARNESS_FINAL_HASH` re-baseline points):
- Slices 0–3: baseline **unchanged** (0/1/2 add no hashed state; 3 changes the *contacts* fold representation — Slice 3 is the first re-baseline at a behavior-bearing boundary).
- Slice 4: re-baseline (`dock_entered_with` + slot-folded contacts).
- Slice 5: re-baseline (deleted `unload_timer`/`deposit_cooldown_ticks` change the layout — *intended layout change, not behavior*).
- Slice 6: **no re-baseline expected** (behavior-preserving — `replay_hash_stable_through_slice6` asserts equality to the pre-slice baseline).
- Slice 7: re-baseline (7a `queues` field removed shrinks state; 7b adds `bunker_host`).
- Slice 8 (implemented 2026-06-03): re-baseline once — `MissionCom` folded into `state_hash` (the SOLE hash change; `hash_production` registry fold **retained**, not removed — it is live). Two golden baselines set this slice: `SLICE6_BASELINE_HASH = 11204055998814135587` (re-baselined: entities now carry default mission bytes) and the new `GLOBAL_HARNESS_FINAL_HASH = 669004916847079430` (initial harness commit). Slice 8 deletes no fields; `order_intent` retained.

---

## E. Consistency audit (cross-draft reconciliation log)

Every place the 7 drafts used the target API inconsistently or disagreed on a line anchor, with the canonical resolution. **No draft content was dropped** — divergences were merged into the canonical shapes in §B.

**E.1 — `MissionCom` field name (`mission_com` vs `mission`).** P1 (Slice 2) names the shadow field `mission_com`; P6 (Slice 6) names it `mission`. **Canonical:** the field is added as `mission_com` (shadow) in Slice 2, and **renamed to `mission` in Slice 6** as part of the authority flip (Slice 6's edits explicitly perform the rename + remove `#[serde(skip)]`). All Slice-2 code (`refresh_mission_shadow`, `debug_assert_mission_shadow_consistent`, `derived_mission` writes) references `mission_com`; all Slice-6+ code references `mission`. Recorded so the coder does a single deliberate rename at the 2→6 boundary, not two competing fields.

**E.2 — GameEntity contact field (`radio_contacts: Contacts` vs add parallel `radio: Contacts`).** P1 (Slice 3) swaps the *type* of `radio_contacts` `Vec→Contacts` (rename-free). P2/P4 (Slice 4) *add* a new `radio: Contacts` beside the live `Vec`. **Canonical:** P1's type-swap is the target; the field stays `radio_contacts`. P4's bus code (`target.radio.set_capacity/contains/insert_first_free`) is reconciled by `s/target.radio/target.radio_contacts/`. P2/P4's "add a parallel field" is honored **only as the interim fallback** if Slice 4 must land before Slice 3 in a session — then the parallel `radio` field is created and folded back into `radio_contacts` at the Slice-3 merge. Target end-state = single `radio_contacts: Contacts`.

**E.3 — Teardown helper signature (`assign_mission_with_teardown(id, mission)` vs the `DockTeardown` requirement).** P6 §2b first wrote a fixed-teardown helper, then its own note proved the 9 sites cancel *different* reservation subsets and a fixed set is NOT bit-identical, mandating a `DockTeardown { All, Depot, AircraftOnly, IdleOnly, None }` parameter. **Canonical:** the helper signature is `assign_mission_with_teardown(id, mission, DockTeardown)` and `assign_mission_keep_fields(id, mission, DockTeardown)` — the `DockTeardown` arg is REQUIRED (P6's own §7 residue #3 flags it as the highest-risk detail; the replay-hash gate catches a wrong subset). P6's draft body is adopted with the `DockTeardown` parameter wired through to choose which of `cancel_depot_dock`/`cancel_aircraft_dock`/`release_docked_idle` run, plus the per-mission legacy-field-clear gating (Plan adopts P6's option B: keep-fields variant for Attack/Capture/PlantC4; full-clear variant for Move/Stop, with `capture_target` cleared only for Stop).

**E.4 — `MissionTimer` method names (`arm` vs `defer`, `reset`, private vs public fields).** P0 `defer` + public fields; P3/P5 `arm` + private fields + `clear`/`is_armed`; P6 `reset`. **Canonical (§B.1):** public `start_frame`/`duration` fields (gate adopter needs access); `defer` is the primary, `arm` and `reset(now)=defer(now,0)` are aliases so every draft's call sites compile. `clear`/`is_armed`/`elapsed`/`remaining`/`armed()` all included. No draft's call site needs editing.

**E.5 — `MissionType` discriminant + variant set (`repr(u8)` vs `repr(u16)`, `None`, `Wait` vs `Deliberate`, `AttackMove`).** P0 `repr(u8)`, 32 variants, `Wait`(28), `AttackMove`(29), no `None`. P1 `repr(u8)`, adds `None=0xFF`, `Deliberate`(28), drops `AttackMove`. P6 needs `repr(u16)` for the hash cast. **Canonical (§B.2):** `repr(u16)` (P6's hash requirement; ids 0–31 unchanged), `None=0xFF` + `Default` (P1), index 28 = `Deliberate` with a "Wait" doc-note (merges both names — one variant), **`AttackMove=29` kept** (P0 — `world_commands.rs` AttackMove needs a representable selector; dispatcher skips it per parity), `Ambush=14` inert (V4), `Rescue=21` real-but-AI-only (V4), `PartialOrd,Ord` added (P0's `BTreeMap` key requirement). P1's dropped `AttackMove` is the only outright re-add; everything else is a strict union.

**E.6 — `Contacts` method names (`insert`/`insert_first_free`, `remove`/`break_with`).** P1 `insert -> Option<usize>`/`remove -> Option<usize>`; P2 `insert_first_free -> bool`/`break_with -> bool`. **Canonical (§B.3):** `insert -> Option<usize>` (richer) is primary; `insert_first_free(id) -> bool` and `break_with(id) -> bool` are kept as thin wrappers so P2/P4 bus code compiles. `set_capacity` (grow-only, P2) + `with_capacity` (P1) both present. `hash_fold` (P2) is the canonical hash entry point; `slot(i)` (P1) supports it.

**E.7 — `now` time base (`binary_frame` vs `tick`).** Six drafts use `sim.binary_frame`; P6 used `self.tick as u32`. **Canonical (§B.5): `sim.binary_frame` everywhere.** P6's verb call sites are corrected to pass `self.binary_frame`. Mixing the two would reintroduce the Risk-2 drift the migration removes — this is a correctness reconciliation, not a style choice.

**E.8 — Line-anchor agreements.** The drafts independently re-read overlapping files this session and agree on every shared anchor: `game_entity.rs` `radio_contacts`@240 / `bunker_occupant`@417 / `new` init@516 (P1/P2/P4/P5); `world/mod.rs` `uninit`@940–976, `clear_radio_contacts_for`@955, advance_tick tail@2309–2313 (P1/P5); `world_hash.rs` radio_contacts fold@482–485 (P1/P2/P4/P6); `building_dock.rs` 40–54/209–258 (P3/P5); `aircraft_dock.rs` 107–193/523–567/746–762 (P3/P5); `miner_dock_sequence.rs` 84–170/772–1176 (P3/P4). **No anchor disagreements found.** Two anchors are version-relative and must be re-read at impl time (flagged in-slice): `snapshot.rs` `SNAPSHOT_VERSION` (P6 — depends on Slice-7's value) and the `sim/mod.rs` module-declaration insertion point (P1/P4 both say "re-read before inserting"). The `dock_reservations` `hash_production` deletion (Slice 8) is gated on Slice 4 having removed the registry — re-grep at impl time.

**E.9 — Aircraft QMove DRIFT (V5 vs design §3.1).** Design §3.1 claims AircraftClass overrides the +0x204 (Sleep) slot with a QMove handler. V5 proved this WRONG (`0x00415A50` is at +0x230/Retreat; Aircraft Sleep is the base stub). P0 flagged it for the dispatch slice; Slice 6/8 carry the correction. Canonical: do not preserve a +0x204 override quirk that does not exist; QMove(3) routes to the Sleep slot for all classes (base stub on Aircraft).
