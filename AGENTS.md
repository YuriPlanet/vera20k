# VERA20k — Project contract

**Outside contributors and their agents:** follow the README's
[Contributing](README.md#contributing) section and your issue, and skip the rest of this file.
In short: one small fix per PR, one to three regression tests that fail without it, the
native function address cited next to the code, no native-execution corpora, and a PR
description of about 15 lines. The rest is the contract for the maintainer's own agents:
its build runner, retail-data checks, critic reviews, checkpoints and native comparisons
don't apply to you. Where the two differ, the README wins.

VERA20k is a cross-platform Rust reimplementation of the Command & Conquer:
Yuri's Revenge engine (`gamemd.exe`), using original retail rules and assets.
The goal is to reproduce native gameplay behavior through a Rust-native architecture,
while scaling to **20,000 units and 30 players**. VERA20k must build and run on Linux,
macOS and Windows; keep architecture and dependencies compatible.

Development follows complete gameplay mechanisms and their required dependencies,
with clear ownership of state and shared logic. Native executable behavior and
retail data establish what the implementation must reproduce.

This contract governs Codex and Claude. Use engineering judgment; skills are optional
specialized help. Specific user instructions override workflow defaults.

## Intent and autonomy

The user often thinks aloud. Follow intent, keep exact technical values literal,
and treat proposed causes as hypotheses. Investigate contradictory observations.
Push back briefly with evidence; respect informed decisions.

Complete authorized work without routine approval. Resolve discoverable questions;
ask only for missing authority or consequential, undiscoverable preferences.
Research/review alone does not authorize implementation. Preserve scope amendments,
explicit exclusions and stop instructions across continuations.

Be brief, plain and result-first.

## Exactness and evidence

Confirm gamemd-derived changes against original instructions, active callers, and
retail data. Ghidra annotations are often wrong; treat them as
leads, not proof. Confirm active-YR reachability; unreachable claims need a breakpoint or
flag-to-leaf trace. Never invent offsets, identities or behavior.

Priority follows player visibility and frequency; it does not establish equivalence.
Differences a player cannot see, such as 1–2 px offsets or small color deltas, get a
one-line residual, not investigation; when a visual detail matters, port the drawing
code instead of fitting captures.
Missing or unproven required behavior keeps an exhaustive task open.

Native evidence is the instructions you read: cite the native function name and
address next to the Rust. That is enough for control flow, ordering, gameplay rules,
timers and plain arithmetic; compare with retail play side by side when it matters.
Avoid unqualified “VERIFIED”/“complete”.

Tests are regression tests: per fix, one to three focused tests through production code
that fail without the fix. Use boundary cases, not sweeps or grids, and keep tests
proportional to the change: a 20-line fix gets tens of test lines, not hundreds.

Execute native code ([Unicorn](tools/native_oracle.md) or capture) only for a shared
numeric kernel (lookup tables, fixed-point conversion, distance and angle math), RNG
draws (count, order, stream), arithmetic with a known trap (x87 precision, rounding,
overflow, signed division or shifts) that retail data reaches, or behavior a playtest or
bug report shows differing. Goldens come from that execution, a capture or retail
bytes, never hand calculations or prior Rust. Commit at most about 20 cases holding only
the inputs and outputs a Rust test asserts: no call traces, addresses, registers or
stack values, and under 100 KB per file. Keep a harness only while a Rust test consumes
its output, with a sidecar recording the command.

Every gamemd.exe build is treated as the same game: code, tests and comments neither
check nor cite its hash. Sidecars pin no hashes of repository sources; those go stale
without any change in native output. Do not commit receipts, run logs, capture
screenshots, critic reviews or static disassembly packets. A PR description says what
changed, why (native address) and how it was tested, in about 15 lines; no
evidence-level sections, templates or run logs.

Each cohesive gamemd-derived Rust behavior carries nearby native function name/address
and source; sim-behavior commits cite their evidence.
Consult the [Ghidra reference](docs/ghidra-workflow.md) for access,
interpretation pitfalls and shared-database edits.

## Retail data

gamemd behavior is native code plus the INI keys it reads; port both. For each key,
port the native read: file and layer, section, exact-case key, default, parser, clamps,
read order and any post-read pass. Defaults come from the constructor or the reader's
default argument, never from retail values. Take type names and tuning values from the
keys gamemd reads; hard-code a name only where gamemd holds the literal. Parse with the
native readers in `src/rules/ini_value.rs`, not `str::parse`.

Read retail values in every applicable layer before choosing constants, priorities or
fixtures. Each scenario re-reads rules from `RULESMD.INI`, `LANGRULE.INI` when present,
the selected mode INI, then the map (campaigns and one mode add passes); the map also
extends `AIMD.INI`. `ARTMD.INI` and the sound, EVA and theme INIs are not layered, and
gamemd never reads the RA2 base INIs in `ini/`. A branch gated on a key the retail rules
leave unset is dormant, not unreachable. Check retail data with a gamemd-exact parse,
not grep. When INI values decide an outcome, test through the production reader on
retail INIs. Details: [retail INI reference](src/rules/mod.rs).

## Architecture and delivery

Use the simplest implementation that fully satisfies the required behavior. Avoid
unnecessary abstractions, duplicated logic and speculative features; prefer clarity
over minimizing line count. Choose boundaries and abstractions by responsibility and
consumers, not line/type counts or C++ structure.

Prefer data-oriented design for simulation hot loops. Match gamemd's results, not its
execution strategy: batch, vectorize or parallelize freely. Update order may differ
from gamemd's when it stays deterministic and causes no noticeable gameplay change;
within one object's update, only for a profiled hot spot. Record each order change in
its PR for side-by-side play.

Preserve state authority, lifecycle, tick phase order, timers, same-tick effects,
persistence and numeric semantics under the policy below. Document and validate
floating-point use where native behavior requires it. Storage order and active-object
order are distinct.

All math must preserve simulation determinism across supported platforms and CPU
architectures for identical state, inputs and RNG. Prefer `SimFixed` for simulation
math, accepting documented differences from native precision and rounding. Avoid
x87 emulation unless demonstrated gameplay requirements make it necessary; exact
native arithmetic alone is not a requirement to emulate x87. Validate affected
gameplay, range and overflow behavior. Presentation must not affect simulation
determinism.

`sim/` never depends on `render/`, `ui/`, `audio/` or `net/`.
The engine (`util`, `assets`, `rules`, `map`, `sim`, `net` and the headless and
skirmish roots) is the `vera20k-core` crate (`src/Cargo.toml`); `app`, `render`,
`ui`, `audio` and `asset_tools` stay in `vera20k`, which re-exports core's modules.
Core's test-only code also compiles under its `test-support` feature, so client
tests reach it; only test modules and the hooks into them stay `#[cfg(test)]`.
App code orchestrates without owning duplicate gameplay. Current module contracts
and `advance_tick` phases describe the architecture. Name coordinate frames/units.

Simulation state and shared decisions have one authoritative owner. Before adding
state or decision logic, find existing writers and name the owner in the PR. Keep
authoritative state private to its owning module and expose mutations through the
owner. Extend or fix that owner instead of introducing competing state or duplicated
decision logic, or widening visibility to reach its state or raw data.

Before porting a native function, search the code for its address (e.g. `703850`) and
native name. Each native function has one Rust port, and new callers call it. Never
fork an owner: when an existing one is wrong for your chain, correct it for every
caller (a change toward native is in scope, with evidence for each affected path) or
use it as it is and record the residual. A change that adds a copy, a re-inlined body,
a caller-specific wrapper or a competing owner is a defect to remove before merge; the
critic checks for them.

Derived caches and indexes are allowed when their source of truth, update or
invalidation rules, and consistency validation are explicit.

One owner completes one player-visible gameplay chain at a time through native
evidence, implementation, production integration and review. Bound each chain to one
common unit, weapon or screen path; other paths follow later. Prioritize by
visibility and frequency. One chain is one PR: don't split a chain into several PRs
or open one per gap, and don't stack unmerged implementation branches. Plans are
optional; implementation authority includes design choices.

Trace affected state, call chains and native dependencies across classes and
subsystems wherever the selected behavior requires them: trigger, prerequisites,
admission, effects, consumers and cleanup. Consider the surrounding architecture.
Newly discovered prerequisite state, lifecycle transitions and call chains are in
scope. Establish their initialization, updates, ordering and cleanup through the
proper owners, fix missing or wrong prerequisites in the same change, and revise the
implementation plan when evidence demands it. Leave a residual only for a large
separate mechanism, and tell the user.

Research may range wider and run ahead; implementation and refactoring stay on the
current chain. Within it, fix affected ownership, duplication and maintainability
issues without unrelated redesign. Finish migrations, update consumers and remove
duplicate/superseded code and state in the same change. Preserve intentional
differences, validate affected paths and retain existing owners for shared
dependencies still used elsewhere. An outside contribution is bounded by its issue's
stated done-criteria; prerequisites found beyond it become issues, not commits.
Record unrelated findings as follow-ups.

File `refactor` issues only for work outside the chain or too large for it; add
`tooling` for tools, tests, CI or workflow. Search open issues for a match first.
Describe location, impact and work delayed, not a proposed fix. Comment only with
meaningful new evidence. Verify claims; correct or close false ones. Prioritize by
recorded cost, and close or correct affected issues in the PR that changes them.

Residuals name trigger, effect, frequency and downstream risk. Record the RNG draws,
timer writes and detach calls the chain passes in its residuals or ledger row, even
when they are not ported. Behavior invented where a native body exists is a recorded
residual with a reason, never a silent default. Deferring required loop or
determinism/authority/lifecycle work cannot close that loop.

Delegate independent work with clear ownership. Changes to gameplay behavior get one
read-only critic before their PR; [review.md](.agents/skills/_shared/review.md) holds
the critic rules. Keep a concise [checkpoint](.agents/skills/_shared/handoff.md) for
sustained work.

## Git and validation

Check actual Git/worktree/process state before mutating. Never alter another task's
files, refs or processes; untouched-file failures require causal investigation.

Start `feature/<topic>` from fetched `origin/main`; isolate owned/dirty checkouts.
Continue task-owned branches and commit validated increments. Never commit/push
directly to `main`. Publication requires user/goal authority; PRs target `main`.
Integrate promptly when authorized. Owners resolve conflicts and revalidate.
Preserve unique/local data: remove a worktree or branch only once its work is merged or
saved, with `git worktree remove` and `git branch -d`, never forced.

Preserve contributor credit. Use verified GitHub usernames unless the contributor
explicitly prefers another public name; raw Git names and email addresses do not
establish that preference. Before merging, check the final author identity, message
and trailers using the [attribution rule](.agents/skills/review-pr/SKILL.md#contributor-attribution),
then verify the resulting commit.

After each fetch, fast-forward `<main-checkout>` (`merge --ff-only origin/main`) when it is on
`main` without tracked changes or a Cargo run; sessions load this contract from it.

Choose validation appropriate to the change, considering native fidelity, connected
production behavior (with runtime reproduction when needed) and protection against
regressions. Reassess worsening fixes.

Avoid tests that merely mirror the implementation or require maintaining a second implementation of the same logic.
Before fixing a bug whose expected behavior is established, first make a focused test fail on it where practical.

**Debug narrowly.** Rerun failing tests while debugging, then affected coverage.
Skip `cargo check` when a planned test answers the same question.

- Working Rust: start with focused
  `cargo test -p <crate> --lib <module_path>::`, where `<crate>` is the module's crate
  from the split above (`vera20k-core` or `vera20k`).
- Rust PR readiness: after any critic fixes, focused tests for the modules the change
  touches and their direct consumers, with retail `ini/` and
  `VERA20K_REQUIRE_RETAIL_INI=1`; no baseline runs. The full
  `cargo test --workspace --lib` is not required; run it only when asked. Tests
  tagged `#[ignore = "slow; …"]` run (`-- --ignored`) only when your change touches
  what they cover. Don't dispatch the manual platform workflows unless asked.
- Clippy: once a month on `main`, not per PR: `cargo clippy --workspace --lib` in the
  same environment, with its fixes in one PR.
- Asset binding, loader or rules-closure changes: a `--profile fast` retail map load
  before merge; the lib suite never runs the app loader against retail assets.
- Docs/Markdown-only skills: check content, links/examples and relevant tooling
  (skill sync when applicable); no Cargo or full Python suite.
- Python tools/skill scripts: run `python -m tools.run_tests` once on the final
  candidate with `tools/requirements-test.txt` installed. Failures are regressions;
  the suite passes on `main`.
- Every `cargo test` uses `--lib`.

Run Cargo through `python -m tools.cargo_run -- <cargo arguments>` from the checkout.
It waits for other builds and serializes cooperating worktrees; do not compete with or
kill a compile. Each compile blocks every other session's Cargo until it finishes, so
batch edits and run the narrowest command that answers your question.
Build captures and play-tests with `--profile fast`, timing and performance numbers with
`--release`. Default to unlabelled builds; saved builds, their retirement and low free
space follow the [Cargo ownership procedure](tools/README.md#cargo-ownership-and-labeled-builds).
Run `python -m tools.prepare_checkout` in a new worktree or clone.
Format edited leaf files only (`rustfmt --edition 2024 <file>`), never crate-wide
or recursive `mod.rs`. Coordinate rebaselines; exclude others' WIP.

Don't poll: each check re-reads the whole session context. Run no separate build
process checks. Wait for a long command or CI
through one blocking call or the host's completion notice, not `sleep`/`until` loops
or repeated status queries. When merging is authorized, enable auto-merge on the
validated PR (`gh pr merge <number> --auto --merge`) and move on; GitHub merges it
once every required check passes. Confirm it merged before publishing your next PR.

## Knowledge and guidance

Keep current contracts, focused implementation rationale and reproducible native
evidence close to their code or tools. Update the current owner rather than
maintaining chains of superseded reports. Retention is not proof of correctness.

Internet documentation lookup is allowed without routine approval. Resolve uncertain
technical behavior using authoritative references, specifications and upstream source;
match library/API documentation to the version in use. Graphics work requires both
API knowledge (e.g. wgpu) and rendering principles: visibility/depth, blending,
color/palette math, projection/sampling, GPU execution and performance. Cite
consequential findings near the implementation or review. Validate affected production
output and performance with appropriate captures, GPU readbacks or profiling;
documentation and CPU-only tests alone do not establish rendered gamemd parity.
For Rust style beyond this contract, consult the
[condensed Rust guidelines](.agents/skills/_shared/rust-guidelines.md) when shaping
APIs, hot loops, error handling or tests; this contract wins on conflict.

Resolve `<main-checkout>` with `git worktree list`; its `ini/`, config and `LOCAL.md`
are machine-local. Use `asset`/`asset-browser`; a successful parse or plausible
render is not correctness proof.

Tools follow the same one-owner rule as code. Before writing a helper, look for an
existing tool in `tools/` and `src/bin/`, and extend it instead of copying it. Put
tools and fixtures another session would need in the repo; one-off scripts and run
artifacts stay in scratch.

Check compatibility before dependency changes; document non-obvious decisions near
their owner. Edit skills in `.agents/skills/`; generate Claude copies with
`python tools/skill_sync.py --write`, verify with `--check`. Keep conditional detail
where needed; remove superseded rules.

## Codex notes

Use Codex task tools for cross-session context; verify current source/Git before
treating past conclusions as current. Load only relevant skills and references.

## Claude Code notes

Use host-native tools and relevant skills. For Codex session recovery, consult the
primary checkout's `LOCAL.md`. Preserve explicitly requested `/loop` invocations.
Memory holds durable preferences, not progress claims.
