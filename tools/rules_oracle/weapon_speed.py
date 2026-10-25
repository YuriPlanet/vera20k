"""Original Weapon Speed readers, postpass controls and selected retail inputs.
Supplied native signed-CRC INI caches, allocator/TLS and missing archive boundary
are inherited from bridge_render_inputs.BulletReader. No physical INI loader.
"""
import hashlib,json,struct,sys
from pathlib import Path
from unicorn.x86_const import *
from tools.native_oracle import run_checked,finish_vectors,provenance
from tools.projectile_oracle.bridge_render_inputs import BulletReader,assets_root,lexical
from tools.projectile_oracle.bridge_render_inputs_selection import initialize_weapon
from tools.spatial_oracle.building_body_rules import RULES,SP,dwords

def s32(m,p):return struct.unpack('<i',m.u.mem_read(p,4))[0]
def fresh(name='SpeedProbe',art=None):
 m=BulletReader(art or {});m.u.mem_write(0x887568,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 p=m.invoke(0x772fa0,m.cstring(name));return m,p

def controls():
 m,p=fresh();ctor=s32(m,p+0xa8);rows=[]
 for raw in (None,'-1','-2','-2147483648','0','1','7','30','40','50','99','100','101','255','65536','2147483647','2147483648','4294967295','4294967296','40junk','junk','$28','28h','$FFFFFFFF','FFFFFFFFh','','   ',' 40 ', '+40','speed-key-case'):
  m.u.mem_write(p+0xa8,dwords(177))
  keys={} if raw is None else {'speed' if raw=='speed-key-case' else 'Speed': '40' if raw=='speed-key-case' else raw}
  # AmbientDamage retains an admitted section even for missing Speed.
  keys['AmbientDamage']='1';m.rules_cache({'SpeedProbe':keys})
  m.invoke(0x772080,p,(RULES,));rows.append(dict(raw=raw,supplied_retained_default=177,native_dword=s32(m,p+0xa8)))
 history=[]
 for keys in ({'Speed':'40'}, {'Speed':'-1'}, {'AmbientDamage':'2'}, {'Speed':'30'}, {'Speed':'100'}, {'Speed':'-2'}):
  m.rules_cache({'SpeedProbe':keys});m.invoke(0x772080,p,(RULES,));history.append(dict(keys=keys,result=s32(m,p+0xa8)))
 return dict(ctor_speed=ctor,cached_reader_controls=rows,sequential_history=history)

def physical():
 root=assets_root();names={'FV','HoverMissile','AAHeatSeeker2','AudioVisual','General'}
 art,_=lexical((root/'ARTMD.INI').read_bytes(),{'AAHeatSeeker2','DRAGON','FV'})
 m,w=fresh('HoverMissile',art);u=m.u;rr=m.alloc(0x2000)
 u.mem_write(0x8871e0,dwords(rr));u.reg_write(UC_X86_REG_ESI,rr);run_checked(u,0x6674d6,0x6674e0)
 rows=[]
 for f in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=root/f
  if not path.exists():rows.append(dict(file=f,absent=True));continue
  raw=path.read_bytes();sections,_=lexical(raw,names);m.rules_cache(sections)
  m.invoke(0x772080,w,(RULES,));p=m.read32(w+0xa0);m.invoke(0x46bee0,p,(RULES,))
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,rr);u.reg_write(UC_X86_REG_EDI,RULES);run_checked(u,0x66b3c4,0x66b3e4)
  before=s32(m,w+0xa8);m.invoke(0x7729f0,w)
  fields={k:s32(m,p+v) for k,v in {'rot':0x2dc,'acceleration':0x2d0,'course_lock_duration':0x2e0,'arm':0x2f0}.items()}
  fields.update({k:bool(u.mem_read(p+v,1)[0]) for k,v in {'arcing':0x29b,'inviso':0x29e,'dropping':0x29c,'vertical':0x2c0,'airburst':0x294,'inaccurate':0x2a2,'very_high':0x299,'level':0x29d,'proximity':0x29f,'ranged':0x2a0,'aa':0x2a4,'ag':0x2a5}.items()})
  rows.append(dict(file=f,sha256=hashlib.sha256(raw).hexdigest(),raw_sections=sections,reader_speed=before,postpass_speed=s32(m,w+0xa8),get_speed500=m.invoke(0x773070,w,(500,)),range_leptons=s32(m,w+0xb4),type=fields,art=art,gravity=s32(m,rr+0x16b8)))
 # Native Bullet ctor owns course lock/count defaults.
 u.mem_write(0xa8ed40,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10));b=m.alloc(0x180);m.invoke(0x466380,b)
 ctor=dict(course_locked=bool(u.mem_read(b+0x105,1)[0]),course_frames=s32(m,b+0x108),max_speed=s32(m,b+0x110))
 # FireAt guided scalar reset/store block. Upstream RadialFireSegments=0 and
 # this exact reader-produced Weapon and type; no launch vector claim.
 u.mem_write(SP+0x28,dwords(99));u.mem_write(SP+0x3c,dwords(b));u.mem_write(SP+0x68,dwords(0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBX,w)
 run_checked(u,0x6fea18,0x6fea52)
 launch=dict(supplied_preclamped_amount=99,radial_fire_segments=0,amount=s32(m,SP+0x28),max_speed=s32(m,b+0x110))
 # Original guided acceleration only: incoming cardinal |v|=1 stands for
 # post-Fire unit normalization. No trajectory/steering/collision commits.
 u.mem_write(b+0xac,dwords(p));u.mem_write(b+0xe8,struct.pack('<3d',1.,0.,0.));ramps=[]
 for frame in range(1,39):
  u.mem_write(0xa8ed84,dwords(frame));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,b)
  run_checked(u,0x4668d9,0x466b0c)
  v=bytes(u.mem_read(b+0xe8,24));ramps.append(dict(frame=frame,velocity=list(struct.unpack('<3d',v)),velocity_bits=[f'{x:016x}' for x in struct.unpack('<3Q',v)],course_locked=bool(u.mem_read(b+0x105,1)[0]),course_frames=s32(m,b+0x108)))
 return dict(layers=rows,bullet_ctor=ctor,guided_scalar_launch=launch,cardinal_acceleration_only=ramps)

