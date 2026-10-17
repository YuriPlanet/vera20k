"""Bounded physical Anytown MTNK firing, concrete low bridge damage and repair.

Original layered selected readers, full Unit/Techno FireAt, Bullet/Anim AI,
Detonate/AreaDamage, damage/Recalc/empty occupants and admitted repair execute.
Source lifecycle, inter-frame schedule and world/display/audio admission are
explicit supplied boundaries. This is not an ordinary mission shot-count oracle.
"""
from pathlib import Path
import hashlib,json,os,struct,sys
from collections import deque
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from capstone import Cs,CS_ARCH_X86,CS_MODE_32
from tools.native_oracle import NATIVE_SHA256,RET_MAGIC,run_checked,provenance
from tools.projectile_oracle.bridge_render_inputs import BulletReader,lexical
from tools.projectile_oracle.guided_step import i32,xyz,vec
from tools.native_slope import slope_matrices
from tools.spatial_oracle.building_body_rules import RULES,SP,INI,dwords
from tools.rules_oracle.bridge_child_sound import Sound,sections as sound_sections

HERE=Path(__file__).resolve().parent
from .anytown_resident import Resident,rules,identity,inputs,sr

from .inputs import ASSETS,ATTACK_SHPS

def sha(b):return hashlib.sha256(b).hexdigest()
def layers():
 root=Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
 return [(n,root/n) for n in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini')]+[('XMP03T4.MAP',identity.ASSETS/'XMP03T4.MAP')]

def sound_inputs(m,root,art,*,wanted_names=None):
 """Read a declared physical SoundList subset in the caller's existing VM.

 The default preserves the MTNK impact corpus. Other compositions select
 their own names before the original type readers bind sound vectors.
 """
 raw=(root/'SOUNDMD.INI').read_bytes();physical=sound_sections(raw)
 wanted=set(wanted_names) if wanted_names is not None else {'GrizzlyTankAttack','Explosion14','ExplosionWaterLarge','ExplosionWaterMed','ExplosionWaterSmall'}
 selected={k:v for k,v in physical.items() if k in wanted|{'Defaults'}}
 selected['SoundList']={k:v for k,v in physical['SoundList'].items() if v in wanted}
 # Reuse the owning Sound source-order cache writer in this same VM and heap.
 proxy=Sound.__new__(Sound);proxy.__dict__=m.__dict__;Sound.make_ini(proxy,selected)
 samples=[]
 def lookup(u,a,n,d):
  if a==0x4015C0:
   name=m.string(u.reg_read(UC_X86_REG_EDX))
   if name not in samples:samples.append(name)
   m.ret(samples.index(name))
 h=m.u.hook_add(UC_HOOK_CODE,lookup)
 try:
  m.u.mem_write(0x87E2A0,dwords(1));m.u.mem_write(0x87E294,dwords(m.alloc(0x100)));m.invoke(0x4072C0,0x87E250)
  m.u.mem_write(0xB1D378,dwords(0x7EB6D4,m.alloc(64),16,1,0,10));m.invoke(0x7510D0,INI)
 finally:m.u.hook_del(h)
 rows=[]
 for name in sorted(wanted):
  index=m.invoke(0x7514D0,m.cstring(name));p=m.read32(m.read32(m.read32(0xB1D37C)+index*4))
  rows.append(dict(name=m.string(p+0x6C),fixture_index=index,samples=[samples[m.read32(p+0xB4+i*4)] for i in range(m.read32(p+0x134))]))
 m.make_ini(art)
 return dict(sha256=sha(raw),selected=selected,rows=rows,boundary='Original selected SoundList/Defaults/type readers and retained-name lookup; AudioIndex sample names mapped to local indexes; playback7509E0 is a recording boundary, excluding audio Main RNG/device output.')

def prepare(*,reader_factory=BulletReader,type_pointer=None,rules_pointer=None):
 root=Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
 art_raw=(root/'ARTMD.INI').read_bytes();art,art_lines=lexical(art_raw,{'MTNK','GTNK','Cannon','120MM','GUNFIRE','S_CLSN16','S_CLSN22','H2O_EXP1','H2O_EXP2','H2O_EXP3'})
 m=reader_factory(art,root);u=m.u
 m.assets.update({name.upper():(ASSETS/name).read_bytes() for name in ATTACK_SHPS})
 sound=sound_inputs(m,root,art)
 initializers=[]
 # Original C initializer table slices: Bullet, Cell, area-damage/SelectAnim,
 # and homing. The area unit owns its own height104 global89E870.
 for table,count in ((0x8127F4,13),(0x8129FC,13),(0x812A40,14),(0x813C2C,13)):
  for a in struct.unpack('<'+'I'*count,u.mem_read(table,count*4)):m.invoke(a,0);initializers.append(hex(a))
 for registry in ((0x887568,0xA8EB00,0xA83CE0,0xA8ED40) if type_pointer is None else (0x887568,0xA8EB00,0xA8ED40)):u.mem_write(registry,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
 typ=type_pointer
 if typ is None:typ=m.alloc(0xF00);m.invoke(0x7470D0,typ,(m.cstring('MTNK'),))
 r=rules_pointer
 if r is None:
  r=m.alloc(0x2000);u.mem_write(0x8871E0,dwords(r));m.invoke(0x665650,r)
 else:u.mem_write(0x8871E0,dwords(r))
 rows=[]
 for name,path in layers():
  if not path.exists():assert name=='LANGRULE.INI';rows.append(dict(file=name,absent=True));continue
  raw=path.read_bytes();sections,lines=lexical(raw,{'MTNK','105mm','105mmE','Cannon','AP','General','AudioVisual','CombatDamage','SpecialFlags','Basic'});m.rules_cache(sections)
  if 'MTNK' in sections:
   for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBX,typ),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EBP,typ+0x24),(UC_X86_REG_EAX,u.mem_read(typ+0x231,1)[0])):u.reg_write(reg,v)
   run_checked(u,0x5F94B3,0x5F9516)
  # Existing ifv_fire_coord owner blocks with MTNK's physical strings.
  for a,b in ((0x71284A,0x712A8F),(0x71338B,0x7133C8),(0x7147B4,0x7147CE),(0x714A49,0x714A63),(0x714016,0x714030)):
   for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,typ),(UC_X86_REG_EBX,typ+0x24),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EDI,RULES),(UC_X86_REG_EAX,u.mem_read(typ+0xD22,1)[0])):u.reg_write(reg,v)
   run_checked(u,a,b)
  for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EDI,typ),(UC_X86_REG_EBP,typ+0x24),(UC_X86_REG_EBX,RULES)):u.reg_write(reg,v)
  run_checked(u,0x747B03,0x747B49)
  w=m.read32(typ+0x898);assert m.string(w+0x24)=='105mm'
  wa=m.invoke(0x772080,w,(RULES,))&255;p=m.read32(w+0xA0);pa=m.invoke(0x46BEE0,p,(RULES,))&255;wh=m.read32(w+0xAC);ha=m.invoke(0x75D3A0,wh,(RULES,))&255
  m.invoke(0x7729F0,w)
  if m.invoke(0x526810,RULES,(m.cstring('AudioVisual'),)):
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EDI,RULES);run_checked(u,0x66B3C4,0x66B3E4)
  if m.invoke(0x526810,RULES,(m.cstring('CombatDamage'),)):
   for a,b in ((0x66CD66,0x66CD8C),(0x66C184,0x66C287),(0x66CE2C,0x66CE57)):
    u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EDI,RULES);run_checked(u,a,b)
  rows.append(dict(file=name,sha256=sha(raw),admitted=[wa,pa,ha],source_lines=lines,weapon=dict(name=m.string(w+0x24),damage=i32(u,w+0xA4),speed=i32(u,w+0xA8),rof=i32(u,w+0xB0),burst=i32(u,w+0x9C),range=i32(u,w+0xB4),anim_count=i32(u,w+0x104)),projectile=dict(name=m.string(p+0x24),arcing=u.mem_read(p+0x29B,1)[0],inaccurate=u.mem_read(p+0x2A2,1)[0],cluster=i32(u,p+0x2AC),gravity=i32(u,r+0x16B8)),warhead=dict(name=m.string(wh+0x24),wall=u.mem_read(wh+0x144,1)[0],cell_spread_bits=f'{m.read32(wh+0x124):08x}',anim_names=[m.string(m.read32(m.read32(wh+0x108)+j*4)+0x24) for j in range(i32(u,wh+0x114))]),bridge_strength=i32(u,r+0x1740)))
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,typ);run_checked(u,0x715B10,0x715F9E)
 anim=[]
 for name in ('GUNFIRE','S_CLSN16','S_CLSN22','H2O_EXP1','H2O_EXP2','H2O_EXP3'):
  ap=m.invoke(0x428B80,m.cstring(name));m.asset_loaded=[];ok=m.invoke(0x427D00,ap,(INI,))&255
  anim.append(dict(name=name,admitted=ok,assets=list(m.asset_loaded),end=i32(u,ap+0x2C0),rate=i32(u,ap+0x2B0),random_rate=list(struct.unpack('<2i',u.mem_read(ap+0x2E4,8)))))
 scenario=m.read32(0xA8B230);m.invoke(0x6B8AE0,scenario);flag0=m.read32(scenario)
 mp,mp_lines=lexical(layers()[-1][1].read_bytes(),{'SpecialFlags'});m.rules_cache(mp);u.mem_write(0xA8B238,dwords(5));m.invoke(0x6B8CA0,scenario,(RULES,))
 return m,typ,w,r,dict(sound=sound,strength=i32(u,typ+0xA0),special_flags=dict(constructor=flag0,after_physical_map=m.read32(scenario),map_sections=mp,mode=5),layers=rows,art_sha256=sha(art_raw),art_lines=art_lines,flh=xyz(u,typ+0x89C),anim=anim,initializers=initializers,height_units={hex(a):i32(u,a) for a in (0x89DE70,0x89E7C0,0x89E870)})

