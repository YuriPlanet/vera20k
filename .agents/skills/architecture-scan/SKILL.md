---
name: architecture-scan
description: "Review VERA20k ownership, dependencies, APIs, and code placement when explicitly invoked as architecture-scan. Reports findings without refactoring."
---

# Architecture scan

Apply `AGENTS.md` and [review guidance](../_shared/review.md). Inspect the requested
boundary and its consumers using the relevant [lenses](references/review-lenses.md).

- No target: begin at `Cargo.toml`, `src/lib.rs`, `src/main.rs`, and module contracts.
- A path selects that file/module in context.
- `--changed [--base <revision>]`: structural changes; default base: `main`.
- Repeat `--focus`: `boundaries`, `ownership`, `api`, `layout`, `naming`, `tests`,
  or `packages`. Defaults: boundaries, ownership, API, layout.

Trace state owners, mutation paths, public seams, and callers, resolving relevant
re-exports, macros, traits, and conditional compilation. Distinguish active
migrations from settled architecture. File size, nesting, visibility, and
implementation counts are clues, not defects. Use `cargo metadata --no-deps`
only when manifest topology needs it; follow the project contract's Cargo coordination.

Report each finding's owner, consumers, source, consequence, trigger/frequency,
and improvement direction with migration risks. State checkout/HEAD, inspected
scope, unresolved questions, and existing boundaries worth preserving.
