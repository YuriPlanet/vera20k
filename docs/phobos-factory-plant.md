# FactoryPlant restricted lists

Phobos adds three BuildingType keys that keep a `FactoryPlant=` building out
of the stock House+140 product and price its bonuses per type, plus a
TechnoType key that scales the whole restricted discount.

Keys: `Ext/BuildingType/Body.cpp:224-226`, declarations
`Ext/BuildingType/Body.h:74-76`, constructors `:199-201`
(`FactoryPlant_MaxCount { -1 }`); `FactoryPlant.Multiplier` at
`Ext/TechnoType/Body.cpp:788`, `Body.h:48,486`
(`Valueable<float> { 1.0f }`). Official text:
`docs/New-or-Enhanced-Logics.md` ("Factory Plant" section).

## Keys

| Key | Scope | Native type | Default |
| --- | --- | --- | --- |
| `FactoryPlant.AllowTypes` | BuildingType | `ValueableVector<TechnoTypeClass*>` | empty |
| `FactoryPlant.DisallowTypes` | BuildingType | `ValueableVector<TechnoTypeClass*>` | empty |
| `FactoryPlant.MaxCount` | BuildingType | `Valueable<int>` | `-1` |
| `FactoryPlant.Multiplier` | TechnoType | `Valueable<float>` | `1.0` |

`ValueableVector::Read` clears and re-parses only when the key is present
(`Utilities/TemplateDef.h:1947-1955`); `Parser<TechnoTypeClass*>::Parse`
drops an unresolvable token (`:1728-1742`), so a list of only unknown names
is *empty* and therefore applies no filter. An absent or invalid
`MaxCount` keeps the constructor's `-1`.

## The lifecycle hooks

`BuildingClass_Unlimbo_FactoryPlant @ 0x441501`
(`Ext/Building/Hooks.cpp:437-454`) appends a plant whose allow or disallow
list is non-empty to `HouseExt::RestrictedFactoryPlants` and returns
`Skip = 0x441553`, skipping the House+140 append. The two ChangeOwner hooks
`0x448A31` (`:456-478`, skip `0x448A78`) and `0x449149` (`:480-499`, skip
`0x449197`) stable-remove from the old House's restricted vector and append
to the new one's.

**`FactoryPlant=` is a gate, even though the hook bodies never test it.**
The call sites do:

```
0x4414F1 mov eax,[esi+0x520]      ; BuildingTypeClass
0x4414F7 mov cl,[eax+0x16cd]      ; FactoryPlant
0x4414FD test cl,cl
0x4414FF je  0x441553            ; skip the House+140 append entirely
0x441501 ...                     ; <- hooked instruction
```

`0x448A2D` and `0x449139..0x449147` repeat the same `+0x16CD` test, and
`Ext/SWType/FireSuperWeapon.cpp:131` spells it out as `if (pType->FactoryPlant)`.
A building without `FactoryPlant=yes` therefore never enters the restricted
vector. VERA enforces this by projecting the restricted settings only when
`RegisteredBuilding.plant` is `Some`.

Removal on death or sale is **not** the ordinary House+140 removal: a
restricted plant was never in that vector. `HouseExt::OnDetach` at
`Ext/House/Body.cpp:743-772` erases it from `RestrictedFactoryPlants` at
`:765-770`; VERA's `remove_membership` removes from both lists.

## The multiplier

`HouseExt::GetRestrictedFactoryPlantMult @ Ext/House/Body.cpp:538-588`:

1. `mult = 1.0f`, and a `counts[pType->ArrayIndex]` map starts empty.
2. Walk `RestrictedFactoryPlants` in vector order. Skip when
   `MaxCount > -1 && counts[type] >= MaxCount`; skip when a non-empty
   allow list misses the priced type; skip when the disallow list contains it.
3. Only after all three filters pass, `counts[type]++` and
   `mult *= plantType->CostBonus[slot]`, where the slot is the **priced**
   type's `WhatAmI()` (`BuildCat == Combat` uses `DefensesCostBonus`).
4. Return `1 - (1 - mult) * pricedType.FactoryPlant_Multiplier`, all SSE
   single precision.

VERA's `ObjectType::factor_slot` is exactly that switch, and
`ObjectType::cost_bonuses` holds the five floats in that slot order, so the
loop uses `bonuses[target.factor_slot()]` instead of a second switch.

The two cost hooks `TechnoTypeClass_CostOf_FactoryPlant @ 0x711F39` and
`TechnoTypeClass_RefundAmount_FactoryPlant @ 0x711FDF`
(`Ext/TechnoType/Hooks.cpp:97-122`) multiply the stack float `[esp+8]` by
that result. The surrounding instructions are
`fild cost; fmul [esp+8]; fmul [esp+0xc]; call ftol`
(`0x00711F35..0x00711F41`), so the fold is
`plant * restricted` in C++ `float` (SSE `mulss`, round-to-nearest)
**before** the two x87 multiplies; the x87 steps are not reordered.

