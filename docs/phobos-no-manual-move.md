# NoManualMove

One TechnoType key that takes an object away from the player's order paths
while leaving its AI free to move it.

Key: `Ext/TechnoType/Body.cpp:803` (`this->NoManualMove.Read(exINI, pSection,
"NoManualMove")`). Declaration: `Ext/TechnoType/Body.h:73` as
`Valueable<bool>`, so the constructor leaves it `false` and an absent, empty
or invalid value keeps that default. Consumers: `Ext/Techno/Hooks.cpp:778-798`.
Official text: `docs/New-or-Enhanced-Logics.md:2411-2427`.

## Key

| Key | Scope | Native type | Default |
| --- | --- | --- | --- |
| `NoManualMove` | TechnoType | `Valueable<bool>` | `false` |

No `[General]` counterpart: the key is a per-type `Valueable`, not a
`Nullable` deferring to a global.

## The three hooks

### `TechnoClass_CanPlayerMove @ 0x00700C40` — hook `0x00700C58`

`0x00700C40` is the techno's **virtual slot 0xA0** answer: `push esi; mov
esi,ecx; mov ecx,[esi+0x21c]; call 0x0050B6F0` asks
`HouseClass::IsHumanPlayer`, then the body tests the weapon/armed virtual
`vt+0x37C`, `+0x2E4`, `[type+0xD54]` and further virtuals. The hook replaces
the six bytes at `0x00700C58` (`call dword ptr [eax+0x37C]`) and jumps to
`0x00700C62` = `xor al,al; pop esi; ret`, so a type with the key answers
**false** to the whole predicate, not merely to the weapon test.

### `BuildingClass_CanPlayerMove @ 0x0044F62B` — hook `0x0044F62B`

For buildings the same slot 0xA0 is `0x0044F5C0`. Its arms are the rally line
(`vt+0x284`, `0x0044F5D5`), the owner's MCV-redeploy option
(`[house+0x16B9]` with `[0xA8B238]`/`[0xA8B320]` and `+0x2C0`), and finally a
fall-through into `0x0044F62B`:

```
0044F622  mov edx,[esi+0x520]      ; BuildingTypeClass
0044F628  xor eax,eax
0044F62A  pop esi
0044F62B  mov ecx,[edx+0x408]      ; UndeploysInto   <- hooked, 6 bytes
0044F631  test ecx,ecx
0044F633  setne al
0044F636  ret
```

The hook forces `ECX` to 0 and resumes at `0x0044F631`, so the building answers
false at the `UndeploysInto` tail while the rally-line and MCV-redeploy arms
above stay native.

### `BuildingClass_CellClickedAction @ 0x004437B3` — hook `0x004437B3`

The cell click reads `UndeploysInto` at `+0x408` first; the hook replaces those
six bytes and jumps to `0x0044384E`, the arm that stores no destination. A
`NoManualMove` building therefore sets no move goal from the click — the
"prevents it from undeploying when setting the rally point" line of the
official text.

## In VERA

All three hooks land on one function, because VERA already ports virtual+A0 as
`Simulation::techno_player_controllable` (`src/sim/world/world_orders.rs`), and
its two arms are exactly the two native implementations:

- **non-building arm** — the port of `0x00700C40`. `NoManualMove` returns false
  right after `HouseClass::IsHumanPlayer` and the bunker-link test, matching
  where `0x00700C58` sits inside the native body.
- **building arm** — the port of `0x0044F5C0`. `has_rally_line()` and the
  ConYard/`MCVRedeploy` arm are untouched; only the last line
  (`sim::production::undeploy_target`, the `UndeploysInto` test of
  `0x0044F62B`) is gated.
- the cell click needs no separate gate: a building's click destination is
  admitted through `Simulation::takes_rally_point`, which asks the same
  predicate, so the false answer covers `0x004437B3` too.

Nothing is added to the mission or event path: `EventClass::Execute` does not
ask virtual+A0, so an AI or script Move still lands on the object, exactly as
the hook leaves the AI branch alone.

## Residuals

- **Wider than the official text.** Because the hooks sit inside the shared
  virtual+A0 answer, `NoManualMove` refuses *every* player path that asks it —
  Move, Stop (`0x00730ECD..0x00730EE3`), Deploy/Unload, and the attack rung —
  not only move orders. VERA follows the implementation, which is what the
  native instruction does.
- **Rally point and sell stay open for factories.** The `0x455DA0` arm returns
  before the gated tail, so a building that carries a rally line still answers
  true in both builds; only the `UndeploysInto` arm is refused.
- **No retail INI validation.** The assertions run on synthetic sections; the
  shipped rules set no `NoManualMove`, so the key is dormant in retail play.
- **Cursor drawing is out of scope.** Native refuses the click; it does not
  change the move cursor, and neither does this port.
