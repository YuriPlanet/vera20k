"""Full native CanMakeMoney and small joined CheckBuildNeed controls.

Data-shaped valid lists/counters/types; no selected return or instruction patch.
No native object/INI construction, world admission or whole AI parity claim.
"""
from copy import deepcopy
from functools import lru_cache
from itertools import product
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP,
    UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    IMAGE_BASE, NATIVE_FPCW, RET_MAGIC, STACK_BASE, STACK_SIZE, NativeCallTrace,
    OracleError, _sections, finish_vectors, image_bytes, load_image, provenance,
    run_checked,
)
from tools.spatial_oracle.cost_of import VTABLE, f32

ARENA, ARENA_SIZE = 0x50000000, 0x60000
HOUSE, RULES, COUNTRY = ARENA, ARENA + 0x10000, ARENA + 0x15000
SCENARIO, TYPES = ARENA + 0x18000, ARENA + 0x30000
COUNTRY_ITEMS, COUNTER_ITEMS = ARENA + 0x50000, ARENA + 0x50100
PAD1, PAD2, DOCK, PADS = (ARENA + n for n in (0x51000, 0x52000, 0x53000, 0x53100))
TIBERIUM_LIST, TIBERIUMS = ARENA + 0x54000, ARENA + 0x54100
LIST_ITEMS, BUILDING_ITEMS, BUILDINGS = ARENA + 0x55000, ARENA + 0x56000, ARENA + 0x57000
FACTORY_ITEMS = ARENA + 0x59000
CAN_MAKE_MONEY, CHECK_BUILD_NEED = 0x4F6540, 0x4FD9A0
COUNTRIES = [dict(id='PARENT', name='ParentAlias', index=0),
             dict(id='CHILD', name='ChildAlias', index=1)]
LIST_OFFSETS = dict(base_units=0xB20, harvesters=0xB3C,
                    refineries=0x8E4, war_factories=0x938)
RNGS = dict(main=0x886B88, scenario=SCENARIO + 0x218, mapgen=0xABE890)
TIMER = 0xB0EA80
INT_MIN, INT_MAX = -0x80000000, 0x7FFFFFFF


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def bits(value):
    return f'{value & 0xFFFFFFFF:08X}'


def signed(value):
    return struct.unpack('<i', dwords(value))[0]


@lru_cache(maxsize=1)
def executable_sections():
    data = image_bytes()
    sections = []
    for rva, raw, raw_size, virtual_size, flags in _sections(data):
        if flags & 0x20000000:
            size = max(raw_size, virtual_size)
            sections.append((IMAGE_BASE + rva, data[raw:raw + raw_size].ljust(size, b'\0')))
    if not sections:
        raise OracleError('no executable PE sections')
    return sections


def check_code(u):
    for address, expected in executable_sections():
        if bytes(u.mem_read(address, len(expected))) != expected:
            raise OracleError(f'original executable section changed at {address:#x}')


def type_input(name, category, index, cost, *, owner=1, free_unit=None):
    return dict(id=name, category=category, array_index=index, cost=cost,
                owner_mask=bits(owner), required_mask=bits(-1), forbidden_mask=bits(-1),
                planned_side=-1, build_cat=0, free_unit=free_unit, super_weapon_index=-1)


def base_input(name, money=0, *, domain='retail_shaped'):
    return dict(name=name, domain=domain, money=money,
                counts=dict(destinations=0, gatherers=0, war_factories=0, yards=1, base_units=0),
                country_id='PARENT', country_index=0, parent_country='PARENT', country_side=0,
                country_factors=[bits(f32(1))] * 5, plant_factors=[bits(f32(1))] * 5,
                income_bits=bits(f32(1)), super_weapons=True,
                lists=dict(base_units=['MCV'], harvesters=['HARV'], refineries=['REFN'],
                           war_factories=['WEAP']),
                types=[type_input('MCV', 'unit', 0, 3000),
                       type_input('MCV_ALT', 'unit', 1, 3000, owner=2),
                       type_input('HARV', 'unit', 2, 1400),
                       type_input('HARV_ALT', 'unit', 3, 1500, owner=2),
                       type_input('REFN', 'building', 0, 2000, free_unit='HARV'),
                       type_input('REFN_ALT', 'building', 1, 2500),
                       type_input('WEAP', 'building', 2, 2000),
                       type_input('WEAP_ALT', 'building', 3, 2300),
                       type_input('YARD', 'building', 4, 3000)])


