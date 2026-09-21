# Phase 3 Drive/Ship slope-transition design

**Status: proposed for fresh read-only design review.**

## Goal

Implement the verified active-retail Drive/Ship containing-cell slope cache and
three-frame voxel interpolation without changing impact-driven body rocking,
general locomotion, or any excluded locomotor class.

The representative production path is an ordinary stock YR skirmish in which a
voxel-bodied vehicle or ship using its currently active Drive or Ship locomotor
spawns on, idles on, or crosses a ramp. The complete native evidence is
[docs/research/PHASE3_DRIVE_SHIP_SLOPE_TRANSITION_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_DRIVE_SHIP_SLOPE_TRANSITION_LIFECYCLE_GHIDRA_REPORT.md).
That report proves stock activation for 52 Drive and 13 Ship types and leaves no
load-bearing native question open.

## Architecture Context

### Native owner and lifecycle

The state belongs to the complete active Drive or Ship locomotor object, not to
the unit body and not to terrain presentation. Both classes persist the same
defined fields: cached/current slope byte, previous slope byte, signed timer
start frame, duration, and transition total. Construction starts at slope zero
with an inactive timer; successful `FootClass::Unlimbo` immediately snaps both
slopes to the final containing cell. At each eligible `Process` entry, before
any movement or track work, a changed current-cell byte copies current to
previous, installs the sample as current, and starts duration/total `3`.
[doc: [PHASE3_DRIVE_SHIP_SLOPE_TRANSITION_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_DRIVE_SHIP_SLOPE_TRANSITION_LIFECYCLE_GHIDRA_REPORT.md)
§§2–3; GHIDRA `0x004AF540`, `0x0069EC50`, `0x004D7170`, `0x004B0500`,
`0x0069FC10`]

`Draw_Matrix` derives, but never mutates, the phase from the signed global
frame. A change detected at frame `F` renders old at `F`, `1/3` at `F+1`,
`2/3` at `F+2`, and stable new at `F+3`. Expiry does not clear the stored total.
A retarget during interpolation starts from the prior cached target byte, not
from a baked intermediate matrix. No RNG participates.
[doc: same report §3; GHIDRA `0x0046B640`, `0x004B4D70`, `0x004AFF60`,
`0x0069F670`, `0x00755A40`]

### Current Rust owners and flow

- `GameEntity::new_at_frame` in `src/sim/game_entity.rs` initializes
  `rocking: None`. `RockingState` in `src/sim/components.rs` currently combines
  five impact/body-rocking fields with three slope fields. Production spawn does
  not attach it, so production Drive/Ship slope transitions are absent.
- `src/sim/rocking/rocking_system.rs` samples slope for every manually attached
  `RockingState`, decrements a mutable counter on an equal sample, and runs in
  the global Phase 2.5 pass after movement. The same pass also owns the existing
  spring/damper, ship-rocking integration, and wide-amplitude self-destruct
  check; those body-rocking operations must retain their current ordering and
  eligibility.
- `LocomotorState` distinguishes `active_kind()` from `effective_kind()`.
  `LocomotorRuntimePayload` and `StashedLocomotor` in
  `src/sim/movement/locomotion/piggyback.rs` are the existing Rust-native owner
  for class-local state that must travel with an active or suspended complete
  locomotor. Drive and Ship payloads are currently unit variants.
- `World::advance_master_frame` visits each live object in authoritative order,
  runs object AI, then calls `tick_movement_object_with_grids`. Inside
  `tick_movement_with_grids_scoped`, active low-bridge `TubeMovement` can own the
  whole object turn, forced Drive tracks run before ordinary movement, and
  stationary entities still enter the scoped function. This is the smallest
  production seam capable of representing a Drive/Ship `Process` prologue
  without absorbing movement mechanics.
- All successful entity placement converges on
  `Simulation::try_reveal_entity` in `src/sim/world/lifecycle.rs`. Coordinates
  are committed before `mark_entity_put`; a failed mark restores limbo and
  returns before display/LogicVector registration. This is the single correct
  Rust owner for the successful-unlimbo snap.
- `build_unit_instances` in
  `src/app/presentation/instances/units.rs` reads the live immutable simulation
  after a committed frame. It currently derives phases from the mutable
  `transition_ticks_remaining` and otherwise falls back to the terrain cell.
  The transient atlas already keys a numerator with denominator `3`, so its
  ownership remains usable, but the numerator type and rasterizer's lower clamp
  must change to preserve signed native extrapolation.
