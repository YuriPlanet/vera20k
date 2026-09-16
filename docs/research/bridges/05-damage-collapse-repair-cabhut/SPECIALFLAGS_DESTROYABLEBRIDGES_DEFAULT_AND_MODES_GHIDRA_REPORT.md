# SpecialFlags DestroyableBridges Default And Modes - Ghidra Research Report

**Address(es):** `0x006B8AE0` reset/default writer; `0x006B8CA0` `[SpecialFlags]` reader; `0x006B8B30` `[SpecialFlags]` writer; `0x006832C0` `ScenarioClass` constructor; `0x0052F620` init/reset; `0x00689E90` `ScenarioClass::Read_INI_Basic`; `0x00686B20` `ScenarioClass::Full_Init`; consumer spot-check `0x00489280`.
**Investigation Mode:** exhaustive-slice.
**Claimed Scope:** exact default initialization and read/write mode behavior for `DestroyableBridges` / SpecialFlags bit `0x8000`, including campaign/editor vs multiplayer override ownership and current Rust parser handoff.
**Non-Scope:** bridge collapse state machines, C4/CABHUT collapse, bridge visual fallout, full multiplayer lobby UI control construction, full savegame serialization.
**Confidence:** High for reset/default, reader/writer body, mode gates, multiplayer active-flag overwrite, and Rust mismatch. Medium for the exact UI path from rules `BridgeDestruction` into `DAT_00A8B260`, because this report verifies the rules parser and scenario-load consumer but not every lobby control writer.
**Active in YR:** Yes, conditional by mode. Standard YR skirmish/multiplayer uses lobby/session ownership; campaign and map editor can read map `[SpecialFlags] DestroyableBridges`.

## 0. Working Notes Required By Swarm Prompt

Target question: What initializes and owns `DestroyableBridges` / SpecialFlags bit `0x8000`, which INI section reads/writes it, and how should Rust own the flag?

Non-goals: Do not re-investigate bridge damage results, bridge state machines, bridge repair/hut collapse, or unit fallout. Do not patch Rust.

Evidence needed to mark COMPLETE: decompile plus assembly context for the reset/default instruction; decompile plus assembly/string context for the `[SpecialFlags]` reader and writer; decompile plus assembly context for scenario-load mode gates; INI evidence for stock rule/default lines; Rust scan of parser/storage surfaces.

Stop conditions: stop once bit default, absent-key defaulting, campaign/editor read behavior, multiplayer/lobby ownership, writer body, consumer identity, and Rust deltas are all resolved or explicitly deferred.

## 1. Overview

`DestroyableBridges` is SpecialFlags bit 15 (`0x8000`) in the active `ScenarioClass` flag dword. The binary default/reset path sets this bit on with mask/or literal `0x8088`; the map `[SpecialFlags]` reader preserves that default when the key is absent and only reads `DestroyableBridges` from map INI in campaign or map editor mode.

Normal skirmish/multiplayer does not let a map `[SpecialFlags] DestroyableBridges=` line override the bridge-damage gate. In multiplayer, scenario load later copies the staging/session flags word `DAT_00A8E960` into active `*g_ScenarioClass_Instance`; if the lobby/session byte `DAT_00A8B260` is off, it clears staging bit `0x8000` before that copy.

## 2. Class Layout / Key Offsets

