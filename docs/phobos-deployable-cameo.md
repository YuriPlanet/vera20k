# Optional Phobos deployable-building alternate cameo

Phobos is in every build; turn it on from a game's data to let a building that undeploys show its
`AltCameo=` or `AltCameoPCX=` after the local player's spy infiltrates an
enemy War Factory (`Factory=UnitType`), or when the country's `VeteranUnits=`
names the unit the building undeploys into.

Ares answers such a building by comparing the **BuildingType** pointer against
that unit-only list, which INI-produced state can never match, and it returns
before either check — so the building never shows its alternate cameo. Phobos
`TechnoTypeExt::CameoIsVeteran` fixes the arm: a `Trainable=` building with
`UndeploysInto=` asks the War Factory infiltration first, then its unit's
`VeteranUnits` entry.

Source: Phobos `src/Ext/TechnoType/Body.cpp` (`CameoIsVeteran`, the
BuildingType arm) and `docs/Fixed-or-Improved-Logics.md` ("Fixed a bug
introduced by Ares where building types that have `UndeploysInto` cannot
display `AltCameo` or `AltCameoPCX`..."). Ares-only builds keep the Ares
answer, and a building without `UndeploysInto` is untouched.

## Native anchors

The cameo itself is drawn by `TechnoTypeClass::GetCameo @ 0x00712040`, which is where the
`AltCameo=`/`AltCameoPCX=` choice is made; the sidebar reaches it through its own draw path
(`StripClass::Draw @ 0x006A9540`). The veteran arm that mis-reads a building is Ares-side
(`TechnoTypeExt::CameoIsVeteran`), so no gamemd address is cited for the fix.

## Residuals

VERA models one ground veteran flag (`HouseState::veteran_units`), so the naval variant Phobos reads
separately cannot differ here.

## Where VERA20K owns it

- `app/presentation/sidebar_render.rs` — `shows_alt_cameo` and
  `undeploying_building_is_veteran`.

VERA models one ground veteran flag (`HouseState::veteran_units`), so the
naval variant Phobos reads separately cannot differ here.
