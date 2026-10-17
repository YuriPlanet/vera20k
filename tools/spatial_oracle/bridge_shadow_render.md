# High concrete bridge shadow rendering

The adjacent oracle executes the original `Cell::DrawOverlay_Shadow` caller
through the physical retail `BRIDGE.TEM` image and original SHP raster code.
The affected shared high wood bridge consumer uses the same path and is
covered by bounded `BRIDGB.TEM` rows.
It establishes this bridge's own shadow stencil, placement, clipping, depth
admission, destination darkening and depth writes. It does not certify a whole
native scene or bridge gameplay.

```sh
python -m tools.spatial_oracle.bridge_shadow_render --check
```

Set `RA2_DIR` or `VERA20K_GAMEMD_EXE` as described in
[native_oracle.md](../native_oracle.md). The harness reads the original
`ra2.mix` and `ra2md.mix` beside the selected executable, using the existing
archive input owner. No asset extraction directory or Rust output is required.
`--write` records a new reference only after original execution succeeds.

The pinned executable SHA-256 is
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
`bridge_shadow_render.meta.json` records execution boundaries, substitutions
and the payload digest. Archive, INI, palette and shape hashes are in the data.

## Established native chain

The original Overlay registry constructor and physical declared prefix place
`BRIDGE1` and `BRIDGE2` at indices 24 and 25. Their complete original
`OverlayType::ReadINI @ 0x005FE770` reads physical RULESMD and MPBattleMD
strings, fixed ART data and the original filename/image binding. Both select
`Image=BRIDGE`, then the physical `BRIDGE.TEM` image: 180x180 canvas, 36 frames,
SHA-256 `4ef96b8bbdbd2dfde5e99afaeb28fce35d8b0cec3cd83240edb727ecd7f82bc7`.
Stock wall, tiberium and crate flags are false and Land is 0. Consequently
`OverlayType::Get_Draw_Offset @ 0x005FDCC0` contributes (0,0).

The compressed shadow frame data and crop/format headers are byte-identical
across all six physical theater variants. `inputs.shape_aliases` records this
comparison, excluding only absolute frame-data offsets, which vary with body
compression. The combined shadow-byte SHA-256 is
`260a7533040da24c0c119feb6bf130e9750b93d4fa9b12d7ccfefe4c085a2373`.
This is a static source-equivalence witness; the other five theater loaders
are not emulated by this corpus.

The same original registry/reader chain places `BRIDGEB1` and `BRIDGEB2` at
indices 237 and 238 and binds `Image=BRIDGB` to physical `BRIDGB.TEM`: a
253x242 canvas with 36 frames, SHA-256
`3d0fb31a5e41b2f42ab5117fc271b00cb023108dbcfaefff8e319e225dedc921`.
The native center uses integer half-width 126 for this odd-width canvas.
Its six theater variants also have identical shadow halves, combined SHA-256
`52fef50b4716f311890fbdd9d1bdd6222e58df4c0397320b0b8cd85866480f62`.

`Cell::DrawOverlay_Shadow @ 0x0047F510`:

- Reads the live signed cell level at `+0x11B` and raw state at `+0x11E`.
- Selects `raw state + raw frame count / 2`, with no body Latin-square jitter.
- Uses `Cell::Get_Draw_Offset @ 0x00480110`: add (30,15), subtract 15 times
  signed cell level, and include the native viewport Y offset. Flag `0x80`
  subtracts 16Y; states 9..17 with that flag subtract another 15Y.
- For those same flagged states 9..17, the shadow caller adds (-15,+7).
- Rebases the point by the dirty rectangle origin, then passes flags `0x4601`,
  Z adjustment `-2 - signed level * 15`, gradient 0, brightness 1000 and no
  optional Z-shape. The body's extra four height levels do not apply.
- Has no structural `0x100` admission gate. With a retained high overlay,
  controls with flags 0, `0x80`, `0x100` and `0x180` all draw. Flag `0x80` affects
  placement; retained partial/collapsed artwork must not be suppressed merely
  because the structural bit cleared.

All 18 physical shadow frames use format 3. Original `CC_Draw_Shape @ 0x004AED70`
executes frame rectangle/data access, format selection, clipping and the
extended rowwalker `0x00437A10`. `0x0069E900` tests the RLE format bit; it is
not a Z-shape accessor. With no optional Z-shape, the rowwalker selects the
zero fallback at `0x0089C568`. Every captured leaf row observes only zero bytes
there. The renderer therefore needs the shared native row-depth behavior,
not a new bridge-specific per-pixel Z texture.

