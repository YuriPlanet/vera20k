# Movies & Credits: native evidence

Bounded evidence packet for the main-menu Movies & Credits family: dialog
`0x101`, movie list `0x129`, the shell Play_Movie route and Show_Credits. It
records what the original does and how it was observed; Rust validation is in
the change that cites it.

## Identity

Retail `gamemd.exe`, SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`. Control
flow and constants were read from instructions (Capstone over the PE mapping,
Ghidra as a navigation aid). `CRCEngine` @ `0x004A1DE0` was executed under the
project Unicorn runner (`tools/native_oracle.py`) to obtain the `Network`
section CRC `0x70CAA741` (the same run reproduces the existing `RandomMap`
constant `0x1597B573`).

## Route (Main__PrepareSession `0x0052D9A0`, jump table `0x0052EB58`)

| State | Native action | Returns to |
|---|---|---|
| 4 | `ShellDialog__RunUntilResult` (`0x0060D380`) with dialog `0x101`, proc `0x0052D790` | per command |
| `0xD` | Play_Movie `0x005BED40`(`"RENEGADE.BIK"`, -1, 1, 1, 1, 0), then `0x0052FEC0(0,1)` | 4 |
| `0xE` | dialog `0x129`, proc `0x0052D870`; a returned entry is CD-checked (`0x004790E0`), played, then `0x0052FEC0(0,1)` (`0x0052DEBB..0x0052DEBF`) | `0xE` (list), -1/0 → 4 |
| `0xF` | Show_Credits `0x004C3E30`, Queue_Song(INTRO), `0x0052FEC0(0,1)` | 4 |

`0x0052D790` maps (index bytes `0x0052D85C`, targets `0x0052D848`): `0x68D`
Sneak Peeks → `0xD`, `0x68E` Play Movies → `0xE`, `0x68F` View Credits → `0xF`,
`0x686` Main Menu → `0x12`; `0x687..0x68C` write nothing. WM_PAINT forwards
`0x4F0` to the RA2TS static `0x71A`.

`0x0052D870`: on `0x497` it fills list `0x744` through OptionsClass
`0x005FC000` and sends `LB_SETCURSEL(DAT_00825C80)`; the list subclass handles
that message itself (`0x0061A534..0x0061A5F5`) and always returns 0, so the
`LB_SETCURSEL(0)` fallback never runs and a fresh process opens with no
selection. Play Movie `0x745` stores `LB_GETCURSEL` in `DAT_00825C80` (initial
`0xFFFFFFFF`) and returns the row's item data; Back `0x686` returns -1. The
`0x744` notification-code-0 branch has no sender (the subclass only sends codes
1 and 2), so a double-click never plays. The ListBox class takes double-clicks:
the second press arrives as `WM_LBUTTONDBLCLK`, which the subclass only posts
to the parent as `LBN_DBLCLK` (`0x0061A904..0x0061A945`), so it neither
selects nor clicks.

## Movie table and unlock progress

`0x00832C20` intro `{A00_F00E, Name:IntroMovie, cd -1}`; Allied
`0x00832C30` and Soviet `0x00832CA0`, eight `{A0n_F00e | S0n_F00e,
Name:All0nMD | Sov0nMD | *FinalMovie, cd 2}` rows each. `0x005FC000` lists the
intro, Soviet rows `0..=+0x4C`, then Allied rows `0..=+0x50`.

OptionsClass `+0x4C` (Soviet) / `+0x50` (Allied) default to -1
(`0x005FA3C7`). `ReadFromINI` `0x005FABA6..0x005FAC54` decodes RA2MD.INI
`[Network] NetID` with `INIClass__ReadCommaHexUTF16` (200 units), NOTs every
unit (a resulting 0 becomes `0xFFFF`), reads two `wcstok(L" ")` tokens with
`wcstol` minus one, and resets values outside -1..7 to -1. `WriteToINI`
`0x005FAF9E` writes `swprintf(L"%d %d", soviet+1, allied+1)` NOT'd through
`INIClass__PutUTF16AsHexCSV`. The retail prefix's file holds
`NetID=ffcf,ffdf,ffcf,` ("0 0"); with it set to `ffc7,ffdf,ffc7,` ("8 8") the
retail list shows all 17 rows and the game writes the same value back on exit.
The section CRC is `CRCEngine` @ `0x004A1DE0`: CRC-32 over the name padded
the Westwood way (`Network\x03`), as the Unicorn run confirmed. Every Play_Movie name resolution
(`0x005C0640`) calls `0x005FBF80`, which raises each side to the `_stricmp`
(`0x007C8D20`) table index of the played stem.

## Play_Movie (shell arguments)

Name truncated at the first `.`, unlock progress raised, then `.BIK` (then
`.VQA`) availability; a missing file returns with no visible change. Session
mode `[0xA8B238]` must be 0. The mouse is hidden, the screen cleared, EVA,
stream players (the theme) and sound events are paused, not stopped. Bink plays
its own soundtrack: `BinkSetVolume(ftol(VoiceVolume × 32768.0))` right after
BinkOpen (`0x00432897`; 32768 is unity gain), re-applied per frame when
VoiceVolume changes (`0x00432E7A`). Frames are copied unscaled to
`((client − movie) / 2)` per axis, clamped at 0 (top-left at 640×480). Only a
bare Escape release (`Keyboard::Get() == 0x081B`) aborts; other keys and mouse
buttons are read and ignored. The inactive application pauses Bink. Afterwards
the screen is cleared again, paused audio resumes and the full-redraw flag is
set; `0x0052FEC0(0,1)` paints the empty shell backdrop before the next dialog.

## Show_Credits

CREDITSMD.TXT (16,180 bytes, `langmd.mix`, CRLF) is parsed once: CR +16 px and
column 0, LF ignored, space column+1, TAB to the next multiple of 8; other bytes
start a run ending at TAB/LF/CR or a second space. Runs ending at the final byte
are dropped; a trailing space before the terminator is trimmed and the
terminator reread. Column 4..7 centers, >8 right-aligns, else left; `{LABEL}`
resolves through the CSF (`{Missing Label}` / `{Bad Label}` keep the `}`).
Initial y is screen height + 2. Each frame (two 16 ms buckets, absolute
schedule) scans the lines from last to first, moving each up 2 px; the first
one found at y ≤ −21 is deleted and the scan stops, so the lines before it keep
their position for that frame (`0x004C43C7..0x004C443C`).
Lines are drawn in GAME.FNT RGB(255,255,128) in a 520 px box centered on the
screen; rows 0..31 and H−32..H−1 are faded `floor(v·f/256)` per RGB565 channel
with `f = 255 − min(256 − 8r, 255)`. Each line is drawn and clipped in the
rect `((W − 520) / 2, y, 520, …)` (`0x004C3D46..0x004C3D6B`); nothing is drawn
or copied while the application is inactive (`0x004C3D8A`, `0x004C46AA`). A
bare Escape press ends the roll; other
queued keys and mouse buttons end one frame wait early. Theme: Stop(0),
ScoreVolume 0 lifted to 0.4 while CREDITS plays, restored at the end. The
scheme-name global `0x00822724` is left at `"Blue"`. Its getter `0x004E12D0`
has 20 call sites, all outside the shell dialog and owner-draw code
(`0x00600000..0x00626000`): in-game gadget boxes and gadget `Draw` methods
(`0x004A5A50`, `0x004E2690`, `0x00557D20` via vtable `0x007ED1D4`),
multiplayer chat (`0x0055E420`), WOL (`0x0078A470..`, `0x007A93xx..`) and a
debug overlay; not every one was classified individually.

## Dialog composition

Both dialogs are in the full-screen allow-list (`0x0060C63A`, `0x0060C645`),
take the default MNSCRNS (640 wide) / MNSCRNL background through SHELL.PAL and
slide on first paint. `0x101` has the RA2TS static (started by
`ShellDialog__RunUntilResult`); `0x129` has none. `0x129`'s list `0x744` and
prompt `0x40C` keep raw DLU rectangles at every resolution (`0x0060C42E`).
Tooltips: `0x101` `STT:OptionsButtonSneak/Movies/Credits/Back`; `0x129` Play
Movie `GUI:PlayMovie`, Back `STT:MsnDltButtonBack`, list `GUI:SelectMovie`.
Escape and Enter reach the procs as IDCANCEL/IDOK through `IsDialogMessageA` and
are ignored there and in the common handler `0x00622B50`.

Owner-draw list `0x744` (paint `0x00619230`, measured in the capture below):
interior `(x+1, y+1, w−1, h−1)` is the parent image one RGB565 unit darker per
channel (translucency 0); two rings frame it — outer `x−1..x+w+1`, light
`0xC5BEA7` top/left and dark `0x807A68` bottom/right; inner `x..x+w` inverted;
the top-right and bottom-left ring corners use `0xA29C87`. Rows are 19 px, the
selected row is filled red, text is GAME.FNT yellow at row x+2. A press selects
row `client_y / 19 + top` (no border correction) and plays GenericClick.

## Movie list scrollbar

With more than 15 rows the list draws its scrollbar child (paint
`0x0061C690`). Retail captures with all 17 movies unlocked (`list17-*.png`)
show: the bar in columns `x + w − 20 .. x + w − 1` (the shared list geometry
over the window grown by one pixel, the paint surface); its left edge is a
light line at `bar.x` and a dark line at `bar.x + 1` between the list's top and
bottom rings, taking the corner color where a line crosses a ring of the other
tone; inside, the parent background without the list's darkening; 18x22
`UPARROWR`/`DNARROWR` at `bar.x + 2` against the rings; the grip `SBGRIPM`
tiled every 30 rows from the thumb top with the 2-row `SBGRIPT`/`SBGRIPB` caps
drawn over it; thumb height `thumb_height(305, 2) = 202`. One down-arrow press
scrolls one row. Down-arrow keys after a row press change neither the
selection nor the scroll position. The mouse wheel was not tested (the capture
helper cannot send it).

## Native captures

Retail YR under the Porting Kit Wine wrapper with cnc-ddraw 7.1 at 800×600,
driven by the local `yr-control` helper (the helper requires `WINEESYNC=1` and
`WINEMSYNC=1` to match the running wineserver). The helper recenters the
pointer at (400,300) after each click; captures do not include a cursor. SHA-256
prefixes of the PNGs (kept outside the repository):

| Capture | State | SHA-256 prefix |
|---|---|---|
| `mc-0x101.png` | 0x101 entry frame (MNSCRNL + slide, no movie yet) | `f2b83bca669cc19f` |
| `mc-0x101-settled.png` | 0x101 steady | `a3bdb74842a8bcf8` |
| `list-0x129.png` | 0x129 entry frame | `7245ed6ae07bee1d` |
| `list-0x129-settled.png` | 0x129 steady, pointer over list | `7ca6f58f247dbbd6` |
| `list-row-click.png` | row 0 selected | `c097ee65ba49f9bc` |
| `play-intro-6s.png` | intro movie, full screen | `0d28e85c8b0d1c5f` |
| `after-esc-2s.png` | list after Escape, row 0 selected | `b10e18dc96224ed5` |
| `sneak-8s.png` | RENEGADE.BIK | `7179d6328e963274` |
| `credits-10s.png` | credits roll | `1fdb66a4aae7cbd0` |
| `credits-esc.png`, `credits-esc-3s.png` | 0x101 recreated after Escape | `24a83ac1b0a35e92`, `240733fad1215c3e` |
| `list17-open.png` | 17 rows, top 0 (profile `8 8`) | `d124e4b99d008659` |
| `list17-down2.png` | 17 rows after two down-arrow presses | `2cd63115251be955` |
| `credits-timed/t1..t60.png` | credits every ~5 s, with file times | log `log.tsv` |

Credits cadence, executed: 60 retail screenshots taken every ~5 s through the
credits, timed by the screenshot files' modification times.
`tools/shell_scroll_rate.py` aligns consecutive frames: 38 pairs align
exactly, 11,124 px over 177.988 s, **62.499 px/s = one 2 px step per
32.001 ms**, every pair within 1.8 px of 2 px/32 ms. The file's last line
(y₀ = 12,778) sits at y ≈ 272 in the last scrolling shot, so the Rust schedule
(6,400 scroll frames, then no hold because the hold counter is already past
`0xBB`) ends the roll 204.7 s after the credits start; retail shows the
faded/black end at 204.3 s and `0x101` again by 208.7 s, bounding any hold
below 4 s.

Observed: only the intro row is listed in a fresh profile; the first list entry
shows no highlight until clicked, and after a movie the list reopens with the
played row highlighted; Escape during a movie returns to the list, during the
credits to `0x101`; `0x101`'s entry frame shows the MNSCRNL background under the
sliding buttons before the RA2TS movie appears.

## Comparison with VERA20k

Production frames come from `--shell-capture <checkpoint> --width 800 --height
600 --cursor-x 400 --cursor-y 300` (release build of the change that adds this
note), which drives the production action handlers through the route; the
`0x129` row and scrollbar presses go through the mouse handlers. The full-list
checkpoints override the movie progress to `7 7` before opening the list. They are
compared with the native PNGs above by `tools/shell_capture_diff.py` in RGB565
units (both engines compose on a 16-bit surface; see the tool's docstring).
Masks: `cursor` 398,298,40,40 (native captures have no cursor), `monitor`
632,0,168,150 (the `0x71C` animation, whose phase is not synchronized),
`status` 0,570,300,30 and `ra2ts` 0,0,632,570 (the RA2TS movie phase).

| Checkpoint | Native | Masks | Differing / compared | Max units |
|---|---|---|---|---|
| `movie-list-0x129-selected` | `list-row-click.png` | cursor, monitor, status | 0 / 444,200 | 0 |
| `movie-list-0x129-steady` | `list-0x129-settled.png` | cursor, monitor, status | 0 / 444,200 | 0 |
| `movie-list-0x129-steady` | `list-0x129-settled.png` | cursor | 4,192 / 478,400, all in the monitor and status boxes | 63 |
| `movies-0x101-steady` | `mc-0x101-settled.png` | cursor, ra2ts | 4,584 / 119,760, all inside x 669..761, y 6..100 (title `0x694`, monitor `0x71C`) | 63 |
| `credits-roll-frame-434` | `credits-10s.png` | none | 0 / 480,000 | 0 |
| `sneak-peek-frame-158` | `sneak-8s.png` | none | 100,931 / 480,000, all ±1 | 1 |
| `movie-list-0x129-full` | `list17-open.png` | cursor, monitor, status | 0 / 444,200 | 0 |
| `movie-list-0x129-full-down2` | `list17-down2.png` | cursor, monitor, status | 0 / 444,200 | 0 |

Known open differences (next shell-statics change): the `0x71C` SDWRNANM monitor
animation is not drawn on these pages; the title `0x694` is drawn at the menu
page rectangle instead of native `(635, 9, 162, 17)` (`+7` y, `+1` h at
`0x0060BD17`) and without its kind-1 reveal; the status text `0x695` is 2 px
low and appears without its reveal. The Bink frame differs from binkw32's
YUV→RGB565 conversion by one unit on about a fifth of the pixels.

Coverage limits: one resolution (800×600), one steady frame per state, the
intro-only and all-17 lists, and the first credits page (centered lines; the
left/right-aligned cast section has no pixel comparison). Entry and exit
transitions were compared visually, not pixel-for-pixel. Not established: the
mouse wheel on the list, an 800-wide movie on a 640×480 surface
(`BinkCopyToBuffer@28` takes no destination width; Rust shows the top-left
crop), and the credits catch-up after a long inactive period.
