# Ordinary tactical shroud ABuffer evidence

`shroud.py/json/meta.json` execute the original reset, SHP access, cell-edge
selection and tactical ABuffer producer against physical stock `SHROUD.SHP`.
The sidecar records executable, emulator, source and payload identities. This
is the ordinary `FogOfWar=no`, empty-AlphaShape-list route; it does not establish
all vision lifecycle state or the wider fog/AlphaShape mechanisms.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --check
```

Use the executable/environment described in [`native_oracle.md`](../native_oracle.md).
The existing `rally.Rally` owns Surface/ABuffer setup and `stock.mix` owns retail
archive extraction. There is no second rasterizer producing the golden pixels.

## Concrete prerequisite correction

Stock `ra2.mix/conquer.mix/SHROUD.SHP` has SHA256
`9818f7e2685fb1e8ed75ceff568aaf5d36350c747cbd4f30856458bba12e4964`, canvas60x30
and96 frames. The selector uses frames0..46. Each of these47 frames has exactly
the same900-pixel nontransparent diamond; their only other pixels are254(skip).
Frame15's drawn pixels are **2**, not0. Frame0's drawn pixels are127.

Original `47EFE0` writes each source byte directly as a u16 A value except254,
which preserves the destination. Original `4801F0` chooses frame15 when
`6D8700` returns-2. Executed flags `Cell+0x12C=0` produce cache-2/frame15 and
900 A=2 pixels. A generated black diamond therefore changes native rally pass
admission: `4C0750` distinguishes A=0 from A!=0. For ordinary palette blitters,
small nonzero A can still choose a black palette row; it is not interchangeable
with A=0 for procedural consumers.

The prior Rust diamond's scan intervals match frame15's900-pixel mask exactly.
Row0 is empty (the prior comment claiming an added tip was incorrect); rows1..15
expand4,8,...,60 pixels and rows16..29 contract56,...,4. Reuse the loaded stock
frame15 in the existing CPU frame blitter and remove the generated substitute.
That retains one asset and drawing owner and also respects replacement assets.

## Executed coverage and schema

- `leaf_cases`:73 original47EFE0 executions: all47 selected stock frames,
  left/top/right/bottom/offscreen origins, four interior clips and two circular
  storage offsets. `input.point` is the native top-left frame origin;
  `input.clip` defaults to `[0,0,160,120]`. `frame_rect` is returned by original
  69E7E0. `alpha_hex` contains19200 logical row-major A bytes, without padding.
- `selector_cases`: all256 neighbor masks with center flags0x18 through whole
  4801F0/6D8700. `caches` are signed Cell+120/+121 bytes; `frame` is the actual
  47EFE0 argument. Sixteen `flag_controls` cover0x00,0x08,0x10,0x18 with masks
  0x00,0x01,0xAA,0xFF. The two bitmask domains remain distinct from Rust's compact
  presentation bitmap.
- `scenes`: nine whole6D3660 executions. Full redraw reaches6D71E0; dirty-cell
  cases reach480A30/6D1F10. Cases cover clear, unrevealed, a revealed factory
  frontier, an island, integer camera changes, circular storage wrap, flat and
  raised dirty cells, and a separately bounded partial0x10 flag state.
- Every reset runs original4112D0 and asserts all A values become127. SHP rect,
  SHP data, clipping, circular-row access and pixel stores all execute original
  instructions. Only platform Surface storage comes from the existing native
  BSurface fixture; there are no function-result substitutions.
- The factory-frontier scene feeds the resulting original ABuffer to original
  rally6DA9D0/4C0750 and records `rally_passes`. Factory/selection/color fields
  are the existing rally fixture inputs, not a complete Scenario constructor.

Production tests should load the SHA-identified asset through `AssetManager`,
`ShpFile` and `extract_shp_brightness`, then compare the shared CPU frame/rebuild
owner's bytes to this corpus. Synthetic GPU tests alone do not establish these
A inputs. Container hashes are in `stock`; this oracle reads the named physical
archive, not a complete native archive/loose-file override resolver.

## Projection, ordering and clipping

Dirty-cell6D3660 calls GetCenter480A30, forces Z=0 at6D36C0, projects with6D1F10,
subtracts integer camera+0xB0/+0xB4 and centers the frame by(-30,-15). Full scan
6D71E0 executes the original inverse-matrix constructor values and projection.
The same Cell10,20 at native camera[-320,440] is drawn at[-10,10]; the raised
level8 dirty control has identical A bytes to level0. Rust's existing world
coordinate convention adds15 to camera Y, so its matching camera is[-320,455].
Fractional camera/zoom behavior is a separate presentation extension.

The native full scan walks diagonal screen rows; dirty cells use queue order.
Rust walks `ry` then `rx`. For all47 selected stock frames, the common mask has
zero intersections with its lattice neighbors at offsets(30,15),(-30,15),
(0,30),(60,0). Thus the order cannot change this ordinary shroud-only result.
Every full scene also replays the exact original per-cell calls through
4801F0/47EFE0 in Rust row order and with full-plane clips; byte equality is
asserted and recorded as `row_major_full_plane_replay_equal`. This establishes
that native per-cell clipped rectangles and full-plane CPU blits agree for
these cases. It does not establish fog/AlphaShape blend ordering.

Frame0 writes127 instead of being empty. Skipping it is equivalent only after
a full127 reset with the established disjoint stock masks. Incremental native
redraws must still execute the write; this is not a general frame0 skip rule.

## Remaining boundaries

The partial `0x10` state chooses frame0 even though bit0x08 is clear. It is saved
as a raw consumer boundary, not claimed to be an ordinary reachable state in
this chain. Existing `shroud_current_sight` evidence establishes selected
counter/open/pending transitions but explicitly excludes complete edge-cache
outputs. Its shared FogState/vision owners remain the lifecycle authority.

Dynamic AlphaShape `420F40/421350` can further alter A; enabled fog uses47F250
and FOG.SHP under the scenario0x1000 gate. Those writers, incremental dirty
invalidation, runtime scroll production, map-edge dummy state, mod assets and
full reveal traversal are outside this ordinary-input corpus. No full-game or
whole procedural-drawing closure follows from these vectors alone.

## Selected ordinary building brackets

The additive `--building-brackets` mode writes/checks
`building_brackets.json` and its provenance sidecar. It reuses
`HutTypeReader` for original construction and INI caches,
`action_lines.physical_palette` / `PaletteReader` for physical palette startup,
and `building_prism.LaserPixels` → `rally.Rally` for the original packed Surface
kernel. It creates no second VM, INI reader, projection or line rasterizer.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --building-brackets --check
```

