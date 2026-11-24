"""Original bridgehead branches of 576BA0 (concrete) and 571490 (wooden).

A synthetic BridgeMiddle1 (2x5, NS) and BridgeMiddle2 (5x2, EW) template is
laid into the stock xbayopigs crop at its native subtile layout. Each case
runs the driver once from one input cell. BlowUpBridge, the perpendicular
helpers, FloodFillIsoTileType, the rim, 56DAE0 and 586990 are sinks; the
driver's own walk, branch selection and 0x42FCB0 vector construction execute.
"""
import copy
import hashlib
import json
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_body_publication import OriginalBody
from tools.spatial_oracle.bridge_rim import COORD
from tools.spatial_oracle.map_queries import dwords, packed

HELPERS = {
    0x572230: ('high', 'ns_damage_a'), 0x572330: ('high', 'ns_damage_b'),
    0x572440: ('high', 'ns_collapse_a'), 0x5727E0: ('high', 'ns_collapse_b'),
    0x572B80: ('high', 'ew_damage_a'), 0x572C90: ('high', 'ew_damage_b'),
    0x572DA0: ('high', 'ew_collapse_a'), 0x573170: ('high', 'ew_collapse_b'),
    0x56ED40: ('low', 'ns_damage_a'), 0x56EE40: ('low', 'ns_damage_b'),
    0x56EF50: ('low', 'ns_collapse_a'), 0x56F2F0: ('low', 'ns_collapse_b'),
    0x56F690: ('low', 'ew_damage_a'), 0x56F7A0: ('low', 'ew_damage_b'),
    0x56F8B0: ('low', 'ew_collapse_a'), 0x56FC80: ('low', 'ew_collapse_b'),
}
RIMS = {0x576770: 'high', 0x571050: 'low'}
DRIVERS = {'high': 0x576BA0, 'low': 0x571490}
HEAP = 0x45000000
# Template origins inside the crop, clear of the stock span.
NS_ORIGIN, EW_ORIGIN = (108, 136), (108, 131)


