"""Original IFV launch through impact and bounded object retirement.
No Rust results or current-chain fixtures supply outputs.
The optional --magnetron-drop mode joins temporary Bullet construction,
detonation and immediate destruction with an explicit Unit receiver boundary.
"""
import importlib.util,json,struct,sys,hashlib,os
from pathlib import Path
from collections import deque
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_INVALID
from unicorn.x86_const import *
from capstone import Cs,CS_ARCH_X86,CS_MODE_32
from tools.native_oracle import RET_MAGIC,run_checked,finish_vectors,provenance,initialize_empty_windows_seh,checked_is_bad_read_ptr_transport
from tools.spatial_oracle.building_body_rules import SP,dwords,INI
from tools.projectile_oracle.bridge_render_inputs import lexical,assets_root
# Isolate fixture adapters from other consumers of the shared launch harness.
spec=importlib.util.spec_from_file_location('_impact_launch',Path(__file__).with_name('ifv_launch.py'))
launch=importlib.util.module_from_spec(spec);spec.loader.exec_module(launch)
i32,xyz,vec=launch.i32,launch.xyz,launch.vec
base_prepare=launch.prepare
def initialize_effect_registries(m):
 """Original effect/world initializer regions shared by complete Anim witnesses."""
 u=m.u;m.invoke(0x561910,0)
 for start,end in ((0x4e7ae0,0x4e7b16),(0x7253d0,0x725406),(0x7252d0,0x725306),(0x725450,0x725486)):
  u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,start,end)

def initialize_effect_world(m,cells,seed=31,*,map_size=(64,64),clear_terrain=True):
 """Supply the existing flat world's active/display state and seed natively."""
 u=m.u;u.mem_write(0xa8e9a0,b'\x01');u.mem_write(0x87f914,dwords(*map_size))
 for start,end in ((0x40b540,0x40b5ab),(0x725850,0x725886),(0x4e6d60,0x4e6d96)):
  u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,start,end)
 from tools.rmg_oracle.gen_rng_vectors import seeded_struct
 u.mem_write(m.read32(0xa8b230)+0x218,seeded_struct(seed))
 if clear_terrain:
  for c in list(cells.values())+[0xabdc50]:
   u.mem_write(c+0x38,dwords(-1));u.mem_write(c+0x44,dwords(-1))

def retirement_transport(m,pc,events):
 """Only the existing Windows COM decrement/readability boundaries."""
 u=m.u;sp=u.reg_read(UC_X86_REG_ESP)
 if pc==0x46b007:
  ptr=m.read32(sp);value=(m.read32(ptr)-1)&0xffffffff
  u.mem_write(ptr,dwords(value));u.reg_write(UC_X86_REG_EAX,value)
  u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,0x46b00d)
  events.append(dict(event='OSInterlockedDecrement',field=hex(ptr),result=value))
  return True
 return checked_is_bad_read_ptr_transport(u,pc,sp,events)

def prepare():
 result=base_prepare();m=result[0];u=m.u;initialize_effect_registries(m)
 names=['XGRYSML1','XGRYSML2','EXPLOSML','XGRYMED1','XGRYMED2','EXPLOMED','EXPLOLRG','TWLT070']
 art,_=lexical((assets_root()/'ARTMD.INI').read_bytes(),set(names));m.make_ini(art);rows=[]
 for name in names:
  t=m.invoke(0x428b80,m.cstring(name));m.asset_loaded=[];admitted=m.invoke(0x427d00,t,(INI,))
  image=m.read32(t+0xa4)
  rows.append(dict(name=name,admitted=admitted&255,image_present=bool(image),frame_count=struct.unpack('<h',u.mem_read(image+6,2))[0] if image else None,end=i32(u,t+0x2c0),rate=i32(u,t+0x2b0),report=i32(u,t+0x2f8),scorch=bool(u.mem_read(t+0x36b,1)[0]),crater=bool(u.mem_read(t+0x36d,1)[0]),spawns_particle=i32(u,t+0x2cc),num_particles=i32(u,t+0x2d0),assets=m.asset_loaded))
 result[-1]['impact_anim_art']=rows
 return result
