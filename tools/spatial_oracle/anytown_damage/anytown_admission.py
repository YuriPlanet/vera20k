"""Native ordinary bridge admission joined to physical Anytown driver/Recalc/repair.
MTNK weapon selection and selected rules readers execute on physical caches.
Interior area frame, enabled Scenario gate and isolated seed0 are supplied;
attempt counts are not a native shot count or full scenario comparison.
"""
import struct
from unicorn.x86_const import *
from tools.native_oracle import run_checked,provenance,STACK_BASE,STACK_SIZE
from tools.projectile_oracle.bridge_render_inputs import BulletReader,lexical
from tools.spatial_oracle.building_body_rules import SP,RULES
from .anytown_resident import Resident,rules,identity,ri,sr,HERE,inputs

def combat_inputs():
 m=BulletReader({},root=ri.ASSETS);u=m.u
 for a in (0x887568,0xA8EB00):u.mem_write(a,sr.dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
 techno=m.alloc(0xE00);m.invoke(0x710AF0,techno,(m.cstring('MTNK'),));rules=m.alloc(0x2000)
 u.reg_write(UC_X86_REG_ESI,rules);run_checked(u,0x6675DA,0x6675E4)
 assert m.string(0x81859C)=='Damage'
 initial_strength=m.read32(rules+0x1740);rows=[]
 for name,path in [('RULESMD.INI',ri.ASSETS/'RULESMD.INI'),('LANGRULE.INI',ri.ASSETS/'LANGRULE.INI'),('MPBattleMD.ini',ri.ASSETS/'MPBattleMD.ini'),('XMP03T4.MAP',identity.ASSETS/'XMP03T4.MAP')]:
  if not path.exists():assert name=='LANGRULE.INI';rows.append(dict(file=name,absent=True));continue
  raw=path.read_bytes();sections,lines=lexical(raw,{'MTNK','105mm','Cannon','AP','CombatDamage','SpecialFlags','Basic'});m.rules_cache(sections)
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,techno);u.reg_write(UC_X86_REG_EBX,techno+0x24);u.reg_write(UC_X86_REG_ESI,RULES)
  run_checked(u,0x7129AB,0x712A1D);assert u.reg_read(UC_X86_REG_ESP)==SP
  weapon=m.read32(techno+0x898);assert weapon
  weapon_admitted=bool(m.invoke(0x772080,weapon,(RULES,))&255);warhead=m.read32(weapon+0xAC)
  whname=m.string(warhead+0x24);wall_admitted=bool(m.invoke(0x526810,RULES,(m.cstring(whname),)))
  if wall_admitted:
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,warhead);u.reg_write(UC_X86_REG_EDI,RULES);u.reg_write(UC_X86_REG_EBP,warhead+0x24)
   run_checked(u,0x75D4F4,0x75D50E);assert u.reg_read(UC_X86_REG_ESP)==SP
  if m.invoke(0x526810,RULES,(m.cstring('CombatDamage'),)):
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,rules);u.reg_write(UC_X86_REG_EDI,RULES)
   run_checked(u,0x66CD66,0x66CD8C);assert u.reg_read(UC_X86_REG_ESP)==SP
  result=dict(primary=m.string(weapon+0x24),secondary=m.read32(techno+0x8B4),projectile=m.string(m.read32(weapon+0xA0)+0x24),warhead=whname,damage=struct.unpack('<i',u.mem_read(weapon+0xA4,4))[0],wall=u.mem_read(warhead+0x144,1)[0],bridge_strength=struct.unpack('<i',u.mem_read(rules+0x1740,4))[0])
  rows.append(dict(file=name,sha256=sr.sha(raw),weapon_admitted=weapon_admitted,wall_admitted=wall_admitted,result=result,source_lines=lines))
 return dict(constructor_bridge_strength=initial_strength,layers=rows,result=result,boundary='Actual710AF0 ctor,7129AB primary block,full772080 weapon reader,AP Wall block75D4F4 andBridgeStrength constructor/read. Techno admission prefix, empty ART/sound cache, other AP fields and Scenario0x8000 are outside this input proof.')

