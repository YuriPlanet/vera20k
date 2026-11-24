# Initial spawns number

`[TechnoType] InitialSpawnsNumber=N` starts a `Spawns=` pool with only the
first `N` nodes alive; the rest start dead and regenerate after
`SpawnRegenRate`. `Nullable<int>` read by `Ext/TechnoType/Body.cpp:1117`;
unset uses `SpawnsNumber`.

```ini
[CARRIER]
SpawnsNumber=3
SpawnRegenRate=600
InitialSpawnsNumber=2   ; two live Hornets, the third rebuilds after 600 frames
```

## Native evidence

`Ext/Techno/Hooks.Misc.cpp:162-178` (`0x6B6D44`,
`SpawnManagerClass_Init_Spawns`):

```
if ((int)i >= pTypeExt->InitialSpawnsNumber.Get(pThis->SpawnCount))
{
    pControl->Unit = nullptr;
    pControl->SpawnTimer.Start(pThis->RegenRate);
    pControl->Status = SpawnNodeStatus::Dead;
    ...
}
```

The node index `i` runs over the pool the constructor allocated from
`SpawnsNumber`; `InitialSpawnsNumber` only decides which of those are live.
A value above `SpawnsNumber` therefore leaves the whole pool live; a negative
value starts every node dead.

## Where VERA20K owns it

- `rules::object_type::ObjectType::initial_spawns_number` (`Nullable<int>`).
- `sim::spawn_manager::init_spawn_manager` starts each slot `Regenerating`
  with its `SpawnTimer`: 0 for a live node, `SpawnRegenRate` for the dead
  tail (`sim/spawn_manager.rs:506`).
- `commit_spawn_manager_pool` now materialises only slots whose timer already
  expired, so the constructor no longer fills the dead tail
  (`sim/spawn_manager.rs:1344`).

## Residuals

- The native `Status=Dead` node becomes VERA's `spawn: None` +
  `Regenerating` with a started timer; the existing slot walk rebuilds it on
  expiry, which is the same observable pool state.
- Retail/visual parity is untested here (no retail `ini/` in this checkout).
