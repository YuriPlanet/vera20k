# Original DRAGON frame and physical drawing evidence

`ifv_render` independently reruns the five `ifv_launch` cases, then executes
original `GetAnimFrame(0x468000)` and the full Object Render/Bullet Draw/shape
pipeline for each of their 125 retained flight states and five launch states.
Thirty-two supplied direction centers additionally cover every physical DRAGON
frame, followed by four clipping/dirty-region controls: 166 draws in total.

```sh
source ~/Documents/vera20k-dev/env.sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/tmp/bridge-ifv-assets-1dDM9D/extract \
~/Documents/vera20k-dev/.venv/bin/python \
tools/projectile_oracle/ifv_render.py --check
```

`frames[]` contains a stable name, kind, native binary frame, retained XYZ,
binary64 velocity values/bits, the original frame result and its `draw_index`.
`draws[]` uses the existing `bridge_render_shape` schema: explicit input,
native camera/projection, shape-call frame/position/flags/depth/palette, reached
leaf rows with source palette indices, changed RGB565 pixels, complete color
hash and whether the depth buffer remained unchanged.

The physical reader establishes DRAGON's 32 frames, 24×16 canvas, rotating
selection and no shadow. Its unchanged SHP hash and the native palette load
hashes are retained. The native frame getter executes original atan2 and integer
conversion; host atan2 does not supply any expectation. Direction centers use
explicit host-generated binary64 inputs and are not movement producer claims.

Drawing reuses prepared 64×96 RGB565 surfaces, A127, a 32768 depth baseline and
65535 old depth. The explicit body screen point determines a camera through
original projection. Full native shape clipping, selection, rowwalking and pixel
leaves execute; no drawing call is replaced. Existing `bridge_render_shape`
retains Cannon's physical120MM depth/geometry controls; this corpus adds the
physical DRAGON image, selected flight rotations and pixels.

Launch source/FLH/world admission boundaries remain those of `ifv_launch`.
Retained last-step states are sampled before retirement; drawing them does not
establish native Display scheduling at impact. Neither GPU nor whole-scene
parity follows from this corpus alone. Metadata pins the original executable and
payload; `--check` reproduces without writing, while `--write` regenerates.

## Production comparison receipt

The joined candidate identified in the [IFV ledger](ifv_launch.md) passes
`original_ifv_dragon_frame_getter_matches_retained_flights_and_all_directions`
for all 166 retained frame/projection rows. The production renderer test
`retail_ifv_dragon_all_frames_and_flight_match_original_shape_pixels` also
passes all 166 physical draws in both BGRA/RGBA formats (1.20 s), comparing native
packed pixels and unchanged depth through the shared Bullet path. These are
supplied 64×96 scenes with the corpus Z/ABuffer and palette inputs, not native
Display cadence or whole-scene captures. [LineTrail](line_trail.md) has separate
ring/pixel/overlap receipts; body success does not establish trail scheduling.
The [IFV ledger](ifv_launch.md) records the passing full retail suite, release
map load and visible save-restoration checks, with their separate binary identities.
