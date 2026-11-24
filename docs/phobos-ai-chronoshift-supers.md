# Phobos AI Chronoshift supers (`AIChronoSphereSW`, `AIChronoWarpSW`)

The computer's Chronoshift script actions (`TMission::ChronoShiftToBuilding`
`0x006EFE60`, `ChronoShiftToTarget` `0x006F0130`) otherwise take the last
`Type=ChronoSphere` and the last `Type=ChronoWarp` entry of the house's Supers
vector. Phobos replaces both lookups with
`HouseExt::GetAIChronoshiftSupers` (`Ext/House/Body.cpp:333-368`, hooked at
`0x6EFEFB`, `0x6F0008`, `0x6F005F`, `0x6F01B0`, `0x6F02AD`, `0x6F0304`) and two
`[General]` keys:

| Key | Phobos read | Type | Constructor |
|---|---|---|---|
| `AIChronoSphereSW=` | `Ext/Rules/Body.cpp:122` | `ValueableIdx<SuperWeaponTypeClass>` | unset (`-1`) |
| `AIChronoWarpSW=` | `Body.cpp:123` | same | unset (`-1`) |

**The value is a superweapon type ID, not an index.** `ValueableIdx::Read`
(`Utilities/TemplateDef.h:1814-1827`) reads a string and calls
`SuperWeaponTypeClass::FindIndex` (`Utilities/Enumerable.h:21`), so
`AIChronoSphereSW=EUCLSF` is what works and `AIChronoSphereSW=3` matches
nothing. `none` / `<none>` / a blank value stores the unset index; a name that
matches no type keeps the previous value.

## Behaviour

- An authored sphere and warp are used as they are — their `Type=` does not
  have to be `ChronoSphere` / `ChronoWarp`.
- Only when `AIChronoWarpSW=` is unset does the sphere type's
  `SW.PostDependent=` (`Ext/SWType/Body.cpp:146`, also a type ID) name the warp
  (`Ext/House/Body.cpp:343-349`). `SW.PostDependent=` is the same key Ares reads
  for the player's ChronoSphere click (`1006ED30`); each feature keeps its own
  field of the one authored name.
- Unless *both* slots are filled, the native loop over the Supers still runs and
  re-assigns **each** pointer it matches, so a scanned ChronoSphere overwrites
  an indexed one. That loop alone is the whole story without Phobos.
- The house fires through `SuperClass::Fire_SW`, whose first argument is the
  Super's `Type=` value used as the Supers index, so indexing an entry of
  another type fires that type's entry (see the module residual in
  `src/sim/team_script_vm/super_actions.rs`).
- The readiness gate (`0x006EFF3A..0x006F011B`) reads the sphere slot's charge,
  so a named non-ChronoSphere super gates the action on its own timer.

## VERA owners

- `src/rules/phobos_globals.rs` — `ai_chrono_sphere_sw`, `ai_chrono_warp_sw`
  (names, read through `IniSection::read_phobos_type_name`,
  `src/rules/ini_value.rs`).
- `src/rules/superweapon_type.rs` — `phobos_post_dependent`.
- `src/sim/superweapon/mod.rs` — `ai_chronoshift_super_indices`, the one owner
  of the selection, returning the two `[SuperWeaponTypes]` positions.
- `src/sim/team_script_vm/super_actions.rs` — script actions 56 and 57.

## Tests

`src/sim/superweapon/phobos_ai_chronoshift_tests.rs` covers name precedence,
the `SW.PostDependent` step, the scan's overwrite of a half-filled pair, and
`none` / unknown names. `chronoshift_building_tests.rs` runs actions 56 through
both keys end to end, and `super_actions_tests.rs` keeps the native-execution
goldens for the unkeyed scan path.
