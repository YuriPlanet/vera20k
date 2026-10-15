"""Full native Bullet detonation: AreaDamage receipt, selection and nullify effect.

The Unit ReceiveDamage method is an explicit transport boundary. AreaDamage,
IC timer, SelectAnim, Scenario RNG and physical animation constructors execute.
"""
import hashlib,struct,os
from pathlib import Path
from collections import deque
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.projectile_oracle.ifv_select_anim import setup
from tools.projectile_oracle import ifv_impact as impact
from tools.native_oracle import NATIVE_SHA256,run_checked,finish_vectors,provenance
from tools.spatial_oracle.building_body_rules import SP,RULES,INI,dwords
from tools.rmg_oracle.gen_rng_vectors import seeded_struct

def execute(receiver_mode,em_effect):
 m,b,wh,cell,obj,typ,initial=setup();u=m.u;r=m.read32(0x8871e0);scenario=m.read32(0xa8b230)
 nullify_layers=[]
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  p=impact.assets_root()/name
  if not p.exists():continue
  sections,_=impact.lexical(p.read_bytes(),{'General'});m.rules_cache(sections)
  u.mem_write(SP-4,dwords(128));u.reg_write(UC_X86_REG_ESP,SP-4);u.reg_write(UC_X86_REG_ECX,SP+0x50);u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EDI,RULES);run_checked(u,0x66e2af,0x66e2e5)
  selected=m.read32(r+0x350);nullify_layers.append(dict(file=name,sha256=hashlib.sha256(p.read_bytes()).hexdigest(),authored=sections.get('General',{}).get('WeaponNullifyAnim'),selected=m.string(selected+0x24) if selected else None))
 nullify=m.read32(r+0x350);assert m.string(nullify+0x24)=='IRONFX'
 asset=Path(os.environ.get('VERA20K_IFV_NULLIFY_ASSETS',str(impact.assets_root())))/'ironfx.shp'
 raw=asset.read_bytes();m.assets['IRONFX.SHP']=raw
 art,_=impact.lexical((impact.assets_root()/'ARTMD.INI').read_bytes(),{'IRONFX'});m.make_ini(art);m.asset_loaded=[];admitted=m.invoke(0x427d00,nullify,(INI,));art_state=m.result('IRONFX',nullify,admitted);art_state['assets']=m.asset_loaded
 initial.update(nullify_reference_layers=nullify_layers,art_sha256=hashlib.sha256((impact.assets_root()/'ARTMD.INI').read_bytes()).hexdigest(),bullet_constructor_combat_light=bool(u.mem_read(b+0xe0,1)[0]))
 assert art_state['art_body_read'] and art_state['raw_shp_frame_count']==10
 m.rules_cache({'HE':{'EMEffect':'yes' if em_effect else 'no'}});m.invoke(0x75d3a0,wh,(RULES,))
 for y in range(16,25):
  for x in range(6,25):
   p=m.read32(0x26000000+4*(y*512+x));u.mem_write(p+0x38,dwords(-1));u.mem_write(p+0x44,dwords(-1));u.mem_write(p+0xe4,dwords(0));u.mem_write(p+0x140,dwords(0))
 u.mem_write(0xabdc50+0x38,dwords(-1));u.mem_write(0xabdc50+0x44,dwords(-1));u.mem_write(0xa8ed84,dwords(100));u.mem_write(0xa8e9a0,b'\1');u.mem_write(0x87f914,dwords(64,64))
 for start,end in ((0x40b540,0x40b5ab),(0x725850,0x725886),(0x4e6d60,0x4e6d96)):
  u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,start,end)
 live=[2688,5248,624];placement=[2688,5248,624];position=m.alloc(12);u.mem_write(position,dwords(*placement));u.mem_write(b+0x9c,dwords(*live));u.mem_write(b+0x90,b'\1')
 u.mem_write(cell+0xe4,dwords(0 if receiver_mode=='empty' else obj));u.mem_write(obj+0x14,dwords(1));u.mem_write(obj+0x30,dwords(0));u.mem_write(obj+0x6c,dwords(100));u.mem_write(obj+0x90,b'\1');u.mem_write(obj+0x74,b'\1');u.mem_write(obj+0x81,b'\0');u.mem_write(obj+0x9c,dwords(*placement));u.mem_write(obj+0x18c,dwords(100));u.mem_write(obj+0x194,dwords(10 if receiver_mode=='iron_curtain' else 0));u.mem_write(obj+0x1c4,dwords(0))
 seed=seeded_struct(31);rng=scenario+0x218;u.mem_write(rng,seed);events=[];trace=deque(maxlen=20);pending={}
 id_before=impact.i32(u,scenario+0x214)
 def rng_hash():return hashlib.sha256(bytes(u.mem_read(rng,len(seed)))).hexdigest()
 def event(kind,**kw):events.append(dict(event=kind,**kw))
 def obs(uc,a,n,d):
  trace.append(a);sp=u.reg_read(UC_X86_REG_ESP)
  if a in pending:
   kind=pending.pop(a);value=u.reg_read(UC_X86_REG_EAX);kw=dict(result=value,pc=hex(a),rng_hash=rng_hash())
   if kind=='SelectAnim':kw['selected']=m.string(value+0x24) if value else None
   if kind=='AnimCtor':kw.update(native_id=impact.i32(u,value+0x10),name=m.string(m.read32(value+0xc8)+0x24),retained=impact.xyz(u,value+0x9c),alive=u.mem_read(value+0x90,1)[0])
   event(kind+'_return',**kw)
  if a==0x737c90:event('ReceiveDamageBoundary',damage=impact.i32(u,m.read32(sp+4)),distance=impact.i32(u,sp+8),supplied_result=0);m.ret(0,28)
  if a in (0x489280,0x48a4f0,0x421ea0,0x65c7e0):
   kind={0x489280:'AreaDamage',0x48a4f0:'SelectAnim',0x421ea0:'AnimCtor',0x65c7e0:'RandomRanged'}[a];pending[m.read32(sp)]=kind;kw=dict(pc=hex(a),return_pc=hex(m.read32(sp)),rng_hash=rng_hash())
   if kind=='AreaDamage':kw.update(position=impact.xyz(u,u.reg_read(UC_X86_REG_ECX)),damage=u.reg_read(UC_X86_REG_EDX))
   if kind=='SelectAnim':kw.update(position=impact.xyz(u,m.read32(sp+8)),land=impact.i32(u,sp+4),damage=u.reg_read(UC_X86_REG_ECX))
   if kind=='AnimCtor':kw.update(name=m.string(m.read32(sp+4)+0x24),position=impact.xyz(u,m.read32(sp+8)),delay=impact.i32(u,sp+12),loop=impact.i32(u,sp+16),flags=m.read32(sp+20),z_adjust=impact.i32(u,sp+24),reverse=m.read32(sp+28))
   if kind=='RandomRanged':kw.update(low=impact.i32(u,sp+4),high=impact.i32(u,sp+8))
   event(kind,**kw)
  if a in (0x65c84b,0x65c79d):event('RawWord',pc=hex(a),word=u.reg_read(UC_X86_REG_ESI))
  if a in (0x48a620,0x46a2a1,0x5f4ec0,0x4a9720,0x424ce0):event({0x48a620:'CombatLight',0x46a2a1:'NullifyBranch',0x5f4ec0:'Unlimbo',0x4a9720:'DisplaySubmit',0x424ce0:'AnimStart'}[a],pc=hex(a))
 h=u.hook_add(UC_HOOK_CODE,obs)
 try:m.invoke(0x4690b0,b,(position,))
 except Exception:
  print('FAILED',list(map(hex,trace)),events);raise
 finally:u.hook_del(h)
 def only(name):
  found=[e for e in events if e['event']==name];assert len(found)==1,(name,found);return found[0]
 area=only('AreaDamage_return');selected=only('SelectAnim_return');ctor=only('AnimCtor');constructed=only('AnimCtor_return')
 assert events.index(area)<events.index(selected)<events.index(ctor)<events.index(constructed)
 assert selected['rng_hash']==ctor['rng_hash']==constructed['rng_hash']==rng_hash()
 assert constructed['alive']==1 and constructed['retained']==placement
 return dict(input=dict(name=f'{receiver_mode}_em_{str(em_effect).lower()}',receiver_mode=receiver_mode,em_effect=em_effect,live=live,placement=placement,binary_frame=100,ic_start=100,ic_duration=10 if receiver_mode=='iron_curtain' else 0,ic_kind=0,receiver_health=100,receiver_result_boundary=0,scenario_rng_seed=31),initial=initial,art=art_state,area_result=area['result'],selected=selected['selected'],constructed=constructed['name'],constructor=ctor,events=events,rng_before_sha256=hashlib.sha256(seed).hexdigest(),rng_after_sha256=rng_hash(),anim_count=impact.i32(u,0xa8e9b8),native_cursor_before=id_before,native_cursor_after=impact.i32(u,scenario+0x214),bullet_native_id=impact.i32(u,b+0x10),bullet_alive=bool(u.mem_read(b+0x90,1)[0]))

