# Fast access to a vehicle or structure (`NoQueueUpToEnter`)

Two keys that let an infantryman board a transport immediately, without waiting
its turn in the transport's boarding queue, when it is already close enough.

## Keys

| Key | Type | Default |
|---|---|---|
| `NoQueueUpToEnter=` | `Nullable<bool>` | unset |
| `NoQueueUpToEnter.BoardDistance=` | `Nullable<int>` | unset |

Read: `TechnoTypeExt::LoadFromINI` (`Ext/TechnoType/Body.cpp:1045-1046`),
declarations `Ext/TechnoType/Body.h:277-278`, constructor `Body.h:709-710`.
VERA reads them in `src/rules/phobos_techno_extras.rs`, owner
`ObjectType::phobos_techno_extras`.

`NoQueueUpToEnter=` falls back to `[General] NoQueueUpToEnter`
(`Ext/Rules/Body.cpp:197`, constructor `Ext/Rules/Body.h:640`, default `no`);
`NoQueueUpToEnter.BoardDistance=` to `[General] NoQueueUpToEnter.BoardDistance`
(`:198`, `Body.h:641`, default `384` leptons). Both live on
`RuleSet::phobos_globals`, along with `[General] NoQueueUpToEnter.Buildings`
(`:200`, `Body.h:643`, unset), which extends the switch to buildings.

## Behaviour (not yet consumed)

`Ext/Techno/Hooks.Transport.cpp:217-851` is a rewrite of the boarding path,
not a hook into it. Its pieces:

- `IsCloseEnoughToEnter(transport, passenger)` (`:218-228`) — a **square**
  range: `|Δx| < BoardDistance && |Δy| < BoardDistance && |Δz| < CellHeight`,
  deliberately not a circular distance.
- `CanEnterNow` (`:231-284`) — replaces the `RadioCommand::QueryCanEnter` send at
  `0x73758A`. Adds the ally, warp, bridge, deactivation, EMP, mind-control,
  parasite and capture-manager refusals, the `AmphibiousEnter` water test, the
  `Passengers.BySize` size prediction, and the overlap guard against the link
  that is already on its way.
- `DoEnterNow` (`:287-...`) — replaces `0x51A21B`/`0x73A6D1`: raises
  `TriggerEvent::EnteredBy`, clears the archive target and the gattling state,
  then `Limbo`, `OnBridge = false`, `NextObject = null` in a fixed order.

## Residual

**The keys are read and retained; the boarding rewrite is not ported.** VERA's
boarding admission still runs the stock radio command, so an infantryman always
joins the queue. Reaching parity means replacing the enter arm of
`UnitClass::Mission_Enter` (currently split across `sim::docking` and the
passenger paths) with the three functions above plus their destination and
`NextObject` bookkeeping — a subsystem-sized change, tracked separately rather
than smuggled in behind two INI keys.

Until then `NoQueueUpToEnter=` and its two distances are inert.

## Tests

- `src/rules/phobos_techno_extras_tests.rs` — the parse group.
- `src/rules/phobos_globals_tests.rs` — `the_board_queue_and_veteran_reload_globals`.