- `Simulation` is serialized positionally with bincode. Snapshot version `104`
  is strict; `#[serde(default)]` cannot migrate a changed mid-record shape.
  `world_hash.rs` already hashes both the active locomotor payload and a nested
  stashed runtime, but Drive/Ship payloads contain no state. It separately hashes
  every current `RockingState` field.

The older slope reports named in the verified report's stale-doc section are
discovery maps only. In particular, post-movement sampling, a mutable
three-tick countdown, next/head-to-cell sampling, a Process-time immediate
snap, and locomotor byte `+0x62` as slope state are superseded. The two Chrono
Miner system-model syntheses also predate the current active/effective
locomotor and complete-runtime piggyback implementation; current Rust plus the
new exhaustive report are authoritative for this design.

## Impact Analysis

### Files and functions that change

- `src/sim/movement/slope_transition.rs` (new): the deterministic state and
  pure constructor, snap, Process-entry sample, signed remaining-time, and
  render-phase derivation.
- `src/sim/movement/mod.rs`: expose the bounded mechanism internally.
- `src/sim/movement/locomotion/piggyback.rs`: change only
  `LocomotorRuntimePayload::Drive` and `::Ship` to carry the dedicated state;
  construct it with the current binary frame and transfer it unchanged through
  capture/stash/restore. Remove `LocomotorRuntimePayload::Default`; a class-local
  payload may no longer be invented without a kind and construction frame.
- `src/sim/movement/locomotor.rs`: add active-payload accessors and thread the
  construction frame through normal locomotor and piggyback construction.
  Remove `#[serde(default)]` from `runtime_payload`; strict schema 105 requires
  the payload. `active_kind()`, never `effective_kind()`, gates the class half
  of the mechanism.
- Production/test callers of `LocomotorState::from_object_type`,
  `LocomotorRuntimePayload::for_kind`, and `begin_piggyback`, especially
  `src/sim/world/world_spawn.rs`, `src/sim/movement/movement_commands.rs`,
  `src/sim/miner/miner_system.rs`, `src/sim/movement/teleport_movement.rs`,
  `src/sim/world/world_commands.rs`, and direct locomotor fixtures: supply the
  exact current frame. A frame-zero test adapter may remain explicit, but no
  production constructor may silently default the frame.
- `src/sim/movement/movement_tick.rs`: run the active Drive/Ship sample once at
  the eligible Process-entry seam, after entry-active Tube ownership is known
  and before forced-track, pending-arrival, or ordinary movement can change the
  position.
- `src/sim/world/lifecycle.rs`: snap the active Drive/Ship payload immediately
  after a successful `mark_entity_put` and before display/LogicVector exposure.
  Failed and early-rejected reveals do not touch it.
- `src/sim/components.rs`, `src/sim/game_entity.rs`, and
  `src/sim/rocking/rocking_system.rs`: make `RockingState` body-rocking-only and
  remove slope sampling/counter mutation from the Phase 2.5 pass. Keep
  `GameEntity.rocking`, its `None` construction default, impact impulse math,
  spring/damper order, ship-rocking behavior, and self-destruct behavior.
- `src/sim/world/mod.rs`: rename the Phase 2.5 description to body rocking and
  preserve its present place and gate. Do not make creation of slope state
  activate the body-rocking pass.
- `src/app/presentation/instances/units.rs`: read the active Drive/Ship payload,
  snapshot one last-processed binary frame for the build pass, and derive
  `0/3`, `1/3`, `2/3`, or stable without sim mutation. Preserve the existing
  terrain fallback for locomotors outside this mechanism; an active Drive/Ship
  cache is authoritative even when its cached slope is zero.
- `src/render/unit_slope_transition_cache.rs` and `src/render/vxl_raster.rs`:
  make the transient `phase_num` signed and preserve the exact numerator in the
  cache key. Remove the current lower-bound clamp/early return so negative
  native fractions extrapolate through quaternion SLERP; retain the `t >= 1`
  stable destination branch and the equal-slope shortcut. These remain
  read-only presentation caches, not persistence/hash authority.
- `src/sim/world/world_hash.rs`: remove the retired slope bytes from the body
  rocking fold and hash every dedicated field under both active Drive/Ship
  payloads and any stashed Drive/Ship runtime.
- `src/sim/snapshot.rs`: bump the strict internal schema from `104` to `105`
  (or from the then-current value to its next coordinated value if another
  authorized snapshot change lands first) and add the exact reason.
