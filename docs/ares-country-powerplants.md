# Ares country AI power plants

Build with `--features ares`. Ares replaces how a computer house picks the
power plant it splices into its base plan when it runs short of power, and
`[Country] AI.PowerPlants=` lists that house's candidates.

| Key | Resolution |
| --- | --- |
| `AI.PowerPlants=` | `HouseTypeExt::GetPowerplants @ 0x1001BD20` answers the country's list while it is non-empty, otherwise the side's `[General]` plants: side 0 `GDIPowerPlant=`, side 1 `NodRegularPower=` then `NodAdvancedPower=`, side 2 `ThirdPowerPlant=`, none for a later side. The common country reader `0x1001C5D0` reads it on every pass (`0x1001C60C`), right after the one-shot `ParentCountry=` copy (`0x1001BDC0`), so a child's own list replaces the inherited one and a map can set it. |

The consumer is `AI_Choose_Building`'s power splice: the installed DLL patches
six bytes at `0x4FE782` (body `0x1001DDC0`) for every house, the stock
side-index branch included. The replacement keeps the candidates that Ares
CanBuild (`0x10021B00`) answers 1 and whose prerequisites the house owns
(`0x10021CA0`), draws `Randomizer(0, count-1)` once on Scenario+`0x218` and
splices that entry; with one survivor the draw takes nothing from the stream.
When none survives it splices the first candidate and logs "Fall back to the
first one". The choice moves the simulator's RNG stream, so the list enters
the state hash under `ares-country-powerplants-v1`.

On retail data no country sets the key, so a Soviet computer house that owns a
Battle Lab (NANRCT's `Prerequisite=NATECH,NACNST`) splices a nuclear reactor
or a Tesla reactor at random, where stock gamemd always splices NAPOWR.

## Consumers

- `sim::ai_base_building::power_plant_to_splice`: after the stock short-power
  gates (`0x4FE74D..0x4FE77C`) it asks `RuleSet::ares_power_plants` for the
  candidates and filters them through `production::can_build` and
  `production::owns_prerequisites`, then draws or falls back as above.

## Residuals

- `0x10021CA0` is met by any of the type's prerequisite lists; VERA reads only
  `Prerequisite=` (list 0), not `Prerequisite.Lists=`/`Prerequisite.List#=`.
  No retail INI sets either key.
- A house with no candidate at all (a side past 2, or unset `[General]`
  plants) is a fatal error natively (`0x1001DEF6`); VERA splices nothing.
- The fallback's debug log line is not written.
