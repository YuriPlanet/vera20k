# Optional Ares veterancy, academies, build time and cloning

Build with `--features ares` to let types choose how they gain experience,
what a promotion does, how long they take to build and where they are
cloned. Default builds keep the stock kill award (`0x00702E9D`), promotion
detector (`0x006FA054`), Time_To_Build (`0x006F47A0`), FactoryPlant factor
(`0x0050BEB0`) and Cloning Vat loop; INI keys cannot enable the extension
there.

```ini
[AudioVisual]
VeteranFlashTimer=0                ; a veteran's flash (elite: EliteFlashTimer)

[APC]                              ; any TechnoType
Promote.VeteranType=APC2           ; converted in place on promotion
Promote.VeteranExperience=0.5      ; added after a conversion, capped
Promote.IncludePassengers=yes      ; trainable passengers share the rank
Promote.VeteranFlash=10            ; also Promote.EliteType/Experience/Flash
EVA.VeteranPromoted=none           ; default EVA_UnitPromoted
Experience.PromotePassengers=yes   ; also FromPassengers, PassengerModifier,
Experience.SpawnModifier=1.0       ;   SpawnOwnerModifier, MindControl*Modifier,
Experience.FromAirstrike=no        ;   FromAirstrike, AirstrikeModifier
BuildTime.Speed=0.5                ; also Cost, LowPowerPenalty, MinLowPower,
FactoryPlant.Multiplier=1.0        ;   MaxLowPower, MultipleFactory
Cloneable=yes
ClonedAt=NACLON                    ; else vats (infantry) or CloningFacility
ClonedAs=INIT

[ACAD]                             ; BuildingType
Academy.InfantryVeterancy=1.0      ; also Aircraft/Vehicle/BuildingVeterancy
Academy.Types=E1                   ; empty: every type
Academy.Ignore=E2
CloningFacility=yes                ; clones vehicles of its Naval=
```

## Where VERA20K owns it

- `rules/ares_veterancy.rs`, `rules/ares_production.rs`: the TechnoTypeExt
  and BuildingTypeExt reads (Ares `1003DFB0`, `1000DD40`), RulesExt
  `VeteranFlashTimer` (`1002AB60`) and the ClonedAt/CloningFacility
  validation (`100677B0`). The EVA cues bind with the other EVA references.
- `sim/ares_veterancy.rs`: `UpdateVeterancy` (`10046AF0`, hook `0x006FA054`)
  and the academy list and bonus (`10022760`, `10020750`; hooks
  `0x00446366`, `0x00445905`, `0x00448AB2`, `0x004491D5` and the
  InitFromType hooks, applied at `world_spawn/construction.rs`).
- `sim/combat/record_kill.rs`: who gains a kill's experience (`1004EEF0`,
  hook `0x00702E9D`) and AddExperience (`1004EE30`).
- `sim/production/factory.rs`: GetBuildSpeed (`10041D40`) and GetBuildTime
  (`10041AB0`); `rules/ruleset.rs` HouseCostFactors: GetCostMult
  (`10041780`); `sim/production/production_queue.rs`: KickOutClones
  (`100127C0`, hooks `0x00444DBC`, `0x004445F6`, `0x0044441A`, `0x004449DF`).

## Not covered

- Reverse engineering is its own chain
  ([ares-reverse-engineering.md](ares-reverse-engineering.md)).
- `IKnowWhatImDoing` is VERA's placement scope: preplaced map objects skip
  academies because they are read inside it.
