"""Original Building power-loss/restore slot policy, pause bytes and scalar deletes.
Allocation is an explicit selection boundary, matching building_art_transition.
Old Anims use original scalar destructors; operator_delete records arena release.
"""
from itertools import product
from pathlib import Path
import hashlib
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_ECX
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.object_health import OBJ, TYPE, SP, dwords
from tools.spatial_oracle.building_art_transition import ArtFixture
from tools.spatial_oracle.building_slot_replacement import VECTORS
ARENA = SCRATCH+0x10000
SPANS=((0x4545D0,0x454721),(0x4547C0,0x4549A5),(0x451E40,0x451E9C),
       (0x425260,0x425278),(0x426590,0x4265AC),(0x4228E0,0x422B1D))

def generate():
 f=ArtFixture();u=f.u;u.mem_map(ARENA,0x10000)
 deleted=[]
 def observe(u,a,n,d):
  if a==0x7C8B3D:
   sp=u.reg_read(UC_X86_REG_ESP);deleted.append(f.read(sp+4))
   u.reg_write(UC_X86_REG_EIP,f.read(sp));u.reg_write(UC_X86_REG_ESP,sp+4)
 u.hook_add(UC_HOOK_CODE,observe)
 original=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 cases=[]
 for restoring,slot,flags,occupied,replay in product((False,True),range(21),range(8),(False,True),(False,True)):
  cases.append(dict(restoring=restoring,slot=slot,flags=flags,occupied=occupied,replay=replay,
    placed=True,delayed=False,current=75,strength=100,active_powered=False))
 for restoring,slot,flags,placed,delayed,current,strength,active_powered in product(
     (False,True),(10,16,20),(2,4),(False,True),(False,True), (25,75),(100,0),(False,True)):
  cases.append(dict(restoring=restoring,slot=slot,flags=flags,occupied=True,replay=True,
    placed=placed,delayed=delayed,current=current,strength=strength,active_powered=active_powered))
 rows=[]
 for i in cases:
  slot=i['slot'];f.prepare(i['current'],i['strength'],0,'all');deleted.clear()
  for a,vt,_ in VECTORS:
   u.mem_write(a,bytes(24));u.mem_write(a,dwords(vt))
  u.mem_write(OBJ+0x55C,bytes(21*4));u.mem_write(OBJ+0x5B0,bytes(21))
  u.mem_write(OBJ+0x6E4,bytes((int(i['placed']),)))
  u.mem_write(TYPE+0x16A7,bytes((int(i['delayed']),)))
  # Each branch sees exactly one authored power record plus Active3's fallback flag.
  for n in range(21):u.mem_write(TYPE+0xF8C+n*0x44,bytes(4))
  u.mem_write(TYPE+0x1058,bytes((int(i['active_powered']),)))
  u.mem_write(TYPE+0xF8C+slot*0x44,bytes((int(bool(i['flags']&1)),int(bool(i['flags']&2)),int(bool(i['flags']&4)),0)))
  u.mem_write(OBJ+0x5B0+slot,bytes((int(i['replay']),)))
  pointer=ARENA+slot*0x200;u.mem_write(pointer,bytes(0x1C8));u.mem_write(pointer,dwords(0x7E3354))
  u.mem_write(pointer+0x19E,bytes((int(i['restoring']),)))
  if i['occupied']:u.mem_write(OBJ+0x55C+slot*4,dwords(pointer))
  u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ECX,OBJ)
  run_checked(u,0x4545D0 if i['restoring'] else 0x4547C0,RET_MAGIC,count=12000)
  rows.append(dict(input=i,output=dict(replacements=list(f.replacements),
   occupied=[f.read(OBJ+0x55C+n*4)!=0 for n in range(21)],
   replay=[int(u.mem_read(OBJ+0x5B0+n,1)[0]) for n in range(21)],
   paused=int(u.mem_read(pointer+0x19E,1)[0]),deleted=[(a-ARENA)//0x200 for a in deleted])))
 assert original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 return rows

def metadata():
 f=ArtFixture()
 p=provenance(scope='Original Building4545D0 restoration and4547C0 power-loss 21-slot policy',
  assumptions=['21 slot positions; all combinations Powered/Light/Effect precedence, pointer/replay presence',
   'Additional slot10/16/20 delayed-fire/placement/ActivePowered and signed health/zeroStrength cases',
   'Old Anims have null type/owner and empty registry vectors; full operational-edge and PoweredSpecial outer dispatcher excluded'],
  substitutions=['451890 records selected allocation arguments and supplies pointer (no constructor or flag synchronization)',
   'operator_delete7C8B3D records arena release; original scalar destructor and pause/resume execute; no code patches'],
  entry_points={'restore':0x4545D0,'loss':0x4547C0})
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
  hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
 return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
