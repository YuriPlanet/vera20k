# LightSource Queued Mode Caller Census Ghidra Report

Date: 2026-05-22

Target addresses:

- `0x00554A60`
- `0x00554A80`
- `0x00554AF0`
- `0x00554D50`

## Target Question

Do any active standard Yuri's Revenge paths pass a nonzero queued-mode argument into `0x00554AF0` directly or through its wrappers, and how do the callers split between building lamp lights, radiation lights, and global refresh?

## Non-goals

- Do not re-derive `0x00554AF0` falloff/math; that was covered by the prior cell-compute and dirty-scheduling reports.
- Do not study unrelated map ambience, LightConvert RGB normalization, or spotlight beam rendering.
- Do not mutate Ghidra labels/comments, Rust, INI, existing docs, or [.swarm-claims.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/.swarm-claims.md).

## Evidence Needed To Mark COMPLETE

- Exhaust cross-references to `0x00554A60`, `0x00554A80`, `0x00554AF0`, and `0x00554D50`.
- For every wrapper caller, verify the pushed queued-mode argument at the call site.
- For every direct `0x00554AF0` caller, classify whether it is a wrapper, destructor/remove path, radiation update path, or other refresh path.
- For the radiation update wrapper `0x00554AA0`, exhaust its callers and verify the final queued-mode argument.
- Separate normal tick processing of `0x00554D50` from forced flush calls inside `0x00554AF0`.

## Stop Conditions

- Stop once all xrefs reported by Ghidra for the four target functions have been inspected.
- Stop if any call site passes a symbolic/nonconstant mode that cannot be proven zero from local context; mark PARTIAL and name the caller.
- Stop if a nonzero caller is found; classify it and hand it off directly instead of broadening into the queue implementation.

## Verified Findings

### Wrapper Semantics

- Active in YR: Yes. `0x00554A60` is the enable wrapper. It checks `LightSource+0x48`; if inactive, it sets the byte to `1` and calls `0x00554AF0(mode)`.
- Active in YR: Yes. `0x00554A80` is the disable wrapper. It checks `LightSource+0x48`; if active, it sets the byte to `0` and calls `0x00554AF0(mode)`.
- Active in YR: Yes. `0x00554AA0` is a parameter-update wrapper for an existing `LightSourceClass`. If changed fields are detected and the source is active, it calls `0x00554AF0(mode)`.
- Active in YR: Yes. `0x00554AF0` supports nonzero `mode`: if `mode != 0` and pending queue count is nonzero, it first calls `0x00554D50` with forced-flush semantics, then appends 0x14-byte pending records instead of immediately recomputing cells.
- Active in YR: Yes. `0x00554AF0` with `mode == 0` recomputes affected cells immediately through `0x00483E30`.

### Xrefs To `0x00554A60` Enable Wrapper

All active callers pass literal zero.

- Active in YR: Yes. `BuildingClass__ReadFromINI` at `0x0044FC8B` calls `0x00554A60` after `PUSH 0x0`.
- Active in YR: Yes. `BuildingClass__GoOnline` at `0x004522C3` calls `0x00554A60` after `PUSH 0x0`.
- Active in YR: Yes. `RadSiteClass__Activate` at `0x0065B7CA` calls `0x00554A60` after `PUSH 0x0` when a new radiation LightSource is constructed.
- Active in YR: Yes. `BuildingClass__RestoreOnlineEffects` at `0x00452428` calls `0x00554A60` after `PUSH 0x0`.
- Active in YR: Yes. `BuildingClass__Unlimbo` at `0x00440DFD` calls `0x00554A60` after `PUSH 0x0` after creating `BuildingClass+0x614 LightSourceClass*`.
- Active in YR: Yes. `BuildingClass__OnConstructionComplete` at `0x00446767` calls `0x00554A60` after `PUSH 0x0` after creating `BuildingClass+0x614 LightSourceClass*`.
- Active in YR: Yes. `BuildingClass__ChangeOwner` at `0x004484D3` calls `0x00554A60` after `PUSH 0x0` for captured/engineer-enabled building effects.

### Xrefs To `0x00554A80` Disable Wrapper

All active callers pass literal zero or a register already proven zero.

- Active in YR: Yes. `BuildingClass__ApplyOfflineEffects` at `0x00452498` calls `0x00554A80` after `PUSH 0x0`.
- Active in YR: Yes. `BuildingClass__Destructor` at `0x0043BD47` calls `0x00554A80` after `PUSH EBP`; local assembly immediately before this has `XOR EBP, EBP`, so the argument is zero. The path then deletes and clears `BuildingClass+0x614`.
- Active in YR: Yes. `BuildingClass__Sell` at `0x00449F1E` calls `0x00554A80` after `PUSH 0x0` during deploy/sell conversion handling.
- Active in YR: Yes. `BuildingClass__Sell` at `0x0044A20B` calls `0x00554A80` after `PUSH 0x0` during the later sell/removal path.
- Active in YR: Yes. `BuildingClass__ReceiveDamage` at `0x0044264C` calls `0x00554A80` after `PUSH 0x0` on destruction/damage-removal handling.

