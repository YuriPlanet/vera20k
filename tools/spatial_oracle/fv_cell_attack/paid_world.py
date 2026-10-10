"""Original FV placement, paid pursuit, firing and physical bridge impact continuation.

Reuse Navigation for physical TMP/type/Cell/Terrain/graph initialization and
Mission for actual FV constructor, Unlimbo, command and live object pass.
"""
from pathlib import Path
import json, gzip, struct, sys
from types import MethodType
from unicorn import UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from . import pursuit as frozen
from .publication import finish_vectors
from tools.native_oracle import provenance
from tools.spatial_oracle.anytown_damage.inputs import ASSETS
from tools.native_oracle import run_checked, NATIVE_SHA256, first_difference, _canonical
from tools.spatial_oracle.anytown_damage import mission as mission_owner
from tools.spatial_oracle.anytown_damage.navigation import Navigation, Inputs, extract_tiles, identity, sr
from tools.spatial_oracle.naval_head_producer import Head
from tools.spatial_oracle.building_body_rules import SP, RULES, INI, dwords
from tools.rules_oracle import bridge_anim_lists as lists_owner, bridge_anim_inputs as reader_owner
HERE=Path(__file__).resolve().parent
OUTPUT=HERE/'paid_world_drive_crt.json.gz'
sha=frozen.proof.sha
WORLD_GLOBALS=[(0x87F7E8,0x200),(0xC00000,0x100000),(0xABDC50,0x200),(0x89EA40,12*36),(0xA8ED28,24),(0xA83D80,24),(0xA8E318,24),(0x8B4150,24),(0xB0F4E8,24),(0xB0EDC0,0x1000),(0xB0F670,24),(0xA8E988,24),(0x87E8B8,0xD00)]

def sources():
 return {str(p.relative_to(frozen.proof.REPO)):sha(p.read_bytes())for module in tuple(sys.modules.values())if(name:=getattr(module,'__file__',None))and(p:=Path(name).resolve()).is_relative_to(frozen.proof.REPO/'tools')and p.suffix=='.py'}

def native_worlds():
 """Execute the shared physical owner; retain only original native snapshots."""
 t=identity.theater();r=Inputs(t);tiles,assets=extract_tiles(t);n=Navigation(r,t,tiles,create_actor=False)
 original=json.loads(gzip.decompress(frozen.proof.FACTS.read_bytes()));reference=[original['initial']]+[row['state']for row in original['stages']]
 global_ranges=list(WORLD_GLOBALS)+[(p,4)for p in t['globals']]
 worlds=[]
 def capture(name,index):
  state=n.state()
  for key in('cells','navigation','graphs'):assert state[key]==reference[index][key],(name,key,first_difference(reference[index][key],state[key]))
  worlds.append(dict(stage=name,native_sha256=NATIVE_SHA256,initial=state,case=n.case,ptrs=n.ptrs,theater=t,assets=assets,readers=r.snapshot(),regions=[(a,b-a+1,perms,bytes(n.uc.mem_read(a,b-a+1)))for a,b,perms in n.uc.mem_regions()if a==0x24000000 or a>=0x40000000],globals=[(a,bytes(n.uc.mem_read(a,size)))for a,size in global_ranges],code_hash=n.code_hash,rules_cliff=bytes(n.uc.mem_read(r.rules+0x664,1)),sweeps=n.sweeps))
  print('PASS native physical world',name,'matches all frozen cells/planes/graphs',flush=True)
 capture('healthy',0)
 for index,(name,fn,coord)in enumerate([('damaged',0x57CCF0,n.case['impact']),('collapsed',0x57CCF0,n.case['impact']),('repaired',0x573540,n.case['start'])],1):
  n.stages=[(name,fn,coord)];n.run();capture(name,index)
 return worlds

