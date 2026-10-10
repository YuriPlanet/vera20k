# DRAGON LineTrail input, lifecycle and pixels

`line_trail.py` executes the pinned active-retail executable. The physical input
set is the same selected Hills scenario as the IFV chain: RULESMD, absent
LANGRULE, MPBattleMD, Hills and fixed ARTMD. Its `source_files` records the exact
extracted-byte hashes. Archive traversal is supplied by the production asset
extraction boundary; this harness does not execute the native MIX filesystem.

Run from the repository using the native-oracle Python environment:

```sh
VERA20K_GAMEMD_EXE=/path/to/gamemd.exe \
VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted-inputs \
PYTHONPATH=. python tools/projectile_oracle/line_trail.py --check
```

The extraction directory needs ARTMD.INI, RULESMD.INI, MPBattleMD.ini, Hills.map
and dragon.shp. The local research set is `/tmp/bridge-ifv-assets-1dDM9D/extract`;
its frozen input manifest is
`/tmp/bridge-ifv-assets-1dDM9D/manifest-native-inputs.json`. The DRAGON SHA256 is
`f361b34efa27811f8f8cc5d12aed2e2b62b82e185bb1424f597a54d0682c09c2`.

## Original instructions and boundaries

The full original BulletType reader46BEE0 calls ObjectType5F9574..5F95E2 for
`UseLineTrail`, `LineTrailColor`, `LineTrailColorDecrement` in the retained ART
Image section. Native constructor defaults are false, RGB128/128/128,16.
Selected DRAGON reads true,216/216/255,16. Rules constructor66784C..66785E and
AudioVisual66B77D..66B7A7 independently establish override RGB0/0/0. Options
constructor5FA350 and reader5FA776..5FA7B4 establish DetailLevel default2 and
clamp0..2. Only detail0 doubles the copied decrement (556B50).

ObjectUnlimbo5F514B..5F5210 executes with prior world admission supplied. It
uses the actual Bullet vtable and selected type, allocates210 bytes, calls
original556A20, appends the plain registry, copies the selected RGB/decrement,
and writes both Object+A8 and trail+4. No Abstract identity or RNG is consumed.
Allocator and atexit services are supplied; no game-world admission is claimed.

Original556D40 visits the registry backwards. Each visit samples a changed
owner coordinate into a32-slot ring and starts strength255, then subtracts
the copied decrement from every slot in the SAME visit. There is no simulation
frame guard. Detach556B30 clears both owner links but retains history until a
later visit sees the unattached newest strength reach0. ZeroXYZ terminates the
draw walk. Existing rings do not reread Options/type/Rules configuration.

The segmented cadence witness executes original RenderFrame4F4480 and the
Tactical suffix6D4582..6D4678. With its original53BAE0 gate open, RenderFrame
calls passes0,1,2; only pass2 reaches LineTrail. A separately supplied pass3
also reaches it. Peripheral display virtuals, time service, and unrelated
scene families are substituted. MainThrottle55E160 and pause683EB0 callers
are inspected, not executed with the OS/network clock. A fixed simulation-Hz
trail clock is not supported by this evidence.

Original556C00 projects both endpoints through6D2140, supplies
`-2-AdjustForZ(Z)` and newer-sample strength to4BEAC0. The full original line
body clips XY via7BC2B0 and adjusts clipped endpoint Z using4C1B50
(Sqrt_Approx4CAC40 then ftol). Its dominant axis may be depth: the steep control
makes53 stores at49 pixels. Pixel order and repeated blending are observable.
It narrows candidate Z to u16 before strict comparison with stored Z, reads
u16 ABuffer, blends packed RGB565, and never writes depth. The46 individual pixel controls
cover geometry, clipping/origins, backgrounds, depth, A values and fade visits.
Four mixed-A controls exercise X-, Y- and Z-dominant walks and a reversed,
clipped line. LineTrail4BEAC0 and packed laser4BFD30 sample A at each raster
pixel; additive laser4BDF00 retains the clipped start X and advances A only
with Y. The shared geometry retains that starting coordinate for its additive
consumer. Original laser pixels live in `spatial_oracle/building_prism.json`.
The RGB565 surface, scene Z/A contents and camera are supplied boundaries;
no full-scene lighting/visibility production or GPU parity is claimed here.