def attach_world(m,r):
 t=identity.theater();rr,fields=rules();case=inputs(t);resident=Resident(case,rr,t);dst=m.u;src=resident.uc
 for base,size in ((0x40000000,0x2000000),(0x44000000,0x800000),(0x45000000,0x2000000)):
  dst.mem_map(base,size);dst.mem_write(base,bytes(src.mem_read(base,size)))
 for base,size in ((sr.MAP,0x150),(0xC00000,0x100000),(0xABDC50,0x200),(0x89EA40,12*36),(0xA8ED2C,16),(0xA83D84,4)):
  dst.mem_write(base,bytes(src.mem_read(base,size)))
 for a in set(t['globals'])|{0xABC2B4,0xAA1130,0xABC1E8,0xAA0E38,0xAA1548,0xAA0740,0xABC1D0,0xAA1540,0xABAD30,0xAA1028}:dst.mem_write(a,bytes(src.mem_read(a,4)))
 dst.mem_write(r+0x664,bytes(src.mem_read(0x44400664,1)))
 scenario=m.read32(0xA8B230)
 resident.rngs['scenario']=scenario+0x218
 for k,addr in resident.rngs.items():dst.mem_write(addr,sr.state_bytes(case['supplied_rng'][k]))
 for c in [*resident.ptrs.values(),0xABDC50]:dst.mem_write(c,dwords(0x7E4EEC))
 resident.uc=resident.u=dst;resident.phase='measure';resident.trace.clear();resident.pending.clear()
 # One allocation authority remains Reader; use resident observer for only its
 # gameplay/path/presentation branches, never its separate bump allocator.
 def observe(uc,a,n,d):
  if a not in (0x7C8E17,0x7C8B3D,0x421EA0):resident.observe(uc,a,n,d)
 dst.hook_add(UC_HOOK_CODE,observe);dst.hook_add(UC_HOOK_MEM_WRITE,resident.write)
 m.invoke(0x561910,0);m.invoke(0x49F2F0,0)
 from tools.spatial_oracle.fire_error import LEVEL_HEIGHT_INITIALIZERS
 for a in LEVEL_HEIGHT_INITIALIZERS:m.invoke(a,0)
 for a,b in ((0x4E7AE0,0x4E7B16),(0x7253D0,0x725406),(0x7252D0,0x725306),(0x725450,0x725486),(0x40B540,0x40B5AB),(0x725850,0x725886),(0x4E6D60,0x4E6D96)):
  dst.reg_write(UC_X86_REG_ESP,SP);run_checked(dst,a,b)
 m.invoke(0x47D2B0,resident.ptrs[87,50],(-1,));row=resident.pending.pop(RET_MAGIC);row['after']=resident.snapshot(resident.ptrs[87,50]);world_source=row;resident.trace.clear()
 mats=slope_matrices()
 for index,row in enumerate(mats):dst.mem_write(0xB45188+48*index,dwords(*row))
 return resident,dict(input_sha256=sha(json.dumps(case,sort_keys=True).encode()),physical_map_sha256=case['map_sha256'],resident_assets=resident.assets,zone_storage=resident.zone_storage,source_recalc=world_source)

