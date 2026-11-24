"""Original56E990 and pristine damaged-data getter5471F0.

Control witnesses use supplied native TMP headers; the stock sequence uses actual
Ovrps02.urb bytes and a frozen current-loader cell footprint. Radar and tactical
display are declared sinks. This is not native loading or full repair execution.
"""
import hashlib
import json
import os
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_rim import OriginalRim, GLOBALS, COORD, DUMMY, CELLS
from tools.spatial_oracle.map_queries import dwords, packed

ROOT = Path(__file__).resolve().parents[2]
TABLE = 0x44001000


class OriginalPavement(OriginalRim):
    def __init__(self, case, heads, callback=None):
        self.trace = []
        self.entries = 0
        self.callback = callback
        self.heads = {}
        super().__init__(case)
        u = self.uc
        u.mem_write(DUMMY + 0x38, dwords(0xFFFF))
        u.mem_map(0x44000000, 0x100000)
        u.mem_write(0xA8ED2C, dwords(TABLE))
        for index, (tile, source) in enumerate(heads.items()):
            head = 0x44010000 + index * 0x400
            data = 0x44020000 + index * 0x10000
            tmp = bytearray(source)
            width, height = struct.unpack_from('<II', tmp)
            for sub in range(width * height):
                offset = struct.unpack_from('<I', tmp, 16 + sub * 4)[0]
                if offset:
                    struct.pack_into('<I', tmp, 16 + sub * 4, data + offset)
            u.mem_write(TABLE + tile * 4, dwords(head))
            u.mem_write(head, dwords(0x7ECC48))
            u.mem_write(head + 0xA4, dwords(data))
            u.mem_write(head + 0x2F0, dwords(1))
            u.mem_write(data, bytes(tmp))
            self.heads[head] = tile

    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x56E990:
            self.entries += 1
            if self.entries == 2 and self.callback:
                x, y, tile = self.callback
                u.mem_write(self.ptrs[x, y] + 0x38, dwords(tile))
                self.trace.append(dict(kind='entry_write', coord=[x, y], tile=tile))
        elif address == 0x5471F0:
            sub = struct.unpack('<I', u.mem_read(sp + 4, 4))[0]
            self.trace.append(dict(kind='gate', tile=self.heads[u.reg_read(UC_X86_REG_ECX)], sub=sub))
        elif address == 0x6551C0:
            pointer = struct.unpack('<I', u.mem_read(sp + 4, 4))[0]
            self.trace.append(dict(kind='radar', coord=list(struct.unpack('<hh', u.mem_read(pointer, 4)))))
        elif address == 0x6D2790:
            self.trace.append(dict(kind='screen'))
        elif address in (0x47D2B0, 0x47DD70):
            raise AssertionError(f'Unexpected Recalc/fallout {address:08x}')
        super().observe(u, address, size, data)

    def write(self, u, access, address, size, value, data):
        cell = (CELLS + (address - CELLS) // 0x200 * 0x200
                if CELLS <= address < CELLS + len(self.ptrs) * 0x200 else DUMMY)
        if address == cell + 0x140:
            self.trace.append(dict(kind='flags', coord=self.coord(cell), flags=value))

    def step(self, coord, state, entry=0x56E990, direction=6):
        self.trace.clear()
        self.uc.mem_write(COORD, packed(*coord))
        self.call(entry, args=(COORD, state, 0) if entry == 0x56E990 else (COORD, direction))
        final = [[*coord, struct.unpack('<i', self.uc.mem_read(p + 0x38, 4))[0],
                  struct.unpack('<I', self.uc.mem_read(p + 0x140, 4))[0]]
                 for coord, p in self.ptrs.items()]
        return dict(state=state, trace=list(self.trace), final=final,
                    dummy=[*self.coord(DUMMY), struct.unpack('<I', self.uc.mem_read(DUMMY + 0x140, 4))[0]])

    def gate_sweep(self, tile):
        head = next(pointer for pointer, identity in self.heads.items() if identity == tile)
        return [bool(self.call(0x5471F0, this=head, args=(sub,)) & 0xFF) for sub in range(256)]


def synthetic_tmp(entries):
    result = bytearray(16 + 4 * len(entries) + 0x30 * len(entries))
    struct.pack_into('<4I', result, 0, len(entries), 1, 60, 30)
    for sub, damaged in enumerate(entries):
        if damaged is not None:
            offset = 16 + 4 * len(entries) + 0x30 * sub
            struct.pack_into('<I', result, 16 + 4 * sub, offset)
            struct.pack_into('<I', result, offset + 0x24, 4 if damaged else 0)
    return result


