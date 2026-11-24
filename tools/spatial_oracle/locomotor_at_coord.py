"""Original active-YR Head_To/Is_At_Coord, including native track tables.

These are supplied locomotor states, not a movement or repair simulation. Both
virtual calls and Drive/Ship transforms execute unmodified retail instructions.
NullCoord=(0,0,0) and height step104 are explicit initialized fixture inputs.
Stored-head creation/clearing, runtime startup and object admission are excluded.
"""
from pathlib import Path
import struct
import hashlib

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EDI
from tools.native_oracle import (
    load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE,
    RET_MAGIC, finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords


# ILocomotion interface addresses; head_offset is relative to that interface.
FAMILIES = {
    'drive': dict(vtable=0x7E7EB0, head=0x4AFCC0, query=0x4B4920,
                  head_offset=0x3C, null=0x8A0790, step=0x8A07D0,
                  after_head=0x4B4935, after_transform=0x4B49E6,
                  transform=0x4B4780, turns=0x7E7B28, raw=0x7E7A28, count=72),
    'ship': dict(vtable=0x7F2D8C, head=0x69F3D0, query=0x6A3F50,
                 head_offset=0x3C, null=0xB077F8, step=0xB07838,
                 after_head=0x6A3F65, after_transform=0x6A4016,
                 transform=0x6A3DB0, turns=0x7F2A40, raw=0x7F2960, count=64),
    'walk': dict(vtable=0x7F69F8, head=0x75AC00, query=0x75CA80,
                 head_offset=0x24, null=0xB45BE8, step=0xB45C28,
                 after_head=0x75CA96),
    'hover': dict(vtable=0x7EACFC, head=0x514D10, query=0x517210,
                  head_offset=0x20, null=0xA8F180, step=0xA8F1C0,
                  after_head=0x517226),
}
# Four active YR families with the shared false leaf. Dormant TS Mech has a
# different, non-stub query and is deliberately outside this active-retail corpus.
FALSE_VTABLES = {'fly': 0x7E89F4, 'jumpjet': 0x7ECD68,
                 'rocket': 0x7F0B1C, 'teleport': 0x7F5000}
LOCO, FOOT, OUTPUT = SCRATCH + 0x104, SCRATCH + 0x1000, SCRATCH + 0x2000


class OriginalQuery:
    def __init__(self, case):
        self.case = case
        self.family = FAMILIES[case['family']]
        self.uc = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, SCRATCH_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        f = self.family
        assert self.read32(f['vtable'] + 0x18) == f['head']
        assert self.read32(f['vtable'] + 0xA0) == f['query']
        u.mem_write(LOCO, dwords(f['vtable']))
        u.mem_write(LOCO + 8, dwords(FOOT))
        u.mem_write(FOOT + 0x9C, dwords(*case['current']))
        u.mem_write(LOCO + f['head_offset'], dwords(*case['stored_head']))
        u.mem_write(f['null'], dwords(0, 0, 0))
        u.mem_write(f['step'], dwords(104))
        if 'turns' in f:
            u.mem_write(LOCO + 0x54, dwords(case.get('turn_index', -1),
                                          case.get('cursor', 0)))
            u.mem_write(LOCO + 0x5C, bytes((int(case.get('reversed', False)),)))
        self.head_result = self.track_result = None
        u.hook_add(UC_HOOK_CODE, self.observe)

    def read32(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    def coord(self, address, size=3):
        return list(struct.unpack('<' + 'i' * size, self.uc.mem_read(address, 4 * size)))

    def observe(self, u, address, _size, _data):
        if address == self.family['after_head']:
            self.head_result = self.coord(u.reg_read(UC_X86_REG_EAX))
        elif address == self.family.get('after_transform'):
            self.track_result = self.coord(u.reg_read(UC_X86_REG_EAX), 2)

    def call(self, entry, args, required):
        sp = STACK_BASE + STACK_SIZE - 0x1000
        self.uc.mem_write(sp, dwords(RET_MAGIC, *args))
        self.uc.reg_write(UC_X86_REG_ESP, sp)
        run_checked(self.uc, entry, RET_MAGIC, count=5000, required_addresses=required)
        assert self.uc.reg_read(UC_X86_REG_ESP) == sp + 4 * (len(args) + 1)

    def head(self):
        self.call(self.family['head'], [LOCO, OUTPUT], [self.family['head']])
        assert self.uc.reg_read(UC_X86_REG_EAX) == OUTPUT
        return self.coord(OUTPUT)

    def query(self, probe):
        self.head_result = self.track_result = None
        self.call(self.family['query'], [LOCO, *probe],
                  [self.family['query'], self.family['head']])
        return dict(at=bool(self.uc.reg_read(UC_X86_REG_EAX) & 0xFF),
                    head=self.head_result, track_xy=self.track_result)

    def marker_probe(self, cell, height):
        """Original42B080 prefix only; stop before the first Map lookup."""
        # These bounded controls request ground height zero. The executable
        # global is recorded, but zero * its bits needs no startup assumption.
        assert height == 0
        u = self.uc
        cell_slot = SCRATCH + 0x3000
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.mem_write(cell_slot, struct.pack('<hh', *cell))
        u.mem_write(sp, dwords(RET_MAGIC, cell_slot, height))
        u.reg_write(UC_X86_REG_ESP, sp)
        code = bytes(u.mem_read(0x42B080, 0x78))
        trace = []
        hook = u.hook_add(UC_HOOK_CODE,
                          lambda _u, pc, _size, _data: trace.append(pc))
        try:
            run_checked(u, 0x42B080, 0x42B0F8, count=100,
                        required_addresses=(0x42B08A, 0x42B08D, 0x42B091,
                                            0x42B094, 0x42B09E, 0x42B0A3, 0x42B0AB))
        finally:
            u.hook_del(hook)
        retained_sp = u.reg_read(UC_X86_REG_ESP)
        assert retained_sp == sp - 0x2C
        xyz = [*self.coord(retained_sp + 0x20, 2),
               wrap32(u.reg_read(UC_X86_REG_EDI))]
        assert bytes(u.mem_read(0x42B080, 0x78)) == code
        assert bytes(u.mem_read(cell_slot, 4)) == struct.pack('<hh', *cell)
        return dict(cell=list(cell), requested_height=height, xyz=xyz,
                    entry='0x0042B080', stop_before='0x0042B0F8',
                    original_level_step_bytes=bytes(u.mem_read(0x89C2D8, 4)).hex(),
                    prefix_sha256=hashlib.sha256(code).hexdigest(),
                    executed_pcs=[f'0x{pc:08X}' for pc in trace],
                    full_nearby_scan_executed=False)

    def turn(self, index):
        f = self.family
        normal, short, facing, flags = struct.unpack(
            '<bb2xii', self.uc.mem_read(f['turns'] + index * 12, 12))
        point_table, _chain, _entry, handoff = struct.unpack(
            '<Iiii', self.uc.mem_read(f['raw'] + normal * 16, 16))
        point = self.coord(point_table + handoff * 12) if normal and handoff >= 0 else None
        return dict(normal=normal, short=short, facing=facing, flags=flags,
                    handoff=handoff, point=point)

    def run(self):
        head = self.head()
        first = self.query(head)
        # Native outputs select discriminating input coordinates. Expected query
        # results still come only from subsequent original query executions.
        candidates = [head, self.case['current']]
        if first['track_xy'] is not None:
            candidates += [[*first['track_xy'], self.case['current'][2]],
                           [*first['track_xy'], head[2]]]
        probes = list(self.case.get('probes', []))
        for xyz in candidates:
            probes += [[*xyz[:2], wrap32(xyz[2] + dz)] for dz in (-105, -104, 0, 104, 105)]
        probes = [list(xyz) for xyz in dict.fromkeys(map(tuple, probes))]
        queries = [dict(probe=probe, **self.query(probe)) for probe in probes]
        assert all(query['head'] == head for query in queries)
        assert self.coord(LOCO + self.family['head_offset']) == self.case['stored_head']
        assert self.coord(FOOT + 0x9C) == self.case['current']
        return dict(head=head, queries=queries)


def wrap32(value):
    return struct.unpack('<i', dwords(value))[0]


def fixture(family, name, current=(1408, 1408, 416), head=(1664, 1408, 17), **kwargs):
    return dict(family=family, name=name, current=list(current), stored_head=list(head), **kwargs)


def inputs():
    for family in FAMILIES:
        yield fixture(family, 'retained_head_height')
        yield fixture(family, 'null_head_uses_current', head=(0, 0, 0))
        yield fixture(family, 'both_null', current=(0, 0, 0), head=(0, 0, 0))
        for axis in range(3):
            head = [0, 0, 0]
            head[axis] = 1
            yield fixture(family, f'partial_null_axis_{axis}', head=head)
        for value in (-257, -256, -255, -1, 0, 1, 255, 256, 257):
            yield fixture(family, f'signed_xy_{value}', head=(value, value, 17),
                          probes=[[x, y, 17] for x, y in
                                  [(0, 0), (-256, -256), (256, 256),
                                   (value + 256 * 65536, value, ),
                                   (value, value + 256 * 65536)]])
        for z in (-2147483648, 2147483647):
            yield fixture(family, f'wrapping_z_{z}', head=(1664, 1408, z),
                          probes=[[1664, 1408, 0], [1664, 1408, -1]])
    # FindNearbyBridgePeer42B080 supplies native-produced probe XYZ to slot40.
    # Prefix execution is bounded before the first Map lookup; the complete
    # locomotor decision below still executes its original receiver.
    reader = OriginalQuery(fixture('drive', 'marker_table_reader'))
    shared = reader.marker_probe((5, 4), 0)
    yield fixture('drive', 'marker_shared_cell_handoff_z',
                  current=(1152, 1152, 0), head=(1520, 1152, 416),
                  turn_index=2, cursor=18, table=reader.turn(2),
                  marker_probe_receipt=shared, probes=[shared['xyz']])
    null = reader.marker_probe((0, 0), 0)
    yield fixture('drive', 'marker_null_head_center',
                  current=(0, 0, 0), head=(0, 0, 0),
                  marker_probe_receipt=null, probes=[null['xyz']])
    negative = reader.marker_probe((-1, -1), 0)
    yield fixture('hover', 'marker_negative_signed_cell_center',
                  current=(128, 128, 0), head=(128, 128, 0),
                  marker_probe_receipt=negative, probes=[negative['xyz']])
    for family in ('drive', 'ship'):
        reader = OriginalQuery(fixture(family, 'table_reader'))
        for index in range(FAMILIES[family]['count']):
            turn = reader.turn(index)
            variants = [('before', max(0, turn['handoff'] - 1), False),
                        ('at', max(0, turn['handoff']), False),
                        ('reversed', 0, True)] if turn['normal'] and turn['handoff'] >= 0 else [
                            ('no_handoff', 0, False)]
            for name, cursor, reverse in variants:
                yield fixture(family, f'turn_{index}_{name}', turn_index=index,
                              cursor=cursor, reversed=reverse, table=turn)
        yield fixture(family, 'null_stored_head_with_active_track', head=(0, 0, 0),
                      turn_index=1, cursor=0)
        yield fixture(family, 'signed_track_cursor', head=(-128, -128, 17),
                      turn_index=1, cursor=-1)


def false_queries():
    runner = OriginalQuery(fixture('drive', 'false_leaf'))
    rows = []
    for family, table in FALSE_VTABLES.items():
        entry = runner.read32(table + 0xA0)
        assert entry == 0x4B6630
        runner.uc.mem_write(LOCO, dwords(table))
        # The original false leaf does not touch the supplied null Foot pointer.
        runner.uc.mem_write(LOCO + 8, dwords(0))
        for probe in [(0, 0, 0), (-1, 2147483647, -2147483648)]:
            runner.call(entry, [LOCO, *probe], [entry])
            rows.append(dict(family=family, vtable=table, probe=list(probe),
                             at=bool(runner.uc.reg_read(UC_X86_REG_EAX) & 255)))
    return rows


def generate():
    return dict(cases=[dict(input=case, output=OriginalQuery(case).run()) for case in inputs()],
                false_queries=false_queries())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original active-YR locomotor Head_To and Is_At_Coord with original Drive/Ship transforms',
        assumptions=[
            'Supplied live interface/Foot/raw-head/turn/cursor states; no movement producers or lifecycle',
            'NullCoord=(0,0,0) and per-family height step104 explicitly supplied; startup is not executed',
            'Original vtable slots, TurnTrack descriptors, RawTrack metadata and point arrays are retained',
            'Drive72 and ordinary Ship64 TurnTrack descriptors; inputs are bounded branch witnesses',
            'Mech, Tunnel and DropPod dormant TS behavior is excluded; active false families use original leaf',
            'Signed/overflow cases describe native scalar behavior, not reachability on stock maps',
            'Three marker controls execute original42B080 signed-cell center/height prefix through stop-before42B0F8, then original slot40; no Map lookup or full nearby-list scan executes. Requested height is zero, with original level-step global bytes recorded; no startup claim for that global.',
        ], substitutions=[], entry_points={
            **{f'{name}_head': row['head'] for name, row in FAMILIES.items()},
            **{f'{name}_query': row['query'] for name, row in FAMILIES.items()},
            'nearby_probe_prefix': 0x42B080, 'nearby_probe_stop': 0x42B0F8,
            'drive_transform': 0x4B4780, 'ship_transform': 0x6A3DB0, 'false_query': 0x4B6630,
        }))
