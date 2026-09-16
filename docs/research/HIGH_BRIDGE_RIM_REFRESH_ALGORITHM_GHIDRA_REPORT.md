# High Bridge Rim Refresh Algorithm - Ghidra Research Report

## 2026-09-11 live high-body publication evidence

The production adapter covers the `576BA0` high-body branch, its perpendicular
state frames, and full `47E040` publication. A fresh read-only critic
independently decompressed active-retail `xbayopigs.map`: `(112,140)` and
`(112,144)` have overlay25/state9; `(111,140)`, `(110,140)`, `(113,140)` have
overlay255/state9. The retained production-loader export gives anchor flags
`0x11380` and nonanchor `(111,140)` flags `0x11300`, pointing to `(112,140)`.
Native `587180` excludes these from direct overlay bands `4A..63`/`CD..E6`, then
resolves structural/self-or-`+2C` and dispatches anchor overlay18/19 to `576BA0`.
Thus stock anchor and nonanchor damage reach this body branch. Its first
effective invocation moves state9 to15; its next invocation collapses it.

The [body publication corpus](../../tools/spatial_oracle/bridge_body_publication.py)
executes the original high-body branch and complete setter for seven cases,
using [preserved stock inputs](../../tools/spatial_oracle/bridge_rim_stock_inputs.json).
Perpendicular, fallout, radar, rim and zone bodies are explicit synchronous
sinks. Executable coverage is **states9/15, direction6/set0 only**. It records
17 native writes per ordinary collapse, including only one overlay clear at
the canonical anchor. Nonanchor damage requests rim cleanup at `(111,140)` but
zone work at `(112,140)`. Synthetic callbacks prove that changing the anchor's
state after branch selection does not cancel collapse, that later slot flags
are freshly read, and that moving F1's retained coordinate changes the next
receiver allocation from `(110,140)` to `(109,140)`. These injected writes are
control tests, not claims of stock callback reachability.

Fresh instruction evidence supplements that corpus: state/axis is captured at
`005776E4..005776FA`; the selected branch dispatches at `00577704/0057770A`.
NS reloads the retained anchor coordinate at `0057776D/0057777B` before B;
EW does so at `00577849/00577857`. NS setter/canonical stores/rim are
`00577790/00577795/0057779F/005777A6`; EW uses
`005778AC/005778B1/005778BB/005778C2`. Zone invalidation is `005778CE`, with
conditional connectivity rebuild at `005778D9`. The current executable
coordinate mutation covers setter F1, not this body anchor reload.

The full setter resolves each next cell after the prior fallout/radar, using
the retained cell's current coordinate. Reachable perpendicular collapse frames
also need live continuation: `572440` executes recursion, setter, canonical
clear and radar before freshly reading tile at `5724F7`; its independent final
tile arm recurses again, rereads subtile at `572550`, then runs three immediate
fallouts before flood-fill. An outcome replay after pre-mutation cannot preserve
these observations. Existing `DynamicTerrainCellState` already captures full
facts and restores before retained flags; reuse it, preserving literal `+2C`
separately from derived self relations. Canonical overlay stores must not use
the convenience writer that automatically queues an extra Recalc.

All seven cases and the recovered rim corpus independently regenerated exactly;
the fresh evidence review found no harness defect in the disclosed slice.
The Rust core compares callback boundaries and final cell fields against those
seven original executions. NS/intact/partial/dummy setter execution and the
actual perpendicular/fallout/rim/zone bodies are not certified by this corpus.

`world/bridge_publication.rs` now runs the core from the ordinary damage entry.
Each literal scalar write publishes full retained fields and serialized terrain
state before synchronous fallout. A recursive explosion therefore sees current
flags, state and literal `+2C` allocation identity. An already-cleared structural
input rejects instead of falling back through old topology. Existing high/head
writers still publish encoded state and overlay identity through
`BridgeRuntimeState`; the adapter reads that current authority before map-derived
fallbacks, including the cleared-overlay sentinel. Removing structural flags
also removes the derived deck navigation fields on nonanchor cells that have
no overlay identity, while marker-only F3/extra writes preserve unrelated ramps.

