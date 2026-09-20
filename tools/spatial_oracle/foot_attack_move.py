"""Original Foot attack-move retained-state receivers and callback boundaries.

Setup, clear, predicates, coordinate lookup/conversion, resume, idle resume,
acquisition and the ordinary effective-mission getter execute retail bytes.
Scanner, target predicate, QueueMission and destination/target setters are
explicit external machine-code fixture receivers. Extension cases also execute
the original token resolver, map lookup and ordinary coordinate/eligibility
receivers; explicitly named mutation cases supply those virtual callbacks.
No original instruction or original vtable is patched; hooks only observe.

    python -m tools.spatial_oracle.foot_attack_move --check
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

OWNER, VTABLE = SCRATCH + 0x1000, SCRATCH + 0x2000
DEST_A, DEST_B = SCRATCH + 0x3000, SCRATCH + 0x3100
TARGET_A, TARGET_B, TARGET_C = SCRATCH + 0x4000, SCRATCH + 0x4100, SCRATCH + 0x4200
SP = STACK_BASE + STACK_SIZE - 0x1000
EXTRA, MAP_GRID = 0x20100000, 0x20200000
TYPE, TYPE_VTABLE, EVENT, COORD_OUT = EXTRA, EXTRA + 0x2000, EXTRA + 0x3000, EXTRA + 0x4000
OBJECT_VTABLE, COORD_RETURN, TOKEN_INDEX = EXTRA + 0x5000, EXTRA + 0x6000, EXTRA + 0x7000
MAP, DUMMY = 0x87F7E8, 0xABDC50
TOKEN_RESOLVE, MAP_LOOKUP, GET_COORD = 0x6E6E20, 0x5657A0, 0x5F65A0
FAMILIES = {'unit': 0x7F5C70, 'infantry': 0x7EB058}
ENTRIES = dict(clear=0x4DF1A0, has_saved_mission=0x4DF1C0,
               is_attack_move=0x4DF310, resume=0x4DF320,
               acquire=0x4DF3A0, idle_resume=0x4DF4B0,
               setup=0x4DF0E0, saved_coordinate=0x4DF1F0,
               convert_target=0x4DF280, expiry_slice=0x4D9AC9)
GET_MISSION = 0x5B3040
CODE_RANGES = ((0x4DF0E0, 0x4DF507), (GET_MISSION, 0x5B3052),
               (0x4D9978, 0x4D998C), (0x4D9AC9, 0x4D9B43),
               (TOKEN_RESOLVE, 0x6E6F16), (MAP_LOOKUP, 0x5657D8),
               (GET_COORD, 0x5F65C1), (0x523440, 0x523484),
               (0x746CC0, 0x746CC5), (0x5228C0, 0x5228C5),
               (0x70F090, 0x70F0A2), (0x711E90, 0x711EAA),
               (0x6F3270, 0x6F3278), (0x741490, 0x741497),
               (0x51FAF0, 0x51FAF7), (0x7CAAE4, 0x7CAF20),
               (0x7CDA90, 0x7CDBA0))
FIELDS = dict(saved_mission=(0x5C4, 4), saved_destination=(0x5C8, 4),
              saved_target=(0x5CC, 4), engaged=(0x5D1, 1),
              current_target=(0x2B4, 4), mission=(0xAC, 4),
              queued_mission=(0xB4, 4), alive=(0x90, 1),
              x=(0x9C, 4), y=(0xA0, 4), z=(0xA4, 4))
EVENT_FIELDS = dict(event_mission=(0xC, 1), event_target_id=(0xE, 4),
                    event_target_tag=(0x12, 1), event_destination_id=(0x13, 4),
                    event_destination_tag=(0x17, 1))
CALLBACKS = {
    'queue_mission': (0x1E8, 8, SCRATCH + 0x6000),
    'scan': (0x39C, 8, SCRATCH + 0x6800),
    'target_predicate': (0x3B4, 4, SCRATCH + 0x7000),
    'assign_target': (0x3C8, 4, SCRATCH + 0x7800),
    'assign_destination': (0x480, 8, SCRATCH + 0x8000),
    'supplied_mission_getter': (0x184, 0, SCRATCH + 0x8800),
    'supplied_eligibility': (0x4C0, 0, SCRATCH + 0x9000),
    'supplied_coordinate': (0x48, 4, SCRATCH + 0x9800),
}
SAVED_REGISTERS = ((UC_X86_REG_EBX, 0x13579BDF), (UC_X86_REG_EBP, 0x2468ACE0),
                   (UC_X86_REG_ESI, 0x31415926), (UC_X86_REG_EDI, 0x27182818))
EAX_SEED = 0xA5B6C75E


def read_u32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def state(u):
    result = {}
    for key, (offset, size) in FIELDS.items():
        value = int.from_bytes(u.mem_read(OWNER + offset, size), 'little')
        if key in ('saved_mission', 'mission', 'queued_mission', 'x', 'y', 'z'):
            value = struct.unpack('<i', dwords(value))[0]
        result[key] = value
    return result


def initial_state(**changes):
    result = dict(saved_mission=29, saved_destination=0, saved_target=0,
                  engaged=0, current_target=TARGET_C, mission=2,
                  queued_mission=-1, alive=1, x=2688, y=-384, z=48)
    result.update(changes)
    return result


def action(value=0xFACE0085, **changes):
    return dict(return_eax=value, writes=changes)


def receiver_body(base, counter, actions):
    """External callback implementation, including declared per-call mutations.

    Its dispatcher rejects unexpected extra invocations with INT3. It does not
    inspect the native return address, branch at a native PC, or hook a result.
    """
    code = bytearray(b'\xA1' + dwords(counter) + b'\xFF\x05' + dwords(counter))
    branches = []
    for index in range(len(actions)):
        code += b'\x3D' + dwords(index) + b'\x0F\x84'
        branches.append(len(code))
        code += bytes(4)
    code += b'\xCC'
    for branch, supplied in zip(branches, actions):
        struct.pack_into('<i', code, branch, len(code) - (branch + 4))
        for key, value in supplied['writes'].items():
            owner = EVENT if key in EVENT_FIELDS else OWNER
            offset, size = EVENT_FIELDS[key] if key in EVENT_FIELDS else FIELDS[key]
            if size == 1:
                code += b'\xC6\x05' + dwords(owner + offset) + bytes((value & 0xFF,))
            else:
                code += b'\xC7\x05' + dwords(owner + offset, value)
        code += b'\xB8' + dwords(supplied['return_eax'])
        pop = supplied['pop_bytes']
        code += b'\xC2' + struct.pack('<H', pop) if pop else b'\xC3'
    if len(code) >= 0x700:
        raise ValueError(f'External receiver overflow at {base:08X}')
    return bytes(code)


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    originals = [bytes(u.mem_read(a, b-a)) for a, b in CODE_RANGES]
    original_vtables = {name: bytes(u.mem_read(address, 0x600))
                        for name, address in FAMILIES.items()}
    native_vtable = FAMILIES[row['family']]
    u.mem_write(VTABLE, original_vtables[row['family']])
    for slot, entry in ((0x4A8, 'clear'), (0x4AC, 'has_saved_mission'),
                        (0x4C4, 'is_attack_move'), (0x4C8, 'resume'),
                        (0x4CC, 'acquire'), (0x4D0, 'idle_resume'),
                        (0x4A4, 'setup'), (0x4B8, 'saved_coordinate')):
        assert read_u32(u, native_vtable + slot) == ENTRIES[entry]
    assert read_u32(u, native_vtable + 0x184) == GET_MISSION
    u.mem_write(OWNER, dwords(VTABLE))
    # Non-owned neighbors make accidental over-wide stores observable.
    u.mem_write(OWNER + 0x5C0, bytes.fromhex('aabbccdd'))
    u.mem_write(OWNER + 0x5D0, bytes.fromhex('a5005a6b7c8d9eaf'))
    for key, (offset, size) in FIELDS.items():
        value = row['state'][key]
        u.mem_write(OWNER + offset, bytes((value,)) if size == 1 else dwords(value))
    extended = row['entry'] in ('setup', 'saved_coordinate', 'convert_target', 'expiry_slice')
    if extended:
        u.mem_map(EXTRA, 0x10000)
        u.mem_map(MAP_GRID, 0x100000)
        # FS:[0] is used only by the original successful/null RTTI cast paths.
        # No Windows exception or failure recovery is emulated.
        u.mem_map(0, 0x1000)
        u.mem_write(0, dwords(0xFFFFFFFF))
        u.mem_write(OWNER + (0x6C4 if row['family'] == 'unit' else 0x6C0), dwords(TYPE))
        u.mem_write(TYPE, dwords(TYPE_VTABLE))
        u.mem_write(TYPE_VTABLE + 0xA4, dwords(0x711E90))
        u.mem_write(TYPE + 0x898, dwords(row.get('primary', 1)))
        u.mem_write(TYPE + 0x6C8, bytes((row.get('prevent_attack_move', 0),)))
        # Preserve original RTTI locator immediately preceding the copied table.
        u.mem_write(OBJECT_VTABLE - 4, bytes(u.mem_read(native_vtable - 4, 0x604)))
        for pointer in (DEST_A, DEST_B, TARGET_A, TARGET_B, TARGET_C):
            u.mem_write(pointer, dwords(OBJECT_VTABLE))
            coords = row.get('object_coords', {}).get(str(pointer),
                [2816, -129, 64] if pointer == DEST_A else [-257, 511, -48])
            u.mem_write(pointer + 0x9C, dwords(*coords))
        u.mem_write(COORD_OUT, dwords(0x11111111, 0x22222222, 0x33333333))
        u.mem_write(COORD_RETURN, dwords(*row.get('callback_coords', [777, -888, 999])))
        event = row.get('event', {})
        for key, (offset, size) in EVENT_FIELDS.items():
            value = event.get(key, 0)
            u.mem_write(EVENT + offset, bytes((value & 0xFF,)) if size == 1 else dwords(value))
        u.mem_write(MAP + 0x13C, dwords(MAP_GRID))
        u.mem_write(MAP_GRID, dwords(row.get('map_cell', DEST_B)) * 0x40000)
        u.mem_write(DUMMY + 0x24, struct.pack('<hh', 1234, -5678))
        # Two supplied, pre-sorted Abstract-ID records; the original resolver,
        # binary search and RTTI cast execute. Missing IDs take the original null.
        u.mem_write(TOKEN_INDEX, dwords(101, TARGET_B, 202, DEST_B))
        u.mem_write(0xB0E840, dwords(TOKEN_INDEX, 2))
        u.mem_write(0xB0E84C, b'\x01')
        u.mem_write(0xB0E850, dwords(0))
        u.mem_write(0xB78828, dwords(0))
    callbacks, declared = {}, {}
    for index, (name, (slot, pop_bytes, address)) in enumerate(CALLBACKS.items()):
        if name.startswith('supplied_') and name not in row['callbacks']:
            continue
        defaults = ([action(0), action(0)] if name == 'target_predicate'
                    else [action(0)] if name == 'scan' else [action()])
        supplied = row['callbacks'].get(name, defaults)
        supplied = [dict(item, pop_bytes=pop_bytes) for item in supplied]
        counter = SCRATCH + 0xF000 + index * 4
        body = receiver_body(address, counter, supplied)
        u.mem_write(address, body)
        u.mem_write((OBJECT_VTABLE if name == 'supplied_coordinate' else VTABLE) + slot,
                    dwords(address))
        callbacks[address] = (name, slot, pop_bytes, counter, supplied)
        declared[name] = dict(original_receiver=f'{read_u32(u, native_vtable + slot):08X}',
                              fixture_receiver=f'{address:08X}', calls=supplied)
    u.mem_write(SP, dwords(RET_MAGIC))
    if row['entry'] in ('setup', 'saved_coordinate'):
        u.mem_write(SP + 4, dwords(EVENT if row['entry'] == 'setup' else COORD_OUT))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, OWNER)
    u.reg_write(UC_X86_REG_EAX, EAX_SEED)
    for register, value in SAVED_REGISTERS:
        u.reg_write(register, value)
    if row['entry'] == 'expiry_slice':
        u.reg_write(UC_X86_REG_ESI, OWNER)
        u.reg_write(UC_X86_REG_EDI, row['expired_pointer'])
        u.reg_write(UC_X86_REG_EBP, 0)
    before_owner = bytes(u.mem_read(OWNER, 0x800))
    events, writes, visited, pending = [], [], [], []
    observed_native = {address: name for name, address in ENTRIES.items()}
    observed_native[GET_MISSION] = 'native_effective_mission'
    native_helpers = {
        TOKEN_RESOLVE: 'native_token_resolver', MAP_LOOKUP: 'native_map_lookup',
        GET_COORD: 'native_object_coordinate', 0x746CC0: 'native_unit_eligibility',
        0x5228C0: 'native_infantry_eligibility', 0x70F090: 'native_eligibility_type_dispatch',
        0x711E90: 'native_type_eligibility', 0x6F3270: 'native_get_type',
        0x741490: 'native_unit_type', 0x51FAF0: 'native_infantry_type',
        0x523440: 'native_token_index_search', 0x7CAAE4: 'native_abstract_rtti_cast',
    }

    def observe(uc, address, _size, _data):
        if pending and address == pending[0]['return_address']:
            event = pending.pop()
            event['after'] = state(uc)
            event['returned_eax'] = uc.reg_read(UC_X86_REG_EAX)
            if extended:
                event['event_bytes_after'] = bytes(uc.mem_read(EVENT, 0x18)).hex()
        if address in observed_native:
            if address != ENTRIES['expiry_slice']:
                assert uc.reg_read(UC_X86_REG_ECX) == OWNER
            visited.append(f'{address:08X}')
            events.append(dict(kind='original_receiver', name=observed_native[address],
                               address=f'{address:08X}', before=state(uc)))
        if extended and address in native_helpers:
            event = dict(kind='original_helper', name=native_helpers[address],
                         address=f'{address:08X}', this=uc.reg_read(UC_X86_REG_ECX),
                         before=state(uc))
            sp = uc.reg_read(UC_X86_REG_ESP)
            if address == TOKEN_RESOLVE:
                token = uc.reg_read(UC_X86_REG_ECX)
                event.update(token_id=read_u32(uc, token), token_tag=uc.mem_read(token+4, 1)[0])
            elif address == MAP_LOOKUP:
                assert uc.reg_read(UC_X86_REG_ECX) == MAP
                event['cell_argument'] = list(struct.unpack('<hh', uc.mem_read(read_u32(uc, sp+4), 4)))
            elif address == GET_COORD:
                event['output_argument'] = read_u32(uc, sp+4)
            events.append(event)
        if address not in callbacks:
            return
        assert not pending
        name, slot, pop_bytes, counter, supplied = callbacks[address]
        receiver = uc.reg_read(UC_X86_REG_ECX)
        assert receiver in (DEST_A, DEST_B, TARGET_A, TARGET_B, TARGET_C) if name == 'supplied_coordinate' else receiver == OWNER
        index = read_u32(uc, counter)
        assert index < len(supplied), (row['name'], name, index)
        sp = uc.reg_read(UC_X86_REG_ESP)
        args = [read_u32(uc, sp + 4 + i) for i in range(0, pop_bytes, 4)]
        event = dict(kind='supplied_callback', name=name, call_index=index,
                     vtable_slot=f'{slot:03X}',
                     original_receiver=declared[name]['original_receiver'],
                     args=args, return_address=read_u32(uc, sp),
                     before=state(uc), supplied=supplied[index])
        if name == 'scan':
            event['coordinate_argument'] = list(struct.unpack('<iii', uc.mem_read(args[0], 12)))
            assert args[1] == 1
        if extended:
            event['receiver'] = receiver
            event['event_bytes_before'] = bytes(uc.mem_read(EVENT, 0x18)).hex()
        events.append(event)
        pending.append(event)

    def observe_write(uc, _access, address, size, value, _data):
        if OWNER <= address < OWNER + 0x800:
            key = next((key for key, pair in FIELDS.items()
                        if pair == (address - OWNER, size)), None)
            assert key is not None, (row['name'], hex(address), size)
            pc = uc.reg_read(UC_X86_REG_EIP)
            writes.append(dict(field=key, offset=address - OWNER, size=size,
                               value=value, instruction=f'{pc:08X}',
                               origin='original' if pc < SCRATCH else 'supplied_callback'))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    entry = ENTRIES[row['entry']]
    boundary = 0x4D9B43 if row['entry'] == 'expiry_slice' else RET_MAGIC
    run_checked(u, entry, boundary, count=10000, required_addresses=(entry,))
    assert not pending
    if row['entry'] == 'expiry_slice':
        assert u.reg_read(UC_X86_REG_ESP) == SP
        assert u.reg_read(UC_X86_REG_ESI) == OWNER
        assert u.reg_read(UC_X86_REG_EDI) == row['expired_pointer']
        assert u.reg_read(UC_X86_REG_EBP) == 0
    else:
        assert u.reg_read(UC_X86_REG_ESP) == SP + (8 if row['entry'] in ('setup', 'saved_coordinate') else 4)
        assert all(u.reg_read(register) == value for register, value in SAVED_REGISTERS)
    after_owner = bytearray(u.mem_read(OWNER, 0x800))
    for offset, size in FIELDS.values():
        after_owner[offset:offset+size] = before_owner[offset:offset+size]
    assert bytes(after_owner) == before_owner
    assert all(bytes(u.mem_read(a, b-a)) == original
               for (a, b), original in zip(CODE_RANGES, originals))
    assert all(bytes(u.mem_read(FAMILIES[name], 0x600)) == original
               for name, original in original_vtables.items())
    eax = u.reg_read(UC_X86_REG_EAX)
    result = dict(input=row, supplied_receivers=declared, after=state(u), events=events,
                owner_writes=writes, returned_eax=eax, returned_al=eax & 0xFF,
                return_is_boolean=row['entry'] in ('has_saved_mission', 'is_attack_move',
                                                   'resume', 'idle_resume'),
                observed_original_entries=visited,
                other_owner_bytes_preserved=True, original_code_and_vtables_unchanged=True)
    if extended:
        result.update(boundary='before_unrelated_expiry_tail' if row['entry'] == 'expiry_slice' else 'original_return',
                      boundary_address=f'{boundary:08X}',
                      coordinate_output=list(struct.unpack('<iii', u.mem_read(COORD_OUT, 12))),
                      event_bytes_after=bytes(u.mem_read(EVENT, 0x18)).hex(),
                      dummy_cell_after=list(struct.unpack('<hh', u.mem_read(DUMMY + 0x24, 4))),
                      fs_chain_restored=read_u32(u, 0) == 0xFFFFFFFF)
        assert result['fs_chain_restored']
    return result


def cases():
    rows = []

    def add(name, entry, changes=None, callbacks=None):
        for family in FAMILIES:
            rows.append(dict(name=f'{family}_{name}', family=family, entry=entry,
                             state=initial_state(**(changes or {})), callbacks=callbacks or {}))

    pairs = {'none': (0, 0), 'destination': (DEST_A, 0),
             'target': (0, TARGET_A), 'both': (DEST_A, TARGET_A)}
    for mission, latch in ((-1, 0), (29, 1), (30, 2)):
        add(f'clear_{mission}_{latch}', 'clear', dict(saved_mission=mission,
            saved_destination=DEST_A, saved_target=TARGET_A, engaged=latch))
    for mission in (-2, -1, 0, 1, 2, 5, 29, 30):
        for entry in ('has_saved_mission', 'is_attack_move'):
            add(f'{entry}_{mission}', entry, dict(saved_mission=mission,
                saved_destination=DEST_A, saved_target=TARGET_A, engaged=2))
    for pair, latch, mission in product(pairs, (0, 1, 2), (-1, 29)):
        dest, target = pairs[pair]
        add(f'resume_{pair}_latch{latch}_saved{mission}', 'resume',
            dict(saved_destination=dest, saved_target=target, engaged=latch,
                 saved_mission=mission))
    for pair, (dest, target) in pairs.items():
        for latch, saved, mission, queued in product((0, 1), (-1, 29, 30),
                                                    (2,), (-1,)):
            add(f'idle_{pair}_{latch}_{saved}_current2', 'idle_resume',
                dict(saved_destination=dest, saved_target=target, engaged=latch,
                     saved_mission=saved, mission=mission, queued_mission=queued))
        for latch, (mission, queued) in product((0, 1), ((5, 2), (2, 5), (-1, 5), (-1, 2), (-1, -1))):
            add(f'idle_{pair}_{latch}_current{mission}_queued{queued}', 'idle_resume',
                dict(saved_destination=dest, saved_target=target, engaged=latch,
                     mission=mission, queued_mission=queued))
    for pair, latch, checks, found in product(pairs, (0, 1), ((0, 0), (0, 1), (1, 0)), (0, 1)):
        dest, target = pairs[pair]
        add(f'acquire_{pair}_latch{latch}_checks{checks[0]}{checks[1]}_scan{found}',
            'acquire', dict(saved_destination=dest, saved_target=target, engaged=latch),
            dict(target_predicate=[action(value) for value in checks],
                 scan=[action(found, current_target=TARGET_B)]))

    # Queue receivers can mutate retained pointers; native resume reloads the
    # already-selected branch's pointer after that receiver, even if now null.
    for entry in ('resume', 'idle_resume'):
        for pair, field, replacement in (('destination', 'saved_destination', DEST_B),
                                         ('destination', 'saved_destination', 0),
                                         ('target', 'saved_target', TARGET_B),
                                         ('target', 'saved_target', 0)):
            dest, target = pairs[pair]
            add(f'{entry}_queue_reload_{field}_{replacement:08X}', entry,
                dict(saved_destination=dest, saved_target=target, engaged=1),
                dict(queue_mission=[action(0xFACE0000, **{field: replacement,
                    'engaged': 2, 'saved_mission': -1, 'alive': 0})]))
    add('resume_setter_mutations_survive', 'resume',
        dict(saved_destination=DEST_A, saved_target=TARGET_A, engaged=1),
        dict(assign_destination=[action(0xABCDE000, saved_destination=0,
             saved_target=TARGET_B, engaged=2, mission=5)]))
    add('idle_live_mission_first_non_guard_then_guard', 'idle_resume',
        dict(saved_destination=DEST_A, mission=2),
        dict(supplied_mission_getter=[action(2, mission=5), action(5)]))
    add('idle_first_guard_skips_second_read', 'idle_resume',
        dict(saved_target=TARGET_A, mission=5),
        dict(supplied_mission_getter=[action(5, mission=2)]))
    add('idle_second_non_guard_clears', 'idle_resume',
        dict(saved_target=TARGET_A, mission=2),
        dict(supplied_mission_getter=[action(2, queued_mission=5), action(2)]))
    add('idle_second_getter_mutates_branch_pointer', 'idle_resume',
        dict(saved_destination=DEST_A, saved_target=TARGET_A),
        dict(supplied_mission_getter=[action(2), action(5, saved_destination=0)]))
    add('acquire_scan_success_sets_latch_after_queue', 'acquire',
        dict(saved_destination=DEST_A),
        dict(scan=[action(1, current_target=TARGET_B)],
             queue_mission=[action(0xFACE0000, engaged=0, saved_destination=0,
                                   saved_target=TARGET_C, alive=0)]))
    add('acquire_failed_scan_restores_live_saved_target', 'acquire',
        dict(saved_target=TARGET_A),
        dict(target_predicate=[action(0)],
             scan=[action(0, saved_target=TARGET_B, current_target=TARGET_C)]))
    add('acquire_failed_scan_restores_null_saved_target', 'acquire',
        dict(saved_target=TARGET_A),
        dict(target_predicate=[action(0)],
             scan=[action(0, saved_target=0, current_target=TARGET_B)]))
    add('acquire_target_predicate_live_current_reload', 'acquire',
        dict(saved_target=TARGET_A, engaged=1),
        dict(target_predicate=[action(0, current_target=TARGET_B), action(1)]))
    add('acquire_destination_predicate_live_coords', 'acquire',
        dict(saved_destination=DEST_A, engaged=1),
        dict(target_predicate=[action(0, x=-768, y=4992, z=256)], scan=[action(0)]))
    add('acquire_destination_branch_retained_after_callback_clear', 'acquire',
        dict(saved_destination=DEST_A, saved_target=TARGET_A, engaged=1),
        dict(target_predicate=[action(0, saved_destination=0, saved_target=TARGET_B)],
             scan=[action(0, current_target=TARGET_C)]))
    add('acquire_no_saved_mission_direct_entry', 'acquire',
        dict(saved_mission=-1, saved_destination=DEST_A),
        dict(scan=[action(1, current_target=TARGET_B)]))
    for pair, latch in product(pairs, (0, 1)):
        dest, target = pairs[pair]
        add(f'acquire_no_current_target_{pair}_{latch}', 'acquire',
            dict(saved_destination=dest, saved_target=target, current_target=0, engaged=latch),
            dict(target_predicate=[action(0), action(0)], scan=[action(0)]))
    # Keep the original366 receiver rows as an identifiable, unchanged prefix.
    assert len(rows) == 366
    extension_start = len(rows)

    def extend(name, entry, changes=None, callbacks=None, **fixture):
        add(name, entry, changes, callbacks)
        for row in rows[-len(FAMILIES):]:
            row.update(fixture)

    tokens = {
        'absent': (0, 0), 'cell': (11, 5007),
        'object': (52, 101), 'missing_object': (52, 999),
        'unsupported_tag': (1, 202),
    }
    retained = dict(saved_mission=5, saved_destination=DEST_A,
                    saved_target=TARGET_A, engaged=2)
    for dest_name, target_name, (primary, prevent) in product(
            tokens, tokens, ((0, 0), (1, 0), (1, 1))):
        dest_tag, dest_id = tokens[dest_name]
        target_tag, target_id = tokens[target_name]
        extend(f'setup_{dest_name}_{target_name}_primary{primary}_prevent{prevent}',
               'setup', retained, primary=primary, prevent_attack_move=prevent,
               event=dict(event_mission=29, event_destination_tag=dest_tag,
                          event_destination_id=dest_id, event_target_tag=target_tag,
                          event_target_id=target_id))
    for mission in (-128, -1, 0, 1, 2, 5, 127):
        extend(f'setup_signed_mission_{mission}', 'setup', retained,
               primary=0, prevent_attack_move=1,
               event=dict(event_mission=mission, event_destination_tag=11,
                          event_destination_id=5007, event_target_tag=52, event_target_id=101))
    setup_event = dict(event_mission=29, event_destination_tag=11,
                       event_destination_id=5007, event_target_tag=52, event_target_id=101)
    extend('setup_eligibility_mutates_live_token_preserves_selected_branch', 'setup', retained,
           dict(supplied_eligibility=[action(1, event_destination_tag=52,
               event_destination_id=101, event_target_id=202, event_mission=1,
               saved_target=TARGET_C)]), event=setup_event)
    extend('setup_eligibility_clears_tag_selected_destination_becomes_null', 'setup', retained,
           dict(supplied_eligibility=[action(1, event_destination_tag=0,
               saved_target=TARGET_C)]), event=setup_event)
    extend('setup_eligibility_adds_tags_after_absence_snapshot', 'setup', retained,
           dict(supplied_eligibility=[action(1, event_destination_tag=11,
               event_destination_id=5007, event_target_tag=52, event_target_id=101)]),
           event=dict(event_mission=29))
    extend('setup_refusal_preserves_callback_mutations_and_original_tag_decision', 'setup', retained,
           dict(supplied_eligibility=[action(0, event_target_tag=0, saved_mission=-2,
               saved_destination=DEST_B, saved_target=TARGET_C, engaged=1)]), event=setup_event)
    extend('setup_callback_target_token_reload', 'setup', retained,
           dict(supplied_eligibility=[action(1, event_target_id=202)]),
           event=dict(event_mission=29, event_target_tag=52, event_target_id=101))
    extend('setup_noncanonical_tag_and_eligibility_al', 'setup', retained,
           dict(supplied_eligibility=[action(0x100)]),
           event=dict(event_mission=29, event_target_tag=255, event_destination_tag=255))
    for pair, (dest, target) in pairs.items():
        extend(f'saved_coordinate_{pair}', 'saved_coordinate',
               dict(saved_destination=dest, saved_target=target, engaged=1))
    extend('saved_coordinate_chosen_object_survives_callback_pointer_change', 'saved_coordinate',
           dict(saved_destination=DEST_A, saved_target=TARGET_A),
           dict(supplied_coordinate=[action(COORD_RETURN, saved_destination=0,
                                           saved_target=TARGET_B)]))
    extend('saved_coordinate_target_callback_return_buffer', 'saved_coordinate',
           dict(saved_target=TARGET_A),
           dict(supplied_coordinate=[action(COORD_RETURN, saved_target=0)]))
    coord_cases = [(0, 0, 0), (255, 256, 99), (256, 257, -99),
                   (-1, 511, 48), (-255, 512, 48), (-256, 512, 48),
                   (-257, 512, 48), (131071, 130816, 48),
                   (131072, 0, 48), (0, 131072, 48),
                   (2147483647, -2147483648, 48)]
    for coords, cell in product(coord_cases, (DEST_B, 0)):
        extend(f'convert_{coords[0]}_{coords[1]}_grid{cell:08X}', 'convert_target',
               dict(saved_destination=DEST_A, saved_target=TARGET_A, engaged=1),
               object_coords={str(TARGET_A): list(coords)}, map_cell=cell)
    extend('convert_no_target_clears_both', 'convert_target',
           dict(saved_destination=DEST_A, engaged=2))
    extend('convert_original_coordinate_wins_over_mutated_saved_target', 'convert_target',
           dict(saved_destination=DEST_A, saved_target=TARGET_A, engaged=1),
           dict(supplied_coordinate=[action(COORD_RETURN, saved_target=TARGET_B,
                                           saved_destination=TARGET_C, engaged=2)]),
           callback_coords=[-257, 511, 1234])
    # The expiry slice supplies only its documented live registers, starts after
    # unrelated archive/link callbacks, and stops before nav-queue processing.
    for pair, (dest, target) in pairs.items():
        for expired in (0, DEST_A, TARGET_A, TARGET_C):
            extend(f'expiry_{pair}_expired{expired:08X}', 'expiry_slice',
                   dict(saved_destination=dest, saved_target=target, engaged=1),
                   expired_pointer=expired)
    extend('expiry_converted_cell_equals_expired_pointer_cleared', 'expiry_slice',
           dict(saved_destination=DEST_A, saved_target=TARGET_A, engaged=2),
           expired_pointer=TARGET_A, map_cell=TARGET_A,
           object_coords={str(TARGET_A): [256, 256, 8]})
    extend('expiry_coordinate_callback_mutation_then_native_clear', 'expiry_slice',
           dict(saved_destination=DEST_A, saved_target=TARGET_A, engaged=1),
           dict(supplied_coordinate=[action(COORD_RETURN, saved_target=TARGET_B,
                                           saved_mission=30, engaged=2)]),
           expired_pointer=TARGET_A, callback_coords=[512, 768, 48])
    assert len(rows) > extension_start
    return rows


def generate():
    inputs = cases()
    assert len({row['name'] for row in inputs}) == len(inputs)
    rows = [execute(row) for row in inputs]
    assert {row['input']['entry'] for row in rows} == set(ENTRIES)
    return dict(schema_version=1, row_count=len(rows), original_receiver_rows=366,
                pointers=dict(owner=OWNER, destination_a=DEST_A, destination_b=DEST_B,
                              target_a=TARGET_A, target_b=TARGET_B, target_c=TARGET_C),
                fields={key: dict(offset=offset, size=size)
                        for key, (offset, size) in FIELDS.items()}, rows=rows)


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    counts = {entry: sum(row['entry'] == entry for row in cases()) for entry in ENTRIES}
    result = provenance(
        scope='Original Foot retained AttackMove setup, clear, predicates, coordinates/conversion, resume, idle resume and acquisition; bounded saved-pointer expiry slice; explicit callback substitutions and original resolver/map/type/RTTI helpers',
        assumptions=[
            'Full receiver entries use fresh native-shaped thiscall frames and finish through original RET; callee-saved registers, ESP and other owner bytes are checked. Setup and saved-coordinate accept one stack argument; other complete receivers accept none',
            'Four retained fields are Foot+5C4 saved mission, +5C8 saved Abstract destination pointer, +5CC saved target pointer and +5D1 engagement latch. Additional observed fields are current target+2B4, current mission+AC, queued mission+B4, alive+90 and position+9C/+A0/+A4. Original366 rows use identity sentinels; extension rows supply corresponding disjoint objects and coordinates',
            'Each row runs for Unit and Infantry copied fixture vtables. Actual retail clear+4A8, predicates+4AC/+4C4, resume+4C8, acquisition+4CC and idle-resume+4D0 slots are asserted; original vtable bytes remain unchanged',
            'Ordinary effective-mission calls execute original5B3040: current+AC unless -1, then queued+B4. Explicit mutation rows replace only this copied-vtable slot with listed external returns and field writes, establishing a callback boundary rather than claiming the actual getter mutates',
            'Callbacks are external x86 stubs with explicit per-call return/mutation scripts and fail on an unexpected extra invocation. Hooks only observe; they never alter memory, registers, branch outcomes or instruction bytes',
            'Every callback event records arguments and before/after state. Scan coordinate arguments are copied by the original body. QueueMission argument1 is recorded literally; actual QueueMission/Commence effects are not executed',
            'Resume and idle-resume report the actual AL result; EAX upper bytes are incidental. Acquisition has no boolean return contract here: returned AL/EAX are raw observations and may be callback residue. Clear EAX=0 is likewise observed, not an invented API contract',
            'Rows with saved_mission -1, latch2, dead-after-callback or inconsistent saved pointer pairs are direct receiver boundary probes, not proof of production reachability. Acquisition direct entry does not enforce its upstream attack-move/timer gate',
            'A supplied false target predicate for a null target is a boundary stress case, not a claim about native6F78D0: its underlying6F77B0 null-target path returns AL1. The actual weapon/NeverUse-sensitive query is excluded from these callback fixtures',
            'Setup executes original4DF0E0 and signed event byte+C, tags+12/+17, payloads+E/+13. Normal eligibility executes class746CC0/5228C0 through70F090, real owner/type getters and type711E90 on supplied primary+898 and PreventAttackMove+6C8. A supplied type vtable points+A4 at that original predicate. Dedicated mutation rows replace only owner+4C0',
            'Original token resolver6E6E20 handles tag11 via original Map5657A0, tag52 via supplied pre-sorted two-record index (101->TARGET_B,202->DEST_B), original523440 search and native CRT Abstract RTTI cast, and other tags via its null return. Null fallback is explicitly supplied zero; index sorting/registration and unrelated token types are not covered. FS:[0] is a mapped sentinel restored after native non-throwing casts; no Windows exception emulation',
            'Extension objects copy the actual class vtable and preceding RTTI locator. Ordinary+48 executes original5F65A0 from supplied object+9C; callback mutation rows instead return an explicit separate coordinate buffer. Coordinate consumer copy/division/narrowing/precedence instructions remain original',
            'Original Map5657A0 uses a supplied262144-entry grid with uniform configured pointer or null. Its signed linear-index checks, alias behavior and retained dummy-coordinate writes execute. This does not construct a real map or prove map-object lifetimes',
            'Expiry slice executes4D9AC9 through stop-before4D9B43 with ESI=owner, EDI=expired pointer, EBP=0 and private stack locals. Original upstream4D9978..4D998C establishes EBP0. All earlier Foot/Techno expiry callbacks/gates and following nav-queue processing are excluded; boundary rows are not full PointerExpired execution and raw EAX/AL at this stop are not returns',
            'Original366 receiver rows remain the unchanged payload prefix. Extensions exclude command executor4C71CA, actual target range/weapon query, threat scan selection/RNG, QueueMission scheduling, setter navigation/target effects, caller AI cadence, save/hash and full Foot/Unit idle behavior. No Rust comparison or whole-loop parity is claimed',
        ],
        substitutions=[
            'Fixture copies of retail vtables replace +1E8 QueueMission, +39C scan, +3B4 target predicate, +3C8 AssignTarget and +480 AssignDestination with declared external receiver bodies. Native branch/call/body bytes and retail vtables are never changed',
            'Three kinds of mission-getter mutation boundary plus second-read pointer mutation use external+184 scripts; all other+184 reads use original5B3040',
            'Extension-specific eligibility+4C0 and coordinate+48 mutation rows have explicit external return and memory-write scripts; ordinary extension rows execute native class/type eligibility and native coordinate getter. No direct token-resolver or map-lookup calls are replaced',
        ],
        entry_points={**ENTRIES, 'expiry_slice_stop': 0x4D9B43,
                      'effective_mission': GET_MISSION, 'token_resolver': TOKEN_RESOLVE,
                      'map_lookup': MAP_LOOKUP, 'object_coordinate': GET_COORD,
                      **{f'{family}_vtable': address for family, address in FAMILIES.items()}},
    )
    result['row_counts'] = counts
    result['original_code'] = {
        f'{a:08X}..{b:08X}': dict(hex=bytes(u.mem_read(a, b-a)).hex(),
                                  sha256=hashlib.sha256(bytes(u.mem_read(a, b-a))).hexdigest())
        for a, b in CODE_RANGES}
    result['original_external_slots'] = {
        family: {f'{slot:03X}': f'{read_u32(u, address + slot):08X}'
                 for slot, _, _ in CALLBACKS.values()}
        for family, address in FAMILIES.items()}
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
