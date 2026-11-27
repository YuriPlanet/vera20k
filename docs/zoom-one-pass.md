# One camera for the battlefield (experiment)

VERA-only, experimental, branch `feature/zoom-one-pass`.

## Goal

The battlefield is drawn the way a 3D game's camera draws it: one camera,
one copy of each asset, one draw, at the window's resolution, at any zoom.
Zooming only changes the camera's scale (`zoom` = wheel zoom times the
Options scale); it is continuous, not a set of tiers. Nothing is drawn at
native pixels and then magnified, and nothing is drawn twice. The UI
(sidebar, text, health strips, dialogs) is a layer of its own drawn over
the battlefield at its own scale. At zoom 1 with retail data the frame is
the original's, pixel for pixel.

Phases:

1. One camera at every zoom: the world straight into a window-resolution
   target (Options zoom too), the frame UI drawn over it with a camera that
   scales frame pixels (nearest); continuous wheel zoom, zoom out included.
   The old path goes: `WorldZoomPass`'s magnification and window copy, the
   `Changed` mask, `picture_zoom`'s redraw, the `VERA_ZOOM_ONE_PASS` switch.
2. VERA HD art sampled by the ordinary blitters (voxel HD pages, tile and
   overlay twins, SHP HD twins), replacing the `hd_art`/`hd_terrain`/
   `hd_voxel` redraws over the magnified frame.
3. One copy of each asset: picture pages keep only their HD page (mip level
   by zoom; CPU bounds from the HD frames); `Model=` meshes drawn straight
   into the target.

The sections below record the first step, before phase 1.

## Today

A zoomed match draws the battlefield at native game pixels into a world
target `ceil(frame / z)` big (camera `zoom` forced to 1 by
`BatchRenderer::set_native_world_resolution`), then `WorldZoomPass::composite`
magnifies it into the frame. Picture art (`render::picture_zoom`) and VERA's HD
art (`hd_art`, `hd_terrain`, `hd_voxel`) are drawn a second time over the
magnified frame, reading the world target's depth and colour to find where
the 1x draw passed.

## The experiment

While the switch is on, the wheel zooms in (`z > 1`), the Options zoom is
100% (no window copy) and HD art is off, the battlefield is drawn once,
straight into the frame-sized composition target:

1. **No world zoom pass.** `render_game` leaves `world_zoom_pass` `None`; the
   world camera keeps `screen_size` = the frame and `zoom` = `z`
   (`camera_pos` stays the world pixel at the frame's top-left corner). The
   world depth is the frame-sized depth. Everything the code draws "after
   the composite" into `WorldZoomPass::color_view()` (building status text,
   frame health bars, band box, timers, ...) draws into the composition
   instead, as at zoom 1, with cameras that map world pixels by `z`.
2. **Snap in world pixels, then scale.** Every world blitter's vertex
   position is `floor(pos - camera_pos + 0.5) * zoom`, so a world pixel is
   exactly a `zoom`-sized block whatever the zoom; at zoom 1 this is the
   current rounding. (Today the shaders skip the snap when zoom != 1.)
3. **World pixel of a fragment** is `floor(frag.xy / zoom)` from the
   target's top-left; per-row native Z already comes from the interpolated
   `world_pos`, and the A plane from `tactical_a_at` (already `/ zoom`).
   Anything that uses `position.xy` as a world pixel (texel fetches,
   parity, offsets, masks) divides first.
4. **Pixel art stays blocky.** Palette-index pages are sampled nearest, so
   each world pixel's block shows one texel, as the magnified 1x image did.
5. **Pictures are sharp.** An entry on a picture page with an HD twin
   samples the twin (premultiplied, mipmapped, house layer) at the level
   for `scale / zoom` texels per target pixel, instead of the 1x page; the
   index plane comes from the twin's level-0 indices. The HD redraw pass
   (`picture_zoom`) does not run in this mode.
6. **Destination-reading passes** work in world pixels: surface strokes
   draw `zoom`-sized blocks, combat-light masks and their copy bounds scale
   by `zoom`, packed warp offsets and parity use world pixels, terrain edit
   replay and ion blast take the real zoom, surface lines use their zoom
   mode.

Out of scope for the first step: zooming out (`z < 1`), the Options zoom
above 100% (the world would go to the window), VERA HD art.

## Checks

