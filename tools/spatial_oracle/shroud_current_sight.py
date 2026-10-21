"""Original ordinary-cell reveal and selected hostile-gap transitions.
Run python -B -m tools.spatial_oracle.shroud_current_sight --check / --write.
"""
import json,struct
from pathlib import Path
from unicorn import Uc,UC_ARCH_X86,UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EAX,UC_X86_REG_EDI,UC_X86_REG_ESI,UC_X86_REG_EBX
from tools.native_oracle import load_image,run_checked,finish_vectors,provenance,STACK_BASE,STACK_SIZE,RET_MAGIC
from tools.spatial_oracle.map_queries import packed,dwords
MAP,TABLE,CELL,QUERY,WORLD,PLAYER=0x87F7E8,0xC00000,0xB00000,0xB90000,0xB90020,0xBA0000

def execute(name,ops):
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u);u.mem_map(STACK_BASE,STACK_SIZE);u.mem_map(RET_MAGIC,4096)
 sp=STACK_BASE+STACK_SIZE-0x1000
 u.mem_write(MAP+0x13C,dwords(TABLE,0x40000));u.mem_write(TABLE,bytes(0x100000))
 u.mem_write(MAP+0xF4,dwords(12,12))
 n=0
 for ay in range(33):
  for ax in range(33):
   if not (12<ax+ay<=36 and ax-ay<12 and ay-ax<12):continue
   p=0xB10000+n*0x200;n+=1
   u.mem_write(p,bytes(0x200));u.mem_write(p,dwords(0x7E4EEC));u.mem_write(p+0x24,packed(ax,ay));u.mem_write(p+0x120,bytes([254,254]));u.mem_write(p+0x130,dwords(1,0,0,0));u.mem_write(TABLE+(ay*512+ax)*4,dwords(p))
 u.mem_write(sp,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,sp);run_checked(u,0x49F2F0,RET_MAGIC,count=1000)
 for y in range(9,12):
  for x in range(9,12):
   p=CELL+((y-9)*3+x-9)*0x200
   u.mem_write(p,bytes(0x200));u.mem_write(p,dwords(0x7E4EEC));u.mem_write(p+0x24,packed(x,y));u.mem_write(p+0x120,bytes([254,254]));u.mem_write(p+0x130,dwords(1,0,0,0))
   u.mem_write(TABLE+(y*512+x)*4,dwords(p))
 center=CELL+4*0x200
 u.mem_write(QUERY,packed(10,10));u.mem_write(WORLD,dwords(2560,2560,0));u.mem_write(0xABDE88,dwords(104));u.mem_write(0xA83D4C,dwords(PLAYER));u.mem_write(0xA8B230,dwords(0xBB0000));u.mem_write(0x887324,dwords(0xBC0000))
 def observe(op):
  u.mem_write(sp,dwords(RET_MAGIC,WORLD));u.reg_write(UC_X86_REG_ESP,sp);run_checked(u,0x586360,RET_MAGIC,count=1000)
  return dict(op=op,counter=struct.unpack('<i',u.mem_read(center+0x130,4))[0],gap=struct.unpack('<i',u.mem_read(center+0x134,4))[0],alt=struct.unpack('<I',u.mem_read(center+0x12C,4))[0],flags=struct.unpack('<I',u.mem_read(center+0x140,4))[0],cache=list(struct.unpack('<bb',u.mem_read(center+0x120,2))),shrouded=u.reg_read(UC_X86_REG_EAX)&0xFF)
 out=[observe('constructed')]
 for op in ops:
  u.reg_write(UC_X86_REG_ESP,sp)
  if op in ('reveal','leave'):
   u.mem_write(sp,dwords(RET_MAGIC,QUERY,PLAYER,int(op=='leave')));u.reg_write(UC_X86_REG_ECX,MAP)
   run_checked(u,0x4A9CA0,RET_MAGIC,count=10000)
  elif op=='unshroud':
   u.mem_write(sp,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ECX,center);run_checked(u,0x4876F0,RET_MAGIC,count=100)
  elif op in ('map_reveal_bulk','map_reset_bulk'):
   u.reg_write(UC_X86_REG_EAX,center);u.reg_write(UC_X86_REG_EDI,0);u.reg_write(UC_X86_REG_EBX,0);u.reg_write(UC_X86_REG_ESI,MAP)
   if op=='map_reveal_bulk':run_checked(u,0x577EBF,0x577EE9,count=100)
   else:run_checked(u,0x577B3C,0x577B6C,count=100)
  elif op.startswith('frame'):
   frame=int(op[5:]);u.mem_write(0xA8ED84,dwords(frame));u.reg_write(UC_X86_REG_EDI,frame);run_checked(u,0x55B29A,0x55B2C4,count=100000)
  elif op=='gap':
   u.mem_write(sp+0x10,packed(10,10));run_checked(u,0x6FB2F7,0x6FB3C1,count=1000)
  elif op.startswith('remove'):
   u.mem_write(PLAYER+0x577A,bytes([int(op=='remove_mapclear')]));u.mem_write(sp+0x10,packed(10,10));run_checked(u,0x6FB5E1,0x6FB69E,count=1000)
  else:raise ValueError(op)
  out.append(observe(op))
 return dict(name=name,observations=out)
