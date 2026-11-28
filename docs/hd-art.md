# HD art

A VERA-only switch for the battlefield and the sidebar. With HD art on and an
HD art pack installed, the battlefield draws buildings, infantry, effects, the
map's tiles, overlays, trees and voxel units from 2x art (upscaled by AI, for
example), at every mouse-wheel zoom and sharpest at 200%, and the sidebar
draws its cameos and frame from HD pictures. gamemd has nothing like it. It
is off by default; off, or without a pack, the game draws the original
picture, byte for byte.

It only touches the battlefield and the sidebar. Health bars, timers,
messages, tooltips, menus and the cursor stay as they are. It is
presentation only: nothing reaches the simulation, a save, a replay or the
network, so players in one match can use different settings.

## Install an HD art pack

An HD art pack installs like any mod: a folder in `Mods` (docs/modding.md)
holding `hd_art/pack`, with an optional `mod.ini`:

```
Mods/
  My HD Pack/
    mod.ini
    hd_art/
      pack/
        reference.pal
        <hash>.png ...
        terrain/<hash>.png ...
        sidebar/<hash>.png ...
```

Pick it in the Mods menu, or import its ZIP with Add Mod. A gameplay mod can
ship its own `hd_art/pack` the same way, for its own art. The selected mod's
pack is the one drawn; `VERA20K_HD_ART_PACK=<folder>` replaces it for one
process.

HD art can be on only while the running mod brings a pack. Without one the
Options list offers Off alone and its help says an HD art pack mod is needed,
Page Up in a match says the same and changes nothing, and a saved `HdArt=yes`
is kept but draws nothing until a pack runs.

The pre-match check of a mod's files leaves `hd_art` out (`app::mod_files`):
players with and without a pack play together, and the pictures are not read
before a match. A pack only changes what one player sees.

## Options and the HD art key

Options' **HD art** list (General page; Extensions on the launcher) is Off or
On, applied to the next frame. In a match, **Page Up** (Keyboard Options,
VERA category: HD art) flips the same setting at once and saves it; the zoom
stays where it is. The mouse wheel's view distances go to 150% and 200%, and
a match starts at 200% while HD art is on. Voxels keep making their HD
pictures once HD art has been on in a run, so turning it on again shows them
at once.

## Settings

