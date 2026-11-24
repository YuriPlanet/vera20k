"""Selected original Foot missions, threat wrapper, idle, firing, emission, stage clocks and Infantry frame selection using Anytown Mission.

Invoke through mission --foot-missions, or --foot-capture for three Capture
cadence controls. No scanner, approach, RNG or map decision
is reimplemented here. Explicit prior-state controls and the scanner-return seam
are distinguished from the original full scanner rows; see foot_missions.md.
"""
import os, sys, json, hashlib, struct, time
from pathlib import Path
from collections import deque

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]

from tools.spatial_oracle.anytown_damage import mission as native_owner
from tools.native_oracle import RET_MAGIC, run_checked, finish_vectors, provenance, file_span, image_bytes
from tools.spatial_oracle.building_body_rules import INI, RULES, SP, dwords
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.spatial_oracle.bridge_target_composed import GENERAL_KEYS
from tools.spatial_oracle.locomotor_at_coord import FAMILIES
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *

class FootMissions(native_owner.Mission):
 def observe(self,u,a,n,d):
  if a==0x517B74:
   sp=u.reg_read(UC_X86_REG_ESP)
   clsid,outer,context,iid,ppv=struct.unpack('<5I',u.mem_read(sp,20))
   assert self.m.read32(self.e1_type+0x34C)==self.m.read32(clsid)
   assert (outer,context)==(0,7)
   u.mem_write(sp,dwords(a+6,0,outer,iid,ppv));u.reg_write(UC_X86_REG_EIP,0x6C4790)
   self.events.append(dict(kind='COM_Walk_original_factory',factory='0x006C4790',clsid=bytes(u.mem_read(clsid,16)).hex(),iid=bytes(u.mem_read(iid,16)).hex()))
   return
  if a==0x517B83:
   sp=u.reg_read(UC_X86_REG_ESP)
   self.events.append(dict(kind='OS_OleRun',interface=hex(self.m.read32(sp))))
   u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,a+6)
   return
  super().observe(u,a,n,d)

 def initialize_companion(self):
  self.setup();m,u=self.m,self.u
  self.phase='setup'
  # Existing owner's registry convention, now for the original InfantryType.
  u.mem_write(0xA8E348,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
  u.mem_write(0xA83DE8,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
  self.e1_type=m.alloc(0x1900)
  m.invoke(0x5236A0,self.e1_type,(m.cstring('E1'),))
  art,_=lexical((Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])/'ARTMD.INI').read_bytes(),{'E1','GI','GISequence','MTNK','GTNK'})
  m.make_ini(art)
  self.mission_inputs=[];self.e1_weapon_read_attempts=[]
  for name,path in native_owner.base.layers():
   if not path.exists():
    assert name=='LANGRULE.INI';self.mission_inputs.append(dict(file=name,absent=True));continue
   raw=path.read_bytes();selected,lines=lexical(raw,{'Rescue','Area Guard','Guard','Attack','E1','M60','M60E','Invisible','SA','General','ElevationModel'})
   general={key:value for key,value in selected.get('General',{}).items() if key in GENERAL_KEYS}
   m.rules_cache({'General':general} if general else {});m.invoke(0x668BF0,self.rules,(RULES,))
   m.rules_cache(selected)
   elevation=m.invoke(0x66D150,self.rules,(RULES,))
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,RULES)
   run_checked(u,0x679C92,0x679CAF,required_addresses=[0x5B3760])
   e1=m.invoke(0x5240A0,self.e1_type,(RULES,))
   weapon=m.read32(self.e1_type+0x898)
   if 'M60' in selected:
    weapon_al=m.invoke(0x772080,weapon,(RULES,))&255
    projectile=m.read32(weapon+0xA0);warhead=m.read32(weapon+0xAC)
    projectile_al=m.invoke(0x46BEE0,projectile,(RULES,))&255;warhead_al=m.invoke(0x75D3A0,warhead,(RULES,))&255
    m.invoke(0x7729F0,weapon)
    self.e1_weapon_read_attempts.append(dict(file=name,weapon=dict(entry='0x00772080',name=m.string(weapon+0x24),returned_al=weapon_al),
     projectile=dict(entry='0x0046BEE0',name=m.string(projectile+0x24),returned_al=projectile_al),
     warhead=dict(entry='0x0075D3A0',name=m.string(warhead+0x24),returned_al=warhead_al),postpass='0x007729F0'))
   self.mission_inputs.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),inputs=selected,lines=lines,
    mission={str(k):bytes(u.mem_read(0xA8E3A8+k*32,32)).hex() for k in (5,11,21)},
    e1_reader_al=e1&255,e1_image=m.string(self.e1_type+0x1F8),e1_range=native_owner.base.i32(u,weapon+0xB4),
    e1_locomotor=bytes(u.mem_read(self.e1_type+0x34C,16)).hex(),elevation_reader_al=elevation&255,elevation_increment=m.read32(self.rules+0x1838),
    elevation_bonus_bits=bytes(u.mem_read(self.rules+0x1840,8)).hex(),elevation_bonus_cap_bits=bytes(u.mem_read(self.rules+0x1848,8)).hex()))
  self.e1_weapon=m.read32(self.e1_type+0x898)
  self.e1=m.alloc(0x1000);m.invoke(0x517A50,self.e1,(self.e1_type,0))
  self.e1_ctor=dict(actor=hex(self.e1),vtable=hex(m.read32(self.e1)),loco=hex(m.read32(self.e1+0x674)),
   archive=hex(m.read32(self.e1+0x218)),target=hex(m.read32(self.e1+0x2B4)),health=native_owner.base.i32(u,self.e1+0x6C),
   flags=m.read32(self.e1+0x14),limbo=u.mem_read(self.e1+0x81,1)[0],doing=native_owner.base.i32(u,self.e1+0x6C4))
  initial_ctor=self.initial_action_snap(self.e1)
  u.mem_write(self.e1+0x21C,dwords(self.house));u.mem_write(self.e1+0x14C,dwords(self.house))
  # Same supplied House count-vector contract as the existing Unit owner;
  # original Infantry Unlimbo indexes its distinct live-count vector+5578.
  for off in (0x5528,0x5578):
   u.mem_write(self.house+off+4,dwords(m.alloc(4096),1024));u.mem_write(self.house+off+0x10,dwords(0))
  self.phase='placement'
  self.e1_unlimbo=[]
  for xy in ((87,49),(88,50),(86,50),(88,49)):
   m.invoke(0x486840,self.resident.ptrs[xy],(self.coord,))
   value=m.invoke(0x51DFF0,self.e1,(self.coord,0x80))
   self.e1_unlimbo.append(dict(cell=list(xy),al=value&255))
   if value&255:break
  assert self.e1_unlimbo[-1]['al'],self.e1_unlimbo
  self.initial_action_receipt=self.probe_initial_action(initial_ctor)
  self.e1_ready=m.invoke(0x51D6F0,self.e1,(0,0,0))
  self.e1_placement=dict(doing=native_owner.base.i32(u,self.e1+0x6C4),xyz=native_owner.base.xyz(u,self.e1+0x9C),on_bridge=u.mem_read(self.e1+0x8C,1)[0])
  self.baselines={p:bytes(u.mem_read(p,0x1000)) for p in (self.src,self.e1)}
  self.loco_baselines={m.read32(p+0x674)-4:bytes(u.mem_read(m.read32(p+0x674)-4,0x100)) for p in (self.src,self.e1)}
  self.phase='setup'
  self.initializers=[]
  for table in (0x8141D8,0x815040):
   for address in struct.unpack('<14I',u.mem_read(table,56)):
    m.invoke(address,0);self.initializers.append(hex(address))
  self.enemy=m.alloc(0x16000)
  u.mem_write(self.enemy+0x30,dwords(1,self.country));u.mem_write(self.enemy+0x188,struct.pack('<d',1.0));u.mem_write(self.enemy+0x1A8,struct.pack('<d',1.0));u.mem_write(self.enemy+0x1EC,b'\1')
  for house in (self.house,self.enemy):
   u.mem_write(house+0x5600,dwords(-1))
   for reg,value in ((UC_X86_REG_EBP,house),(UC_X86_REG_ESI,self.country),(UC_X86_REG_EBX,0)):u.reg_write(reg,value)
   run_checked(u,0x4F643B,0x4F6455)
  self.victim=m.alloc(0x1000);m.invoke(0x7353C0,self.victim,(self.typ,0))
  self.candidate=m.alloc(0x1000);m.invoke(0x7353C0,self.candidate,(self.typ,0))
  for p,house,xyz in ((self.victim,self.house,(23040,12928,416)),(self.candidate,self.enemy,(23296,12928,416))):
   u.mem_write(p+0x21C,dwords(house));u.mem_write(p+0x9C,dwords(*xyz))
  for off in (0x5514,0x5564):
   u.mem_write(self.enemy+off+4,dwords(m.alloc(4096),1024));u.mem_write(self.enemy+off+0x10,dwords(0))
  self.candidate_unlimbo=[];self.phase='placement'
  for xy in ((88,50),(86,50),(88,49),(86,49)):
   m.invoke(0x486840,self.resident.ptrs[xy],(self.coord,))
   value=m.invoke(0x737BA0,self.candidate,(self.coord,0x80))
   self.candidate_unlimbo.append(dict(cell=list(xy),al=value&255))
   if value&255:break
  assert self.candidate_unlimbo[-1]['al'],self.candidate_unlimbo
  self.candidate_placement=dict(xyz=native_owner.base.xyz(u,self.candidate+0x9C),display_layer=m.read32(self.candidate+0x94),flags=m.read32(self.candidate+0x14),limbo=u.mem_read(self.candidate+0x81,1)[0])
  self.phase='placement';m.invoke(0x486840,self.resident.ptrs[86,50],(self.coord,))
  self.victim_unlimbo=m.invoke(0x737BA0,self.victim,(self.coord,0x80))
  assert self.victim_unlimbo&255
  self.victim_placement=dict(xyz=native_owner.base.xyz(u,self.victim+0x9C),display_layer=m.read32(self.victim+0x94),flags=m.read32(self.victim+0x14),limbo=u.mem_read(self.victim+0x81,1)[0])
  self.prototype_baselines={p:bytes(u.mem_read(p,0x1000)) for p in (self.victim,self.candidate)}
  self.cell_baselines={p:bytes(u.mem_read(p,0x200)) for p in self.resident.ptrs.values()}
  self.entry_getcoords={m.read32(m.read32(p)+0x48) for p in (self.src,self.e1,self.victim,self.candidate,*self.resident.ptrs.values())}
  self.vtables={hex(v):bytes(u.mem_read(v,0x600)).hex() for v in {m.read32(p) for p in (self.src,self.e1,self.victim,self.candidate,*self.resident.ptrs.values())}}
  self.phase='logic';self.events.clear();self.pending.clear()

 def snap(self,p):
  m,u=self.m,self.u
  return dict(mission=native_owner.base.i32(u,p+0xAC),queued=native_owner.base.i32(u,p+0xB4),status=native_owner.base.i32(u,p+0xBC),
   archive=hex(m.read32(p+0x218)),target=hex(m.read32(p+0x2B4)),nav=hex(m.read32(p+0x5A4)),
   dispatch=[native_owner.base.i32(u,p+0xC8),native_owner.base.i32(u,p+0xD0)],
   position=native_owner.base.xyz(u,p+0x9C),on_bridge=u.mem_read(p+0x8C,1)[0],loco_head=native_owner.base.xyz(u,m.read32(p+0x674)+FAMILIES['walk' if m.read32(p)==0x7EB058 else 'drive']['head_offset']),firing=u.mem_read(p+0x68D,1)[0],scan=u.mem_read(p+0x688,1)[0],
   visit=m.read32(p+0xC4),targeting_timer=list(struct.unpack('<3i',u.mem_read(p+0x180,12))),idle_timer=list(struct.unpack('<3i',u.mem_read(p+0x168,12))),
   doing=native_owner.base.i32(u,p+0x6C4) if m.read32(p)==0x7EB058 else None)

 def initial_action_snap(self,p):
  m,u=self.m,self.u;interface=m.read32(p+0x674)
  return dict(actor=hex(p),vtable=hex(m.read32(p)),doing=native_owner.base.i32(u,p+0x6C4),
   mission=native_owner.base.i32(u,p+0xAC),queued=native_owner.base.i32(u,p+0xB4),
   alive=u.mem_read(p+0x90,1)[0],limbo=u.mem_read(p+0x81,1)[0],position=native_owner.base.xyz(u,p+0x9C),on_bridge=u.mem_read(p+0x8C,1)[0],
   walk_interface=hex(interface),walk_vtable=hex(m.read32(interface)),walk_moving_getter=hex(m.read32(m.read32(interface)+0x10)),walk_moving=u.mem_read(interface+0x30,1)[0],
   walk_destination=native_owner.base.xyz(u,interface+0x18),walk_head=native_owner.base.xyz(u,interface+0x24),
   speed_fraction_bits=bytes(u.mem_read(p+0x578,8)).hex(),frame_f8=native_owner.base.i32(u,p+0xF8),
   sequence_timer_words=list(struct.unpack('<4i',u.mem_read(p+0x100,16))),body_facing_words=list(struct.unpack('<6I',u.mem_read(p+0x388,24))))

 def probe_initial_action(self,ctor):
  # Execute before this companion's explicit Ready; restore all native writes,
  # CPU/x87, heap cursor and inherited observations to preserve existing rows.
  m,u=self.m,self.u;p=self.e1;before=self.initial_action_snap(p)
  rng_before={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  cpu=u.context_save();cursor=m.cursor;phase=self.phase;inherited_events=list(self.events);inherited_pending=dict(self.pending)
  events=[];writes=[];journal=[];returns={};instruction=[0]
  entries={0x520AE0:('sequencer',0),0x75AB30:('walk_is_moving',1),0x51D6F0:('do_action',3),0x4C9300:('primary_facing',1),0x65C7E0:('rng_ranged',2),0x65C780:('rng_next',0)}
  def observe(uc,a,n,d):
   instruction[0]+=1;sp=uc.reg_read(UC_X86_REG_ESP)
   if a in returns:
    for event in returns.pop(a):event['returned_eax']=uc.reg_read(UC_X86_REG_EAX);event['returned_al']=uc.reg_read(UC_X86_REG_EAX)&255
   if a in entries:
    kind,count=entries[a];args=[m.read32(sp+4+i*4) for i in range(count)]
    event=dict(instruction=instruction[0],pc=hex(a),kind=kind,caller=hex(m.read32(sp)),this=hex(args[0] if a==0x75AB30 else uc.reg_read(UC_X86_REG_ECX)),convention='COM_stdcall' if a==0x75AB30 else 'thiscall',args=args)
    events.append(event);returns.setdefault(m.read32(sp),[]).append(event)
  fields={0x6C4:'doing',0xF8:'frame_f8',0x100:'sequence_start',0x104:'sequence_aux',0x108:'sequence_duration',0x10C:'sequence_copy'}
  def written(uc,access,address,size,value,d):
   journal.append((address,bytes(uc.mem_read(address,size))))
   if address-p in fields:writes.append(dict(instruction=instruction[0],field=fields[address-p],pc=hex(uc.reg_read(UC_X86_REG_EIP)),offset=hex(address-p),bytes=size,value=value))
  self.phase='logic';self.events.clear();self.pending.clear()
  h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,written)
  try:
   answer=m.invoke(0x520AE0,p)
   result=dict(entry='0x00520AE0',constructor=ctor,before=before,after=self.initial_action_snap(p),rng_before=rng_before,
    rng_after={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()},returned_eax=answer,
    events=events,writes=writes,instruction_count=instruction[0],callback_events=list(self.events))
   result['rng_unchanged']=result['rng_before']==result['rng_after']
   result['native_fpcw']=u.reg_read(UC_X86_REG_FPCW)
   result['native_frame']=m.read32(0xA8ED84)
   assert m.cursor==cursor,'unexpected allocation in bounded stationary initial-action control'
   assert hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==self.resident.code_hash
  finally:
   u.hook_del(h);u.hook_del(w)
   for address,previous in reversed(journal):u.mem_write(address,previous)
   u.context_restore(cpu);self.phase=phase;self.events[:]=inherited_events;self.pending.clear();self.pending.update(inherited_pending)
  assert before==self.initial_action_snap(p)
  assert rng_before=={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  result['native_state_restored_before_explicit_ready']=True
  return result

 def locomotor_readback(self,p):
  m,u=self.m,self.u;interface=m.read32(p+0x674);vtable=m.read32(interface)
  getter=m.read32(vtable+0x10);cpu=u.context_save();stack=bytes(u.mem_read(SP,16));journal=[]
  def written(uc,access,address,size,value,d):journal.append((address,bytes(uc.mem_read(address,size))))
  hook=u.hook_add(UC_HOOK_MEM_WRITE,written)
  try:answer=m.invoke(getter,0,(interface,))&255
  finally:
   u.hook_del(hook)
   for address,previous in reversed(journal):u.mem_write(address,previous)
   u.mem_write(SP,stack);u.context_restore(cpu)
  family='walk' if m.read32(p)==0x7EB058 else 'drive'
  return dict(family=family,interface=hex(interface),vtable=hex(vtable),move_to=hex(m.read32(vtable+0x44)),
   is_moving_getter=hex(getter),is_moving_al=answer,
   destination=native_owner.base.xyz(u,interface+(0x18 if family=='walk' else 0x30)),
   head=native_owner.base.xyz(u,interface+FAMILIES[family]['head_offset']))

 def navigation_snap(self,p,queue_entries):
  m,u=self.m,self.u;state=self.snap(p);queue=m.read32(p+0x58C)
  state.update(locomotor=self.locomotor_readback(p),aux=hex(m.read32(p+0x5A0)),
   path=list(struct.unpack('<24i',u.mem_read(p+0x5E0,96))),reference_cell=list(struct.unpack('<2h',u.mem_read(p+0x558,4))),
   nav_queue=dict(vtable=hex(m.read32(p+0x588)),backing=hex(queue),capacity=m.read32(p+0x590),initialized=u.mem_read(p+0x594,1)[0],storage_owned=u.mem_read(p+0x595,1)[0],count=m.read32(p+0x598),entries=[hex(m.read32(queue+i*4)) for i in range(queue_entries)]),
   movement_timer_words=list(struct.unpack('<3i',u.mem_read(p+0x640,12))),blocked_timer_words=list(struct.unpack('<3i',u.mem_read(p+0x668,12))),
   blocked=u.mem_read(p+0x6B7,1)[0],retry=native_owner.base.i32(u,p+0x64C),frame_f8=native_owner.base.i32(u,p+0xF8),
   sequence_timer_words=list(struct.unpack('<4i',u.mem_read(p+0x100,16))),body_facing_words=list(struct.unpack('<6I',u.mem_read(p+0x388,24))))
  return state

 def cell_readback(self,p):
  u=self.u
  return dict(pointer=hex(p),cell=list(struct.unpack('<2h',u.mem_read(p+0x24,4))),level=struct.unpack('<b',u.mem_read(p+0x11B,1))[0],slope=u.mem_read(p+0x11C,1)[0],flags=self.m.read32(p+0x140),land=native_owner.base.i32(u,p+0xEC))

 def dummy_snap(self):return self.cell_readback(0xABDC50)

 def idle_snap(self,p):
  m,u=self.m,self.u;state=self.snap(p);interface=m.read32(p+0x674)
  state.update(primary_facing_words=list(struct.unpack('<6I',u.mem_read(p+0x388,24))),
   frame_f8=native_owner.base.i32(u,p+0xF8),prone_6db=u.mem_read(p+0x6DB,1)[0],
   sequence_timer_words=list(struct.unpack('<4i',u.mem_read(p+0x100,16))),
   walk_moving=dict(interface=hex(interface),vtable=hex(m.read32(interface)),getter=hex(m.read32(m.read32(interface)+0x10)),field=hex(interface+0x30),value=u.mem_read(interface+0x30,1)[0]))
  return state

 def ground_firing_snap(self,p):
  """Read the retained ground Infantry clock without advancing any receiver."""
  m,u=self.m,self.u;state=self.initial_action_snap(p)
  state.update(status=native_owner.base.i32(u,p+0xBC),target=hex(m.read32(p+0x2B4)),archive=hex(m.read32(p+0x218)),nav=hex(m.read32(p+0x5A4)),
   health=native_owner.base.i32(u,p+0x6C),firing_68d=u.mem_read(p+0x68D,1)[0],prone_6db=u.mem_read(p+0x6DB,1)[0],
   stage_changed_fc=u.mem_read(p+0xFC,1)[0],stage_timer_words=list(struct.unpack('<5i',u.mem_read(p+0x100,20))),
   rearm_timer_words=list(struct.unpack('<3i',u.mem_read(p+0x2EC,12))),dispatch_timer_words=list(struct.unpack('<3i',u.mem_read(p+0xC8,12))),
   targeting_timer_words=list(struct.unpack('<3i',u.mem_read(p+0x180,12))),idle_timer_words=list(struct.unpack('<3i',u.mem_read(p+0x168,12))),mission_visit=m.read32(p+0xC4))
  return state

 def ground_firing_case(self,name,frames,source_bridge=0,target_bridge=0,bridge_flags=False,expiry_after=None):
  # Complete selected InfantryAI calls, or their actual pre-launch prefix.
  # Every original write is restored so controls share constructor/reader state.
  m,u=self.m,self.u;p=self.e1;target=self.candidate
  locomotor_vtables={m.read32(m.read32(actor+0x674)):bytes(u.mem_read(m.read32(m.read32(actor+0x674)),0x100)) for actor in (p,target)}
  cpu=u.context_save();cursor=m.cursor;phase=self.phase;old_frame=self.frame
  inherited_events=list(self.events);inherited_pending=dict(self.pending);old_trace=list(self.trace)
  resident_trace=list(self.resident.trace);resident_pending=dict(self.resident.pending)
  events=[];writes=[];journal=[];returns={};instruction=[0];segment=['command']
  names={0x51BAB0:('infantry_ai',0),0x4DA530:('foot_ai',0),0x6F9E50:('techno_ai',0),0x5B3060:('mission_dispatch',0),
   0x5B35E0:('queue_mission',2),0x5B3570:('commence',0),0x4D4DC0:('attack_mission',0),0x70F7E0:('targeting_timer',0),
   0x5206B0:('fire_at_target',0),0x5218E0:('select_weapon',1),0x51C8B0:('infantry_fire_error',3),0x6FC0B0:('techno_fire_error',3),
   0x55AD00:('walk_fire_error',1),0x70E140:('get_weapon',1),0x7012C0:('weapon_range',1),
   0x51D6F0:('do_action',3),0x51DF60:('infantry_fire',2),0x6FDD50:('base_fire_entry_boundary',2),
   0x520AE0:('sequencer',0),0x520F40:('movement_actions',0),0x75AB30:('walk_is_moving',1),0x4C9300:('primary_facing',1),
   0x51AA10:('infantry_pointer_expired',2),0x4D9960:('foot_pointer_expired',2),0x7077C0:('techno_pointer_expired',2),
   0x51B1F0:('infantry_assign_target',1),0x6FCDB0:('techno_assign_target',1),0x565730:('xyz_cell',1),0x5657A0:('cell_lookup',1),
   0x65C640:('rng_raw_init',0),0x65C660:('rng_control',0),0x65C780:('rng_next',0),0x65C7E0:('rng_ranged',2)}
  markers={0x6FABC4:'stage_before',0x6FAC31:'stage_after',0x51BF59:'ai_fire_call',0x51BF6A:'ai_sequencer_call',0x51BF7B:'ai_movement_call'}
  fields={0xAC:'mission',0xB4:'queued',0xBC:'status',0xC4:'mission_visit',0xC8:'dispatch_start',0xCC:'dispatch_aux',0xD0:'dispatch_duration',
   0x168:'idle_start',0x16C:'idle_aux',0x170:'idle_duration',0x180:'targeting_start',0x184:'targeting_aux',0x188:'targeting_duration',
   0x2B4:'target',0x68D:'firing_68d',0x6C4:'doing',0x6DB:'prone_6db',
   0xF8:'frame_f8',0xFC:'stage_changed_fc',0x100:'stage_start',0x104:'stage_aux',0x108:'stage_duration',0x10C:'stage_repeat',0x110:'stage_increment',
   0x2EC:'rearm_start',0x2F0:'rearm_aux',0x2F4:'rearm_duration',0x388:'facing_desired',0x38C:'facing_current',0x390:'facing_start',0x394:'facing_aux',0x398:'facing_duration'}
  def compact():
   return dict(doing=native_owner.base.i32(u,p+0x6C4),frame_f8=native_owner.base.i32(u,p+0xF8),stage_changed_fc=u.mem_read(p+0xFC,1)[0],
    stage_timer_words=list(struct.unpack('<5i',u.mem_read(p+0x100,20))),firing_68d=u.mem_read(p+0x68D,1)[0],target=hex(m.read32(p+0x2B4)))
  def rng():return {key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  def observe(uc,a,n,d):
   instruction[0]+=1;sp=uc.reg_read(UC_X86_REG_ESP)
   if a in returns:
    for event in returns.pop(a):
     event.update(return_instruction=instruction[0],returned_eax=uc.reg_read(UC_X86_REG_EAX),returned_al=uc.reg_read(UC_X86_REG_EAX)&255,after=compact())
     if event['kind'] in ('xyz_cell','cell_lookup'):event['resolved_cell']=self.cell_readback(uc.reg_read(UC_X86_REG_EAX))
   if a in markers:events.append(dict(instruction=instruction[0],segment=segment[0],pc=hex(a),kind=markers[a],state=compact()))
   if a in names:
    kind,count=names[a];args=[m.read32(sp+4+4*i) for i in range(count)]
    event=dict(instruction=instruction[0],segment=segment[0],pc=hex(a),kind=kind,caller=hex(m.read32(sp)),
     this=hex(args[0] if a in (0x75AB30,0x55AD00) else uc.reg_read(UC_X86_REG_ECX)),args=args,before=compact())
    if a in (0x75AB30,0x55AD00):event['convention']='COM_stdcall'
    if a==0x565730:event['coordinate']=native_owner.base.xyz(uc,args[0])
    if a==0x5657A0:event['cell']=list(struct.unpack('<2h',uc.mem_read(args[0],4)))
    if a==0x75AB30:event['moving_field']=hex(args[0]+0x30);event['moving_byte']=uc.mem_read(args[0]+0x30,1)[0]
    if a==0x4C9300:event['desired_u16']=struct.unpack('<H',uc.mem_read(args[0],2))[0]
    if a in (0x65C640,0x65C660,0x65C780,0x65C7E0):event['stream']=next((key for key,ptr in self.resident.rngs.items() if ptr==uc.reg_read(UC_X86_REG_ECX)),hex(uc.reg_read(UC_X86_REG_ECX)))
    events.append(event);returns.setdefault(m.read32(sp),[]).append(event)
  def written(uc,access,address,size,value,d):
   journal.append((address,bytes(uc.mem_read(address,size))))
   if address-p in fields:writes.append(dict(instruction=instruction[0],segment=segment[0],field=fields[address-p],pc=hex(uc.reg_read(UC_X86_REG_EIP)),offset=hex(address-p),bytes=size,value=value))
  def supplied(address,raw):journal.append((address,bytes(u.mem_read(address,len(raw)))));u.mem_write(address,raw)
  def layer_poses(source_bridge,target_bridge,bridge_flags):
   poses=[]
   for actor,on_bridge in ((p,source_bridge),(target,target_bridge)):
    position=native_owner.base.xyz(u,actor+0x9C);cell=(int(position[0]/256),int(position[1]/256));cell_ptr=self.resident.ptrs[cell]
    before_cell=self.cell_readback(cell_ptr)
    if on_bridge:position[2]+=416;supplied(actor+0x9C,dwords(*position))
    supplied(actor+0x8C,bytes([on_bridge]))
    if bridge_flags:supplied(cell_ptr+0x140,dwords(m.read32(cell_ptr+0x140)|0x100))
    poses.append(dict(actor=hex(actor),supplied_on_bridge=on_bridge,supplied_xyz=None if not on_bridge else position,
     cell_before=before_cell,cell_after=self.cell_readback(cell_ptr),ground_object_head=hex(m.read32(cell_ptr+0xE4)),upper_object_head=hex(m.read32(cell_ptr+0xE8))))
   return poses
  self.phase='logic';self.events.clear();self.pending.clear();self.trace.clear();self.resident.trace.clear();self.resident.pending.clear()
  h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,written)
  try:
   initial=self.ground_firing_snap(p);rng_initial=rng()
   poses=layer_poses(source_bridge,target_bridge,bridge_flags)
   input_row=dict(name=name,frames=frames,entry='0x0051BAB0' if frames else '0x0051C8B0',target=hex(target),attack_queue=[1,0],source_on_bridge=source_bridge,target_on_bridge=target_bridge,
    supplied_has_bridge_flags=bridge_flags,expiry_after_completed_ai_calls=expiry_after,expiry_entry='0x0051AA10' if expiry_after is not None else None,expiry_control=1 if expiry_after is not None else None,
    comparison='original selected InfantryAI or actual pre-launch prefix' if frames else 'original fire-error admission only; Attack pursuit excluded',
    geometry_context='native constructor/Unlimbo ground placement' if not (source_bridge or target_bridge or bridge_flags) else 'supplied post-placement layer/HasBridge state; original occupancy lists retained')
   registration=self.registration();target_before=dict(self.snap(target),actor=hex(target),vtable=hex(m.read32(target)),health=native_owner.base.i32(u,target+0x6C),
    alive=u.mem_read(target+0x90,1)[0],limbo=u.mem_read(target+0x81,1)[0],object_next=hex(m.read32(target+0x30)))
   candidate_liveness=[dict(pointer=pointer,limbo=u.mem_read(int(pointer,16)+0x81,1)[0],alive=u.mem_read(int(pointer,16)+0x90,1)[0],xyz=native_owner.base.xyz(u,int(pointer,16)+0x9C),house=hex(m.read32(int(pointer,16)+0x21C))) for pointer in registration['techno']['actors']]
   assigned=m.invoke(0x51B1F0,p,(target,));queued=m.invoke(0x5B35E0,p,(1,0))
   fire_error=m.invoke(0x51C8B0,p,(target,0,1))
   command=dict(before=initial,after=self.ground_firing_snap(p),assign_target_eax=assigned,queue_mission_eax=queued,diagnostic_fire_error=fire_error,
    rng_before=rng_initial,rng_after=rng(),events=list(events),field_writes=list(writes),callback_events=list(self.events))
   frame_rows=[];expiry=None
   for index,frame in enumerate(frames):
    if expiry_after==index:
     segment[0]='expiry';begin=len(events);begin_writes=len(writes);callbacks=len(self.events);before=self.ground_firing_snap(p);before_rng=rng()
     entry=m.read32(m.read32(p)+0x28);assert entry==0x51AA10
     answer=m.invoke(entry,p,(target,1))
     expiry=dict(entry=hex(entry),args=[target,1],before=before,after=self.ground_firing_snap(p),returned_eax=answer,rng_before=before_rng,rng_after=rng(),
      events=events[begin:],field_writes=writes[begin_writes:],callback_events=self.events[callbacks:])
    segment[0]=f'ai_call_{index}';self.frame=frame;supplied(0xA8ED84,dwords(frame))
    before=self.ground_firing_snap(p);before_rng=rng();begin=len(events);begin_writes=len(writes);callbacks=len(self.events)
    supplied(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,p)
    try:stop=run_checked(u,0x51BAB0,(RET_MAGIC,0x6FDD50),count=1_000_000,required_addresses=(0x51BAB0,0x4DA530,0x6F9E50,0x5206B0))
    except Exception as error:raise RuntimeError(f'{name}: original InfantryAI frame {frame}: {error}') from error
    returned=stop==RET_MAGIC;sp=u.reg_read(UC_X86_REG_ESP)
    row=dict(native_frame=frame,entry='0x0051BAB0',stop=hex(stop),returned=returned,before=before,after=self.ground_firing_snap(p),
     rng_before=before_rng,rng_after=rng(),events=events[begin:],field_writes=writes[begin_writes:],callback_events=self.events[callbacks:])
    if returned:row['returned_eax']=u.reg_read(UC_X86_REG_EAX)
    else:
     row['base_fire_entry']=dict(this=hex(u.reg_read(UC_X86_REG_ECX)),return_pc=hex(m.read32(sp)),args=[m.read32(sp+4),m.read32(sp+8)],
      frame_f8=native_owner.base.i32(u,p+0xF8),firing_68d=u.mem_read(p+0x68D,1)[0],unexecuted=True)
     assert row['base_fire_entry']['firing_68d']==0
     assert any(e['kind']=='infantry_fire' for e in row['events'])
     assert any(write['field']=='firing_68d' and write['pc']=='0x51df70' and write['value']==0 for write in row['field_writes'])
    frame_rows.append(row)
    if not returned:break
   result=dict(input=input_row,registration_before=registration,candidate_liveness_before=candidate_liveness,actor_before=initial,target_before=target_before,
    target_after=dict(self.snap(target),alive=u.mem_read(target+0x90,1)[0],limbo=u.mem_read(target+0x81,1)[0]),poses=poses,command=command,expiry=expiry,frames=frame_rows,
    final=self.ground_firing_snap(p),rng_initial=rng_initial,rng_final=rng(),native_fpcw=u.reg_read(UC_X86_REG_FPCW),instruction_count=instruction[0])
   assert m.cursor==cursor,'unexpected allocation before native E1 launch boundary'
   assert hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==self.resident.code_hash
   assert all(bytes(u.mem_read(int(address,16),len(bytes.fromhex(raw))))==bytes.fromhex(raw) for address,raw in self.vtables.items())
   assert all(bytes(u.mem_read(address,len(raw)))==raw for address,raw in locomotor_vtables.items())
  finally:
   u.hook_del(h);u.hook_del(w)
   for address,previous in reversed(journal):u.mem_write(address,previous)
   u.context_restore(cpu);self.frame=old_frame;self.phase=phase;self.events[:]=inherited_events;self.pending.clear();self.pending.update(inherited_pending)
   self.trace.clear();self.trace.extend(old_trace);self.resident.trace.clear();self.resident.trace.extend(resident_trace);self.resident.pending.clear();self.resident.pending.update(resident_pending)
  assert initial==self.ground_firing_snap(p) and rng_initial==rng()
  return result

 def ground_firing_receipt(self):
  # Fresh VM after all frozen220 rows: no registration/read/RNG history leaks
  # backward into their payload. Reuse this sole owner's original constructors.
  self.initialize_companion();rules=self.rules_reader_receipts();weapons=self.weapon_reader_receipts()
  m,u=self.m,self.u;table=m.read32(self.e1_type+0xE3C)
  result=dict(text_sha256=self.resident.code_hash,inputs=self.inputs,world=self.world,mission_inputs=self.mission_inputs,
   setup=dict(e1=hex(self.e1),candidate=hex(self.candidate),house=hex(self.house),enemy=hex(self.enemy),
    e1_ctor=self.e1_ctor,e1_placement=self.e1_placement,e1_unlimbo=self.e1_unlimbo,candidate_placement=self.candidate_placement,candidate_unlimbo=self.candidate_unlimbo,
   original_vtables={address:hashlib.sha256(bytes.fromhex(raw)).hexdigest() for address,raw in self.vtables.items()},
    original_locomotor_vtables={hex(m.read32(m.read32(actor+0x674))):hashlib.sha256(bytes(u.mem_read(m.read32(m.read32(actor+0x674)),0x100))).hexdigest() for actor in (self.e1,self.candidate)},
    sequence_table=hex(table),fire_up_sequence=dict(index=4,bytes=bytes(u.mem_read(table+4*0x24,0x24)).hex(),native_frame_count=native_owner.base.i32(u,table+4*0x24+4)),
    fire_up_frame=native_owner.base.i32(u,self.e1_type+0xE40),action_flags_rate_bytes=bytes(u.mem_read(0x7EAF7C+4*4,4)).hex()),
   reader_context=dict(rules_reader_receipts=rules,weapon_reader_receipts=weapons,idle_action_frequency_bits=bytes(u.mem_read(self.rules+0x1710,8)).hex(),
    weapon_context='all four physical E1 normal/elite weapons and referenced children',rules_context='physical AudioVisual idle frequency; same supplied storage and key-block bounds'),cases=[])
  controls=[('E1_ground_attack',[1,2,3],{}),('E1_ground_repeated_frame',[1,1,2,3],{}),('E1_ground_elapsed_gap',[1,20,21],{}),
   ('E1_supplied_deck_attack',[1,2,3],dict(source_bridge=1,target_bridge=1,bridge_flags=True)),
   ('E1_initial_ground_to_deck_fire_error_only',[],dict(target_bridge=1,bridge_flags=True)),
   ('E1_initial_deck_to_ground_fire_error_only',[],dict(source_bridge=1,bridge_flags=True)),
   ('E1_target_expiry_before_fireup',[1],dict(expiry_after=0)),('E1_target_expiry_while_fireup_pending',[1,2],dict(expiry_after=1))]
  for name,frames,kw in controls:result['cases'].append(self.ground_firing_case(name,frames,**kw))
  result['native_text_unchanged']=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==self.resident.code_hash
  return result

 def ground_emission_inputs(self):
  """Supplement only this fresh VM; historical readers/payload remain untouched."""
  from tools.rules_oracle.bridge_child_sound import Sound,sections as sound_sections
  m,u=self.m,self.u;root=Path(os.environ['VERA20K_FOOT_EMISSION_INPUTS'])
  receipt_path=root.parent/'extraction_receipt.json';extraction=json.loads(receipt_path.read_bytes())
  physical_assets=[]
  for row in extraction['rows']:
   name=row['name'];raw=(root/name).read_bytes();m.assets[name.upper()]=raw
   physical_assets.append(dict(name=name,bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest(),header8_hex=raw[:8].hex(),source_archive=row['result']['source_archive'],entry_id=row['result']['entry_id']))
  assert len(physical_assets)==10
  raw=(Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])/'SOUNDMD.INI').read_bytes();physical=sound_sections(raw)
  wanted={row['name'] for row in self.inputs['sound']['rows']}|{'GIAttack'}
  selected={key:value for key,value in physical.items() if key in wanted|{'Defaults'}}
  selected['SoundList']={key:value for key,value in physical['SoundList'].items() if value in wanted}
  # Retain existing fixture-local sample identities before extending its registry.
  samples={}
  for row in self.inputs['sound']['rows']:
   index=m.invoke(0x7514D0,m.cstring(row['name']));p=m.read32(m.read32(m.read32(0xB1D37C)+index*4))
   assert m.read32(p+0x134)==len(row['samples'])
   for j,name in enumerate(row['samples']):samples[name]=m.read32(p+0xB4+j*4)
  proxy=Sound.__new__(Sound);proxy.__dict__=m.__dict__;Sound.make_ini(proxy,selected)
  looked_up=[]
  def lookup(uc,a,n,d):
   if a==0x4015C0:
    name=m.string(uc.reg_read(UC_X86_REG_EDX))
    if name not in samples:samples[name]=max(samples.values(),default=-1)+1
    looked_up.append(dict(name=name,fixture_index=samples[name]));m.ret(samples[name])
  h=u.hook_add(UC_HOOK_CODE,lookup)
  try:m.invoke(0x7510D0,INI)
  finally:u.hook_del(h)
  index=m.invoke(0x7514D0,m.cstring('GIAttack'));p=m.read32(m.read32(m.read32(0xB1D37C)+index*4))
  sound=dict(sha256=hashlib.sha256(raw).hexdigest(),selected=selected,sample_lookups=looked_up,
   name=m.string(p+0x6C),fixture_index=index,samples=[dict(index=m.read32(p+0xB4+j*4),name=next(name for name,i in samples.items() if i==m.read32(p+0xB4+j*4))) for j in range(m.read32(p+0x134))],
   retained_fields={key:m.read32(p+off) for key,off in {'control':0x10,'type_flags':0x14,'volume_fixed16_raw':0x1C,'vshift':0x68,'sample_count':0x134}.items()},
   boundary='Original sound registry/read/binding; fixture-relative sample lookup IO. Playback7509E0 remains the inherited observed request boundary; audio-device/Main audio RNG excluded.')
  self.initialize_companion();rules=self.rules_reader_receipts();weapons=self.weapon_reader_receipts()
  self.phase='setup';art_raw=(Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])/'ARTMD.INI').read_bytes()
  weapon=m.read32(self.e1_type+0x898);warhead=m.read32(weapon+0xAC)
  anims=[m.read32(m.read32(weapon+0xF8)+j*4) for j in range(m.read32(weapon+0x104))]
  impacts=[m.read32(m.read32(warhead+0x108)+j*4) for j in range(m.read32(warhead+0x114))]
  names={m.string(p+0x24) for p in anims+impacts};art,lines=lexical(art_raw,names|{'E1','GI','GISequence'})
  assert len(anims)==8 and len(impacts)==2 and len(names)==9
  m.make_ini(art);rows=[]
  # Game-speed4 is explicit; actual normalized-rate reader/constructor execute.
  u.mem_write(0xA8EB60,dwords(4))
  for p in dict.fromkeys(anims+impacts):
   name=m.string(p+0x24);mark=len(m.asset_loaded);answer=m.invoke(0x427D00,p,(INI,))
   row=m.result(name,p,answer);row['pointer']=hex(p);row['asset_loads']=m.asset_loaded[mark:];rows.append(row)
   assert row['art_body_read'] and row['raw_shp_frame_count']>0,(name,row)
  image=m.read32(self.e1_type+0xA4)
  assert image and m.string(self.e1_type+0x1F8)=='GI'
  result=dict(physical_assets=physical_assets,extraction_tool=dict(label=extraction['label'],binary_sha256=extraction['binary_sha256'],manifest_sha256=extraction['manifest_sha256']),
   art_sha256=hashlib.sha256(art_raw).hexdigest(),art_sections=art,art_lines=lines,game_speed_index=4,
   e1_image=dict(name=m.string(self.e1_type+0x1F8),pointer=hex(image),frame_count=struct.unpack('<h',u.mem_read(image+6,2))[0]),
   anim_types=rows,muzzle_slots=[dict(pointer=hex(p),name=m.string(p+0x24)) for p in anims],impact_slots=[dict(pointer=hex(p),name=m.string(p+0x24)) for p in impacts],
   report_indexes=[m.read32(m.read32(weapon+0xC0)+j*4) for j in range(m.read32(weapon+0xCC))],sound=sound,
   rules_reader_receipts=rules,weapon_reader_receipts=weapons)
  assert result['report_indexes']==[index],result['report_indexes']
  self.phase='logic';self.events.clear();self.pending.clear();return result

 def emission_memberships(self):
  m,u=self.m,self.u
  def vector(p):return dict(count=m.read32(p+0x10),actors=[hex(m.read32(m.read32(p+4)+j*4)) for j in range(m.read32(p+0x10))])
  return dict(logic=vector(0x87F778),display=[vector(0x8A0360+j*24) for j in range(5)],bullets=vector(0xA8ED40),anims=vector(0xA8E9A8),deferred=vector(0xB0F698))

 def ground_emission_case(self,name,frames,deck=False):
  from tools.spatial_oracle.anim_bouncer_launch import constructor_state
  m,u=self.m,self.u;p=self.e1;t=self.candidate
  before_rng={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  vt={hex(v):hashlib.sha256(bytes(u.mem_read(v,n))).hexdigest() for v,n in [(m.read32(p),0x600),(m.read32(t),0x600),(m.read32(m.read32(p+0x674)),0x100),(m.read32(m.read32(t+0x674)),0x100)]}
  def rng():return {key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  def target():return dict(pointer=hex(t),health=native_owner.base.i32(u,t+0x6C),mission=native_owner.base.i32(u,t+0xAC),queued=native_owner.base.i32(u,t+0xB4),target=hex(m.read32(t+0x2B4)),position=native_owner.base.xyz(u,t+0x9C),on_bridge=u.mem_read(t+0x8C,1)[0],alive=u.mem_read(t+0x90,1)[0],limbo=u.mem_read(t+0x81,1)[0])
  def compact():return dict(source=self.ground_firing_snap(p),target=target(),memberships=self.emission_memberships())
  def bullet_state(ptr):
   def named(pointer):return dict(pointer=hex(pointer),name=m.string(pointer+0x24) if pointer else None)
   return dict(pointer=hex(ptr),vtable=hex(m.read32(ptr)),position=native_owner.base.xyz(u,ptr+0x9C),
    velocity_f64_bits=[f'{value:016x}' for value in struct.unpack('<3Q',u.mem_read(ptr+0xE8,24))],
    owner=hex(m.read32(ptr+0xB0)),target=hex(m.read32(ptr+0x10C)),damage=native_owner.base.i32(u,ptr+0x6C),
    projectile=named(m.read32(ptr+0xAC)),warhead=named(m.read32(ptr+0x128)),weapon=named(m.read32(ptr+0x130)),
    detector_bytes_b8_through_e8=bytes(u.mem_read(ptr+0xB8,0x30)).hex(),alive=u.mem_read(ptr+0x90,1)[0],
    limbo=u.mem_read(ptr+0x81,1)[0],marked=u.mem_read(ptr+0x74,1)[0],logic_registered=u.mem_read(ptr+0x98,1)[0],display_layer=m.read32(ptr+0x94))
  native_vtables={}
  def pin_vtable(ptr,length):
   address=m.read32(ptr);raw=bytes(u.mem_read(address,length));original=file_span(image_bytes(),address,length)[1]
   assert raw==original,'native vtable data was changed'
   native_vtables[hex(address)]=dict(length=length,sha256=hashlib.sha256(raw).hexdigest())
  events=[];writes=[];returns={};instruction=[0];segment=['command'];constructed=[]
  entries={0x51BAB0:('infantry_ai',0),0x4DA530:('foot_ai',0),0x6F9E50:('techno_ai',0),0x5206B0:('fire_at_target',0),0x51D6F0:('do_action',3),0x51DF60:('infantry_fire',2),0x6FDD50:('techno_fire',2),0x523250:('infantry_flh',5),0x46B050:('bullet_create',5),0x466380:('bullet_ctor',0),0x468670:('bullet_fire',2),0x5880A0:('firestorm_trace',4),0x4CC100:('cliff_wall_trace',2),0x6FCFA0:('rof',1),0x421EA0:('anim_ctor',7),0x424B50:('anim_attach',1),0x520AE0:('sequencer',0),0x520F40:('movement_actions',0),0x4666E0:('bullet_ai',0),0x468D80:('detonate_wrapper',1),0x4690B0:('detonate',1),0x489280:('area_damage',4),0x737C90:('unit_damage',7),0x4D7330:('foot_damage',7),0x701900:('techno_damage',7),0x5F5390:('object_damage',7),0x48A4F0:('select_anim',2),0x423AC0:('anim_ai',0),0x4255B0:('anim_destroy',0),0x5F65F0:('uninit',0),0x7258D0:('expiry_broadcast',0),0x725C70:('deferred_drain',0),0x466560:('bullet_dtor',0),0x426590:('anim_dtor',0),0x65C7E0:('rng_ranged',2),0x65C780:('rng_next',0),0x55BAA0:('logic_register',2),0x55BAE0:('logic_remove',1),0x51B1F0:('infantry_assign_target',1),0x6FCDB0:('assign_target',1),0x5B35E0:('queue_mission',2),0x5B3570:('commence',0),0x75AB30:('walk_is_moving',1),0x4A9720:('display_register',1),0x4A9770:('display_remove',1)}
  def observe(uc,a,n,d):
   instruction[0]+=1;sp=uc.reg_read(UC_X86_REG_ESP)
   if a in returns:
    for event in returns.pop(a):
     event.update(return_instruction=instruction[0],returned_eax=uc.reg_read(UC_X86_REG_EAX),source_after=self.ground_firing_snap(p),target_after=target(),memberships_after=self.emission_memberships())
     if event['kind']=='anim_ctor':
      ptr=int(event['this'],16);event['constructed_state']=constructor_state(u,ptr);pin_vtable(ptr,0x200)
     if event['kind']=='anim_ai':event['anim_after']=constructor_state(u,int(event['this'],16))
     if event['kind'] in ('bullet_ctor','bullet_fire','bullet_ai'):event['bullet_after']=bullet_state(int(event['this'],16))
     if event['kind']=='bullet_create':event['bullet_after']=bullet_state(uc.reg_read(UC_X86_REG_EAX))
     if event['kind'].endswith('_damage') and event['kind']!='area_damage':event['damage_pointer_after']=native_owner.base.i32(u,event['args'][0])
   if a==0x55B610:events.append(dict(instruction=instruction[0],segment=segment[0],frame=m.read32(0xA8ED84),kind='logic_visit',index=uc.reg_read(UC_X86_REG_ESI),count=m.read32(0x87F788),actor=hex(uc.reg_read(UC_X86_REG_ECX))))
   if a in entries:
    kind,count=entries[a];args=[m.read32(sp+4+j*4) for j in range(count)]
    event=dict(instruction=instruction[0],segment=segment[0],frame=m.read32(0xA8ED84),pc=hex(a),kind=kind,caller=hex(m.read32(sp)),this=hex(uc.reg_read(UC_X86_REG_ECX)),args=args)
    event['ecx']=hex(uc.reg_read(UC_X86_REG_ECX));event['edx']=hex(uc.reg_read(UC_X86_REG_EDX))
    if kind in ('bullet_create','expiry_broadcast'):event['calling_convention']='fastcall'
    if kind=='expiry_broadcast':event['control_dl']=uc.reg_read(UC_X86_REG_EDX)&255
    if kind in ('bullet_fire','bullet_ai'):event['bullet_before']=bullet_state(uc.reg_read(UC_X86_REG_ECX));pin_vtable(uc.reg_read(UC_X86_REG_ECX),0x200)
    if kind=='anim_ai':event['anim_before']=constructor_state(u,uc.reg_read(UC_X86_REG_ECX));pin_vtable(uc.reg_read(UC_X86_REG_ECX),0x200)
    if kind=='walk_is_moving':event.update(this=hex(args[0]),calling_convention='COM_stdcall')
    if kind=='area_damage':event.update(calling_convention='fastcall',position=native_owner.base.xyz(u,uc.reg_read(UC_X86_REG_ECX)),damage=uc.reg_read(UC_X86_REG_EDX),warhead=m.string(args[1]+0x24))
    if kind=='select_anim':event.update(calling_convention='fastcall',damage=uc.reg_read(UC_X86_REG_ECX),warhead=m.string(uc.reg_read(UC_X86_REG_EDX)+0x24))
    if kind.endswith('_damage') and kind!='area_damage':event.update(incoming_damage=native_owner.base.i32(u,args[0]),target_before=target())
    if kind=='anim_ctor':event.update(name=m.string(args[0]+0x24),position=native_owner.base.xyz(u,args[1]),memberships_before=self.emission_memberships());constructed.append(uc.reg_read(UC_X86_REG_ECX))
    if kind in ('rng_ranged','rng_next'):event['stream']=next((key for key,ptr in self.resident.rngs.items() if ptr==uc.reg_read(UC_X86_REG_ECX)),hex(uc.reg_read(UC_X86_REG_ECX)))
    events.append(event);returns.setdefault(m.read32(sp),[]).append(event)
  fields={0xF8:'frame_f8',0xFC:'stage_changed',0x100:'stage_start',0x108:'stage_duration',0x110:'stage_increment',0x68D:'firing',0x6C4:'doing',0x2EC:'rearm_start',0x2F4:'rearm_duration',0x120:'last_fire'}
  def written(uc,access,address,size,value,d):
   if address-p in fields:writes.append(dict(instruction=instruction[0],segment=segment[0],pc=hex(uc.reg_read(UC_X86_REG_EIP)),actor='source',field=fields[address-p],offset=hex(address-p),size=size,value=value))
   if address-t in (0x6C,0xAC,0xB4,0x2B4):writes.append(dict(instruction=instruction[0],segment=segment[0],pc=hex(uc.reg_read(UC_X86_REG_EIP)),actor='target',offset=hex(address-t),size=size,value=value))
  self.phase='logic';self.events.clear();self.pending.clear();self.bullets.clear();self.trace.clear()
  h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,written)
  try:
   initial=compact();poses=[]
   if deck:
    for actor in (p,t):
     xyz=native_owner.base.xyz(u,actor+0x9C);cell=self.resident.ptrs[int(xyz[0]/256),int(xyz[1]/256)];before=self.cell_readback(cell)
     xyz[2]+=416;u.mem_write(actor+0x9C,dwords(*xyz));u.mem_write(actor+0x8C,b'\1');u.mem_write(cell+0x140,dwords(m.read32(cell+0x140)|0x100))
     poses.append(dict(actor=hex(actor),position=xyz,cell_before=before,cell_after=self.cell_readback(cell),ground_head=hex(m.read32(cell+0xE4)),upper_head=hex(m.read32(cell+0xE8))))
   original_actors=self.emission_memberships()['logic'];suffix_index=original_actors['count'];assert suffix_index==4
   m.invoke(0x51B1F0,p,(t,));m.invoke(0x5B35E0,p,(1,0));m.invoke(0x51C8B0,p,(t,0,1))
   command=dict(after=compact(),rng_after=rng());visits=[]
   for index,frame in enumerate(frames):
    segment[0]=f'initial_ai_{index}';self.frame=frame;u.mem_write(0xA8ED84,dwords(frame));begin=len(events);start=rng();before=compact()
    answer=m.invoke(0x51BAB0,p)
    visits.append(dict(frame=frame,entry='0x0051BAB0',returned_eax=answer,before=before,after=compact(),rng_before=start,rng_after=rng(),event_range=[begin,len(events)]))
    if self.bullets:break
   assert len(self.bullets)==1
   emitted=self.bullets[0];suffix_rows=[]
   for _ in range(40):
    frame=m.read32(0xA8ED84);self.frame=frame;segment[0]=f'live_suffix_{frame}';before=compact();begin=len(events);start=rng()
    if self.emission_memberships()['logic']['count']>suffix_index:
     u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EDI,0x87F778);u.reg_write(UC_X86_REG_ESI,suffix_index)
     run_checked(u,0x55B608,0x55B61B,count=2000000)
    after_pass=compact()
    u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EDI,0);run_checked(u,0x55DE73,0x55DE87)
    self.frame=m.read32(0xA8ED84);segment[0]=f'drain_{frame}';m.invoke(0x725C70,0)
    suffix_rows.append(dict(frame=frame,entry='0x0055B608',stop='0x0055B61B',supplied_start_index=suffix_index,before=before,after_pass=after_pass,after_drain=compact(),rng_before=start,rng_after=rng(),event_range=[begin,len(events)]))
    if self.emission_memberships()['logic']['count']==suffix_index and not m.read32(0xB0F6A8):break
    segment[0]=f'cleanup_source_ai_{self.frame}';begin=len(events);start=rng();before=compact();answer=m.invoke(0x51BAB0,p)
    visits.append(dict(frame=self.frame,entry='0x0051BAB0',returned_eax=answer,before=before,after=compact(),rng_before=start,rng_after=rng(),event_range=[begin,len(events)]))
   assert self.emission_memberships()['logic']['count']==suffix_index
   assert m.read32(0xA8ED50)==0 and m.read32(0xA8E9B8)==0 and m.read32(0xB0F6A8)==0
   result=dict(input=dict(name=name,frames=frames,source_on_bridge=int(deck),target_on_bridge=int(deck),game_speed_index=4,
    scheduler='Original per-source InfantryAI calls followed by original dynamic live Logic suffix from first newly appended index4; existing four actor visits/global Logic phases excluded.',
    rng_boundary='All three streams are actual retained states. Inherited7509E0 records the bound Report request before audio playback/Main RNG/device; other inherited runtime sinks remain explicit.',
    geometry='Original ground ctor/Unlimbo placement' if not deck else 'Supplied XYZ+416/OnBridge/HasBridge after ground Unlimbo; original ground/upper occupancy retained'),
    initial=initial,poses=poses,original_actor_prefix=original_actors,command=command,ai_visits=visits,logic_suffix=suffix_rows,events=events,field_writes=writes,
    bullet=hex(emitted),constructed_anims=[hex(ptr) for ptr in constructed],final=compact(),rng_before=before_rng,rng_after=rng(),
    inherited_rng_events=[event for event in self.events if event['kind'] in ('rng','raw')],runtime_boundaries=[event for event in self.events if 'boundary' in event['kind'] or 'OS_' in event['kind'] or 'COM_' in event['kind']],
    native_class_vtables=native_vtables,text_sha256=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest(),vtable_sha256=vt,instruction_count=instruction[0])
   assert result['text_sha256']==self.resident.code_hash
   assert all(hashlib.sha256(bytes(u.mem_read(int(address,16),row['length']))).hexdigest()==row['sha256'] for address,row in native_vtables.items())
   assert all(hashlib.sha256(bytes(u.mem_read(int(address,16),0x100 if int(address,16) in (m.read32(m.read32(p+0x674)),m.read32(m.read32(t+0x674))) else 0x600))).hexdigest()==digest for address,digest in vt.items())
   return result
  finally:u.hook_del(h);u.hook_del(w)

 def ground_emission_receipt(self):
  inputs=self.ground_emission_inputs()
  result=dict(text_sha256=self.resident.code_hash,inputs=inputs,world=self.world,cases=[])
  # Each fresh VM executes the exact same independent physical input closure.
  for name,frames,deck in [('E1_ground_complete',[1,2,3],False),('E1_ground_repeated_complete',[1,1,2,3],False),('E1_ground_gap_complete',[1,20,21],False),('E1_supplied_deck_complete',[1,2,3],True)]:
   if result['cases']:
    other=FootMissions();assert other.ground_emission_inputs()==inputs;case=other.ground_emission_case(name,frames,deck)
   else:case=self.ground_emission_case(name,frames,deck)
   result['cases'].append(case)
  return result
 def draw_stage_modulo_receipt(self):
  """Only the original signed count clamp/CDQ/IDIV block, not a renderer."""
  m,u=self.m,self.u;p=self.e1;table=m.alloc(0x120);offset=4*36
  state=u.context_save();frame=bytes(u.mem_read(p+0xF8,4));stack=bytes(u.mem_read(SP,32))
  rng=lambda:{key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  before_rng=rng();rows=[]
  sequence=m.read32(self.e1_type+0xE3C);physical_count=native_owner.base.i32(u,sequence+4*36+4)
  counts=(-2147483648,-7,-1,0,1,2,3,physical_count,65537,2147483647)
  values=(-2147483648,-65537,-1,0,1,65535,65536,2147483647)
  binary=image_bytes();file_offset,original=file_span(binary,0x518E08,0x19)
  text_sha=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
  vtable=m.read32(p);vtable_sha=hashlib.sha256(bytes(u.mem_read(vtable,0x600))).hexdigest()
  try:
   for count in counts:
    for value in values:
     u.mem_write(table+offset+4,dwords(count));u.mem_write(p+0xF8,dwords(value));u.mem_write(SP,dwords(RET_MAGIC))
     for reg,n in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,p),(UC_X86_REG_EAX,table),(UC_X86_REG_EBX,offset),(UC_X86_REG_ECX,0x13579BDF),(UC_X86_REG_EDX,0x2468ACE0)):u.reg_write(reg,n)
     input=dict(count=count,stage=value,count_address=hex(table+offset+4),stage_address=hex(p+0xF8),
      caller_registers=dict(eax=hex(table),ebx=offset,ebp=hex(p),esp=hex(SP),ecx='0x13579bdf',edx='0x2468ace0'),stack_first_dword=hex(RET_MAGIC))
     begin=rng();run_checked(u,0x518E08,0x518E21,required_addresses=[0x518E1E,0x518E1F])
     regs={key:struct.unpack('<i',dwords(u.reg_read(reg)))[0] for key,reg in (('eax',UC_X86_REG_EAX),('ecx',UC_X86_REG_ECX),('edx',UC_X86_REG_EDX))}
     end=rng();assert begin==end
     rows.append(dict(input=input,entry='0x00518E08',stop='0x00518E21',result_registers=regs,rng_before=begin,rng_after=end,rng_unchanged=True))
  finally:
   u.mem_write(p+0xF8,frame);u.mem_write(SP,stack);u.context_restore(state)
  assert before_rng==rng()
  assert text_sha==self.resident.code_hash==hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
  assert vtable_sha==hashlib.sha256(bytes(u.mem_read(vtable,0x600))).hexdigest()
  return dict(text_sha256=text_sha,vtable=dict(address=hex(vtable),length=0x600,sha256=vtable_sha),
   native_span=dict(address='0x00518E08',stop='0x00518E21',length=len(original),file_offset=file_offset,sha256=hashlib.sha256(original).hexdigest(),hex=original.hex()),
   physical_gi_sequence=dict(pointer=hex(sequence),supplied_index=4,fire_up_bytes=bytes(u.mem_read(sequence+4*36,36)).hex(),native_count=physical_count),
   boundary='Original isolated Infantry draw signed sequence-count clamp and StageCDQ/IDIV only; supplied caller EAX/EBX/EBP/ESP and scratch count table. Stops before518E21, so Facing, frame-index composition, drawing and renderer effects are not executed.',
   rows=rows,rng_before=before_rng,rng_after=rng(),rng_unchanged=True)

 def infantry_frame_selection_receipt(self):
  """Whole original518D80 with real E1, sequence bank, map and facing getters."""
  self.initialize_companion();m,u=self.m,self.u;p=self.e1;typ=self.e1_type
  sequence=m.read32(typ+0xE3C);bank=bytes(u.mem_read(sequence,42*36));vtable=m.read32(p)
  physical=native_owner.base.xyz(u,p+0x9C);cell=self.resident.ptrs[physical[0]//256,physical[1]//256]
  physical_land=native_owner.base.i32(u,cell+0xEC);face_input=m.alloc(4)
  assert u.mem_read(typ+0xD94,1)==b'\0'
  binary=image_bytes();off,original=file_span(binary,0x518D80,0x209)
  assert bytes(u.mem_read(0x518D80,len(original)))==original
  rng=lambda:{key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  regs=lambda:{name:u.reg_read(reg) for name,reg in (('eax',UC_X86_REG_EAX),('ebx',UC_X86_REG_EBX),('ecx',UC_X86_REG_ECX),('edx',UC_X86_REG_EDX),('esi',UC_X86_REG_ESI),('edi',UC_X86_REG_EDI),('ebp',UC_X86_REG_EBP),('esp',UC_X86_REG_ESP),('eip',UC_X86_REG_EIP),('eflags',UC_X86_REG_EFLAGS),('fpcw',UC_X86_REG_FPCW),('fpsw',UC_X86_REG_FPSW),('fptag',UC_X86_REG_FPTAG))}
  signed=lambda value:struct.unpack('<i',dwords(value))[0]
  native_rng=rng();text_sha=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
  assert text_sha==self.resident.code_hash
  vtable_bytes=bytes(u.mem_read(vtable,0x600));loco=m.read32(p+0x674);loco_vtable=m.read32(loco)
  loco_vtable_bytes=bytes(u.mem_read(loco_vtable,0x100))
  spans={name:dict(address=hex(address),bytes=size,sha256=hashlib.sha256(bytes(u.mem_read(address,size))).hexdigest()) for name,address,size in
   (('actor',p,0x1000),('actual_type',typ,0x1900),('sequence_bank',sequence,len(bank)),('current_cell',cell,0x200),('walk',loco-4,0x100),('current_player',0xA83D4C,4))}
  def one(name,group,doing,stage,bam,record=None,land=None,on_bridge=0):
   cpu=u.context_save();cpu_before=regs();cursor=m.cursor;phase=self.phase
   inherited_events=list(self.events);inherited_pending=dict(self.pending);old_trace=list(self.trace)
   resident_trace=list(self.resident.trace);resident_pending=dict(self.resident.pending)
   journal=[];writes=[];observations=[];instructions=[];instruction=[0];section=['supplied']
   def supplied(address,raw):journal.append((address,bytes(u.mem_read(address,len(raw)))));u.mem_write(address,raw)
   def observe(uc,a,n,d):
    instruction[0]+=1
    if section[0]=='draw' and 0x518D80<=a<0x518F89:instructions.append(hex(a))
    if section[0]=='draw' and a in (0x518DAE,0x518DDF,0x518DE5,0x518DF0,0x518DF6,0x518DFC,0x518E08,0x518E21,0x518F4D,0x4C93D0,0x518F52,0x518F65,0x518F6C,0x518F74,0x518F76,0x518F7A,0x518F84,0x518F88,0x5657A0):
     registers=regs();sp=registers['esp'];event=dict(instruction=instruction[0],pc=hex(a),registers=registers)
     if a==0x518DAE:
      c=registers['eax'];event.update(kind='map_cell_return',cell=hex(c),land_type=native_owner.base.i32(u,c+0xEC),physical_xy=physical[:2])
     elif a==0x518DDF:event.update(kind='observer_gate_call',receiver=hex(registers['ecx']),vtable=hex(m.read32(p)),entry=hex(m.read32(vtable+0x440)),argument=hex(m.read32(sp)))
     elif a==0x518DE5:event.update(kind='observer_gate_return',returned_al=registers['eax']&255)
     elif a==0x518DF0:event.update(kind='selected_type_call',receiver=hex(registers['ecx']),entry=hex(m.read32(vtable+0xCC)),argument=m.read32(sp))
     elif a==0x518DF6:event.update(kind='selected_type_return',selected_type=hex(registers['eax']))
     elif a==0x518DFC:event.update(kind='selected_action_bank',selected_doing=registers['ebx'],selected_type=hex(registers['esi']),actual_type=hex(m.read32(p+0x6C0)))
     elif a==0x518E08:
      address=registers['eax']+registers['ebx'];words=list(struct.unpack('<9i',u.mem_read(address,36)))
      event.update(kind='selected_record',address=hex(address),words_i32=words,start=words[0],count=words[1],stride=words[2])
     elif a==0x518E21:event.update(kind='signed_division',quotient=signed(registers['eax']),divisor=signed(registers['ecx']),remainder=signed(registers['edx']))
     elif a==0x518F4D:event.update(kind='facing_current_call',receiver=hex(registers['ecx']),destination=hex(m.read32(sp)))
     elif a==0x518F52:event.update(kind='facing_current_return',pointer=hex(registers['eax']),raw_u32=m.read32(registers['eax']),raw_hex=bytes(u.mem_read(registers['eax'],4)).hex())
     elif a==0x518F65:event.update(kind='direction_index',index=registers['eax'])
     elif a==0x518F6C:event.update(kind='direction_lookup',value_u32=registers['edx'])
     elif a==0x518F74:event.update(kind='stride_product',product_u32=registers['edx'],product_i32=signed(registers['edx']),stage_remainder=signed(registers['eax']))
     elif a==0x518F76:event.update(kind='stride_sum',sum_u32=registers['eax'],sum_i32=signed(registers['eax']))
     elif a==0x518F84:event.update(kind='frame_sum',frame_u32=registers['eax'],frame_i32=signed(registers['eax']))
     observations.append(event)
   def written(uc,access,address,size,value,d):
    journal.append((address,bytes(uc.mem_read(address,size))))
    if not SP-0x400<=address<SP+0x100:writes.append(dict(section=section[0],instruction=instruction[0],pc=hex(uc.reg_read(UC_X86_REG_EIP)),address=hex(address),bytes=size,value=value))
   self.phase='logic';self.events.clear();self.pending.clear();self.trace.clear();self.resident.trace.clear();self.resident.pending.clear()
   h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,written)
   try:
    supplied(SP-0x400,b'\xCD'*0x500);supplied(face_input,dwords(bam))
    supplied(p+0x6C4,dwords(doing));supplied(p+0xF8,dwords(stage));supplied(p+0x8C,bytes([on_bridge]))
    if record is not None:supplied(sequence+doing*36,dwords(*record))
    if land is not None:supplied(cell+0xEC,dwords(land))
    section[0]='facing_setup';facing_return=m.invoke(0x4C9300,p+0x388,(face_input,))
    caller=dict(eax=0x13579BDF,ebx=0x11223344,ecx=p,edx=0x2468ACE0,esi=0x99AABBCC,edi=0xDDEEFF00,ebp=0x55667788,esp=SP,eflags=0x202)
    for key,reg in (('eax',UC_X86_REG_EAX),('ebx',UC_X86_REG_EBX),('ecx',UC_X86_REG_ECX),('edx',UC_X86_REG_EDX),('esi',UC_X86_REG_ESI),('edi',UC_X86_REG_EDI),('ebp',UC_X86_REG_EBP),('esp',UC_X86_REG_ESP),('eflags',UC_X86_REG_EFLAGS)):u.reg_write(reg,caller[key])
    supplied(SP,dwords(RET_MAGIC));section[0]='draw'
    before=dict(doing=native_owner.base.i32(u,p+0x6C4),stage=native_owner.base.i32(u,p+0xF8),body_facing_hex=bytes(u.mem_read(p+0x388,24)).hex(),
     actual_type=hex(m.read32(p+0x6C0)),sequence_pointer=hex(m.read32(typ+0xE3C)),jumpjet=u.mem_read(typ+0xD94,1)[0],jumpjet_turn=u.mem_read(typ+0xECB,1)[0],
     target=hex(m.read32(p+0x2B4)),current_player=hex(m.read32(0xA83D4C)),physical_xyz=native_owner.base.xyz(u,p+0x9C),on_bridge=u.mem_read(p+0x8C,1)[0],current_cell=hex(cell),cell_land_type=native_owner.base.i32(u,cell+0xEC))
    begin=rng();run_checked(u,0x518D80,RET_MAGIC,count=10_000,required_addresses=[0x518D80,0x518DDF,0x518DFC,0x518E1E,0x518E1F,0x518F88])
    returned=regs();end=rng();assert begin==end==native_rng and m.cursor==cursor
    assert returned['esp']==SP+4
    assert all(returned[key]==caller[key] for key in ('ebx','esi','edi','ebp'))
    result=dict(name=name,input=dict(group=group,doing=doing,stage=stage,facing_bam_u32=bam,record_override=record,supplied_cell_land_type=land,on_bridge=on_bridge,
     caller_registers=caller,return_address=hex(RET_MAGIC),supplied_stack_window=dict(address=hex(SP-0x400),bytes=0x500,fill_u8=0xCD),facing_setup=dict(entry='0x004C9300',receiver=hex(p+0x388),argument=hex(face_input),returned_al=facing_return&255)),
     entry='0x00518D80',stop=hex(RET_MAGIC),before=before,native_observations=observations,native_instructions=instructions,native_world_writes=writes,
     output=dict(frame_u32=returned['eax'],frame_i32=signed(returned['eax']),registers=returned),rng_before=begin,rng_after=end,rng_unchanged=True,callback_events=list(self.events))
    assert not self.events,'Unexpected inherited runtime callback during full frame selection'
   finally:
    u.hook_del(h);u.hook_del(w)
    for address,previous in reversed(journal):u.mem_write(address,previous)
    u.context_restore(cpu);self.phase=phase;self.events[:]=inherited_events;self.pending.clear();self.pending.update(inherited_pending)
    self.trace.clear();self.trace.extend(old_trace);self.resident.trace.clear();self.resident.trace.extend(resident_trace);self.resident.pending.clear();self.resident.pending.update(resident_pending)
   assert cpu_before==regs() and native_rng==rng()
   assert all(hashlib.sha256(bytes(u.mem_read(int(span['address'],16),span['bytes']))).hexdigest()==span['sha256'] for span in spans.values())
   result['native_state_restored']=True
   return result
  physical_rows=[one(f'GI_doing{doing}_BAM{bam:04X}','physical_gi',doing,7,bam) for doing in range(42) for bam in range(0,0x10000,0x2000)]
  facings=list(range(0,0x10000,0x800))+[0x03FF,0x0400,0x0401,0x7FFF,0xFBFF,0xFC00,0xFC01,0xFFFF]
  facing_rows=[one(f'GI_Guard1_facing_{bam:04X}','facing_rounding',1,0,bam) for bam in facings]
  counts=(-2147483648,-7,-1,0,1,2,3,6,65537,2147483647);stages=(-2147483648,-65537,-1,0,1,65535,65536,2147483647)
  scalar_rows=[one(f'GI_supplied_count{count}_stage{stage}','signed_count_stage',4,stage,0,(2147483647,count,2147483647)) for count in counts for stage in stages]
  scalar_rows.extend(one(f'GI_supplied_start{start}_stride{stride}','signed_start_stride',4,65536,0,(start,6,stride)) for start in (-2147483648,-65537,-1,0,1,65537,2147483647) for stride in (-2147483648,-7,-1,0,1,2,65537,2147483647))
  default_rows=[one(f'GI_uninitialized_land{land}_bridge{bridge}','doing_minus1_default',-1,7,0x8000,land=land,on_bridge=bridge) for land in (physical_land,2) for bridge in (0,1)]
  assert bank==bytes(u.mem_read(sequence,len(bank))) and native_rng==rng()
  assert text_sha==hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
  assert vtable_bytes==bytes(u.mem_read(vtable,len(vtable_bytes))) and loco_vtable_bytes==bytes(u.mem_read(loco_vtable,len(loco_vtable_bytes)))
  art_path=Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])/'ARTMD.INI';art,lines=lexical(art_path.read_bytes(),{'E1','GI','GISequence'})
  return dict(text_sha256=text_sha,native_span=dict(address='0x00518D80',stop='0x00518F89',file_offset=off,length=len(original),sha256=hashlib.sha256(original).hexdigest(),hex=original.hex()),
   inputs=dict(ARTMD=dict(sha256=hashlib.sha256(art_path.read_bytes()).hexdigest(),sections=art,lines=lines),type_reader='0x005240A0',sequence_reader='0x00523D00',
    setup='Fresh existing FootMissions.initialize_companion original E1 type/Infantry/Walk constructors, layered readers and successful Unlimbo; explicit Ready fixture baseline and supplied House/crop retained.'),
   physical_gi_sequence=dict(pointer=hex(sequence),bytes=len(bank),sha256=hashlib.sha256(bank).hexdigest(),records=[dict(doing=i,raw_hex=bank[i*36:(i+1)*36].hex(),words_i32=list(struct.unpack('<9i',bank[i*36:(i+1)*36]))) for i in range(42)]),
   primary_facing_lookup=dict(address='0x007EAEFC',raw_hex=bytes(u.mem_read(0x7EAEFC,0x80)).hex(),words_i32=list(struct.unpack('<32i',u.mem_read(0x7EAEFC,0x80)))),
   real_vtables=dict(infantry=dict(address=hex(vtable),bytes=len(vtable_bytes),sha256=hashlib.sha256(vtable_bytes).hexdigest()),walk=dict(address=hex(loco_vtable),bytes=len(loco_vtable_bytes),sha256=hashlib.sha256(loco_vtable_bytes).hexdigest())),
   restored_state_spans=spans,physical_rows=physical_rows,facing_rows=facing_rows,scalar_rows=scalar_rows,default_rows=default_rows,
   coverage=dict(physical_records=42,physical_rows=len(physical_rows),facing_rows=len(facing_rows),scalar_rows=len(scalar_rows),default_rows=len(default_rows)),
   rng_before=native_rng,rng_after=rng(),rng_unchanged=True,
   boundary='Whole original518D80 frame selector returns through518F88 with actual E1, actual observer gate70EE30, selected physical GI bank, real body FacingCurrent4C93D0 and native DWORD IDIV/IMUL/ADD. Doing-1 calls real physical-coordinate virtual and Map5657A0 on the concrete current Cell. Signed first-three record fields, Stage, facing and water/OnBridge are explicit supplied prior inputs restored afterward. Actual Type+D94 is0 and observer gate returns1, so JumpJet CLSID/target-facing and observer/disguise-selected alternate banks are excluded. No SHP draw/render, native bridge placement/occupancy or water transition is claimed; no gameplay callback/result/code/vtable is substituted.')

 def stage_clock_snap(self,p):
  u=self.u
  return dict(value=native_owner.base.i32(u,p+0xF8),changed=u.mem_read(p+0xFC,1)[0],
   timer=dict(start=native_owner.base.i32(u,p+0x100),aux_raw_u32=self.m.read32(p+0x104),duration=native_owner.base.i32(u,p+0x108)),
   rate=native_owner.base.i32(u,p+0x10C),increment=native_owner.base.i32(u,p+0x110),raw_stage_bytes=bytes(u.mem_read(p+0xF8,0x1C)).hex())

 def stage_clock_case(self,control,family,p):
  # These are unchanged scalar instruction regions, not whole class AI calls.
  # Retained native writes carry between frames; supplied state is input only.
  m,u=self.m,self.u
  entry,stop,aux_offset=(0x6FABC4,0x6FAC31,0x2C) if family=='techno' else (0x4509DE,0x450A38,0x18)
  cpu=u.context_save();cursor=m.cursor;phase=self.phase;old_frame=self.frame
  inherited_events=list(self.events);inherited_pending=dict(self.pending);old_trace=list(self.trace)
  resident_trace=list(self.resident.trace);resident_pending=dict(self.resident.pending)
  original=self.stage_clock_snap(p);rng_initial={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  journal=[];events=[];writes=[];instructions=[];returns={};instruction=[0]
  fields={0xF8:'value',0xFC:'changed',0x100:'timer_start',0x104:'timer_aux_raw',0x108:'timer_duration',0x10C:'rate',0x110:'increment'}
  def rng():return {key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  def registers():return {name:hex(u.reg_read(reg)) for name,reg in (('eax',UC_X86_REG_EAX),('ebx',UC_X86_REG_EBX),('ecx',UC_X86_REG_ECX),('edx',UC_X86_REG_EDX),('esi',UC_X86_REG_ESI),('edi',UC_X86_REG_EDI),('ebp',UC_X86_REG_EBP),('esp',UC_X86_REG_ESP),('eflags',UC_X86_REG_EFLAGS))}
  def observe(uc,a,n,d):
   instruction[0]+=1;instructions.append(dict(instruction=instruction[0],pc=hex(a),bytes=bytes(uc.mem_read(a,n)).hex()))
   if a in returns:
    for event in returns.pop(a):event.update(return_instruction=instruction[0],returned_eax=uc.reg_read(UC_X86_REG_EAX),returned_signed=struct.unpack('<i',dwords(uc.reg_read(UC_X86_REG_EAX)))[0])
   if a==0x426630:
    sp=uc.reg_read(UC_X86_REG_ESP);timer=uc.reg_read(UC_X86_REG_ECX)
    event=dict(instruction=instruction[0],pc=hex(a),kind='original_timer_remaining',this=hex(timer),caller=hex(m.read32(sp)),timer_words=list(struct.unpack('<3i',uc.mem_read(timer,12))))
    events.append(event);returns.setdefault(m.read32(sp),[]).append(event)
  def written(uc,access,address,size,value,d):
   journal.append((address,bytes(uc.mem_read(address,size))))
   field=fields.get(address-p)
   if family=='building' and address==SP+0xE:field='caller_changed_local'
   if field:writes.append(dict(instruction=instruction[0],field=field,pc=hex(uc.reg_read(UC_X86_REG_EIP)),address=hex(address),offset=hex(address-p) if address-p in fields else 'esp+0xE',bytes=size,value=value))
  def supplied(address,raw):journal.append((address,bytes(u.mem_read(address,len(raw)))));u.mem_write(address,raw)
  self.phase='logic';self.events.clear();self.pending.clear();self.trace.clear();self.resident.trace.clear();self.resident.pending.clear()
  h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,written)
  try:
   supplied(p+0xF8,dwords(control['value']));supplied(p+0xFC,bytes([control['changed']]))
   supplied(p+0x100,dwords(control['start'],control['aux_raw_u32'],control['duration'],control['rate'],control['increment']))
   rows=[]
   for frame in control['frames']:
    self.frame=frame;supplied(0xA8ED84,dwords(frame));supplied(SP,dwords(RET_MAGIC));supplied(SP+aux_offset,dwords(0x13579BDF))
    if family=='building':supplied(SP+0xE,b'\xA5')
    u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,p);u.reg_write(UC_X86_REG_EBP,0)
    before=self.stage_clock_snap(p);before_rng=rng();before_registers=registers();begin=len(events);begin_writes=len(writes);begin_instructions=len(instructions);callbacks=len(self.events)
    reached=run_checked(u,entry,stop,count=10_000,required_addresses=(entry,0x426630) if family=='building' else (entry,))
    row=dict(native_frame=frame,entry=hex(entry),stop=hex(reached),before=before,after=self.stage_clock_snap(p),registers_before=before_registers,registers_after=registers(),
     rng_before=before_rng,rng_after=rng(),events=events[begin:],field_writes=writes[begin_writes:],instructions=instructions[begin_instructions:],callback_events=self.events[callbacks:],native_fpcw=u.reg_read(UC_X86_REG_FPCW),
     stack_aux_raw_u32=m.read32(SP+aux_offset),caller_changed_local=u.mem_read(SP+0xE,1)[0] if family=='building' else None)
    row['rng_unchanged']=row['rng_before']==row['rng_after'];rows.append(row)
   result=dict(input=dict(control,family=family,receiver=hex(p),entry=hex(entry),stop=hex(stop),
    receiver_context='original constructed E1; only common stage fields are read' if family=='techno' else 'supplied zero-initialized 0x200-byte scalar receiver; no Building constructor or vtable is used',
    supplied_registers=dict(esi=hex(p),ebp=0,esp=hex(SP)),supplied_caller_stack=dict(aux_offset=hex(aux_offset),aux_raw_u32=0x13579BDF,changed_local_before=0xA5 if family=='building' else None)),
    frames=rows,rng_initial=rng_initial,rng_final=rng(),instruction_count=instruction[0])
   assert m.cursor==cursor and hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==self.resident.code_hash
  finally:
   u.hook_del(h);u.hook_del(w)
   for address,previous in reversed(journal):u.mem_write(address,previous)
   u.context_restore(cpu);self.frame=old_frame;self.phase=phase;self.events[:]=inherited_events;self.pending.clear();self.pending.update(inherited_pending)
   self.trace.clear();self.trace.extend(old_trace);self.resident.trace.clear();self.resident.trace.extend(resident_trace);self.resident.pending.clear();self.resident.pending.update(resident_pending)
  assert original==self.stage_clock_snap(p) and rng_initial==rng()
  result['native_state_restored']=True
  return result

 def stage_action_restart(self,action):
  # Actual GI DoAction is the restart owner. No result, field or rate is supplied
  # after its entry; the uncommon prior clock is a declared input control.
  m,u=self.m,self.u;p=self.e1
  cpu=u.context_save();cursor=m.cursor;phase=self.phase;old_frame=self.frame
  inherited_events=list(self.events);inherited_pending=dict(self.pending);old_trace=list(self.trace)
  resident_trace=list(self.resident.trace);resident_pending=dict(self.resident.pending)
  original=self.ground_firing_snap(p);rng_initial={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  journal=[];events=[];writes=[];returns={};instruction=[0];segment=['setup']
  fields={0xF8:'value',0xFC:'changed',0x100:'timer_start',0x104:'timer_aux_raw',0x108:'timer_duration',0x10C:'rate',0x110:'increment',0x6C4:'doing'}
  names={0x51D6F0:('do_action',3),0x5216D0:('action_admission',0),0x4C9300:('primary_facing',1),0x5FB2E0:('game_options_delay',1),0x65C780:('rng_next',0),0x65C7E0:('rng_ranged',2)}
  def rng():return {key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  def observe(uc,a,n,d):
   instruction[0]+=1;sp=uc.reg_read(UC_X86_REG_ESP)
   if a in returns:
    for event in returns.pop(a):event.update(return_instruction=instruction[0],returned_eax=uc.reg_read(UC_X86_REG_EAX),returned_al=uc.reg_read(UC_X86_REG_EAX)&255)
   if a in names:
    kind,count=names[a];event=dict(instruction=instruction[0],segment=segment[0],pc=hex(a),kind=kind,this=hex(uc.reg_read(UC_X86_REG_ECX)),caller=hex(m.read32(sp)),args=[m.read32(sp+4+4*i) for i in range(count)])
    events.append(event);returns.setdefault(m.read32(sp),[]).append(event)
  def written(uc,access,address,size,value,d):
   journal.append((address,bytes(uc.mem_read(address,size))))
   if address-p in fields:writes.append(dict(instruction=instruction[0],segment=segment[0],field=fields[address-p],pc=hex(uc.reg_read(UC_X86_REG_EIP)),offset=hex(address-p),bytes=size,value=value))
  def supplied(address,raw):journal.append((address,bytes(u.mem_read(address,len(raw)))));u.mem_write(address,raw)
  self.phase='logic';self.events.clear();self.pending.clear();self.trace.clear();self.resident.trace.clear();self.resident.pending.clear()
  h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,written)
  try:
   setup_before=self.stage_clock_snap(p);setup_rng=rng();setup_answer=m.invoke(0x51D6F0,p,(4,0,0))
   setup=dict(entry='0x0051D6F0',args=[4,0,0],before=setup_before,after=self.stage_clock_snap(p),doing_after=native_owner.base.i32(u,p+0x6C4),returned_eax=setup_answer,returned_al=setup_answer&255,
    events=list(events),field_writes=list(writes),rng_before=setup_rng,rng_after=rng(),callback_events=list(self.events))
   supplied(p+0xF8,dwords(31));supplied(p+0xFC,b'\1');supplied(p+0x100,dwords(99,0x2468ACE0,9,9,7))
   self.frame=1337;supplied(0xA8ED84,dwords(self.frame));segment[0]='forced_restart';begin=len(events);begin_writes=len(writes);callbacks=len(self.events)
   before=self.stage_clock_snap(p);before_rng=rng();doing_before=native_owner.base.i32(u,p+0x6C4)
   answer=m.invoke(0x51D6F0,p,(action,1,0));table=m.read32(self.e1_type+0xE3C)
   result=dict(input=dict(name='E1_physical_GI_forced_Ready_clock_restart' if action==0 else 'E1_physical_GI_forced_same_FireUp_refusal',entry='0x0051D6F0',args=[action,1,0],native_frame=self.frame,
    supplied_prior=dict(value=31,changed=1,start=99,aux_raw_u32=0x2468ACE0,duration=9,rate=9,increment=7),
    doing_context='FireUp4 produced by original setup51D6F0(4,0,0); subsequent requested action and force are explicit arguments'),
    setup=setup,before=before,after=self.stage_clock_snap(p),doing_before=doing_before,doing_after=native_owner.base.i32(u,p+0x6C4),returned_eax=answer,returned_al=answer&255,
    events=events[begin:],field_writes=writes[begin_writes:],callback_events=self.events[callbacks:],rng_before=before_rng,rng_after=rng(),native_fpcw=u.reg_read(UC_X86_REG_FPCW),instruction_count=instruction[0],
    physical_GI=dict(table=hex(table),fire_up_sequence_bytes=bytes(u.mem_read(table+4*0x24,0x24)).hex(),requested_sequence_bytes=bytes(u.mem_read(table+action*0x24,0x24)).hex(),action_flags_rate_bytes=bytes(u.mem_read(0x7EAF7C+action*4,4)).hex()))
   result['rng_unchanged']=result['rng_before']==result['rng_after']
   assert m.cursor==cursor and hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==self.resident.code_hash
  finally:
   u.hook_del(h);u.hook_del(w)
   for address,previous in reversed(journal):u.mem_write(address,previous)
   u.context_restore(cpu);self.frame=old_frame;self.phase=phase;self.events[:]=inherited_events;self.pending.clear();self.pending.update(inherited_pending)
   self.trace.clear();self.trace.extend(old_trace);self.resident.trace.clear();self.resident.trace.extend(resident_trace);self.resident.pending.clear();self.resident.pending.update(resident_pending)
  assert original==self.ground_firing_snap(p) and rng_initial==rng()
  result['native_state_restored']=True
  return result

 def stage_clock_receipt(self):
  # Reuse the restored original-constructor VM after ground_firing_receipt. This
  # addition cannot affect any earlier payload or native RNG/registration state.
  m,u=self.m,self.u;building=m.alloc(0x200)
  controls=[dict(name='running_repeated_and_gap',frames=[100,100,101,120]),
   dict(name='not_expired_then_expired',start=100,duration=2,rate=2,frames=[100,101,102]),
   dict(name='zero_rate_zero_duration',duration=0,rate=0,frames=[100]),
   dict(name='zero_rate_elapsed_duration',start=100,rate=0,frames=[101]),
   dict(name='negative_rate_elapsed',duration=-3,rate=-3,frames=[100]),
   dict(name='negative_rate_countdown',start=100,duration=3,rate=-1,frames=[100,103,103]),
   dict(name='paused_zero_duration',start=-1,duration=0,frames=[100,100]),
   dict(name='paused_nonzero_duration',start=-1,duration=3,rate=3,frames=[100,200]),
   dict(name='paused_negative_duration',start=-1,duration=-3,rate=-3,frames=[100]),
   dict(name='zero_increment',changed=0,increment=0,frames=[100]),
   dict(name='negative_increment',value=0,increment=-1,frames=[100]),
   dict(name='signed_add_wrap_max',value=2147483647,frames=[100]),
   dict(name='signed_add_wrap_min',value=-2147483648,increment=-1,frames=[100]),
   dict(name='frame_wrap_elapsed',start=2147483647,frames=[-2147483648,-2147483647]),
   dict(name='frame_wrap_countdown',start=2147483647,duration=2,rate=2,frames=[-2147483648,-2147483647]),
   dict(name='delta_wrap_negative',start=-2147483648,frames=[2147483647]),
   dict(name='current_precedes_start',start=101,frames=[100]),
   dict(name='paused_zero_rate',start=-1,duration=0,rate=0,frames=[100])]
  result=dict(text_sha256=self.resident.code_hash,
   setup_context='same original fully initialized FootMissions VM, restored after all ground_firing_receipt controls',
   supplied_building_storage=dict(pointer=hex(building),bytes=0x200,initial_sha256=hashlib.sha256(bytes(u.mem_read(building,0x200))).hexdigest(),constructor_executed=False,vtable_used=False),
   clock_rows=[],action_restart_rows=[])
  for item in controls:
   control=dict(value=7,changed=1,start=99,aux_raw_u32=0x2468ACE0,duration=1,rate=1,increment=1)
   control.update(item)
   for family,p in (('techno',self.e1),('building',building)):result['clock_rows'].append(self.stage_clock_case(control,family,p))
  for action in (0,4):result['action_restart_rows'].append(self.stage_action_restart(action))
  result['native_text_unchanged']=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==self.resident.code_hash
  result['native_vtables_unchanged']=all(bytes(u.mem_read(int(address,16),len(bytes.fromhex(raw))))==bytes.fromhex(raw) for address,raw in self.vtables.items())
  assert result['native_text_unchanged'] and result['native_vtables_unchanged']
  return result

 def row(self,name,family='MTNK',mission=21,status=2,target=0,archive=0,nav=0,seed=31,stop=None,scan_expired=False,idle_expired=False,candidate_result=None,candidate_xyz=None,archive_xyz=None,source_xyz=None,on_bridge=None,head_xyz=None,source_cell_flags=None,archive_flags=None,candidate_live=False,dispatch_entry=False,greatest_args=None,idle_args=None,navigation_args=None,home_args=None,evidence_context=None,**flags):
  m,u=self.m,self.u;p=self.src if family=='MTNK' else self.e1
  u.mem_write(p,self.baselines[p])
  local=m.read32(p+0x674)-4;u.mem_write(local,self.loco_baselines[local])
  # Candidate controls must follow this restore of their placed, live baselines.
  for ptr,raw in self.prototype_baselines.items():u.mem_write(ptr,raw)
  for ptr,raw in self.cell_baselines.items():u.mem_write(ptr,raw)
  u.mem_write(self.candidate+0x81,bytes([not candidate_live]))
  if source_xyz is not None:u.mem_write(p+0x9C,dwords(*source_xyz))
  if on_bridge is not None:u.mem_write(p+0x8C,bytes([on_bridge]))
  if head_xyz is not None:u.mem_write(m.read32(p+0x674)+FAMILIES['walk' if family=='E1' else 'drive']['head_offset'],dwords(*head_xyz))
  if archive_flags is not None:u.mem_write(self.victim+0x14,dwords(archive_flags))
  if source_cell_flags is not None:
   xyz=native_owner.base.xyz(u,p+0x9C);xy=(int(xyz[0]/256),int(xyz[1]/256));u.mem_write(self.resident.ptrs[xy]+0x140,dwords(source_cell_flags))
  if greatest_args is not None:
   for label,ptr in self.extra_candidates.items():u.mem_write(ptr+0x81,bytes([not greatest_args['extra_live'].get(label,False)]))
  if candidate_xyz is not None:u.mem_write(self.candidate+0x9C,dwords(*candidate_xyz))
  if archive_xyz is not None:u.mem_write(self.victim+0x9C,dwords(*archive_xyz))
  if idle_args is not None:
   if idle_args['entry']!='completion':u.mem_write(p+0x6C4,dwords(idle_args['doing']))
   if 'prone' in idle_args:u.mem_write(p+0x6DB,bytes([idle_args['prone']]))
   if 'frame_f8' in idle_args:u.mem_write(p+0xF8,dwords(idle_args['frame_f8']))
   if 'loco_moving' in idle_args:u.mem_write(m.read32(p+0x674)+0x30,bytes([idle_args['loco_moving']]))
  if navigation_args is not None:
   for ptr in navigation_args['inactive_registered_candidates']:
    u.mem_write(ptr+0x81,bytes([1]));u.mem_write(ptr+0x90,bytes([0]))
   assert len(navigation_args['path'])==24
   u.mem_write(p+0x5E0,dwords(*navigation_args['path']));u.mem_write(p+0x558,struct.pack('<2h',*navigation_args['reference_cell']))
   u.mem_write(p+0x58C,dwords(navigation_args['queue_backing'],len(navigation_args['queue_entries'])))
   u.mem_write(p+0x598,dwords(len(navigation_args['queue_entries'])))
   u.mem_write(navigation_args['queue_backing'],dwords(*navigation_args['queue_entries']))
   u.mem_write(p+0x5A0,dwords(navigation_args['aux']))
   u.mem_write(p+0x640,dwords(*navigation_args['movement_timer_words']));u.mem_write(p+0x668,dwords(*navigation_args['blocked_timer_words']))
   u.mem_write(p+0x64C,dwords(navigation_args['retry']));u.mem_write(p+0x6B7,bytes([navigation_args['blocked']]))
  if home_args is not None:
   u.mem_write(self.house+0x5490,struct.pack('<2h',*home_args['primary']));u.mem_write(self.house+0x5494,struct.pack('<2h',*home_args['alternate']))
   u.mem_write(self.house+0x5498,dwords(home_args['radius']))
  # Explicit prior mission state/placement continuation, not a constructor claim.
  for off,value in [(0xAC,mission),(0xB4,-1),(0xBC,status),(0x2B4,target),(0x218,archive),(0x5A4,nav),(0xC8,0),(0xD0,0)]:u.mem_write(p+off,dwords(value))
  if scan_expired:u.mem_write(p+0x180,dwords(-1,0,0))
  if idle_expired:u.mem_write(p+0x168,dwords(-1,0,0))
  if idle_args is not None and 'timer_words' in idle_args:u.mem_write(p+0x168,dwords(*idle_args['timer_words']))
  for key,value in flags.items():
   off={'firing':0x68D,'scan':0x688,'moving':0x3D5}[key];u.mem_write(p+off,bytes([value]))
  scenario=m.read32(0xA8B230)+0x218;m.invoke(0x65C6D0,scenario,(seed,))
  start=self.snap(p);rng0={k:native_owner.base.sr.rng_state(u,p) for k,p in self.resident.rngs.items()}
  u.mem_write(SP-0x1000,bytes(0x2000))
  self.events.clear();self.pending.clear();events=[];writes=[];visited=set();returns={};last=deque(maxlen=30);journal=[];instruction=[0]
  names={0x5B3060:'dispatch',0x4DBDF0:'navigation_coordinate',0x4DDC40:'bridge_layer',0x4DDF90:'rescue',0x4D6AA0:'area_guard',0x5B35E0:'queue',0x5B3570:'commence',0x5B3A00:'mission_control',
   0x70C610:'archive',0x743190:'unit_greatest',0x51E140:'infantry_greatest',0x6F8DF0:'greatest',0x70CD10:'score',0x6F7CA0:'evaluate',0x4D9920:'foot_greatest',0x709820:'retaliate_scan',
   0x707E60:'threat_range',0x7414E0:'unit_approach',0x522340:'infantry_approach',0x4D5690:'foot_approach',
   0x6FCDB0:'assign_target',0x51B1F0:'infantry_assign_target',0x741970:'unit_destination',0x51AA40:'infantry_destination',
   0x4D94B0:'foot_destination',0x565730:'xyz_cell',0x5657A0:'cell_lookup',0x5F6360:'distance',0x65C7E0:'rng',0x65C780:'rng_next',0x65C640:'rng_raw_init',0x65C660:'rng_control',
   0x51D6F0:'infantry_action',0x7099E0:'idle_admission',0x41C040:'unit_idle',0x51CDB0:'infantry_idle',0x7091D0:'can_acquire',0x70F7E0:'targeting_timer'}
  counts={0x5B35E0:2,0x70C610:1,0x743190:3,0x51E140:3,0x6F8DF0:3,0x707E60:1,
   0x7414E0:1,0x522340:1,0x4D5690:1,0x6FCDB0:1,0x51B1F0:1,0x741970:2,0x51AA40:2,0x4D94B0:2,
   0x565730:1,0x5657A0:1,0x5F6360:1,0x65C7E0:2,0x70CD10:2,0x6F7CA0:7,0x4D9920:3,0x709820:2,0x51D6F0:3}
  for address in self.entry_getcoords:names[address]='physical_getcoords';counts[address]=1
  if idle_args is not None:
   names.update({0x4C9300:'primary_facing',0x5F3E50:'literal_type_check',0x5FB2E0:'game_options_delay',0x5216D0:'infantry_idle_admission',0x75AB30:'walk_is_moving',0x520AE0:'infantry_sequencer'});counts.update({0x4C9300:1,0x5F3E50:1,0x5FB2E0:1,0x75AB30:1})
   start=self.idle_snap(p)
  if navigation_args is not None:
   names.update({0x4AFD40:'drive_move_to',0x75ACB0:'walk_move_to',0x4E0190:'clear_nav_queue'});counts.update({0x4AFD40:4,0x75ACB0:4})
   start=self.navigation_snap(p,len(navigation_args['queue_entries']))
   destination_input_readback=dict(archive_object=None if not archive else self.snap(archive),archive_locomotor=None if not archive else self.locomotor_readback(archive),rules_repath_delay=native_owner.base.i32(u,self.rules+0x1768),frame=m.read32(0xA8ED84))
   counts.update({0x4DBDF0:2,0x4DDC40:1})
  if home_args is not None:
   names.update({0x500200:'house_return',0x501AC0:'house_random_return',0x50DEF0:'house_base_center',0x49F420:'random_direction',0x578460:'inside_map',0x586E50:'clamp_map',0x56D230:'zone',0x56DC20:'nearby_cell'})
   counts.update({0x500200:2,0x501AC0:2,0x50DEF0:1,0x49F420:2,0x578460:2,0x586E50:2,0x56D230:3,0x56DC20:15})
   home_before=self.dummy_snap()
  if evidence_context is not None:
   registration_before=self.registration()
   candidate_liveness_before=[dict(pointer=pointer,limbo=u.mem_read(int(pointer,16)+0x81,1)[0],alive=u.mem_read(int(pointer,16)+0x90,1)[0],xyz=native_owner.base.xyz(u,int(pointer,16)+0x9C),house=hex(m.read32(int(pointer,16)+0x21C))) for pointer in registration_before['techno']['actors']]
  def observe(uc,a,n,d):
   instruction[0]+=1;visited.add(a);last.append(a);sp=uc.reg_read(UC_X86_REG_ESP)
   if a in returns:
    for row in returns.pop(a):
     row['result_eax']=uc.reg_read(UC_X86_REG_EAX)
     if row['kind']=='physical_getcoords':row['xyz']=native_owner.base.xyz(uc,uc.reg_read(UC_X86_REG_EAX))
     if row['kind']=='mission_control':row['rate_bits']=bytes(uc.mem_read(uc.reg_read(UC_X86_REG_EAX)+0x10,8)).hex()
     if row['kind']=='evaluate':row['score_result']=native_owner.base.i32(uc,row['args'][4])
     if '_cell_output' in row:row['output_cell']=list(struct.unpack('<2h',uc.mem_read(row.pop('_cell_output'),4)))
     if '_coord_output' in row:row['output_coordinate']=native_owner.base.xyz(uc,row.pop('_coord_output'))
     if navigation_args is not None and row['kind'] in ('unit_destination','infantry_destination'):row['rng_at_return']={key:native_owner.base.sr.rng_state(uc,ptr) for key,ptr in self.resident.rngs.items()}
     if navigation_args is not None and row['kind']=='navigation_coordinate':row['output_coordinate']=native_owner.base.xyz(uc,uc.reg_read(UC_X86_REG_EAX))
     if (navigation_args is not None or home_args is not None) and row['kind'] in ('xyz_cell','cell_lookup'):row['resolved_cell']=self.cell_readback(uc.reg_read(UC_X86_REG_EAX))
     if idle_args is not None and row['kind']=='infantry_idle':row['rng_at_return']={key:native_owner.base.sr.rng_state(uc,ptr) for key,ptr in self.resident.rngs.items()}
   if a in names:
    row=dict(instruction=instruction[0],kind=names[a],pc=hex(a),caller=hex(m.read32(sp)),this=hex(uc.reg_read(UC_X86_REG_ECX)),
     args=[m.read32(sp+4+4*i) for i in range(counts.get(a,0))]);events.append(row);returns.setdefault(m.read32(sp),[]).append(row)
    if a in (0x65C640,0x65C660,0x65C780,0x65C7E0):row['stream']=next((k for k,p in self.resident.rngs.items() if p==uc.reg_read(UC_X86_REG_ECX)),hex(uc.reg_read(UC_X86_REG_ECX)))
    if idle_args is not None and a==0x4C9300:row['desired_u16']=struct.unpack('<H',uc.mem_read(row['args'][0],2))[0]
    if idle_args is not None and a==0x5F3E50:row['literal']=m.string(row['args'][0])
    if a==0x565730:row['xyz']=native_owner.base.xyz(uc,row['args'][0])
    if a==0x5657A0:row['cell']=list(struct.unpack('<2h',uc.mem_read(row['args'][0],4)))
    if navigation_args is not None and a in (0x4AFD40,0x75ACB0):row.update(interface=hex(row['args'][0]),coordinate=[native_owner.base.i32(uc,sp+8+i*4) for i in range(3)],convention='COM_stdcall')
    if navigation_args is not None and a in (0x741970,0x51AA40):row['rng_at_entry']={key:native_owner.base.sr.rng_state(uc,ptr) for key,ptr in self.resident.rngs.items()}
    if home_args is not None:
     row['dummy']=self.dummy_snap()
     if a in (0x500200,0x501AC0,0x50DEF0,0x56DC20,0x586E50):row['_cell_output']=row['args'][0]
     if a==0x501AC0:row.update(variant=row['args'][1],radius=native_owner.base.i32(uc,self.house+0x5498))
     if a==0x56D230:row['cell']=list(struct.unpack('<2h',uc.mem_read(row['args'][0],4)))
     if a==0x56DC20:row.update(seed_cell=list(struct.unpack('<2h',uc.mem_read(row['args'][1],4))),target_cell=list(struct.unpack('<2h',uc.mem_read(row['args'][12],4))))
     if a==0x49F420:row.update(current_coordinate=native_owner.base.xyz(uc,uc.reg_read(UC_X86_REG_EDX)),_coord_output=uc.reg_read(UC_X86_REG_ECX))
     if a in (0x578460,0x586E50):row['cell']=list(struct.unpack('<2h',uc.mem_read(row['args'][0 if a==0x578460 else 1],4)))
    if a==0x70CD10:row['anchor_xyz']=native_owner.base.xyz(uc,row['args'][1])
    if a==0x6F7CA0:row['anchor_xyz']=native_owner.base.xyz(uc,row['args'][6])
    if a in (0x743190,0x51E140,0x6F8DF0,0x4D9920):row['anchor_xyz']=native_owner.base.xyz(uc,m.read32(sp+8))
   if greatest_args is not None and a==0x6F9C86:events.append(dict(instruction=instruction[0],kind='global_candidate_visit',pc=hex(a),index=uc.reg_read(UC_X86_REG_EBX),candidate=hex(m.read32(uc.reg_read(UC_X86_REG_EDX)+uc.reg_read(UC_X86_REG_EBX)*4))))
   if greatest_args is not None and a in (0x6F9D87,0x6F9D8B):events.append(dict(instruction=instruction[0],kind='global_score_compare' if a==0x6F9D87 else 'global_best_update',pc=hex(a),candidate=hex(uc.reg_read(UC_X86_REG_EDI)),score=uc.reg_read(UC_X86_REG_EAX),previous_score=uc.reg_read(UC_X86_REG_ECX)))
   if a==0x6F8710:events.append(dict(instruction=instruction[0],kind='score_before_vhp',pc=hex(a),eax=uc.reg_read(UC_X86_REG_EAX)))
   if a in (0x65C84B,0x65C79D):events.append(dict(instruction=instruction[0],kind='rng_raw',pc=hex(a),value=uc.reg_read(UC_X86_REG_ESI)))
   if home_args is not None and a==0x65C7D0:events.append(dict(instruction=instruction[0],kind='direction_rng_raw',pc=hex(a),value=uc.reg_read(UC_X86_REG_EAX)))
   if home_args is not None and a==0x501B15:events.append(dict(instruction=instruction[0],kind='house_clamped_radius',pc=hex(a),radius=uc.reg_read(UC_X86_REG_EAX)))
   if a in (0x4D6E64,0x4DE0DB,0x4DE1AF,0x4D703A,0x4D712E):events.append(dict(instruction=instruction[0],kind='math',pc=hex(a),eax=uc.reg_read(UC_X86_REG_EAX)))
   if a==0x4D6E8F:events.append(dict(instruction=instruction[0],kind='chosen_leash',pc=hex(a),edi=uc.reg_read(UC_X86_REG_EDI)))
   if a in (0x70CFBE,0x70D023,0x70D0A0):events.append(dict(instruction=instruction[0],kind='score_anchor_branch',pc=hex(a),eax=uc.reg_read(UC_X86_REG_EAX)))
  fields={p+off:label for off,label in [(0xAC,'mission'),(0xB4,'queued'),(0xBC,'status'),(0x218,'archive'),(0x2B4,'target'),(0x5A4,'nav'),(0xC8,'timer_frame'),(0xD0,'timer_duration'),(0x688,'scan'),(0x168,'idle_start'),(0x170,'idle_delay'),(0x180,'targeting_start'),(0x188,'targeting_delay'),(0x6C4,'doing')]}
  if idle_args is not None:
   fields.update({p+offset:f'primary_facing_{offset-0x388:x}' for offset in range(0x388,0x3A0,4)})
   fields.update({p+offset:label for offset,label in ((0xF8,'sequence_frame_f8'),(0x100,'sequence_start'),(0x104,'sequence_aux'),(0x108,'sequence_duration'),(0x10C,'sequence_copy'))})
  if navigation_args is not None:
   fields.update({p+offset:label for offset,label in ((0x5A0,'aux'),(0x5E0,'path_head'),(0x598,'nav_queue_count'),(0x640,'movement_start'),(0x644,'movement_aux'),(0x648,'movement_delay'),(0x668,'blocked_start'),(0x66C,'blocked_aux'),(0x670,'blocked_delay'),(0x6B7,'blocked'),(0xF8,'sequence_frame_f8'),(0x100,'sequence_start'),(0x104,'sequence_aux'),(0x108,'sequence_duration'),(0x10C,'sequence_copy'))})
   interface=m.read32(p+0x674);destination=interface+(0x18 if family=='E1' else 0x30)
   fields.update({destination+i*4:f'locomotor_destination_{i}' for i in range(3)})
  dummy_writes=[]
  def record(uc,access,a,n,value,d):
   journal.append((a,bytes(uc.mem_read(a,n))))
   if a in fields:writes.append(dict(instruction=instruction[0],field=fields[a],pc=hex(uc.reg_read(UC_X86_REG_EIP)),value=value,bytes=n))
   if home_args is not None and a==0xABDC74:dummy_writes.append(dict(instruction=instruction[0],pc=hex(uc.reg_read(UC_X86_REG_EIP)),bytes=n,cell=list(struct.unpack('<2h',dwords(value)))))
  h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,record)
  try:
   entry=0x4DDF90 if mission==21 else (0x744100 if family=='MTNK' else 0x51F640)
   if dispatch_entry:entry=0x5B3060
   completion_setup=None
   if idle_args is not None and idle_args['entry']=='idle':entry=0x51CDB0
   if idle_args is not None and idle_args['entry']=='completion':
    entry=0x520AE0;setup_before=self.idle_snap(p);setup_rng_before=rng0
    admitted=m.invoke(0x51D6F0,p,(idle_args['doing'],0,0))&255;assert admitted
    table=m.read32(self.e1_type+0xE3C);sequence=table+idle_args['doing']*0x24;count=native_owner.base.i32(u,sequence+4)
    completion_setup=dict(entry='0x0051D6F0',action=idle_args['doing'],returned_al=admitted,before=setup_before,after=self.idle_snap(p),rng_before=setup_rng_before,rng_after={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()},sequence_table=hex(table),sequence=hex(sequence),sequence_bytes=bytes(u.mem_read(sequence,0x24)).hex(),native_frame_count=count)
    u.mem_write(p+0xF8,dwords(count));start=self.idle_snap(p);rng0=completion_setup['rng_after']
    events.append(dict(instruction=instruction[0],kind='supplied_completion_stage',frame_f8=count))
   if greatest_args is not None:
    entry=0x4D9920 if greatest_args['entry']=='foot' else m.read32(m.read32(p)+0x3C4)
    u.mem_write(self.coord,dwords(*greatest_args['point_xyz']))
   if candidate_result is not None:
    u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,p)
    run_checked(u,entry,0x4DE056,count=2000000)
    sp=u.reg_read(UC_X86_REG_ESP)
    prefix=dict(pc='0x004DE056',args=[m.read32(sp+4*i) for i in range(3)],anchor_xyz=native_owner.base.xyz(u,m.read32(sp+4)),
     virtual_receiver=hex(u.reg_read(UC_X86_REG_ECX)),vtable=hex(m.read32(p)),scanner=hex(m.read32(m.read32(p)+0x3C4)))
    events.append(dict(instruction=instruction[0],kind='supplied_scanner_return',**prefix,input_result=hex(candidate_result)))
    u.reg_write(UC_X86_REG_ESP,sp+12);u.reg_write(UC_X86_REG_EAX,candidate_result)
    end=run_checked(u,0x4DE05C,(RET_MAGIC,0x4DE12D),count=2000000);answer=u.reg_read(UC_X86_REG_EAX)
   elif stop is None:answer=m.invoke(entry,p,() if greatest_args is None else (greatest_args['mask'],self.coord,greatest_args['aux_bool']));end=RET_MAGIC
   else:
    u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,p)
    end=run_checked(u,entry,stop,count=2000000);answer=u.reg_read(UC_X86_REG_EAX)
  except Exception as e:
   print('FAILED',name,type(e).__name__,str(e),'last',[hex(a) for a in last],self.snap(p),flush=True);raise
  finally:u.hook_del(h);u.hook_del(w)
  result=dict(input=dict(name=name,family=family,entry=hex(entry),mission=mission,status=status,target=hex(target),archive=hex(archive),nav=hex(nav),flags=flags,scenario_seed=seed,scan_expired=scan_expired,idle_expired=idle_expired,
    supplied_scanner_result=None if candidate_result is None else hex(candidate_result),candidate_xyz=candidate_xyz,archive_xyz=archive_xyz,source_xyz=source_xyz,on_bridge=on_bridge,head_xyz=head_xyz,source_cell_flags=source_cell_flags,archive_flags=archive_flags,candidate_live=candidate_live,dispatch_entry=dispatch_entry),
   before=start,returned_eax=answer,after=self.snap(p),rng_before=rng0,rng_after={k:native_owner.base.sr.rng_state(u,p) for k,p in self.resident.rngs.items()},events=events,writes=writes,
   stop=hex(end),returned=end==RET_MAGIC,original_rescue_visited=0x4DDF90 in visited,original_area_guard_visited=0x4D6AA0 in visited,navigation_coordinate_calls=sum(e['kind']=='navigation_coordinate' for e in events),bridge_layer_calls=sum(e['kind']=='bridge_layer' for e in events),inherited_rng_events=[e for e in self.events if e['kind'] in ('rng','raw')],callback_events=[e for e in self.events if any(k in e['kind'] for k in ('boundary','OS_','COM_'))],instruction_count=instruction[0])
  if idle_args is not None:
   result['input']['idle_control']=dict(**idle_args,idle_action_frequency_bits=bytes(u.mem_read(self.rules+0x1710,8)).hex(),native_fpcw=u.reg_read(UC_X86_REG_FPCW))
   result['after']=self.idle_snap(p)
   if idle_args['entry']=='idle':result['returned_al']=answer&255
   if completion_setup is not None:result['completion_setup']=completion_setup
  if greatest_args is not None:
   result['input']['greatest_threat']=greatest_args
   result['greatest_threat_masks']={kind:[e['args'][0] for e in events if e['kind']==kind] for kind in ('unit_greatest','infantry_greatest','foot_greatest','greatest')}
  if navigation_args is not None:
   result['input']['navigation_control']=navigation_args;result['after']=self.navigation_snap(p,len(navigation_args['queue_entries']))
   result['destination_input_readback']=destination_input_readback
   result['inactive_candidate_readback']=[dict(pointer=hex(ptr),limbo=u.mem_read(ptr+0x81,1)[0],alive=u.mem_read(ptr+0x90,1)[0]) for ptr in navigation_args['inactive_registered_candidates']]
  if home_args is not None:
   result['input']['house_return_control']=home_args
   result['house_return']=dict(before_dummy=home_before,after_dummy=self.dummy_snap(),dummy_writes=dummy_writes,
    map_size_words=list(struct.unpack('<6i',u.mem_read(native_owner.base.sr.MAP+0xF4,24))),zone_storage=self.resident.zone_storage)
  if evidence_context is not None:
   result['input'].update(evidence_context)
   result.update(registration_before=registration_before,candidate_liveness_before=candidate_liveness_before)
  for address,previous in reversed(journal):u.mem_write(address,previous)
  assert hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==self.resident.code_hash
  assert all(bytes(u.mem_read(int(a,16),0x600)).hex()==raw for a,raw in self.vtables.items())
  return result

 def e1_weapon_state(self):
  """Observe actual normal/elite slots and execute shared native range getters."""
  m,u=self.m,self.u;slots=[]
  def named(p):return dict(pointer=hex(p),name=m.string(p+0x24))
  for tier,entry in (('normal',0x7177C0),('elite',0x7177E0)):
   for index in (0,1):
    slot=m.invoke(entry,self.e1_type,(index,));p=m.read32(slot);weapon=None
    if p:
     projectile=m.read32(p+0xA0);warhead=m.read32(p+0xAC)
     weapon=dict(**named(p),range_leptons=native_owner.base.i32(u,p+0xB4),minimum_range_leptons=native_owner.base.i32(u,p+0xB8),
      damage=native_owner.base.i32(u,p+0xA4),rof=native_owner.base.i32(u,p+0xB0),burst=native_owner.base.i32(u,p+0x9C),speed=native_owner.base.i32(u,p+0xA8),
      projectile=None if not projectile else dict(**named(projectile),**m.bullet_state(projectile),
       subject_to_cliffs=u.mem_read(projectile+0x296,1)[0],subject_to_elevation=u.mem_read(projectile+0x297,1)[0],subject_to_walls=u.mem_read(projectile+0x298,1)[0]),
      warhead=None if not warhead else dict(**named(warhead),verses_bits=[f'{v:016x}' for v in struct.unpack('<11Q',u.mem_read(warhead+0xA0,88))],
       prone_damage_bits=bytes(u.mem_read(warhead+0xF8,8)).hex(),cell_spread_bits=bytes(u.mem_read(warhead+0x124,4)).hex(),
       percent_at_max_bits=bytes(u.mem_read(warhead+0x12C,4)).hex(),wall=u.mem_read(warhead+0x144,1)[0],wood=u.mem_read(warhead+0x147,1)[0]))
    slots.append(dict(tier=tier,index=index,slot=hex(slot),type_get_weapon_entry=hex(entry),weapon=weapon))
  vtable=m.read32(self.e1);range_entry=m.read32(vtable+0x168);weapon_entry=m.read32(vtable+0x3F8)
  assert (range_entry,weapon_entry)==(0x7012C0,0x70E140)
  events=[];returns={};rng_before={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  def observe(uc,a,n,d):
   sp=uc.reg_read(UC_X86_REG_ESP)
   if a in returns:
    for event in returns.pop(a):event['returned_eax']=uc.reg_read(UC_X86_REG_EAX)
   if a in (0x7012C0,0x70E140,0x7177C0,0x7177E0,0x707E60):
    event=dict(pc=hex(a),caller=hex(m.read32(sp)),this=hex(uc.reg_read(UC_X86_REG_ECX)),argument=native_owner.base.i32(uc,sp+4));events.append(event);returns.setdefault(m.read32(sp),[]).append(event)
  hook=u.hook_add(UC_HOOK_CODE,observe)
  veterancy=bytes(u.mem_read(self.e1+0x150,4));controls=[]
  try:
   ranges=[m.invoke(range_entry,self.e1,(index,)) for index in (0,1)]
   threats=[m.invoke(0x707E60,self.e1,(mode,)) for mode in (0,1,2)]
   controls.append(dict(name='retained_rookie',supplied=False,veterancy_bits=veterancy.hex(),weapon_range_leptons=ranges,threat_range_leptons=threats))
   u.mem_write(self.e1+0x150,struct.pack('<f',2.0))
   controls.append(dict(name='supplied_elite',supplied=True,veterancy_bits=bytes(u.mem_read(self.e1+0x150,4)).hex(),
    weapon_range_leptons=[m.invoke(range_entry,self.e1,(index,)) for index in (0,1)],threat_range_leptons=[m.invoke(0x707E60,self.e1,(mode,)) for mode in (0,1,2)]))
  finally:u.mem_write(self.e1+0x150,veterancy);u.hook_del(hook)
  rng_after={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
  assert rng_before==rng_after
  return dict(slots=slots,veterancy_bits=bytes(u.mem_read(self.e1+0x150,4)).hex(),guard_range=native_owner.base.i32(u,self.e1_type+0x5B8),
   range_entry=hex(range_entry),get_weapon_entry=hex(weapon_entry),weapon_range_leptons=ranges,threat_range_leptons=threats,
   range_controls=controls,getter_events=events,rng_unchanged=True)

 def weapon_reader_receipts(self):
  # Run after every historical row. No old weapon, child or handler value is
  # retroactively changed by completing the omitted physical read context.
  m,u=self.m,self.u;self.phase='setup';before=self.e1_weapon_state()
  names={row['weapon']['name'] for row in before['slots'] if row['weapon']}
  # These are lexical reference names from the physical weapon sections.
  # Native readers decide references/scalars; no Rust export supplies a value.
  for _,path in native_owner.base.layers():
   if not path.exists():continue
   weapons,_=lexical(path.read_bytes(),names)
   for entries in weapons.values():
    for key in ('Projectile','Warhead'):
     if entries.get(key):names.add(entries[key])
  omitted=[]
  for row in before['slots']:
   weapon=row['weapon']
   if weapon and not any(read['weapon']['name']==weapon['name'] for read in self.e1_weapon_read_attempts):omitted.append(weapon['name'])
  for read in self.e1_weapon_read_attempts:
   for kind in ('projectile','warhead'):
    if not read[kind]['returned_al'] and read[kind]['name'] not in omitted:omitted.append(read[kind]['name'])
  # SSA belongs to an unread secondary and is physically present but was
  # never allocated/read by that weapon's constructor-only old slot.
  for _,path in native_owner.base.layers():
   if not path.exists():continue
   weapons,_=lexical(path.read_bytes(),set(omitted))
   for entries in weapons.values():
    for key in ('Projectile','Warhead'):
     if entries.get(key) and entries[key] not in omitted and not any(read[kind]['name']==entries[key] and read[kind]['returned_al'] for read in self.e1_weapon_read_attempts for kind in ('projectile','warhead')):omitted.append(entries[key])
  result=dict(omitted_sections=omitted,selected_sections=sorted(names),before=before,layers=[],historical_read_attempts=self.e1_weapon_read_attempts,
   reader_contexts=dict(rows='handler_rules_before_physical',initial_action_receipt='handler_rules_before_physical',
    retail_idle_rows='handler_rules_after_physical',greatest_threat_rows='handler_rules_after_physical',
    navigation_rows='handler_rules_after_physical',empty_rescue_rows='handler_rules_after_physical',retail_weapon_rows='handler_rules_after_physical'),
   historical_weapon_context='Only M60 receives WeaponType772080. InvisibleLow46BEE0 is attempted against an omitted section and retains native constructor fields. SA75D3A0 reads physical rules. Para/M60E/ParaE retain original771C70 constructor fields.',
   rules_context='handler_rules_after_physical',idle_action_frequency_bits=bytes(u.mem_read(self.rules+0x1710,8)).hex())
  active=[None];events=[];writes=[]
  def observe(uc,a,n,d):
   if a in (0x474620,0x5276D0,0x5283D0,0x528A10,0x5295F0):
    sp=uc.reg_read(UC_X86_REG_ESP)
    events.append(dict(pc=hex(a),caller=hex(m.read32(sp)),receiver=active[0],ini=hex(uc.reg_read(UC_X86_REG_ECX)),
     section=m.string(m.read32(sp+4)),key=m.string(m.read32(sp+8)),default_word=hex(m.read32(sp+12))))
  def record(uc,access,address,size,value,d):
   if active[0] is not None:
    p=int(active[0]['pointer'],16)
    if p<=address<p+active[0]['size']:writes.append(dict(pc=hex(uc.reg_read(UC_X86_REG_EIP)),receiver=active[0],offset=hex(address-p),bytes=size,value=value))
  h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,record)
  try:
   for name,path in native_owner.base.layers():
    if not path.exists():
     assert name=='LANGRULE.INI';result['layers'].append(dict(file=name,absent=True));continue
    raw=path.read_bytes();sections,lines=lexical(raw,names);m.rules_cache(sections);events.clear();writes.clear()
    rng_before={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()};reads=[]
    for p in dict.fromkeys(int(row['weapon']['pointer'],16) for row in before['slots'] if row['weapon']):
     active[0]=dict(kind='weapon',pointer=hex(p),name=m.string(p+0x24),size=0x160);wa=m.invoke(0x772080,p,(RULES,))&255
     projectile=m.read32(p+0xA0);warhead=m.read32(p+0xAC)
     active[0]=None if not projectile else dict(kind='projectile',pointer=hex(projectile),name=m.string(projectile+0x24),size=0x2F8)
     pa=m.invoke(0x46BEE0,projectile,(RULES,))&255 if projectile else None
     active[0]=None if not warhead else dict(kind='warhead',pointer=hex(warhead),name=m.string(warhead+0x24),size=0x1D0)
     ha=m.invoke(0x75D3A0,warhead,(RULES,))&255 if warhead else None
     active[0]=dict(kind='weapon_postpass',pointer=hex(p),name=m.string(p+0x24),size=0x160);m.invoke(0x7729F0,p)
     reads.append(dict(weapon=dict(entry='0x00772080',name=m.string(p+0x24),returned_al=wa),
      projectile=None if not projectile else dict(entry='0x0046BEE0',name=m.string(projectile+0x24),returned_al=pa),
      warhead=None if not warhead else dict(entry='0x0075D3A0',name=m.string(warhead+0x24),returned_al=ha),postpass='0x007729F0'))
    active[0]=None;rng_after={key:native_owner.base.sr.rng_state(u,ptr) for key,ptr in self.resident.rngs.items()}
    assert rng_before==rng_after
    result['layers'].append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),sections=sections,source_lines=lines,
     read_attempts=reads,events=list(events),writes=list(writes),after=self.e1_weapon_state(),rng_before=rng_before,rng_after=rng_after,rng_unchanged=True))
  finally:u.hook_del(h);u.hook_del(w)
  result['after']=self.e1_weapon_state()
  assert hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==self.resident.code_hash
  self.phase='logic';self.events.clear();self.pending.clear()
  return result

 def rules_reader_receipts(self):
  m,u=self.m,self.u;self.phase='setup';self.events.clear();self.pending.clear()
  fresh=m.alloc(0x2000)
  def state(pointer):
   return dict(guard_mode_stray=native_owner.base.i32(u,pointer+0x1724),idle_action_frequency_bits=bytes(u.mem_read(pointer+0x1710,8)).hex())
  def construct(pointer,guard_value):
   u.mem_write(pointer+0x1724,dwords(guard_value));before=state(pointer);native_fpcw=u.reg_read(UC_X86_REG_FPCW);writes=[]
   def record(uc,access,address,size,value,d):
    if pointer+0x1710<=address<pointer+0x1718 or address==pointer+0x1724:
     writes.append(dict(pc=hex(uc.reg_read(UC_X86_REG_EIP)),offset=hex(address-pointer),bytes=size,value=value))
   hook=u.hook_add(UC_HOOK_MEM_WRITE,record)
   try:returned=m.invoke(0x665650,pointer)
   finally:u.hook_del(hook)
   return dict(entry='0x00665650',receiver=hex(pointer),provided_guard_mode_stray=guard_value,returned_eax=hex(returned),before=before,after=state(pointer),writes=writes,native_fpcw=native_fpcw)
  result=dict(constructor=construct(fresh,0),constructor_retained_guard_control=construct(m.alloc(0x2000),0x12345678),controls=[],physical_layers=[])
  def read(name,pointer,sections,blocks,source=None):
   before=state(pointer);m.rules_cache(sections);events=[];visited=set();writes=[]
   def observe(uc,a,n,d):
    visited.add(a)
    if a in (0x474620,0x5283D0):
     sp=uc.reg_read(UC_X86_REG_ESP)
     event=dict(pc=hex(a),this=hex(uc.reg_read(UC_X86_REG_ECX)),section=m.string(m.read32(sp+4)),key=m.string(m.read32(sp+8)))
     event['default']=native_owner.base.i32(uc,sp+12) if a==0x474620 else bytes(uc.mem_read(sp+12,8)).hex()
     events.append(event)
   def record(uc,access,address,size,value,d):
    if pointer+0x1710<=address<pointer+0x1718 or address==pointer+0x1724:
     writes.append(dict(pc=hex(uc.reg_read(UC_X86_REG_EIP)),offset=hex(address-pointer),bytes=size,value=value))
   h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,record)
   try:
    for begin,end,required in blocks:
     for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,pointer),(UC_X86_REG_EDI,RULES)):u.reg_write(reg,value)
     run_checked(u,begin,end,required_addresses=[required])
   finally:u.hook_del(h);u.hook_del(w)
   row=dict(name=name,sections=sections,before=before,after=state(pointer),events=events,writes=writes,blocks=[[hex(a),hex(b)] for a,b,_ in blocks])
   if source is not None:row['source']=source
   return row
  guard=((0x670EBD,0x670EDD,0x474620),);idle=((0x66B3E4,0x66B40B,0x5283D0),)
  for name,sections in (('guard_missing',{}),('guard_wrong_case',{'General':{'guardmodestray':'2'}}),('guard_two',{'General':{'GuardModeStray':'2'}}),('guard_minus_one_sentinel',{'General':{'GuardModeStray':'-1'}}),('guard_negative_two',{'General':{'GuardModeStray':'-2'}}),('guard_later_missing_retains',{})):
   result['controls'].append(read(name,fresh,sections,guard))
  for name,sections in (('idle_missing',{}),('idle_wrong_case',{'AudioVisual':{'idleactionfrequency':'0.15'}}),('idle_retail_literal',{'AudioVisual':{'IdleActionFrequency':'0.15'}}),('idle_later_missing_retains',{})):
   result['controls'].append(read(name,fresh,sections,idle))
  result['handler_rules_before_physical']=state(self.rules)
  for name,path in native_owner.base.layers():
   if not path.exists():
    assert name=='LANGRULE.INI';result['physical_layers'].append(dict(file=name,absent=True));continue
   raw=path.read_bytes();sections,lines=lexical(raw,{'General','AudioVisual'})
   selected={section:{key:value for key,value in entries.items() if key==('GuardModeStray' if section=='General' else 'IdleActionFrequency')} for section,entries in sections.items()}
   row=read(name,self.rules,selected,guard+idle,dict(sha256=hashlib.sha256(raw).hexdigest(),lines=lines))
   result['physical_layers'].append(dict(file=name,**row))
  result['handler_rules_after_physical']=state(self.rules)
  self.phase='logic';self.events.clear();self.pending.clear()
  return result

 def registration(self):
  m,u=self.m,self.u
  arrays={}
  for name,header in (('techno',0xA8EC78),('unit',0x8B4108),('infantry',0xA83DE8),('logic',0x87F778),('ground_display',0x8A0390)):
   capacity=m.read32(header+8);count=m.read32(header+16);data=m.read32(header+4)
   assert count<=capacity<=1024
   arrays[name]=dict(header=hex(header),buffer=hex(data),capacity=capacity,count=count,actors=[hex(m.read32(data+i*4)) for i in range(count)])
  return arrays

 def initialize_greatest_candidates(self):
  m,u=self.m,self.u;self.pending.clear();self.events.clear();self.phase='setup'
  out=dict(before=self.registration(),rng_before={k:native_owner.base.sr.rng_state(u,p) for k,p in self.resident.rngs.items()},placements=[])
  for off in (0x5528,0x5578):
   u.mem_write(self.enemy+off+4,dwords(m.alloc(4096),1024));u.mem_write(self.enemy+off+0x10,dwords(0))
  donor=m.alloc(0x200);m.invoke(0x47BBF0,donor)
  owners=[m.read32(donor+off) for off in (0x54,0x58)];empty_cells=[]
  for xy,ptr in self.resident.ptrs.items():
   if m.read32(ptr+0xE4)==0 and m.read32(ptr+0xE8)==0:
    empty_cells.append(dict(cell=list(xy),before=[m.read32(ptr+off) for off in (0x54,0x58)],after=owners))
    for off,value in zip((0x54,0x58),owners):u.mem_write(ptr+off,dwords(value))
  out['native_empty_cell_defaults']=dict(constructor='0x0047BBF0',donor=hex(donor),owners=owners,cells=empty_cells)
  self.extra_candidates={}
  for label,typ,ctor,unlimbo,xy in (('E1',self.e1_type,0x517A50,0x51DFF0,(86,49)),('MTNK_tie',self.typ,0x7353C0,0x737BA0,(88,51))):
   p=m.alloc(0x1000);self.phase='setup';m.invoke(ctor,p,(typ,0));constructed=self.registration()
   u.mem_write(p+0x21C,dwords(self.enemy));u.mem_write(p+0x14C,dwords(self.enemy));self.phase='placement'
   attempts=[]
   for probe in (xy,(88,49),(87,51),(88,51),(86,51),(87,52),(88,52),(86,52),(87,49)):
    if probe not in self.resident.ptrs:continue
    m.invoke(0x486840,self.resident.ptrs[probe],(self.coord,));al=m.invoke(unlimbo,p,(self.coord,0x80))&255;attempts.append(dict(cell=list(probe),al=al))
    if al:xy=probe;break
   assert al,(label,attempts)
   if label=='E1':m.invoke(0x51D6F0,p,(0,0,0))
   self.extra_candidates[label]=p;self.prototype_baselines[p]=bytes(u.mem_read(p,0x1000))
   out['placements'].append(dict(label=label,actor=hex(p),constructor=hex(ctor),unlimbo=hex(unlimbo),cell=list(xy),attempts=attempts,al=al,xyz=native_owner.base.xyz(u,p+0x9C),display_layer=m.read32(p+0x94),logic_registered=u.mem_read(p+0x98,1)[0],flags=m.read32(p+0x14),limbo=u.mem_read(p+0x81,1)[0],after_constructor=constructed,after_unlimbo=self.registration()))
  self.cell_baselines={p:bytes(u.mem_read(p,0x200)) for p in self.resident.ptrs.values()}
  out.update(after=self.registration(),rng_after={k:native_owner.base.sr.rng_state(u,p) for k,p in self.resident.rngs.items()},events=list(self.events))
  self.phase='logic';self.events.clear();self.pending.clear()
  return out

