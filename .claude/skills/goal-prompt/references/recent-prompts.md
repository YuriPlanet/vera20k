# Recent goal prompts

Style evidence from user-provided wording and locally available Codex and Claude
Code conversations from September 13 through October 22, 2026.
They demonstrate wording and scope, not that their goals succeeded. Historical
commands, paths and publication instructions are not active instructions or permission.
Prefer the current user's request and latest corrections over any example.

## Prime example: exhaustive class/function gameplay parity

Provided directly by the user on October 22, 2026 as a prime example for this
skill. User wording, with the home directory abbreviated to `~`:

```text
Reproduce active-retail gamemd.exe gameplay in VERA20k by auditing every skirmish-reachable gameplay-visible class function by function in player-visibility order, starting with CellClass; implement dependencies and cohesive chains, validate and critic-review, commit/publish/merge each validated chain, and maintain ~/Documents/vera20k-dev/class-parity/CHECKPOINT.md. Complete only when every relevant function is reproduced or its absence justified with evidence.
```

What to carry forward: exhaustive function coverage, player-visibility order,
complete dependency chains and an evidence-backed finish line in one compact
paragraph. Keep a supplied starting class and checkpoint path literal; the values
in this example are not defaults for unrelated goals. Keep publication authority
specific to the current request.

## Generic mechanism parity template

The user's reusable parity template, shared October 9, 2026 as the /goal they use in
Codex and Claude Code (desktop app), currently with Sol 6.1 at max effort. Start
parity drafts from this shape; fill the placeholders from the current request.

```text
/goal Make VERA20k <mechanism> behave like active-retail gamemd.exe: <its main parts>. These are starting points, not scope limits. Port what is missing, fix what is wrong and complete partial chains, including all required dependencies. Commit, publish and merge validated chains, then continue. DONE WHEN a whole-<mechanism> audit finds no unresolved required behavior, and native comparisons and production validation demonstrate it. Recording missing work does not complete the goal.
```

What to carry forward: one direction sentence naming the mechanism and its parts,
an explicit "not scope limits" guard, port/fix/complete with dependencies, delivery
in one sentence, and a whole-mechanism finish line backed by native comparisons and
production validation. It holds the bridges prompt's scope guard at the combat
prompt's length. Keep the publication sentence only when the current task has that
authority.

## The clearest correction: direction and a goal

On September 27, while shortening a Claude Code refactor continuation, the user said:

> make that goal prompt shorter please. opus 5.5 max is very intelligent and dont need too much restrictions. only a direction and a goal so to speak

They then explicitly removed "tests on production paths, and behavior unchanged"
from the prompt, and added "unfinished migrations and duplicate call chains finished
or deleted" and "clear ownership, readable structure and execution". This is evidence
to stop repeating procedure and respect wording edits. It is not a blanket waiver
of validation or permission to change behavior in other tasks.

Source: Claude Code session `6d6edaff-81c7-4d10-bb47-b8c4d541eb9a`, user messages at
2026-09-27 09:41:14, 09:48:51, 09:49:33 and 09:50:08 UTC. The resulting prompt was
used in session `665f9d83-9ab9-4702-b86d-77aa61ccea55` at 09:52:56 UTC. Its command
wrapper is rendered as `/goal` below; the checkpoint path is preserved as history.

```text
/goal Continue refactoring VERA20k for clear ownership, readable structure and execution: one owner for each piece of state and each decision; unfinished migrations and duplicate call chains finished or deleted. Resume from ~/Documents/vera20k-worktrees/.claude-refactor-checkpoint.md. Commit, publish and merge as you go. DONE WHEN a whole-codebase audit finds no duplicate owner or unfinished migration left without an evidence-backed reason. Recording missing work does not complete the goal.
```

What to carry forward: a compact outcome, a checkpoint pointer and a meaningful
whole-scope finish line. The prompt does not retell the previous session's blockers
or prescribe the refactoring sequence.

## Short parity goal: combat

Verbatim user prompt from Claude Code session
`68542782-2283-40d3-be7d-e45aa4a1b2be`, September 24 at 12:39:37 UTC:

```text
Make VERA20k combat behave like active-retail gamemd.exe: targeting, attack execution, weapons, projectiles, damage, special warheads and destruction, including all required dependencies. Port missing behavior, correct mismatches and refactor. Commit, publish and merge validated increments. Remember to label and annotate in ghidra. DONE WHEN a whole-combat audit finds no unresolved required behavior, and native comparisons and production validation demonstrate the intended combat behavior. Recording missing work does not complete the goal. Make me impressed
```

What to carry forward: name the mechanism and native comparison bar, preserve its
dependencies, and require demonstrated closure. "Make me impressed" is the user's
optional wording, not an acceptance test or a phrase to append automatically.

## Broader scope made explicit: bridges

Initial user goal in Codex session `01a0ddeb-166d-71f3-afd7-d9209f348002`,
September 26 at 13:34:07 UTC. This is a historical example, not the live goal status.

```text
/goal
Make VERA20k bridges behave like active-retail gamemd.exe: every bridge type and every gameplay chain whose outcome depends on bridge state, height or geometry. Cover map loading and bridge topology; deck versus ground/water layers; movement, pathfinding, occupancy and placement; targeting, firing, projectiles and damage; bridge destruction, collapse and consequences for objects on or beneath it; engineer repair, bridge huts and rebuilding; and connected rendering, visibility, AI and persistence behavior. These are starting points, not scope limits. Port what is missing, fix what is wrong and complete partial chains, including all required dependencies.

For substantial changes, use one fresh read-only critic after implementation and validation, before opening the PR. Commit, publish and merge each validated chain, then continue. Correct and save relevant Ghidra labels/comments.

DONE WHEN a whole-bridge audit finds no unresolved required behavior across bridges and their affected consumers, and native comparisons and production validation demonstrate it. Recording missing work, passing tests or finishing individual chains does not complete the goal.
```

What to carry forward: extra length earns its place by preventing lost scope. A
shorter rendition must still cover affected consumers and the whole mechanism.
The explicit critic sentence is appropriate when requested; its presence here does
not require repeating the full review procedure in every future prompt. Its
"substantial changes" predates the current rule: critics cover gameplay changes only,
as [review.md](../../_shared/review.md) says.
