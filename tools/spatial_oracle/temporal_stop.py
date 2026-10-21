"""Bounded original Infantry target-clear -> retained Attack dispatch -> Temporal LetGo.

Supplied already-warping ordinary Infantry/Unit runtime state, actual CLEG
InfantryType constructor/physical ART sequence reader, actual Teleport constructor,
MissionControl constructor/physical readers, original setters and dispatch bodies.
No native gameplay result or RNG return is substituted. Full Event admission,
Infantry/target construction, firing and full per-frame AI remain outside scope.

From the repository root, set VERA20K_GAMEMD_EXE (or RA2_DIR),
VERA20K_SHRAPNEL_INPUTS to physical RULESMD/ARTMD/MPBattleMD extracts, and
VERA20K_ANYTOWN_INPUTS to the physical XMP03T4.MAP directory. Run with
PYTHONPATH=. python tools/spatial_oracle/temporal_stop.py --check (the default);
--write explicitly refreshes the native result and provenance sidecar.

Rust consumer: src/sim/temporal_tests.rs. The supplied retained timer proves
before/deadline admission, not a fixed Stop-to-release delay or full Scenario
cadence. No post-dispatch class Commence or target TemporalAI is invoked.
"""
from pathlib import Path
import hashlib, json, os, struct, sys
from collections import deque
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC,run_checked,finish_vectors,provenance
from tools.projectile_oracle.bridge_render_inputs import BulletReader,lexical
from tools.spatial_oracle.building_body_rules import RULES,SP,dwords
HERE=Path(__file__).resolve().parent
ROOT=Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
MAP=Path(os.environ['VERA20K_ANYTOWN_INPUTS'])/'XMP03T4.MAP'

def sha(raw):return hashlib.sha256(raw).hexdigest()
def i32(u,p):return struct.unpack('<i',u.mem_read(p,4))[0]

