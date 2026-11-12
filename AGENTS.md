# VERA20k — Project contract

**Outside contributors and their agents:** follow the README's
[Contributing](README.md#contributing) section and your issue, and skip the rest of this file.
Keep each PR small: one fix, one to three regression tests that fail without it, the
native function address cited next to the code, no native-execution corpora, and a PR
description of about 15 lines. Leave code marked as a sim-scale deviation as it is: it
differs from gamemd on purpose, so VERA can run 20,000 units. If it plays visibly wrong,
open an issue. The rest is the contract for the maintainer's own agents:
its build runner, retail-data checks, critic reviews, checkpoints and native comparisons
don't apply to you. Where the two differ, the README wins.

VERA20k is a cross-platform Rust reimplementation of the Command & Conquer:
Yuri's Revenge engine (`gamemd.exe`), using original retail rules and assets.
It must build and run on Linux, macOS and Windows, and its browser preview must keep
compiling; keep architecture and dependencies compatible.

What it is for, in the order that decides when two of these pull apart:

1. **It works.** No crash, no desync, and a match that starts. A fault players hit in
   a released build comes before other work.
2. **It plays like the original.** With nothing turned on, a player sees and gets what
   `gamemd.exe` gives: native code and retail data establish what to reproduce, through
   a Rust-native architecture.
3. **It runs big battles.** **20,000 units and 30 players**, also on older PCs. Where a
   player cannot notice the difference, speed wins over gamemd's execution order.
4. **It adds to the original.** Ares and Phobos features, and new ones only VERA has,
   are wanted: extensions switch on from a mod's or map's data, VERA's own features
   from INI files or the Options screen. The original stays the default.
5. **Playing online is easy.** A match starts from inside the game, and an older
   version is told plainly when it can no longer join.

This contract governs Codex and Claude. Specific user instructions override workflow
defaults.

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
timers and plain arithmetic. Comments are claims: when behavior changes, search for
the address and fix every comment that still states the old behavior. Avoid
unqualified “VERIFIED”/“complete”.

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

Sim-behavior commits cite their evidence. Consult the
[Ghidra reference](docs/ghidra-workflow.md) for access, interpretation pitfalls and
shared-database edits.

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
leave unset is dormant, not unreachable. Check retail values with `asset ini-get`
([INI lookup](tools/ini_lookup.md)), a gamemd-exact parse, not grep. When INI values
decide an outcome, test through the production reader on retail INIs. Details:
[retail INI reference](src/rules/mod.rs).

## Stability, extensions and online play

Outside data never stops the game: clamp or refuse values from INI files, maps, saves
and the network, never `unwrap`, `expect` or index on them, and carry on wherever
gamemd would. When something fails for a player, say in the game what happened and
what they can do, not only in the log.

Stock play stays as gamemd plays it. An engine extension (Ares, Phobos, NPatch, VeraX)
is in every build and switches on from the game's data: its keys in the rules or map, or
a mod that ships it. With retail data the game plays as gamemd. Ares has that switch
(`RuleSet::ares`): new Ares code reads its own keys or the switch, never
`cfg(feature = "ares")`, and a PR that touches older `ares`-gated code moves it to the
switch. The others stay behind their build features until they get one. A VERA feature
stays off until a player option or an INI key turns it on; a bug gamemd itself has stays
unless the maintainer asks for an opt-in fix (`npbug`). A change in or beside a
[capability](docs/capabilities.md) compiles with it on as well as off
(`python -m tools.capability_tests build <name>`; `focus <name>` runs its tests).

An extension behavior is a hook into gamemd: cite the hook address, the stock
instructions it replaces and where it resumes, with the extension's source. Stock code
the hook skips does not run; code it resumes into still does. Read its keys with the
extension's own reader rules and defaults. Extensions mix: none assumes another is
absent or re-owns its state
([cross-extension rule](docs/npatch.md#cross-extension-compatibility)).

VERA's own features come from data too: a new rule or game mode is INI keys a modder
or map can set, documented under `docs/`, not names in the code.

Online play reaches players on other versions. A change to what game, relay and lobby
exchange keeps [Networking](docs/Networking.md) current and says in its PR what a
released game, a mixed room and a rollback do with it: the older side is refused with
a message or the feature stays off, and nothing hangs or crashes.

## Architecture and delivery

Use the simplest implementation that fully satisfies the required behavior. Avoid
unnecessary abstractions, duplicated logic and speculative features; prefer clarity
over minimizing line count. Choose boundaries and abstractions by responsibility and
consumers, not line/type counts or C++ structure.

Prefer data-oriented design for simulation hot loops. Match gamemd's results, not its
execution strategy: batch, vectorize or parallelize freely. Update order may differ
from gamemd's when it stays deterministic and causes no noticeable gameplay change;
within one object's update, only for a profiled hot spot. Record each order change in
its PR for side-by-side play, and next to its code as a sim-scale deviation. Keep them:
moving one back toward native needs a `--release` sim-bench run at 20,000 units and 30
houses showing frames are no slower, or the maintainer's agreement to the cost.

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
common unit, weapon or screen path; other paths follow later. One chain is one PR:
don't split a chain into several PRs or open one per gap, and don't stack unmerged
implementation branches. Plans are optional; implementation authority includes design
choices.

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

File `refactor` issues (`tooling` for tools, tests, CI or workflow) only for work
outside the chain or too large for it. Search for an open match first, describe
location and impact, not a fix, and close or correct an issue in the PR that changes it.

Residuals name trigger, effect, frequency and downstream risk. Record the RNG draws,
timer writes and detach calls the chain passes in its residuals or ledger row, even
when they are not ported. Behavior invented where a native body exists is a recorded
residual with a reason, never a silent default. Deferring required loop or
determinism/authority/lifecycle work cannot close that loop.

Delegate independent work with clear ownership. Changes to gameplay behavior or online
play get one read-only critic before their PR;
[review.md](.agents/skills/_shared/review.md) holds the critic rules. Keep a concise
[checkpoint](.agents/skills/_shared/handoff.md) for sustained work.

## Git and validation

Check actual Git/worktree/process state before mutating. Never alter another task's
files, refs or processes; untouched-file failures require causal investigation.

Start `feature/<topic>` from fetched `origin/main`; isolate owned/dirty checkouts.
Continue task-owned branches and commit validated increments. Never commit/push
directly to `main`. Publication requires user/goal authority; PRs target `main`.
Integrate promptly when authorized. Owners resolve conflicts and revalidate.
Preserve unique/local data: remove a worktree or branch only once its work is merged or
saved, with `git worktree remove` and `git branch -d`, never forced.

Preserve contributor credit: before merging someone else's work, check the final author
identity, message and trailers against the
[attribution rule](.agents/skills/review-pr/SKILL.md#contributor-attribution), then
verify the resulting commit.

After each fetch, fast-forward `<main-checkout>` (`merge --ff-only origin/main`) when it is on
`main` with no tracked changes and no Cargo run from it; sessions load this contract
from it.

Before a run (test, capture, benchmark, replay), ask which result would change your
next step; if none would, skip it. When reading the code settles the question, say so
and offer to close the task: don't build harnesses or add runs to complete a set. In a
goal's finish line, "native comparison" means comparing gamemd's instructions with the
Rust, plus native execution only where Exactness and evidence calls for it, and
"production validation" means the changed path ran once in the game or a headless map.
Never run gamemd as a whole game under Wine to compare.

Performance work: find waste by reading the code, starting where existing profiles
point, and fix it. Measure once per chain, at the end: one `--release` before/after,
with sim-bench's final state hash unchanged for a simulation change. No timing per
step.

A check counts only if it ran: the test filter matched tests, no retail test printed
`SKIPPED`, each build printed `Finished`, and an empty search counts only after the
same search finds a known use.

Avoid tests that merely mirror the implementation. See each new test fail without the
fix: write it first, or run it once with the fix reverted, and say so in the PR.

Skip `cargo check` when a planned test answers the same question. A test run does not
show that the game builds, though: tests switch on core's `test-support` code. When
production code touches anything behind `test-support` or `#[cfg(test)]`, also run
`python -m tools.cargo_run -- check -p vera20k --lib --bins`; for a file listed in
`.github/workflows/wasm.yml`, the wasm check in the [Wasm guide](docs/wasm.md#checks).

- Working Rust: start with focused
  `python -m tools.cargo_run -- test -p <crate> --lib <module_path>::`, where `<crate>`
  is the module's crate from the split above (`vera20k-core` or `vera20k`).
- Rust PR readiness: after any critic fixes, focused tests for the modules the change
  touches and their direct consumers, with retail `ini/`, `RA2_DIR` set,
  `VERA20K_REQUIRE_RETAIL_INI=1` and `VERA20K_REQUIRE_RETAIL_ASSETS=1`; no baseline
  runs. The full suite (`test --workspace --lib`) is not required; run it only when
  asked. Tests tagged `#[ignore = "slow; …"]` run (`-- --ignored`) only when your
  change touches what they cover. Don't dispatch the manual platform workflows unless
  asked.
- Clippy: not per PR; once a month on `main`, with its fixes in one PR.
- Asset binding, loader or rules-closure changes: a `--profile fast` retail map load
  before merge; the lib suite never runs the app loader against retail assets.
- Docs/Markdown-only skills: check content, links/examples and relevant tooling
  (skill sync when applicable); no Cargo or full Python suite.
- Python tools/skill scripts: run `python -m tools.run_tests` once on the final
  candidate with `tools/requirements-test.txt` installed. Failures are regressions;
  the suite passes on `main`.

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

Run your heavy jobs one at a time: a build, a test run, a sim-bench battle or a
retail-asset scan. Never run a long battle on a `test-support` build; its test-only
recorders grow until the OS kills the run.

Don't poll: each check re-reads the whole session context. Don't look for other builds
before Cargo; the runner waits for them. Wait for a long command or CI
through one blocking call or the host's completion notice, not `sleep`/`until` loops
or repeated status queries. When merging is authorized, merge the validated PR
(`gh pr merge <number> --merge`) and move on; add `--auto` only when the PR lists
pending required checks, and `main` requires none today. Confirm it merged before
publishing your next PR.

## Knowledge and guidance

Keep current contracts, focused implementation rationale and reproducible native
evidence close to their code or tools. Update the current owner rather than
maintaining chains of superseded reports. Retention is not proof of correctness.

Internet documentation lookup is allowed without routine approval. Resolve uncertain
technical behavior using authoritative references, specifications and upstream source;
match library/API documentation to the version in use. Cite consequential findings
near the implementation or review. CPU-only tests do not show what the GPU draws; when
the picture is the question, look at a capture or GPU readback.

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
treating past conclusions as current.

## Claude Code notes

Memory holds durable preferences, not progress claims.
