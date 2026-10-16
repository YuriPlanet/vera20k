# Flat Riparius body and cell reduction reference

The `--ore` mode of the existing
[`bridge_shadow_render.py`](bridge_shadow_render.py) executes original
`CellClass::DrawOverlay_Body` (`0x47F6A0`) on physical stock TIB artwork,
through the global ordinary Convert, shape clipping, row walker and RGB565/Z
stores. It shares the bridge modes' archive/lexical, VM, surface, caller and
output-run owners. Their default startup and saved corpora remain unchanged.

```sh
python -m tools.spatial_oracle.bridge_shadow_render --ore --check
python -m tools.spatial_oracle.bridge_shadow_render --ore --write
```

The default checks without writing. The selected files are
[`ore_render.json`](ore_render.json) and its
[sidecar](ore_render.meta.json). The sidecar pins executable identity and command,
without repository-source hashes. Use the native dependencies and installation
configuration in [`../native_oracle.md`](../native_oracle.md). Authored MAP packs
use the existing `shrapnel_repair.map_facts.decode_cells` LCW/liblzo2 owner;
this is data extraction, not native map loading.

## Physical inputs and reader boundaries

The physical expansion `RULESMD.INI`, `MPBattleMD.ini`, `ARTMD.INI`, twelve
`TIB01..12.TEM` files and palettes are read through the existing MIX owner.
Original OverlayType constructors/full rules readers establish image pointers
and flags. Full original Tiberium constructor `0x7216C0` and reader `0x721A50`
establish Riparius index0, image run102, NumImages12, NumSlopes8, timers,
percentages and Value from physical lexical keys. Missing mode/map sections
retain these fields through original readers. The existing supplied unused
ColorScheme registry is outside the selected ore RGB path: that path reads the
global ordinary Convert and never Riparius's `Color=` scheme.

The packet also pins the physical loose `GoldSt.mmx` hash and authored
`(123,33)` id102/data4/tile141/sub0/level5, and `(124,33)` id102/data7/tile133/
sub0/level5. The physical map declares **URBAN**, Size103x82. All twelve matching
`TIB*.URB` files and `URBAN.PAL` are retained as physical source identities and
asserted byte-identical to the selected TEM artwork and `TEMPERAT.PAL`.
These bytes support the selected artwork/palette alias, not native map
construction or the derived live slope/flags. The raster supplies slope0,
flags0, level5 and its projected z0 point explicitly. A full native TMP/map
reader remains outside this fixture.

`inputs.sources` records physical hashes/lengths and archive identities. Loose
map input has `kind: loose` and null `outer`/`inner`; it must be read as a loose
file. The selected expansion rules override the base copy. The packet does not
replay the entire asset search policy, optional LANGRULE discovery, all scenario
passes, or arbitrary map overrides.

## The ordinary converter

Ore reads global `0x87F6B8`, rather than the Cell LightConvert used by high
bridges. The optional ordinary initialization extends the existing
[`bridge_render_inputs_palette.initialize`](../projectile_oracle/bridge_render_inputs_palette.py)
owner without changing its default behavior or allocation order.

Original `Init_Game` loads the file-backed literal `TEMPERAT.PAL` through
`0x52BC49..0x52BCBB`, including the original byte expansion. Original
`0x52BE0F..0x52BE48` constructs a basic Convert (`0x48E740`), builds its shade
table (`0x4BBB00`), initializes its blitters (`0x48EBF0`) and publishes
`0x87F6B8`. Source and destination arguments are `0x885780`, using the existing
RGB565 surface transport and53 shade rows. The allocated table is native
output; no high-bridge LightConvert, generated table or reconstructed RGB enters
this VM. Its source palette and expanded/table hashes are recorded in
`inputs.ordinary_convert`.

The later `Init_Theater` palette writer selects `%s.PAL`; selected TEMPERAT and
URBAN physical bytes are identical. The basic Convert has its own shade-table
storage. Other theater lifetime/color behavior is not inferred from these
aliases.

## Native output and lifecycle

Eight rows retain the established `surface`, `input`, `draws`, `native_rect`,
`canvas` and `output.runs` schema: density4/1/0 at123,33, empty, a124,33 image
variant, and one-scanline near/equal/far old-Z controls. The first density1 row
precedes a native reduction; its stored result selects the density0 raster.
Depth-control candidates come from an original density4 raster pixel. Native
shape/selector/row-walker/opaque leaf `0x4990E0` supply all RGB565 and Z values,
including strict equality refusal. Sourcezero stays transparent. A127,
backgroundFFFF, oldZ, viewport, clip and cell point are component inputs.

`controls` executes the original signed-coordinate flat selector
`0x47F8D7..0x47F900`: all twelve positive remainders, the two selected map
coordinates and signed16 boundaries. Its divisor/base come from original type
readers. The corpus contains no host implementation of this selector.

`lifecycle` executes original `Reduce_Tiberium` (`0x480A80`) twice with amount1
in the same raster VM. The supplied sparse103x82 map has this one resident cell
and empty shared-Dummy neighbors, sentinel TMPFFFF, CliffBack0, and no LAT.
The complete original `RecalcAttributes` (`0x47D2B0`) executes; no recalc or
reduction result is substituted. Native direction initialization, all-class
spread-bit clear and neighbor admission execute. Empty neighbors produce no
Scenario RNG draws, and full RNG-state hashes remain identical. This bounds
removal of this component cell, **not** GoldSt's actual tile141 attribute
recalculation or all queue/rebuild histories. Those already have consumed native
owners in `terrain_recalc`, `harvest_field` and `ore_queue`.

After clear, the existing original Tactical suffix executes with identity-1 and
records no body/shadow calls. The empty row's framebuffer/Z is that untouched
native output; `native_rect=[0,0,0,0]` is the no-draw representation, not a shape
rectangle query. The suffix retains the existing map/admission/rectangle/draw
transport boundaries. Reduction's radar/tactical dirty calls are observed sinks;
its old-art rectangle queries and mutation body execute original instructions.

The fixture does not execute normal harvester scheduling, cargo credit, the
production frame/index handoff or GPU submission. Production consumers must
exercise those existing owners against these outputs. It does not establish a
complete original frame, shroud composition, FoggedObject drawing, sloped ore,
other resource types, or closure of every `DrawOverlay_Body` branch.