launch.prepare=prepare

def execute(bridge=True,origin=(2688,5248,1030),bridge_band=None,changes=()):
 m,b,cells,result=launch.launch(origin=origin,bridge=bridge);u=m.u
 initialize_effect_world(m,cells)
 if bridge_band is not None:
  for xy,c in cells.items():u.mem_write(c+0x140,dwords(0x100 if xy in bridge_band else 0))
  result['supplied']['bridge_band']=[list(xy) for xy in bridge_band]
 result['supplied']['between_frame_flag_changes']=[dict(frame=f,live=live) for f,live in changes]
 trace=deque(maxlen=32);events=[];pending={};raw_words=[];frame=0;area=False;fault=[];cs=Cs(CS_ARCH_X86,CS_MODE_32)
 addresses={0x5f4ec0:'NativeUnlimbo',0x4a9720:'DisplaySubmit',0x4690b0:'Detonate',0x489280:'AreaDamage',0x7258d0:'PointerExpired',0x5f65f0:'UnInit',0x466560:'BulletDtor',0x5f3b80:'ObjectDtor',0x725c70:'Drain',0x4a9770:'DisplayRemove',0x5f4d30:'Conceal',0x48a4f0:'SelectAnim',0x421ea0:'AnimCtor',0x424ce0:'AnimStart',0x424f00:'AnimMiddle',0x4255b0:'AnimDestroy',0x6b4a50:'SmudgeCtor',0x426590:'AnimScalarDtor',0x68bcb0:'NextNativeId',0x65c640:'Random1',0x65c660:'Random2',0x65c780:'RandomRaw',0x65c7e0:'RandomRanged',0x587180:'BridgeDriver',0x57baa0:'BridgeDriver2',0x57ccf0:'BridgeDriver3'}
 def invalid(uc,access,address,size,value,data):
  fault.append(dict(access=access,address=hex(address),size=size,pc=hex(u.reg_read(UC_X86_REG_EIP))));return False
 def obs(uc,a,n,d):
  nonlocal area
  trace.append(a)
  if a in pending:
   event=pending.pop(a);value=u.reg_read(UC_X86_REG_EAX);returned=dict(event=event['event']+'_return',frame=frame,pc=hex(a),eax=value)
   event['returned_eax']=value
   if event['event']=='AnimCtor':returned.update(native_id=i32(u,value+0x10),anim_type=m.string(m.read32(value+0xc8)+0x24),alive=u.mem_read(value+0x90,1)[0]);event['constructed_native_id']=i32(u,value+0x10)
   if event['event']=='NativeUnlimbo':returned.update(admitted=bool(value&255),retained=xyz(u,int(event['ecx'],16)+0x9c))
   if event['event']=='SelectAnim':returned['selected_name']=m.string(value+0x24) if value else None
   events.append(returned)
  if a in (0x65c84b,0x65c79d):raw_words.append(dict(frame=frame,pc=hex(a),word=u.reg_read(UC_X86_REG_ESI)))
  if a in addresses:
   sp=u.reg_read(UC_X86_REG_ESP);entry=dict(event=addresses[a],pc=hex(a),frame=frame,return_pc=hex(m.read32(sp)),ecx=hex(u.reg_read(UC_X86_REG_ECX)),args=[hex(m.read32(sp+4+j*4)) for j in range(4)])
   if a==0x5f4ec0:entry.update(position=xyz(u,m.read32(sp+4)))
   if a==0x421ea0:entry.update(anim_type=m.string(m.read32(sp+4)+0x24),position=xyz(u,m.read32(sp+8)))
   if a==0x48a4f0:entry.update(position=xyz(u,m.read32(sp+8)),retained_bullet=xyz(u,b+0x9c),land=i32(u,sp+4),damage=u.reg_read(UC_X86_REG_ECX),height_gate_unit=i32(u,0x89de70),level_height=i32(u,0x89e870))
   if a==0x489280:entry.update(position=xyz(u,u.reg_read(UC_X86_REG_ECX)),damage=u.reg_read(UC_X86_REG_EDX));area=True
   if a in (0x489280,0x48a4f0,0x421ea0,0x68bcb0,0x65c780,0x65c7e0,0x5f4ec0):pending[m.read32(sp)]=entry
   if a==0x65c7e0:entry.update(low=i32(u,sp+4),high=i32(u,sp+8))
   events.append(entry)
  retirement_transport(m,a,events)
  if a>=0x20000000 and a!=RET_MAGIC:raise AssertionError(('non-image-code',hex(a)))
 h=u.hook_add(UC_HOOK_CODE,obs);hi=u.hook_add(UC_HOOK_MEM_INVALID,invalid)
 frames=[]
 try:
  for frame in range(1,121):
   for change_frame,live in changes:
    if frame==change_frame:
     for xy in bridge_band:u.mem_write(cells[xy]+0x140,dwords(0x100 if live else 0))
   u.mem_write(0xa8ed84,dwords(frame));u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,b)
   run_checked(u,0x4666e0,RET_MAGIC,count=1000000)
   frames.append(dict(frame=frame,position=xyz(u,b+0x9c),velocity=vec(u,b+0xe8),alive=u.mem_read(b+0x90,1)[0]))
   if area:break
  result['before_drain']=dict(alive=u.mem_read(b+0x90,1)[0],queue_count=i32(u,0xb0f6a8),bullet_count=i32(u,0xa8ed50),anim_count=i32(u,0xa8e9b8))
  initialize_empty_windows_seh(u)
  m.invoke(0x725c70,0)
  result['after_drain']=dict(alive=u.mem_read(b+0x90,1)[0],queue_count=i32(u,0xb0f6a8),bullet_count=i32(u,0xa8ed50),anim_count=i32(u,0xa8e9b8))
  result['anim_frames']=[]
  for _ in range(100):
   if i32(u,0xa8e9b8)==0:break
   a=m.read32(m.read32(0xa8e9ac));frame+=1;u.mem_write(0xa8ed84,dwords(frame))
   m.invoke(0x423ac0,a)
   result['anim_frames'].append(dict(frame=frame,alive=u.mem_read(a+0x90,1)[0],anim_frame=i32(u,a+0xac),delay=i32(u,a+0x184),timer_start=i32(u,a+0xb4),timer_duration=i32(u,a+0xbc),queue_count=i32(u,0xb0f6a8)))
   if i32(u,0xb0f6a8):m.invoke(0x725c70,0)
  result['after_anim']=dict(queue_count=i32(u,0xb0f6a8),anim_count=i32(u,0xa8e9b8))
 except Exception as exc:
  result['failure']=dict(error=str(exc),fault=fault,registers={r:hex(u.reg_read(v)) for r,v in [('eax',UC_X86_REG_EAX),('ebx',UC_X86_REG_EBX),('ecx',UC_X86_REG_ECX),('edx',UC_X86_REG_EDX),('esi',UC_X86_REG_ESI),('edi',UC_X86_REG_EDI),('ebp',UC_X86_REG_EBP),('esp',UC_X86_REG_ESP)]},trace=[f'{a:08x}: '+ '; '.join(f'{x.mnemonic} {x.op_str}' for x in cs.disasm(bytes(u.mem_read(a,15)),a,count=1)) for a in trace])
 finally:u.hook_del(h);u.hook_del(hi)
 result.update(frames=frames,events=events,raw_rng_words=raw_words,final_scenario_native_id=i32(u,m.read32(0xa8b230)+0x214),rng_state_sha256=hashlib.sha256(bytes(u.mem_read(m.read32(0xa8b230)+0x218,0x3f4))).hexdigest());return result