- Existing rocking, locomotor/piggyback, movement, lifecycle, presentation,
  snapshot, and hash tests require targeted updates; no broad subsystem rewrite
  is part of this slice.

### Blast radius and risks

- Drive/Ship payload shape changes are serialized and hashed for most stock
  vehicles, so committed state-hash goldens will move even when a unit remains
  on flat ground. Rebaseline only verified affected goldens and coordinate the
  one snapshot-version owner.
- A top-level entity component would lose native ownership during a piggyback
  swap. Keeping the state in the typed payload ensures active and stashed
  locomotors serialize and hash independently through the existing mechanism.
- Rust commits `session.binary_frame` at the end of the master frame, after the
  Process-entry writer ran. Presentation reads after that commit. If rendering
  used the post-commit value directly, a transition started during frame `F`
  would first appear at `1/3` and skip native phase zero. The design therefore
  snapshots `session.binary_frame.wrapping_sub(1)` as the last processed/display
  frame for this derived render query. State still stores the exact pre-commit
  writer frame `F`.
- Low-bridge `TubeMovement` is not `TunnelLocomotionClass`. Entry-active Tube
  owns the whole unit turn and must bypass the Drive/Ship Process sample;
  a direction-8 Tube armed later in an ordinary Drive Process does not undo the
  sample that already occurred at that Process entry.
- Missing terrain/cell authority exists only in synthetic/headless Rust worlds.
  It must retain the existing cache without inventing slope zero; production
  map bootstrap supplies resolved terrain.

## Chosen Approach

Add a dedicated `SlopeTransitionState` and store it in the Drive and Ship
variants of `LocomotorRuntimePayload`.

This is slightly more source movement than reshaping `RockingState`, but it is
the smallest coherent ownership model. Native state lives in the complete
locomotor and follows save/load and piggyback ownership. Rust already represents
that exact concept with typed payloads and a boxed stashed runtime. The design
therefore follows an established mechanism rather than creating a parallel
entity-level lifecycle. Impact body rocking remains an independent optional
component and cannot accidentally become active merely because a Drive unit
needs slope state.

The recommended state is:

```text
SlopeTransitionState
  previous_slope: u8
  current_slope: u8
  start_frame: i32
  transition_total: u8  // exactly 0 or 3 in valid constructed state
```

`transition_total` is also the duration. The exhaustive writer census proves
that the two native fields are always written together as `0/0` or `3/3`, so
one stored value is formally equivalent. The native timer's unused middle
dword is intentionally absent. All fields are private outside the mechanism;
writers preserve the valid `0|3` invariant.

## Player-Experience Detail Ledger

- **MILESTONE-BLOCKING — recurrent ramp orientation.** Every ramp entry, exit,
  or orientation change by one of 65 stock Drive/Ship types must show old,
  `1/3`, `2/3`, then new tilt. Trigger frequency is recurrent on any ramped map
  and immediately visible on voxel bodies. [doc: report §§1–3; GHIDRA
  `0x004AFF60`, `0x0069F670`]
- **MILESTONE-BLOCKING — Process-entry order.** Sampling uses only the owner's
  current containing cell before movement/track work. A boundary crossed later
  in frame `F` is first discovered at the next eligible Process entry, not by a
  post-movement pass in `F`. Forced Drive tracks are still Drive Process work;
  entry-active low-bridge Tube turns are not. [doc: report §3; GHIDRA
  `0x004B050B..0x004B0576`, `0x0069FC1B..0x0069FC86`; Rust:
  `movement_tick.rs`]
- **MILESTONE-BLOCKING — successful-unlimbo snap.** A Drive/Ship placed on a
  ramp renders the ramp immediately with no flat-to-ramp spawn blend. Failed
  placement cannot mutate the cache. Trigger frequency includes all scenario,
  production, held-production, and later reveal paths. [doc: report §3;
  GHIDRA `0x004D71A9`, `0x004B04D0`, `0x0069FBE0`]
- **MILESTONE-BLOCKING — Foot plus active-class authority.** Eligibility
  requires a normally processed Foot-equivalent Rust category (`Unit`,
  `Infantry`, or `Aircraft`) and `active_kind() == Drive|Ship`, even when
  installed/effective identity is Teleport during a Chrono Miner Drive
  piggyback. A `Structure` with a modded positive speed and Drive payload is not
  a Foot and remains excluded. Conversely, a modded Infantry or Aircraft with
  active Drive/Ship is eligible; stock members of those categories are absent
  only because their active locomotor classes differ. Gating on installed
  identity, voxel art, or `RockingState` would visibly miss ordinary stock
  movement.
  [doc: report §§2,10; Rust: `locomotor.rs::active_kind/effective_kind`]
