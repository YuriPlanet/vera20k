# TransactMoney warhead keys

A detonating warhead can move credits to its firing house and show the amount
as a flying credit label. Phobos adds the block to `WarheadTypeExt::Detonate`
(`Ext/WarheadType/Detonate.cpp:102-111`); the keys are read in
`Ext/WarheadType/Body.cpp:159-163` and defaulted in `Ext/WarheadType/Body.h:309-313`.
The label is client-side text and never changes the wallet.

```ini
[WARHEAD]                            ; WarheadType
TransactMoney=0                      ; integer, signed, default 0
TransactMoney.Display=false          ; boolean, default no
TransactMoney.Display.Houses=All     ; AffectedHouse, default All
TransactMoney.Display.AtFirer=false  ; boolean, default no
TransactMoney.Display.Offset=0,0     ; Point2D, default 0,0
```

All five are `Valueable`: an absent or unparseable key keeps the constructor
value, so later layers may replace each field independently.

## Behavior

A weapon whose warhead carries a negative amount refuses the shot when the
firing house's `Available_Money` is below `-amount`
(`TechnoClass_CanFire @ 0x6FC339`, `Ext/Techno/Hooks.Firing.cpp:325-328`).
The negation wraps like the native two's-complement, so `i32::MIN` passes the
gate. A non-negative amount never gates.

On every detonation whose warhead carries a non-zero `TransactMoney`, before
the crit/AttachEffect per-target pass:

1. The decided house pays or receives the signed amount. Phobos documents the
   key as "warheads can now give credits to its owner at impact": the house is
   the bullet owner's house, else `BulletExt::FirerHouse` for the
   `BulletClass_Detonate @ 0x46920B` hook, or the passed source house for
   `MapClass_DamageArea @ 0x489286`. `HouseClass::TransactMoney` adds a
   positive amount (`Add_Credits @ 0x004F9950`) and spends a negative one
   (`Spend_Money @ 0x004F9790`).
2. With `TransactMoney.Display` the amount becomes
   `FlyingStrings::AddMoneyString(amount, nullptr, pHouse, Houses, coords,
   Offset)`: green for a gain, red for a loss. `coords` is the impact point
   unless `Display.AtFirer` is set and the firer is known, in which case it is
   the firer's `Location`. `Display.Houses` filters the label against the
   local player and `Display.Offset` shifts it in screen pixels. The
   `nullptr` source means no source visibility test: only the anchor's shroud
   is consulted.

## Where VERA20K owns it

Rules live in `rules::phobos_transact_money` and the warhead's
`WarheadType::transact_money`. The transfer and label are emitted by
`sim::combat::crit::apply_transact_money`, called at the top of the
`WarheadTypeExt::Detonate` port (`sim::combat::crit::detonate`) so the block
runs even when the warhead has no crit or AttachEffects. Both directions go
through `sim::credit_income::transact_money`, the single
`HouseClass::TransactMoney` owner (also used by the Ares superweapon money
path). The firing gate lives in `sim::combat::fire_error` at its `0x6FC339`
site, reading `WarheadFacts::transact_money` and `FirerFacts::owner_money`.
The label reuses `sim::flying_text::BountyText` / `FrameEffects`'s money-text
boundary. `BountyText::entity_id` is `Option`, and `None` selects the source-less
path native's `nullptr` source describes; the client's
`app::presentation::flying_credits` still owns the shroud, House and pixel
checks.

## Residuals

- The display's shroud test runs per client against the local player's fog,
  while native `FlyingStrings::AddMoneyString` tests
  `MapClass::IsLocationShrouded` once at emission. This is the same
  approximation the DrainMoneyDisplay and DisplaySoylent labels already
  record; a label can therefore appear or vanish for one client and not
  another.
- `TransactMoney` shares `WarheadTypeExt::Detonate`'s other per-house arms
  (`CreateGap`, `Reveal`, `SpawnsCrate.*`, `LaunchSW.*`) only in ordering.
  Those are now separate owners rather than gaps: `sim::combat::crit` runs the
  `CreateGap` (`:136-150`), `Reveal` (`:152-`), `SpawnsCrate` (`:210`) and
  `LaunchSW` (`:222`) arms, each with its own tests. A warhead that combines
  them with `TransactMoney` therefore runs this block plus those owners, still
  in native order.
