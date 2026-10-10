"""Original AEGIS destination, full path search and first Ship head production.

The hierarchy comes from the separately native-built Shrapnel packet; actor,
owner and unloaded runtime services are explicit fixture inputs. The command
dispatch/scheduler and full scenario loader do not execute.
"""
from pathlib import Path
from collections import Counter
from tools.spatial_oracle.naval_occupants import *
from tools.spatial_oracle.shrapnel_repair.hierarchy_composition import HierarchyRepair, Rules, theater, input_case
from tools.spatial_oracle.anytown_damage.navigation import Navigation

class Head(Native):
 def read_inputs(self):
  super().read_inputs();self.movement_readers=[];u=self.u
  mode=ASSETS/'MPBattleMD.ini'
  for name,path in [('RULESMD.INI',ASSETS/'RULESMD.INI'),('MPBattleMD.ini',mode),('XShrapnel.MAP',ASSETS/'XShrapnel.MAP')]:
   sections,_=lexical(path.read_bytes(),{'AEGIS','General','AI'});self.make_ini(sections)
   if 'AEGIS' in sections:
    self.block(0x71464A,0x71469F,{UC_X86_REG_EBP:self.typ,UC_X86_REG_ESI:INI,UC_X86_REG_EBX:self.typ+0x24})
    self.block(0x714B14,0x714B35,{UC_X86_REG_EBP:self.typ,UC_X86_REG_EDI:INI,UC_X86_REG_EBX:self.typ+0x24})
   if 'AI' in sections:self.block(0x6739E5,0x673A0C,{UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:INI})
   if 'General' in sections:
    self.block(0x670EDD,0x670EFD,{UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:INI})
   self.movement_readers.append(dict(file=name,sha256=sha(path.read_bytes()),speed=self.read32(self.typ+0x678),rot=self.read32(self.typ+0x71C),path_delay_bits=bytes(u.mem_read(RULES+0x1760,8)).hex(),close_enough=self.read32(RULES+0x1718)))
 def __init__(self,h,t):
  self.long_heap=None;self.path_events=[];self.path_pending=[];self.stages=[]
  super().__init__(dict(type='AEGIS',current=[29056,15232,208],target=[114,59],member=[113,59],land=2))
  u=self.u;self.phase='setup'
  for start,end,perms in h.u.mem_regions():
   if start>=0x40000000:
    u.mem_map(start,end-start+1,perms)
    u.mem_write(start,bytes(h.u.mem_read(start,end-start+1)))
  for address,size in ((MAP,0x200),(0xC00000,0x100000),(0xABDC50,0x200),(0xA8ED2C,4),(0xA8ED38,4),(0xA83D84,4),(0x89EA40,12*36)):
   u.mem_write(address,bytes(h.u.mem_read(address,size)))
  self.cells=dict(h.ptrs);self.selected=self.cells[114,59]
  self.covered={tuple(r['coord']) for r in h.case['supplied_cells']}
  for p in self.cells.values():u.mem_write(p,dwords(0x7E4EEC))
  for a,v in t['globals'].items():u.mem_write(a,dwords(v))
  u.mem_write(RULES+0x664,bytes(h.u.mem_read(0x44400664,1)))
  u.mem_write(self.cells[113,59]+0xE4,dwords(self.actor))
  u.mem_write(self.actor+0x55C,packed(113,59));u.mem_write(self.actor+0x74,b'\1')
  u.mem_write(self.actor+0xAC,dwords(2));u.mem_write(self.house+0x1EC,b'\1');u.mem_write(0xA8B238,dwords(5))
  u.mem_map(0x48000000,0x1000000);self.long_heap=0x48000000
  # Native Ship globals include the 104/416 height scales and direction tables.
  self.invoke(0x69EBB0,0)
  for a in (0x561710,0x5617A0,0x5617C0,0x5617E0):self.invoke(a,0)
  assert self.read32(0xABDE88)==104
  self.invoke(0x6D1C20,self.alloc(0x2000))
  self.invoke(0x5F5B90,self.actor,(0,self.alloc(0x80)))
  for a in (0x49F0E0,0x49F190,0x49F2F0,0x49F2D0,0x49F280):self.invoke(a,0)
  Navigation.setup_pathfinder(self.invoke)
  # Production-facing64 is native raw16-bit16384. Actual Facing.SetDesired
  # is not needed when both retained Facing values already agree.
  for off in (0x388,0x3A0):u.mem_write(self.actor+off,dwords(16384,16384))
  u.mem_write(0xA8ED84,dwords(26))
  self.text_hash=sha(bytes(u.mem_read(0x401000,0x3E0000)))
  self.phase='measure'
 def alloc(self,n):
  if self.long_heap is None:return super().alloc(n)
  p=self.long_heap;self.long_heap+=(max(n,1)+15)&~15;assert self.long_heap<0x49000000;return p
 def invoke(self,fn,this,args=(),*,timeout_us=60000000,context=None):
  u=self.u;u.mem_write(SP,dwords(RET_MAGIC,*args));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,this)
  run_checked(u,fn,RET_MAGIC,count=10000000,timeout_us=timeout_us,context=context);return u.reg_read(UC_X86_REG_EAX)
 def hook(self,u,pc,n,d):
  if pc==0x7C978A:
   self.path_events.append(dict(kind='CRT_atexit_registration',function=hex(self.read32(u.reg_read(UC_X86_REG_ESP)+4))))
   self.ret(0);return
  if pc==0x6C8C40:
   assert self.phase=='setup';self.ret(0);return
  if self.long_heap is not None and pc in (0x7C8E17,0x7C8B3D):
   sp=u.reg_read(UC_X86_REG_ESP)
   if pc==0x7C8E17:
    size=self.read32(sp+4);p=self.alloc(size);self.path_events.append(dict(kind='allocate',size=size,pointer=hex(p)));self.ret(p)
   else:self.path_events.append(dict(kind='free',pointer=hex(self.read32(sp+4))));self.ret(0)
   return
  if self.phase=='measure':
   sp=u.reg_read(UC_X86_REG_ESP)
   if self.path_pending and pc==self.path_pending[-1]['return']:
    row=self.path_pending.pop();row['result']=signed(u.reg_read(UC_X86_REG_EAX));row.pop('return')
    if row['kind']=='foot_find_path_return':row['state']=self.snapshot();row['result_al']=row['result']&255
    self.path_events.append(row)
   names={0x741970:'unit_set_destination',0x4D94B0:'foot_set_destination',0x69F450:'ship_move_to',0x69FC10:'ship_process',0x4D3920:'foot_find_path',0x4CBBA0:'run_astar',0x42C900:'pathfinder_find',0x429A90:'astar_search',0x73F0A0:'unit_can_enter',0x6A28FF:'fresh_head_coordinates',0x6A293F:'fresh_head_coordinates_complete',0x6A3F50:'ship_at_coord',0x4D3780:'mark',0x47D2B0:'recalc',0x47EA90:'cell_remove_content',0x6A3C3B:'head_finalize',0x6A3D09:'head_published'}
   if pc in names:
    row=dict(kind=names[pc],pc=hex(pc),this=hex(u.reg_read(UC_X86_REG_ECX)))
    if pc==0x73F0A0:
     p=self.read32(sp+4);coord=list(struct.unpack('<hh',u.mem_read(p+0x24,4)));assert tuple(coord) in self.covered,('CanEnter outside physical input',coord)
     row.update(coord=coord,land=signed(self.read32(p+0xEC)),args=[signed(self.read32(sp+8+i*4)) for i in range(4)])
     self.path_pending.append(dict(row,**{'return':self.read32(sp)}))
    elif pc==0x4D3920:self.path_pending.append(dict(kind='foot_find_path_return',**{'return':self.read32(sp)}))
    elif pc==0x4D3780:row['mark']=self.read32(sp+4)
    elif pc==0x47D2B0:row['cell']=list(struct.unpack('<hh',u.mem_read(u.reg_read(UC_X86_REG_ECX)+0x24,4)))
    elif pc==0x6A3D09:row['head']=list(struct.unpack('<3i',u.mem_read(self.loco+0x40,12)))
    self.path_events.append(row)
  super().hook(u,pc,n,d)
 def snapshot(self):
  u=self.u
  ints=lambda p,n:list(struct.unpack('<'+'i'*n,u.mem_read(p,n*4)))
  return dict(xyz=ints(self.actor+0x9C,3),head=ints(self.loco+0x40,3),destination=ints(self.loco+0x34,3),path=ints(self.actor+0x5E0,24),nav=self.read32(self.actor+0x5A4),movement_timer=ints(self.actor+0x640,3),retries=self.read32(self.actor+0x64C),fraction=struct.unpack('<d',u.mem_read(self.loco+0x50,8))[0],track_selector=signed(self.read32(self.loco+0x58)),track_cursor=self.read32(self.loco+0x5C),head_valid=u.mem_read(self.loco+0x63,1)[0],marked=u.mem_read(self.actor+0x74,1)[0],current_ground_head=self.read32(self.cells[113,59]+0xE4),object_next=self.read32(self.actor+0x30))
 def run(self):
  self.path_events.clear();self.calls=[];self.writes=[];self.stages.append(dict(stage='before',state=self.snapshot()))
  before={k:bytes(self.u.mem_read(p,0x3F4)).hex() for k,p in self.rngs.items()}
  self.invoke(0x741970,self.actor,(self.cells[117,59],1));self.stages.append(dict(stage='accepted',state=self.snapshot()))
  self.u.mem_write(0xA8ED84,dwords(27))
  self.invoke(0x69FC10,0,(self.loco+4,));self.stages.append(dict(stage='process',state=self.snapshot()))
  assert not self.path_pending
  assert self.text_hash==sha(bytes(self.u.mem_read(0x401000,0x3E0000)))
  return dict(stages=self.stages,events=self.path_events,receiver_calls=self.calls,writes=self.writes,rng_before=before,rng_after={k:bytes(self.u.mem_read(p,0x3F4)).hex() for k,p in self.rngs.items()})

