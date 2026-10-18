"""Original sidebar comparator, admitted insertion and retained removal order.

The comparator and insertion execute whole, with original RTTI, cost virtuals
and wcscmp. AddCameo stops after insertion or at its rejection epilogue; its
presentation tail is outside this fixture. Retained histories stop Recalculate
after filtering; separate scroll controls execute it through its return.
See cameo_order.md for boundaries.
No Python implementation supplies an expected comparison or ordered list.
"""

from itertools import product
from pathlib import Path
import copy
import hashlib
import os
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI,
    UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
    UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    RET_MAGIC, STACK_BASE, STACK_SIZE, NativeCallTrace, finish_vectors, provenance, run_checked,
)
from tools.sidebar_oracle.geometry import machine, put32, read
from tools.spatial_oracle.cost_of import VTABLE, f32
from tools.spatial_oracle.building_body_rules import (
    Fixture as IniFixture, INI, SP as INI_SP, TYPE as INI_TYPE, dwords,
)

COMPARE = 0x6A8420
INSERT = 0x6A8710
ADD = 0x6A6300
ADD_ACCEPTED, ADD_REJECTED = 0x6A6423, 0x6A65FF
RECALCULATE, FILTER_DONE = 0x6AA600, 0x6AAAB3
SIDEBAR = 0x87F7E8
HEAP, HEAP_SIZE = 0x21000000, 0x00400000
STACK = STACK_BASE + STACK_SIZE - 0x1000
STRIPS = [SIDEBAR + 0x1544 + index * 0xF94 for index in range(4)]
ARRAYS = {40: 0xA83CE4, 3: 0xA8B21C, 7: 0xA83C6C,
          16: 0xA8E34C, 31: 0xA8E334}
TYPE_NAMES = {40: 'unit', 3: 'aircraft', 7: 'building', 16: 'infantry'}
SUPER_KINDS = (31, 32, 57)


def item(name, kind=40, *, side=0, tech_level=1, cost=500, ui_name='Same',
         considered_aircraft=False, naval=False, build_cat=0,
         recharge_frames=4500, free_unit=None):
    return dict(id=name, kind=kind, side=side, tech_level=tech_level, cost=cost,
                ui_name=ui_name, considered_aircraft=considered_aircraft,
                naval=naval, build_cat=build_cat, recharge_frames=recharge_frames,
                free_unit=free_unit)


class Sidebar:
    """Supplied initialized type/House objects; original ordering instructions."""

    def __init__(self, catalog, *, side=0, plant=None, country=None):
        self.u = machine()
        self.u.mem_map(HEAP, HEAP_SIZE)
        self.cursor = HEAP
        self.catalog = {entry['id']: dict(entry) for entry in catalog}
        self.pointers = {}
        self.lookup = {}
        self.events = []
        self.scroll_observation = None
        self.available = {}
        self.can_build = {}
        self.house = self.alloc(0x6000)
        self.country = self.alloc(0x400)
        self.rules = self.alloc(0x2000)
        self.pad_aircraft = self.alloc(0x400)
        self.pad_vector = self.alloc(8)
        self.pad_dock = self.alloc(4)
        self.scenario = self.alloc(0x4000)
        self.factory = self.alloc(0x300)
        self.super_instances = self.alloc(512)
        self.visible_copy = self.alloc(0x1000)
        self.arrays = {kind: self.alloc(512) for kind in ARRAYS}
        u = self.u
        u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        put32(u, 0xA83D4C, self.house)
        put32(u, self.house + 0x34, self.country)
        put32(u, self.house + 0x258, self.super_instances)
        put32(u, self.factory + 0x21C, self.house)
        put32(u, self.country + 0xBC, side)
        put32(u, 0x8871E0, self.rules)
        put32(u, self.rules + 0xB5C, self.pad_vector)
        put32(u, self.pad_vector, self.pad_aircraft)
        put32(u, self.pad_aircraft + 0x3EC, self.pad_dock)
        put32(u, 0xA8B230, self.scenario)
        # The cost owner reads SeparateAircraft; no bundled pad fixture here.
        u.mem_write(self.rules + 0x17E8, b'\x01')
        self.set_factors(plant=plant, country=country)
        # Three visible rows in Recalculate's original geometry computation.
        put32(u, 0x886F9C, 341)
        put32(u, 0x886F94, 0)
        put32(u, 0xB0B4F8, 158)
        put32(u, 0xA8ED84, 123)
        u.mem_write(0xA8ED6B, b'\0')
        put32(u, 0xA8E7AC, 0)
        for kind, address in ARRAYS.items():
            put32(u, address, self.arrays[kind])
        counts = {kind: 0 for kind in ARRAYS}
        for entry in self.catalog.values():
            kind = 31 if entry['kind'] in SUPER_KINDS else entry['kind']
            index = counts[kind]
            counts[kind] += 1
            entry['type_index'] = index
            ptr = self.alloc(0x1800)
            self.pointers[entry['id']] = ptr
            self.lookup[(entry['kind'], index)] = entry['id']
            put32(u, self.arrays[kind] + index * 4, ptr)
            name = (entry['ui_name'] + '\0').encode('utf-16-le')
            text = self.alloc(len(name))
            u.mem_write(text, name)
            put32(u, ptr + 0x60, text)
            if kind == 31:
                put32(u, ptr + 0xB0, entry['recharge_frames'])
                super_ptr = self.alloc(0x80)
                put32(u, self.super_instances + index * 4, super_ptr)
                u.mem_write(super_ptr + 0x6D, b'\x01')
            else:
                put32(u, ptr, VTABLE[TYPE_NAMES[kind]])
                put32(u, ptr + 0x610, entry['cost'])
                put32(u, ptr + 0x634, entry['tech_level'])
                put32(u, ptr + 0x6D0, entry['side'])
                put32(u, ptr + 0xE08, entry['build_cat'])
                u.mem_write(ptr + 0xCCE, bytes([entry['naval']]))
                u.mem_write(ptr + 0xD96, bytes([entry['considered_aircraft']]))
                self.available[ptr] = True
                self.can_build[ptr] = 1
        put32(u, self.house + 0x264, counts[31])
        for entry in self.catalog.values():
            if entry['free_unit']:
                put32(u, self.pointers[entry['id']] + 0xEA0,
                      self.pointers[entry['free_unit']])
        self.factory_queries = {
            read(u, table + 0x94)[0] for table in VTABLE.values()
        }
        u.hook_add(UC_HOOK_CODE, self.hook)
        for tab, strip in enumerate(STRIPS):
            self.invoke(0x6A80A0, strip, (0,), context={'case': 'strip-constructor', 'tab': tab})
            put32(u, strip + 0x38, tab)
        self.events = []

    def alloc(self, size):
        ptr = self.cursor
        self.cursor += (size + 15) & ~15
        if self.cursor > HEAP + HEAP_SIZE:
            raise RuntimeError('sidebar oracle heap exhausted')
        return ptr

    def set_factors(self, *, plant=None, country=None):
        self.plant = list(plant or [1.0] * 5)
        self.country_factors = list(country or [1.0] * 5)
        self.u.mem_write(self.house + 0x5390, dwords(*(f32(x) for x in self.plant)))
        self.u.mem_write(self.country + 0x114, dwords(*(f32(x) for x in self.country_factors)))

    def native_return(self, value, pop):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        target = struct.unpack('<I', u.mem_read(sp, 4))[0]
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_ESP, sp + 4 + pop)
        u.reg_write(UC_X86_REG_EIP, target)

    def hook(self, u, address, _size, _data):
        if address == 0x752700:
            self.events.append('EVA_NewConstructionOptions')
            self.native_return(0, 4)
        elif address == 0x7C8E17:
            self.events.append('visible-copy-allocation')
            sp = u.reg_read(UC_X86_REG_ESP)
            size = struct.unpack('<I', u.mem_read(sp + 4, 4))[0]
            if size > 0x1000:
                raise RuntimeError(f'unexpected visible-copy allocation {size}')
            u.mem_write(self.visible_copy, bytes(size))
            self.native_return(self.visible_copy, 0)
        elif address == 0x6A6610:
            self.events.append('UpdateScrollButtons')
            self.native_return(0, 0)
        elif address == 0x7C8B3D:
            sp = u.reg_read(UC_X86_REG_ESP)
            pointer = struct.unpack('<I', u.mem_read(sp + 4, 4))[0]
            if pointer != self.visible_copy:
                raise RuntimeError(f'unexpected Recalculate delete {pointer:#x}')
            self.events.append('visible-copy-delete')
            self.native_return(0, 0)
        elif address in self.factory_queries:
            ptr = u.reg_read(UC_X86_REG_ECX)
            self.events.append('supplied-FindFactory')
            self.native_return(self.factory if self.available[ptr] else 0, 16)
        elif address == 0x4F7870:
            sp = u.reg_read(UC_X86_REG_ESP)
            ptr = struct.unpack('<I', u.mem_read(sp + 4, 4))[0]
            self.events.append('supplied-CanBuild')
            self.native_return(self.can_build[ptr], 12)
        elif address in (0x711F00, 0x45EDD0):
            self.events.append('Cost_Of')
        elif address == 0x7CA5D3:
            self.events.append('wcscmp')
        elif address == COMPARE:
            self.events.append('CompareItems')
        elif self.scroll_observation is not None and address in (0x6AA711, FILTER_DONE):
            slots = read(u, self.visible_copy)[0]
            visible = []
            for index in range(slots):
                type_index, kind = read(u, self.visible_copy + 4 + index * 0x34, 2)
                visible.append(None if (kind, type_index) == (0, 0)
                               else self.lookup[(kind, type_index)])
            self.scroll_observation['visible_slots'] = slots
            key = 'snapshot_before' if address == 0x6AA711 else 'snapshot_after'
            self.scroll_observation[key] = visible

    def invoke(self, address, obj=0, args=(), *, ends=RET_MAGIC, context=None):
        u = self.u
        u.mem_write(STACK, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, STACK)
        u.reg_write(UC_X86_REG_ECX, obj)
        return run_checked(u, address, ends, count=200_000,
                           required_addresses=(address,), context=context)

    def compare(self, left, right):
        lhs, rhs = self.catalog[left], self.catalog[right]
        self.events = []
        self.invoke(COMPARE, args=(lhs['kind'], lhs['type_index'],
                                  rhs['kind'], rhs['type_index']),
                    context={'case': 'compare', 'left': left, 'right': right})
        return dict(left=left, right=right, before_or_equal=bool(self.u.reg_read(UC_X86_REG_EAX) & 255),
                    cost_calls=self.events.count('Cost_Of'), name_compared='wcscmp' in self.events)

    def cost(self, name):
        ptr = self.pointers[name]
        table = read(self.u, ptr)[0]
        target = read(self.u, table + 0x84)[0]
        self.invoke(target, ptr, (self.house,), context={'case': 'cost-input', 'id': name})
        return struct.unpack('<i', dwords(self.u.reg_read(UC_X86_REG_EAX)))[0]

    def order(self):
        result = []
        for strip in STRIPS:
            entries = []
            for index in range(read(self.u, strip + 0x54)[0]):
                type_index, kind = read(self.u, strip + 0x58 + index * 0x34, 2)
                entries.append(self.lookup[(kind, type_index)])
            result.append(entries)
        return result

    def add(self, name):
        entry = self.catalog[name]
        self.events = []
        reached = self.invoke(ADD, SIDEBAR, (entry['kind'], entry['type_index']),
                              ends=(ADD_ACCEPTED, ADD_REJECTED),
                              context={'case': 'add', 'id': name})
        return dict(action='add', id=name, accepted=reached == ADD_ACCEPTED,
                    order=self.order(), eva_calls=self.events.count('EVA_NewConstructionOptions'))

    def recalculate(self, tab, *, complete=False):
        self.events = []
        self.scroll_observation = {} if complete else None
        self.invoke(RECALCULATE, STRIPS[tab],
                    ends=RET_MAGIC if complete else (FILTER_DONE, RET_MAGIC),
                    context={'case': 'recalculate-complete' if complete else 'recalculate-filter',
                             'tab': tab})
        if 'CompareItems' in self.events or 'Cost_Of' in self.events or 'wcscmp' in self.events:
            raise RuntimeError('unexpected re-sort during native retained-entry filter')
        if complete:
            result = dict(self.scroll_observation, order=self.order()[tab],
                          top_row_after=read(self.u, STRIPS[tab] + 0x44)[0],
                          removed=bool(self.u.reg_read(UC_X86_REG_EAX) & 255),
                          needs_full_redraw=bool(self.u.mem_read(SIDEBAR + 0x53A7, 1)[0]),
                          strip_needs_redraw=bool(self.u.mem_read(STRIPS[tab] + 0x3C, 1)[0]),
                          scroll_button_updates=self.events.count('UpdateScrollButtons'),
                          visible_copy_frees=self.events.count('visible-copy-delete'))
            self.scroll_observation = None
            return result
        return dict(action='recalculate', tab=tab, order=self.order())


