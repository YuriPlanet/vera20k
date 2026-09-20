---
name: RadarEventClass Research Report
description: Event queue that drives minimap pulsing diamonds — type table, callers, lifecycle, and INI-vs-binary reconciliation. Companion to [RADAR_MINIMAP_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADAR_MINIMAP_DEEP_DIVE.md) and [RADAR_MINIMAP_RENDERING.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/RADAR_MINIMAP_RENDERING.md).
---

# RadarEventClass — Ghidra Research Report

**Primary addresses (inherited from prior docs, verified earlier):**
- `0x0065FA70` — CreateRadarEvent (distance dedup + alloc, 36 lines)
- `0x0065FB80` — InitRadarEvent (populate 64-byte struct, 73 lines)
- `0x0065FDD0` — TickAllRadarEvents (per-frame event loop, 16 lines)
- `0x0065FE00` — TickRadarEvent (single event tick, 86 lines)
- `0x00660000` — TickAndDrawRadarEvents wrapper (35 lines)
- `0x00660050` — DrawRadarEvent (164 lines)
- `0x006603B0` — CleanupExpiredEvents (48 lines)
- `0x00660540` — DrawViewportRect (special "camera rect" event, 64 lines)

**Type-config table:** `DAT_007F0998` — 17 entries × 16 B (272 B total). The first 6 rows are configurable from `rulesmd.ini`; rows 6–16 are hardcoded defaults baked into the binary and used by live YR code paths.

**Active in YR:** YES — all 17 type slots are pushed by live YR code paths (verified by Ghidra xref pass on `0x0065FA70`).

**Confidence:** HIGH for struct layout, lifecycle, INI keys, type-config table contents, and caller→type mapping (all verified directly in Ghidra during the post-audit spot-check). LOW only for the few callers whose type argument is dynamic (e.g., `TriggerAction::Execute` reads its type from map data).

**Ghidra MCP availability:** The MCP server was offline during the initial draft. After it reconnected, a spot-check pass (a) read the full type-config table from memory, (b) extracted the type argument at every CreateRadarEvent xref via `MOV ECX, <type>` immediately preceding each call, and (c) read the EVA string for each call site to label the type semantically. The findings overturned several earlier assumptions inherited from sibling docs — see §4, §8, §11 for the corrected content.

---

## 1. Overview

`RadarEventClass` is a 64-byte heap object representing a single animated diamond on the minimap. A small global event-array + ring-buffer holds the live set. Six logical event *types* are user-configurable from `rulesmd.ini`; the binary actually carries a 13-slot type-config table (the extra 7 slots are dormant defaults). Game systems push events by calling a single entry point, `CreateRadarEvent(type, cell)`, which does per-type distance-based dedup and then allocates + initializes the event.

Two decoupled things are easy to conflate:
- **RadarEventClass** — the *visual* event (pulsing diamond on the minimap). This report.
- **EVA announcement** — the voice cue ("Our base is under attack"). Owned by `VoxClass` (see `EVA_SYSTEM_DEEP_DIVE_GHIDRA_REPORT.md`).

These are separate queues. They are *correlated* (most callers fire both), and for "Base under attack" the EVA is actually rate-limited *by* the radar event: `HouseClass::BaseUnderAttack` at `0x004F93E0` only plays `EVA_OurBaseIsUnderAttack` if `CreateRadarEvent` returned 1 (the dedup distance check passed). *Corrected 2026-09-20:* it is not the only coupling. The same return-value gate was read at `ChangeOwner 0x0044847C` (`TEST AL,AL; JZ` past `EVA_BuildingCaptured`), and the Rust producers cite it for the miner and ally lines of `NotifyUnderAttack`, `Place_Production` (`EVA_UnitReady`), `Death_Announcement` (`EVA_UnitLost`) and the bridge repair line. The remaining §8 callers were not rechecked for it.

---

## 2. Event object layout (64 bytes)

Re-stated from [RADAR_MINIMAP_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADAR_MINIMAP_DEEP_DIVE.md) §"Per-Tick Lifecycle" and [RADAR_SYSTEM_COMPREHENSIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADAR_SYSTEM_COMPREHENSIVE.md) §"Event object layout" — verified previously from the binary:

