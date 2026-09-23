"""Original ordinary Walk Cell-click resolver. Run as a module; default checks saved outputs."""
from pathlib import Path
import struct, json
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import load_image,run_checked,STACK_BASE,STACK_SIZE,SCRATCH,RET_MAGIC,finish_vectors,provenance
from tools.spatial_oracle.map_queries import dwords,packed
ACTOR,TYPE,HOUSE,LOCO,VT,BASE,RAW,RECORDS,OUT,CLICK=[SCRATCH+i*0x1000 for i in range(10)]
CELLS=SCRATCH+0x10000
ACTION=SCRATCH+0xF000
MAP,TABLE,DUMMY=0x87F7E8,0xC00000,0xABDC50

def query(row):
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u);u.mem_map(STACK_BASE,STACK_SIZE);u.mem_map(SCRATCH,0x40000);u.mem_map(RET_MAGIC,0x1000);u.reg_write(UC_X86_REG_FPCW,0x0E7F)
 def r32(p):return struct.unpack('<I',u.mem_read(p,4))[0]
 def ret(cleanup,result):
  sp=u.reg_read(UC_X86_REG_ESP);u.reg_write(UC_X86_REG_EAX,result&0xffffffff);u.reg_write(UC_X86_REG_EIP,r32(sp));u.reg_write(UC_X86_REG_ESP,sp+4+cleanup)
 def call(addr,this,args):
  sp=STACK_BASE+STACK_SIZE-0x1000;u.mem_write(sp,dwords(RET_MAGIC,*args));u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,this);run_checked(u,addr,RET_MAGIC,count=500000,required_addresses=[addr]);assert u.reg_read(UC_X86_REG_ESP)==sp+4*(len(args)+1)
 call(0x49F2F0,0,[])
 unit=row.get('receiver') in ('unit','ship')
 ship=row.get('receiver')=='ship'
 u.mem_write(ACTOR,dwords(VT));u.mem_write(VT,bytes(u.mem_read(0x7F5C70 if unit else 0x7EB058,0x600)));u.mem_write(VT+0x70,dwords(ACTION));u.mem_write(ACTOR+(0x6C4 if unit else 0x6C0),dwords(TYPE));u.mem_write(ACTOR+0x21C,dwords(HOUSE));u.mem_write(ACTOR+0x684,b'\xff');u.mem_write(ACTOR+0x9C,dwords(*row.get('xyz',[5*256+64,5*256+64,0])));u.mem_write(ACTOR+0x8C,bytes([row.get('on_bridge',False)]));u.mem_write(ACTOR+0x74,bytes([row.get('marked',True)]));u.mem_write(ACTOR+0x90,b'\x01')
 u.mem_write(TYPE+0x5B4,dwords(row.get('movement_zone',4)));u.mem_write(TYPE+0x67C,dwords(5 if ship else 1 if unit else 0));u.mem_write(TYPE+0xC8D,bytes([row.get('move_to_shroud',True)]))
 # Unit receiver: the Drive or Ship constructor and its head (+40); Walk keeps +28.
 call(0x69EC50 if ship else 0x4AF540 if unit else 0x75AA90,LOCO,[]);u.mem_write(LOCO+0xC,dwords(ACTOR));u.mem_write(ACTOR+0x674,dwords(LOCO+4));u.mem_write(LOCO+0x14,dwords(1));u.mem_write(LOCO+(0x40 if unit else 0x28),dwords(*row.get('head',[0,0,0])))
 for p,v in [(0x89E7C0,104),(0xAC13C8,104),(0xAC13BC,416),(0xABDE88,104),(0x8B3DF4,416),(0xA8ED84,row.get('frame',100))]:u.mem_write(p,dwords(v))
 u.mem_write(0x89EA40,struct.pack('<90f',*([1.0]*90)));u.mem_write(MAP+0xF4,dwords(8,8,0,0,8,8));u.mem_write(MAP+0x13C,dwords(TABLE,0x40000));u.mem_write(TABLE,bytes(0x100000));u.mem_write(MAP+0x68,dwords(BASE,289));u.mem_write(BASE,bytes(289*4));u.mem_write(MAP+0x18+row.get('movement_zone',4)*4,dwords(RAW));u.mem_write(RAW,packed(2,3));u.mem_write(MAP+0x54,dwords(RECORDS,16,0,0))
 u.mem_write(DUMMY,bytes(0x200));u.mem_write(DUMMY+0x24,packed(99,98));u.mem_write(DUMMY+0x44,dwords(-1))
 for y in range(16):
  for x in range(16):
   p=CELLS+(y*16+x)*0x200;u.mem_write(p,dwords(0x7E4EEC));u.mem_write(p+0x24,packed(x,y));u.mem_write(p+0x44,dwords(-1));u.mem_write(p+0x12C,b'\x08');u.mem_write(TABLE+(y*512+x)*4,dwords(p));u.mem_write(BASE+(y*17+x)*4+2,struct.pack('<H',int(row.get('split',False) and x>=8)))
 for c in row.get('cells',[]):
  p=CELLS+(c['xy'][1]*16+c['xy'][0])*0x200;u.mem_write(p+0x11B,bytes([c.get('level',0),0]));u.mem_write(p+0x140,dwords(c.get('flags',0)));u.mem_write(p+0x124,dwords(c.get('raw',0),c.get('deck_raw',0)))
 for i,(a,b,active,kind) in enumerate(row.get('records',[])):u.mem_write(RECORDS+i*16,packed(*a)+packed(*b)+dwords(active,kind))
 u.mem_write(MAP+0x60,dwords(len(row.get('records',[]))))
 for c in row.get('shrouded',[]):u.mem_write(CELLS+(c[1]*16+c[0])*0x200+0x12C,b'\0')
 events=[];last=[]
 def observe(_u,a,_s,_d):
  sp=u.reg_read(UC_X86_REG_ESP);last.append(hex(a))
  if a==ACTION:events.append(['action',row.get('action',1)]);ret(12,row.get('action',1))
  elif a in [r32(0x7E11C8),r32(0x7E11CC)]:
   p=r32(sp+4);v=r32(p)+(1 if a==r32(0x7E11C8) else -1);u.mem_write(p,dwords(v));ret(4,v)
  elif SCRATCH<=a<SCRATCH+0x40000:raise RuntimeError(last[-30:])
  elif a in [0x565730,0x578080,0x586360]:events.append([hex(a),list(struct.unpack('<iii',u.mem_read(r32(sp+4),12)))])
  elif a in [0x4DE1FD,0x4DE3BA,0x4DE4B9]:events.append([hex(a),u.reg_read(UC_X86_REG_EAX)&255])
  elif a==0x56D100:
   args=[r32(sp+4+i*4) for i in range(6)];events.append(['reach',list(struct.unpack('<hh',u.mem_read(args[0],4))),list(struct.unpack('<hh',u.mem_read(args[1],4))),*args[2:]])
  elif a==0x56DC20:
   args=[r32(sp+4+i*4) for i in range(15)];events.append(['fnpc',list(struct.unpack('<hh',u.mem_read(args[1],4))),*args[2:12],list(struct.unpack('<hh',u.mem_read(args[12],4))),*args[13:]])
 u.hook_add(UC_HOOK_CODE,observe)
 u.mem_write(CLICK,packed(*row.get('clicked',[11,5])));before=bytes(u.mem_read(ACTOR,0x700));before_loco=bytes(u.mem_read(LOCO,0x38));call(0x4DE1D0,ACTOR,[OUT,CLICK,0]);assert bytes(u.mem_read(ACTOR,0x700))==before;assert bytes(u.mem_read(LOCO,0x38))==before_loco
 return dict(input=row,cell=list(struct.unpack('<hh',u.mem_read(OUT,4))),dummy=list(struct.unpack('<hh',u.mem_read(DUMMY+0x24,4))),events=events)
