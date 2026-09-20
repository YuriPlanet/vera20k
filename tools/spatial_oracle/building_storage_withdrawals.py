"""Original saved Building storage transfer, sale withdrawal and House spending.
No changed code or callback hooks. Sale begins after refund and Limbo; those
caller effects are independently traced, not claimed by the numeric slice.
"""
from pathlib import Path
from itertools import product
import hashlib, struct
from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_ECX,UC_X86_REG_ESI,UC_X86_REG_EDI,UC_X86_REG_EBP,UC_X86_REG_EIP
from tools.native_oracle import SCRATCH,RET_MAGIC,OracleError,run_checked,finish_vectors,provenance
from tools.spatial_oracle.object_health import Fixture,OBJ,TYPE,SP,dwords
HOUSE=SCRATCH+0x10000
SPANS=((0x6C9650,0x6C96DD),(0x6C9740,0x6C977A),(0x6C97E0,0x6C9841),
 (0x5027F4,0x5028C7),(0x502B57,0x502B88),(0x44A232,0x44A287),
 (0x4F9610,0x4F9667),(0x4F9790,0x4F9944),(0x4F9970,0x4F9A0E))
def fb(v):return struct.unpack('<I',struct.pack('<f',v))[0]
AMOUNTS=[list(map(fb,a)) for a in ((0,0,0,0),(.9,.9,.9,.9),(1.5,2.5,0,0),(2,0,0,0),(-1,3,0,0),(-0.,1,0,0))]+[[0x7FC01234,fb(1),0,0],[0x7F800000,fb(1),0,0]]
def fixture():
 f=Fixture();f.u.mem_map(HOUSE,0x10000);return f

def prepare(f,amounts,cash,income,capacity=100,house_amounts=None):
 f.reset('building',100,100);u=f.u
 u.mem_write(HOUSE,bytes(0x10000));u.mem_write(OBJ+0x21C,dwords(HOUSE));u.mem_write(OBJ+0x33C,dwords(*amounts));u.mem_write(TYPE+0x800,dwords(capacity))
 u.mem_write(HOUSE+0x2FC,dwords(*(amounts if house_amounts is None else house_amounts)));u.mem_write(HOUSE+0x30C,dwords(cash,100));u.mem_write(HOUSE+0x78,dwords(1));u.mem_write(HOUSE+0x6C,dwords(HOUSE+0x8000));u.mem_write(HOUSE+0x8000,dwords(OBJ))
 u.mem_write(HOUSE+0x34,dwords(HOUSE+0x9000));u.mem_write(HOUSE+0x9148,dwords(income));u.mem_write(0xB0F4EC,dwords(HOUSE+0x8100))
 for k in range(4):
  u.mem_write(HOUSE+0x8100+4*k,dwords(HOUSE+0x8200+k*0x100));u.mem_write(HOUSE+0x82B8+k*0x100,dwords(20*(k+1)))

def output(f):
 u=f.u
 return dict(building_bits=[f'{x:08X}' for x in struct.unpack('<4I',u.mem_read(OBJ+0x33C,16))],
  house_bits=[f'{x:08X}' for x in struct.unpack('<4I',u.mem_read(HOUSE+0x2FC,16))],
  credits=f.read(HOUSE+0x30C),spent=f.read(HOUSE+0x2DC),harvested=f.read(HOUSE+0x54E8),capacity=f.read(HOUSE+0x310))

def generate():
 f=fixture();u=f.u;original=[bytes(u.mem_read(a,b-a)) for a,b in SPANS];rows=[]
 for amounts,cash,income,source in product(AMOUNTS,(-10,0,10,2147483647),map(fb,(-1,0,.75,1,1.1)),('sell','spend')):
  for charge in ((-10,0,1,35) if source=='spend' else (0,)):
   prepare(f,amounts,cash,income,house_amounts=([0]*4 if source=='sell' else None))
   u.mem_write(SP,dwords(RET_MAGIC,charge));u.reg_write(UC_X86_REG_ESP,SP)
   bounded_prefix=None
   if source=='spend':
    u.reg_write(UC_X86_REG_ECX,HOUSE)
    try:run_checked(u,0x4F9790,RET_MAGIC,count=20000,required_addresses=[0x4F9970])
    except OracleError as error:
     # Infinity cannot be reduced by finite1.0. Nonpositive income leaves this
     # caller in its withdrawal loop; record only this bounded original prefix.
     assert amounts[0]==0x7F800000 and income in (fb(-1),fb(0)) and str(error).startswith('Incomplete execution'),(amounts,cash,income,charge,error)
     bounded_prefix=dict(instruction_limit=20000,eip=f'{u.reg_read(UC_X86_REG_EIP):08X}')
   else:
    u.reg_write(UC_X86_REG_EBP,OBJ);run_checked(u,0x44A232,0x44A287,count=20000)
   rows.append(dict(input=dict(source=source,amount_bits=[f'{x:08X}' for x in amounts],cash=cash,income_bits=f'{income:08X}',charge=charge),bounded_prefix=bounded_prefix,output=output(f)))
 transfer=[]
 for amounts,capacity,source in product(AMOUNTS,(-2147483648,-1,0,100,2147483647),('add','remove')):
  prior=list(map(fb,(.5,-.5,3.25,-0.)))
  prepare(f,amounts,17,fb(1),capacity,prior);u.reg_write(UC_X86_REG_ESI,OBJ);u.reg_write(UC_X86_REG_EDI,HOUSE);u.reg_write(UC_X86_REG_ESP,SP);u.mem_write(SP+0x28,dwords(0))
  entry,end=(0x502B57,0x502B88) if source=='add' else (0x5027F4,0x5028C7)
  run_checked(u,entry,end,count=2000)
  transfer.append(dict(input=dict(source=source,amount_bits=[f'{x:08X}' for x in amounts],prior_house_bits=[f'{x:08X}' for x in prior],capacity=capacity),output=output(f)))
 assert original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 return dict(schema_version=1,withdrawal_rows=rows,transfer_rows=transfer)

def metadata():
 f=fixture();p=provenance(scope='Original Building saved-storage transfer/sale and full HouseSpend4F9790 numeric owner',
 assumptions=['One Building in House roster; resource values20/40/60/80; signed cash/charge, rawf32 IncomeMult, no SiloDamage redraw callback. House spends against matching raw Building/House storage.',
 'Sale slice44A232 enters AFTER refund and virtual+D4 Limbo; supplied House storage zero reflects removal; stops before virtual+F8 Destroy.',
 'Positive Infinity/nonpositive IncomeMult spend rows may hit20000instruction limit: explicitly bounded_prefix, NOT a completed result or proof of ultimate termination.',
 'House transfer slices run after class/roster admission, with prior capacity100. Removal flag0 is both actual Limbo6F6BD1 and ChangeOwner70159D callers; Building raw storage is retained.'],
 substitutions=['Data-shaped original Building, House, resource types, country and stack. No patched code, no replaced callbacks.'],
 entry_points=dict(sale=0x44A232,spend=0x4F9790,add=0x502B57,remove=0x5027F4))
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
 return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
