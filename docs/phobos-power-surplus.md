# Optional Phobos AI spare power

These keys make a computer house keep some power spare when it plans its base.
They work in every build: either Phobos key in the rules, mode or map turns
Phobos on for the game ([the Phobos switch](phobos.md#the-phobos-switch)), and
without Phobos on a computer house keeps no spare power, as in the original.

```ini
[AI]
EnablePowerSurplus=false          ; boolean
PowerSurplus=50                   ; integer, the original's own key
PowerSurplus.ScaleToDrainAmount=0 ; integer
```

Before a computer house builds something that drains power, it checks that its
output covers its drain plus the new building's; when it does not, it builds a
power plant first. The original takes a spare amount off the output in that
check, but leaves it at zero, and never uses its own `[AI] PowerSurplus=`
(retail sets 50). With `EnablePowerSurplus=yes` the spare amount is
`PowerSurplus`, so the house builds plants sooner and runs short of power less
often.

With `PowerSurplus.ScaleToDrainAmount` above zero, the spare amount is instead
`PowerSurplus` times the house's drain over that amount, worked out in double
precision and cut to a whole number: a house draining twice that amount keeps
twice `PowerSurplus` spare. The drain is the one the house last counted from
its buildings, and a new house's is zero.

Source: the `RulesExt` reads in `src/Ext/Rules/Body.cpp` (`LoadBeforeTypeData`,
hook 0x679A15) and `HouseExt::CalculatePowerSurplus` in
`src/Ext/House/Body.cpp`, which the House constructor hook (0x4F6532, same
file) and `HouseClass_UpdatePower_AfterBuildings` (0x508D8D,
`src/Ext/House/Hooks.cpp`) run. It writes the House field (+0x160B4) that the
original's `AI_Choose_Building` takes off the output at 0x4FE6FC..0x4FE71D.

## Where VERA20K owns it

- `rules/ruleset.rs` — the three `[AI]` reads and their configuration hash.
- `rules/phobos_switch.rs` — whether Phobos plays (`RuleSet::phobos`).
- `sim/power_system.rs` — `PowerState::power_surplus` (House+0x160B4) and the
  drain it scales from, kept at each count of the house's buildings.
- `sim/ai_base_building.rs` — the power plant check.

## Residuals

- Phobos' `AI.Power` keys and its unit power drain (`UnitPowerDrain`) are not
  ported. Without them Phobos counts the original's drain and makes the
  original's check, as VERA does.
- `HouseClass::ComputerTakeover` (0x50A5C0) also reads the spare amount. Only
  the original's null-modem disconnect calls it (0x5F031A), and VERA has no
  port of it.
