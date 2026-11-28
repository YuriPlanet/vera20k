# Refinery normal active anim

`Refinery.UseNormalActiveAnim=` makes a refinery use its ordinary active
animations instead of the storage-fill animations the stock building art
swaps in as its ore store grows.

```ini
[GAREFN]                            ; art.ini section (the type's ImageFile)
Refinery.UseNormalActiveAnim=false  ; Valueable<bool>, default false
```

Reader: `Ext/BuildingType/Body.cpp:309` (field `Ext/BuildingType/Body.h:106`,
constructor `:227`), from the ART INI under the type's `ImageFile` section.
VERA reads it into `ArtEntry::refinery_use_normal_active_anim` in
`src/rules/art_data.rs` and resolves it through the same
`resolve_metadata_entry` path the other ART keys use.

## Native body

Two hooks, both keyed on the building type:

- `0x00445FE4` (`BuildingClass_Place_RefineryActiveAnim`,
  `Ext/Building/Hooks.Refinery.cpp:63`) is the start of
  `OnConstructionComplete`'s refinery branch. When the key is set it returns
  `0x00446183`, the normal opening-anim path, instead of the storage branch.
- `0x00450DAA` (`BuildingClass_UpdateAnimations_RefineryActiveAnim`, `:70`)
  is the start of `UpdateAnimations`' refinery storage arm; a set key returns
  `0x00450F9E`, the arm's merge point, so no tier comparison or anim swap
  runs.

Stock refineries (`BuildingType+0x16BB`) create one ActiveAnim slot from the
storage tier at `0x00446020..0x0044617E`: tier 0 fills slot 3 (`ActiveAnim`),
tier 1 slot 4 (`ActiveAnimTwo`), tier 2 slot 5 (`ActiveAnimThree`) and tier 3
or more slot 6 (`ActiveAnimFour`). With the key the type skips that branch
entirely and allocates the normal opening slots 3..6 plus IdleAnim18 through
`0x00446183`.

## VERA

`Simulation::grand_opening` (`src/sim/building_art.rs`) computes
`refinery &= !refinery_uses_normal_active_anim(...)`, so a keyed refinery
takes the existing normal arm (`[18, 3, 4, 5, 6]`) instead of
`initialize_refinery_storage_anim`. `update_building_storage_anims`
(`src/sim/building_art_storage.rs`) applies the same gate at its
`0x00450DAA` point, leaving `refinery_tier` and its slots untouched.

## Tests

- `src/rules/art_data_tests.rs` — the key, its default and its ART section.
- `src/sim/building_art_storage.rs` — the place and update arms: a keyed
  refinery keeps tier 0 and allocates slots 18/3/4/5/6, the stock one writes
  the tier and allocates only the storage slot.

## Residuals

- The two arms above are the only consumers; placement and per-frame update
  both read the same key.
- The key is Phobos-only. With its default false a refinery keeps the stock
  storage-anim behavior.
