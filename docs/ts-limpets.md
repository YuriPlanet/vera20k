# Tiberian Sun limpets

Firestorm's Limpet Drone deploys into a mine that clamps onto the first enemy vehicle to pass:
the vehicle slows down and its attacker sees what it sees, until a heal or a service depot
takes the limpet off. Yuri's Revenge dropped the code and its keys. VERA reads them as
Firestorm 2.0 does. No retail INI sets them, so stock games play as before.

```ini
; RULESMD.INI (or a map), Firestorm's own values
[LIMPY]                  ; a warhead
LimpetFactor=35          ; nonzero: every shot with this warhead is a limpet
Verses=0%,100%,100%,100%,100%

[LimpetBullet]           ; a projectile
Inviso=yes
AV=yes                   ; its weapon looks for vehicles only

[LIMP]
Damage=1
Range=2
Projectile=LimpetBullet
Warhead=LIMPY
Report=LIMPBOM1

[DLIMPET]                ; a building
Primary=LIMP
IsLimpetMine=yes
UndeploysInto=LIMPET

[LIMPET]                 ; a vehicle
DeploysInto=DLIMPET
IsLimpetDrone=yes
```

- A shot whose warhead has a nonzero `LimpetFactor=` at a unit, infantryman, aircraft or
  building fires nothing. The firer's house is added to the target's limpets, the weapon's
  `Report=` plays without a position, and the firer is deleted: no death, explosion, debris
  or kill credit. If that house already has a limpet on the target, the shot does nothing
  and the firer tries again on its next shot.
- `LimpetFactor=` is read as a number and truncated toward zero, so `35` is 35 and `35%`
  (0.35) is 0, an ordinary shot.
- A limpeted vehicle's speed is its `Speed=` times `(100 - LimpetFactor) / 100`, truncated;
  with several limpets the last one's factor counts. Infantry, aircraft and buildings keep
  their speed. A negative factor speeds the vehicle up, as in Firestorm. Above 100 the
  vehicle stops: Firestorm wraps the number round and drives it at an absurd speed.
- Each player whose house limpeted an object sees what that object sees. Their allies do
  not, unless they limpeted it too.
- Any heal (negative damage) or a service depot's repair takes every limpet off, even at
  full health. Deploying or undeploying carries the limpets to the new object.
- A service depot takes a limpeted vehicle at full health as it takes a damaged one, as
  Firestorm's does: sent to a busy depot it waits its turn, and a depot it is already in
  contact with does not turn it away. A repair vehicle, as in Firestorm, can be ordered
  onto a damaged one only.
- `AV=yes` makes the weapon's own target search look for vehicles only.
- A vehicle can drive into a limpet mine's cell, and a player cannot order a mine to
  attack: it picks its own targets.
- An `IsLimpetDrone=yes` vehicle draws its body as a ten-frame loop, a frame each game
  frame, whichever way it faces, and draws no turret.

Firestorm 2.0 evidence: `TechnoClass::Fire_At` `0x006301A0` (the arm `0x006301EB..0x0063032A`);
`UnitClass::Get_Max_Speed` `0x00659710` and its readers, which are gamemd's vt+0x38C readers
(`FootClass::GetCurrentSpeed` `0x004DB1C3`, the Drive brake `0x004B10AC`, the base-defense
threat score `0x004D985F`); `TechnoClass::Look` `0x00638160` (`0x0063829A..0x006382B8`);
`TechnoClass::Take_Damage` `0x006325A0` (`0x006326E1`); `TechnoClass::Receive_Message`
RADIO_REPAIR (`0x0062ABE1`); the depot's admission, `BuildingClass::Receive_Message`
(`0x00426B4A`) and `UnitClass::Assign_Destination` (`0x00657EA5`), and the repair vehicle's
click test in `UnitClass::What_Action` (`0x00656127`); `UnitClass::Unlimbo` (`0x0064F83D`),
`UnitClass::AI` (`0x0064E205`) and the Unit SHP draw (`0x00652F2D`) for the drone;
`UnitClass::Try_To_Deploy` (`0x00650D9E`) and
`BuildingClass::Mission_Deconstruction` (`0x004309E1`); `UnitClass::Can_Enter_Cell`
(`0x0065570A`); `BuildingClass::What_Action` (`0x0042ECC6`, `0x0042EF65`);
`WeaponTypeClass::Allowable_Threat` `0x00681150`; the readers `WarheadTypeClass::Read_INI`
`0x0066EE30`, `BulletTypeClass::Read_INI` (`0x00448027`), `BuildingTypeClass::Read_INI`
`0x00440A40` and `UnitTypeClass::Read_INI` `0x0065BBA0`. Code: `sim::limpet`.

How VERA maps it:

- Firestorm keeps one bit per house, which runs out past 32 houses; VERA keeps the houses.
- Firestorm has one map, the local player's. VERA gives each player's map the sight of what
  that player's house limpeted, and takes the sight back when the limpet comes off.
- A deploy or an undeploy carries the limpets before the new object appears, so its first
  reveal already includes the limpeting player. Firestorm copies them just after, and that
  player sees through the new object from its next sight update.
- The limpet arm runs after gamemd's own first test in FireAt, so a target in limbo takes no
  limpet. No ordinary order fires at one.

Not ported:

- Trigger event 55, which Firestorm raises when a limpet attaches to an object with a tag.
  Yuri's Revenge's event 55 is a different event, so a Tiberian Sun map's trigger on it
  does not fire.
- The limpeted selection cue. Firestorm adds 8 to a limpeted vehicle's selection-bracket
  frame (3 for a vehicle, `0x0062C3D3..0x0062C409`), and that shape is SELECT.SHP
  (loaded at `0x005879D0`), not PIPBRD.SHP, which Tiberian Sun does not have. Firestorm's
  SELECT.SHP has 16 frames, Tiberian Sun's 8: the same four bracket sizes repeated. It
  also draws a limpeted building's selection box in colour 5 where others use 15
  (`0x0062B122..0x0062B14B`, `0x0062BED0`). gamemd loads no SELECT.SHP and draws no
  building selection box; its health bar takes PIPBRD.SHP frame 0 for a vehicle and 1 for
  infantry (`DrawHealthBar` `0x006F693F`, `0x006F68AB`), with no limpet term. Neither cue
  has anything to land on, so a limpeted object looks like any other.
- An EMP on a limpet mine. Firestorm gives it a call of its own (`0x0042AA70`) in place of
  the EMP disable; VERA treats it as any building.
- The repair cursor over a limpeted building at full health, which Firestorm allows
  (`TechnoClass::Can_Repair` `0x006321D0`). Repairing a building never removed a limpet.
- Infantry walk through a mine's cell only if gamemd lets them: Firestorm's skip is in the
  vehicle's cell test alone, and its infantry test was not read.
