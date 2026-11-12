# Interface layout (`veraui.ini`)

A mod can lay out the match screen with a file named `veraui.ini`: put the sidebar on
any edge of the screen, put the command bar on top or leave it out, move the sidebar's
parts, leave parts out, change how many columns of build pictures it has and how big it
is, place the messages, the timers, the banner and the tooltips, and add buttons and
text of your own. Without the file the game looks exactly as the original does.

A player can also [drag the parts in the game](#a-players-own-layout) and keep where
they put them, on top of any mod's layout.

The keys on this page keep their meaning in later versions, so a layout you write now
keeps working.

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
  side, and of the layout's own two buttons, the match clock and a second grid of
  build pictures that always shows the infantry.
- Yuri gets a wider sidebar with three columns of build pictures and no top buttons,
  which needs wider pictures of your own to look finished. His timers sit at the top
  right of the battlefield and his tooltips stay in a corner of the screen.

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

## On the battlefield

The battlefield holds four pieces of its own:

| Part | What it is | Where it sits |
| --- | --- | --- |
| `Messages` | The lines of text at the top left: chat, EVA and mission text | Starts at the top-left corner of its box. Lines are as wide as the box |
| `Timers` | The superweapon and mission timers | Stacks up from the bottom-right corner of its box |
| `Banner` | The picture or text that says you won or lost | In the middle of its box |
| `Powers` | [Phobos](phobos.md)' column of superweapon buttons, when a mod turns it on | At the battlefield's left edge |

`Messages`, `Timers` and `Banner` each have a box inside the battlefield. In the
original the box is the whole battlefield. These keys move and size it:

| Key | Meaning | Original |
| --- | --- | --- |
| `Hold` | The corner or edge of the battlefield the box is measured from: `TopLeft`, `Top`, `TopRight`, `Left`, `Middle`, `Right`, `BottomLeft`, `Bottom` or `BottomRight` | `TopLeft` |
| `X`, `Y` | How far in from there the box starts | 0, 0 |
| `Width`, `Height` | The box's size. `Rest` is what is left of the battlefield | `Rest` |

These numbers are screen pixels. The Sidebar size option doesn't scale these pieces.

This gives the timers a box 120 tall at the top of the battlefield, 8 pixels down:

```ini
[Mine.Timers]
Hold=TopRight
Y=8
Height=120
```

The timers stack up from the box's bottom-right corner, so the first one sits 128
pixels down at the right edge and the next ones go above it.

This starts the messages 180 pixels in, clear of a sidebar that floats on the left:

```ini
[Mine.Messages]
X=180
```

`Powers` has no box. Its `X` and `Y` move the column right and down from its own place.

The battlefield's `Parts=` says which of the four it has:

```ini
[Mine.Field]
Parts=Messages,Timers,Banner,Powers
```

A piece that isn't in the list isn't drawn. The game still plays its sounds, and the
chat keys still work.

### Tooltips

`Tooltip` is the small box of text at the cursor. It belongs to the whole screen, so it
is in no `Parts=` list and can't be left out.

| Key | Meaning | Original |
| --- | --- | --- |
| `Hold` | `Cursor`, or a corner or edge of the screen to keep every tooltip in: the same nine words as above | `Cursor` |
| `X`, `Y` | With `Cursor`: how far right and down from the cursor. With a corner: how far in from it | 12, 16 |

```ini
[Mine.Tooltip]
Hold=BottomLeft
X=8
Y=8
```

## Adding buttons and text

A layout can add parts of its own, of two kinds: a button that gives a command when it
is clicked, and a text that shows a value.

Give the new part a name, put the name in the `Parts=` of `Side` or of `Field`, and
give it a section with `Type=`:

```ini
[Mine.Side]
Parts=Credits,Top,Radar,Middle,Cameos,Power,StopAll

[Mine.StopAll]
Type=Button
Command=StopObject
Label=Stop
X=10
Y=300

[Mine.Field]
Parts=Messages,Timers,Banner,Powers,Clock

[Mine.Clock]
Type=Text
Value=Time
Hold=TopRight
X=6
Y=6
```

This puts a Stop button in the sidebar and the match clock in the top-right corner of
the battlefield.

| Key | Meaning |
| --- | --- |
| `Type` | `Button` or `Text`. `Row`, `Column` and `CameoGrid` are below |
| `Command` | For a button: what a click does. See [Commands](#commands) |
| `Label` | For a button without a picture: the words on it. The name of a line in the game's string table shows that line's text. Without `Label` the button shows its command's name |
| `Art` | For a button: its picture, an SHP file like `mybutton.shp`. Frame 0 is the button and frame 1 the button held down. It uses the sidebar's palette (`sidebar.pal`), as the command bar's buttons do |
| `Value` | For a text: what it shows. See [Values](#values) |
| `Hold` | The corner or edge of `Side` or `Field` the part is measured from: the nine words of [On the battlefield](#on-the-battlefield). `TopLeft` if you leave it out |
| `X`, `Y` | How far in from there the part sits |
| `Width`, `Height` | The part's size. Left out, it is as big as its words or its picture. A picture is never stretched |

- In `Side` the numbers are sidebar pixels and the part scales with the Sidebar size
  option. In `Field` they are screen pixels.
- An added part is drawn over the original parts. Words are in the colour of the
  side's text. A button without a picture is a framed box with its label in it, and a
  text on the battlefield stands on black, like the timers.
- Put a button's picture file in your mod's folder, or name one of the game's own, like
  `button06.shp`. If the file isn't found the button shows its label instead.
- A part with a section but in no `Parts=` list isn't shown.
- A new part needs a name of its own: not `Money`, `Radar` or another original part's.

### Rows and columns

A `Row` or a `Column` holds buttons and texts and places them one after another, so
they need no `X` and `Y` of their own:

```ini
[Mine.Field]
Parts=Messages,Timers,Banner,Powers,Orders

[Mine.Orders]
Type=Row
Hold=Bottom
Y=4
Gap=2
Parts=StopAll,GuardAll,Spread

[Mine.StopAll]
Type=Button
Command=Stop
Art=button08.shp

[Mine.GuardAll]
Type=Button
Command=Guard
Art=button06.shp

[Mine.Spread]
Type=Button
Command=ScatterObject
Label=Scatter
```

This puts three buttons side by side, in the middle of the battlefield's bottom edge.

| Key | Meaning |
| --- | --- |
| `Type` | `Row` places its parts from left to right, `Column` from top to bottom |
| `Parts` | The buttons and texts it holds, in order |
| `Gap` | The pixels between one part and the next. 0 if you leave it out |
| `Hold`, `X`, `Y` | Where the row or column sits in `Side` or `Field`, as for a button |
| `Width` on a row, `Height` on a column | Starts a new line when the next part would pass it |

- A part goes in one list only: the row's or column's, not also `Side`'s or `Field`'s.
- Inside a row or column a part's own `Hold`, `X` and `Y` aren't used. `Width` and
  `Height` still size it.
- A row or column can't hold another row or column yet.

With a row you can build a command bar of your own. Leave the original out with
`Parts=Field,Side` on the layout, and use the bar's own pictures: `button00.shp` to
`button10.shp` are `Team01`, `Team02`, `Team03`, `TypeSelect`, `Deploy`, `AttackMove`,
`Guard`, `Beacon`, `Stop`, `PlanningMode` and `Cheer`.

### More grids of build pictures

A `CameoGrid` shows one tab's build pictures in the sidebar, beside the sidebar's own
grid:

```ini
[Mine.Side]
Parts=Credits,Top,Radar,Middle,Cameos,Power,Troops

[Mine.Troops]
Type=CameoGrid
Tab=Infantry
Columns=3
Rows=2
X=336
Y=1
```

| Key | Meaning |
| --- | --- |
| `Tab` | Whose build pictures it shows: `Building`, `Defense`, `Infantry` or `Vehicle` |
| `Columns` | How many pictures are in a row, 1 to 8. The sidebar's own number if you leave it out |
| `Rows` | How many rows it shows, 1 to 60. 2 if you leave it out |
| `Hold`, `X`, `Y` | Where its first picture sits in `Side`, as for a button |

- It works like the sidebar's own grid: click to build, right-click to hold or cancel,
  and the same tooltips, clocks and Ready labels.
- The mouse wheel over the grid scrolls it, one row a notch. It has no scroll buttons.
- It goes in `Side` only, and it has no background of its own: the pictures draw over
  whatever is there.
- Its pictures are spaced as `[Cameos]` says (`ColumnWidth`, `RowHeight`).
- To show all four tabs at once, add a grid for each. You can then leave `Cameos` out
  of `Side` and `Tabs` out of `Middle`.

### Commands

A button gives a command the game already has, as if you pressed its key:

- Any name the keyboard file (`keyboardmd.ini`) binds a key to: `StopObject`,
  `GuardObject`, `DeployObject`, `ScatterObject`, `ToggleRepair`, `ToggleSell`,
  `CenterBase`, `Options`, `PlaceBeacon`, `TeamSelect_1` to `TeamSelect_10`,
  `TeamCreate_1` to `TeamCreate_10`, `StructureTab`, `InfantryTab` and the rest. They
  are the commands on the game's keyboard options page.
- A command bar button's name from `uimd.ini`: `Team01`, `Team02`, `Team03`,
  `TypeSelect`, `Deploy`, `AttackMove`, `Guard`, `Beacon`, `Stop`, `PlanningMode` or
  `Cheer`. The button then does what that bar button does.

Commands that only work while a key is held, like `CameraUp`, do nothing from a button.

### Values

| Value | What it shows |
| --- | --- |
| `Credits` | Your money, counting as the sidebar's number does |
| `Power` | The power your base makes |
| `Drain` | The power your base uses |
| `Name` | Your player name |
| `Time` | How long the match has run |
| `Kills` | How many units and buildings you have destroyed |
| `Losses` | How many you have lost |

## A player's own layout

A player can move the parts in a match without writing a file.

1. In a match, press the **Edit Layout** key: Ctrl+Alt+E, unless you changed it on the
   keyboard options page. Every part that can move gets an outline and its name.
2. Drag a part with the left mouse button. It follows the cursor.
3. Press the key again to finish. Until then the mouse only moves parts.

What can be dragged: the sidebar's `Credits`, `Top`, `Radar`, `Middle`, `Cameos` and
`Power`; the `Messages`, `Timers` and `Banner` on the battlefield, each by a small
handle where it sits; and every button, text, row, column and grid of build pictures a
layout added.

The game keeps what you moved in a `veraui.ini` of your own, in the
[VERA20k folder](modding.md#the-vera20k-folder). It is the same kind of file as a
mod's, so you can also write it by hand, with any key on this page.

- Your file is laid over the mod's. It changes only the keys it writes; everything
  else stays as the mod has it.
- A side the mod gives a layout keeps that layout, and your changes go under that
  layout's name: `[Low.Radar]` for a mod's layout called `Low`. With another mod, or
  without one, those changes simply aren't used.
- A side the mod leaves as the original gets a layout of your own, named `Own` and the
  side: `[OwnGDI.Radar]`.
- To undo a change, delete its section from your file. To undo everything, delete the
  file.
- Putting the sidebar or the command bar on another edge isn't done by dragging:
  write `Hold=` in your file.

## Not yet

These keys belong to the format, but this version can't change them. You can leave them
out or write the original value. Any other value is skipped, and the game tells you.

| Key | What it is for | Original value |
| --- | --- | --- |
| `Type` on an original part, or a new part of an original kind | The kind of part: `Battlefield`, `Panel`, `Radar`, `Tabs` or `PowerBar`. A layout can only add a `Button`, a `Text`, a `Row`, a `Column` or a `CameoGrid` | The part's own kind |
| `Hold` | The edge or corner of its parent a part is measured from, for the sidebar's own parts and `Field` | `TopLeft`. `Top` for `Money` |
| `Width`, `Height` | A part's size, for the sidebar's own parts, `Bar` and `Field`. `Rest` means what is left | `Width=Rest` and `Height=32` for `Bar`, `Rest` for both on `Field`, `Height=Rest` for `Cameos` |
| `Art` on an original part | The pictures an original part draws | None yet: replace the picture files instead |
| `CellWidth`, `CellHeight` | The size of one build picture | 60, 48 |
| `Command`, `Value` on an original part | A new command for an original button, or a new value for the money number | The part's own |

The three big parts hold on to their edges: they have no `X` or `Y`. The command bar
can't stand on the left or the right, and its buttons can't be changed here yet. A
layout can add [buttons, text, rows, columns and grids of build
pictures](#adding-buttons-and-text) but no other kind of part yet, and a section that
is neither an original part nor one of those is skipped.

## If you make a mistake

A mistake in the file never stops the game.

| In the file | What the game does |
| --- | --- |
| A key or a part it doesn't know | Skips it |
| A value that isn't a whole number | Skips it and keeps the original |
| A number that is far too big or too small | Uses the nearest number it allows |
| A `Parts=` list with a name that is no part, or a part held somewhere else | Skips that name and keeps the rest of the list |
| A button with a `Command=` the game doesn't have, or a text with a `Value=` it doesn't show | Leaves that part out |
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

Phobos' own column of superweapon buttons sits at the battlefield's left edge, beside
a sidebar on the left. A layout moves it with [`Powers`](#on-the-battlefield).
