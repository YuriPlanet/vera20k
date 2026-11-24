"""Original56EB80 control flow with declared Recalc/radar/screen callouts.

This isolates recursive ordering, live membership, and fixed-stride aliases.
Recalc behavior is excluded; terrain_recalc separately executes that body.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_rim import OriginalRim, GLOBALS, CELLS, COORD, DUMMY
from tools.spatial_oracle.map_queries import dwords, packed


class OriginalFlood(OriginalRim):
    def __init__(self, case):
        self.trace = []
        self.recalc_count = 0
        self.callback = case.get('first_recalc_write')
        super().__init__(case)

    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x47D2B0:
            cell = u.reg_read(UC_X86_REG_ECX)
            level = struct.unpack('<i', u.mem_read(sp + 4, 4))[0]
            self.trace.append(dict(kind='recalc', coord=self.coord(cell), level=level))
            self.recalc_count += 1
            if self.recalc_count == 1 and self.callback:
                x, y, tile = self.callback
                u.mem_write(self.ptrs[x, y] + 0x38, dwords(tile))
                self.trace.append(dict(kind='callback_write', coord=[x, y], tile=tile))
            u.reg_write(UC_X86_REG_EIP, struct.unpack('<I', u.mem_read(sp, 4))[0])
            u.reg_write(UC_X86_REG_ESP, sp + 8)
            return
        if address == 0x6551C0:
            p = struct.unpack('<I', u.mem_read(sp + 4, 4))[0]
            self.trace.append(dict(kind='radar', coord=list(struct.unpack('<hh', u.mem_read(p, 4)))))
        elif address == 0x6D2790:
            self.trace.append(dict(kind='screen'))
        super().observe(u, address, size, data)

    def write(self, u, access, address, size, value, data):
        if CELLS <= address < CELLS + len(self.ptrs) * 0x200:
            cell = CELLS + (address - CELLS) // 0x200 * 0x200
        else:
            cell = DUMMY
        if address == cell + 0x38:
            self.trace.append(dict(kind='tile', coord=self.coord(cell), tile=value))


def cases():
    result = []
    for name, rows, start, callback in [
        ('same_tile_still_dirties_screen', [(100, 100, 101)], (100, 100), None),
        ('connected_depth_first', [(100, 100, 100), (100, 99, 100),
                                  (101, 99, 100), (101, 100, 100),
                                  (105, 105, 100)], (100, 100), None),
        ('membership_after_recalc_callback', [(100, 100, 100), (100, 99, 100),
                                              (101, 99, 100)], (100, 100), (100, 99, 200)),
        ('requested_coordinates_across_stride_alias', [(511, 100, 100), (0, 101, 100),
                                                       (1, 101, 100)], (511, 100), None),
        ('retained_dummy_with_real_neighbors', [(100, 99, -1), (101, 99, -1)], (100, 100), None),
    ]:
        supplied = dict(bridge_base=0, rim_keys={key: None for key in GLOBALS},
                        size=[136, 140],
                        cells=[[x, y, tile, 0, 0, None, 0, None, 4, 0] for x, y, tile in rows],
                        first_recalc_write=callback)
        native = OriginalFlood(supplied)
        native.uc.mem_write(COORD, packed(*start))
        native.call(0x56EB80, args=(COORD, 101, 0, 2, 0))
        final = [[*coord, struct.unpack('<i', native.uc.mem_read(p + 0x38, 4))[0]]
                 for coord, p in native.ptrs.items()]
        dummy = [*native.coord(DUMMY), struct.unpack('<i', native.uc.mem_read(DUMMY + 0x38, 4))[0]]
        result.append(dict(name=name, cells=rows, start=start, replacement=101, level=2,
                           first_recalc_write=callback, trace=native.trace, final=final, dummy=dummy))
    return dict(cases=result)


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Five original56EB80 synthetic control witnesses; ordered scalar writes and callback boundaries',
        assumptions=[
            'Supplied sparse native512-stride cell plane; dummy tile=-1; native49F2F0 initializes directions',
            'Normal first entry with recursion flag0; replacement101, level override2',
            'Synthetic membership callback changes one neighbor at the first Recalc boundary; no stock callback reachability claim',
            'Fixed-stride alias case distinguishes requested coordinates from retained Cell+24',
            'No complete Recalc, stock bridge caller, loader, object damage or display equivalence claim',
        ], substitutions=[
            '47D2B0 records the receiver and level argument; optional declared host mutation then immediate return',
            '6551C0 records radar coordinate then returns; no radar queue',
            '6D2140 supplies screen point0,0 and6D2790 records screen call; no projection or display output',
        ], entry_points={'flood': 0x56EB80, 'directions': 0x49F2F0}))
