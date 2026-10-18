"""Original physical FV numbered FLH, flat Drive transform and two burst tails.

Physical INI cache setup, source lifecycle, source pose, HouseROF1, ScenarioSeed31
and second FireAt call time are supplied. This proves neither whole Unit AI nor
whole-scenario native identity prefix. Read ifv_fire_coord.md for exact coverage.
spawn_launch adds GetFLH with a nonzero base and SpawnManagerClass::AI case 0's
launch coordinate for retail V3, DRED and BSUB on a supplied flat pose.
"""
import hashlib,json,struct,sys
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import NATIVE_SHA256,NATIVE_FPCW,RET_MAGIC,run_checked,finish_vectors,provenance
from tools.projectile_oracle.bridge_render_inputs import assets_root,lexical
from tools.projectile_oracle.guided_step import create,i32,vec,xyz
from tools.projectile_oracle.ifv_launch import bullet_addref_transport,bullet_world_admission_transport
from tools.spatial_oracle.building_body_rules import RULES,SP,dwords


# Partial TechnoType rules reader slices (Primary..EliteSecondary, Weapon%d,
# WeaponCount/TurretCount and the other keys the FV fixture needs).
TYPE_SLICES=((0x71284a,0x712a8f),(0x71338b,0x7133c8),(0x7147b4,0x7147ce),(0x714a49,0x714a63),(0x714016,0x714030))

def read_type_rules(u,typ):
 for begin,end in TYPE_SLICES:
  for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,typ),(UC_X86_REG_EBX,typ+0x24),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EDI,RULES),(UC_X86_REG_EAX,u.mem_read(typ+0xd22,1)[0])):u.reg_write(reg,v)
  run_checked(u,begin,end)

def prepare():
 m,_,cells,init=create(False);u=m.u
 u.mem_write(0xa8ed84,dwords(0))
 for registry in (0xa8eb00,0xa83ce0):u.mem_write(registry,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 typ=m.alloc(0xf00);m.invoke(0x7470d0,typ,(m.cstring('FV'),));layers=[]
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=assets_root()/name
  if not path.exists():continue
  raw=path.read_bytes();sections,_=lexical(raw,{'FV'});m.rules_cache(sections)
  read_type_rules(u,typ)
  for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EDI,typ),(UC_X86_REG_EBP,typ+0x24),(UC_X86_REG_EBX,RULES)):u.reg_write(reg,v)
  run_checked(u,0x747b03,0x747b49)
  layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),burst_delays=list(struct.unpack('<4i',u.mem_read(typ+0xe48,16)))))
 src=m.alloc(0x1000);u.mem_write(src,dwords(0x7f5c70));u.mem_write(src+0x6c4,dwords(typ));u.mem_write(src+0x2b4,dwords(cells[16,20]));u.mem_write(src+0x90,b'\x01')
 heading=m.alloc(4)
 for offset in (0x388,0x3a0):
  m.invoke(0x4c91e0,src+offset,(0,));u.mem_write(heading,dwords(0 if offset==0x388 else 0x3fff));m.invoke(0x4c9300,src+offset,(heading,))
 u.reg_write(UC_X86_REG_ESI,src);u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,0x735678,0x735691,required_addresses=(0x70dc70,))
 slot=m.invoke(0x6f3330,src,(cells[16,20],));w=m.read32(m.invoke(0x70e140,src,(slot,)))
 assert m.string(w+0x24)=='HoverMissile'
 return m,src,typ,w,cells,dict(**init,type_layers=layers,selected_slot=slot)

