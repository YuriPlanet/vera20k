"""Original70CD10 health term/spill/tail and7087C0 score-comparison slices.

Supplied prior accumulator and distance term; executes actual GetHealthRatio,
retail Unit type getter, FSTP64 and caller ftol. Not whole weapon selection or scan.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance, run_checked, RET_MAGIC
from tools.spatial_oracle.object_health import Fixture, SP, TYPE, dwords, i32

SPANS = ((0x70CF58, 0x70CF6D), (0x70D0A0, 0x70D0D0),
         (0x5F5C60, 0x5F5C80), (0x741490, 0x741497), (0x7C5F00, 0x7C5F3D),
         (0x708A9F, 0x708AAA), (0x6FDBCD, 0x6FDBD3))

def bits(value):
    return struct.pack('>d', value).hex()

def generate():
    f = Fixture()
    original = [bytes(f.u.mem_read(a, b-a)) for a, b in SPANS]
    rows = []
    for hp, strength, coefficient, previous, beyond, distance_coefficient in product(
        (-2147483648, -1, 0, 1, 65536, 2147483647),
        (-2147483648, -1, 0, 1, 100, 65536, 2147483647),
        (-100.0, 0.0, 100.0, 1e308), (0.0, 0.1), (0, 17), (-10.0,)):
        f.reset('unit', hp, strength)
        f.u.mem_write(SP+0x10, struct.pack('<d', previous))
        f.u.mem_write(SP+0x40, struct.pack('<d', coefficient))
        f.u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(f.u, 0x70CF58, 0x70CF6D, count=80,
                    required_addresses=(0x5F5C60, 0x741490, 0x70CF69))
        spill = bytes(f.u.mem_read(SP+0x10, 8))[::-1].hex()
        f.u.mem_write(SP+0x0C, dwords(0))
        f.u.mem_write(SP+0x50, struct.pack('<d', distance_coefficient))
        frame = SP+0x90
        f.u.reg_write(UC_X86_REG_EAX, beyond)
        f.u.reg_write(UC_X86_REG_EBP, frame)
        f.u.mem_write(frame, dwords(0, 0x7C5F00, 0, 0, RET_MAGIC))
        run_checked(f.u, 0x70D0A0, RET_MAGIC, count=100,
                    required_addresses=(0x70D0BC, 0x70D0C4, 0x7C5F00))
        assert f.u.reg_read(UC_X86_REG_ESP) == frame+20
        rows.append(dict(current=hp, strength=strength, coefficient_bits=bits(coefficient),
                         previous_bits=bits(previous), beyond=beyond,
                         distance_coefficient_bits=bits(distance_coefficient),
                         health_spill_bits=spill, score=i32(f.u.reg_read(UC_X86_REG_EAX))))
    comparisons = []
    values = (-1.0, 0.0, 0.5, 100000.0, float('inf'), -float('inf'), float('nan'))
    for current, attacker in product(values, values):
        f.reset('unit', 1, 1)
        f.u.mem_write(TYPE+0x188, struct.pack('<d', attacker))
        f.u.reg_write(UC_X86_REG_EAX, TYPE)
        run_checked(f.u, 0x6FDBCD, 0x6FDBD3, count=2)
        f.u.mem_write(SP+0x10, struct.pack('<d', current))
        stop = run_checked(f.u, 0x708A9F, (0x708AAA, 0x708B17), count=8)
        comparisons.append(dict(current_bits=bits(current), attacker_bits=bits(attacker),
                                refuse=stop == 0x708B17))
    assert [bytes(f.u.mem_read(a, b-a)) for a, b in SPANS] == original
    assert len(rows) == 672 and len(comparisons) == 49
    return dict(health_terms=rows, retaliation_comparisons=comparisons)

def metadata():
    f = Fixture()
    result = provenance(scope='Original70CF58 health ratio/spill,70D0A0 distance tail/ftol,708A9F retaliation comparison',
        assumptions=['672 supplied health/Strength/coefficient/previous/distance combinations; signed HP and Strength include0 and extremes',
            'PC53/chop0E7F; copied Unit vtable retains original741490 type getter; original5F5C60 divides with no zero guard',
            'Entry70CF58 supplies prior accumulator and D; second slice70D0A0 supplies distance beyond range and E, then returns into originalftol',
            '49 comparison rows use originalFLD6FDBCD to supply attacker ST0 and memory current score, execute708A9F..A8 C0 branch; quiet NaN only',
            'Does not execute prior A/B/C weapon-selection terms, coordinate/sqrt distance, eligibility, full scorer or full retaliation/scanner',
            'Stored spill bits and low32 result are original execution observations; no host-side score golden'],
        substitutions=[], entry_points={'health':0x70CF58,'tail':0x70D0A0,'comparison':0x708A9F})
    result['original_slices'] = [{'start':f'{a:08X}', 'end_exclusive':f'{b:08X}',
        'hex':bytes(f.u.mem_read(a,b-a)).hex(), 'sha256':hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()} for a,b in SPANS]
    return result

if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
