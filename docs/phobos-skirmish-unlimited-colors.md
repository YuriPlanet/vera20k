# Optional Phobos unlimited skirmish colours

Build with `--features phobos` and set `[General] SkirmishUnlimitedColors=yes`
to let a spawner address any `[Colors]` entry as a player colour. The feature is
Phobos `Phobos.INI.cpp`, hook `InitRules_ThingsThatShouldntBeSerailized` at
`0x0052D21F` (6 bytes), which reads the key with `CCINIClass::ReadBool` and then
patches `Game_GetLinkedColor` (`0x0069A310`) for the rest of the process.

```ini
[General]
SkirmishUnlimitedColors=false  ; source default false
```

## What the hook changes

gamemd converts a player colour priority into a runtime colour-scheme index at
`0x0069A310`. Vanilla answers the `SessionClass` table
`[3, 11, 21, 29, 13, 25, 17, 15, 5]`, which covers YR's eight multiplayer
colours plus the default. The scheme list is doubled (two schemes per
`[Colors]` entry), so the priority table's odd index divided by two is the
`[Colors]` entry the player gets: priority 6 selects entry 8.

Phobos writes seven bytes over the function's entry:

```text
mov eax, [esp+4]   ; the passed colour index
shl eax, 1
inc eax
```

The patch has no return of its own, so execution continues into the native body
at `0x0069A317`. Its `jne` is always taken (`2 * index + 1` is odd, never zero),
and the tail at `0x0069A328` still reads the priority table for a value below 9.
From index 4 on, the function returns `2 * index + 1` and the `[Colors]` entry is
the index the spawner sent; with stock rulesmd.ini, index 6 is the seventh entry,
`Orange=25,230,255` — the example Phobos documents. Indices 0–3 instead read
table entries 1, 3, 5 and 7 (schemes 11, 29, 25 and 15) and resolve to `[Colors]`
entries 5, 14, 12 and 7; VERA20K reproduces both ranges. The source comment for
that patch is "Disable Ares hook at this address so that our logic can run":
Ares hooks the same seven bytes for its customisable dropdown colours, which is
why Phobos lists the two as mutually exclusive. VERA20K has no Ares dropdown
colour feature, so nothing is disabled here.

Every consumer of the conversion follows the mode, because the hook is global:
the launch `HouseColorMap`, the loading screen's participant rows and their map
markers, the map-transfer text, trigger message colours, and network chat lines.
A YR-lobby priority that is not a `[Colors]` index therefore shows a different
colour under this mode, which is the "undesireable effects" Phobos warns about
for the stock launcher. Launch the game through a spawner client that sends
`[Colors]` indices; a `phobos` build's spawn.ini reader accepts the whole colour
byte for that reason.

## Where VERA20K owns it

- `rules/skirmish_colors.rs` — `SkirmishColorMode`, the key read and the
  priority conversion; the vanilla answer stays in `rules/color_scheme.rs`.
- `rules/ruleset.rs` — the immutable mode and its config hash;
  `RuleSet::skirmish_colors()` never answers this mode in a build without the
  feature, so callers carry no `cfg`.
- `app/frontend/skirmish.rs` — the launch `HouseColorMap`.
- `app/frontend/cncnet_spawn.rs` — the spawn.ini colour admission width.
- `app/loading/progress_row.rs` / `pump.rs` / `pump/render.rs` — the loading
  rows, their marker atlas keys and the map-transfer text.
- `app/input/messages.rs` — trigger and network chat colours.

## Residuals

- The skirmish shell lists YR's eight dropdown colours under this mode; the
  feature only changes how a received index resolves, as Phobos documents.
  VERA's [extra lobby colours](skirmish-colors.md) are not listed with the key
  on, because the hook gives every index its own meaning.
- A save records player priorities, not the mode. Loading a save made with the
  feature under the other setting re-resolves the priorities, which is the same
  behaviour as editing the key between sessions.
- `-2` is the lobby's random/observer marker, not an entry: the hook returns `-3`
  for it, which addresses no `[Colors]` entry. VERA clamps it to entry 0, where
  the untouched launcher path would leave the observer on the default scheme.
- An index past the end of `[Colors]` falls back to the default scheme ramp
  instead of reading past the list as the native array would.
- A stock (non-`phobos`) build keeps the spawn.ini colour cap at YR's eight
  priorities, so it rejects a spawner index of 8 or more rather than resolving
  it through the vanilla table.
- VERA reads the key from each scenario's layered rules; Phobos reads it once
  from RULESMD.INI at startup (`0x0052D21F`), so only in VERA can a mode or map
  INI change it.
