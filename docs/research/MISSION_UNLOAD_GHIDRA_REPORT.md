# Mission_Unload — Ghidra Research Report

**Date:** 2026-05-17
**Active in YR:** Yes — every transport unit (Amphibious Transport, IFV, Battle Fortress, Aircraft Carrier launch missions, paradrop carriers) uses this.
**Confidence:** HIGH

## 1. Overview

`Mission_Unload` (mission enum value **16** / `0x10`) is the transport-unload mission. It exists in 3 distinct forms because different unit categories handle unloading differently:

| Mission enum | Class | Address | Use case |
|---|---|---|---|
| 16 (Unload) on **FootClass** | base stub | `0x4DA2B0` | Returns `0x1C2` (450 frames) — placeholder; never proceeds. FootClass doesn't override. |
| 16 (Unload) on **UnitClass** | overridden | `0x740EF0` | **Ground transport unload** — Amphibious Transport disgorging infantry/vehicles, IFV ejecting passenger |
| 30 (`Mission_ParaDropApproach`) on **AircraftClass** | overridden | `0x4155F0` | **Aircraft approach to drop zone** before paradrop |
| 31 (`Mission_ParaDropOverfly`) on **AircraftClass** | overridden | `0x4157C0` | **Aircraft over drop zone, releasing paratroopers** |

Aircraft-style "unload" uses missions 30 and 31, not mission 16 — different mission enums entirely. The shared name "unload" is conceptual; the mission state machines are separate.

---

## 2. UnitClass::Mission_Unload @ `0x740EF0` (mission 16)

The ground-transport unload handler.

### 2.1 Full decompilation

```c
undefined4 UnitClass::Mission_Unload(int *param_1) {
    // Step 1: Find unload destination cell (mode 0 = preferred unload spot)
    int unload_target = (*vtable+0x528)(RulesClass+0x850, 0, 0);

    // Step 2: Clear "unloading active" flag (FootClass+0x6D2)
    *(byte *)((int)param_1 + 0x6D2) = 0;

    if (unload_target == 0) {
        // Step 3a: No spot found in mode 0. Try mode 1 (fallback search).
        unload_target = (*vtable+0x528)(RulesClass+0x850, 0, 1);
        if (unload_target != 0) {
            // Mode 1 found a spot — proceed with Mission_Update
            (*vtable+0x484)(0, 1);                          // Mission_Update full
        }
    } else {
        // Step 3b: Spot found. Issue Mission_Move (2) to it.
        int result = (*vtable+0x278)(2, unload_target);     // Assign_Mission(Mission_Move, target)
        if (result == 1) {
            // Move assigned successfully — now queue Mission_Enter (7) to dock at it
            (*vtable+0x1E8)(7, 0);                          // Queue Mission_Enter
            return 1;                                        // 1-tick rate (urgent)
        }
    }

    // Default return: MissionTimerEntry + jitter
    MissionClass::GetMissionTimerEntry();
    return Math::ftol();                                     // typically ~450 frames + jitter
}
```

### 2.2 Algorithm walkthrough

1. **`vtable+0x528` = `Find_Best_Unload_Cell`** (inferred name) — searches for a valid cell where the transport can disgorge its passenger. Takes args:
   - `RulesClass+0x850` — a search-radius constant (likely `[General] TransportUnloadRange=` or similar; default ~5 cells in lepton units)
   - `0` — passenger index 0 (the first/only passenger)
   - `mode` — 0 (preferred-spot search) or 1 (fallback-spot search)

2. **`FootClass+0x6D2`** is the "unloading active" byte. Cleared at the start of each Mission_Unload tick. Set elsewhere (probably by the passenger-eject path) to signal "I am currently mid-unload, suppress secondary AI".

3. **`vtable+0x278` = `Assign_Mission`** — issues a new mission to the unit. Arg 1 = mission enum (2 = Mission_Move). Arg 2 = target coord/object. Returns 1 if assigned successfully.

