# Skirmish Start Game → loading screen → game, native evidence

Bounded evidence for what the player sees between Start Game on Skirmish
`0x102` and the first game frame, for a selected (not random) map. Retail
`gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone), Ghidra as a navigation aid. The
Ghidra database carries the comments cited here.

## Timeline

1. Start (`0x006ACEE0` case `0x617`) packs the session; the runner slides
   `0x102` out (`ShellDialog__Teardown`).
2. `Main__PrepareSession` first clears the keyboard (`0x0052E614`), seeds the
   random number system (`Init_Random_Number_System` `0x0052FC20`, called at
   `0x0052E619`; VERA20k's owner is the match seed clock at Start) and resets
   a list (`0x0052E61E`), then (`0x0052E64C..0x0052E69E`): Hide_Mouse
   (`[0x887640]` vtable `+0x0C`), the palette fade (a no-op at 16 bits,
   `0x004A3C30`), a black fill of the hidden surface (`[0x88730C]` vtable
   `+0x18`) and its blit (`0x004F4780`), then Show_Mouse.
3. `ScenarioClass__Start_Scenario` `0x00683AB0` hides the cursor again
   (`0x00683C11`), starts the LOADING theme and reads the scenario. Nothing
   reaches the screen until the loading screen's first repaint at 3%, so the
   screen stays black with no cursor while the map is read.
4. The loading screen repaints at each milestone. Read_Scenario emits
   45 → 50 (`0x00687847`) → Read_INI_Basic 55/58/60 (`0x0068ACA0`,
   `0x0068AD34`, `0x0068AD53`) → Read_Map_Section 63/65/67/68/69
   (`0x004AD011`, `0x004AD0AF`, `0x004AD339`, `0x004AD716`, `0x004AD74F`) →
   70 (`0x00687A28`); the gate drops any value below the current one.
5. After 100% the game mode is set (`0x00683DF3`) and the hidden surface is
   filled black and blitted (`0x00683E07..0x00683E1C`); Main_Game repeats the
   black at `0x0052EAA1..0x0052EAEB`, then the game loop starts.
6. The window keeps its title: `CreateMainWindow` `0x00777C30` names it
   "Yuri's Revenge" at `0x00777CC5`, and gamemd imports no SetWindowText.

## VERA20k

- Every native loading session (selected maps, random maps, the quick-play
  and tactical-capture launches) opens with a black frame (`next_frame`
  starts at `Blank`): the frame after the shell closes clears the shell
  surface to black with no cursor; the scenario is read on the next frame,
  under that black, and the loading screen then shows at 3% (1% for random
  maps).
- `apply_map_load_result` presents a black frame at the game size right
  after the window switches, before the tactical install.
- `init.rs` emits 63/65/67 in native order; before, 67 came before 50 and
  the gate dropped 50–65 (a 45 → 67 jump on the bar).
- The window title no longer changes to the map name.

## Validation

- Capture checkpoints `skirmish-start-blank` and
  `skirmish-loading-first-frame` drive Start Game through the production
  action (`App::start_game_from_shell`). The blank frame is 480,000 black
  pixels; the first frame is the loading screen at 3%. The capture of the
  first frame is followed by the whole map load in the release binary
  (`Match surface 800x600` in the log).
- `milestone_order_tests` is a source-order guard: the loader's literal
  milestones, in source order, never fall back.

## Residuals

- Random maps: VERA20k generates the whole map while preparing the first
  frame, so the black frame stays up for the whole generation and the bar
  then jumps 1% → 49% → 100%. Retail draws the loading screen inside
  Generate (`0x00684989` → `0x00598A74` → Full_Init `0x00599A56`, 3% at
  `0x00687594`) and reports the generation as 50–99% (`0x00599B68..0x0059A60A`,
  `0x00598B30..0x00599490`, gated on Scenario `+0x3598` set at `0x00684675`).
  Random-map chain; before this change the closed shell stayed frozen there.
- No retail loading-screen still exists: the helper's PrintScreen hook runs
  on the game thread, which pumps no messages during the synchronous load.
  The loading screen's own composition keeps its earlier Rust fixtures.
- Milestone timing: VERA20k emits some milestones in bursts at its own phase
  boundaries (12..25, 72..78, 82..98), each with a full present; native
  spaces them by its phases. Order and values match.
- Cooperative scenarios use the coop campaign's load art in retail
  (`0x00553321..0x005533C0`); Skirmish lists no Cooperative mode.
- A missing LOADMD/LOAD.MIX makes retail skip the loading art and carry on;
  VERA20k shows "Loading Failed".
- The window's startup title stays "RA2 Engine" (retail: "Yuri's Revenge").
