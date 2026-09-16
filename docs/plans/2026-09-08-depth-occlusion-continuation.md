# Depth and occlusion continuation

Continues `feature/sprite-zbuffer-depth` from `51213e58`; retains its integer
Z axis, BUILDNGZ loader, TMP atlas, voxel seed grouping and ordered Ground pass.
Scope: ordinary buildings, cliffs, walls and unit overlap. This is an
implementation continuation, not a new renderer or a lighting redesign.

## Native evidence and resulting changes

Read-only live Ghidra against retail gamemd.exe, SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
An independent critic rechecked the important branches and stored asset headers.

- **BUILDNGZ:** `Extended_SHP_blitter` at 0x437A10, seed at 0x437C39..0x437C72 and
  row loop at 0x437E67..0x437EA7. A supplied second shape bypasses gradient quantisation
  and row stepping. Its seed is `(u16)(32768-screen_top-height+1)+z_adjust`;
  the leaf at 0x4990E0 subtracts a signed shape byte and tests strict-less with write.
  The prior shader applied ordinary gradient 2 as well, changing occlusion.
- **Shape intersection:** CC_Draw_Shape at 0x4AF08C..0x4AF0FB intersects the body
  with the second shape before dispatch. Seed top/height come from this
  intersection. SHP format bit 1 at 0x69E900 chooses extended versus standard;
  raw frames clip but use the ordinary gradient and ignore shape values.
- **SHP rectangle:** CC_Draw_Shape at 0x4AED70 centers by integer canvas halves,
  adds the stored frame X/Y, then passes stored width/height to the blitter.
  Sprite/overlay atlases previously padded frames to the canvas, preserving
  color placement but changing the depth seed. They now retain stored frames;
  a separate logical canvas rectangle preserves existing picking/sort metrics.
  Retail examples: GI canvas 78x66 / frame 0 (32,7,13,29), GGCNST
  canvas 284x226 / frame 0 (37,74,212,148), GAWALL canvas 78x70 / frame 0 (17,17,42,43).
- **Walls/ordinary overlays:** Cell_ContentRendering at 0x6D6D10 calls 0x47F6A0,
  which draws SHP with flags 0x4E00. Selector 0x490B90 reaches the same opaque
  read/write family as buildings. Walls force gradient 2 and use -15*level-2;
  other ordinary overlays select DrawFlat (default true), with the native
  upright/non-rock adjustment. Policy now survives fixed-cell lowering and
  contiguous draw dispatch. Existing special resource/rubble paths remain
  explicitly separate.
- **Units:** Infantry at 0x518F90 and normal AnimClass at 0x422CA0 keep the Z test
  regardless of display layer. Bridge infantry no longer writes flat depth;
  Top SHP also uses the existing per-pixel read-only pipeline. Their registration
  order and the voxel rendering path remain intact.
- **Cliffs:** TMP at 0x547CF0 seeds `(u16)(32768-y-tileH)-tileH*level/2`, then adds
  authored Z bytes and tests <= with write. Current terrain position/adjustment
  algebra already matches; no new cliff projection or sign change was made.
- **Color regression:** the newer zsprite shader dropped the existing
  encoded-palette tint helper. It now applies the same map/effect tint as the
  predecessor batch shader, avoiding an incidental appearance change from
  switching draw pipelines. Existing RGB565/light-table approximation remains.

## Validation and comparison

GPU checks passed on AMD Radeon integrated graphics through Vulkan:
`test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 8575 filtered out; finished in 1.84s`.
Full library:
`test result: ok. 8494 passed; 0 failed; 85 ignored; 0 measured; 0 filtered out; finished in 26.78s`.
The earlier focused run had 596 passed / 0 failed / 6 ignored. `cargo clippy -p vera20k
--lib` ran and exited 101 on six existing `approx_constant` errors in
`render/vxl_raster.rs` (3), `map/rmg/phases/meander.rs` (1), and
`map/rmg/phases/river.rs` (2). All three files are byte-for-byte unchanged
from 51213e58 (`git diff --exit-code`); no lint failure is in this change.
The command also reports existing repository warnings. Release build succeeded (4m 22s). Candidate executable SHA-256:
`09d1c0d1c9283bbeefd0d953e7f478d9bd43269f35909a9dd2deac658541ce6a`.

GPU tests execute production WGSL with production
instance/uniform layouts and isolate depth admission, transparent pixels,
read-only versus writing sprites, signed shape values, clipping, and tint.
They are regression tests, not native scene parity goldens. Loader tests exercise
the real loose-file SHP loading path with synthetic retail-shaped frame headers.

The previous second fixture encoded OverlayPack with LZO, while native YR
requires LCW/Format80. The portable stdlib-only generator is `tools/render_depth_fixture.py`:

