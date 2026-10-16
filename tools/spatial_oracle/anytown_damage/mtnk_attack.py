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

def prepare(*,reader_factory=BulletReader):
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
 for registry in (0x887568,0xA8EB00,0xA83CE0,0xA8ED40):u.mem_write(registry,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
 typ=m.alloc(0xF00);m.invoke(0x7470D0,typ,(m.cstring('MTNK'),))
 r=m.alloc(0x2000);u.mem_write(0x8871E0,dwords(r));m.invoke(0x665650,r)
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

from .publication import finish_vectors

if __name__=='__main__':
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
