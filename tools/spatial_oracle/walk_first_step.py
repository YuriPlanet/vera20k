"""Original Walk first ready-path Process and completed-head queue propagation.
Path success and CanEnter=0 are supplied; numeric paid movement is not executed.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.spatial_oracle import walk_head_occupation as h
from tools.spatial_oracle.map_queries import dwords, packed
from tools.native_oracle import STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance

CAN_ENTER = SCRATCH + 0xE000

class Original(h.Original):
 def __init__(self,row):
  self.row=row
  super().__init__()
  self.uc.reg_write(UC_X86_REG_FPCW,0x0E7F)
 def observe(self,u,address,size,data):
  if address==CAN_ENTER:
   sp=u.reg_read(UC_X86_REG_ESP)
   self.events.append(['can_enter_args',list(struct.unpack('<IIIII',u.mem_read(sp+4,20)))])
   self.ret(20,0)
  elif address==h.MISSION_GET:
   self.events.append(['mission',self.row.get('mission',1)])
   self.ret(0,self.row.get('mission',1))
  elif address==0x75C240:
   self.events.append(['head_input',list(struct.unpack('<iii',u.mem_read(self.read32(u.reg_read(UC_X86_REG_ESP)+4),12)))])
  else:
   super().observe(u,address,size,data)
 def process(self):
  row=self.row;u=self.uc
  u.mem_write(0xA8ED84,dwords(100))
  if row.get('infantry_constructor_facing',False):
   self.call(0x4C91C0,h.OWNER+0x388,[])
   sp=STACK_BASE+STACK_SIZE-0x1000
   u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ESI,h.OWNER)
   run_checked(u,0x517BBD,0x517BCA,count=100,required_addresses=[0x4C9680])
   assert u.reg_read(UC_X86_REG_ESP)==sp
  self.call(0x75AA90,h.LOCO,[]);u.mem_write(h.LOCO+0xC,dwords(h.OWNER))
  sx,sy=row['sub'];current=[9*256+sx,10*256+sy,260]
  self.setup(dict(input=[10*256+sx,10*256+sy,260],ground=0,deck=0,level=2,slope=1))
  u.mem_write(h.OWNER+0x9C,dwords(*current));u.mem_write(h.OWNER+0x90,b'\1')
  u.mem_write(h.OWNER+0x5E0,dwords(2,3,4,5));u.mem_write(h.OWNER+0x558,packed(9,8))
  u.mem_write(h.OWNER+0x5A4,dwords(h.CELL if row.get('nav',True) else 0))
  u.mem_write(h.OWNER+0x2B4,dwords(h.BUILDING if row.get('target',True) else 0))
  u.mem_write(h.VTABLE+0x1AC,dwords(CAN_ENTER));u.mem_write(h.VTABLE+0x1BC,dwords(0x5F6960))
  u.mem_write(h.VTABLE+0x48,dwords(0x5F65A0));u.mem_write(h.VTABLE+0x544,dwords(0x4D3710))
  u.mem_write(h.LOCO+0x1C,dwords(10*256+128,10*256+128,260))
  u.mem_write(0x89F6D8,dwords(0,-256,256,-256,256,0,256,256,0,256,-256,256,-256,0,-256,-256))
  u.mem_write(h.OUTPUT,dwords(*current));self.call(0x5217C0,h.OWNER,[h.OUTPUT])
  initial_raw=self.read32(h.CURRENT+0x124)&255
  self.call(0x65C6D0,h.SCENARIO+0x218,[row.get('seed',31)])
  before=bytes(u.mem_read(h.SCENARIO+0x218,0x3F4)).hex();self.events=[]
  sp=STACK_BASE+STACK_SIZE-0x1000;u.mem_write(sp,dwords(RET_MAGIC,0));u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,h.LOCO)
  run_checked(u,0x75AEC0,RET_MAGIC,count=10000,required_addresses=[0x75AEC0,0x75B690,0x75BC1A,0x75BC2A,0x75BCBD])
  assert u.reg_read(UC_X86_REG_ESP)==sp+8
  return dict(input=row,initial_current=current,initial_raw=initial_raw,
    current=list(struct.unpack('<iii',u.mem_read(h.OWNER+0x9C,12))),
    head=list(struct.unpack('<iii',u.mem_read(h.LOCO+0x28,12))),motion=u.mem_read(h.LOCO+0x36,1)[0],
    path_head=list(struct.unpack('<iiii',u.mem_read(h.OWNER+0x5E0,16))),reference=list(struct.unpack('<hh',u.mem_read(h.OWNER+0x558,4))),
    speed_fraction=struct.unpack('<d',u.mem_read(h.OWNER+0x578,8))[0],
    facing=dict(current=self.read32(h.OWNER+0x388)&65535,prev=self.read32(h.OWNER+0x38C)&65535,start_frame=self.read32(h.OWNER+0x390),duration_frames=self.read32(h.OWNER+0x398),rot_per_frame=self.read32(h.OWNER+0x39C)&65535),
    current_raw=self.read32(h.CURRENT+0x124)&255,head_raw=self.read32(h.CELL+0x124)&255,
    rng_before=before,rng_after=bytes(u.mem_read(h.SCENARIO+0x218,0x3F4)).hex(),events=self.events)
 def completed_queue(self,invalidated):
  u=self.uc;before=[-1 if invalidated else 2,3,4,5]+[-1]*20
  u.mem_write(h.OWNER+0x5E0,dwords(*before));u.mem_write(h.LOCO+0xC,dwords(h.OWNER))
  u.reg_write(UC_X86_REG_EBP,h.LOCO);u.reg_write(UC_X86_REG_EBX,h.LOCO+0x28)
  u.reg_write(UC_X86_REG_ESP,STACK_BASE+STACK_SIZE-0x1000)
  run_checked(u,0x75BD83,0x75BDBB,count=100,required_addresses=[0x75BD89,0x75BDAC,0x75BDB1])
  return dict(invalidated=invalidated,before=before,after=list(struct.unpack('<24i',u.mem_read(h.OWNER+0x5E0,96))))

def generate():
 rows=[
  {'sub':[192,64]}, {'sub':[128,128]}, {'sub':[64,64]},
  {'sub':[128,128],'seed':0}, {'sub':[128,128],'mission':0,'nav':False,'target':False}]
 return dict(ready_path=[Original(row).process() for row in rows+[dict(row,infantry_constructor_facing=True) for row in rows]],
  completed_queue=[Original({}).completed_queue(value) for value in [False,True]])

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
  scope='Original first ready-path Walk Process through actual fresh-head return, plus completed-head path terminator propagation block. This complements accepted setter/first FindPath request timing; pathfinder core and CanEnter admission remain supplied seams.',
  entry_points={'walk_constructor':0x75AA90,'walk_process':0x75AEC0,'prospective_coords':0x75B59C,'can_enter_dispatch':0x75B690,'head':0x75C240,'placement':0x481180,'new_head_return':0x75BCBD,'queue_propagation':0x75BD83,'foot_speed':0x4D3710,'owner_cell':0x5F6960,'facing_constructor':0x4C91C0,'infantry_facing_rate':0x517BBD},
  assumptions=['Supplied ready Foot path2,3,4,5 and reference9,8, current cell9,10, nextcell10,10, signed level2/slope1, literal currentZ260, destination Cell center, ordinary alive Infantry mission1 (mission0 contrast), no slave/crate/gate/transport.',
   'Native frame100; five legacy rows supply zero-initialized FacingClass/rate0, and five matching rows execute the original facing constructor plus Infantry constructor517BBD..517BCA rate127 block. Final facing words and logical timer fields are observed. Actual Walk constructor executes. Owner virtual+1BC uses original5F6960 (Infantry slot7EB214); +48 uses5F65A0; +544 uses4D3710. Actual locomotor facing callback from the constructed table and Foot speed callback execute before return75BCBD. No paid numeric motion executes in this first-head arm.',
   'Octant lepton vector table89F6D8 and imported subcell offsets, heights104/416, FPCW0E7F are supplied runtime data. Initial current raw bits are produced by original5217C0; they are not inferred from a chosen slot.',
   'Full1012-byte ScenarioRandom state is recorded before/after Process; center rows exercise actual draws and noncenter row skips them. CurrentXYZ/head/raw/queue/reference/speed are observed separately.',
   'Completed-queue rows enter original75BD83 after the caller Mark removal, stop before SetCoords75BDBB, and supply24 raw path words. They prove the -1 propagation before shift only, not the whole completed-head transaction.'],
  substitutions=['CanEnter+1AC records its five arguments and returns0 (admitted). This is not proof of CanEnter behavior or pathfinder output.',
   'Inherited head fixture virtual seams: supplied owner index41; mission0/1; false owner predicates; missing ground building/gate. Map, placement, RNG, raw leaves, facing and speed callbacks execute original bytes.']))
