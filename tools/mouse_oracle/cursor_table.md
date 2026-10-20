# Mouse cursor tables

`cursor_table.py` reads two structures that are compiled into `gamemd.exe` and
are not INI-driven, and writes them to
[`cursor_table.json`](cursor_table.json) for `src/render/cursor_atlas.rs` to
check its `CURSOR_DEFS` against.

```sh
py -3 -m tools.mouse_oracle.cursor_table --exe <gamemd.exe> --write
py -3 -m tools.mouse_oracle.cursor_table --exe <gamemd.exe> --check
```

Any `gamemd.exe` is read; the five anchor checks below gate it.

## What it extracts

**The cursor descriptor table** at `0x0082D028`: records of 28 bytes, indexed
by `Set_Mouse_Cursor_Shape` (`0x005BDC80`) as `base + 28 * row`. Each record is

| field | meaning |
| --- | --- |
| `frame`, `count` | the row's mouse.sha frame set |
| `rate` | animation clock units of 16 ms; `0` does not advance |
| `mini_frame`, `mini_count` | a second frame set, `-1` when the row has none |
| `hotspot_x`, `hotspot_y` | `0` near edge, `12345` centre, `54321` far edge (`0x005BDD27`, `0x005BDD44`) |

The table stores no length, so the extent is derived: records are read until one
fails the layout above, and the first rejected record is written to
`table_extent` with its bytes. On the stock layout that is 86 rows, after which
the bytes are an unrelated string (`Stretching movie %dx%d -> %d`).

**The action-to-row switch** in `DisplayClass::SetCursorFromAction`
(`0x004AAE90`): the dispatch at `0x004AB449` indexes the jump table at
`0x004AB884` with `action - 1`. The entry count is read from that block's own
`cmp eax, 0x47` — `ja` is strictly above, so the bound is inclusive and the
table holds **72** entries (action 72 reaches row 84; index 72 onward is
`0x90909090` padding). Each target pushes a cursor row and calls the display's
`+0x48` virtual; the default target pushes row 0. Two targets (`0x004AB4D2`,
`0x004AB765`) take their row from a call (`0x00731CC0`, `0x00731CB0`) instead of
an immediate, and are recorded as such.

## What it does not establish

* Which game state produces which action. The map is action → row → frames only;
  the `What_Action_OnObject`/`OnCell` ladders are separate owners.
* How the original advances a record with `count > 1` and `rate 0`. It is stored
  as read.
* Which caller asks for a record's `mini_frame` set.
* Any rendered pixel: nothing here executes or captures native drawing.

## Provenance of the committed fixture

The committed fixture was extracted from a launcher-patched copy. Its sidecar's
legacy `native_sha256` and `binary_is_stock` fields are inert history; new
sidecars record neither.
It is usable because the tool refuses to read a binary until five anchor checks
pass — the table-base `lea` at `0x005BDC95`, the dispatch `jmp` at `0x004AB453`,
the `call [edx+0x48]` at `0x004AB43C`, and the Building vtable slots `+0x4E4`
and `+0x444` — the last two being addresses this repository already recorded
from stock analysis in [`../voxel_oracle/building_barrel.md`](../voxel_oracle/building_barrel.md).
Regenerate against an unpatched `gamemd.exe` to drop that dependency.