At zoom 1 nothing changes (retail captures identical). At `z > 1` with the
switch on, the frame should match today's magnified frame except where the
picture art is sharper; compare `map_observation` captures with the switch
off and on.

## Status: destination-reading passes (point 6)

Each pass keeps its zoom-1 shader path; the zoomed path runs only when the
world camera's zoom is not 1 (which today's magnified world pass never is).

- **Surface strokes**: `vs_stroke_block` + `stroke_block_pipeline` (batch.rs)
  draw each walked pixel as a two-triangle quad over its block,
  `(floor(pixel - camera_pos + 0.5) + corner) * zoom`; `draw_surface_strokes`
  picks it when the uploaded world camera's zoom != 1. The A gate already
  divides by the zoom.
- **Combat lights**: the 256x128 mask covers `zoom` times the target area,
  texel `floor((p - origin) / zoom)`; the quad and `light_copy_bounds` scale
  too; RGB565 quantising stays per target pixel.
- **Packed composites**: the world pass draws through the camera at surface
  scale 1 (scratch is the target size), so non-warp blends need nothing. The
  warp reads `floor(p + 0.5 + warp * zoom)` with the parity of the world row
  `floor((y + 0.5) / zoom)`; `warp_param` seeds from world-pixel tactical
  top/left and the pieces' snapped world-pixel left; the read halo is
  `ceil(3 * zoom + 0.5)`. `Params` carries the zoom in its former pad word.
- **Terrain edits**: replay stays off at zoom != 1 (`replays`): overlapping
  edits draw a wave each, and every edit/read-only/depth path reads and writes
  only its own target pixel, so it is correct at any zoom (more passes only).
  `piece_scissor` pads `zoom / 2` (was 0.5) for the world-pixel vertex snap.
- **Ion blast**: unchanged; already zoom-parameterised (logical pixel
  `floor(p / zoom)`, moves `round(move * zoom)`), prepared with
  `world_camera.zoom`. Exact at integer zoom; at a fractional zoom a moved
  read can land one target pixel off near a block edge.
- **Surface lines**: `tactical_surface_line_viewport` takes the uploaded world
  camera's zoom (1 unless one-pass), keeping the logical clip
  (`tactical / zoom`, which is the same logical lattice in both modes); the
  zoom mode expands each logical pixel to its block, Z tests read the target
  pixel's depth, gate/source reads sit at the block centre.

## Status: point 5 (sharp pictures)

- The page layout (group 1) has two more bindings for every page: 4, a
  house layer, and 5, a "1x page"; ordinary pages bind 1x1 placeholders
  (`BatchRenderer::default_source_house` / `default_source_low_page`).
- A picture page with an HD twin gets an HD binding
  (`BatchRenderer::create_picture_hd_binding`, `PictureHdPage::one_pass_binding`):
  the twin's premultiplied mipmapped colour at 0 with a linear, mip-linear
  sampler at 1, its level-0 indices at 3, its house layer at 4 (placeholder
  for the model page's twin) and the 1x page itself at 5 (the house ramp
  strip stays there).
- The choice is made at bind time, with no instance flag:
  `SpriteAtlas::bind_one_pass_hd(device, batch, on)` swaps each such page's
  `texture.bind_group` for its HD binding when `on`, and puts every 1x binding
  back when not, so every draw that binds a page (draw passes, merge passes,
  terrain picture, fogged sources) reads the twin without changes there.
  **The frame must call it once before the world draws, after
  `begin_model_frame`, with `one_pass_zoom::active(..)`** (not wired yet; the
  render code owns that switch).
- `sprite_source.wgsl` knows an HD binding by binding 5 being larger than 1x1.
  `sampled_source_color` samples the twin at lod = log2(twin texels per
  target pixel) from the UV derivatives (= log2(scale / zoom) for an unscaled
  quad), clamped to its levels; `source_color` loads level 0. Both
  unpremultiply, add the house colour by coverage as `picture_zoom.wgsl` does,
  return alpha 1, and return a hole where coverage < 0.5 or the nearest
  level-0 index is 0 (the caller's index comes from the twin's index plane).
- With `on` false, or at zoom 1, nothing differs: the same bind groups (plus
  the placeholders) and the same shader results.
- Not covered: the interface strips' HD pages (selection overlay, beacons).

## Status: frame flow and vertex snap (points 1-3)

