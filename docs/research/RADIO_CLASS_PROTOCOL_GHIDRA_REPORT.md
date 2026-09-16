# RadioClass — Message Protocol Ghidra Report

**Primary addresses:** `0x0065A750` (ctor), `0x0065A820` (Receive_Radio base), `0x0065A970` (Transmit_Radio_Impl core)
**Confidence:** HIGH (full decompile of all Radio functions + every Receive_Radio override)
**Active in YR:** Yes — fundamental inter-unit handshake used by every dock/board/tether/repair interaction

---

## 1. Overview

`RadioClass` is the middle base class in the Techno hierarchy
(`AbstractClass → ObjectClass → MissionClass → RadioClass → TechnoClass → Foot|Building`)
that gives every on-map object a small synchronous-RPC "radio" channel. Objects establish
pairwise links via `HELLO` (0x02), break them via `OVER_AND_OUT` (0x03), and exchange
command/query messages (0x07–0x24) over those links. The protocol is used for:

- **Harvester ↔ Refinery** — docking handshake, queue, unload choreography
- **Passenger ↔ Transport** — boarding permission, passenger arrival, eject
- **Aircraft ↔ Airfield/Helipad** — landing slot reservation, reload tick
- **Carryall ↔ Liftable unit** — `RADIO_WANT_RIDE`/`RADIO_NEED_TO_MOVE` pickup protocol
- **Service Depot ↔ Vehicle / Hospital/Armory ↔ Infantry** — repair/heal tick
- **IFV / OpenTopped** — passenger tether for weapon swap
- **Bunker / Tank Bunker** — occupant deploy-in-place
- **Grinder (YAGRND) ↔ Absorbable unit** — "I'm entering you to die" handshake
- **Construction Yard residual** — OVER_AND_OUT triggers ambient-anim reset

All transports use the **same vtable primitives** (`Transmit_Radio`, `Receive_Radio`, etc.).
The diversity of behavior comes from each subclass's `Receive_Radio` override, not from
distinct message lanes.

### The protocol is synchronous, not queued

There is **no message queue**. `Transmit_Radio_Impl` calls the target's `Receive_Radio`
directly through vtable slot 0x194 and uses its integer return as the response code,
all on the caller's stack. There is no scheduler, no frame delay, no mailbox. Two
objects deep in a dock handshake can exchange several messages in a single caller's
tick. This is important: **response codes flow back by return value, not by reply
message.**

---

## 2. Class Layout — RadioClass adds 26 bytes at +0xD4…+0xED

Verified from `RadioClass::Constructor` @ `0x0065A750`.

| Off    | Size | Field                         | Init value                   | Purpose |
|--------|------|-------------------------------|------------------------------|---------|
| +0xD4  | 4    | `RadioHistory[0]`             | 0                            | Most recent msg code (for dedup) |
| +0xD8  | 4    | `RadioHistory[1]`             | 0                            | Previous msg code |
| +0xDC  | 4    | `RadioHistory[2]`             | 0                            | Oldest msg code (slot 2) |
| +0xE0  | 4    | `Contacts.vtable`             | `&PTR_FUN_007e180c`          | `DynamicVectorClass<TechnoClass*>` vtable |
| +0xE4  | 4    | `Contacts.data` (`TechnoClass**`) | `operator_new(4)`         | Contact-pointer array (1 slot default) |
| +0xE8  | 4    | `Contacts.Capacity`           | **1**                        | Max simultaneous radio partners |
| +0xEC  | 1    | `Contacts.CanGrow`            | 1                            | Resize flag |
| +0xED  | 1    | `Contacts.Initialized`        | 1                            | Ctor-completed flag |

### Key structural facts

1. **Capacity defaults to 1.** In YR every Foot/Unit/Infantry/Aircraft starts and stays
   at 1 contact. Only `BuildingClass::Constructor` resizes via
   `RadioClass::Set_Contact_Count(type->NumberOfDocks)` (see §7). One slot is enough for
   a harvester (linked to at most one refinery at a time), a passenger (one transport),
   a carried vehicle (one carryall), a docked aircraft (one helipad).
2. **The "vector" is used as a sparse fixed-capacity array.** `OVER_AND_OUT` (BREAK)
   writes `NULL` into the matching slot; it does **not** compact. The next `HELLO` finds
   the first `NULL` slot and fills it. `Capacity` is the iteration bound, not a live count.
3. **There is no separate "active count" field.** To count active partners you must walk
   `[0 .. Capacity)` and skip nulls. This is what `FindDockSlot` @ `0x0065AD90` does.
4. **RadioHistory is a 3-slot dedup log, not a ring.** The shift in `Receive_Radio` is
   **D8→DC, D4→D8, msg→D4** (see §4). Slot D4 is always "most recent"; D8 is "last seen
   before that"; DC is the pre-empted value of D8. It's a tiny cache used to suppress
   duplicate-message side effects.

### Vtable slots added by RadioClass

| Slot | Offset  | Method                     | Signature                                                           |
|------|---------|----------------------------|---------------------------------------------------------------------|
| 101  | +0x194  | `Receive_Radio`            | `int (this, TechnoClass* sender, int msg, void** payload)`          |
| 157  | +0x274  | `Transmit_Radio_ToFirst`   | `int (this, int msg)` — sends to `Contacts[0]` implicitly           |
| 158  | +0x278  | `Transmit_Radio`           | `int (this, int msg, TechnoClass* target)`                          |
| 159  | +0x27C  | `Transmit_Radio_Impl`      | `int (this, int msg, void** payload, TechnoClass* target)`          |
| 160  | +0x280  | `Broadcast_Radio_ToAll`    | `void (this, int msg)` — sends to every non-null slot               |

`Transmit_Radio` is a thin wrapper that passes the global `&g_RadioScratchBuffer`
as payload to `Transmit_Radio_Impl`. `Transmit_Radio_ToFirst` additionally defaults
`target = Contacts[0]`.

---

## 3. The Send Path — `Transmit_Radio_Impl` @ `0x0065A970`

```c
int RadioClass::Transmit_Radio_Impl(this, int msg, void** payload, TechnoClass* target)
{
    // 1. Default target to Contacts[0] when caller passed null.
    if (target == NULL) {
        target = Contacts[0];
        if (target == NULL) return 0;     // no partner, silent no-op
    }

    // 2. BREAK (0x03) path
    if (msg == 0x03) {
        // Null every slot that matches target (no compaction).
        for (int i = 0; i < Contacts.Capacity; ++i)
            if (Contacts[i] == target) Contacts[i] = NULL;
        // Dispatch to target: target->Receive_Radio(filtered_sender, 0x03, payload)
        filtered = Filter_AbstractType_InMap(this);   // NULL out sender if not Techno RTTI
        return target->vtable[0x194](target, filtered, 0x03, payload);
    }

    // 3. HELLO (0x02) path — add target as a contact
    if (msg == 0x02) {
        int freeSlot = -1;
        for (int i = 0; i < Contacts.Capacity; ++i) {
            if (freeSlot == -1 && Contacts[i] == NULL) freeSlot = i;
            if (Contacts[i] == target) return 1;      // already linked → ROGER, no retransmit
        }
        if (freeSlot == -1) {
            // All slots full — evict Contacts[0] by sending it BREAK.
            this->vtable[0x278](this, 0x03, Contacts[0]);   // Transmit_Radio(BREAK, old)
            freeSlot = 0;
        }
        filtered = Filter_AbstractType_InMap(this);
        int r = target->vtable[0x194](target, filtered, 0x02, payload);
        if (r == 1 /* ROGER */) {
            Contacts[freeSlot] = target;
            return 1;
        }
        return 10 /* NEGATORY */;
    }

    // 4. Any other message — fire to target, return its response verbatim.
    filtered = Filter_AbstractType_InMap(this);
    return target->vtable[0x194](target, filtered, msg, payload);
}
```

### Filter_AbstractType_InMap @ `0x0040DD70`

```c
int* Filter_AbstractType_InMap(this)
{
    switch (this->vtable[0x2C]() /* What_Am_I */) {
        case 1:  // UnitClass
        case 2:  // AircraftClass
        case 6:  // BuildingClass
        case 0xF:// InfantryClass
            return this;
    }
    return NULL;
}
```

