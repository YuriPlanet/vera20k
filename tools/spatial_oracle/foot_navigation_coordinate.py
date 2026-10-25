"""Original Foot navigation, bridge layer queries and bounded base response.

The default 44-row coordinate corpus is unchanged. Use --bridge-layers to check
the separate original Foot+BC/Object bridge-layer and base-response call seams:
python -m tools.spatial_oracle.foot_navigation_coordinate --bridge-layers --check
See foot_bridge_layer.md for fixture inputs and evidence bounds.
Use --base-response for the separate readers/scorer/selection/dispatch corpus;
see base_defense_response.md for its executable and supplied-state limits.
"""
import argparse
import hashlib
import os
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESI,
    UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_EIP, UC_X86_REG_EDI,
    UC_X86_REG_EBP, UC_X86_REG_EDX,
)
from tools.native_oracle import (
    STACK_BASE, STACK_SIZE, RET_MAGIC, SCRATCH, file_span,
    image_bytes, run_checked, finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import TABLE, EMPTY_TABLE, DUMMY, dwords, packed
from tools.spatial_oracle.locomotor_at_coord import OriginalQuery, FAMILIES, LOCO, FOOT, OUTPUT


def query(row):
    fixture = OriginalQuery(row)
    u = fixture.uc
    # Original concrete owner table, including +48 Object-coordinate dispatch.
    u.mem_write(FOOT, dwords(0x7EB058 if row['family'] == 'walk' else 0x7F5C70))
    u.mem_write(FOOT + 0x674, dwords(LOCO))
    u.mem_write(0x8B3DA8, dwords(0, 0, 0))
    tube = row.get('tube')
    if tube is None:
        u.mem_write(FOOT + 0x684, b'\xff')
    else:
        table, descriptor = SCRATCH + 0x7000, SCRATCH + 0x7100
        u.mem_write(FOOT + 0x684, b'\x00')
        u.mem_write(0x8B413C, dwords(table))
        u.mem_write(table, dwords(descriptor))
        u.mem_write(descriptor + 0x28, packed(*tube))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC, OUTPUT, 0))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, FOOT)
    required = [0x4DBDF0, 0x4DBE01] if tube is not None else [0x4DBDF0, FAMILIES[row['family']]['head']]
    run_checked(u, 0x4DBDF0, RET_MAGIC, count=5000, required_addresses=required)
    assert u.reg_read(UC_X86_REG_ESP) == sp + 12
    assert u.reg_read(UC_X86_REG_EAX) == OUTPUT
    return dict(input=row, coordinate=fixture.coord(OUTPUT),
                retained_head=fixture.coord(LOCO + FAMILIES[row['family']]['head_offset']),
                current=fixture.coord(FOOT + 0x9C))


def generate():
    rows = []
    for family in FAMILIES:
        base = dict(family=family, current=[1408, 1408, 416],
                    stored_head=[1664, 1408, 17], turn_index=-1, cursor=0)
        variants = [dict(name='retained_without_selector'),
                    dict(name='null_head_current', stored_head=[0, 0, 0]),
                    dict(name='both_null', stored_head=[0, 0, 0], current=[0, 0, 0]),
                    dict(name='signed_head', stored_head=[-257, -1, -2147483648]),
                    dict(name='selector_independent', turn_index=1, cursor=-1)]
        variants += [dict(name=f'partial_null_{axis}', stored_head=[int(i == axis) for i in range(3)]) for axis in range(3)]
        variants += [dict(name=f'tube_{x}_{y}', tube=[x, y])
                     for x, y in [(10, 20), (-1, -32768), (32767, 0)]]
        rows += [query(base | variant) for variant in variants]
    return rows


MAP = 0x87F7E8
SP = STACK_BASE + STACK_SIZE - 0x1000
PROBE, VICTIM, VICTIM_LOCO = SCRATCH + 0x2F00, SCRATCH + 0x3000, SCRATCH + 0x4004
NAV_LOCO, NAV_FOOT, TYPE, CELLS = SCRATCH + 0x4804, SCRATCH + 0x5000, SCRATCH + 0x6000, SCRATCH + 0x8000
ZONE_BASE, ZONE_RAW, ZONE_RECORDS = 0xD10000, 0xD11000, 0xD12000
CONCRETE_TABLES = {'walk': 0x7EB058, 'drive': 0x7F5C70,
                   'ship': 0x7F5C70, 'hover': 0x7F5C70}
# Bounded original instruction spans, never callable replacements. Cell47B3A0
# lazily initializes its own native slope coefficients; those data writes are
# part of the original execution and are not an instruction patch.
BRIDGE_SPANS = {
    'foot_coordinate': (0x4DBDF0, 0xDF),
    'object_coordinate': (0x5F65A0, 0x21),
    'foot_bridge_layer': (0x4DDC40, 0x1C),
    'object_bridge_layer': (0x5F6A70, 0xE0),
    'map_ground': (0x578080, 0x77),
    'cell_ground': (0x47B3A0, 0x7BB),
    'map_coordinate_cell': (0x565730, 0x68),
    'ftol': (0x7C5F00, 0x3D),
    'response_infantry_seam': (0x708291, 0xB4),
    'response_unit_seam': (0x7084E9, 0xB8),
    'reach_zone': (0x56D100, 0x122),
    'get_zone': (0x56D230, 0x16A),
    'record_lookup': (0x56DA10, 0xC4),
    'playfield': (0x578460, 0x190),
    'harvest_seam': (0x4DCF26, 0x71),
    'locomotor_base_constructor': (0x55A6C0, 0x23),
    'drive_constructor': (0x4AF540, 0x9A),
    'ship_constructor': (0x69EC50, 0x9A),
    **{family + '_head': (state['head'], 0x80) for family, state in FAMILIES.items()},
}


