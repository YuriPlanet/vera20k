"""Original FV MissionAttack through complete Unit/Foot Approach and Drive destination.

External next-chain evidence. Existing native VM/COM/allocation owners execute;
physical full-map facts and actor placement are supplied, not a Scenario load.
All hierarchy levels are rebuilt by original instructions and compared with
the separately preserved native live or post-load hierarchy corpus."""
import importlib.util, json, struct, hashlib, gzip
from collections import deque
from pathlib import Path
from types import SimpleNamespace
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE, UC_HOOK_MEM_READ
from unicorn.x86_const import *
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from tools.native_oracle import RET_MAGIC, run_checked, _canonical, first_difference, provenance
from tools.spatial_oracle.anytown_damage.mission import Mission
from tools.spatial_oracle.building_body_rules import SP, RULES, dwords
HERE=Path(__file__).resolve().parent
from . import range as proof
from .publication import finish_vectors

class Continuation(Mission):
 def __init__(self, row):
  captured=[];original=proof.setup
  def capture(stage):
   value=original(stage);captured.append(value);return value
  proof.setup=capture
  try:self.prefix=proof.execute(row)
  finally:proof.setup=original
  self.m,self.src,self.typ,self.weapon,self.cells,self.inputs,self.projection=captured[0]
  self.u=self.m.u;self.trace=deque(maxlen=200);self.events=[];self.pending={};self.frame=self.m.read32(0xA8ED84);self.phase='pursuit';self.bullets=[];self.shots=[];self.frames=[];self.impacts=[]
  self.resident=SimpleNamespace(rngs={'main':0x886B88,'scenario':self.m.read32(0xA8B230)+0x218,'mapgen':0xABE890})
  self.os_owner=True;self.writes=[];self.com=[];self.queries=[];self.ranges=[];self.range_pending={}
  self.u.hook_add(UC_HOOK_CODE,self.observe)
  self.u.hook_add(UC_HOOK_MEM_WRITE,self.written)
  self.rules_reads={};rules_ptr=self.m.read32(0x8871E0);self.u.hook_add(UC_HOOK_MEM_READ,self.read_rules,begin=rules_ptr,end=rules_ptr+0x1FFF)
  self.flow=[];self.flow_pending={};self.candidates=[];self.geometry=[];self.hierarchy=[];self.unsafe_cells=set()
 def observe(self,u,a,n,d):
  if self.phase=='graph_setup':
   if a in (0x581F90,0x581F50,0x42C1C0):self.events.append(dict(kind='graph_setup',pc=hex(a)))
   return
  sp=u.reg_read(UC_X86_REG_ESP);m=self.m
  if not self.os_owner:self.trace.append(a)
  if a in self.range_pending:
   for event in self.range_pending.pop(a):event['returned_al']=u.reg_read(UC_X86_REG_EAX)&255
  if a==0x6F7220:
   srcp,target,weapon=struct.unpack('<3I',u.mem_read(sp+4,12));event=dict(caller=f'{m.read32(sp):08x}',source_xyz=list(struct.unpack('<3i',u.mem_read(srcp,12))),target=f'{target:08x}',weapon=m.string(weapon+0x24));self.ranges.append(event);self.range_pending.setdefault(m.read32(sp),[]).append(event)
  if a in (0x565730,0x5657A0):
   p=m.read32(sp+4)
   xy=tuple(struct.unpack('<2i',u.mem_read(p,8))) if a==0x565730 else tuple(struct.unpack('<2h',u.mem_read(p,4)))
   if a==0x565730:xy=tuple(v//256 if v>=0 else -((-v)//256) for v in xy)
   self.queries.append(dict(pc=f'{a:08x}',caller=f'{m.read32(sp):08x}',xy=xy))
  if self.phase in ('pursuit','control'):
   self.observe_flow(a,sp)
  if a in (0x55A950,0x55A965,0x55A96B,0x55A970,0x55A987,0x55A98D,0x55A9A4,0x55A5C0,0x55A750,0x6C4010):
   self.com.append(dict(pc=f'{a:08x}',ecx=f'{u.reg_read(UC_X86_REG_ECX):08x}',esp=f'{sp:08x}',stack=[f'{v:08x}' for v in struct.unpack('<6I',u.mem_read(sp,24))],iat_inc=f'{m.read32(0x7E11C8):08x}',iat_dec=f'{m.read32(0x7E11CC):08x}'))
  if self.os_owner:
   super().observe(u,a,n,d)
  elif a>=0x7E1000 and a!=RET_MAGIC:
   raise AssertionError(('non-image-code',hex(a)))
 def observe_flow(self,a,sp):
  m=self.m;u=self.u;this=u.reg_read(UC_X86_REG_ECX)
  if a in self.flow_pending:
   for e in self.flow_pending.pop(a):
    if e['kind']=='hierarchy_preflight':
     counts=[m.read32(0x87E8B8+0xC74+i*4)for i in range(3)];assert all(n<=500 for n in counts),counts;e['path_counts']=counts;e['paths']=[list(struct.unpack('<'+'H'*n,u.mem_read(0x87E8B8+0xBC+i*1000,n*2)))for i,n in enumerate(counts)]
    e['returned_eax']=u.reg_read(UC_X86_REG_EAX);e['returned_signed']=struct.unpack('<i',dwords(e['returned_eax']))[0];e['returned_al']=e['returned_eax']&255
  names={0x7414E0:('unit_approach',1),0x4D5690:('foot_approach',1),0x578460:('map_bounds',2),0x56D230:('movement_zone',3),0x4834A0:('cell_admission',7),0x42D170:('path_cost',6),0x42C290:('hierarchy_preflight',4),0x741970:('unit_destination',2),0x4D94B0:('foot_destination',2),0x4AFD40:('drive_destination',4),0x6FCDB0:('target_assignment',1)}
  if a in names:
   kind,count=names[a];args=list(struct.unpack('<'+'I'*count,u.mem_read(sp+4,count*4)))
   e=dict(kind=kind,pc=f'{a:08x}',caller=f'{m.read32(sp):08x}',this=f'{this:08x}',args=args)
   if a in (0x578460,0x56D230):e['cell_xy']=list(struct.unpack('<hh',u.mem_read(args[0],4)))
   if a==0x4834A0:
    xy=tuple(struct.unpack('<hh',u.mem_read(this+0x24,4)));e.update(cell_xy=list(xy),cell_land=m.read32(this+0xEC),cell_flags=m.read32(this+0x140));self.guard_cell(xy)
   if a==0x42C290:e.update(source_cell=list(struct.unpack('<hh',u.mem_read(args[0],4))),destination_cell=list(struct.unpack('<hh',u.mem_read(args[1],4))))
   if a==0x42D170:e.update(from_cell=list(struct.unpack('<hh',u.mem_read(args[0],4))),to_cell=list(struct.unpack('<hh',u.mem_read(args[1],4))))
   if a in (0x741970,0x4D94B0):e['destination_cell']=list(struct.unpack('<hh',u.mem_read(args[0]+0x24,4))) if args[0] else None
   if a==0x4AFD40:e['destination_xyz']=[struct.unpack('<i',dwords(v))[0]for v in args[1:4]]
   self.flow.append(e);self.flow_pending.setdefault(m.read32(sp),[]).append(e)
  if a in (0x4D56E4,0x4D56FC,0x4D5BAA,0x4D5BCD,0x4D5BEE,0x4D5CBC):self.geometry.append(dict(pc=f'{a:08x}',eax=u.reg_read(UC_X86_REG_EAX),range_or_radius=struct.unpack('<i',u.mem_read(sp+0x38,4))[0],facing_byte=m.read32(sp+0xD0),source_range_adjusted=struct.unpack('<i',u.mem_read(sp+0x64,4))[0]))
  if a==0x42C3B9:self.hierarchy.append(dict(kind='level_begin',level=u.reg_read(UC_X86_REG_ESI),source_id=u.reg_read(UC_X86_REG_EBX),target_id=m.read32(sp+0x3C)))
  if a==0x42C4EB:
   node=u.reg_read(UC_X86_REG_EAX);self.hierarchy.append(dict(kind='heap_pop',level=m.read32(sp+0x38),zone_id=m.read32(node+4),cost_bits=bytes(u.mem_read(node+8,4)).hex(),depth=m.read32(node+12)))
  if a==0x42C543:
   edge=u.reg_read(UC_X86_REG_EAX);self.hierarchy.append(dict(kind='edge',level=m.read32(sp+0x38),source_id=m.read32(sp+0x48),target_id=m.read32(edge),slope=u.mem_read(edge+4,1)[0]))
  if a==0x4D5FFE:
   p=m.read32(sp+0x24);self.candidates.append(dict(kind='radial_probe',pc=f'{a:08x}',radius=struct.unpack('<i',u.mem_read(sp+0x64,4))[0],base_direction=m.read32(sp+0xD0),direction_offset=struct.unpack('<i',u.mem_read(p,4))[0],direction_index=(p-0x8224DC)//4))
  if a==0x4D6081:self.candidates[-1]['unsnapped_xyz']=[u.reg_read(UC_X86_REG_EAX),m.read32(sp+0x38),u.reg_read(UC_X86_REG_EDI)]
  if a==0x4D61A4:self.candidates[-1]['candidate_xyz']=list(struct.unpack('<iii',u.mem_read(sp+0x2C,12)))
  if a==0x4D6469:self.candidates.append(dict(kind='neighbor_probe',pc=f'{a:08x}',neighbor_index=u.reg_read(UC_X86_REG_EDI),base_cell=list(struct.unpack('<hh',u.mem_read(sp+0x14,4)))))
  if a==0x4D64CE:self.candidates[-1]['candidate_xyz']=list(struct.unpack('<iii',u.mem_read(sp+0x2C,12)))
 def guard_cell(self,xy):
  if hasattr(self,'cell_facts') and xy in self.cell_facts:
   f=self.cell_facts[xy]
   assert not f['occupation'] and not f['has_ground_object'],('Unprojected live occupant',xy,f)
  elif hasattr(self,'cell_facts'):raise AssertionError(('Queried outside allocated physical cells',xy))
 def read_rules(self,u,access,addr,size,value,data):
  if self.phase != 'pursuit':return
  base=self.m.read32(0x8871E0)
  if base<=addr<base+0x2000:
   pc=u.reg_read(UC_X86_REG_EIP);self.rules_reads[pc,addr-base,size]=bytes(u.mem_read(addr,size)).hex()
 def written(self,u,access,addr,size,value,data):
  assert addr+size<=proof.TEXT_START or addr>=proof.TEXT_START+proof.TEXT_SIZE
  if self.phase=='graph_setup':return
  loco=self.m.read32(self.src+0x674)-4
  if self.src<=addr<self.src+0x1000 or loco<=addr<loco+0x100:
   self.writes.append(dict(pc=f'{u.reg_read(UC_X86_REG_EIP):08x}',owner='source' if self.src<=addr<self.src+0x1000 else 'drive',offset=addr-(self.src if self.src<=addr<self.src+0x1000 else loco),size=size,value=value))
 def snapshot(self):
  m=self.m;u=self.u;p=self.src;loco=m.read32(p+0x674)-4
  return dict(actor=self.state(),nav=f'{m.read32(p+0x5A4):08x}',loco=f'{loco:08x}',loco_ref=m.read32(loco+0x14),actor_bytes=bytes(u.mem_read(p,0x1000)).hex(),drive_bytes=bytes(u.mem_read(loco,0x100)).hex(),rng={k:proof.state(m,v) for k,v in self.resident.rngs.items()})
 def plane(self):
  packet=json.loads(gzip.decompress(proof.FACTS.read_bytes()))
  stage=self.prefix['input'].get('stage','healthy')
  state=packet['initial'] if stage=='healthy' else packet['stages'][{'damaged':0,'collapsed':1,'repaired':2}[stage]]['state']
  n=state['navigation'];width=sum(packet['case']['size']);side=width+1;u=self.u;m=self.m
  from tools.spatial_oracle.map_queries import normalize
  self.bounds=normalize(packet['case']['size'],packet['case']['local_size'])
  u.mem_write(0x87F7E8+0xFC,dwords(*self.bounds['normalized']))
  u.mem_map(0x44000000,0x800000);base=0x44200000;plane=0x44300000
  self.graph_source_state=state;self.graph_case=packet['case'];self.width=width;self.side=side
  nodes=bytearray(b'\x07\x00\x00\x00'*(side*side))
  for y in range(width):
   for x in range(width):
    i=y*width+x;struct.pack_into('<BBH',nodes,(y*side+x)*4,n['classes'][i],n['levels'][i],n['base_ids'][i])
  u.mem_write(base,bytes(nodes));u.mem_write(plane,bytes(side*side*10));u.mem_write(0x87F7E8+0x68,dwords(base,side*side,plane))
  u.mem_write(0x87F7E8+0x4C,dwords(n['zone_count']+1))
  for i,row in enumerate(n['raw_rows']):
   raw=struct.pack('<'+'H'*len(row),*row);p=m.alloc(len(raw));u.mem_write(p,raw);u.mem_write(0x87F7E8+0x18+i*4,dwords(p))
  self.plane_input=dict(source_packet_sha256=proof.sha(proof.FACTS.read_bytes()),native_stage=stage,base_nodes_sha256=proof.sha(bytes(nodes)),width=width,side=side,zone_count=n['zone_count'],boundary='Frozen native full plane/rows are supplied inputs. No connectivity algorithm or candidate result is replaced; all runtime calls execute.')
 def pathfinder(self):
  from tools.spatial_oracle.naval_head_producer import Head
  from tools.rules_oracle.bridge_anim_lists import Lists
  from types import MethodType
  m=self.m;u=self.u;self.phase='setup'
  u.mem_map(0x48000000,0x1000000);m.long_heap=0x48000000;m.path_events=[]
  proxy=Head.__new__(Head);proxy.__dict__=m.__dict__
  old=Lists.hook
  def allocator(owner,uc,pc,n,d):
   if owner is m and pc in (0x7C8E17,0x7C8B3D):return Head.hook(proxy,uc,pc,n,d)
   return old(owner,uc,pc,n,d)
  self.old_allocator_hook=old;Lists.hook=allocator;m.alloc=MethodType(Head.alloc,m)
  self.heap_owner=proxy
  for a in (0x49F0E0,0x49F190,0x49F2F0,0x49F2D0,0x49F280,0x49F3A0):m.invoke(a,0)
  m.invoke(0x42A6D0,0x87E8B8)
  m.invoke(0x42AC00,0x87E8B8,(0x87F7E8+0xEC,))
  m.invoke(0x42C1C0,0x87E8B8)
  self.phase='pursuit'
 def graphs(self):
  from tools.spatial_oracle.shrapnel_repair.zone_composition import ConnectivityRepair
  from tools.spatial_oracle.shrapnel_repair.hierarchy_composition import HierarchyRepair
  m=self.m;u=self.u;self.phase='graph_setup';self.graph_rng_before={k:proof.state(m,p) for k,p in self.resident.rngs.items()}
  # Replay original hierarchy writers over frozen native class/level/Cell inputs.
  # These full-map Cell bodies are geometry inputs; terrain actors are excluded.
  u.mem_map(0x50000000,0x800000);next_cell=0x50000000;table=m.read32(0x87F7E8+0x13C)
  u.mem_write(table,bytes(0x100000));overlays={tuple(c[:2]):c[5] for c in self.graph_case['cells']}
  packet=json.loads(gzip.decompress(proof.FACTS.read_bytes()));stage=self.prefix['input'].get('stage','healthy')
  if stage!='healthy':
   for step in packet['stages'][:{'damaged':1,'collapsed':2,'repaired':3}[stage]]:
    for event in step['trace']:
     if event['kind']=='overlay':overlays[tuple(event['coord'])]=event['value']
  self.cell_facts={tuple(f['coord']):f for f in self.graph_source_state['cells']}
  for f in self.graph_source_state['cells']:
   xy=tuple(f['coord']);p=self.cells.get(xy)
   if p is None:
    p=next_cell;next_cell+=0x200;m.invoke(0x47BBF0,p);self.cells[xy]=p
   u.mem_write(p+0x24,struct.pack('<hh',*xy));u.mem_write(p+0x38,dwords(f['tile']));u.mem_write(p+0x44,dwords(overlays[xy] if overlays[xy] is not None else -1));u.mem_write(p+0x4C,dwords(f['zone_type']));u.mem_write(p+0xEC,dwords(f['land']));u.mem_write(p+0x11A,bytes((f['subtile'],f['level'],f['slope'])));u.mem_write(p+0x140,dwords(f['flags']))
   u.mem_write(table+(xy[1]*512+xy[0])*4,dwords(p))
  print('Projected native Cell geometry',len(self.cells),flush=True)
  map_ptr=0x87F7E8;u.mem_write(map_ptr+0x54,dwords(0,0,1,0,10));u.mem_write(0x87E8B8+0x40,bytes(9*4))
  template=m.alloc(24);m.invoke(0x58B070,template,(0,0));u.mem_write(template,dwords(0x7ED520));u.mem_write(template+16,dwords(0,20));raw=bytes(u.mem_read(template,24));w,h=self.graph_case['size']
  for level in range(3):
   header=map_ptr+0x8C+level*24;m.invoke(0x58AE60,header,(0,0));u.mem_write(header,dwords(0x7ED4A0));u.mem_write(header+16,dwords(0,w*h*4//(1<<(2*(level+1)))));buckets=m.alloc(256*24);root=m.alloc(16);u.mem_write(root,dwords(buckets,0x56CB80,256,20));u.mem_write(map_ptr+0x80+level*4,dwords(root));u.mem_write(buckets,raw*256)
  proxy=SimpleNamespace(uc=u,width=self.width,side=self.side)
  for level in (2,1,0):
   print('Building original hierarchy',level,flush=True)
   ConnectivityRepair.call(proxy,0x581F90,this=map_ptr,args=(level,),count=80000000)
  ConnectivityRepair.call(proxy,0x42C1C0,this=0x87E8B8,count=10000000)
  self.built_graphs=HierarchyRepair.graph_snapshot(proxy)
  self.graph_exact=self.built_graphs==self.graph_source_state['graphs']
  reference=self.graph_source_state['graphs']
  self.graph_reference=dict(kind='live_initial',file=str(proof.FACTS),sha256=proof.sha(proof.FACTS.read_bytes()))
  if stage!='healthy':
   restore=proof.REPO/'tools/spatial_oracle/anytown_navigation_restore.json.gz';restored=json.loads(gzip.decompress(restore.read_bytes()));reference=restored['cases'][{'damaged':0,'collapsed':1,'repaired':2}[stage]]['after']['graphs']
   self.graph_reference=dict(kind='post_load_rebuilt',file=str(restore),sha256=proof.sha(restore.read_bytes()))
  assert self.built_graphs==reference,first_difference(reference,self.built_graphs)
  self.graph_reference['graphs_payload_sha256']=proof.sha(_canonical(self.built_graphs))
  assert self.graph_rng_before=={k:proof.state(m,p) for k,p in self.resident.rngs.items()}
  print('Native graph sizes',[len(g['records']) for g in self.built_graphs],'exact_live_reference',self.graph_exact,flush=True)
  self.phase='pursuit'
 def construct(self,full=False):
  m=self.m;u=self.u;self.phase='setup';old=self.src
  u.mem_map(0,0x1000);u.mem_write(0,dwords(-1))
  m.invoke(0x4E7CF0,0)
  self.constructor_layers=[]
  land_names={m.string(m.read32(0x839D68+i*4)) for i in range(12)}
  for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','XMP03T4.MAP'):
   path=proof.MAP if name=='XMP03T4.MAP' else proof.assets_root()/name
   if not path.exists():self.constructor_layers.append(dict(file=name,absent=True));continue
   raw=path.read_bytes();sections,lines=proof.lexical(raw,{'FV','Attack','Guard','Sleep','Move','General','AI'}|land_names)
   m.rules_cache(sections)
   m.invoke(0x674000,0,(RULES,))
   if full:m.invoke(0x66D530,m.read32(0x8871E0),(RULES,))
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,RULES);run_checked(u,0x679C92,0x679CAF)
   if 'FV' in sections and full:m.invoke(0x747620,self.typ,(RULES,))
   if 'FV' in sections:
    for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,self.typ),(UC_X86_REG_EBX,self.typ+0x24),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EDI,RULES)):u.reg_write(reg,v)
    run_checked(u,0x7123ED,0x71243A)
   self.constructor_layers.append(dict(file=name,sha256=proof.sha(raw),source_lines=lines,land_table_hex=bytes(u.mem_read(0x89EA40,12*36)).hex(),speed_type=m.read32(self.typ+0x67C),locomotor=bytes(u.mem_read(self.typ+0x34C,16)).hex()))
  for p in (0xB0F720,0x8B4108,0xA8EC78,0x8B3DC0,0xB0F5D8,0xB0F6C8):u.mem_write(p,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
  self.src=m.alloc(0x1000);self.constructor_before_rng={k:proof.state(m,p) for k,p in self.resident.rngs.items()}
  m.invoke(0x7353C0,self.src,(self.typ,0))
  self.constructor_after=self.snapshot()
  # Explicit supplied placement/Human House, inherited from the range fixture.
  for off,size in ((0x21C,4),(0x9C,12),(0x8C,1),(0x2B4,4),(0x74,1),(0x81,1),(0x90,1),(0x3D5,1)):
   u.mem_write(self.src+off,bytes(u.mem_read(old+off,size)))
  u.mem_write(self.src+0xAC,dwords(5))
  facing=m.alloc(4);m.invoke(0x5F3DB0,self.src,(facing,m.read32(self.src+0x2B4)))
  for field in (0x388,0x3A0):m.invoke(0x4C9300,self.src+field,(facing,))
  self.phase='pursuit'
 def run(self,mission=None,ref=False,construct=False,plane=False,full=False,pathfinder=False,graphs=False,entry=0x7414E0):
  if plane:self.plane()
  if construct:self.construct(full)
  if pathfinder:self.pathfinder()
  if graphs:self.graphs()
  m=self.m;u=self.u;loco=m.read32(self.src+0x674)-4
  if ref:
   # Original COM AddRef is stdcall, as in the existing Unit Scatter owner.
   u.mem_write(SP,dwords(RET_MAGIC,loco));u.reg_write(UC_X86_REG_ESP,SP)
   run_checked(u,0x55A950,RET_MAGIC)
  if mission is not None:
   m.invoke(0x5B35E0,self.src,(mission,1))
   m.invoke(0x5B3570,self.src)
  before=self.snapshot();self.writes=[];start=len(self.events);failure=None
  try:answer=m.invoke(entry,self.src,(0,) if entry==0x7414E0 else ())
  except Exception as e:
   answer=None
   cs=Cs(CS_ARCH_X86,CS_MODE_32)
   failure=dict(error=str(e),pc=f'{u.reg_read(UC_X86_REG_EIP):08x}',trace=[f'{a:08x} '+ '; '.join(f'{i.mnemonic} {i.op_str}' for i in cs.disasm(bytes(u.mem_read(a,15)),a,count=1)) for a in self.trace])
  assert self.prefix['text_sha256']==proof.sha(bytes(u.mem_read(proof.TEXT_START,proof.TEXT_SIZE)))
  return dict(rules_reads=[dict(pc=f'{pc:08x}',offset=off,size=size,hex=value)for (pc,off,size),value in self.rules_reads.items()],row=self.prefix['input'],os_owner=self.os_owner,queue_mission=mission,owned_loco_reference=ref,before=before,after=self.snapshot(),returned=answer,failure=failure,com=self.com,events=self.events,event_start=start,writes=self.writes,physical_inputs=self.inputs,projection=self.projection,text_sha256=self.prefix['text_sha256'],queries=self.queries,ranges=self.ranges,graph_exact=getattr(self,'graph_exact',None),graph_counts=[len(g['records']) for g in self.built_graphs] if hasattr(self,'built_graphs') else None,plane_input=getattr(self,'plane_input',None),flow=self.flow,candidates=self.candidates,geometry=self.geometry,hierarchy=self.hierarchy,graph_reference=self.graph_reference,constructor_layers=getattr(self,'constructor_layers',None),constructor_before_rng=getattr(self,'constructor_before_rng',None),constructor_after=getattr(self,'constructor_after',None))

 def close(self):
  from tools.rules_oracle.bridge_anim_lists import Lists
  Lists.hook=self.old_allocator_hook
 def checkpoint(self):
  # Restore all CPU/mapped bytes between controls, including graph scratch.
  self.saved_context=self.u.context_save()
  self.saved_memory=[(a,bytes(self.u.mem_read(a,b-a+1))) for a,b,_ in self.u.mem_regions()]
  self.saved_heap=self.m.long_heap
 def reset(self):
  for a,raw in self.saved_memory:self.u.mem_write(a,raw)
  self.u.context_restore(self.saved_context);self.m.long_heap=self.saved_heap
  self.events=[];self.pending={};self.writes=[];self.com=[];self.queries=[];self.ranges=[];self.range_pending={};self.flow=[];self.flow_pending={};self.candidates=[];self.geometry=[];self.hierarchy=[];self.rules_reads={};self.trace.clear();self.phase='setup'
 def execute_case(self,row):
  self.reset();m=self.m;u=self.u;self.prefix['input']=row
  source_xy=tuple(row.get('source_cell',[87,48]));target_xy=tuple(row.get('target',[87,54]));self.guard_cell(source_xy);self.guard_cell(target_xy)
  out=m.alloc(12);m.invoke(0x486840,self.cells[source_xy],(out,));xyz=list(struct.unpack('<iii',u.mem_read(out,12)));xyz[1]+=row.get('source_y_delta',0)
  u.mem_write(self.src+0x9C,dwords(*xyz));u.mem_write(self.src+0x2B4,dwords(self.cells[target_xy]))
  facing=m.alloc(4);m.invoke(0x5F3DB0,self.src,(facing,self.cells[target_xy]))
  for field in (0x388,0x3A0):m.invoke(0x4C9300,self.src+field,(facing,))
  self.phase='control';controls=[]
  for name,address,args in [('can_fire_at',0x6F77B0,(self.cells[target_xy],0)),('unit_fire_error',0x740FD0,(self.cells[target_xy],0,1)),('unit_fire_error_no_range',0x740FD0,(self.cells[target_xy],0,0))]:
   rng={k:proof.state(m,p)for k,p in self.resident.rngs.items()};actor=bytes(u.mem_read(self.src,0x1000));rstart=len(self.ranges);answer=m.invoke(address,self.src,args)
   if RET_MAGIC in self.pending:self.pending.pop(RET_MAGIC)['returned_eax']=answer
   assert actor==bytes(u.mem_read(self.src,0x1000));assert rng=={k:proof.state(m,p)for k,p in self.resident.rngs.items()}
   controls.append(dict(name=name,returned=answer&255 if name=='can_fire_at' else answer,ranges=self.ranges[rstart:]))
  self.phase='pursuit';self.ranges=[];self.range_pending={};self.flow=[];self.flow_pending={};self.candidates=[];self.geometry=[];self.hierarchy=[];self.queries=[]
  result=self.run(mission=1,entry=0x5B3060)
  if result['failure'] is not None:(HERE/'failure.json').write_text(json.dumps(result,indent=2)+'\n')
  assert result['failure'] is None,result['failure']
  assert not self.pending and not self.flow_pending and not self.range_pending,(self.pending,self.flow_pending,self.range_pending)
  result.update(controls=controls,source_xyz=xyz)
  # Keep semantic state adjacent to the full byte witness.
  for name in ('before','after'):
   snap=result[name];actor=bytes.fromhex(snap['actor_bytes']);drive=bytes.fromhex(snap['drive_bytes']);ptr=int(snap['nav'],16)
   snap['nav_cell']=list(struct.unpack('<hh',u.mem_read(ptr+0x24,4))) if ptr else None
   snap['drive_destination']=list(struct.unpack_from('<iii',drive,0x34));snap['drive_head']=list(struct.unpack_from('<iii',drive,0x40));snap['path']=list(struct.unpack_from('<24i',actor,0x5E0));snap['blockage_timer']=[struct.unpack_from('<i',actor,off)[0]for off in (0x668,0x670)];snap['movement_timer']=[struct.unpack_from('<i',actor,off)[0]for off in (0x640,0x648)]
  print('PASS case',row['name'],'result',result['returned'],'nav',result['after']['nav_cell'],'candidates',len(result['candidates']),flush=True)
  return result


def generate():
 stages=[]
 for stage in ('healthy','damaged','collapsed','repaired'):
  q=Continuation(dict(name='prepare_'+stage,stage=stage));q.plane();q.construct(True);q.pathfinder();q.graphs();q.checkpoint()
  rows=[dict(name=stage+'_maximum_exact',stage=stage),dict(name=stage+'_maximum_outside_two',stage=stage,source_y_delta=-2)]
  if stage=='healthy':
   rows.extend([dict(name='maximum_inside_one',stage=stage,source_y_delta=1),dict(name='maximum_outside_one',stage=stage,source_y_delta=-1),dict(name='minimum_exact',stage=stage,source_cell=[87,53]),dict(name='minimum_inside_one',stage=stage,source_cell=[87,53],source_y_delta=1),dict(name='minimum_outside_one',stage=stage,source_cell=[87,53],source_y_delta=-1)])
  stages.append(dict(stage=stage,results=[q.execute_case(row)for row in rows]))
  # Only the current machine may use Head's existing allocator override.
  q.close()
  del q
  __import__('gc').collect()
 return dict(schema=1,original_constants=original_constants(),stages=stages)


def original_constants():
 from tools.native_oracle import image_bytes,file_span
 raw=image_bytes();rows=[]
 for address,size in ((0x7E9228,8),(0x7E9250,8),(0x7E2810,8),(0x7E2818,8),(0x7E2820,8),(0x7E9238,8),(0x8224DC,100)):
  offset,data=file_span(raw,address,size);row=dict(address=f'{address:08x}',file_offset=offset,bytes=data.hex())
  if size==8:row['binary64']=struct.unpack('<d',data)[0]
  else:row['signed_offsets']=list(struct.unpack('<25i',data))
  rows.append(row)
 return rows


def metadata():
 result=provenance(scope=__doc__,assumptions=[
  'Existing bridge_target_composed/FV range preparation supplies physical native type/weapon/projectile inputs and original CRT/WinMain FPCW0E3F. Full UnitType747620, General66D530, land674000, MissionControl679C92 and Locomotor7123ED reads run over physicalRULESMD,optionalLANGRULE,MPBattleMD,andAnytown map before original Unit7353C0 constructor. Full Scenario/House/type discovery is excluded.',
  'Existing Mission OS boundary runs actual native Drive factory/constructor and retained COM ownership. Null-House Unit construction is followed by explicitly supplied human House, alive/unlimbo/marked state, source position, targetCell and stationary facings. Native UnitUnlimbo, command dispatch, Cell membership and paid movement are excluded.',
  'Full physical 13515Cell map geometry,class/level/base IDs and13movementrows are supplied from frozen original Anytown packet. The native47BBF0 constructor executes for projected Cells; live Terrain actor pointers/occupation are not projected. Any reached Cell admission with such occupancy fails closed.',
  'Native581F90 builds all3 complete hierarchy levels and42C1C0 scratch. Healthy graphs match frozen live initialization exactly; damaged/collapsed/repaired graphs must match separately executed native post-load canonical rebuild corpus exactly, not live incremental IDs/record order. Full32-bit node tails are created by native constructors, never synthesized from incomplete JSON.',
  'Each control restores the full mapped VM memory and CPU state prepared for its stage. Actual source CellGetCoords and facing setters establish input pose; optional +/-1/2Y offsets are explicit boundary controls. ActualQueueMission5B35E0,Commence5B3570 then fullMissionDispatch5B3060 run at supplied frame173. There is no intervening UnitAI/firing/Drive traversal.',
  'Source/Drive complete bytes, three full RNG streams, executable draws, ordered queries/candidate checks, target/nav/timer writes are retained. Timer middle dwords are raw native scratch/padding, not assigned gameplay meaning.',
 ],substitutions=[
  'Reuse existing native readers, heap, physical input caches and Mission Windows/COM import seams. No range, candidate, admission, hierarchy, path cost, destination, mission, timer or RNG result is replaced. Original complete.text hash and code-write guard remain enforced.',
  'Head.alloc/Head.hook successful bounded allocator is reused for larger pathfinder/graph arrays. No custom VM, allocator algorithm, or graph/path algorithm is added. Inherited type visual5F8110/5F8CE0 boundaries return no shape during setup only.',
 ],entry_points=dict(unit_ctor=0x7353C0,drive_factory=0x6C4010,mission_dispatch=0x5B3060,mission_attack=0x4D4DC0,unit_approach=0x7414E0,foot_approach=0x4D5690,range=0x6F7220,cell_admission=0x4834A0,path_cost=0x42D170,hierarchy_preflight=0x42C290,unit_destination=0x741970,foot_destination=0x4D94B0,drive_destination=0x4AFD40,attack_rng=0x65C7E0))
 result['native_packets']={str(p.relative_to(proof.REPO)):proof.sha(p.read_bytes())for p in (proof.FACTS,proof.REPO/'tools/spatial_oracle/anytown_navigation_restore.json.gz')}
 return result


if __name__=='__main__':finish_vectors(generate,HERE/'pursuit.json.gz',provenance=metadata)
