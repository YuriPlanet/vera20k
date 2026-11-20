# DelayedFire.* (Phobos)

`DelayedFire.*` gives a weapon a wind-up delay before each shot: `FireAt` is
suppressed for a number of game frames, a wind-up animation is spawned, and the
firing-sequence frame can be frozen, before the shot is finally discharged.
This is the Phobos `WeaponTypeExt::DelayedFire_*` family from
`src/Ext/WeaponType/Body.cpp:189-199`, driven by the
`TechnoClass_FireAt_BeforeTruelyFire` hook at `0x6FDDC0`
(`src/Ext/Techno/Hooks.Firing.cpp:638-693`). Reference:
`docs/New-or-Enhanced-Logics.md:3670-3700`.

> This is **not** the stock `IsAnimDelayedFire` prism-tower mechanism
> (`src/sim/world/techno_ai/building_missions.rs`, `src/rules/art_data.rs`).
> All new VERA symbols are prefixed `phobos_delayed_fire_*`.

## Important premise correction: the mechanism has 11 keys, not 9

The task's 9 keys are `Body.cpp:190-196` plus `:198-199`. Two were omitted:

- `Body.cpp:197` — `DelayedFire.OnlyOnInitialBurst` (the ninth listed key
  `PauseFiringSequence` is `:196`; `:197` was skipped).
- `Body.cpp:189` — `DelayedFire.Duration`, the delay length itself.

`Duration` and `OnlyOnInitialBurst` were already read by VERA, but **only for
the `Burst.NoDelay` compatibility gate** (`WeaponType::delayed_fire_allows_burst_repeat`,
consumed at `src/sim/combat/world_fire.rs:99-110`); the base mechanism (timer,
pause, animation family) was entirely missing. They are now wired into the real
mechanism too, and the `Burst.NoDelay` gate is preserved — its regression test
(`burst_no_delay_tests.rs:206`) was adapted because a set `Duration` now
genuinely delays the first shot.

## Keys

| Key | Type | Default | VERA representation |
|---|---|---|---|
| `DelayedFire.Duration` | `Nullable<PartialVector2D<int>>` (single value or `min,max` random interval, **game frames**) | unset | `Option<PartialIntPair>` |
| `DelayedFire.SkipInTransport` | `Valueable<bool>` | `false` | `bool` |
| `DelayedFire.Animation` | `Valueable<AnimTypeClass*>` (blank sets null; a non-type token such as `none` fails the native parse and keeps the prior value — VERA's single merged read starts from the constructor null either way) | `null` | `Option<String>` |
| `DelayedFire.OpenToppedAnimation` | `Nullable<AnimTypeClass*>` (clearable with `none`/`<none>`/`<default>`) | unset | `Option<String>` (`None` = unset/fall back) |
| `DelayedFire.AnimIsAttached` | `Valueable<bool>` | `true` | `bool` |
| `DelayedFire.CenterAnimOnFirer` | `Valueable<bool>` | `false` | `bool` |
| `DelayedFire.RemoveAnimOnNoDelay` | `Valueable<bool>` | `false` | `bool` |
| `DelayedFire.PauseFiringSequence` | `Valueable<bool>` | `false` | `bool` |
| `DelayedFire.OnlyOnInitialBurst` | `Valueable<bool>` | `false` | `bool` |
| `DelayedFire.AnimOffset` | `Nullable<CoordStruct>` (`Forward,Lateral,Height`, **replaces** weapon FLH) | unset | `Option<[i32;3]>` |
| `DelayedFire.AnimOnTurret` | `Valueable<bool>` | `true` | `bool` |

Readers are all from `src/rules/ini_value.rs` (`read_ares_partial_int_pair`,
`read_ares_bool`, `read_type_name`, `read_phobos_nullable_with`,
`read_int_fields_complete::<3>`); no `str::parse` is used. `Valueable<bool>`
keeps the constructor default on absent/invalid reads. The two animation names
are `Option<String>` resolved through `StringInterner` at spawn time; the
`Nullable` sibling distinguishes "unset" (fall back to `Animation`) from a
resolved name, and its `none` tokens clear to unset.

## Mechanism chain (native evidence)

Instance state (4 fields) lives in `src/Ext/Techno/Body.h:60-63,120-123`,
serialized at `Body.cpp:1183-1186`: `DelayedFireSequencePaused`,
`DelayedFireWeaponIndex`, `DelayedFireTimer`, `CurrentDelayedFireAnim`.

1. **Main gate** — `DEFINE_HOOK(0x6FDDC0, TechnoClass_FireAt_BeforeTruelyFire, 0x6)`
   `Hooks.Firing.cpp:638-693` (`SkipFiring = 0x6FDE03`). The hook sits inside
   `TechnoClass::Fire_At @ 0x006FDD50`, after the class fire-error admission
   and Fire_At's own InLimbo refusal, so every Fire_At caller (unit, infantry,
   aircraft, building and the Direct visit) passes it; VERA runs it in
   `emit_admitted_fire`. Weapon change → `ResetDelayedFireTimer()`. If
   `Duration.isset() && (!Transporter || !SkipInTransport)` and
   `Burst<=1 || !OnlyOnInitialBurst || CurrentBurstIndex==0`: `InProgress` →
   suppress; `!HasStarted` → record index, start timer, pick anim
   (`Animation`, or `isset` `OpenToppedAnimation` in a transport),
   `AnimOffset` overrides FLH, `CreateDelayedFireAnim`, suppress; `Completed`
   → reset, fire next frame. Infantry / turretless SHP Units with
   `PauseFiringSequence` yield to the class pause hooks.
