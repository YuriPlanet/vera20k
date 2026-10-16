# VERA20k — Project contract

**Outside contributors and their agents:** follow the README's
[Contributing](README.md#contributing) section and your issue, and skip the rest of this file.
It is the contract for the maintainer's own agents: its build runner, retail-data checks,
critic reviews, checkpoints, evidence levels and native comparisons don't apply to you. Where
the two differ, the README wins.

VERA20k is a cross-platform Rust reimplementation of the Command & Conquer:
Yuri's Revenge engine (`gamemd.exe`), using original retail rules and assets.
The goal is to reproduce native gameplay behavior through a Rust-native architecture,
while scaling to **20,000 units and 30 players**. VERA20k must build and run on Linux,
macOS and Windows; keep architecture and dependencies compatible.

All simulation-affecting math must produce identical results across supported
platforms and CPU architectures for identical state, inputs and RNG. Prefer
`SimFixed` for simulation calculations; document and validate any differences
from native precision and rounding.

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
Research/review alone does not authorize implementation. Preserve scope amendments
and stop instructions across continuations.

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

Distinguish and cite:

- **Native behavior established:** body/caller/data evidence, or retail play
  observed side by side.
- **Rust regression tested:** named implementation checks.
- **Parity demonstrated:** gamemd-derived executable comparison or exhaustive proof,
  bounded by stated coverage. A sample cannot certify the whole mechanism.

Parity goldens come from native execution/emulation, capture or retail bytes,
not hand calculations or prior Rust; inputs a comparison takes from VERA need
their own native evidence. Avoid unqualified “VERIFIED”/“complete”.

Execute native code ([Unicorn](tools/native_oracle.md) or capture) and pin the results
as golden values in Rust tests only where a misread is likely and spreads: shared numeric
kernels (lookup tables, fixed-point conversion, distance and angle math), RNG draws
(count, order, stream), arithmetic with a known trap (x87 precision, rounding, overflow,
signed division or shifts), or behavior a playtest or bug report shows differing.
Everything else, including control flow, ordering, gameplay rules, timers and plain
arithmetic, may rest on instruction-level reading, a behavior test through the
production code and side-by-side play against retail. Each PR states the evidence level
each claim reached.

Keep a native comparison only while a Rust test or tool consumes it: its harness and a
small corpus of controls rather than bulk replays, with a sidecar recording the binary
hash and the command. Sidecars pin no hashes of repository sources; those go stale
without any change in native output. Validation results (test counts, captures, critic
findings) go in the PR description and the checkpoint, not the tree: do not
commit receipts, run logs, capture screenshots, critic reviews or static disassembly
packets.

Each cohesive gamemd-derived Rust behavior carries nearby native identity/address
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
over minimizing line count.

Prefer data-oriented design for simulation hot loops. Match gamemd's results, not its
execution strategy: batch, vectorize or parallelize freely. Update order may differ
from gamemd's when it stays deterministic and causes no noticeable gameplay change;
within one object's update, only for a profiled hot spot. Record each order change in
its PR for side-by-side play.

Choose boundaries and abstractions by responsibility and consumers, not line/type
counts or C++ structure. Preserve state authority, lifecycle, tick phase order, timers,
same-tick effects, persistence and numeric semantics under the policy below.
Document and validate floating-point use where native behavior requires it.
Storage order and active-object order are distinct.

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

VERA20k contains duplicate state, functions and call chains from incomplete migrations.
Trace their use before changes. Untangle affected ownership, consolidate duplicates,
finish required migrations and remove obsolete code/state. Preserve intentional
differences, validate affected paths and keep cleanup within task scope.

Fix refactoring in the code your change touches. File a GitHub issue labeled `refactor`
only for what would change behavior outside your chain or is too big for the change;
add `tooling` when it concerns tools, test infrastructure, CI or workflow. Search open
issues first and comment on a match instead of filing a duplicate. Say where it is, why
it hurts and what work it slowed down, not how to fix it, and comment again each time
it slows work down. Issues are leads: verify them before acting, and correct or close
any that prove wrong. Pick refactoring work by that recorded cost. When your PR fixes or
changes what an open issue says, close or correct it in that PR.

Simulation state and shared decisions have one authoritative owner. Before adding
state or decision logic, find existing writers and name the owner in the PR. Extend
or fix that owner instead of introducing competing state or duplicated decision
logic. Keep authoritative state private to its owning module and expose mutations
through the owner. Never widen visibility to reach another owner's state or raw data;
extend the owner instead.

Before porting a native function, search the code for its address (e.g. `703850`) and
native name. Each native function has one Rust port; new callers call it instead of
adding a wrapper or re-inlining its body. Never fork an owner: when an existing one is
wrong for your chain, correct it for every caller (a change toward native is in scope,
with evidence for each affected path) or use it as it is and record the residual. A
change that adds a copy, a caller-specific wrapper or a competing owner is a defect to
remove before merge; the critic checks for them.

Derived caches and indexes are allowed when their source of truth, update or
invalidation rules, and consistency validation are explicit.

One owner follows a complete mechanism through evidence, implementation, production
integration and review. Consider the surrounding architecture and affected consumers,
and use integration evidence appropriate to the change, including runtime reproduction
when needed. Reassess worsening fixes.
Design/plan artifacts are optional; implementation authority includes design choices.

Work one complete chain at a time: a player-visible mechanism traced through its
native call chains from trigger through prerequisites, admission, effects, downstream
consumers and cleanup, across class and subsystem boundaries. Bound each chain to one
common path (unit, weapon or screen route) so it stays reviewable; other paths are
later chains. Pick the next chain by player visibility and frequency. Research may
range wider; implementation stays on the current chain. In the same change, migrate
affected consumers, delete superseded paths, and remove duplicates and dead code in
the code the chain touches. Shared dependencies still used by other mechanisms remain
with their existing owner.

Record the RNG draws, timer writes and detach calls the chain passes in its residuals
or ledger row, even when they are not ported. Behavior invented where a native body
exists is a recorded residual with a reason, never a silent default.

Follow native dependencies across subsystem and class boundaries wherever the selected
behavior requires them. Newly discovered prerequisite state, lifecycle transitions and
call chains are in scope. Establish their initialization, updates, ordering and cleanup
through the proper owners, and revise the implementation plan when evidence demands it.
Fix missing or wrong prerequisites in the same change; leave a residual only for a large
separate mechanism, and tell the user. An outside contribution is bounded by its issue's
stated done-criteria; prerequisites found beyond it become issues, not commits.
Preserve explicit user exclusions and stop instructions; record unrelated findings as
follow-ups.

One chain is one PR: don't split a chain into several PRs or open one per gap, and
don't stack unmerged implementation branches (researching ahead is fine).
Residuals name trigger, effect, frequency and downstream risk; deferring required loop or
determinism/authority/lifecycle work cannot close that loop.

Delegate independent work with clear ownership. For changes to gameplay behavior, run
one fresh read-only [critic](.agents/skills/_shared/review.md) after implementation
and focused tests, before the full suite. The critic is free to inspect original evidence
and challenge scope/design. It identifies implementation defects and useful refactoring
opportunities, explaining their impact and risks. The owner fixes confirmed defects,
rejects false positives with evidence and may implement worthwhile in-scope refactors,
validating all changes. Unrelated opportunities become `refactor` issues. Each validated,
dependency-coherent mechanism gets its own PR and at most one critic pass; do not
hold validated mechanisms back to batch them into one review. Do not run critics per
implementation increment within a mechanism or repeat reviews after fixes or revisions
unless the user explicitly asks.
Keep a concise [checkpoint](.agents/skills/_shared/handoff.md) for sustained work.

## Git and validation

Check actual Git/worktree/process state before mutating. Never alter another task's
files, refs or processes; untouched-file failures require causal investigation.

Start `feature/<topic>` from fetched `origin/main`; isolate owned/dirty checkouts.
Continue task-owned branches and commit validated increments. Never commit/push
directly to `main`. Publication requires user/goal authority; PRs target `main`.
Integrate promptly when authorized. Owners resolve conflicts and revalidate.
Preserve unique/local data; use `sync` for complex cleanup.

Choose validation appropriate to the change, considering native fidelity, connected
production behavior and protection against regressions.

Avoid tests that merely mirror the implementation or require maintaining a second implementation of the same logic.
Before fixing a bug whose expected behavior is established, first make a focused test fail on it where practical.

- Working Rust: `cargo check --workspace` as needed; focused
  `cargo test -p <crate> --lib <module_path>::`, where `<crate>` is `vera20k-core`
  for engine modules and `vera20k` for `app`, `render`, `ui`, `audio` and `asset_tools`.
- Rust PR readiness: once per PR, after any critic fixes, one full
  `cargo test --workspace --lib` with retail `ini/` and `VERA20K_REQUIRE_RETAIL_INI=1`;
  no baseline runs, and rerun only after a failure or a `main` merge conflict. Tests
  tagged `#[ignore = "slow; …"]` run (`-- --ignored`) only when your change touches
  what they cover. Don't dispatch the manual platform workflows unless asked.
- Clippy: once a month on `main`, not per PR: `cargo clippy --workspace --lib` in the
  same environment, with its fixes in one PR.
- Asset binding, loader or rules-closure changes: a release-build retail map load
  before merge; the lib suite never runs the app loader against retail assets.
- Docs/skills: validate content, links/examples and tooling; no Cargo suite.
- Python tools and skills (`tools/`, `.agents/skills/`): one `python -m tools.run_tests`
  on the final candidate, with `tools/requirements-test.txt` installed. It passes on
  `main`, so treat any failure as a regression.
- Every `cargo test` uses `--lib`.

Run Cargo through `python -m tools.cargo_run -- <cargo arguments>` from the checkout.
It waits for other builds and serializes cooperating worktrees; do not compete with or
kill a compile. Each compile blocks every other session's Cargo until it finishes, so
batch edits and run the narrowest command that answers your question.
Default to unlabelled builds; label only active comparison, capture or debugging binaries.
Prefer release captures and retain debug tests selectively. Reference hashes, manifests
and results without extra executable copies. After each merge, dry-run retirement of
superseded owned labels, preserving required binaries, dependencies and evidence.
Resolve unmet minimum free space before another large build; follow the
[retention procedure](tools/README.md#cargo-ownership-and-labeled-builds). Run
`python -m tools.prepare_checkout` in a new worktree or clone.
Format edited leaf files only (`rustfmt --edition 2024 <file>`), never crate-wide
or recursive `mod.rs`. Coordinate snapshot versions/rebaselines; exclude others' WIP.

Don't poll: each check re-reads the whole session context. `cargo_run` already waits
for other builds, so run no separate process checks. Wait for a long command or CI
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
