# Phase briefs

One brief per phase of
[`2026-07-30-clean-slate-system-implementation-order.md`](../2026-07-30-clean-slate-system-implementation-order.md).
A goal session reads the brief for its phase **before** the plan or the
research archive. The brief is the handover between sessions working the
same phase; the plan is only the dependency order.

Briefs are written when a phase is first targeted and updated by the session
that changes the phase's state (a merged PR, a corrected claim, a new residual).
Briefs link historical closure records even when those records move to the
archive. Phases never targeted have no brief yet.

Phase 7 predates these briefs. Its [historical closure record](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/gap-scans/2026-09-06-phase7-closure/README.md)
and [final audit, residuals and coverage limits](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/gap-scans/2026-09-06-phase7-closure/reverse-audit-phase7-final.md#part-c--consolidated-remaining-findings-all-thirteen-rows-ranked)
remain starting evidence for harvesting, economy and audio work. The audit
describes `d7733d37`, not current source; its executable-native-comparison gap
and residuals are not resolved by archival. The [Phase 8 brief](phase-08-production-power-radar.md#inherited-residuals)
already carries its inherited findings. For other Phase 7 work, recheck the
original residuals and coverage before selecting a mechanism or claiming parity.

## Template

Copy the sections in this order. Keep each section current, not cumulative:
replace stale rows, do not append a diary.

```markdown
# Phase N brief — <phase title>

Rows <first>–<last> of the plan. State: OPEN | IN PROGRESS | CLOSED <date>.

## Rows

| Row | GSI | Rust owner(s) | Native anchor(s) | Evidence and coverage | Notes |

One line per row. Owners are current file paths (check they exist). Anchors are
symbol + address with the research doc that established them. Evidence and
coverage cite the inspected code revision and actual native comparisons or
Rust checks, with their limits. Do not copy status from the retired System Map.

## Loops

The production loops the rows participate in and which stage each row owns,
established from current code and native evidence. Historical loop IDs may be
retained as references, but archived topology does not establish ordering.

## Prior work

Merged PRs that moved these rows, newest first, one line each. Rows closed by
evidence rather than code, with the evidence.

## Corrections

Claims in research, System Map or code comments that a session must not
trust, with the evidence that corrected them. Sessions add to this list; they
never silently fix the source doc without also recording the correction here.

## Inherited residuals

Residuals recorded by other phases that land on these rows (trigger, effect,
frequency, where labelled).

## Coverage

What the global parity harness fixture and the retail oracles exercise for
these rows, so a moved hash pin is read correctly.

## Open queue

Mechanisms found DRIFT/MISSING and not yet merged, with owner branch and review
state. Empty when the phase is closed or not yet scanned.

## Start here

The first concrete action for a fresh session.
```

## Goal modes

Two modes run against a brief. The goal prompt names the mode; the brief does
not prescribe one.

**Exhaustive phase close.** Every row in the phase is an ownership hypothesis.
Scan each row's mechanisms from the binary, build every DRIFT/MISSING mechanism
with a builder and an independent read-only critic, merge one coherent
mechanism (or its prerequisite foundation) per PR,
run a phase-wide reverse audit, and close only when no omission or regression
remains and `cargo test -p vera20k --lib` passes. Parity stays
undemonstrated until a gamemd-derived executable comparison or exhaustive
proof establishes it within stated coverage; closure is by evidence, not by claim.

**Bounded slice.** Select one ordinary-stock end-to-end loop from the brief's
loop list, trace it in runtime stage order, find the first player-visible or
determinism-relevant divergence, and close the smallest coherent prerequisite
capability (not merely the smallest patch). Deliver a separable foundation
first with its own evidence, validation and review. Rerun the parent loop;
close it only if its end-to-end check passes, otherwise record residuals in
the brief and stop after the handoff.

Both modes update the brief for touched, verified work. The System Map is
retired: no registry/topology/mechanism updates or map checker are required.
Existing briefs may retain explicitly historical map citations and status
columns; revalidate them before using them to select work or claim completion.
