"""Original EvaluateCandidate VHPScan integer slices; not a full target scan."""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDI,
    UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    SCRATCH, STACK_BASE, STACK_SIZE, NATIVE_FPCW, load_image, run_checked, finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords

ACTOR, TARGET = SCRATCH + 0x1000, SCRATCH + 0x2000
ATYPE, TTYPE, SCORE, VTABLE = (SCRATCH + n for n in (0x3000, 0x4000, 0x5000, 0x6000))
SPANS = ((0x6F7CF7, 0x6F7D19), (0x6F8719, 0x6F875F), (0x6F8928, 0x6F895B),
         (0x6FDBCD, 0x6FDBD3), (0x7C5F00, 0x7C5F3D))


def generate():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    original = [bytes(u.mem_read(a, b-a)) for a, b in SPANS]
    # Copy the Unit vtable; both +84 getters execute original instructions.
    u.mem_write(VTABLE, bytes(u.mem_read(0x7F5C70, 0x100)))
    u.mem_write(ACTOR, dwords(VTABLE))
    u.mem_write(TARGET, dwords(VTABLE))
    u.mem_write(ACTOR + 0x6C4, dwords(ATYPE))
    u.mem_write(TARGET + 0x6C4, dwords(TTYPE))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    rows = []
    pairs = [(-2147483648, -2147483648), (-1, 300), (0, 300),
             (1, -3), (1, 0), (1, 1), (1, 2), (149, 300), (150, 301),
             (151, 301), (32767, 65535), (32768, 65535),
             (1073741823, 2147483647), (1073741824, 2147483647),
             (2147483647, 2147483647)]
    scores = (-2147483648, -1073741825, -3, -1, 0, 1, 3,
              1073741823, 1073741824, 2147483647)
    cases = [(mode, pair, score, None) for mode, pair, score in product(range(3), pairs, scores)]
    doubles = (2147483648.0, -2147483649.0, 4294967299.0, -4294967299.0,
               9223372036854775808.0, -9223372036854775808.0, 1e30,
               float.fromhex('0x1.fffffffffffffp+1023'))
    cases += [(mode, (estimate, 301), None, value)
              for mode, estimate, value in product(range(3), (0, 150), doubles)]
    for mode, (estimate, strength), score, value in cases:
        if value is not None:
            u.mem_write(ATYPE + 0x188, struct.pack('<d', value))
            u.reg_write(UC_X86_REG_EAX, ATYPE)
            u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
            # Execute an original FLD as the declared supplied-ST0 boundary.
            run_checked(u, 0x6FDBCD, 0x6FDBD3, count=2)
            u.mem_write(sp - 4, dwords(0x6F8710))
            u.reg_write(UC_X86_REG_ESP, sp - 4)
            run_checked(u, 0x7C5F00, 0x6F8710, count=60,
                        required_addresses=(0x7C5F00,))
            score = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
        u.mem_write(ATYPE + 0x394, dwords(mode))
        u.mem_write(TARGET + 0x70, dwords(estimate))
        u.mem_write(TTYPE + 0xA0, dwords(strength))
        u.mem_write(SCORE, dwords(score))
        for reg, register_value in ((UC_X86_REG_ESI, TARGET), (UC_X86_REG_EDI, ACTOR),
                                   (UC_X86_REG_EBP, SCORE), (UC_X86_REG_ESP, sp)):
            u.reg_write(reg, register_value)
        stop = run_checked(u, 0x6F7CF7, (0x6F7D19, 0x6F894F), count=40,
                           required_addresses=(0x6F7CF7,))
        rejected = stop == 0x6F894F
        # Entry6F8719 follows MOV ECX,EDI at6F8714 in the original body.
        u.reg_write(UC_X86_REG_ECX, ACTOR)
        run_checked(u, 0x6F8719, 0x6F875F, count=60,
                    required_addresses=(0x6F8719,))
        adjusted = struct.unpack('<i', u.mem_read(SCORE, 4))[0]
        stop = run_checked(u, 0x6F8928, (0x6F8939, 0x6F8948), count=20,
                           required_addresses=(0x6F8928,))
        accepted = None if stop == 0x6F8948 else struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
        assert u.reg_read(UC_X86_REG_ESP) == sp
        assert u.mem_read(TARGET + 0x70, 4) == dwords(estimate)
        row = dict(mode=mode, estimated=estimate, strength=strength, score=score,
                         early_rejected=rejected, adjusted=adjusted,
                         score_accepted=accepted, combined=None if rejected else accepted)
        if value is not None:
            row['float_score_bits'] = struct.pack('>d', value).hex()
        rows.append(row)
    assert all(bytes(u.mem_read(a, b-a)) == old for (a, b), old in zip(SPANS, original))
    return rows


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope='Original EvaluateCandidate Strong gate, Normal transform and final integer acceptance slices',
        assumptions=[
            '450 integer rows:3 modes x15 estimate/Strength pairs x10 scores, including signed and wrapping boundaries',
            '48 appended conversion rows execute original FLD6FDBCD on supplied finite f64 ST0 then original7C5F00 under PC53/chop; i32/i64 overflow and masked conversion-indefinite feed the original integer slices',
            'Supplied object/type/score state; +84 calls execute the original Unit getter through a copied retail vtable',
            'Slices are independently entered: after selection/probe, after ftol, and after omitted later modifiers',
            'Does not execute INI parser, float score construction, earlier eligibility callbacks, later house/zone modifiers, or full scanner',
            'Final acceptance stops before register-pop/return sequence; observes EAX clamp or zero-rejection boundary',
        ], substitutions=[], entry_points={'strong':0x6F7CF7,'normal':0x6F8719,'accept':0x6F8928},
    )
    result['original_slices'] = [{'start':f'{a:08X}','end_exclusive':f'{b:08X}',
        'sha256':hashlib.sha256(bytes(u.mem_read(a,b-a))).hexdigest()} for a,b in SPANS]
    result['original_unit_type_getter'] = f'{struct.unpack("<I",u.mem_read(0x7F5C70+0x84,4))[0]:08X}'
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
