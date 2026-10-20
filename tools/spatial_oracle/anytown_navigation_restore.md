# Anytown hierarchy reconstruction after load

Original active-retail `Load_Game_Content_From_Stream 67E730` calls `581F50`
at `67E8CD` after `MouseLoad 5BDF70` and object restoration. This clears and
rebuilds hierarchy levels 2, 1 and 0, then calls `42C1C0` to refresh pathfinder
scratch. Vector vtable `7ED4A0+0C` resolves to `588D60`, which clears its count
at `588D6A`, destroys/frees the records and clears capacity. `58200F` zeroes
the selected per-cell hierarchy IDs before rebuilding.

MouseLoad preserves the saved Map+68 class/height/base-ID plane
(`5BE355..5BE369`) and all thirteen base movement-row arrays
(`5BE379..5BE3A7`). Thus exact pre-save hierarchy IDs and accumulated/dead
records are not a restoration invariant. The required hierarchy result is
original full reconstruction from the restored source facts. These ordered IDs
and records still affect route selection; mere graph isomorphism is insufficient.

[Instruction/byte evidence](anytown_navigation_restore.native_bytes.json)
contains saved Ghidra excerpts and hashes independently checked against the
retail binary.

The [runner](anytown_navigation_restore.py) reuses the existing physical full-map
[`Navigation`](anytown_damage/navigation.py) owner and its unchanged `run` driver.
For each selected boundary it constructs an independent machine and requires its
initial/selected state and trace to match the frozen Navigation packet exactly.
It then executes full original `581F50`; no Rust graph or plane is supplied.

| State | Before rebuild: levels 0/1/2 | After original 581F50 |
| --- | --- | --- |
| First damage | 3355 / 1336 / 704 | 3355 / 1336 / 704 |
| Collapse | 3363 / 1356 / 732 | 3356 / 1338 / 706 |
| Repair | 3370 / 1372 / 756 | 3355 / 1336 / 704 |

The [compressed result](anytown_navigation_restore.json.gz) preserves every native
value. Its canonical payload SHA256 is
`062a367873176005b65afb2b0bddb1feb9a8c4bb48f6684c63092e9d07fea9db`; the
[promotion receipt](anytown_navigation_restore.promotion.json) identifies the
original uncompressed packet. All 27,225 exported cell IDs and 331 padding IDs per
level, ordered records/edges, base planes/rows, physical cells, full RNG states,
admission samples, frame and wrapper call order are retained. Source planes,
cells, admissions, all RNG streams and frame remain unchanged by reconstruction.
[Metadata](anytown_navigation_restore.meta.json) pins imported source identities.

Replay from the repository root after supplying the local retail assets described
in [Anytown inputs](anytown_damage/README.md):

```sh
export VERA20K_ANYTOWN_INPUTS=/path/to/flat/anytown/assets
export VERA20K_SHRAPNEL_INPUTS=/path/to/shrapnel/bootstrap/assets
export RA2_DIR=/path/to/retail/directory
export PYTHONDONTWRITEBYTECODE=1
python -B -m tools.spatial_oracle.anytown_navigation_restore --write
python -B -m tools.spatial_oracle.anytown_navigation_restore --check
python -B -m tools.spatial_oracle.anytown_navigation_restore --compare-prefix /path/to/concrete-v10 --comparison-output /path/to/restored-comparison.json
```

The comparison requires `.restored_damaged.json`, `.restored_collapsed.json` and
`.restored_repaired.json` exports and checks every base plane/row and complete
ordered graph. A supplied full export's `navigation` member or a standalone
navigation object is accepted. Publication uses the existing shared compressed
packet owner, with a frozen payload guard even under `--write`.

Coverage is the selected reconstruction and comparisons made by the command
above. Full SaveGame/MouseLoad execution, pointer swizzling, actor lifecycle/path
invalidation and rendering are excluded. Existing Navigation physical construction, bounded allocation/free,
presentation, shroud and waterfall-animation seams remain. Stock Anytown has no
structural bridge/Tube records. Broader equivalence of Rust recomputation to
native-loaded base IDs/rows remains outside this witness. The comparison checks
the selected exported source facts exactly; it must pass before claiming parity.
