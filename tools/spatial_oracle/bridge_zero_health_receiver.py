"""Original Infantry receiver reaches its death tail after a zero-HP join.

Uses the stock-Super selector/forced/null-source ABI. Cases establish entry
into Death_Announcement and a separate one-entry DieSound draw/PlayAt request.
Sound playback, announcement owner/UI effects and the remaining Infantry action
are excluded. Object callbacks must not be replayed merely because Techno
re-enters its own fatal postlude.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import (
    load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE,
    RET_MAGIC, finish_vectors, provenance,
)
from tools.rmg_oracle.gen_rng_vectors import seeded_struct, STRUCT_LEN


def words(*values):
    return struct.pack('<' + 'I' * len(values), *(value & 0xFFFFFFFF for value in values))


def original_case(alive, die_sound, *, entry=0x517FA0, death_weapon=False):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    for base, size in [(STACK_BASE, STACK_SIZE), (SCRATCH, SCRATCH_SIZE), (RET_MAGIC, 0x1000)]:
        u.mem_map(base, size)
    obj, typ, vtable, warhead, rules, weapon = (SCRATCH + n for n in (0, 0x2000, 0x4000, 0x5000, 0x6000, 0x8000))
    u.mem_write(obj, words(vtable))
    u.mem_write(obj + 0x6C, words(0))
    u.mem_write(obj + 0x90, bytes((alive,)))
    u.mem_write(warhead + 0x120, words(2))
    if death_weapon:
        u.mem_write(typ + 0xD15, b'\x01')  # Explodes=yes.
    u.mem_write(0x8871E0, words(rules))
    u.mem_write(vtable + 0x3B8, words(0x4D98C0))
    rng = 0x886B88
    u.mem_write(rng, seeded_struct(1))
    rng_before = bytes(u.mem_read(rng, STRUCT_LEN))
    if die_sound:
        # The pointer is the vector's items at Type+514; +520 is count.
        u.mem_write(typ + 0x514, words(SCRATCH + 0x8100))
        u.mem_write(typ + 0x520, words(1))
        u.mem_write(SCRATCH + 0x8100, words(0x123))
    stubs = {}
    for slot, argc, value in [
        (0x84, 0, typ), (0x160, 0, 0), (0x1D4, 0, 0), (0x280, 1, 0),
        (0x3A0, 0, 0), (0x1C8, 0, 0), (0x3F8, 1, weapon),
    ]:
        address = SCRATCH + 0x9000 + slot
        u.mem_write(vtable + slot, words(address))
        stubs[address] = (argc, value)
    visited = []

    def code(uc, address, _size, _data):
        if address in (0x517FA0, 0x737C90, 0x4D7330, 0x701900, 0x5F5390, 0x702035, 0x518077, 0x65C780, 0x702603):
            visited.append(f'{address:08X}')
        if address not in stubs:
            return
        argc, value = stubs[address]
        sp = uc.reg_read(UC_X86_REG_ESP)
        destination = struct.unpack('<I', uc.mem_read(sp, 4))[0]
        uc.reg_write(UC_X86_REG_EAX, value)
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + argc * 4)
        uc.reg_write(UC_X86_REG_EIP, destination)

    u.hook_add(UC_HOOK_CODE, code)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, words(RET_MAGIC, obj + 0x6C, 0, warhead, 0, 1, 1, 0))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, obj)
    endpoint = 0x70D690 if death_weapon else (0x7509E0 if die_sound else 0x4D98C0)
    required = [0x5F5390, 0x702035, 0x702603 if death_weapon else (0x65C780 if die_sound else 0x518077)]
    run_checked(u, entry, endpoint, count=3000, required_addresses=required)
    rng_after = bytes(u.mem_read(rng, STRUCT_LEN))
    result = dict(initial_alive=bool(alive), initial_health=0, die_sound_count=int(die_sound),
                final_health=struct.unpack('<i', u.mem_read(obj + 0x6C, 4))[0],
                final_alive=bool(u.mem_read(obj + 0x90, 1)[0]),
                endpoint=f'{u.reg_read(UC_X86_REG_EIP):08X}',
                sound_id=u.reg_read(UC_X86_REG_ECX) if die_sound else None,
                announcement_receiver_matches=None if die_sound else u.reg_read(UC_X86_REG_ECX) == obj,
                rng_draws=visited.count('0065C780'), rng_state_changed=rng_after != rng_before,
                rng_indices_before=list(struct.unpack_from('<II', rng_before, 4)),
                rng_indices_after=list(struct.unpack_from('<II', rng_after, 4)),
                visited=visited)
    if death_weapon:
        result['entry'] = f'{entry:08X}'
        result['death_weapon_receiver_matches'] = result.pop('announcement_receiver_matches')
    return result


def cases():
    return dict(cases=[original_case(alive, sound) for sound in (False, True) for alive in (0, 1)])


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Infantry517FA0 -> Foot4D7330 -> Techno701900 -> Object5F5390; stop at Death_Announcement4D98C0 or DieSound PlayAt7509E0',
        assumptions=[
            'Health0 and Alive0/1 variants, damage aliases Health, distance0, null source/house, forced flags true/true',
            'Stock Super InfDeath2; zero other supplied object/type/warhead/rules state except optional one-entry DieSound vector',
            'Synthetic DieSound ID123hex; actual Random65C6D0 seeds main RNG886B88 with1; original65C780 draw has no substitution',
            'No original Object exact-zero callbacks run: its entry Health gate returns before them',
            'Sound playback, Death_Announcement body and remaining Infantry postlude are excluded',
        ], substitutions=[
            'Seven supplied vtable results:84=Type,160=false,1D4=false,280=false,3A0=false,1C8=height0,3F8=emptyWeaponRecord',
            'Vtable3B8 binds original Death_Announcement; no executable instructions replaced',
        ], entry_points={'infantry_receive':0x517FA0, 'foot_receive':0x4D7330,
                         'techno_receive':0x701900, 'object_receive':0x5F5390,
                         'random_seed':0x65C6D0, 'random_next':0x65C780}))