class OriginalBridgeQuery(OriginalQuery):
    """Supplied Foot/map state with observations of original bridge queries."""

    def __init__(self, case):
        super().__init__(case)
        u = self.uc
        self.stage, self.queries, self.pending = 'setup', [], {}
        self.reach_queries, self.reach_pending, self.dummy_writes = [], {}, []
        self.coordinate_calls, self.coordinate_pending, self.bridge_calls = [], {}, []
        self.cell_ids = {DUMMY: 'dummy'}
        self.setup_foot(FOOT, LOCO, case)
        u.mem_write(FOOT + 0x5A4, dwords(NAV_FOOT if case['nav_com_coord'] is not None else 0))
        if case['nav_com_coord'] is not None:
            self.setup_foot(NAV_FOOT, NAV_LOCO, dict(
                family='drive', current=case['nav_com_coord'], stored_head=case['nav_com_coord'],
                on_bridge=False, tube=None, locomotor_present=True))
        u.mem_write(TABLE, EMPTY_TABLE)
        u.mem_write(MAP + 0x13C, dwords(TABLE, case['table_capacity']))
        for name, value in case['scalar_globals'].items():
            u.mem_write(int(name, 16), dwords(value))
        u.reg_write(UC_X86_REG_FPCW, case['fpcw'])
        u.mem_write(0x822D80, dwords(case['ftol_control_word']))
        u.mem_write(0x89E770, b'\0')
        self.write_cell(DUMMY, case['dummy'])
        self.cell_snapshots = {}
        for index, cell in enumerate(case['cells']):
            pointer = CELLS + index * 0x200
            x, y = cell['xy']
            slot = y * 512 + x
            assert 0 <= slot < case['table_capacity'] <= 0x40000
            self.write_cell(pointer, cell)
            self.cell_ids[pointer] = f'cell_{index}'
            u.mem_write(TABLE + slot * 4, dwords(pointer))
            self.cell_snapshots[pointer] = bytes(u.mem_read(pointer, 0x200))
        self.originals = {name: bytes(u.mem_read(address, size))
                          for name, (address, size) in BRIDGE_SPANS.items()}
        self.foot_snapshot = bytes(u.mem_read(FOOT, 0x700))
        self.loco_snapshot = bytes(u.mem_read(LOCO, 0x100))
        u.hook_add(UC_HOOK_CODE, self.observe_bridge)
        u.hook_add(UC_HOOK_MEM_WRITE, self.observe_dummy_write)

    def setup_foot(self, foot, loco, row):
        u = self.uc
        table = CONCRETE_TABLES[row['family']]
        assert self.read32(table + 0x4C) == 0x4DBDF0
        assert self.read32(table + 0xBC) == 0x4DDC40
        assert self.read32(table + 0x48) == 0x5F65A0
        u.mem_write(foot, dwords(table))
        u.mem_write(foot + 0x9C, dwords(*row['current']))
        u.mem_write(foot + 0x8C, bytes((int(row['on_bridge']),)))
        u.mem_write(foot + 0x674, dwords(loco if row['locomotor_present'] else 0))
        u.mem_write(0x8B3DA8, dwords(0, 0, 0))
        family = FAMILIES[row['family']]
        u.mem_write(loco, dwords(family['vtable']))
        u.mem_write(loco + 8, dwords(foot))
        u.mem_write(loco + family['head_offset'], dwords(*row['stored_head']))
        u.mem_write(family['null'], dwords(0, 0, 0))
        u.mem_write(family['step'], dwords(104))
        tube = row['tube']
        u.mem_write(foot + 0x684, b'\xff' if tube is None else b'\0')
        if tube is not None:
            table, descriptor = SCRATCH + 0x7000, SCRATCH + 0x7100
            u.mem_write(0x8B413C, dwords(table))
            u.mem_write(table, dwords(descriptor))
            u.mem_write(descriptor + 0x28, packed(*tube))

    def write_cell(self, pointer, cell):
        self.uc.mem_write(pointer, bytes(0x200))
        self.uc.mem_write(pointer + 0x24, packed(*cell['xy']))
        self.uc.mem_write(pointer + 0x11B, bytes((cell['level'] & 255, cell['slope'])))
        self.uc.mem_write(pointer + 0x140, dwords(cell['flags']))

    def cell_state(self, pointer):
        return dict(identity=self.cell_ids[pointer],
                    xy=list(struct.unpack('<hh', self.uc.mem_read(pointer + 0x24, 4))),
                    level=struct.unpack('<b', self.uc.mem_read(pointer + 0x11B, 1))[0],
                    slope=self.uc.mem_read(pointer + 0x11C, 1)[0],
                    flags=self.read32(pointer + 0x140))

    def finish_query(self, address):
        u = self.uc
        if address in self.coordinate_pending:
            row = self.coordinate_calls[self.coordinate_pending.pop(address)]
            row['coordinate'] = self.coord(u.reg_read(UC_X86_REG_EAX))
        if address in self.pending:
            row = self.queries[self.pending.pop(address)]
            if row['entry'] == '0x00578080':
                row['ground'] = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
            else:
                row['cell'] = self.cell_state(u.reg_read(UC_X86_REG_EAX))
            row['dummy_after'] = self.cell_state(DUMMY)
        if address in self.reach_pending:
            row = self.reach_queries[self.reach_pending.pop(address)]
            eax = u.reg_read(UC_X86_REG_EAX)
            row['result'] = bool(eax & 255) if row['entry'] == '0x00578460' else struct.unpack('<i', dwords(eax))[0]
            row['dummy_after'] = self.cell_state(DUMMY)

    def observe_bridge(self, u, address, _size, _data):
        self.finish_query(address)
        if address in (0x578080, 0x565730):
            sp = u.reg_read(UC_X86_REG_ESP)
            argument = self.read32(sp + 4)
            self.queries.append(dict(stage=self.stage, entry=f'0x{address:08X}',
                                     xyz=self.coord(argument)))
            self.pending[self.read32(sp)] = len(self.queries) - 1
        elif address == 0x47B3A0:
            self.queries[-1]['cell'] = self.cell_state(u.reg_read(UC_X86_REG_ECX))
        elif address == 0x4DBDF0:
            sp = u.reg_read(UC_X86_REG_ESP)
            receiver = u.reg_read(UC_X86_REG_ECX)
            assert receiver in (FOOT, VICTIM)
            self.coordinate_calls.append(dict(stage=self.stage,
                                             receiver='candidate' if receiver == FOOT else 'victim'))
            self.coordinate_pending[self.read32(sp)] = len(self.coordinate_calls) - 1
        elif address in (0x4DDC40, 0x5F6A70):
            self.bridge_calls.append(dict(stage=self.stage, entry=f'0x{address:08X}'))
        elif address in (0x578460, 0x56D230, 0x56DA10):
            sp = u.reg_read(UC_X86_REG_ESP)
            argument = self.read32(sp + 4)
            count = 1 if address == 0x578460 else 2
            self.reach_queries.append(dict(entry=f'0x{address:08X}',
                                           xy=list(struct.unpack('<hh', u.mem_read(argument, 4))),
                                           arguments=[self.read32(sp + 8 + i * 4) for i in range(count)]))
            self.reach_pending[self.read32(sp)] = len(self.reach_queries) - 1

    def observe_dummy_write(self, u, _access, address, size, value, _data):
        if address == DUMMY + 0x24:
            assert size == 4
            self.dummy_writes.append(dict(stage=self.stage,
                                          pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',
                                          xy=list(struct.unpack('<hh', dwords(value)))))

    def invoke(self, entry, receiver, arguments, required):
        self.uc.reg_write(UC_X86_REG_ECX, receiver)
        self.call(entry, arguments, required)
        self.finish_query(RET_MAGIC)
        assert not self.pending and not self.coordinate_pending
        return self.uc.reg_read(UC_X86_REG_EAX)

    def validate_retained_state(self):
        assert self.originals == {name: bytes(self.uc.mem_read(address, size))
                                  for name, (address, size) in BRIDGE_SPANS.items()}
        assert self.foot_snapshot == bytes(self.uc.mem_read(FOOT, 0x700))
        assert self.loco_snapshot == bytes(self.uc.mem_read(LOCO, 0x100))
        assert all(blob == bytes(self.uc.mem_read(pointer, 0x200))
                   for pointer, blob in self.cell_snapshots.items())
        dummy = self.cell_state(DUMMY)
        assert all(dummy[key] == self.case['dummy'][key] for key in ('level', 'slope', 'flags'))

    def run_bridge(self):
        self.stage = 'prefix'
        for xyz in self.case['prefix_queries']:
            self.uc.mem_write(PROBE, dwords(*xyz))
            self.invoke(0x565730, MAP, [PROBE], [0x565730])
        self.stage = 'coordinate'
        required = [0x4DBDF0, 0x4DBE01] if self.case['tube'] is not None else [
            0x4DBDF0, FAMILIES[self.case['family']]['head']]
        assert self.invoke(0x4DBDF0, FOOT, [OUTPUT, 0], required) == OUTPUT
        coordinate = self.coord(OUTPUT)
        self.stage = 'bridge_layer'
        required = [0x4DDC40] if self.case['tube'] is not None else [
            0x4DDC40, 0x5F6A70, 0x4DBDF0, 0x578080, 0x47B3A0]
        layer = bool(self.invoke(0x4DDC40, FOOT, [0], required) & 255)
        if self.case['tube'] is not None:
            assert not any(row['stage'] == 'bridge_layer' for row in self.queries)
            assert not any(row['stage'] == 'bridge_layer' for row in self.coordinate_calls)
            assert not any(row['entry'] == '0x005F6A70' for row in self.bridge_calls)
        self.validate_retained_state()
        return dict(coordinate=coordinate, should_be_on_bridge=layer,
                    queries=self.queries, coordinate_calls=self.coordinate_calls,
                    bridge_calls=self.bridge_calls, final_dummy=self.cell_state(DUMMY),
                    dummy_writes=self.dummy_writes,
                    retained_head=self.coord(LOCO + self.family['head_offset']),
                    current=self.coord(FOOT + 0x9C), on_bridge=bool(self.uc.mem_read(FOOT + 0x8C, 1)[0]))

    def run_constructed_bridge(self):
        """Original fresh Drive/Ship constructor, supplied Foot link, queries."""
        constructor = {'drive': 0x4AF540, 'ship': 0x69EC50}[self.case['family']]
        local = LOCO - 4
        # Constructor inputs are explicit. Nonzero backing bytes ensure its
        # NullCoord initialization is observed rather than zero-filled memory.
        self.uc.mem_write(local, bytes((self.case['constructor_prefill_byte'],)) * 0x70)
        self.stage = 'constructor'
        returned = self.invoke(constructor, local, [], [constructor, 0x55A6C0])
        assert returned == local
        constructed = dict(
            entry=f'0x{constructor:08X}', base_entry='0x0055A6C0',
            object_vtable=f'0x{self.read32(local):08X}',
            locomotion_vtable=f'0x{self.read32(local + 4):08X}',
            piggyback_vtable=f'0x{self.read32(local + 0x18):08X}',
            destination=self.coord(local + 0x34), head=self.coord(local + 0x40),
            linked_foot=self.read32(local + 0xC), frame=self.read32(local + 0x24),
            selector=self.coord(local + 0x58, 2),
            payload_before_link=bytes(self.uc.mem_read(local, 0x6C)).hex())
        assert self.read32(local + 4) == self.family['vtable']
        # Linking the live Foot remains fixture state. The entire original
        # constructor/base and its head/destination/vtable writes ran above.
        self.uc.mem_write(local + 0xC, dwords(FOOT))
        self.loco_snapshot = bytes(self.uc.mem_read(LOCO, 0x100))
        return dict(input=self.case, constructor=constructed, output=self.run_bridge())

    def response_seam(self, victim, movement_zone, zones):
        """Execute the original scan-local block through original 56D100.

        Earlier scan admission and ranking are not executed or substituted.
        Stop before threat scoring, or at the reachability-refusal continuation.
        """
        self.setup_foot(VICTIM, VICTIM_LOCO, victim)
        infantry = self.case['family'] == 'walk'
        start, call_site = (0x708291, 0x70833C) if infantry else (0x7084E9, 0x708594)
        candidate_coord, layer_call = (0x7082DB, 0x708319) if infantry else (0x708533, 0x708571)
        self.uc.mem_write(FOOT + (0x6C0 if infantry else 0x6C4), dwords(TYPE))
        self.uc.mem_write(TYPE + 0x5B4, dwords(movement_zone))
        self.setup_zones(movement_zone, zones)
        self.foot_snapshot = bytes(self.uc.mem_read(FOOT, 0x700))
        self.stage = 'response_seam'
        self.uc.reg_write(UC_X86_REG_EBX, VICTIM)
        self.uc.reg_write(UC_X86_REG_ESI, FOOT)
        self.uc.reg_write(UC_X86_REG_ESP, SP)
        admitted, refused = (0x708345, 0x7083BC) if infantry else (0x7085A1, 0x708622)
        return self.run_reach_seam(start, movement_zone, (admitted, refused),
                                   (start, candidate_coord, layer_call, call_site, 0x4DBDF0, 0x4DDC40))

    def harvest_seam(self, clicked_cell, movement_zone, zones):
        """Original IsCellHarvestable coordinate/layer/zone block only."""
        type_offset = 0x6C0 if self.case['family'] == 'walk' else 0x6C4
        self.uc.mem_write(FOOT + type_offset, dwords(TYPE))
        self.uc.mem_write(TYPE + 0x5B4, dwords(movement_zone))
        self.setup_zones(movement_zone, zones)
        self.foot_snapshot = bytes(self.uc.mem_read(FOOT, 0x700))
        self.uc.mem_write(PROBE, packed(*clicked_cell))
        self.stage = 'harvest_seam'
        self.uc.reg_write(UC_X86_REG_ESI, FOOT)
        self.uc.reg_write(UC_X86_REG_EDI, PROBE)
        self.uc.reg_write(UC_X86_REG_ESP, SP)
        type_accessor = self.read32(CONCRETE_TABLES[self.case['family']] + 0x84)
        return self.run_reach_seam(0x4DCF26, movement_zone, 0x4DCF97,
                                   (0x4DCF31, 0x4DCF6F, 0x4DCF7A, 0x4DCF92,
                                    0x4DBDF0, 0x4DDC40, type_accessor))

    def run_reach_seam(self, start, movement_zone, ends, required):
        """Observe the same native six-argument protocol in either caller."""
        run_checked(self.uc, start, 0x56D100, count=5000,
                    required_addresses=required)
        sp = self.uc.reg_read(UC_X86_REG_ESP)
        arguments = [self.read32(sp + 4 + 4 * i) for i in range(6)]
        assert self.read32(sp) in (0x708341, 0x708599, 0x4DCF97)
        assert arguments[2] == movement_zone
        source_cell = list(struct.unpack('<hh', self.uc.mem_read(arguments[0], 4)))
        destination_cell = list(struct.unpack('<hh', self.uc.mem_read(arguments[1], 4)))
        dummy_at_reach = self.cell_state(DUMMY)
        endpoint = run_checked(self.uc, 0x56D100, ends, count=10000,
                               required_addresses=(0x56D100, 0x578460))
        assert not self.pending and not self.reach_pending and not self.coordinate_pending
        assert self.uc.reg_read(UC_X86_REG_ESP) == SP
        self.validate_retained_state()
        return dict(start=f'0x{start:08X}', stop_before=f'0x{endpoint:08X}',
                    source_cell=source_cell, destination_cell=destination_cell,
                    movement_zone=arguments[2], source_on_bridge=bool(arguments[3] & 255),
                    destination_on_bridge=bool(arguments[4] & 255),
                    allow_destination_fringe=bool(arguments[5] & 255),
                    raw_scalar_arguments=arguments[2:], queries=self.queries,
                    coordinate_calls=self.coordinate_calls, bridge_calls=self.bridge_calls,
                    reachable=bool(self.uc.reg_read(UC_X86_REG_EAX) & 255), reach_queries=self.reach_queries,
                    dummy_at_reach=dummy_at_reach, dummy_writes=self.dummy_writes,
                    final_dummy=self.cell_state(DUMMY),
                    retained_head=self.coord(LOCO + self.family['head_offset']), current=self.coord(FOOT + 0x9C))

    def setup_zones(self, movement_zone, zones):
        # Supplied simple base/raw zone state follows the existing
        # walk_move_admission fixture contract. There is no flood-fill result
        # substitution: original56D100/56D230/56DA10 read this state themselves.
        width, height = zones['size']
        side = width + height + 1
        capacity = side * side
        self.uc.mem_write(MAP + 0xF4, dwords(width, height, *zones['bounds']))
        self.uc.mem_write(MAP + 0x68, dwords(ZONE_BASE, capacity))
        self.uc.mem_write(ZONE_BASE, bytes(capacity * 4))
        for cell in zones['group_cells']:
            x, y = cell['xy']
            assert 0 <= x + y * side < capacity
            self.uc.mem_write(ZONE_BASE + (x + y * side) * 4 + 2, struct.pack('<H', cell['group']))
        self.uc.mem_write(MAP + 0x18 + movement_zone * 4, dwords(ZONE_RAW))
        self.uc.mem_write(ZONE_RAW, struct.pack('<' + 'H' * len(zones['raw_labels']), *zones['raw_labels']))
        for index, record in enumerate(zones['records']):
            self.uc.mem_write(ZONE_RECORDS + index * 16,
                              packed(*record['a']) + packed(*record['b']) + dwords(int(record['active']), record['kind']))
        self.uc.mem_write(MAP + 0x54, dwords(ZONE_RECORDS, len(zones['records']), 0, len(zones['records'])))


def bridge_case(family, name, **updates):
    row = dict(family=family, name=name, current=[1280, 1280, 416],
               stored_head=[1792, 1280, 17], turn_index=-1, cursor=0,
               on_bridge=False, tube=None, locomotor_present=True,
               nav_com_coord=[2304, 1280, 999], table_capacity=0x40000,
               scalar_globals={'0x0089E7C0': 104, '0x00AC13C8': 104},
               fpcw=0x0E7F, ftol_control_word=0x0E7F, prefix_queries=[],
               cells=[dict(xy=[5, 5], level=3, slope=1, flags=0),
                      dict(xy=[7, 5], level=0, slope=0, flags=0x100),
                      dict(xy=[9, 5], level=9, slope=0, flags=0x400)],
               dummy=dict(xy=[1234, -2345], level=-7, slope=1, flags=0x500))
    row.update(updates)
    return row


