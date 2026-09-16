# Slice 4 kickoff prompt (paste into a fresh session)

GOAL: Continue the front-end shell-dialog substrate migration — fold the three shells
(main_menu 0xE2, single_player 0x100, skirmish 0x102) onto one shared `ui::shell` substrate,
pixel/behavior INDISTINGUISHABLE from gamemd.exe. Package is `vera20k`. This is Slice 4 of 6.

ALREADY DONE (committed on `dev` — confirm with `git log --grep="substrate Slice" --oneline`):
- Slice 0: shared geom `src/ui/shell/geom.rs` (DLU->px, right-panel chrome, button snap).
- Slice 1: descriptor + layout pass `src/ui/shell/{descriptor,layout}.rs`; main menu migrated.
- Slice 2: DialogController input authority `src/ui/shell/controller.rs` (hit-test/press-match/
  hover/disabled for 0xE2+0x100; per-shell structs mirrored for render).
- Slice 3: descriptor-driven paint pass `src/render/shell_paint.rs` (both shell emitters
  collapsed; byte-identical pixels; free functions + ButtonPolicy + ArtFit — the
  OwnerDrawControl trait was deliberately deferred to Slice 4).

READ FIRST:
- Study (verified): [docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) (contracts
  C1-C14, §7 retire list, §8 slices).
- Main plan: `docs/plans/2026-05-31-shell-substrate-plan.md` — Slice 4 outline (~L797-805).
- Design: `docs/plans/2026-05-31-shell-substrate-design.md` (§5).
- Proven recipe (prior slice plans): `docs/plans/2026-06-01-shell-substrate-slice2-plan.md`,
  `...-slice3-plan.md`.
- Substrate modules: `src/ui/shell/{geom,descriptor,layout,controller}.rs`,
  `src/render/shell_paint.rs`.

SLICE 4 SCOPE:
- Fold skirmish (0x102) controls onto the substrate: combo / trackbar / checkbox / listbox
  become `ControlKind`s.
- Unify the two skirmish scroll models: combo dropdown (`skirmish_shell/state/combos.rs`) vs
  choose-map listbox (`skirmish_shell/layout.rs`).
- Likely where the deferred OwnerDrawControl trait + per-ControlKind paint dispatch lands
  (skirmish's heterogeneous controls are the real dispatch point a trait justifies).
- Contract: per-control behavior + C14 defaults seed.
- Acceptance: combo open/scroll/select, trackbar drag value, checkbox icon-vs-label hit,
  choose-map listbox scroll ALL identical (existing skirmish tests green); and
  `[MultiplayerDialogSettings]` seeds every control byte-exact (TechLevel 10, GameSpeed 1,
  FogOfWar off, ...).

PRE-REQ / RISK (do at slice start):
- BIGGEST BLAST RADIUS of all slices. The `skirmish_shell/state/tests.rs` suite is the safety
  net — it MUST stay green, unchanged.
- Re-verify ALL skirmish state + render line ranges by CONTENT (plan numbers drift; parallel
  sessions edit `src/ui/*` and `src/app_*`).
- Skirmish is a far larger surface than the button-only shells. Plan carefully; SERIOUSLY
  consider sub-stepping Slice 4 (e.g. one ControlKind family per cycle) rather than one
  monster change.

APPROACH (the recipe used for Slices 2-3; ultracode on):
1. Planning workflow: verify current skirmish code + contracts -> draft -> adversarial
   review -> judge. Save the plan to `docs/plans/2026-..-shell-substrate-slice4-plan.md`
   with required fixes baked in.
2. Implement per the saved plan.
3. Separate verification pass: `cargo build -p vera20k` (NOT buried inside a workflow).
4. Adversarial verify workflow (byte-identical behavior + pixel diff vs `git show HEAD:`).
5. STOP, hand back for in-game verification. Do NOT commit until I OK it in-game.

CADENCE — HARD RULES (do not break):
- ONE slice per cycle. Never chain multiple slices unattended.
- Use a workflow to plan->review->implement a single slice if helpful.
- Verify = `cargo build -p vera20k` (build; the skirmish test suite must also stay green here).
- Build clean is necessary, NOT sufficient — after the slice STOP and ask me to look in-game
  before anything else.
- Commit each slice separately, after build passes AND my in-game OK.
- Parallel human sessions edit `src/ui/*` and `src/app_*` concurrently: if the build fails in
  files you did NOT touch, suspect a parallel session — don't fix unrelated code; re-anchor on
  function names, not line numbers.

START: Plan Slice 4 now — run the planning workflow against the study + plan + current
skirmish code, save the plan, and show me the plan + a recommended sub-step breakdown +
any open questions BEFORE implementing. Do not implement until I approve the plan.
