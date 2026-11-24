# AttackMove.Follow.*

The follower half of Phobos' attack-move family: an attack-mover can give up
its own destination and escort a nearby friendly attack-mover instead.

Keys: `Ext/TechnoType/Body.cpp:1149-1151`. Fields and constructor:
`Ext/TechnoType/Body.h:383-385`, `:805-807`. Consumer:
`Ext/Techno/Hooks.cpp:1266-1358` (hook `0x4DF3A6`), plus the per-frame
decrement at `:50-51` (hook `0x4DA54E`). Per-object state:
`src/Ext/Foot/Body.h:26,44`, serialized at `src/Ext/Foot/Body.cpp:951`.
Official text: `docs/New-or-Enhanced-Logics.md` (`AttackMove.Follow`).

## Keys

| Key | Scope | Native type | Default |
| --- | --- | --- | --- |
| `AttackMove.Follow` | TechnoType | `Valueable<bool>` | `false` |
| `AttackMove.Follow.IncludeAir` | TechnoType | `Valueable<bool>` | `false` |
| `AttackMove.Follow.IfMindControlIsFull` | TechnoType | `Valueable<bool>` | `false` |

All three are plain `Valueable<bool>`: no `[General]` counterpart, no unset
state. `IncludeAir` alone does nothing — it only widens the search the other two
trigger.

## Native control flow — `0x4DF3A6`

`Arrival_Target_Handler @ 0x004DF3A0` (vt+0x4CC) begins
`sub esp,0xC; push esi; mov esi,ecx` (six bytes, `0x4DF3A0..0x4DF3A6`), then
`mov eax,[esi+0x5C8]` (MegaDestination) at `0x4DF3A6`. Phobos hooks that
second slot and jumps to the handler's epilogue `0x4DF425` when the follow arm
runs. The hook therefore sees the very start of the arrival decision, after
Phobos' own `AttackMove.UpdateTarget` hook at `0x4DF3A0`.

Two halves:

1. **Mission repair, unconditional** (`:1273-1295`). `GetCurrentMission()` not
   Attack and not Move → `ContinueMegaMission()`, except a `UnitClass` whose
   type is `IsSimpleDeployer=` while on Mission::Unload. This runs for every
   attack-mover, follow or not.
2. **Follow search, conditional** (`:1297-1357`), entered when
   `AttackMove.Follow=` is on, or `.Follow.IfMindControlIsFull=` is on and
   `CaptureManager->CannotControlAnyMore()`.

The search:

- `getCellSpreadItems(GetCoords(), GetGuardRange(2) / 256.0, IncludeAir)` —
  Phobos' `Helpers::Alex::getCellSpreadItemsExt` alias of Ares' walk, returning
  Technos in UniqueID order.
- `pMegaMissionTarget = MegaDestination ? MegaDestination : (MegaTarget ?
  MegaTarget : pThis)`.
- Keep a candidate iff it is a Foot, not `pThis`, same `Owner`,
  `MegaMissionIsAttackMove()`, its `AttackMoveFollowerTempCount < 6`, and its
  type does **not** set `AttackMove.Follow` (a follower is never a leader).
- Nearest candidate by `DistanceFrom(pMegaMissionTarget)`; integer lepton
  distance, strict `<`, initial cutoff 65536.
- Found: charge the leader `+1` (Infantry) or `+3` (anything else), then
  `SetDestination(leader, false)`, `SetArchiveTarget(leader)`,
  `QueueMission(Area_Guard, true)`.
- Not found: `SetDestination(MegaTarget ? MegaTarget : MegaDestination, false)`.
- Both arms: `ClearMegaMissionData()`, answer the leader pointer in EAX and
  return through `0x4DF425` — the normal arrival body never runs.

`AttackMoveFollowerTempCount` is transient: `FootClass_AI`'s tail decrements a
non-zero value by one every frame (`0x4DA54E`), so a leader stops being
"surrounded" shortly after its escorts leave.

## VERA