class Paid(mission_owner.Mission):
 def __init__(self,world):
  self.world_cache=world;self.lifecycle=[];self.navigation_setup=None;self.extra=[];self.timeline=[];self.memwrites=[];self.physical_queries=[];self.source_coord=(87,48);self.source_y_delta=-2;self.control_pending={};self.flow=[];self.flow_pending={};self.candidates=[];self.geometry=[];self.hierarchy=[]
  prepare0=mission_owner.base.prepare;attach0=mission_owner.base.attach_world
  def prepare():
   m,old_source,typ,weapon,cells,inputs,projection=frozen.proof.setup('healthy')
   # The range fixture's supplied frame173 is not this new lifetime's clock.
   # Construct and place the new native actor at frame0, as Mission.setup does.
   m.u.mem_write(0xA8ED84,dwords(0))
   # Seed the otherwise unused global image structs through the original owner,
   # so all three measured streams are valid explicitly controlled inputs.
   for addr in(0x886B88,0xABE890):m.invoke(0x65C6D0,addr,(0,))
   inputs['rng_initialization']=dict(main_seed=0,mapgen_seed=0,scenario_seed=31,entry='0x65C6D0',scenario_owner='bridge_target_composed.setup')
   self.old_source=old_source;self.preparation_projection=projection
   return m,typ,weapon,m.read32(0x8871E0),inputs
  mission_owner.base.prepare=prepare;mission_owner.base.attach_world=self.attach_world
  try:super().__init__()
  finally:mission_owner.base.prepare=prepare0;mission_owner.base.attach_world=attach0
  self.cells=self.resident.ptrs
  self.u.hook_add(UC_HOOK_MEM_WRITE,self.written)
  self.init_allocator()
 def attach_world(self,m,rules):
  u=m.u;w=self.world_cache
  temporary_anims=[m.string(m.read32(m.read32(0x8B4154)+i*4)+0x24)for i in range(m.read32(0x8B4160))]
  for a,size,perms,raw in w['regions']:u.mem_map(a,size,perms);u.mem_write(a,raw)
  for a,raw in w['globals']:u.mem_write(a,raw)
  u.mem_write(rules+0x664,w['rules_cliff'])
  area_initializers=list(struct.unpack('<14I',u.mem_read(0x812A40,14*4)))
  for address in area_initializers:m.invoke(address,0)
  self.inputs['area_initializers']=[f'{a:08x}'for a in area_initializers]
  # The actual full-map AnimType registry becomes the sole lookup owner.
  # Re-execute the selected native readers against that registry so no weapon
  # or warhead retains an orphaned temporary preparation-registry reference.
  rejoined=[]
  for name,path in mission_owner.base.layers():
   if not path.exists():continue
   sections,lines=frozen.proof.lexical(path.read_bytes(),{'HoverMissile','AAHeatSeeker2','HE'})
   m.rules_cache(sections);weapon=self.weapon
   m.invoke(0x772080,weapon,(RULES,));m.invoke(0x46BEE0,m.read32(weapon+0xA0),(RULES,));m.invoke(0x75D3A0,m.read32(weapon+0xAC),(RULES,));m.invoke(0x7729F0,weapon)
   rejoined.append(dict(file=name,sha256=sha(path.read_bytes()),source_lines=lines))
  # Join the existing complete HE impact ART owner in this same reader/VM.
  from tools.projectile_oracle import ifv_impact as impact_owner
  wh=m.read32(self.weapon+0xAC)
  for i in range(m.read32(wh+0x114)):
   name=m.string(m.read32(m.read32(wh+0x108)+i*4)+0x24)+'.SHP'
   path=ASSETS/name
   assert path.is_file(),('Extract the physical HE effect asset into VERA20K_ANYTOWN_INPUTS',name)
   m.assets[name.upper()]=path.read_bytes()
  prior=impact_owner.base_prepare;impact_owner.base_prepare=lambda:(m,self.old_source,self.typ,self.weapon,w['ptrs'],self.inputs)
  try:impact_owner.prepare()
  finally:impact_owner.base_prepare=prior
  assert all(row['image_present']for row in self.inputs['impact_anim_art'])
  art_names={'FV','AAHeatSeeker2','DRAGON'}|{row['name']for row in self.inputs['impact_anim_art']}
  art,_=frozen.proof.lexical((frozen.proof.assets_root()/'ARTMD.INI').read_bytes(),art_names);m.make_ini(art)
  # Complete the original shared world/retirement initialization used by the
  # ordinary MTNK Mission and IFV impact owners; no detach result is replaced.
  m.invoke(0x49F2F0,0)
  from tools.spatial_oracle.fire_error import LEVEL_HEIGHT_INITIALIZERS
  for a in LEVEL_HEIGHT_INITIALIZERS:m.invoke(a,0)
  for a,b in((0x40B540,0x40B5AB),(0x725850,0x725886),(0x4E6D60,0x4E6D96)):
   u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,a,b)
  # Shared LineTrail/GameOptions initialization from ifv_trail_impact. The
  # complete admitted Object.Unlimbo tail will now decide trail construction.
  m.invoke(0x556940,0);u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,0x5569A0,0x5569D6)
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,0xA8EB60);run_checked(u,0x5FA350,0x5FA377)
  self.inputs['options_constructor']=dict(game_speed=m.read32(0xA8EB60),detail_level=m.read32(0xA8EB78),entry='0x5FA350..0x5FA377')
  combat_rows=[]
  for name,path in mission_owner.base.layers():
   if not path.exists():continue
   sections,lines=frozen.proof.lexical(path.read_bytes(),{'CombatDamage'});m.rules_cache(sections)
   if m.invoke(0x526810,RULES,(m.cstring('CombatDamage'),)):
    for a,b in((0x66CD66,0x66CD8C),(0x66C184,0x66C287),(0x66CE2C,0x66CE57)):
     u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,rules);u.reg_write(UC_X86_REG_EDI,RULES);run_checked(u,a,b)
   combat_rows.append(dict(file=name,sha256=sha(path.read_bytes()),source_lines=lines,bridge_strength=m.read32(rules+0x1740)))
  # Read ART for the names actually returned by original SplashList parsing.
  splash=[m.string(m.read32(m.read32(rules+0xBC4)+i*4)+0x24)for i in range(m.read32(rules+0xBD0))]
  water_art,water_lines=frozen.proof.lexical((frozen.proof.assets_root()/'ARTMD.INI').read_bytes(),set(splash));m.make_ini(water_art);water=[]
  for name in splash:
   path=mission_owner.base.ASSETS/(name+'.SHP') if hasattr(mission_owner.base,'ASSETS') else frozen.proof.assets_root()/(name+'.SHP')
   if not path.exists():
    import os
    path=Path(os.environ['VERA20K_ANYTOWN_INPUTS'])/(name+'.SHP')
   assert path.exists(),path
   raw=path.read_bytes();m.assets[name+'.SHP']=raw;ap=m.invoke(0x428B80,m.cstring(name));m.asset_loaded=[];admitted=m.invoke(0x427D00,ap,(INI,));img=m.read32(ap+0xA4);assert img,(name,'missing physical splash SHP')
   water.append(dict(name=name,sha256=sha(raw),admitted_al=admitted&255,frames=struct.unpack('<h',u.mem_read(img+6,2))[0],end=m.read32(ap+0x2C0),rate=m.read32(ap+0x2B0),report=m.read32(ap+0x2F8),assets=list(m.asset_loaded)))
  self.inputs['splash_art']=dict(source_lines=water_lines,rows=water)
  art,_=frozen.proof.lexical((frozen.proof.assets_root()/'ARTMD.INI').read_bytes(),art_names|set(splash));m.make_ini(art)
  # Original CRT startup7CD8B4 ->7CBDAF ->7CBED3 walks812000..815DA4.
  # Its Drive slice812D2C..812D64 contains14 initializers4AF330..4AF520.
  #4AF400 (not mid-body4AF420) writes level height8A07D0 at4AF42B;
  #4AF4A0 derives bridge scale8A07C4. Image-zero globals incorrectly reject
  # reached-destination4B2196 and re-enter path search at frame7 of this FV.
  # Execute the original dispatcher/table before actor construction; no supplied
  # scale, movement result or floating-point calculation replaces native code.
  drive_table=bytes(u.mem_read(0x812D2C,56))
  drive_before={f'{a:08x}':m.read32(a)for a in(0x8A07D0,0x8A07C4)}
  m.invoke(0x7CBED3,0,(0x812D2C,0x812D64))
  self.inputs['drive_crt']=dict(dispatcher='007cbed3',table_begin='00812d2c',table_end='00812d64',table_bytes=drive_table.hex(),initializers=[f'{a:08x}'for a in struct.unpack('<14I',drive_table)],before=drive_before,after={f'{a:08x}':m.read32(a)for a in(0x8A07D0,0x8A07C4)})
  scenario=m.read32(0xA8B230);m.invoke(0x6B8AE0,scenario);flag_before=m.read32(scenario)
  map_sections,map_lines=frozen.proof.lexical(frozen.proof.MAP.read_bytes(),{'SpecialFlags'});m.rules_cache(map_sections);u.mem_write(0xA8B238,dwords(5));m.invoke(0x6B8CA0,scenario,(RULES,))
  self.inputs['impact_combat_layers']=combat_rows;self.inputs['special_flags']=dict(constructor=flag_before,after_physical_map=m.read32(scenario),map_sections=map_sections,mode=5)
  # Reuse the resident snapshot/coordinate owner without transplanting donor
  # observer sinks: every Recalc, membership, shroud and graph consumer runs.
  n=Navigation.__new__(Navigation);n.uc=n.u=u;n.ptrs=w['ptrs'];n.coords={v:k for k,v in n.ptrs.items()};n.case=w['case'];n.width=sum(n.case['size']);n.side=n.width+1;n.rngs={'main':0x886B88,'scenario':m.read32(0xA8B230)+0x218,'mapgen':0xABE890};n.code_hash=w['code_hash'];n.trace=[];n.pending={};n.observe=lambda *args:None
  return n,dict(native_stage=w.get('stage','healthy'),raw_world_regions=[dict(base=a,bytes=size,sha256=sha(raw))for a,size,_,raw in w['regions']],native_initial_sha256=sha(_canonical(w['initial'])),whole_world_physical=True,reader_heap='0x28000000',donor_heap='0x24000000',native_reader_inputs=w['readers'],temporary_preparation_anims=temporary_anims,rejoined_weapon_readers=rejoined)
 def init_allocator(self):
  m=self.m;u=self.u;u.mem_map(0x47000000,0x1000000);m.long_heap=0x47000000;m.path_events=[]
  proxy=Head.__new__(Head);proxy.__dict__=m.__dict__;old=lists_owner.Lists.hook
  def allocator(owner,uc,pc,n,d):
   if owner is m and pc in(0x7C8E17,0x7C8B3D):return Head.hook(proxy,uc,pc,n,d)
   return old(owner,uc,pc,n,d)
  self.allocator0=old;lists_owner.Lists.hook=allocator;m.alloc=MethodType(Head.alloc,m);m.invoke=MethodType(Head.invoke,m)
  # Actual Pathfinder arrays consume the actual full Map dimensions.
  for p in(0x49F0E0,0x49F190,0x49F2F0,0x49F2D0,0x49F280,0x49F3A0):m.invoke(p,0)
  m.invoke(0x42A6D0,0x87E8B8);m.invoke(0x42AC00,0x87E8B8,(0x87F7E8+0xEC,));m.invoke(0x42C1C0,0x87E8B8)
 def guard_cell(self,xy):assert xy in self.cells,('Outside physical full map',xy)
 def observe(self,u,a,n,d):
  m=self.m;sp=u.reg_read(UC_X86_REG_ESP)
  order_names={0x55B610:'logic_visit',0x7360C0:'unit_ai',0x4DA530:'foot_ai',0x6F9E50:'techno_ai',0x5B3060:'dispatch',0x4D4DC0:'attack_handler',0x7414E0:'approach',0x4B0500:'drive_process',0x4B0F20:'drive_paid_points',0x4D3780:'mark',0x5F6940:'set_position',0x736DF0:'firing_update',0x741340:'unit_fire',0x6FDD50:'techno_fire',0x466380:'bullet_ctor',0x68BCB0:'next_identity',0x55BAA0:'logic_register',0x4666E0:'bullet_ai',0x4690B0:'detonate',0x489280:'area_damage',0x57CCF0:'concrete_damage',0x421EA0:'anim_ctor',0x423AC0:'anim_ai',0x7258D0:'notify_expiry',0x5F65F0:'uninit',0x725C70:'deferred_drain',0x466560:'bullet_dtor',0x426590:'anim_dtor',0x556A20:'line_trail_ctor',0x556B30:'line_trail_detach',0x736990:'facing_update',0x5B3570:'commence',0x736465:'ready_before_foot',0x7366EF:'ready_after_firing',0x7362ED:'alive_after_warp_drive',0x736305:'warp_guard_result',0x73631D:'tube_guard_result',0x7365BB:'alive_after_foot',0x73646B:'ready_before_foot_result',0x7366F5:'ready_after_firing_result',0x7365ED:'facing_update_return'}
  if a in order_names:
   this=u.reg_read(UC_X86_REG_ECX);row=dict(kind=order_names[a],pc=f'{a:08x}',caller=f'{m.read32(sp):08x}',frame=self.frame,phase=self.phase,this=f'{this:08x}',logic_count=m.read32(0x87F788),scenario_id=m.read32(m.read32(0xA8B230)+0x214))
   if a==0x55B610:row.update(index=u.reg_read(UC_X86_REG_ESI),native_id=m.read32(this+0x10),vtable=f'{m.read32(this):08x}')
   if self.src:row.update(source_xyz=list(struct.unpack('<3i',u.mem_read(self.src+0x9C,12))),source_alive=u.mem_read(self.src+0x90,1)[0],eax=u.reg_read(UC_X86_REG_EAX),mission=struct.unpack('<i',u.mem_read(self.src+0xAC,4))[0],queued=struct.unpack('<i',u.mem_read(self.src+0xB4,4))[0])
   self.timeline.append(row)
  if a==0x737BA0 and self.phase=='placement':
   out=m.read32(sp+4);p=self.resident.ptrs[self.source_coord]
   # Supply only the requested spawn coordinate before original Unlimbo.
   m.u.mem_write(out,dwords(self.source_coord[0]*256+128,self.source_coord[1]*256+128+self.source_y_delta,u.mem_read(p+0x11B,1)[0]*104))
   self.extra.append(dict(kind='spawn_coordinate_input',xyz=list(struct.unpack('<3i',u.mem_read(out,12))),facing=m.read32(sp+8)))
   nav=self.resident.nav_snapshot();graphs=self.resident.graph_snapshot();native=self.world_cache['initial']
   assert graphs==native['graphs'],first_difference(native['graphs'],graphs)
   nav_kind='unchanged_native_live'
   if nav!=native['navigation']:
    stage=self.world_cache.get('stage','healthy');assert stage!='healthy',first_difference(native['navigation'],nav)
    path=frozen.proof.REPO/'tools/spatial_oracle/anytown_navigation_restore.json.gz';restored=json.loads(gzip.decompress(path.read_bytes()))['cases'][{'damaged':0,'collapsed':1,'repaired':2}[stage]]['after']['navigation']
    assert nav==restored,first_difference(restored,nav);nav_kind='native_base_rebuilt_by_Mission_setup56C510'
   self.navigation_setup=dict(base_identity=nav_kind,base_sha256=sha(_canonical(nav)),graph_identity='unchanged_native_live',graphs_sha256=sha(_canonical(graphs)),cell_plane_sha256=sha(_canonical(self.resident.cell_plane())))
   assert self.resident.cell_plane()==native['cells'],first_difference(native['cells'],self.resident.cell_plane())
  if a in(0x7353C0,0x737BA0):
   row=dict(kind='constructor'if a==0x7353C0 else'unlimbo',pc=f'{a:08x}',caller=f'{m.read32(sp):08x}',before=self.lifecycle_state())
   self.lifecycle.append(row)
  if a in self.control_pending:
   for row in self.control_pending.pop(a):
    row['returned_eax']=u.reg_read(UC_X86_REG_EAX)
    if row.get('pc')=='0047d2b0':row['after']=self.resident.snapshot(int(row['this'],16))
    if row.get('kind')in('constructor','unlimbo'):row['after']=self.lifecycle_state()
  if a in(0x4DB1A0,0x50C050,0x4D3710,0x68BCB0):
   row=dict(kind='speed_or_identity',pc=f'{a:08x}',caller=f'{m.read32(sp):08x}',frame=self.frame,this=f'{u.reg_read(UC_X86_REG_ECX):08x}',args=list(struct.unpack('<3I',u.mem_read(sp+4,12))))
   if a==0x68BCB0:row['scenario_id_before']=m.read32(m.read32(0xA8B230)+0x214)
   self.extra.append(row);self.control_pending.setdefault(m.read32(sp),[]).append(row)
  if a in(0x4B0500,0x4B2860,0x4B2900,0x4D3920,0x4CBBA0,0x42C900,0x429A90,0x4D3780,0x47D2B0,0x47EA90,0x47E8A0,0x4AFD40):
   row=dict(kind='movement_entry',pc=f'{a:08x}',caller=f'{m.read32(sp):08x}',frame=self.frame,this=f'{u.reg_read(UC_X86_REG_ECX):08x}',args=list(struct.unpack('<4I',u.mem_read(sp+4,16))))
   if a in(0x47D2B0,0x47EA90,0x47E8A0):
    row.update(cell=self.resident.snapshot(u.reg_read(UC_X86_REG_ECX)))
    if a==0x47D2B0:
     tile=row['cell']['tile'];assert 0<=tile<m.read32(0xA8ED38),('Invalid physical tile before Recalc',row)
     typ=m.read32(m.read32(0xA8ED2C)+tile*4);tmp=m.read32(typ+0xA4);assert tmp,('Missing physical TMP',row)
     row.update(tile_type=f'{typ:08x}',tmp=f'{tmp:08x}',tmp_header=bytes(u.mem_read(tmp,16)).hex());self.control_pending.setdefault(m.read32(sp),[]).append(row)
   self.extra.append(row)
  if a==0x73F0A0:
   p=m.read32(sp+4);xy=tuple(struct.unpack('<hh',u.mem_read(p+0x24,4)));self.guard_cell(xy)
   row=dict(pc=f'{a:08x}',caller=f'{m.read32(sp):08x}',frame=self.frame,cell=self.resident.snapshot(p));self.physical_queries.append(row);self.control_pending.setdefault(m.read32(sp),[]).append(row)
  if self.phase=='logic':
   sizes=[len(x)for x in(self.flow,self.candidates,self.geometry,self.hierarchy)]
   frozen.Continuation.observe_flow(self,a,sp)
   for rows,start in zip((self.flow,self.candidates,self.geometry,self.hierarchy),sizes):
    for row in rows[start:]:row['frame']=self.frame
  super().observe(u,a,n,d)
 def written(self,u,access,a,size,value,data):
  assert not 0x401000<=a<0x7E1000
  if not self.src:return
  loco=self.m.read32(self.src+0x674)-4
  if self.src<=a<self.src+0x1000 or loco<=a<loco+0x100:
   self.memwrites.append(dict(pc=f'{u.reg_read(UC_X86_REG_EIP):08x}',frame=self.frame,phase=self.phase,owner='actor'if self.src<=a<self.src+0x1000 else'drive',offset=a-(self.src if self.src<=a<self.src+0x1000 else loco),size=size,value=value))
 def setup(self):
  lexical0=mission_owner.base.lexical
  def fv_lexical(raw,wanted):return lexical0(raw,(set(wanted)-{'MTNK'})|({'FV'}if'MTNK'in wanted else set()))
  mission_owner.base.lexical=fv_lexical
  invoke=self.m.invoke
  def observed_invoke(fn,this,args=(),**kwargs):
   result=invoke(fn,this,args,**kwargs)
   if fn in(0x7353C0,0x737BA0):
    row=self.lifecycle[-1];assert int(row['pc'],16)==fn
    row.update(returned_eax=result,after=self.lifecycle_state())
   return result
  self.m.invoke=observed_invoke
  try:super().setup()
  finally:mission_owner.base.lexical=lexical0;self.m.invoke=invoke
  assert self.m.string(self.typ+0x24)=='FV'
  self.after_setup_full=self.full_state()
 def tick(self):
  # Observer-rich original searches can take longer than the generic ten-second
  # VM wall-clock guard, especially beside an independent native replay.
  # Keep the original Mission-owned calls and instruction-count guards intact.
  runner=mission_owner.run_checked
  def bounded(u,begin,end,**kwargs):
   kwargs['timeout_us']=60_000_000
   return runner(u,begin,end,**kwargs)
  mission_owner.run_checked=bounded
  try:super().tick()
  finally:mission_owner.run_checked=runner
 def lifecycle_state(self):
  m=self.m;u=self.u
  return dict(actor=self.state(),native_id=m.read32(self.src+0x10),scenario_next_id=m.read32(m.read32(0xA8B230)+0x214),actor_bytes=bytes(u.mem_read(self.src,0x1000)).hex(),rng={k:bytes(u.mem_read(addr,1012)).hex()for k,addr in self.resident.rngs.items()})
 def full_state(self):
  m=self.m;u=self.u;p=self.src;loco=m.read32(p+0x674)-4
  result=dict(actor=self.state(),actor_bytes=bytes(u.mem_read(p,0x1000)).hex(),drive_bytes=bytes(u.mem_read(loco,0x100)).hex(),rng={k:bytes(u.mem_read(addr,1012)).hex()for k,addr in self.resident.rngs.items()},nav=f'{m.read32(p+0x5A4):08x}',drive_destination=list(struct.unpack('<3i',u.mem_read(loco+0x34,12))),drive_head=list(struct.unpack('<3i',u.mem_read(loco+0x40,12))),logic_count=m.read32(0x87F788),source_cell=self.resident.snapshot(self.cells[self.source_coord]),target_cell=self.resident.snapshot(self.cells[87,54]),area_level_height=m.read32(0x89E870),all_bullets=[dict(ptr=f'{b:08x}',raw=bytes(u.mem_read(b,0x180)).hex(),native_id=m.read32(b+0x10),xyz=list(struct.unpack('<3i',u.mem_read(b+0x9C,12))),alive=u.mem_read(b+0x90,1)[0])for b in self.bullets],bullet_count=m.read32(0xA8ED50),anim_count=m.read32(0xA8E9B8),deferred_count=m.read32(0xB0F6A8))
  result['nav_cell']=list(self.resident.coords[m.read32(p+0x5A4)])if m.read32(p+0x5A4)else None
  result['scenario_next_id']=m.read32(m.read32(0xA8B230)+0x214)
  result['span']=[self.resident.snapshot(self.cells[x,y])for y in range(50,60)for x in range(85,90)]
  result['logic_order']=[dict(ptr=f'{b:08x}',native_id=m.read32(b+0x10),vtable=f'{m.read32(b):08x}')for i in range(m.read32(0x87F788))for b in[m.read32(m.read32(0x87F77C)+i*4)]]
  result['line_trails']=[dict(ptr=f'{t:08x}',owner=f'{m.read32(t+4):08x}',raw=bytes(u.mem_read(t,0x210)).hex())for i in range(m.read32(0xABCB88))for t in[m.read32(m.read32(0xABCB7C)+i*4)]]
  if hasattr(self,'country'):result['speed_inputs']=dict(country_118=bytes(u.mem_read(self.country+0x118,4)).hex(),country_12c=bytes(u.mem_read(self.country+0x12C,4)).hex(),house_5394=bytes(u.mem_read(self.house+0x5394,4)).hex(),type_speed=m.read32(self.typ+0x678))
  return result
 def close(self):lists_owner.Lists.hook=self.allocator0

