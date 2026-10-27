# Optional Phobos building production queue

Build with `--features phobos` and set `[General] BuildingProductionQueue=yes`
to queue buildings like mobile production. Default is `no`.

```ini
[General]
BuildingProductionQueue=false ; boolean
```

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

- `rules/ruleset.rs` — the `[General]` read and its configuration hash.
- `sim/production/factory_lifecycle.rs` — `enqueue_selected` gates the busy
  refusal and the held-product abandonment; the existing generic queue tail
  already starts the next building after placement.
- `app/input/sidebar_eva.rs` — a busy structure strip accepts the press, and
  Shift repeat may append when the switch is on.
- `app/input/dispatch.rs` — passes the rules switch to the click decision.
- `sim/production/factory.rs` — `cancel_one` follows Phobos'
  `HouseClass_AbandonProduction_RewriteForBuilding @ 0x004FAAD8`, which replaces
  vanilla's `BuildCat == DontCare` test so a structure cancel removes one queued
  copy before the held product. That rewrite is unconditional in Phobos, so it
  follows the `phobos` feature rather than this switch.

## Residuals

- The computer's base planner still requests one building at a time; the
  switch does not add new AI planning or queue depth.
- Queued building cameos use the existing generic queue count. The badge's
  factory lookup resolves a structure's Factory only under Phobos'
  `StripClass_DrawStrip_FindFactoryDehardCode @ 0x006A9C54`, which is
  unconditional, so `shows_queue_badge` opens the Building/Defense categories
  for the `phobos` feature rather than for this switch.
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
  `0x004FAAC4..0x004FAB2F`. The executed sidebar controls cover the mobile arm
  and the null-held admit, not a structure ABANDON_ALL, so the drain-then-abandon
  sequence is instruction-supported rather than executed.
- Shift-Click batching for buildings is VERA's convenience, not native
  behavior; it remains governed by `[VERA] ShiftQueue=` and
  `[General] MaximumQueuedObjects=`.
