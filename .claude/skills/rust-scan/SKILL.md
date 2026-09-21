---
name: rust-scan
description: "Review VERA20k Rust for determinism, state/lifecycle, correctness, safety, and performance risks. Defaults to src/sim/; reports findings without editing."
---

# Rust scan

Apply `AGENTS.md` and [review guidance](../_shared/review.md). Read relevant
[general](references/general-review.md) and [simulation](references/sim-risks.md)
lenses, including code outside `sim/` that feeds authoritative state.

- No target: `src/sim/`. A path or `scan <path>` selects a Rust file/module.
- `--changed [--base <revision>]`: changed Rust; default base: `main`.
- Repeat `--focus`: `determinism`, `architecture`, `state`, `safety`,
  `performance`, `ownership`, or `structure`.
- Defaults: determinism, architecture, state, safety, performance for simulation;
  safety and structure elsewhere. API/style review is optional.
- `--include-tests` includes test candidates; `--clippy` requests the wrapper.

For broad discovery, run from the repo root:

```text
python .agents/skills/rust-scan/scripts/collect_candidates.py --target <path> --profile auto --format summary
```

The collector accepts the scope flags above except `--clippy`;
`--format jsonl --rule <RULE_ID>` returns details. Matches are neither findings
nor complete coverage; direct reading suffices for small targets. Account for
uninspected candidates and truncated output.

Resolve production callers, types, ranges, authority, macro expansion, and trait
implementations. Follow the live `advance_tick`/`SPINE REGION` boundaries for
ordering and lifecycle risks. Use `architecture-scan` for broader placement questions.

Run this skill's `scripts/run_clippy.ps1` only when requested, from inside the
repo. It checks Cargo ownership and uses the library target. Preserve diagnostics
and exit status; unrelated build failure means incomplete validation.

Report source-backed consequences, trigger/frequency, fix direction, scope/HEAD,
and missing evidence. Retain low-priority findings, unresolved candidates (grouped
when large), and useful rejected matches. Unsupported performance claims remain
profiling candidates; do not create profiling infrastructure during this review.
