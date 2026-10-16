"""Anytown native damage/repair geometry with original Cell Recalc and empty residents.
Physical TMP blocks and native overlay reads are composed; supplied pristine
tile heads and zone storage do not claim full native scenario/theater loading.
"""
from pathlib import Path
import struct
from unicorn.x86_const import *
from tools.native_oracle import provenance,RET_MAGIC
from tools.spatial_oracle.shrapnel_repair import retail_inputs as ri
from tools.projectile_oracle.bridge_render_inputs import lexical
from .anytown_geometry import Geometry,inputs,identity,sr,HERE

def rules():
 r=ri.Rules();indices=range(205,233);names={r.names[i] for i in indices};layers=[]
 for name,path in [('RULESMD.INI',ri.ASSETS/'RULESMD.INI'),('LANGRULE.INI',ri.ASSETS/'LANGRULE.INI'),('MPBattleMD.ini',ri.ASSETS/'MPBattleMD.ini'),('XMP03T4.MAP',identity.ASSETS/'XMP03T4.MAP')]:
  if not path.exists():assert name=='LANGRULE.INI';layers.append(dict(name=name,absent=True));continue
  raw=path.read_bytes();sections,_=lexical(raw,names);r.make_ini(sections)
  for i in indices:r.invoke(0x5FE770,r.overlay_ptrs[i],(ri.INI,))
  layers.append(dict(name=name,sha256=sr.sha(raw)))
 return r,dict(layers=layers,cliff=r.u.mem_read(r.rules+0x664,1)[0],overlays=[dict(index=i,name=r.string(r.overlay_ptrs[i]+0x64),land=r.read32(r.overlay_ptrs[i]+0x298),no_use_tile_land=r.u.mem_read(r.overlay_ptrs[i]+0x2AC,1)[0]) for i in indices],boundary='Reused Rules bootstrap reads physical Shrapnel scalars; the explicit Anytown family pass runs afterward. Anytown has no General or land-speed overrides. No full native Rules chronology claim.')

class Resident(Geometry):
 def __init__(self,case,rules,theater):
  super().__init__(case);self.phase='setup';u=self.uc;u.mem_map(0x44000000,0x800000);self.assets=[];self.theater=theater
  u.mem_write(0xA8ED2C,sr.dwords(0x44000000));u.mem_write(0xA8ED38,sr.dwords(theater['count']))
  selection={(x,y) for y in range(51,58) for x in range(86,89)}|{(85,54),(89,54)}
  needed=sorted({r['tile'] for r in case['supplied_cells'] if tuple(r['coord']) in selection})
  self.ensure_tiles(theater,needed)
  for a,v in theater['globals'].items():u.mem_write(a,sr.dwords(v))
  u.mem_write(0x8871E0,sr.dwords(0x44400000));u.mem_write(0x44400664,bytes(rules.u.mem_read(rules.rules+0x664,1)))
  u.mem_write(0x89EA40,bytes(rules.u.mem_read(0x89EA40,12*36)));u.mem_write(0xA83D84,sr.dwords(0x44410000))
  for index,p in enumerate(rules.overlay_ptrs):
   q=0x44420000+index*0x400;u.mem_write(q,bytes(rules.u.mem_read(p,0x2C0)));u.mem_write(0x44410000+index*4,sr.dwords(q))
  stride=sum(case['size'])+1;count=stride*stride;u.mem_write(sr.MAP+0x68,sr.dwords(0x44200000,count,0x44300000));u.mem_write(0x44200000,b'\x07\x00\x00\x00'*count)
  for r in case['supplied_cells']:
   x,y=r['coord'];u.mem_write(0x44200000+(y*stride+x)*4,bytes([r['zone_type'],r['level']]));u.mem_write(0x44300000+(y*stride+x)*10+8,bytes([r['level']]))
  self.zone_storage=dict(stride=stride,count=count,boundary='Raw zone rows supplied7, then supplied crop level/zone values; native56D3F0 indexes x+y*(width+height+1). Initial selected cell Recalc and later mutations execute; no whole-map navigation initialization.')
  self.phase='measure';rng={k:sr.rng_state(u,p) for k,p in self.rngs.items()};self.trace.clear()
  for c in sorted(selection,key=lambda c:(c[1],c[0])):
   p=self.ptrs[c];self.call(0x47D2B0,this=p,args=(-1,));row=self.pending.pop(RET_MAGIC);row['result']=u.reg_read(UC_X86_REG_EAX);row['after']=self.snapshot(p)
  assert rng=={k:sr.rng_state(u,p) for k,p in self.rngs.items()}
  self.initial_recalc=dict(trace=list(self.trace),rng_unchanged=True,selected_cells=[self.snapshot(self.ptrs[c]) for c in sorted(selection)])
  assert all(self.snapshot(self.ptrs[x,y])['land']==1 for y in range(51,58) for x in range(86,89))
  assert all(self.snapshot(self.ptrs[x,54])['land']==2 for x in (85,89))
  self.trace.clear();self.reached={}
 def ensure_tiles(self,theater,needed,*,uc=None):
  """Extend the same physical TMP transport in a composed execution image."""
  u=self.uc if uc is None else uc
  for tile in sorted(needed):
   if sr.u32(u,0x44000000+tile*4):continue
   index=len(self.assets);assert index<64,'physical TMP crop arena exhausted'
   name=theater['tiles'][tile];path=identity.ASSETS/name
   raw=path.read_bytes();tmp=bytearray(raw);data=0x44020000+index*0x10000;head=0x44010000+index*0x400;width,height=struct.unpack_from('<II',tmp)
   # Preserve historical slots; later bodies must not overlap the existing
   # raw zone planes at44200000/44300000 in this composed image.
   if index>=30:
    if not any(start<=0x48000000<=end for start,end,_ in u.mem_regions()):u.mem_map(0x48000000,0x400000)
    data=0x48000000+index*0x10000
   assert len(tmp)<=0x10000,'physical TMP exceeds its existing crop slot'
   for sub in range(width*height):
    offset=struct.unpack_from('<I',tmp,16+sub*4)[0]
    if offset:struct.pack_into('<I',tmp,16+sub*4,data+offset)
   u.mem_write(0x44000000+tile*4,sr.dwords(head));u.mem_write(head,sr.dwords(0x7ECC48));u.mem_write(head+0xA4,sr.dwords(data))
   u.mem_write(head+0x2C8,sr.dwords(-1));u.mem_write(head+0x2D4,sr.dwords(-1));u.mem_write(head+0x2F0,sr.dwords(1));u.mem_write(data,bytes(tmp))
   self.assets.append(dict(tile=tile,file=name,sha256=sr.sha(raw),bytes=len(raw),source='ra2.mix/isotemp.mix',boundary='Physical TMP body with runtime pointer relocation; native pristine-type heads supplied, terrain anim=-1 and unused permissions zero.'))
 def observe(self,u,address,size,data):
  if self.phase=='measure':
   if address==0x47D2B0:
    p=u.reg_read(UC_X86_REG_ECX);sp=u.reg_read(UC_X86_REG_ESP);row=dict(kind='recalc',coord=self.coord(p),level=sr.i32(u,sp+4),before=self.snapshot(p));self.trace.append(row);self.pending[sr.u32(u,sp)]=row;return
   if address in (0x547020,0x727FD0,0x421EA0,0x547370):raise AssertionError(('uncovered resident dependency',hex(address)))
  super().observe(u,address,size,data)

