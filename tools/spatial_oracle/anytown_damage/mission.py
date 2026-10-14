"""Physical Anytown MTNK command and native live object-loop continuation.

Original source constructors/placement, Attack event, Unit/Foot/Techno/Mission AI,
FireAt/Bullet/Anim, collapse/target release and deferred retirement execute.
Single supplied House, cropped map and omitted global phases remain boundaries.
"""
from pathlib import Path
import argparse,hashlib,json,struct,sys
from collections import deque
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_INVALID
from unicorn.x86_const import *
from capstone import Cs,CS_ARCH_X86,CS_MODE_32

HERE=Path(__file__).resolve().parent
from . import mtnk_attack as base
from .mission_publication import finish_vectors
from tools.native_oracle import NATIVE_SHA256,RET_MAGIC,run_checked,finish_vectors as finish_unpublished_vectors,provenance,initialize_empty_windows_seh
from tools.spatial_oracle.building_body_rules import SP,RULES,dwords
from tools.rules_oracle.bridge_anim_inputs import Reader

class Mission:
 def __init__(self,continuation=None):
  self.continuation=continuation
  self.m,self.typ,self.weapon,self.rules,self.inputs=base.prepare();self.u=self.m.u
  self.resident,self.world=base.attach_world(self.m,self.rules)
  self.trace=deque(maxlen=200);self.events=[];self.pending={};self.frame=0;self.phase='setup';self.src=0;self.bullets=[];self.shots=[];self.frames=[];self.impacts=[]
  previous=self.resident.observe
  def resident_observer(u,a,n,d):
   if a==0x56C510:self.events.append(dict(kind='native_connectivity',phase=self.phase,frame=self.frame));return
   previous(u,a,n,d)
  self.resident.observe=resident_observer
  self.u.hook_add(UC_HOOK_CODE,self.observe);self.u.hook_add(UC_HOOK_MEM_INVALID,self.invalid)
 def state(self):
  u=self.u;m=self.m;p=self.src
  return dict(frame=self.frame,mission=base.i32(u,p+0xAC),queued=base.i32(u,p+0xB4),status=base.i32(u,p+0xBC),dispatch=[base.i32(u,p+0xC8),base.i32(u,p+0xD0)],rearm=[base.i32(u,p+0x2EC),base.i32(u,p+0x2F4)],target=hex(m.read32(p+0x2B4)),alive=u.mem_read(p+0x90,1)[0],limbo=u.mem_read(p+0x81,1)[0],marked=u.mem_read(p+0x74,1)[0],health=base.i32(u,p+0x6C),position=base.xyz(u,p+0x9C),global_techno_count=m.read32(0xA8EC88),global_unit_count=m.read32(0x8B4118),random_phase_raw_u16=int.from_bytes(u.mem_read(p+0x3C8,2),'little'),mission_visit_count=m.read32(p+0xC4),primary_facing=[m.read32(p+0x388),m.read32(p+0x38C)],turret_facing=[m.read32(p+0x3A0),m.read32(p+0x3A4)],logic_registered=u.mem_read(p+0x98,1)[0])
 def invalid(self,u,access,address,size,value,data):
  self.events.append(dict(kind='unmapped',access=access,address=hex(address),pc=hex(u.reg_read(UC_X86_REG_EIP))));return False
 def observe(self,u,a,n,d):
  self.trace.append(a);m=self.m;sp=u.reg_read(UC_X86_REG_ESP)
  if a in self.pending:
   row=self.pending.pop(a);row['returned_eax']=u.reg_read(UC_X86_REG_EAX)
   if row['kind']=='area_damage':row['after']=self.resident.snapshot(self.resident.ptrs[87,54]);row['source_target_after']=hex(m.read32(self.src+0x2B4))
   if row['kind']=='collapse_detach':row['source_target_after']=hex(m.read32(self.src+0x2B4))
   if row['kind']=='select_anim':row['selected_name']=m.string(u.reg_read(UC_X86_REG_EAX)+0x24) if u.reg_read(UC_X86_REG_EAX) else None
  if a in (0x65C640,0x65C660,0x65C780,0x65C7E0):
   row=dict(kind='rng',frame=self.frame,phase=self.phase,pc=hex(a),return_pc=hex(m.read32(sp)),stream=next((k for k,p in self.resident.rngs.items() if p==u.reg_read(UC_X86_REG_ECX)),hex(u.reg_read(UC_X86_REG_ECX))),args=[base.i32(u,sp+4),base.i32(u,sp+8)] if a==0x65C7E0 else []);self.events.append(row);self.pending[m.read32(sp)]=row
  if a in (0x65C84B,0x65C79D):self.events.append(dict(kind='raw',frame=self.frame,phase=self.phase,pc=hex(a),value=u.reg_read(UC_X86_REG_ESI)))
  entries={0x7353C0:'unit_ctor',0x737BA0:'unit_unlimbo',0x5F4EC0:'object_unlimbo',0x55BAA0:'logic_register',0x4A9720:'display_register',0x6F6CA0:'techno_unlimbo',0x4D31E0:'foot_ctor',0x6F2B40:'techno_ctor',0x5B3060:'mission_dispatch',0x4D4DC0:'attack_mission',0x736DF0:'firing_update',0x741340:'unit_fire',0x6FDD50:'techno_fire',0x740FD0:'fire_error',0x7360C0:'unit_ai',0x4DA530:'foot_ai',0x6F9E50:'techno_ai',0x5B35E0:'queue_mission',0x5B3570:'commence',0x7414E0:'unit_approach',0x4D5690:'foot_approach',0x70D4A0:'collapse_detach',0x489280:'area_damage',0x57CCF0:'concrete_damage',0x6FCDB0:'techno_set_target',0x4C6860:'event_ctor',0x4C6CB0:'event_execute',0x4DF0E0:'foot_setup_command',0x741970:'unit_set_destination',0x73F0A0:'unit_can_enter',0x4666E0:'bullet_ai',0x423AC0:'anim_ai',0x725C70:'deferred_drain',0x48A4F0:'select_anim',0x421EA0:'anim_ctor'}
  if a in entries:
   row=dict(kind=entries[a],frame=self.frame,phase=self.phase,pc=hex(a),return_pc=hex(m.read32(sp)),this=hex(u.reg_read(UC_X86_REG_ECX)),args=[m.read32(sp+4+i*4) for i in range({0x7353C0:2,0x737BA0:2,0x5F4EC0:2,0x55BAA0:2,0x4A9720:1,0x6F6CA0:2,0x4D31E0:1,0x6F2B40:1,0x741340:2,0x6FDD50:2,0x740FD0:3,0x5B35E0:2,0x70D4A0:1,0x489280:4,0x57CCF0:1,0x6FCDB0:1,0x4C6860:10,0x741970:2,0x73F0A0:5,0x48A4F0:2}.get(a,0))]);self.events.append(row)
   if a in (0x4D4DC0,0x740FD0,0x489280,0x70D4A0,0x48A4F0):self.pending[m.read32(sp)]=row
   if a==0x489280:
    row.update(position=base.xyz(u,u.reg_read(UC_X86_REG_ECX)),damage=u.reg_read(UC_X86_REG_EDX),warhead=m.string(m.read32(sp+8)+0x24),before=self.resident.snapshot(self.resident.ptrs[87,54]));self.impacts.append(row)
   if a==0x70D4A0:row['source_target_before']=hex(m.read32(self.src+0x2B4))
   if a==0x48A4F0:row.update(land_argument=base.i32(u,sp+4),position=base.xyz(u,m.read32(sp+8)))
   if a==0x421EA0:row['anim']=m.string(m.read32(sp+4)+0x24)
  if a==0x55B610:
   actor=u.reg_read(UC_X86_REG_ECX);self.events.append(dict(kind='logic_visit',frame=self.frame,index=u.reg_read(UC_X86_REG_ESI),count=m.read32(0x87F788),actor=hex(actor),vtable=hex(m.read32(actor)),ai=hex(m.read32(m.read32(actor)+0x5C))))
  if a==0x6FE562:self.bullets.append(u.reg_read(UC_X86_REG_EAX))
  if a==0x7413D3:
   b=u.reg_read(UC_X86_REG_EAX);assert b==self.bullets[-1]
   self.shots.append(dict(frame=self.frame,bullet=hex(b),rearm=[base.i32(u,self.src+0x2EC),base.i32(u,self.src+0x2F4)],position=base.xyz(u,b+0x9C),velocity=base.vec(u,b+0xE8),damage=base.i32(u,b+0x6C),rng_after=base.sr.rng_state(u,self.resident.rngs['scenario'])))
  # Composed worlds retain the existing generic read32/string/ret receiver;
  # call the single Reader transport owner without requiring Reader inheritance.
  if Reader.guid_transport(m,u,a,sp,self.events):return
  if a in (0x41C27D,0x41C2CB):
   clsid,outer,context,iid,ppv=struct.unpack('<5I',u.mem_read(sp,20));assert bytes(u.mem_read(clsid,16))==bytes(u.mem_read(0x7E9A30,16));assert outer==0 and context==7
   u.mem_write(sp,dwords(a+6,0,outer,iid,ppv));u.reg_write(UC_X86_REG_EIP,0x6C4010);self.events.append(dict(kind='COM_Drive_original_factory'));return
  if a==0x41C28C:
   self.events.append(dict(kind='OS_OleRun',interface=hex(m.read32(sp))));u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,a+6);return
  if a==0x46B072:
   clsid,outer,context,iid,ppv=struct.unpack('<5I',u.mem_read(sp,20));assert (clsid,outer,context,iid)==(0x7E96E0,0,7,0x7F7C90)
   u.mem_write(sp,dwords(0x46B078,0,outer,iid,ppv));u.reg_write(UC_X86_REG_EIP,0x6C5090);self.events.append(dict(kind='COM_Bullet_original_factory'));return
  if a in (0x655560,0x655740):
   self.events.append(dict(kind='radar_tracker_boundary',pc=hex(a),frame=self.frame,args=[m.read32(sp+4+i*4) for i in range(3)]));m.ret(0,12);return
  if a in (0x5F8110,0x5F8CE0):
   assert self.phase=='setup';self.events.append(dict(kind='type_visual_asset_boundary',pc=hex(a)));m.ret(0);return
  if a==0x6C8C40:
   self.events.append(dict(kind='setup_wall_clock',phase=self.phase));assert self.phase=='setup';m.ret(0);return
  if a==0x7C978A:self.events.append(dict(kind='CRT_exit_registration',callback=hex(m.read32(sp+4))));m.ret(0);return
  if a in (0x46AFE5,0x46B007,0x55A965,0x55A987):
   ptr=m.read32(sp);value=(m.read32(ptr)+(1 if a in (0x46AFE5,0x55A965) else -1))&0xffffffff;u.mem_write(ptr,dwords(value));u.reg_write(UC_X86_REG_EAX,value);u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,a+6);return
  if a==0x7CAA5E:
   ptr,length=struct.unpack('<2I',u.mem_read(sp,8));u.mem_read(ptr,length);u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_ESP,sp+8);u.reg_write(UC_X86_REG_EIP,0x7CAA64);return
  if a==0x7509E0:
   index=u.reg_read(UC_X86_REG_ECX)
   name=m.string(m.read32(m.read32(m.read32(0xB1D37C)+index*4))+0x6C) if index<m.read32(0xB1D388) else str(index)
   self.events.append(dict(kind='sound_boundary',frame=self.frame,name=name,position=base.xyz(u,u.reg_read(UC_X86_REG_EDX))));m.ret(0,4);return
  if a>=0x7E1000 and a!=RET_MAGIC:raise AssertionError(('non-image-code',hex(a)))
 def setup(self,*,placement_observer=None,context=None):
  if context is not None and not isinstance(context,dict):
   raise TypeError('Mission setup diagnostic context must be a JSON object')
  fixture=type(self).__module__+'.'+type(self).__qualname__
  diagnostic_context={'case':fixture+'.setup',**(context or {}),
   'setup':dict(fixture=fixture,phase=self.phase,frame=self.frame,continuation=self.continuation)}
  m=self.m;u=self.u
  # Existing native retirement owner supplies a valid empty Windows SEH chain.
  initialize_empty_windows_seh(u)
  # Actual MissionControl static construction, then the original Rules loop for
  # each physical layer. Source-order caches remain the existing INI boundary.
  m.invoke(0x4E7CF0,0);mission_rows=[]
  for name,path in base.layers():
   if not path.exists():continue
   sections,lines=base.lexical(path.read_bytes(),{'Attack','Guard','Sleep','Move','MTNK'})
   m.rules_cache(sections)
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,RULES);run_checked(u,0x679C92,0x679CAF)
   for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,self.typ),(UC_X86_REG_EBX,self.typ+0x24),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EDI,RULES)):u.reg_write(reg,v)
   run_checked(u,0x7123ED,0x71243A)
   # ROT is read from the physical type name through the native CCINI reader.
   for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,self.typ),(UC_X86_REG_EBX,self.typ+0x24),(UC_X86_REG_EDI,RULES)):u.reg_write(reg,v)
   run_checked(u,0x714B14,0x714B35)
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,RULES);run_checked(u,0x714293,0x7142AD)
   mission_rows.append(dict(file=name,source_lines=lines,attack=bytes(u.mem_read(0xA8E3C8,32)).hex(),locomotor=bytes(u.mem_read(self.typ+0x34C,16)).hex(),rot=m.read32(self.typ+0x71C),sight=m.read32(self.typ+0x5E8)))
  self.inputs['mission_layers']=mission_rows
  # Read common Rules sections consumed by complete Foot/Techno AI.
  self.inputs['ai_rules_layers']=[]
  for name,path in base.layers():
   if not path.exists():continue
   raw=path.read_bytes();sections,lines=base.lexical(raw,{'General','Radiation'});m.rules_cache(sections)
   layer_context={**diagnostic_context,'rules_layer':dict(name=name,path=str(path),sha256=hashlib.sha256(raw).hexdigest())}
   # Full retail GeneralRules retains its measured30s wall-time override and
   # unchanged shared two-million instruction cap; diagnostics add no VM writes.
   general=m.invoke(0x66D530,self.rules,(RULES,),timeout_us=30_000_000,context=layer_context)
   radiation=m.invoke(0x66CF70,self.rules,(RULES,),context=layer_context)
   self.inputs['ai_rules_layers'].append(dict(file=name,general_al=general&255,radiation_al=radiation&255,rad_application_delay=m.read32(self.rules+0x1808)))
  # Country and side registry construction from physical ordered names. Their
  # scalar readers still execute below; this does not model complete load order.
  import os
  sec,lines=base.lexical((Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])/'RULESMD.INI').read_bytes(),{'Countries','Sides'})
  for registry in (0xA83C98,0x8B4120):u.mem_write(registry,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
  self.countries={}
  for name in sec['Countries'].values():
   p=m.alloc(0x300);m.invoke(0x5113F0,p,(m.cstring(name),));self.countries[name]=p
  for name in sec['Sides']:
   p=m.alloc(0xC0);m.invoke(0x6A4550,p,(m.cstring(name),))
  self.inputs['country_side_lists']=dict(sections=sec,source_lines=lines)
  # Whole UnitType reader supplements the frozen shot-only selected readers.
  full_type=[]
  for name,path in base.layers():
   if not path.exists():continue
   sections,lines=base.lexical(path.read_bytes(),{'MTNK'});m.rules_cache(sections)
   ok=m.invoke(0x747620,self.typ,(RULES,));full_type.append(dict(file=name,admitted_al=ok&255,resolved_image=m.string(self.typ+0x1F8),voxel=u.mem_read(self.typ+0x236,1)[0],primary_flh=base.xyz(u,self.typ+0x89C),turret_offset=base.i32(u,self.typ+0x720)))
  self.inputs['full_type_layers']=full_type
  for p in (0xB0F720,0x8B4108,0xA8EC78,0x8B3DC0,0xB0F5D8,0xB0F6C8):u.mem_write(p,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
  self.src=m.alloc(0x1000);m.invoke(0x7353C0,self.src,(self.typ,0))
  self.after_ctor=self.state()
  # Explicit supplied single human House boundary, retaining native source body.
  self.country=self.countries['Americans']
  country_rows=[]
  for name,path in base.layers():
   if not path.exists():continue
   sections,lines=base.lexical(path.read_bytes(),{'Americans'});m.rules_cache(sections);ok=m.invoke(0x511850,self.country,(RULES,));country_rows.append(dict(file=name,source_lines=lines,admitted_al=ok&255,firepower_bits=bytes(u.mem_read(self.country+0xC8,8)).hex(),rof_bits=bytes(u.mem_read(self.country+0xE8,8)).hex(),multiplay_passive=u.mem_read(self.country+0x1A6,1)[0]))
  self.inputs['country_layers']=country_rows
  self.house=m.alloc(0x16000);u.mem_write(self.house+0x34,dwords(self.country));u.mem_write(0xA83D4C,dwords(self.house));u.mem_write(self.src+0x21C,dwords(self.house));u.mem_write(self.src+0x14C,dwords(self.house))
  u.mem_write(self.house+0x188,struct.pack('<d',1.0));u.mem_write(self.house+0x1A8,struct.pack('<d',1.0));u.mem_write(self.house+0x1EC,b'\x01');u.mem_write(self.house+0x1ED,b'\x01')
  for off in (0x5514,0x5564):u.mem_write(self.house+off+4,dwords(m.alloc(4096),1024));u.mem_write(self.house+off+0x10,dwords(0))
  u.mem_write(0xA8ED84,dwords(0));u.mem_write(0xA8E9A0,b'\x01')
  m.invoke(0x40CB80,0);m.invoke(0x4A8630,0);m.invoke(0x6D1C20,m.alloc(0x2000))
  surface=m.alloc(0x40);u.mem_write(surface,dwords(0x7E2070,640,480,0,2,0,0,0));u.mem_write(0x880A04,dwords(surface))
  for a in (0x561710,0x5617A0,0x5617C0,0x5617E0):m.invoke(a,0)
  assert m.read32(0xABDE88)==104
  # UnitType's actual post-read inverse-Turret assignment.
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EDI,self.typ);u.reg_write(UC_X86_REG_EBX,RULES);u.reg_write(UC_X86_REG_EBP,self.typ+0x24);run_checked(u,0x747739,0x74775F)
  # Reuse ConnectivityRepair's exact setup contract on this supplied crop plane.
  # Classes outside the physical crop remain blocked7; no full-map claim.
  map_ptr=base.sr.MAP
  u.mem_write(map_ptr+0x18,bytes(13*4));u.mem_write(map_ptr+0x54,dwords(0,0,1,0,10))
  for address in (0x49F0E0,0x49F190,0x49F2F0,0x49F2D0,0x49F280):m.invoke(address,0)
  root=m.alloc(16);buckets=m.alloc(256*24);template=m.alloc(24);m.invoke(0x58AFF0,template,(0,0))
  u.mem_write(template,dwords(0x7ED540));u.mem_write(template+16,dwords(0,20));u.mem_write(buckets,bytes(u.mem_read(template,24))*256);u.mem_write(root,dwords(buckets,0x56CB80,256,20));u.mem_write(map_ptr+0x14,dwords(root))
  m.invoke(0x56C510,map_ptr);self.initial_zone_count=m.read32(map_ptr+0x4C)
  self.coord=m.alloc(12);m.invoke(0x486840,self.resident.ptrs[87,50],(self.coord,))
  self.phase='placement';self.placement_result=m.invoke(0x737BA0,self.src,(self.coord,0x80));self.after_placement=self.state()
  # A read-only observer can record the original return before this fixture's
  # later command. The ordinary mission setup and its refusal assertion stay here.
  if placement_observer is not None:placement_observer()
  assert self.placement_result&255==1
  self.before_continuation=dict(actor=self.state(),rng={k:base.sr.rng_state(u,p) for k,p in self.resident.rngs.items()})
  self.frame=1
  if self.continuation is not None:
   value=self.continuation
   assert set(value)=={'provenance','command_frame','rng','actor'},set(value)
   assert value['provenance'] and isinstance(value['command_frame'],int) and value['command_frame']>=0
   assert set(value['rng'])==set(self.resident.rngs)
   self.frame=value['command_frame']
   for stream,p in self.resident.rngs.items():
    r=value['rng'][stream];assert set(r)=={'disabled','index_a','index_b','state'} and len(r['state'])==250
    assert 0<=r['index_a']<250 and 0<=r['index_b']<250 and r['disabled'] in (0,1)
    u.mem_write(p,base.sr.state_bytes(r))
   assert set(value['actor'])<= {'random_phase_raw_u16'}
   if 'random_phase_raw_u16' in value['actor']:
    phase=value['actor']['random_phase_raw_u16'];assert 0<=phase<=65535;u.mem_write(self.src+0x3C8,struct.pack('<H',phase))
  u.mem_write(0xA8ED84,dwords(self.frame));self.before_command=dict(actor=self.state(),rng={k:base.sr.rng_state(u,p) for k,p in self.resident.rngs.items()});self.phase='command'
  arr=m.alloc(4);u.mem_write(arr,dwords(self.house));u.mem_write(0xA8022C,dwords(arr))
  assert m.read32(0xB0E844)==1 and m.read32(m.read32(0xB0E840)+4)==self.src
  self.command=m.alloc(0x70);m.invoke(0x4C6860,self.command,(0,m.read32(m.read32(0xB0E840)),0x34,1,54087,0xB,0,0,0,0))
  self.command_bytes=bytes(u.mem_read(self.command,0x6F)).hex();m.invoke(0x4C6CB0,self.command);self.after_command=self.state()
  self.rng_after_command={k:base.sr.rng_state(u,p) for k,p in self.resident.rngs.items()}
  self.phase='logic'
 def tick(self):
  m=self.m;u=self.u;self.frame=m.read32(0xA8ED84);self.phase='logic';start=len(self.events);resident_start=len(self.resident.trace)
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EDI,0x87F778);run_checked(u,0x55B5FF,0x55B61B,count=5000000)
  self.frames.append(dict(state=self.state(),target=self.resident.snapshot(self.resident.ptrs[87,54]),event_range=[start,len(self.events)],resident_range=[resident_start,len(self.resident.trace)],logic_count=m.read32(0x87F788),anim_count=m.read32(0xA8E9B8),deferred_count=m.read32(0xB0F6A8)))
  # Original Main_Tick increments the frame before deferred physical finalization.
  # Other Logic/global phases are deliberately outside this isolated object pass.
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EDI,0);run_checked(u,0x55DE73,0x55DE87)
  self.frame=m.read32(0xA8ED84);self.phase='drain';m.invoke(0x725C70,0)
 def run(self):
  collapse=None
  for _ in range(5000):
   self.tick()
   if collapse is None and self.resident.snapshot(self.resident.ptrs[87,54])['overlay']==232:collapse=self.frame-1
   if collapse is not None and self.frame>collapse+40:break
  self.collapse_frame=collapse
  assert collapse is not None,'No collapse within5000 native object passes'
  self.after_unit_ai=self.state()
  self.rng_after_logic={k:base.sr.rng_state(self.u,p) for k,p in self.resident.rngs.items()}
  self.span_after_logic=[self.resident.snapshot(self.resident.ptrs[x,y]) for y in range(51,58) for x in range(86,89)]




