# Optional Ares building types: rubble, garrisons and engineers

Build with `--features ares` to let a BuildingType turn into rubble when it
dies and back when an engineer rebuilds it, to tune garrisons for urban
combat, and to change what an engineer does to a hostile building. Default
builds keep the original destruction (`BuildingClass::DestructionEffects @
0x004415F0`), garrison admission (`BuildingClass::CanBeOccupiedBy @
0x00457CE0`) and engineer arms (`InfantryClass::What_Action_OnObject @
0x0051E3B0`, `InfantryClass::PerCellProcess @ 0x00519630`); INI keys cannot
enable the extension there.

```ini
[GAPOWR]                                ; rulesmd.ini BuildingType
Rubble.Destroyed=GAPOWR_R               ; the rubble it turns into (same foundation)
Rubble.Destroyed.Anim=FIRE3             ; played at its centre
Rubble.Destroyed.Owner=Default          ; Default, Invoker, Killer, Victim, Civilian,
                                        ;   Special, Neutral or Random
Rubble.Destroyed.Strength=0             ; 0: full; >0: capped; -100..-1: percent
Rubble.Destroyed.Remove=no              ; yes: leave nothing
EngineerRepairable=yes                  ; unset: Repairable=
IsPassable=no                           ; yes: Foots walk through its cells

[GAPOWR_R]
Rubble.Intact=GAPOWR                    ; what a friendly engineer rebuilds
Rubble.Intact.Strength=-1               ; the default: 1% of Strength

[CABHUT01]                              ; a garrisonable building
UC.PassThrough=50%                      ; a snapped shot hits an occupant instead
UC.FatalRate=10%                        ; ... who dies outright
UC.DamageMultiplier=100%                ; ... or takes this share of the damage
Bunker.Raidable=yes                     ; another house may garrison it while empty
CanBeOccupiedBy=E1,GGI                  ; empty: any Occupier=

[InvisibleLow]                          ; a projectile
SubjectToTrenches=no                    ; always passes through to the occupants

[General]
EngineerDamage=0.25                     ; 0 (default) bars entry above the capture level
EngineerAlwaysCaptureTech=yes
```

## Where VERA20K owns it

- `rules/ares_building_type.rs`: the BuildingTypeExt keys, read per rules
  pass in the DLL's order (Ares `1000DD40`; the Find-only type, anim and
  InfantryType readers `1000CA20`, `1000C9C0`, `1000ED90` belong to the
  rules registry), the rubble foundation check of `CompleteInitialization`
  (`1000D380`) and the RulesExt engineer keys (`LoadAfterTypeData
  1002A800`). `SubjectToTrenches=` is read with the projectile
  (`rules/projectile_type.rs`, BulletTypeExt `100193C0`).
- `sim/ares_rubble.rs`: `SwitchRubble` (`1000F2B0`), the destruction hooks
  (`10017160` at `0x441F12` before SpawnSurvivors, `10017130` and
  `KickOutOfRubble 10012970` at `0x441F2C` after KillPassengers, both from
  `combat::world_receiver`) and the engineer's rebuild (`100257A0`, hook
  `0x519FAF`), which kicks him out of the new building unspent.
- `sim/world/world_orders.rs`: the engineer's action over a rubble
  (`10025420`, hook `0x51E635`), `EngineerRepairable` (`10014850`, hook
  `0x51E4ED`, and the arrival at `0x519FAF`), and
  `GetEngineerEnterEnemyBuildingAction` (`10025220`) for the hostile
  action (hooks `0x51E5BB`, `0x51E5E1`) and arrival damage (`0x519D9C`).
- `sim/ares_garrison.rs`: the occupier admission (`10017060`, hook
  `0x457D58`, inside `passenger::can_dock_occupier_garrison`), the raid of
  another house's building on entry (`10017240`, hook `0x52297F`) and its
  return once empty (`evalRaidStatus 100136C0` after `0x4581CD`,
  `0x458729` and a pass-through hit). The raided owner is saved and
  hashed with the building; `production::can_sell_building` refuses a
  raided building (`10017200`, hook `0x4494D2`).
- `sim/world/object_entry.rs` and `object_turn.rs`: `IsPassable` in the
  Infantry and Unit `Can_Enter_Cell` building arms (`10014D70`, hook
  `0x51C4C8`; `10015100`, hook `0x73F7B0`) and the idle infantryman's
  scatter from a building (`10014DB0`, hook `0x51BD4C`).
- `sim/combat/world_receiver.rs`: `DamageOccupants` (`10019AC0`) in
  `BulletClass_Detonate`'s Ares effects (`100536E0`, hook `0x46920B`),
  for snapped shots only, after AttachEffect and before the permanent
  mind control; a shot that passes through is neutered.

## Not covered

- A rubble pair with different foundations stops Ares with a fatal error;
  VERA logs it and drops the link.
- Ares' RepairTrench (`0x5E`) and EngineerDamage (`0x57`) cursors: the
  ordinary repair and damage cursors show. The fogged-building cell arm of
  `EngineerRepairable` (`0x51FA82`) has no VERA counterpart.
- `KickOutOfRubble` restores the selection the sim keeps; nothing else of
  the client's selection.
- The occupant's TechnoExt `GarrisonedIn` (only Ares trenches read it) and
  the `Assaulter=` arms the original admission already leaves out.
- `SolidHeight`, `SolidLevel`, `SubjectToBuildings` and
  `AlliedSolidTransparency`: Ares replaces the line-of-fire walk
  (`0x4CC100`, `0x4CC310`, `0x4CC360`) and the bullet obstacle hooks
  wholesale, which `combat::line_of_fire` does not follow yet.
- The remaining BuildingTypeExt sale, capture, degrade and sound keys.
