"""Original Drive/Ship MoveTo, Foot destination and ordinary Unit caller.

All class and locomotor functions run unchanged. The supplied objects have no
radio contacts, lift links, retained fire particles, EMP or deploy timer.
"""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.unit_source_scatter import make_source_fixture, ACTOR, TYPE, LOCO, CELLS
from tools.spatial_oracle.unit_entry import EXTRA, HOUSE, CELL
from tools.spatial_oracle.unit_scatter_state import SP


def make_destination_fixture(case):
    u, call, read32 = make_source_fixture(dict(case, live_entry=True))
    u.mem_map(EXTRA, 0x30000)
    family = case['family']
    if family == 'ship':
        call(0x69EC50, LOCO, [])
        u.mem_write(LOCO + 0xC, dwords(ACTOR))
        u.mem_write(LOCO + 0x14, dwords(1))
    u.mem_write(LOCO + 0x10, bytes([not case.get('power_off', False)]))
    u.mem_write(ACTOR, dwords(0x7F5C70))
    u.mem_write(TYPE, dwords(0x7F6218))
    u.mem_write(ACTOR + 0x21C, dwords(HOUSE))
    u.mem_write(ACTOR + 0x14, dwords(5))
    # Unit735416, Radio65A750 and Foot4D31E0 constructor prestates.
    u.mem_write(ACTOR + 0x6D8, dwords(-1))
    u.mem_write(ACTOR + 0xE0, dwords(0x7E180C, EXTRA + 0x2F000, 1))
    u.mem_write(ACTOR + 0xEC, b'\x01\x01')
    for offset in (0x588, 0x5AC):
        call(0x4E0E80, ACTOR + offset, [0, 0])
        u.mem_write(ACTOR + offset, dwords(0x7E91EC))
        u.mem_write(ACTOR + offset + 0x10, dwords(0, 10))
    u.mem_write(0x8871E0, dwords(EXTRA + 0x10000))
    u.mem_write(EXTRA + 0x10000 + 0x1768, dwords(22))
    u.mem_write(ACTOR + 0x270, bytes([case.get('warp_out', False), case.get('warp_in', False)]))
    u.mem_write(ACTOR + 0x5A0, dwords(CELLS))
    u.mem_write(ACTOR + 0x5A4, dwords(CELL if case.get('same_nav') else 0))
    u.mem_write(ACTOR + 0x5E0, dwords(2, 3, 4, 5))
    u.mem_write(ACTOR + 0x558, packed(9, 8))
    u.mem_write(ACTOR + 0x640, dwords(50, 0, 5))
    u.mem_write(ACTOR + 0x64C, dwords(7))
    u.mem_write(ACTOR + 0x668, dwords(40, 0, 6))
    u.mem_write(ACTOR + 0x6B7, b'\x01')
    u.mem_write(ACTOR + 0x6AC, bytes([case.get('skip_move', False)]))
    u.mem_write(ACTOR + 0x1F8, bytes([case.get('force_reassign', False)]))
    u.mem_write(LOCO + 0x34, dwords(*case.get('prior', [700, 800, 900])))
    u.mem_write(LOCO + 0x40, dwords(*case.get('head', [2816, 2688, 123])))
    u.mem_write(CELL + 0x140, dwords(0x100 if case.get('bridge') else 0))
    # Original bridge-scale leaves, with their established level scale104.
    for address in (0x8A07D0, 0xB07838):
        u.mem_write(address, dwords(104))
    call(0x4AF4A0, 0, [])
    call(0x69EBB0, 0, [])
    assert read32(0x8A07C4) == read32(0xB0782C) == 416
    return u, call, read32


