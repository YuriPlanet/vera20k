"""Original ObjectGetCell5F6960 and both physical-coordinate Map lookups.

Supplied object XYZ/marked byte, sparse cells and retained shared-dummy land/flags.
No virtual receiver, lifecycle or full FireError claim. Both native bodies execute.
"""
import hashlib,struct
from pathlib import Path
from unicorn import Uc,UC_ARCH_X86,UC_MODE_32,UC_HOOK_CODE,UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EIP
from tools.native_oracle import SCRATCH,STACK_BASE,STACK_SIZE,RET_MAGIC,load_image,run_checked,finish_vectors,provenance
from tools.spatial_oracle.map_queries import TABLE,EMPTY_TABLE,DUMMY,dwords,packed
ACTOR,CELL_A,CELL_B=SCRATCH+0x1000,SCRATCH+0x3000,SCRATCH+0x3200
SP=STACK_BASE+STACK_SIZE-0x1000

def execute(case):
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u);u.mem_map(STACK_BASE,STACK_SIZE);u.mem_map(SCRATCH,0x10000)
 u.mem_write(TABLE,EMPTY_TABLE);u.mem_write(0x87F924,dwords(TABLE,0x40000));ids={CELL_A:'cell_a',CELL_B:'cell_b',DUMMY:'dummy'}
 for ptr,row in zip((CELL_A,CELL_B),case['mapped_cells']):
  x,y=row['xy'];u.mem_write(TABLE+(y*512+x)*4,dwords(ptr));u.mem_write(ptr+0x24,packed(x,y));u.mem_write(ptr+0xEC,dwords(row['land']));u.mem_write(ptr+0x140,dwords(row['flags']))
 u.mem_write(DUMMY+0x24,packed(*case['dummy']['xy']));u.mem_write(DUMMY+0xEC,dwords(case['dummy']['land']));u.mem_write(DUMMY+0x140,dwords(case['dummy']['flags']))
 u.mem_write(ACTOR+0x9C,dwords(*case['xyz']));u.mem_write(ACTOR+0x74,bytes((case['marked'],)));u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,ACTOR)
 queries=[];writes=[];original=[bytes(u.mem_read(a,b-a))for a,b in ((0x5F6960,0x5F69B7),(0x565730,0x565798))]
 def observe(uc,pc,size,data):
  if pc==0x565730:
   p=int.from_bytes(u.mem_read(u.reg_read(UC_X86_REG_ESP)+4,4),'little');queries.append(dict(xyz=list(struct.unpack('<3i',u.mem_read(p,12)))))
  elif pc in (0x5F698F,0x5F69B2):queries[-1]['identity']=ids[u.reg_read(UC_X86_REG_EAX)]
  elif pc in (0x65C7E0,0x65C780,0x68BCB0):raise AssertionError(hex(pc))
 def written(uc,access,address,size,value,data):
  if STACK_BASE<=address<STACK_BASE+STACK_SIZE:return
  assert(address,size)==(DUMMY+0x24,4),(hex(address),size)
  writes.append(dict(xy=list(struct.unpack('<2h',dwords(value))),pc=f'{u.reg_read(UC_X86_REG_EIP):08x}'))
 h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,written)
 try:run_checked(u,0x5F6960,RET_MAGIC,count=1000,required_addresses=(0x5F698A,0x5F69AD,0x565730))
 finally:u.hook_del(h);u.hook_del(w)
 p=u.reg_read(UC_X86_REG_EAX);assert len(queries)==2 and u.reg_read(UC_X86_REG_ESP)==SP+4
 assert all(bytes(u.mem_read(a,b-a))==v for(a,b),v in zip(((0x5F6960,0x5F69B7),(0x565730,0x565798)),original))
 return dict(input=case,identity=ids[p],land=int.from_bytes(u.mem_read(p+0xEC,4),'little'),flags=int.from_bytes(u.mem_read(p+0x140,4),'little'),dummy_xy=list(struct.unpack('<2h',u.mem_read(DUMMY+0x24,4))),queries=queries,writes=writes)

def generate():
 cases=[]
 for name,xyz,mapped in (
  ('physical_real',[2688,2688,624],[[10,10],[11,10]]),('negative_fraction',[ -1,-255,0],[[0,0],[1,0]]),
  ('wrapped_alias',[-256,256,0],[[511,0],[0,1]]),('missing',[10496,10496,624],[[10,10],[11,10]]),
  ('signed_extreme',[-2147483648,2147483647,0],[[10,10],[11,10]])):
  for marked in (0,1):
   cases.append(dict(name=f'{name}_marked{marked}',xyz=xyz,marked=marked,mapped_cells=[dict(xy=xy,land=l,flags=f)for xy,l,f in zip(mapped,(7,2),(0x100,0x400))],dummy=dict(xy=[111,-222],land=2,flags=0x500)))
 return dict(rows=[execute(c)for c in cases])

def metadata():return provenance(scope=__doc__,entry_points={'object_get_cell':0x5F6960,'map_get_cell':0x565730},assumptions=['Ten controls: real/negativefraction/wrappedalias/missing/signedextreme with marked0/1. ObjectGetCell ignores marked and reads physicalXYZ twice.','Supplied sparse fixed512stride cells and retained dummyLand2/rawflags500. Neither map nor object lifecycle initialized. Read-only hooks; only allowed nonstackwrite is dummy coordinate.'],substitutions=[])
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
