# Ghidra annotation passes

[`ApplyGhidraPass.java`](ghidra_pass/ApplyGhidraPass.java) replays a recorded
annotation pass onto any copy of the gamemd.exe Ghidra database. Several copies
exist (the main annotation machine, downloaded snapshots), so a pass made on one
copy is kept as a JSON ledger in [`ghidra_pass/passes/`](ghidra_pass/passes/) and
replayed on the others instead of being redone by hand.

The script refuses a program whose executable SHA-256 is not the ledger's
(`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`) and rejects a
malformed ledger (unknown operation, missing field, two renames to one name) before
it reads anything else. It never overwrites state it did not plan against: each
operation records what it expects to find, and anything else is a conflict that is
reported and skipped.

The [Rust-cited names ledger](ghidra_pass/passes/2026-10-04-vera-cited-names.json)
was reviewed against the pinned original PE on 2026-10-05. It now admits 189 of
the original 191 names, including ten neutral replacements where a proposed
class or source name lacked evidence, and 217 bounded function/table plates.
Five template/operator spellings also use C identifiers: Ghidra otherwise prints
both vector `*=` and `+=` helpers as the same `Vector3D<float>__operator__` name.
`0x522D00` and `0x522D20` keep their default names: the inspected slave callers
do not establish the proposed InfantryClass receiver. Eleven additional notes
on unnamed functions are withheld because their broader receiver, source
equivalence or reachability claims were not established. The complete proposals,
decisions and witnesses are in the [review evidence](ghidra_pass/evidence/2026-10-05-cited-names-review.json).

This ledger uses `atomic: true`. Copies that already applied the original PR #1055
may conflict on a replaced name or a changed paragraph under the same tag. Compare
those exact conflicts individually; do not disable the guard to obtain a partial
replay. This pass does not create missing functions or establish signatures,
typed `this`, class layouts, executed arithmetic/RNG behavior or runtime reachability.

## Running a pass

Run `check` first. It reports every operation and writes nothing. `apply` writes
the pending operations in one transaction: it evaluates each operation again just
before writing it, reads each write back, and rolls the whole transaction back if
any readback fails or cancellation is detected before commit. Save the program
afterwards, then run `check` again and require `pending=0` **and** `conflict=0`.
With `"atomic": false`, conflicts are skipped, so `pending=0` alone can describe
an incomplete replay. Record any deliberately excluded conflict separately.

In the Ghidra GUI, add `tools/ghidra_pass` to the Script Manager's script
directories, run `ApplyGhidraPass.java`, then pick the ledger and the mode. The
whole pass is one undo step.

Headless, on a project that is not open in the GUI:

```sh
"$GHIDRA_INSTALL_DIR/support/analyzeHeadless" /path/to/projects ProjectName \
  -process <program> -noanalysis -readOnly -scriptPath tools/ghidra_pass \
  -postScript ApplyGhidraPass.java tools/ghidra_pass/passes/<pass>.json check
```

`<program>` is the program's file name in that project (`gamemd.exe`,
`gamemd-working-2026-10-04`, ...). `-readOnly` keeps the project unchanged, which
also rehearses an `apply`; to write, use `apply` without `-readOnly`. Headless
Ghidra uses the GUI's settings directory by default. On 2026-10-04, headless runs
beside an open Ghidra 12.1.2 GUI were followed by `felixcache` `NoSuchFileException`
errors on the GUI's next script run; give a headless run its own directory with
`JAVA_TOOL_OPTIONS=-Dapplication.settingsdir=/some/other/dir`.
`analyzeHeadless` can return exit status 0 after a post-script exception: inspect
the diagnostic log and the expected summary rather than treating status 0 as a
passing replay.

Through GhidraMCP's inline-script runner, pass both arguments (without them the
script waits for a GUI dialog). The runner splits arguments on whitespace, so the
ledger path must not contain spaces.

Each operation prints as `PENDING` (would be written), `DONE` (already in the
database) or `CONFLICT` (the database differs from what the pass expected). A
conflict usually means another copy changed that function first, or lacks an
earlier pass the ledger was planned against (`planned_against`); compare and
decide by hand. A ledger with `"atomic": true` applies nothing while any conflict
remains.

## Ledger format

```json
{
  "format": 1,
  "pass": "2026-10-04 VERA-cited names",
  "tag": "[2026-10-04 VERA-cited names]",
  "program_sha256": "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c",
  "planned_against": "the database snapshot the pass was made on",
  "atomic": false,
  "ops": [
    {"op": "rename_function", "address": "0x004010C0", "from": "FUN_004010c0", "to": "Name"},
    {"op": "append_plate", "address": "0x004010C0", "function": ["FUN_004010c0", "Name"],
     "text": "Evidence for the name."},
    {"op": "append_plate", "address": "0x004059D0", "text": "What this jump table holds."}
  ]
}
```

- `rename_function` renames the function that starts at `address` when its
  current name is `from`. A name that already belongs to another function is a
  conflict.
- `append_plate` appends `tag` and `text` as a new paragraph of a plate comment.
  With `function`, the plate is the function's and a function with one of the listed
  names must start at `address`. Without it, the plate goes on the code unit that
  starts at `address`, where no function may start. It is done once the plate holds
  that paragraph and conflicts with a different paragraph under the same tag.

The ledger is also the record of what the pass changed and why: the plate text
carries the evidence, following the tagged-paragraph convention in
[the Ghidra workflow](../docs/research/ghidra-workflow.md#names-and-their-sources).

## Runner regression checks

On a private copy, run `CheckGhidraPassCancellation.java --private-copy` with
`analyzeHeadless -process <program> -noanalysis -readOnly` and the same script
directory. It exercises Ghidra's normal script execution and transactions,
cancelling after the first rename both with and without a second operation.
Both runs must restore the original name and plate. It also checks that an atomic
pass with zero pending operations and one conflict is rejected. The expected
output has two `CANCELLATION ROLLBACK OK` lines and one
`ATOMIC CONFLICT REJECTION OK` line.
