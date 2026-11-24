"""Original EvaluateCandidate bridge-layer gate and its complete Map getters.

Research-ahead fixture: enters6F8682 after earlier eligibility, stops at threat
scoring6F86FE or rejection6F894F. Object XYZ/OnBridge, sparse Map storage and
retained fallback flags are supplied. No full acquisition/lifecycle claim.
"""
import hashlib
import struct
from itertools import product
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, STACK_BASE, STACK_SIZE, load_image, run_checked, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed, TABLE, DUMMY, EMPTY_TABLE

BEGIN, ACCEPT, REJECT = 0x6F8682, 0x6F86FE, 0x6F894F
ACTOR, TARGET, CELL_A, CELL_B = (SCRATCH + p for p in (0x1000, 0x2000, 0x3000, 0x3200))
SP = STACK_BASE + STACK_SIZE - 0x1000
MAP = 0x87F7E8


def execute(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    old = bytes(u.mem_read(BEGIN, ACCEPT - BEGIN))
    u.mem_write(TABLE, EMPTY_TABLE)
    u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    for ptr, xy, flags in zip((CELL_A, CELL_B), case['mapped_cells'], case['flags']):
        u.mem_write(TABLE + (xy[1] * 512 + xy[0]) * 4, dwords(ptr))
        u.mem_write(ptr + 0x24, packed(*xy))
        u.mem_write(ptr + 0x140, dwords(flags))
    u.mem_write(DUMMY + 0x24, packed(1234, -2345))
    u.mem_write(DUMMY + 0x140, dwords(case.get('dummy_flags', 0)))
    for ptr, xyz, on_bridge in zip((ACTOR, TARGET), case['coords'], case['on_bridge']):
        u.mem_write(ptr + 0x9C, dwords(*xyz))
        u.mem_write(ptr + 0x8C, bytes((on_bridge,)))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EDI, ACTOR)
    u.reg_write(UC_X86_REG_ESI, TARGET)
    # Original6F8555 installs EBX=100; the intervening admitted path retains it.
    u.reg_write(UC_X86_REG_EBX, 0x100)
    lookups, writes, pointers = [], [], []

    def read32(p):
        return int.from_bytes(u.mem_read(p, 4), 'little')

    def observe(uc, pc, size, data):
        if pc in (0x65C780, 0x65C7E0, 0x68BCB0):
            raise AssertionError(('unexpected RNG/identity call', hex(pc)))
        if pc == 0x565730:
            arg = read32(u.reg_read(UC_X86_REG_ESP) + 4)
            lookups.append(dict(at=f'{pc:08x}', xyz=list(struct.unpack('<iii', u.mem_read(arg, 12)))))
        elif pc in (0x6F86AF, 0x6F86DA):
            ptr = u.reg_read(UC_X86_REG_EAX)
            pointers.append(ptr)
            lookups[-1].update(return_identity={CELL_A:'cell_a', CELL_B:'cell_b', DUMMY:'dummy'}[ptr],
                               flags=read32(ptr + 0x140),
                               dummy_coord=list(struct.unpack('<hh', u.mem_read(DUMMY + 0x24, 4))))

    def written(uc, access, address, size, value, data):
        if STACK_BASE <= address < STACK_BASE + STACK_SIZE:
            return
        assert address == DUMMY + 0x24 and size == 4, (hex(address), size, value)
        writes.append(dict(address=f'{address:08x}', size=size, value=value))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, written)
    stop = run_checked(u, BEGIN, (ACCEPT, REJECT), count=300,
                       required_addresses=(0x565730, 0x6F86DA))
    assert len(lookups) == 2 and u.reg_read(UC_X86_REG_ESP) == SP
    assert bytes(u.mem_read(BEGIN, ACCEPT-BEGIN)) == old
    return dict(input=case, rejected=stop == REJECT, stop=f'{stop:08x}',
                lookups=lookups, same_cell_identity=pointers[0] == pointers[1],
                writes=writes, dummy_coord=list(struct.unpack('<hh', u.mem_read(DUMMY + 0x24, 4))),
                rng_calls=0, native_id_calls=0)