- **COMPOUNDING — locomotor-owned piggyback state.** Constructor, active, and
  stashed Drive/Ship slope caches must move with their complete locomotor
  runtime. A top-level cache can survive under the wrong active class and later
  restore stale state, affecting subsequent ramp draws and save/load.
  [doc: report §§2–3, persistence; Rust: `locomotion/piggyback.rs`]
- **MILESTONE-BLOCKING — exact no-write/equal behavior.** An equal sample,
  including while stationary, writes nothing: it does not decrement, reset, or
  clear total on expiry. A changed sample during interpolation uses cached
  current as the new previous slope. [doc: report §3; GHIDRA `0x004B0523`,
  `0x0069FC33`]
- **MILESTONE-BLOCKING — global-frame phase and repeated draws.** Timer reads
  are signed/wrapping and render-only. At writer frame `F`, ordinary forward
  phases are `0/3`, `1/3`, `2/3`, stable; two draws in one frame produce
  identical output and no hash mutation. Native signed arithmetic also permits
  negative elapsed/numerator after a persisted timer crosses the signed frame
  domain (for example start `0`, current `u32::MAX` gives `-1/3`); presentation
  must preserve that SLERP extrapolation rather than clamp to the source. The
  `start_frame == -1` sentinel returns the stored duration and therefore yields
  numerator zero, including the real frame collision at `u32::MAX`.
  [doc: report §3; GHIDRA `0x0046B640`, `0x004B4D70`; Rust:
  `World::run_late_region` late frame commit]
- **COMPOUNDING — persistence and hash.** Previous, current, start, total, active
  versus stashed ownership, and payload presence affect future output and must
  round-trip and hash. Load does not resample or snap; the next eligible Process
  performs the normal comparison. Trigger frequency is each player save/load;
  a mismatch can also desynchronize replay/lockstep. [doc: report §3
  persistence; GHIDRA `0x004AF780`, `0x004AF800`, `0x0069EE90`, `0x0069EF10`]
- **COMPOUNDING — raw source byte.** Sim stores the exact current-cell `u8` and
  performs no look-ahead, previous-cell lookup, `min(20)`, or renderer-driven
  rewrite. Existing render handling for slope-table values outside populated
  `0..=16` remains a presentation concern. [doc: report §§2–3]
- **COMPOUNDING — body-rocking independence.** Damage impulse velocities,
  spring/damper integration, ship-rocking mode, saturation/deadband, and
  self-destruct checks remain on the existing optional `RockingState` and its
  current Phase 2.5 schedule. Trigger frequency is each active body-rocking
  update; coupling slope construction to that component would create unrelated
  gameplay and hash changes. [Rust: `rocking/impulse.rs`,
  `rocking/rocking_system.rs`, `rocking/self_destruct.rs`]
- **EXACTIFICATION-RESIDUAL — non-voxel eligible bodies.** Drive/Ship slope
  state still updates and persists, but a SHP/non-voxel body has no voxel
  matrix on which to show the SLERP. No alternate asset or frame is selected.
  Trigger frequency is every slope crossing by such a modded/stock art case;
  player effect for this mechanism is none. [doc: report §§3,9]
- **EXACTIFICATION-RESIDUAL — narrow Tunnel restoration.** Native snaps only
  after the proved ground-layer Tunnel/piggyback restoration inside
  `TechnoClass::Set_Destination`; it is not a generic move-start or piggyback-END
  rule. Rust's subterranean `TunnelLocomotionClass` is intentionally dormant TS
  and the active low-bridge Tube path is a different mechanism, so no ordinary
  stock caller exists. The exact hook location is nevertheless specified below
  so later activation cannot generalize it. [doc: report §§2–3; GHIDRA
  `0x00742BE3`; AGENTS.md TS exclusion]
- **EXACTIFICATION-RESIDUAL — unused native timer dword.** It is indeterminate,
  never read by this lifecycle, and need not be serialized, hashed, or seeded.
  Trigger frequency and player effect are zero. [doc: report §2]
