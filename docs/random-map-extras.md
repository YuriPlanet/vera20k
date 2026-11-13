# Random map Extras

A VERA feature for offline Skirmish; the original game has no such view.

In the Generate Map dialog, the Advanced button steps from Basic to Advanced to **Extras**. The
Extras view has tools for sharing a map and choices that add to the map the generator makes. Every
choice starts at Original or Off, and a map with all of them there is the same map as before.
Online games have no Extras view and carry no Extras.

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
- A mood takes four whole numbers, in the percents `[General] TemperateAmbientLight=` and
  `TemperateAmbientRed=`, `TemperateAmbientGreen=` and `TemperateAmbientBlue=` give a time of day. A
  mood with fewer numbers keeps its default. The level light stays the time of day's.

## Saved maps and codes

A saved random map (`.SED`) keeps each choice that is not Off as its own key in `[RandomMap]`:
`VeraOreVariant=`, `VeraTownVariant=`, `VeraMood=`, `VeraRuins=`, `VeraCountryside=`,
`VeraLandmark=`, `VeraFences=`, `VeraCentreGems=`, `VeraFairTech=`, `VeraRoads=`, `VeraBaseRoom=`
and `VeraSeaAccess=`, each a number from 1.

A share code is `VR1-` and then groups of five characters. It holds the VERA20k version that made
it, every option, the seed and the Extras, and ends in a check, so a mistyped code is refused. It
leaves out the map's description. Letters may be in either case, and `I`, `L` and `O` read as
`1`, `1` and `0`. A code from another VERA20k version still reads, but the dialog warns that the
map may differ, because another version's generator can build another map from the same options.

## Code

- `src/map/rmg/extras.rs`: the choices and `[Extras]`.
- `src/map/rmg/embellish.rs`: what the choices add to a map.
- `src/map/rmg/build.rs` (`war_damage`) and `src/map/rmg/settings.rs` (moods).
- `src/map/rmg/options.rs`: `.SED` keys and share codes.
- `src/ui/skirmish_shell/state/random_map_extras.rs`: the view.
- `src/app/os_clipboard.rs`: the clipboard.