def edit_type(case, name, **fields):
    next(typ for typ in case['types'] if typ['id'] == name).update(fields)


class Fixture:
    def __init__(self, case):
        self.case = case
        self.u = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(ARENA, ARENA_SIZE)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        self.ptrs = {typ['id']: TYPES + index * 0x2000 for index, typ in enumerate(case['types'])}
        self.ids = {ptr: name for name, ptr in self.ptrs.items()}
        self.vector_names = {RULES + off: name for name, off in LIST_OFFSETS.items()}
        self.put(0x8871E0, RULES)
        self.put(0xA8B230, SCENARIO)
        self.put(0xA8B238, 1)
        self.put(0xB782A0, 0)  # Declared ASCII C-locale stricmp fast path.
        self.put(0xA83C9C, COUNTRY_ITEMS)
        self.put(0xA83CA8, len(COUNTRIES))
        for country in COUNTRIES:
            ptr = COUNTRY + country['index'] * 0x200
            self.put(COUNTRY_ITEMS + country['index'] * 4, ptr)
            self.string(ptr + 0x24, country['id'])
            self.string(ptr + 0x64, country['name'])
            self.string(ptr + 0x98, case['parent_country'] if country['id'] == case['country_id'] else country['id'])
            self.put(ptr + 0xB8, country['index'])
            self.put(ptr + 0xBC, case['country_side'])
            u.mem_write(ptr + 0x114, dwords(*(int(word, 16) for word in case['country_factors'])))
            self.put(ptr + 0x148, int(case['income_bits'], 16))
        active_country = next(c for c in COUNTRIES if c['id'] == case['country_id'])
        if active_country['index'] != case['country_index']:
            raise OracleError('country index differs from declared registry')
        self.put(HOUSE + 0x34, COUNTRY + active_country['index'] * 0x200)
        self.put(HOUSE + 0x24, 0x7EA834)
        self.put(HOUSE + 0x30C, case['money'])
        for field, offset in [('destinations', 0x15C), ('gatherers', 0x158),
                              ('war_factories', 0x160), ('yards', 0x60)]:
            self.put(HOUSE + offset, case['counts'][field])
        self.put(HOUSE + 0x564C, -1)
        self.put(HOUSE + 0x5650, -1)
        u.mem_write(HOUSE + 0x5390, dwords(*(int(word, 16) for word in case['plant_factors'])))
        # Original CounterClass vtable. Capacity covers every declared Unit
        # index, so GetItemCount reads without resizing or writing this object.
        self.put(HOUSE + 0x5564, 0x7E5C54, COUNTER_ITEMS, 8, 1, case['counts']['base_units'])
        for typ in case['types']:
            ptr = self.ptrs[typ['id']]
            self.put(ptr, VTABLE[typ['category']])
            self.string(ptr + 0x24, typ['id'])
            for field, offset in [('cost', 0x610), ('array_index', 0xDF8),
                                  ('planned_side', 0x6D0), ('build_cat', 0xE08),
                                  ('super_weapon_index', 0x16F0)]:
                self.put(ptr + offset, typ[field])
            for field, offset in [('owner_mask', 0x6CC), ('required_mask', 0xDA0),
                                  ('forbidden_mask', 0xDA4)]:
                self.put(ptr + offset, int(typ[field], 16))
            self.put(ptr + 0xEA0, self.ptrs[typ['free_unit']] if typ['free_unit'] else 0)
        for index, (name, offset) in enumerate(LIST_OFFSETS.items()):
            items = LIST_ITEMS + index * 0x100
            names = case['lists'][name]
            self.put(RULES + offset, 0x7EB6D4, items, max(2, len(names)), 1, len(names), 10)
            for slot, ident in enumerate(names):
                self.put(items + slot * 4, self.ptrs[ident])
        # Both cost virtuals read PadAircraft[0].Dock[0] even when SeparateAircraft=yes.
        self.put(RULES + 0xB5C, PADS)
        self.put(PADS, PAD1, PAD2)
        for pad in (PAD1, PAD2):
            self.put(pad, VTABLE['aircraft'])
            self.put(pad + 0x3EC, DOCK)
        self.put(DOCK, self.ptrs['YARD'])
        u.mem_write(RULES + 0x17E8, b'\1')
        u.mem_write(0xA8B263, bytes([case['super_weapons']]))
        self.put(0xB0F4EC, TIBERIUM_LIST)
        for index in range(4):
            self.put(TIBERIUM_LIST + index * 4, TIBERIUMS + index * 0x100)
            self.put(TIBERIUMS + index * 0x100 + 0xB8, 25)
        # Counter count is attached to the first compatible MCV type, rather
        # than a substituted GetItemCount return. All normal rows use index0.
        parent_index = next(c['index'] for c in COUNTRIES
                            if case['parent_country'] in (c['id'], c['name']))
        owner_bit = 1 << parent_index
        selected_base = next(typ for ident in case['lists']['base_units']
                             for typ in case['types'] if typ['id'] == ident
                             and int(typ['owner_mask'], 16) & owner_bit)
        self.put(COUNTER_ITEMS + selected_base['array_index'] * 4, case['counts']['base_units'])
        # Coherent yard roster for the joined no-destination CheckBuildNeed controls.
        yards = case['counts']['yards']
        if yards not in (0, 1):
            raise OracleError('negative/corrupt yard-vector lengths are outside this fixture')
        self.put(HOUSE + 0x6C, BUILDING_ITEMS)
        self.put(HOUSE + 0x78, yards)
        self.put(0xA8EB44, BUILDING_ITEMS)
        self.put(0xA8EB50, yards)
        self.put(BUILDING_ITEMS, BUILDINGS)
        self.put(BUILDINGS + 0x520, self.ptrs['YARD'])
        self.put(0xA83E34, FACTORY_ITEMS)
        self.put(0xA83E40, 0)
        u.mem_write(TIMER, dwords(995, 0, 25))
        for address in RNGS.values():
            self.invoke(0x65C6D0, address, (31,))
        self.assert_virtuals()
        check_code(u)

    def put(self, address, *values):
        self.u.mem_write(address, dwords(*values))

    def read32(self, address):
        return struct.unpack('<I', self.u.mem_read(address, 4))[0]

    def string(self, address, value):
        self.u.mem_write(address, value.encode('ascii') + b'\0')

    def invoke(self, entry, obj, args=()):
        sp = STACK_BASE + STACK_SIZE - 0x1000
        self.put(sp, RET_MAGIC, *args)
        self.u.reg_write(UC_X86_REG_ESP, sp)
        self.u.reg_write(UC_X86_REG_ECX, obj)
        self.u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        run_checked(self.u, entry, RET_MAGIC, count=100000, required_addresses=(entry,))
        if self.u.reg_read(UC_X86_REG_ESP) != sp + 4 + len(args) * 4:
            raise OracleError('unexpected callee stack cleanup')
        return self.u.reg_read(UC_X86_REG_EAX)

    def assert_virtuals(self):
        for vtable, cost, actual in [(VTABLE['building'], 0x45EDD0, 0x45ED50),
                                     (VTABLE['unit'], 0x711F00, 0x711EB0)]:
            if (self.read32(vtable + 0x84), self.read32(vtable + 0xAC)) != (cost, actual):
                raise OracleError('original type cost virtuals differ')
        if self.read32(0x7EA834 + 0x18) != 0x4F6990:
            raise OracleError('original IHouse money virtual differs')


