# Optional Phobos custom radiation types

Build with `--features phobos` to register `[RadiationTypes]`, bind object
weapons with `RadType=`, and read the per-type radiation keys. Default builds
retain the original single global `[Radiation]` configuration.

Each weapon's `RadType=` names a `[RadiationTypes]` entry; an absent, empty or
unknown name binds the default `Radiation` entry, which reads the vanilla
`[Radiation]` section. Every key is nullable: an omitted key falls back to the
global value, so a later Rules layer that omits a key retains the earlier
layer's read.

In `rulesmd.ini`:

```ini
[RadiationTypes]
0=SOMERADTYPE

[SOMEWEAPON]                       ; WeaponType
RadType=SOMERADTYPE                ; RadType to use instead of default of [Radiation]

[SOMERADTYPE]                      ; RadType
RadDurationMultiple=1              ; integer
RadApplicationDelay=16             ; integer
RadApplicationDelay.Building=0     ; integer
RadBuildingDamageMaxCount=-1       ; integer
RadLevelMax=500                    ; integer
RadLevelDelay=90                   ; integer
RadLightDelay=90                   ; integer
RadLevelFactor=0.2                 ; floating point value
RadLightFactor=0.1                 ; floating point value
RadTintFactor=1.0                  ; floating point value
RadColor=0,255,0                   ; integer - Red,Green,Blue
RadSiteWarhead=RadSite             ; WarheadType
RadSiteWarhead.Detonate=false      ; boolean
RadSiteWarhead.Detonate.Full=true  ; boolean
RadHasOwner=false                  ; boolean
RadHasInvoker=false                ; boolean

[Radiation]
UseGlobalRadApplicationDelay=true  ; boolean
RadApplicationDelay.Building=0     ; integer
RadBuildingDamageMaxCount=-1       ; integer
RadSiteWarhead.Detonate=false      ; boolean
RadSiteWarhead.Detonate.Full=true  ; boolean
RadHasOwner=false                  ; boolean
RadHasInvoker=false                ; boolean
```

The bound type owns the site's lifetime (`RadDurationMultiple`), decay cadence
(`RadLevelDelay`), light (`RadLightDelay`, `RadLightFactor`, `RadTintFactor`,
`RadColor`), the creation-time level clamp (`min(RadLevel, RadLevelMax)`), and
the damage a foot takes: the per-cell levels group by bound type, each type's
sites apply `min(level, remaining share of RadLevelMax) x RadLevelFactor`
through the type's own `RadSiteWarhead`. When `UseGlobalRadApplicationDelay` is
false, each type's own `RadApplicationDelay` gates its sites instead of the
shared global gate. A type whose warhead does not resolve deals no damage, as
native's missing-warhead skip does.

`RadColor` reaches the field through the site's light, and the theater has no
say in a Phobos build. Retail `RadSiteClass::Activate` substitutes signed -128
for the red and blue channels when `Scenario+1258 == SNOW1`
(`RadSite65B5EA..65B684`), so a retail field takes its color from the theater on
snow maps. Phobos never runs that code: it replaces the whole activation with
`RadSiteExt::CreateLight`, which tints from the bound type's `RadColor` alone —
all three channels, `min((1000 * channel / 255) * RadTintFactor, 2000)` — with
no theater test anywhere in `Ext/RadSite`. A `RadColor=255,0,255` type is
therefore purple on every scenario, snow included, and a build without the
capability keeps the retail substitution.

The retained Rules registry owns the ordered catalog; the type's canonical
name and the bound weapon IDs enter the simulation configuration hash, and the
catalog crosses the save boundary with the retained Rules records. A save
records the build capability and cannot load in the other mode.

`RadHasOwner` and `RadHasInvoker` attribute a site's damage.
`RadSiteExt::CreateInstance` copies the detonating round's house and firer onto
the site only when the bound type asks for them, a merge keeps the existing
site's copies, and each Foot damage call passes them to `ReceiveDamage` — the house
as its source house (native alliance and credit rules) and the firer as its
source object. A type without the flags keeps neither, so its radiation keeps
hitting every house neutrally.

Buildings take radiation in a Phobos build through
`BuildingClass_AI_Radiation @ 0x0043FB23`, the branch that follows the
operational edge sample at `0x0043FB20`. It runs for every live building each
frame over the building's foundation cells, groups each cell's sites by bound
type, clamps each type's summed level at its `RadLevelMax` and applies its
`RadLevelFactor` through its own warhead to the building. The shared gate is
`RadApplicationDelay.Building`; its default `0` means "skip", so stock rules
leave buildings immune even with the capability and a mod opts in by setting a
delay. Immune buildings, warp-out and temporal states skip the branch, and the
type's warhead verses still decide the damage. Phobos' hook tests a per-site
`RadBuildingDamageMaxCount` counter it never increments, so that key has no
observable effect there and none here.

`RadSiteWarhead.Detonate` turns a site's Foot damage into a warhead detonation at
the target instead of a direct `ReceiveDamage` call, so the warhead's own
`CellSpread`, verses and effects apply around the victim.
`RadSiteWarhead.Detonate.Full` selects the `WarheadTypeExt::DetonateAt` form,
which then runs the ordinary detonation anim and flash; without it the site
makes the bare area call that `DamageAreaWithTarget` makes. A site whose
warhead does not resolve deals no damage in either form, as native's
missing-warhead skip does.

Building radiation currently uses the neutral direct receiver.
This feature does not load Phobos DLLs or establish complete mod compatibility.
