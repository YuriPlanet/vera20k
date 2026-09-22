"""Original Unit destination -> first Drive/Ship Process path-request timing.

Runs real setters and Process bodies, stopping at the first Foot::Find_Path
entry or ordinary return. No path result, route or movement is supplied.
"""
from itertools import product
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.track_destination import make_destination_fixture, ACTOR, LOCO, CELL
from tools.spatial_oracle.unit_scatter_state import SP

PROCESS = {'drive': 0x4B0500, 'ship': 0x69FC10}
FIND_PATH = 0x4D3920


def query(case):
    u, call, read32 = make_destination_fixture(dict(case, entry='unit', head=[0, 0, 0]))
    calls = []
    observed = {0x741970: 'unit', 0x4D94B0: 'foot', 0x4AFD40: 'drive_move',
                0x69F450: 'ship_move', FIND_PATH: 'find_path',
                PROCESS[case['family']]: 'process'}
    u.hook_add(UC_HOOK_CODE, lambda _u, address, _size, _data:
               calls.append(observed[address]) if address in observed else None)
    call(0x741970, ACTOR, [CELL, 1])
    assert 'find_path' not in calls

    def snapshot():
        signed = lambda address, count: list(struct.unpack('<' + 'i' * count,
                                                           u.mem_read(address, count * 4)))
        return dict(destination=signed(LOCO + 0x34, 3), head=signed(LOCO + 0x40, 3),
                    nav=[11, 10] if read32(ACTOR + 0x5A4) == CELL else None,
                    path=signed(ACTOR + 0x5E0, 4), power=u.mem_read(LOCO + 0x10, 1)[0],
                    movement_timer=[read32(ACTOR + 0x640), read32(ACTOR + 0x648)])

    accepted = snapshot()
    # Independent timer prestate represents a later Process invocation. Zero
    # is the exact accepted-setter output; 7 isolates the original wait arm.
    u.mem_write(ACTOR + 0x640, dwords(100, 0, case['delay']))
    entry = read32(read32(LOCO + 4) + 0x40)
    assert entry == PROCESS[case['family']]
    u.mem_write(SP, dwords(RET_MAGIC, LOCO + 4))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, 0)
    stop = run_checked(u, entry, (RET_MAGIC, FIND_PATH), count=200000,
                       required_addresses=[entry])
    return dict(input=case, accepted=accepted, process=snapshot(), calls=calls,
                requested_path=stop == FIND_PATH)


def generate():
    return [query(dict(family=family, power_off=power_off, delay=delay))
            for family, power_off, delay in product(PROCESS, (False, True), (0, 7))]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='8 original Unit Cell setter -> first Drive/Ship Process path-request boundaries',
        entry_points={'unit': 0x741970, 'foot': 0x4D94B0, 'find_path': FIND_PATH, **PROCESS},
        substitutions=['Only OS Interlocked imports inherited from track_destination fixture.'],
        assumptions=[
            'Real Unit/Foot/locomotor vtables and calls. No paid head, radio/EMP/deploy/lift/particle state.',
            'Both power values; accepted movement timer is zero. Delay7 is supplied after acceptance to isolate the Process wait arm.',
            'Shared flat map/Rules fixture; PathDelay is zero. Stops BEFORE Find_Path executes; no search result, movement or full Process claim.']))