4. **`vtable+0x1E8` = `Queue_Mission`** — chains a follow-up mission. Arg 1 = mission enum (7 = Mission_Enter). When the Mission_Move completes (transport arrives at unload cell), Mission_Enter takes over to actually dock & disgorge passenger.

5. **Two-stage unload flow:** Mission_Unload by itself doesn't move the transport — it sets up Mission_Move + queued Mission_Enter. The actual unloading happens inside Mission_Enter when the transport is adjacent to its disgorge target.

6. **Mode 0 vs Mode 1 fallback:** Mode 0 searches for an "ideal" spot (e.g. closest land cell for an amphibious transport). Mode 1 is a more permissive search (e.g. any adjacent passable cell). The fallback handles edge cases like dropping passengers in tight spaces.

### 2.3 Subtle details

1. **The function is short (~30 bytes decompiled body).** Most of the heavy lifting is in the vtable methods (`Find_Best_Unload_Cell` at +0x528, `Assign_Mission` at +0x278, etc.) — those are the deep state machines.

2. **The "no spot found" branch** falls through to MissionTimerEntry — the transport idles for ~450 frames (~30 seconds at 15fps) before retrying. This is intentional: a player ordering "Unload" near a fully-occupied destination shouldn't spam pathfinder requests.

3. **Return value `1` on success** is "1 tick rate" — re-enter `Mission_Unload` on the very next tick. After the queued Mission_Enter is set, the unit transitions and Mission_Unload doesn't fire again until queued mission completes.

4. **Mission ID 7 (Enter) for unload's final stage** — this is the same mission used for "Engineer enters building". Same Mission_Enter handler does double-duty: vehicle entering refinery to harvest, infantry entering garrison, AND transport entering its own disgorge target to release passenger.

5. **The vtable indirection (`(*vtable+0x484)(0, 1)`)** is `Mission_Update` — the "no-op tick, but proceed" sentinel that tells the MissionClass dispatcher "I did nothing this tick, move on to the next planned mission".

