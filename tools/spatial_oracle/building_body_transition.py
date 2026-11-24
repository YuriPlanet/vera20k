"""Original whole Building GetCurrentFrame and receiver frame-change transition.

The original bodies and virtual getters execute unchanged. Anim allocation uses
ArtFixture's explicit recorded call boundary; this is not a constructor, renderer,
full damage receiver or runtime animation-state-producer equivalence claim.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.building_art_transition import ArtFixture
from tools.spatial_oracle.object_health import OBJ, TYPE, RULES, SP, dwords

SPANS = ((0x43EF90, 0x43F174), (0x442B37, 0x442BE5),
         (0x5B3040, 0x5B3052), (0x4581F0, 0x4581F7),
         (0x459EE0, 0x459EE7), (0x6F3270, 0x6F3278), (0x5F5C60, 0x5F5C80),
         (0x451890, 0x4518CF))
PAIR_OFFSETS = (0xF10, 0xF1C, 0xF34, 0xF40)


def case(**overrides):
    row = dict(before=50, after=51, strength=100, old_flag=1, pattern='all',
               yellow_bits='3fe0000000000000', red_bits='3fd0000000000000',
               gate=False, laser_fence=False, firestorm_wall=False,
               can_be_occupied=False, state=1, base_frame=0, gate_stages=4,
               laser_frame=7, firestorm_frame=9, occupants=0, tech_level=5,
               mission=5, queued_mission=5, buildup_start=10, buildup_count=6,
               frame_pairs=[[1, 2], [3, 4], [5, 6], [7, 8]])
    row.update(overrides)
    return row


def execute(f, row):
    u = f.u
    f.prepare(row['before'], row['strength'], row['old_flag'], row['pattern'])
    for key, offset in (('gate', 0x16B7), ('laser_fence', 0x16BF),
                        ('firestorm_wall', 0x16C0), ('can_be_occupied', 0x157B)):
        u.mem_write(TYPE+offset, bytes([int(row[key])]))
    for key, offset in (('state', 0x534), ('base_frame', 0xF8),
                        ('laser_frame', 0x618), ('firestorm_frame', 0x61C),
                        ('occupants', 0x694), ('mission', 0xAC), ('queued_mission', 0xB4)):
        u.mem_write(OBJ+offset, dwords(row[key]))
    for key, offset in (('gate_stages', 0x16F8), ('tech_level', 0x634),
                        ('buildup_start', 0xF04), ('buildup_count', 0xF08)):
        u.mem_write(TYPE+offset, dwords(row[key]))
    for offset, pair in zip(PAIR_OFFSETS, row['frame_pairs'], strict=True):
        u.mem_write(TYPE+offset, dwords(*pair))
    u.mem_write(RULES+0x1700, struct.pack('<QQ', int(row['yellow_bits'], 16), int(row['red_bits'], 16)))
    frames = []
    for hp in (row['before'], row['after']):
        u.mem_write(OBJ+0x6C, dwords(hp))
        u.reg_write(UC_X86_REG_ECX, OBJ)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.mem_write(SP, dwords(RET_MAGIC))
        run_checked(u, 0x43EF90, RET_MAGIC, count=1000)
        assert u.reg_read(UC_X86_REG_ESP) == SP+4
        frames.append(u.reg_read(UC_X86_REG_EAX))
    u.reg_write(UC_X86_REG_ESI, OBJ)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.mem_write(SP+0x40, dwords(frames[0]))
    run_checked(u, 0x442B37, 0x442BE5, count=3000)
    assert u.reg_read(UC_X86_REG_ESP) == SP
    f.unchanged()
    return dict(input=row, output=dict(before_frame=frames[0], after_frame=frames[1],
        retained_flag=int(u.mem_read(OBJ+0x6E6, 1)[0]), dirty=int(u.mem_read(OBJ+0x80, 1)[0]),
        replacements=f.replacements, slot_pointers=[f.read(OBJ+0x55C+n*4) for n in range(21)]))


def inputs():
    # Priority matrix, including state-zero bypass and health-frame transitions
    # which collide despite a changed yellow predicate.
    for flags, state, transition in product(range(16), (-1, 0, 1, 2),
            ((50, 51, 1), (51, 50, 0), (25, 26, 1), (26, 25, 0))):
        before, after, old = transition
        yield case(gate=bool(flags&1), laser_fence=bool(flags&2),
                   firestorm_wall=bool(flags&4), can_be_occupied=bool(flags&8),
                   state=state, before=before, after=after, old_flag=old)
    # GateStages signed wrap and 16-bit frame collisions; original EAX is 32 bits.
    for stages, before, after, old in product((-2147483648, -1, 0, 4, 65535, 2147483647),
            (50,), (51,), (0, 1)):
        yield case(gate=True, gate_stages=stages, before=before, after=after, old_flag=old)
    # State-zero animation reflection through Gate, then Mission_Unload19.
    for gate, mission, queued, base in product((False, True), (5, 19, -1), (5, 19), (-1, 0, 7, 2147483647)):
        yield case(state=0, gate=gate, mission=mission, queued_mission=queued, base_frame=base)
    # Full signed frame-offset arithmetic: zero/colliding offsets and wrapped
    # pair sums are intentionally supplied data, not parser admission claims.
    for pairs, base in product(([[0, 0]]*4, [[-1, 0]]*4,
            [[2147483647, 1], [0, 1], [2, 3], [4, 5]],
            [[65535, 1], [0, 0], [0, 0], [0, 0]],
            [[2147483647, 0]]*4), (0, 65535, 2147483647)):
        yield case(state=2, frame_pairs=pairs, base_frame=base)
    # Garrison body priority, critical/yellow tier distinctions and collapse.
    for tech, occupants, gate in product((-1, 0, 5), (-1, 0, 8), (False, True)):
        yield case(can_be_occupied=True, tech_level=tech, occupants=occupants, gate=gate)
        yield case(can_be_occupied=True, tech_level=tech, occupants=occupants, gate=gate,
                   before=25, after=26)
    # Raw signed/zero/wide ratios and independent exceptional thresholds.
    for before, after, strength, yellow, red in (
            (0, 1, 0, '3fe0000000000000', '3fd0000000000000'),
            (-1, 0, 100, '3fe0000000000000', '3fd0000000000000'),
            (-51, -50, -100, '3fe0000000000000', '3fd0000000000000'),
            (16777216, 16777217, 33554432, '3fe0000000000000', '3fd0000000000000'),
            (1, 2, 10, '3fb9999999999999', '3fb9999999999999'),
            (50, 51, 100, '7ff8000000000001', '3fd0000000000000'),
            (25, 26, 100, '3fe0000000000000', '7ff8000000000001'),
            (50, 51, 100, '7ff0000000000000', 'fff0000000000000')):
        for gate, occupied, old in product((False, True), (False, True), (0, 1)):
            yield case(before=before, after=after, strength=strength, yellow_bits=yellow,
                       red_bits=red, gate=gate, can_be_occupied=occupied, old_flag=old)
    for pattern, old in product(('sparse', 'missing'), (0, 1)):
        yield case(pattern=pattern, old_flag=old)


def generate():
    f = ArtFixture()
    original = [bytes(f.u.mem_read(a,b-a)) for a,b in SPANS]
    rows = [execute(f, row) for row in inputs()]
    assert original == [bytes(f.u.mem_read(a,b-a)) for a,b in SPANS]
    return dict(schema_version=1, rows=rows)


def metadata():
    f = ArtFixture()
    result = provenance(scope='Whole Building43EF90 body frame and442B37 receiver second animation transition',
        assumptions=['PC53/chop masked exceptions; original Building vtable copied byte-for-byte and original getters execute',
            'Before/after HP and full frame/type data are supplied; no full ReceiveDamage or body-state-producer claim',
            'All16 Gate/LaserFence/FirestormWall/CanBeOccupied priority combinations and state-1/0/1/2',
            'Statezero includes original mission getter and queued mission fallback, Gate and mission19 reflections',
            'Raw signed32 frame fields and EAX are retained; no u16 narrowing in native comparison',
            'NaN/infinity thresholds and extreme frame inputs do not claim parser or normal gameplay reachability',
            'Receiver slice stops at442BE5 before unrelated later receiver effects'],
        substitutions=['ArtFixture451890 allocation hook records arguments and supplies a pointer; no original code is patched; no full animation allocation/lifecycle claim'],
        entry_points={'GetCurrentFrame':0x43EF90,'receiver_second_transition':0x442B37})
    result['original_slices'] = [dict(start=f'{a:08X}', end_exclusive=f'{b:08X}',
        hex=bytes(f.u.mem_read(a,b-a)).hex(), sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
    result['original_vtable_entries'] = {f'{slot:03X}': f'{f.read(0x7E3EBC+slot):08X}' for slot in (0x84,0x88,0x184,0x408)}
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
