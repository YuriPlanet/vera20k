# Reveal houses

`RevealHouses` chooses which players a techno's sight reveals to. Three
layers, each `AffectedHouse`:

- `[TechnoType] RevealHouses=` (`Nullable`, `Ext/TechnoType/Body.cpp:1207`)
- `[Country] RevealHouses=` (`Nullable`, `Ext/HouseType/Body.cpp:30`)
- `[AudioVisual] RevealHouses=` (`Valueable`, default `Team`,
  `Ext/Rules/Body.cpp:711`, constructor `Ext/Rules/Body.h:1063`)

`Team` is `Owner | Allies`; other values are `owner`, `allies`, `enemies`,
`neutral`, `all`, `none`, `others`.

```ini
[AudioVisual]
RevealHouses=team

[GAPOWR]
RevealHouses=owner   ; this building's sight never reveals to allies
```

## Native evidence

`Ext/Techno/Hooks.cpp:2562-2596` hooks `TechnoClass::See` (`0x70AF22`),
`UpdateSight` (`0x70B15A`) and `vt_entry_48C` (`0x70B1F2`). Each replaces the
`PlayerPtr` the reveal writes with `nullptr` unless

```cpp
EnumFunctions::CanTargetHouse(affectHouses, pHouse, pPlayer)
```

admits it, where `affectHouses` is the techno type's key, else the country's,
else the global. The stock `RevealArea` house gate still decides whether the
player's map takes the cells, so the key can only subtract viewers, never add
an enemy.

## Where VERA20K owns it

- `rules::phobos_globals::PhobosGlobals::reveal_houses` (`[AudioVisual]`).
- `rules::ruleset::CountryRules::reveal_houses` +
  `RuleSet::country_reveal_houses`.
- `rules::object_type::ObjectType::phobos_reveal_houses` (`Nullable`).
- `sim::world::shroud_refresh::update_object_sight_for_viewers`: after the
  stock `reveal_admits` gate, each logical client is asked
  `sim::combat::can_target_house` with the resolved flags, matching the hook's
  `PlayerPtr` substitution per client.

## Residuals

- The hook reads the single local `CurrentPlayer`; VERA applies the same
  predicate per maintained logical client map, which is the union of each
  client's own local result. It can only remove viewers because
  `reveal_admits` still keeps sight to the owner and `AllyReveal=` allies.
- `RevealToAll=` buildings use their own owner/reveal path and are not
  filtered by this key.