**Effect:** Every `Receive_Radio` callee sees `sender` as either a valid on-map Techno
pointer, or `NULL`. It is safe to dereference `sender` (guarding for null) without further
RTTI checks. Abstract/Bullet/Anim/Overlay senders are silently filtered to `NULL`.

### Broadcast_Radio_ToAll @ `0x0065ACE0`

```c
void RadioClass::Broadcast_Radio_ToAll(this, int msg)
{
    for (int i = 0; i < Contacts.Capacity; ++i) {
        TechnoClass* p = Contacts[i];
        if (p) this->vtable[0x27C](this, msg, &g_RadioScratchBuffer, p);  // Transmit_Radio_Impl
    }
}
```

Uses the same global payload buffer `g_RadioScratchBuffer` as single-target sends.

### The `g_RadioScratchBuffer` global (trap for reimplementation)

`Transmit_Radio` and `Broadcast_Radio_ToAll` pass the address of a **single static
global buffer** as the `payload` argument. Any callee writing `*payload = x;` clobbers
the shared global. This works in gamemd.exe because the engine is single-threaded and
message handling is synchronous — by the time the caller reads `payload`, no other
radio call has run. **A concurrent/parallel reimplementation must replace this with a
per-call stack slot or a per-tick scratch pool.**

---

## 4. The Receive Path — `RadioClass::Receive_Radio` @ `0x0065A820`

```c
int RadioClass::Receive_Radio(this, TechnoClass* sender, int msg, void** payload)
{
    // --- A. RadioHistory dedup shift ---
    // Only shift if the new msg differs from the most-recent slot.
    if (msg != RadioHistory[0]) {
        RadioHistory[2] = RadioHistory[1];    // the previous D8 value goes to DC
        RadioHistory[1] = RadioHistory[0];    // old most-recent becomes second-most-recent
        RadioHistory[0] = msg;                // new most-recent
    }
    // Duplicates of the most-recent message are NOT re-shifted and still fall through
    // to the handler below. The history just records "what was the last different msg".

    // --- B. BREAK (0x03) — remove sender from my Contacts, notify sender's ObjectClass ---
    if (msg == 0x03) {
        for (int i = 0; i < Contacts.Capacity; ++i) {
            if (Contacts[i] == sender) {
                ObjectClass::Receive_Radio(sender, payload, 0x03);   // sender-side side effects
                Contacts[i] = NULL;                                  // sparse-null my slot
                return 1 /* ROGER */;
            }
        }
        // sender wasn't a contact — fall through to ObjectClass::Receive_Radio
    }

    // --- C. HELLO (0x02) — accept a new contact (with ally gate) ---
    else if (msg == 0x02 && this->ObjectClass_field_0x6C != 0 /* alive/active */) {
        // Ally gates (both checked, each strict):
        if (!HouseClass::Is_Ally_ByObject(this, sender))   return 10;
        if (this != NULL && (this->AbstractFlags & 1) && !HouseClass::Is_Ally_ByObject(sender))
            return 10;

        // Already contacted? Return ROGER idempotently.
        if (sender != NULL) {
            for (int i = 0; i < Contacts.Capacity; ++i)
                if (Contacts[i] == sender) return 1;
        }
        // Find first NULL slot, insert.
        if (Contacts.Capacity < 1) return 10;
        for (int i = 0; i < Contacts.Capacity; ++i) {
            if (Contacts[i] == NULL) { Contacts[i] = sender; return 1; }
        }
        return 10 /* no free slot → NEGATORY */;
    }

    // --- D. Everything else: defer to ObjectClass::Receive_Radio ---
    return ObjectClass::Receive_Radio(sender, msg, payload);
}
```

### Two ally checks, not one

The HELLO case runs ally verification **twice** — once from the receiver's POV
(`Is_Ally_ByObject(this, sender)`) and once testing the reverse from the inverse object
at `this` (gating on `AbstractFlags & 1`). This is the standard MP ally handshake: both
sides must agree. If MP ally state just flipped mid-link this double-check avoids one-way
link states.

### BREAK does not shrink Capacity

BREAK writes NULL into the slot but never shrinks the array. This is why subsequent HELLO
can reuse the slot without allocation. BuildingClass's multi-slot Contacts[] therefore
acts like a fixed-size dock roster.

### RadioClass::Receive_Radio is the common tail

Every subclass override (Techno/Foot/Unit/Aircraft/Building) either returns its own
response or calls into RadioClass::Receive_Radio (sometimes via ObjectClass::Receive_Radio
which also routes here). So the HELLO/BREAK contact-list bookkeeping is centralised.

---

## 5. The Message Code Table

All codes verified by decompiling every Receive_Radio override (`Object`, `Radio`,
`Techno`, `Foot`, `Unit`, `Aircraft`, `Building`). Names below are the YRpp/TS-era names
where confirmed by binary debug strings; otherwise named by observed behavior and marked.

