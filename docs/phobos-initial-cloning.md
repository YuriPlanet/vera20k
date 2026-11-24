# Cloned infantry strength (`InitialStrength.Cloning`)

Phobos lets a `Cloning=` building start its exiting clone at a percentage of
the clone infantry type's own Strength, as a single percentage or a
`min,max` range (`Ext/BuildingType/Body.cpp:196`, the
`Valueable<PartialVector2D<double>>` field `Ext/BuildingType/Body.h:33`,
constructor `{ { 1.0 } }` at `Body.h:169`).

```ini
[CLONER]                              ; BuildingType
InitialStrength.Cloning=1.0           ; Valueable<PartialVector2D<double>>, default {1.0}
```

## Native behavior

`BuildingClass_ExitObject_InitialClonedHealth` @ `0x443C81`
(`Ext/Building/Hooks.cpp:91-106`) sits on the `BuildingClass::ExitObject`
prologue (`443C81` sets Techno+3D5) and therefore runs on every ExitObject
call, before RTTI dispatch, admission or Unlimbo. When the exiting object is
an `InfantryClass` and the producer building's type is `Cloning=`:

```cpp
const double percentage = GeneralUtils::GetRangedRandomOrSingleValue(
    BuildingTypeExt::Fetch(pBuilding->Type)->InitialStrength_Cloning);
const int health = pInf->Type->Strength;
const int strength = Math::clamp(static_cast<int>(health * percentage), 1, health);
pInf->Health = strength;
pInf->EstimatedHealth = strength;
```

The health is the clone infantry type's Strength — not the building's — and
both the actual and the estimated value take the same computed strength.

`GeneralUtils::GetRangedRandomOrSingleValue(PartialVector2D<double>)`
(`src/Utilities/GeneralUtils.cpp:66-72`) takes `min = (int)(X * 100)` and
`max = (int)(Y * 100)` — the stored doubles are the Phobos `Parser<double>`
scan (strtod, binary32 store, `%` at 0.01f), widened back to binary64 — and
returns `X` when `X >= Y || ValueCount < 2`, else
`ScenarioClass::Instance->Random.RandomRanged(min, max) / 100.0`. `PartialVector2D`
counts the values its `MultiParser<double>::Parse` read (`Utilities/TemplateDef.h:290`,
`Utilities/Parser.h:345`): at most two comma-separated tokens, the scan stops
at the first token that does not convert, and a one-token read leaves the
second slot stale with `ValueCount == 1`.

So the two arms are:

- **Single value** (`1.0`, `0.5`, or a single-point `min,max` where
  `min >= max`): the percentage is `X` itself — no RNG draw — and
  `(int)(health * X)` floors the product; `0.335` stores the widened
  binary32 `0.3300000131...`, so a Strength-100 clone floors to 33.
- **Real range** (`X < Y` with two authored values): one inclusive integer
  percentage draw on the Scenario RNG per ExitObject call, divided by 100,
  then the same truncation and the `Math::clamp` to `[1, health]`.

## Where VERA20K owns it

- `rules::ini_value::PartialDoublePair` is the `PartialVector2D<double>`
  storage (`low`/`high`/`values`), with `read_ares_partial_double_pair`
  mirroring the int pair's token walk and [`parse_phobos_double`] as the
  token scan. `ObjectType::phobos_initial_cloning` carries the key, read
  per rules pass with the `{1.0}` default.
- `sim::production::production_queue::exit_produced_object` — the shared
  `BuildingClass::ExitObject` owner (its doc cites `Exit443C81`) — applies
  the hook after the producer type resolves and before the exit arms, so a
  placement retry re-applies it exactly like the native per-call hook. The
  producer type's stock `cloning` flag guards it and the Infantry category
  test stands in for the `abstract_cast<InfantryClass*>`. Both the stock
  cloning-vat copies (`exit_cloning_vat_copies`) and the Ares clone path
  reach the same owner. The health write uses `Health::set` (the
  constructor-time store owner) and `EstimatedHealth::reset` (the explicit
  constructor-initializer owner), so the alias follows the same owner as
  other construction-time health writes.
- `initial_cloning_percentage` is the `GetRangedRandomOrSingleValue` port:
  the `is_single()` no-draw rule, the `int` truncations of `X * 100` /
  `Y * 100`, and one `ScenarioRng::next_range_i32_inclusive(min, max)` draw
  for a real range.

## Phobos quirks kept

- The no-draw test compares the stored doubles (`X >= Y`), not the truncated
  integer bounds: `0.201,0.202` truncates to `20,20` but still draws
  (`RandomRanged(20, 20)` in VERA's designated owner returns the bound
  without consuming a draw).
- `EstimatedHealth` takes the computed strength too — the hook writes both
  fields explicitly, so the clone's scan reserve matches its actual health.
- The key only ever reads the producer's own type; a barracks whose product
  a cloning vat duplicates keeps the stock full-health exit for its own
  product, and the copy takes the vat's key.

## Residuals

- **RNG/timer/detach ledger.** The chain passes exactly one Scenario RNG
  draw per ExitObject call on the exiting clone when a real `min,max` range
  is authored, and none for a single value, a single-point range or the
  `{1.0}` default. No timer writes or detach calls.
- **Per-call re-application.** The native hook runs on every ExitObject
  call, so a factory retry of a refused or failed placement redraws the
  ranged percentage; the port reproduces this because the write sits on the
  same per-call owner. Trigger: a cloning exit that retries. Impact: the
  final clone keeps the last draw. Frequency: retry paths only. Risk: none
  beyond the native behavior it mirrors.
- Retail INIs never author the key (Phobos-only), so the default `{1.0}`
  truncates to the full Strength and the stock exit is unchanged.
- Retail/Phobos side-by-side play is untested in this checkout (no retail
  `ini/` present).
