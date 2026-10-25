"""Original UnitType constructor and the two FootApproach bool readers.

Retained INI-cache entries are supplied through the existing native reader
fixture. File parsing/type discovery and unrelated whole-reader passes are
outside this focused original7144A0..7144D4 execution.
"""
from pathlib import Path
import hashlib,json,struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_ESI,UC_X86_REG_EBX,UC_X86_REG_EBP,UC_X86_REG_EAX
from tools.native_oracle import finish_vectors,provenance,run_checked,_canonical
from tools.projectile_oracle.ifv_fire_coord import prepare
from tools.projectile_oracle.bridge_render_inputs import assets_root,lexical
from tools.spatial_oracle.building_body_rules import SP,RULES
from tools.spatial_oracle.anytown_damage.inputs import ASSETS
HERE=Path(__file__).resolve().parent
KEYS={'CanApproachTarget':0xD33,'CanRecalcApproachTarget':0xD34}
def sha(raw):return hashlib.sha256(raw).hexdigest()
def generate():
 m,_,_,_,_,_=prepare();u=m.u;typ=m.alloc(0xF00);calls=[];ctor=[];pending={}
 def state():return {name:u.mem_read(typ+off,1)[0]for name,off in KEYS.items()}
 def observe(_u,pc,n,d):
  sp=u.reg_read(UC_X86_REG_ESP)
  if pc in (0x71152A,0x711530):ctor.append(dict(pc=f'{pc:08x}',eax=u.reg_read(UC_X86_REG_EAX),before=state()))
  if pc in pending:
   pending.pop(pc)['returned_al']=u.reg_read(UC_X86_REG_EAX)&255
  if pc==0x5295F0:
   section,key,default=struct.unpack('<III',u.mem_read(sp+4,12))
   r=dict(reader=f'{pc:08x}',caller=f'{m.read32(sp):08x}',section=m.string(section),key=m.string(key),default=default&255)
   calls.append(r);pending[m.read32(sp)]=r
 u.hook_add(UC_HOOK_CODE,observe)
 m.invoke(0x7470D0,typ,(m.cstring('FV'),));default=state();ctor_bytes=bytes(u.mem_read(typ,0xF00));rows=[]
 inputs=[('missing',[{}]),('exact_false',[{'CanApproachTarget':'no','CanRecalcApproachTarget':'false'}]),
         ('lowercase_ignored',[{'canapproachtarget':'no','canrecalcapproachtarget':'no'}]),
         ('invalid_defaults',[{'CanApproachTarget':'bogus','CanRecalcApproachTarget':'2'}]),
         ('retained_after_missing',[{'CanApproachTarget':'no','CanRecalcApproachTarget':'no'},{}]),
         ('retained_after_invalid',[{'CanApproachTarget':'no','CanRecalcApproachTarget':'no'},{'CanApproachTarget':'invalid','CanRecalcApproachTarget':'2'}]),
         ('map_override',[{'CanApproachTarget':'no','CanRecalcApproachTarget':'no'},{'CanApproachTarget':'yes','CanRecalcApproachTarget':'true'}]),
         ('first_character_bool',[{'CanApproachTarget':'yesterday','CanRecalcApproachTarget':'NoSuffix'}]),
         ('numeric_bool',[{'CanApproachTarget':'1tail','CanRecalcApproachTarget':'0tail'}]),
         ('layer_order',[{'CanApproachTarget':'no','CanRecalcApproachTarget':'yes'},{'CanApproachTarget':'yes'},{'CanRecalcApproachTarget':'no'},{'CanApproachTarget':'no','CanRecalcApproachTarget':'yes'}])]
 physical=[]
 for name,path in [('RULESMD.INI',assets_root()/'RULESMD.INI'),('LANGRULE.INI',assets_root()/'LANGRULE.INI'),('MPBattleMD.ini',assets_root()/'MPBattleMD.ini'),('XMP03T4.MAP',ASSETS/'XMP03T4.MAP')]:
  if not path.exists():
   assert name=='LANGRULE.INI';physical.append(dict(name=name,absent=True));continue
  raw=path.read_bytes();sections,_=lexical(raw,{'FV'});physical.append(dict(name=name,sha256=sha(raw),sections_sha256=sha(_canonical(sections)),section_present='FV'in sections,selected={k:v for k,v in sections.get('FV',{}).items()if k in KEYS},cache=sections))
 for name,layers in inputs+[('physical_retail',None)]:
  u.mem_write(typ,ctor_bytes);steps=[]
  layers=physical if layers is None else [dict(name=str(i),cache={'FV':values},selected=values)for i,values in enumerate(layers)]
  for layer in layers:
   if layer.get('absent'):steps.append(dict(name=layer['name'],absent=True));continue
   calls.clear();pending.clear();m.rules_cache(layer['cache']);before=state()
   for reg,v in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EBX,typ+0x24),(UC_X86_REG_EBP,typ)):u.reg_write(reg,v)
   run_checked(u,0x7144A0,0x7144D4,count=100000,required_addresses=(0x5295F0,0x7144B4,0x7144CE))
   assert not pending
   steps.append(dict(input={k:v for k,v in layer.items()if k!='cache'},before=before,reads=calls.copy(),after=state()))
  rows.append(dict(name=name,steps=steps,after=state()))
 return dict(schema=1,constructor=dict(entry='007470d0',observed=ctor,after=default),cases=rows)
def metadata():
 r=provenance(scope=__doc__,assumptions=['The existing ifv_fire_coord.prepare owner establishes the native type registries and reader environment, then a fresh complete original UnitType7470D0 constructor executes. Exact stores71152A/711530 and AL are observed.','Rules layers use unique supplied INI CRC-cache lexical entries; original5295F0 bool parser and7144A0..7144D4 retained defaults/read order execute. Missing sections are a reader-fragment input; the outer full type reader section admission is not replayed.','Physical RULESMD, optionalLANGRULE, MPBattleMD and Anytown map FV entries are read through the same original fragment. Outputs retain only selected short keys and lexical hashes; proprietary INI bodies are not published.'],substitutions=['Existing allocator/INI-cache/reader bootstrap only; no bool parser, default, store or branch return is supplied.'],entry_points={'unit_type_ctor':0x7470D0,'techno_type_reader':0x7144A0,'read_bool':0x5295F0})
 return r
if __name__=='__main__':finish_vectors(generate,HERE/'approach_rules.json',provenance=metadata)
