# Warhead debris block

A bullet's warhead throws death debris from its own `MinDebris=`/
`MaxDebris=` budget. Phobos replaces the whole block
(`BulletClass_Logics_Debris`, hook `0x469D1A`,
`Ext/Bullet/Hooks.DetonateLogics.cpp:171-242`) with a fixed budget, a
restored per-slot loop and two extra warhead keys.

```ini
[General]
MetallicDebris=DBRIS1LG,DBRIS2LG    ; vanilla fallback list
Debris.Conventional=no              ; Valueable<bool>, default no

[WH]                                ; [Warheads] section
MinDebris=2
MaxDebris=3
DebrisTypes=TIRE
DebrisMaximums=4
DebrisMinimums=3                    ; ValueableVector<int>, default empty
DebrisTypes.Limit=yes               ; Nullable<bool>, unset = count > 1
DebrisAnims=DBRI-WM1                ; ValueableVector<AnimTypeClass*>
Debris.Conventional=yes             ; Nullable<bool>, over the global
```

## Native body

- Budget: `RandomRanged(MinDebris, MaxDebris)` — the vanilla `DEC EDI`
  (`MaxDebris - 1` top) is not used.
- `count = min(DebrisTypes.Count, DebrisMaximums.Count)`. A nonzero count
  runs the restored loop: one `RandomRanged(min, max(1, maximum))` pick per
  slot, the warhead `DebrisMinimums[i]` (last entry repeating, floored at 0)
  as the floor, the pick capped by the remaining budget, and a single pass
  when `DebrisTypes.Limit` resolves true (unset: `count > 1`).
- A zero count throws the warhead's `DebrisAnims` list, else
  `[General] MetallicDebris=`, one `RandomRanged(0, n-1)` pick per piece.
- `Debris.Conventional` over the global: when true and the bullet's cell
  LandType is `Water`, the whole block is skipped.

## VERA

`WarheadType::phobos_debris_conventional`, `phobos_debris_types_limit`,
`phobos_debris_minimums` and `phobos_debris_anims` read the four
`WarheadTypeExt` keys; `PhobosGlobals::debris_conventional` reads the
`[General]` fallback (both `Ext/WarheadType/Body.cpp:299-302`,
`Ext/Rules/Body.cpp:494`). `combat::world_receiver::emit_warhead_debris`
owns the block: the water skip, the `RandomRanged(Min, Max)` budget, the
shared `throw_typed_debris` restored loop and the warhead
`DebrisAnims`/metallic SHP tail through
`ShpDebrisSource::WarheadDebrisAnims`. The Techno death hook
(`0x702299`) shares the same loop through `throw_death_debris`.

## Tests

- `src/sim/combat/world_receiver_debris_tests.rs` — the Min..Max budget,
  the type/global `Debris.Conventional` water skip (and Beach not being
  Water, an explicit no beating the global), the warhead `DebrisMinimums`
  through a one-pass typed loop, and the warhead `DebrisAnims` list before
  the metallic fallback. All four paths were mutated red and restored.

## Residuals

- The cell is the detonation impact's cell; native reads the bullet's
  current cell, which differs only across a cell boundary mid-frame.
- `DebrisAnims` unresolvable names are dropped at use time, shrinking the
  draw space as native's load-time filter does.
- VERA's INI store drops an empty `DebrisAnims=` value, so an explicitly
  empty list reads as unset and falls back to the metallic list; Phobos
  keeps it set-but-empty (no SHP debris, no draws).
- The SHP anims carry the firer's house (`pOwner`); the voxel pieces use
  the same resolved house.
- The vanilla arm's 20-lepton lift is omitted under Phobos, whose hook
  passes its coordinate unchanged.