| Offset | Type | Init | Name / Purpose |
|-------:|------|------|----------------|
| +0x00 | int   | param             | **type** (0–12; drives color and type-config row) |
| +0x04 | int   | computed          | **radar_x** (pixel on radar surface − radar_origin_x at `DAT_00880c84`) |
| +0x08 | int   | computed          | **radar_y** (− `DAT_00880c88`) |
| +0x0C | float | max-edge distance | **radius** (starts large, shrinks toward `RadarEventMinRadius`) |
| +0x10 | float | 0x3F490FDB (π/4)  | **rotation_angle** (radians) |
| +0x14 | float | Rules+0x84        | **rotation_speed** (init = `RadarEventRotationSpeed` = 0.05) |
| +0x18 | float | 0.0               | **color_fade** (oscillates 0.0 ↔ 1.0) |
| +0x1C | float | Rules+0x78        | **fade_speed** (init = `RadarEventColorSpeed` = 0.1; sign flips at bounds) |
| +0x20 | int   | packed cell       | **source_cell** (original cell coordinate, used by Spacebar cycle) |
| +0x24 | int   | g_CurrentFrame    | **timer1_start** (expand-phase start frame) |
| +0x28 | int   | —                 | timer1_aux |
| +0x2C | int   | 0                 | **timer1_duration** (frames; set from `type_config.blink_duration` on phase-2 entry) |
| +0x30 | int   | g_CurrentFrame    | **timer2_start** (set when expanding ends) |
| +0x34 | int   | —                 | timer2_aux |
| +0x38 | int   | 0                 | **timer2_duration** (from `type_config.visibility_duration`) |
| +0x3C | byte  | 1                 | **expanding_flag** (1 = phase 1 radius-shrink, 0 = phase 2 steady/fade) |
| +0x3D | byte  | 1                 | **needs_draw_flag** (TickRadarEvent returns early when 0 → event is dead) |

**Initial radius** is the distance from the event position to the farthest radar edge: `max(radar_x, radar_y, radar_w − radar_x, radar_h − radar_y)`. Guarantees the diamond starts off-screen and is always visible as it contracts.

---

## 3. Global state

From [RADAR_MINIMAP_RENDERING.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/RADAR_MINIMAP_RENDERING.md) §"Event creation" — previously verified:

| Address | Type | Name |
|---------|------|------|
| `0x00B04DA8` | DynVec*   | `event_vector` (DynVec backing store) |
| `0x00B04DAC` | int*      | `event_array` (pointer to array of event ptrs) |
| `0x00B04DB0` | int       | `event_array_capacity` |
| `0x00B04DB8` | int       | `event_count` |
| `0x00B04D48` | int[8]    | `event_cell_ring` — last 8 event cells |
| `0x00B04DD8` | int       | `ring_write_index` (mod 8) |
| `0x00B04D88` | int       | `ring_counter` |
| `0x007F0998` | byte[272] | `radar_event_type_config` (17 × 16 B; **all rows are compile-time hardcoded defaults**. Despite the INI suggesting otherwise, no code path patches this table at INI load — see §5 and §11 OQ1.) |

The ring buffer at `0x00B04D48` is the Spacebar-cycle backing store — pressing Spacebar scrolls the tactical view through the eight most recent event cells.

---

## 4. Type-config table at `DAT_007F0998` (17 × 16 B)

Four ints per row: `{dedup_distance_cells, visibility_duration_frames, blink_duration_frames, unique_flag}`. Verified by direct memory read of 272 bytes from `0x007F0998`:

| Type | Semantic label | Source of label | dedup | vis | blink | unique | Color in DrawRadarEvent |
|:---:|---|---|:---:|:---:|:---:|:---:|---|
| 0  | Combat                | INI                       | 8 | 200 | 400 | yes | WHITE  (0xFF,0xFF,0xFF) |
| 1  | Noncombat             | INI                       | 8 | 200 | 400 | no  | YELLOW (0xFF,0xFF,0x00) |
| 2  | Dropzone              | INI                       | 8 | 200 | 400 | no  | YELLOW (0xFF,0xFF,0x00) |
| 3  | BaseUnderAttack       | INI                       | 8 | 200 | 600 | yes | WHITE  (0xFF,0xFF,0xFF) |
| 4  | HarvesterUnderAttack  | INI                       | 8 | 200 | 400 | yes | WHITE  (0xFF,0xFF,0xFF) |
| 5  | EnemyObjectSensed     | INI                       | 6 | 200 | 400 | yes | CYAN   (0x00,0xFF,0xFF) |
| 6  | UnitReady             | EVA string at caller      | 2 |   0 | 200 | yes | (default — see below)   |
| 7  | UnitLost              | EVA string at caller      | 8 |   0 | 200 | yes | (default)               |
| 8  | UnitRepaired          | EVA string at caller      | 2 |   0 | 400 | yes | (default)               |
| 9  | SpyInfiltration       | dispatcher caller         | 5 |   0 | 400 | no  | (default)               |
| 10 | BuildingCaptured      | EVA string at caller      | 8 |   0 | 100 | no  | (default)               |
| 11 | BeaconPlaced          | EVA string at caller      | 8 | 200 | 200 | yes | YELLOW (explicit case)  |
| 12 | ConstructionComplete  | inferred from caller      | 8 | 200 | 400 | no  | YELLOW (explicit case)  |
| 13 | ImpactSilent          | inferred from callers     | 8 |   0 | **5** | no  | (default — and blink=5 makes it vanish almost immediately) |
| 14 | BridgeRepaired        | EVA string at caller      | 8 |   0 | 200 | yes | (default)               |
| 15 | StructureAbandoned    | EVA string at caller      | 8 |   0 | 400 | yes | (default)               |
| 16 | AllyUnderAttack       | EVA string at caller      | 8 | 200 | 600 | yes | (default)               |