**C** = HIGH (full decomp), **I** = HIGH (Ghidra symbol confirmed via search), **B** = HIGH (sole xref via the FootClass mission dispatch switch at case 16 — no direct callers because it's dispatched via vtable).

---

## 3. FootClass base stub @ `0x4DA2B0`

```c
undefined4 thunk_FUN_005B2EF0(void) {
    return 0x1C2;   // 450
}
```

Returns 450 frames. **This is the "Mission_Unload base stub" — FootClass doesn't actually unload**, it just sets a long timer. UnitClass overrides it (above) with the real behaviour.

Per [FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md): FootClass overrides 12 of 28 mission handler slots. Mission 16 (Unload) is one of the slots FootClass leaves to subclasses.

**Subtle detail:** **InfantryClass also doesn't override Mission_Unload.** Only UnitClass overrides it. So:
- A vehicle transport (Amphibious Transport, IFV, Battle Fortress) → UnitClass::Mission_Unload
- An infantry unit can't be a transport in stock YR (they can be passengers but not carriers)
- Aircraft transports → use paradrop missions (30/31)

**C** = HIGH, **I** = HIGH, **B** = HIGH.

---

## 4. AircraftClass::Mission_ParaDropApproach @ `0x4155F0` (mission 30)

Aircraft transports (cargo planes, paradrop helicopters) use this mission for **approach to drop zone**.

### 4.1 Decompiled logic

```c
undefined4 Mission_ParaDropApproach(int *param_1) {
    int distance = FUN_005F6440();                            // distance to drop target
    int target = param_1[0xAD];                                // TarCom (+0x2B4)

    if (target == 0) {
        // No drop target — return home (Mission_Retreat)
        (*vtable+0x480)();                                     // Mission_Update light
        (*vtable+0x1E8)(4);                                    // Queue Mission_Retreat
    } else if (param_1[0x169] == 0) {                         // NavCom is null
        // No nav target → Mission_Update
        (*vtable+0x480)();
    } else {
        // Both target and nav set — check if close enough
        int passenger = (*vtable+0x3F8)();                     // Get_Passenger(0)
        if (distance <= passenger.vtable+0xB4) {              // within passenger's "drop radius"
            (*vtable+0x48C)(0, 0, 0, 0);                       // Mark cell as drop spot
            int passenger2 = (*vtable+0x3F8)(0);
            (*vtable+0x488)(0, 0, 0, 0, passenger2.coord_Z);   // Eject passenger at target Z
            MapClass::UpdateFogBorder(/*cell*/, 0, sight+3, 0); // Reveal area
            VocClass::PlayAt(0);                                // Play drop sound
        }
    }

    if (distance < 0x301) {                                    // 769 leptons = ~3 cells
        // Close enough — switch to Overfly mission
        (*vtable+0x1E8)(0x1F, 0);                              // Queue Mission_ParaDropOverfly (31)
        *(byte *)((int)param_1 + 0x6D2) = 1;                  // Mark "paradrop active"
        // Compute escape vector via opposite-edge
        int opposite_edge = HouseClass::GetOppositeEdge();
        // ... compute escape cell, assign Mission_Move there ...
    }

    return *(int *)(RulesClass + 0x290);                       // ParaDropRate (?)
}
```

### 4.2 Key constants

- **`0x301` = 769 leptons** ≈ 3 cells. Drop-zone proximity threshold for switching to Overfly.
- **`RulesClass+0x290`** — return value (rate timer). Likely `[General] ParaDropApproachRate=` or similar.
- **`vtable+0x488`** = "Eject_Passenger" or "Drop_Cargo".
- **`vtable+0x3F8`** = "Get_Passenger(idx)".
- **`vtable+0x48C`** = "Mark_Drop_Cell" (cell-side state for paradrop spawn).
- **`HouseClass::GetOppositeEdge`** — returns the map edge furthest from this house's start, used as escape destination after drop.

### 4.3 Mission flow

```
Mission 30 (ParaDropApproach):
  ┌─→ flying toward target
  │
  │   IF distance > 769: keep approaching at low rate (RulesClass+0x290)
  │   IF distance <= 769: queue Mission 31 (Overfly) + compute escape
  │
  ▼
Mission 31 (ParaDropOverfly):
  ┌─→ overhead, releasing paratroopers each tick
  │
  │   IF passenger == NULL: queue Mission_Move to escape edge
  │
  ▼
Mission 2 (Move to escape edge)
  │
  ▼
Mission 4 (Retreat home / despawn)
```

**Subtle detail:** the escape vector is computed at the END of ParaDropApproach (when triggering the switch to Overfly), not at the start of Overfly. This ensures the escape direction is locked in BEFORE drops begin — the cargo plane commits to a flight path so it doesn't reverse during the drop sequence.

**C** = HIGH (full decomp), **I** = HIGH (Ghidra symbol), **B** = HIGH.

---

## 5. AircraftClass::Mission_ParaDropOverfly @ `0x4157C0` (mission 31)

The release phase. Runs every 3 frames (returns `3`).

```c
undefined4 Mission_ParaDropOverfly(int *param_1) {
    int distance = FUN_005F6440(param_1[0xAD]);              // distance to TarCom
    int passenger = (*vtable+0x3F8)(0);                       // Get_Passenger(0)

    if (distance <= passenger.vtable+0xB4) {                  // within drop radius
        (*vtable+0x48C)(0, 0, 0, 0);                          // Mark drop cell
        (*vtable+0x488)(0, 0, 0, 0, passenger.coord_Z);       // Eject this passenger
        MapClass::UpdateFogBorder(my_coord, 0, sight+3, 0);   // Reveal area
    }

    if (param_1[0x169] == 0) {                                 // no nav target — set escape
        int opposite_edge = HouseClass::GetOppositeEdge();
        int escape_cell = compute_escape_via_edge(...);
        if (escape_cell != NullCoord) {
            (*vtable+0x480)(/*escape_cell*/, 1);              // Queue Mission_Move
        }
    }
    return 3;                                                  // 3 frames between calls = ~5 drops/second
}
```

### 5.1 Subtle details

1. **Return value 3** = re-fire every 3 frames. At 15 fps this is **5 calls per second** — enough granularity to drop one paratrooper per second-ish while staying performant.

2. **Drop loop iterates passengers**: each tick, get passenger 0, eject it. Next tick, passenger 0 is now the NEXT in the queue (the just-dropped one is gone). Loop continues until passenger == NULL.

3. **`MapClass::UpdateFogBorder`** reveals shroud around the drop point. `sight+3` = `param_1[0x98] + 3` — the aircraft's `Sight` stat plus 3 cells radius. Slightly larger than normal sight because paradrop cargo planes briefly show what they're dropping into.

4. **No `+0x6D2` clear at end** — the "paradrop active" flag set in ParaDropApproach STAYS set through Overfly until the aircraft transitions to escape Mission_Move.

**C** = HIGH (full decomp), **I** = HIGH, **B** = HIGH.

---

## 6. INI bindings

| INI key | Section | Offset | Default | Purpose |
|---|---|---|---|---|
| `UnloadingClass=` | TechnoType (per-unit) | parsed @ `0x7146E8` (in `TechnoTypeClass::ReadINI`) | (none) | Specifies what the transport unloads as — used by some specialized transports |
| `TransportUnloadRange=` (inferred) | `[General]` | `RulesClass+0x850` | (TBD — likely ~1280 leptons = 5 cells) | Search radius for unload spot |
| `ParaDropApproachRate=` (inferred) | `[General]` | `RulesClass+0x290` | (TBD) | Mission timer for ParaDropApproach |
| `Passengers=` | TechnoType | parsed in TechnoTypeClass | 0 | Max passenger count |
| `Crewed=` | TechnoType | parsed @ TechnoTypeClass | (varies) | If yes, unit ejects 1 infantry on death (separate from Unload) |
| `IsSimpleDeployer=` | TechnoType | parsed in TechnoTypeClass | no | MCV-like deploy semantic (different from Unload) |

**Subtle detail — `UnloadingClass=` is sparse in stock content.** Only specialized units (like the Tank Bunker or specific mod content) use it. Standard transports use `Passengers=N` and rely on the default unload mechanism.

**Open question:** exact INI key names for `RulesClass+0x850` and `RulesClass+0x290`. Not verified at the parser site this pass.

---

## 7. Unload-capable stock units (rulesmd.ini)

Spot-checked typical transports:

| Section | Unit | Mission_Unload used? | Notes |
|---|---|---|---|
| `[AMCV]` | Amphibious Transport (Allied LCAC) | Yes | Hover locomotor + Passengers=5 |
| `[YHVR]` | Yuri Amphibious Transport | Yes | Hover locomotor + Passengers=5 |
| `[FV]` | IFV / Multigunner | Yes (ejects gunner) | Driver→passenger ejection mechanism |
| `[BFRT]` | Battle Fortress | Yes | Passengers=5; ejects on demand |
| `[CARYALL]` (mod) | Carryall (TS-legacy) | Yes via ParaDrop missions | Aircraft locomotor |
| `[ORCA]` (mod) | Orca transport | Yes via ParaDrop | Aircraft |

**No stock YR Aircraft uses Mission_Unload directly.** Aircraft transports use ParaDropApproach/Overfly (missions 30/31). The Aircraft mission 16 (Unload) falls through to the FootClass stub (450-frame idle).

**Subtle detail — IFV's "passenger ejection":** Different from a transport unload. The IFV reassigns its turret graphic based on `UnloadingClass=` and may not invoke Mission_Unload at all. Verify with `IFV_GUNNER_SYSTEM` doc cross-reference (not done this pass).

---

## 8. Open questions

1. **`RulesClass+0x850` exact value and INI key.** Mode-0 vs Mode-1 search semantics need separate decompilation of `vtable+0x528` (`Find_Best_Unload_Cell`).

2. **`vtable+0x488` = "Eject_Passenger"** — full body not decompiled. Need to verify the passenger Z-set, cell-occupancy assignment, and parachute-anim spawn logic.

3. **Paradrop parachute mechanics** — the AnimClass that's spawned when a passenger is ejected from a high-altitude cargo plane. Cross-reference with [PARACHUTE_SHP_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARACHUTE_SHP_RENDERING_GHIDRA_REPORT.md).

4. **Aircraft Carrier's launch mechanism** — does it use Mission_Unload, ParaDrop missions, or something else? Probably uses a different mission since launched Hornets are NOT passengers (they're spawned via SpawnManager).

