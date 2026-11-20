# RevealToAll radius

`RevealToAll.Radius=` gives a `RevealToAll=yes` building an explicit sight
radius for the circle it shows the current house, instead of its own `Sight`.

```ini
[CAMERA]                        ; Rules BuildingType section
RevealToAll=yes
RevealToAll.Radius=7            ; Nullable<int>, unset keeps Sight
```

Reader: `Ext/BuildingType/Body.cpp:284` (field `Ext/BuildingType/Body.h:128`,
constructor unset `:241`). Hooks `0x446816` (placement) and `0x4ADE55` (the
sight walk) replace the stock `UpdateSight(false,false,true,CurrentPlayer)`
call with an explicit radius `Get(RevealToAll.Radius, Type->Sight)`.

VERA stores the key on `ObjectType::reveal_to_all_radius` and feeds it as the
existing `radius_override` of `entity_sight_admission` when the RevealToAll
footprint is computed (`sim/world/shroud_refresh.rs`), so the stored
alternate admission and the death/Limbo rereveal reuse the same circle.

Tests: `src/sim/world/reveal_to_all_tests.rs` covers the key, `<default>` and
absent rows through the retail-gated world fixture (skipped without retail
INI, run by the retail validation).

Residuals:
- The explicit radius replaces the elevation/veterancy-scaled sight, matching
  the hook's explicit `Type->Sight` argument; elevated RevealToAll buildings
  therefore use the raw radius.
