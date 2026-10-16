"""Original full Anytown navigation graphs, deriving Cell inputs through original readers/Recalc."""
from .navigation_inputs import *
from .navigation_publication import finish_vectors
from .anytown_geometry import Geometry,inputs as crop_inputs,sr
from tools.spatial_oracle.shrapnel_repair.zone_composition import ConnectivityRepair,BASE,PLANE
from tools.spatial_oracle.shrapnel_repair.hierarchy_composition import HierarchyRepair
from tools.spatial_oracle.bridge_rim import TABLE,CELLS,DUMMY,MAP,COORD
from tools.native_oracle import run_checked,RET_MAGIC,STACK_BASE,STACK_SIZE
from tools.spatial_oracle.map_queries import packed,dwords,normalize
from collections import Counter
import copy

class Navigation(Geometry):
 allocate=ConnectivityRepair.allocate
 call=ConnectivityRepair.call
 nav_snapshot=ConnectivityRepair.nav_snapshot
 graph_snapshot=HierarchyRepair.graph_snapshot
 dummy_snapshot=HierarchyRepair.dummy_snapshot
 @classmethod
 def on_existing_map(cls,owner,*,size,class_height_plane):
  """Join native graphs to an existing VM and declared admitted map prior.

  Cell membership, dimensions and the class/height bytes are caller inputs,
  not answers to a path query. Original connectivity/hierarchy and all later
  pathfinding execute. The existing runtime owns every allocation and hook.
  """
  from types import MethodType
  m=cls.__new__(cls);m.uc=owner.u;m.case=dict(size=list(size))
  m.width=sum(size);m.side=m.width+1
  assert list(struct.unpack('<2i',m.uc.mem_read(MAP+0xF4,8)))==list(size)
  assert len(class_height_plane)==m.side*m.side*4
  m.allocate=owner.allocate;m.call=MethodType(ConnectivityRepair.call,m)
  m.uc.mem_map(0x44000000,0x800000)
  m.uc.mem_write(BASE,class_height_plane)
  m.uc.mem_write(MAP+0x68,dwords(BASE,m.side*m.side,PLANE))
  return m
 @staticmethod
 def setup_pathfinder(call):
  """Original shared workspace producer; call transport owns execution budget.

  42A6D0 constructs the global, 42AC00 sizes its map arrays, and 42C1C0
  derives scratch from retained native hierarchy. No query result is supplied.
  """
  call(0x49F3A0,this=0)
  call(0x42A6D0,this=0x87E8B8)
  call(0x42AC00,this=0x87E8B8,args=(MAP+0xEC,))
  call(0x42C1C0,this=0x87E8B8)
 def __init__(self,r,t,tiles,*,case=None,create_actor=True,actor_coord=(87,53),actor_height=4,admission_cells=None,stages=None,scenario_theater=None,cell_inputs_only=False,terrain_inputs_only=False):
  assert not (cell_inputs_only and terrain_inputs_only)
  self.create_actor=create_actor;self.actor_coord=actor_coord;self.actor_height=actor_height
  self.admission_cells=admission_cells if admission_cells is not None else [(87,53),(86,54),(87,54),(88,54),(87,55),(85,54),(89,54)]
  self.stages=stages;self.tile_count=t['count']
  case=copy.deepcopy(case) if case is not None else crop_inputs(t);raw,sections,physical=map_inputs(r.map_file);case['cells']=[[*c,p['tile'],p['subtile'],0,p['overlay'],p['frame'],None,p['level'],0] for c,p in sorted(physical.items(),key=lambda kv:(kv[0][1],kv[0][0]))];case['supplied_cells']=[];case['boundary']='Physical full-diamond tile/subtile/level/ice/overlay/frame supplied from MAP bytes; original Cell constructor and Recalc derive runtime land/slope/class. Actual native scenario/file loader, actor command dispatch and projectile admission excluded.'
  case['local_size']=normalize(case['size'],case['local_size'])['normalized'];self.activity='bootstrap';self.counters=Counter();self.illegal=[];self.range_pending=[];self.used_tile_heads=set();self.tile_image_getter=None
  super().__init__(case);u=self.uc;self.width=sum(case['size']);self.side=self.width+1;self.physical=physical
  if scenario_theater is not None:
   # Copy the original Map/Theater reader's output into this VM's retained
   # Scenario receiver before original Terrain/Cell consumers execute.
   u.mem_write(sr.u32(u,0xA8B230)+0x1258,dwords(scenario_theater['value']))
  u.mem_map(0x24000000,0x400000);u.mem_write(0x24000000,bytes(r.u.mem_read(0x24000000,0x400000)))
  u.mem_map(0x44000000,0x800000);u.mem_map(0x48000000,0x8000000)
  for a,n in [(0xA83D80,24),(0xA8E318,24),(0x8B4150,24),(0xB0F4E8,24),(0xB0EDC0,0x1000),(0x89EA40,12*36)]:u.mem_write(a,bytes(r.u.mem_read(a,n)))
  u.mem_write(0x8871E0,dwords(r.rules));u.mem_write(MAP+0x68,dwords(BASE,self.side*self.side,PLANE));u.mem_write(BASE,b'\x07\x00\x00\x00'*(self.side*self.side))
  self.activity='cell_ctor'
  for c,p in self.ptrs.items():
   self.call(0x47BBF0,this=p,count=2000);v=physical[c];u.mem_write(p+0x24,packed(*c));u.mem_write(p+0x38,dwords(v['tile']));u.mem_write(p+0x44,dwords(v['overlay'] if v['overlay'] is not None else -1));u.mem_write(p+0x11A,bytes([v['subtile'],v['level']]));u.mem_write(p+0x11E,bytes([v['frame']]));u.mem_write(p+0x119,bytes([v['ice']]))
  self.call(0x47BBF0,this=DUMMY,count=2000)
  self.tile_image_getter=sr.u32(u,0x7ECC48+0x9C)
  self.activity='tile_ctor';u.mem_write(0xA8ED28,dwords(0x7EB6D4,0x44000000,t['count'],1,0,10));u.mem_write(0xB0F670,dwords(0x7EB6D4,self.allocate(4096*4),4096,1,0,10))
  for a,v in t['globals'].items():u.mem_write(a,dwords(v))
  data=0x48000000
  shadow_bases=[v['base'] for v in r.tile_properties if v['shadow']]
  assert len(shadow_bases)<=5
  u.mem_write(0xAA102C,dwords(*(shadow_bases+[0]*(5-len(shadow_bases)))))
  self.tile_properties={v['base']+j:v for v in r.tile_properties for j in range(v['count'])}
  for i,name in t['tiles'].items():
   p=0x44480000+i*0x400;u.mem_write(COORD+0x100,name[:-4].encode('ascii')+b'\0');self.call(0x5447C0,this=p,args=(i,-65,0,COORD+0x100,0),count=10000)
   properties=self.tile_properties[i]
   if 'allow_tiberium' in properties:u.mem_write(p+0x306,bytes([properties['allow_tiberium']]))
   u.mem_write(p+0x2E1,bytes([int(bool(properties['shadow'] and properties['shadow_tiles']))]))
   anim=properties['animations'].get(i-properties['base'])
   if anim:u.mem_write(p+0x2C8,dwords(anim['index'],anim['values']['XOffset'],anim['values']['YOffset'],anim['values']['AttachesTo'],anim['values']['ZAdjust']))
   raw=tiles.get(i)
   if raw:
    tmp=bytearray(raw);w,h=struct.unpack_from('<II',tmp)
    for j in range(w*h):
     off=struct.unpack_from('<I',tmp,16+j*4)[0]
     if off:struct.pack_into('<I',tmp,16+j*4,data+off)
    u.mem_write(data,bytes(tmp));u.mem_write(p+0xA4,dwords(data));u.mem_write(p+0x2E4,dwords(w&255,h&255));data+=(len(tmp)+15)&~15
  assert sr.u32(u,0xA8ED38)==t['count']
  print('native cells and tile heads ready',flush=True)
  self.activity='initial_recalc';self.sweeps=[];self.trace.clear();self.writes.clear()
  cell_rng_before={k:sr.rng_state(u,p) for k,p in self.rngs.items()}
  self.sweep()
  if cell_inputs_only:
   # Reuse the authentic Cell/type/TMP input producer without executing later
   # Terrain placement, graph construction or actor setup. No native return is
   # substituted to impose this fixture boundary.
   self.cell_inputs_rng=dict(before=cell_rng_before,after={k:sr.rng_state(u,p) for k,p in self.rngs.items()})
   self.initial_cells=self.cell_plane();self.trace.clear();self.writes.clear();self.pending.clear();self.reached={};self.activity='cell_inputs'
   return
  self.activity='terrain_place';self.terrain_placement=[]
  u.mem_write(0xA8E988,dwords(0x7EB6D4,self.allocate(1024*4),1024,1,0,10))
  for key,name,line in sections['Terrain']:
   p=self.allocate(0xE0);q=r.terrain_ptrs[name];coord=(int(key)%1000,int(key)//1000);self.call(0x71BB90,this=p,args=(q,0xB0ECF0),count=100000)
   u.mem_write(COORD,packed(*coord));u.mem_write(COORD+16,dwords(coord[0]*256+128,coord[1]*256+128,0));self.call(0x71E0D0,this=q,args=(COORD+32,COORD+16));self.call(0x5F6940,this=p,args=(COORD+32,))
   # Direct original Place_Down after constructor and coordinate callback. Display/Logic reveal excluded.
   self.call(0x5683C0,args=(COORD,p),count=300000)
   self.terrain_placement.append(dict(key=key,name=name,coord=list(coord),native_xyz=list(struct.unpack('<3i',u.mem_read(p+0x9C,12)))))
  print('native terrain placements ready',flush=True)
  if terrain_inputs_only:
   # Full_Init687A85/687A8A initializes ore queues after the existing
   # Terrain reader/place phase, before Technos and final568BB0. Reuse
   # the prepared original Cell/TMP/Terrain owner without later graphs.
   self.initial_cells=self.cell_plane();self.trace.clear();self.writes.clear();self.pending.clear();self.reached={};self.activity='terrain_inputs'
   return
  self.activity='final_recalc';self.final_tiberium_value=self.call(0x568BB0,args=(0,),count=80000000)
  print('native cell planes established',dict(self.counters),flush=True)
  self.initial_cells=self.cell_plane();self.trace.clear();self.writes.clear();self.pending.clear();self.reached={}
  self.finish_graphs();self.actor_constructor_before={k:sr.rng_state(u,p) for k,p in self.rngs.items()};self.trace.clear()
  if self.create_actor:self.build_actor(r.mtnk)
  self.actor_constructor=dict(before_rng=self.actor_constructor_before,after_rng={k:sr.rng_state(u,p) for k,p in self.rngs.items()},trace=list(self.trace));self.initial=self.state();print('native hierarchy established',[len(x['records']) for x in self.initial['graphs']],flush=True)
  self.activity='bridge'
 def finish_graphs(self,*,compute_bridge_records=False):
  """Build original graphs over this VM's already-constructed Cell state.

  This owner reuses its established vector caller-boundary headers and native
  56C510/581F90/42C1C0 chain. It does not reconstruct Cells or bridge stamps.
  """
  u=self.uc;case=self.case
  self.activity='connectivity';u.mem_write(MAP+0x18,bytes(13*4))
  if compute_bridge_records:
   from tools.spatial_oracle.bridge_records import compute_on_map
   self.bridge_record_production=compute_on_map(self)
  else:u.mem_write(MAP+0x54,dwords(0,0,1,0,10))
  for address in (0x49F0E0,0x49F190,0x49F2F0,0x49F2D0,0x49F280):self.call(address,count=10000)
  root=self.allocate(16);buckets=self.allocate(256*24);template=self.allocate(24);self.call(0x58AFF0,this=template,args=(0,0),count=1000);u.mem_write(template,dwords(0x7ED540));u.mem_write(template+16,dwords(0,20));u.mem_write(buckets,bytes(u.mem_read(template,24))*256);u.mem_write(root,dwords(buckets,0x56CB80,256,20));u.mem_write(MAP+0x14,dwords(root));self.call(0x56C510,count=60000000)
  print('native base graph established',self.nav_snapshot()['zone_count'],flush=True)
  self.activity='hierarchy';u.mem_write(0x87E8B8+0x40,bytes(9*4));template=self.allocate(24);self.call(0x58B070,this=template,args=(0,0),count=1000);u.mem_write(template,dwords(0x7ED520));u.mem_write(template+16,dwords(0,20));bucket_template=bytes(u.mem_read(template,24));w,h=case['size']
  for level in range(3):
   header=MAP+0x8C+level*24;self.call(0x58AE60,this=header,args=(0,0),count=1000);u.mem_write(header,dwords(0x7ED4A0));u.mem_write(header+16,dwords(0,w*h*4//(1<<(2*(level+1)))));buckets=self.allocate(256*24);root=self.allocate(16);u.mem_write(root,dwords(buckets,0x56CB80,256,20));u.mem_write(MAP+0x80+level*4,dwords(root));u.mem_write(buckets,bucket_template*256)
  for level in (2,1,0):self.call(0x581F90,args=(level,),count=80000000)
  self.call(0x42C1C0,this=0x87E8B8,count=10000000)
 def sweep(self):
  self.call(0x578350);coords=[]
  while True:
   p=self.call(0x578290)
   if not p:break
   coords.append(self.coord(p));self.call(0x47D2B0,this=p,args=(-1,),count=100000)
  assert {tuple(c) for c in coords}==set(self.ptrs),(len(coords),len(self.ptrs))
  self.sweeps.append(dict(activity=self.activity,count=len(coords),order_sha256=sha(b''.join(packed(*c) for c in coords))))
 def cell_plane(self):
  u=self.uc
  return [dict(coord=list(c),tile=sr.i32(u,p+0x38),subtile=u.mem_read(p+0x11A,1)[0],level=u.mem_read(p+0x11B,1)[0],slope=u.mem_read(p+0x11C,1)[0],land=sr.i32(u,p+0xEC),zone_type=sr.i32(u,p+0x4C),flags=sr.u32(u,p+0x140),occupation=u.mem_read(p+0x124,1)[0],has_ground_object=bool(sr.u32(u,p+0xE4))) for c,p in self.ptrs.items()]
 def build_actor(self,typ):
  u=self.uc;self.actor=self.allocate(0x800);self.drive=self.allocate(0x100);house=self.allocate(0x17000);u.mem_write(0xB0F720,dwords(0x7EB6D4,self.allocate(4096),1024,1,0,10));sp=STACK_BASE+STACK_SIZE-0x1000;u.mem_write(sp,dwords(RET_MAGIC,typ,0));u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,self.actor);run_checked(u,0x7353C0,0x7354CE,count=300000)
  self.call(0x4AF540,this=self.drive);u.mem_write(self.drive+0xC,dwords(self.actor));u.mem_write(self.drive+0x14,dwords(1));u.mem_write(self.actor+0x674,dwords(self.drive+4));u.mem_write(self.actor+0x21C,dwords(house));u.mem_write(self.actor+0x6C,dwords(sr.u32(u,typ+0xA0)));u.mem_write(self.actor+0x90,b'\x01');u.mem_write(self.actor+0x81,b'\0');u.mem_write(self.actor+0xAC,dwords(5));u.mem_write(self.actor+0xB4,dwords(-1));u.mem_write(self.actor+0x3D5,b'\1');u.mem_write(self.actor+0x6D8,dwords(-1));u.mem_write(self.actor+0x338,dwords(-1));u.mem_write(self.actor+0x684,b'\xff');u.mem_write(self.actor+0x9C,dwords(self.actor_coord[0]*256+128,self.actor_coord[1]*256+128,self.actor_height*sr.i32(u,0x89E7C0)));u.mem_write(self.actor+0x55C,packed(*self.actor_coord));self.call(0x4AF4A0,this=0)
 def movement_admissions(self):
  result=[];u=self.uc;before={k:sr.rng_state(u,p) for k,p in self.rngs.items()};fn=sr.u32(u,sr.u32(u,self.actor)+0x1AC);assert fn==0x73F0A0
  for c in self.admission_cells:
   p=self.ptrs[c];value=self.call(fn,this=self.actor,args=(p,4,self.actor_height,0,1),count=300000);result.append(dict(candidate=list(c),direction=4,height=self.actor_height,previous_cell=None,arg5=1,land=sr.i32(u,p+0xEC),class_result=value))
  assert before=={k:sr.rng_state(u,p) for k,p in self.rngs.items()}
  return result
 def state(self):return dict(rng={k:sr.rng_state(self.uc,p) for k,p in self.rngs.items()},movement_admissions=self.movement_admissions() if self.create_actor else [],navigation=self.nav_snapshot(),graphs=self.graph_snapshot(),dummy=self.dummy_snapshot(),cells=self.cell_plane())
 def observe(self,u,a,n,d):
  if self.phase!='measure':return super().observe(u,a,n,d)
  if self.range_pending and a==self.range_pending[-1][0]:
   _,row=self.range_pending.pop();row['result']=u.reg_read(UC_X86_REG_EAX)
  if a==self.tile_image_getter:
   p=u.reg_read(UC_X86_REG_ECX)
   if 0x44480000<=p<0x44480000+self.tile_count*0x400:
    index=(p-0x44480000)//0x400;self.used_tile_heads.add(index);assert sr.u32(u,p+0xA4),('reached primary TMP absent',index)
  if a==0x65C7E0:
   sp=u.reg_read(UC_X86_REG_ESP);this=u.reg_read(UC_X86_REG_ECX);stream=next(k for k,p in self.rngs.items() if p==this);row=dict(kind='range_request',stream=stream,minimum=sr.i32(u,sp+4),maximum=sr.i32(u,sp+8));self.trace.append(row);self.range_pending.append((sr.u32(u,sp),row));self.counters[f'{self.activity}:{stream}:range_requests']+=1
  if a==0x65C84B:self.counters[f'{self.activity}:ranged_raw_draws']+=1
  if a==0x65C780:
   stream=next(k for k,p in self.rngs.items() if p==u.reg_read(UC_X86_REG_ECX));self.counters[f'{self.activity}:{stream}:next_draws']+=1
  if a in (0x47D2B0,0x47CA80,0x483C80,0x5FDD20,0x547370,0x71C110,0x47E8A0):
   self.counters[f'{self.activity}:{a:08X}']+=1
   if a==0x47D2B0:return
  if a==0x421EA0:
   sp=u.reg_read(UC_X86_REG_ESP);self.counters[f'{self.activity}:animation_constructor_boundary']+=1;self.ret(28,u.reg_read(UC_X86_REG_ECX));return
  if a in (0x547020,0x727FD0):raise AssertionError(('uncovered dependency',self.activity,hex(a)))
  if a in (0x56C510,0x586990,0x584550,0x581F90,0x5824A0,0x42C1C0,0x578460,0x56CB90,0x56C6CB):
   self.counters[f'{self.activity}:{a:08X}']+=1
   if self.activity=='bridge':
    sp=u.reg_read(UC_X86_REG_ESP)
    if a==0x56C510:self.event('connectivity')
    elif a==0x586990:
     v=sr.u32(u,sp+4);data=sr.u32(u,v+4);count=sr.u32(u,v+16);self.event('hierarchy_batch',cells=[list(struct.unpack('<hh',u.mem_read(data+i*4,4)))for i in range(count)])
    elif a==0x584550:self.event('hierarchy_patch',coord=list(struct.unpack('<hh',u.mem_read(sr.u32(u,sp+4),4))))
    elif a==0x42C1C0:self.event('pathfinder_scratch_refresh')
    elif a==0x578460 and sr.u32(u,sp) in (0x5869BA,0x586A5B):self.event('hierarchy_cell_pass',pass_number=1 if sr.u32(u,sp)==0x5869BA else 2,coord=list(struct.unpack('<hh',u.mem_read(sr.u32(u,sp+4),4))))
   return
  if a==0x7C978A:self.ret(eax=0);return
  if a in (0x586360,0x5865E0):self.counters[f'{self.activity}:shroud_boundary']+=1;self.ret(4,1);return
  if a==0x7D140B:self.ret(eax=0x447F0000);return
  if a==0x4068E0:raise AssertionError(('native warning reached',self.activity,hex(a)))
  super().observe(u,a,n,d)
 def write(self,u,access,a,n,v,d):
  if self.activity=='bridge':return super().write(u,access,a,n,v,d)
  assert not 0x401000<=a<0x7E1000
 def run(self):
  stages=[]
  for name,fn,c in (self.stages if self.stages is not None else [('first_damage',0x57CCF0,self.case['impact']),('collapse',0x57CCF0,self.case['impact']),('repair',0x573540,self.case['start'])]):
   counts0=self.counters.copy();self.trace.clear();self.pending.clear();rng0={k:sr.rng_state(self.uc,p) for k,p in self.rngs.items()};self.uc.mem_write(COORD,packed(*c));answer=self.call(fn,args=(COORD,),count=80000000)
   stages.append(dict(name=name,returned_low_byte=answer&255,trace=list(self.trace),rng_before=rng0,rng_after={k:sr.rng_state(self.uc,p) for k,p in self.rngs.items()},state=self.state(),native_reached_delta=dict(self.counters-counts0)));assert not self.pending and not self.range_pending;assert sha(bytes(self.uc.mem_read(0x401000,0x3E0000)))==self.code_hash;print(name,'done',flush=True)
  return stages

def generate():
 t=identity.theater();r=Inputs(t);tiles,assets=extract_tiles(t);m=Navigation(r,t,tiles)
 assert not m.range_pending
 assert sha(bytes(m.uc.mem_read(0x401000,0x3E0000)))==m.code_hash
 return dict(schema=1,readers=r.snapshot(),theater=t,assets=assets,text_sha256=m.code_hash,case=m.case,sweeps=m.sweeps,terrain_placement=m.terrain_placement,final_tiberium_value=m.final_tiberium_value,fpcw=m.uc.reg_read(UC_X86_REG_FPCW),bootstrap=m.bootstrap,cell_startup=m.cell_startup,actor_constructor=m.actor_constructor,initial=m.initial,stages=m.run(),native_reached=dict(m.counters),reached_primary_tmp_heads=sorted(m.used_tile_heads))
if __name__=='__main__':finish_vectors(generate,HERE/'navigation.json.gz',provenance=lambda:provenance(scope=__doc__,assumptions=['Physical XMP03T4.MAP diamond fields, original scalar/type readers and relocated physical TEMPERATMD primary TMP bytes; original Cell constructors and Recalc derive all class/height inputs. No VERA planes or IDs supplied.','Original Terrain constructors and direct Place_Down follow physical source order; full Unlimbo, loaded world object arrays and scenario timing are excluded. Ordinary building class gates are independently native-read false; ordinary mobile RTTIs are ignored by483C80.','Actual568BB0 final initialization,56C510 and581F90(2,1,0), then original57CCF0 twice and573540 repair execute. Graph header final-vtable/growth caller stores are supplied as in the established hierarchy harness.','Main, Scenario and MapGen begin with original seeded0 complete states; the original Unit constructor prefix advances Scenario once, recorded separately. No full native scenario load/RNG claim.'],substitutions=['Successful bounded malloc/free, CRT atexit registration and TLS. Physical archive and lexical INI cache preparation remain host-side.','Waterfall Anim421EA0 returns original allocated receiver and records caller-side state; animation registration/playback/lifetime and its hidden RNG are excluded from initialization. Shroud query returns hidden; Terrain discovery does not supply graph state.','Screen/radar/dirty-rectangle sinks inherited from the frozen geometry harness. Dynamic mobiles and ordinary buildings are omitted only from class-neutral membership; this is not a full actor/world loading comparison.'],entry_points={'cell_ctor':0x47BBF0,'terrain_ctor':0x71BB90,'terrain_place':0x5683C0,'recalc':0x47D2B0,'zone':0x483C80,'final_init':0x568BB0,'connectivity':0x56C510,'hierarchy':0x581F90,'batch':0x586990,'damage':0x57CCF0,'repair':0x573540,'unit_can_enter':0x73F0A0}))