def control_cases():
    # x,y,tile,sub,flags. Mixed flags also prove that only bit13 is changed.
    rows = [(100, 100, 100, 0, 0xABCD1180), (100, 99, 100, 1, 0),
            (101, 99, 100, 2, 0x100), (101, 100, 100, 3, 0),
            (104, 104, 100, 0, 0)]
    cases = [
        ('plain_and_structural_depth_first', rows, (100, 100), [1, 1, 0], None),
        ('already_set_blocks_recursion', [(100, 100, 100, 0, 0x2000), (100, 99, 100, 0, 0)], (100, 100), [1], None),
        ('recursive_skips_damaged_gate', [(100, 100, 100, 0, 0), (100, 99, 100, 1, 0)], (100, 100), [1], None),
        ('kickoff_false_gate', [(100, 100, 100, 1, 0)], (100, 100), [1], None),
        ('kickoff_sparse_gate', [(100, 100, 100, 2, 0)], (100, 100), [1], None),
        ('unsigned_subtile_wrap', [(100, 100, 100, 255, 0)], (100, 100), [1], None),
        ('reject_ffff', [(100, 100, 0xFFFF, 0, 0)], (100, 100), [1], None),
        ('reject_00ff', [(100, 100, 0xFF, 0, 0)], (100, 100), [1], None),
        ('missing_cell_dummy_gate', [], (100, 100), [1], None),
        ('negative_is_not_sentinel', [(100, 100, -1, 0, 0)], (100, 100), [1], None),
        ('requested_stride_alias', [(511, 100, 100, 0, 0), (0, 101, 100, 0, 0), (1, 101, 100, 0, 0)], (511, 100), [1, 0], None),
        ('child_captures_current_tile', [(100, 100, 100, 0, 0), (100, 99, 100, 0, 0), (100, 98, 200, 0, 0)], (100, 100), [1], (100, 99, 200)),
    ]
    result = []
    predicate = [True, False, None, True]
    for name, cells, start, states, callback in cases:
        case = dict(size=[136, 140], bridge_base=0, rim_keys={k: None for k in GLOBALS},
                    cells=[[x,y,tile,sub,flags,None,0,None,4,0] for x,y,tile,sub,flags in cells])
        native = OriginalPavement(case, {tile: synthetic_tmp(predicate) for tile in (100, 200, -1)}, callback)
        row = dict(name=name, cells=cells, start=start, predicate=predicate,
                   second_entry_write=callback)
        if not result:
            row['gate_sweep'] = native.gate_sweep(100)
        row['steps'] = [native.step(start, state) for state in states]
        result.append(row)
    return result


def stock_case():
    source = Path(__file__).with_name('bridge_pavement_stock_inputs.json')
    case = json.loads(source.read_text(encoding='utf-8'))
    asset = Path(os.environ.get('VERA20K_BRIDGE_PAVEMENT_ASSETS', str(ROOT / '.local/bridge-pavement-assets/extract'))) / 'ovrps02.urb'
    data = asset.read_bytes()
    native = OriginalPavement(case, {278: data})
    start = (66, 102)  # A road receiver with no BridgeRuntimeCell membership.
    perpendicular = OriginalPavement(case, {278: data}).step((67, 102), 1, entry=0x572330)
    return dict(source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                asset_sha256=hashlib.sha256(data).hexdigest(), start=start,
                gate_sweep=native.gate_sweep(278),
                perpendicular=dict(entry=0x572330, input=[67, 102], direction=6, result=perpendicular),
                steps=[native.step(start, state) for state in (1, 1, 0)])


if __name__ == '__main__':
    finish_vectors(lambda: dict(control=control_cases(), stock=stock_case()),
        Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
            scope='Original56E990 control and stock Ovrps02.urb damage/repeat/clear; actual572330 stock perpendicular entry',
            assumptions=[
                'Actual5471F0 and pristine getter vtable execute over relocated TMP headers; synthetic controls supply four entries true,false,absent,true',
                'Active callers supply Boolean states0/1; nonboolean low bytes are excluded',
                'Native49F2F0 initializes directions; supplied512-stride cell plane and ctor dummyFFFF',
                'Synthetic second-entry tile mutation distinguishes child current tile from inherited parent tile; no stock callback/reentry claim',
                'Stock35-cell crop from frozen xmp34u4 loader export; map/export hashes recorded; native loading excluded',
                'Actual Ovrps02.urb bytes relocated without native TMP loading; no full repair, radar colors, navigation or displayed-output equivalence claim',
            ], substitutions=[
                '6551C0 records coordinate and returns; radar queue/color excluded',
                '6D2140 supplies screen0,0 and6D2790 records screen callback; no projection/display',
            ], entry_points={'pavement':0x56E990,'damaged_gate':0x5471F0,'directions':0x49F2F0,'stock_perpendicular':0x572330}))
