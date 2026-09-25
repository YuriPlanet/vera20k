# Shell message boxes over the empty backdrop, native evidence

Retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone), Ghidra as a navigation aid (names and
comments saved there).

## Native

- `ShellMessageBox__Run` `0x005D3490` draws and presents the empty backdrop
  before it creates the box (`0x005D3514`: `Shell__DrawEmptyBackdrop(NULL,
  present)` `0x0052FEC0`), then runs the box and tears it down
  (`0x00622720`). The dialog that asked is covered while the box is up.
- `Shell__DrawEmptyBackdrop` fills the hidden surface, calls
  `Shell__DrawRightPanelAndBackground` `0x0072E820` with 0 and blits.
  - `RightPanel__Draw(0)` `0x0072E450`: SDTP frame 0, the SDBTNBKGD tiles,
    SDBTNANM frame 10 over every tile row (all slots shuttered), the bottom
    cap and the lower strip.
  - `Background_Overlay` with the generic shell background: the SHELL.PAL
    convert `[0xB0FBCC]`, MNSCRNS `[0xB0FB50]` and MNSCRNL `[0xB0FA04]`
    (loaded at `0x0072AB49..0x0072AB7E`), never the dialog's own art.
- Front-end callers include Skirmish's start checks
  (`SkirmishDialog__OnCommand`, four sites) and Choose Map's eject question
  (`ChooseMapDialog__UseMap` `0x005E7336`); the main menu's exit confirmation
  runs after `0xE2` is destroyed and shows the same backdrop.

## VERA20k

`push_message_box_backdrop_instances` (skirmish shell renderer) draws the
generic background, the right panel with every slot shuttered and the lower
strip; while `0x102`'s validation box or `0x6B`'s eject box is up the renderer
draws only that backdrop and the box (no dialog controls, text, preview or
status line).

## Comparison

`skirmish-0x6b-eject-box` (Use Map on the first map with three AI rows)
against the retail exit-confirm still (the same two-button box type):
every differing pixel lies inside the box rectangle (175..628 × 137..465);
the backdrop matches exactly. Inside the box the message text and the
`Ok`/`OK` caption differ by string, and the box art sits one pixel off
vertically — the same offset as VERA20k's own main-menu exit confirmation
(`main-menu-0xe2-exit-confirm`), so it predates this change.

## Residuals

- The box art's one-pixel vertical offset (both box painters).
- The random-map dialogs' saved-seed prompts (`0x005F*` callers) still draw
  over their browser; they belong to the random-map chain.