Production regression cases exercise ground C4, canonical-only overlay removal,
the rebuilt navigation map, a reentrant event through a cleared slot, an anchor
erased by a legacy writer, and repeated effective tile-class transitions. The
reentrant callback injection is a control test, not a stock DeathWeapon witness.
Focused/full validation and independent production review are tracked with the
delivery commit. The adapter retains existing effective tile-class/pavement,
rim and zone projections as synchronous callbacks. Literal tile replacement,
complete `56EB80`/`47D2B0`, perpendicular three-cell tile fallout, full rim
migration, head/hut/direct-overlay drivers and the phase-wide audit remain open.
This increment does not close the bridge row.

## 2026-09-11 ground receiver production increment

This increment replaces the bridge ground pass's sorted-ID, anchor-coordinate
HP writes with synchronous direct receivers over live cell membership. It also
visits nonanchor building foundation cells. Native `CellClass::BlowUpBridge`
`47DD70` supplies current Health, distance0, C4, null source/house and
`ignore_defenses=true,arg6=true`. It captures `NextObject` at `47DD96` before
calling the receiver at `47DDAE`. A removed captured successor is still visited;
its cleared next pointer ends the walk. Ground receivers finish before that
cell's deck-drop pass and debris. The surrounding bridge batch/field/rim
sequencing is unchanged and remains open.

The shared forced receiver bypasses both bunker protection arms, matching
`701900`'s branch to `701BF6`. This corrects collapse damage to an occupied stock
Tank Bunker with `Super.PenetratesBunker=yes`. The direct Terrain adapter retains
`71B920`'s Wood/Immune gate before Object `5F5390`; forced Object damage bypasses
verses, falloff, NoDamage and MaxDamage, but not the earlier Health<=0 or
requested-damage0 rejection. Nested effects and Terrain removal use the existing
world transaction and finish before the direct receiver returns.

### Repeated zero-Health receivers

An earlier ground object's DeathWeapon can kill and unlink the captured next
object. `Object5F5390` returns0 at Health0 without repeating its kill callbacks,
but Techno's `70202E..702035` join still selects its fatal tail. Rust now keeps
that receiver entry separate from a fresh HP-to-zero transition. Repeatable
death sounds and DeathWeapon work execute again without granting fresh kill
credit. Concrete class work follows the nested DeathWeapon and reads the
retained object's current state. Building `442230` returns before Techno when
already at Health0; its concrete death helper also skips an object whose Alive
flag was cleared. Infantry Do_Action `51D6F0` preserves progress for the same
action; changing a retired object's action does not restore Logic membership
or cancel pending deletion.

The [receiver corpus](../../tools/spatial_oracle/bridge_zero_health_receiver.py)
executes original Infantry `517FA0`, Foot `4D7330`, Techno `701900` and Object
`5F5390` with stock-Super InfDeath2/forced/null-source arguments. Alive0 and
Alive1 cases reach original Death_Announcement `4D98C0`. One-entry DieSound
cases execute original `Random__Next65C780` and reach sound request `7509E0`:
main-RNG indices change from `[0,103]` to `[1,104]` with one draw and the supplied
sound ID. Original `65C6D0` seeds the fixture with1. Sound playback,
announcement owner/UI effects and the remaining class postlude are excluded.
The [companion corpus](../../tools/spatial_oracle/bridge_zero_health_deathweapon.py)
executes Infantry `517FA0`, Unit `737C90` and Aircraft `4165C0`, both Alive values,
through their zero-Health joins to death-weapon producer `70D690`. It stops at producer entry;
it does not execute its detonation body. Both metadata files disclose supplied
state and substituted virtual results. The ten cases were independently
regenerated without differences against active retail gamemd SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.

Rust production regressions in `world/bridge_ground.rs` exercise a stock
TERROR-to-E1 cascade: TerrorBomb kills the captured GI, whose later direct
Super receiver repeats GIDie while its loss/kill credit remains single. A second
cascade requires the captured Terrorist's repeated TerrorBomb to kill an MTNK.
These use retail damage definitions in minimal fixtures, not a complete native
explosion comparison. Additional checks cover live registration order, unlinked
objects, nonanchor foundations, occupied Tank Bunkers, Terrain gates, retired
Infantry action state and a nested DeathWeapon crater before the caller returns.
The extracted candidate passes `cargo test -p vera20k --lib`: 8,668 passed,
0 failed, 121 ignored. `cargo clippy -p vera20k --lib` exits0 with warnings
(1,147 reported); this is not a zero-warning claim. The fresh independent critic
reviewed the scoped production changes and the corrected recursive-cleanup test
expectations, and independently regenerated all sixteen native cases. These
results do not constitute a phase-wide pass.

