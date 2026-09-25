# Skirmish score screen `0x108`, native evidence

Bounded evidence for the score dialog a skirmish win or loss shows before the
shell resumes. Retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone), the static timers and the slide
schedule executed under Unicorn, Ghidra as a navigation aid. The Ghidra
database carries the names and comments cited here.

## Route

- `GameExit__Victory` `0x00685670` / `GameExit__Defeat` `0x00685DC0` restore
  the shell video pair (`0x006857AE`: only when both width and height differ),
  count the game (`[0xA8D57C]`) and run `ScoreDialog__Run` `0x005C9720` in
  GameMode 5. An aborted game gets no score (`0x00686570`).
- `ScoreDialog__Run`: `ScoreArt__Load` `0x0072D730` for the local side, the
  right panel and the art presented, the modal family dialog `0x108` (proc
  `ScoreDialog__WndProc` `0x005C9B10`), then the panel and the backdrop again
  (`0x005C9786`) and `ScoreArt__Release` `0x0072D780`.
- Continue `0x6D1` writes result 1 (`0x005CA06C`); `0x00622720` slides the
  dialog out. No key closes it (`IsDialogMessage` sends IDOK/IDCANCEL, which
  the proc ignores). Main_Game then re-enters PrepareSession, which builds a
  new `0x102` (merged earlier: `2026-09-25-post-game-skirmish-evidence.md`).

## Page

- **Slides:** tuple (0, 1, 0, 0), in and out (executed `0x006071E0`, 66
  cases in `tools/storage_oracle/shell_slide_engine.json`). During the entry
  slide the dialog paint (panel, art, shaded rectangle), the bars and the
  monitor show and the kind-1 texts wait for the slide's end (retail still
  `g2-s2.png`, taken mid-slide; the same holds for `0x6B` and `0x105`). The
  teardown slide repaints the dialog (`0x00622C4F`, `0x00621E90`) and runs
  before the proc paints its rectangle, so it shows the art and the column.
- **Art** (`ScoreArt__LoadShape` `0x0072D830`, tables `0x00844CA0..0x00844CB4`,
  `0x00844BD4..0x00844BDC`): side 0 MPASCRNL/MPASCRNS through MPASCRN.PAL,
  side 1 MPSSCRNL/MPSSCRNS through MPSSCRN.PAL, any other side MPYSCRNL (at
  640 wide MPSSCRNS) through MPYSCRN.PAL; drawn at the dialog origin
  `[0xB0FC1C]`. The side is Scenario `+0x34B8`, the side of session player
  0's country (`0x0068779A`).
- **Bars** (`ScoreDialog__BandImage` `0x005CA110`, tables `0x00844B24`,
  `0x00844B4C`, `0x00844B74`): the Game/Time band and the header band always
  (`0x005C9D82`), player band `i` only below the row count (`0x005C9E67`).
  Opaque 440x36 PCX, magenta-keyed like every kind-2 static; other sides
  get none.
- **Shaded rectangle** (`0x005CA07F..0x005CA0FB`): `AlphaBlendRect`
  `0x00621B80` with colour 0 and weight `0x9F`, i.e. `floor(3c/8)` per RGB565
  channel, from Game's top-left to the last row's Score cell grown by (4, 8).
- **Texts:** heading `0x694` (`GUI:SkirmishScore` in GameMode 5,
  `0x005C9BEC`), status line `0x695` (303x14 DLU), Game `TXT_GAME`, Time
  `TXT_TIME_FORMAT_HOURS` clamped at 99:59:59, the five headers and, per row,
  the name and four `%d` numbers. Every one is a kind-1 static; the executed
  getters (`tools/storage_oracle/shell_static_timers.py`, 50 cases for
  `0x108`) give 30 ms/1/32 for Game, Time, the headers and the names,
  60 ms/1/64 for the numbers. They all start at the entry slide's end and
  print through the shell print `0x00621040` → `0x00434CD0`; the ScoreFont
  objects (`0x00690A10`, `0x00690AE0`) serve the campaign score presentation
  (`0x0068D3DE`, `0x0068D3FF`), not `0x108`.
- **Row colour** (`0x005C9E0C`): `HSV_To_RGB` of the house colour scheme's base
  HSV, sent with `0x498` to the row's five cells (stored as the COLORREF,
  `0x00615E8E`). The shell print truncates it to R5G6B5 units (`0x00621040`)
  and the reveal blend shifts the units back to bytes with the low bits clear
  before mixing in the white highlight (`0x00434DED..0x00434E2E`). For the
  shell's yellow this changes no unit; for DarkBlue (34, 105, 212) it decides
  the last letter's red unit while its trail fades.
- **Names:** the house UI name. Skirmish setup gives a human house its handle
  (`0x00688094`) and every computer house `TXT_COMPUTER` (`0x00688263`).
- **Order:** `qsort` with `0x005C9AE0` (`0x005C9D7A`), which for up to eight
  rows is MSVC's `shortsort`: ties land in a fixed permutation (executed,
  12 cases).
- **Score:** only a positive score enters the entry (`0x005C99EB`); a
  survivor gets `v + v/2 + Random(v/2, v)`.
