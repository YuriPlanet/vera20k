"""Original Foot PerCell range-stop caller and actual Infantry null setter.

SelectWeapon/GetWeapon identity and final InRange boolean are supplied seams;
target coordinate conversion, range source, mission/queue gates, the Infantry
setter, Foot setter and Walk Stop execute original instructions. This is not
whole WalkProcess or full InRange parity.
"""
from pathlib import Path
import struct,json,sys
from unicorn import Uc,UC_ARCH_X86,UC_MODE_32,UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EBP,UC_X86_REG_EBX,UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_EDX,UC_X86_REG_ESI,UC_X86_REG_EIP,UC_X86_REG_ESP,UC_X86_REG_FPCW
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
 u.mem_write(ACTOR+0x6C0,dwords(TYPE));u.mem_write(ACTOR+0x6C4,dwords(row.get('doing',0)))
 u.mem_write(ACTOR+0xAC,dwords(row.get('mission',1)));u.mem_write(ACTOR+0xB4,dwords(row.get('queued_mission',-1)))
 u.mem_write(ACTOR+0x90,b'\x01');u.mem_write(ACTOR+0x9C,dwords(10*256+64,10*256+64,0))
 u.mem_write(ACTOR+0x2B4,dwords(TARGET if row.get('target',True) else 0));u.mem_write(TARGET+0x14,dwords(4 if row.get('foot',True) else 0))
 u.mem_write(TARGET+0x9C,dwords(*row.get('target_xyz',[11*256+220,10*256+220,123])))
 u.mem_write(ACTOR+0x5A0,dwords(123));u.mem_write(ACTOR+0x5A4,dwords(TARGET))
 u.mem_write(ACTOR+0x5E0,dwords(2,3,4,5));u.mem_write(ACTOR+0x598,dwords(row.get('nav_queue',0)))
 u.mem_write(ACTOR+0x58C,dwords(SCRATCH+0x1A000));u.mem_write(SCRATCH+0x1A000,dwords(TARGET,CELL))
 if row.get('contact'):
  u.mem_write(ACTOR+0xE4,dwords(SCRATCH+0x1A100));u.mem_write(ACTOR+0xE8,dwords(1));u.mem_write(SCRATCH+0x1A100,dwords(TARGET))
 u.mem_write(ACTOR+0x6B7,b'\x01');u.mem_write(ACTOR+0x640,dwords(50,0,5));u.mem_write(ACTOR+0x668,dwords(40,0,6))
 u.mem_write(0xA8ED84,dwords(100));u.mem_write(0x8871E0,dwords(RULES));u.mem_write(RULES+0x1768,dwords(22))
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
 u.mem_write(CVT+0x48,dwords(0x486840));u.mem_write(TABLE,bytes(table));u.mem_write(MAP+0x13C,dwords(TABLE,0x40000));u.mem_write(0x87F924,dwords(TABLE))
 u.mem_write(WEAPON_SLOT,dwords(WEAPON));u.mem_write(WEAPON+0x134,bytes([int(row.get('cell_rangefinding',False))]))
 events=[]
 def observer(_u,address,_size,_data):
  sp=u.reg_read(UC_X86_REG_ESP)
  if address==read32(VT+0x2E4):
   assert read32(sp+4)==(TARGET if row.get('target',True) else 0);events.append(['select',bool(read32(sp+4))]);ret(4,row.get('weapon_index',1))
  elif address==read32(VT+0x3F8):events.append(['weapon',read32(sp+4)]);ret(4,WEAPON_SLOT)
  elif address==0x6F7220:
   p=read32(sp+4);target=read32(sp+8);w=read32(sp+12);assert w==WEAPON
   events.append(['range',list(struct.unpack('<iii',u.mem_read(p,12))),list(struct.unpack('<hh',u.mem_read(target+0x24,4))),target==DUMMY]);ret(12,int(row.get('in_range',True)))
  elif address in [0x4D9FF0,0x41BDD0,0x5F65A0,0x6F7970,0x565730,0x6F77B0,0x5B3040,0x51AA40,0x51AD11,0x4D94B0,0x75ADA0,0x521B40,0x4D896E]:events.append(hex(address))
 u.hook_add(UC_HOOK_CODE,observer)
 if row.get('setter_only'):call(0x51AA40,ACTOR,[0,1])
 else:
  sp=STACK_BASE+STACK_SIZE-0x1000;u.mem_write(sp,b'\0'*0x80);u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ESI,ACTOR)
  run_checked(u,0x4D882F,0x4D8978,count=100000,required_addresses=[0x4D882F,0x4D8916]);assert u.reg_read(UC_X86_REG_ESP)==sp
 return {'input':row,'events':events,'nav':[read32(ACTOR+0x5A0),read32(ACTOR+0x5A4)],'queue':list(struct.unpack('<iiii',u.mem_read(ACTOR+0x5E0,16))),'destination':list(struct.unpack('<iii',u.mem_read(LOCO+0x1C,12))),'head':list(struct.unpack('<iii',u.mem_read(LOCO+0x28,12))),'moving':u.mem_read(LOCO+0x34,1)[0],'animation_moving':u.mem_read(LOCO+0x36,1)[0],'produced_animation_moving':produced_motion,'nav_queue_count':read32(ACTOR+0x598),'nav_queue_entries':list(struct.unpack('<II',u.mem_read(SCRATCH+0x1A000,8))),'blocked':u.mem_read(ACTOR+0x6B7,1)[0],'movement_timer':list(struct.unpack('<iii',u.mem_read(ACTOR+0x640,12))),'blocked_timer':list(struct.unpack('<iii',u.mem_read(ACTOR+0x668,12))), 'dummy':list(struct.unpack('<hh',u.mem_read(DUMMY+0x24,4)))}