def recalculate_scroll_controls():
    """Whole native filter/snapshot/row-adjustment with observed UI/delete sinks.

    All expected orders, snapshots, capacities and rows come from execution.
    Python only selects supplied layouts, starting rows and removal inputs.
    Each source strip is first populated through original AddCameo/InsertEntry.
    """
    catalog = [item(f'unit_{i:02}', cost=100 + i, ui_name=f'Unit {i:02}')
               for i in range(25)]
    m = Sidebar(catalog)
    for tab in range(4):
        m.invoke(0x69DCF0, 0xB07C48 + tab * 0x60,
                 context={'case': 'tab-gadget-constructor', 'tab': tab})
    # Original static initializer6A4CE0 uses these same four ctor calls. They
    # allow the all-entries-gone path to execute actual Gadget disable dispatch.
    layouts = [
        ('two_slots', 1, dict(body_height=241, body_y=0, sidebar_top=158, scenario_side=0)),
        ('four_slots', 2, dict(body_height=291, body_y=0, sidebar_top=158, scenario_side=0)),
        ('six_slots', 3, dict(body_height=341, body_y=0, sidebar_top=158, scenario_side=0)),
        ('ten_slots', 5, dict(body_height=441, body_y=0, sidebar_top=158, scenario_side=0)),
    ]
    specs = []
    six_slots = layouts[2][2]
    for label, removed in (
        ('none', []), ('remove0', [0]), ('remove1', [1]), ('remove2', [2]),
        ('remove3', [3]), ('remove6', [6]), ('remove7', [7]),
        ('remove8', [8]), ('remove19', [19]), ('remove0_1', [0, 1]),
        ('remove0_2', [0, 2]), ('remove2_3', [2, 3]),
        ('visible_gone', list(range(2, 8))), ('prefix_gone', list(range(8))),
        ('all_gone', list(range(20))),
    ):
        specs.append((f'twenty_six_slots_row1_{label}', 20, 1, six_slots, removed))
    for count, layout in product((1, 2, 5, 6, 7, 8, 12, 19, 20, 21, 24, 25), layouts):
        layout_name, rows, geometry = layout
        # Only input selection: the last ordinary scroll row is admitted while
        # (row + visible_rows) * 2 < count at6A8BFF..6A8C09, then row increments.
        last_row = max(0, (count + 1) // 2 - rows)
        for row in sorted({0, last_row}):
            for label, removed in (
                ('none', []), ('last', [count - 1]),
                ('first_visible', [row * 2]),
                ('visible_gone', list(range(row * 2, min(count, row * 2 + rows * 2)))),
                ('all_gone', list(range(count))),
            ):
                specs.append((f'count{count}_{layout_name}_row{row}_{label}',
                              count, row, geometry, removed))
    # Nonzero sidebar origin and either non-Allied scenario side exercise the
    # original eight-pixel footer difference without supplying its output.
    for side in (1, 2):
        geometry = dict(body_height=309, body_y=24, sidebar_top=158, scenario_side=side)
        specs.append((f'nonzero_origin_side{side}_remove19', 20, 1, geometry, [19]))
    # Exact division boundary and a zero-slot viewport use the original
    # capacity arithmetic; the latter must take the no-survivor branch.
    for height in (240, 340, 342):
        geometry = dict(body_height=height, body_y=0, sidebar_top=158, scenario_side=0)
        specs.append((f'capacity_boundary_height{height}', 20, 1, geometry, [19]))

    source_strips = {}
    cases = []
    for label, count, row, geometry, removed in specs:
        if count not in source_strips:
            m.invoke(0x6A81B0, STRIPS[3], context={'case': 'scroll-source-clear'})
            for name in list(m.catalog)[:count]:
                if not m.add(name)['accepted']:
                    raise RuntimeError(f'native scroll fixture failed to admit {name}')
            source_strips[count] = bytes(m.u.mem_read(STRIPS[3], 0xF94))
        # Restore a previously executed native source state for independent
        # cases; do not synthesize or sort a candidate result in Python.
        m.u.mem_write(STRIPS[3], source_strips[count])
        put32(m.u, STRIPS[3] + 0x44, row)
        put32(m.u, SIDEBAR + 0x539C, 3)
        m.u.mem_write(SIDEBAR + 0x53A7, b'\0')
        m.u.mem_write(STRIPS[3] + 0x3C, b'\0')
        for key, address in (('body_height', 0x886F9C), ('body_y', 0x886F94),
                             ('sidebar_top', 0xB0B4F8), ('scenario_side', m.scenario + 0x34B8)):
            put32(m.u, address, geometry[key])
        initial_order = m.order()[3]
        remove_ids = [initial_order[index] for index in removed]
        for name, pointer in m.pointers.items():
            m.available[pointer] = name not in remove_ids
        observed = m.recalculate(3, complete=True)
        cases.append(dict(id=label, initial_order=initial_order, remove_ids=remove_ids,
                          top_row_before=row, geometry=geometry, **observed))
    return dict(types=list(m.catalog.values()), cases=cases)


def comparison_catalog():
    return [
        item('ground'), item('air', considered_aircraft=True),
        item('naval', naval=True), item('air_naval', considered_aircraft=True, naval=True),
        item('aircraft_ground', 3), item('aircraft_air', 3, considered_aircraft=True),
        item('aircraft_naval', 3, naval=True),
        item('aircraft_both', 3, considered_aircraft=True, naval=True),
        item('infantry', 16), item('infantry_flags_ignored', 16, considered_aircraft=True, naval=True),
        item('building', 7), item('defense', 7, build_cat=5),
        item('other_side', side=1, tech_level=-1, cost=-10, ui_name='A'),
        item('third_side', side=2, tech_level=-1, cost=-10, ui_name='A'),
        item('negative_tech', tech_level=-1), item('high_tech', tech_level=2147483647),
        item('low_cost', cost=1), item('high_cost', cost=1001), item('negative_cost', cost=-10),
        item('empty_name', ui_name=''), item('upper_name', ui_name='Zulu'),
        item('lower_name', ui_name='alpha'), item('accented_name', ui_name='\u00c5ngstr\u00f6m'),
        item('bmp_name', ui_name='\ue000'), item('supplementary_name', ui_name='\U00010000'),
        item('same_tie'), item('super', 31), item('short_super', 31, recharge_frames=900),
        item('negative_super', 31, recharge_frames=-1), item('super_same_name', 31),
        item('super_name', 31, ui_name='Zulu'), item('super_kind32', 32), item('super_kind57', 57),
    ]


def pair_controls():
    catalog = comparison_catalog()
    m = Sidebar(catalog)
    names = list(m.catalog)
    rows = [m.compare(left, right) for left, right in product(names, repeat=2)]
    return dict(types=list(m.catalog.values()), current_side=0, country=[1.0] * 5,
                plant=[1.0] * 5, cases=rows)


def cost_contexts():
    catalog = [item('unit', cost=500), item('aircraft', 3, cost=400),
               item('infantry', 16, cost=300), item('plain_building', 7, cost=900),
               item('free_unit_building', 7, cost=1000, free_unit='unit'),
               item('defense', 7, cost=800, build_cat=5)]
    rows = []
    for label, country, plant in (
        ('unadjusted', [1.0] * 5, [1.0] * 5),
        ('unit_plant_discount', [1.0] * 5, [1.0, 0.75, 1.0, 1.0, 1.0]),
        ('different_country_slots', [1.5, 2.0, 0.5, 0.75, 0.25], [1.0] * 5),
        ('combined_factors', [1.5, 2.0, 0.5, 0.75, 0.25], [0.5, 0.75, 1.0, 0.5, 1.0]),
    ):
        m = Sidebar(catalog, country=country, plant=plant)
        rows.append(dict(id=label, types=list(m.catalog.values()), current_side=0,
                         country=country, plant=plant,
                         native_costs={name: m.cost(name) for name in m.catalog},
                         cases=[m.compare(a, b) for a, b in product(m.catalog, repeat=2)]))
    return rows


def histories():
    catalog = [
        item('power', 7, tech_level=1, cost=800, ui_name='Power'),
        item('refinery', 7, tech_level=1, cost=600, ui_name='Refinery'),
        item('defense', 7, build_cat=5, ui_name='Defense'),
        item('gi', 16, cost=200, ui_name='GI'), item('engineer', 16, cost=500, ui_name='Engineer'),
        item('tank', cost=1000, ui_name='Tank'), item('aircraft', 3, cost=800, ui_name='Aircraft'),
        item('new_vehicle', cost=1100, ui_name='New'), item('unit_tie_a', cost=1000, ui_name='Tank'),
        item('unit_tie_b', cost=1000, ui_name='Tank'),
        item('super_long', 31, recharge_frames=9000, ui_name='Long'),
        item('super_short', 31, recharge_frames=4500, ui_name='Short'),
    ]
    rows = []
    for label, sequence in (
        ('registration_order', [entry['id'] for entry in catalog]),
        ('reverse_registration', [entry['id'] for entry in reversed(catalog)]),
        ('mixed_unlock_batches', ['tank', 'gi', 'power', 'defense', 'super_long', 'aircraft',
                                 'engineer', 'unit_tie_a', 'refinery', 'super_short', 'unit_tie_b']),
    ):
        m = Sidebar(catalog)
        initial_costs = {name: m.cost(name) for name, entry in m.catalog.items()
                         if entry['kind'] not in SUPER_KINDS}
        steps = [m.add(name) for name in sequence]
        steps += [m.add(sequence[0]), m.add(sequence[-1])]
        # A late Industrial Plant changes the cross-class cost ordering but does
        # not trigger native comparisons for existing entries during Recalculate.
        m.set_factors(plant=[1.0, 0.75, 1.0, 1.0, 1.0])
        steps.append(dict(action='plant_factors', values=m.plant, order=m.order(),
                          native_costs={name: m.cost(name) for name, entry in m.catalog.items()
                                        if entry['kind'] not in SUPER_KINDS}))
        steps.extend(m.recalculate(tab) for tab in range(4))
        steps.append(m.add('new_vehicle'))
        m.available[m.pointers['tank']] = False
        steps.append(dict(action='factory_available', id='tank', value=False))
        m.can_build[m.pointers['unit_tie_a']] = 0
        steps.append(dict(action='can_build', id='unit_tie_a', value=0))
        steps.append(m.recalculate(3))
        m.available[m.pointers['tank']] = True
        m.can_build[m.pointers['unit_tie_a']] = 1
        steps.append(dict(action='factory_available', id='tank', value=True))
        steps.append(dict(action='can_build', id='unit_tie_a', value=1))
        steps.extend((m.add('tank'), m.add('unit_tie_a')))
        sw = m.catalog['super_short']['type_index']
        pointer = struct.unpack('<I', m.u.mem_read(m.super_instances + 4 * sw, 4))[0]
        m.u.mem_write(pointer + 0x6D, b'\0')
        steps.append(dict(action='super_granted', id='super_short', value=False))
        steps.append(m.recalculate(1))
        m.u.mem_write(pointer + 0x6D, b'\x01')
        steps.append(dict(action='super_granted', id='super_short', value=True))
        steps.append(m.add('super_short'))
        for strip in STRIPS:
            m.invoke(0x6A81B0, strip, context={'case': 'strip-init-clear'})
        steps.append(dict(action='init_clear', order=m.order()))
        steps.extend(m.add(name) for name in ('unit_tie_a', 'tank', 'unit_tie_b'))
        rows.append(dict(id=label, types=list(m.catalog.values()), current_side=0,
                         initial_native_costs=initial_costs, steps=steps))
    return rows


def capacity_history():
    # Original AddCameo admits count75, despite the original 75-record strip
    # allocation. These boundary controls deliberately observe that extra record
    # in mapped adjacent storage. The AddCameo/Remove UI tails do not execute and
    # no claim is made that the resulting complete native Sidebar is valid.
    catalog = [item(f'unit_{i:02}', cost=100 + i, ui_name=f'Unit {i:02}')
               for i in range(77)]
    m = Sidebar(catalog)
    initial = list(m.catalog)[:76]
    for name in initial:
        result = m.add(name)
        if not result['accepted']:
            raise RuntimeError(f'native capacity fixture failed to admit {name}')
    before = m.order()
    m.available[m.pointers['unit_00']] = False
    steps = [dict(action='factory_available', id='unit_00', value=False),
             m.add('unit_76'), m.recalculate(3), m.add('unit_76')]
    return dict(types=list(m.catalog.values()), current_side=0, initial_adds=initial,
                initial_order=before, steps=steps,
                limitation='The native gate allows76 records in75-record storage; this fixture '
                           'observes adjacent mapped bytes only and excludes the UI tail. '
                           'A safe Rust vector may reproduce admission without native memory corruption.')


def recharge_reader():
    """Original ctor stores and reader slice; supplied signed-CRC INI cache."""
    m = IniFixture()
    u = m.u
    u.mem_write(INI_SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, INI_SP)
    run_checked(u, 0x7C8F5E, RET_MAGIC)
    u.reg_write(UC_X86_REG_EBP, INI_TYPE)
    run_checked(u, 0x6CE5C0, 0x6CE681, required_addresses=(0x6CE5E9,))
    initial = read(u, INI_TYPE + 0xB0)[0]
    rows = []
    for raw in (None, '0', '5', '10', '3.5', '0.01', '-1', '50%', '0', None):
        before = read(u, INI_TYPE + 0xB0)[0]
        m.ini(0x842634, raw)
        u.reg_write(UC_X86_REG_ESP, INI_SP)
        u.reg_write(UC_X86_REG_EBP, INI_TYPE)
        u.reg_write(UC_X86_REG_EBX, INI)
        u.reg_write(UC_X86_REG_ESI, INI_TYPE + 0x1F8)
        run_checked(u, 0x6CED5C, 0x6CED95, required_addresses=(0x5283D0,),
                    context={'case': 'RechargeTime-reader', 'raw': raw, 'retained': before})
        rows.append(dict(raw=raw, before=before, recharge_frames=read(u, INI_TYPE + 0xB0)[0]))
    return dict(constructor_frames=initial, cases=rows)


def ready_building_inputs(root, *, layers=None):
    """Extend the existing physical joined reader; no placement result is supplied."""
    from tools.spatial_oracle import building_construction as bc
    from tools.spatial_oracle import building_death_anims as death
    from tools.rules_oracle.bridge_child_sound import sections
    m = death.joined_inputs(root, damage_fires=False, layers=layers)
    u, get = m.u, m.read32
    sound = sections((root / 'SOUNDMD.INI').read_bytes())
    wanted = ('MenuClick', 'PlaceBuilding')
    selected = {name: values for name, values in sound.items()
                if name in (*wanted, 'Defaults')}
    selected['SoundList'] = {key: value for key, value in sound['SoundList'].items()
                             if value in wanted}
    m.make_ini(selected)
    m.invoke(0x7510D0, INI)
    m.ready_sounds = [dict(name=name, native_index=m.invoke(0x7514D0, m.cstring(name)),
                          physical_section=sound[name]) for name in wanted]
    art = sections((root / 'ARTMD.INI').read_bytes())
    selected_art = {name: values for name, values in art.items()
                    if name in (*bc.JOINED_ART_NAMES, 'GAPILL')}
    m.make_ini(selected_art)
    art_cache = bytes(u.mem_read(INI, 0x40))
    pill = m.alloc(0x2000)
    m.invoke(0x45DD90, pill, (m.cstring('GAPILL'),))
    m.types['GAPILL'] = pill
    m.ready_layers = []
    for filename in m.input_rule_layers:
        path = root / filename
        row = dict(file=filename, absent=not path.exists())
        m.ready_layers.append(row)
        if not path.exists():
            continue
        raw = path.read_bytes()
        physical = sections(raw)
        cached = {name: values for name, values in physical.items()
                  if name in ('GAPILL', 'AudioVisual', 'General')}
        m.make_ini(cached)
        layer = m.alloc(0x40)
        u.mem_write(layer, bytes(u.mem_read(INI, 0x40)))
        u.mem_write(INI, art_cache)
        row.update(sha256=hashlib.sha256(raw).hexdigest(), physical_input_sections=cached,
                   gapill_reader_result=m.invoke(0x45FE50, pill, (layer,)),
                   sounds_before={'GUIBuildSound': get(m.rules + 0x18C),
                                  'BuildingSlam': get(m.rules + 0x6EC)})
        # Original AudioVisual ReadString528A10 -> FindIndex7514D0 retained
        # readers. Rules665650 owns both initial -1 values. The second slice
        # starts with the preceding sound's surviving EAX store; supply exactly
        # that already-read field, not a result for either selected key.
        for begin, end, previous in ((0x6693A6, 0x6693F1, None),
                                     (0x66AA7F, 0x66AACF, 0x6E8)):
            u.mem_write(INI_SP, bytes(0x300))
            u.reg_write(UC_X86_REG_ESP, INI_SP)
            u.reg_write(UC_X86_REG_ESI, m.rules)
            from unicorn.x86_const import UC_X86_REG_EDI
            u.reg_write(UC_X86_REG_EDI, layer)
            if previous is not None:
                u.reg_write(UC_X86_REG_EAX, get(m.rules + previous))
            run_checked(u, begin, end, count=2_000_000)
        row['sounds_after'] = {'GUIBuildSound': get(m.rules + 0x18C),
                               'BuildingSlam': get(m.rules + 0x6EC)}
    # The reused Engineer input intentionally narrows the registry to GAPOWR.
    # Reset to a declared empty service, then run the original constructor's
    # registration tail; ArrayIndex is never authored by this producer.
    u.mem_write(0xA83C68, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    for name in ('GACNST', 'GAPOWR', 'GAPILE', 'GAPILL'):
        u.reg_write(UC_X86_REG_ESP, INI_SP)
        u.reg_write(UC_X86_REG_ESI, m.types[name])
        u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(u, 0x45E2ED, 0x45E362)
    m.ready_art = selected_art
    return m


class ReadyBuildingFixture:
    """Actual paid factories and retained local Display/command-ring execution.

    Uses the existing joined VM/heap/physical readers/navigation/audio transport.
    Case restoration copies an executed ready baseline, including native memory,
    CPU context and transport state. It is not a scenario replacement producer.
    """

    def __init__(self, inputs, root, *, unit_startup=False,
                 producer_types=('GACNST', 'GAPILE')):
        from tools.spatial_oracle import building_construction as bc
        from tools.spatial_oracle import building_death_anims as death
        from tools.spatial_oracle import engineer_repair_admission as er
        from tools.spatial_oracle.refinery_dock import cell
        from unicorn.x86_const import UC_X86_REG_EDI
        from unicorn import UC_HOOK_MEM_WRITE
        self.bc, self.er, self.inputs = bc, er, inputs
        self.f = death.joined_fixture(inputs, 750, seed=2, building_ai=True,
                                      unit_startup=unit_startup)
        self.u, self.get = self.f.u, self.f.read32
        self.calls, self.writes, self.transport = [], [], []
        self.call_trace = NativeCallTrace(self.u, self.get, self.calls)
        self.identities = {0: None, er.HOUSE: 'local_house', self.f.source: 'selected_mtnk'}
        self.type_names = {pointer: name for name, pointer in self.f.types.items()}
        self.factories, self.products = {}, {}
        self.yard, self.bar = self.f.allocate(0x1000), self.f.allocate(0x1000)
        self.identities.update({self.yard: 'yard', self.bar: 'barracks'})
        self.watch = {
            0x4FA350: ('begin_production', 4, 16),
            0x4C9C70: ('start_production', 3, 12),
            0x4C9EA0: ('set_production_rate', 1, 4),
            0x5F7900: ('type_find_factory', 4, 16),
            0x5F5C20: ('object_find_factory', 2, 8),
            0x4F7870: ('can_build', 3, 12),
            0x6AAD00: ('select_action', 3, 12),
            0x4FB840: ('manual_place', 2, 8),
            0x4A91B0: ('preview_anchor', 2, 8),
            0x4A8BF0: ('active_foundation', 1, 4),
            0x4A8D50: ('saved_foundation', 1, 4),
            0x4AB9B0: ('tactical_left_up', 5, 20),
            0x4AAD30: ('tactical_right_up', 1, 4),
            0x4C6AE0: ('place_event_constructor', 6, 24),
            0x4FB0E0: ('house_place', 4, 16),
            0x4FAA10: ('house_abandon', 4, 16),
            0x4CA1A0: ('factory_completed', 0, 0),
            0x4C9FF0: ('factory_abandon', 0, 0),
            0x509140: ('house_update_factory_queue', 3, 12),
            0x4CA5A0: ('start_next_queued', 0, 0),
            0x4CA770: ('factory_scalar_delete', 1, 4),
            0x4FB9B0: ('house_pointer_expired', 2, 8),
            0x6A6140: ('sidebar_link_factory', 3, 12),
            0x6ABAD0: ('sidebar_abandon_factory', 3, 12),
            0x6ABB60: ('strip_abandon_factory', 1, 4),
            0x48DC90: ('unselect_all', 0, 0),
            0x5F44A0: ('deselect', 0, 0),
            0x5F4520: ('select', 0, 0),
            0x7258D0: ('dispatch_pointer_expired', 0, 0),
            0x6DA560: ('tactical_pointer_expired', 2, 8),
            0x4CA580: ('factory_pointer_expired', 2, 8),
            0x5F5280: ('object_detach', 1, 4),
            0x43BCF0: ('building_destructor', 0, 0),
            0x750920: ('play_gui_sound', 2, 8),
            0x752700: ('eva', 1, 4),
        }
        self.branches = {pc: name for pc, name in (
            (0x4FB1B4, 'cell_not_empty_sentinel'), (0x4FB1C4, 'ordinary_cell'),
            (0x4FB1F1, 'builder_hello'), (0x4FB234, 'unlimbo_result'),
            (0x4FB35F, 'failed_unlimbo'), (0x4FB395, 'restore_saved_client'),
            (0x4FB3DD, 'saved_type_guard'), (0x4FB489, 'clear_saved_retry'),
            (0x4FB32D, 'success_clear_saved'), (0x4FB520, 'sentinel_corridor'),
            (0x4FAB64, 'abandon_index_admitted'), (0x4FAB83, 'abandon_clear_current'),
            (0x6AB32A, 'no_builder_cameo'), (0x4FB852, 'manual_existing_type_guard'),
            (0x6AADC1, 'right_own_factory_guard'),
            (0x6AB0AC, 'right_without_own_factory'),
            (0x6AADDC, 'right_clear_current_building'),
            (0x509216, 'held_builder_query_result'),
            (0x50928C, 'queue_recheck_final_factory_state'),
            (0x5092A0, 'queue_recheck_delete_empty_factory'),
        )}
        self.observe_hook = self.u.hook_add(UC_HOOK_CODE, self.observe)
        self.write_hook = self.u.hook_add(UC_HOOK_MEM_WRITE, self.write)
        u, get = self.u, self.get
        self.f.phase = 'setup_clear_tile_physical_prior'
        self.invoke(0x4E69E0)
        tile, name = self.f.allocate(0x400), self.f.allocate(32)
        u.mem_write(name, b'Clear01\0')
        self.invoke(0x5447C0, tile, 0, 0xFFFFFFBF, 0, name, 0)
        raw = (root / 'CLEAR01.TEM').read_bytes()
        data = self.f.allocate(len(raw) + 16)
        cached = bytearray(raw)
        struct.pack_into('<I', cached, 16, data + struct.unpack_from('<I', raw, 16)[0])
        u.mem_write(data, bytes(cached))
        u.mem_write(tile + 0xA4, dwords(data))
        u.mem_write(tile + 0x2E4, dwords(1, 1))
        for y in range(1, 32):
            for x in range(1, 32):
                u.mem_write(cell(x, y) + 0x38, dwords(0))
                self.invoke(0x47D2B0, cell(x, y), 0xFFFFFFFF)
        for name, pointer, coord in ((producer_types[0], self.yard, (14, 14)),
                                      (producer_types[1], self.bar, (18, 14))):
            self.f.building(name, pointer, self.f.allocate(0x100), coord=coord)
            u.mem_write(pointer + 0x74, b'\0')
            self.invoke(0x43F180, pointer, 3)
            u.mem_write(pointer + 0xAC, dwords(5))
            u.mem_write(pointer + 0xB4, dwords(-1))
            u.mem_write(pointer + 0x534, dwords(1))
        self.f.house_prior(er.HOUSE, (self.yard, self.bar))
        u.reg_write(UC_X86_REG_EBP, er.HOUSE)
        run_checked(u, 0x4F5DCC, 0x4F5E02)
        # This original House constructor slice registers its Factory-expiry
        # listener. Without it, Factory scalar deletion removes the live
        # registry entry but leaves a supplied House slot pointing to released
        # arena bytes. Other House constructor/listener regions remain outside
        # the declared fixture bounds; no slot-clear is supplied here.
        self.house_factory_registration = dict(entry='0x004F5EB8', stop='0x004F5F0B',
            vector='0x00B0F640', before=bytes(u.mem_read(0xB0F640, 24)).hex(),
            members_before=[self.identity(get(get(0xB0F644) + i * 4))
                            for i in range(get(0xB0F650))])
        u.reg_write(UC_X86_REG_EBP, er.HOUSE)
        u.reg_write(UC_X86_REG_EBX, 0)
        u.reg_write(UC_X86_REG_ESP, bc.SP)
        run_checked(u, 0x4F5EB8, 0x4F5F0B)
        self.house_factory_registration['after'] = bytes(u.mem_read(0xB0F640, 24)).hex()
        self.house_factory_registration['members_after'] = [
            self.identity(get(get(0xB0F644) + i * 4)) for i in range(get(0xB0F650))]
        assert sum(get(get(0xB0F644) + i * 4) == er.HOUSE
                   for i in range(get(0xB0F650))) == 1
        self.invoke(0x49F9B0, er.HOUSE + 0x55A0)
        self.invoke(0x4F9950, er.HOUSE, 10000)
        u.mem_write(er.HOUSE + 0x1D4, dwords(10))  # declared local House technology prior
        self.invoke(0x4E6CE0)
        u.reg_write(UC_X86_REG_EBP, er.HOUSE)
        u.reg_write(UC_X86_REG_EBX, 0)
        u.reg_write(UC_X86_REG_ESP, bc.SP)
        run_checked(u, 0x4F619C, 0x4F61EF)
        u.reg_write(UC_X86_REG_ESP, bc.SP)
        run_checked(u, 0x52E10F, 0x52E119)
        for entry in (0x4E6E60, 0x4F50A0, 0x6A4BC0, 0x6A4DC0, 0x4E7D40):
            self.invoke(entry)
        selection = self.f.allocate(64 * 4)
        u.mem_write(0xA8ECBC, dwords(selection, 64, 1, 0, 10))
        for begin, end in ((0x4A8738, 0x4A8847), (0x5BDA48, 0x5BDA74)):
            u.reg_write(UC_X86_REG_ESI, SIDEBAR)
            u.reg_write(UC_X86_REG_ESP, bc.SP)
            run_checked(u, begin, end)
        self.mouse, self.key, self.cell_arg, self.out_cell = (
            self.f.allocate(0x100), self.f.allocate(4),
            self.f.allocate(8), self.f.allocate(8))
        u.mem_write(bc.SP, dwords(RET_MAGIC, 0, 0))
        u.reg_write(UC_X86_REG_ESP, bc.SP)
        u.reg_write(UC_X86_REG_ECX, self.mouse)
        run_checked(u, 0x7B8730, 0x7B8856)
        u.mem_write(0x887640, dwords(self.mouse))
        u.mem_write(self.mouse + 0x1C, dwords(320, 240))
        u.mem_write(self.key, dwords(0))
        u.mem_write(0x886F9C, dwords(341))
        u.mem_write(0x886F94, dwords(0))
        u.mem_write(0xB0B4F8, dwords(158))
        for tab, strip in enumerate(STRIPS):
            self.invoke(0x6A80A0, strip, 0)
            self.invoke(0x6A8220, strip, tab)
        self.watch[get(get(self.yard) + 0xD8)] = ('building_unlimbo', 2, 8)
        self.f.phase = 'ready_building_setup_complete'
        self.setup = self.snapshot()

    def invoke(self, pc, this=0, *args):
        result = self.bc.invoke(self.u, pc, this, *args)
        self.call_trace.returned(RET_MAGIC, self.u.reg_read(UC_X86_REG_ESP))
        return result

    def observe(self, u, pc, size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        self.call_trace.returned(pc, sp)
        if pc in self.watch:
            index = self.call_trace.entered(pc, sp, self.watch[pc])
            if pc in (0x6AAD00, 0x4FB840, 0x4FB0E0, 0x4FAA10, 0x7258D0):
                self.calls[index]['placement'] = self.placement()
        if pc in self.branches:
            self.calls.append(dict(name=self.branches[pc], pc=hex(pc),
                eax=u.reg_read(UC_X86_REG_EAX), placement=self.placement()))
        # Original native event/UI code calls the opaque WinMM import. The
        # existing VM owns its narrower646F20 arm; extend that SAME raw OS
        # transport convention to reached callsites, never a gameplay body.
        instruction = bytes(u.mem_read(pc, size))
        direct_clock = instruction == b'\xFF\x15\x30\x15\x7E\x00'
        indirect_clock = (pc == 0x6A8F18 and instruction == b'\xFF\xD3'
                          and u.reg_read(UC_X86_REG_EBX) == self.get(0x7E1530))
        # StripAI's automatic mobile PLACE issuer loads this same import into
        # EBX at6A8DC0 and CALLs it at6A8F18; WinMM takes no stack arguments.
        if u.reg_read(UC_X86_REG_EIP) == pc and (direct_clock or indirect_clock):
            self.transport.append(dict(kind='OS_timeGetTime', pc=hex(pc), wall_ms=0))
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_EIP, pc + size)

    def write(self, u, _access, address, size, value, _data):
        timer_ranges = [(self.yard + 0xC8, 12), (self.bar + 0xC8, 12),
                        (0xB0EA80, 12)]
        timer_ranges += [(p + 0x2C, 12) for p in self.factories.values()]
        if any(low <= address < low + length for low, length in timer_ranges):
            self.writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),
                address=hex(address), size=size, before=bytes(u.mem_read(address, size)).hex(),
                after=(value & ((1 << (size * 8)) - 1)).to_bytes(size, 'little').hex()))

    def identity(self, pointer):
        return self.identities.get(pointer, self.type_names.get(pointer, hex(pointer)))

    def placement(self):
        get, u = self.get, self.u
        xy = lambda address: list(struct.unpack('<2h', u.mem_read(address, 4)))
        return dict(current_product=self.identity(get(0x88098C)),
            current_type=self.type_names.get(get(0x880990)), issuer=get(0x880994),
            active_foundation=hex(get(0x880964)), anchor=xy(0x88095C), offset=xy(0x880960),
            can_place=u.mem_read(0x880968, 1)[0], adjacent=u.mem_read(0x880969, 1)[0],
            saved_product=self.identity(get(0x880978)),
            saved_type=self.type_names.get(get(0x88097C)), saved_issuer=get(0x880980),
            saved_foundation=hex(get(0x880974)), saved_anchor=xy(0x88096A),
            saved_offset=xy(0x88096E))

    def snapshot(self):
        u, get = self.u, self.get
        objects = {}
        for name, pointer in dict(yard=self.yard, barracks=self.bar, **self.products).items():
            objects[name] = dict(pointer=hex(pointer), native_id=get(pointer + 0x10),
                type=self.type_names.get(get(pointer + 0x520)), owner=self.identity(get(pointer + 0x21C)),
                health=struct.unpack('<i', u.mem_read(pointer + 0x6C, 4))[0],
                alive=u.mem_read(pointer + 0x90, 1)[0], limbo=u.mem_read(pointer + 0x81, 1)[0],
                marked=u.mem_read(pointer + 0x74, 1)[0],
                location=list(struct.unpack('<3i', u.mem_read(pointer + 0x9C, 12))),
                mission=struct.unpack('<i', u.mem_read(pointer + 0xAC, 4))[0],
                queued_mission=struct.unpack('<i', u.mem_read(pointer + 0xB4, 4))[0],
                body_state=struct.unpack('<i', u.mem_read(pointer + 0x534, 4))[0],
                queued_body_state=struct.unpack('<i', u.mem_read(pointer + 0x538, 4))[0],
                mission_timer=bytes(u.mem_read(pointer + 0xC8, 12)).hex())
        factories = {}
        for name, offset in (('GAPOWR', 0x53BC), ('GAPILL', 0x53CC)):
            pointer = get(self.er.HOUSE + offset)
            factories[name] = None if not pointer else dict(pointer=hex(pointer),
                product=self.identity(get(pointer + 0x58)), stage=get(pointer + 0x24),
                balance=struct.unpack('<i', u.mem_read(pointer + 0x60, 4))[0],
                suspended=u.mem_read(pointer + 0x70, 1)[0], queue_count=get(pointer + 0x50),
                rate=get(pointer + 0x38), stage_timer=bytes(u.mem_read(pointer + 0x2C, 12)).hex())
        selected = [self.identity(get(get(0xA8ECBC) + i * 4)) for i in range(get(0xA8ECC8))]
        event_count, event_top = get(0xA802C8), get(0xA802CC)
        events = [bytes(u.mem_read(0xA802D4 + ((event_top + i) & 127) * 111, 111)).hex()
                  for i in range(event_count)]
        factory_order = [get(get(0xA83E34) + i * 4) for i in range(get(0xA83E40))]
        factory_registry = [dict(pointer=hex(pointer), id=self.identity(pointer))
                            for pointer in factory_order]
        sidebar_entries = {}
        building_names = {get(pointer + 0xDF8): name
                          for name, pointer in self.f.types.items()
                          if name in ('GACNST', 'GAPOWR', 'GAPILE', 'GAPILL')}
        for tab, strip in enumerate(STRIPS):
            entries = []
            for slot in range(get(strip + 0x54)):
                address = strip + 0x58 + slot * 52
                index, kind, category, factory = struct.unpack('<4I', u.mem_read(address, 16))
                # +60 is category metadata; +64 is the actual Factory link
                # written by6A6140 and used by6AAD00 after5BDC80's RET8.
                live = factory in factory_order
                entries.append(dict(slot=slot, type_index=index, kind=kind,
                    type=building_names.get(index) if kind in (6, 7) else None,
                    category_metadata=category, factory=self.identity(factory),
                    factory_pointer=hex(factory), factory_live=live,
                    linked_stage=get(factory + 0x24) if live else None,
                    linked_product=self.identity(get(factory + 0x58)) if live else None,
                    linked_rate=get(factory + 0x38) if live else None,
                    linked_suspended=u.mem_read(factory + 0x70, 1)[0] if live else None))
            sidebar_entries[str(tab)] = entries
        return dict(frame=get(self.bc.FRAME), placement=self.placement(), objects=objects,
            factories=factories, selected=selected, follow=self.identity(get(0x880988)),
            follow_enabled=u.mem_read(0x880984, 1)[0],
            modes=dict(zip(('repair', 'sell', 'power'), u.mem_read(0x88099A, 3))),
            selection_mode=get(0xB0FE54), across_map=u.mem_read(0xB0FE64, 1)[0],
            action_timer=bytes(u.mem_read(0xB0EA80, 12)).hex(),
            camera=bytes(u.mem_read(0xB0CE08, 16)).hex(),
            credits=struct.unpack('<i', u.mem_read(self.er.HOUSE + 0x30C, 4))[0],
            factory_registry=factory_registry, sidebar_entries=sidebar_entries,
            out_list=dict(count=event_count, top=event_top, tail=get(0xA802D0), events=events), rng=self.f.rng())

    def step(self, command, operation, **inputs):
        self.f.phase = command
        before = self.snapshot()
        marks = [len(x) for x in (self.calls, self.writes, self.transport,
                  self.f.draws, self.f.advances, self.f.trace, self.f.events)]
        result = operation()
        after = self.snapshot()
        streams = (self.calls, self.writes, self.transport,
                   self.f.draws, self.f.advances, self.f.trace, self.f.events)
        evidence = [copy.deepcopy(stream[mark:]) for stream, mark in zip(streams, marks)]
        return dict(command=command, inputs=inputs, result=result,
            before=before, after=after, native_calls=evidence[0], timer_writes=evidence[1],
            os_transport=evidence[2], rng_requests=evidence[3], rng_advances=evidence[4],
            lifecycle_trace=evidence[5], runtime_arena_events=evidence[6],
            native_code_unchanged=self.f.code_unchanged())

    def produce(self, name, tab, offset):
        get, u = self.get, self.u
        index = get(self.f.types[name] + 0xDF8)
        self.invoke(0x6A8710, STRIPS[tab], 6, index)
        result = self.invoke(0x4FA350, self.er.HOUSE, 6, index, 0, 0)
        factory = get(self.er.HOUSE + offset)
        assert factory and result != 3
        self.factories[name] = factory
        self.identities.setdefault(factory, name + '_factory')
        visits, changes = 0, []
        for _ in range(2500):
            frame = get(self.bc.FRAME) + 1
            u.mem_write(self.bc.FRAME, dwords(frame))
            old = bytes(u.mem_read(factory + 0x24, 0x50))
            self.invoke(0x4C9B20, factory)
            visits += 1
            if old != bytes(u.mem_read(factory + 0x24, 0x50)):
                changes.append(dict(frame=frame, stage=get(factory + 0x24),
                    balance=get(factory + 0x60), timer=bytes(u.mem_read(factory + 0x2C, 12)).hex()))
            if get(factory + 0x24) == 54:
                break
        assert self.invoke(0x4CA130, factory) & 255
        product = self.invoke(0x4CA160, factory)
        self.products[name] = product
        self.identities[product] = name + '_product'
        self.invoke(0x6A8B30, STRIPS[tab], 0, 0)
        return dict(begin_result=result, factory_visits=visits,
                    changed_visits=changes, ready_product=self.identity(product))

    def start_without_visits(self, name, offset):
        result = self.invoke(0x4FA350, self.er.HOUSE, 6,
                             self.get(self.f.types[name] + 0xDF8), 0, 0)
        factory = self.get(self.er.HOUSE + offset)
        assert factory
        self.factories[name] = factory
        self.identities[factory] = name + '_factory'
        product = self.get(factory + 0x58)
        self.products[name] = product
        self.identities[product] = name + '_product'
        return result

    def click(self, name, *, right=False, kind=6, tab=None, shift=False):
        if tab is None:
            tab = 1 if name == 'GAPILL' else 0
        index = self.get(self.f.types[name] + 0xDF8)
        slots = [slot for slot in range(self.get(STRIPS[tab] + 0x54))
                 if self.get(STRIPS[tab] + 0x58 + slot * 52) == index
                 and self.get(STRIPS[tab] + 0x5C + slot * 52) == kind]
        assert len(slots) == 1, (name, slots)
        visible_slot = slots[0] - 2 * self.get(STRIPS[tab] + 0x44)
        assert 0 <= visible_slot < 60
        return self.invoke(0x6AAD00, 0xB07E80 + (tab * 60 + visible_slot) * 56,
                           0x10 if right else 1, self.key, int(shift))

    def preview(self, xy):
        self.u.mem_write(self.cell_arg, struct.pack('<2h', *xy))
        self.invoke(0x4A91B0, SIDEBAR, self.out_cell, self.cell_arg)
        return self.placement()

    def left_up(self, xy):
        self.u.mem_write(self.cell_arg, struct.pack('<2h', *xy))
        return self.invoke(0x4AB9B0, SIDEBAR, 0, self.cell_arg, 0, 0, self.key)

    def receive(self):
        self.u.reg_write(UC_X86_REG_ESP, self.bc.SP)
        return run_checked(self.u, 0x6474C7, 0x6474BF, count=10_000_000)

    def receiver(self, name, xy):
        self.u.mem_write(self.cell_arg, struct.pack('<2h', *xy))
        return self.invoke(0x4FB0E0, self.er.HOUSE, 6,
                           self.get(self.f.types[name] + 0xDF8), 0, self.cell_arg)

    def abandon(self, name):
        return self.invoke(0x4FAA10, self.er.HOUSE, 6,
                           self.get(self.f.types[name] + 0xDF8), 0, 0)

    def expire(self, pointer):
        from unicorn.x86_const import UC_X86_REG_EDX
        self.u.reg_write(UC_X86_REG_EDX, 1)
        return self.invoke(0x7258D0, pointer)

    def stationary_blocker(self, xy):
        pointer, coord = self.f.allocate(0x1000), self.f.allocate(16)
        self.invoke(0x43B740, pointer, self.f.types['GAPILL'], self.er.HOUSE)
        self.identities[pointer] = 'stationary_blocker'
        self.products['blocker'] = pointer
        self.u.mem_write(coord, dwords(xy[0] * 256 + 128, xy[1] * 256 + 128, 0))
        target = self.get(self.get(pointer) + 0xD8)
        result = self.invoke(target, pointer, coord, 0)
        assert result & 255
        return dict(pointer=hex(pointer), unlimbo_entry=hex(target), result=result)

    def select_and_follow(self):
        result = self.invoke(0x5F4520, self.f.source)
        self.invoke(0x4AEB30, SIDEBAR, self.f.source)
        self.u.mem_write(0xB0FE54, dwords(1))
        self.u.mem_write(0xB0FE64, b'\1')
        return result

    def save(self):
        self.baseline = [(low, bytes(self.u.mem_read(low, high - low + 1)))
                         for low, high, _flags in self.u.mem_regions()]
        self.baseline_cpu = self.u.context_save()
        self.baseline_heap = self.f.heap
        self.baseline_dicts = copy.deepcopy((self.identities, self.products, self.factories))
        self.baseline_audio = {key: copy.deepcopy(value) for key, value in self.f.platform_audio.__dict__.items()
                               if key not in ('owner', 'root', 'prepared_files')}

    def restore(self):
        for low, raw in self.baseline:
            self.u.mem_write(low, raw)
        self.u.context_restore(self.baseline_cpu)
        self.f.heap = self.baseline_heap
        self.identities, self.products, self.factories = copy.deepcopy(self.baseline_dicts)
        for key, value in self.baseline_audio.items():
            setattr(self.f.platform_audio, key, copy.deepcopy(value))
        self.call_trace.pending.clear()
        self.f.pending.clear()


