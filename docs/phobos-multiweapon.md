# Multi-weapon selection

Phobos lets a TechnoType with more than two weapons choose which slots its
`WhatWeaponShouldIUse` ladder cycles between, and which of them read as
"secondary" (`Ext/TechnoType/Body.h:387-390`, reader
`Ext/TechnoType/Hooks.MultiWeapon.cpp:3-80`, hook `0x7128B2`; selector
`Ext/TechnoType/Body.cpp:175-291` at `0x6F3428`).

```ini
[TECHNO]                       ; TechnoType
MultiWeapon=no                 ; Valueable<bool>, default no
MultiWeapon.SelectCount=       ; Valueable<int>, default 2, read only while on
MultiWeapon.IsSecondary=       ; ValueableVector<int> of slot indices
```

## Reading

`MultiWeapon` is read on every reached TechnoType pass with the `Valueable`
reader, so an absent key keeps the previous value. The switch also computes the
`ReadMultiWeapon` latch, `TurretCount > 0 || MultiWeapon`. When the latch
changes, base and elite slots 0/1 are cleared before the weapon list is read
(`Hooks.MultiWeapon.cpp:14-31`). The numbered `WeaponN` read runs while the
latch is on (`:65`; second gate `0x715B10`), so `MultiWeapon=yes` reaches
`WeaponN` with `TurretCount=0` and `MultiWeapon=no` keeps the stock
`Primary`/`Secondary` slots.

While the switch is on, `MultiWeapon.SelectCount` is read and, when
`WeaponCount > 0`, a fresh `MultiWeapon.IsSecondary` list is parsed with the
Phobos integer-vector reader. A non-empty list is `resize(WeaponCount, false)`
(existing prefix retained) and each in-range index sets its slot; negative and
out-of-range indices are dropped. Every other case leaves the retained list
alone, and the switch off clears it (`:35-63`).

`IsSecondary(slot)` (`Body.cpp:166-173`) answers the list only when the switch
is on and the list is non-empty, else the stock `slot != 0`; a gattling type
keeps its odd non-zero slot rule.

## Selection

`SelectMultiWeapon` runs at the `0x6F3428` hook after `SelectForceWeapon` and
after every native ladder arm that returns earlier (class deploy overrides,
occupied building, weapon resolution, `NeverUse`, null target, open-topped
slot, Ares `NoAmmoWeapon`, gattling stage). The turreted non-gattling slot arm
(`0x6F333B`) returns earlier **unless** the type is `MultiWeapon` and not a
Unit with `Gunner`: the `0x6F3360` hook then skips that return
(`Ext/Techno/Hooks.Firing.cpp:41-53`) and the ladder continues. It needs the
switch and a target and refuses gattling types and
`HasMultipleTurrets() && Gunner` types.

`count = min(WeaponCount, SelectCount)`; below 2 it forces slot 0, exactly 2
falls through (unless `NoSecondaryWeaponFallback`), and otherwise it walks
slot 1..count (or 0..count when the `IsSecondary` list is non-empty, skipping
false entries). A slot whose weapon is missing/`NeverUse` is marked
"cannot target". A slot gets secondary priority when it is the naval choice or
its warhead is `Airstrike`, the target is airborne, it is a `DrainWeapon`
against a drainable non-ally while not draining, or its warhead is
`ElectricAssault` against an allied `Overpowerable` building. A priority slot
that `MultiWeaponCanFire` admits is answered; otherwise it is marked and the
final walk answers the first slot that can fire, else 0.

`MultiWeaponCanFire` (`Ext/Techno/WeaponHelpers.cpp:360-503`) rejects a
missing/`NeverUse` weapon, an open-topped transport without
`FireInTransport`, a non-AA projectile against an airborne target, an
`AAOnly` projectile against a ground target, a misplaced `ElectricAssault` or
`IsLocomotor`, then asks the shared weapon-picking eligibility, the
bomb/temporal/parasite/mind-control/drain gates and `AllowAirstrike`. That
eligibility arm omits `LegalTarget` and `CanTargetVeterancy`, which the shrapnel
caller reads but this body does not.

`NoSecondaryWeaponFallback` and `.AllowAA` (`Ext/TechnoType/Body.cpp:186,223`)
are independent type keys: the fallback keeps `count == 2` inside
`SelectMultiWeapon` and drops the airstrike/drain/electric secondary priorities
unless `AllowAA` re-enables the airborne one.

At the stock ladder's Q arm (`0x6F36DB`, `Ext/Techno/Hooks.Firing.cpp:121-165`)
Phobos asks `TechnoExt::PickWeaponIndex` for slots 0/1 before native's Verses
checks at `0x6F36E3` (`Ext/Techno/WeaponHelpers.cpp:7-100`). The pick answers
when the secondary is targetable and the primary is not, or the reverse:
`SkipWeaponPicking` bypasses a slot's group, otherwise the cell `CanTarget`
(airborne targets skip the cell), techno `CanTarget`, `CanTargetHouses`, the
health and veterancy ranges and required AttachEffects decide. `allowFallback`
is `!NoSecondaryWeaponFallback`; with the key on, only `secondIsAA` (airborne
target, AA projectile) or `.AllowAA` keeps the airborne-secondary fallback,
and a `CanFireNoAmmoWeapon` secondary also blocks the primary answer. A cell
target on land also picks the secondary for `NavalTargeting=1` or
`LandTargeting=2`.