| Code | Name                        | Direction        | Handler(s) that decide it              | Summary |
|------|-----------------------------|------------------|----------------------------------------|---------|
| 0x01 | `ROGER` *(response only)*   | reply            | —                                      | Positive ack return value |
| 0x02 | `HELLO`                     | any → any        | RadioClass                             | Establish radio link (ally-gated, contact-list insert) |
| 0x03 | `OVER_AND_OUT` / `BREAK`    | any → any        | RadioClass + Techno + Unit + Building  | Sever link. Clears sender from my Contacts; side-effects per subclass. |
| 0x07 | `DOCKING_COMPLETE`          | Building → Unit  | Techno + Unit                          | Unit side exits its dock: clear nav, clear target, Queue_Mission(GUARD=5), maybe re-HELLO + Queue_Mission(0x18). |
| 0x08 | `REQUEST_DOCKING_CLEARANCE` | Unit → Building  | Techno-base, Aircraft, Building        | "May I approach?" Building responds ROGER or QUEUED(0x17). Aircraft-side rejects if already dedicated to another airfield. |
| 0x09 | *(mirror of 0x07)*          | Building → Unit  | Techno                                 | Same tail as 0x07 — ENTER_DOCK to sender then delegate. |
| 0x0A | `NEGATORY` *(response only)*| reply            | —                                      | Negative ack (value 10) |
| 0x0B | `DOCK_APPROACH`             | Building → Unit  | Building                               | Building tells unit to Queue_Mission(UNLOAD=0x14). |
| 0x0C | `DOCK_ARRIVED`              | Unit → Building  | Building                               | Unit reports at dock cell. Triggers ambient idle anim + Queue_Mission(GUARD) if not currently MISSION_UNLOAD. |
| 0x0D | *(ambient-anim reset)*      | any              | Building + Object                      | Object handler calls `this->vtable[0x124](2)` (anim-stop). BuildingClass silently consumes it for WeaponsFactory. |
| 0x0E | `CAN_DOCK` / "full dock query" | Unit → Building | Building + Unit + Aircraft            | The large docking arbiter (power, zone, ally, size, occupancy, refinery-queue cell). See §6. |
| 0x0F | `CAN_ENTER`                 | Unit → Building / Unit | Building + Unit + Aircraft       | Passenger entry request. Checks ally, naval zone, capacity, stealth, mind-control. Return 0=reject, 1=ROGER, 10=NEGATORY. |
| 0x10 | `RESERVE_DOCK` / "PrepareToLink" | Unit → Building | Building                           | Reserve a dock slot pre-flight. Refineries, UnitRepair, Weeder accept when idle, else NEGATORY. |
| 0x11 | `IS_UNIT_LINKED` *(inferred)* | query          | Foot                                   | Probe: "are you currently in MISSION_ENTER (7) toward me?" FootClass returns ROGER(1) if current or queued mission == 7 (side-effect-delegates to Techno first), else silent 0. No Carryall debug string names this code. |
| 0x12 | `MOVE_TO_CELL`              | Building/Aircraft → Unit | Foot + Aircraft                | Payload = target CellClass*. Unit compares its cell to payload cell; if already there return CELL_ACCEPTED(0x14), else nav to it, return ROGER. |
| 0x13 | **`NEED_TO_MOVE`** *(confirmed by Carryall LAND debug string @ 0x00817C14)* | Carryall → cargo / Building → harvester | Foot + Aircraft + Unit | "Tell me if you can move / report destination." Writes my NavCom destination into *payload. Returns NEGATORY(10) if locomotor busy, ROGER(1) otherwise. Used by Carryall LAND to check cargo readiness AND by BuildingClass dock negotiation (case 0xE sub-protocol). Same code, dual role. |
| 0x14 | `CELL_ACCEPTED` *(response only)* | reply      | —                                      | Returned by 0x12 when unit is already at the target cell. |
| 0x15 | `DOCK_NOW`                  | Unit → Building / Building → Unit | Building + Unit + Aircraft | "Begin dock sequence now." Unit turns to face pad, plays voice; Building marks anim-complete and Queue_Mission(UNLOAD). |
| 0x16 | `TIMING_SYNC`               | Unit → Building / Building → Unit | Techno + Unit                | Mid-sequence rendezvous. Unit sets 0x4000-tick RateTimer (dock-beacon) and may immediately send DOCK_NOW(0x15). |
| 0x17 | `QUEUED` *(response only)* + `EVICT_QUEUE` | reply / mgmt | Foot + Unit + Aircraft       | Return value 0x17 = "you are in queue." As a sent message: Foot's 0x17 handler clears nav if dest==Contacts[0]; Unit's handler scatter-dismisses abductee pre-condition; Aircraft's handler reroutes to next airfield. |
| 0x18 | `ENTER_DOCK`                | any              | Techno                                 | Set `DockedIn` byte = 1 (if not already), propagate to sender via Transmit_Radio(0x18). |
| 0x19 | `LEAVE_DOCK`                | any              | Techno                                 | Clear `DockedIn` byte, propagate. |
| 0x1A | *(secondary dock lock set)* | any              | Techno                                 | Set second lock byte = 1, propagate. Used during late-stage dock anims (prevents re-entry). |
| 0x1B | *(secondary dock lock clear)* | any            | Techno                                 | Clear second lock byte, propagate. |
| 0x1C | `REPAIR_TICK`               | Building → Unit  | Techno + Foot                          | Service Depot periodic heal. Deducts money, adds HP. Returns INSUFFICIENT_FUNDS(0x20), REPAIR_COMPLETE(0x21), or ROGER. Foot-override rejects when unit already has a navcom destination. |
| 0x1D | *(aircraft helipad-reserve ack)* | Building → Aircraft | Aircraft                       | Returns ROGER unless aircraft is launched and already equals type's cargo-max; then NEGATORY. |
| 0x1E | *(deploy / set-nav action)* | any              | Techno                                 | If vtable+0x3F4 (GetDeployable-like) returns non-null non-empty: set nav to *payload, Queue_Mission(MOVE=1). |
| 0x1F | `LINK_PASSENGER` *(cap check)* | Unit → Transport | Techno + Aircraft                   | Increment transport's internal-passenger count (cap compared to Type+0x684). Aircraft version has a half-cap early-ROGER shortcut. |
| 0x20 | `INSUFFICIENT_FUNDS` *(response)* | reply      | —                                      | 1C path: owner can't afford repair step. |
| 0x21 | `REPAIR_COMPLETE` *(response)*    | reply      | —                                      | 1C path: health crossed `Rules.ConditionYellowRepair` threshold. Aircraft also returns this for msg 0x21 as a cap-equal check. |
| 0x22 | `IS_REPAIRING` / "healthy enough?"| query        | ObjectClass                            | Returns NEGATORY if `HP / MaxHP >= Rules+0x16F8` (a constant 1.0 stored at `0x0066B32D`, not an INI key), else ROGER. The 0x22→0x17 eviction loop in `BuildingClass::Receive_Radio 0x0E` is the Hospital/Armory branch (`0x0043CB0C`); UnitRepair depots return before it (corrected 2026-09-06). |
| 0x23 | `IS_OCCUPIED` *(cell-occupied check)* | query    | FootClass                              | Looks up building currently in unit's cell. Returns NEGATORY(10) if a building is there. Used by Bunker/UnitRepair to refuse an occupied pad. |
| 0x24 | `RADIO_WANT_RIDE`           | Unit → Carryall  | UnitClass                              | Unit asks Carryall for a lift. Returns 0 if cloaked, 10 if on tunnel cell with no-enter, 10 if mission==0x10 ENTER, else ROGER (or NEGATORY if passenger slot taken). |

**Highest known code:** 0x24. Reads in the Aircraft handler (guard around `field_0xA5 != 0`
for mission-states 4/0x1A/0x1B/0x1E/0x1F) suggest message codes above 0x24 don't dispatch.

### 5.1 Named-string confirmations

Only three RADIO_* names survive as literal strings in the binary, all inside
`AircraftClass::Do_MISSION_MOVE_Carryall` (0x00416D50) debug traces at
`0x00817C14`, `0x00817E04`, `0x00817E58` (verified via `get_function_by_address(0x00417272)` → Entry: 00416d50, body 00416d50–004172e3; 0x00417272 is mid-body, 2026-05-20):

- `RADIO_HELLO`          = **0x02** (confirmed — VALIDATE_LZ sends `vtable[0x278](0x02, piVar7)`, debug logs "RADIO_HELLO got RADIO_ROGER")
- `RADIO_ROGER`          = **0x01** (response value, confirmed)
- `RADIO_NEED_TO_MOVE`   = **0x13** (confirmed — LAND state sends `vtable[0x274](0x13)`, debug logs "RADIO_NEED_TO_MOVE got RADIO_ROGER". Re-verified this pass — the prior report labeled this code 0x11 in error.)
- `RADIO_WANT_RIDE`      = **0x24** (confirmed — VALIDATE_LZ sends `vtable[0x274](0x24)`, debug logs "RADIO_WANT_RIDE did not get RADIO_ROGER")

Names for codes without string literals are inferred from behavior and use of the codes
in the decompiled switch handlers (see the master dock-protocol report cross-references
in [BUILDINGCLASS_MISSILE_AND_RADIO_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MISSILE_AND_RADIO_GHIDRA_REPORT.md) §2.1 where the existing TS-era
enum is documented).

### 5.2 YR activity of Carryall-protocol codes (new, 2026-04-24 verification)

The `RADIO_WANT_RIDE` / `RADIO_NEED_TO_MOVE` pickup protocol is only driven by
`AircraftClass::Do_MISSION_MOVE_Carryall` (0x00416D50). In standard YR content:

- `rulesmd.ini` has exactly one type with `Carryall=yes` — `[HIND]` at line 10822.
- `[HIND]` has `TechLevel=-1` — unbuildable in skirmish.
- No campaign or skirmish AI script invokes the carryall mission on default content.

**Conclusion:** Codes 0x13 (NEED_TO_MOVE) and 0x24 (WANT_RIDE) are reachable only
via mods, map triggers, or scripted missions that put a unit into MISSION_MOVE with
a target while `Carryall=yes` on the sender. **However, code 0x13 is also dual-used
by the harvester↔refinery CAN_DOCK sub-protocol** (see UnitClass::Receive_Radio case
0xE, where the unit side sends `0x13 NEED_TO_MOVE` followed by `0x12 MOVE_TO_CELL`
to its refinery partner). That path is fully active in every YR skirmish. Code 0x24
is genuinely Carryall-only and is dormant in standard YR play.

---

## 6. Subclass `Receive_Radio` cheat-sheet

All overrides use vtable slot 0x194. Every override either returns its own response or
delegates to the next-up base. The **only** subclasses with overrides are:

| Class           | Address      | Handled codes (explicit case)                                   | Default tail |
|-----------------|--------------|-----------------------------------------------------------------|--------------|
| ObjectClass     | `0x005F5320` | 0x0D (stops anim state 2), 0x22 (IS_REPAIRING)                  | return 0 |
| RadioClass      | `0x0065A820` | 0x02 (HELLO), 0x03 (BREAK)                                      | ObjectClass |
| TechnoClass     | `0x006F4AB0` | 0x03, 0x07, 0x08, 0x09, 0x16, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1E, 0x1F | RadioClass |
| FootClass       | `0x004D8FB0` | 0x11, 0x12, 0x13, 0x17, 0x1C, 0x23                              | TechnoClass |
| UnitClass       | `0x00737430` | 0x03, 0x07, 0x0E, 0x0F, 0x15, 0x16, 0x17, 0x24                  | FootClass |
| AircraftClass   | `0x004190B0` | 0x08, 0x0E, 0x0F, 0x12, 0x13, 0x15, 0x17, 0x1D, 0x1F, 0x21     | FootClass |
| BuildingClass   | `0x0043C2D0` | 0x03, 0x08, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x15            | TechnoClass |

