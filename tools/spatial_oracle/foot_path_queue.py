"""Original Foot-owned replay shifts and preservation at Stop/Drive END.

Interior fresh/chain/tube blocks use supplied admitted frames. Complete FootStop
and Drive END helpers execute unchanged. No gameplay callback or path search is
emulated; this corpus proves the bounded owner-memory operations only.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance, run_checked, RET_MAGIC, SCRATCH
from tools.spatial_oracle.locomotor_track_cursor import OriginalCursor, LOCO, FOOT, SP
from tools.spatial_oracle.map_queries import dwords

BLOCKS = {
    'drive_fresh_one': (0x4B45F6, 0x4B464F),
    'drive_fresh_two': (0x4B45CB, 0x4B464F),
    'ship_fresh_one': (0x6A3C22, 0x6A3C7E),
    'ship_fresh_two': (0x6A3BF7, 0x6A3C7E),
    'drive_chain': (0x4B1DF7, 0x4B1E1A),
    'ship_chain': (0x6A143A, 0x6A145D),
    'drive_tube': (0x4B1362, 0x4B137D),
}


def witness(operation, directions, endpoint):
    n = OriginalCursor('ship' if operation.startswith('ship') else 'drive')
    n.seed(-1, -1, False, 0)
    u = n.uc
    reference = [-17, 301]
    u.mem_write(FOOT + 0x558, struct.pack('<hh', *reference))
    u.mem_write(FOOT + 0x5E0, dwords(*(directions + [-1] * (24 - len(directions)))))
    u.mem_write(FOOT + 0x5A0, dwords(0x12345678, 0x76543210))
    u.reg_write(UC_X86_REG_EAX, FOOT)
    u.reg_write(UC_X86_REG_ECX, FOOT)
    u.mem_write(SP + 0x38, dwords(*endpoint))
    if operation == 'foot_stop':
        u.mem_write(SP, dwords(RET_MAGIC))
        run_checked(u, 0x4DF0D0, RET_MAGIC, count=20, required_addresses=(0x4DF0DE,))
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4
        assert n.ints(FOOT + 0x5A0, 2) == [0, 0]
    elif operation == 'drive_end':
        # IPiggyback subobject at base+18, whose +50 stash is base+68.
        interface, output, stashed = LOCO + 0x18, SCRATCH + 0x7000, SCRATCH + 0x7100
        u.mem_write(interface + 0x50, dwords(stashed))
        u.mem_write(SP, dwords(RET_MAGIC, interface, output))
        run_checked(u, 0x4AF930, RET_MAGIC, count=30, required_addresses=(0x4AF94D,))
        assert u.reg_read(UC_X86_REG_ESP) == SP + 12
        assert n.ints(output) == [stashed]
        assert n.ints(interface + 0x50) == [0]
    else:
        if operation == 'drive_tube':
            u.reg_write(UC_X86_REG_ECX, 23)
        n.run(*BLOCKS[operation])
    queue = n.ints(FOOT + 0x5E0, 24)
    suffix = queue[:queue.index(-1)] if -1 in queue else queue
    return dict(input=dict(operation=operation, directions=directions,
                           reference=reference, endpoint=endpoint),
                output=dict(directions=suffix,
                            reference=list(struct.unpack('<hh', u.mem_read(FOOT + 0x558, 4)))))


def generate():
    cases = []
    for directions in ([2, 8, 7], [i % 8 for i in range(24)]):
        for operation in [*BLOCKS, 'foot_stop', 'drive_end']:
            for endpoint in ([2176, -731], [-8388992, 8388736]) if 'fresh' in operation else ([0, 0],):
                cases.append(witness(operation, directions, endpoint))
    for operation in ('foot_stop', 'drive_end'):
        cases.append(witness(operation, [], [0, 0]))
    return cases


def metadata():
    return provenance(
        scope='Foot+5E0 replay and signed Foot+558 reference across fresh acceptance, chain, tube, FootStop and Drive END',
        assumptions=[
            'Supplied accepted fresh/chain/tube frames; branch admission and Find_Path are not executed',
            'Queues cover a short route with tube direction8, full24 entries, and empty preservation cases',
            'Fresh endpoints include negative division toward zero and signed16 wrapping',
        ], substitutions=[
            'Original code is unpatched; stack, owner and locomotor memory are supplied directly',
            'FootStop and Drive END execute complete helper bodies; constructors, caller release and callback dispatch are excluded',
        ], entry_points={**{k: a for k, (a, _) in BLOCKS.items()},
                         'foot_stop': 0x4DF0D0, 'drive_end': 0x4AF930})


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