def bridge_inputs():
    for family in FAMILIES:
        for on_bridge in (False, True):
            suffix = f'_on_bridge_{int(on_bridge)}'
            for name, local_x, level, slope in (
                    ('311', 1, 2, 3), ('312', 0, 3, 1), ('313', 3, 3, 1), ('416', 0, 4, 0)):
                yield bridge_case(family, 'down_' + name + suffix, on_bridge=on_bridge,
                                  current=[1280 + local_x, 1280, -777], cells=[
                                      dict(xy=[5, 5], level=level, slope=slope, flags=0),
                                      dict(xy=[7, 5], level=0, slope=0, flags=0x100)])
                yield bridge_case(family, 'up_' + name + suffix, on_bridge=on_bridge,
                                  current=[1280, 1280, 777], stored_head=[1792 + local_x, 1280, -999], cells=[
                                      dict(xy=[5, 5], level=0, slope=0, flags=0),
                                      dict(xy=[7, 5], level=level, slope=slope, flags=0x100)])
            for flags in (0, 0x400, 0x500):
                yield bridge_case(family, f'down_416_flags_{flags:x}' + suffix, on_bridge=on_bridge,
                                  cells=[dict(xy=[5, 5], level=4, slope=0, flags=0),
                                         dict(xy=[7, 5], level=0, slope=0, flags=flags)])
            for head in ([0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1]):
                yield bridge_case(family, 'head_' + '_'.join(map(str, head)) + suffix,
                                  on_bridge=on_bridge, stored_head=head,
                                  cells=[dict(xy=[5, 5], level=4, slope=0, flags=0),
                                         dict(xy=[0, 0], level=0, slope=0, flags=0x100)])
            yield bridge_case(family, 'both_null' + suffix, on_bridge=on_bridge,
                              current=[0, 0, 0], stored_head=[0, 0, 0], cells=[
                                  dict(xy=[0, 0], level=-128, slope=0, flags=0x100)])
            yield bridge_case(family, 'head_equals_physical_nav_unrelated' + suffix,
                              on_bridge=on_bridge, stored_head=[1280, 1280, 416])
            yield bridge_case(family, 'null_nav_paid_head' + suffix,
                              on_bridge=on_bridge, nav_com_coord=None)
            for level in (-128, -1, 0, 1, 127):
                yield bridge_case(family, f'signed_level_{level}' + suffix, on_bridge=on_bridge,
                                  cells=[dict(xy=[5, 5], level=level, slope=0, flags=0),
                                         dict(xy=[7, 5], level=0, slope=0, flags=0x100)])
            for slope in range(21):
                yield bridge_case(family, f'slope_{slope}' + suffix, on_bridge=on_bridge,
                                  current=[1293, 1307, 123], stored_head=[1819, 1293, 456], cells=[
                                      dict(xy=[5, 5], level=3, slope=slope, flags=0),
                                      dict(xy=[7, 5], level=-1, slope=20 - slope, flags=0x100)])
            for name, current, head, cells in (
                    ('head_missing_uses_dummy', [1280, 1280, 416], [4864, 1280, 0],
                     [dict(xy=[5, 5], level=4, slope=0, flags=0)]),
                    ('current_missing_uses_dummy', [4864, 1280, 0], [1792, 1280, 416],
                     [dict(xy=[7, 5], level=4, slope=0, flags=0x100)]),
                    ('both_missing_retained_dummy', [-510, 256, 123], [131200, -300, 456], []),
                    ('negative_fraction_real_zero', [-1, -255, 0], [1792, 1280, 17],
                     [dict(xy=[0, 0], level=4, slope=0, flags=0),
                      dict(xy=[7, 5], level=0, slope=0, flags=0x100)]),
                    ('fixed_stride_alias', [1280, 1280, 416], [-256, 256, 17],
                     [dict(xy=[5, 5], level=4, slope=0, flags=0),
                      dict(xy=[511, 0], level=0, slope=0, flags=0x100)]),
                    ('signed_extreme_dummy', [-2147483648, 2147483647, 1],
                     [2147483647, -2147483648, -1], [])):
                yield bridge_case(family, name + suffix, on_bridge=on_bridge, current=current,
                                  stored_head=head, cells=cells,
                                  prefix_queries=[[-256, 0, 999], [2560, 2816, -999]])
            for present, tube in ((True, [10, 20]), (False, [-1, -32768])):
                yield bridge_case(family, f'tube_locomotor_{int(present)}' + suffix,
                                  on_bridge=on_bridge, tube=tube, locomotor_present=present,
                                  stored_head=[0, 0, 0])


def generate_bridge_layers():
    rows = [dict(input=row, output=OriginalBridgeQuery(row).run_bridge()) for row in bridge_inputs()]
    seams = []
    for family in FAMILIES:
        for name, updates, zone_updates in (
                ('paid_head', dict(current=[1283, 1280, 416]), {}),
                ('paid_head_record_missing', dict(current=[1283, 1280, 416]),
                 dict(records=[], raw_labels=[65535, 65535])),
                ('paid_head_split', dict(current=[1283, 1280, 416]),
                 dict(group_cells=[dict(xy=[4, 5], group=1)])),
                ('paid_head_split_equal_labels', dict(current=[1283, 1280, 416]),
                 dict(group_cells=[dict(xy=[4, 5], group=1)], raw_labels=[2, 2])),
                ('up_from_bridge', dict(on_bridge=True, cells=[
                    dict(xy=[5, 5], level=0, slope=0, flags=0),
                    dict(xy=[7, 5], level=4, slope=0, flags=0x100)]), {}),
                ('null_head', dict(stored_head=[0, 0, 0]), {}),
                ('tube_no_locomotor', dict(tube=[-1, -32768], locomotor_present=False,
                                         on_bridge=True, stored_head=[0, 0, 0]), {})):
            row = bridge_case(family, 'response_' + name, **updates)
            victim = dict(family='drive', current=[384, 1408, 777], stored_head=[1152, 1408, -333],
                          on_bridge=False, tube=None, locomotor_present=True)
            movement_zone = {'drive': 0, 'walk': 4, 'ship': 10, 'hover': 3}[family]
            zones = dict(size=[8, 8], bounds=[0, 0, 8, 8], group_cells=[], raw_labels=[2, 3],
                         records=[dict(a=[5, 5], b=[9, 5], active=True, kind=0)])
            zones.update(zone_updates)
            seams.append(dict(input=row, victim=victim, movement_zone=movement_zone, zones=zones,
                              output=OriginalBridgeQuery(row).response_seam(victim, movement_zone, zones)))
    harvest_seams = [dict(input=row['input'], clicked_cell=[4, 5],
                          movement_zone=row['movement_zone'], zones=row['zones'],
                          output=OriginalBridgeQuery(row['input']).harvest_seam(
                              [4, 5], row['movement_zone'], row['zones']))
                     for row in seams if row['input']['family'] == 'drive']
    constructor_rows = []
    for family in ('drive', 'ship'):
        for name, current, on_bridge, frame in (
                ('physical', [1283, 1280, 777], False, 100),
                ('zero', [0, 0, 0], True, 0),
                ('signed', [-257, -1, -999], False, -1)):
            row = bridge_case(family, f'fresh_{family}_constructor_{name}', current=current,
                              stored_head=[0, 0, 0], on_bridge=on_bridge,
                              constructor_prefill_byte=0xA5,
                              scalar_globals={'0x0089E7C0': 104, '0x00AC13C8': 104, '0x00A8ED84': frame})
            constructor_rows.append(OriginalBridgeQuery(row).run_constructed_bridge())
    return dict(rows=rows, response_seams=seams, harvest_seams=harvest_seams,
                constructor_rows=constructor_rows,
                original_spans={name: dict(address=f'0x{address:08X}', bytes=file_span(image_bytes(), address, size)[1].hex())
                                for name, (address, size) in BRIDGE_SPANS.items()},
                caller_roles={'infantry': dict(victim_coordinate='0x0070829C', candidate_coordinate='0x007082DB',
                                                candidate_bridge_layer='0x00708319', reach_zone='0x0070833C'),
                              'unit': dict(victim_coordinate='0x007084F4', candidate_coordinate='0x00708533',
                                           candidate_bridge_layer='0x00708571', reach_zone='0x00708594'),
                              'harvest': dict(candidate_coordinate='0x004DCF31', candidate_bridge_layer='0x004DCF6F',
                                              candidate_type='0x004DCF7A', reach_zone='0x004DCF92')})


def bridge_metadata():
    return provenance(
        scope='Original Foot4DBDF0 and Foot4DDC40→Object5F6A70 with original Map578080/565730 and Cell47B3A0; original708080 and4DCE80 coordinate/layer blocks through original56D100 reachability; six original fresh Drive/Ship constructor controls.',
        assumptions=[
            'The416 supplied-state rows use live Walk/Drive/Ship/Hover ILocomotion and original concrete Infantry/Unit tables. Physical XYZ, retained head XYZ, OnBridge, Tube and NavCom are independent inputs. These rows do not execute object construction, head producers or NavCom lifecycle.',
            'NullCoord=(0,0,0), Cell level scalar89E7C0=104, Object level scalarAC13C8=104 and FPCW/ftol control word0E7F supplied explicitly. OriginalCell47B3A0 lazily initializes its own original double slope coefficients; no height or decision replacements.',
            'Sparse fixed512-stride table with capacity40000 and explicit real-cell signed level/slope/flags. Actual shared Dummy retains supplied level/slope/flags while native lookups stamp its packed coordinate. No map loader, topology, passability or zone-map construction claimed.',
            'Original coordinate and bridge queries preserve candidate/locomotor/real-cell bytes. Outputs record original navigation coordinate, AL verdict, ordered exact-XYZ height/structural queries with selected cell, height result and final Dummy state. Tube rows with a null locomotor execute coordinate override and layer bypass without ground queries.',
            'Boundary311/312/313/416 rows use explicit level/slope/local-coordinate inputs established by original ground execution. Signed/extreme and alias rows are scalar witnesses, not a reachability claim on stock maps.',
            'Response seams supply the surrounding scan register/stack context, candidate typeMovementZone and a concrete Unit victim with independent retained/physical XYZ. Original caller instructions execute through original56D100/56D230/56DA10 with explicit Size8,8 Local0,0,8,8 base/raw/active-record state and stop before threat scoring or at reachability refusal. Earlier eligibility, ranking, inactive-record traversal, map loader/zone producer and dispatch are excluded.',
            'Caller victim+4C addresses are70829C/7084F4; candidate+4C are7082DB/708533; candidate+BC are708319/708571; CanReachZone calls are70833C/708594. All six native reachability arguments are observed; fixture movement-zone choices are supplied rather than INI-loaded.',
            'Seven Drive harvest seams execute original4DCF26→56D100→4DCF97 with originalUnit+84 type accessor. The clicked Cell4,5 and prior harvest admission are supplied. Native LandType, CanEnterCell, scan candidate ordering and ore selection are excluded. These are the same layer/zone protocol, not whole harvesting parity.',
            'Six constructor_rows execute original Drive4AF540 or Ship69EC50 and original base55A6C0 on explicitA5 backing bytes, with NullCoord0 and supplied frame100/0/-1. They record original destination/head/vtables/link/frame/selector and full constructor payload before linking. Linking the Foot pointer is supplied afterward; original4DBDF0/4DDC40 then execute. Physical/zero/signed Foot XYZ and OnBridge are supplied. No complete Unit constructor, COM creation/link lifecycle or producer/scheduler claim.',
        ], substitutions=[], entry_points={
            'foot_coordinate': 0x4DBDF0, 'foot_bridge_layer': 0x4DDC40,
            'object_bridge_layer': 0x5F6A70, 'map_ground': 0x578080,
            'cell_ground': 0x47B3A0, 'map_coordinate_cell': 0x565730,
            'response_infantry_start': 0x708291, 'response_unit_start': 0x7084E9,
            'response_infantry_victim_coordinate': 0x70829C,
            'response_infantry_candidate_coordinate': 0x7082DB,
            'response_infantry_bridge_layer': 0x708319, 'response_infantry_reach': 0x70833C,
            'response_unit_victim_coordinate': 0x7084F4,
            'response_unit_candidate_coordinate': 0x708533,
            'response_unit_bridge_layer': 0x708571, 'response_unit_reach': 0x708594,
            'reach_zone': 0x56D100, 'get_zone': 0x56D230,
            'record_lookup': 0x56DA10, 'playfield': 0x578460,
            'harvest_coordinate': 0x4DCF31, 'harvest_bridge_layer': 0x4DCF6F,
            'harvest_type': 0x4DCF7A, 'harvest_reach': 0x4DCF92, 'harvest_stop_boundary': 0x4DCF97,
            'locomotor_base_constructor': 0x55A6C0, 'drive_constructor': 0x4AF540,
            'ship_constructor': 0x69EC50,
        })


