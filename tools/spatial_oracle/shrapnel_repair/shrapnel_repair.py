"""Bounded original ordinary repair composed over physical Shrapnel projection.

No native scenario loader or already-admitted Engineer prefix is claimed. The
frozen production scalar/RNG input is checked against physical packed MAP data.
Run with PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. from the owning worktree.
"""
from pathlib import Path
import hashlib,json,struct
from unicorn.x86_const import *
from tools.native_oracle import provenance,run_checked,RET_MAGIC,STACK_BASE,STACK_SIZE
from tools.spatial_oracle.bridge_rim import OriginalRim,MAP,COORD,DUMMY,CELLS,GLOBALS
from tools.spatial_oracle.map_queries import dwords,packed,normalize
from .retail_inputs import Rules,theater,ASSETS
from . import map_facts as mapfacts
from tools.spatial_oracle import mapgen_range as mapgen
from .production_inputs import production,source_sha256
from .packet_io import finish_vectors

HERE=Path(__file__).resolve().parent

def sha(raw):return hashlib.sha256(raw).hexdigest()
def u32(u,p):return struct.unpack('<I',u.mem_read(p,4))[0]
def i32(u,p):return struct.unpack('<i',u.mem_read(p,4))[0]
def state_bytes(r):return struct.pack('<B3xii250I',r['disabled'],r['index_a'],r['index_b'],*r['state'])
def rng_state(u,p):
 b=bytes(u.mem_read(p,1012));return dict(disabled=b[0],index_a=i32(u,p+4),index_b=i32(u,p+8),state=list(struct.unpack('<250I',b[12:])))

def physical_map():
 raw=(ASSETS/'XShrapnel.MAP').read_bytes();_,cells=mapfacts.decode_cells(raw)
 # Preserve the original Shrapnel witness schema; ice is not a supplied input.
 for cell in cells.values():cell.pop('ice')
 return raw,cells

def input_case(hut,theater_inputs):
 d=production('before_command')
 physical_raw,authored=physical_map();rows=[]
 for r in d['cells']:
  p=authored[tuple(r['coord'])];b=r['bridge']
  assert (r['tile'],r['subtile'],r['level'],b['overlay_id'],b['state_byte'])==(p['tile'],p['subtile'],p['level'],p['overlay'],p['frame']),r['coord']
  rows.append([*r['coord'],r['tile'],r['subtile'],b['raw_flags'],b['overlay_id'],b['state_byte'],None,r['level'],r['land']])
 bounds=normalize(d['native_size'],d['local_size'])
 return dict(name='hut_'+str(hut[0])+'_'+str(hut[1]),start=hut,cells=rows,size=d['native_size'],local_size=bounds['normalized'],bounds_normalization=bounds,bridge_base=theater_inputs['globals'][0xAA0E28],wood_base=theater_inputs['globals'][0xABAD1C],rim_keys={k:theater_inputs['globals'][a] for k,a in GLOBALS.items()},
  supplied_rng=d['rng'],supplied_cells=[{k:r[k] for k in ('coord','tile','subtile','level','slope','land','zone_type','bridge')} for r in d['cells']],production_input_sha256=source_sha256('before_command'),physical_map_sha256=sha(physical_raw))