scenarios=[('never_seen',['gap','remove']),('current_sight',['reveal','gap','remove']),('past_sight',['reveal','leave','gap','remove']),('current_sight_overlap',['reveal','reveal','gap','leave','leave','remove']),('enter_gap',['gap','reveal','leave','remove']),('spysat_remove_gate',['reveal','leave','gap','remove_mapclear']),('fire_only',['unshroud','gap']),('psychic_only',['reveal','leave','gap']),('departure_boundary',['reveal','gap','leave','frame119','frame120']),('return_cancels_pending',['reveal','gap','leave','frame119','reveal','frame120']),('removal_preserves_pending',['gap','reveal','leave','remove','frame120']),('departure_after_boundary',['reveal','gap','frame120','leave','frame121','frame239','frame240']),('second_gap_consumes_pending',['reveal','gap','leave','gap']),('fire_under_gap',['gap','unshroud','frame119','frame120']),('psychic_under_gap',['gap','reveal','leave','frame119','frame120']),('second_gap_then_return',['reveal','gap','leave','gap','reveal','frame120']),('first_fire_without_gap',['unshroud','frame119','frame120']),('first_psychic_without_gap',['reveal','leave','frame119','frame120'])]
scenarios.append(('due_same_footprint_refresh',['reveal','gap','leave','gap','reveal','frame119','leave','reveal','frame120']))

scenarios.extend([
 ('spysat_bulk_preserves_pending',['reveal','gap','leave','remove','map_reveal_bulk','gap','frame120']),
 ('reset_bulk_preserves_pending',['reveal','gap','leave','remove_mapclear','map_reset_bulk','gap','frame120']),
 ('spysat_source_before_gaps',['reveal','gap','leave','gap','reveal','leave','remove','remove','map_reveal_bulk','reveal','gap','gap','frame120']),
 ('spysat_gap_before_source',['reveal','gap','leave','gap','reveal','remove','leave','remove','map_reveal_bulk','gap','reveal','gap','frame120']),
])

def timer_cases():
 values=[(0,0,0),(104,15,118),(104,15,119),(119,15,120),(-1,0,120),(-1,5,120),(-1,-1,120),(2147483640,15,-2147483641),(120,15,119)]
 out=[]
 for start,duration,frame in values:
  u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
  foot=0xBD0000
  u.mem_write(foot+0x65C,dwords(start,0,duration));u.mem_write(0xA8ED84,dwords(frame))
  u.reg_write(UC_X86_REG_ESI,foot);u.reg_write(UC_X86_REG_EBX,0)
  stop=run_checked(u,0x4DA6C8,(0x4DA6EF,0x4DA7B0),count=100)
  out.append(dict(start=start,duration=duration,frame=frame,due=stop==0x4DA6EF))
 return out


