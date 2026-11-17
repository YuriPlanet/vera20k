# Phobos AutoDeath

A Techno type that destroys itself when a condition holds. The ported arms are
`AutoDeath.Behavior=kill|vanish|sell` with `OnAmmoDepletion`, `AfterDelay`,
`PlayerPowerState`, `PlayerMoney.Max/Min`, `TechnosDontExist` and
`TechnosExist`, and the `AutoDeath.VanishAnimation` list, plus `AllowLimboed`
(kill a limboed object through the scenario-level `AutoDeathObjects` sweep), and
`KillSelf`'s limbo arm; the residuals below list what is still open.

## Keys

| key | section | default | source |
|---|---|---|---|
| `AutoDeath.Behavior` | TechnoType | unset | `Ext/TechnoType/Body.cpp:817` |
| `AutoDeath.AfterDelay` | TechnoType | `0` | `Ext/TechnoType/Body.cpp:825` |
| `AutoDeath.OnAmmoDepletion` | TechnoType | `no` | `Ext/TechnoType/Body.cpp:820` |
| `AutoDeath.PlayerPowerState` | TechnoType | `none` | `Ext/TechnoType/Body.cpp:834` |
| `AutoDeath.PlayerMoney.Max` | TechnoType | `-1` (off) | `Ext/TechnoType/Body.cpp:835` |
| `AutoDeath.PlayerMoney.Min` | TechnoType | `-1` (off) | `Ext/TechnoType/Body.cpp:836` |
| `AutoDeath.TechnosDontExist` | TechnoType | empty | `Ext/TechnoType/Body.cpp:826` |
| `AutoDeath.TechnosDontExist.Any` | TechnoType | `no` | `Ext/TechnoType/Body.cpp:827` |
| `AutoDeath.TechnosDontExist.AllowLimboed` | TechnoType | global | `Ext/TechnoType/Body.cpp:828` |
| `AutoDeath.TechnosDontExist.Houses` | TechnoType | `owner` | `Ext/TechnoType/Body.cpp:829` |
| `AutoDeath.TechnosExist` | TechnoType | empty | `Ext/TechnoType/Body.cpp:830` |
| `AutoDeath.TechnosExist.Any` | TechnoType | `yes` | `Ext/TechnoType/Body.cpp:831` |
| `AutoDeath.TechnosExist.AllowLimboed` | TechnoType | global | `Ext/TechnoType/Body.cpp:832` |
| `AutoDeath.TechnosExist.Houses` | TechnoType | `owner` | `Ext/TechnoType/Body.cpp:833` |
| `AutoDeath.OnOwnerChange` | TechnoType | `no` | `Ext/TechnoType/Body.cpp:821` |
| `AutoDeath.OnOwnerChange.HumanToComputer` | TechnoType | master | `Ext/TechnoType/Body.cpp:823` |
| `AutoDeath.OnOwnerChange.ComputerToHuman` | TechnoType | master | `Ext/TechnoType/Body.cpp:824` |
| `AutoDeath.OnOwnerChange.IgnoreRevertOnExit` | TechnoType | global | `Ext/TechnoType/Body.cpp:822` |
| `AutoDeath.AllowLimboed` | TechnoType | global | `Ext/TechnoType/Body.cpp:818` |
| `AutoDeath.VanishAnimation` | TechnoType | empty | `Ext/TechnoType/Body.cpp:819` |

The two `AllowLimboed` keys fall back to `[CombatDamage]
AutoDeath.TechnosDontExist.AllowLimboed` / `AutoDeath.TechnosExist.AllowLimboed`
(`Ext/Rules/Body.cpp:631-632`), both `Valueable<bool>` defaulting to `no` and
read once from the merged layer state.

`AutoDeath.PlayerPowerState` takes `none`, `low`/`consumer` or `full`/`normal`
(`Utilities/TemplateDef.h:1762-1781`). The two money bounds are inclusive and
`-1` leaves that end open.

`AutoDeath.Behavior` is a `Nullable<AutoDeathBehavior>` (`Utilities/Enum.h:196`):
`kill` (0), `sell` (2) or `vanish` (1), matched with `_strcmpi`
(`Utilities/TemplateDef.h:1063-1083`). Absent, blank or unrecognised leaves the
`Nullable` unset, and `TechnoExt::CheckDeathConditions`
(`Ext/Techno/Body.Update.cpp:142`) returns at its first test — so
`AutoDeath.AfterDelay` alone arms nothing.

`AutoDeath.AfterDelay` is a `Valueable<int>` default `0`; a non-positive value
never starts the countdown (`:173`).

## Behavior

