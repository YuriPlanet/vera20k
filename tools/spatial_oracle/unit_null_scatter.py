"""Original Unit Scatter's null-coordinate arm, after its state gates.

Reuses the unit_scatter_state fixture (real Unit vtable, Drive COM object,
Facing and gate readers) and runs 743A50 with the null coordinate to its
return. Foot+4C, the map cell lookup and Scenario RNG are original. FNPC
answers and the destination and QueueMission receivers are observers.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.unit_scatter_state import ACTOR, LOCO, SOURCE, TYPE, make_fixture

MAP, TABLE = 0x87F7E8, 0xC00000
CELLS, SCENARIO = SCRATCH + 0x9000, SCRATCH + 0xC000
FNPC, RANDOM, COORDINATE, QUEUE, SET = 0x56DC20, 0x65C7E0, 0x4DBDF0, 0x5B35E0, 0x741970


def query(case):
    u, call, read32 = make_fixture(dict(mission=5))
    answer = case['answer']
    table = bytearray(0x100000)
    for i, (x, y) in enumerate({tuple(answer), (10, 10)}):
        cell = CELLS + i * 0x200
        struct.pack_into('<I', table, (y * 512 + x) * 4, cell)
        u.mem_write(cell + 0x24, packed(x, y))
    u.mem_write(TABLE, bytes(table))
    u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    u.mem_write(ACTOR + 0x684, b'\xff')  # no tube: Foot+4C asks the locomotor
    u.mem_write(ACTOR + 0x9C, dwords(*case.get('actor', [2688, 2688, 0])))
    u.mem_write(ACTOR + 0x8C, bytes([case.get('on_bridge', False)]))
    u.mem_write(TYPE + 0x67C, dwords(case.get('speed_type', 0)))
    for address in [0x89C848, 0xA8F200, 0x8B3DA8]:
        u.mem_write(address, dwords(0, 0, 0))
    u.mem_write(0xA8B230, dwords(SCENARIO))
    call(0x65C6D0, SCENARIO + 0x218, [31])
    before = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
    process = read32(read32(LOCO + 4) + 0x40)  # the Drive's ILocomotion::Process
    events, destination, seed = [], None, None

    def ret(cleanup, value):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def observe(_u, address, _size, _data):
        nonlocal destination, seed
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == COORDINATE:
            events.append('coordinate')
        elif address == RANDOM:
            events.append('random')
        elif address == FNPC:
            args = [read32(sp + 4 + i * 4) for i in range(15)]
            seed = list(struct.unpack('<hh', u.mem_read(args[1], 4)))
            # (out, &cell, SpeedType, zone -1, MZone 0, bridge byte, 1x1,
            # 0, 1, 0, 1, &NullCell, 0, 0); the bridge dword's upper bytes
            # are register residue and only its low byte is read.
            assert args[2:5] == [case.get('speed_type', 0), 0xFFFFFFFF, 0], args
            assert args[5] & 0xFF == int(case.get('on_bridge', False)), args
            assert args[6:12] == [1, 1, 0, 1, 0, 1], args
            assert list(struct.unpack('<hh', u.mem_read(args[12], 4))) == [0, 0]
            assert args[13:] == [0, 0], args
            events.append('fnpc')
            u.mem_write(args[0], packed(*answer))
            ret(60, args[0])
        elif address == QUEUE:
            events.append('queue')
            ret(8, 0)
        elif address == process:
            events.append('process')
            ret(4, 0)
        elif address == SET:
            destination = [list(struct.unpack('<hh', u.mem_read(read32(sp + 4) + 0x24, 4))),
                           read32(sp + 8)]
            events.append('destination')
            ret(8, 0)

    u.hook_add(UC_HOOK_CODE, observe)
    u.mem_write(SOURCE, dwords(0, 0, 0))
    call(0x743A50, ACTOR, [SOURCE, *case.get('flags', [1, 1])])
    return dict(input=case, events=events, fnpc_seed=seed, destination=destination,
                random_indices=[before, [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]])


def generate():
    cases = [dict(answer=[11, 10]), dict(answer=[0, 0]), dict(answer=[11, 10], flags=[1, 0]),
             dict(answer=[9, 12], actor=[3000, 2400, 0], on_bridge=True, speed_type=5)]
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Unit743A50 null-coordinate arm 743BE0..744070 after the admitted state gates: FNPC literal arguments, the Foot+4C seed, SetDestination(cell,1) only for a non-NullCell answer, no RNG, QueueMission or Process. Not FNPC, setter or movement parity.',
        entry_points={'scatter': 0x743A50, 'navigation_coord': COORDINATE, 'get_cell': 0x5657A0,
                      'random_seed': 0x65C6D0, 'random_ranged': RANDOM},
        assumptions=['unit_scatter_state make_fixture: original Unit vtable, constructed Drive, current mission 5, no NavCom, no turn, both deploy bytes clear.',
                     'No tube (Foot+684=-1) and a null Drive head, so Foot+4C returns the physical coordinate; zero NullCoord/NullCell globals; Scenario seeded 31.'],
        substitutions=['FNPC 56DC20 answers the row cell after checking its arguments; Unit SetDestination 741970, QueueMission 5B35E0 and the Drive Process slot are recording observers.']))
