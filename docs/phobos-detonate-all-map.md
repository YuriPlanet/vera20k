# Detonate on all map objects (`DetonateOnAllMapObjects.*`)

A warhead can replace its bullet's ordinary impact with one pass over every
live Techno on the map. Seven keys (`Ext/WarheadType/Body.cpp:304-319`,
fields `Body.h:153-159`, constructor `Body.h:432-438`); two pairs are
deprecated aliases read first so the real key wins.

```ini
[WARHEAD]
DetonateOnAllMapObjects=false            ; boolean
DetonateOnAllMapObjects.Full=true        ; boolean, full detonation per target
DetonateOnAllMapObjects.RequireVerses=false ; boolean, skip 0% Verses targets
DetonateOnAllMapObjects.AffectsTarget=None  ; AffectedTarget flags
DetonateOnAllMapObjects.AffectsHouse=None   ; AffectedHouse flags
DetonateOnAllMapObjects.AffectTypes=     ; ValueableVector<TechnoTypeClass*>
DetonateOnAllMapObjects.IgnoreTypes=     ; ValueableVector<TechnoTypeClass*>
; deprecated spellings (read first, overwritten by the real keys):
; DetonateOnAllMapObjects.AffectTargets= / .AffectHouses=
```

## Native behavior

`BulletClass_Logics_DetonateOnAllMapObjects @ 0x4690C1`
(`Ext/Bullet/Hooks.DetonateLogics.cpp:78-168`), at the head of
`BulletClass::DetonateAtCoord @ 0x004690B0`, runs when the warhead is
enabled, has not fired this detonation, and both `AffectsTarget` and
`AffectsHouse` are non-`None`. It then:

1. Snapshots each class array it affects — aircraft, buildings, infantry,
   units, in that order — before any target can die.
2. For each Techno that `EligibleForFullMapDetonation`
   (`Ext/WarheadType/Body.cpp:110-134`) accepts (on map, alive, not limbo,
   not sinking; house filter `CanTargetHouse`; `AffectTypes` include /
   `IgnoreTypes` exclude; `RequireVerses` skips a 0% Verses row):
   - `Full=yes`: re-point the bullet at the target and call
     `BulletClass::Detonate`, a complete detonation at the target's coords.
   - `Full=no`: `DamageAreaWithTarget(target coords,
     (bullet Health * DamageMultiplier) >> 8, owner, warhead, true,
     firing house, target)`.
3. Restore the bullet's target and position, clear the guard, and return from
   `DetonateAtCoord` — no radiation, special arm, shrapnel, area damage or
   anim tail for the original impact.

## Where VERA20K owns it

- `rules::phobos_detonate_all_map::DetonateOnAllMapObjectsRules` holds the
  keys; `WarheadType::detonate_on_all_map` carries them.
- `sim::combat::world_receiver::detonate_on_all_map_objects` runs at the head
  of `emit_detonation_receivers` (the `DetonateAtCoord` port): it snapshots
  each class store immediately before that class's walk (so crew spawned by
  the building walk is visited by later walks), applies the eligibility owner
  and calls the existing nested owners (`commit_projectile_detonations_inline`
  with `force_single_detonation` for `Full`, so one DetonateAtCoord with no
  Explode cluster draws; `apply_area_damage_in_run_with_target` otherwise).
  The nested detonations commit their own receivers synchronously; their
  appended receiver records are dropped so the outer area pass does not
  commit them twice. The `Simulation::phobos_all_map_detonation_active`
  `Option<InternedId>` is the per-warhead `WasDetonatedOnAllMapObjects`
  guard.
- The claimed detonation skips the caller's shake draws, anim/debris tail
  (`all_map_claimed` in `commit_projectile_detonations_inline`) but keeps the
  outer cluster successor draws, as native's return to `0x46A2FB` does.
- `BulletClass +0x10C` re-pointing goes through `ProjectileStore::set_target`,
  the same owner the Ares firestorm wall uses.

## Residuals (trigger · impact · frequency · risk)

- **Array order vs id order**: native walks each class's `DynamicVectorClass`
  (construction order); VERA snapshots each category from the entity store in
  ascending stable id immediately before its walk. These coincide for
  ordinary construction; a removed and reinserted object could order
  differently. Trigger: mid-game object churn. Impact: RNG-free damage order
  only. Frequency: rare. Risk: low.
- **`AffectsTarget` bits beyond the four class flags** (`Land`, `Water`,
  `NoContent`) are parsed but unused: the hook itself only walks Technos, but
  a land-only token set still claims the detonation and hits nothing (tested).
- **Deprecated-key debug log** is not emitted (Phobos logs a developer
  warning when both spellings exist). Trigger: both keys authored. Impact:
  log only. Frequency: broken mod data. Risk: none.
- **`AffectTypes`/`IgnoreTypes` names** are filtered through `RuleSet::object`
  at use time, reproducing the native read-time drop (an all-invalid authored
  list means "no type filter", as natively). A name that resolves to a
  non-Techno type would still be compared by name; no retail list does that.
- **`RequireVerses`** compares the truncated `GetTotalDamage(100)/100`
  against zero, matching a Verses rate below 1%; exact rounding of the
  warhead-versus-armor pipeline for rates with fractional percentages follows
  `shield::warhead_versus_armor`.
- **No retail/visual validation** in this checkout.