class Probe(BulletReader):
 def __init__(self):
  self.phase='setup';self.receipts=[];self.calls=[];self.writes=[];self.trail=deque(maxlen=40)
  raw=(ROOT/'ARTMD.INI').read_bytes();art,_=lexical(raw,{'CLEG'});seq=art['CLEG']['Sequence'];art,_=lexical(raw,{'CLEG',seq})
  super().__init__(art,ROOT)
  u=self.u
  self.code_before=sha(bytes(u.mem_read(0x401000,0x3E0000)))
  self.typ=self.alloc(0xF00);self.invoke(0x5236A0,self.typ,(self.cstring('CLEG'),));self.invoke(0x523D00,self.typ)
  self.rules=self.alloc(0x2000);u.mem_write(0x8871E0,dwords(self.rules));self.invoke(0x665650,self.rules)
  self.invoke(0x4E7CF0,0)
  self.inputs={'art_sha256':sha(raw),'sequence':seq,'sequence_bytes':bytes(u.mem_read(self.read32(self.typ+0xE3C),42*36)).hex(),'layers':[]}
  for name,path in [('RULESMD.INI',ROOT/'RULESMD.INI'),('LANGRULE.INI',ROOT/'LANGRULE.INI'),('MPBattleMD.ini',ROOT/'MPBattleMD.ini'),('XMP03T4.MAP',MAP)]:
   if not path.exists():assert name=='LANGRULE.INI';self.inputs['layers'].append(dict(file=name,absent=True));continue
   raw=path.read_bytes();sections,lines=lexical(raw,{'Attack','Guard','Sleep','Move','CLEG'});self.rules_cache(sections)
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,RULES);run_checked(u,0x679C92,0x679CAF,count=1000000)
   self.inputs['layers'].append(dict(file=name,sha256=sha(raw),selected_input_sha256=sha(json.dumps(sections,sort_keys=True,separators=(',',':')).encode()),attack_lexical=sections.get('Attack'),attack_record=bytes(u.mem_read(0xA8E3C8,32)).hex()))
  self.actor=self.alloc(0x1000);self.victim=self.alloc(0x1000);self.link=self.alloc(0x50);self.house=self.alloc(0x6000);self.loco=self.alloc(0x80)
  p=self.actor;t=self.victim;l=self.link
  u.mem_write(p,dwords(0x7EB058));u.mem_write(p+0x6C0,dwords(self.typ));u.mem_write(p+0x6C,dwords(125));u.mem_write(p+0x90,b'\x01');u.mem_write(p+0x94,dwords(-1));u.mem_write(p+0x21C,dwords(self.house));u.mem_write(self.house+0x1EC,b'\x01');u.mem_write(self.house+0x1ED,b'\x01');u.mem_write(0xA8B238,dwords(5))
  u.mem_write(p+0xAC,dwords(1,-1,-1));u.mem_write(p+0xC8,dwords(100,0,16));u.mem_write(p+0x2B4,dwords(t));u.mem_write(p+0x274,dwords(l));u.mem_write(p+0x520,dwords(-1));u.mem_write(p+0x5C4,dwords(-1));u.mem_write(p+0x6C4,dwords(1));u.mem_write(p+0x9C,dwords(2688,2688,0))
  u.mem_write(t,dwords(0x7F5C70));u.mem_write(t+0x6C,dwords(400));u.mem_write(t+0x90,b'\x01');u.mem_write(t+0x278,dwords(l));u.mem_write(t+0x270,b'\x01')
  u.mem_write(l,dwords(0x7F5180));u.mem_write(l+0x24,dwords(p,t));u.mem_write(l+0x48,dwords(3500))
  self.invoke(0x718000,self.loco);u.mem_write(self.loco+0xC,dwords(p));u.mem_write(p+0x674,dwords(self.loco+4))
  self.rngs={'scenario':self.read32(0xA8B230)+0x218,'main':0x886B88,'mapgen':0xABE890}
  for address in self.rngs.values():self.invoke(0x65C6D0,address,(3,))
  u.reg_write(UC_X86_REG_FPCW,0x0E3F);u.mem_write(0x822D80,struct.pack('<H',0x0E3F))
  self.saved=[(a,bytes(u.mem_read(a,b-a+1))) for a,b,_ in u.mem_regions()]
  self.cpu=u.context_save();self.cursor_saved=self.cursor
  u.hook_add(UC_HOOK_MEM_WRITE,self.write_hook)
 def hook(self,u,pc,n,d):
  if pc>=0x7E1000 and pc!=RET_MAGIC:raise RuntimeError(("non-original-code",hex(pc),[hex(a) for a in self.trail],self.calls[-10:]))
  self.trail.append(pc)
  if pc in (0x55A965,0x55A987):
   # Same imported Interlocked transport as anytown_damage.mission.Mission.observe.
   sp=u.reg_read(UC_X86_REG_ESP);ptr=self.read32(sp);value=(self.read32(ptr)+(1 if pc==0x55A965 else -1))&0xffffffff
   u.mem_write(ptr,dwords(value));u.reg_write(UC_X86_REG_EAX,value);u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,pc+6);return
  if pc==0x7C978A:self.receipts.append(dict(pc=hex(pc),kind='CRT_atexit_registration',phase=self.phase));self.ret(0);return
  if self.phase!='setup':
   names={0x4C75ED:'event_destination_call',0x4C75F8:'event_target_call',0x51AA40:'infantry_destination',0x4D94B0:'foot_destination',0x718230:'teleport_stop',0x51B1F0:'infantry_target',0x51D6F0:'infantry_action',0x6FCDB0:'techno_target',0x5B3060:'mission_dispatch',0x51F3E0:'infantry_attack',0x4D4DC0:'foot_attack',0x51CBA0:'infantry_idle',0x4D82B0:'foot_idle',0x709A40:'techno_idle',0x71ABC0:'temporal_let_go',0x4D3780:'mark',0x5B35E0:'queue_mission',0x5B3570:'commence',0x65C7E0:'range',0x65C780:'next',0x65C84B:'raw_range',0x4D4EAB:'range_return'}
   if pc in names:
    sp=u.reg_read(UC_X86_REG_ESP);row=dict(phase=self.phase,frame=self.read32(0xA8ED84),pc=hex(pc),kind=names[pc],this=hex(u.reg_read(UC_X86_REG_ECX)),return_pc=hex(self.read32(sp)))
    if pc in (0x5B35E0,0x65C7E0):row['args']=[i32(u,sp+4),i32(u,sp+8)]
    if pc in (0x51B1F0,0x4D3780,0x51D6F0):row['arg0']=i32(u,sp+4)
    if pc==0x65C84B:row['raw_u32']=u.reg_read(UC_X86_REG_ESI)
    if pc==0x4D4EAB:row['range_result']=u.reg_read(UC_X86_REG_EAX)
    self.calls.append(row)
  super().hook(u,pc,n,d)
 def write_hook(self,u,access,address,size,value,data):
  if self.phase=='setup':return
  for name,p in [('actor',self.actor),('victim',self.victim),('link',self.link),('teleport',self.loco)]:
   if p<=address<p+(0x1000 if name in ('actor','victim') else 0x50):
    # Unicorn's callback can carry garbage above a byte store's width.
    # Record only the bits the original instruction actually writes.
    self.writes.append(dict(phase=self.phase,frame=self.read32(0xA8ED84),pc=hex(u.reg_read(UC_X86_REG_EIP)),object=name,offset=hex(address-p),size=size,value=value & ((1 << (size * 8)) - 1)));break
 def frame_state(self):
  u=self.u;p=self.actor;t=self.victim;l=self.link
  return dict(mission=i32(u,p+0xAC),queued=i32(u,p+0xB4),dispatch=[i32(u,p+0xC8),i32(u,p+0xD0)],target_is_victim=self.read32(p+0x2B4)==t,target_is_null=self.read32(p+0x2B4)==0,nav_is_null=self.read32(p+0x5A4)==0,temporal_target=self.read32(l+0x28)==t,temporal_head=self.read32(t+0x278)==l,victim_warped=u.mem_read(t+0x270,1)[0],link_fields={hex(o):self.read32(l+o) for o in (0x38,0x3C,0x40,0x44,0x48)},doing=i32(u,p+0x6C4),action_timer=[i32(u,p+0x100),i32(u,p+0x108),i32(u,p+0x10C)],movement_timer=[i32(u,p+0x640),i32(u,p+0x648)],blocked_timer=[i32(u,p+0x668),i32(u,p+0x670)],idle_latch=u.mem_read(p+0x6B3,1)[0],rng={k:bytes(u.mem_read(a,0x3F4)).hex() for k,a in self.rngs.items()})
 def execute(self,row):
  u=self.u
  for a,b in self.saved:u.mem_write(a,b)
  u.context_restore(self.cpu);self.cursor=self.cursor_saved;self.phase='setup';self.calls=[];self.writes=[]
  u.mem_write(self.actor+0xC8,dwords(row['start'],0,row['delay']));u.mem_write(0xA8ED84,dwords(row['event_frame']))
  before=self.frame_state();self.phase='event_setters'
  # Original Event6 admitted ordinary null-destination then null-target calls.
  # The earlier admission/radio/team clearing and later optional deploy/miner arms
  # are outside this fragment; ESI=actor and EDI=0 are supplied entry registers.
  u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,self.actor);u.reg_write(UC_X86_REG_EDI,0)
  run_checked(u,0x4C75E6,0x4C75FE,count=1000000,required_addresses=(0x51AA40,0x51B1F0,0x6FCDB0,0x718230))
  assert u.reg_read(UC_X86_REG_ESP)==SP
  after_event=self.frame_state();visits=[]
  assert after_event['rng']==before['rng']
  assert after_event['dispatch']==before['dispatch'] and after_event['mission']==before['mission'] and after_event['queued']==before['queued']
  assert after_event['target_is_null'] and after_event['nav_is_null']
  assert after_event['temporal_target'] and after_event['temporal_head'] and after_event['victim_warped']==1
  for frame in row['visits']:
   u.mem_write(0xA8ED84,dwords(frame));self.phase='dispatch';mark=len(self.calls)
   self.invoke(0x5B3060,self.actor)
   visits.append(dict(frame=frame,state=self.frame_state(),calls=self.calls[mark:]))
  assert all(v['state']['rng']['main']==before['rng']['main'] and v['state']['rng']['mapgen']==before['rng']['mapgen'] for v in visits)
  assert self.code_before==sha(bytes(u.mem_read(0x401000,0x3E0000)))
  return dict(input=row,before=before,after_event=after_event,visits=visits,calls=self.calls,writes=self.writes,code_unchanged=True)