def generate():
 q=FootMissions();q.initialize_companion();m,u=q.m,q.u
 rows=[]
 def add(name,family,**kw):
  row=q.row(f'{family}_{name}',family,**kw);rows.append(row)
 for family in ('MTNK','E1'):
  source=q.src if family=='MTNK' else q.e1
  xyz=native_owner.base.xyz(u,source+0x9C);ground=xyz[:];deck=[xyz[0],xyz[1],xyz[2]+416]
  for seed in (0,1,31):
   add(f'rescue_state2_seed{seed}',family,status=2,seed=seed)
   add(f'rescue_state1_seed{seed}',family,status=1,seed=seed)
  add('rescue_state1_nav_wait',family,status=1,nav=q.resident.ptrs[87,53])
  add('rescue_dispatch_state2',family,status=2,dispatch_entry=True)
  add('rescue_dispatch_state1',family,status=1,dispatch_entry=True)
  add('rescue_existing_target',family,status=0,target=q.resident.ptrs[87,51],moving=1,scan=1)
  add('area_guard_existing_target',family,mission=11,status=0,target=q.resident.ptrs[87,51],moving=1)
  add('area_guard_post',family,mission=11,status=0)
  add('area_guard_scan_empty',family,mission=11,status=0,scan_expired=True,idle_expired=True)
  add('area_guard_scan_live',family,mission=11,status=0,scan_expired=True,idle_expired=True,candidate_live=True)
  add('rescue_native_scan_live',family,status=0,archive=q.victim,stop=(RET_MAGIC,0x4DE12D),candidate_live=True,scan=1)
  add('rescue_native_scan_empty',family,status=0,archive=q.victim,stop=(RET_MAGIC,0x4DE12D),scan=1)
  add('rescue_default_ground_anchor',family,status=0,stop=(RET_MAGIC,0x4DE12D),candidate_live=True,scan=1)
  add('rescue_default_deck_anchor',family,status=0,stop=(RET_MAGIC,0x4DE12D),candidate_live=True,scan=1,
   source_xyz=deck,on_bridge=1,source_cell_flags=0x100,head_xyz=[23808,13184,832],nav=q.resident.ptrs[86,53])
  add('area_guard_default_deck_anchor',family,mission=11,status=0,scan_expired=True,idle_expired=True,
   source_xyz=deck,on_bridge=1,source_cell_flags=0x100,head_xyz=[23808,13184,832])
  add('rescue_entity_deck_anchor',family,status=0,archive=q.victim,archive_xyz=[23040,12928,832],
   stop=(RET_MAGIC,0x4DE12D),candidate_live=True,scan=1)
  add('rescue_zero_archive_point',family,status=0,archive=q.victim,archive_xyz=[0,0,0],
   stop=(RET_MAGIC,0x4DE12D),candidate_live=True,scan=1)
  add('rescue_nonzero_archive_point',family,status=0,archive=q.victim,archive_xyz=[0,0,1],
   stop=(RET_MAGIC,0x4DE12D),candidate_live=True,scan=1)
  edge=3840 if family=='MTNK' else 3072
  for delta in (edge-1,edge,edge+1):
   add(f'rescue_gate_{delta}',family,status=0,archive=q.victim,candidate_result=q.candidate,
    candidate_xyz=[23040+delta,12928,416],archive_xyz=[23040,12928,416],candidate_live=True,scan=1)
  for z in (416,832):
   add(f'rescue_gate_vertical_archive_z{z}',family,status=0,archive=q.victim,candidate_result=q.candidate,
    candidate_xyz=[23040+edge-20,12928,416],archive_xyz=[23040,12928,z],candidate_live=True,scan=1)
  add('rescue_gate_paid_head_irrelevant',family,status=0,archive=q.victim,candidate_result=q.candidate,
   candidate_xyz=[23040+edge-1,12928,416],archive_xyz=[23040,12928,416],candidate_live=True,scan=1,
   source_xyz=deck,on_bridge=1,source_cell_flags=0x100,head_xyz=[23808,13184,832],nav=q.resident.ptrs[86,53])
  leash=2816 if family=='MTNK' else 2252
  for delta in (leash-1,leash,leash+1,leash+2,leash+3):
   add(f'area_guard_cell_leash_{delta}',family,mission=11,status=0,archive=q.resident.ptrs[86,50],
    source_xyz=[22144-delta,12928,416])
  stray=native_owner.base.i32(u,q.rules+0x1724)
  for delta in (stray,stray+1,stray+2,stray+3):
   add(f'area_guard_foot_stray_{delta}',family,mission=11,status=0,archive=q.victim,
    archive_xyz=[ground[0]+delta,ground[1],ground[2]])
  add('area_guard_leash_firing_bypass',family,mission=11,status=0,archive=q.resident.ptrs[86,50],
   source_xyz=[22144-leash-3,12928,416],firing=1)
  add('area_guard_leash_nav_bypass',family,mission=11,status=0,archive=q.resident.ptrs[86,50],
   source_xyz=[22144-leash-3,12928,416],nav=q.resident.ptrs[87,53],moving=1)
  add('area_guard_foot_stray_clear_live_target',family,mission=11,status=0,archive=q.victim,archive_xyz=[ground[0]+stray+3,ground[1],ground[2]],target=q.candidate,candidate_live=True)
  add('area_guard_dispatch_post',family,mission=11,status=0,dispatch_entry=True)
 out=dict(schema_version=1,text_sha256=q.resident.code_hash,inputs=q.inputs,world=q.world,
  mission_inputs=q.mission_inputs,e1_ctor=q.e1_ctor,e1_placement=q.e1_placement,e1_unlimbo=q.e1_unlimbo,
  candidate_placement=q.candidate_placement,candidate_unlimbo=q.candidate_unlimbo,victim_placement=q.victim_placement,victim_unlimbo=q.victim_unlimbo,initializers=q.initializers,
  setup=dict(source=hex(q.src),e1=hex(q.e1),victim=hex(q.victim),candidate=hex(q.candidate),house=hex(q.house),enemy=hex(q.enemy),country=hex(q.country),
   original_vtables={address:hashlib.sha256(bytes.fromhex(raw)).hexdigest() for address,raw in q.vtables.items()},
   physical_getcoords=[hex(a) for a in sorted(q.entry_getcoords)],stray=native_owner.base.i32(u,q.rules+0x1724),
   null_coord=native_owner.base.xyz(u,0xB0EA90),e1_nonvehicle=u.mem_read(q.e1_type+0x695,1)[0],
   elevation_cap_key=m.string(0x83B344),leash_key=m.string(0x83BD90)),rows=rows)

 out['initial_action_receipt']=q.initial_action_receipt
 out['rules_reader_receipts']=q.rules_reader_receipts();idle_rows=[]
 for seed in (0,1,31):
  for name,expired in (('area_guard_post',False),('area_guard_scan_empty',True)):
   idle_rows.append(q.row(f'E1_retail_idle_{name}_seed{seed}',family='E1',mission=11,status=0,seed=seed,scan_expired=expired,idle_expired=expired,idle_args=dict(entry='area_guard',doing=0)))
  idle_rows.append(q.row(f'E1_retail_idle_direct_seed{seed}',family='E1',mission=11,status=0,seed=seed,idle_expired=True,idle_args=dict(entry='idle',doing=0)))
 for doing in (1,16,-1,4,9,27,28,29,30):
  idle_rows.append(q.row(f'E1_retail_idle_admission_doing{doing}',family='E1',mission=11,status=0,idle_expired=True,idle_args=dict(entry='idle',doing=doing)))
 for name,control,kw in (('foot_flag_3d5',dict(entry='idle',doing=0),dict(moving=1)),('walk_moving',dict(entry='idle',doing=0,loco_moving=1),{}),('prone',dict(entry='idle',doing=0,prone=1),{}),('firing',dict(entry='idle',doing=0),dict(firing=1)),('future_timer',dict(entry='idle',doing=0,timer_words=[1,0,99]),{}),('target_nav_pose',dict(entry='idle',doing=0,frame_f8=7),dict(target=q.candidate,nav=q.resident.ptrs[87,53]))):
  idle_rows.append(q.row(f'E1_retail_idle_admission_{name}',family='E1',mission=11,status=0,idle_expired=True,idle_args=control,**kw))
 for seed in range(2,12):
  idle_rows.append(q.row(f'E1_retail_idle_direct_seed{seed}',family='E1',mission=11,status=0,seed=seed,idle_expired=True,idle_args=dict(entry='idle',doing=0)))
 for doing in (9,10):
  idle_rows.append(q.row(f'E1_retail_idle_completion_doing{doing}',family='E1',mission=11,status=0,idle_args=dict(entry='completion',doing=doing,loco_moving=0,prone=0)))
 out['retail_idle_rows']=idle_rows
 registration=q.initialize_greatest_candidates();greatest_rows=[]
 def scan(name,family,mask,latch,kind='MTNK',live=True,entry='concrete',point=(22144,12928,416),tie=False):
  result=q.row(f'{family}_{name}',family,status=0,archive=q.victim,archive_xyz=list(point),candidate_live=live and kind=='MTNK',scan=latch,
   greatest_args=dict(entry=entry,mask=mask,point_xyz=list(point),aux_bool=0,candidate_kind=kind,extra_live={'E1':live and kind=='E1','MTNK_tie':tie}))
  greatest_rows.append(result)
 for family in ('MTNK','E1'):
  for mask in (0,1,2):
   for latch in (0,1):
    for live in (False,True):
     scan(f'foot_literal_mask{mask}_latch{latch}_live{int(live)}',family,mask,latch,live=live,entry='foot')
    for kind in ('MTNK','E1'):
     for live in (False,True):
      scan(f'concrete_mask{mask}_latch{latch}_{kind}_live{int(live)}',family,mask,latch,kind=kind,live=live)
  for point_name,point in (('null_coord',(0,0,0)),('near_null_coord',(0,0,1)),('archive_deck',(23040,12928,832))):
   for latch in (0,1):
    scan(f'concrete_{point_name}_latch{latch}',family,0,latch,point=point)
  scan('concrete_global_equal_score_candidates',family,0,0,point=(22656,13056,416),tie=True)
 out['greatest_threat_registration']=registration;out['greatest_threat_rows']=greatest_rows
 # Additive downstream receipts run only after every historical row is captured.
 # Supplied limbo controls leave actual constructor/registration history intact.
 for ptr in q.extra_candidates.values():u.mem_write(ptr+0x81,bytes([1]))
 queue=m.alloc(8)
 control=dict(path=[2,3,4,5]+[-1]*20,reference_cell=[85,49],queue_backing=queue,
  queue_entries=[q.victim,q.resident.ptrs[87,53]],aux=q.candidate,
  movement_timer_words=[50,0,5],blocked_timer_words=[40,0,6],retry=7,blocked=1,
  inactive_registered_candidates=[q.candidate,*q.extra_candidates.values()])
 navigation_rows=[]
 for family in ('MTNK','E1'):
  source=q.src if family=='MTNK' else q.e1;xyz=native_owner.base.xyz(u,source+0x9C)
  row=q.row(f'{family}_area_guard_foot_stray_destination',family,mission=11,status=0,archive=q.victim,
   archive_xyz=[xyz[0]+515,xyz[1],xyz[2]],navigation_args=control)
  assert row['returned'] and any(e['kind']=='foot_destination' for e in row['events'])
  assert any(e['kind']=='drive_move_to' if family=='MTNK' else e['kind']=='walk_move_to' for e in row['events'])
  navigation_rows.append(row)
 out['navigation_rows']=navigation_rows
 home_rows=[]
 for family in ('MTNK','E1'):
  row=q.row(f'{family}_rescue_empty_home',family,mission=21,status=0,archive=q.victim,candidate_live=False,scan=1,
   navigation_args=control,home_args=dict(primary=[87,50],alternate=[0,0],radius=1024))
  assert row['returned'] and row['after']['status']==1 and row['after']['archive']=='0x0'
  assert all(any(e['kind']==kind for e in row['events']) for kind in ('house_return','house_random_return','random_direction','zone','nearby_cell'))
  home_rows.append(row)
 out['empty_rescue_rows']=home_rows
 # Complete every physically referenced E1 weapon only after all208 old rows.
 # The old unloaded Para/elite and omitted InvisibleLow state stays declared.
 out['weapon_reader_receipts']=q.weapon_reader_receipts();weapon_rows=[]
 retail_context=dict(reader_context='weapon_reader_receipts.after',rules_context='handler_rules_after_physical',registration_context='greatest_threat_registration.after')
 for delta in (2253,2254,2815,2816,2817,2818,2819):
  weapon_rows.append(q.row(f'E1_retail_weapons_area_guard_cell_leash_{delta}',family='E1',mission=11,status=0,
   archive=q.resident.ptrs[86,50],source_xyz=[22144-delta,12928,416],idle_args=dict(entry='area_guard',doing=0),evidence_context=retail_context))
 for delta in (3839,3840,3841):
  weapon_rows.append(q.row(f'E1_retail_weapons_rescue_gate_{delta}',family='E1',status=0,archive=q.victim,candidate_result=q.candidate,
   candidate_xyz=[23040+delta,12928,416],archive_xyz=[23040,12928,416],candidate_live=True,scan=1,evidence_context=retail_context))
 for live in (False,True):
  weapon_rows.append(q.row(f'E1_retail_weapons_rescue_native_scan_live{int(live)}',family='E1',status=0,archive=q.victim,
   stop=(RET_MAGIC,0x4DE12D),candidate_live=live,scan=1,evidence_context=retail_context,
   navigation_args=dict(control,inactive_registered_candidates=list(q.extra_candidates.values()))))
 out['retail_weapon_rows']=weapon_rows
 preserved=json.dumps(out,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode()
 preserved_sha=hashlib.sha256(preserved).hexdigest()
 assert preserved_sha=='a7f520feedbcc1adbc479951356a0bd6049d3626bb6003952b48fc51b013c995','frozen220-row payload changed'
 firing_owner=FootMissions();receipt=firing_owner.ground_firing_receipt()
 receipt['preserved_payload']=dict(canonical_sha256=preserved_sha,canonical_bytes=len(preserved),row_counts={key:len(out[key]) for key in ('rows','retail_idle_rows','greatest_threat_rows','navigation_rows','empty_rescue_rows','retail_weapon_rows')})
 out['ground_firing_receipt']=receipt
 preserved=json.dumps(out,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode()
 preserved_sha=hashlib.sha256(preserved).hexdigest()
 assert preserved_sha=='0fc230b21725dd9c13a84c548c76423f173b481057239d5e2b7899a1ad9f0dba','frozen payload including ground_firing_receipt changed'
 stage=firing_owner.stage_clock_receipt()
 stage['preserved_payload']=dict(canonical_sha256=preserved_sha,canonical_bytes=len(preserved),row_counts={key:len(out[key]) for key in ('rows','retail_idle_rows','greatest_threat_rows','navigation_rows','empty_rescue_rows','retail_weapon_rows')},ground_firing_cases=len(receipt['cases']))
 out['stage_clock_receipt']=stage
 preserved=json.dumps(out,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode()
 preserved_sha=hashlib.sha256(preserved).hexdigest()
 assert preserved_sha=='35c0e43f412d76cf9cdd1dd65a4cb13801e7e979d672314de39f15574e4e6fb7','frozen payload including stage_clock_receipt changed'
 emission_owner=FootMissions();emission=emission_owner.ground_emission_receipt()
 emission['preserved_payload']=dict(canonical_sha256=preserved_sha,canonical_bytes=len(preserved),row_counts={key:len(out[key]) for key in ('rows','retail_idle_rows','greatest_threat_rows','navigation_rows','empty_rescue_rows','retail_weapon_rows')},ground_firing_cases=len(receipt['cases']),stage_clock_rows=len(stage['clock_rows']))
 out['ground_emission_receipt']=emission
 out['draw_stage_modulo_receipt']=emission_owner.draw_stage_modulo_receipt()
 preserved=json.dumps(out,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode()
 preserved_sha=hashlib.sha256(preserved).hexdigest()
 assert preserved_sha=='94db9dcd2cb08ecbc23867f5e11f09f5cb1b2d1e3db7f29eebf491dbb59a8a09','frozen payload including old80 draw modulo controls changed'
 frame_owner=FootMissions();frames=frame_owner.infantry_frame_selection_receipt()
 frames['preserved_payload']=dict(canonical_sha256=preserved_sha,canonical_bytes=len(preserved),old_draw_modulo_rows=len(out['draw_stage_modulo_receipt']['rows']))
 out['infantry_frame_selection_receipt']=frames
 return out


NATIVE_SPANS = {
 'rescue':(0x4DDF90,0x23E),'area_guard':(0x4D6AA0,0x6C4),
 'unit_area_guard':(0x744100,0x75),'infantry_area_guard':(0x51F640,0x16),
 'mission_dispatch':(0x5B3060,0x486),'mission_control_reader':(0x5B3760,0x213),
 'elevation_reader':(0x66D150,0x98),'physical_object_coordinate':(0x5F65A0,0x25),
 'physical_cell_coordinate':(0x486840,0x47),'object_distance':(0x5F6360,0xD3),
 'fast_sqrt':(0x4CAC40,0x6E),'fast_sqrt_table':(0x8650BC,0x10000),
 'rescue_multiplier':(0x7E48F0,8),'area_guard_multiplier':(0x7E9258,8),
 'mission_rate_multiplier':(0x7E27F8,8),'null_coordinate_initializer':(0x6F2A50,0x20),
 'rules_constructor':(0x665650,0x23D7),'guard_mode_stray_key_block':(0x670EBD,0x20),
 'idle_action_frequency_key_block':(0x66B3E4,0x27),'ini_range_reader':(0x474620,0x3A),
 'ini_double_reader':(0x5283D0,0x1D2),'idle_action_frequency_literal':(0x83A338,20),
 'foot_greatest_threat':(0x4D9920,0x3F),'unit_greatest_threat':(0x743190,0xD4),
 'infantry_greatest_threat':(0x51E140,0x26C),'techno_greatest_threat':(0x6F8DF0,0xFBF),
 'techno_registry_append':(0x6F3183,0x52),'techno_global_fold':(0x6F9C67,0x148),
 'cell_constructor':(0x47BBF0,0x1B8),'logic_register':(0x55BAA0,0x36),
 'infantry_idle':(0x51CDB0,0x2F8),'infantry_idle_jump_table':(0x51D0A8,40),
 'infantry_idle_admission':(0x5216D0,0x86),'walk_is_moving':(0x75AB30,0xA),
 'infantry_action':(0x51D6F0,0x3F5),'primary_facing':(0x4C9300,0xCA),
 'game_options_delay':(0x5FB2E0,0x26),'infantry_sequencer':(0x520AE0,0x41C),
 'idle_max_multiplier':(0x7E44B8,8),'idle_min_multiplier':(0x7E5D78,8),
 'idle_random_unit_scale':(0x7E3570,8),
 'infantry_constructor':(0x517A50,0x269),'infantry_unlimbo':(0x51DFF0,0x144),
 'walk_constructor':(0x75AA90,0x67),'foot_destination':(0x4D94B0,0x264),
 'unit_destination':(0x741970,0x1817),'infantry_destination':(0x51AA40,0x7A1),
 'drive_move_to':(0x4AFD40,0xB5),'walk_move_to':(0x75ACB0,0xE1),
 'drive_is_moving':(0x4AFB80,0x96),'nav_queue_clear':(0x4E0190,0x2A),
 'house_ordinary_return':(0x500200,0xF9),'house_random_return':(0x501AC0,0x628),
 'house_base_center':(0x50DEF0,0x3C),'random_direction':(0x49F420,0x121),
 'clamp_map':(0x586E50,0x169),'nearby_cell':(0x56DC20,0xBA0),
 'zone':(0x56D230,0x16A),'raw_random':(0x65C780,0x51),'ranged_random':(0x65C7E0,0xAD),
 'weapon_constructor':(0x771C70,0x281),'weapon_find_or_allocate':(0x772FA0,0x83),
 'weapon_reader':(0x772080,0x965),'weapon_speed_postpass':(0x7729F0,0x5D),
 'projectile_constructor':(0x46BBC0,0x21E),'projectile_find_or_allocate':(0x46C790,0x83),'projectile_reader':(0x46BEE0,0x556),
 'warhead_constructor':(0x75CEC0,0x2F0),'warhead_reader':(0x75D3A0,0xB1E),
 'weapon_range':(0x7012C0,0xDB),'get_weapon':(0x70E140,0x56),
 'normal_weapon_slot':(0x7177C0,0x15),'elite_weapon_slot':(0x7177E0,0x15),
 'elite_predicate':(0x750010,0x18),'elite_threshold':(0x7E37B4,4),
 'infantry_ai':(0x51BAB0,0x4D7),'foot_ai_entry':(0x4DA530,0x20),'techno_ai':(0x6F9E50,0x11B5),
 'absolute_stage_tick':(0x6FABC4,0x6D),'infantry_fire_at_target':(0x5206B0,0x42F),
 'building_stage_tick':(0x4509DE,0x5A),'stage_timer_remaining':(0x426630,0x1C),
 'infantry_select_weapon':(0x5218E0,0x7E),'infantry_fire_error':(0x51C8B0,0x2E5),'techno_fire_error':(0x6FC0B0,0xC88),
 'walk_fire_error':(0x55AD00,5),'infantry_fire_prefix':(0x51DF60,0x1C),'base_fire_unexecuted_entry':(0x6FDD50,0x10),
 'infantry_movement_actions_entry':(0x520F40,0x20),'fire_up_action_flags_rate':(0x7EAF8C,4),
 'infantry_pointer_expired':(0x51AA10,0x3A),'foot_pointer_expired':(0x4D9960,0x295),'techno_pointer_expired':(0x7077C0,0x4EE),
 'infantry_fire_wrapper':(0x51DF60,0x83),'techno_fire':(0x6FDD50,0x1C1F),
 'infantry_fire_flh':(0x523250,0x9F),'fire_rof':(0x6FCFA0,0x266),
 'bullet_create':(0x46B050,0x64),'bullet_constructor':(0x466380,0x13F),
 'bullet_fire':(0x468670,0x516),'bullet_ai':(0x4666E0,0x1916),
 'detonate_wrapper_entry':(0x468D80,0x30),'detonate_entry':(0x4690B0,0x30),
 'area_damage_entry':(0x489280,0x30),'unit_damage':(0x737C90,0x9EC),
 'foot_damage_entry':(0x4D7330,0x30),'techno_damage_entry':(0x701900,0x30),'object_damage_entry':(0x5F5390,0x30),
 'impact_select_entry':(0x48A4F0,0x30),'anim_constructor_entry':(0x421EA0,0x30),
 'anim_ai':(0x423AC0,0x108A),'anim_destructor':(0x4228E0,0x23D),
 'deferred_drain':(0x725C70,0x11F),'logic_remove':(0x55BAE0,0x50),
 'logic_emitted_suffix':(0x55B608,0x13),'frame_commit':(0x55DE73,0x14),
 'infantry_draw_signed_modulo':(0x518E08,0x19),
 'infantry_frame_selection':(0x518D80,0x209),'primary_facing_current':(0x4C93D0,0x99),
 'infantry_observer_gate':(0x70EE30,0xC3),'infantry_selected_type':(0x522640,0x7B),
 'infantry_primary_facing_lookup':(0x7EAEFC,0x80),
}


def metadata():
 result=provenance(scope=__doc__,entry_points={
  'rules_constructor':0x665650,'guard_mode_stray_key_block':0x670EBD,
  'idle_action_frequency_key_block':0x66B3E4,'ini_range_reader':0x474620,'ini_double_reader':0x5283D0,
  'cell_constructor':0x47BBF0,'logic_register':0x55BAA0,'display_register':0x4A9720,
  'infantry_idle_admission':0x5216D0,'walk_is_moving':0x75AB30,
  'infantry_action':0x51D6F0,'primary_facing':0x4C9300,'game_options_delay':0x5FB2E0,'infantry_sequencer':0x520AE0,
  'mission_control_static_initializer':0x4E7CF0,'mission_control_reader':0x5B3760,
  'rules_mission_loop':0x679C92,'rules_elevation':0x66D150,'rules_general':0x66D530,
  'unit_constructor':0x7353C0,'infantry_type_constructor':0x5236A0,
  'infantry_type_reader':0x5240A0,'infantry_constructor':0x517A50,
  'drive_factory':0x6C4010,'walk_factory':0x6C4790,
  'unit_unlimbo':0x737BA0,'infantry_unlimbo':0x51DFF0,'infantry_ready':0x51D6F0,
  'rescue':0x4DDF90,'area_guard':0x4D6AA0,'mission_dispatch':0x5B3060,
  'unit_area_guard':0x744100,'infantry_area_guard':0x51F640,
  'archive_setter':0x70C610,'queue_mission':0x5B35E0,'commence':0x5B3570,
  'mission_control':0x5B3A00,'threat_range':0x707E60,
  'rescue_scan_call':0x4DE056,'rescue_refusal_stop':0x4DE12D,
  'unit_greatest_threat':0x743190,'infantry_greatest_threat':0x51E140,
  'foot_greatest_threat':0x4D9920,'greatest_threat':0x6F8DF0,
  'evaluate_target':0x6F7CA0,'threat_score':0x70CD10,
  'passive_scan':0x709820,'unit_approach':0x7414E0,'infantry_approach':0x522340,
  'foot_approach':0x4D5690,'object_distance':0x5F6360,'fast_sqrt':0x4CAC40,
  'unit_destination':0x741970,'infantry_destination':0x51AA40,'foot_destination':0x4D94B0,
  'assign_target':0x6FCDB0,'infantry_assign_target':0x51B1F0,
  'can_acquire':0x7091D0,'targeting_timer':0x70F7E0,'infantry_idle':0x51CDB0,
  'rng_seed':0x65C6D0,'rng_ranged':0x65C7E0,
  'walk_constructor':0x75AA90,'drive_is_moving':0x4AFB80,
  'drive_move_to':0x4AFD40,'walk_move_to':0x75ACB0,'nav_queue_clear':0x4E0190,
  'house_return':0x500200,'house_random_return':0x501AC0,'house_base_center':0x50DEF0,
  'random_direction':0x49F420,'clamp_map':0x586E50,'zone':0x56D230,'nearby_cell':0x56DC20,
  'weapon_constructor':0x771C70,'weapon_find_or_allocate':0x772FA0,'weapon_reader':0x772080,
  'projectile_constructor':0x46BBC0,'projectile_find_or_allocate':0x46C790,'projectile_reader':0x46BEE0,'warhead_constructor':0x75CEC0,'warhead_reader':0x75D3A0,
  'weapon_speed_postpass':0x7729F0,'weapon_range':0x7012C0,'get_weapon':0x70E140,
  'normal_weapon_slot':0x7177C0,'elite_weapon_slot':0x7177E0,'elite_predicate':0x750010,
  'infantry_ai':0x51BAB0,'foot_ai':0x4DA530,'techno_ai':0x6F9E50,'absolute_stage_tick':0x6FABC4,
  'building_stage_tick':0x4509DE,'stage_timer_remaining':0x426630,
  'fire_at_target':0x5206B0,'infantry_select_weapon':0x5218E0,'infantry_fire_error':0x51C8B0,'techno_fire_error':0x6FC0B0,'walk_fire_error':0x55AD00,
  'infantry_fire_prefix':0x51DF60,'base_fire_unexecuted_entry_boundary':0x6FDD50,'infantry_movement_actions':0x520F40,
  'infantry_pointer_expired':0x51AA10,'foot_pointer_expired':0x4D9960,'techno_pointer_expired':0x7077C0,
  'techno_fire_complete':0x6FDD50,'bullet_create':0x46B050,'bullet_factory':0x6C5090,'bullet_constructor':0x466380,
  'bullet_fire':0x468670,'bullet_ai':0x4666E0,'fire_flh':0x523250,'fire_rof':0x6FCFA0,
  'detonate_wrapper':0x468D80,'detonate':0x4690B0,'area_damage':0x489280,'unit_damage':0x737C90,
  'foot_damage':0x4D7330,'techno_damage':0x701900,'object_damage':0x5F5390,
  'select_impact_anim':0x48A4F0,'anim_constructor':0x421EA0,'anim_ai':0x423AC0,'anim_destructor':0x4228E0,
  'anim_attach':0x424B50,'object_uninit':0x5F65F0,'expiry_broadcast':0x7258D0,'deferred_drain':0x725C70,
  'logic_emitted_suffix':0x55B608,'logic_remove':0x55BAE0,'frame_commit':0x55DE73,
  'anim_art_reader':0x427D00,'sound_registry_reader':0x7510D0,'sound_name_lookup':0x7514D0,
  'firestorm_trace':0x5880A0,'cliff_wall_trace':0x4CC100,'signed_draw_modulo':0x518E08,
  'infantry_frame_selection':0x518D80,'primary_facing_current':0x4C93D0,
  'infantry_observer_gate':0x70EE30,'infantry_selected_type':0x522640,'draw_default_cell_lookup':0x5657A0,

 },assumptions=[
  'infantry_frame_selection_receipt appends only after the entire prior17,535,517-byte canonical payload retains SHA94db9dcd2cb08ecbc23867f5e11f09f5cb1b2d1e3db7f29eebf491dbb59a8a09, including all old80 draw_stage_modulo_receipt controls. A fresh existing FootMissions VM executes original constructors/readers/Unlimbo and whole518D80 through actual518F88 RET. The receipt records its exact521-byte original body. Real70EE30 observer gate and body FacingCurrent4C93D0 run; native count clamp/CDQ/IDIV, positive-only stride branch, table lookup, DWORD IMUL and wrapping ADD outputs are observed without Python expected arithmetic. All42 actual physical GI records,336 physical action/facing rows,40 full32-index/threshold facing rows and136 supplied signed count/Stage/Start/Stride controls are separate. Input facing runs original4C9300 and retains its actual duration0 state; in-flight rotation is not covered. All three full RNG states, original code/vtables, CPU/x87 and supplied/native memory writes restore unchanged.',
  'The frame selector also runs Doing-1 through actual physical-coordinate vtable+1B8 and Map5657A0 on its concrete current Cell. Four rows retain physical XYZ/Cell identity and supply Cell+EC ground1/water2 and OnBridge+8C0/1. Original518D80 chooses the actual draw action; these are raw draw-input controls, not native water transition, bridge placement or occupancy evidence. Actual E1 Type+D94 is0 and original observer gate returns1 throughout; JumpJet CLSID/target-facing, alternate observer/disguise bank selection, invalid Doing outside-1/0..41, SHP lookup/drawing and renderer are excluded. No inherited runtime callback/result substitution is reached in these calls.',
  'ground_emission_receipt is additive only after the entire prior10,895,933-byte payload including stage_clock_receipt retains canonical SHA35c0e43f412d76cf9cdd1dd65a4cb13801e7e979d672314de39f15574e4e6fb7. Four independent fresh existing FootMissions VMs supplement the historical sparse asset inputs with physical GI744-frame SHP, all eight6-frame MGUN directional SHPs and12-frame PIFFPIFF, extracted with the recorded existing asset build. Existing Reader427D00/file-buffer boundary and Sound source-order cache/7510D0/7514D0 bind their actual retail ART/SOUND fields and GIAttack Report. Every physical E1 normal/elite weapon and referenced projectile/warhead reader runs in the existing owner; the separate historical payload is untouched.',
  'Four ground_emission_receipt cases execute original whole51BAB0 through actual51DF60/6FDD50, native Infantry523250 FLH, actual Bullet factory/construct/fire, original5880A0 Firestorm and4CC100 cliff/wall queries, native6FCFA0 ROF/rearm and real MGUN-S constructor/attach. Full unchanged original BulletAI4666E0 follows through4690B0/489280 and the real Unit737C90/Foot4D7330/Techno701900/Object5F5390 damage chain; actual PIFFPIFF SelectAnim/constructor and damage retaliation execute. Actual whole InfantryAI, BulletAI, AnimAI and deferred native cleanup return. Call/register/stack data, class damage-pointer mutation, object clocks/poses/retirement, original list append/compact/remove and all three complete RNG states are observed.',
  'The emitted-object schedule executes the original dynamic Logic suffix55B608..55B61B, beginning with supplied ESI equal to the actual four-actor prefix count, followed by original55DE73..55DE87 frame commit and full725C70 drain. Original loop index/count/list mutations determine order. On the first impact pass the Bullet removes itself, compacts MGUN-S to index4, appends PIFFPIFF at5, and the native increment visits PIFFPIFF5; MGUN-S first runs in the following supplied pass. No host supplies a Bullet/Anim event order or replaces their gameplay bodies/results. Per-source full InfantryAI calls run at the declared frames; full-match actor/global scheduling is not claimed.',
  'draw_stage_modulo_receipt separately executes original518E08..518E21 for80 count/Stage controls, including the actually read physical GI FireUp count6. It supplies caller EAX/EBX/EBP/ESP and a scratch signed-count slot, runs the native count<=1 clamp/CDQ/IDIV, and records actual signed EAX quotient, ECX divisor and EDX remainder. Negative/count-zero/count-one, small/large counts, signed Stage extremes, negative stages and values aboveu16 are explicit; every full RNG state and original code/vtable remains unchanged. No expected quotient or remainder is calculated in Python.',

  'stage_clock_receipt appends only after the entire frozen payload, including ground_firing_receipt, retains canonical SHA256 0fc230b21725dd9c13a84c548c76423f173b481057239d5e2b7899a1ad9f0dba. It reuses that fresh original-constructor VM after all controls restore native writes/CPU/observations/RNG. Original Techno scalar region6FABC4 stops before6FAC31 under explicit ESI receiver, EBP0, ESP and raw caller auxiliary; original Building scalar region4509DE stops before common boundary450A38, executing actual426630. Building storage is a supplied zero-initialized0x200-byte scalar receiver, not original Building construction. No class/vtable callback is reached in that region. Eighteen supplied controls per family retain actual native writes between repeated absolute frames, elapsed gaps, zero/negative rates, paused zero/nonzero/negative durations, zero/negative increment, signed addition wrap, delta wrap and absolute frame wrap. Each frame records before/after signed stage/timer/rate/increment, changed byte, original executed instruction bytes, native field writes/registers, caller stack-local output and full three RNG states.',
  'stage_clock_receipt action_restart_rows executes physical GI original51D6F0(4,0,0) to produce FireUp4, then supplies stage31/changedFC1/timer99,0x2468ACE0,9/rate9/increment7 and absolute frame1337 before forced original51D6F0(0,1,0) or(4,1,0). The Ready transition exercises admitted clock writes, while the same-FireUp request observes original51D90B/51D913 unchanged-action refusal even when forced. Actual return, writes, full RNG and before/after stage/Doing/sequence bytes establish these outcomes; no action/rate/stage/RNG result is replaced. All writes, CPU/x87 and inherited observer state are restored. Original full text and class vtables remain unchanged.',
  'ground_firing_receipt creates a fresh existing FootMissions VM only after the frozen220-row payload canonical identity is checked. It repeats original constructors, actual Drive/Walk factories, successful E1/MTNK Unlimbo/registration, the existing rules key-block receipts and all physically referenced E1 normal/elite WeaponType/Projectile/Warhead readers. The explicit companion Ready fixture baseline is retained; target admission51B1F0, Attack queue5B35E0 and native whole51BAB0 promotion/AI run. Original Foot4DA530, Techno6F9E50, Mission5B3060, absolute stage6FABC4..6FAC31, select5218E0, fire error51C8B0/6FC0B0/Walk55AD00, DoAction51D6F0, FireAtTarget5206B0, sequencer520AE0 and movement520F40 execute without gameplay-result substitutions. Clock inputs, call/write order, actual receiver/arguments/returns, stage/F8/FC, firing68D, rearm/mission timers, real Walk state, facing and full three RNG streams are recorded.',
  'Four accepted ground_firing_receipt controls run complete selected InfantryAI returns before the launch visit. The accepted visit stops before executing the real6FDD50 entry, after original51DF60 pushed actual target/weapon0 and wrote68D0 at51DF70. No EAX or stack return is supplied at that boundary; the enclosing InfantryAI/FireAt/sequencer tails have not returned. Repeated absolute-frame and elapsed-gap controls execute the original absolute stage arithmetic, including actual mission cadence/RNG when due. Concrete51AA10 expiry control1 executes before the first AI visit or between the first two, including the real Techno/Foot and Infantry AssignTarget/DoAction receivers. This is a supplied expired-pointer notification, not original target death/UnInit/broadcast.',
  'The additive weapon_reader_receipts runs only after all208 historical rows. Original normal7177C0/elite7177E0 slot getters expose M60/Para/M60E/ParaE names and actual retained constructor/read fields. Original7012C0 GetWeaponRange and707E60 ThreatRange run through original70E140 and native vtables for the retained rookie state and separately supplied veterancy float2.0 elite control. The getter control restores that supplied field and consumes no RNG. Before closure, only M60 has read physical WeaponType772080; its actual InvisibleLow46BEE0 attempt lacks a matching section and retains original46BBC0 constructor fields, while SA75D3A0 reads. The other three weapons retain original771C70 fields and null child references. Explicit omitted_sections/read attempts qualify the historical208 results; no old result is rewritten.',
  'New weapon closure reads exact-case physical E1-referenced normal/elite weapon sections and their Projectile/Warhead names across RULESMD, optional LANGRULE, mode and map layers. Every original772080, actual child46BEE0/75D3A0 and original7729F0 postpass executes, with native AL section status, reader arguments, observed fields/writes and full unchanged RNG recorded. No native range/default/scanner/child value comes from Rust. The separate12 retail_weapon_rows execute7 consecutive Cell-leash controls including the prior2254 contrast,3 strict Rescue supplied-result boundaries and2 actual scanner prefixes under the completed physical weapon context and physically read .15 idle frequency. Each new input names its weapon/rules/registration receipt context; actual ordered registries and every registered actor limbo/alive/coordinate/House are read before entry. The two real prefix controls reuse the existing navigation fixture to supply only the two extra candidates limbo1/alive0 after prototype restoration. Other new rows restore their actual placed extra baselines; the main candidate uses candidate_live. These continue original handlers, not a whole Rules Process or whole match.',
  'The additive initial_action_receipt captures original E1517A50/Walk75AA90 constructor state and successful51DFF0 Unlimbo before the companion explicit Ready. Both retain Doing-1. Full original520AE0 then executes actual COM Walk75AB30 and forced51D6F0(Ready0,1,0), recording the51D9D2 Doing write, raw frame/sequence/facing fields and full three RNG states. All probe writes and CPU/x87 are restored before explicit fixture Ready, preserving all204 historical values. The actor is the initialized stationary source at the declared crop/frame; first whole InfantryAI, moving admission, absolute-stage clock and whole ScenarioLoad are excluded.',
  'The additive two navigation_rows run original concrete AreaGuard through its Foot-anchor stray branch, original Unit741970/Infantry51AA40 destination setters, Foot4D94B0 and concrete Drive4AFD40/Walk75ACB0. Native+4C target-coordinate output, actual Map-returned Cell scalars, class setter entry/return RNG, destination/actual IsMoving/head, NavCom/Aux, all24 path words/reference, NavQueue and timers/sequence are recorded. Their nonempty path/queue/timer/Aux prestate is explicit supplied prior state, not producer evidence; native ctor locomotor head/destination are retained. Actual ILocomotion+10 getter is separately executed for before/after readback with its writes/CPU restored.',
  'The additive two empty_rescue_rows execute full original4DDF90 past4DE12D with actual empty scanner results, status1 store, House500200/501AC0/50DEF0,49F420, Map zone56D230 and FNPC56DC20, concrete Cell destination, archive clear and current-mission cadence. House retained physical fields+5490/+5494/+5498 are supplied primary87,50/alternate0,0/radius1024 under the existing house_base_projection.query_return field contract. Full three RNG states, ordered ranged/direction raw draws, original returned cells/coordinates, actual map queries and final retained Dummy state are captured; no House, FNPC, range, target or RNG result is replaced.',
  'Existing anytown_damage.mission.Mission is the sole physical source/crop/constructor bootstrap. Its original class and default native payload are unchanged. The extension adds original E1 type constructor5236A0/full layered reader5240A0, M60 and SA readers plus an explicitly unsuccessful InvisibleLow read attempt, and original Infantry517A50/Walk factory6C4790. Original Unit7353C0/Drive construction runs for the source, victim and enemy candidate; actual native Unlimbo, Mark, Display ground layer2 and Logic registration run for all four actors. Full physical E1 weapon closure is the separate additive receipt and rows.',
  'RULESMD.INI, absent LANGRULE.INI, MPBattleMD.ini and XMP03T4.MAP layers are explicit. MissionControl original static constructors/read loop, original66D150 ElevationModel and selected668BF0 threat coefficients execute per layer. Existing full66D530 supplies General.GuardModeStray. The preserved82 handler rows retain constructor IdleActionFrequency; they do not execute its AudioVisual key block. Fixed ART GI/GISequence is loaded with existing source-order cache; native E1 Ready51D6F0 executes before its baseline.',
  'Rows supply prior mission21 or11, status, queue-1, target/archive/NavCom, dispatch0/0, optional expired targeting/idle timer and explicit retained coordinate/layer/latch/head controls after original constructors/placement. Every row restores actor/locomotor/prototype/cell baseline; all native writes are journaled and rolled back after capture. Scenario65C6D0 reseeds with declared0/1/31 while Main/MapGen retained states remain explicit.',
  'Full original Rescue4DDF90, concrete Unit/Infantry AreaGuard wrappers and common4D6AA0 execute. Separate rows enter original dispatcher5B3060, including native timer epilogue. Actual RNG ranged/raw calls, stream identity, full three-stream states, physical+48 coordinates, mission control bits, original distance/fast-sqrt/ftol, target/destination/archive calls and selected field writes retain instruction order.',
  'Full-scanner rows execute original concrete+3C4 wrappers, Foot4D9920, Techno6F8DF0, evaluate6F7CA0 and score70CD10 on one native placed hostile MTNK. The unmodified real vtables and full text hashes are checked after each row. NullPoint0/0/0 comes from original6F2A50; zero and0/0/1 archive controls exercise actual native score branch and whole-cell versus leptonic distance.',
  'The additive rules_reader_receipts executes original665650 on explicit zero-initialized storage, records actual selected writes, and records the constructor retaining both supplied zero and0x12345678 at GuardModeStray+1724 without any overlapping native write. Its zero is a supplied storage control, not a native allocation/default claim. Original670EBD..670EDD/474620 and66B3E4..66B40B/5283D0 execute unchanged with supplied ESI Rules receiver and EDI existing CCINI cache; missing/exact-case/sentinel/negative/later-missing controls and the physical layers are explicit. The whole AudioVisual reader6691E0 is not run.',
  'The additive36 retail_idle_rows execute original E1 idle51CDB0, admission5216D0/7099E0 and actual ILocomotion+10 Walk75AB30, with selected Doing, actual interface+30 moving, prone, firing, timer, target/NavCom/animation-frame and seed controls. Original51D6F0 action,5FB2E0 delay and4C9300 body-facing execute. Two idle9/10 completion rows execute original action setup, supply stage at the real GI sequence count, then run full520AE0 through default520D1B and forced Ready; sequence table bytes and setup RNG are explicit. They do not validate the producer/absolute-stage clock. Idle timer/sequence/frame/facing bytes, full three RNG streams, instruction-ordered calls/writes and idle return RNG before AreaGuard cadence are recorded. Native FPCW is observed0x0E7F (the existing VM PC53/chop setting); native read .15 widens its parsedfloat bits before the original stored-f64 idle arithmetic.',
  'The additive86 greatest_threat_rows distinguish literal Foot4D9920 entry masks0/1/2 from original concrete+3C4 Unit743190/Infantry51E140 weapon-mask augmentation. Foot latch+688 rewrites only AL before original Techno6F8DF0 and clears only on an actual NULL return. Live MTNK/E1 candidates use original constructors/Unlimbo; no target or score return is supplied. Original array visits, evaluate/score outputs and strict better-score updates execute. Equal-score controls use a supplied midpoint and observe the earlier actual Techno registry candidate retained.',
  'greatest_threat_registration observes actual Techno/Unit/Infantry arrays after constructors, and Logic/ground Display arrays after original Unlimbo. Techno constructor appends; Logic appends for the supplied sort0 contract; Display layer2 uses its own ordering. Additional enemy E1 requires native empty Cell owner fields: original47BBF0 executes on a donor and only its+54/+58 values are supplied to empty cropped cells after all82 preserved rows. Full Cell/map construction is not claimed. Houses/counter vectors and declaring placed candidates limbo remain supplied boundaries.',
  'Cell anchors execute486840/47B3A0 on physical level/slope and remain ground anchors when the source pose is deck+416/OnBridge1. Entity anchors execute native5F65A0 physical XYZ. Drive/Walk retained-head offsets reuse locomotor_at_coord.FAMILIES; alternate head and unrelated NavCom are explicit controls. Full scan input, Rescue strict gate and AreaGuard leash use the archived physical point.',
 ],substitutions=[
  'ground_emission_receipt excludes the four preexisting actor AI visits and other Logic/global phases, whole ScenarioLoad/House construction, input/multiplayer cadence, movement/pathfinding completion and rendering. Source/target constructors/Unlimbo execute on the inherited physical crop and supplied Houses/count vectors. The existing companion Ready baseline remains explicit. The supplied high-deck control adds416 to actual ground XYZ and sets OnBridge/CellHasBridge after ground Unlimbo, retaining original ground and empty upper occupancy. Original impact therefore applies no target damage in that control; it establishes the declared layer-consumer continuation, not genuine bridge placement or deck occupancy/damage production. Bridge collapse/destruction and target death are not triggered.',
  'Physical SHP and INI input bytes are real, but the inherited file-buffer/source-order-cache/heap/OS boundaries exclude original archive/INI loading and full render/palette binding. GIAttack SoundList/type/sample-name reading and Weapon Report binding are original; sample indexes are fixture-relative. Inherited7509E0 records the actual sound request and returns before playback, audio-device work and Main audio RNG, so unchanged Main is bounded by that explicit request boundary. All three retained streams are recorded; this is not a complete audio RNG claim. Existing radar, visual, hierarchy and runtime sinks remain declared by the sole Mission owner. The imported anim_bouncer_launch.constructor_state is only a readback decoder; its separate supplied-constructor harness is not invoked.',
  'draw_stage_modulo_receipt stops before518E21 and does not execute the remaining draw frame/facing/index composition, SHP access or renderer. Caller registers, stack marker, chosen sequence offset and count table are supplied. The original native block produces all arithmetic results; this isolated receipt does not certify full drawing, current Doing selection or frame/pixel parity.',

  'stage_clock_receipt proves selected original scalar clock regions and a full original DoAction restart, not a complete Building/Techno AI call, first actor initialization, whole Scenario/Logic scheduling, persistence or an absolute ground animation renderer. Stage/timer/rate/increment/current-frame and caller register/stack premises are supplied inputs. Building450A33 is not a common stop: the admitted branch jumps past it to450A38, which is the declared boundary before the next virtual call. Raw copied timer auxiliary and caller stack-local bytes are observations, not authoritative timer fields. Full RNG states are read before/after; no RNG, timer, arithmetic, gameplay return or native instruction/vtable is substituted.',
  'ground_firing_receipt excludes6FDD50 body, launch/projectile allocation, MGUN/PIFF/Report asset or sound closure, post-fire rearm writes, impact/damage/collapse and whole Logic/Scenario/object interleaving. It is original selected per-actor AI or a pre-launch prefix, not a complete shot. Supplied high-deck controls add416 to actual placed XYZ and set OnBridge/physical Cell HasBridge0x100; real Map queries and original firing receivers run, but original ground/upper occupancy lists are retained. Native bridge Unlimbo, deck occupancy relocation and layer/head production are not established. Opposite-layer controls run original51C8B0→6FC0B0 admission only and execute no AI visit: full Attack pursuit reaches Foot4D5690 and unprepared AStar42D170/42A5B0, whose initialization/hierarchy belongs to the existing anytown_damage/navigation.py owner. No path answer, mission timer, target/fire/sequence/RNG result, code or vtable is substituted to bypass that prerequisite.',
  'Historical208 rows have an explicitly partial physical weapon read context: Para, M60E, ParaE, InvisibleLow and SSA sections are unread even though E1 TypeINI allocates all four slot weapon types. Their executed results and RNG remain unchanged. Old82 rows and initial_action_receipt use constructor IdleActionFrequency, while the later36 idle/86 threat/2 destination/2 home rows use the separately read physical frequency. The new weapon receipt publishes this array-to-reader-context map. New12 handler controls use all physically read E1 weapons and children but retain the inherited supplied crop, House, registration, archive/geometry and receiver-state bounds; strict Rescue result-seam rows do not certify actual acquisition.',
  'The historical220-row portion is a selected handler/dispatcher continuation on the existing Anytown crop, not a native whole-match ScenarioLoad/RespondToBaseAttack/damage trigger or full live UnitAI/FootAI object tick. Queue promotion and source state that select those handlers are supplied. The separate ground_firing_receipt executes original per-actor InfantryAI returns/pre-launch prefixes under its explicit bounds. Full House construction/bookkeeping remain excluded; null-House object constructors are followed by explicit owner links and valid family-specific count vectors. Physical Americans country readers and original4F643B coefficient selection execute.',
  'Supplied-scanner-result rows execute the original Rescue prefix to4DE056, record actual virtual receiver/vtable/three arguments, then supply only the declared EAX result and consumed12-byte stack before running original caller4DE05C. They do not execute the scanner or certify candidate acquisition; full-scanner rows are separate. The preserved no-target/refused rows stop at4DE12D before status1 write, House500200 home selection, destination/archive cleanup or the cadence tail. The additive empty_rescue_rows execute that original suffix with the separate supplied House/crop premises.',
  'Geometry/layer/head/NullPoint and raw leash-boundary poses are supplied post-placement controls; they do not prove native bridge Unlimbo, locomotor head production, occupancy relocation or map loader chronology. Placed candidates can be declared limbo after actual placement to exercise empty scans; no physical UnInit transition is claimed. Crop and out-of-crop map/zone storage retain the existing owner bounds.',
  'Existing successful native heap/file/INI cache, empty SEH, wall-clock/atexit/ASCII/CLSID/Interlocked/OleRun OS boundaries remain. Additional Infantry COM activation is routed to original Walk factory after checking its physical CLSID/arguments. Visual asset/radar/sound sinks and inherited hierarchy586990 callback are recorded; full audio Main RNG/device work, rendering and future movement/occupancy effects are excluded. No range, RNG, approach, target, bridge or scanner result is replaced except the explicitly declared strict-gate scanner-result seam.',
  'New destination/home rows run after all204 historical rows are captured. The existing candidate and additional registered E1/MTNK tie candidates are explicitly supplied inactive with limbo+81=1 and alive+90=0 after row restores their original placement baselines. An earlier diagnostic set limbo before row; prototype restoration reset that candidate to limbo0/alive1 before native evaluation, explaining its selection. Native6F7D90/6F7D96/6F7D98 rejects a candidate whose limbo byte is nonzero, as the preserved greatest_threat_rows miss controls also demonstrate. Both bytes remain declared and recorded premises for the selected four downstream rows, and their original registration history remains real. House construction, base/radius producers, prior path/NavQueue/timer producers, full navigation initialization, movement Process and future occupancy/bridge relocation remain excluded; the inherited crop zone planes and resident runtime sinks apply to the full Rescue suffix.',
  'Raw EAX is recorded at callable returns; it is not a semantic return value for void/x87-returning methods. Native score_before_vhp and evaluate.score_result are integer score observations. The copied middle timer word is retained as raw stack data, not an authoritative timer field. These native vectors provide no Rust or production parity claim by themselves.',
 ])
 binary=image_bytes()
 result['native_spans']={}
 for name,(address,length) in NATIVE_SPANS.items():
  offset,raw=file_span(binary,address,length)
  row=dict(address=f'0x{address:08X}',file_offset=offset,length=length,sha256=hashlib.sha256(raw).hexdigest())
  if length<=0x700:row['hex']=raw.hex()
  result['native_spans'][name]=row
 return result


# The Mission producer's transitive module inventory, which its historical
# receipt recorded; the run's source check covers it.
MISSION_SOURCES=(
  'tools/native_oracle.py', 'tools/projectile_oracle/bridge_render_inputs.py',
  'tools/projectile_oracle/flat_art.py', 'tools/projectile_oracle/guided_step.py',
  'tools/projectile_oracle/ifv_bridge_impact.py', 'tools/projectile_oracle/ifv_fire_coord.py',
  'tools/projectile_oracle/ifv_impact.py', 'tools/projectile_oracle/ifv_launch.py',
  'tools/projectile_oracle/ifv_select_anim.py', 'tools/projectile_oracle/ordinary_collision.py',
  'tools/rmg_oracle/gen_rng_vectors.py', 'tools/rmg_oracle/harness.py',
  'tools/rules_oracle/bridge_anim_inputs.py', 'tools/rules_oracle/bridge_anim_lists.py',
  'tools/rules_oracle/bridge_child_sound.py', 'tools/rules_oracle/theater_general_reader.py',
  'tools/spatial_oracle/__init__.py', 'tools/spatial_oracle/anytown_damage/__init__.py',
  'tools/spatial_oracle/anytown_damage/anytown_geometry.py',
  'tools/spatial_oracle/anytown_damage/anytown_resident.py',
  'tools/spatial_oracle/anytown_damage/inputs.py',
  'tools/spatial_oracle/anytown_damage/mission.py',
  'tools/spatial_oracle/anytown_damage/mission_counter.py',
  'tools/spatial_oracle/anytown_damage/mission_interleaving.py',
  'tools/spatial_oracle/anytown_damage/mission_publication.py',
  'tools/spatial_oracle/anytown_damage/mission_test_vectors.py',
  'tools/spatial_oracle/anytown_damage/mtnk_attack.py',
  'tools/spatial_oracle/anytown_damage/next_family_native.py',
  'tools/spatial_oracle/anytown_damage/publication.py', 'tools/spatial_oracle/bridge_rim.py',
  'tools/spatial_oracle/building_body_rules.py', 'tools/spatial_oracle/map_queries.py',
  'tools/spatial_oracle/mapgen_range.py', 'tools/spatial_oracle/shrapnel_repair/__init__.py',
  'tools/spatial_oracle/shrapnel_repair/map_facts.py',
  'tools/spatial_oracle/shrapnel_repair/packet_io.py',
  'tools/spatial_oracle/shrapnel_repair/production_inputs.py',
  'tools/spatial_oracle/shrapnel_repair/retail_inputs.py',
  'tools/spatial_oracle/shrapnel_repair/shrapnel_repair.py')


def source_paths():
 paths={name:REPO/name for name in MISSION_SOURCES}
 for relative in ('tools/spatial_oracle/anytown_damage/foot_missions.py',
                  'tools/spatial_oracle/bridge_target_composed.py',
                  'tools/spatial_oracle/locomotor_at_coord.py',
                  'tools/spatial_oracle/house_base_projection.py',
                  'tools/spatial_oracle/foot_attack_move.py',
                  'tools/native_slope.py',
                  'tools/spatial_oracle/fire_error.py',
                  'tools/spatial_oracle/anim_bouncer_launch.py'):
  paths[relative]=REPO/relative
 return paths


def publish(argv=None):
 finish_vectors(generate,HERE/'foot_missions.json',provenance=metadata,
                source_paths=source_paths(),argv=argv)


def generate_capture():
 """Original E1 Capture4D4B20 through Mission5B3060, not a second emulator.

 Capture's NULL Target/nonNULL NavCom/human path never dereferences NavCom.
 The supplied Building identity therefore bounds cadence only, not a native
 click producer, Building constructor/placement or successful walking/entry.
 """
 q=FootMissions();q.initialize_companion();m,u=q.m,q.u
 layers=[];slot=0xA8E3A8+8*32
 # Add this physical reader pass only in the new mode. Historical Foot rows
 # keep their original constructor/reader contexts and payloads unchanged.
 for name,path in native_owner.base.layers():
  if not path.exists():
   assert name=='LANGRULE.INI';layers.append(dict(file=name,absent=True));continue
  raw=path.read_bytes();selected,_=lexical(raw,{'Capture'})
  before=bytes(u.mem_read(slot+0x10,8)).hex();m.rules_cache(selected)
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,RULES)
  run_checked(u,0x679C92,0x679CAF,required_addresses=[0x5B3760])
  layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),
   capture=selected.get('Capture',{}),rate_before_bits=before,
   rate_after_bits=bytes(u.mem_read(slot+0x10,8)).hex()))
 building=m.alloc(0x900);u.mem_write(building,dwords(0x7E3EBC))
 frame=m.read32(0xA8ED84);assert frame==1
 actor=dict(type_name=m.string(q.e1_type+0x24),
  occupier=bool(u.mem_read(q.e1_type+0xEB4,1)[0]),
  house_is_human=bool(m.invoke(0x50B730,m.read32(q.e1+0x21C))))
 assert actor==dict(type_name='E1',occupier=True,house_is_human=True)
 assert m.read32(m.read32(q.e1)+0x214)==0x4D4B20
 controls=[]
 for seed in (0,1,31):
  row=q.row(f'E1_capture_cadence_seed{seed}',family='E1',mission=8,status=0,
   target=0,nav=building,seed=seed,dispatch_entry=True)
  assert row['returned'] and row['input']['entry']=='0x5b3060'
  for state in (row['before'],row['after']):
   assert (state['mission'],state['queued'],state['target'],state['nav'])==(8,-1,'0x0',hex(building))
  requests=[e for e in row['events'] if e['kind']=='rng']
  assert len(requests)==1
  request=requests[0]
  assert (request['pc'],request['caller'],request['stream'],request['args'])==('0x65c7e0','0x4d4c9c','scenario',[0,2])
  mission_control=[e for e in row['events'] if e['kind']=='mission_control']
  assert len(mission_control)==1 and mission_control[0]['caller']=='0x4d4c78'
  raw_draws=[e['value'] for e in row['events'] if e['kind']=='rng_raw']
  assert raw_draws
  main_unchanged=row['rng_before']['main']==row['rng_after']['main']
  mapgen_unchanged=row['rng_before']['mapgen']==row['rng_after']['mapgen']
  assert main_unchanged and mapgen_unchanged
  timer_writes=[dict(pc=e['pc'],field=e['field'],value=e['value'])
   for e in row['writes'] if e['field'] in ('timer_frame','timer_duration')]
  assert [e['pc'] for e in timer_writes]==['0x5b331b','0x5b3324']
  # Shared original-state decoder/encoder, not another RNG packing policy.
  controls.append(dict(name=row['input']['name'],scenario_seed=seed,frame=frame,
   mission=8,queued=-1,target=0,nav_nonnull=True,
   rate_bits=mission_control[0]['rate_bits'],timer_before=row['before']['dispatch'],
   timer_after=row['after']['dispatch'],returned_delay=row['returned_eax'],
   ranged_requests=[dict(caller=request['caller'],stream=request['stream'],
    args=request['args'],result=request['result_eax'])],raw_draws=raw_draws,
   rng_before=dict(native_state_hex=native_owner.base.sr.state_bytes(row['rng_before']['scenario']).hex()),
   rng_after=dict(native_state_hex=native_owner.base.sr.state_bytes(row['rng_after']['scenario']).hex()),
   timer_writes=timer_writes,main_unchanged=main_unchanged,mapgen_unchanged=mapgen_unchanged))
 text_unchanged=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==q.resident.code_hash
 vtables_unchanged=all(bytes(u.mem_read(int(address,16),len(bytes.fromhex(raw))))==bytes.fromhex(raw)
  for address,raw in q.vtables.items())
 assert text_unchanged and vtables_unchanged
 return dict(schema=1,layers=layers,actor=actor,
  controls=controls,native_text_unchanged=text_unchanged,native_vtables_unchanged=vtables_unchanged)


