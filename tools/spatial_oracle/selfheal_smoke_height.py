"""Original selfheal smoke retirement with real Object height/terrain branches.
The scenario supplies one real CellClass, its slope/level and an existing Smoke
ParticleSystem. No code patch or substituted call is used on this path.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import run_checked, finish_vectors, provenance, RET_MAGIC, SCRATCH
from tools.spatial_oracle.building_art_transition import ArtFixture
from tools.spatial_oracle.object_health import OBJ, TYPE, RULES, SP, dwords

TABLE=0xC00000
CELL=SCRATCH+0xA000
COORD=SCRATCH+0xA300
SPANS=((0x6FA743,0x6FA793),(0x5F5F40,0x5F5F92),(0x578080,0x5781A0),
       (0x565730,0x565798),(0x6301E0,0x6301E8))

def generate():
    f=ArtFixture(); u=f.u
    originals=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
    rows=[]
    reached=[]
    u.hook_add(UC_HOOK_CODE,lambda _u,a,_s,_d: reached.append(a) if a==0x5F5F40 else None)
    for (hp,strength),level,slope,bridge,relative,xy in product(
        ((49,100),(50,100),(-1,0)),(0,3),(0,1,4,17),(False,True),(-11,-10),((525,795),(759,969))):
        f.prepare(hp,strength,1,'all')
        u.mem_write(TABLE,bytes(0x100000))
        u.mem_write(TABLE+(3*512+2)*4,dwords(CELL))
        u.mem_write(0x87F924,dwords(TABLE))
        u.mem_write(0x87F928,dwords(0x40000))
        u.mem_write(CELL+0x24,struct.pack('<hh',2,3))
        u.mem_write(CELL+0x11B,bytes((level,slope)))
        u.mem_write(0xAC13BC,dwords(416))
        u.mem_write(COORD,dwords(*xy,0))
        u.mem_write(SP,dwords(RET_MAGIC,COORD))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_ECX,0x87F7E8)
        run_checked(u,0x578080,RET_MAGIC,count=1000)
        ground=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]
        z=(ground+(416 if bridge else 0)+relative)
        u.mem_write(OBJ+0x9C,dwords(*xy,z))
        u.mem_write(OBJ+0x8C,bytes((int(bridge),)))
        u.mem_write(TYPE+0xD14,b'\1')
        u.mem_write(RULES+0x16E0,struct.pack('<d',1.0))
        u.mem_write(0xA8ED84,dwords(900))
        # Restore the receiver registers supplied by the real Techno AI slice.
        from unicorn.x86_const import UC_X86_REG_ESI,UC_X86_REG_EBP
        u.reg_write(UC_X86_REG_ESI,OBJ)
        u.reg_write(UC_X86_REG_EBP,0)
        u.reg_write(UC_X86_REG_ESP,SP)
        reached.clear()
        run_checked(u,0x6FA743,0x6FA793,count=1500)
        rows.append(dict(input=dict(current=hp,strength=strength,level=level,slope=slope,
            on_bridge=bridge,relative_height=relative,xy=xy,ground=ground,z=z),
            output=dict(actual=f.read(OBJ+0x6C),smoke_done=int(u.mem_read(SCRATCH+0xF0F8,1)[0]),
                queried_height=bool(reached),retained_flag=int(u.mem_read(OBJ+0x6E6,1)[0]),
                smoke_pointer=f.read(OBJ+0x310))))
    assert originals==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
    return rows

def metadata():
    f=ArtFixture()
    p=provenance(scope='Original selfheal6FA743 through smoke tail with Object5F5F40 and actual ground578080',
        assumptions=['192 rows: strict-11/-10 heights, explicit bridge416, four slopes, two levels and two subcell coordinates',
            'HP49/100 becomes50 so height arm reached;50/100 becomes51 so height arm skipped; -1/0 becomes0/0 unordered',
            'Sparse real CellClass table supplied; no map-load or slope-producer equivalence claim'],
        substitutions=[],entry_points={'selfheal':0x6FA743,'height':0x5F5F40,'ground':0x578080})
    p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
        hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
    return p

if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
