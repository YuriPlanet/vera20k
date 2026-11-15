# Phobos `AllowBerzerkOnAllies` (Psychedelic ally test)

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

## VERA owners

- `src/rules/phobos_globals.rs` — `allow_berzerk_on_allies` (the `[CombatDamage]`
  pass, next to `BerzerkTargeting`).
- `src/sim/combat/receiver_call.rs` — `psychedelic_allied_rejection`, computed
  per feature: Phobos' attacker-first test under `phobos`, the stock SourceHouse
  test otherwise.
- `src/sim/combat/damage/gates.rs` — gate 9 uses that one fact, so the gate
  order and the `psionics_immune` / `target_is_building` terms are untouched.

## Tests

`src/sim/combat/berzerk_alliance_tests.rs` runs the receiver end to end with an
allied firer and a hostile SourceHouse: Phobos refuses, `AllowBerzerkOnAllies=yes`
accepts, and without an attacker the SourceHouse still decides. The stock build
keeps the mirrored test that the attacker's house is never consulted.
`rules::phobos_globals` pins the key's section and its `no` constructor.

Related but separate: `BerzerkMission=` (the mission the berserk block queues,
Phobos' `0x701DAE` hook) is not in this change — the same hook also adds a
`SetDestination(NULL, false)`, so it is its own chain.