**InfantryClass has no override.** Infantry inherit the FootClass handler directly.
This is why infantry boarding a transport or garrisoning a building uses the same
foot-level 0x12 (MOVE_TO_CELL) and 0x13 (IS_UNIT_LINKED) code paths as vehicles.

### 6.1 AircraftClass mission-state guard

`AircraftClass::Receive_Radio` opens with a gate **before** the message switch:

```c
switch (this->CurrentMission /* +0x2B * 4 = +0xAC, MissionClass field */) {
    case 4:    // Retreat
    case 0x1A: // Paradrop Approach (26)
    case 0x1B: // Paradrop Overfly (27)
    case 0x1E: // Spyplane Approach (30)
    case 0x1F: // Spyplane Overfly (31)
        if (this->field_0xA5 == 0) return 0;   // ignore radio entirely
}
```

Mission-enum mapping verified against [MISSIONCLASS_STATE_MACHINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSIONCLASS_STATE_MACHINE.md) (the canonical
binary-verified table). All five gated missions are **aircraft-specific flight modes**:
one emergency (Retreat) and two pairs of scripted overfly sequences (Paradrop, Spyplane).
The common thread — these are missions where the aircraft is in a scripted path that
must not be interrupted by a HELLO or CAN_DOCK from an outside actor. `field_0xA5` acts
as a "scripted mission is cleared to receive orders" latch within those modes. (Note:
a previous revision of this report mislabeled 0x1A/0x1B/0x1E/0x1F as Selling/Repair/
ParaDropApproach/ParaDropOverfly — that was taken from a doc with inconsistent
numbering. The state-machine mapping above is the authoritative one.)

### 6.2 UnitClass cases of note

- **0x03 (BREAK):** If unit is in mission state 0xC (UNLOAD), Queue_Mission(GUARD=5)
  first; then delegate to Foot.
- **0x07 (DOCKING_COMPLETE):** Clear navcom, clear target, Queue_Mission(SLEEP=0),
  call FUN_004DA1C0 (likely ResetMissionState), and if not flagged `field_0x106` or
  dest==0, **re-send HELLO** and Queue_Mission(0x18=ENTER_DOCK_FINAL). This is the
  "handshake from the top" re-connect at the end of a dock cycle.
- **0x0E (CAN_DOCK):** The unit-side of the full dock query. Checks:
  - Current nav dest matches requesting building's SizeLimit (type+0x5E0) vs sender Size (type+0x380)
  - Pathfinder has valid steps
  - Zone match via MapClass::GetZoneID on unit type's movement zone (type+0x5B4)
  - Occupying building's cell pass-through check
  Returns ROGER only when size+zone pass and sender's CAN_DOCK→IS_UNIT_LINKED(0x13)→MOVE_TO_CELL(0x12) chain also succeeds.
- **0x24 (WANT_RIDE):** The Carryall target-side. Rejects if cloaked (`vtable+0x1D4`),
  on a tunnel cell with NoMoveIn flag, or if own mission==0x10 (ENTER). Allows ROGER
  when passenger slot is free (`field_0x1A1 == -1`), NEGATORY otherwise.

### 6.3 FootClass cases of note

- **0x11 (IS_UNIT_LINKED, inferred):** Returns ROGER(1) iff current mission
  (`vtable[0x184]()`) == 7 `MISSION_ENTER` OR queued mission (`param_1[0x2d]` a.k.a.
  `+0xB4`) == 7. In the ROGER branch the handler side-effect-calls
  `TechnoClass::Receive_Radio(sender, 0x11, payload)` first — Techno has no case 0x11
  so this falls through to RadioClass then ObjectClass, which returns 0 silently. The
  explicit `return 1` overrides. Net effect: "yes, I'm currently walking to enter
  you" without any dock/lock byte mutation. If not in mission 7 the case falls
  through to the bottom `TechnoClass::Receive_Radio` call and returns 0 (ignored).
- **0x13 (NEED_TO_MOVE):** Writes my NavCom destination (`param_1[0x169]` at +0x5A4)
  into `*param_4`. If I have a destination and my locomotor's vtable[0x10] (the
  `Is_Moving`-style check) returns non-zero, return NEGATORY(10) — "busy". If no
  destination, return ROGER(1) — "I'm idle and can be moved." This is what Carryall
  LAND state (0x00417272 case 3) sends to confirm the cargo unit is stationary
  before landing. The harvester↔refinery path also uses this code: in
  `UnitClass::Receive_Radio` case 0x0E (CAN_DOCK) the unit sends `0x13` to its
  building partner, then on ROGER sends `0x12 MOVE_TO_CELL`.
- **0x12 (MOVE_TO_CELL):** Compares my current cell (vtable+0x1B8 returns `&cell.xy`
  as shorts) to `payload->xy` (via vtable+0x48 returning 32-bit pixel coords, shifted
  `>> 8` to cell-space). If equal → return `CELL_ACCEPTED = 0x14`. Else kick navcom
  (vtable+0x480, 1 = urgent), stamp frame counter to nav-departure fields
  (`+0xC8/+0xCC/+0xD0` in Foot locals — written as `field_0x32/0x33/0x34 * 4`),
  and return ROGER. Has two fix-up branches: state 5 with nav-target unset → Queue(2);
  state 7 while Is_Interrupt_Allowed → Queue_Clear. Those untangle sleep/wait states
  when mid-pathfind.
- **0x23 (IS_OCCUPIED):** `CellClass::Get_Cell_At(vtable+0x48)` + `Look_up_building_in_cell`.
  Returns `NEGATORY` if a building sits on my cell, `ROGER` otherwise. Used by
  Bunker/UnitRepair dock-in-place to ensure their dock tile is clear.

### 6.4 The TechnoClass base-case logic (full)

Already covered in the companion report
`BUILDINGCLASS_MISSILE_AND_RADIO_GHIDRA_REPORT.md §2.3`, confirmed here:

- 0x03 (BREAK): if I'm docked (`+0x418.lo == 1`) and sender has grind flag (`sender+0x418 != 0`),
  Transmit_Radio(0x19 LEAVE_DOCK, sender) first, then delegate to RadioClass::Receive_Radio.
- 0x07 / 0x09 / 0x16: send 0x18 ENTER_DOCK back to sender, then delegate.
- 0x08: send 0x19 LEAVE_DOCK to sender, then send 0x03 BREAK to sender.
- 0x18 / 0x19: flip `DockedIn` byte, propagate to sender.
- 0x1A / 0x1B: flip second lock byte, propagate.
- 0x1C: full repair formula (see §8.2).
- 0x1E: set nav to *payload, Queue_Mission(MOVE=1), return ROGER.
- 0x1F: if `field_0x4C == type+0x684` return NEGATORY, else `field_0x4C++`, return ROGER.

---

## 7. Who sizes `Contacts[]` above 1?

**Only `BuildingClass::Constructor` @ `0x0043B740`.** Verified by xref of
`RadioClass::Set_Contact_Count @ 0x0065AE60` (verified via `get_function_by_address(0x0043BCD0)` → Entry: 0043b740 — 0x0043BCD0 is mid-body; `get_function_callers(0x0065AE60)` → sole caller `BuildingClass__Constructor @ 0043b740`, 2026-05-20):

```
From 0043b740 (body) in BuildingClass::Constructor [UNCONDITIONAL_CALL]
From 0043b740 (body) in BuildingClass::Constructor [UNCONDITIONAL_CALL]
```

Constructor tail (verbatim):

```c
if (param_1[0x148] /* type ptr */ != 0) {
    int docks = *(int *)(param_1[0x148] + 0x1780);   // BuildingTypeClass+0x1780
    if (docks < 1) docks = 1;
    RadioClass::Set_Contact_Count(docks);
    return this;
}
RadioClass::Set_Contact_Count(1);
```

`BuildingTypeClass+0x1780` is the computed **dock count** (size of the `Dock=` list in
rulesmd.ini; see [BUILDING_DOCKING_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/BUILDING_DOCKING_SYSTEM_GHIDRA_REPORT.md) and [READINI_FIELD_MAPS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/READINI_FIELD_MAPS.md)).
For a standard YR refinery `Dock=NAREFN` → 1 dock slot; for helipads and naval yards with
multiple landing pads the count grows.

