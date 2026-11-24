# Phobos weeds counter

The third branch of `CreditsClass_GraphicLogic_HarvesterCounter @ 0x4A25E0`
(`src/Misc/Hooks.UI.cpp:175-190`), after the harvester counter and the power
delta. It prints the local player's stored weed total on the sidebar.

## Keys

| key | file | default | source |
|---|---|---|---|
| `[Sidebar] WeedsCounter.Show` | `uimd.ini` | `no` | `Phobos.INI.cpp:186-187` |
| `[Phobos] ShowWeedsCounter` | `RA2MD.INI` / profile | `yes` | `Phobos.INI.cpp:110` |
| `[<Side>] Sidebar.WeedsCounter.Offset` | `rulesmd.ini` | `0,0` | `Ext/Side/Body.cpp:39`, `Body.h:35,66` |
| `[<Side>] Sidebar.WeedsCounter.Color` | `rulesmd.ini` | unset | `Ext/Side/Body.cpp:40`, `Body.h:36,67` |

`ShowWeedsCounter`'s static initializer is `false` (`Phobos.INI.cpp:71`) but the
reader's default argument is `true` (`:110`), and the read always runs, so the
effective default is `yes`. `Sidebar.WeedsCounter.Color` is a
`Nullable<ColorStruct>`; unset falls back to `Drawing::TooltipColor` at the draw
(`Hooks.UI.cpp:179`).

## Behavior

Both switches must hold. The defeated player returns at the hook entry (`:104`)
before any branch. Unlike the power delta, this branch asks nothing about owned
buildings.

The text is `L"%d"` of `static_cast<int>(pPlayer->OwnedWeed.GetTotalAmount())` —
the bare total, with no label prefix and no `/max` half. `GetTotalAmount @
0x006C9650` already leaves an integer in ST0, so the narrowing is exact.

The anchor is the sidebar midpoint `+50` on X and the literal `2` on Y, plus the
side's offset: the same column as the harvester counter, which Phobos documents
as overlapping by design. The alignment bits are hardcoded
`UseGradPal | Center | Metal12`; there is no alignment key.

## Data owner

The store is House `+0x314`, ported with `HouseClass::GiveWeed @ 0x004F9700` and
`StorageClass::GetTotalAmount` / `Add_Amount` in `sim::storage_class`. The
indicator reads it through `Economy::owned_weed_total`; it owns no state.

## Weed-charged superweapons

`UseWeeds=yes` on a `[SuperWeaponTypes]` entry ties its charge to the owner's
house weed store instead of its recharge clock
(`Ext/SWType/Hooks.cpp:126-177`, `0x6CBD2C`). Four `Valueable<T>` keys:

| key | default | effect |
|---|---|---|
| `UseWeeds.Amount` | `[General] WeedCapacity` | rounds the store must hold |
| `UseWeeds.StorageTimer` | `no` | show the weed ratio as the cameo stage |
| `UseWeeds.ReadinessAnimationPercentage` | `0.9` | parked-timer threshold |

Each Super AI visit compares the whole store with the amount. At or above it,
`StorageClass::RemoveAmount @ 0x006C96B0` takes the amount from **slot 0 only**
(the comparison covered every slot), the recharge timer restarts at zero so
the stock charged branch announces it, and a ready Super stops consuming.
Below it the timer parks at 15 frames when the store is at least
`ReadinessAnimationPercentage` of the amount and at 915 otherwise, and the
cameo stage comes from the weed ratio (`int(total * 54 / amount)`, capped at
54; `0x6CBEF4`). `ShowTimer` is cleared, so no sidebar timer slot is taken,
and a scripted `SetSWCharge`/percentage set is refused (`0x6CC1E6`).

## Residuals

- The weed source terrain, the `Weeder=` harvest branches, the gather animation
  and the Teleport locomotor arm are unported, so nothing fills the store in a
  retail match yet; the counter reads zero until a weeder chain lands.
- `UseWeeds.Amount` resolves `[General] WeedCapacity` from the merged rules
  layer. Phobos captures its constructor default when the type is built in the
  first pass, so a later layer that changes `WeedCapacity` without naming
  `UseWeeds.Amount` would not move the amount there. A later layer naming
  `UseWeeds.Amount=<none>` also reads as unset here, while Phobos' `Valueable`
  keeps the prior layer's number.
- Phobos clears the type's `ShowTimer` on its first Super AI visit, so the very
  first grant of a `ShowTimer=yes` UseWeeds type takes a sidebar timer slot
  and draws a countdown for one visit. VERA has no mutable type state and
  suppresses the slot at grant; the difference is one visit, first grant only.
- `UseWeeds` together with `UseChargeDrain` runs the stock +0x7C drain state
  machine; the restarted clock uses `RechargeTime` (or the instance's scripted
  custom recharge), matching native's pick between Super+0x24 and Type+0xB0.
- VERA resolves the indicator on the per-frame pass that ticks the power bar,
  where native runs it inside the credits tick. Same numbers, one pass per
  frame.
