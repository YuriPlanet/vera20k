# Phobos berserk damage block (`AllowBerzerkOnAllies`, `BerzerkMission`)

A Psychedelic warhead makes its target berserk instead of wounding it. Stock
YR refuses that outright when the damage's **SourceHouse** is allied with the
target (`TechnoClass::ReceiveDamage` `0x701CD7..0x701CF8`, which returns 0 from
the call's house argument `[esp+0xE0]`).

Phobos deletes that test (`DEFINE_JUMP(LJMP, 0x701CE5, 0x701D0B)`) and puts its
own six-byte hook at `0x701D2E`
(`Ext/Techno/Hooks.ReceiveDamage.cpp:528-539`):

```cpp
if (!RulesExt::Global()->AllowBerzerkOnAllies
    && pThis->Owner->IsAlliedWith(receiveDamageArgs.Attacker
            ? receiveDamageArgs.Attacker->Owner
            : receiveDamageArgs.SourceHouse))
    return DisallowBerzerk;   // 0x701D3E, the same return-0 exit
```

Two things follow, and both are load-bearing:

- `[CombatDamage] AllowBerzerkOnAllies=` (`Ext/Rules/Body.cpp:421`,
  `Valueable<bool>` constructed **no** at `Body.h:339,847`) switches the whole
  allied refusal off: with `yes` an allied Psychedelic hit berserks its target.
- The house compared is the **live attacker's owner first**, and the call's
  SourceHouse only when no attacker object is present. That is the opposite
  priority to the stock test, so a shot whose projectile carries a hostile
  SourceHouse but whose firer is an ally is refused under Phobos and accepted
  under stock.

## The mission the block queues

After the flag and timer write, the same block clears the target (`vt+0x3C8(0)`,
`0x701DA0`) and queues its mission (`push 0; push 0xF; call vt+0x1E8(0xF, 0)` at
`0x701DAE`) — stock always queues Hunt (`0xF`). Phobos replaces those six bytes
with its own pair of calls (`Ext/Techno/Hooks.ReceiveDamage.cpp:543-553`,
returns `0x701DBA`):

```cpp
pThis->SetDestination(0, false);
pThis->QueueMission(RulesExt::Global()->BerzerkMission, false);
```

- `[CombatDamage] BerzerkMission=` (`Ext/Rules/Body.cpp:615`,
  `Valueable<Mission>` constructed **Hunt** at `Body.h:513,1010`) is a
  `[MissionControl]` mission name, read by
  `detail::read<Mission>` (`Utilities/TemplateDef.h:508-526`): a name that
  matches no mission, like a blank value, keeps the previous one.
- `SetDestination(NULL, false)` is the class setter vt+0x480 in mode 0, which
  clears the NavCom but **keeps the NavQueue** — VERA's
  `Simulation::assign_destination_keeping_queue`. Stock has no such call here,
  so a berserk object that was moving keeps its NavCom.

Both halves run before `SuperClass`'s berserk tail returns, and the queued
mission is not commenced in the same frame (`QueueMission(m, false)`).

## VERA owners

- `src/rules/phobos_globals.rs` — `allow_berzerk_on_allies` and
  `berzerk_mission` (the `[CombatDamage]` pass, next to `BerzerkTargeting`).
- `src/sim/combat/receiver_call.rs` — `psychedelic_allied_rejection`, computed
  per feature: Phobos' attacker-first test under `phobos`, the stock SourceHouse
  test otherwise.
- `src/sim/combat/damage/gates.rs` — gate 9 uses that one fact, so the gate
  order and the `psionics_immune` / `target_is_building` terms are untouched.
- `src/sim/combat/world_receiver.rs` — the berserk block calls
  `assign_destination_keeping_queue` (mode 0) and then
  `mission_queue_exact(MissionId::from_known(berzerk_mission), 0, ..)`, so the
  authored mission goes through the same represented MissionClass wrapper stock
  Hunt used.

## Tests

`src/sim/combat/berzerk_alliance_tests.rs` runs the receiver end to end with an
allied firer and a hostile SourceHouse: Phobos refuses, `AllowBerzerkOnAllies=yes`
accepts, and without an attacker the SourceHouse still decides. The stock build
keeps the mirrored test that the attacker's house is never consulted.
`src/sim/combat/berzerk_mission_tests.rs` pins the queued mission (unauthored
Hunt, an authored `Area Guard`, an unknown name keeping the previous value) and
that the target's NavCom is cleared while the hit still lands.