def generate():
 band=[(x,20) for x in range(12,16)]
 specs=[dict(name='rise_live',bridge=True),dict(name='rise_clear',bridge=False),dict(name='band_live',bridge=False,origin=(2688,5248,1250),bridge_band=band),dict(name='band_clear16',bridge=False,origin=(2688,5248,1250),bridge_band=band,changes=((16,False),)),dict(name='band_clear16_repair24',bridge=False,origin=(2688,5248,1250),bridge_band=band,changes=((16,False),(24,True)))]
 rows=[]
 for case in specs:
  name=case.pop('name');row=execute(**case);row['name']=name
  assert 'failure' not in row,(name,row.get('failure'))
  assert row['after_drain']['queue_count']==0 and row['after_drain']['bullet_count']==0
  assert row['after_anim']['queue_count']==0 and row['after_anim']['anim_count']==0
  rows.append(row)
 return dict(scope='Same five supplied launch/bridge-flag cases, physical DRAGON plus eight native HE AnimType ART readers, full original BulletAI/Detonate/AreaDamage with empty damage receivers and ScenarioDestroyableBridges=false, native impact Anim creation and Bullet retirement/drain, then single AnimAI until complete retirement. No damageable bridge topology, renderer, audio binding, Burst2 scheduler or whole Process/identity claim.',scenario_rng_seed=31,rows=rows)
