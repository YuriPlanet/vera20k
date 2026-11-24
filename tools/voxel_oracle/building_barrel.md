# Building voxel turret and barrel submission

`building_barrel.py` executes the original retail instructions from
`gamemd.exe`.
Its `.json` records 21 loader cases and 56 draw cases; the `.meta.json` records
Unicorn versions, substitutions and the payload hash.

```sh
python -m tools.voxel_oracle.building_barrel --check
python -m tools.voxel_oracle.building_barrel --raster --check
python -m tools.voxel_oracle.barrel_pitch --check
```

These are component comparisons. They establish the original filename and slot
control flow, integer frame/cache selection, submission order and matrices on the
supplied boundaries. The draw corpus does not compare native VXL/HVA decoding,
raster output, cache storage, game scheduling or a complete live match; the
[stock gun rasters](#stock-gun-rasters) below add the raster. The existing
`barrel_pitch` oracle remains the owner of shared unit-pitch/offset comparison;
this fixture imports its camera and facing setup rather than reproducing it.

## Active callers and loader ownership

BuildingType `LoadVisualAssets` (`0x45F230`) calls `0x45FA90` at `0x45FA78`;
the saved-type load also calls it at `0x4651EF` and `0x46527F`. This is the
building-specific loader. ObjectType `0x5F8110` and its `%sBARL` convention do not
establish the building's name. The correction on issue #829 is supported, with an
additional prefix boundary described below.

At `0x45FA9C..0x45FAAA`, either `TurretAnimIsVoxel` (`type+0x16C5`) or
`BarrelAnimIsVoxel` (`+0x16C6`) admits loading. The name starts as `TurretAnim`
(`+0x11B0`). Original `makepath`, `strupr`, `strstr` and `strncpy` execute:

- The first `TUR` search starts four characters into the `.VXL` filename
  (`0x45FB06`), after uppercasing that suffix. A match loads the original name
  into turret VXL/HVA slots `+0xB8/+0xBC`.
- The subsequent search starts four characters into the basename
  (`0x45FC56`). Its first match is overwritten with `BARL` and NUL
  (`0x45FC73..0x45FC82`). It truncates any suffix after that match.
  `GTGCANTUR` therefore supplies `GTGCANBARL`.
- With no match, the unchanged name goes to barrel VXL/HVA slots `+0xC0/+0xC4`.
  If `BarrelAnimIsVoxel` is set, `VoxelBarrelFile` (`+0x1714`) replaces the name
  on this no-match arm. A `TUR` match takes priority over that explicit name.
- Each VXL availability gate controls replacement of that VXL and its HVA.
  An unavailable file preserves previous pointers. HVA construction does not
  have a second independent availability gate. Fresh fixtures begin with null
  slots; a separate reload fixture preserves all four prior pointers.

For example, `SAM` is a lone native barrel-slot model, not a body plus an optional
barrel. `ABCTUR` has no eligible match while `ABCDTURSUFFIX` becomes `ABCDBARL`.
Mixed-case rows show that the original leaves the first four characters alone;
archive/file selection is a supplied boundary in this corpus.

Fixture file constructors/availability, allocators and asset constructors/destructors
are explicit hooks. The HVA constructor supplies a scale-ready object; the alternate
native MotLib scaling path and missing/corrupt HVA behavior are outside this corpus.
No executable instructions are patched. The allocation-failure control records
native slot publication without claiming normal resource-exhaustion recovery.

## Original building draw

The Building vtable at `0x7E3EBC` holds `0x43DA80` at `+0x4E4`.
`DrawIfVisible` (`0x43CEA0`) calls it at `0x43CFE9`, using the render-coordinate
virtual (`+0xAC -> 0x459EF0`) through `CoordsToClient2`. Within `0x43DA80`,
`TurretAnimIsVoxel` admits the arm examined here after construction/selling gates;
`Artillary` is a separate gate override. The corpus starts at `0x43DFC1` after
those admissions.

Original `FacingClass::Current`, identity, rotation, translation, camera copy and
matrix product execute. The cell lookup supplies a cell with top 7. Native
`Techno::DrawVoxel` (`0x706640`, vtable `+0x444`) is intercepted at entry to record
its ten arguments, including final matrix and per-part cache owner. The point is
fixture `[100,200]` plus fixture `TurretAnimX/Y=[3,-60]`, hence `[103,140]` for every
submission. The [stock gun rasters](#stock-gun-rasters) follow that point to the
blit; no later correction applies.

With a native turret VXL (`+0xB8`) present:

1. Start at identity, rotate Z by the primary facing's rounded 32-step angle,
   and translate local X by **signed truncation of TurretOffset/8**
   (`0x43E0C9..0x43E0E3`). This differs from the unit draw's units-per-lepton scale.
2. Copy that turret matrix to the barrel, subtract its translation, apply each
   part's own recoil displacement along local X, pitch the barrel about Y, and
   restore its translation. Turret recoil does not carry the barrel: the copy is
   made first. Barrel recoil applies before pitch.
3. The rounded 4-quadrant facing, `(((raw >> 13)+1)>>1)&3`, draws barrel before
   turret for quadrants 0/3, after for 1/2. All 32 facing centers are executed.
4. Turret frame is signed `TurretAnimFrame % turret_hva_frames`. Barrel frame
   is **0**, even when the barrel HVA has more frames (`0x43E2FA/0x43E405`).
5. Both submit cache key `facing32 | ((turret_frame & 255)<<16)`. Pitch is absent.
   Missing VPL or either active recoil state sets the key to `0xFFFFFFFF`.
   Active zero-travel recoil still disables the key. The distinct owner pointers
   are type+`0x258` (turret) and type+`0x280` (barrel).

With no native turret VXL, the barrel-only arm (`0x43E41D`) requires both barrel
VXL/HVA pointers. It yaws and pitches the identity matrix, uses the barrel HVA's
own modulo frame and cache key, and does **not** read `TurretOffset` or recoil.
The corpus compares this arm with and without pitch and with irrelevant nonzero
recoil/offset controls. Missing barrel is also a turret-only control.

The shared pitch formula is the original rounded facing step minus8 followed by
negated Y rotation (`0x43E1CB..0x43E1F4`, barrel-only `0x43E4F9..0x43E51F`).
The 56 cases include fractional/negative recoil, signed frame remainder, offset
boundaries -9/-8/-7 and7/8/9, pitch, 32 facing centers and VPL absence. Matrix bits
are recorded before rasterization; matching them does not certify pixel parity.

## Stock gun rasters

`--raster` writes `building_gun_raster.json`. It takes the nine stock gun images
(GTGCANTUR, GTGCANBARL, FLAKTUR, SMINTUR, SAM, LASER, YAGGUN, OUTP, CAHEAD) from
the configured install's `ra2md.mix -> localmd.mix`, then `ra2.mix -> local.mix`.
For each of eight facings, plus the Grand Cannon barrel raised one step, it runs the
draw above and passes each submitted matrix to the shared
[raster owner](raster.py). That executes `706ED0`'s Render core through `754510`.
Each row keeps the six rect words, the crop's SHA-256, lit count and lit bounds,
with asset identities but no asset bytes. All 80 rows are recoil-free, TurretOffset 0,
HVA frame 0 submissions with zero matrix translation.

The crop is what both blits draw, at the submitted point plus `rect[0..1]`:

- Cache miss: `706ED0` adds the rect origin to the point (`0x007071CB..0x007071E2`)
  and blits `rect[2..5]` of the shared raster surface (`753C70`) through `4AF2A0`
  (`0x00707235`).
- Cache store: `6C89E0` writes the record's x/y/width/height from rect words
  0/1/4/5 (`0x006C8A16..0x006C8A34`) and encodes rows from the crop's top-left
  (`0x006C8A3C`).
- Cache hit: `707480` adds the record x/y to the point (`0x007076A2..0x007076CE`)
  before `437A10` (`0x00707736`).

These three blits are instruction readings; the corpus executes the raster only.

`app::presentation::instances::shp::building_voxel_tests::retail_building_guns_draw_the_native_crop_at_the_native_origin`
loads the same files through the production asset owner and checks their
identities. Stock ART leaves `TurretOffset` unset on all eight buildings. Each row's
production atlas sprite must equal the native crop and rect, and the production
emitter must place it at `[103,140] + rect[0..1]`. Lighting pages are inside that
crop hash; palette conversion, the RLE blit, cache lifetime and surface pixels
are not compared.

## Barrel elevation lifecycle boundaries

Building construction first calls `FacingClass::Set` with the low byte of stored
`type+0x1710`, shifted left 8 (`0x43BA48..0x43BA5E`) and then calls `Set(0x4000)`
(`0x43BAB9..0x43BACB`). These are gradual setters, not independent standing
pitch authorities.

The live Building Unlimbo calls the shared Techno Unlimbo at `0x440B0F`.
The original shared arm `0x6F6DAF..0x6F6DFA` sets current elevation to `0x4000`,
then sets desired to `0x4000-(FireAngle byte<<8)`. No later Building Unlimbo
barrel write overrides it. The production owner therefore remains shared.

The apparent sale write at `0x44A08C` addresses the newly undeployed unit and is
behind `Artillary`; it is not an unconditional sale reset of the standing building.
Mission_Missile's elevation writes (`0x44CEE2/0x44CF7A`) are behind
`EMPulseCannon`. These are dependency boundaries, not reasons to change the ordinary
Grand Cannon Attack path. Constructor, sale and Missile notes are instruction-level
findings; this fixture does not claim their timer arithmetic execution.

No RNG draw, timer write or detach call occurs in the bounded loader-name/matrix
blocks. Whole native asset destruction and draw-cache lifecycle are supplied or
excluded as stated above. Recoil lifecycle arithmetic has its own shared simulation
owner and [native comparison](recoil.md); this corpus supplies its already-computed travel/state.

## Retail boundary receipt

Production `asset ini-get` on Hills.mmx, Battle mode 1 reloaded `RULESMD.INI`,
absent `LANGRULE.INI`, `MPBattleMD.ini`, then the map. GTGCAN reads
`TurretAnim=GTGCANTUR`, `TurretAnimIsVoxel=true`, no `FireAngle` override
(default 8), and no `Artillary`, `EMPulseCannon`, `BarrelAnimIsVoxel` or
`VoxelBarrelFile` override. Fixed ART has no `TurretOffset` override for GTGCAN
or CAEAST02 (default 0). These are production accessor observations, distinct
from the supplied native CPU fixtures. Reproduce key checks with:

```sh
asset ini-get GTGCAN TurretAnim --domain rules --reader raw --map Hills.mmx --mode-id 1
asset ini-get GTGCAN FireAngle --domain rules --reader int --default 8 --map Hills.mmx --mode-id 1
asset ini-get GTGCAN TurretOffset --domain art --reader int --default 0
asset info GTGCANTUR.VXL
asset info GTGCANBARL.VXL
```

The rules source is `expandmd01.mix` (`rulesmd.ini` SHA-256
`3d341ef8a13a4b5ab24af2eef48ac94931ac2bb87d950fe3330a07e2d25672ef`);
fixed ART is `ra2md.mix -> localmd.mix` (SHA-256
`e1f0378394313c04ebbd5073f47785ee3e46f1b3c62d65724e8f3c310ee7ba31`).
Grand Cannon TUR/BARL come from `ra2.mix -> local.mix`, each with one
`DUMMY01` VXL limb and one HVA frame. Their VXL sizes are 89,541/21,417 bytes
and HVA files are 88 bytes. Their X bounds are approximately -29..25 versus 21..75,
explaining why the absent barrel removes the long gun silhouette.

The eight stock building `TurretAnim` values observed through the same production
reader are NASAM=`SAM`, GTGCAN=`GTGCANTUR`, NALASR=`LASER`, NAFLAK=`FLAKTUR`,
YAREFN=`SMINTUR`, YAGGUN=`YAGGUN`, CAOUTP=`OUTP`, CAEAST02=`CAHEAD`. The filename
corpus uses those model names plus boundary controls; its availability answers
are deliberately supplied and do not assert that every optional rewritten file
exists in retail archives.
