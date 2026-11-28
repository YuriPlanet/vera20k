# Customizable spawns queue (`Spawns.Queue`)

A `ValueableVector<AircraftType*>` that overrides which type a spawn manager
builds into each of its slots, and which it rebuilds next when one is lost.

## Keys

| Key | Type | Default |
|---|---|---|
| `Spawns.Queue=` | `ValueableVector<AircraftType*>` | empty |

Read: `TechnoTypeExt::LoadFromINI` (`Ext/TechnoType/Body.cpp:1118`),
declaration `Ext/TechnoType/Body.h:334`, constructor `Body.h:768`. VERA reads it
in `src/rules/phobos_techno_extras.rs`, owner
`ObjectType::phobos_techno_extras.spawns_queue`.

## Behaviour

Two hooks, both in `Ext/Techno/Hooks.Misc.cpp`:

```cpp
// 0x6B6D44, SpawnManagerClass::Init — slot i
if (Spawns_Queue.size() <= i || !Spawns_Queue[i]) return Continue;
R->EAX(Spawns_Queue[i]->CreateObject(pThis->Owner->Owner));

// 0x6B78D3, SpawnManagerClass::Update — regeneration
std::vector<AircraftTypeClass*> vec = Spawns_Queue;
for (const auto pNode : SpawnedNodes)
    if (pNode->Unit) { auto it = find(vec, pNode->Unit->Type); if (it != vec.end()) vec.erase(it); }
if (vec.empty() || !vec[0]) return 0;
R->EAX(vec[0]->CreateObject(pOwner->Owner));
```

So the init arm names the type per slot by index, and the regeneration arm
picks the first listed type that no live node currently is.

VERA stores the resolved queue on `SpawnManagerState::spawn_queue` (unknown
names dropped, as the `ValueableVector` reader does) and `fill_slot`
(`src/sim/spawn_manager.rs`) selects between the two arms through its
`initial` flag: the constructor transaction uses `queue[index]`, a
regenerating slot uses the first queue entry whose type no live slot carries,
and both fall back to `Spawns=` when the queue is exhausted.

## `InitialSpawnsNumber=` (ported)

The other half of the same `0x6B6D44` hook
(`Ext/Techno/Hooks.Misc.cpp:170-178`) is wired: `Simulation`'s spawn-manager
constructor reads the type's `InitialSpawnsNumber=` (`rules/object_type.rs`
field, read at `:7298` as a `Nullable<int>`), falls back to `SpawnsNumber=`
when the key is unset, clamps it into `0..SpawnsNumber`, and starts every slot
at or past that count `Dead` with `SpawnTimer.Start(SpawnRegenRate)`
(`sim/spawn_manager.rs:536-561`). Regression test
`initial_spawns_number_leaves_the_tail_dead_until_regen`
(`sim/spawn_manager_tests.rs:3043`) lowers the key below `SpawnsNumber=` and
asserts the tail only appears after the regen delay.
No residual.

## Tests

- `src/rules/phobos_techno_extras_tests.rs` — the parse group
  (`the_two_type_lists_split_on_commas_only` covers the same `ValueableVector`
  reader through the two `RadarJam*` lists, and `every_key_reads_on_its_own`
  covers `Spawns.Queue`'s own split).
- `src/sim/spawn_manager_tests.rs` —
  `spawns_queue_seeds_the_constructor_slots_in_order` (`0x6B6D44`) and
  `spawns_queue_regeneration_skips_types_that_are_still_alive` (`0x6B78D3`,
  including the `Spawns=` fallback once every queued type is alive).
