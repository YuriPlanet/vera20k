"""Execute full original587410 over supplied CellClass/BridgeRecord inputs.

Complements the physical Anytown cursor packet; does not substitute query,
lookup, record search or packed-coordinate arithmetic. No native loader/UI.
"""
from pathlib import Path
import hashlib
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance, image_bytes, file_span
from tools.spatial_oracle.bridge_rim import OriginalRim, MAP, COORD, DUMMY, GLOBALS
from tools.spatial_oracle.map_queries import dwords, packed

TYPES, TYPE, RECORDS = 0x40100000, 0x40150000, 0x40160000


def cell(x, y, tile=0, sub=0, overlay=None):
    return [x, y, tile, sub, 0, overlay, 0, None, 0, 0]


def fixture(name, cells=(), records=(), center=(100, 100), width=3,
            concrete=100, wood=200, dummy_overlay=-1):
    return dict(name=name, cells=list(cells), records=list(records), center=list(center),
                width=width, concrete=concrete, wood=wood, dummy_overlay=dummy_overlay)


def cases():
    out = [fixture('empty')]
    for base, terminal in [(74, 100), (205, 231)]:
        for axis, seed_overlay in [('x', base), ('y', base + 9)]:
            for sign in [-1, 1]:
                seed = [102, 102]
                rows = [cell(*seed, overlay=seed_overlay)]
                for step in [1, 2, 3]:
                    x, y = seed
                    if axis == 'x': x += sign * step
                    else: y += sign * step
                    rows.append(cell(x, y, overlay=terminal if step == 3 else seed_overlay))
                out.append(fixture(f'overlay_{base}_{axis}_{sign}', rows))
            out.append(fixture(f'overlay_{base}_{axis}_intact', [cell(102, 102, overlay=seed_overlay)]))
        for value in range(base, base + 28):
            out.append(fixture(f'overlay_single_{value}', [cell(102, 102, overlay=value)]))
    x_offsets = struct.unpack('<16h', file_span(image_bytes(), 0x82AA04, 32)[1])
    y_offsets = struct.unpack('<16h', file_span(image_bytes(), 0x82AA24, 32)[1])
    directions = struct.unpack('<16i', file_span(image_bytes(), 0x82AA44, 64)[1])
    # Expected query decisions come only from original instructions. These
    # tables generate supplied endpoints; every raw table is retained below.
    for family, base in [('concrete', 100), ('wood', 200)]:
        for offset in range(16):
            x, y = 101 + x_offsets[offset], 101 + y_offsets[offset]
            rows = [cell(102, 102, base + offset, 4, 231)]
            far = [x, y + 3] if directions[offset] in [0,4] else [x + 3, y]
            for active in [False, True]:
                out.append(fixture(f'{family}_offset_{offset}_active_{int(active)}', rows,
                    [[[x, y], far, active, 0]]))
    for offset, direction in enumerate(directions[:6]):
        x, y = 101 + x_offsets[offset], 101 + y_offsets[offset]
        vertical = direction in [0, 4]
        a, b = ([x, y - 3], [x, y]) if vertical else ([x - 3, y], [x, y])
        if direction in [2, 4]:
            a, b = ([x, y], [x, y + 3]) if vertical else ([x, y], [x + 3, y])
        rows = [cell(102, 102, 100 + offset, 4)]
        out.append(fixture(f'endpoint_walk_{offset}', rows, [[a, b, True, 0]]))
        out.append(fixture(f'wrong_kind_{offset}', rows, [[a, b, False, 1]]))
        mid_a, mid_b = ([x, y - 1], [x, y + 1]) if vertical else ([x - 1, y], [x + 1, y])
        out.append(fixture(f'interior_active_{offset}', rows, [[mid_a, mid_b, True, 0]]))
        dx, dy = {0:(0,-1),2:(1,0),4:(0,1),6:(-1,0)}[direction]
        target = [b[0]+dx,b[1]+dy] if direction in [2,4] else [a[0]+dx,a[1]+dy]
        out.append(fixture(f'chained_inactive_{offset}', rows,
            [[a,b,True,0],[target,target,False,0]]))
    out.extend([
        fixture('tile_overrides_own_collapsed_overlay', [cell(102,102,106,4,231)]),
        fixture('later_tile_overrides_collapsed_overlay', [cell(100,100,overlay=231),cell(102,102,106,4)]),
        fixture('later_overlay_overrides_tile', [cell(100,100,106,4),cell(102,102,overlay=231)]),
        fixture('wood_wins_overlapping_windows', [cell(102,102,102,4)],
                [[[102,103],[105,103],False,0]], wood=102),
        fixture('dummy_seed_moves_during_scan', center=(0,0), dummy_overlay=231),
        fixture('fixed_stride_alias_scan', [cell(511,99,overlay=231)], center=(0,100)),
    ])
    for width in [1,2,3,4,255]:
        for sub in [0,1,4,7,255]:
            origin = (102-sub % width,102-sub // width)
            point = [origin[0]+1,origin[1]+2]
            out.append(fixture(f'width_{width}_sub_{sub}',[cell(102,102,100,sub)],
                               [[point,point,False,0]],width=width))
    return out


class Query(OriginalRim):
    def __init__(self, case):
        supplied = dict(bridge_base=case['concrete'], rim_keys={k:None for k in GLOBALS},
                        size=[136,140],cells=case['cells'])
        self.capture = False
        super().__init__(supplied)
        u=self.uc
        u.mem_write(0xABAD1C,dwords(case['wood']))
        u.mem_write(DUMMY+0x38,dwords(0xFFFF))
        u.mem_write(DUMMY+0x44,dwords(case['dummy_overlay']))
        u.mem_write(0xA8ED2C,dwords(TYPES))
        u.mem_write(TYPES,b''.join(dwords(TYPE) for _ in range(65536)))
        u.mem_write(TYPE+0x2E4,dwords(case['width']))
        u.mem_write(MAP+0x54,dwords(RECORDS))
        u.mem_write(MAP+0x60,dwords(len(case['records'])))
        for i,(a,b,active,kind) in enumerate(case['records']):
            u.mem_write(RECORDS+i*16,packed(*a)+packed(*b)+dwords(int(active),kind))
        self.trace=[]
        self.capture=True

    def observe(self,u,address,size,data):
        if self.capture:
            sp=u.reg_read(UC_X86_REG_ESP)
            if address == 0x5657A0:
                p=struct.unpack('<I',u.mem_read(sp+4,4))[0]
                self.trace.append(dict(kind='lookup',coord=list(struct.unpack('<hh',u.mem_read(p,4)))))
            elif address == 0x56DA10:
                p,tolerance,start=struct.unpack('<III',u.mem_read(sp+4,12))
                self.trace.append(dict(kind='record_search',coord=list(struct.unpack('<hh',u.mem_read(p,4))),
                                       tolerance=tolerance,start=start))
            elif address == 0x5876BA:
                p=u.reg_read(UC_X86_REG_EBX)
                self.trace.append(dict(kind='selection',coord=self.coord(p) if p else None,
                    tile_family=u.mem_read(sp+0x12,1)[0],concrete=u.mem_read(sp+0x13,1)[0]))
        super().observe(u,address,size,data)


def generate():
    out=[]
    for case in cases():
        native=Query(case);u=native.uc
        before=bytes(u.mem_read(0x401000,0x3E0000))
        cells_before=[bytes(u.mem_read(p,0x200)) for p in native.ptrs.values()]
        records_before=bytes(u.mem_read(RECORDS,len(case['records'])*16))
        u.mem_write(COORD,packed(*case['center']))
        result=native.call(0x587410,args=(COORD,),count=100000)
        assert before==bytes(u.mem_read(0x401000,0x3E0000))
        assert cells_before==[bytes(u.mem_read(p,0x200)) for p in native.ptrs.values()]
        assert records_before==bytes(u.mem_read(RECORDS,len(case['records'])*16))
        assert not native.events and not native.writes
        out.append(dict(input=case,result_al=result&255,trace=native.trace,
                        dummy_coord=native.coord(DUMMY),cells_and_records_unchanged=True))
    return dict(cases=out,tables={hex(a):file_span(image_bytes(),a,n)[1].hex()
                                for a,n in [(0x82AA04,32),(0x82AA24,32),(0x82AA44,64)]},
                body_sha256=hashlib.sha256(file_span(image_bytes(),0x587410,0x837)[1]).hexdigest())


if __name__ == '__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope=__doc__,assumptions=[
            'Supplied sparse CellClass table, pristine tile widths and ordered BridgeRecords; not native construction or retail-map reachability.',
            'Original direction initializer49F2F0 executes before queries; template widths and unsigned subtiles are supplied.',
            'Original complete587410,5657A0,56DA10 and42D510 execute without patched bytes; cell/record memory and full mapped.text remain unchanged.',
            'Dummy coordinate mutations and original lookup/record-search order are retained; the Boolean contract is AL.'],
        substitutions=['Inherited OriginalRim output sinks exist but no query reaches them; any sink event rejects the case.',
                       'No UI/Engineer/class lifecycle or full Scenario initialization. No query/lookup/record result is substituted.'],
        entry_points={'repair_query':0x587410,'record_search':0x56DA10,'lookup':0x5657A0,'coord_add':0x42D510}),
        source_paths={'query':Path(__file__), 'native_oracle':Path(__file__).parents[1]/'native_oracle.py',
                      'map_queries':Path(__file__).with_name('map_queries.py'),
                      'bridge_rim':Path(__file__).with_name('bridge_rim.py')})
