"""Original `BuildingClass::GetFLH`'s voxel-turret shot origin (`+0x16C5`).

Retail `gamemd.exe` under Unicorn (identity in the meta sidecar).
`BuildingClass::GetFLH @ 0x00453840` returns, for a type whose
`TurretAnimIsVoxel=` byte (`+0x16C5`) is set and that takes no earlier arm,
`TechnoClass::GetFLH @ 0x006F3AD0`'s coordinate with
`TacticalClass::IsometricPixelToWorld @ 0x006D2070` of the type's signed
`TurretAnimX=`/`TurretAnimY=` (`+0x11E0`/`+0x11E4`) added to X and Y
(`0x00453A12..0x00453A4C`). The arm is only reached with
`PrimaryFirePixelOffset=` (`+0xE44`) unset and the earlier `+0x16C6`
(`GetTurretDrawPosition`) byte clear.

Fixture. `building_prism.LaserScene`'s building fixture: the
`building_construction` BuildingClass (vtable 0x7E3EBC, its `+0xAC` =
`0x00459EF0`), its type at `sm.YTYPE`, `TacticalClass` singleton `0x887324`
with the native matrix initializer run, and the formerly substituted GetFLH
executed natively. Per row the type receives:

- `+0x11E0`/`+0x11E4` (`TurretAnimX=`, `TurretAnimY=`) as signed dwords and
  `+0x16C5` (`TurretAnimIsVoxel=`); `+0x16C6` stays 0 (the scene clears both),
- `+0xE44` (`PrimaryFirePixelOffset=`) and `+0x1764`
  (`PrimaryFireDualOffset=`), the stock Grand Cannon and Gattling Cannon state
  being the constructor's 0xFFFF/0xFFFF and 0,
- `+0x89C`/`+0x8B8`, the primary/secondary FLH the ART reader merges
  (`PrimaryFireFLH=`/`SecondaryFireFLH=`), and the building's Location.

The stock rows use the values retail authors for the two types the issue
names: `[GTGCAN]` TurretAnimX=3 / TurretAnimY=28 / `ARTMD [GTGCAN]`
PrimaryFireFLH=250,0,230, and `[YAGGUN]` TurretAnimX=0 / TurretAnimY=15 /
`ARTMD [YAGGUN]` Weapon1FLH=150,60,200. Which physical layer supplies them is
the production reader's own evidence; these rows only fix the values the
original instructions read.

`+0x388` is the building's facing slot: the scene constructs it at rate 0 and
`FacingClass::Set_Current @ 0x004C9300` snaps it to the row's heading, which
is what `0x006F3B2E..0x006F3C52` turns the FLH by. `+0x3B8` is the burst index
the odd-shot lateral mirror reads.

Each row executes `BuildingClass::GetFLH` with the original five arguments
(out, weapon index, base x, base y, base z), then the building's own
`GetCoords @ 0x00459EF0` for the coordinate the offset is added to. It records
both coordinates, the reached instruction addresses and that the Scenario RNG
is untouched. The recorded inputs describe the coordinate cases; unrelated
fixture initialization is shared with `LaserScene`.

Coverage: one fixture building, one heading and burst parity per row, and the
stock/control inputs above. It is not a whole Scenario, not the map layer
selection, and not a claim about the unported `+0x16C6` arm. The `+0xE44`
precedence rows reuse the earlier arm's already-owned pixel path; the
`voxel_disabled` control differs from `gtgcan_burst0` only in `+0x16C5`.

Usage: python -m tools.spatial_oracle.building_voxel_flh [--check|--write]
"""

import struct
from pathlib import Path

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle import building_prism as prism
from tools.spatial_oracle.map_queries import dwords

# `BuildingClass::GetFLH`; the building's `+0xAC` `GetCoords` (vtable 0x7E3EBC
# at 0x7E3F68); the facing slot's rate constructor and `Set_Current`.
GET_FLH, BUILDING_GET_COORDS = prism.GET_FLH, 0x459EF0
FACING_RATE_CTOR, FACING_SET_CURRENT = 0x4C91E0, 0x4C9300
# The fixture's Location buffer, the row's object coordinate and the heading
# scratch all live in the scene's mapped PRISM region.
COORD = prism.COORD
OBJECT_COORDINATE, HEADING = COORD + 16, COORD + 64
UNSET_PIXEL_OFFSET = [0xFFFF, 0xFFFF]
# The two stock types the issue names, with the values retail authors.
GTGCAN = dict(turret_anim_x=3, turret_anim_y=28, primary_flh=[250, 0, 230])
YAGGUN = dict(turret_anim_y=15, primary_flh=[150, 60, 200])