Convert construction and blitter initialization execute original
`0x0048E740`/`0x0048EBF0`. The flags select Convert slot `+0x54` for plain
images or `+0x114` for RLE images, reaching `0x00493830`/`0x00497390`.
For a nonzero source pixel, both compare the signed candidate against stored
unsigned 16-bit Z using strict less. Accepted pixels halve the packed destination
and store the candidate's low 16 bits. The RLE leaf subtracts signed per-pixel
Z-shape before the comparison. The comparison precedes the low 16-bit store, so
wrapping first and relying on hardware depth comparison is not equivalent.

An identical ordinary draw is rejected on its second pass after the first
stored the equal depth. Negative-candidate controls deliberately demonstrate
the exception: a repeated negative candidate can remain less than its wrapped
stored value and darken again. Do not replace this with general shadow-union
logic.

## Native vectors and production consumers

The 43 concrete caller/shape rows cover all 18 raw bridge states, both original
type identities, levels -1/2/6, the four flag combinations, near/equal/far depth,
repeat, viewport Y offsets, dirty rebasing and edge clipping. Each records
the actual draw arguments, physical source crop and all reached leaf row
candidates. `output.runs` is a lossless sparse representation of the final
native framebuffer and Z array: `[x,y,count,RGB565,storedZ]`; omitted pixels
retain the explicit input background and depth. Complete array hashes also
remain in each row.

The 20 additional wood rows cover all 18 states of `BRIDGEB1`, and both
orientations of `BRIDGEB2`, through their independently read retail type/image
bindings. They establish the same flags, generic offsets and depth policy,
and protect the shared renderer/atlas consumer without claiming wood bridge
gameplay. Their supplied cell point is (36,81) to keep the wider native shadow
crop fully visible on the reference surface.

The 36 `shapes` entries expose source opacity as full-canvas horizontal runs,
identified by `(image_name, frame)`,
derived from native accepted pixels on the unrestricted reference surface.
No Python SHP decoder, Rust pixels or hand-computed expected renderer supplies
these masks. The 15 `pixel_rows` separately exercise both selected original
leaf formats, transparency, signed depth, low 16-bit wrapping, repeated writes and
an asymmetric signed Z-shape.

The `traversal` witness executes original Tactical suffix
`0x006D6E5B..0x006D71D3`, with declared map/visibility and draw sinks. It
records all body calls first, then all shadow calls. Each sweep visits
descending `cellX + cellY`, then ascending `cellX` within a diagonal. This
establishes the required order for overlapping destination/depth edits;
sorting bridge shadows by one sprite depth or by texture is insufficient.
The shadow loop checks map bounds, an overlay other than -1, and a positive
intersection with the native shadow rectangle. The original rectangle helper
`0x0047FDE0..0x0047FF71` checks image availability and computes that frame's
projected bounds. Neither contains a fog/revealed gate; this statement does
not describe outer Display composition or fogged snapshots.

The production bridge instance builder and atlas must supply this mask,
placement, live level, raw state and order to the existing shared destination
edit renderer. That owner already implements the native shadow leaf, RGB565
darkening and the live tactical depth authority. The focused GPU comparison
must check color and depth through those production consumers, including
both supported sRGB render-target formats; this CPU corpus alone does not
demonstrate their integration.

## Boundaries and residuals

- Cell contents, the projected ground-cell point, dirty/viewport rectangles
  and old destination pixels/depth are fixture inputs. Viewport Y sets native
  rectangle `0x00886FA4`; the Z buffer row origin stays independently at 0.
  Native map loading, damage/collapse/repair and their ownership are not
  emulated here. The
  traversal witness executes projection; the raster rows deliberately bind
  caller points directly so clipping and depth boundaries are reproducible.
- Original type reads cover physical RULESMD, fixed ART and MPBattleMD. Map
  overrides and LANGRULE are outside this selected fixture. The native
  lexical INI and archive-resource loading boundaries are shared with the
  existing oracle owners and explicitly recorded in metadata.
- A prepared nonnull Cell Convert uses the original initialized PALETTE.PAL
  Convert. Both shadow leaves depend on its selected blitter and RGB565
  half-word mask, not palette-index colors. Cell lazy Convert creation and
  theater lighting are not claimed; the original caller's lazy-init branch
  is bypassed by this valid retained input.
- Only the live Tactical two-sweep suffix is covered. FoggedObject snapshot
  traversal and native whole-scene composition remain outside this claim.
- The separate tile-shadow emitter `0x00547230` and its `C_SHADOW.SHP` resource
  do not occur in this high bridge own-shadow call chain. Its stock inactivity
  and the removed, misidentified railing route are described below.
- No RNG draws, gameplay timer writes or detach calls occur in the selected
  draw chain. Frame/visibility caching and graphics resource creation are
  presentation concerns; this change must not alter simulation state.

