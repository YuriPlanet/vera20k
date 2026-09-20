"""Original ART SiloDamage bool reader and constructor default."""
from pathlib import Path
import hashlib
from unicorn.x86_const import UC_X86_REG_EBP,UC_X86_REG_EDI,UC_X86_REG_ESP
from tools.native_oracle import run_checked,finish_vectors,provenance
from tools.spatial_oracle.building_body_rules import Fixture,TYPE,SP

def generate():
 f=Fixture();rows=[]
 for raw in (None,'','yes','no','true','false','1','0','junk'):
  f.ini(0x81A780,raw);f.u.mem_write(TYPE+0x16A8,b'\0')
  f.u.reg_write(UC_X86_REG_EBP,TYPE);f.u.reg_write(UC_X86_REG_EDI,TYPE+0x1F8);f.u.reg_write(UC_X86_REG_ESP,SP)
  run_checked(f.u,0x461169,0x461186)
  rows.append(dict(raw=raw,output=bool(f.u.mem_read(TYPE+0x16A8,1)[0])))
 return rows

def metadata():
 f=Fixture();p=provenance(scope='Original Building ART SiloDamage ReadBool461169 and false constructor default45E0BA',
 assumptions=['Fresh constructorfalse; fixed ART source, originalINI CRC/keylookup/ReadBool execute.'],substitutions=[],entry_points={'read':0x461169})
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in ((0x461169,0x461186),(0x45E0BA,0x45E0C0))]
 return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
