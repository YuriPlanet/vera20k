"""Original Aircraft Mission_Attack successful-release loop and state selection.

Admission is supplied by entering418403. Weapon selection and FireAt are explicit
scratch callbacks (FireAt returns null and has no effects); original GetWeapon,
Aircraft auxiliary classifiers and mission writes execute. The intervening map
reveal and the later mission entries are excluded, not replaced by claimed parity.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, run_checked, finish_vectors, provenance
from tools.spatial_oracle.crate_speed_effect import fixture
from tools.spatial_oracle.map_queries import dwords

VTABLE, SELECT, FIRE, WEAPON, ELITE, PROJECTILE, TARGET = [SCRATCH+n for n in
    (0x10000, 0x11000, 0x11100, 0x12000, 0x13000, 0x14000, 0x15000)]


def execute(case):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind='aircraft')]))
    owner = actors[0]
    object_type = owner + 0x800
    # Clone the original Aircraft table, changing only the two declared calls.
    u.mem_write(VTABLE, bytes(u.mem_read(0x7E22A4, 0x600)))
    u.mem_write(VTABLE+0x2E4, dwords(SELECT))
    u.mem_write(VTABLE+0x3CC, dwords(FIRE))
    u.mem_write(SELECT, b'\x31\xC0\xC2\x04\x00')
    u.mem_write(FIRE, b'\x31\xC0\xC2\x08\x00')
    u.mem_write(owner, dwords(VTABLE))
    u.mem_write(owner+0x6C0, dwords(0x7E2250))
    u.mem_write(owner+0x2B4, dwords(TARGET))
    u.mem_write(owner+0x2FC, dwords(case['ammo']))
    u.mem_write(owner+0xBC, dwords(4))
    u.mem_write(owner+0x150, struct.pack('<f', case.get('veterancy', 0)))
    u.mem_write(object_type+0x898, dwords(WEAPON))
    u.mem_write(object_type+0xA94, dwords(ELITE if case.get('elite_weapon', True) else 0))
    u.mem_write(object_type+0xE0E, bytes([case['fighter']]))
    for weapon, burst, rof in ((WEAPON, case['burst'], 20), (ELITE, case.get('elite_burst', 3), 37)):
        u.mem_write(weapon+0x9C, dwords(burst))
        u.mem_write(weapon+0xA0, dwords(PROJECTILE))
        u.mem_write(weapon+0xB0, dwords(rof))
    u.mem_write(PROJECTILE+0x2DC, dwords(case['rot']))
    u.mem_write(PROJECTILE+0x29E, bytes([case['inviso']]))
    u.reg_write(UC_X86_REG_ESI, owner)
    u.reg_write(UC_X86_REG_EBX, owner+0x6C0)
    u.reg_write(UC_X86_REG_ESP, sp)
    events=[]
    def observe(_u, pc, _size, _data):
        if pc == SELECT:
            events.append(dict(call='select', pending=u.mem_read(owner+0x6C8,1)[0]))
        elif pc == FIRE:
            call_sp=u.reg_read(UC_X86_REG_ESP)
            assert u.reg_read(UC_X86_REG_ECX) == owner
            assert bytes(u.mem_read(call_sp+4,8)) == dwords(TARGET,0)
            events.append(dict(call='fire', pending=u.mem_read(owner+0x6C8,1)[0],
                               ammo=struct.unpack('<i',u.mem_read(owner+0x2FC,4))[0]))
    u.hook_add(UC_HOOK_CODE,observe)
    run_checked(u,0x418403,0x418478,count=30000,required_addresses=[0x41840E,0x70E140])
    assert u.reg_read(UC_X86_REG_ESP) == sp
    # Resume after the omitted map reveal with its preserved ESI/EBX/SP.
    run_checked(u,0x4184C2,(0x4184F1,0x418539,0x418581),count=5000,
                required_addresses=[0x41B7F0])
    assert u.reg_read(UC_X86_REG_ESP) == sp
    assert struct.unpack('<i',u.mem_read(owner+0x2FC,4))[0] == case['ammo']
    return dict(input=case,events=events,state=struct.unpack('<I',u.mem_read(owner+0xBC,4))[0],
                pending=bool(u.mem_read(owner+0x6C8,1)[0]),latch_6d2=bool(u.mem_read(owner+0x6D2,1)[0]),
                delay=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0])


def entry_housekeeping(case):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind='aircraft')]))
    owner=actors[0]
    u.mem_write(owner+0x2FC,dwords(case['ammo']))
    u.mem_write(owner+0x6C8,bytes([case['pending']]))
    u.mem_write(owner+0x6D2,bytes([case['latch_6d2']]))
    u.reg_write(UC_X86_REG_ESI,owner)
    u.reg_write(UC_X86_REG_ESP,sp)
    entry,end={1:(0x418031,0x418056),3:(0x4180A1,0x4180CC),
               10:(0x418BEC,0x418C15)}[case['state']]
    run_checked(u,entry,end,count=100)
    assert u.reg_read(UC_X86_REG_ESP)==sp
    return dict(input=case,ammo=struct.unpack('<i',u.mem_read(owner+0x2FC,4))[0],
                pending=bool(u.mem_read(owner+0x6C8,1)[0]),
                latch_6d2=bool(u.mem_read(owner+0x6D2,1)[0]))


def generate():
    rows=[dict(ammo=ammo,burst=burst,fighter=fighter,rot=rot,inviso=inviso)
          for ammo in (-1,1,2) for burst in (-1,0,1,2,5)
          for fighter in (False,True) for rot in (-1,0,1,2,3) for inviso in (False,True)]
    rows += [dict(ammo=2,burst=2,elite_burst=3,fighter=fighter,rot=rot,inviso=False,
                  veterancy=veterancy,elite_weapon=elite_weapon)
             for fighter in (False,True) for rot in (0,3)
             for veterancy in (1,2) for elite_weapon in (False,True)]
    entries=[dict(state=state,ammo=ammo,pending=pending,latch_6d2=latch)
             for state in (1,3,10) for ammo in (-2147483648,-1,0,1,2,2147483647)
             for pending in (False,True) for latch in (False,True)]
    return dict(releases=[execute(row) for row in rows],
                entries=[entry_housekeeping(row) for row in entries])


if __name__ == '__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        entry_points={'release_loop':0x418403,'loop_end':0x418478,'post_reveal':0x4184C2,
                      'get_weapon':0x70E140,'auxiliary_18':0x41B7F0,'fighter':0x41B840,
                      'state1_entry':0x418031,'state3_entry':0x4180A1,'state10_entry':0x418BEC},
        assumptions=['Entry after successful GetFireError; target, ammo, tier and weapon/type inputs supplied.',
                     'Original Aircraft vtable cloned to scratch; original auxiliary vtable and GetWeapon execute.',
                     'Map reveal418478..4184C2 omitted; original preserved ESI/EBX and balanced stack resumed.',
                     'Entry-housekeeping rows stop before target/ammo guards and navigation; signed Ammo and retained flags supplied.',
                     'No FireAt effects, target mutation, damage, RNG, admission or next-entry scheduling claim.'],
        substitutions=['Scratch SelectWeapon returns slot0; scratch FireAt records arguments and returns null without effects.'],
        scope='316 bounded original successful-release loops and next-state/delay decisions, plus72 original state1/3/10 pending-ammo housekeeping prefixes. Native loop counts, pending-before-FireAt, no mission-side immediate ammo decrement, tier fallback and auxiliary/Fighter branches. Not whole-burst combat parity.',
    ))