## In VERA

- `rules::phobos_factory_plant::FactoryPlantRestrictions` reads the three
  BuildingType keys with the existing `read_list` / `read_ares_int` readers
  (a failed `Parser<int>` keeps the constructor's -1). A post-read pass in
  `RuleSet` drops unknown type tokens before any list is asked for its size.
- `HouseBaseState` keeps `restricted_plants: Vec<u64>` beside the plain
  `plants` (House+140), and each `RegisteredBuilding` retains the plant's
  bonuses, both lists, `max_count` and its BuildingType identity for the
  count. `factory_plant_factors` still folds only `plants`. Both custom
  `Hash` implementations and both serde structs carry the new state.
- `Simulation::house_cost_factors_for(owner, object)` computes
  `restricted_factory_plant_multiplier(object)` and stores it on the
  **Copy** `HouseCostFactors` as `restricted_plant`; `adjust`,
  `ares_factory_plant` and `type_refund` multiply the plant float by it in
  that one place. `Simulation::cost_of`, `techno_refund`, the refinery,
  reverse engineering, passenger deletion, crew survival, building upgrade
  sale and `HouseClass::Added_To_Game` / `Removed_From_Game` all use the
  folded factors. `ForceValueFacts` needs no change: its `priced` receiver
  calls the same `adjust` / `ares_adjust`.
- `RuleSet::cost_of_with_restricted` resolves the factor per priced type:
  native `Cost_Of` re-enters the hooked base for a BuildingType's bundled
  PadAircraft and FreeUnit, so `Simulation::cost_of` and the kill-experience
  pricing (`sim::combat::record_kill`) recompute it for each sub-type instead
  of reusing the building's value.
- Keeping the factor on `HouseCostFactors` rather than overwriting
  `factory_plant[slot]` preserves native order when Ares is also enabled:
  Ares's `1 - (1 - product) * multiplier` must be computed from the
  un-folded product first, then Phobos's factor multiplies that result.
  Writing the already-folded float into `factory_plant[slot]` would make
  `ares_adjust` apply the Ares formula a second time.

## Ares and Phobos both read `FactoryPlant.Multiplier`

Ares stores a `double` (`ProductionTypeRules`, `read_ares_double_bits`);
Phobos stores a `float` (`INI_EX::ReadDouble`, the same Ares
`Parser<double>`, then C++ `static_cast<float>`, i.e. round-to-nearest,
`Utilities/TemplateDef.h:177-189`). VERA keeps one field in `ares,phobos`
builds: Ares's retained reader parses the key into `NativeF64Bits` and
`ObjectType::factory_plant_multiplier_f32` narrows it once with a Rust
`as f32`. In a Phobos-only build a dedicated `NativeF32Bits` field is read
with the same parser and narrowed the same way. There is never a second
field beside the Ares one.

## Residuals

- **No separate LimboDelivery residual.** `Ext/SWType/FireSuperWeapon.cpp:131-142`
  is a fourth append only because Phobos's `LimboCreate` bypasses Unlimbo and
  writes `House+68` / `House+140` / `RestrictedFactoryPlants` by hand. VERA's
  `sim::superweapon::limbo::limbo_create` calls
  `spawn_object_at_height_with_overlay_context`, which reaches
  `try_reveal_entity_with_context` -> `append_house_base_building` ->
  `append_membership` (`src/sim/world/lifecycle.rs:1743`). The same gate
  therefore covers delivered plants, and
  `delivered_restricted_factory_plant_uses_the_restricted_list` asserts it.
  Phobos also calls `CalculateCostMultipliers()` for a non-restricted plant;
  VERA folds `plants` when read, so no explicit recompute is needed.
- **Ares/Phobos multiplier precision.** Phobos's `Valueable<float>` parses
  with `INI_EX::ReadDouble` and then `static_cast<float>`
  (`Utilities/TemplateDef.h:177-189`), so narrowing Ares's parsed double to
  f32 yields the same bits for ordinary decimal input. A second stored field
  would only matter for malformed or overflowing text and would put a second
  field next to the Ares one in an `ares,phobos` build, which this port
  deliberately avoids.
- **`ForceValueFacts`** reaches the folded factor through
  `HouseCostFactors`; no residual remains there.
- **No retail INI validation.** The shipped rules leave all four keys unset,
  so the feature is dormant in retail play; assertions use synthetic rules.
  Retail-data checks belong to the integrating agent's workflow.
- **Cursor / sidebar display** of the discounted price is presentation and
  is not part of this port; it reads the same `Simulation::cost_of`.
