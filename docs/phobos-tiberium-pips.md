# Phobos Tiberium storage pips (`Pips.Tiberiums.*`)

The stock `PipScale=Tiberium` arm (`0x70A1F6..0x70A340`) draws gems first
(`GetAmount(1)` slots) and then the summed Riparius/Vinifera/Aboreus slots,
via two rounded quantities. Phobos replaces the whole arm at hook
`0x70A1F6` (skip to `0x70A4EC`) with
`TechnoClass_DrawPips_Tiberium` (`Ext/Techno/Hooks.Pips.cpp:306-410`), which
counts and rounds every `[Tiberiums]` class on its own and exposes five
`[AudioVisual]` globals:

| Key | Phobos read | Constructor (`Body.h:624-628`) | VERA |
|---|---|---|---|
| `Pips.Tiberiums.Frames` | `Body.cpp:164` `ValueableVector<int>` | empty | `Frames[index]`; past the list 5 for gems (1), 2 otherwise |
| `Pips.Tiberiums.EmptyFrame` | `Body.cpp:165` `Valueable<int>` | `0` | frame of a slot no class fills |
| `Pips.Tiberiums.DisplayOrder` | `Body.cpp:166` `ValueableVector<int>` | empty | class draw order; empty takes the native scan order `{0,2,3,1}` |
| `Pips.Tiberiums.WeedFrame` | `Body.cpp:167` `Valueable<int>` | `1` | a Weeder's filled weed pip |
| `Pips.Tiberiums.WeedEmptyFrame` | `Body.cpp:168` `Valueable<int>` | `0` | a Weeder's empty weed pip |

The frame list and draw order are read with `read_ares_int_list`
(`ValueableVector<int>`: a present key replaces the list, an absent one keeps
it, unparsable tokens are skipped); the three scalars with `read_int`.
VERA's owners are `src/rules/phobos_globals.rs` and the Tiberium arm of
`app/presentation/ui_overlays.rs::scale_pip_draws`.

## Behaviour

- A Weeder (`Weeder=yes` on a Unit or Building) draws its whole hold with
  `WeedFrame`/`WeedEmptyFrame`: a Unit uses its total Storage amount, a
  Building the owner's weed storage percentage. VERA models neither fractional
  Storage amounts nor weed storage, so the Unit arm reads its whole bales and
  a Building stays empty (residual).
- Otherwise each `[Tiberiums]` class counts its own bales and is rounded on
  its own with the shared `rounded_pip_quantity` kernel
  (`amount / Type.Storage * GetPipMax + 0.5`, x87 PC53/chop).
- `DisplayOrder` is deduplicated and range-checked, and classes it omits are
  appended; the default `{0,2,3,1}` draws Riparius before gems, the reverse of
  the stock arm's gems-first loop. This is the visible default change of the
  Phobos hook.
- A slot no class can fill uses `EmptyFrame`. The row advances with the same
  `Pips.Generic.Size` pair as the other non-Ammo rows (`0x709B2E`).

VERA's storage holds `CargoBale`s carrying their `[Tiberiums]` index, so the
per-class counts read the same authority the stock arm's gems/ore split used.

## Tests

- `the_tiberium_pip_globals_drive_the_strip` in
  `src/app/presentation/passenger_pip_tests.rs` (through
  `scale_pip_draws`): default order, authored `DisplayOrder`, `Frames` +
  `EmptyFrame`, and the Weeder arm.
- `the_tiberium_pip_globals_default_and_read_from_audio_visual` in
  `src/rules/phobos_globals_tests.rs` (constructors and authored reads).
- `a_slave_shows_the_load_it_carries` now expects the Phobos order in a
  Phobos build (ore before gems) and the stock order otherwise.
- The stock raster oracle `shared_harvester_scale_...` is gated to stock
  builds: its native rows are the gems-first arm that Phobos replaces.

## Residuals

- A Building Weeder's owner weed-storage percentage has no VERA owner
  (`sim/economy.rs` records the unported weed economy); its pips stay empty.
  No retail type sets `Weeder=`, so this is dormant. Trigger: custom
  `Weeder=yes` Building with `PipScale=Tiberium`.
- Negative authored frames clamp to 0 because `ScalePipDraw.frame` is a
  `usize`; native passes the raw int to DrawSHP. Rules authoring error only.
- VERA's bales are whole units; fractional `Storage.GetAmount` slots that a
  savegame could hold are not represented, matching the existing Storage
  owner's residual.
