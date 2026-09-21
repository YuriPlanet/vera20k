---
name: goal-prompt
description: >
  Compose, revise or review an autonomous VERA20k goal prompt or cross-session
  continuation. Produces paste-ready text; never launches or schedules the goal.
---

# Goal Prompt

Compose paste-ready text only; do not launch or schedule it. Use the user's preferred
August 30–September 6, 2026 pattern: clear outcome, production evidence, independent
review before PRs, integrated increments, and persistence through whole-goal acceptance.
These are experience-backed defaults, not a guarantee of success or a fixed procedure.

Establish a clear vision and an inspectable acceptance bar before implementation.
Make these and the review cadence below explicit in every implementation goal;
brevity must not erase them.

Keep the prompt concise, usually a few connected paragraphs. Reference
[ENGINE.md](../../../ENGINE.md) for shared rules. Preserve explicit scope, exclusions,
publication authority (including established user preferences), model/effort, time/token
limits and invocations such as `/goal` or `/loop`; invent none. Save prompt files only
when asked. Leave design, decomposition and tools open except for real constraints;
do not impose upfront ledgers, frozen designs or research-only phases by default.
Infer routine choices from the task and these preferences; the user need not supply
an execution checklist. Include only clauses that help this particular goal succeed.

## Shape the goal

State the outcome, why it matters, an inspectable comparison bar and production
acceptance. For parity, use active-retail gamemd bodies/callers and retail data;
plans, labels and existing owners are hypotheses. Preserve what matches, replace
what is wrong and implement what is missing. Interleave research and implementation
per coherent mechanism. Require evidence that the Rust production path reaches the
behavior; disconnected or unit-test-only work does not complete the mechanism.
For refactors, require demonstrated maintenance benefits and preserved production
behavior; for tooling, a concrete user workflow. Adapt evidence to that domain.

For every implementation goal, one builder owns a coherent mechanism through
implementation and validation. For substantial/risky changes, run one fresh read-only
critic who did not build the change before opening the PR, with the requirement,
original evidence, complete diff and actual validation output.
The critic independently checks the evidence and may challenge priorities, design,
exclusions, production reachability and tests. The requirement and builder's account
are claims to verify, not limits on inquiry. Fix all confirmed findings, justify
rejected findings with evidence, and have the owner validate fixes and affected
conclusions. Do not run critics per implementation increment or repeat reviews after
fixes or revisions unless the user explicitly asks. Keep delegation prompts short.
Adjacent discoveries do not automatically expand implementation scope; follow
ENGINE.md's prerequisite guidance.

For reverse-engineering goals, include evidence maintenance: after independent
confirmation, correct relevant Ghidra labels/comments and source annotations, and
update affected research, plans and current-state documents. Check native bodies,
callers, receivers and active-YR reachability before assigning identities. Replace
disproven claims rather than leaving contradictory guidance; retain useful evidence
and explicit uncertainty. Derive status from actual source, validation and Git;
distinguish implemented, validated and merged work. Keep updates close to the changed
mechanism, without creating permanent trackers or broad documentation chores.
Follow the [Ghidra working notes](../../../docs/research/ghidra-workflow.md): respect
read-only/no-sync instructions, coordinate one writer for shared annotations, save
and read back changes. Annotation authority does not authorize unrelated analysis
repairs or binary patches. Include equivalent documentation upkeep in other domains
only where the work changes existing claims or usage.

For implementation goals, the user's standing preference is to let the executor
create or update PRs when it judges the work coherent and useful to review. Carry
that permission into the prompt unless the current request narrows it. Choose PR
boundaries and timing by dependency coherence and reviewability, not one PR per
plan row or an arbitrary batch size. Drafts may expose unfinished work when useful;
they do not count as accepted work. Preserve any explicit no-draft restriction.
When merge is also authorized, integrate validated increments, then continue from
refreshed origin/main; do not accumulate unmerged accepted work. PR creation alone
does not grant merge authority. Validate before merging; reuse the pre-PR critic pass,
with subsequent fixes validated by the owner.
Preserve narrower authority when supplied. If blocked, keep the mechanism open and
continue independent in-scope work when possible. A passed critic, commit, PR or
finished mechanism does not complete a larger goal. Proceed autonomously through
authorized work; ask only for missing authority or an undiscoverable user-only decision.

Preserve the selected completion standard. Exhaustive parity leaves every unresolved,
unverified, approximate, missing or residual in-scope mechanism open. Require a final
whole-scope reverse audit by the owner for omissions, cross-mechanism gaps and
regressions, plus applicable production and ENGINE.md validation. Ranked or refactoring
goals may finish with explicitly allowed deferrals or evidence-backed no-change
decisions. Never silently substitute that standard for exhaustive closure.

## Continuation

Prefer a reusable goal whose progress can be rederived from current source, evidence
and Git rather than stale plan statuses. When ownership or unfinished work needs
transfer, prepend only the necessary branch/worktree, relevant HEAD/unmerged work,
artifact locations, review/validation state and next safe action. Read the governing
prompt and latest amendments; adopt supported work, recheck contradicted premises
and preserve scope, budget, publication and stop instructions. Do not revive a
superseded goal.
