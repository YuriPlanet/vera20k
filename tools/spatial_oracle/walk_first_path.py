"""Original Infantry Cell destination and additive produced-GI input histories.

The seven setter/three head rows retain their original supplied-prior bounds.
Produced histories reuse factory_infantry_output's initialized live VM; no eager
queue/head, second initializer or native gameplay callable is supplied.
"""
from pathlib import Path
import struct,json,sys
from unicorn import Uc,UC_ARCH_X86,UC_MODE_32,UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EBP,UC_X86_REG_EBX,UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_EDX,UC_X86_REG_EDI,UC_X86_REG_ESI,UC_X86_REG_EIP,UC_X86_REG_ESP,UC_X86_REG_FPCW
from tools.native_oracle import load_image,run_checked,STACK_BASE,STACK_SIZE,SCRATCH,RET_MAGIC,finish_vectors,provenance
from tools.spatial_oracle.map_queries import dwords,packed
from tools.spatial_oracle import walk_head_occupation as head_native

ACTOR,TARGET,TYPE,HOUSE,LOCO,VT,TVT,CELL,CVT,WEAPON,WEAPON_SLOT,RULES=[SCRATCH+i*0x2000 for i in range(12)]
MAP,TABLE,DUMMY=0x87F7E8,0xC00000,0xABDC50

def query(row):
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
 u.mem_map(STACK_BASE,STACK_SIZE);u.mem_map(SCRATCH,0x20000);u.mem_map(RET_MAGIC,0x1000)
 u.reg_write(UC_X86_REG_FPCW,0x0E7F)
 def read32(p):return struct.unpack('<I',u.mem_read(p,4))[0]
 def ret(cleanup,result):
  sp=u.reg_read(UC_X86_REG_ESP);u.reg_write(UC_X86_REG_EAX,result&0xffffffff);u.reg_write(UC_X86_REG_EIP,read32(sp));u.reg_write(UC_X86_REG_ESP,sp+4+cleanup)
 def call(entry,this,args):
  sp=STACK_BASE+STACK_SIZE-0x1000;u.mem_write(sp,dwords(RET_MAGIC,*args));u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,this)
  run_checked(u,entry,RET_MAGIC,count=100000,required_addresses=[entry]);assert u.reg_read(UC_X86_REG_ESP)==sp+4*(len(args)+1)
 u.mem_write(VT,bytes(u.mem_read(0x7EB058,0x600)));u.mem_write(TVT,bytes(u.mem_read(0x7F5C70,0x600)))
 u.mem_write(ACTOR,dwords(VT));u.mem_write(TARGET,dwords(TVT));u.mem_write(ACTOR+0x21C,dwords(HOUSE));u.mem_write(HOUSE+0x1EC,bytes([int(row.get('human',True))]))
 u.mem_write(ACTOR+0x6C0,dwords(TYPE));u.mem_write(ACTOR+0x6C4,dwords(row.get('doing',0)))
 u.mem_write(ACTOR+0xAC,dwords(row.get('mission',1)));u.mem_write(ACTOR+0xB4,dwords(row.get('queued_mission',-1)))
 u.mem_write(ACTOR+0x90,b'\x01');u.mem_write(ACTOR+0x9C,dwords(10*256+64,10*256+64,0))
 u.mem_write(ACTOR+0x2B4,dwords(TARGET if row.get('target',True) else 0));u.mem_write(TARGET+0x14,dwords(4 if row.get('foot',True) else 0))
 u.mem_write(TARGET+0x9C,dwords(*row.get('target_xyz',[11*256+220,10*256+220,123])))
 u.mem_write(ACTOR+0x5A0,dwords(123));u.mem_write(ACTOR+0x5A4,dwords(TARGET))
 u.mem_write(ACTOR+0x558,packed(9,8));u.mem_write(ACTOR+0x5E0,dwords(2,3,4,5));u.mem_write(ACTOR+0x598,dwords(row.get('nav_queue',0)))
 u.mem_write(ACTOR+0x58C,dwords(SCRATCH+0x1A000));u.mem_write(SCRATCH+0x1A000,dwords(TARGET,CELL))
 if row.get('contact'):
  u.mem_write(ACTOR+0xE4,dwords(SCRATCH+0x1A100));u.mem_write(ACTOR+0xE8,dwords(1));u.mem_write(SCRATCH+0x1A100,dwords(TARGET))
 u.mem_write(ACTOR+0x6B7,b'\x01');u.mem_write(ACTOR+0x640,dwords(50,0,5));u.mem_write(ACTOR+0x668,dwords(40,0,6))
 u.mem_write(0xA8ED84,dwords(100));u.mem_write(0x8871E0,dwords(RULES));u.mem_write(RULES+0x1768,dwords(22))
 u.mem_write(RULES+0x1760,struct.pack('<d',row.get('retry_delay',0.0)))
 u.mem_write(DUMMY+0x24,packed(99,98));u.mem_write(0xB45BE8,dwords(0,0,0));u.mem_write(0xB45C28,dwords(104))
 call(0x75AA90,LOCO,[]);u.mem_write(LOCO+0xC,dwords(ACTOR));u.mem_write(ACTOR+0x674,dwords(LOCO+4))
 u.mem_write(LOCO+0x1C,dwords(30*256+128,10*256+128,0));u.mem_write(LOCO+0x34,b'\x01')
 if row.get('head') or row.get('process_motion'):u.mem_write(LOCO+0x28,dwords(11*256+64,10*256+64,0))
 produced_motion=None
 if row.get('process_motion'):
  # Execute original entry/head gate and +36 producer, stopping before numeric
  # motion. A later cleared head is a declared retirement seam, not Process.
  u.reg_write(UC_X86_REG_ECX,LOCO);u.reg_write(UC_X86_REG_ESP,STACK_BASE+STACK_SIZE-0x1000)
  run_checked(u,0x75AEC0,0x75BD29,count=100,required_addresses=[0x75AEC0,0x75BD25])
  produced_motion=u.mem_read(LOCO+0x36,1)[0]
  if not row.get('head'):u.mem_write(LOCO+0x28,dwords(0,0,0))
 table=bytearray(0x100000)
 for i,(x,y) in enumerate([(10,10),(11,10)]):
  cell=CELL+i*0x200;struct.pack_into('<I',table,(y*512+x)*4,cell);u.mem_write(cell,dwords(CVT));u.mem_write(cell+0x24,packed(x,y));u.mem_write(cell+0x44,dwords(0xffffffff))
 u.mem_write(CVT,bytes(u.mem_read(0x7E4EEC,0x100)));u.mem_write(CVT+0x48,dwords(0x486840));u.mem_write(TABLE,bytes(table));u.mem_write(MAP+0x13C,dwords(TABLE,0x40000));u.mem_write(0x87F924,dwords(TABLE))
 u.mem_write(WEAPON_SLOT,dwords(WEAPON));u.mem_write(WEAPON+0x134,bytes([int(row.get('cell_rangefinding',False))]))
 events=[]
 recent=[]
 def observer(_u,address,_size,_data):
  sp=u.reg_read(UC_X86_REG_ESP)
  recent.append(hex(address))
  if SCRATCH<=address<SCRATCH+0x20000:raise RuntimeError(recent[-20:])
  if address in [read32(0x7E11C8),read32(0x7E11CC)]:
   p=read32(sp+4);value=read32(p)+(1 if address==read32(0x7E11C8) else -1);u.mem_write(p,dwords(value));ret(4,value)
  elif address in [0x4D9FF0,0x41BDD0,0x5F65A0,0x6F7970,0x565730,0x6F77B0,0x5B3040,0x51AA40,0x51AD11,0x4D94B0,0x75ADA0,0x75ACB0,0x4D3920,0x521B40,0x4D896E]:events.append(hex(address))
 u.hook_add(UC_HOOK_CODE,observer)
 u.mem_write(ACTOR+0x5A4,dwords(0));u.mem_write(LOCO+0x14,dwords(1));u.mem_write(LOCO+0x34,b'\0');u.mem_write(LOCO+0x1C,dwords(0,0,0))
 initial_timer=dict(start_frame=read32(ACTOR+0x640),duration=read32(ACTOR+0x648))
 call(0x51AA40,ACTOR,[CELL+0x200,1])
 def state():return dict(movement_timer=dict(start_frame=read32(ACTOR+0x640),duration=read32(ACTOR+0x648)),reference=list(struct.unpack('<hh',u.mem_read(ACTOR+0x558,4))),nav_queue_count=read32(ACTOR+0x598),nav_queue_entries=list(struct.unpack('<II',u.mem_read(SCRATCH+0x1A000,8))),queue=list(struct.unpack('<iiii',u.mem_read(ACTOR+0x5E0,16))),head=list(struct.unpack('<iii',u.mem_read(LOCO+0x28,12))),destination=list(struct.unpack('<iii',u.mem_read(LOCO+0x1C,12))),nav=read32(ACTOR+0x5A4),moving=u.mem_read(LOCO+0x34,1)[0],motion=u.mem_read(LOCO+0x36,1)[0])
 before=state()
 endpoint=0x75BD29 if row.get('head') else 0x4D3920
 if not any(before['destination']):
  call(0x75AEC0,LOCO,[0])
  return dict(input=row,initial_movement_timer=initial_timer,setter=before,process=state(),find_args=None,events=events)
 sp=STACK_BASE+STACK_SIZE-0x1000;u.mem_write(sp,dwords(RET_MAGIC,0));u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,LOCO)
 run_checked(u,0x75AEC0,endpoint,count=100000,required_addresses=[0x75AEC0,0x75BD25 if row.get('head') else 0x75AFC5])
 sp=u.reg_read(UC_X86_REG_ESP)
 return dict(input=row,initial_movement_timer=initial_timer,setter=before,process=state(),find_args=None if row.get('head') else list(struct.unpack('<III',u.mem_read(sp+4,12))),events=events)