| Offset / global | Meaning | Evidence | Active in YR |
|---|---|---|---|
| `ScenarioClass+0x000` / `*g_ScenarioClass_Instance` | active SpecialFlags bitfield | `0x006B8CA0` takes `uint *`; consumer `0x00489280` tests `*g_ScenarioClass_Instance & 0x8000`; constructor calls reset with `ECX=ScenarioClass this` | Yes |
| bit `0x8000` | `DestroyableBridges` | reader uses string `0x00840248`, shifts by `0xF`; writer shifts by `0xF`; consumer tests mask `0x8000` | Yes |
| `DAT_00A8E960` | staging/session flags word used in multiplayer | init reset at `0x0052F62B..0x0052F634`; `Full_Init` clear/copy at `0x00687955..0x00687966` and `0x00687C1E..0x00687C29` | Conditional: multiplayer/session path |
| `DAT_00A8B260` | bridge destruction lobby/session byte | `Full_Init` tests it before clearing `DAT_00A8E960 & ~0x8000`; `RulesClass::ReadMultiplayerDialogSettings` reads `BridgeDestruction` to `Rules+0x14AC` | Conditional: multiplayer/session path |
| `Rules+0x14AC` | `[MultiplayerDialogSettings] BridgeDestruction` default | parser `0x00671EA0`; retail `rulesmd.ini:3029` = yes | Conditional: multiplayer lobby default |
| `PTR_s_SpecialFlags_008401CC/008401C8` | read/write section names | reader `0x006B8CA0`, writer `0x006B8B30` | Yes |

## 3. Core Logic

### 3.1 Reset / Default

The reset helper at `0x006B8AE0` reads the target flags dword, applies `& 0xFFF88088`, then `| 0x8088`, and writes it back. Bit `0x8000` is therefore set on reset. Bit `0x0080` is also set; bits outside this slice are not claimed here.

Active in YR: Yes. Evidence: decompile `0x006B8AE0`; assembly context `0x006B8AE0..0x006B8AEE` shows `AND EAX,0xfff88088`, `OR EAX,0x8088`, store through `ECX`.

`ScenarioClass__Constructor @ 0x006832C0` calls that reset after moving `ECX=EBP` (the new scenario object). Active in YR: Yes. Evidence: decompile `0x006832C0`; assembly context around `0x006833E0..0x006833EE` shows `MOV ECX,EBP` then `CALL 0x006B8AE0`.

The process/init path `FUN_0052F620` also resets the staging flags word before command-line parsing by setting `ECX=0x00A8E960` and calling `0x006B8AE0`. Active in YR: Yes. Evidence: decompile `0x0052F620`; assembly context `0x0052F62B..0x0052F634`.

### 3.2 `[SpecialFlags]` Reader

`FUN_006B8CA0` reads map/scenario `[SpecialFlags]` into the target flags dword. `DestroyableBridges` is read with current bit 15 as the default value, then written back as `(read_bool & 1) << 0xF` while clearing the old bit with `& 0xFFFF7FFF`.

The read is conditional. The first six flags in the function are always read: `TiberiumExplosive`, `MCVDeploy`, `InitialVeteran`, `IonStorms`, `Meteorites`, and `Visceroids`. `DestroyableBridges` is in the later block gated by `(g_GameMode == 0) || (g_IsMapEditor != 0)`, alongside `TiberiumGrows`, `TiberiumSpreads`, `FixedAlliance`, `FogOfWar`, `Inert`, and `HarvesterImmune`.

Active in YR: Yes, conditional. Evidence: decompile `0x006B8CA0`; assembly context `0x006B8E1F..0x006B8E39` shows string `0x00840248`, call to `CCINIClass__ReadBool`, `SHL EAX,0xF`, `AND CH,0x7F`, and store. Caller evidence: `ScenarioClass::Read_INI_Basic @ 0x00689E90` calls `0x006B8CA0` at the beginning of map basic parsing.

### 3.3 `[SpecialFlags]` Writer

`FUN_006B8B30` serializes all SpecialFlags keys under `[SpecialFlags]`. For `DestroyableBridges`, it writes `(*flags >> 0xF) & 1` using the same key string `0x00840248`. There is no mode gate inside the writer body.

Active in YR: Conditional. The writer body is present and uses the same bit/key mapping; the exact map-save/editor caller inventory was not material to gameplay ownership and was not exhausted. Evidence: decompile `0x006B8B30`; assembly context `0x006B8B92..0x006B8BA0` shows `SHR EAX,0xF`, `AND AL,0x1`, push key string `0x840248`, and writer call `0x00529560`.

