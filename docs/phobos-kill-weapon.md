# Kill weapons on a fatal hit

Phobos warheads and techno types can fire extra weapons when a hit kills
(`Ext/Techno/WeaponHelpers.cpp:230-252`, hooked at
`TechnoClass::ReceiveDamage` `0x702672`).

```ini
[HIT]                            ; WarheadType
KillWeapon=                      ; weapon, fired at the dead object
KillWeapon.OnFirer=              ; weapon, fired at the killing source
KillWeapon.AffectsHouse=all      ; owner/self, allies/ally, enemies/enemy, neutral, team, others, all, none
KillWeapon.OnFirer.AffectsHouse=all
KillWeapon.AffectsTarget=all     ; land, water, empty, infantry, units, buildings, aircraft, all, none
KillWeapon.OnFirer.AffectsTarget=all

[TARGET]                         ; TechnoType of the victim
SuppressKillWeapons=false        ; bool, default false
SuppressKillWeapons.Types=       ; weapon list, default empty (no filter)
```

`KillWeapon.AffectsHouses`/`KillWeapon.Affects` and their `.OnFirer`
counterparts are deprecated aliases read before the canonical keys.

## Behavior

`ApplyKillWeapon` runs for the hit that killed the object, after the death
weapon and before the AttachEffect revenge weapons. `KillWeapon=` detonates on
the dead object; the shot's owner is the killing source or, without one, no
owner, while its firing house falls back to the killing hit's source house.
`KillWeapon.OnFirer=` detonates on the killing source and only exists when that
source does. Each shot is dropped when its `AffectsHouse` refuses the source
house against the victim's owner, when the victim's category is not in
`AffectsTarget`, or when the victim type's `SuppressKillWeapons` covers the
weapon. A suppressed field with a non-empty `SuppressKillWeapons.Types` list
still fires a weapon the list does not name.

Both shots go through the ordinary bullet receiver, so a ground detonation on
the shooter itself (an `OnFirer` shot) needs the native self-damage admission
(the type's `DamageSelf=` or the warhead's `AllowDamageOnSelf=`), exactly like
every other area-damage source.

## Where VERA20K owns it

`rules::phobos_kill_weapon` reads the warhead and techno-type keys into
`WarheadType`/`ObjectType`. `Simulation::apply_phobos_kill_weapons` in
`sim::kill_weapon` builds the two detonations and commits them through the
shared receiver; the call sits at the `0x702672` site in
`sim::combat::world_receiver`, after the death weapon and before the
AttachEffect revenge weapons. House and target eligibility reuse the shared
`combat::can_target_house` and `crit::techno_eligible` owners (the latter now
also treats a 1x1-with-undeploy building as a unit, as `IsTechnoEligible`
does).

## Residuals

- VERA's weapon registry is built from the weapons reached by type readers.
  A `KillWeapon=`/`.OnFirer=` naming a weapon that no other key or type slot
  references is not found, while gamemd's `[Weapons]` array would hold it.
- The scalar `KillWeapon=`/`.OnFirer=` reads resolve the type at the merged
  section like every other `Valueable<WeaponTypeClass*>` field here; an
  unknown later-layer name replaces an earlier valid one where Phobos keeps
  it.
