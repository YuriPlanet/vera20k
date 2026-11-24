"""Original bridge ramp/span repair control with explicitly substituted callbacks.

Executes High573540/568E40 and Low570050/569760,56EB80,56E990 and5471F0.
Synthetic sparse cells/TMP headers isolate caller ordering; constructor, Recalc,
ValidateBridgeZones, connectivity and trailing rebuild are declared host seams.
This does not establish their production delivery or whole engineer repair.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_pavement import OriginalPavement, synthetic_tmp
from tools.spatial_oracle.bridge_rim import MAP, COORD, DUMMY, CELLS
from tools.spatial_oracle.map_queries import dwords, packed

KEYS = dict(BridgeTopLeft1=1, BridgeTopLeft2=2, BridgeBottomRight1=3,
            BridgeBottomRight2=3, BridgeTopRight1=4, BridgeTopRight2=5,
            BridgeBottomLeft1=6, BridgeBottomLeft2=6, BridgeMiddle1=7, BridgeMiddle2=12)


class OriginalRepair(OriginalPavement):
    def __init__(self, case):
        self.case = case
        self.heap = 0x45000000
        self.ordinals = {}
        heads = {tile: synthetic_tmp([True] * 16)
                 for tile in {row[2] for row in case['cells']} if tile not in (0xffff, 0xff)}
        super().__init__(case, heads)
        self.uc.mem_map(self.heap, 0x100000)
        self.uc.mem_write(0xABAD1C, dwords(case['wood_base']))
        self.uc.mem_write(MAP + 0x124, dwords(*case.get('search_rect', [0, 0, 511, 511])))
        self.uc.mem_write(0xA83D84, dwords(0x450F0000))
        for overlay in (24, 25, 237, 238):
            self.uc.mem_write(0x450F0000 + overlay * 4, dwords(0x450F1000 + overlay * 16))

    def ret(self, cleanup=0, eax=0):
        u = self.uc
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, eax)
        u.reg_write(UC_X86_REG_EIP, struct.unpack('<I', u.mem_read(sp, 4))[0])
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def event(self, kind, **fields):
        self.trace.append(dict(kind=kind, **fields))
        ordinal = self.ordinals.get(kind, 0) + 1
        self.ordinals[kind] = ordinal
        for callback in self.case.get('callbacks', []):
            if callback['kind'] == kind and callback['ordinal'] == ordinal:
                for change in callback['writes']:
                    target = DUMMY if change['target'] == 'dummy' else self.ptrs[tuple(change['target'])]
                    if 'coord' in change:
                        self.uc.mem_write(target + 0x24, packed(*change['coord']))
                    if 'tile' in change:
                        self.uc.mem_write(target + 0x38, dwords(change['tile']))
                    self.trace.append(dict(kind='callback_write', change=change))

    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        args = struct.unpack('<5I', u.mem_read(sp + 4, 20))
        if address == 0x7C8E17:
            p = self.heap
            self.heap += (args[0] + 15) & ~15
            assert self.heap < 0x450E0000
            self.ret(eax=p)
            return
        if address == 0x7C8B3D:
            self.ret()
            return
        if address == 0x47D2B0:
            self.event('recalc', coord=self.coord(u.reg_read(UC_X86_REG_ECX)),
                       level=struct.unpack('<i', dwords(args[0]))[0])
            self.ret(4)
            return
        if address == 0x56DB70:
            point = list(struct.unpack('<hh', u.mem_read(args[0], 4)))
            ordinal = self.ordinals.get('validate', 0)
            returns = self.case.get('validate_returns', [])
            result = returns[ordinal] if ordinal < len(returns) else False
            self.event('validate', coord=point, result=result)
            self.ret(4, int(result))
            return
        if address == 0x5FC380:
            overlay = (args[0] - 0x450F1000) // 16
            self.event('construct', coord=list(struct.unpack('<hh', u.mem_read(args[1], 4))),
                       overlay=overlay, frame=struct.unpack('<i', dwords(args[2]))[0])
            self.ret(12, u.reg_read(UC_X86_REG_ECX))
            return
        if address == 0x56C510:
            self.event('connectivity')
            self.ret()
            return
        if address == 0x586990:
            pointer, count = struct.unpack('<I', u.mem_read(args[0] + 4, 4))[0], struct.unpack('<I', u.mem_read(args[0] + 16, 4))[0]
            cells = [list(struct.unpack('<hh', u.mem_read(pointer + i * 4, 4))) for i in range(count)]
            self.event('rebuild', cells=cells)
            self.ret(4)
            return
        if address in (0x57F200, 0x57F440):
            self.event('ordinary', family='low' if address == 0x57F200 else 'high',
                       coord=list(struct.unpack('<hh', u.mem_read(args[0], 4))))
            self.ret(4)
            return
        if address in (0x568E40, 0x569760):
            self.event('span_entry', family='high' if address == 0x568E40 else 'low',
                       coord=self.coord(args[0]), direction=args[1])
        elif address in (0x573540, 0x570050):
            self.event('repair_entry', family='high' if address == 0x573540 else 'low',
                       coord=list(struct.unpack('<hh', u.mem_read(args[0], 4))))
        elif address == 0x56EB80 and args[4] == 0:
            self.event('replace', coord=list(struct.unpack('<hh', u.mem_read(args[0], 4))),
                       tile=struct.unpack('<i', dwords(args[1]))[0],
                       level=struct.unpack('<i', dwords(args[3]))[0], recursive=args[4])
        elif address == 0x56E990 and args[2] == 0:
            self.event('pavement', coord=list(struct.unpack('<hh', u.mem_read(args[0], 4))),
                       state=args[1], recursive=args[2])
        super().observe(u, address, size, data)

    def write(self, u, access, address, size, value, data):
        cell = (CELLS + (address - CELLS) // 0x200 * 0x200
                if CELLS <= address < CELLS + len(self.ptrs) * 0x200 else DUMMY)
        if address == cell + 0x38:
            self.trace.append(dict(kind='tile', coord=self.coord(cell), tile=value))
        elif address == cell + 0x11B:
            self.trace.append(dict(kind='level', coord=self.coord(cell), level=value))
        super().write(u, access, address, size, value, data)

    def run(self):
        self.trace.clear()
        start = tuple(self.case['start'])
        high = self.case['family'] == 'high'
        if self.case['entry'] == 'span':
            p = self.ptrs.get(start, DUMMY)
            if p == DUMMY:
                self.uc.mem_write(DUMMY + 0x24, packed(*start))
            self.call(0x568E40 if high else 0x569760, args=(p, self.case['direction'], 0))
        else:
            self.uc.mem_write(COORD, packed(*start))
            self.call(0x573540 if high else 0x570050, args=(COORD,))
        final = [[*coord, struct.unpack('<i', self.uc.mem_read(p + 0x38, 4))[0],
                  struct.unpack('<I', self.uc.mem_read(p + 0x140, 4))[0],
                  self.uc.mem_read(p + 0x11B, 1)[0]] for coord, p in self.ptrs.items()]
        return dict(trace=self.trace, final=final,
                    dummy=[*self.coord(DUMMY), struct.unpack('<i', self.uc.mem_read(DUMMY + 0x38, 4))[0],
                           struct.unpack('<I', self.uc.mem_read(DUMMY + 0x140, 4))[0],
                           self.uc.mem_read(DUMMY + 0x11B, 1)[0]])


def supplied(name, family, direction, rows, start=(100, 100), entry='span', **kwargs):
    return dict(name=name, family=family, direction=direction, cells=rows, start=start,
                entry=entry, size=[136, 140], bridge_base=100, wood_base=500,
                rim_keys=KEYS, **kwargs)


def row(x, y, tile=0, sub=0, flags=0, level=0, overlay=None):
    return [x, y, tile, sub, flags, overlay, 0, None, level, 0]


def control_inputs():
    for family, base in [('high', 100), ('low', 500)]:
        for direction in (2, 4):
            ns = direction == 2
            a, b, middle = (base, base + 2, base + 6) if ns else (base + 3, base + 5, base + 11)
            for distance in (1, 29, 30):
                rows = [row(100, 100, a, 8 if ns else 12, 0x2000)]
                rows += [row(100 + i * ns, 100 + i * (not ns)) for i in range(1, distance)]
                rows += [row(100 + distance * ns, 100 + distance * (not ns), b, 4 if ns else 2, 0x2000)]
                yield supplied(f'{family}_{direction}_distance_{distance}', family, direction, rows, validate_returns=[True])
            for variant in range(5):
                rows = [row(100, 100, a, 8 if ns else 12, 0x2000)]
                if ns:
                    rows += [row(101 + x, 98 + y, middle + variant, y * 2 + x, level=254)
                             for y in range(5) for x in range(2)]
                    rows += [row(103, 100, b, 4, 0x2000)]
                else:
                    rows += [row(98 + x, 101 + y, middle + variant, y * 5 + x, level=254)
                             for y in range(2) for x in range(5)]
                    rows += [row(100, 103, b, 2, 0x2000)]
                yield supplied(f'{family}_{direction}_middle_{variant}', family, direction, rows, validate_returns=[True, False])
                if variant == 4:
                    partial = [r for r in rows if r[2] != b]
                    partial += [row(100 + i * ns, 100 + i * (not ns)) for i in range(3, 30)]
                    partial += [row(100 + 30 * ns, 100 + 30 * (not ns), b, 4 if ns else 2, 0x2000)]
                    yield supplied(f'{family}_{direction}_abort_after_middle_writes', family, direction, partial, validate_returns=[True])
                    center = [101, 100] if ns else [100, 101]
                    yield supplied(f'{family}_{direction}_captured_old4_after_callback', family, direction, rows,
                                   callbacks=[dict(kind='validate', ordinal=1, writes=[dict(target=center, tile=777)])])
                if variant == 0:
                    following = [101, 100] if ns else [100, 101]
                    yield supplied(f'{family}_{direction}_construction_reads_live_next', family, direction, rows,
                                   callbacks=[dict(kind='construct', ordinal=1, writes=[dict(target=following, tile=777)])])
            yield supplied(f'{family}_{direction}_retained_dummy_start', family, direction,
                           [row(101 if ns else 100, 100 if ns else 101, b, 4 if ns else 2, 0x2000)])
            rows = [row(100, 100, a, 8 if ns else 12, 0x2180 if ns else 0x2980),
                    row(101 if ns else 100, 100 if ns else 101, b, 4 if ns else 2, 0x2000)]
            yield supplied(f'{family}_{direction}_ramp_entry', family, direction, rows, entry='repair', validate_returns=[True])
            # Same live endpoint reached through the native three-step ray,
            # and through a structural cell's literal +2C anchor pointer.
            ray_start = (100, 103) if ns else (103, 100)
            yield supplied(f'{family}_{direction}_ray_three_steps', family, direction, rows,
                           start=ray_start, entry='repair')
            member = row(*ray_start, flags=0x100 if ns else 0x900)
            member[7] = [100, 100]
            yield supplied(f'{family}_{direction}_literal_anchor', family, direction, rows + [member],
                           start=ray_start, entry='repair')
            for variant in range(5):
                if ns:
                    recursive = [row(98, 100, a, 8, 0x2180), row(101, 100, b, 4, 0x2000)]
                    recursive += [row(99 + x, 98 + y, middle + variant, y * 2 + x,
                                      flags=0x180 if (x, y) == (1, 2) else 0, level=254)
                                  for y in range(5) for x in range(2)]
                else:
                    recursive = [row(100, 98, a, 12, 0x2980), row(100, 101, b, 2, 0x2000)]
                    recursive += [row(98 + x, 99 + y, middle + variant, y * 5 + x,
                                      flags=0x980 if (x, y) == (2, 1) else 0, level=254)
                                  for y in range(2) for x in range(5)]
                yield supplied(f'{family}_{direction}_recursive_middle_{variant}', family, direction,
                               recursive, entry='repair', validate_returns=[True, False, True])
        overlay = 0xE7 if family == 'high' else 0x64
        yield supplied(f'{family}_x_major_overlay_scan', family, 2,
                       [row(98, 101, overlay=overlay), row(101, 98, overlay=overlay)], entry='repair')
        yield supplied(f'{family}_negative_scan_stride_alias', family, 2,
                       [row(510, 97, overlay=overlay)], start=(0, 100), entry='repair')
        rows = [row(101, 100, flags=0x400), row(102, 100, flags=0x400), row(103, 100),
                row(101, 98, base, 8, 0x2000), row(102, 98, base + 2, 4, 0x2000)]
        yield supplied(f'{family}_pure_destroyed_anchor_search', family, 2,
                       rows, start=(101, 100), entry='repair')
        rows = [row(x, 100, flags=0x400) for x in range(101, 106)]
        yield supplied(f'{family}_four_destroyed_steps_abort', family, 2,
                       rows, start=(101, 100), entry='repair')


def cases():
    outputs = []
    for case in control_inputs():
        native = OriginalRepair(case)
        outputs.append(dict(input=case, output=native.run()))
    return dict(cases=outputs)


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original high and low ramp/span repair caller control over synthetic supplied cells',
        assumptions=[
            'Native49F2F0 initializes direction offsets; concretebase100 and woodbase500 distinguish family dispatch',
            'Theater relative keys use stock urbannmd values; synthetic16-entry TMP heads admit the pavement predicate',
            'No resource exhaustion; successful bounded synthetic heap supplied, native vector growth/copy executes',
            'No native loading, object lifecycle, real bridge zone graph, complete engineer order or rendered parity claim',
        ], substitutions=[
            '47D2B0 records receiver/level and returns; tile metadata recalculation excluded',
            '56DB70 records coordinate and returns a supplied Boolean; real zone records and edge mutation excluded',
            '5FC380 records overlay type, coordinate and frame then returns; constructor registration, terrain gate, Mark, Recalc and UnInit excluded',
            '56C510 and586990 record ordered calls/list then return; connectivity and batched terrain/zone work excluded',
            '57F200/57F440 record first-match ordinary repair dispatch; ordinary strip walker excluded',
            '7C8E17/7C8B3D provide successful allocation/free; native vector writes/growth execute',
            '6551C0 radar and6D2790 display sinks;6D2140 supplies point0,0; pixels and rectangle values excluded',
        ], entry_points={'high_entry':0x573540,'low_entry':0x570050,'high_span':0x568E40,'low_span':0x569760,
                         'flood':0x56EB80,'pavement':0x56E990,'damaged_gate':0x5471F0}))