class ChaseHead(head_native.Original):
 def __init__(self,row):
  self.row=row
  super().__init__()
  self.uc.reg_write(UC_X86_REG_FPCW,0x0E7F)
 def observe(self,u,address,size,data):
  if address==head_native.MISSION_GET:
   self.events.append(['mission',self.row['mission']]);self.ret(0,self.row['mission'])
  else:super().observe(u,address,size,data)
 def call(self,entry,this,args):
  if entry==0x75C240:
   self.uc.mem_write(head_native.OWNER+0x5A4,dwords(head_native.CELL if self.row['nav'] else 0))
   self.uc.mem_write(head_native.OWNER+0x2B4,dwords(head_native.BUILDING if self.row['target'] else 0))
   self.before_rng=bytes(self.uc.mem_read(head_native.SCENARIO+0x218,0x3F4)).hex()
  super().call(entry,this,args)

def head_query(row):
 n=ChaseHead(row)
 output=n.producer({'input':[2752,2624,260],'ground':0,'deck':0,'owner':41,'current':[2496,2624,260],'seed':31})
 return dict(input=row,output=output,rng_before=n.before_rng,rng_after=bytes(n.uc.mem_read(head_native.SCENARIO+0x218,0x3F4)).hex())

def generate():
 return dict(setter=[query(row) for row in [{},{'mission':5},{'head':True},{'doing':27},{'doing':27,'human':False},{'nav_queue':1},{'retry_delay':7.75}]],
             head_producer=[head_query(row) for row in [{'mission':0,'nav':False,'target':False},{'mission':1,'nav':True,'target':True},{'mission':1,'nav':False,'target':True}]])