**Color switch (verified by direct decompilation of `DrawRadarEvent` at `0x00660050`):**
- `case 0, 3, 4:` → bright `(0xFF,0xFF,0xFF)` / dim `(0x80,0x80,0x80)` — WHITE
- `case 5:`       → bright `(0x00,0xFF,0xFF)` / dim `(0x00,0x80,0x80)` — CYAN
- `case 1, 2, 11, 12:` → bright `(0xFF,0xFF,0x00)` / dim `(0x80,0x80,0x00)` — YELLOW
- `default (6–10, 13–16):` → calls `FUN_004355b0(0, 0, 0)` → returns 0 → the subsequent guard `if (local_94 != 0 || ... || local_92 != 0)` skips the entire draw block. **Default-color types do NOT pulse on the minimap.** They exist in the event array (and contribute to the Spacebar ring buffer + dedup) but render no diamond.

The dim color for explicit cases is always the bright color with each RGB channel halved (0xFF → 0x80). Each tick's actual render color is `lerp(dim, bright, color_fade)`, and `color_fade` oscillates on `RadarEventColorSpeed` — that's where the pulse comes from.

**This explains the apparent contradiction with in-game behavior.** Bullet impacts (type 13) and super-weapon launches (also type 13) don't paint the minimap with diamonds — they push silent ring-buffer entries. The visible "incoming nuke" minimap effect comes from a different mechanism (likely the warhead anim or a SuperClass-specific overlay), not from RadarEventClass. **Only types 0–5 and 11–12 actually draw a diamond.**

**Earlier label mistake corrected.** The earlier [RADAR_MINIMAP_RENDERING.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/RADAR_MINIMAP_RENDERING.md) (line 844–856) and the first revision of this doc inferred labels for rows 6–12 from the (wrong) assumption that all default types pulse yellow. The verified caller→EVA mapping from §8 supersedes that — semantic labels for types 6–16 now come from each caller's EVA string, not from color guessing. The first revision also stopped at 13 rows; the actual table extends to row 16.

---

## 5. INI keys — six labeled types

From `ini/rulesmd.ini` lines 451–470, copied verbatim:

```
; Controls for radar events
; The events, in order, are:
; (1) Generic Combat Event,
; (2) Generic Noncombat Event,
; (3) Dropzone Event,
; (4) Base Under Attack Event,
; (5) Harvester Under Attack Event,
; (6) Enemy Object Sensed Event
; So, for example, to change the visibility duration of the Harvester Under Attack Event,
; you would change the fifth number in the list for RadarEventVisibilityDurations
;
RadarEventSuppressionDistances=8, 8, 8, 8, 8, 6  ; suppression distance in cells
RadarEventVisibilityDurations=200,200,200,200,200,200  ; event visibility in frames
RadarEventDurations=400,400,400,400,400,400            ; event duration in frames
FlashFrameTime=7
RadarCombatFlashTime=49  ; ALWAYS odd multiple of FlashFrameTime
RadarEventMinRadius=8
RadarEventSpeed=1.2
RadarEventRotationSpeed=.05
RadarEventColorSpeed=.1
```

The comment numbers are 1-indexed. **In code they are 0-indexed:**

| Code type | INI index | Logical name         |
|:---------:|:---------:|----------------------|
| 0         | (1)       | Combat               |
| 1         | (2)       | Noncombat            |
| 2         | (3)       | Dropzone             |
| 3         | (4)       | BaseUnderAttack      |
| 4         | (5)       | HarvesterUnderAttack |
| 5         | (6)       | EnemyObjectSensed    |

**The three array INI keys are effectively dead.** `RulesClass::ReadGeneral` (at `0x0066d530`) parses `RadarEventSuppressionDistances` / `RadarEventVisibilityDurations` / `RadarEventDurations` into RulesClass instance fields at `+0x43C` / `+0x458` / `+0x474`, but **no code path copies those parsed values into the runtime type-config table at `0x007F0998`**. Verified by full xref scan of the table base (only 3 readers, 0 writers) — see §11 OQ1 for the full reasoning. The 4 scalar INI keys (`RadarEventMinRadius`, `RadarEventSpeed`, `RadarEventRotationSpeed`, `RadarEventColorSpeed`) DO take effect — they're stored in RulesClass+0x7c/0x80/0x84/0x78 and read directly by `InitRadarEvent` and `TickRadarEvent`. But the per-type arrays are silently ignored; modders changing them get no behavior change. All 17 types are reachable from live YR code paths (see §8) — they are NOT TS-legacy.

