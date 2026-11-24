"""Original four-slot storage total and Building refinery/SiloDamage slot policy.
Allocation/deletion callbacks are recorded boundaries, not constructor coverage.
"""
from pathlib import Path
from itertools import product
import struct,hashlib
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_ESI,UC_X86_REG_EBP,UC_X86_REG_EIP,UC_X86_REG_FPCW
from tools.native_oracle import run_checked,finish_vectors,provenance,RET_MAGIC
from tools.spatial_oracle.building_art_transition import ArtFixture
from tools.spatial_oracle.object_health import OBJ,TYPE,SP,dwords
SPANS=((0x6C9650,0x6C9693),(0x445FE4,0x446183),(0x446357,0x446366),(0x450CCB,0x450D96),(0x450DAA,0x450F9E))
def float_bits(value):return struct.unpack('<I',struct.pack('<f',value))[0]
AMOUNTS=[list(map(float_bits,v)) for v in ((0,0,0,0),(.9,.9,.9,.9),(1.5,1.5,1.5,1.5),(-1.5,.5,.5,.5),(1,0,0,0),(25,0,0,0),(50,0,0,0),(75,0,0,0),(100,0,0,0),(-25,0,0,0),(2**29,0,0,0),(2**31,0,0,0))]+[[0x7FC00000,float_bits(1),0,0],[0x7F800000,float_bits(1),0,0]]
def generate():
 f=ArtFixture();u=f.u;deletions=[]
 def clear(u,a,n,d):
  if a!=0x451E40:return
  sp=u.reg_read(UC_X86_REG_ESP);slot=f.read(sp+4);deletions.append(slot)
  u.mem_write(OBJ+0x55C+slot*4,dwords(0));u.reg_write(UC_X86_REG_EIP,f.read(sp));u.reg_write(UC_X86_REG_ESP,sp+8)
 u.hook_add(UC_HOOK_CODE,clear);original=[bytes(u.mem_read(a,b-a)) for a,b in SPANS];rows=[]
 for amounts,capacity,hp,source in product(AMOUNTS,(-2147483648,-100,-1,0,1,100,2147483647),(25,75),('initial','update','silo')):
  for old in ((-1,0,2,3) if source=='update' else (0,)):
   f.prepare(hp,100,int(hp<=50),'all');deletions.clear()
   u.mem_write(OBJ+0x33C,dwords(*amounts));u.mem_write(TYPE+0x800,dwords(capacity));u.mem_write(OBJ+0x6F0,dwords(old))
   u.mem_write(OBJ+0x55C,bytes(21*4))
   if source=='update' and old>=0:u.mem_write(OBJ+0x55C+(3+min(old,3))*4,dwords(OBJ+0x900))
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,OBJ);u.reg_write(UC_X86_REG_EBP,OBJ);u.reg_write(UC_X86_REG_ECX,TYPE);u.reg_write(UC_X86_REG_FPCW,0xE7F)
   entry,end={'initial':(0x445FE4,0x446366),'update':(0x450DAA,0x450F9E),'silo':(0x450CCB,0x450D96)}[source]
   fault=None
   try:run_checked(u,entry,end,count=5000)
   except Exception:
    fault=u.reg_read(UC_X86_REG_EIP)
    if fault not in (0x446010,0x450DD6,0x450E38):raise
   slot10=f.read(OBJ+0x584)
   rows.append(dict(input=dict(amount_bits=[f'{x:08X}' for x in amounts],capacity=capacity,current=hp,source=source,old_tier=old),
    output=dict(fault=None if fault is None else f'{fault:08X}',tier=struct.unpack('<i',u.mem_read(OBJ+0x6F0,4))[0],deletions=list(deletions),replacements=list(f.replacements),silo_frame=f.read(slot10+0xAC) if slot10 else None)))
 assert original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 return dict(schema_version=1,rows=rows)
def metadata():
 f=ArtFixture();p=provenance(scope='Original storage total + initial/refinery update/SiloDamage policy',
 assumptions=['Raw4float32 storage slots, signed capacity, HP25or75/Strength100/yellow.5; allnormal/damaged namespresent.',
 'Update supplied retained6F0 tiers-1,0,2,3; original reached divide faults recorded, zero total skips divide.',
 'Storage numeric owner6C9650 andftol execute unmodified; policy caller slices entered after type admission.'],
 substitutions=['451890 records selection and supplies pointer;451E40 records deletion and clears pointer. Actual Anim lifetime covered separately by building_slot_replacement corpus.'],
 entry_points={'initial':0x445FE4,'update':0x450DAA,'silo':0x450CCB,'total':0x6C9650})
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
 return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
