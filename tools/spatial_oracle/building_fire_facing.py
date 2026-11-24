"""Original building aim directions:

- vt+0x4E8 (0x43ED40): the direction from GetCoords (vt+0x48, 0x447AC0) moved
  by a pixel offset (IsometricPixelToWorld 0x6D2070) to the target's GetCoords.
  The offset is PrimaryFirePixelOffset= (Type+0xE44/+0xE48) unless both words
  are the 0xFFFF sentinel, else TurretAnimX=/TurretAnimY= (+0x11E0/+0x11E4)
  when either is non-zero. Mission_Attack's Set_Desired (0x44B162, 0x44B19B,
  0x44B1F2), its voxel-turret retry (0x44B056) and GetFireError's FACING test
  (0x447FF8) aim through it.
- vt+0x308 (0x44D7D0), FireAt's fire facing (the 8-way muzzle pick at
  0x6FF2E5 and the launch heading at 0x6FE2D2/0x6FE950): with no TarCom
  (+0x2B4), +0x388's current facing rounded to 1/256 turn; for HasTurret
  (vt+0x3FC, 0x4527D0) without TurretAnimIsVoxel= (+0x16C5), rounded to 1/32;
  otherwise vt+0x4E8 at TarCom.
- vt+0x2A8 (0x445E50), the facing TechnoClass::GetFLH's arm without a
  locomotor turns the FLH by: +0x388's current facing for HasTurret or no
  TarCom, otherwise the direction between the two GetCoords, with no offset.

The building's GetCoords (0x447AC0, the foundation centre) is recorded too:
HouseClass::NotifyUnderAttack takes its radar cell from it (0x4F94AE,
0x4F950E, 0x4F956A).

Native: the three functions and everything they call, GetCoords (0x447AC0,
ObjectClass 0x5F65A0 for the unit), HasTurret, FacingClass::Current
(0x4C93D0), IsometricPixelToWorld, atan2 (0x4CAE30) and ftol (0x7C5F00). The
Tactical matrix comes from the TacticalClass constructor's initializer
(0x6D1DC5..0x6D1E1E, as bridge_click_oracle runs it).

Fixture: a BuildingClass (vtable 0x7E3EBC) over zeroed memory with the row's
Location (+0x9C), type (+0x520: foundation index +0xEF0, Turret= +0xCA1,
TurretAnimIsVoxel= +0x16C5, the two pixel pairs), no upgrades (+0x702), TarCom
and a +0x388 FacingClass at rest (duration 0) on the row's facing. The target
is a unit (ObjectClass GetCoords through a vtable holding only +0x48) or a
second building.

Usage: python -m tools.spatial_oracle.building_fire_facing [--check|--write]
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EFLAGS,
                               UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW)
from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
                                 finish_vectors, load_image, provenance, run_checked)

HERE = Path(__file__).resolve().parent
BUILDING_VTABLE, OBJECT_GET_COORDS, GET_COORDS = 0x7E3EBC, 0x5F65A0, 0x447AC0
DIRECTION_TO, FIRE_FACING, TURRET_FACING = 0x43ED40, 0x44D7D0, 0x445E50
MATRIX_INITIALIZER = (0x6D1DC5, 0x6D1E1E)
TACTICAL_POINTER, FRAME = 0x887324, 0xA8ED84
BUILDING, KIND, TARGET, TARGET_KIND, VTABLE, OUT, TACTICAL = (
    SCRATCH + offset for offset in (0x0000, 0x1000, 0x3000, 0x3800, 0x5000, 0x5800, 0x6000))
SP = STACK_BASE + STACK_SIZE - 0x1000
# Foundation= index (Type+0xEF0) by name, from the width/height tables at
# 0x8192B8/0x819310.
FOUNDATIONS = {'1x1': 0, '2x2': 3, '3x3': 6, '4x3': 12, '3x2': 5}


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def fixture():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, TACTICAL)
    u.reg_write(UC_X86_REG_EBX, 0)  # the live constructor's xor ebx,ebx
    u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(u, *MATRIX_INITIALIZER, count=30)
    u.mem_write(TACTICAL_POINTER, dwords(TACTICAL))
    return u


def building(u, address, kind, case):
    u.mem_write(address, bytes(0x800))
    u.mem_write(kind, bytes(0x1800))
    u.mem_write(address, dwords(BUILDING_VTABLE))
    u.mem_write(address + 0x9C, dwords(*case['location'], 0))
    u.mem_write(address + 0x520, dwords(kind))
    u.mem_write(kind + 0xEF0, dwords(FOUNDATIONS[case['foundation']]))
    u.mem_write(kind + 0xCA1, bytes([case.get('turret', False)]))
    u.mem_write(kind + 0x16C5, bytes([case.get('voxel', False)]))
    u.mem_write(kind + 0xE44, dwords(*(case.get('pixel_offset') or (0xFFFF, 0xFFFF))))
    u.mem_write(kind + 0x11E0, dwords(*case.get('turret_anim', (0, 0))))


def call(u, function, this, *args, size=2):
    u.mem_write(SP - 0x400, bytes(0x800))
    sp = SP
    for value in reversed((OUT,) + args):
        sp -= 4
        u.mem_write(sp, dwords(value))
    sp -= 4
    u.mem_write(sp, dwords(RET_MAGIC))
    u.mem_write(OUT, bytes(12))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, this)
    u.reg_write(UC_X86_REG_EAX, 0)
    u.reg_write(UC_X86_REG_EFLAGS, 2)
    u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(u, function, RET_MAGIC, count=20_000)
    if size == 12:
        return list(struct.unpack('<3i', u.mem_read(OUT, 12)))
    return struct.unpack('<H', u.mem_read(OUT, 2))[0]


def run(u, case):
    building(u, BUILDING, KIND, case)
    # +0x388 at rest: desired (+0x0) = start (+0x4) = the row's facing, the
    # timer (+0x8 start -1, +0x10 duration 0) run out and a positive ROT
    # (+0x14); 0x4C93F8 then returns the desired facing.
    facing = case['facing']
    u.mem_write(BUILDING + 0x388, struct.pack('<HxxHxxiiih', facing, facing, -1, 0, 0, 0x0A00))
    u.mem_write(FRAME, dwords(1000))
    target = case.get('target')
    if target is None:
        tarcom = 0
    elif target['kind'] == 'unit':
        u.mem_write(VTABLE, bytes(0x600))
        u.mem_write(VTABLE + 0x48, dwords(OBJECT_GET_COORDS))
        u.mem_write(TARGET, bytes(0x800))
        u.mem_write(TARGET, dwords(VTABLE))
        u.mem_write(TARGET + 0x9C, dwords(*target['location'], 0))
        tarcom = TARGET
    else:
        building(u, TARGET, TARGET_KIND, target)
        tarcom = TARGET
    u.mem_write(BUILDING + 0x2B4, dwords(tarcom))
    out = dict(fire_facing=call(u, FIRE_FACING, BUILDING), turret_facing=call(u, TURRET_FACING, BUILDING),
               get_coords=call(u, GET_COORDS, BUILDING, size=12))
    if tarcom:
        out['direction_to'] = call(u, DIRECTION_TO, BUILDING, tarcom)
    return out


def cases():
    pill = dict(foundation='1x1', location=[5 * 256 + 128, 5 * 256 + 128], pixel_offset=[0, 10])
    unit = lambda x, y: dict(kind='unit', location=[x, y])
    rows = []
    # No TarCom: +0x388 rounded to 1/256 (fire) and as is (FLH), turret or not.
    for value in (0x0000, 0x007F, 0x0080, 0x00FF, 0x3F7F, 0x3F80, 0x7FFF, 0xFF7F, 0xFF80, 0xFFFF):
        rows.append(dict(name=f'idle_{value:04x}', facing=value, **pill))
    rows.append(dict(name='idle_turret', facing=0x1234, turret=True, **pill))
    # A turret (not voxel) with a TarCom: 1/32 turn.
    for value in (0x03FF, 0x0400, 0x0BFF, 0x0C00, 0x7BFF, 0xFBFF, 0xFC00, 0xFFFF):
        rows.append(dict(name=f'turret_{value:04x}', facing=value, turret=True, turret_anim=[0, 10],
                         target=unit(9 * 256 + 40, 2 * 256 + 200), **{k: v for k, v in pill.items()
                                                                    if k != 'pixel_offset'}))
    # A voxel turret (GTGCAN: 2x2, TurretAnimX/Y 3,28) aims its fire facing.
    for name, target in (('voxel_east', unit(20 * 256, 6 * 256)), ('voxel_north', unit(6 * 256, 1 * 256)),
                         ('voxel_southwest', unit(1 * 256 + 17, 14 * 256 + 99))):
        rows.append(dict(name=name, facing=0x4000, turret=True, voxel=True, foundation='2x2',
                         location=[5 * 256 + 128, 5 * 256 + 128], turret_anim=[3, 28], target=target))
    # Turretless: the fire facing and the FLH facing both aim at TarCom.
    targets = {'n': (5 * 256 + 128, 256), 'ne': (9 * 256, 256 + 40), 'e': (12 * 256 + 128, 5 * 256 + 128),
               'se': (11 * 256, 11 * 256 + 7), 's': (5 * 256 + 128, 13 * 256), 'sw': (256 + 5, 9 * 256),
               'w': (128, 5 * 256 + 128), 'nw': (2 * 256, 2 * 256 + 60), 'near': (5 * 256 + 140, 5 * 256 + 100),
               'far': (60 * 256 + 3, 41 * 256 + 250)}
    for key, (x, y) in targets.items():
        rows.append(dict(name=f'pillbox_{key}', facing=0x8000, target=unit(x, y), **pill))
    rows.append(dict(name='pillbox_on_origin', facing=0x8000, target=unit(5 * 256 + 128, 5 * 256 + 128), **pill))
    for name, extra in (('prism', dict(pixel_offset=[0, -4])), ('obelisk', dict(pixel_offset=[11, -26])),
                        ('no_offset', {}), ('turret_anim_only', dict(turret_anim=[-8, 16])),
                        ('both_offsets', dict(pixel_offset=[0, -80], turret_anim=[3, 28])),
                        ('sentinel_x_only', dict(pixel_offset=[0xFFFF, 7], turret_anim=[3, 28]))):
        for foundation in ('1x1', '2x2', '4x3'):
            rows.append(dict(name=f'{name}_{foundation}', facing=0x2000, foundation=foundation,
                             location=[20 * 256 + 128, 20 * 256 + 128],
                             target=unit(26 * 256 + 77, 15 * 256 + 3), **extra))
    rows.append(dict(name='building_target', facing=0x2000, foundation='3x3',
                     location=[20 * 256 + 128, 20 * 256 + 128],
                     target=dict(kind='building', foundation='2x2', location=[30 * 256 + 128, 12 * 256 + 128])))
    return rows


def generate():
    u = fixture()
    return [dict(input=case, output=run(u, case)) for case in cases()]


def main(argv=None):
    finish_vectors(
        generate, HERE / 'building_fire_facing.json',
        provenance=lambda: provenance(
            scope='BuildingClass vt+0x4E8 (0x43ED40), vt+0x308 (0x44D7D0) and vt+0x2A8 (0x445E50) '
                  'over a BuildingClass with the row\'s Location, type flags, pixel offsets, TarCom and '
                  'a +0x388 facing at rest',
            entry_points={'direction_to': DIRECTION_TO, 'fire_facing': FIRE_FACING,
                          'turret_facing': TURRET_FACING, 'get_coords': GET_COORDS,
                          'matrix_initializer': MATRIX_INITIALIZER[0]},
            assumptions=['BuildingClass vtable 0x7E3EBC over zeroed memory; no upgrades (+0x702 = 0)',
                         'the unit target is ObjectClass GetCoords 0x5F65A0 behind a vtable holding only '
                         '+0x48; Locations at Z 0',
                         'the Tactical matrix from the TacticalClass constructor initializer; FPCW 0x0E7F',
                         '+0x388 at rest: desired = start = the facing, duration 0, ROT 0x0A00'],
            substitutions=[]),
        argv=argv)


if __name__ == '__main__':
    main()