- **Status help:** only Continue has help (`STT:MPScoreButtonContinue`). The
  proc runs the common handler `0x00622B50` first; its `WM_NCHITTEST` case
  (`0x00622CCB..0x00622E83`) finds the child under the pointer with
  `ChildWindowFromPointEx` (or the dialog itself), looks its help up
  (`0x4E8`, `0x4E9`, the table `0x006040B0`) and always writes the result to
  `0x695`. The bands sit above the texts in Z-order and cover the table, so
  every point but Continue writes an empty text, the background included.
  (Choose Map's proc `0x005E6920` never calls the common handler, which is
  why its status line keeps the last text.)

## Retail comparison

Retail helper run at 800x600, retail RA2MD.INI (Americans, DC Uprising, one
Easy AI, Short Game): deploy the MCV, sell the Construction Yard, defeat.
Stills in the goal's `native/`: `sco-after-sell-1.png` (0.3 s after the entry
slide), `sco-after-sell-6.png` (settled), `sco-hover-cont.png` (pointer on
Continue), `sco-continue.png` (after Continue), `sco-cont-steady.png` (the
new `0x102`).

VERA20k captures (release build, 800x600, `--shell-capture`) against them,
in RGB565 units with `tools/shell_capture_diff.py`, masking only the software
pointer VERA composites and the monitor's animated screen:

- `score-0x108-steady` vs `sco-after-sell-6.png`: 474117 pixels compared,
  **0 differ** (art, shaded rectangle, bars, every text and colour, heading,
  status line, panel, Continue).
- `score-0x108-hover-continue` vs `sco-hover-cont.png`: 474117 compared,
  **0 differ** (the Continue help on the status line).
- Before the blend fix the steady page differed in 26 pixels by one unit: the
  last letter of "[New Player]", whose trail blended from the unquantized
  COLORREF.
- A second retail defeat caught the entry slide (`g2-s2.png`, rapid
  screenshots after the sale): `score-0x108-entry-tick-15` and `-17` match it
  with **0 differing pixels** (art, shaded rectangle, bars, empty heading,
  the column's last ticks). Ticks 3–13 differ only in the column (the slide
  is further in). This corrected an earlier reading that the bars appear only
  after the slide.
- `score-0x108-slide-out-tick-9` shows the teardown with the art and the
  column; there is no retail still of it.
- `score-0x108-leave-continue` (pointer from Continue onto the bare art)
  vs `sco-hover-bg.png` (empty status line): **0 differ**.
- `sco-after-sell-1.png`, 0.3 s after the entry slide, shows the texts
  mid-reveal with white trails, the bars and the monitor.

Production route (release build, retail RA2MD.INI: Americans vs one Easy AI
on DC Uprising, Short Game): `skirmish-defeat-score` starts the game from
`0x102`, deploys the MCV and sells the Construction Yard through the
production commands; the exit cascade opens the score page with Game 1,
00:00:04, the Allied art, "Computer" (DarkRed) above "[New Player]"
(DarkBlue) — the retail names, colours, order and side. The statistics differ
because the games differ (4 s against 79 s; the retail AI built three
things); they are not compared. `skirmish-defeat-continue` presses Continue:
the page slides out and the new `0x102` settles with the retained settings;
against `sco-cont-steady.png` it differs only in the map preview (the known
preview-scaling residual) outside the rows Wine mispaints.

Rust regression tests: `ui::score_shell` (executed relayout rects at three
sizes, shaded rectangle, hover help, kind-1 parameters against the timer
oracle, shortsort against the 12 executed cases, page texts),
`render::main_menu_shell_chrome` (37.5 % shading against the still, art
names), `render::shell_text_reveal` (the 16-bit blend against the still),
`app::match_runtime::sim_tick` (row names; row colours through the retail
`[Colors]` reader and `HSV_To_RGB` against the still), `sim::score` (negative
score), `ui::shell::slide` (66 executed slide cases).

## Residuals

- **After Continue:** retail presents the panel and the plain backdrop
  (`0x005C9786`) and holds it while PrepareSession reads the settings and
  builds the new `0x102` (0.3 s to 1 s on the helper). VERA20k builds `0x102`
  in the same frame and starts its entry slide at once.
- **640x480:** the table overlaps the right panel there; VERA20k shades only
  the art under the rectangle, not the panel part.
- **Video mode:** retail restores the shell size only when both the width and
  the height differ (`0x006857AE`); VERA20k restores it whenever the size
  differs (only game sizes sharing one dimension with the shell's differ).
- **More than eight rows:** retail's `qsort` partitions above eight elements;
  a skirmish has at most eight contenders, so only `shortsort` is ported.
- **Observer:** the observer house gets no row (`[0xAC1198]`); VERA20k's
  skirmish launch has no observer.
- **Saved games:** a save restored into a running match keeps that match's
  handle and colours; retail restores the houses' saved UI names.
- **Statistics:** the Losses of a sold-out defeat were not compared (the
  games differ); the stats sources are established by instruction reading.
- **Entry tick 0:** `ScoreDialog__Run` presents the panel and the art before
  the dialog exists (`0x005C9761`), without the shaded rectangle; VERA20k's
  first entry frame already has the dialog paint.
- The monitor's animated screen is not compared (its frame depends on
  timing).
