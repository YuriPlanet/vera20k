# SpawnManagerClass — Ghidra Research Report

**Primary class address:** `0x006B6C90` (constructor) through `0x006B8140` (scalar-deleting destructor)
**Class size:** `0x74` bytes (116 bytes), verified via `SpawnManagerClass::GetSize` at `0x006B8120`
**Vtable (primary):** `vtable__SpawnManagerClass` at `0x007F3650` (primary vtable; AI at vtable+0x5C)
**AI entry:** `SpawnManagerClass::AI` at `0x006B7230`
**AI dispatcher:** `TechnoClass::AI_Update` at `0x006F9E50` calls `this.SpawnManager->vtable[0x5C]()` once per tick
**Overall confidence:** High — all key functions decompiled from binary
**Active in YR:** Yes — five YR units + four AI variants + meteor spawner use this class (no TS-legacy gating)

---

## 1. Overview

`SpawnManagerClass` is an `AbstractClass`-derived manager object attached to any `TechnoClass`
whose `TechnoTypeClass` has `Spawns=` set to a valid `TechnoType`. It owns a fixed pool of
sub-units ("spawn slots"), each holding one spawned child plus a per-slot state machine. The
manager runs its AI every 10 frames, stepping slots through a 7-state machine that covers:
idle / launching / in-flight / returning to dock / docked-landing / reloading / regenerating.

The class does **not** own rendering, pathing, or combat — it only issues high-level mission
commands (Assign_Target / Assign_Destination / Assign_Mission / Limbo / Unlimbo) on the
spawned children. The actual movement, firing, and death are handled by the spawned `TechnoClass`
objects themselves through their own AI pipelines (`AircraftClass`, `UnitClass` etc.).

### YR consumers (verified from rulesmd.ini)

| Parent section | Spawns= | SpawnsNumber | SpawnRegenRate | SpawnReloadRate | Child flavor |
|---|---|---|---|---|---|
| `[CARRIER]` Aircraft Carrier | HORNET | 3 | 600 | 150 | aircraft (returns to dock) |
| `[DEST]` Destroyer | ASW | 1 | 400 | 150 | aircraft (returns to dock) |
| `[DRED]` Dreadnought | DMISL | 2 | 80 | 0 | missile (fire-and-forget) |
| `[BSUB]` Boomer Submarine | CMISL | 2 | 80 | 0 | missile (fire-and-forget) |
| `[V3]` V3 Launcher | V3ROCKET | 1 | 400 | 0 | missile (fire-and-forget) |
| `[VLAD]` (campaign / `VLADIMIR`) | DMISL | — | — | — | missile — campaign/AI-only |
| `[CARRIERB]`, `[DREDB]`, `[CDEST]` | HORNET / DMISL / ASW | — | — | — | AI-difficulty variants |
| `[METEOR01]`, `[METEOR02]` (Meteor Shower SW) | PEBBLE | — | — | — | fire-and-forget missile-style |

**Important correction to the gap-scan brief**: `SpawnManagerClass` is **not** what backs
Aegis (uses `Primary=AGMissile` weapon), Kirov (`Primary=Bomb` + `BombClass` falling-bomb
mechanic), Dolphin (`Primary=DolphinSonic` weapon), or Carryall (distinct "picked-up unit"
passenger mechanic on `AircraftClass`). Those units do not set `Spawns=` in rulesmd.ini and
are not consumers of this class.

---

## 2. Class Layout

### SpawnManagerClass (0x74 bytes total)

| Byte offset | Type | Field | Source / purpose |
|---|---|---|---|
| `0x00` | vtable* | primary vtable | `vtable__SpawnManagerClass` at 0x007F3690 |
| `0x04` | vtable* | MI vtable #4  | IPersistStream-like thunks |
| `0x08` | vtable* | MI vtable #8  | IRTTITypeInfo-like thunks |
| `0x0C` | vtable* | MI vtable #12 | INoticeSink/Source-like thunks |
| `0x10` | int | AbstractClass.UniqueID | assigned by `AbstractClass__AssignUniqueID` |
| `0x14` | int | AbstractClass.AbstractFlags | base flags |
| `0x18` | int | AbstractClass field_0x18 | AbstractClass reserved |
| `0x1C` | int | AbstractClass field_0x1C | AbstractClass reserved |
| `0x20` | int | AbstractClass.Dirty | touched by ComputeCRC / save state |
| `0x24` | TechnoClass* | **Owner** | the parent unit (carrier, dreadnought, …) |
| `0x28` | TechnoTypeClass* | **SpawnType** | child type (HORNET/DMISL/V3ROCKET/CMISL/ASW) |
| `0x2C` | int | **SpawnsNumber** | pool capacity (copied from `TechnoType.SpawnsNumber`) |
| `0x30` | int | **SpawnRegenRate** | frames to rebuild a destroyed child (from `TechnoType.SpawnRegenRate`) |
| `0x34` | int | **SpawnReloadRate** | frames between launches / reload between missions |
| `0x38` | vtable* | DynamicVector<SpawnControl*>.vtable | `PTR_FUN_007F36B4` |
| `0x3C` | void** | DynamicVector.data | array of SpawnControl* |
| `0x40` | int | DynamicVector.capacity | |
| `0x44` | byte | DynamicVector.growth_allowed | =1 |
| `0x45` | byte | DynamicVector.is_heap_allocated | =1 if grown |
| `0x48` | int | DynamicVector.count | active slot count (== SpawnsNumber after init) |
| `0x4C` | int | DynamicVector.growth_step | =10 |
| `0x50` | int | UpdateTimer.StartFrame | RateTimerClass |
| `0x54` | int | UpdateTimer.AccumulatedTime | upper dword of 64-bit timer |
| `0x58` | int | UpdateTimer.Duration | =0x14 (20) at construction, =10 after first fire |
| `0x5C` | int | ReloadTimer.StartFrame | gates launches per manager (not per slot) |
| `0x60` | int | ReloadTimer.AccumulatedTime | |
| `0x64` | int | ReloadTimer.Duration | set from `SpawnReloadRate` or 9/20 (see §3) |
| `0x68` | AbstractClass* | **CurrentTarget** | active target for the wing |
| `0x6C` | AbstractClass* | **QueuedTarget** | next target; promoted to `CurrentTarget` each tick |
| `0x70` | int | **ManagerMode** | 0 = idle, 1 = launching, 2 = returning |