The installed original archives beside the configured `gamemd.exe` supply
`expandmd01.mix/RULESMD.INI`, optional loose `LANGRULE.INI`,
`ra2md.mix/localmd.mix/MPBattleMD.ini`, and `multimd.mix/XMP03T4.MAP`, in that
order. Native HasSection `526810` admits the selected `GAPOWR` sections before
original Image caller `5F92F9..5F9340` runs. Fixed
`ra2md.mix/localmd.mix/ARTMD.INI` supplies Height/OccupyHeight and Foundation to
`4610D8..461112` and `461225..46125D`; it is not a rules layer. Existing lexical
extraction supplies strings, and the original parsers/defaults/stores execute.
Every layer's identity, selected keys and actual reads are in `geometry.readers`.
This is focused field loading, not full Scenario or type-file loading.

The complete original BuildingType constructor `45DD90` yields Height2 and
Foundation0; these physical reads yield Height4, Foundation3. Original
Dimension2 `464AF0` returns `[512,512,416]` with the explicitly supplied native
startup height factor104. Original palette initialization consumes physical
`ra2.mix/cache.mix/ANIM.PAL` and `PALETTE.PAL`. Index15 of global `87F6C4`'s
middle lookup row is `0xFFFF`; the packed color is a native palette result.

Original complete DrawBehind `6F60D0` and DrawExtras `6F5190` prefix execute on
prepared selected GAPOWR state. Extras stops at `6F5E17` before health drawing.
Original GetCoords, pair helper `6F5EF0` and Tactical `6DBB60` produce the saved
world endpoints, screen endpoints and paired Z adjustments. GetHeight
`5F5F40` returns supplied ground-relative0. Location `[2688,5248,0]`, native
camera `[-384,376]` and viewport160×160 keep the full cuboid in the supplied
crop; points are not translated after projection. `geometry.passes` contains
10 `behind` and11 `extras` commands. Deselected produces0+0; the prepared
sinking gate produces10+0, without claiming that ordinary GAPOWR can sink.

Each captured seven-argument Surface call crosses a declared recording
boundary into the existing pixel runtime. Original `4BFD30` then executes its
clipping, three-axis walk, strict unsigned Z admission, A sampling, RGB565
modulation and color stores. Every command has writeZ0. The four
`raster_controls` are:

- `clear`: A127 and Z65535, producing315 ordered writes at311 distinct pixels.
- `mixed_a`: existing Rally's `(0,1,63,127,255)` pattern, producing270 writes at
  266 pixels. A0 skips; A1 writes black; A63 writes `0x7BEF`; A127 writes
  `0xFFFF`; A255 writes `0xEFBD`. The native modulation does not clamp A to127.
- `occluding_z`: A127 with supplied Z32700 in rectangle `[40,40,120,120]` and
  Z65535 elsewhere, producing270 writes at266 pixels. This synthetic depth
  control establishes consumer occlusion, not native building-body occlusion.
- `native_ground_body_z`: A127 with `input.native_z_runs` from actual original
  flat ClearTile/TMP and GAPOWR body/shadow execution. Runs are
  `[x,y,length,u16]`; omitted positions retain `input.z` (65535). All12 lower
  commands fail native Z admission on this flat control, leaving132 roof
  pixels. This establishes ordinary terrain occlusion, without claiming that
  12 visible stubs were missing from the prior presentation.

`stores` preserves original write order and repeated pixels as `[x,y,word]`;
`pixels` gives final changed words in row-major order. Each segment identifies
its pass/index and half-open `store_range`. Every execution checks unchanged
original code, A and Z, pixel guards, and no observed raw/ranged RNG entry.
The Rust producer and shared packed-line consumer consume these native values.

`ground_occlusion` describes the fourth control's upstream native depth. The
existing TMP/PaletteReader runtime owns physical archive, lazy TMP, palette,
lighting and Surface setup. Original BuildingType45DD90/45FE50 consume layered
physical rules and fixed ART, resolving physical `GGPOWR.SHP`; original45E8F0
loads/remaps physical `BUILDNGZ.SHA`. Native GAPOWR BibShape isNULL. The existing
`color_scheme_palette` owner constructs LightGold in that runtime. Native
Building43B740 and idle/frame routines execute with explicit placed-state,
local House, flat/revealed Cell, A127 and camera priors; active Anim slots are
empty. Placement itself and native Scenario/selection acquisition do not run.

The actual `+104` Building admission43CEA0 executes its native455C20 bounds
and virtual `+AC`459EF0 render coordinates, projecting body point`[84,74]`.
This differs from bracket GetCoords center`[84,104]`: render coordinates are
Location minus`[128,128,0]`. Whole43D290/705E00/4AED70 draw physical body frame0
and shadow frame3, consuming BUILDNGZ. Behind and initial Extras precede this
body; a repeated pre-health Extras prefix follows it. Per-command store
counts, original shape arguments and full crop color/Z hashes are retained;
the final Z plane crosses to the packed-line control as compact native row
runs. Complete color planes and screenshots are excluded. This consumer test
uses supplied native depth; it is not a GPU body-renderer parity claim.

The corpus does not execute placement, the whole Display loop, owned active
Anim rendering, native shroud/AlphaShapes, or a whole GPU frame. Native callers
place behind and initial Extras inside each Building parent's slot before
that parent's body, then invoke Extras again in a separate late sweep. The
two command lists alone do not establish that ordering or intervening body
composition. The bounded original ground/body composition supplies one
occlusion control. Complete app output, nearby overlap and whole-frame order
remain validation gates; the synthetic Z rectangle cannot close them.

## Ordinary selected mobile health strip