BASE_RESPONSE_SPANS = {
    'rules_constructor': (0x665650, 0x3240),
    'response_ai_reader': (0x673E41, 0x20),
    'response_general_readers': (0x670B7F, 0x6D),
    'occupant_threat_reader': (0x67011B, 0x20),
    'type_threat_reader': (0x7149C7, 0x1A),
    'type_speed_reader': (0x71464A, 0x55),
    'type_image_reader': (0x5F92F8, 0x48),
    'building_bunker_reader': (0x46093A, 0x20),
    'threat_posed': (0x708B40, 0x71),
    'building_garrison_count': (0x4581F0, 0x7),
    'evaluate_target_threat': (0x4D97A0, 0x110),
    'response': (0x708080, 0x735),
    'object_distance': (0x5F6360, 0xD5),
    'sqrt_approx': (0x4CAC40, 0x6E),
    'ftol': (0x7C5F00, 0x3D),
    'queue_mission': (0x5B35E0, 0x65),
    'techno_assign_target': (0x6FCDB0, 0x1E5),
    'infantry_assign_target': (0x51B1F0, 0x159),
    'infantry_do_action': (0x51D6F0, 0x3F3),
    'random_seed': (0x65C6D0, 0xAF),
    'random_ranged': (0x65C7E0, 0xB2),
    'get_weapon_range': (0x7012C0, 0xDB),
    'get_type_speed': (0x70EFE0, 0x16),
    'bunker_mission': (0x44B780, 0x40),
    'bunker_install': (0x458E50, 0x549),
    'bunker_release_death': (0x4593A0, 0x21D),
    'bunker_release_normal': (0x4595C0, 0x271),
}


