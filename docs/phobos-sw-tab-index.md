# Custom super-weapon sidebar tab (`TabIndex`)

Which sidebar tab a super weapon's cameo occupies.

## Keys

| Key | Type | Default |
|---|---|---|
| `TabIndex=` | `Valueable<int>`, validated `0..=3` | `1` |

Read: `SWTypeExt::LoadFromINI` (`Ext/SWType/Body.cpp:285-286`), declaration
`Ext/SWType/Body.h:88`, constructor `Body.h:187`. VERA reads it in
`src/rules/phobos_sw_tab_index.rs`, owner
`SuperWeaponType::phobos_tab_index`. Tests:
`src/rules/phobos_sw_tab_index_tests.rs`.

`GeneralUtils::IntValidCheck(&TabIndex, pSection, "TabIndex", 1, 0, 3)`
(`Utilities/GeneralUtils.cpp:19-26`) is **not** a clamp: a value outside
`[0, 3]` is reset to `1` wholesale and logged as a developer warning. So
`TabIndex=5` and `TabIndex=-1` both land on the Defense tab, never on `3` or
`0`.

## Behaviour

`SidebarClass::GetObjectTabIndex` (`0x6ABC9D`,
`Ext/SWType/Hooks.cpp:245-258`) answers `pSWTypExt->TabIndex` for a
SuperWeapon type, and the four `SidebarClass_*_TabIndex` hooks
(`:204-204`, `:215`, `:230`, `:262`) route every tab lookup through that
getter. A fifth, `SidebarClass_6A8D07` (`:264`), skips the stock tab check
outright.

VERA's `SidebarTab` (`src/ui/sidebar/mod.rs`) is the same 0..3 scan order —
Building, Defense, Infantry, Vehicle — so `TabIndex` maps straight onto it.
The production cameo construction is `super_cameo`
(`src/app/sidebar_projection.rs`), which used to hardcode
`SidebarTab::Defense` and now reads
`SidebarTab::for_sw_tab_index(kind.phobos_tab_index.index())`. A stock build
keeps the hardcoded Defense strip.

## Residuals

An out-of-range or blank `TabIndex` resets to `1` instead of clamping, which is what the reader does;
a stock build keeps the hardcoded Defense strip.

## Tests

- `src/rules/phobos_sw_tab_index_tests.rs` — the default of `1`, all four
  valid indices, and that an out-of-range or blank value resets to `1`
  rather than clamping.
