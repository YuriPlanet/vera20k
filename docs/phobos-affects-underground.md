# AffectsUnderground

```ini
[SOMEWARHEAD]
AffectsUnderground=no        ; Valueable<bool>, default false
```

Reader `Ext/WarheadType/Body.cpp:425`, default `Ext/WarheadType/Body.h:555-557`.

`MapClass_DamageArea_DamageUnderGround @ 0x4899DA`
(`Ext/Bullet/Hooks.DetonateLogics.cpp:933-975`) runs inside
`MapClass::DamageArea` before the collected surface receivers dispatch: with
the key on and the impact not nullified, every techno in the Underground
tracker is damaged directly (`ReceiveDamage`) when it is alive, not Iron
Curtained, not on the map and not in limbo, and its distance to the impact is
within `CellSpread * 256` leptons (2D when `CellSpread.Cylinder`, else 3D),
passing `(int)distance` as the ReceiveDamage distance and the firing techno
and house as the source.

VERA: `WarheadType::phobos_anim.affects_underground`;
`sim::combat::world_receiver::apply_underground_area_damage` runs from
`apply_area_damage_in_run_with_target` before `collect_area_at`, scans the
Underground-layer entities, applies the same filters and commits each through
the direct-receiver path. A hit with no surface receiver reports
`ReceiverDispatched`, like the native `hitted` stack flag.

`CellSpread.Cylinder` is read as `WarheadType::phobos_cell_spread_cylinder`
(see `docs/phobos-extra-warheads.md`) and the underground pass picks the 2D
distance when it is set, matching the hook's `getCellSpreadItemsExt` arm; no
residual.