def execute():
 m,typ,w,r,info=prepare();u=m.u;resident,world=attach_world(m,r);target=resident.ptrs[87,54]
 src=m.alloc(0x1000);u.mem_write(src,dwords(0x7F5C70));u.mem_write(src+0x6C4,dwords(typ));u.mem_write(src+0x2B4,dwords(target));u.mem_write(src+0x90,b'\x01');u.mem_write(src+0x14,dwords(7))
 u.mem_write(src+0x160,struct.pack('<d',1.0));house=m.alloc(0x6000);u.mem_write(src+0x21C,dwords(house));u.mem_write(house+0x188,struct.pack('<d',1.0));u.mem_write(house+0x1A8,struct.pack('<d',1.0))
 loco=m.alloc(0x100);m.invoke(0x4AF540,loco);u.mem_write(loco+0xC,dwords(src));u.mem_write(src+0x674,dwords(loco+4))
 heading=m.alloc(4)
 for off in (0x388,0x3A0):
  m.invoke(0x4C91E0,src+off,(0,));u.mem_write(heading,dwords(0x7fff));m.invoke(0x4C9300,src+off,(heading,))
 # Supplied stationary origin on the physical north approach; target is the
 # actual center-cell object. Native FLH and target getter execute in FireAt.
 m.invoke(0x486840,resident.ptrs[87,50],(src+0x9C,));u.mem_write(0xA8ED84,dwords(0));u.mem_write(0xA8E9A0,b'\x01')
 u.reg_write(UC_X86_REG_ESI,src);u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,0x735678,0x735691)
 slot=m.invoke(0x746CD0,src,(target,));assert slot==0
 trace=deque(maxlen=30);events=[];bullet=[];pending={};frames=[];area=[];frame=0;shots=[];repair=None;cs=Cs(CS_ARCH_X86,CS_MODE_32)
 u.mem_map(0,0x1000);u.mem_write(0,dwords(-1))
 rng_initial={k:sr.rng_state(u,p) for k,p in resident.rngs.items()}
 def obs(uc,a,n,d):
  trace.append(a);sp=u.reg_read(UC_X86_REG_ESP)
  if a in pending:
   row=pending.pop(a);value=u.reg_read(UC_X86_REG_EAX);row['return']=value
   if row.get('pc')=='0x48a4f0':row['selected_name']=m.string(value+0x24) if value else None
  if a in (0x4690B0,0x489280,0x489E87,0x57CCF0,0x48A2B9,0x48A4F0,0x421EA0,0x49F420,0x7258D0,0x5F65F0,0x466560,0x70D4A0,0x6B4A50):
   row=dict(kind='entry',pc=hex(a),frame=frame,caller=hex(m.read32(sp)))
   if a==0x489280:row.update(position=xyz(u,u.reg_read(UC_X86_REG_ECX)),damage=u.reg_read(UC_X86_REG_EDX),warhead=m.string(m.read32(sp+8)+0x24));area.append(row)
   if a==0x48A2B9:row['driver_low_byte']=u.reg_read(UC_X86_REG_EAX)&255
   if a==0x421EA0:row['anim']=m.string(m.read32(sp+4)+0x24)
   if a==0x48A4F0:
    row.update(land_argument=i32(u,sp+4),position=xyz(u,m.read32(sp+8)),damage=u.reg_read(UC_X86_REG_ECX),current_target=resident.snapshot(target));pending[m.read32(sp)]=row
   if a==0x70D4A0:row.update(source_still_targets_cell=m.read32(src+0x2B4)==target)
   events.append(row)
  if a==0x46B007:
   ptr=m.read32(sp);value=(m.read32(ptr)-1)&0xffffffff;u.mem_write(ptr,dwords(value));u.reg_write(UC_X86_REG_EAX,value);u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,0x46B00D);return
  if a==0x7CAA5E:
   ptr,length=struct.unpack('<2I',u.mem_read(sp,8));u.mem_read(ptr,length);u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_ESP,sp+8);u.reg_write(UC_X86_REG_EIP,0x7CAA64);return
  if a in (0x65C640,0x65C660,0x65C780,0x65C7E0):
   row=dict(kind='rng',frame=frame,pc=hex(a),caller=hex(m.read32(sp)),stream=next((k for k,p in resident.rngs.items() if p==u.reg_read(UC_X86_REG_ECX)),hex(u.reg_read(UC_X86_REG_ECX))),args=[i32(u,sp+4),i32(u,sp+8)] if a==0x65C7E0 else []);events.append(row);pending[m.read32(sp)]=row
  if a in (0x65C84B,0x65C79D):events.append(dict(kind='raw',frame=frame,pc=hex(a),value=u.reg_read(UC_X86_REG_ESI)))
  if a==0x7C978A:
   events.append(dict(kind='CRT_process_exit_registration_boundary',callback=hex(m.read32(sp+4))));m.ret(0);return
  if a==0x7509E0:
   index=u.reg_read(UC_X86_REG_ECX);ptr=m.read32(m.read32(m.read32(0xB1D37C)+index*4))
   events.append(dict(kind='sound_boundary',name=m.string(ptr+0x6C),position=xyz(u,u.reg_read(UC_X86_REG_EDX))));m.ret(0,4);return
  if a==0x46B072:
   clsid,outer,context,iid,ppv=struct.unpack('<5I',u.mem_read(sp,20));assert (clsid,outer,context,iid)==(0x7E96E0,0,7,0x7F7C90)
   u.mem_write(sp,dwords(0x46B078,0,outer,iid,ppv));u.reg_write(UC_X86_REG_EIP,0x6C5090);events.append(dict(kind='COM_activation_to_original_factory'))
  elif a==0x46AFE5:
   ptr=m.read32(sp);value=m.read32(ptr)+1;u.mem_write(ptr,dwords(value));u.reg_write(UC_X86_REG_EAX,value);u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,0x46AFEB)
  elif a==0x6FE562:bullet.append(u.reg_read(UC_X86_REG_EAX))
  elif a in (0x5F4EC0,0x4A9770,0x4A9720):
   this=u.reg_read(UC_X86_REG_ECX)
   if a==0x5F4EC0:u.mem_write(this+0x9C,bytes(u.mem_read(m.read32(sp+4),12)));u.mem_write(this+0x90,b'\x01')
   events.append(dict(kind='supplied_world_admission',pc=hex(a)));m.ret(1,8 if a==0x5F4EC0 else 4)
  if a>=0x7E1000 and a!=RET_MAGIC:raise AssertionError(('non-image-code',hex(a)))
 hook=u.hook_add(UC_HOOK_CODE,obs);failure=None;fire_error=None
 def tick(active_bullet=None):
  nonlocal frame
  frame+=1;u.mem_write(0xA8ED84,dwords(frame))
  if active_bullet and u.mem_read(active_bullet+0x90,1)[0]:
   m.invoke(0x4666E0,active_bullet)
   frames.append(dict(frame=frame,position=xyz(u,active_bullet+0x9C),velocity=vec(u,active_bullet+0xE8),alive=u.mem_read(active_bullet+0x90,1)[0]))
  # Explicit supplied scheduling: one AI visit per current native Anim, then
  # native deferred-retirement drain. Full Logic ordering is not claimed.
  anims=[m.read32(m.read32(0xA8E9AC)+i*4) for i in range(m.read32(0xA8E9B8))]
  for a in anims:
   if u.mem_read(a+0x90,1)[0]:m.invoke(0x423AC0,a)
  if m.read32(0xB0F6A8):m.invoke(0x725C70,0)
 try:
  for off in (0x6D8,0x6C0,0x2FC,0x2EC,0x6A0):u.mem_write(src+off,dwords(-1))
  u.mem_write(src+0x6C,dwords(m.read32(typ+0xA0)));u.mem_write(src+0xAC,dwords(1))
  for number in range(1,101):
   before=resident.snapshot(target);start=len(events);resident_start=len(resident.trace);area_start=len(area)
   shot=dict(number=number,frame=frame,before=before,scenario_before=sr.rng_state(u,resident.rngs['scenario']))
   fire_error=m.invoke(0x740FD0,src,(target,slot,1));assert fire_error==0,fire_error
   events.append(dict(kind='fire_error',frame=frame,value=fire_error))
   b=m.invoke(0x741340,src,(target,slot));assert b==bullet[-1]
   shot.update(rearm=[i32(u,src+0x2EC),i32(u,src+0x2F4)],launch=dict(damage=i32(u,b+0x6C),position=xyz(u,b+0x9C),velocity=vec(u,b+0xE8),target=m.read32(b+0x10C)==target),scenario_after_fire=sr.rng_state(u,resident.rngs['scenario']))
   for _ in range(120):
    tick(b)
    if len(area)>area_start:break
   else:raise AssertionError('no native area impact within120 frames')
   shot.update(impact_frame=frame,area=area[area_start:],after=resident.snapshot(target),source_still_targets_cell=m.read32(src+0x2B4)==target,event_range=[start,len(events)],resident_range=[resident_start,len(resident.trace)],scenario_after_impact=sr.rng_state(u,resident.rngs['scenario']))
   shots.append(shot)
   if shot['after']['overlay']==232:break
   deadline=sum(shot['rearm'])
   while frame<deadline:tick()
  else:raise AssertionError('no collapse within100 scheduled shots')
  # Finish the resulting animations with original AI before repair. This may
  # include original crater/marking work and its own RNG, all recorded.
  for _ in range(100):
   if not m.read32(0xA8E9B8):break
   tick()
  assert not m.read32(0xA8E9B8)
  begin=len(events);ri=len(resident.trace);rng0={k:sr.rng_state(u,p) for k,p in resident.rngs.items()}
  u.mem_write(sr.COORD,sr.packed(*resident.case['start']));resident.call(0x573540,args=(sr.COORD,),count=3000000)
  repair=dict(frame=frame,events=events[begin:],resident_trace=resident.trace[ri:],rng_before=rng0,rng_after={k:sr.rng_state(u,p) for k,p in resident.rngs.items()},span=[resident.snapshot(resident.ptrs[x,y]) for y in range(51,58) for x in range(86,89)])
 except Exception as e:
  failure=dict(error=str(e),trace=[f'{a:08x}: '+ '; '.join(f'{x.mnemonic} {x.op_str}' for x in cs.disasm(bytes(u.mem_read(a,15)),a,count=1)) for a in trace])
 finally:u.hook_del(hook)
 assert failure is None,failure
 text_hash=sha(bytes(u.mem_read(0x401000,0x3E0000)));assert text_hash==resident.code_hash
 assert not pending,pending
 return dict(native_sha256=NATIVE_SHA256,text_section_sha256=text_hash,inputs=info,world=world,slot=slot,shots=shots,events=events,stop=hex(u.reg_read(UC_X86_REG_EIP)),frames=frames,area=area,resident_trace=resident.trace,repair=repair,rng_initial=rng_initial,rng_final={k:sr.rng_state(u,p) for k,p in resident.rngs.items()},failure=failure,fire_error=fire_error,rearm=[i32(u,src+0x2EC),i32(u,src+0x2F4)],source_after=dict(position=xyz(u,src+0x9C),health=i32(u,src+0x6C),alive=u.mem_read(src+0x90,1)[0],still_targets_cell=m.read32(src+0x2B4)==target))