def capture_metadata():
 return provenance(scope='Three original E1 Capture dispatcher cadence controls, using the existing FootMissions owner.',
  entry_points=dict(mission_control_init=0x4E7CF0,mission_control_reader=0x5B3760,
   layered_mission_read=0x679C92,infantry_type_ctor=0x5236A0,infantry_type_reader=0x5240A0,
   infantry_ctor=0x517A50,infantry_unlimbo=0x51DFF0,walk_factory=0x6C4790,
   mission_dispatch=0x5B3060,capture=0x4D4B20,mission_control=0x5B3A00,
   capture_ranged_call=0x4D4C97,rng_seed=0x65C6D0,rng_ranged=0x65C7E0,
   ranged_raw_store=0x65C84B,dispatch_timer_start=0x5B331B,dispatch_timer_duration=0x5B3324),
  assumptions=[
   'Replay from the checkout: python -m tools.spatial_oracle.anytown_damage.mission --foot-capture --check (or --write to accept newly executed native outputs). Select the original supported gamemd.exe through VERA20K_GAMEMD_EXE or RA2_DIR. Neither that binary nor repository sources are hashed.',
   'Use the separate physical sparse caches VERA20K_SHRAPNEL_INPUTS and VERA20K_ANYTOWN_INPUTS declared by tools/spatial_oracle/anytown_damage/retail_manifest.json. The former supplies original RULESMD/ARTMD/SOUNDMD/MPBattleMD reader bootstrap; the latter supplies XMP03T4.MAP and the existing physical crop/asset dependencies. Keep LANGRULE absent for these stock controls. layers records actual physical file hashes, selected Capture bytes and native Rate results.',
   'The existing FootMissions VM executes original MissionControl, E1 type, Infantry/Foot/Techno and Walk construction, physical selected readers and successful original E1 Unlimbo/registration, retaining its explicit Ready fixture baseline. An additive Capture reader pass runs the existing original679C92..679CAF MissionControl loop through5B3760 in physical RULESMD, optional LANGRULE, MPBattleMD and XMP03T4 order; older Foot corpora are not generated or rewritten.',
   'Controls supply current mission8, queued-1, status0, NULL Target and nonNULL Building NavCom at retained absolute frame1 after initialization. The Building is a supplied allocated identity with original7E3EBC vtable, not native construction or placement. On this NULL Target/nonNULL NavCom/controlled-human E1 path, original Capture only tests NavCom against NULL and never dereferences its Building identity. Original5B3060 dispatches the unchanged Infantry vtable+214 to4D4B20, including current Rate multiplication/ftol, actual Scenario65C7E0 rejection retries and real timer writes.',
   'Scenario seeds0/1/31 are explicit per-row inputs. rng_before/after.native_state_hex encode all logical native Random2Class fields through existing shrapnel_repair.state_bytes; unmodeled padding is zero in that shared encoding. Ordered raw_draws come from actual65C84B stores, not host RNG arithmetic. Full native Main and MapGen logical states are compared unchanged; original text and concrete vtables are asserted unchanged. No gameplay return or native instruction/vtable is substituted in the selected cadence path.',
  ],substitutions=[
   'Inherited physical source-order INI/file-buffer caches, successful allocation, empty Windows SEH and OS/COM bootstrap boundaries remain those of the sole Mission/FootMissions owner. House storage/count vectors and ownership links are supplied with actual physical country readers; full House/Scenario/map-load chronology is excluded.',
   'No whole input/ClickedAction/Event production, native Building construction/placement, walking/pathfinding/arrival, garrison admission/AddGarrison/Tag, rendering, audio playback/Main audio RNG, whole-match scheduling, multiplayer or persistence equivalence is claimed. This corpus establishes selected Capture cadence and RNG/timer behavior only.',
  ])


def publish_capture(argv=None):
 finish_vectors(generate_capture,HERE/'foot_capture.json',provenance=capture_metadata,
                source_paths=source_paths(),argv=argv)