- **MILESTONE-BLOCKING — RNG absence.** Construction, snap, sample, timer, phase
  extraction, and render-state selection consume no `SimRng`; sharing wake,
  dust, or combat-rocking RNG would desynchronize ordinary play. [doc: report
  §3 Native RNG audit]

There are no `UNKNOWN-RISK` ledger items. The native slice is exhausted, and
the current Rust call sites provide a discoverable integration seam for every
active stock behavior.

## Design

### Components

`src/sim/movement/slope_transition.rs` owns the state and named constant
`SLOPE_TRANSITION_FRAMES: u8 = 3`. It provides only these operations:

1. `constructed(binary_frame)`: `previous=current=0`, signed
   `start_frame=binary_frame as i32`, total `0`.
2. `snap(sample, binary_frame)`: `previous=current=sample`, capture the signed
   frame, total `0`.
3. `sample_process_entry(sample, binary_frame)`: on difference only, assign
   `previous=current`, `current=sample`, capture the signed frame, total `3`;
   on equality, return without a write.
4. `remaining(binary_frame) -> i32`: if total is zero, the caller is stable; if
   `start_frame == -1`, return signed total; otherwise compute signed
   `elapsed = (binary_frame as i32).wrapping_sub(start_frame)`. If
   `elapsed < i32::from(total)`, return
   `i32::from(total).wrapping_sub(elapsed)`; otherwise return zero. This
   intentionally permits a value greater than total for negative elapsed and
   preserves 32-bit wrapping.
5. `render_phase(binary_frame)`: return stable when total is zero or native
   remaining is zero; otherwise return source, destination, signed numerator
   `i32::from(total).wrapping_sub(remaining)` and denominator `3`. Ordinary produced
   phases are `0`, `1`, and `2`, while persisted/wrapped valid state may yield
   a negative numerator. Presentation carries that signed numerator unchanged
   into unclamped quaternion SLERP.

`LocomotorRuntimePayload` becomes `Drive(SlopeTransitionState)` and
`Ship(SlopeTransitionState)`. Its existing capture/install/serde path carries
the state without a new pointer, side table, or entity lookup. Its `Default`
implementation and `LocomotorState.runtime_payload`'s serde default are
removed: every constructor and replacement must supply kind plus binary frame,
and current-version deserialization must contain the exact payload. Accessors
on `LocomotorState` match both active kind and payload variant; mismatched
manually constructed/corrupt state is not treated as eligible.

`RockingState` retains only body angles, velocities, and
`is_ship_rocking`. Its `is_neutral` predicate becomes body-only. No slope
constructor creates `RockingState`, and no impact writer touches the new
locomotor payload.

### Interfaces / Contracts

- **Construction contract:** every production creation of a Drive/Ship
  locomotor supplies the current `ScenarioSession::binary_frame`, including a
  fresh piggyback replacement. Constructor start is defined persisted state
  even while total is zero. Test-only constructors state frame zero explicitly.
- **Eligibility contract:** only a normally processed Foot-equivalent category
  (`Unit | Infantry | Aircraft`) whose current active payload matches Drive or
  Ship is readable/writable. Structures remain excluded even if mod data gives
  them speed and a Drive payload. Installed/effective class, voxel art,
  `IsTrain`, and body-rocking presence do not add or remove eligibility after
  that Foot boundary; a modded Infantry/Aircraft with active Drive/Ship is
  included.
- **Terrain contract:** the sim helper receives the exact `ResolvedTerrainCell`
  `slope_type` at the owner's current `(rx, ry)`. It never clamps or asks for a
  destination/head-to cell. Missing terrain/cell retains the cache unchanged
  and may emit a debug diagnostic; it does not synthesize native behavior for
  an invalid Rust world.
- **Body-rocking contract:** Phase 2.5 keeps its existing rules/terrain gate,
  stable-ID iteration, per-axis order, and self-destruct call. Only the slope
  read/write is removed.
- **Presentation contract:** sim exposes immutable state plus pure phase
  derivation; app presentation snapshots one display frame and selects the
  existing stable or transition atlas path. Render and cache code never writes
  sim state.
- **Persistence contract:** all new defined fields serialize as part of active
  or stashed locomotor payload. Restore trusts them, rebuilds only skipped
  caches, and never routes through unlimbo or a terrain resample.
- **RNG contract:** none of these APIs accepts `SimRng` or another random
  source.

