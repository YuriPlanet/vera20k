# Cloak keys

A cloaking host can eject its parasite.

```ini
[TechnoType]
Cloak.KickOutParasite=               ; Nullable<bool>, unset -> [General]

[General]
Cloak.KickOutParasite=no             ; Valueable<bool>
```

Reader `Ext/TechnoType/Body.cpp:1205`, global `Ext/Rules/Body.cpp:289`
(constructor `Ext/Rules/Body.h:1056`).

## Behavior

- `FootClass_PointerGotInvalid_Parasite` (`0x4D9992`,
  `Ext/Techno/Hooks.cpp:1415-1445`): the parasite owner's `PointerExpired`
  receives the real removal control instead of stock's constant true, and a
  cloak's `Detach_All(false)` control falls back to the host type's key, else
  `[General]`. With neither set, a cloaking host keeps its parasite.
- A true control releases the parasite and clears the victim's own link; a
  false one leaves both in place.

## Residuals

The cloak/decloak anim is spawned on the next AI visit after a transition that happens outside the
cloak AI, one frame later than native. Presentation only: no state, timer or RNG draw shifts.

## Where VERA20K owns it

`rules::phobos_globals::cloak_kick_out_parasite` and
`ObjectType::phobos_cloak_kick_out_parasite` feed
`sim::combat::parasite::foot_parasite_pointer_expired`, the single pointer-
expiry owner. The Ares hooks (`10048280`/`100482D0`) use the same real-control
model, so the two features agree; without either feature stock's constant true
still releases the parasite.

## CloakAnims / DecloakAnims

```ini
[AudioVisual]
CloakAnims=                                ; ValueableVector<AnimTypeClass*>
DecloakAnims=

[SOMETECHNO]                               ; TechnoType
CloakAnims=                                ; ValueableVector, empty -> [AudioVisual]
DecloakAnims=
```

Readers `Ext/Rules/Body.cpp:287-288`, `Ext/TechnoType/Body.cpp:1203-1204`;
consumers `TechnoClass_Cloak_CloakAnim @ 0x7037F7` and
`TechnoClass_Uncloak_DecloakAnim @ 0x703736`
(`Ext/Techno/Hooks.cpp:1405-1420`). The list is resolved type-first, else the
`[AudioVisual]` fallback; one `RandomRanged(0, len-1)` pick
(`AnimExt::CreateRandomAnim`, `Ext/Anim/Body.cpp:407-430`) spawns at the
techno's center with the techno as invoker and its house as owner, attached
as the anim's owner object.

VERA: `rules::phobos_globals::{cloak_anims,decloak_anims}` and
`ObjectType::{phobos_cloak_anims,phobos_decloak_anims}`;
`CloakRuntime::pending_anim` records the transition edge in
`StartCloaking`/`StartUncloaking`, and
`Simulation::drain_cloak_anim` (run from `cloak_field_ai_step`) spawns it.
The anim is spawned on the next AI visit after a transition that happens
outside the cloak AI (one frame later than native; presentation only).