- `render_game` computes `window_copy` before choosing, and with
  `one_pass_zoom::active(z, window_copy, hd_art_on)` leaves
  `world_zoom_pass` `None` and skips `set_native_world_resolution`: the world
  camera keeps the frame's size and zoom `z`. With no world pass every world
  draw already takes the frame-sized `depth_view`, the frame's tactical rect
  as its scissor and the composition as its target; the composite, the HD art
  and `picture_zoom`'s redraw (`prepare` clears without a world pass) do not
  run. The same frame calls `SpriteAtlas::bind_one_pass_hd(.., one_pass)`
  right after `begin_model_frame`.
- `tactical_world_view` without a world pass divides the frame's
  battlefield by the zoom (`world_pixels`, the `WorldZoomPass::world_clip`
  rounding), so object admission, Z seeds and the action/rally stores keep
  world pixels. At zoom 1 it is unchanged.
- World blitters (`batch_shader` `vs_main`/`vs_depth`, `zsprite_shader`,
  `zdepth_shader`, `sprite_voxel_shader`) place corners at
  `floor(pos - camera_pos + 0.5) * zoom`. Their fragments read
  `position.xy` only through `tactical_a_at` (already `/ zoom`); Z rows,
  z-shapes and sinking rows come from the interpolated `world_pos`.
- Frame health strips stay deferred (`defer_health_to_frame`, by `z != 1`):
  their producer puts them on the frame's lattice, so they draw through
  `batch_shader`'s `vs_frame` (`frame_passthrough_pipeline`), unsnapped, as
  today after the composite.
- Without a world pass the frame-pixel UI draws in its native place:
  building status text after Ground2, now with the UI camera (its producer
  projects by the zoom; at zoom 1 the UI camera is the world camera's
  projection), the band box between the rally passes, bounty text and beacon
  labels through the UI camera as before. Timers, outcome and layout art and
  the chrono capture go to the composition as at zoom 1.
- Terrain picture reuse is off at zoom != 1 (`shift_for`); the shroud A plane
  is composed at `frame / zoom` and read through `tactical_a_at`, so it needs
  nothing.

Left:

- An instance at a fractional world position or size interpolates its UV and
  `world_pos` across a block, so one world pixel's block can show two texels
  or Z rows where the magnified frame showed one. Integer-placed quads (the
  native blits) are exact.
- Building status text and the band box draw in their native order (under
  Air/Top, and the rally passes), not over the finished world as the world
  pass draws them.
- No HD redraw for the interface strips (beacons, PLACE cells).

## Status: phase 1 camera

- **One zoom model.** `ViewDistanceTier`, `cycle_view_distance` and replay's
  `apply_zoom` are gone; `camera::wheel_zoom(input, lines)` serves the live
  wheel (with `MouseWheelZoom=` on, cursor over the battlefield) and replay
  alike. Each notch multiplies `zoom_target` by 2^(1/4) (four notches double
  it); trackpad pixel deltas already arrive as fractional lines (`/ 30`,
  clamped to ±3), so they zoom by the same factor per line.