## Inactive retail tile shadows

`CellClass::DrawTileShadow @ 0x004802A0` tests tile-type byte `+0x2E1` before
calling `IsometricTileTypeClass::Draw_Tile_Shadow @ 0x00547230`. The constructor
clears this byte (`0x00544880`). The selected-theater reader `0x00545150` enables
it only when the tile's `TileSet####` section reads both `ShadowCaster=true`
(default false) and `ShadowTiles!=0` (default zero); base and variant stores are
`0x005464A6` and `0x00546AF8`. These inputs come from the selected theater MD
INI, independently of layered map/rules files. None of the six stock files
enables the gate. The earlier lazy TMP image lookup remains with its existing
tile/variant owner.

The emitter binds literal `C_SHADOW.SHP` (`0x00545214..0x0054524D`), not
`RAILBRDG.<theater>`. Its tables select zero-based frames 0..13, all empty in
the six identical stock C_SHADOW images. `CC_Draw_Shape` forwards the frame
unchanged to frame bounds and pixels (`0x004AEE60..0x004AEE88`); darkening flags
do not add half the frame count. The production retail regression
`map::theater::theater_tests::active_retail_tile_shadow_route_is_dormant` checks the
actual theater readers and shape metadata. A custom theater that enables the
gate and replaces the image requires a separate implementation. The former
RAILBRDG atlas, bridge-overlay admission and passthrough pass represented
neither this native path nor the bridge's own overlay shadow and were removed.


## Renderer ownership and validation

The bridge call now uses the existing `TerrainDrawRenderer` destination-edit
owner. Its live `Depth32Float` attachment remains authoritative; snapshots are
read inputs for ordered edits, not a second persistent depth owner. The shared
renderer compares signed native candidates before storing wrapped 16-bit Z.
The bridge atlas retains integer source indices and the native cropped frame
rectangle; the builder reads live cell state/level and preserves the original
cell sweep. The bridge-only alpha passthrough and its known-lightness-drift test
were removed. Empty bridge lists do not add a render pass.

The focused GPU regression first failed on the former production bridge route:
white became RGB 187, where the original leaf requires RGB 123/125/123. After the
change, `bridge_shadow_submission_matches_native_color_depth_and_repeat` and
`retail_bridge_caller_matches_native_color_and_depth_attachments` pass on Apple
M4/Metal. The latter matches every pixel of both color and live depth attachments
for all 63 recorded cases in BGRA and RGBA sRGB formats. Native traversal order,
transparent preservation, clipping, equal-depth rejection and the negative-Z
repeat exception are covered. These are bounded native pixel comparisons.

The release observations use seven sealed profiles: both concrete orientations
on Pacific over water, both on Kalifornia over land, two spans on GoldSt, authored
broken concrete on Deadman, and ordinary Hills tank movement above/below the same
cell on the affected high-wood consumer. The strict comparator reports only
`frame.bytes` differences for every before/after pair. All initial/final simulation
hashes, full observation/command transcripts, input/camera and UnitAtlas records
match. Pacific state 9 repeated on the candidate reports strict `MATCH`, including
its image. These are Rust production observations, not native whole-scene frames.

Before release SHA256:
`e54fcfa7d3e563803582807dfe5655e513fcea709cd508df2e3cd61285962b56`.
Candidate release SHA256:
`32f480590a982cf9540a04a05ae2d945f0b632add3514e576531cf3a5b12efb4`.
Both builds are identified by shared Cargo-owner manifests; no copied executable
or conventional target-path guess supplies capture identity.

### Rendering cost

`retail_bridge_shadow_workload_timing` uses the same physical source stencil
through the production atlas, builder and dispatch. On this Apple M4/Metal debug
harness, median prepare-to-completion wall times (three samples after warmup) were:

| Supplied workload | Former alpha pass | Corrected pass |
| --- | ---: | ---: |
| One shadow | 1.312 ms | 1.321 ms |
| 16 adjacent shadows | 1.297 ms | 4.189 ms |
| 128 repeated overlapping shadows | 1.310 ms | 15.929 ms |

The last row deliberately stresses ordered overlaps and exercises two bounded
intermediate submissions (128 waves, 256 passes). It is not a representative
retail frame. These intervals include query readback and waiting for completion;
they are not release-frame/FPS or 20,000-unit estimates. Every encoder timestamp
pair returned zero on this backend, so GPU-only duration is unavailable. The test
retains raw samples and explicitly reports the limitation rather than inventing
a GPU time. `VERA20K_BRIDGE_SHADOW_PERF_OUTPUT` writes the reproducible receipt. The
[recorded samples](bridge_shadow_render.timing.json) retain every timestamp and
completion interval.


