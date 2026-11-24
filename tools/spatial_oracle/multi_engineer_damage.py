"""The MultiEngineer Engineer damage, an unpatched native slice of Infantry
PerCell's building entry: 0x00519DB6, the HealthRatio test against
ConditionRed, to the ftol'd damage at 0x00519E82 or the capture branch at
0x00519EAA. The session/MultiEngineer gates before it and the ReceiveDamage
call after it are instruction evidence only.
"""
from pathlib import Path
import hashlib
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EDI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
                               UC_X86_REG_FPSW, UC_X86_REG_FPTAG)
from tools.native_oracle import load_image, run_checked, finish_vectors, provenance, SCRATCH, STACK_BASE, STACK_SIZE
from tools.spatial_oracle.map_queries import dwords

BUILDING, TYPE, RULES = SCRATCH + 0x1000, SCRATCH + 0x4000, SCRATCH + 0x6000
SP = STACK_BASE + STACK_SIZE - 0x1000
BEGIN, DAMAGE, CAPTURE = 0x519DB6, 0x519E82, 0x519EAA
EVIDENCE = ((0x519D82, 0x519DB6), (0x519E82, 0x519EAA))


def execute(health, strength, red):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    original = bytes(u.mem_read(BEGIN, CAPTURE - BEGIN))
    # BuildingClass's vtable; its type getters (vt+0x84, +0x88) read +0x520.
    u.mem_write(BUILDING, dwords(0x7E3EBC))
    u.mem_write(BUILDING + 0x520, dwords(TYPE))
    u.mem_write(BUILDING + 0x6C, dwords(health & 0xFFFFFFFF))
    u.mem_write(TYPE + 0xA0, dwords(strength & 0xFFFFFFFF))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x1708, struct.pack('<d', red))
    u.reg_write(UC_X86_REG_EDI, BUILDING)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    u.reg_write(UC_X86_REG_FPSW, 0)
    u.reg_write(UC_X86_REG_FPTAG, 0xFFFF)
    stop = run_checked(u, BEGIN, (DAMAGE, CAPTURE))
    assert original == bytes(u.mem_read(BEGIN, CAPTURE - BEGIN))
    damage = struct.unpack('<i', struct.pack('<I', u.reg_read(UC_X86_REG_EAX)))[0]
    return dict(health=health, strength=strength,
                condition_red_bits=f"{struct.unpack('<Q', struct.pack('<d', red))[0]:016x}",
                damage=damage if stop == DAMAGE else None)


def generate():
    cases = (
        # Retail ConditionRed=25% on a GAPOWR: full, after one Engineer,
        # short of a full share, just above and at the red line.
        (750, 750, 0.25), (422, 750, 0.25), (400, 750, 0.25), (188, 750, 0.25), (187, 750, 0.25),
        # Inexact thresholds, where PC53/chop products differ from binary64 rounding.
        (1000, 1000, 0.3), (999, 1000, 0.3), (900, 900, 0.33), (7, 7, 0.1), (5, 7, 0.1),
        (300, 300, 0.5), (1, 750, 0.0), (750, 750, 1.0), (100, 0, 0.25),
        (2147483647, 2147483647, 0.25),
    )
    return dict(schema_version=1, rows=[execute(*case) for case in cases])


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)

    def packet(ranges):
        result = {}
        for a, b in ranges:
            raw = bytes(u.mem_read(a, b - a))
            result[f'{a:08X}..{b:08X}'] = dict(hex=raw.hex(), sha256=hashlib.sha256(raw).hexdigest())
        return result

    result = provenance(
        scope='MultiEngineer Engineer damage: HealthRatio above ConditionRed and the ftol damage, 0x519DB6..0x519E82/0x519EAA',
        assumptions=['PC53/chop with masked exceptions; the original HealthRatio 0x5F5C60, BuildingClass vt+0x84/+0x88 and _ftol 0x7C5F00 execute unchanged.',
                     'ConditionRed is a supplied raw binary64 at Rules+0x1708; values other than 25% do not claim retail data.',
                     'The GameMode/MultiEngineer gates and the ReceiveDamage(C4Warhead) call are instruction evidence, not executed.'],
        substitutions=['Only data-shaped Building/type/Rules, stack/register state and ambient FPCW; no original instruction patches or callbacks.'],
        entry_points=dict(multi_engineer_damage=BEGIN))
    result['original_code'] = packet(((BEGIN, CAPTURE),))
    result['instruction_evidence_not_executed'] = packet(EVIDENCE)
    result['native_data'] = {f'{a:08X}': bytes(u.mem_read(a, n)).hex() for a, n in
                             ((0x7E3EBC + 0x84, 4), (0x7E3EBC + 0x88, 4), (0x7E1738, 8), (0x7E1718, 8))}
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