### 3.4 Scenario Load Modes

`ScenarioClass::Read_INI_Basic @ 0x00689E90` always invokes the SpecialFlags reader early, but `DestroyableBridges` itself is only read inside the campaign/editor block described above. Therefore:

- Campaign (`g_GameMode == 0`): map `[SpecialFlags] DestroyableBridges=` can override the reset default.
- Map editor (`g_IsMapEditor != 0`): map/editor INI can override.
- Skirmish/multiplayer (`g_GameMode != 0`, editor false): map `[SpecialFlags] DestroyableBridges=` is ignored for this bit.

Active in YR: Yes. Evidence: reader decompile `0x006B8CA0`; caller `0x00689E90`; `Full_Init @ 0x00686B20` sets up the scenario-load mode flow.

In multiplayer, after `Read_INI_Basic`, `Full_Init` checks `g_GameMode != 0` and `DAT_00A8B260 == 0`; if bridge destruction is disabled in the session, it clears bit `0x8000` in staging `DAT_00A8E960` via `AND AH,0x7F`. Later in the same load path, when `g_GameMode != 0`, it copies `DAT_00A8E960` into the active scenario flags dword.

Active in YR: Yes for multiplayer/skirmish load. Evidence: decompile `0x00686B20`; assembly context `0x0068794D..0x00687966` for `DAT_00A8B260` test and staging clear; assembly context `0x00687C16..0x00687C29` for `if g_GameMode != 0 { *g_ScenarioClass = DAT_00A8E960 }`.

### 3.5 Rules / INI Sources

Retail `ini/rulesmd.ini:804` and `ini/rules.ini:664` contain `[CombatDamage] DestroyableBridges=yes`, but this is not the binary reader for the active gate. The active map reader is `[SpecialFlags] DestroyableBridges`; the multiplayer lobby default is the separate `[MultiplayerDialogSettings] BridgeDestruction=yes` at `ini/rulesmd.ini:3029` / `ini/rules.ini:2509`, parsed to `Rules+0x14AC`.

Active in YR: `[CombatDamage] DestroyableBridges` is not active as a rules key; `[SpecialFlags] DestroyableBridges` is active conditionally; `[MultiplayerDialogSettings] BridgeDestruction` is active for lobby/session defaulting. Evidence: `0x006B8CA0`, `0x00671EA0`, retail INI lines above, and no `DestroyableBridges` read in `RulesClass::ReadCombatDamage` per [WEAPON_AOE_BRIDGE_DAMAGE_ENTRY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/WEAPON_AOE_BRIDGE_DAMAGE_ENTRY_GHIDRA_REPORT.md).

### 3.6 Consumer Identity

The bridge tile damage consumer remains the standard AoE gate in `Apply_area_damage`: SpecialFlags bit `0x8000` must be set and the warhead must have `Wall=yes`.

Active in YR: Yes. Evidence: consumer spot-check `0x00489280` and prior report [WEAPON_AOE_BRIDGE_DAMAGE_ENTRY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/WEAPON_AOE_BRIDGE_DAMAGE_ENTRY_GHIDRA_REPORT.md). This report did not re-drain bridge collapse outcomes after the gate.

## 4. INI Keys

| Key | Section | Stock YR value | Binary read/write | Effect | Active in YR |
|---|---|---:|---|---|---|
| `DestroyableBridges` | map/scenario `[SpecialFlags]` | absent in stock rules; map optional | reader `0x006B8CA0`, writer `0x006B8B30` | bit `0x8000` in SpecialFlags | Conditional: campaign/editor read; writer body always writes if invoked |
| `DestroyableBridges` | rules `[CombatDamage]` | `yes` | no `RulesClass::ReadCombatDamage` read found in prior report | no binary rules-parser effect | No as rules key |
| `BridgeDestruction` | `[MultiplayerDialogSettings]` | `yes` | `RulesClass::ReadMultiplayerDialogSettings @ 0x00671EA0` to `Rules+0x14AC` | lobby/session default feeding `DAT_00A8B260` ownership | Conditional: multiplayer/skirmish session |