def generate():
 rows=[execute(mode,em) for mode in ('receiver','empty','iron_curtain') for em in (False,True)]
 initial=rows[0].pop('initial');art=rows[0].pop('art')
 for row in rows[1:]:assert row.pop('initial')==initial and row.pop('art')==art
 assert {row['area_result'] for row in rows}=={0,1,2}
 return dict(native_sha256=NATIVE_SHA256,initial=initial,nullify_art=art,rows=rows)

def metadata():
 return provenance(scope='Six full native Bullet detonation controls joining AreaDamage0/1/2, EMEffect selector RNG and ordinary versus WeaponNullifyAnim construction',assumptions=[
  'Full original4690B0 runs from entry to return, including full AreaDamage489280, original IC timer/admission/receipt, Bullet selection caller, full SelectAnim48A4F0 and full AnimCtor421EA0/Start/Unlimbo/Display submission. AreaDamage return is observed, never supplied.',
  'Physical HoverMissile/AAHeatSeeker2 and HE reader setup is inherited from ifv_select_anim. Full original HE reader consumes explicit EMEffect=no/yes controls; ordinary physical HE has EMEffect=false. General WeaponNullifyAnim reads physical ordered RULESMD, optional LANGRULE, MPBattleMD and Hills layers through original66E2AF block. This is not full Rules Process chronology.',
  'Full physical IRONFX ART and unchanged10-frame SHP load through native reader427D00; the eight ordinary HE animation types/assets come from ifv_impact.prepare. Sound registry is empty, so Report remains-1 and audio binding/playback is outside this witness.',
  'Prepared flat level6 mapped cells have no bridge/tile/overlay damage effects. Live BulletXYZ and detonation placement are both2688,5248,624. Supplied GameActive1 permits native animation admission; binary frame100, seed31, receiver health100, alive/marked/nonlimbo membership and IC start100/duration0or10/kind0 are explicit inputs. No source/receiver gameplay lifecycle or full Unit damage mutation is claimed.',
  'Bullet constructor CombatLight+E0 remains false. These controls establish selection/animation order and do not exercise the ordinary CombatLight body.',
  'Native partial construction cursor before/after is recorded, not a whole-scenario prefix. Detonate alone does not schedule Bullet retirement or subsequent AnimAI; retained Bullet alive and one newly admitted animation are bounded outputs. Existing joined impact fixtures cover their ordinary lifecycle.',
 ],substitutions=[
  'Inherited lexical CRC INI caches, exact archive byte transport, allocator/delete/CRT/TLS boundaries. Unit ReceiveDamage737C90 returns explicit0 with28bytecleanup and no health mutation; native AreaDamage independently produces its receipt. No detonation/AreaDamage/selector/RNG/animation instruction is replaced.',
 ],entry_points={'detonate':0x4690b0,'area_damage':0x489280,'receiver_boundary':0x737c90,'select_anim':0x48a4f0,'random_ranged':0x65c7e0,'nullify_branch':0x46a2a1,'anim_ctor':0x421ea0,'anim_start':0x424ce0,'object_unlimbo':0x5f4ec0,'nullify_anim_reader':0x66e2af,'anim_type_reader':0x427d00})

