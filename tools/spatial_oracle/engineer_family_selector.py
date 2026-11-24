"""Original Engineer519C07 family selector, through the repair entry boundary.

Supplied already-admitted Engineer frame and sparse CellClass table. Original
Infantry vtable7EB058 +1B8 ->41BEA0 and Map5657A0 execute unchanged. Stops
BEFORE570050/573540; no repair, side effect receiver, constructor or entry
admission is supplied as though it executed. Hooks only observe instructions.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed

MAP, TABLE, DUMMY = 0x87F7E8, 0xC00000, 0xABDC50
ACTOR, HUT, CELLS = SCRATCH + 0x1000, SCRATCH + 0x2000, SCRATCH + 0x3000
BEGIN, LOW, HIGH = 0x519C07, 0x570050, 0x573540

def execute(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(ACTOR, dwords(0x7EB058))
    # Physical center coordinate, not a supplied GetCell answer.
    u.mem_write(ACTOR + 0x9C, dwords(10 * 256 + 128, 10 * 256 + 128, 1040))
    u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    u.mem_write(TABLE, bytes(0x100000))
    u.mem_write(0xABAD1C, dwords(case['wood_base']))
    u.mem_write(DUMMY + 0x38, dwords(65535))
    u.mem_write(DUMMY + 0x44, dwords(-1))
    u.mem_write(DUMMY + 0x24, packed(1234, -2345))
    changes = {tuple(row[:2]): row[2:] for row in case['cells']}
    pointers = {}
    for index, (y, x) in enumerate((y, x) for y in range(8, 13) for x in range(8, 13)):
        if case.get('missing') == [x, y]:
            continue
        cell = CELLS + index * 0x200
        pointers[cell] = [x, y]
        tile, overlay = changes.get((x, y), (65535, -1))
        u.mem_write(cell + 0x24, packed(x, y))
        u.mem_write(cell + 0x38, dwords(tile))
        u.mem_write(cell + 0x44, dwords(overlay))
        u.mem_write(TABLE + (y * 512 + x) * 4, dwords(cell))
    def read32(address):
        return struct.unpack('<I', u.mem_read(address, 4))[0]
    def signed(address):
        return struct.unpack('<i', u.mem_read(address, 4))[0]
    def xy(address):
        return list(struct.unpack('<hh', u.mem_read(address, 4)))
    assert read32(0x7EB058 + 0x1B8) == 0x41BEA0
    spans = [(BEGIN, 0x519D17), (0x41BEA0, 0x41BEDD), (0x5657A0, 0x5657D8)]
    code = [bytes(u.mem_read(a, b-a)) for a, b in spans]
    trace, queries, selected_latch = [], [], []
    def observe(_u, address, _size, _data):
        here_sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x519CD9:
            # The final GetCell reuses this stack slot for the repair argument.
            selected_latch.append(u.mem_read(sp+0x54, 1)[0])
        elif address == 0x41BED9:
            trace.append(dict(kind='engineer_cell', cell=xy(u.reg_read(UC_X86_REG_EAX))))
        elif address == 0x5657A0:
            phase = {0x519C58: 'tile', 0x519C9A: 'overlay'}[read32(here_sp)]
            query = dict(kind='map_lookup', field=phase, cell=xy(read32(here_sp+4)))
            queries.append(query)
            trace.append(query)
        elif address in (0x519C58, 0x519C9A):
            cell = u.reg_read(UC_X86_REG_EAX)
            query = queries[-1]
            query['identity'] = 'dummy' if cell == DUMMY else 'real'
            query['returned_cell'] = xy(cell + 0x24)
            query['value'] = signed(cell + (0x38 if address == 0x519C58 else 0x44))
        elif address in (0x65C780, 0x65C7E0, 0x598030):
            raise AssertionError('selector unexpectedly drew RNG')
    u.hook_add(UC_HOOK_CODE, observe)
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ESI, ACTOR)
    u.reg_write(UC_X86_REG_EDI, HUT)
    end = run_checked(u, BEGIN, (LOW, HIGH), count=25000,
                      required_addresses=(BEGIN, 0x41BEA0, 0x5657A0, 0x519C53, 0x519C95, 0x519CD9))
    assert [bytes(u.mem_read(a, b-a)) for a, b in spans] == code
    assert u.reg_read(UC_X86_REG_ESP) == sp - 8
    assert u.reg_read(UC_X86_REG_ECX) == MAP
    assert len(queries) == 50
    assert sum(row['kind'] == 'engineer_cell' for row in trace) == 51
    expected_queries = [(x, y, field) for y in range(8, 13) for x in range(8, 13) for field in ('tile', 'overlay')]
    assert [(q['cell'][0], q['cell'][1], q['field']) for q in queries] == expected_queries
    return dict(input=case, output=dict(family='low' if end == LOW else 'high',
                repair_entry=f'{end:08X}', repair_argument=xy(read32(sp-4)),
                low_latch=selected_latch[0], trace=trace,
                dummy_cell=xy(DUMMY+0x24)))

def inputs():
    rows = [dict(name='no_member', wood_base=200, cells=[])]
    for tile in (199, 200, 215, 216):
        rows.append(dict(name=f'tile_{tile}', wood_base=200, cells=[[10, 10, tile, -1]]))
    for overlay in (73, 74, 101, 102):
        rows.append(dict(name=f'overlay_{overlay}', wood_base=200, cells=[[10, 10, 65535, overlay]]))
    for left, right, name in [((200, -1), (216, 205), 'wood_then_high'),
                              ((216, 205), (200, -1), 'high_then_wood'),
                              ((65535, 74), (216, 205), 'low_overlay_then_high'),
                              ((216, 205), (65535, 101), 'high_then_low_overlay')]:
        rows.append(dict(name=name, wood_base=200, cells=[[8, 8, *left], [12, 12, *right]]))
    rows.append(dict(name='missing_last_query', wood_base=200, cells=[[8, 8, 200, -1]], missing=[12, 12]))
    return rows

def generate():
    return [execute(case) for case in inputs()]

if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=__doc__,
        entry_points={'selector': BEGIN, 'engineer_cell': 0x41BEA0, 'map_lookup': 0x5657A0,
                      'low_boundary_not_executed': LOW, 'high_boundary_not_executed': HIGH},
        assumptions=['Already-admitted interior519C07 frame; ESI is Engineer and EDI is retained hut.',
                     'Physical Engineer XYZ=(2688,2688,1040), original Infantry vtable and supplied sparse CellClass table.',
                     'Wood base200 is a supplied theater input; this is not map/theater loading.'],
        substitutions=['No native call answers or code patches; observational hooks only. Execution ends before either repair entry.'],
    ))
