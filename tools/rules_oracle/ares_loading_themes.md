# Ares loading music

Installed Ares adds `LoadingTheme` selection at `683C70 -> 1002FB80` and
`683D05 -> 1002FC20`. Each loading attempt resets its selected index to -2.
Campaigns read the selected map's `[Basic] LoadingTheme`. Other sessions read
from the retained process Rules INI: the actual first StartSpot Country's Side
first, then that Country. This is before `Read_Scenario684620`, not the later
mode/map Rules projection or MISSIONMD loading-screen metadata.

Both calls use the original `ReadTheme4758F0`: exact-case key, ReadString128,
empty default string. Missing/empty keeps the caller's default. A nonempty name
resolves through `ThemeClass::From_Name721210`; an unknown name becomes -1 and
therefore overrides an earlier valid Side theme. Only -2 runs the original
`From_Name("LOADING")` path; all other indexes reach the shared Play_Song at
`683D1A`. Repeat, Sound, playback, source selection and the stream remain with
VERA's existing process Theme owner.

`NativeRulesProcessOwner::loading_theme_names` reads the frozen process root
through the current retained Country/Side registry. It does not reread files,
use ParentCountry or add per-type state. The loading pump uses the already
parsed `MapLoadInitial` for campaign Basic. The Ares request occurs only in the
consuming Selected-to-Prepared transition, before the Country loading prefix
can reset/reprocess Types. Repeated Prepared visits cannot start it again; no
new once flag is needed. The original build keeps its immediate LOADING
request through the existing asset lease. The Ares change adds no simulation,
persistence or hash fields.
The existing RMG initial-map phase also generates its map synchronously; its
loading music therefore starts after that generation. Native RMG start-time
parity is a remaining presentation boundary, beyond the authored-map path here.

`ares_loading_themes.json` records exactly two new executed histories. The
campaign history covers missing/empty, valid mixed-case and unknown names. The
other history covers Side fallback, Country override, unknown overriding a
valid Side, both missing and a valid Country overriding an unknown Side.
The original Ares hooks, retail reader, From_Name and retail call tail execute;
existing EmpProbe supplies the VM/CRT/ReadString boundary, with declared
Theme/Country/Side registries. Play_Song is a request recording service here,
not native audio-device or decoding evidence. All three RNG states and code
bytes remain unchanged; no earlier corpus is regenerated. Rust replay uses the
production INI, early process registry, leased AssetManager and Theme owner.
Actual Rust test results and audible output must be recorded separately.

```text
python -m tools.rules_oracle.ares_loading_themes --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --check
```
