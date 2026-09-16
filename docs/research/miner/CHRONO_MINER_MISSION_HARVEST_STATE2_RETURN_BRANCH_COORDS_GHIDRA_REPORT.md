# Chrono Miner Mission_Harvest State 2 Return Branch Coordinates - Ghidra Research Report

**Address(es):** `0x0073E5E0` primary (`UnitClass__Mission_Harvest`), `0x004DF040` (`FootClass__Find_Docking_Bay`), `0x0043C2D0` (`BuildingClass__Receive_Radio`), `0x00741970` (`UnitClass/Techno Set_Destination override`), `0x00447B20` (`BuildingClass__GetDockCoord`)  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** `Mission_Harvest` state 2 return branch for standard YR `CMIN`: distance coordinate inputs, inclusive close/far threshold, and object/cell passed to the next radio/movement step.  
**Non-Scope:** dock-arrival timing, unloading cadence, post-unload exit, full `Set_Destination` semantics, multi-dock aircraft/repair bay behavior.  
**Confidence:** High  
**Active in YR:** Yes. Standard `[CMIN]` has `Harvester=yes`, `Teleporter=yes`, and `Dock=NAREFN,GAREFN`; standard `[GAREFN]` has `DockUnload=yes`, `Refinery=yes`, and `NumberOfDocks=1`.

## 1. Overview

`UnitClass__Mission_Harvest` state 2 first tries to find a dockable building and decide whether the harvester is close enough to reserve it directly. For `CMIN`, the distance check is from the miner object coordinate to the refinery object coordinate, which maps to the refinery origin/top-left anchor in cell terms. It is not measured to the refinery center, the accepted dock pad, or `QueueingCell`.

If the chrono miner is within the inclusive `ChronoHarvTooFarDistance * 0x100` threshold, state 2 radios the refinery object with message `2` and moves to substate 3 on acceptance. If it is farther, or the normal reservation path does not fire, the fallback path computes a `QueueingCell`-based seed cell and hands a nearby passable `CellClass*` to `Set_Destination`.

## 2. Class Layout / Key Offsets

| Offset | Owner | Meaning in this slice | Evidence | Active in YR |
|---:|---|---|---|---|
| `+0xBC` (`param_1[0x2f]`) | Unit | harvest substate; `2` is return-to-refinery, `3` queues `Mission_Enter` | `UnitClass__Mission_Harvest @ 0x0073E5E0` switch | Yes; gated by `Harvester=yes` |
| `+0x5A4` (`param_1[0x169]`) | Foot/Techno | current destination pointer; state 2 only selects a new return target when zero | `0x0073E5E0` state 2 | Yes |
| `+0x6C4` (`param_1[0x1b1]`) | Techno | type pointer | `0x0073E5E0` entry | Yes |
| `+0xCD4` | TechnoType | `Teleporter=yes`; nonzero selects chrono branch | `0x0073E5E0`, `[CMIN] Teleporter=yes` at `ini/rulesmd.ini:7396` | Yes |
| `+0xE0E` | TechnoType | `Harvester=yes` | `0x0073E5E0`, `[CMIN] Harvester=yes` at `ini/rulesmd.ini:7364` | Yes |
| `+0x3F8` | TechnoType | `Dock=` vector count | `0x0073E5E0`, `[CMIN] Dock=NAREFN,GAREFN` at `ini/rulesmd.ini:7361` | Yes |
| `+0xD7C` | RulesClass | `ChronoHarvTooFarDistance` in cells | `0x0073E5E0`; `ini/rulesmd.ini:294` | Yes |
| `+0x1618/+0x161C` | BuildingType | `QueueingCell` X/Y offsets used only in state-2 fallback destination seed | `0x0073E5E0`; read from art per prior `NUMBEROFDOCKS...`; `ini/artmd.ini:1773` | Yes, conditional on fallback |

## 3. Core Logic

### State-2 close/far decision

Verified binary behavior, active in YR: Yes.

1. State 2 calls the unit vtable slot `+0x528`, resolved in prior docs and rechecked as `FootClass__Find_Docking_Bay @ 0x004DF040`, with the unit type dock list (`Type + 1000` / dock vector area), `arg3=0`, `arg4=0`.
2. `Find_Docking_Bay` iterates the dock list and returns a dockable object pointer. For standard CMIN/GAREFN this is the refinery `BuildingClass*`.
3. If `Teleporter=yes` (`Type+0xCD4 != 0`) and a refinery was found, state 2 calls:
   - `refinery->vtable+0x48` for the refinery object coordinate,
   - `unit->vtable+0x48` for the CMIN object coordinate.
