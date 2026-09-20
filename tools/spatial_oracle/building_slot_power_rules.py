"""All60 original Building ART slot power reads with real bool parser/stores."""
from pathlib import Path
import hashlib
from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_EDI, UC_X86_REG_ESP
from tools.native_oracle import run_checked, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, SP
SPECS = [(3,0,0x461a03,0x81a554),
 (3,1,0x461a20,0x81a53c),
 (3,2,0x461a3d,0x81a524),
 (3,3,0x461a5a,0x81a508),
 (4,0,0x461cb1,0x81a464),
 (4,1,0x461cce,0x81a448),
 (4,2,0x461ceb,0x81a42c),
 (4,3,0x461d08,0x81a410),
 (5,0,0x461f5f,0x81a35c),
 (5,1,0x461f7c,0x81a340),
 (5,2,0x461f99,0x81a320),
 (5,3,0x461fb6,0x81a300),
 (6,0,0x46220d,0x81a258),
 (6,1,0x46222a,0x81a23c),
 (6,2,0x462247,0x81a220),
 (6,3,0x462264,0x81a200),
 (14,0,0x4624bb,0x81a17c),
 (14,1,0x4624d8,0x81a164),
 (14,2,0x4624f5,0x81a14c),
 (14,3,0x462512,0x81a134),
 (15,0,0x462769,0x81a09c),
 (15,1,0x462786,0x81a080),
 (15,2,0x4627a3,0x81a064),
 (15,3,0x4627c0,0x81a048),
 (16,0,0x462a17,0x819fa0),
 (16,1,0x462a34,0x819f84),
 (16,2,0x462a51,0x819f68),
 (16,3,0x462a6e,0x819f48),
 (17,0,0x462cc5,0x819ea4),
 (17,1,0x462ce2,0x819e88),
 (17,2,0x462cff,0x819e6c),
 (17,3,0x462d1c,0x819e50),
 (10,0,0x462f73,0x819dbc),
 (10,1,0x462f90,0x819da4),
 (10,2,0x462fad,0x819d88),
 (10,3,0x462fca,0x819d6c),
 (11,0,0x463221,0x819cc4),
 (11,1,0x46323e,0x819ca8),
 (11,2,0x46325b,0x819c8c),
 (11,3,0x463278,0x819c6c),
 (12,0,0x4634cf,0x819bb4),
 (12,1,0x4634ec,0x819b94),
 (12,2,0x463509,0x819b74),
 (12,3,0x463526,0x819b54),
 (13,0,0x46377d,0x819aa0),
 (13,1,0x46379a,0x819a84),
 (13,2,0x4637b7,0x819a64),
 (13,3,0x4637d4,0x819a44),
 (19,0,0x463a2b,0x8199cc),
 (19,1,0x463a48,0x8199b4),
 (19,2,0x463a65,0x81999c),
 (19,3,0x463a82,0x819984),
 (20,0,0x463cd9,0x8198e0),
 (20,1,0x463cf6,0x8198c4),
 (20,2,0x463d13,0x8198a8),
 (20,3,0x463d30,0x81988c),
 (18,0,0x4641bd,0x819784),
 (18,1,0x4641da,0x81976c),
 (18,2,0x4641f7,0x819754),
 (18,3,0x464214,0x81973c)]

def generate():
 f=Fixture();rows=[]
 for slot,flag,store,key in SPECS:
  offset=0xF8C+slot*0x44+flag
  name=bytes(f.u.mem_read(key,64)).split(b'\0')[0].decode('ascii')
  for initial in (False,True):
   for raw in (None,'','yes','no','true','false','1','0','junk'):
    f.ini(key,raw)
    f.u.mem_write(TYPE+offset,bytes((int(initial),)))
    f.u.reg_write(UC_X86_REG_EBP,TYPE);f.u.reg_write(UC_X86_REG_EDI,TYPE+0x1F8)
    f.u.reg_write(UC_X86_REG_ESP,SP)
    run_checked(f.u,store-0x17,store+6)
    rows.append(dict(slot=slot,flag=flag,name=name,raw=raw,initial=initial,
      output=bool(f.u.mem_read(TYPE+offset,1)[0])))
 return rows

def metadata():
 f=Fixture()
 p=provenance(scope='60 BuildingType ART slot power fields original ReadBool and store',
  assumptions=['Default Powered=true, other3=false from original ctor45E3BE..45E416',
   'Supplied INI indexes use original CRC; all reads execute native5295F0 and original caller loads/stores',
   'Slots0..2,7..9 have no ReadINI power stores; retain constructor defaults'],
  substitutions=[],entry_points={'bool_reader':0x5295F0})
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
   hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest())
  for a,b in [(0x45E3BE,0x45E416)]+[(s-0x17,s+6) for _,_,s,_ in SPECS]]
 return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
