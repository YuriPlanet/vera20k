# Service Repair Radio 0x1C / 0x22 Path - Ghidra Research Report

**Address(es):** `0x0044B780` (`BuildingClass::MissionRepairAndProduce`), `0x0043C2D0` (`BuildingClass::Receive_Radio`), `0x004D8FB0` (`FootClass::Receive_Radio`), `0x006F4AB0` (`TechnoClass::Receive_Radio`), `0x005F5320` (`ObjectClass::Receive_Radio`)
**Investigation Mode:** exhaustive-slice
**Target question:** How do service depot / hospital / armory paths use radio `0x22` and `0x1C`, who sends them, what receiver behavior gates repair/heal, where timing/formula decisions happen, and what does Rust need to preserve?
**Claimed Scope:** Radio handoff centered on `0x22` need-repair query and `0x1C` repair/heal tick for UnitRepair/Hospital/Armory service paths.
**Non-Scope:** Full repair economy, all service-depot locomotor piggyback details, full helipad reload economy, bridge repair, stock tech-hospital self-heal aura implementation.
**Evidence needed to mark COMPLETE:** Decompile plus assembly context for each scoped sender and receiver; INI/default source plus parser address for flags/rates; Rust surface scan for current service-depot implementation.
**Stop conditions:** Stop after confirming direct senders, receiver returns, chrono rejection, timing threshold, stock YR activity, and Rust-facing deltas. Do not expand into all docking or economy systems.
**Confidence:** High for scoped radio handoff; Medium for the concrete identities of type virtuals `+0xB0/+0xB4` beyond their observed use.
**Active in YR:** Conditional. Service-depot `UnitRepair=` is active in stock YR. Legacy `Hospital=` / `Armory=` walk-in radio paths are parsed and live if set, but stock YR `rulesmd.ini` comments them out and uses self-heal aura keys instead.

## 1. Overview

The repair radio path uses two separate messages. `0x22` is a read-only "does this object still need service?" query handled by `ObjectClass::Receive_Radio`; it returns `10` when health ratio is already at or above `Rules+0x16F8`, otherwise `1`. `0x1C` is the actual repair/heal tick; Foot-derived receivers first reject it while chrono destination `Foot+0x5A4` is non-null, then `TechnoClass::Receive_Radio` charges money, adds HP, updates visual repair effects, and returns continue/done/cannot-afford.

`BuildingClass::MissionRepairAndProduce` sends `0x1C` from timed service loops. `BuildingClass::Receive_Radio` sends `0x22` during admission / cleanup checks so a service building can reject already-healthy or otherwise non-needing occupants before or during handoff.

## 2. Key Offsets

| Owner | Offset | Meaning | Evidence | Active in YR |
|---|---:|---|---|---|
| `BuildingTypeClass` | `+0x16A9` | `UnitRepair=` service-depot flag | key `0x0081AAF0`, reader `0x0046090D` | Yes |
| `BuildingTypeClass` | `+0x16C1` | `Hospital=` legacy walk-in heal flag | key `0x0081AA14`, reader `0x00460AE1`, write `0x00460AFD` | Conditional; stock YR off |
| `BuildingTypeClass` | `+0x16C2` | `Armory=` legacy walk-in promotion flag | key `0x0081AA0C`, reader `0x00460AF7`, write `0x00460B08` | Conditional; stock YR off |
| `BuildingClass` | `+0xBC` | service sub-state | `0x0044B780` decompile | Yes when flags set |
| `BuildingClass` | `+0x620/+0x624/+0x628/+0x634/+0x638` | timer accumulator and CDTimer fields | `0x0044BCE9..0x0044BD0C`, `0x0044B8A2..0x0044B8C5` | Yes when flags set |
| `BuildingClass` | `+0x6DD` | service/dock anim flag | `0x0044BD6D`, `0x0044B90D` | Yes when flags set |
| `RulesClass` | `+0x16E8` | `URepairRate` double | key `0x0083BDC4`, reader/store `0x00670E44..0x00670E51` | Yes |
| `RulesClass` | `+0x16F0` | `IRepairRate` double | key `0x0083BDB8`, reader/store `0x00670E6B..0x00670E78` | Conditional |
| `RulesClass` | `+0x16F8` | service completion health-ratio threshold | read by `0x22` and `0x1C` receivers | Yes |
| `FootClass` | `+0x5A4` | chrono destination / chrono-in-progress gate | `0x004D900E..0x004D9028` | Yes |
| `ObjectClass` | `+0x6C/+0x70` | health / estimated health | `0x006F4D4D..0x006F4D5C`, `0x005F5339` | Yes |