def ready_building_histories(root=None, *, inputs=None):
    root = Path(root or os.environ['VERA20K_READY_BUILDING_ASSETS'])
    inputs = inputs or ready_building_inputs(root)
    m = ReadyBuildingFixture(inputs, root)
    rows = [dict(id='paid_stock_ready', inputs=dict(seed=2,
                  rng_seeds={'main': 2, 'scenario': 2, 'mapgen': 31}, technology_level=10,
                  initial_credits=10000, builder='GACNST', builder_cell=[14, 14],
                  barracks_cell=[18, 14], mouse_pixels=[320, 240],
                  layers=inputs.ready_layers, physical_art_input_sections=inputs.ready_art,
                  sounds=inputs.ready_sounds, flat_map=m.f.flat_map,
                  reused_native_rule_layers=inputs.death_layers,
                  native_types=[dict(name=name, pointer=hex(inputs.types[name]),
                      array_index=m.get(inputs.types[name] + 0xDF8),
                      cost=struct.unpack('<i', m.u.mem_read(inputs.types[name] + 0x610, 4))[0],
                      build_cat=m.get(inputs.types[name] + 0xE08),
                      factory=m.get(inputs.types[name] + 0xEB8))
                      for name in ('GACNST', 'GAPOWR', 'GAPILE', 'GAPILL')],
                  physical_files=[dict(file=name, sha256=hashlib.sha256((root / name).read_bytes()).hexdigest())
                      for name in ('ARTMD.INI', 'SOUNDMD.INI', 'CLEAR01.TEM',
                                   'GGPILL.SHP', 'PILLICON.SHP', 'GAPILLMK.SHP')],
                  initialization=dict(display_fields=['0x004A8738', '0x004A8847'],
                    mouse_fields=['0x005BDA48', '0x005BDA74'],
                    ww_mouse_fields=['0x007B8730', '0x007B8856'],
                    empty_cell='0x004F50A0', preview_sentinels='0x006A4BC0',
                    select_controls='0x006A4DC0', select_zones='0x006A8220',
                    building_built_counter='0x0049F9B0@House+55A0',
                    house_factory_listener_registration=m.house_factory_registration,
                    supplied_selection_capacity=64, wall_ms=0,
                    scope='Selected initialized regions/services and supplied admitted House/map/builders; whole platform startup is excluded')),
                 steps=[m.step('produce_GAPOWR', lambda: m.produce('GAPOWR', 0, 0x53BC)),
                        m.step('produce_GAPILL', lambda: m.produce('GAPILL', 1, 0x53CC))])]
    m.save()
    rows.extend(ready_building_cases(m))
    return rows


