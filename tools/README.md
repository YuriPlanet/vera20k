# Repository tools

Start here before writing a session-local helper. Run commands from the checkout
root with Python 3.12 or newer (the build runner alone supports 3.11). Individual native tools also require Unicorn;
see [native setup](native_oracle.md). This index currently covers the shared entry
points; the exhaustive oracle/tool inventory remains tracked in issue #746.

| Job | Owner / entry point |
| --- | --- |
| Run all Python tool and source-skill tests | `python -m tools.run_tests` (below) |
| Wait for builds; test, check, lint or build the current checkout; preserve A/B binaries | `python -m tools.cargo_run` (below) |
| Find a built or preserved host executable for shell use | `python -m tools.cargo_run --resolve asset --profile release` (below) |
| Run retail corpus checks or export a decoder-baseline candidate | [retail corpus](retail_corpus.md) |
| Check hermetic gameplay fixtures against production-selected retail sources | [retail fixture contracts](retail_fixture_contracts.md), ordinary library tests with `RA2_DIR` |
| Inspect exact retail INI values through production sources and readers | [INI lookup](ini_lookup.md), `asset ini-get` |
| Select media archives without ambient argument parsing | [media policy](media_policy.md) |
| Inspect/extract/render assets | [asset browser](asset_browser/README.md), `asset` binary |
| Reproduce FireAt-tail launch goldens | [projectile fixture family](projectile_oracle/README.md) |
| Reproduce native projectile launch, timer, collision and arc-domain goldens | [projectile comparisons](projectile_oracle/README.md); shared [slope initializer](native_slope.py) |
| Reproduce palette, scanline, brightness and Ground/Level goldens | [checked palette oracle](palette_oracle/README.md) |
| Reproduce ground-height setters and Unit/Infantry placement leaves | [checked ramp-height oracle](ramp_height_oracle.md) |
| Reproduce native animation boundary decisions and stores | [checked Anim boundary oracle](anim_oracle/README.md) |
| Refresh Anytown packet source provenance after helper maintenance | [checked native replay and receipt refresh](spatial_oracle/anytown_damage/README.md) |
| Read/disassemble native VAs; scan callers, fields and bytes | [native inspection](native_inspect.md), `python -m tools.native_inspect` |
| Run pinned native executable comparisons | [native oracle runner](native_oracle.md) |
| Compare shell captures | `python -m tools.shell_capture_diff --help` |
| Capture and certify shell routes | [shell certification](shell_certification/README.md) |
| Capture and certify tactical routes | [tactical certification](tactical_certification/README.md) |
| Measure resident unit-atlas texture pages and sprite counts | [production map observation](map_observation.md#resident-unit-atlas-measurement) |
| Validate retained map captures or compare exact production observations | [map observation](map_observation.md), `python -m tools.map_observation compare` |
| Load a chosen retail map, step, capture and exit | [map observation](map_observation.md), `python -m tools.map_observation` |
| Run one bounded child with retained diagnostics | `tools.child_process.run_child` (shared by capture wrappers) |
| Check shell UI matrices | [exact shell matrix](exact_shell_ui_matrix/README.md) |
| Synchronize authoritative skill sources | `python tools/skill_sync.py --write`, then `--check` |

## Cargo ownership and labeled builds

```sh
python -m tools.cargo_run -- test -p vera20k --lib
python -m tools.cargo_run -- clippy -p vera20k --lib
python -m tools.cargo_run --label release-before -- build --locked --release -p vera20k --bin vera20k
python -m tools.cargo_run --label tests-before -- test -p vera20k --lib --no-run
python -m tools.cargo_run --resolve asset --profile release
python -m tools.cargo_run --resolve vera20k --profile release --from-label release-before
python -m unittest tools.tests.test_cargo_run -v
```

`--wait-seconds 60` bounds the wait (default one hour). Ctrl-C interrupts the
runner. It does not kill other owners. Failed commands retain their exit code,
produce no label and leave compiler diagnostics visible. Source edits during a
run fail validation even if Cargo succeeds.

One kernel lock in the shared Git directory serializes cooperating worktrees.
The runner also waits for any observed `cargo` or `rustc` process on this host.
Unwrapped Cargo can still start after that observation: all sessions must use the
runner to eliminate the check/start race. This coordinates one repository's
worktrees; independent repository clones do not share the lock.

Each worktree has a separate cache under
`<CARGO_TARGET_DIR or checkout/target>/owned-worktrees/<checkout-path-hash>`.
The first build compiles dependencies into this namespace, costing time and disk;
subsequent builds reuse it. Existing shared caches are neither deleted nor trusted.
`--target-dir`, `--manifest-path`, `--config` and `--message-format` are reserved
by the runner. Other Cargo/test arguments and the caller's environment pass through.
Tests must select `--lib`. Confirm ignored `ini/`, config and retail assets in a new
worktree as usual; the runner does not copy them from someone else's checkout.

A label preserves every executable reported by **that Cargo invocation**, including
fresh cache hits, under `<git-common-dir>/owned-builds/artifacts/<label>`.
Executables retain their original basename inside numbered subdirectories.
The printed directory's `manifest.json` gives the executable filenames, SHA-256s,
checkout, commit, dirty status, combined tracked/nonignored source hash, command,
Cargo/Rust versions and common build environment overrides. Labels cannot be
replaced. This supports builds of selected binaries and `test --lib --no-run`;
labels are not attached to checks or executed tests. Run a preserved test binary
from its checkout so relative fixtures still resolve.

The manifest identifies source and executable bytes; it is **not** a hermetic
reproducibility claim. Ignored/local retail inputs, external dependencies, Cargo
user configuration and arbitrary build-script environment inputs are not sealed.
Capture and native-oracle tools retain responsibility for their own input evidence.
Debug-symbol sidecars are not copied. Keep the source checkout and its cache when
debugging a preserved binary; the executable alone suffices for ordinary A/B runs.

`--resolve <BIN> --profile release|debug --from-label <LABEL>` selects a
preserved executable from that label. It verifies the recorded artifact path,
host/profile classification and actual SHA-256; missing, ambiguous, malformed or
changed artifacts fail without falling back to a latest build. This uses the
same preserved manifest owner as `--label`, including existing schema-1 labels.
The original build source and toolchain metadata stay in the label's manifest.

For shell use, `--resolve <BIN> --profile release|debug` prints only the absolute
verified path to stdout. Missing records/files or changed bytes produce a nonzero
exit and diagnostics on stderr. It never builds or falls back to another profile;
resolve mode accepts no Cargo arguments, label or build-wait option. For example:

```sh
asset_bin=$(python -m tools.cargo_run --resolve asset --profile release)
"$asset_bin" parse-check --all-mixes
```

Resolution checks recorded binary bytes, not whether they reflect current source.
Build after source changes; use a preserved label when exact build provenance is
needed. The Python API permits an omitted profile for the existing MCP preference.

The asset-browser MCP uses `cargo_run.resolve_binary` to discover emitted host
release/debug executables from the per-checkout record in
`<git-common-dir>/owned-builds/latest/`. That record is updated atomically after
successful builds, and discovery verifies executable bytes. Cross-target and
custom-profile builds still support explicit paths/labels; they are not auto-run
by host tooling. Conventional target paths are not a fallback.

## Python regression suite

```sh
python -m pip install -r tools/requirements-test.txt
python -m tools.run_tests --list
python -m tools.run_tests
```

Requires Python 3.12+ (including Windows junction detection). The runner finds
`test_*.py` recursively under `tools/` and authoritative `.agents/skills/`, including
folders without `__init__.py`; it excludes generated `.claude` mirrors and the
machine-local `ghidra-up` skill. It prints every module and its test count, fails
on import errors or empty test modules, and propagates test failures. Focus a
module with ordinary `python -m unittest <module> -v` when investigating a failure.
The same complete synthetic suite and skill-mirror check run on Linux, macOS and
Windows for every PR and push to main. No Cargo, game executable, GPU or retail
install is needed; Unicorn exercises synthetic x86 in the runner failure tests.
This protects native execution contracts; it does not reproduce retail goldens.

Two existing checks require private sealed evidence and are explicitly skipped
by default, even when a local `config.toml` happens to exist:

- Tactical environment preflight: the pinned archive and font from
  `tactical_certification/profiles/soviet-radar-online-v2.json`, a project `config.toml`,
  and that profile's Windows environment. Set `VERA20K_TEST_PROJECT_DIR` to the
  configured checkout (defaults to this checkout).
- Historical title differential: Windows plus `VERA20K_SHELL_GUARD`,
  `VERA20K_ORACLE_RUNS` and `VERA20K_SHELL_CAPTURE`, pointing to the original sealed
  title comparison evidence described in the shell-certification tests. Its
  expected mismatch counts describe that specific historical capture.

`python -m tools.run_tests --retail` opts into **both** checks; missing inputs fail
instead of silently passing or skipping. For just one, set `VERA20K_TEST_RETAIL=1`
and run its fully qualified unittest name. Platform-unavailable symlink creation
may also skip its rejection test with an explicit OS reason. The generated matrix
check now creates its own artifact, so it runs without a session-local target file.
