"""Original full low repair suffix with native-built initial hierarchy on supplied live-cell planes."""
import re
from .zone_composition import *
from tools.spatial_oracle.bridge_rim import TABLE

class HierarchyRepair(ConnectivityRepair):
 def __init__(self,case,rules,theater):
  super().__init__(case,rules,theater);self.stage='native_hierarchy';u=self.uc;n=self.navigation
  for key in ('live_cell_levels','live_cell_slopes','live_cell_allocated'):assert len(n[key])==self.width*self.width
  _,physical=physical_map();table=bytearray(0x100000)
  allocated={(x,y) for y in range(self.width) for x in range(self.width) if n['live_cell_allocated'][y*self.width+x]}
  assert set(physical)==allocated
  assert all(physical[x,y]['level']==n['live_cell_levels'][y*self.width+x] for x,y in allocated)
  self.physical_live_check=dict(physical_cells=len(physical),allocated_membership_exact=True,all_cell_levels_exact=True)
  for y in range(self.width):
   for x in range(self.width):
    i=y*self.width+x
    if not n['live_cell_allocated'][i]:continue
    if (x,y) not in self.ptrs:
     p=CELLS+len(self.ptrs)*0x200;self.ptrs[x,y]=p;self.coords[p]=(x,y)
     a=physical.get((x,y));u.mem_write(p+0x24,packed(x,y));u.mem_write(p+0x38,dwords(a['tile'] if a else -1));u.mem_write(p+0x44,dwords(a['overlay'] if a and a['overlay'] is not None else -1))
     if a:u.mem_write(p+0x11A,bytes([a['subtile']]));u.mem_write(p+0x11E,bytes([a['frame']]))
    p=self.ptrs[x,y];u.mem_write(p+0x11B,bytes([physical[x,y]['level'],n['live_cell_slopes'][i]]));u.mem_write(p+0x116,b'\xff\xff')
    struct.pack_into('<I',table,(y*512+x)*4,p)
  u.mem_write(TABLE,bytes(table));u.mem_write(0x87E8B8+0x40,bytes(9*4))
  dummy=re.fullmatch(r'SharedCellDummySnapshot \{ coord: \((-?\d+), (-?\d+)\), level: (\d+), slope_type: (\d+), bridge_flags_0x1180: (\d+) \}',n['dummy'])
  assert dummy,n['dummy'];dx,dy,dz,ds,df=map(int,dummy.groups());u.mem_write(DUMMY+0x24,packed(dx,dy));u.mem_write(DUMMY+0x11B,bytes([dz,ds]));u.mem_write(DUMMY+0x140,dwords(df));u.mem_write(DUMMY+0x116,b'\xff\xff')
  template=self.allocate(24);self.call(0x58B070,this=template,args=(0,0),count=1000)
  u.mem_write(template,dwords(0x7ED520));u.mem_write(template+16,dwords(0,20));bucket_template=bytes(u.mem_read(template,24))
  w,h=case['size']
  for level in range(3):
   header=MAP+0x8C+level*24;self.call(0x58AE60,this=header,args=(0,0),count=1000)
   u.mem_write(header,dwords(0x7ED4A0));u.mem_write(header+16,dwords(0,w*h*4//(1<<(2*(level+1)))))
   buckets=self.allocate(256*24);root=self.allocate(16);u.mem_write(root,dwords(buckets,0x56CB80,256,20));u.mem_write(MAP+0x80+level*4,dwords(root));u.mem_write(buckets,bucket_template*256)
  for level in (2,1,0):self.call(0x581F90,args=(level,),count=50000000)
  self.call(0x42C1C0,this=0x87E8B8,count=5000000)
  self.initial_graphs=self.graph_snapshot();self.initial_reached=dict(self.reached);self.initial_dummy=self.dummy_snapshot()
 def observe(self,u,address,size,data):
  if self.phase=='measure' and address in (0x586990,0x584550,0x581F90,0x5824A0,0x42C1C0,0x578460):
   self.reached[hex(address)]=self.reached.get(hex(address),0)+1
   sp=u.reg_read(UC_X86_REG_ESP)
   if address==0x586990:
    v=u32(u,sp+4);p=u32(u,v+4);count=u32(u,v+16);self.event('rebuild',cells=[list(struct.unpack('<hh',u.mem_read(p+i*4,4))) for i in range(count)])
   elif address==0x584550:self.event('patch',coord=list(struct.unpack('<hh',u.mem_read(u32(u,sp+4),4))))
   elif address==0x578460 and u32(u,sp) in (0x5869BA,0x586A5B):self.event('batch_query',pass_number=1 if u32(u,sp)==0x5869BA else 2,coord=list(struct.unpack('<hh',u.mem_read(u32(u,sp+4),4))))
   return
  if self.phase=='measure' and address==0x47D2B0:
   coord=self.coord(u.reg_read(UC_X86_REG_ECX));assert 114<=coord[0]<=116 and 58<=coord[1]<=60,('uncovered Recalc receiver',coord)
  super().observe(u,address,size,data)
 def graph_snapshot(self):
  u=self.uc;plane=bytes(u.mem_read(PLANE,self.side*self.side*10));graphs=[]
  for level in range(3):
   count=u32(u,MAP+0x74+level*4);header=MAP+0x8C+level*24;assert count==u32(u,header+16)<=u32(u,header+8)
   p=u32(u,header+4);records=[]
   for zone in range(count):
    node=p+zone*36;edge_count=u32(u,node+16);edge_pointer=u32(u,node+4)
    records.append(dict(parent=struct.unpack('<H',u.mem_read(node+24,2))[0],zone_type=u32(u,node+28),edges=[[u32(u,edge_pointer+i*8),u.mem_read(edge_pointer+i*8+4,1)[0]] for i in range(edge_count)]))
   graphs.append(dict(ids=[struct.unpack_from('<H',plane,(y*self.side+x)*10+level*2)[0] for y in range(self.width) for x in range(self.width)],padding_ids=[struct.unpack_from('<H',plane,(y*self.side+x)*10+level*2)[0] for y in range(self.side) for x in range(self.side) if x>=self.width or y>=self.width],records=records))
  return graphs
 def dummy_snapshot(self):
  u=self.uc;return dict(coord=list(struct.unpack('<hh',u.mem_read(DUMMY+0x24,4))),level=u.mem_read(DUMMY+0x11B,1)[0],slope_type=u.mem_read(DUMMY+0x11C,1)[0],bridge_flags_0x1180=u32(u,DUMMY+0x140)&0x1180)
 def run(self):
  result=super().run();result.update(initial_graphs=self.initial_graphs,final_graphs=self.graph_snapshot(),initial_dummy=self.initial_dummy,final_dummy=self.dummy_snapshot(),physical_live_check=self.physical_live_check);return result

def generate():
 rules=Rules();theater_inputs=theater();case=input_case([117,56],theater_inputs);m=HierarchyRepair(case,rules,theater_inputs)
 return dict(schema=1,input=case,stage=m.stage,bootstrap=m.bootstrap,cell_startup=m.cell_startup,assets=m.assets,initial_recalc=m.initial_recalc,text_section_sha256=m.code_hash,native_inputs=rules.snapshot(),theater=theater_inputs,result=m.run())

if __name__=='__main__':
 finish_vectors(generate,HERE/'hierarchy_composition.json.gz',provenance=lambda:provenance(scope=__doc__,
 assumptions=['Frozen production class/height and live Cell allocation/level/slope planes supply inputs; physical MAP pack fields supply affected cells and unused far-cell metadata. Original56C510 and581F90(2,1,0) build initial graph; no supplied Rust graph IDs or edges.',
 'No bridge/tube records in selected map. Both586990 reverse passes, resident47D2B0, incremental584550 and actual42C1C0 scratch refresh execute. Recalc outside physical nine-cellrepairregion fails closed.',
 'Untouched x117,y56 hut selector; repeat repair on same machine. No admitted Engineer, full scenario loader or runtime Windows process claimed.'],
 substitutions=['Successful bounded allocation/free and declared display/radar sinks only during reached repair; vector constructor caller stores are supplied per565800/567110.'],
 entry_points={'repair':0x570050,'connectivity':0x56C510,'build':0x581F90,'batch':0x586990,'patch':0x584550,'scratch':0x42C1C0}))