def brute_setup():
 """Compose physical readers and full Anim admission in the existing world."""
 from tools.rules_oracle.bridge_anim_inputs import physical_sections
 from tools.spatial_oracle.bridge_debris_producer import METALLIC
 from tools.native_oracle import initialize_empty_windows_seh
 m,old,_,cell,obj,typ,initial=setup();u=m.u;r=m.read32(0x8871e0)
 root=impact.assets_root();layers=[]
 w=m.invoke(0x772fa0,m.cstring('Smash'))
 m.initialize_vectors(r)
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=root/name
  if not path.exists():
   assert name=='LANGRULE.INI';layers.append(dict(file=name,absent=True));continue
  raw=path.read_bytes();sections,_=impact.lexical(raw,{'Smash','InvisibleLow','Smashing','General','AudioVisual'})
  m.rules_cache(sections);m.invoke(0x772080,w,(RULES,))
  p=m.read32(w+0xa0);wh=m.read32(w+0xac)
  m.invoke(0x46bee0,p,(RULES,));m.invoke(0x75d3a0,wh,(RULES,))
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EDI,RULES)
  run_checked(u,0x66b3c4,0x66b3e4)
  m.invoke(0x7729f0,w)
  pool=m.read('MetallicDebris',sections.get('General',{}).get('MetallicDebris'),r)
  selected={'Smash':('Damage','Projectile','Warhead','Range','Speed'),
   'InvisibleLow':('Inviso','Cluster','Airburst'),
   'Smashing':('MinDebris','MaxDebris','DebrisTypes','DebrisMaximums','AnimList','Verses'),
   'AudioVisual':('Gravity',)}
  keys={section:{key:sections.get(section,{}).get(key) for key in names} for section,names in selected.items()}
  layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),selected_keys=keys,pool=pool))
 assert m.state('MetallicDebris',r)['names']==METALLIC
 art_raw=(root/'ARTMD.INI').read_bytes();art=physical_sections(art_raw)
 art.update(impact.lexical(art_raw,{'IRONFX'})[0]);m.make_ini(art)
 names=METALLIC+['TWLT026','TWLT036','TWLT070','SMOKEY2','IRONFX']
 pointers={name:m.invoke(0x428b80,m.cstring(name)) for name in names}
 arts=[]
 for name,t in pointers.items():
  m.asset_loaded=[];admitted=m.invoke(0x427d00,t,(INI,))
  arts.append(dict(**m.result(name,t,admitted),assets=list(m.asset_loaded)))
 assert next(a for a in arts if a['name']=='D')['art_body_read'] is False
 # The unchanged full native nullify reader selects IRONFX from physical layers.
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=root/name
  if not path.exists():continue
  sections,_=impact.lexical(path.read_bytes(),{'General'});m.rules_cache(sections)
  u.mem_write(SP-4,dwords(128));u.reg_write(UC_X86_REG_ESP,SP-4)
  u.reg_write(UC_X86_REG_ECX,SP+0x50);u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EDI,RULES)
  run_checked(u,0x66e2af,0x66e2e5)
 initialize_empty_windows_seh(u)
 # The original Logic vector initializer (same owner as the composed shrapnel).
 def platform(_u,pc,_n,_d):
  if pc==0x7c978a:m.ret(0)
 u.hook_add(UC_HOOK_CODE,platform);m.invoke(0x40cb80,0)
 for y in range(16,25):
  for x in range(6,25):
   c=m.read32(0x26000000+4*(y*512+x))
   for off,value in ((0x38,-1),(0x44,-1),(0xe4,0),(0x140,0)):u.mem_write(c+off,dwords(value))
 u.mem_write(0xabdc50+0x38,dwords(-1));u.mem_write(0xabdc50+0x44,dwords(-1))
 u.mem_write(0xa8ed84,dwords(100));u.mem_write(0xa8e9a0,b'\1');u.mem_write(0x87f914,dwords(64,64))
 for begin,end in ((0x40b540,0x40b5ab),(0x725850,0x725886),(0x4e6d60,0x4e6d96)):
  u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,begin,end)
 b=m.alloc(0x180);m.invoke(0x466380,b)
 m.invoke(0x4664c0,b,(p,cell,0,100,wh,m.read32(w+0xa8),0));u.mem_write(b+0x130,dwords(w))
 return m,b,wh,cell,obj,typ,dict(layers=layers,art_sha256=hashlib.sha256(art_raw).hexdigest(),art=arts,
  projectile=dict(name=m.string(p+0x24),inviso=bool(u.mem_read(p+0x29e,1)[0]),cluster=impact.i32(u,p+0x2ac),airburst=bool(u.mem_read(p+0x294,1)[0])),
  warhead=dict(name=m.string(wh+0x24),minimum=impact.i32(u,wh+0x1c8),maximum=impact.i32(u,wh+0x1c4),typed_count=m.read32(wh+0x19c),anim_count=m.read32(wh+0x114)),
  metallic_pool=m.state('MetallicDebris',r)['names'])


