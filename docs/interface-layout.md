# Interface layout (`veraui.ini`)

A mod can lay out the match screen with a file named `veraui.ini`: put the sidebar on
any edge of the screen, put the command bar on top or leave it out, move the sidebar's
parts, leave parts out, and change how many columns of build pictures it has and how
big it is. Without the file the game looks exactly as the original does.

This is part of a larger plan. Later versions will let a layout add buttons and text
and place the messages and timers on the battlefield. The keys on this page keep their
meaning when that comes, so a layout you write now keeps working.

## Where the file goes

Put `veraui.ini` in your [mod's folder](modding.md#the-mods-folder), next to its other
files, or inside the mod's `expandmdNN.mix`. A `veraui.ini` in the
[game folder](modding.md#the-game-folder) works too, with every mod and without one; the
mod's own file wins. Restart the game after you change it.

The file only changes what you see. Online players don't need the same file, and it
isn't one of the files a match compares. Packed inside an archive it is part of that
archive, which a match does compare like every other file in a mod's folder.

An older VERA20k doesn't read the file, or skips the keys it doesn't know and tells
the player. If your mod needs its layout, say so with `Needs=` in the mod's
[`mod.ini`](modding.md#modini).

[`docs/examples/veraui.ini`](examples/veraui.ini) is a sample you can copy. It has
three layouts, all with the original pictures:

- The Allies get a mirrored screen: the sidebar on the left, the command bar on top,
  Repair swapped with Sell, Diplomacy with Options, and the tabs from right to left.
- The Soviets get the sidebar along the bottom of the screen, with its panels side by
  side.
- Yuri gets a wider sidebar with three columns of build pictures and no top buttons,
  which needs wider pictures of your own to look finished.

## A small example

```ini
[Layouts]
GDI=Mine

[Mine.Radar]
Y=52

[Mine.Tabs]
X=20
Step=32
```

This moves the Allied radar 4 pixels down and spaces the Allied tabs like the Soviet
ones. Everything else stays as it was.

## Which side gets which layout

`[Layouts]` has one line for each side: the side's name, then the name you give its
layout.

```ini
[Layouts]
GDI=Mine
Nod=Mine
Default=Plain
```

- Side names are the ones under `[Sides]` in `rulesmd.ini`. In the original game `GDI`
  is the Allies, `Nod` is the Soviets and `ThirdSide` is Yuri.
- `Default=` is for every side that has no line of its own.
- A side with no line and no `Default=` keeps the original interface.
- Several sides can use one layout.

## A layout

A layout starts as the original interface of the side that uses it. You write only what
you change.

Each part you change gets a section named after the layout and the part, with a dot
between them: `[Mine.Radar]`.

The layout's own section, `[Mine]`, holds two keys:

| Key | Meaning |
| --- | --- |
| `Version=1` | The version of this format the layout was written for. Leave it at 1, or leave it out. |
| `Parts=Field,Side,Bar` | The big parts of the screen. See [The screen](#the-screen). |

All numbers are pixels of the original sidebar, which is 168 wide. The Sidebar size
option in the game makes the whole interface bigger, your layout included.

## The screen

The screen has three big parts:

| Part | What it is |
| --- | --- |
| `Field` | The battlefield |
| `Side` | The sidebar |
| `Bar` | The command bar: the row of buttons along the bottom of the original screen |

The sidebar takes its edge of the screen first. The command bar takes the top or the
bottom of what is left, and the battlefield gets the rest.

| Part | Key | Meaning | Original |
| --- | --- | --- | --- |
| `Side` | `Hold` | The edge the sidebar sits on: `Left`, `Right`, `Top` or `Bottom` | `Right` |
| `Side` | `Width` | How wide a sidebar on the left or the right is. 16 or more | 168 |
| `Side` | `Height` | How tall a sidebar on the top or the bottom is. 16 or more | 168 |
| `Side` | `Float` | `yes` lays the sidebar over the battlefield: the battlefield and the command bar then use the whole screen under it | `no` |
| `Bar` | `Hold` | The edge the command bar sits on: `Top` or `Bottom` | `Bottom` |

This puts the sidebar on the left and the command bar on top:

```ini
[Mine.Side]
Hold=Left

[Mine.Bar]
Hold=Top
```

A screen without the command bar leaves it out of the layout's `Parts=`:

```ini
[Mine]
Parts=Field,Side
```

The buttons are gone then, but their hotkeys still work. `Field` and `Side` can't be
left out yet.

### A sidebar on the top or the bottom

On the left or the right the sidebar is as tall as the screen and `Width` wide. On the
top or the bottom it is as wide as the screen and `Height` tall.

Its parts don't rearrange themselves. Each one still sits where its `X` and `Y` say,
measured from the sidebar's top-left corner, so a sidebar along the bottom needs new
places for the parts that would hang below it:

```ini
[Mine.Side]
Hold=Bottom
Height=160

[Mine.Middle]
X=168
Y=0

[Mine.Cameos]
X=336
Y=0

[Mine.Power]
X=336
Y=0
```

The money, the top buttons and the radar stay in the first 168 pixels. The panel with
Repair, Sell and the tabs moves beside them, and the build pictures beside that. The
sidebar shows as many rows of build pictures as fit in its height: two here.

Where no part draws, the sidebar is black. Wider pictures of your own fill it.

## The parts and where they sit

`X` and `Y` say where a part's top-left corner is, measured from the top-left corner of
the part it is inside. When you move a part, the parts inside it move with it.

| Part | What it is | Inside | Original `X`, `Y` (Allied) | Original `X`, `Y` (Soviet, Yuri) |
| --- | --- | --- | --- | --- |
| `Credits` | The plate behind the money (`credits.shp`) | `Side` | 0, 0 | 0, 0 |
| `Money` | The money number. Its `X` is measured from the middle of the plate | `Credits` | 0, 2 | 0, 2 |
| `Top` | The strip behind the two top buttons (`top.shp`) | `Side` | 0, 16 | 0, 16 |
| `Diplomacy` | The diplomacy button (`diplobtn.shp`) | `Top` | 11, 4 | 14, 5 |
| `Options` | The options button (`optbtn.shp`) | `Top` | 83, 4 | 86, 5 |
| `Radar` | The radar and its frame (`radar.shp`) | `Side` | 0, 48 | 0, 48 |
| `Middle` | The panel behind Repair, Sell and the tabs (`side1.shp`) | `Side` | 0, 158 | 0, 158 |
| `Repair` | The repair button (`repair.shp`) | `Middle` | 20, 8 | 33, 7 |
| `Sell` | The sell button (`sell.shp`) | `Middle` | 84, 8 | 85, 7 |
| `Tabs` | The four tab buttons. `X`, `Y` place the first one | `Middle` | 26, 39 | 20, 39 |
| `Cameos` | The build pictures, the rows behind them (`side2.shp`), the piece under them (`side3.shp`) and the two scroll buttons | `Side` | 0, 227 | 0, 227 |
| `Power` | The power bar. It is as tall as the rows of build pictures | `Side` | 5, 227 | 0, 227 |

`Side` is the sidebar itself.

Some parts have more keys:

| Part | Key | Meaning | Allied | Soviet, Yuri |
| --- | --- | --- | --- | --- |
| `Radar` | `MapX`, `MapY` | Where the map picture sits inside the radar frame | 16, 1 | 19, 2 |
| `Radar` | `MapWidth`, `MapHeight` | The size of the map picture | 140, 108 | 140, 108 |
| `Tabs` | `Step` | From the left edge of one tab to the left edge of the next. A number below zero runs the tabs from right to left | 29 | 32 |
| `Cameos` | `Columns` | How many build pictures are in a row. 1 to 8 | 2 | 2 |
| `Cameos` | `CellX`, `CellY` | Where the first build picture sits, measured from the part's top-left corner | 22, 1 | 22, 1 |
| `Cameos` | `ColumnWidth` | From the left edge of one column to the left edge of the next | 63 | 64 |
| `Cameos` | `RowHeight` | From the top of one row to the top of the next. It also decides how many rows fit in the sidebar. 16 or more | 50 | 50 |
| `Cameos` | `Footer` | The height kept free under the last row, for the scroll buttons | 33 | 25 |
| `Cameos` | `ScrollX`, `ScrollY` | Where the scroll-down button sits: from the part's left edge, and down from the bottom of the last row | 39, 7 | 39, 7 |
| `Cameos` | `ScrollStep` | From the left edge of the scroll-down button to the left edge of the scroll-up button | 46 | 45 |

Sizes that come from your pictures:

- A button or a tab is as big as its picture.
- The rows behind the build pictures repeat at the height of `side2.shp`. If your
  picture isn't 50 tall, give `RowHeight` the same height.
- A picture is never stretched. A wider sidebar or more columns need wider pictures of
  your own behind them.

## Leaving parts out

`Parts=` on a part that holds other parts says which of them it has. A part that isn't
in the list isn't drawn and takes no clicks, and the parts inside it go with it. The
original lists are:

```ini
[Mine.Side]
Parts=Credits,Top,Radar,Middle,Cameos,Power

[Mine.Credits]
Parts=Money

[Mine.Top]
Parts=Diplomacy,Options

[Mine.Middle]
Parts=Repair,Sell,Tabs
```

This layout has no top strip and no Sell button:

```ini
[Mine.Side]
Parts=Credits,Radar,Middle,Cameos,Power

[Mine.Middle]
Parts=Repair,Tabs
```

- The order is the order they are drawn in: where two panels overlap, the later one is
  on top. Buttons and tabs are always in front of the panels.
- `Parts=None` keeps a part but empties it: `[Mine.Top]` with `Parts=None` draws the
  strip without its buttons.
- A list can only name the parts the original holds there. `Sell` can't move from
  `Middle` to `Top` yet.
- Leaving a part out only takes it off the screen. The hotkeys for Repair, Sell, the
  tabs and Options still work, and the radar still goes on and off with its sounds.

## Not yet

These keys belong to the format, but this version can't change them. You can leave them
out or write the original value. Any other value is skipped, and the game tells you.

| Key | What it is for | Original value |
| --- | --- | --- |
| `Type` | The kind of part: `Battlefield`, `Panel`, `Radar`, `CameoGrid`, `Tabs`, `PowerBar`, `Button` or `Text` | The part's own kind |
| `Hold` | The edge or corner of its parent a part is measured from, for every part but `Side` and `Bar` | `TopLeft`. `Top` for `Money` |
| `Width`, `Height` | A part's size, for every part but `Side`. `Rest` means what is left | `Width=Rest` and `Height=32` for `Bar`, `Rest` for both on `Field`, `Height=Rest` for `Cameos` |
| `Art` | The pictures a part draws | None yet: replace the picture files instead |
| `CellWidth`, `CellHeight` | The size of one build picture | 60, 48 |
| `Command` | What a button does | None yet |
| `Value` | What a text part shows | None yet |

The three big parts hold on to their edges: they have no `X` or `Y`. The command bar
can't stand on the left or the right, and its buttons can't be changed here yet. A
layout also can't add a part: a section for a part that isn't in the tables above is
skipped.

## If you make a mistake

A mistake in the file never stops the game.

| In the file | What the game does |
| --- | --- |
| A key or a part it doesn't know | Skips it |
| A value that isn't a whole number | Skips it and keeps the original |
| A number that is far too big or too small | Uses the nearest number it allows |
| A `Parts=` list with a name that is no part, or a part held somewhere else | Skips that name and keeps the rest of the list |
| A layout named in `[Layouts]` that the file doesn't have | Keeps the original interface for that side |
| A `Version=` newer than the game knows | Keeps the original interface for that side |

When a match starts, a message names the first line that was skipped. All of them are
in `logs/ra2.log` in the [VERA20k folder](modding.md#the-vera20k-folder): search for
`VERAUI.INI`.

Capitals don't count in this file: `rowheight=` works like `RowHeight=`. In the game's
other INI files they do count.

## With Phobos

In a build with [Phobos](phobos.md), a side's
[`Sidebar.GDIPositions=`](phobos-sidebar-gdi.md) still picks whether the side starts
from the Allied or the Soviet numbers. The layout's numbers then go on top.

Phobos' own column of superweapon buttons stays at the left edge of the screen. With
the sidebar on the left the two overlap; a later version lets a layout place it.