2. **Pause-sequence path** — `TechnoExt::HandleDelayedFireWithPauseSequence`
   `Body.cpp:699-750`, callers `Unit/Hooks.Firing.cpp:89,103` (hook `0x736F61`)
   and `Infantry/Hooks.Firing.cpp:118` (hook `0x5209AF`).
3. **"Pause" = freeze the firing-sequence frame, not ROF** — `UnitClass_AI`
   hook `0x736480` (`Hooks.cpp:60-82`) does not decrement `CurrentFiringFrame`
   (`animation::tick_unit_firing_counter`); `TechnoClass_AI_AnimationPaused`
   hook `0x6FABC4` (`Hooks.cpp:1195-1207`) skips the generic Stage block
   (`techno_ai::techno_common_post` keeps the flash/particle/EMP tail).
   `Fire`/`RearmTimer` are never reached during the delay.
4. **Anim creation** — `TechnoExt::CreateDelayedFireAnim` `Body.cpp:671-697`:
   `CenterAnimOnFirer` → `GetCenterCoords()`; else
   `GetFLHAbsoluteCoords(pThis, firingCoords, AnimOnTurret)`
   (`Body.Internal.cpp:80-89`); `GameCreate<AnimClass>`; `AnimIsAttached`
   only → `SetOwnerObject @ 0x00424B50` + register `CurrentDelayedFireAnim` +
   store `RemoveAnimOnNoDelay` on the AnimExt. **When `Attach=false`,
   `RemoveAnimOnNoDelay` is completely inert.**
5. **Removal** — `ResetDelayedFireTimer` `Body.cpp:658-669`: only when
   `RemoveAnimOnNoDelay` is set does it `UnInit()` the tracked anim. Callers:
   weapon change, timer completion, target→null (`0x6FCF3E`
   `Hooks.cpp:1175`). `TechnoClass_DetachAnim` hook `0x710415`
   (`Body.cpp:1256-1267`) clears the pointer without destroying (non-owning
   raw pointer).
6. **Determinism** — a ranged `Duration` uses
   `ScenarioClass::Random.RandomRanged` (`GeneralUtils.cpp:61-64`); VERA uses
   `Simulation::scenario_rng`, whose state is folded into `world_hash`.

Other cited addresses: `0x736F67` / `0x5209EE` (`Burst.NoDelay` compatibility
gate, `!Duration.isset() || OnlyOnInitialBurst`), `0x6F7248`
(`TechnoClass_InRange_WeaponRange`, `ExtraRange.Prefiring` uses
`timer.InProgress()`), `0x6FC5C7` (`TechnoClass_CanFire_OpenTopped`).

## VERA implementation scope ("base mechanism implemented together")

All 11 keys are read. The production `FireVisit::UnitTarget` →
`admit_attacker_fire` path carries the `0x6FDDC0` gate (returns `None` =
SkipFiring) after weapon selection and before `GetFireError`. `GameEntity`
holds the phobos-gated timer (absolute start frame + duration), weapon index,
pause flag and tracked anim id; only non-default state enters `world_hash`
(`b"phobos-delayed-fire-v1"`, so untouched saves keep their old hash).
`assign_target_represented` resets the timer on a lost target. The ranged
`Duration` draws from `scenario_rng`. The wind-up anim uses
`spawn_owned_anim` (`spawn_anim_at_world` + `descriptor.owner`) and is
force-destroyed with `destroy_anim` on reset when attached. `tick_unit_firing_counter`
holds the firing frame while paused. The `Burst.NoDelay` compatibility gate is
unchanged.

