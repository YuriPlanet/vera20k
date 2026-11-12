# Optional Ares mouse cursors

Build with `--features ares` to draw the software cursor through the Ares
`[MouseCursors]` list and to let types name the cursors of their actions.
Default builds keep the compiled cursor table (`0x0082D028`) and its action
switches (`DisplayClass::SetCursorFromAction @ 0x004AAE90`); INI keys cannot
enable the extension there.

```ini
[MouseCursors]                          ; rulesmd.ini, every Rules pass
Spin=100,8,2,-1,-1,center,middle        ; Frame,Count,Interval,MiniFrame,MiniCount,HotX,HotY
Move=Spin                               ; a name copies that entry's cursor

[MTNK]                                  ; TechnoType
Cursor.Move=Spin                        ; also NoMove, Enter, NoEnter, Deploy, NoDeploy
Cursor.NoMove=none                      ; none stores -1

[GAPILE]                                ; BuildingType
Cursor.Spy=Enter                        ; a spy's infiltration cursor

[105mm]                                 ; WeaponType
Cursor.Attack=Attack
Cursor.AttackOutOfRange=AttackOutOfRange

[SpyPlaneSpecial]                       ; SuperWeaponType with an Ares handler
Cursor=SpyPlane                         ; handler default; constructor Attack
NoCursor=Disallowed                     ; over a cell it may not fire at
```

## Where VERA20K owns it

- `rules/ares_mouse_cursor.rs`: the list (`MouseCursorTypes`). The defaults
  (`InitDefaults @ 10007130`) are the 86 vanilla rows under Ares' names, six of
  them animated first by `YR_GameInit_Pre @ 1005D7F0` (hook `0x52BA78`:
  ParaDrop, Chronosphere, IronCurtain, Detonate, `0x36`, IvanBomb), then
  Tote, EngineerDamage, TogglePower, NoTogglePower, InfantryHeal,
  UnitRepair, TakeVehicle, Sabotage and RepairTrench (`0x56..0x5E`). Each
  Rules pass appends its new keys and reads every entry in list order
  (`LoadBeforeTypeData @ 1002AEF0`, `MouseCursorTypeEntry::LoadFromINI @
  100075B0`), before the type bodies of the same pass. The type keys use
  `detail::read<MouseCursorType> @ 1000FB50` (TechnoTypeExt `1003DFB0`,
  BuildingTypeExt `1000DD40`, WeaponTypeExt `10055520`, SWTypeExt `10033370`
  with the handlers' Initialize defaults). The list lives with the Rules
  registries: saved, rebuilt at a destructive reset.
- `render/cursor_atlas_ares.rs`: the match cursor is built from the list
  (hooks `0x5BDC8C`, `0x5BDADF`, `0x5BDDC8`, `0x5BDE64`, `0x5BDBC4`,
  `0x5BDC1B`, `0x5BDB90`): every stock cursor draws its row's entry, so
  redefining a vanilla name changes it; the radar draws an entry's mini set,
  or its main set when it has none.
- `app/input/ares_cursor.rs`: the per-query override table
  (`Actions::CursorOverrides`, reset like `0x5BDDC0`), written where the
  What_Action port answers: `Cursor.Move`/`NoMove` and the hovered Techno's
  `Cursor.Enter`/`NoEnter` (`0x6FFEC0`, `0x700600`), the self-deploy cursors
  (`0x7000CD`, a bunkered unit's bunker at `0x7400F0`), the weapon's
  `Cursor.Attack` (`0x70055D`, `0x700AA8`) and, for a lone selection out of
  range, `Cursor.AttackOutOfRange` (`0x4AB35A`); Cursor.Spy or Sabotage for
  an infiltration (`0x51EE6B`), TakeVehicle for a hijack (`0x51E7BF`),
  RepairTrench over a rubble (`0x51E635`) and EngineerDamage (`0x51E5E1`).
  `DisplayClass_SetAction_CustomCursor @ 10057F10` then draws the action's
  override or its table cursor, after the shroud remap. A handled
  superweapon shows `Cursor` or `NoCursor` (`SWTypeExt::WhatAction @
  10033EB0`), on the tactical map and on the radar (`0x653B3A`).
- `app/input/radar.rs`: an overridden action stays on the radar only when
  its cursor has radar frames (`0x653CA6`); others keep the stock keep table.
  So over a Techno the radar also keeps Repair (Grinder, depot) and NoEnter,
  whose `Cursor.Enter`/`NoEnter` defaults have radar frames, and an armed
  Chronosphere leaves no radar action, selection or not.

Cursor choice is presentation only: no simulation state, RNG draw or hash
reads the list or the overrides.

## Not covered

- `DisplayClass_ChooseAction_CanSell` (`0x6929FC`) is ported with the unit
  sale ([ares-reverse-engineering.md](ares-reverse-engineering.md)).
- `DisplayClass_LeftMouseButtonUp_ExecPowerToggle` (`0x4ABFBE`) and the
  TogglePower/NoTogglePower actions: the player's toggle-power mode (the Ares
  TogglePower command, `0x69281E`) is not in VERA.
- `FootClass_ActionOnObject_Allow` (`0x4D7524`) only lets the Ares
  superweapon actions reach a Foot's click; VERA's radar drops a NoCursor
  click instead of dispatching it. `0x4AB366`, `0x4AB44A` and `0x5BDD9F` are
  0.A hooks 3.0 no longer installs.
- InfantryHeal and UnitRepair (`0x5A`/`0x5B`): the heal arms (`0x51E710`,
  `0x73FDBD`) set them over an Organic or Infantry target and over any other
  one (`unit_repair_feedback`); an Infantryman's Heal shows the cursor
  directly.
- A cursor of -1 (`none`) or outside the list faults in Ares; VERA keeps the
  stock cursor. A zero Count, which Ares divides by, draws one frame.
- Overrides another selected object's What_Action left in the same frame are
  not reproduced; only the best object's answer writes them. An Attack that a
  class arm answers without the Techno rung still takes `Cursor.Attack`.
- Unported stock redirects (ParaDrop, IronCurtain, ForceShield,
  PsychicReveal) keep the stock superweapon cursor; UnitDelivery and the other
  handled types now show their `Cursor`.
