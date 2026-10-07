# Repository tools

Start here before writing a session-local helper. Run commands from the checkout
root with Python 3.12 or newer (the build runner alone supports 3.11). Individual native tools also require Unicorn;
see [native setup](native_oracle.md). This index currently covers the shared entry
points; the exhaustive oracle/tool inventory remains tracked in issue #746.

| Job | Owner / entry point |
| --- | --- |
| Run all Python tool and source-skill tests | `python -m tools.run_tests` (below) |
| Inspect scenario ART ownership, native checks and retail load validation | [ART registry owner](art_registry_owner.md) |
| Inspect process audio definitions, sample sources and retention evidence | [Process audio catalog owner](audio_catalog_owner.md), `asset sound` |
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
| Reproduce Unit Foot Z selectors/composition and prepare a cliff comparison map | [checked Foot Z oracle](render_depth_oracle.md) |
| Reproduce ground-height setters and Unit/Infantry placement leaves | [checked ramp-height oracle](ramp_height_oracle.md) |
| Reproduce native animation boundary decisions and stores | [checked Anim boundary oracle](anim_oracle/README.md) |
| Refresh Anytown packet source provenance after helper maintenance | [checked native replay and receipt refresh](spatial_oracle/anytown_damage/README.md) |
| Compare empty-IFV bridge pursuit, missiles, collapse, Stop and restoration | [conditional native continuation and input provenance](spatial_oracle/fv_cell_attack/README.md) |
| Read/disassemble native VAs; scan callers, fields and bytes | [native inspection](native_inspect.md), `python -m tools.native_inspect` |
| Compare Ghidra decompiles, callers and native stack frames | [Ghidra comparisons](ghidra_compare.md), `python -m tools.ghidra_compare` |
| Replay a recorded annotation pass onto another copy of the Ghidra database | [Ghidra annotation passes](ghidra_pass.md), `ApplyGhidraPass.java` |
| Reproduce Foot coordinates and bridge source-layer / reachability queries | [checked Foot bridge-layer oracle](spatial_oracle/foot_bridge_layer.md) |
| Run pinned native executable comparisons | [native oracle runner](native_oracle.md) |
| Preserve failed native execution context and diagnose timeouts | [native failure reports](native_oracle.md#investigating-a-failed-native-run) |
| Compare shell captures | `python -m tools.shell_capture_diff --help` |
| Capture and certify shell routes | [shell certification](shell_certification/README.md) |
| Capture and certify tactical routes | [tactical certification](tactical_certification/README.md) |
| Measure resident unit-atlas texture pages and sprite counts | [production map observation](map_observation.md#resident-unit-atlas-measurement) |
| Validate retained map captures or compare exact production observations | [map observation](map_observation.md), `python -m tools.map_observation compare` |
| Load a chosen retail map, step, capture and exit | [map observation](map_observation.md), `python -m tools.map_observation` |
| Run one bounded child with retained diagnostics | `tools.child_process.run_child` (shared by capture wrappers) |
| Check shell UI matrices | [exact shell matrix](exact_shell_ui_matrix/README.md) |
| Synchronize authoritative skill sources | `python tools/skill_sync.py --write`, then `--check` |
| Check that a change does not raise the number of `src/sim` struct fields any simulation module can write (CI runs it on every PR) | `python tools/sim_field_ratchet.py --base origin/main` |

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
run fail validation even if Cargo succeeds: labelled builds compare file
contents, other runs compare sizes and timestamps.

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
labels are not attached to checks or executed tests. A preserved test binary reads
its fixtures from the checkout that built it when it runs (`src/test_fixture.rs`),
so run it from that checkout while it still matches the manifest's source hash.

The manifest identifies source and executable bytes; it is **not** a hermetic
reproducibility claim. Ignored/local retail inputs, external dependencies, Cargo
user configuration and arbitrary build-script environment inputs are not sealed.
Capture and native-oracle tools retain responsibility for their own input evidence.
Debug-symbol sidecars are not copied. Retention preserves required debug objects
and sidecars; keep the source checkout when debugging a preserved binary.

Cargo runs check retention automatically, under the same build lock. A retention
pass inventories every registered cache, which takes seconds and grows with each
worktree, so the runner starts one only when it can matter:

- **Before compiling**, when the target volume is below the minimum free space
  (labelled builds also measure the saved-artifact volume).
- **After compiling**, also after failed builds, at most once every 30 minutes
  (`AUTOMATIC_INTERVAL_SECONDS`), or at once when a volume is below the minimum
  or cannot be measured. Any applied pass counts, including one that ended
  blocked or partial; a dry run does not. `--trim-cache` always runs one.

Between passes the cache can exceed its budgets; the free-space reserve is still
measured on every invocation. Defaults are **32 GiB total compiler
cache**, **4 GiB incremental cache**, and **16 GiB minimum free space** per cache
volume. Set `VERA20K_CACHE_GIB`, `VERA20K_INCREMENTAL_GIB`, and
`VERA20K_MIN_FREE_GIB`, or pass `--cache-gib`, `--incremental-gib`, and
`--min-free-gib` before `--`; they set the policy the next pass applies. Sizes
accept finite nonnegative decimal GiB.
Cache budgets are soft when protected files prevent reclaiming enough space.
The minimum free-space target also controls build admission: after cleanup, the
runner measures the target volume again and blocks Cargo below that target.
Labelled builds also check the saved-artifact volume. Measurement failure blocks
the build; it never permits unsafe deletion. This reserve cannot predict the
peak size of a future build or prevent unrelated applications consuming space.

Before a large build, check the free-space result of `--trim-cache --dry-run`. If
its minimum free-space target remains unmet, resolve the owned retention pressure before
starting another large build; preserve required files and report any remaining
shortfall. The runner enforces the free-space admission check for every Cargo
invocation using the configured target, including checks and unlabelled builds.

Preview or trim without starting Cargo:

```sh
python -m tools.cargo_run --trim-cache --dry-run
python -m tools.cargo_run --trim-cache
```

The owner records every new target (including failed, unlabelled, check and custom
target invocations) in `owned-builds/cache-roots.json`. Historical labelled roots
are adopted only when their live checkout/common Git directory and namespace
prove ownership. Unregistered, abandoned or unverifiable caches stay untouched.
Only orphaned `deps/*.rcgu.o` and complete finalized incremental sessions are eligible;
paths must belong to exact Cargo profile directories. Cross-target profiles require
an executable path reported by Cargo or retained in a preserved build manifest.
Older sessions are considered first; newest sessions are eligible if pressure
remains. Working sessions and sessions containing links or unknown files stay
protected. Shared compiler objects are eligible only when every inode alias
is accounted for in registered caches and required debug-reference paths survive.
Unknown external hardlinks remain protected; the cleaner never unlinks a required
debug path. Cache and reclaimed-space accounting counts each inode once, and
removing an alias does not claim freed allocation while another alias remains. Source, assets, native evidence,
executables, libraries, labels and debug sidecars are never deletion candidates.
Every saved executable remains protected, including library-test binaries.
Already absent debug inputs are reported separately as degraded debugging support;
their absence does not block deletion of independently verified orphan objects.
Existing dependencies remain protected even when another input of the same binary
is missing. Restored/mutated dependencies invalidate an in-progress deletion plan.
This does not restore lost symbols or claim complete debugging support.

Mach-O inspection reads every N_OSO/N_AST reference directly from every symbol
table/universal slice, including missing inputs that dsymutil's debug-map output
omits. ELF embedded DWARF uses GNU `readelf`; external/split debug information,
thin archives and PE/PDB dependency closure remain unsupported and stop trimming.
Malformed inputs, links, unknown formats and inspector failures still block deletion.

Inspection results and preserved SHA checks are reused only while executable
identity (including inode and ctime), expected checksum and inspector revision match.
Dependency presence and identity are always checked anew. Unknown inode identities
are never reused. Unchanged inspector failures are cached for at most five minutes;
changing the binary, inspector or readelf identity retries immediately. These caches
never authorize deletion without fresh inventory and dependency checks. Automatic
cleanup failure reports its reason; Cargo can proceed only when a fresh
measurement meets the configured free-space reserve. Explicit trimming returns
nonzero on inspection/deletion errors. See [retention validation](cargo_cache_validation.md).

Saved builds have a separate, explicit lifecycle. Default to unlabelled iteration;
preserve only binaries needed for active comparisons, captures or debugging.
Prefer release binaries for ordinary captures and label debug tests selectively.
Evidence should reference the shared label, manifest, source/binary hashes and
results instead of creating additional executable copies.

After a validated chain merges, review its exact owned labels. Keep the final
build and relevant control/debug binaries still required; archive manifests,
checksums, results and native inputs through their evidence owners. Preview
superseded labels, then apply the same exact selection without `--dry-run`, then
trim rebuildable compiler caches:

```sh
python -m tools.cargo_run --retire-label old-test-v1 --retire-label old-release-v1 --dry-run
python -m tools.cargo_run --retire-label old-test-v1 --retire-label old-release-v1
python -m tools.cargo_run --trim-cache
```

Retirement never selects labels by age or glob and never touches their former
checkout, compiler cache or external evidence. Original manifests and checksums
are durably recorded in `owned-builds/label-retirements/` before deletion; retired
executables themselves are not backed up. Partial failures retain progress receipts.
All selected labels are preflighted for hashes, unexpected content, links, shared
hardlinks and open files. Linux/macOS require working `lsof`; unsupported platforms
fail closed. Process visibility is limited to the invoking user; stop consumers
launched outside the shared build owner before retiring their labels. Never
select by age or glob, touch another task's work, or delete active binaries,
their required debugging/dependency files, source, assets or native evidence.
External legacy copies of Rust binaries need a checked retirement owner; do not
remove them with arbitrary `rm`. This review is manual: the runner has no
GitHub-aware saved-build retirement. Automatic retention trims compiler caches.

Legacy exported **Rust libtest** copies have an explicit, macOS-only retirement
mode. Review an exact JSON plan, preview it, then apply that same plan:

```sh
python -m tools.cargo_run --retire-export-plan /absolute/canonical/export-plan.json --dry-run
python -m tools.cargo_run --retire-export-plan /absolute/canonical/export-plan.json
```

The [export-plan contract](cargo_export_retirement.md) defines required fields.
The mode never discovers files by age, glob or GitHub state and never runs old
executables. It accepts only identified VERA Rust test harnesses in executable
Mach-O images; PE/native gamemd, apps, scripts, assets, ambiguous/stripped images,
links and shared hardlinks are rejected. Original ownership/results references
and a same-checkout retained libtest replacement must be pinned by SHA-256.
An explicit review must establish no active or required consumers. Missing debug
inputs, inspection errors or changed replacements block deletion. Every selection
preflights under the shared lock before any unlink. Original plan, identities,
provenance/results references, replacement manifest and progress are durably
recorded by the existing retirement owner in `owned-builds/label-retirements/`.
Only exact selected executable files are unlinked; directories, dependency files,
source, assets and native evidence remain. Old copies lacking pre-existing
ownership records or debug inputs remain blocked; do not manufacture metadata.

Each attempt writes `owned-builds/retention/<timestamp>-<id>.json`: selected and
removed files, allocated/logical bytes removed, observed free-space change, errors
and unmet targets. Dry runs never remove files; their projected space is an estimate.
APFS clones/snapshots and unrelated volume activity can make observed reclamation
differ from file allocation; applying a plan rechecks real free space and tries
additional eligible cold entries when necessary. Plans are never reused for deletion.

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