def authored_fence_case(seed,*,forced_cells=None):
 from types import SimpleNamespace
 from tools.spatial_oracle.anytown_damage.navigation_inputs import Inputs,extract_tiles
 from tools.spatial_oracle.anytown_damage.navigation import Navigation
 from tools.spatial_oracle.bridge_constructor import REGISTRIES
 from tools.spatial_oracle.ore_radar import prepare_dirty_tracking
 from tools.spatial_oracle.bridge_shadow_render import initialize_overlay_theater
 from tools.spatial_oracle.ore_queue import RANDOM,RANGED
 t=identity.theater()
 reader=Inputs(t,damage_overlays=(),wall_inputs=True)
 BulletReader.initialize_default_color(reader)
 defaults=reader.wall_defaults
 theater_input=reader.read_map_theater()
 overlay_theater=initialize_overlay_theater(reader,theater_input['value'])
 def existing_reader(art,root):
  reader.make_ini(art)
  return reader
 m,typ,weapon,r,attack=prepare(reader_factory=existing_reader,type_pointer=reader.mtnk,rules_pointer=reader.rules)
 tilts=None
 if forced_cells is not None:
  # The same native type constructor supplies D2B's current default. Execute
  # the exact layered boolean read, rather than infer it from stock key absence.
  retained_rules=bytes(reader.u.mem_read(RULES,0x40))
  tilts=dict(constructor_default=reader.u.mem_read(typ+0xD2B,1)[0],layers=[])
  for name,path in layers():
   if not path.exists():
    assert name=='LANGRULE.INI'
    tilts['layers'].append(dict(file=name,absent=True));continue
   raw=path.read_bytes();sections,lines=lexical(raw,{'MTNK'});reader.rules_cache(sections)
   before=reader.u.mem_read(typ+0xD2B,1)[0]
   admitted=bool(reader.invoke(0x526810,RULES,(typ+0x24,)))
   if admitted:
    for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,typ),(UC_X86_REG_EBX,typ+0x24),(UC_X86_REG_ESI,RULES)):
     reader.u.reg_write(reg,value)
    run_checked(reader.u,0x7153DA,0x7153FB)
   tilts['layers'].append(dict(file=name,sha256=sha(raw),section_admitted=admitted,
    source_lines=lines,before=before,after=reader.u.mem_read(typ+0xD2B,1)[0]))
  tilts.update(value=reader.u.mem_read(typ+0xD2B,1)[0],offset='D2B',key='TiltsWhenCrushes',
   constructor_store='711500',reader=['7153DA','7153FB'])
  # rules_cache allocates each lexical cache; restore the prior live header so
  # later native map/asset consumers retain exactly the shared setup's inputs.
  reader.u.mem_write(RULES,retained_rules)
 # Original lazy image getter, with physical NewTheater assets provided by the
 # same Reader. The map caller repeats its own actual image/admission tests.
 wall_images=[]
 for row in reader.wall_type_states():
  if not row['wall']:continue
  p=reader.overlay_ptrs[row['index']]
  mark=len(reader.asset_loaded)
  image=reader.invoke(reader.read32(reader.read32(p)+0x9C),p)
  wall_images.append(dict(index=row['index'],image=reader.string(p+0x1F8),
                          image_present=bool(image),loads=reader.asset_loaded[mark:]))
  assert image,('physical wall theater image absent',row['name'])
 tiles,tmp_receipt=extract_tiles(t)
 native=Navigation(reader,t,tiles,create_actor=False,scenario_theater=theater_input,
                   terrain_inputs_only=True,runtime_lifecycle=True,wall_overlay_packs=True)
 u=native.uc
 read=lambda p:sr.u32(u,p)
 scenario=read(0xA8B230)
 # The selected AreaDamage setup reuses original process initializers, including
 # CellSpread's entire369-pair producer. No host offset table is supplied.
 before_registries={p:bytes(u.mem_read(p,24)) for p in REGISTRIES}
 for entry in attack['initializers']:native.call(int(entry,16),this=0,count=1000000)
 native.call(0x561910,this=0,count=1000000)
 assert before_registries=={p:bytes(u.mem_read(p,24)) for p in REGISTRIES}
 native.call(0x568BB0,args=(0,),count=100000000)
 native.finish_graphs()
 # Original FullInit/ReadScenario exit and ordinary Main zero producer. Calls
 # outside these counter stores are intentionally not claimed as full startup.
 sp=sr.STACK_BASE+sr.STACK_SIZE-0x1000
 u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ESI,0)
 run_checked(u,0x687C2B,0x687C3A)
 native.scenario_init.append(read(0xA8E7AC))
 # FullInit restores its outer depth after the excluded wall-connection pass.
 run_checked(u,0x687C44,0x687C4A)
 native.scenario_init.append(read(0xA8E7AC))
 run_checked(u,0x684B58,0x684B6C)
 native.scenario_init.append(read(0xA8E7AC))
 u.reg_write(UC_X86_REG_EBX,0);run_checked(u,0x48CDF0,0x48CDF6)
 native.scenario_init.append(read(0xA8E7AC))
 u.mem_write(0xA8B238,dwords(attack['special_flags']['mode']))
 assert read(0xA8E7AC)==0
 for pointer in native.rngs.values():native.call(0x65C6D0,this=pointer,args=(seed,),count=100000)
 # Share the radar storage owner and execute its actual deduplicating dirty
 # writer. Full radar generation/online display is a separate presentation gate.
 radar=SimpleNamespace(u=u,alloc=native.allocate)
 radar_inputs=prepare_dirty_tracking(radar,192,192,0,0,capacity=512)
 native.wall_live=True;native.activity='wall_damage'
 point=native.allocate(12)
 source_cell=forced_cells[0] if forced_cells is not None else (43,81)
 native.call(0x480A30,this=native.ptrs[source_cell],args=(point,))
 center=xyz(u,point)
 damage=-1 if forced_cells is not None else sr.i32(u,weapon+0xA4)
 warhead=read(weapon+0xAC)
 cells_by_pointer={p:xy for xy,p in native.ptrs.items()}
 def cell(p):
  return dict(cell=native.coord(p),overlay=sr.i32(u,p+0x44),data=u.mem_read(p+0x11E,1)[0],
   wall_owner=sr.i32(u,p+0x50),neighbors=u.mem_read(p+0x122,1)[0],land=sr.i32(u,p+0xEC),
   zone=sr.i32(u,p+0x4C),tile=sr.i32(u,p+0x38),subtile=u.mem_read(p+0x11A,1)[0],
   level=u.mem_read(p+0x11B,1)[0],slope=u.mem_read(p+0x11C,1)[0],flags=read(p+0x140),
   ground_object=bool(read(p+0xE4)))
 def rng():return {k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in native.rngs.items()}
 def radar_state():
  data,count=read(sr.MAP+0x1228),read(sr.MAP+0x1234)
  return dict(cells=[list(struct.unpack('<hh',u.mem_read(data+i*4,4))) for i in range(count)],
              dirty_flag=u.mem_read(sr.MAP+0x14D9,1)[0])
 def state():
  return dict(rng=rng(),runtime_id_cursor=read(scenario+0x214),pending_delete=read(0xB0F6A8),
   techno_listeners=read(0xA8EC88),radar=radar_state())
 watched={0x489280:'area_damage',0x480CB0:'wall_damage',0x480630:'wall_cleanup',0x480510:'wall_connection',
  0x47D2B0:'recalc',0x56D460:'navigation_remove',0x56D5A0:'navigation_add',0x584550:'hierarchy_patch',
  0x6551C0:'radar_dirty',0x655250:'radar_flush',0x7258D0:'pointer_expiry',0x70D4A0:'cell_target_detach',
  0x6D2790:'tactical_dirty',0x725C70:'deferred_drain'}
 events=[];pending=[];dirty=set()
 def observe(_u,pc,_size,_data):
  esp=u.reg_read(UC_X86_REG_ESP);this=u.reg_read(UC_X86_REG_ECX)
  while pending and pc==pending[-1][0]:
   _,index,receiver=pending.pop();events[index]['returned_eax']=u.reg_read(UC_X86_REG_EAX)
   if receiver in cells_by_pointer:events[index]['after']=cell(receiver)
  if pc in watched:
   event=dict(kind=watched[pc],pc=f'{pc:08X}',caller=f'{read(esp):08X}')
   if this in cells_by_pointer:event['before']=cell(this)
   if pc in (0x480CB0,0x480630):event['argument']=sr.i32(u,esp+4)
   if pc==0x480510:event['arguments']=list(struct.unpack('<2i',u.mem_read(esp+4,8)))
   if pc==0x7258D0:event.update(parameter_dl=u.reg_read(UC_X86_REG_EDX)&255,techno_listeners=read(0xA8EC88))
   if pc in (0x56D460,0x56D5A0,0x584550,0x6551C0):event['cell']=list(struct.unpack('<hh',u.mem_read(read(esp+4),4)))
   events.append(event)
   if pc in (0x480CB0,0x480630,0x480510,0x47D2B0,0x7258D0,0x70D4A0):pending.append((read(esp),len(events)-1,this))
  if pc in (RANDOM,RANGED):
   stream=next(k for k,p in native.rngs.items() if p==this)
   events.append(dict(kind='rng_request',entry=f'{pc:08X}',stream=stream,
    arguments=list(struct.unpack('<2i',u.mem_read(esp+4,8))) if pc==RANGED else []))
   pending.append((read(esp),len(events)-1,0))
  if pc in (0x65C84B,0x65C79D):
   pointer=u.reg_read(UC_X86_REG_EDX) if pc==0x65C84B else this
   stream=next(k for k,p in native.rngs.items() if p==pointer)
   events.append(dict(kind='raw_draw',pc=f'{pc:08X}',stream=stream,value=u.reg_read(UC_X86_REG_ESI)))
 def write(_u,_access,address,size,_value,_data):
  pointer=address&~0x1FF
  if pointer in cells_by_pointer:dirty.add(pointer)
 native.entry_observer=observe;whook=u.hook_add(UC_HOOK_MEM_WRITE,write)
 initial_cells={p:cell(p) for p in native.ptrs.values()}
 initial=state();steps=[]
 source_prior=cell(native.ptrs[source_cell])
 assert source_prior['overlay']==203 and source_prior['neighbors']>0,source_prior
 if forced_cells is None:assert source_prior['data']==5,source_prior
 initial_patch=[dict(initial_cells[native.ptrs[x,y]]) for y in range(78,85) for x in range(40,47)]
 for hit in range(1,len(forced_cells)+1 if forced_cells is not None else 13):
  start=len(events);dirty.clear();before=state()
  target_cell=forced_cells[hit-1] if forced_cells is not None else source_cell
  target=native.ptrs[target_cell]
  if forced_cells is not None:
   assert cell(target)['overlay']==203 and cell(target)['neighbors']>0,cell(target)
   native.call(0x480CB0,this=target,args=(-1,),count=100000000)
   # The host stop boundary is not executed as an instruction. Finalize only
   # its actual returned PC/EAX/stores through the same observation closure.
   assert u.reg_read(UC_X86_REG_EIP)==RET_MAGIC
   observe(u,u.reg_read(UC_X86_REG_EIP),0,None)
  else:
   u.reg_write(UC_X86_REG_EDX,damage)
   native.call(0x489280,this=point,args=(0,warhead,0,0),count=100000000)
  assert not pending,pending
  changed=[]
  for p in sorted(dirty,key=lambda p:(cells_by_pointer[p][1],cells_by_pointer[p][0])):
   now=cell(p)
   if now!=initial_cells[p]:changed.append(dict(before=initial_cells[p],after=now));initial_cells[p]=now
  before_drain=state();native.call(0x725C70,this=0,count=10000000)
  assert not pending,pending
  steps.append(dict(hit=hit,before=before,events=events[start:],changed=changed,
   before_drain=before_drain,after=state(),source=cell(target)))
  if forced_cells is not None:
   steps[-1]['source_cell']=list(target_cell)
   assert steps[-1]['source']['overlay']==-1,steps[-1]['source']
   assert before['rng']==steps[-1]['after']['rng'],'forced wall consumed RNG'
   assert not any(e['kind'] in ('area_damage','cell_target_detach','rng_request','raw_draw') for e in steps[-1]['events'])
   assert any(e['kind']=='pointer_expiry' for e in steps[-1]['events']),'no original wall expiry'
  elif sr.i32(u,target+0x44)==-1:break
 else:
  if forced_cells is None:raise AssertionError('no original source wall removal in12 native AP impacts')
 native.entry_observer=None;u.hook_del(whook)
 assert steps[-1]['source']['overlay']==-1,'no source removal'
 if forced_cells is None:assert any(e['kind']=='raw_draw' for e in events),'no actual wall RNG'
 for event in events:
  if event['kind']=='tactical_dirty':
   caller=int(event['caller'],16)
   assert 0x401000<=caller<0x7E1000,event
   instruction=bytes(u.mem_read(caller-5,5))
   assert instruction[0]==0xE8 and caller+struct.unpack('<i',instruction[1:])[0]==0x6D2790,event
 print('native wall source history',json.dumps(dict(seed=seed,steps=[dict(hit=s['hit'],
  source=[s['source']['overlay'],s['source']['data']],calls=[dict(before=e['before']['data'],
   after=[e['after']['overlay'],e['after']['data']],returned=e['returned_eax'])
   for e in s['events'] if e['kind']=='wall_damage' and e.get('before',{}).get('cell')==s.get('source_cell',[43,81])])
  for s in steps])),flush=True)
 result=dict(input=dict(seed=seed,impact_cell=list(source_cell),impact_xyz=center,damage=damage,
   attacker=None,game_mode=attack['special_flags']['mode'],game_active=native.game_active,scenario_init=native.scenario_init,
   stock_attack=attack,overlay_theater=overlay_theater,theater=theater_input,wall_defaults=defaults,wall_layers=reader.wall_layers,wall_images=wall_images,
   manifest=reader.wall_sources,size=native.case['size'],local_size=native.case['local_size'],
   map_rect=list(struct.unpack('<4i',u.mem_read(sr.MAP+0x124,16))),
   allocated_cells=len(native.ptrs),radar=radar_inputs,
   wall_stamps=native.wall_stamp_receipts,wall_types=reader.wall_type_states(),
   original_cell_spread_pairs=369,
   physical_tmp=dict(outer_sha256=tmp_receipt['outer_sha256'],inner_sha256=tmp_receipt['inner_sha256'])),
  initial=initial,initial_patch=initial_patch,steps=steps)
 if forced_cells is not None:
  result['input'].update(driver='Cell480CB0',source_cells=[list(c) for c in forced_cells],mtnk_tilts=tilts)
  return result
 # Native query-only controls use the same allocated Cell and actual type
 # registry. Raw identity priors deliberately exercise shared prefix callers;
 # no wall admission or GDI/Nod building-gate behavior is claimed for these.
 query_cell=native.ptrs[43,81];retained=bytes(u.mem_read(query_cell,0x200))
 query_rng=rng();queries=[]
 priors=[(value,-1,-1) for value in (-1,2,26,203,243)]
 priors += [(value,203,direction) for direction in (0,2,4,6) for value in (203,204)]
 for stored,requested,direction in priors:
  u.mem_write(query_cell+0x44,dwords(stored));before=bytes(u.mem_read(query_cell,0x200))
  answer=native.call(0x480510,this=query_cell,args=(requested,direction),count=10000)
  assert bytes(u.mem_read(query_cell,0x200))==before
  assert rng()==query_rng
  queries.append(dict(stored=stored,requested=requested,direction=direction,
   returned_eax=answer,returned_low_byte=answer&255,rng_unchanged=True))
 u.mem_write(query_cell,retained)
 query_controls=dict(cell=[43,81],ground_object=bool(read(query_cell+0xE4)),
  prior='Explicit raw Cell identity inputs restored after query-only original480510 calls; no GDI/Nod gate walk or wall constructor claim.',
  rng_before=query_rng,rng_after=rng(),rows=queries)
 result['query_controls']=query_controls
 return result


