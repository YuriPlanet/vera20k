# Retained Terrain coordinates in drawing

`terrain_render.py` preserves 82 original-engine captures: the 41 inputs from
`terrain_coordinate.py`, crossed with viewport/dirty-Y pairs `(0,0)` and
`(37,100)`. It also records 44 animated SpawnsTiberium captures: stages 0..10
of a supplied 22-frame SHP, on flat and level-2/slope-1 ground, crossed with
the same two viewport/dirty-Y pairs. The original Terrain placement, Object
coordinate storage, Terrain Render suffix, projection, height getter and DrawIt execute. After placement,
the source cell becomes level 7/flat; rendering still reads the retained XYZ.
The separate `stock_shapes` records 44 original format-3 decoded-index witnesses:
all 22 body/shadow frames of the two unique retail byte variants, bound to all
18 TIBTRE01–03 theater/type assets.

The binary is active-retail `gamemd.exe`.
With the existing Unicorn environment and `VERA20K_GAMEMD_EXE` or `RA2_DIR` set:

```sh
python -m tools.spatial_oracle.terrain_render --check
```

`--write` deliberately regenerates the JSON and provenance. The checked payload
records the raw retained coordinate, projected point and all 14 native draw
arguments, along with their interpreted fields. No executable or original SHP
files are distributed. The stock witnesses preserve hashes of complete native
decoded index crops, their rectangles and counts, without sprite index bytes.
Native regeneration and `--check` passed when this corpus was
introduced; Rust and production validation are recorded by the chain owner.

## Native owner and calls

Terrain's actual vtable `7F522C+104` is Render `71CC50`, whose admitted suffix
`71CD22..71CD81` calls center getter `41BE00` through `+AC`. That getter calls
Terrain's `+48 = 5F65A0`, copying retained Object Location. At `71CD3D`, original
`6D2140` projects the XYZ and subtracts the tactical camera. The suffix then
rebases for dirty clipping and calls `+114 = 71C1B0` at `71CD7B`. The active
Tactical rendering loop dispatches the `+104` slot (for example `6D916C`);
its class/visibility admission is outside this fixture. The older generic
Object Render `5F4B10` is not Terrain's `+104` owner.

DrawIt copies its supplied point at `71C234..245`. The `+1D0` call at `71C249`
is Terrain's original `5F5F30`, which reads signed Location.Z at `Object+A4`.
Original `6D20E0` converts it to pixel lift. The ordinary body at `71C304` uses
Z adjustment `-lift-12`, gradient 2, flags `4E00`; shadow at `71C34E` uses the
same point, adjustment `-lift-3`, gradient 0 and flags `4E01`. No second ground
or bridge-height query changes either position or depth adjustment.

Examples from original execution at world XY `(2688,5248)`, before camera or
VERA's constant world-row bias:

| Placement input | Retained Z | Point | Lift | Body/shadow Z adjustment |
| --- | ---: | --- | ---: | --- |
| Level 0, flat | 0 | (-300,465) | 0 | -12 / -3 |
| Level 2, flat | 208 | (-300,435) | 30 | -42 / -33 |
| Level 2, slope 1 | 260 | (-300,428) | 37 | -49 / -40 |
| Level 2, slope 15 | 312 | (-300,420) | 45 | -57 / -48 |
| Level 2, supplied Z 999 | 999 | (-300,321) | 144 | -156 / -147 |

The structural `0x100` flag does not lift Terrain's ground placement to a
bridge deck. The companion coordinate oracle establishes that behavior.

## Animated SpawnsTiberium draw caller

The `animated_rows` execute the same suffix and full DrawIt with
`TerrainType+2B1 = SpawnsTiberium`, `+2B3 = IsAnimated`, `Terrain+AC = stage`
and raw SHP frame count 22. Original `71C208` reads the authoritative stage;
`71C2A9` subtracts 16 pixels from the supplied point before both draws. The
body uses global Tiberium Convert `87F6BC` and signed Cell top brightness
`+10A`, with flags `2E00`, gradient 2 and Z adjustment `-lift-12`. The shadow
uses Cell Convert `+34`, literal brightness 1000, flags `2E01`, gradient 0
and adjustment `-lift-3`. Its frame is `stage + trunc(raw_frame_count / 2)`.
Both Convert register arguments are observed, with distinct supplied pointers
and top/ground brightness 700/400 so the selection is visible in the corpus.

| Ground / stage | Projected point | Body/shadow point | Body/shadow frame | Body/shadow Z adjustment |
| --- | --- | --- | --- | --- |
| Flat / 0 | (-300,465) | (-300,449) | 0 / 11 | -12 / -3 |
| Flat / 10 | (-300,465) | (-300,449) | 10 / 21 | -12 / -3 |
| Level 2, slope 1 / 0 | (-300,428) | (-300,412) | 0 / 11 | -49 / -40 |
| Level 2, slope 1 / 10 | (-300,428) | (-300,412) | 10 / 21 | -49 / -40 |

