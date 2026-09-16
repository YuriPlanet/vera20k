# Bridge Collapse Fallout Ordering - Ghidra Research Report

**Address(es):** `0x0047DD70`, `0x005F4160`, `0x004D3780`, `0x00575BA0`, `0x00575870`, `0x00575540`, `0x00575220`, `0x00576BA0`, `0x00575EE0`, `0x006E53A0`, `0x006D3D10`
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** ordering of bridge-collapse fallout after a bridge cell or bridge sweep reaches collapse: `CellClass::BlowUpBridge`, `CollapseBridge_*`, ground/deck occupants, debris/animation/sound timing, zone/radar/full-redraw invalidation, and trigger event `0x1F` separation.
**Non-Scope:** re-proving C4/CABHUT and weapon/AoE entry gates; low-bridge TubeClass internals beyond the shared collapse tail; complete trigger action execution; complete shroud renderer internals.
**Confidence:** High for the ordering claims below; Medium for exact shroud-edge timing because this slot verified the draw-pass consumer but did not re-decompile the shroud-edge helper itself.
**Active in YR:** Yes. These paths are reached by standard YR bridge collapse; `DestroyableBridges=yes` is stock in `ini/rulesmd.ini`.

## Working Notes

Target question: What exact order does gamemd use for bridge-collapse fallout after `BlowUpBridge`/`CollapseBridge_*`, and what must Rust avoid doing out of order?

Non-goals: Do not re-prove entry gates; do not study full bridge pathfinding or low-bridge TubeClass lifecycle; do not implement Rust.

Evidence needed to mark COMPLETE: decompile plus assembly context for `BlowUpBridge` ground/deck loops; decompile plus assembly context for `DropIn` relayer order; decompile of high/low `CollapseBridge_*` tails; decompile/assembly evidence for event `0x1F`; current Rust scan for cascade order and no-op trigger/audio surfaces.

Stop conditions: stop once ground kill vs deck drop-in order, anim spawn timing, zone/radar/full-redraw side effects, event `0x1F` separation, Rust deltas, and do-not-do notes are covered with no open ordering questions inside this slice.

## Bounded list-ownership update (2026-09-06)

Read-only inspection of the active retail `/gamemd.exe` confirms that
`0047DD96` saves the ground successor before ReceiveDamage, and
`0047DDBA..0047DDC9` reads the deck head after ground callbacks and saves each
successor before DropIn. Native mobile insertion prepends, so a deck list
`[newer, older]` becomes ground `[older, newer]`. This order can affect an
already-running Iron Curtain walk, which reads its current receiver's successor
**after** the nested collapse returns.

Rust now selects the live Bridge list and invokes one world-owned represented
relayer. It removes the complete foundation footprint from Bridge, preserves the
existing ground snap and movement reset, records one new serialized entry order,
reconciles the derived vehicle occupation projection, and re-adds the footprint
to Ground. Full snapshot restoration uses those entry orders to reproduce the
same list. The actual hut dispatcher and an Iron Curtain command with a nested
DeathWeapon exercise both production callers. Unmarked coordinate twins are
excluded; raw bytes, serialized Drive reservations and building smudges are
preserved. This is bounded list authority and restoration coverage, not complete
DropIn or falling parity.

The new native inspection also corrects overly broad callback assumptions:
CellClass AddContent `0047E8A0` and RemoveContent `0047EA90` explicitly skip their
raw occupation virtual calls for Infantry (`WhatAmI == 0xF`). DropIn `005F4160`
sets the falling bytes, removes/re-adds membership around clearing OnBridge,
then has a locomotor-dependent occupation clear. It does not itself snap XYZ
or reset the Rust movement state. Reusing the full Rust Mark/Unmark transaction
would add Infantry callbacks, hard-removal reservation disposal, building smudge
removal and air-spatial policies that are not established for this call.

Remaining **DRIFT**: exact falling/landing progression and original-Z callback
inputs, concrete raw occupation writes and the later occupation clear, hidden
occupation entry for supported building profiles, and the ground ReceiveDamage
health-alias transaction. The latter distinguishes immunity writes to aliased HP
from Object kill callbacks and Techno fatal admission; copying current HP into an
ordinary damage event does not establish equivalence for authored C4 warheads.
These remain required follow-up mechanisms in the broader ownership goal.

## 1. Overview

