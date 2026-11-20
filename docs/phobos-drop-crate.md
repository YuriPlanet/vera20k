# Drop a crate on death (`DropCrate`)

A `Nullable<Powerup>` that makes a dying object leave a crate of a fixed
`[Powerups]` kind on the cell it dies on.

## Keys

| Key | Type | Default |
|---|---|---|
| `DropCrate=` | `Nullable<Powerup>` | unset |

Read: `TechnoTypeExt::LoadFromINI` (`Ext/TechnoType/Body.cpp:1446`),
declaration `Ext/TechnoType/Body.h:431`, constructor `Body.h:851`. VERA reads it
in `src/rules/phobos_techno_extras.rs`, owner
`ObjectType::phobos_techno_extras.drop_crate`.

`detail::read<Powerup>` (`Utilities/TemplateDef.h:648-676`) is an exact,
case-insensitive scan of the fixed `[Powerups]` name table (`Powerups::Effects`),
so an unknown name is a parse failure and leaves the nullable unset. VERA
resolves the name against `crate::rules::powerups::POWERUP_NAMES`, the same
nineteen names in the same slot order, and stores the slot index.

## Behaviour

`TechnoExt::GetDropCrateIndex` (`Ext/Techno/Body.Internal.cpp:275-299`) answers
the type's slot when the instance has no per-instance override, else `-1`. Two
call sites, both after a techno's destruction is registered:

```cpp
// 0x702E4E, TechnoClass::RegisterDestruction
const int nSelectedPowerup = TechnoExt::GetDropCrateIndex(pVictim);
if (nSelectedPowerup >= 0)
    TechnoExt::TryToCreateCrate(pVictim->Location, static_cast<Powerup>(nSelectedPowerup));

// 0x70337D, HouseClass::RegisterDestruction (the Chronoshift teleport path)
```

`TryToCreateCrate` (`Ext/Techno/Body.cpp:579-656`) converts the location to
its cell and scans outward in rings of `currentRange = 0..10` — the upper line
`y - r` left to right, the lower line `y + r`, then the left and right columns
between them — calling `MapClass::PlacePowerupCrate @ 0x0056BEC0` at each cell
until one accepts. VERA already owns that callee as
`sim::crates::place_crate` (`src/sim/crates.rs`): it snaps the cell, takes the
first empty crate slot, places the overlay, arms `CrateRegen`, and writes the
powerup byte the pickup reads as its stored selection.

VERA ports the destruction hook as
`Simulation::drop_crate_on_destruction` (`src/sim/phobos_drop_crate.rs`),
called from `Simulation::record_the_kill` before the kill bookkeeping — the
`0x702E4E` position. It reproduces the ring scan over `place_crate`.

## The per-instance override

`TechnoExt::DropCrate` / `DropCrateType` (`Ext/Techno/Body.Internal.cpp:285-295`)
are written only by the Phobos trigger action `SetDropCrate=600`
(`Ext/TAction/Body.cpp:690-720`):

```cpp
for (const auto pTechno : TechnoClass::Array) {
    // the tag's FirstTrigger chain must list a trigger whose Type->ID
    // matches the firing trigger's Type->ID (_stricmp)
    pExt->DropCrate = pThis->Value;
    if (pExt->DropCrate == 1)
        pExt->DropCrateType = static_cast<Powerup>(pThis->Param3);
}
```

`GetDropCrateIndex` then answers `-1` unless the instance value is `1` and the
stored slot is `>= 0`; `0` disables the type key while any other value also
answers `-1`. VERA keeps the two ints on `GameEntity` (`phobos_drop_crate`,
`phobos_drop_crate_type`, both `-1` by default), writes them from the action
600 arm of the live-trigger dispatch (`src/sim/trigger_runtime.rs`, with
`TriggerRuntime::object_tag` / `tag_carries_trigger_type` for the two scans)
and reads them in `Simulation::phobos_set_drop_crate`'s counterpart,
`drop_crate_on_destruction`.

Remaining difference: Phobos serializes the two ints with `TechnoExt`;
VERA serializes them per entity through serde defaults, so an old save keeps
`-1` (unset) for a techno whose override was unknown to it.

The `HouseClass::RegisterDestruction` (`0x70337D`) teleport call site has no
separate VERA entry; a Chronoshift that destroys its target reaches the same
`record_the_kill` transaction.

## Tests

- `src/rules/phobos_techno_extras_tests.rs` —
  `drop_crate_resolves_a_powerup_slot_and_rejects_unknown_names`.
- `src/sim/phobos_drop_crate_tests.rs` — through the production destruction
  transaction: `drop_crate_places_its_powerup_on_the_death_cell` and
  `an_unset_drop_crate_places_nothing`.
- `src/sim/trigger_runtime_drop_crate_tests.rs` — action 600 through
  `apply_live_trigger_action`: `action_600_retypes_the_crate_of_a_tagged_techno`
  (then killed through `record_the_kill`, so the action's slot reaches
  `place_crate`), `action_600_with_zero_suppresses_the_type_key` and
  `action_600_leaves_an_untagged_techno_on_its_type_key`.
