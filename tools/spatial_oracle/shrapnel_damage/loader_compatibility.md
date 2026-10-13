# Pinned loader compatibility

This evidence supports a source-provenance refresh after integrating
`0be4174ef6649ed110bcb330c5bae25dbf7001e9`. It executes both actual Python PE
loaders against the original executable. **No game function or bridge corpus
was replayed for this refresh.**

[The result](loader_compatibility.json) compares every byte of the complete
10,485,760-byte Unicorn mapping, including headers, sections, BSS and gaps.
Both loaders produce identical bytes and memory permissions. Their complete
mapped image SHA-256 is
`c8dbc3d1c4671589ffa5d430f1be05ff5fe19bcfb51050374b04af6ccf9f657a`;
the complete `.text` SHA-256 remains
`4cd5557a7490debc493ff965afc4483d8d2f1065f434f6b665cbb8fc4835b0cc`.

The original source at `d77f60318578f31bee757b3a645de267c2e45ef7` has SHA-256
`dd5dbf5f3532b0eb20d761b8637c18ca8033317b67e99481f633c45dec6b2de3`.
The upstream source has SHA-256
`f3f58e1a15a4fd3be9e3bc2030819f079ab9155bdb08833e228c1c661445e51e`.
AST comparison proves that only the existing `_sections` function changes and
`file_span` is added. `load_image`, `run_checked`, `call`, comparison/provenance
helpers and all other module code are unchanged. The new bounds checks accept
the pinned valid executable; malformed-file rejection intentionally differs.
The new `file_span` also resolves each complete file-backed section to its
original bytes. It does not change mapping.

[The compatibility receipt](loader_compatibility_receipt.json) preserves the
old execution source identity and the exact original hashes/JSON paths of all
17 refreshed source-pin files. Their original contents are recoverable from
the recorded pre-refresh commit. Ten native sidecars now pin the compatible
upstream loader; this does not relabel their old native runs as new executions.
Current enclosing manifests are refreshed separately. Every existing result,
Rust projection, promotion guard and input/evidence file stays byte-identical.
The earlier ResidentRepair compatibility records, including their then-current
sidecar hashes, remain unchanged historical records.

The receipt intentionally excludes `bridge_wood_validation.json` (in git at 1b1a8b43e), whose owner
records the final integrated Rust/production candidate separately. This loader
proof does not establish compatibility for other binary or harness changes.

The receipt's source and artifact pins describe the integration preserved at
`318e5ae3`. Later source changes, including Anytown's checked receipt publisher
from PR779, have their own current provenance. The loader receipt and comparison
JSON remain unchanged historical evidence; their hashes are not advanced to
describe those later changes.

## Reproduce

Run from the repository root with Python 3.10+, Unicorn and the pinned original
`gamemd.exe` selected by `VERA20K_GAMEMD_EXE` or `RA2_DIR`. Git must contain the
two commits above and the receipt's pre-refresh commit. A shallow checkout must
obtain those Git objects first. No extracted maps, INIs, TMPs or copied VM are
needed for the loader comparison.

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.loader_compatibility --check --verify-pins --pins-at-ref 318e5ae3
```

Omitting `--check` also checks. Explicit `--write` updates only the loader
comparison JSON, with fixed source/binary/mapped-image identities enforced.
`--verify-pins --pins-at-ref REF` resolves the reference once to an exact Git
commit and reads the receipt, source maps and artifacts from that snapshot.
The result prints the resolved commit. Original pre-refresh metadata still
comes from the receipt's recorded earlier commit. A missing reference is an
error; no working-tree fallback occurs. `--pins-at-ref` requires `--verify-pins`.

Omitting `--pins-at-ref` keeps strict verification of the current working tree,
which intentionally fails when later source or artifact hashes have changed.
The ordinary `--check` always executes the two declared Python PE loaders and
checks the saved mapping comparison, independently of the optional pin snapshot.
The receipt records the original separate write and independent check.

For the Anytown aggregate, with its already documented local retail inputs:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.anytown_damage.validate_packet --manifest-only --check
```

That mode checks source/input/artifact manifests and pure projections without
executing game functions. The Shrapnel repair manifest was refreshed through
its existing `retail_inputs()`/`manifest()` owners in a fresh process; its full
`validate_packet` CLI would also replay native functions and was not used.
