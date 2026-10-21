"""Original Rules constructor/Process/type-postpass/AudioVisual chronology.

The historical controls use cached INI objects and inherited BulletReader
allocator/CRT/archive boundaries. The optional RetailRulesHistory extension
executes physical loading and full selected-retail type discovery; its caller
records the admitted byte catalog and platform/world boundaries separately.
Original Process is void; its incidental EAX is deliberately not a success flag.
"""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from tools.native_oracle import finish_vectors,provenance
from tools.projectile_oracle.bridge_render_inputs import BulletReader
from tools.spatial_oracle.building_body_rules import RULES,dwords

def s32(m,p):return struct.unpack('<i',m.u.mem_read(p,4))[0]
def fresh():
 m=BulletReader({})
 m.u.mem_write(0x887568,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 w=m.invoke(0x772fa0,m.cstring('OrderProbe'))
 r=m.alloc(0x2000);m.u.mem_write(0x8871e0,dwords(r));m.invoke(0x665650,r)
 return m,w,r

def chronology(cold_gravity=None):
 m,w,r=fresh();ctor=dict(speed=s32(m,w+0xa8),gravity=s32(m,r+0x16b8))
 cold=None
 if cold_gravity is not None:
  m.rules_cache({'AudioVisual':{'Gravity':str(cold_gravity)}})
  m.invoke(0x6691e0,r,(RULES,))
  cold=dict(authored_gravity=cold_gravity,result=s32(m,r+0x16b8))
 phases={0x668ef0:'before_type_data',0x668ef5:'after_type_data',0x668f56:'before_audio_visual',0x668f5b:'after_audio_visual'}
 events=[]
 def observe(u,a,size,data):
  if a in phases:events.append(dict(at=f'{a:08x}',phase=phases[a],speed=s32(m,w+0xa8),gravity=s32(m,r+0x16b8)))
 m.u.hook_add(UC_HOOK_CODE,observe)
 rows=[]
 cases=[
  ('base_ballistic_gravity_changes_late',{'OrderProbe':{'Speed':'40','Range':'5','Projectile':'OrderBullet'},'OrderBullet':{'ROT':'0'},'AudioVisual':{'Gravity':'12'}}),
  ('absent_type_sections_still_postprocess',{'AudioVisual':{'Gravity':'24'}}),
  ('switch_guided_inherit_prior_postpass',{'OrderBullet':{'ROT':'60'},'AudioVisual':{'Gravity':'6'}}),
  ('guided_minus_one_retains',{'OrderProbe':{'Speed':'-1'}}),
  ('guided_authored_readspeed',{'OrderProbe':{'Speed':'40'}}),
  ('switch_back_ballistic_then_new_gravity',{'OrderBullet':{'ROT':'0'},'AudioVisual':{'Gravity':'3'}}),
  ('negative_rot_preserves_stored_speed',{'OrderBullet':{'ROT':'-1'}}),
  ('completely_empty_pass_preserves_nonzero_rot',{}),
  ('clear_projectile_retains_speed',{'OrderProbe':{'Projectile':'none'}}),
  ('empty_projectile_retains_null',{'OrderProbe':{'Projectile':'','Speed':'50'}}),
  ('rebind_floater_uses_prior_gravity',{'OrderProbe':{'Projectile':'OrderBullet'},'OrderBullet':{'ROT':'0','Floater':'yes'},'AudioVisual':{'Gravity':'-6'}}),
  ('negative_gravity_postpass_then_zero',{'AudioVisual':{'Gravity':'0'}}),
  ('zero_gravity_postpass_then_restore',{'AudioVisual':{'Gravity':'6'}}),
  ('empty_projectile_keeps_live_binding',{'OrderProbe':{'Projectile':'','Range':'-1'}}),
 ]
 for name,sections in cases:
  m.rules_cache(sections);events.clear();m.invoke(0x668bf0,r,(RULES,))
  assert [e['phase'] for e in events]==list(phases.values())
  rows.append(dict(name=name,cached_sections=sections,original_process_visits=list(events),result_speed=s32(m,w+0xa8),result_gravity=s32(m,r+0x16b8),get_speed_500=m.invoke(0x773070,w,(500,))))
 return dict(constructor=ctor,cold_audio_visual=cold,rows=rows)

def generate():
 return dict(sequences=[chronology(),chronology(6)])

def metadata():
 return provenance(scope='Retained Weapon Speed and prior-pass Rules Gravity through complete original RulesClass Process',assumptions=[
  'Original RulesClass665650 and WeaponType771C70 constructors initialize Gravity3 and Speed0. Original Process668BF0 executes all its calls, including full ReadTypeData679A10, Weapon postpass7729F0 and AudioVisual6691E0. The Process function is void; incidental EAX is not treated as a success code.',
  'One Weapon is constructed before the first supplied Process, standing for prior type discovery. Unrelated native type registries are initially empty. Cached minimal source sections supply inputs, not native physical INI loading or complete active-retail type discovery.',
  'The cold_gravity6 sequence first executes full AudioVisual6691E0, matching the inspected cold caller52D132. The complete startup wrapper and destructive reset6686C0 are not executed. Their retained RulesClass lifetime still requires separate evidence.',
  'All phase observations are read-only hooks at actual Process call/return sites668EF0/668EF5/668F56/668F5B. GetSpeed773070 executes after each completed pass, so it sees the newly read Gravity while the stored postpass speed used the earlier value.',
  'Native x87 fixture uses 53-bit precision and truncation0E7F. No arithmetic, reader, postpass, or phase result is replaced with Python output.',
 ],substitutions=[
  'Inherited BulletReader supplies signed-CRC INI cache structures, allocator/delete/CRT/TLS and archive file boundaries. Unrelated Color fallback points to a supplied fixture entry; no color behavior is claimed.',
 ],entry_points={'rules_ctor':0x665650,'weapon_ctor':0x771c70,'rules_process':0x668bf0,'type_data':0x679a10,'weapon_postpass':0x7729f0,'audio_visual':0x6691e0,'read_speed':0x474810,'get_speed':0x773070})


class RetailRulesInputs:
 """Immutable source-selected bytes; the stock MIX decoder remains the owner.

 This is the supported retail registry experiment's IO boundary, not a port
 of native archive startup. Different conflicting bytes stop unless the
 precise pair has original construction/first-match evidence below.
 """
 def __init__(self, directory):
  import hashlib
  from tools.sidebar_oracle.stock import mix, mix_hash
  self.directory=Path(directory);self.mix_hash=mix_hash;self.tables=[];self.requests={}
  for outer,children in (
   ('expandmd01.mix',()),('langmd.mix',('cameomd.mix','audiomd.mix')),
   ('language.mix',('cameo.mix','audio.mix')),
   ('ra2md.mix',('cachemd.mix','localmd.mix','conqmd.mix','genermd.mix','urbann.mix','ubn.mix','isoubnmd.mix','isoubn.mix')),
   ('ra2.mix',('cache.mix','local.mix','conquer.mix','generic.mix','sidec01.mix','sidec02.mix')),
   ('mapsmd03.mix',())):
   raw=(self.directory/outer).read_bytes();table=mix(raw)
   self.tables.append((outer,hashlib.sha256(raw).hexdigest(),table))
   for child in children:
    value=table.get(mix_hash(child))
    if value is not None:self.tables.append((outer+'/'+child,hashlib.sha256(value).hexdigest(),mix(value)))
 def lookup(self,name):
  import hashlib
  key=name.upper()
  if key in self.requests:return self.requests[key][1]
  candidates=[];loose=self.directory/name
  if loose.is_file():candidates.append(('loose',None,loose.read_bytes()))
  candidates.extend((source,archive_sha,table[self.mix_hash(name)])
   for source,archive_sha,table in self.tables if self.mix_hash(name) in table)
  hashes={hashlib.sha256(raw).hexdigest() for _,_,raw in candidates}
  if len(hashes)>1:
   sources={s for s,_,_ in candidates}
   # InitBase5303D4/530400: LOCALMD precedes LOCAL. InitMix530529/
   # 530569: GENERMD precedes GENERIC;5306D4/53075D: CAMEOMD before
   # CAMEO. Original5B3C20 appends;5B4430 searches head to tail.
   pairs={frozenset(('ra2md.mix/localmd.mix','ra2.mix/local.mix')):'ra2md.mix/localmd.mix',
    frozenset(('ra2md.mix/genermd.mix','ra2.mix/generic.mix')):'ra2md.mix/genermd.mix',
    frozenset(('langmd.mix/cameomd.mix','language.mix/cameo.mix')):'langmd.mix/cameomd.mix'}
   winner=pairs.get(frozenset(sources))
   # Original530200 probes EXPANDMD99..00 before base archives;
   # ordinary68765B constructs theater archives later.
   if winner is None and 'expandmd01.mix' in sources and all(
     source=='expandmd01.mix' or source.startswith(('ra2.mix','ra2md.mix')) for source in sources):
    winner='expandmd01.mix'
   if winner is None:raise ValueError(('Conflicting physical native input',name,sorted(sources)))
   candidates.sort(key=lambda row:row[0]!=winner)
  # Absence of an ordinary 8.3-style file is a recorded catalog answer (type
  # images also probe theater/ghost variants and empty-base names like
  # '.SHP'). Anything else stops before a null result is returned.
  suffix='.'+name.rsplit('.',1)[1].lower() if '.' in name else ''
  if not candidates and not (len(suffix)==4 and suffix[1:].isalnum()):
   raise ValueError(('Unadmitted resource namespace',name))
  row=dict(file=name,absent=not candidates,candidates=[dict(source=s,archive_sha256=h,
   sha256=hashlib.sha256(raw).hexdigest(),bytes=len(raw)) for s,h,raw in candidates])
  value=candidates[0][2] if candidates else None
  self.requests[key]=(row,value)
  return value
 def receipt(self):
  return dict(catalog=[dict(source=s,sha256=h,entries=len(t)) for s,h,t in self.tables],
   requests=[row for key,(row,_) in sorted(self.requests.items())],
   boundary='Selected patch/language/cache/local/conquer/generic/NewUrban/side/map sources and loose files; no full archive startup, movies, other theaters, hardware or World loading claim. Absence applies only to this catalog.')


def _retail_history_class():
 # Lazy imports keep the historical selected-cache controls independent.
 import ast,copy,inspect,tempfile,time
 from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_EDX,UC_X86_REG_EIP,UC_X86_REG_ESP,UC_X86_REG_EBP,UC_X86_REG_EBX
 from tools.input_oracle.campaign_start import CampaignReader
 from tools.rules_oracle.bridge_anim_inputs import Reader
 from tools.spatial_oracle.engineer_repair_admission import NativeAudioPlatform
 from tools.native_oracle import RET_MAGIC,NativeExecutionError,image_bytes,initialize_empty_windows_seh,run_checked
 from tools.native_inspect import find_bytes,selected_ranges
 from tools.spatial_oracle.building_body_rules import SP
 from tools.projectile_oracle.bridge_render_inputs_palette import initialize as initialize_palettes

 class RetailRulesHistory(CampaignReader):
  """One unchanged PE/allocator/physical INI/type namespace through Process.

  Runtime values are original outputs. Only bytes, Windows services, RGB565
  storage and successful heap capacity are admitted platform inputs.
  """
  def __init__(self, inputs, *, arena_size=0xC000000):
   self.inputs=inputs;self.phase='construct';self.platform=None;self.full_runner=False
   self.registry_events=[];self.pending_registry={};self.budgets=[];self.phase_seconds=[]
   self.guid_events=[];self.phase_visits=[]
   self.temporary_root=tempfile.TemporaryDirectory(prefix='vera20k-native-rules-')
   super().__init__(Path(self.temporary_root.name),arena_size=arena_size)
   self.platform=NativeAudioPlatform(self,Path(self.temporary_root.name))
   self.platform.configure_transport(wsprintf_formatting=True)
   self.intercepts={node.value for cls in type(self).__mro__[1:] if hasattr(cls,'hook')
    for node in ast.walk(ast.parse(inspect.getsource(cls)))
    if isinstance(node,ast.Constant) and type(node.value) is int and node.value>=0x400000}
   self.intercepts.update(node.value for node in ast.walk(ast.parse(inspect.getsource(NativeAudioPlatform)))
    if isinstance(node,ast.Constant) and type(node.value) is int and node.value>=0x400000)
   self.intercepts.update((0x535B30,0x4739F0,0x4741F0,0x525A60,0x6686C0,0x668BF0,0x679A10,
    0x771C70,0x772FA0,0x772080,0x7C978A,0x527AF9,0x527B0C,self.clock_sink,*self.atomic.imports))
   self.full_runner=True
  def hook(self,u,pc,size,data):
   if hasattr(self,'intercepts') and pc not in self.intercepts:return
   sp=u.reg_read(UC_X86_REG_ESP)
   if pc==0x7C978A:self.ret(0);return
   if self.platform is not None:
    if pc==0x5B40B0:
     name=self.string(u.reg_read(UC_X86_REG_ECX))
     if name.upper() not in self.assets:self.assets[name.upper()]=self.inputs.lookup(name)
    elif pc in (0x65CC59,0x65CBBB):
     name=self.string(self.read32(sp)).lower()
     if name not in self.platform.prepared_files:
      self.platform.configure_transport(prepared_files=dict(self.platform.prepared_files,**{name:self.inputs.lookup(name)}))
    if self.platform.hook(u,pc,size):return
   if Reader.guid_transport(self,u,pc,sp,self.guid_events):return
   key=pc,sp
   if key in self.pending_registry:
    row=self.pending_registry.pop(key)
    pointer=row['pointer'] if row['kind'] in ('read','construct') else u.reg_read(UC_X86_REG_EAX)
    row['returned']=True
    if pointer:
     row.update(id=self.string(pointer+0x24),pointer=pointer)
     if row['kind']=='read':
      wh=self.read32(pointer+0xAC)
      row.update(admitted=bool(u.reg_read(UC_X86_REG_EAX)&255),damage=s32(self,pointer+0xA4),
       warhead=self.string(wh+0x24) if wh else None)
   if pc in (0x771C70,0x772FA0,0x772080):
    kind={0x771C70:'construct',0x772FA0:'find_or_allocate',0x772080:'read'}[pc]
    row=dict(phase=self.phase,kind=kind,entry=f'{pc:08X}',caller=f'{self.read32(sp):08X}',
     count_before=self.read32(0x887578))
    if pc!=0x772FA0:row['pointer']=u.reg_read(UC_X86_REG_ECX)
    row['requested']=self.string(self.read32(sp+4) if pc==0x771C70 else
     u.reg_read(UC_X86_REG_ECX)+(0x24 if pc==0x772080 else 0))
    target=self.read32(sp)
    self.pending_registry[target,sp+(4 if pc==0x772FA0 else 8)]=row
    self.intercepts.add(target);self.registry_events.append(row)
   if pc in (0x535B30,0x4739F0,0x4741F0,0x525A60,0x6686C0,0x668BF0,0x679A10):
    self.phase_visits.append(dict(phase=self.phase,entry=f'{pc:08X}',caller=f'{self.read32(sp):08X}'))
   super().hook(u,pc,size,data)
  def run(self,begin,end,*,context=None):
   pc=begin;started=time.monotonic()
   for part in range(60):
    try:
     run_checked(self.u,pc,end,count=8_000_000,timeout_us=45_000_000,
      context=dict(phase=self.phase,part=part,**(context or {})))
     self.phase_seconds.append(dict(phase=self.phase,seconds=time.monotonic()-started))
     return
    except NativeExecutionError as error:
     if error.diagnostics['reason'] not in ('timeout','instruction_limit_reached'):raise
     self.budgets.append(error.diagnostics);pc=self.u.reg_read(UC_X86_REG_EIP)
   raise RuntimeError(('Native phase exhausted60 finite budgets',self.phase,hex(pc)))
  def invoke(self,entry,obj,args=(),*,timeout_us=10_000_000,context=None):
   if not self.full_runner:return super().invoke(entry,obj,args,timeout_us=timeout_us,context=context)
   self.u.reg_write(UC_X86_REG_ESP,SP);self.u.reg_write(UC_X86_REG_ECX,obj)
   self.u.mem_write(SP,dwords(RET_MAGIC,*args));self.run(entry,RET_MAGIC,context=context)
   return self.u.reg_read(UC_X86_REG_EAX)
  def string_read(self,ini,section,key,capacity=512,default=''):
   out=self.alloc(capacity)
   length=self.invoke(0x528A10,ini,(self.cstring(section),self.cstring(key),self.cstring(default),out,capacity))
   return self.string(out),length
  def load_physical_ini(self,name,*,address=None):
   self.phase='physical:'+name
   ini=address if address is not None else self.alloc(0x58);file=self.alloc(0x6C)
   self.invoke(0x535B30,ini);self.invoke(0x4739F0,file,(self.cstring(name),))
   result=self.invoke(0x4741F0,ini,(file,0,0))
   if result&255!=1:raise ValueError(('Original physical INI load refused',name,result))
   return ini,dict(file=name,returned=result&255,section_count=self.read32(ini+0x2C),check_digest=0)
  def optional_absence(self,name):
   self.phase='optional_guard:'+name;file=self.alloc(0x6C)
   self.invoke(0x4739F0,file,(self.cstring(name),))
   result=self.invoke(0x473C50,file,(0,))&255
   if result:raise ValueError(('Supported registry history requires extending the actual optional pass',name))
   return dict(file=name,exists=False,reader='00473C50')
  def weapon_registry(self):
   result=[];items=self.read32(0x88756C)
   for index in range(self.read32(0x887578)):
    pointer=self.read32(items+index*4);wh=self.read32(pointer+0xAC)
    result.append(dict(id=self.string(pointer+0x24),damage=s32(self,pointer+0xA4),
     warhead=self.string(wh+0x24) if wh else None))
   return result
  def snapshot(self):
   assert not self.pending_registry,self.pending_registry
   return dict(cpu=self.u.context_save(),cursor=self.cursor,
    memory=[(a,bytes(self.u.mem_read(a,b-a+1))) for a,b,_ in self.u.mem_regions()],
    asset_ptr=dict(self.asset_ptr),asset_loaded=copy.deepcopy(self.asset_loaded),
    files=copy.deepcopy(self.platform.files),native_events=copy.deepcopy(self.native_events),
    registry_events=copy.deepcopy(self.registry_events),phase_visits=copy.deepcopy(self.phase_visits))
  def restore(self,state):
   for address,raw in state['memory']:self.u.mem_write(address,raw)
   self.u.context_restore(state['cpu']);self.cursor=state['cursor']
   self.asset_ptr=dict(state['asset_ptr']);self.asset_loaded=copy.deepcopy(state['asset_loaded'])
   self.platform.files=copy.deepcopy(state['files']);self.native_events=copy.deepcopy(state['native_events'])
   self.registry_events=copy.deepcopy(state['registry_events']);self.phase_visits=copy.deepcopy(state['phase_visits'])
   self.pending_registry={};self.check_code()
  def original_type_initializers(self):
   entries=list(struct.unpack('<28I',self.u.mem_read(0x813248,28*4)))
   entries.extend((0x40F490,0x40FB30,0x5F6FF0,0x725350,0x725750,0x68C330))
   ranges=selected_ranges(image_bytes(),None,None,code_only=False);ordered=[]
   for entry in entries:
    slots=[r['address'] for r in find_bytes(ranges,dwords(entry))['matches'] if 0x810000<=r['address']<0x818000]
    assert len(slots)==1,(hex(entry),slots)
    ordered.append((slots[0],entry))
   self.phase='original_type_initializers'
   for _,entry in sorted(ordered):self.invoke(entry,0)
   return [dict(table=f'{slot:08X}',entry=f'{entry:08X}') for slot,entry in sorted(ordered)]
  def campaign_root(self,map_names):
   physical={};pointers={}
   for name,address in (('RULESMD.INI',None),('ARTMD.INI',0x887180),*((name,None) for name in map_names)):
    pointers[name],physical[name]=self.load_physical_ini(name,address=address)
   self.phase='scenario_constructor';self.invoke(0x6832C0,self.scenario);self.seed_streams(31)
   self.rules=self.alloc(0x2000);self.u.mem_write(0x8871E0,dwords(self.rules))
   self.phase='rules_constructor';self.invoke(0x665650,self.rules)
   initialize_empty_windows_seh(self.u);initializers=self.original_type_initializers()
   root=pointers['RULESMD.INI'];self.u.mem_write(0x887048,dwords(root))
   self.phase='palette_platform';initialize_palettes(self,ordinary=True,early_temperat=True)
   for entry,ini in ((0x66D3A0,root),(0x66D480,root),(0x674550,pointers['ARTMD.INI']),(0x6691E0,root),(0x671EA0,root)):
    self.phase=f'cold:{entry:08X}';self.invoke(entry,self.rules,(ini,))
   self.phase='scenario_init_depth';depth=self.begin_scenario_init(pointers[map_names[0]])
   # 6BD7CB passes media -2 in ECX; MixFile CD checks then admit it.
   self.phase='source_selected_media';self.invoke(0x4790B0,0xFFFFFFFE)
   optional=[self.optional_absence(name) for name in ('LANGRULE.INI',*(Path(n).stem+'.INI' for n in map_names))]
   for name in map_names:
    theater,_=self.string_read(pointers[name],'Map','Theater')
    if theater!='NEWURBAN':raise ValueError(('Unsupported selected theater',name,theater))
   self.phase='native_theater_init';self.u.reg_write(UC_X86_REG_ESP,SP)
   self.u.reg_write(UC_X86_REG_EBP,pointers[map_names[0]]);self.u.reg_write(UC_X86_REG_EBX,0)
   self.run(0x687631,0x687660)
   self.phase='root_reset_and_process';before=self.rng_state();self.invoke(0x6686C0,self.rules,(root,))
   self.check_code()
   return dict(pointers=pointers,physical=physical,initializers=initializers,optional=optional,
    initialization_depth=depth,theater=self.read32(self.scenario+0x1258),
    rng_before=before,rng_after=self.rng_state(),weapons=self.weapon_registry())
  def map_process(self,ini,name):
   self.phase='map_process:'+name;self.u.reg_write(UC_X86_REG_ESP,SP)
   self.u.mem_write(SP+0x18,dwords(ini));self.run(0x687744,0x687754)
   self.check_code();return self.weapon_registry()
  def close(self):self.temporary_root.cleanup()
 return RetailRulesHistory


def retail_history(inputs,**kwargs):
 """Construct the full physical history without changing legacy controls."""
 return _retail_history_class()(inputs,**kwargs)

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