### Live damage and repair

`retail_high_bridge_damage_and_repair_reach_shadow_pixels` loads physical
Hills (wood) and Pacific (concrete), applies damage through the existing HE
receiver, then repairs through an ordinary Engineer command and hut entry.
It observes loaded, damaged, collapsed and repaired live cells, retained atlas
selection, and both GPU attachments. Rendering leaves each stage's simulation
hash unchanged. Damage changes the published instances; collapse removes shadow
coverage; repair restores it. Concrete healthy/damaged masks can produce the
same image, as the original source masks establish.

The [lifecycle receipt](bridge_shadow_render.lifecycle.json) records cells,
instances, simulation and attachment hashes for all eight stages. These are
Rust integration results with shadows isolated against white and clear depth;
they do not claim native map loading, a projectile launch or complete bridge
lifecycle parity. Physical map identities and portable full-app capture
profiles are in the [production packet](bridge_shadow_render.production.json).

### Reproduce the Rust checks

Set `RA2_DIR` to the same physical retail installation. The ordinary suite and
CI clippy set passed with `VERA20K_REQUIRE_RETAIL_INI=1`: **9,567 passed,
0 failed, 231 ignored**; clippy exited 0 with existing repository warnings.
The simulation field ratchet passed at **2,532 baseline/current fields**.
The [validation receipt](bridge_shadow_render.validation.json) records exact
commands, source/log hashes and the corrected lifecycle fixture's separate run.

```sh
VERA20K_REQUIRE_RETAIL_INI=1 python -m tools.cargo_run -- test --workspace --lib
VERA20K_REQUIRE_RETAIL_INI=1 python -m tools.cargo_run -- clippy --workspace --lib --bins --test '*' --examples
VERA20K_REQUIRE_RETAIL_INI=1 python -m tools.cargo_run -- test -p vera20k --lib app::presentation::render::bridge_shadow_gpu_tests:: -- --ignored --nocapture --test-threads=1
```

The GPU gate is explicit because it requires physical retail assets and a real
adapter. Set `VERA20K_BRIDGE_SHADOW_LIFECYCLE_OUTPUT` for stage PNGs/receipts,
and `VERA20K_BRIDGE_SHADOW_PERF_OUTPUT` for timing JSON. Keep previews outside
sealed [map-observation](../map_observation.md) capture bundles. Run the timing
case with an uncontended GPU; it includes setup/wait boundaries stated above.


### Independent review and final candidate

One fresh read-only critic found no blocking correctness defect. Its allocation
finding was fixed in the existing bridge-name classifier: case-insensitive
comparison now avoids allocating once per ordinary named overlay scanned each
frame. Numeric identity semantics and all consumers remain with that owner.
Afterward, **594 focused bridge tests** and **all three GPU correctness tests**
passed. A new retained release replayed the 753-step Hills route and produced an
exact `MATCH` against the validated after capture, including every image byte
and simulation observation. The [production packet](bridge_shadow_render.production.json)
records this final repeat; the [validation receipt](bridge_shadow_render.validation.json)
distinguishes initial full-suite checks from focused post-review validation.

Final release SHA-256:
`6a07e15bbf01369807e4c011dd7a90095a649b20f0518dd15ac3f9ccb3bc88f7`.
The timing table predates this allocation-only refactor; no frame-time saving
is claimed without measurement.


## Paid concrete-wall body and shadow

`python -m tools.spatial_oracle.bridge_shadow_render --concrete-wall --check`
replays the component inputs in `concrete_wall_render.json`. `--write` regenerates
that corpus from the original executable. The selected initial, partially damaged,
stage-two, endpoint and erased cells come from `anytown_damage/concrete_wall.json`;
the normal paid-production Rust path independently reaches these states.

Each row retains the original body comparison, an independent shadow surface,
and body followed by shadow on the same RGB/Z/A stores. The shadow executes
`CellClass::DrawOverlay_Shadow @ 0047F510` through its original indexed destination
blitter. The erased case uses the Tactical admission gate. Physical retail inputs,
component points, clipping and surface initialization are declared by the sidecar.
This bounds cell raster behavior; it does not compare a whole Tactical frame or
execute the native paid Factory lifecycle.

The Rust consumer is
`app::presentation::instances::overlays::ore_gpu_tests::retail_paid_concrete_wall_fire_reaches_index_a_and_native_cell_raster`.
It reads published overlay-index entries and the live grid after ordinary paid
construction and combat, then exercises the existing overlay-body and shared
Cell-shadow production renderers in both sRGB formats. Wall shadows use each
material's actual SHP half-count, independently of DamageLevels, and share the
native traversal order with retained high-bridge shadows.