`Set_Contact_Count` @ `0x0065AE60`:

```c
void RadioClass::Set_Contact_Count(this, int newCap)
{
    if (Contacts.Capacity < newCap) {
        Contacts.vtable->Resize(newCap, 0);          // DynamicVector::Resize (vtable+8)
        for (int i = Contacts.Capacity + 1; i <= newCap; ++i)
            Contacts.data[i-1] = NULL;               // zero every newly-added slot
    }
}
```

Only grows, never shrinks. The contact array size is locked at building creation.

### Why no vehicle ever grows its contacts

Every other constructor path (Unit, Infantry, Aircraft, tiny things like Overlays) keeps
the 1-slot default. This is an observable invariant: **a non-building Techno can hold at
most one radio partner at a time.** This matters for the engine's docking model — when a
harvester HELLOs a refinery, it uses its single slot; if someone else tries to HELLO the
harvester (e.g. a repair pad) before it completes the refinery cycle, that HELLO will
**evict the refinery contact** via the "evict slot 0" fallback in `Transmit_Radio_Impl`.
This is why the harvester-to-refinery link is fragile under rapid order changes — the
repair-pad autopath would sever the refinery link unless the caller guards it.

---

## 8. Integration points

### 8.1 When does radio dispatch in the tick?

Radio is **not a standalone subsystem**. Every `Transmit_*` call happens inside some
mission, animation, damage, or cell-event handler, and resolves synchronously via the
target's `Receive_Radio` return. Typical call sites:

| Site                                         | What it sends                 |
|----------------------------------------------|-------------------------------|
| `FootClass::Mission_Harvest`                 | 0x08, 0x0E, 0x0C, 0x15        |
| `BuildingClass::Mission_Unload` / `Mission_Repair` | 0x0B, 0x1C                |
| `AircraftClass::Do_MISSION_MOVE_Carryall` @ `0x00416D50`     | 0x02, 0x24, 0x07, 0x13, 0x03 (see §8.3) |
| `AircraftClass::Carryall_Pickup` @ `0x00416AF0` | 0x19, 0x03 (on pickup finalize) |
| `FootClass::Mission_Enter`                   | 0x0F, 0x02, 0x1F              |
| `InfantryClass::Do_MISSION_ENTER`            | 0x02, 0x0F, 0x15              |
| `TechnoClass::Take_Damage`                   | 0x03 (on receiver destruction) |
| `BuildingClass::Update_AI` (service depot)   | 0x1C                          |

Ordering within the tick follows the CLAUDE.md sim order: commands → movement → vision →
combat → AI. Radio calls piggyback on whichever of these stages drives the caller. There
is no dedicated "radio phase."

### 8.2 The repair-tick formula (msg 0x1C, Techno base)

```c
if (HealthRatio >= Rules.ConditionYellowRepair /* +0x16F8 */) return 10 /* NEGATORY */;
int costStep = type->vtable[0xB0]();                        // money per tick
int hpStep   = max(1, type->vtable[0xB4]());                // hp per tick
if (ownerMoney < costStep) return 0x20 /* INSUFFICIENT_FUNDS */;
HouseClass::Spend_Money(costStep);
this->Health += hpStep;
this->EstimatedHealth += hpStep;
// iron-curtain / warp attachment cleanup if (AbstractFlags >> 2) & 1
if (HealthRatio > Rules+0x1700 /* ConditionYellow */ || vtable[0x1C8]() < -10)
    callback_ptr_at_4_0x60->vtable[0xF8]();                  // repair-complete anim callback
if (HealthRatio >= Rules.ConditionYellowRepair) {
    this->Health = type->MaxHealth;
    this->EstimatedHealth = type->MaxHealth;
    return 0x21 /* REPAIR_COMPLETE */;
}
return 1 /* ROGER */;
```

FootClass overrides this case to short-circuit NEGATORY when unit already has a nav
destination — prevents auto-repair from looping while unit is mid-move.

### 8.3 The Carryall pickup protocol — complete exchange (verified 2026-04-24)

Traced from `AircraftClass::Do_MISSION_MOVE_Carryall` @ `0x00416D50` and
`AircraftClass::Carryall_Pickup` @ `0x00416AF0`. This is the reference implementation
of the radio pickup handshake — every named debug string in the binary comes from here.

**State machine** (`this->field_0x2F * 4 = +0xBC` SubState on Aircraft):

| Sub | Name        | Radio action                                                                 |
|-----|-------------|-------------------------------------------------------------------------------|
| 0   | VALIDATE_LZ | If cargo is Unit, ally, non-null, not-already-carried and same-ally-both-sides: `vtable[0x274](3)` BREAK existing contact, `vtable[0x278](0x02, cargo)` HELLO cargo, on ROGER `vtable[0x274](0x24)` WANT_RIDE; on ROGER again `vtable[0x274](7)` DOCKING_COMPLETE, transition to sub=1; on WANT_RIDE NEGATORY `vtable[0x274](3)` BREAK and bail. |
| 1   | FLY_TO_LZ_approach | No radio; just set navcom to cargo cell. |
| 2   | FLY_TO_LZ   | If cargo disappeared or wandered → reset. When locomotor stops, sub → 3 (LAND). |
| 3   | LAND        | `vtable[0x274](0x13)` NEED_TO_MOVE — debug string "LAND - RADIO_NEED_TO_MOVE got RADIO_ROGER" confirms 0x13's name. On ROGER: invoke `cargo->vtable[0xD4]()` (limbo/detach), call `CargoClass::AddPassenger(cargo)`, then `vtable[0x274](3)` BREAK and begin lift. On any rejection: reset to sub=0. |

**Carryall_Pickup (called from LAND sub=3):**

```c
// After AddPassenger and locomotor swap:
if (FootClass::GetDestination() == this) {
    this->vtable[0x274](0x19);   // Transmit_Radio_ToFirst(LEAVE_DOCK)
    this->vtable[0x274](0x03);   // Transmit_Radio_ToFirst(BREAK)
}
```

The LEAVE_DOCK(0x19) + BREAK(0x03) pair at pickup-finalize flips the cargo's `DockedIn`
byte off before severing the link — this order matters because RadioClass::Receive_Radio
case 0x03 nulls the Contacts slot immediately on BREAK, so LEAVE_DOCK must fire first or
the target's DockedIn flag would never clear.

**Notable: Carryall does not send msg 0x11.** My earlier table attributed 0x11 to the
Carryall path; the re-check shows 0x11 is *only* sent by the harvester-refinery tail of
`FootClass::Mission_Harvest` (where FootClass responds ROGER while in MISSION_ENTER=7)
and is never transmitted by Carryall's mission loop. The "NEED_TO_MOVE" debug string at
`0x00817C14` follows a `vtable[0x274](0x13)` call, confirming 0x13 is the named code.

### 8.4 AircraftClass `field_0xA5` (the radio mission-state gate)

The guard `if (Mission_State ∈ {4, 0x1A, 0x1B, 0x1E, 0x1F} && field_0xA5 == 0) return 0;`
at the top of `AircraftClass::Receive_Radio` silently drops all messages when this field
is zero. `field_0xA5` is at offset `0x294` on the object (int-index 0xA5 of a `int*` view).

**Status after re-check (2026-04-24):** Partially resolved.
- `AircraftClass::Constructor` @ `0x00413D20` does **not** touch 0x294 — it's left
  zero-initialised by `operator_new`.
- `FootClass::Constructor` @ `0x004D31E0` writes many fields (0x148–0x1AE in int-index,
  corresponding to +0x520–+0x6B8 byte range) but **not 0xA5 / +0x294** either.
- Mission-state enum mapping for the five guard cases (per canonical
  [MISSIONCLASS_STATE_MACHINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSIONCLASS_STATE_MACHINE.md) table): 4 = Retreat, 0x1A = Paradrop Approach (26),
  0x1B = Paradrop Overfly (27), 0x1E = Spyplane Approach (30), 0x1F = Spyplane Overfly
  (31). The state-specific mission handlers (`AircraftClass::Mission_ParaDropApproach`
  @ `0x004155F0`, `Mission_ParaDropOverfly` @ `0x004157C0`, `Mission_SpyPlane` @
  `0x00417300`) are candidate write sites — the flag is almost certainly set after
  a mission-enter latch inside one of these. Confirming the exact setter address was
  not completed this pass; documenting as a targeted follow-up rather than a blocker
  since every gated mission is an aircraft-scripted flight mode outside the parity-
  critical harvester/passenger/ifv handshakes.

