"""Original43FB20 edge,4555D0 predicate,4549B0 normal/special slot dispatcher.
No code bytes changed. Anim allocation remains a named selection boundary.
"""
from itertools import product
from pathlib import Path
import hashlib
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.object_health import OBJ, TYPE, SP, dwords
from tools.spatial_oracle.building_art_transition import ArtFixture
from tools.spatial_oracle.building_slot_replacement import VECTORS
ARENA=SCRATCH+0x10000
HOUSE=SCRATCH+0x20000
SPANS=((0x43FB20,0x43FC39),(0x4549B0,0x454D04),(0x4555D0,0x4556D3))

def generate():
 f=ArtFixture();u=f.u;u.mem_map(ARENA,0x10000);u.mem_map(HOUSE,0x10000)
 deleted=[];calls=[]
 def observe(u,a,n,d):
  if a in (0x4549B0,0x4545D0,0x4547C0):calls.append(f'{a:08X}')
  if a==0x7C8B3D:
   sp=u.reg_read(UC_X86_REG_ESP);deleted.append((f.read(sp+4)-ARENA)//0x200)
   u.reg_write(UC_X86_REG_EIP,f.read(sp));u.reg_write(UC_X86_REG_ESP,sp+4)
 u.hook_add(UC_HOOK_CODE,observe)
 original=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 rows=[]
 for powered,drain,output,special,outage,current,old in product((0,1),(-2147483648,0,100),(0,200),(0,1),(0,1),(25,75),(0,1)):
  f.prepare(current,100,0,'all');deleted.clear();calls.clear()
  u.mem_write(HOUSE,bytes(0x10000));u.mem_write(OBJ+0x21C,dwords(HOUSE))
  u.mem_write(OBJ+0x660,b'\1');u.mem_write(OBJ+0x6EA,b'\1')
  u.mem_write(OBJ+0x6E4,b'\1');u.mem_write(OBJ+0x6C8,bytes((old,)))
  u.mem_write(OBJ+0xAC,dwords(5));u.mem_write(OBJ+0xB4,dwords(-1))
  u.mem_write(TYPE+0xE80,dwords(-1,-1));u.mem_write(TYPE+0xEE4,dwords(drain))
  u.mem_write(TYPE+0x1573,bytes((powered,special)))
  u.mem_write(HOUSE+0x53A4,dwords(output,100))
  u.mem_write(HOUSE+0x2A4,dwords(-1));u.mem_write(HOUSE+0x2AC,dwords(outage*10))
  for a,vt,_ in VECTORS:u.mem_write(a,bytes(24));u.mem_write(a,dwords(vt))
  u.mem_write(OBJ+0x55C,bytes(21*4))
  for slot in range(21):u.mem_write(TYPE+0xF8C+slot*0x44,bytes(4))
  u.mem_write(TYPE+0xF8C+3*0x44,b'\1');u.mem_write(TYPE+0xF8F+10*0x44,b'\1')
  for slot in (3,10):
   pointer=ARENA+slot*0x200;u.mem_write(pointer,bytes(0x1C8));u.mem_write(pointer,dwords(0x7E3354))
   u.mem_write(OBJ+0x55C+slot*4,dwords(pointer))
  u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,OBJ)
  run_checked(u,0x43FB20,0x43FC39,count=12000,required_addresses=[0x4555D0])
  rows.append(dict(input=dict(powered=bool(powered),drain=drain,output=output,special=bool(special),outage=outage*10,current=current,old=bool(old)),
   output=dict(operational=bool(u.mem_read(OBJ+0x6C8,1)[0]),calls=list(calls),replacements=list(f.replacements),deleted=list(deleted),
    paused=bool(u.mem_read(ARENA+3*0x200+0x19E,1)[0]),occupied=[f.read(OBJ+0x55C+s*4)!=0 for s in range(21)])))
 assert original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 return rows

def metadata():
 f=ArtFixture();p=provenance(scope='Original Building43FB20 operational edge through actual4555D0 and4549B0 animation power tail',
  assumptions=['HasPower1, no EMP/overpower/NeedsEngineer, ordinary Guard mission, no RobotControl/capture/temporal/gate/gap optional branches',
   'House total drain100 independently of the receiver Type drainMIN/0/100; both ordinary deficit and PoweredSpecial outage inputs',
   'Slot3 Powered, slot10 PoweredSpecial; original old Anim scalar destructor and pause/resume execute'],
  substitutions=['451890 records allocation arguments and supplies replacement pointer; operator_delete records freed arena pointers',
   'Power sounds both-1; stops43FC39 before other Building AI'],entry_points={'edge':0x43FB20,'operational':0x4555D0,'dispatcher':0x4549B0})
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
 return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