def run(world,frames=60):
 original_heaps=(lists_owner.HEAP,reader_owner.HEAP);lists_owner.HEAP=reader_owner.HEAP=0x28000000
 q=None;failure=None;states=[]
 try:
  q=Paid(world);q.setup();states.append(q.full_state());print('Native FV Unlimbo',q.state(),flush=True)
  for _ in range(frames):
   q.tick();states.append(q.full_state());print('tick',q.frame,'xyz',q.state()['position'],'head',states[-1]['drive_head'],'shots',len(q.shots),flush=True)
   if len(q.impacts)>=2 and states[-1]['bullet_count']==0 and states[-1]['anim_count']==0:break
 except Exception as exc:
  cs=Cs(CS_ARCH_X86,CS_MODE_32);failure=dict(error=str(exc),pc=f'{q.u.reg_read(UC_X86_REG_EIP):08x}'if q else None,trace=[f'{a:08x}: '+ '; '.join(f'{i.mnemonic} {i.op_str}'for i in cs.disasm(bytes(q.u.mem_read(a,15)),a,count=1))for a in q.trace]if q else [])
 finally:
  if q:q.close()
  lists_owner.HEAP,reader_owner.HEAP=original_heaps
 result=dict(schema=1,failure=failure,source_pins=sources(),states=states,world=getattr(q,'world',None),inputs=getattr(q,'inputs',None),events=q.events if q else [],extra=q.extra if q else [],timeline=q.timeline if q else [],writes=q.memwrites if q else [],queries=q.physical_queries if q else [],flow=q.flow if q else [],candidates=q.candidates if q else [],geometry=q.geometry if q else [],hierarchy=q.hierarchy if q else [],frames=q.frames if q else [],shots=q.shots if q else [],last=q.full_state()if q and q.src else None)
 result.update(lifecycle=q.lifecycle if q else [],navigation_setup=q.navigation_setup if q else None,impacts=q.impacts if q else [],command_bytes=getattr(q,'command_bytes',None),before_command=getattr(q,'before_command',None),after_command=getattr(q,'after_command',None))
 print('RESULT',failure,flush=True)
 if q:assert sha(bytes(q.u.mem_read(0x401000,0x3E0000)))==world['code_hash']
 return result

