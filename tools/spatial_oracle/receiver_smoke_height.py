"""Original received-damage smoke gate, actual Object ground/bridge height.
Stops at selected-type continuation or no-spawn continuation; no substitutedcalls.
"""
from itertools import product
from pathlib import Path
import hashlib,struct
from unicorn.x86_const import *
from tools.native_oracle import run_checked,finish_vectors,provenance,RET_MAGIC
from tools.spatial_oracle.object_health import Fixture,OBJ,SP,dwords
from tools.spatial_oracle.selfheal_smoke_height import TABLE,CELL,COORD
SPANS=((0x702952,0x702965),(0x5F5F40,0x5F5F92),(0x578080,0x5781A0),(0x565730,0x565798))

def generate():
 f=Fixture();u=f.u;rows=[]
 originals=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 for level,slope,bridge,relative,xy in product((0,3),(0,1,4,17),(False,True),(-11,-10,-9),((525,795),(759,969))):
  f.reset('building',25,100)
  u.mem_write(TABLE,bytes(0x100000));u.mem_write(TABLE+(3*512+2)*4,dwords(CELL))
  u.mem_write(0x87F924,dwords(TABLE));u.mem_write(0x87F928,dwords(0x40000))
  u.mem_write(CELL+0x24,struct.pack('<hh',2,3));u.mem_write(CELL+0x11B,bytes((level,slope)))
  u.mem_write(0xAC13BC,dwords(416));u.mem_write(COORD,dwords(*xy,0))
  u.mem_write(SP,dwords(RET_MAGIC,COORD));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,0x87F7E8)
  run_checked(u,0x578080,RET_MAGIC,count=1000)
  ground=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]
  z=ground+(416 if bridge else 0)+relative
  u.mem_write(OBJ+0x9C,dwords(*xy,z));u.mem_write(OBJ+0x8C,bytes((int(bridge),)))
  u.reg_write(UC_X86_REG_ESI,OBJ);u.reg_write(UC_X86_REG_ESP,SP)
  endpoint=run_checked(u,0x702952,(0x702965,0x702A04),required_addresses=(0x5F5F40,),count=1500)
  rows.append(dict(input=dict(level=level,slope=slope,on_bridge=bridge,relative_height=relative,xy=xy,ground=ground,z=z),
   output=dict(spawn_admitted=endpoint==0x702965,height=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0])))
 assert originals==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 return rows

def metadata():
 f=Fixture();p=provenance(scope='Original reached Techno702952 smoke spawn height predicate',
  assumptions=['96rows height-11/-10/-9;twolevels,fourslopes,explicitbridge416,twosubcellcoords',
   'Earlier health/pointer/type-list predicates assumed passed; stops before random type selection/constructor'],
  substitutions=[],entry_points={'receiver_gate':0x702952,'height':0x5F5F40})
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
 return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
