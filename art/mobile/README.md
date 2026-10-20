# Landscape touch artwork

`landscape-controls.png` is an original transparent RGBA sprite atlas generated
with the built-in imagegen tool on 2026-10-20. The game embeds it in the native
binary; it does not require a network download or changes to retail MIX files.
The returned image is 1254 square; normalized quarter-atlas UVs preserve its alpha.

The grid is read row by row: joystick base, thumb cap, home, same type; army,
radar event, scatter, deploy; next unit, stop, attack move, force attack; cancel,
unit details, guard, squad. Editing glyphs use the matching small code-native
mask atlas in `src/app/ios_hud_render.rs`.

## Generation prompt

Create one production game UI sprite atlas for a landscape mobile real-time
strategy game. A single square 1024x1024 PNG with a genuinely transparent
background, precisely divided into a 4-column by 4-row grid of 256x256 cells.
Each cell contains ONE isolated centered sprite, all artwork stays within its
own cell with at least 18 pixels clear margin. No text, letters, numbers,
captions, labels, watermark, UI mockup, or background scene. Style: polished
original retro military sci-fi control hardware, dark brushed gunmetal,
beveled circular rims, readable ivory/cyan pictograms, restrained amber
accents, strong silhouette at 44px. Orthographic flat front view, consistent
lighting, crisp clean edges, no connecting objects between cells. Row 1 left
to right: (1) large circular joystick base ring, four directional arrowheads,
transparent center; (2) circular convex joystick thumb cap, small four-way
arrow emblem; (3) command base/home building silhouette; (4) three identical
tank silhouettes with one highlighted, select same type. Row 2: (5) formation
of tanks and infantry, select army; (6) radar with one alert pulse, latest
event; (7) four arrows radiating outward from a center, scatter; (8) mobile
command vehicle unfolding into a building, deploy. Row 3: (9) single tank and
rightward chevron, next unit; (10) solid square stop pictogram; (11) route arrow
ending in a crosshair, attack move; (12) strong crosshair with a lightning bolt,
force attack. Row 4: (13) clear X cancellation pictogram; (14) eye with a small
information diamond, show unit details; (15) shield with a small target, guard;
(16) three squad dots inside a selection bracket, squad control. Each button
except the joystick base and thumb cap is a matching circular metal medallion,
with the symbol very legible and no typography.

## Faction sidebar and toolbar art

`sidebar-allied.png`, `sidebar-soviet.png`, and `sidebar-yuri.png` were generated
with the built-in imagegen tool (generation mode) on 2026-10-20. Each original
1254-square image is embedded unchanged. `ios_sidebar_art.rs` records the measured
UV regions for its tall sidebar plate, long bottom strip, and offline radar.
The bottom strip keeps its end caps while stretching its center. The offline
display fits inside the live radar aperture with its aspect ratio preserved.
Native production cameos and live map content draw above the new chrome.

Allied uses blue-grey alloy and cyan precision details; Soviet uses dark armor,
red enamel and amber lamps; Yuri uses violet metal, curved ribs and psionic glass.
The installed local player's sidebar theme selects both sidebar and bottom strip.

### Faction generation prompts

+### allied

Use case: ui-mockup. Asset type: production RTS mobile HUD texture atlas, one square 1024 by 1024 raster atlas with exact rectangular texture regions. This is actual in-game chrome, not a mockup screenshot. Theme: ALLIED military interface: blue-grey precision machined aluminum, naval steel, cool cyan light strips, clean angular aircraft engineering, restrained ivory highlights. Standby emblem an original abstract winged compass.
Exact orthographic composition, edge-to-edge rectangular regions, no perspective:
REGION A occupies x=0 through 255, y=0 through 1023: one tall uninterrupted vertical sidebar backing plate. Very dark low-contrast center, detailed narrow metallic outer rails and fine seams. Uniform usable center so game content can overlay anywhere. No internal compartments, no radar hole, no buttons, no icon slots.
REGION B occupies x=256 through 1023, y=0 through 127: one long horizontal toolbar backing strip, continuous dark brushed-metal recessed center, slim raised upper and lower edges, end caps. No buttons, slots, marks or icons. This will sit behind actual shortcut icons.
REGION C occupies x=256 through 767, y=128 through 511: one rectangular radar-standby display, dark tinted glass, subtle technical etched scan lines, a single small winged compass emblem centered, faint broken scan traces around it; no map, no readable text.
All remaining atlas area fill with matching dark metal material, no extra elements. Maintain exact straight pixel-aligned region boundaries without outer margin or gutters. Frame details confined to edges, dark interiors allow overlays. Premium hand-painted/3D rendered late-1990s military RTS interface art with crisp readable bevels. No letters, no words, no numerals, no labels, no watermark, no device, no battlefield, no perspective. Opaque background.