`CellClass::BlowUpBridge` is the per-cell fallout primitive. Its order is: kill ground-list occupants, drop bridge-list occupants via `ObjectClass::DropIn`, enqueue the collapsed cell in a global cell queue, then maybe spawn metallic debris and one bridge-explosion anim.

The bounded `CollapseBridge_*_{High,Low}` walkers have a separate animation path: for each of up to four axial steps, they spawn three `BridgeExplosions` anims first, then call `DestroyBridge_{High,Low}` up to three times, then advance. Their tail always calls `UpdateBridgeZonesHelper()` and sets `g_Tactical+0xD7C = 1`.

Trigger event `0x1F` is not part of `BlowUpBridge`, not audio, and not bridge mutation. It is a cell-tag trigger broadcast from `RepairBridgeSegment`/endpoint paths and must stay distinct from the separate event `0x18` destroyed registry path.

## 2. Key Offsets / Fields

| Owner | Offset / value | Meaning | Evidence | Active in YR |
|---|---:|---|---|---|
| `CellClass` | `+0xE4` | ground object list walked first by `BlowUpBridge` | `0x0047DD84..0x0047DDAE` | Yes |
| `CellClass` | `+0xE8` | bridge/deck object list walked second by `BlowUpBridge` | `0x0047DDBA..0x0047DDC9` | Yes |
| `ObjectClass` | `+0x30` | next-object link snapshotted before kill/drop calls | `0x0047DD96`, `0x0047DDC6` | Yes |
| `ObjectClass` | `+0x8C` | `OnBridge` list selector cleared by `DropIn` after removal | `0x005F418F`; add/remove readers `0x005684B1`, `0x005688E1` | Yes |
| `ObjectClass` | `+0x8D`, `+0x8F` | falling/bomb bytes set by `DropIn` before relayering | `0x005F416A`, `0x005F4171` | Yes |
| `RulesClass` | `+0xFA8` | `C4Warhead` used for ground-list force kill | `0x0047DD8E..0x0047DDAE` | Yes |
| `RulesClass` | `+0x15C`, `+0x168` | `BridgeExplosions` vector/count | `0x0047E020`, `0x00575Dxx` decompile | Yes |
| `RulesClass` | `+0x140`, `+0x14C` | `MetallicDebris` vector/count for `BlowUpBridge` optional anim | `0x0047DFAE`, `0x0047DF7C` | Yes |
| `CellClass` | `+0x3C` | attached cell tag gate before event `0x1F` | `0x00575EE0`; assembly push sites `0x00575F95..0x005761DE` | Conditional |
| `TacticalClass` | `+0xD7C` | full-terrain/full-redraw flag set by collapse tails, cleared by draw | writer `0x00575B91`; reader/clearer `0x006D3D10` | Yes |

## 3. Core Ordering

### 3.1 `CellClass::BlowUpBridge @ 0x0047DD70`

Verified order:

1. Early-out if map editor flag is set.
2. Walk `CellClass+0xE4` ground list. For each object:
   - snapshot `object+0x30` before mutation;
   - call vtable `+0x16C` with `&Object.Health` (`+0x6C`), distance `0`, `RulesClass+C4Warhead`, null attacker, `ignoreDefenses=1`, `arg6=1`, and null source House. This is an aliased in/out damage pointer, not damage zero.
3. Walk `CellClass+0xE8` deck list. For each object:
   - snapshot `object+0x30` before mutation;
   - call vtable `+0xEC`, which is `ObjectClass::DropIn` for normal objects.
4. Append the cell coord to a global collapsed-cell queue.
5. If `BridgeExplosions.ActiveCount > 0` and the outer 95% RNG gate passes, compute a jittered coord, maybe spawn one `MetallicDebris` anim, then spawn one delayed `BridgeExplosions` anim.

Evidence: decompile `0x0047DD70`; assembly ground loop `0x0047DD84..0x0047DDAE`, deck loop `0x0047DDBA..0x0047DDC9`, anim calls `0x0047DFBA` and `0x0047E02C`.

Active in YR: Yes. The callers are live bridge-collapse paths and the stock rules define non-empty `BridgeExplosions`.

Important detail: deck occupants are not killed by the ground pass because the ground pass walks only `+0xE4`; deck occupants are not removed before `DropIn` because `BlowUpBridge` begins the second pass from `+0xE8` and directly calls vtable `+0xEC`.

### 3.2 `ObjectClass::DropIn @ 0x005F4160`

Verified order:

1. Set falling byte `+0x8D = 1`.
2. Set bomb/falling damage byte `+0x8F = 1`.
3. Call vtable `+0x124` with arg `0`.
4. Remove from display layer.
5. Clear `ObjectClass+0x8C OnBridge = 0`.
6. Submit to display layer.
7. Call vtable `+0x124` with arg `1`.

For normal Techno objects, vtable `+0x124` is `TechnoClass::MarkCellLists @ 0x004D3780`. After MarkAndNotify succeeds and virtual `+0x78` reports display layer 2, mode `0` calls `TechnoClass__ExitCell_RemoveFromMultiCells @ 0x005687F0`; modes `1/3` call `TechnoClass__EnterCell_AddToMultiCells @ 0x005683C0`. The enter/exit helpers read `ObjectClass+0x8C` immediately before `CellClass::RemoveContent`/`AddContent`, so removal observes `OnBridge==1` and insertion observes `OnBridge==0`.

Evidence: decompile `0x005F4160`, `0x004D3780`, `0x005683C0` (enter/add), `0x005687F0` (exit/remove); assembly `0x005F416A..0x005F41A1`, `0x005684B1`, `0x005688E1`.

Active in YR: Yes for ordinary bridge-deck units/infantry; conditional only for exotic non-Techno objects that might occupy the bridge list.

### 3.3 `CollapseBridge_*_{High,Low}` walkers

The four walkers share the same observable order:

1. Measure bridge extents backward and forward inside the family overlay band.
2. Choose direction toward the longer side and set a biased start cell.
3. Iterate at most four axial cells.
4. For each iteration, if the center cell is not already the destroyed-anchor sentinel, spawn three `BridgeExplosions` anims at the perpendicular row/column. Each anim consumes jitter RNG, delay `Random(1,5)`, then anim-index RNG.
5. After the three anim spawns, call `DestroyBridge_High` or `DestroyBridge_Low` up to three times until it returns non-zero.
6. Step to the next axial cell and break if that cell leaves the bridge overlay band.
7. Tail: call `MapClass__UpdateBridgeZonesHelper()` and set `g_Tactical+0xD7C = 1`.

Evidence: decompile `0x00575BA0`, `0x00575870`, `0x00575540`, `0x00575220`; assembly retry loop `0x00575E42..0x00575E4F`; tail write `0x00575B91` on the EW/NS twin tail pattern.

Active in YR: Yes. These are the already-verified bounded bridge-collapse walkers.

Ordering consequence: the walker-spawned `BridgeExplosions` animations happen before the per-cell `DestroyBridge_*` call in each axial iteration. `BlowUpBridge`'s optional `MetallicDebris` and one delayed bridge explosion are a later per-cell fallout path called by `DestroyBridge_*`/`SetBridgeDirection`-driven collapse, not the same spawn block.

### 3.4 Damage state-machine collapse tail

`ProcessBridgeDamageStateMachine_High @ 0x00576BA0` has two relevant collapse shapes:

- Body/ramp final-destroy arms call `CellClass::BlowUpBridge` for a three-cell row/column before the overlay/ramp propagation and zone invalidation tail.
- Bridge-direction collapse arms call `UpdateRamp_*`, then `CellClass::SetBridgeDirection_NESW(..., state=0)`, then clear the state byte and overlay index, update adjacent bridges, and call `InvalidateBridgeZones`; if it returns true, `UpdateBridgeZonesHelper` runs.

Evidence: decompile `0x00576BA0`; direct `BlowUpBridge` call context `0x00576DAD`; `SetBridgeDirection` call context `0x0057778A..0x00577795`; invalidation/update context `0x005778CC..0x005778D9`.

Active in YR: Yes.

### 3.5 `SetBridgeDirection_NESW @ 0x0047E040`

When called with `state=0`, this function updates the affected cells' bridge flags/state byte, calls `CellClass::BlowUpBridge` for that cell, then calls `RadarClass::MarkTerrainDirty`. This repeats over the anchor/body/opposite cells selected by the direction.

Evidence: decompile `0x0047E040`; assembly docs from prior relayer report cite `0x0047E114` and `0x0047E1EF` as representative `BlowUpBridge` calls; fresh decompile shows `BlowUpBridge` before each `RadarClass__MarkTerrainDirty`.

Active in YR: Yes.

## 4. INI Keys

