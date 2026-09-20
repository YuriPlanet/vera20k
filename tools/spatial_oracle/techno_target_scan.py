"""Original 709820 scanner ordering, with explicit external receiver fixtures.

No original code is replaced. Direct callees use segmented execution with their
real CALL frames; virtual calls use a copied Unit vtable. This does not execute
the supplied threat, GetFireError, weapon, damage or DistributedFire bodies.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI,
    UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_ESP, UC_X86_REG_EIP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

ENTRY, END, UNIT_VTABLE = 0x709820, 0x7099CD, 0x7F5C70
OWNER, VTABLE, TYPE, TARGET, OTHER, WEAPON_SLOT, WEAPON, PROJECTILE_TYPE, RULES, SCENARIO, SPAWN, COORD = (
    SCRATCH + offset for offset in
    (0x1000, 0x2000, 0x3000, 0x4000, 0x4800, 0x5000, 0x5100, 0x5500,
     0x6000, 0x7000, 0x7400, 0x7800))
SP = STACK_BASE + STACK_SIZE - 0x1000
# name: (original direct entry or virtual slot, stack argument byte count)
VIRTUAL = dict(select_weapon=(0x2E4, 4), fire_error=(0x3C0, 12),
               assign_target=(0x3C8, 4), greatest_threat=(0x3C4, 12),
               get_type=(0x84, 0), get_weapon=(0x3F8, 4))
DIRECT = dict(random=(0x65C7E0, 8), spawn_abandon=(0x6B7BB0, 0),
              distributed_fire=(0x709550, 0), estimated_damage=(0x6FDB80, 8))
STUBS = {name: SCRATCH + 0x8000 + 0x200 * index
         for index, name in enumerate((*VIRTUAL, *DIRECT))}
FIELDS = dict(target=(OWNER + 0x2B4, 4), passive=(OWNER + 0x50C, 1),
              last_scan=(OWNER + 0x4FC, 4), timer_start=(OWNER + 0x180, 4),
              timer_aux=(OWNER + 0x184, 4), timer_duration=(OWNER + 0x188, 4),
              estimated_health=(TARGET + 0x70, 4),
              other_health=(OTHER + 0x70, 4))
SAVED = {UC_X86_REG_EBX: 0x12345678, UC_X86_REG_EBP: 0x23456789,
         UC_X86_REG_ESI: 0x3456789A, UC_X86_REG_EDI: 0x456789AB}


def u32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def state(u):
    return {key: int.from_bytes(u.mem_read(address, size), 'little')
            for key, (address, size) in FIELDS.items()}


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    original = bytes(u.mem_read(ENTRY, END - ENTRY))
    original_vtable = bytes(u.mem_read(UNIT_VTABLE, 0x600))
    assert u32(u, UNIT_VTABLE + 0x39C) == ENTRY
    u.mem_write(VTABLE, original_vtable)
    u.mem_write(OWNER, dwords(VTABLE))
    u.mem_write(OWNER + 0xAC, dwords(row['mission']))
    u.mem_write(OWNER + 0x2D0, dwords(SPAWN if row['spawn'] else 0))
    for key, value in dict(target=row['target'], passive=row['passive'],
                           last_scan=99, timer_start=33, timer_aux=44,
                           timer_duration=55, estimated_health=row['health'],
                           other_health=456).items():
        address, size = FIELDS[key]
        u.mem_write(address, bytes((value,)) if size == 1 else dwords(value))
    u.mem_write(TARGET + 0x14, bytes((row['pick_flags'],)))
    u.mem_write(OTHER + 0x14, b'\x01')
    u.mem_write(TYPE + 0x6B0, bytes((row['distributed'],)))
    u.mem_write(WEAPON_SLOT, dwords(WEAPON if row['weapon'] else 0))
    # Weapon parser7729A5..AA resolves/stores Projectile at+A0; Warhead is+AC.
    # Keep BulletType+2A2 unnamed beyond its observed debit-exclusion effect.
    u.mem_write(WEAPON + 0xA0, dwords(PROJECTILE_TYPE))
    u.mem_write(PROJECTILE_TYPE + 0x2A2, bytes((row['projectile_debit_excluded'],)))
    u.mem_write(RULES + 0xE04, dwords(41, 23))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(0xA8B230, dwords(SCENARIO))
    u.mem_write(0xA8ED84, dwords(173))
    u.mem_write(COORD, dwords(2688, -384, 48))
    # Native reads this uninitialised local into the timer's inactive member.
    # Supply an explicit stack seed; do not claim a universal native value.
    u.mem_write(SP - 8, dwords(row['stack_seed']))
    returns = dict(select_weapon=1, fire_error=row['error'], assign_target=0,
                   greatest_threat=row['pick'], get_type=TYPE,
                   get_weapon=WEAPON_SLOT, random=row['jitter'],
                   spawn_abandon=0, distributed_fire=0,
                   estimated_damage=row['damage'])
    specs = {}
    for name, (native, pop) in {**VIRTUAL, **DIRECT}.items():
        body = b''
        if name == 'assign_target':
            body += b'\x8B\x44\x24\x04\xA3' + dwords(OWNER + 0x2B4)
            body += b'\xC6\x05' + dwords(OWNER + 0x50C) + b'\x00'
        changes = row.get('mutations', {}).get(name, {})
        for key, value in changes.items():
            address, size = FIELDS[key]
            body += ((b'\xC6\x05' + dwords(address) + bytes((value,))) if size == 1
                     else (b'\xC7\x05' + dwords(address, value)))
        body += b'\xB8' + dwords(returns[name])
        body += b'\xC2' + struct.pack('<H', pop) if pop else b'\xC3'
        u.mem_write(STUBS[name], body)
        if name in VIRTUAL:
            u.mem_write(VTABLE + native, dwords(STUBS[name]))
        specs[name] = dict(native=f'{u32(u, UNIT_VTABLE + native) if name in VIRTUAL else native:08X}',
                           args_bytes=pop, return_eax=returns[name], mutations=changes,
                           setter_effects=name == 'assign_target')
    u.mem_write(SP, dwords(RET_MAGIC, COORD, row['mask']))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, OWNER)
    for reg, value in SAVED.items():
        u.reg_write(reg, value)
    before_owner = bytes(u.mem_read(OWNER, 0x800))
    events, writes = [], []
    by_address = {address: name for name, address in STUBS.items()}

    def observe(uc, address, _size, _data):
        if address not in by_address:
            return
        name = by_address[address]
        sp = uc.reg_read(UC_X86_REG_ESP)
        args = [u32(uc, sp + 4 + offset)
                for offset in range(0, specs[name]['args_bytes'], 4)]
        expected_this = SCENARIO + 0x218 if name == 'random' else SPAWN if name == 'spawn_abandon' else OWNER
        assert uc.reg_read(UC_X86_REG_ECX) == expected_this
        event = dict(name=name, args=args, before=state(uc))
        if name == 'greatest_threat':
            event['coordinate_argument'] = list(struct.unpack('<iii', uc.mem_read(args[1], 12)))
        events.append(event)

    def observe_write(uc, _access, address, size, value, _data):
        key = next((key for key, pair in FIELDS.items() if pair == (address, size)), None)
        if key is not None:
            pc = uc.reg_read(UC_X86_REG_EIP)
            writes.append(dict(field=key, size=size, value=value, instruction=f'{pc:08X}',
                               origin='original' if ENTRY <= pc < END else 'supplied_callback'))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    direct_entries = {address: name for name, (address, _) in DIRECT.items()}
    current = ENTRY
    for _ in range(10):
        boundary = run_checked(u, current, (RET_MAGIC, *direct_entries), count=1000,
                               required_addresses=(current,))
        if boundary == RET_MAGIC:
            break
        name = direct_entries[boundary]
        return_address = u32(u, u.reg_read(UC_X86_REG_ESP))
        run_checked(u, STUBS[name], return_address, count=100,
                    required_addresses=(STUBS[name],))
        current = return_address
    else:
        raise AssertionError('Unexpected repeated external direct calls')
    assert u.reg_read(UC_X86_REG_ESP) == SP + 12
    assert all(u.reg_read(reg) == value for reg, value in SAVED.items())
    assert bytes(u.mem_read(ENTRY, END - ENTRY)) == original
    assert bytes(u.mem_read(UNIT_VTABLE, 0x600)) == original_vtable
    after_owner = bytearray(u.mem_read(OWNER, 0x800))
    for address, size in FIELDS.values():
        if OWNER <= address < OWNER + 0x800:
            offset = address - OWNER
            after_owner[offset:offset+size] = before_owner[offset:offset+size]
    assert bytes(after_owner) == before_owner
    assert [e['name'] for e in events].count('random') == 1
    assert events[0]['name'] == 'random' and events[0]['args'] == [0, 2]
    return dict(input=row, supplied_receivers=specs, events=events, writes=writes,
                after=state(u), returned_al=u.reg_read(UC_X86_REG_EAX) & 255,
                original_code_and_vtable_unchanged=True)


def generate():
    rows = []

    def add(name, **changes):
        row = dict(name=name, mission=5, target=0, passive=0, spawn=False,
                   error=0, pick=TARGET, pick_flags=1, distributed=0, weapon=True,
                   projectile_debit_excluded=0, health=100, damage=25, jitter=1,
                   stack_seed=0x1234ABCD, mask=1)
        row.update(changes)
        rows.append(row)

    for target, passive, error, spawn in product((0, TARGET), (0, 1), range(10), (False, True)):
        add(f'drop_{target}_{passive}_{error}_{spawn}', target=target,
            passive=passive, error=error, spawn=spawn)
    for pick, flags, distributed, weapon, excluded in product(
            (0, TARGET), (0, 1), (0, 1), (False, True), (0, 1)):
        add(f'install_{pick}_{flags}_{distributed}_{weapon}_{excluded}',
            pick=pick, pick_flags=flags, distributed=distributed, weapon=weapon,
            projectile_debit_excluded=excluded)
    for mission, jitter, mask in product((5, 11, -1), (0, 1, 2), (0, 1, 2, 3, 7)):
        add(f'cadence_{mission}_{jitter}_{mask}', mission=mission, jitter=jitter, mask=mask)
    for health, damage in ((0, 1), (0x80000000, 1), (0x7FFFFFFF, -1), (100, -25)):
        add(f'debit_{health}_{damage}', health=health, damage=damage)
    for callback, changes in (
            ('select_weapon', dict(target=OTHER)),
            ('fire_error', dict(target=OTHER)),
            ('spawn_abandon', dict(target=0)),
            ('assign_target', dict(target=OTHER)),
            ('get_weapon', dict(target=0)),
            ('estimated_damage', dict(estimated_health=7)),
            ('distributed_fire', dict(target=OTHER))):
        add(f'mutation_{callback}', target=TARGET if callback in ('fire_error', 'spawn_abandon') else 0,
            passive=1, error=6, spawn=True, distributed=int(callback == 'distributed_fire'),
            mutations={callback: changes})
    add('mutation_preexisting_select_weapon', target=TARGET, passive=1,
        mutations={'select_weapon': dict(target=OTHER)})
    add('mutation_greatest_threat', mutations={'greatest_threat': dict(target=OTHER)})
    add('mutation_get_type', mutations={'get_type': dict(target=OTHER)})
    results = [execute(row) for row in rows]
    reload_row = next(result for result in results
                      if result['input']['name'] == 'mutation_preexisting_select_weapon')
    fire_error = next(event for event in reload_row['events'] if event['name'] == 'fire_error')
    assert fire_error['args'] == [OTHER, 1, 1]
    return results


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope='Original complete Techno scanner709820 callback ordering and retained writes; all external callees supplied',
        assumptions=[
            'Copied Unit vtable; original 709820..7099CD and Unit table unchanged; stack and nonvolatile registers verified',
            'Timer inactive member receives explicitly seeded uninitialised stack local; its value is not a universal native invariant',
            'Owner current mission selects delay; scenario RNG draw supplied, not full RNG parity; frame173, normal23, area41',
            'AssignTarget fixture writes current target and clears passive byte; actual class setter effects excluded',
            'Original body reads supplied GetFireError codes and threat results; this is not proof of their eligibility or selection semantics',
            'Original body performs estimated-health subtraction; damage, weapon selection, projectile-type exclusion and DistributedFire callback semantics are not implemented by this fixture',
        ],
        substitutions=[
            'Direct calls65C7E0/6B7BB0/709550/6FDB80 segmented only at callee entry; execute supplied machine RET with real call frame',
            'Copied virtual slots2E4/3C0/3C8/3C4/84/3F8 use explicit supplied machine receivers; per-row return values and writes recorded',
        ],
        entry_points={'scanner': ENTRY, 'unit_vtable': UNIT_VTABLE},
    )
    result['original_code_sha256'] = hashlib.sha256(bytes(u.mem_read(ENTRY, END-ENTRY))).hexdigest()
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
