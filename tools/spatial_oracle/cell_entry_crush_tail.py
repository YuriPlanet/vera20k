"""Original Unit Can_Enter_Cell post-walk crush-latch continuation.

The prior object walk, layer choice and latch producer are supplied inputs.
Original GetUnit, IsCrushableBy, alliance, timer and virtual getter bytes run;
the observer only records calls. No target is damaged by this continuation.
"""
from itertools import product
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

TAIL, GET_UNIT, CRUSHABLE = 0x73FC24, 0x47EBA0, 0x5F6CD0
UNIT_VTABLE, INFANTRY_VTABLE = 0x7F5C70, 0x7EB058
MOVER, MOVER_TYPE, CELL = SCRATCH, SCRATCH + 0x1000, SCRATCH + 0x3000
HOUSE, ENEMY, ALLY = SCRATCH + 0x10000, SCRATCH + 0x16000, SCRATCH + 0x1C000
SP = STACK_BASE + 0x1000


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x30000)
    u.mem_map(RET_MAGIC, 0x1000)
    u.mem_write(0xA8E9A0, b'\x01')  # Active game: GetUnit walks its list.
    u.mem_write(0xA8ED84, dwords(100))
    for house, index in ((HOUSE, 0), (ENEMY, 1), (ALLY, 2)):
        u.mem_write(house + 0x30, dwords(index))
    u.mem_write(HOUSE + 0x5788, dwords(1 << 2))
    u.mem_write(MOVER, dwords(UNIT_VTABLE))
    u.mem_write(MOVER + 0x14, dwords(1))
    u.mem_write(MOVER + 0x21C, dwords(HOUSE))
    u.mem_write(MOVER + 0x6C4, dwords(MOVER_TYPE))
    u.mem_write(MOVER_TYPE + 0xD28, bytes((1, int(row['omni']))))
    addresses = {MOVER: 'mover'}
    next_slot = 0
    for layer, offset in (('ground', 0xE4), ('deck', 0xE8)):
        nodes = []
        for node in row[layer]:
            if node.get('self', False):
                address = MOVER
            else:
                address = SCRATCH + 0x4000 + next_slot * 0x2000
                target_type = address + 0x800
                next_slot += 1
                addresses[address] = node['name']
                infantry = node['category'] == 'infantry'
                u.mem_write(address, dwords(INFANTRY_VTABLE if infantry else UNIT_VTABLE))
                u.mem_write(address + 0x14, dwords(1))
                u.mem_write(address + (0x6C0 if infantry else 0x6C4), dwords(target_type))
                owner = ALLY if node.get('allied', False) else ENEMY
                u.mem_write(address + 0x21C, dwords(owner))
                if node.get('reverse_allied', False):
                    u.mem_write(owner + 0x5788, dwords(1))
                u.mem_write(target_type + 0x22D, bytes((int(node.get('crushable', False)),)))
                u.mem_write(target_type + 0xD2A, bytes((int(node.get('resistant', False)),)))
                # No deploy-immunity byte on Unit fixtures. Both active and
                # exactly-expired timer states execute the real +160 slot.
                duration = {'none': 0, 'active': 20, 'expired': 10}[node.get('timer', 'none')]
                u.mem_write(address + 0x18C, dwords(90))
                u.mem_write(address + 0x194, dwords(duration))
            nodes.append(address)
        u.mem_write(CELL + offset, dwords(nodes[0] if nodes else 0))
        for index, address in enumerate(nodes):
            u.mem_write(address + 0x30, dwords(nodes[index + 1] if index + 1 < len(nodes) else 0))
    u.mem_write(CELL + 0x124, dwords(row['raw_ground'], row['raw_deck']))
    raw = row['raw_ground'] if row['bits_layer'] == 'ground' else row['raw_deck']
    u.mem_write(SP + 0x14, bytes((raw, int(bool(raw & 0x20)))))
    u.mem_write(SP + 0x17, b'\x01')  # Supplied successful object-walk latch.
    u.mem_write(SP + 0x90, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EBP, row['running_code'])
    u.reg_write(UC_X86_REG_EBX, MOVER)
    u.reg_write(UC_X86_REG_EDI, CELL)
    calls = []

    def observe(uc, address, _size, _data):
        if address == GET_UNIT:
            stack = uc.reg_read(UC_X86_REG_ESP)
            calls.append(dict(function='get_unit', deck=struct.unpack('<I', uc.mem_read(stack + 4, 4))[0]))
        elif address == CRUSHABLE:
            calls.append(dict(function='is_crushable_by', target=addresses[uc.reg_read(UC_X86_REG_ECX)]))

    before = bytes(u.mem_read(SCRATCH, 0x30000))
    u.hook_add(UC_HOOK_CODE, observe)
    run_checked(u, TAIL, RET_MAGIC, count=1000, required_addresses=(TAIL,))
    assert u.reg_read(UC_X86_REG_ESP) == SP + 0xA8
    assert bytes(u.mem_read(SCRATCH, 0x30000)) == before
    return dict(input=row, result=u.reg_read(UC_X86_REG_EAX), calls=calls,
                object_and_cell_memory_unchanged=True)


