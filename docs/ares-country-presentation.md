# Ares country presentation

Build with `--features ares`. This covers the Ares 3.0 country keys that the
shell, loading screen and taunt stream consume. The ten stock Country IDs
(`Americans` … `YuriCountry`) keep their stock presentation wherever they sit
in `[Countries]`; any other ID takes the installed DLL's unknown-country
defaults.

## Keys

| Key | Effect |
| --- | --- |
| `File.Flag=` | Insignia surface for every `Game_GetFlagSurface` (`0x1001DCE0`, patch site `0x4E3562`, 5 bytes) consumer: the Skirmish/LAN/WOL player and opponent rows, the Co-Op score screen, the diplomacy rows and the loading progress rows. Stock IDs keep `usai.pcx` … `yrii.pcx` and other IDs `rani.pcx`. The name is lowercased and cut to 31 bytes; an empty value keeps the current file, and `none`/`<none>` clears it, which answers the hook's null-surface branch at `0x4E3567`, so the row shows `rani.pcx`. A country index outside the HouseType array (an observer or Random row) keeps the stock selection chain at `0x4E3579`. |
| `MenuText.Status=` | Status-help CSF key in the country dropdown. Stock IDs keep `STT:PlayerSide*`; other IDs keep `GUI:Unknown`. |
| `File.Taunt=` | Taunt path (31 bytes; `none` clears it) with `~~` (Ares docs) or `%02i` (stock defaults) replaced by the two-digit taunt number. |
| `ListIndex=` | Dropdown position; ties keep `[Countries]` order. A negative value hides the row. Default 100. |
| `RandomSelectionWeight=` | Human “Random” country weight. Default 1. |
| `AIRandomSelectionWeight=` | AI “Random” country weight. Optional: until a read sets it, AI rows use `RandomSelectionWeight`. `ParentCountry` does not copy it. |
| `File.ObserverFlag=`, `File.ObserverBackground=`, `File.ObserverFlagAltPalette=` | Retained observer-strip art. Its only consumers are `StripClass_Draw_DrawObserverBackground @ 0x6AA0CA` and `StripClass_Draw_DrawObserverFlag @ 0x6AA164`; VERA ports no observer strip, so no consumer yet. |

`ParentCountry=` runs a one-shot copy (`0x1001BDC0`) after the primary-Rules
block (`0x1001C770`) has read the child's `File.*` and `MenuText.Status`
keys, so the parent's flag, status, taunt and observer art overwrite them.
Only the common keys read after the copy (`RandomSelectionWeight`,
`AIRandomSelectionWeight`, `ListIndex`) override it; `ListIndex` becomes
parent + 1 unless the child sets it. The same copy applies to the
loading-screen fields.

The stock and unknown defaults are the 3.0 DLL's initializer table at
`HouseTypeExt::Initialize` (`0x1001C0F0`), called from the extension
constructor and keyed by the stored Country ID; VERA holds it in the shared
`country_loading` table. Its unknown branch at `0x1001C413`
is `rani.pcx`, `GUI:Unknown`, `taunts\tauam%02i.wav`, `ls%sobs.shp` and
`mplsobs.pal`. The country dropdown hook is `hWnd_PopulateWithCountryNames @
0x4E3A6A` (Ares function behind the `.inj` entry): it filters
`Multiplay=yes` + nonempty `UIName`, drops negative `ListIndex`, sorts by
`ListIndex` then `ArrayIndex2`, and inserts `GUI:Random` first.

Random country selection uses the Ares pool of `Multiplay=yes` countries with
a positive weight. One bounded Scenario draw over the cumulative weights
selects a country; AI rows use `AIRandomSelectionWeight`. Hidden rows are
still eligible while their weight stays positive, as the Ares documentation
describes.

## Consumers

- `RuleSet::country_flag`, `country_status_key`, `country_taunt_pattern`,
  `country_list_index`, `country_random_weight`, `country_ai_random_weight`
  project the retained CountryExt values.
- `ui::skirmish_shell::country_choices` sorts/hides; `CountryMenuCatalog`
  carries the flag and status beside each visible country.
- `SkirmishShellChromeAtlas` preloads the stock flags plus every custom
  `File.Flag` from the startup rules; the Skirmish/Co-Op rows use the catalog
  names.
- `app::loading::pump::loading_row_insignia` resolves each loading progress
  row from its own HouseType index, and `render::loading_screen_chrome` decodes
  that name into the attempt's atlas. A row for a country past index 9 now
  draws its own flag where the stock chain had no entry; an observer row keeps
  `obsi.pcx`.
- `app::input::taunt` and `SfxPlayer::play_taunt` substitute the taunt
  number into the country pattern; a missing custom pattern falls back to the
  stock byte mapping only for indices 0..9.

## Residuals

- **Observer art** (`File.ObserverFlag`, `File.ObserverBackground`,
  `File.ObserverFlagAltPalette`) is parsed, defaulted and inherited, but no
  renderer consumes it. The installed DLL hooks the two drawing sites inside
  `StripClass::Draw`: `0x6AA0CA` (6 bytes) substitutes the Country's observer
  background SHP, or blits the PCX into the sidebar at the row origin sized
  121x96; `0x6AA164` (6 bytes) substitutes the observer flag SHP with palette
  index 9 when `File.ObserverFlagAltPalette` is set and 0 otherwise, or blits
  the PCX 45x21 at row origin + (70, 70). VERA draws no observer cameo strip
  (`ui::sidebar::cameo_order` records the same page-1 gap), so porting this
  needs that strip first.
- A `File.Flag` whose PCX is missing or unparsable leaves the row without an
  insignia in VERA. Ares clears the name when the load fails
  (`0x1001C7CA..0x1001C7E5`), so gamemd sees a null surface and answers the
  `0x4E3567` `rani.pcx` branch. Only a `none` key reaches that branch here.
- The LAN lobby's own peer rows keep `side_icon: None`, so the three remaining
  `Game_GetFlagSurface` call sites in those row functions (`0x5DB239`,
  `0x5EA41C`, `0x5EA697`) have no VERA counterpart to override.
- The loading row measure call still reserves index 0's icon size
  (`insignia_measure_size`); native routes that call through the same hook, so
  a mod that clears America's `File.Flag` measures `rani.pcx` instead. A 1-2 px
  label shift at most.
- `ParentCountry` is copied in `[Countries]` registration order. A child
  listed before its parent inherits the parent's constructor/default values,
  not values read later in the same pass; Ares likewise reads countries in
  array order.
- The weighted pick uses one bounded draw over cumulative weights. The
  installed `DiscreteDistributionClass` body was not separately executed in
  this chain; the documented weight/sum probability and the Ares pool gates
  are what the production path implements.
- A negative `RandomSelectionWeight`/`AIRandomSelectionWeight` is treated as
  ineligible. Ares casts the signed field to unsigned in its distribution,
  which is an authoring error rather than a usable exclusion (use 0).
- Country presentation state is process/GUI state, not part of the simulation
  CRC. It is retained by the live Rules owner and rebuilt from the same
  Country reader on a new start.
