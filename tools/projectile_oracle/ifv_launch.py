"""Research only. Original physical FV weapon selection and guided launch.

Source Unit lifecycle/FLH and world admission are supplied boundaries.
The separate ifv_fire_coord fixture executes bounded flat muzzle/rearm paths.
This is not a whole FireAt or whole-scenario identity proof.
"""
import json,struct,sys,hashlib
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import NATIVE_FPCW,RET_MAGIC,run_checked,finish_vectors,provenance
from tools.projectile_oracle.bridge_render_inputs import assets_root,lexical
from tools.spatial_oracle.building_body_rules import RULES,SP,dwords

from tools.projectile_oracle import guided_step as guided
i32,xyz,vec=guided.i32,guided.xyz,guided.vec

def bullet_addref_transport(m,address,events):
 """Verified Bullet IUnknown Windows InterlockedIncrement boundary."""
 if address!=0x46afe5:return False
 u=m.u;sp=u.reg_read(UC_X86_REG_ESP);ptr=m.read32(sp);value=(m.read32(ptr)+1)&0xffffffff
 u.mem_write(ptr,dwords(value));u.reg_write(UC_X86_REG_EAX,value)
 u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,0x46afeb)
 events.append(dict(call='KERNEL32.InterlockedIncrement',com_refcount=value))
 return True

def bullet_activation_transport(m, address, events):
 """Windows COM activation at the existing Bullet creation call sites.

 Only activation is supplied: registered factory6C5090 and Bullet constructors
 execute unchanged. Reuse the five COM argument words for the factory ABI.
 """
 if address not in (0x46b072, 0x46a564, 0x46a9e2):return False
 u=m.u;sp=u.reg_read(UC_X86_REG_ESP)
 assert bytes(u.mem_read(address,6))==bytes.fromhex('ff15fc157e00')
 clsid,outer,context,iid,ppv=struct.unpack('<5I',u.mem_read(sp,20))
 assert (clsid,outer,context,iid)==(0x7e96e0,0,7,0x7f7c90)
 events.append(dict(call='CoCreateInstance activation to original registered factory',clsid_hex=bytes(u.mem_read(clsid,16)).hex(),iid_hex=bytes(u.mem_read(iid,16)).hex()))
 u.mem_write(sp,dwords(address+6,0,outer,iid,ppv));u.reg_write(UC_X86_REG_EIP,0x6c5090)
 return True

def bullet_world_admission_transport(m,address,bullet,events):
 """Supply world admission while retaining original Bullet coordinate fixup.

 Native Unlimbo's prologue saves its ABI state. Admission guards are supplied
 at5F4ECD; the original5F4F4A..5F4FAE type getter, virtual+6C fixup and
 SetLocation then execute, including the original+80/+81 admission writes.
 This corrects the former raw-copy boundary that left the Bullet InLimbo and
 suppressed its later Conceal/DetachAll cleanup. Post-placement world effects remain the existing
 supplied boundary, returning through original5F5210. Display calls retain
 their prior supplied admission. No native coordinate result is computed here.
 """
 u=m.u
 if address==0x5f4ec0:
  assert u.reg_read(UC_X86_REG_ECX)==bullet
  assert m.read32(bullet)==0x7e46e4
  events.append(dict(call=hex(address),supplied_world_admission=True))
  return True
 if address in (0x5f4ecd,0x5f4fae) and u.reg_read(UC_X86_REG_ESI)==bullet:
  u.reg_write(UC_X86_REG_EIP,0x5f4f4a if address==0x5f4ecd else 0x5f5210)
  return True
 if address in (0x4a9770,0x4a9720):
  events.append(dict(call=hex(address),supplied_world_admission=True));m.ret(1,4)
  return True
 return False

