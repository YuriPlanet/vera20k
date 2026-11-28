# Anim VisibleTo

Phobos gates which houses see an animation. `VisibleTo` names the houses
relative to the anim's owning house; `VisibleTo.ConsiderInvokerAsOwner`
resolves that house from the anim's invoker when one exists.

```ini
[SOMEANIM]                         ; AnimationType / art section
VisibleTo=owner,allies             ; AffectedHouse, default all
VisibleTo.ConsiderInvokerAsOwner=no ; boolean
```

Readers: `VisibleTo` (`Valueable<AffectedHouse>`, constructor `All`) at
`Ext/AnimType/Body.cpp:109` and `Body.h:64,116`; the invoker key at
`Ext/AnimType/Body.cpp:110` and `Body.h:65,117`.

## Native body

- `AnimClass_DrawIt_Visibility` (hook `0x423061`,
  `Ext/Anim/Hooks.cpp:520-570`) returns early when
  `RestrictVisibilityIfCloaked=` is off and `VisibleTo` is `All`. Otherwise it
  resolves the owning house as `OwnerObject`'s house, or the anim's own
  `Owner` without an owner object. With `VisibleTo.ConsiderInvokerAsOwner`
  and an `Invoker` (or else `InvokerHouse`), that house replaces the owner.
- The draw is skipped unless the current house is an observer or
  `EnumFunctions::CanTargetHouse(VisibleTo, CurrentHouse, owner)` admits it.
  `AffectedHouse::None` is a valid value and hides the anim from everyone.

## VERA

`PhobosAnimTypeExt::visible_to` reads the key with `Valueable` semantics
(`none` stays `None`, rejected text keeps the unset `All` default).
`AnimTypeRuntimeConfig::phobos_visible_to_consider_invoker_as_owner` reads the
boolean. `Simulation::anim_visibility_skipped` combines the cloak arm with
this house gate, resolving the owner object's house first, then substituting
the invoker entity's house (or the invoker house) when keyed, and asks the
shared `can_target_house`.

## Tests

- `src/rules/phobos_anim_type_ext.rs` — the existing reader test now pins
  `VisibleTo=none` as `AffectedHouse::NONE` and rejected text as unset.
- `src/sim/anim_class.rs` —
  `visible_to_filters_anim_draws_by_the_owner_or_invoker_house`: `owner`
  hides from the enemy and shows to the owner, `enemies` the reverse, and a
  keyed invoker house replaces the owner object's house (an unkeyed anim
  keeps the owner's).
- `a_cloaked_owners_anim_hides_only_from_unsensing_unallied_viewers` keeps
  covering the cloak arm through the same entry point.

The invoker arm, the VisibleTo read, and the pre-existing cloak arm were each
mutated red and restored.

## Residuals

- The cloak arm's trailer-anim invoker fallback (`IsTechnoTrailerAnim`) has no
  VERA marker; such a non-techno-owned anim keeps the stock answer.
- `VisibleTo` is read from the one ARTMD layer; a scenario-layer override
  never applies to animation types.