def authored_fence_generate():
 cases=[authored_fence_case(seed) for seed in (0,31)]
 # Coverage belongs to the corpus, not every independent seed. Last-stage
 # isolated walls can be removed on their first accepted source hit.
 assert any(s['source']['overlay']==203 and s['source']['data']>=16 for c in cases for s in c['steps']),'no published source partial'
 wall=[e for c in cases for s in c['steps'] for e in s['events'] if e['kind']=='wall_damage']
 assert any(e['before']==e['after'] for e in wall),'no native rejected wall control'
 assert any(e['after']['overlay']==-1 and e['before']['overlay']!=-1 for e in wall),'no native wall removal'
 return dict(native_sha256=NATIVE_SHA256,cases=cases)


def fence_driveover_generate():
 return dict(native_sha256=NATIVE_SHA256,cases=[authored_fence_case(0,forced_cells=((43,82),(44,82)))])


def fence_driveover_metadata():
 result=authored_fence_metadata()
 result['scope']='Physical XMP03T4 CAFNCB Cell480CB0(-1) sequence43,82 then44,82 using the existing authored-fence VM. Direct Cell component; not native UnitPerCell, audio, movement scheduling or whole-World RNG parity.'
 result['assumptions']=[
  'Same physical readers, actual95 authored Wall admissions/data pass, original Map/Cell/TMP/navigation/ScenarioInit/GameActive producers and shared radar storage as --authored-fence. No actor or positive Techno/listener population. Independent seed0 is installed on all3 native streams after setup.',
  'Source cells43,82 then44,82 are independently declared component inputs. Each real480CB0(-1), its complete reached cleanup/Recalc/navigation/radar/expiry and deferred drain execute in order; second input state is the actual first call output. Direct Unit caller73B056 uses -1; AreaDamage and its70D4A0 detach are not part of this route.',
  'All3 complete RNG states, request/raw events, Cell/122 changes, return values, callback order, runtime cursor and delete queue are observed. Selected forced branch is asserted draw-free; this says nothing about Unit sound playback Main-stream RNG. Full UnitPerCell timing, captured+1B8 target, sound+48 position and recoil use separate original296 numeric/caller evidence and normal World integration.',
  'Native constructor default D2B and admitted exact-case TiltsWhenCrushes7153DA..7153FB reads run through the same MTNK reader and physical RULESMD/LANGRULE/Battle/map layers. This does not execute Drive pre-entry+64/-0.05 tilt, Unit+334 recoil, or Unit+6B5 lifetime.',
 ]
 result['entry_points'].pop('area_damage');result['entry_points'].pop('detach')
 result['entry_points'].update(unit_forced_caller='0x0073B056',physical_cell_query='0x0041BEA0',
  sound_coordinate_query='0x005F65A0',tilts_reader='0x007153DA')
 return result


