# VERA20k — Project contract

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
Missing or unproven required behavior keeps an exhaustive task open.

Distinguish and cite:

- **Native behavior established:** body/caller/data evidence.
- **Rust regression tested:** named implementation checks.
- **Parity demonstrated:** gamemd-derived executable comparison or exhaustive proof,
  bounded by stated coverage. A sample cannot certify the whole mechanism.

Parity goldens come from native execution/emulation, capture or retail bytes,
not hand calculations or prior Rust. Avoid unqualified “VERIFIED”/“complete”.

[Unicorn](tools/native_oracle.md) can help with native comparisons; use when useful.

Use native executable comparisons when they resolve a concrete uncertainty that matters to gameplay.

Preserve native comparisons as reproducible harnesses and results, recording binary
identity and coverage limits. Link them to Rust tests where practical; parity claims
must cite saved evidence and actual validation results.

Each cohesive gamemd-derived Rust behavior carries nearby native identity/address
and source; sim-behavior commits cite their evidence.
Consult the [Ghidra reference](docs/research/ghidra-workflow.md) for access,
interpretation pitfalls and shared-database edits.

## Architecture and delivery

Use the simplest implementation that fully satisfies the required behavior. Avoid
unnecessary abstractions, duplicated logic and speculative features; prefer clarity
over minimizing line count.

Prefer data-oriented design for simulation hot loops: organize data for efficient access and batch processing, minimize unnecessary per-entity work.

Choose boundaries and abstractions by responsibility and consumers, not line/type
counts or C++ structure. Preserve state authority, lifecycle, scheduler/RNG order,
timers, same-tick effects, persistence and numeric semantics under the policy below.
Document and validate floating-point use where native behavior requires it.
Storage order and active-object order are distinct.

All math must preserve simulation determinism across supported platforms and CPU
architectures for identical state, inputs and RNG. Prefer `SimFixed` for simulation
math, accepting documented differences from native precision and rounding. Avoid
x87 emulation unless demonstrated gameplay requirements make it necessary; exact
native arithmetic alone is not a requirement to emulate x87. Validate affected
gameplay, range and overflow behavior. Presentation must not affect simulation
determinism.

`sim/` never depends on `render/`, `ui/`, `sidebar/`, `audio/` or `net/`.
App code orchestrates without owning duplicate gameplay. Current module contracts
and `advance_tick` phases describe the architecture. Name coordinate frames/units.

VERA20k contains duplicate state, functions and call chains from incomplete migrations.
Trace their use before changes. Untangle affected ownership, consolidate duplicates,
finish required migrations and remove obsolete code/state. Preserve intentional
differences, validate affected paths and keep cleanup within task scope.

Simulation state and shared decisions have one authoritative owner. Before adding
state or decision logic, find existing writers and name the owner in the PR. Extend
or fix that owner instead of introducing competing state or duplicated decision
logic. Keep authoritative state private to its owning module and expose mutations
through the owner.

Derived caches and indexes are allowed when their source of truth, update or
invalidation rules, and consistency validation are explicit.

One owner follows a complete mechanism through evidence, implementation, production
integration and review. Consider the surrounding architecture and affected consumers,
and use integration evidence appropriate to the change, including runtime reproduction
when needed. Reassess worsening fixes.
Design/plan artifacts are optional; implementation authority includes design choices.

Port one bounded gameplay mechanism at a time, tracing its native call chains from
trigger through all required effects across class boundaries. Migrate affected
consumers and delete superseded paths in the same change. Shared dependencies still
used by other mechanisms remain with their existing owner.

Record the RNG draws, timer writes and detach calls the chain passes in its residuals
or ledger row, even when they are not ported. Behavior invented where a native body
exists is a recorded residual with a reason, never a silent default.

Follow native dependencies across subsystem and class boundaries wherever the selected
behavior requires them. Newly discovered prerequisite state, lifecycle transitions and
call chains are in scope. Establish their initialization, updates, ordering and cleanup
through the proper owners, and revise the implementation plan when evidence demands it.
Preserve explicit user exclusions and stop instructions; record unrelated findings as
follow-ups.

Choose branch/PR boundaries to keep dependencies coherent and reviewable.
Residuals name trigger, effect, frequency and downstream risk; deferring required loop or
determinism/authority/lifecycle work cannot close that loop.

Delegate independent work with clear ownership. For substantial/risky changes, run
one fresh read-only [critic](.agents/skills/_shared/review.md) after implementation
and validation, before opening the PR. The critic is free to inspect original evidence
and challenge scope/design. It identifies implementation defects and useful refactoring
opportunities, explaining their impact and risks. The owner fixes confirmed defects,
rejects false positives with evidence and may implement worthwhile in-scope refactors,
validating all changes. Unrelated opportunities become follow-ups. Each validated,
dependency-coherent mechanism gets its own PR and its own single critic pass; do not
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

- Working Rust: `cargo check -p vera20k` as needed; focused
  `cargo test -p vera20k --lib <module_path>::`.
- Rust PR readiness: one full `cargo test -p vera20k --lib` plus
  `cargo clippy -p vera20k --lib` for the final candidate; repeat only if later
  changes/failures invalidate it.
- Asset binding, loader or rules-closure changes: a release-build retail map load
  before merge; the lib suite never runs the app loader against retail assets.
- Docs/skills: validate content, links/examples and tooling; no Cargo suite.
- Every `cargo test` uses `--lib`.

Before Cargo: `Get-Process cargo,rustc -ErrorAction SilentlyContinue`. Wait for other
owners; never compete or kill a compile. Confirm fresh-worktree config/assets.
Format edited leaf files only (`rustfmt --edition 2024 <file>`), never crate-wide
or recursive `mod.rs`. Coordinate snapshot versions/rebaselines; exclude others' WIP.

## Knowledge and guidance

Keep current contracts, focused implementation rationale and reproducible native
evidence close to their code or tools. Historical investigations live in the
[research archive](docs/research/README.md); consult them explicitly when useful,
recheck their claims, and update the current owner rather than maintaining chains
of superseded reports. Retention or an index status is not proof of correctness.

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
are machine-local. Read retail data before selecting constants.
YR loads standalone `RULESMD.INI`/`ARTMD.INI`/`AIMD.INI`, then applicable language,
mode and map overrides—no underlying RA2 INI merge. Use `asset`/`asset-browser`;
a successful parse or plausible render is not correctness proof.

Check compatibility before dependency changes; document non-obvious decisions near
their owner. Edit skills in `.agents/skills/`; generate Claude copies with
`python tools/skill_sync.py --write`, verify with `--check`. Keep conditional detail
where needed; remove superseded rules.

When changing the shared project contract, update both `AGENTS.md` and `CLAUDE.md`.

## Codex notes

Use Codex task tools for cross-session context; verify current source/Git before
treating past conclusions as current. Load only relevant skills and references.