def brute_execute(case):
 from tools.procedural_drawing_oracle.electric_bolt import RngTrace
 from tools.spatial_oracle.anim_bouncer_launch import constructor_state
 from tools.native_oracle import RET_MAGIC
 m,b,wh,cell,obj,typ,initial=brute_setup();u=m.u;scenario=m.read32(0xa8b230)
 case=dict(dict(seed=1,route='outer',receiver='empty',live=[2688,5248,624],placement=[2688,5248,624]),**case)
 if case.get('zero_max'):
  m.rules_cache({'Smashing':{'MaxDebris':'0','MinDebris':'0'}});m.invoke(0x75d3a0,wh,(RULES,))
 live=case['live'];placement=case['placement'];position=m.alloc(12);u.mem_write(position,dwords(*placement))
 u.mem_write(b+0x9c,dwords(*live));u.mem_write(b+0x90,b'\1');u.mem_write(b+0x150,dwords(256))
 if case['receiver']!='empty':
  u.mem_write(cell+0xe4,dwords(obj));u.mem_write(obj+0x14,dwords(1));u.mem_write(obj+0x30,dwords(0))
  u.mem_write(obj+0x6c,dwords(300));u.mem_write(obj+0x90,b'\1');u.mem_write(obj+0x74,b'\1');u.mem_write(obj+0x81,b'\0')
  u.mem_write(obj+0x9c,dwords(*placement));u.mem_write(obj+0x18c,dwords(100));u.mem_write(obj+0x194,dwords(10 if case['receiver']=='iron_curtain' else 0));u.mem_write(obj+0x1c4,dwords(0))
 case.update(minimum=impact.i32(u,wh+0x1c8),maximum=impact.i32(u,wh+0x1c4),inviso=initial['projectile']['inviso'],cluster=initial['projectile']['cluster'],binary_frame=100,damage=100,scenario_rng_seed=case['seed']+1,ic_start=100,ic_duration=10 if case['receiver']=='iron_curtain' else 0,ic_kind=0)
 rng=RngTrace(u,scenario,case['seed']);before=rng.states();cursor=impact.i32(u,scenario+0x214)
 events=[];pending=[];anims=[];trace=deque(maxlen=24);raw_advances=[]
 kinds={0x468d80:'Impact',0x4690b0:'Detonate',0x489280:'AreaDamage',0x48a4f0:'SelectAnim',0x421ea0:'AnimCtor',0x424ce0:'AnimStart',0x5f4ec0:'Unlimbo',0x4a9720:'DisplaySubmit',0x4397e0:'BounceInit',0x46a2a1:'NullifyBranch',0x49f420:'ClusterCoord',0x469e50:'DebrisGetCoords',0x68bcb0:'NextNativeId',0x469038:'SuccessorAliveGate'}
 def event(kind,**kw):events.append(dict(event=kind,rng_request_count=len(rng.requests),native_cursor=impact.i32(u,scenario+0x214),**kw))
 def observe(_u,pc,_n,_d):
  trace.append(pc);sp=u.reg_read(UC_X86_REG_ESP)
  if pc in (0x65c788,0x65c837):raw_advances.append(f'{pc:08X}')
  if pending and (pc,sp)==pending[-1][:2]:
   _,_,kind,ptr=pending.pop();result=u.reg_read(UC_X86_REG_EAX)
   if kind=='AnimCtor':
    items=m.read32(0x87f77c);members=[m.read32(items+4*i) for i in range(m.read32(0x87f788))]
    row=dict(type=m.string(m.read32(ptr+0xc8)+0x24),logic_registered=ptr in members,**constructor_state(u,ptr));anims.append(row);event('AnimCtor_return',anim_index=len(anims)-1,native_id=row['native_id'],rng_scenario=rng.states()['scenario'])
   elif kind=='ClusterCoord':event(kind+'_return',coord=impact.xyz(u,result))
   else:event(kind+'_return',result=result,selected=m.string(result+0x24) if kind=='SelectAnim' and result else None)
  if pc==0x737c90:event('ReceiveDamageBoundary',damage=impact.i32(u,m.read32(sp+4)),supplied_result=0);m.ret(0,28);return
  if pc not in kinds:return
  kind=kinds[pc];kw=dict(pc=f'{pc:08X}')
  if kind=='Detonate':kw['placement']=impact.xyz(u,m.read32(sp+4))
  if kind=='AreaDamage':kw.update(position=impact.xyz(u,u.reg_read(UC_X86_REG_ECX)),damage=u.reg_read(UC_X86_REG_EDX))
  if kind=='SelectAnim':kw.update(position=impact.xyz(u,m.read32(sp+8)),damage=u.reg_read(UC_X86_REG_ECX),land=impact.i32(u,sp+4))
  if kind=='AnimCtor':kw.update(type=m.string(m.read32(sp+4)+0x24),position=impact.xyz(u,m.read32(sp+8)),delay=impact.i32(u,sp+12),loops=impact.i32(u,sp+16),flags=m.read32(sp+20),z_adjust=impact.i32(u,sp+24),reverse=m.read32(sp+28),rng_scenario=rng.states()['scenario'])
  if kind=='DebrisGetCoords':kw['live_bullet']=impact.xyz(u,b+0x9c)
  if kind=='ClusterCoord':kw['distance']=impact.i32(u,sp+4)
  if kind=='SuccessorAliveGate':kw['bullet_alive']=bool(u.mem_read(b+0x90,1)[0])
  event(kind,**kw)
  if kind in ('AreaDamage','SelectAnim','AnimCtor','ClusterCoord'):
   cleanup={'AreaDamage':16,'SelectAnim':8,'AnimCtor':28,'ClusterCoord':8}[kind]
   pending.append((m.read32(sp),sp+4+cleanup,kind,u.reg_read(UC_X86_REG_ECX)))
 h=u.hook_add(UC_HOOK_CODE,observe)
 try:
  if case['route']=='outer':m.invoke(0x468d80,b,(0,))
  else:m.invoke(0x4690b0,b,(position,))
 except Exception:
  print('BRUTE FAILED',list(map(hex,trace)),events[-8:]);raise
 finally:u.hook_del(h)
 assert not pending and not rng.pending,(pending,rng.pending)
 assert before['main']==rng.states()['main'] and before['mapgen']==rng.states()['mapgen']
 assert sum(len(q['raw_draws']) for q in rng.requests)==len(raw_advances)
 assert all(a['logic_registered'] for a in anims)
 return dict(input=case,initial=initial,events=events,anims=anims,rng_before=before,rng_after=rng.states(),rng_requests=rng.requests,
  raw_draw_count=sum(len(q['raw_draws']) for q in rng.requests),raw_advance_count=len(raw_advances),raw_advance_sites=raw_advances,native_cursor_before=cursor,native_cursor_after=impact.i32(u,scenario+0x214),
  bullet_native_id=impact.i32(u,b+0x10),bullet_live_after=impact.xyz(u,b+0x9c),bullet_alive=bool(u.mem_read(b+0x90,1)[0]))