class VoxelFlhScene(prism.LaserScene):
    """`LaserScene`'s building fixture with the `+0x16C5` arm's type fields.

    `LaserScene` already clears `+0x16C5`/`+0x16C6` and writes `+0xE44`,
    `+0x1764` and the FLH slots from the case; this subclass adds the two pixel
    offsets the voxel arm reads.
    """

    def __init__(self, case):
        super().__init__(dict(case, native_flh=True))
        self.u.mem_write(self.kind + 0x16C5, bytes([1 if case['turret_anim_is_voxel'] else 0]))
        self.u.mem_write(self.kind + 0x11E0, dwords(case['turret_anim_x']))
        self.u.mem_write(self.kind + 0x11E4, dwords(case['turret_anim_y']))
        assert self.u.mem_read(self.kind + 0x16C6, 1) == b'\0'


def row(name, **fields):
    """One case with every input the fixture writes stated explicitly."""
    case = dict(name=name, location=[3200, 3200, 0], heading=0x2000, burst=0,
                weapon_index=0, base=[0, 0, 0],
                turret_anim_is_voxel=1, turret_anim_x=0, turret_anim_y=0,
                pixel_offset=list(UNSET_PIXEL_OFFSET), primary_dual=0,
                primary_flh=[0, 0, 0], secondary_flh=[0, 0, 0])
    case.update(fields)
    return case


def case_rows():
    return [
        # The stock Grand Cannon arm, both burst parities and three headings:
        # the conversion is added in world axes after GetFLH's own rotation.
        row('gtgcan_burst0', **GTGCAN),
        row('gtgcan_burst1', burst=1, **GTGCAN),
        row('gtgcan_heading_0x4000', heading=0x4000, **GTGCAN),
        row('gtgcan_heading_0xc000', heading=0xC000, **GTGCAN),
        # The stock Gattling Cannon: pixels (0,15) add (128,128) leptons.
        row('yaggun_burst0', **YAGGUN),
        row('yaggun_burst1', burst=1, **YAGGUN),
        row('yaggun_heading_0x4000', heading=0x4000, **YAGGUN),
        # `+0x16C5` clear with the same type: the base GetFLH arm.
        row('voxel_disabled', turret_anim_is_voxel=0, **GTGCAN),
        # The arm's ±0 conversion and its signed input.
        row('zero_pixels', turret_anim_is_voxel=1, primary_flh=[250, 0, 230]),
        row('negative_pixels', turret_anim_x=-25, turret_anim_y=15,
            primary_flh=[250, 0, 230]),
        # `+0xE44` precedence: the primary-pixel arm returns before this one,
        # with the dual flag adding the base GetFLH instead.
        row('primary_pixel_precedence', pixel_offset=[6, -4], primary_dual=0, **GTGCAN),
        row('primary_pixel_dual_precedence', pixel_offset=[6, -4], primary_dual=1, **GTGCAN),
        # A non-primary weapon slot keeps its own (zero) FLH.
        row('gtgcan_weapon1', weapon_index=1, **GTGCAN),
    ]