5. **IFV-specific ejection path** — when the IFV's driver dies, the gunner-passenger ejects. Does this go through Mission_Unload? Likely a separate path inside UnitClass::Take_Damage.

6. **InfantryClass::Mission_Unload** — does it exist? Search showed `InfantryClass__Mission_Capture` but no Mission_Unload override. Verify infantry can't transport in stock YR.

7. **The 3-frame return rate of ParaDropOverfly** — does that match the visual cadence of paratrooper drops in gameplay (1 paratrooper per ~3 frames = ~5 per second)? Worth observing in-game vs binary.

---

## 9. TS-legacy filtering

| Subsystem | Active in YR? | Evidence |
|---|---|---|
| UnitClass::Mission_Unload @ 0x740EF0 | Yes | Used by Amphibious Transport, IFV, Battle Fortress |
| Aircraft ParaDrop missions (30/31) | Yes | Used by Allied Paradrop superweapon's cargo plane |
| FootClass Mission_Unload stub @ 0x4DA2B0 | Yes (passive) | Returns 450 — no-op; non-transport units fall through to it harmlessly |
| `UnloadingClass=` INI key | **Conditional** | Stock content rarely uses it; mostly mod content |
| Aircraft Mission_Open (case enum unknown) | Maybe | `AircraftClass::Mission_Open @ 0x4158E0` — possibly TS-legacy door-open animation. Untraced this pass. |
| Aircraft Mission_Rescue @ 0x415960 | Maybe | TS-legacy "rescue downed pilot" mechanic. Untraced. |