def ready_building_retained_factory_cases(m, common):
    """Original NULL-held, retained link, rearm and direct recheck boundaries."""
    rows = []
    m.restore()
    rows.append(dict(id='current_product_expiry_right_receiver', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('dispatch_pointer_expired', lambda: m.expire(m.products['GAPOWR']),
               target='product', control_dl=1, boundary='Whole7258D0; no fatal destructor'),
        m.step('tactical_right_up', lambda: m.invoke(0x4AAD30, SIDEBAR, m.key)),
        m.step('left_cameo', lambda: m.click('GAPILL'), type='GAPILL'),
        m.step('right_cameo', lambda: m.click('GAPOWR', right=True), type='GAPOWR'),
        m.step('local_event_corridor', m.receive)]))
    m.restore()
    rows.append(dict(id='saved_product_expiry_right_receiver', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
        m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18]),
        m.step('dispatch_pointer_expired', lambda: m.expire(m.products['GAPOWR']),
               target='product', control_dl=1, boundary='Whole7258D0; no fatal destructor'),
        m.step('left_cameo', lambda: m.click('GAPILL'), type='GAPILL'),
        m.step('right_cameo', lambda: m.click('GAPOWR', right=True), type='GAPOWR'),
        m.step('local_event_corridor', m.receive)]))
    m.restore()
    rows.append(dict(id='null_product_direct_abandon_then_no_link_right', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('dispatch_pointer_expired', lambda: m.expire(m.products['GAPOWR']),
               target='product', control_dl=1, boundary='Whole7258D0; no fatal destructor'),
        m.step('house_abandon', lambda: m.abandon('GAPOWR'), type='GAPOWR'),
        m.step('left_cameo', lambda: m.click('GAPILL'), type='GAPILL'),
        m.step('right_cameo', lambda: m.click('GAPOWR', right=True), type='GAPOWR'),
        m.step('local_event_corridor', m.receive)]))
    m.restore()
    rows.append(dict(id='null_product_requested_category_guards', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('dispatch_pointer_expired', lambda: m.expire(m.products['GAPOWR']),
               target='product', control_dl=1, boundary='Whole7258D0; no fatal destructor'),
        m.step('house_abandon', lambda: m.abandon('GACNST'), type='GACNST',
               boundary='Direct category0 requested-index control, not a buildable-cameo trigger'),
        m.step('house_abandon', lambda: m.abandon('GAPILE'), type='GAPILE',
               boundary='Direct nonzero-category mismatched-index control on NULL held product')]))
    m.restore()
    rows.append(dict(id='defense_null_product_direct_abandon', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPILL'), type='GAPILL'),
        m.step('dispatch_pointer_expired', lambda: m.expire(m.products['GAPILL']),
               target='product', type='GAPILL', control_dl=1,
               boundary='Whole7258D0; no fatal destructor'),
        m.step('house_abandon', lambda: m.abandon('GAPILL'), type='GAPILL')]))
    for case_id, active, expire, category in (
        ('main_active_factory_queue_recheck', 'GAPOWR', False, 0),
        ('defense_active_factory_queue_recheck_helper', 'GAPILL', False, 5),
        ('defense_client_main_factory_queue_recheck', 'GAPILL', False, 0),
        ('main_null_product_factory_queue_recheck', 'GAPOWR', True, 0),
        ('defense_null_product_factory_queue_recheck_helper', 'GAPILL', True, 5)):
        m.restore()
        steps = [m.step('left_cameo', lambda active=active: m.click(active), type=active)]
        if expire:
            steps.append(m.step('dispatch_pointer_expired',
                lambda active=active: m.expire(m.products[active]), target='product', type=active,
                control_dl=1, boundary='Whole7258D0; no fatal destructor'))
        else:
            steps.append(m.step('supplied_builder_limbo',
                lambda: m.u.mem_write(m.yard + 0x81, b'\1'), builder='yard', limbo=True,
                boundary='Explicit admitted-prior mutation; not whole Limbo'))
        steps.append(m.step('house_update_factory_queue',
            lambda category=category: m.invoke(0x509140, m.er.HOUSE, 7, 0, category),
            factory_rtti=7, naval=False, build_cat=category,
            boundary='Whole original509140 direct helper. StockGACNST active callers push category0; category5 is helper-only. Caller scheduling is not emulated'))
        rows.append(dict(id=case_id, inputs=common, steps=steps))
    m.restore()
    rows.append(dict(id='saved_main_factory_queue_recheck_receiver', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
        m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18]),
        m.step('supplied_builder_limbo', lambda: m.u.mem_write(m.yard + 0x81, b'\1'),
               builder='yard', limbo=True, boundary='Explicit admitted-prior mutation; not whole Limbo'),
        m.step('house_update_factory_queue', lambda: m.invoke(0x509140, m.er.HOUSE, 7, 0, 0),
               factory_rtti=7, naval=False, build_cat=0,
               boundary='Whole original509140 direct helper; caller scheduling is not emulated'),
        m.step('local_event_corridor', m.receive)]))
    m.restore()
    rows.append(dict(id='expired_product_same_category_rearm_retained_old_right', inputs=common, steps=[
        m.step('dispatch_pointer_expired', lambda: m.expire(m.products['GAPOWR']),
               target='product', control_dl=1, boundary='Whole7258D0; no fatal destructor'),
        m.step('produce_GAPILE', lambda: m.produce('GAPILE', 0, 0x53BC), type='GAPILE'),
        m.step('left_cameo', lambda: m.click('GAPILL'), type='GAPILL'),
        m.step('right_cameo', lambda: m.click('GAPOWR', right=True), type='GAPOWR'),
        m.step('local_event_corridor', m.receive)]))
    m.restore()
    rows.append(dict(id='expired_product_same_category_rearm_retained_old_left', inputs=common, steps=[
        m.step('dispatch_pointer_expired', lambda: m.expire(m.products['GAPOWR']),
               target='product', control_dl=1, boundary='Whole7258D0; no fatal destructor'),
        m.step('produce_GAPILE', lambda: m.produce('GAPILE', 0, 0x53BC), type='GAPILE'),
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
        m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18]),
        m.step('local_event_corridor', m.receive)]))
    m.restore()
    rows.append(dict(id='rearmed_factory_sidebar_abandon_unlinks_all_retains_factory', inputs=common, steps=[
        m.step('dispatch_pointer_expired', lambda: m.expire(m.products['GAPOWR']),
               target='product', control_dl=1, boundary='Whole7258D0; no fatal destructor'),
        m.step('produce_GAPILE', lambda: m.produce('GAPILE', 0, 0x53BC), type='GAPILE'),
        m.step('sidebar_abandon_factory',
            lambda: m.invoke(0x6ABAD0, SIDEBAR, 6,
                             m.get(m.f.types['GAPILE'] + 0xDF8), m.factories['GAPILE']),
            type='GAPILE', kind=6, factory='GAPOWR_factory',
            boundary='Whole original6ABAD0 ->6ABB60 direct owner boundary; not whole HouseAbandon or mobile delivery')]))
    m.restore()
    rows.append(dict(id='local_unlinked_strip_clear_active_abandon_order', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('strip_clear', lambda: m.invoke(0x6A81B0, STRIPS[0]), tab=0,
               boundary='Whole original6A81B0 direct initialized helper; not whole UI/world reset'),
        m.step('house_abandon', lambda: m.abandon('GAPOWR'), type='GAPOWR')]))
    return rows