- Rules: `PhobosAttackMove::{follow, follow_include_air,
  follow_if_mind_control_is_full}` in `src/rules/phobos_attack_move.rs`, read by
  `read_bool` (`Valueable<bool>`).
- State: `GameEntity::phobos_attack_move_follower_temp_count`, aged in
  `object_ai_visit_one_with_effects` right after `techno_ai_shell` for every
  non-Structure category.
- Behaviour: `Simulation::phobos_attack_move_follow`, called from
  `foot_arrival_target_handler` immediately after
  `phobos_attack_move_select_new_target`; `true` returns before
  `foot_arrival_step`, mirroring the jump to `0x4DF425`.
- Search: `Simulation::phobos_capture_cannot_control_any_more` (`IsFull @
  0x004722A0` plus Phobos' size-summed count), `threat_range::threat_range_leptons(obj, 2, …)`
  for `GetGuardRange(2)` (Patrol mode: double, floor 1792, cap 4096 leptons),
  and `combat::cell_spread_technos_with_air` (the `getCellSpreadItemsExt` walk
  already used by airburst, now with the `includeInAir` switch exposed).
- Hand-off: `assign_destination_represented` (vt+0x480),
  `GameEntity::set_archive_target`, `foot_queue_mission_now(AreaGuard)`, then
  `foot_clear_all_tarcom` (`Clear_All_TarCom @ 0x004DF1A0`, the same four-field
  clear as `ClearMegaMissionData`).

## Residuals (trigger · impact · frequency · risk)

- **`GetGuardRange(2)` lacks Phobos' `AreaGuardRange` / `MaxGuardRange`.**
  TechnoType `AreaGuardRange=` (an Area Guard override) and `MaxGuardRange=`
  are separate Phobos keys that this tree does not read yet; VERA's
  `threat_range_leptons` computes the same double/floor/cap from `GuardRange=`
  and the live weapon ranges. Trigger: an INI that authors those two keys.
  Impact: the follower search radius follows stock YR instead of the authored
  override. Frequency: only with those keys. Risk: low.
- **The decrement runs after the whole `techno_ai_shell`, not at
  `FootClass::AI`'s own tail.** A Temporal-frozen object leaves the shell early
  but still ages its counter, and the exact frame an escort's charge expires can
  differ by one. Trigger: a follower whose leader is frozen or destroyed on the
  same frame. Impact: one frame of leader capacity. Frequency: rare. Risk: low.
- **No return value.** Native hands the leader pointer back in EAX to
  `Arrival_Target_Handler`'s caller; VERA's handler returns `()`. The caller
  only tests AL, so nothing observable changes, but a future caller that wanted
  the pointer would not get it.
- **`DistanceFrom` is the shared `Sqrt_Approx` length, not a bespoke
  integer square root.** VERA uses `native_x87::sqrt_approx_length` with the
  `(dx² + dz²) + dy²` order the other distance sites use. Ties or a one-lepton
  difference can pick the other candidate than retail; the ordering key is the
  same and the cutoff (65536) is far from any realistic tie.
- **`IncludeAir` depends on the reconstructed AircraftTracker.** The airborne
  arm reads `EntityStore::airborne_in_query_order`, the same projection Ares
  area damage uses, including Phobos' `×0.25` squared-distance discount for
  Aircraft. A Unit parked off the cell grid but not registered with the tracker
  is not found, matching the tracker's own membership.
- **No retail validation.** This checkout has no retail `ini/`, `config.toml`
  or `RA2_DIR`, so every assertion runs on synthetic fixtures. Trigger: any run
  against real RA2 data. Impact: key reading and the hand-off are verified on
  fixtures only. Frequency: always in this workspace. Risk: medium.
- **The aircraft specialisations are not here.** `0x6FA68B`, `0x4DF3BA` and
  `0x4DF42A` belong to `ExtendedAircraftMissions` (`[General]` +
  `AircraftType`), not to `AttackMove.Follow.*`, and need the extended-mission
  subsystem. Trigger: an INI using that separate feature. Impact: the keys are
  inert rather than wrong. Frequency: only with those keys. Risk: low.