def generate(continuation=None):
 q=Mission(continuation);failure=None
 try:q.setup();q.run()
 except Exception as exc:
  cs=Cs(CS_ARCH_X86,CS_MODE_32);failure=dict(error=str(exc),trace=[f'{a:08x}: '+ '; '.join(f'{i.mnemonic} {i.op_str}' for i in cs.disasm(bytes(q.u.mem_read(a,15)),a,count=1)) for a in q.trace])
 text_hash=hashlib.sha256(bytes(q.u.mem_read(0x401000,0x3E0000))).hexdigest();assert text_hash==q.resident.code_hash
 result=dict(native_sha256=NATIVE_SHA256,text_sha256=text_hash,continuation=continuation,inputs=q.inputs,world=q.world,events=q.events,state=q.state() if q.src else None,after_ctor=getattr(q,'after_ctor',None),before_continuation=getattr(q,'before_continuation',None),before_command=getattr(q,'before_command',None),after_command=getattr(q,'after_command',None),after_unit_ai=getattr(q,'after_unit_ai',None),placement_result=getattr(q,'placement_result',None),placement_al=(q.placement_result&255) if hasattr(q,'placement_result') else None,failure=failure,shots=q.shots,frames=q.frames,impacts=q.impacts,collapse_frame=getattr(q,'collapse_frame',None),rng_after_command=getattr(q,'rng_after_command',None),rng_final={k:base.sr.rng_state(q.u,p) for k,p in q.resident.rngs.items()},resident_trace=q.resident.trace,span_after_logic=getattr(q,'span_after_logic',None),command_bytes=getattr(q,'command_bytes',None))
 if failure is not None:(HERE/'mission.failure.latest.json').write_text(json.dumps(result,indent=2)+'\n')
 assert failure is None,failure
 assert not q.pending,q.pending
 assert len(q.shots)==len(q.impacts)
 return result

