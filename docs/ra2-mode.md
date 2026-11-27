# RA2 Mode

A VERA feature, experimental: VERA plays original Red Alert 2 instead of Yuri's
Revenge, from the Red Alert 2 files a Yuri's Revenge installation includes.

On desktop, use the switch just above **Exit Game** in the main menu. The
button names the other edition. Confirming saves that edition (`[VERA]
RA2Mode=` in VERA.INI) and restarts VERA into it; Cancel changes nothing.
Starting VERA with `-ra2` still overrides the saved edition for that launch.
On Android, set **RA2 Mode** in Options and restart the app.
A new profile starts in Yuri's Revenge.

## What changes

- **Files.** The game reads Red Alert 2's archives first (`ra2.mix` and the
  others without `md`) and uses Yuri's Revenge's only for what they lack. Its
  rules are `rules.ini`, `art.ini`, `ai.ini` and the theater INIs without `md`,
  whatever `config.toml`'s `[ini]` says. Its text is `ra2.csf`, with
  `ra2md.csf` for labels it lacks, and its subtitles `subtitle.txt`.
- **Menus.** The campaign menu shows Red Alert 2's three emblems: Allied, Boot
  Camp and Soviet. The loading screens, exit dialog and palettes are Red
  Alert 2's, and the YR Original Style box has no effect.
- **Play.** Where Red Alert 2 plays differently from Yuri's Revenge, the game
  follows Red Alert 2: garrisoned buildings fire their own weapon, with the
  weapon's own flash and a rate of fire that grows with the occupants; mind
  control holds one unit; the Armory trains infantry to elite; the Hospital
  heals without limit; the Cloning Vats recycle infantry, as a Grinder does;
  computer players get the difficulty values Red Alert 2 has built in; and the
  health bars, rank and ammo pips use Red Alert 2's frames.

## Online

RA2 Mode is part of your game's rules, which players compare before a match.
A player in RA2 Mode cannot play with players in Yuri's Revenge. When a room
or a start refuses, that player's game names RA2 Mode, the other players'
games list it among the reasons another game can differ, and an online room
says the player has "RA2 Mode or the INI files named in config.toml". Quick
Match plays Yuri's Revenge and asks you to turn RA2 Mode off. Players who all
have it on play together ([Networking](Networking.md)). The browser version
has no RA2 Mode.

## Known gaps

RA2 Mode is new and not fully checked against the original. Some menus and
overlays may still show Yuri's Revenge art, and a campaign trigger in the
second Allied mission is being checked: capturing the chapel may count as
destroying it. Please report what plays differently from Red Alert 2.