### SpawnControl (per-slot, 0x18 bytes, allocated via `operator_new(0x18)`)

| Offset | Type | Field | Purpose |
|---|---|---|---|
| `0x00` | TechnoClass* | Spawn | pointer to the spawned child; null while in regen (state 7) |
| `0x04` | int | **SlotState** | state machine (0..7, no state 5) |
| `0x08` | int | Timer.StartFrame | per-slot RateTimer |
| `0x0C` | int | Timer.Accumulated | |
| `0x10` | int | Timer.Duration | set per-state (see §3) |
| `0x14` | int | IsMissileSpawn | 1 if SpawnType ∈ {V3RocketType, DMislType, CMislType}; else 0 |

### Back-pointer into the child

The constructor writes the parent TechnoClass pointer into the spawned child at
`child + 0x2D4`:
```
*(undefined4 *)(*piVar3 + 0x2d4) = param_1[9];   // owner ptr
```
This lets the child find its parent without a manager lookup. The same write is repeated
inside state-7 regen. `0x2D4` on TechnoClass is the `SpawnOwner`/parent back-pointer.

### Parent's forward pointer to the manager

`TechnoClass` stores the `SpawnManagerClass*` at its own offset `0x2D0`
(`param_1[0xb4]` inside `TechnoClass__Init_Managers`). `PointerExpired` and every
manager-related callsite reaches it via this field.

### Global registry of all spawn managers

The DAT cluster `0x00B0B880` (vtable) / `0x00B0B884` (data) / `0x00B0B888` (capacity)
/ `0x00B0B890` (count) is a global `DynamicVector<SpawnManagerClass*>`. The constructor
appends `this` and the destructor removes it via linear shift. No periodic global iteration
is currently visible (the AI tick is reached through the owner TechnoClass's own AI chain —
see §5).

---

## 3. Slot State Machine

Slot indices (`SlotState` values) observed in `SpawnManagerClass::AI`:

| State | Name | Enter condition | Exit |
|---|---|---|---|
| 0 | **ReadyDocked** | Initial; also from state 6 after reload | → 2 when manager mode = 1 AND ReloadTimer expired AND owner-ready check passes |
| 1 | **KamikazeWait** | Missile slot after FUN_0054e3b0 (self-destruct flight) | → PointerExpired → state 7 when Timer expires (= `V3Rocket/DMislPauseFrames + TiltFrames`) |
| 2 | **InFlight** | From 0: Unlimbo+Assign_Target | Never returns from 2 directly; follows target each AI tick. Manager mode 1→2 moves aircraft to state 3, missiles to state 1. |
| 3 | **ReturningToDock** | Manager mode = 2 (returning), aircraft slot only | → 4 on next tick if parent still alive and target cleared |
| 4 | **LandingAtDock** | From 3 when parent alive and target cleared | → 6 when child's 2D coord matches owner AND `child.Z - owner.Z < 0x14`; `Limbo()` called; Timer = SpawnReloadRate |
| 5 | **(unused)** | — | No case 5 handler. Likely reserved. |
| 6 | **Reloading** | From 4 after docking | → 0 when Timer expires; HP restored to `TechnoType.Strength (type+0x684)`, ammo restored from `type+0xa0`, then fields `child+0x6C` and `child+0x70` set to ammo value |
| 7 | **Regenerating** | From `PointerExpired` when child destroyed, OR Kill_All_Spawns; also entered at state-1 timeout | Timer = SpawnRegenRate; on expiry, allocate new child via `SpawnType::CreateObject(owner_house)`, set IsMissileSpawn flag, call `Limbo()`, write back-ptr, set state = 0 |

### Timing constants (from binary + rulesmd.ini defaults)

