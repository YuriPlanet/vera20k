# Auto-target own position

`AutoTargetOwnPosition=` makes a Techno's auto-target scan hold itself (or its
cell) as its target instead of searching for an enemy.

```ini
[SOMEUNIT]                          ; TechnoType
AutoTargetOwnPosition=false         ; Valueable<bool>, alias AutoFire=
AutoTargetOwnPosition.Self=false    ; Valueable<bool>, alias AutoFire.TargetSelf=
```

Readers: `Ext/TechnoType/Body.cpp:896-905`; constructor defaults `false`
(`Body.h:561-562`). The deprecated aliases are read first and the canonical
keys override them (`:898-899`, `:904-905`).

## Native body

`TechnoClass::Retaliate_And_Scan @ 0x00709820` arms the targeting timer and
draws `RandomRanged(0, 2)` (`0x0070982E..0x007098B6`). Phobos
`TechnoClass_TargetSomethingNearby_AutoFire` (`0x7098B9`,
`Ext/Techno/Hooks.TargetEvaluation.cpp:8-22`) replaces the first six bytes of
the held-target step: a type with `AutoTargetOwnPosition` calls
`SetTarget(pThis)` when `.Self=` is set and `SetTarget(pThis->GetCell())`
otherwise, then returns at `0x7099B8` without scanning.

`TechnoClass::Set_Target @ 0x006FCDB0` converts a request naming the receiver
itself into the cell under its location (`0x006FCEB8`), so both arms store a
cell target in this engine; the flags still decide whether the hook runs and
which request is passed.

## VERA

`ObjectType::phobos_auto_target_own_position` and
`phobos_auto_target_own_position_self` read the keys. The scan host's
`phobos_auto_target` answers the request in
`sim/world/techno_ai/target_scan.rs::retaliate_and_scan` after the timer
arming; the target is committed through the production
`assign_target_represented` owner, which performs the same self-to-cell
conversion.

## Residuals

- Only `TechnoClass::Retaliate_And_Scan` is hooked; other Greatest_Threat
  callers (mission handlers, the TargetSomethingNearby sites outside this
  function) keep their native scans.
- The `.Self` request goes through the production target setter, which
  converts a self target with the raw Location cell (`0x6FCEB8` models this
  with `position.rx/ry`). Native's conversion uses `MapClass::GetCellAt`,
  which stamps the shared Dummy for out-of-map coordinates and differs from
  the raw cell inside a tube. Trigger: an off-map or tunnelled self-target.
  Frequency: pathological placements. Risk: a one-cell target difference
  between the two arms.
- Retail rules author neither key; the default reproduces the stock scan.