def launch_shot(m,source,st,w,cells,initial,origin,frame=0,bridge=False):
 u=m.u;u.mem_write(0xa8ed84,dwords(frame));p=m.read32(w+0xa0);target=cells[16,20]
 for xy,c in cells.items():u.mem_write(c+0x140,dwords(0x100 if bridge and xy==(10,20) else 0))
 scratch=m.alloc(12);m.invoke(0x486890,target,(scratch,));target_xyz=xyz(u,scratch)
 u.mem_write(SP,bytes(0x2000))
 for ptr,values in ((SP+0x40,[w]),(SP+0x44,origin),(SP+0x68,[p]),(SP+0x88,target_xyz),(SP+0x1000+8,[target,0])):u.mem_write(ptr,dwords(*values))
 for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,SP+0x1000),(UC_X86_REG_ESI,source),(UC_X86_REG_EDI,m.read32(w+0xa4)),(UC_X86_REG_FPCW,NATIVE_FPCW)):u.reg_write(reg,value)
 scenario=m.read32(0xa8b230);events=[];bullet=[];visited=set();last=[]
 id_before=i32(u,scenario+0x214);scalars={}
 def observe(uc,a,n,d):
  if a>=0x20000000:raise AssertionError(('non-image-code',hex(a),'trace',last,'events',events,'bullet',bullet))
  last.append(hex(a));del last[:-15]
  if a in (0x773070,0x46b050,0x6c5090,0x466380,0x410230,0x68bcb0,0x4664c0,0x70bcb0,0x740f80,0x70e140,0x7177c0,0x70d590,0x468670,0x4e1130):visited.add(hex(a))
  if a in (0x65c640,0x65c660,0x65c780,0x65c7e0):events.append(dict(rng=hex(a),caller=hex(m.read32(u.reg_read(UC_X86_REG_ESP))),bounds=[i32(u,u.reg_read(UC_X86_REG_ESP)+4),i32(u,u.reg_read(UC_X86_REG_ESP)+8)] if a==0x65c7e0 else None))
  if a==0x68bcb0:events.append(dict(constructor_id_before=i32(u,scenario+0x214),caller=hex(m.read32(u.reg_read(UC_X86_REG_ESP)))))
  if a==0x65c84b:events.append(dict(rng_raw=u.reg_read(UC_X86_REG_ESI)))
  if a in (0x421ea0,0x62dc50,0x6b4a50,0x75e950):events.append(dict(effect_constructor=hex(a)))
  if a in (0x7258d0,0x5f65f0,0x466560,0x5f3b80):raise AssertionError(('unexpected launch detach/retirement',hex(a)))
  if a==0x46b072:
   sp=u.reg_read(UC_X86_REG_ESP);clsid,outer,context,iid,ppv=struct.unpack('<5I',u.mem_read(sp,20))
   assert (clsid,outer,context,iid)==(0x7e96e0,0,7,0x7f7c90)
   events.append(dict(call='CoCreateInstance activation to original registered factory',clsid_hex=bytes(u.mem_read(clsid,16)).hex(),iid_hex=bytes(u.mem_read(iid,16)).hex()))
   # Native stdcall factory consumes(this,outer,iid,ppv). Reuse the COM arg
   # stack with explicit native return so resulting ESP equals the COM call.
   u.mem_write(sp,dwords(0x46b078,0,outer,iid,ppv));u.reg_write(UC_X86_REG_EIP,0x6c5090)
  elif a==0x46afe5:
   # Verified PE import KERNEL32.InterlockedIncrement. The single-threaded
   # Windows API boundary updates COM refcount only; native Abstract ID remains
   # original410230/68BCB0 and is never assigned by this hook.
   bullet_addref_transport(m,a,events)
  elif a==0x6fe562:bullet.append(u.reg_read(UC_X86_REG_EAX))
  elif a==0x6fe53f:scalars['get_speed']=u.reg_read(UC_X86_REG_EAX)
  elif a==0x6fea52:scalars['launch_amount']=i32(u,SP+0x28)
  elif bullet_world_admission_transport(m,a,bullet[0] if bullet else 0,events):pass
 h=u.hook_add(UC_HOOK_CODE,observe)
 try:run_checked(u,0x6fe4f2,(0x6ff43f,0x6ff751,0x6ff93c),count=1000000,required_addresses=(0x6c5090,0x466380,0x68bcb0,0x70bcb0,0x740f80,0x468670))
 except Exception:
  print('LAUNCH FAILED',last,'ECX',hex(u.reg_read(UC_X86_REG_ECX)),file=sys.stderr);raise
 finally:u.hook_del(h)
 assert u.reg_read(UC_X86_REG_EIP)==0x6ff43f,hex(u.reg_read(UC_X86_REG_EIP))
 b=bullet[0]
 result=dict(native_sha256=NATIVE_SHA256,initial=initial,supplied=dict(origin=list(origin),source_heading=0x3fff,source_flags=0,weapon_slot=0,target_cell=[16,20],live_bridge_cell=[10,20] if bridge else None,binary_frame=frame),launch=dict(position=xyz(u,b+0x9c),target=target_xyz,velocity=vec(u,b+0xe8),**scalars,maximum_speed=i32(u,b+0x110),pitch=m.read32(SP+0x80)&0xffff,unique_id=i32(u,b+0x10),scenario_id_before=id_before,scenario_id_after=i32(u,scenario+0x214),course_locked=bool(u.mem_read(b+0x105,1)[0]),course_frames=i32(u,b+0x108),detector=dict(first_timer_start=i32(u,b+0xb8),first_timer_duration=i32(u,b+0xc0),arm_start=i32(u,b+0xc4),arm_duration=i32(u,b+0xcc),reference=xyz(u,b+0xd0),distance_watermark=i32(u,b+0xdc)),warhead=m.string(m.read32(b+0x128)+0x24),calls=sorted(visited),supplied_calls=events))
 result['launch']['next_burst']=i32(u,source+0x3b8);result['launch']['rearm']=[i32(u,source+0x2ec),i32(u,source+0x2f4)];return m,b,cells,result

