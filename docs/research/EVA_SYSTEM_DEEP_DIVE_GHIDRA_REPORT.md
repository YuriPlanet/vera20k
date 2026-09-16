# EVA System Deep Dive - Ghidra Reverse Engineering Report

**Date:** 2026-03-23
**Binary:** gamemd.exe (Yuri's Revenge)
**Confidence:** HIGH - all findings verified from decompiled binary code
**Companion:** This supplements [EVA_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/EVA_SYSTEM_GHIDRA_REPORT.md) with exhaustive detail.

---

## 1. VoxClass Struct Layout (size = 0x54 = 84 bytes) -- COMPLETE

Verified from `VoxClass__ReadINI` (0x00752db0) and `VoxClass__ReadEVAINI` (0x00753000).

```
Offset  Size  Type       Field              Source / Evidence
------  ----  ---------  -----------------  -----------------
0x00    40    char[40]   Name               Copied from [DialogList] value in ReadEVAINI
                                             (memcpy of up to 40 bytes including null)
0x28    4     float      Volume             ReadINI: *(float*)(this+0x28) = ReadDouble("Volume", default 1.0)
                                             Initialized to 0x3F800000 (1.0f) in both ReadEVAINI and ReadINI
0x2C    9     char[9]    YuriSound          ReadINI: strncpy(this+0x2C, ReadString("Yuri"), 9); *(this+0x34)=0
0x35    9     char[9]    RussianSound       ReadINI: strncpy(this+0x35, ReadString("Russian"), 9); *(this+0x3D)=0
0x3E    9     char[9]    AlliedSound        ReadINI: strncpy(this+0x3E, ReadString("Allied"), 9); *(this+0x46)=0
0x47    1     pad        (alignment)        Implicit from 0x46+1 to 0x48
0x48    4     int        Priority           ReadINI: compared via stricmp against "LOW"/"NORMAL"/"IMPORTANT"/"CRITICAL"
                                             Default = 1 (NORMAL), set in ReadEVAINI constructor
0x4C    4     int        Type               ReadINI: compared via stricmp against "QUEUE"/"STANDARD"/"INTERRUPT"/"QUEUED_INTERRUPT"
                                             Default = 0 (STANDARD), set in ReadEVAINI constructor
0x50    4     int        PlayState          Default = 2 (DONE), set in ReadEVAINI constructor
                                             0 = PLAYING, 1 = QUEUED, 2 = DONE/FREE
```

### Constructor Initialization (in ReadEVAINI at 0x753000)

```c
VoxClass* vox = operator_new(0x54);  // 84 bytes
// Volume bytes set explicitly:
vox[0x28] = 0x00;   // \  These 4 bytes form 0x3F800000 = 1.0f
vox[0x29] = 0x00;   // |  (little-endian)
vox[0x2A] = 0x80;   // |
vox[0x2B] = 0x3F;   // /
// memcpy name from local buffer (up to 40 chars)
vox[0x2C] = 0;      // YuriSound[0] = null (empty)
vox[0x35] = 0;      // RussianSound[0] = null (empty)
vox[0x3E] = 0;      // AlliedSound[0] = null (empty)
vox[0x48..0x4B] = 1; // Priority = NORMAL (little-endian int)
vox[0x4C..0x4F] = 0; // Type = STANDARD
vox[0x50..0x53] = 2; // PlayState = DONE
```

### Observation: No "Text" or "Sound" INI Key

The VoxClass__ReadINI function reads exactly 6 INI keys:
1. `Volume` (float, string at 0x846568)
2. `Type` (enum string, key string at 0x824314)
3. `Priority` (enum string, key string at 0x84301c)
4. `Yuri` (sound name, key string at 0x846798)
5. `Russian` (sound name, key string at 0x846790)
6. `Allied` (sound name, key string at 0x846788)

There is NO "Text" key, "Sound" key, or CSF reference parsed in ReadINI.
The event name itself (e.g., "EVA_ConstructionComplete") serves as the CSF lookup key.

### Type Enum (comparison order in ReadINI, stricmp via FUN_007c8d20)

| Order | String (address)                | Value | Meaning              |
|-------|--------------------------------|-------|----------------------|
| 1st   | "QUEUE" (0x8467cc)             | 1     | Queued in priority list |
| 2nd   | "STANDARD" (0x8467c0)          | 0     | Fire-and-forget      |
| 3rd   | "INTERRUPT" (0x816120)         | 2     | Interrupts current   |
| 4th   | "QUEUED_INTERRUPT" (0x8467ac)  | 3     | Queued + highest priority |

### Priority Enum (comparison order in ReadINI)

| Order | String (address)                | Value | Meaning    |
|-------|--------------------------------|-------|------------|
| 1st   | "LOW" (0x8161dc)               | 0     | Lowest     |
| 2nd   | "NORMAL" (0x8161d4)            | 1     | Default    |
| 3rd   | "IMPORTANT" (0x8467a0)         | 2     | High       |
| 4th   | "CRITICAL" (0x8161c0)          | 3     | Highest    |

---

## 2. EVA INI Parsing -- Complete Flow

### Loading Sequence (in main init at 0x0052ba60)

1. `VoxClass__ClearAllEntries()` (0x7531a0) -- destroys all existing VoxClass objects
2. `VoxClass__ReadEVAINI()` (0x753000) -- parses EVAMD.INI from MIX archives

The game loads **EVAMD.INI** (not EVA.INI). The "md" variant is the only file parsed.
String reference: "EVAMD.INI" at 0x825df0, with error strings "Failed to find EVAMD.INI!" and
"Failed to load EVAMD.INI!".

### ReadEVAINI (0x00753000) -- Pseudocode

```c
void VoxClass__ReadEVAINI(void) {
    // Step 1: Open EVAMD.INI and find [DialogList] section
    int ini = CCINIClass__Open("EVAMD.INI");
    if (!CCINIClass__FindSection(ini, "DialogList"))  // 0x8467d4
        return;

    // Step 2: Count entries in [DialogList]
    int count = CCINIClass__GetEntryCount(ini, "DialogList");

    // Step 3: For each entry
    for (int i = 0; i < count; i++) {
        // Get the key name at index i (e.g., "0", "1", "2"...)
        CCINIClass__GetEntryName(ini, "DialogList", i);
        // Read value string (e.g., "EVA_ConstructionComplete")
        char eventName[200];
        CCINIClass__ReadString(ini, "DialogList", keyName, "", eventName, 200);

        if (eventName[0] == '\0') continue;  // skip empty

        // Step 4: Check for duplicate (case-insensitive)
        bool duplicate = false;
        for (int j = 0; j < VoxArray_Count; j++) {
            if (stricmp(eventName, VoxArray[j]->Name) == 0) {
                duplicate = true;
                break;
            }
        }
        // If duplicate found, skip creation but still call ReadINI
        // (this allows EVAMD.INI to override existing entries)

        if (!duplicate) {
            // Step 5: Allocate and initialize VoxClass
            VoxClass* vox = new VoxClass();  // 0x54 bytes
            vox->Volume = 1.0f;
            memcpy(vox->Name, eventName, strlen(eventName)+1);
            vox->YuriSound[0] = '\0';
            vox->RussianSound[0] = '\0';
            vox->AlliedSound[0] = '\0';
            vox->Priority = 1;   // NORMAL
            vox->Type = 0;       // STANDARD
            vox->PlayState = 2;  // DONE

            // Step 6: Add to global VoxArray
            VoxArray[VoxArray_Count++] = vox;
        }

        // Step 7: Parse section-specific data
        VoxClass__ReadINI(vox_or_existing);
    }
}
```

### Key Insight: EVA.INI vs EVAMD.INI Merging

There is **no merging** of eva.ini and evamd.ini in gamemd.exe. Only EVAMD.INI is loaded.
The YR expansion replaced eva.ini entirely with evamd.ini. The duplicate check in ReadEVAINI
is for entries within evamd.ini itself (preventing duplicate [DialogList] entries), not for
merging two files.

### Section Format in EVAMD.INI

```ini
[DialogList]
0=EVA_ConstructionComplete
1=EVA_UnitReady
2=EVA_NewConstructionOptions
...

[EVA_ConstructionComplete]
Priority=LOW
Type=QUEUE
Allied=ceva048
Russian=csof048
Yuri=cyur048

[EVA_UnitReady]
Priority=LOW
Type=QUEUE
Allied=ceva049
Russian=csof049
Yuri=cyur049
```

---

## 3. Queue Data Structures -- Exact Layout

### Queue Node (size = 0x20 = 32 bytes)

Allocated by `VoxClass__InsertIntoQueue` (0x752590) via `operator_new(0x20)`.

```
Offset  Size  Type        Field           Description
------  ----  ----------  -----------     -----------
0x00    8     LinkedList  ListLinks       Doubly-linked list prev/next (initialized by FUN_004072d0)
0x08    4     ???         ListHead        Back-pointer to owning list head
0x0C    4     VoxClass*   VoxEntry        Pointer to the VoxClass being queued
0x10    4     ???         unknown         (padding or reserved)
0x14    4     int         Priority        Priority level (0-3) for this queued entry
0x18    4     int         Type            Type value (0-3) for this queued entry
0x1C    4     int         SequenceNum     SequenceCounter % 100 (monotonic ordering)
```

### Fixed Global Queue Heads (12 bytes each = 3 dwords)

Each queue head is a doubly-linked list sentinel node (prev, next, count/size).

| Address Range        | Queue Name       | Used For                                   |
|---------------------|------------------|--------------------------------------------|
| `0xb1d3c8..0xb1d3d3` | InterruptQueue   | Type=3 (QUEUED_INTERRUPT) entries          |
| `0xb1d3f0..0xb1d3fb` | CriticalQueue    | Priority=3 (CRITICAL) entries              |
| `0xb1d450..0xb1d45b` | PriorityQueue[0] | Type=1 (QUEUE), Priority=0 (LOW) entries   |
| `0xb1d45c..0xb1d467` | PriorityQueue[1] | Type=1 (QUEUE), Priority=1 (NORMAL)        |
| `0xb1d468..0xb1d473` | PriorityQueue[2] | Type=1 (QUEUE), Priority=2 (IMPORTANT)     |
| `0xb1d474..0xb1d47f` | PriorityQueue[3] | Type=1 (QUEUE), Priority=3 (CRITICAL)      |
| `0xb1d4b8`           | PendingImmediate | Single-slot for STANDARD/INTERRUPT entries  |

### Queue Sizes

- **InterruptQueue, CriticalQueue, PriorityQueues**: Doubly-linked lists with dynamic allocation.
  No fixed size limit. Each node is heap-allocated (32 bytes).
- **PendingImmediate**: Single pointer slot. At most 1 entry. New entries with higher priority
  replace the existing one; lower priority entries are discarded.

### Ordering Within Queues

**FIFO within each queue**, not priority-sorted. The SequenceNum field (counter % 100) provides
secondary ordering for save/load but is NOT used for dequeue priority. Nodes are inserted at
the tail and dequeued from the head.

### Dequeue Priority Order (in PlayNextQueued)

1. **InterruptQueue** (0xb1d3c8) -- checked first, highest precedence
2. **CriticalQueue** (0xb1d3f0) -- checked second
3. **PendingImmediate** (0xb1d4b8) -- checked third, with special handling:
   - If CriticalQueue had an entry, PendingImmediate is DISCARDED (freed)
   - If PendingImmediate is the only entry, it plays normally
4. **PriorityQueue[3]** (0xb1d474) -- highest priority number
5. **PriorityQueue[2]** (0xb1d468)
6. **PriorityQueue[1]** (0xb1d45c)
7. **PriorityQueue[0]** (0xb1d450) -- lowest priority number

The loop iterates `from 0xb1d474 downward by 0xC bytes until < 0xb1d450`, checking each
queue's head. First non-empty queue wins.

### InsertIntoQueue Routing Logic (0x752590)

```c
void InsertIntoQueue(VoxClass* vox, int type, int priority) {
    QueueNode* node = new QueueNode();  // 32 bytes
    LinkedList_Init(node);
    node->VoxEntry = vox;
    node->Priority = priority;
    node->Type = type;
    node->SequenceNum = SequenceCounter % 100;
    SequenceCounter++;
    vox->PlayState = 1;  // QUEUED

    if (type == 3) {  // QUEUED_INTERRUPT
        LinkedList_InsertTail(&InterruptQueue, node);
        return;
    }
    if (type == 1) {  // QUEUE
        LinkedList_InsertTail(&PriorityQueue[priority], node);
        return;
    }
    if (priority == 3) {  // CRITICAL priority with non-QUEUE type
        LinkedList_InsertTail(&CriticalQueue, node);
        return;
    }

    // STANDARD (0) or INTERRUPT (2) with non-CRITICAL priority
    // Can only go to PendingImmediate, and ONLY if:
    //   1. InterruptQueue is empty, AND
    //   2. CriticalQueue is empty, AND
    //   3. Either PendingImmediate is NULL, or this has higher priority
    if (InterruptQueue.IsEmpty() && CriticalQueue.IsEmpty() &&
        (PendingImmediate == NULL || PendingImmediate->Priority < priority)) {
        PendingImmediate = node;
        return;
    }

    // DISCARD the event -- too low priority, something else is queued
    vox->PlayState = 2;  // DONE
    free(node);
}
```

---

## 4. Complete Call Site Table (75 xrefs to VoxClass__PlayEVA)

All callers of `VoxClass__PlayEVA` at 0x00752700, extracted via Ghidra script
with string argument identification.

### Production Events

| Call Address | Caller Function | EVA Event | Type Override |
|-------------|-----------------|-----------|---------------|
| 0x006a8e2f | StripClass__AI | EVA_ConstructionComplete | default |
| 0x006a8837 | FUN_006a87f0 (StripClass__AddEntry) | EVA_NewConstructionOptions | default |
| 0x006a6415 | SidebarClass__AddCameo | EVA_NewConstructionOptions | default |
| 0x004fb644 | HouseClass__Place_Production | EVA_UnitReady | default |
| 0x004fb377 | HouseClass__Place_Production | EVA_CannotDeployHere | default |
| 0x006ab498 | SelectClass__Action | EVA_Building | default |
| 0x006ab6c9 | SelectClass__Action | EVA_Building | default |
| 0x006aafa7 | SelectClass__Action | EVA_SelectTarget | default |
| 0x006ab3b1 | SelectClass__Action | EVA_UnableToComply | default |
| 0x006ab693 | SelectClass__Action | EVA_UnableToComply | default |
| 0x006ab007 | SelectClass__Action | EVA_OnHold | 2 (INTERRUPT) |
| 0x006ab108 | SelectClass__Action | EVA_OnHold | 2 (INTERRUPT) |
| 0x006aae39 | SelectClass__Action | EVA_Canceled | 2 (INTERRUPT) |

Note: SelectClass__Action does NOT play EVA_Training directly in the xref list.
The "EVA_Building" and "EVA_Training" branches depend on whether the production
item is a unit or structure -- confirmed by the conditional branching around 0x6ab498.

### Combat Events

| Call Address | Caller Function | EVA Event | Type Override |
|-------------|-----------------|-----------|---------------|
| 0x004f94fb | HouseClass__BaseUnderAttack | EVA_OreMinerUnderAttack | default |
| 0x004f95b3 | HouseClass__BaseUnderAttack | EVA_OurAllyIsUnderAttack | default |
| 0x004d9911 | (FootClass area) | EVA_UnitLost | default |
| 0x0071b05f | TemporalClass__InitiateWarp | EVA_OreMinerUnderAttack | default |
| 0x00738530 | UnitClass__Mission_Harvest | EVA_OreMinerUnderAttack | default |

Note: HouseClass__BaseUnderAttack plays EVA_OreMinerUnderAttack for harvesters,
and EVA_OurAllyIsUnderAttack/EVA_OurBaseIsUnderAttack for regular bases. The
"EVA_OurBaseIsUnderAttack" string is at 0x824768 but is loaded via a
conditional branch not captured as a separate xref (same function, different path).

### Building Events

| Call Address | Caller Function | EVA Event | Type Override |
|-------------|-----------------|-----------|---------------|
| 0x00452355 | BuildingClass__GoOnline | EVA_BuildingOnLine | default |
| 0x004523de | BuildingClass__GoOffline | EVA_BuildingOffLine | default |
| 0x0044fd1a | BuildingClass__ReadFromINI | EVA_BuildingOnLine | default |
| 0x004582d8 | BuildingClass__CheckAutoSellOrCivilian | EVA_StructureAbandoned | default |
| 0x005229c1 | BuildingClass__AddGarrisonOccupant | EVA_StructureGarrisoned | default |
| 0x00443a69 | BuildingClass__SetRallyPoint | EVA_NewRallyPointEstablished | default |
| 0x004470b7 | (BuildingClass repair area) | EVA_Repairing | default |
| 0x00448226 | (BuildingClass area) | EVA_PrimaryBuildingSelected | default |
| 0x004f8ba0 | HouseClass__Update | EVA_InsufficientFunds | default |
| 0x004f8d14 | HouseClass__Update | EVA_LowPower | default |
| 0x0044b973 | BuildingClass__MissionRepairAndProduce | EVA_UnitRepaired | default |
| 0x0044bdc5 | BuildingClass__MissionRepairAndProduce | EVA_UnitRepaired | default |
| 0x0044bfea | BuildingClass__MissionRepairAndProduce | EVA_InsufficientFunds | default |
| 0x0044c507 | BuildingClass__MissionRepairAndProduce | EVA_Repairing | default |

### Selling Events

| Call Address | Caller Function | EVA Event | Type Override |
|-------------|-----------------|-----------|---------------|
| 0x00449ce5 | BuildingClass__Sell | EVA_StructureSold | default |
| 0x0044ab36 | BuildingClass__Sell | EVA_StructureSold | default |
| 0x004d9f94 | (FootClass sell area) | EVA_UnitSold | default |

### Capture Events

| Call Address | Caller Function | EVA Event | Type Override |
|-------------|-----------------|-----------|---------------|
| 0x00448428 | BuildingClass__ChangeOwner | EVA_TechBuildingLost | default |
| 0x0044848a | BuildingClass__ChangeOwner | EVA_BuildingCaptured | default |

### Infiltration Events

| Call Address | Caller Function | EVA Event | Type Override |
|-------------|-----------------|-----------|---------------|
| 0x00457288 | BuildingClass__OnSpyInfiltrate | EVA_RadarSabotaged | default |
| 0x00457338 | BuildingClass__OnSpyInfiltrate | EVA_BuildingInfiltrated | default |
| 0x00457481 | BuildingClass__OnSpyInfiltrate | EVA_CashStolen | default |
| 0x004574f9 | BuildingClass__OnSpyInfiltrate | EVA_TechnologyStolen | default |
| 0x0045755a | BuildingClass__OnSpyInfiltrate | EVA_TechnologyStolen | default |
| 0x0045757c | BuildingClass__OnSpyInfiltrate | EVA_BuildingInfiltrated | default |
| 0x0045758b | BuildingClass__OnSpyInfiltrate | EVA_NewTechnologyAcquired | default |
| 0x00519bc9 | InfantryClass__Mission_Enter | EVA_BridgeRepaired | default |

Note: The InfantryClass__Mission_Enter call plays EVA_BridgeRepaired when an
engineer enters a bridge hut, not an infiltration event per se.

### Alliance Events

| Call Address | Caller Function | EVA Event | Type Override |
|-------------|-----------------|-----------|---------------|
| 0x004f9f35 | HouseClass__MakeAlly | EVA_AllianceRequested | default |
| 0x004fa1d5 | HouseClass__BreakAlliance | EVA_AllianceBroken | default |

Note: EVA_AllianceFormed, EVA_RequestingAlliance, and EVA_EnemyAllianceFormed
are defined in evamd.ini but their string constants exist in the binary (0x8247e4,
0x8247b4, 0x8247cc). They may be triggered from map trigger actions or other
code paths not captured as direct PlayEVA calls.

### Victory/Defeat Events

| Call Address | Caller Function | EVA Event | Type Override |
|-------------|-----------------|-----------|---------------|
| 0x004fcba9 | HouseClass__Flag_To_Win | EVA_YouAreVictorious | 0 (STANDARD) |
| 0x004fcda1 | HouseClass__Flag_To_Lose | EVA_YouHaveLost | default |
| 0x004fc2ea | HouseClass__MPlayer_Defeated | EVA_YouHaveLost | default |
| 0x004fc3bc | HouseClass__MPlayer_Defeated | EVA_PlayerDefeated | default |

Note: EVA_YouAreVictorious is the ONLY call site that explicitly overrides type=0 (STANDARD).

### Superweapon Detected Events (in BuildingClass__OnConstructionComplete)

The function at 0x00446950 uses a jump table (switch on SuperWeaponType index 0-9):

| Call Address | SuperWeapon Type | EVA Event |
|-------------|------------------|-----------|
| 0x00446995 (switch) | 0 (Nuke) | EVA_NuclearSiloDetected |
| 0x00446995 (switch) | 1 (IronCurtain) | EVA_IronCurtainDetected |
| 0x00446995 (switch) | 2 (WeatherDevice) | EVA_WeatherDeviceReady |
| 0x00446995 (switch) | 7 (Chronosphere) | EVA_ChronosphereDetected |
| 0x00446995 (switch) | 10 (GeneticMutator) | EVA_GeneticMutatorDetected |
| 0x00446995 (switch) | 11 (PsychicDominator) | EVA_PsychicDominatorDetected |

### Superweapon Ready Events (in SuperClass__AI_Ready 0x6cbca0 and SuperClass__AI_Charging 0x6cc080)

Both functions share the same switch table structure on SuperWeaponTypeClass offset 0xB4:

| Switch Case | EVA Event | String Address |
|------------|-----------|----------------|
| 0 | EVA_NuclearMissileReady | 0x8424d4 |
| 1 | EVA_IronCurtainReady | 0x8424bc |
| 2 | EVA_ForceShieldReady | 0x8424a4 |
| 3 | EVA_LightningStormReady | 0x84248c |
| 5, 6 | EVA_PsychicDominatorReady | 0x842470 |
| 7 | EVA_ChronosphereReady | 0x842458 |
| 8 | EVA_ReinforcementsReady | 0x842440 |
| 9 | EVA_SpyPlaneReady | 0x84242c |
| 10 | EVA_GeneticMutatorReady | 0x842414 |
| 11 | EVA_PsychicRevealReady | 0x8423fc |
| 4 | (no EVA, falls through to default) | -- |

### Superweapon Launch Events (in SuperClass__Launch at 0x6cc390)

| Call Address | EVA Event |
|-------------|-----------|
| 0x006ccd03 | EVA_ChronosphereActivated |
| 0x006ccd81 | EVA_LightningStormCreated |
| 0x006ccdfa | EVA_PsychicDominatorActivated |
| 0x006ccf21 | EVA_IronCurtainActivated |
| 0x006cd8bd | EVA_GeneticMutatorActivated |
| 0x006cdc98 | EVA_NuclearMissileLaunched |
| 0x006cde01 | EVA_NuclearMissileLaunched |

### Upgrade/Crate Events

| Call Address | Caller Function | EVA Event |
|-------------|-----------------|-----------|
| 0x006fa0cb | TechnoClass__AI_Update | EVA_UnitPromoted |
| 0x006fa139 | TechnoClass__AI_Update | EVA_UnitPromoted |
| 0x00482ebb | (Crate pickup handler) | EVA_UnitArmorUpgraded |
| 0x004830aa | (Crate pickup handler) | EVA_UnitSpeedUpgraded |
| 0x0048328a | (Crate pickup handler) | EVA_UnitFirePowerUpgraded |

### Miscellaneous Events

| Call Address | Caller Function | EVA Event | Type Override |
|-------------|-----------------|-----------|---------------|
| 0x00430d78 | RadarClass__PlaceBeacon | EVA_BeaconPlaced | default |
| 0x00430f1b | RadarClass__PlaceBeacon | EVA_BeaconDetected | default |
| 0x0053ab11 | LightningStorm__Process | EVA_LightningStormCreated | default |
| 0x00686616 | GameExit__BattleControlTerminated | EVA_BattleControlTerminated | 2 (INTERRUPT) |
| 0x0050e0d0 | HouseClass__RobotTanksBackOnline | EVA_RobotTanksBackOnline | default |
| 0x0050e19b | HouseClass__RobotTanksOffline | EVA_RobotTanksOffline | default |
| 0x00502776 | HouseClass__Removed_From_Game | EVA_RobotTanksOffline | default |
| 0x004abc80 | DisplayClass__BandBox_LeftUp | EVA_CannotDeployHere | default |
| 0x0073950a | UnitClass__Deploy | EVA_CannotDeployHere | default |

### Events with No Programmatic Trigger in Binary

These EVA events are defined in evamd.ini but their strings don't appear as call
arguments in gamemd.exe. They are triggered by map trigger actions or scripting:

- EVA_BattlefieldControlOnline (no string constant in .exe)
- EVA_MissionAccomplished (string at 0x824c54, likely via trigger action)
- EVA_MissionFailed (string at 0x824cac, likely via trigger action)
- EVA_IncomingTransmission (string at 0x8392c0, stored as global by RadarClass)

---

## 5. Faction Selection -- VoxClass__SetSide Internals

### Function (0x007534e0)

```c
void __fastcall VoxClass__SetSide(int side) {
    if (side == -1) {
        CurrentSide = 0;  // default to Allied
        return;
    }
    CurrentSide = side;
}
```

Trivial function. The side parameter is:

| Value | Faction | Sound Field Used | Example |
|-------|---------|-----------------|---------|
| 0 | Allied | offset 0x3E (AlliedSound) | "ceva048" |
| 1 | Russian/Soviet | offset 0x35 (RussianSound) | "csof048" |
| 2 | Yuri | offset 0x2C (YuriSound) | "cyur048" |
| -1 | (invalid/default) | remapped to 0 (Allied) | |

### Caller: InitSideMixFiles (0x00534fa0)

Called once during game initialization. The side index is passed via ECX (__fastcall)
from the game's side selection. The function also loads the side-specific MIX files
(e.g., langmd.mix, audiomd.mix for the correct language/faction sounds).

### Sound Field Selection in PlayNextQueued

```c
// In PlayNextQueued at 0x7528e8:
switch (CurrentSide) {
    case 0:  soundName = vox + 0x3E;  break;  // Allied
    case 1:  soundName = vox + 0x35;  break;  // Russian
    default: soundName = vox + 0x2C;  break;  // Yuri (or any other value)
}
```

Note: The default case maps to Yuri (offset 0x2C), meaning any side value >= 2
(including invalid values) uses the Yuri sound.

---

## 6. StreamPlayer -- Audio Engine Internals

### StreamPlayer Struct (size = 0xF8 = 248 bytes)

Allocated by `StreamPlayer__Create` (0x407860) via `operator_new(0xF8)`.

```
Offset  Size  Type             Field            Description
------  ----  ---------------  ---------------  -----------
0x00    16    LinkedList       ListNode         Linked list membership (for global tracking)
0x0C    4     uint             Flags            Bit 0 = playing, Bit 1 = paused, Bit 2 = has data
0x10    4     ???              (reserved)
0x14    4     AudioDriver*     Driver           Pointer to DirectSound driver wrapper
0x18    4     int              BufferSizeLo     Low buffer size parameter
0x1C    4     int              BufferSizeHi     High buffer size parameter
0x20    8     int64            LastEndTime      QPC timestamp when last playback ended
0x28    8     int64            StartTime        QPC timestamp when playback started
0x30    4     ???              (unknown)
0x34    4     DSBuffer*        Buffer1          DirectSound secondary buffer #1
0x38    4     DSBuffer*        Buffer2          DirectSound secondary buffer #2
0x3C    4     int              PlayPosition     Current write position in buffer
0x40    4     int              BufferSize       Actual buffer size in bytes
0x44-0x4F     ???              (internal state)
0x50    4     void*            CallbackData     Callback context pointer
0x5C    36    ???              (unknown block)
0x80    4     int              PauseCounter     Nested pause counter (0 = playing)
0x84    4     ???              (unknown)
0x88    4     int              Destroyed         1 if StreamPlayer has been destroyed
0x8C    4     ???              (unknown)
0x90    4     int              RemainingBytes   Bytes remaining to stream
0x94    8     ???              WavFormat        WAVEFORMATEX data
0x9C    4     int              DataSize         Total audio data size (bytes)
0xA0    4     int              ChunkSize        Size of each streaming chunk
0xA4    80    char[80]         Name             Display name (default "Audio Stream")
0xF3    1     char             NameTerminator   Null terminator forced at 0xF3
0xF4    4     ???              (tail padding)
```

### Creation (StreamPlayer__Create at 0x407860)

```c
StreamPlayer* StreamPlayer__Create(int driver, uint bufferSize, int param3) {
    if (driver == 0) return NULL;
    if (param3 == 0 && (bufferSize == 0 || bufferSize < 7000)) {
        bufferSize = 7000;  // minimum buffer size
    }

    StreamPlayer* sp = new StreamPlayer();  // 0xF8 bytes, zeroed
    LinkedList_Init(sp);
    sp->Driver = driver;
    sp->AudioContext = CreateAudioContext(driver);
    sp->BufferSizeLo = bufferSize;
    sp->BufferSizeHi = param3;
    // ... setup DirectSound format, create buffers ...
    strncpy(sp->Name, "Audio Stream", 80);
    // ... register callbacks for streaming fill ...
    return sp;
}
```

### EVA StreamPlayer Configuration

- **Buffer size**: 3000 bytes (passed from VoiceSystem__Init)
- **Minimum enforced**: 7000 bytes (clamped in Create if < 7000 with param3=0)
- **Actual buffer**: 7000 bytes (since 3000 < 7000, minimum applies)
- **Format**: Determined by WAV header parsing of each played file
- **Double-buffered**: Two DirectSound secondary buffers for gapless streaming

### How StreamPlayer__PlayFile Works (0x407b60)

```c
int StreamPlayer__PlayFile(StreamPlayer* sp, char* filename, int fromMix) {
    if (sp->Destroyed) return 0;

    // Stop any current playback
    if (sp->Driver) {
        EnterCriticalSection(&AudioCS);
        StopPlayback(sp);
        LeaveCriticalSection(&AudioCS);
    }

    // Release old file handle
    if (sp->FileHandle) {
        sp->FileHandle->Release();
        sp->FileHandle = NULL;
    }

    // Open file -- try raw first, then MIX lookup
    if (fromMix == 0) {
        sp->FileHandle = new RawFileClass(filename);
        if (!sp->FileHandle->Open(READ)) {
            sp->FileHandle->Release();
            sp->FileHandle = NULL;
        }
    }
    if (sp->FileHandle == NULL) {
        sp->FileHandle = new CCFileClass(filename);  // MIX archive lookup
    }

    if (sp->FileHandle == NULL) return 0;
    if (!sp->FileHandle->Open(READ)) { ... cleanup; return 0; }

    // Parse WAV/AUD header
    if (!WAV__ParseHeader(sp->FileHandle, &sp->WavFormat)) { ... cleanup; return 0; }

    sp->DataSize = sp->WavFormat.dataSize;
    sp->RemainingBytes = sp->DataSize;
    // ... create DirectSound buffers matching format ...
    // ... fill initial buffer data ...
    // ... start playback ...

    sp->StartTime = GetPerformanceTimestamp();
    sp->LastEndTime = sp->StartTime;
    sp->Flags |= 0x5;  // playing + has data
    return 1;
}
```

### Key Differences from Normal SFX

1. **Streaming vs loaded**: StreamPlayer streams from disk/MIX in chunks.
   Normal SFX loads entire samples into DirectSound buffers.
2. **Dedicated channel**: EVA has its own StreamPlayer instance, separate from
   the SFX secondary buffer pool.
3. **Sequential**: Only one EVA can play at a time. SFX can have many simultaneous.
4. **CriticalSection**: StreamPlayer uses Windows CriticalSection for thread safety
   (the streaming fill callback runs on a timer/thread).
5. **Format auto-detection**: Parses WAV headers at playback time. Despite the
   ".WAV" extension appended to filenames, the actual files are IMA ADPCM (.aud).
   The WAV parser handles both formats.

### Interruption Handling

When a new EVA fires with Type=INTERRUPT:
1. `StreamPlayer__Stop(EVAStreamPlayer)` -- stops current playback immediately
2. All queues are cleared
3. InterAnnouncementDelay is reset to 0
4. The interrupt event is then queued and played immediately

For normal queue transitions, the current EVA finishes naturally, then after the
500ms delay, the next one plays. There is NO crossfade or truncation for normal
queue flow.

---

## 7. Suspend/Pause Mechanism -- Complete

### Suspend (blocks new events from being queued)

**SuspendEVA** (0x753570):
```c
void VoxClass__SuspendEVA() {
    SuspendCounter++;     // at 0xb1d3d8
}
```

**ResumeEVA** (0x753580):
```c
void VoxClass__ResumeEVA() {
    if (SuspendCounter != 0) {
        SuspendCounter--;
        if (SuspendCounter < 0) SuspendCounter = 0;  // safety clamp
    }
}
```

**Effect**: In QueueVoice, `if (SuspendCounter > 0) return;` -- new events silently dropped.
Events already in the queue continue to play. The currently playing EVA continues.

**Callers of SuspendEVA**:
| Address | Function | Context |
|---------|----------|---------|
| 0x0053b554 | Process_QueuedEvents_WithSuspend | Suspends during queued network event processing |
| 0x006579c0 | RadarClass__PlayRadarMovie | Suspends during radar animation movies |

**Callers of ResumeEVA**:
| Address | Function | Context |
|---------|----------|---------|
| 0x0053b714 | Process_QueuedEvents | After queued event processing completes |
| 0x0065792b | RadarClass__PlayRadarMovie | After radar movie finishes |
| 0x006539b7 | Minimap_Chat_Dispatch | After minimap chat processing |

### Pause (freezes playback, preserves queue)

**PauseEVA** (0x7535b0):
```c
void VoxClass__PauseEVA() {
    if (EVAStreamPlayer != NULL) {
        StreamPlayer__Pause(EVAStreamPlayer);  // stops DirectSound playback
    }
    PauseCounter++;     // at 0xb1d428
}
```

**UnpauseEVA** (0x753620):
```c
void VoxClass__UnpauseEVA() {
    if (EVAStreamPlayer != NULL) {
        StreamPlayer__Resume(EVAStreamPlayer);  // resumes DirectSound playback
    }
    if (PauseCounter != 0) {
        PauseCounter--;
        if (PauseCounter < 0) PauseCounter = 0;  // safety clamp
    }
}
```

**Effect**: In PlayNextQueued, `if (PauseCounter > 0) return;` -- no new EVAs start playing,
but queued events are preserved. The currently playing EVA is paused (DirectSound stopped),
and resumes from where it left off when unpaused.

**Callers of PauseEVA**:
| Address | Function | Context |
|---------|----------|---------|
| 0x005bee08 | FUN_005bed40 (Play_Movie) | Before movie/cutscene playback (Bink/VQA) |
| 0x005bf069 | FUN_005bed40 (Play_Movie) | Before movie/cutscene playback (alternate path) |
| 0x00406f27 | GamePause__Enter | When player presses Pause (game pause screen) |
| 0x005bf589 | Audio__PauseForMovie | Standalone pause for movie (called separately) |

**Callers of UnpauseEVA**:
| Address | Function | Context |
|---------|----------|---------|
| 0x005bee9e | FUN_005bed40 (Play_Movie) | After movie finishes |
| 0x005bf105 | FUN_005bed40 (Play_Movie) | After movie finishes (alternate path) |
| 0x00406f45 | GamePause__Exit | When player exits pause screen |
| 0x005bf4ea | Audio__ResumeAfterMovieAndCleanup | After movie + cleanup |
| 0x005bf568 | Audio__ResumeAfterMovie | After movie (simple resume) |

### Key Differences

| | Suspend | Pause |
|---|---------|-------|
| **Mechanism** | Nested counter at 0xb1d3d8 | Nested counter at 0xb1d428 |
| **New events** | Silently dropped | Still queued normally |
| **Current playback** | Continues playing | Frozen mid-stream (DirectSound paused) |
| **Queue** | Preserved, continues draining | Preserved, frozen |
| **Use case** | Brief suppression (radar movie, event processing) | Full pause (game pause, cutscene) |
| **StreamPlayer** | Not touched | Pause/Resume called on DirectSound |

### ResetAll (0x7535d0)

```c
void VoxClass__ResetAll() {
    if (CurrentlyPlaying != NULL) {
        CurrentlyPlaying->PlayState = 2;  // DONE
        CurrentlyPlaying = NULL;
    }
    if (EVAStreamPlayer != NULL) {
        StreamPlayer__Stop(EVAStreamPlayer);
    }
    VoxClass__ClearAllQueues();
    PauseCounter = 0;
    SuspendCounter = 0;
}
```

Called from the game exit handler (0x69bb40) when ending a game session.
Clears everything: stops playback, empties all queues, resets both counters.

---

## 8. The 500ms Inter-Announcement Delay

### Exact Mechanism

The delay is stored as a 64-bit value at `0x00b1d4d0` (low 32 bits) and
`0x00b1d4d4` (high 32 bits).

**Setting the delay** (in PlayNextQueued at 0x75296d):
```asm
MOV dword ptr [0x00b1d4d0], 0x1F4    ; 500 decimal
MOV dword ptr [0x00b1d4d4], 0x0      ; high dword = 0
```

**Checking the delay** (in PlayNextQueued at 0x7527a1):
```c
int64 endTime = StreamPlayer__GetEndTime(EVAStreamPlayer);  // last play end timestamp
int64 delay = *(int64*)0xb1d4d0;                             // 500
uint64 now = GetPerformanceTimestamp();                       // current time in ms

if ((uint64)(endTime + delay) >= now) {
    return;  // too soon, wait
}
```

### Time Units: MILLISECONDS

Confirmed via `Timer__InitPerformanceCounter` (0x409360):

```c
void Timer__InitPerformanceCounter() {
    LARGE_INTEGER freq;
    if (QueryPerformanceFrequency(&freq)) {
        // Store freq / 1000 as the divisor
        TicksPerMs = freq / 1000;
        TimerFunc = &GetPerfCounterInMs;  // returns QPC / TicksPerMs
    }
}
```

`GetPerformanceTimestamp()` returns **milliseconds** (QPC ticks divided by frequency/1000).
Therefore, the delay of 500 = **500 milliseconds**.

### Hardcoded, Not Configurable

The value 0x1F4 (500) is an immediate constant in the instruction stream, not read from
INI or any configurable source. It cannot be changed without binary patching.

### When Delay is Cleared

The delay is set to 0 in two situations:
1. When an INTERRUPT event fires (clears delay to allow immediate playback)
2. Implicitly at first playback (endTime starts at 0, so delay check passes)

---

## 9. Taunt System

### Architecture

Taunts use a **completely separate** audio path from EVA:

| | EVA System | Taunt System |
|---|-----------|-------------|
| StreamPlayer | 0xb1d4cc (EVAStreamPlayer) | 0xb1d4d8 (SpeechStreamPlayer) |
| Init function | VoiceSystem__Init (0x752290) | SpeechSystem__Init (0x752ad0) |
| Play function | VoxClass__PlayEVA | SpeechSystem__PlayTaunt (0x752b70) |
| Buffer size | 3000 (min 7000) | 3000 (min 7000) |
| Source | evamd.ini [DialogList] | Hardcoded country format strings |

### Taunt ID Encoding

The taunt ID is a single byte with two packed fields:
```
Bits 0-3: Taunt number (1-8 valid, 0 and 9+ rejected)
Bits 4-7: Country index (0-9, selects format string)
```

### Country Format Strings (switch at 0x752b90)

| Case | Country | Format String | Example |
|------|---------|---------------|---------|
| 0 | American | `taunts\tauam%02i.wav` | taunts\tauam01.wav |
| 1 | Korean | `taunts\tauko%02i.wav` | taunts\tauko01.wav |
| 2 | French | `taunts\taufr%02i.wav` | taunts\taufr01.wav |
| 3 | German | `taunts\tauge%02i.wav` | taunts\tauge01.wav |
| 4 | British | `taunts\taubr%02i.wav` | taunts\taubr01.wav |
| 5 | Libyan | `taunts\tauli%02i.wav` | taunts\tauli01.wav |
| 6 | Iraqi | `taunts\tauir%02i.wav` | taunts\tauir01.wav |
| 7 | Cuban | `taunts\taucu%02i.wav` | taunts\taucu01.wav |
| 8 | Russian | `taunts\tauru%02i.wav` | taunts\tauru01.wav |
| 9 | Yuri | `taunts\tauyu%02i.wav` | taunts\tauyu01.wav |

### Taunt Triggers

| Caller Address | Function | Context |
|---------------|----------|---------|
| 0x0064a75e | FUN_0064a630 (NetworkMessage handler) | Network message type 0x78 (taunt request) |
| 0x0048da3b | FUN_0048d1e0 | Crate/powerup handler |
| 0x00536438 | (game init area) | Startup |

Taunts are triggered by multiplayer network messages. When a player sends a taunt
(via F5-F12 keys), a network packet with type 0x78 is sent. The receiver's game
calls SpeechSystem__PlayTaunt with the encoded country+taunt byte.

### Taunt Guard: DAT_00b1d480

```c
if (SpeechStreamPlayer != NULL && DAT_00b1d480 == 0) {
    // ... play taunt
}
```

DAT_00b1d480 is initialized to 0 by SpeechSystem__Init. When non-zero, taunts are blocked.
This address sits immediately after the PriorityQueue array (0xb1d450 + 4*12 = 0xb1d480).
In practice, it is always 0 during normal gameplay -- no code path sets it to non-zero.
It may be a vestigial field or future expansion point.

### Taunts Cannot Interrupt EVA

The EVA and taunt StreamPlayers are completely independent. Taunts play on their own
audio channel and do not affect the EVA queue or playback in any way. Both can play
simultaneously without interference.

### Taunt Pause Behavior

GamePause__Enter calls `SpeechSystem__Pause` (0x753500) which pauses the SpeechStreamPlayer.
GamePause__Exit calls `SpeechSystem__Resume` (0x753510) which resumes it.
So taunts are paused during the game pause screen, just like EVA.

---

## 10. Volume Control

### Volume Architecture

EVA volume is controlled by the **VoiceVolume** slider in the Options menu, which is
separate from SoundVolume (SFX) and ScoreVolume (music).

### Options Struct Fields

```
Options + 0x38 (param_1[0x0E]): SoundVolume (float 0.0-1.0)
Options + 0x3C (param_1[0x0F]): VoiceVolume (float 0.0-1.0)
Options + 0x40 (param_1[0x10]): ScoreVolume (float 0.0-1.0)
```

Serialized to RA2MD.INI under `[Audio]` section:
```ini
[Audio]
SoundVolume=0.5
VoiceVolume=0.8
ScoreVolume=0.3
```

### Volume Flow

```
User adjusts VoiceVolume slider
    |
    v
OptionsClass__SetVoiceVolume (0x5fa590)
    |
    +-- Clamps to [0.0, 1.0]
    +-- Stores float at Options+0x3C
    |
    +-- Converts float to int (0-255) via Math__ftol
    +-- Calls VoxClass__SetGlobalVolume(intVolume)  (0x752ab0)
    |       |
    |       +-- Stores at DAT_00846614 (clamped to max 255)
    |
    +-- Converts float to int again
    +-- Calls FUN_00407150(DirectSoundMixer, intVolume)
    |       |
    |       +-- Sets volume on the global DirectSound mixer at 0x87e740
    |       +-- This affects ALL streaming audio (EVA + taunts)
    |
    +-- Calls CreditUpDown_Sound (UI feedback sound for slider)
```

### Global Volume Variable

| Address | Type | Value | Description |
|---------|------|-------|-------------|
| 0x00846614 | int | 0-255 | Global EVA volume level (0=silent, 255=max) |

This value is set by `VoxClass__SetGlobalVolume` (0x752ab0):
```c
void VoxClass__SetGlobalVolume(int volume) {
    g_EVAVolume = volume;
    if (volume > 254) g_EVAVolume = 255;
}
```

### Per-Event Volume

Each VoxClass has a `Volume` field (float at offset 0x28, default 1.0). However,
**this per-event volume is NOT used during playback**. In PlayNextQueued, the volume
applied to the StreamPlayer is the global VoiceVolume from the Options, not the
per-event Volume field.

The per-event Volume field exists in the struct and is read from INI, but the
original engine does not appear to use it for per-event volume scaling. It may be
a vestigial feature from an earlier version of the engine or reserved for mod use.

### Volume Applied To

The VoiceVolume setting affects:
1. **EVA StreamPlayer** (0xb1d4cc) -- via the DirectSound mixer
2. **Speech StreamPlayer** (0xb1d4d8) -- same mixer affects both

There is no separate volume control for taunts vs EVA. They share the VoiceVolume slider.

---

## Global Variable Map (Complete)

| Address | Size | Type | Name | Description |
|---------|------|------|------|-------------|
| 0xb1d3b8 | 4 | int | CurrentPlayingType | Type of currently playing EVA entry |
| 0xb1d3c8 | 12 | ListHead | InterruptQueue | Queue for Type=3 (QUEUED_INTERRUPT) |
| 0xb1d3d8 | 4 | int | SuspendCounter | Nested counter; EVA suppressed when > 0 |
| 0xb1d3e0 | 4 | int | CurrentPlayingPriority | Priority of currently playing EVA entry |
| 0xb1d3f0 | 12 | ListHead | CriticalQueue | Queue for Priority=CRITICAL non-QUEUE types |
| 0xb1d428 | 4 | int | PauseCounter | Nested counter; playback frozen when > 0 |
| 0xb1d450 | 12 | ListHead | PriorityQueue[0] | Queue for Type=QUEUE, Priority=LOW |
| 0xb1d45c | 12 | ListHead | PriorityQueue[1] | Queue for Type=QUEUE, Priority=NORMAL |
| 0xb1d468 | 12 | ListHead | PriorityQueue[2] | Queue for Type=QUEUE, Priority=IMPORTANT |
| 0xb1d474 | 12 | ListHead | PriorityQueue[3] | Queue for Type=QUEUE, Priority=CRITICAL |
| 0xb1d480 | 4 | int | TauntGuard | When non-zero, blocks taunt playback (always 0) |
| 0xb1d4a0 | 4 | void* | VoxArray_VTable | VTable for DynamicVectorClass |
| 0xb1d4a4 | 4 | VoxClass** | VoxArray_Data | Pointer to array of VoxClass pointers |
| 0xb1d4a8 | 4 | int | VoxArray_Capacity | Allocated capacity |
| 0xb1d4b0 | 4 | int | VoxArray_Count | Number of loaded entries |
| 0xb1d4b8 | 4 | QueueNode* | PendingImmediate | Single pending STANDARD/INTERRUPT entry |
| 0xb1d4bc | 4 | int | SystemEnabled | 1 = EVA system active |
| 0xb1d4c0 | 4 | int | SequenceCounter | Monotonic counter for queue ordering |
| 0xb1d4c4 | 4 | VoxClass* | CurrentlyPlaying | VoxClass currently being played |
| 0xb1d4c8 | 4 | int | CurrentSide | 0=Allied, 1=Russian, 2=Yuri |
| 0xb1d4cc | 4 | StreamPlayer* | EVAStreamPlayer | Dedicated audio stream for EVA |
| 0xb1d4d0 | 4 | int | InterAnnouncementDelay_Lo | Low 32 bits of delay (500ms) |
| 0xb1d4d4 | 4 | int | InterAnnouncementDelay_Hi | High 32 bits of delay |
| 0xb1d4d8 | 4 | StreamPlayer* | SpeechStreamPlayer | Separate stream for taunts |
| 0x846614 | 4 | int | g_EVAVolume | Global EVA volume (0-255) |

---

## All Functions Labeled in Ghidra

| Address | Name | Description |
|---------|------|-------------|
| 0x00407860 | StreamPlayer__Create | Allocates and initializes StreamPlayer (0xF8 bytes) |
| 0x00407b60 | StreamPlayer__PlayFile | Opens file, parses WAV, streams via DirectSound |
| 0x00407f40 | StreamPlayer__Stop | Stops current playback |
| 0x00407fb0 | StreamPlayer__Pause | Pauses DirectSound playback (nested counter) |
| 0x00408000 | StreamPlayer__Resume | Resumes DirectSound playback (nested counter) |
| 0x00408070 | StreamPlayer__IsPlaying | Returns Flags & 1 |
| 0x00408140 | StreamPlayer__GetEndTime | Returns 64-bit end timestamp |
| 0x00409360 | Timer__InitPerformanceCounter | Inits QPC with ms conversion (freq/1000) |
| 0x004093b0 | GetPerformanceTimestamp | Returns current time in milliseconds |
| 0x00406f00 | GamePause__Enter | Pauses EVA + speech + game audio |
| 0x00406f40 | GamePause__Exit | Resumes EVA + speech + game audio |
| 0x00430ba0 | RadarClass__PlaceBeacon | Beacon placement (EVA_BeaconPlaced/Detected) |
| 0x00449c30 | BuildingClass__Sell | Building sell handler (EVA_StructureSold) |
| 0x0050e010 | HouseClass__RobotTanksBackOnline | Robot tanks come back online |
| 0x0050e0e0 | HouseClass__RobotTanksOffline | Robot tanks go offline |
| 0x0053a6c0 | LightningStorm__Process | Lightning storm active phase |
| 0x0053b460 | Process_QueuedEvents_WithSuspend | Network event processing with EVA suspend |
| 0x005bf450 | Audio__ResumeAfterMovieAndCleanup | Resume all audio + cleanup after movie |
| 0x005bf530 | Audio__ResumeAfterMovie | Resume all audio after movie |
| 0x005bf580 | Audio__PauseForMovie | Pause all audio for movie playback |
| 0x005fa590 | OptionsClass__SetVoiceVolume | Sets VoiceVolume from UI slider |
| 0x00686570 | GameExit__BattleControlTerminated | Game exit with EVA_BattleControlTerminated |
| 0x006cbca0 | SuperClass__AI_Ready | Superweapon ready handler (EVA_*Ready events) |
| 0x006cc080 | SuperClass__AI_Charging | Superweapon charging handler (EVA_*Ready events) |
| 0x00737c90 | UnitClass__Mission_Harvest | Harvester mission (EVA_OreMinerUnderAttack) |
| 0x00752290 | VoiceSystem__Init | Creates EVA StreamPlayer, inits queues |
| 0x00752370 | VoxClass__ClearAllQueues | Empties all priority queues |
| 0x00752460 | VoxClass__GetByIndex | Array index lookup |
| 0x00752480 | VoxClass__QueueVoice | Queue dispatch with type/priority resolution |
| 0x00752590 | VoxClass__InsertIntoQueue | Routes to correct queue by type/priority |
| 0x00752680 | VoxClass__FindInQueues | Searches all queues for a VoxClass |
| 0x00752700 | VoxClass__PlayEVA | Main entry point: name lookup + queue |
| 0x00752760 | VoxClass__PlayNextQueued | Dequeues and plays next EVA |
| 0x007529e0 | VoxClass__PumpAndCheckActive | Pumps playback, returns 1 if anything active |
| 0x00752a40 | VoxClass__RemoveFromQueues | Removes all instances of a VoxClass from queues |
| 0x00752ab0 | VoxClass__SetGlobalVolume | Sets g_EVAVolume (0-255) |
| 0x00752ad0 | SpeechSystem__Init | Creates Speech StreamPlayer for taunts |
| 0x00752b40 | SpeechSystem__Shutdown | Destroys Speech StreamPlayer |
| 0x00752b70 | SpeechSystem__PlayTaunt | Plays taunt by encoded country+number |
| 0x00752ca0 | SpeechSystem__Stop | Stops Speech StreamPlayer |
| 0x00752db0 | VoxClass__ReadINI | Reads per-section INI data |
| 0x00753000 | VoxClass__ReadEVAINI | Parses [DialogList], creates VoxClass entries |
| 0x007531a0 | VoxClass__ClearAllEntries | Destroys all VoxClass objects |
| 0x007532d0 | VoxClass__FindByName | Name lookup (filters `<none>`) |
| 0x007533f0 | VoxClass__LoadFromSave | Restores queue state from savegame |
| 0x007534e0 | VoxClass__SetSide | Sets CurrentSide (0/1/2) |
| 0x00753500 | SpeechSystem__Pause | Pauses Speech StreamPlayer |
| 0x00753510 | SpeechSystem__Resume | Resumes Speech StreamPlayer |
| 0x00753570 | VoxClass__SuspendEVA | Increments SuspendCounter |
| 0x00753580 | VoxClass__ResumeEVA | Decrements SuspendCounter |
| 0x007535b0 | VoxClass__PauseEVA | Pauses EVA StreamPlayer + increments PauseCounter |
| 0x00753620 | VoxClass__UnpauseEVA | Resumes EVA StreamPlayer + decrements PauseCounter |
| 0x007535d0 | VoxClass__ResetAll | Full reset: stop, clear, zero counters |

---

## Save/Load Format

### VoxClass__LoadFromSave (0x7533f0)

The save format uses a tagged binary stream:

```
Header:     4 bytes  "VoxS" (0x566F7853) -- start marker
Loop:
  Tag:      4 bytes  "VoxI" (0x566F7849) -- item marker
  Data:     12 bytes:
    Offset 0: int VoxArrayIndex  (index into VoxArray)
    Offset 4: int Priority       (queue priority)
    Offset 8: int Type           (queue type)
End:        4 bytes  "VoxE" (0x566F7845) -- end marker
```

On load:
1. Stop current playback
2. Clear all queues
3. Read "VoxS" header
4. For each "VoxI" tag, read the 12-byte record and call InsertIntoQueue
5. Stop at "VoxE" tag

---

## Implementation Notes for Rust Engine

1. **VoxClass is simple**: 84-byte struct with fixed-size char arrays. Map directly to
   a Rust struct with `[u8; 40]` for name, `[u8; 9]` for sound names, `f32` for volume,
   and enums for type/priority/play_state.

2. **Queue system**: Use `VecDeque<QueueEntry>` for each of the 6 queues plus an
   `Option<QueueEntry>` for PendingImmediate. FIFO ordering within each queue.

3. **500ms delay**: Use `Instant::now()` and `Duration::from_millis(500)` for the
   inter-announcement gap. Compare against the last playback end time.

4. **Suspend vs Pause**: Two independent `u32` counters. Suspend blocks queueing,
   Pause blocks playback. Both support nesting.

5. **Volume**: Use a single `u8` (0-255) for EVA volume mapped from the VoiceVolume
   slider. The per-event Volume field from INI can be stored but is not functionally
   used by the original engine.

6. **Taunts are separate**: Completely independent audio channel with its own StreamPlayer.
   Triggered by multiplayer network messages, not by the EVA queue system.

7. **".WAV" extension is cosmetic**: The engine appends ".WAV" to sound names but the
   actual files in MIX archives are .aud (IMA ADPCM). Use the existing .aud parser.

8. **FindByName filters `<none>`**: The string "`<none>`" (0x817474) is explicitly
   rejected in VoxClass__FindByName. Any INI entry with value `<none>` is treated
   as not found.
