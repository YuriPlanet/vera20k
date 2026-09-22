"""Original retained Techno+3D4 writer and ordinary click-action gate.

Executes bounded regions, not the surrounding construction/Unlimbo or complete
DisplayDetermineAction. Foot Unlimbo's success is an explicit region input.
Real Aircraft vtable/GetWeapon instructions choose base/elite Camera flags.
"""
from itertools import product
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP
from tools.native_oracle import (
    SCRATCH, STACK_BASE, STACK_SIZE, load_image, run_checked, finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords

OWNER, TYPE, WEAPON, ELITE = [SCRATCH + n for n in (0, 0x2000, 0x4000, 0x5000)]


def execute(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(OWNER, dwords(0x7E22A4))
    u.mem_write(OWNER + 0x6C4, dwords(TYPE))
    u.mem_write(OWNER + 0x3D4, bytes([case['previous']]))
    u.mem_write(OWNER + 0x150, struct.pack('<f', case['veterancy']))
    u.mem_write(TYPE, dwords(0x7E2868))
    u.mem_write(TYPE + 0x230, bytes([case['selectable']]))
    u.mem_write(TYPE + 0xE0A, bytes([case['landable']]))
    for offset, ptr, weapon in ((0x898, WEAPON, case['weapon']),
                                (0xA94, ELITE, case['elite_weapon'])):
        u.mem_write(TYPE + offset, dwords(ptr if weapon != 'none' else 0))
        u.mem_write(ptr + 0x147, bytes([weapon == 'camera']))
    u.reg_write(UC_X86_REG_ESI, OWNER)
    u.reg_write(UC_X86_REG_EAX, int(case['success']))
    u.reg_write(UC_X86_REG_ESP, sp)
    original = bytes(u.mem_read(0x4143A0, 0x52))
    end = run_checked(u, 0x4143A0, (0x4143F2, 0x41449B), count=5000)
    assert end == (0x4143F2 if case['success'] else 0x41449B)
    assert u.reg_read(UC_X86_REG_ESP) == sp
    assert original == bytes(u.mem_read(0x4143A0, 0x52))
    retained = bool(u.mem_read(OWNER + 0x3D4, 1)[0])
    # Established preceding type/cloak/visibility gates; observe whether this
    # retained-byte gate promotes the supplied NONE action to SELECT(7).
    u.reg_write(UC_X86_REG_EDI, OWNER)
    u.mem_write(sp + 0x10, dwords(0))
    run_checked(u, 0x692762, 0x692778, count=20)
    action = struct.unpack('<i', u.mem_read(sp + 0x10, 4))[0]
    return dict(input=case, mission_only=retained, click_action=action)


def generate():
    rows=[]
    for previous, success, selectable, landable, weapon in product(
            (False, True), (False, True), (False, True), (False, True), ('none', 'gun', 'camera')):
        rows.append(dict(previous=previous, success=success, selectable=selectable,
                         landable=landable, weapon=weapon, elite_weapon='none', veterancy=0))
    for previous, success, veterancy, weapon, elite in product(
            (False, True), (False, True), (1, 2), ('gun', 'camera'), ('none', 'gun', 'camera')):
        rows.append(dict(previous=previous, success=success, selectable=True,
                         landable=True, weapon=weapon, elite_weapon=elite, veterancy=veterancy))
    return [execute(case) for case in rows]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda:provenance(
        entry_points={'unlimbo_flag_suffix':0x4143A0, 'get_weapon':0x70E140,
                      'click_action_flag_gate':0x692762},
        assumptions=['Supplied Aircraft object/type and base/elite weapon slots; original vtable.',
                     'Foot Unlimbo AL result is supplied at the region entry; its body does not execute.',
                     'Click region starts after preceding type/cloak/visibility gates; input action NONE.'],
        substitutions=[],
        scope='96 original retained-flag suffix/click-gate pairs. Covers failed/successful parent result, prior flag history, Selectable/Landable and null/base/elite Camera. No complete Unlimbo, reinforcement, forced selection, cursor/action resolution or flight navigation claim.',
    ))