def controls():
    base=dict(mapped_cells=[[10,10],[11,10]], coords=[[2688,2688,0],[2944,2688,416]])
    for fa, fb, oa, ob in product((0,0x100,0x200,0x400,0x500,0xFFFFFFFF),
                                  (0,0x100,0x200,0x400,0x500,0xFFFFFFFF), (0,1), (0,1)):
        yield dict(base, name=f'flags_{fa:x}_{fb:x}_layers_{oa}_{ob}', flags=[fa,fb], on_bridge=[oa,ob])
    for oa, ob in ((1,2),(2,2),(255,1),(255,255)):
        yield dict(base, name=f'raw_bytes_{oa}_{ob}', flags=[0x100,0x100], on_bridge=[oa,ob])
    for name, coords, cells, dummy in (
        ('same_cell', [[2688,2688,0],[2688,2688,9999]], [[10,10],[11,10]], 0),
        ('negative_fraction_truncates_zero', [[-1,-255,0],[256,0,-100]], [[0,0],[1,0]], 0),
        ('fixed_stride_alias', [[-256,256,0],[131072,0,0]], [[511,0],[0,1]], 0),
        ('first_missing', [[-256,0,0],[2944,2688,416]], [[10,10],[11,10]], 0x100),
        ('second_missing_no_structure', [[2688,2688,0],[4096,4096,416]], [[10,10],[11,10]], 0),
        ('second_missing_structural', [[2688,2688,0],[4096,4096,416]], [[10,10],[11,10]], 0x100),
        ('both_missing_shared_dummy', [[-256,0,0],[4096,4096,416]], [[10,10],[11,10]], 0x100),
        ('signed_extreme_wrapping_index', [[2147483647,2147483647,0],[-2147483648,-2147483648,0]], [[10,10],[11,10]], 0x100),
    ):
        yield dict(name=name, mapped_cells=cells, coords=coords, flags=[0x100,0x100],
                   on_bridge=[0,1], dummy_flags=dummy)

    for layer in (0,1):
        yield dict(base, name=f'both_missing_same_layer_{layer}',
                   coords=[[-256,0,0],[4096,4096,416]], flags=[0x100,0x100],
                   on_bridge=[layer,layer], dummy_flags=0x100)
    yield dict(base, name='first_real_nonstructural_second_missing',
               coords=[[2688,2688,0],[4096,4096,416]], flags=[0,0x100],
               on_bridge=[0,1], dummy_flags=0x100)


def generate():
    return dict(rows=[execute(c) for c in controls()])


def metadata():
    u=Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result=provenance(scope=__doc__, entry_points={'gate':BEGIN,'accept':ACCEPT,'reject':REJECT,
        'coordinate_lookup':0x565730}, assumptions=[
        '159 controls:144 raw flag/layer combinations,4 noncanonical OnBridge byte comparisons and11 coordinate/fallback boundaries, including same-layer and first-nonstructural no-short-circuit witnesses.',
        'Original6F8555 EBX=100 is supplied at the declared gate boundary. Earlier EvaluateCandidate admission and later score/publication are not executed.',
        'Both original565730 calls execute with sparse fixed512-stride map storage. Missing slots use the actual shared dummy and stamp its signed-short coordinate; dummy raw flags are supplied retained state.',
        'Read-only hooks observe outputs. All non-stack writes must be the shared dummy coordinate. RNG/identity calls are rejected. No virtual or direct callee is substituted in this slice.',
        'No native map/object constructors, retail source loading, full scanner, Unit scheduling or target lifecycle is claimed.',
    ], substitutions=[])
    result['original_slices']=[dict(start=f'{a:08x}', end_exclusive=f'{b:08x}',
        sha256=hashlib.sha256(bytes(u.mem_read(a,b-a))).hexdigest())
        for a,b in ((BEGIN,ACCEPT),(0x565730,0x565798))]
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
