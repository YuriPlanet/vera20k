# Building placement adjacency filters

Phobos can filter which buildings count as adjacency providers for a placing
building type, forbid placement near disallowed types, and ignore a provider
that is still building (`Ext/BuildingType/Body.cpp:233-238`, hooks in
`Ext/BuildingType/Hooks.cpp:203-273`).

```ini
[PLACER]                            ; BuildingType
Adjacent.Allowed=                   ; building type list, default empty (all)
Adjacent.Disallowed=                ; building type list, default empty (none)
Adjacent.Disallowed.Prohibit=false  ; bool, default false
Adjacent.Disallowed.ProhibitDistance=0 ; integer, default 0
NoBuildAreaOnBuildup=false          ; bool, default false
```

## Behavior

The proximity check walks the ring of cells around the placement footprint at
the placing type's `Adjacent=` distance (each adjacent cell's first ground
building counts). A provider whose own type sets `NoBuildAreaOnBuildup=yes`
contributes no build area while that provider's current mission is
`Construction`; the check runs before the allow list, reads the provider's
live mission (never its queued one) and the provider's type flag (never the
placing type's), and an idle provider of the same type still counts.
`Adjacent.Allowed`, when non-empty, keeps only the named building types as
providers. A same-house building of a type in `Adjacent.Disallowed` is skipped
as a provider; with `Adjacent.Disallowed.Prohibit=yes` it instead makes the
placement invalid. `Adjacent.Disallowed.ProhibitDistance` above zero first runs
the whole proximity check over a larger ring (that distance) with the
disallowed list fatal; the normal ring then ignores the disallowed list. The
check is ring-local: a disallowed building outside the scanned ring never
matters.

## Where VERA20K owns it

`rules::phobos_adjacent` reads the five type keys. The single placement
proximity owner `production::production_placement::is_within_build_area`
(the port of `DisplayClass::PassesProximityCheck`/Display4A8EB0) applies the
buildup skip, the allow filter, the same-house disallow skip/fatal and the
prohibit pre-pass through `build_area_scan`; stock builds run the unchanged
scan.

## Native anchors

`DisplayClass_BuildingProximityCheck_BuildArea` (`0x4A8FD7`,
`Ext/BuildingType/Hooks.cpp:236`) is the buildup skip and both filter passes
(`:245` for the skip, `:251`/`:254` for the lists);
`CellClass_DamageWall_AdjacentWallDamage` (`0x480EA8`, `Ext/Cell/Hooks.cpp:5`)
is the wall-damage visit the adjacency keys restrict; no second site reads
them.

## Residuals

None from this hook: `Adjacent.*`, the prohibit pre-pass and
`NoBuildAreaOnBuildup` are ported.