def ready_building_cases(m):
    rows = []
    common = dict(source='paid_stock_ready executed VM/CPU/OS-transport bytes',
                  building_types=['GACNST', 'GAPOWR', 'GAPILL', 'GAPILE'],
                  technology_level=10, builder_cell=[14, 14], barracks_cell=[18, 14],
                  mouse_pixels=[320, 240], cell=[13, 18])
    m.restore()
    rows.append(dict(id='fresh_same_different_right_then_place', inputs=common, steps=[
        m.step('select_and_follow_prior', m.select_and_follow,
               selected=['selected_mtnk'], follow='selected_mtnk', selection_mode=1, across_map=True,
               boundary='Whole ObjectSelect5F4520/follow4AEB30; supplied prior navigation scope'),
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('left_cameo', lambda: m.click('GAPILL'), type='GAPILL'),
        m.step('tactical_right_up', lambda: m.invoke(0x4AAD30, SIDEBAR, m.key)),
        m.step('left_cameo', lambda: m.click('GAPILL'), type='GAPILL'),
        m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
        m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18]),
        m.step('local_event_corridor', m.receive)]))
    for right_type in ('GAPOWR', 'GAPILL'):
        m.restore()
        rows.append(dict(id='right_own_factory_' + right_type, inputs=common, steps=[
            m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
            m.step('right_cameo', lambda: m.click(right_type, right=True), type=right_type),
            m.step('local_event_corridor', m.receive)]))
    m.restore()
    rows.append(dict(id='abandon_mismatch_then_other_then_missing_factory', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('house_abandon', lambda: m.abandon('GAPILE'), type='GAPILE'),
        m.step('house_abandon', lambda: m.abandon('GAPILL'), type='GAPILL'),
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('house_abandon', lambda: m.abandon('GAPILL'), type='GAPILL'),
        m.step('direct_receiver', lambda: m.receiver('GAPILL', (13, 18)), type='GAPILL', cell=[13, 18])]))
    m.restore()
    rows.append(dict(id='direct_empty_and_negative_cell_guards', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('direct_receiver', lambda: m.receiver('GAPOWR', (0, 0)), type='GAPOWR', cell=[0, 0]),
        m.step('direct_receiver', lambda: m.receiver('GAPOWR', (-1, -1)), type='GAPOWR', cell=[-1, -1])]))
    m.restore()
    rows.append(dict(id='incomplete_factory_receiver_guard', inputs=common, steps=[
        m.step('house_abandon', lambda: m.abandon('GAPOWR'), type='GAPOWR'),
        m.step('begin_without_factory_visits', lambda: m.start_without_visits('GAPOWR', 0x53BC),
               type='GAPOWR', factory_visits=0),
        m.step('left_cameo', lambda: m.click('GAPILL'), type='GAPILL'),
        m.step('direct_receiver', lambda: m.receiver('GAPOWR', (13, 18)), type='GAPOWR', cell=[13, 18])]))
    m.restore()
    rows.append(dict(id='local_invalid_site_retains_then_valid_place', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('preview_cell', lambda: m.preview((14, 14)), cell=[14, 14]),
        m.step('tactical_left_up', lambda: m.left_up((14, 14)), cell=[14, 14]),
        m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
        m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18]),
        m.step('local_event_corridor', m.receive)]))
    for first, second in (('GAPILL', 'GAPOWR'), ('GAPOWR', 'GAPILL')):
        m.restore()
        steps = []
        for name in (first, second):
            steps += [m.step('left_cameo', lambda name=name: m.click(name), type=name),
                      m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
                      m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18])]
        steps.append(m.step('local_event_corridor', m.receive))
        rows.append(dict(id='queued_same_cell_' + first + '_then_' + second,
                         inputs=common, steps=steps))
    m.restore()
    steps = [m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
             m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
             m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18]),
             m.step('direct_stationary_blocker', lambda: m.stationary_blocker((13, 18)),
                    type='GAPILL', cell=[13, 18],
                    boundary='Whole43B740 and440580; direct caller, not trigger/map-loader proof'),
             m.step('local_event_corridor', m.receive),
             m.step('house_abandon', lambda: m.abandon('GAPOWR'), type='GAPOWR')]
    rows.append(dict(id='admitted_then_blocked_retry_then_abandon', inputs=common, steps=steps))
    for queued_abandon in (False, True):
        m.restore()
        steps = [m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
                 m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
                 m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18])]
        if queued_abandon:
            steps.append(m.step('right_cameo', lambda: m.click('GAPILL', right=True), type='GAPILL'))
        else:
            steps += [m.step('left_cameo', lambda: m.click('GAPILL'), type='GAPILL'),
                      m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
                      m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18])]
        steps += [m.step('direct_stationary_blocker', lambda: m.stationary_blocker((13, 18)),
                         type='GAPILL', cell=[13, 18],
                         boundary='Whole43B740 and440580; direct caller, not trigger/map-loader proof'),
                  m.step('local_event_corridor', m.receive)]
        rows.append(dict(id='queued_failure_then_' + ('abandon' if queued_abandon else 'failure'),
                         inputs=common, steps=steps))
    for target, saved in (('product', False), ('type', False),
                          ('product', True), ('type', True)):
        m.restore()
        steps = [m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR')]
        if saved:
            steps += [m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
                      m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18])]
        pointer = m.products['GAPOWR'] if target == 'product' else m.f.types['GAPOWR']
        steps.append(m.step('dispatch_pointer_expired', lambda: m.expire(pointer),
                            target=target, control_dl=1,
                            boundary='Whole7258D0 on initialized object; no destructor proof'))
        if target == 'product':
            steps.append(m.step('local_event_corridor' if saved else 'tactical_left_up',
                                m.receive if saved else lambda: m.left_up((13, 18)), cell=[13, 18]))
        rows.append(dict(id=('saved_' if saved else 'current_') + target + '_expiry',
                         inputs=common, steps=steps))
    m.restore()
    rows.append(dict(id='alive_false_is_not_pointer_expiry', inputs=common, steps=[
        m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR'),
        m.step('supplied_product_alive', lambda: m.u.mem_write(m.products['GAPOWR'] + 0x90, b'\0'),
               product='GAPOWR_product', alive=False, boundary='Explicit Alive byte; no fatal death/destructor'),
        m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
        m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18])]))
    for before_cameo in (False, True):
        m.restore()
        steps = [m.step('left_cameo', lambda: m.click('GAPOWR'), type='GAPOWR')]
        if not before_cameo:
            steps += [m.step('preview_cell', lambda: m.preview((13, 18)), cell=[13, 18]),
                      m.step('tactical_left_up', lambda: m.left_up((13, 18)), cell=[13, 18])]
        steps.append(m.step('supplied_builder_limbo', lambda: m.u.mem_write(m.yard + 0x81, b'\1'),
                            builder='yard', limbo=True, boundary='Explicit admitted-prior mutation; not whole Limbo'))
        steps += ([m.step('left_cameo', lambda: m.click('GAPILL'), type='GAPILL')]
                  if before_cameo else [])
        steps.append(m.step('local_event_corridor', m.receive))
        rows.append(dict(id='builder_limbo_' + ('before_repeat_cameo' if before_cameo else 'after_admission'),
                         inputs=common, steps=steps))
    rows.extend(ready_building_retained_factory_cases(m, common))
    return rows


