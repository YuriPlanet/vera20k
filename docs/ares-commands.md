# Ares keyboard commands

Ares 3.0 registers its keyboard commands where gamemd allocates UnitTab's
(CommandClassCallback_Register, hook `0x533058`, Ares.dll `10002A80`,
`Ares.Commands.cpp`), in this order: AIControl (only with `-AI-CONTROL`),
MapSnapshot, DumpTypes, DumpMemory, DumpAIBasePlan, FPSCounter and
TogglePower. VERA registers them at the same place in its command list
(`app::input::hotkeys::catalog`) and lists them on the keyboard page while
Ares plays. Their names, categories and descriptions come from the CSF
(`TXT_TOGGLE_POWER`, `TXT_INTERFACE`, `TXT_TOGGLE_POWER_DESC`, ...). None has
a default key.

## FPSCounter and the release note

The FPSCounter command (`src/Commands/FPSCounter.h`, Ares.dll `100022F0`;
texts `TXT_FPS_COUNTER`, `TXT_DEVELOPMENT`, `TXT_FPS_COUNTER_DESC`) shows
or hides a frame counter. It starts hidden and stays as set from one match
to the next.

TacticalClass_Draw_TheDarkSideOfTheMoon (Ares.dll `1006BE80`, hook
`0x006D4B25`, `Misc/Surfaces.cpp`) prints lines at the bottom left of the
battlefield, after the timer lines and before the outcome banner, from the
bottom up (`app::presentation::ares_tactical_text`):

- the string table's `TXT_RELEASE_NOTE`, in red, when it has text. Ares'
  own `ares.csf` holds it empty, so a mod sets it in a
  `stringtable00.csf` to `stringtable99.csf`;
- while the counter is on, `FPS: <rate> Avg: <average>` in white: the
  frames the game counted in the last second and the average since the
  count began (gamemd `0x0055E33B`).

Each line sits on a black box its own size at the battlefield's left edge.
The stability warning Ares 3.0 could print first is never set
(`Ares::GetStabilityWarning`, `10003650`, returns nothing).

## TogglePower

```ini
[General]
TogglePowerAllowed=yes   ; rulesmd.ini: lets the command arm power mode
```

The command (`src/Commands/TogglePower.h`) calls gamemd's
`SetTogglePowerMode(-1)` (`0x004AC820`) when `TogglePowerAllowed=` is set.
That turns on power mode, the Display byte YR never arms
(`app::input::power_mode`, the byte in `ui::sidebar::gadget_flash`):

- Entry needs one of the player's buildings and no building placement, and
  clears the selection. Repair, Sell and beacon mode turn it off, and it
  turns them off. Right-click (Cancel `0x004AAD30`), the team keys and
  Next/Previous Object turn it off; so do placing a building, a superweapon
  button and a trigger's input lock.
- With nothing selected, the cursor over one of the player's buildings that
  may be selected, is no 1x1 undeployer, has `TogglePower=` and drains power
  or is `Powered=` shows TogglePower, elsewhere NoTogglePower
  (DetermineAction `0x00692815`, Ares DisplayClass_ChooseAction_TogglePower
  `1005C790`, hook `0x0069281E`, which also refuses a building warping out
  or under EMP). Both cursors are `[MouseCursors]` entries.
- A click on such a building switches it off when it is on and on when it
  is off (BandBox_LeftUp `0x004ABFB3`, Ares
  DisplayClass_LeftMouseButtonUp_ExecPowerToggle `10057ED0`, hook
  `0x004ABFBE`), through the existing On/Off Line event
  (`Command::SetBuildingOnLine`). No band box starts in the mode
  (`0x004AC31A`).

A new scenario starts with power mode off, as with Repair and Sell
(Display Init_Clear `0x004A88C0`). A save made in power mode loads with the
mode off (`sim::snapshot::saved_local_view`).

## Not applicable

- AIControl hands the player's House to the computer. Ares registers it only
  when started with `-AI-CONTROL`, a switch VERA's launcher has no
  counterpart for ([ares-skipped-hooks.md](ares-skipped-hooks.md)).
- DumpTypes and DumpAIBasePlan write the type lists and the computer
  players' base plans to Ares' debug log; DumpMemory writes a Windows
  minidump. VERA has neither the log nor the dump.

MapSnapshot is open in [ares-remaining.md](ares-remaining.md).