The independent additive `--unit-health` mode uses one existing
`bridge_render_inputs_palette.PaletteReader` runtime and the shared
`bridge_render_shape.prepare_surface` BSurface/A/Z storage. It executes
unchanged complete original DrawExtras6F5190, DrawHealth6F64A0 and their
ordinary PipScaleNone709A90/70AA60 tail to normal return. Every physical SHP
call4AED70 executes frame access, centering, clipping, picker490B90, rowwalker
and actual pixel leaves; there is no argument-sink substitution in this mode.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --unit-health --check
```

`unit_health.json` and its sidecar retain the original executable identity and
command. Normal STOCK_DIGITAL production asset lookup independently resolved
PIPS.SHP to `ra2md.mix/conqmd.mix` (21frames,1920bytes,
SHA256`7525b25f80b5b67e9a5c2977b7624c4b3ce7ad9ce332169c4622eddb91c61486`),
PIPBRD.SHP to `ra2.mix/conquer.mix` (272bytes,
SHA256`8da08a64beb8c7916a3ae51fc7239d9474ef3366e73e78e893e24e31c119812e`),
and PALETTE.PAL to `ra2.mix/cache.mix` (768bytes,
SHA256`e85c535002573b5f95f7c361c4bdb95ee43e4914ddc926afda9ace995c54242b`).
Loose/expansion/language overrides were excluded by that production lookup;
the oracle then asserts these same immutable archive-byte identities. It does
not emulate original archive search. The older19frame RA2 PIPS is excluded.
ANIM.PAL is an inherited palette-startup input; health uses only the original
plain PALETTE.PAL Convert87F6C4. Full palette/LUT construction executes.

Whole Rules constructor665650 supplies both Condition thresholds0.5; whole
UnitType7470D0 supplies Strength0, PixelSelectionBracketDelta0 and PipScale0.
Original selected scalar/enum caller/read/store ranges consume exact lexical
RULESMD, optional LANGRULE, MPBattleMD and XMP03T4 layers. The result is
Strength300, delta0, PipScaleNone0, ConditionYellow0.5 and ConditionRed0.25.
These are rules keys, not ART. Physical MTNK leaves SpawnsNumber, Passengers
and Organic unset; their actual constructor values remain the ordinary-tail
inputs. No Python numeric parse supplies these values.

Prepared original Unit table/type/House linkage, rookie state, no group,
selfheal, cargo or managers and supplied HP/selection/hover/sinking are
consumer boundaries. Original5F65A0/6D2140 project actor XYZ through native
camera`[-380,385]` to point`[80,80]`; the supplied overlap actor projects to
`[84,80]`. This is not a native Unit lifecycle, occupancy, damage, Display loop,
MTNK VXL body or shroud-generation execution. The two nearby points prove
component draw order; legal dense-unit placement is not claimed.

The `raster_controls` share explicit input size/background/A/Z/clip and actors.
Each `draws` row has actual asset/frame/point/flags, native leaf/vtable and
half-open `store_range` into ordered `[x,y,u16]` stores. `pixels` contains final
changed RGB565 words. The existing producer and GPU consumers use these native
values; no second health/count/raster implementation supplies goldens.

Selected HP300/150/75/1 yields17/8/4/1 filled pips, using16/17/18 at actual
native threshold comparisons.151/76 distinguish the just-above arms. Selected
PIPBRD0 uses flagsE00, leaf493DF0/vtable7E57E0 and consumes144A samples without
Z. PIPS uses flags600, leaf4912B0/vtable7E5A20, bypassing both A and Z. Hover
omits the border; absent/sinking controls emit no shapes. Original ordinary
709A90 and70AA60 execute, with no additional shape calls for these priors.

A127, partial63, stock unrevealedA2 and explicitA0 controls preserve the fill
colors while changing the border. Existing original shroud evidence establishes
A2 from physical frame15; this mode supplies that downstream A plane rather
than rerunning its writer. Z0 over the entire strip has identical color output
to Z65535: both shapes bypass Z and leave it unchanged. One left-edge clipped
control exercises original shape clipping; one ordered two-parent overlap
retains each parent's border→fills before the next parent's border→fills.

Original code/vtables, prepared actor bytes, A/Z and buffer guards stay
unchanged; no raw/ranged RNG entry is observed. Commands are frame-local.
Selection/hover acquisition, fog/AlphaShapes, active selfheal flashes,
rank/status/control-group text, other PipScales/classes and whole DrawExtras
closure remain outside this ordinary selected rookie MTNK evidence. Production
GPU readback and normal game captures remain separate validation.

## Ordinary Building health and actual body composition

The additive `--building-health` mode extends the same health generator and
recorder as `--unit-health`. It borrows the existing bracket ground/body
initializer's already initialized `tmp_clear_render`/`PaletteReader`; it creates
no second VM, palette, SHP decoder, INI parser, rasterizer or body mask.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --building-health --check
```

`building_health.json` has two explicit comparison boundaries:

- `raster_controls` share the unit-health input/draw/pixel schema. Inputs use raw
  Object Location XYZ; `actor_results.coords` and `.point` come from original
  Building GetCoords447AC0 and Tactical6D2140. Positive selected rows enter the
  complete health function6F64A0 with third argument0. Hover-only, absent and
  sinking negatives enter complete Extras6F5190 instead, preserving its caller
  gates without adding selected bracket pixels to a health-only comparison.
  The exact native entry is recorded per actor. Health, selection, hover and
  sinking are supplied priors, not lifecycle proof.
- `composition_controls` execute complete Behind, complete first Extras, actual
  admitted GAPOWR body, and complete late Extras, with a second control adding
  one earlier unselected body and its empty Extras. Native4AED70 commands and
  entries carry phase/actor identities. `before_body_pixels`,
  `before_late_pixels` and `pixels` sample the locations touched by actual health
  stores; `body_overwritten_health_pixels` records any original first/body/final
  word differences; both controls record an empty difference list. They
  demonstrate no positive visible order difference for these placements.
  These are bounded consumer samples, not full color planes.
  Full native color/Z crop hashes and the native body-admission point/clip
  identify the intervening body interval. The two health phases remain explicit
  even where their repeated opaque pixels are idempotent.

The selected physical stack is RULESMD, optional LANGRULE, Battle MPBattleMD,
then XMP03T4. Fixed ARTMD supplies Building properties. Whole native
Rules665650/BuildingType45DD90 and BuildingType45FE50 execute; constructor and
per-layer fields include Strength, Height/Foundation, Pip/PipScale,
PipsDrawForAll, CanBeOccupied and SpawnsNumber. The shared original
ConditionRed/Yellow reader blocks consume each layer's AudioVisual section.
Stock GAPOWR resolves Strength750, Height4/Foundation3, dimensions512/512/416,
delta0, PipScale0, PipsDrawForAllfalse and CanBeOccupiedfalse. The ordinary
709A90 tail executes; its lack of extra SHP draws is bounded to these priors.

