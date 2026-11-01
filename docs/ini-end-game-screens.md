# End-game screens

`[EndGameScreens]` lets a rules set say what a match looks like when it ends.
There are two places a match ends, and they used to share one picture and one
theme; they are now separate halves with separate keys:

* **the verdict** — the card that rises over the battlefield where the match was
  decided, in the slot the retail `GRFXTXT` banner occupies
  (`Tactical 6D4B25 -> 6D4E20`), with the theme that starts with it;
* **the score page** — the background and the theme of dialog `0x108`.

This is a product extension: gamemd reads no `[EndGameScreens]` at all. The only
native paths it feeds are the ones it projects onto — the banner slot above and
the score dialog's own background (`0x0072D730`) and theme literal (`0x0082FEBC`
`SCORE`).

## Registry

```ini
[EndGameScreens]
0=AlliedEnd
1=SovietEnd
```

`N=Section` registers `[Section]` as an entry. The entry that applies is the
first one, in registration order, that names the **local player's** country and
says something about the outcome — so write narrow entries first and a catch-all
last. A section with no `Country=` covers every country; `*` or `All` means the
same. Country names are the keys of `[Countries]`, case-insensitive.

## One entry

```ini
[AlliedEnd]
Country=Americans,British,French,Germans,Alliance

; the verdict: the RA3 Allied card, whole and unclipped
VictoryArt=ENDAWIN.SHP
VictoryPalette=ENDAWIN.PAL
VictoryMusic=Power
DefeatArt=ENDALOS.SHP
DefeatPalette=ENDALOS.PAL
DefeatMusic=InDeep

; the score page: Allied artwork behind the score columns
ScoreVictoryArt=SCAWIN.SHP
ScoreVictoryPalette=SCAWIN.PAL
ScoreVictoryMusic=SCORE
ScoreDefeatArt=SCALOS.SHP
ScoreDefeatPalette=SCALOS.PAL
ScoreDefeatMusic=SCORE
```

| Key | Meaning |
|---|---|
| `VictoryArt=` / `DefeatArt=` | SHP whose **first frame** is the card. Must be installed; one the game cannot open is logged and the retail picture stays. |
| `VictoryPalette=` / `DefeatPalette=` | Optional, one per art. Omitted: the side's palette, which is what a picture drawn in the side's own colours wants. |
| `VictoryMusic=` / `DefeatMusic=` | A **theme name** from `[Themes]` in `theme.ini` / `thememd.ini`, never a file name. The score page's own keys are `ScoreVictoryMusic=` / `ScoreDefeatMusic=`. |

Either half may be written on its own. Writing only the score half leaves the
battlefield retail; writing only the verdict half leaves the score page retail
with its side theme.

**Music is a theme, not a track.** The name has to be declared in `[Themes]`
(`theme.ini` for RA2's set, `thememd.ini` for Yuri's Revenge's), and that entry's
`Sound=` names the audio. Retail's own end-of-match theme is `SCORE`; the
in-match playlist themes (`Power`, `InDeep`, `Motorized`, ... — the full list is
in `[Themes]`) work too and are what a distinct verdict theme usually wants.

## Pictures

| Half | Slot | Size |
|---|---|---|
| Verdict card | the tactical viewport, centred; the sidebar and radar stay uncovered | authored for `GRFXTXT`'s 354x512, drawn whole — never cropped |
| Score background | the whole 800x600 score dialog, behind the columns | fills the dialog; cropped to 4:3 |

`tools/end_game_cards.py` converts RA3's end-game art into both halves:

```sh
python -m tools.end_game_cards --sources <ra3-art-dir> --out <game-dir>
```

Banners come out at 354x512 unclipped (the RA3 cards are banners, not score
pages — the earlier mix-up that gave 2:1 pictures a 4:3 slot); score pages are
cropped from the wallpapers to fill the dialog.

## The beat, in order

With `Custom End Game Movie` **on** and a beaten computer commander that has a
losing clip:

1. the last opponent falls: the retail banner goes up and the clip starts in
   the radar window. The result's own line ("You are victorious" or "You have
   lost") waits for the clip to end, so the clip cannot cut it; when the kill
   earns no clip, the line is said at once;
2. the battlefield keeps running for the retail `SavourDelay`, then the result
   is published and the battlefield stops, on the same frame as with the switch
   off, in a replay or in a headless run;
3. the verdict's theme starts, and the rest of the clip plays over the stopped
   battlefield;
4. the clip ends: the result's line is said if it is still waiting, the card
   rises, and the screen holds for 3 seconds;
5. the retail teardown fades out and the score page takes over with its own
   theme.

With the switch **off** the clip is cut when the result is published, and the
retail sequence runs unchanged, minus the card and its 3 seconds.

The score counts nothing after the result: kills, losses and the elapsed time
stop on the frame it is published.

## Switches

Three toggles in Options, stored as `[VERA]` keys in `VERA.INI`, all on by
default:

| Option | `[VERA]` key | What it controls |
|---|---|---|
| Custom End Game Banner | `CustomEndGameBanner=` | the card and its theme |
| Custom End Game Score | `CustomEndGameScore=` | the score page's background and theme |
| Custom End Game Movie | `CustomEndGameMovie=` | whether the card and score page wait for the losing commander's clip |

None of them changes the simulation: the result is published on the same frame
whatever they say.

## Which modes get what

| | Verdict card | Score page art | Verdict theme | Losing commander's clip |
|---|---|---|---|---|
| Skirmish | yes | yes | yes | yes, and the card and score page wait for it |
| LAN / WOL | yes | yes | yes | no, but the card's 3 s still put the score dialog 3 s later than retail |
| Campaign | no | no | no | no — missions use their own result dialogs |
| Replay playback | no | no | no | no — the recorded stream owns the timing |
| Observer | no card of its own | retail page | retail theme | no |