### Health input scope and remaining coverage

Native `47DDA8..47DDAE` passes **&object.Health**. The
[Health-input corpus](../../tools/spatial_oracle/bridge_health_alias.py) executes
the original ground loop/Techno receiver for three synthetic immunity cases,
with copied-packet controls. Radiation, PsychicDamage and Poison write zero
through the packet at `701C1C`, `701C4F`, `701C82`: aliased Health becomes0 while
a copied packet leaves Health100, without Object death callbacks. Type/predicate
virtuals are supplied and concrete class wrappers are excluded.

The helper currently copies Health. A fresh independent scope review found no
stock bridge witness requiring a global signed-Health migration: RULESMD selects
C4Warhead=Super (line818), whose section (27093) has InfDeath2, 100% Verses and
none of those immunity flags or Psychedelic. The 184 extracted named retail
maps have no C4Warhead/Super/Psychedelic overrides; three unidentified map entries
and mode coverage are not certified. In the ordinary forced-Super fatal path,
Object writes initial HP through the packet at `5F54C2`, then Health0 at
`5F550D` before callbacks. Techno's later anger read requires the absent source;
its `702016` timer read excludes fatal result4. Forced Infantry skips prone
writes and InfDeath2 skips `518010`; after Foot the pointer register is replaced
at `5180E8`/`518BB2`. Foot, Unit and Building do not consume that packet after
their shared receiver returns. This is bounded ordinary-retail equivalence.

The following remain open for the phase-wide audit:

- Accepted custom C4 immunity/InfDeath9/Psychedelic inputs require receiver-owned
  writes through a Value/TargetHealth input; negative Psychedelic additionally
  needs coherent signed Health and persistence. The saved alias corpus proves
  the current custom-input difference. Unusual callback-driven survival and
  every FirstObject category are not certified by the ordinary-stock argument.
- Mixed membership preserves terrain-first scenario registration. Late Terrain
  insertion has no unified native AddContent order in the existing substrate.
- Wood-capable C4 with destructible SpawnsTiberium Terrain reaches an explosion
  animation at `71BA7B..71BA85` before nested damage at `71BABF`; that animation
  is still absent. Stock Super omits Wood, so ordinary stock trees survive.
- Building BeforeDeathEffects/AfterDeathEffects hooks are still outside the
  newly placed concrete Alive guard. No stock bridge witness clearing an
  initially live Building's Alive during that nested interval was established;
  complete Building postlude coverage is not claimed.
- Ordered bridge setter publication, body/head/hut caller sequencing, rim
  integration and phase-wide reverse audit remain separate unfinished work.

The larger rim-core work is preserved on
`feature/phase3-bridge-rim-publication-20260911`; it is excluded from this ground
receiver PR. None of these changes closes a Phase 3 row.

**Address(es):** `0x00576770` (`MapClass__UpdateAdjacentBridges_High`), `0x00576200` (`MapClass__UpdateBridgeEdgeTiles_High`), `0x0047E040` (`CellClass__SetBridgeDirection_NESW`)
**Investigation Mode:** exhaustive-slice, downgraded to PARTIAL only for runtime-populated tile-table values
**Claimed Scope:** HIGH rim refresh chain after bridge damage/collapse: entry, edge-tile scan, direct per-cell writes, `0x1E` walk cap, and group clear side effects
**Non-Scope:** LOW rim refresh algorithm, ramp-perpendicular helpers, bridge damage RNG, zone rebuild internals, rendering composition, and runtime capture of theater-populated table values
**Confidence:** High for control flow and writes; Medium for symbolic names of runtime tile-class globals
**Active in YR:** Yes. Reached from live high bridge damage/collapse paths and high map-init/destruction callers.

## Target question

Does the high bridge rim refresh chain merely dirty render output, or does it mutate cells, and what exact writes/caps/group clear semantics must Rust mirror?

## Non-goals

- Do not verify LOW except where the alleged `0x00570AE0` address forces an address/name resolution.
- Do not recover runtime-populated tile-set values; this report records the static comparisons and which globals must be captured elsewhere.
- Do not modify Rust, INI, Ghidra labels, or existing research docs.