The supplied 22-frame shape agrees with raw retail TIBTRE01–03 asset headers:
all 18 variants in TEM/SNO/URB/UBN/DES/LUN contain 22 format-3 frame records
in an 84×56 canvas. Each body frame rectangle is `(24,4,35,48)`. The files
are 17776 bytes; all nonsnow variants have SHA-256
`0a9218d1c55745b98c7cfe6ca4e02cf730ea00c40bae43eac0b298e6045df47c`,
and snow variants have
`76222b3d339b33d9535e1b1bc978cb78d0778ec3d633d1b8fdf34217b5d1673a`.
These bytes were inspected using the existing `tools.sidebar_oracle.stock.mix`
and `mix_hash` readers. Shape decoding and retail image binding do not execute
in this caller fixture; animation timing has separate native evidence.

For format 3, original extended selector `490E50` with `2E00` selects Convert
slot `+138` at `490F68`; `2E01` selects `+F0` at `490EFA`. Original binary
mask `81DC28` is `3000`. Initializer `48EBF0` installs vtable `7E5420` for
`+138` and `7E5540` for `+F0`, whose method `+4` leaves are `497FD0` and
`496820`. Both compare a signed candidate against stored unsigned 16-bit Z
with strict less-than and only write accepted color. They do not store Z.
This selector/vtable/instruction trace establishes the read-only depth policy;
the caller corpus substitutes the drawing routine and cannot establish it by
itself. The existing [selected pixel oracle](../projectile_oracle/bridge_render.md#selected-leaves-and-exact-scope)
executes these exact leaves, including repeat draws and all 65536 destination
colors for the shadow. Its prepared palette/depth inputs and coverage limits
apply when reusing that evidence for Terrain.

## Stock format-3 decoded indices

The `stock_shapes` inputs come directly from `ra2.mix` and `ra2md.mix` through
the existing MIX reader. Outer archive, inner archive and SHP hashes bind each
of the 18 asset aliases. Identical SHP bytes share one execution. Original
`69E7E0` produces the cropped frame rectangle; original `4AED70` then executes
frame access `69E740`/`69E900`, clipping, selector `490E50`, row walker `437A10`
and the original format-3 RLE body leaf `497FD0`. No reached call is substituted,
and there is no Python RLE decoder.

Each prepared Convert maps source indices to equal 16-bit output words. A zero
shade lookup and accepting Z buffer isolate decoding from color selection.
The native framebuffer's low bytes give source indices; high bytes must remain
zero. The harness checks every native row destination, width and left skip,
crops by the original rectangle, and records the complete crop's SHA-256 and
nonzero count, alongside the native rectangle and RLE row count. Index bytes
remain transient and are not tracked. Transparent runs retain the zero background. Native Z bytes must
remain unchanged. Both unique variants produce 882 nonzero pixels in body frame
0 and 248 in shadow source frame 11; all 44 complete frame hashes are recorded.

This fixture uses a prepared 128×64 BSurface, full clip, point `(42,28)`, A 127,
old Z 65535 and baseline 32768. All frames use body flags `2E00`, Z adjustment
`-12`, gradient 2 and brightness 1000. Shadow source frames deliberately pass
through the body leaf to recover their indices. Consequently these witnesses
establish stock decoding and rectangles, with the stated fixture inputs; they
do not establish retail palette colors, actual Terrain shadow pixels, resource
construction, animation AI or scene parity.

## Production comparison and limits

The changed `overlays.rs` reads `TerrainObjectState::world_coord`, projects it
through `absolute_leptons_to_screen`, and supplies the same retained-Z lift to
`native_terrain_instance`. VERA's established world frame adds 15 to
native absolute screen Y. Its temporary half-tile subtraction/addition cancels
before the pair is emitted. `lifted_z_adjust` matches the native body/shadow
arguments; `compute_sprite_depth_params_lifted` cancels the lift for the legacy
sort/depth scalar. These are source comparisons; the accompanying Rust test
must establish the named production helper comparisons separately.

In the caller rows, only `CC_Draw_Shape4AED70` is substituted: it records arguments and returns
without drawing. Map lookups, coordinate getters, projection, AdjustForZ and
the Terrain DrawIt body run unchanged. The ordinary fixture supplies a
synthetic 4-frame SHP, nonanimated/non-SpawnsTiberium Terrain with health 200,
nonnull Cell Convert and brightness 1000. The animated fixture supplies the
state and converters described above. Both supply shadow enable, runtime
startup constants and clipping/camera locals. The suffix begins after visibility
and rectangle admission. Neither the caller nor the stock-index corpus proves image loading, GPU output,
dirty-region admission, full render scheduling or complete Terrain rendering.
The animated rows establish draw arguments for supplied stage inputs, not AI
cadence or production pixels. Stock decoding is bounded to the physical files
and prepared Convert route described above. No whole-bridge or
whole-Terrain parity claim follows from these rows.