Physical PIPS.SHP21 frames, PIPBRD.SHP and PALETTE.PAL use the chain3 normal
STOCK_DIGITAL winners. Body dependencies GGPOWR.SHP6 frames, UNITTEM.PAL,
BUILDNGZ.SHA and fixed ARTMD were also resolved through the production asset
owner with no loose override; the corpus pins their actual archive bytes.
Original native palette/LightGold constructors run in the same VM. Original
Cell47BBF0/TMP480350 establish flat ground, and
43CEA0→455C20/459EF0→43D290→705E00→4AED70 establishes actual body admission,
render coordinates, SHP/shadow and BUILDNGZ depth. GAPOWR's native BibShape is
NULL. Sparse Cell enrollment, traversal order, placed byte6E4, House/light/frame,
startup height factor104 and camera/viewport are declared inputs.

Component controls cover HP750/375/187/1, immediate-above thresholds, A127/63/2/0,
old Z0, actual viewport-edge clipping and ordered overlapping strips. Building
health always draws15 physical PIPS frames, including frame0 for empty pips;
flags600 select the plain leaf and bypass A/Z. The recorder closes each shape
at its real caller return, so following bracket A/Z accesses do not contaminate
health/body policy counts. Original code and component A/Z remain unchanged;
no drawing RNG entry is observed. These samples do not establish other Building
health/status routes, selection/damage lifecycle, whole Display/Map loading,
a GPU body renderer or a whole native frame. Existing production replay and
runtime capture remain separate integration gates.

The Building component corpus retains `store_count` and per-draw `color_stores`
instead of ordered color-store arrays; no consumer needs a second bulk write
trace. Historical unit/bracket payloads retain their original schema and bytes.

## Ordinary mobile rank prefix

`--unit-rank` extends the existing health generator and `_HealthRecorder`; the
same physical PIPS/PIPBRD/PALETTE, native INI caches, palette initialization,
Surface and shape leaves produce `unit_rank.json`. No second VM, rank state,
SHP atlas, palette or raster implementation produces expected output.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --unit-rank --check
```

The18 `raster_controls` use the health corpus input/pixels/draws schema and
compact store counts. Actual native setters750080/750090/7500B0 and
7500E0(-100) produce `input.actors[].veterancy_raw_bits`; these bits are canonical
consumer inputs. The corresponding setter entry/arguments and original
GetCoords5F65A0/Tactical6D2140 point are recorded in `actor_results`. No host
rank formula supplies expected frames or colors.

Every row executes whole DrawExtras6F5190. Native sinking admission precedes
bomb/repair, then IsDisguisedTo(CurrentHouse) and VisualCharacter(false,NULL)
gate rank before the selected/hover health branches. Complete70A990 uses
PIPS14 veteran,15 elite,19 negative; ordinary rookie0 draws nothing. The
non-Infantry anchor is point+[10,6], flagsE00, remap0, signed Z-adjust-2,
gradient0, brightness1000 and PALETTE Convert87F6C4. Native493DF0/7E57E0
consumes A and bypasses Z. A127/63/2/0 and occluding Z0 controls retain original
code, actor bytes and both A/Z planes. A2/A0 make black stores rather than
skipping the chevron. Selected/hover rows continue through full health and
ordinary709A90/70AA60. The two-parent `ordered_overlap` includes eight actual
first-rank pixels overwritten by the later parent's health; the native values
are retained in `overwritten_rank_pixels`. `clipped` uses raw Location2976,4960,0
and original projection point148,80, producing18 visible rank pixels through
the full viewport clip.

The explicit constructor boundary follows the existing MTNK input owner:
Unit7353C0→Foot4D31E0 prefix stops7354CE before typed COM activation, followed
by original Drive4AF540. Supplied owner/link/active priors expose the actual
Drive55ABC0 callback to Foot4DA4E0; its zero result is executed, not stubbed.
Whole typed Unit construction, Unlimbo, placement, movement and promotion
lifecycle remain excluded. The prefix consumes one original Scenario RNG draw
at6F3259 and initializes the recorded passive scan timer; the measured drawing
intervals consume no RNG, update no timer and detach nothing.

Observer controls reuse the existing constructed flat Cell setup in the same
owner, with explicit local/enemy House indices0/1, no mutual alliance, Cell+7C
sensor and+AC disguise-detection counters. Native746750/41C010/4F9A50,
4DA4E0→55ABC0→703860 and4870D0/4870F0 execute. Inputs spell out
`owner_is_current`, `owned_by_current_house`, `cloak_state`, `cloak_progress`,
`disguised`, `disguised_as_current_house`, `sensor_count` and
`disguise_detect_count`. Enemy cloak state2/no sensor yields visual5 and
suppresses rank; own or sensed state2 yields3 and draws. Enemy disguise as the
observer suppresses rank until its detection counter is positive. These are
prepared consumer states, not claims that stock MTNK intrinsically produces
cloak/disguise. Rank material remains opaque and does not inherit body cloak
opacity/tint. `gates` records original results and `callbacks` records reached
native dependencies.

`vtable_bindings` reads active Unit7F5C70, Infantry7EB058 and Aircraft7E22A4
slots: all+110 use6F5190 and+454 use70A990. Their visual entry is shared
Foot4DA4E0; Unit/Infantry disguise queries differ and Aircraft returnsfalse.
Original70A990 selects point+[5,2] for Infantry RTTI15 and+[10,6] otherwise.
These bindings and instructions protect the shared production consumers;
full Infantry/Aircraft constructors, bodies and scenario compositions are not
established by the MTNK component corpus. All historical health/bracket/default
modes retain their schema and outputs.

## Ordinary IFV passenger pips and affected shared scales

`--passenger-pips` extends the same health generator, recorder, PaletteReader,
Surface and SHP owners. It writes `passenger_pips.json` and its sidecar; all
historical modes retain their output and provenance bytes.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --passenger-pips --check
```

