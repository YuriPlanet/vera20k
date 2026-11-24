"""Original non-null Fly MoveTo with retained destination and mode writes.

Starts at the real interface entry. Original owner gates, ground queries and
Aircraft auxiliary methods execute. Cases keep the owner airborne or already
taking off, excluding BeginTakeoff's separate spatial/sound transaction.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import Fixture, OWNER, TYPE, TARGET, cell
from tools.spatial_oracle.map_queries import dwords

LOCO = SCRATCH + 0x30000
RULES = SCRATCH + 0x31000


def execute(case):
    f = Fixture(dict(aircraft=[10368,16512,case.get('z',500)]))
    u = f.u
    f.call(0x4CC9A0, LOCO, [])
    u.mem_write(LOCO + 0xC, dwords(OWNER))
    u.mem_write(LOCO + 0x1C, dwords(*case.get('previous', [0,0,0])))
    u.mem_write(LOCO + 0x34, bytes([case.get('moving', False)]))
    u.mem_write(LOCO + 0x38, dwords(37))
    u.mem_write(LOCO + 0x50, bytes([case.get('taking_off',False),case.get('landing',False)]))
    u.mem_write(LOCO + 0x5C, bytes([case.get('mode',False)]))
    u.mem_write(OWNER + 0x6C, dwords(100))
    u.mem_write(OWNER + 0x2FC, dwords(case.get('ammo',2)))
    u.mem_write(OWNER + 0x2B4, dwords(TARGET if case.get('target',False) else 0))
    u.mem_write(OWNER + 0x6D2, bytes([case.get('ready',False)]))
    u.mem_write(TYPE + 0xE0A, bytes([case.get('landable',True)]))
    # Type virtual+BC ->717800: +618, with -1 selecting Rules+7B4.
    u.mem_write(TYPE + 0x618, dwords(case.get('flight_level',-1)))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x7B4, dwords(1500))
    u.mem_write(cell(64,64) + 0x11B, bytes([case.get('level',0),case.get('slope',0)]))
    u.mem_write(0x89E7C0, dwords(104))
    u.mem_write(0xAC13C8, dwords(104))
    if not case.get('powered',True):
        u.mem_write(LOCO + 0x10, b'\0')
    request = case.get('request',[16512,16512,0])
    events=[]
    def observe(_u, pc, _size, _data):
        if pc in (0x4CF950,0x4CFA70):
            raise AssertionError('phase callback must be outside these supplied-state cases')
        if pc == 0x578080:
            sp=u.reg_read(UC_X86_REG_ESP)
            ptr=struct.unpack('<I',u.mem_read(sp+4,4))[0]
            events.append(dict(call='ground',xyz=list(struct.unpack('<iii',u.mem_read(ptr,12)))))
        elif pc in (0x41B6A0,0x41B860):
            events.append(dict(call='landing_base' if pc==0x41B6A0 else 'ready'))
    u.hook_add(UC_HOOK_CODE,observe)
    original=bytes(u.mem_read(0x4CCC80,0x344))
    u.mem_write(f.sp,dwords(RET_MAGIC,LOCO+4,*request))
    u.reg_write(UC_X86_REG_ESP,f.sp)
    run_checked(u,0x4CCC80,RET_MAGIC,count=30000)
    assert u.reg_read(UC_X86_REG_ESP)==f.sp+20
    assert original==bytes(u.mem_read(0x4CCC80,0x344))
    return dict(input=case,destination=list(struct.unpack('<iii',u.mem_read(LOCO+0x1C,12))),
                moving=bool(u.mem_read(LOCO+0x34,1)[0]),mode=bool(u.mem_read(LOCO+0x5C,1)[0]),
                height=struct.unpack('<i',u.mem_read(LOCO+0x38,4))[0],events=events)


def generate():
    rows=[dict(name='plain')]
    rows += [dict(name=f'z_{z}',request=[16512,16512,z]) for z in (-10,119,120,121,500)]
    rows += [dict(name=f'armed_{ammo}',target=True,ammo=ammo) for ammo in (-1,0,1,2)]
    rows += [dict(name='ready',ready=True),dict(name='nonlandable',landable=False),
             dict(name='unpowered',powered=False,previous=[20000,21000,17],moving=True,mode=True),
             dict(name='same_cell_landing',landing=True,previous=[16500,16500,999],request=[16512,16512,200]),
             dict(name='same_cell_takeoff',taking_off=True,previous=[16500,16500,999],request=[16512,16512,200])]
    rows += [dict(name=f'ground_{level}_{slope}',target=True,level=level,slope=slope)
             for level,slope in ((1,0),(2,1),(3,4))]
    rows += [dict(name=f'level_{level}',target=True,flight_level=level) for level in (-1,0,120,121,40000)]
    rows += [dict(name='subcell',request=[16519,16523,333]),
             dict(name='signed_cell_alias',landing=True,previous=[-256,-256,77],request=[16776960,16776960,200]),
             dict(name='signed_cell_truncation',landing=True,previous=[-255,-255,77],request=[1,1,200])]
    return [execute(row) for row in rows]


if __name__ == '__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        entry_points={'move_to':0x4CCC80,'constructor':0x4CC9A0,'ground':0x578080,
                      'landing_base':0x41B6A0,'readiness':0x41B860,'flight_level':0x717800},
        assumptions=['Original Aircraft/Fly vtables; supplied health100 and airborne coordinate or taking-off phase avoid BeginTakeoff.',
                     'Non-null destination only; landing same-cell refusal precedes gates and all queries.',
                     'Flat allocated map, optional destination level/slope; owner warp/EMP/Foot timer gates zero, radio and cargo empty.',
                     'Constructor initializes power; optional power-off contrast. Original executable and all callees unmodified.'],
        substitutions=[],
        scope='26 original non-null MoveTo calls: retained XYZ, signed-cell landing refusal, power refusal, Target/Ammo Z substitution, height120 boundary, readiness and Landable mode. Rust compares destination, refusals and mode. Moving-byte lifetime, null requests, phase callbacks, owner-gate producers and the subsequent Process/navigation/mission chain remain outside Rust comparison coverage.',
    ))