def prepare():
 m,_,cells,initial=guided.create(False);u=m.u
 u.mem_write(0xa8ed84,dwords(0))
 u.mem_write(0xa8eb00,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 st=m.alloc(0xe00);m.invoke(0x710af0,st,(m.cstring('FV'),))
 type_layers=[]
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=assets_root()/name
  if not path.exists():continue
  sections,_=lexical(path.read_bytes(),{'FV'});m.rules_cache(sections)
  for begin,end in ((0x71284a,0x712a8f),(0x71338b,0x7133c8),(0x7147b4,0x7147ce),(0x714a49,0x714a63),(0x714016,0x714030)):
   for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,st),(UC_X86_REG_EBX,st+0x24),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EDI,RULES),(UC_X86_REG_EAX,u.mem_read(st+0xd22,1)[0])):u.reg_write(reg,v)
   run_checked(u,begin,end)
  type_layers.append(dict(file=name,turret_count=i32(u,st+0x808),weapon_count=i32(u,st+0x80c),turret=bool(u.mem_read(st+0xca1,1)[0]),gunner=bool(u.mem_read(st+0x805,1)[0]),radial_fire_segments=i32(u,st+0x6a4),weapons=[m.string(m.read32(st+0x898+n*0x1c)+0x24) for n in range(i32(u,st+0x80c))]))
 source=m.alloc(0x1000);u.mem_write(source,dwords(0x7f5c70));u.mem_write(source+0x6c4,dwords(st));u.mem_write(source+0x2b4,dwords(cells[16,20]));u.mem_write(source+0x90,b'\x01')
 # Deliberately not a native Unit constructor/placement proof. Flags=0 excludes
 # moving Foot locomotor arm, and supplied source coordinates/heading are static.
 m.invoke(0x4c91e0,source+0x3a0,(0,));heading=m.alloc(4);u.mem_write(heading,dwords(0x3fff));m.invoke(0x4c9300,source+0x3a0,(heading,))
 u.reg_write(UC_X86_REG_ESI,source);u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,0x735678,0x735691,required_addresses=(0x70dc70,))
 slot=m.invoke(0x6f3330,source,(cells[16,20],));weapon_ref=m.invoke(0x70e140,source,(slot,));w=m.read32(weapon_ref)
 assert m.string(w+0x24)=='HoverMissile'
 wh=m.read32(w+0xac);warhead_layers=[]
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=assets_root()/name
  if not path.exists():continue
  sections,_=lexical(path.read_bytes(),{'HE'});m.rules_cache(sections);admitted=m.invoke(0x75d3a0,wh,(RULES,))&255
  warhead_layers.append(dict(file=name,keys=sections.get('HE'),reader_admitted=bool(admitted),cell_spread_bits=f'{m.read32(wh+0x124):08x}',percent_at_max_bits=f'{m.read32(wh+0x12c):08x}',wall=bool(u.mem_read(wh+0x144,1)[0]),em_effect=bool(u.mem_read(wh+0x154,1)[0])))
 return m,source,st,w,cells,dict(**initial,type_layers=type_layers,warhead_layers=warhead_layers,selected_weapon_slot=slot)

