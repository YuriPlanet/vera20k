"""Read-only original refund/survivor wrapper evidence; no Rust parity claim."""
import hashlib,json,struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from tools.native_oracle import call,load_image,NATIVE_SHA256,SCRATCH
from tools.spatial_oracle.map_queries import dwords
OBJ=SCRATCH; HOUSE=SCRATCH+0x1000; TYPE=SCRATCH+0x7000
RULES=SCRATCH+0x9000; COUNTRY=SCRATCH+0xb000; PAD=SCRATCH+0xc000; DOCK=SCRATCH+0xd000

def query(hp,strength,kind,cost=401,survivors=False):
 writes={OBJ:dwords(0x7e3ebc),OBJ+0x520:dwords(TYPE),OBJ+0x21c:dwords(0 if kind=='null' else HOUSE),
  OBJ+0x6c:dwords(hp),OBJ+0x70:dwords(-77),TYPE:dwords(0x7e4570),TYPE+0xa0:dwords(strength),
  TYPE+0x610:dwords(cost),TYPE+0xccd:b'\1',0x8871e0:dwords(RULES),RULES+0x1738:struct.pack('<d',.5),
  RULES+0x14f8:dwords(500),RULES+2908:dwords(PAD),RULES+6120:b'\1',PAD:dwords(PAD+256,PAD+256),
  PAD+256+1004:dwords(DOCK),DOCK:dwords(0),HOUSE+52:dwords(COUNTRY),
  HOUSE+0x1ec:bytes([kind=='human']),HOUSE+0x1ed:b'\0',0xa8b238:dwords(0),
  COUNTRY+276:struct.pack('<5f',1,1,1,1,1),HOUSE+21392:struct.pack('<5f',1,1,1,1,1)}
 entry=0x451330 if survivors else 0x70ada0
 required=[entry,0x711f60,0x7c5f00]
 if survivors:required.extend([0x4513a6,0x4513ad])
 if kind=='null':required.append(0x712024)
 else:required.extend([0x711fdb,0x50b730])
 out=call(entry,ecx=OBJ,writes=writes,required_addresses=required,timeout_instr=10000,
  dumps={'actual_estimate':(OBJ+0x6c,8),'strength':(TYPE+0xa0,4)})
 return dict(input=dict(hp=hp,strength=strength,owner=kind,cost=cost),result=struct.unpack('<i',dwords(out['eax']))[0],unchanged=out['dumps'])

def repair_fragments(kind,hp,strength,estimate,step):
 from tools.native_oracle import STACK_BASE,STACK_SIZE,run_checked
 from unicorn.x86_const import UC_X86_REG_ESI,UC_X86_REG_EBP,UC_X86_REG_EDI,UC_X86_REG_ESP,UC_X86_REG_FPCW
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u);u.mem_map(SCRATCH,0x10000);u.mem_map(STACK_BASE,STACK_SIZE)
 unit=kind=='depot'
 for a,b in {OBJ:dwords(0x7f5c70 if unit else 0x7e3ebc),OBJ+(0x6c4 if unit else 0x520):dwords(TYPE),
  OBJ+0x6c:dwords(hp),OBJ+0x70:dwords(estimate),OBJ+0x6e8:b'\1',TYPE:dwords(0x7f6218 if unit else 0x7e4570),
  TYPE+0xa0:dwords(strength),0x8871e0:dwords(RULES),RULES+0x16f8:struct.pack('<d',1.)}.items():u.mem_write(a,b)
 for reg,value in [(UC_X86_REG_ESI,OBJ),(UC_X86_REG_ESP,STACK_BASE+STACK_SIZE-0x1000),
  (UC_X86_REG_FPCW,0xe7f),(UC_X86_REG_EDI,step),(UC_X86_REG_EBP,step)]:u.reg_write(reg,value&0xffffffff)
 if unit:
  run_checked(u,0x6f4d4d,0x6f4d5f,required_addresses=[0x6f4d53,0x6f4d55,0x6f4d59,0x6f4d5c])
  # Declared omission: radio callback/notification region6F4D5F..6F4DE5.
  stop=run_checked(u,0x6f4de5,(0x6f4dff,0x6f4e24),required_addresses=[0x5f5c60,0x6f4df2])
  complete=stop==0x6f4e24
 else:
  run_checked(u,0x4508a8,0x4508d7,required_addresses=[0x4508b4,0x4508b6,0x4508c6])
  complete=bytes(u.mem_read(OBJ+0x6e8,1))==b'\0'
 return dict(input=dict(kind=kind,hp=hp,strength=strength,estimate=estimate,step=step),
  actual_estimate=list(struct.unpack('<ii',u.mem_read(OBJ+0x6c,8))),complete=complete)