4. It computes `floor(sqrt(dx*dx + dy*dy + dz*dz))` in leptons.
5. The chrono branch compares with `RulesClass+0xD7C * 0x100`.
6. The comparison is inclusive: `distance <= ChronoHarvTooFarDistance * 256`.

For stock YR, `ChronoHarvTooFarDistance=50`, so the inclusive close threshold is `12800` leptons. This is about 50 cells in the same coordinate unit used by object coordinates.

The refinery coordinate input is the refinery object coordinate. In the same function, fallback code converts `piVar3[0x27]` and `piVar3[0x28]` to a cell by signed `+255 >> 8`, yielding the building origin/top-left anchor used for `QueueingCell` addition. The close/far branch does not call `GetDockCoord`, does not call `Receive_Radio 0x0E`, and does not read `QueueingCell`.

### Close branch output

Verified binary behavior, active in YR: Yes.

When `distance <= Rules+0xD7C * 0x100`, state 2 calls the unit radio vtable slot `+0x278` with:

```text
message = 2
target = refinery BuildingClass*
```

If the radio call returns `1`, `Mission_Harvest` writes substate `3`. Substate 3 assigns mission `7` (`Mission_Enter`) on the next state execution. Therefore, the immediate state-2 output is not a cell; it is a radio message to the refinery object.

The later accepted dock movement target comes from the radio/Mission_Enter path. `BuildingClass__Receive_Radio @ 0x0043C2D0` case `0x0E` for `DockUnload`/refinery constructs a cell from `building_anchor + (3,1)` and returns a `CellClass*` through the radio parameter. For retail `GAREFN` at origin `(rx, ry)`, this is `(rx+3, ry+1)`, matching the visible open pad cell (`RemoveOccupy1=3,1` at `ini/artmd.ini:1795`), not `QueueingCell=(4,1)`.

### Far/fallback branch output

Verified binary behavior, active in YR: Yes, conditional. It triggers when the normal close reservation branch does not fire: too far, no normal dock, or radio acceptance failure path continuing into fallback.

1. State 2 increments `g_MapEditorMode`, then calls `Find_Docking_Bay` again with `arg3=1`, then decrements `g_MapEditorMode`.
2. If a dock object is found, it recomputes distance from unit object coordinate to refinery object coordinate.
3. It enters the destination fallback if `distance > 0x300 || Teleporter=yes`. For `CMIN`, `Teleporter=yes` makes this condition true whenever the fallback has a found refinery.
4. It converts the refinery object coordinate to a building anchor cell.
5. It computes the seed cell:

```text
seed.x = building_anchor.x + *(short *)(BuildingType + 0x1618)
seed.y = building_anchor.y + *(short *)(BuildingType + 0x161C)
```

6. `+0x1618/+0x161C` are `QueueingCell` values. For retail `GAREFN`, `QueueingCell=4,1`, so the seed is `(rx+4, ry+1)`.
7. It calls `FootClass__Find_Nearby_Passable_Cell` around that seed.
8. If no passable cell is found, it calls the unit destination vtable slot `+0x480` with null/clear destination. Otherwise it calls `MapClass__Get_CellClass` for the passable cell and hands that `CellClass*` to vtable `+0x480` (`Set_Destination`).

This fallback cell is not the close-branch accepted dock pad. For stock `GAREFN`, fallback seed `(rx+4, ry+1)` differs from accepted CAN_DOCK target `(rx+3, ry+1)`.

## 4. INI Keys