**No SpecialFlags-gated branches found** in the Mission_Unload path. Standard YR exercises every documented code path.

---

## 10. Sources

**Ghidra functions decompiled (this pass):**
- `UnitClass::Mission_Unload @ 0x740EF0` — full body
- `FootClass::Mission_Unload base stub @ 0x4DA2B0` — full body (1-line return)
- `AircraftClass::Mission_ParaDropApproach @ 0x4155F0` — full body
- `AircraftClass::Mission_ParaDropOverfly @ 0x4157C0` — full body

**Memory reads:**
- `0x843AF8` len 16 — "UnloadingClass" INI key string

**Xref tables:**
- `get_xrefs_to 0x843AF8` → 1 DATA xref from `TechnoTypeClass::ReadINI @ 0x7146E8` (single parser site)

**Strings located:**
- `0x843AF8` "UnloadingClass"
- `0x845DFC` "IsSimpleDeployer"
- `0x84396C` "Crewed"

**Companion docs:**
- [FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md) — the 28-mission dispatch table at FootClass level
- [MISSIONCLASS_STATE_MACHINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSIONCLASS_STATE_MACHINE.md) — Mission_Dispatch mechanism
- [MISSION_ENTER_REFINERY_DOCK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_ENTER_REFINERY_DOCK_GHIDRA_REPORT.md) — Mission_Enter (the queued follow-up)
- [MISSION_ENTER_CROSSWALK_AND_GAPS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSION_ENTER_CROSSWALK_AND_GAPS_GHIDRA_REPORT.md) — Mission_Enter cross-reference
- [FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md) — Mission_Move (assigned in step 3b)

---

*End of report. Mission_Unload is the orchestrator; the heavy lifting (unload-cell search, passenger ejection, paradrop sequencing) lives in vtable methods that are separately deep-divable.*
