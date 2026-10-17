# Prepared native GI berserk body

`shroud.py --infantry-berserk-body` executes eight original GI body draws into
independent RGB565 surfaces. The consumed JSON contains the physical inputs,
native draw parameters, body pixels before shadow and final pixels after shadow.
Its sidecar records the original binary identity, command and proof boundaries.
No VERA framebuffer, palette table or hand-computed pixel supplies a golden.

From the repository root, with the configured retail `gamemd.exe` and Python
dependencies from `tools/requirements-test.txt`:

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --infantry-berserk-body --check
```

`--write` deliberately regenerates the JSON and metadata from original execution.
The default shroud corpus and other opt-in modes keep their existing generators.

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

## Proof boundary

These are complete selected body draws on declared component inputs. The
shrouded case deliberately enters DrawIt to exercise word clearing; it does
not prove that whole Object DrawIfVisible admits a shrouded GI. Constructor
RNG/timer observations are separate from the RNG-free measured draw interval.
Full constructor/COM activation/Unlimbo, actual House/scenario/Cell production,
display admission, CAOS hit/refresh/expiry, GPU output and whole frames/worlds
require their own evidence. Other stances, effects/translucency, custom rules,
nonflat sites, global ordering and Behind remain outside these eight samples.