**Global knobs (shared across all types):**
- `FlashFrameTime=7` — base flash interval (frames). Used by object-selection flash, not directly by event drawing.
- `RadarCombatFlashTime=49` — total combat flash duration. `49 / 7 = 7` (must be odd multiple per INI comment).
- `RadarEventMinRadius=8` — diamond shrinks to this radius minimum (pixels).
- `RadarEventSpeed=1.2` — radius-shrink rate per tick (pixels/tick).
- `RadarEventRotationSpeed=.05` — diamond spin rate (radians/tick).
- `RadarEventColorSpeed=.1` — `color_fade` delta per tick (bounces between 0.0 and 1.0).

---

## 6. Core logic — tick lifecycle

Pseudocode of `TickRadarEvent` (FUN_0065FE00) from [RADAR_MINIMAP_RENDERING.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/RADAR_MINIMAP_RENDERING.md) §"TickRadarEvent" (previously verified):

```text
if (!event->needs_draw) return                         # +0x3D == 0 → dead

if (timer2 elapsed past timer2_duration):              # visibility over
    event->needs_draw = 0
    return

# Phase 1 — shrinking radius
new_r = event->radius - Rules.RadarEventSpeed          # -1.2 / tick
new_r = max(new_r, Rules.RadarEventMinRadius)          # floor = 8
event->radius = new_r

if (event->expanding_flag):                            # phase 1 active
    rotation_speed decays toward base_speed * 0.3333 (1/3 min)
    rotation_speed step = base_speed * 0.02

    if (radius == RadarEventMinRadius within epsilon 0.01):
        event->expanding_flag = 0                      # enter phase 2
        event->timer2_start = g_CurrentFrame
        event->timer2_duration = type_config[type].visibility_duration
        event->timer1_start   = g_CurrentFrame
        event->timer1_duration = type_config[type].blink_duration

# Rotation (both phases)
event->rotation_angle += event->rotation_speed
if (event->rotation_angle > 2*PI) event->rotation_angle -= 2*PI

# Color fade — oscillates 0.0 ↔ 1.0 with sign-flip on hitting bounds
new_fade = event->color_fade + event->fade_speed
if (new_fade < 0.0 && fade_speed < 0.0):
    event->fade_speed = -event->fade_speed              # bounce up
    event->color_fade = 0.0
elif (new_fade > 1.0 && fade_speed > 0.0):
    event->fade_speed = -event->fade_speed              # bounce down
    event->color_fade = 1.0
else:
    event->color_fade = new_fade
```

**Verified numeric constants** (previously):
- Rotation decel step = `base_rot_speed × 0.02` (`0x3CA3D70A` at `0x7F0AE8`).
- Rotation floor = `base_rot_speed × 0.3333` (`0x3EAAAAAB` at `0x7ED968`).
- Phase-transition epsilon = `0.01` double at `0x7E3808`.
- Color-fade bounds = `0.0` and `1.0` (`0x3F800000` at `0x7E2AC8`).

`CleanupExpiredEvents` (FUN_006603B0) sweeps the array each frame, removes any event with `needs_draw == 0`, and frees its 64 bytes.

---

## 7. Create / dedup entry point

Pseudocode of `CreateRadarEvent` (FUN_0065FA70) — previously verified:

```text
if (type_config[type].unique_flag == 1):
    for each existing event e in event_array:
        if (e.type == type):
            dist = sqrt((cell.x - e.source_cell.x)^2 + (cell.y - e.source_cell.y)^2)
            if (dist < type_config[type].dedup_distance):
                return 0           # suppressed — too close to a live same-type event

event = operator_new(0x40)
InitRadarEvent(event, type, cell)  # sets all fields per §2
event_array.push(event)

# Ring-buffer for Spacebar cycling
ring_index = (ring_counter + 1) % 8
event_cell_ring[ring_index] = cell
ring_counter = ring_index

return 1
```

**Suppression matters for callers.** Because only `unique_flag == 1` types (0, 3, 4, 5) dedup, consecutive `CreateRadarEvent(Combat, …)` calls from adjacent bullet impacts don't spam — the first combat event within an 8-cell radius wins and subsequent calls return 0. Types 1 and 2 (`Noncombat`, `Dropzone`) have `unique_flag == 0` and therefore **never** dedup — every call creates a new diamond.

---

## 8. Callers → type argument

The **complete xref set** for `CreateRadarEvent` (`0x0065FA70`) — 25 distinct call sites across 19 caller functions, with the type argument extracted directly from the `MOV ECX, <imm>` immediately preceding each call, and the EVA string (when present) read from the `MOV ECX, <ptr>; CALL 0x00752700` (PlayEVA) sequence after the call.

