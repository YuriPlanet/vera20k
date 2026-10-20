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