def generate():
 rows=[]
 for world in native_worlds():
  print('STAGE',world['stage'],flush=True);result=run(world,80)
  assert result['failure'] is None,(world['stage'],result['failure'])
  assert len(result['shots'])>=2 and len(result['impacts'])>=2,(world['stage'],'first burst did not impact')
  assert not result['last']['bullet_count'] and not result['last']['anim_count'],(world['stage'],'native burst did not finish in the declared bound')
  # Source census belongs to the current replay metadata. Every actual native
  # field and observed heap digest remains protected by the frozen publisher.
  result.pop('source_pins');rows.append(dict(stage=world['stage'],result=result))
 return dict(schema=1,native_sha256=NATIVE_SHA256,cases=rows)

def metadata():
 result=provenance(scope=__doc__,entry_points={'unit_ctor':0x7353C0,'unit_unlimbo':0x737BA0,'command':0x4C6CB0,'live_logic':0x55B5FF,'unit_ai':0x7360C0,'foot_ai':0x4DA530,'mission_attack':0x4D4DC0,'approach':0x4D5690,'drive_process':0x4B0500,'drive_paid_points':0x4B0F20,'firing_update':0x736DF0,'facing_update':0x736990,'bullet_ai':0x4666E0,'area_damage':0x489280,'concrete_damage':0x57CCF0,'retirement':0x725C70},assumptions=[
  'Four physical Anytown states are produced by the existing complete native Navigation owner: healthy, one original57CCF0 damage, second57CCF0 collapse, and original573540 repair. Original TMP/Cell/Terrain/Recalc and full graph initialization run; raw whole-world heaps and selected globals are composed unchanged into the existing FV Mission VM. Frozen native full cells/planes/graphs compare exactly at every state. Physical worlds are rebuilt on every replay; raw world-region digests are also protected by the original paid packet canonical guard. No retail heap cache is committed or required.',
  'The original Mission setup executes Unit/Foot/Techno constructors, country/side constructor lists and original Americans reader, actual Drive COM factory, and full Unit/Techno/Object Unlimbo and Cell/Display/Logic membership. Explicit coordinate input is[22400,12414,416], two leptons north of87,48 center, facing128. This is a supplied spawn position, not native center adjustment. The original Event attack command targets Cell87,54 at frame1.',
  'The existing single-human-House fixture is retained: supplied storage binds native Americans country, combat multipliers1, human/current-player flags and selected valid counters. Full House construction, Scenario load chronology, startup native ID prefix, mouse/UI admission and multiplayer command queue are not claimed. Full RNG and NextID are captured before and after the measured Unit constructor/Unlimbo. Each case retains original Scenario Seed31 from the shared composed reader owner; original65C6D0 explicitly initializes Main and MapGen with seed0. No donor mutation RNG continuation is imported. All three controlled native structs are retained in full.',
  'Original UnitAI/FootAI/TechnoAI, native Attack dispatch gate/cadence, full radial candidate/graph/path admission, Drive points, per-object firing/facing, original Bullet/Anim constructors and live append order execute. Newly admitted Bullets and Anims can receive AI in the same original pass. Original frame commit and deferred destructor drain execute. Other Scenario/House/Terrain/global logic phases are excluded.',
  'Physical FV, HoverMissile, AAHeatSeeker2, HE, DRAGON, impact HE and parsed SplashList ART/SHP inputs run through existing original readers. GameOptions constructor default speed3/detail2 and LineTrail registry initialization run. Complete Bullet Unlimbo chooses LineTrail creation; Bullet retirement detaches it. Tactical drawing/global trail presentation cadence is excluded, so detached trails may remain in that separate presentation registry.',
  'The measured original object loop runs until the first burst Bullets and Anims have drained, with a declared80-frame upper bound. Full source/Drive bytes, full three RNG streams, memberships, all passed original event addresses and impacted physical span snapshots are preserved. Sound registry/binding is omitted; sound playback uses the inherited explicit boundary. No full audio/Main RNG claim.'],substitutions=[
  'Existing bounded native fixture owners provide OS string/CLSID/COM transport, bump allocation and deallocation, selected native INI cache entries and physical SHP transport. Actual Drive/Bullet COM factories execute. Existing setup-only wall-clock and type-visual sinks, radar and sound boundaries are retained and reported as events.',
  'Whole physical-world native heap/global snapshots are supplied inputs; source constructor clock is reset to frame0 before the new lifetime. The existing Mission setup then runs original base connectivity again; the packet distinguishes unchanged native live base labels from exact native post-load rebuilt labels, while retaining the donor live hierarchy.',
  'No original gameplay code or native return values are patched. VM wall-clock allowance for the unchanged Mission-owned loop is60seconds with the original instruction-count guard, to accommodate tracing and concurrent independent replay.'])
 result.update(source_pins=sources(),harness_sha256=sha(Path(__file__).read_bytes()),frozen_source_sha256=json.loads((HERE/'promotion.json').read_bytes())['results']['paid_world_drive_crt.json']['frozen_source_sha256'],supersedes='paid_world.json.gz',publication_changes=['Imported-source census moves to this metadata; the external pickle transport hash is omitted. The original Drive CRT table now executes before construction; the previous image-zero packet is preserved as superseded evidence.','The shared publisher replaces copied lexical INI entries with hashes. The portable native generator must match the original frozen canonical projection even under --write.'])
 return result


if __name__=='__main__':finish_vectors(generate,OUTPUT,provenance=metadata)
