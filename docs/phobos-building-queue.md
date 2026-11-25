# Building production queue

Every build reads Phobos' rules key; a mod, mode or map may set it. Default is
`no`.

```ini
[General]
BuildingProductionQueue=no ; boolean
```

Players turn it on with **Gameplay → Building Production Queue** in Options
(`[VERA] BuildingProductionQueue=yes` in `VERA.INI`). That box is a rule
switch, as Terrain Deformation is (`rules/personal_rules.rs`): the next offline
scenario's root gets `[General] BuildingProductionQueue=yes`, so the simulation,
saves, replays and the rules hash all read the same value, and a match keeps
what it started with. A network room plays its host's box instead
(`HostRules`, [room rules](Networking.md#room-rules)), which the host can also
tick in the room's Game Options; a launcher's network match (`tools/spawn`) has
no host and plays without it. The key is read outside `phobos_read`,
so the box does not switch the rest of Phobos on.

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
- `rules/personal_rules.rs` — the Options box composed into the scenario root.
- `app/persistence/options_profile.rs` — the `[VERA]` read/write and default.
- `ui/main_menu_dialogs/options.rs` — the Options checkbox and its event.
- `sim/production/factory_lifecycle.rs` — `enqueue_selected` gates the busy
  refusal and the held-product abandonment; the existing generic queue tail
  already starts the next building after placement.
- `app/input/sidebar_eva.rs` — a busy structure strip accepts the press, and
  Shift repeat may append when the switch is on.
- `app/input/dispatch.rs` — passes the rules switch to the click decision.
- `app/presentation/sidebar_build.rs` — opens the Building/Defense queue-count
  badge under the `phobos` feature or the rules switch.
- `sim/production/factory.rs` — `cancel_one` follows Phobos'
  `HouseClass_AbandonProduction_RewriteForBuilding @ 0x004FAAD8`, which replaces
  vanilla's `BuildCat == DontCare` test so a structure cancel removes queued
  copies before the held product. The rewrite is unconditional in Phobos, and
  the rules switch also enables it in a default build: `cancel_one` follows
  either the `phobos` feature or `[General] BuildingProductionQueue=`. That is what makes Shift + right-click ABANDON_ALL drain every queued
  copy of the clicked type (with `Shift-Click Batch Queue` also on) instead of
  abandoning only the held building.

## Residuals

- The computer's base planner still requests one building at a time; the
  switch does not add new AI planning or queue depth.
- Queued building cameos use the existing generic queue count. The badge's
  factory lookup resolves a structure's Factory only under Phobos'
  `StripClass_DrawStrip_FindFactoryDehardCode @ 0x006A9C54`, which is
  unconditional, so `shows_queue_badge` opens the Building/Defense categories
  under the `phobos` feature or whenever the rules switch is on.
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

## Design stance / 设计立场

The building production queue originates in Phobos, but VERA20K promotes it
to a first-party player option available in **every** build — no feature flag
and no dependence on a Phobos ruleset. It changes how production plays, so it
reaches the simulation as a rule of the scenario that every peer, save and
replay reads, never as one client's preference. It materially improves the
playing experience. A network room plays its host's choice, as it does the
host's Terrain Deformation. This follows the precedent of VERA's Shift-Click
batch queue, which was adopted from Ares and likewise promoted to a normal,
always-available convenience.

建筑生产线虽源自 Phobos，但我们把它转正为所有构建都可用的一方玩家功能：
不依赖 Phobos 编译开关，也不依赖 Phobos 规则集。它会改变生产的玩法，所以
以场景规则的形式进入模拟，所有玩家、存档和录像读到的都一样，而不是某个
客户端自己的偏好。它能显著改善玩家体验。联机房间使用房主的设置，与地形
变形一样。这与 VERA 从 Ares 引入 Shift-Click 批量
队列、并把它转成普通可用功能的先例一致。
