"""Original Fly numeric step with live Primary.Current and real speed getter.

Executes4CDA3C..4CDB4C after Mark(REMOVE), stopping before candidate validation
and placement. Supplies a live facing history and a Q16-representable current
speed fraction; no calls or instructions are substituted.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.crate_speed_effect import fixture
from tools.spatial_oracle.map_queries import dwords

LOCO, ARG, OUTPUT = SCRATCH + 0x1B000, SCRATCH + 0x1D000, SCRATCH + 0x1D100


def execute(case):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind='aircraft')]))
    owner, object_type = actors[0], actors[0] + 0x800

    def call(entry, receiver, *args):
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ECX, receiver)
        u.reg_write(UC_X86_REG_ESP, sp)
        run_checked(u, entry, RET_MAGIC, count=2000)
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + len(args) * 4
        return u.reg_read(UC_X86_REG_EAX)

    call(0x4CC9A0, LOCO)
    u.mem_write(LOCO + 0xC, dwords(owner))
    u.mem_write(LOCO + 0x48, struct.pack('<d', case['fraction_bits'] / 65536))
    # Original ReadINI conversion block, after its integer reader returned.
    # -1 means retain constructor zero; all other signed values are clamped.
    u.mem_write(object_type + 0x678, dwords(0))
    from unicorn.x86_const import UC_X86_REG_EBP
    u.reg_write(UC_X86_REG_EBP, object_type)
    u.reg_write(UC_X86_REG_EAX, case['ini_speed'] & 0xFFFFFFFF)
    run_checked(u, 0x71465F, 0x71469F, count=100)
    type_speed = struct.unpack('<i', u.mem_read(object_type + 0x678, 4))[0]
    u.mem_write(owner + 0x9C, dwords(*case['current']))
    u.mem_write(0xA8ED84, dwords(90))
    call(0x4C91C0, owner + 0x388)
    call(0x4C9680, owner + 0x388, case.get('rot', 5))
    u.mem_write(ARG, dwords(case['facing']))
    call(0x4C9300, owner + 0x388, ARG)
    if 'destination' in case:
        u.mem_write(ARG, dwords(case['destination']))
        call(0x4C9220, owner + 0x388, ARG)
    u.mem_write(0xA8ED84, dwords(case.get('frame', 100)))
    before = bytes(u.mem_read(owner + 0x388, 24))
    calls = []
    speeds = []
    observed = {0x4C93D0: 'primary_current', 0x4CFE20: 'fly_speed',
                0x4CACB0: 'sin_table', 0x4CAD00: 'cos_table',
                0x568300: 'candidate_in_map'}
    def observe(_u, address, _size, _data):
        if address in observed:
            calls.append(observed[address])
        if address == 0x4CDA78:
            speeds.append(struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0])
    u.hook_add(UC_HOOK_CODE, observe)
    u.reg_write(UC_X86_REG_ESI, LOCO)
    u.reg_write(UC_X86_REG_EDI, LOCO + 4)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x4CDA3C, 0x4CDB4C, count=5000,
                required_addresses=[0x4C93D0, 0x4CFE20, 0x4CDA78])
    assert u.reg_read(UC_X86_REG_ESP) == sp
    assert bytes(u.mem_read(owner + 0x9C, 12)) == dwords(*case['current'])
    assert bytes(u.mem_read(owner + 0x388, 24)) == before
    proposed = list(struct.unpack('<iii', u.mem_read(sp + 0x50, 12)))
    # This stack slot is reused by the trig call frame; observe the getter
    # return at4CDA78 instead of interpreting its later contents as speed.
    assert len(speeds) == 1
    speed = speeds[0]
    step_calls = calls.copy()
    call(0x4C93D0, owner + 0x388, OUTPUT)
    facing = struct.unpack('<H', u.mem_read(OUTPUT, 2))[0]
    return dict(input=case, type_speed=type_speed, speed=speed, facing=facing,
                proposed=proposed, calls=step_calls)


def generate():
    base = dict(current=[2688, 2688, 1500], ini_speed=14, fraction_bits=65536)
    cases = [dict(base, name=f'heading_{facing}', facing=facing)
             for facing in (0, 1, 255, 256, 8191, 8192, 16383, 16384,
                            16385, 32767, 32768, 49152, 65535, 0x1234, 0xABCD)]
    cases += [dict(base, name=f'fraction_{speed}_{bits}', facing=0xABCD,
                   ini_speed=speed, fraction_bits=bits)
              for speed in (-100, -1, 0, 1, 4, 6, 8, 10, 11, 14, 40, 100, 101, 2147483647)
              for bits in (-65536, 0, 1, 3276, 6553, 6554, 16384, 32768, 65535, 65536, 98304)]
    cases += [dict(base, name=f'boundary_{x}_{y}', current=[x, y, 416], facing=facing)
              for x, y, facing in ((2559, 2559, 8192), (2560, 2560, 40960),
                                    (10, 10, 49152), (131070, 131070, 16384),
                                    (-3, -4, 65535), (100000, 120000, 0xFEFF))]
    cases += [dict(base, name=f'turn_{rot}_{frame}', facing=0x4000,
                   destination=0xC000, rot=rot, frame=frame)
              for rot in (-255, -1, 0, 5, 127, 128) for frame in (90, 91, 100, 116)]
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'paid_step': 0x4CDA3C, 'candidate_ready': 0x4CDB4C,
                      'speed': 0x4CFE20, 'type_speed_conversion': 0x71465F,
                      'primary_current': 0x4C93D0, 'sin': 0x4CACB0, 'cos': 0x4CAD00},
        assumptions=['Real Aircraft and Fly vtables; original Fly and Facing constructors and setters execute.',
                     'Original ReadINI numeric Speed block receives the supplied integer; -1 retains supplied constructor zero. INI text parsing and inheritance excluded.',
                     'Current fraction is supplied as an exact Q16 value, matching the existing SimFixed policy; unrestricted native binary64 fractions and ramping are excluded.',
                     'Entry follows Mark(REMOVE), with original stack/register frame supplied. The first map predicate runs against the supplied zero map geometry and its result is ignored by this range.',
                     'Owner coordinates/facing history must remain unchanged; output is the provisional XYZ before candidate validation and SetCoords.'],
        substitutions=[],
        scope='199 original paid Fly steps: full heading words, active facing histories, signed/clamped type speed, Q16 fractions and cell/world boundaries. Includes actual Current and Fly speed getters. Excludes IsMoving admission, map-edge correction, placement, height, descent drift, slowdown, navigation and landing.',
    ))
