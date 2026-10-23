# Ordinary flat temperate ClearTile

Run from the checkout with the configured retail executable/assets:

```sh
python -m tools.spatial_oracle.bridge_shadow_render --tile --check
```

`--write` deliberately regenerates the small consumed corpus and sidecar. The
default bridge-shadow, `--body` and `--ore` modes retain their existing inputs,
allocation order and corpora. This mode extends their physical-asset and native
caller owner; `tmp_clear_render.py` contains no VM, image decoder, palette,
projection, RNG or expected-pixel implementation.

The additive packed-color numeric mode is consumed by
`native_surface_format::packed_color_mixer_matches_original_executed_controls`:

```sh
python -m tools.spatial_oracle.tmp_clear_render --packed-color-mix --write
python -m tools.spatial_oracle.tmp_clear_render --packed-color-mix --check
```

Its 20 rows contain only format, two logical input words, unsigned amount and
the original `4BF650` low-word result. Existing PaletteReader initialization
is reused; original `4BA9CD..4BAB06` derives masks from declared RGB565/RGB555
descriptors. Only exact amounts 0/25/50/75/100 have special dispatch. Both 25
and 75 quantize the first word without reading the second; 50 adds masked
halves. The remaining controls cover adjacent/default amounts, zero source,
RGB555 and two retail ore/gem colors. The latter RGB inputs come from the
production TIB01/GEM01 reader and are packed by original `641409..641470`.
Their ground input is existing `tmp_preview` level0 Cell output42282;
additional controls use zero and original first-mix48549 as the following
operation's input. The sidecar records these boundaries and the replay command.
This mode does not execute RMG, raster drawing or gameplay; existing default
and `--preview` modes keep their inputs and native instruction boundaries.

The active route is Tactical6D2DE0 → full-view6D7560 → Cell480350 →
IsometricTileType547CF0. The selected path is tile0/subtile0, ordinary draw
argument0, no smudge, physical `clear01.tem` and its seven contiguous independent
siblings. Original General56-read block545535..545C3F and ordinal publication
545CEF..545FA3 establish ClearTile0 from physical `temperatmd.ini`; the original
TileSet0000 integer/string readers establish TilesInSet1 and FileNameClear.
Original TileType5447C0 constructors run. Variant chain/count/lazy filenames are
supplied component metadata from the physical contiguous family; the full
theater type-loader loop is excluded. Original GetImage544CB0 and full
lazy547020 execute CCFile reads, ownership, offset relocation, radar rebuild and
local-file destruction. Existing RawFile Windows transport returns immutable
physical bytes. No resident host-relocated TMP substitutes for these calls.

The existing palette owner expands physical ISOTEM.PAL and TEMPERAT.PAL and
constructs the actual ISO-source default LightConvert. Each row supplies a cold
Cell+34; original483E30,484180 and544E70 gather the Scenario profile, calculate
the Cell scalars, select/create its Convert and update the native reference.
The produced Convert's source/destination fields must name ABBED0/885780.
Normal defaults6838D3..6839BD and the whole six-key normal Lighting read/conversion
block68A817..68A979 plus its Level store68A984..68A98A execute. Physical XMP03T4
is a separate authored profile. Empty local-light/effect registries, supplied
detail2 and RGB565 palette format bound these comparisons.
`cell_normalized_rgb` records Cell+110; `convert_rgb_key` records the effective
LightConvert+198 key after native detail quantization. These are distinct values.

Main seed0 and original4814F0 generate the process-persistent64-entry selector
table. Generation records128 native Main draws and before/after state hashes.
Raster rows use that warm table, with explicit rotations for all eight physical
files. Every warm caller must leave the Main structure unchanged. Full process
bootstrap/load continuation remains with the existing consumed
`bridge_recalc_presentation` comparison.

The19 raster rows cover all eight siblings, levels2/6 and supplied signed-1,
A127/A0, one clipped rectangle, a repeated call, one full-view XMP03T4 cell,
one physical SHROUD edge, and one-pixel near/equal/far depth controls. Original
TMP547CF0 creates its diamond tables, acquires/releases an intensity table,
uses original BSurface methods and executes scalar548D90 pixel stores.
Unlike the SHP body path, this TMP base loop admits **unsigned equal Z**.
Depth-control candidates come from an original raster output, not a host depth
formula. Final output is lossless RGB565/Z runs relative to supplied background
and oldZ. The256 draws-free lighting controls execute full483E30 for every raw
Cell11B byte under the default profile; they establish signed-byte numerical
behavior without claiming a natural negative-height producer.