## Evidence needed to mark COMPLETE

- Static binary evidence for high-chain entry and direct field writes: satisfied.
- Static binary evidence for walk bounds and caller/callee identities: satisfied.
- Runtime capture of `DAT_00AA0E28`, `DAT_00ABC2B4`, `DAT_00AA1130`, `DAT_00ABAD30`, `DAT_00AA1548`, `DAT_00AA0740`, `DAT_00AA1028`, `DAT_00ABC1E8`, `DAT_00AA0E38`, `DAT_00ABC1D0`, `DAT_00AA1540`: deferred to runtime/theater-load capture.

## Stop conditions

Stop at the high rim refresh chain. A LOW decompile was used only to resolve `0x00570AE0`; no LOW parity contract is claimed.

## 1. Overview

`UpdateAdjacentBridges_High @ 0x00576770` is an active high-bridge post-collapse rim refresh selector. It searches around a supplied rim coordinate, picks a bridge walk direction, pattern-matches high bridge edge/ramp tile classes, and calls `UpdateBridgeEdgeTiles_High @ 0x00576200`. The selector itself does not write cell fields; the callee clears dangling bridge stubs and delegates the multi-cell bridge-flag clear to `CellClass::SetBridgeDirection_NESW @ 0x0047E040`.

## 2. Address/name discrepancy

The user-cited `SetBridgeDirection_NESW @ 0x00570AE0` is not `SetBridgeDirection_NESW`. Ghidra decompiles `0x00570AE0` as `MapClass__UpdateBridgeEdgeTiles_Low`, and its only xrefs are from `MapClass__UpdateAdjacentBridges` / `MapClass__UpdateBridgeEdgeTiles_Low` (`0x00571028`, `0x005713BB`, `0x0057142E`). The real high-chain group-clear call is `CALL 0x0047E040` from `0x0057671C`, and Ghidra decompiles `0x0047E040` as `CellClass__SetBridgeDirection_NESW`. Existing docs that cite `CellClass::SetBridgeDirection_NESW @ 0x0047E040` are correct for this high chain; wording that names `0x00570AE0` as `SetBridgeDirection_NESW` is stale/mislabeled.

## 3. Class layout / key offsets

| Offset / global | Meaning in this slice | Evidence |
|---|---|---|
| `Cell+0x24` | packed map coord | `0x00576770`, `0x00576200`, `0x0047E040` |
| `Cell+0x2C` | anchor pointer written by `SetBridgeDirection` | `0x0047E040` |
| `Cell+0x38` | iso tile type index, normalized against high bridge-set base | `0x00576770`, `0x00576200` |
| `Cell+0x44` | overlay byte/index; rim clear writes `0xFFFFFFFF` | `0x00576728`, `0x00576721..0x00576734` |
| `Cell+0x11A` | iso sub-tile slot used for bridge edge/ramp matching | `0x00576A18..0x00576B2A`, `0x00576291..0x00576343` |
| `Cell+0x11B` | level byte used to compute dirty rect Z input | `0x00576378`, `0x00576397` |
| `Cell+0x11E` | bridge damage/state byte; clear writes `0` | `0x00576721`, `0x0047E055`, `0x0047E10D` |
| `Cell+0x140` | flags; `0x80`, `0x100`, `0x400`, `0x800`, `0x1000`, `0x10000` are rewritten/read here | `0x005767CE..0x00576895`, `0x0047E0F0` |
| `Map+0x13C` | per-cell map occupancy/presence table checked before tile pattern matching | `0x005769F4` |
| `g_DirectionOffsets @ 0x0089F688` family | 8-way map-coordinate deltas | all three functions |

## 4. Core logic

### 4.1 `UpdateAdjacentBridges_High @ 0x00576770`