The root health schema remains `readers`, `physical_inputs`, `palette` and
`raster_controls`. Actors declare `type_name`, passenger/unload, canonical rank
bits and ordinary selected/hover/sinking/Location priors. Original FV type
readers establish Strength200, Passengers1, PipScale3, Gunnertrue, Size3 and
SizeLimit1. Original E1/ENGINEER readers supply transport Pip3/5. Complete
6F5190→6F64A0→709A90/70AA60 executes, including actual716290 capacity,
473460 current-type-size walk,473450 head and the native frame array. The
one-slot FV draws PIPS2 frame0 empty,3 GI,5 engineer at point+[−15,+10].
Native4AED70 flags600 reach4912B0/7E5A20: no A/Z access or depth writes.
A127/63/2/0, oldZ0, clipping and ordered parent controls retain native stores.
The shared integer-size control uses physical LCRF Passengers12, SizeLimit6,
Size16, Strength300 and MTNK Size3. Original Cargo.Add4733A0 reaches Unit
Limbo4DB260/Techno6F6AC0 with the passenger's constructor Limbo1; the retained
count is1 and native473460 returns3. Whole Extras emits raw PIPS2 frames
`[5,3,3,0,0,0,0,0,0,0,0,0]`:709F0A..709F23 writes Size−1 fillerframe3 at
higher indices **before** placing Unitframe5 at the lower index. This
distinguishes the order that a size-one passenger cannot expose.
`integer_size_passenger` declares actor `type_name=LCRF`, `passenger=MTNK`;
`actor_results.cargo_size` records the native total. Its original Unit prefix
uses real Hover513C20 for LCRF (physical Locomotor742) and Drive4AF540 for
MTNK, with explicit interface linkage. The control establishes frame
consumption and Cargo operations; whole construction, transport boarding,
hover movement and placement remain outside this boundary.
The overlap has points[80,80] then[80,115]; twelve earlier scale pixel values
are overwritten by the later health strip, recorded in
`overwritten_scale_pixels`. This is a positive ordering witness for migrating
the former global scale bucket into the shared parent Extras owner.

The original Unit/Foot/Drive prefix is reused. Infantry517A50 stops517AE6
before ID/COM activation; original Walk75AA90 and explicit interface linkage
expose real callbacks. Constructor Limbo1 permits whole Cargo4733A0→51DF10→
4DB260→6F6AC0 and473430. Thus count/head/link values come from native cargo
operations. Full typed construction, placement, boarding/unloading missions
and passenger cleanup remain production lifecycle validation, not oracle
claims. Constructor Scenario RNG requests/passive timers are separate from
zero measured drawing RNG, timer updates and detach calls.

`read_pip` contains constructor defaults, the original table, direct native
controls and sequential admitted-section cases. Both Pip5240E3 and
OccupyPip5240FD always call4748A0 after base section admission. Missing names
use the current frame as a table index, so admitted unrelated-key passes
change fields; absent sections retain them. The recorded pairs are1/7→3/11,
absent3/11, then4/1→5/2→7/3, then explicit3/11. Indices11/12 read adjacent
Ammo/Tiberium default names and return1. Explicit empty-cache controls are
separate from physical INI empty-value omission. These controls are consumed
by the existing retained type/INI reader owner, not a new parser.

Affected MIND controls reuse original4717D0 construction and declared retained
manager vector/count; native Primary/Damage/InfiniteMindControl readers and
4722D0/4726C0 execute. HARV controls supply authoritative consumer storage
slots and execute6C9650/6C9680 and independent ore/gem counts. Ambient0E7F
matters: Storage40 amount4 yields0, amount5 yields1, while20 yields3. Gems
are drawn first, then ore, with exactly five slots even when rounded counts
sum above five. `numeric_results` captures the actual native post-ftol counts.
Neither capture/overload nor harvest lifecycle is established by those priors.

`garrison_controls` borrows the existing Building health runtime in an
additive NABNKR health-only configuration. Full original BuildingType45FE50
reads stock RULES layers/fixed ART; original Building constructor/idle,
GetCoords447AC0 and Dimension2464AF0 supply geometry. Direct full6F64A0
consumer calls reach709C27..709D17 with a declared one-E1 occupant vector:
raw OccupyPip11 and reader-evolved1 are followed by four emptyframe6 pips,
flags600 and step[4,2]. This tests the shared full-PIPS resource/material;
whole Building Extras admission, bunker body and garrison-entry lifecycle
are excluded. Stock battle bunker is NABNKR; no CABUNK section is invented.

RULESMD, optional LANGRULE, selected MPBattleMD/map and fixed ART inputs are
recorded separately. Section lookup526810 returns a pointer; admission tests
full EAX rather than treating its low byte as a boolean. Original5F76B0 pushes
the literal `PIPS2.SHP` at5F7758
through CCFile65CA80/loader4A3890, retainsAC1480 at5F7776, and709AEF consumes
that same asset. Independent normal STOCK_DIGITAL AssetManager lookup selects
`ra2md.mix/conqmd.mix/PIPS2.SHP`:1,384bytes,19frames, SHA256
`0afe9935160c04bdabed5bfefd0d55aad645ae38ec7dddeb5b3f4f2b2d2712d4`,
with no loose override or catalog-only hit. The older RA2 conquer copy is
shadowed. The oracle supplies these selected immutable physical bytes;
the native archive search itself is outside this runtime boundary.
Stock size-one infantry match the current integer cargo owner. Fractional
Size/SizeLimit and per-type reload behavior remain explicit residuals; the
oracle adds no competing size, cargo, palette or raster implementation.

## Ordinary Harrier ammo and the shared pip-fill query