The gattling hook at `0x6F3432` (`Ext/Techno/Hooks.Firing.cpp:204-283`) reuses
that same pick owner over `odd = 2 * CurrentGattlingStage` and `even = odd + 1`
with **both** fallback gates hard-wired true, not the type keys. When the pick
answers `-1`, its `isWeaponValid` lambda walks, in order, airborne without an
AA projectile, underground without `AU`, an active target shield whose
`CanBeTargeted` refuses the weapon, and Verses exactly zero against the
target's own armor; the map's water/beach cell then answers through
`SelectNavalTargeting`, and `LandTargeting=2` answers `even` after that.

## Voice attack

`TechnoTypeExt::GetVoiceAttack` (`Ext/TechnoType/Hooks.MultiWeapon.cpp:82-116`,
hook `0x7090A0`) runs before the stock/Ares list and answers, in order: a
repairing weapon's `VoiceIFVRepair` (then the literal `FV` global), the
selected slot's `VoiceWeapon%dAttack` / `VoiceEliteWeapon%dAttack`
(`Ext/TechnoType/Body.cpp:421-462`), then the primary/secondary slot voice
chosen by `IsSecondary(slot)`.

## Where VERA20K owns it

`rules::phobos_multi_weapon` holds the fields and the per-pass reader,
including the voice keys bound against the fixed sound catalog;
`native_processing::WeaponSlots` retains them with the slot arrays and the
latch, and `ruleset` projects the result onto `ObjectType::phobos_multi_weapon`.
`sim::combat::combat_weapon::select_weapon_index_against_in_query` computes the
slot lazily at the `0x6F3428` position via `phobos_multi_weapon_index`, and
`phobos_pick_weapon_index` runs the `0x6F36DB` pick from the same owner (it
reuses `world_fire::phobos_skip_weapon_picking`, `crit::cell_eligible` /
`techno_eligible`, `can_target_house`, `health_in_threshold`,
`affected_veterancy_allows` and `weapon_attach_effects_allow`); the same pick
owner serves the gattling pair through `phobos_gattling_weapon_index`, whose
`phobos_weapon_valid_against` is the `isWeaponValid` lambda;
`multi_weapon_can_fire` reuses `phobos_airburst::weapon_picking_allows_with`
for the target-eligibility and warhead-versus gates, and reads the fired-at
cell's overlay through `resolved_terrain.overlay_passability`, whose
`wall`/`armor_is_wood` bits are the `Wall=`/`Armor=` bindings
`recalc_overlay_passability` already carries, for the `Wall=`/`Wood=` gate
(`Ext/Techno/WeaponHelpers.cpp:486-495`).
`sim::world::selection_voice::attack_voice_request` owns the Phobos voice arm.
The regression tests are
`phobos_stock_weapon_pick_honours_can_target_and_the_fallback_keys` (pick
answers the secondary, `NoSecondaryWeaponFallback` suppresses it, `.AllowAA`
keeps the airborne arm) and
`phobos_no_secondary_weapon_fallback_disables_the_drain_priority` (the
SelectMultiWeapon arm).

## Residuals

- `IsSecondary` consumers in the Infantry/Unit firing-frame hooks
  (`Ext/Infantry/Hooks.Firing.cpp:45`, `Ext/Unit/Hooks.Firing.cpp:42`) are
  presentation; VERA's firing frames do not read the predicate yet.
- `NoSecondaryWeaponFallback` is ported in both owners: `SelectMultiWeapon`
  (`0x6F3428`) and the stock `PickWeaponIndex` pick (`0x6F36DB`).
- The shield arm following the `0x6F36DB` pick (`Ext/Techno/Hooks.Firing.cpp:
  140-162`) is ported in `phobos_shield_weapon_index` on the same owner:
  when the pick answers `-1` and the target Techno's shield is active, a
  secondary the fallback/AA/underground/no-ammo gates admit takes the shot
  once `ShieldClass::CanBeTargeted` refuses the primary
  (`sim::shield::can_be_targeted`), and an unusable secondary answers the
  primary. Retail rules author no `ShieldType`, so only mod data reaches it.
- `SkipWeaponPicking` needs no separate key here: VERA applies the picking
  gates unconditionally and every gate passes when Phobos derives the flag as
  true.
- Airborne target cell gate: native `MultiWeaponCanFire` leaves `pTargetCell`
  null for an airborne target and therefore skips `IsCellEligible`; VERA's
  shared weapon-picking owner (`sim::combat::phobos_airburst::
  weapon_picking_allows_with`) always applies the target cell's `CanTarget`
  land/water bits. Trigger: a multi-weapon type whose slot weapon authors ART
  `CanTarget=` and the target is airborne; effect: a land/water-only `CanTarget`
  can reject a slot native admits. No retail weapon authors `CanTarget=`.
- Other `What_Weapon_Should_I_Use` callers (threat evaluation, temporal,
  GetFireError slot queries) keep the stock/force arms like `ForceWeapon`;
  only the SelectWeapon owner passes the multi context.