| Key | File / section | Stock YR value | Binary reader/use | Effect | Active in YR |
|---|---|---|---|---|---|
| `BridgeExplosions=` | `ini/rulesmd.ini` `[General]` | `TWLT026,TWLT036,TWLT050,TWLT070` | parsed by `RulesClass::ReadGeneral`; used at `Rules+0x15C/+0x168` | animation pool for bridge explosion anims | Yes |
| `MetallicDebris=` | `ini/rulesmd.ini` `[General]` | `DBRIS1LG..DBRS10SM` | parsed by `RulesClass::ReadGeneral`; used at `Rules+0x140/+0x14C` | optional metallic debris pool in `BlowUpBridge` | Yes |
| `RepairBridgeSound=` | `ini/rulesmd.ini` `[AudioVisual]` | `BridgeRepaired` | repair path, not `BlowUpBridge` | repair sound only | Yes, but not collapse |
| `DestroyableBridges=` | `ini/rulesmd.ini` `[General]` | `yes` | bridge damage entry gate in sibling reports | enables bridge collapse damage paths | Yes |

## 5. Integration Points

### Occupants

Ground-list occupants receive forced aliased-HP damage before the deck pass. Deck occupants receive `DropIn` and are relayered into the ground list after `OnBridge` is cleared; this establishes call ordering, not universal survival through later falling or nested effects.

Active in YR: Yes. Evidence: `0x0047DD84..0x0047DDC9`, `0x005F4160`.

### Debris / anims / sounds

`BlowUpBridge` has no direct sound call. It constructs anims; `AnimClass::Constructor @ 0x00421EA0` calls `AnimClass::Middle @ 0x00424CE0` immediately only when delay is zero, and `AnimClass::Middle` plays the anim type's sound field through `VocClass__PlayAt` if `AnimType+0x2F8 != -1`.

`BlowUpBridge` creates optional `MetallicDebris` with delay `0` and `BridgeExplosions` with delay `1..5`. The `CollapseBridge_*` walkers also create `BridgeExplosions` with delay `1..5`.

Active in YR: Yes. Evidence: `0x0047DFBA`, `0x0047E02C`, `0x00421EA0`, `0x00424CE0`; standard `BridgeExplosions=` anims have `Report=` in `artmd.ini` per [BRIDGE_COLLAPSE_SOUND_SOURCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_COLLAPSE_SOUND_SOURCE_GHIDRA_REPORT.md).

### Zone/path invalidation

There are two verified collapse invalidation modes:

- `CollapseBridge_*` tail always calls `UpdateBridgeZonesHelper()` and sets `g_Tactical+0xD7C=1`.
- `ProcessBridgeDamageStateMachine_*` final-destroy paths call `InvalidateBridgeZones`; only if it returns true do they call `UpdateBridgeZonesHelper`.

Rust-facing meaning: path/zone rebuild belongs after the collapse mutation side effects, but the binary does not use event `0x1F` to perform that rebuild.

Active in YR: Yes. Evidence: `0x00575BA0`, `0x00575870`, `0x00575540`, `0x00575220`, `0x00576BA0`, `0x005778CC..0x005778D9`.

### Radar / full redraw / shroud

`SetBridgeDirection_*` marks radar terrain dirty after the corresponding `BlowUpBridge` call for a state-0 cell. `CollapseBridge_*` tails set `g_Tactical+0xD7C`; `TacticalClass::Draw @ 0x006D3D10` reads that byte near the terrain draw fast-path and clears it near the end of the draw path.

Shroud-edge redraw is not a bridge-collapse trigger event. The fresh `TacticalClass::Draw` decompile shows `Tactical_layer_shroud_edges()` runs in the draw stack. The exact dirty-bit recompute helper was not re-drained in this slot; prior high-bridge docs say it is active in YR independently of TS fog-of-war.

Active in YR: Yes for radar/full-redraw; Yes/Medium confidence for shroud-edge draw integration. Evidence: `0x0047E040`, `0x006D3D10`, prior `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` section 13.7.

### Event `0x1F`

`RepairBridgeSegment @ 0x00575EE0` walks an endpoint span and calls `TechnoClass__ProcessCellAction(0x1F, 0, DAT_00ABD480, 0, 0)` only when the tested cell has `CellClass+0x3C != 0`. `TechnoClass__ProcessCellAction @ 0x006E53A0` is a trigger dispatcher: it checks attached tag/list fields, evaluates trigger conditions, may play trigger-configured voice and queue trigger actions, and returns. It does not mutate bridge overlays, call `BlowUpBridge`, repair zones, or emit built-in bridge audio.

