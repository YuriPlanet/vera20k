# Checked Foot Z oracle

Use this tool for ordinary Unit Foot Z composition, cliff/column/tunnel selectors,
near-bridge selection and base Z adjustment. It executes the original pinned
`gamemd.exe` instructions. The Rust consumer is
[`render::foot_depth`](../src/render/foot_depth.rs); app body/shadow presentation
calls that owner through [`instances::foot_depth`](../src/app/presentation/instances/foot_depth.rs).

```sh
python -m tools.render_depth_oracle --check
python -O -m tools.render_depth_oracle --check
python -m unittest tools.tests.test_render_depth_oracle tools.tests.test_oracle_lifecycle -v
```

Set `VERA20K_GAMEMD_EXE` or `RA2_DIR` as described in
[native setup](native_oracle.md). Import and `--help` are inert without retail.
Default invocation checks without writing. Only explicit `--write` replaces
`render_depth_vectors.json` and its `.meta.json` provenance sidecar. A failed
native execution publishes neither. Review intentional reference updates.

## Ownership and execution

`tools.native_oracle` owns original image selection/hash validation, PE mapping,
completion/timeout enforcement and vector publication/comparison. This fixture
owns supplied object/cell/TMP state, ABI resets, case generation and coverage.
There is no RMG compatibility adapter, private image hash gate, competing
instruction observer or private write/check path.

Every run must reach its declared return boundary within 100,000 instructions
and the shared runner's wall-time limit. In addition, each full Foot invocation
must visit `704240`, `703E70`, `704000` and `704350` in that same invocation.
Earlier standalone selector probes cannot satisfy that obligation. Native
initializer `49F2F0` executes before the fixture writes cached FPCW `822D80`;
its live FPCW, and every later call's live/cached word, retain `0x0E7F` chronology.
Original virtual methods and instructions remain unpatched.

Native binary SHA-256:
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
The sidecar records the binary, Unicorn core/binding, entry points, runtime
assumptions and canonical payload hash. The shared publisher hashes the producer
and shared native owner before generation and refuses to compare or publish if
one changed through metadata construction; nothing is recorded.

Schema 2 removed `generator_normalized_lf_sha256` from the result payload. All
85 cases, their list order, inputs, native results, defaults,
region hashes and coverage restrictions were compared with exact typed equality
against the pre-migration checked native run. Shared serialization sorts object
keys; this explains the larger textual JSON diff. No native numeric golden
changed and no comparison fields are silently ignored by ordinary `--check`.

## Coverage limits

The fixture supplies Unit/UnitType storage, ordinary mapped 8x8 cells with native
512-wide slots, runtime type coefficients, standard AdjustForZ multiplier and
relocated TMP headers. The direction initializer and original selector/composition
bodies execute; object constructors, INI/map loading and SHP/voxel admission do
not. The locomotor is null. Active tunnels, overlays, low bridges, harvester or
transporter alternatives, dummy/alias cells and active-game lifecycle remain
outside this corpus. It establishes bounded arithmetic/control-flow parity,
not a native rendered-pixel or full gameplay comparison. No RNG, timer or detach
mechanism is changed; all Rust production arithmetic remains untouched.

Portable tests use synthetic PE/x86 only to challenge execution, identity and
publication guards. Their output is never native evidence. Tests cover initializer
and Foot faults, HLT, exhausted budgets, omission of each required helper,
wrong-image identity, simulated concurrent producer/shared-owner edits, and preservation of existing or absent payload/sidecar
pairs on failed writes. These guards also execute under Python `-O`.

## Production regression route

Generate the existing correctly oriented cliff fixture (the older wall/cliff
mode has zero cliff-selector scores):

```sh
python -m tools.render_depth_fixture target/foot-z/cliff-back.map --cliff-back
```

Use a [map observation profile](map_observation.md) with its absolute
`launch.selected_map_file`, then capture before/after preserved builds through
`tools.map_observation` and compare them with the same tool. The fixture contains
controlled tank/infantry positions and a scenario-only
`[General] CliffBackImpassability=0` override; it does not retune ordinary maps.
A matching capture establishes production load/step/render regression evidence,
not proof that those pixels match the original executable. The Rust native-vector
and app foot-depth integration tests separately exercise the affected arithmetic
and presentation inputs. Every Rust test invocation uses `--lib`.

## Recorded validation

The validation receipt (`tools/render_depth_oracle.validation.json`, in git at 1b1a8b43e) records the checked
85-case migration, normal/optimized native reproduction, 30 focused shared/Foot lifecycle tests in both normal and optimized Python,
and 298 total Python tests (2 optional skips), 18 Foot Z Rust tests, full strict-retail library
suite (9,592 passed, 213 ignored), clippy (870 existing warnings) and release build.
The wider ignored-test and oracle inventories remain open.

The 30-tick cliff-map runs have identical release executable bytes, map input,
initial/final simulation state and atlas evidence. Full-frame comparison reports
**MISMATCH**, retained in the receipt: 11,032 pixels differ within the exclusive
rectangle `(644, 48)..(786, 158)`, entirely in the Allied radar animation. Every
pixel outside that rectangle is identical. The production renderer samples
`Instant::now()` for the radar's wall-clock animation (`render/mod.rs:77`), so
identical simulation ticks do not pin that presentation frame. This route does
not yet seal presentation clock sampling; a deterministic capture clock is a
separate tool follow-up. No comparison mask or timer change was introduced.
The current camera also does not certify visibility of every authored cliff
probe. Bounded unchanged-production evidence here includes identical executable
bytes and the matching simulation/tactical output, not an exact full-frame match.

The independent critic found a source-identity race in the initial migration.
The shared publisher now owns the pre/post generation source check; regression
tests reproduce edits to each source and preserve existing or absent references.
Only Python tooling and provenance changed after the Rust/release validation;
the native Foot payload and Rust sources remained unchanged.

The shared-owner source change also required fresh dependent native runs before
renewing current source pins. Anytown/Shrapnel completed 24 commands, including
independent checks after generation and Rust projection checks. Only ten sidecars
and two aggregate receipts changed; every golden payload and historical receipt
remained byte-identical. Three incomplete-fixture setup failures are retained in
the validation record with verified asset recovery. LineTrail also reproduced
its original payload before its source pin changed; the palette corpus passed
without rewriting its historical receipt. The replay checked 505 source files
against its frozen snapshot before publication. A wider indexed coordinator for
this dependency refresh and fixture preparation is still a tooling follow-up.

After integrating concurrent AI work from main, both oracle lifecycle entries
were retained and readiness was repeated: 298 Python tests passed (2 optional
skips), 9,600 strict-retail Rust library tests passed (213 ignored), and clippy
passed with 860 warnings. The release/capture evidence above predates that
unrelated merge; it is not presented as a new AI runtime comparison.