## 3. Core Logic

### `0x22` read-only query

`ObjectClass::Receive_Radio @ 0x005F5320` handles only `0x0D` and `0x22`.

For `0x22`, it loads object health from `+0x6C`, calls vtable `+0x88` for type data, loads max health at type `+0xA0`, computes `current / max`, compares against `Rules+0x16F8`, and returns `10` if at/above threshold or `1` otherwise. It does not mutate health, money, mission, or contacts.

Evidence: decompile `0x005F5320`; assembly `0x005F532C` checks `0x22`, `0x005F5339` loads health, `0x005F5342` calls vtable `+0x88`, `0x005F5348` loads type `+0xA0`, `0x005F5358` compares `Rules+0x16F8`, `0x005F5365` returns `10`, `0x005F537A` returns `1`.

Active in YR: Yes as receiver behavior. Stock service buildings send it from live UnitRepair admission/eviction paths.

### `0x1C` repair tick and chrono rejection

`FootClass::Receive_Radio @ 0x004D8FB0` case `0x1C` is a pre-gate. It reads `Foot+0x5A4`; if non-zero, it returns `10` immediately. If zero, it tail-calls `TechnoClass::Receive_Radio`.

Evidence: decompile `0x004D8FB0`; assembly `0x004D900E` reads `[ESI+0x5A4]`, `0x004D9016` jumps to TechnoClass tail when zero, `0x004D901F` returns `0xA` when non-zero, `0x004D90CC..0x004D90D9` calls `0x006F4AB0`.

`TechnoClass::Receive_Radio @ 0x006F4AB0` case `0x1C`:

1. If `GetHealthRatio() >= Rules+0x16F8`, return `10`.
2. Call type virtual `+0xB0` for repair cost.
3. Call type virtual `+0xB4` for repair step; if `< 2`, clamp to `1`.
4. Query owner available money via owner-side vtable `+0x18`.
5. If money is less than cost, return `0x20`.
6. If cost is non-zero, call `HouseClass::Spend_Money @ 0x004F9790`.
7. Add step to `Health` and `EstimatedHealth`.
8. If a visual repair/warp attachment exists, update its timer and detach.
9. Re-check health ratio; if at/above `Rules+0x16F8`, clamp both health fields to type `+0xA0` and return `0x21`.
10. Otherwise return `1`.

Evidence: decompile `0x006F4AB0`; assembly `0x006F4CD7..0x006F4CEE` threshold early-out, `0x006F4CF0..0x006F4D14` virtual calls, `0x006F4D1C..0x006F4D21` step clamp, `0x006F4D26..0x006F4D37` money check, `0x006F4D48` spend, `0x006F4D4D..0x006F4D5C` HP add, `0x006F4DF2..0x006F4E0A` continue return, `0x006F4E0D..0x006F4E2E` clamp-to-max and `0x21`, `0x006F4E31..0x006F4E3A` insufficient-funds return.

Active in YR: Yes.

## 4. Senders and Timing

`BuildingClass::Receive_Radio @ 0x0043C2D0`, case `0x0E`, sends `0x22` before accepting UnitRepair docking when `Type+0x16A9 UnitRepair` and `DynamicVectorClass::Contains(...)` pass. If the response is `10`, the building returns `10`.