def native_get_flh(m,source,output,slot=0):
 """Shared full original GetFLH call on a caller-owned native Unit/runtime."""
 return m.invoke(0x6F3AD0,source,(output,slot,0,0,0))


def generate():
 m,source,typ,w,cells,initial=prepare();u=m.u
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,typ);run_checked(u,0x715b10,0x715f9e)
 loco=m.alloc(0x100);m.invoke(0x4af540,loco);u.mem_write(loco+0xc,dwords(source));u.mem_write(source+0x674,dwords(loco+4));u.mem_write(source+0x14,dwords(4))
 house=m.alloc(0x2000);u.mem_write(house+0x1a8,struct.pack('<d',1.0));u.mem_write(source+0x21c,dwords(house))
 u.mem_write(source+0x9c,dwords(2688,5248,800));out=m.alloc(12);scenario=m.read32(0xa8b230);m.invoke(0x65c6d0,scenario+0x218,(31,))
 rows=[];frame=0
 for shot in range(2):
  burst=i32(u,source+0x3b8);flh_calls=[]
  def observe_flh(uc,a,n,d):
   if a in (0x6f3ad0,0x70e140,0x7177c0,0x4aff60,0x55a730):flh_calls.append(hex(a))
   if a in (0x68bcb0,0x65c7e0,0x65c780):raise AssertionError(('unexpected FLH effect',hex(a)))
  h=u.hook_add(UC_HOOK_CODE,observe_flh)
  try:native_get_flh(m,source,out)
  finally:u.hook_del(h)
  origin=xyz(u,out);rng_before=bytes(u.mem_read(scenario+0x218,0x3f4)).hex()
  _,b,_,row=launch_shot(m,source,typ,w,cells,initial,origin,frame)
  row['supplied']['source_flags']=4;row['supplied']['body_heading']=0;row['supplied']['source_lifecycle']='supplied Unit state; original Drive constructor and native matrix';row['supplied']['source_origin']=[2688,5248,800];row['launch']['burst_before']=burst;row['flh_calls']=flh_calls;row['rng_before']=rng_before;row['rng_after']=bytes(u.mem_read(scenario+0x218,0x3f4)).hex();rows.append(row)
  frame+=row['launch']['rearm'][1]
 return dict(native_sha256=NATIVE_SHA256,scope=__doc__,assumptions=['Partial physical type readers; UnitType concrete constructor and numbered FLH/BurstDelay readers execute','Supplied Unit source state and flat pose/body0/turret0x3fff/ownerROF1; original Drive constructor/DrawMatrix execute','Original Scenario Seed31 is supplied immediately before shot1; first call frame0 and second call at returned rearm duration4; no native Unit scheduler or intervening Bullet AI runs','Original full Weapon reader has empty sound registry; HoverMissile no muzzle Anim/Fire/Spark/Railgun systems; report sound itself not proven','Common native Bullet activation executes through native factory, Windows atomic increment supplied; ObjectUnlimbo and Display admission supplied','Stops before Sonic/post-fire tail at6FF43F; source construction/lifecycle, map placement, impact/retirement and other objects between calls remain unproved'],art_sha256=hashlib.sha256((assets_root()/'ARTMD.INI').read_bytes()).hexdigest(),flh=xyz(u,typ+0x89c),cases=rows)

