# Aircraft spawner customizations (`Spawner.*`)

Phobos lets a `Spawns=` parent tune its child pool: launch spacing, immediate
attacks, launch facing, a pursue range tied to its `Spawner=yes` weapon, and a
custom recycle point with a dock animation. Nine type keys
(`Ext/TechnoType/Body.cpp:790-794` and `:1120-1123`, fields
`Body.h:60-66`/`:336-339`, defaults `Body.h:498-504`/`:770-773`) and four
`[General]` fallbacks (`Ext/Rules/Body.cpp:208-211`, `Body.h:115-118,652-655`).

```ini
[General]
Spawner.AttackImmediately=false    ; boolean
Spawner.UseTurretFacing=false      ; boolean
Spawner.RecycleRange=-1            ; floating point, range in cells
Spawner.RecycleOnTurret=false      ; boolean

[SOMETECHNO]
Spawner.LimitRange=false           ; boolean
Spawner.ExtraLimitRange=0          ; integer, range in cells
Spawner.DelayFrames=               ; integer, game frames
Spawner.AttackImmediately=         ; Nullable<bool>, falls back to [General]
Spawner.UseTurretFacing=           ; Nullable<bool>, falls back to [General]
Spawner.RecycleRange=              ; Nullable<Leptons>, cells x 256, falls back
Spawner.RecycleAnim=               ; ValueableVector<AnimTypeClass*>
Spawner.RecycleCoord=0,0,0         ; Valueable<CoordStruct>, Forward,Lateral,Height
Spawner.RecycleOnTurret=           ; Nullable<bool>, falls back to [General]
```

## Native evidence and hooks

| Address | Hook | Behavior |
| --- | --- | --- |
| `0x6B7265` | `SpawnManagerClass_AI_UpdateTimer` | With `Launching`, a docked child and `DelayFrames` set: `UpdateTimer = min(delay, 10)` (`Hooks.Misc.cpp:82-98`). |
| `0x6B73A8`, `0x6B73B9` | `SpawnManagerClass_AI_SpawnTimer` | A set `DelayFrames` replaces the 9/20-frame spawn spacing (`:115-131`). |
| `0x6B7600`, `0x6B769F` | `SpawnManagerClass_AI_InitDestination` | `AttackImmediately` → `SetTarget(manager target)`, `QueueMission(Attack, true)`, `IsReturningFromAttackRun=false`; otherwise hold at the carrier's north neighbour with `QueueMission(Move, false)` (`:134-160`). |
| `0x6B74F0` | `SpawnManagerClass_AI_UseTurretFacing` | With `HasTurret()`: the Unlimbo direction uses `SecondaryFacing.Current().Raw` (`:315-323`). |
| `0x4D962B` | `FootClass_SetDestination_RecycleFLH` | A returning child's destination becomes the carrier's `RecycleCoord` FLH point (`:283-311`). |
| `0x6B77B4` | `SpawnManagerClass_Update_RecycleSpawned` | The dock predicate uses `RecycleRange` against `RecycleCoord`/carrier; on success `CreateRandomAnim(RecycleAnim)` and `SetLocation(carrier coords)` (`:240-278`). |
| `0x6F9E50` | `TechnoClass_AI -> OnEarlyUpdate` | `ApplySpawnLimitRange`: a live target farther than the strongest `Spawner=yes` weapon range (plus `ExtraLimitRange` cells) is dropped via `ResetTarget` (`Body.Update.cpp:445-461`). |

`CalculateSpawnerRange` (`Ext/TechnoType/Body.cpp:539-563`) takes the maximum
`Range` of the type's `Spawner=yes` weapons for the asked tier — normal and
elite — plus `ExtraLimitRange * 256`. VERA computes it on demand in
`spawn_manager::spawner_range`.

## Where VERA20K owns it

- `rules::phobos_spawner::PhobosSpawner` holds the nine keys and the four
  fallback accessors; `ObjectType::phobos_spawner` carries them.