def metadata():
 return provenance(scope=__doc__,entry_points={'mission_control_ctor':0x5B3700,'mission_control_reader':0x5B3760,'rules_general':0x66D530,'rules_radiation':0x66CF70,'unit_type_reader':0x747620,'country_ctor':0x5113F0,'country_reader':0x511850,'unit_ctor':0x7353C0,'drive_factory':0x6C4010,'unit_unlimbo':0x737BA0,'command_ctor':0x4C6860,'command_dispatch':0x4C6CB0,'logic_object_loop':0x55B5FF,'unit_ai':0x7360C0,'foot_ai':0x4DA530,'techno_ai':0x6F9E50,'mission_dispatch':0x5B3060,'attack_mission':0x4D4DC0,'attack_rng_call':0x4D4EA6,'firing_update':0x736DF0,'unit_fire':0x741340,'bullet_ai':0x4666E0,'anim_ai':0x423AC0,'area_damage':0x489280,'concrete_damage':0x57CCF0,'detach':0x70D4A0,'frame_increment':0x55DE73,'retirement_drain':0x725C70,'connectivity':0x56C510},assumptions=[
  'Frozen v1 mtnk_attack prepares physical Anytown XMP03T4.MAP crop and actual MTNK/105mm/Cannon/AP/ART/selected SOUND native readers. This extension preserves v1 files and adds original full UnitType and selected complete common Rules readers, native MissionControl registry/readers, country/side registry constructors and original Americans country reader. The original full ObjectType Image reader corrects the shot-only v1 MTNK image boundary: actual RULESMD[MTNK]Image=GTNK selects ART[GTNK]PrimaryFireFLH=150,0,100; v1 omitted Image and used constructor MTNK art190,25,120. The measured final native fields are recorded per layer.',
  'Original complete Unit/Foot/Techno constructors execute, including Scenario Next6F3254 and raw AX store+3C8. Actual Drive COM factory/constructor, Unit/Techno/Object Unlimbo, CanEnter, Mark, Cell/Recalc, sight, Logic and Display registration run at frame0 on physical bank87,50. Facing0x80 is a supplied placement argument. The source begins Guard with HP300 at[22400,12928,416].',
  'Original Event4C6860/4C6CB0 command uses human House index0, the constructor-registered native Unit ID/tag0x34, Attack mission1 and Cell target token54087/tag0xB. It actually queues Attack and assigns the physical Cell. It does not execute mouse/UI force-fire admission or a multiplayer event queue.',
  'The original live Logic object walk55B5FF..55B61B runs once per supplied tick, allowing actual append/remove effects and callbacks through native vtables. Original UnitAI/FootAI/TechnoAI/MissionAI, mission and per-frame firing decisions execute without result substitutions. Original frame increment55DE73..55DE87 follows each pass, then complete original725C70 deferred drain. All current Bullet/Anim callbacks and source target release execute.',
  'Seed0 complete Main/Scenario/MapGen states are supplied by the frozen physical fixture before constructor execution. No reseeding occurs by default. Optional --continuation deliberately imports recorded full RNG states, command frame and optional source random-phase word after native construction/placement, before command execution; original constructor draws are not re-consumed after import. Input provenance and before/after values are recorded.',
  'Original56C510 builds connectivity from supplied physical crop classes/levels. Outside-crop classes remain blocked7. Real physical TMP bodies and native overlay/theater readers remain inherited; this is not a whole-map native load. Original .text must retain its binary hash. FPCW and bounded host runtime follow existing BulletReader owner.',
 ],substitutions=[
  'Full Scenario, House and map load are excluded. Single human House storage is supplied with Americans country pointer, combat multipliers1, human/current-player flags and valid selected counters. Source constructor uses null House, then the supplied House binds before Unlimbo; full House ctor/AddTracking/bookkeeping are excluded. Physical country/side constructors and selected country reads do not establish complete native Rules load chronology.',
  'Logic global prefix/suffix, other units/Houses/Factories, tactical/map updates, multiplayer event scheduling, rendering and frame-pause gates are excluded. A supplied monotonic tick invokes only the original live object pass and original frame-commit/deferred-drain slices. This establishes the isolated selected live-owner continuation, not ordinary full-match shot count or production ScenarioLoad alignment.',
  'Native full UnitType reader has explicit VXL/turret-shape file-loader boundaries5F8110/5F8CE0 returning no loaded visual data. Selected native ART/FLH and animation SHP inputs remain real. Full type Voice/MoveSound indexes outside the selected sound registry are unresolved; playback7509E0 is recorded, excluding device work and Main audio RNG. Radar tracker655560/655740 are presentation sinks; inherited screen/radar callbacks remain.',
  'Inherited physical source-order INI caches, successful heap/file mappings and CPU bootstrap remain. COM OS activation routes to actual original Drive/Bullet factories; imported ASCII/CLSID conversion, OleRun, Interlocked, IsBadReadPtr, empty Windows SEH, atexit and setup wall-clock services are supplied. No gameplay return is replaced.',
  'Native hierarchy586990 remains an inherited callback; whole-map hierarchy and movement after collapse are not shown. Engineer approach/hut admission/repair are outside this extension and remain separate frozen witnesses. CliffBack ctor0 versus production missing-key default2 remains a separate follow-up.',
 ])

