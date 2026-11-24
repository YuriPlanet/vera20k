# Custom Ares radar-jam (`RadarJamHouses`, `RadarJamDelay`, `RadarJamAffect`, `RadarJamIgnore`)

The Ares `RadarJamRadius=` jamming scan, retargeted from "every non-allied
House" to a `CanTargetHouse` filter plus two building-type lists.

## Keys

| Key | Type | Default |
|---|---|---|
| `RadarJamHouses=` | `Nullable<AffectedHouse>` | unset |
| `RadarJamDelay=` | `Nullable<int>` | unset |
| `RadarJamAffect=` | `ValueableVector<BuildingType*>` | empty |
| `RadarJamIgnore=` | `ValueableVector<BuildingType*>` | empty |

Read: `TechnoTypeExt::LoadFromINI` (`Ext/TechnoType/Body.cpp:780-783`),
declarations `Ext/TechnoType/Body.h:44-47`, constructor `Body.h:482-485`.
VERA reads them in `src/rules/phobos_techno_extras.rs`, owner
`ObjectType::phobos_techno_extras`.

`RadarJamHouses=` falls back to `[General] RadarJamHouses` (`Ext/Rules/Body.cpp:213`,
constructor `Ext/Rules/Body.h:657`, default `Enemies`);
`RadarJamDelay=` to `[General] RadarJamDelay` (`:214`, `Body.h:658`, default
`30`). Both live on `RuleSet::phobos_globals`.

`RadarJamHouses=none` is a **value** (`AffectedHouse::None`, "jam nobody"), not
a clearing token: only a blank read or `<default>` resets the nullable
(`Utilities/TemplateDef.h:1833-1849`).

## Behaviour

`RadarJammerClass::Update` (`New/Entity/Ares/RadarJammerClass.cpp:5-38`):

```cpp
if (Unsorted::CurrentFrame - this->LastScan < pTypeExt->RadarJamDelay.Get(Global()->RadarJamDelay))
    return;
this->LastScan = Unsorted::CurrentFrame;
for (const auto pHouse : HouseClass::Array)
{
    if (!EnumFunctions::CanTargetHouse(pTypeExt->RadarJamHouses.Get(Global()->RadarJamHouses), pOwner, pHouse))
        continue;
    for (const auto pBuilding : pHouse->Buildings)
    {
        if (!pBuildingType->Radar && !pBuildingType->SpySat) continue;
        if (inRange && !RadarJamIgnore.Contains(t) && (RadarJamAffect.empty() || RadarJamAffect.Contains(t)))
            this->Jam(pBuilding);
        else
            this->Unjam(pBuilding);
    }
}
```

So a House the filter refuses is skipped entirely — its buildings keep whatever
registration they already had — while an admitted House's building is
explicitly unjammed when it fails range or the type lists.

VERA's owner is `Simulation::update_ares_radar_jammer` (`src/sim/radar_jammer.rs`):
the scan interval, the House filter (`sim::combat::can_target_house`) and the
two lists all come from the jammer's own type, read before the mutable borrow;
`in_range && admitted` jams, anything else unjams.

## Native anchors

This chain has no gamemd address: it replaces Ares' `RadarJammerClass::Update`, which Phobos patches
at `AresBase + 0x68500` and `+0x69470` (`Misc/Hooks.Ares.cpp:345..346,469..470`). The scan body the
keys feed is `New/Entity/Ares/RadarJammerClass.cpp:5..47`.

## Residuals

- **Interior House iteration order.** Native walks `HouseClass::Array` and each
  House's `Buildings`; VERA walks the entity store in id order. `Jam`/`Unjam`
  are per-building idempotent `RegisteredJammers` inserts, so only the order of
  the `RecheckRadar` writes differs, and that flag is a boolean.
- **`ValueableVector` drops unknown names.** Native's `detail::read` discards a
  token whose type lookup fails, so a misspelt name simply never matches. VERA
  stores the token and compares by name, which also never matches — same
  outcome, but the stored vector keeps the misspelling.
- **`AffectedHouse::Neutral`.** `can_target_house` answers the multiplay-passive
  arm before the ally test; the Ares scan has no separate neutral cascade, so
  `RadarJamHouses=neutral` admits exactly the passive Houses there too.

## Tests

- `src/rules/phobos_techno_extras_tests.rs` — the parse group, including the
  `none`-is-a-value and `Valueable<AffectedHouse>`-keeps-current cases.
- `src/rules/phobos_globals_tests.rs` — `the_radar_jam_globals_default_and_read_from_general`.