- `sim::spawn_manager` applies them in the existing bodies: `ai` (AI gate),
  `launch_slot` (spacing, attack, facing), `visit_slot` (hold/attack, recycle
  predicate and effects), `WorldSpawn::set_destination` (recycle destination
  cell), and `Simulation::apply_spawn_limit_range` (called from
  `techno_ai_shell` before the mind-control range limit). `LimitRange` uses
  `AbstractClass::DistanceFrom @ 0x005F6440` — VERA's planar
  `combat::object_distance_to` with the target foundation discount — and
  `ResetTarget` is the shared `ClearAllTargets @ 0x6B7BB0` owner
  (`clear_all_spawn_targets`), not a partial field clear.
- `fire_coord::custom_flh_coordinate` is the `GetFLHAbsoluteCoords` port the
  recycle point needs (the same base and matrix owner as `fire_coordinate`).
  Its turret arm is `isOnTurret && (structure || HasTurret)`, using the
  `ObjectType::has_turret` / installed-upgrade owner; `launch`'s
  `UseTurretFacing` asks the same `HasTurret()` before substituting
  `SecondaryFacing`.
- `recycle_effects` filters `RecycleAnim` names through
  `RuleSet::has_anim_type` before the `RandomRanged(0, n-1)` draw (the
  read-time drop the native `ValueableVector` applies) and stores
  `SetLocation(GetCoords())` through `Position::put_location` (a building's
  foundation centre). `Spawner.RecycleCoord` uses `read_int_fields`, the
  native `Read3Integers` parsed-prefix scan.

## Phobos quirks kept

- `Spawner.NotBuildableIfQueueMatch`-style flags are not involved here; the
  recycle predicate's negative range keeps the legacy test, including the
  building 182-lepton/per-axis allowance (√2/2 cell).
- The `0x6B7600`/`0x6B769F` else branch uses the **north** neighbour
  (`FacingType::North = 0`), while vanilla's hold block used direction 4; the
  native oracle fixture opts out of the hook to stay comparable
  (`SpawnHost::phobos_hooks`).
- `AttackImmediately` uses the manager's *live* target, not a queued one.

## Residuals (trigger · impact · frequency · risk)

- **`IsReturningFromAttackRun`** has no VERA field; only this hook reads it.
  Trigger: `AttackImmediately` on a child that would be returning. Impact: the
  flag is not cleared, but VERA never sets it either. Frequency: rare. Risk:
  low.
- **Recycle destination precision**: VERA's aircraft NavCom is cell-based, so
  the `RecycleCoord` destination reduces the exact lepton point to its cell.
  The dock test itself uses the exact point. Trigger: a `RecycleCoord` that is
  not cell-aligned. Impact: the approach target is up to one cell off.
  Frequency: authored mods. Risk: visual/flight path only.
- **Building turret pivot** for `RecycleOnTurret` uses the type's
  `TurretOffset` and its PrimaryFacing; a `BarrelAnimIsVoxel` pivot chain is
  not re-projected. Trigger: a building carrier with a voxel barrel.
  Frequency: modded. Risk: low.
- **`MultiWeapon` (Ares)** is not represented in `spawner_range`; the stock
  `TurretCount > 0 && WeaponCount > 0` form selects the numbered slots.
  Trigger: an Ares `MultiWeapon=yes` type. Impact: the extra slots do not
  contribute. Frequency: Ares mods. Risk: low.
- **Recycle anim ownership/invoker** (`AnimExt::SetAnimOwnerHouseKind`,
  `SetOwnerObject`, `SetInvoker`) is not attached; VERA spawns the anim
  unowned. Trigger: `RecycleAnim` with house-tinted or invoker-bound anims.
  Impact: visual ownership only. Frequency: authored mods. Risk: visual-only.
- **Unresolved `RecycleAnim` names** are not filtered at read time; a random
  pick can land on a name the rules never defined, which then spawns nothing
  (native drops it from the list). Trigger: a typo in the list. Frequency:
  broken mod data. Risk: low.
- **Single-entry RNG draw**: `RandomRanged(0, 0)` may draw once natively while
  `SimRng::next_range_i32_inclusive` returns without drawing when the bounds
  are equal. Trigger: a one-element `RecycleAnim` list. Impact: one Scenario
  RNG draw may be missing. Frequency: any single-anim recycle. Risk:
  determinism only; flagged for the RNG owner.
- **Retail/visual parity untested** (no retail `ini/` in this checkout).