| Caller (address) | ECX (type) | Followed by EVA | Confidence |
|---|:---:|---|:---:|
| `HouseClass::NotifyUnderAttack` (0x004f9544) | **3** BaseUnderAttack | EVA_OurBaseIsUnderAttack | HIGH |
| `UnitClass::ReceiveDamage` (0x0073851d) | **4** HarvesterUnderAttack | EVA_OreMinerUnderAttack | HIGH |
| `HouseClass::NotifyUnderAttack` (0x004f94e4) | **4** HarvesterUnderAttack | EVA_OreMinerUnderAttack | HIGH |
| `TemporalClass::InitiateWarp` (0x0071b04c) | **4** HarvesterUnderAttack | EVA_OreMinerUnderAttack | HIGH (gated: target.RTTI == UNIT && target.Owner == local && targetType+0xe0e flag set — likely Harvester=yes) |
| `TechnoClass::IdleAnimDispatch` (0x0070dad7) | **5** EnemyObjectSensed | (none) | HIGH |
| `HouseClass::Place_Production` (0x004fb631) | **6** UnitReady | EVA_UnitReady | HIGH |
| (unlabeled function — body starts at **0x004d98D4**, call at 0x004d98fe) | **7** UnitLost | EVA_UnitLost | HIGH |
| `BuildingClass::MissionRepairAndProduce` (0x0044b960) | **8** UnitRepaired | EVA_UnitRepaired | HIGH |
| `BuildingClass::MissionRepairAndProduce` (0x0044bdb2) | **8** UnitRepaired | EVA_UnitRepaired | HIGH |
| `BuildingClass::OnSpyInfiltrate` (0x0045722f) | **9** SpyInfiltration | (return value drives sub-branch) | HIGH |
| `BuildingClass::ChangeOwner` (0x00448477) | **10** BuildingCaptured | EVA_BuildingCaptured | HIGH |
| `RadarClass::PlaceBeacon` (0x00430f08) | **11** BeaconPlaced | EVA_BeaconDetected | HIGH |
| `FUN_00431450` (0x004316e5) | **11** BeaconPlaced | (none) | HIGH (type confirmed; caller likely beacon-related per address adjacency) |
| `BuildingClass::OnConstructionComplete` (0x004468a8) | **12** ConstructionComplete | (after IsHumanPlayer guard) | HIGH |
| `BulletClass::AI` (0x00467ea7) | **13** ImpactSilent | (none — falls through to AnimType lookup) | HIGH |
| `LightningStorm::Start` (0x00539f89) | **13** ImpactSilent | — | HIGH |
| `SuperClass::Launch` (0x006cc4be) | **13** ImpactSilent | — | HIGH |
| `SuperClass::Launch` (0x006cc4d2) | **13** ImpactSilent | — | HIGH |
| `SuperClass::Launch` (0x006ccdd7) | **13** ImpactSilent | — | HIGH |
| `SuperClass::Launch` (0x006ccf2f) | **13** ImpactSilent | — | HIGH |
| `SuperClass::Launch` (0x006cd8e0) | **13** ImpactSilent | — | HIGH |
| `InfantryClass::Mission_Enter` (0x00519bb6) | **14** BridgeRepaired | EVA_BridgeRepaired | HIGH |
| `BuildingClass::CheckAutoSellOrCivilian` (0x004582c5) | **15** StructureAbandoned | EVA_StructureAbandoned | HIGH |
| `HouseClass::NotifyUnderAttack` (0x004f95a0) | **16** AllyUnderAttack | EVA_OurAllyIsUnderAttack | HIGH |
| `TriggerAction::Execute` (0x006df1ca) | dynamic from `[ESI+0x90]` | — | LOW (type = trigger-action data field) |

### Methodology

`CreateRadarEvent` is `__thiscall(int type, int cell)` — first arg in ECX. Every call site is preceded by `MOV ECX, <type>`. Reading that immediate at each xref gives the type argument with HIGH confidence. The follow-on EVA string was read from the `MOV ECX, <ptr>; CALL 0x00752700` (PlayEVA) sequence after each radar-event call, when present.

### Useful structural notes

