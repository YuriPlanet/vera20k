# Reviewed legacy libtest exports

`cargo_run --retire-export-plan PLAN [--dry-run]` extends the existing saved-build
retirement owner; it does not discover candidates or establish their historical
ownership. Use Python 3.11 or newer, macOS `nm` and working `lsof`. Paths must be
absolute canonical paths without symlink ancestors (including system aliases).

Review each exact file for ownership and consumers before constructing a plan.
Keep active/final/control/debug binaries still required. A replacement label is
not evidence that a historical control binary can be discarded. Preserve all
native inputs, results, source, assets and required debugging files.

Schema 1 accepts exactly these keys. SHA placeholders below mean actual lowercase
64-digit SHA-256 values, not computed Rust/native goldens or invented provenance:

```json
{
  "schema": 1,
  "checkout": "/absolute/canonical/task-checkout",
  "export_root": "/absolute/canonical/external-evidence",
  "files": [{
    "file": "old-validation-libtests",
    "sha256": "EXACT_ORIGINAL_BINARY_SHA256",
    "reason": "Concrete reason this reviewed binary is superseded",
    "consumer_status": "reviewed-no-active-or-required-consumers",
    "provenance": [{
      "path": "/absolute/canonical/existing-original-build-record.json",
      "sha256": "EXACT_EXISTING_RECORD_SHA256"
    }],
    "results": [{
      "path": "/absolute/canonical/retained-validation-results.json",
      "sha256": "EXACT_EXISTING_RESULTS_SHA256"
    }],
    "replacement": {
      "label": "retained-final-tests",
      "file": "0/vera20k-recorded-cargo-name",
      "sha256": "EXACT_RETAINED_BINARY_SHA256",
      "manifest_sha256": "EXACT_RETAINED_MANIFEST_SHA256",
      "purpose": "vera20k-libtest"
    }
  }]
}
```

The plan binds the exact calling checkout and an external root separate from
source/shared builds. Each file is a unique normalized relative path, without
traversal, wildcard syntax or `.app` components. The selection must contain only
owned exported VERA Rust libtest executables. Provenance/results lists must be
nonempty pinned regular files. At least one pre-existing UTF-8 provenance record
must contain both the exact original binary SHA and recorded checkout path;
otherwise ownership is unresolved. Pinned records are reviewed evidence, not
cryptographic authentication of the original compiler or reviewer.

The replacement uses the canonical preserved-manifest/artifact validator. Its
manifest must record this checkout, source SHA, `cargo test -p vera20k --lib
--no-run` (or `-p vera20k-core`), and the selected Cargo host `debug/deps` or
`release/deps` artifact.
Replacement manifest/artifact hashes and purpose must match the plan. Both
original and replacement must expose defined Rust `test_main_static` and VERA
test symbols; native `nm` inspection never executes them. The existing Mach-O
owner validates every slice as `MH_EXECUTE` and enumerates OSO/AST debug inputs.
Every referenced debug input must exist as an unchanged regular file; missing
inputs are counted and block deletion, even when they predate the plan.

All entries preflight under the shared build lock before any deletion. Open-file
inspection, unwrapped Cargo/rustc detection and exact input identities are checked
again before each unlink, including retained replacements and evidence records.
Selection overlapping those inputs is rejected. The lock excludes cooperating
writers; stop other consumers first. `lsof` sees only processes visible to the
invoking user. Arbitrary external writers can still race filesystem operations.

Dry runs write no receipt and unlink nothing. Apply durably records original plan,
SHA/identity bindings, provenance/results references and replacement manifests
before unlinking only selected binary files. It fsyncs their containing directory
and durably records progress after each file. Failures stop deletion and report
`blocked` or `partial`. Receipts include projected/reclaimed allocated bytes and
observed export-volume free-space change; APFS clones/snapshots and other volume
activity can make these differ. Executable bytes are not backed up; all other
files and directories remain. The process is not a filesystem transaction.

Validation: `python -m unittest tools.tests.test_cargo_exports
 tools.tests.test_cargo_labels tools.tests.test_cargo_run
 tools.tests.test_cargo_macho`. Existing label retirement and cache retention
remain separate explicit operations through the same shared owner.

A real macOS dry-run/apply ran on one disposable copy of the then-retained
libtest. Native symbol/dependency inspection, `lsof` and the shared lock ran without
mocks. Apply removed only that copy (806,465,536 allocated bytes); its retained
binary, manifest, results, plan and all 402 debug-input identities remained
unchanged. Historical evidence executables were not selected. The focused suite ran 70 checks: 69 passed and one existing optional compiler
check was skipped. Shared replacement
inspection is reused within a locked batch; first-seen identities are rechecked
before every unlink, including mutations during later export inspection.

After an initial Windows CI failure, the
POSIX transaction fixtures now follow the existing saved-label retirement test
admission, while unsupported Windows native inspection still has a fail-closed
check. Simulated Darwin symbol tests explicitly provide native-tool discovery;
no host `nm` installation is assumed and no deletion check is weakened.
