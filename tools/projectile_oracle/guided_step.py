"""Original guided step through strict bridge contact before coordinate commit.
Supplied map/source/target/incoming flight and the precommit boundary are explicit.
"""
import hashlib,json,struct,sys
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import run_checked,finish_vectors,provenance
from tools.projectile_oracle.bridge_render_inputs import BulletReader,assets_root,lexical
from tools.spatial_oracle.building_body_rules import RULES,SP,dwords

def i32(u,p):return struct.unpack('<i',u.mem_read(p,4))[0]
def vec(u,p):
 raw=bytes(u.mem_read(p,24));return dict(value=list(struct.unpack('<3d',raw)),bits=[f'{v:016x}' for v in struct.unpack('<3Q',raw)])
def xyz(u,p):return list(struct.unpack('<3i',u.mem_read(p,12)))
def create(construct_bullet=True, *, reader=None):
 root=assets_root();art,_=lexical((root/'ARTMD.INI').read_bytes(),{'AAHeatSeeker2','DRAGON','FV'})
 # A composed witness may supply the same reader owner with additional fixed
 # ART sections. World construction and all original initializers stay here.
 m=reader if reader is not None else BulletReader(art);u=m.u
 # Verified C initializer table members for Bullet, Cell, and homing helper.
 initializers=[]
 for base,count in ((0x8127f4,13),(0x8129fc,13),(0x813c2c,13)):
  for addr in struct.unpack('<'+'I'*count,u.mem_read(base,count*4)):
   m.invoke(addr,0);initializers.append(hex(addr))
 u.mem_write(0x887568,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 w=m.invoke(0x772fa0,m.cstring('HoverMissile'));r=m.alloc(0x2000);u.mem_write(0x8871e0,dwords(r))
 u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EBX,0);run_checked(u,0x665dac,0x665dd3);run_checked(u,0x6674d6,0x6674e0)
 layers=[]
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=root/name
  if not path.exists():layers.append(dict(file=name,absent=True));continue
  raw=path.read_bytes();sections,_=lexical(raw,{'HoverMissile','AAHeatSeeker2','General'})
  m.rules_cache(sections);m.invoke(0x772080,w,(RULES,));p=m.read32(w+0xa0);m.invoke(0x46bee0,p,(RULES,))
  for start,end in ((0x66b3c4,0x66b3e4),(0x66ec1b,0x66ec89)):
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EDI,RULES);run_checked(u,start,end)
  m.invoke(0x7729f0,w);layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),speed=i32(u,w+0xa8),rot=i32(u,p+0x2dc),acceleration=i32(u,p+0x2d0),missile_rot_var=struct.unpack('<d',u.mem_read(r+0x598,8))[0],safety_altitude=i32(u,r+0x5a0)))
 # Supplied mapped flat cells; native target getter, lookups and floor execute.
 table=0x26000000;u.mem_map(table,0x100000);u.mem_write(0x87f924,dwords(table,262144))
 cells={}
 for y in range(16,25):
  for x in range(6,25):
   c=m.alloc(0x160);cells[x,y]=c;u.mem_write(c,dwords(0x7e4eec));u.mem_write(c+0x24,struct.pack('<2h',x,y));u.mem_write(c+0x11b,bytes((6,0)));u.mem_write(table+(y*512+x)*4,dwords(c))
 u.mem_write(0xabdc50,dwords(0x7e4eec))
 u.mem_write(0xa8ed40,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 b=0
 if construct_bullet:
  b=m.alloc(0x180);m.invoke(0x466380,b);m.invoke(0x4664c0,b,(p,cells[12,20],0,25,m.read32(w+0xac),m.read32(w+0xa8),0));u.mem_write(b+0x130,dwords(w))
 initial=dict(initializers=initializers,globals={hex(a):i32(u,a) for a in (0x89de70,0x89de64,0x89e7c0,0xabef50,0xabef44,0x89e7b4)},layers=layers,bullet_ctor_unique_id=i32(u,b+0x10) if b else None)
 return m,b,cells,initial

def generate():
 m,b,cells,initial=create();u=m.u;fresh=bytes(u.mem_read(b,0x180));rows=[]
 cases=[
  ('rise_live',[2688,5248,1039],[0.,0.,1.],(12,20),[(10,20)]),
  ('rise_cleared',[2688,5248,1039],[0.,0.,1.],(12,20),[]),
  ('fall_live',[2688,5248,1041],[0.,0.,-1.],(12,20),[(10,20)]),
  ('old_equal',[2688,5248,1040],[0.,0.,1.],(12,20),[(10,20)]),
  ('new_equal',[2688,5248,1036],[0.,0.,1.],(12,20),[(10,20)]),
  ('far_clear',[2688,5248,800],[1.,0.,0.],(16,20),[]),
  ('far_live',[2688,5248,800],[1.,0.,0.],(16,20),[(10,20)]),
  ('cruise_height_clear',[2688,5248,1250],[1.,0.,0.],(16,20),[]),
  ('cruise_height_live',[2688,5248,1250],[1.,0.,0.],(16,20),[(10,20)]),
  ('cruise_cross_clear',[2688,5248,1030],[1.,0.,0.],(16,20),[]),
  ('cruise_cross_live',[2688,5248,1030],[1.,0.,0.],(16,20),[(10,20)]),
  ('old_only',[2815,5248,1039],[1.,0.,1.],(13,20),[(10,20)]),
  ('new_only',[2815,5248,1039],[1.,0.,1.],(13,20),[(11,20)]),
  ('phase_frame6',[2688,5248,800],[1.,0.,0.],(16,20),[]),
  ('phase_id105',[2688,5248,800],[1.,0.,0.],(16,20),[]),
  ('phase_negative_id',[2688,5248,800],[1.,0.,0.],(16,20),[]),
  ('phase_wrap_sum',[2688,5248,800],[1.,0.,0.],(16,20),[]),
  ('phase_min_frame',[2688,5248,800],[1.,0.,0.],(16,20),[]),
  ('phase_negative_frame',[2688,5248,800],[1.,0.,0.],(16,20),[]),
 ]
 for name,old,velocity,target,bridge in cases:
  u.mem_write(b,fresh)
  for xy,c in cells.items():u.mem_write(c+0x140,dwords(0x100 if xy in bridge else 0))
  frame,identity={
   'phase_frame6':(6,100),'phase_id105':(1,105),
   'phase_negative_id':(1,-100),'phase_wrap_sum':(1,2147483647),
   'phase_min_frame':(-2147483648,100),'phase_negative_frame':(-123,100),
  }.get(name,(1,100))
  u.mem_write(b+0x10,dwords(identity));u.mem_write(b+0x10c,dwords(cells[target]));u.mem_write(b+0x9c,dwords(*old));u.mem_write(b+0xe8,struct.pack('<3d',*velocity));u.mem_write(0xa8ed84,dwords(0));
  reference=m.alloc(12);m.invoke(0x486890,cells[target],(reference,));m.invoke(0x4e1130,b+0xb8,(b+0x9c,reference,2,0x7fffffff));u.mem_write(0xa8ed84,dwords(frame));u.mem_write(SP,bytes(0x200));u.mem_write(SP+0x24,dwords(*old))
  snapshots=[];queries=[];visited=set();turns=[]
  def obs(uc,a,n,d):
   if a in (0x65c640,0x65c660,0x65c780,0x65c7e0):raise AssertionError(('unexpected RNG',hex(a)))
   if a in (0x466b0c,0x466d36,0x466db1,0x467032,0x467b75):
    snapshots.append(dict(pc=hex(a),candidate=xyz(u,SP+0x24),velocity=vec(u,b+0xe8),mode=i32(u,SP+0x20),impact=u.mem_read(SP+0x18,1)[0]))
   if a in (0x565730,0x578080):
    ptr=m.read32(u.reg_read(UC_X86_REG_ESP)+4)
    queries.append(dict(pc=hex(a),xyz=xyz(u,ptr)))
   if a==0x5b20f0:
    sp=u.reg_read(UC_X86_REG_ESP);turns.append(dict(word=struct.unpack('<H',u.mem_read(m.read32(sp+8),2))[0],target=xyz(u,m.read32(sp+4))))
   if a in (0x5b20f0,0x5b2350,0x5b263e,0x5b274d,0x5b275c,0x5b289c,0x5b2a30,0x4670b2):visited.add(hex(a))
  hook=u.hook_add(UC_HOOK_CODE,obs)
  try:
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,b);run_checked(u,0x4668bd,0x467b7a,required_addresses=(0x5b20f0,0x467032))
  finally:u.hook_del(hook)
  rows.append(dict(name=name,supplied=dict(old=old,velocity=velocity,target_cell=list(target),bridge_cells=[list(c) for c in bridge],level=6,slope=0,source=None,binary_frame=frame,unique_id=identity),candidate=xyz(u,SP+0x24),velocity=vec(u,b+0xe8),mode=i32(u,SP+0x20),impact=u.mem_read(SP+0x18,1)[0],course_locked=bool(u.mem_read(b+0x105,1)[0]),course_frames=i32(u,b+0x108),closing_count=i32(u,b+0x118),closing_accum=struct.unpack('<d',u.mem_read(b+0x120,8))[0],queries=queries,visited=sorted(visited),turns=turns,snapshots=snapshots,arm_timer=[i32(u,b+0xc4),i32(u,b+0xcc)]))
  candidate=m.alloc(12);u.mem_write(candidate,dwords(*rows[-1]['candidate']));rows[-1]['separate_proximity_check_during_arm']=m.invoke(0x4e11f0,b+0xb8,(candidate,))
 return dict(scope='Full original ramp, steering and bridge contact before native world commit; supplied in-flight state and mapped flat cells, no launch/damage/render proof',initial=initial,rows=rows)
