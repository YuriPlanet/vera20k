"""Research ahead: actual low-family readers and pristine Anytown repair selector.
No damage, repair mutation, production map load, Engineer admission, or live
occupant outcome is claimed. Already-admitted interior519C07 frame is supplied.
"""
from pathlib import Path
import json,struct
from unicorn.x86_const import *
from tools.native_oracle import provenance,run_checked,STACK_BASE,STACK_SIZE
from tools.spatial_oracle.shrapnel_repair import retail_inputs as ri,shrapnel_repair as sr
from tools.spatial_oracle.shrapnel_repair.retail_inputs import INI
from tools.rules_oracle.theater_general_reader import TheaterReader,general_text
from tools.rules_oracle import theater_general_reader
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.rmg_oracle.gen_rng_vectors import seeded_struct
from tools.spatial_oracle.map_queries import normalize
from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
from tools.spatial_oracle.shrapnel_repair.packet_io import digest as sha
from .inputs import ASSETS
HERE=Path(__file__).resolve().parent

def theater(*, tile_reader=None):
 """Read the shared scalar theater projection; optionally reuse a caller's INI reader.

 The General/project owner remains TheaterReader in its separate prerequisite
 VM. A composed fixture may reuse its existing reader for TileSet ReadInt,
 avoiding unrelated Rules/map preparation. Historical callers retain ri.Rules.
 """
 raw=(ri.ASSETS/'TEMPERATMD.INI').read_bytes();previous=theater_general_reader.ROOT;theater_general_reader.ROOT=ri.ASSETS
 try:m=TheaterReader()
 finally:theater_general_reader.ROOT=previous
 result=m.read(general_text(raw));rows=m.reads
 assert m.asset_loaded==[], 'scalar theater witness must not load graphics'
 r=ri.Rules() if tile_reader is None else tile_reader;sections,_=lexical(raw,{f'TileSet{i:04d}' for i in range(300)});r.make_ini(sections);base=0;tiles={};sets=[]
 for ordinal in range(300):
  name=f'TileSet{ordinal:04d}'
  if name not in sections:break
  count=r.invoke(0x5276D0,INI,(r.cstring(name),r.cstring('TilesInSet'),0));m.project(ordinal,base)
  for n in range(count):tiles[base+n]=sections[name]['FileName']+str(n+1).zfill(2)+'.tem'
  sets.append(dict(ordinal=ordinal,base=base,count=count,file=sections[name]['FileName']));base+=count
 values={int(row.get('resolved_global',row['location']),16):m.read32(int(row.get('resolved_global',row['location']),16)) for row in rows}
 return dict(sha256=sha(raw),general=result,globals=values,tiles=tiles,sets=sets,count=base)

def native_rules():
 r=ri.Rules();indices=list(range(74,102))+list(range(205,233));names={r.names[i] for i in indices};layers=[]
 for name,p in [('RULESMD.INI',ri.ASSETS/'RULESMD.INI'),('LANGRULE.INI',ri.ASSETS/'LANGRULE.INI'),('MPBattleMD.ini',ri.ASSETS/'MPBattleMD.ini'),('XMP03T4.MAP',ASSETS/'XMP03T4.MAP')]:
  if not p.exists():assert name=='LANGRULE.INI';layers.append(dict(name=name,absent=True));continue
  raw=p.read_bytes();sections,_=lexical(raw,names);r.make_ini(sections)
  admitted=[]
  for i in indices:
   p=r.overlay_ptrs[i]
   if r.invoke(0x5FE770,p,(INI,))&255:admitted.append(i)
  layers.append(dict(name=name,sha256=sha(raw),admitted=admitted))
 result=[]
 for i in indices:
  p=r.overlay_ptrs[i];result.append(dict(index=i,id=r.string(p+0x24),name=r.string(p+0x64),image=r.string(p+0x1F8),land=r.read32(p+0x298),no_use_tile_land=r.u.mem_read(p+0x2AC,1)[0],radar_invisible=r.u.mem_read(p+0x22F,1)[0]))
 assert {r['name'] for r in result if r['index']>=205}=={'Concrete Low Bridge'}
 assert {r['name'] for r in result if r['index']<=101}=={'Low Bridge'}
 return dict(layers=layers,outputs=result,asset_request_count=len(r.asset_loaded),unique_generic_asset_requests=sorted({a['name'] for a in r.asset_loaded}),image_boundary='Full original INI readers; generic SHP asset loads return absent from the declared cache. Selected theater art is inspected separately, not loaded by this witness.')

def seed_state():
 raw=seeded_struct(0);a,b=struct.unpack_from('<ii',raw,4)
 return dict(disabled=raw[0],index_a=a,index_b=b,state=list(struct.unpack_from('<250I',raw,12)))