1. Starts from the caller-supplied rim coord and scans 8 directions. It stops on the first neighboring cell whose `Flags & 0x500` is nonzero (`0x100` bridge-structural or `0x400` destroyed marker). If no found cell has `0x100` or `0x400`, it returns.
2. If the matched cell has `0x400` but not `0x100`, it walks through already-destroyed cells in direction `2` or `4` depending on `Flags & 0x800`, then backs up two cells in the opposite direction. The guard returns on the fourth consecutive destroyed cell (`local_1c` becomes `4`).
3. If the matched cell has `0x100` but not `0x80`, it jumps to `*(cell+0x2C)+0x24` (anchor coord). If `0x100` and `0x80` are both set, it uses the matched cell coord directly.
4. It initializes a dirty rectangle from globals `DAT_00ABD470..47C`, derives the forward walk direction as `0` or `6` from `Flags & 0x800`, then walks while the high-bridge diamond bounds and `Map+0x13C` presence table allow it.
5. It normalizes `cell+0x38` as `(tile_index - DAT_00AA0E28) + 1` and dispatches:
   - `(DAT_00ABC2B4 or DAT_00AA1130) && cell+0x11A == 8` -> call edge tiles with direction `2`.
   - `DAT_00ABAD30..DAT_00ABAD30+3 && cell+0x11A == 5` -> call edge tiles with direction `2`.
   - `(DAT_00AA1548 or DAT_00AA0740) && cell+0x11A == 12` -> call edge tiles with direction `4`.
   - `DAT_00AA1028..DAT_00AA1028+3 && cell+0x11A == 7` -> call edge tiles with direction `4`.
6. If the callee returns nonzero and the dirty rectangle differs from the empty sentinel, it calls `TacticalClass__DirtyScreenRect(rect, 0)`.

Per-cell writes in `UpdateAdjacentBridges_High` itself: none. It reads cells, writes only locals, and queues dirty screen work after the callee mutates state.

### 4.2 `UpdateBridgeEdgeTiles_High @ 0x00576200`

Signature shape from decompile: `thiscall MapClass__UpdateBridgeEdgeTiles_High(this, coord*, direction, rect*)`.

1. Reads the start cell and computes `direction & 7`.
2. Walks forward one cell at a time. `local_44` starts at `1`; the loop continues while `local_44 < 0x1E`. A match is accepted only when `local_44 != 0x1E`, so the static cap is the `0x1E` sentinel: candidate distances `1..29` are checked, and reaching `30` returns `0`.
3. For `direction == 2`, it looks for high bridge tile classes `(DAT_00ABC1E8, DAT_00AA0E38, DAT_00ABAD30..+3)` with `cell+0x11A == 4`.
4. For `direction == 4`, it looks for `(DAT_00ABC1D0, DAT_00AA1540, DAT_00AA1028..+3)` with `cell+0x11A == 2`.
5. If no matching edge class is found before the sentinel, it returns `0`.
6. If `rect*` is non-null, it unions a rectangle around the start coord and found coord after `TacticalClass__CoordsToClient2`, padded by `0x40` on position and `0x80` on size. The Z input for each conversion is `(char)(start_cell+0x11B) * DAT_00ABDE88`.
7. It then walks from the start cell toward the found edge for `local_44` steps, tracking `Flags & 0x80` transitions:
   - If a prior clear segment is followed by a bridge cell (`0x80` set while the previous `bVar10` says clear), it records that coord as the last bridge coord.
   - If a prior bridge coord exists and the current cell is clear (`0x80 == 0` and previous state also says clear), it steps one cell back with `(direction - 4) & 7`, calls `CellClass__SetBridgeDirection_NESW(dir_code, 0)`, writes `cell+0x11E = 0`, writes `cell+0x44 = 0xFFFFFFFF`, marks radar dirty, recursively calls `UpdateBridgeEdgeTiles_High(start, original_direction, rect*)`, and returns `1`.
   - If it sees a clear cell before any repair has been emitted, it calls `RepairBridgeSegment(found_edge_coord, next_coord)` once, latching `bVar2 = true`.
8. If the backward/transition pass finishes without clear/recurse, it returns `0`.

### 4.3 Multi-cell group clear through `SetBridgeDirection_NESW @ 0x0047E040`

The high edge-tile clear calls `SetBridgeDirection_NESW` on the backstepped cell with `state=0`. The direction argument is `0` when the original edge direction is `2`, otherwise `6` when the original edge direction is `4` (`0x00576712..0x0057671C`).

For `state=0`, `SetBridgeDirection_NESW` mutates up to six cells in this order:

