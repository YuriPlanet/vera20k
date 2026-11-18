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
key; a refused `Picture=` or `Model=` leaves the section with no art (it draws
nothing), as a missing SHP or VXL does today. A file that is missing, cannot
be read or cannot be drawn is told the same way. The match shows these once,
when it has loaded: the first three, then how many more the log file lists.

For a building, `Buildup=` and `BibShape=` name pictures (`NAME.png`) when the
building's section declares `Picture=`; they take its `PictureSize=`.

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
- Materials: the base colour texture or factor, the normal texture, the emissive
  texture or factor (emissive parts are unlit), `alphaMode`. Metallic and
  roughness are ignored; light follows the game's voxel light.
- The house-colour mask: `NAME.mask.png` beside the model, in the base colour
  texture's UV space, else none.
- Sidecar files beside the model replace embedded images:
  `NAME.texture.png` (base colour), `NAME.normals.png` (normal map),
  `NAME.mask.png`.
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
  RGBA images placed the way a voxel image is (chain 4 settles the details).
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
- Chain 6: picture art drawn (`render::picture_zoom`), written but not yet compiled or run; models follow with chain 4.