## 5. Integration Points

`ScenarioClass__Constructor` and init reset make bit `0x8000` default on before map parsing. Campaign/editor map load may override it through `[SpecialFlags]`; absent key preserves the reset value because `CCINIClass__ReadBool` receives the current bit as default. Multiplayer/skirmish keeps map `[SpecialFlags]` from changing this bit, then takes the final active value from session staging `DAT_00A8E960`, with the bridge-destruction session byte able to clear it.

The consumer reads active scenario flags at runtime; the staging word matters only because multiplayer copies it into active scenario flags during scenario load.

## 6. Current Rust Implementation Status

Scanned Rust surfaces:

- `src/map/basic.rs:28..67` parses map `[SpecialFlags] DestroyableBridges` as `Option<bool>`. This is useful but currently lacks the binary's mode condition at the point of applying it.
- `src/rules/ruleset.rs:735..757` defaults `BridgeRules::destroyable_by_default = true` but then reads `[CombatDamage] DestroyableBridges`. That is the wrong binary owner for this bit.
- `src/rules/ruleset.rs:2516..2531` has a regression test whose comments assert the old wrong premise: `[SpecialFlags] DestroyableBridges` ignored because gamemd reads `[CombatDamage]`.
- `src/sim/bridge_state/mod.rs:792..795` stores the runtime destroyable flag as the outer gate; this is a good consumer location if fed from the right source.
- `src/sim/world/bridge_orchestrator.rs:62..66` bails when `bridge_state.is_destroyable()` is false, matching the consumer shape.

Main Rust delta: split the binary concepts. `[CombatDamage] DestroyableBridges` must not set bridge destroyability. Map `[SpecialFlags] DestroyableBridges` should apply only in campaign/editor-style loading; skirmish/multiplayer should derive the active bit from reset default plus session/lobby `BridgeDestruction`.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| reset/default bit `0x8000` | verified | `0x006B8AE0`; assembly `0x006B8AE0..0x006B8AEE` | none |
| ScenarioClass constructor reset call | verified | `0x006832C0`; assembly `0x006833E0..0x006833EE` | none |
| staging `DAT_00A8E960` reset call | verified | `0x0052F620`; assembly `0x0052F62B..0x0052F634` | none |
| `[SpecialFlags]` reader and mode gate | verified | `0x006B8CA0`; assembly `0x006B8E1F..0x006B8E39` | none |
| `[SpecialFlags]` writer body | verified | `0x006B8B30`; assembly `0x006B8B92..0x006B8BA0` | exact caller census deferred |
| `Read_INI_Basic` invokes reader | verified | `0x00689E90` | none |
| multiplayer clear of staging bit when bridge destruction off | verified | `0x00686B20`; assembly `0x0068794D..0x00687966` | none |
| multiplayer copy staging to active scenario flags | verified | `0x00686B20`; assembly `0x00687C16..0x00687C29` | none |
| `BridgeDestruction` rules parser | verified | `0x00671EA0`; `ini/rulesmd.ini:3029` | full UI writer chain to `DAT_00A8B260` not drained |
| AoE bridge consumer | touched-not-exhausted | `0x00489280`; prior report | bridge outcomes are other swarm slots |
| current Rust rules/map/parser surface | verified | `src/rules/ruleset.rs`; `src/map/basic.rs`; `src/sim/bridge_state/mod.rs`; `src/sim/world/bridge_orchestrator.rs` | implementation later |

## 8. Open Questions - Final State of Investigation Log