def voxel_flh(case):
    scene = VoxelFlhScene(case)
    u, actor = scene.u, scene.address[scene.first]
    u.mem_write(actor + 0x3B8, dwords(case['burst']))
    scene.invoke(FACING_RATE_CTOR, actor + 0x388, 0)
    u.mem_write(HEADING, dwords(case['heading']))
    scene.invoke(FACING_SET_CURRENT, actor + 0x388, HEADING)
    scene.getters.clear()
    before = scene.rng()
    scene.invoke(GET_FLH, actor, COORD, case['weapon_index'], *case['base'])
    source = list(struct.unpack('<3i', u.mem_read(COORD, 12)))
    assert before == scene.rng() and not scene.rng_entries
    scene.invoke(BUILDING_GET_COORDS, actor, OBJECT_COORDINATE)
    return dict(
        # Coordinate-case inputs, in the representation the native arm reads.
        input=dict(
            name=case['name'],
            location=list(case['location']),
            heading=case['heading'],
            burst=case['burst'],
            weapon_index=case['weapon_index'],
            base=list(case['base']),
            turret_anim_is_voxel=int(bool(case['turret_anim_is_voxel'])),
            turret_anim_x=case['turret_anim_x'],
            turret_anim_y=case['turret_anim_y'],
            barrel_anim_is_voxel=0,
            primary_pixel_offset=list(case['pixel_offset']),
            primary_dual=int(bool(case['primary_dual'])),
            primary_flh=list(case['primary_flh']),
            secondary_flh=list(case['secondary_flh']),
        ),
        object_coordinate=list(struct.unpack('<3i', u.mem_read(OBJECT_COORDINATE, 12))),
        source=source,
        getters=sorted(set(scene.getters)),
        rng_unchanged=True,
    )


def generate():
    return {'source': 'unicorn/gamemd.exe',
            'voxel_flh': [voxel_flh(case) for case in case_rows()]}


def main(argv=None):
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='BuildingClass::GetFLH 0x00453840\'s voxel-turret arm '
                  '(0x00453A12..0x00453A4C): TechnoClass::GetFLH 0x006F3AD0 plus '
                  'TacticalClass::IsometricPixelToWorld 0x006D2070 of the BuildingType\'s '
                  '+0x11E0/+0x11E4 for a +0x16C5 type, its +0x11E0/+0x11E4/+0x16C5/'
                  '+0xE44/+0x1764/+0x89C/+0x8B8 inputs, the +0x16C6-gated alternative arm\'s '
                  'unreached state, the building\'s own GetCoords 0x00459EF0, and the '
                  'FacingClass 0x4C91E0/0x4C9300 heading the FLH turns by.',
            entry_points={'building_get_flh': GET_FLH, 'techno_get_flh': 0x6F3AD0,
                          'isometric_pixel_to_world': 0x6D2070,
                          'building_get_coords': BUILDING_GET_COORDS,
                          'facing_rate_ctor': FACING_RATE_CTOR,
                          'facing_set_current': FACING_SET_CURRENT},
            assumptions=['the building_prism LaserScene fixture building is the firer: the '
                         'building_construction fixture BuildingClass (vtable 0x7E3EBC) whose '
                         'type is the slave_manager type; the TacticalClass singleton at '
                         '0x887324 has its native matrix initializer run',
                         'each row states every input the fixture writes: the type\'s '
                         'TurretAnimIsVoxel/TurretAnimX/TurretAnimY (+0x16C5/+0x11E0/+0x11E4), '
                         'PrimaryFirePixelOffset/PrimaryFireDualOffset (+0xE44/+0x1764, the '
                         'stock constructor state being 0xFFFF/0xFFFF and 0), the primary and '
                         'secondary FLH (+0x89C/+0x8B8) and the building Location',
                         'the stock rows use the retail-authored values the comparison rests '
                         'on: GTGCAN 3/28 with PrimaryFireFLH 250,0,230 and YAGGUN 0/15 with '
                         'Weapon1FLH 150,60,200. A later layer could override them; the '
                         'production reader owns that selection',
                         'the facing slot +0x388 is constructed at rate 0 and snapped to the '
                         'row heading; +0x3B8 is the row burst index',
                         'the weapon index selects the type\'s GetWeapon slot natively '
                         '(0x006F3B23), so the row FLH values are read exactly as the ART '
                         'reader merged them',
                         'FPCW 0x0E7F; the fixture is one building, one shot per row'],
            substitutions=['the building fixture\'s unrelated hooks (SelectWeapon, PlayAnim, '
                           'Is_Operational and the other LaserScene boundaries) stay installed '
                           'but are not reached by GetFLH',
                           'no GetFLH, TechnoClass::GetFLH, IsometricPixelToWorld or GetCoords '
                           'output is substituted: all four execute natively'],
        ),
        argv=argv)


if __name__ == '__main__':
    main()
