# Superweapon Detonate keys

A superweapon can detonate a weapon or warhead at its target cell right after
it fires.

```ini
[SW]                                  ; SuperWeaponType
Detonate.Warhead=                     ; warhead, FindOrAllocate
Detonate.Weapon=                      ; weapon, FindOrAllocate
Detonate.Damage=                      ; Nullable<int>
Detonate.Warhead.Full=yes             ; bool
Detonate.AtFirer=no                   ; bool
```

Reader `Ext/SWType/Body.cpp:274-278`; consumer
`SWTypeExt::ApplyDetonation` (`Ext/SWType/FireSuperWeapon.cpp:267-303`) from
the shared `FireSuperWeaponExt` tail (`:13-40`).

## Behavior

- When either `Detonate.Warhead` or `Detonate.Weapon` is set, the tail runs
  `ApplyDetonation` **before** `SW.Next`.
- The firing building is the first building in the owner's House+68 order
  that provides the superweapon, is alive, in play and powered, and is within
  the type's launch-site range (`IsLaunchSiteEligible(cell, building, false)`).
- `Detonate.AtFirer=yes` detonates at that building's centre; with no eligible
  building the coordinates are `CoordStruct::Empty` (`(0,0,0)`), as native.
- `Detonate.Weapon`: `WeaponTypeExt::DetonateAt` builds a bullet of the
  weapon's projectile and explodes it there with the weapon's warhead;
  `Detonate.Damage` defaults to the weapon's damage.
- Else `Detonate.Warhead`: `Detonate.Warhead.Full=yes` (default) explodes a
  bullet carrying only the warhead; `no` calls the engine `MapClass::DamageArea`
  directly. `Detonate.Damage` defaults to 0.
- An impact off the map is dropped with a log, as native.

## Native anchors

`Detonate.Warhead`/`Detonate.Weapon` are `[SuperWeaponTypes]` keys read in `Ext/SWType/Body.cpp`, and
the detonation they describe runs from `SuperClass::Launch` (`0x6CC390`) at its tail `0x6CDE40`.

## Where VERA20K owns it

`rules::phobos_sw_detonate::PhobosSuperWeaponDetonate` (per-superweapon,
projected by the retained registry), consumed by
`sim::superweapon::detonate`, called from
`sim::superweapon::chaining::apply_phobos_launch_tail` in the native order.
`Detonate.Weapon`/`.Warhead` names are FindOrAllocate: the same pass
constructs them before type bodies (`allocate_sw_detonate_references`).

`SW.RangeMinimum`/`.RangeMaximum` are now read into the Phobos superweapon
metadata too (defaults -1.0/-1.0), and the detonation firer gate uses them in
both builds (`phobos_range_keys_limit_the_detonation_firer`,
`phobos_sw_range_defaults_to_minus_one_and_reads`). No residual.
RESIDUAL: the at-firer coordinate uses the object's `Location` rather than
`GetCenterCoords`'s centre-height point; cell-based area damage is unaffected.
RESIDUAL: a phobos-only build does not read `SuperWeapons=`, so a building that
only that list names is not a firer there (Phobos reads it in
`Ext/BuildingType/Body.cpp:341`; VERA owns it only for Ares).
RESIDUAL: the Ares-handler launch tail calls the detonation with no overlay
registry and default frame effects, so its `MapClass::DamageArea` arm skips
tiberium reduction and the frame's effect routing.