The active CPU startup chain is WinMain call6BDF5F →5355D0 call53560A
→4E6190 →7C5F3D/7C5FFF. This fixture executes both original CPU probes in
its shared VM before TMP drawing and requires MMX84E862=1 with independent
TMP dispatchABC55C=0. The CPU probes write the84E860..864 capability bytes;
the palette row builder7DE200 reads MMX84E862/CMOV84E860. ABC55C has only
two original literal consumers5488F3/5494F8, no direct/overlapping writer or
literal address escape found in the checked executable, and lies in the
zero-filled PE virtual tail. The probes leave it zero even with MMX enabled.
Its alternate extra-image function7C61CF is scalar; it is not an MMX branch.
This evidence does not exhaust arbitrary computed aliases or execute full
WinMain, and makes no universal unreachability claim for injected/custom state.

The full-view row supplies Tactical origin894,1254 and redraw rectangle192x160.
Map bounds/lookup transport admits exactly the physical authored Cell64,31 at
level6. Original inverse projection, traversal,47FF80 bounds, rectangle
intersection and forward projection produce incoming480350 point66,171; the
same call continues through cold lighting and raster. This is a bounded
full-caller comparison, not a native whole-map loader/Recalc claim. Physical
authored facts for64,31 and66,34 are retained separately from live-cell input.
The shroud row executes original47EFE0 on physical SHROUD.SHP frame15 in the
same VM, then passes its captured row-major A bytes to the TMP draw. Reveal
traversal and AlphaShape composition are separate mechanisms.

The Rust consumers are the production TMP/TileAtlas/visible-instance/draw path
in `tmp_clear_gpu_tests.rs` and the shared Cell-light signed-height tests. No
GPU result or whole-game parity is implied by replaying this native tool.
Smudges, extra/cliff images, damaged/null/custom files, custom opaque index0,
other theaters, full graphics startup, arbitrary zoom,
dirty-redraw6D7F20 and native map population remain outside this corpus.
Native Cell/Convert destruction is not invoked when resetting supplied cold
component rows. Its reference counts are observed effects, not a resource
retirement comparison. This selected draw passes no gameplay timer or detach;
its only RNG is the recorded cold process-table generation.

## Retained partial redraw versus full reconstruction

The additive `--tile-partial` mode generates `tmp_partial_redraw.json` and its
sidecar in external scratch. Copy the committed pair there before checking:

```sh
mkdir -p "$HOME/Documents/vera20k-dev/class-parity/cell-class/cell-partial-native-controls-v3"
cp tools/spatial_oracle/tmp_partial_redraw{,.meta}.json \
  "$HOME/Documents/vera20k-dev/class-parity/cell-class/cell-partial-native-controls-v3/"
python -m tools.spatial_oracle.bridge_shadow_render --tile-partial \
  --partial-output-dir "$HOME/Documents/vera20k-dev/class-parity/cell-class/cell-partial-native-controls-v3" --check
```

Use the shared physical projectile-render input directory required by the other
modes (`VERA20K_PROJECTILE_RENDER_ASSETS` when the default extraction is absent).
`--write` regenerates controls for review. `--partial-detail` keeps all intermediate
surface runs in a separate scratch file. Existing modes and reset defaults retain
their behavior. The new mode directly invokes original functions on their shared
VM/surfaces, since the independent-row TMP and Shroud wrappers reset their planes.

Original Cell constructors enroll every member of a declared 32×32 registration;
MapSize16×16 remains an independent supplied input. Every prior and measured
Cell480180/480350 receiver must belong to that registration by address, including
offscreen traversal. No scene membership is inferred from the resulting draw list.
Flat tile0/subtile0/level0, known flags, camera, HWND and disabled Fog/AlphaShapes
are component priors, not a native map/window/reveal bootstrap.

Original full Shroud6D3660 and terrain6D2DE0 first produce each prior from
black/FFFF/A127. Physical SHROUD output must be nonuniform. Four histories then
retain RGB/Z/A: no dirty cells, actual6DA7D0 enqueue, full-redraw bypass, and a
clipped maintenance rectangle. Tactical6D2B60 invokes Cell480180 at6D2DB1;
480180 selects the first TMP/LightConvert, signed Cell10C/11B and special
flags[0,1,0,0]. Actual Shroud then terrain continue on those retained planes.
The two unconditional queue/redraw cleanup stores6D4571/6D4B37 execute separately;
unrelated upper-object/shell bodies are excluded. Executable sections remain
byte-identical to the original executable.

The compact corpus retains lossless prior/final RGB565/Z runs, native A bytes,
intermediate hashes, queue transitions, actual calls and all three complete RNG
states. Interner indexes only select observed data. It supplies no expected
pixels or native callback results. Main's cold selector setup precedes the measured
transition; measured RNG requests and states cover Main, Scenario and Mapgen.
No gameplay timer or detach occurs on the selected component route.

The production consumer in `tmp_clear_gpu_tests.rs` compares full reconstruction
through `begin_main_pass`, unlike the independent-row load-pass tests above.
This is a bounded flat temperate comparison. Native-produced A is a presentation
input, not proof of live reveal/A lifecycle. Other theaters, custom/no-Z first
images (which can affect RGB), scrolling, Fog/AlphaShapes, upper-object
interleaving and universal partial/full reconstruction equivalence remain open.