| Key | Stock value | Effect in this slice | Evidence | Active in YR |
|---|---:|---|---|---|
| `[General] ChronoHarvTooFarDistance` | `50` | Inclusive close threshold for teleporter harvesters: `50 * 256` leptons | `ini/rulesmd.ini:294`; `0x0073E5E0` reads `Rules+0xD7C` | Yes |
| `[General] HarvesterTooFarDistance` | `5` | Non-teleporter harvester threshold; compared through same formula with `Rules+0xD78` | `ini/rulesmd.ini:293`; `0x0073E5E0` | Yes for HARV, not selected for CMIN |
| `[CMIN] Harvester` | `yes` | Enables `Mission_Harvest` harvester path | `ini/rulesmd.ini:7364`; `0x0073E5E0` reads `Type+0xE0E` | Yes |
| `[CMIN] Teleporter` | `yes` | Selects chrono threshold and fallback behavior | `ini/rulesmd.ini:7396`; `0x0073E5E0` reads `Type+0xCD4` | Yes |
| `[CMIN] Dock` | `NAREFN,GAREFN` | Dock list searched by `Find_Docking_Bay` | `ini/rulesmd.ini:7361`; `0x004DF040` iterates list | Yes |
| `[GAREFN] DockUnload` | `yes` | Makes refinery CAN_DOCK case produce dock cell | `ini/rulesmd.ini:11726`; `0x0043C2D0` checks `Type+0x16B3`/refinery path | Yes |
| `[GAREFN] Refinery` | `yes` | Refinery identity for harvester/dock behavior | `ini/rulesmd.ini:11727`; `0x0043C2D0` refinery flags | Yes |
| `[GAREFN] NumberOfDocks` | `1` | Stock refinery dock count; not used for state-2 close/far distance | `ini/rulesmd.ini:11729` | Yes, but not the branch coordinate |
| `[GAREFN] QueueingCell` | `4,1` | Fallback seed only after close/far path fails | `ini/artmd.ini:1773`; `0x0073E5E0` reads `BuildingType+0x1618/+0x161C` | Conditional |
| `[GAREFN] RemoveOccupy1` | `3,1` | Opens the visible foundation dock pad matching CAN_DOCK `(rx+3,ry+1)` | `ini/artmd.ini:1795`; radio path at `0x0043C2D0` uses `+3,+1` | Yes |

## 5. Integration Points

| Function | Role in this slice | Evidence | Active in YR |
|---|---|---|---|
| `UnitClass__Mission_Harvest @ 0x0073E5E0` | Owns state 2 return branch, distance compare, and close/fallback dispatch | Direct decompile | Yes |
| `FootClass__Find_Docking_Bay @ 0x004DF040` | Iterates `Dock=` list and returns best dock object | Direct decompile | Yes |
| unit vtable `+0x278` radio transmit | Close branch sends message `2` to refinery object | Direct state-2 call in `0x0073E5E0`; prior radio reports bind slot | Yes |
| `BuildingClass__Receive_Radio @ 0x0043C2D0` | Later CAN_DOCK case creates accepted dock target cell `anchor+(3,1)` | Direct decompile case `0x0E` | Yes |
| unit vtable `+0x480` / `Set_Destination` | Fallback branch receives null or `CellClass*` from passable-cell search | Direct state-2 call in `0x0073E5E0`; `0x00741970` destination path | Yes |
| `BuildingClass__GetDockCoord @ 0x00447B20` | Not used by close/far decision; relevant later dock-coordinate systems | Direct decompile; no call in state-2 distance branch | Conditional elsewhere, not this branch |

## 6. Current Rust Implementation Status

Scanned for comparison only; no code was changed.

| Area | Current Rust reference | Status vs this slice |
|---|---|---|
| Rule parsing | `src/rules/ruleset.rs:957-958` parses `HarvesterTooFarDistance` and `ChronoHarvTooFarDistance` | Present |
| Far-return comment/path | `src/sim/miner/miner_system.rs:824-852` describes chrono return staging as `QueueingCell` passable-cell search | Directionally matches fallback, but this report only verified binary behavior |
| Dock/accepted anchor | `src/sim/miner/miner_dock_sequence.rs:86-89` hardcodes accepted CAN_DOCK queue cell `(rx+3,ry+1)` | Matches accepted radio target, not fallback `QueueingCell` |
| QueueingCell parsing | `src/rules/art_data.rs:324-327`; merge at `src/rules/ruleset.rs:1755-1757` | Present |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `Mission_Harvest` state 2 close/far coordinate inputs | verified | `0x0073E5E0` direct decompile | none for this slice |
| Chrono threshold constant and inclusivity | verified | `0x0073E5E0`, `Rules+0xD7C`, `ini/rulesmd.ini:294` | none |
| Close branch output target | verified | `0x0073E5E0` radio call `message=2,target=piVar3` | none |
| State 3 next step | verified | `0x0073E5E0` state 3 assigns mission `7` | detailed Mission_Enter timing out-of-scope |
| Accepted CAN_DOCK cell | verified for reconciliation | `0x0043C2D0` case `0x0E`, `ini/artmd.ini:1795` | arrival/link timing out-of-scope |
| Far fallback seed and destination handoff | verified | `0x0073E5E0` fallback reads `+0x1618/+0x161C`, calls nearby passable search, then vtable `+0x480` | exact search ordering/radius out-of-scope |
| `QueueingCell` vs `DockingOffset%d` | verified by prior report and spot-check | [NUMBEROFDOCKS_VS_DOCKOFFSET_RECONCILE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/NUMBEROFDOCKS_VS_DOCKOFFSET_RECONCILE_GHIDRA_REPORT.md); no `+0x1788` read in state 2 | none for this slice |
| `BuildingClass__GetDockCoord` | touched-not-exhausted | `0x00447B20` direct decompile | broader dock coordinate consumers out-of-scope |
| TS legacy gate | verified | Standard YR INI plus live branches in `0x0073E5E0`/`0x0043C2D0` | none |

