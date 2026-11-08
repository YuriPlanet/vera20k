# Ares radiation types

Installed Ares 3.0 has no live radiation-type mechanism. The restored source
still carries `Enum/RadTypes.cpp` (a `[RadiationTypes]` enumeration with
`Warhead`, `Color`, `DurationMultiple`, `ApplicationDelay`, `LevelMax`,
`LevelDelay`, `LightDelay`, `LevelFactor`, `LightFactor` and `TintFactor`)
and a weapon `Rad_Type` pointer, but the 3.0 DLL never reads any of them:

- `RadType::LoadFromINI` is absent from the DLL. Only the constructor
  `10008A90`, `LoadFromStream` `10008C50` and `SaveToStream` `10008DD0`
  exist, and the only allocation is `LoadHelper::Process<RadType>`
  `10004100`, which rebuilds the array from a save stream. `RulesExt::
  LoadBeforeTypeData` (`1002AEF0`) loads the generic prerequisites, armors,
  cursors and tunnels, never `RadType::LoadFromINIList`. No DLL function
  references `DurationMultiple`, `ApplicationDelay`, `LevelMax`,
  `LightDelay`, `LevelFactor` or `TintFactor`.
- `WeaponTypeExt::ExtData::LoadFromINIFile` (`10055520`) reads no `RadType`
  key; `Rad_Type` (+0xAC) is cleared by the constructor `100548B0` and only
  serialized.
- Every RadSite consumer of `Rad_Type` in `Ext/WeaponType/Hooks.RadSite.cpp`
  (`4691D5`, `65B593`, `65B5CE`, `65B63E`, `65B6A0`, `65B6CA`, `65B6F2`,
  `65B73A`, `65B843`, `65B8B9`, `487CB0`) is commented out and absent from
  the 3.0 hook table.

So a `--features ares` build keeps the retail radiation owner
(`sim::radiation`, Rules `Rad*` keys) and reads neither `[RadiationTypes]`
nor weapon `RadType=`. `ares,phobos` keeps the Phobos owner
(`rules::phobos_radiation`, `docs/phobos-custom-radiation.md`) exactly as the
`phobos` build does; there is no second catalog to merge. The default build
is unchanged.

| gamemd hook | Ares.dll | name | disposition |
| --- | --- | --- | --- |
| `0x65B5FB` | `10056DB0` | RadSiteClass_Radiate_UnhardcodeSnow | ported: `RadSite::activate_light` skips the snow-theater red/blue -128 substitution under `ares` |

Residuals: none in scope. The listed keys have no effect in Ares 3.0.
