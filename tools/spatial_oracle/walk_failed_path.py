"""Original failed Walk search consequences and bounded Infantry Move restart.

Run python -m tools.spatial_oracle.walk_failed_path --check (or --write).
FindPath failure is supplied in18rows; one executes actual raw-zone rejection,
and four execute the full wrapper with only the AStar result supplied NULL;
original caller, setters, timers, map predicates and
Stop execute. Restart rows continue at original520F40 and stop BEFORE520FAF:
no intervening InfantryAI or downstream Doing/zone logic is emulated here.
"""
from pathlib import Path
import struct
from unicorn import Uc,UC_ARCH_X86,UC_MODE_32,UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_EIP,UC_X86_REG_ESP,UC_X86_REG_FPCW
from tools.native_oracle import load_image,run_checked,STACK_BASE,STACK_SIZE,SCRATCH,RET_MAGIC,finish_vectors,provenance
from tools.spatial_oracle.map_queries import dwords,packed

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
 if row.get('astar_null'):
  u.mem_write(0x89EA40,struct.pack('<90f',*([1.0]*90)))
  u.mem_write(TYPE+0xE3C,dwords(SCRATCH+0x1D000));u.mem_write(ACTOR+0x6C,dwords(100))
  for action in range(42):u.mem_write(SCRATCH+0x1D000+action*36+4,dwords(6))
  call(0x49F2F0,0,[])
 u.mem_write(ACTOR+0x6C0,dwords(TYPE));u.mem_write(ACTOR+0x6C4,dwords(row.get('doing',0)))
 u.mem_write(ACTOR+0xAC,dwords(row.get('mission',1)));u.mem_write(ACTOR+0xB4,dwords(row.get('queued_mission',-1)))
 u.mem_write(ACTOR+0x90,b'\x01');u.mem_write(ACTOR+0x9C,dwords(10*256+64,10*256+64,0))
 u.mem_write(ACTOR+0x2B4,dwords(TARGET if row.get('target',True) else 0));u.mem_write(TARGET+0x14,dwords(4 if row.get('foot',True) else 0))
 u.mem_write(TARGET+0x9C,dwords(*row.get('target_xyz',[11*256+220,10*256+220,123])))
 u.mem_write(ACTOR+0x5A0,dwords(123));u.mem_write(ACTOR+0x5A4,dwords(TARGET))
 u.mem_write(ACTOR+0x558,packed(9,8));u.mem_write(ACTOR+0x5E0,dwords(2,3,4,5));u.mem_write(ACTOR+0x598,dwords(row.get('nav_queue',0)))
 u.mem_write(ACTOR+0x58C,dwords(SCRATCH+0x1A000));u.mem_write(SCRATCH+0x1A000,dwords(TARGET,CELL))
 u.mem_write(ACTOR+0x6B7,b'\x01');u.mem_write(ACTOR+0x640,dwords(50,0,5));u.mem_write(ACTOR+0x668,dwords(40,0,6))
 u.mem_write(0xA8ED84,dwords(100));u.mem_write(0x8871E0,dwords(RULES));u.mem_write(RULES+0x1768,dwords(22))
 u.mem_write(ACTOR+0x418,bytes([row.get('close_gate',False)]));u.mem_write(ACTOR+0x684,bytes([255]));u.mem_write(ACTOR+0x64C,dwords(row.get('retries',10)));u.mem_write(ACTOR+0x3D5,bytes([row.get('in_playfield',True)]));u.mem_write(TYPE+0x5B4,dwords(4));u.mem_write(RULES+0x1718,dwords(row.get('close_enough',128)))
 u.mem_write(RULES+0x1760,struct.pack('<d',row.get('retry_delay',0.01)))
 u.mem_write(DUMMY+0x24,packed(99,98));u.mem_write(0xB45BE8,dwords(0,0,0));u.mem_write(0xB45C28,dwords(104))
 call(0x75AA90,LOCO,[]);u.mem_write(LOCO+0xC,dwords(ACTOR));u.mem_write(ACTOR+0x674,dwords(LOCO+4))
 u.mem_write(MAP+0xF4,dwords(8,8,0,0,8,8));u.mem_write(MAP+0x68,dwords(SCRATCH+0x1B000,289));u.mem_write(SCRATCH+0x1B000,bytes(289*4));u.mem_write(SCRATCH+0x1B000+(10*17+11)*4+2,packed(int(row.get('different_zone',False)),0));u.mem_write(MAP+0x28,dwords(SCRATCH+0x1C000));u.mem_write(SCRATCH+0x1C000,packed(1,2))
 table=bytearray(0x100000)
 for i,(x,y) in enumerate([(10,10),(row.get('destination_x',11),10)]):
  cell=CELL+i*0x200;struct.pack_into('<I',table,(y*512+x)*4,cell);u.mem_write(cell,dwords(CVT));u.mem_write(cell+0x24,packed(x,y));u.mem_write(cell+0x44,dwords(0xffffffff))
 u.mem_write(CVT,bytes(u.mem_read(0x7E4EEC,0x100)));u.mem_write(CVT+0x48,dwords(0x486840));u.mem_write(TABLE,bytes(table));u.mem_write(MAP+0x13C,dwords(TABLE,0x40000));u.mem_write(0x87F924,dwords(TABLE))
 # Original674000: Water is land2; Foot occupies float0 in each9-float row.
 # Retail rulesmd[Water]Foot=0%; keep the current-cell contrast explicit.
 if row.get('current_water'):
  u.mem_write(CELL+0xEC,dwords(2))
  u.mem_write(0x89EA40+18*4,struct.pack('<f',0.0))
 events=[]
 recent=[]
 def observer(_u,address,_size,_data):
  sp=u.reg_read(UC_X86_REG_ESP)
  recent.append(hex(address))
  if row.get('astar_null'):
   if address in [0x51DAF0,0x51D6F0,0x4D55C0,0x4D3E11,0x4D3EAC,0x4D4016,0x4D4044,0x4D413A,0x4D4150,0x4D41D6,0x4D41E5]:events.append(hex(address))
   if address in [0x4D41E5,0x4D41C8]:events.append(['find_path_failure_return',state()])
  if SCRATCH<=address<SCRATCH+0x20000:raise RuntimeError(recent[-20:])
  if address in [read32(0x7E11C8),read32(0x7E11CC)]:
   p=read32(sp+4);value=read32(p)+(1 if address==read32(0x7E11C8) else -1);u.mem_write(p,dwords(value));ret(4,value)
  elif address==0x4CBBA0 and row.get('astar_null'):
   events.append(['astar_null',read32(0xA8ED84),list(struct.unpack('<IIIIII',u.mem_read(sp+4,24)))]);ret(24,0)
  elif address==0x4D3920 and not row.get('native_find_path'):
   events.append(['find_path_false',read32(0xA8ED84),list(struct.unpack('<III',u.mem_read(sp+4,12)))]);ret(12,0)
  elif address in [0x520F40,0x520F72,0x520F97,0x520FA9,0x75B03B,0x75B06C,0x75B085,0x75B2BC,0x75B580,0x4D3810,0x521DD0,0x4DC030,0x56D100,0x4D3710,0x4D9FF0,0x41BDD0,0x5F65A0,0x6F7970,0x565730,0x6F77B0,0x5B3040,0x51AA40,0x51AD11,0x4D94B0,0x75ADA0,0x75ACB0,0x4D3920,0x521B40,0x4D896E]:events.append(hex(address))
 u.hook_add(UC_HOOK_CODE,observer)
 u.mem_write(ACTOR+0x5A4,dwords(0));u.mem_write(LOCO+0x14,dwords(1));u.mem_write(LOCO+0x34,b'\0');u.mem_write(LOCO+0x1C,dwords(0,0,0))
 initial_timer=dict(start_frame=read32(ACTOR+0x640),duration=read32(ACTOR+0x648))
 call(0x51AA40,ACTOR,[CELL+0x200,1])
 def base_state():return dict(xyz=list(struct.unpack('<iii',u.mem_read(ACTOR+0x9C,12))),speed=struct.unpack("<d",u.mem_read(ACTOR+0x578,8))[0],retries=read32(ACTOR+0x64C),frame=read32(0xA8ED84),movement_timer=dict(start_frame=read32(ACTOR+0x640),duration=read32(ACTOR+0x648)),reference=list(struct.unpack('<hh',u.mem_read(ACTOR+0x558,4))),nav_queue_count=read32(ACTOR+0x598),nav_queue_entries=list(struct.unpack('<II',u.mem_read(SCRATCH+0x1A000,8))),queue=list(struct.unpack('<iiii',u.mem_read(ACTOR+0x5E0,16))),head=list(struct.unpack('<iii',u.mem_read(LOCO+0x28,12))),destination=list(struct.unpack('<iii',u.mem_read(LOCO+0x1C,12))),nav=read32(ACTOR+0x5A4),moving=u.mem_read(LOCO+0x34,1)[0],motion=u.mem_read(LOCO+0x36,1)[0])
 def state():
  result=base_state()
  if row.get('astar_null'):result.update(marked=u.mem_read(ACTOR+0x74,1)[0],entry_state=u.mem_read(ACTOR+0x6DC,1)[0],target=read32(ACTOR+0x2B4),action_frame=read32(ACTOR+0xF8),action_timer_start=read32(ACTOR+0x100),action_timer_duration=read32(ACTOR+0x108),doing=read32(ACTOR+0x6C4),mission=read32(ACTOR+0xAC),queued_mission=read32(ACTOR+0xB4))
  return result
 before=state()
 phases=[]
 for frame in row.get('frames',[100,108,109]):
  u.mem_write(0xA8ED84,dwords(frame))
  events.clear()
  call(0x75AEC0,LOCO,[0])
  phase=dict(state=state(),events=list(events))
  if row.get('restart'):
   events.clear()
   sp=STACK_BASE+STACK_SIZE-0x1000;u.mem_write(sp,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,ACTOR)
   run_checked(u,0x520F40,0x520FAF,count=100000,required_addresses=[0x520F40])
   phase['restart']=dict(state=state(),events=list(events))
  phases.append(phase)
 return dict(input=row,initial_movement_timer=initial_timer,setter=before,phases=phases)

