# Build cache retention validation

The shared `cargo_run` owner holds `owned-builds/cargo.lock` across any retention
pass, Cargo, label publication and the final pass when one is due (see the
cadence in [the tool index](README.md#cargo-ownership-and-labeled-builds)). `_cargo_cache` is its private
implementation, not a separate cleanup command or lock owner. The immutable
manifest validator is shared with labelled executable resolution; protecting a
library-test label does not make it eligible for host executable resolution.

The policy tests in [test_cargo_cache.py](tests/test_cargo_cache.py) exercise
dry-run nonmutation, budgets, real-free-space fallback, protected dependencies,
all-or-nothing preflight, stale mutable latest records, links, whole sessions,
publication/input races and partial unlink failure. Default repository Python
tests require no compiler, retail data or debugging utilities:

```sh
python -m tools.run_tests
```

The full final371-test Python suite passed (3 optional tests skipped).
The focused final suite passed64 tests:42 retention tests and22 existing runner
tests. The opt-in native test passed separately. Saved log identities are in the
receipt. The single fresh critic found discovery-window, nested-layout and
free-space-error defects. Five focused tests first reproduced those failures;
the corrected owner passes them, and two further tests bound cross-target profile
admission. Directory identities are captured before enumeration, including
intermediate directories. Nested research/build-script outputs never qualify as
compiler cache profiles. Unavailable final free-space samples remain explicit in
the blocked/partial receipt.

Windows CI exposed two portability defects in the initial candidate: `DirEntry.stat`
returns zero device/inode/hardlink metadata, and a parser fixture assumed Unix
absolute paths. Inventory now uses real `lstat`; unidentified inode values are
counted individually rather than collapsed. The fixture uses host-absolute paths.
Two added regressions model Windows directory-entry metadata and unknown-inode
accounting. The initial CI failure is retained in the receipt. A second run passed the
retention mechanisms but found a library-test fixture without the Windows `.exe`
suffix; correcting that fixture preserves the resolver's existing host-only
admission. Both failure receipts remain recorded. See the
[Python metadata contract](https://docs.python.org/3.12/library/os.html#os.DirEntry.stat).

[The opt-in compiled test](tests/test_cargo_cache_native.py) creates a temporary
Git repository and owned cache, compiles a real debug object and binary, preserves
a schema-1 library-test label, and puts an orphan object beside the required one.
It acquires the actual project's build lock before invoking the compiler. Run:

```sh
VERA20K_CACHE_NATIVE_TEST=1 python -m unittest tools.tests.test_cargo_cache_native
```

On macOS arm64 / Apple clang17, the native test passed: dry-run left all files
unchanged, apply removed the orphan, source/binary/required-object hashes matched,
the preserved binary printed42, and its debugging dependencies were unchanged.
After trimming, `dsymutil` built a dSYM containing `required_answer` and `fixture.c`.
[The saved receipt](cargo_cache_validation.json) retains compiler identity,
hashes, selected/removed bytes and observed volume changes. Temporary fixture
paths identify that run and are deleted by the test; the harness reproduces it.
This validates the host example, not gamemd or whole-game debugging equivalence.

The earlier production receipt is retained as historical evidence: cleanup was
blocked by debug-map warnings about already missing objects. That behavior is now
covered by a failing-before-fix policy regression. The replacement Mach-O reader
extracts OSO/AST references directly; absent objects no longer disappear from the
inventory. Existing inputs are protected, missing inputs are recorded, and the
preflight rejects a missing input that reappears during inspection. Dangling links
are errors, never treated as missing objects.

[Mach-O tests](tests/test_cargo_macho.py) exercise both byte orders, 32/64-bit thin
and universal layouts, archives, AST inputs, every fixture truncation and malformed
bounds. The opt-in compiled fixture compares references with actual dsymutil output,
then deletes its inputs and verifies that all references remain discoverable:

```sh
VERA20K_CACHE_NATIVE_TEST=1 python -m unittest tools.tests.test_cargo_macho tools.tests.test_cargo_cache_native
```

The retention native fixture now also removes one debug object while preserving a
second one: real trimming removes an unrelated orphan, keeps the surviving debug
input and saved binary byte-identical, reports the missing input, and executes the
binary successfully. This demonstrates retention safety, not recovery of deleted
symbols. [Label lifecycle tests](tests/test_cargo_labels.py) cover exact retirement,
all-label preflight, durable manifest evidence before deletion, running executable
rejection and partial failure reporting.

The direct reader follows LLVM's
[MachODebugMapParser](https://github.com/llvm/llvm-project/blob/release/17.x/llvm/tools/dsymutil/MachODebugMapParser.cpp)
and [archive naming](https://github.com/llvm/llvm-project/blob/release/17.x/llvm/tools/dsymutil/BinaryHolder.cpp),
with wire layouts cited in [the owner](./_cargo_macho.py). Unsupported layouts,
malformed commands, relative paths and unknown formats fail closed. Reusing an
inspection never reuses its dependency existence decisions: current identities and
missing paths are rechecked before deletion. The cache tests cover restored inputs,
changed binaries, cached failures, inspector invalidation and unknown inode fallback.

ELF scanning uses [GNU readelf](https://sourceware.org/binutils/docs/binutils/readelf.html)
to inspect sections and all compilation units without following debug links or
contacting debuginfod. Embedded DWARF may permit trimming; split/external debug
information blocks it. [Separate debug files](https://www.sourceware.org/gdb/current/onlinedocs/gdb.html/Separate-Debug-Files.html)
need identity/CRC and external closure support that is not implemented here.
PE/PDB closure also blocks trimming: [MSVC FASTLINK](https://learn.microsoft.com/en-us/cpp/build/reference/debug-generate-debug-info?view=msvc-170)
may depend on original objects/libraries beyond the PDB. Those formats retain
normal Cargo behavior and make no unsupported deletion claim.

Allocated bytes removed and observed free-space delta are distinct. The native
receipt's small deletion did not imply a positive volume delta: receipt writes,
APFS allocation and concurrent unrelated activity also affect free space.
Dry-run projections are estimates; apply checks actual free space as it proceeds,
then records remaining protected-budget and free-space shortfalls.

The [retirement follow-up receipt](cargo_cache_retirement_validation.json) records
402 passing Python tests (four optional skips), native fixture comparisons and the
real production apply: 8,231 compiler files removed, 19.50 GiB allocated and
19.42 GiB observed volume reclamation. All 51 saved labels' manifests and executable
hashes were reverified afterward. Seven binaries retain explicitly reported missing
debug inputs; their surviving dependencies remain protected. A second preview reused
all 799 inspections with no misses, completed in 13.65 seconds and selected nothing.
Protected files leave the soft budgets unmet; no wider reclamation is claimed.
The old local hourly deletion hook was backed up and changed to invoke this shared
owner with a zero-second lock wait, retaining its existing safe main-sync behavior.

## Recurring pressure and build admission

The [pressure follow-up receipt](cargo_cache_pressure_validation/receipt.json)
records 435 Python checks (431 passed, four optional skips) and a real compiled
Mach-O fixture. Cleanup removes the fixture’s finalized incremental hardlink
aliases and metadata; required debug paths and binary/source hashes survive,
the binary still prints42, and dsymutil reconstructs its debug information.
Four new regression checks fail against the original owner/runner and pass with
the fix. Whole known finalized sessions follow
[rustc’s immutable cache lifecycle](https://doc.rust-lang.org/stable/nightly-rustc/src/rustc_incremental/persist/fs.rs.html).

Every shared inode must have all aliases accounted for in registered caches.
Unknown external links, working sessions, unknown contents and debug-reference
paths remain protected. Cold sessions are preferred, with newest-session fallback
under remaining pressure. Own unlinks update only the expected inode link-count
and ctime transition; unexpected mutation stops deletion. Allocation is reclaimed
only when the final inode alias is removed, while incremental-budget accounting
ends with the final incremental alias. Dry-run projections remain estimates.

A fresh free-space measurement, taken after automatic cleanup when the reserve was
short, admits or blocks Cargo.
The configured minimum applies to the target volume and, for labels, the artifact
volume. Missing measurements block a build. This prevents new builds starting
below the reserve; it cannot bound a running build’s peak allocation or unrelated
volume activity. Compiler cache budgets remain soft for protected files.

The single fresh critic found a Linux-only fixture assumption: embedded DWARF
need not retain the original object file. The assertion now follows actual debug
references, and the corrected compiled macOS fixture passed. The external-copy
audit inspected61 Mach-O evidence binaries: no selected path was a dependency;
the eight unique existing references were protected, ineligible `.rlib` archives.
No exported evidence executable was deleted.

The real locked apply removed42,026 compiler-cache paths, reclaiming61,837,312,000
allocated bytes (57.59 GiB) and61,827,600,384 observed volume bytes. Free space
rose from4,989,837,312 to66,817,437,696 bytes. All three targets were met, no errors
were reported, and116 retained manifests/executable records passed their hash
checks afterward. Full local receipts and preserved SHA-256 values are bound in
the saved follow-up receipt; no executable copies are included. Later volume
activity can change free space, so future runs still measure it afresh.