### Xrefs To `0x00554AF0` Directly

- Active in YR: Yes. `0x00554A70` is the call inside `0x00554A60`; it forwards the caller-supplied mode. Every drained caller to `0x00554A60` supplies zero.
- Active in YR: Yes. `0x00554A90` is the call inside `0x00554A80`; it forwards the caller-supplied mode. Every drained caller to `0x00554A80` supplies zero.
- Active in YR: Yes. `0x00554ADC` is the call inside `0x00554AA0`; it forwards the caller-supplied mode. Every drained caller to `0x00554AA0` supplies zero.
- Active in YR: Yes. `0x005551CE` is a LightSource removal/destructor-style path. It removes the source from the global `DAT_00ABCA14` array, tests `LightSource+0x48`, sets it to zero, then calls `0x00554AF0` after `PUSH 0x0`.

### Xrefs To `0x00554AA0` Parameter Update Wrapper

Both active radiation callers pass zero as the queued-mode argument.

- Active in YR: Yes. `RadSiteClass__Activate` at `0x0065B7E8` calls `0x00554AA0`; assembly shows the first pushed stack argument is `PUSH 0x0`, which is the final/rightmost wrapper parameter forwarded to `0x00554AF0`.
- Active in YR: Yes. `RadSiteClass__AI` at `0x0065B8A4` calls `0x00554AA0`; assembly shows `PUSH 0x0` before the other wrapper parameters, again making the forwarded mode zero.

### Xrefs To `0x00554D50`

- Active in YR: Yes. `LogicClassPerTickUpdateLiveVector` calls `0x00554D50` at `0x0055B5F1` after `MOV ECX,0x6` and `XOR DL,DL`. This is the normal tick budgeted queue processor: 6 ms budget, non-forced mode.
- Active in YR: Conditional. `0x00554AF0` calls `0x00554D50` at `0x00554B2E` after `MOV DL,0x1` and `XOR ECX,ECX`, but only when `mode != 0` and pending queue count is nonzero. No active standard YR caller found in this census passes nonzero mode, so this branch is implemented but not reached from the drained standard caller set.

## Answer To Target Question

- Active in YR: No. No active standard YR caller found in this bounded census passes nonzero queued mode into `0x00554AF0`, either directly or through `0x00554A60`, `0x00554A80`, or `0x00554AA0`.
- Active in YR: Yes. Building lamp enable/disable/destruction paths all pass `0`, so they perform immediate affected-cell recompute.
- Active in YR: Yes. Radiation LightSource activation and AI parameter updates also pass `0`, so they perform immediate affected-cell recompute.
- Active in YR: Conditional. The queued-mode support and forced-flush code exists in `0x00554AF0`/`0x00554D50`, but this census found no active standard YR caller that uses it.

## Implementation Handoff

- Active in YR: Yes. Implement building lamp and radiation LightSource invalidation as immediate recompute for parity with all verified standard callers.
- Active in YR: Yes. Keep `0x00554D50`-style queued processing in the model only if implementing the engine's latent queue infrastructure or supporting undiscovered/modded direct calls; it is not required for the verified building/radiation standard caller paths.
- Active in YR: Yes. Separate caller categories in Rust: building lamps use `BuildingClass+0x614`; radiation owns its own `LightSourceClass`; normal logic tick may process queued records but should not be invoked by the standard building/radiation callers when mode is zero.
- Active in YR: Conditional. If a future report finds a live nonzero caller outside this target set, model the all-or-nothing queue commit semantics from `0x00554D50`; do not commit each queued record visibly as soon as it is prepared.

## Negative Facts / Do Not Do

- Active in YR: No. Do not assume building lamp power toggles, construction completion, ownership change, sell, destruction, or INI load enqueue delayed lighting records; all verified call sites pass zero.
- Active in YR: No. Do not assume radiation glow updates enqueue delayed lighting records; both activation and AI parameter-update callers pass zero.
- Active in YR: No. Do not wire `0x00554D50` forced flush into ordinary lamp toggles unless a nonzero caller is separately proven.
- Active in YR: Conditional. Do not delete queued-mode support from the system model entirely; the branch exists and can flush pending records, but this census did not find a standard active caller that requests it.

## Remaining Uncertainty

- Active in YR: Conditional. This report is COMPLETE for the requested four target functions and the `0x00554AA0` wrapper callers. It does not prove that no obscure indirect/vtable path elsewhere can call the same implementation with nonzero mode if Ghidra does not report it as an xref.
- Active in YR: Conditional. The forced-flush branch inside `0x00554AF0` remains real code, but based on the drained xrefs it appears unused by standard building lamp and radiation paths.

## Status

COMPLETE for `LIGHTSOURCE_QUEUED_MODE_CALLER_CENSUS`.
