# Random map Extras

A VERA feature for Skirmish, LAN and online rooms; the original game has no such view.

In the Generate Map dialog, the Advanced button steps from Basic to **Advanced**, then to
**Extras**. The Advanced view shows seven sliders at a time; its own vertical scrollbar (arrows or
drag) scrolls the remaining six into view. The Extras view has tools for sharing a map and choices
that add to the map the generator makes. Every choice starts at Original or Off, and a map with all of them
there is the same map as before. In a room the host's Extras travel with the random map's options,
and every player's game builds the same map from them.

After generating a map, click **Save** and give it a name. To play it later,
open **Choose Map → Random Maps**, select its name and click **Use Map**.
VERA keeps new saves in `Maps/Random Maps/`. The folder in Choose Map also
lists seeds saved by earlier versions.

## Advanced — building sliders

Three VERA sliders open the Advanced scroll range after the original seven. All are
0..100 and default to 0, which leaves the original generator's behaviour untouched:

| Slider | What it does |
|---|---|
| Tech Buildings | 0 keeps the original random count of neutral tech buildings; higher seats more, up to 40. |
| Derelicts | Stands neutral, unpowered combat buildings — construction yard, barracks, battle lab and war factory — on open ground away from the starts, taken from the active Rules' `[AI]` build lists. |
| Damage | Randomly damages every neutral building across the whole map. Higher values wreck more buildings and leave them at lower health, for a war-torn city. |

Derelicts are always unpowered and damaged. Damage applies to tech buildings, towns, farms and
derelicts alike; where it overlaps the War Damage Extra, the worse health wins. All three are
saved in the `.SED` (`VeraTechBuildings`, `VeraDerelicts`, `VeraDamage`) and ride in the share
code; a code from before they existed reads back with all three at 0.

The Tech Buildings slider keeps the seed's original placements and adds buildings on open
ground toward the requested total. On a crowded map fewer may fit. Derelicts start powered
down; tech buildings and towns keep their usual power.

## Advanced — neutral foot sliders

Three more VERA sliders follow the building sliders in the Advanced scroll range. All are 0..100
and default to 0, which adds nothing. Each scatters foot-sized units over clear open ground — and,
for the types the rules say stand only on water, over open sea — away from the starts, on the
**Neutral** house, from a random stream of its own. Objects share the available space. The seed keeps its terrain, ore and starts.

| Slider | What it does |
|---|---|
| Civilians | Adds civilian infantry and civilian vehicles (`Civilian=yes`, or a `TechLevel=-1` vehicle that cannot start in multiplayer — the buses, taxis and trucks a town already uses). 0 keeps only the original town foot traffic. |
| Animals | Scatters the rules' `Name=Animal …` units: cows, bears, monkeys, camels and the dinosaurs. Water-only kinds, such as the Bront (`MovementZone=Water`), go into the sea. |
| Neutral Defenders | Leaves player-buildable units neutral and hostile — the infantry, tanks and navy a human could build (E1, GI, the tanks, destroyers, submarines, …). Good for guarded ruins, roaming patrols and a contested sea. |

A type is picked for the Neutral Defenders from `TechLevel>=0`, an owner, a construction
prerequisite and a weapon; the support and transport types a player can build are left out (the
miners, the engineer, the spy, the MCV) and so are the choppers, which need an airfield pad.
`AllowedToStartInMultiplayer` is deliberately not the test: retail rules mark 138 types `no`
there — every navy hull, the Guardian GI and the tanks' heavier cousins among them — which used
to narrow the slider to six basic infantry and tanks. Navy hulls and other water-only kinds go on
the water, so a raised slider garrisons the sea as well as the land.

The types are read from the rules, never hardcoded, so a mod gets its own civilians, animals and
buildable units for free: only types the mod defines can appear. Infantry keep their infantry
category and vehicles their vehicle category, exactly as the game's type registry has them. These
sliders save as `VeraCivilians`, `VeraAnimals` and `VeraWild` (0..100, written only when above 0)
and ride in the share code; a code from before they existed reads back with all three at 0.

## Tools

- **Share Code:** copies a short code of the options. A friend who uses it gets the same map.
- **Use a Code:** reads a code from the clipboard and builds that map at once.
- **Previous Map:** goes back to the map generated before this one.
- **My Preset:** keeps the options as one of up to five presets of your own, which the Presets
  button then offers.

## Choices

| Row | Values | What it does |
|---|---|---|
| Ore Fields | Original, 1 to 9 | Draws other ore fields on the same land. |
| Towns | Original, 1 to 9 | Draws other towns on the same land (with Buildings above 0). |
| Base Room | Off, On | Clears the trees round every base, and prefers maps whose tightest base has more room. |
| Sea Access | Off, On | Prefers maps where every base has about the same water nearby. |
| Fair Derricks | Off, On | Gives every base one tech building at the same distance: an oil derrick unless `FairTech=` names another. |
| Centre Gems | Off, Small, Large | Lays a gem field at the middle of the map. |
| Roads | Off, On | Paves a road from every base to the middle of the map. |
| Mood | Original, Golden Hour, Red Dusk, Moonlight, Overcast, Toxic Haze | Lights the map in a mood instead of the light of its time of day. |
| War Damage | Off, Some, Heavy | Town and farm buildings start damaged. Tech buildings stay whole. |
| Countryside | Off, Some, Many | Scatters farmsteads over open land, away from the bases. |
| Landmark | Off, On | Stands one monument at the middle of the map. |
| Town Fences | Off, On | Lines the sides of town buildings with fences; the streets stay open. |

