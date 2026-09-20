"""Original refinery smoke producer coordinates, offsets and lifetime override.
Particle constructor is a recorded boundary; original setter6301F0 executes.
"""
from pathlib import Path
from itertools import product
import hashlib,struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import SCRATCH,RET_MAGIC,run_checked,finish_vectors,provenance
from tools.spatial_oracle.object_health import Fixture,OBJ,TYPE,SP,dwords
from tools.spatial_oracle.building_body_rules import Fixture as IniFixture
SPANS=((0x459900,0x459C00),(0x459F60,0x459F89),(0x6301F0,0x6301FA),
 (0x43B110,0x43B12A),(0x45E02F,0x45E039),(0x4601CF,0x4601E9))

def generate():
 f=Fixture();u=f.u;events=[];allocated=[]
 def boundary(u,a,n,d):
  if a not in (0x7C8E17,0x62DC50):return
  sp=u.reg_read(UC_X86_REG_ESP)
  if a==0x7C8E17:
   assert f.read(sp+4)==0x100
   pointer=SCRATCH+0xC000+len(allocated)*0x200;allocated.append(pointer)
   u.mem_write(pointer,bytes(0x100));pop=0
  else:
   pointer=u.reg_read(UC_X86_REG_ECX);pop=24
   args=[f.read(sp+x) for x in (4,8,12,16,20,24)]
   coords=list(struct.unpack('<iii',u.mem_read(args[1],12)))
   target=list(struct.unpack('<iii',u.mem_read(args[4],12)))
   events.append(dict(coords=coords,target=target,attached=args[2],owner=args[3],house=args[5],pointer=pointer))
  u.reg_write(UC_X86_REG_EAX,pointer);u.reg_write(UC_X86_REG_EIP,f.read(sp));u.reg_write(UC_X86_REG_ESP,sp+4+pop)
 u.hook_add(UC_HOOK_CODE,boundary)
 originals=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 rows=[]
 patterns=[[[0,0,0]]*4,[[128,128,0]]*4,[[10,-20,30],[20,30,-40],[1,2,3],[0,0,1]],
   [[2147483647,-2147483648,2147483647]]*4,[[0,0,0],[128,128,0],[1,2,3],[4,5,6]]]
 for origin,offsets,frames,has_type in product(((0,0,0),(525,795,631),(2147483647,-2147483648,-1)),patterns,(-2147483648,-1,0,1,25,50,2147483647),(False,True)):
  f.reset('building',100,100);events.clear();allocated.clear()
  u.mem_write(SP,dwords(RET_MAGIC));run_checked(u,0x43B110,RET_MAGIC)
  u.mem_write(OBJ+0x9C,dwords(*origin));u.mem_write(TYPE+0x774,dwords(SCRATCH+0xE000 if has_type else 0))
  u.mem_write(TYPE+0x156C,dwords(frames))
  for i,v in enumerate(offsets):u.mem_write(TYPE+0x7CC+i*12,dwords(*v))
  u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,OBJ)
  run_checked(u,0x459900,RET_MAGIC,count=2000)
  output=[]
  for event in events:
   event=dict(event);event['lifetime']=f.read(event.pop('pointer')+0xEC);output.append(event)
  rows.append(dict(input=dict(origin=origin,offsets=offsets,frames=frames,has_type=has_type),output=output))
 assert originals==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 ini=IniFixture()
 reads=[dict(raw=raw,output=ini.scalar(raw,0x81ACF0,25,0x5276D0)) for raw in (None,'','25','-1','2147483648','2147483647','65536','junk')]
 return dict(rows=rows,frames_parser=reads)

def metadata():
 f=Fixture()
 p=provenance(scope='Original459900 refinery smoke ordered constructor arguments and signed lifetime override',
  assumptions=['210 producer rows:3raworigins×5offset patterns×7signedlifetimes×typepresence',
   'Original43B110 initializer supplies sentinel(128,128,0); zeroCoord staticdata remains zero',
   'Native ReadInt529? actual5276D0 parsed8RefinerySmokeFrames rows default25'],
  substitutions=['operator_new7C8E17 supplies arena; ParticleSystem ctor62DC50 records arguments and returns; original6301F0 setter executes; no codepatch'],
  entry_points={'producer':0x459900,'lifetime_setter':0x6301F0,'sentinel_initializer':0x43B110})
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
 return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