def inputs():
 for row in [{},{'movement_zone':3},{'movement_zone':3,'split':True},{'split':True},{'split':True,'frame':0},{'split':True,'frame':1},{'split':True,'frame':5},
 {'split':True,'head':[2368,1344,0]},{'split':True,'head':[1856,1344,0]},
 {'shrouded':[[11,5]]},{'shrouded':[[11,5]],'move_to_shroud':False},
 {'clicked':[0,0]},{'clicked':[16,5]},
 {'split':True,'clicked':[9,5],'cells':[{'xy':[9,5],'flags':256}],'records':[[[7,5],[11,5],1,0]]},
 {'split':True,'clicked':[9,5],'cells':[{'xy':[9,5],'flags':256}]},
 {'head':[1856,1344,0],'cells':[{'xy':[5,5],'level':4},{'xy':[7,5],'flags':256}], 'xyz':[1344,1344,416]},
 {'head':[1856,1344,0],'cells':[{'xy':[7,5],'level':4}], 'on_bridge':True,'xyz':[1344,1344,416]},
 {'head':[1856,1344,0],'cells':[{'xy':[5,5],'level':3},{'xy':[7,5],'flags':256}], 'xyz':[1344,1344,312]},
 {'cells':[{'xy':[11,5],'level':1}], 'move_to_shroud':False,'shrouded':[[10,4]]},
 {'cells':[{'xy':[11,5],'level':1}], 'move_to_shroud':False,'shrouded':[[10,4],[11,5]]}]:
  yield row
 # The same resolver behind a Unit receiver (Unit vtable, Drive head).
 for row in [{},{'split':True},{'split':True,'head':[2368,1344,0]},{'shrouded':[[11,5]]},{'clicked':[16,5]}]:
  yield dict(row,receiver='unit')
 # Stock Drive zones (Normal, Crusher) and a Ship receiver (Water, Float).
 for row in [{'movement_zone':0,'split':True},{'movement_zone':1},{'movement_zone':10,'split':True,'receiver':'ship'},{'movement_zone':10,'clicked':[16,5],'receiver':'ship'}]:
  yield dict({'receiver':'unit'},**row)