def generate():
 m=Probe();rows=[dict(name='remaining_13',start=100,delay=16,event_frame=103,visits=[103,104,115,116]),dict(name='due_same_frame',start=100,delay=16,event_frame=116,visits=[116]),dict(name='already_due',start=100,delay=16,event_frame=117,visits=[117]),dict(name='stopped_timer_nonzero',start=-1,delay=16,event_frame=103,visits=[103,116,1000]),dict(name='stopped_timer_zero',start=-1,delay=0,event_frame=103,visits=[103])]
 out=[]
 for row in rows:
  try:out.append(m.execute(row))
  except Exception as e:raise RuntimeError((row,str(e),[hex(a)for a in m.trail]))from e
 return dict(inputs=m.inputs,constructor_boundary='Raw valid already-warping Infantry/Unit runtime objects; actual CLEG type and Teleport constructors; no Infantry/Unit constructor or firing producer',cases=out,text_sha256=m.code_before)

def metadata():
 return provenance(scope=__doc__,entry_points={'event_setters':0x4C75E6,'infantry_target':0x51B1F0,'techno_target':0x6FCDB0,'dispatch':0x5B3060,'infantry_attack':0x51F3E0,'foot_attack':0x4D4DC0,'infantry_idle':0x51CBA0,'foot_idle':0x4D82B0,'techno_idle':0x709A40,'let_go':0x71ABC0,'range':0x65C7E0},assumptions=['Supplied live human rookie ordinary Infantry with Attack1, no NavCom/queue, retained active Temporal single-head link to ordinary Unit, no pending scan/transport/parasite/particle/archived/AttackMove references; no full object constructors or original shot producer.','Original InfantryType CLEG constructor plus physical ART Sequence reader; unrelated CLEG rules fields remain constructor defaults. This is not full retail CLEG loader closure.','MissionControl static construction and actual rules MissionControl reader over physical RULESMD, optional LANGRULE, MPBattleMD and XMP03T4; lexical section caches replace physical file loading.','Event executes only admitted setter fragment4C75E6..4C75FE. Earlier admission/radio/team work and later deploy/miner arms are excluded. Native event selector never supplies Stop13; full Event proof remains the existing FV packet.','Scenario/Main/MapGen are originally seeded3. FPCW0E3F and ftol cache are supplied; no claim the WinMain producer is executed. Tick values and retained timer are supplied boundary controls; target AI/warp progress and full InfantryAI per-frame ordering excluded.'],substitutions=['Shared Reader bounded heap/delete/TLS/file transport; CRT atexit registration is a no-op; imported Interlocked increment/decrement transport matches the existing Mission owner. No native gameplay callable, mission/timer state, Temporal release, Mark(2), RNG value or return is replaced.'])|dict(write_trace_policy='UC_HOOK_MEM_WRITE values are masked to the actual store width; high bits outside the written byte/word/dword are not native state.')

