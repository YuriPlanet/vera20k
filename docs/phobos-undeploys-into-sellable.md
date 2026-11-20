# Sellable repack buildings

`UndeploysInto.Sellable=` lets a non-Construction-Yard building with
`UndeploysInto=` sell normally instead of packing up into its vehicle.

```ini
[SOMEBUILDING]                      ; BuildingType
UndeploysInto=UNIT
UndeploysInto.Sellable=yes          ; Valueable<bool>, default false
```

Reader: `Ext/BuildingType/Body.cpp:256` (`Body.h:110`, constructor `:229`).

## Native body

`BuildingClass::Mission_Selling` decides with
`BuildingExt::CanUndeployOnSell` (`Ext/Building/Hooks.Selling.cpp:33-58`).
A building with no `UndeploysInto=` never packs up. A Construction Yard packs
up only with `MCVRedeploy`, `AllowDeployControlledMCV` or no mind controller,
and an ArchiveTarget. Any other repack building packs up unless
`UndeploysInto.Sellable=yes` is set, in which case it packs up only while it
holds an ArchiveTarget (`pThis->ArchiveTarget != nullptr`); otherwise the
sale runs normally. The `0x449CC1` EVA announcement uses the separate
Phobos condition `!ArchiveTarget || !UndeploysInto`
(`Ext/Building/Hooks.Selling.cpp:70-76`), and the `0x44A7CF` sell-sound hook
consults the conversion answer.

## VERA

`ObjectType::phobos_undeploys_into_sellable` carries the key and
`sim/production/production_sell.rs::qualifying_undeploy` applies it in the
non-ConYard branch: the default returns true (pack up), while a sellable
building returns the shared `sale_archive` answer. The same owner feeds
`sell_stage_zero` and the sell-sound path; `sell_complete` emits the
`StructureSold` announcement with the Phobos `0x449CC1` condition.

## Residuals

- Construction Yard behaviour keeps VERA's existing `game_mode_nonzero` and
  human-owner gates from the original `0x0044A8DF` sale visit. Phobos's
  `0x449CC1` rewrite returns `CanUndeployOnSell` straight to the conversion,
  bypassing those two tests, so a single-player ConYard with an ArchiveTarget
  and `MCVRedeploy=yes` can convert in retail+Phobos where VERA sells.
  Trigger: selling an archived ConYard outside multiplayer. Frequency: rare.
  Risk: the wrong refund/unit outcome.
- Retail rules author no `UndeploysInto.Sellable`; the default packs up.