def flh_controls():
 m,src,typ,w,cells,init=prepare();u=m.u;rows=[]
 cases=[('absent',{},4,1),('numbered',{'Weapon1FLH':'64,48,180'},4,1),('elite',{'Weapon1FLH':'64,48,180','EliteWeapon1FLH':'-100, +25,300tail'},4,1),('decimal_wrap',{'Weapon1FLH':'4294967297,-4294967297,2147483648'},4,1),('wrong_case',{'weapon1flh':'7,8,9'},4,1),('count_zero',{'Weapon1FLH':'7,8,9'},4,0),('ordinary',{'Weapon1FLH':'7,8,9','PrimaryFireFLH':'1,2,3'},0,1),('slot_three',{'Weapon1FLH':'1,2,3','Weapon3FLH':'4,5,6','EliteWeapon3FLH':'7,8,9'},4,3)]
 for name,keys,turret_count,weapon_count in cases:
  m.make_ini({'FV':keys});u.mem_write(typ+0x808,dwords(turret_count,weapon_count))
  for n in range(18):
   u.mem_write(typ+0x89c+n*0x1c,dwords(0,0,0));u.mem_write(typ+0xa98+n*0x1c,dwords(0,0,0))
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,typ);run_checked(u,0x715b10,0x715f9e)
  index=2 if name=='slot_three' else 0
  rows.append(dict(name=name,art=keys,turret_count=turret_count,weapon_count=weapon_count,index=index,normal=xyz(u,typ+0x89c+index*0x1c),elite=xyz(u,typ+0xa98+index*0x1c)))
 return rows


# SpawnManagerClass::AI case 0 owners: retail type and original locomotor
# constructor (Drive 4AF540, Ship 69EC50).
SPAWN_OWNERS=(('V3',0x4af540),('DRED',0x69ec50),('BSUB',0x69ec50))
SPAWN_WEAPONS=('V3Launcher','DredLauncher','BoomerTorpedo','BoomerTorpedoE','CruiseLauncher')
# Supplied owner pose: the Rust spawn fixture's cell (10,10) centre on level 4,
# PrimaryFacing and SecondaryFacing DirType 0x20.
SPAWN_ORIGIN=(2688,2688,416);SPAWN_HEADING=0x2000
NO_DRAWS=(0x68bcb0,0x65c640,0x65c660,0x65c780,0x65c7e0)