- **Anchor: the cursor.** The world point under the cursor stays under it while
  `animate_zoom` eases (`ZOOM_EASE` 0.35 a frame, snap at 0.002), as a 3D
  camera dollies toward its aim. The cursor and the stored camera share the
  frame's corner, so `cursor / zoom + camera` is that point in any layout.
  The resets (middle button, wheel zoom turned off, a capture's `wheel_zoom`,
  HD art's start) ease around the battlefield's centre
  (`ease_zoom_around`), the middle button and turning it off back to 1.
- **Range** `MIN_ZOOM` 0.5 ..= `MAX_ZOOM` 4 (was 0.25..4 for replays and
  tiers 2/3..2 live), continuous. `constrain_zoom_to_texture` still raises
  the minimum so `frame / zoom` fits one A texture (only above a
  `limit / 2` frame). Saved games keep their float zoom: any positive zoom up
  to 4 loads (replays and the HD capture zoom reached 0.25) and the restore
  clamps it into the range; the options only store the `MouseWheelZoom=` bool, so there
  is no tier to migrate.
- **Input stays in frame pixels / wheel zoom.** Every CPU screen-world
  conversion (`screen_point_to_world`, band box, spawn pointer, planning
  points, touch pan) is `frame_px / zoom_level + camera`, correct at any
  continuous zoom. The cursor is kept in frame pixels
  (`ZoomGeometry::window_to_source`). With the render camera at window
  resolution and `zoom` = wheel × Options scale this holds only if the render
  camera maps world `w` to window `origin + (w - camera) * zoom_level * scale`
  with `camera` = `input.camera_*` (world at the frame's corner) and
  `origin`, `scale` the same `ZoomGeometry` the cursor uses.
- **A plane.** Sized `ceil(frame / zoom_level)`; it must keep being given the
  frame size and the wheel zoom (the clamp's pair), not the window and the
  product, or the float ratio can land one texel over the texture limit the
  clamp checked (an assert). During an ease the plane is reallocated each
  frame its size changes, as with the tiers.
- **Tools.** `presentation.wheel_zoom` takes any view distance 0.25..2
  (1 / the zoom range); old profiles' 0.8, 1, 1.25, 1.5 keep working. The
  capture eases to it around the centre and checks it settled.
  `VERA20K_HD_CAPTURE_ZOOM` clamps to 50..400.

## Status: phase 2 HD art

The `hd_art` / `hd_terrain` / `hd_voxel` redraws over the magnified frame are
gone (pipelines, piece types, colour-match gates, inferred light); the world
blitters sample the HD art themselves. The pack, its loading, `HdArt=` and
`hd_switch` stay. Light, Z, palette rows, house ramps and silhouettes are
the 1x pixel's in every case; only the colour (or voxel index) is HD.

- **One hook.** `hd_art::bind_hd_twins(device, queue, batch, sprites, tiles,
  overlays, camera_zoom, hd_art_on)`, once a frame before the world's
  instances are built and drawn. Twins are on when `camera_zoom > 1`, HD art
  is on and the device is not the WebGL2 fallback (`hd_art::twins_on`); off,
  every page has its 1x binding back. Picture twins keep
  `SpriteAtlas::bind_one_pass_hd`.
- **Twins.** Every twin has two levels, 2x and 1x (box-filtered,
  premultiplied by the 1x coverage), sampled linearly at
  lod = log2(twin texels per target pixel) from the UV derivatives, so a zoom
  between 1 and 2 blends them and 2 or more magnifies level 0 bilinearly.
  A twin texel takes its 1x pixel's coverage; where the pack art does not
  cover it, the 1x colour.
- **SHP frames** (`hd_art::ShpHd`): no full-page twins (a 4096 growth page's
  would be 341 MiB). A frame drawn while on is looked up (its key goes on a
  demand list from `SpriteAtlas::get`), read from `<pack>/<hash>.png` by the
  pack reader threads (`render::hd_loader`, 256 in flight; at once in a
  capture), and once back (32 MiB of pictures a frame) put on an HD page of
  the sprite atlas
  (at most 4 of 2048 px, each with a 4096 twin): the 1x indices at its place,
  bound like an indexed page (palette rows at 0, indices at 3) with the twin
  at 4 and a linear sampler (`BatchRenderer::create_shp_hd_binding`).
  `SpriteAtlas::get` then hands out the 1x entry moved to that page, so every
  draw that binds a page reads it. The twin stores the art against the
  pack's `reference.pal` at each texel's 1x index (`compose_shp_twin`):
  difference halved around 128, or for 16..31 the peak ratio over 2.5;
  `sprite_source.wgsl` `shp_hd_color` adds it to (or scales) the row's own
  colour, the old redraw's theater/house correction. Frames never move once
  placed; past 4 pages they stay 1x.
- **Tiles and overlays** (`hd_terrain::RgbaTwin`): with a pack, each atlas
  keeps its 1x RGBA and makes a same-layout twin the first frame it is on
  (each picture doubled, nearest), then every picture takes its
  `<pack>/terrain/<hash>.png` as the reader threads bring it back (512 in
  flight, 16 MiB a frame; at once in a capture). The tile atlas's `zdepth_bind_group` becomes
  the twin with a linear sampler beside the 1x depth plane
  (`create_zdepth_hd_bind_group`; `zdepth_shader.wgsl` tells it by the page
  being larger than the depth plane and reads depth by texel). The overlay
  atlas binds the twin as an RGBA HD binding (`create_picture_hd_binding`
  with its own index plane and the page as the 1x page). A twin past the
  device's texture limit is left out (logged); the page stays 1x.
- **Voxels**: the unit atlas and the pose cache bind their HD index pages
  (`VxlHd`) at group 1 binding 1 beside the 1x page once they exist
  (`BatchRenderer::bind_unit_atlas_hd`); `sprite_voxel_shader.wgsl`
  `voxel_byte_at` (also the packed voxel raster) reads the HD index while
  `camera.zoom > 1`, the 1x one where the HD index is 0 or the 1x one is.
  Not tied to the HD switch, and no hook.
- **Packed SHP sources** sample the twin texel under their UV
  (`source_color_at`); other texel-load callers (`terrain_edit`) read the
  twin texel at the index texel's top-left.

Visible differences from the redraw: HD art shows wherever the 1x frame
draws, also under units, effects and fog edges that the colour gate used to
skip; light comes from the blit's own palette light, not a ratio inferred
from the composed pixel (so cell light and shadows step per world pixel, not
between HD pixels); silhouettes are the 1x ones at HD resolution; no debug
tint on voxels. Residuals: bilinear filtering mixes neighbouring 1x indices'
differences at index edges; overlay alpha is taken as coverage (opaque
above half).

## Status: phase 1 composition

One camera draws the battlefield at the window's pixels at every zoom; the
switch, the world pass and the window copies are gone.

- **The composition is the frame's rect in the window**
  (`ZoomGeometry::frame_size`, `RendererState::composition_size`): the window
  without an Options zoom, else `floor(frame * scale)`. `render_game` uploads
  the world camera through `BatchRenderer::update_match_camera`:
  `screen_size` = the composition, `zoom` = wheel zoom times the Options
  scale, `camera_pos` as before (the world pixel at the frame's corner, which
  is the composition's corner). World pixels stay `frame / wheel zoom`, so
  instances, the A plane, Z rows and the surface stores count as before; only
  their blocks grow. The composition (`CombatLightRenderer`), the terrain
  scratch and the light masks follow the camera; the depth is
  `UpscalePass::depth_view`, sized to the composition, swapped in for the
  frame as before.
- **Frame UI scales by the Options zoom.** The UI camera is the world camera
  at zoom = the Options scale (`ui_camera(world, zoom)`), so every UI draw
  (band box, status and bounty text, beacon labels, timers, outcome art,
  layout art, Phobos SW sidebar, module panels, messages, the in-match page)
  lands frame pixel `f` at `f * scale`, each a whole block snapped like a
  world pixel, straight over the world, blending with it where it blends.
  Health strips draw through `vs_frame` with a camera at the world camera's
  zoom. Scissors go through `RendererState::frame_to_composition` (edges
  scaled and rounded, so the battlefield and the column still meet).
- **The sidebar copies at the window's pixels** (`SidebarSurface::place` at
  `window_copy_scale`, `composite(pass, scale, target)`); its surface cameras
  keep zoom 1. Its text and the battlefield's window text still go to the
  window after the composition (`window_ui`), with the software cursor last.
- **Presenting**: `UpscalePass::present` clears the window to black and
  copies the composition to the frame's origin (no resampling). Without a
  zoom pass the composition is the window and is copied as before.
- **Chrono screen** snapshots and warps the composition with the battlefield
  rect in its pixels; the bar rows the warp leaves are 32 times the scale.
- **Retained composition**: the screenshot cache holds the composition;
  `encode_previous_composition` copies it, presents it into the client, or,
  for a shell painting at frame pixels over the match (saved-load mode 2),
  takes it back to frame pixels (`UpscalePass::shrink`). The active-game
  shells themselves still paint at frame pixels and are scaled by
  `UpscalePass::draw`.
- **HD twins** bind whenever the camera's zoom is above 1: the sprite atlas
  pages, and now the interface strips (`SelectionOverlay::bind_one_pass_hd`,
  `BeaconArt::bind_one_pass_hd`, by the same bind-group swap).
- **Deleted**: `one_pass_zoom` (`VERA_ZOOM_ONE_PASS`), `WorldZoomPass`,
  `set_native_world_resolution`, the `Changed` and `Depth` window masks
  (`MaskedCopy`), both `draw_window_copy`s, `picture_zoom`'s redraw and
  shader (the module keeps only `PictureHdPage`, which the atlases name), the
  HD art redraw calls (`// HD art: wired by phase 2` marks the hook).

At Options 100% and wheel zoom 1 the camera, the UI camera (zoom 1), the
composition (the frame), every scissor and every draw are the original's; the
4:3 bars' copy is a whole-pixel copy.

Left:

- Frame UI at a fractional Options scale (150%) is nearest blocks of uneven
  width, not the old box-filtered seams; text stays sharp in the window.
- The saved-load box over a zoomed match shrinks the retained composition to
  frame pixels and scales it back up (soft).
- The fractional-position residual of the first step now applies at every
  zoom above or below 1, Options zoom included.

## Status: phase 3b models

`Model=` meshes are drawn at the camera's scale: one model page, no HD twin.

- **Scale.** `instances::begin_model_frame(state, world_camera.zoom)` hands
  `ModelFrame::begin_frame` the world camera's zoom (wheel zoom times the
  Options scale). `model_draw::page_scale` draws the page at that zoom,
  exactly, at least 1: an image of `w x h` game pixels is `w * zoom` by
  `h * zoom` texels (slot `ceil` of that, placed by the shelf in texels),
  the meshes and shadow masks projected at that scale (`page_clip` takes
  it), its UV size `w * zoom / page`. The blitters give each game pixel
  `zoom` target pixels, so they read one texel per target pixel through the
  page's ordinary binding (nearest, level 0, index plane at the colour's
  resolution): sharp at any zoom, whole or fractional, with no derivative
  lod. `ModelImage::rect` and everything native rules take from it (native
  Z, Display order, shadow crop, the bridge split, darkening through the
  shadow edit) stay in game pixels. Zoom 1 and below: the page is the old
  one, texel for texel.
- **Size and caps.** `wanted` (game pixels) starts at 1024, doubles after an
  overflow, halves after 600 frames under an eighth full. The texture is
  allocated for the zoom rounded up to a quarter (one reallocation per
  quarter of an easing zoom, the images still drawn at the exact zoom), at
  most `MAX_SCALED_SIDE` 4096 texels (or `wanted` itself if larger) and
  `max_texture_dimension_2d`; past it the scale drops to what fits (a 2048
  page draws at 2x, a 4096 one at 1x), and nearest magnifies the rest.
- **Cost.** Vertex work is per pose, unchanged. Fragment work and texels
  grow with `zoom^2` per visible model, but the world in the window shrinks
  by the same factor, so the page's drawn texels are about the window pixels
  models cover. Memory: 4096^2 RGBA8 + R8 + Depth32 = 144 MiB at the cap (the
  old 4x twin of a 1024 page was the same plus the 1x page); one pass, not
  two.
- **Gone**: `model_draw::HdTwin`, the twin's second pass and its
  `set_picture_hd_page` calls; the model page is never among the atlas's
  `picture_hd_pages`, so `bind_one_pass_hd` leaves its binding alone.
  `PictureHdPage::from_views` and `SpriteAtlas::set_picture_hd_page` have no
  caller left (owned by the picture pages' change).
- **Posed voxels** (`unit_pose_cache`, `vxl_raster::set_hd_poses`) keep their
  2x CPU raster. The same approach would fit them: the GPU voxel raster
  (`voxel_gpu`) already draws posed images every frame onto frame pages;
  drawing them at `ceil(zoom)` into an index page bound at group 1
  binding 1 (`bind_unit_atlas_hd`, which `voxel_byte_at` reads above zoom 1)
  would replace the CPU 2x path, but voxels would then show finer splats
  than retail's, so they stay as they are.

Left: above the cap a model is magnified nearest (blocky by `zoom / scale`);
at a fractional zoom an image's sampling phase can shift by up to a texel
with its block's snap; the WebGL2 fallback draws scaled pages too (no
separate cap).

## Status: phase 3a pictures

Each picture frame exists once on the GPU, at its own resolution, and every
draw samples the level its camera needs, at every zoom (1 and below too).

- **One page.** A sprite-atlas picture page is `picture_atlas::PicturePage`:
  premultiplied encoded colour with mip levels down to one texel per game
  pixel plus `ZOOM_OUT_LEVELS` (1, for `MIN_ZOOM` 0.5), its level-0 index
  plane and, with house-colour frames, its house layer. 1x art's page is
  its game pixels with the zoom-out level. Pages of one page scale each,
  `PAGE_GAP` (2) empty game pixels around every frame, sides a multiple of
  the last level's texel; UVs are game pixels over the page's game-pixel
  extent (`uv_extent`). The 1x RGBA page, its upload and packing, the HD
  twins and the bind swap are gone: `SpriteAtlas::bind_one_pass_hd`,
  `picture_hd_pages`, `one_pass_hd`, `set_picture_hd_page`,
  `picture_hd_page`, `has_picture_hd_pages`, `PictureHdPage::from_views`
  and `one_pass_binding`, `HdSprite`/`HdPage`, and the strips'
  `CenteredShapeStrip`/`SelectionOverlay`/`BeaconArt::bind_one_pass_hd`
  (the frame's calls in `render_game` with them).
- **Non-power-of-two scales** (3, 5, 6, 7) are resampled once at load to
  the next power of two (`picture_atlas::page_scale`): each new texel the
  area-weighted mean of the picture pixels it overlaps (so a game pixel's
  mean and coverage are kept exactly), the index plane nearest. Halving then
  reaches one texel per game pixel exactly. Cost: up to (8/5)^2 = 2.56x the
  texels for 5x art.
- **Binding.** A picture page's only binding
  (`BatchRenderer::create_picture_hd_binding`) binds the colour with a
  linear, mip-linear sampler, the index plane, the house layer (or the 1x1
  placeholder) and, at binding 5, the renderer's house ramps texture
  (256x16 sRGB, `write_picture_ramps`, written from the atlas's
  `PreparedPictures::ramps`). `sprite_source.wgsl` knows an RGBA HD page by
  binding 5 being larger than 1x1 (`source_ramps`); every plain page (the
  model page included: RGBA and index plane at one resolution, nearest)
  binds 1x1 placeholders there. The overlay atlas's HD art twin binds the
  ramps too. The plain RGBA path has no picture house colour any more.
- **Holes and house colour** as phase 1's HD branch: coverage < 0.5 at the
  sampled level, or a nearest level-0 index of 0, is a hole; a house texel is
  black and takes the ramp colour from binding 5 by its filtered coverage and
  step.
- **CPU metadata** stays the CPU shrink (`GamePictureFrames::shrink`):
  drawn rectangles, `frame_bounds`, `building_render_bounds`,
  `stored_frame`, and `sprite()`'s game-pixel RGBA and classes for the
  CPU consumers (bridge shadow stencils, overlay/rubble/terrain/smudge
  sprites, cameos, the radar beacon). The GPU page comes from
  `page_sprite()`, the same crop at the picture's resolution.
- **Interface strips** (`CenteredShapeStrip::from_picture_on_device`:
  selection overlay strips, beacons, the placement preview) are one picture
  page each, frames on its top row, sampled by lod like the atlas's.
- **WebGL2**: picture pages are built there too now (the HD branch has GLSL
  ES forms).
- **At zoom 1** an art-pack frame is the page's game-pixel mip (the box mean
  of its texels, bilinear at the texel centre) instead of the CPU's
  coverage-weighted shrink: a game pixel is drawn by the same coverage rule;
  its class is the level-0 texel's under the pixel centre, not the
  majority, so edges and unlit/house borders can move by a pixel.

Left:

- The overlay, bridge and sidebar cameo atlases keep their pictures at game
  pixels on their single 1x pages (their only copy; none had an HD twin).
  Moving them to picture pages needs a page per entry through the terrain
  draw's fixed atlas slots, the packed and merge passes and the cameo
  batch. Cameos are drawn into the sidebar surface at frame pixels, so
  their 1x copy is the level that surface needs anyway.
- Packed SHP sources (`terrain_packed_shp.wgsl`, `source_color_at`) and
  other texel-load callers read a picture page's level-0 texel under the
  UV, not a filtered level: zoomed out or at zoom 1 a picture there is
  point-sampled.
- Destination-edit replay (`terrain_draw::replays`) needs one texel per
  pixel, so it stays off for picture pages above 1x (more passes only).

## Status: picture streaming

Phase 3a held every picture frame at its own resolution from load: a 4x
art pack (`yurihd`) filled 6 pages of 16384x16000 texels (about 9 bytes a
texel with mips, index plane and house layer: some 14 GB of GPU memory and
several GB of CPU memory while packing), loaded in 140 s, swapped at 1-2
fps, and 12 animations were refused by the decoder's 512 MiB cap. Now the
pictures stream like a 3D game's textures (`render::picture_stream`):

- **Resident level only at load.** Every frame the atlas needs is on a
  scale-1 picture page (the existing `PicturePage` path: its zoom-out mip,
  index plane, house layer), built from the game-pixel shrink.
  `GamePictureFrames` no longer keeps the HD frames: `stream` reads the
  file and its mask a frame at a time in lockstep
  (`picture_atlas::PictureFrameStream` over `picture_file::FrameStream`),
  shrinks each frame and drops it, so no picture is refused for size. It
  keeps a handle on the file (`PictureBytes`: `AssetManager::get_shared`
  for the picture and the mask, no copy). `page_sprite` is now the
  game-pixel page sprite; `hd_page_sprites` streams every frame at the
  picture's resolution once for the interface strips (selection overlay,
  beacons, placement preview), the only load-time HD users. Overlay,
  bridge, cameo and rubble art use the game pixels only; the overlay radar
  colours stream too.