- **`HouseClass::NotifyUnderAttack` is one function with 3 dispatch sites** (one each for harvester / own-base / allied-base attack). The first revision of this doc treated this as "one BaseUnderAttack call" — that was wrong; it's a three-way switch keyed on the victim's relation to the local player.
- **Type 13 is the dominant impact-class event** (8 of 25 sites). It has `vis=0, blink=5, unique=0` and falls in the no-draw default color branch — i.e., **type 13 is silent**: it pushes a ring-buffer entry for Spacebar cycling but does not paint the minimap. Visible super-weapon / nuke / lightning-storm radar effects come from elsewhere (warhead anim, SuperClass-specific overlay, etc.).
- **Type 4 (HarvesterUnderAttack) fires from three different callers** (UnitClass::ReceiveDamage, NotifyUnderAttack, TemporalClass::InitiateWarp). The Temporal-warp caller using OreMiner EVA is unexpected — see §11 OQ 2.
- **The function containing the UnitLost call has no Ghidra header.** Body is `0x004d98D4`–`0x004d9919` (`__thiscall(TechnoClass* this, int unused)`; `RET 4`). Logic: check `Owner` is human-player, virtual call `vtable[0x1B8]` (slot 110) to read cell coords, `CreateRadarEvent(7, cell)`, then `EVA_UnitLost` if the radar event wasn't suppressed. A future labeling pass should create the function and name it `TechnoClass::Notify_Owner_Of_Loss` (or similar).
- **`TriggerAction::Execute`** can fire any type because the type arg is read from map-trigger data at `[ESI+0x90]`. This means custom YR maps can spawn arbitrary radar event types via mission triggers.
- **Type 0 (Combat) has no live caller in this xref set.** Despite being labeled "Combat" in the INI, no compiled engine code passes type 0. It is reachable only via `TriggerAction::Execute` (which reads its type argument from map-trigger data at `[ESI+0x90]`). The "Combat" type is effectively a slot reserved for map triggers — modders can fire it from missions but the engine itself never emits it.

---

## 9. Integration points

**Tick cycle placement:**
- `RadarClass::Update` (the radar per-frame workhorse at `0x00656EC0`) calls `TickAllRadarEvents` (`0x0065FDD0`) and then `TickAndDrawRadarEvents` (`0x00660000`) every frame — see [RADAR_MINIMAP_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADAR_MINIMAP_DEEP_DIVE.md) §7.
- `DrawViewportRect` (the rotating camera rectangle) shares the event struct layout — it's "just another event" that never expires, with a fixed rotation and type field.
- The ring buffer at `0x00B04D48` is consumed by the Spacebar hotkey handler (see [HOTKEY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/HOTKEY_SYSTEM_GHIDRA_REPORT.md)) — it's how cycling through recent combat events works.

**Inputs (RulesClass):**
- `+0x78` `RadarEventColorSpeed`, `+0x7C` `RadarEventMinRadius`, `+0x80` `RadarEventSpeed`, `+0x84` `RadarEventRotationSpeed`
- `+0x88` `FlashFrameTime`, `+0x8C` `RadarCombatFlashTime`
- `+0x43C` `RadarEventSuppressionDistances[6]` — parsed but unused (see §5, §11 OQ1)
- `+0x458` `RadarEventVisibilityDurations[6]` — parsed but unused
- `+0x474` `RadarEventDurations[6]` — parsed but unused. Despite earlier sibling-doc claims, no code copies these arrays into the runtime type-config table at `0x007F0998`. The per-type dedup/vis/blink values are baked-in compile-time constants; the parsed arrays sit in RulesClass instance memory and are read by nothing.

**Outputs:**
- Visual: radar primary surface (only) — drawn on top of object dots each frame.
- Spacebar cycling: last-8-cell ring buffer.
- EVA coupling: one-way. A caller gates its EVA line on `CreateRadarEvent`'s return value; see §1 for the sites where that gate was read. *Corrected 2026-09-20: an earlier revision named `BaseUnderAttack` as the only gated line.*

---

## 10. Current Rust implementation — status vs binary

Rechecked 2026-09-20 against `CreateRadarEvent 0x0065FA70`, `InitRadarEvent
0x0065FB80`, `TickRadarEvent 0x0065FE00` and `CleanupExpiredEvents 0x006603B0`
(decompilation and the complete 25-entry xref set of `0x0065FA70`).

**One owner.** [src/render/radar_events.rs](../../src/render/radar_events.rs)
holds the only event array: all 17 types, the compiled type table of §4, the
colour switch, the shared tick/cleanup lifecycle and the eight-cell Spacebar
ring. It is client-local presentation state, never snapshot or hash input,
because every native caller is gated on `g_PlayerPtr`.

**Producers.** The simulation publishes a `RadarEventRequest` (type + cell,
[src/sim/radar.rs](../../src/sim/radar.rs)) on the sound event of each caller it
models: types 3/4/16 (`NotifyUnderAttack`), 6 (`Place_Production`), 7
(`Death_Announcement`), 10 (`ChangeOwner`) and 14 (bridge repair). The app
dispatcher applies the caller's local-owner test, then asks the minimap's
event array for admission; that result gates the caller's EVA line, as in §8.
Type 5 is created render-side by the radar object tracker.