`FUN_0071F680 @ 0x0071F680` proves `0x1F` and `0x18` are not interchangeable: `0x1F` appears in the first category set, while the destroyed-event bit `0x04` is set only for event `8` and `0x18`.

Active in YR: Conditional. The bridge call is live, but trigger effects require authored cell tags/events.

Evidence: decompile `0x00575EE0`, assembly call sites `0x00575F95`, `0x00576007`, `0x0057606C`, `0x005760CC`, `0x00576137`, `0x0057619C`, `0x005761DE`; decompile `0x006E53A0` and `0x0071F680`.

## 6. Historical Rust Implementation Scan

This original scan predates subsequent implementation changes, including the bounded update above. Its deltas are historical observations, not current validation.

Original Rust file scanned: `src/sim/world/bridge_orchestrator.rs`.

Observed cascade order in `apply_bridge_damage_events`:

1. aggregate destroyed and `BlowUpBridge` action cells;
2. kill ground occupants for `blow_up_cells`;
3. drop in deck entities for all `destroyed_set`;
4. spawn debris for all `destroyed_set`;
5. update adjacent bridges;
6. no-op event `31`;
7. refresh zones if dirty.

Rust matches the high-level "kill before DropIn before debris before trigger before zone refresh" shape. The main deltas are:

- Rust kills only `blow_up_cells` but drops/spawns debris for the broader `destroyed_set`; binary `BlowUpBridge` performs kill, DropIn, queue, and its debris block together per `BlowUpBridge` cell.
- Rust `spawn_bridge_debris` uses an approximate RNG gate (`next_range_u32(20) == 0`) and discrete jitter draws; binary uses `RandomRanged(0, 0x7ffffffe) * scale < 0.95` and floating jitter math. This may be an accepted RNG-boundary approximation, but it is not exact binary RNG.
- Rust has `SimSoundEvent::BridgeRepaired` but no generic anim-start `Report=` sound event for bridge explosion visuals. Current `WorldEffect` is visual-only.
- Rust's `notify_bridge_span_collapse` no-ops event `31`, which is acceptable for skirmish but incomplete for campaign/map-trigger support.
- Rust's `drop_in_bridge_deck_entities` uses `occupancy.move_entity(..., MovementLayer::Ground, ...)` after clearing `on_bridge`. Earlier docs noted `OccupancyGrid::remove` is more forgiving than gamemd selected-layer removal; parity-sensitive relayer tests should pin old-layer removal before state mutation.

No Rust was modified by this investigation.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `CellClass::BlowUpBridge` ground kill order | verified | decompile `0x0047DD70`; assembly `0x0047DD84..0x0047DDAE` | none |
| `CellClass::BlowUpBridge` deck `DropIn` order | verified | decompile `0x0047DD70`; assembly `0x0047DDBA..0x0047DDC9` | none |
| `ObjectClass::DropIn` relayer | verified | decompile `0x005F4160`; assembly `0x005F416A..0x005F41A1` | exotic non-Techno deck occupants not exhaustively classified |
| Techno list helpers read `OnBridge` at call time | verified | decompile `0x004D3780`, `0x005683C0` (enter/add), `0x005687F0` (exit/remove); assembly `0x005684B1`, `0x005688E1` | none for normal units |
| `CollapseBridge_*` anim-before-destroy order | verified | decompile `0x00575BA0`, `0x00575870`, `0x00575540`, `0x00575220` | none |
| `CollapseBridge_*` tail zone/full-redraw order | verified | decompile same four walkers; tail write `0x00575B91` pattern | none |
| Damage state-machine zone invalidation tail | verified | decompile `0x00576BA0`; assembly `0x005778CC..0x005778D9` | low-state twin not re-decompiled in this slot |
| `SetBridgeDirection` radar after `BlowUpBridge` | verified | decompile `0x0047E040` | `NWSE` twin not re-decompiled; prior docs say byte-identical |
| `TacticalClass+0xD7C` draw consumer | verified | decompile `0x006D3D10` | none for flag read/clear |
| Shroud-edge helper internals | touched-not-exhausted | `0x006D3D10` calls `Tactical_layer_shroud_edges`; prior high-bridge docs | exact helper timing not re-drained here |
| Event `0x1F` trigger-only path | verified | decompile `0x00575EE0`, `0x006E53A0`; assembly call sites | none for bridge-side hook |
| Event `0x18` separation | verified | decompile `0x0071F680` | full public trigger enum labels out of scope |
| Original Rust cascade scan | historical | `src/sim/world/bridge_orchestrator.rs` scan | exact test coverage not exhaustively audited |

