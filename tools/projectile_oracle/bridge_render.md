# Ordinary Cannon / 120MM bridge rendering

This corpus bounds the common `MTNK → 105mm → Cannon → Image=120MM` SHP
presentation route. All executions use original `gamemd.exe` through the shared
runner and Unicorn 2.1.4.
The separate [input corpus](bridge_render_inputs.py) establishes the selected
original constructors, readers, image and palette prerequisites. `120MM` is the
projectile's image identifier; it is not the `120mm` weapon.

## Run and recorded outputs

Set `VERA20K_GAMEMD_EXE` to the original executable, `PYTHONPATH=.` and
`VERA20K_PROJECTILE_RENDER_ASSETS` to the physical extraction directory used by
`bridge_render_inputs`. That directory contains the selected layered INIs, map,
`120mm.shp`, `palette.pal` and `anim.pal`. No retail binary is copied here.

```sh
python -m tools.projectile_oracle.bridge_render --check
python -m tools.projectile_oracle.bridge_render_shape --check
python -m tools.projectile_oracle.bridge_render_pixels --check
python -m tools.projectile_oracle.bridge_render_flight --check
```

The default is also check-only. `--write` deliberately regenerates JSON and its
`.meta.json` provenance sidecar. The pixel script also regenerates
`bridge_render_pixels.rgb565.bin`, 65536 original little-endian RGB565 words;
check-only compares that file byte-for-byte. These commands were run with
`--write` followed by an independent `--check`. They do not run Rust or a GPU.

| Corpus | Original execution and output | Boundaries |
|---|---|---|
| `bridge_render` | 149 fresh draw rows and six same-object live structural-state rows; projection, draw gates, frame, body/shadow arguments, Convert and surface identity | Prepared retained Bullet/map/camera/dirty state. Original input readers rerun first. Only final shape draw is an argument sink. |
| `bridge_render_shape` | 32 baseline1024 plus eight baseline32768 complete Object Render/Bullet Draw calls through the physical one-frame 120MM, original palette startup/Convert construction, shape frame access, clipping, selector, plain rowwalker and actual RGB565 pixel leaves | Prepared 64×96 RGB565 BSurface and circular A/Z buffers; supplied old color/depth. No calls replaced during the draw. |
| `bridge_render_flight` | One native selected105mm speed/launch, eight ordinary motion blocks, then eight draw captures | Supplied upstream source/target coordinates, admitted collision commits and current cell state; final shape sink. |
| `bridge_render_pixels` | 400 signed-depth/transparency/repeat cases across all four Bullet-selected plain/RLE leaves; four multi-pixel stencil cases; both shadow leaves over every 16-bit destination word | Prepared Convert leaf/palette/A/Z inputs. Body color `0x55AA` is deliberately synthetic. Shape decoding and retail colors belong to the preceding corpus. |

## Native chain and ABI

The shared surface owner also serves the physical Flat voxel controls in
`tools/voxel_oracle/raster_fixtures.py --bullet-flat`. The optional mode keeps
the existing 22 `--bullet` Kirov raster controls unchanged and extends the same
`raster.py` machine through completed Bullet draw, native Convert selection,
packed-raster BSurface, clipping, rowwalker and RGB565 leaves:

```sh
VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extract \
python -m tools.voxel_oracle.raster_fixtures --bullet-flat --check
```

The extraction directory needs `SUBT.VXL`, `SUBT.HVA`, `VOXELS.VPL`,
`UNITTEM.PAL`, `ANIM.PAL`, `PALETTE.PAL` and the physical `RULESMD.INI`.
Existing native palette owners parse LightGold and execute selected temperate
palette expansion plus whole ColorScheme/LightConvert constructors; voxel
draws receive that actual ColorScheme Convert, not an SHP palette substitute.
The selected theater/name, paired registry and retained Bullet/surface inputs
are explicit component boundaries. Successful allocator and CRT destructor
registration transports supply no gameplay or pixel result.