class Select(sr.Repair):
 def __init__(self,case):
  self.entries=[];self.selector_sp=STACK_BASE+STACK_SIZE-0x1000;self.latch=None
  super().__init__(case)
 def observe(self,u,address,size,data):
  if self.phase=='measure':
   if address in (0x573540,0x57F440,0x5800D0,0x580600):
    sp=u.reg_read(UC_X86_REG_ESP);p=sr.u32(u,sp+4);self.entries.append(dict(address=hex(address),coord=list(struct.unpack('<hh',u.mem_read(p,4)))))
   if address==0x519CD9:self.latch=u.mem_read(self.selector_sp+0x54,1)[0]
   if address in (0x598030,0x65C780,0x47D2B0,0x487A10,0x56C510,0x586990):raise AssertionError(('pristine control unexpectedly mutates',hex(address)))
  super().observe(u,address,size,data)
 def run(self):
  u=self.uc;actor,hut=0x46010000,0x46012000;x,y=self.case['start'];u.mem_write(actor,sr.dwords(0x7EB058));u.mem_write(actor+0x9C,sr.dwords(x*256+128,y*256+128,416))
  assert sr.u32(u,0x7EB058+0x1B8)==0x41BEA0
  before={c:self.snapshot(p) for c,p in self.ptrs.items()};rng={k:sr.rng_state(u,p) for k,p in self.rngs.items()}
  self.trace.clear();u.reg_write(UC_X86_REG_ESP,self.selector_sp);u.reg_write(UC_X86_REG_ESI,actor);u.reg_write(UC_X86_REG_EDI,hut)
  end=run_checked(u,0x519C07,0x519D17,count=2000000,required_addresses=(0x41BEA0,0x5657A0,0x573540,0x57F440,0x580600))
  assert before=={c:self.snapshot(p) for c,p in self.ptrs.items()}
  assert rng=={k:sr.rng_state(u,p) for k,p in self.rngs.items()}
  assert sr.sha(bytes(u.mem_read(0x401000,0x3E0000)))==self.code_hash
  return dict(hut=self.case['start'],entries=self.entries,low_latch=self.latch,stop=hex(end),no_cell_changes=True,all_rng_unchanged=True,trace=self.trace,text_sha256=self.code_hash)

def generate():
 t=theater();raw=(ASSETS/'XMP03T4.MAP').read_bytes();sections,physical=decode_cells(raw)
 mapfields={k:v for k,v,line in sections['Map']};size=[int(v) for v in mapfields['Size'].split(',')[2:]];local=[int(v) for v in mapfields['LocalSize'].split(',')]
 rows=[];supplied=[]
 for y in range(47,62):
  for x in range(82,94):
   p=physical[x,y];rows.append([x,y,p['tile'],p['subtile'],0,p['overlay'],p['frame'],None,p['level'],0]);supplied.append(dict(coord=[x,y],zone_type=0,slope=0))
 normalized=normalize(size,local);cases=[]
 for hut in ([85,58],[89,51]):
  case=dict(start=hut,cells=rows,size=size,local_size=normalized['normalized'],bridge_base=t['globals'][0xAA0E28],wood_base=t['globals'][0xABAD1C],rim_keys={k:t['globals'][a] for k,a in sr.GLOBALS.items()},supplied_rng={k:seed_state() for k in ('main','scenario','mapgen')},supplied_cells=supplied)
  cases.append(Select(case).run())
 return dict(scope=__doc__,map=dict(file='XMP03T4.MAP',sha256=sha(raw),bytes=len(raw),size=size,local_size=local,bounds_normalization=normalized),rules=native_rules(),theater=t,cases=cases,physical_crop=[dict(coord=list(c),**v) for c,v in sorted(physical.items()) if 82<=c[0]<94 and 47<=c[1]<62],boundary='Physical authored tile/subtile/level/overlay/frame; all structural flags/land/zone/slope and absent object heads are supplied zeros unused by this pristine branch. Main/Scenario/MapGen use separately native-seeded0 inputs, not production loaded states. Stops519D17 before Engineer suffix; no placement/path/admission/load claim.')

from .publication import finish_vectors

if __name__=='__main__':
 finish_vectors(generate,HERE/'next_family_native.json.gz',provenance=lambda:provenance(scope=__doc__,assumptions=['Exact physical stock MAP crop and native rules/type/theater readers; lexical INI caches and ordinal iteration are supplied as in the accepted Shrapnel helper.','Already-admitted Engineer frame at either authored untagged hut; intact original519C07->573540->57F440->580600 then stop519D17. Common suffix and full map loading excluded.'],substitutions=['Generic SHP requests see the declared sparse asset cache; no runtime theater art binding is claimed.','Unused native Cell flags/land/zone/slope supplied zero; object heads empty; original seed0 logical states supplied. Any RNG or repair mutation callback reached during pristine selector control fails closed.'],entry_points={'overlay_reader':0x5FE770,'object_reader':0x5F92D0,'abstract_reader':0x410A60,'engineer_selector':0x519C07,'controller':0x573540,'selector':0x57F440,'walker':0x580600}))
