# Roof production exit (`ExitThroughRoof`)

A produced unit whose factory draws it through the roof animation instead of
the gate.

## Keys

| Key | Type | Default |
|---|---|---|
| `ExitThroughRoof=` | `Nullable<bool>` | unset |

Read: `TechnoTypeExt::LoadFromINI` (`Ext/TechnoType/Body.cpp:1200`),
declaration `Ext/TechnoType/Body.h:437`, constructor `Body.h:859`. VERA reads
it in `src/rules/phobos_exit_through_roof.rs`, owner
`ObjectType::phobos_exit_through_roof`. Tests:
`src/rules/phobos_exit_through_roof_tests.rs`.

## Behaviour

Two consumers answer the same key with **different** fallbacks
(`Ext/Building/Hooks.cpp`):

```cpp
// :1053 — "is this a roof-exit techno?"
static bool IsRoofExitTechno(TechnoTypeClass* pType)
{
    return TechnoTypeExt::Fetch(pType)->ExitThroughRoof
        .Get(pType->JumpJet || pType->BalloonHover);
}

// :1068 — BuildingClass_DrawDoorAnim (0x43D0CF)
return ExitThroughRoof.Get(pType->JumpJet) ? UseRoof : UseGate;
```

So the predicate defaults to `JumpJet || BalloonHover` while the door-anim
**draw** defaults to `JumpJet` alone, and an explicit `yes`/`no` on the type
outranks both. `IsRoofExitBuildingUnit` (`:1058`) then applies it to the
building's `GetNthLink` unit. A third hook, `BuildingClass_DrawUnderDoorAnim`
(`:1071`, `0x43D363`), marks the roof or skips the under-door anim on the
same answer.

VERA owns both answers as
`PhobosExitThroughRoof::exits_through_roof(jump_jet, balloon_hover)` and
`::draws_through_roof(jump_jet)`.

The unload-art owner `factory_unload_art`
(`src/app/presentation/instances/shp.rs`, the `BuildingClass::DrawIt
@ 0x0043D290` block at `0x0043D33F..0x0043D381`) now passes the radio
contact-0 product's type through `exits_through_roof` (the `0x43D363`
predicate), so `ExitThroughRoof=` selects the roof or gate pair while a
jump-jet or balloon-hover product keeps the vanilla roof default.

## Residual

**`BuildingClass_DrawDoorAnim` (`0x43D0CF`) has no separate VERA owner.** The
`draws_through_roof(jump_jet)` answer (the narrower fallback) is retained for
that draw, but VERA's factory door art is emitted only from the DrawIt unload
block above. Trigger: a build that draws the standalone door anim outside
`DrawIt`. Effect: the narrower `JumpJet`-only default is not consulted; the
DrawIt predicate covers the same produced-unit art pair. Frequency: none in
retail. Risk: a mod that separates the two door draws may pick the other art.

## Tests

- `src/rules/phobos_exit_through_roof_tests.rs` — the two fallbacks are not
  the same predicate, the type's value outranks both, and the `<default>` /
  `none` reset tokens.
- `src/app/presentation/instances/factory_door_tests.rs` —
  `exit_through_roof_overrides_the_vanilla_locomotor_gate`: a ground product
  with `ExitThroughRoof=yes` takes the roof pair and a jump-jet product with
  `no` takes the gate pair; the retail test above covers the unset default.