def query(case):
    u, call, read32 = make_destination_fixture(case)
    events = []
    addresses = {0x741970:'unit', 0x4D94B0:'foot', 0x4AFD40:'drive_move',
                 0x69F450:'ship_move', 0x4E0190:'clear_queue', 0x565730:'lookup'}
    def observe(_u, address, _size, _data):
        # Destination acceptance never calls Foot::Find_Path. The first
        # locomotor Process owns that request, even for an obstructed route.
        assert address != 0x4D3920, 'destination setter performed Find_Path'
        if address in addresses:
            events.append(addresses[address])
    u.hook_add(UC_HOOK_CODE, observe)
    before = bytes(u.mem_read(ACTOR, 0x700))
    entry = case['entry']
    if entry == 'move':
        call(0x4AFD40 if case['family'] == 'drive' else 0x69F450, 0,
             [LOCO + 4, *case['request']])
        assert u.reg_read(UC_X86_REG_ESP) == SP + 20
        assert bytes(u.mem_read(ACTOR, 0x700)) == before
    else:
        call(0x741970 if entry == 'unit' else 0x4D94B0, ACTOR, [CELL, 1])
        assert u.reg_read(UC_X86_REG_ESP) == SP + 12
    call(read32(read32(LOCO + 4) + 0x10), 0, [LOCO + 4])
    moving = bool(u.reg_read(UC_X86_REG_EAX) & 255)
    signed = lambda address, count: list(struct.unpack('<' + 'i' * count, u.mem_read(address, count * 4)))
    return dict(input=case, events=events,
                destination=signed(LOCO + 0x34, 3), head=signed(LOCO + 0x40, 3),
                power=u.mem_read(LOCO + 0x10, 1)[0], moving=moving,
                nav=[11, 10] if read32(ACTOR + 0x5A4) == CELL else None,
                aux=bool(read32(ACTOR + 0x5A0)), path=signed(ACTOR + 0x5E0, 4),
                reference=list(struct.unpack('<hh', u.mem_read(ACTOR + 0x558, 4))),
                movement_timer=[read32(ACTOR + 0x640), read32(ACTOR + 0x648)],
                blocked_timer=[read32(ACTOR + 0x668), read32(ACTOR + 0x670)],
                blocked=u.mem_read(ACTOR + 0x6B7, 1)[0], retries=read32(ACTOR + 0x64C),
                skip_move=u.mem_read(ACTOR + 0x6AC, 1)[0],
                force_reassign=u.mem_read(ACTOR + 0x1F8, 1)[0])


def generate():
    cases = []
    for family in ('drive', 'ship'):
        for warp in ({}, {'warp_out':True}, {'warp_in':True}):
            for power_off in (False, True):
                for bridge in (False, True):
                    base = dict(family=family, power_off=power_off, bridge=bridge, **warp)
                    for request in ([2944, 2688, -123], [0, 0, 0]):
                        cases.append(dict(base, entry='move', request=request))
                    cases.append(dict(base, entry='move', request=[0, 0, 0], head=[0, 0, 0]))
                    cases.append(dict(base, entry='foot'))
                    cases.append(dict(base, entry='unit'))
        for extra in ({'same_nav':True}, {'same_nav':True, 'force_reassign':True}, {'skip_move':True}):
            cases.append(dict(family=family, entry='unit', **extra))
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='126 complete original calls:72 Drive/Ship MoveTo,24 Foot destination,30 ordinary Unit Cell destination, each followed by original IsMoving. Warp-in/out, power, zero/raw/bridge coordinates, same NavCom/force and one-shot skip-MoveTo. No full Scatter/Process parity.',
        entry_points={'unit':0x741970,'foot':0x4D94B0,'drive_move':0x4AFD40,'ship_move':0x69F450,
                      'drive_bridge_scale':0x4AF4A0,'ship_bridge_scale':0x69EBB0},
        assumptions=['Original constructors for Drive/Ship and embedded vectors; supplied Unit constructor6D8=-1, empty Radio contact slot, House, map and Rules BlockagePathDelay22. No EMP, Foot6A0 timer, lift/particle links, deploy, Jumpjet/Teleporter type arms, docking buildings or queue allocation.',
                     'Original bridge scales execute from supplied established level scale104. Native Unit/Foot/locomotor vtables are unchanged; actor and head are supplied prestates. Power remains unchanged; MoveTo refusal does not refuse the enclosing accepted Foot setter.'],
        substitutions=['Only inherited OS Interlocked import operations. No gameplay replacement.']))
