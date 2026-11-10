# Interface layout (`veraui.ini`)

A mod can move the parts of the sidebar with a file named `veraui.ini`. Without the
file the game looks exactly as the original does.

This is the first step of a larger plan. Later versions will let a layout leave parts
out, add buttons and text, and put the sidebar on another edge of the screen. The keys
on this page keep their meaning when that comes, so a layout you write now keeps working.

## Where the file goes

Put `veraui.ini` in your [mod's folder](modding.md#the-mods-folder), next to its other
files, or inside the mod's `expandmdNN.mix`. A `veraui.ini` in the
[game folder](modding.md#the-game-folder) works too, with every mod and without one; the
mod's own file wins. Restart the game after you change it.

The file only changes what you see. Online players don't need the same file, and it
isn't one of the files a match compares.

[`docs/examples/veraui.ini`](examples/veraui.ini) is a sample you can copy. It keeps the
original pictures, swaps Repair with Sell and Diplomacy with Options, and runs the tabs
from right to left.

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

The layout's own section, `[Mine]`, holds one key:

| Key | Meaning |
| --- | --- |
| `Version=1` | The version of this format the layout was written for. Leave it at 1, or leave the section out. |

## Parts you can move

All numbers are pixels of the original sidebar, which is 168 wide. The Sidebar size
option in the game makes the whole interface bigger, your layout included.

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
| `Cameos` | `CellX`, `CellY` | Where the first build picture sits, measured from the part's top-left corner | 22, 1 | 22, 1 |
| `Cameos` | `ColumnWidth` | From the left edge of one column to the left edge of the next | 63 | 64 |
| `Cameos` | `RowHeight` | From the top of one row to the top of the next. It also decides how many rows fit on the screen. 16 or more | 50 | 50 |
| `Cameos` | `Footer` | The height kept free under the last row, for the scroll buttons | 33 | 25 |
| `Cameos` | `ScrollX`, `ScrollY` | Where the scroll-down button sits: from the part's left edge, and down from the bottom of the last row | 39, 7 | 39, 7 |
| `Cameos` | `ScrollStep` | From the left edge of the scroll-down button to the left edge of the scroll-up button | 46 | 45 |

Sizes that come from your pictures:

- A button or a tab is as big as its picture.
- The rows behind the build pictures repeat at the height of `side2.shp`. If your
  picture isn't 50 tall, give `RowHeight` the same height.

## Not yet

These keys belong to the format, but this version can't change them. You can leave them
out or write the original value. Any other value is skipped, and the game tells you.

| Key | What it is for | Original value |
| --- | --- | --- |
| `Type` | The kind of part: `Battlefield`, `Panel`, `Radar`, `CameoGrid`, `Tabs`, `PowerBar`, `Button` or `Text` | The part's own kind |
| `Hold` | The edge or corner of its parent a part is measured from | `TopLeft`. `Top` for `Money`, `Right` for `Side`, `Bottom` for `Bar` |
| `Width`, `Height` | A part's size. `Rest` means what is left | `Width=168` and `Height=Rest` for `Side`, `Width=Rest` and `Height=32` for `Bar`, `Rest` for both on `Field`, `Height=Rest` for `Cameos` |
| `Parts` | The parts inside a part, in the order they are drawn | See below |
| `Art` | The pictures a part draws | None yet: replace the picture files instead |
| `Columns`, `CellWidth`, `CellHeight` | The number of columns of build pictures, and the size of one | 2, 60, 48 |
| `Command` | What a button does | None yet |
| `Value` | What a text part shows | None yet |

The original interface holds its parts like this:

```ini
[Mine]
Parts=Field,Side,Bar

[Mine.Side]
Parts=Credits,Top,Radar,Middle,Cameos,Power

[Mine.Credits]
Parts=Money

[Mine.Top]
Parts=Diplomacy,Options

[Mine.Middle]
Parts=Repair,Sell,Tabs
```

Three parts can't be moved at all yet: `Field` (the battlefield), `Side` (the sidebar)
and `Bar` (the bar along the bottom of the screen). A layout also can't add a part: a
section for a part that isn't in the table above is skipped.

## If you make a mistake

A mistake in the file never stops the game.

| In the file | What the game does |
| --- | --- |
| A key or a part it doesn't know | Skips it |
| A value that isn't a whole number | Skips it and keeps the original |
| A number that is far too big or too small | Uses the nearest number it allows |
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
