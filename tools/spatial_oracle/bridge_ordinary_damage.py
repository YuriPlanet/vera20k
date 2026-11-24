"""Original concrete ground-bridge selector57CCF0, root walkers and propagation.

Synthetic overlay strips isolate identity/store/callback order. Reuses the
ordinary-bridge fixture and declared Recalc/occupant/navigation/display seams.
Physical map, actual occupants and ordinary firing have separate joined witnesses.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_ordinary_repair import OrdinaryRepair, KEYS
from tools.spatial_oracle.bridge_rim import COORD, DUMMY
from tools.spatial_oracle.map_queries import packed


class OrdinaryDamage(OrdinaryRepair):
    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x575EE0:
            self.event('notify', endpoints=[list(struct.unpack('<hh', u.mem_read(sp + n, 4)))
                                           for n in (4, 8)])
        if address == 0x487A10:
            self.event('occupants', coord=self.coord(u.reg_read(UC_X86_REG_ECX)),
                       mode=u.mem_read(sp + 4, 1)[0])
            self.ret(4)
            return
        super().observe(u, address, size, data)

    def run(self):
        self.trace.clear()
        self.uc.mem_write(COORD, packed(*self.case['start']))
        returned = self.call(self.case.get('entry', 0x57CCF0), args=(COORD,))
        final = [[*coord, struct.unpack('<i', self.uc.mem_read(p + 0x44, 4))[0]]
                 for coord, p in self.ptrs.items()]
        return dict(returned=returned & 255, trace=self.trace, final=final,
                    dummy=[*self.coord(DUMMY), struct.unpack('<i', self.uc.mem_read(DUMMY + 0x44, 4))[0]])


def inputs():
    for overlay in range(204, 234):
        ns = 205 <= overlay <= 213 or 223 <= overlay <= 226 or overlay == 231
        for across in (-1, 0, 1):
            cells = [[100 + x, 100 + y, 0xffff, 0, 0, overlay, 0, None, 0, 0]
                     for x in range(-2 if ns else -1, 3 if ns else 2)
                     for y in range(-1 if ns else -2, 2 if ns else 3)]
            yield dict(name=f'concrete_{overlay}_across_{across}', family='high',
                       start=[100 if ns else 100 + across, 100 + across if ns else 100],
                       size=[136, 140], bridge_base=100, wood_base=500,
                       rim_keys=KEYS, cells=cells)


def generate():
    return dict(cases=[dict(input=case, result=OrdinaryDamage(case).run()) for case in inputs()])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=__doc__, assumptions=[
            'Synthetic finite strips, every concrete family overlay plus adjacent rejected sentinels, all3 input width positions',
            'Original endpoint scans and untagged575EE0 traversal execute; no CellTags supplied',
            'Original rectangle5868A0 enumeration; successful bounded allocation',
        ], substitutions=[
            '47D2B0/487A10/56C510/586990 record calls and return; no live objects or graph proof here',
            '47FDE0/47FB90 zero display rectangles and6551C0/6D2790 sinks',
        ], entry_points={'selector':0x57CCF0, 'ns_root':0x57CF60, 'ew_root':0x57D530,
                         'ns_propagation':0x57E7A0, 'ew_propagation':0x57ED00}))