class BaseResponseNative:
    """The existing retail reader machine, with observations of original bodies.

    No scoring, sort, RNG, mission or target result is supplied by a hook.
    Object payloads/class-array entries and selection-stage scores are declared
    fixture inputs. Heap/TLS/archive boundaries belong to BulletReader/Reader.
    """

    def __init__(self):
        from tools.projectile_oracle.bridge_render_inputs import BulletReader, assets_root, lexical
        from tools.spatial_oracle.building_body_rules import INI, RULES as RULES_INI, SP as READER_SP

        self.root = Path(os.environ.get('VERA20K_BASE_RESPONSE_ASSETS', str(assets_root())))
        self.lexical, self.ini, self.rules_ini, self.sp = lexical, INI, RULES_INI, READER_SP
        art_path = self.root / 'ARTMD.INI'
        art_raw = art_path.read_bytes()
        art, _ = lexical(art_raw, {'E1', 'GI', 'GISequence'})
        self.m = BulletReader(art, self.root)
        self.u = self.m.u
        self.originals = {name: bytes(self.u.mem_read(address, size))
                          for name, (address, size) in BASE_RESPONSE_SPANS.items()}
        self.vtables = {name: bytes(self.u.mem_read(pointer, 0x600)) for name, pointer in
                       [('unit', 0x7F5C70), ('infantry', 0x7EB058), ('building', 0x7E3EBC)]}
        self.visited, self.events, self.writes, self.field_names = set(), [], [], {}
        self.ptr_names = {0: 'null'}
        self.record = False
        self.u.hook_add(UC_HOOK_CODE, self.observe)
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.observe_write)
        self.start()
        self.rules = self.m.alloc(0x2000)
        self.m.invoke(0x665650, self.rules)
        self.u.mem_write(0x8871E0, dwords(self.rules))
        for address in (0xA8EB00, 0xA83CE0, 0x887568):
            self.u.mem_write(address, dwords(0x7EB6D4, self.m.alloc(4096), 1024, 1, 0, 10))
        self.scenario = self.m.read32(0xA8B230)
        self.types, self.weapons = {}, {}
        for name in ('E1', 'MTNK', 'HTNK', 'HARV', 'ENGINEER', 'NATBNK'):
            pointer = self.m.alloc(0x1900)
            constructor = 0x5236A0 if name in ('E1', 'ENGINEER') else 0x45DD90 if name == 'NATBNK' else 0x7470D0
            self.m.invoke(constructor, pointer, (self.m.cstring(name),))
            self.types[name] = pointer
        self.constructor = dict(rules=self.rules_state(), types=self.type_state(),
                                **self.finish((0x665650, 0x5236A0, 0x7470D0, 0x45DD90)))
        self.files = [dict(file='ARTMD.INI', sha256=hashlib.sha256(art_raw).hexdigest())]

    def signed(self, address):
        return struct.unpack('<i', self.u.mem_read(address, 4))[0]

    def who(self, pointer):
        return self.ptr_names.get(pointer, f'0x{pointer:08X}')

    def observe(self, u, pc, _size, _data):
        if not self.record:
            return
        self.visited.add(pc)
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x5276D0, 0x5283D0, 0x5295F0, 0x474620):
            self.events.append(dict(reader=f'0x{pc:08X}', section=self.m.string(self.m.read32(sp + 4)),
                                    key=self.m.string(self.m.read32(sp + 8))))
        elif pc in (0x71465F, 0x71466E, 0x71467D, 0x714699):
            stage, register = {0x71465F: ('speed_read_int', UC_X86_REG_EAX),
                               0x71466E: ('speed_upper_clamped', UC_X86_REG_EAX),
                               0x71467D: ('speed_nonnegative_raw', UC_X86_REG_ECX),
                               0x714699: ('speed_converted', UC_X86_REG_EDX)}[pc]
            self.events.append({stage: struct.unpack('<i', dwords(u.reg_read(register)))[0]})
        elif pc == 0x708B40:
            self.events.append(dict(threat=self.who(u.reg_read(UC_X86_REG_ECX))))
        elif pc == 0x4581F0:
            self.events.append(dict(garrison_count=self.signed(u.reg_read(UC_X86_REG_ECX) + 0x694)))
        elif pc == 0x4D9838:
            self.events.append(dict(distance=u.reg_read(UC_X86_REG_EAX)))
        elif pc == 0x4D9857:
            self.events.append(dict(beyond=struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EDI)))[0],
                                    shifted_threat=struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EBX)))[0]))
        elif pc == 0x65C7E0:
            self.events.append(dict(rng=f'0x{pc:08X}', bounds=[self.signed(sp + 4), self.signed(sp + 8)]))
        elif pc == 0x65C84B:
            rng = u.reg_read(UC_X86_REG_EDX)
            self.events.append(dict(raw_draw=u.reg_read(UC_X86_REG_ESI), stream='Scenario+218',
                                    indices=[self.m.read32(rng + 4), self.m.read32(rng + 8)]))
        elif pc == 0x65C880:
            self.events.append(dict(masked_draw=u.reg_read(UC_X86_REG_EAX),
                                    upper_span=u.reg_read(UC_X86_REG_EDI)))
        elif pc == 0x7086F5:
            self.events.append(dict(draw=u.reg_read(UC_X86_REG_EAX)))
        elif pc == 0x5B35E0:
            self.events.append(dict(queue=self.who(u.reg_read(UC_X86_REG_ECX)),
                                    mission=self.signed(sp + 4), commence=self.m.read32(sp + 8)))
        elif pc in (0x6FCDB0, 0x51B1F0):
            self.events.append(dict(assign=self.who(u.reg_read(UC_X86_REG_ECX)),
                                    entry=f'0x{pc:08X}', target=self.who(self.m.read32(sp + 4))))
        elif pc == 0x51D6F0:
            self.events.append(dict(action=self.who(u.reg_read(UC_X86_REG_ECX)),
                                    requested=self.signed(sp + 4)))
        elif pc == 0x708744:
            self.events.append(dict(assigned=self.signed(sp + 0x24),
                                    next_sum=struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_ECX)))[0],
                                    budget=struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]))

    def observe_write(self, u, _access, address, size, value, _data):
        if self.record and address in self.field_names:
            name = self.field_names[address]
            self.writes.append(dict(field=name, bytes=size, value=value,
                                    signed=struct.unpack('<i', dwords(value))[0] if size == 4 else value,
                                    pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}'))

    def start(self):
        self.visited, self.events, self.writes = set(), [], []
        self.record = True

    def finish(self, required):
        self.record = False
        assert all(pc in self.visited for pc in required), [hex(pc) for pc in required if pc not in self.visited]
        assert self.originals == {name: bytes(self.u.mem_read(address, size))
                                  for name, (address, size) in BASE_RESPONSE_SPANS.items()}
        assert self.vtables == {name: bytes(self.u.mem_read(pointer, 0x600)) for name, pointer in
                               [('unit', 0x7F5C70), ('infantry', 0x7EB058), ('building', 0x7E3EBC)]}
        return dict(events=self.events, writes=self.writes, required_addresses=[f'0x{pc:08X}' for pc in required],
                    original_code_and_vtables_unchanged=True)

    def rules_state(self):
        return dict(computer_base_defense_response=self.signed(self.rules + 0xB18),
                    threat_per_occupant=self.signed(self.rules + 0xDF4),
                    base_defense_delay_bits=f'{self.m.read32(self.rules + 0x14DC):08X}{self.m.read32(self.rules + 0x14D8):08X}',
                    suspend_priority=self.signed(self.rules + 0x14E0),
                    suspend_delay_bits=f'{self.m.read32(self.rules + 0x14EC):08X}{self.m.read32(self.rules + 0x14E8):08X}')

    def type_state(self):
        return {name: dict(threat=self.signed(pointer + 0x670), speed=self.signed(pointer + 0x678),
                           image=self.m.string(pointer + 0x1F8),
                           bunker=self.u.mem_read(pointer + 0x16AB, 1)[0] if name == 'NATBNK' else None)
                for name, pointer in self.types.items()}

    def speed_getter(self, name):
        pointer = self.entity(name, name)
        self.start()
        value = self.m.invoke(0x70EFE0, pointer)
        return dict(type=name, output=struct.unpack('<i', dwords(value))[0],
                    **self.finish((0x70EFE0,)))

    def read_layer(self, sections):
        self.m.rules_cache(sections)
        self.start()
        for begin, end in ((0x673E41, 0x673E61), (0x67011B, 0x67013B), (0x670B7F, 0x670BEC)):
            for reg, value in ((UC_X86_REG_ESP, self.sp), (UC_X86_REG_ESI, self.rules),
                               (UC_X86_REG_EDI, self.rules_ini)):
                self.u.reg_write(reg, value)
            run_checked(self.u, begin, end)
        admitted = []
        for name, pointer in self.types.items():
            # Original526810 returns the section pointer, not a boolean byte.
            # Testing AL would drop any supplied section at an xx00 address.
            if not self.m.invoke(0x526810, self.rules_ini, (pointer + 0x24,)):
                continue
            admitted.append(name)
            self.u.mem_write(self.sp + 0x1B0, dwords(self.rules_ini))
            self.u.reg_write(UC_X86_REG_ESP, self.sp)
            self.u.reg_write(UC_X86_REG_EBX, pointer)
            run_checked(self.u, 0x5F92F8, 0x5F9340)
            for begin, end in ((0x7149C7, 0x7149E1), (0x71464A, 0x71469F)):
                for reg, value in ((UC_X86_REG_ESP, self.sp), (UC_X86_REG_EBP, pointer),
                                   (UC_X86_REG_EBX, pointer + 0x24), (UC_X86_REG_ESI, self.rules_ini),
                                   (UC_X86_REG_EDI, self.rules_ini)):
                    self.u.reg_write(reg, value)
                run_checked(self.u, begin, end)
            if name == 'NATBNK':
                self.u.reg_write(UC_X86_REG_ESP, self.sp)
                self.u.reg_write(UC_X86_REG_EBP, pointer)
                self.u.reg_write(UC_X86_REG_EBX, pointer + 0x24)
                self.u.reg_write(UC_X86_REG_ESI, self.rules_ini)
                run_checked(self.u, 0x46093A, 0x46095A)
        trace = self.finish((0x673E41, 0x67011B, 0x670B7F, 0x5276D0, 0x5283D0))
        return dict(rules=self.rules_state(), types=self.type_state(), admitted_types=admitted, **trace)

    def physical_layers(self, *, record_files=True):
        rows, retained = [], {}
        wanted = set(self.types) | {'General', 'AI', 'M60', '105mm', '120mm'}
        for name in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'):
            path = self.root / name
            if not path.exists():
                assert name == 'LANGRULE.INI', path
                rows.append(dict(file=name, absent=True))
                continue
            raw = path.read_bytes()
            sections, _ = self.lexical(raw, wanted)
            selected = {n: {k: v for k, v in entries.items() if k in
                           ('ThreatPosed', 'Speed', 'Image', 'Bunker', 'Primary', 'Cost', 'Range',
                            'ComputerBaseDefenseResponse', 'BaseDefenseDelay', 'SuspendPriority',
                            'SuspendDelay', 'ThreatPerOccupant')} for n, entries in sections.items()}
            for n, entries in sections.items():
                retained.setdefault(n, {}).update(entries)
            sha = hashlib.sha256(raw).hexdigest()
            if record_files:
                self.files.append(dict(file=name, sha256=sha))
            rows.append(dict(file=name, sha256=sha, inputs=selected, output=self.read_layer(sections)))
        self.m.rules_cache(retained)
        range_rows = []
        for name in ('E1', 'MTNK', 'HTNK'):
            # Binding is a supplied fixture boundary, not a TechnoType Primary
            # reader claim. The physical name and original Weapon ctor/default
            # and ReadRange result independently establish the range input.
            weapon_name = retained[name]['Primary']
            weapon = self.m.alloc(0x200)
            self.m.invoke(0x771C70, weapon, (self.m.cstring(weapon_name),))
            default = self.signed(weapon + 0xB4)
            self.start()
            value = self.m.invoke(0x474620, self.rules_ini,
                                  (self.m.cstring(weapon_name), self.m.cstring('Range'), default))
            self.u.mem_write(weapon + 0xB4, dwords(value))
            self.u.mem_write(self.types[name] + 0x898, dwords(weapon))
            self.weapons[name] = weapon
            range_rows.append(dict(name=name, primary_name=weapon_name, raw_range=retained[weapon_name]['Range'],
                                   constructor_range=default, native_range=self.signed(weapon + 0xB4),
                                   **self.finish((0x474620, 0x5283D0))))
        self.start()
        self.m.invoke(0x523D00, self.types['E1'])
        sequence = self.m.read32(self.types['E1'] + 0xE3C)
        sequence_row = dict(image=self.m.string(self.types['E1'] + 0x1F8),
                            ready=list(struct.unpack('<9i', self.u.mem_read(sequence, 36))),
                            **self.finish((0x523D00, 0x528A10)))
        return dict(layers=rows, ranges=range_rows, infantry_sequence=sequence_row)

    def entity(self, name, label, *, xyz=(1280, 1280, 416), threat=None, speed=None):
        pointer = self.m.alloc(0x1000)
        self.ptr_names[pointer] = label
        infantry = name in ('E1', 'ENGINEER')
        building = name == 'NATBNK'
        table = 0x7EB058 if infantry else 0x7E3EBC if building else 0x7F5C70
        self.u.mem_write(pointer, dwords(table))
        self.u.mem_write(pointer + (0x6C0 if infantry else 0x520 if building else 0x6C4), dwords(self.types[name]))
        self.u.mem_write(pointer + 0x14, dwords(3 if building else 5))
        self.u.mem_write(pointer + 0x6C, dwords(100))
        self.u.mem_write(pointer + 0x90, b'\1')
        self.u.mem_write(pointer + 0x9C, dwords(*xyz))
        self.u.mem_write(pointer + 0xAC, dwords(5))
        self.u.mem_write(pointer + 0xB4, dwords(-1))
        self.u.mem_write(pointer + 0x684, b'\xff')
        self.u.mem_write(pointer + 0x50C, b'\1')
        if infantry:
            self.u.mem_write(pointer + 0x6C4, dwords(-1))
        if threat is not None:
            self.u.mem_write(self.types[name] + 0x670, dwords(threat))
        if speed is not None:
            self.u.mem_write(self.types[name] + 0x678, dwords(speed))
        for offset, field in ((0xB4, 'queue'), (0x218, 'archive'), (0x2B4, 'target'), (0x50C, 'passive'),
                              (0x650, 'timer_start'), (0x654, 'timer_aux'), (0x658, 'timer_duration'),
                              (0x6C4, 'doing'), (0x5E0, 'path_field'), (0x68D, 'firing_latch')):
            self.field_names[pointer + offset] = f'{label}.{field}'
        return pointer

    def score(self, row):
        candidate = self.entity(row['type'], 'candidate', xyz=row['current'],
                                threat=row['threat'], speed=row['speed'])
        attacker = self.entity('HTNK', 'attacker', xyz=row['attacker'])
        # A single type is shared by HTNK controls; restore the requested
        # source scalar after constructing the attacker of the same type.
        self.u.mem_write(self.types[row['type']] + 0x670, dwords(row['threat']))
        self.u.mem_write(self.weapons[row['type']] + 0xB4, dwords(row['range']))
        if row.get('target') == 'attacker':
            self.u.mem_write(candidate + 0x2B4, dwords(attacker))
        if row.get('harvest'):
            self.u.mem_write(candidate + 0xAC, dwords(10))
        if 'team_base_defense' in row:
            team, team_type = self.m.alloc(0x100), self.m.alloc(0x200)
            self.u.mem_write(team + 0x24, dwords(team_type))
            self.u.mem_write(team_type + 0xF6, bytes((int(row['team_base_defense']),)))
            self.u.mem_write(candidate + 0x5D4, dwords(team))
        self.start()
        result = self.m.invoke(0x4D97A0, candidate, (0 if row.get('null_attacker') else attacker,))
        required = [0x4D97A0]
        if any('distance' in event for event in self.events):
            required += [0x5F6360, 0x4CAC40, 0x7C5F00, 0x7012C0, 0x708B40]
        return dict(input=row, output=struct.unpack('<i', dwords(result))[0], **self.finish(required))

    def threat_posed(self, row):
        owner = self.entity(row['type'], 'owner')
        # Clone the original constructed type for supplied resident-field
        # controls so synthetic threats do not alter the retail score inputs.
        offset = 0x520 if row['type'] == 'NATBNK' else 0x6C4
        typ = self.m.alloc(0x1900)
        self.u.mem_write(typ, bytes(self.u.mem_read(self.types[row['type']], 0x1900)))
        self.u.mem_write(typ + 0x670, dwords(row['own_threat']))
        self.u.mem_write(owner + offset, dwords(0 if row.get('null_type') else typ))
        self.u.mem_write(owner + 0x694, dwords(row.get('garrison_count', 0)))
        if 'linked_threat' in row:
            occupant = self.entity('MTNK', 'bunker_occupant')
            linked_type = self.m.alloc(0x1900)
            self.u.mem_write(linked_type, bytes(self.u.mem_read(self.types['MTNK'], 0x1900)))
            self.u.mem_write(linked_type + 0x670, dwords(row['linked_threat']))
            self.u.mem_write(occupant + 0x6C4, dwords(linked_type))
            self.u.mem_write(owner + 0x2E4, dwords(occupant))
            self.u.mem_write(occupant + 0x2E4, dwords(owner))
        retained = self.m.read32(self.rules + 0xDF4)
        self.u.mem_write(self.rules + 0xDF4, dwords(row.get('threat_per_occupant', 10)))
        self.start()
        result = self.m.invoke(0x708B40, owner)
        trace = self.finish((0x708B40,))
        self.u.mem_write(self.rules + 0xDF4, dwords(retained))
        return dict(input=row, output=struct.unpack('<i', dwords(result))[0], **trace)

    def selection(self, entries, budget, anchor):
        self.u.mem_write(self.sp, bytes(0x100))
        self.u.mem_write(self.sp + 0x18, dwords(budget))
        victim = self.entity('HARV', 'victim')
        self.u.mem_write(victim + 0x218, dwords(victim if anchor else 0))
        pointers, history = {}, []
        for id_, score, family in entries:
            if id_ not in pointers:
                pointers[id_] = self.m.alloc(0x1000)
            pointer = pointers[id_]
            self.ptr_names[pointer] = str(id_)
            count = self.m.read32(self.sp + 0x14)
            for reg, value in ((UC_X86_REG_ESP, self.sp), (UC_X86_REG_EAX, score & 0xFFFFFFFF),
                               (UC_X86_REG_EBX, victim), (UC_X86_REG_ESI, pointer),
                               (UC_X86_REG_EBP, count * 4)):
                self.u.reg_write(reg, value)
            begin, end = (0x708351, 0x7083BC) if family == 'infantry' else (0x7085AD, 0x708622)
            self.start()
            run_checked(self.u, begin, end)
            count = self.m.read32(self.sp + 0x14)
            history.append(dict(id=id_, raw_score=score, family=family, remaining=self.signed(self.sp + 0x18),
                                minimum=self.signed(self.sp + 0x1C),
                                scores=list(struct.unpack('<' + 'i' * count, self.u.mem_read(self.sp + 0x54, count * 4))),
                                ids=[self.who(self.m.read32(self.sp + 0x6C + index * 4)) for index in range(count)],
                                **self.finish((begin,))))
        return dict(input=dict(entries=entries, budget=budget, victim_is_self_anchor=anchor), output=history)

    def sort(self, scores):
        self.u.mem_write(self.sp, bytes(0x100))
        self.u.mem_write(self.sp + 0x14, dwords(len(scores)))
        self.u.mem_write(self.sp + 0x54, dwords(*scores))
        self.u.mem_write(self.sp + 0x6C, dwords(*range(1, len(scores) + 1)))
        self.u.reg_write(UC_X86_REG_ESP, self.sp)
        self.start()
        run_checked(self.u, 0x708647, 0x7086AF)
        return dict(input=scores, scores=list(struct.unpack('<' + 'i' * len(scores),
                                                           self.u.mem_read(self.sp + 0x54, len(scores) * 4))),
                    ids=list(struct.unpack('<' + 'i' * len(scores), self.u.mem_read(self.sp + 0x6C, len(scores) * 4))),
                    **self.finish((0x708647,)))

    def dispatch(self, row):
        victim = self.entity('HARV', 'victim')
        attacker = self.entity('HTNK', 'attacker')
        self.u.mem_write(attacker + 0x14, dwords(row.get('attacker_flags', 5)))
        self.u.mem_write(attacker + 0x650, dwords(-1, 777, 0))
        objects = {label: self.entity(name, label, threat=threat) for label, name, threat in row['objects']}
        for label in row.get('base_defense_team', []):
            team, team_type = self.m.alloc(0x100), self.m.alloc(0x200)
            self.u.mem_write(team + 0x24, dwords(team_type))
            self.u.mem_write(team_type + 0xF6, b'\1')
            self.u.mem_write(objects[label] + 0x5D4, dwords(team))
        self.u.mem_write(self.rules + 0x14D8, struct.pack('<Q', int(row.get('delay_bits', '3fd0000000000000'), 16)))
        self.u.mem_write(0xA8ED84, dwords(row.get('frame', 41)))
        self.m.invoke(0x65C6D0, self.scenario + 0x218, (row.get('seed', 31),))
        rng_before = bytes(self.u.mem_read(self.scenario + 0x218, 0x3F4)).hex()
        self.u.mem_write(self.sp, bytes(0x100))
        self.u.mem_write(self.sp + 0x14, dwords(len(row['selected']), row['budget']))
        self.u.mem_write(self.sp + 0x24, dwords(row.get('initial_assigned', 0)))
        self.u.mem_write(self.sp + 0x30, dwords(victim))
        self.u.mem_write(self.sp + 0x4C, dwords(row.get('stack_aux', 0x12345678)))
        self.u.mem_write(self.sp + 0x54, dwords(*row['scores']))
        self.u.mem_write(self.sp + 0x6C, dwords(*(objects[label] for label in row['selected'])))
        self.u.mem_write(self.sp + 0x84, dwords(RET_MAGIC, attacker))
        self.u.reg_write(UC_X86_REG_ESP, self.sp)
        self.start()
        run_checked(self.u, 0x708647, RET_MAGIC, count=200000)
        required = [0x708647]
        if row['selected']:
            required += [0x65C7E0, 0x5B35E0, 0x708B40, 0x6FCDB0]
        trace = self.finish(required)
        return dict(input=row, cooldown=list(struct.unpack('<3i', self.u.mem_read(attacker + 0x650, 12))),
                    assigned=self.signed(self.sp + 0x24),
                    rng_before=rng_before, rng_after=bytes(self.u.mem_read(self.scenario + 0x218, 0x3F4)).hex(),
                    final={label: dict(queue=self.signed(pointer + 0xB4), target=self.who(self.m.read32(pointer + 0x2B4)),
                                       archive=self.who(self.m.read32(pointer + 0x218)),
                                       passive=self.u.mem_read(pointer + 0x50C, 1)[0],
                                       doing=self.signed(pointer + 0x6C4) if name == 'E1' else None)
                           for label, name, _ in row['objects'] for pointer in (objects[label],)}, **trace)

    def infantry_assignment(self):
        """Full original E1 class setter over fixed physical GISequence.

        Selected type readers execute; current action/lifecycle/target links are
        supplied. The action, common setter and their call order are observed,
        never answered by a hook. This is separate from generic DoAction tools.
        """
        from tools.native_oracle import IMAGE_BASE, _sections

        code_regions = [(IMAGE_BASE + rva, bytes(self.u.mem_read(IMAGE_BASE + rva, raw_size)))
                        for rva, _, raw_size, _, flags in _sections(image_bytes())
                        if raw_size and flags & 0x20000000]
        action_flags = bytes(self.u.mem_read(0x7EAF7C, 42 * 4))
        physical = self.physical_layers()
        typ = self.types['E1']
        sequence = self.m.read32(typ + 0xE3C)
        sequence_records = [dict(index=index,
                            record=list(struct.unpack('<9i', self.u.mem_read(sequence + index * 36, 36))),
                            action_flags=list(action_flags[index * 4:index * 4 + 4])) for index in range(42)]
        # A5 proves the constructor owns this byte, rather than merely reading
        # the zero-filled bump heap as its default. Original5236A0 reaches the
        # actual TechnoType false store711144.
        constructor_type = self.m.alloc(0x1900)
        self.u.mem_write(constructor_type, b'\xA5' * 0x1900)
        self.field_names[constructor_type + 0x6AC] = 'constructor_type.deploy_fire'
        self.start()
        self.m.invoke(0x5236A0, constructor_type, (self.m.cstring('E1'),))
        constructor = dict(backing_byte='A5', byte=self.u.mem_read(constructor_type + 0x6AC, 1)[0],
                           **self.finish((0x5236A0, 0x711144)))
        literal = self.m.string(0x843AA0)
        assert literal == 'DeployFire'
        def read_deploy_fire(sections):
            self.m.rules_cache(sections)
            before = self.u.mem_read(typ + 0x6AC, 1)[0]
            self.start()
            admitted = bool(self.m.invoke(0x526810, self.rules_ini, (typ + 0x24,)))
            if admitted:
                for reg, value in ((UC_X86_REG_ESP, self.sp), (UC_X86_REG_EBP, typ),
                                   (UC_X86_REG_EBX, typ + 0x24), (UC_X86_REG_EDI, self.rules_ini)):
                    self.u.reg_write(reg, value)
                run_checked(self.u, 0x7147E8, 0x714802)
            required = (0x526810, 0x7147E8, 0x5295F0, 0x7147FC) if admitted else (0x526810,)
            return dict(input=sections, admitted=admitted, before=before,
                        after=self.u.mem_read(typ + 0x6AC, 1)[0], **self.finish(required))
        type_layers = []
        for name in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'):
            path = self.root / name
            if not path.exists():
                type_layers.append(dict(file=name, absent=True))
                continue
            raw = path.read_bytes()
            sections, _ = self.lexical(raw, {'E1'})
            selected = {name: {key: value for key, value in entries.items() if key == literal}
                        for name, entries in sections.items()}
            type_layers.append(dict(file=name, sha256=hashlib.sha256(raw).hexdigest(),
                                    **read_deploy_fire(selected)))
        controls = [dict(name=name, **read_deploy_fire(sections)) for name, sections in (
                    ('missing_retains', {'E1': {'Other': '1'}}),
                    ('wrong_case_retains', {'E1': {'deployfire': 'no'}}),
                    ('malformed_retains_true', {'E1': {'DeployFire': 'junk'}}),
                    ('map_false', {'E1': {'DeployFire': 'no'}}),
                    ('missing_retains_false', {'E1': {'Other': '1'}}),
                    ('malformed_retains_false', {'E1': {'DeployFire': 'junk'}}),
                    ('map_true', {'E1': {'DeployFire': 'yes'}}))]
        actor = [0]
        pending_action_returns = {}
        def state(pointer):
            return dict(doing=self.signed(pointer + 0x6C4), target=self.who(self.m.read32(pointer + 0x2B4)),
                        passive=self.u.mem_read(pointer + 0x50C, 1)[0],
                        firing_latch=self.u.mem_read(pointer + 0x68D, 1)[0],
                        byte68e=self.u.mem_read(pointer + 0x68E, 1)[0], path_field=self.signed(pointer + 0x5E0),
                        frame=self.signed(pointer + 0xF8),
                        action_timer=list(struct.unpack('<3i', self.u.mem_read(pointer + 0x100, 12))),
                        action_repeat=self.signed(pointer + 0x10C), prone=self.u.mem_read(pointer + 0x6DB, 1)[0],
                        pair2a8=self.who(self.m.read32(pointer + 0x2A8)))
        def observe(_u, pc, _size, _data):
            if not self.record or not actor[0]:
                return
            if pc in pending_action_returns:
                self.events.append(dict(action_return=self.u.reg_read(UC_X86_REG_EAX) & 255,
                                        return_address=f'0x{pc:08X}', state=state(actor[0])))
                pending_action_returns.pop(pc)
            if pc == 0x51D6F0:
                stack = self.u.reg_read(UC_X86_REG_ESP)
                pending_action_returns[self.m.read32(stack)] = True
                self.events.append(dict(action_args=list(struct.unpack('<3i', self.u.mem_read(stack + 4, 12))),
                                        state=state(actor[0])))
            elif pc in (0x51B1F0, 0x6FCDB0, 0x51B293, 0x51B33F):
                self.events.append(dict(at=f'0x{pc:08X}', state=state(actor[0])))
        observe_hook = self.u.hook_add(UC_HOOK_CODE, observe)
        cases = [dict(name=f'doing_{doing}_target_{target}', doing=doing, target=target,
                      prone=int(doing in (2, 8))) for doing in (-1, 0, 2, 4, 8, 27, 28, 29, 30, 31, 11, 33)
                 for target in ('assign', 'same')]
        cases += [dict(name=f'prone_{prone}_doing_{doing}', doing=doing, prone=prone)
                  for doing in (-1, 0, 4, 27, 29, 31) for prone in (0, 1)]
        cases += [dict(name=f'health_{health}_doing_{doing}', doing=doing, health=health,
                       prone=int(doing == 2)) for health in (0, -1) for doing in (-1, 2, 27, 29, 31)]
        cases += [dict(name=f'deployfire_false_{doing}_{target}', doing=doing, target=target,
                       deploy_fire_raw='no') for doing in (27, 28, 29, 30) for target in ('assign', 'same')]
        cases += [dict(name=f'target_{target}', doing=4, target=target)
                  for target in ('replace', 'clear', 'same_null')]
        cases += [dict(name='falling_paradrop', doing=33, falling=1),
                  dict(name='pair2a8_cleanup', doing=4, pair2a8=True),
                  dict(name='pair2a8_cleanup_even_when_deploy_gate_blocks', doing=27,
                       pair2a8=True, deploy_fire_raw='no'),
                  dict(name='same_pair2a8_preserved', doing=4, target='same', pair2a8=True)]
        rows = []
        try:
            for updates in cases:
                row = dict(health=100, target='assign', prone=0, doing=-1, deploy_fire_raw='yes',
                           falling=0, frame=100, seed=31, path_field=123, firing_latch=1, byte68e=1,
                           old_frame=7, old_action_timer=[17, 305419896, 91], old_action_repeat=92,
                           object_byte74=0, on_bridge=False, carry_link2dc=0, navcom=0)
                row.update(updates)
                # Synthetic overrides pass the actual retained bool reader;
                # no row writes a DeployFire result directly into the type.
                read_deploy_fire({'E1': {'DeployFire': row['deploy_fire_raw']}})
                owner = self.entity('E1', 'E1')
                attacker = self.entity('HTNK', 'new_target')
                old = self.entity('MTNK', 'old_target')
                actor[0] = owner
                pending_action_returns.clear()
                old_target, requested = {'assign': (0, attacker), 'same': (attacker, attacker),
                                        'replace': (old, attacker), 'clear': (old, 0),
                                        'same_null': (0, 0)}[row['target']]
                self.u.mem_write(owner + 0x2B4, dwords(old_target))
                self.u.mem_write(owner + 0x6C4, dwords(row['doing']))
                self.u.mem_write(owner + 0x6C, dwords(row['health']))
                self.u.mem_write(owner + 0x6DB, bytes((row['prone'],)))
                self.u.mem_write(owner + 0x8D, bytes((row['falling'],)))
                self.u.mem_write(owner + 0x5E0, dwords(row['path_field']))
                self.u.mem_write(owner + 0x68D, bytes((row['firing_latch'],)))
                self.u.mem_write(owner + 0x68E, bytes((row['byte68e'],)))
                self.u.mem_write(owner + 0xF8, dwords(row['old_frame']))
                self.u.mem_write(owner + 0x100, dwords(*row['old_action_timer'], row['old_action_repeat']))
                if row.get('pair2a8'):
                    self.u.mem_write(owner + 0x2A8, dwords(old))
                    self.u.mem_write(old + 0x2A8, dwords(owner))
                for offset, field in ((0x68E, 'byte68e'), (0xF8, 'frame'), (0x100, 'action_start'),
                                      (0x104, 'action_aux'), (0x108, 'action_duration'),
                                      (0x10C, 'action_repeat'), (0x6DB, 'prone'), (0x2A8, 'pair2a8')):
                    self.field_names[owner + offset] = f'E1.{field}'
                self.field_names[old + 0x2A8] = 'old_target.pair2a8'
                self.u.mem_write(0xA8ED84, dwords(row['frame']))
                self.m.invoke(0x65C6D0, self.scenario + 0x218, (row['seed'],))
                rng_before = bytes(self.u.mem_read(self.scenario + 0x218, 0x3F4)).hex()
                before = state(owner)
                self.start()
                self.m.invoke(0x51B1F0, owner, (requested,))
                trace = self.finish((0x51B1F0,))
                assert not pending_action_returns
                assert all(code == bytes(self.u.mem_read(address, len(code))) for address, code in code_regions)
                assert action_flags == bytes(self.u.mem_read(0x7EAF7C, 42 * 4))
                rows.append(dict(input=row, before=before, after=state(owner),
                                 old_target_pair2a8=self.who(self.m.read32(old + 0x2A8)),
                                 rng_before=rng_before,
                                 rng_after=bytes(self.u.mem_read(self.scenario + 0x218, 0x3F4)).hex(),
                                 full_executable_sections_unchanged=True, **trace))
        finally:
            self.u.hook_del(observe_hook)
        return dict(type='E1', image=self.m.string(typ + 0x1F8), physical_fixture=physical,
                    retained_type_fields=dict(movement_zone=self.signed(typ + 0x5B4),
                    crawls=self.u.mem_read(typ + 0xEBD, 1)[0], can_c4=self.u.mem_read(typ + 0xEBE, 1)[0]),
                    deploy_fire=dict(offset='0x6AC', key_address='0x00843AA0', key=literal,
                    reader_start='0x007147E8', store='0x007147FC', constructor=constructor,
                    layers=type_layers, controls=controls), sequence_records=sequence_records, rows=rows,
                    original_executable_sections=[dict(address=f'0x{address:08X}', bytes=len(code),
                    sha256=hashlib.sha256(code).hexdigest()) for address, code in code_regions])


def whole_base_response():
    """Compose the existing physical empty-FV fixture with original708080.

    The original admission/query/scorer/selection/dispatch execute from entry to
    return. World, Houses, registry entries and object lifecycle are supplied;
    no damage receiver, ToProtect trigger, populated Team or map-loader claim.
    """
    from tools.spatial_oracle.bridge_target_composed import setup

    m, victim, attacker, typ, weapon, cells, rules, physical = setup()
    u = m.u
    assert physical['layers'][0]['type_admitted'], 'Inherited FV readers must have executed'
    m.uc = u  # The retained-zone fixture accepts the existing machine protocol.
    zones = dict(size=[20, 20], bounds=[0, 0, 41, 41], group_cells=[], raw_labels=[1], records=[])
    OriginalBridgeQuery.setup_zones(m, 0, zones)
    u.mem_write(0xA83DF8, dwords(0))
    u.mem_write(0x8B40F8, dwords(0))
    u.mem_write(m.read32(victim + 0x21C) + 0x1EC, b'\0')
    u.mem_write(victim + 0x421, b'\0')
    u.mem_write(victim + 0x684, b'\xff')
    u.mem_write(attacker + 0x684, b'\xff')
    u.mem_write(attacker + 0x650, dwords(-1, 0, 0))
    u.mem_write(0xA8ED84, dwords(41))
    scenario = m.read32(0xA8B230)
    m.invoke(0x65C6D0, scenario + 0x218, (31,))
    rng_before = bytes(u.mem_read(scenario + 0x218, 0x3F4)).hex()
    candidates = []
    for _ in range(4):
        candidate = m.alloc(0x1000)
        u.mem_write(candidate, bytes(u.mem_read(victim, 0x1000)))
        u.mem_write(candidate + 0x421, b'\1\1')
        u.mem_write(candidate + 0xB4, dwords(-1))
        loco = m.alloc(0x100)
        m.invoke(0x4AF540, loco)
        u.mem_write(loco + 0xC, dwords(candidate))
        u.mem_write(candidate + 0x674, dwords(loco + 4))
        candidates.append(candidate)
    items = m.alloc(16)
    u.mem_write(items, dwords(*candidates))
    u.mem_write(0x8B410C, dwords(items))
    u.mem_write(0x8B4118, dwords(len(candidates)))
    u.mem_write(SP - 0x200, b'\xa5' * 0x200)
    u.mem_write(SP, dwords(RET_MAGIC, attacker))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, victim)
    local = SP - 0x84
    identities = {0: 'null', victim: 'victim', attacker: 'attacker',
                  **{pointer: f'FV{index + 1}' for index, pointer in enumerate(candidates)}}
    who = lambda pointer: identities.get(pointer, f'0x{pointer:08X}')
    events, writes, visited, field_names = [], [], set(), {}
    for pointer, label in identities.items():
        if pointer:
            for offset, field in ((0xB4, 'queue'), (0x218, 'archive'), (0x2B4, 'target'),
                                  (0x50C, 'passive'), (0x650, 'timer_start'),
                                  (0x654, 'timer_aux'), (0x658, 'timer_duration')):
                field_names[pointer + offset] = f'{label}.{field}'
    field_names[local + 0x4C] = 'caller.stack_aux'
    originals = {name: bytes(u.mem_read(address, size)) for name, (address, size) in
                 (BASE_RESPONSE_SPANS | BRIDGE_SPANS).items()}
    table = bytes(u.mem_read(0x7F5C70, 0x600))
    def signed(address):
        return struct.unpack('<i', u.mem_read(address, 4))[0]
    def observe(_u, pc, _size, _data):
        visited.add(pc)
        stack = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x5F65F0, 0x7258D0, 0x68BCB0, 0x7C8E17, 0x7C8B3D, 0x7D140B, 0x5B40B0):
            raise AssertionError(('unexpected runtime boundary', hex(pc)))
        if pc in (0x708080, 0x7081A9, 0x7083D9, 0x708647, 0x70879A):
            events.append(dict(pc=f'0x{pc:08X}', stack_aux=signed(local + 0x4C)))
        elif pc == 0x7085A9:
            events.append(dict(score=struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0],
                               candidate=who(u.reg_read(UC_X86_REG_ESI))))
        elif pc == 0x65C7E0:
            events.append(dict(rng=f'0x{pc:08X}', bounds=[signed(stack + 4), signed(stack + 8)]))
        elif pc == 0x65C84B:
            rng = u.reg_read(UC_X86_REG_EDX)
            events.append(dict(raw_draw=u.reg_read(UC_X86_REG_ESI), stream='Scenario+218',
                               indices=[m.read32(rng + 4), m.read32(rng + 8)]))
        elif pc == 0x65C880:
            events.append(dict(masked_draw=u.reg_read(UC_X86_REG_EAX), upper_span=u.reg_read(UC_X86_REG_EDI)))
        elif pc == 0x7086F5:
            events.append(dict(draw=u.reg_read(UC_X86_REG_EAX)))
        elif pc == 0x5B35E0:
            events.append(dict(queue=who(u.reg_read(UC_X86_REG_ECX)),
                               mission=signed(stack + 4), commence=m.read32(stack + 8)))
        elif pc == 0x6FCDB0:
            events.append(dict(assign=who(u.reg_read(UC_X86_REG_ECX)), target=who(m.read32(stack + 4))))
    def written(_u, _access, address, size, value, _data):
        if address in field_names:
            writes.append(dict(field=field_names[address], bytes=size,
                               signed=struct.unpack('<i', dwords(value))[0] if size == 4 else value,
                               pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}'))
    code_hook = u.hook_add(UC_HOOK_CODE, observe)
    write_hook = u.hook_add(UC_HOOK_MEM_WRITE, written)
    required = (0x708080, 0x6EC250, 0x4DBDF0, 0x4DDC40, 0x5F6A70,
                0x56D100, 0x4D97A0, 0x708647, 0x65C7E0, 0x5B35E0, 0x6FCDB0, 0x708B40)
    try:
        run_checked(u, 0x708080, RET_MAGIC, count=200000, required_addresses=required)
    finally:
        u.hook_del(code_hook)
        u.hook_del(write_hook)
    assert originals == {name: bytes(u.mem_read(address, size)) for name, (address, size) in
                         (BASE_RESPONSE_SPANS | BRIDGE_SPANS).items()}
    assert table == bytes(u.mem_read(0x7F5C70, 0x600))
    return dict(physical_fixture=physical, input=dict(type='FV', source='direct original708080 call',
                zones=zones, infantry_array=[], unit_array=['FV1', 'FV2', 'FV3', 'FV4'], team_array=[],
                frame=41, seed=31, type_speed=signed(typ + 0x678), type_threat=signed(typ + 0x670),
                range=signed(weapon + 0xB4), victim_xyz=list(struct.unpack('<3i', u.mem_read(victim + 0x9C, 12))),
                attacker_xyz=list(struct.unpack('<3i', u.mem_read(attacker + 0x9C, 12))),
                supplied_computer_owner=True, stack_backing_byte='A5', bridge_cell_flags=0,
                candidates='cloned supplied victim payload plus original fresh Drive4AF540'),
                cooldown=list(struct.unpack('<3i', u.mem_read(attacker + 0x650, 12))),
                assigned=signed(local + 0x24), budget=signed(local + 0x18), events=events, writes=writes,
                rng_before=rng_before, rng_after=bytes(u.mem_read(scenario + 0x218, 0x3F4)).hex(),
                final={who(pointer): dict(queue=signed(pointer + 0xB4), target=who(m.read32(pointer + 0x2B4)),
                archive=who(m.read32(pointer + 0x218))) for pointer in candidates},
                required_addresses=[f'0x{pc:08X}' for pc in required], original_code_and_vtables_unchanged=True)


def generate_base_response():
    native = BaseResponseNative()
    native.constructor['speed_getters'] = [native.speed_getter(name) for name in ('E1', 'MTNK', 'HTNK')]
    physical = native.physical_layers()
    physical['speed_getters'] = [native.speed_getter(name) for name in ('E1', 'MTNK', 'HTNK')]
    type_readers = []
    for label, sections in (
            ('type_negative_threat', {'MTNK': {'ThreatPosed': '-1'}}),
            ('type_missing_threat', {'MTNK': {'Other': '1'}}),
            ('type_malformed_threat', {'MTNK': {'ThreatPosed': 'junk'}}),
            ('type_wrapping_threat', {'MTNK': {'ThreatPosed': '2147483648'}}),
            ('type_wrong_case', {'MTNK': {'threatposed': '19', 'speed': '10'}}),
            ('bunker_wrong_case', {'NATBNK': {'bunker': 'no'}}),
            ('bunker_authored_no', {'NATBNK': {'Bunker': 'no'}}),
            ('bunker_missing_retains', {'NATBNK': {'Other': '1'}})):
        type_readers.append(dict(layer=label, input=sections, output=native.read_layer(sections)))
    readers = []
    for label, sections in (
            ('conflicting_sections', {'General': {'ComputerBaseDefenseResponse': '17'}, 'AI': {'ComputerBaseDefenseResponse': '5'}}),
            ('general_decoy_retains_ai', {'General': {'ComputerBaseDefenseResponse': '-9'}}),
            ('map_ai_signed', {'AI': {'ComputerBaseDefenseResponse': '-4'}}),
            ('wrong_case_retains', {'AI': {'computerbasedefenseresponse': '8'}, 'ai': {'ComputerBaseDefenseResponse': '8'}}),
            ('map_general_timers', {'General': {'BaseDefenseDelay': '.001', 'SuspendPriority': '-7', 'SuspendDelay': '.125', 'ThreatPerOccupant': '-2'}})):
        readers.append(dict(layer=label, input=sections, output=native.read_layer(sections)))
    speed_rows = []
    for raw in ('7', '-1', '4294967295', '$FFFFFFFF', 'junk', None, '7', '',
                '100', '101', '99', '2147483647', '2147483648', '4294967296',
                '-2', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10'):
        sections = {'MTNK': {'ThreatPosed': '15'}}
        if raw is not None:
            sections['MTNK']['Speed'] = raw
        speed_rows.append(dict(raw=raw, output=native.read_layer(sections)))
    speed_rows.append(dict(raw=None, output=native.read_layer({'MTNK': {'Other': '1'}})))
    # Restore physical source fields after the deliberately synthetic layers.
    native.physical_layers(record_files=False)
    type_fields = native.type_state()
    threat_rows = [native.threat_posed(row) for row in (
        dict(name='constructor_building_default', type='NATBNK', own_threat=0),
        dict(name='own_building_threat', type='NATBNK', own_threat=31),
        dict(name='garrison_has_priority', type='NATBNK', own_threat=31, garrison_count=3,
             linked_threat=7, threat_per_occupant=10),
        dict(name='zero_garrison_bunker_occupant', type='NATBNK', own_threat=31, linked_threat=7),
        dict(name='negative_garrison_bunker_occupant', type='NATBNK', own_threat=31,
             garrison_count=-2, linked_threat=7),
        dict(name='zero_bunker_occupant_threat', type='NATBNK', own_threat=31, linked_threat=0),
        dict(name='garrison_signed_overflow', type='NATBNK', own_threat=31,
             garrison_count=2147483647, threat_per_occupant=2),
        dict(name='two_garrison_signed_overflow', type='NATBNK', own_threat=31,
             garrison_count=2, threat_per_occupant=2147483647),
        dict(name='garrison_negative_multiplier', type='NATBNK', own_threat=31,
             garrison_count=3, threat_per_occupant=-2),
        dict(name='unit_link_does_not_redirect', type='MTNK', own_threat=15, linked_threat=7),
        dict(name='missing_own_type', type='NATBNK', own_threat=31, null_type=True,
             garrison_count=3, linked_threat=7))]
    scorers = []
    for name in ('E1', 'MTNK', 'HTNK'):
        scorers.append(native.score(dict(type=name, threat=type_fields[name]['threat'], speed=type_fields[name]['speed'],
                                         current=[1280, 1280, 416], attacker=[3280, 1280, 0],
                                         range=native.signed(native.weapons[name] + 0xB4))))
    for updates in (
            dict(name='inside_range', attacker=[1408, 1280, 0]), dict(name='null_attacker', null_attacker=True),
            dict(name='already_attacking', target='attacker'), dict(name='harvest', harvest=True),
            dict(name='non_defense_team', team_base_defense=False), dict(name='defense_team', team_base_defense=True),
            dict(name='zero_threat', threat=0), dict(name='negative_threat_far', threat=-2),
            dict(name='negative_threat_near', threat=-2, attacker=[1408, 1280, 0]),
            dict(name='shift_wrap_near', threat=2097152, attacker=[1408, 1280, 0]),
            dict(name='shift_wrap_far', threat=2097152), dict(name='wrapping_range', range=-2147483648),
            dict(name='already_attacking_negate_wrap', threat=-2147483648, target='attacker'),
            dict(name='shift_maximum_near', threat=2147483647, attacker=[1408, 1280, 0]),
            dict(name='range_equal_distance', current=[0, 0, 0], attacker=[1280, 0, 0]),
            dict(name='range_one_under', current=[0, 0, 0], attacker=[1280, 0, 0], range=1279),
            dict(name='range_one_over', current=[0, 0, 0], attacker=[1280, 0, 0], range=1281),
            dict(name='speed_zero', speed=0), dict(name='speed_one', speed=1), dict(name='speed_negative_supplied', speed=-7)):
        row = dict(type='MTNK', threat=15, speed=17, current=[1280, 1280, 416],
                   attacker=[3280, 1280, 0], range=1280)
        row.update(updates)
        scorers.append(native.score(row))
    selection_rows = [native.selection(entries, budget, anchor) for entries, budget, anchor in (
        ([(i, score, 'infantry') for i, score in enumerate((1, 2, 3, 1, 5, 6, 99, 100), 1)], 1000, False),
        ([(i, score, 'unit') for i, score in enumerate((1, 2, 3, 1, 5, 6, 99, 100), 1)], 1000, False),
        ([(1, -4, 'infantry'), (2, -4, 'unit'), (3, 5, 'infantry'), (4, 5, 'unit')], 1000, True),
        ([(1, -2147483648, 'infantry'), (2, 2147483647, 'unit')], 9, True),
        ([(1, 1, 'unit'), (2, -17, 'unit')], 10, False),
        ([(i, 1, 'unit') for i in range(1, 8)], 1000, False),
        ([(i, -2147483648, 'infantry') for i in range(1, 7)] + [(7, 2, 'infantry')], 1000, True),
        ([(1, 5, 'infantry'), (2, 5, 'unit')], 1000, True))]
    sorts = [native.sort(scores) for scores in ([], [1], [1, 1, 2], [4, 9, 4, 7],
                                               [2, 1, 1], [1, 2, 1, 2], [4, 4, 4, 4], [3, -1, -2, 3])]
    dispatch_rows = []
    base = dict(objects=[['A', 'MTNK', 15], ['B', 'MTNK', 15], ['C', 'HTNK', 40]],
                selected=['A', 'B', 'C'], scores=[1, 1, 2], seed=31)
    for budget in (40, 55, 70):
        dispatch_rows.append(native.dispatch(dict(base, name=f'budget_{budget}', budget=budget)))
    for updates in (
            dict(name='duplicates', selected=['A', 'B', 'B'], scores=[1, 2, 2], budget=100),
            dict(name='base_defense_team', base_defense_team=['B'], budget=55),
            dict(name='attacker_without_foot_flag', attacker_flags=1, budget=40),
            dict(name='timer_float_reader_bits', delay_bits='3f50624de0000000', budget=40),
            dict(name='assigned_wrap', initial_assigned=2147483640, budget=40),
            dict(name='infantry_idle_setter', objects=[['E', 'E1', 10]], selected=['E'], scores=[10], budget=1),
            dict(name='mission_draw65', seed=3, selected=['A'], scores=[1], budget=1),
            dict(name='mission_draw66', seed=15, selected=['A'], scores=[1], budget=1),
            dict(name='base_defense_draw65', seed=3, selected=['A'], scores=[1], budget=1, base_defense_team=['A']),
            dict(name='empty_selected', selected=[], scores=[], budget=0)):
        dispatch_rows.append(native.dispatch(dict(base, **updates)))
    retail_scores = [native.score(dict(name=label, type=name, threat=threat, speed=speed,
                     current=[1280, 1280, 416], attacker=[1408, 1280, 0], range=range_))
                     for label, name, threat, speed, range_ in
                     (('A', 'MTNK', 15, 17, 1280), ('B', 'MTNK', 15, 17, 1280), ('C', 'HTNK', 40, 15, 1472))]
    for budget in (40, 55, 70):
        dispatch_rows.append(native.dispatch(dict(base, name=f'retail_scored_budget_{budget}', budget=budget,
                             scores=[row['output'] for row in retail_scores], original_score_rows=retail_scores)))
    whole = whole_base_response()
    return dict(schema_version=1, physical_files=native.files,
                constructor=native.constructor, section_pointers={f'0x{p:08X}': dict(pointer=f'0x{native.m.read32(p):08X}',
                section=native.m.string(native.m.read32(p))) for p in (0x7F0C9C, 0x7F0CD4)},
                physical=physical, reader_controls=readers, type_reader_controls=type_readers,
                speed_history=speed_rows, threat_posed=threat_rows,
                scoring=scorers, selection=selection_rows, sort=sorts, dispatch=dispatch_rows,
                whole_response=[whole], infantry_assignment=BaseResponseNative().infantry_assignment(),
                original_spans={name: dict(address=f'0x{address:08X}', bytes=size,
                sha256=hashlib.sha256(native.originals[name]).hexdigest()) for name, (address, size) in BASE_RESPONSE_SPANS.items()})


def base_response_metadata():
    return provenance(scope='Original base-response readers, ThreatPosed, nonbuilding scorer, six-slot selection, exchange sort and dispatch tail; one whole708080 supplied empty-FV transaction; explicitly supplied lifecycle/registry/map state.',
        assumptions=[
            'Physical RULESMD, optional LANGRULE, MPBattleMD and Hills.map exact-case lexical strings feed the existing signed-CRC native INI caches; ARTMD is fixed. Original constructors and selected retained scalar/Image readers execute. Full file/MIX loading, full Rules Process/type discovery and other reader phases are excluded.',
            'Original Rules665650 supplies defaults. ReadAI673E41 takes AI from7F0CD4; ReadGeneral blocks take General from7F0C9C. Conflicting section, case, signed and sequential controls execute the original readers, not a Rust projection.',
            'Weapon Primary binding is supplied from physical names; actual Weapon771C70 constructor and ReadRange474620 independently establish range inputs. Original GetWeapon/Range getters execute in4D97A0. OpenTopped/cargo and foundation targets are excluded from the nonbuilding admitted response route.',
            'Native type scalars are loaded from physical readers. Sequential TechnoType71464A Speed controls record parsed i32, signed clamps0..100 before conversion, and minus-one retained layers. Explicit negative resident speed/Threat, shift/range/assigned overflow controls supply synthetic fields and do not claim stock reader reachability. Object payloads, missions, Team/BaseDefense links and XYZ are supplied; full object construction/logic lifecycle is excluded.',
            'ThreatPosed708B40 executes original concrete Building/Unit type accessors and Building4581F0 garrison count. Building+2E4/linked Unit+2E4 are supplied reciprocal TankBunker occupancy, whose active type gate44B780 and writer/release body spans are pinned; original bunker installation/release is not executed. Positive garrison has priority and queries count twice; zero/negative count falls through to bunker occupant, then own type. Signed multiplication and null-own-type controls execute.',
            'Selection rows begin after supplied scoring, at original708351 or7085AD. They execute original class/anchor multipliers, negative debits, growth and all-minimum replacement. Sort rows execute708647..7086AF. They are distinct from the full original4D97A0 scoring rows.',
            'Dispatch begins at708647 with explicit selected arrays/score/budget/stack state. Actual Scenario65C6D0 seed and65C7E0 draws, Mission5B35E0 deferred queue, Unit6FCDB0/Infantry51B1F0 setters and708B40 run. Type E1 records come from original523D00 and fixed physical GISequence. Linked temporal/animation/spawn/radio state is absent; no broader detach claim.',
            'Appended infantry_assignment uses a separate instance of the same BaseResponseNative owner so prior receipts remain identical. Full51B1F0 and any51D6F0 then6FCDB0 execute with actual E1 Image/GISequence records. Current Doing, health, prone/falling, pointer links and timer backing are supplied. All executable PE sections and action table bytes7EAF7C are checked unchanged after each row. Initial Doing30 has no physical GISequence frames and is a supplied synthetic state; death Doing11 with positive health is also a branch control, not a stock lifecycle claim.',
            'Infantry Type+6AC is independently established DeployFire: original5236A0 overA5 backing writes false at711144; original7147E8..714802 ReadBool5295F0 uses literal843AA0 and current-byte default, final store7147FC. Physical RULESMD[E1]yes and mode/map absence execute in order, followed by explicit retained/case/malformed/map overrides. Constructor-owned other type fields, absent carry/nav/air/water state and full world producers remain bounded.',
            'The full infantry setter rows observe firing68D before action, Doing/frame/timer writes, DWORD5E0, passive50C/base target, BYTE68E and reciprocal2A8 cleanup order. Timer+104 is recorded as copied reused stack data without a gameplay interpretation. Supplied reciprocal2A8 identity is intentionally unnamed; no parasite/temporal production or larger linked-effects parity is asserted. Native Scenario bytes remain unchanged in these requested0/2/28, unforced, randomStart0 rows.',
            'Tail stack+4C is an explicit sentinel and copies to cooldown+654. The separate whole708080 row reuses existing bridge_target_composed.setup: inherited selected physical FV readers, supplied computer House and four cloned lifecycle payloads, original fresh Drive constructors, empty Team/Infantry arrays and supplied simple zone state. Full original admission, zone/query/scorer/selection/dispatch run from entry to RET_MAGIC. The original prologue leaves A5 stack+4C intact; Unit victim Foot4DBDF0 later writes navigation Y5248 there, then cooldown copies5248. No zero auxiliary initialization is supplied or asserted.',
            'The whole708080 FV fixture retains native constructor Speed0 because its inherited preparation does not execute that key reader; it is explicitly a supplied getter state, not stock physical FV Speed parity. Its cells have no bridge flags; bridge-specific retained-head/layer admission remains covered by separate --bridge-layers consumer seams. The direct call bypasses the damage/ToProtect trigger. Populated TeamSuspend/remove, global construction/lifecycle, saves, loaders and bridge producers are excluded.',
            'Native instructions and concrete object vtables are checked unchanged after every row. Required original addresses are recorded. Existing default44 and bridge-layer payloads are preserved.'
        ], substitutions=['Existing BulletReader/Reader supplies bounded operator_new7C8E17 storage, operator_delete7C8B3D and CRT TLS7D140B. Physical archive IO boundary is inherited but no selected scalar/score/sort/dispatch callback return is substituted.'],
        entry_points={'rules_constructor': 0x665650, 'type_threat_reader': 0x7149C7,
                      'type_speed_reader': 0x71464A, 'threat_posed': 0x708B40,
                      'score': 0x4D97A0, 'selection_infantry': 0x708351, 'selection_unit': 0x7085AD,
                      'sort': 0x708647, 'dispatch_rng': 0x65C7E0, 'mission_queue': 0x5B35E0,
                      'unit_assign': 0x6FCDB0, 'infantry_assign': 0x51B1F0, 'infantry_action': 0x51D6F0,
                      'whole_response': 0x708080, 'team_suspend_empty_array': 0x6EC250})


if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--bridge-layers', action='store_true')
    parser.add_argument('--base-response', action='store_true')
    mode, arguments = parser.parse_known_args()
    if mode.base_response:
        assert not mode.bridge_layers, 'Choose one separate corpus mode'
        finish_vectors(generate_base_response, Path(__file__).with_name('base_defense_response.json'),
                       provenance=base_response_metadata, argv=arguments, source_paths={
                           'foot_navigation_coordinate.py': Path(__file__),
                           'native_oracle.py': Path(__file__).parents[1] / 'native_oracle.py',
                           'bridge_render_inputs.py': Path(__file__).parents[1] / 'projectile_oracle' / 'bridge_render_inputs.py',
                           'bridge_anim_inputs.py': Path(__file__).parents[1] / 'rules_oracle' / 'bridge_anim_inputs.py',
                           'bridge_anim_lists.py': Path(__file__).parents[1] / 'rules_oracle' / 'bridge_anim_lists.py',
                           'building_body_rules.py': Path(__file__).with_name('building_body_rules.py'),
                           'bridge_target_composed.py': Path(__file__).with_name('bridge_target_composed.py'),
                           'ifv_fire_coord.py': Path(__file__).parents[1] / 'projectile_oracle' / 'ifv_fire_coord.py',
                           'guided_step.py': Path(__file__).parents[1] / 'projectile_oracle' / 'guided_step.py',
                           'flat_art.py': Path(__file__).parents[1] / 'projectile_oracle' / 'flat_art.py',
                           'locomotor_at_coord.py': Path(__file__).with_name('locomotor_at_coord.py'),
                           'map_queries.py': Path(__file__).with_name('map_queries.py'),
                       })
    elif mode.bridge_layers:
        finish_vectors(generate_bridge_layers, Path(__file__).with_name('foot_bridge_layer.json'),
                       provenance=bridge_metadata, argv=arguments, source_paths={
                           'foot_navigation_coordinate.py': Path(__file__),
                           'locomotor_at_coord.py': Path(__file__).with_name('locomotor_at_coord.py'),
                           'map_queries.py': Path(__file__).with_name('map_queries.py'),
                           'native_oracle.py': Path(__file__).parents[1] / 'native_oracle.py',
                       })
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Foot4DBDF0 navigation coordinate wrapper, including Tube override and original active Walk/Drive/Ship/Hover ILocomotion+18 head queries. Supplied retained state only; no producers/FindPath/target callbacks.',
        entry_points={'foot_coordinate': 0x4DBDF0, 'tube_override': 0x4DBE01,
                      **{family + '_head': state['head'] for family, state in FAMILIES.items()}},
        assumptions=['Original concrete Infantry table for Walk and Unit table for the other three active families. Original ILocomotion tables and supplied link pointer; no callable replacement.',
                     'Native null-coordinate globals are supplied zero. Retained XYZ and physical XYZ are independent and observed after each call. Track selectors and cursors cannot gate HeadTo dispatch.',
                     'Foot+684=-1 without a Tube; explicit Tube rows supply index0 and a one-entry Tube table with signed exit CellStruct. Wrapper must bypass the locomotor query and return signed exit center with Z0.',
                     'Inherited OriginalQuery maps verified unmodified retail bytes and observes existing helper addresses; its separate IsAtCoord and track transform methods are not invoked.'],
        substitutions=[]), argv=arguments)