- **Demand.** `SpriteAtlas::get` asks `PictureStream::entry` first: while
  active (world camera zoom above 1, threads available) a key of a frame
  of art above 1x returns its HD entry if the frame is on an HD page (and
  stamps that page's last use, an atomic), else it moves the frame's slot
  from idle to wanted (an atomic compare-exchange; only then a mutex'd
  push) and the game-pixel entry is drawn. HD entries are the game-pixel
  entry with only `page` and UV changed.
- **Hook.** `SpriteAtlas::stream_pictures(device, queue, batch, zoom)`,
  called by `render_game` right after `bind_hd_twins`. Inactive: wanted
  slots go back to idle, nothing is uploaded (running jobs wait on the
  full result channel). Active: wanted slots are grouped by file; a file
  with no running job gets one (at most 4 jobs at once) on two decode
  threads (`std::thread`, a job channel, a result `sync_channel` of 8
  tiles, so decoded art never piles up). A job streams the file up to its
  last wanted frame and, for each wanted frame, builds its HD tile:
  `hd_page_sprite` (the one crop/class/house/resample implementation,
  shared with `page_sprite` through `crop_page_sprite`), composed into a
  tile with `PAGE_GAP` around it rounded to the last level's texel, with
  its colour and house mip chains (`PicturePage::mip_chain`) and index
  plane. The hook takes up to 16 MiB of tiles a frame and writes them
  (`PictureHdPage::write`, `write_texture` per level). A job's end logs
  one line (file, frames decoded, ms); a failed one keeps its frames at
  game pixels. Worker panics are caught.
- **Pages.** `PagePool`: pages of 2048 texels (or the device limit), one
  page scale each, shelf-packed in game pixels, made by
  `PictureHdPage::empty` (colour with `page_levels(scale)` mips, R8 index
  plane, RG8 house layer, `COPY_DST`) and bound through
  `create_picture_hd_binding` as new pages among the atlas's pages (and in
  `picture_pages`). Budget 1 GiB (`VERA_PICTURE_HD_BUDGET_MB`), about 36
  MiB a page. Full: the page whose last use is oldest, and not this or the
  last frame, is emptied (its frames idle again, their keys back on their
  game-pixel entries) and reused, made again for another scale if need be;
  a tile with no evictable page waits until the next eviction or summary.
  A frame wider or taller than a page (512 game pixels at 4x, 256 at 8x)
  stays at game pixels (logged once per file).
