"""Original Fly takeoff phase callback4CE680, with real Aircraft methods.

This callback clears both phase flags, then conditionally retargets one facing
and sets target speed. It does not itself change coordinates. The surrounding
Process phase admission/Mark/Display transaction remains outside this corpus.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.crate_speed_effect import fixture, CELL
from tools.spatial_oracle.map_queries import dwords, EMPTY_TABLE, TABLE, GLOBAL_TABLE

LOCO, ARG, OUTPUT = SCRATCH + 0x1B000, SCRATCH + 0x1D000, SCRATCH + 0x1D100


def execute(case, *, phase_transaction=False):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind='aircraft')]))
    owner = actors[0]
    object_type = owner + 0x800
    u.mem_write(TABLE, EMPTY_TABLE)
    u.mem_write(GLOBAL_TABLE, dwords(TABLE, 0x40000))
    u.mem_write(TABLE + (10 * 512 + 10) * 4, dwords(CELL))
    u.mem_write(CELL + 0x11B, bytes([case.get('level', 0), case.get('slope', 0)]))
    u.mem_write(CELL + 0x140, dwords(0x100 if case.get('bridge', False) else 0))
    for address, value in ((0xAC13C8, 104), (0xAC13BC, 416), (0x8B3CAC, 416)):
        u.mem_write(address, dwords(value))
    u.mem_write(owner + 0x8C, bytes([int(case.get('on_bridge', False))]))
    u.mem_write(owner + 0x9C, dwords(2688, 2688, case['z']))
    u.mem_write(owner + 0x6C0, dwords(0x7E2250))
    u.mem_write(owner + 0x118, dwords(SCRATCH + 0x1C000 if case.get('loaded') else 0))
    u.mem_write(object_type + 0xDFC, bytes([int(case.get('carryall', False))]))
    u.mem_write(object_type + 0x71C, dwords(case.get('rot', 5)))

    def call(entry, receiver, *args):
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ECX, receiver)
        u.reg_write(UC_X86_REG_ESP, sp)
        run_checked(u, entry, RET_MAGIC, count=20000)
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + len(args) * 4
        return u.reg_read(UC_X86_REG_EAX)

    # Native Techno ctor calls these two constructors; Aircraft InitFromType
    # then supplies Type ROT to both. Execute that original interior block.
    u.mem_write(0xA8ED84, dwords(90))
    for offset in (0x388, 0x3A0):
        call(0x4C91C0, owner + offset)
    u.reg_write(UC_X86_REG_ESI, owner)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x413FD2, 0x41401A, count=10000,
                required_addresses=[0x413FE7, 0x414001, 0x414015])
    assert u.reg_read(UC_X86_REG_ESP) == sp
    for offset, initial, target in ((0x388, 0x4000, 0xC000), (0x3A0, 0x6000, 0x2000)):
        u.mem_write(ARG, dwords(initial))
        call(0x4C9300, owner + offset, ARG)
        u.mem_write(ARG, dwords(target))
        call(0x4C9220, owner + offset, ARG)
    call(0x4CC9A0, LOCO)
    u.mem_write(LOCO + 0xC, dwords(owner))
    u.mem_write(LOCO + 0x1C, dwords(*case.get('destination', [3456, 2688, 0])))
    u.mem_write(LOCO + 0x38, dwords(case.get('target', 1500)))
    u.mem_write(LOCO + 0x40, struct.pack('<d', 0.25))
    u.mem_write(LOCO + 0x50, bytes([1, int(case.get('landing', False))]))
    u.mem_write(0xA8ED84, dwords(100))
    calls = []
    phase_calls = []
    if phase_transaction:
        from tools.spatial_oracle.crate_ground_membership import LAYERS
        buffers, peer = SCRATCH + 0x18000, SCRATCH + 0x17000
        u.mem_write(owner + 0x6C, dwords(case.get('health',100)))
        u.mem_write(owner + 0x90, b'\1')
        u.mem_write(owner + 0x674, dwords(LOCO+4))
        u.mem_write(owner + 0x94, dwords(-1))
        u.mem_write(object_type + 0xE0A, bytes([case.get('landable',True)]))
        u.mem_write(LOCO + 0x50, bytes([case.get('taking_off',True),0]))
        u.mem_write(peer, bytes(u.mem_read(owner,0x700)))
        u.reg_write(UC_X86_REG_ESP,sp)
        run_checked(u,0x4A8630,0x4A866D,count=100)
        for layer in range(5):
            u.mem_write(LAYERS+24*layer+4,dwords(buffers+0x100*layer,16))
        if case.get('marked',False):
            call(0x4D3780,owner,1)
        call(0x4A9720,0x87F7E8,owner)
        call(0x4A9720,0x87F7E8,peer)

    def observe(_u, address, _size, _data):
        if phase_transaction and address in (0x4D3780,0x4A9770,0x4A9720,0x4CE680):
            arg=struct.unpack('<I',u.mem_read(u.reg_read(UC_X86_REG_ESP)+4,4))[0]
            phase_calls.append(['mark',arg] if address==0x4D3780 else
                               ['remove' if address==0x4A9770 else 'submit' if address==0x4A9720 else 'takeoff'])
        if address == 0x47B3A0:
            assert u.reg_read(UC_X86_REG_ECX) == CELL, 'ground query selected wrong cell'
        elif address == 0x4CE6CB:
            assert u.reg_read(UC_X86_REG_EAX) == CELL, 'bridge query selected wrong cell'
        elif address == 0x4C9220:
            receiver = u.reg_read(UC_X86_REG_ECX)
            assert receiver in (owner + 0x388, owner + 0x3A0)
            calls.append('primary_set' if receiver == owner + 0x388 else 'secondary_set')

    u.hook_add(UC_HOOK_CODE, observe)
    returned = call(0x4CD2A0 if phase_transaction else 0x4CE680, LOCO) & 0xFF
    facings = []
    for offset in (0x388, 0x3A0):
        call(0x4C93D0, owner + offset, OUTPUT)
        current = struct.unpack('<H', u.mem_read(OUTPUT, 2))[0]
        destination, previous, start, _unused, duration, rate = struct.unpack(
            '<6I', u.mem_read(owner + offset, 24))
        facings.append(dict(destination=destination & 0xFFFF, previous=previous & 0xFFFF,
                            start=start, duration=duration, rate=rate & 0xFFFF, current=current))
    result = dict(input=case, returned=returned, facings=facings, calls=calls,
                phase=list(u.mem_read(LOCO + 0x50, 2)),
                speed=struct.unpack('<d', u.mem_read(LOCO + 0x40, 8))[0],
                coordinates=list(struct.unpack('<iii', u.mem_read(owner + 0x9C, 12))),
                on_bridge=bool(u.mem_read(owner + 0x8C, 1)[0]))
    if phase_transaction:
        layers=[]
        for layer in range(5):
            count=struct.unpack('<I',u.mem_read(LAYERS+24*layer+16,4))[0]
            assert count<=16
            pointers=struct.unpack('<'+'I'*count,u.mem_read(buffers+0x100*layer,count*4))
            layers.append([0 if pointer==owner else 1 if pointer==peer else -1 for pointer in pointers])
        result.update(phase_calls=phase_calls,layers=layers,marked=bool(u.mem_read(owner+0x74,1)[0]))
    return result


def cases():
    cases = [dict(z=z, carryall=carryall, loaded=loaded)
             for carryall, loaded in ((False, False), (False, True), (True, False), (True, True))
             for z in (0, 750, 751, 800, 801, 1000, 1001, 1034, 1035, 1500)]
    cases += [dict(z=z, bridge=True, on_bridge=on_bridge)
              for on_bridge in (False, True) for z in (415, 416, 1166, 1167, 1416, 1417)]
    cases += [dict(z=z, level=2, slope=1) for z in (1010, 1011, 1260, 1261)]
    cases += [dict(z=900, destination=[2688 + dx, 2688 + dy, 0])
              for dx, dy in ((0, -768), (768, -768), (768, 768), (0, 768),
                             (-768, 768), (-768, 0), (-768, -768), (617, -289), (0, 0), (-2688, -2688))]
    cases += [dict(z=z, rot=rot, landing=True)
              for rot in (-255, -1, 0, 127, 128) for z in (900, 1200)]
    cases += [dict(z=z, target=target) for z, target in ((0, 0), (1, 0), (-2, -2), (-1, -2))]
    return cases


def generate():
    return [execute(case) for case in cases()]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'takeoff_callback': 0x4CE680, 'aircraft_facing_init': 0x413FD2,
                      'fly_constructor': 0x4CC9A0, 'get_height': 0x5F5F40,
                      'landing_base': 0x41B6A0, 'set': 0x4C9220, 'current': 0x4C93D0},
        assumptions=['Real Aircraft vtable and auxiliary interface; live loaded/Carryall state supplied, radio slots empty.',
                     'Original facing constructors and Aircraft InitFromType rate block; both facings start active turns at frame90, callback runs at frame100.',
                     'Original Fly constructor followed by supplied destination XYZ, target height, taking-off=true, optional landing flag and target speed0.25.',
                     'One real map cell10,10 with table AND length initialized; read-only observers verify ground/bridge cell selection.104/416 terrain constants supplied.'],
        substitutions=[],
        scope='80 original takeoff callbacks: both height thresholds, live Carryall base, bridge normalization, ramp height, retained destination directions including zero, signed ROT and both phase flags. Also executes Aircraft facing initialization. Excludes outer Process admission, BeginTakeoff, phase Mark/Display transaction, landing and complete aircraft flight; no Rust production parity claim.',
    ))