- `[RESOLVED] OQ-1 - What is the target bit and key? -> bit `0x8000` / shift `0xF`, key `DestroyableBridges` under `[SpecialFlags]`.` (evidence: `0x006B8CA0`, `0x006B8B30`, string `0x00840248`)
- `[RESOLVED] OQ-2 - What is the reset/default value? -> reset sets bit `0x8000` on via `| 0x8088`.` (evidence: `0x006B8AE0`, assembly `0x006B8AE0..0x006B8AEE`)
- `[RESOLVED] OQ-3 - Does the scenario constructor call reset? -> yes, with `ECX=this`.` (evidence: `0x006832C0`, assembly `0x006833E0..0x006833EE`)
- `[RESOLVED] OQ-4 - Does staging/session state also reset? -> yes, `FUN_0052F620` calls reset with `ECX=0x00A8E960`.` (evidence: `0x0052F620`, assembly `0x0052F62B..0x0052F634`)
- `[RESOLVED] OQ-5 - What happens when map key is absent? -> current bit is passed as `ReadBool` default, so reset/default survives.` (evidence: `0x006B8CA0`)
- `[RESOLVED] OQ-6 - Which modes read map `[SpecialFlags] DestroyableBridges`? -> campaign (`g_GameMode==0`) or map editor only.` (evidence: `0x006B8CA0`)
- `[RESOLVED] OQ-7 - Does skirmish/multiplayer read map `[SpecialFlags] DestroyableBridges`? -> no when editor is false; reader skips that block.` (evidence: `0x006B8CA0`)
- `[RESOLVED] OQ-8 - What overrides multiplayer active flags? -> session staging `DAT_00A8E960`, copied to active scenario flags late in load.` (evidence: `0x00686B20`, assembly `0x00687C16..0x00687C29`)
- `[RESOLVED] OQ-9 - What clears multiplayer bridge destruction when disabled? -> `DAT_00A8B260==0` clears bit `0x8000` in staging before the copy.` (evidence: `0x00686B20`, assembly `0x0068794D..0x00687966`)
- `[RESOLVED] OQ-10 - Where is `BridgeDestruction` default read? -> `[MultiplayerDialogSettings] BridgeDestruction` to `Rules+0x14AC`.` (evidence: `0x00671EA0`, `ini/rulesmd.ini:3029`)
- `[RESOLVED] OQ-11 - Is `[CombatDamage] DestroyableBridges` the binary rules key? -> no as a rules key; it is stock INI text without this parser binding.` (evidence: [WEAPON_AOE_BRIDGE_DAMAGE_ENTRY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/WEAPON_AOE_BRIDGE_DAMAGE_ENTRY_GHIDRA_REPORT.md), `ini/rulesmd.ini:804`)
- `[RESOLVED] OQ-12 - What writes `[SpecialFlags] DestroyableBridges`? -> `FUN_006B8B30` writes bit 15 under the same key.` (evidence: `0x006B8B30`, assembly `0x006B8B92..0x006B8BA0`)
- `[RESOLVED] OQ-13 - What is the gameplay consumer? -> `Apply_area_damage` tests active scenario bit `0x8000` before bridge tile damage.` (evidence: `0x00489280`, [WEAPON_AOE_BRIDGE_DAMAGE_ENTRY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/WEAPON_AOE_BRIDGE_DAMAGE_ENTRY_GHIDRA_REPORT.md))
- `[RESOLVED] OQ-14 - Does Rust parse the right owner? -> no; rules parser reads `[CombatDamage] DestroyableBridges`.` (evidence: `src/rules/ruleset.rs:754..757`)
- `[RESOLVED] OQ-15 - Does Rust already parse map `[SpecialFlags]`? -> yes, but application must respect mode ownership.` (evidence: `src/map/basic.rs:28..67`)
- `[DEFERRED] OQ-16 - Full map-save/editor caller census for `FUN_006B8B30`.` (category: bounded-cost-too-high; reason: writer body/bit mapping is verified and caller inventory does not affect runtime bridge-damage handoff; next-step-if-pursued: xref the writer in a save/load-focused investigation)
- `[DEFERRED] OQ-17 - Every UI writer that feeds `DAT_00A8B260`.` (category: requires-different-system-context; reason: this slice verified the scenario-load consumer and rules parser default; full lobby UI control flow belongs to skirmish UI/session research; next-step-if-pursued: follow `BridgeDestruction` checkbox and network packet writers)