- **Summary**: every 600 frames while anything is held or waiting, one
  line: pages, MiB of the budget, frames HD, decoding, wanted.
- **Capture** (`VERA20K_HD_CAPTURE_ZOOM`): the hook waits for the frame's
  jobs and uploads without a budget.
- **Browser build**: no threads; pictures draw from their game-pixel pages
  at every zoom.
- **Tests**: `a_4x_picture_keeps_game_pixels_and_streams_its_own_resolution`
  (picture_atlas), `a_4x_picture_is_paged_at_game_pixels_and_streams_its_resolution`
  (sprite atlas prepare), `a_full_pool_evicts_its_least_recently_drawn_page`
  and `a_drawn_frame_is_wanted_then_redirected_with_its_own_offsets`
  (picture_stream).

Expected for the 4x pack: the resident pages are the HD pages' texels over
16 (about 0.1 G texels, under 1 GB of GPU memory with mips and house
layer) plus at most the 1 GiB HD budget; load decodes and shrinks each file
once, a frame at a time, with no HD packing. Zoomed in, a frame shows its
game-pixel page magnified until its tile arrives (a few frames for a
building, longer for a late frame of a long animation, which is decoded
from its first frame).

Left: an HD entry handed out stays valid only until its page is evicted;
every draw takes entries each frame, so nothing should keep one across
frames (none does today). Decoding a late frame of a long animation reads
all frames before it (APNG compositing), once per job.
