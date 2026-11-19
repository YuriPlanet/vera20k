# Optional Phobos skirmish AI dehardcoding

These keys tune the hardcoded last-legs behavior of a skirmish computer house
and its regrouping after an MCV deploy. They work in every build: any of them
in the rules, mode or map turns Phobos on for the game
([the Phobos switch](phobos.md#the-phobos-switch)), and without Phobos on the
house sells off, hunts and regroups as in the original.

```ini
[General]
AIFireSale=true           ; boolean
AIFireSaleDelay=0         ; integer, frames
AIAllToHunt=true          ; boolean
GatherWhenMCVDeploy=true  ; boolean
```

A computer house with no live building whose type has `Factory=` sells every
building and sends every techno to hunt. `AIFireSale` and `AIAllToHunt` gate
those two calls independently. With a positive `AIFireSaleDelay`, the first
trigger arms a per-house timer and the sale waits until it completes;
`All_To_Hunt` still happens each tick. State four of the emergency block and
`Manage_Build_Queue`'s own hunt calls are not gated.

When a computer house's MCV deploys into its Construction Yard,
`GatherWhenMCVDeploy=no` skips `HouseClass::MoveGroundForcesToBase`, leaving
the starting force on its existing orders. The native block is entered only for
a skirmish computer house: `0x50B730` skips it for human- or
player-controlled houses.

Source: `src/Ext/House/Hooks.cpp` hook at `0x4FD8F7` (the
`sell all and all in` block), `AIFireSaleDelayTimer` in
`src/Ext/House/Body.h`, and `src/Ext/Techno/Hooks.Misc.cpp` hook at
`0x739920` (`UnitClass::TryToDeploy`, skipping the `0x50C920` call). The delay
timer uses the same current-frame anchor and `start + delay` expiry as YRpp's
`TimerStruct<FrameTimer>`; its `-1` start sentinel is VERA's `None`.

## Where VERA20K owns it

- `rules/ruleset.rs` — the four `[General]` reads and their configuration
  hash.
- `rules/phobos_switch.rs` — whether Phobos plays (`RuleSet::phobos`), which
  every consumer below checks first.
- `sim/house_strategy.rs` — gates the no-factory block's Fire_Sale and
  All_To_Hunt calls; the timer lives with the strategy state.
- `sim/house_state.rs` — the persisted per-house sale-delay timer.
- `sim/world/world_spawn.rs` — skips the deploy's ground-force regroup call.

## Residuals

- The Ares/Phobos keys apply to the no-factory block only; the emergency
  state-four sale and `Manage_Build_Queue`'s hunts stay unconditional, as in
  Phobos.