The check runs from `TechnoExt::OnEarlyUpdate` (`:26`), called at
`TechnoClass_AI @ 0x6F9E50+0x5` after `ApplyMindControlRangeLimit` and before
`ApplyInterceptor`. On the first visit it starts `AutoDeathTimer` with
`AfterDelay`; once the `CDTimerClass` completes it calls `KillSelf`.

`if (this->CheckDeathConditions()) return;` (`:26`) is an early exit for
`ApplyInterceptor` (`:29-30`, ported at `sim/world/techno_ai/interceptor.rs`):
the visit that killed its object never reaches the interceptor scan, and
`sim/world/techno_ai.rs` calls `apply_interceptor` only when
`check_auto_death` reports no self-kill. Every arm returning `true`
(`:162`, `:169`, `:182`, `:219`, `:232`, `:243`, `:254`) has just called
`KillSelf` on the line above, so the skipped scan only covers an object that is already gone
(`Kill`/`Vanish`) or one whose `Selling` mission `ApplyInterceptor`'s own
`Selling`/`Construction` guard (`:60-61`) refuses anyway. The limbo sweep below
never runs `ApplyInterceptor` at all, matching native, which only calls
`CheckDeathConditions(true)` from `UpdateAutoDeathObjectsInLimbo`.

Phobos tests `HasStarted()` before `Completed()`, which matters: an unstarted
`CDTimerClass` reads as already completed. VERA keeps that order by comparing
`start_frame()` with `PAUSED_START_FRAME`.

`KillSelf`'s non-limbo dispatch (`:769-824`) selects the arm from
`AutoDeath.Behavior`. The `Kill` arm (`:809-821`) deals the object's own Health
as damage with `[CombatDamage] C4Warhead=`, no attacker and no source house,
ignoring defenses: `ReceiveDamage(&pThis->Health, 0, C4Warhead, nullptr, true,
false, nullptr)`. VERA routes that through the shared direct-receiver owner, so
the object dies through the ordinary damage and death path.

The `Vanish` arm (`:771-790`) first plays a random
`AutoDeath.VanishAnimation` at `pThis->GetCoords()` with the object's house as
the animation owner (the list is `ValueableVector<AnimTypeClass*>`, so an
unresolvable name never entered it and the pick covers only loaded types). The
call at `:773` is
`AnimExt::CreateRandomAnim(pVanishAnimation, pThis->GetCoords(), pThis,
nullptr, true)` — five arguments, so `invoker` is true while `ownedObject`
keeps its `Ext/Anim/Body.h:122` default `false`. The animation therefore
records the dying object as its `Invoker`/`InvokerHouse` and its house as
owner house, and receives **no** owner object: `AnimClass::SetOwnerObject @
0x00424B50` is called only under `if (ownedObject)` (`Ext/Anim/Body.cpp`,
`:420-421`). The arm then
unloads a bunker the building holds (`BuildingClass::UnloadBunker @
0x004593A0`, the sell/death release that does not reposition the unit or queue
a Move), kills its passengers with the object as
their killer, stuns it, limboes it, registers the kill under its own owner
(`RegisterKill(pThis->Owner)`) and uninitialises it — no death animation,
corpse or C4 damage. The `Sell` arm (`:792-807`) sells a Building whose Buildup
SHP is bound (`+0x6E9`, `rules.has_buildup`) by queueing and commencing the
Selling mission unless it is already selling; anything else — a non-building or
a type without a Buildup SHP — falls through to the `Kill` arm, as the native
developer warning does.

A limboed object — an Ares initial payload, or a passenger a building absorbed —
is not in the Logic vector, so the ordinary AI walk never reaches it. Phobos
instead keeps `ScenarioExt::AutoDeathObjects`
(`Ext/Scenario/Body.cpp:101-134`) and visits every entry that is not in Logic but
still alive, once per frame, from `LogicClass_Update_BeforeAll`
(`DEFINE_HOOK(0x55B4E1, …, 0x5)` at `:293-301`), ahead of the transport reloaders
and before the object pass. VERA sweeps the same way from the `BombList` step,
which already owns that hook. Only three Ares admissions register: the Unit and
Aircraft payload (`Misc/Hooks.Ares.cpp:31-36`), the building payload (`:38-42`)
and the last occupant of a placed building (`:44-57`). Ordinary boarding, a
Chrono limbo and a sell/repair limbo never register, so those objects keep no
`AutoDeath` visits at all — `AutoDeath.AllowLimboed` is not a permission to be
visited, only whether a visit may kill. VERA registers at its one Ares payload
owner, which covers the first two; its map loader authors no building occupants,
so the third has no separate admission.

