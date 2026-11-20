# Promotion animations and promoted spawns (`Promote.*`)

Phobos lets a type name the animation that plays when it is promoted, per
tier, overriding the global `[AudioVisual]` lists; and lets a `Spawns=` parent
lift its children to its own Veterancy. Three type keys
(`Ext/TechnoType/Body.cpp:800` and `:1128-1129`, fields `Body.h:68,341-342`,
constructors `Body.h:506,778-779`) and three globals
(`Ext/Rules/Body.cpp:212,316-317`, fields `Body.h:119,239-240`, constructors
`:656,748-749`).

```ini
[General]
Promote.IncludeSpawns=false             ; Valueable<bool>

[AudioVisual]
Promote.VeteranAnimation=               ; ValueableVector<AnimTypeClass*>
Promote.EliteAnimation=                 ; ValueableVector<AnimTypeClass*>

[SOMETECHNO]
Promote.IncludeSpawns=                  ; Nullable<bool>, falls back to [General]
Promote.VeteranAnimation=               ; ValueableVector<AnimTypeClass*>
Promote.EliteAnimation=                 ; ValueableVector<AnimTypeClass*>
```

## Native evidence and hooks

| Address | Hook | Behavior |
| --- | --- | --- |
| `0x6FA07A` | `TechnoClass_AI_PromoteAnim` | Replaces the promotion block's `call 0x750030` (`GetRemainingLevel`) and resumes at `0x6FA07F`, so it runs on every sampled rank change — before the stock sound and elite-flash arms (`Ext/Techno/Hooks.cpp:158-170`). |
| `0x6B7282` | `SpawnManagerClass_AI_PromoteSpawns` | After the update-timer restart (`0x6B7265..0x6B726E`) and before the slot walk (`0x6B7287`): with the owner type's `Promote.IncludeSpawns.Get([General])`, every slot child below the owner's Veterancy takes `Veterancy.Add(owner - child)` (`Ext/Techno/Hooks.Misc.cpp:216-238`). |

`TechnoExt::ShowPromoteAnim` (`Ext/Techno/Body.Visuals.cpp:863-874`) picks the
tier's list — the type's own list when non-empty, otherwise that tier's
`[AudioVisual]` list — and plays one `AnimExt::CreateRandomAnim` entry: the
`RandomRanged(0, size - 1)` draw over the resolved list, an `AnimClass` at
`GetCenterCoords()`, owner house installed and `SetOwnerObject(pThis)`.

The `GetRemainingLevel` codes are `0` Elite, `1` Veteran, `2` Rookie. The
anim hook tests only `== Rank::Veteran` (1); its `else if` therefore fires the
**elite** list for every other tier, including a demotion back to Rookie. The
hook runs whenever the sampled rank differs from the cache and the cache was
already sampled — not on the first sample after construction, and not while
the rank is unchanged.

`Veterancy.Add(owner - child)` is YRpp's inline `VeterancyStruct::Add(double)`:
the difference and the add run under gamemd's ambient 53-bit x87 word
(round-toward-zero), the sum is capped at `[General] VeteranCap`, and the
store narrows to a chopped `f32`. The child's rank cache is untouched, so the
next `TechnoClass::AI` visit announces the crossing through the ordinary
sound/flash path.

## Where VERA20K owns it

- `rules::phobos_promote::PhobosPromote` holds the three type keys and the
  two fallback accessors (`veteran_animation`/`elite_animation` return the
  global slice when the type list is empty); `ObjectType::phobos_promote`
  carries them. The globals live in `rules::phobos_globals::PhobosGlobals`
  (`promote_include_spawns`, `promote_veteran_animation`,
  `promote_elite_animation`).
- `sim::world::techno_ai::veterancy_promotion_step` is the rank-crossing
  owner. It already samples `Veterancy::sample_promotion`; the same step now
  calls `show_promote_anim` when the cache changed and was previously
  sampled, matching the native hook position before the sound event.
  `show_promote_anim` resolves the type/global tier, filters the names through
  `RuleSet::has_anim_type` (the read-time drop a native
  `ValueableVector<AnimTypeClass*>` applies), draws with
  `SimRng::next_range_i32_inclusive`, and spawns through
  `Simulation::spawn_owned_anim` (the existing `CreateRandomAnim` creation
  owner used by `Spawner.RecycleAnim`) at `object_get_coords`.
- `sim::spawn_manager::ai` calls `SpawnHost::promote_spawned_ranks` directly
  after the update-timer restart; `WorldSpawn` implements it over the
  manager's slots and `Veterancy::promote_toward`, the shared 53-bit
  `Add(double)` tail (also used by `accumulate`).

## Phobos quirks kept

- The type animation lists **replace** the global lists per tier; an empty or
  absent type key keeps the global list of that tier. There is no clearing
  token for the `Valueable` global.
- The veteran list plays only at `Rank::Veteran`; the elite list is the
  `else` arm for a Rookie crossing too, and for a Veteran crossing whose
  veteran list is empty. `Promote.EliteAnimation` has no fallback to
  `Promote.VeteranAnimation` in the source, although
  `docs/New-or-Enhanced-Logics.md` says it does; the hook source wins.
- `Promote.IncludeSpawns` is `Nullable` on the type and a `Valueable` global
  with no unset state; a type `no` overrides a global `yes`.
- The spawn lift runs on the manager's AI cadence (after its gate), not every
  frame, and applies to every slot that holds a unit regardless of its state.

## Residuals (trigger · impact · frequency · risk)

- **`GetCenterCoords` centre height.** The anim is placed at `GetCoords`
  (`object_get_coords`) instead of the native centre-height point. Trigger:
  any promote animation. Impact: a small Z offset; same residual as the
  delayed-fire `CenterAnimOnFirer` arm. Frequency: always. Risk: visual-only.
- **Ares build.** The Phobos hook documented here is the non-Ares arm
  (`TechnoClass_AI_PromoteAnim`); the Ares duplicate
  (`Misc/Hooks.Ares.cpp:74`) is out of scope, so `--features ares,phobos`
  reads the keys but plays no promote animation. Trigger: Ares builds with
  the keys set. Impact: no anim. Frequency: Ares only. Risk: visual-only.
- **Single-entry random draw.** `RandomRanged(0, 0)` may draw once natively
  while `SimRng::next_range_i32_inclusive` returns without drawing when the
  bounds are equal. Trigger: a one-element veteran/elite list. Impact: one
  Scenario RNG draw may be missing. Frequency: any single-anim list. Risk:
  determinism only; same residual as `Spawner.RecycleAnim`.
- **Retail/visual parity untested** (no retail `ini/` in this checkout).