def generate():
 values=[-2147483648,-20,0,1,65535,65536,2147483647]
 rows=[query(hp,strength,kind) for hp in values for strength in values for kind in ['null','human','nonhuman']]
 survivors=[query(hp,strength,kind,survivors=True) for hp,strength in zip(values,reversed(values)) for kind in ['human','nonhuman']]
 for row in rows:assert row['result']==(401 if row['input']['owner']=='nonhuman' else 200)
 for row in survivors:assert row['result']==1
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
 spans=[(0x70ada0,0x70adc0),(0x711f60,0x712039),(0x50b730,0x50b753),
 (0x44a19e,0x44a1b5),(0x44a210,0x44a227),(0x451330,0x4513ce),
 (0x4508a8,0x4508d7),(0x6f4d4d,0x6f4d62),(0x6f4de5,0x6f4e24),(0x5f5339,0x5f537a)]
 byte_rows=[]
 for a,b in spans:
  data=bytes(u.mem_read(a,b-a));byte_rows.append(dict(start=hex(a),end=hex(b),hex=data.hex(),sha256=hashlib.sha256(data).hexdigest()))
 repairs=[repair_fragments(kind,hp,strength,estimate,step) for kind,step in [('building',4),('depot',8)] for hp,strength,estimate in [(70000,100000,-20),(2147483646,2147483647,2147483645),(-20,-10,-50),(-12,-10,-50),(100,100,-50),(0,0,-50),(-1,0,-50),(1,0,-50),(99,100,-50)]]
 return dict(native_sha256=NATIVE_SHA256,scope='147 complete authentic Techno refund wrapper executions and14 complete authentic Building survivor count executions. No callbacks replaced. 18 additional repair numeric-fragment rows execute original ADD and completion ranges only; depot callback/notification region6F4D5F..6F4DE5 is omitted and its mutations are not covered. Ordinary sale credit-call snippets are byte evidence only. No full sale lifecycle, human-field producer, survivor spawning, native cost admission or Rust parity claim.',
 assumptions=['Supplied signed actual/estimate/type Strength and authored type Cost401. Fields varied independently with same type and owner configuration.',
 'Authentic Building and BuildingType vtables. Native GetType, Building actual cost, owner/country cost multipliers, owner human selector, refund float spill, native ftol all execute. PC53/chop0xE7F.',
 'Cost modifiers all1; Soylent0; FreeUnit0; SeparateAircrafttrue; fixture Pad/Dock distinct; RefundPercent.5; game mode0. Owner null or nonnull +1EC true/false, +1EDfalse.',
 'Survivor fixture side0/divisor500, typeCrewed+CCDtrue, building+6E0/+6E3false. These raw fixtures do not certify game/map admission for zero or negative health/type Strength.'],
 substitutions=['Repair fragment entry state is supplied; depot callback/notification interval omitted without claiming its behavior.'],refund_rows=rows,survivor_rows=survivors,repair_fragment_rows=repairs,original_bytes=byte_rows,
 vslots={hex(a):bytes(u.mem_read(a,4)).hex() for a in [0x7e3ebc+0x2bc,0x7e3ebc+0x84,0x7e4570+0xb8,0x7e4570+0xac]})
if __name__=='__main__':
 import sys
 out=Path(__file__).with_suffix('.json');data=generate()
 if '--check' in sys.argv:assert json.loads(out.read_text())==data;print('PASS original refund147/survivor14/repairfragments18')
 else:out.write_text(json.dumps(data,indent=2)+'\n');print('WROTE original refund147/survivor14/repairfragments18')
