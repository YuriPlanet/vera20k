# Building production queue (player option)

Tick **Gameplay → Building Production Queue** in Options, or set
`[VERA] BuildingProductionQueue=yes` in `VERA.INI`. It is on in every build,
no feature flag needed. Default is off.

```ini
[VERA]
BuildingProductionQueue=no ; boolean
```

This is a player preference, not a ruleset setting: it is not part of a map or
scenario, is not saved with a game, and is not written into either the ruleset
hash or the simulation state hash. The app reads it from the profile and puts
it on each frame's `FrameEffects`, which the simulation reads.

With the switch on, a BuildingType cameo press appends behind the category
factory's running or completed product instead of being dropped or replacing
the held building. The queued building's construction starts only after the
current completed building is placed. VERA's existing Shift-Click batch queue
may also append multiple building copies when `[VERA] ShiftQueue=` is enabled.

Source: `HouseClass_BeginProduction` busy refusal is skipped at `0x4FA520`,
`FactoryClass_QueueProduction`'s held-product abandon is skipped at `0x4C9C7B`,
and `SelectClass::Action`'s busy structure check is skipped at `0x6AB689`
(`src/Ext/Building/Hooks.cpp`).

## Where VERA20K owns it

- `app/persistence/options_profile.rs` — the `[VERA]` read/write and default.
- `ui/main_menu_dialogs/options.rs` — the Options checkbox and its event.
- `app/match_runtime/sim_tick.rs` — reads the profile once per frame and puts
  it on the tick's `FrameEffects`.
- `sim/world/frame_effects.rs` — carries the flag to the simulation boundary.
- `sim/production/factory_lifecycle.rs` — `enqueue_selected` gates the busy
  refusal and the held-product abandonment; the existing generic queue tail
  already starts the next building after placement.
- `app/input/sidebar_eva.rs` — a busy structure strip accepts the press, and
  Shift repeat may append when the switch is on.
- `app/input/dispatch.rs` — passes the player option to the click decision.
- `app/presentation/sidebar_build.rs` — opens the Building/Defense queue-count
  badge when the option is on.
- `sim/production/factory.rs` — `cancel_one` follows Phobos'
  `HouseClass_AbandonProduction_RewriteForBuilding @ 0x004FAAD8`, which replaces
  vanilla's `BuildCat == DontCare` test so a structure cancel removes queued
  copies before the held product. The rewrite is unconditional in Phobos, and
  this player switch also enables it in a default build: `cancel_one` reads the
  flag from the frame's `FrameEffects` and follows either `phobos` or this
  option. That is what makes Shift + right-click ABANDON_ALL drain every queued
  copy of the clicked type (with `Shift-Click Batch Queue` also on) instead of
  abandoning only the held building.

## Residuals

- The computer's base planner still requests one building at a time; the
  switch does not add new AI planning or queue depth.
- Queued building cameos use the existing generic queue count. The badge's
  factory lookup resolves a structure's Factory only under Phobos'
  `StripClass_DrawStrip_FindFactoryDehardCode @ 0x006A9C54`, which is
  unconditional, so `shows_queue_badge` opens the Building/Defense categories
  whenever this player option is on.
- The rewrite sits in the shared `House::Abandon_Production` body, but of the
  other `cancel_production_for_owner` callers only the factory building's
  detach (`0x0044EE9E..0x0044EEC3`: `all = false` with the held type's index)
  changes in a Phobos build: where a queued copy of the held type exists, it
  now removes that copy and keeps the held product. The mobile delivery and
  `refund_failed_delivery` callers have no BuildCat and already took this arm,
  and `abandon_after_placement` runs after `complete_placement` cleared the held
  product, so Phobos' own null-held skip keeps vanilla there. Only the sidebar
  right-click path is covered by tests here.
- The structure cancel order comes from the static decode of
  `0x004FAAC4..0x004FAB2F`. `factory::tests` now executes the structure
  ABANDON_ALL drain-then-abandon sequence (`cancel_all_structures_drain_the_queue_then_the_held_product`
  and `building_queue_option_drains_copies_in_a_default_build`), so it is no
  longer only instruction-supported.
- Shift-Click batching for buildings is VERA's convenience, not native
  behavior; it remains governed by `[VERA] ShiftQueue=` and
  `[General] MaximumQueuedObjects=`.
