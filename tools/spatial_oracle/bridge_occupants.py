"""Original487A10 bridge occupant traversal with declared virtual seams.

The full controller,5657A0 cell lookup,41C230 coordinate construction and47B3A0
signed/slope ground-height calculation execute. Object admission, abstract kind,
locomotor Is_At_Coord and direct damage are supplied callbacks. Mutable lists,
health and cell coordinates expose callback order; this is not concrete admission,
locomotor or death-lifecycle parity, nor a stock-map witness for every input.
"""
from copy import deepcopy
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import (
    load_image, run_checked, STACK_BASE, STACK_SIZE, RET_MAGIC, finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords, packed

MAP, TABLE, DUMMY = 0x87F7E8, 0xC00000, 0xABDC50
MEM, OBJECTS, VTABLE, LOCO_VTABLE = 0x44000000, 0x44100000, 0x44200000, 0x44201000
ADMIT, KIND, DAMAGE, AT_COORD, GET_TYPE = (0x44210000 + i * 16 for i in range(5))
RULES, WARHEAD = 0x44220000, 0x44222000
TYPES = 0x44180000


class OriginalOccupants:
    def __init__(self, case):
        self.case = case
        self.uc = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(MEM, 0x300000)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        self.cells = {tuple(row['coord']): MEM + i * 0x200 for i, row in enumerate(case['cells'])}
        self.cell_names = {pointer: i for i, pointer in enumerate(self.cells.values())}
        self.cell_names[DUMMY] = 'dummy'
        self.objects = {row['id']: OBJECTS + i * 0x1000 for i, row in enumerate(case['objects'])}
        self.object_names = {pointer: identity for identity, pointer in self.objects.items()}
        self.object_inputs = {row['id']: deepcopy(row) for row in case['objects']}
        self.trace, self.ordinals = [], {}
        self.pending_lookup = None
        table = bytearray(0x100000)
        for row in case['cells']:
            coord = row['coord']
            p = self.cells[tuple(coord)]
            index = coord[1] * 512 + coord[0]
            assert 0 <= index < 512 * 512
            struct.pack_into('<I', table, index * 4, p)
            self.init_cell(p, row)
        u.mem_write(DUMMY, bytes(0x200))
        self.init_cell(DUMMY, case['dummy'])
        u.mem_write(TABLE, bytes(table))
        u.mem_write(MAP + 0x13C, dwords(TABLE, 512 * 512))
        u.mem_write(0x89E7C0, dwords(104))
        u.mem_write(0x8871E0, dwords(RULES))
        u.mem_write(RULES + 0xFA8, dwords(WARHEAD))
        for slot, entry in [(0x1AC, ADMIT), (0x2C, KIND), (0x16C, DAMAGE), (0x84, GET_TYPE)]:
            u.mem_write(VTABLE + slot, dwords(entry))
        u.mem_write(LOCO_VTABLE + 0xA0, dwords(AT_COORD))
        for i, row in enumerate(case['objects']):
            p = self.objects[row['id']]
            u.mem_write(p, dwords(VTABLE))
            u.mem_write(p + 0x14, dwords(row.get('flags', 4)))
            u.mem_write(p + 0x30, dwords(self.objects.get(row.get('next'), 0)))
            u.mem_write(p + 0x6C, dwords(row.get('health', 100)))
            u.mem_write(p + 0x674, dwords(p + 0x800))
            u.mem_write(p + 0x800, dwords(LOCO_VTABLE))
            u.mem_write(TYPES + i * 0x1000 + 0xD94, bytes((int(row.get('jumpjet', False)),)))
        self.selected = self.cells.get(tuple(case['start']), DUMMY)
        u.hook_add(UC_HOOK_CODE, self.observe)

    def init_cell(self, p, row):
        self.uc.mem_write(p + 0x24, packed(*row['coord']))
        self.uc.mem_write(p + 0x11B, bytes((row.get('level', 0) & 255, row.get('slope', 0))))
        self.uc.mem_write(p + 0xE4, dwords(self.objects.get(row.get('head'), 0)))

    def read32(self, p):
        return struct.unpack('<I', self.uc.mem_read(p, 4))[0]

    def coord(self, p):
        return list(struct.unpack('<hh', self.uc.mem_read(p + 0x24, 4)))

    def ret(self, cleanup, result=0):
        sp = self.uc.reg_read(UC_X86_REG_ESP)
        self.uc.reg_write(UC_X86_REG_EAX, result & 0xFFFFFFFF)
        self.uc.reg_write(UC_X86_REG_EIP, self.read32(sp))
        self.uc.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def event(self, kind, **values):
        self.trace.append(dict(kind=kind, **values))
        key = kind, values.get('object')
        ordinal = self.ordinals.get(key, 0) + 1
        self.ordinals[key] = ordinal
        for callback in self.case.get('callbacks', []):
            if (callback['event'], callback.get('object')) != key or callback['ordinal'] != ordinal:
                continue
            for change in callback['writes']:
                if 'object' in change:
                    identity = change['object']
                    p = self.objects[identity]
                    for field, offset in [('health', 0x6C), ('flags', 0x14)]:
                        if field in change:
                            self.uc.mem_write(p + offset, dwords(change[field]))
                    if 'next' in change:
                        self.uc.mem_write(p + 0x30, dwords(self.objects.get(change['next'], 0)))
                    for field in ['admission', 'at', 'kind']:
                        if field in change:
                            self.object_inputs[identity][field] = change[field]
                else:
                    p = self.selected if change['cell'] == 'selected' else DUMMY
                    if 'coord' in change:
                        self.uc.mem_write(p + 0x24, packed(*change['coord']))
                    if 'level' in change:
                        self.uc.mem_write(p + 0x11B, bytes((change['level'] & 255,)))
                    if 'head' in change:
                        self.uc.mem_write(p + 0xE4, dwords(self.objects.get(change['head'], 0)))
                self.trace.append(dict(kind='callback_write', change=change))

    def observe(self, u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x5657A0:
            self.pending_lookup = list(struct.unpack('<hh', u.mem_read(self.read32(sp + 4), 4)))
        elif address == 0x487B2D:
            self.event('lookup', requested=self.pending_lookup,
                       cell=self.cell_names[u.reg_read(UC_X86_REG_EAX)])
        elif address == 0x487AE9:
            point = list(struct.unpack('<ii', u.mem_read(sp + 0x20, 8)))
            z = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
            self.event('probe', point=[*point, z])
        elif address in (ADMIT, KIND, DAMAGE, AT_COORD, GET_TYPE):
            args = struct.unpack('<7I', u.mem_read(sp + 4, 28))
            p = args[0] - 0x800 if address == AT_COORD else u.reg_read(UC_X86_REG_ECX)
            identity = self.object_names[p]
            inputs = self.object_inputs[identity]
            if address == ADMIT:
                assert args[:5] == (self.selected, 0xFFFFFFFF, 0xFFFFFFFF, 0, 1)
                result = inputs.get('admission', 0)
                self.event('admit', object=identity, result=result)
                self.ret(20, result)
            elif address == KIND:
                result = inputs.get('kind', 6)
                self.event('abstract_kind', object=identity, result=result)
                self.ret(0, result)
            elif address == GET_TYPE:
                self.event('jumpjet_type', object=identity, result=inputs.get('jumpjet', False))
                self.ret(0, TYPES + list(self.objects).index(identity) * 0x1000)
            elif address == DAMAGE:
                assert args[1:7] == (0, WARHEAD, 0, 1, 1, 0)
                damage = struct.unpack('<i', u.mem_read(args[0], 4))[0]
                self.event('damage', object=identity, damage=damage,
                           health_alias=args[0] == p + 0x6C)
                # A packet write must not alter Object.Health via aliasing.
                u.mem_write(args[0], dwords(-999))
                self.ret(28)
            else:
                point = [struct.unpack('<i', dwords(v))[0] for v in args[1:4]]
                result = inputs.get('at', True)
                self.event('at_coord', object=identity, point=point, result=result)
                self.ret(16, int(result))

    def run(self):
        sp = STACK_BASE + STACK_SIZE - 0x1000
        self.uc.mem_write(sp, dwords(RET_MAGIC, self.case.get('mode', 0)))
        self.uc.reg_write(UC_X86_REG_ESP, sp)
        self.uc.reg_write(UC_X86_REG_ECX, self.selected)
        run_checked(self.uc, 0x487A10, RET_MAGIC, count=100000,
                    required_addresses=[0x487AE4, 0x47B3A0, 0x487B28, 0x5657A0, 0x487C0C])
        return dict(trace=self.trace, dummy_coord=self.coord(DUMMY),
                    selected_coord=self.coord(self.selected),
                    health=[[identity, self.read32(p + 0x6C)] for identity, p in self.objects.items()])


def fixture(name, objects=(), *, head=None, neighbors=(), **kwargs):
    cells = [dict(coord=[10 + x, 10 + y]) for x in range(-2, 3) for y in range(-2, 3)]
    cells[12]['head'] = head
    for coord, member in neighbors:
        next(cell for cell in cells if cell['coord'] == list(coord))['head'] = member
    return dict(name=name, start=[10, 10], cells=cells, objects=list(objects),
                dummy=dict(coord=[-1, -1]), **kwargs)


def inputs():
    yield fixture('empty_ground_and_neighbors')
    yield fixture('ground_admission_codes_and_aircraft',
                  [dict(id=i + 1, admission=i, kind=2 if i == 3 else 6,
                        next=i + 2 if i < 7 else None) for i in range(8)], head=1)
    yield fixture('neighbor_foot_at_coord_and_exact_seven', [
        dict(id=1, flags=0, next=2), dict(id=2, at=False, next=3),
        dict(id=3, admission=6, kind=2, next=4), dict(id=4, admission=7),
    ], neighbors=[([8, 9], 1)])
    yield fixture('ground_next_captured_before_admission',
                  [dict(id=1, next=2, admission=7), dict(id=2), dict(id=3)], head=1,
                  callbacks=[dict(event='admit', object=1, ordinal=1,
                                  writes=[dict(object=1, next=3, health=23)])])
    yield fixture('ground_next_captured_before_damage',
                  [dict(id=1, next=2, admission=7), dict(id=2), dict(id=3)], head=1,
                  callbacks=[dict(event='damage', object=1, ordinal=1,
                                  writes=[dict(object=1, next=3), dict(object=2, health=0)])])
    yield fixture('ground_revisits_unlinked_captured_successor',
                  [dict(id=1, next=2, admission=7), dict(id=2, next=3, admission=7),
                   dict(id=3, admission=7)], head=1,
                  callbacks=[dict(event='damage', object=1, ordinal=1,
                                  writes=[dict(object=1, next=3), dict(object=2, next=None, health=0)])])
    yield fixture('ground_health_read_after_aircraft_kind', [dict(id=1, kind=2)], head=1,
                  callbacks=[dict(event='abstract_kind', object=1, ordinal=1,
                                  writes=[dict(object=1, health=67)])])
    yield fixture('neighbor_next_reread_after_damage',
                  [dict(id=1, next=2, admission=7), dict(id=2), dict(id=3, admission=7)],
                  neighbors=[([8, 9], 1)],
                  callbacks=[dict(event='damage', object=1, ordinal=1,
                                  writes=[dict(object=1, next=3), dict(object=3, health=47)])])
    yield fixture('neighbor_next_reread_after_at_coord',
                  [dict(id=1, next=2, at=False), dict(id=2), dict(id=3, admission=7)],
                  neighbors=[([8, 9], 1)],
                  callbacks=[dict(event='at_coord', object=1, ordinal=1,
                                  writes=[dict(object=1, next=3)])])
    yield fixture('neighbor_unlink_stops_current_suffix',
                  [dict(id=1, next=2, admission=7), dict(id=2, admission=7)],
                  neighbors=[([8, 9], 1)],
                  callbacks=[dict(event='damage', object=1, ordinal=1,
                                  writes=[dict(object=1, next=None)])])
    changed = fixture('probe_reads_post_ground_coord_and_height', [dict(id=1, admission=7)], head=1,
                      callbacks=[dict(event='damage', object=1, ordinal=1,
                                      writes=[dict(cell='selected', coord=[11, 10], level=-4)])])
    changed['cells'][12]['slope'] = 9
    yield changed
    yield fixture('neighbor_coord_fresh_but_probe_retained', [dict(id=1, admission=7)],
                  neighbors=[([8, 8], 1)],
                  callbacks=[dict(event='damage', object=1, ordinal=1,
                                  writes=[dict(cell='selected', coord=[11, 11])])])
    for level, slope in [(-128, 0), (-4, 1), (5, 9), (127, 20)]:
        case = fixture(f'signed_height_{level}_slope_{slope}', [dict(id=1, admission=7)],
                       neighbors=[([8, 8], 1)])
        case['cells'][12].update(level=level, slope=slope)
        yield case
    for point in [[0, 0], [-32768, 32767]]:
        case = fixture(f'dummy_alias_{point[0]}_{point[1]}')
        case.update(start=point, cells=[], dummy=dict(coord=point, level=-4, slope=1))
        yield case
    case = fixture('neighbor_changes_selected_dummy_then_lookups_move_it',
                   [dict(id=1, admission=7)],
                   callbacks=[dict(event='damage', object=1, ordinal=1,
                                   writes=[dict(cell='selected', coord=[12, 12])])])
    case.update(cells=[dict(coord=[8, 8], head=1)], dummy=dict(coord=[10, 10]))
    yield case


def damage_inputs():
    for mode in (0, 1, 256, 257):
        yield fixture(f'jumpjet_low_byte_{mode}', [dict(id=1, flags=5, kind=15, jumpjet=True)],
                      head=1, mode=mode)
    for flags, kind, jumpjet in ((4, 15, True), (5, 1, True), (1, 6, True), (5, 1, False), (5, 2, False)):
        yield fixture(f'positive_type_gate_{flags}_{kind}_{jumpjet}',
                      [dict(id=1, flags=flags, kind=kind, jumpjet=jumpjet)], head=1, mode=1)
    yield fixture('positive_mode_does_not_change_neighbor_gate',
                  [dict(id=1, flags=5, kind=15, jumpjet=True)], neighbors=[([8, 9], 1)], mode=1)


def generate():
    return dict(cases=[dict(input=case, output=OriginalOccupants(case).run()) for case in inputs()],
                damage_cases=[dict(input=case, output=OriginalOccupants(case).run()) for case in damage_inputs()])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original487A10 two-pass bridge occupant controller, including nonzero low-byte mode',
        assumptions=[
            'Bounded supplied Cell/Object lists, initialized ground height104 and C4Warhead pointer',
            'Original5657A0 fixed cell-pointer lookup and47B3A0 signed/slope ground evaluation execute',
            'Mode0/1/256/257 distinguishes the consumed low byte; positive-mode JumpJet gate uses Techno flag and type+D94, not AbstractKind',
            'Every supplied Foot has a nonnull locomotor; native COM error on null is not executed',
            'Synthetic mutable callback states expose ordering, not stock-map reachability of every state',
        ], substitutions=[
            'Object1AC admission and2C abstract kind return supplied values',
            'Object84 returns supplied type storage; original JumpJet byte read and Techno-flag gate execute',
            'LocomotorA0 Is_At_Coord returns supplied value after recording original coordinate arguments',
            'Object16C direct damage records actual packet/flags and mutates its local packet; no death lifecycle',
            'Declared callback writes change live links, health, cell coordinates or supplied virtual results',
        ], entry_points={'repair_occupants': 0x487A10, 'cell_lookup': 0x5657A0,
                         'ground_height': 0x47B3A0}))
