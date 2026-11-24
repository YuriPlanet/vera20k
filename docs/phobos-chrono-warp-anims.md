# Warp animation lists

A teleporting techno can draw its endpoint animation from per-type lists
instead of the single `[General] WarpIn` / `WarpOut` animation. Phobos adds
four `ValueableVector<AnimTypeClass*>` keys, read in
`Ext/TechnoType/Body.cpp:857-860` (fields `Ext/TechnoType/Body.h:123-124`,
constructed empty at `:524-525`).

```ini
[TECHNO]                       ; TechnoType
WarpIn=                        ; AnimType list, default empty
WarpOut=                       ; AnimType list, default empty
Chronoshift.WarpIn=            ; AnimType list, default empty
Chronoshift.WarpOut=           ; AnimType list, default empty
```

The native `ValueableVector` reader drops a name that does not resolve, so
only declared AnimTypes reach the lists.

## Behavior

Each hook replaces the stock single-animation construction with its own
selection chain (`Ext/TechnoType/Hooks.Teleport.cpp:13-140`); every pick is
`AnimExt::CreateRandomAnim(list, Location, nullptr, pLinked->Owner)`, a
`RandomRanged(0, size - 1)` draw whose animation is owned by the teleporting
object's house, with no invoker and no owner-object link.

| Native site | Chain |
|---|---|
| Ordinary departure `0x7193F6` | `WarpOut` > `[General] WarpOut` |
| Ordinary arrival `0x719742` | `WarpIn` > `[General] WarpIn` |
| Chronoshift departure `0x719827` | `Chronoshift.WarpOut` > `WarpOut` > `[General] WarpOut` |
| Chronoshift arrival `0x719B1E` | `Chronoshift.WarpIn` > `WarpIn` > `[General] WarpIn` |

A list with no resolvable name behaves like an empty list and consumes no
draw; the `[General]` animation then plays exactly as before.

## Where VERA20K owns it

The four lists live in `rules::phobos_warp_anim::WarpAnimRules` on
`ObjectType::phobos_warp_anim`. The single consumer is
`sim::movement::teleport_movement::Simulation::teleport_warp_anim`, which
takes a `WarpAnimKind` for the four native sites and shares the existing
`spawn_owned_anim` random-anim owner (registry filter, `RandomRanged`, owner
house). It is called from the ordinary Process sites in
`sim::world::object_turn` and the Chronosphere states 2 and 5 in
`sim::movement::teleport_chrono`, replacing the former direct
`teleport_warp_out` calls. The `[General]` fallback is the unchanged
`spawn_general_warp_anim`.

## Residuals

- Arrival fallback: the ordinary and Chronoshift arrival sites keep VERA's
  existing `[General] WarpOut` animation. Native's last fallback there is
  `[General] WarpIn` (`Hooks.Teleport.cpp:82,133`); switching it would change
  stock builds too, so it is left for the stock warp-in animation chain.
- List priority walks resolved names: a Chronoshift list whose authored names
  all fail to resolve behaves like the load-time-empty vector and falls
  through to the type's `WarpIn`/`WarpOut` list, then to `[General]`.
- Chronosphere state 5 has no separate `[General]` `WarpIn` pick; with all
  four lists empty both Chronosphere endpoints play `[General] WarpOut` as
  before.
- The Chronosphere's `WarpIn`/`WarpOut` list pick only runs when the entity's
  type is still resolvable; a limboed or dead object falls back to the
  `[General]` animation.