class Repair(OriginalRim):
 def __init__(self,case):
  self.case=case;self.phase='setup';self.trace=[];self.pending={};self.reached={};self.stage='controller'
  super().__init__(case);self.u=self.uc;u=self.uc
  self.code_hash=sha(bytes(u.mem_read(0x401000,0x3E0000)))
  self.events=self.trace;self.heap=0x45000000;u.mem_map(self.heap,0x2000000)
  self.bootstrap=mapgen.Machine.startup(self)
  initializers=struct.unpack('<13I',u.mem_read(0x8129FC,13*4))
  for address in initializers:self.call(address,count=100000)
  self.cell_startup=dict(table='0x8129fc',initializers=[hex(a) for a in initializers],level_height=i32(u,0x89E7C0),bridge_height=i32(u,0x89E7B4))
  assert self.cell_startup['level_height']==104
  self.rngs={'main':0x886B88,'scenario':0x46000000+0x218,'mapgen':0xABE890}
  u.mem_write(0xA8B230,dwords(0x46000000))
  for k,p in self.rngs.items():u.mem_write(p,state_bytes(case['supplied_rng'][k]))
  u.mem_write(0xABAD1C,dwords(case['wood_base']))
  u.mem_write(MAP+0xFC,dwords(*case['local_size']))
  u.mem_write(MAP+0x124,dwords(0,0,511,511))
  for r in case['supplied_cells']:
   p=self.ptrs[tuple(r['coord'])];u.mem_write(p+0x4C,dwords(r['zone_type']));u.mem_write(p+0x11C,bytes([r['slope']]))
   u.mem_write(p+0x116,b'\xff\xff')
  self.phase='measure'
 def ret(self,cleanup=0,eax=0):
  u=self.uc;sp=u.reg_read(UC_X86_REG_ESP);u.reg_write(UC_X86_REG_EAX,eax);u.reg_write(UC_X86_REG_EIP,u32(u,sp));u.reg_write(UC_X86_REG_ESP,sp+4+cleanup)
 def event(self,kind,**values):self.trace.append(dict(kind=kind,**values))
 def observe(self,u,address,size,data):
  if self.phase!='measure':return
  if address not in self.pending and address not in (0x570050,0x57F200,0x57FBC0,0x598030,0x65C780,0x487A10,0x47B3A0,0x5868A0,0x7C5F00,0x7C8E17,0x7C8B3D,0x47FDE0,0x47FB90,0x47D2B0,0x56C510,0x586990,0x47DD70,0x6551C0,0x6D2140,0x6D2790,0x576200,0x47E040,0x575EE0):return
  sp=u.reg_read(UC_X86_REG_ESP);args=struct.unpack('<5I',u.mem_read(sp+4,20));this=u.reg_read(UC_X86_REG_ECX)
  if address in self.pending:
   row=self.pending.pop(address);row['result']=u.reg_read(UC_X86_REG_EAX)
   if row['kind']=='recalc':row['after']=self.snapshot(self.ptrs[tuple(row['coord'])])
  if address in (0x570050,0x57F200,0x57FBC0,0x598030,0x65C780,0x487A10,0x47B3A0,0x5868A0,0x7C5F00):self.reached[hex(address)]=self.reached.get(hex(address),0)+1
  if address in (0x570050,0x57F200,0x57FBC0):
   self.event('repair_entry' if address==0x570050 else 'ordinary' if address==0x57F200 else 'walker',coord=list(struct.unpack('<hh',u.mem_read(args[0],4))));return
  if address==0x598030:
   row=dict(kind='random',minimum=this,maximum=u.reg_read(UC_X86_REG_EDX),before_index_a=u32(u,0xABE894));self.trace.append(row);self.pending[u32(u,sp)]=row;return
  if address==0x65C780:self.event('next',stream=next(k for k,p in self.rngs.items() if p==this),index_a=u32(u,this+4));return
  if address==0x7C8E17:
   p=self.heap;self.heap+=(max(args[0],1)+15)&~15;assert self.heap<0x45E00000;self.ret(eax=p);return
  if address==0x7C8B3D:self.ret();return
  if address in (0x47FDE0,0x47FB90):u.mem_write(args[0],dwords(0,0,0,0));self.ret(4,args[0]);return
  if address==0x47D2B0:
   self.event('recalc_boundary',coord=self.coord(this),level=i32(u,sp+4));self.ret(4);return
  if address==0x487A10:self.event('occupants',coord=self.coord(this),mode=args[0]);return
  if address==0x56C510:self.event('connectivity_boundary');self.ret();return
  if address==0x5868A0:self.event('rectangle',value=list(struct.unpack('<4i',u.mem_read(args[0],16))));return
  if address==0x586990:
   v=args[0];data=u32(u,v+4);count=u32(u,v+16)
   self.event('rebuild_boundary',cells=[list(struct.unpack('<hh',u.mem_read(data+i*4,4))) for i in range(count)]);self.ret(4);return
  super().observe(u,address,size,data)
 def write(self,u,access,address,size,value,data):
  assert not (0x401000 <= address < 0x7E1000), ('original code write forbidden',hex(address))
  if self.phase=='measure' and CELLS<=address<CELLS+len(self.ptrs)*0x200:
   cell=CELLS+((address-CELLS)//0x200)*0x200;off=address-cell
   if off==0x44:self.event('overlay',coord=self.coord(cell),value=value)
   if off in (0x4C,0xEC,0x11B,0x11C):self.event('cell_write',coord=self.coord(cell),offset=hex(off),bytes=size,value=value)
  super().write(u,access,address,size,value,data)
 def snapshot(self,p):
  r=super().snapshot(p);u=self.uc;r.update(tile=i32(u,p+0x38),subtile=u.mem_read(p+0x11A,1)[0],level=u.mem_read(p+0x11B,1)[0],slope=u.mem_read(p+0x11C,1)[0],land=i32(u,p+0xEC),zone_type=i32(u,p+0x4C),ground_head=u32(u,p+0xE4));return r
 def run(self):
  result=[]
  for repetition in range(2):
   self.trace.clear();self.reached={};before={c:self.snapshot(p) for c,p in self.ptrs.items()};rng_before={k:rng_state(self.uc,p) for k,p in self.rngs.items()}
   self.uc.mem_write(COORD,packed(*self.case['start']));self.call(0x570050,args=(COORD,),count=30000000)
   assert not self.pending
   assert self.reached.get('0x570050')==self.reached.get('0x57f200')==self.reached.get('0x57fbc0')==1
   if repetition==0:assert self.reached.get('0x598030')==self.reached.get('0x65c780')==3 and self.reached.get('0x487a10')==9
   after={c:self.snapshot(p) for c,p in self.ptrs.items()};changes=[dict(before=before[c],after=after[c]) for c in before if before[c]!=after[c]]
   result.append(dict(repetition=repetition,trace=list(self.trace),reached=self.reached,changes=changes,strip=[after[x,y] for y in range(54,65) for x in range(114,117)],rng_before=rng_before,rng_after={k:rng_state(self.uc,p) for k,p in self.rngs.items()}))
  assert sha(bytes(self.uc.mem_read(0x401000,0x3E0000)))==self.code_hash
  return result

class ResidentRepair(Repair):
 def __init__(self,case,rules,theater,*,resident_cells=None):
  super().__init__(case);self.stage='resident_recalc';self.phase='setup';u=self.uc
  # The ordinary damage witness touches nine cells. Hut walkers reuse this
  # owner with a larger physical strip; preserve the original default order.
  resident_cells=list(resident_cells) if resident_cells is not None else [(x,y) for y in range(58,61) for x in range(114,117)]
  selected={tuple(c) for c in resident_cells};assert selected<=self.ptrs.keys()
  u.mem_map(0x44000000,0x800000);self.assets=[]
  u.mem_write(0xA8ED2C,dwords(0x44000000));u.mem_write(0xA8ED38,dwords(theater['count']))
  needed=sorted({r['tile'] for r in case['supplied_cells'] if tuple(r['coord']) in selected})
  for index,tile in enumerate(needed):
   path=ASSETS/theater['tiles'][tile];raw=path.read_bytes();tmp=bytearray(raw)
   data=0x44020000+index*0x10000;head=0x44010000+index*0x400
   width,height=struct.unpack_from('<II',tmp)
   for sub in range(width*height):
    offset=struct.unpack_from('<I',tmp,16+sub*4)[0]
    if offset:struct.pack_into('<I',tmp,16+sub*4,data+offset)
   u.mem_write(0x44000000+tile*4,dwords(head));u.mem_write(head,dwords(0x7ECC48));u.mem_write(head+0xA4,dwords(data))
   u.mem_write(head+0x2C8,dwords(-1));u.mem_write(head+0x2D4,dwords(-1));u.mem_write(head+0x2F0,dwords(1));u.mem_write(data,bytes(tmp))
   self.assets.append(dict(tile=tile,name=path.name,sha256=sha(raw),bytes=len(raw),source='ra2.mix/isosnow.mix',boundary='physical TMP bytes with only runtime pointer relocation; resident head data supplied'))
  for a,v in theater['globals'].items():u.mem_write(a,dwords(v))
  u.mem_write(0x8871E0,dwords(0x44400000));u.mem_write(0x44400664,bytes([rules.u.mem_read(rules.rules+0x664,1)[0]]))
  u.mem_write(0x89EA40,bytes(rules.u.mem_read(0x89EA40,12*36)))
  u.mem_write(0xA83D84,dwords(0x44410000))
  for index,p in enumerate(rules.overlay_ptrs):
   q=0x44420000+index*0x400;u.mem_write(q,bytes(rules.u.mem_read(p,0x2C0)));u.mem_write(0x44410000+index*4,dwords(q))
  count=165*165;u.mem_write(MAP+0x68,dwords(0x44200000,count,0x44300000))
  u.mem_write(0x44200000,b'\x07\x00\x00\x00'*count)
  for r in case['supplied_cells']:
   x,y=r['coord'];u.mem_write(0x44200000+(y*165+x)*4,bytes([r['zone_type'],r['level']]))
   u.mem_write(0x44300000+(y*165+x)*10+8,bytes([r['level']]))
  self.phase='measure'
  self.trace.clear();self.reached={};rng_before={k:rng_state(u,p) for k,p in self.rngs.items()}
  for x,y in resident_cells:
   p=self.ptrs[x,y];self.call(0x47D2B0,this=p,args=(-1,))
   row=self.pending.pop(RET_MAGIC);row['result']=u.reg_read(UC_X86_REG_EAX);row['after']=self.snapshot(p)
  assert not self.pending and rng_before=={k:rng_state(u,p) for k,p in self.rngs.items()}
  self.initial_recalc=dict(trace=list(self.trace),reached=dict(self.reached),rng_unchanged=True)
  self.trace.clear();self.reached={}
 def observe(self,u,address,size,data):
  if self.phase=='measure' and address==0x47D2B0:
   p=u.reg_read(UC_X86_REG_ECX);sp=u.reg_read(UC_X86_REG_ESP)
   row=dict(kind='recalc',coord=self.coord(p),level=i32(u,sp+4),before=self.snapshot(p));self.trace.append(row);self.pending[u32(u,sp)]=row
   self.reached[hex(address)]=self.reached.get(hex(address),0)+1;return
  if self.phase=='measure' and address in (0x5471B0,0x47CA80,0x483C80):self.reached[hex(address)]=self.reached.get(hex(address),0)+1
  if self.phase=='measure' and address in (0x547020,0x727FD0,0x421EA0,0x547370):raise AssertionError(('uncovered resident dependency',hex(address)))
  super().observe(u,address,size,data)

def generate():
 rules=Rules();theater_inputs=theater();rows=[]
 for hut in ([117,56],[113,62]):
  case=input_case(hut,theater_inputs)
  for cls in (Repair,ResidentRepair):
   m=cls(case) if cls==Repair else cls(case,rules,theater_inputs)
   rows.append(dict(input=case,stage=m.stage,bootstrap=m.bootstrap,cell_startup=m.cell_startup,text_section_sha256=m.code_hash,assets=getattr(m,'assets',[]),initial_recalc=getattr(m,'initial_recalc',None),steps=m.run()))
 return dict(schema=2,native_inputs=rules.snapshot(),theater=theater_inputs,cases=rows)

if __name__=='__main__':
 finish_vectors(generate,HERE/'shrapnel_repair.json.gz',provenance=lambda:provenance(scope=__doc__,
  assumptions=['Frozen supplied production scalar cells are checked against physical MAP pack values; absent occupants are an explicit boundary, not a claim about the live map.',
   'Full Main/Scenario/MapGen state from production is a recorded supplied boundary. Actual598030 and65C780 execute; native map loading and native reachability of input RNG states are not claimed.',
   'Actual ordinary low570050/57F200/57FBC0 and empty487A10 execute. Existing process CRT precision tail and WinMain rounding block establish x87; OS probe excluded.'],
  substitutions=['Successful bounded allocation/free; zero display rectangles and radar/screen sinks.',
   'Controller stage substitutes47D2B0. Resident stage executes47D2B0 with actual TMP and native-read overlay/land/theater inputs. Both stages still substitute56C510/586990; no connectivity or hierarchy result claimed.'],
  entry_points={'low':0x570050,'ordinary':0x57F200,'walker':0x57FBC0,'range':0x598030,'next':0x65C780,'occupants':0x487A10,'rectangle':0x5868A0}))
