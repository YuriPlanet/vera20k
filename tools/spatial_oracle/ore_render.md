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
[sidecar](ore_render.meta.json). The sidecar pins the command,
without executable or repository-source hashes. Use the native dependencies and installation
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

## Additive current-cell radar comparison

`radar` executes after every historical input, selector, eight raster rows and
sentinel reduction result has been captured; those existing fields are unchanged.
`ore_radar.py` orchestrates the existing shared VM, native runner, archive/INI,
TMP resource and surface owners. `sidebar_oracle/radar_surface.py` remains the
independent stock radar-frame4912B0 history owner; it is not a weighted minimap
sampler and is neither copied nor replaced here.

`radar.depletion.terrain` pins physical `URBANMD.INI` and `clat10.urb` from
ra2md/localmd and ra2/isourb. Original theater General/ordinal projection plus
FileName/TilesInSet reads establish selected tile141, TileSet0014 base132,
FileNameclat/count16. The accumulation/selected registration is a supplied
bootstrap boundary. The shared `tmp_clear_render.load_tmp_files` owner executes
original constructors and full lazy RawFile/TMP IO; no relocated TMP or decoded
color is prepared by the host. The tile/placement callers use that same owner,
retaining their original construction/load order and variant linking. The
physical subtile0 gives slope0/Land1 and radar pairs Left102,78,48/Right101,77,48.
Original Scenario theater2 selects brightness bits3F800000 from the executable.

The selected sparse Cell starts at authored GoldSt123,33/tile141/sub0/level5.
Current density1 is a supplied control. Original Rules constructor and layered
CliffBack/land reads execute. Physical neighbor tile identities/levels cover
native LAT's four cardinal reads and later Recalc height reads. Authored neighbor
overlay/data fields are recorded separately: their supplied **live overlays are
empty**, isolating this consumer from natural patch reseeding. Unused neighbor
slope0 fields do not claim native neighbor TMP production. Full original Recalc
runs before the initial color and again inside terminal reduction; native LAT
retains tile141 and normal selected ground becomes Land1. This establishes the
selected final tile/color at the declared sparse boundary, not a native whole
GoldSt map/queue bootstrap. Normal production must independently match current
identity before consuming the physical ground golden.

The actual layered TIB01 has Tiberium1 and resolved CellAnim=null. Original
`47C060` returns `[170,170,130]` into both raw halves for density1 and density0.
After full removal it reads physical TMP Left, calls original brightness661190,
shifts and duplicates `[51,39,24]`. No SHP average, resource Color scheme, guessed
palette or host channel arithmetic creates these expected colors.

`radar.depletion.input` explicitly supplies raw173x113, columnBase10/rowBase100
and black backing; these are component geometry, not computed native GoldSt
radar bounds. Original `6551C0` populates the real spare vector, `655250` calls
`47C060`, writes the raw pair at100,56/101,56 and calls complete `6547C0`.
Original BSurface allocation/lock/unlock and RGB565 stores execute. The native
initial/full-removal generated dirty rectangle is80,45,3,2, and vector count is
zero after processing. Partial1→0 leaves the ore/raw/generated surface unchanged
with no queued cell; full removal queues one entry, replaces the ground and
clears it; unchanged-empty processing queues nothing and retains pixels.
`rows[].raw_pixels` and `output.runs` losslessly retain nonblack native stores;
full raw/generated hashes also cover every unchanged pixel. No Python raster or
native-equivalent output calculation is used.

Each row's `rebuild_output` comes from original6547C0 over the full raw rectangle
on that exact post-dirty raw buffer. The original incremental surface bytes are
restored afterward; assertions retain unchanged BSurface/header/radar-cache and
raw-buffer state. Thus `output`, dirty history and later incremental rows are
unchanged. This native full-generation result is the honest comparison target
for the current Rust full-rebuild strategy, without fitting sampling weights.
Residual: original incremental density1 differs from the native-exact Rust full
rebuild at five pixels in3x2/one fringe row (max expanded channels≈25/21/17);
exact incremental drawing strategy remains outside this bounded comparison.

`radar.weighted_controls` executes complete original6547C0 on the existing
terrain-tests input pattern (wrapping channels17x+3y,5x+29y,41x+7y). The supplied
raw sizes173x113 and137x149 produce native140x91 and99x108. Hashes cover all
little-endian RGB565 stores; retaining full pattern/output runs would be bulk.
These expected hashes replace the affected handwritten reference sampler rather
than introducing another sampling implementation. Native FCW0E7F is recorded.

The selected Cell color/dirty/raster calls pass no gameplay timer or Detach.
With the explicit empty-overlay neighbors, native reduction admits no spread
entry or Scenario RNG draw; retained full-state hashes are unchanged. That does
not certify natural GoldSt patch neighbors as RNG-free. Normal harvester timers,
resource growth/reseed state and queues remain with harvest_field/ore_queue;
whole online-radar admission, fog/objects/events, final GPU/HUD composition and
upload/acknowledgement remain separate production consumers to validate. The
Tactical dirty call remains a declared sink; no Radar color/dirty/generated
store, Recalc or reduction return is substituted in this new payload.