## 8. Open Questions - Final State of the Investigation Log

- `[RESOLVED] OQ-1 - Does `BlowUpBridge` kill deck units? -> No; it force-kills only the ground list, then calls `DropIn` on deck-list objects.` (evidence: `0x0047DD84..0x0047DDC9`)
- `[RESOLVED] OQ-2 - Does `BlowUpBridge` snapshot next before mutating occupants? -> Yes for both ground and deck loops.` (evidence: `0x0047DD96`, `0x0047DDC6`)
- `[RESOLVED] OQ-3 - Does `DropIn` remove before clearing `OnBridge`? -> Yes; vtable `+0x124(0)` precedes `+0x8C=0`.` (evidence: `0x005F4178`, `0x005F418F`)
- `[RESOLVED] OQ-4 - Does `DropIn` re-add after clearing `OnBridge`? -> Yes; vtable `+0x124(1)` follows the clear.` (evidence: `0x005F418F..0x005F41A1`)
- `[RESOLVED] OQ-5 - Do Techno add/remove helpers sample `OnBridge` at call time? -> Yes.` (evidence: `0x005684B1`, `0x005688E1`)
- `[RESOLVED] OQ-6 - Do `CollapseBridge_*` anims spawn before per-cell destruction? -> Yes; the three-anim loop precedes `DestroyBridge_*` retry loop.` (evidence: `0x00575BA0`, `0x00575E42..0x00575E4F`)
- `[RESOLVED] OQ-7 - Does `CollapseBridge_*` always run the zone/full-redraw tail? -> Yes; all four twins call `UpdateBridgeZonesHelper` and set `g_Tactical+0xD7C=1` at tail.` (evidence: `0x00575BA0`, `0x00575870`, `0x00575540`, `0x00575220`)
- `[RESOLVED] OQ-8 - Does `BlowUpBridge` play collapse sound directly? -> No; sounds come from spawned anims' `AnimClass::Middle` path.` (evidence: `0x0047DD70`, `0x00424CE0`)
- `[RESOLVED] OQ-9 - Does event `0x1F` mutate bridge state? -> No; dispatcher evaluates/queues trigger actions only.` (evidence: `0x00575EE0`, `0x006E53A0`)
- `[RESOLVED] OQ-10 - Is event `0x1F` the same as destroyed-registry event `0x18`? -> No.` (evidence: `0x0071F680`)
- `[RESOLVED] OQ-11 - Did the original Rust scan have a direct bridge-collapse sound event? -> No; scan found repair sound only and visual `WorldEffect` bridge explosions.` (evidence: `src/sim/world/mod.rs`, `src/sim/world/bridge_orchestrator.rs`)
- `[RESOLVED] OQ-12 - Did the original Rust scan run event 31? -> No, `notify_bridge_span_collapse` is an intentional no-op.` (evidence: `src/sim/world/bridge_orchestrator.rs`)
- `[DEFERRED] OQ-13 - Exact shroud-edge helper internals after bridge collapse.` (category: `bounded-cost-too-high`; reason: this slot verified draw integration and prior docs cover active shroud-edge behavior, but did not re-drain the helper body; next-step-if-pursued: targeted `/re-investigate Tactical_layer_shroud_edges bridge dirty bits`.)
- `[DEFERRED] OQ-14 - Exotic non-Techno objects in `CellClass+0xE8`.` (category: `out-of-scope`; reason: normal player-visible deck occupants are Techno-derived; next-step-if-pursued: classify all possible `AltObject` occupants and vtable `+0xEC` bindings.)

## 9. Historical Implementation Handoff

