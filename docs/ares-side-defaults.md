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
| `ParaDrop.Types` + `ParaDrop.Num` | `[General] AllyParaDropInf/Num` (both keys required) | generic paradrop, behind the [country layer](ares-country-paradrop.md) |
| `ParaDrop.Aircraft` | `PDPLANE` resolved at SideExt initialization; `none` falls back to `PDPLANE` | paradrop plane |
| `Parachute.Anim` | unset: the existing `[General] Parachute=` / NPSE `VehicleParachute=` owner; `none`: `PARACH` | attached canopy |
| `ToolTipColor` | `164,210,255` | in-game tooltip text/border |
| `GraphicalText.Image` + `GraphicalText.Palette` | unset: InitGame's literal `GRFXTXT.SHP` / `.PAL` | victory/defeat banner (`app::presentation::outcome_banner`) |
| `CampaignScore.Background` / `.Transition` / `.Animation` / `.Palette` | the side's row of `SideExt::initialize` (Allied `ASCRBKMD/ASCRTMD/ASCRAMD` + `ASCORE.PAL`, Soviet `SSCRBKMD…` + `SSCORE.PAL`, Yuri and later sides `SYCRBKMD…` + `YSCORE.PAL`) | Campaign StandaloneScore art (`render::main_menu_shell_chrome::build_campaign_score_art`) |
| `MultiplayerScore.Background` / `.Palette` | `MPASCRNL.SHP` + `MPASCRN.PAL`, `MPSSCRNL.SHP` + `MPSSCRN.PAL`, `MPYSCRNL.SHP` + `MPYSCRN.PAL` | Skirmish score screen art (`build_score_art`) |
| `MultiplayerScore.Bars` | `mpascrnlbar~~.pcx` / `mpsscrnlbar~~.pcx` / `mpyscrnlbar~~.pcx` | the ten score bands; the first `~~` takes the two-digit band number |
| `CampaignScore.UnderParTheme` / `.OverParTheme` | `SCORE` | theme the campaign dialog `0x103` starts on `WM_INIT`, picked by the same `0x6C9231` comparison as the rank strings |
| `MultiplayerScore.WinTheme` / `.LoseTheme` | `SCORE` | theme the skirmish dialog `0x108` starts, picked on the local player's `HouseClass+0x1F5` defeated flag |
| `MessageTextColor` | runtime ColorScheme 21 (`[Colors]` entry 10) | map-action 11 message |
| `Sidebar.MixFileIndex` | 1 | sidebar MIX |
| `Sidebar.YuriFileNames` | no | sidebar filenames |
| `EVA.Tag` | index 0 (Allied) | EVA queue |

Each score name is substituted on its own, the way `0x1002F270` copies every
SideExt field over gamemd's buffer, so an Ares build never selects the 640-wide
`*SM.SHP` shapes: `SideExt::initialize` always holds a medium name. The SideExt
reader also retains `HunterSeeker` and `DialogBackground.*`; those have no
consumer in this chain yet. `GraphicalText.*` is consumed by the
victory/defeat banner: `Game_LoadUI_LoadSideData` (`0x1002F4A0`) loads the
local side's two files into the overlay slots through the writer at
`0x1002ECC0`, and the draw hook `0x1002FAD0` (patch site `0x6D4E79`) reads each
slot through `0x1002DF00` / `0x1002DF10`, which fall back to InitGame's literal
`GRFXTXT` surface per slot.

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

- The skirmish theme name is resolved once, when the score model is built,
  while native re-reads the side's field at each `0x5C9B75` call; a local
  player whose `HouseClass+0x1F5` changes after the terminal snapshot keeps the
  theme the dialog already started.
- `DialogBackground.*` is retained but not drawn; the dialog background is a
  separate UI mechanism (`0x621E90`'s paint).
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
  `0x1001BCB0`), then the Side's. VERA keeps the stock launch with the
  [Country](ares-country-paradrop.md) and Side layers but no per-Super
  layer. Side lists for Soviet and any side >= 3 keep the stock unchecked
  list pair, where the Ares sender (`0x100741A0`) sends nothing unless both
  lists are equally long and nonempty.
- **Canopy layers.** The Ares canopy hook (`0x5F5ADD` -> `0x10047C80`)
  takes the dropped type's own `Parachute.Anim`, then the Country's, before
  the Side's. VERA reads the Country and Side layers but not the type key.
- The Ares save layout gained the new fields; saves made before this change
  do not load in Ares builds.
- With Ares and NPatch compiled together, `Parachute.Anim` takes precedence
  over NPSE `VehicleParachute=` and the stock canopy. The installed DLL's
  hook order at the shared `0x5F5ADD` site is not separately proven.
- The fourth side's sidebar geometry uses the existing nonzero (Soviet)
  layout while its files come from `Sidebar.MixFileIndex` /
  `Sidebar.YuriFileNames`. Ares does not add a fourth geometry family.
