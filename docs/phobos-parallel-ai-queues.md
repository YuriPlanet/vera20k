# Optional Phobos per-category AI clone limits

These keys let a skirmish computer house clone-produce in parallel except for
the categories or TechnoTypes you exclude. They work in every build: a
`ForbidParallelAIQueues` key in the rules, mode or map turns Phobos on for the
game ([the Phobos switch](phobos.md#the-phobos-switch)), and without Phobos on
none of this runs. `AllowParallelAIQueues`, which Ares reads too, does not turn
it on.

```ini
[GlobalControls]
AllowParallelAIQueues=yes          ; boolean, default yes
ForbidParallelAIQueues.Infantry=no ; boolean
ForbidParallelAIQueues.Vehicle=no  ; boolean
ForbidParallelAIQueues.Navy=no     ; boolean
ForbidParallelAIQueues.Aircraft=no ; boolean
ForbidParallelAIQueues.Building=no ; boolean

[SOMETECHNO]                       ; TechnoType
ForbidParallelAIQueues=false       ; boolean
```

The tags apply unless `AllowParallelAIQueues=no`: Phobos defaults the gate to
yes, and Ares documents `no` as the switch that stops AI cloning. With the gate
on, the first factory building of a category claims the house's slot. A second
building of the same category does not start its own build while that
category's `ForbidParallelAIQueues.<Category>` is set, or while the house's
current choice for that category (`Producing*TypeIndex`) has
`ForbidParallelAIQueues=yes`. A unit, infantryman or aircraft leaving its
factory, or an abandonment of a forbidden category/type, releases the slot,
and so does the removal of the building that holds it (sold, destroyed or
undeployed, idle or not). A placed structure keeps the building slot: the
building exit's hook (0x44531F) is never reached, since the exit sets the
house's building choice to -1 at its start (0x444F3F) and the compare before
the hook (0x44531B) then fails.

Source: `RulesExt` reads in `src/Ext/Rules/Body.cpp`, the TechnoType read in
`src/Ext/TechnoType/Body.cpp`, `src/Ext/Building/Hooks.Production.cpp`
(0x4401BB, 0x4502F4, 0x4CA07A, 0x444119, 0x444131, 0x443CCA, 0x44531F), and
`HouseExt::OnDetach` in `src/Ext/House/Body.cpp`, which the
`AnnounceInvalidPointer` hook (0x7258D0, `src/Phobos.Ext.cpp`) reaches for a
removed building.

## Where VERA20K owns it

- `rules/phobos_parallel_ai_queues.rs` — the `[GlobalControls]` reads and the
  category query.
- `rules/phobos_switch.rs` — whether Phobos plays (`RuleSet::phobos`), which
  `RuleSet::parallel_ai_queues_act` adds to Ares' gate for every hook below.
- `rules/ruleset.rs` — projection and configuration hash.
- `rules/object_type.rs` — per-type `ForbidParallelAIQueues=`.
- `sim/ai_base_building.rs` — the per-house factory slot per category,
  which Ares' serial queues under `AllowParallelAIQueues=no` use too
  ([ares-building-house-hooks.md](ares-building-house-hooks.md)); the gate
  lets only one of the two act in a game.
- `sim/production/factory_ai.rs` — the start gate.
- `sim/production/production_queue.rs` — the unit/infantry/aircraft exit
  prelude release, before FreeRadio and placement retries.
- `sim/production/factory_lifecycle.rs` — the abandonment release.
- `sim/world/lifecycle/pointer_expiry.rs` — the removal release, in the pointer-expiry
  broadcast.

## Residuals

- The type check reads the native `Producing*TypeIndex` choice fields. Builds
  without NPSE keep the vanilla shared Unit choice for land and naval
  factories, so a naval producer's product check uses that shared slot.
- The aircraft free-dock primary-factory reassignment hook (0x4401BB) is
  dormant because VERA has no AI primary-factory switch on full docks.
