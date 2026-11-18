# BuildLimitGroup.*

Phobos `BuildLimitGroup.*` lets a TechnoType share one build cap across a group
of types, add extra quota for owning other types, and gate the cap on the
production queue. The keys are read in `src/Ext/TechnoType/Body.cpp:1033-1041`
(fields/defaults `Body.h:265-273` and `Body.h:697-705`); there is **no**
`IsBuildLimitGroupReached` in TechnoType. The whole algorithm lives in
`src/Ext/House/Body.cpp:773-1189`.

## Keys

Nine per-TechnoType keys (all `#[cfg(feature = "phobos")]` on `ObjectType`):

| Key | Native type | Default |
| --- | --- | --- |
| `BuildLimitGroup.Types` | `ValueableVector<TechnoTypeClass*>` | empty |
| `BuildLimitGroup.Nums` | `ValueableVector<int>` | empty |
| `BuildLimitGroup.Factor` | **`Valueable<int>`** (not double) | `1` |
| `BuildLimitGroup.ContentIfAnyMatch` | **`Nullable<bool>`** | unset → `[General]` |
| `BuildLimitGroup.NotBuildableIfQueueMatch` | **`Nullable<bool>`** | unset → `[General]` |
| `BuildLimitGroup.ExtraLimit.Types` | `ValueableVector<TechnoTypeClass*>` | empty |
| `BuildLimitGroup.ExtraLimit.Nums` | `ValueableVector<int>` | empty |
| `BuildLimitGroup.ExtraLimit.MaxCount` | `ValueableVector<int>` | empty |
| `BuildLimitGroup.ExtraLimit.MaxNum` | `Valueable<int>` | `0` (`<= 0` = no cap) |

Two `[General]` `Valueable<bool>` globals are the `Nullable` fallbacks
(`src/Ext/Rules/Body.cpp:223-224`, declared `Body.h:130-131`, constructed
`false` at `Body.h:667-668`): `BuildLimitGroup.ContentIfAnyMatch` and
`BuildLimitGroup.NotBuildableIfQueueMatch`. VERA reads them in
`PhobosGlobals::from_ini`'s `[General]` block; the new fields carry
`#[serde(default)]` and enter `RuleSet`'s hash automatically through
`PhobosGlobals`' derived `Hash` (`ruleset.rs:9138` hashes the whole struct).

## Algorithm (`Ext/House/Body.cpp:773-1189`)

- `CountOwnedIncludeDeploy` (`:774-780`) = `CountOwnedNow(pItem)` +
  `DeploysInto` + `UndeploysInto`.
- `QueuedNum` (`:1015-1034`) is `CountTotal(pType)` minus the factory's held
  object when it is that exact type; `RemoveProduction` (`:782-800`) drops
  queued copies on a match.
- The two native bodies disagree, so VERA keeps two owners:
  `BuildLimitGroupUpgradeCheck` (`:802-1013`) for `CanBuild` and
  `ReachedBuildLimit` (`:1036-1188`) for the cameo grey-out.

### Grey-out: `ReachedBuildLimit` (`check_build_limit`)

1. Empty `Types` or `Nums` (`:1040-1041`) means no cap.
2. ExtraLimit quota (`:1049-1077`): `limits` is a local copy of `Nums`. Only
   when `ExtraLimit.Types` and `.Nums` are both non-empty, each index `i`
   pairs `Types[i]/Nums[i]/MaxCount[i]`; `count` is the type's owned units
   (`GetUpgradesAmount` for PowersUp upgrades, else `CountOwnedNow`), clamped
   by `MaxCount[i] > 0`; every `limit` gains `count * Nums[i]` (only when
   `Nums[i] > 0`) and is then capped by `MaxNum > 0`.
3. Single `Nums` (`:1085-1112`): `count = Σ CountOwnedNow[i]*Factor[i]`,
   `queued = Σ QueuedNum[i]*Factor[i]`, `num = count - limits[0]`; returns
   true when `num + queued >= 1 - Factor(pItem)`. `ContentIfAnyMatch` is not
   read here; **`NotBuildableIfQueueMatch` does not change the return** — it
   only picks `RemoveProduction`'s count.
