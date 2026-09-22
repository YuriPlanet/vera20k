"""Original Mission_Attack state1 through search, destination and RNG epilogue.

Only OS Interlocked imports are emulated. Original COM, Aircraft/Foot setters,
Fly MoveTo and all search callees execute. Supplied airborne owners exclude
takeoff, repair-pad contacts, linked lifts and queued Enter preprocessing.
"""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import (
    Fixture, OWNER, TARGET, TYPE, SCENARIO, SIDE, CELLS, cell, dwords,
)
from tools.spatial_oracle.fly_destination import LOCO, RULES


def execute(case, configure=None):
    f = Fixture(case)
    u = f.u
    u.mem_write(TARGET + 4, dwords(0x7F5C54))
    for y in range(SIDE):
        for x in range(SIDE):
            u.mem_write(cell(x, y), dwords(0x7E4EEC, 0x7E4ED0))
    f.call(0x4CC9A0, LOCO + 0x100, [])
    u.mem_write(LOCO + 0x10C, dwords(TARGET))
    u.mem_write(TARGET + 0x674, dwords(LOCO + 0x104))
    u.mem_write(TARGET + 0x684, b'\xff')
    f.call(0x4CC9A0, LOCO, [])
    u.mem_write(LOCO + 0xC, dwords(OWNER))
    u.mem_write(LOCO + 0x14, dwords(1))  # already-owned COM reference
    u.mem_write(LOCO + 0x1C, dwords(8192, 8192, 1500))
    u.mem_write(LOCO + 0x34, b'\1')
    u.mem_write(LOCO + 0x38, dwords(37))
    u.mem_write(LOCO + 0x10, bytes([case.get('powered', True)]))
    u.mem_write(OWNER + 0x674, dwords(LOCO + 4))
    u.mem_write(OWNER + 0x6C, dwords(100))
    u.mem_write(OWNER + 0xAC, dwords(1))
    u.mem_write(OWNER + 0xB4, dwords(0xFFFFFFFF))
    u.mem_write(OWNER + 0xBC, dwords(1))
    u.mem_write(OWNER + 0x2B4, dwords(0 if case.get('null_target', False) else TARGET))
    u.mem_write(OWNER + 0x2FC, dwords(case.get('ammo', 2)))
    u.mem_write(OWNER + 0x6C8, bytes([case.get('pending', False)]))
    u.mem_write(OWNER + 0x6D2, b'\1')
    u.mem_write(OWNER + 0x5A0, dwords(TARGET))
    u.mem_write(OWNER + 0x5A4, dwords(cell(32, 32) if case.get('old_nav', False) else 0))
    u.mem_write(OWNER + 0x6AD, bytes([case.get('swap', False)]))
    u.mem_write(OWNER + 0x82, bytes([case.get('open_transport', False)]))
    u.mem_write(OWNER + 0x2E4, dwords(TARGET if case.get('bunker', False) else 0))
    u.mem_write(OWNER + 0x640, dwords(7, 0, 23, 19))
    u.mem_write(OWNER + 0x668, dwords(9, 0, 29))
    u.mem_write(OWNER + 0x6B7, b'\1')
    u.mem_write(TYPE + 0xE0A, b'\1')
    u.mem_write(TYPE + 0x618, dwords(-1))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x7B4, dwords(1500))
    u.mem_write(RULES + 0x1768, dwords(11))
    u.mem_write(0xA8ED84, dwords(100))
    # ReadINI widens an f32 Rate into the native table's double.
    rate = struct.unpack('<f', struct.pack('<f', case.get('rate', 0.016)))[0]
    u.mem_write(0xA8E3A8 + 0x20 + 0x10, struct.pack('<d', rate))
    u.mem_write(0x89E7C0, dwords(104))
    u.mem_write(0xAC13C8, dwords(104))
    if configure is not None:
        configure(f)
    calls = []
    def read(p): return struct.unpack('<I', u.mem_read(p, 4))[0]
    def signed(p): return struct.unpack('<i', u.mem_read(p, 4))[0]
    def observe(_u, pc, _size, _data):
        if pc in (read(0x7E11C8), read(0x7E11CC)):
            sp = u.reg_read(UC_X86_REG_ESP)
            p = read(sp + 4)
            value = (read(p) + (1 if pc == read(0x7E11C8) else -1)) & 0xFFFFFFFF
            u.mem_write(p, dwords(value))
            u.reg_write(UC_X86_REG_EAX, value)
            u.reg_write(UC_X86_REG_EIP, read(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 8)
        elif pc in (0x4197C0, 0x41AA80, 0x4D94B0, 0x4CCC80, 0x4CCFD0, 0x65C7E0):
            sp = u.reg_read(UC_X86_REG_ESP)
            calls.append(dict(entry=hex(pc), args=[read(sp + 4), read(sp + 8)]))
        elif pc in (0x4CF950, 0x4CFA70):
            raise AssertionError('supplied airborne fixture must not enter phase callbacks')
    u.hook_add(UC_HOOK_CODE, observe)
    delay = f.call(0x417FE0, OWNER, [])
    pointer = read(OWNER + 0x5A4)
    nav = None if pointer == 0 else ('target' if pointer == TARGET else
          list(struct.unpack('<hh', u.mem_read(pointer + 0x24, 4))))
    result = dict(input=case, state=read(OWNER + 0xBC), delay=delay, nav=nav,
                  aux=read(OWNER + 0x5A0) != 0, ammo=signed(OWNER + 0x2FC),
                  pending=bool(u.mem_read(OWNER + 0x6C8, 1)[0]),
                  latch=bool(u.mem_read(OWNER + 0x6D2, 1)[0]),
                  destination=list(struct.unpack('<iii', u.mem_read(LOCO + 0x1C, 12))),
                  moving=bool(u.mem_read(LOCO + 0x34, 1)[0]), height=signed(LOCO + 0x38),
                  movement_timer=[signed(OWNER + 0x640), signed(OWNER + 0x648)],
                  blocked_timer=[signed(OWNER + 0x668), signed(OWNER + 0x670)],
                  retry=signed(OWNER + 0x64C), blocked=bool(u.mem_read(OWNER + 0x6B7, 1)[0]),
                  calls=calls)
    result['next_random'] = f.call(0x65C780, SCENARIO + 0x218, [])
    return result


def generate():
    cases = [dict(name='base'), dict(name='direct_target', rot=0),
             dict(name='failed_search', range=512, old_nav=True),
             dict(name='no_target', null_target=True, old_nav=True),
             dict(name='zero_ammo', ammo=0, old_nav=True),
             dict(name='last_pending', ammo=1, pending=True, old_nav=True),
             dict(name='pending', ammo=2, pending=True),
             dict(name='unpowered', powered=False),
             dict(name='alternate_seed', seed=2), dict(name='authored_rate', rate=0.1)]
    for gate in ('swap', 'open_transport', 'bunker'):
        for old in (False, True):
            cases.append(dict(name=f'{gate}_{old}', old_nav=old, **{gate: True}))
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='16 full original Mission_Attack state1 calls through live search, Aircraft/Foot destination setter, Fly MoveTo and mission delay/RNG. Supplied airborne flat-map states; not full attack or flight parity.',
        entry_points={'attack': 0x417FE0, 'search': 0x4197C0, 'aircraft_destination': 0x41AA80,
                      'foot_destination': 0x4D94B0, 'fly_move': 0x4CCC80},
        assumptions=['Original Aircraft, Unit, Cell and constructed Fly COM tables; owned reference count1.',
                     'Current Attack, no queued Enter, no repair-pad contact, linked lift, attached304 object or6AC latch; owner airborne500.',
                     'Inherited search fixture map and RNG; supplied frame100, timers, raw ammo flags and mission Rate.',
                     'Negative/null destination paths with live Attack target skip Fly Stop as in original Foot setter.'],
        substitutions=['OS InterlockedIncrement/Decrement imports update their pointed count with stdcall cleanup; no gameplay call replaced.']))
