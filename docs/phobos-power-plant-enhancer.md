# Optional Phobos power plant enhancers

Build with `--features phobos` to let buildings raise the output of nearby
power plants, scale a plant's damage loss, and show the enhancers' ranges
while placing a plant.

```ini
[SOMEBUILDING]                     ; rulesmd.ini BuildingType
PowerPlantEnhancer.PowerPlants=    ; BuildingTypes this building enhances
PowerPlantEnhancer.Range=0         ; cells, 0 for any distance
PowerPlantEnhancer.Amount=0        ; added to each enhanced plant
PowerPlantEnhancer.Factor=1.0      ; multiplies each enhanced plant
PowerPlantEnhancer.MaxCount=-1     ; enhancers of this type per plant, <= 0 unlimited
PowerPlant.DamageFactor=1.0        ; 0 ignores damage, 1 is the stock loss

[AudioVisual]                      ; rulesmd.ini
ShowPowerPlantEnhancerRange=true

[Phobos]                           ; RA2MD.INI
ShowPowerPlantEnhancerRange=false
```

A building is an enhancer when its list is non-empty and `Amount` is not 0 or
`Factor` is not 1. Phobos's `BuildingClass_GetPowerOutput_Enhancer` hook
(`0x44E826`) replaces the stock output sum:

- a plant without power (`HasPower`) or under EMP outputs 0;
- each of the owner's enhancers that is active, powered, names the plant's (or
  an installed upgrade's) type, is within `Range` of the plant by
  `ObjectClass::Distance_To` (`0x5F6440`, planar, less the plant's foundation)
  and is under its type's `MaxCount` multiplies `Factor` and adds `Amount`
  (`BuildingTypeExt::GetEnhancedPower`);
- the factored output is rounded, scaled by health through `DamageFactor`, and
  the amounts are added after the health scaling.

While a building is being placed, each of the current player's active
enhancers that names the placing type and has a range draws its range ring,
when both switches are on and the enhancer is within 1.5 ranges of the
foundation centre (`TacticalClass_DrawRadialIndicator_Building_Extras`,
`0x6DBEA3`).

## Where VERA20K owns it

- `rules/object_type.rs`: the per-type keys, read with Phobos's readers
  (`Valueable<Leptons>` through `ReadDouble`, `Nullable<float>` for `Factor`,
  whose `<default>` and `none` reset it to 0).
- `rules/phobos_power_plant_enhancer.rs`: the `[AudioVisual]` switch.
- `sim/power_system.rs`: `enhanced_power` and `scaled_power_plant_output` in
  the House power assessment; `util/native_x87.rs`'s `object_planar_distance`
  is the shared `0x5F6440` arithmetic.
- `app/presentation/radial_indicators.rs`: the placement rings;
  `app/persistence/phobos_settings.rs` reads the client switch.

## Residuals

- Enhancers apply in the House's building order, not Phobos's list order
  (unlimbo and capture order). Only the float product of different enhancer
  types' factors can round differently.
- Phobos's `BuildingClass_GetRadialIndicatorRange_Extras` (`0x45670D`), which
  gives a selected enhancer its own range ring, is not ported.
- `[AudioVisual] ShowPowerPlantEnhancerRange=` is read from each scenario's
  rules. Phobos reads it with the other `RulesExt` scalars
  (`LoadBeforeTypeData`, `0x679A15`), which outlive a scenario, so a value
  that only an earlier map set carries over there.
- The placement ring is drawn at the enhancer's `GetCoords` height rather than
  the cell floor height (`0x578080`); the two agree for a building standing on
  its flat foundation.
- With `PowerPlant.DamageFactor=1`, VERA20K keeps the stock x87 product of
  output and health, which chops. Phobos is built with `/arch:SSE2`, so its
  product rounds to nearest: at some health values a damaged plant gives one
  less power than in Phobos. Outputs of 2^24 and more, which Phobos passes
  through a binary32 product, are not modelled either.
