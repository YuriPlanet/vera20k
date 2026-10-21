"""Original Bullet animation-selection caller and full SelectAnim on physical HE.
Prepared map/object inputs isolate retained-coordinate, water and height gates.
"""
import hashlib,struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC,run_checked,finish_vectors,provenance
from tools.projectile_oracle import ifv_impact as impact
from tools.spatial_oracle.building_body_rules import SP,INI,dwords
from tools.rmg_oracle.gen_rng_vectors import seeded_struct

def setup():
 m,source,st,w,cells,initial=impact.prepare();u=m.u;r=m.read32(0x8871e0);wh=m.read32(w+0xac)
 u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EBX,0);u.reg_write(UC_X86_REG_EDI,10);run_checked(u,0x6665d5,0x666604)
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=impact.assets_root()/name
  if not path.exists():continue
  sections,_=impact.lexical(path.read_bytes(),{'CombatDamage'});m.rules_cache(sections)
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EDI,impact.launch.RULES);run_checked(u,0x66c184,0x66c287)
 b=m.alloc(0x180);m.invoke(0x466380,b);m.invoke(0x4664c0,b,(m.read32(w+0xa0),cells[10,20],source,25,wh,m.read32(w+0xa8),0))
 building=m.alloc(0x800);bt=st;u.mem_write(building,dwords(0x7f5c70));u.mem_write(building+0x6c4,dwords(bt))
 # Native Object deck-height constant, supplied separately for OnBridge controls.
 u.mem_write(0xac13bc,dwords(416));u.mem_write(0x89e870,dwords(104))
 initial.update(conventional=bool(u.mem_read(wh+0x14d,1)[0]),em_effect=bool(u.mem_read(wh+0x154,1)[0]),height_gate_unit=impact.i32(u,0x89de70),level_height=impact.i32(u,0x89e870),anim_list=[m.string(m.read32(m.read32(wh+0x108)+4*i)+0x24) for i in range(m.read32(wh+0x114))],splash_list=[m.string(m.read32(m.read32(r+0xbc4)+4*i)+0x24) for i in range(m.read32(r+0xbd0))])
 return m,b,wh,cells[10,20],building,bt,initial