def generate():return [query(row) for row in inputs()]

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
  scope='Original Foot4DE1D0 ordinary Cell-click destination resolver behind 20 Infantry/Walk, 7 Unit/Drive (MovementZone 4, 0 and 1) and 2 Unit/Ship (MovementZone 10) receivers, including original coordinate/ShouldBeOnBridge, shroud, CanReachZone, full FNPC and its projection. Input action selection is supplied; synchronized executor and AStar are outside this comparison.',
  assumptions=['Original49F2F0 startup initializes the89F688 neighbor-direction table before any query; direction3=(1,1) is consumed by the odd-height shroud branch. Original Infantry/Cell vtables and original Walk constructor. Supplied live actor, owner/type, zero links, Tube index-1, MovementZone4 with stock AmphibiousDestroyer3 contrasts, Foot SpeedType0, MoveToShroud true unless row says false; Type JumpJet/Teleporter/Subterranean flags false. No full actor constructor/retail type loader claim.',
   'Map Size8,8 and LocalSize0,0,8,8; all16x16 Cell slots allocated with flat level0/slope0, no overlay, no occupation, raw shroud bit8 open except explicit rows. Shared Dummy starts zero except overlay-1 and coordinate99,98. Cells/records/raw zone rows are supplied rather than loaded or flood-filled.',
   'Raw zone row contains labels2/3; split assigns base group1 at x>=8. Concrete bridge record and structural flag are supplied independently; missing record remains missing. All90 land speed floats1.0. Ground/height/shroud constants104 and bridge416 with FPCW0E7F are supplied from previously established startup domain, not initialized here.',
   'Frame100 unless varied; FNPC receives null reference Cell(0,0), so frame-based candidate selection executes. Original 56DC20,56E7C0,4834A0 and6D6410 projection execute without substitution. This does not establish arbitrary blocker layouts, geometry or allocator failure.',
   'Output preserves exact chosen Cell, ordered observed map/shroud/reach/FNPC entries and final Dummy coordinate. Actor and Walk payload bytes are asserted unchanged. No input sound/selection/cursor, event encoding, whole click dispatch or deferred Process execution claimed.',
   'Unit rows copy the original Unit vtable 0x7F5C70 (type at +6C4) and construct Drive 0x4AF540 (SpeedType1) or Ship 0x69EC50 (SpeedType5), head at +40; the Unit +70 action selector is supplied like the Infantry one. Teleporter/JumpJet/Subterranean type terms stay false; every speed row is 1.0, so Ship rows cross land cells.'],
  substitutions=['Only Infantry virtual+70 action selector returns supplied action1 with original12-byte cleanup; every reached resolver/coordinate/height/shroud/zone/FNPC callable executes original bytes.',
   'OS InterlockedIncrement/Decrement imports update their pointed counter and return it with stdcall cleanup. No gameplay callable besides declared action selector is replaced.'],
  entry_points={'direction_initializer':0x49F2F0,'resolver':0x4DE1D0,'walk_constructor':0x75AA90,'foot_coordinate':0x4DBDF0,'walk_coordinate':0x75AC00,'should_be_on_bridge':0x4DDC40,'bridge_height_query':0x5F6A70,'shroud':0x586360,'can_reach_zone':0x56D100,'get_zone':0x56D230,'nearby':0x56DC20,'rectangle':0x56E7C0,'raw_passability':0x4834A0,'projection':0x6D6410}))
