# LightConvert row selection and active MMX RGB565 palette conversion

Read-only native investigation, 2026-09-09. This resolves the row-selection gap
recorded in `src/render/palette_light.rs`, and finds two additional differences:
normal YR x87 arithmetic produces base scale **65535**, not 65536, for input 1000;
the active MMX builder discards another four fractional scale bits. ColorScheme
palettes also exempt indices **240..254** from ordinary RGB lighting above a low
brightness threshold. These are native behavior findings, not a claim that current
Rust or any full scene now matches.

## Evidence and reproduction

Target: Ghidra `testProsjekt:/gamemd.exe`, image base `0x00400000`, original file
SHA256 `1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
The file at `C:/Users/enok/Documents/Command and Conquer Red Alert II/gamemd.exe`
was hashed before executing its bytes. No Ghidra labels, bytes, function boundaries,
native process state, or Rust files were changed by this investigation.

Portable reproducer (requires an independently supplied copy of the matching
retail executable and Unicorn 2.1.4):

```powershell
python tools/palette_oracle/oracle.py --exe "C:/retail/gamemd.exe" --output .local/palette-oracle
python tools/palette_oracle/check.py --generated .local/palette-oracle
```

The [oracle](../../tools/palette_oracle/oracle.py) parses PE sections itself and
executes original x86 bytes in isolated Unicorn memory. The checked-in
[fixture provenance](../../tools/palette_oracle/fixtures/provenance.json) records
hashes of native-generated outputs over synthetic palette bytes. No retail
art or executable bytes are distributed. Original investigation disassemblies
remain local provenance in the coordinator's `ra2-rust-game-rendering-goal`
checkout under `.local/rendering-parity/palette`; addresses below allow fresh
read-only disassembly without that directory.

The coordinator independently read the retail fixture process at
`2026-09-09T10:12:04.786465Z`, PID 12636, after verifying that process's executable
path/hash. The local runtime record is
`.local/rendering-parity/flat8-v2/native/render-state.json` in the coordinator's
`ra2-rust-game-rendering-goal` checkout. Its essential fields are preserved here
and in fixture provenance:

| Address | Observed value | Meaning established from native branches |
|---|---:|---|
| `0x0084E862`, byte | 1 | MMX branch selected at `0x007DE20B..212` |
| `0x0084E860`, byte | 1 | CMOV available, but bypassed because MMX was selected |
| `0x00829D20`, dword | 2 | LightConvert builder dispatches to RGB565 `0x007DE200` |
| `0x008205D0`, dword | 2 | DSurface recognizes RGB565 |
| `0x00822D80`, dword | `0x0E7F` | cached x87 53-bit precision, round toward zero |

This identifies the active branch for the coordinator's `flat8-v2` retail session.
The corresponding PCX was captured at `10:10:50Z`, 800x600, SHA256
`c81dd77a62725a5318a32699d5ba5068dba61840120cdc4bd52b494c310f35e7`.
The process read and image are close observations, not an atomic state/frame capture.
Other native sessions must record their own dispatch state.

## Exact brightness-to-row selection

For ordinary supported nonnegative brightness arithmetic (including signed-cell
brightness and normal draw values), the drawing leaf first computes:

```text
q = min((max(brightness, 0) * 261) >> 11, 254)
row = min((A * q * (N - 1)) / 32258, N - 1)
palette_word = palette_rows[(row << 8) | source_index]
```

All divisions truncate. `A` is the current pixel's unsigned A-buffer value; `N`
is the selected Convert's row count. `32258 = 127 * 254`. Native code is 32-bit
and can wrap for corrupt/extreme brightness values; the formula above is not a
promise to saturate arbitrary `i32::MAX` inputs before multiplication.

`0x00420140` owns a cached 256x256 table for each `N`. Its loop
`0x00420196..0x004201D3` computes the row from table-index low/high bytes and
stores `row << 8` as a word. The blitter selects the table's `q` row and indexes
it with `A`, then ORs in a palette index. The generated table covers all low/high
byte pairs, even though normal brightness clamps `q` to 254.

Clear A-buffer value is 127: tactical full redraw calls
`CircBuf__FillAll(g_ABuffer, 0x7F)` at `0x006D3F9F..0x006D3FA7`; dirty rectangles
also reset to 127. See the directly sourced
[A-buffer report](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/building-selection-brackets/TACTICAL_ABUFFER_SHROUD_VALUES_FOR_BRACKET_LINES_GHIDRA_REPORT.md).

At clear A=127:

| Brightness | q | Row with N=53 | Row with N=27 |
|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 |
| 992 | 126 | 25 | 12 |
| 999 | 127 | 26 | 13 |
| 1000 | 127 | 26 | 13 |
| 1200 | 152 | 31 | 15 |
| 1999 | 254 | 52 | 26 |
| 2000 | 254 | 52 | 26 |

Thus the previous comment's "row 31 or 32" for brightness 1200 is resolved:
**row 31 for a 53-row Convert with clear A-buffer**. This is two quantizations,
first brightness into q and then the A/q pair into a palette row, not rounding a
continuous combined RGB tint.

## Active consumers and selected leaves

| Consumer | Native route and evidence | Coverage |
|---|---|---|
| TMP terrain | `CellOverlay_TileDraw 0x00480350` passes cell `+0x34` Convert and signed `+0x10C` brightness to `TMP_TileBlitter 0x00547CF0`. At `0x00547Dxx`, the latter gets the table for Convert `+0x16C`, computes q, then inner loops use A/q, source index, and Convert `+0x170`. | Direct decompile/body read, including diamond pixel lookup. Whole TMP routine was not emulated. |
| Ordinary SHP without depth/translucency variant bits | `CC_Draw_Shape 0x004AED70` selects standard/extended families based on the SHP frame flag. Standard selector `0x00490B90` with `0x800` and without `0x10/0x3000/0x4000/6` picks `+0x6C`; init `0x0048F4BF..0x0048F4D3` sets vtable `0x007E57E0`; method +4 is `0x00493DF0`. | Original leaf executed; A/q/table rule confirmed. High centering bits do not change this selector branch. |
| SHP remap/intensity variant | Selector `+0x70`, vtable populated by `0x0048EBF0`, method `0x00493F30`; performs a source remap byte lookup before packed palette lookup. | Original leaf executed. This slot alone ignores Z; do not generalize to other slots. |
| Uncached ordinary voxel | `TechnoClass::Draw 0x00706640` starts flags at `0x2000`, adds `0x800`; `TechnoClass::Render 0x00706ED0` clears `0x10` before selector. Ordinary `0x2800` picks `+0x98`; init gives vtable `0x007E56F0`, method +4 `0x00494B60`. | Original leaf executed. Raw source byte 0 skips; visible pixels use A/q/table. Z-tested, no Z write in this inspected leaf. |
| Cached ordinary voxel | `VXL_CacheBlit 0x00707480` clears `0x10`; extended selector `0x00490E50` picks `+0x138` for `0x2800`; init gives vtable `0x007E5420`, method +4 `0x00497FD0`. | Original RLE leaf executed with nonzero runs, no left skip, zero per-pixel depth offsets. Same A/q/table rule. All RLE/clipping/depth cases were not certified. |

The two voxel methods are not currently defined Ghidra functions. Their code was
decoded directly from original executable bytes with Capstone 5.0.7, rather than
repairing the shared database. Their vtable entries were read independently.
Techno/SHP special draw flags, second shapes and effects choose other families;
the table above is not an assertion that every sprite uses the same leaf.

## Palette generation, including x87 and MMX rounding

`LightConvertClass` constructor `0x00555DA0` calls table builder `0x00556090`.
Fields: `+0x16C=N`, `+0x170=packed rows`, `+0x188=source RGB-byte palette`,
`+0x190=per-index light mask`, `+0x198/+0x19C/+0x1A0=current RGB`, and
`+0x1A4/+0x1A8/+0x1AC=alternate RGB`. Builder `param_5` selects which RGB triple
is stored/used; value -1 for the red parameter requests reusing the corresponding
stored triple. Ordinary explicit RGB values are clamped to 0..2000 here.

For each component, `0x00556192..0x005561F8` uses integer LEA/SHL to multiply
the component by 1000, loads that integer into x87, multiplies by the binary64
at `0x007ED0B0` (bits `0x3FB0C6F7A0B5ED8D`, displayed as 0.065536), then calls
original `Math__ftol 0x007C5F00`. That helper forces the cached word if necessary,
executes `FISTP qword`, returns the integer, and retains the forced control word.
Under normal 0x0E7F, **base_scale16(1000)=65535** and
**base_scale16(2000)=131071**. Replacing these operations with default Rust
round-to-nearest `f64` arithmetic is not equivalent.

For nonnegative integer components 0..2000, exact rational multiplication by the
stored binary64 followed by floor agrees with every native-executed component:

```text
base = floor(component * 1000 * exact_binary64_at_0x007ED0B0)
row_scale = (base * 2 * row) / (N - 1)       # N > 1
```

The original builder was executed for every component 0..2000 to check this
integer interpretation. Samples: 512 -> 33554, 768 -> 50331, 992 -> 65011,
999 -> 65470, 1000 -> 65535, 1200 -> 78643, 1999 -> 131006, 2000 -> 131071.
N=1 is a separate builder branch using base directly, outside the scene tests below.

RGB565 format dispatch 2 calls `0x007DE200`; source index zero is written as
packed zero. For ordinary light-mask entries:

```text
scalar or CMOV: lit_byte = min((palette_byte * row_scale) >> 16, 255)
MMX:           lit_byte = min((palette_byte * (row_scale >> 4)) >> 12, 255)
packed565 = ((red >> 3) << 11) | ((green >> 2) << 5) | (blue >> 3)
```

The MMX interpretation applies to the established valid scale range. At
`0x007DE27E..0x007DE28E`, it shifts the four scales right four and packs signed
words; the loop shifts unpacked palette channels into `byte << 4`, uses `PMULHW`,
then `PACKUSWB` and RGB565 masks. This additional precision loss can change a
packed word. With the synthetic all-channel-byte palette in the oracle, the
53-row neutral table differs from scalar at 5 of 13,568 entries; RGB
(992,768,512) differs at 89; the 27-row (512,640,768) table differs at 36.
Scalar and CMOV agree on all tested tables. These are bounded comparisons, not
an exhaustive proof over every possible palette/profile.

## ColorScheme special indices and row-count ownership

Cell Convert cache `0x00544E70` chooses N=27 when the normalized RGB key sums
to less than 2000; otherwise N=53. See the established
[cache contract](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAP_LIGHTCONVERT_CACHE_00483E30_00544E70_GHIDRA_REPORT.md).
Its default mask `0x00829C20` is 256 bytes of 1 in the retail executable.

Ordinary house palettes have their own Convert. `Init_Color_Schemes_INI
0x0066D3A0` creates variants with N=1 and N=53 for each color. ColorScheme
constructor `0x0068C710` passes initial RGB (1000,1000,1000) and mask
`0x0083E1AC` through `ColorScheme__BuildRampPalette 0x0068C3B0` into LightConvert.
The latter writes house RGB shades into indices16..31 before constructing the
Convert. Rebuild `0x0068C860` uses the same mask with replacement RGB parameters.
`TechnoClass` remap selection `0x00705D70` obtains a selected scheme's Convert
at scheme `+0x30C`, with type/owner/captor override branches.

The mask bytes are 0 at indices240..254 inclusive and 1 everywhere else,
including 255. No writes to this static mask were found; xrefs are the two
constructor/rebuild callers. For a zero mask byte, the RGB component scales are
replaced by one neutral scale:

```text
D = min(max((N * 30) / 200 - 1, 0), (N - 1) >> 1)
neutral_scale = (row * 65536) / D  when row <= D
neutral_scale = 65536             when row > D
```

For the scoped normal N=53, D=6; N=27 would give D=3. Thus indices240..254
reach full neutral brightness at row6 in house palettes and remain there even
when ordinary RGB profile values tint/dim the rest of the palette. They still
become dark in the bottom rows, so simply treating them as always unlit would
also be wrong. N=1 uses neutral65536 directly and avoids the division.

**Row count and special-index mask belong to the selected palette Convert.**
Inferring both from a cell RGB key is wrong for ordinary player ColorSchemes;
those use N=53 even when a cell Convert would use N=27. Exact selection/update
of every custom palette, animation palette, and weather transition was not
rederived by this bounded investigation.

## Executed coverage and implementation implications

The reproducible oracle passes:

- all 65,536 intensity table entries at each of N=53 and N=27;
- base scale conversion for every integer component0..2000;
- 21 complete native palette tables: 4 profiles across scalar/CMOV/MMX, plus
  2 ColorScheme-mask profiles across all 3 branches, one N=1 ColorScheme
  table, and N=1/N=53 plain Convert tables;
- 208 brightness/A/N cases through each of 4 original SHP/VXL scanline methods,
  832 scanline executions total; each compares every source index1..255;
- all entries in the 6 ColorScheme-mask tables against the decoded scalar/MMX
  formulas, including both ordinary and special-index branches.

The scene's active MMX/RGB565/0E7F mode is independently observed. The native oracle alone does not supply Rust regression or rendered GPU
parity. Separate implementation checks are recorded below.

The implementation contract is to preserve the separate selected palette/profile,
row count, integer brightness, final source palette index and per-pixel A value
until color resolution. Palette row storage can use Rust-native/GPU-native
ownership; reproducing C++ allocation or refcount layout is unnecessary. Existing
`CellLightGrid` already stores more of this information than its compatibility
RGB tint accessors return.

The base commit c68dd4d2 contained three assumptions requiring correction:

1. `palette_light.rs` described neutral65536 and continuous tint; its neutral
   identity test is a Rust-only assertion contradicted by the native0E7F path.
2. The shared shader tint combined RGB profile and brightness, losing row-count
   and special-index ownership. Baking SHP RGB without preserving source-index
   metadata prevents applying the native per-index mask later.
3. Native shroud/A contributes **before** palette lookup and packed output.
   A final scene-wide multiply cannot generally reproduce row boundaries or
   per-index exceptions, and translucency combines already packed source words.

Acceptance for a repair: feed native-generated palette/index/brightness/A fixtures
through actual production GPU paths; compare packed/display output, including
brightness boundaries, indices16..31 and240..255, colored profiles, N27 cell
versus N53 house palettes, clear/shroud edges, and scalar/MMX mode policy. Then
compare identical retail scenes. The later bounded Neutral approval below
does not establish full composition, custom palette selection, house overrides
or translucent-effect parity.


## Ordinary ConvertClass is a separate palette owner

Fresh original-body/caller inspection during integration found that global
animation and ore converts are **not LightConvertClass**. `Init_Game` at
`0x0052BE26..3B` creates the global `0x0087F6B8` convert with N=53;
`0x0052BEF5..0x0052BF0D` does the same for ANIM `0x0087F6C0`.
`ConvertClass::Constructor 0x0048E740` calls `0x004BBB00` on 16-bit surfaces.
Its rows use `scale=131072*k/(N-1)` (N=1 uses 65536), then each channel
`min(byte*scale >>16,255)` and surface loss/shift packing. There is no MMX
branch and no LightConvert x87 scale. Source index zero is not forced zero in
this constructor; SHP blitters still skip source byte zero. Original-byte
emulation now covers all entries of N=1/N=53 plain tables as well.

Normal terrain objects (`0x0071C2B2..304`) pass cell Convert and common scalar.
`SpawnsTiberium` at `0x0071C286..2B0` selects global `0x0087F6BC` and top scalar.
The latter aliases global `0x0087F6B4` (`0x0052C1EA`), built with N=53.
Ordinary ANIM (`0x00423200`, branch `0x00423280..354`) uses global ANIM and
**top** cell scalar unless UseNormalLight is set. AltPalette selects the first
ColorScheme Convert, whose N=1 must remain distinct from a player's N=53 scheme.
The cell-drawer animation branch instead passes cell Convert/common scalar.

Building `TerrainPalette` is an art-image boolean: `0x004612B4..CB` reads the
string at `0x0081A700` using art INI `0x00887180`; default false is written at
`0x0045E20C`. `TechnoClass_DrawSHP 0x00705EC7..0x00705F52` selects cell Convert
and common scalar when it is set. Otherwise ordinary bodies reach selected
ColorScheme through vtable+0x1E4 at `0x00705F81`.

The distinct normal ColorScheme profile is confirmed by `Init_Theater` at
`0x00534D77`, rebuilding schemes with RGB=(1000,1000,1000). VXL at
`0x00707194..19A` stores the selected scheme Convert in ESI, and at
`0x0070720E..10` passes it to the blitter selector. It does not pass cell hue.
Global weather changes at `0x0053C280` call `0x0053AD00` with selected scenario
percent RGB multiplied by ten and alternate=1; restoration sends (-1,-1,-1)
and alternate=0 to reuse each Convert's own stored normal triple. This does
not justify reconstructing every ColorScheme from the current cell key.

## Integration evidence and ownership boundaries

### Cell common brightness is the normalization helper's scalar

Original `0x00483EB0..0x00483EDA` prepares output pointers in order
scale, additive, top, common, bottom, R, G, B. In `0x00484180`, the common
pointer is stack+0x4C: `0x004845B0` loads it into EDI, `0x004845C2` initializes
it from capped top, and `0x004845CB` passes that pointer in EDX to
`0x005558E0`. The helper's `0x00555A8F..9C` scales this common value, not raw
additive intensity. The near-black path sets common to zero while returning
identity scale and neutral RGB. Top remains unnormalized; bottom scales
separately at `0x004845D4..EB`. Original stores `0x00483F7A..0x00483FD8`
publish additive to Cell+0x108, top to +0x10A and common to +0x10C.

This corrects an older Rust caller error in `build_cell_light_from_raw`:
it passed raw additive intensity to the generic normalizer and retained
`common=top`. The helper now names its input/output common scalar, the grid
retains source additive independently, and common receives the normalized
value. Neutral lighting is unaffected. Colored map profiles and colored lamps
exercise this correction frequently; simply multiplying the returned scale
later would miss the near-black reset.

The portable original-byte oracle now includes 6,014 finalization cases,
every clamped maximum 0..2000 in each dominant-channel branch, detail0..2,
scalar caps and negative/near-black cases. It executes the original pointer
setup, finalization with the complete helper, actual Cell stores and native
`0x00555AC0` key quantization. Prepared post-gather inputs are explicit;
gathering, Convert allocation and scene composition are excluded. Examples:

| Raw RGB | Additive | Top input | Native common | Native key, detail2 |
|---|---:|---:|---:|---|
| 1050,1050,1010 | 200 | 1150 | 1207 | 992,992,960 |
| 800,600,500 | 200 | 1000 | 799 | 992,736,608 |
| 2000,1000,1500 | 600 | 900 | 1800 | 992,480,736 |
| 0,0,0 | 200 | 1000 | 0 | 1000,1000,1000 |
| 240,480,800 | 0 | 1000 | 799 | 288,576,992 |

The real map-grid producer test uses Ambient1/Ground0/Level0 and RGB
(.24,.48,.80): cell key sum1856 selects N27 with common799; selected unit
ColorScheme retains N53, neutral profile, and top1000 plus ExtraUnitLight200.
Ground alone changes both top/common equally in a neutral profile.

### Production integration

The implementation work is isolated on `feature/retail-palette-conversion`, now
based on approved voxel-lighting commit `a1a68ad776282dae81b8c1d7879b0da1e0f43269`.
`render::palette_light::PaletteLight` carries four integer words: converter RGB
scales and row count, per-palette mask and plain/LightConvert mode, and signed
brightness. It does not combine cell hue with a selected ColorScheme profile.
The shared WGSL resolves packed RGB565 then uses the existing
`native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION` codebooks. PCX output
remains separately owned by `screenshot.rs`: display-codebook bytes are masked
back to native stored RGB565 channel codes. There is no screenshot calibration.

Normal `SpriteInstance` gains 16 bytes (108 to 124), one vertex attribute, with
no extra per-instance bind group. SHP atlas pages retain one additional source
index byte per pixel in R8Uint textures. The same source index remains available
after house RGB baking, including 240..254. Explicit `ShpPaletteContext` keys
separate global ANIM, selected ColorScheme, and cell Convert use of the same
animation/frame/house. Registration, runtime lookup and refresh coverage select
the same context. A Legacy entity entry cannot satisfy a SelectedScheme
animation preload, and an already-loaded SelectedScheme avoids repeated uploads.
Legacy effect keys remain available for producers whose complete native palette
route has not been migrated; explicit contexts can require additional atlas
images when their source RGB differs.

The shader converts only opaque pixels without effect flags or effect-color
modulation. Partly transparent pixels and cloak/effect paths retain their
existing shader conversion and composition; changing a global animation's source
palette can still change its translucent input RGB. Native A-buffer lookup is implemented
for clear A=127; the later Rust shroud curtain remains a discrepancy whenever
native non-clear A changes the selected palette row.

Fresh independent critic inspection established further active ownership:

- `BuildingClass::DrawBody` at `0x0043D812..0x0043D85F` reads signed top
  `Cell+0x10A`, adds signed `BuildingType+0x1548` (`ExtraLight`), and passes this
  brightness to `0x00705E00`. Buildup at `0x0043D644` omits ExtraLight; the bib
  repeats top+ExtraLight at `0x0043D892..0x0043D8AF`. The old ArtEntry comment
  calling ExtraLight a Z adjustment was contradicted by these original pushes.
- Building VXL dispatch `0x0043CFE9` reaches `0x0043DA80` through vtable+0x4E4.
  Its submissions read top `Cell+0x10A` directly (e.g. `0x0043E2B4..0x0043E2FF`,
  `0x0043E324..0x0043E36F`, `0x0043E3BF..0x0043E40A`) and call `0x00706640`.
  The selected scheme at `0x00707194` is independent of SHP TerrainPalette.
  No ExtraLight read occurs in this function. The turret producer therefore
  retains selected scheme/top independently of the SHP body descriptor.
- OREGATH in `UnitClass::DrawExtras 0x0073CEC0` selects global ANIM Convert
  `0x0087F6C0` at `0x0073D276` and calls `0x004AED70` at `0x0073D283`.
  Ordinary brightness reads top at `0x0073D0B4` and adds Rules+0x17D4 at
  `0x0073D0C9..CF`. The bridge/neighbor branch `0x0073CFC9..0x0073D076`
  adds a selected scenario field times four; the qualifying Cell+0x140 bit
  0x10000 branch `0x0073D089..BB` subtracts 500. Those special scalar branches
  remain unresolved in Rust; the ordinary producer now uses plain N53 ANIM
  conversion instead of the unit ColorScheme mask/profile.
- Attached-animation refresh `0x0043F9A6..0x0043FA68` selects vtable+0x1E4
  (`0x00705D70`) and top scalar, applies vtable+0x464 (`0x00456F80`) and an
  optional `0x0070E360` effect adjustment, stores a u16 brightness, then writes
  Anim+D4/FC only when AnimType+35C (`ShouldUseCellDrawer`) is set. Ordinary
  unaffected buildings keep the input brightness; the +F0 bit2 branch in
  `0x00456F80` adds 500 at brightness<=1500 and subtracts 500 above it. The
  effect-controlled lifetime remains outside this ordinary opaque increment.
- Smudges are dispatched inside `CellOverlay_TileDraw 0x00480350`, after TMP:
  `0x004804CE..0x004804F5` calls SmudgeType vtable+A0, whose original word at
  `0x007F35C8` is `0x006B55F0`. That leaf reads common scalar at `0x006B567B`
  and cell Convert at `0x006B56AD`. Rust's footprint-origin deduplication does
  not preserve per-cell brightness/clipping/order for multi-cell smudges.
  Smudge composition is therefore explicitly deferred with its native owner.

Correction to a misleading global label: `0x0087F6B8` is not proven to be a
UNIT palette. `Init_Game 0x0052BC50` loads original string `0x008260C8`,
`TEMPERAT.PAL`, into `0x00885780`, then `0x0052BE2A..0x0052BE36` constructs the
plain N=53 Convert from those bytes. `Init_Theater` replaces that palette with
`%s.PAL`, while separate `UNIT%s.PAL` populates `0x00886380` for ColorSchemes.
The ordinary Convert has already baked its table and no retained source pointer.
No global B8 rebuild was found in the inspected theater body. Thus existing
ore `tiberium_palette` is consistent with Temperate; cross-theater table lifetime
is unresolved. `0x0087F6BC` aliases the N=53 Convert baked from `UNITSNO.PAL`
(original string `0x008260D8`, `0x0052BFDA..0x0052C075`, alias `0x0052C1EA`).
SpawnsTiberium's current theater unit palette is a source-identity residual.

Remaining discrepancies within the broader rendering goal include non-clear
A/shroud and packed translucent blending; wall selected-player remap; smudge
per-cell composition; aircraft altitude brightness; explicit animation Convert
brightness lifecycle; effect-controlled building brightness; RGB-trigger
ColorScheme rebuild history; ordinary Convert source lifetime across theaters;
and legacy producers/custom palette choices not yet traced end-to-end. These
boundaries prevent calling the entire opaque renderer or palette system closed.

Validation of the palette candidate after the common-brightness correction:

- Original-byte oracle: 21 complete palette tables, 832 scanline samples,
  all 2,001 clamped base scales and 6,014 cell finalizations; 13 tracked fixture
  hashes matched a fresh run.
- Full library suite: `test result: ok. 8556 passed; 0 failed; 94 ignored;
  0 measured; 0 filtered out; finished in 24.62s`. This includes all 6,014
  original Cell finalizations, real map-grid palette producers, building art
  overrides and context-specific atlas refresh coverage.
- Explicit palette GPU filter: `test result: ok. 5 passed; 0 failed; 0 ignored;
  0 measured; 8645 filtered out; finished in 10.00s`. Three tests exercise GPU
  pixels; two existing shell palette-decode checks also match this filter.
  `native_palette_tables_match_all_ordinary_shader_pixels` uses production
  Batch, TMP, SHP read/write and VXL vertex/fragment entrypoints, all nine native
  fixture palettes and all source indices/rows. The indexed voxel check verifies
  remap, holes and read-only depth. The SHP loader test follows the production
  decoder and page-copy helper through actual shaders, with test-owned texture
  and bind-group creation; it does not exercise the full BatchRenderer upload
  owner. Adapter: AMD Radeon integrated GPU, Vulkan, driver 23.19.24.07.
- `cargo clippy -p vera20k --lib`: exit 0, 1,150 warnings, 56.26 seconds.
  System Map check: zero errors across three files. Diff whitespace check passed.
- Independent critic cleared the source corrections and separately regenerated
  all 6,014 Cell cases with exact fixture SHA256
  `6f884fd21713515a87a5bc3680c3128f41d6d7db326550874a6da2b0f7ac5eb0`.
  Final visual approval remains conditional on the combined release captures.

The release will combine this palette candidate with the separately reviewed
voxel raster correction. These source/GPU checks do not stand in for that
integrated candidate's checks, normal runtime upload/draw route, or frame-cost
measurements. The user will take the game captures. A task-local GPU timestamp
probe is prepared for the combined release; results are not asserted here.

The parent comparison independently identified all 46 native clear-tile
variants using original isotem.pal and the native N53 neutral MMX table. All
900 pixels of each native diamond matched row26 exactly. Eight matching native
and pre-palette VERA variants isolate 7,200 identical source pixels: none had
identical RGB, green differed by +4 throughout, and red/blue by zero or +8.
This demonstrates the pre-fix packed-output discrepancy independently of random
clear-tile selection. Native comparison table SHA256:
`bf0d68b4865df9bc5ee172e23d45f797c4b38dd0c009e18742da221c358971e0`.
The original PCX and local identification scripts are retained by the parent in
its `flat8-v2` / `terrain-proof` audit directories; no retail bytes are shipped
with this report. The later integrated candidate
`2adc7854d40e17a267b80717f605708278062c52` passed the full library suite
(8,581 passed, 0 failed, 119 ignored) and clippy (exit 0). Its user-captured
800 x 600 Neutral scene received independent visual approval, retaining the
sampled terrain, vehicle-body and TREE matches. The VERA PCX SHA256 is
`c9e21d4003fd7677d0a31746991029c3d9d45070d0811203d48f96a7207966ea`;
the local `capture-scenes-v4/01-neutral` receipt records the candidate and
original retail capture identities. This supersedes the fresh-capture gate
for that scene only; the composition and producer exclusions above remain.


## Ground/Level parser correction

The 2026-09-09 native runtime observation exposed a required parser prerequisite:
explicit map `Ground`/`Level` values use a **1000** scale. Earlier reports inferred
250 from editor-authored `.032` and the reset value8; that inference was false.
Original `0068A92E` and `0068A968` multiply the `ReadDouble` result by the double
at `007E4658`, whose bytes `0000000000408f40` encode1000.0. Both add the double
`.01` at `007E3808`, then execute `Math__ftol` at `007C5F00`. IonGround/Level
repeat the same instructions at `0068AA8A`/`0068AAC4`.

Missing keys are different from explicitly authored editor values. Original
reset instructions `006838F7` and `00683901` store Ground50 and Level8. The
missing-key path at `0068A905..0068A91F` / `0068A93F..0068A959` multiplies those
integers by float `007F0E78` (bytes `6f12833a`, approximately.001) before supplying
the double default to ReadDouble. Executed roundtrips preserve50/8. Therefore
internal defaults remain50/8, while public ratio defaults are.05/.008. Explicit
`.20/.032` produces200/32; negative`-.032` produces-31 because the positive.01
bias precedes truncation towardzero.

The portable [oracle](../../tools/palette_oracle/oracle.py) now executes the
original reset, two default inverses, and all four conversion siblings for16
prepared float-scan/widened inputs. [Ground/Level goldens](../../tools/palette_oracle/fixtures/ground-level.json)
retain exact results plus seven original post-gather Cell finalizations. The
boundary supplies ReadDouble returns and post-gather values; it does not execute
CCINI token scanning or the map/light-source gather. Production Rust tests
`native_ground_level_ini_quantization_matches_all_four_original_sites` and
`native_ground_level_defaults_and_authored_values_reach_cell_grid` carry the
fixture values through both parser paths and grid construction. They distinguish
no section, empty section, either missing key, authored.20/.032, Ground.40/Level0,
and the actual neutral fixture's Ground0/Level.032.

With ordinary Ambient/RGB1 and no lamps, native post-gather finalizations are:

| Authored keys | Height | Top/common | Bottom |
|---|---:|---:|---:|
| Missing Ground/Level |0|950|982|
| Missing Ground/Level |4|982|1014|
| Ground.20/Level.032 |0|800|928|
| Ground.20/Level.032 |4|928|1056|
| Ground.40/Level0 |0|600|600|
| Ground0/Level.032 |0|1000|1128|

The corrected Ground.40 fixture selects neutral cell N53row15 (brightness600),
and an ordinary unit with rules ExtraUnitLight200 selects house N53row20
(brightness800). The prepared tinted fixture Ground0/Level0 is unchanged:
RGB.24/.48/.80 gives cell key288/576/992, N27, common799/row10; ordinary selected
house N53 uses top1000 plus ExtraUnitLight200/row31. Selected-house rebuild
history outside the inspected ordinary path remains a residual.

A read-only live snapshot of the user-visible neutral scene corroborates the
field and table ownership. At12:16:56UTC on2026-09-09, PID37792 matched the pinned
original executable hash. Two requested cells (48,48) and (53,51)
had height0, scale65536, additive0, RGB1000, top/common1000 and bottom1128;
Scenario+3540=0 and+3544=32. Their actual N53 table hash was
`bf0d68b4865df9bc5ee172e23d45f797c4b38dd0c009e18742da221c358971e0`, equal to the
original neutral isotem oracle table used for the41,400 retail diamond pixels.
Selected scheme21of42 had N53, base/alternateRGB1000, alternate-active0, and table
hash`9754182c15aae9f3c874a3b4b144f46e97963c1c6554eb6da4f930533da1b7d7`.
This was a selected-field read, not an atomic frame or colored-scene proof.

The parent's retained local provenance is
`flat8-v2/native/runtime-palette-user-neutral-with-scenario.json`, SHA256
`001916483bbfb74398774ea17357d5729913c2025c3d420968a859171e724837`.
No retail process data files are shipped. The earlier41-file handoff and checks
above remain historical; this prerequisite and approved voxel commit2b188398
require the subsequent combined candidate validation below.


The user subsequently captured the prepared static tinted scene. The original
800x600 PCX has SHA256
`186277e690ce6cd9494bf282968b5cb1b624d3b18f8e8ddf3fd40448c8ac4b8f`.
All 46 identified clear diamonds (41,400 source pixels) exactly matched the
original N27 row10 colors, with no unresolved tile identities. The accompanying
12:33:43UTC read showed Ground/Level0; all four cells had top1000,
common/bottom799 and scale52428. Stored unquantized Cell RGB was300/600/1000;
the active Convert had N27, quantized baseRGB288/576/992, alternate-active0,
and complete table SHA256
`5cedbecd37900c17a270ad5f44ae1855cca8f3c0ffb0d48a620104371ba870e9`.
The selected house remained N53/baseRGB1000/alternate-active0, and its entire
table hash
`9754182c15aae9f3c874a3b4b144f46e97963c1c6554eb6da4f930533da1b7d7`
was unchanged from the neutral scene. These observations
establish the separated cell/house profile ownership and expected common
brightness for this static tinted scene; they do not cover trigger rebuilds.

Local parent provenance lives under `capture-scenes-v1/04-tinted/retail`:
`native-comparison.json` SHA256
`ce648c19a8481df9093f027fa7ad54e8c0d37e28eac709ceaaed4eb146a74cbd`,
and `runtime-palette.json` SHA256
`02f3b4b43b7626c7b83a436522eadcb8cfd46af6a88c54e84b6c77e074e5e3c5`.
The comparison's map hash is
`17620278673553a264b47fa633e70920f14a0f0654974498e94c966f1f642680`.
Candidate-to-retail equivalence still requires the combined VERA capture.


Combined candidate validation after the Ground/Level correction, based on the
approved voxel commit `2b18839831c234917cde6597d8d8ea016f8ad9aa`:

- Full library: `test result: ok. 8556 passed; 0 failed; 98 ignored;
  0 measured; 0 filtered out; finished in 26.80s`. Both new original-derived
  Ground/Level parser/grid tests passed in this run.
- Explicit palette GPU filter: `test result: ok. 5 passed; 0 failed; 0 ignored;
  0 measured; 8649 filtered out; finished in 12.10s`. This is the same three
  actual shader/readback tests plus two shell decode tests described above.
- Explicit voxel GPU: `test result: ok. 1 passed; 0 failed; 0 ignored;
  0 measured; 8653 filtered out; finished in 0.73s`. The production native
  voxel-buffer readback matched every cropped palette byte in 32 original
  executable fixture cases. It does not certify multipart caller composition.
- Clippy library: exit0, 1,144 warnings, 1m09s. System Map: zero errors across
  three files. Diff whitespace check passed. Original palette/Cell oracle:
  all14 tracked fixture hashes matched fresh original instruction execution,
  including the added Ground/Level records.
- Independent critic cleared the corrected source/native default boundary;
  its final visual decision remains dependent on combined production captures.

These checks cover the combined source. A sealed release, normal runtime atlas
upload/draw comparison and performance measurements are still required before
claiming demonstrated candidate scene matches. Broader residuals listed above
remain open; this increment does not close the full rendering goal.
