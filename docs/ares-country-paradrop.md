# Ares country paradrop overrides

Build with `--features ares`. Ares reads a paradrop block for every country and
consults it in front of the SideExt values this support already feeds, so a
house that sets these keys paradrops its own list, plane and canopy.

| Key | Resolution |
| --- | --- |
| `ParaDrop.Types=` | `GetParadropContent @ 0x1001BC20`: while the country's list is non-empty it replaces the side's list. `remove_non_paradroppables` (`0x100768C0`) keeps InfantryType and UnitType identities and drops unregistered names. |
| `ParaDrop.Num=` | Taken together with the type list. The sender `0x100741A0` sends nothing unless the two lists are equally long, so a country type list with no or too few counts sends nothing and never falls back to the side. |
| `ParaDrop.Aircraft=` | `GetParadropPlane @ 0x1001BCB0` (field `+0x154`): the country index unless it is `-1` (never set, or `none`), then the side's `+0x74`, then `PDPLANE`. |
| `Parachute.Anim=` | `GetParachuteAnim @ 0x1001BBC0` (field `+0x158`): the country animation, else the side's getter at `0x1002E0F0`, else the literal `PARACH`. The canopy hook reaches it only when the type names no parachute of its own. |

Each key is `Valueable`, not `Nullable`. An absent or empty value keeps the
field (ReadString returns 0), and so does an unknown type name; `none`/`<none>`
clears the plane and canopy, and a list value replaces the whole list. There is
no `<default>` form: leaving the type list empty is what defers to the side.
The block is read by the common country reader `0x1001C5D0`, which
`0x1001C530` runs for every INI (Rules, mode and map), right after the one-shot
`ParentCountry=` copy (`HouseTypeExt::InheritSettings @ 0x1001BDC0`), so a
child's own keys override what it inherits. Which units a paradrop sends is
simulation state, so the fields enter both the retained country extension and
the state hash under `ares-country-paradrop-v1`.

## Consumers

- `sim::superweapon::paradrop::launch` case 5 resolves the acting house's
  country first, then the side lists, then the `[General] *ParaDropInf` pair,
  and prefers the country's plane before the SideExt plane and `PDPLANE`.
  Country lists always take the sender's equal-length check.
- `sim::movement::parachute_descent::attach_parachute_anim` answers the canopy
  name through that same country → side → rules order.

## Residuals

- Ares's own `[SuperWeaponType]` `ParaDrop` plane/subject lists
  (`SWTypeExt::ParaDrop`, consulted by `SW_ParaDrop::SendParadrop`) are not
  ported, so a superweapon that names its own lists still uses the country,
  side and `[General]` chain here.
- A vehicle entry in the country list resolves to a type identity but boards no
  passenger: the passenger owner resolves InfantryType names only (the same
  limit the side list has).
- The plane and canopy country adapters ride the already-tested side chains;
  the regression tests cover the list precedence, the types-without-counts
  answer and the read/inherit rules for all four keys.
