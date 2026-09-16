# Power, SpySat Reveal, and Tactical Radar — Current System Model Synthesis

Date: 2026-07-10  
Type: conflict-map  
Status: core mechanisms are doc-patch-ready; passive SpySat power eligibility needs re-investigation

## Scope

This synthesis reconciles the currently documented and live-binary evidence for:

- `BuildingTypeClass` `Radar` and `SpySat` flags;
- tactical-radar availability, including scenario `FreeRadar`;
- passive SpySat map reveal and restoration;
- the relationship between house power rechecks and those two mechanisms;
- current Rust implementation deltas.

Non-scope: exact animated satellite-overlay asset geometry and timing, generic radar pixel formulas, and Rust implementation work.

## Claim Table

| Claim | Best evidence | Rank | Disposition |
|---|---|---:|---|
| Building-type byte `+0x16A4` is `Radar`; `+0x16A5` is `SpySat` | Live `BuildingTypeClass__ReadINI` at `0x0045FE50` | BINARY_HIGH | Implementation-safe |
| Stock `GASPYSAT` has `Radar=yes`, `SpySat=yes`, `Power=-100`, `Powered=true` | `ini/rulesmd.ini` plus live INI reader | BINARY_HIGH | Implementation-safe |
| `0x00508DF0` computes tactical-radar availability | Live body and sole caller in `HouseClass__Update` | BINARY_HIGH | Implementation-safe |
| Scenario `+0x34A4` is `[Basic] FreeRadar`, default false | Live `ScenarioClass__Read_INI_Basic` at `0x00689E90` and initializer `0x00683610` | BINARY_HIGH | Implementation-safe |
| `0x00508F60` manages passive SpySat activation/restoration | Live body, sole caller, and reveal/restore callees | BINARY_HIGH | Implementation-safe for role |
| `0x00577D90` reveals the local map; `0x00577AB0` restores shroud bookkeeping | Live bodies and cell-state writes | BINARY_HIGH | Implementation-safe |
| Virtual slot `+0x1D4` resolves to `TechnoClass__IsWarpingOut` at `0x0070C5B0` | Vtable-target resolution and target body | BINARY_HIGH | Doc-patch-ready |
| Passive SpySat is directly gated by house power, building `+0x660`, or type `Powered` | Not present in live `0x00508F60`; existing prose disagrees | CONFLICT | Needs re-investigation |
| Animated satellite overlay is separate from passive map reveal | Existing address-backed research and distinct call pipelines | RESEARCH_HIGH | Safe negative separation |
| Current Rust matches the native transition mechanism | Rust recomputes reveal each fog update and lacks `FreeRadar` | SOURCE_HIGH | DRIFT |

## Current Model

`Radar` and `SpySat` are separate building-type flags. A stock `GASPYSAT` carries both, so one structure participates in two distinct house-level mechanisms.

Tactical-radar availability is computed by `0x00508DF0`. For the local house, scenario `FreeRadar` makes radar available unconditionally. Otherwise, the function first requires sufficient house power and then searches for an eligible `Radar` building. Its building checks include online byte `+0x660`, active/not-limbo state, mission-state exclusions, EMP exclusion at `+0x504`, and `IsWarpingOut == false`. It compares the desired result with the current tactical-map state and invokes `0x00656DF0` only on a transition.

Passive SpySat reveal is managed separately by `0x00508F60`. It searches for an eligible building whose type has `SpySat`, excludes inactive/limbo and selected mission states, and excludes warping-out objects. On an inactive-to-active transition it calls `0x00577D90`; when no eligible SpySat remains, it calls `0x00577AB0`. These functions use house/player idempotence bytes and mutate map-cell reveal/shroud fields before refreshing radar/tactical presentation.

`HouseClass__Update` at `0x004F8440` connects both checks to the house update loop. When `RecheckPower` is set, it assesses power and schedules `RecheckRadar`; the radar recheck then calls both `0x00508DF0` and `0x00508F60`. This proves scheduling adjacency, not identical eligibility rules.

## Implementation-Safe Facts

- Read `Radar` from type byte `+0x16A4` and `SpySat` from `+0x16A5`.
- Treat tactical radar and passive SpySat reveal as different state machines even when the same building supplies both flags.
- Implement `FreeRadar` as a scenario-level override for tactical-radar availability. The live reader maps `[Basic] FreeRadar` to scenario `+0x34A4`; initialization sets it false.
- Tactical-radar availability is transition-driven and power-sensitive. The stock-building scan includes `Radar`, online, active/not-limbo, non-EMP, non-warping, and mission-state checks.
- Passive SpySat reveal/restore is transition-driven and idempotent, not a blanket reveal write performed every fog recomputation.
- Keep passive reveal separate from the animated satellite overlay. Overlay animation is not the mechanism that changes persistent cell reveal state.

## Doc-Patch-Ready Facts

- Replace every interpretation of type `+0x16A5` as `NeedsPower` or `PoweredSpecialShroud` with `SpySat`.
- Relabel `0x00508DF0` by verified role as the tactical-radar availability check; its current local Ghidra label suggesting superweapon readiness is misleading.
- Relabel `0x00508F60` by verified role as the passive SpySat activation/restoration check; its current local label suggesting a generic low-power check is misleading.
- Replace descriptions of virtual `+0x1D4` as an online/powered-style predicate with `TechnoClass__IsWarpingOut` at `0x0070C5B0`.
- Identify scenario `+0x34A4` as `FreeRadar`, not an unnamed or inferred radar override.
- Do not use the power report's old radar-polarity or Rust-degradation handoff until corrected against `0x00508DF0`.