Original proposed work is retained for context; it is not a current backlog or validation result. The explicitly dated relayer row reflects this update.

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| `BlowUpBridge` applies ground kill, deck `DropIn`, collapsed-cell queue, and its debris block per `BlowUpBridge` cell, in that order. | `0x0047DD84..0x0047E02C` | partial: Rust splits kill over `blow_up_cells` but DropIn/debris over all `destroyed_set` | `src/sim/world/bridge_orchestrator.rs` | Keep fallout scoped to the cells that actually receive `BlowUpBridge`, unless a sibling state-machine report proves additional destroyed cells should get separate visual-only effects. | Collapse a state-machine bridge segment where `destroyed_cells` includes cells without `SetBridgeDirection::BlowUpBridge`; only `BlowUpBridge` cells force-kill/drop/spawn `BlowUpBridge` debris. Proposed test: `bridge_collapse_fallout_runs_only_on_blowupbridge_cells` | Do not treat every destroyed overlay cell as a `BlowUpBridge` cell. |
| [2026-09-06] Deck list traversal captures next before each selected-layer relayer. | `0x0047DDBA..0x0047DDC9`, `0x005F4160` | Bounded Rust list authority now resides in one world operation; full falling/raw/hidden callbacks remain DRIFT. | `src/sim/world/bridge_orchestrator.rs`, `src/sim/world/lifecycle.rs` | Complete footprint removal/re-entry and fresh serialized order; preserve existing raw and reservation state. | Actual hut and nested IC command tests cover order, unmarked exclusion, non-anchor foundation and snapshot restoration. | Do not substitute full Mark/Unmark or claim complete DropIn parity. |
| `CollapseBridge_*` walker-spawned `BridgeExplosions` occur before `DestroyBridge_*` per axial iteration; `BlowUpBridge` anims are a separate per-cell fallout block. | `0x00575BA0`, `0x00575870`, `0x00575540`, `0x00575220`; `0x0047DFBA`, `0x0047E02C` | partial/unchecked: Rust has one `spawn_bridge_debris` after aggregation | `src/sim/world/bridge_orchestrator.rs`, `src/sim/bridge_state/mod.rs` | If exact visual/RNG parity is pursued, represent walker-spawned three-perpendicular `BridgeExplosions` separately from `BlowUpBridge` metallic/explosion spawns. | Deterministic bounded hut collapse consumes walker anim RNG before each per-cell destruction and consumes `BlowUpBridge` RNG only for cells where `BlowUpBridge` is called. Proposed test: `collapsebridge_spawns_walker_anims_before_destroybridge_rng` | Do not merge all bridge collapse visuals into one post-hoc destroyed-set loop when validating RNG lockstep. |
| Bridge collapse sound is an anim-start `Report`/`StartSound` consequence, not a direct bridge sound and not `RepairBridgeSound`. | `0x00421EA0`, `0x00424CE0`, `0x0047E02C`; `ini/rulesmd.ini:721` | missing: bridge explosion `WorldEffect` has no anim-start sound routing | `src/sim/world/bridge_orchestrator.rs`, app/audio animation sound routing | Emit selected anim's resolved start/report sound when the delayed bridge explosion begins. | A collapse selecting `TWLT036` emits `Explosion06` after the chosen 1-5 frame delay, not immediately. Proposed test: `bridge_explosion_report_sound_plays_when_delayed_anim_starts` | Do not add a hardcoded `BridgeCollapseSound`; do not reuse `BridgeRepaired`. |
| Event `0x1F` is trigger-only, cell-tag-gated, and separate from event `0x18`. | `0x00575EE0`, assembly `0x00575F95..0x005761DE`, `0x006E53A0`, `0x0071F680` | acceptable no-op for skirmish; missing campaign trigger runtime | `src/sim/world/bridge_orchestrator.rs`, `src/sim/trigger_runtime.rs` | Keep no-op for skirmish, but future campaign support should deliver numeric event 31 only to tagged span-footprint cells. | A map with event-31 cell tags queues trigger action on collapse; skirmish without tags has no side effects. Proposed test: `bridge_span_event31_is_trigger_only_and_distinct_from_event18` | Do not use event 31 for damage, audio, zone rebuild, or global bridge destroyed notification. |
| Zone/path rebuild happens through state-machine/collapse tails, not event 31; `g_Tactical+0xD7C` is a draw/full-terrain flag consumed by `TacticalClass::Draw`. | `0x00575BA0` tails, `0x005778CC..0x005778D9`, `0x006D3D10` | mostly present: Rust rebuilds path/zone after cascade when `zones_dirty` | `src/sim/world/bridge_orchestrator.rs`, `src/sim/world/mod.rs` | Keep zone/path rebuild after bridge-state mutation and before next movement; do not couple it to trigger event 31. | After collapse, next movement tick treats the span as disconnected even when trigger runtime is disabled. Proposed test: `bridge_collapse_rebuilds_zones_independent_of_event31` | Do not wait for or depend on trigger actions to update passability. |