### soviet

Use case: ui-mockup. Asset type: production RTS mobile HUD texture atlas, one square 1024 by 1024 raster atlas with exact rectangular texture regions. This is actual in-game chrome, not a mockup screenshot. Theme: SOVIET military interface: thick dark gunmetal armor, burgundy and deep red enamel, chunky industrial rivets and chamfered steel, restrained warm amber status lights. Standby emblem an original geometric red star inside a gear.
Exact orthographic composition, edge-to-edge rectangular regions, no perspective:
REGION A occupies x=0 through 255, y=0 through 1023: one tall uninterrupted vertical sidebar backing plate. Very dark low-contrast center, detailed narrow metallic outer rails and fine seams. Uniform usable center so game content can overlay anywhere. No internal compartments, no radar hole, no buttons, no icon slots.
REGION B occupies x=256 through 1023, y=0 through 127: one long horizontal toolbar backing strip, continuous dark brushed-metal recessed center, slim raised upper and lower edges, end caps. No buttons, slots, marks or icons. This will sit behind actual shortcut icons.
REGION C occupies x=256 through 767, y=128 through 511: one rectangular radar-standby display, dark tinted glass, subtle technical etched scan lines, a single small star within gear emblem centered, faint broken scan traces around it; no map, no readable text.
All remaining atlas area fill with matching dark metal material, no extra elements. Maintain exact straight pixel-aligned region boundaries without outer margin or gutters. Frame details confined to edges, dark interiors allow overlays. Premium hand-painted/3D rendered late-1990s military RTS interface art with crisp readable bevels. No letters, no words, no numerals, no labels, no watermark, no device, no battlefield, no perspective. Opaque background.

### yuri

Use case: ui-mockup. Asset type: production RTS mobile HUD texture atlas, one square 1024 by 1024 raster atlas with exact rectangular texture regions. This is actual in-game chrome, not a mockup screenshot. Theme: YURI military interface: dark violet brushed metal, elegant curved ribs and organic symmetric engineering, purple psionic glass and soft magenta energy accents. Standby emblem an original stylized psychic eye inside concentric arcs.
Exact orthographic composition, edge-to-edge rectangular regions, no perspective:
REGION A occupies x=0 through 255, y=0 through 1023: one tall uninterrupted vertical sidebar backing plate. Very dark low-contrast center, detailed narrow metallic outer rails and fine seams. Uniform usable center so game content can overlay anywhere. No internal compartments, no radar hole, no buttons, no icon slots.
REGION B occupies x=256 through 1023, y=0 through 127: one long horizontal toolbar backing strip, continuous dark brushed-metal recessed center, slim raised upper and lower edges, end caps. No buttons, slots, marks or icons. This will sit behind actual shortcut icons.
REGION C occupies x=256 through 767, y=128 through 511: one rectangular radar-standby display, dark tinted glass, subtle technical etched scan lines, a single small psychic eye with arcs emblem centered, faint broken scan traces around it; no map, no readable text.
All remaining atlas area fill with matching dark metal material, no extra elements. Maintain exact straight pixel-aligned region boundaries without outer margin or gutters. Frame details confined to edges, dark interiors allow overlays. Premium hand-painted/3D rendered late-1990s military RTS interface art with crisp readable bevels. No letters, no words, no numerals, no labels, no watermark, no device, no battlefield, no perspective. Opaque background.
