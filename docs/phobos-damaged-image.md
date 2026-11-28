# Damaged image changes for Units and Aircraft (`Image.Condition*`, `WaterImage.Condition*`)

A Unit or Aircraft may name replacement types for the yellow and red health
bands, with a separate pair for a Unit that sits on water.

## Keys

| Key | Type | Default |
|---|---|---|
| `Image.ConditionYellow=` | `Nullable<TechnoTypeClass*>` | unset |
| `Image.ConditionRed=` | `Nullable<TechnoTypeClass*>` | unset |
| `WaterImage.ConditionYellow=` | `Nullable<UnitTypeClass*>` | unset |
| `WaterImage.ConditionRed=` | `Nullable<UnitTypeClass*>` | unset |

Read: the land pair for every techno type in `TechnoTypeExt::LoadFromINI`
(`Ext/TechnoType/Body.cpp:1112-1113`), declarations `Ext/TechnoType/Body.h:329-330`,
constructor `Body.h:763-764`; the water pair in `UnitTypeExt::LoadFromINI`
(`Ext/UnitType/Body.cpp:78-79`), declaration `Ext/UnitType/Body.h:63-64`,
constructor `Body.h:125-126`. All four are **rule**-section keys.

`NeedDamagedImage` (`Ext/TechnoType/Body.cpp:1115`,
`Ext/UnitType/Body.cpp:80`) is the `|=` of them, so a type with any one set is
asked for a damaged image at all.

VERA reads the four keys in `src/rules/phobos_damaged_image.rs`, owner
`ObjectType::phobos_damaged_image`.

## Behaviour

Only two classes consume the keys; the other techno types read them and never
ask.

- `UnitExt::GetUnitTypeExtra` (`Ext/Unit/Body.cpp:343-375`) — green HP keeps
  the type; yellow HP takes `WaterImage.ConditionYellow=` on water and
  `Image.ConditionYellow=` elsewhere; red HP takes the red key of the same pair
  and falls back to that pair's yellow key. The water and land arms are an
  `if`/`else if` pair: a Unit on water never falls back to the land keys.
- `AircraftExt::GetAircraftTypeExtra` (`Ext/Aircraft/Body.cpp:195-220`) — the
  same two bands without a water pair.

The band is the stock `Object5F5DD0` split (`IsGreenHP`/`IsYellowHP`), i.e.
`[AudioVisual] ConditionYellow=` / `ConditionRed=` against the live `Strength`;
VERA reuses `sim::combat::health_category`.

The swap happens where gamemd picks the draw type, so it replaces the image,
the voxel and the turret decisions together:

- `UnitClass_DrawVXL_WaterType_Extra` (`0x73B4DA`,
  `Ext/Techno/Hooks.cpp:825-840`) and `UnitClass_DrawSHP_WaterType_Extra`
  (`0x73C602`, `:842-861`), both gated on `NeedDamagedImage &&
  IsClearlyVisibleTo(CurrentPlayer) && !Deployed`;
- `AircraftClass_Draw_Extra` (`0x414987`, `:863-871`).

VERA's owner is `app::presentation::instances::damaged_image::swap`, called
from `units::drawn_model_id` (Unit SHP and voxel, Aircraft voxel) and from the
non-Unit arm of the SHP visitor (Aircraft SHP). The swap precedes disguise,
`NoSpawnAlt` and the display override, and a fully deployed Unit has already
left through `UnloadingClass=` — which is the `!Deployed` gate. `water` is
`GetCell()->LandType == Water && !OnBridge`, read through
`damaged_image::unit_on_water`.

## Residuals

- **The `UnitTypeClass*`/`AircraftTypeClass*` cast is not modelled.** Native
  `abstract_cast`s the named type and drops the swap when the name is of the
  wrong class; VERA has one `ObjectType` for every class, so a name is taken
  as-is. Trigger: a rules set that points `Image.ConditionYellow=` at an
  Aircraft type from a Unit.
- **`IsClearlyVisibleTo(CurrentPlayer)` is the draw admission.** VERA applies
  the swap on the draw path that already admitted the object for the current
  viewer (shroud, limbo and `DrawState`), which is the same set for the stock
  cases. Trigger: a fully cloaked object still on a draw path.
- **Infantry and Buildings never swap.** That matches native — neither class
  has a `Get*TypeExtra` reader — but the keys parse on those types.

## Tests

- `src/rules/phobos_damaged_image_tests.rs` — the four keys, the `|=` flag and
  the `<default>`/blank reset.
- `src/app/presentation/instances/damaged_image_tests.rs` — the green/yellow/
  red split, the red band's fallback to the yellow key, the water pair
  replacing the land pair instead of following it, and the Aircraft reader.