The conditional native Tunnel restoration contract is deliberately narrow:
only after an exact `Set_Destination` branch has proved an active
`TunnelLocomotionClass`, ground layer, and a stashed Drive runtime may it restore
that Drive and then call `snap(current_containing_cell_slope, binary_frame)` on
the now-active Drive payload before destination handling continues. Generic
`end_piggyback`, Teleport-to-Drive creation, Stop, ordinary move commands,
low-bridge Tube entry/exit, and every successful Drive/Ship Process sample must
not call this snap. Current Rust has no active-retail owner for the dormant
Tunnel branch, so this slice records and tests the predicate contract without
wiring a speculative caller.

### Data Flow

#### Construction and placement

1. Spawn/type resolution constructs the installed locomotor payload at current
   binary frame. Drive/Ship state is flat/inactive; other variants carry none.
2. The object enters limbo storage. Failed reveal leaves constructor state.
3. After coordinates commit and `mark_entity_put` succeeds,
   `try_reveal_entity` samples the final containing cell and snaps an active
   Drive/Ship payload.
4. Display and LogicVector registration occur afterward. The first render on a
   ramp is stable at that ramp slope.

#### One live object turn

```text
object AI
  -> capture entry-active low-bridge Tube ownership
  -> if not Tube-owned: active Drive/Ship slope sample at Process entry
  -> active Tube leaf OR forced Drive track OR ordinary/pending-arrival movement
  -> air/special locomotor leaves and piggyback restore
  -> post-movement object tail
```

The sample runs even when no movement target exists. A Drive/Ship that crosses
the cell boundary later in this turn still holds the old containing-cell cache
until its next eligible Process entry. If ordinary processing arms low-bridge
Tube during the turn, that does not retroactively suppress the already-run
prologue; an entry-active Tube turn runs no locomotor Process and therefore no
sample.

#### Render time

`World::run_late_region` processes frame `F`, then commits session frame `F+1`.
`build_unit_instances` snapshots
`display_binary_frame = session.binary_frame.wrapping_sub(1)` once and passes it
to every slope query in the build. The derived transition key stores a signed
numerator, and `VxlSlopeBlend` converts `numerator / 3` without a lower clamp so
native negative extrapolation survives. A state started at `F` therefore
selects:

| presentation snapshot | derived numerator | result |
|---:|---:|---|
| `F` | `0` | old/source slope |
| `F+1` | `1` | one-third blend |
| `F+2` | `2` | two-thirds blend |
| `F+3` and later | stable | current/destination slope |

Repeated redraws while paused reuse the same snapshot. A saved simulation
restores the same session frame and payload, so it also reconstructs the same
display frame without a mutable presentation countdown.

For an active Drive/Ship, cached current slope remains authoritative after
expiry, including cached zero. For another locomotor, retain the current
adjacent terrain-derived stable behavior; this slice does not claim or change
the stable-matrix rules of excluded classes.

### Error Handling

- Valid production state has a coherent active kind/payload pair and total
  `0|3`. Keep fields private so normal Rust writers cannot create anything
  else. Debug assertions may flag internal incoherence, but gameplay code does
  not repair it by borrowing terrain or another component.
- There is no payload default. Historically, the slope-payload change advanced
  schema 104 -> 105 so missing payload bytes could not default to Drive state at
  frame zero. The later Cell ground-height correction advanced 105 -> 106; the
  current loader rejects a v105 preamble before body decode, so missing v105
  payload bytes no longer reach deserialization.
- A missing terrain grid or out-of-grid current cell in synthetic fixtures is
  a no-write result. It must not become a slope-zero transition and must not
  make ordinary movement fail.
- Do not clamp raw slope bytes in sim. Existing presentation handling for
  unpopulated native matrix-table slots remains at the app/render boundary.
- No cross-version bincode migration is attempted. Version `104` snapshots are
  rejected cleanly after the bump. Mapping their `RockingState` counter into the
  new payload would promote known-wrong post-movement/countdown data and still
  could not reconstruct active versus stashed native ownership. Current-version
  save/load is exact and performs no load-time snap.

### Testing Strategy

The implementation builder should use focused `--lib` filters only; the phase
owner retains the single full-suite run for Phase 3 closure. Tests must be
production-path discriminating rather than manually attaching `RockingState`.

1. **Drive successful-unlimbo:** spawn a real Drive unit on nonzero slope via
   normal placement. Assert the Drive payload exists without a manual component,
   previous=current=cell byte, total zero, and the first render is stable ramp.
   A failed mark leaves constructor state and produces no exposure.
2. **Ship counterpart:** repeat through a real Ship type and prove identical
   state and phase behavior.
