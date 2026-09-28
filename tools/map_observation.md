# Chosen-map production observation

`python -m tools.map_observation` launches the normal production loader through
`--tactical-capture map-observe-v1`, advances the requested exact simulation steps,
and retains a hidden-window GPU readback. This is a production observation, not a
native comparator or a gameplay/pixel parity certification.

The explicit profile schema belongs to
`src/app/diagnostics/tactical_capture/map_observation.rs`. Start from
[`map_observation.example.json`](map_observation.example.json), whose launch is the
existing Rust `SkirmishLaunchSession` DTO. Change `launch.selected_map_file` to the
chosen retail map. Keep explicit countries, colors and distinct start slots, and
keep `pre_fill_house_roster` consistent with the opponents. Rust validates launch
admission; Python does not maintain a second launch parser or synthesize defaults.

Build through the shared Cargo owner, then run from the checkout with an existing
`config.toml` pointing at retail assets. Set `graphics.upscale = false` for the
native-resolution capture. All supplied paths must be absolute without symlink
ancestors; use canonical temporary paths on macOS. The output parent must exist,
and the output directory itself must not exist.

```sh
python -m tools.cargo_run -- build -p vera20k --release --bin vera20k
env -u RA2_DIR python -m tools.map_observation \
  --profile /absolute/checkout/tools/map_observation.example.json \
  --contract /absolute/checkout/src/app/diagnostics/tactical_capture/contract.v2.json \
  --cwd /absolute/checkout \
  --output /absolute/evidence/new-map-observation
```

The default executable is the unchanged release `vera20k` recorded for this
checkout by `tools.cargo_run`; it never guesses a target directory. Recorded byte
identity does not establish source freshness: build the intended revision first.
`--executable /absolute/path/to/vera20k` selects an explicit binary instead.
For preserved builds, `--build-label LABEL` asks the shared Cargo owner for that
label's verified host release `vera20k`; the two selectors are mutually exclusive.
It checks the artifact path, target/profile classification, ambiguity and actual
executable SHA, with no fallback to the latest build. For example:

```sh
python -m tools.cargo_run --label before-map-change -- build -p vera20k --release --bin vera20k
python -m tools.cargo_run --resolve vera20k --profile release --from-label before-map-change
env -u RA2_DIR python -m tools.map_observation \
  --build-label before-map-change \
  --profile /absolute/checkout/tools/map_observation.example.json \
  --contract /absolute/checkout/src/app/diagnostics/tactical_capture/contract.v2.json \
  --cwd /absolute/checkout \
  --output /absolute/evidence/before-map-change
```

The supplied contract must match the repository contract bytes. Every environment
variable in its denylist must be absent, even if set to an empty string. The
wrapper reports denied variables and does not silently change the environment.
After sourcing the native development environment, explicitly remove `RA2_DIR`
for this command as above; asset loading uses the working directory's config.

A new wrapper v2 bundle contains sealed `profile.json`, `config.toml` and
`contract.json` copies, plus `stdout.log`, `stderr.log`, `run.json` and the child's
atomically published `child-output/{capture.json,frame.bgra}`. Runtime still reads
the supplied original paths; retaining copies does not redirect the game loader.
Original files and retained copies must remain unchanged during capture.
`run.json` records exact input hashes, command, child PID/status, timeout, receipt
validation and capture artifact identities. A valid observation requires unchanged
profile/config/executable/contract files, matching profile and contract receipts,
a v2 child manifest with resident UnitAtlas statistics, zero initial tick/frame/time,
the requested final tick/frame and endpoint step
receipts, a loaded loose/MIX map digest, hidden unfocused rendering without input
violations, and correctly sized/hashed BGRA bytes. Simulation time must advance
for nonzero steps; its scheduling formula remains owned by Rust. Zero steps must
retain the initial fingerprint. Map hashes attest the bytes reported consumed by
the loader; this wrapper does not independently extract MIX entries or reimplement
the loader. Compare deterministic fingerprints separately from presentation pixels.

The shared `tools.child_process` owner bounds spawn/wait/cleanup and collects
finite output snapshots. On timeout it kills only its exact child. Existing
outputs are never overwritten. A failed child, failed manifest, missing artifact,
changed input or invalid receipt produces an invalid report when publication is
possible. Exit codes: `0` valid observation, `1` retained invalid observation,
`2` invalid inputs or publication failure. This tool never claims native parity.

Portable validation:

```sh
python -m unittest tools.tests.test_map_observation tools.tests.test_child_process
```

The saved [validation receipt](map_observation.validation.json) records the checked
release binary and source snapshot, repeated MIX-map state, loose-map and zero-step
captures, missing-map diagnostics, and the explicit coverage limits. It is a run
summary, not a native oracle golden.

## Validate and compare saved observations

Use the same owner to recheck an existing run or compare before/after captures.
These commands read evidence and write one new JSON report outside the run
directories; they do not launch a game or change the captures.

```sh
python -m tools.map_observation validate \
  --run /absolute/evidence/before-map-change \
  --output /absolute/evidence/before-validation.json
python -m tools.map_observation compare \
  --before /absolute/evidence/before-map-change \
  --after /absolute/evidence/after-map-change \
  --output /absolute/evidence/comparison.json
```

Validation reads the fixed artifact paths under the supplied run, checks actual
profile/config/contract/frame/log bytes and child receipt semantics, then
cross-checks the wrapper's copied evidence and original input identities. It does
not trust a stored `VALID` verdict or matching digest strings. The executable must
still exist at its recorded original path and match its recorded length and SHA;
a labeled preserved build makes that requirement durable. The report marks this
as `EXTERNALLY_REVALIDATED`. It cannot validate a deleted or replaced executable
from its old receipt alone.

