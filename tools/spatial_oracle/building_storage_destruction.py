"""Original Building storage-destruction loop, RNG and coordinate scatter.
Resource Cell callback487190 is recorded; terrain mutations are not covered.
"""
from pathlib import Path
from itertools import product
import struct,hashlib
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EIP,UC_X86_REG_ESP,UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_ESI,UC_X86_REG_EBX
from tools.native_oracle import RET_MAGIC,OracleError,run_checked,finish_vectors,provenance
from tools.spatial_oracle.object_health import OBJ,SP,dwords
from tools.spatial_oracle.building_storage_withdrawals import fixture,prepare,output,AMOUNTS,HOUSE,fb
SCENARIO=HOUSE+0xA000
SPANS=((0x441B30,0x441C0C),(0x49F420,0x49F541),(0x6C9650,0x6C96DD),(0x6C9820,0x6C9841),(0x565730,0x5657A0),(0x65C6D0,0x65C90B))

def generate():
 f=fixture();u=f.u;events=[];target=None
 original=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 def observe(u,a,n,d):
  nonlocal target
  sp=u.reg_read(UC_X86_REG_ESP)
  if a==0x441B9F:events.append(dict(distance=u.reg_read(UC_X86_REG_EAX)))
  if a==0x565730:
   target=list(struct.unpack('<iii',u.mem_read(f.read(sp+4),12)))
  if a==0x487190:
   events[-1].update(coordinate=target,resource=f.read(sp+4),amount=f.read(sp+8),cell=list(struct.unpack('<hh',u.mem_read(u.reg_read(UC_X86_REG_ECX)+0x24,4))))
   u.reg_write(UC_X86_REG_EAX,1);u.reg_write(UC_X86_REG_EIP,f.read(sp));u.reg_write(UC_X86_REG_ESP,sp+12)
 u.hook_add(UC_HOOK_CODE,observe);rows=[]
 for amounts,seed,position,matching in product(AMOUNTS,(0,1,31,1337),((2688,2688,0),(-1,-1,104),(131071,131071,-10)),(False,True)):
  prepare(f,amounts,0,fb(1),house_amounts=amounts if matching else [fb(10)]*4)
  u.mem_write(OBJ+0x9C,dwords(*position));u.mem_write(0xA8B230,dwords(SCENARIO));u.mem_write(0xC00000,bytes(0x100000));u.mem_write(0x87F924,dwords(0xC00000));u.mem_write(0x87F928,dwords(0x40000))
  u.mem_write(SP,dwords(RET_MAGIC,seed));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,SCENARIO+0x218);run_checked(u,0x65C6D0,RET_MAGIC)
  events.clear();u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,OBJ);u.reg_write(UC_X86_REG_EBX,0)
  bounded_prefix=None
  try:run_checked(u,0x441B30,0x441C0C,count=30000)
  except OracleError as error:
   # Positive Infinity in slot0 with later1 keeps total1 and survives every
   # finite1.0 withdrawal. Preserve only the observed original loop prefix.
   assert amounts[0]==0x7F800000 and str(error).startswith('Incomplete execution'),(amounts,error)
   bounded_prefix=dict(instruction_limit=30000,eip=f'{u.reg_read(UC_X86_REG_EIP):08X}')
  rows.append(dict(input=dict(amount_bits=[f'{x:08X}' for x in amounts],seed=seed,position=position,matching_house=matching),bounded_prefix=bounded_prefix,output=dict(**output(f),events=list(events),rng_indices=[f.read(SCENARIO+0x21C),f.read(SCENARIO+0x220)])))
 assert original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 return dict(schema_version=1,rows=rows)

def metadata():
 f=fixture();p=provenance(scope='Original441B30 Building stored-resource destruction loop, native Scenario RNG and49F420 scatter',
 assumptions=['Enters after preceding explosion visuals; supplied Scenario seed describes this boundary, not whole DestructionEffects RNG.',
 'Sparse original map table has no allocated cells, so original565730 resolves its actual shared dummy; requested XYZ and dummy cell are recorded.',
 'Infinity+later1 retains original total1 and never reduces Infinity; these rows record only explicit30000instruction bounded prefixes, not completed destruction.',
 'Eight raw storage vectors, matching or surplus House slots, three edge/interior coordinates, four RNG seeds.'],
 substitutions=['Cell487190 receives original selected resource and amount1; observer records and returns1 without terrain mutation. No code bytes patched.'],entry_points=dict(destruction=0x441B30,scatter=0x49F420,random_seed=0x65C6D0))
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
 return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