def generate():
    return dict(schema=1, comparisons=pair_controls(), histories=histories(),
                cost_contexts=cost_contexts(), capacity_history=capacity_history(),
                recharge_reader=recharge_reader(),
                recalculate_scroll=recalculate_scroll_controls(),
                ready_building_histories=ready_building_histories())


def metadata():
    record = provenance(
        scope='Original whole Strip CompareItems/InsertEntry, AddCameo admission through insertion, '
              'Recalculate retained filter/removal and complete pruning-scroll controls, '
              'and SuperWeaponType RechargeTime reader block. '
              'Synthetic initialized ordering inputs; additive original paid ready-building retained '
              'SelectAction/ManualPlace/local event/receiver and expiry histories use existing '
              'initialized physical joined readers/VM. Not a complete tech-tree, native scene, or whole-sidebar claim.',
        assumptions=[
            'Fresh PE mapping per case family. Actual RTTI_To_TypeArray, original type vtables, '
            'Cost_Of country/FactoryPlant arithmetic and UTF-16 wcscmp execute. Type/House inputs '
            'are supplied fields, not a claim of native constructor/INI initialization. FPCW0E7F.',
            'All 33 by33 pair combinations include type-class flag gating, both flags set, same-side '
            'priority, signed TechLevel/cost/recharge, name case, supplementary UTF-16 and exact ties.',
            'Four six-type cost contexts execute24 complete cost queries and144 comparisons with '
            'country/plant factors, all five cost slots and the BuildingType FreeUnit adjustment. '
            'PadAircraft contains an initialized nonmatching first Dock; SeparateAircraft is true.',
            'Four strips are constructed by original6A80A0. AddCameo starts at6A6300 and stops at '
            '6A6423 after original insertion or6A65FF before rejection return. Its remaining UI '
            'presentation tail is excluded. The three ordinary histories remain within75 slots.',
            'A separate capacity history executes the native76th-entry overrun into mapped adjacent '
            'storage, then rejection at76, prune to75 and retry. It proves the admission gate and '
            'addition-before-removal consequence, not integrity of a native oversized Sidebar. '
            'Rust must keep memory safe; adjacent overwritten state is not a golden.',
            'Recalculate starts at6AA600 and stops at6AAAB3 after the complete retained filter. '
            'Supers granted bits are supplied. Entries have no linked production factories and '
            'House primary-factory fields are zero; original500510 executes on removals. '
            'The original histories exclude cancellation events, tab selection and scroll adjustment.',
            'The additive recalculate_scroll cases instead execute whole6AA600 to its RET. '
            'Original AddCameo populates the source vehicle strip; cases restore those executed '
            'bytes and supply TopRow, geometry and unavailable IDs. Actual prologue capacity, '
            'visible record snapshots, snapshot erasure and6AABC9..6AAC76 row adjustment execute. '
            'No Python algorithm supplies expected rows. Original69DCF0 initializes all four '
            'tab gadgets so the all-entries-gone disable path runs. Other strips remain empty; '
            'switching to another populated tab and linked-factory cancellation remain excluded.',
            'RechargeTime uses original constructor6CE5C0..6CE681 field initialization and '
            'reader6CED5C..6CED95 with original ReadDouble and ftol, supplied cached INI lookup, '
            'and original floating-scanner startup. Whole SuperWeaponType reader is excluded.',
            'Additive ready_building_histories reuse building_death_anims.joined_inputs/joined_fixture, '
            'building_construction, EngineerJoinedFixture, the one native arena, physical sound/ART '
            'readers and native flat-map connectivity/Pathfinder. The old seven family payloads are '
            'unchanged. Only the new family requires VERA20K_READY_BUILDING_ASSETS.',
            'Original selected BuildingType registration45E2ED..45E362 produces GACNST/GAPOWR/GAPILE/GAPILL '
            'indices; whole45DD90/45FE50 read GAPILL over RULESMD, optional LANGRULE, Battle mode and '
            'Hills map. GUIBuildSound6693A6..6693F1 and BuildingSlam66AA7F..66AACF execute original '
            'ReadString528A10/FindIndex7514D0 retaining constructor/current defaults. Selected sound '
            'indices and object pointers are fixture-relative; original unlayered SOUNDMD supplies names/samples.',
            'The new ready baseline executes original healthy GACNST/GAPILE whole43B740 constructors '
            'then uses existing explicit admitted preplaced priors and original Mark/Logic admission. '
            'House technology10, credits10000, empty FactoryPlant, local House number0, four strips '
            'and flat clear map are declared inputs. Whole preplaced map/House constructors are excluded.',
            'Original House interface constructor region4F5DCC..4F5E02, Factory-expiry listener '
            'registration4F5EB8..4F5F0B intoB0F640, BuildingBuilt Counter49F9B0 at55A0, '
            'House registry insertion, skirmish store52E10F..119, Factory/selection/preview/Select CRT owners '
            'and original Display/Mouse/WWMouse constructor regions execute. The WWMouse region stops '
            'before opaque platform wait/window setup; supplied physical pointer is320,240, timeGetTime0. '
            'No full GUI startup or hardware cursor drawing is claimed.',
            'Paid GAPOWR and separate Defense GAPILL factories are created by whole4FA350/4C9C70. '
            'Whole4C9B20 visits monotonically increasing frames until original stage54; no ready stage '
            'is supplied. Payment, timers, balance and original IsComplete/GetObject/linking are observed. '
            'Each subsequent history restores those executed ready VM/CPU/OS-transport bytes. Timer '
            'padding is raw observation, not a meaningful semantic field.',
            'Actual Factory construction-arrayA83E30 order and retained strip records are observed. '
            'Entry+08 is BuildCat metadata; entry+0C/strip+64 is the actual Factory link written '
            'by6A6140. NULL-held same-category GAPILE rearm retains the original Factory identity '
            'and array order while both old GAPOWR and new GAPILE entries reference it. Original '
            '6ABB60 clears all same-Factory links on admitted HouseAbandon. Link publication at '
            '4FA6AA occurs only for the local House after4FA61E; no-builder/Start-refused and '
            'queued-tail4FA5F8 returns bypass it. Same suspended-type shortcut passes it. The '
            'queued-tail guard is instruction-supported, not an executed forged building queue.',
            'The additive rearmed_factory_sidebar_abandon_unlinks_all_retains_factory control '
            'restores the executed paid baseline, expires GAPOWR through7258D0, and pays for '
            'GAPILE in the same live Factory. Whole SidebarAbandon6ABAD0(6,GAPILE index,Factory) '
            'executes Strip6ABB60 at a declared direct owner boundary, clearing both retained '
            'GAPOWR/GAPILE links while preserving Factory identity/registry order and the '
            'independent Defense GAPILL Factory/link. It refunds GAPILE once, destroys its '
            'limbo product and dispatches expiry; all three RNG streams and zero draws/advances '
            'are recorded. No queue or mobile state is forged, and mobile cancellation/delivery '
            'caller reachability is instruction-supported rather than an executed mobile chain.',
            'The additive local_unlinked_strip_clear_active_abandon_order control restores '
            'the executed paid GAPOWR/GAPILL baseline, enters GAPOWR through whole Select, '
            'then executes whole StripClear6A81B0(tab0) before whole HouseAbandon4FAA10. '
            'This direct initialized helper clears Building-strip records without settling '
            'the live Factory. It is not whole UI/scenario reset or mobile execution. '
            'The zero-link Sidebar pass has no inner FactoryAbandon;4FAB83 retains current '
            'product/type/foundation, whereas product expiry7258D0 sees the cleared cursor '
            'after the House prefix and before core Factory deletion. Existing linked '
            'cancellation instead refunds/destroys/expires before6ABBDF unlink and the '
            'House cursor prefix. StripClear initializes75 slot clocks outside the existing '
            'stage/building/action timer watch windows; no whole reset/timer claim. Its '
            'raw EAX and opaque timer padding are observations. Complete three RNG states, '
            'zero House-abandon draws/advances, the three stage-clock stores and reached '
            'lifecycle calls are retained; no link, stage or expected result is authored.',
            'Old GAPOWR Left after same-Factory paid GAPILE rearm executes6AB2CC GetObject, '
            'ObjectFindFactory on the actual held GAPILE and ManualPlace(yard,GAPILE product). '
            'Current and saved cursor/event identity is GAPILE, not the requested old cameo type. '
            'Whole tactical producer and local receiver successfully place GAPILE and remove '
            'the Factory; no Python caller substitutes the held type or placement result.',
            'Whole SelectAction6AAD00 -> ObjectFindFactory5F5C20 -> TypeFindFactory5F7900 -> ManualPlace4FB840 '
            'executes on actual paid products. Fresh ManualPlace calls UnselectAll48DC90; both same '
            'and different ready building repeats refuse at the nonnull current-type guard. Sound and '
            'builder query occur before that guard. Current/saved issuer880994/880980 is House+30, not '
            'a builder pointer. 4A91B0 owns map-cell foundation preview coordinates, not the camera.',
            'Whole tactical left/right producers, actual111-byte OutList events and local corridor '
            '6474C7..6474BF ->64C380 ->4C6CB0 ->HousePlace4FB0E0 execute. Only actual failed Building '
            'Unlimbo440580 restores latest saved product/type; early missing/incomplete/null-held/builder '
            'or sentinel guards do not. Both same-cell event orders, two failures and fail-then-Abandon '
            'preserve original order. An independent stationary blocker executes whole43B740/440580 '
            'at a declared direct caller boundary, not a whole trigger/map-loader/tick.',
            'Whole7258D0 calls actual FactoryPointerExpired4CA580 and TacticalPointerExpired6DA560. '
            'Current product expiry retains type/foundation and prevents tactical event emission; '
            'saved product expiry clears saved product/type/issuer/foundation. Current type expiry '
            'clears current type/foundation but retains product; saved type expiry alone retains backup. '
            'Alive=false and builder Limbo=true are explicit input changes, not fatal/Limbo producers. '
            'Original Abandon/destructor/expiry reached by cancellation execute against initialized '
            'services. Full fatal death, all destructor classes and scenario/save/modal-resume loading are excluded.',
            'TacticalRight4AAD30 requires current product and AbstractFlags bit0 before clearing '
            'the cursor; NULL product preserves orphan type/foundation. Own-factory Right6AAD00 '
            'uses the same retained entry link, and its clear-prefix requires a nonnull Building '
            'current product. NULL-held category Factory survives expiry until admitted Abandon '
            'or direct queue recheck. HouseAbandon4FABF6 admits NULL held for nonzero requested '
            'BuildCat without a requested-index match; stockGACNST BuildCat0 is an executed '
            'refusal control, GAPILE nonzero mismatched index and Defense GAPILL admit. NULL-held '
            'FactoryAbandon has no refund, timer write or RNG draw.',
            'Whole HouseUpdateFactoryQueue509140 executes as a direct helper with RTTI7/naval0 '
            'and category0 or helper-only Defense5. Selected active stockGACNST callers push '
            'category0. Supplied builder Limbo makes actual TypeFindFactory refuse. Original '
            '50921C calls FactoryAbandon directly, not HouseAbandon, followed by StartNextQueued. '
            'Original5092A0 scalar deletion4CA770 dispatches Factory expiry; initialized House '
            'listener4FB9B0 clears the matching slot and original destructor removes live '
            'Factory/object-expiry registry entries. It retains orphan current type/foundation '
            'and can leave a raw old cameo link to released arena bytes. Such links are recorded '
            'as factory_live=false and never dereferenced. Stale freed-pointer Right, whole '
            'builder lifecycle callers, their scheduling and subsequent sidebar recalculate '
            'are excluded; no every-frame recheck claim is made.',
            'Main886B88/Scenario+218 use inherited original seed2; MapGenABE890 uses original seed31. '
            'Every step records complete three RNG states, actual draw/advance sequence, timers and '
            'lifecycle calls; neither expected mutation nor RNG nor placement predicates are implemented '
            'in Python. Native executable sections are checked unchanged.',
        ],
        substitutions=[
            'EVA752700 is an observed no-op sink with its actual RET4 convention.',
            'Recalculate allocator7C8E17 supplies a zeroed visible-copy buffer. Type virtual+94 '
            'FindFactory and House4F7870 CanBuild return declared availability inputs. '
            'Admission producer bodies and complete House/Building lifecycle are not executed.',
            'Complete pruning-scroll controls observe UpdateScrollButtons6A6610 and '
            'operator_delete7C8B3D as no-op sinks with their actual caller-cleaned conventions. '
            'Delete must receive the original supplied allocation. No button appearance or '
            'allocator-internal claim; zero-filled unused snapshot slots are an explicit input.',
            'New ready histories do not use the legacy availability or comparator substitutions. '
            'They retain existing joined native physical asset-cache/allocator/OS-file/DirectSound '
            'transport and declared presentation sinks. Reached WinMM import callsites receive '
            'literal wall_ms0 with caller-owned stack unchanged; the existing6C8C40/646F20 owner '
            'arms are not overridden twice. Full mounted MIX/theater traversal, audible hardware '
            'output, Options dialog platform/resume and drawing are excluded.',
        ], entry_points={'compare': COMPARE, 'insert': INSERT, 'add': ADD,
                         'recalculate': RECALCULATE, 'strip_ctor': 0x6A80A0,
                         'strip_clear': 0x6A81B0, 'wcscmp': 0x7CA5D3,
                         'prune_scroll_tail': 0x6AABC9, 'tab_gadget_ctor': 0x69DCF0,
                         'super_ctor_fields': 0x6CE5C0, 'recharge_reader': 0x6CED5C,
                         'ready_select': 0x6AAD00, 'object_find_factory': 0x5F5C20,
                         'type_find_factory': 0x5F7900, 'manual_place': 0x4FB840,
                         'tactical_left_up': 0x4AB9B0, 'tactical_right_up': 0x4AAD30,
                         'place_event': 0x4C6AE0, 'house_place': 0x4FB0E0,
                         'building_unlimbo': 0x440580, 'house_abandon': 0x4FAA10,
                         'factory_start': 0x4C9C70, 'factory_ai': 0x4C9B20,
                         'house_factory_listener_ctor': 0x4F5EB8,
                         'house_factory_expiry': 0x4FB9B0,
                         'house_update_factory_queue': 0x509140,
                         'factory_scalar_delete': 0x4CA770,
                         'sidebar_link_factory': 0x6A6140,
                         'sidebar_abandon_factory': 0x6ABAD0,
                         'strip_abandon_factory': 0x6ABB60,
                         'pointer_expiry': 0x7258D0, 'tactical_expiry': 0x6DA560,
                         'factory_expiry': 0x4CA580, 'gui_sound_reader': 0x6693A6,
                         'building_slam_reader': 0x66AA7F})
    record['command'] = (
        'VERA20K_GAMEMD_EXE=/absolute/path/gamemd.exe '
        'VERA20K_READY_BUILDING_ASSETS=/absolute/path/extracted-retail-cache '
        'python -B -m tools.sidebar_oracle.cameo_order --check; '
        'replace --check with --write to regenerate')
    return record