### 8.5 Broadcast_Radio_ToAll callers (resolved 2026-04-24)

`Broadcast_Radio_ToAll` (vtable +0x280) has exactly one call site identified this pass:

**`TechnoClass::Limbo_Tail_CallConceal` @ `0x0065AA80`** (sibling function, lives in the
same code section as the Radio methods at 0x65Axxx):

```c
void TechnoClass::Limbo_Tail_CallConceal(this)
{
    if (this->InLimbo /* +0x81 */ == 0) {
        this->vtable[0x280](3);   // Broadcast_Radio_ToAll(BREAK = 0x03)
    }
    ObjectClass::Conceal(this);   // sets InLimbo = 1, deselects, removes from cell
}
```

**Reached via:** `FootClass::Limbo` → `TechnoClass::Limbo_Helper` → this tail. Since
`UnitClass::Limbo` and `InfantryClass::Limbo` both delegate to `FootClass::Limbo`, and
`AircraftClass` inherits `FootClass::Limbo` directly (no override), **every mobile Techno
broadcasts BREAK to all radio contacts when it enters limbo.**

**Why this matters for parity:**
1. When a harvester is mind-controlled (`CaptureManagerClass::CaptureUnit` →
   `TechnoClass::ChangeOwner` → ...), the harvester's active refinery contact gets BREAK'd.
2. When a transport with passengers is destroyed, the Limbo pathway sends BREAK to every
   passenger contact — that's the trigger for the "passengers spill out on transport
   death" behavior (subsequently handled by the passenger-eject mission code).
3. Same for aircraft carrying cargo — the carryall↔cargo link is severed via this
   broadcast on carryall death mid-flight, producing the "dropped cargo" visual.
4. For buildings specifically — `BuildingClass::Limbo` was not found by name in this
   pass; the vtable slot for the Limbo virtual has its own override. See
   [BUILDING_DAMAGE_DESTRUCTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_DAMAGE_DESTRUCTION_GHIDRA_REPORT.md) for the building destruction path and
   whether it reaches Broadcast_Radio_ToAll or sends individual BREAKs (preliminary
   inspection of `TechnoClass::Limbo_Helper` suggests `param_1->vtable[0x2C0]()` returns
   the cargo count which, if positive, is used as a side loop — the broadcast still
   happens through the tail call regardless).

There is an additional DATA xref at `0x007F05DC` to `Limbo_Tail_CallConceal` — a vtable
slot registration. Without decoding which vtable this belongs to (offset ~`+0xD4` from
whatever vtable base starts near `0x007F0508`), I cannot yet identify whether a second
class exposes this function as a virtual. Not parity-blocking.

### 8.6 RadioHistory reads (not found)

The 3-slot RadioHistory at +0xD4/+0xD8/+0xDC is written by `RadioClass::Receive_Radio`
(see §4) and cleared by the constructor. No decompiled handler in this investigation
**reads** from those offsets — not the `Receive_Radio` overrides, not `Transmit_Radio_Impl`,
not the base. That's consistent with the structure existing as a write-only debug log
inherited from Tiberian Sun. A definitive answer requires scanning every function in the
binary for reads of `[this+0xD4]` / `[this+0xD8]` / `[this+0xDC]`, which this pass did
not do. **Working hypothesis: RadioHistory is inert in YR** — the shift cost is constant
per message and negligible, so the TS authors left the write side in place even after
whatever consumed it was removed. A reimplementation can omit the history slots without
observable effect unless a counter-example surfaces.

---

## 9. INI keys that feed the radio protocol (indirectly)

Radio messages themselves have **no direct INI keys**. Every gating flag the handlers
read lives on the TypeClass:

| Key            | Struct offset   | Referenced by message |
|----------------|-----------------|------------------------|
| `Dock=`        | BuildingType+? (list parser populates 0x1780 = count) | Constructor Set_Contact_Count, 0x0E, 0x10 |
| `NumberOfDocks=` (computed)  | BuildingType+0x1780  | Set_Contact_Count |
| `Refinery=`    | BuildingType+0x16B3 | 0x0E refinery-queue cell (+3,+1) |
| `Weeder=`      | BuildingType+0x16BC | 0x0E (+3,+1 queue), 0x10 (reserve) |
| `WeaponsFactory=` | BuildingType+0x16BD | 0x08 QUEUED, 0x0D silent swallow |
| `UnitRepair=`  | BuildingType+0x16A9 | 0x0E/0x08 dist<0x180 accept, 0x0F vehicle+aircraft gate, 0x10 reserve |
| `Bunker=`      | BuildingType+0x16AB | 0x0E deploy-here, 0x0F gate, 0x15 auto-deploy |
| `Hospital=`    | BuildingType+0x16C1 | 0x0E infantry-accept, 0x0F gate, 0x15 UNLOAD |
| `Armory=`      | BuildingType+0x16C2 | 0x0E infantry-accept, 0x0F gate (per-house cap) |
| `UnitAbsorb=`  | BuildingType+0x16AE | 0x0F (Grinder), 0x15 |
| `InfantryAbsorb=` | BuildingType+0x16AF | 0x0F (Grinder), 0x15 |
| `Grinding=`    | BuildingType+0x16AD | 0x0F always ROGER |
| `Helipad=`     | BuildingType+0x16CB | 0x0E MOVE_TO_CELL self, 0x0F aircraft-type filter |
| `Passengers=`  | UnitType+0x5E0  | 0x0E size-limit arithmetic, 0x0F naval/cap gate |
| `SizeLimit=`   | UnitType+0x388  | 0x0E size comparison |
| `Size=`        | UnitType+0x380  | 0x0E (sender side), 0x0F |
| `Gunner=`      | UnitType+0xDFC  | 0x08 Aircraft carryall gate |
| `MovementZone=`| UnitType+0x5B4  | 0x0E MapClass::GetZoneID match |
| `OpenTopped=`  | UnitType+?      | Passenger-side weapon swap (not in Receive_Radio directly; set by mission code) |
| `AllowGarrisonByType=` | HouseType+0xE0E/0xE0F | 0x0F garrison branch |
| `ConditionYellowRepair` | Rules+0x16F8  | 0x1C, 0x22 thresholds |
| `ConditionYellow` | Rules+0x1700  | 0x1C repair-complete anim trigger |

All of these are read by the Receive_Radio handler on the **receiver's** TypeClass.
The sender's type flags only rarely matter (e.g. naval-zone type+0x5B4, Size type+0x380).

---

## 10. Current Rust implementation status