def brute_generate():
 # Seeds found by original full468D80 execution, then independently rerun on
 # fresh physical worlds. RngTrace's Scenario stream uses base seed+1.
 cases=[dict(name='single_lg_no_anim',seed=0),dict(name='two_bouncer_constructors',seed=1),
  dict(name='unread_d_then_large',seed=4),dict(name='single_small',seed=8),
  dict(name='zero_max',seed=1,zero_max=True),dict(name='isolated_iron_curtain',seed=1,receiver='iron_curtain'),
  dict(name='distinct_live_and_placement',seed=1,route='detonate',placement=[2816,5248,624]),
  dict(name='receiver_boundary',seed=1,receiver='receiver')]
 rows=[brute_execute(case) for case in cases]
 initial=rows[0].pop('initial')
 for row in rows[1:]:assert row.pop('initial')==initial
 by_name={r['input']['name']:r for r in rows}
 assert [a['type'] for a in by_name['single_lg_no_anim']['anims']]==['DBRIS2LG']
 assert [a['type'] for a in by_name['single_small']['anims']]==['DBRIS1SM']
 assert [a['type'] for a in by_name['unread_d_then_large']['anims']]==['D','DBRS10LG']
 assert not by_name['zero_max']['anims']
 assert [a['type'] for a in by_name['isolated_iron_curtain']['anims']]==['IRONFX']
 for row in rows:
  assert row['native_cursor_after']-row['native_cursor_before']==len(row['anims'])
  if row['input']['route']=='outer':
   successor=[q for q in row['rng_requests'] if q['caller']=='0x46905c']
   assert len(successor)==1 and successor[0]['low']==256 and successor[0]['high']==512
  for anim in row['anims']:
   if anim['type']!='IRONFX':assert anim['location']==[2688,5248,644]
 return dict(native_sha256=NATIVE_SHA256,initial=initial,rows=rows)