def launch(origin=(2688,5248,1030),bridge=True):
 m,source,st,w,cells,initial=prepare();u=m.u;p=m.read32(w+0xa0);target=cells[16,20]
 for xy,c in cells.items():u.mem_write(c+0x140,dwords(0x100 if bridge and xy==(10,20) else 0))
 u.mem_write(source+0x9c,dwords(*origin));scratch=m.alloc(12);m.invoke(0x486890,target,(scratch,));target_xyz=xyz(u,scratch)
 u.mem_write(SP,bytes(0x2000))
 for ptr,values in ((SP+0x40,[w]),(SP+0x44,origin),(SP+0x68,[p]),(SP+0x88,target_xyz),(SP+0x1000+8,[target,0])):u.mem_write(ptr,dwords(*values))
 for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,SP+0x1000),(UC_X86_REG_ESI,source),(UC_X86_REG_EDI,m.read32(w+0xa4)),(UC_X86_REG_FPCW,NATIVE_FPCW)):u.reg_write(reg,value)
 scenario=m.read32(0xa8b230);events=[];bullet=[];visited=set();last=[]
 id_before=i32(u,scenario+0x214);scalars={}
 def observe(uc,a,n,d):
  if a>=0x20000000:raise AssertionError(('non-image-code',hex(a),'trace',last,'events',events,'bullet',bullet))
  last.append(hex(a));del last[:-15]
  if a in (0x773070,0x46b050,0x6c5090,0x466380,0x410230,0x68bcb0,0x4664c0,0x70bcb0,0x740f80,0x70e140,0x7177c0,0x70d590,0x468670,0x4e1130):visited.add(hex(a))
  if a in (0x65c640,0x65c660,0x65c780,0x65c7e0):raise AssertionError(('unexpected launch RNG',hex(a)))
  if a in (0x7258d0,0x5f65f0,0x466560,0x5f3b80):raise AssertionError(('unexpected launch detach/retirement',hex(a)))
  if bullet_activation_transport(m,a,events):pass
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
 try:run_checked(u,0x6fe4f2,(0x6ff01a,0x6ff751,0x6ff93c),count=1000000,required_addresses=(0x6c5090,0x466380,0x68bcb0,0x70bcb0,0x740f80,0x468670))
 except Exception:
  print('LAUNCH FAILED',last,'ECX',hex(u.reg_read(UC_X86_REG_ECX)),file=sys.stderr);raise
 finally:u.hook_del(h)
 assert u.reg_read(UC_X86_REG_EIP)==0x6ff01a,hex(u.reg_read(UC_X86_REG_EIP))
 b=bullet[0]
 result=dict(initial=initial,supplied=dict(origin=list(origin),source_heading=0x3fff,source_flags=0,weapon_slot=0,target_cell=[16,20],live_bridge_cell=[10,20] if bridge else None,binary_frame=0),launch=dict(position=xyz(u,b+0x9c),target=target_xyz,velocity=vec(u,b+0xe8),**scalars,maximum_speed=i32(u,b+0x110),pitch=m.read32(SP+0x80)&0xffff,unique_id=i32(u,b+0x10),scenario_id_before=id_before,scenario_id_after=i32(u,scenario+0x214),course_locked=bool(u.mem_read(b+0x105,1)[0]),course_frames=i32(u,b+0x108),detector=dict(first_timer_start=i32(u,b+0xb8),first_timer_duration=i32(u,b+0xc0),arm_start=i32(u,b+0xc4),arm_duration=i32(u,b+0xcc),reference=xyz(u,b+0xd0),distance_watermark=i32(u,b+0xdc)),warhead=m.string(m.read32(b+0x128)+0x24),calls=sorted(visited),supplied_calls=events))
 return m,b,cells,result

def flight(bridge,origin=(2688,5248,1030),bridge_band=None,changes=()):
 m,b,cells,result=launch(origin=origin,bridge=bridge);u=m.u;frames=[];last=[];visits=set();impact=[]
 if bridge_band is not None:
  for xy,c in cells.items():u.mem_write(c+0x140,dwords(0x100 if xy in bridge_band else 0))
  result['supplied']['bridge_band']=[list(xy) for xy in bridge_band]
 result['supplied']['between_frame_flag_changes']=[dict(frame=f,live=live) for f,live in changes]
 def obs(uc,a,n,d):
  last.append(hex(a));del last[:-15]
  if a>=0x20000000 and a!=RET_MAGIC:raise AssertionError(('non-image-code',hex(a),last))
  if a in (0x65c640,0x65c660,0x65c780,0x65c7e0):raise AssertionError(('unexpected RNG before impact sink',hex(a)))
  if a in (0x7258d0,0x5f65f0,0x466560,0x5f3b80):raise AssertionError(('unexpected detach/retirement before impact sink',hex(a)))
  if a in (0x5f3e70,0x5b20f0,0x467032,0x4670b2,0x5f5850,0x5f6940,0x468bb0,0x4e11f0,0x468d80,0x4690b0,0x489280):visits.add(hex(a))
  if a==0x4690b0:
   sp=u.reg_read(UC_X86_REG_ESP);impact.append(dict(position=xyz(u,m.read32(sp+4)),retained=xyz(u,b+0x9c),arm_timer=[i32(u,b+0xc4),i32(u,b+0xcc)]))
 h=u.hook_add(UC_HOOK_CODE,obs)
 try:
  for frame in range(1,121):
   for change_frame,live in changes:
    if frame==change_frame:
     for xy in bridge_band:u.mem_write(cells[xy]+0x140,dwords(0x100 if live else 0))
   visits.clear();u.mem_write(0xa8ed84,dwords(frame));u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,b)
   run_checked(u,0x4666e0,(RET_MAGIC,0x489280),count=1000000)
   # Endpoint hook need not run; capture the original handoff argument directly.
   if u.reg_read(UC_X86_REG_EIP)==0x489280:
    sp=u.reg_read(UC_X86_REG_ESP);src,wh,tiberium,house=struct.unpack('<4I',u.mem_read(sp+4,16))
    result['area_damage_handoff']=dict(position=xyz(u,u.reg_read(UC_X86_REG_ECX)),damage=u.reg_read(UC_X86_REG_EDX),source_present=src==m.read32(b+0xb0),warhead=m.string(wh+0x24),affects_resource=tiberium,house_is_null=house==0)
   frames.append(dict(frame=frame,position=xyz(u,b+0x9c),velocity=vec(u,b+0xe8),course_locked=bool(u.mem_read(b+0x105,1)[0]),course_frames=i32(u,b+0x108),closing_count=i32(u,b+0x118),closing_accum=struct.unpack('<d',u.mem_read(b+0x120,8))[0],calls=sorted(visits),stopped_at=hex(u.reg_read(UC_X86_REG_EIP))))
   if impact:break
 except Exception:
  print('FLIGHT FAILED',frame,last,'ECX',hex(u.reg_read(UC_X86_REG_ECX)),file=sys.stderr);raise
 finally:u.hook_del(h)
 result['frames']=frames;result['impact']=impact;return result

