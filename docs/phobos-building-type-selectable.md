# Optional Phobos type select for buildings

Build with `--features ares,phobos` to let type select pick buildings that the
retail ID match refuses.

```ini
[General]                       ; rulesmd.ini
BuildingTypeSelectable=false    ; boolean, default no
```

With the switch on, a second T press (or the sidebar's TypeSelect button) with
a building selected selects every building of that type the local player owns,
on screen first and across the map when the screen has no more of them.

## Native site

Type select's ID match (`0x00732C30`) admits an object of a listed type when
its CanBeSelectedNow virtual (vt+0x13C) answers true, or, for a building, when
its type has `UndeploysInto=` (`0x00732C61..0x00732C83`). A building's
CanBeSelectedNow (`0x00459C00`) first asks `0x00465D40`, which answers true
only for a 1x1 type with `UndeploysInto=` (Phobos names it
`Is1x1AndUndeployable`), and then the TechnoClass checks at `0x006FC030`. Both
arms need `UndeploysInto=`, so an unmodified game type-selects only buildings
that undeploy, such as a construction yard; a `GAPOWR` is never a seed.

Phobos's `BuildingClass_Is1x1AndUndeployable_BuildingMassSelectable` hook at
`0x00465D40` makes it answer true while the ID match runs and
`[General] BuildingTypeSelectable` is set, so CanBeSelectedNow admits every
building that passes its TechnoClass checks. The hook returns at once when
Ares is not loaded (`AresHelper::CanUseAres`), because both patch the same
region, so VERA compiles the switch only with both the `ares` and `phobos`
capabilities.

The working method in vanilla - hold the second T press and click the building -
never runs the ID match, which is why it already reached buildings; the tap and
the sidebar button both run `UICommands_TypeSelect`, which is the path the hook
patches.

## Where VERA20K owns it

- `rules/ruleset.rs` - `GeneralRules::building_type_selectable` and
  `RuleSet::structure_type_selectable`, the admission predicate: the retail
  `UndeploysInto=` test, or every building with the switch on.
- `app/input/entity_pick.rs` - `type_select_candidates` asks the predicate for
  a structure instead of the hardcoded `UndeploysInto` test. Both callers of
  the tap (the T key and the sidebar's TypeSelect button) and the map-wide
  stage run through it.

## Residuals

- With the switch on, native CanBeSelectedNow still applies its TechnoClass
  checks (`0x006FC030`) to each building; VERA admits every building of the
  type. Trigger: a building those checks refuse while the switch is on;
  effect: it joins the type selection; rare, presentation only.
- The held-T click and the T-held band drag reach buildings without asking the
  predicate at all, and this key does not change that. Their admission is the
  ordinary type group, so the switch only shows on the tap and the sidebar
  button.