**There is no RadioClass-style protocol in the Rust codebase today.** The engine models
every dock/board/tether handshake as **direct state mutation** against per-entity fields
or central reservation maps. Summary (see codebase scan in this investigation's Step 1):

| gamemd.exe mechanism                               | Rust equivalent                                                    |
|----------------------------------------------------|--------------------------------------------------------------------|
| `Contacts[]` single slot on harvester              | (none — link is implicit in `DockReservations.occupied`)           |
| `HELLO` + add to Contacts[]                         | `DockReservations::try_reserve()` return value                     |
| `BREAK` + null Contacts slot                        | `DockReservations::release()` / `cancel()`                         |
| Harvester dock state machine (0x08, 0x0E, 0x0C, 0x15) | `RefineryDockPhase` (Approach/WaitForDock/…/ExitPad) in `src/sim/miner/miner_dock_sequence.rs` |
| Repair-tick 0x1C                                    | `docking/building_dock.rs` service_timer + no_funds_ticks          |
| Aircraft reload 0x1F / 0x21                         | `aircraft_ammo` + `AirfieldDocks` in `docking/aircraft_dock.rs`    |
| Passenger boarding 0x0F                             | `PassengerRole::Boarding { target_transport_id, phase }` in `sim/passenger.rs` |
| IFV gunner 0x0F + 0x15 side-effect                  | `entity.ifv_weapon_index: Option<u32>` mutated at board/unload     |
| Carryall `WANT_RIDE` 0x24 + `NEED_TO_MOVE` 0x13     | (not implemented; Carryall=yes present only on [HIND], TechLevel=-1 — dormant in standard YR) |
| BREAK-broadcast on limbo (`Broadcast_Radio_ToAll(3)`) | Per-system cleanup hooks at entity despawn — verify each system severs its side refs |
| Slave master link                                   | `slave_harvester.master_id: u64` + `ProductionState.slave_bindings` |

### What we lose by not modeling the protocol explicitly

1. **Handshake failure modes collapse.** `try_reserve()` returns a boolean; gamemd's
   `Transmit_Radio(HELLO)` can return ROGER, NEGATORY, or — critically — **evict a
   prior contact and get re-assigned**. The eviction produces observable behavior:
   a harvester redirected mid-move to a repair pad loses its refinery queue slot.
   Our current engine doesn't model this mid-flight eviction.
2. **Multi-stage choreography is baked into the sub-state enum.** gamemd's dock
   sequence is a conversation (0x08 → 0x0E → 0x0C → 0x15 → 0x19 → 0x03) that both
   parties drive; our `RefineryDockPhase` is a unidirectional state machine on the
   miner side. Parity-wise this is close enough for refineries but will break down
   for any future system that needs the building to also drive the conversation
   (bunkers, carryall, ifv gunner swap on the fly).
3. **`BREAK` side effects are lost.** gamemd's BREAK runs `ObjectClass::Receive_Radio`
   on the peer to pump anim-state and ambient-idle resets. Dropping the radio layer
   means any building ambient-anim transitions that fire on contact break must be
   triggered explicitly from the docking state machine exit.
4. **`DockedIn` byte lock (0x18/0x19/0x1A/0x1B)** — a 2-bit lock that prevents
   re-entry while a dock sequence is mid-anim. Not mirrored in Rust; currently
   relies on phase enum being in a servicing state.

### What we keep

The Rust approach is **more deterministic and lockstep-safe** than radio: no
global scratch buffer (`g_RadioScratchBuffer`), no non-compacting sparse array
reuse, no mid-tick state flips via synchronous RPC return values. The question
for parity is whether any **observable** difference slips through the abstraction.

---

## 11. Open Questions

1. **Canonical name for msg 0x11.** Re-verified 2026-04-24: 0x11 is NOT
   `NEED_TO_MOVE` (that's 0x13 — confirmed by the Carryall LAND debug string at
   0x00817C14). The mission-7 gate in FootClass suggests `IS_UNIT_LINKED` or
   `ARE_YOU_COMING_TO_ME`, but no binary string names it. Need to grep every
   `Transmit_Radio(0x11, ...)` call site to see what sends it and in what
   state. Likely candidates: `BuildingClass::Mission_Unload` polling docked
   harvesters, or `FootClass::Mission_Enter` arrival confirmation.
2. **Message 0x0D is "anim reset"** in ObjectClass but **silent swallow for WeaponsFactory**
   in BuildingClass. What sends 0x0D in the live game? I saw no `Transmit_Radio(0x0D, …)`
   call during this investigation. Candidate: it may be sent by the production-anim-complete
   callback or by `ObjectClass::Detach` on destruction. **To verify:** xref all immediate
   0xD stores and 0xD as an argument in call-site disassembly.
3. **`field_0xA5` gating on AircraftClass.** Partial (see §8.4). Not initialised in
   Aircraft/Foot/Techno constructors — zero from `operator_new`. Gates mission states
   `{RETREAT=4, Paradrop Approach=0x1A, Paradrop Overfly=0x1B, Spyplane Approach=0x1E, Spyplane Overfly=0x1F}`
   only (corrected per §6.1 authoritative table; prior text had wrong names Selling/Repair/ParaDropApproach/ParaDropOverfly;
   confirmed via `decompile_function(0x004190B0)` switch cases 4/0x1A/0x1B/0x1E/0x1F, 2026-05-20).
   The write site is almost certainly inside `Mission_ParaDropApproach` @
   `0x004155F0` or `Mission_ParaDropOverfly` @ `0x004157C0` — check the first tick of
   each. Non-blocking because all five gated states are aircraft-specific scripted flight
   modes outside the parity-critical docking loop.
4. **`Broadcast_Radio_ToAll` — who calls it?** Resolved (see §8.5).
   `TechnoClass::Limbo_Tail_CallConceal` @ `0x0065AA80` broadcasts BREAK(3) to every
   contact when the Techno enters limbo. Single primary caller; additional vtable-slot
   registration at `0x007F05DC` not yet identified. This is the engine's "sever all
   radio links on despawn" hook and is observationally important for parity (mind
   control disconnects, transport-death passenger ejection, carryall mid-flight death).
5. **The `RadioHistory` dedup array is WRITE-only from this base handler.** Status
   after re-check: no reads surfaced across the decompiled handlers (see §8.6).
   Working hypothesis: inert TS-era debug log. A definitive answer requires a
   binary-wide instruction scan for reads of `[ECX+0xD4]`/`[ECX+0xD8]`/`[ECX+0xDC]`
   which this pass did not perform. Are there save/load serialisers or
   debug logs that use it? If not, it may be inert in YR (TS-era debug artifact).
6. **`Filter_AbstractType_InMap` Ghidra-labeled name doesn't match its call-site use.**
   In `ObjectClass::Select` (per its Ghidra label) it filters candidates. In
   `Transmit_Radio_Impl` it's called on `ESI=this` to validate sender identity. Same
   function, two semantically different uses. Consider re-labeling to
   `Self_Or_Null_If_Not_OnMapTechno`.
7. **IFV / OpenTopped tether is NOT in Receive_Radio.** The IFV weapon-swap happens at
   board-time via `SetGunnerWeapon(slot)` and at unload via `SetGunnerWeapon(0)` — no
   radio message drives it. This is a separate passenger subsystem, despite the
   "tether" framing in the task description. For our engine the Rust `ifv_weapon_index`
   field is a faithful model of this behavior; the radio protocol itself isn't involved.

---

## 12. Address Summary

| Symbol                                       | Address       | Notes |
|----------------------------------------------|---------------|-------|
| `RadioClass::Constructor` (int)              | `0x0065A750`  | Full ctor, allocates 1-slot Contacts |
| `RadioClass::Constructor` (copy)             | `0x0065A7E0`  | Copy ctor, delegates to ObjectClass |
| `RadioClass::Receive_Radio`                  | `0x0065A820`  | Base — HELLO/BREAK + history shift |
| `RadioClass::Transmit_Radio_Impl`            | `0x0065A970`  | Core dispatch (vtable+0x27C) |
| `RadioClass::Transmit_Radio`                 | `0x0065AAA0`  | Wrapper: msg + target → Impl |
| `RadioClass::Transmit_Radio_ToFirst`         | `0x0065ACB0`  | Wrapper: implicit target=Contacts[0] |
| `RadioClass::Broadcast_Radio_ToAll`          | `0x0065ACE0`  | Loop all Contacts, Transmit_Radio_Impl |
| `RadioClass::FindDockSlot`                   | `0x0065AD90`  | Linear search → slot index or -1 |
| `RadioClass::Set_Contact_Count`              | `0x0065AE60`  | Resize Contacts (only grows) |
| `RadioClass::Tether_Count`                   | `0x006B7D80`  | Different struct; **not** the RadioClass Contacts accessor (misleadingly named) |
| `ObjectClass::Receive_Radio`                 | `0x005F5320`  | Upstream base: msg 0x0D, 0x22 |
| `TechnoClass::Receive_Radio`                 | `0x006F4AB0`  | Covers dock/repair byte flips + repair formula |
| `FootClass::Receive_Radio`                   | `0x004D8FB0`  | 0x11, 0x12, 0x13, 0x17, 0x1C, 0x23 |
| `UnitClass::Receive_Radio`                   | `0x00737430`  | 0x03, 0x07, 0x0E, 0x0F, 0x15, 0x16, 0x17, 0x24 |
| `AircraftClass::Receive_Radio`               | `0x004190B0`  | 0x08, 0x0E, 0x0F, 0x12, 0x13, 0x15, 0x17, 0x1D, 0x1F, 0x21 + state guard |
| `BuildingClass::Receive_Radio`               | `0x0043C2D0`  | 0x03, 0x08, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x15 |
| `BuildingClass::Constructor`                 | `0x0043B740`  | Calls `Set_Contact_Count(type+0x1780)`; body spans 0x0043B740–0x0043BCEF (verified via `get_function_by_address(0x0043BCD0)` → Entry: 0043b740, 2026-05-20) |
| `Filter_AbstractType_InMap` (a.k.a. Self_Or_Null_If_Not_OnMapTechno) | `0x0040DD70` | RTTI gate: returns `this` only for type codes 1/2/6/0xF |
| `g_RadioScratchBuffer` (shared payload)      | (data)        | `&g_RadioScratchBuffer` passed as payload by Transmit_Radio / Broadcast_Radio_ToAll |

