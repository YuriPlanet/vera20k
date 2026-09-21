# Ghidra working notes

[AGENTS.md](../../AGENTS.md) defines evidence and delivery. These notes cover tool
behavior and recurring interpretation errors; choose the investigation method yourself.

## Connect to the intended program

Discover the instance and confirm the program path, binary identity and image base
before relying on addresses. Use explicit program selectors when the tool exposes
them; another session can change the shared current program.

The installed bridge registers analysis tools after connection. `check_tools` checks
that registry, not endpoint health: `not_found` before connection does not establish
that an operation is unsupported. After connecting, inspect the current schema,
load needed groups if using lazy loading, and try a relevant read. Tool names,
arguments and availability come from that schema, not an old command list.

Empty discovery does not prove Ghidra is stopped. Inspect the process and configured
connection before relaunching; use machine-local `ghidra-up` when available. Reuse
the analyzed program. Re-importing or enabling analysis is not routine reconnection.

## Interpret evidence

- Names, signatures and pseudocode are interpretations. Resolve consequential
  ambiguity from bytes/instructions, receiver/argument flow and actual callers.
  A nearby label or attractive decompile is not proof of identity.
- Check pointer types: `int *p; p[0xac]` addresses byte offset `0x2b0` on this
  32-bit target. Addition to an integer address uses byte offsets.
- For virtual calls, establish the table/subobject owner, read the actual slot,
  follow receiver-adjusting thunks and check callers. Inspect surrounding instructions
  for questionable boundaries; compiler lifecycle plumbing can resemble gameplay.
- Find state writers and initialization. Zero-filled image data may be populated
  at runtime. Confirm active-YR gates and retail inputs; inherited TS code alone
  does not establish a feature's applicability.

Follow production consumers far enough to establish the claimed result. Visual/audio
work includes composition, active flags, selected assets/frames, timing and output;
a loaded asset or working helper does not prove the final result. Keep address,
verified role and reproducible evidence together, naming uncertainty honestly.

## Preserve findings without polluting shared analysis

During authorized reverse engineering, preserve proven identities and useful evidence
with focused labels, comments and missing references. Read-only requests or
`--no-sync-ghidra-labels` disable these writes; `--sync-ghidra-labels` explicitly requests
them. This policy is the same for serial and delegated work. No separate candidate
ledger is required when a concise finding suffices.

Keep uncertain identities unnamed. References need decoded endpoints, operand and
reference kind; check for duplicates. Type, prototype or boundary repairs belong
within an authorized analysis-repair task, with prior definitions recoverable.
Once that scope is granted, do not ask permission for every edit. Read back structural
repairs immediately, including layout/offsets and affected decompilation. Byte patches,
bulk reanalysis and unrelated database changes need their own task scope.

Use one writer per shared program and coordinate changes affecting other workers'
evidence. Small coherent annotation batches are allowed. Inspect per-item results,
explicitly save the intended program, and read back the changes before unrelated
work or handoff. A committed analysis transaction is not a disk save. After a timeout,
inspect actual state before retrying; report partial or unsaved work accurately.

Label/type changes affect analysis, not executable bytes. Inspect current analyzer
settings when relevant; do not assume historical settings are still in force or
blame drift on an analyzer without evidence.

Tool behavior checked 2026-09-04 against the installed GhidraMCP 5.14.2 bridge
(`connect_instance`, `check_tools`) and plugin (`CommentService`,
`ProgramScriptService.saveCurrentProgram`). No connected instance was available
for a live persistence test; repeat capability checks when working against one.
