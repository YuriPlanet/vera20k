# SHP building turrets

A `Turret=yes` type draws its turret from `TurretAnim=` in its own rules
section. When `TurretAnimIsVoxel=` is not set, that name is an ordinary SHP
animation with thirty-two facings, drawn on the building's `Special` slot 9.

```ini
[SOMEBASE]                    ; rules: a BuildingType
Turret=yes
TurretAnim=GAGUNTUR           ; [Animations] name, not an ART image
TurretAnimX=0                 ; pixel offsets from the building's draw origin
TurretAnimY=0
TurretAnimZAdjust=-15
TurretAnimIsVoxel=no          ; yes sends the turret down the voxel path instead
```

The thirty-two frames are a heading ring, not a playing animation: the frame is
derived from the building's `+0x388` direction, so the turret follows its aim
whatever the slot's own timer does. `TurretAnimX`/`Y` shift the turret from the
building's draw origin and `TurretAnimZAdjust` is the slot's own depth adjust.
A type that names no `TurretAnim`, or whose name is absent from `[Animations]`,
has no turret — an ART section for the name is not a type.

## Frame order is counter-clockwise, with an NW start

The ring is **not** in DirStruct order, in two independent ways. Measured
frame-by-frame on Mental Omega's `GGGUNTUR.SHP` (`expandmo95.mix`, 32 facing
frames plus 32 shadow frames): frame 0 points screen-up, 8 screen-left, 16
screen-down (barrel hidden behind the base), 24 screen-right — i.e. counter-
clockwise in screen space. On top of that, **frame 0 is screen-up, which the
2:1 isometric projection maps to world NW, not world N**; that is a 45-degree
(4-frame) start offset. The engine already carries the same offset for other
SHP art: `vehicle_facing_slot` documents "NW — not N — at frame 0" and adds 1
to the direction, and `INFANTRY_FACING_SLOT_TABLE` maps step 0 (north) to slot
7 rather than slot 0.

A DirStruct runs clockwise (0 = N, 0x4000 = E, 0x8000 = S, 0xC000 = W), so
frame `k` of the ring depicts world bucket `(28 - k) & 31`. VERA20K therefore
quantizes `+0x388` with the shared 32-way bucket and inverts the ring:
`frame = (28 - bucket) & 31`. Indexing with the bucket directly mirrors the
heading (error grows as twice the heading, so a target due east draws a turret
facing west). Mirroring **without** the NW offset — `(32 - bucket) & 31` —
leaves a constant 45-degree error on every heading, which is much easier to
mistake for "almost fixed".

`TurretAnimX`/`Y`, the muzzle FLH and the fire-error facing check all use the
clockwise DirStruct, so only the frame index needs the ring inverse.

Mental Omega 3.3.6 has twenty-three such buildings: CAART, CACANN, CAEAST02,
CAHMG, CASAM, CASSAM, CATUR, FAAVAL, FACOMP, FACOMPAI, FARAIL, FASONI, GAAAGUN,
GAGUN, GAHYPE, NAFLAK, NAFTUR, NALASR, NAMORT, NASAMT, NASCOM, YAHADE and
YAHADEAI. Its other `Turret=yes` types set `TurretAnimIsVoxel=true` (NASAM,
GTGCAN, YAGGUN, FASWPR among them) and draw a voxel turret instead.

## Native sites

- Grand_Opening `0x00446580..0x004466D5` constructs the slot-9 `AnimClass` from
  the rules `TurretAnim` name at the rules anchor, after the ART ActiveAnims and
  before the light block. Nothing else builds it, so a repeat opening keeps
  what the first one installed.
- `BuildingClass::DrawIt` asks the slot for `facing >> 3` — the raw DirStruct
  bucket, which the authored SHP ring then interprets in screen space; the
  animation's own frame counter keeps running behind it. VERA20K must apply the
  ring inverse above because its `+0x388` is a world-space DirStruct.
- `BuildingClass::Init` gives `+0x388` the type's `TurretRot=` rotation speed,
  and `BuildingClass::Mission_Attack` (`0x0044B16F`, `0x0044B1A8`, `0x0044B1FF`)
  sets its destination toward the target, which is what moves the drawn frame.

## Where VERA20K owns it

- `rules/art_data.rs`: `shp_turret_anim_config` builds the slot-9 row from the
  rules section and `SHP_TURRET_FACING_FRAMES` is the ring size.
- `sim/building_art.rs`: `install_shp_turret_anim` from Grand_Opening;
  `sim/building_art_power.rs`: `ensure_shp_turret_anim` fills the slot for a
  preset building that never opened.
- `rules/effect_asset_catalog.rs`: the tolerant asset roots carry these names
  too. Slot 9's name never appears in an ART `building_anims` row, so without
  that root the SHP binds no frame count and the constructor is refused
  (`animation type [X] has no bound SHP frame count`) — the slot stays empty and
  nothing is drawn.
- `render/sprite_atlas.rs`: the thirty-two frames are registered with the
  building's house colour whenever the type enters the atlas.
- `app/presentation/instances/shp.rs`: `emit_building_anims` drops an ART row
  for slot 9 when the rules own one, and takes the frame from
  `shp_turret_frame(body_facing_current(...))` — the NW-start, counter-
  clockwise ring inverse `(28 - bucket) & 31` described above.

## Residuals

- A mod that declares both ten ART ActiveAnims (filling slot 9) and an SHP
  turret would keep the ActiveAnim slot; the turret row then replaces the ART
  row without a slot of its own, so nothing is drawn.
- The theatre fallback is the shared building-animation one; a turret SHP that
  only exists in another theatre empties the slot.