`--ammo-pips` adds `ammo_pips.json` and its sidecar through the same health
status generator, mobile-prefix helper, recorder, PaletteReader/native cache,
palette/Convert and Surface/shape owners. The six historical modes keep their
payloads and sidecars unchanged. No second runtime, actor bootstrap, cargo,
manager, storage, parser, palette or numeric emulator supplies expected values.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --ammo-pips --check
```

The existing root schema (`readers`, `physical_inputs`, `palette`,
`raster_controls`) is retained. Actors declare `type_name=ORCA`,
`kind=aircraft`, `ammo_initialization=full|depleted|rearmed`, canonical rank bits
and ordinary health/selection/observer/raw Location priors.
`actor_results` includes `initialized_ammo`, `current_ammo`, `pending_release`,
`fill_count`, `pip_max`, native GetCoords/projection and `ammo_events`.
`full` executes41403A..414051's InitialAmmo fallback; `depleted` additionally
enters the admitted pending writer41840E..418415 and next-entry consume
418031..418056; `rearmed` then invokes the whole original Aircraft radio
receiver4190B0 with message1F, reaching actual Techno6F4C9C. Admission to that
pending writer is supplied. These controls do not run a shot, mission scheduler,
return flight, dock service or selection lifecycle.

Whole original AircraftType41C8B0/Rules665650 constructors execute, followed by
selected native type read/store blocks on physical RULESMD, absent LANGRULE,
MPBattleMD and XMP03T4. ORCA's PipScale read714120 produces Ammo1; PipWrap
constructor710D40 is0 and ReadInt714188/store714198 retains0; InitialAmmo
constructor7110E6 is−1 and ReadInt71475B retains−1; Ammo constructor7110EC is−1
and ReadInt714775 reads physical1. These readers retain their current defaults
and apply no clamp. Strength150/delta0 and existing numeric dependency type
inputs come through the same reader owner. Fixed ART is not layered and
supplies none of these selected status keys; full type/ART/body readers are
outside this component. Authored nonzero PipWrap is a dormant branch in this
physical stack, not unreachable or equivalent to the unwrapped arm.

The shared mobile-prefix helper executes Aircraft413D20..413DB1 before COM,
then original Fly4CC9A0 and its actual55ABC0 visual callback through Foot4DA4E0.
Interface owner/link/active and scene priors are supplied, as with existing
Unit/Drive and Infantry/Walk controls. Constructor Scenario RNG request65C780
at6F3259 and passive timer `[0,0,45]` are recorded separately. Every raster row
runs complete6F5190→6F64A0→709A90→70AA60 with original observer gates,
708C30 and716290, projection, SHP selection, picker and pixel leaves.

Stock full ORCA emits one YR PIPS2 frame13 at GetCoords' projected point plus
`[-15,+7]`, flags600, step[4,0]. Depleted0 emits no empty ammo pip. The actual
4912B0/7E5A20 leaf bypasses A/Z, while PIPBRD retains E00/A consumption. Rows
cover A127/2/0, occluding Z0, hover/deselection/sinking, actual viewport clip,
supplied airborne Z1500 and ordered parents. Native projection derives the
clip point[17,80] from XYZ[2416,5520,0], and airborne[80,81] from
[4544,7104,1500]. Ordered points[80,80] and[80,115] produce sixteen earlier
ammo pixels overwritten by the later health strip, recorded in
`overwritten_scale_pixels`; legal dense-unit placement is not claimed.

`fill_controls` executes complete original708C30 on the same retained actor,
manager, cargo and float-storage owners. Each row has `input`, actual
`type_fields`, native `pip_max`, signed `result`, native spilled `arithmetic`
operands and directly observed `ftol` EDX:EAX/FPCW at708D2A, before the
positive-total minimum. `arithmetic.storage_total` is observed at6C9677:
6C9650 truncates each signed integer accumulator plus the next f32 slot, then
returns the final integer as binary64. No host sum supplies that operand.

Stock ORCA counts MIN,−2,−1,0,1,2,MAX yield MIN+1,−1,0,0,1,2,MAX. HARV
Storage40 with amounts0,1,4,5,20,40 yields0,1,1,1,3,5; raw pre-minimum ftol
values are0,0,0,1,3,5. This shared quantity differs from the independent
ore/gem raster rounding in the passenger corpus. MIND counts0/3/4 and actual
FV Cargo count0/1 exercise their same shared query. Explicit configuration
boundaries execute original ReadInt blocks for Storage0/−1 and Ammo0, and
ReadPipScale for Power/None, then restore through those readers. Storage0 with
positive amount yields invalid signed64 conversion EDX:EAX80000000:00000000,
then minimum1; negative Storage−1 with amount1 yields−4. Power/None return0
without arithmetic. Those configuration and extreme signed-count inputs do
not claim ordinary stock reachability, and huge counts are never rasterized.

Original code/vtables, actor bytes and A/Z remain unchanged over the measured
queries/draws; they draw no RNG and perform no timer write or detach. Full
construction, native archive search, shroud generation, VXL body composition,
world Display, combat/flight/dock lifecycle and other PipWrap/status routes
remain separate owners or residuals. Normal STOCK_DIGITAL physical PIPS2
winner and native5F76B0 filename lookup are the same as the passenger mode.

## Ordinary control-group number

`--control-group-status` extends the same health generator/recorder and native
MTNK Unit/Foot/Drive/type owners. The existing
[control-group font owner](../control_group_text_oracle.md) now accepts that
same PaletteReader VM and executes original GAME.FNT initialization and glyphs.
Historical seven selected modes and the old11-case font-submission payload
retain their exact outputs and provenance.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --control-group-status --check
```

The new `control_group_status.json` follows `input.actors`/`actor_results`,
physical inputs/readers/palette and final sparse `pixels` used by the existing
status GPU consumer. Original731060/7310D0 produce each actual signed
`group_slot`; `group_commands` carries native parameters1..10 and the selected
prior used for assignment. Selection is restored to the supplied render state
before whole6F5190→6F64A0→709A90/70AA60→70A6FD executes. No persistent group state
is duplicated. `assignment_controls` records eligible/ineligible-owner native
before/after fields in mode0 and mode1; timer bytes stay unchanged.

Original7BB020/7BB050 fill,7BAD90/7BADC0 outline and
4A66D0→4A5EB0→434B90→434500→434120 font stores execute to their real returns.
`text_draws` exposes native rectangles/text/origin/packed owner word, stage
counts and actual glyph stores; `overwritten_group_pixels` demonstrates the
positive per-parent opaque and later-health overlap. A127/63/2/0, oldZ0,
selected/hover/absent/sinking, cleared/unassigned and viewport clipping are
bounded controls. Font/rectangle writes bypass A/Z; they are independent of
4BFD30's line material. The physical Gold RGB prior comes from the consumed
native House-color owner on the same RULESMD hash, then original code packs565.
Normal STOCK_DIGITAL GAME.FNT resolves ra2.mix/local.mix with no loose override;
its exact physical hash and original loader read ranges are recorded.

Full Unit construction/COM, Unlimbo, selection and ownership lifecycle,
collision-valid dense placement, VXL body, whole Display/frame, non-allied
Techno+210 and other status families remain outside these controls. Building
PipScale2 is Tiberium and exits709A90 at709B68 before the number tail; it is not
Ammo1. Original constructor Scenario RNG/passive timer initialization is
recorded separately from unchanged measured draw actor/A/Z and zero draw RNG.
Whole DrawExtras/health/pip-tail and other RTTI lifecycle routes remain
OPEN/PARTIAL.

