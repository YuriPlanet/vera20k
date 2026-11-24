# A disguised object moves at its disguise's speed (`UseDisguiseMovementSpeed`)

One `Nullable<bool>` that lets a disguised object inherit the `Speed=` of the
type it pretends to be.

## Keys

| Key | Type | Default |
|---|---|---|
| `UseDisguiseMovementSpeed=` | `Nullable<bool>` | unset |

Read: `TechnoTypeExt::LoadFromINI` (`Ext/TechnoType/Body.cpp:877`),
declaration `Ext/TechnoType/Body.h:147`, constructor `Body.h:544`. VERA reads
it in `src/rules/phobos_techno_extras.rs`, owner
`ObjectType::phobos_techno_extras`.

Falls back to `[General] UseDisguiseMovementSpeed` (`Ext/Rules/Body.cpp:221`,
unset). Both the local key and the global are `Nullable<bool>`, and native
passes the global's *value* as the default, so an unset pair resolves to `no`.

## Behaviour

`TechnoClass::GetMaxSpeed @ 0x70EFE0` (`Ext/Techno/Hooks.cpp:806-824`):

```cpp
int maxSpeed = pThisType->Speed;
if (pTypeExt->UseDisguiseMovementSpeed.Get(Global()->UseDisguiseMovementSpeed) && pThis->IsDisguised())
{
    if (auto const pType = TechnoTypeExt::GetTechnoType(pThis->Disguise))
        maxSpeed = pType->Speed;
}
R->EAX(maxSpeed);
```

`GetTechnoType` answers null for a disguise the catalog does not name, and the
object then keeps its own `Speed=`.

VERA's owner is `sim::movement::foot_speed`, whose `max_speed` now replaces the
bare `object.speed` that `adjusted_speed` used. Disguise state comes from
`GameEntity::disguise` (`DisguiseRuntime::is_disguised` plus `type_id`), and the
disguise type resolves through the interner.

## Residuals

- **`TechnoTypeExt::GetTechnoType(pThis->Disguise)`** climbs the same
  `WhatAm_I`/type-pointer chain VERA's `DisguiseRuntime::type_id` already
  holds, so no second lookup rule is invented.
- **`GetMaxSpeed`'s other readers.** Only `FootClass::GetCurrentSpeed`'s input
  is redirected. `base_defense_response` scores a candidate by
  `object.speed` directly (its comment pins that to the same virtual); that
  scorer is a threat estimate, not a mover, and is left as-is.

## Tests

- `src/rules/phobos_techno_extras_tests.rs` — the parse group.
- `src/rules/phobos_globals_tests.rs` — `the_disguise_voice_and_hover_globals`.