def authored_fence_metadata():
 return provenance(scope='Physical XMP03T4 CAFNCB independent stock AP Cell-center AreaDamage histories through actual wall damage, connectivity, retained122, Recalc/navigation/radar/expiry. Not a normal whole-shot/World RNG or native attacker target-release oracle.',
  assumptions=[
   'Existing BulletReader unused-color registry prerequisite is reused: selected Cannon Color is physically absent; this placeholder is unrelated to actual wall raster ColorScheme. Existing full physical Navigation runtime and same Inputs reader; opt-in full Rules ctor, selected physical Overlay ART/readers and actual Map caller Overlay InitTheater5FE620 and existing MTNK/105mm/Cannon/AP readers. All actual authored Wall admissions execute original decoded5FD41A..5FD508 after Cell/TMP initialization and before Terrain, under original ScenarioInit producers; actual second data-pass5FD61A..5FD646 follows. Source id/frame/122 is not injected.',
   'Existing native connectivity/hierarchy initialize and update the actual physical cache. Original CellSpread561910 and existing process initializer entries execute. Original counter-exit/Main zero and active-game store execute; independent stock-reader Session GameMode5/seed0 or31 and null attacker are explicit inputs. Original map/archive loading, authored Building/Techno placement and full Scenario bootstrap excluded.',
   'Full489280,480CB0,480630,480510,47D2B0,56D460/56D5A0/584550,6551C0,7258D0/70D4A0 and725C70 execute. Empty Techno/listener registry remains explicit: actual wall expiry entry does not establish ordinary native MTNK target clearance. Normal World integration is a separate consumer.',
   'Actual full3 RNG states/request/raw order, changed Cell/122/owner/zone/material, callback order and native admission readbacks retained. Live wall animation boundary fails closed. Two stock independent seed histories, no altered random word or supplied damage/admission return.',
  ],substitutions=['Existing successful bounded allocation/free, CRT/TLS/cache/physical asset transport and Windows SEH/checked pointer service. Empty tactical extents and screen/shroud presentation sinks remain; radar dirty body runs on shared supplied storage. No wall/area/Recalc/navigation/expiry result is supplied.'],
  entry_points=dict(area_damage=0x489280,wall_damage=0x480CB0,cleanup=0x480630,connection=0x480510,
   map_identity=0x5FD41A,map_data=0x5FD61A,recalc=0x47D2B0,pointer_expiry=0x7258D0,
   detach=0x70D4A0,main_counter_zero=0x48CDF0,active_game=0x52D9D7,cell_spread_init=0x561910))


