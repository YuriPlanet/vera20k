# Independent review

These are the critic rules; AGENTS.md only points here. A critic prompt gives the
packet and what is specific to the change. It need not restate the rules below.

## When

Run one fresh read-only critic for each change to gameplay behavior, after
implementation and focused tests and before opening its PR. Each validated,
dependency-coherent mechanism gets its own PR and at most one critic pass; do not
hold validated mechanisms back to batch them into one review. Changes outside
gameplay (rendering, UI, tooling, tests, docs) and trivial edits get no critic.
Do not run critics per implementation increment, or follow-up or replacement critics
after fixes or revisions, unless the user explicitly asks.

## Packet

- The requirement: issue, brief or user request.
- The commit to review and its diff command (`git -C <checkout> diff <base>...<sha>`).
- The owner's claims, with their evidence.
- The actual validation output, including what was not run.

Naming claims worth challenging helps; the packet does not bound inquiry.

## Critic rules

- Read-only: no edits, commits, stashes or branch switches in any checkout, and no
  Cargo or other builds. A run you would need is a coverage gap for the owner.
- Ghidra (`mcp__ghidra__*`, program `gamemd.exe`): read calls only. `disassemble_bytes`
  is a write: it creates instructions that the next save keeps; use
  `disassemble_function` instead.
- Retail assets: single named files through the repo's `asset` tools, one archive at a
  time, never movie, audio or theme MIXes, under ~1 GB RSS; no whole-archive or corpus
  scans.

## Review

The critic did not build the change and has access to original sources and Ghidra.
Challenge omitted scope, design, production reachability and tests, including any copy,
wrapper or competing owner the change keeps or adds. Trace suspected defects to
consumers; distinguish evidence from speculation and architectural preference. Flag
claims that AGENTS.md requires to be executed (numeric kernels, RNG draws, arithmetic
traps, reported mismatches) but that lack executable evidence. Name useful refactoring
opportunities with their impact and risks.

Report actionable findings first, most severe first, each with trigger, consequence and
source, marked CONFIRMED or PLAUSIBLE; then what was checked and found sound; then
coverage gaps. Keep it under about 600 words. A scoped pass certifies no wider system.
Review grants no mutation/publication authority.

## Owner

Start the critic without blocking on it. While it reviews, leave the reviewed
checkout's files and branch as they are, and use the time: write the PR description
and checkpoint, run remaining focused tests, or research the next chain. Wait for the
report only when nothing else is left.

Fix confirmed mechanism/prerequisite defects and reject false positives with evidence;
worthwhile in-scope refactors are optional. Validate every change and recheck affected
conclusions. Adjacent findings become follow-ups, not automatic expansion; unrelated
opportunities become `refactor` issues.
