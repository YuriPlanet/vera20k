# Warhead `CreateGap`

`[WarheadType] CreateGap=N` shroud the impact area for enemies of the firing
house. `N` is in cells; a negative value reshrouds the whole map for every
non-allied house. Plain `Valueable<int>` read by
`Ext/WarheadType/Body.cpp:158`, default `0`.

```ini
[WARHEAD]
CreateGap=4      ; shroud a 4-cell disc for enemies
CreateGap=-1     ; reshroud every non-allied house
```

## Native evidence

`Ext/WarheadType/Detonate.cpp:51-80`, before the `Reveal` block:

```
if (createGap > 0) {
    if (!pHouse->IsControlledByCurrentPlayer() && !pHouse->IsAlliedWith(pCurrent)
        && !pCurrent->Defeated && !pCurrent->SpySatActive)
        // clear Revealed/Clear over the CreateGap + 0.5 disc, ShroudCounter=1
} else if (createGap < 0) {
    for (pOtherHouse : HouseClass::Array)
        if (!pHouse->IsAlliedWith(pOtherHouse) && !pOtherHouse->Defeated
            && !pOtherHouse->SpySatActive)
            MapClass::Instance.Reshroud(pOtherHouse);
}
```

The positive arm writes only the maps of players the firing house is not
allied with; the negative arm resets each such house's whole map.

## Where VERA20K owns it

- `rules::warhead_type::WarheadType::create_gap`.
- `sim::world::shroud_refresh::apply_warhead_create_gap` (same owner as
  `reveal_area`/`reset_shroud_for_owner`, `sim/world/shroud_refresh.rs:288`):
  - positive: `CellRangeIterator<CellClass>` around `Coord2Cell(impact)` at
    radius `CreateGap + 0.5`, then `FogState::conceal_map_cell` per admitted
    enemy client map and `update_shroud_edges` (`Map567F70`,
    `sim/vision/cell_reveal.rs:1176`).
  - negative: `reset_shroud_for_owner` for every non-allied house
    (`MapClass::Reset_Shroud @ 0x00577AB0`).
  - defeated and SpySat houses are skipped; the owner is its own ally.
- `sim::combat::crit::detonate`'s prefix calls it before Reveal
  (`sim/combat/crit.rs:130`).

## Residuals

- The positive arm's native `CurrentPlayer` is the VERA client-map domain
  (`fog_viewers`); the owner's own map and allies are never concealed.
- Retail/visual parity is untested here (no retail `ini/` in this checkout).