The2 persistence controls execute original AbstractSave410320, ClearAll556DF0
(single trail) and full BulletLoad46AE70 including ObjectLoad5F5E80. The356-byte
stream contains the old pointer token, but ObjectLoad5F5EED clears+A8 and never
queues that field for swizzle. No ring is restored by this object path. The
IStream transport is supplied; the whole game loader and post-load scene are
not executed. The separate joined impact witness
[`ifv_trail_impact`](ifv_trail_impact.md) reaches556B30 during admitted Bullet
UnInit/Conceal through DetachAll5F528E. The later destructor sees the cleared
owner slot. VERA's later deletion-time detach remains tracked in
[#1257](https://github.com/YuriPlanet/vera20k/issues/1257).

## Coverage and implementation boundaries

The corpus contains6 producer/ring/retirement sequences,14 reader controls,
8 Options controls,46 complete pixel controls,2 save/load controls,4 RenderFrame
and4 Tactical pass controls, a reverse-registry case and retained-config/zeroXYZ
case. Five additional complete native registry draws place1/8/128/512 trails
on the same30pixels, with alternating copied colors; one uses A64. These
exercise ordered destination blending across trails, not only within one line.
Malformed RGB controls additionally poison original sscanf locals:
incomplete nonempty scans expose caller stack residue, not a zero default.
VERA deliberately retains current RGB on malformed/incomplete scans. Complete
triplets use the shared native decimal scanner and byte narrowing. This matches
the selected retail triplets; malformed stack residue is not a parity claim.

The first retained candidate passes the ring/save-load and42-case CPU raster
comparisons. Root also ran `production_line_trail_matches_original_full_line_pixels`
against all42 original cases in both BGRA and RGBA formats, with exact color
and unchanged depth. The retained executable is
`/tmp/bridge-ifv-trail-gpu-libtests`, SHA256
`4c6831230234fa9583f78ba837e19b09f4edcfc48dfcd3566192e412bdaafdd4`.
The later retained selector candidate
`/tmp/bridge-ifv-selector-libtests`, SHA256
`25d0126000e51a881508e816d6c1533c4491ea27b8238b66747b2040e4276e77`,
passes all5 native overlap cases in both formats. Its8-trail/A127 control also
passes with forced3-operation chunks, crossing a bounded submission boundary
(80 chunks,160 passes). Color remains exact and depth unchanged. The reverse
registry-order CPU comparison passes. Receipts are
`/tmp/bridge-ifv-trail-overlap.log` and `/tmp/bridge-ifv-trail-order.log`.
The earlier integrated CPU group passes6 reader/ring/load/projection/raster
checks (`/tmp/bridge-ifv-integrated-trail.log`). Native comparisons remain
bounded to the listed inputs. Failed/repeated Bullet Unlimbo, all multi-trail
world-clear behavior, non-selected callers, and whole-scene compositing remain
outside this witness.

## Current production integration

The app owns a presentation-only ring keyed by a stable runtime handle. Ordered
launch and physical-destruction outputs attach/detach it; loading clears it.
One actual tactical composite ages it once, regardless of simulation tick count.
The render owner groups the original ordered pixel operations by destination
and resolves each group against the existing TerrainDrawRenderer color/Z
snapshot. Different pixels may execute independently; operations at one pixel
keep their original order. Bounded chunks retain that order across snapshots.

The current app uses a late global shroud multiply, unlike native per-family
ABuffer blits. The trail runs after that multiply and before UI so its exact
ABuffer operation is not followed by another blanket multiplication. The shared
background producer still uses an approximate sRGB curve at fog edges: A<127
can therefore alter the already-composed background and its mix with a trail.
This is a frequent fog-edge rendering limitation with no gameplay/RNG effect.
Native later RadBeam/other effect overlap ordering is not established by the
selected ordinary IFV scene. Zoom1 is the original pixel domain; other zooms
scale logical pixels. These limits preclude a whole-scene parity claim.

## Bounded rendering cost

The same selector executable passes the quiet Apple M4/Metal timing fixture
in both formats (`/tmp/bridge-ifv-trail-timing.log`). Each row uses a64x96 target,
one30-pixel segment per trail,2 warmups and20 measured samples. Total time
includes CPU preparation, encoder finish, queue submission, GPU execution and
completion wait; readback is outside the measured region. These debug-fixture
medians are neither GPU-only timings nor game FPS, and do not measure full rings
or a20,000-unit scene.

| Trails at A127 | Preparation median, ms | Completed submission median, ms |
| --- | ---: | ---: |
| 1 | 0.020–0.045 | 1.563–1.620 |
| 8 | 0.069–0.101 | 1.608–1.674 |
| 128 | 0.875–1.176 | 2.421–2.736 |
| 512 | 3.515–3.556 | 5.096–5.121 |

The8-trail/A64 control completes in1.613–1.617ms with0.072–0.073ms
preparation. Native overlapping pixels, chunk correctness and production timing
have separate receipts; the timing values are not native performance comparisons.
Final full-library, release and visible-runtime validation for the whole IFV
chain remains with its owner.
