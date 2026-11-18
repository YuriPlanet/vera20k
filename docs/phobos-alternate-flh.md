# Alternate FLH gating

Two Phobos keys decide **where** the `AlternateFLH*` art offsets are taken
from; VERA already reads the array itself (`src/rules/weapon_offsets.rs`,
`src/sim/combat/fire_coord.rs`).

```ini
[General]
AlternateFLH.OnTurret=no       ; Valueable<bool>, default yes
AlternateFLH.ApplyVehicle=no   ; Valueable<bool>, default no

[TECHNO]                       ; Art section
AlternateFLH.OnTurret=         ; Nullable<bool>, [General] fallback
AlternateFLH.ApplyVehicle=     ; Nullable<bool>, [General] fallback
```

## Behavior

`GetFLH` is called with a negative weapon index (`-k`) for an alternate FLH.
Phobos's consolidated hook (`Ext/Techno/Hooks.Firing.cpp:1031-1088`, site
`0x6F3AEB`) takes `AlternateFLHs[-weaponIndex-1]` and then asks
`AlternateFLH.OnTurret` (the ART `Nullable<bool>`, else `[General]`, default
yes, `:1066-1070`) for `allowOnTurret`. `GetFLHAbsoluteCoords` then applies the
turret matrix only when `allowOnTurret && (pType->Turret || !isFoot)`
(`Ext/Techno/Body.Internal.cpp:48-66`). False — or a Foot type without a
turret — keeps that FLH hull-relative: the `TurretOffset` and the turret's
rotation relative to the body are skipped, so the body facing and the
unrotated offset place the shot.

Phobos also installs the UnitClass `GetFLH` override
(`Ext/Techno/Hooks.Firing.cpp:1004-1028`, vtable `0x7F5D20`). A **Unit**
passenger riding an open-topped transport takes the carrier's FLH instead of
its own: with `k` its cargo index from the head, the shot leaves from
`carrier.GetFLH(-k)` — the carrier's `AlternateFLH[k-1]`. The gate is the
**carrier type's** ART `AlternateFLH.ApplyVehicle` (else `[General]`, default
no, `:1012`); the passenger's own value is never read. When the key is off, the
Unit keeps its own weapon FLH. An **Infantry** rider is unaffected: its stock
`GetFLH @ 0x00523250` always uses the carrier port, and only the
`OnTurret` space selection applies.

## Where VERA20K owns it

`rules::weapon_offsets::WeaponOffsets` reads both ART keys in its per-pass
`read_pass`, keeps the five stock `AlternateFLH0..4` ports plus Phobos's sixth
`AlternateFLH5`, and exposes the keys through `ObjectType`;
`rules::phobos_globals` reads the two `[General]` fallbacks.
`sim::combat::fire_coord` is the only selection owner: its negative-index arm
and `open_topped_port_coordinate` use the transport's/type's keys to choose
hull or turret space, and the port path adds the Unit passenger arm behind the
carrier's `ApplyVehicle`.

## Scope note

`ApplyVehicle` is modeled for Unit passengers only, matching the UnitClass
vtable override; Aircraft passengers keep their own FLH as in the unhooked
game. The regular (`weaponIndex >= 0`) weapon FLH is gated by the same
`pType->Turret` check as the alternate arms, because Phobos's consolidated
hook routes every arm through `GetFLHAbsoluteCoords`.

## Residuals

- `UnitTypeExt::BurstPerTurret` (`Ext/UnitType/Body.cpp`) supplies the turret
  index `GetFLHAbsoluteCoords` passes to `ApplyTurretOffset`:
  `turIdx = ((CurrentBurstIndex / BurstPerTurret) % (BurstPerTurret + 1)) - 1`
  for a Unit whose key is > 0. This is ported on the firing path:
  `sim::combat::fire_coord::phobos_turret_pivot_offset` (the read lives in
  `rules/phobos_turret_barrel.rs`, gated on a positive `ExtraTurretCount`).
  A `BurstPerTurret` larger than the offset vector keeps the stock pivot rather
  than reading past it. Retail sets no `BurstPerTurret`.
- The carrier-port arm uses the rider's cargo position for
  `AlternateFLH[k-1]`; a rider beyond the carrier's sixth port gets a zero FLH
  in both native and VERA. The port's burst-parity mirror is skipped under
  Phobos (`Hooks.Firing.cpp:1062-1071`), as in the consolidated hook.