from .publication import finish_vectors

if __name__=='__main__':
 if '--authored-fence' in sys.argv[1:] or '--authored-fence-driveover' in sys.argv[1:]:
  driveover='--authored-fence-driveover' in sys.argv[1:]
  assert not (driveover and '--authored-fence' in sys.argv[1:]),'choose one authored-fence mode'
  flag='--authored-fence-driveover' if driveover else '--authored-fence'
  from tools.native_oracle import finish_vectors as native_finish
  sources={str(p):p for p in (Path(__file__),HERE/'navigation.py',HERE/'navigation_inputs.py',
    HERE.parent/'bridge_constructor.py',HERE.parent/'bridge_shadow_render.py',HERE.parent/'ore_radar.py',
    HERE.parent/'shrapnel_repair/retail_inputs.py',HERE.parent/'shrapnel_repair/shrapnel_repair.py',
    Path('tools/native_oracle.py'),Path('tools/projectile_oracle/bridge_render_inputs.py'),Path('tools/rules_oracle/bridge_anim_inputs.py'),
    Path('tools/rules_oracle/bridge_child_sound.py'))}
  native_finish(fence_driveover_generate if driveover else authored_fence_generate,
    HERE/('fence_driveover.json' if driveover else 'authored_fence.json'),
    provenance=fence_driveover_metadata if driveover else authored_fence_metadata,
    argv=[v for v in sys.argv[1:] if v!=flag],source_paths=sources)
  raise SystemExit(0)
 finish_vectors(execute,HERE/'mtnk_attack.json.gz',provenance=lambda:provenance(scope=__doc__,
  entry_points={'rules_ctor':0x665650,'unit_type_ctor':0x7470D0,'weapon_reader':0x772080,'projectile_reader':0x46BEE0,'warhead_reader':0x75D3A0,'ballistic_speed_postpass':0x7729F0,'special_flags_ctor':0x6B8AE0,'special_flags_map_reader':0x6B8CA0,'select_weapon':0x746CD0,'fire_error':0x740FD0,'unit_fire_at':0x741340,'techno_fire_at':0x6FDD50,'bullet_factory':0x6C5090,'bullet_ai':0x4666E0,'detonate':0x4690B0,'area_damage':0x489280,'concrete_damage':0x57CCF0,'detach':0x70D4A0,'recalc':0x47D2B0,'occupants':0x487A10,'anim_ai':0x423AC0,'retirement_drain':0x725C70,'repair':0x573540,'mapgen_range':0x598030,'scenario_range':0x65C7E0},
  assumptions=[
   'Unmodified physical XMP03T4.MAP crop, native overlay/theater readers and original Recalc reused from frozen Resident owner. The selected MTNK/105mm/Cannon/AP physical rules layers and original ART/selected SOUND readers execute in the existing BulletReader VM.',
   'Original SpecialFlags initialization6B8AE0 and complete physical map reader6B8CA0 execute with supplied GameMode5. This preserves constructor DestroyableBridges bit0x8000; map reader skips that group outside campaign/map editor.',
   'Stationary unmodified MTNK at physical87,50, target Cell87,54, normal facing and firepower/ROF multipliers1. Actual Cell target coordinates, FLH, weapon selection, FireError, whole Unit/Techno FireAt, original Bullet/Anim AI and retirement execute.',
   'Original-seeded0 full Main/Scenario/MapGen states are supplied after setup. Actual native RNG requests, raw draws and complete retained states are recorded; event caller is the return PC, not the call opcode address.',
   'Original .text is unchanged and protected against writes. Any uncovered execution fault fails the replay; --check independently executes and compares payload and metadata.'
  ],substitutions=[
   'Source Unit/House fields and placement are supplied, not created/admitted by ordinary Unit constructor/Unlimbo or registered in the global Techno list. No live damage recipients or other occupants. Actual Detach executes but cannot establish ordinary source target release in these empty registries.',
   'Host fires at frame0 and each prior native ROF deadline, visits the bullet once per subsequent frame, then each current native Anim once and native deferred drain. Unit/Foot/Mission/Logic scheduling and other world activity are excluded. In particular Foot MissionAttack4D4DC0 range(0,2) at4D4EA6 is not in this schedule; recorded FireAt count is not a native match shot count.',
   'Original object Unlimbo5F4EC0 and Display admission4A9770/4A9720 are successful recording boundaries with position/alive stores. COM activation maps to original Bullet factory; OS Interlocked and CRT exit-registration services are supplied. Existing bounded heap/file/INI-cache/CPU bootstrap boundaries remain inherited.',
   'Selected physical SOUND records use local sample/type indexes. Playback7509E0 records name/position and returns; audio-device work and Main-stream audio RNG are excluded, so unchanged Main is a consequence of this boundary.',
   'Native raw map fields/physical TMP bodies are bound to supplied pristine heads and initial zone planes; not a native whole-map load. Recalc and empty487A10 run, while connectivity56C510 and hierarchy586990, radar/screen output remain recording sinks.',
   'Repair starts at already-admitted573540 after animations retire; no Engineer order, approach, hut entry, ownership or Engineer destruction. No production/Rust/runtime parity claim. CliffBack constructor0 versus default2 remains a separate retained follow-up.'
  ]))
