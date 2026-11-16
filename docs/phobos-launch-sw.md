# LaunchSW

A warhead that launches superweapons at its impact.

```ini
[WH]                                  ; Warhead
LaunchSW=                             ; list of SuperWeaponType names
LaunchSW.RealLaunch=true              ; Valueable<bool>
LaunchSW.IgnoreInhibitors=false       ; Valueable<bool>
LaunchSW.IgnoreDesignators=true       ; Valueable<bool>
LaunchSW.DisplayMoney=false           ; Valueable<bool>
LaunchSW.DisplayMoney.Houses=all      ; AffectedHouse
LaunchSW.DisplayMoney.Offset=0,0      ; Point2D
```

Reader `Ext/WarheadType/Body.cpp:290-296`; constructor defaults
`Ext/WarheadType/Body.h:418-424`. `LaunchSW` is a
`ValueableIdxVector<SuperWeaponTypeClass>`: native resolves each trimmed token
to the house's Super index at parse time and drops unknown names. VERA keeps
the trimmed names and resolves them in the consumer.

## Behavior

`WarheadTypeExt::Detonate`'s LaunchSW block (`Ext/WarheadType/Detonate.cpp:121-150`),
after the `SpawnsCrate` arm and before `Crit_Active`/the per-target pass, for
every listed index:

- the house's Super must exist; with `RealLaunch` (default true) it must also
  be present, ready and not suspended (`!RealLaunch || (IsPresent && IsReady
  && !IsSuspended)`);
- `pHouse->CanTransactMoney(Money.Amount)` must pass;
- `(IgnoreInhibitors || !HasInhibitor)` and
  `(IgnoreDesignators || HasDesignator)` around the impact cell must pass;
- with `DisplayMoney` and a non-zero cost, a money label is added;
- `pSuper->SetReadiness(true); pSuper->Launch(cell, IsCurrentPlayer);
  pSuper->Reset();` runs, restoring the saved recharge timer unless
  `RealLaunch`.

The inhibitor/designator lists are the Ares-owned
`SW.Inhibitors`/`SW.Designators`; VERA mirrors the `SW.Next` behaviour: with
the lists unread (no Ares) the inhibitor gate is open and designators are not
required.

## Native anchors

`LaunchSW` runs inside the warhead body Phobos hooks at `BulletClass_Detonate` (`0x46920B`, `Ext/WarheadType/Hooks.cpp:8`) (`Ext/WarheadType/Detonate.cpp`), and the launches it starts go through
`SuperClass::Launch` (`0x6CC390`), whose tail at `0x6CDE40` is the same site the LimboDelivery chain
uses.

## Where VERA20K owns it

`rules::phobos_launch_sw` reads the keys onto `WarheadType`.
`sim::superweapon::launch_sw::apply_on_detonation` runs from
`combat::crit::detonate` right after the `TransactMoney` block and before the
crit prefix, reusing `chaining::affordable`, `chaining::influence_gates_pass`,
the shared `fire::launch_chained_super` (charge-preserving or consuming) and
the `BountyText` money label.

RESIDUAL: `LaunchSW=` resolves to indices at parse time natively; VERA keeps
the names and resolves in the consumer. A later INI pass that supplies an
unresolvable name leaves the native vector empty (the earlier entries were
cleared by the pass's read) while VERA still launches a name an earlier or
later layer defines; a forward-declared name likewise launches in VERA where
native dropped it. Trigger: layered rules or a name defined only by a later
layer; effect: an extra or missing launch.

RESIDUAL: native passes `pHouse->IsCurrentPlayer()` to `SuperClass::Launch`;
the shared `fire::launch_chained_super`/`launch` owner has no such flag
(documented in `launch_super_preserving_charge`), so the launch effects are
the same for both audiences.

`Money.Amount` is read whenever Ares or Phobos is compiled
(`Ext/SWType/Body.cpp:121`), so the affordability gate and the display label
work in both builds; no residual.

RESIDUAL: the launch cell is `Coord2Cell` of the impact; VERA floors the
lepton coordinate with the shared `lepton_to_cell` and clamps it to the `u16`
cell range, which differs only for an impact outside the map rectangle.
