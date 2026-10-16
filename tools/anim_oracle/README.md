# Anim boundary oracle

`boundary.py` executes original gamemd.exe instructions from `0042468C` through
one of four declared boundaries. It records ping-pong direction changes, ordinary
loop decrement, forward/reverse stage reset and continuation/terminal decisions.
The shared `tools.native_oracle` owner verifies the retail executable, loads its
PE image, checks execution completion and publishes references. No native branch
result, instruction or callee result is substituted.

Use the repository's `tools/requirements-test.txt` environment (Unicorn 2.1.4),
set `RA2_DIR` or `VERA20K_GAMEMD_EXE`, then run from the repository root:

```sh
python -m tools.anim_oracle.boundary --check
python -O -m tools.anim_oracle.boundary --check
```

Omitting the flag also checks without writing. Imports and `--help` require no
retail files and execute no native instructions. Reference changes require
explicit `--write`; `--output <candidate.json>` stages a candidate for review.
Every case must execute its entry and reach a declared stop before the original
100-instruction limit and shared ten-second timeout. Faults, exhausted limits and
early stops cannot become rows or publish a golden, including with Python `-O`.

| Stop, before its instruction | Recorded cases | Meaning |
| --- | ---: | --- |
| `004246DC` | 3,520 | Bounce, after original negation/store |
| `004247B1` | 2,976 | Loop, after decrement and stage reset |
| `004247F3` | 3,176 | Terminal boundary, before Next/terminal consumers |
| `00424B42` | 3,640 | Continue, before epilogue |

The 13,312 rows supply runtime/type fields and the already-committed stage. They
cover stock-shaped normal/damaged bounds and explicit signed/wrapping fixtures;
the latter are not asserted to arise in retail. Entry registers and per-case
writes retain the original fixture. One machine is reused in the original case
order. No constructor, timer, complete AI host, ART loader or whole-animation
parity is claimed; subsequent RNG, detach and lifecycle effects are outside these
boundaries. The preserved disassembly/region digest covers `0042468C..004247B7`,
while the verified whole-image digest also binds the later branch paths.

`boundary.meta.json` owns current provenance. The old payload's `script_sha256`
already differed from the checked-in producer before this cleanup. That stale
field was removed from the payload and both historical identities were retained
in the sidecar. Checked native execution reproduces **every other payload field**,
including all rows, bounds, decisions, stores, disassembly and region hash. This
is not a claim that the entire JSON file is byte-identical. The
[validation record](boundary.validation.json) records the migration hashes and
replay/test results.

The normal Rust test
`sim::anim_class::tests::native_anim_boundary_and_reset_vectors` compares the
production boundary transaction against every row. The connected
`combat_anim_shadow_endpoint_retires_in_runtime_and_survives_restore` test uses an
explicit ART override through the existing combat-animation producer and live
`SimRuntime` scheduler, checking retirement, neighbor visitation and save/load
continuation. It does not establish the stock NAMISL Building producer or the
earlier Anim Middle callback.

```sh
python -m tools.cargo_run -- test -p vera20k-core --lib native_anim_boundary_and_reset_vectors
python -m tools.cargo_run -- test -p vera20k-core --lib combat_anim_shadow_endpoint_retires_in_runtime_and_survives_restore
python -m unittest tools.tests.test_anim_boundary -v
```

The portable tests execute synthetic HLT, infinite-loop and invalid-instruction
fixtures through the actual shared runner, and check that failed writes preserve
existing references. Shared `tools.tests.test_oracle_lifecycle` guards import/help behavior for
animation and projectile producers. The failure gates also run with optimized
Python. They do not reproduce Anim behavior in a second model.