3. **Pre-movement boundary timing:** cross A→B during frame `F`; prove no
   post-movement write in `F`, then prove the next eligible entry starts A→B.
   Separate fixtures show forced Drive track samples before advancing and an
   entry-active low-bridge Tube turn does not sample.
4. **Stationary and stable no-write:** change the authoritative current-cell
   slope without assigning a movement target; the next Drive/Ship Process
   discovers it. Repeated equal samples before and after expiry leave the full
   state byte-identical.
5. **Exact render ledger:** through the production late-frame commit and unit
   instance extraction, assert `0/3`, `1/3`, `2/3`, stable. Extract the same
   frame twice and prove equal output plus unchanged sim hash.
6. **Signed/wrapping timer and extrapolation:** cover a forward transition
   across `i32::MAX` to `i32::MIN`; a transition started at
   `u32::MAX`/signed `-1` proving the sentinel keeps remaining equal to duration;
   and persisted `start=0, current=u32::MAX`, which must return remaining `4`,
   numerator `-1`, key signed phase `-1`, and an unclamped `-1/3` SLERP result
   distinct from the source matrix. No unsigned, saturating, source-clamped, or
   monotonic `session.tick` implementation may pass.
7. **Mid-transition retarget:** A→B followed before expiry by B→C starts B→C at
   numerator zero; no interpolated A/B matrix becomes state.
8. **Eligibility matrix:** Unit Drive and Ship pass; Foot-equivalent Infantry
   and Aircraft with modded active Drive/Ship also pass; a Structure with a
   Drive payload fails. Stock Walk, Hover, Fly, Jumpjet, Rocket, Teleport,
   Tunnel, Mech, and DropPod active classes do not acquire or mutate this state.
   A Drive fixture with `IsTrain=yes` remains eligible. SHP art does not remove
   sim ownership.
9. **Active versus effective piggyback:** a Teleport-installed unit with a fresh
   active Drive payload samples as Drive. Capture/stash/restore round-trips the
   payload with no leakage to the inactive class. Generic restore and move
   issuance do not snap. A focused synthetic predicate test pins that only a
   ground Tunnel→stashed-Drive restoration is eligible for the documented
   immediate snap; low-bridge Tube and Teleport cases are rejected.
10. **Save/load and hash:** save at `1/3`, restore at the same session frame,
    and assert payload, derived render phase, and state-hash continuity before
    any Process. Then run an equal sample and prove start/total are unchanged.
    Independently mutate each field in active Drive, active Ship, and stashed
    Drive payloads and require hash divergence.
11. **No load resample:** save a cache whose current byte differs from live
    terrain, restore it unchanged, then prove only the next eligible Process
    restarts the transition.
12. **No RNG:** compare scenario RNG state across construction, unlimbo snap,
    equal sample, changed sample, phase extraction, and repeated render reads.
13. **Impact body-rocking regression:** retain the existing impulse,
    spring/damper, ship-rocking, self-destruct, serde, and hash discrimination
    tests with body-only `RockingState`. Prove slope construction leaves
    `entity.rocking` unchanged and body rocking never writes the locomotor
    payload.
14. **Schema boundary:** the slope change historically advanced snapshots
    `104 -> 105`; the later Cell ground-height correction advanced `105 -> 106`.
    Current snapshots report version `106`, and the loader rejects v105 (as well
    as older) preambles before body decode. The round trip includes both an
    active and a stashed Drive/Ship slope payload. A nonzero-frame ordinary
    constructor and fresh piggyback replacement must retain that frame before
    reveal/sampling; no default/omitted payload path may create frame zero.

## Architectural Decisions

- **Follow typed locomotor payload ownership.** This is the existing Rust-native
  equivalent of native class-local complete-object state. It preserves the
  behavior contract without recreating COM objects, vtables, or raw persistence.
- **Keep body rocking separate.** The current optional component and Phase 2.5
  pass remain the owner of impact behavior. This corrects an existing hidden
  coupling instead of widening it.
- **Place sampling at the scoped movement entry, not in a new global pass.** The
  seam is already called once for each live object after object AI and can see
  Tube ownership before any position writer. It represents the required
  Process prologue while leaving track/path systems in their current modules.
- **Place snapping in lifecycle authority.** Central successful reveal covers
  map, production, held, and later real unlimbo paths and naturally excludes
  failed attempts. Spawn call-site duplication would eventually miss a path.
