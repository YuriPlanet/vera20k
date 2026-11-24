# Resident terrain files and saved-load selector

Run `python -m tools.spatial_oracle.bridge_recalc_presentation --check`, with
`RA2_DIR` or `VERA20K_GAMEMD_EXE` configured through
[`native_oracle`](../native_oracle.md). `VERA20K_BRIDGE_REPAIR_ASSETS` names a
directory containing the original extracted `Ovrpsb03.ubn` and `Ovrpsb03a.ubn`;
their corpus hashes identify the physical NewUrban entries. `--write` explicitly
regenerates the reference and sidecar.

Historical `groups`, `process_table` and `wheel_values` retain their existing
Recalc, independent radar and tactical lookup observations. Those controls use
a supplied initialized table. The NewUrban damaged-file out-of-bounds witness
is an asset boundary, not a claimed authored stock-map path or full GPU result.

## Native saved-load caller and admission

Original LoadContent `0x67E730` clears the outgoing scene and loads Scenario,
resources and Rules before MouseLoad `0x5BDF70` at `0x67E870`. MouseLoad reads
saved cells through OleLoad; CellLoad `0x4839F0` restores the raw body and repairs
owned pointers. Its theater tail calls `0x71DCA0`, compares saved theater with
previous `0x822CF8`, then either loads types through `0x545150` with DL=1 or
clears resident-use counts through `0x547110`. At `0x5BE653..0x5BE657` it sets
CL=DL=0 and calls full `0x546DA0`. This route never requires fresh map Fill.
These whole-loader/caller facts are instruction-level evidence.

The original `0x546DA0` scan resets the map iterator `0x578350` and repeatedly
calls `0x578290`. For each restored cell:

- Tile `+0x38 == 0xFFFF`, or signed tile index at/above registry count `A8ED38`,
  selects ClearTile `AA10B0` and reaches selector `0x4814F0` even with one file.
- A valid tile with file count `Type+0x2F0 <= 1` uses file zero without calling
  the selector.
- With more files, it obtains the resident TMP through virtual slot `+0x9C`,
  indexes the subheader with unsigned Cell `+0x11A` modulo template width×height,
  and tests subheader `+0x24` bit 2. A set bit uses Cell `+0x140` bit 13 as file
  0/1 and skips the selector. Missing raw/header pointers or a clear bit reach
  the selector at `0x546E7C`.

The selector checks process flag `89E7C7` before examining its file-count branch.
On first entry, original `0x481502..0x481570` initializes 64 **dwords** at
`89E620`, using Main raw Next `0x65C780` at `0x481526`. Fixed-table counts ≤4
still pass through this initializer. With the flag set, the existing process
table survives and no initialization draw is consumed. No save-owned table is
restored by these bodies; a load uses the current process Main continuation.
The menu's later reseed boundary is separate; see [the RNG owner](../rmg_oracle/rng.md).

## Executed load controls and downstream effects

Eight `load_selector_cases` execute original `0x5BE653` through its return at
`0x5BE65C`, including full `0x546DA0`, real map iterator, selector, usage counts
and culling. They compose this owner's resident TMP fixture and native Seed/Next
producer. All TMPs start resident; type file-management byte `+0x2F4` and
filenames are supplied facts. Declared seams are clock zero, diagnostic return
and allocator-free transport, which records released fixture file pointers.

The native scan increments the selected variant's Type `+0x308` count. Its
later cull releases unreferenced managed resident files and clears Type `+0xA4`
and `+0xA8`. The recorded Cell fixture bytes, including tile, subtile, flags and
metadata, remain unchanged. This scan does not rewrite a cell's variant or
radar state. Separate later radar/tactical consumers select from the process
table; restored mutable cells and current process presentation selection are
different authorities. The `0x547020` reload arm can reconstruct type file/header
metadata when a needed file is absent; these rows do not need or execute that IO.

The seed-0 cold examples consume 128 Main draws; seed 1234 after seven prior
draws consumes 149. Warm initialization consumes zero. Valid single-file and
damaged-header rows can leave a cold cache uninitialized, while a ClearTile
sentinel with one file initializes it. Therefore neither unconditional eager
initialization nor a fixed draw count follows from native behavior.

The two-cell witness observes native visit order `(1,2),(2,1)` on a supplied
width-2 pointer plane, choosing files 4 and 2 with one initialization. This is
an iterator witness, not an exhaustive map-order proof. Each row exposes
`input`, raw RNG `before/after`, `raw_draws`, `table_before/after`, `visited_cells`,
`selected_files`, `usage_counts`, `freed_files`, `file_present` and unchanged
`cell_before/after`. Original Next independently replays the exact observed
draws and Main continuation; Scenario and MapGen are bracketed unchanged.

No complete native SAV, cold process startup prefix, every theater/TMP reader,
full-load RNG total, or whole-world/pixel parity is established. The supplied
one/two-cell maps and synthetic TMPs isolate this required load boundary.
