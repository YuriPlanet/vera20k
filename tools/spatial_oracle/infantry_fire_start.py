"""Original Infantry Fire_At_Target prefix through the start-facing update.

Executes retail 5206B0..52094C, DirectionToTarget, object/building/cell coordinate
getters and FacingClass::UpdateFacing. Weapon selection, GetFireError and
DoAction are explicitly supplied virtual receivers in external fixture memory.
The stop precedes fire-frame dispatch: these vectors do not certify legality,
animation advancement, emission, locomotion or a whole infantry AI visit.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import (
    NATIVE_FPCW, RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)

ENTRY, AFTER_START = 0x5206B0, 0x52094C
OWNER, TARGET = SCRATCH + 0x1000, SCRATCH + 0x2000
VTABLE, TYPE, SEQUENCES = SCRATCH + 0x3000, SCRATCH + 0x4000, SCRATCH + 0x6000
SELECT, LEGALITY, ACTION = SCRATCH + 0x7000, SCRATCH + 0x7100, SCRATCH + 0x7200
TARGET_TYPE = SCRATCH + 0x8000
FRAME = 100


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def execute(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    # Read only the original vtable. Fixture substitutions never patch its bytes.
    assert bytes(u.mem_read(0x7EB058 + 0x48, 4)) == dwords(0x5F65A0)
    u.mem_write(VTABLE + 0x48, dwords(0x5F65A0))
    for slot, address in ((0x2E4, SELECT), (0x3C0, LEGALITY), (0x558, ACTION)):
        u.mem_write(VTABLE + slot, dwords(address))
    u.mem_write(SELECT, b'\xB8' + dwords(case['weapon_slot']) + b'\xC2\x04\x00')
    u.mem_write(LEGALITY, b'\xB8' + dwords(case['fire_error']) + b'\xC2\x0C\x00')
    # Record the requested action; do not emulate native animation advancement.
    u.mem_write(ACTION, b'\x8B\x44\x24\x04\x89\x81\xC4\x06\x00\x00\xC2\x0C\x00')
    for obj, xy in ((OWNER, case['source']), (TARGET, case['target'])):
        u.mem_write(obj, dwords(VTABLE))
        u.mem_write(obj + 0x9C, dwords(*xy, 0))
    if case['target_kind'] == 'building':
        assert bytes(u.mem_read(0x7E3EBC + 0x48, 4)) == dwords(0x447AC0)
        # Original foundation index 4 is 2x3, read by 45EC90/45ECA0.
        assert bytes(u.mem_read(0x8192B8 + 4*4, 4)) == dwords(2)
        assert bytes(u.mem_read(0x819310 + 4*4, 4)) == dwords(3)
        u.mem_write(TARGET, dwords(0x7E3EBC))
        u.mem_write(TARGET + 0x520, dwords(TARGET_TYPE))
        u.mem_write(TARGET_TYPE + 0xEF0, dwords(4))
    elif case['target_kind'] == 'cell':
        assert bytes(u.mem_read(0x7E4EEC + 0x48, 4)) == dwords(0x486840)
        u.mem_write(TARGET, dwords(0x7E4EEC))
        u.mem_write(TARGET + 0x24, struct.pack('<hh', *case['target']))
        # Initialized terrain scalar; fixture cell is flat at ground level zero.
        u.mem_write(0x89E7C0, dwords(104))
    u.mem_write(OWNER + 0x2B4, dwords(TARGET if case['has_target'] else 0))
    u.mem_write(OWNER + 0x6C0, dwords(TYPE))
    u.mem_write(OWNER + 0x6C4, dwords(0x1C if case['deployed'] else 0))
    u.mem_write(OWNER + 0x6DB, bytes((case['prone'],)))
    u.mem_write(OWNER + 0x68D, bytes((case['pending'],)))
    u.mem_write(TYPE + 0xE3C, dwords(SEQUENCES))
    u.mem_write(SEQUENCES + 0x5A4, dwords(1))
    u.mem_write(SEQUENCES + 0x5C8, dwords(1))
    u.mem_write(OWNER + 0x388, dwords(case['initial_facing'], case['initial_previous'],
                                    case['initial_start'], 0, case['initial_duration'],
                                    case['rot'] << 8))
    u.mem_write(0xA8ED84, dwords(FRAME))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ECX, OWNER)
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    visited = []
    observed = {SELECT: 'select_weapon', LEGALITY: 'get_fire_error', ACTION: 'do_action',
                0x5F3DB0: 'direction_to_target', 0x4C9300: 'update_facing',
                0x447AC0: 'building_get_coords', 0x486840: 'cell_get_coords'}
    u.hook_add(UC_HOOK_CODE, lambda _u, address, _size, _data:
               visited.append(observed[address]) if address in observed else None)
    run_checked(u, ENTRY, (AFTER_START, RET_MAGIC), count=20000)
    target, previous, start, _unused, duration, rot = struct.unpack(
        '<6I', u.mem_read(OWNER + 0x388, 24))
    return dict(case, facing=target & 0xFFFF, previous=previous & 0xFFFF,
                start=start, duration=duration, raw_rot=rot,
                pending_after=u.mem_read(OWNER + 0x68D, 1)[0],
                action=struct.unpack('<I', u.mem_read(OWNER + 0x6C4, 4))[0],
                calls=visited)


def generate():
    base = dict(source=[1409, 1517], target=[2177, 1517], initial_facing=0x8123,
                prone=0, deployed=False, pending=0, has_target=True,
                weapon_slot=0, fire_error=0, initial_previous=0x8123,
                initial_start=0xFFFFFFFF, initial_duration=0, rot=5, target_kind='object')
    rows = []
    offsets = [(0, -768), (768, -768), (768, 0), (768, 768), (0, 768),
               (-768, 768), (-768, 0), (-768, -768), (617, -289), (0, 0)]
    modes = [('standing', 0, False, 0), ('prone', 1, False, 0),
             ('deployed', 0, True, 0), ('secondary', 0, False, 1),
             ('secondary_prone', 1, False, 1)]
    for mode, prone, deployed, slot in modes:
        for index, (dx, dy) in enumerate(offsets):
            case = dict(base, name=f'{mode}_{index}', prone=prone, deployed=deployed,
                        weapon_slot=slot, target=[base['source'][0]+dx, base['source'][1]+dy])
            rows.append(execute(case))
    for error in (1, 2, 6, 8):
        rows.append(execute(dict(base, name=f'refused_{error}', fire_error=error)))
    rows.append(execute(dict(base, name='already_pending', pending=1)))
    rows.append(execute(dict(base, name='no_target', has_target=False, pending=1)))
    rows.append(execute(dict(base, name='rotating_heading_already_matches',
                             target=[1409, 749], initial_facing=0x3FFF,
                             initial_previous=0, initial_start=FRAME,
                             initial_duration=1, rot=32)))
    rows.append(execute(dict(base, name='building_foundation_center',
                             source=[1408, 1408], target=[2176, 1408], target_kind='building')))
    rows.append(execute(dict(base, name='ground_cell_center',
                             source=[1447, 1531], target=[8, 5], target_kind='cell')))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'infantry_fire_at_target': ENTRY, 'prefix_end': AFTER_START,
                      'direction_to_target': 0x5F3DB0, 'get_coords': 0x5F65A0,
                      'building_get_coords': 0x447AC0, 'cell_get_coords': 0x486840,
                      'update_facing': 0x4C9300},
        assumptions=['Supplied Infantry type/action state and coordinates; no paradrop or NavCom==TarCom.',
                     'Frame 100; stationary facing plus one active-turn equality case; pending sequence skips the update.',
                     'One 2x3 building target and one flat level-zero cell target exercise their original coordinate getters.'],
        substitutions=['External virtual weapon-slot and fire-error results; DoAction records its argument.'],
        scope='59 prefix cases: five action families, ten XY deltas, four refusals, pending/absent target, active-turn equality and building/cell targets; no emission or full AI.',
    ))