Affected self-heal controls reuse the existing House counter/leaf owner from
`tools/selfheal_oracle.py`: `input.self_heal_infantry` and `self_heal_units`
are declared House priors, and `actor_results.self_heal_queries` records real
50D9C0/50D9D0 results. Original General ReadInt blocks66E6E2/66E715 consume each
physical layer's SelfHealInfantryFrames/UnitFrames, retaining50/75. The stock
E1 type/Infantry/Walk setup reuses the passenger prerequisite owner. No new
House counter, Infantry VM/bootstrap or simulation lifecycle is introduced.

At point[80,80], actual healthy MTNK self-heal draws PIPSframe20 at[103,58]
(relative[23,-22]); E1 draws frame13 at[95,55] (relative[15,-25]). An explicit
MTNK Organic=yes scenario boundary executes original715024..715045 ReadBool:
its frame13 still uses the Unit anchor[103,58]. PixelSelectionBracketDelta11
through native71415F..714179 shifts health while self-heal stays at[103,58].
The anchor follows RTTI, rather than the frame/organic arm; self-heal does not
add the bracket delta. Healthy600 markers bypass A/Z, through actual4960A0
(frame20) or4912B0 (frame13). The positive two-parent self-heal/group row records
later opaque group stores overwriting earlier marker pixels.

These five added controls cover affected material/geometry/order consumers.
House grant/revoke and self-heal simulation timing remain their existing
owners; only an admitted counter prior is exercised here. The separate damaged
first-six-frame601 material branch remains an explicit residual, not an extra
draw: original70A626 chooses one600 or601 call.

`--repair-wrench` adds ordinary repairing GAPOWR to the same Building-health
runtime and phase recorder. It preserves every historical mode and sidecar.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --repair-wrench --check
```

`repair_wrench.json` supplies `readers`, physical inputs, the actual MOUSEPAL N1
palette, `raster_controls` and `composition_controls` to the shared status GPU
consumer. The existing palette owner optionally executes original startup
52C136..52C1EA, including physical MOUSEPAL copy/expansion and complete Convert
construction; original45E98D..45E9A3 loads WRENCH.SHP. Archive hashes identify
candidate inputs; normal STOCK_DIGITAL AssetManager checks independently gate
winner and loose-override precedence. This fixture does not emulate archive
search. Existing PALETTE N53 remains available for health in complete Extras.

Component `input.entry_boundary="extras_prefix"` starts actual6F5190 and stops
before6F534E, after its wrench branch, or at the earlier sinking return. It
executes the real RTTI/repair-byte/Cell487950 gates,459EF0 render coordinates,
Tactical projection, SpeedNormalize/frame arithmetic and every4AED70/493DF0
pixel store. It does not treat this prefix as whole DrawExtras. The frame0 and
frame6 controls, A127/63/2/0, oldZ0 and actual viewport clipping establish E00
A-consuming/no-Z stores with the N1 table. N1 produces identical colors for all
four A controls. Selected/hover/other-owner inputs retain the wrench; repair-off,
sinking and shrouded-cell inputs suppress it. Cell flags08 alone are admitted,
which differs from a UI reveal-and-gap approximation. These are declared
render-consumer priors, not native repair, ownership or vision lifecycles.

Complete composition rows execute Behind, whole first Extras, actual admitted
43CEA0→43D290 body and whole late Extras. Wrench precedes bracket/rank/health
inside each Extras. Physical TMP ground, GGPOWR body, BUILDNGZ and ColorScheme
reuse their existing native owners; GAPOWR has NULL BibShape. The neighbor is
another stock GAPOWR two cells east, with disjoint 2×2 foundations, traversed
after the repairing actor in both supplied parent phases. Sparse ground and
before-late color/Z samples plus final pixels are the GPU seam; body crop hashes
and actual body-admission records identify the executed interval. This is not a
GPU body comparison or whole Display traversal. `body_overwritten_wrench_pixels`
and `overwritten_wrench_pixels` expose actual positive overwritten stores,
including later-neighbor status over the earlier wrench. Their values come only
from original execution, rather than inferred geometry overlap.

Full Rules/BuildingType constructors and45FE50 readers consume RULESMD,
LANGRULE when present, selected MPBattleMD and XMP03T4; ARTMD is fixed. The new
mode additionally records native constructor/layer Strength, Cost, Repairable
and ClickRepairable. The supplied private Building repair byte belongs to the
existing repair owner; command admission, Flash7, payment, healing and every stop
remain its production lifecycle tests. Drawing itself writes no actor/timer,
consumes no RNG and calls no detach. Arbitrary Building types, missing resources,
large foundations, other status families and whole function/class coverage stay
OPEN/PARTIAL.

## Ordinary Crazy Ivan bomb clock

`--bomb-clock` extends the same native status generator and recorder with
ordinary IVAN attachment, MTNK clock drawing and the existing GAPOWR
first-Extras/body/late-Extras composition. Historical modes remain unchanged.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --bomb-clock --check
```

`bomb_clock.json` exposes `readers`, `physical_inputs`, actual MOUSEPAL N1
`palette`, independent `health_palette`, `status_runtime`, `raster_controls`
and `composition_controls` to the shared GPU status consumer. Whole
Rules665650 establishes constructor defaults100/450/8 for IvanDamage,
IvanTimedDelay and IvanIconFlickerRate. Original current-default ReadInt5276D0
blocks read the physical RULESMD/LANGRULE/Battle/XMP03T4 layers, ending450/450/8;
native66C5F4 loads literalBOMBCURS.SHP intoRulesFE0. The clock has no ART section.
Original Init52C136..52C1EA loads/expands physical MOUSEPAL and builds its N1
Convert87F6C8. Asset candidate hashes are retained; the independent production
AssetManager consumer establishes normal winner and loose-file precedence.

The existing native Infantry/Unit prefixes and Drive prepare actors in the same
PaletteReader/TMP VM. Actual Attach438E70 creates the record at each supplied
`bomb_start_frame`; UpdateAll438BF0 twice refreshes native BombVisible68, while
one call leaves an armed, unshown record. `actor_results[].bomb` records native
kind/start/end/visible/countdown and actual Attach/refresh entries. The sibling
`clock_frame` is the original438A00 result. Source `bomb_visible` describes the
requested refresh boundary; no host-computed end/frame or second state machine
initializes a golden. Constructor soundIDs stay-1 in this rendering profile.
The independently consumed `tools/spatial_oracle/bomb_class.json` owns native
bullet admission, visibility/detectors, sound, strict expiry, blast/defuse and
pointer cleanup; production Bomb remains the exclusive gameplay owner.