def generate():
 rows=[dict(mission=2,target=False,retries=n) for n in (10,1,0,256,0xffffffff)]
 rows += [dict(mission=2,target=False,retries=10,close_enough=n) for n in (325,326,327,512)]
 rows += [dict(mission=2,target=False,retries=0,in_playfield=False),dict(mission=2,target=False,retries=10,close_enough=512,close_gate=True),dict(mission=2,target=False,retries=10,different_zone=True),dict(mission=15,target=False,retries=10)]
 rows += [dict(mission=2,target=False,retries=n,restart=True,frames=[100,101,102]) for n in (10,1,0)]
 rows += [dict(mission=-1,queued_mission=2,target=False,retries=1,restart=True,frames=[100,101,102]),dict(mission=1,target=False,retries=10,restart=True,frames=[100,101,102])]
 rows += [dict(mission=2,target=False,retries=10,different_zone=True,native_find_path=True)]
 rows += [dict(mission=2,target=False,retries=10,native_find_path=True,astar_null=True,destination_x=x,frames=[100]) for x in (11,13)]
 rows += [dict(mission=2,target=False,retries=10,native_find_path=True,astar_null=True,destination_x=x,frames=[100],current_water=True) for x in (11,13)]
 return [query(row) for row in rows]

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=provenance(
  scope='Original Infantry51AA40 accepted Cell setter then full Walk75AEC0: 18 supplied FindPath failure rows, one actual FindPath4D3920 raw-zone rejection row and four original-wrapper/AStar-NULL-result rows (Clear/Water current-cell contrasts); failure/cancellation consequences and connected520F40..520FAF effective-Move restart prefix. No AStar search failure, full InfantryAI/Doing, successful movement or Rust parity claim.',
  entry_points={'setter':0x51AA40,'foot_setter':0x4D94B0,'walk_constructor':0x75AA90,'process':0x75AEC0,'find_path':0x4D3920,'astar_result_seam':0x4CBBA0,'startup_directions':0x49F2F0,'infantry_failure_receiver':0x51DAF0,'foot_zone_predicate':0x4D3810,'can_reach_zone':0x56D100,'failure_receiver':0x521DD0,'mission_receiver':0x4DC030,'stop':0x75ADA0,'restart_prefix':0x520F40,'restart_endpoint':0x520FAF},
  assumptions=[
   'Fixture derived from walk_first_path. Original Infantry/Unit/Cell vtable bytes copied; Cell+48 explicitly selects original486840. Supplied ordinary non-Jumpjet Infantry/House/type, human Doing0, alive, no transport/bunker/radio/links, Type+D94false, Type MovementZone4, no TarCom, no paid head. All unused actor/type storage is zero. Foot+684 byte255 matches Foot constructor4D338F and selects ordinary4DBDF0 ILoco destination coordinates; Foot+6B7 supplied true as in accepted setter fixture.',
   'CurrentXYZ2624,2624,0; destination Cell11,10 has original Cell center2944,2688,0. Two ordinary flat cells; Cell+44=-1, bridge flags0, original Cell vtable,104 level constant and FPCW0E7F. Map width8/height8/bounds0,0,8,8, raw path-group records289 and zone4 lookup[1,2]. Default current/destination group0; different_zone assigns destination group1. Original56D100/56D230 execute; no map loading, zone rebuild or pathfinding-core proof.',
   'Initial FootNavComNULL; backing path2,3,4,5/reference9,8 and NavQueue count0/storage TARGET,CELL supplied. Actual setter clears first path word. Actual Walk constructor and referencecount1 model owned ILoco. Initial logical movement timer50/5 and blocked timer40/6; frame100, Rules+1768=22, PathDelay double0.01, CloseEnough128 with325/326/327/512 contrasts. Retry counter dword10/1/0/256/FFFFFFFF supplied before setter; no counter-lifetime/domain/load claim beyond these inputs. Raw actor+418 gate false with true contrast; raw+3D5 true with false contrast; +68Afalse avoids exhausted-path sound branch.',
   'The astar_null rows initially supply all90 land-speed floats1.0 at89EA40 (flat LandType0/SpeedType0 clear-cell premise), all42 Infantry sequence counts6 at Type+E3C and health100, and execute original direction initializer49F2F0. Other original fixture state is retained, including Object+74/dirty+80/type+235zero, no Team/TarCom and ordinary human/MZ4. Near Cell11,10 and far Cell13,10 are same-zone flat destinations, each allocated alongside currentCell10,10. Original Mark0/1, Infantry+500=51DAF0, DoAction, current-cell CanEnter, WalkStop, NULL setters and human queued-Guard callback execute. Capture full state immediately before original FindPath failure return and again after outer Process. No marked-object occupancy/redraw lifetime, adjacent-bridge/actorOnBridge contrast, nonhuman/Team failure, full AStar core or generic mission/action proof.',
   'Two added current_water rows change only current Cell+EC to LandType2 and its Foot speed at89EA40+18*4 to0.0, matching active rulesmd[Water]Foot=0%. Original674000 uses land-name table839D68, whose third pointer81BAE8 is Water; Foot key81DBD4 writes float0 in each24h-byte land record. Original CanEnter51C7AB..51C7D3 indexes Cell+EC and Type+67C and returns7 for this zero speed; original51DAF0 therefore retains Infantry+6DC=1 rather than the Clear controls0. Near/far XYZ, cancellation and queued-Guard results are captured unchanged. Supplied current terrain does not prove bridge-collapse publication, marked-object lifetime or the later520FF1 consumer.',
   'Non-restart rows run Process at frames100,108,109, except astar_null rows use frame100 only. They preserve resulting FootNavCom separately from cleared Walk destination; later direct Process calls are not complete game turns. Mission15/noTarCom contrast executes the original failure callback cancellation; no universal mission claim.',
   'Restart rows run failed Process then original520F40 until520FAF in the SAME fixture, advancing frames100/101/102. Effective Move2 (including currentNONE/queuedMove2) reaches original same-pointer Infantry/Foot setter and speed1.0; mission1 contrast skips the prefix. No gameplay state injected between Process and prefix. Real caller order: Infantry51BC9F calls Foot4DA530 with Process4DA877, then surviving51BF7B calls520F40. Intervening shooting/animation/alive gates and520FAF onward remain outside this bounded continuation. No unconditional retry or full Doing delivery claim.'
  ],
  substitutions=[
   'In the first 18 rows FindPath4D3920 returns EAX0 with original stdcall12 cleanup, leaving actor/path/map state unchanged. The native_find_path/different_zone row executes original4D3920: its raw-zone predicate rejects before AStar, then Process rechecks the zone and cancels through the original NULL setter. That row has no gameplay callable substitution; it proves a supplied raw-zone disconnect, not a bridge-collapse producer, AStar failure or its side effects. Every reached caller/zone/receiver/distance/timer/Stop/restart setter remains original.',
   'In the four astar_null rows only4CBBA0 returns EAX0 with original stdcall24 cleanup and no path/core writes. Actual4D3920 wrapper failure effects therefore precede the original Process failure continuation. This is a supplied NULL core-result boundary, not proof of an AStar search producingNULL. It deliberately demonstrates why the first18 caller-only rows cannot model complete original FindPath failure: +500 always Stops Walk before returning, so the caller retained destination pointer now observes zero and cancelsNavCom. The far wrapper also clears target and queuesGuard5 before returning; current mission2 is preserved.',
   'OS InterlockedIncrement/Decrement IAT imports update one pointed count and return it with original stdcall4 cleanup; no other callable substituted beyond the explicitly row-selected path/core seams. Restart prefix ends at520FAF before its next instruction; each subsequent Process gets a fresh ABI call frame.'
  ]))