## Stale or Superseded Statements

- `POWER_SYSTEM_GHIDRA_REPORT.md`: `+0x16A5` as `PoweredSpecialShroud`, `0x00508F60` as a low-power transition, and `0x00508DF0` as a superweapon-ready check are superseded by live-body evidence.
- [HOUSECLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/HOUSECLASS_GHIDRA_REPORT.md): `+0x16A5` as `NeedsPower` is superseded.
- [SPY_SATELLITE_REVEAL_RADAR_PIXEL_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPY_SATELLITE_REVEAL_RADAR_PIXEL_PIPELINE_GHIDRA_REPORT.md): the virtual `+0x1D4` gate is not an online/powered-style gate; it resolves to `IsWarpingOut`.

## Cross-Document Conflict

The dedicated SpySat report describes an “eligible powered SpySat building,” while the live `0x00508F60` body contains no direct read of house power ratio, building online byte `+0x660`, or type `Powered` at `+0x1573`. Power recalculation does cause `HouseClass__Update` to schedule the SpySat check, but that scheduling alone does not prove that low power invalidates the candidate. The radar function at `0x00508DF0` does contain explicit power and online gates, which may have been incorrectly generalized to passive reveal in earlier prose.

## Needs Re-Investigation

One bounded question blocks a complete implementation contract:

> What exact native state transition makes a `Powered=true` SpySat cease or resume passive reveal during low power, EMP, offline, sell, destruction, and multiple-SpySat cases?

Required investigation:

1. Trace writers and semantic roles for the `0x00508F60` candidate fields `+0x74`, `+0x81`, and type/house gate `+0x41B`.
2. Trace `Powered=true` and building online `+0x660` transitions from power assessment into any field consumed by `0x00508F60`.
3. Observe call ordering and same-tick state for low-power, repower, EMP, sell, destruction, and two-SpySat fixtures.
4. Verify whether restoration occurs only when the last eligible SpySat disappears.

Suggested command:

`/re-investigate SpySat passive reveal eligibility across Powered=true, low power, EMP, online +0x660, HouseClass::Update 0x004F8440 and 0x00508F60`

## Do Not Implement Yet

- Do not directly gate passive SpySat reveal on `PowerRatio`, `+0x660`, or `Powered=true` without the bounded investigation above.
- Do not assume tactical-radar eligibility and passive-reveal eligibility are identical.
- Do not reset all explored state immediately when any one SpySat is sold or destroyed unless last-provider and same-tick behavior are proven.
- Do not treat an every-tick full-map reveal as mechanism parity merely because its eventual visible result can look similar.
- Do not reuse the animated overlay's assets or timing as reveal-state authority.

## Current Rust Delta

- `src/sim/vision/mod.rs` applies `VISIBLE | REVEALED` to every cell whenever `apply_spy_sat` runs.
- `src/sim/world/mod.rs` rebuilds eligible SpySat owners during each fog recomputation and reapplies the full reveal, rather than preserving the native transition/idempotence state machine.
- Destruction and sell paths reset explored state for the owner when a SpySat is removed. This is unproven for multiple-provider and same-tick cases.
- `src/sim/power_system.rs` and `src/sim/radar.rs` combine `Radar`/`SpySat` providers under a powered-building rule. That may approximate stock `GASPYSAT`, but it does not preserve the separate native mechanisms.
- No Rust `FreeRadar` scenario handling was found.
- Existing vision tests prove a basic full reveal only; they do not certify activation/deactivation order, idempotence, repower, EMP, sell/destruction, or multiple-provider parity.

Verdict: DRIFT for exact mechanism; the tactical-radar path is ready for an implementation contract, while passive SpySat power eligibility remains blocked on targeted binary research.

## Source Ledger

- Live Ghidra decompile: `BuildingTypeClass__ReadINI`, `0x0045FE50`.
- Live Ghidra decompile/callers: house functions `0x00508DF0`, `0x00508F60`; `HouseClass__Update`, `0x004F8440`.
- Live Ghidra vtable target: `TechnoClass__IsWarpingOut`, `0x0070C5B0`.
- Live Ghidra decompile: reveal `0x00577D90`, restore `0x00577AB0`, tactical-map state change `0x00656DF0`.
- Live Ghidra decompile: `ScenarioClass__Read_INI_Basic`, `0x00689E90`; scenario initializer `0x00683610`.
- Retail rules: `ini/rulesmd.ini` and `ini/rules.ini`, `[GASPYSAT]`.
- Research: [SPY_SATELLITE_REVEAL_RADAR_PIXEL_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPY_SATELLITE_REVEAL_RADAR_PIXEL_PIPELINE_GHIDRA_REPORT.md), `POWER_SYSTEM_GHIDRA_REPORT.md`, [HOUSECLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/HOUSECLASS_GHIDRA_REPORT.md), [RADAR_SYSTEM_COMPREHENSIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADAR_SYSTEM_COMPREHENSIVE.md).
- Rust: `src/sim/vision/mod.rs`, `src/sim/world/mod.rs`, `src/sim/power_system.rs`, `src/sim/radar.rs`, sell/destruction paths, and vision tests.
