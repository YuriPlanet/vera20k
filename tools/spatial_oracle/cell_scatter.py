"""Original Cell Scatter recipient selection; class Scatter calls are observers.

The original list snapshot/growth, Techno cast, rank and ability readers execute.
Only allocation/free and the recipient virtual are supplied. This is evidence
for dispatch eligibility and order, not displacement or full aircraft combat.
"""
from itertools import product
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import (SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC,
                                 load_image, run_checked, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

CELL, RULES, SOURCE, SPY, HEAP = [SCRATCH + n for n in (0, 0x1000, 0x4000, 0x5000, 0x8000)]


def query(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0x100000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    def read(p): return struct.unpack('<I', u.mem_read(p, 4))[0]
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x17ED, bytes([case.get('player_scatter', False)]))
    u.mem_write(RULES + 0x144C, dwords(case.get('threshold', 2)))
    u.mem_write(SOURCE, dwords(1000, -500, 750))
    addresses, ids = {}, {}
    for n, row in enumerate(case['objects']):
        actor = SCRATCH + 0x10000 + n * 0x4000
        vt, kind, house = actor + 0x1000, actor + 0x2000, actor + 0x3000
        addresses[row['id']] = actor
        ids[actor] = row['id']
        u.mem_write(vt, bytes(u.mem_read(0x7F5C70, 0x600)))
        u.mem_write(vt + 0x174, dwords(SPY))
        u.mem_write(actor, dwords(vt))
        u.mem_write(actor + 0x14, dwords(5 if row.get('techno', True) else 0))
        u.mem_write(actor + 0x150, struct.pack('<f', row.get('rank', 0)))
        u.mem_write(actor + 0x6C4, dwords(kind))
        u.mem_write(actor + 0x21C, dwords(house))
        u.mem_write(house + 0x24C, dwords(row.get('iq', 0)))
        u.mem_write(kind + 0x29F, bytes([row.get('veteran_scatter', False)]))
        u.mem_write(kind + 0x2B1, bytes([row.get('elite_scatter', False)]))
    for layer, offset in ((False, 0xE4), (True, 0xE8)):
        rows = [row for row in case['objects'] if row.get('bridge', False) == layer]
        pointers = [addresses[row['id']] for row in rows]
        u.mem_write(CELL + offset, dwords(pointers[0] if pointers else 0))
        for n, actor in enumerate(pointers):
            u.mem_write(actor + 0x30, dwords(pointers[n+1] if n+1 < len(pointers) else 0))
    dispatch, allocations = [], []
    heap = HEAP
    def ret(cleanup, result=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, result)
        u.reg_write(UC_X86_REG_EIP, read(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
    def observe(_u, pc, _size, _data):
        nonlocal heap
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x7C8E17:
            size = read(sp+4)
            allocations.append(size)
            allocation = heap
            heap += (size + 15) & ~15
            assert heap < SCRATCH + 0x10000
            ret(0, allocation)
        elif pc == 0x7C8B3D:
            ret(0)
        elif pc == SPY:
            assert read(sp+4) == SOURCE
            assert [read(sp+8), read(sp+12)] == [case.get('first_flag', True), case.get('dispatch_all', False)]
            dispatch.append(ids[u.reg_read(UC_X86_REG_ECX)])
            # A callback can unlink the rest of the cell. The saved array must
            # still deliver every original recipient in its original order.
            if case.get('unlink_on_first') and len(dispatch) == 1:
                u.mem_write(CELL + 0xE4, dwords(0))
                for actor in addresses.values(): u.mem_write(actor + 0x30, dwords(0))
            ret(12)
    u.hook_add(UC_HOOK_CODE, observe)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC, SOURCE, case.get('first_flag', True),
                          case.get('dispatch_all', False), case.get('bridge', False)))
    u.reg_write(UC_X86_REG_ECX, CELL)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x481670, RET_MAGIC, count=100000, required_addresses=[0x481670, 0x48180A])
    assert u.reg_read(UC_X86_REG_ESP) == sp + 20
    return dict(input=case, dispatch=dispatch, allocations=allocations)


def generate():
    cases = []
    for rank, veteran, elite, iq in product((0, 1, 2), (False, True), (False, True), (-1, 1, 2, 5)):
        cases.append(dict(name=f'rank{rank}_v{int(veteran)}_e{int(elite)}_iq{iq}',
                          objects=[dict(id=7, rank=rank, veteran_scatter=veteran, elite_scatter=elite, iq=iq)]))
    for forced, player, peer in product((False, True), repeat=3):
        cases.append(dict(name=f'cell_{int(forced)}{int(player)}{int(peer)}', dispatch_all=forced,
                          player_scatter=player, objects=[dict(id=7, techno=False), dict(id=3),
                                                         dict(id=9, rank=2 if peer else 0)]))
    objects = [dict(id=i, iq=5, bridge=i%2 == 0) for i in (23, 4, 19, 8, 17, 15, 13, 11, 9, 7, 5, 3, 1, 21, 25)]
    for bridge, unlink in product((False, True), repeat=2):
        cases.append(dict(name=f'ordered_{int(bridge)}_{int(unlink)}', bridge=bridge,
                          unlink_on_first=unlink, first_flag=False, objects=objects))
    cases.append(dict(name='elite_other_layer', objects=[dict(id=1), dict(id=2, rank=2, bridge=True)]))
    cases.append(dict(name='empty', objects=[]))
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='62 full original Cell Scatter calls: rank/ability/IQ eligibility, whole-list elite gate, both object lists, 13 recipients, callback unlinking and empty list. No displacement claim.',
        entry_points={'cell_scatter': 0x481670, 'techno_cast': 0x40DD70, 'ability': 0x70D0D0, 'array_growth': 0x40CE50},
        assumptions=['Supplied Unit objects and houses; copied original Unit vtable except Scatter observer.',
                     'Fixed source coordinate, explicit two forwarded booleans; no allocation failure.'],
        substitutions=['operator new[]/delete[] allocate/free scratch memory with original cdecl stack cleanup.',
                       'Object virtual+174 records recipient and args, optionally unlinks the cell; no class Scatter body executes.']))