Component `input.entry_boundary="complete_extras"` executes whole6F5190:
sinking3CD rejects everything; BombVisible68/AttachedBomb38 admit the clock,
then actual Cell487950/47B3A0/586360 and render-coordinate/Tactical projection
execute. Native4AED70/493DF0 draws flagsE00, remap0, Z-adjust0, gradient0 and
brightness1000. It consumesA and bypassesZ; N1 gives identical RGB565 colors
atA127/63/2/0, even with oldZ0. Selected and hover controls retain clock before
their health; absent bomb, unshown, shrouded and sinking controls suppress it.
Frame/blink/hand/end-boundary controls call original438A00, whose arithmetic
already has consumed BombClass controls. Kind1/death bookkeeping remains outside
this ordinary timed profile.

The clock explicitly passes global tactical clip886FA0 at6F522A. `global_clip`
supplies a different parent rectangle and retains the clock's actual global
clip. Cell raw bit08 alone admits it. Two raised controls use the existing
`initialize_discovery_geometry` owner before Cells: original CRT startup yields
Cell/shroud level104, level2 gives groundZ208, and native487950 projects origin
cell10,20 into9,19. `cell_overrides` declares each native cell/level/bits input.
Projected-only reveal admits the clock; origin-only reveal rejects it. These
are visibility-consumer controls, with no native reveal lifecycle claimed.

`ordered_overlap` records positive first-clock overwrites by the later parent.
The GAPOWR controls share original flat TMP, Building/idle, BUILDNGZ and body
owners. Complete Behind/first Extras/body/late Extras execute with the clock
before wrench and health in each visit, plus one later neighboring GAPOWR.
Sparse ground/before-late color/Z and final status pixels cross to the GPU
consumer; actual body/status clock overwrites are retained as witnesses. This
does not compare GPU body raster or execute a whole Display traversal.

Drawing preserves actor/bomb bytes andA/Z and has no RNG, timer write or detach.
Setup Attach writes start/end/countdown1, then UpdateAll writes countdown0/45
and carrier visibility/redraw; constructor RNG/passive timers are recorded
separately. Selection, placement, supplied frame/terrain/House and traversal
are explicit consumer priors. Other types, missing assets, arbitrary clocks,
full DrawExtras/Bomb functions, and whole native frame coverage remain open.

## Ordinary selected GAPOWR Power/Drain text

`--building-power-status` adds `building_power_status.json/.meta.json` through
an opt-in mode of the existing PRIMARY generator. The Building/PaletteReader,
physical layers, Ground2 caller, text recorder, GAME.FNT loader, House colors
and packed Surface raster remain the same owners. Historical PRIMARY, group
and every other corpus are preserved.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --building-power-status --check
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.shroud --primary-factory-status --check
```

Original normal TacticalDraw calls6D8DB0 at6D465F. Its after-Ground2 sweep
6D9423..6D95A1 walks the native constructor-registered Building array, tests
marked74/body-opportunity99 and viewport bounds, then calls43E7B0. Selected83
and owner-allied-to-current admit virtual+458=70AA60. Its first arm requires
Building RTTI6, positive rated Power atType+EE0 and false primary3D3. Ordinary
stock GAPOWR satisfies those type/primary conditions. No debug/info switch is
read in this call chain; earlier Display/selection writers remain instruction
and production evidence, rather than a complete native frame in this corpus.

Whole original45DD90/45FE50 readers record constructor Power output/drain0/0,
physical RULES200/0 and Strength750, then retained values when mode/map sections
are absent. `reader_controls.{constructor,history}` additionally executes full
45FE50 on a fresh native GAPOWR type in the same VM. Each history row keeps raw
`sections`, normalized `cached_sections`, `admitted`, actual `power_reads`
(default/value from5276D0), and native `after` fields. Existing lexical input
preparation omits physical empty entries. Missing/empty values retain current;
present junk reads0; key case is exact; numeric suffix17junk reads17. Original
signed overflow2147483648 readsINT_MIN and NEG32 leaves the drain slotINT_MIN;
a later missing read reconstructs that same signed default. These are cached
native reader controls, excluding full native file loading and synthetic
stored-empty entries.

Native House constructor-store prefix4F5DCC..4F5DF6 publishes actual House+24
interface7EA834. Draw70AA60 queries its drain slot24=4F6A10 first and output
slot20=4F6A00 second. They return retained House53A8/53A4 without recomputing.
`input.retained_house_power` is keyed by `Owner`/`Ally`, each with signed
`output`/`drain`; those are declared consumer priors, including zero, signed and
other-owner totals that differ from the visible plant's type or fresh sum.
`getter_calls` records original return EAX and owner. The existing PowerState
and power-assessment lifecycle remain authoritative; this oracle adds neither.

Physical `TXT_POWER_DRAIN2` is `Power = %d Drain = %d`. Original CRT7CA564 and
7D21BE execute both substitutions; `format_calls` observes actual arguments,
return count and UTF16. The existing shared `install_digit_format(native=True)`
only installs the established single-thread Interlocked import owner. Its
default single-digit transport remains unchanged. No host string formatting,
text measurement, clip, rectangle, glyph or pixel algorithm produces a golden.

Seventeen `raster_controls` preserve the PRIMARY row/input schema and use
`current_layer=2`: selected/deselected/hover-only, mark/opportunity, directional
alliance/nonallied-zero-mask, four A values, occluding Z, viewport edges,
retained caches and two positive overlapping parents. Original4A59E0/421B60,
black fill/House-color outline and4A5EB0/GAME.FNT raster execute in order for each
parent. `text_draws` keeps actual text/origin/rect/stage A/Z counters; `pixels`
keeps final sparse packed words. Unused individual glyph memory events are
omitted in this new mode. Two separate `caller_controls` execute native layer1/3
rejection; they do not require another production phase decision in the frame
producer. Historical PRIMARY remains the consumed stock GAWEAP nonpositive
Power/PRIMARY fallback guard.

Every measured row asserts unchanged actor/House bytes, A/Z and original code,
with no drawing RNG, timer write or detach. Selected/hover, mark/opportunity,
health/location, palette and retained House totals are declared priors. Full
House construction/assessment, selection, Display traversal, device output,
nonallied+210 production, arbitrary strings/locales and whole functions/classes
remain OPEN/PARTIAL.