Evidence: `0x0043C820` tests UnitRepair branch, `0x0043C827` calls `DynamicVectorClass::Contains`, `0x0043C833` pushes `0x22`, `0x0043C837` calls vtable `+0x278`, `0x0043C83D` compares response to `0xA`.

Hospital/Armory direct-enter cleanup in `BuildingClass::Receive_Radio 0x0E` loops existing contacts and uses `0x22`; if an existing contact returns `10`, the building sends `0x17` to that contact. Evidence: `0x0043CB18..0x0043CB47`.

UnitRepair entry admission in `BuildingClass::Receive_Radio 0x0F` uses `0x23`, not `0x22`, for occupancy. Evidence: sender type gate `0x0043C544..0x0043C564`; `0x0043C56D` pushes `0x23`; `0x0043C571` sends; `0x0043C577` compares to `1`.

Repair Depot timed service in `BuildingClass::MissionRepairAndProduce` sends `0x13` first, then `0x1C` only if `0x13` returns `1`. Its threshold is `Rules+0x16E8 * 900.0 <= accumulator`. Evidence: `0x0044BD32` loads accumulator, `0x0044BD38` loads `Rules+0x16E8`, `0x0044BD3E` multiplies `0x007E27F8` (`900.0`), `0x0044BD54` pushes `0x13`, `0x0044BD58` sends, `0x0044BD5E` requires response `1`, `0x0044BD69` pushes `0x1C`, `0x0044BD7A` sends.

Hospital timed service sends `0x1C` directly after `Rules+0x16F0 * 900.0 <= accumulator`. Evidence: `0x0044B8EB` loads accumulator, `0x0044B8F1` loads `Rules+0x16F0`, `0x0044B8F7` multiplies `900.0`, `0x0044B90D` pushes `0x1C`, `0x0044B91E` sends.

Armory uses the same `IRepairRate * 900.0` threshold but promotes directly through veterancy helpers and does not send `0x1C`. Evidence: `0x0044BAF4`, `0x0044BAFA`, `0x0044BB00`, then `0x0044BB23` `IsRookie`, `0x0044BB30` `SetVeteran`, `0x0044BB37` `SetElite`.

## 5. INI Keys and Stock YR Activity

| INI key | Binary field | Parser evidence | Stock YR value / status | Active in YR |
|---|---:|---|---|---|
| `UnitRepair=` | `BuildingType+0x16A9` | `0x0081AAF0` xref `0x0046090D` | `GADEPT`, `NADEPT`, `YADEPT`, `CAOUTP` set `UnitRepair=yes` in `rulesmd.ini` | Yes |
| `Hospital=` | `BuildingType+0x16C1` | `0x0081AA14` xref `0x00460AE1`, write `0x00460AFD` | Stock YR comments out `Hospital=yes` on `[CATHOSP]` and `[CAHOSP]` | No for stock YR; Conditional for mods/maps |
| `Armory=` | `BuildingType+0x16C2` | `0x0081AA0C` xref `0x00460AF7`, write `0x00460B08` | Stock YR comments out `;Armory=yes` on old tech building section | No for stock YR; Conditional for mods/maps |
| `URepairRate=` | `Rules+0x16E8` | `0x0083BDC4` xref `0x00670E44`, store `0x00670E51` | `.016` in `rulesmd.ini` | Yes |
| `IRepairRate=` | `Rules+0x16F0` | `0x0083BDB8` xref `0x00670E6B`, store `0x00670E78` | `.001` in `rulesmd.ini` | Conditional legacy path |
| `RepairStep=` | `Rules+0x16CC` | `0x0083BDE8` xref `0x00670DD6`, store `0x00670DE3` | `8` | Not directly read by scoped `TechnoClass 0x1C`; type virtual `+0xB4` supplies step |
| `RepairPercent=` | `Rules+0x16D0` | `0x0083BDF4` xref `0x00670DB7`, store `0x00670DC4` | `15%` | Not directly read by scoped `TechnoClass 0x1C`; type virtual `+0xB0` supplies cost |
| `RepairBay=` | `Rules+0x850` vector | `0x0083C818` xref `0x0066F362` | `GADEPT,NADEPT,CAOUTP;,YADEPT` | Yes for Mission_Unload repair-bay search, not the radio tick itself |
| `InfantryGainSelfHeal=` / `UnitsGainSelfHeal=` | `BuildingType+0x1564/+0x1568` | [TECH_CAHOSP_VS_CATHOSP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECH_CAHOSP_VS_CATHOSP_GHIDRA_REPORT.md) GREEN audit | stock YR tech hospital / machine shop style aura | Yes, separate from this walk-in radio path |

