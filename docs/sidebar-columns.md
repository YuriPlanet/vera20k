# Sidebar build-list columns

`[VERA] SidebarColumns=` chooses how many build pictures a sidebar row holds.
gamemd lays two (GetVisibleSlotCount `0x006AC430` counts `2 * rows`), and VERA
can lay three to five inside the same 168-pixel sidebar by shrinking the cells.
This is a VERA display mode: `sim/` never reads it, so saves, replays, network
sync and the AI are unaffected.

Set it in **Options → Interface → Sidebar Columns**. It applies at once and is
saved as `[VERA] SidebarColumns=` in `VERA.INI`; the profile writer stores the
key again with the other `[VERA]` values, and editing the file directly works
too.

| Value | Columns |
|---|---|
| `2` (default) | gamemd's own layout: 60x48 cameos, 63 pixels apart (64 on the Soviet table). |
| `3` | 39x31 cameos, 42 pixels apart. |
| `4` | 28x23 cameos, 31.5 pixels apart. |
| `5` | 22x18 cameos, 25 pixels apart. |

A value this build does not know reads as `2`, so a hand-edited file never
redraws the list in a layout the build cannot measure.

## How the cells share the width

The two original columns keep the span they take — `2 * ColumnWidth`, 126
pixels on the allied table — and each further column takes an equal share of
it. Every cameo and its row shrink by the same factor, so the grid keeps the
original's proportions and stays inside the sidebar's own width; the sidebar,
the battlefield, the radar and the command bar do not move.

At 1080 pixels tall the strip then holds about 25, 35 and 45 rows for three,
four and five columns, so one screen shows roughly 75, 140 and 225 entries
instead of the original 32. The scroll buttons, the capacity and the wheel
follow the new grid because they already count `rows * columns`.

## The row's button frames

SIDE2, the row background, is one 168x50 bitmap that paints the original's two
button frames: a 22-pixel panel edge, two 63-pixel cells, and a 20-pixel edge.
Stretching that pair to a wider count squashes the frames and still leaves two
of them, so above two columns VERA stops drawing the row whole. It cuts one
cell out of the art and draws it once per column, scaled with the cameo it
holds, then paints the panel's two edges back at their own width. The border
runs unbroken and every empty slot gets a frame of its own; the two-column
layout still draws the untouched row bitmap.

## What it costs

A cameo is a 60x48 bitmap, so it is resampled at these sizes. At three columns
the pictures and the price line still read; at five columns a picture is 22x18
and the Ready label has no room, so pick the count you can actually aim at.
Hover outlines, clicks, tooltips and clocks follow the smaller cells.

At the Large Sidebar size the sidebar draws at its own 1.5x pixels and copies
them 1:1, so a cell is drawn at the size the frame shows it: a three-column
cameo is a 58.5-pixel cell, not a 39-pixel one blown up afterwards. The
pictures stay as sharp as the Sidebar size allows instead of going through a
second resample.

## With `veraui.ini`

A mod's `[Layout.Cameos] Columns=` still sets the grid; the player's choice is
applied after the layout and decides the count, using the span the layout
leaves. `2` leaves a layout's own numbers alone. The mod's `ColumnWidth=` is
honoured as the span's source, so a layout that already narrows its columns
gives smaller cells rather than overlap.

## Residuals

- A capture keeps two columns so its pictures stay reproducible.
- The sprite count follows the visible grid: a 1080-pixel screen draws about
  75, 140 and 225 cameos for three, four and five columns, against the
  original 32. Each is one instance, and none is culled for its size. Above two
  columns the row background costs `columns + 2` pieces per row instead of one
  — about 125, 190 and 287 for the whole strip at 1080 pixels.