def generate():
 r=Rules();t=theater();case=input_case([117,56],t);h=HierarchyRepair(case,r,t)
 m=Head(h,t)
 try:
  result=m.run()
  assert result['rng_before']==result['rng_after']
  return dict(schema=1,input=dict(source_cell=[113,59],source_xyz=[29056,15232,208],destination_cell=[117,59],facing_raw=16384,mission=2,human_owner=True,game_mode=5,order_frame=26,process_frame=27,initial_marked=True,initial_head=[0,0,0],production_input_sha256=case['production_input_sha256'],physical_map_sha256=case['physical_map_sha256'],covered_cells=case['supplied_cells'],native_size=case['size'],local_size=case['local_size']),movement_readers=m.movement_readers,native_type=m.inputs,initial_graph_counts=[len(g['records']) for g in h.initial_graphs],result=result)
 except Exception:
  print(json.dumps(dict(stages=m.stages,events=m.path_events,last=m.snapshot()),indent=2));raise

def metadata():return provenance(scope=__doc__,assumptions=['Physical Shrapnel cells/TMP/theater/overlays and supplied production class/height planes feed the frozen HierarchyRepair fixture. Actual56C510,581F90 and42C1C0 produce its initial hierarchy; selected physical cells and whole native-built map/graph buffers are transplanted unchanged into the original AEGIS fixture. This is not a native scenario load.','Original Unit/Foot constructors and Ship constructor from naval_occupants; explicit owner/human, Move mission, frame26/27, stable facing16384, marked current-cell membership and no head/path inputs. Native Unit741970 setter and complete Ship69FC10 Process execute. Original Foot4D3920,4CBBA0,Pathfinder42C900/429A90 and concrete Unit73F0A0 execute without path/query/Mark return substitutions. Every queried CanEnter cell must be among the170 covered physical inputs.','Actual Pathfinder constructor42A6D0, map-array resize42AC00 and hierarchy scratch42C1C0 execute. Actual direction, Ship and Map shroud-height initializers execute; actual Tactical ctor and Foot footprint startup execute. MarkUp/Down and resident Recalc execute. Empty scenario services/visibility are supplied; full Unlimbo, command-dispatch, prior scheduler frames and paid movement after the first head are excluded.','Original selected Speed71464A,ROT714B14,CloseEnough670EDD andPathDelay6739E5 blocks read physicalRULESMD,MPBattleMD,map lexical caches. Active MPBattleMD is pinned by the adjacent physical input packet; its recorded bytes are identical to the former MPBattle.INI alias, with no affected keys. Inherited native fixture retains explicitly suppliedFPCW0E7F; this is not an ambient hardware capture.','FrameTimer middle dword+644 is raw native stack carry/padding; logical fields are start+640 and duration+648. Full states and writes are retained without assigning meaning to the middle word. Main/Scenario/MapGen full states remain unchanged.'],substitutions=['Successful allocation/free and CRT atexit registration (registered targets recorded, no destruction occurs in the measured interval). Tactical OS clock0 only during setup. Inherited INI lexical-cache and Interlocked imports; no gameplay result is substituted.'],entry_points={'unit_destination':0x741970,'ship_move':0x69F450,'ship_process':0x69FC10,'foot_find_path':0x4D3920,'astar_wrapper':0x4CBBA0,'pathfinder_search':0x42C900,'astar':0x429A90,'unit_can_enter':0x73F0A0,'fresh_coordinates':0x6A28FF,'head_finalize':0x6A3C3B})
if __name__=='__main__':finish_vectors(generate,HERE/'naval_head_producer.json',provenance=metadata)
