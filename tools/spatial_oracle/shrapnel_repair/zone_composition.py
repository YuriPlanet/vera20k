"""Original connectivity/hierarchy suffixes on supplied full Shrapnel cache planes."""
from .shrapnel_repair import *

BASE,PLANE=0x44200000,0x44300000

class ConnectivityRepair(ResidentRepair):
 def __init__(self,case,rules,theater):
  super().__init__(case,rules,theater);self.stage='native_connectivity';u=self.uc
  self.navigation=production('before_command')['navigation']
  n=self.navigation;self.width=n['width'];self.side=self.width+1
  assert n['height']==self.width==sum(case['size']) and not n['records'] and n['records_match_bridge_authority']
  physical_raw,_=physical_map();assert not mapfacts.sections(physical_raw).get('Tubes')
  nodes=bytearray(b'\x07\x00\x00\x00'*(self.side*self.side))
  for y in range(self.width):
   for x in range(self.width):
    i=y*self.width+x;struct.pack_into('<BBH',nodes,(y*self.side+x)*4,n['classes'][i],n['levels'][i],0)
  u.mem_write(BASE,bytes(nodes));u.mem_write(PLANE,bytes(self.side*self.side*10))
  u.mem_write(MAP+0x18,bytes(13*4));u.mem_write(MAP+0x54,dwords(0,0,1,0,10))
  self.pending.clear();self.trace.clear();self.reached={}
  for address in (0x49F0E0,0x49F190,0x49F2F0,0x49F2D0,0x49F280):self.call(address,count=10000)
  root=self.allocate(16);buckets=self.allocate(256*24);template=self.allocate(24)
  self.call(0x58AFF0,this=template,args=(0,0),count=1000)
  # Original565800 substitutes final8-byte-vector vtable and growth20 after58AFF0.
  u.mem_write(template,dwords(0x7ED540));u.mem_write(template+16,dwords(0,20))
  u.mem_write(buckets,bytes(u.mem_read(template,24))*256);u.mem_write(root,dwords(buckets,0x56CB80,256,20));u.mem_write(MAP+0x14,dwords(root))
  self.call(0x56C510,count=30000000)
  self.initial_navigation=self.nav_snapshot();self.initial_trace=list(self.trace);self.initial_reached=dict(self.reached)
 def allocate(self,size):
  p=self.heap;self.heap+=(max(size,1)+15)&~15;assert self.heap<0x45E00000;return p
 def call(self,address,this=MAP,args=(),count=30000000):
  u=self.uc;sp=STACK_BASE+STACK_SIZE-0x1000;u.mem_write(sp,dwords(RET_MAGIC,*args));u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,this)
  run_checked(u,address,RET_MAGIC,count=count,timeout_us=60000000);return u.reg_read(UC_X86_REG_EAX)
 def observe(self,u,address,size,data):
  if self.phase=='measure' and address in (0x56C510,0x56CB90,0x56C6CB):
   self.reached[hex(address)]=self.reached.get(hex(address),0)+1
   if address==0x56C510:self.event('connectivity')
   return
  super().observe(u,address,size,data)
 def nav_snapshot(self):
  u=self.uc;nodes=bytes(u.mem_read(BASE,self.side*self.side*4));count=u32(u,MAP+0x4C)
  return dict(classes=[nodes[(y*self.side+x)*4] for y in range(self.width) for x in range(self.width)],
   levels=[nodes[(y*self.side+x)*4+1] for y in range(self.width) for x in range(self.width)],
   base_ids=[struct.unpack_from('<H',nodes,(y*self.side+x)*4+2)[0] for y in range(self.width) for x in range(self.width)],
   raw_rows=[list(struct.unpack('<'+'H'*count,u.mem_read(u32(u,MAP+0x18+i*4),count*2))) for i in range(13)],zone_count=count-1)
 def run(self):
  rows=super().run();return dict(initial_navigation=self.initial_navigation,initial_reached=self.initial_reached,steps=rows,final_navigation=self.nav_snapshot())

def generate():
 rules=Rules();theater_inputs=theater();case=input_case([117,56],theater_inputs);m=ConnectivityRepair(case,rules,theater_inputs)
 return dict(schema=1,input=case,stage=m.stage,bootstrap=m.bootstrap,cell_startup=m.cell_startup,assets=m.assets,initial_recalc=m.initial_recalc,text_section_sha256=m.code_hash,native_inputs=rules.snapshot(),theater=theater_inputs,result=m.run())

if __name__=='__main__':
 finish_vectors(generate,HERE/'zone_composition.json.gz',provenance=lambda:provenance(scope=__doc__,
 assumptions=['Full class and height planes are frozen production inputs; original56C510 produces initial and final base IDs and13 raw movement rows, not a native scenario load.',
 'Initial adjacency root and bucket headers follow original565800 construction;58AFF0 executes and finalvtable7ED540/growth20 stores supplied. No bridge/tube records: physical map has no raised bridge overlay or Tubes; production authority records empty.',
 'Resident TMP, original retail readers, ordinary repair, MapGen, empty occupants and rectangle enumeration execute as described byshrapnel_repair.'],
 substitutions=['Successful bounded allocation/free and presentation sinks. Hierarchy586990 remains recorded callback boundary in connectivity stage.'],
 entry_points={'repair':0x570050,'connectivity':0x56C510,'flood':0x56CB90,'pairs':0x56C6CB}))
