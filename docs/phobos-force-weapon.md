# Forced weapon selection

Phobos can force the weapon a Techno selects against a target, overriding the
native `WhatWeaponShouldIUse` ladder (`Ext/TechnoType/Body.cpp:60-155`,
reader `:905-936`, hook `0x6F3428` in `Ext/Techno/Hooks.Firing.cpp:94-118`).

```ini
[TECHNO]                              ; TechnoType
ForceWeapon.Naval.Decloaked=-1        ; slot, default -1 (unset)
ForceWeapon.Cloaked=-1
ForceWeapon.Disguised=-1
ForceWeapon.UnderEMP=-1
ForceWeapon.InRange=                  ; ordered slots, default empty
ForceWeapon.InRange.Overrides=        ; ranges in cells, default empty
ForceWeapon.InRange.TechnoOnly=       ; Nullable<bool>, default [General] yes
ForceWeapon.InRange.ApplyRangeModifiers= ; Nullable<bool>, default [General] no
ForceWeapon.Buildings=-1
ForceWeapon.Defenses=-1
ForceWeapon.Infantry=-1
ForceWeapon.Naval.Units=-1
ForceWeapon.Units=-1
ForceWeapon.Aircraft=-1
ForceAAWeapon.InRange=                ; air-target list
ForceAAWeapon.InRange.Overrides=
ForceAAWeapon.InRange.ApplyRangeModifiers= ; default [General] no
ForceAAWeapon.Infantry=-1             ; airborne Infantry targets
ForceAAWeapon.Units=-1                ; airborne Unit targets
ForceAAWeapon.Aircraft=-1             ; airborne Aircraft targets

[General]                             ; global fallbacks
ForceWeapon.InRange.TechnoOnly=yes
ForceWeapon.InRange.ApplyRangeModifiers=no
ForceAAWeapon.InRange.ApplyRangeModifiers=no
```

## Behavior

The forced slot is answered at the `0x6F3428` hook point inside the native
`WhatWeaponShouldIUse` ladder: the class overrides (Infantry deploy-fire and
Unit deployed deploy-fire), the turreted non-gattling slot, an occupied
building, the two weapon-resolution arms, `NeverUse`, a null target or force
fire, the open-topped slot, Ares `NoAmmoWeapon` and the gattling stage all
return before it, exactly as natively. A live Techno target is tested in
order: `Naval.Decloaked` (Cloakable, Naval, Uncloaked), `Cloaked`,
`Disguised`, `UnderEMP`. Otherwise `InRange`/`ForceAAWeapon.InRange` walk their
slots in order; a slot inside its range (an override in cells, else the
weapon's own `Range=`, optionally with attach-effect range modifiers) is
forced, and a `-1` entry inside its range stops the walk without forcing.
Finally the class arms answer: a `BuildCat=Combat` building takes
`Defenses` before `Buildings`; an Infantry target takes `ForceAAWeapon.Infantry`
when airborne, else `Infantry`; a Unit takes `ForceAAWeapon.Units` when
airborne, else `Naval.Units` for a naval target, else `Units`; an Aircraft
takes `ForceAAWeapon.Aircraft` when airborne, else `Aircraft`. With no armed
field the type skips the check entirely.

In-range uses `ObjectClass::Distance_To` and the native `GetWeapon` of each
slot; the in-range gate needs a Techno target when `TechnoOnly` is yes.

## Where VERA20K owns it

`rules::phobos_force_weapon` reads the type fields (int list reader for slots,
Phobos `ValueableVector<double>` reader for the overrides, Phobos
`Nullable<bool>` for the modifiers) and `phobos_globals` reads the three
`[General]` fallbacks. The one selection owner
`sim::combat::combat_weapon::select_weapon_index_against_in_query` computes
the forced slot and hands it to `what_weapon_should_i_use_with_force`, which
applies it at the `0x6F3428` position. Range modifiers reuse
`attach_effect::range_with_attach_effects`, distance reuses
`object_distance_to`.

## Residuals

- `MultiWeapon.*` (the next arm at the same hook site) is a separate chain.
- Four SelectWeapon consumers still call the raw ladder without a force
  context: `greatest_threat.rs` acquisition and threat scoring and
  `combat_targeting.rs` retaliation. Trigger: a type with any ForceWeapon arm
  whose forced slot differs from the ladder's answer, during target
  acquisition, threat scoring or retaliation. Effect: those gates use the
  unforced slot. Frequency: every such evaluation. Risk: a unit may acquire,
  score or retaliate with the wrong weapon slot; firing itself is correct.
- `range_with_attach_effects` models the attach-effect arm of Phobos
  `GetRangeWithModifiers`; the Ares RANGE-ability term is not applied, so an
  in-range override with `ApplyRangeModifiers=yes` on such a firer may differ
  by that bonus. Trigger: Ares RANGE ability plus an in-range force list.

Tests: the reader defaults/overrides, the `[General]` fallbacks, and a
production selection test covering a class arm, the air-only arm replacing the
class arm, the in-range hit and the fall-through outside its range.
