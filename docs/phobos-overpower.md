# Overpower thresholds

Phobos makes the Tesla Trooper charger numbers type-configurable: a building
type sets how many summed charger levels keep it online and how many make its
overpowered weapon fire, and a warhead sets each charger's level.

```ini
[BUILDING]                          ; BuildingType
Overpower.KeepOnline=2              ; Valueable<int>, default 2
Overpower.ChargeWeapon=1            ; Valueable<int>, default 1

[WARHEAD]                           ; WarheadType
ElectricAssaultLevel=1              ; Valueable<int>, default 1
```

Readers: `Ext/BuildingType/Body.cpp:242-243`, declarations `Body.h:92-93`,
constructor defaults `Body.h:216-217`; the warhead key is
`Ext/WarheadType/Body.cpp:398`, declaration `Body.h:221`, default
`Body.h:493`. `ElectricAssaultLevel` is read for every warhead; it only
matters when `ElectricAssault=yes` admits the warhead to the charger sample.

## Overpowered weapon latch

Hook `0x4400F9` (`Ext/Building/Hooks.cpp:902-956`, 6 bytes, skips to
`0x44019D`) replaces the stock Overpowerable block. It walks the building's
charger list back to front; a charger whose target is no longer the building,
whose `GetWeapon(1)` has no weapon, warhead or `ElectricAssault`, is removed.
The rest add their `ElectricAssaultLevel` to `overPower`. With
`Overpower.ChargeWeapon >= 0`:

```
IsOverpowered = overPower >= keepOnline + charge
             || (Owner power percentage == 1.0 && HasPower && overPower >= charge)
```

and a negative `ChargeWeapon` clears the latch. VERA reproduces this in
`sim/tesla_charging.rs::sample_tesla_charge`, called by
`TechnoClass::AI`'s building tail `update_tesla_charge` only for an
`Overpowerable` type, exactly like the hook's own gate. The per-charger level
is `electric_assault_level` (weapon, warhead and `ElectricAssault` must all
exist); the full-power arm compares the house ratio with
`power_system::native_power_ratio` through the existing x87 equity owner, and
`HasPower` is the building's online latch.

## Operational power gate

Hooks `0x4555E4` / `0x45563B` (`Ext/Building/Hooks.cpp:958-983`, 6 bytes;
low-power branch `0x4556BE`, continue branches `0x4555F0` / `0x455643`)
replace the two charger comparisons inside
`BuildingClass::Is_Operational_For_Output`. The `EDI` threshold is the stock
`2`; Ares' `Battery.KeepOnline` hook passes `0`, in which case the stock
continue is kept. Otherwise:

- `keepOnline < 0` is low power;
- every charger's slot-1 weapon warhead adds its `ElectricAssaultLevel` —
  there is no `ElectricAssault` check here, and the list is not pruned at
  this point;
- `overPower < keepOnline` is low power.

VERA carries the two Phobos values in `power_system::OperationalFacts` and
uses them in `is_operational_for_output` for both the offline bypass and the
powered-below-full arm; `sim/world/gap_generator.rs` fills them from the
building's live charger list with `power_charger_level_sum` and
`ObjectType::overpower_keep_online`. `charger_bypass == 0` (Ares
`Battery.KeepOnline`) still keeps the stock continue.

## Defaults preserve stock

Keep 2 and charge 1 with level 1 per charger give the stock behaviour:
three retained chargers, or one at full house power while online, enable the
overpowered weapon, and two chargers bypass the operational power gate.
Without the `phobos` feature the crate keeps those constants and one level
per charger, so the default build is unchanged.

## Residuals

- The AI sample resolves `GetWeapon(1)` through the shared owner
  (`combat_weapon::get_weapon`), including its building-upgrade and garrison
  arms; native reads the object's own weapon array. Trigger: a charger that
  is a structure with an armed upgrade; no retail scenario observed.
- Retail rules do not author the Phobos keys; the regressions use synthetic
  rules and the defaults above.