## 8. Open Questions - Final State

[RESOLVED] OQ-1 - What coordinates feed the state-2 CMIN distance compare? Unit object coordinate and refinery object coordinate via each object's vtable `+0x48`; evidence `0x0073E5E0`.

[RESOLVED] OQ-2 - Is the refinery coordinate center, pad, QueueingCell, or object/top-left anchor? It is the refinery object coordinate; in cell terms the same function converts the refinery object fields to the building origin/top-left anchor. It is not center, pad, CAN_DOCK cell, or QueueingCell; evidence `0x0073E5E0`.

[RESOLVED] OQ-3 - What threshold and comparison operator does CMIN use? Inclusive `distance <= Rules+0xD7C * 0x100`; stock value `50 * 256 = 12800` leptons; evidence `0x0073E5E0`, `ini/rulesmd.ini:294`.

[RESOLVED] OQ-4 - What does the close branch hand to the next step? Radio message `2` with the refinery `BuildingClass*`; on return `1`, state becomes `3`; evidence `0x0073E5E0`.

[RESOLVED] OQ-5 - What cell does the later accepted refinery path use? `Receive_Radio 0x0E` returns `CellClass*` for `building_anchor + (3,1)`; evidence `0x0043C2D0`, `ini/artmd.ini:1795`.

[RESOLVED] OQ-6 - What does fallback hand to movement? A `CellClass*` for a nearby passable cell seeded by `building_anchor + QueueingCell`; or null if no passable cell was found; evidence `0x0073E5E0`.

[RESOLVED] OQ-7 - Is `QueueingCell` used in the close/far decision? No. It is read only after fallback has been selected; evidence `0x0073E5E0`.

[RESOLVED] OQ-8 - Is this active in standard YR, not TS-only legacy? Yes. `CMIN` and `GAREFN` stock YR flags reach the branch; evidence `ini/rulesmd.ini:7361,7364,7396,11726,11727` and live Ghidra branch conditions.

[DEFERRED] OQ-9 - Exact `Find_Nearby_Passable_Cell` search order/radius for fallback destination. Category: out-of-scope; this slot only needed the seed and handoff target class.

## Sources

- Ghidra read-only decompile: `UnitClass__Mission_Harvest @ 0x0073E5E0`
- Ghidra read-only decompile: `FootClass__Find_Docking_Bay @ 0x004DF040`
- Ghidra read-only decompile: `BuildingClass__Receive_Radio @ 0x0043C2D0`
- Ghidra read-only decompile: `TechnoClass__Set_Destination` / unit override path @ `0x00741970`
- Ghidra read-only decompile: `BuildingClass__GetDockCoord @ 0x00447B20`
- `docs/research/CHRONO_MINER_TELEPORT_GHIDRA_REPORT.md`
- `docs/research/NUMBEROFDOCKS_VS_DOCKOFFSET_RECONCILE_GHIDRA_REPORT.md`
- `docs/research/traces/chrono_miner_full_cargo_return_teleport_to_refinery_pad_TRACE.md`
- `docs/research/traces/chrono_miner_forced_return_unload_command_TRACE.md`
- `docs/research/traces/MINER_FSM_FULL_CARGO_RETURN_RESERVE_REFINERY_TRACE.md`
- `docs/research/traces/chrono_miner_return_state_anchor_reserved_refinery_TRACE.md`
- `ini/rulesmd.ini`
- `ini/artmd.ini`