def generate():
 m,b,wh,c,building,bt,initial=setup();u=m.u;r=m.read32(0x8871e0);fresh_wh=bytes(u.mem_read(wh,0x180));rows=[]
 specs=[]
 for land in (0,2):
  for damage in (0,1,24,25,26,34,35,69,70,104,105,199,200,2147483647):
   specs.append(dict(name=f'select_land{land}_damage{damage}',route='selector',land=land,damage=damage))
 for flags in (0,256):
  for height in (-1,0,207,208,209):
   specs.append(dict(name=f'caller_flags{flags}_height{height}',route='caller',flags=flags,z=624+height))
 for on_bridge in (False,True):
  specs.append(dict(name=f'caller_on_bridge_{on_bridge}',route='caller',z=1040,on_bridge=on_bridge))
 for gate,water_bound,direct in ((0,0,0),(1,0,0),(1,1,0),(1,0,1)):
  specs.append(dict(name=f'caller_unit_{gate}_{water_bound}_direct{direct}',route='caller',building=True,building_gate=gate,water_bound=water_bound,direct=direct))
 specs += [dict(name='caller_nonunit_building',route='caller',building=True,nonbuilding=True,building_gate=1),dict(name='selector_no_conventional',route='selector',keys={'Conventional':'no'}),dict(name='selector_em_effect',route='selector',land=0,keys={'EMEffect':'yes'}),dict(name='selector_null_warhead',route='selector',null_warhead=True)]
 for input_ in specs:
  spec=dict(route='selector',damage=25,land=2,flags=0,z=624,level=6,on_bridge=False,direct=0);spec.update(input_)
  u.mem_write(wh,fresh_wh)
  if 'keys' in spec:m.rules_cache({'HE':spec['keys']});m.invoke(0x75d3a0,wh,(impact.launch.RULES,))
  ptr=0 if spec.get('null_warhead') else wh
  u.mem_write(c+0xec,dwords(spec['land']));u.mem_write(c+0x140,dwords(spec['flags']));u.mem_write(c+0x11b,bytes((spec['level'],0)));u.mem_write(c+0xe4,dwords(building if spec.get('building') else 0));u.mem_write(building,dwords(0x7e3ebc if spec.get('nonbuilding') else 0x7f5c70));
  m.rules_cache({'FV':{'Naval':'yes' if spec.get('building_gate',0) else 'no','Underwater':'yes' if spec.get('water_bound',0) else 'no'}})
  for start,end in ((0x714a63,0x714a7d),(0x714d6d,0x714d8e)):
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,bt);u.reg_write(UC_X86_REG_EBX,bt+0x24);u.reg_write(UC_X86_REG_EDI,impact.launch.RULES);run_checked(u,start,end)
  u.mem_write(b+0x9c,dwords(2688,5248,spec['z']));u.mem_write(b+0x8c,bytes((spec['on_bridge'],)));u.mem_write(b+0x6c,dwords(spec['damage']));u.mem_write(b+0x128,dwords(ptr));u.mem_write(SP,bytes(0x200))
  seed=seeded_struct(31);rng=m.read32(0xa8b230)+0x218;u.mem_write(rng,seed);events=[];passed={}
  def observe(uc,a,n,d):
   sp=u.reg_read(UC_X86_REG_ESP)
   if a in (0x565730,0x578080):events.append(dict(pc=hex(a),query=impact.xyz(u,m.read32(sp+4))))
   if a==0x469afd:passed['object_height']=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]
   if a==0x48a4f0:passed.update(land=impact.i32(u,sp+4),position=impact.xyz(u,m.read32(sp+8)),damage=u.reg_read(UC_X86_REG_ECX))
   if a==0x65c7e0:events.append(dict(pc=hex(a),low=impact.i32(u,sp+4),high=impact.i32(u,sp+8)))
   if a in (0x65c84b,0x65c79d):events.append(dict(pc=hex(a),raw_word=u.reg_read(UC_X86_REG_ESI)))
  h=u.hook_add(UC_HOOK_CODE,observe)
  try:
   if spec['route']=='caller':
    u.mem_write(SP+0x34,dwords(spec['direct']));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,b);run_checked(u,0x469af0,0x469bd4)
   else:
    u.mem_write(SP,dwords(RET_MAGIC,spec['land'],b+0x9c));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,spec['damage']);u.reg_write(UC_X86_REG_EDX,ptr);run_checked(u,0x48a4f0,RET_MAGIC)
  finally:u.hook_del(h)
  selected=u.reg_read(UC_X86_REG_EAX)
  rows.append(dict(input=spec,conventional=bool(u.mem_read(wh+0x14d,1)[0]),em_effect=bool(u.mem_read(wh+0x154,1)[0]),unit_naval=bool(u.mem_read(bt+0xcce,1)[0]),unit_underwater=bool(u.mem_read(bt+0xd69,1)[0]),passed=passed,selected=m.string(selected+0x24) if selected else None,events=events,rng_unchanged=bytes(u.mem_read(rng,len(seed)))==seed,rng_after_sha256=hashlib.sha256(bytes(u.mem_read(rng,len(seed)))).hexdigest()))
 # Original shared fallback constructor and lookup-miss persistence controls.
 m.invoke(0x47bbf0,0xabdc50);q=m.alloc(12)
 dummy=[dict(phase='constructor',land=impact.i32(u,0xabdc50+0xec))]
 for land in (0,2,7):
  u.mem_write(0xabdc50+0xec,dwords(land));u.mem_write(q,dwords(999999,-1,0));p=m.invoke(0x565730,0x87f7e8,(q,))
  dummy.append(dict(phase='lookup_miss',supplied_land=land,is_dummy=p==0xabdc50,land=impact.i32(u,p+0xec),coord=list(struct.unpack('<2h',u.mem_read(p+0x24,4)))))
 m.invoke(0x47bbf0,0xabdc50);dummy.append(dict(phase='reconstruct',land=impact.i32(u,0xabdc50+0xec)))
 return dict(initial=initial,rows=rows,dummy_land_controls=dummy)

def metadata():
 return provenance(scope='Original physical HE SelectAnim and Bullet caller controls for damage bands, retained coordinates, height, water, bridge and building gates',assumptions=[
  'Original48A4F0 executes in every row; caller rows start469AF0 and stop469BD4 after the selector. Native ObjectHeight5F5F40, mapped Cell/floor queries and original object type getter execute. Original startup gives89DE70=104 and89E870=104.',
  'Physical HE full reader and SplashList constructor/layered reader execute. Conventional/EMEffect controls use the full original HE reader; incoming damage/XYZ/land/structural/OnBridge and caller direct-target local are explicit fixture inputs.',
  'Prepared Unit/Building vtables are original, UnitType comes from original FV constructor; original Naval714A63 and Underwater714D6D reader blocks consume explicit yes/no keys. Cell+E4 membership remains supplied; no Unit occupancy lifecycle proof. Original Object deck constant416 supplied for OnBridge.',
  'Original full Cell47BBF0 initializes fallback Land0; native565730 miss only stamps coord, preserving deliberately supplied Land2/7 controls. Reconstruct resets0; no gameplay Land mutation producer claimed.',
  'Full native Scenario RNG independently seeded31 each row; every reached draw/raw transition and final state hash retained. No animation constructor or AreaDamage executes in this isolated selection corpus; joined impact fixtures establish their ordering.',
 ],substitutions=['Inherited native reader fixture archive/allocation/CRT/TLS transport only. No selector, caller, getter or RNG instruction replaced.'],entry_points={'caller':0x469af0,'caller_after_select':0x469bd4,'select_anim':0x48a4f0,'object_height':0x5f5f40,'splash_ctor':0x6665d5,'splash_reader':0x66c184,'warhead_reader':0x75d3a0,'random_ranged':0x65c7e0})

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
