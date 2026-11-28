# Base-center exclusion (`IgnoreForBaseCenter`)

A TechnoType can opt out of its House's base-center math: the building or
starting techno still exists and plays normally, but it does not pull the
House's base center, and a starting techno likewise does not pull the opening
view's average. Phobos reads the flag as a `Valueable<bool>` (default
`no`) in `Ext/TechnoType/Body.cpp:810`, declared `Body.h:77`, constructed
`false` at `:515`.

```ini
[SOMETECHNO]
IgnoreForBaseCenter=no                      ; boolean, default no
```

## Native evidence and hooks

| Address | Hook | Behavior |
| --- | --- | --- |
| `0x4FD1CD`, `0x4FD463` | `HouseClass_RecalcCenter_LimboDelivery` | The building loop skips a building whose map coordinates are illegal, a limbo-delivered building the House still owns, or one whose type has `IgnoreForBaseCenter`. `Ext/House/Hooks.cpp:94-105`. |
| `0x4AC534` | `DisplayClass_ComputeStartPosition_IllegalCoords` | The starting-techno loop skips a techno with illegal map coordinates or `IgnoreForBaseCenter`. `Ext/House/Hooks.cpp:111-120`. |
| `0x4FD166` | `HouseClass_RecalcCenter_SetContext` | Installs the House extension before the loop so the limbo-delivery ownership test can run (`:85-91`). |

The flag affects only the center math; it does not remove the object, change
its mission, or alter `BaseSpawnCell`.

## Where VERA20K owns it

- `rules::object_type::ObjectType::phobos_ignore_for_base_center` reads
  `IgnoreForBaseCenter` with `read_bool` (constructor default `false`).
- `sim::world::house_base::recalculate_house_base_geometry` is VERA's
  `HouseClass::RecalculateBase @ 0x004FD150` projection. It builds the single
  admitted list that `project_geometry` weights into the centre and then walks
  again for the radius, so one filter serves both hook sites
  (`0x4FD1CD` / `0x4FD463`, `Ext/House/Hooks.cpp:94-105`): a building whose
  type has the flag never enters the list, leaving the weighted numerator and
  its denominator together. Phobos' hook name says `RecalcCenter`; the patched
  addresses lie inside the function VERA ports here.
  `initialize_house_base_from_build_const` (House 0x0050C210, the `BuildConst`
  anchor) keeps its own gate on the same key.
- `sim::world::camera_home::opening_view_on_player_technos` is
  `DisplayClass::CenterCameraOnPlayerTechnos @ 0x004AC4F0`. The hook Phobos
  names `DisplayClass_ComputeStartPosition_IllegalCoords` (`0x4AC534`,
  `Ext/House/Hooks.cpp:111-120`) patches the same routine: an ignored techno is
  dropped from the wrapping sum and from the count, so a house whose starting
  technos are all ignored keeps `HouseClass::Get_Base_Or_Starting_Coord`'s
  coordinate as its opening view. VERA has no separate start-position scan;
  this opening view is the native centre read the Phobos gate covers, and
  `set_base_center` remains the only writer of the base centre.

Other base-centre writes (the launch waypoint, the auto-base trigger's first
`BuildConst` cell, an MCV deploy) are explicit assignments, not averages,
and native filters none of them with this key.

## Phobos quirks kept

- The key is a `Valueable<bool>`: an absent, empty or invalid value keeps the
  constructor's `false`; there is no unset state or global fallback.
- The skip applies to the building's map coordinate read, not to its
  foundation or ownership: an ignored building still pays upkeep, counts for
  power and can be sold.
- An ignored building cannot become the BuildConst anchor; a non-ignored
  earlier building becomes the center instead.

## RNG draws

None; the center derivation is a list scan and coordinate read.

## Residuals (trigger · impact · frequency · risk)

- **The illegal-coordinate term.** Both hooks also skip a techno or building
  whose map coordinate is illegal; neither VERA loop models that term, because
  a stored entity always carries a cell. Trigger: an object placed outside the
  map. Frequency: rare. Risk: such an object would still count.
- **Other base-centre writers.** The launch waypoint
  (`sim::scenario_bootstrap::assign_launch_base_centers`), the auto-base trigger
  and an MCV deploy assign the centre directly; native leaves them alone too.
  Trigger: none. Frequency: always. Risk: none.
- **Network / multi-client opening views.** Native runs
  `CenterCameraOnPlayerTechnos` per client; VERA's port answers for the local
  current House. The gate reads only the techno's type, so a peer's answer can
  differ only through its own object set. Untested.
- **Retail/visual parity untested** (no retail `ini/` in this checkout).