### Key data-structure offsets

| Class / offset                 | Meaning                               |
|--------------------------------|---------------------------------------|
| `TechnoClass+0xD4`             | RadioHistory[0] (most recent msg code) |
| `TechnoClass+0xD8`             | RadioHistory[1] |
| `TechnoClass+0xDC`             | RadioHistory[2] |
| `TechnoClass+0xE0`             | Contacts vtable (DynamicVectorClass) |
| `TechnoClass+0xE4`             | Contacts array base pointer |
| `TechnoClass+0xE8`             | Contacts capacity |
| `TechnoClass+0xEC`             | Contacts CanGrow byte |
| `TechnoClass+0xED`             | Contacts Initialized byte |
| `BuildingType+0x1780`          | NumberOfDocks (sized from `Dock=` list; feeds Set_Contact_Count) |
| `BuildingType+0x16A9..0x16CB`  | Dock/role flags read in cases 0x08, 0x0E, 0x0F, 0x10, 0x15 |

---

## 13. Reinvestigation log — 2026-04-24

This section records the verification pass that triggered the corrections above,
so future readers can see which claims were re-checked against live Ghidra and
which came from earlier research.

**Re-decompiled this pass** (every one at the listed address, decompile + disassembly cross-check):

- `RadioClass::Receive_Radio` @ 0x0065A820 — confirmed the 3-slot history shift
  writes only on *different-from-slot-0* messages, verified the BREAK and HELLO
  paths match the existing report, confirmed the reverse-ally-check gate is
  `AbstractFlags & 1` (= IsTechno bit) via disassembly at 0x0065A8DC.
- `RadioClass::Transmit_Radio_Impl` @ 0x0065A970 — assembly re-walked. Confirmed
  the full-Contacts eviction path (`this->vtable[0x278](3, Contacts[0])`) at
  0x0065aa2e, and the ROGER-gated slot-commit at 0x0065aa4f.
- `RadioClass::Transmit_Radio` @ 0x0065AAA0, `Transmit_Radio_ToFirst` @ 0x0065ACB0,
  `Broadcast_Radio_ToAll` @ 0x0065ACE0, `FindDockSlot` @ 0x0065AD90,
  `Set_Contact_Count` @ 0x0065AE60 — each confirmed as a one- or two-line
  wrapper as the report describes.
- `TechnoClass::Receive_Radio` @ 0x006F4AB0 — re-decompiled; confirms the
  0x07/0x09/0x16 → send-0x18-then-delegate pattern and the 0x08 → send-0x19-then-BREAK
  pattern documented in §6.4.
- `FootClass::Receive_Radio` @ 0x004D8FB0 — re-decompiled; this is where the
  prior-report 0x11/0x13 labeling error was caught. Case 0x11 gates on mission==7
  and returns 1 (no debug string). Case 0x13 writes destination into *payload
  and returns 10-on-busy / 1-on-idle — and IS the code Carryall sends from its
  LAND state at 0x00417272, which logs "RADIO_NEED_TO_MOVE got RADIO_ROGER".
- `UnitClass::Receive_Radio` @ 0x00737430 — re-decompiled; verified case 0x0E
  (CAN_DOCK) fires 0x13 NEED_TO_MOVE → 0x12 MOVE_TO_CELL → (on fail) 0x03 BREAK
  sequence from the unit side, which is the harvester↔refinery active-in-YR
  reuse of code 0x13.
- `AircraftClass::Receive_Radio` @ 0x004190B0 — re-decompiled; confirms the
  pre-switch mission-state gate (missions 4/0x1A/0x1B/0x1E/0x1F guarded by
  `field_0xA5`) and the cap arithmetic in cases 0x0F/0x1D/0x1F/0x21.
- `AircraftClass::Mission_Move_Carryall` @ 0x00417272 — re-decompiled to bind
  each RADIO_* debug string to its sent message code. VALIDATE_LZ sends
  0x02→0x24→0x07 (or fails to 0x03). LAND sends 0x13. This is the primary source
  for 5.1 confirmation.
- `ObjectClass::Receive_Radio` @ 0x005F5320 — re-decompiled; confirms the base
  tail handles exactly 0x0D and 0x22 and returns 0 for everything else.
- `Filter_AbstractType_InMap` @ 0x0040DD70 — re-decompiled; confirms type-code
  filter (1=Unit, 2=Aircraft, 6=Building, 0xF=Infantry) with everything else
  nulled out. Used in Transmit_Radio_Impl to filter the SENDER before passing
  to target's Receive_Radio.
- `HouseClass::Is_Ally_ByObject` @ 0x004F9A90 — re-decompiled; confirmed
  two-sided use in the HELLO handler (sender.House.Is_Ally(this) on line 1,
  this.House.Is_Ally(sender) gated on IsTechno on line 2).

**Correction applied:** swap codes 0x11 (now `IS_UNIT_LINKED`-inferred) and
0x13 (now confirmed `NEED_TO_MOVE` from the Carryall debug string).

**New finding logged:** HIND is the only `Carryall=yes` unit in YR rulesmd.ini
(line 10822) and has `TechLevel=-1`, so the pickup protocol that uses codes
0x13+0x24 is dormant in standard YR skirmish — but 0x13 is ALSO used (fully
active) by the harvester↔refinery CAN_DOCK sub-protocol. Added as §5.2.

**No other claims changed.** The struct layout (§2), send path (§3), receive path
(§4), subclass cheat-sheet (§6), repair formula (§8.2), INI key mapping (§9),
and address summary (§12) all re-verified cleanly against this pass.

---

## Sources

- Live Ghidra decompilation of `gamemd.exe`:
  `0x0065A750`, `0x0065A7E0`, `0x0065A820`, `0x0065A970`, `0x0065AAA0`, `0x0065ACB0`,
  `0x0065ACE0`, `0x0065AD90`, `0x0065AE60`, `0x0040DD70`, `0x005F5320`, `0x006F4AB0`,
  `0x004D8FB0`, `0x00737430`, `0x004190B0`, `0x0043C2D0`, `0x0043BCD0`.
- Existing reports cross-referenced and extended, **not** duplicated:
  - [BUILDINGCLASS_MISSILE_AND_RADIO_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MISSILE_AND_RADIO_GHIDRA_REPORT.md) — BuildingClass::Receive_Radio cases & ResponseType enum
  - [MISSION_ENTER_REFINERY_DOCK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_ENTER_REFINERY_DOCK_GHIDRA_REPORT.md) — harvester dock handshake sequence
  - [HARVESTER_DOCK_UNLOAD_SEQUENCE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/HARVESTER_DOCK_UNLOAD_SEQUENCE.md) — in-depth refinery unload choreography
  - [IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md) — IFV gunner swap mechanics (non-radio)
  - [BUILDING_DOCK_AND_HEAL_STATE_MACHINES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/BUILDING_DOCK_AND_HEAL_STATE_MACHINES.md) — hospital/armory healing cycle
  - [TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_EXPANDED_STRUCT_LAYOUT.md) — confirms 0xD4–0xED RadioClass fields
  - [ABSTRACTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ABSTRACTCLASS_GHIDRA_REPORT.md) — inheritance chain
- Binary strings: `0x00817C14`, `0x00817E04`, `0x00817E58` (Carryall debug traces naming
  `RADIO_HELLO`, `RADIO_ROGER`, `RADIO_NEED_TO_MOVE`, `RADIO_WANT_RIDE`).
- INI key mapping: `ini/rulesmd.ini` §Dock*/Passengers/SizeLimit/Gunner keys.
- Rust codebase survey: `src/sim/miner/miner_dock.rs`, `src/sim/miner/miner_dock_sequence.rs`,
  `src/sim/docking/building_dock.rs`, `src/sim/docking/aircraft_dock.rs`, `src/sim/passenger.rs`,
  `src/sim/game_entity.rs`, `src/sim/slave_miner.rs`.