def template(case, kind, variant):
    """Overwrite crop cells with one middle template of the given variant."""
    keys = case['rim_keys']
    base = case['bridge_base']
    cells = {(row[0], row[1]): row for row in case['cells']}
    if kind == 'ns':
        tile = base + keys['BridgeMiddle1'] - 1 + variant
        layout = [((NS_ORIGIN[0] + sub % 2, NS_ORIGIN[1] + sub // 2), sub) for sub in range(10)]
    else:
        tile = base + keys['BridgeMiddle2'] - 1 + variant
        layout = [((EW_ORIGIN[0] + sub % 5, EW_ORIGIN[1] + sub // 5), sub) for sub in range(10)]
    written = []
    for coord, sub in layout:
        row = cells[coord]
        assert row[4] & 0x100 == 0, coord
        row[2], row[3] = tile, sub
        # Distinct levels expose which cell supplies the flood level.
        row[8] = 4 + sub % 3
        written.append([coord[0], coord[1], tile, sub, row[8]])
    return case, written


class OriginalHead(OriginalBody):
    def __init__(self, case, family):
        self.heap = HEAP
        super().__init__(case)
        self.uc.mem_map(HEAP, 0x100000)
        # The wooden driver reads g_WoodBridgeSet_TileSetBase.
        self.uc.mem_write(0xABAD1C, dwords(case['bridge_base']))

    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        args = struct.unpack('<5I', u.mem_read(sp + 4, 20))
        point = lambda pointer: list(struct.unpack('<hh', u.mem_read(pointer, 4)))
        if address == 0x7C8E17:
            pointer = self.heap
            self.heap += (args[0] + 15) & ~15
            assert self.heap < HEAP + 0x100000
            self.return_with(0, pointer)
        elif address == 0x7C8B3D:
            self.return_with(0)
        elif address in HELPERS:
            family, function = HELPERS[address]
            self.events.append(dict(kind='perpendicular_sink', family=family, function=function,
                                    coord=point(args[0]), direction=args[1]))
            self.return_with(8)
        elif address in RIMS:
            self.events.append(dict(kind='rim_sink', family=RIMS[address], coord=point(args[0])))
            self.return_with(4)
        elif address == 0x56DAE0:
            self.events.append(dict(kind='zone_sink', coord=point(args[0])))
            self.return_with(4)
        elif address == 0x56EB80:
            self.events.append(dict(kind='flood', coord=point(args[0]),
                                    tile=struct.unpack('<i', dwords(args[1]))[0],
                                    level=struct.unpack('<i', dwords(args[3]))[0]))
            self.return_with(20)
        elif address == 0x586990:
            items = struct.unpack('<I', u.mem_read(args[0] + 4, 4))[0]
            count = struct.unpack('<I', u.mem_read(args[0] + 16, 4))[0]
            self.events.append(dict(kind='recalc_sink',
                                    cells=[point(items + 4 * i) for i in range(count)]))
            self.return_with(4)
        elif address in (0x47DD70, 0x6551C0, 0x6D2140, 0x6D2790):
            super().observe(u, address, size, data)

    def return_with(self, cleanup, eax=0):
        self.return_from_sink(cleanup)
        self.uc.reg_write(UC_X86_REG_EAX, eax)


CASES = [
    # (name, template, variant, input offset in template subtiles)
    ('ns_first_hit_from_north', 'ns', 0, 0),
    ('ns_first_hit_from_south', 'ns', 1, 8),
    ('ns_second_column_absorbs', 'ns', 0, 3),
    ('ns_collapse_from_anchor', 'ns', 3, 4),
    ('ns_collapse_from_south', 'ns', 3, 8),
    ('ew_first_hit_from_west', 'ew', 0, 0),
    ('ew_first_hit_from_east', 'ew', 2, 4),
    ('ew_second_row_absorbs', 'ew', 0, 7),
    ('ew_collapse_from_west', 'ew', 3, 1),
]


def input_coord(kind, sub):
    if kind == 'ns':
        return (NS_ORIGIN[0] + sub % 2, NS_ORIGIN[1] + sub // 2)
    return (EW_ORIGIN[0] + sub % 5, EW_ORIGIN[1] + sub // 5)


def cases():
    source = Path(__file__).with_name('bridge_rim_stock_inputs.json')
    stock = json.loads(source.read_text(encoding='utf-8'))
    result = []
    for family in ('high', 'low'):
        for name, kind, variant, sub in CASES:
            case, written = template(copy.deepcopy(stock), kind, variant)
            native = OriginalHead(case, family)
            coord = input_coord(kind, sub)
            before = {c: native.snapshot(p) for c, p in native.ptrs.items()}
            native.uc.mem_write(COORD, packed(*coord))
            returned = native.call(DRIVERS[family], args=(COORD,))
            result.append(dict(name=f'{family}_{name}', family=family, template=kind,
                               variant=variant, template_cells=written,
                               input_coord=coord, returned=returned,
                               calls=native.events,
                               changed_cells=[dict(before=before[c], after=native.snapshot(p))
                                              for c, p in native.ptrs.items()
                                              if before[c] != native.snapshot(p)]))
    return dict(stock_input_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                origins=dict(ns=NS_ORIGIN, ew=EW_ORIGIN), cases=result)


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original bridgehead branches of 576BA0 and 571490: tile gate, walk, variant branch, '
              'blow-up row, flood tile/level, helper/rim/zone order and the 586990 vector',
        assumptions=[
            'Synthetic BridgeMiddle1/2 templates in native subtile layout, laid over non-structural '
            'stock xbayopigs crop cells; no stock bridgehead is loaded',
            'Template levels are 4 + subtile % 3 so the flood level source is observable',
            'The concrete and wooden drivers run over the same cells with the stock BridgeSet base '
            'in their respective tileset-base globals',
            'One driver call per case from a fresh crop; outer damage admission/RNG excluded',
        ], substitutions=[
            'BlowUpBridge47DD70/radar/presentation sinks inherited from bridge_rim',
            'All sixteen UpdateRamp helpers, FloodFillIsoTileType56EB80, rims576770/571050, '
            '56DAE0 (returns0) and 586990 are recording sinks',
            'operator new/delete (7C8E17/7C8B3D) use a bounded bump heap; 42FCB0/42F860/42F7C0 execute',
        ], entry_points={'high_driver': 0x576BA0, 'low_driver': 0x571490}))
