# Exact Shell UI Certification Matrix

This tool generates the evidence-routed certification matrix for the active
offline stock-Skirmish shell journey. It is a coverage and provenance artifact,
not a hand-edited parity ledger.

Generate the fail-closed baseline:

```powershell
python -m tools.exact_shell_ui_matrix
python -m tools.exact_shell_ui_matrix --check
python -m tools.exact_shell_ui_matrix.validate target/exact-shell-ui/matrix.v1.json
```

Apply a status/evidence overlay:

```powershell
python -m tools.exact_shell_ui_matrix `
  --evidence target/exact-shell-ui/evidence.v1.json `
  --artifact-root C:/path/to/confined/artifact/root
```

Artifact paths in the evidence manifest are portable paths relative to
`--artifact-root`. Evidence-bearing generation and validation resolve every
artifact beneath that root and hash its actual bytes. Missing files, path
escapes, and digest mismatches fail closed.

Validation also recomputes the catalog and embedded-overlay digests and binds
all immutable rows, blockers, sources, exclusions, and resolutions to the
current generator catalog. A hand-edited or stale matrix cannot pass merely by
repairing its summary counts.

`VERIFIED` requires a native-executable differential or exhaustive proof,
resolution coverage where applicable, and resolution of every catalog blocker
attached to the row. Static research and Rust production regressions can be
recorded on `UNVERIFIED` or `DRIFT_FIXED_UNVERIFIED` rows without promoting
them.

The JSON contracts are:

- `matrix.v1.schema.json` — generated self-contained matrix;
- `evidence.v1.schema.json` — optional evidence/status overlay.

Generated artifacts live under `target/exact-shell-ui/`.
