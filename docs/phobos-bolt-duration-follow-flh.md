# Bolt.Duration / Bolt.FollowFLH

An electric bolt normally lives a fixed 17-exponent lifetime and only a Unit's
firing bolt follows its firer. Phobos exposes both per weapon.

```ini
[COILWEAPON]
Bolt.Duration=17       ; Valueable<int>, default 17
Bolt.FollowFLH=no      ; Nullable<bool>, unset follows only a Unit firer
```

Readers: the two keys at `Ext/WeaponType/Body.cpp:101-102` (constructors
`Ext/WeaponType/Body.h:36-37,131-132`).

## Native body

- `EBoltExt::CreateEBolt` (`Ext/EBolt/Body.cpp:28-31`) stores
  `Lifetime = 1 << (clamp(Bolt.Duration, 1, 31) - 1)`.
- `EBolt::SetOwner` is replaced by `EBoltExt::_SetOwner`
  (`Ext/EBolt/Hooks.cpp:104-121`, jump at `0x4C2BD0`; the
  `TechnoClass_InitEBolt` hook at `0x6FD5D6` calls it): a bolt is linked to its
  firer only when `Bolt.FollowFLH.Get(firer is a Unit)`; the hook skips the
  native SetOwner InLimbo check (`0x4C2BF2`) and the Unit `+0x6DC` slot test
  (`0x006FD5F2`..`0x006FD600`), so every linked bolt follows and enters
  `TechnoExt::ElectricBolts`. The one-bolt `+0x6DC` slot is stock-only.
- The tactical pass (`EBoltMgr::UpdateAndDrawAll @ 0x004C2830`) reads the
  owner's FLH, restoring the spawn-time `CurrentBurstIndex`
  (`EBoltExt::BurstIndex`, hook `0x4C285D`).

## VERA

`WeaponType::phobos_bolt_duration` and `.phobos_bolt_follow_flh` read the
keys. `electric_bolt::emit` stamps `ElectricBoltBirth::decay` with the
exponent formula (0x10000 for the stock 17) and the app registry seeds each
bolt's decay from it instead of the fixed constant. `fired` links the owner
when the key set, else for a Unit firer, and marks a Unit owner's
`ElectricBoltOwner::unit_slot`. Only the stock build applies the one-bolt
`+0x6DC` slot rule in the app registry; under Phobos every linked bolt keeps
its owner, so a non-Unit `FollowFLH=yes` owner and a Unit both follow with
several bolts.

## Tests

`src/sim/combat/electric_bolt_bolt_keys_tests.rs`:
`bolt_duration_sets_the_lifetime_exponent` (17 -> 0x10000, 3 -> 4, 1/0 -> 1,
31/40 -> 1<<30) and
`bolt_follow_flh_overrides_the_unit_only_owner_link` (Unit default, Unit no,
non-Unit default no, non-Unit yes with no Unit slot) and
`phobos_links_every_unit_bolt_and_a_limbo_unit_still_links` (the hook's slot
and InLimbo bypass). Both the decay wiring and the follow gate were mutated
red and restored. The app-side `electric_bolt_lifetime_*` tests pin the
composite decay and that `create` seeds it from the birth.

## Residuals

- `TechnoExt::ElectricBolts` is not persisted as a per-owner vector; the app
  registry finds owners by scanning live bolts. Every linked bolt follows in
  place, but the vector itself is not saved.
- Phobos applies `Bolt.Duration` where it intercepts EBolt creation (the
  Ares CreateBolt patch and the bullet path); a Phobos build without those
  patches keeps the stock 0x10000 for an ordinary `FireAt` bolt. VERA applies
  the key to every electric bolt, one uniform rule.
- A following bolt's FLH read uses the firer's current burst index, not the
  spawn-time `EBoltExt::BurstIndex` (hook `0x4C285D`); the two differ only
  when the firer's burst advanced between the shot and the draw.
- Only `FireAt`'s `SpawnElectricBoltEffect` path links an owner; shrapnel and
  split-child `CreateElectricBolt6FD460` bolts never run the
  `TechnoClass_InitEBolt` hook natively either.
