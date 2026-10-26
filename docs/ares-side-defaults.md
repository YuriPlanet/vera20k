# Ares fourth-side defaults

Build with `--features ares`. The stock build and INIs without Ares Side
sections keep their existing three-side behavior.

`[Sides]` already registers any number of Side identities; a Country's
`Side=` selects one. Ares 3.0 attaches a `SideExt` to every allocated Side.
For a fourth or later side, the getters fall back to the Allied stock values,
while a side's own section can replace them:

| Key | Fourth-side default (Ares 3.0) | Consumer |
| --- | --- | --- |
| `Crew` | `[General] AlliedCrew` | `sim::crew_survival` |
| `Engineer` | `[General] Engineer` | building survivor engineer |
| `Technician` | `[General] Technician` | armed survivor technician roll |
| `DefaultDisguise` | `[General] AlliedDisguise` | spy/perma-disguise construction, GetDisguise |
| `SurvivorDivisor` | `[General] AlliedSurvivorDivisor` | building survivor count |
| `AI.BaseDefenses` | `[AI] AlliedBaseDefenses` | AI base-defence candidates |
| `AI.BaseDefenseCounts` | `[General] AlliedBaseDefenseCounts` | BasePlan Recalc/preflight |
| `ParaDrop.Types` + `ParaDrop.Num` | `[General] AllyParaDropInf/Num` (both keys required) | generic paradrop |
| `ParaDrop.Aircraft` | `PDPLANE` resolved at SideExt initialization; `none` falls back to `PDPLANE` | paradrop plane |
| `Parachute.Anim` | unset: the existing `[General] Parachute=` / NPSE `VehicleParachute=` owner; `none`: `PARACH` | attached canopy |
| `ToolTipColor` | `164,210,255` | in-game tooltip text/border |
| `MessageTextColor` | runtime ColorScheme 21 (`[Colors]` entry 10) | map-action 11 message |
| `Sidebar.MixFileIndex` | 1 | sidebar MIX |
| `Sidebar.YuriFileNames` | no | sidebar filenames |
| `EVA.Tag` | index 0 (Allied) | EVA queue |

The SideExt reader also retains `HunterSeeker`, the seven campaign and
multiplayer score-screen filenames, four score themes, `GraphicalText.*` and
`DialogBackground.*`. Those values have no consumer in this chain yet.

## Reader semantics

`SideExt::constructor @ 0x1002D9F0` leaves the fields at their zero/`-1`
member defaults. `SideExt::initialize @ 0x1002E190` applies the ID defaults
above on the first reached pass. The reader at `0x1002E450..0x1002EA4C` then
reads the current INI in native order:

`AI.BaseDefenseCounts`, `AI.BaseDefenses`, `Crew`, `Engineer`, `Technician`,
`DefaultDisguise`, `EVA.Tag`, `Parachute.Anim`, `ParaDrop.Aircraft`,
`ParaDrop.Types`, `ParaDrop.Num`, `Sidebar.MixFileIndex`,
`Sidebar.YuriFileNames`, `ToolTipColor`, `SurvivorDivisor`,
`MessageTextColor`, `HunterSeeker`, then the score/graphic/dialog keys.

Every reader keeps its current field when ReadString returns 0, so an
absent or empty key retains, and so does an unknown type or scheme name.
Only `none`/`<none>` (`0x10003950`) stores null: a type reference becomes
explicitly unset, an index (`ParaDrop.Aircraft`, `MessageTextColor`)
becomes -1, and a 0x20 fixed string becomes empty. A registered type name
sets the field. Vectors follow `NullableVector` (`0x1002EB00`/`0x1002EB60`):
`<default>` returns to the getter fallback and any other nonempty value
replaces the vector, skipping unknown type names. `ParaDrop.Types` keeps
only InfantryType and UnitType identities. Fixed strings keep their first
31 bytes. `MessageTextColor` resolves through the same kind-1 ColorScheme
lookup (`0x1001BB30`) the Country color reader uses, over every `[Colors]`
entry retained so far.

## Evidence

Native spans read from the installed Ares 3.0 DLL (the NPSE `Ares.dll`, the
same build as `tools/rules_oracle/ares_side_sidebar.py`):

- default getters: Crew `0x1002DE60`, Disguise `0x1002DEA0`, Engineer
  `0x1002DEE0`, SurvivorDivisor `0x1002E130`, Technician `0x1002E170`,
  BaseDefenses `0x1002DDD0`, BaseDefenseCounts `0x1002DD40`, ParaDrop.Num
  `0x1002DFB0`, ParaDrop.Types `0x1002E060`, Parachute.Anim `0x1002E0F0`;
- consumers: building crew count `0x1004C6E0`, building crew `0x1004C650`,
  techno crew `0x1004C780`, disguise `0x52267D`/`0x6F422F`, base-defence
  picks `0x507BCA`/`0x507DBA`/`0x507FAA`, paradrop plane and canopy through
  the Country getters `0x1001BCB0`/`0x1001BBC0`, tooltip `0x72F440`,
  message color `0x6DE0D3`, trigger side `0x1002F2D0`;
- SideExt state is retained on the existing `ProcessedType` owner
  (`src/rules/native_processing.rs`), projected once into `RuleSet`
  (`src/rules/ruleset.rs`), and shared by the consumers above.

## Residuals

- **Country selection metadata** (`File.Flag`, `ListIndex`,
  `RandomSelectionWeight`, `AIRandomSelectionWeight`, observer flag and
  background, `File.Taunt`) is not parsed or consumed. Fourth-side Countries
  are already selectable through the registry, but show no stock flag; a
  modifier using these keys gets the stock ten-country presentation.
- **Score screens and themes**, `GraphicalText.*` and `DialogBackground.*`
  are retained but not drawn; those are separate UI mechanisms.
- **HunterSeeker** is retained but the Hunter Seeker superweapon launch is
  not ported, so it has no effect.
- **Vehicle entries in `ParaDrop.Types`** are retained but the passenger
  owner resolves InfantryType names only; such entries board no passenger.
- A custom `AI.BaseDefenseCounts` vector shorter than the difficulty row is
  rejected by preflight as a missing difficulty; the native read past the
  vector is undefined data.
- **Paradrop layers.** Ares replaces the paradrop launch
  (`SuperClass_Launch` hook `0x6CC390` -> `0x10074350`) and reads the
  Super's own `ParaDrop.*` keys, then the Country's (`0x1001BC20`/
  `0x1001BCB0`), then the Side's. VERA keeps the stock launch with only the
  Side layer. Soviet and any side >= 3 keep the stock unchecked list pair,
  where the Ares sender (`0x100741A0`) sends nothing unless both lists are
  equally long and nonempty.
- **Canopy layers.** The Ares canopy hook (`0x5F5ADD` -> `0x10047C80`)
  takes the dropped type's own `Parachute.Anim`, then the Country's, before
  the Side's. VERA reads only the Side layer.
- The Ares save layout gained the new fields; saves made before this change
  do not load in Ares builds.
- With Ares and NPatch compiled together, `Parachute.Anim` takes precedence
  over NPSE `VehicleParachute=` and the stock canopy. The installed DLL's
  hook order at the shared `0x5F5ADD` site is not separately proven.
- The fourth side's sidebar geometry uses the existing nonzero (Soviet)
  layout while its files come from `Sidebar.MixFileIndex` /
  `Sidebar.YuriFileNames`. Ares does not add a fourth geometry family.