Deferred items are not load-bearing for the Rust bridge-damage parser ownership fix.

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Reset/default sets `DestroyableBridges` on independently of `[CombatDamage] DestroyableBridges` | `0x006B8AE0`; constructor `0x006832C0`; staging reset `0x0052F620`; `ini/rulesmd.ini:804` non-reader from prior report | mismatch: Rust reads `[CombatDamage] DestroyableBridges` in `BridgeRules::from_ini` | `src/rules/ruleset.rs`, bridge runtime initialization | remove `[CombatDamage] DestroyableBridges` as an owner of `BridgeRules::destroyable_by_default`; default remains true from reset/session model | rules INI with `[CombatDamage] DestroyableBridges=no` still creates destroyable bridges in default/skirmish setup | proposed test `combatdamage_destroyablebridges_no_does_not_clear_default_bridge_flag`; risk: preserving stale decorative INI line as gameplay |
| Campaign/editor map `[SpecialFlags] DestroyableBridges` can override bit `0x8000`; skirmish/multiplayer map line cannot | reader `0x006B8CA0`; caller `0x00689E90`; MP copy `0x00687C16..0x00687C29` | partial/missing: map parser captures the key but no mode-aware application is visible in scanned surfaces | `src/map/basic.rs`, scenario/map load configuration, `BridgeRuntimeState` construction | apply map `SpecialFlagsSection.destroyable_bridges` only for campaign/editor-equivalent loads; skirmish should ignore map override and use session/default | same map with `[SpecialFlags] DestroyableBridges=no`: campaign load blocks AoE bridge damage, skirmish load leaves bridge damage enabled unless lobby/session disables it | proposed test `specialflags_destroyablebridges_map_override_campaign_only`; risk: making custom skirmish maps silently override lobby/session bridge destruction |
| Multiplayer/skirmish final active bit comes from session staging and `BridgeDestruction` can clear bit `0x8000` | `0x0068794D..0x00687966`, `0x00687C16..0x00687C29`, rules parser `0x00671EA0`, `ini/rulesmd.ini:3029` | likely missing/unchecked: no full session option ownership found in scanned bridge rules; current bridge flag appears rules-derived | future skirmish/session config surface plus `BridgeRuntimeState` construction | represent `BridgeDestruction` as a session/lobby option distinct from map `[SpecialFlags]` and rules `[CombatDamage]` | skirmish/session with BridgeDestruction off blocks weapon AoE bridge tile damage even if the map says `[SpecialFlags] DestroyableBridges=yes`; session on leaves default enabled | proposed test `skirmish_bridge_destruction_option_controls_specialflags_bit_8000`; risk: conflating `BridgeDestruction` with `[CombatDamage] DestroyableBridges` |

### Negative Facts / Do Not Do

- Do not read `[CombatDamage] DestroyableBridges` as the binary owner of the bridge-damage gate; Active in YR: No as rules key. Evidence: prior `RulesClass::ReadCombatDamage` audit and this reader/writer proof.
- Do not apply map `[SpecialFlags] DestroyableBridges` in normal skirmish/multiplayer when editor is false; Active in YR: No for that mode. Evidence: `0x006B8CA0` mode gate plus `Full_Init` staging overwrite.
- Do not delete `src/map/basic.rs` parsing of `[SpecialFlags] DestroyableBridges`; Active in YR: Yes for campaign/editor. Evidence: `0x006B8CA0`.
- Do not treat `[MultiplayerDialogSettings] BridgeDestruction` as the same INI key as `[SpecialFlags] DestroyableBridges`; Active in YR: both are active in different ownership layers. Evidence: `0x00671EA0` vs `0x006B8CA0`.
- Do not route C4/CABHUT collapse through this SpecialFlags gate; Active in YR: separate path per bridge-collapse reports. Evidence: [CABHUT_C4_COLLAPSE_ENTRY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/CABHUT_C4_COLLAPSE_ENTRY_GHIDRA_REPORT.md).