def spawn_launch(include_negative_flh=False):
 """Physical V3/DRED/BSUB types and weapons, GetFLH with a base, case 0 launch."""
 m,_,cells,init=create(False);u=m.u
 u.mem_write(0xa8ed84,dwords(0))
 for registry in (0xa8eb00,0xa83ce0):u.mem_write(registry,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 weapons={name:m.invoke(0x772fa0,m.cstring(name)) for name in SPAWN_WEAPONS}
 types={}
 for name,_ in SPAWN_OWNERS:
  typ=m.alloc(0xf00);m.invoke(0x7470d0,typ,(m.cstring(name),));types[name]=typ
 layers=[]
 for layer in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=assets_root()/layer
  if not path.exists():continue
  raw=path.read_bytes();sections,_=lexical(raw,set(SPAWN_WEAPONS)|set(types));m.rules_cache(sections)
  for w in weapons.values():m.invoke(0x772080,w,(RULES,))
  for typ in types.values():read_type_rules(u,typ)
  layers.append(dict(file=layer,sha256=hashlib.sha256(raw).hexdigest(),sections=sorted(sections)))
 # ART reader from HasTurrets to the SecondSpawnOffset store (71602E..71605B).
 art,_=lexical((assets_root()/'ARTMD.INI').read_bytes(),set(types));m.make_ini(art)
 for typ in types.values():
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,typ);run_checked(u,0x715b10,0x71605e)
 def weapon_name(p):return m.string(p+0x24) if p else None
 type_rows={name:dict(weapons=[weapon_name(m.read32(typ+0x898)),weapon_name(m.read32(typ+0x8b4))],weapon_flh=[xyz(u,typ+0x89c),xyz(u,typ+0x8b8)],second_spawn_offset=xyz(u,typ+0xdb0),turret_count=i32(u,typ+0x808)) for name,typ in types.items()}
 weapon_rows={name:dict(burst=i32(u,w+0x9c),spawner=bool(u.mem_read(w+0x131,1)[0])) for name,w in weapons.items()}
 owners={}
 for name,locomotor in SPAWN_OWNERS:
  src=m.alloc(0x1000);u.mem_write(src,dwords(0x7f5c70));u.mem_write(src+0x6c4,dwords(types[name]));u.mem_write(src+0x90,b'\x01');u.mem_write(src+0x14,dwords(4))
  heading=m.alloc(4)
  for offset in (0x388,0x3a0):
   m.invoke(0x4c91e0,src+offset,(0,));u.mem_write(heading,dwords(SPAWN_HEADING));m.invoke(0x4c9300,src+offset,(heading,))
  loco=m.alloc(0x100);m.invoke(locomotor,loco);u.mem_write(loco+0xc,dwords(src));u.mem_write(src+0x674,dwords(loco+4))
  u.mem_write(src+0x9c,dwords(*SPAWN_ORIGIN));owners[name]=src
  type_rows[name].update(locomotor_constructor=hex(locomotor),draw_matrix=hex(m.read32(m.read32(loco+4)+0x24)))
 out=m.alloc(12)
 def get_flh(control,name,index,burst,base):
  src=owners[name];u.mem_write(src+0x3b8,dwords(burst));calls=set()
  def observe(uc,a,n,d):
   if a in (0x70e140,0x6f3270,0x41be00,0x746e30,0x4c93d0,0x55a730,0x4aff60,0x69f670,0x5ae890,0x5af1a0,0x5afb80,0x7c5f00):calls.add(hex(a))
   if a in NO_DRAWS:raise AssertionError(('unexpected FLH effect',hex(a)))
  h=u.hook_add(UC_HOOK_CODE,observe)
  try:m.invoke(0x6f3ad0,src,(out,index,*base))
  finally:u.hook_del(h)
  return dict(control=control,owner=name,weapon_index=index,burst_index=burst,base=list(base),location=xyz(u,src+0x9c),coordinate=xyz(u,out),calls=sorted(calls))
 flh_rows=[get_flh('retail SecondSpawnOffset','BSUB',1,1,type_rows['BSUB']['second_spawn_offset']),
  get_flh('zero base','BSUB',1,1,(0,0,0)),
  get_flh('supplied lateral base, odd burst','DRED',0,1,(0,25,0)),
  get_flh('supplied lateral base, even burst','DRED',0,0,(0,25,0))]
 if include_negative_flh:
  # Negative GetFLH controls supply five ART-record triples, not code.
  saved_alternates=bytes(u.mem_read(types['DRED']+0x85c,60))
  alternates=[[11,22,33],[-64,25,90],[50,-80,120],[100,90,-20],[0,0,220]]
  for i,flh in enumerate(alternates):u.mem_write(types['DRED']+0x85c+12*i,dwords(*flh))
  for index in range(-1,-7,-1):
   for burst in (0,1):
    row=get_flh('supplied alternate ports','DRED',index,burst,(0,0,0))
    row['alternate_flh']=alternates
    flh_rows.append(row)
  u.mem_write(types['DRED']+0x85c,saved_alternates)
 # Case 0 from the missile-slot test (6B73C4) to the child's Unlimbo call
 # (6B7505), then the burst reset (6B757A..6B758F). Native compares only the
 # CMislType pointer (6B74BC), so a supplied identity stands for CMISL.
 cmisl=m.alloc(0x10);u.mem_write(m.read32(0x8871e0)+0x548,dwords(cmisl))
 spawn_types={'V3':m.alloc(0x10),'DRED':m.alloc(0x10),'BSUB':cmisl}
 launches=[]
 for name,slot in (('V3',0),('DRED',0),('DRED',1),('BSUB',0),('BSUB',1)):
  src=owners[name];u.mem_write(src+0x3b8,dwords(1))
  child=m.alloc(0x20);u.mem_write(child,dwords(m.alloc(0x100)))
  slots=[m.alloc(0x18) for _ in range(2)]
  for s in slots:u.mem_write(s,dwords(child,0,0,0,0,1))
  array=m.alloc(8);u.mem_write(array,dwords(*slots))
  mgr=m.alloc(0x80);u.mem_write(mgr+0x24,dwords(src,spawn_types[name]));u.mem_write(mgr+0x3c,dwords(array))
  u.mem_write(SP,bytes(0x200));u.mem_write(SP+0x14,dwords(slot))
  flh=[];visited=set()
  def observe(uc,a,n,d):
   if a==0x6f3ad0:
    sp=uc.reg_read(UC_X86_REG_ESP);flh.append(dict(weapon_index=i32(uc,sp+8),base=[i32(uc,sp+0xc),i32(uc,sp+0x10),i32(uc,sp+0x14)],burst_index=i32(uc,src+0x3b8)))
   if a in (0x70e140,0x6f3270,0x6f3ad0,0x4c93d0,0x4aff60,0x69f670):visited.add(hex(a))
   if a in NO_DRAWS+(0x7c8e17,0x421ea0):raise AssertionError(('unexpected launch effect',hex(a)))
  for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,mgr),(UC_X86_REG_EBX,slot),(UC_X86_REG_EBP,1),(UC_X86_REG_EDI,child),(UC_X86_REG_FPCW,NATIVE_FPCW)):u.reg_write(reg,v)
  h=u.hook_add(UC_HOOK_CODE,observe)
  try:run_checked(u,0x6b73c4,0x6b7505,required_addresses=(0x6f3ad0,0x4c93d0))
  finally:u.hook_del(h)
  sp=u.reg_read(UC_X86_REG_ESP);assert sp==SP-8 and u.reg_read(UC_X86_REG_ECX)==child
  row=dict(owner=name,slot=slot,cmisl_type=name=='BSUB',burst_before=1,flh_calls=flh,unlimbo_coordinate=xyz(u,m.read32(sp)),unlimbo_direction=m.read32(sp+4),burst_at_unlimbo=i32(u,src+0x3b8),reset_flag=u.mem_read(SP+0x13,1)[0],slot_status=i32(u,slots[slot]+4),calls=sorted(visited))
  # The child's Unlimbo is not executed; a CMislType launch's V3TAKOFF anim
  # block (6B750B..6B7579) is skipped to reach the same reset block.
  u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,0x6b757a if name=='BSUB' else 0x6b750b,0x6b758f)
  row['burst_after']=i32(u,src+0x3b8);launches.append(row)
 return dict(supplied=dict(origin=list(SPAWN_ORIGIN),primary_and_secondary_heading=SPAWN_HEADING,source_flags=4,binary_frame=0,veterancy='zero (rookie)',slot_is_spawned_missile=1,cmisl_type='supplied pointer identity in Rules+548 and the BSUB manager'),layers=layers,types=type_rows,weapons=weapon_rows,get_flh=flh_rows,launches=launches)