def brute_metadata():
 return provenance(scope='Eight physical Smashing detonation controls: full4690B0 and seven full468D80 Cluster1 calls, native15-type read128 pool, full Anim constructors/Start and complete RNG states',assumptions=[
  'Existing physical IFV world/readers establish a supplied admitted Bullet/source/receiver boundary, not full Brute launch or MTNK health. New native Smash/InvisibleLow/Smashing readers run on physical RULESMD, absent LANGRULE, Battle and current Hills lexical caches. Weapon postpass includes physical AudioVisual.Gravity.',
  'Rules MetallicDebris runs original constructor/read128/strtok/FindOrAllocate on the same Rules object. Fourteen real DBRIS types plus unreadD result; no supplied20-pool. Actual physical ART/SHP bytes run full427530/427D00/image binding, including absentD ART and physical IRONFX.',
  'Original AreaDamage489280, IC timer/isolation, SelectAnim48A4F0, nullify branch, debris budget/list picks, full Anim421EA0/Unlimbo/Logic/Start424CE0 and Cluster1 successor49F420 run. No return at those gates is supplied. The distinct-placement row invokes full4690B0 alone; all other rows enter full468D80.',
  'Normal physical Smashing has no ordinary impact Anim. Min1/Max3 produces original inclusive1..2 budget. The zero control reads explicit authored Max0/Min0 through native Warhead reader; stock fields otherwise remain unchanged. Live BulletXYZ2688,5248,624 and direct placement2816,5248,624 are supplied coordinate boundaries.',
  'Receiver empty is the original no-receiver AreaDamage1 result. Receiver/IC rows supply admitted original Unit-vtable state and timer with UnitReceiveDamage737C90 returning0/no health mutation; original AreaDamage independently returns0/2. No full native MTNK receiver RNG/IDs/health or whole native ID-prefix claim.',
  'GameActive1, flat level6 mapped cells, native frame100, no bridge/tile/overlay effects and empty sound registry are explicit inputs. All allocations succeed in this corpus; OOM/allocator failure is excluded. Sound binding/playback, whole Logic scheduling and later AnimAI flight/landing are excluded.',
  'Existing RngTrace records full0x3F4 Main/Scenario/MapGen states, helper bounds/results and every rejected raw word; direct advance stores independently count actual generator advances. Main=baseSeed, Scenario=baseSeed+1, MapGen=baseSeed+2. Native-found seeds were rerun on fresh worlds; no Rust output supplies goldens.',
 ],substitutions=[
  'Existing physical lexical INI caches, raw archive-byte I/O, allocator/delete/CRT/TLS transport and mapped empty SEH. CRT atexit7C978A returns0, registering no host process teardown; this existing OS/CRT boundary supplies no gameplay result. UnitReceiveDamage737C90 is the declared receiver-only boundary. No detonation/AreaDamage/selector/IC/debris/RNG/Anim constructor/Start result is replaced.',
 ],entry_points={'impact':0x468d80,'detonate':0x4690b0,'area_damage':0x489280,'select_anim':0x48a4f0,'budget':0x469d06,'debris_get_coords':0x469e50,'list_pick':0x469e85,'anim_ctor':0x421ea0,'anim_start':0x424ce0,'bounce_init':0x4397e0,'successor':0x469057,'random_ranged':0x65c7e0,'random_raw':0x65c780,'metallic_reader':0x66da90})


if __name__=='__main__':
 import sys
 if '--brute-debris' in sys.argv:
  finish_vectors(brute_generate,Path(__file__).with_name('brute_smash_debris.json'),provenance=brute_metadata,
   argv=[arg for arg in sys.argv[1:] if arg!='--brute-debris'])
 else:finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