### Remaining Uncertainty

- Full caller census for the `[SpecialFlags]` writer `0x006B8B30` was deferred; the writer body and bit mapping are verified.
- Full UI/network writer chain to `DAT_00A8B260` was not drained; scenario-load semantics and rules default parser are verified.

### Stale Docs / Follow-up Docs

- `docs/research/DESTROYABLEBRIDGES_INI_GATE_GHIDRA_REPORT.md`: replace the Open Question "Runtime SpecialFlags constructor default..." with: "Closed by `SPECIALFLAGS_DESTROYABLEBRIDGES_DEFAULT_AND_MODES_GHIDRA_REPORT.md`: `FUN_006B8AE0` applies `flags = (flags & 0xFFF88088) | 0x8088`; constructor `0x006832C0` calls it for active scenario flags and init `0x0052F620` calls it for staging `DAT_00A8E960`, so bit `0x8000` defaults on."
- `docs/research/SPECIAL_FLAGS_SYSTEM.md`: replace the uncertain `DAT_00A8E960 uses a DIFFERENT bit layout` paragraph with: "For the bridge-destruction bit specifically, staging and active flags both use bit `0x8000`. `Full_Init` clears staging bit `0x8000` when `g_GameMode != 0 && DAT_00A8B260 == 0`, then later copies `DAT_00A8E960` into active `*g_ScenarioClass_Instance` for multiplayer. This report does not claim the other packed lobby bits."
- [docs/research/SCENARIO_INIT_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SCENARIO_INIT_DEEP_DIVE.md): replace wording that calls bit `0x8000` "superweapons flag" with: "`0x8000` is `DestroyableBridges`; in multiplayer, `DAT_00A8B260 == 0` clears this bridge-destruction bit in staging before staging is copied to active scenario flags."

## Sources

- Ghidra decompiled/read:
  - `0x006B8AE0` SpecialFlags reset/default writer.
  - `0x006B8CA0` `[SpecialFlags]` reader.
  - `0x006B8B30` `[SpecialFlags]` writer.
  - `0x006832C0` `ScenarioClass__Constructor`.
  - `0x0052F620` init/reset path for `DAT_00A8E960`.
  - `0x00689E90` `ScenarioClass__Read_INI_Basic`.
  - `0x00686B20` `ScenarioClass__Full_Init`.
  - `0x00671EA0` `RulesClass__ReadMultiplayerDialogSettings`.
  - `0x00489280` consumer spot-check via prior report.
- Read-only assembly contexts:
  - `0x006B8AE0..0x006B8AEE`
  - `0x006B8E1F..0x006B8E39`
  - `0x006B8B92..0x006B8BA0`
  - `0x006833E0..0x006833EE`
  - `0x0052F62B..0x0052F634`
  - `0x0068794D..0x00687966`
  - `0x00687C16..0x00687C29`
- Docs referenced:
  - `docs/research/WEAPON_AOE_BRIDGE_DAMAGE_ENTRY_GHIDRA_REPORT.md`
  - `docs/research/DESTROYABLEBRIDGES_INI_GATE_GHIDRA_REPORT.md`
  - `docs/research/SPECIAL_FLAGS_SYSTEM.md`
- INI checked:
  - `ini/rulesmd.ini:804`, `ini/rulesmd.ini:3029`
  - `ini/rules.ini:664`, `ini/rules.ini:2509`
- Rust scanned:
  - `src/rules/ruleset.rs`
  - `src/map/basic.rs`
  - `src/sim/bridge_state/mod.rs`
  - `src/sim/world/bridge_orchestrator.rs`