def corpus():
 result=generate();result['flh_controls']=flh_controls();result['spawn_launch']=spawn_launch();return result


def metadata():
 out=provenance(scope=__doc__,entry_points={'unit_type_ctor':0x7470d0,'numbered_flh_reader':0x715b10,'read_coord':0x529ca0,'burst_delay_reader':0x747b03,'get_flh':0x6f3ad0,'drive_ctor':0x4af540,'drive_draw_matrix':0x4aff60,'launch_to_muzzle_gate':0x6fe4f2,'get_rof':0x6fcfa0,'weapon_reader':0x772080,'ship_ctor':0x69ec50,'ship_draw_matrix':0x69f670,'spawn_launch':0x6b73c4,'spawn_burst_reset':0x6b757a},assumptions=['Physical retail strings supplied through native CRC caches; full physical file loader omitted','Partial FV type readers; original UnitType constructor/default BurstDelay loop and numbered FLH loop execute','Source Unit lifecycle/flat pose and HouseROF1 supplied; original Drive constructor and GetFLH execute','Scenario Seed31 and host shot frames0,4 supplied; no intervening world/Bullet AI or whole-world native-ID claim','HE constructor only, sound registry empty, no native impact/retirement or post6FF43F tail','spawn_launch: partial V3/DRED/BSUB type readers, full Weapon readers and the ART reader through SecondSpawnOffset execute; owner Unit state, flat pose, heading0x2000, burst index1, frame0 and spawn manager/slot memory supplied','spawn_launch: case0 runs 6B73C4 to the child Unlimbo call 6B7505 and the reset block to 6B758F; Unlimbo, the CMislType V3TAKOFF anim block and the rest of AI are not executed'],substitutions=['Original Bullet COM activation dispatched to actual6C5090 factory; Windows InterlockedIncrement supplied','ObjectUnlimbo and Display admission supplied; originalPE instructions unchanged','spawn_launch: CMislType is a supplied pointer identity (native compares pointers only); the CMislType anim block is skipped by resuming at 6B757A'])
 out['physical_files']=[dict(name=p.name,sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map','ARTMD.INI') if (p:=assets_root()/name).exists()]
 return out

if __name__=='__main__':
 finish_vectors(corpus,Path(__file__).with_suffix('.json'),provenance=metadata)