def postpass_history():
 m,w=fresh('RetainedSpeed');u=m.u;r=m.alloc(0x2000);u.mem_write(0x8871e0,dwords(r));u.mem_write(r+0x16b8,dwords(6))
 rows=[]
 for label,ws,bs in (
  ('base_ballistic',{'Projectile':'RetainedBullet','Speed':'40','Range':'5'},{'ROT':'0'}),
  ('map_guided_absent_speed',{'AmbientDamage':'2'},{'ROT':'60'}),
  ('map_guided_minus_one_speed',{'Speed':'-1'},{'ROT':'60'}),
  ('map_guided_authored_speed',{'Speed':'40'},{'ROT':'60'}),
  ('map_back_ballistic_new_range',{'Range':'10'},{'ROT':'0'}),
  ('map_negative_rot_absent_speed',{'AmbientDamage':'3'},{'ROT':'-1'})):
  m.rules_cache({'RetainedSpeed':ws,'RetainedBullet':bs});m.invoke(0x772080,w,(RULES,));p=m.read32(w+0xa0);m.invoke(0x46bee0,p,(RULES,))
  before=s32(m,w+0xa8);m.invoke(0x7729f0,w)
  rows.append(dict(label=label,weapon_keys=ws,bullet_keys=bs,read_speed=before,postpass_speed=s32(m,w+0xa8),get_speed500=m.invoke(0x773070,w,(500,))))
 return rows

def cannon():
 m=BulletReader({});w,p,r=initialize_weapon(m);m.u.mem_write(0x8871e0,dwords(r))
 before=s32(m,w+0xa8);m.invoke(0x7729f0,w)
 return dict(reader_speed=before,range_leptons=s32(m,w+0xb4),gravity=s32(m,r+0x16b8),postpass_speed=s32(m,w+0xa8),get_speed500=m.invoke(0x773070,w,(500,)))

def generate():
 return dict(controls=controls(),physical_ifv=physical(),ordinary_cannon=cannon(),postpass_history=postpass_history())

def metadata():
 return provenance(scope='Original retained Weapon ReadSpeed, postpass components and physical IFV scalar inputs', assumptions=[
  'Full original Weapon771C70/772080 and Bullet46BBC0/46BEE0 execute from supplied cached INI objects. Thirty raw Speed controls cover sentinel, clamp, signed overflow, prefix, hexadecimal, empty and case behavior; sequential history retains the same native object.',
  'Physical RULESMD/LANGRULE/MPBattleMD/Hills keys are source bytes parsed into the supplied native cache. Reader order here intentionally reads Gravity before Weapon postpass: this is a component fixture, NOT full Rules Process chronology. See weapon_speed_order.py for original full Process ordering. The selected guided ROT60 speed is unaffected by Gravity.',
  'Cannon and retained postpass history supply Gravity6. Original postpass7729F0 and GetSpeed773070 execute; no Python formula supplies expected speed.',
  'Guided scalar launch executes6FEA18..6FEA52 with supplied prior amount99 and RadialFireSegments0. Cardinal acceleration executes4668D9..466B0C from supplied velocity1,0,0 for38visits. These are component controls, not full launch or trajectory comparisons.',
  'Native x87 fixture selects53-bit precision and truncation0E7F.',
 ], substitutions=[
  'BulletReader supplies INI cache objects, allocator/delete/CRT/TLS and absent archive boundaries. No physical INI loader, scenario loader, full Type discovery, world admission, receiver or rendering claim.',
 ], entry_points={'weapon_ctor':0x771c70,'weapon_reader':0x772080,'read_speed':0x474810,'weapon_postpass':0x7729f0,'get_speed':0x773070,'bullet_ctor':0x466380,'bullet_reader':0x46bee0,'guided_ramp':0x4668d9})

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