| Slot | Coord source | Direct writes in `SetBridgeDirection_NESW` | BlowUpBridge? | Radar dirty? |
|---|---|---|---|---|
| 0 anchor | `this` | `+0x11E=0`; `+0x140 = (old & 0xFFFEE07F) | 0x400 | (dir==0 ? 0x800 : 0)` | yes | yes |
| 1 forward | `anchor + dir` when `dir < 8` | `+0x2C=0`; `+0x140 = ((old & 0xFFFEE8FF) | 0x400) & 0xFFFFF7FF | (dir==0 ? 0x800 : 0)`; `+0x11E=0` | yes | yes |
| 2 forward | previous + `dir` | `+0x2C=0`; `+0x140 = ((old & 0xFFFEE8FF) | 0x400) & 0xFFFFF7FF | (dir==0 ? 0x800 : 0)` but without setting the `0x200` intact-bit input used by slot 1; `+0x11E=0` | yes | yes |
| 3 forward | previous + `dir` | `+0x140 = old & 0xFFFFEFFF` | no | no |
| 4 opposite | `anchor + ((dir - 4) & 7)` | `+0x2C=0`; `+0x140 = ((old & 0xFFFFF8FF) | 0x400) & 0xFFFEE7FF | (dir==0 ? 0x800 : 0)`; `+0x11E=0` | yes | yes |
| 5 extra | only when `dir == 6`: slot 4 coord + `DAT_0089F690` (E offset) | `+0x2C=0`; `+0x140 = old & 0xFFFEFFFF` | no | no |

`BlowUpBridge @ 0x0047DD70` does not directly write these bridge cell fields. It damages/limbos objects on the cell, appends the coord to a queue, and may spawn bridge debris animations with RNG if map-editor mode is off.

## 5. INI keys

No INI key controls this rim refresh algorithm directly. Tile-class constants are runtime/theater-loader globals, not parsed in this function body. Standard bridge damage enables the path through live high bridge collapse, but `BridgeStrength` / `DestroyableBridges` are outside this slice.

## 6. Integration points

Verified xrefs to `UpdateAdjacentBridges_High @ 0x00576770`:

- `0x0057702C`, `0x00577065`, `0x0057754F`, `0x0057757B`, `0x005777A6`, `0x005778C2` inside high damage/collapse processing.
- `0x005745B4`, `0x005751D0` from high/related bridge destruction flows.

Verified xrefs to `UpdateBridgeEdgeTiles_High @ 0x00576200`:

- `0x00576AB7`, `0x00576B38` from `UpdateAdjacentBridges_High`.
- `0x00576748` self-recursion after a group clear.

Verified high-chain call to `CellClass__SetBridgeDirection_NESW @ 0x0047E040`:

- `0x0057671C` in `UpdateBridgeEdgeTiles_High`.

`ProcessBridgeDamageStateMachine_High @ 0x00576BA0` directly reaches the high rim refresh after collapse cases: NS collapse calls `SetBridgeDirection_NESW(0,0)`, clears `+0x11E/+0x44`, then `UpdateAdjacentBridges_High`; EW collapse does the same with direction `6`. Active in standard YR bridge damage.

## 7. Current Rust implementation status

Current Rust has a rim-refresh implementation in `src/sim/world/bridge_orchestrator.rs:1070` with a `WALK_LIMIT = 30`, but it is not the verified binary algorithm. It searches for `Bridgehead`/`Destroyed` roles instead of `Flags & 0x500`, walks toward the discovered neighbor direction rather than reproducing the `0x100/0x400/0x80/0x800` direction selection, and clears a simplified stub by setting `overlay_byte=0xFF`, `DamageState::Healthy { variant: 0 }`, `bridge_group_id=None`, and `deck_present=false`. It does not pattern-match `(normalized tile index, +0x11A)`, does not call a group-clear equivalent from the actual backstepped edge cell, and does not reproduce the `SetBridgeDirection_NESW` per-slot flag/anchor writes. `src/sim/bridge_specs.rs:467` has a simplified action-list model for `SetBridgeDirection_NESW`, but it records only `BlowUpBridge` vs `FlagOnly`, not the per-slot field writes needed by this rim refresh.