4. Multiple `Nums` (`:1115-1187`): pair by index with
   `size = min(Nums, Types)`; `num = CountOwnedNow[i] * (Factor[i] - limits[i])`
   (the Phobos defect, not `owned * Factor - limit`); the pItem slot tests
   `num + queued >= 1 - Factor`, other slots `num + queued >= 0`;
   `ContentIfAnyMatch` returns on any reaching slot, else the AND `reached`
   is the answer. `realReached` only selects `RemoveProduction`'s argument.

### CanBuild: `BuildLimitGroupUpgradeCheck` (`can_build`)

Reached only for a BuildingType whose `PowersUp.Buildings` is non-empty
(`Hooks.Upgrade.cpp:104-108`).

1. Empty `Types` (`:806`) returns Buildable. Empty `Nums` with non-empty
   `Types` falls into the multi branch with `size = 0`, which is reached
   (`TemporarilyUnbuildable`).
2. ExtraLimit quota (`:809-843`) and contributions (`:845-861`):
   `contribution = CountOwnedIncludeDeploy * that type's OWN Factor` (not
   pItem's).
3. Single `Nums` (`:867-890`): `Σ contributions >= limits[0] + 1 - factor`.
4. Multiple `Nums` (`:892-947`): first pass pairs by index; the pItem slot is
   never required by the AND (its only `realReached` assignment is
   unreachable), other slots need `contribution - limit >= 0`;
   `ContentIfAnyMatch` returns on any reaching slot.
5. Second pass (`:949-1012`) runs when the first pass did not return:
   - `ContentIfAnyMatch`: any slot with `contribution >= limit + 1 - factor`
     and, when `includeQueued`, no copy in production blocks.
   - Single `Nums`: the same sum comparison would already have returned; the
     branch is dead but reproduced.
   - Multiple `Nums`: every slot must be at its threshold (`1 - factor` for
     the pItem slot, `limit` for others) and have no copy in production when
     `includeQueued`; otherwise Buildable.
6. `NotBuildableIfQueueMatch` never changes either owner's return value;
   only the unported `RemoveProduction` reads it. Queue presence in the
   second pass is `FactoryClass::FindByOwnerAndProduct`, VERA's
   `held_in_factory`.

## Native evidence (gamemd / Phobos hooks)

| Address | Function | Source |
| --- | --- | --- |
| `0x4F8361` | `HouseClass_CanBuild_UpgradesInteraction` | `Ext/BuildingType/Hooks.Upgrade.cpp:93-111` (calls UpgradeCheck at `:107`) |
| `0x4F7877` | `HouseClass_CanBuild_UpgradesInteraction_WithoutAres` | `Hooks.Upgrade.cpp:113` (self-disabling patch) |
| `0x50B669` | `HouseClass_ShouldDisableCameo_GreyCameo` | `Ext/House/Hooks.cpp:493-503` (sole `ReachedBuildLimit` caller) |
| `0x452678` / `0x4408EB` | `BuildingClass_CanUpgrade` / `Unlimbo` UpgradeBuildings | `Hooks.Upgrade.cpp:31,48` |
| `0x004F7870` | `HouseClass::CanBuild` | VERA `can_build` owner |
| `0x0050B370` | `HouseClass::CheckBuildLimit` | VERA `check_build_limit` owner |
| `0x004CA670` | `FactoryClass::CountTotal` | `FactoryRegistry::count_total` (`factory.rs:1011`) |
| `0x005F7900` | `ObjectTypeClass::FindFactory` | `can_build::find_factory` |

`Building/Body.cpp` and `Techno/Body.cpp` contain no `BuildLimitGroup` match.
The Phobos hook at `0x4F8361` only replaces the result for a BuildingType
whose `PowersUp_Buildings` is non-empty, so `build_limit()` asks
`build_limit_group_upgrade_check()` only for such a type; every type still
meets `build_limit_group_reached()` through the grey-out. Both reuse
`HouseTracking::owned_count`, `held_in_factory()` and
`sim.production.factories()`; `count_owned_building_upgrades()` supplies
`GetUpgradesAmount` over **all** `PowersUp.Buildings` hosts.

## Phobos inconsistencies — ruling

The two duplicated algorithms disagree; each port keeps its own body because
both are observable (cameo grey-out vs CanBuild):

1. `BuildLimitGroupUpgradeCheck:901` uses `num = contribution - limit`;
   `ReachedBuildLimit:1142` uses `num = owned * (Factor - limit)`, which is
   mathematically different and looks like a Phobos defect. VERA reproduces
   both: the CanBuild gate subtracts the limit, the grey-out multiplies by
   the difference.
2. `ReachedBuildLimit:1102/1140` counts only `CountOwnedNow`, while
   `BuildLimitGroupUpgradeCheck:858` uses `CountOwnedIncludeDeploy`. Each
   owner uses its own counter.
3. In the multi-limit `UpgradeCheck` first pass every `realReached = false`
   assignment sits inside the opposite branch of its own `if`, so
   `reached && (realReached || notBuildable)` reduces to `reached` and the
   pItem slot is never required by that pass. Reproduced as written.
4. `NotBuildableIfQueueMatch` is only read by `RemoveProduction`'s argument in
   both bodies; it never changes a return value.

## Reader: `ValueableVector` token dropping

`ValueableVector<int>::Read` (`TemplateDef.h:1947`) sends every comma token
through `Parser<int>` and **skips** a token that does not convert
(`detail::parse_values`, `TemplateDef.h:1728-1741`), so `Nums=2,3,x` reads as
`[2,3]`. `IniSection::read_ares_int_list` (`ini_value.rs:627`) has exactly that
behaviour and is used for `Nums`, `ExtraLimit.Nums` and `ExtraLimit.MaxCount`.
Type-name lists (`ValueableVector<TechnoTypeClass*>`) drop unresolvable names
the same way; VERA reads them with `read_valueable_type_vector` while the
types are still being constructed, so the names stay as `Vec<String>` and the
consumer applies the same filter *before* pairing with `Nums` — a dropped name
does not hold a slot.

## Residuals (trigger · impact · frequency · risk)

- **No retail / visual / GPU validation**: this checkout has no retail `ini/`,
  `config.toml` or `RA2_DIR`, so retail-INI tests skip. Trigger: any run
  against real RA2 data. Impact: cap behavior only verified on synthetic
  fixtures. Frequency: always in this workspace. Risk: medium — real INI
  values may exercise untested `Types`/`Nums` shapes.
- **`RemoveProduction` queue deletion** (`:782-800`): ported for the
  grey-out arm. Native runs it inside the read-only query; VERA's gate returns
  the trim (`BuildLimitGroupGate`) and the queue-admission owner applies it
  (`phobos_trim_build_limit_group_queue` from `queue_production`, using
  `FactoryRegistry::remove_one_queued` — `FactoryClass::RemoveFromQueue @
  0x004CA620`, no refund, held product untouched). `NotBuildableIfQueueMatch`
  now widens the tail prune (`Body.cpp:1179`). Regression tests
  `build_limit_group_reach_prunes_the_excess_queue` and
  `build_limit_group_tail_prune_follows_not_buildable_if_queue_match`.
  Residual: `BuildLimitGroupUpgradeCheck`'s own prunes (`Body.cpp:896,943,
  962,995,1008`) still have no mutable caller — its `CanBuild` owner is a
  query shared with the UI. Trigger: a `PowersUp.Buildings` item reaches a
  group cap with copies queued through the CanBuild gate. Impact: those copies
  survive. Frequency: mods that combine upgrade hosts with build-limit groups.
  Risk: low.
- **Held-object deduction scope**: `QueuedNum` uses the primary factory of the
  type; VERA's `held_in_factory()` matches any factory of the owner, so a held
  object in a different strip is deduced from a queue count it was never part
  of (the count is clamped at 0). Trigger: a type held in a cross-strip
  factory. Impact: that type's queued contribution may be 0. Frequency: never
  on synthetic fixtures. Risk: low.
- **`GetUpgradesAmount` host resolution**: VERA resolves `PowersUp.Buildings`
  names through the RuleSet; an unresolved host contributes nothing, matching
  the native reader's dropped pointer. Trigger: a typo'd host name. Impact:
  installed upgrades count as 0. Frequency: broken mod data only. Risk: low.