### Negative Facts / Do Not Do

- Do not kill bridge-deck units during collapse. Evidence: deck list `+0xE8` calls vtable `+0xEC` (`DropIn`) after the ground kill loop, with no damage call in that deck loop (`0x0047DDBA..0x0047DDC9`).
- Do not clear `OnBridge` before removing a deck unit from the bridge list. Evidence: `DropIn` calls vtable `+0x124(0)` before `MOV [ESI+0x8C],0` (`0x005F4178`, `0x005F418F`).
- Do not play `RepairBridgeSound` or `EVA_BridgeRepaired` on collapse. Evidence: `BlowUpBridge` has no sound call; collapse audio is anim `Report`/`StartSound` via `AnimClass::Middle`.
- Do not merge event `0x1F` with event `0x18`. Evidence: `0x0071F680` gives destroyed bit `0x04` only to event `8` and `0x18`, while `0x1F` is a different category.
- Do not route path/zone rebuild through event `0x1F`. Evidence: zone updates are direct collapse/state-machine tails (`0x00575BA0`, `0x005778CC..0x005778D9`); `0x006E53A0` is trigger dispatch.
- Do not treat all destroyed cells as cells that received `CellClass::BlowUpBridge`. Evidence: `BlowUpBridge` is called from specific `SetBridgeDirection`/state-machine cells; overlay writes can affect wider cells through `DestroyBridge_*`/`ApplyBridgeDestruction`.

### Remaining Uncertainty

- Exact shroud-edge bit recompute timing after collapse remains inherited from prior high-bridge docs, not re-drained here.
- Exotic non-Techno objects on `CellClass+0xE8` were not exhaustively classified.
- Exact binary RNG equivalence for Rust's simplified 95% and jitter draws remains a broader RNG-parity boundary.

### Stale Docs / Follow-up Docs

- `docs/research/HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md`: any wording implying event `0x1F` is a general "BridgeDestroyed" runtime event should be replaced with: "The bridge span-collapse path delivers numeric trigger event `0x1F` to tagged cells only; it is trigger-only and distinct from the separate event `0x18` destroyed registry path."
- `docs/research/BRIDGE_COLLAPSE_CHAIN_MECHANISM_GHIDRA_REPORT.md`: any wording saying `RepairBridgeSegment` has 7 event call sites should be treated as stale for exact count; the decompile has four horizontal and three decompiler-visible vertical explicit calls in current output, while prior event report lists seven assembly push sites. The load-bearing fact is the repeated gated `0x1F` trigger-only calls, not the exact prose count.

## Sources

- Ghidra decompile: `0x0047DD70`, `0x005F4160`, `0x004D3780`, `0x005683C0`, `0x005687F0`, `0x00575BA0`, `0x00575870`, `0x00575540`, `0x00575220`, `0x00576BA0`, `0x0047E040`, `0x00575EE0`, `0x006E53A0`, `0x0071F680`, `0x00421EA0`, `0x00424CE0`, `0x006D3D10`.
- Ghidra assembly context: `0x0047DD84..0x0047DDAE`, `0x0047DDBA..0x0047DDC9`, `0x005F416A..0x005F41A1`, `0x005684B1`, `0x005688E1`, `0x00575E42..0x00575E4F`, `0x0057778A..0x00577795`, `0x005778CC..0x005778D9`, event push sites `0x00575F95`, `0x00576007`, `0x0057606C`, `0x005760CC`, `0x00576137`, `0x0057619C`, `0x005761DE`.
- INI: `ini/rulesmd.ini`, `ini/rules.ini`, `ini/artmd.ini`.
- Existing docs: [BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md), [BRIDGE_COLLAPSE_SOUND_SOURCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_COLLAPSE_SOUND_SOURCE_GHIDRA_REPORT.md), [BRIDGE_DESTROYED_TRIGGER_EVENT_0X1F_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_DESTROYED_TRIGGER_EVENT_0X1F_GHIDRA_REPORT.md), [BRIDGE_DROPIN_ONBRIDGE_RELAYER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_DROPIN_ONBRIDGE_RELAYER_GHIDRA_REPORT.md), `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md`, [BRIDGE_ZONE_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_LIFECYCLE_GHIDRA_REPORT.md).
- Rust scan: `src/sim/world/bridge_orchestrator.rs`, `src/sim/world/mod.rs`, `src/audio/events.rs`.