| Source | Value (YR default) | Purpose |
|---|---|---|
| Manager `UpdateTimer.Duration` | `0x14` → `10` | First tick gates for 20 frames, subsequent ticks every 10 frames |
| Per-slot aircraft re-launch delay after enter state 2 (from LAB_006b735c) | `0x14` = 20 frames | Delay between launches when `type+0xD68` (MissileSpawn on owner type) is 0 |
| Per-slot missile re-launch delay after enter state 2 | `9` frames | Delay when `type+0xD68` is non-zero |
| State 1 timer (V3ROCKET) | `Rules.V3RocketPauseFrames + V3RocketTiltFrames` (0+60 = 60) | How long V3 body stays tilted before silo closes |
| State 1 timer (DMISL / CMISL) | `Rules.DMislPauseFrames + DMislTiltFrames` (20+60 = 80) | How long missile stays tilted before silo closes |
| State 7 regen timer | `this+0x30` = `TechnoType.SpawnRegenRate` | Time to rebuild destroyed child |
| State 6 reload timer | `this+0x34` = `TechnoType.SpawnReloadRate` | Refill ammo / rearm time |
| State 4 dock-landing detection | `|child.Z - owner.Z| < 0x14` (20 leptons) | Height threshold for "docked" |

### Boomer missile spawn-offset jitter

State 0 launch code contains a CMISL-specific offset subtraction:
```c
if (spawn_type == Rules.CMislType) {
  spawn_launch.x -= DAT_0084009c;
  spawn_launch.y -= DAT_008400a0;
}
```
and immediately after, if the spawn is CMISL, a `SMOKESYS`-like `AnimClass` is spawned at
the launch position (`operator_new(0x1c8)` for `AnimClass`, looked up via `AnimTypeClass::FindByIndex`
with a specific anim-type global). This is the Boomer underwater launch smoke effect.

### V3 / DMISL launch code path

State 0 → 2 transition includes a hardcoded facing derivation from the game's RateTimer
current value (`(timer >> 7) + 1 >> 1 & 0xFF`) passed to `spawn->Unlimbo(coord, facing)`
(vtable+0xD8). For missile slots that uses a tilt orientation; for aircraft it uses 0 (the
second-level `spawn+0x29` Z check uses this later).

---

## 4. Manager-level state machine (`ManagerMode`, offset 0x70)

Runs at the end of `AI` **after** all slots have been walked. Only fires when the manager
update timer ticks (every 10 frames).

| Mode | Name | Entry | Next |
|---|---|---|---|
| 0 | **Idle** | Start / after all in-flight recovered | → 1 when a target promoted from QueuedTarget to CurrentTarget AND vtable+0x3AC (owner check, likely "not ReallyAlive/can-attack" negated) returns true |
| 1 | **Launching** | Target acquired | → 2 once every slot is in state 2 (in-flight) or state 7 (regenerating). For aircraft children that haven't entered 2 yet, re-issue mission 1 (move/attack); for missile children, call `FUN_0054e3b0(spawn, target)` (self-destruct toward target) and set state = 1 with appropriate PauseFrames+TiltFrames timer |
| 2 | **Returning** | All launched | → 0 when no slot is in states 3 or 4 (landing/returning) |

### Mission/target dispatch primitives (vtable calls on the spawned child)

| Vtable offset | Likely meaning (from TECHNOCLASS doc + observed usage) | When called |
|---|---|---|
| `+0xD4` | `Limbo()` — remove from world | State 4→6 transition; also Kill_All_Spawns (state 0/6) |
| `+0xD8` | `Unlimbo(coord, facing)` — place in world | State 0→2 launch |
| `+0xF8` | `Destroy()` / `MarkForRemoval` | Kill_All_Spawns (docked and regen slots) |
| `+0x1B8` | `Get_Coord()` — current 3D coord | State 2/3/4 target-following, landing detection |
| `+0x1BC` | `Get_Center_Coord()` | Used inside the kamikaze helper |
| `+0x1E8` | `Assign_Mission(MissionEnum, allow_interrupt)` | Every state transition that changes the child's mission |
| `+0x3C8` | `Assign_Destination(cell)` | Every movement/path update |
| `+0x480` | `Assign_Target(target, force)` | Target change (owner=dock, or wing target) |
| `+0x3DC` | `Kill_Self()` | Missile kamikaze fallback when `type+0xD68` is 0 (should not fire for actual missiles) |

Passed mission IDs observed: `1` (walk to dest / follow target) and `2` (attack) — consistent
with standard RA2 mission enum.

---

## 5. Integration Points — call chain

### Construction

```
TechnoClass::InitFromType / Constructor (AircraftClass/UnitClass/InfantryClass)
  → TechnoClass::Init_Managers   (0x006F3FF4)
      → if (type->Spawns != nullptr):
          operator new(0x74)
          SpawnManagerClass::Constructor(owner, type->Spawns, type->SpawnsNumber,
                                         type->SpawnRegenRate, type->SpawnReloadRate)
          this->SpawnManager = manager     (stored at owner+0x2D0)
```

`TechnoClass::Init_Managers` is invoked unconditionally from
`AircraftClass::InitFromType` (0x00413F80), `InfantryClass::InitFromType` (0x00517CC0),
`UnitClass::Constructor` (0x007353C0), `UnitClass::InitFromType` (0x00746810), and
`FUN_00442c40`. In practice only the units listed in §1 set `Spawns=`.

Sizes allocated (per `operator new`):
- SpawnManagerClass: `0x74` (116)
- SlaveManagerClass: `0x64` (100) — for reference
- Each SpawnControl slot: `0x18` (24)

### Per-tick AI dispatch (resolved)

`TechnoClass::AI_Update` at **`0x006F9E50`** calls the SpawnManager once per tick:

```c
if (this->SpawnManager != nullptr) {          // at field_0x2D0
    this->SpawnManager->vtable[0x5C]();       // = SpawnManagerClass::AI at 0x006B7230
}
```

The primary vtable is at **`0x007F3650`** (`vtable__SpawnManagerClass`). Index 23
(byte offset `0x5C`) holds the address `0x006B7230`. The same dispatch pattern is used
for `SlaveManager` (field_0x2D8 + vtable+0x5C → SlaveManagerClass::AI_Update).

Relative position inside `AI_Update`: the SpawnManager call fires **after** the
SlaveManager call, after CaptureManager update, after health/cloak bookkeeping, and
before the final damage-particle logic. So SpawnManager runs late in the per-tick AI
chain — if it launches children in state 0→2, those children's `AI_Update` has already
run this tick and won't execute until next tick.

The AI itself self-gates with `UpdateTimer.Duration` = 10 frames, so even though invoked
every tick it only does state-machine work every 10 frames (≈0.67 s at the engine's
15 Hz sim rate). State 2 in-flight updates therefore retarget the wing at ~10-frame
granularity.

### Target assignment (resolved callers)

`SpawnManagerClass::SetTarget` (0x006B7B90) is trivial:
```c
if (new_target != this.CurrentTarget) this.QueuedTarget = new_target;
```
The next tick promotes `QueuedTarget` → `CurrentTarget`.

Only two callers:

| Caller | Address | Passes | Purpose |
|---|---|---|---|
| `TechnoClass::Fire_At` | `0x006FDD50` | the current fire target | When the parent fires its primary weapon at target T, it also sets the SpawnManager's target to T so the spawn wing engages the same target |
| `TechnoClass::Set_ArchiveTarget` | `0x006FCDB0` | `null` (clear only) | Called when the parent's archive target is cleared; propagates clear to spawn wing |

So the SpawnManager target is slaved to the parent's fire target, *not* set externally.
The parent's regular combat pipeline (acquire target → `Fire_At` → SetTarget) drives the
wing. This matches the observed behavior: a Carrier fires its "weapon" (which is a no-op
ammunition type in YR) and the Hornets follow.

### Retreat / kamikaze helper (`FUN_0054e3b0` at 0x0054E3B0)

Called from three sites (all in SpawnManager), with implicit `this` = global retreat-list
DynamicVector at `DAT_00ABC5F8..00ABC600` (3-DWORD RateTimer) + associated data vector.

Behavior given (spawn_child, optional_target):
1. **Check `child.Type.MissileSpawn` (at `type+0xD68`)**: if **false** (aircraft-type),
   just call `child->vtable+0x3DC` (`Kill_Self`) and return. Aircraft don't enter the
   retreat queue — they die outright.
2. **Missile-type path**: allocate 8-byte entry `{ child_ptr, dest_cell }`.
   - If optional target is null: derive dest from `child->Get_Center_Coord()` +
     `Pathfinding_update_continued((timer>>12)>>1 & 7)` — a deterministic random-ish
     offset.
   - If target given: use `MapClass::Get_Cell_At(target.Coord / 256)` — cell below target.
3. **Mark child**: `child+0x6CA = 1` (IsRetreating / abandoned flag), `child.HP = 1`
   (flagged as near-death so it self-destructs on any hit).
4. Append entry to the retreat DynamicVector (with capacity-growth logic).

**What drains the list:** a global tick handler (not decompiled here) walks the entries
and, for each, re-issues `Assign_Destination(dest)` and `Assign_Mission(1)` until the
child reaches the cell or dies. This is why Hornets keep flying toward the target after
the Carrier sinks — they're on this separate list now, not the manager's slot list.

### Retreat-list cleanup (`FUN_0054e590` at 0x0054E590)

Called from `Kill_All_Spawns` for state-1 slots (missiles mid-kamikaze when owner dies).
Walks the retreat-list DynamicVector backwards:
- If entry's `child_ptr` matches the passed spawn → free the entry, shift-down, remove.
- Else if entry's `dest_cell` (as a pointer comparison) matches the spawn → break, then
  re-derive destination via `spawn->Get_Coord() → Get_Cell_At()` and re-issue
  `Assign_Destination` + `Assign_Mission(1)` on the entry's child.

