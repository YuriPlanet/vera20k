# Optional Phobos deployable-building alternate cameo

Build with `--features phobos` to let a building that undeploys show its
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

## Where VERA20K owns it

- `app/presentation/sidebar_render.rs` — `shows_alt_cameo` and
  `undeploying_building_is_veteran`.

VERA models one ground veteran flag (`HouseState::veteran_units`), so the
naval variant Phobos reads separately cannot differ here.