**Removed.** The former sim-side `RadarEventQueue` (a second, capped queue with
its own aging, a rotation-only animation and an owner-blind dedupe), its
`minimap_legacy_events` drawer, and the Spacebar fallback that could not reach
it once a type-5 cell existed. Also removed: the type-0 `Combat` push on every
`RevealOnFire` shot. §8 shows no engine caller passes type 0, and the
`BulletClass::AI` site (`0x00467EA7`) is the `NUKE` payload's silent type 13.

**Not yet produced** (no Rust caller; each needs its own native caller port):
types 8 `UnitRepaired`, 9 `SpyInfiltration`, 11 `BeaconPlaced`, 12
`ConstructionComplete`, 13 `ImpactSilent` (superweapon launches, lightning
storm start, nuke payload), 15 `StructureAbandoned`, the
`TemporalClass::InitiateWarp` type-4 site, and `TriggerAction::Execute`'s
dynamic type.

**Known difference.** Native dedupe distance is `ftol(Sqrt_Approx(dx²+dy²))`;
Rust compares squared integer distance, which is identical for an exact square
root. `Sqrt_Approx` was not compared numerically.

---

## 11. Open questions — all closed by the post-audit Ghidra pass

### OQ1 (closed) — `RadarEventDurations` semantics

`RulesClass::ReadGeneral` (at `0x0066d530`) parses `RadarEventSuppressionDistances`, `RadarEventVisibilityDurations`, and `RadarEventDurations` into RulesClass instance fields at `+0x43C` / `+0x458` / `+0x474`. **No code path copies those parsed values into the runtime type-config table at `0x007F0998`.** Verified by full xref scan of the table base — only 3 readers (`CreateRadarEvent` reads unique_flag column at `+0x9A4`, `TickRadarEvent` reads vis/blink columns at `+0x99C`/`+0x9A0`, `FUN_00660460` reads the dedup column for a "would-be-suppressed" predicate); zero writers anywhere in the binary.

The values match between INI defaults and the table because both are the same compile-time constant — not because of any runtime patch. **A modder editing the three array INI keys gets no behavior change.** The 4 scalar INI keys (MinRadius, Speed, RotationSpeed, ColorSpeed) DO take effect because `InitRadarEvent` reads them from RulesClass+0x7c/0x80/0x84/0x78 directly.

This contradicts both the earlier sibling docs and an earlier revision of this doc that claimed "the INI arrays patch the first 6 rows of the type-config table at INI load." That claim was inferred from value coincidence and never verified. It is wrong.

### OQ2 (closed) — Why TemporalClass::InitiateWarp uses harvester EVA

The `CreateRadarEvent(4) + EVA_OreMinerUnderAttack` block at `0x0071b04c` is gated by:
1. `(target).vtable[0x2c]() == 1` — target's RTTI is UNIT (not building);
2. `target.Owner == g_PlayerPtr` (`+0x21C` field check via `[0x87]` int-stride) — target is owned by local player;
3. `target.Type[+0xe0e] != 0` — target's UnitTypeClass has a specific flag set (likely `Harvester=yes` or a comparable resource-collector flag).

A separate later branch in the same function handles `iVar2 == 6` (target is a BUILDING) by calling `HouseClass::NotifyUnderAttack(target.Owner)` — that's the path that fires the proper `BaseUnderAttack` (type 3) or `AllyUnderAttack` (type 16) chain. So the apparent contradiction with the earlier sibling doc is resolved: ChronoLegion warping a player BUILDING goes through NotifyUnderAttack → BaseUnderAttack EVA; warping a player HARVESTER goes through this direct path → OreMiner EVA. The earlier sibling doc's claim that "target is building" gates the call was wrong.

### OQ3 (closed) — `FUN_00431450` purpose

It's a beacon placement / registration helper. Signature: `__thiscall(int* beacon_array, int param_2, int row, int col, char param_5)`. Iterates the multiplayer-beacon grid (8 slots × 3 cells per slot at `param_1 + n*3`); when called with `row==-1 && col==-1` it scans for an existing beacon, otherwise it indexes directly. The `CreateRadarEvent(11)` call at `0x004316e5` fires on the "ally placed a beacon, show it on my radar" branch (gated by `target.Owner != local && IsAllied(target.Owner)`).

Suggested Ghidra label: `Beacon::Place_Or_Update` or similar.

### OQ4 (closed) — Function at 0x004d98fe (UnitLost caller)

The function has no Ghidra header but its body is `0x004d98D4`–`0x004d9919`. Signature: `__thiscall(TechnoClass* this, int unused)` (returns void with `RET 4`). Body:
1. Check `HouseClass::IsHumanPlayer(this.Owner)` — skip if AI.
2. Virtual call `this->vtable[0x1B8]` (slot 110) with output buffer to read this Techno's cell coords.
3. `CreateRadarEvent(7, cell)` → if not suppressed, `VoxClass::PlayEVA("EVA_UnitLost")`.

