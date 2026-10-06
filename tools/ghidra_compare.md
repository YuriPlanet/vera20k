# Ghidra comparison tools

`python -m tools.ghidra_compare` compares a saved analysis with an owned rehearsal
copy before accepting metadata changes. It makes only explicit-program GET
requests; it does not create projects, change annotations, save programs, launch
servers or approve a change. The comparison algorithms previously lived in the
machine-local type-layout research scripts.

Use Python 3.12+ from the repository root. Decompile comparisons use the standard
library. Native frame checks also need the existing
[`tools/requirements-test.txt`](requirements-test.txt) dependencies (Unicorn and
Capstone) and the original `gamemd.exe` configured as described in
[native_oracle.md](native_oracle.md). No additional dependencies are introduced.
Help, imports and the synthetic tests need no Ghidra server or retail executable.

The HTTP adapter follows GhidraMCP **5.14.2**: `/decompile_function`,
`/get_xrefs_to`, `/get_function_by_address` and `/get_function_pcode`. An
incompatible or malformed response is an incomplete read, not an empty result.
Confirm each server's program path, binary SHA-256, x86 language and image base
before using it. For GhidraMCP 5.14.2 headless, pass the `name` returned by
`/list_open_programs` (for example `gamemd.exe`) to the read API, not its project
`path` (`/gamemd.exe`). Check that the name uniquely identifies the intended
program and that `/get_metadata?program=gamemd.exe` resolves it before comparing.
The server's project-loading argument separately uses the project path.
`--before-program` and `--after-program` are required; the tool never uses a
mutable current-program selection. The client does not attest the
server's loaded bytes. Native frame bytes are independently checked through the
shared oracle owner against the two supported identities documented in
[`native_oracle.md`](native_oracle.md#steam-compatibility-and-evidence-identity).
Frame reports record the actual selected file hash, not the legacy vector identifier.

Coordinate with the project owner and compare stable saved/rehearsal projects.
Do not open the same project concurrently in GUI and headless Ghidra. Follow
[the shared Ghidra workflow](../docs/research/ghidra-workflow.md) for the separate
annotation, save and readback process.

## Decompilation and callers

Provide a JSON plan with one row per function. Every action except `none` and
`skip` is compared; action names are descriptive and never executed:

```json
[
  {"addr": "0x00401000", "action": "signature"},
  {"addr": "0x00401080", "action": "skip"}
]
```

Replace the example addresses and program selectors with your actual plan:

```sh
python -m tools.ghidra_compare decompile \
  --before-url http://127.0.0.1:8089 --before-program gamemd.exe \
  --after-url http://127.0.0.1:8090 --after-program gamemd.exe \
  --plan plan.json --computed --out decompile-comparison.json
```

This compares all planned functions and known direct/tail callers, follows
callers through entry-point jump thunks, and with `--computed` includes Ghidra's
resolved computed/virtual-call references. Xrefs are paginated in batches of
500 with one overlapping anchor row: blank responses, repeated pages or a changed
anchor cannot silently become missing callers. `--max-callers N` selects a deterministic sample spread over the address
range; omission compares all discovered callers. `--max-callers 0` still reads
caller discovery but compares only the plan. Reports distinguish discovered
callers from sampled callers. Unknown indirect targets remain outside coverage;
Ghidra flow overrides and computed references are leads, not native proof.

The report retains every attempted entry, readable counts for each side, read
errors, improved and worsened functions, artifact totals and hashes of input
files. It checks new artifact variables, warning text/counts and lost total call
sites, as well as `unaff_`, `in_stack_`, `extraout_`, input-register artifacts and
bad instructions. Named calls may legitimately become pointer calls: total
calls are compared before claiming loss. These are textual heuristics, not a
proof that two decompilations mean the same thing.

Use repeatable `--expect-warning 'WARNING: exact text'` only for a justified
expected warning. Its occurrences remain counted in `expected_counts`; unrelated
new warnings still fail. This never exempts missing or partial reads.

## Stack frames against the native instructions

Frame reads request `granularity=basic` from GhidraMCP 5.14.2. On the installed
Ghidra 12.1.2 endpoint this exports each operation of the fresh decoded
`HighFunction` once, retaining its SSA inputs, outputs and widths. The full mode
also repeats those operations in a global array and can exhaust a 2 GB server
heap during JSON serialization (`0x675210`: 248,735 operations). Missing blocks,
export errors and malformed operations fail the read; no frame is inferred from
a partial graph. This equivalence concerns the endpoint's unchanged decoded AST,
not a generally edited p-code bank, which can contain dead operations outside
blocks.

Frame checks require an explicit **before** census JSONL file. Export one from the
exact saved project with the repository's
[`SignatureCensus.java`](ghidra_compare/SignatureCensus.java). The exporter reads
internal function metadata only; it does not disassemble, reanalyze or change
annotations. Use a stopped **owned snapshot**, never the project open in another
session. From the repository root, with JDK 21 configured in `JAVA_HOME`:

```sh
export GHIDRA_INSTALL_DIR=/absolute/path/to/ghidra_12.1.2_PUBLIC
"$GHIDRA_INSTALL_DIR/support/analyzeHeadless" \
  /absolute/path/to/owned-snapshot-directory SnapshotProject \
  -process gamemd.exe -noanalysis -readOnly \
  -scriptPath "$PWD/tools/ghidra_compare" \
  -postScript SignatureCensus.java /absolute/path/to/new-before.jsonl
```

`SnapshotProject` is the saved `.gpr` name without its extension; `gamemd.exe`
selects the existing program in that project's root. For a nested program,
append its folder to the project argument as `SnapshotProject/folder` and keep
the exact program filename after `-process`. Do not use an import command.
Export a separate new file from the owned after snapshot when comparing purges.

The exporter requires the x86 language and `0x00400000` image base and preserves
stored values, including unknown purges. It writes a temporary sibling and
publishes the completed file with an exclusive hard link; failed/cancelled
exports leave no final census, and an existing or concurrently created output
is never overwritten. Use an output filesystem supporting hard links; failure
does not fall back to exposing a partial file. The successful log message is
`Census complete`; the frame command then validates the census schema and hashes
the exact input. A retained compatible census can also be supplied explicitly.
Each line is a function object with these fields:

```json
{"entry":"00401000","name":"example","proto":"void example()","external":false,"thunk":false,"noreturn":false,"purge":0,"body":["00401000","00401003",1],"ranges":[["00401000","00401003"]]}
```

The addresses above illustrate the schema, not a trustworthy retail boundary or
signature. `body` is inclusive min/max; `ranges` retains every actual body range.
`purge` is Ghidra's stored integer value, including its unknown sentinel when
unknown. Do not replace unknown values with zero. Native call purges are derived
from original RET instructions and tail targets, not copied from that stored
value. Include the functions being compared and their known direct callees;
missing callee information can leave the stack equations unsolved. External
entries are not frame targets. No machine-local census is loaded implicitly.

```sh
python -m tools.ghidra_compare frames \
  --before-url http://127.0.0.1:8089 --before-program gamemd.exe \
  --after-url http://127.0.0.1:8090 --after-program gamemd.exe \
  --census before.jsonl --address 0x00401000 --out frame-comparison.json
```

Repeat `--address` for several entries. Alternatively use
`--comparison decompile-comparison.json --sample 50` to inspect every worse/better
entry plus up to 50 other recorded functions spread over the address range.
The report must name the same servers/programs. `--after-census after.jsonl`
additionally finds changed stored purges and checks all known direct callers,
including calls through thunks. That mode requires identical census entry sets;
it is for signature changes, not simultaneous function recovery. Additional
instruction-established no-return targets can be supplied with repeated
`--noreturn 0xADDRESS`; the report records them. They are assumptions to review.

The native walker uses file-backed PE bytes through `tools.native_oracle`, with
no zero-filled BSS masquerading as code. It computes ESP/EBP-relative accesses,
models the complete checked MSVC stack-probe body, and solves unknown call purges
using path-merge and return constraints. Distinct EAX constants at a merge remain
separate until overwritten or consumed by a stack probe; conflicting allocation
paths cannot pass as one known frame. Unsupported ESP writes, ambiguous EBP
merges, undecodable/range-crossing instructions, unresolved equations and
conflicts are reported as limitations. It is a bounded static model, not native
execution and not a complete x86 analyzer.

High p-code is compared instruction by instruction. Constant stack/member
expressions stay within one instruction; call-prototype inputs do not certify a
frame. For a native MOV into an ESP/EBP slot, only a stack output or a checked
STORE destination counts: a carried stack source cannot approve that write.
The report distinguishes right-to-wrong changes from errors already visible
before, and lists native accesses absent from each p-code map. Optimized-away,
indexed and otherwise unmapped accesses are outside its agreement coverage.
A changed mapping or overlapping byte range still needs inspection.

## Results and validation

Both commands require a new `--out` path and refuse to overwrite a receipt or
input. Exit **0** means no flagged regression within the reported coverage;
**1** means review findings; **2** means an invalid request, incomplete read or
unresolved native-frame analysis. Read failures produce a report where possible,
including when both decompilations are unreadable. A failure before comparison
(for example a missing/invalid census or wrong native image) prints a diagnostic
and writes no report. HTTP reads have `--timeout` (60 seconds per attempt) and
`--attempts` (1–3, default 3) bounds. A clean result is never automatic permission
to apply metadata or a parity claim.

```sh
python -m unittest tools.tests.test_ghidra_compare -v
python -m tools.run_tests
```

The optional exporter test compiles against an installed Ghidra and exercises
publication, cancellation/failure cleanup, collision preservation and JSON
escaping without opening a program. With `GHIDRA_INSTALL_DIR` and `JAVA_HOME`
set, run `python -m unittest tools.tests.test_ghidra_census_exporter -v`.
Otherwise that test is skipped; it is not a live export validation.

Tests use synthetic x86 bytes, p-code and HTTP responses, with network connection
attempts blocked. They cover incomplete reads on either/both sides, pagination,
thunks and virtual callers, warning identities, member-address expressions,
write-role counterexamples, stack equations and CLI receipts. They do not
certify a live Ghidra installation or regenerate native goldens.

A separate end-to-end check on 2026-10-03 used disposable copies of the
2026-10-02 11:39:36 and 12:18:23 saved projects with Ghidra 12.1.2, JDK 21 and
GhidraMCP 5.14.2. Both exports contained 19,889 internal functions; every exported
field in the after census matched the retained census. Four changed functions
and 12 sampled callers decompiled without read errors. A separate check of
`0x00445880` reproduced the retained `extraout_EDX` regression. Five frame reads
completed with four pre-existing write-mapping findings and no new regressions;
their 1,964 function-body bytes matched the pinned native image on each side.
An invalid program selector returned exit 2 with explicit read errors. This
checks the real export/HTTP workflow within that sample, not the entire database.
Source backup checksums were unchanged; temporary servers and copies were removed.

## Migration provenance

Read-only snapshots were checked on 2026-10-03 from the maintainer's
`ghidra-type-layouts-20261001/scripts` research directory. The active research
files, projects and receipts were left intact; existing owners can migrate
callers between passes. The old positional CLI/report formats are not an API
compatibility promise; use this documented repository CLI for new work.

| Inspected source | Snapshot SHA-256 |
| --- | --- |
| `decomp_compare.py` | `9c6fb8d1218702987e66b0788652c03e9246086857154d6cc814d239ccc52dc1` |
| `frame_compare.py` | `47bb6696e87dbbd7d31edd07cf7418a8159472f1480bce57f321bea2f6f37014` |
| `fnwalk.py` | `f5499881ade00a1baa7dfa5d9b9ba4873f72dbfc40a9ffbab4d011ff4c706a84` |
| `pe.py` (replaced by the shared native image owner) | `8fde41c5e2ec07cbbb4b3c9c3353d18a5fd4cec6e65e12b8c7882de7cab59381` |
| `test_decomp_compare.py` | `bd4c4569d91ac8a1e59b02dc43b65563f602735ac4aef5b23afde0071227e863` |
| `test_frame_compare.py` | `474498d5210a8493805dab4c2679d6a29bc19c454aab7b19844468de56579acc` |
| `SignatureCensus.java` from `ghidra-signatures-20261001/scripts` (metadata-only subset retained) | `b08e534438bd6bc73bb00cdeec54375645bcf473c419f4c60121c606c0571487` |
