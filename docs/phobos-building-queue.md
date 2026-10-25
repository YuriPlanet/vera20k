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

## Residuals

- The computer's base planner still requests one building at a time; the
  switch does not add new AI planning or queue depth.
- Queued building cameos use the existing generic queue state; no new strip
  painting is added.
- Shift-Click batching for buildings is VERA's convenience, not native
  behavior; it remains governed by `[VERA] ShiftQueue=` and
  `[General] MaximumQueuedObjects=`.