`KillSelf`'s limbo arm (`:721-766`) ignores `AutoDeath.Behavior=`: `isInLimbo`
returns from that arm before the `kill`/`vanish`/`sell` dispatch, so every
behavior takes it. In order it exits a parasite from its victim, drops a limboed
building's `AddToLimboTracking` entry (its `!Insignificant && !DontScore` gate
means the type never entered the count), dirties the transporter owner's power
when the object is `Absorbed` (Techno `+0x439`), registers the kill as
`RegisterKill(pThis->Owner)` — the owner house, not an attacking Techno —
uninits, and then settles a `Gunner=` transport: `RemoveGunner(nullptr)` followed
by `ReceiveGunner` on the **last** remaining passenger (native walks the cargo
list to its tail, where `Detonate.cpp:887-913` takes the head). Nothing is
played, stunned, limboed again or sold: no vanish animation, no corpse, no C4 hit.

The low-power arm is written
`(Full && !isLowPower) || (Low && isLowPower) && !isFirstFrame`. `&&` binds
tighter than `||`, so the first-frame guard applies to the `Low` arm only and a
`Full` type also dies on frame 0. That is reproduced as written.

The presence arms (`Ext/Techno/Body.Update.cpp:186-255`) scan the selected
houses for each listed type. Without `AllowLimboed` they read
`HouseClass::CountOwnedAndPresent` through VERA's on-map `+0x5550` family; with
it, the distinct limbo count is added. RESIDUAL: the wrapper's body was not read
instruction-level, and `docs/phobos-harvester-counter.md` records `+0x5500` as
the same query, so the counter identity is unproven. `Houses=` selects the house
set through `CanTargetHouse`, `owner` short-circuiting to the object's own
house. `TechnosDontExist` passes `!Any` into the scan, so its default `Any=no`
means "kill when none of the listed types exists" and `Any=yes` means "kill
unless all exist". `TechnosExist` uses `Any` directly.

The lists are `ValueableVector<TechnoTypeClass*>`: tokens arrive untrimmed, as
`strtok(",")` yields them, and a token that names no type is dropped when the
list is resolved (`Utilities/TemplateDef.h:1727-1742`). An all-unknown list
therefore leaves the arm's emptiness guard in charge instead of flipping the
decision — a typo keeps the object alive rather than killing it.

An owner change sets `ShouldBeDead` at
`TechnoClass_Captured_UpdateTracking @ 0x7015C9`
(`Ext/House/Hooks.cpp:231-259`), and the next `CheckDeathConditions` visit kills
on it (`Ext/Techno/Body.Update.cpp:159-163`). The master `OnOwnerChange=` arms
both directions unless each names its own; a change between a human and a
computer house takes the matching one. A transported object is spared only when
`AllowLimboed` is turned off — the `[CombatDamage]` global constructs `yes`
(`Ext/Rules/Body.h:1027`).

A type change through `TechnoClass::ChangeType` calls
`FootExt::UpdateTypeData` (`Ext/Foot/Body.cpp:219-221`), which stops a running
countdown when the new type has no `AfterDelay`. Without that reset a round trip
through an untimed type would return the unit with an already-completed clock
and kill it on its first timed visit. VERA reaches `ChangeType` from the Ares
`Convert.Water=`/`Convert.Land=` surface conversion, Ares `Convert.Deploy=`
(`sim/unit_simple_deploy.rs`) and the team-script conversion action
(`sim/team_script_vm/actions.rs`); all of them pass through the one owner, so
every conversion drops a stale countdown.

`GetValuesForDisplay` (`Ext/Techno/Body.Visuals.cpp:696-717`) reports the clock's
remaining time over its full duration for `DigitalDisplay` entries of type
`AutoDeath`; a type without `AutoDeath.Behavior=` draws nothing, and an unstarted
clock has a zero maximum, which the renderer skips.

## Residuals

- `FootExt::IsOwnerChangeFromRevertOnExit` is unported — the AttachEffect
  discard chain also reads it as `false` — so `IgnoreRevertOnExit` cannot fire
  and the owner-change transporter gate has no revert term.
- Ares `SpawnSurvivors` before the fatal hit (`Ext/Techno/Body.Update.cpp:810-816`)
  is not ported. The hit ignores defenses, and VERA's death path then refuses its
  own survivor release and marks the transport handled, so an `AutoDeath`-killed
  `Crewed=`/driver Unit or Aircraft loses its crew where native ejects it.
  Reproducing Phobos' pre-call needs VERA's survivor owner taught the
  pre-damage context (it currently assumes the unit is already lifted), which is
  its own chain.