`[VERA] HdArt=yes|no` in `VERA.INI` (the settings folder's profile), `no` by
default.

## Captures

`VERA20K_HD_ART=0|1|debug` replaces the profile's switch for one process, so a
tactical capture (which runs with a private settings folder) can take before
and after pictures with one build. `debug` is on with the HD art tinted
green, the HD voxels cyan and the HD terrain magenta.
`VERA20K_HD_CAPTURE_ZOOM=<percent>` (25 to 400) holds the wheel zoom, since
captures have no wheel gesture.

## HD art

The battlefield draws once, at the window's resolution, through a camera
that carries the wheel zoom (docs/zoom-one-pass.md). While that camera
zooms in (above 100%) with HD art on, the ordinary world blitters sample the
HD art in place of the 1x art (`render::hd_art::bind_hd_twins`, once a
frame): the same instances, the same light, Z, palette rows, house ramps and
silhouettes, only the colour comes from the art. Each HD copy has a 2x and a
1x level, so a zoom between 100% and 200% blends them and 200% or more
magnifies the 2x one. HD art shows wherever the 1x frame draws, also under
units, effects and fog edges. Not on the WebGL2 fallback.

The art lives in a pack folder ([Install an HD art pack](#install-an-hd-art-pack)). It holds `reference.pal` and one `<hash>.png` per
frame, exactly 2x the frame's stored rectangle, alpha for coverage. The hash
is FNV-1a 64 over the frame's width and height (u16 little-endian) and its
palette indices, so a frame finds its art whatever type, theater or mod draws
it. `VERA20K_HD_DUMP=<folder>` writes `<hash>.idx` (u16 width, u16 height,
indices) for every such frame on screen the pack lacks; an offline
upscaler turns those into pack files.

Every indexed frame of the SHP sprite atlas takes part: buildings, their
animations and bibs, infantry, and the effects (explosions, fire, smoke,
debris, muzzle flashes, SHP projectiles and particles). A frame drawn while
zoomed in is asked for (`hd_art::ShpHd`); desktop reader threads
(`render::hd_loader`) open and decode its picture, and once it is back it
goes on an HD page of the sprite atlas (at most four 2048x2048 pages, each
with its 4096x4096 2x copy): the frame's 1x indices at its place, and the
art stored against the pack's reference palette at each texel's 1x index,
so the draw moves it to the frame's real palette row (theater colours by
difference, house colour by brightness along the house ramp). The frame
draws 1x until then, and from that page afterwards. Frames never move once
placed; past the four pages they stay 1x. Single-frame captures read the
pictures on screen at once.

## HD voxels

Every voxel picture the unit atlas rasterizes (vehicle bodies, turrets and
barrels, aircraft, building voxel turrets) also gets an index picture at
twice the resolution (`vxl_raster::VxlHd`): the same native stores, in the
same order, each covering two by two pixels at the half pixel its 8.8
cursor reached. A model has more voxels than its native picture has pixels
(a Grizzly hull is 59 voxels long on about 45 pixels), so the zoomed picture
shows detail the native one rounds away. The HD pages are bound beside the
1x ones (`BatchRenderer::bind_unit_atlas_hd`), and the voxel draw reads the
HD index while the camera zooms in, the 1x one where the HD one has none;
palette, house ramp and light are the voxel draw's own. Posed pictures
(crashing, rocking or tilted bodies, pitched rockets, voxel debris:
`render::unit_pose_cache`) are rasterized on the CPU while the camera zooms
in (`vxl_raster::hd_poses`), so they get their HD picture too; at 100% or
below they stay on the GPU raster. Voxel shadows stay native.

## HD terrain

`render::hd_terrain` does for the ground what the HD art does for
buildings. With a pack, the tile atlas and the overlay atlas each make a
copy of their page at twice the resolution in the same layout
(`hd_terrain::RgbaTwin`) the first frame they are zoomed in: every picture
doubled at first, then the pack's picture where it has one, with the 1x
picture's coverage. While zoomed in, the tile and overlay draws read that
copy in place of their page; cell lighting, shadows and Z stay the draw's
own. A copy past the GPU's texture limit is left out, and its page stays 1x.

The pictures are in the pack's `terrain` folder: `<hash>.png`, exactly 2x
the tile's rectangle, the hash being FNV-1a 64 over the tile's width and
height (u16 little-endian) and the RGBA the engine colours it with.
`VERA20K_HD_DUMP_TERRAIN=<folder>` makes a map load
write every tile of its theater the pack lacks as `<hash>.rgba` (u16 width,
u16 height, RGBA) for an offline upscaler. Loading one map per theater
collects them all. Upscale each tile padded with copies of itself on the
isometric lattice, so tiles meet without seams. The pictures are read by
reader threads as the art's are, a few hundred at a time, and take their
place on the copy as they come back.

The overlay atlas's pictures take part the same way: smudges (craters,
scorch marks), ore, gems, walls, fences, rocks and the other overlays, and
trees and the other terrain objects (shadows stay native). Their pictures
share the pack folder and the hash. In the dump, every overlay frame and
every TerrainType of the theater is written as `<hash>.srgba`; pad those
with each sprite's own edge colours instead of the tile lattice.

## HD sidebar

With HD art on and the smooth sidebar (`[VERA] SmoothSidebar=`) drawing
at 2x, 3x or 4x, each cameo and sidebar frame piece the pack's `sidebar`
folder has a picture for draws from it instead of the xBR copy
(`render::sidebar_smoothing`): `<hash>.png`, 4x the piece, the hash being
`hd_art::frame_hash` of its size and RGBA, resampled to the draw scale. The
names baked into the cameos stay sharp blocks, as before. The copy is made
again when the sidebar's scale changes or HD art is turned on or off. Large (150%) draws its surfaces at 3x while the HD sidebar is on and
shows them at half in the frame (`SidebarFrame::with_whole_draw_scale`), so
it takes the HD pictures as well; XL draws at 2x as before.
`VERA20K_HD_DUMP_SIDEBAR=<folder>` writes each piece the pack lacks as
`<hash>.rgba` for an offline upscaler.
