# Picture and model art (design, experimental)

VERA-only art formats that replace an SHP with a PNG/APNG picture and a
VXL+HVA with a glTF 2.0 model. This file is the design the implementation
follows; each section that is built says so in [Status](#status).

## Goals

- A modder draws units, buildings and animations in ordinary tools (any image
  editor, Blender) at any resolution, and the game draws them at their own
  resolution when the view is zoomed.
- An art section that does not declare the new art plays, draws and costs
  exactly what it does without this feature. Retail data declares none, so the
  game with retail data is unchanged, frame for frame and pixel for pixel.

Non-goals for now: terrain, tiles, overlays, smudges, particles, `AlphaImage=`,
cameos, the sidebar and the shell UI, skinned or morphing models.

## The switch

The art section decides, nothing else:

- A section with `Picture=` uses that picture and never opens its SHP.
- A section with `Model=` uses that model and never opens its VXL or HVA.
- A section with neither uses the SHP, VXL and HVA as before, and the engine
  never looks for a picture or a model for it, by key or by file name.

There is no player option and no build feature. The simulation takes some
values from the art files (below), so turning the new art off on one machine
would change the game on that machine only. A mod or map turns it on by
declaring it; every player loads the same declaration.

## Keys

All four keys live in the art section that names the image (the section the
SHP or VXL would be read through: `Image=`, or the type's own name). Every
other art key keeps its meaning; frame numbers in a picture are SHP frame
numbers, so `Start=`, `End=`, `LoopStart=`, `Shadow=`, `Sequence=`,
`AnimIdle=`, `WalkFrames=`, `Buildup=`, `BibShape=`, `TurretOffset=`,
`ShadowIndex=` and the rest work unchanged.

| Key | Value | Default |
|---|---|---|
| `Picture=` | file name of a PNG or APNG; theater substitution applies to it as to the SHP name | none (no picture) |
| `PictureSize=W,H` | the frame canvas in game pixels; the picture is a whole multiple of it (1x, 2x, 4x art) | the picture's own size (1x art) |
| `Model=` | file name of a binary glTF 2.0 model (`.glb`) | none (no model) |
| `ModelSize=L` | the length of the model's `body` node along its forward axis, in its rest pose, in leptons (256 per cell); the whole model scales by the same factor | required with `Model=` |

A value out of range is refused with an in-game message naming the section and
key; a refused `Picture=` leaves the section with no art (it draws nothing), as
a missing SHP does today. `Model=` without a valid `ModelSize=` (1 to 1536,
six cells) is refused and the section keeps its VXL path; a declared model
that is missing or unreadable, or lacks a `body` node, draws nothing and its
VXL and HVA are never opened, as a missing VXL does today.

`Model=` takes effect for a type that loads a voxel model, that is whose art
section also says `Voxel=yes`; the section is the one the VXL would be read
through (the rules `Image=`, else the type's own name).

For a building, `Buildup=`, `BibShape=` and a war factory's `DeployingAnim=`,
`RoofDeployingAnim=`, `UnderDoorAnim=` and `UnderRoofDoorAnim=` (SHP names
there, not AnimTypes) name pictures (`NAME.png`) when the building's section
declares `Picture=`; they take its `PictureSize=`.

## Files

### Picture: `NAME.png`

- An APNG with one animation frame per SHP frame, shadow frames included in the
  SHP's order (the back half when the art has shadows). A plain PNG is one frame.
- Frames may be whole-canvas or the optimised regions with blend and dispose
  operations APNG tools write; the engine composites them.
- Straight RGBA. A pixel at least half covered is drawn.

### Mask: `NAME.mask.png`

Beside the picture (same folder or archive), same size and frame count. Red at
128 or more marks house colour, green at 128 or more marks pixels light does
not change. Without a mask the picture has no house colour and no unlit pixels.

### Model: `NAME.glb`

- Nodes by name: `body`, `turret` / `turretN`, `barrel` / `barrelN`, `shadow`.
  `turretN` / `barrelN` stand in for `TURN.VXL` / `BARLN.VXL` (`TurretCount=`).
- Node animations (translation, rotation, scale) stand in for HVA frames.
  A part takes the animation named for it (`body`, `turret`, `barrel1`, ...),
  else the first animation not named for a part. Its frame count is the key
  count of its channel with the most keys, and HVA frame `k` draws at that
  channel's key `k`: export one key per HVA frame. The frame numbers are the
  voxel parts' (`Foot+0x538` modulo the body's count; the turret's own
  `Techno+0x148` modulo its count while the body's frame is 0; the barrel the
  body's). A shadow draws at frame 0, as `0x00707280` samples HVA frame 0.
- Materials: the base colour texture or factor, the normal texture, the emissive
  texture or factor (emissive parts are unlit), `alphaMode`. Metallic and
  roughness are ignored; light follows the game's voxel light.
- The house-colour mask: `NAME.mask.png` beside the model, in the base colour
  texture's UV space, else none.
- Sidecar files beside the model replace embedded images:
  `NAME.texture.png` (base colour), `NAME.normals.png` (normal map, glTF's
  tangent-space convention, +Y up; the tangent frame is taken from the
  screen derivatives of position and UV, so no tangents are needed),
  `NAME.mask.png`.
- `NAMEWO.glb` beside `Model=NAME.glb` is the `NoSpawnAlt=` model, drawn
  where the VXL path draws `%sWO` (a unit whose spawns are all away). It
  scales so its own `body` is `ModelSize=` long and takes its own
  `NAMEWO.*.png` sidecars. Without it the main model draws. It is read only
  for a unit with `NoSpawnAlt=yes` whose art declares `Model=`, and it is
  drawing only: the simulation takes nothing from it.
- Refused: Draco or meshopt compression, KTX2/Basis textures, skins, morph
  targets. Ignored: cameras, lights, scenes other than the default.

## What the simulation takes from the art

Today these values come from SHP/VXL headers at scenario load, through the
owners listed, and enter `RuleSet::simulation_config_hash`. With the new art
the same owners take them from the picture or model instead, read in the core
crate (headless loads too) and hashed the same way. Nothing else in the
simulation reads art.

| Value | Owner today | Source with the new art |
|---|---|---|
| Anim raw frame count (End/LoopEnd defaults, Middle = count/2, PsyDom, Lightning, flaming infantry) | `ArtRegistry::bind_one_anim_asset` (`427B50`) | APNG frame count (`acTL`), 1 for a plain PNG |
| Anim canvas height (WeatherCon bolt Z) | same, `raw_shp_height` (`53A16B`) | `PictureSize` height |
| Middle frame size (Crater/Scorch smudge size) | `ArtRegistry::populate_anim_frame_dims` (`424F00`) | `PictureSize` |
| Buildup frame count and presence (build/sell time, `CanSell`) | `buildup_asset_catalog` (`45F23E`) | the Buildup picture's frame count |
| Body frame count (rubble) | `building_rubble` (`45F160`) | the body picture's frame count |
| Body art present in this theater (random maps) | `map/rmg/tech_catalog` | the declared picture |
| Voxel model loaded (rocking, rockers) | `VoxelAssetCatalog::bind` (`6F9E10`) | `Model=` declared and the file loaded |
| Tilt half sizes (slope tilt, fire coordinates) | `voxel_asset_catalog::voxel_tilt_half_sizes` (`6F3B6C`) | `body` bounds scaled by `ModelSize` |

The tilt half sizes take the VXL section sizes the model stands in for:
`size_x = round(ModelSize * u)` voxels along the body's glTF +Z and
`size_y = round(width / length * ModelSize * u)` across its X, `u` being the
native model units per lepton (`0x00B1D008`, about 0.16573), each held to 1..255;
then `[size_y / 2 + 0.5, size_x / 2]` as for a VXL
(`voxel_asset_catalog::model_tilt_half_sizes`). A unit with indexed guns
(`TurretCount=`) needs a `turretN` node for each of them, as it needs each
`TURN.VXL` (`5F865F`, `5F8ACB`). The config hash takes the model's file name,
`ModelSize=` and a digest of its bytes.

A declared file that is missing on one machine changes these values there, as a
missing SHP does today; the config hash refuses that machine online and in
saves and replays.

## Drawing

The new art never reads an SHP, VXL, HVA, palette or VPL. It reaches the
screen through the blitters every SHP and voxel image already uses, unchanged,
so it sorts, shades and shadows by the same rules:

- **Pictures** become RGBA source pages of the sprite atlas, beside its
  palette-index pages: the blitters' existing RGBA source mode
  (`sprite_source.wgsl`, `source_palette` 0) with an index plane. Each frame is
  shrunk to `PictureSize=` game pixels (a game pixel is drawn when half its
  picture pixels are; its colour is their mean), cropped to its drawn pixels
  like a stored SHP frame, and centred on the canvas the same way. The index
  plane holds 0 for holes, 1 for lit pixels and 240 for the mask's unlit
  pixels, so `PaletteLight` leaves those alone. House-colour pixels take the
  owner's ramp by brightness before upload, one copy per house only for frames
  that have such pixels. The atlas entries are keyed and drawn as an SHP
  frame's: native Z from the drawn rectangle, Display order, `Shadow=` frames,
  darkening counts and translucency all follow without new code.
- **Models** are drawn by the GPU every frame from their meshes into transient
  RGBA images placed the way a voxel image is (`render::model_draw`). Each
  object that draws one asks for its pose while the frame's pieces are built;
  equal poses share an image. One render pass then draws the body, turret and
  barrel meshes of every asked pose, composited into one image as `UnitClass`
  composites its parts (`0x0073B140`), at the matrices the voxel parts would
  take (`ground_unit_pose_matrices`, or the posed body's locomotor arm), in
  the rectangle a voxel draw of that pose takes
  (`native_draw_bounds_from_extents`), onto one RGBA page of the sprite
  atlas held for the purpose. Light is the voxel world light with the
  Blinn-Phong form of `vxl_normals`; the mask's red takes the owner's ramp by
  brightness as a picture's does; the mask's green and emissive texels are
  unlit (index 240). The unit's piece draws that page through the SHP
  blitters' RGBA source mode with the unit's Z-read policy, its Z from the
  drawn rectangle (`ZGradient::Vertical`). Over a bridge, a
  `TooBigToFitUnderBridge=` unit's rectangle is two blits as `0x0073B2FE`
  splits the composite: the rows above the bottom 16 flat and 5 lower, the
  bottom 16 vertical, each seeded on its own (`native_z::voxel_depth_region`).
  Nothing of a pose is kept between
  frames; only each model's meshes and textures stay on the GPU. The page
  starts at 1024 square and doubles after a frame whose images did not fit.
- **Model shadows.** The `shadow` node's meshes, else the body's, at frame
  0, flattened onto their lowest point and taken through the shadow key's
  facing and slope matrix, as `0x00753F90` projects a VXL section's
  bottom-face quad: camera z dropped, the viewer halved for
  `ConsideredAircraft=` (`0x00707319`), then 3 pixels right (the light,
  `0x00887420`). The GPU draws it as a mask on the model page, index 1 where
  shadowed, in the rectangle `0x00754510`'s rule gives its extents. The
  unit's piece hands it to the SHP shadow edit
  (`SpriteAtlasEdit(page, Shadow)`), so darkening counts follow the native
  rule. It takes each voxel shadow path's admission (`NoShadow=`, cloak,
  sinking, Tunnel travel, the atlas's shadow types), anchor, facing, slope,
  sort depth and Z adjustment: an aircraft's (`0x004148A0`), a lifted
  Unit's (`0x0073C1D2..0x0073C5C4`) and a ground Unit's (`0x0073C5C4`). It
  draws before the body, which covers it as the native body mask does.
- **Model space.** glTF +Y is up, +Z forward, +X left. The model draws with
  +Z along the voxel hull's X, X along its Y and +Y along its Z, scaled so
  the `body` length is `ModelSize=` leptons in voxel units (one voxel is one
  screen pixel). The origin is the point the object stands on. A turret turns
  about the vertical through the origin moved forward by `TurretOffset=`, and
  a barrel pitches about the turret's pivot, as `TUR.VXL` and `BARL.VXL` do,
  so turret and barrel meshes are placed as those files place their voxels.
- **Hooks.** The atlas does not register SHP keys for a type that declares
  `Picture=`; it appends the picture pages and entries instead
  (`render::picture_atlas`). No shader, pipeline, bind group layout or draw
  pass changes.
- **Zoomed.** At 1x a picture draws at `PictureSize`. With the view magnified
  (wheel zoom or the Options zoom), chain 6 draws picture and model objects
  again after the magnification at the art's own resolution, depth-tested
  against the battlefield's depth in their shader. Known residual: such an
  object then draws over smoke and explosions in front of it.

## Implementation chains

One chain per PR, in this order. A chain finishes data, drawing, tests and
production validation before the next starts.

1. **Foundation and animations** (`Picture=` on AnimTypes): keys, APNG and mask
   decode, the anim facts in the core crate, picture pages and the picture
   draw for anims, the off-state checks below.
2. **Buildings** (`Picture=`): body, damaged/garrisoned frames, buildup, bib,
   building anims, Z shape, shadows; buildup and rubble facts.
3. **Infantry, SHP vehicles and projectiles** (`Picture=`).
4. **Voxel vehicles and aircraft** (`Model=`): glb decode, GPU mesh draw,
   body/turret/barrel, shadows, slope tilt; model-loaded and tilt facts.
5. **Voxel building turrets, voxel projectiles, debris** (`Model=`).
6. **Zoomed drawing** at window resolution.

## Where the new code meets the old

The only edits to existing files are these hooks; a review compares the diff
against this list.

- Art parsing: the four keys in `ArtRegistry`.
- Each fact owner in the table above: one branch, "the section declares the new
  art", taking the value from the picture or model; the existing arm unchanged.
- Asset lookup: no new probes; a picture or model is opened only by its key.
- The sprite atlas: no SHP keys for a type with `Picture=`; picture pages and
  entries appended. Existing shaders, pipelines, layouts and passes untouched.
- The overlay and bridge atlases: a building whose rubble art names a
  declared picture takes its rubble body and shadow frames from the picture
  (`picture_atlas::rubble_picture`). Those atlases are already RGBA pages
  with an index plane, so the frames share their page and draw path.
- The world zoom pass (chain 6): one call that draws the new art after
  magnification.
- Models: `VoxelAssetCatalog::bind` admits declared models and leaves their
  VXL/HVA requests unopened (`VoxelModelPlan::slot_requests`); the sprite
  atlas holds one model page (`install_model_page`); `BatchRenderer` makes
  its render targets; the unit draw (`instances::units`) hands a type with a
  declared model to `instances::model_units` in place of its VXL parts and
  shadows; `render_game` begins and `dispatch_draw_passes` encodes the
  frame's models (`RendererState::model_frame`). `vxl_raster` exposes the body
  draw matrix and `vxl_normals` the world light; the native model units per
  lepton moved to `map::voxel_pose`. For shadows, node animations and
  `NoSpawnAlt`: `instances::units` asks `model_units::declared_model_of` for
  the drawn type's model (its `WO` name included), passes the voxel shadow
  path's anchor and admission (`model_shadow_draw`) and shares its frame
  rule (`animation_frames_with`, split out of `animation_frames_of`
  unchanged); `unit_atlas::voxel_shadow_viewer` exposes the shadow types'
  gate; `AircraftShadowPose` is `Copy`; `VoxelAssetCatalog` reads
  `NAMEWO.glb` into `DeclaredModel::no_spawn_alt`.

## Verification

### Off: nothing declared (retail data), identical to `main`

| Check | How | Passes when |
|---|---|---|
| No new lookups | a test loads the retail rules through an asset manager that records lookups | no `.png`, `.glb` or `.mask.png` name is looked up |
| Shared code untouched | `git diff main` of existing files against the hook list above | every edit is a listed hook |
| Simulation | `sim-bench --release`, 30-start map, one player against 29 AI, on `main` and on the branch | same final state hash and config hash |
| Replays | play the same replays on both builds | same final hashes |
| Pixels | `tools.map_observation` hidden-window readback at fixed steps: 1x, Options zoom, wheel zoom, on both builds | `frame.bgra` byte-identical |
| Frame time | the same sim-bench run | not slower |
| Browser | the wasm check in [docs/wasm.md](wasm.md#checks) | builds |

The first three run from chain 1 on and again in every chain.

### On: declared art

Per chain, one to three regression tests through production code, for example:
an APNG with an SHP's frame count gives an AnimType the same End, LoopEnd and
Middle as that SHP; a building with a Buildup picture can be sold. Then one
production run on a real map with converted art.

## Status

- Design: this file.
- Chain 1: in progress.
- Chains 4 and 5 (`Model=`), written, not yet compiled or run: keys and the
  model-loaded and tilt facts in the core crate; the GPU draw of voxel units'
  and aircraft's bodies, turrets and barrels, posed by node animations at
  their HVA frame numbers, with normal maps; their shadows; the bridge split
  of their depth rectangle; the `NoSpawnAlt` `WO` model. Not yet: voxel
  building turrets, voxel projectiles and debris (their VXL path stays).
  Known residuals of the model draw: a shadow is the exact footprint of the
  flattened mesh, not the two-pixel-wide columns `0x00756860` marks, and
  takes no body mask (the body drawn after it covers it, so where native
  leaves a high-flying Jumpjet's unmasked shadow over its own body, the model
  shadow stays under it); an animated part's rectangle is the box of its
  nodes' boxes, larger than its rest bounds; alpha-masked texels cast shadow;
  the `WO` model is not in the config hash.