def draw_availability_generate():
    """Execute StripDraw's whole-strip busy scan and per-entry overlay decision.

    The catalog, retained request words and Factory getter fields are explicit
    inputs. Type availability is supplied by Sidebar's existing query boundary;
    no admission, event delivery, pixels or production timing is claimed here.
    """
    catalog = [item('GAPOWR', 7), item('GAPILE', 7),
               item('GAPILL', 7, build_cat=5), item('NASAM', 7, build_cat=5),
               item('E1', 16), item('ENGINEER', 16),
               item('MTNK'), item('FV'), item('ORCA', 3)]
    m = Sidebar(catalog)
    u = m.u
    from tools.can_build_oracle import OWNED, ACTIVE, PRODUCED, COUNTER_CAPACITY
    for offset in (*OWNED.values(), *ACTIVE.values(), *PRODUCED):
        m.invoke(0x49F9B0, obj=m.house + offset)
        put32(u, m.house + offset + 4, m.alloc(COUNTER_CAPACITY * 4))
        put32(u, m.house + offset + 8, COUNTER_CAPACITY)
    for entry in catalog:
        m.add(entry['id'])
        # Non-limiting explicit type prior for original CheckBuildLimit.
        put32(u, m.pointers[entry['id']] + 0x3B8, 2147483647)
        put32(u, m.pointers[entry['id']] + 0xDF8,
              m.catalog[entry['id']]['type_index'])
    image_getters = {read(u, VTABLE[TYPE_NAMES[kind]] + 0x98)[0] & 0xFFFFFFFF
                     for kind in (7, 16, 40, 3)}
    def image_boundary(_u, pc, _size, _data):
        if pc in image_getters:
            m.native_return(0, 0)  # Image pointer is not an availability input.
    hook = u.hook_add(UC_HOOK_CODE, image_boundary)
    factory = m.alloc(0x100)
    product = m.alloc(0x100)
    controls = []
    for tab, source in ((0, 'GAPOWR'), (1, 'GAPILL'), (2, 'E1'), (3, 'MTNK')):
        strip = STRIPS[tab]
        names = m.order()[tab]
        source_slot = names.index(source)
        for state, linked, stage, label in (
                (0, False, 0, 'idle'), (1, False, 0, 'pending'),
                (2, False, 0, 'pending_hold'), (3, False, 0, 'state3'),
                (1, True, 0, 'building'), (1, True, 54, 'ready')):
            for other_strip, other_names in zip(STRIPS, m.order()):
                for slot in range(len(other_names)):
                    put32(u, other_strip + 0x64 + slot * 0x34, 0)
                    put32(u, other_strip + 0x68 + slot * 0x34, 0)
            put32(u, strip + 0x68 + source_slot * 0x34, state)
            put32(u, strip + 0x64 + source_slot * 0x34, factory if linked else 0)
            put32(u, factory + 0x58, product)
            put32(u, factory + 0x24, stage)
            u.mem_write(STACK, bytes(0x500))
            u.reg_write(UC_X86_REG_ESI, strip)
            m.invoke(0x6A95A3, ends=0x6A95EE)
            busy = u.mem_read(STACK + 0x17, 1)[0]
            entries = []
            for slot, name in enumerate(names):
                u.mem_write(STACK, bytes(0x500))
                u.mem_write(STACK + 0x490, b'\x01')  # Draw requested, not a skipped repaint.
                u.mem_write(STACK + 0x17, bytes([busy]))
                put32(u, STACK + 0x30, strip + slot * 0x34)
                put32(u, STACK + 0x3C, slot)
                u.reg_write(UC_X86_REG_EAX, strip + slot * 0x34)
                u.reg_write(UC_X86_REG_EDI, strip)
                m.invoke(0x6A9761, obj=m.catalog[name]['kind'], ends=0x6A99C3)
                entries.append(dict(type=name, unavailable=bool(u.reg_read(UC_X86_REG_EBX) & 255)))
            controls.append(dict(tab=tab, source=source, state=label,
                request_state=state, linked=linked, stage=stage,
                strip_busy=bool(busy), entries=entries))
    u.hook_del(hook)
    # Select's unlinked-right-click arm: execute the request-word branch and
    # actual EventClass ctor/OutList append. Sound, redraw and wall time are
    # presentation/platform sinks; no request state or event is supplied.
    pending_clicks, event_arguments = [], []
    def pending_boundary(_u, pc, size, _data):
        if pc == 0x4F42F0:
            m.native_return(0, 4)
        elif pc == 0x750920:
            m.native_return(0, 8)
        elif pc == 0x4C6970:
            sp = u.reg_read(UC_X86_REG_ESP)
            event_arguments.append(list(read(u, sp + 4, 5)))
        elif bytes(u.mem_read(pc, size)) == b'\xFF\x15\x30\x15\x7E\x00':
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_EIP, pc + size)
    pending_hook = u.hook_add(UC_HOOK_CODE, pending_boundary)
    select = m.alloc(0x40)
    for tab, source in ((0, 'GAPOWR'), (1, 'GAPILL'), (2, 'E1'), (3, 'MTNK')):
        strip = STRIPS[tab]
        slot = m.order()[tab].index(source)
        request = strip + 0x68 + slot * 0x34
        put32(u, select + 0x2C, strip)
        for state in (0, 1, 2):
            put32(u, request, state)
            u.mem_write(STACK, bytes(0x500))
            put32(u, STACK + 0x20, select)
            put32(u, STACK + 0x14, m.catalog[source]['type_index'])
            put32(u, 0xA802C8, 0)
            put32(u, 0xA802CC, 0)
            put32(u, 0xA802D0, 0)
            event_arguments.clear()
            u.reg_write(UC_X86_REG_ESI, slot)
            u.reg_write(UC_X86_REG_EDI, m.pointers[source])
            u.reg_write(UC_X86_REG_EBP, m.catalog[source]['kind'])
            m.invoke(0x6AB0AC, ends=0x6AB18D)
            count = read(u, 0xA802C8)[0]
            pending_clicks.append(dict(tab=tab, source=source, before=state,
                after=read(u, request)[0], event_arguments=copy.deepcopy(event_arguments),
                events=[dict(kind=u.mem_read(0xA802D4 + i * 111, 1)[0])
                        for i in range(count)]))
    # Retain House's original category request flag across successive clicks.
    # This executes the entire unlinked left arm, including its original
    # getter/setter gates, rather than supplying request words between clicks.
    pending_histories = []
    histories = [(tab, source, [(tab, source, press)
                  for press in ('left', 'right', 'left', 'right')])
                 for tab, source in ((0, 'GAPOWR'), (1, 'GAPILL'), (2, 'E1'), (3, 'MTNK'))]
    histories.append((3, 'MTNK', [(2, 'E1', 'left'), (3, 'MTNK', 'left'),
                                 (3, 'MTNK', 'right')]))
    for tab, source, presses in histories:
        for other_strip, names in zip(STRIPS, m.order()):
            for slot in range(len(names)):
                put32(u, other_strip + 0x64 + slot * 0x34, 0)
                put32(u, other_strip + 0x68 + slot * 0x34, 0)
        u.mem_write(m.house + 0x53D0, bytes(9))
        for address in (0xA802C8, 0xA802CC, 0xA802D0):
            put32(u, address, 0)
        event_arguments.clear()
        steps = []
        for click_tab, name, press in presses:
            strip = STRIPS[click_tab]
            slot = m.order()[click_tab].index(name)
            entry = m.catalog[name]
            put32(u, select + 0x2C, strip)
            u.mem_write(STACK, bytes(0x500))
            put32(u, STACK + 0x14, entry['type_index'])
            put32(u, STACK + 0x20, select)
            put32(u, STACK + 0x24, entry['build_cat'])
            put32(u, STACK + 0x28, m.pointers[source])
            put32(u, STACK + 0x2C, slot)
            u.reg_write(UC_X86_REG_EBX, slot * 0x34)
            u.reg_write(UC_X86_REG_ESI, slot)
            u.reg_write(UC_X86_REG_EDI, m.pointers[source])
            u.reg_write(UC_X86_REG_EBP, entry['kind'])
            before = read(u, 0xA802C8)[0]
            m.invoke(0x6AB602 if press == 'left' else 0x6AB0AC,
                     ends=(0x6AB825, 0x6AB95A) if press == 'left' else 0x6AB18D)
            count = read(u, 0xA802C8)[0]
            requests = [dict(source=other_name, request_state=state)
                        for other_strip, names in zip(STRIPS, m.order())
                        for other_slot, other_name in enumerate(names)
                        if (state := read(u, other_strip + 0x68 + other_slot * 0x34)[0])]
            steps.append(dict(source=name, tab=click_tab, press=press, requests=requests,
                              events=[u.mem_read(0xA802D4 + i * 111, 1)[0]
                                      for i in range(before, count)]))
        pending_histories.append(dict(tab=tab, source=source, steps=steps))
    u.hook_del(pending_hook)
    return dict(schema_version=1, catalog=catalog, controls=controls,
                pending_right_clicks=pending_clicks,
                pending_click_histories=pending_histories)