A new wrapper v2 run validates its `SEALED_COPY` inputs without requiring the
original profile, config or contract files to remain available. It validates the
retained contract's v2 rules without requiring today's checkout to have identical
contract bytes. Offline checking does not apply the current process environment
denylist because it does not launch a child.

Both runs must validate before comparison. Profile, config and contract **bytes**
must match, including formatting; differing inputs make the comparison `INVALID`.
Executable bytes may intentionally differ, and both verified identities appear in
the report. Comparison checks complete initial/final fingerprints, map source,
exact steps, resident atlas statistics, surface format and actual BGRA bytes.
Differences name precise field paths and before/after values. There are no pixel
tolerances or omitted atlas fields.

`MATCH` means these checked observations are exactly equal for the compared
fields; it neither establishes independent execution nor certifies native parity.
`MISMATCH` means valid, comparable observations differ. `INVALID` means an input,
artifact, identity or receipt check failed. `native_comparator` and
`parity_certification` remain `NONE`. Compare exits `0` for `MATCH`, `1` for
`MISMATCH`, and `2` for `INVALID` or publication failure; validate exits `0` for
`VALID` and `2` otherwise. Reports are never overwritten.

Run bundles currently must remain at their original absolute location: command
output and wrapper artifact identity paths are cross-checked against the supplied
run. A copied or relocated bundle is rejected, and validation never follows its
stored artifact paths to read some other bundle. Same-directory and symlink-alias
comparisons are rejected. Keep reports outside both evidence directories.

### Historical captures

Wrapper v1 retained only `profile.json`. It is rejected by default with a specific
diagnostic. `--allow-legacy-inputs` explicitly rereads the original config and
contract paths and requires their actual lengths/hashes to match the old receipt.
These inputs are reported as `EXTERNALLY_REVALIDATED_UNSEALED`, never as retained
copies. Missing or changed originals fail validation; the profile copy and
original executable are checked as above. This policy also allows comparing an
old wrapper v1 run to a new v2 run when their actual input bytes match.

```sh
python -m tools.map_observation compare \
  --before /absolute/evidence/old-wrapper-capture \
  --after /absolute/evidence/new-wrapper-capture \
  --allow-legacy-inputs \
  --output /absolute/evidence/historical-comparison.json
```

Both paths still require **child** manifest v2 with recorded atlas statistics.
Earlier child v1 captures remain unsupported historical evidence; the tool does
not invent missing statistics or rewrite historical receipts.

Focused portable checks, including malformed/tampered packages, input provenance,
exact mismatches, legacy policy and binary selection:

```sh
python -m unittest tools.tests.test_map_observation tools.tests.test_cargo_run
python -O -m unittest tools.tests.test_map_observation tools.tests.test_cargo_run
```

The [checked comparison validation](map_comparison.validation.json) records
whole-suite checks, preserved-label selection, revalidated historical pairs and
new sealed production captures for this workflow. The earlier capture validation
receipt above remains historical evidence for its recorded source revision.

## Resident unit-atlas measurement

The final rendered frame records `render.unit_atlas` in the v2 child manifest;
the validated wrapper retains it as `capture.unit_atlas` in `run.json`. The
`UnitAtlas` owner reads actual wgpu texture descriptors and resident entries.
It reports resident sprite count, the last actual build's rasterized sprite count,
and each page's extent, format, dimensions, mip/sample counts and texel payload
bytes, plus their total. Page count is the length of `pages`.

Pages are currently single-layer, single-mip, single-sample D2 `R8Uint`, so the
payload is width × height bytes, including unused page space. Unsupported
descriptors fail observation rather than retaining stale byte arithmetic. The
last-build count includes rasterized shadow sprites, can differ from admitted
resident entries, and stays unchanged on a no-op refresh.

This is resident UnitAtlas texture payload for the captured scenario. It excludes
CPU caches, `VxlPoseFrameCache`, `VxlSlopeTransitionCache`, other atlases, palettes,
driver overhead and peak allocation; it does not establish 30-player saturation.
Statistics are captured with the final render evidence before readback and kept
outside deterministic simulation fingerprints. GPU allocation may differ across
adapters even for identical simulation state.

This replaces the retired `measure-atlas` binary, which estimated a hardcoded
roster and tile sizes without constructing an atlas. The unused
`bridge-oracle-compare` binary was also retired: its trace schema had no repository
producer, and its five bin-only tests covered that abandoned comparison schema,
not active bridge behavior. Current native bridge comparison owners remain in
[`anytown_damage`](spatial_oracle/anytown_damage/README.md) and
[`shrapnel_damage`](spatial_oracle/shrapnel_damage/navigation.md); their native
outputs and Rust regression witnesses are preserved.

For a headless retail growth/no-op/fresh-pack comparison on a real GPU:

```sh
VERA20K_REQUIRE_RETAIL_INI=1 python -m tools.cargo_run -- test -p vera20k --lib --release \
  render::atlas_refresh_retail_tests::retail_atlas_refresh_costs -- --ignored --nocapture
```

Set `RA2_DIR` or provide the local config for that test. It emits the same owner's
statistics at each allocation stage; fresh and grown totals need not match.

The [atlas observation validation](atlas_observation.validation.json) records
before/after production captures and the explicit retail GPU atlas refresh test.
The older `map_observation.validation.json` remains historical v1 evidence; it
has not been retroactively given statistics or new source identities.
