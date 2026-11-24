"""Original bridge ground Health-pointer ABI and early Techno immunity writes.

Synthetic accepted warheads expose pointer aliasing; stock C4Warhead=Super does
not enable these three gates. This corpus establishes neither a stock-map
witness nor complete concrete Unit/Infantry/Building receiver semantics.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import (
    load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE,
    RET_MAGIC, finish_vectors, provenance,
)


OBJECT, TYPE, WARHEAD, VTABLE = (SCRATCH + n for n in (0, 0x2000, 0x4000, 0x6000))
CELL, RULES, PACKET = (SCRATCH + n for n in (0x7000, 0x8000, 0xA000))
FALSE_STUB, TYPE_STUB = SCRATCH + 0xB000, SCRATCH + 0xB010


def words(*values):
    return struct.pack('<' + 'I' * len(values), *(value & 0xFFFFFFFF for value in values))


def original_case(name, warhead_flag, type_flag, write_address, alias):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.mem_write(OBJECT, words(VTABLE))
    u.mem_write(OBJECT + 0x6C, words(100))
    u.mem_write(PACKET, words(100))
    u.mem_write(TYPE + type_flag, b'\x01')
    u.mem_write(WARHEAD + warhead_flag, b'\x01')
    for slot, entry in [(0x84, TYPE_STUB), (0x160, FALSE_STUB),
                        (0x1D4, FALSE_STUB), (0x16C, 0x701900)]:
        u.mem_write(VTABLE + slot, words(entry))
    u.mem_write(CELL + 0xE4, words(OBJECT))
    u.mem_write(0x8871E0, words(RULES))
    u.mem_write(RULES + 0xFA8, words(WARHEAD))
    # Explicit inactive bridge suppression byte and null Object.NextObject.
    u.mem_write(0xA8ED6B, b'\x00')
    u.mem_write(OBJECT + 0x30, words(0))
    writes, calls = [], []

    def code(uc, address, _size, _data):
        if address == 0x701900:
            sp = uc.reg_read(UC_X86_REG_ESP)
            args = struct.unpack('<7I', uc.mem_read(sp + 4, 28))
            calls.append(dict(damage_is_health=args[0] == OBJECT + 0x6C,
                              distance=args[1], warhead_matches=args[2] == WARHEAD,
                              attacker=args[3], ignore_defenses=args[4],
                              arg6=args[5], source_house=args[6]))
        if address not in (FALSE_STUB, TYPE_STUB):
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        destination = struct.unpack('<I', uc.mem_read(sp, 4))[0]
        uc.reg_write(UC_X86_REG_EAX, TYPE if address == TYPE_STUB else 0)
        uc.reg_write(UC_X86_REG_ESP, sp + 4)
        uc.reg_write(UC_X86_REG_EIP, destination)

    def write(uc, _access, address, size, value, _data):
        if address in (OBJECT + 0x6C, PACKET):
            writes.append(dict(location='health' if address == OBJECT + 0x6C else 'packet',
                               size=size, value=value,
                               instruction=f'{uc.reg_read(UC_X86_REG_EIP):08X}'))

    u.hook_add(UC_HOOK_CODE, code)
    u.hook_add(UC_HOOK_MEM_WRITE, write)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.reg_write(UC_X86_REG_ESP, sp)
    if alias:
        u.mem_write(sp, words(RET_MAGIC))
        u.reg_write(UC_X86_REG_ECX, CELL)
        # Stop after the complete ground list, before the deck/debris stages.
        begin, end, required = 0x47DD70, 0x47DDBA, [0x47DDAE, write_address]
    else:
        u.mem_write(sp, words(RET_MAGIC, PACKET, 0, WARHEAD, 0, 1, 1, 0))
        u.reg_write(UC_X86_REG_ECX, OBJECT)
        begin, end, required = 0x701900, RET_MAGIC, [write_address]
    run_checked(u, begin, end, count=2000, required_addresses=required)
    read = lambda address: struct.unpack('<i', u.mem_read(address, 4))[0]
    return dict(name=name, input_location='health' if alias else 'value',
                result=u.reg_read(UC_X86_REG_EAX), health=read(OBJECT + 0x6C),
                damage=read(OBJECT + 0x6C if alias else PACKET), calls=calls, writes=writes)


def cases():
    return dict(cases=[original_case(name, wh, typ, instruction, alias)
                      for name, wh, typ, instruction in [
                          ('radiation_immune', 0x177, 0xD37, 0x701C1C),
                          ('psychic_damage_immune', 0x178, 0xD36, 0x701C4F),
                          ('poison_immune', 0x156, 0xD3B, 0x701C82)]
                      for alias in (False, True)])


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original47DD70 ground loop calling original701900 early immunity returns; copied packet controls',
        assumptions=[
            'Synthetic single-member ground list, Health100, null next/source/house, forced flags true/true',
            'Accepted C4 warhead flags; stock Super has none of these immunity flags',
            'Zero supplied unused object/type fields; no ammo update, Object receiver, or death callbacks reached',
            'Aliased cases stop at47DDBA before deck/debris work; controls return from701900',
        ], substitutions=[
            'Supplied Object vtable16C binds directly to Techno701900; concrete class wrappers excluded',
            'Vtable160 and1D4 predicates return false; vtable84 returns supplied Type',
        ], entry_points={'bridge_ground':0x47DD70, 'techno_receive':0x701900}))