def generate():
 t=identity.theater();r,fields=rules();case=inputs(t);m=Resident(case,r,t);steps=m.run()
 assert not any(v['kind']=='recalc_boundary' for s in steps for v in s['trace'])
 assert {tuple(c['after']['coord']):c['after']['land'] for c in steps[1]['changed'] if c['after']['coord'][1]==54}=={(86,54):2,(87,54):2,(88,54):2}
 assert all(c['after']['land']==1 for c in steps[2]['changed'])
 return dict(scope=__doc__,input=case,rules=fields,theater=t,assets=m.assets,zone_storage=m.zone_storage,initial_recalc=m.initial_recalc,text_sha256=m.code_hash,steps=steps,excluded='Original47D2B0,47CA80,483C80 and empty487A10 execute. Connectivity56C510 and hierarchy586990 remain recording callbacks. Pristine TMP type heads and initial raw zone planes are supplied; no scenario load, actor admission, live occupant consequence, art binding or production comparison.')

from .publication import finish_vectors

if __name__=='__main__':
 finish_vectors(generate,HERE/'anytown_resident.json.gz',provenance=lambda:provenance(scope=__doc__,assumptions=['Geometry packet inputs reused; original overlay readers for205..232 plus physical TMP bodies and original Recalc, LAT and zone classification.','All23 selected cells receive actual initial Recalc; all later repair/damage Recalc executes, as does empty487A10. No objects or CellTags.','Original56D3F0 index stride166 from80+85+1; initial native zone planes supplied, original stores retained.'],substitutions=['Pristine tile-type heads supplied with actual relocated TMP bodies, terrain anim=-1, no auxiliary cell list; no native type/theater loader.','NativeRules helper bootstrap precedes explicit Anytown family-layer reads; no full Rules load chronology.','Connectivity56C510 and hierarchy586990 callbacks; display/radar sinks; bounded successful heap.','Direct admitted damage driver and repair controller calls, no projectile/area or Engineer admission and no production comparison.'],entry_points={'damage':0x57CCF0,'repair':0x573540,'recalc':0x47D2B0,'lat':0x47CA80,'zone':0x483C80,'index':0x56D3F0,'occupants':0x487A10}))
