"""Original signed operational predicate and House DrainingMe assessment writer."""
from pathlib import Path
from itertools import product
import hashlib
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.object_health import OBJ, TYPE, SP, dwords
from tools.spatial_oracle.building_art_transition import ArtFixture
HOUSE=SCRATCH+0x20000
SPANS=((0x4555D0,0x4556D3),(0x4FCE30,0x4FCE73),(0x508C30,0x508D7F),(0x70FEC0,0x70FECA),(0x44E7B0,0x44E879))
def fixture():
 f=ArtFixture();f.u.mem_map(HOUSE,0x10000);return f

def generate():
 f=fixture();u=f.u;original=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 rows=[]
 for output,drain in product((-2147483648,-100,-20,-10,-1,0,1,10,20,100,2147483647),repeat=2):
  f.prepare(75,100,0,'all');u.mem_write(HOUSE,bytes(0x10000))
  u.mem_write(OBJ+0x21C,dwords(HOUSE));u.mem_write(OBJ+0x660,b'\1')
  u.mem_write(OBJ+0xAC,dwords(5));u.mem_write(OBJ+0xB4,dwords(-1))
  u.mem_write(TYPE+0xEE4,dwords(100));u.mem_write(TYPE+0x1573,b'\1')
  u.mem_write(HOUSE+0x53A4,dwords(output,drain));u.mem_write(SP,dwords(RET_MAGIC))
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,OBJ);u.reg_write(UC_X86_REG_FPCW,0xE7F)
  run_checked(u,0x4555D0,RET_MAGIC,required_addresses=[0x4FCE30])
  rows.append(dict(output=output,drain=drain,operational=bool(u.reg_read(UC_X86_REG_EAX)&255)))
 assessment=[]
 for current,draining,marked,limbo in product((-100,0,50,100),(False,True),(False,True),(False,True)):
  f.prepare(current,100,0,'all');u.mem_write(HOUSE,bytes(0x10000))
  u.mem_write(OBJ+0x660,b'\1');u.mem_write(TYPE+0xEE0,dwords(200));u.mem_write(TYPE+0xEE4,dwords(0))
  u.mem_write(OBJ+0x1D0,dwords(0x1234 if draining else 0));u.mem_write(OBJ+0x74,bytes((marked,)));u.mem_write(OBJ+0x81,bytes((limbo,)))
  u.mem_write(HOUSE+0x78,dwords(1));u.mem_write(HOUSE+0x6C,dwords(HOUSE+0x8000));u.mem_write(HOUSE+0x8000,dwords(OBJ))
  u.mem_write(HOUSE+0x2A4,dwords(-1));u.mem_write(0xA8B238,dwords(1));u.mem_write(0xA83D4C,dwords(0))
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,HOUSE);u.reg_write(UC_X86_REG_FPCW,0xE7F)
  run_checked(u,0x508C30,0x508D7F)
  assessment.append(dict(current=current,draining=draining,marked=marked,limbo=limbo,
   output=int.from_bytes(u.mem_read(HOUSE+0x53A4,4),'little',signed=True),special_outage=bool(u.mem_read(HOUSE+0x577B,1)[0])))
 assert original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 return dict(schema_version=1,predicate_rows=rows,assessment_rows=assessment)

def metadata():
 f=fixture();p=provenance(scope='Original4555D0 signed House ratio plus508C30 DrainingMe producer/force-zero output',
  assumptions=['HasPower1, no EMP/overpower/NeedsEngineer, ordinary Guard; TypePowered true and receiver drain100.',
   'House assessment one generator Power200/Strength100, no upgrades/absorb/warp; noncampaign nonlocal House admission; outage timer stopped0.'],
  substitutions=['Data-shaped original Building/type/House and stack only. No reached callback replacements or patched instructions.'],
  entry_points={'operational':0x4555D0,'assessment':0x508C30})
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
 return p
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