Base Room and Sea Access search the seeds the way Fair Starts does. The other choices run after the
generator has finished, each from a random stream of its own, so they never move what the seed made.

## For modders

A mod can change what the Extras place and how the moods light a map, in an `[Extras]` section of
its own `RMGMD.INI`: a copy of the game's file, which then replaces it (see [modding](modding.md)).
The game's own copy has no `[Extras]`, so the defaults below apply:

```ini
[Extras]
Farms=CAFARM01,CAFRMA,CABARN02,CATS01,CAFARM02,CAFRMB,CAWT01
Landmarks=CAWSH12,CAPARS11,CAEURO05,CASTL04,CAWASH19,CAWASH14,CAWASH15,CAMEX01
FairTech=CAOILD          ; one of [AI] NeutralTechBuildings=
; Derelicts defaults to the Rules' [AI] BuildConst, BuildBarracks, BuildTech
; and BuildWeapons lists. Set it here to override that derivation.
FarmFence=CAFNCW         ; an overlay
TownFence=CAFNCB         ; an overlay
GoldenHour=90,125,98,55  ; ambient light, red, green, blue, in percent
RedDusk=62,135,70,55
Moonlight=52,55,85,130
Overcast=68,88,92,98
ToxicHaze=70,80,120,60
```

- Lists split on commas, and the blanks round a name are dropped.
- A building is used only where the rules define it and its art exists on the map's theater.
- The `Farms=` buildings of more than one cell are the farmhouses a farmstead is built round; the
  rest stand beside them.
- `Derelicts=` are the buildings the Derelicts slider stands. Absent, they are derived from the
  active Rules' `[AI] BuildConst=`, `BuildBarracks=`, `BuildTech=` and `BuildWeapons=` lists, so a
  mod's own buildings stand too. Setting it overrides the derivation. A building is used only where
  the rules define it and its theater art exists. The derived list intentionally leaves out:

  - Power plants — the game already fields the Tech Power Plant, so a derelict one is redundant.
  - Radar towers — power from a derelict plant would feed a radar and the Eiffel Tower as well, and
    a powered Eiffel Tower attacks everyone on the map. No derelict plant, no such problem.
  - Ore refineries — Yuri's refinery shoots at whatever comes near, which a neutral building
    should not.
  - The naval yard and grinder — a shipyard on open ground looks wrong, and a grinder is not a
    combat building.
- A mood takes four whole numbers, in the percents `[General] TemperateAmbientLight=` and
  `TemperateAmbientRed=`, `TemperateAmbientGreen=` and `TemperateAmbientBlue=` give a time of day. A
  mood with fewer numbers keeps its default. The level light stays the time of day's.

## Saved maps and codes

A saved random map (`.SED`) keeps each choice that is not Off as its own key in `[RandomMap]`:
`VeraOreVariant=`, `VeraTownVariant=`, `VeraMood=`, `VeraRuins=`, `VeraCountryside=`,
`VeraLandmark=`, `VeraFences=`, `VeraCentreGems=`, `VeraFairTech=`, `VeraRoads=`, `VeraBaseRoom=`
and `VeraSeaAccess=`, each a number from 1. The six Advanced sliders save as `VeraTechBuildings=`,
`VeraDerelicts=`, `VeraDamage=`, `VeraCivilians=`, `VeraAnimals=` and `VeraWild=`, each 0..100 and
written only when above 0.

A share code is `VR1-` and then groups of five characters. It holds the VERA20k version that made
it, every option, the seed and the Extras, and ends in a check, so a mistyped code is refused. It
leaves out the map's description. Letters may be in either case, and `I`, `L` and `O` read as
`1`, `1` and `0`. A code from another VERA20k version still reads, but the dialog warns that the
map may differ, because another version's generator can build another map from the same options.
New six-slider codes explicitly mark the population fields. Codes from before the sliders
load with all six at zero. An early development build used the three-slider flag for six values.
When the same bytes fit both formats, VERA reads the original three-slider layout.

## Code

- `src/map/rmg/extras.rs`: the choices and `[Extras]`.
- `src/map/rmg/embellish.rs`: what the choices, the Derelicts slider and the three neutral foot
  sliders add to a map.
- `src/map/rmg/tech_catalog.rs`: resolves the mod's civilians, animals and buildable units from the
  rules into the catalog the spawn sliders draw from.
- `src/map/rmg/build.rs` (`war_damage`, `apply_derelicts_and_damage`) and `src/map/rmg/settings.rs`
  (moods).
- `src/map/rmg/phases/tech_buildings.rs`: original tech placements. The Tech Buildings slider adds to them in `embellish.rs`.
- `src/map/rmg/options.rs`: `.SED` keys and share codes.
- `src/ui/skirmish_shell/layout.rs` and `state/random_map_setup.rs`: the Advanced rows and their
  scrollbar.
- `src/ui/skirmish_shell/state/random_map_extras.rs`: the Extras view.
- `src/app/os_clipboard.rs`: the clipboard.