```powershell
python tools/render_depth_fixture.py target/depth-comparison/depth-walls-cliff.map
python tools/render_depth_fixture.py target/depth-comparison/depth-walls.map --walls-only
```

The corrected outputs match the checked fixture bytes and independently decode
to 4,560 map cells, 32 chunks per 262,144-byte overlay plane, and 12 connected
wall cells. The cliff map has the four present Cliff01 subtiles and a low-side
power plant. Its SHA-256 is
`a110a6131bbc7a768cdb493f0bb088af7f3247e8d11cecee0932a862c970afd3`.
VERA accepts the absolute path through its existing RA2_QUICKPLAY hook. The
isolated native test copy's Soviet campaign points at the same bytes under
`depthwalls20260908.map`, with its previous battlemd.ini backed up in the
local checkpoint folder. Original retail files were not edited.
The release process loaded this map and reached a responsive tactical window
(`RA2 - Depth Continuation - Walls and Cliff`); its log confirms the 396x477
BUILDNGZ load. Original YR was restarted with the staged campaign. User
confirmation of native loading and comparison of occlusion boundaries remain
pending; this is not visual parity sign-off.

## Bounded residuals

Special sloped tiberium, rubble and TS vein shapes keep their prior passthrough;
this change does not fabricate slope Z data. Cloak/warp leaf families, native
16-bit wrap/dirty-rectangle behavior and scene-wide pixel parity are not closed.
Stock TMP census: 5,326 files/18,983 cells/4,399 extra+Z cells across 14
installed iso archives; no opaque diamond/extra overlap, missing-Z cells or
parse errors. Archive hashes and counts are in local
`target/depth-critic-census/tmp-census.json`. Conditional decoder concerns
therefore have no demonstrated stock trigger. Stock cliff geometry must
still be compared in the corrected fixture. The old
fixture-1 screenshot report is historical, not validation of this candidate.

## Screenshot follow-up and fixture correction

The user's two comparison screenshots align at (+34,+36) pixels from the
first to the second. Visible construction-yard and war-factory overlap
boundaries show no definite depth mismatch. Unit movement and animation prevent
using all differences as depth evidence. The user has not yet identified which
image is native. No unit clearly intersects the cliff, and neither image has
the intended wall ring, so those acceptance cases remain open.

The missing walls were a shared fixture error: `[OverlayTypes]` begins
`1=GASAND, 2=CYCL, 3=GAWALL, 4=BARB`, but OverlayPack indexes the zero-based
declaration array, not the INI key. `RulesClass::Process` at 0x668BF0 and
`ReadMapOverlayPacks` at 0x5FD2E0 confirm this. The original generator's ID 3
selected BARB, which has no active retail section/SHP; GAWALL is ID 2.
The generator now emits ID 2 and keeps the near-wall GI outside the ring.
A Python regression decodes both generated maps and resolves their identities
against the retail declaration prefix; it passes for both variants.

V2 changes only 12 overlay identity bytes and that GI's cell. Terrain and wall
connectivity planes are unchanged. The corrected cliff fixture SHA-256 is
`6bc6d34dad3be8c5faf9639e3112ff38d5eef1532b046cc440bbed5746339828`.
Task-local file: `target/depth-comparison/depth-walls-cliff-v2.map`.
The isolated native campaign's existing `depthwalls20260908.map` now has these
same bytes; restart the Soviet test mission to load them. The previous map is
preserved in the task's comparison folder. No renderer changes or rebuild were
needed for this fixture correction. The subsequent paired result is below.

## Paired wall and building comparison: bounded pass

The user identified `codex-clipboard-cf14eedd-6717-4066-b315-e30da8d0fb49.png`
as original YR and `codex-clipboard-9bfb8b77-8aa4-49f1-a8d9-3cc8c8b43b49.png`
as VERA, and accepted the VERA appearance. Both use the corrected V2 fixture.
Fixed-art patches align with a native-to-VERA translation of (-43,-12) pixels.
The independent critic found matching continuous wall segments/corners,
pillbox placement, clipping of the tank behind the upper-right wall, and
visibility of the foreground tank. Visible construction-yard and war-factory
unit overlap also agrees. No definite depth mismatch is visible in this pair.

This is a visual pass for these wall/building overlap cases, not scene-wide
pixel equality. Infantry/crane animation differences were excluded. Neither
image has a unit crossing the cliff face or a bridge overlap; those are not
claimed as demonstrated by these captures. Images and comparison metadata
are preserved under `.local/depth-occlusion/user-comparison-20260908/`.

## Cliff critic follow-up: confirmed remaining gap