def metadata():
 return provenance(scope='Five physical IFV impact controls: 125 original Bullet AI visits and 70 impact Anim AI visits through complete bounded retirement',assumptions=[
  'Original cell-spread table initializer561910, selected physical readers, FireAt launch, guided flight, full Detonate4690B0 and AreaDamage489280, impact SelectAnim/constructor/Start and full Bullet/Anim retirement execute. Earlier launch supplied boundaries remain: source lifecycle/FLH, Unlimbo/Display admission and inter-frame structural-bit changes.',
  'The shared launch boundary executes original Unlimbo admission-state writes, BulletType coordinate fixup and SetLocation. Correcting its former raw-coordinate copy clears InLimbo, so Conceal now executes DetachAll PointerExpired7258D0 and DisplayRemove4A9770 before drain. Every other retained payload field is unchanged by that boundary correction.',
  'Supplied GameActiveA8E9A0=1 admits original Anim Unlimbo/coordinate commit/Display submission. Flat level6 map cells, empty damage/air receivers, no tile/overlay identities and ScenarioDestroyableBridges=false. No damageable bridge topology; that is the separate ifv_bridge_impact fixture.',
  'Physical DRAGON and eight HE effect SHPs are read through native readers. Original partial pool construction gives Bullet29 then impact Anim30; no full startup/wholematch native-ID claim. Each control independently seeds Scenario RNG31.',
  'Host schedules pending drain after the Bullet tick, then one impact AnimAI per frame until its native retirement. All timer bodies execute; no whole Logic/Display scheduler claim. The LineTrail slot is null; ifv_trail_impact separately joins its admitted producer and detach.',
  'Shared selected reader setup is not full Rules Process chronology. No Rust expectation, native instruction patch, rendering or audio binding claim.',
 ],substitutions=[
  'Shared launch/input helpers supply lexical physical INI caches, archive bytes, allocation/delete/CRT/TLS and world admission. Verified imported InterlockedDecrement changes COM refcount only; original Release/destructors execute. IsBadReadPtr is false after mapped-memory validation, with an empty supplied FS exception chain.',
 ],entry_points={'bullet_ai':0x4666e0,'detonate':0x4690b0,'area_damage':0x489280,'select_anim':0x48a4f0,'anim_ctor':0x421ea0,'anim_ai':0x423ac0,'uninit':0x5f65f0,'expiry':0x7258d0,'conceal':0x5f4d30,'drain':0x725c70,'bullet_dtor':0x466560,'anim_dtor':0x426590})

