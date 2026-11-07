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
  (`rules/ares_bullet_type.rs`, BulletTypeExt `100193C0`).
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
  `AlliedSolidTransparency` belong to the
  [line-of-fire chain](ares-line-of-fire.md).
- The sale, capture, degrade and power keys: part 2 below.

## Part 2: sale, return, capture, power and AI keys

```ini
[GAPOWR]                                ; rulesmd.ini BuildingType
BuildupTime=0.1                         ; minutes; unset: [General] BuildupTime=
SellTime=0.05                           ; minutes; unset: the build-up's
Returnable=yes                          ; unset: [General] ReturnStructures=
LostEvaEvent=EVA_TechBuildingLost       ; the default; none: silent
Message.Capture=MSG:OilCaptured         ; CSF labels, printed for the player
Message.Lost=MSG:OilLost
Degrade.Amount=5                        ; unset: Degrade.AmountConsumer/Normal
Degrade.Percentage=50%                  ; unset: [General] Degrade.Percentage
AIBuildCounts=3,2,1                     ; per AI difficulty, hardest first
AIExtraCounts=1,1,0                     ; up to this many more, at random
AIBaseNormal=yes                        ; unset: the stock BaseNormal test

[GAREFN]                                ; artmd.ini, the type's Image=
DockUnloadCell=3,1                      ; the pad offset (default 3,1)
DockUnloadFacing=8                      ; 0..31 (default 8, east)

[Americans]                             ; a country
Degrades=no                             ; unset: not MultiplayPassive=

[General]
ReturnStructures=no
Degrade.Enabled=no
Degrade.Percentage=                     ; unset: ConditionYellow=
Degrade.AmountNormal=0
Degrade.AmountConsumer=1
TogglePowerAllowed=no
TogglePowerDelay=45

[IQ]
TogglePower=-1                          ; the IQ a computer needs to balance
```

- `rules/ares_building_type.rs` reads the BuildingTypeExt keys (`1000DD40`,
  the art ones through `1000FAE0` and `1000FBF0`) and the RulesExt ones
  (`LoadBeforeTypeData 1002AB60`, `LoadAfterTypeData 1002A800`);
  `LostEvaEvent` binds against EVAMD in `RuleSet::bind_type_eva_references`
  and the country `Degrades` is read with the other CountryExt keys
  (`1001C5D0`, inherited by `1001BDC0`) in `native_processing.rs`.
- `rules/buildup_asset_catalog.rs`: `UpdateBuildupFrames` (`1000F450`,
  hooks `0x45F2B4`, `0x465A48`, `0x45EAA5`) rates the build-up in Ares'
  divide-first order from `BuildupTime` and the sale from `SellTime`;
  `sim/building_construction.rs` starts any body mode at that sale rate
  while the mission is Selling (`100144E0`, hook `0x447A63`).
- `sim/world/house_defeat.rs`: Blowup_All returns a building first owned
  by a passive house to it (`10024150`, hook `0x4FC731`), killing its
  occupants, handing it back unannounced, resetting it to Guard and
  disabling a `NeedsEngineer=` one.
- `sim/world/world_orders.rs` and the app's `sound_dispatch`,
  `eva_producers` and `input::messages`: a captured tech building's
  `LostEvaEvent` and its `Message.Lost`/`Message.Capture` lines for the
  player (`10013E40`, hook `0x44840B`).
- `sim/ares_degrade.rs`: low-power degrading on each `DamageDelay`
  (`100247A0`, hook `0x4F8B08`); the timer's expiries are derived from the
  frame (the House constructor starts it at frame 0 and the hook restarts
  it on each expiry).
- `sim/ares_toggle_power.rs`: computer power balancing
  (`UpdateTogglePower 100228A0`, hook `0x4F8440`) through `GoOffline` and
  `GoOnline`.
- `sim/base_plan_generation.rs`: `AIBuildCounts`/`AIExtraCounts` copies in
  the AI build list (`10023940`, hook `0x505C95`).
- `ObjectType::base_reservation_writer_eligible`: `AIBaseNormal` decides
  whether the AI base reservation counts the building (`IsBaseNormal
  10011810`, hooks `0x440C08`, `0x445A72`, `0x456370`).
- `sim/radio/receive.rs` and `sim/miner/refinery_dock.rs`: the dock pad at
  `DockUnloadCell` (`10014540`, hook `0x43CA80`) and the facing a unit
  docks and unloads at (`GetDockUnloadFacing 10044C00`, hooks `0x7376D9`,
  `0x73DF66`); Mission_Unload then compares the facing exactly and lets a
  mid-swing turret through.

### Not covered in part 2

- `UnitSell` and `UnitsUnsellable`: VERA has no unit sale on a repair depot
  (`FootClass::CanBeSold 0x004D9E70`, `FootClass::Sell 0x004D9F70`, the
  SELL event's unit arm `0x004C6F5A`), which `10048FD0` (hook `0x4D9EBD`)
  gates.
- `SecretLab.GenerateOnCapture`: part of the Ares Secret Lab rework
  (`SecretLab.PossibleBoons`, `UpdateSecretLab`), which VERA does not port.
- HouseExt `Degrades=` in a map's house section (`10021A70`): a house takes
  its country's answer.
- `GateUpSound`, `GateDownSound`: the gate sounds themselves are not
  played yet (`sim::gate_runtime`).
- `SlamSound` (`10010290`, hook `0x4FB2FD`) and `MassSelectable`
  (`10013CF0`, hook `0x459C03`): the stock `BuildingSlam` and the stock
  band-selection test stay.
- `ProduceCashDisplay` (`100117C0`): it rides Ares' replaced ProduceCash
  payout (`100146E0`, `10013D40`, `10014490`: hooks `0x43FD2C`, `0x4482BD`,
  `0x4409F4`, with the upgrades' cash timers), which VERA does not port.
- `AIInnerBase` (`10023860`, hook `0x506306`): its `CloakGenerator=` arm of
  `FindBaseBuildingSite` is not ported (`sim::ai_base_site`).
- `IsBaseNormal`'s per-building `NoBaseNormal` (a `Deliver.BaseNormal=no`
  delivery) is the superweapon chain's.
