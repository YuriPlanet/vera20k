# Optional Ares trigger events and actions

Requires an explicit `--features ares` build. Default builds keep the stock
trigger bodies: an event kind outside 0..61 is always met and the stock
TechTypeExists/DoesNotExist (60/61) stay as they are; see
[build capability](ares.md).

```ini
[Events]
WARP=1,77,2,3,ChronoSphere   ; a ChronoSphere fired within 5 cells of waypoint 3
LOST=1,85,0,1                ; this object was destroyed by House 1
FEW=1,82,2,2,HTNK            ; the Trigger's House owns at most 2 HTNK
[Actions]
POWER=1,146,11,0,0,0,0,0,200 ; House 0 (Value) gains 200 power (token 8)
```

The ports follow the installed Ares 3.0 bodies cited beside the Rust: TEventExt
HasOccured `0x1004F7A0` (hook `0x71E949`), GetFlags `0x1004F6A0` (hook
`0x71F683`), TActionExt Execute `0x100396C0` (hook `0x6DD8D7`) and GetFlags
`0x10039930` (hook `0x6E3EE0`). No Ares DLL is loaded.

## Events

Ares answers its own kinds before the stock switch. None writes the
persistence flag or latches: the stock StateA (`0x71F950`) is false above 59,
so Ares' Persistable answer (hook `0x71F9C0`) never reaches a latch.

| Kind | Met when | Raised by |
| --- | --- | --- |
| 60, 61 | at least Value (60) or none (61) of the named type exist | every Logic poll |
| 62..65 | the object is (62, 63) or is no longer (64, 65) under EMP; 63/65 by House Value | the EMP owner |
| 66 | always | the spotlight's scan, after 35 |
| 67..70 | driver killed (67, 68) or vehicle taken (69, 70); 68/70 by House Value | KillDriver, vehicle theft |
| 71, 72 | the object was abducted; 72 by a Techno of House Value | the abduction, on the abductee |
| 73, 74 | the object abducted something; 74 of House Value (the previous owner) | the abduction, on the abductor |
| 75, 76 | a superweapon of type index Value was activated (75) or deactivated (76) | the owner's House Tags |
| 77 | the named superweapon fired within 5 cells of waypoint Value | the owner's House Tags |
| 78 | the Trigger's House reverse engineered the named type | never met (see below) |
| 79, 80 | something (79) or the named type (80) was reverse engineered | not raised |
| 81, 82 | the Trigger's House owns at least Value (81) or at most Value (82) of the named type | House Tag polls |
| 83, 84 | the object was attacked or destroyed; 84 by House Value | every damage the object survives so far |
| 85 | the object was destroyed by House Value | RecordKill |
| 86 | at most Value of the named type exist | every Logic poll |
| 87, 88 | House Value has no KeepAlive objects (87) or KeepAlive Buildings (88) | House Tag polls |

House Value is the House array index; 87/88 resolve it as below. Type names
match without case (TechnoTypeClass::Find `0x10007EA0`); a significant type is
counted through each House's tracked count, an Insignificant or DontScore one
through its owned objects. KeepAlive Buildings is HouseExt `+0x1C`, kept with
the KeepAlive count by the House tracking owner, saved and hashed.

Damage also raises 6 and 44 without an attacker, then 84 and 83 while the
object lives (hook `0x5F57B5`). RecordKill raises 85 with the destroyer's
House: for a Unit before 7 (hook `0x744745`), for other classes past the class
test (hook `0x702DD6`), and from the House-sourced kill after 4 for every class
(hook `0x7032B0`). A launch of a new superweapon type raises 77 then 75 once
the common Launch has finished (SWTypeExt::Activate `0x10032020`);
Deactivate (`0x100325D0`) raises 76 after the type's own Deactivate.

GetFlags makes 62..74, 79, 80, 83 and 84 attached-object events, 75..78, 81,
82, 85, 87 and 88 House Tag events and 86 a Logic Tag event.

## House parameters

HasOccured's lookups of the event value (hooks `0x71ED01`, `0x71ED33`,
`0x71F06C`, `0x71F1C9`, `0x71F1ED`) take a `<Player @ X>` selector as that
start slot's House and 8997 as no House; other values stay Countries. The
shared lookup that most events end in returns false for a Player@X slot
without a House (stock: true). Attacked by House (44) turns a Player@X value
into that House's index first (hook `0x71EE79`).

## Actions

- 146: the House Value names (the stock House selector) gains Value2
  (token 8, parameter type 11) auxiliary power.
- 147: every Foot whose Tag holds this Trigger, alive, placed, outside Limbo,
  with a live driver that CanKillDriver(1.0) admits, loses its driver to the
  House Value names, else to `Special`; true when any Foot qualified.
- 148: the announcer voice becomes Value (at least -1) when it is below the
  voice count (three stock tags plus `[EVATypes]`); the client applies it.
- 149: the triggering object, while alive, joins group Value.
- 92/93 (Activate/Deactivate Firestorm) raise or drop the trigger House's
  firestorm ([ares-firestorm.md](ares-firestorm.md)).
- 98 (play a sound at a random waypoint) chooses among every defined
  waypoint, without the 400 cap, and draws nothing when none is defined
  (`0x10039B60`, hook `0x6E1780`).

147 and 149 are attached-object actions.

## Where VERA20K owns it

- `sim/trigger_runtime_ares.rs`: HasOccured, TechTypeExists, the Ares raise
  entries and the actions; `sim/trigger_runtime.rs` calls them first and owns
  GetFlags and the House parameter lookups.
- Raise sites: `sim/combat/object_health.rs`, `sim/combat/record_kill.rs`,
  `sim/world/spotlight.rs`, `sim/abduction.rs`, `sim/superweapon/fire.rs`
  and `sim/superweapon/mod.rs`.
- `sim/house_tracking.rs` (KeepAlive Buildings), `sim/power_system.rs`
  (AuxPower), `sim/vehicle_theft/driver_kill.rs` (KillDriver),
  `sim/world/object_sound.rs` (action 98), `audio/vox.rs` (the voice).

## Not included

- Reverse engineering (Grinding `ReverseEngineersVictims`) is not ported, so
  78 is never met and 79/80 are never raised.
- 76 from a drain running out or a hold runs its Triggers without the frame's
  sound boundary and overlay table, which those House passes do not receive.
- PrepareSaveToINI's parameter kinds (hook `0x71F39B`) and the action write
  modes (hook `0x6E3B60`): VERA never writes trigger INIs.