## 8. Coverage ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| High entry `UpdateAdjacentBridges_High` | verified | decompile `0x00576770`; xrefs listed above | none for control flow |
| High edge writer `UpdateBridgeEdgeTiles_High` | verified | decompile `0x00576200`; assembly `0x0057671C..0x00576748` | runtime tile global values |
| Real `SetBridgeDirection_NESW` target | verified | decompile `0x0047E040`; xref `0x0057671C` | none |
| User-cited `0x00570AE0` | verified-as-not-high-target | decompile `0x00570AE0`; xrefs `0x00571028/0x005713BB/0x0057142E` | no LOW contract claimed |
| Direct cell writes in selector | verified | no `Cell+` writes in `0x00576770` except callee effects | none |
| Direct cell writes in edge clear | verified | `0x00576721`, `0x00576728`, `0x00576734`, recursive call `0x00576748` | none |
| Multi-cell group clear | verified | `0x0047E040` decompile and assembly contexts `0x0047E0F0`, `0x0047E10D`, `0x0047E114` | exact semantic names of all flag bits beyond bridge docs |
| Runtime tile constants | deferred | globals read in decompile | capture after theater/map load |
| Dirty rect math | verified | `0x00576378..0x0057648C` decompile | none for this slice |
| Rust delta | touched-not-exhausted | `bridge_orchestrator.rs:1070`, `bridge_specs.rs:467` | implementer must inspect full surrounding state model before patching |

## 9. Open questions - final state

- `[RESOLVED] OQ1 - Is the high rim refresh active in YR? -> Yes; high damage/collapse calls reach it.` (evidence: `0x005777A6`, `0x005778C2`, xrefs to `0x00576770`)
- `[RESOLVED] OQ2 - Does `UpdateAdjacentBridges_High` write cells? -> No direct cell writes; it selects, calls edge tiles, and dirties screen.` (evidence: decompile `0x00576770`)
- `[RESOLVED] OQ3 - Which function writes the dangling-stub clear? -> `UpdateBridgeEdgeTiles_High`, after the transition scan.` (evidence: `0x0057671C..0x00576748`)
- `[RESOLVED] OQ4 - What is the walk cap? -> Immediate `0x1E`; candidate distances 1..29, sentinel 30 returns 0.` (evidence: `0x00576276..0x0057634D`)
- `[RESOLVED] OQ5 - What resolves `0x00570AE0`? -> LOW edge-tile function, not `SetBridgeDirection_NESW`.` (evidence: decompile `0x00570AE0`, xrefs)
- `[RESOLVED] OQ6 - What is the real `SetBridgeDirection_NESW` address? -> `0x0047E040`.` (evidence: decompile `0x0047E040`; call at `0x0057671C`)
- `[RESOLVED] OQ7 - Does the high body call LOW from the same function body? -> No; high edge tiles calls high self-recursion, not LOW.` (evidence: xrefs/calls in `0x00576200`)
- `[RESOLVED] OQ8 - Which fields are cleared directly after `SetBridgeDirection`? -> Backstepped cell `+0x11E=0`, `+0x44=0xFFFFFFFF`, then radar dirty and high recursion.` (evidence: `0x00576721..0x00576748`)
- `[RESOLVED] OQ9 - Does `BlowUpBridge` add more bridge cell field writes? -> No direct `+0x11E/+0x44/+0x140/+0x2C` writes in decompile; it affects objects, coord queue, debris RNG.` (evidence: decompile `0x0047DD70`)
- `[DEFERRED] OQ10 - What are the actual runtime values of the compared tile globals?` (category: `needs-runtime-debugger`; reason: globals are runtime/theater-populated; next-step-if-pursued: capture after theater load on a stock YR map)
- `[DEFERRED] OQ11 - Are all flag-bit names beyond known bridge bits fully named?` (category: `requires-different-system-context`; reason: this slice proves masks and writes, not the full CellClass flag taxonomy; next-step-if-pursued: verify against `CELLCLASS_0X140` census)