def unit(name='unit', **fields):
    return dict(name=name, category='unit', **fields)


def generate():
    rows = []
    def add(name, **fields):
        row = dict(name=name, running_code=0, omni=False, raw_ground=0x20,
                   raw_deck=0, bits_layer='ground', ground=[], deck=[])
        row.update(fields)
        rows.append(execute(row))

    add('latch_no_raw', raw_ground=0)
    add('latch_infantry_bits_only', raw_ground=0x1D)
    add('latch_vehicle_no_unit')
    for code in range(1, 8):
        add(f'preserve_accumulator_{code}', running_code=code)
    for omni, crushable, resistant, allied, timer in product(
            (False, True), (False, True), (False, True), (False, True),
            ('none', 'active', 'expired')):
        name = f'predicate_{int(omni)}{int(crushable)}{int(resistant)}{int(allied)}_{timer}'
        add(name, omni=omni, ground=[unit(crushable=crushable, resistant=resistant,
                                        allied=allied, timer=timer)])
    add('first_self', omni=True, ground=[unit('mover', self=True), unit('enemy')])
    add('first_uncrushable', ground=[unit('first'), unit('second', crushable=True)])
    add('first_crushable', ground=[unit('first', crushable=True), unit('second')])
    add('skip_infantry', ground=[dict(name='infantry', category='infantry'), unit(crushable=True)])
    add('deck_unit_is_not_ground_unit', deck=[unit(crushable=True)])
    add('deck_raw_still_queries_ground', bits_layer='deck', raw_ground=0,
        raw_deck=0x20, ground=[unit(crushable=True)], deck=[unit('deck')])
    add('deck_raw_no_ground_unit', bits_layer='deck', raw_ground=0,
        raw_deck=0x20, deck=[unit(crushable=True)])
    add('unselected_ground_raw_ignored', bits_layer='deck', raw_ground=0x20, raw_deck=0)
    add('unselected_deck_raw_ignored', bits_layer='ground', raw_ground=0, raw_deck=0x20)
    add('only_target_considers_mover_allied', ground=[unit(crushable=True, reverse_allied=True)])
    return dict(cases=rows)


def metadata():
    return provenance(
        scope='Original Unit Can_Enter_Cell supplied post-walk crush-latch continuation, including first GROUND Unit predicate and unchanged object/cell memory',
        assumptions=[
            'Entry73FC24 receives supplied EBP accumulator0..7 and successful crush latch1; preceding object walk, terrain/layer selection and production of those values are excluded',
            'Captured raw byte and vehicle latch are supplied from the declared selected ground/deck plane, matching73F0ED..109/73F32C..348; original tail performs no fresh raw lookup',
            'Active-game global1; real Unit7F5C70/Infantry7EB058 vtables, supplied linked lists, Techno abstract flag1, type pointers and owner houses; no constructors or full scenario load',
            'Current frame100; timer start90 with duration0/10/20; Unit deploy-immunity byte0; unsigned wrap and paused timer cases excluded',
            'Mover type Crusher1 and OmniCrusher0/1; targets vary Crushable, OmniCrushResistant, directional enemy/allied house and active/expired timer; no building/aircraft target coverage',
            'Entire fixture object/cell memory must remain unchanged. Observation records original GetUnit ground arguments and IsCrushableBy receiver; no PerCell execution or kills',
            'Primary live consumer is Unit vtable7F5E1C (slot1AC); Drive Process4B1C3E dispatches its return via4B2608. Full CanEnter/Drive/PerCell parity is not claimed',
        ],
        substitutions=['No patched instructions, custom vtables, substituted callees or control-flow-changing hooks; prior continuation inputs are explicitly supplied'],
        entry_points=dict(unit_can_enter=0x73F0A0, post_walk=TAIL, crush_tail=0x73FCF6,
                          get_unit=GET_UNIT, is_crushable_by=CRUSHABLE,
                          is_ally_by_object=0x4F9A90, invulnerability_slot=0x41BF40),
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