def generate_open_topped_guard():
 """Carrier Stop broadcasts NULL; a hidden Guard rider retains its Temporal link.

 The already-active single-head link is supplied. Execute the actual carrier
 OpenTopped Stop suffix, original concrete passenger target setter, retained
 Guard dispatch and FireAtTarget no-target arm. One later original target-head
 Update proves the raw-damage step despite the cleared source TarCom. No full
 Event admission, BFRT constructor, original shot, whole AI or frame cadence.
 """
 p=Probe();u=p.u
 carrier=p.alloc(0x1000);typ=p.alloc(0x1900)
 u.mem_write(carrier,dwords(0x7F5C70));u.mem_write(carrier+0x6C4,dwords(typ));u.mem_write(typ+0x5E4,b'\x01')
 u.mem_write(carrier+0x114,dwords(1,p.actor));u.mem_write(p.actor+0x14,b'\x07');u.mem_write(p.actor+0x30,dwords(0))
 u.mem_write(p.actor+0x11C,dwords(carrier));u.mem_write(p.actor+0x81,b'\x01\x01');u.mem_write(p.actor+0xAC,dwords(5,-1,-1))
 u.mem_write(p.actor+0xC8,dwords(100,0,16));u.mem_write(0xA8ED84,dwords(103))
 entries=[]
 def observe_entries(uc,pc,size,data):
  names={0x710550:'passenger_target_broadcast',0x51F620:'infantry_guard',0x521320:'deployed_guard_shim',0x4D5070:'foot_guard',0x51CDB0:'idle_action',0x5206B0:'infantry_fire_at_target',0x71A760:'target_head_update',0x5218E0:'infantry_select_weapon',0x6F3330:'techno_select_weapon',0x70E140:'get_weapon',0x71ABC0:'let_go',0x709A40:'enter_idle'}
  if p.phase!='setup' and pc in names:
   entries.append(dict(phase=p.phase,pc=hex(pc),kind=names[pc],this=hex(uc.reg_read(UC_X86_REG_ECX))))
 u.hook_add(UC_HOOK_CODE,observe_entries)
 def retained(state):
  result={name:state[name] for name in ('mission','queued','target_is_null','temporal_target','temporal_head','victim_warped','doing','dispatch')}
  result['temporal_target']=p.read32(p.link+0x28)
  result['temporal_head']=p.read32(p.victim+0x278)
  result['victim_warped']=bool(state['victim_warped'])
  result['warp_remaining']=state['link_fields']['0x48']
  result['rng_sha256']={name:sha(bytes.fromhex(value)) for name,value in state['rng'].items()}
  return result
 before=retained(p.frame_state());p.phase='carrier_stop_open_topped_suffix'
 u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,carrier);u.reg_write(UC_X86_REG_EDI,0)
 run_checked(u,0x4C7639,0x4C7655,count=1000000,required_addresses=(0x710550,0x51B1F0,0x6FCDB0))
 after=retained(p.frame_state());assert u.reg_read(UC_X86_REG_ESP)==SP
 assert after['target_is_null'] and after['mission']==5 and after['temporal_target'] and after['temporal_head'] and after['victim_warped']==1
 assert after['rng_sha256']==before['rng_sha256']
 visits=[]
 for frame in (103,116,150,300,1000):
  u.mem_write(0xA8ED84,dwords(frame));p.phase='hidden_guard_dispatch';p.invoke(0x5B3060,p.actor)
  p.phase='hidden_fire_at_target_null';p.invoke(0x5206B0,p.actor)
  state=retained(p.frame_state());visits.append(dict(frame=frame,state=state))
  assert state['mission']==5 and state['target_is_null'] and state['temporal_target'] and state['temporal_head'] and state['victim_warped']==1
 # Prepared raw weapon Damage8 and nearby coordinates; actual original
 # Infantry/Techno selection and GetWeapon run inside the full Update body.
 weapon=p.alloc(0x200);u.mem_write(p.typ+0x898,dwords(weapon));u.mem_write(weapon+0xA4,dwords(8));u.mem_write(p.victim+0x9C,dwords(2944,2688,0))
 p.phase='target_head_update_after_stop';update_before=retained(p.frame_state());p.invoke(0x71A760,p.link);update_after=retained(p.frame_state())
 assert update_after['warp_remaining']==3492 and update_before['warp_remaining']==3500
 assert update_after['target_is_null'] and update_after['mission']==5 and update_after['temporal_target'] and update_after['temporal_head'] and update_after['victim_warped']==1
 assert update_after['rng_sha256']==update_before['rng_sha256']
 assert not any(entry['kind'] in ('let_go','enter_idle') for entry in entries)
 assert p.code_before==sha(bytes(u.mem_read(0x401000,0x3E0000)))
 row=dict(input=dict(name='carrier_stop_retained_hidden_guard',in_limbo=True,raw_open_top=True,carrier_open_topped=True,mission=5,timer_start=100,timer_duration=16,stop_frame=103,guard_visit_frames=[103,116,150,300,1000],warp_remaining=3500,source_raw_damage=8,source_coords=[2688,2688,0],target_coords=[2944,2688,0]),output=dict(before=before,after_stop=after,guard_visits=visits,before_target_update=update_before,after_target_update=update_after),native_entries=entries,calls=p.calls,writes=p.writes,code_unchanged=True)
 return dict(schema_version=1,inputs=p.inputs,rows=[row],text_sha256=p.code_before)