The upshot: when the owner dies, already-retreating missiles are purged from the list
(they'll be killed explicitly via `vtable+0xF8`), and any retreating missile whose
destination was the spawn itself is redirected to a generic cell.

### Destruction / death propagation

1. **Child dies (normal death):**
   `TechnoClass::PointerExpired(child)` runs on every other TechnoClass. For the parent
   carrier/dreadnought, the check at `0x00707A6F`:
   ```
   if (this->SpawnManager != nullptr)
       SpawnManagerClass::PointerExpired(child);
   ```
   inside `SpawnManagerClass::PointerExpired` (0x006B7C60), the slot whose
   `spawn == child` is found; if the slot's child still had ammo AND was alive AND was not
   a missile slot, the call short-circuits (guard against stale pointer for live aircraft).
   Otherwise: `slot.Spawn = null`, `slot.State = 7`, `slot.Timer.Duration = SpawnRegenRate`.

2. **Target dies:** same function clears `CurrentTarget` / `QueuedTarget` and calls
   `ClearAllTargets()` (0x006B7BB0) if there's no queued replacement. `ClearAllTargets`
   forces every state-2 aircraft whose type has `MissileSpawn=yes` into an immediate
   kamikaze (should be empty set — aircraft spawns don't have `MissileSpawn=yes`), then
   zeros both targets and sets `ManagerMode = 0`.

3. **Owner dies:** `TechnoClass::PointerExpired(owner)` hits the matching path at end of
   `SpawnManagerClass::PointerExpired`:
   ```
   if (expired == owner) {
       Kill_All_Spawns();
       ClearAllTargets();
   }
   ```
   `Kill_All_Spawns` (0x006B7100) per-slot:
   - state 0 or 6 (docked/reloading): `child.Destroy()` (vtable+0xF8), null pointer, state=7
   - state 1 (kamikaze-wait): `FUN_0054e590(child)` then Destroy, null, state=7
   - states 2/3/4 (in-flight): `FUN_0054e3b0(child, CurrentTarget)` — mark child as
     "flee/kamikaze toward last target" (sets `child+0x6CA = 1` and `child.HP = 1`).
     Hornets that were launched before the carrier sank continue toward the target and
     self-destruct; this reproduces the original "Hornets keep flying" behavior.
   The regen timer duration assigned on the killed slot is `SpawnRegenRate` if
   `owner.Ammo < 1 OR owner.AmmoFlag == 0`, else `0`. Since the owner is about to be
   destroyed anyway and the manager follows it in the destructor, this is mostly
   bookkeeping.

4. **Owner mind-controlled / chrono-warped / deployed:**
   - `TechnoClass::ChangeOwner` (0x0070157E) → `Kill_All_Spawns()`
   - `TemporalClass::InitiateWarp` (0x0071AF39) → `Kill_All_Spawns()`
   - `TechnoClass::PerformDeploy` (0x00710021) → `Kill_All_Spawns()`
   - `FootClass::StopFiring` → `FUN_006fcd40` → `Kill_All_Spawns() + ClearAllTargets()`
     (gated by a flag at `owner+0x6AD`)

5. **Destructor** (0x006B7010): calls `Kill_All_Spawns` if `g_GameActive != 0`, then frees
   each SpawnControl via `FUN_007c8b3d` (operator delete), frees the DynamicVector data
   block, and removes the manager from the global registry at `0x00B0B884`.

### Save / load / CRC determinism

- `ComputeCRC` (0x006B7DE0) folds: AbstractClass base, ManagerMode, QueuedTarget.UniqueID,
  CurrentTarget.UniqueID, ReloadTimer remaining, UpdateTimer remaining, count, SpawnsNumber,
  SpawnType.UniqueID, Owner.UniqueID. Per-slot state (state/timer/spawn-ptr per slot) is
  **NOT** folded. The spawn children are themselves TechnoClass instances registered in
  the global Techno list, so they're CRC'd independently — their HP/mission/position flow
  through `TechnoClass::ComputeCRC`. **Slot states 0..7 and per-slot timers, however,
  have no independent CRC coverage**; if they diverge between sims, the parent CRC will
  not catch it. In practice these transitions are tied to deterministic inputs
  (g_CurrentFrameCounter, child state), so this is a latent risk rather than an active
  bug. For Rust: either replicate the omission (byte-compatible CRC with gamemd.exe) or
  fold slot state (stricter lockstep, incompatible replay).
- `Save` (0x006B80B0) format:
  1. `AbstractClass::Save(io)` — base fields (UniqueID, AbstractFlags, Owner pointer,
     SpawnType pointer, SpawnsNumber, SpawnRegenRate, SpawnReloadRate, timers, targets,
     ManagerMode — everything from 0x10 through 0x73 **except** the DynamicVector body).
  2. Write `DWORD count` (from offset 0x48).
  3. For each slot: write 0x18 bytes raw (`spawn_ptr`, `state`, `timer{start,acc,dur}`,
     `is_missile`).
- `Load` (0x006B7F10) format (mirror):
  1. `AbstractClass::Load(io)` → reinitializes base fields; reset vtables; restart
     timer.
  2. Read `DWORD count`.
  3. Allocate `count` SpawnControl structs of 0x18 bytes each, fill from stream, append
     to the DynamicVector.
  4. **Pointer remap**: for each slot's `spawn_ptr`, enqueue remap into the global
     pointer-fixup table (`DAT_00B0C110`); also remap `Owner` (offset 0x24), `SpawnType`
     (offset 0x28), `CurrentTarget` (offset 0x68), `QueuedTarget` (offset 0x6C). Timers
     are saved as absolute frame counts and don't need remap since
     `g_CurrentFrameCounter` is restored from the same save.
- `GetClassID` (0x006B7ED0), `WhatAmI` (0x006B8130), `GetSize` (0x006B8120 → returns 0x74)
  round out the IPersistStream COM surface.

---

## 6. INI Keys → TechnoTypeClass offsets (verified)

Read in `TechnoTypeClass::ReadINI` at `0x00714EE1` and surrounding addresses. `param_1` is
`int *` in this function, so indices multiply by 4 for byte offsets.

| INI key | Reader | Byte offset on TechnoTypeClass | Type | Default |
|---|---|---|---|---|
| `Spawned=` | `ReadBool` at ~0x00714E8F | `0xD54` | byte | false |
| `Spawns=` | `CCINIClass::ReadType` → `FUN_0067BD30` at 0x00714EB3 | `0xD58` | TechnoTypeClass* | null |
| `SpawnsNumber=` | `ReadInt` at 0x00714EF5 | `0xD5C` | int | 0 |
| `SpawnRegenRate=` | `ReadInt` at 0x00714ED4 | `0xD60` | int | 0 |
| `SpawnReloadRate=` | `ReadInt` at 0x00714F16 | `0xD64` | int | 0 |
| `MissileSpawn=` | `ReadBool` at 0x00714F37 | `0xD68` | byte | false |

`MissileSpawn=yes` on the **child** TechnoType flips behavior in `FUN_0054e3b0` (the
"retreat/kamikaze" helper) and in the aircraft/missile reload delay selector inside AI.
Child types that have `MissileSpawn=yes` in rulesmd.ini: `DMISL`, `CMISL`. V3ROCKET is
treated as missile-style via the hardcoded Rules-pointer check, regardless of its
`MissileSpawn=` value.

### Global hardcoded TechnoType references (RulesClass)

Read in `RulesClass::ReadGeneral` (0x0066D530). These are *pointers* into the
`TechnoTypeClass` heap, not strings — they are compared by pointer equality.

| RulesClass byte offset | INI key | Default (YR) |
|---|---|---|
| `0x4B0` | `[General] V3RocketPauseFrames` | 0 |
| `0x4B4` | `[General] V3RocketTiltFrames`  | 60 |
| `0x4E0` | `[General] V3RocketType`        | V3ROCKET |
| `0x4E4` | `[General] DMislPauseFrames`    | 20 |
| `0x4E8` | `[General] DMislTiltFrames`     | 60 |
| `0x514` | `[General] DMislType`           | DMISL |
| `0x548` | `[General] CMislType`           | CMISL |

The constructor and AI compare `this->SpawnType (offset 0x28)` against the three Type
pointers (0x4E0 / 0x514 / 0x548) to set the per-slot `IsMissileSpawn` flag:
```c
slot->IsMissileSpawn =
    (spawn_type == Rules.V3RocketType) ||
    (spawn_type == Rules.DMislType)    ||
    (spawn_type == Rules.CMislType);
```
This is the hardcoded "are you a missile-style spawn?" test. **Any other child type with
`MissileSpawn=yes` in INI will behave differently from these three** (reduced reload
delay path in `FUN_0054e3b0` applies, but the slot-side `IsMissileSpawn` flag does not).
This asymmetry is a real YR quirk.

---

## 7. Aircraft dock / rearm interaction

The return-to-dock sequence uses standard `AircraftClass` missions; SpawnManager does not
reach into `AircraftClass` internals. The flow:

1. State 2 (InFlight) continuously retargets the spawn to `CurrentTarget` via
   `Assign_Destination(cell_of(target))` and `Assign_Mission(ATTACK=2)`.
2. Manager mode flips from 1 → 2 when all slots are in state 2 or 7 — typically triggered
   by `ClearAllTargets()` when the target dies, or externally by the owner clearing targets.
3. Per-slot, manager mode 2 moves each aircraft slot from state 2 → 3: `Assign_Target(owner)`
   and `Assign_Mission(MOVE=1)`. The aircraft's own AI handles the `ENTER`/`MOVE`-to-owner
   logic.
4. State 3 → 4 happens when the child's own logic has landed the aircraft on the owner's
   cell AND proximity conditions are met. This triggers `Limbo()` on the spawn and starts
   the reload timer (`SpawnReloadRate`).
5. State 4 → 6 after `Limbo()`; state 6 → 0 when reload timer expires (HP + ammo
   restored).

No explicit interaction with `AircraftClass::Mission_Return` or refinery-dock logic was
observed — the spawn's own `Assign_Mission(MOVE, owner)` reuses the generic "move to target"
path. The docking is *not* a `BuildingDock`-style hardpoint; it's just a proximity check
(`|child.Z - owner.Z| < 20`) in the Z axis combined with an XY-cell match.

**Kirov / BombClass is NOT this class.** Kirov's `Primary=Bomb` uses the weapon/projectile
path into `BombClass`, not `SpawnManagerClass`. See [BOMB_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BOMB_CLASS_GHIDRA_REPORT.md).

---

## 8. YR-activity verification (per Tiberian-Sun-ghost checklist)

| Code path | Active in YR? | Evidence |
|---|---|---|
| `TechnoClass::Init_Managers` → SpawnManager ctor | **Yes** | Unconditionally gated by `type->Spawns != nullptr`; five YR rulesmd.ini sections set Spawns= |
| State machine 0..7 in AI | **Yes** | Not gated by SpecialFlags; runs whenever Init_Managers created a manager |
| Boomer CMISL launch offset + SMOKESYS anim | **Yes** — YR only | Checks `Rules.CMislType`; CMISL is a YR-specific TechnoType (no RA2 base equivalent) |
| V3 rocket branch (0x4E0 pointer check) | **Yes** | V3ROCKET is shipped in both RA2 and YR rulesmd.ini |
| DMISL branch | **Yes** | DRED/DMISL shipped in both RA2 and YR rulesmd.ini |
| `FUN_0053a130` always-zero check | **Yes** | Return-0 stub; unconditional "always launch" branch. Possibly a dev/debug flag, no YR effect |
| `Kill_All_Spawns` from `ChangeOwner`, `TemporalWarp`, `PerformDeploy` | **Yes** | All three are live YR behaviors (mind control, temporal weapon, deploy) |
| `FUN_006fcd40` via `FootClass::StopFiring` | Conditional | Gated by `owner+0x6AD != 0` — a flag I did not trace. Active path worth confirming |

No TS-legacy gates found. No `SpecialFlags & 0x1000` or similar dormant-feature check
anywhere in the class.

---

## 9. Current Rust implementation status

Scan summary (rust-scan agent over `src/`):

| Item | Status | File/line |
|---|---|---|
| `SpawnManager` struct | **Not implemented** | — |
| `Spawns=` parsing | **Not parsed** | `src/rules/object_type.rs:295-303` only handles `Enslaves=/SlavesNumber=/SlaveReloadRate=` for Slave Miner |
| `SpawnsNumber=`, `SpawnRegenRate=`, `SpawnReloadRate=` | **Not parsed** | — |
| `MissileSpawn=` | **Not parsed** | — |
| `Spawned=` (child flag) | **Not parsed** | — |
| `V3RocketType` / `DMislType` / `CMislType` in Rules | **Not parsed** | — |
| `V3RocketPauseFrames`/`TiltFrames`, `DMisl*Frames` | **Not parsed** | — |
| CARRIER / DEST / DRED / V3 / BSUB combat | **Not implemented** | No spawner hookup; parents fire nothing |

Only the `SpawnsTiberium=` overlay key (unrelated — for ore-growth particle systems) is
currently referenced in `src/render/overlay_atlas.rs:227`.

---

## 10. Follow-up pass — resolved open questions

### Owner+0x674 is `ILocomotor` COM interface pointer

The missile-ready-to-launch gate on state 0 for missile slots is:
```c
ILocomotor* loco = owner->Locomotor;      // offset 0x674 on TechnoClass
if (loco == nullptr) Assert(E_NOTIMPL);
if (!loco->Is_Moving()        /* vtable+0x10 */
 && !loco->Is_Moving_Now()    /* vtable+0x80 */) {
    // owner is fully stationary → launch is allowed
    goto LAB_006b735c;
}
// else: owner still moving/turning, skip launch this tick
```

**Interpretation:** V3 launchers, Dreadnoughts, and Boomers must be fully stationary
(including no turn in progress) before a missile can launch. Aircraft-style spawns
(Carrier, Destroyer) skip this check entirely — Hornets can launch while the carrier
is moving.

Evidence: [DRIVE_LOCOMOTION_CLASS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_LOCOMOTION_CLASS.md) line 784 confirms vtable+0x10 = Is_Moving
(0x4AFB80, three-tier dest/head_to/XY check) and line 812 confirms vtable+0x80 =
Is_Moving_Now (0x4AFC20, CDTimer-active OR has waypoint+speed). The same two methods
exist on every ILocomotor implementor (Drive, Fly, Rocket, Ship, Hover, JumpJet,
Teleport, Walker).

### AI dispatch callsite

Resolved: `TechnoClass::AI_Update` at `0x006F9E50` — see §5 "Per-tick AI dispatch".
Dispatches via primary vtable at `0x007F3650`, index 23 (byte offset 0x5C).

### SetTarget callers

Resolved: `TechnoClass::Fire_At` (0x006FDD50) passes the fire target; `TechnoClass::Set_ArchiveTarget` (0x006FCDB0) passes null to clear. See §5 "Target assignment".

### FUN_0054e3b0 / FUN_0054e590 retreat system

Resolved: see §5 "Retreat / kamikaze helper" and "Retreat-list cleanup". Global
retreat-list DynamicVector at `DAT_00ABC5F8..ABC600` (state triple: StartFrame,
AccTime, Duration=2). Aircraft-style children skip the list entirely and die via
`Kill_Self`; only missile-style children enter the retreat queue.

### Child TechnoClass offsets — resolved

| Offset | Name | Purpose | Confidence |
|---|---|---|---|
| `0x6AD` | **IsDeployed** (byte) | Set in `PerformDeploy`. Blocks auto-cloaking and re-deploy. Checked on the **owner** in SpawnManager — e.g., deployed V3 shouldn't behave like a moving V3. | HIGH (per `TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md:1526`) |
| `0x6C4` | **TechnoTypeClass\*** (cached Type pointer) | Alternate cache of the type pointer (separate from the constructor-set 0x14C). Used by spawn code and `SelectWeaponAgainst`. | MEDIUM — behaves as a Type ptr; exact relationship to 0x14C not traced |
| `0x6CA` | **IsRetreating / Abandoned** (byte) | Set to 1 by `FUN_0054e3b0` when a missile is pushed onto the retreat list. Combined with `HP=1`, guarantees the missile self-destructs on any hit and is owned by no targeting system. | HIGH (set inside 0x0054E3B0 under that specific condition only) |

Additional back-pointer resolved in [TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md):
- **`0x2D4` on the child = SpawnOwner** (back-pointer to the parent). Written by the
  constructor and by state-7 regen. Read by `RecordKill` (0x00702D40) to credit kills
  to the parent spawner rather than the missile.

### ComputeCRC sufficiency

Resolved: per-slot state is **not** folded into the manager's CRC, but the spawned
children are themselves in the global Techno list and their own `ComputeCRC` runs
per-tick — that covers HP, position, mission, ammo. What remains uncovered is:
- Slot state value (0..7)
- Per-slot timer (StartFrame / Duration)
- `IsMissileSpawn` flag (derivable from SpawnType so effectively covered)

Since all slot-state transitions are derived from deterministic inputs (child state +
global frame counter), divergence is structurally unlikely. However, there is no CRC
guard. See §5 Save/Load note for Rust-side trade-off.

### MissileSpawn on parents

Resolved: **no shipped YR parent has `MissileSpawn=yes`**. Grep over `rulesmd.ini`
confirms zero hits among `Spawns=`-setting TechnoTypes. The 9-frame launch-delay
branch in state 0 (selected when `ownerType+0xD68 != 0`) is **unreachable dead code
in shipped YR**. All parents hit the 20-frame delay branch.

This is a real YR quirk carried over from TS development: the field exists on every
TechnoType and is read during state 0, but no parent unit sets it. A mod that set
`MissileSpawn=yes` on a Spawns=-using parent would trigger the 9-frame branch.

### State 5

No case-5 handler. Verified by searching the entire AI switch for `case 5:` — absent.
Historical TS-era artifact or unused reserved value. Not a dormant branch; slots
physically never enter state 5 via any observed transition.

---

## Sources

**Ghidra decompilations (verified, `gamemd.exe`, image base 0x00400000):**
- `0x006B6C90` — `SpawnManagerClass::Constructor`
- `0x006B7010` — `SpawnManagerClass::Destructor`
- `0x006B7100` — `SpawnManagerClass::Kill_All_Spawns`
- `0x006B7230` — `SpawnManagerClass::AI` (primary tick entry)
- `0x006B7B90` — `SpawnManagerClass::SetTarget`
- `0x006B7BB0` — `SpawnManagerClass::ClearAllTargets`
- `0x006B7C60` — `SpawnManagerClass::PointerExpired`
- `0x006B7D30` — `SpawnManagerClass::CountAliveSpawns`
- `0x006B7D50` — `SpawnManagerClass::CountDockedSpawns`
- `0x006B7DE0` — `SpawnManagerClass::ComputeCRC`
- `0x006B7F10` — `SpawnManagerClass::Load` (save-blob deserializer)
- `0x006B80B0` — `SpawnManagerClass::Save` (save-blob serializer)
- `0x006B8120` — `SpawnManagerClass::GetSize` (returns 0x74)
- `0x006B8880` — `DynamicVectorClass<T>::Constructor` (inline base)
- `0x006F3FF4` — `TechnoClass::Init_Managers`
- `0x006F9E50` — `TechnoClass::AI_Update` (per-tick driver; dispatches `SpawnManager.AI` via vtable+0x5C)
- `0x007077C0` — `TechnoClass::PointerExpired` (propagation to SpawnManager)
- `0x006FCD40` — `FUN_006fcd40` (FootClass::StopFiring → Kill_All_Spawns)
- `0x006FCDB0` — `TechnoClass::Set_ArchiveTarget` (clears SpawnManager target)
- `0x006FDD50` — `TechnoClass::Fire_At` (propagates fire target to SpawnManager)
- `0x0054E3B0` — `FUN_0054e3b0` (retreat-list insertion / kamikaze helper)
- `0x0054E590` — `FUN_0054e590` (retreat-list removal / redirect)
- `0x0053A130` — `FUN_0053a130` (always-returns-0 stub)
- `0x00714EE1` — `TechnoTypeClass::ReadINI` (spawn key reads)
- `0x0066D530` — `RulesClass::ReadGeneral` (hardcoded Type pointers)

**Vtable addresses (verified):**
- `0x007F3650` — `vtable__SpawnManagerClass` (primary; AI at index 23 / byte 0x5C)
- `0x007F3634` — `vtable__SpawnManagerClass__secondary_4`
- `0x007F362C` — `vtable__SpawnManagerClass__secondary_8`
- `0x007F3624` — `vtable__SpawnManagerClass__secondary_12`

**INI references:**
- `ini/rulesmd.ini` — sections `[CARRIER]`, `[DEST]`, `[DRED]`, `[V3]`, `[BSUB]`,
  `[HORNET]`, `[DMISL]`, `[CMISL]`, `[ASW]`, `[V3ROCKET]`, `[General]`
- `ini/rules.ini` — base RA2 defaults (identical for these keys)

**Related existing research docs:**
- [ABSTRACTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ABSTRACTCLASS_GHIDRA_REPORT.md) — base class layout (0x24 bytes)
- [TECHNOCLASS_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_STRUCT_LAYOUT.md) / [TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md) —
  owner offsets `0x2D0` (SpawnManager*) and child's `0x2D4` (owner back-ptr)
- [BOMB_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BOMB_CLASS_GHIDRA_REPORT.md) — Kirov bomb-drop mechanism (distinct system)
- [SLAVE_MINER_ORE_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/SLAVE_MINER_ORE_SYSTEM_GHIDRA_REPORT.md) — sister manager class (SlaveManagerClass)

**Rust implementation files referenced:**
- `src/rules/object_type.rs:295-303` — Slave manager INI (no spawn equivalent yet)
- `src/render/overlay_atlas.rs:227` — unrelated `SpawnsTiberium=` particle usage