def generate():
 band=[(x,20) for x in range(12,16)]
 return dict(scope='Physical FV selected empty-Gunner slot and original contiguous FireAt6FE4F2 through native factory/launch, then full Bullet AI and impact resolution to AreaDamage489280 entry; supplied source lifecycle/FLH/placement/Display admission/map cells and flag changes. No AreaDamage effects, native bridge collapse/repair driver or retirement execution.',cases=[flight(True),flight(False),flight(False,origin=(2688,5248,1250),bridge_band=band),flight(False,origin=(2688,5248,1250),bridge_band=band,changes=((16,False),)),flight(False,origin=(2688,5248,1250),bridge_band=band,changes=((16,False),(24,True)))])
def metadata():
 out=provenance(scope='Five original FV guided launches and125 full Bullet AI visits through AreaDamage489280 entry',entry_points={'select_weapon':0x6f3330,'set_empty_gunner':0x70dc70,'fireat_launch':0x6fe4f2,'com_factory':0x6c5090,'bullet_ctor':0x466380,'bullet_ai':0x4666e0,'area_damage_entry':0x489280},assumptions=[
  'Physical HoverMissile and AAHeatSeeker2 full type readers and selected partial FV rules fields; source lifecycle, FLH/pose and fixed launch coordinates supplied',
  'FV type here is base TechnoType only; do not extend this allocation to UnitType GetROF reads at+E48. Separate ifv_fire_coord executes actual UnitType and BurstDelay readers.',
  'Original source empty-gunner constructor slice runs; slot0 HoverMissile selected by original6F3330/70E140',
  'Original full HE reader executes; native sound registry is empty; world source admission/Unit AI/shot schedule/muzzle/report are outside this fixture',
  'Mapped flat level6 Cell objects and live structural-bit100 changes supplied; native bridge collapse/repair drivers not executed',
  'Original whole Bullet AI executes125 visits over5cases, stopping before AreaDamage; effects, impact animations and object retirement are covered separately',
  'Partial fixture native cursor28 produces Bullet29; this is not a whole-scenario prefix guarantee. Native x87 PC53/chop0E7F.',
 ],substitutions=[
  'Inherited BulletReader supplies CRC INI caches, allocation, CRT TLS and archive IO',
  'COM activation dispatched to original6C5090 factory; Windows InterlockedIncrement supplied',
  'ObjectUnlimbo and Display admission supplied; hooks observe flight and reject unexpected RNG/detach',
 ])
 out['physical_files']=[dict(name=p.name,sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map','ARTMD.INI') if (p:=assets_root()/name).exists()]
 out['research_payload_sha256']='1082e8902b818465e5047e3517ed55808f49a24c0f9aba7d8a9035cd056c1a31'
 return out

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
