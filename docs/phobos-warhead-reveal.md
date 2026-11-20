# Warhead `Reveal`

`[WarheadType] Reveal=N` reveals the map around the impact to the firing
house. `N` is in cells; a negative value reveals the whole map. Plain
`Valueable<int>` read by `Ext/WarheadType/Body.cpp:157`, default `0`.

```ini
[WARHEAD]
Reveal=5      ; reveal a 5-cell radius for the owner
Reveal=-1     ; reveal the whole map for the owner
```

## Native evidence

`Ext/WarheadType/Detonate.cpp:82-100` runs before `TransactMoney`:

```
if (reveal > 0) {
    if ((pHouse->IsControlledByCurrentPlayer() || pHouse->IsAlliedWith(pCurrent))
        && !pCurrent->Defeated && !pCurrent->Visionary)
        MapClass::RevealArea2(&coords, reveal, pHouse, 0, 0, 0, 1, 0);
} else if (reveal < 0) {
    MapClass::Reveal(pHouse);
}
```

Two gates are in series on the positive arm. `Detonate.cpp:88` asks
`PlayerPtr` (one per client in a network game): the warhead's house must be
that player (`IsControlledByCurrentPlayer`) or name it an ally
(`HouseClass::IsAlliedWith @ 0x004F9A50`, which reads the asking house's own
ally bits), and that player must not be `Defeated` and not `Visionary`. Then
`RevealArea2`'s own house gate (`0x00567AB0..0x00567B12`, pinned by
`sim/vision/reveal_area_tests.rs::the_house_gate_matches_native`) admits only
the map of the owner or of a house the owner names an ally while
`AllyReveal=yes`. The negative arm is unconditional inside `if (pHouse)`: it
sets that house's map-clear byte and explores every allocated cell. Neither arm
runs without a firing house.

## Where VERA20K owns it

- `rules::warhead_type::WarheadType::reveal`, read by
  `WarheadType::from_ini_section` through `ini_value::IniSection::read_int`.
- `sim::combat::crit::detonate`'s prefix, before TransactMoney:
  - negative: `Simulation::reveal_map_for_owner`
    (`sim/world/shroud_refresh.rs`, `MapClass::Reveal @ 0x00577D90`).
  - positive: `Simulation::warhead_reveal_admits_current_player` answers
    `Detonate.cpp:88`'s gate for the current player, then
    `vision::direct_reveal_viewers` picks the client maps the RevealArea gate
    admits and `Simulation::reveal_area` runs the native final0/final1 pair at
    the impact cell.
  - no firing house: no-op.

## Residuals

- `PlayerPtr->Visionary` has no VERA field: the game keeps that byte on
  `HouseClass`, VERA stores no such state, so this term cannot veto a reveal.
  Everything else of `Detonate.cpp:88` is ported.
- With no current House there is no `PlayerPtr`, so the positive arm writes
  nothing; native always has one.
- In a network game native asks the gate once per peer, each client's
  `PlayerPtr`; VERA asks it once, for the local current House, and then writes
  the admitted client maps. Single-player and every local client agree with
  native; a mixed peer outcome is untested.
- Retail/visual parity is untested here (no retail `ini/` in this checkout).
