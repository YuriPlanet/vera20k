# AttackMove.*

Phobos refines the native attack move (MegaMission 29) with three keys: two
per-type switches that change what an attack-mover shoots at, and one global
that lifts the "you need a weapon" requirement from the order itself.

Keys: `Ext/TechnoType/Body.cpp:1086-1087` (type), `Ext/Rules/Body.cpp:377-378,423`
(`[General]`). Constructors: `Ext/Rules/Body.h:800-801,853`.
Consumers: `Ext/Techno/Hooks.cpp:1259-1264`,
`Ext/Techno/Hooks.TargetEvaluation.cpp:144-216`.
Official text: `docs/New-or-Enhanced-Logics.md:1562-1600`.

## Keys

| Key | Scope | Native type | Default |
| --- | --- | --- | --- |
| `AttackMove.IgnoreWeaponCheck` | `[General]` | `Valueable<bool>` | `false` |
| `AttackMove.Aggressive` | TechnoType | `Nullable<bool>` | unset → `[General]` (`false`) |
| `AttackMove.UpdateTarget` | TechnoType | `Nullable<bool>` | unset → `[General]` (`false`) |

The two `Nullable` type keys carry no constructor value
(`Ext/TechnoType/Body.h`, read at `Body.cpp:1086-1087`), so an unset key is not
"`false`" — it defers to the `[General]` flag, and `<default>`/`none` clears a
set key back to that fallback.

## `AttackMove.IgnoreWeaponCheck` — `0x00711E90`

`TechnoTypeClass::Can_Attack_Move @ 0x00711E90` answers
`Primary != null && !PreventAttackMove`; Infantry (`0x005247C0`) and Unit
(`0x007473D0`) types jump to it, Aircraft (`0x0041CB60`) and Building
(`0x00465920`) answer false on their own. The hook replaces the **six bytes at
`0x00711E90..0x00711E9A`** — the `Primary != null` test — and resumes at
`0x00711E9A`, so `PreventAttackMove` (`+0x6C8`) and the class answer are
untouched. With the global on, a weaponless Infantry or Unit type accepts the
order; a type that authors `PreventAttackMove=yes` still refuses it, and an
Aircraft or Building still refuses by class.

VERA: `ObjectType::can_attack_move_with` beside the unchanged
`can_attack_move`, reached through `Simulation::techno_can_attack_move`
(`sim::mission::mega_mission`), which reads
`rules.phobos_globals.attack_move_ignore_weapon_check`.

## `AttackMove.Aggressive` — `0x006F85AB`

`TechnoClass::Evaluate_Candidate`'s human-attacker building gate refuses an
enemy building with no weapon **or** no `ThreatPosed=`. The hook at
`0x006F85AB` (the `IsControlledByHuman` test) sends an attack-mover with the
key on straight to `0x006F8604`, i.e. the candidate is accepted and scoring
continues; otherwise it resumes at `0x006F85BA`, the unarmed test.

VERA: the `ScanContext.attack_move_aggressive` flag, computed once per scan
from `scanner.mega_mission.is_attack_move()` and the scanner type's key with
its `[General]` fallback, and consumed as an extra condition on the building
reject in `evaluate_candidate` (`sim::combat::greatest_threat`).

## `AttackMove.UpdateTarget` — `0x4DF3A0`

`FootClass::Arrival_Target_Handler @ 0x004DF3A0` normally keeps the target it
already holds. The hook runs first and, when
`CheckAttackMoveCanResetTarget` (`Ext/Techno/Hooks.TargetEvaluation.cpp:144-176`)
agrees, writes `Target = nullptr`, `HaveAttackMoveTarget = false` and calls
`UpdateGattlingRateDownReset()`. The handler then rescans.

The predicate, in order:

1. no target, or the target **is** the MegaTarget → refuse;
2. the target is not a Techno, or `IsArmed()` → refuse;
3. `TargetingTimer.InProgress()` → refuse;
4. no slot-0 weapon → refuse;
5. `Greatest_Threat(ThreatType::Range, &Location, false)` finds nothing, or
   the pick is the same TechnoType as the held target → refuse;
6. a slot-1 weapon that exists and is `NeverUse=` refuses unless its `Range`
   is at most the slot-0 `Range` (a melee unit's virtual scanner).

VERA: `Simulation::phobos_attack_move_select_new_target` and
`Simulation::phobos_can_reset_attack_move_target` in
`sim::mission::mega_mission`, called from `foot_arrival_target_handler` before
`foot_arrival_step`. The scan is `sim::world::direct_greatest_threat` with
`ScanMission::Guard` (mask 1, `ThreatType::Range`), the same wrapper the
script and aircraft callers use. `InProgress()` maps to
`!passive_scan.timer().due(frame)`, the established "gate still closed" idiom.

## Residuals (trigger · impact · frequency · risk)

- **Computer houses keep the native free pass.** Phobos replaces the
  `IsControlledByHuman` test for *every* player, so an AI attack-mover also
  goes through the unarmed test and is then decided by its own
  `AutoTarget_NoThreatBuildings` / `AutoTargetAI_NoThreatBuildings` globals —
  a separate Phobos feature this port does not have. VERA applies
  `Aggressive` only inside the existing `human` branch, so an AI house's
  attack-mover still auto-targets unarmed buildings for free. Trigger: a
  computer-owned attack-mover facing an unarmed enemy building. Impact: the AI
  is slightly more willing than Phobos. Frequency: every AI attack move past a
  power plant. Risk: low — it matches stock YR, not Phobos.
- **The engineer arm is not skipped.** `0x006F8604` also skips the engineer
  building gate below it (`0x006F860C..8667`); VERA's `Aggressive` only
  bypasses the unarmed reject, so an Engineer that can attack-move
  (`IgnoreWeaponCheck`) still runs its own gate. Trigger: an Engineer with both
  keys set on an attack move. Impact: a divergence from Phobos only in that
  corner. Frequency: negligible. Risk: low.
- **`AnimOffset`-style visual parity does not apply here**, but no retail,
  visual or GPU validation was done: this checkout has no retail `ini/`,
  `config.toml` or `RA2_DIR`, so retail-INI tests skip and every assertion runs
  on synthetic fixtures. Trigger: any run against real RA2 data. Impact: the
  three gates are verified on fixtures only. Frequency: always in this
  workspace. Risk: medium.
- **`AttackMove.Follow` / `.IncludeAir` / `.IfMindControlIsFull` are not
  ported.** They need a follower subsystem (a `GetGuardRange(2)` cell-spread
  scan, a per-Foot temporary follower count and `QueueMission(Area_Guard)`),
  not a change to an existing owner, and the aircraft specialisations
  (`0x6FA68B`, `0x4DF3BA`, `0x4DF42A`) come with them. Trigger: an INI that
  sets those keys. Impact: no follower behaviour at all. Frequency: only with
  those keys authored. Risk: low — the keys are inert rather than wrong.
- **Extra `Greatest_Threat` draw.** `UpdateTarget` runs one extra scan per
  arrival-handler visit when the key is on, and that scan draws the Scenario
  RNG for computer disguise detection and commits the shell state exactly as
  any other scan does. Phobos makes the same call, so parity holds, but this
  is a per-tick cost on a large attack move. Trigger: `UpdateTarget=yes` over
  many units. Impact: one scan per unit per arrival visit. Frequency: only with
  the key. Risk: low.