def generate():
 rows=[{}, {'setter_only':True,'head':True},{'setter_only':True},
       {'in_range':False},{'nav_queue':1},{'mission':5},{'mission':11},
       {'mission':15},{'mission':21},{'mission':-1,'queued_mission':1},
       {'doing':27},{'doing':27,'human':False},
       {'foot':False},{'target':False},{'cell_rangefinding':True},
       {'target_xyz':[-257,2780,123],'mission':5}, {'weapon_index':0},
       {'process_motion':True},
       {'setter_only':True,'head':True,'process_motion':True,'nav_queue':1},
       {'setter_only':True,'mission':7,'process_motion':True},
       {'setter_only':True,'mission':7,'contact':True,'process_motion':True}]
 return [query(row) for row in rows]

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
  scope='FootPerCell mode2 interior range-stop caller through actual Infantry null SetDestination, Foot null setter and Walk Stop; coordinate/source and mission/NavQueue order. Final InRange boolean and original-target weapon choice are supplied, not full range/selection parity.',
  entry_points={'percell_range_block':0x4D882F,'percell_range_end':0x4D8978,'target_coordinate':0x4D9FF0,'coordinate_cell_wrapper':0x6F7970,'range_source':0x6F77B0,'infantry_set_destination':0x51AA40,'foot_set_destination':0x4D94B0,'walk_constructor':0x75AA90,'walk_stop':0x75ADA0,'infantry_stopped':0x521B40},
  assumptions=[
   'Supplied FootPerCell mode2 register/frame at4D882F, after prior mode2 work; this does not execute whole InfantryPerCell or WalkProcess head retirement.',
   'Original Infantry/Unit tables7EB058/7F5C70 copied unchanged, supplied valid actor/target/type/house objects. Only Walk constructor executes; default Infantry state is supplied, not constructor-certified.',
   'Type+5E4=false ordinary range branch; no strict XYZ branch claim. Actor at2624,2624,0, marked=false; target actual XYZ defaults3036,2780,123. Real flat cells10,10/11,10 plus shared Dummy; target coordinates copied by actual+4F0 before signed565730.',
   'Supplied ordinary null-setter envelope: reciprocal2A8 absent, no swap/open-top/bunker links; Enter contrasts execute original65AE30 with absent/present contact; stopped callback6E4=false. Doing27 human/nonhuman contrast executes originalHouse50B730 and actual early refusal.',
   'Supplied ownerNavCom/aux, queue head2 with suffix3/4/5, timers/latch and Walk destination/moving. Setter-only head row has a supplied committed head, not a whole movement producer proof.',
   'process_motion rows execute original75AEC0 entry/head gate through75BD25, observing full+36=true; no-head variants then supply head retirement before the caller. Numeric motion/retirement and Infantry6E4=true stopped-DoAction branch remain outside scope.',
   'NavQueue count and two backing entries are supplied independently from the path queue and captured even in setter-only rows.',
   'Frame100 and Rules+1768=22 supplied. Timer middle DWORD records original stack-derived ignored CDTimer slot; no logical timer meaning inferred. NullXYZ global suppliedzero; captured startup x87 control0E7F.'
  ],
  substitutions=[
   'SelectWeapon virtual+2E4 records originalTarCom and returns supplied index; GetWeapon+3F8 records same index and returns supplied weapon slot.',
   '6F7220 records actual sourceXYZ and converted targetCell then returns supplied boolean; this isolates caller ordering from existing range geometry/line-of-fire.',
   'No substitution of target+4F0,565730,6F7970,6F77B0,Mission+184,House50B730,Infantry51AA40,Foot4D94B0,Walk75ADA0 or stopped521B40.'
  ]))
