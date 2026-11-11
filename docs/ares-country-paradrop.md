# Ares paradrop overrides

Build with `--features ares`. Ares hands both paradrop Types (`ParaDrop`,
`AmerParaDrop`) to its SW_ParaDrop handler, which reads planes of its own per
superweapon and falls back to a paradrop block for every country, in front of
the SideExt values, so a house that sets these keys paradrops its own list,
plane and canopy.

## Country keys

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

## Superweapon keys

SW_ParaDrop::LoadFromINI (`0x10073F70`) reads, in the superweapon's section,
planes for the default (`ParaDrop`), every Side (`ParaDrop.<Side>`) and every
Country (`ParaDrop.<Country>`), each through GetParadropPlane (`0x100739E0`):

| Key | Resolution |
| --- | --- |
| `<base>.Count=` | ReadInteger; default the list's current size, or for a new list 1 (default), the default's size (Side) or the Country's Side's size (Country). A negative count keeps no plane. |
| `<base>[.PlaneN].Aircraft=` | Plane N (N > 1; plane 1 has no `.PlaneN`): `Valueable<AircraftTypeClass*>`, Find; `none` clears it. |
| `<base>[.PlaneN].Types=` | `ValueableVector<TechnoTypeClass*>`, Find, then `remove_non_paradroppables`. |
| `<base>[.PlaneN].Num=` | Read only once `.Types` has a value; a missing `.Num`, or one of another length, empties both lists of that plane. |

Each pass parses the stored planes in place, so unset keys keep earlier values.
Initialize (`0x10073DC0`) gives an `AmerParaDrop` a default plane of
`[General] AmerParaDropInf=`/`AmerParaDropNum=` as Rules holds them on its first
read. SendParadrop (`0x10074350`) flies as many planes as the most specific of
the Country, Side and default lists has; plane i starts from the house's
fallbacks (the country keys above) and takes, from plane 1 and then plane i of
the default, Side and Country lists in that order, the lists of any plane that
has both and the aircraft of any plane that names one. Each plane
(SendPDPlane `0x100741A0`) carries every listed InfantryType and UnitType its
count times. The planes live in the superweapon's `AresSuperWeaponLaunch`,
which the superweapon metadata hashes.

## Consumers

- `sim::superweapon::paradrop::ares_launch` (SW_ParaDrop::Activate
  `0x10073CD0`, through the Ares redirect in `superweapon::fire`) resolves the
  superweapon's planes over the acting house's country, side and `[General]`
  lists, and the country's plane before the SideExt plane and `PDPLANE`.
- `sim::aircraft::drop_payload` drops a non-infantry passenger from the centre
  of the cell under the plane (AircraftClass_Paradrop_Units `0x10048BA0`, hook
  `0x415CA6`).
- `sim::movement::parachute_descent::attach_parachute_anim` answers the canopy
  name through that same country → side → rules order.

## Residuals

- A paradrop plane that Put refuses stays in limbo with its passengers, as Ares
  leaves it; VERA never puts it on the map either.
- An unregistered name in the `[General]` lists boards nothing (the stock
  unknown-name residual in `sim::superweapon::paradrop`); the plane carries its
  other types.
- The plane and canopy country adapters ride the already-tested side chains;
  the regression tests cover the list precedence, the types-without-counts
  answer and the read/inherit rules for all four country keys.