## 10. Implementation handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| High rim refresh first selects an adjacent cell by `Flags & 0x500`, then derives anchor/walk coord from `0x100/0x400/0x80/0x800`, not from roles or BFS groups | `0x00576770` | mismatch | `src/sim/world/bridge_orchestrator.rs:update_adjacent_bridges` | Use gamemd flag/state inputs and direction table ordering for the selector | `high_rim_refresh_selects_first_flags_0x500_neighbor_in_direction_order` | Do not infer from `BridgeCellRole::Bridgehead` or `DamageState::Destroyed` alone |
| Edge scan matches `(normalized Cell+0x38, Cell+0x11A)` with direction-specific sets, then uses `0x1E` sentinel cap | `0x00576200` | missing | `bridge_orchestrator.rs`, terrain/bridge runtime cell fields | Preserve normalized tile index and sub-tile slot; scan only the binary direction-specific sets | `high_edge_tiles_rejects_candidate_at_distance_30_and_accepts_distance_29` | Do not treat `+0x11A` as damage state or deck level |
| Dangling clear calls `SetBridgeDirection_NESW(dir 0 or 6, state 0)` on the backstepped cell, then writes same cell `+0x11E=0`, `+0x44=-1`, marks radar, and recurses | `0x00576712..0x00576748`; `0x0047E040` | simplified/missing | `bridge_orchestrator.rs`, `src/sim/bridge_specs.rs:set_bridge_direction`, bridge runtime cell flags | Apply per-slot `+0x140`, `+0x2C`, `+0x11E`, `BlowUpBridge`, and radar/dirty effects in the exact order | `high_rim_refresh_clears_group_slots_0_1_2_4_and_flag_only_slots_3_5` | Do not blank only the found stub or skip the multi-cell group clear |

## Negative facts / do not do

- Do not cite `0x00570AE0` as `SetBridgeDirection_NESW`; it is the LOW edge-tile walker.
- Do not implement high rim refresh as render-only dirtying; the callee mutates bridge state and overlay.
- Do not use `deck_level >= 4`, bridge roles, or neighbor masks as substitutes for `(normalized tile index, +0x11A)` comparisons.
- Do not collapse the `0x1E` sentinel into an unbounded or BFS walk.
- Do not treat `Cell+0x11A` as the bridge damage byte; `Cell+0x11E` is the bridge state byte in this slice.

## Stale docs / follow-up wording

- Replace any sentence saying "`SetBridgeDirection_NESW @ 0x00570AE0`" with: "`0x00570AE0` is `MapClass__UpdateBridgeEdgeTiles_Low`; the high-chain group clear calls `CellClass__SetBridgeDirection_NESW @ 0x0047E040` from `0x0057671C`."
- Replace any rim-refresh wording that says "`UpdateAdjacentBridges_High` only marks redraw" with: "`UpdateAdjacentBridges_High` itself only selects and dirties, but its callee `UpdateBridgeEdgeTiles_High` mutates cells by invoking `SetBridgeDirection_NESW`, clearing `Cell+0x11E`, clearing `Cell+0x44`, marking radar dirty, and recursing."
- Replace "`+0x11A` bridge damage state" in this context with: "`+0x11A` is the iso sub-tile slot used by rim-refresh tile matching; `+0x11E` is the bridge damage/state byte."

## Remaining uncertainty

The only material uncertainty for this high-chain slice is the real runtime value of theater-populated tile globals. Static decompilation proves which globals are compared and where; a post-theater-load debugger capture is needed to turn them into concrete tile indices for stock theater/map fixtures.

## Sources

- Ghidra decompile: `0x00576770`, `0x00576200`, `0x00570AE0`, `0x0047E040`, `0x0047DD70`, `0x00576BA0`.
- Ghidra xrefs: `0x00576770`, `0x00576200`, `0x00570AE0`, `0x0047E040`.
- Assembly contexts: `0x0057671C`, `0x00576AB7`, `0x00576B38`, `0x00576748`, `0x00571028`, `0x005713BB`, `0x0057142E`, `0x0047E0F0`, `0x0047E10D`, `0x0047E114`.
- Prior docs checked: `docs/research/bridges/06-render-presentation-audio/BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md`, `docs/research/bridges/05-damage-collapse-repair-cabhut/HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md`, [docs/research/bridges/02-cell-state-layering-zones/CELL_0x11A_POLARITY_RECONCILE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELL_0x11A_POLARITY_RECONCILE_GHIDRA_REPORT.md).
- Rust surfaces checked: `src/sim/world/bridge_orchestrator.rs`, `src/sim/bridge_specs.rs`, `src/sim/bridge_state/mod.rs`.

**Status:** PARTIAL - algorithm and writes verified; concrete runtime tile-table values require debugger capture.