def metadata():
 return provenance(scope='Original guided ramp, HomingTrack, phase and strict live bridge crossing before world commit',assumptions=[
  'Nineteen supplied incoming-flight rows execute original4668BD..467B7A, full HomingTrack5B20F0 and bridge467032. No arithmetic/steering/contact result is computed by Python. ProximityDetector4E1130/4E11F0 additionally executes the still-armed contact check.',
  'The selected physical HoverMissile/AAHeatSeeker2 and ART reader components execute; source input files are hashed. Type discovery, full Rules Process chronology, FireAt launch, world commit, damage, collapse/repair, retirement and rendering are outside this fixture.',
  'Original CRT initializers establish Bullet/Cell/Homing helper globals. Supplied mapped flat level6 cells use native Cell virtual getters, Map lookups and ground-height calculation. Live bit100 flags are fixture inputs, not native collapse/repair outputs.',
  'Phase controls execute signed32 ID plus binary-frame wrap and signed remainder through the original trig table and ftol; negative IDs and binary frames are supplied explicitly.',
  'Native x87 fixture uses53-bit precision and truncation0E7F.',
 ],substitutions=[
  'Inherited BulletReader supplies signed-CRC INI caches, allocator/CRT/TLS/archive boundaries. Map cells, initial XYZ/velocity/IDs/frame, no-source state and structural flags are supplied. Hooks only observe and reject unexpected RNG.',
 ],entry_points={'bullet_ctor':0x466380,'guided_ai':0x4668bd,'homing_track':0x5b20f0,'bridge_contact':0x467032,'proximity_setup':0x4e1130,'proximity_check':0x4e11f0})

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
