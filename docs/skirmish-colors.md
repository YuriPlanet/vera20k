# Lobby player colours

Yuri's Revenge lists eight player colours. VERA20k seats up to thirty players, so its
Skirmish screen, LAN and online game rooms and My Information list thirty: the same
eight first, then twenty-two more. None is black, white or grey.

This is a VERA20k feature. gamemd has no such list, so only the conversion it extends
has a native address (`Game_GetLinkedColor`, `0x0069A310`).

## What stays as in the original

- The first eight colours, their order, their swatches and their numbers (0 to 7).
- An observer's colour number (8).
- A Random colour. While one of the eight is free, Random draws from the eight exactly
  as gamemd does, so a match with eight players or fewer looks the same unless someone
  picks one of the new colours by hand. With more than eight players, Random then takes
  the new colours nobody holds, and reuses a colour only when every listed colour is
  taken.

## The list

The colours after the eight are a list in the rules INI, in the order the lobby shows
them:

```ini
[SkirmishColors]
1=VeraBurgundy
2=VeraRose
3=MyTeal

[Colors]
MyTeal=123,255,145
```

- Each line names a `[Colors]` entry. `[Colors]` values are Hue,Saturation,Value, each
  0 to 255, as for every other colour in the game.
- The list holds at most 22 names. Later ones are ignored.
- A name `[Colors]` does not have is skipped, unless it is one of VERA20k's own (below).
- `none` is skipped, so `1=none` alone leaves the lobby with the original eight.
- A colour listed twice, or one of the eight's own `[Colors]` entries, is listed once.
- Without a `[SkirmishColors]` section the lobby uses VERA20k's list.
- Rules whose `[Colors]` has fewer than the 15 entries the original eight need get
  none of VERA20k's own colours; names their `[Colors]` defines still work.

Put the section in a mod's `rulesmd.ini` or in your own
[`VERARULES.INI`](personal-rules.md). In a network game every player needs the same
list, as for any other rules change.

## VERA20k's own colours

These are the default list, in order. Their values live in
[`src/rules/skirmish_colors.ini`](../src/rules/skirmish_colors.ini).

| Name | H,S,V | Name | H,S,V |
| --- | --- | --- | --- |
| `VeraBurgundy` | 250,255,120 | `VeraMint` | 107,120,255 |
| `VeraRose` | 249,135,252 | `VeraSeaGreen` | 113,210,200 |
| `VeraHotPink` | 227,255,255 | `VeraTeal` | 123,255,145 |
| `VeraPlum` | 229,255,170 | `VeraAzure` | 138,255,215 |
| `VeraBrown` | 23,255,144 | `VeraRoyalBlue` | 168,255,215 |
| `VeraTan` | 18,150,195 | `VeraNavy` | 165,210,125 |
| `VeraBronze` | 41,240,190 | `VeraSteelBlue` | 139,240,125 |
| `VeraLime` | 54,255,235 | `VeraPeriwinkle` | 166,120,255 |
| `VeraOlive` | 49,159,140 | `VeraViolet` | 188,255,255 |
| `VeraNeonGreen` | 84,255,255 | `VeraLavender` | 185,120,175 |
| `VeraForest` | 86,251,120 | `VeraOrchid` | 204,165,255 |

To change one, define the same name in your `[Colors]`:

```ini
[Colors]
VeraTeal=120,255,200
```

A listed name the rules `[Colors]` leaves out is added after the rules' own entries,
so the original entries keep their places. A map can use these names in a House's
`Color=` too.

## How a colour is numbered

A player's colour is one number in saved settings, replays and network games: 0 to 7
for the original eight, 8 for an observer, and 9 to 30 for the list above in order.
gamemd passes a number from 9 up straight through as a colour-scheme index; VERA20k
reads 9 to 30 as its list instead and leaves any higher number as gamemd has it.

## Older versions and online play

- Nothing new is sent: a room already carries each colour as a number from 0 to 255,
  and the lobby server passes it on unchanged.
- A game from before this list cannot be in a match with one that has it: LAN and
  online games already refuse a different build before they start.
- Going back to an older version is safe. Your own colour is saved as
  `[MultiPlayer] Color=` and `ColorEx=`. A new colour is saved as `Color=0` with its
  number in `ColorEx=`, which an older game reads as the first colour, picked. Its
  next save forgets the new colour. Computer players' new colours come back as
  Random there.

## Where VERA20K owns it

- `rules/skirmish_colors.rs`: the list, the built-in colours and the conversion from a
  colour number to its `[Colors]` entry (`SkirmishColorMode`).
- `skirmish_launch.rs`: the Random colour draw (`draw_collision_free_color`).
- `ui/skirmish_shell/state/combos.rs`: the list every colour box shows
  (`available_color_choices`).
- `app/frontend/skirmish_shell_render/controls.rs`: the swatches (`house_color_tint`).

## Residuals

- The new colours have no name in the status line under the lobby; the eight keep
  gamemd's (`STT:PlayerColor*`).
- Quick Match plays in the original eight. A saved new colour goes as Random there.
- A match started from a `spawn.ini` still takes colours 0 to 7 only. A `phobos`
  build's `spawn.ini` takes any number, and there 9 to 30 now mean this list unless
  `SkirmishUnlimitedColors` is on; before, they were raw colour-scheme indexes.
- The loading screen picks each player's colour with the rules loaded before the map.
  A map or mode INI that adds its own `[Colors]` entries can make a new colour show as
  another one there; in the match it is right.
- A host does not yet turn a colour number its lobby does not list, sent by a changed
  game, into Random. It draws as a grey or other house colour, as before this list.
- With Phobos [`SkirmishUnlimitedColors`](phobos-skirmish-unlimited-colors.md) on, the
  lobby lists the original eight only: that hook gives every colour number its own
  meaning.
- A map whose own rules change `[SkirmishColors]` changes what a picked colour is in
  that match; the lobby shows the list from the rules loaded before the map.
- Ares' `uimd.ini` colour slots are still not read
  ([R2](ares-sidebar-ui-hooks.md)).