def drop_temporary_bullets(source_self=False):
 """Original Unit drop callback with a declared fatal receiver boundary.

 Keep the factory, Bullet/Anim constructors, random draws, detonation and
 destruction in original code. The supplied Unit damage transition is the
 bounded seam; it executes original broadcast/PointerExpired before removal.
 """
 from tools.procedural_drawing_oracle.electric_bolt import RngTrace
 m,credit,st,_weapon,cells,_initial=prepare();u=m.u
 effect_inputs=_initial['impact_anim_art']
 assert all(row['image_present'] for row in effect_inputs), 'drop controls require all eight physical impact SHPs'
 assert 'DRAGON.SHP' in m.assets, 'shared input world requires physical DRAGON.SHP'
 dragon=m.assets['DRAGON.SHP']
 shared_image=dict(name='DRAGON.SHP',bytes=len(dragon),sha256=hashlib.sha256(dragon).hexdigest())
 initialize_effect_world(m,cells);initialize_empty_windows_seh(u)
 # Object.cpp has its own thirteen header statics. In particular IsHighFlying
 # reads AC13C8; the Bullet/Cell statics in the shared world do not set it.
 object_initializers=struct.unpack('<13I',u.mem_read(0x8141d8,52))
 for address in object_initializers:m.invoke(address,0)
 assert i32(u,0xac13c8)==104
 rules=m.read32(0x8871e0);section=m.cstring('CombatDamage');buf=m.alloc(128)
 fields={'death_weapon':(0xfdc,'DeathWeapon',0x772fa0),
         'crush':(0xfac,'CrushWarhead',0x75e3b0),'c4':(0xfa8,'C4Warhead',0x75e3b0)}
 layers=[];refs={};health=300
 for filename in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=assets_root()/filename
  if not path.exists():
   assert filename=='LANGRULE.INI';layers.append(dict(file=filename,absent=True));continue
  raw=path.read_bytes();sections,_=lexical(raw,{'CombatDamage','DefaultDeathWeapon','Invisible','Crush','Super','MTNK'})
  m.rules_cache(sections)
  for name,(offset,key,allocator) in fields.items():
   if m.invoke(0x528a10,launch.RULES,(section,m.cstring(key),m.cstring(''),buf,128)):
    refs[name]=m.invoke(allocator,buf);u.mem_write(rules+offset,dwords(refs[name]))
  w=refs['death_weapon'];m.invoke(0x772080,w,(launch.RULES,))
  p=m.read32(w+0xa0);m.invoke(0x46bee0,p,(launch.RULES,))
  for wh in (refs['crush'],refs['c4']):m.invoke(0x75d3a0,wh,(launch.RULES,))
  for reg,val in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,rules),(UC_X86_REG_EDI,launch.RULES),(UC_X86_REG_EAX,section)):
   u.reg_write(reg,val)
  run_checked(u,0x66c9b7,0x66c9f6)
  health=m.invoke(0x5276d0,launch.RULES,(m.cstring('MTNK'),m.cstring('Strength'),health))
  layers.append(dict(file=filename,sha256=hashlib.sha256(raw).hexdigest(),physical_sections=sections,
    falling_multiplier_bits=bytes(u.mem_read(rules+0xf64,4)).hex(),
    current_strength_damage=bool(u.mem_read(rules+0xf68,1)[0]),health=health))
 assert m.string(p+0x24)=='Invisible'
 assert m.string(refs['crush']+0x24)=='Crush' and m.string(refs['c4']+0x24)=='Super'
 owner=m.alloc(0x1000);below=m.alloc(0x1000)
 # Unit actors/type state are supplied, as in the existing impact receiver
 # fixtures. The MTNK HP and CombatDamage fields above use original readers.
 for actor,position,hp in ((owner,[2688,5248,624],health),(below,[2688,5248,624],10000),
                            (credit,[3968,5248,624],150)):
  u.mem_write(actor,dwords(0x7f5c70));u.mem_write(actor+0x6c4,dwords(st))
  u.mem_write(actor+0x14,dwords(7));u.mem_write(actor+0x74,b'\1')
  u.mem_write(actor+0x6c,dwords(hp));u.mem_write(actor+0x90,b'\1')
  u.mem_write(actor+0x9c,dwords(*position))
 u.mem_write(owner+0x6ad,b'\1');u.mem_write(owner+0x428,dwords(0 if source_self else credit))
 u.mem_write(owner+0x30,dwords(below));u.mem_write(cells[10,20]+0xe4,dwords(owner))
 u.mem_write(0xa8ed84,dwords(100))
 falling_height=m.invoke(m.read32(m.read32(owner)+0x1c8),owner)
 falling_is_high=bool(m.invoke(m.read32(m.read32(owner)+0x54),owner)&255)
 assert falling_height==0 and not falling_is_high
 rng=RngTrace(u,m.read32(0xa8b230),31);before=rng.states()
 cursor_before=i32(u,m.read32(0xa8b230)+0x214)
 actors={owner:'falling',below:'below',credit:'credit'};cell_names={ptr:list(xy) for xy,ptr in cells.items()}
 bullets=[];pointers={};events=[];pending={};platform=[];recent=deque(maxlen=24)
 def identity(ptr):
  if not ptr:return None
  if ptr in actors:return dict(kind='entity',name=actors[ptr])
  if ptr in cell_names:return dict(kind='cell',cell=cell_names[ptr])
  if ptr in pointers:return dict(kind='bullet',index=pointers[ptr])
  raise AssertionError(('unknown drop identity',hex(ptr)))
 def contains(vector,ptr):
  data=m.read32(vector+4);count=m.read32(vector+0x10)
  return ptr in [m.read32(data+4*i) for i in range(count)]
 def state(b):
  return dict(native_id=i32(u,b+0x10),location=xyz(u,b+0x9c),
   source=identity(m.read32(b+0xb0)),target=identity(m.read32(b+0x10c)),
   projectile=m.string(m.read32(b+0xac)+0x24) if m.read32(b+0xac) else None,
   installed_weapon=m.string(m.read32(b+0x130)+0x24) if m.read32(b+0x130) else None,
   warhead=m.string(m.read32(b+0x128)+0x24),damage=i32(u,b+0x6c),
   damage_multiplier=i32(u,b+0x150),maximum_speed=i32(u,b+0x110),
   bright=bool(u.mem_read(b+0xe0,1)[0]),alive=bool(u.mem_read(b+0x90,1)[0]),
   in_limbo=bool(u.mem_read(b+0x81,1)[0]),on_map=bool(u.mem_read(b+0x80,1)[0]),
   on_bridge=bool(u.mem_read(b+0x8c,1)[0]),in_logic=bool(u.mem_read(b+0x98,1)[0]),
   timers=[i32(u,b+v) for v in (0xb8,0xc0,0xc4,0xcc)],reference=xyz(u,b+0xd0),
   object_registered=contains(0xa8e360,b),bullet_registered=contains(0xa8ed40,b),
   expiry_registered=contains(0xb0f720,b))
 def add(kind,**values):
  row=dict(kind=kind,**values);events.append(row);return row
 def observe(_u,pc,_size,_data):
  recent.append(pc);sp=u.reg_read(UC_X86_REG_ESP);obj=u.reg_read(UC_X86_REG_ECX)
  key=(pc,sp)
  if key in pending:
   kind,record,b=pending.pop(key)
   if kind=='construct':record['constructed']=state(b)
   elif kind=='detonate':record['after_detonate']=state(b);record['rng_after']=rng.states()
   elif kind=='release':record['destroyed']=state(b)
   elif kind=='expiry':record['after']=state(b)
   elif kind=='anim':record['native_id']=i32(u,b+0x10)
   add(kind+'_return',bullet=pointers.get(b),native_id=i32(u,b+0x10))
  if launch.bullet_activation_transport(m,pc,platform):return
  if launch.bullet_addref_transport(m,pc,platform):return
  if retirement_transport(m,pc,platform):return
  if pc==0x4664c0:
   index=len(bullets);pointers[obj]=index;record={};bullets.append(record)
   add('construct',bullet=index)
   pending[(m.read32(sp),sp+32)]=('construct',record,obj)
  elif pc==0x4690b0:
   record=bullets[pointers[obj]];record['impact']=xyz(u,m.read32(sp+4));record['before_detonate']=state(obj)
   add('detonate',bullet=pointers[obj],impact=record['impact'])
   pending[(m.read32(sp),sp+8)]=('detonate',record,obj)
  elif pc==0x46aff0:
   # IUnknown::Release is stdcall; its receiver is the stack argument.
   b=m.read32(sp+4);add('release',bullet=pointers[b])
   pending[(m.read32(sp),sp+8)]=('release',bullets[pointers[b]],b)
  elif pc==0x466560:add('bullet_dtor',bullet=pointers[obj])
  elif pc==0x7258d0:add('broadcast',expired=identity(obj),removed=bool(u.reg_read(UC_X86_REG_EDX)&255))
  elif pc==0x4684e0:
   record=add('pointer_expired',bullet=pointers[obj],expired=identity(m.read32(sp+4)),before=state(obj))
   pending[(m.read32(sp),sp+12)]=('expiry',record,obj)
  elif pc==0x48a620:
   add('combat_light',damage=struct.unpack('<i',dwords(obj))[0],
       warhead=m.string(u.reg_read(UC_X86_REG_EDX)+0x24),
       location=xyz(u,sp+4),force=m.read32(sp+16),flags=m.read32(sp+20))
  elif pc==0x421ea0:
   record=add('anim_ctor',name=m.string(m.read32(sp+4)+0x24),location=xyz(u,m.read32(sp+8)))
   pending[(m.read32(sp),sp+32)]=('anim',record,obj)
  elif pc in (0x65c780,0x65c7e0):add('rng',request=len(rng.requests)-1)
  elif pc==0x737c90:add('receiver',target=identity(obj),damage=i32(u,m.read32(sp+4)))
  elif pc==0x702d40:
   add('record_kill_boundary',owner=identity(obj));m.ret(0,4)
  elif pc==0x4de5d0:
   add('unit_uninit_boundary',owner=identity(obj));m.ret(0,0)
  elif pc==0x468670 or (pc==0x55baa0 and m.read32(sp+4) in pointers):
   raise AssertionError(('unexpected Bullet Fire or Logic admission',hex(pc)))
 hook=u.hook_add(UC_HOOK_CODE,observe)
 # The original receiver is the declared seam. A first lethal falling-target
 # transition runs native broadcast on a separate stack; no RNG/ID result is
 # supplied. Original AreaDamage still traverses the second receiver itself.
 hit=False
 try:
  u.mem_write(SP,dwords(RET_MAGIC,0x117c,0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,owner+8)
  pc=0x746100
  while True:
   run_checked(u,pc,(RET_MAGIC,0x737c90),count=2000000)
   pc=u.reg_read(UC_X86_REG_EIP)
   if pc==RET_MAGIC:break
   receiver=u.reg_read(UC_X86_REG_ECX)
   if receiver==owner and not hit:
    hit=True;context=u.context_save();u.mem_write(owner+0x6c,dwords(0))
    add('fatal_receiver_boundary',target=identity(owner))
    u.mem_write(SP+0x8000,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP+0x8000)
    u.reg_write(UC_X86_REG_ECX,owner);u.reg_write(UC_X86_REG_EDX,1)
    run_checked(u,0x7258d0,RET_MAGIC,count=2000000)
    u.context_restore(context);u.mem_write(owner+0x90,b'\0');u.mem_write(cells[10,20]+0xe4,dwords(below));m.ret(4,28)
   else:m.ret(0,28)
   pc=u.reg_read(UC_X86_REG_EIP)
 finally:u.hook_del(hook)
 assert hit and len(bullets)==2 and not pending,pending
 assert all(not row['constructed']['installed_weapon'] for row in bullets)
 assert all(row['constructed']['location']==[0,0,0] for row in bullets)
 assert all(not row['constructed']['in_logic'] and row['constructed']['in_limbo'] for row in bullets)
 assert all(not row['destroyed']['object_registered'] and not row['destroyed']['bullet_registered']
            and not row['destroyed']['expiry_registered'] for row in bullets)
 return dict(input=dict(name='self_source' if source_self else 'credited_source',source_self=source_self,
    health=health,frame=100,seed=31,falling_location=[2688,5248,624],below_location=[2688,5248,624]),
   layers=layers,effect_inputs=effect_inputs,shared_world_image=shared_image,
   effective=dict(death_weapon=m.string(w+0x24),projectile=m.string(p+0x24),
     weapon_bright=bool(u.mem_read(w+0x12f,1)[0]),weapon_rad_level=i32(u,w+0x158),
     current_strength_damage=bool(u.mem_read(rules+0xf68,1)[0]),
     falling_multiplier_bits=bytes(u.mem_read(rules+0xf64,4)).hex(),
     supplied_unit_type=m.string(st+0x24),object_level_height=i32(u,0xac13c8),
     falling_height=falling_height,falling_is_high=falling_is_high,
     object_initializers=[f'{address:08X}' for address in object_initializers]),
   native_id_before=cursor_before,native_id_after=i32(u,m.read32(0xa8b230)+0x214),
   rng_before=before,rng_after=rng.states(),requests=rng.requests,bullets=bullets,events=events)

def drop_generate():
 return dict(cases=[drop_temporary_bullets(False),drop_temporary_bullets(True)])

def drop_metadata():
 return provenance(scope='Two original Unit falling callbacks, real temporary Bullet factory/Construct/Detonate/Release, native scatter/light/Anim construction and ID ordering, with declared Unit receiver transition',
  assumptions=[
   'Original746100 enters its746209 lifted-Unit arm. Unit actors, FV type from the shared fixture, null house pointers, coordinates/health and ground linked list are supplied; no Unit constructor, lift flight or actual Unit receiver/death is claimed. Falling HP uses native-read MTNK Strength300; below HP10000 is supplied. CombatDamage names/scalars use original readers over physical RULESMD/LANGRULE/Battle/Hills strings.',
   'All13 original Object header statics in CRT table8141D8 execute after the shared Bullet/Cell world setup. Object IsHighFlying reads its own AC13C8=104; native precheck establishes falling object height0 and not high. Bullet constructor Location remains originalzero, independent of supplied impact624.',
   'Physical DefaultDeathWeapon/Invisible/Crush/Super use original full Weapon/Bullet/Warhead readers. Existing impact owner reads eight physical effect ART/SHPs; their native frame/end/rate/flags and asset hashes are recorded and missing images reject generation. Physical DRAGON.SHP is required for the shared prepared world and its hash is recorded. No raw game files are embedded. Sound registry remains empty. This is selected reader execution, not full Rules Process or full scenario construction.',
   'The first falling Unit ReceiveDamage737C90 boundary supplies health0, one original7258D0 broadcast before marking dead/unlinking, and return4. Other reached Unit receivers return0 with no mutation. Parent/world production tests own actual Unit death admission, all its repeated notifications and damage amount. The boundary supplies no random word, native ID, light or animation output.',
   'Original702D40 kill accounting and final FootUnInit4DE5D0 return0 as declared external boundaries; the temporary Bullets are actually Released/destroyed before each. No Fire/AI/Logic admission for those Bullets. All timer stores, originalPointerExpired and destructor broadcasts execute.',
   'Main/Scenario/MapGen full streams seeded31/32/33 by existing RngTrace. Each original BulletCtor obtains its ID before its detonation and nested effect constructors; the measured cursor is a partial prepared-world prefix, not whole startup identity. The physical TWLT070 has26frames and does not end during construction; only the two Inviso scatters draw in this boundary. Its eventual Crater draw during later AnimAI is outside these controls. All reached draws and full states are recorded, never supplied.',
  ],substitutions=[
   'Inherited BulletReader archive/INI cache, heap/delete/CRT/TLS transport. Existing COM activation invokes original registered factory6C5090; existing InterlockedIncrement/Decrement and checked IsBadReadPtr boundaries retain original constructor/destructor code.',
   'Declared737C90 receiver,702D40 accounting and4DE5D0 UnitUnInit seams above. Original Unit callback, Bullet/Anim constructors, AreaDamage traversal, selection, scatter, light, expiry and immediate Bullet destruction are not replaced.',
  ],entry_points=dict(unit_notice=0x746100,drop_arm=0x746209,create=0x46b050,construct=0x4664c0,
   detonate=0x4690b0,area=0x489280,scatter=0x49f420,light=0x48a620,anim_ctor=0x421ea0,
   bullet_dtor=0x466560,object_dtor=0x5f3b80,expiry=0x7258d0,bullet_pointer_expired=0x4684e0))


if __name__=='__main__':
 if '--magnetron-drop' in sys.argv:
  args=[a for a in sys.argv[1:] if a!='--magnetron-drop']
  finish_vectors(drop_generate,Path(__file__).with_name('magnetron_drop.json'),provenance=drop_metadata,argv=args)
 else:
  finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
