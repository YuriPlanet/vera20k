"""Original SiloDamage positive-level null write and existing-slot bypass."""
from pathlib import Path
from itertools import product
import struct,hashlib
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EIP,UC_X86_REG_ESI,UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_FPCW
from tools.native_oracle import SCRATCH,run_checked,finish_vectors,provenance
from tools.spatial_oracle.building_art_transition import ArtFixture
from tools.spatial_oracle.object_health import OBJ,TYPE,SP,dwords

def generate():
 f=ArtFixture();u=f.u;deletions=[]
 def clear(u,a,n,d):
  if a!=0x451E40:return
  sp=u.reg_read(UC_X86_REG_ESP);slot=f.read(sp+4);deletions.append(slot);u.mem_write(OBJ+0x55C+slot*4,dwords(0));u.reg_write(UC_X86_REG_EIP,f.read(sp));u.reg_write(UC_X86_REG_ESP,sp+8)
 u.hook_add(UC_HOOK_CODE,clear);rows=[]
 for total,hp,name,existing in product((0,25),(25,75),(False,True),(False,True)):
  f.prepare(hp,100,int(hp<=50),'all');deletions.clear()
  u.mem_write(OBJ+0x33C,struct.pack('<ffff',total,0,0,0));u.mem_write(TYPE+0x800,dwords(100));u.mem_write(OBJ+0x55C,bytes(21*4))
  if not name:
   u.mem_write(TYPE+0x11F4,b'\0');u.mem_write(TYPE+0x1204,b'\0')
  if existing:u.mem_write(OBJ+0x584,dwords(SCRATCH+0xB000));u.mem_write(SCRATCH+0xB0AC,dwords(99))
  u.reg_write(UC_X86_REG_ESI,OBJ);u.reg_write(UC_X86_REG_ECX,TYPE);u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_FPCW,0xE7F)
  fault=None
  try:run_checked(u,0x450CCB,0x450D96)
  except Exception:
   fault=u.reg_read(UC_X86_REG_EIP)
   if fault!=0x450D81:raise
  pointer=f.read(OBJ+0x584)
  rows.append(dict(input=dict(total=total,current=hp,name=name,existing=existing),output=dict(fault=None if fault is None else f'{fault:08X}',deletions=list(deletions),replacements=list(f.replacements),frame=f.read(pointer+0xAC) if pointer else None)))
 return rows

def metadata():
 f=ArtFixture();a,b=0x450CCB,0x450D96;p=provenance(scope='Original SiloDamage existing pointer,zero level and missing-name null-write boundary',assumptions=['Capacity100/Strength100/yellow.5;amounttotal0or25;selectednamepresent/empty;existingpointerpresent/absent.'],substitutions=['451890 allocation selection hook and451E40 clear callback; original null write recorded as reached fault, no code patch.'],entry_points={'silo':a})
 p['original_slice']=dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest());return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