## 6. Current Rust Implementation Status

Current Rust has a direct service-depot FSM in `src/sim/docking/building_dock.rs`. It stores `DockState` on entities, uses `DockPhase::{Approach, WaitForDock, EnterDock, Servicing, ExitDock}`, and applies repair directly on a `service_timer`. It does not model generic radio `0x22`/`0x1C`, Foot chrono rejection, `0x13` pre-check, ToFirst/Contacts[0] delivery, or response-code handling.

Rust rules parsing includes `UnitRepair=`, `URepairRate=`, `RepairStep=`, and `RepairPercent=` in `src/rules/object_type.rs` and `src/rules/ruleset.rs`. Current Rust computes per-step cost from `cost * repair_percent / max_hp * repair_step`, but the verified radio handler asks type virtuals `+0xB0` and `+0xB4`, then charges the full returned cost each successful tick. That may or may not collapse to the same formula after the type virtuals are investigated; this report does not prove equivalence.

The current Rust service timer rounds `URepairRate` to ticks. The binary service-depot loop uses accumulator threshold `Rules+0x16E8 * 900.0 <= +0x620`; default `.016` is `14.4` accumulator units, so exact cadence should follow the accumulator/compare behavior rather than only a rounded nominal timer.

No Rust path was found for legacy `Hospital=` / `Armory=` walk-in service. That is acceptable for stock YR only if the self-heal aura keys are implemented separately; it is not equivalent to the radio walk-in path.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `ObjectClass::Receive_Radio 0x22` | verified | decompile `0x005F5320`; assembly `0x005F532C..0x005F537A` | none |
| `FootClass::Receive_Radio 0x1C` chrono gate | verified | decompile `0x004D8FB0`; assembly `0x004D900E..0x004D9028` | none |
| `TechnoClass::Receive_Radio 0x1C` | verified | decompile `0x006F4AB0`; assembly `0x006F4CD7..0x006F4E3A` | exact type virtual `+0xB0/+0xB4` bodies not renamed |
| Building admission sender `0x22` | verified | decompile `0x0043C2D0`; assembly `0x0043C820..0x0043C83D` | none |
| Hospital/Armory cleanup sender `0x22 -> 0x17` | verified | decompile `0x0043C2D0`; assembly `0x0043CB18..0x0043CB47` | stock YR inactive for flags |
| UnitRepair entry uses `0x23`, not `0x22` | verified | assembly `0x0043C544..0x0043C577` | none |
| Repair Depot timed `0x13 -> 0x1C` sender | verified | decompile `0x0044B780`; assembly `0x0044BCE9..0x0044BD7A` | exact locomotor piggyback behavior deferred |
| Hospital timed `0x1C` sender | verified | decompile `0x0044B780`; assembly `0x0044B8A2..0x0044B91E` | stock YR inactive for flag |
| Armory no-`0x1C` promotion branch | verified | decompile `0x0044B780`; assembly `0x0044BAAB..0x0044BB37` | stock YR inactive for flag |
| INI parser offsets | verified | strings/xrefs listed in section 5 | none |
| Current Rust direct service FSM | verified | `src/sim/docking/building_dock.rs`, `src/rules/ruleset.rs`, `src/rules/object_type.rs` scan | none |
| Full repair economy virtuals | deferred | out of scope | separate investigation of type vtable `+0xB0/+0xB4` concrete implementations |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-SR-001 - What is the target slice? -> Service repair/heal radio handoff around `0x22` query and `0x1C` tick, not full economy.` (evidence: user scope)
- `[RESOLVED] OQ-SR-002 - What does `0x22` return? -> `10` when health ratio >= `Rules+0x16F8`, else `1`; no mutation.` (evidence: `0x005F5339..0x005F537A`)
- `[RESOLVED] OQ-SR-003 - Who sends `0x22` for UnitRepair admission? -> `BuildingClass::Receive_Radio 0x0E` under `UnitRepair` + vector-contained gate.` (evidence: `0x0043C820..0x0043C83D`)
- `[RESOLVED] OQ-SR-004 - Do Hospital/Armory paths use `0x22`? -> Yes in the legacy direct-enter cleanup loop; if an existing contact answers `10`, building sends `0x17`.` (evidence: `0x0043CB18..0x0043CB47`)
- `[RESOLVED] OQ-SR-005 - Is `0x22` the UnitRepair occupancy check? -> No; `0x0F` UnitRepair uses `0x23` occupancy.` (evidence: `0x0043C56D..0x0043C577`)
- `[RESOLVED] OQ-SR-006 - What is the `0x1C` chrono rejection? -> If `Foot+0x5A4 != 0`, return `10`; no Techno repair logic runs.` (evidence: `0x004D900E..0x004D9028`)
- `[RESOLVED] OQ-SR-007 - What does Techno `0x1C` mutate? -> Charges money, adds repair step to `Health` and `EstimatedHealth`, updates/detaches visual repair effect, clamps to max on completion.` (evidence: `0x006F4D26..0x006F4E2E`)
- `[RESOLVED] OQ-SR-008 - What are Techno `0x1C` replies? -> `10` already at threshold, `0x20` insufficient funds, `0x21` complete, `1` continue.` (evidence: `0x006F4CD7..0x006F4E3A`)
- `[RESOLVED] OQ-SR-009 - Does Repair Depot send `0x13` before `0x1C`? -> Yes; it sends `0x1C` only if `0x13` returns `1`.` (evidence: `0x0044BD54..0x0044BD7A`)
- `[RESOLVED] OQ-SR-010 - What is the Repair Depot timing threshold? -> `Rules+0x16E8 * 900.0 <= accumulator`.` (evidence: `0x0044BD32..0x0044BD44`)
- `[RESOLVED] OQ-SR-011 - What is the Hospital timing threshold? -> `Rules+0x16F0 * 900.0 <= accumulator`.` (evidence: `0x0044B8EB..0x0044B904`)
- `[RESOLVED] OQ-SR-012 - Does Armory send `0x1C`? -> No; it uses the infantry timer threshold but calls veterancy helpers directly.` (evidence: `0x0044BAF4..0x0044BB37`)
- `[RESOLVED] OQ-SR-013 - Are Hospital/Armory walk-in paths stock YR active? -> No for stock `rulesmd.ini`; `Hospital=` and `Armory=` are parsed but stock YR comments them out and uses aura keys.` (evidence: `ini/rulesmd.ini:13992`, `14016`, `14040`; parser `0x00460AE1`, `0x00460AF7`; [TECH_CAHOSP_VS_CATHOSP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECH_CAHOSP_VS_CATHOSP_GHIDRA_REPORT.md))
- `[RESOLVED] OQ-SR-014 - Are UnitRepair service depots stock YR active? -> Yes; `GADEPT`, `NADEPT`, `YADEPT`, and `CAOUTP` set `UnitRepair=yes`.` (evidence: `ini/rulesmd.ini:11895`, `12683`, `13438`, `13886`)
- `[RESOLVED] OQ-SR-015 - What current Rust surface implements this? -> `src/sim/docking/building_dock.rs` direct FSM plus rules parsing in `src/rules/ruleset.rs` / `src/rules/object_type.rs`.` (evidence: Rust scan)
- `[DEFERRED] OQ-SR-016 - Exact concrete implementations of type virtuals `+0xB0/+0xB4`.` (category: out-of-scope; reason: repair radio handoff only needs observed call shape; next-step-if-pursued: investigate TechnoType/ObjectType virtual table cost/repair-step methods)
- `[DEFERRED] OQ-SR-017 - Full repair-depot piggyback locomotor entry/exit parity.` (category: out-of-scope; reason: not needed to prove 0x1C/0x22 handoff; next-step-if-pursued: investigate `0x0044C62A..0x0044C819` as a locomotor slice)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Repair Depot service tick sends `0x13` first, then `0x1C` only on `ROGER`, after accumulator threshold `URepairRate * 900.0`. | `0x0044BD32..0x0044BD7A` | Missing: direct timer applies HP without radio/link pre-check. | `src/sim/docking/building_dock.rs`; future generic radio/contact layer | Preserve the link check and only apply repair when contact is valid and non-chrono. | `service_depot_repair_tick_requires_radio_0x13_before_0x1c` | Do not make depot repair a free-standing entity timer detached from radio/contact state. |
| Foot-derived `0x1C` receivers reject while `Foot+0x5A4` is non-null. | `0x004D900E..0x004D9028` | Missing/unchecked: Rust service repair does not check chrono warp state before healing. | `src/sim/docking/building_dock.rs`, `src/sim/movement/teleport_movement.rs` | A chrono-warping unit on/near a service depot must not receive repair ticks until chrono state clears. | `service_depot_repair_rejects_chrono_warping_unit` | Do not check only movement target or mission; binary gate is chrono destination/state. |
| `0x22` is a health-ratio query and must not mutate state. | `0x005F532C..0x005F537A` | Missing generic radio; direct code uses `hp >= max_hp` locally. | future radio abstraction; building admission logic | Admission/cleanup should distinguish "needs service" (`1`) from "already at threshold" (`10`) without applying HP or spending money. | `service_repair_radio_0x22_is_read_only_health_threshold_query` | Do not fold `0x22` into `0x1C`. |
| Techno `0x1C` charges returned type cost and adds returned type repair step, clamping step `<2` to `1`, then clamps to max only on completion. | `0x006F4CF0..0x006F4E2E` | Likely mismatch: Rust computes cost from `RepairPercent` and `RepairStep` directly per step. | `src/sim/docking/building_dock.rs`, rules/type methods | Model the binary call shape or verify type virtuals before relying on Rust's current formula. | `service_repair_cost_and_step_follow_type_virtuals` | Do not assume `RepairPercent` is directly the per-tick charge in the radio handler. |
| Stock YR tech Hospital / Armory walk-in radio flags are inactive; stock Hospital behavior is self-heal aura. | `ini/rulesmd.ini:13992`, `14016`, `14040`; parser `0x00460AE1`, `0x00460AF7` | Correct to omit legacy walk-in service for stock parity; self-heal aura separate. | `src/rules/object_type.rs`, future tech-building capture/aura systems | Prioritize `InfantryGainSelfHeal` / `UnitsGainSelfHeal` for stock tech buildings. | `stock_yr_cathosp_uses_global_infantry_self_heal_not_walkin_radio` | Do not implement stock CAHOSP as a docked walk-in `0x1C` hospital. |

**Concrete Rust test-name proposal:** `service_depot_repair_rejects_chrono_warping_unit`.

## 10. Negative Facts / Do Not Do

- Do not treat radio `0x22` as "repair one step"; it is a read-only health-threshold query.
- Do not apply `0x1C` repair to Foot-derived receivers while chrono destination/state `+0x5A4` is non-null.
- Do not implement Armory promotion as repeated `0x1C` repair ticks; Armory promotes directly through veterancy helpers after the infantry timer threshold.
- Do not treat stock YR `[CATHOSP]`/`[CAHOSP]` as legacy `Hospital=yes` walk-in buildings; `rulesmd.ini` comments out that key and uses aura counters.
- Do not preserve the older [BUILDINGCLASS_MISSION_REPAIR_AND_PRODUCE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MISSION_REPAIR_AND_PRODUCE.md) wording that service depot threshold is `Rules+0x16E8 * 1.0`; verified assembly uses `* 900.0`.

## 11. Remaining Uncertainty

- The exact concrete bodies behind type virtuals `+0xB0` and `+0xB4` were not decompiled in this slice, so the report states the verified call shape, not a complete economy formula.
- The repair-depot locomotor piggyback entry/exit shape is only touched where needed for sender timing; exact movement parity remains separate.
- Empty/misaligned contact-array edge cases for `ToFirst(0x1C)` rely on the prior RadioClass report rather than being re-decompiled here.

## 12. Stale Docs / Follow-up Docs

- [docs/research/BUILDINGCLASS_MISSION_REPAIR_AND_PRODUCE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MISSION_REPAIR_AND_PRODUCE.md) section "Repair tick tuning" should replace "threshold = Rules+0x16E8 x 1.0" with "threshold = Rules+0x16E8 x 900.0 (`DAT_007E27F8`), verified at `0x0044BD38..0x0044BD44`."
- [docs/research/MISSION_REPAIR_AND_PRODUCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSION_REPAIR_AND_PRODUCE_GHIDRA_REPORT.md) should avoid saying `Rules+0x16F8` is `RepairPercent`; replacement wording: "`Rules+0x16F8` is the service completion health-ratio threshold read by `0x1C` and `0x22`; `RepairPercent=` is parsed to `Rules+0x16D0` and is not directly read at the scoped `TechnoClass::Receive_Radio 0x1C` site."

## Sources

- Ghidra `decompile_function 0044B780`
- Ghidra `get_assembly_context 0044BCE9,0044BD54,0044BD7A`
- Ghidra `get_assembly_context 0044B8EB,0044B90D,0044B91E`
- Ghidra `get_assembly_context 0044BAF4,0044BB00,0044BB1B`
- Ghidra `decompile_function 0043C2D0`
- Ghidra `get_assembly_context 0043C820,0043C833,0043C837,0043CB24,0043CB47,0043C56D`
- Ghidra `decompile_function 004D8FB0`
- Ghidra `get_assembly_context 004D900E,004D901F,004D90CC`
- Ghidra `decompile_function 006F4AB0`
- Ghidra `get_assembly_context 006F4CD7,006F4CF0,006F4D1A,006F4D48,006F4DF2,006F4E0D`
- Ghidra `decompile_function 005F5320`
- Ghidra `get_assembly_context 005F532C,005F5339,005F5358,005F537A`
- Ghidra string/xref checks for `UnitRepair`, `Hospital`, `Armory`, `URepairRate`, `IRepairRate`, `RepairStep`, `RepairPercent`, `RepairBay`
- `ini/rulesmd.ini`, `ini/rules.ini`
- [docs/research/FOOTCLASS_RECEIVE_RADIO_FULL_SWITCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_RECEIVE_RADIO_FULL_SWITCH_GHIDRA_REPORT.md)
- [docs/research/BUILDINGCLASS_RECEIVE_RADIO_FULL_SWITCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_RECEIVE_RADIO_FULL_SWITCH_GHIDRA_REPORT.md)
- [docs/research/RADIOCLASS_CORE_PRIMITIVES_VERIFIED_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADIOCLASS_CORE_PRIMITIVES_VERIFIED_GHIDRA_REPORT.md)
- [docs/research/TECH_CAHOSP_VS_CATHOSP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECH_CAHOSP_VS_CATHOSP_GHIDRA_REPORT.md)
- Rust scan: `src/sim/docking/building_dock.rs`, `src/rules/ruleset.rs`, `src/rules/object_type.rs`

## Status

COMPLETE for the scoped `0x1C` / `0x22` service repair radio handoff.
