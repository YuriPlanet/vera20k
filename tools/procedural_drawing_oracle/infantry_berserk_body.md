# Prepared native GI body and viewport

`shroud.py --infantry-berserk-body` keeps eight original GI body controls and
adds 28 joined Object admission/projection/body controls in the same VM.
The consumed JSON contains the physical inputs, native draw parameters,
legacy body pixels before shadow and final pixels after each Object return.
Its sidecar records the command and proof boundaries.
No VERA framebuffer, palette table or hand-computed pixel supplies a golden.

From the repository root, with the configured retail `gamemd.exe` and Python
dependencies from `tools/requirements-test.txt`:

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --infantry-berserk-body --check
```

`--write` deliberately regenerates the JSON and metadata from original execution.
The default shroud corpus and other opt-in modes keep their existing generators.
The existing eight `rows` and prior `shared` fields retain their values; the
viewport group is additive. The same command produces and checks both groups.

## Native route and reused owners

The original route is Infantry `518F90` → Foot `41C090` → Techno DrawSHP
`705E00` → shape `4AED70` → original raw/RLE picker, rowwalker and blitter.
Infantry's `518FDA` arm reads the existing Rules BerserkColor selector and
ColorAdd bytes. Its original pixel-format packing and Cell shroud predicates
execute; no word or intensity result is supplied. The selected ordinary body
uses flags `2E00`, Z read without Z write, intensity 1200 and Z adjust −13.
The original whole DrawSHP also issues physical GI shadow calls for 372/373
with flags `2E01`, intensity 1000 and colour word zero. Original frame accessors
report empty rectangles for these shadow frames; they store no shadow pixels.

This mode composes existing owners:

- `RetailRulesInputs`, `physical_layers`, `sections` and `stock` select immutable
  retail bytes. No new archive, INI or SHP decoder.
- `PaletteReader`, `initialize` and `color_scheme_palette` execute native palette
  expansion, Convert startup and the selected DarkRed 1/53 ColorScheme pair.
- `CampaignReader.ordered_ini` supplies the existing native INI cache transport;
  Rules `665650`, `66D480` and `6691E0` execute the selected colour readers.
- `_prepare_status_passengers` runs Infantry prefix `517A50..517AE6`, including
  Foot/Techno/Object, and the whole Walk constructor `75AA90`.
- `_prepare_flat_cells`, `initialize_discovery_geometry`, `theater_tile_files`
  and `load_tmp_files` supply native-constructed Cells, original geometry and
  the original physical TMP registry/lazy loader required by GetZAdjust `704350`.
- `prepare_surface` owns original BSurface storage and circular A/Z planes.
  The existing Infantry atomic service supplies only the Windows Interlocked ABI.
- The joined group borrows `EntityReveal`'s original retained-viewport prefix
  `6D5F60..6D5F8D` through the existing reader-block runner. It runs inside the
  existing `PaletteReader`, without constructing another `EntityReveal` VM.

No VM, gameplay world, reader, numeric kernel or raster implementation is copied.
Original executable code remains unchanged throughout the measured draws.

## Inputs and schema

`shared` records the common inputs once. Physical root RULESMD supplies ColorAdd,
BerserkColor, ExtraInfantryLight and E1 Image/Strength/Armor. The native type
constructor and full reader `5236A0/5240A0` receive those three E1 keys and fixed
ART GI/GISequence. Native Image resolves `GI.SHP`; sequence loading executes.
Other type fields retain native constructor defaults. This bounded type subset
is not complete retail type fill or a scenario rules-layer history.

Shared physical identities include GI.SHP, ARTMD, RULESMD, palettes,
TEMPERATMD.INI and CLEAR01.TEM. `colour_scheme` identifies native DarkRed53;
its local scheme index 1 is a prepared selected-name registry position, not a
lobby or world scheme index. `frames` records original `69E7E0` rectangles and
`69E900` compression predicates for the four selected physical frames.

The declared admitted actor is at `[2688,5248,0]`, HP125, Alive, outside Limbo,
Ready/Stage0, without cloak/disguise/curtain/flash. Its owner is CurrentHouse.
The flat native Cell crop spans x9..11/y19..21, tile0/subtile0, level/slope0,
no bridge/overlay, top light1000. Selected theater registration starts at0.
The native facing table `7EAEFC` supplies facing53248 for frame0 and45056 for
frame1; original snap `4C9300` and resolver `518D80` choose the actual frame.

Surface priors are 64×96, draw point `[32,48]`, clip `[0,0,64,96]`, RGB565
format2, CMOV/MMX enabled, FPCW `0E7F`, circular depth baseline1024, A127 and
background `39E7`. Old Z is2000 for visible controls or0 for blocked controls.

Each `rows` entry contains:

- `input`: physical frame/facing, prepared berserk and shrouded flags, old Z.
- `body`: actual shape frame, point/clip, flags, Z adjust/gradient, intensity,
  consumed low16 colour word, picker/leaf, first candidate Z and the complete
  little-endian RGB565 buffer at actual body return `70645D`, before shadow.
- `shadow`: actual shadow shape parameters, without a duplicate pixel buffer.
- `final_rgb565_le_hex`: complete buffer at actual shadow return `706602`.
- `invariants`: exact unchanged source, A, Z, all three complete RNG buffers,
  no observed RNG calls, and byte-identical executable code.

The eight controls are frame0 (physical raw format1) and frame1 (RLE format3),
each ordinary, berserk, shrouded berserk and Z-blocked berserk. Native ordinary
and shrouded words are zero; admitted berserk words are `E000`. Raw plain/colour
leaves are `494B60/494C40`; RLE leaves are `497FD0/498150`. Both coloured leaves
OR the word after native palette/A conversion. Index-zero source holes preserve
the background. Blocked body pixels remain background and A/Z stay unchanged.
The final buffer follows the actual separate shadow call. It equals the body
buffer for these physical empty-shadow frames; no positive shadow-store or
shadow-overlap result is established.

## Joined Object viewport group

`shared.viewport` records the unchanged Infantry table and actual method slots,
original zero-camera projected point, retained viewport prefix, ordinary client
priors and projection margins. The native camera uses Tactical `+B0/+B4` in
the original projected-world frame. A Rust consumer translates the declared
camera using the existing shared projection-origin query, then applies its
canonical actor projection and ordinary camera uniform. This preserves VERA's
common world-row bias without using a native output point to place the sprite.

Native `RenderCoords` `41BE00` delegates to the actual `GetCoords` `5F65A0`.
Original `6D2140` first produces the zero-camera point from that coordinate.
Control cameras subtract selected screen anchors from this original point.
Partial anchors use approximately half the selected header width or height;
adjacent outside anchors put that header exactly beyond the relevant surface
edge. Neither a VERA projection nor expected pixels supply those inputs.

Every joined draw calls whole Object `5F4B10` with the actual constructed
Infantry receiver and `(caller_clip,1,0)`. The viewport and caller clip are
`[0,0,64,96]`, graphical-client handle `B73550` is nonzero, editor `A8ED6B`
is zero, and the declared receiver starts with Redraw0/Limbo0. Actual native
projection and caller clipping choose whether to call virtual `+114` `518F90`;
the fixture does not substitute an admission result or body point.

`viewport_rows` contains 14 visits for each physical frame0 and frame1:

- Center and four partial overlaps, required to produce original nonbackground
  pixels.
- Four adjacent outside placements, required to leave the destination unchanged
  even though Object projection admits and calls the body.
- Four rejected projections, required to leave the destination unchanged and
  never call the body. Original `6D2223..6D2269` uses inclusive bounds
  X `[-360,W+360]` and Y `[-180,H+180]`; these controls sit one beyond each
  bound. The strict boundary interpretation comes from the original branches,
  rather than from a Rust predicate.
- Return to the original center camera, required to reproduce its exact buffer.

Frame0 retains prepared facing53248. Frame1 uses49152, the authored map-facing
192 shifted left8; original table `7EAEFC` and selector `518D80` choose frame1
for Ready/Guard with the physical `0,1,1` record. This does not itself establish
a live scenario's Doing, facing, Stage or input admission.

Each joined row records:

- `input`: the standard ordinary frame/facing/shroud/Z priors plus native
  `camera`, `caller_clip`, `redraw`, `force` and `limbo`.
- `admission`: actual `raw_return` and `returned_al`, `projection_admitted`,
  full-viewport projected `point`, separately observed `body_point`,
  `body_entry_count`, `body_admitted`, actual mutated `clip`, `redraw_after`
  and bounded entry events. Projection outputs are read at actual caller return
  `5F4B6F`; body arguments are read only when native `518F90` is reached.
- `body` and `shadow`: nullable actual shape argument records. Body admission
  can reach `518F90` yet emit no `4AED70` shape call. These records omit duplicate
  per-shape RGB buffers; `final_rgb565_le_hex` always records the full destination
  after the Object return, including rejections. `changed_pixel_count` counts
  its words that differ from the declared background.
- `invariants`: the existing source/A/Z/all-three-full-RNG/code guards plus
  `vtable_unchanged` and `facing_table_unchanged`. The actual Infantry vtable
  and `7EAEFC`'s 128 facing-table bytes are compared with original file bytes.
  Every row checks these guards, including rejected draws.

Object AL1 means body dispatch, not visible pixels. A Rust comparison must
submit its own canonical actor projection, camera and atlas through its ordinary
clipping owner; native point, clip and admission outputs must never gate or
position that Rust draw. The original buffer alone supplies expected pixels.

## Proof boundary

These are complete selected body draws on declared component inputs. The
shrouded case deliberately enters DrawIt to exercise word clearing; it does
not prove that whole Object DrawIfVisible admits a shrouded GI. The ordinary
joined viewport group establishes only its explicit client/camera/lifecycle
priors and selected physical body poses. Constructor
RNG/timer observations are separate from the RNG-free measured draw interval.
Full constructor/COM activation/Unlimbo, actual House/scenario/Cell production,
full Tactical traversal/display registration, CAOS hit/refresh/expiry, GPU output and whole frames/worlds
require their own evidence. Other stances, effects/translucency, custom rules,
nonflat sites, global ordering and Behind remain outside these bounded samples.
Object admission clears Redraw+80 before projection and adds no RNG draw, timer
write or detach call. No simulation dirty latch or duplicate display state is
introduced for the ordinary main-Tactical force1 route.
