"""Original57F200/57F440 and all four ordinary repair walkers.

Runs original selection, stores, continuation and5868A0 rectangle enumeration.
Recalc,487A10,connectivity,586990 and display are declared callbacks; supplied
RandomRanged results isolate the walker (the MapGen stream has its own corpus).
This witness does not establish the live callbacks or whole engineer behavior.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_repair import OriginalRepair, KEYS
from tools.spatial_oracle.bridge_rim import COORD, DUMMY, CELLS
from tools.spatial_oracle.map_queries import dwords, packed


class OrdinaryRepair(OriginalRepair):
    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        args = struct.unpack('<5I', u.mem_read(sp + 4, 20))
        if address in (0x57F200, 0x57F440):
            return  # Execute the actual selector and walker.
        if address in (0x47FDE0, 0x47FB90):
            # Display extents do not select spatial writes. Zero extents keep
            # the original pure clipping/union helpers independent of art.
            u.mem_write(args[0], dwords(0, 0, 0, 0))
            self.ret(4, args[0])
            return
        if address == 0x598030:
            value = self.case.get('variant', 0)
            self.event('random', minimum=u.reg_read(UC_X86_REG_ECX),
                       maximum=3, result=value)
            self.ret(eax=value)
            return
        if address == 0x487A10:
            self.event('occupants', coord=self.coord(u.reg_read(UC_X86_REG_ECX)), mode=args[0])
            self.ret(4)
            return
        super().observe(u, address, size, data)

    def write(self, u, access, address, size, value, data):
        p = (CELLS + (address - CELLS) // 0x200 * 0x200
             if CELLS <= address < CELLS + len(self.ptrs) * 0x200 else DUMMY)
        if address == p + 0x44:
            self.trace.append(dict(kind='overlay', coord=self.coord(p), overlay=value))
        super().write(u, access, address, size, value, data)

    def run(self):
        self.trace.clear()
        self.uc.mem_write(COORD, packed(*self.case['start']))
        self.call(0x57F200 if self.case['family'] == 'low' else 0x57F440, args=(COORD,))
        final = [[*coord, struct.unpack('<i', self.uc.mem_read(p + 0x44, 4))[0]]
                 for coord, p in self.ptrs.items()]
        return dict(trace=self.trace, final=final,
                    dummy=[*self.coord(DUMMY), struct.unpack('<i', self.uc.mem_read(DUMMY + 0x44, 4))[0]])


def inputs():
    for family, first, last in [('low', 74, 101), ('high', 205, 232)]:
        for overlay in range(first - 1, last + 2):
            ns = (74 <= overlay <= 82 or 92 <= overlay <= 95 or overlay == 100
                  if family == 'low' else
                  205 <= overlay <= 213 or 223 <= overlay <= 226 or overlay == 231)
            cells = [[100 + x, 100 + y, 0xffff, 0, 0, overlay, 0, None, 0, 0]
                     for x in range(-2, 3) for y in range(-2, 3)
                     if (y in (-1, 0, 1) and x in (0, 1) if ns else
                         x in (-1, 0, 1) and y in (0, 1))]
            yield dict(name=f'{family}_{overlay}', family=family, start=[100, 100],
                       size=[136, 140], bridge_base=100, wood_base=200, rim_keys=KEYS,
                       cells=cells, variant=overlay % 4)


def cases():
    return dict(cases=[dict(input=case, result=OrdinaryRepair(case).run()) for case in inputs()])


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original ordinary bridge repair selection, scalar writes and callback order',
        assumptions=['Synthetic sparse strips cover every family overlay and rejected sentinels',
                     'Successful bounded heap; native rectangle vector enumeration executes'],
        substitutions=['47D2B0,487A10,56C510 and586990 record calls and return',
                       '598030 supplies variant0..3; MapGen RNG implementation has a separate corpus',
                       '47FDE0/47FB90 return zero display rectangles; radar/display sinks',
                       '7C8E17/7C8B3D supply successful allocation/free'],
        entry_points={'low': 0x57F200, 'high': 0x57F440, 'rectangle': 0x5868A0}))
