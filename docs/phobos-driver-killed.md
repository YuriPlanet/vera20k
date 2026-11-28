# Driver-killed passengers (`DriverKilled.KeptPassengers`, `DriverKilled.KillPassengers`)

Phobos lets a TechnoType decide what happens to the remaining passengers of a
vehicle whose driver is killed by an Ares `KillDriver` takeover: keep them
aboard, kill them, or leave the stock random ejection. Two type keys
(`Ext/TechnoType/Body.cpp:969-970`, `Nullable<bool>` fields
`Ext/TechnoType/Body.h:212-213`, constructor unset `:641-642`) with two
`[CombatDamage]` globals (`Ext/Rules/Body.cpp:533-534`, `Valueable<bool>`
fields `Ext/Rules/Body.h:949-950`, both construct `false`).

```ini
[SOMETECHNO]                                    ; TechnoType
DriverKilled.KeptPassengers=                    ; Nullable<bool>, unset -> [CombatDamage]
DriverKilled.KillPassengers=                    ; Nullable<bool>, unset -> [CombatDamage]

[CombatDamage]
DriverKilled.KeptPassengers=false               ; Valueable<bool>, default false
DriverKilled.KillPassengers=false               ; Valueable<bool>, default false
```

## Native behavior

`TechnoExt::ApplyKillDriver` (`Ext/Techno/Body.cpp:930-1017`) is the Phobos
body installed over the Ares driver-change tail at `100458F3..10045953`
(`TechnoExt::KillDriver` `100456D0`). After `passive = pToHouse->IsNeutral()`
and the `DriverKilled` byte, and after the operator passengers are removed
(`Operator_Any` / the `Operators` list):

- `if (passive && KeptPassengers.Get(global)) break;` — every passenger stays
  aboard the vehicle; no ejection, no destruction, no `ExitedOpenTopped`.
  The `Get` returns the type value when set, else the global.
- Otherwise `const bool kill = KillPassengers.Get(global);` and the passenger
  loop runs: each passenger leaves the cargo (`RemoveGunner` when a
  `Gunner=` vehicle's last passenger leaves), then
  `kill || !EjectRandomly(passenger, Location, 128, false)` records the
  destruction (`RegisterDestruction(nullptr)`) and `UnInit()`s it — the
  short-circuit skips the ejection attempt entirely, including its placement
  draws — while a successful ejection from an `OpenTopped=` vehicle also
  calls `ExitedOpenTopped`.

`KeptPassengers` only ever applies to a `passive` takeover (the new owner's
country is `MultiplayPassive=yes`); with a non-passive takeover the stock
loop runs regardless of the flag.

## Where VERA20K owns it

- `rules::phobos_driver_killed::PhobosDriverKilled` holds the two type keys
  on `ObjectType::phobos_driver_killed`, read per type section like
  `Promote.*`. The globals live in `rules::phobos_globals::PhobosGlobals`
  (`driver_killed_kept_passengers`, `driver_killed_kill_passengers`), read
  from the merged rules layers' `[CombatDamage]` once per pass.
- `sim::vehicle_theft::driver_kill::ares_replace_driver` (the ported
  `100456D0` driver change) reads both effective values after its operator
  removal, in the native order. The kept arm skips the passenger pass; the
  kill arm turns `depart_cargo_head`'s survivor closure into
  `record_kill_and_uninit(id, None, context)` — the existing
  `RegisterDestruction(nullptr)`/`UnInit()` owner — without calling
  `place_ares_survivor`, the shared `EjectRandomly` owner. Successful
  placement keeps owning the `ExitedOpenTopped` clearing, and
  `depart_cargo_head` keeps owning the cargo unload and the `Gunner=`
  empty-hold reset, so this chain adds no competing state.

## Phobos quirks kept

- A type `no` overrides a global `yes`: `Nullable<T>::Get` returns the set
  type value, the globals have no unset state and no clearing token.
- With `KeptPassengers=yes` the operator passengers are still removed and
  destroyed first — the break happens after the operator removal, not
  before it.
- With `KillPassengers=yes` no ejection is attempted for any remaining
  passenger, even where the placement would succeed, so the placement and
  scatter draws of `EjectRandomly` do not happen.

## Native anchors

Ares reimplements the native driver-change body (`100458F3..10045953`), and Phobos replaces that
reimplementation — `Patch::Apply_LJMP(AresBase + 0x456D0)` and `(+0x46240)` (`Misc/Hooks.Ares.cpp:296,420`).
The two keys are Ares-side, so no gamemd instruction reads them.

## Residuals

- **RNG/timer/detach ledger.** The chain passes the one Scenario draw of
  `KillDriver.Chance` (`10052BF8..10052C2F`, already owned by
  `ares_apply_kill_driver`) and adds no new draws, timer writes or detach
  calls: with `KillPassengers=yes` the placement draws are skipped exactly
  as the native short-circuit skips them, and the kept arm draws nothing.
- **Retail INIs.** Neither key exists in retail rules; with the feature
  compiled but the keys unset, the stock ejection stays byte-identical.
- Retail/Phobos side-by-side play is untested in this checkout (no retail
  `ini/` present).