class GIMoveHistory:
 """Observe original input/event/AI histories on the existing live Foot VM."""
 def __init__(self,f,final,name):
  import copy,hashlib
  from collections import deque
  from unicorn import UC_HOOK_MEM_WRITE
  from tools.spatial_oracle import building_construction as bc
  from tools.spatial_oracle.refinery_dock import cell
  from tools.spatial_oracle._factory_infantry_output.runtime import require
  self.f,self.u,self.r,self.bc,self.cell=f,f.u,f.read32,bc,cell
  self.require,self.copy,self.hashlib=require,copy,hashlib
  self.unit=name.startswith('unit_')
  self.actor=f.source if self.unit else final['delivered_infantry'][-1]['pointer']
  self.producer=final['barracks']['pointer']
  self.loco=self.r(self.actor+0x674)-4
  self.name=name;self.sequence=0;self.phase='created'
  self.events=[];self.writes=[];self.pending=[];self.boundaries=[]
  self.inputs=[];self.streams={};self.recent=deque(maxlen=80)
  self.draw_start=len(f.draws);self.advance_start=len(f.advances)
  self.initial_actor_allocation=f.source_arena if self.unit else next(x for x in f.allocations if x['pointer']==self.actor)
  self.actor_bytes=self.initial_actor_allocation['size']
  require(0x6DC<=self.actor_bytes<=0x1000,'Unexpected original GI allocation')
  require(self.r(self.actor)==(0x7F5C70 if self.unit else 0x7EB058),'Live actor vtable differs')
  self.loco_bytes=next(x['size'] for x in f.allocations if x['pointer']==self.loco) if self.unit else 0x38
  if self.unit:
   require(self.loco_bytes==0x70 and self.r(self.loco+4)==0x7E7EB0,'Expected original constructed Drive')
   target=self.r(self.actor+0x2B4)
   require(target and self.r(target)==0x7E3EBC,'Expected retained original Building target')
   target_type=self.r(target+0x520)
   self.target_prior=dict(pointer=target,vtable=self.r(target),native_id=self.r(target+0x10),
       raw=bytes(self.u.mem_read(target,0x800)).hex(),owner=self.r(target+0x21C),
       health=self.ints(target+0x6C,1)[0],alive=self.u.mem_read(target+0x90,1)[0],
       limbo=self.u.mem_read(target+0x81,1)[0],marked=self.u.mem_read(target+0x74,1)[0],
       flags=self.r(target+0x14),location=self.ints(target+0x9C,3),
       mission=self.ints(target+0xAC,1)[0],queued=self.ints(target+0xB4,1)[0],
       type=dict(pointer=target_type,name=bytes(self.u.mem_read(target_type+0x24,25)).split(b'\0')[0].decode('latin1')),
       origin='Retained native source TarCom, original Building type/vtable; supplied admitted fixture prior, not a fresh target')
  self.map_cells=[cell(x,y) for y in range(32) for x in range(32)]
  self.map_before={p:bytes(self.u.mem_read(p,0x200)) for p in self.map_cells}
  self.map_base=min(self.map_cells);self.map_end=max(self.map_cells)+0x200
  self.vtables={p:bytes(self.u.mem_read(p,n)) for p,n in
      [(self.r(self.actor),0x600),(self.r(self.loco),0x20),(self.r(self.loco+4),0x70)]}
  self.hooks=[self.u.hook_add(UC_HOOK_CODE,self.observe),
      self.u.hook_add(UC_HOOK_MEM_WRITE,self.written)]
  self.initial=self.snapshot()

 def next_sequence(self):
  self.sequence+=1
  return self.sequence

 def ints(self,p,n):
  return list(struct.unpack('<'+'i'*n,self.u.mem_read(p,4*n)))

 def rng(self):
  output={}
  values=self.f.rng()
  self.require(set(values)=={'main','scenario','mapgen'},'All three RNG owners required')
  for name,raw in values.items():
   data=bytes.fromhex(raw)
   self.require(len(data)==0x3F4,'Unexpected RandomClass state length')
   digest=self.hashlib.sha256(data).hexdigest()
   self.streams.setdefault(name+':'+digest,dict(stream=name,sha256=digest,bytes=raw))
   output[name]=name+':'+digest
  return output

 def vector(self,p):
  r,u=self.r,self.u
  pointer,count=r(p+4),r(p+16)
  self.require(count<=1024,'Unbounded native vector at '+hex(p))
  capacity=r(p+8)
  self.require(count<=capacity<=1024,'Invalid native vector capacity at '+hex(p))
  return dict(address=p,header=bytes(u.mem_read(p,24)).hex(),pointer=pointer,
      capacity=capacity,count=count,items=[r(pointer+4*i) for i in range(count)],
      backing_raw=bytes(u.mem_read(pointer,4*capacity)).hex() if pointer and capacity else '')

 def members(self,p):
  nodes=[];seen=set()
  while p:
   self.require(p not in seen and len(nodes)<128,'Invalid original Cell list')
   seen.add(p);nodes.append(dict(pointer=p,vtable=self.r(p),next=self.r(p+0x30),
       flags=self.r(p+0x14),location=self.ints(p+0x9C,3)))
   p=self.r(p+0x30)
  return nodes

 def cell_state(self,p):
  u,r=self.u,self.r
  return dict(pointer=p,xy=list(struct.unpack('<2h',u.mem_read(p+0x24,4))),
      raw_ground=r(p+0x124),raw_upper=r(p+0x128),ground_head=r(p+0xE4),
      upper_head=r(p+0xE8),ground_members=self.members(r(p+0xE4)),
      upper_members=self.members(r(p+0xE8)),land=self.ints(p+0xEC,1)[0],
      level=u.mem_read(p+0x11B,1)[0],slope=u.mem_read(p+0x11C,1)[0],
      flags=r(p+0x140),shroud=r(p+0x12C),raw=bytes(u.mem_read(p,0x200)).hex())

 def rings(self):
  r,u=self.r,self.u
  def ring(p,start,limit):
   count,head,tail=r(p),r(p+4),r(p+8)
   self.require(count<=limit and head<limit and tail<limit,'Native Event ring is invalid')
   return dict(count=count,head=head,tail=tail,header=bytes(u.mem_read(p,12)).hex(),
       events=[dict(slot=(head+i)%limit,bytes=bytes(u.mem_read(start+((head+i)%limit)*0x6F,0x6F)).hex()) for i in range(count)])
  return dict(outlist=ring(0xA802C8,0xA802D4,128),dolist=ring(0x8B41F8,0x8B4204,16384))

 def snapshot(self):
  p,l,r,u=self.actor,self.loco,self.r,self.u
  positions=[self.ints(p+0x9C,3),self.ints(l+(0x34 if self.unit else 0x1C),3),self.ints(l+(0x40 if self.unit else 0x28),3)]
  picked={self.cell(x//256,y//256) for x,y,z in positions if 0<=x//256<32 and 0<=y//256<32}
  picked.update(self.cell(*xy) for xy in ((20,20),(21,20)))
  # Full raw bytes of occupied/paid/request/current Cells remain alongside
  # the complete ordered Cell writes. An empty untouched Cell is in the
  # baseline map content address, rather than repeatedly copied here.
  active=[q for q in self.map_cells if r(q+0xE4) or r(q+0xE8) or r(q+0x124) or r(q+0x128)]
  picked.update(active)
  t=r(p+(0x6C4 if self.unit else 0x6C0))
  state=dict(frame=r(self.bc.FRAME),actor=p,actor_raw=bytes(u.mem_read(p,self.actor_bytes)).hex(),
      location=positions[0],marked=u.mem_read(p+0x74,1)[0],on_bridge=u.mem_read(p+0x8C,1)[0],
      object_list_next=r(p+0x30),mission=self.ints(p+0xAC,1)[0],queued=self.ints(p+0xB4,1)[0],
      mission_status=self.ints(p+0xBC,1)[0],mission_timer=self.ints(p+0xC8,3),
      nav=r(p+0x5A4),aux_nav=r(p+0x5A0),archive=r(p+0x218),target=r(p+0x2B4),
      nav_queue=self.vector(p+0x588),destination_history=self.vector(p+0x5AC),
      path=self.ints(p+0x5E0,24),reference_cell=list(struct.unpack('<2h',u.mem_read(p+0x558,4))),
      speed_fraction_bits=bytes(u.mem_read(p+0x578,8)).hex(),
      movement_timer=self.ints(p+0x640,3),retry=self.ints(p+0x64C,1)[0],
      blocked_timer=self.ints(p+0x668,3),raw_6b7=u.mem_read(p+0x6B7,1)[0],entry_blocked=u.mem_read(p+0x6DC,1)[0],
      doing=self.ints(p+0x6C4,1)[0],prone=u.mem_read(p+0x6DB,1)[0],
      stage_words=self.ints(p+0xF8,4),sequence_timer=self.ints(p+0x100,4),
      legacy_index=self.ints(p+0x520,1)[0],tube_684_byte=u.mem_read(p+0x684,1)[0],
      tube_684_raw4=bytes(u.mem_read(p+0x684,4)).hex(),
      tether=u.mem_read(p+0x418,1)[0],contacts=[r(r(p+0xE4)+i*4) for i in range(r(p+0xE8))],
      house=dict(pointer=r(p+0x21C),human=u.mem_read(r(p+0x21C)+0x1EC,1)[0],current=r(0xA83D4C)),
      walk=dict(base=l,interface=l+4,raw=bytes(u.mem_read(l,0x38)).hex(),
          owner=r(l+0xC),references=r(l+0x14),destination=positions[1],head=positions[2],
          moving=u.mem_read(l+0x34,1)[0],motion=u.mem_read(l+0x36,1)[0]),
      type=dict(pointer=t,name=bytes(u.mem_read(t+0x24,25)).split(b'\0')[0].decode('latin1'),
          movement_zone=self.ints(t+0x5B4,1)[0],speed_type=self.ints(t+0x67C,1)[0],
          locomotor_guid=bytes(u.mem_read(t+0x34C,16)).hex(),
          fraidycat=u.mem_read(t+0xEBF,1)[0],cyborg=u.mem_read(t+0xEAC,1)[0],crawls=u.mem_read(t+0xEBD,1)[0]),
      cells=[self.cell_state(q) for q in sorted(picked)],rng=self.rng(),
      rings=self.rings(),planning_mode=u.mem_read(0xAC4CF4,1)[0],voice_mode=u.mem_read(0x822CF2,1)[0],
      draw_count=len(self.f.draws),advance_count=len(self.f.advances),fpcw=u.reg_read(UC_X86_REG_FPCW))
  if self.name in ('archive_A_B','prone_B','prone_A_A','findpath_existing_idle_MTNK','findpath_existing_GAPILE',
      'mode5_current_mismatch_B','mode5_current_nonhuman_Doing27_input_class_B',
      'nonDeployer_Doing27_B','berserk_B'):
   house=r(p+0x21C)
   state['input_context']=dict(game_mode=r(0xA8B238),current_house=r(0xA83D4C),actor_owner=house,
       human=u.mem_read(house+0x1EC,1)[0],player_control=u.mem_read(house+0x1ED,1)[0],
       deployer=u.mem_read(t+0xEC8,1)[0],berserk=u.mem_read(p+0x298,1)[0])
  if self.unit:
   for key in ('walk','doing','prone','entry_blocked'):
    del state[key]
   for key in ('fraidycat','cyborg','crawls'):
    del state['type'][key]
   state.update(health=self.ints(p+0x6C,1)[0],alive=u.mem_read(p+0x90,1)[0],limbo=u.mem_read(p+0x81,1)[0],
       deploy_bytes=list(u.mem_read(p+0x6E0,3)),force_reassign=u.mem_read(p+0x1F8,1)[0],
       raw_techno_2b0=r(p+0x2B0),
       skip_move=u.mem_read(p+0x6AC,1)[0],suspended_mission=self.ints(p+0xB0,1)[0],
       actor_flags=r(p+0x14),native_id=r(p+0x10),logic_registered=u.mem_read(p+0x98,1)[0],
       drive=dict(base=l,interface=l+4,raw=bytes(u.mem_read(l,self.loco_bytes)).hex(),
           owner=r(l+0xC),references=r(l+0x14),power=u.mem_read(l+0x10,1)[0],
           destination=positions[1],head=positions[2],word_4c=r(l+0x4C),
           # Original4B3E00 stores the class target fraction here, distinct
           # from Foot+578 and the constructor's separate DWORD+68.
           target_speed_fraction_bits=bytes(u.mem_read(l+0x50,8)).hex(),selector=self.ints(l+0x58,1)[0],
           cursor=self.ints(l+0x5C,1)[0],reversed=u.mem_read(l+0x60,1)[0],
           latch=u.mem_read(l+0x62,1)[0],valid=u.mem_read(l+0x63,1)[0],
           straight=u.mem_read(l+0x64,1)[0],flag_65=u.mem_read(l+0x65,1)[0],dword_68=r(l+0x68)))
   state['type'].update(crusher=u.mem_read(t+0xD28,1)[0],move_to_shroud=u.mem_read(t+0xC8D,1)[0],
       simple_deployer=u.mem_read(t+0xE13,1)[0],balloon_hover=u.mem_read(t+0xD6A,1)[0],
       teleporter=u.mem_read(t+0xCD4,1)[0],passengers=self.ints(t+0x5E0,1)[0])
   house=r(p+0x21C)
   state['input_context']=dict(game_mode=r(0xA8B238),current_house=r(0xA83D4C),actor_owner=house,
       human=u.mem_read(house+0x1EC,1)[0],player_control=u.mem_read(house+0x1ED,1)[0],berserk=u.mem_read(p+0x298,1)[0])
  return state

 WATCH={0x51F800:('GI_WhatAction',12),0x4DDDE0:('Foot_WhatAction',12),
     0x700600:('Techno_WhatAction',12),0x54F5C0:('KeyDown',4),0x53EC90:('KeyState',0),
     0x51F250:('GI_CellClick',16),0x4D7D50:('Foot_CellClick',16),
     0x4DE1D0:('Foot_CellResolver',12),0x51BF90:('GI_CanEnter',None),
     0x6FFBE0:('Techno_ClickedMission',None),0x637AA0:('PlanningMode',0),
     0x646E90:('Event_Issuer',12),0x4C6860:('Event_Construct',40),
     0x4C6CB0:('Event_Execute',None),0x51AA40:('GI_SetDestination',8),
     0x4D94B0:('Foot_SetDestination',8),0x75ACB0:('Walk_MoveTo',4),
     0x75ADA0:('Walk_StopMoving',0),0x51DAF0:('GI_Stop',0),
     0x51D6F0:('GI_DoAction',12),0x5B35E0:('QueueMission',8),
     0x51BAB0:('GI_AI',0),0x520F40:('GI_Movement',0),0x75AEC0:('Walk_Process',4),
     0x4D3920:('Foot_FindPath',12),0x75C240:('Walk_HeadProducer',4),
     0x56DC20:('Map_FindNearbyPassable',60),0x56D230:('Map_Zone',12),
     0x42D170:('AStar_PathEstimate',20),
     0x481180:('Cell_HeadChoice',None),0x5217C0:('GI_RawMark',4),
     0x521850:('GI_RawClear',4),0x47E8A0:('Cell_OccupyDown',8),
     0x47EAE0:('Cell_OccupyUp',None),0x65A970:('Radio_Transmit',None),
     0x4D9FF0:('Foot_Stop',0),0x4DA030:('Foot_RecycleDestination',0),
     0x65C780:('RNG_Random',0),0x65C7E0:('RNG_RandomRanged',8)}
 UNIT_WATCH={0x7404B0:('Unit_WhatAction',12),0x738910:('Unit_CellClick',16),
     0x50B6F0:('House_InputOwner',0),0x73F0A0:('Unit_CanEnter',20),
     0x741970:('Unit_SetDestination',8),0x6FCDB0:('Techno_SetTarget',4),
     0x7360C0:('Unit_AI',0),0x4B0500:('Drive_Process',4),
     0x4AFD40:('Drive_MoveTo',16),0x4AFE00:('Drive_StopMoving',4),
     0x4AFB80:('Drive_IsMoving',4),0x4C65E0:('StopEvent_Construct',16)}
 UNIT_RET8=(0x741A7D,0x742D24,0x742E37,0x743170,0x743184)

 def finish_returns(self,pc,sp):
  for row in self.pending[:]:
   if pc==row['return_pc'] and sp>row['sp']:
    row.update(return_sequence=self.next_sequence(),returned_sp=sp,
        returned_eax=self.u.reg_read(UC_X86_REG_EAX),after=self.snapshot())
    if row['pc']==0x51AA40:
     self.require(row.get('true_ret8_pc')==0x51B1DE,'Class setter did not reach true RET8')
     self.require(sp==row['sp']+12,'Class setter original RET8 stack cleanup differs')
    if self.unit and row['pc'] in self.UNIT_WATCH:
     self.require(sp==row['sp']+4+row['declared_cleanup'],'Unit/COM original cleanup differs')
     if row['pc']==0x741970:
      self.require(row.get('true_ret8_pc') in self.UNIT_RET8,'Unit setter did not reach true RET8')
    if row['pc']==0x70C610:
     self.require(sp==row['sp']+8,'Archive setter original RET4 stack cleanup differs')
    if row['pc'] in (0x4C6860,0x4C65E0):
     row['constructed_event_bytes']=bytes(self.u.mem_read(row['this'],0x6F)).hex()
    if row['pc']==0x4DE1D0:
     row['resolved_cell']=list(struct.unpack('<2h',self.u.mem_read(row['args'][0],4)))
     if self.name=='unit_shift_outside_playfield_B':
      row['resolved_cell_fields']=self.cell_state(self.cell(*row['resolved_cell']))
    if row['pc']==0x56DC20:
     row['chosen_cell']=list(struct.unpack('<2h',self.u.mem_read(row['args'][0],4)))
    self.pending.remove(row)

 def observe(self,u,pc,size,data):
  self.recent.append(pc)
  sp=u.reg_read(UC_X86_REG_ESP)
  self.finish_returns(pc,sp)
  if pc in (0x65C84B,0x65C880):
   self.events.append(dict(sequence=self.next_sequence(),kind='original_RNG_advance_site',pc=pc,
       frame=self.r(self.bc.FRAME),phase=self.phase,sp=sp,
       edx=u.reg_read(UC_X86_REG_EDX),esi=u.reg_read(UC_X86_REG_ESI),
       eax=u.reg_read(UC_X86_REG_EAX),edi=u.reg_read(UC_X86_REG_EDI),rng=self.rng()))
  if pc==0x51B1DE or self.unit and pc in self.UNIT_RET8:
   entry=0x51AA40 if pc==0x51B1DE else 0x741970
   candidates=[r for r in self.pending if r['pc']==entry]
   self.require(candidates,'RET8 without class entry')
   row=candidates[-1]
   self.require(sp==row['sp'] and self.r(sp)==row['return_pc'],'Class RET8 entry stack differs')
   row.update(true_ret8_pc=pc,true_ret8_sequence=self.next_sequence(),true_ret8_sp=sp,
       true_ret8_bytes=bytes(u.mem_read(pc,3)).hex(),at_ret8=self.snapshot())
  if pc in (0x4D3CC7,0x4D3DF9,0x700B03,0x700B1E,0x700B75,0x4D806F,0x4C747C):
   self.events.append(dict(sequence=self.next_sequence(),kind='original_caller_site',pc=pc,
       frame=self.r(self.bc.FRAME),phase=self.phase,sp=sp,
       ecx=u.reg_read(UC_X86_REG_ECX),eax=u.reg_read(UC_X86_REG_EAX),
       stack=self.ints(sp,6),state=self.snapshot()))
  if self.name=='unit_shift_outside_playfield_B' and pc==0x578460:
   name,cleanup='Map_IsInPlayfield',8
  elif self.name=='archive_A_B' and pc==0x70C610:
   name,cleanup='Archive_SetTarget',4
  elif self.unit and pc in self.UNIT_WATCH:name,cleanup=self.UNIT_WATCH[pc]
  elif pc in self.WATCH:name,cleanup=self.WATCH[pc]
  else:return
  row=dict(sequence=self.next_sequence(),kind=name,pc=pc,frame=self.r(self.bc.FRAME),
      phase=self.phase,sp=sp,this=u.reg_read(UC_X86_REG_ECX),return_pc=self.r(sp),
      declared_cleanup=cleanup,args=[self.r(sp+4+i*4) for i in range(max(10,(cleanup or 0)//4))],before=self.snapshot())
  if pc==0x56DC20:
   row.update(seed_cell=list(struct.unpack('<2h',u.mem_read(row['args'][1],4))),
       target_cell=list(struct.unpack('<2h',u.mem_read(row['args'][12],4))))
  self.events.append(row);self.pending.append(row)

 def written(self,u,access,address,size,value,data):
  ranges=[(self.actor,self.actor+self.actor_bytes,'actor'),(self.loco,self.loco+self.loco_bytes,'Drive' if self.unit else 'Walk'),
      (self.map_base,self.map_end,'Cell'),(0xA802C8,0xA83C54,'OutList'),
      (0x8B41F8,0xA70204,'DoList'),(0xA8EB60,0xA8EC40,'Options')]
  for offset in (0x588,0x5AC):
   p,n=self.r(self.actor+offset+4),self.r(self.actor+offset+8)
   if p and n and n<1024:ranges.append((p,p+4*n,'vector_'+hex(offset)))
  labels=[name for lo,hi,name in ranges if address<hi and lo<address+size]
  if labels:self.writes.append(dict(sequence=self.next_sequence(),pc=u.reg_read(UC_X86_REG_EIP),
      frame=self.r(self.bc.FRAME),phase=self.phase,address=address,size=size,value=value,
      before=bytes(u.mem_read(address,size)).hex(),owners=labels))

 def invoke(self,label,pc,this,*args,count=2000000,wall=60000000):
  self.phase=label;self.f.phase='gi_reissue_'+self.name+'_'+label
  u,sp=self.u,self.bc.SP
  row=dict(label=label,entry=pc,this=this,args=list(args),before=self.snapshot(),
      first_sequence=self.sequence+1,draw_start=len(self.f.draws),advance_start=len(self.f.advances))
  self.boundaries.append(row)
  u.mem_write(sp,dwords(RET_MAGIC,*args));u.reg_write(UC_X86_REG_ECX,this);u.reg_write(UC_X86_REG_ESP,sp)
  run_checked(u,pc,RET_MAGIC,count=count,timeout_us=wall,required_addresses=[pc])
  returned_sp=u.reg_read(UC_X86_REG_ESP)
  self.finish_returns(RET_MAGIC,returned_sp)
  self.require(returned_sp==sp+4*(len(args)+1),'Actual top-level stack cleanup differs')
  row.update(returned_eax=u.reg_read(UC_X86_REG_EAX),returned_sp=returned_sp,after=self.snapshot(),
      last_sequence=self.sequence,requests=self.copy.deepcopy(self.f.draws[row['draw_start']:]),
      advances=self.copy.deepcopy(self.f.advances[row['advance_start']:]))
  return row['returned_eax']

 def pump(self):
  self.phase='actual_local_OutList_DoList_dispatch';self.f.phase='gi_reissue_'+self.name+'_'+self.phase
  row=dict(label=self.phase,entry=0x6474C7,stop=0x6474BF,before=self.snapshot(),first_sequence=self.sequence+1,
      draw_start=len(self.f.draws),advance_start=len(self.f.advances))
  self.boundaries.append(row)
  self.u.reg_write(UC_X86_REG_ESP,self.bc.SP)
  run_checked(self.u,0x6474C7,0x6474BF,count=10000000,timeout_us=60000000,required_addresses=[0x6474C7])
  self.finish_returns(0x6474BF,self.u.reg_read(UC_X86_REG_ESP))
  row.update(after=self.snapshot(),last_sequence=self.sequence,
      requests=self.copy.deepcopy(self.f.draws[row['draw_start']:]),
      advances=self.copy.deepcopy(self.f.advances[row['advance_start']:]))

 def frame(self,value):
  before=self.r(self.bc.FRAME)
  self.u.mem_write(self.bc.FRAME,dwords(value))
  self.events.append(dict(sequence=self.next_sequence(),kind='authored_absolute_frame',before=before,after=value))

 def click(self,xy,keys=(),*,dispatch=True):
  from tools.spatial_oracle import walk_move_admission as owner
  row=owner.live_cell_input(self.f,self.actor,xy,keys=keys,invoke=self.invoke,
      snapshot=self.snapshot,sequence=self.next_sequence,record=self.inputs.append)
  if dispatch:self.pump()

 def supply_fields(self,words,*,bounds):
  """Retain literal component priors; never supply a gameplay answer."""
  supplied=[]
  for p,n,value in words:
   before=bytes(self.u.mem_read(p,n)).hex();self.u.mem_write(p,value.to_bytes(n,'little'))
   supplied.append(dict(address=p,bytes=n,before=before,after=bytes(self.u.mem_read(p,n)).hex()))
  self.events.append(dict(sequence=self.next_sequence(),kind='supplied_native_input_context',words=supplied,bounds=bounds))

 def execute(self):
  import traceback
  failure=None
  try:
   self.invoke('original_Options_registered_defaults',0x4E7E20,0)
   self.defaults={hex(a):self.r(a) for a in (0xA8EBF8,0xA8EBFC,0xA8EC00,0xA8EC04,0xA8EC08,0xA8EC0C)}
   self.require(list(self.defaults.values())==[0x12,0x12,0x11,0x11,0x10,0x10],'Original modifier default values differ')
   a,b=(20,20),(21,20)
   if self.name in ('plain_A_B','unit_plain_A_B'):
    self.click(a);self.click(b)
   elif self.name in ('shift_A_B','unit_shift_A_B'):
    self.click(a,keys=(0x10,));self.click(b,keys=(0x10,))
   elif self.name=='unit_shift_outside_playfield_B':
    from tools.spatial_oracle.anytown_damage.navigation import Navigation,BASE
    # An authored coordinate, not an authored resolver/passability answer.
    # Reuse the actual constructed map/zone/terrain state and retain its
    # literal query inputs before the original wrappers/FNPC run.
    requested=(4,20)
    # Reuse the existing navigation observer on this same VM. Width/side
    # are observation dimensions derived from the original Map header;
    # no Navigation instance or graph/initializer is reconstructed.
    self.uc=self.u;self.width=self.r(0x87F7E8+0xF4)+self.r(0x87F7E8+0xF8)
    self.side=self.width+1
    self.require(self.r(0x87F7E8+0x68)==BASE,'Existing navigation plane owner differs')
    self.resolver_map_prior=dict(map_address=0x87F7E8,
        raw_header=bytes(self.u.mem_read(0x87F7E8,0x150)).hex(),
        land_table_address=0x89EA40,
        land_table_hex=bytes(self.u.mem_read(0x89EA40,12*36)).hex(),
        navigation=Navigation.nav_snapshot(self),
        cells=[self.cell_state(self.cell(*xy)) for xy in sorted({requested,a,
            tuple(v//256 for v in self.initial['location'][:2])})],
        requested=list(requested),
        bounds='Authored existing physical Cell4,20 is selected from original578460 bounds and recorded map header; original query/resolver establishes the outcome. No map/zone/terrain, gameplay query, resolver, event, class or RNG answer is supplied. Full baseline Cell bytes remain retained. Earlier40,20 and31,20 attempts remain separate failed receipts with as-run sources.')
    self.click(a,keys=(0x10,));second_click_sequence=self.sequence
    self.click(requested,keys=(0x10,))
    self.require(any(row.get('pc')==0x56DC20 and row['sequence']>second_click_sequence
        for row in self.events),
        'Original outside-playfield resolver did not reach FNPC')
    self.require(any(row.get('pc')==0x4DE1D0 and row.get('resolved_cell')!=list(requested)
        and row['sequence']>second_click_sequence for row in self.events),
        'Original outside-playfield resolver did not distinguish the requested Cell')
   elif self.name=='other12_A_B':
    self.click(a,keys=(0x12,));self.click(b,keys=(0x12,))
   elif self.name in ('plain_A_A','unit_plain_A_A'):
    self.click(a);self.click(a)
   elif self.name=='unit_shift_A_A':
    self.click(a,keys=(0x10,));self.click(a,keys=(0x10,))
   elif self.name in ('plain_A_later_B','unit_plain_A_later_B'):
    self.click(a);self.frame(self.r(self.bc.FRAME)+1);self.click(b)
   elif self.name=='unit_shift_A_later_B':
    self.click(a,keys=(0x10,));self.frame(self.r(self.bc.FRAME)+1);self.click(b,keys=(0x10,))
   elif self.name in ('unit_owner_mismatch_B','unit_berserk_B','unit_deployed_B',
       'unit_deploying_B','unit_undeploying_B','unit_deployed_delayed_A'):
    fields={'unit_owner_mismatch_B':[(0xA83D4C,4,0)],
        'unit_berserk_B':[(self.actor+0x298,1,1)],
        'unit_deployed_B':[(self.actor+0x6E0,1,1)],
        'unit_deploying_B':[(self.actor+0x6E1,1,1)],
        'unit_undeploying_B':[(self.actor+0x6E2,1,1)],
        'unit_deployed_delayed_A':[(self.actor+0x6E0,1,1)]}[self.name]
    if self.name=='unit_deployed_delayed_A':self.click(a,dispatch=False)
    self.supply_fields(fields,bounds='Literal original-field component prior on stock MTNK; no deployment cycle, query, event, class, path or RNG answer supplied. The delayed control retains the actual emitted Move before changing6E0 and pumping it.')
    if self.name=='unit_deployed_delayed_A':self.pump()
    else:self.click(b)
   elif self.name=='archive_A_B':
    self.invoke('original_Archive_SetTarget_CellA_component_prior',0x70C610,self.actor,self.cell(*a))
    self.require(self.r(self.actor+0x218)==self.cell(*a),'Original Archive setter did not retain CellA')
    self.click(b)
   elif self.name=='attack_same_A':
    self.click(a)
    self.invoke('actual_QueueMission_Attack',0x5B35E0,self.actor,1,1)
    self.require(self.ints(self.actor+0xAC,1)[0]==1,'Original Attack producer did not commence')
    self.click(a)
   elif self.name=='prone_B':
    self.invoke('actual_GI_DoAction_Down',0x51D6F0,self.actor,5,1,0)
    self.require(self.u.mem_read(self.actor+0x6DB,1)[0]==1,'Original Down did not produce prone')
    self.click(b)
   elif self.name=='prone_A_A':
    self.click(a)
    self.invoke('actual_GI_DoAction_Down',0x51D6F0,self.actor,5,1,0)
    self.require(self.u.mem_read(self.actor+0x6DB,1)[0]==1,'Original Down did not produce prone')
    self.click(a)
   elif self.name in ('mode5_current_mismatch_B','mode5_current_nonhuman_Doing27_input_class_B',
       'nonDeployer_Doing27_B','berserk_B'):
    house=self.r(self.actor+0x21C);typ=self.r(self.actor+0x6C0)
    if self.name=='mode5_current_mismatch_B':priors=[(0xA8B238,4,5),(0xA83D4C,4,0)]
    elif self.name=='mode5_current_nonhuman_Doing27_input_class_B':
     priors=[(0xA8B238,4,5),(0xA83D4C,4,house),(house+0x1EC,1,0),(house+0x1ED,1,0)]
    elif self.name=='nonDeployer_Doing27_B':priors=[(typ+0xEC8,1,0)]
    else:priors=[(self.actor+0x298,1,1)]
    self.supply_fields(priors,bounds='Explicit original-field component priors; no native query, event, class or gameplay answer is supplied.')
    if 'Doing27' in self.name:
     self.invoke('actual_GI_DoAction_Deploy',0x51D6F0,self.actor,27,1,0)
     self.require(self.ints(self.actor+0x6C4,1)[0]==27,'Original Deploy did not produce Doing27')
    self.click(b)
    if self.name=='mode5_current_nonhuman_Doing27_input_class_B':
     self.invoke('actual_nonhuman_class_Deploy_Cell_boundary',0x51AA40,self.actor,self.cell(*b),1)
   elif self.name in ('human_Doing27_B','human_Doing27_shift_B','human_Doing27_class_B'):
    self.invoke('actual_GI_DoAction_Deploy',0x51D6F0,self.actor,27,1,0)
    self.require(self.ints(self.actor+0x6C4,1)[0]==27,'Original Deploy did not produce Doing27')
    self.require(self.u.mem_read(self.r(self.actor+0x21C)+0x1EC,1)[0]==1,'Human owner required')
    if self.name=='human_Doing27_class_B':
     self.invoke('actual_class_Deploy_Cell_boundary',0x51AA40,self.actor,self.cell(*b),1)
    else:self.click(b,keys=(0x10,) if self.name=='human_Doing27_shift_B' else ())
   elif self.name=='current_cell_raw20_B':
    self.click(a)
    x,y,_=self.ints(self.actor+0x9C,3);p=self.cell(x//256,y//256)
    before=self.cell_state(p)
    self.u.mem_write(p+0x124,dwords(0x20))
    self.events.append(dict(sequence=self.next_sequence(),kind='supplied_current_cell_raw_word',
        pointer=p,offset=0x124,value=0x20,before=before,after=self.cell_state(p),
        bounds='Explicit raw word contrast; membership remains native. No claim that a VERA cache or owner mask produced this word.'))
    self.click(b)
   elif self.name in ('findpath_existing_idle_MTNK','findpath_existing_GAPILE'):
    obj=self.f.source if self.name.endswith('MTNK') else self.producer
    x,y,_=self.ints(obj+0x9C,3);xy=(x//256,y//256)
    self.require(self.u.mem_read(obj+0x90,1)[0] and not self.u.mem_read(obj+0x81,1)[0],'Existing redirect object is not live')
    if self.name.endswith('MTNK'):
     interface=self.r(obj+0x674)
     self.require(interface and self.r(obj+0x5A4)==0,'Existing MTNK does not have NULL NavCom')
     moving=self.invoke('original_existing_MTNK_IsMoving',self.r(self.r(interface)+0x10),0,interface)
     self.require(not (moving&255),'Original existing MTNK IsMoving is true')
    typ=self.r(obj+(0x6C4 if self.name.endswith('MTNK') else 0x520))
    ambient=dict(object_arena_bytes=0x1000,object_raw=bytes(self.u.mem_read(obj,0x1000)).hex(),
        vtable=self.r(obj),owner=self.r(obj+0x21C),gi_owner=self.r(self.actor+0x21C),flags=self.r(obj+0x14),
        health=self.ints(obj+0x6C,1)[0],alive=self.u.mem_read(obj+0x90,1)[0],
        limbo=self.u.mem_read(obj+0x81,1)[0],marked=self.u.mem_read(obj+0x74,1)[0],
        on_bridge=self.u.mem_read(obj+0x8C,1)[0],logic_registered=self.u.mem_read(obj+0x98,1)[0],
        mission=self.ints(obj+0xAC,1)[0],queued=self.ints(obj+0xB4,1)[0],
        mission_status=self.ints(obj+0xBC,1)[0],mission_timer=self.ints(obj+0xC8,3),
        target=self.r(obj+0x2B4),archive=self.r(obj+0x218),
        type=dict(pointer=typ,name=bytes(self.u.mem_read(typ+0x24,25)).split(b'\0')[0].decode('latin1'),
            movement_zone=self.ints(typ+0x5B4,1)[0],speed_type=self.ints(typ+0x67C,1)[0],
            locomotor_guid=bytes(self.u.mem_read(typ+0x34C,16)).hex(),
            balloon_hover=self.u.mem_read(typ+0xD6A,1)[0],teleporter=self.u.mem_read(typ+0xCD4,1)[0],
            passengers=self.ints(typ+0x5E0,1)[0]))
    self.require(ambient['owner']==ambient['gi_owner'],'Existing redirect occupant owner differs from GI')
    if self.name.endswith('MTNK'):
     allocation=next(row for row in self.f.allocations if row['pointer']==interface-4)
     ambient['foot']=dict(nav=self.r(obj+0x5A4),aux_nav=self.r(obj+0x5A0),
         path=self.ints(obj+0x5E0,24),movement_timer=self.ints(obj+0x640,3),
         blocked_timer=self.ints(obj+0x668,3),
         locomotor=dict(interface=interface,allocation=allocation,
             raw=bytes(self.u.mem_read(allocation['pointer'],allocation['size'])).hex()))
    self.events.append(dict(sequence=self.next_sequence(),kind='existing_original_redirect_occupant',
        pointer=obj,location=self.ints(obj+0x9C,3),cell=self.cell_state(self.cell(*xy)),ambient=ambient))
    self.invoke('whole_original_FindPath_existing_occupant',0x4D3920,self.actor,
        xy[0]|(xy[1]<<16),0,0,count=100000000,wall=500000000)
    expected=0x4D3CC7 if self.name.endswith('MTNK') else 0x4D3DF9
    self.require(any(row.get('pc')==expected for row in self.events),'Actual FindPath redirect caller was not reached')
    self.require(any(row.get('pc')==0x56DC20 for row in self.events),'Original FNPC was not reached')
    if self.name.endswith('MTNK'):
     self.require(any(row.get('pc')==0x42D170 for row in self.events),'Original code6 path estimate was not reached')
   elif self.name in ('paid_head_B','paid_head_Stop_B','unit_paid_head_B',
       'unit_paid_head_Stop_B','unit_paid_head_same_A'):
    self.click(a)
    for i in range(100):
     self.frame(self.r(self.bc.FRAME)+1)
     self.invoke('whole_original_'+('Unit' if self.unit else 'GI')+'_AI_'+str(i+1),
         0x7360C0 if self.unit else 0x51BAB0,self.actor,count=100000000,wall=500000000)
     state=self.snapshot()
     private=state['drive' if self.unit else 'walk']
     moving=self.invoke('actual_Drive_IsMoving',0x4AFB80,0,self.loco+4)&255 if self.unit else private['moving']
     if moving and any(private['head']):break
    self.require(moving and any(private['head']),'No completed paid-head visit reached')
    self.require(not state['tether'] and not any(state['contacts']),'Paid-head visit retains producer contact')
    if self.name=='paid_head_Stop_B':self.invoke('actual_GI_Stop',0x51DAF0,self.actor)
    if self.name=='unit_paid_head_Stop_B':
     issued=[row for row in self.events if row.get('kind')=='Event_Construct'][-1]
     self.require(bytes.fromhex(issued['constructed_event_bytes'])[0]==4,'Original Move event missing')
     # Reuse the original Move caller's event arena/identity and the existing
     # original Stop4C65E0/4C6CB0 owner, not GI Stop or a MoveNULL surrogate.
     event=issued['this'];owner,uid,kind=issued['args'][:3]
     self.invoke('actual_StopEvent_constructor',0x4C65E0,event,owner,6,uid,kind&255)
     self.require(self.u.mem_read(event,1)[0]==6,'Original Stop opcode differs')
     self.invoke('actual_StopEvent_execute',0x4C6CB0,event)
    self.click(a if self.name=='unit_paid_head_same_A' else b)
    if self.unit:
     self.frame(self.r(self.bc.FRAME)+1)
     self.invoke('whole_original_Unit_AI_after_reissue',0x7360C0,self.actor,count=100000000,wall=500000000)
   else:raise ValueError('Unknown GI native history: '+self.name)
   self.require(not self.pending,'Unreturned recorded original calls')
   self.require(self.f.code_unchanged(),'Original executable code changed')
   self.require(all(bytes(self.u.mem_read(p,len(raw)))==raw for p,raw in self.vtables.items()),'Original class/Walk vtable changed')
  except Exception as exc:
   failure=dict(type=type(exc).__name__,message=str(exc),traceback=traceback.format_exc(),
       recent=[hex(pc) for pc in self.recent])
  finally:
   for hook in self.hooks:self.u.hook_del(hook)
  changed_cells=[dict(pointer=p,before=raw.hex(),after=bytes(self.u.mem_read(p,0x200)).hex())
      for p,raw in self.map_before.items() if bytes(self.u.mem_read(p,0x200))!=raw]
  map_raw=b''.join(self.map_before[p] for p in self.map_cells)
  result=dict(schema_version=1,status='PASS' if failure is None else 'FAIL',name=self.name,
      native_code_unchanged=self.f.code_unchanged(),original_gi_allocation=self.initial_actor_allocation,
      initial=self.initial,modifier_defaults=getattr(self,'defaults',None),
      inputs=self.inputs,boundaries=self.boundaries,ordered_calls=self.events,ordered_writes=self.writes,
      final=self.snapshot(),complete_rng_states=self.streams,
      requests=self.copy.deepcopy(self.f.draws[self.draw_start:]),
      raw_advances=self.copy.deepcopy(self.f.advances[self.advance_start:]),
      baseline_map=dict(cells=self.map_cells,bytes=len(map_raw),sha256=self.hashlib.sha256(map_raw).hexdigest(),raw=map_raw.hex()),
      changed_cells=changed_cells,pending_calls=self.pending,failure=failure)
  if self.unit:
   result['initial_target_prior']=self.target_prior
   result['original_actor_arena']=result.pop('original_gi_allocation')
   result['drive_field_sources']=dict(allocation_bytes=0x70,
       target_speed_fraction_bits=dict(offset=0x50,bytes=8,writer=0x4B3E00),
       straight=dict(offset=0x64,bytes=1,constructor_writer=0x4AF5B8),
       flag_65=dict(offset=0x65,bytes=1,constructor_writer=0x4AF5BB),
       dword_68=dict(offset=0x68,bytes=4,constructor_writer=0x4AF5BF),
       bounds='Concrete Drive base offsets. DWORD68 is raw observed state, not the target speed. Whole original112 bytes remain retained.')
   if hasattr(self,'resolver_map_prior'):result['resolver_map_prior']=self.resolver_map_prior
  return result


GI_HISTORIES=('plain_A_B','shift_A_B','other12_A_B','plain_A_A','plain_A_later_B',
    'paid_head_B','paid_head_Stop_B','attack_same_A','prone_B','prone_A_A','human_Doing27_B','human_Doing27_shift_B',
    'human_Doing27_class_B','current_cell_raw20_B','findpath_existing_idle_MTNK','findpath_existing_GAPILE',
    'archive_A_B','mode5_current_mismatch_B','mode5_current_nonhuman_Doing27_input_class_B',
    'nonDeployer_Doing27_B','berserk_B','unit_plain_A_B','unit_shift_A_B',
    'unit_plain_A_A','unit_shift_A_A','unit_plain_A_later_B','unit_shift_A_later_B',
    'unit_paid_head_B','unit_paid_head_Stop_B','unit_paid_head_same_A',
    'unit_owner_mismatch_B','unit_berserk_B','unit_deployed_B','unit_deploying_B',
    'unit_undeploying_B','unit_deployed_delayed_A','unit_shift_outside_playfield_B')

GI_EVIDENCE=Path(__file__).with_name('_walk_first_path')


def project_gi_reissue(result):
 """The single mechanical Rust-facing selector over a full native receipt.

 Full actor/Walk/Cell bytes and every repeated observer snapshot stay in the
 immutable compressed receipt. Keep structured top-level/class-RET8 states,
 all timer words, vector backing, ordered calls/writes and complete RNG buffers.
 Event111 bytes stay literal, including untouched constructor padding+0x0D;
 a semantic consumer compares the original decoded fields separately.
 """
 from tools.spatial_oracle._factory_infantry_output import runtime as rt
 rt.require(result.get('status')=='PASS' and result.get('history',{}).get('status')=='PASS',
     'Cannot project a failed native GI observation')
 def select(value):
  if isinstance(value,dict):
   if value.get('map_address')==0x87F7E8 and 'raw_header' in value:
    header=bytes.fromhex(value['raw_header'])
    value=dict(value,map_size=list(struct.unpack_from('<2i',header,0xF4)),
        local_size=list(struct.unpack_from('<4i',header,0xFC)))
   if 'actor_raw' in value and ('walk' in value or 'drive' in value):
    if 'drive' in value:
     # This literal exemption input remains in the immutable actor bytes;
     # export it for a bounded class consumer without naming its producer.
     actor_raw=bytes.fromhex(value['actor_raw'])
     value=dict(value,raw_techno_2b0=int.from_bytes(actor_raw[0x2B0:0x2B4],'little'))
    value={k:v for k,v in value.items() if k!='actor_raw'}
    family='drive' if 'drive' in value else 'walk'
    if family=='walk':value[family]={k:v for k,v in value[family].items() if k!='raw'}
    else:
     # Correct the first unregistered receipt's neutral50 label through the
     # same mechanical selector. No value is recomputed or supplied.
     private=dict(value['drive']);raw=bytes.fromhex(private['raw'])
     rt.require(len(raw)==0x70,'Native Drive allocation bytes differ')
     private.pop('accumulator_50',None)
     private.update(target_speed_fraction_bits=raw[0x50:0x58].hex(),
         straight=raw[0x64],flag_65=raw[0x65],dword_68=int.from_bytes(raw[0x68:0x6C],'little'))
     value['drive']=private
    value['cells']=[{k:v for k,v in cell.items() if k!='raw'} for cell in value['cells']]
   return {k:select(v) for k,v in value.items()}
  if isinstance(value,list):return [select(v) for v in value]
  return value
 history={k:v for k,v in result['history'].items() if k not in ('baseline_map','failure')}
 history['inputs']=[{k:v for k,v in row.items() if k not in ('before','after_query','after_click')}
     for row in history['inputs']]
 history['ordered_calls']=[{k:v for k,v in row.items()
     if row.get('kind') in ('GI_SetDestination','Unit_SetDestination') or k not in ('before','after','at_ret8','state')}
     for row in history['ordered_calls']]
 return dict(native_sha256=result['native_sha256'],bootstrap=result['bootstrap'],**select(history))


def _gi_read_pinned(row,*,decode=True):
 import gzip,hashlib
 from tools.spatial_oracle._factory_infantry_output import runtime as rt
 path=GI_EVIDENCE/row['path']
 rt.require(path.resolve().is_relative_to(GI_EVIDENCE.resolve()),'Native evidence path outside owner')
 raw=path.read_bytes()
 rt.require(len(raw)==row['bytes'] and hashlib.sha256(raw).hexdigest()==row['sha256'],
     'Retained GI evidence changed: '+row['path'])
 plain=gzip.decompress(raw) if path.suffix=='.gz' else raw
 if 'uncompressed_sha256' in row:
  rt.require(len(plain)==row['uncompressed_bytes'] and
      hashlib.sha256(plain).hexdigest()==row['uncompressed_sha256'],
      'Decompressed GI evidence changed: '+row['path'])
 return json.loads(plain) if decode else plain


def saved_gi_reissues():
 """Check sealed raw/source identities and reselect saved native observations.

 This is a saved-evidence check. --check-gi-reissue executes a selected whole
 history afresh; the normal corpus check still executes the original 7+3 rows.
 """
 from tools.spatial_oracle._factory_infantry_output import runtime as rt
 meta=json.loads((GI_EVIDENCE/'manifest.json').read_text())
 rt.verify_helpers()
 for files in meta['source_boundaries'].values():
  for row in files.values():_gi_read_pinned(row,decode=False)
 for row in meta['compatibility_receipts'].values():_gi_read_pinned(row)
 for row in meta.get('original_instruction_receipts',[]):_gi_read_pinned(row)
 for row in meta.get('failed_observations',[]):
  rt.require(_gi_read_pinned(row).get('status')=='FAIL','Retained failed native observation was relabeled')
 output=[]
 for row in meta['histories']:
  original=_gi_read_pinned(row)
  source=meta['source_boundaries'][row['source_boundary']]
  for path,digest in original['source_sha256'].items():
   rt.require(source[path]['uncompressed_sha256']==digest,'As-run GI source binding differs: '+path)
  selected=project_gi_reissue(original)
  rt.require(selected['name']==row['name'] and rt.canonical_sha(selected)==row['projected_sha256'],
      'Mechanical GI selection differs: '+row['name'])
  output.append(selected)
 return output


def corpus():
 return dict(**generate(),gi_reissue=saved_gi_reissues())


def gi_evidence_identity():
 import hashlib
 raw=(GI_EVIDENCE/'manifest.json').read_bytes();meta=json.loads(raw)
 return dict(manifest='_walk_first_path/manifest.json',sha256=hashlib.sha256(raw).hexdigest(),
     histories=[row['name'] for row in meta['histories']],
     selector='walk_first_path.project_gi_reissue',
     check_scope='Fresh legacy7setter/3head plus saved raw/source-bound GI selection; selected whole-history replay is --check-gi-reissue',
     bounds=meta['bounds'])


def gi_reissue(name,assets,*,candidate=False):
 """Fresh existing corrected bootstrap; no object/map/RNG initializer copy."""
 import hashlib
 from tools.spatial_oracle._factory_infantry_output import runtime as rt, initialized
 rt.configure_assets(assets)
 rt.require(name in GI_HISTORIES,'Unknown additive GI history')
 def run():
  return initialized.generate('no_rally',live_continuation=lambda f,final:GIMoveHistory(f,final,name).execute(),
      drive_startup=name.startswith('unit_'))
 if candidate:
  with rt.candidate_helper_profile():original=run()
 else:original=run()
 if original['status']!='PASS':return original
 identity=rt.metadata()['initialized_controls']['no_rally']
 row=original['live_continuation']
 result=dict(schema_version=1,status=row['status'],native_sha256=original['native_sha256'],
     shared_helpers=original['shared_helpers'],
     bootstrap=dict(control='no_rally',complete_primary_native_equal=True,complete_private_native_equal=True,
         complete_primary_native_sha256=identity['complete_primary_sha256'],
         complete_private_native_sha256=identity['complete_private_sha256'],
         original_projection=identity['projection'],final_frame=identity['final_frame']),
     source_sha256={str(Path(p).relative_to(rt.REPO_ROOT)):hashlib.sha256(Path(p).read_bytes()).hexdigest()
         for p in (__file__,initialized.__file__,__import__('tools.spatial_oracle.walk_move_admission',fromlist=['x']).__file__)},
     history=row,bounds=['The existing corrected stock GAPILE/two-E1 produced bootstrap is compared completely before continuation.',
         'Authored click Cells20,20/21,20 and explicit OS key words enter actual Infantry+70/+140; caller auxiliary arguments are0.',
         'Options4E7E20/5FA350 defaults execute. GetKeyState at53EC9A and clicked Event timeGetTime646F20 are OS transport seams only.',
         'Original Cell resolver, TargetClass/event111 construction, OutList append, local transfer/DoList dispatch and class51AA40 execute.',
         'Supplied local scheduling/absolute frames and original selected GI AI only; no display hit test, HWND, complete input/Planning Mode, whole-world/network/save-load parity claim.',
         'Every class boundary records actual51B1DE RET8 and12-byte entry-to-caller cleanup; setter incidental EAX is not an acceptance result.',
         'All Main/Scenario/Mapgen0x3F4 states, requests/advances, all timer words and ordered actor/Walk/Cell/vector/ring writes are retained.'])
 if name.startswith('unit_'):
  result['bootstrap']['original_registered_drive_startup']=original['original_registered_drive_startup']
  result['bounds']=list(original['bounds'])+['Selected native source MTNK, supplied fixture arena; inherited prior Drive is excluded. Whole Unit query/Cell resolver/Move Event/class, original Drive14 and all three streams retained. Unit setter EAX is incidental; actual RET8 and native state establish the boundary.']
 return result


if __name__=='__main__':
 if '--project-gi-reissue' in sys.argv:
  import argparse,gzip
  from tools.spatial_oracle._factory_infantry_output import runtime as rt
  parser=argparse.ArgumentParser(description='Select a successful original GI receipt without executing native code')
  parser.add_argument('--project-gi-reissue',type=Path,required=True)
  parser.add_argument('--output',type=Path,required=True)
  args=parser.parse_args()
  raw=args.project_gi_reissue.read_bytes()
  result=json.loads(gzip.decompress(raw) if args.project_gi_reissue.suffix=='.gz' else raw)
  selected=project_gi_reissue(result);rt.write_new(args.output,selected)
  print(json.dumps(dict(status='PASS',history=selected['name'],output=str(args.output))))
  sys.exit(0)
 if '--gi-reissue' in sys.argv or '--check-gi-reissue' in sys.argv:
  import argparse
  from tools.spatial_oracle._factory_infantry_output import runtime as rt
  parser=argparse.ArgumentParser(description='Original produced GI input/event/reissue continuation')
  action=parser.add_mutually_exclusive_group(required=True)
  action.add_argument('--gi-reissue',choices=GI_HISTORIES)
  action.add_argument('--check-gi-reissue',choices=GI_HISTORIES)
  parser.add_argument('--assets',type=Path,required=True)
  parser.add_argument('--output',type=Path,required=True)
  parser.add_argument('--candidate-helper-profile',action='store_true',help='Explicit unregistered comparison-only helper map')
  args=parser.parse_args()
  name=args.gi_reissue or args.check_gi_reissue
  rt.require(not args.output.exists(),'Refuse to overwrite native receipt')
  try:
   result=gi_reissue(name,args.assets,candidate=args.candidate_helper_profile)
   if args.check_gi_reissue and result.get('status')=='PASS':
    from tools.native_oracle import first_difference
    expected=[row for row in saved_gi_reissues() if row['name']==name]
    rt.require(len(expected)==1,'Selected GI history has no accepted golden: '+name)
    difference=first_difference(expected[0],project_gi_reissue(result))
    result['comparison']=dict(status='PASS' if difference is None else 'FAIL',difference=difference,
        scope='Every mechanically selected field; full literal event bytes and all three RNG buffers included')
    if difference is not None:result['status']='FAIL'
  except Exception as exc:
   import traceback
   failure=dict(type=type(exc).__name__,message=str(exc),traceback=traceback.format_exc())
   if 'result' in locals():result.update(status='FAIL',comparison_failure=failure)
   else:result=dict(status='FAIL',name=name,failure=failure)
  rt.write_new(args.output,result)
  print(json.dumps(dict(status=result['status'],history=name,output=str(args.output))))
  sys.exit(0 if result['status']=='PASS' else 1)
 finish_vectors(corpus,Path(__file__).with_suffix('.json'),provenance=lambda:dict(provenance(
  scope='Original Infantry51AA40 nonnull Cell setter, Foot4D94B0, Walk75ACB0, then original WalkProcess through first FindPath4D3920 entry or already-paid-head motion-byte producer. Separate actual75C240 head rows contrast Mission1/TarCom/CellNavCom with the existing no-target producer. No pathfinder-core/whole Process parity.',
  entry_points={'infantry_set_destination':0x51AA40,'foot_set_destination':0x4D94B0,'walk_constructor':0x75AA90,'walk_move_to':0x75ACB0,'walk_process':0x75AEC0,'first_find_path':0x4D3920,'head_producer':0x75C240,'head_choice':0x481180,'raw_mark':0x5217C0,'raw_clear':0x521850},
  assumptions=['Supplied original Infantry/Unit/Cell tables, valid ordinary non-Jumpjet owner/type/House, no links/radio/bunker/transport, currentXYZ2624,2624,0, destination Cell11,10. Human Doing27 refusal contrast and nonhuman acceptance execute original gate.', 'Existing path backing2,3,4,5 and null ownerNavCom are supplied; actual setter clears one path head and publishes Cell plus Walk destination. Optional paid head is supplied and remains independently owned.', 'Actual Walk constructor executes; reference count1 models the already-owned ILoco interface. Original setter obtains/releases a temporary interface reference. OS InterlockedIncrement/Decrement imports emulate their one-integer operations, no gameplay callable substituted.', 'First no-head Process is stopped at original FindPath entry after observing arguments and unchanged path suffix/reference; it does not execute the AStar owner, path writes, Mark or movement. Paid-head row stops after actual+36 producer.', 'Frame100, initial logical Foot timer start+640=50/duration+648=5 (+644 supplied padding0), Rules+1768=22 and Rules+1760 double0.0 (retry_delay7.75 contrast) are supplied. Actual accepted Foot setter resets this timer before first Process; the later Process writes its retry delay before FindPath. Scope is the first request after an accepted setter, not arbitrary Process with a live retry timer. Flat Cell levels, startup control0E7F and known104 level constant are supplied.', 'All three supplied free-head rows preserve the full RandomClass byte-for-byte (zero draws); this is not a general head-choice RNG equivalence claim. Separate head rows use the imported walk_head_occupation fixture: supplied current2496,2624,260 and requested2752,2624,260, level2/slope1, subcell-offset table, no gate/crate/slave. Original75C240/481180/5217C0/521850 and seededScenarioRandom execute; full0x3F4 RandomClass state retained before/after. These are separate kernel rows, not execution through the FindPath core.'],
  substitutions=['Setter rows only: OS InterlockedIncrement/Decrement imports update the pointed count and return it with original stdcall cleanup. No SetDestination, MoveTo, GetCoords, House, Mission or Process callable replaced.', 'Separate head rows inherit explicit owner-index/no-building/gate/false-owner virtual seams from walk_head_occupation; Mission+184 returns supplied0 or1, TarCom and CellNavCom pointers are supplied. The actual75C240 caller decides whether to consult them.']),
  gi_reissue_evidence=gi_evidence_identity()))