The user deferred the manual cliff comparison and requested a critic review.
Two read-only reviewers traced the actual terrain-to-unit production path and
the native unit draw callers. **Cliff parity is not complete.** The existing
terrain depth path is connected, but a stock-active unit depth adjustment is
missing. No cliff implementation, rebuild or game restart was made in this review.

- **Already present before this continuation:** TMP decode, terrain atlas and
  instance depth data, terrain depth shader, voxel instances and voxel depth
  shader are unchanged from `51213e58` to `432f4766`. Terrain writes the shared
  depth attachment first; Ground UnitAtlas/transition runs test it through the
  voxel pipeline using strict-less without writing. There is no disconnected
  terrain-to-tank depth path. Our continuation fixed SHP consumers and walls;
  it did not directly fix ordinary voxel tanks against cliffs.
- **Native omission confirmed:** TechnoType constructor `0x710AF0` stores
  `ZFudgeCliff = 10` at `+0xDC0` (`0x711664`). UnitType constructor `0x7470D0`
  calls it and does not reset that field. UnitType ReadINI `0x747620` calls
  TechnoType ReadINI `0x712170`; `0x71541C..0x715437` retains the existing value
  when the INI key is absent. Thus absence from retail rules does not mean zero.
  The older [CLIFF_OBJECTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/CLIFF_OBJECTS_GHIDRA_REPORT.md) claims of default zero and dormancy
  are contradicted by this live binary evidence and must not justify skipping it.
- **Active draw consumption:** Unit vtable `0x7F5C70`, slot `+0x2EC`, resolves
  to Foot `0x4DAFC0`. Both normal composite `0x73B140` and cached `0x707480`
  paths call it. The compositor `0x4DAFF0` includes
  `max(cliff, column, tunnel, bridge)` plus its base and `0x704350` adjustment;
  `0x4DB03F..0x4DB04C` loads and multiplies the cliff field.
- **Cliff predicate:** `0x704240` returns zero on a bridge. Otherwise it probes
  cell offsets `(1,1)` and `(2,2)` relative to the unit's current cell. A signed
  height difference of at least four sets the first result to 2; the second
  probe independently overrides it to 1. Offset global `0x89F694` is initialized
  to packed `(1,1)` at `0x49F34A`; its zero bytes in the file image are not its
  runtime value. With the stock default, the cliff term can be 10 or 20 native
  Z units before the ordinary blitter quantisation and max with other terms.
- **Current Rust:** `instances/units.rs::voxel_z_adjust` supplies only
  `ground_z_adjust(z, 0)`; there is no `ZFudgeCliff` parser or consumer.
  Ordinary SHP foot bodies likewise omit it. Existing bridge sort-depth bias
  cannot supply it because the per-pixel shaders compute depth from `z_adjust`.
  Omitting the positive native term can leave unit pixels visible through a
  cliff where native YR rejects them. This is a pre-existing gap, not a regression
  introduced by our wall/building fixes; the exact visible boundary still needs
  a paired cliff case.

The four new GPU regressions exercise terrain and SHP shaders in a minimal
harness. They do not exercise the voxel shader or the complete production draw
dispatch, and therefore cannot certify the tank/cliff case. The wall/building
screenshot acceptance above remains valid within its stated coverage. Resume
with the native cliff adjustment and a tank crossing the cliff boundary when
the user returns to cliffs; do not rebuild the already-connected terrain path.

## 2026-09-08 — authorized cliff implementation

The user authorized a plan followed by implementation. The confirmed Foot depth
omission above is now implemented through the existing draw paths; terrain/TMP
projection and voxel shading were preserved. The preceding review describes the
pre-fix state. See [cliff plan and validation](2026-09-08-cliff-unit-depth-plan.md)
for exact native evidence, the shared math/runtime adapter, all consumers, the
pristine-TMP catalogue, independent review and bounded residuals.

Final source validation: 8512 library tests pass (89 ignored), all 8 explicit GPU
tests pass, 14 focused depth/runtime tests pass and 85 unmodified-gamemd function
fixtures reproduce. Clippy remains failed on 6 existing errors in 3 untouched files;
release build succeeds. GPU coverage now includes the actual indexed voxel
shader and decoded retail cliff terrain, extending the earlier SHP-only harness.

Visual cliff acceptance remains open. The old v2 map contains no nonzero cliff
selector cells, despite showing a cliff. A separate --cliff-back fixture now uses
retail Cliff28 back edges, with tank/GI placements selecting +10. Its scenario-only
CliffBackImpassability=0 permits those controlled overlaps in both engines.
Original wall/building fixtures and their accepted evidence remain unchanged.
The current full-game/camera comparison has not been rerun, so no claim of complete
renderer or stock-pathfinding parity follows from this implementation.