- **Derive presentation phase from deterministic state.** No countdown or
  render-owned cache becomes authority. The one-frame display snapshot adapts
  Rust's late frame commit without falsifying the stored writer frame.
- **Strict snapshot boundary, no migration.** The old schema stores a known-wrong
  mechanism in the wrong owner; rejecting it is safer and more honest than an
  approximate conversion.
- **No new architectural blocker.** The only conditional native integration
  without a stock Rust caller is the documented dormant Tunnel restoration.
  It is an evidence-backed inactive exclusion, not an unknown mechanism and not
  a reason to approximate generic piggyback behavior.

Adversarial self-review supports this choice: a common stock ramp loop would
still look wrong if sampling remained after movement, if phase zero were lost
to Rust's late frame commit, or if unlimbo blended from flat. A future
piggyback/save implementation would require expensive rework if the state lived
on `GameEntity` or `RockingState`. Typed payload ownership closes all three
risks now while leaving movement, impact rocking, and rendering boundaries
intact.

## Alternatives Considered

### 1. Dedicated top-level `GameEntity::slope_transition`

This cleanly separates slope state from body rocking and is a smaller enum
change. It is rejected because the state would belong to the unit rather than
the complete active locomotor. A Drive payload stashed under another locomotor
would leave its cache live at entity scope, and restore could not recover two
independent active/stashed caches. Extra swap callbacks could emulate that, but
they would duplicate the ownership transfer already implemented by
`LocomotorRuntimePayload` and make save/hash correctness depend on every caller.

### 2. Reshape shared `RockingState`

Replace its mutable remaining counter with start/total, attach it to all
Drive/Ship entities, move its slope writer to Process entry, and leave body
angles alongside it. This is the research report's smallest textual patch, but
it is rejected architecturally. `RockingState` is optional body-impact state;
attaching it to make production slopes work also activates the body-rocking
pass and self-destruct owner. Conversely, an entity may need impact rocking
without an eligible locomotor. The component still would not travel with a
stashed complete locomotor. Independent gates can suppress immediate symptoms,
but the state owner remains wrong and creates persistent hidden coupling.

### 3. Presentation-only terrain interpolation

Track prior/current terrain cells or visual slope in app/render state and blend
there. This is rejected outright: native Process timing, stationary discovery,
mid-transition retarget, piggyback ownership, save/load continuity, and hash
state are deterministic simulation behavior. A presentation cache cannot
reconstruct them and would violate the `sim/` authority boundary.

## Explicit Exclusions

- General Drive/Ship pathfinding, speed, track selection/geometry, wake, dust,
  and movement-side effects.
- Impact impulse formulas, body-angle rendering, EMP/naval continuous rocking,
  and the existing body-rocking residuals. They are preserved, not exactified
  in this slice.
- Terrain/TMP slope production, LAT smoothing, passability, cliff, bridge, and
  height mechanics. This slice consumes the resolved current-cell byte only.
- Any slope-transition lifecycle for active Fly, Jumpjet, Rocket, Hover, Walk,
  Teleport, Tunnel, Mech, or DropPod locomotors, and any non-Foot Structure.
  Infantry and Aircraft are absent in stock because of their active classes,
  not categorically excluded if a mod gives a Foot object active Drive/Ship.
  Stable matrix behavior outside active Drive/Ship is not generalized.
- Treating low-bridge `TubeMovement` as `TunnelLocomotionClass`, or wiring the
  narrow `0x00742BE3` snap to generic `Set_Destination`, Stop, END, move start,
  Tube exit, or Teleport restoration.
- The dormant bulk helper `LocomotionClass::ForEach_SetSlopeIndex @ 0x004E1570`.
- Native raw-object padding/unused timer middle dword, COM/vtable/pointer
  persistence, or allocator behavior.
- Alternate assets, palettes, anchors, z-order, or full voxel body/turret
  composition. The existing slope transition cache consumes the derived matrix
  phase.
- Load-time terrain resampling, snapshot migration from the wrong countdown,
  RNG consumption, or a new global scheduler pass.

## Decision

Recommend the dedicated Drive/Ship locomotor-payload state. Proceed only after
a fresh read-only design critic confirms the ownership, Tube/forced-track
ordering, last-processed render frame, strict snapshot boundary, body-rocking
preservation, and dormant Tunnel exclusion. Any generic reset, post-movement
writer, mutable render countdown, load-time resample, or un-hashed defined state
keeps the mechanism open.
