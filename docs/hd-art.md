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

`render::hd_art` keeps the world composed at native pixels, exactly as before.
When the wheel zoom magnifies it 1.5x or more, it redraws building bodies,
bibs and animations (ActiveAnim, IdleAnim and the rest, SHP turrets) and
infantry on
the zoomed copy from 2x art (`hd_art.wgsl`), pixel by pixel, only where the
native world still shows that frame's own colour under a plain light:
units, effects and fog edges drawn over it keep their native pixels,
and the silhouette stays the native one. Each HD pixel is moved from the
art's reference palette to the frame's real palette row (theater colours by
difference, house colour by brightness along the house ramp) and takes the
light the native pixel received.

The art lives in a pack folder ([Install an HD art pack](#install-an-hd-art-pack)). It holds `reference.pal` and one `<hash>.png` per
frame, exactly 2x the frame's stored rectangle, alpha for coverage. The hash
is FNV-1a 64 over the frame's width and height (u16 little-endian) and its
palette indices, so a frame finds its art whatever type, theater or mod draws
it. `VERA20K_HD_DUMP=<folder>` writes `<hash>.idx` (u16 width, u16 height,
indices) for every such frame on screen the pack lacks; an offline
upscaler turns those into pack files.
Desktop reader threads (`render::hd_loader`) open and decode pictures as
their frames appear on screen. Frames draw with native art until their HD
pictures reach the GPU. The 2048x2048 index page and its 4096x4096 art page
can repack once per pass, copying recently visible pictures that fit. Evicted
pictures are read again when needed. A new map's sprite atlas clears cached
atlas positions and palette rows. Pictures keyed by hash can stay across maps.
Single-frame captures read visible pictures synchronously.
`VERA20K_HD_PAGE_SHRINK=<n>` limits each usable page dimension to 1/n of
its size so a capture can exercise repacking. Voxels have their own path
(below). Every frame of the SHP sprite atlas takes part: buildings,
their animations and bibs, infantry, and the effects (explosions, fire,
smoke, debris, muzzle flashes, SHP projectiles and particles). Translucent
effect pixels mostly fail the colour test and keep their native look.

## HD voxels

With HD art on, every voxel picture the unit atlas rasterizes (vehicle
bodies, turrets and barrels, aircraft, building voxel turrets) also gets a
picture at twice the resolution (`vxl_raster::VxlHd`): the same native
stores, in the same order, each covering two by two pixels at the half pixel
its 8.8 cursor reached. A model has more voxels than its native picture has
pixels (a Grizzly hull is 59 voxels long on about 45 pixels), so the zoomed
picture shows detail the native one rounds away. The pictures sit on a
twice-size page beside each unit atlas page (`UnitAtlas::hd_page`), and
`render::hd_voxel` draws them on the zoomed copy with the same rule as the
HD art: only where the native world still shows the picture's own colour,
in the same palette and house ramp, under the light the native pixel got.
Posed pictures (crashing, rocking or tilted bodies, pitched rockets, voxel
debris: `render::unit_pose_cache`) are drawn afresh each frame. While the HD
pass draws (HD art on, wheel zoom 1.5x or more, `vxl_raster::hd_poses`)
they are rasterized on the CPU rather than by the GPU voxel raster, so they
get their HD picture too, on a twice-size page beside each pose page
(`VxlPoseFrameCache::hd_page`); below that zoom they stay on the GPU raster.
Voxel shadows stay native. Pictures rasterized before HD art was first on in
a run keep no HD picture until the next map.

## HD terrain

`render::hd_terrain` does for the ground what the HD art does for
buildings. At 1.5x wheel zoom or more it redraws each theater tile the
terrain pass drew (`TerrainInstances::normal`, in walk order) from a 2x
picture, only where the native world still shows that tile's own colour
under a plain light: smudges, ore, overlays, units, a later tile and fog
edges keep their native pixels. Each HD pixel takes the light the native
pixel received, so cell lighting and shadows stay. Tiles are RGB already,
so no palette moves.

The pictures are in the pack's `terrain` folder: `<hash>.png`, exactly 2x
the tile's rectangle, the hash being FNV-1a 64 over the tile's width and
height (u16 little-endian) and the RGBA the engine colours it with
(`TileAtlas::hd_tile`). `VERA20K_HD_DUMP_TERRAIN=<folder>` makes a map load
write every tile of its theater the pack lacks as `<hash>.rgba` (u16 width,
u16 height, RGBA) for an offline upscaler. Loading one map per theater
collects them all. Upscale each tile padded with copies of itself on the
isometric lattice, so tiles meet without seams. Pictures are read when on screen, by reader
threads as the art's are, into one 4096x4096 page. A full page keeps the
pictures drawn in the last frame on a fresh page, so the tile pass of a frame
keeps its pixels when the overlay pass of the same frame fills the page.

Between native pixel centres the pass interpolates the light from the
tile's neighbouring pixels that still show it, so shadow edges and light
steps on the ground fall between the HD pixels instead of in native blocks.

The same pass then redraws the overlay atlas's bodies: smudges (craters,
scorch marks) first, then ore, gems, walls,
fences, rocks and the other overlays (the cell body sweep), and trees and
the other terrain objects (their `TerrainShp` body pieces; shadows stay
native). Their pictures share the pack folder and the hash
(`OverlayAtlas::hd_sprite`); the overlay atlas is an sRGB texture, so the
shader takes its loads as linear. In the dump, every overlay frame and every
TerrainType of the theater is written as `<hash>.srgba`; pad those with
each sprite's own edge colours instead of the tile lattice.

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