def draw_availability_metadata():
    return provenance(
        scope='StripDraw whole-strip request scan and per-entry unavailable decisions, plus Select unlinked click request transitions and original PRODUCE/SUSPEND event construction, for four strips; no raster or production timing.',
        assumptions=[
            'Reuse Sidebar VM, original typed RTTI tables, Compare/Insert/Add and original Factory IsComplete/GetStage getters. Supplied catalog and Factory+24/+58 fields are declared component inputs.',
            'Request words at strip+68+slot*34 are explicit inputs. Select6AB80D/6AB587 write1 after PRODUCE; pending right-click6AB0C9 writes2; StripAbandon6ABBE7 clears0. The live Factory link alone is not the request word.',
            'Original6A95A3..6A95EE scans every retained entry, including offscreen entries, for state1 or2. Original6A9761..6A99C3 makes the BuildingType-only gate and linked/pending overrides; none of these boolean outputs is calculated in Python.',
            'Type BuildLimit=2147483647 and empty House counters isolate busy-strip availability; original CheckBuildLimit executes. No custom rules or whole-map closure claim.'],
        substitutions=[
            'Existing Sidebar FindFactory/CanBuild query priors return available/Yes for all types. Image virtual+98 returnsNULL; shape pointers/pixels are outside the selected range.',
            'Select6AB0AC starts at the no-Factory right-click arm with retained slot/type locals supplied, stopping before the separate queued-copy arm6AB18D. Pending click histories also execute left6AB602 through request/redraw6AB825 or its rejected-request exit6AB95A, retaining the original House request flags between clicks; the cost/clock initialization tail is excluded. Original EventClass4C6970 and OutList append execute. Redraw4F42F0, GUI sound750920, existing EVA sink and WinMM timeGetTime are presentation/platform boundaries.'],
        entry_points=dict(strip_draw=0x6A9540, busy_scan=0x6A95A3,
                          busy_scan_end=0x6A95EE, entry_draw=0x6A9761,
                          decision=0x6A99C3, pending_right_click=0x6AB0AC,
                          pending_right_click_end=0x6AB18D, pending_left_click=0x6AB602,
                          pending_left_request_end=0x6AB825, event_ctor=0x4C6970))


def trigger_cameo_generate():
    """114/115 callers and original115 deadline/drawing-predicate controls.

    Reuse the existing Sidebar machine/retained entry owner; action fields
    are explicit reader-output priors. No second VM, mapper or flash formula.
    """
    catalog = [item('HTNK'), item('E2', 16), item('GAPOWR', 7),
               item('GAPILL', 7, build_cat=5), item('ORCA', 3), item('UNLISTED')]
    m = Sidebar(catalog)
    u = m.u
    for kind, address in ARRAYS.items():
        count = sum((31 if row['kind'] in SUPER_KINDS else row['kind']) == kind
                    for row in m.catalog.values())
        put32(u, address + 12, count)
    for name, pointer in m.pointers.items():
        u.mem_write(pointer + 0x24, name.encode('ascii') + b'\0')
        if name != 'UNLISTED':
            m.add(name)
    techno = m.alloc(len(catalog) * 4)
    u.mem_write(techno, dwords(*(m.pointers[row['id']] for row in catalog)))
    put32(u, 0xA8EB04, techno)
    put32(u, 0xA8EB10, len(catalog))
    action = m.alloc(0x94)
    events, draws = [], []
    def observe(_u, pc, _size, _data):
        if pc in (0x65C780, 0x65C7E0):
            draws.append(f'{pc:08X}')
        if pc == 0x6A7590:
            events.append(read(u, u.reg_read(UC_X86_REG_ESP) + 4)[0])
            # Original114 admission executes; actual tab gadget registration,
            # input event wiring and rendering are outside this boundary.
            m.native_return(0, 4)
    hook = u.hook_add(UC_HOOK_CODE, observe)
    spans = ((0x6E4100, 0x6E41FE), (0x6AC5F0, 0x6AC733),
             (0x6ABAC0, 0x6ABAD0), (0x6A9B4B, 0x6A9B78))
    code = [bytes(u.mem_read(a, b-a)) for a, b in spans]
    for stream in (m.scenario + 0x218, 0x886B88, 0xABE890):
        m.invoke(0x65C6D0, stream, (31,))
    def rng():
        return [hashlib.sha256(u.mem_read(p, 0x3F4)).hexdigest()
                for p in (m.scenario + 0x218, 0x886B88, 0xABE890)]
    def deadlines():
        return {name: read(u, strip + 0x88 + index * 0x34)[0]
                for strip, names in zip(STRIPS, m.order()) for index, name in enumerate(names)}
    def reset_deadlines():
        for strip in STRIPS:
            for index in range(75):
                put32(u, strip + 0x88 + index * 0x34, 0)
        put32(u, SIDEBAR + 0x53A0, 0)
    tab_controls = []
    for value, empty in ((-1, False), (0, False), (1, False), (2, False),
                         (3, False), (4, False), (3, True)):
        old_count = read(u, STRIPS[3] + 0x54)[0]
        if empty:
            put32(u, STRIPS[3] + 0x54, 0)
        u.mem_write(action, bytes(0x94))
        put32(u, action + 0x90, value)
        events.clear()
        m.invoke(0x6E4100, action, (0, 0, 0, 0))
        tab_controls.append(dict(index=value, counts=[read(u, s + 0x54)[0] for s in STRIPS],
            result=u.reg_read(UC_X86_REG_EAX) & 255, selected=list(events)))
        put32(u, STRIPS[3] + 0x54, old_count)
    specs = [
        ('unit_index_collision', 'HTNK', 123, 60), ('infantry', 'E2', 0, 20),
        ('building', 'GAPOWR', 123, 20), ('defense', 'GAPILL', 123, 20),
        ('aircraft', 'ORCA', 123, 20), ('type_without_entry', 'UNLISTED', 123, 20),
        ('wrong_case', 'htnk', 123, 20), ('unknown', 'MISSING', 123, 20),
        ('zero', 'E2', 0, 0), ('negative', 'E2', 123, -5),
        ('signed_wrap', 'E2', 2147483647, 1), ('negative_frame', 'E2', -10, 20),
    ]
    controls = []
    for name, type_name, frame, duration in specs:
        reset_deadlines()
        put32(u, 0xA8ED84, frame)
        u.mem_write(action, bytes(0x94))
        u.mem_write(action + 0x54, type_name.encode('ascii') + b'\0')
        put32(u, action + 0x90, duration)
        before = rng()
        m.invoke(0x6E4150, action, (0, 0, 0, 0))
        result = u.reg_read(UC_X86_REG_EAX) & 255
        controls.append(dict(name=name, type_name=type_name, frame=frame, duration=duration,
            result=result, deadlines=deadlines(), redraw_until=read(u, SIDEBAR + 0x53A0)[0],
            rng_before=before, rng_after=rng()))
    reset_deadlines()
    overwrite = []
    for frame, duration in ((0, 60), (10, 2), (10, -1)):
        put32(u, 0xA8ED84, frame)
        m.invoke(0x6AC5F0, SIDEBAR, (m.pointers['E2'], duration))
        overwrite.append(dict(frame=frame, duration=duration, deadline=deadlines()['E2'],
                              redraw_until=read(u, SIDEBAR + 0x53A0)[0]))
    draw_controls = []
    # Native signed deadline comparison and signed frame%16 execute as one
    # body crop, stopping before either its overlay setup or continuation.
    for deadline in (0, 20, 183, -2147483648, 2147483647):
        for frame in (-2147483648, -17, -10, -1, 0, 8, 9, 15, 16, 19, 20,
                      24, 25, 31, 32, 175, 176, 182, 183, 2147483647):
            put32(u, 0xA8ED84, frame)
            put32(u, STRIPS[2] + 0x88, deadline)
            put32(u, STACK + 0x3C, 0)
            put32(u, STACK + 0x24, STRIPS[2])
            reached = m.invoke(0x6A9B4B, ends=(0x6A9B78, 0x6A9BC5))
            draw_controls.append([deadline, frame, reached == 0x6A9B78])
    assert code == [bytes(u.mem_read(a, b-a)) for a, b in spans]
    u.hook_del(hook)
    return dict(schema_version=1, catalog=catalog, order=m.order(),
        tab_controls=tab_controls, flash_controls=controls,
        overwrite=overwrite, draw_controls=draw_controls, rng_calls=draws,
        darken_asset=bytes(u.mem_read(0x83F92C, 32)).split(b'\0', 1)[0].decode('ascii'))


def trigger_cameo_metadata():
    return provenance(
        scope='Original Actions114/115 helper admission, exact reverse TechnoType search, Sidebar6AC5F0 numeric-index deadline writes and StripDraw signed flash predicate; no native pixels or complete gadget registration.',
        assumptions=[
            'Reuse Sidebar machine, original Strip constructors and original admitted Add/Insert ordering. Explicit initialized type/House catalog, typed arrays/counts and common TechnoType registry are supplied; no complete process/rules/House loaders or TechnoType registry construction execute.',
            'Action+90 duration/tab and Action+54 inline identifier are explicit materialized-reader priors. Original6E4100/6E4150 execute to return. Original reader6DD5B0 is covered by existing trigger_action_values/inline_type evidence and is not executed in this selected mode.',
            '114 executes original6ABAC0 count gate; valid0..3 nonempty strips invoke6A7590, invalid/empty inputs return0.115 scans original A8EB04/A8EB10 backwards with exact case, then original6AC5F0 scans typed pointer arrays and picks a strip. Unsupported/missing type returns0; a registered type without a retained entry still updates53A0.',
            'Original6AC5F0 compares only retained typed-array index+58 and not RTTI+5C. Thus the supplied Vehicle strip[ORCA(AircraftIndex0),HTNK(UnitIndex0)] highlights ORCA when asked for HTNK. This collision is a recorded original behavior, not a Python matching rule.',
            'All deadline arithmetic is original signed i32 wrapping: entry88=Frame+duration and Sidebar53A0=Frame+duration+1. Repeated requests overwrite; no max or cooldown policy is supplied. Original6A9B4B..6A9B78 branches provide draw_controls[deadline,signed-frame,draw-darken]; the threshold and signed remainder are not computed in Python.',
            'Three original RNG streams are initialized with seed31; unchanged states and observed draw entry points are recorded. The selected bodies have no gameplay timer or detach call. Deadline fields are presentation state, not simulation clocks.'],
        substitutions=[
            'Existing Sidebar fixture cost/availability/EVA/allocator boundaries remain setup only; original comparison, typed lookup, action helper, deadline and flash-predicate instructions are unchanged.',
            '6A7590 is a recorded RET4 selection sink. Its actual gadget remove/add, active-tab storage and layout/draw tail are excluded. StripDraw execution stops before either darken overlay setup6A9B78 or ordinary continuation6A9BC5; no pixel, palette, SHP decode or GPU claim.'],
        entry_points=dict(action114=0x6E4100, nonempty=0x6ABAC0,
            tab_switch_boundary=0x6A7590, action115=0x6E4150,
            flash_cameo=0x6AC5F0, draw_gate=0x6A9B4B,
            overlay_selected=0x6A9B78, ordinary_draw=0x6A9BC5))


if __name__ == '__main__':
    import argparse
    selector = argparse.ArgumentParser(add_help=False)
    selector.add_argument('--trigger-cameos', action='store_true')
    selector.add_argument('--draw-availability', action='store_true')
    selected, remaining = selector.parse_known_args()
    if selected.draw_availability:
        finish_vectors(draw_availability_generate,
            Path(__file__).with_name('draw_availability.json'),
            provenance=draw_availability_metadata, argv=remaining)
        raise SystemExit(0)
    if selected.trigger_cameos:
        finish_vectors(trigger_cameo_generate,
            Path(__file__).with_name('trigger_cameos.json'),
            provenance=trigger_cameo_metadata, argv=remaining)
        raise SystemExit(0)
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                   source_paths={
                       'producer': Path(__file__),
                       'sidebar_machine': Path(__file__).with_name('geometry.py'),
                       'cost_owner': Path(__file__).parents[1] / 'spatial_oracle/cost_of.py',
                       'ini_fixture': Path(__file__).parents[1] / 'spatial_oracle/building_body_rules.py',
                       'runner': Path(__file__).parents[1] / 'native_oracle.py',
                       'joined_building_owner': Path(__file__).parents[1] / 'spatial_oracle/building_construction.py',
                       'joined_scene_owner': Path(__file__).parents[1] / 'spatial_oracle/building_death_anims.py',
                       'physical_audio_and_admission_owner': Path(__file__).parents[1] / 'spatial_oracle/engineer_repair_admission.py',
                       'joined_navigation_owner': Path(__file__).parents[1] / 'spatial_oracle/anytown_damage/navigation.py',
                       'joined_transport_owner': Path(__file__).parents[1] / 'spatial_oracle/anytown_damage/mission.py',
                       'map_prior_owner': Path(__file__).parents[1] / 'spatial_oracle/refinery_dock.py',
                   })
