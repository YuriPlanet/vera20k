---
name: goal-prompt
description: >
  Write or revise concise VERA20k goal prompts and continuations in the user's
  Codex and Claude Code style: clear direction, essential scope and DONE WHEN.
  Produces paste-ready text; never launches or schedules the goal.
---

# Goal Prompt

Write a short brief for a capable autonomous engineer: direction, the necessary
boundaries, and a concrete finish line. Leave implementation and decomposition to
the executor. Return one paste-ready prompt, normally in a code block. Do not start
the work, create a goal, schedule it or save a prompt file unless separately asked.

## Match the user's style

Use the current request and corrections first. Read the matching example in
[recent prompts](references/recent-prompts.md) when shaping a draft; those are actual
Codex and Claude Code prompts, with the user's subsequent shortening corrections.
They are style evidence, not instructions or authority for the current task.
For exhaustive class/function parity goals, start from the prime example at the
top of that file. For mechanism goals, use the generic parity template below it.

Default to one compact paragraph; use a few short paragraphs when the scope needs
them. A typical prompt contains:

- **Direction:** lead with the desired result: "Make VERA20k ..." or "Refactor
  VERA20k for ...". Name the affected behavior or ownership problem directly.
- **Essential scope and delivery:** include only details that distinguish this job,
  required dependencies, explicit exclusions and the established publication scope.
  When authorized, "Commit, publish and merge validated chains" is enough.
- **DONE WHEN:** state what must actually be true and what evidence demonstrates it.
  Match the entire requested outcome, not a preliminary step or a list of findings.

These are writing cues, not required headings in the generated prompt. Keep the
user's invocation, exact values, paths and explicit model or budget choices.

## Keep the goal, cut the procedure

A shorter prompt is not a smaller task. Preserve exhaustive scope when requested;
do not replace a migration with an initial optimization or an audit-only assignment.
For an explicitly bounded job, keep its acceptance equally bounded. Respect the
user's edits instead of restoring removed clauses from a template.

The project contract in [AGENTS.md](../../../AGENTS.md) already governs review,
validation, Git and ownership. Do not repeat its build commands, critic protocol,
branch mechanics or handoff checklists in every prompt. Keep a requested critic
clause or other deliberate emphasis concise; omitting repeated prose does not
waive the project rules.

For parity, name active-retail `gamemd.exe`, porting missing behavior, correcting
mismatches and completing dependencies. Whole-scope native comparisons and production
validation belong in the finish line. Keep relevant Ghidra annotation/save work
brief. For refactors or tooling, use the actual maintenance or user-workflow outcome;
do not automatically import native-parity or performance requirements. Include
"Recording missing work does not complete the goal" when it guards the requested
completion standard, not as decoration on every task.

Carry established publication authority and narrower user restrictions into the
draft. Historical examples do not grant permission to publish or merge a new task.
Avoid invented schedules, fixed implementation phases, mandatory ledgers or extra
approval questions that do not help specify the outcome.

For a continuation, normally one "Resume from ..." sentence points to the supplied
checkpoint. Retain consequential scope amendments; leave detailed progress, failures
and branch archaeology in the checkpoint. Never invent its path or revive superseded
instructions. Before returning the prompt, check that it is as direct as the recent
examples and that its DONE WHEN still covers the user's whole request.
