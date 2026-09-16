# Slice 5b (in-game Options + modal pump) kickoff stub — deferred from Slice 5

GOAL: Implement the in-game Options dialog and the sim-behind-modal pump contract — the heavy
half deliberately split out of Slice 5. The in-game Options menu is a **FULL shell dialog**
(533×369 DLU, standard Win32 trackbars/checkboxes/buttons), NOT a message-box modal, so it uses
the skirmish `0x102` owner-draw control family, NOT `paint_modal_shp`. Package `vera20k`. Parity
bar: indistinguishable from gamemd.exe.

PREREQUISITE: Slice 5 (menu modals — `ModalKind` table, `paint_modal_shp`, quit-confirm, validation
modal) must be landed first; this slice consumes the `ModalKind` enum (incl. `InGameOptions`) and the
`DialogController` modal wiring from it.

SOURCE OF TRUTH — READ FIRST:
- `docs/plans/2026-06-01-shell-substrate-slice5-plan.md` — execute its **sub-steps 3, 5a, 5b** (the rest
  were done in Slice 5). All facts below are PROOFED and cited there.
- [docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHELL_DIALOG_FRAMEWORK_SUBSTRATE_SERVICE.md) — substrate contract C2 (pump/lifecycle).

SCOPE — sub-steps to execute (re-anchor by CONTENT; line numbers drift):
3. **Pure pump decision + `service_tick` + `SessionMode`** (`src/app_sim_tick.rs`, `src/app.rs`).
   `modal_pump_should_advance_sim(session_mode, reentrancy)` (pure, unit-tested): advance iff game mode ∈
   {3 LAN, 4 WOL}; freeze iff ∈ {0 campaign, 5 skirmish}. `service_tick`: always net + input + repaint;
   advance via the EXISTING `advance_fixed_simulation` iff the pure decision is true. `SessionMode` read ONLY
   by the app loop, never by `sim/`; `World::advance_tick` signature unchanged. Returns offline for current
   play, so the network branch is dead code this build (unit-testable via the pure seam + a headless `World`).
5a. **In-game Options full-dialog chrome behind the existing paused freeze.** Render pixel-faithfully from the
   parsed control tables — **0xBBB** (active game) and the SEPARATELY-PARSED **0xF5** (shell; different/wider
   slider rects, +Difficulty slider +ScrollCoasting) — via the skirmish-shell owner-draw control family. Select
   0xBBB vs 0xF5 by the PROOFED `g_GameActive == 1` gate. INI-write-on-OK: persist on **result == 1** (every
   close button — OK/Sound/Back — yields 1; only the game ending yields 2 = no persist; there is NO
   cancel-without-save path) → Apply then write `RA2MD.INI`. Resolve the two bounded items inside this sub-step:
   DLU→pixel projection at 800×600 / 1024×768, and confirm which SHP/PAL the controls use (assumed same as
   `0x102`, not yet confirmed identical) — see plan §9.
5b. **`service_tick` swap + C2 assertion.** Replace the paused-only freeze with `service_tick` (offline {5,0}:
   sim frozen AND battlefield frozen-as-last-blit; network {3,4}: advances — dead branch). Add the acceptance
   assertion: offline `World.tick` delta == 0 over N pumped frames with no battlefield recomposite; network
   delta == N (unit-only, no live caller).

CADENCE: same hard rules as Slice 5 — re-anchor by content; ONE sub-step per cycle; separate `cargo build`/
`test` pass (read the literal `test result:` line); `src/ui/skirmish_shell/state/tests.rs` (87 tests) stays
GREEN and UNCHANGED; `sim/` never depends on `ui/`/`render/`; STOP for in-game OK before committing each
sub-step to `dev`.

NOTE: this is the heaviest slice (a second full options dialog). Seriously consider sub-stepping 5a further
(e.g. layout/descriptor first, then owner-draw paint, then INI-write wiring) rather than one change.
