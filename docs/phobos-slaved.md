# Slaves' house on master-killed (`Slaved.OwnerWhenMasterKilled`)

Who a slave miner's infantry belongs to once its master dies.

## Keys

| Key | Type | Default |
|---|---|---|
| `Slaved.OwnerWhenMasterKilled=` | `Valueable<SlaveChangeOwnerType>` | `Killer` |

Read: `InfantryTypeExt::LoadFromINI` (`Ext/InfantryType/Body.cpp:18`),
declaration `Ext/InfantryType/Body.h:14`, constructor `Body.h:39`
(`SlaveChangeOwnerType::Killer`). VERA reads it in
`src/rules/phobos_slaved.rs`, owner `ObjectType::phobos_slaved`. Tests:
`src/rules/phobos_slaved_tests.rs`.

`Utilities/Enum.h:178-184`:

```cpp
enum class SlaveChangeOwnerType { Killer = 0, Master = 1, Suicide = 2, Neutral = 4 };
```

Note `Neutral = 4`, not `3` — those are the exact `switch` values
`SlaveManagerClass_Killed_DecideOwner` compares against.

## Behaviour (not yet consumed)

`SlaveManagerClass_Killed_DecideOwner`
(`Ext/Techno/Hooks.Misc.cpp:27-72`, hooks `0x6B0B9C` and `0x6B0BA4`) runs once
per slave, before the vanilla owner decision:

| Key | killer present | killer == 0 (master sold / self-destroyed) |
|---|---|---|
| `Suicide` | `KillTheSlave` (`0x6B0BDF`) | `KillTheSlave` |
| `Master` | `R->EAX(pSlave->Owner)` → `ChangeSlaveOwner` (`0x6B0BB4`) | same |
| `Neutral` | `HouseClass::FindNeutral()` → `ChangeSlaveOwner`; no neutral → falls through to `Killer` | same |
| `Killer` (default) | `return 0`, i.e. the vanilla path gives the slave to the killer | neutral house, else `KillTheSlave` |

The `killer == 0` arm exists because `0x6B0BA4` is the vanilla
`test eax,eax` on the killer: Phobos bypasses it and branches explicitly so
the "sold master" case lands on the neutral house rather than crashing
through a null killer.

A slave in limbo (inside its master) still dies with the master regardless of
this key; the hook only decides the outside ones.

## The freeing policy (ported)

The hook is wired into the `FreeSlaves` port,
`SlaveManager::free_slaves_retaining_storage` (`src/sim/slave_manager.rs:1783`).
After clearing `entity.slave.owner` and keeping the limbo arm (inside dies
there), the per-slave loop resolves the **slave's** type
(`interner.object_type(type_ref, rules).phobos_slaved.owner_when_master_killed`,
`:1842-1845`) and reproduces `SlaveManagerClass_Killed_DecideOwner`
(`Ext/Techno/Hooks.Misc.cpp`, hooks `0x6B0B9C` and `0x6B0BA4`):

| Value | Arm (`Hooks.Misc.cpp`) | VERA |
| --- | --- | --- |
| `Suicide` | `KillTheSlave` | `slave_dies_unfreed` |
| `Master` | `pSlave->Owner` | the slave's current owner, kept even with a killer |
| `Neutral` | `FindNeutral()`, else fall through | neutral house, else the killer/default house |
| `Killer` (default) | fall through to vanilla | killer, else neutral house, else kill |

The two vanilla arms differ only in the killer: with a killer present the
fall-through hands the slave to that killer, without one (`0x6B0BA4`, master
sold or self-destroyed) Phobos explicitly gives it the neutral house and
otherwise kills it — so the stock `house` argument is deliberately not a
fallback here (comment at `:1798-1805`). Tests: `src/rules/phobos_slave_tests.rs`
(parse, `Neutral = 4`), and the dispatch itself in
`src/sim/world/slave_manager_cycle_tests.rs` — `a_suicide_slave_dies_when_its_master_is_killed`,
`a_master_slave_keeps_its_house` (a killer is present),
`a_neutral_slave_joins_the_neutral_country_house`, the killerless `0x6B0BA4`
arm `a_killerless_release_ignores_an_explicit_house` (without a Neutral House
the slaves die), and the `SlavesFreeSound` override.

No residual: the four values behave as the hook does; only the unset key keeps
the stock killer-else-house-else-civilian arm, as Phobos' fall-through does.

## Tests

- `src/rules/phobos_slaved_tests.rs` — the four names and their exact native
  discriminants (`Neutral` is `4`), the case-insensitive parse, and that an
  unknown or blank read keeps `Killer` because a `Valueable` has no reset
  token.
