# Inspect retail INI values

Build the existing asset tool, then resolve that exact emitted executable:

```sh
python -m tools.cargo_run -- build --release -p vera20k --bin asset
asset_bin=$(python -m tools.cargo_run --resolve asset --profile release)
"$asset_bin" ini-get General TreeStrength --domain rules --reader int --default 200 --map Dustbowl.mmx --mode-id 1
"$asset_bin" ini-get HoverMissile Speed --domain rules --reader raw --map Dustbowl.mmx --mode-id 1
"$asset_bin" ini-get MTNK PrimaryFireFLH --domain art --reader coord --default 0,0,0
"$asset_bin" ini-get MTNK Image --domain art --reader string --default MTNK --capacity 25
```

The defaults above are explicit example caller inputs, not an assertion about every
native caller. Choose the reader, default, capacity and scenario from the native
caller being investigated. `--ra2-dir` follows the asset tool's normal precedence.
`ini-get --help` lists the supported flags. This command writes no files.

Rules queries require an explicit map and a mode ID from the production
`MPModesMD.ini` skirmish list. Maps use the same loose-file/MIX selection and MMX
parsing as the app. RULESMD, optional LANGRULE, selected mode and map are processed
by the production Rules owner. Source timing follows app match loading: startup
selection, neutral-archive registration and the production scenario scan (retaining
loose YRO archives), map resolution, theater activation, then mode-override selection.
The older headless mode-before-theater route is not this command's claim.
The consumed MPModes roster and fixed ARTMD/SOUNDMD identities are reported
separately. The asset tool uses its explicit numbered-media archive policy; it does
not infer a media switch from command arguments. ART queries read only the selected ARTMD and reject map/mode options.
`--all-mixes` is rejected: unreachable catalog entries cannot answer what the
production loader selected.

The JSON reports each source's identity/hash and exact-case authored presence,
the processed section/key presence, and `accessor_result`. Empty INI values are
omitted by the production parser. A later malformed bool or hex int can retain a
prior value through the existing accessor; the tool does not implement a second
layering or parsing algorithm. Missing sections still apply the supplied native
reader default, including string byte truncation and trim.

`--reader` selects `raw`, `int`, `bool`, `double`, `string`, `range`, `speed`, or
`coord`. Typed readers require `--default`; `string` also requires `--capacity`.
CLI defaults are typed values: signed decimal i32, `true`/`false`, binary64 text,
a string, or three comma-separated signed decimal coordinates. Range defaults
and results use leptons; speed uses the reader's internal 0–255 scale. Double
results always include `binary64_bits` and a text representation; nonfinite
values have JSON `value: null` without losing their identity.

**An accessor result is not a final gameplay field.** A weapon's authored `Speed`
can differ from its processed weapon speed after the native projectile-dependent
post-pass. Constructors, allocation timing, per-field defaults/clamps and art
indirection must still be traced at the actual consumer. The command reports
production Rust behavior and its existing residuals; it does not certify universal
native equivalence. Campaign chronology, LANGRULE Digest handling and the
specially flagged extra TMCJ4F pass remain outside the supported path. Malformed
ReadDouble/coord recovery and extreme ReadRange conversion retain the production
readers' documented differences.

Focused checks run as library tests:

```sh
python -m tools.cargo_run -- test -p vera20k --lib asset_tools::verb_ini::
python -m tools.cargo_run -- test -p vera20k --lib asset_tools::args::
python -m tools.cargo_run -- test -p vera20k --lib asset_tools::verb_ini::tests::retail_yro_scenario_inspection_uses_retained_discovery -- --ignored --exact
```

The source selector transfers its snapshots into `NativeRulesProcessOwner`; it
never owns a second registry. Random-map preview and `.SED` launch obtain their
root-only terrain input and startup tech-building projection from that retained
owner. Replacing a loose INI after startup therefore cannot change those inputs.
The app-local Rules composer and source rereader have been removed.

Recorded validation (`tools/ini_lookup.validation.json`, in git at 1b1a8b43e) includes strict-retail library
and Clippy results, explicit random-map/reader witnesses, three reproduced native
corpora, release CLI cases, a headless Hills run and an unchanged production frame.
Reproduce the additional production witnesses with the existing tools:

```sh
VERA20K_REQUIRE_RETAIL_INI=1 python -m tools.cargo_run -- test -p vera20k --lib app::random_map_lifecycle_tests::gsi_04_12_random_map_ui_to_sed_launch_lifecycle_converges -- --ignored --exact
python -m tools.cargo_run -- build --release -p vera20k --bin parity-digest --bin vera20k
parity_bin=$(python -m tools.cargo_run --resolve parity-digest --profile release)
"$parity_bin" --ra2-dir "$RA2_DIR" --map Hills.mmx --seed 305419896 --ticks 30 --out <NEW_JSONL>
```

For the game capture, follow [map observation](map_observation.md) with the
checked-in example profile. Headless parity-digest has a documented 66 ms cadence;
the app capture uses 22 ms. Their hashes are not compared against each other.
No new simulation math, RNG draws, timer writes or detach calls are introduced.
The existing source-policy and reader residuals above remain explicit.