`bullet_flat_fixtures.json` records compact supplied depth/A inputs, physical
asset identities, native matrices/sparse raster indices, source and destination
rectangles, original leaf candidates, and RGB565 changes after each ordered
draw. Unlisted pixels retain the input background. Exact full Z/A bytes remain
unchanged. Controls include accept/reject/tie, edge/full clipping, physical zero
holes, varied A, two draws, non-Flat passthrough, signed negative candidates,
and a circular Z/A scroll crossing the buffer end. Input patches use physical
buffer indices; observed leaf offsets retain their screen-to-buffer mapping.
The file-backed Flat bias is used as inspected; no universal runtime value,
native submarine launch, native Display traversal or whole-scene/GPU parity is
established by this component corpus. Source assets and execution receipts stay
outside the repository; sidecars pin the executable and command, not tool sources.

The actual Bullet vtable is `7E46E4`; `+104 → 5F4B10` is Object Render,
`+114 → 468090` Bullet Draw, `+48 → 5F65A0` retained coordinate getter,
`+1C8 → 5F5F40` ground-relative height, `+1D0 → 5F5F30` raw Z and
`+1E8 → 468000` frame getter. BulletType vtable `7E4948` supplies the SHP.
Original projection `6D2140` and `AdjustForZ 6D20E0` consume signed raw XYZ.

`CC_Draw_Shape 4AED70` has surface in **ECX** and Convert in **EDX**.
The shadow caller loads `EDX=[87F6C4]` at `46836D`; the body caller loads the
surface at `468415` and `EDX=EBX` at `46841B`. The callee saves EDX at
`4AED80` and ECX at `4AED82`. The prototype research recorder incorrectly
called ECX the Convert; the tracked corpus records both correctly.

The shadow call returns at `468379`: flags `0x2601`, gradient 0,
brightness 1000, Z-adjust `-10-AdjustForZ(surface Z)`. The body call returns at
`468422`: flags `0x2E00`, gradient 0, brightness 1000, Z-adjust
`-30-AdjustForZ(raw Z)`. Both use the same image and selected frame.
The shadow Convert always comes from `87F6C4`; body AnimPalette selects
`87F6C0`. FirersPalette is a separate house-Convert input path, outside the
ordinary selected retail route exercised here.

Original `GetHeight 5F5F40` subtracts ground and the independent OnBridge 416
term. Draw rereads the current cell's structural `0x100` flag. Without OnBridge,
height at least 416 over a structural bridge raises the shadow surface by 416
and subtracts 416 from shadow height. A remaining height greater than zero and
Shadow=true admits the shadow. The same retained Bullet is rerendered after
supplied intact→collapsed→repaired flags, proving the live query. The fixture
does not execute collapse or repair lifecycle calls.

For raw XYZ `[2688,5248,1041]` at level 6, native zero-camera projection is
`[-300,315]`. The structural bridge gives shadow `[-300,315]`, Z-adjust -160;
without it the shadow is `[-300,375]`, Z-adjust -100. Body stays at
`[-300,315]`, Z-adjust -180. OnBridge retains its separate ground/height terms:
its shadow position is on the deck but its Z-adjust remains -100. The physical
pixel cases confirm these draw arguments reach actual shape pixels.

## Frame, gates and coordinate coverage

The draw corpus covers levels -1/0/6, raw Z -105 through 1500 including signed
byte boundaries, all 16 slopes, off-map/signed XY, camera translation, dirty
rebasing and inclusive padded projection limits. VERA's common world-row bias
of 15 is deliberately absent from original projection results.

Inviso, deferred-hidden byte and a null image suppress draw calls. The scenario
`0x1000` helper `5865E0` executes its original constant-false body. These results
do not establish complete fog or Display-list admission. The deferred-hidden
byte is supplied for its gate; its NUKEBALL lifecycle remains a separate chain.

The ordinary original Cannon type is inverse-Rotates=true and the physical
120MM contains one frame. Four explicitly synthetic frame-count controls
replace only that SHP header count with 32 to expose rotating/runtime-animation
frame selection. Those controls are not claims about a multi-frame retail120MM.

## Physical shape and color evidence

