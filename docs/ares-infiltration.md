# Optional Ares building infiltration and sabotage

Build with `--features ares` to let a BuildingType choose what a spy does to
it and to let infantry blow buildings up on entry. Default builds keep the
original one-arm infiltration (`BuildingClass::Infiltrate @ 0x004571E0`);
INI keys cannot enable the extension there.

```ini
[BANK]                                  ; rulesmd.ini BuildingType
SpyEffect.Custom=yes                    ; the Ares arms replace the original one
SpyEffect.StolenMoneyPercentage=25%     ; a float; StolenMoneyAmount caps it
SpyEffect.StolenMoneyAmount=2000
SpyEffect.PowerOutageDuration=900       ; frames
SpyEffect.SabotageDelay=-1              ; negative: [General] C4Delay
SpyEffect.SuperWeapon=NukeSpecial       ; granted to the spy, one time unless
SpyEffect.SuperWeaponPermanent=no       ;   permanent
SpyEffect.NavalVeterancy=yes            ; also Unit/Infantry/Vehicle/Aircraft/Building
SpyEffect.RevealProduction=yes
ImmuneToSaboteurs=no                    ; unset: immune unless CanC4 and not civilian

[DEMO]                                  ; InfantryType
Infiltrate=yes
Saboteur=yes                            ; plants a C4Delay charge and is spent
```

## Where VERA20K owns it

- `rules/ares_infiltration.rs`: every `SpyEffect.*` key and
  `ImmuneToSaboteurs`, read per rules pass in the DLL's order (Ares
  `1000DD40`), and `IsSabotagable` (`1000D790`). `Saboteur=` is read with
  the other TechnoType extension keys in `rules/object_type.rs`.
- `sim/spy_infiltration.rs`: `InfiltratedBy` (`10012000`, hook
  `0x004571E0`): an allied house's spy changes nothing, a custom type takes
  each Ares arm its keys name (reshroud, power outage, Super reset and
  grant, C4 fuse, money, veterancy, revealed production), and the app picks
  the EVA lines per arm. The house's naval, aircraft and building veterancy
  live in `HouseState`; construction and the sidebar's elite cameo read
  them (Ares `100237C0`, `10023600`, `100236D0`, `1003D800`).
- `sim/world/world_orders.rs`: `GetInfiltrateAction` (`100252D0`) decides
  the click (`0x0051EE6B`), the walk to the target (`0x0051B2CB`), the
  attack rung (`0x007004AD`) and the arrival (`0x00519FF8`);
  `sim/c4_plant.rs` plants a Saboteur's charge.

## Not covered

- `SpyEffect.RevealRadar`/`KeepRadar` add the spy to the building's
  DisplayProductionTo only: VERA keeps no RadarVisibleTo, so the spy does
  not see the owner's sight.
- `SpyEffect.StolenTechIndex`, `Cursor.Spy` other than its default `Enter`,
  and `SpyEffect.UndoReverseEngineer`'s list (reverse engineering is not
  ported; only its tech recheck and EVA run).
- The revealed factory's production cameo, `EnemyUIName`, the observer arms
  of the status text and `Fake=`.