# Full original no-edge MakeEnemy and its Human virtual reveal methods.
# Inputs supply a camera cache/type identity; map walks and anger are recorded
# transport boundaries. See provenance, not a whole FogOfWar comparison.
def alliance_human_control(discovered=True,limbo=False,in_playfield=True,passive=False):
 from tools.ai_base_building_oracle import Emu,FAKE,STUBS
 SOURCE=FAKE+0x500000
 VTABLE=FAKE+0x502000
 TYPE=FAKE+0x504000
 HUMAN=FAKE+0x506000
 COMPUTER=FAKE+0x50E000
 HUMAN_TYPE=FAKE+0x516000
 COMPUTER_TYPE=FAKE+0x518000
 ARRAY=FAKE+0x51A000
 e=Emu()
 e.write32(0xA8B238,1)
 e.write32(0xA83D4C,HUMAN)
 e.write32(0xA8EC7C,ARRAY)
 e.write32(0xA8EC88,1)
 e.write32(ARRAY,SOURCE)
 for house,typ,index in [(HUMAN,HUMAN_TYPE,1),(COMPUTER,COMPUTER_TYPE,0)]:
  e.write32(house+0x34,typ)
  e.write32(house+0x30,index)
  e.uc.mem_write(typ+0x1A6,bytes([int(passive and house==HUMAN)]))
 e.write32(SOURCE,VTABLE)
 e.write32(SOURCE+0x21C,HUMAN)
 e.uc.mem_write(SOURCE+0x81,bytes([int(limbo)]))
 e.uc.mem_write(SOURCE+0x41B,bytes([int(discovered)]))
 e.uc.mem_write(SOURCE+0x3D5,bytes([int(in_playfield)]))
 e.uc.mem_write(SOURCE+0x250,b'\x01')
 e.uc.mem_write(SOURCE+0x254,struct.pack('<iii',10240,10240,0))
 e.uc.mem_write(SOURCE+0x9C,struct.pack('<iii',10496,10240,0))
 e.write32(SOURCE+0x260,6)
 e.write32(TYPE+0x5E8,0)
 for slot,func in [(0x2C,STUBS+0x200),(0x84,STUBS+0x210),(0x48C,0x70B1D0),(0x488,0x70AF50)]:e.write32(VTABLE+slot,func)
 e.hook(STUBS+0x200,lambda _:1,0)
 e.hook(STUBS+0x210,lambda _:TYPE,0)
 def area(x):
  e.events.append(['area',list(struct.unpack('<iii',e.uc.mem_read(x.arg(0),12))),*[x.arg(i) for i in range(1,8)]])
  return 0
 def border(x):
  e.events.append(['border',list(struct.unpack('<iii',e.uc.mem_read(x.arg(0),12))),*[x.arg(i) for i in range(1,4)]])
  return 0
 def anger(x):
  e.events.append(['anger',x.uc.reg_read(UC_X86_REG_ECX),x.arg(0),x.arg(1)])
  return 0
 e.hook(0x5678E0,area,32)
 e.hook(0x567DA0,border,16)
 e.hook(0x504790,anger,8)
 for a in [0x4ADEE0,0x4ADCD0,0x70B1D0,0x70AF50]:e.mark(a,['entry',hex(a)])
 e.invoke(0x4F9F90,ecx=COMPUTER,args=[HUMAN,0],required=[0x4F9F90])
 return dict(inputs=dict(discovered=discovered,limbo=limbo,in_playfield=in_playfield,passive=passive),events=e.events,latch=e.uc.mem_read(SOURCE+0x250,1)[0],cached_radius=e.read_i32(SOURCE+0x260),ally_bits=e.read32(COMPUTER+0x5788))

if __name__=='__main__':
 finish_vectors(lambda:{'cases':[execute(*s) for s in scenarios],'timer_cases':timer_cases(),'alliance_human_controls':[alliance_human_control(),alliance_human_control(discovered=False),alliance_human_control(limbo=True),alliance_human_control(in_playfield=False),alliance_human_control(passive=True)]},Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
  scope='Original ordinary-cell current-sight versus hostile gap shroud observations',
  assumptions=['Constructor-derived Cell fields supplied:130=1,134=0,120/121=-2; complete Size12x12 allocated diamond, selected3x3 real-cell neighborhood',
   'Original MapCell4A9CA0 executes toreturn with real neighbor-cache and notify leaves; frame0,emptyobjectlists,scenarioFogOfWarclear',
   'Hostile gap apply/remove begin after owner/power/radius admission and stop before second owner classification; original fixedlookup executes',
   'IsShrouded586360 executes on ordinary worldZ0; boolean result isAL, upperEAX ignored',
   'FireUnshroud leaf corresponds5673A0 final0; PsychicReveal sequence corresponds6CD773 final0 then6CD79C final1',
   'Original Logic55B29A modulo gate and complete578100 two-pass sweep execute at supplied signed native frames; selected shroud observations do not certify all edge-cache outputs',
   'Foot timer4DA6C8 executes after admitted moving/high-flight/direct-ally gates to due4DA6EF or rejected4DA7B0 boundary; no reveal call is stubbed or executed by this timer fragment',
   'Original577EBF..577EE9 and577B3C..577B6C bulk stores execute on the selected Cell; source/gap brackets compose existing original leaves with supplied object registration order, not full callback dispatch',
   'Legacy remove_mapclear case label refers to actual House577A SpySatActive, not byte241 MapIsClear',
   'Alliance controls execute full original no-edge4F9F90, visitors4ADEE0/4ADCD0, House50B6F0 and source70B1D0/70AF50; supplied current-player Sight0 camera cache radius6,41B/limbo/playfield/passive gates',
   'No renderer pixels, complete reveal traversal, optionalfogrecords or fullGapGenerator lifecycle comparison'],
  substitutions=['Alliance-only virtual RTTI/type lookup return declared fixture identities; Map5678E0 and border567DA0 record arguments and return without cells; anger504790 records arguments and returns'],entry_points={'make_enemy':0x4F9F90,'alliance_release':0x4ADEE0,'alliance_readmit':0x4ADCD0,'source_release':0x70B1D0,'source_update':0x70AF50,'map_cell':0x4A9CA0,'unshroud':0x4876F0,'hostile_gap_cell':0x6FB2F7,'hostile_remove_cell':0x6FB5E1,'is_shrouded':0x586360,'periodic_logic':0x55B29A,'recalc_shroud':0x578100,'foot_timer_gate':0x4DA6C8,'map_reveal_bulk':0x577EBF,'map_reset_bulk':0x577B3C}))
