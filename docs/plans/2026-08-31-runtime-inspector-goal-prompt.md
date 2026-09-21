# TODO — Runtime Inspector v1 Goal Prompt

Status: ready to run; not launched.

```text
/goal Build VERA20k Runtime Inspector v1 for this workflow: during an ordinary local offline skirmish, the user selects a few problematic entities, explicitly arms bounded recording, reproduces the problem, and chooses Capture Bug. A fresh Codex task then uses one repository CLI to select and inspect an immutable bundle containing the matching rendered frame, committed state facts, direct relationships, and recent causal evidence—without the user transcribing internals.

V1 is the bounded capture contract plus offline CLI. It is not gamemd parity, save/replay compatibility, stable semantic hashing, a profiler, full-world exporter, graph UI, live server/IPC, remote control, or MCP integration; those require later goals.

Read and obey AGENTS.md. Reconcile Git, ownership, worktrees, processes, and current origin/main; preserve other tasks. Repeat reconciliation before every slice touching shared runtime, render, diagnostics, schema, or Cargo files, deferring overlaps owned elsewhere.

First run the architecture-aware brainstorm process. Trace `SimRuntime`/`SimView`, the committed-frame seam in `app::match_runtime::sim_tick`, selection reconciliation, diagnostics/replay history, unit-inspector/debug events, render submission/readback, tactical-capture publication, and CLI conventions. Treat placements as hypotheses.

Before code, freeze:

- A finite owner/ingress inventory and relation enum.
- Exact caps for roots, related IDs, frames, commands, events, cells, bundle bytes, retained bundles, total spool bytes, and writer queue.
- Schema, truncation rules, spool identity, lifecycle, and transaction boundaries.

A fresh read-only design critic must challenge workflow coverage, ownership, frame identity, scaling, and determinism. Builders cannot grade themselves; proceed only after PASS.

Arm snapshots the canonical selected roots and creates a capture epoch. Later selection changes do not alter the watch set. Record only those roots, typed one-hop relations reached by indexed lookup, and a bounded command tail for at most `MAX_FRAMES` committed frames. Capture finalizes on the next committed frame; reaching the cap triggers equivalent automatic finalization. Cancel discards without publishing. Re-arm only after cancellation or finalization. Busy, full, expired, and failure states must remain visible and non-blocking.

Keep activation, watch lifecycle, presentation facts, serialization, publication, and tools in app/tooling ownership. Sim may expose only narrow bounded immutable facts through `SimView` or equivalent accessors; app schemas, files, rendering, and transport must not enter sim.

At the committed-frame seam, create one immutable receipt containing capture epoch, tick, binary frame, returned state hash, and read-only RNG states. Carry it and the bounded owned projection through reconciliation into the exact render submission/readback. Pair composited pixels with that receipt, never timing or duplicated fields.

The versioned bundle includes build/schema identity; map/mode/seed/tick; receipt; player, camera/cursor and canonical selection; bounded root state; frozen typed one-hop target/navigation/radio/dock/transport/bunker/control relations; relevant bounded cells/blockers; issued/admitted/due command envelopes plus only the evidenced aggregate executed count; watched-frame transitions; final image; file hashes; and visited/looked-up/truncated/omitted counts. Unknown causal facts remain explicit. Do not invent per-command outcomes.

Publish bundles atomically in one shared, per-repository user-local diagnostics spool. Its identity must be independent of CWD/worktree and resolve identically for game and CLI. Worktrees of one repository share it; separate clones cannot collide. Manifests remain relocatable and contain no absolute paths.

Give every accepted bundle a stable capture ID, display it in-game, and support capped listing plus exact `--capture <id>` selection. Never silently select “latest” or delete accepted evidence. Refuse visibly at quota.

After owning the capped DTO and pixels, a capacity-bounded app worker performs encoding, hashing, fsync, and staged publication without sim access. Saturation refuses capture without blocking.

Add one Rust-owned read-only CLI with capped text/JSON batch queries: `list`, `summary`, `selected`, `entity`, `relations`, `trace`, `commands`, `rng`, and `diff`. It reads only validated manifest members and rejects partial, malformed, unknown-schema, oversized, unsafe-path, link/reparse, hash-mismatched, and frame/state-mismatched bundles.

Never expose `GameSnapshot` or raw entities as the ABI; traverse or serialize the full world; perform inverse/full-store relation scans; enable all-entity logs; intern names during capture; consume RNG; change ordering; drain outputs; or call `compute_retail_multiplayer_checksum`. Disabled mode adds no per-entity allocation, write, lock, background work, or I/O.

Focused `--lib` and tool tests must prove:

- Projection preserves state hash, every RNG state, `LogicVector` order, and snapshot bytes.
- Inspector-on/off scripted timelines are identical.
- Explicit counters prove capped indexed work and bounded memory/output with 20,000 entities and no full-store traversal.
- Matching frame/state receipts pass; swapped adjacent-frame receipts fail.
- Captures are discoverable across worktrees but isolated across clones.
- Multiple captures resolve only by exact ID.
- Quota/queue saturation and publication faults expose no accepted partial bundle.
- CLI queries recover a constructed stuck-unit blocker/path; negative controls report UNKNOWN.

Give every design/implementation critic the frozen requirement, base SHA, complete diff, sample bundle, and literal validation—not builder conclusions. Fix the largest confirmed gap and resubmit to a fresh critic until every gate passes, rechecking earlier fixes. Run `$rust-scan --changed --base <base-sha>` and `$architecture-scan --changed --base <base-sha>`; confirmed change-caused CRITICAL or WARNING findings block completion.

Treat v1 as one transaction only if the approved design proves its capture contract, production path, reader, and CLI inseparable. Any broad prerequisite or independently publishable boundary gets a fresh `feature/*` branch/PR from current `origin/main`, merged before dependent work; never stack dependent transactions. This goal authorizes publication and merge only for these v1 transactions.

The transaction owner resolves conflicts. If resolution changes the tested tree, rerun affected focused checks and fresh critic/scans, invalidate prior certification, and run one replacement `cargo test -p vera20k --lib` on the exact resolved tree.

Scope fuse: if exact pairing requires new GPU ownership or synchronous GPU waiting, bounded facts require always-on sim indices/per-entity overhead, or inspection requires snapshot migration/live IPC, stop with the exact smaller transaction split; do not broaden v1.

Done only when the production workflow yields a stable capture ID that a fresh Codex task can query to recover the fixture evidence; all cap, quota, failure, determinism, discovery, and frame-pairing tests pass; reverse audit of the frozen owner/ingress inventory plus changed diff finds no mutating, unbounded, or mid-tick path; every critic and scan passes; one final `cargo test -p vera20k --lib` passes on each exact merge-ready transaction tree; every merged commit is verified on `origin/main`; and no v2 work starts. Stop.
```
