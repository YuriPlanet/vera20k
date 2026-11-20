# Warhead `Ammo`

`[WarheadType] Ammo=N` adds `N` to every hit techno's current ammo and clamps
the result into `0..type->Ammo`. A plain `Valueable<int>` read by
`Ext/WarheadType/Body.cpp:452`, default `0`.

```ini
[WARHEAD]
Ammo=-1      ; drain one round; negatives clamp to 0
Ammo=2       ; add two, clamped to the target type's Ammo
```

## Native evidence

`Ext/WarheadType/Detonate.cpp:221-222` runs `ApplyAmmoModifier(pTarget)` in
`DetonateOnOneUnit`'s one-time effects, after the displacement arms and before
`RemoveDisguise`/`ReverseEngineer`/`ReturnWarhead`. The body (`:923-932`):

```
const int maxAmmo = pTarget->GetTechnoType()->Ammo;
int newCurrentAmmo = this->Ammo + pTarget->Ammo;
newCurrentAmmo = newCurrentAmmo < 0 ? 0 : newCurrentAmmo;
pTarget->Ammo = newCurrentAmmo > maxAmmo ? maxAmmo : newCurrentAmmo;
```

A type with `Ammo=0` (no ammo capacity) is therefore clamped back to 0.

## Where VERA20K owns it

- `rules::warhead_type::WarheadType::ammo`.
- `sim::ammo::TechnoAmmo::apply_warhead_delta` implements the native clamp
  against the target type's live `Ammo=`.
- `sim::combat::crit::detonate`'s per-target loop applies it before the
  disguise/return-warhead arms (`sim/combat/crit.rs:265`).

## Native anchors

`ApplyAmmoModifier` runs inside `DetonateOnOneUnit`'s one-time effects (`Ext/WarheadType/Detonate.cpp:221..222`,
body `:923..932`), so the warhead body Phobos hooks at `BulletClass_Detonate` (`0x46920B`, `Ext/WarheadType/Hooks.cpp:8`) is the address for the key.

## Residuals

- The clamp uses the target type's current `Ammo=` from the rules, matching
  native `GetTechnoType()->Ammo`; the per-techno constructor copy is not used.
- Retail/visual parity is untested here (no retail `ini/` in this checkout).