if __name__=='__main__':
 parser=argparse.ArgumentParser(add_help=False);parser.add_argument('--continuation',type=Path);mode=parser.add_mutually_exclusive_group();mode.add_argument('--foot-missions',action='store_true');mode.add_argument('--foot-capture',action='store_true');mode.add_argument('--unit-unlimbo',action='store_true');args,remaining=parser.parse_known_args()
 supplied=json.loads(args.continuation.read_text()) if args.continuation else None
 if args.foot_missions:
  assert args.continuation is None,'--foot-missions has explicit per-row state inputs'
  from .foot_missions import publish
  publish(remaining)
  raise SystemExit(0)
 if args.foot_capture:
  assert args.continuation is None,'--foot-capture has explicit per-row state inputs'
  from .foot_missions import publish_capture
  publish_capture(remaining)
  raise SystemExit(0)
 if args.unit_unlimbo:
  assert args.continuation is None,'--unit-unlimbo has explicit per-row state inputs'
  from .unit_unlimbo import publish
  publish(remaining)
  raise SystemExit(0)
 if args.continuation:assert '--output' in remaining,'A continuation requires an explicit output; preserve the seed0 reference'
 if supplied is not None:
  finish_unpublished_vectors(lambda:generate(supplied),HERE/'mission.continuation.json',provenance=metadata,argv=remaining)
 else:
  finish_vectors(generate,HERE/'mission.json.gz',provenance=metadata,argv=remaining)