class Admission(Resident):
 def __init__(self,*args):
  self.area_active=False;self.range_return=None;self.raw_draws=0
  super().__init__(*args)
 def observe(self,u,address,size,data):
  if self.phase=='measure' and self.area_active:
   sp=u.reg_read(UC_X86_REG_ESP)
   if address==0x65C7E0:
    self.range_return=sr.u32(u,sp);self.range_row=dict(kind='bridge_random',minimum=sr.i32(u,sp+4),maximum=sr.i32(u,sp+8));self.trace.append(self.range_row)
   elif address==0x65C84B:self.raw_draws+=1
   elif address==self.range_return:self.range_row['result']=u.reg_read(UC_X86_REG_EAX);self.range_return=None
   if address==0x48A2B9:self.event('driver_return',value=u.reg_read(UC_X86_REG_EAX)&255)
   if address==0x70D4A0:
    self.event('detach_boundary',coord=self.coord(u.reg_read(UC_X86_REG_ECX)));self.ret();return
  super().observe(u,address,size,data)
 def run(self,combat):
  u=self.uc;p=self.ptrs[tuple(self.case['impact'])];warhead,impact=0x4600C000,0x4600D000;scenario=self.rngs['scenario']-0x218
  u.mem_write(scenario,sr.dwords(0x8000));rule=sr.u32(u,0x8871E0);u.mem_write(rule+0x1740,sr.dwords(combat['bridge_strength']));u.mem_write(rule+0xFF0,sr.dwords(0))
  u.mem_write(warhead+0x144,bytes([combat['wall']]));x,y=self.case['impact'];u.mem_write(impact,sr.dwords(x*256+128,y*256+128,416))
  initial_rng={k:sr.rng_state(u,p) for k,p in self.rngs.items()};attempts=[]
  for number in range(1,1001):
   self.trace.clear();self.raw_draws=0;before=self.snapshot(p);sp=STACK_BASE+STACK_SIZE-0x4000
   u.mem_write(sp+0x18,sr.packed(x,y));u.mem_write(sp+0x24,sr.dwords(combat['damage']));u.mem_write(sp+0x28,sr.dwords(impact));u.mem_write(sp+0x1000+0xC,sr.dwords(warhead))
   u.reg_write(UC_X86_REG_EBP,sp+0x1000);u.reg_write(UC_X86_REG_EBX,warhead);u.reg_write(UC_X86_REG_ESP,sp)
   self.area_active=True;run_checked(u,0x489E87,0x48A2C4,count=2000000,required_addresses=(0x5657A0,0x65C7E0));self.area_active=False
   assert not self.pending and self.range_return is None
   after=self.snapshot(p);attempts.append(dict(number=number,center_before=before,center_after=after,trace=list(self.trace),raw_random_draws=self.raw_draws))
   if after['overlay']==232:break
  else:raise AssertionError('bounded admission sequence did not collapse')
  collapsed_rng={k:sr.rng_state(u,p) for k,p in self.rngs.items()}
  assert initial_rng['main']==collapsed_rng['main'] and initial_rng['mapgen']==collapsed_rng['mapgen']
  self.trace.clear();u.mem_write(sr.COORD,sr.packed(*self.case['start']));self.call(0x573540,args=(sr.COORD,),count=2000000)
  final_rng={k:sr.rng_state(u,p) for k,p in self.rngs.items()};assert final_rng['scenario']==collapsed_rng['scenario'];assert final_rng['main']==initial_rng['main']
  assert sr.sha(bytes(u.mem_read(0x401000,0x3E0000)))==self.code_hash
  return dict(attempts=attempts,repair_trace=list(self.trace),rng_initial=initial_rng,rng_collapsed=collapsed_rng,rng_final=final_rng,span_final=[self.snapshot(self.ptrs[x,y]) for y in range(51,58) for x in range(86,89)])

def generate():
 t=identity.theater();r,fields=rules();combat=combat_inputs();case=inputs(t);m=Admission(case,r,t);result=m.run(combat['result'])
 return dict(scope=__doc__,input=case,combat_inputs=combat,rules=fields,assets=m.assets,zone_storage=m.zone_storage,initial_recalc=m.initial_recalc,text_sha256=m.code_hash,result=result,excluded='Starts489E87 after area receivers/Rocker; normal damage argument supplied equal native-read ordinary105mm Damage, no veteran/house modifiers. Scenario gate0x8000 and all seed0 streams supplied. No full firing/projectile timing/cluster/scatter or native map load. Actual damage driver/Recalc/empty residents run; Detach70D4A0, graphs56C510/586990 and presentation are recorded boundaries. Attempts are not actual ordinary-shot counts.')

from .publication import finish_vectors

if __name__=='__main__':
 finish_vectors(generate,HERE/'anytown_admission.json.gz',provenance=lambda:provenance(scope=__doc__,assumptions=['Physical Anytown topology and resident bindings from prior packet; actual57CCF0/57D530/57ED00/Recalc/empty487A10 execute inside original area bridge blocks.','Native MTNK→105mm→Cannon/AP and Damage/Wall/BridgeStrength readers run in physical RULESMD, absentLANGRULE,MPBattleMD,Anytown layers.','Supplied already-reached489E87 stack frame, enabled Scenario0x8000 and independent original-seeded0 states. Repeat continuation until one interior collapse, then actual573540 repair.'],substitutions=['Native pristine TMP type heads and initial zone storage supplied; original Recalc derives selected rows.','Detach70D4A0 and graphs56C510/586990 are recording boundaries; zero display extents/radar/screen sinks and bounded heap.','No full scenario/type load, projectile flight, cluster/scatter, receiver damage, actor lifetime or production match claim.'],entry_points={'area':0x489E87,'area_stop':0x48A2C4,'damage':0x57CCF0,'random':0x65C7E0,'detach_boundary':0x70D4A0,'repair':0x573540,'weapon_reader':0x772080,'wall_reader':0x75D4F4,'strength_reader':0x66CD66}))
