"""Original LOBRDB damage propagation then repair on physical Anytown topology.
Direct already-admitted damage-driver calls; no projectile/admission or native
scenario load. Recalc/graph work remains explicit callbacks in geometry stage.
"""
from pathlib import Path
import hashlib,json,struct,sys
from unicorn.x86_const import *
from tools.native_oracle import provenance,RET_MAGIC
from tools.spatial_oracle.shrapnel_repair import shrapnel_repair as sr
HERE=Path(__file__).resolve().parent
from . import next_family_native as identity
from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells

def inputs(theater):
 raw=(identity.ASSETS/'XMP03T4.MAP').read_bytes();sections,cells=decode_cells(raw)
 assert not sections.get('CellTags')
 fields={k:v for k,v,line in sections['Map']};size=[int(v) for v in fields['Size'].split(',')[2:]];local=[int(v) for v in fields['LocalSize'].split(',')]
 rows=[];supplied=[]
 for y in range(47,62):
  for x in range(82,94):
   p=cells[x,y];rows.append([x,y,p['tile'],p['subtile'],0,p['overlay'],p['frame'],None,p['level'],0]);supplied.append(dict(coord=[x,y],zone_type=0,slope=0,**p))
 return dict(start=[85,58],impact=[87,54],cells=rows,supplied_cells=supplied,size=size,local_size=local,bridge_base=theater['globals'][0xAA0E28],wood_base=theater['globals'][0xABAD1C],rim_keys={k:theater['globals'][a] for k,a in sr.GLOBALS.items()},supplied_rng={k:identity.seed_state() for k in ('main','scenario','mapgen')},map_sha256=sr.sha(raw),boundary='Empty object heads; native physical map fields but derived land/flags/zone/slope supplied0. Direct admitted57CCF0 calls are not projectile or area-damage admission. Native-seeded0 RNG states are supplied, not scenario-loaded.')

class Geometry(sr.Repair):
 def observe(self,u,address,size,data):
  if self.phase=='measure':
   if address in (0x57CCF0,0x57D530,0x57ED00,0x573540,0x57F440,0x580600):
    sp=u.reg_read(UC_X86_REG_ESP);p=sr.u32(u,sp+4);self.event('native_entry',address=hex(address),coord=list(struct.unpack('<hh',u.mem_read(p,4))))
   if address==0x57DAF0:
    sp=u.reg_read(UC_X86_REG_ESP);self.event('endpoint_scan',coord=list(struct.unpack('<hh',u.mem_read(sp+4,4))))
   if address==0x487A10:
    sp=u.reg_read(UC_X86_REG_ESP);self.event('occupants',coord=self.coord(u.reg_read(UC_X86_REG_ECX)),mode=sr.u32(u,sp+4),mode_byte=u.mem_read(sp+4,1)[0]);return
  super().observe(u,address,size,data)
 def run(self):
  stages=[]
  for name,fn,coord in [('first_damage',0x57CCF0,self.case['impact']),('second_damage',0x57CCF0,self.case['impact']),('repair',0x573540,self.case['start']),('repeat_repair',0x573540,self.case['start'])]:
   self.trace.clear();self.reached={};before={c:self.snapshot(p) for c,p in self.ptrs.items()};rng0={k:sr.rng_state(self.uc,p) for k,p in self.rngs.items()}
   self.uc.mem_write(sr.COORD,sr.packed(*coord));result=self.call(fn,args=(sr.COORD,),count=4000000)
   assert not self.pending,self.pending
   after={c:self.snapshot(p) for c,p in self.ptrs.items()};rng1={k:sr.rng_state(self.uc,p) for k,p in self.rngs.items()}
   assert sr.sha(bytes(self.uc.mem_read(0x401000,0x3E0000)))==self.code_hash
   for key in ('main','scenario'):assert rng0[key]==rng1[key]
   if name!='repair':assert rng0==rng1
   stages.append(dict(name=name,entry=hex(fn),coord=coord,returned_low_byte=result&255,trace=list(self.trace),changed=[dict(before=before[c],after=after[c]) for c in self.ptrs if before[c]!=after[c]],span=[[x,y,after[x,y]['overlay'],after[x,y]['state'],after[x,y]['flags']] for y in range(51,58) for x in range(86,89)],rng_unchanged={k:rng0[k]==rng1[k] for k in rng0},rng_after=rng1))
  return stages

def generate():
 t=identity.theater();case=inputs(t);m=Geometry(case)
 return dict(scope=__doc__,input=case,theater=t,bootstrap=m.bootstrap,cell_startup=m.cell_startup,text_sha256=m.code_hash,steps=m.run(),excluded='Recalc47D2B0, connectivity56C510 and hierarchy586990 are recording callbacks. Actual empty487A10, propagation57ED00, neighbor57CBE0, endpoint57DAF0, untagged575EE0, rectangle5868A0 and MapGen598030/65C780 execute. Renderer/radar are declared sinks. Does not establish object consequences, live navigation, ordinary damage admission or production parity.')

from .publication import finish_vectors

if __name__=='__main__':
 finish_vectors(generate,HERE/'anytown_geometry.json.gz',provenance=lambda:provenance(scope=__doc__,assumptions=['Physical XMP03T4.MAP and original TEMPERATMD scalar/ordinal readers reused from frozen family identity packet.','Two direct calls to actual57CCF0 at87,54 then actual573540 repair from85,58; original propagation and untagged span notification execute.','Empty occupants and supplied native-seeded0 Main/Scenario/MapGen; full native .text checked unchanged.'],substitutions=['Recalc47D2B0, connectivity56C510 and hierarchy586990 record calls and return.','Successful bounded heap, zero display extents, radar/screen sinks.','No area-damage admission, projectile, Engineer prefix/suffix, native scenario loader or production validation.'],entry_points={'damage':0x57CCF0,'walker':0x57D530,'propagation':0x57ED00,'neighbors':0x57CBE0,'endpoints':0x57DAF0,'notification':0x575EE0,'repair':0x573540,'repair_walker':0x580600}))
