# After a Skirmish game: back to a new Skirmish page, native evidence

Retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone); Ghidra comments saved.

## Native

- A Skirmish game sets GameMode `[0xA8B238]` to 5 and it stays 5 through the
  game. Every end of the game (the score's Continue after a win or a loss,
  and an abort, which has no score) returns out of Main_Game to
  `Main__PrepareSession`.
- PrepareSession plays INTRO (`0x0052D9AF..0x0052D9BF`) and picks its start
  state (`0x0052DB09..0x0052DB24`): `0x12` (the main menu) only when GameMode
  is 0, otherwise `0x10 + (GameMode == 4)`.
- State `0x10` re-reads the multiplayer settings (`0x006980C0`) and, for
  GameMode 5, runs the Skirmish runner (`0x0052E168` -> `0x006AE2C0`): a new
  `0x102` that slides in with the settings written when the game started.
- Back on that page sets GameMode 0 and state 1, the Single Player page
  (`0x0052E175..0x0052E17B`).
- Retail still `native/sc-score-b.png` (after an in-game Quit, 800x600): the
  new `0x102` in its entry slide.

## VERA20k

- Starting a Skirmish game sets the shell route to Skirmish (returning to
  Single Player), the equivalent of GameMode 5.
- `App::resume_shell_after_match` runs after the abort exit
  (`return_to_main_menu`) and after the score's Continue
  (`leave_mission_result_screen`): with the Skirmish route it opens a new
  `0x102` exactly as Single Player's Skirmish does
  (`enter_native_skirmish_from_single_player`), whose entry slide then runs.
  The shell music is the INTRO the shell maintains.
- The developer overlay's return keeps going to the main menu.

## Validation

- Capture checkpoint `skirmish-after-quit` (release, 800x600): Main Menu →
  Single Player → Skirmish → Start Game → 30 game frames → Leave through the
  in-game abort action (`app::input::abort::activate`) → the EXIT event →
  the abort exit → the new `0x102`, captured once its entry slide and reveals
  settled. It shows the Skirmish page with the game's settings kept.

## Residuals

- The score screen `0x108` itself is not a family page yet: no slide-out
  before the new `0x102`, and its art, bars and reveals differ (the score
  chain).
