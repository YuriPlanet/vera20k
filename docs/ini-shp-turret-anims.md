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
the building's `+0x388` direction's top five bits (`facing >> 3`), so the turret
follows its aim whatever the slot's own timer does. `TurretAnimX`/`Y` shift the
turret from the building's draw origin and `TurretAnimZAdjust` is the slot's
own depth adjust. A type that names no `TurretAnim`, or whose name is absent
from `[Animations]`, has no turret — an ART section for the name is not a type.

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
- `BuildingClass::DrawIt` asks the slot for `facing >> 3`; the animation's own
  frame counter keeps running behind it.
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
  `body_facing_byte >> 3`.

## Residuals

- A mod that declares both ten ART ActiveAnims (filling slot 9) and an SHP
  turret would keep the ActiveAnim slot; the turret row then replaces the ART
  row without a slot of its own, so nothing is drawn.
- The theatre fallback is the shared building-animation one; a turret SHP that
  only exists in another theatre empties the slot.