Wind-up anim placement now covers the remaining native inputs:
`DelayedFire.AnimOffset` is an FLH **override** (`FireSource::flh_override`), not
an added base, so `0x6FDDC0` → `GetFLHAbsoluteCoords` sees the anim offset where
the weapon FLH would be and takes the same Y-mirror / burst-parity treatment
(`Ext/Techno/Body.cpp:727-733`, `Body.Internal.cpp:80-89`);
`DelayedFire.AnimOnTurret` selects the turret or hull basis through
`FireSource::on_turret` (`Body.cpp:680`, `Body.Internal.cpp:54`). The
`PauseFiringSequence` freeze is set only at the firing-frame boundary and only
for Infantry and turretless non-Voxel SHP Units (`Body.cpp:711-716`,
`Hooks.Firing.cpp:89,103`, `Ext/Infantry/Hooks.Firing.cpp:118`); a turreted or
Voxel Unit keeps its stock Stage/counter cadence while the shot is delayed.
`GameEntity::attached_anim_expired` clears `CurrentDelayedFireAnim` when that
exact anim expires or detaches (`TechnoClass_DetachAnim @ 0x710415`,
`Body.cpp:1256-1267`), without destroying the non-owned pointer again.

## Residuals (not implemented / compromises)

Every entry lists trigger conditions, impact, frequency and risk. There was **no
retail `ini/`, `config.toml` or `RA2_DIR`** on this machine, so retail-INI tests
skipped: **no retail-data or visual/GPU verification was performed**. Only
synthetic-INI, headless production-path tests were run.

- **`CenterAnimOnFirer` uses `GetCoords`, not `GetCenterCoords`.**
  Trigger: `DelayedFire.CenterAnimOnFirer=yes`. Impact: the anim sits at the
  object coordinate without the native centre-height offset. Frequency: only
  that key. Risk: visual-only.
- **`ExtraRange.Prefiring` integration** (`0x6F7248` `Hooks.WeaponRange.cpp:84`,
  uses `timer.InProgress()` to extend weapon range during the delay) is not
  implemented. Trigger: that key on a delayed-fire weapon. Impact: no extra
  pre-firing range. Frequency: only that combination. Risk: low for the base
  mechanism; a separate feature chain.
- **Retail/visual parity untested.** Trigger: any real-map run. Impact: unknown
  visual fidelity of the wind-up anim. Frequency: always in real games. Risk:
  visual-only; mitigated by the headless production-path tests.

## Tests

`src/sim/combat/phobos_delayed_fire_tests.rs` (phobos-only), all through
`Simulation::commit_fire_visit` → `admit_attacker_fire`:

1. A 4-frame delay holds the shot, spawns one attached anim, freezes
   `UnitClass+6C0`, keeps suppressing on `InProgress` frames, then fires on
   `Completed` and force-destroys the anim (`RemoveAnimOnNoDelay`).
2. `AnimIsAttached=no`: the unattached anim is created but survives the reset
   (the `RemoveAnimOnNoDelay` boundary).
3. `SkipInTransport` on an open-topped rider: with the key the shot fires on
   frame 1 and no anim/timer appears; without it the same rider is delayed.
4. A shot refused before `Fire_At` (still rearming) starts no timer and
   spawns no wind-up anim; the first admitted shot starts the delay at its own
   frame.
5. `AnimOffset` with the weapon FLH at zero places the anim at the override's
   matrix translation, and `AnimOnTurret=no` uses the hull basis on a turreted
   unit (body facing, no `TurretOffset`).
6. A turreted or Voxel Unit with `PauseFiringSequence=yes` still delays its
   shot, spawns the anim and advances `UnitClass+6C0` (no freeze), while the
   accepted Infantry path freezes at the firing-frame boundary.
7. Destroying the wind-up anim on its own clears the firer's tracked id; the
   subsequent reset returns no id and discharges normally without a second
   destroy.

`burst_no_delay_tests.rs:206` keeps the `Burst.NoDelay` compatibility gate
regression (adapted for the now-real delay).

## Falsification (production-path check, recorded step by step)

Run on `feature/delayed-fire` before the commit, all via the production
`commit_fire_visit` → `admit_attacker_fire` path:

1. **Break**: in `phobos_delayed_fire_suppresses_this_frame`, the
   `!timer.HasStarted()` branch's `true` (suppress this frame) was changed to
   `false`, so the first delayed FireAt would launch immediately.
2. **Red**: `CARGO_TARGET_DIR=D:/vera20k-build py -m tools.cargo_run -- test -p
   vera20k-core --lib phobos_delayed_fire` → `0 passed; 3
   failed`. All three tests panicked on the frame-100 assertion
   (`left: 1, right: 0`): the shot fired on the very frame the delay should
   have suppressed it.
3. **Restore**: the branch's return value was changed back to `true`; `git diff`
   confirmed the file matched the verified implementation again.
4. **Green**: re-running the same phobos filter after `rustfmt` (leaf files
   only) → `3 passed; 0 failed`.

The rest of the engine was unaffected (the new logic reads Phobos keys only),
and the `Burst.NoDelay` compatibility-gate test stayed green in all three
feature sets throughout.