`bridge_render_shape` freshly obtains the original Cannon type and physical
48-byte 120MM SHP. Palette startup `52BE61..52BFCE` executes the physical
ANIM.PAL/PALETTE.PAL loads, channel expansion, full Convert constructor
`48E740`, blitter initialization `48EBF0` and intensity-table construction.
The resulting heap bytes and palette/format globals are copied unchanged into
each draw machine. No RGB565 color is computed by the Python harness.

The prepared surface uses original BSurface vtable `7E2070`. Original bounds,
lock, unlock and pitch methods run. The physical frame is a 4×4 plain indexed
region at offset (10,10) in a 24×24 canvas; the original decoder and rowwalker,
not Python, consume its source bytes. Twelve indices are nontransparent.
The original palette selects body colors such as index139→RGB565 `0x5AEB`.
The AnimPalette control traverses the same original color machinery separately.

The corpus records source scanlines, destination x/y, candidate depth and
brightness at each leaf entry, plus sparse RGB565 framebuffer values after each
shadow/body call. All 40 cases retain every original Z-buffer byte. Clipping
cases include each screen edge, partly off-screen sprites and dirty rebasing.
For body point (32,16) and raw Z1041, the original body candidates are
830,829,828,827 in the prepared depth baseline1024. At old depth828 only the
last row's two opaque pixels pass; equal depth rejects. This is a native
rowwalker/leaf result, not an expected value calculated by the harness. Eight
`canonical_depth_rows` repeat full, clipped, dirty, below-deck, collapsed and
exact-depth inputs with the independently executed original32768 baseline; the
original32 `rows` retain their1024 baseline.

## Selected leaves and exact scope

| Shape | Plain selector slot / vtable / leaf | RLE selector slot / vtable / leaf |
|---|---|---|
| Body `0x2E00` | `+98 / 7E56F0 / 494B60` | `+138 / 7E5420 / 497FD0` |
| Shadow `0x2601` | `+2C / 7E5948 / 492D20` | `+F0 / 7E5540 / 496820` |

Body writes accepted source color. Shadow halves the existing destination via
`(destination >> 1) & 0x7BEF`; both use strict signed candidate < stored u16 Z
and neither writes Z. Transparent input preserves both attachments. Repeating a
passing shadow on `FFFF` yields `7BEF`, then `39E7` because depth is unchanged.
Both native shadow formats agree over all 65536 destination colors, with packed
SHA-256 `5ad832a7d9435b1d61eac2a0cce43d6bd3db84274c9ef277996a184eba1dc53d`.
The original mask construction `4BAA73..4BAAC8` also executes in the leaf corpus.

The ordinary physical120MM route is plain. RLE results here establish selected
leaf behavior only; they do not claim a physical RLE projectile's complete
rowwalker or Z-shape semantics. Voxel projectiles, house-palette selection,
NUKEBALL lifecycle, complete Display/fog admission and a native full scene
capture remain outside this corpus. The separate `bridge_render_flight` transcript composes original selected
weapon/type/Rules readers, actual FireAt distance/GetSpeed block
`6FE4F2..6FE53F`, launch `6FE8EE..6FF01A` (BulletFire virtual+1F0→`468670`),
eight original motion blocks `46718F..467494`, and these draw calls. The source
`[2688,5248,1120]` and target `[3188,5248,1120]` are explicit upstream fixtures.
The original reader stores weapon Speed102 from physicalSpeed40; original
GetSpeed produces59 for this500-lepton shot at Gravity6. Native launch velocity
bits are `40499976286f0f80,8000000000000000,403d509803a1fb40`. The transcript
carries those native bytes into motion and draw; it does not hand-calculate
trajectory expectations. Its declared world/Display, collision-commit and
current-cell boundaries prevent a complete FireAt/AI/lifecycle claim.

Rust comparisons, GPU comparisons and production runtime validation must be
reported separately from these original executions.

The selected draw chain adds no gameplay RNG draws, timer writes or detach
calls. Supplied map/object state and palette construction have their own
upstream evidence boundaries as described above.
