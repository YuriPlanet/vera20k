"""Original `BuildingClass::GetFLH`'s voxel-turret (`+0x16C5`) and voxel-barrel
(`+0x16C6`) shot origins.

Retail `gamemd.exe` under Unicorn (identity in the meta sidecar).
`BuildingClass::GetFLH @ 0x00453840` returns, for a type whose
`TurretAnimIsVoxel=` byte (`+0x16C5`) is set and that takes no earlier arm,
`TechnoClass::GetFLH @ 0x006F3AD0`'s coordinate with
`TacticalClass::IsometricPixelToWorld @ 0x006D2070` of the type's signed
`TurretAnimX=`/`TurretAnimY=` (`+0x11E0`/`+0x11E4`) added to X and Y
(`0x00453A12..0x00453A4C`). For a type whose `BarrelAnimIsVoxel=` byte
(`+0x16C6`) is set, the earlier arm (`0x004539A1..0x004539DE`) replaces the
whole coordinate with `GetTurretDrawPosition @ 0x00453BF0`: the type's
`VoxelBarrelOffsetTo*` pivots composed by the matrix chain `0x00458810`, the
burst-selected barrel end through the point transform `0x5AFB80`, the
building coordinate (`+0xAC` = `0x00459EF0`) and the tail's own
`TurretAnimX`/`TurretAnimY` pixel add. The chain ends by scaling its three
basis columns by the type's `+0x1728` double (`0x00458975..0x004589A2`).
Nothing initializes it: no instruction stores it, the constructor
`0x0045DD90` skips it, and `FindOrAllocate @ 0x004653C0` allocates the type
without zeroing, so native reads stale heap content. The rows hold 0.0, a
never-used page's content and VERA's value: it removes the barrel end and
the shot leaves from the pivot translation. Both arms are only reached
with `PrimaryFirePixelOffset=` (`+0xE44`) unset.

Fixture. `building_prism.LaserScene`'s building fixture: the
`building_construction` BuildingClass (vtable 0x7E3EBC, its `+0xAC` =
`0x00459EF0`), its type at `sm.YTYPE`, `TacticalClass` singleton `0x887324`
with the native matrix initializer run, and the formerly substituted GetFLH
executed natively. Per row the type receives:

- `+0x11E0`/`+0x11E4` (`TurretAnimX=`, `TurretAnimY=`) as signed dwords and
  `+0x16C5` (`TurretAnimIsVoxel=`),
- `+0x16C6` (`BarrelAnimIsVoxel=`), the `+0x1730/+0x173C/+0x1748/+0x1754`
  pivot triples, and the `+0x1728` scale double at 0.0 (nothing
  initializes it),
- `+0xE44` (`PrimaryFirePixelOffset=`) and `+0x1764`
  (`PrimaryFireDualOffset=`), the stock Grand Cannon and Gattling Cannon state
  being the constructor's 0xFFFF/0xFFFF and 0,
- `+0x89C`/`+0x8B8`, the primary/secondary FLH the ART reader merges
  (`PrimaryFireFLH=`/`SecondaryFireFLH=`), the fixture weapon's `+0x9C`
  (`Burst=`), and the building's Location.

The stock rows use the values retail authors for the two types the issue
names: `[GTGCAN]` TurretAnimX=3 / TurretAnimY=28 / `ARTMD [GTGCAN]`
PrimaryFireFLH=250,0,230, and `[YAGGUN]` TurretAnimX=0 / TurretAnimY=15 /
`ARTMD [YAGGUN]` Weapon1FLH=150,60,200. Which physical layer supplies them is
the production reader's own evidence; these rows only fix the values the
original instructions read.

`+0x388` is the building's facing slot: the scene constructs it at rate 0 and
`FacingClass::Set_Current @ 0x004C9300` snaps it to the row's heading, which
the FLH turn (`0x006F3B2E..0x006F3C52`) and the barrel chain's `Rz` read.
`+0x370`, the barrel elevation, is constructed and snapped the same way.
`+0x3B8` is the burst index the odd-shot lateral mirror and the barrel-end
sign (`0x00453C1A..0x00453C67`: `CurrentBurstIndex - 1`, or, at zero, the
weapon's `Burst - 1`; barrel 0 adds `+Y`, 1 `-Y`, later ones none) read. The
zero scale multiplies that signed barrel end away, so every burst row of the
barrel arm records the same origin.

Each row executes `BuildingClass::GetFLH` with the original five arguments
(out, weapon index, base x, base y, base z), then the building's own
`GetCoords @ 0x00459EF0` for the coordinate the offset is added to. It records
both coordinates, the reached instruction addresses and that the Scenario RNG
is untouched. The recorded inputs describe the coordinate cases; unrelated
fixture initialization is shared with `LaserScene`.

Coverage: one fixture building, one heading, elevation and burst parity per
row, and the stock/control inputs above. It is not a whole Scenario and not
the map layer selection. The `+0xE44` precedence rows reuse the earlier arm's
already-owned pixel path; the `voxel_disabled` control differs from
`gtgcan_burst0` only in `+0x16C5`, and `barrel_pixel_precedence` shows the
pixel arm returning before an armed `+0x16C6` byte.

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
    """`LaserScene`'s building fixture with the `+0x16C5`/`+0x16C6` type fields.

    `LaserScene` already clears `+0x16C5`/`+0x16C6` and writes `+0xE44`,
    `+0x1764` and the FLH slots from the case; this subclass adds the pixel
    offsets the voxel-turret arm reads, the barrel arm's flag, pivot triples
    and 0.0 scale, the fixture weapon's `Burst`, and the barrel-path
    getters.
    """

    def __init__(self, case):
        super().__init__(dict(case, native_flh=True))
        u = self.u
        u.mem_write(self.kind + 0x16C5, bytes([1 if case['turret_anim_is_voxel'] else 0]))
        u.mem_write(self.kind + 0x11E0, dwords(case['turret_anim_x']))
        u.mem_write(self.kind + 0x11E4, dwords(case['turret_anim_y']))
        u.mem_write(self.kind + 0x16C6, bytes([1 if case['barrel_anim_is_voxel'] else 0]))
        for offset, key in ((0x1730, 'voxel_barrel_pitch_pivot'),
                            (0x173C, 'voxel_barrel_rotate_pivot'),
                            (0x1748, 'voxel_barrel_building_pivot'),
                            (0x1754, 'voxel_barrel_end')):
            u.mem_write(self.kind + offset, dwords(*case[key]))
        # Nothing initializes +0x1728: no instruction stores it (not even the
        # VoxelBarrelFile loader at 0x0045FCAF), the constructor 0x0045DD90
        # skips it, and FindOrAllocate 0x004653C0 does not zero the heap
        # block. Write 0.0, a never-used page's content and VERA's value.
        u.mem_write(self.kind + 0x1728, struct.pack('<d', 0.0))
        weapon = struct.unpack('<I', u.mem_read(self.kind + 0x898, 4))[0]
        assert weapon, 'the fixture primary record carries a WeaponTypeClass'
        u.mem_write(weapon + 0x9C, dwords(case['weapon_burst']))
        for pc in (0x453BF0, 0x458810):
            self.at(pc, lambda pc=pc: self.getters.append(hex(pc)))


def row(name, **fields):
    """One case with every input the fixture writes stated explicitly."""
    case = dict(name=name, location=[3200, 3200, 0], heading=0x2000, burst=0,
                weapon_index=0, base=[0, 0, 0],
                turret_anim_is_voxel=1, turret_anim_x=0, turret_anim_y=0,
                barrel_anim_is_voxel=0,
                voxel_barrel_pitch_pivot=[0, 0, 0],
                voxel_barrel_rotate_pivot=[0, 0, 0],
                voxel_barrel_building_pivot=[0, 0, 0],
                voxel_barrel_end=[0, 0, 0],
                elevation=0x4000, weapon_burst=1,
                pixel_offset=list(UNSET_PIXEL_OFFSET), primary_dual=0,
                primary_flh=[0, 0, 0], secondary_flh=[0, 0, 0])
    case.update(fields)
    return case


# Asymmetric pivots for the barrel chain: every component signed, so both
# turns and all three axis translates move the barrel end.
BARREL_PIVOTS = dict(voxel_barrel_building_pivot=[10, -20, 30],
                     voxel_barrel_rotate_pivot=[-40, 50, -60],
                     voxel_barrel_pitch_pivot=[70, -80, 90],
                     voxel_barrel_end=[100, 110, -120])
BARREL = dict(barrel_anim_is_voxel=1, **BARREL_PIVOTS)


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
        # The `+0x16C6` arm. The barrel index is CurrentBurstIndex - 1, or,
        # at index zero, the weapon's Burst - 1 (`barrel_burst0_wrap`):
        # barrel 0 signs the barrel end +Y, 1 -Y, and a third or later barrel
        # zeroes it. The zero scale removes the barrel end, so the three burst
        # rows record one origin.
        row('barrel_burst0_wrap', burst=0, weapon_burst=2, **BARREL),
        row('barrel_burst1_positive', burst=1, **BARREL),
        row('barrel_burst3_beyond', burst=3, **BARREL),
        row('barrel_heading_0xc000', heading=0xC000, burst=1, **BARREL),
        row('barrel_elevation_tilt', burst=1, elevation=0x3000, **BARREL),
        # A voxel-turret type's pixels and FLH: the barrel arm returns first
        # and its tail already carries the TurretAnimX/Y add.
        row('barrel_voxel_turret_pixels', burst=1, turret_anim_is_voxel=1,
            turret_anim_x=3, turret_anim_y=28, primary_flh=[250, 0, 230], **BARREL),
        # An armed `+0x16C6` byte still loses to the primary-pixel arm.
        row('barrel_pixel_precedence', pixel_offset=[6, -4], **BARREL),
    ]


def voxel_flh(case):
    scene = VoxelFlhScene(case)
    u, actor = scene.u, scene.address[scene.first]
    u.mem_write(actor + 0x3B8, dwords(case['burst']))
    scene.invoke(FACING_RATE_CTOR, actor + 0x388, 0)
    u.mem_write(HEADING, dwords(case['heading']))
    scene.invoke(FACING_SET_CURRENT, actor + 0x388, HEADING)
    # The barrel elevation (`+0x370`), constructed and snapped the same way.
    scene.invoke(FACING_RATE_CTOR, actor + 0x370, 0)
    u.mem_write(HEADING, dwords(case['elevation']))
    scene.invoke(FACING_SET_CURRENT, actor + 0x370, HEADING)
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
            barrel_anim_is_voxel=int(bool(case['barrel_anim_is_voxel'])),
            elevation=case['elevation'],
            weapon_burst=case['weapon_burst'],
            voxel_barrel_pitch_pivot=list(case['voxel_barrel_pitch_pivot']),
            voxel_barrel_rotate_pivot=list(case['voxel_barrel_rotate_pivot']),
            voxel_barrel_building_pivot=list(case['voxel_barrel_building_pivot']),
            voxel_barrel_end=list(case['voxel_barrel_end']),
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
                  '+0x11E0/+0x11E4 for a +0x16C5 type; and its voxel-barrel arm '
                  '(0x004539A1..0x004539DE): GetTurretDrawPosition 0x00453BF0\'s '
                  '0x00458810 matrix chain over the +0x1730/+0x173C/+0x1748/+0x1754 pivots '
                  'and the burst-signed barrel end, which the chain\'s uninitialized '
                  '+0x1728 scale, held at 0.0, removes, with the tail\'s own +0x11E0/+0x11E4 '
                  'pixel add. Both arms\' +0x16C5/+0x16C6/+0xE44/+0x1764/+0x89C/+0x8B8 '
                  'inputs, the building\'s own GetCoords 0x00459EF0, and the FacingClass '
                  '0x4C91E0/0x4C9300 heading the FLH turn and the chain\'s Rz read.',
            entry_points={'building_get_flh': GET_FLH, 'techno_get_flh': 0x6F3AD0,
                          'get_turret_draw_position': 0x453BF0,
                          'barrel_matrix_chain': 0x458810,
                          'isometric_pixel_to_world': 0x6D2070,
                          'building_get_coords': BUILDING_GET_COORDS,
                          'facing_rate_ctor': FACING_RATE_CTOR,
                          'facing_set_current': FACING_SET_CURRENT},
            assumptions=['the building_prism LaserScene fixture building is the firer: the '
                         'building_construction fixture BuildingClass (vtable 0x7E3EBC) whose '
                         'type is the slave_manager type; the TacticalClass singleton at '
                         '0x887324 has its native matrix initializer run',
                         'each row states every input the fixture writes: the type\'s '
                         'TurretAnimIsVoxel/TurretAnimX/TurretAnimY (+0x16C5/+0x11E0/'
                         '+0x11E4), BarrelAnimIsVoxel and the four VoxelBarrelOffsetTo* '
                         'triples (+0x16C6, +0x1730/+0x173C/+0x1748/+0x1754), the +0x1728 '
                         'scale double (0.0), PrimaryFirePixelOffset/PrimaryFireDualOffset '
                         '(+0xE44/+0x1764, the stock constructor state being 0xFFFF/0xFFFF '
                         'and 0), the primary and secondary FLH (+0x89C/+0x8B8), the fixture '
                         'weapon\'s Burst (+0x9C) and the building Location',
                         'nothing initializes +0x1728: no instruction stores it (only this '
                         'chain\'s read at 0x0045897B and ComputeCRC 0x00464B30\'s read at '
                         '0x00464F5E reference it), the constructor 0x0045DD90 skips '
                         '+0x1718..+0x175F, and FindOrAllocate 0x004653C0 allocates the type '
                         'through a non-zeroing HeapAlloc (0x007C94B3), so native reads stale '
                         'heap content; every row holds 0.0, a never-used page\'s content and '
                         'VERA\'s value, so the chain\'s Scale 0x5AEAD0/0x5AEAF0/0x5AEB20 zeroes '
                         'its basis and the point transform answers the pivot translation',
                         'the stock rows use the retail-authored values the comparison rests '
                         'on: GTGCAN 3/28 with PrimaryFireFLH 250,0,230 and YAGGUN 0/15 with '
                         'Weapon1FLH 150,60,200. A later layer could override them; the '
                         'production reader owns that selection',
                         'the facing slots +0x388 (heading) and +0x370 (barrel elevation) '
                         'are constructed at rate 0 and snapped to the row values; +0x3B8 '
                         'is the row burst index',
                         'the weapon index selects the type\'s GetWeapon slot natively '
                         '(0x006F3B23 and 0x00453C02), so the row FLH values and the '
                         'Burst-backed barrel index are read exactly as the ART reader '
                         'merged them',
                         'FPCW 0x0E7F; the fixture is one building, one shot per row'],
            substitutions=['the building fixture\'s unrelated hooks (SelectWeapon, PlayAnim, '
                           'Is_Operational and the other LaserScene boundaries) stay installed '
                           'but are not reached by GetFLH',
                           'no GetFLH, TechnoClass::GetFLH, GetTurretDrawPosition, matrix '
                           'chain, IsometricPixelToWorld or GetCoords output is substituted: '
                           'all execute natively'],
        ),
        argv=argv)


if __name__ == '__main__':
    main()