def execute(case, entry=CAN_MAKE_MONEY):
    f = Fixture(case)
    u = f.u
    before = bytes(u.mem_read(ARENA, ARENA_SIZE))
    rng = {name: bytes(u.mem_read(ptr, 1012)) for name, ptr in RNGS.items()}
    timer = bytes(u.mem_read(TIMER, 12))
    globals_before = {ptr: bytes(u.mem_read(ptr, size)) for ptr, size in
                      [(0x8871E0, 4), (0xA83C98, 24), (0xB78298, 12), (0xA8B230, 0x38),
                       (0xB0F4EC, 4), (0xA8EB44, 16), (0xA83E34, 16)]}
    trace = NativeCallTrace(u, f.read32)
    specs = {CAN_MAKE_MONEY: ('can_make_money', 0, 0),
             CHECK_BUILD_NEED: ('check_build_need', 0, 0),
             0x5117D0: ('country_lookup', 0, 0), 0x49FAE0: ('active_count', 1, 4),
             0x4F6990: ('available_money', 1, 4), 0x5051E0: ('first_buildable', 1, 4),
             0x45EDD0: ('building_cost', 1, 4), 0x711F00: ('type_cost', 1, 4),
             0x45ED50: ('building_actual_cost', 0, 0), 0x711EB0: ('raw_cost', 0, 0),
             0x50BDF0: ('country_factor', 1, 4), 0x50BEB0: ('plant_factor', 1, 4)}
    selected = dict(base_unit=None, harvester=None, refineries=[], war_factories=[])

    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        trace.returned(pc, sp)
        if pc in (0x49FAF0, 0x7C8D73, 0x7C8E17, 0x65C780, 0x65C7E0):
            raise OracleError(f'excluded counter-growth/locale/allocator/RNG boundary {pc:#x}')
        if pc == 0x4F65BF:
            selected['base_unit'] = f.ids[u.reg_read(UC_X86_REG_ECX)]
        if pc == 0x4F666C:
            selected['harvester'] = f.ids.get(u.reg_read(UC_X86_REG_ECX))
        if pc in specs:
            index = trace.entered(pc, sp, specs[pc])
            row = trace.calls[index]
            row['object'] = f.ids.get(u.reg_read(UC_X86_REG_ECX))
            if row['name'] == 'first_buildable':
                row['list'] = f.vector_names[row['args'][0]]
            if row['name'] in ('building_cost', 'type_cost') and row['args'] != [HOUSE]:
                raise OracleError('cost query used a different House')
            if row['name'] == 'available_money' and row['args'] != [HOUSE + 0x24]:
                raise OracleError('money query used a different IHouse')

    hook = u.hook_add(UC_HOOK_CODE, observe)
    try:
        raw = f.invoke(entry, HOUSE)
        trace.returned(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
    finally:
        u.hook_del(hook)
    if trace.pending:
        raise OracleError('incomplete native call/return observation')
    check_code(u)
    if bytes(u.mem_read(ARENA, ARENA_SIZE)) != before:
        raise OracleError('native predicate changed supplied fields/counter/list/RNG arena')
    if rng != {name: bytes(u.mem_read(ptr, 1012)) for name, ptr in RNGS.items()}:
        raise OracleError('native predicate changed a complete RNG buffer')
    if timer != bytes(u.mem_read(TIMER, 12)) or any(
            bytes(u.mem_read(ptr, len(old))) != old for ptr, old in globals_before.items()):
        raise OracleError('native predicate changed timer/registry/locale globals')
    if u.reg_read(UC_X86_REG_FPCW) != NATIVE_FPCW:
        raise OracleError('native predicate changed x87 control')
    money, prices = None, dict(harvester=None, war_factory=None, refinery=None)
    can, need = None, None
    for row in trace.calls:
        row['return_i32'] = signed(row['return_eax'])
        if row['name'] == 'first_buildable':
            row['selected'] = f.ids.get(row['return_eax'])
            selected[row['list']].append(row['selected'])
        if row['name'] == 'available_money':
            money = row['return_i32']
        if row['name'] == 'can_make_money':
            can = bool(row['return_eax'] & 0xFF)
        if row['name'] == 'check_build_need':
            need = bool(row['return_eax'] & 0xFF)
        role = {'0x4f66f1': 'harvester', '0x4f6728': 'war_factory', '0x4f679f': 'refinery'}.get(row['caller'])
        if role and row['name'] in ('building_cost', 'type_cost'):
            prices[role] = row['return_i32']
    return dict(input=case, output=dict(al=raw & 0xFF, can_make_money=can,
                                      check_build_need=need, available_money=money,
                                      selected=selected, prices=prices),
                calls=trace.calls,
                checks=dict(original_executable_sections=True, supplied_arena_unchanged=True,
                            complete_rng_buffers_unchanged=True, timer_globals_unchanged=True,
                            stack_restored=True, fpcw='0E7F'))


def controls():
    for base, destination, factory, money in product(('none', 'yard', 'mcv'), (0, 1),
                                                    (0, 1), (1999, 2000, 2001)):
        case = base_input(f'stock threshold {base} d{destination} w{factory} money{money}', money)
        case['counts'].update(yards=int(base == 'yard'), base_units=int(base == 'mcv'),
                              destinations=destination, war_factories=factory)
        yield case
    for money in (1399, 1400, 1401):
        case = base_input(f'harvester threshold {money}', money)
        case['counts'].update(yards=0, destinations=1, war_factories=1)
        yield case
    for money in (0, 2000, INT_MAX):
        case = base_input(f'healthy early success {money}', money)
        case['counts'].update(destinations=1, gatherers=1)
        yield case
    for gatherer, base, money in product((0, 1), (0, 1), (1999, 2000, 2001)):
        case = base_input(f'Yuri-shaped no HarvesterUnit g{gatherer} b{base} money{money}', money)
        case['lists']['harvesters'] = []
        case['counts'].update(gatherers=gatherer, yards=base)
        edit_type(case, 'REFN', free_unit=None)
        yield case
    for kind, monies in [('refinery', (1649, 1650, 1651)), ('harvester', (1049, 1050, 1051))]:
        for money in monies:
            case = base_input(f'Industrial Plant {kind} {money}', money)
            case['plant_factors'][1] = bits(f32(.75))
            if kind == 'harvester':
                case['counts'].update(yards=0, destinations=1, war_factories=1)
            yield case
    for name, money, fields in [
        ('gatherer', 0, dict(gatherers=1, yards=0)),
        ('factory below', 1399, dict(war_factories=1, yards=0)),
        ('factory equal', 1400, dict(war_factories=1, yards=0)),
        ('base below bundle', 3399, {}), ('base equal bundle', 3400, {}),
        ('no base even rich', INT_MAX, dict(yards=0)),
    ]:
        case = base_input(f'custom missing refinery {name}', money, domain='custom_arithmetic')
        case['lists']['refineries'] = []
        case['counts'].update(fields)
        yield case
    for name, factory, harvester in [('wrap to negative', INT_MAX, 1),
                                      ('wrap to positive', INT_MIN, -1)]:
        case = base_input(name, 0, domain='custom_arithmetic')
        case['counts']['destinations'] = 1
        edit_type(case, 'WEAP', cost=factory)
        edit_type(case, 'HARV', cost=harvester)
        edit_type(case, 'REFN', free_unit=None)
        yield case
    case = base_input('custom negative active MCV count', 2001, domain='custom_arithmetic')
    case['counts'].update(yards=0, base_units=-1)
    yield case
    case = base_input('custom negative destination count', 2001, domain='custom_arithmetic')
    case['counts'].update(destinations=-1, gatherers=1)
    yield case
    case = base_input('custom country float factors', 1700, domain='custom_arithmetic')
    case['country_factors'][1] = bits(f32(.85))
    case['country_factors'][3] = '3F599999'
    yield case
    for name in ('actual required', 'parent required rejects', 'owner actual rejects',
                 'actual forbidden rejects', 'parent forbidden passes', 'side order',
                 'owner order', 'war factory order', 'alias parent', 'base and harvester order',
                 'superweapon shell disabled'):
        case = base_input(name, 2001, domain='identity')
        case.update(country_id='CHILD', country_index=1)
        if name == 'actual required':
            edit_type(case, 'REFN', required_mask=bits(2))
        elif name == 'parent required rejects':
            edit_type(case, 'REFN', required_mask=bits(1))
        elif name == 'owner actual rejects':
            edit_type(case, 'REFN', owner_mask=bits(2))
        elif name == 'actual forbidden rejects':
            edit_type(case, 'REFN', forbidden_mask=bits(2))
        elif name == 'parent forbidden passes':
            edit_type(case, 'REFN', forbidden_mask=bits(1))
        elif name == 'side order':
            case['lists']['refineries'] = ['REFN_ALT', 'REFN']
            edit_type(case, 'REFN_ALT', planned_side=1)
        elif name == 'owner order':
            case['lists']['refineries'] = ['REFN_ALT', 'REFN']
            edit_type(case, 'REFN_ALT', owner_mask=bits(2))
        elif name == 'war factory order':
            case['lists']['war_factories'] = ['WEAP_ALT', 'WEAP']
            edit_type(case, 'WEAP_ALT', required_mask=bits(1))
        elif name == 'alias parent':
            case['parent_country'] = 'ParentAlias'
        elif name == 'base and harvester order':
            case['lists']['base_units'] = ['MCV_ALT', 'MCV']
            case['lists']['harvesters'] = ['HARV_ALT', 'HARV']
            case['counts'].update(yards=0, base_units=1)
        elif name == 'superweapon shell disabled':
            case['super_weapons'] = False
        yield case


def need_controls():
    for money in (1999, 2000, 2001):
        case = base_input(f'joined CheckBuildNeed refinery threshold {money}', money)
        # Keep both physically readable entries valid for the unchecked second
        # refinery identity read in other CheckBuildNeed branches.
        case['lists']['refineries'] = ['REFN', 'REFN_ALT']
        yield case


def generate():
    return dict(schema_version=1, countries=COUNTRIES,
                rows=[execute(case) for case in controls()],
                check_build_need_rows=[execute(case, CHECK_BUILD_NEED) for case in need_controls()])


def metadata():
    record = provenance(
        scope='Full original CanMakeMoney4F6540 and three joined whole CheckBuildNeed4FD9A0 controls with real helper/cost bodies.',
        assumptions=[
            'Data-shaped House, Country registry, Rules vectors, Unit/Building types, '
            'PadAircraft/Dock shell and preallocated active-Unit CounterClass are supplied. '
            'No relevant return or original instruction is replaced. Real vtable cost '
            'targets, country/plant factors, raw/actual/free-unit costs and money query execute.',
            'COUNTRIES order is PARENT0/CHILD1 with Name aliases. Raw masks/factors, signed '
            'costs/counts/money, lists and type array_index are explicit priors. retail_shaped '
            'means ordinary whole-count and2000/1400/FreeUnit/plant.75 shapes; actual retail '
            'country/type/config discovery is not established by these synthetic identities. '
            'Custom wrap, negative counter and missing-refinery controls are labeled separately.',
            'House storage is four zero float slots, IncomeMult finite1; AvailableMoney '
            'therefore uses its cash arm through original6C9600/4F6990. Invalid BaseUnit, '
            'missing war-factory/null-cost branches, native saves and nonempty stores are excluded.',
            'Yard-vector lengths are0/1. Counter capacity8 covers selected BaseUnit index; '
            'growth49FAF0, locale slow path7C8D73, allocator7C8E17 and RNG draw entries '
            'are guarded and fail instead of supplying results. ASCII C-locale data and '
            'FPCW0E7F are declared platform priors.',
            'The original65C6D0 seeds complete1012-byte Main/Scenario/MapGen buffers before '
            'measurement. All executable sections match original mapping before/after; '
            'the whole supplied arena, complete RNG buffers, timer sentinel, registry and '
            'locale globals remain unchanged. Native stack cleanup and control word are checked.',
            'Three CheckBuildNeed controls use human flags0, mode1, choices-1, empty '
            'Factory registry, valid two-entry refinery list and no destinations. They '
            'join strict CanMakeMoney threshold with recovery admission, not factory '
            'balance-owed or whole strategic sale/rebuild behavior.',
            'Full call traces retain EAX receipts, which are not semantic values for '
            'x87 factor-returning functions. Output AL is the boolean contract. No native '
            'constructors/readers, world admission, timers/scheduler, broad AI parity or '
            'actual retail identity/type-price reader join is claimed.',
        ], substitutions=[],
        entry_points=dict(can_make_money=CAN_MAKE_MONEY, check_build_need=CHECK_BUILD_NEED,
                          country_lookup=0x5117D0, active_count=0x49FAE0,
                          first_buildable=0x5051E0, available_money=0x4F6990,
                          building_cost=0x45EDD0, type_cost=0x711F00,
                          building_actual_cost=0x45ED50, raw_cost=0x711EB0,
                          country_factor=0x50BDF0, plant_factor=0x50BEB0))
    record['row_counts'] = dict(can_make_money=82, check_build_need=3,
                                retail_shaped=60, custom_arithmetic=11, identity=11)
    return record


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
