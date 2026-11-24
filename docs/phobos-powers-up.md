# Powers-up host list and owner filter

Phobos lets one upgrade building type serve several host types and filters who
may install it (`Ext/BuildingType/Body.cpp:185-186`; the legacy
`PowersUpBuilding` string is kept in sync at `:270-277`; admission in
`BuildingTypeExt::CanUpgrade`, `Ext/BuildingType/Hooks.Upgrade.cpp:6-34`, hooks
`0x452678`/`0x4408EB`).

```ini
[UPGRADE]                     ; BuildingType (the upgrade)
PowersUp.Buildings=           ; host building types, default empty
PowersUp.Owner=Owner          ; AffectedHouse, default Owner
PowersUpBuilding=             ; legacy single host, kept in sync
```

## Behavior

- `PowersUp.Buildings` lists the host types the upgrade may be installed on.
  When the legacy `PowersUpBuilding` string is empty, the first list entry is
  copied into it; when the legacy string is set, its type joins the list if
  missing. Native code that still reads the single string therefore sees a
  consistent host.
- `PowersUp.Owner` is the House relation between the installing owner and the
  host's owner (`AffectedHouse`); the default `Owner` requires the same house.
- The upgrade is admitted when the type exists, the owner filter admits the
  relation and the host's type appears in the list.

## Where VERA20K owns it

`rules::building_upgrade::BuildingUpgradeRules` reads both keys and performs
the legacy sync in its per-pass reader. `hosts()` is the single derivation of
the native host set (legacy field first, then every other list entry once);
`host()` keeps the native legacy string for the code that wants that field or
a boolean upgrade test. The single admission owner
`sim::building_upgrades::can_install_building_upgrade` uses the list and the
`can_target_house` filter under `phobos`; stock builds keep the legacy
`PowersUpBuilding` equality check.

Every runtime consumer that asks "which host carries this upgrade" walks
`hosts()`:

- Ares `HouseOwnsSpecific100082B0` / `PrerequisitesMet10021CA0`
  (`sim::production::can_build::specific_prerequisite_on_map`) admits the type
  when **any** declared host is on the map and carries the upgrade in one of
  its three slots.
- Ares `CountOwned10020AF0` (`sim::production::can_build::
  ares_owned_build_limit_count`) sums the upgrade's installed slots over every
  declared host, so `BuildLimit=` sees all of them.
- `Simulation::count_owned_building_upgrades` (Phobos
  `BuildingTypeExt::GetUpgradesAmount`) already reads the whole list and feeds
  the Phobos `BuildLimitGroup.*` census.
- `sim::ai_build_need` reads no host name: it consumes the generic
  prerequisite result, so it follows the same set.

## Residuals

- None for host selection: installation, prerequisites, build limits and the
  AI's prerequisite path all consult the same native host set. Stock (non-Ares)
  gamemd keeps its own single-name `PowersUpBuilding` behaviour, as before.