def open_topped_guard_metadata():
 return metadata()|dict(scope=generate_open_topped_guard.__doc__,entry_points={'carrier_stop_suffix':0x4C7639,'suffix_end':0x4C7655,'broadcast':0x710550,'infantry_target':0x51B1F0,'techno_target':0x6FCDB0,'dispatch':0x5B3060,'infantry_guard':0x51F620,'foot_guard':0x4D5070,'idle_action':0x51CDB0,'fire_at_target':0x5206B0,'target_head_update':0x71A760,'infantry_select_weapon':0x5218E0,'techno_select_weapon':0x6F3330,'get_weapon':0x70E140,'let_go':0x71ABC0,'enter_idle':0x709A40},assumptions=['Inherited original CLEG type constructor and physical ART sequence reader; unrelated InfantryType rule fields remain constructor defaults. MissionControl original constructor/readers and RNG seeds from Probe. Existing raw already-warping Infantry/Unit runtime preparation is not an original shot producer.','Prepared carrier with original Unit vtable7F5C70, type+5E4 OpenTopped true and cargo head/count pointing to one retained Infantry. No BFRT constructor/full rule reader, full boarding or whole Event admission/earlier hull Stop effects.','Passenger retained Guard5, Limbo+81 and raw open-top+82, original parent+11C, existing single Temporal head/link, live target; timer start100 duration16 and sampled absolute visits are supplied controls, not full game cadence.','After NULL target/Guard controls, prepared raw weaponDamage8 is installed in primary slot and nearby target coords supplied. Actual InfantrySelectWeapon, TechnoSelectWeapon, GetWeapon and TemporalUpdate execute; no weapon/full type reader closure claimed.','No full Infantry or target AI scheduling, full scene, erase callbacks or rendered BFRT Stop claim. A different passenger current mission such as Attack has distinct dispatch-to-Idle behavior in the original default corpus.'],substitutions=['Inherited bounded heap/delete/TLS/file transport, CRT atexit no-op and imported Interlocked transport. No reached native gameplay body, target setter, mission return, RNG, Temporal release or Update result is substituted. Additional entry hook observes only.'])

if __name__=='__main__':
 if '--open-topped-guard' in sys.argv:
  # Preserve sys.argv for finish_vectors' reproducible replay command.
  argv=[arg for arg in sys.argv[1:] if arg!='--open-topped-guard']
  finish_vectors(generate_open_topped_guard,HERE/'temporal_stop.open_topped_guard.json',provenance=open_topped_guard_metadata,argv=argv)
 else:finish_vectors(generate,HERE/'temporal_stop.json',provenance=metadata)
