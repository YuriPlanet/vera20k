"""Original slot frame handoff and full scalar destructor on supplied Anims.
Constructor output is supplied, and operator_delete releases a synthetic arena
allocation through an explicit recorded allocator boundary. Original destructor,
vector lookups, sound-handle teardown and replacement instructions execute.
"""
from itertools import product
from pathlib import Path
import hashlib
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import SCRATCH,run_checked,finish_vectors,provenance
from tools.spatial_oracle.building_art_transition import ArtFixture
from tools.spatial_oracle.object_health import OBJ,SP,dwords
OLD=SCRATCH+0xB000
NEW=SCRATCH+0xD000
# Exact static initializer stores, not inferred template vtables.
VECTORS=((0xB0F698,0x7E91EC,0x72586D),(0xA8E360,0x7E4F64,0x4E7AFD),
 (0xB0F720,0x7E91EC,0x7252ED),(0xB0F670,0x7E91EC,0x72536D),
 (0xB0F618,0x7E91EC,0x7253ED),(0xA8E9A8,0x7E9F24,0x4E6D7D))
SPANS=((0x4519F7,0x451A36),(0x426590,0x4265AC),(0x4228E0,0x422B1D),
       (0x5F3B80,0x5F3D84))

def generate():
 f=ArtFixture();u=f.u; events=[];current_slot=0
 originals=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 def observe(u,a,n,d):
  if a in (0x426590,0x4228E0,0x4255B0,0x7C8B3D):
   events.append(dict(address=f'{a:08X}',slot_pointer=f.read(OBJ+0x55C+current_slot*4),
       copied_frame=f.read(NEW+0xAC)))
  if a==0x7C8B3D:
   sp=u.reg_read(UC_X86_REG_ESP)
   assert f.read(sp+4)==OLD
   u.reg_write(UC_X86_REG_EIP,f.read(sp));u.reg_write(UC_X86_REG_ESP,sp+4)
 u.hook_add(UC_HOOK_CODE,observe)
 rows=[]
 for slot,old_frame,has_old,new_frame in product(range(21),(-2147483648,-1,0,17,2147483647),(False,True),(0,23)):
  f.prepare(50,100,1,'all');f.capture=False;events.clear();current_slot=slot
  for a,vt,_ in VECTORS:
   u.mem_write(a,bytes(24));u.mem_write(a,dwords(vt))
  u.mem_write(OLD,bytes(0x1C8));u.mem_write(NEW,bytes(0x1C8))
  u.mem_write(OLD,dwords(0x7E3354));u.mem_write(OLD+0xAC,dwords(old_frame))
  u.mem_write(NEW+0xAC,dwords(new_frame))
  # Different retained runtime fields prove the handoff copies only+AC.
  u.mem_write(OLD+0xB4,dwords(17,19,23));u.mem_write(NEW+0xB4,dwords(91,0,3))
  u.mem_write(OLD+0x195,b'\x08');u.mem_write(NEW+0x195,b'\xff')
  u.mem_write(OLD+0x19C,b'\0');u.mem_write(NEW+0x19C,b'\1')
  u.mem_write(OBJ+0x55C+slot*4,dwords(OLD if has_old else 0))
  u.mem_write(SP+0x3C,dwords(slot))
  for reg,value in ((UC_X86_REG_ESI,OBJ),(UC_X86_REG_EBP,NEW),(UC_X86_REG_ESP,SP)):
   u.reg_write(reg,value)
  run_checked(u,0x4519F7,0x451A36,count=4000)
  rows.append(dict(input=dict(slot=slot,old_frame=old_frame,has_old=has_old,new_frame=new_frame),
      output=dict(frame=f.read(NEW+0xAC),timer=[f.read(NEW+x) for x in (0xB4,0xB8,0xBC)],
        loop_remaining=int(u.mem_read(NEW+0x195,1)[0]),first_guard=int(u.mem_read(NEW+0x19C,1)[0]),
        installed=f.read(OBJ+0x55C+slot*4),events=list(events))))
 assert originals==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 return rows

def metadata():
 f=ArtFixture()
 p=provenance(scope='Building4519F7 frame handoff/scalar delete/full destructor through451A36 pointer install',
   assumptions=['420 rows,21slots,5 signed frames,old presence,2 supplied constructor frames',
     'Old Anim+CC/type pointer null, no Logic membership, vectors empty with exact original initializer vtables',
     'Constructor output supplied with independent timer/loop/firstAI values; constructor and nonnull type cleanup excluded'],
   substitutions=['operator_delete7C8B3D records arena release and returns; no original code patch'],
   entry_points={'replacement':0x4519F7,'scalar_deleting_destructor':0x426590,'destructor':0x4228E0})
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
   hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
 p['vector_initializers']=[dict(vector=f'{a:08X}',vtable=f'{vt:08X}',store=f'{store:08X}',
   bytes=bytes(f.u.mem_read(store,10)).hex()) for a,vt,store in VECTORS]
 return p

if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