Suggested Ghidra label: `TechnoClass::Notify_Owner_Of_Loss`. Caller likely from `TechnoClass::Limbo` or one of the death paths (`Mark(MARK_DOWN)` etc.) — needs an xref pass on the new function once labeled.

### OQ5 (closed) — Type 0 (Combat) caller

None statically. All 25 xrefs to `CreateRadarEvent` (`0x0065FA70`) load a non-zero immediate into ECX. Type 0 is reachable only via `TriggerAction::Execute` (`0x006df1ca`), which reads the type argument from map-trigger data at `[ESI+0x90]`. **The "Combat" type is effectively a slot reserved for map triggers** — modders can fire it from missions but the engine itself never emits it.

### Originally closed by the first spot-check (still closed)

- Who pushes type 4 (HarvesterUnderAttack)? → `UnitClass::ReceiveDamage` + `HouseClass::NotifyUnderAttack` site 1 + `TemporalClass::InitiateWarp` (gated, see OQ2).
- Who pushes type 5 (EnemyObjectSensed)? → `TechnoClass::IdleAnimDispatch`.
- Paradrop radar event? → No CreateRadarEvent xref from any paradrop function.
- Are types 6–12 dormant in YR? → No. All heavily used.
- Psychic Dominator launch type? → Type 13 (silent), like all 5 SuperClass::Launch sites.
- Spy infiltration scope? → Single CreateRadarEvent at the `BuildingClass::OnSpyInfiltrate` dispatcher (type 9), not per sub-branch.
- "Unit Lost" doesn't push a radar event? → It DOES. Type 7 from the unlabeled function at 0x004d98D4.

---

## Sources

**Existing Ghidra reports referenced (all in `docs/research/`):**
- [RADAR_MINIMAP_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADAR_MINIMAP_DEEP_DIVE.md) — event struct layout, per-tick lifecycle, DrawViewportRect coupling
- [RADAR_MINIMAP_RENDERING.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/RADAR_MINIMAP_RENDERING.md) — type-config table values, color switch, event creation flow, full address map
- [RADAR_SYSTEM_COMPREHENSIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADAR_SYSTEM_COMPREHENSIVE.md) — event globals, field offsets
- [EVA_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/EVA_SYSTEM_GHIDRA_REPORT.md) §5 / §6 — BaseUnderAttack rate-limit coupling; EVA trigger table
- `EVA_SYSTEM_DEEP_DIVE_GHIDRA_REPORT.md` §4 — 75-entry PlayEVA xref table
- [BULLET_CLASS_AI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_CLASS_AI_GHIDRA_REPORT.md) — impact radar blip
- [CHRONOSPHERE_SUPERWEAPON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CHRONOSPHERE_SUPERWEAPON_GHIDRA_REPORT.md) — ChronoWarp dual-event
- [NUKE_SUPERWEAPON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/NUKE_SUPERWEAPON_GHIDRA_REPORT.md) — special detonation radar blip
- [LIGHTNING_STORM_SUPERWEAPON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LIGHTNING_STORM_SUPERWEAPON_GHIDRA_REPORT.md) — storm start radar blip
- [PSYCHIC_DOMINATOR_SUPERWEAPON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PSYCHIC_DOMINATOR_SUPERWEAPON_GHIDRA_REPORT.md) — launch radar blip
- [ION_BLAST_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ION_BLAST_CLASS_GHIDRA_REPORT.md) — genetic mutator detonation
- [TEMPORAL_WEAPON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TEMPORAL_WEAPON_SYSTEM_GHIDRA_REPORT.md) — ChronoLegionnaire player-building hit
- [SPY_INFILTRATION_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPY_INFILTRATION_SYSTEM_GHIDRA_REPORT.md) — spy effect call table
- [BUILDINGCLASS_MISSION_REPAIR_AND_PRODUCE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MISSION_REPAIR_AND_PRODUCE.md) §0x21 — repair-complete eject
- [BUILDINGCLASS_SPECIAL_BUILDINGS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_SPECIAL_BUILDINGS_GHIDRA_REPORT.md) — captured-building eject
- [ADDRESS_MAP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ADDRESS_MAP.md) — radar/EVA/sensor address clusters

**INI:**
- `ini/rulesmd.ini` lines 451–470 (radar event keys + ordering comment)
- `ini/rulesmd.ini` line 613 (`BaseUnderAttackSound`)
- `ini/rulesmd.ini` lines 660–661 (`ChronoInSound` / `ChronoOutSound`)

**Rust implementation:**
- `src/sim/radar.rs`, `src/rules/radar_event_config.rs`, `src/render/minimap.rs`, `src/sim/world/mod.rs`, `src/sim/combat/mod.rs`
