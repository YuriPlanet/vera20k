# Retail corpus checks and decoder baselines

The retail corpus checks belong to `asset_tools::retail_corpus` in the library.
They exercise archive inventories, parser structure and byte/value comparisons,
plus separate decoder regression digests. Use the existing coordinated build
runner from the checkout root:

```sh
python -m tools.cargo_run -- test -p vera20k --lib asset_tools::retail_corpus:: -- --ignored
```

This selects the retail corpus namespace only. Do not run global `--ignored`:
other ignored tests need different fixtures, a GPU, private evidence or an
explicit performance run; some record intentional divergences. The corpus suite
requires no game window, GPU, emulator or `gamemd.exe`. Supply `RA2_DIR` (or a
valid checkout `config.toml`) pointing to the retail archive install. Selected
checks fail when prerequisites are absent. `VERA20K_REQUIRE_RETAIL_INI` controls
a different extracted-INI requirement and does not enable this suite.

The committed reference is `tests/fixtures/retail_goldens/manifest.json`, resolved
from the compile-time checkout path. It records 64 archives under 16 roots,
11,854 directly sniffed assets, 3,438 audio-bag entries and ten named decoder
results. Those counts describe this reference corpus, not every supported install.
The corpus explicitly mounts all disk MIXes, including archives the game's normal
startup skips, while pinning digital-install media index 2. Unknown formats are
outside the sniffed corpus; unreadable indexed entries are errors.

Archive names, traversal changes and input changes can cause inventory drift.
Decoder rollups fold in archive visit order, so an ordering change can alter them
even when each decoded file is unchanged. Investigate inputs, order and decoder
behavior before replacing any reference. The [September migration record](../tests/fixtures/retail_goldens/README.md)
accounts for every changed field when these dormant checks returned to use.

## One explicit candidate writer

Ordinary tests never update the reference. `RETAIL_GOLDENS_WRITE=1` is rejected;
the former four independent manifest writers could overwrite one another.

Build the existing `asset` tool through the shared owner:

```sh
python -m tools.cargo_run --label corpus-tool -- build --release -p vera20k --bin asset
```

Resolve the emitted release binary through the same build owner:

```sh
asset_bin=$(python -m tools.cargo_run --resolve asset --profile release)
"$asset_bin" corpus-baseline --all-mixes --out target/retail-corpus-candidate.json
```

The resolver requires an unchanged recorded release executable and never chooses
another profile or guesses a conventional target path. The label's `manifest.json`
retains the build's source identity; resolution alone does not assert source freshness.

The exporter calculates the whole candidate before writing a new destination.
It refuses an existing destination and never replaces the committed reference.
Compare the candidate with the reference and investigate every changed field.
Accept a baseline change only with the corresponding input/decoder explanation
and validation; record that reason in the commit. Preserve the candidate and
build identity when using it as evidence.

## What these results establish

- Archive inventory and direct layout/byte comparisons are retail-data evidence.
- Structural/value tests have bounded assertions and historical native research
  references. Executing them does not refresh all historical parity claims.
- Decoder digests compare current Rust with prior Rust. They are regression
  baselines, **not native oracle goldens**.

Native oracle outputs have a separate checked-execution and provenance lifecycle:
see [native comparisons](native_oracle.md). Renderer behavior, normal game archive
reachability and complete `gamemd.exe` equivalence are outside this corpus suite.
The ordinary machine-readable parser scan is `asset parse-check`; the obsolete
hardcoded-path `audit-assets` binary has been retired in favor of that owner.
