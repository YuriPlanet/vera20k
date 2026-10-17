"""Original radiation boundary regression after native Verses precision correction.

This records selected final health/admission, not full radiation equivalence.
See radiation_damage_boundary.md for the observed producer precision residual.
"""
import hashlib,json,os,struct,sys
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.rules_oracle.bridge_landing_inputs import Landing,WH
from tools.spatial_oracle.building_body_rules import TYPE,INI,SP,dwords
from tools.native_oracle import run_checked,RET_MAGIC,finish_vectors,provenance
from tools.spatial_oracle.estimated_damage import CLASSES
RAW='100%,100%,100%,50%,10%,10%,0%,0%,0%,100%,100%'
def execute(frame=16,cell=(6,5),armor=5,category='unit', *, field=None):
 cls=CLASSES[category]
 m=Landing();u=m.u
 visits=[]
 u.hook_add(UC_HOOK_CODE,lambda _u,a,_s,_d: visits.append(a) if a in (0x65C780,0x65C7E0,0x65B9C0,0x487CB0,0x489180,0x5F5390) else None)
 wh=m.warhead('RadSite')
 m.make_ini(field['sections'] if field else {'RadSite':{'Verses':RAW},'Radiation':{'RadDurationMultiple':'1','RadApplicationDelay':'16','RadLevelMax':'500','RadLevelDelay':'90','RadLevelFactor':'.2','RadSiteWarhead':'RadSite'}})
 for key,(kp,a,b) in WH.items():
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,wh);u.reg_write(UC_X86_REG_EDI,INI);u.reg_write(UC_X86_REG_EBP,wh+0x24)
  run_checked(u,a,b)
 m.invoke(0x66CF70,TYPE,(INI,));assert m.read32(TYPE+0x1834)==wh
 site=m.alloc(0x100);table=m.alloc(0x100000);obj=m.alloc(0x1000);typ=m.alloc(0x2000);scenario=m.alloc(0x2000)
 u.mem_write(0xA8B230,dwords(scenario));u.mem_write(0x87f924,dwords(table,0x40000))
 cells={}
 for y in (range(48) if field else range(3,8)):
  for x in (range(48) if field else range(3,8)):
   c=m.alloc(0x200);cells[x,y]=c;u.mem_write(c,dwords(0x7E4EEC));u.mem_write(c+0x24,struct.pack('<hh',x,y));u.mem_write(c+0x44,dwords(-1));u.mem_write(table+(y*512+x)*4,dwords(c))
 if field:
  stages,source_inputs=overlap_actions(m,cells,field)
 else:
  u.mem_write(site+0x40,struct.pack('<hh',5,5));m.invoke(0x65B4D0,site,(2,));m.invoke(0x65B4F0,site,(500,))
  m.invoke(0x65B9C0,site)
 levelbits=struct.unpack('<Q',u.mem_read(cells[cell]+0xf0,8))[0]
 u.mem_write(obj,dwords(cls['object_vtable']));u.mem_write(obj+cls['type_offset'],dwords(typ));u.mem_write(typ,dwords(cls['type_vtable']));u.mem_write(typ+0x9c,dwords(armor));u.mem_write(typ+0xa0,dwords(300));u.mem_write(obj+0x6c,dwords(300));u.mem_write(obj+0x90,b'\1');u.mem_write(obj+0x9c,dwords(cell[0]*256+128,cell[1]*256+128,0));u.mem_write(0xA8ED84,dwords(frame))
 u.reg_write(UC_X86_REG_ESI,obj);u.reg_write(UC_X86_REG_EBX,0);u.reg_write(UC_X86_REG_ESP,SP)
 endpoint=run_checked(u,0x4DA554,(0x4DA629,0x4DA63B))
 out=dict(frame=frame,cell=cell,armor=armor,category=category,verses_bits=m.whstate(wh)['verses_bits'],factor_bits=f'{struct.unpack("<Q",u.mem_read(TYPE+0x1818,8))[0]:016x}',level_bits=f'{levelbits:016x}',level=struct.unpack('<d',u.mem_read(cells[cell]+0xf0,8))[0],admitted=endpoint==0x4DA629)
 if endpoint==0x4DA629:
  sp=u.reg_read(UC_X86_REG_ESP);damage_ptr=m.read32(sp);base=struct.unpack('<i',u.mem_read(damage_ptr,4))[0];args=[m.read32(sp+4*i) for i in range(7)]
  assert args[1:]==[0,wh,0,0,1,0],args
  out['base_damage']=base;out['cell_damaging_level']=m.invoke(0x487CB0,cells[cell])
  # Original kernel uses __fastcall: damage ECX, warhead EDX, armor/distance stack.
  u.mem_write(SP,dwords(RET_MAGIC,armor,0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,base);u.reg_write(UC_X86_REG_EDX,wh)
  run_checked(u,0x489180,RET_MAGIC)
  out['kernel_damage']=u.reg_read(UC_X86_REG_EAX)
  dp=m.alloc(4);u.mem_write(dp,dwords(base));out['object_receive_result']=m.invoke(0x5F5390,obj,(dp,0,wh,0,0,1,0));out['final_health']=m.read32(obj+0x6c)
 out['final_health']=m.read32(obj+0x6c)
 out['visited']=[f'{a:08X}' for a in visits]
 out['rng_calls']=[f'{a:08X}' for a in visits if a in (0x65C780,0x65C7E0)]
 assert not out['rng_calls']
 if field:
  out.pop('level')  # Raw bits also represent exceptional native cell values.
  out.setdefault('cell_damaging_level',struct.unpack('<i',dwords(m.invoke(0x487CB0,cells[cell])))[0])
  out.setdefault('base_damage',0)
  out.update(name=field['name'],actions=field['actions'],stages=stages,source_inputs=source_inputs)
 return out

def generate():
 rows=[execute(f,c,a,k) for f in (15,16,17) for c,a,k in (((5,5),0,'infantry'),((6,5),5,'unit'))]
 return dict(schema_version=1,input=dict(verses=RAW,radiation={'RadDurationMultiple':'1','RadApplicationDelay':'16','RadLevelMax':'500','RadLevelDelay':'90','RadLevelFactor':'.2','RadSiteWarhead':'RadSite'},site_level=500,spread=2,source_cell=[5,5],initial_health=300),rows=rows)

def metadata():
 return provenance(scope='Independent original RadSite warhead percentage reader, Radiation reader, flat-map RadSite spread, Foot radiation admission/producer and direct Object nonlethal receiver. Six bounded rows; no whole Radiation or concrete Techno receiver parity claim.',assumptions=[
 'Synthetic radiation_rules fixture input: eleven authored Verses tokens, RadLevelFactor .2, application delay16, cap500, level500/spread2, flat cells3..7 around5,5, fresh objecthealth300 and armor0/5. These are explicit regression inputs, not a claim of full retail layer loading.',
 'Original Warhead factory/constructor and complete selected scalar/Verses blocks75DDCC..75DE5A execute. Full original Rules Radiation reader66CF70 executes using supplied lexical INI indexes; original CRT floating scanner initialization executes. FPCW0E7F.',
 'Original spread/radius setter65B4D0, level/duration setter65B4F0 and full25-cell spread65B9C0 execute using original Cell vtables/getters and map lookup5657A0. Fresh zeroed cell radiation fields. No light activation or site AI/decay scheduling.',
 'Foot4DA554 begins after its ordinary TechnoAI/alive prologue, with valid fresh nonimmune ground Infantry/Unit objects and EBX0; stops before concrete ReceiveDamage4DA629 or at no-application continuation4DA63B. Exact damage ABI asserted: distance0, selectedwarhead, source0, ignoredefenses0, arg6true, sourcehouse0.',
 'The emitted base then independently enters original kernel489180 and original Object5F5390 nonlethal receiver, retaining actual Infantry/Unit object/type vtables. Concrete Infantry/Unit/Techno receivedamage modifiers and downstream nonlethal effects are not executed. This fixture corresponds to ordinary unranked targets with neutral effective modifiers, but does not prove those omitted gates.',
 'Rows atframes15/16/17 are independent fixtures, not native elapsed-site scheduling. No source in this comparison emits RNG draws; site duration stores are from original65B4F0. No death/detach calls are reached.',
 'Native side cell stores299.99999999999994, clamps/truncates299 and emits59. Rust currently stores300 and emits60; both resolve5 versus corrected10%heavy. Native RadLevelFactor ReadDouble bits3fc99999a0000000 differ from current directf64 parser. These are separately recorded unresolved radiation prerequisites, not matching intermediates.'],substitutions=[
 'Landing reader helper supplies cached lexical INI entries/CRC indexes, allocator bump storage, no-op delete and CRTTLS storage. No original executable instructions replaced.',
 'Fixture supplies sparse actual map cell table, explicit instance/type memory and neutral scenario/rules state. Receiver entry is selected explicitly after capturing original Foot arguments; no claim that the intervening concrete receiver body executed.'
 ],entry_points={'warhead_verses':0x75DDCC,'radiation_reader':0x66CF70,'spread_setter':0x65B4D0,'level_setter':0x65B4F0,'spread':0x65B9C0,'foot_gate':0x4DA554,'cell_level':0x487CB0,'foot_receiver_call':0x4DA629,'warhead_damage':0x489180,'object_receiver':0x5F5390})

# Additive field controls use the same reader/map/Foot/Object owner above.
# The default six-row payload and its historical sidecar remain unchanged.
OVERLAP_KEYS = {
 'Radiation': ('RadDurationMultiple','RadApplicationDelay','RadLevelMax','RadLevelDelay',
               'RadLightDelay','RadLevelFactor','RadLightFactor','RadTintFactor','RadColor','RadSiteWarhead'),
 'RadSite': ('Verses',),
 'RadEruptionWeapon': ('RadLevel',),
 'RadEruptionWarhead': ('CellSpread',),
 'DOG': ('Strength','Armor','ImmuneToRadiation'),
}

def overlap_inputs():
 from tools.projectile_oracle.bridge_render_inputs import lexical
 root=Path(os.environ['VERA20K_RADIATION_INPUTS'])
 sections={};layers=[]
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.mmx'):
  path=root/name
  if not path.exists():
   if name!='LANGRULE.INI':raise FileNotFoundError(path)
   layers.append(dict(file=name,absent=True));continue
  raw=path.read_bytes();parsed,_=lexical(raw,set(OVERLAP_KEYS))
  selected={section:{key:value for key,value in values.items() if key in OVERLAP_KEYS[section]}
            for section,values in parsed.items()}
  layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),sections=selected))
  for section,values in selected.items():sections.setdefault(section,{}).update(values)
 return sections,layers

def overlap_actions(m,cells,field):
 """Execute original field operations; no Python falloff/float arithmetic."""
 u=m.u;sections=field['sections']
 # Read source weapon and Warhead independently; they are not fixture constants.
 level=m.invoke(0x5276D0,INI,(m.cstring('RadEruptionWeapon'),m.cstring('RadLevel'),0))
 eruption=m.warhead('RadEruptionWarhead')
 for _,(_,a,b) in WH.items():
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,eruption)
  u.reg_write(UC_X86_REG_EDI,INI);u.reg_write(UC_X86_REG_EBP,eruption+0x24);run_checked(u,a,b)
 spread=int(m.whstate(eruption)['cell_spread'])
 sites={};stages=[];frame=0
 # Declared empty vector storage; original RadSite constructor owns append.
 u.mem_write(0xB04BD0,dwords(0x7EB6D4,m.alloc(64),16,1,0,10))
 def activate(site):
  # Exact activation timer/level-step/scalar prefix; stop before map/light
  # allocation. Both original light branches subsequently call65B9C0.
  u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP)
  u.reg_write(UC_X86_REG_ECX,site);run_checked(u,0x65B580,0x65B74C)
  m.invoke(0x65B9C0,site)
 def sample(cell):
  p=cells[tuple(cell)];bits=struct.unpack('<Q',u.mem_read(p+0xF0,8))[0]
  amount=struct.unpack('<i',dwords(m.invoke(0x487CB0,p)))[0]
  base=0
  if amount>0:
   u.reg_write(UC_X86_REG_ESP,SP);u.mem_write(SP+0x10,dwords(amount))
   run_checked(u,0x4DA5FA,0x4DA60F);base=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]
  return dict(cell=cell,level_bits=f'{bits:016x}',damaging_level=amount,base_damage=base)
 for action in field['actions']:
  kind=action['kind']
  if kind=='create':
   center=tuple(action['cell']);site=m.alloc(0x100);sites[center]=site
   m.invoke(0x65B1E0,site)
   u.mem_write(site+0x40,struct.pack('<hh',*center))
   m.invoke(0x65B4D0,site,(spread,));m.invoke(0x65B4F0,site,(level,));activate(site)
   m.invoke(0x487C70,cells[center],(site,))
  elif kind=='tick_to':
   # Run each original lifetime/level-timer prefix. The independent light
   # timer and final deletion suffix are outside this numeric comparison.
   for frame in range(frame+1,action['frame']+1):
    u.mem_write(0xA8ED84,dwords(frame))
    # Logic55B5CD..55B5E8 visits the native vector in reverse.
    vector=m.read32(0xB04BD4);count=m.read32(0xB04BE0)
    for index in range(count-1,-1,-1):
     site=m.read32(vector+index*4)
     u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP)
     u.reg_write(UC_X86_REG_ECX,site);run_checked(u,0x65B800,0x65B858)
  elif kind in ('decrease','decay'):
   m.invoke(0x65BB50 if kind=='decrease' else 0x65BD00,sites[tuple(action['cell'])])
  elif kind=='merge':
   site=sites[tuple(action['cell'])]
   u.mem_write(SP,dwords(RET_MAGIC,level));u.reg_write(UC_X86_REG_ESP,SP)
   u.reg_write(UC_X86_REG_ECX,site);run_checked(u,0x65B530,0x65B570)
   activate(site)
  else:raise ValueError(kind)
  native_order=[m.read32(m.read32(0xB04BD4)+index*4) for index in range(m.read32(0xB04BE0)-1,-1,-1)]
  stages.append(dict(action=action,frame=frame,update_order=[list(next(cell for cell,p in sites.items() if p==site)) for site in native_order],
   samples=[sample(cell) for cell in field['sample_cells']],
   sites=[dict(cell=list(cell),radius=m.read32(p+0x48),level=m.read32(p+0x4C),
     current_level=struct.unpack('<i',dwords(m.invoke(0x65B510,p)))[0],
     level_steps=m.read32(p+0x50),duration=m.read32(p+0x6C),remaining=m.read32(p+0x70),
     level_timer_start=struct.unpack('<i',u.mem_read(p+0x28,4))[0],level_timer_duration=m.read32(p+0x30)) for cell,p in sites.items()]))
 return stages,dict(rad_level=level,spread=spread,
  duration_multiple=m.read32(TYPE+0x1804),application_delay=m.read32(TYPE+0x1808),
  level_max=m.read32(TYPE+0x180C),level_delay=m.read32(TYPE+0x1810),
  factor_bits=f'{struct.unpack("<Q",u.mem_read(TYPE+0x1818,8))[0]:016x}')

def concrete_dogs(packets):
 """Use the existing populated Infantry/House/Walk/Map owner, not zeroes."""
 from tools.spatial_oracle.anytown_damage.foot_missions import FootMissions,native_owner
 from tools.projectile_oracle.bridge_render_inputs import lexical
 from tools.spatial_oracle.building_body_rules import RULES
 q=FootMissions();q.initialize_companion();m,u=q.m,q.u
 root=Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
 art_raw=(root/'ARTMD.INI').read_bytes()
 art,_=lexical(art_raw,{'DOG','DogSequence'});m.make_ini(art)
 typ=m.alloc(0x1900);m.invoke(0x5236A0,typ,(m.cstring('DOG'),))
 layers=[]
 for name,path in native_owner.base.layers():
  if not path.exists():layers.append(dict(file=name,absent=True));continue
  raw=path.read_bytes();selected,_=lexical(raw,{'DOG','DogJaw','RadSite'})
  m.rules_cache(selected);admitted=m.invoke(0x5240A0,typ,(RULES,))&255
  wh=m.invoke(0x75E3B0,m.cstring('RadSite'));m.invoke(0x75D3A0,wh,(RULES,))
  layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),admitted=admitted))
 rows=[]
 for index,(name,base_damage) in enumerate(packets):
  q.phase='setup'
  actor=m.alloc(0x1000);m.invoke(0x517A50,actor,(typ,0))
  u.mem_write(actor+0x21C,dwords(q.house));u.mem_write(actor+0x14C,dwords(q.house))
  m.invoke(0x486840,q.resident.ptrs[(86,49)],(q.coord,))
  assert m.invoke(0x51DFF0,actor,(q.coord,0x80))&255
  m.invoke(0x51D6F0,actor,(0,0,0));q.phase='logic'
  damage=m.alloc(4);u.mem_write(damage,dwords(base_damage));seen=[]
  def observe(uc,a,size,data):
   if a in (0x517FA0,0x4D7330,0x701900,0x5F5390,0x489180,0x65C780,0x65C7E0):seen.append(a)
  hook=u.hook_add(UC_HOOK_CODE,observe)
  before={key:bytes(u.mem_read(ptr,0x3F4)) for key,ptr in q.resident.rngs.items()}
  result=m.invoke(0x517FA0,actor,(damage,0,wh,0,0,1,0));u.hook_del(hook)
  assert not {0x65C780,0x65C7E0}&set(seen)
  rows.append(dict(name=name,base_damage=base_damage,result=result,health=m.read32(actor+0x6C),
   damage_after=m.read32(damage),alive=bool(u.mem_read(actor+0x90,1)[0]),
   limbo=bool(u.mem_read(actor+0x81,1)[0]),strength=m.read32(typ+0xA0),armor=m.read32(typ+0x9C),
   calls=[f'{a:08X}' for a in seen],rng_unchanged=all(before[key]==bytes(u.mem_read(ptr,0x3F4)) for key,ptr in q.resident.rngs.items()),
   art_sha256=hashlib.sha256(art_raw).hexdigest(),layers=layers))
 return rows

def generate_overlap():
 sections,layers=overlap_inputs();a,b,c=[17,24],[31,24],[24,17]
 create=lambda cell:dict(kind='create',cell=cell)
 cases=[('one_site',[create(a)]),('two_sites',[create(a),create(b)]),
        ('three_sites',[create(a),create(b),create(c)]),
        ('three_sites_reversed',[create(c),create(b),create(a)]),
        ('same_center_merge',[create(a),dict(kind='merge',cell=a)]),
        ('large_level_merge',[create(a),dict(kind='merge',cell=a)]),
        ('first_decay',[create(a),dict(kind='tick_to',frame=90)]),
        ('merge_after_first_decay',[create(a),dict(kind='tick_to',frame=90),dict(kind='merge',cell=a)]),
        ('overlap_first_decay',[create(a),create(b),create(c),dict(kind='tick_to',frame=90)]),
        ('overlap_reversed_first_decay',[create(c),create(b),create(a),dict(kind='tick_to',frame=90)]),
        ('zero_steps_decay',[create(a),dict(kind='decay',cell=a)]),
        ('zero_steps_decrease',[create(a),dict(kind='decrease',cell=a)])]
 samples=[[24,24],a,[18,24],[24,31],[27,24],[27,34],[22,29]]
 rows=[]
 for name,actions in cases:
  selected={key:dict(value) for key,value in sections.items()}
  if name.startswith('zero_steps_'):selected['Radiation']['RadLevelDelay']='600'
  if name=='large_level_merge':selected['RadEruptionWeapon']['RadLevel']='50000'
  rows.append(execute(cell=(24,24),armor=0,category='infantry',field=dict(name=name,sections=selected,
       actions=actions,sample_cells=samples)))
 return dict(schema_version=1,layers=layers,rows=rows,
             concrete_receivers=concrete_dogs([(row['name'],row['base_damage']) for row in rows[:3]]))

def overlap_metadata():
 return provenance(scope='Stock DESO overlapping radiation field arithmetic, merge/first decay, Foot packet, and populated DOG concrete receiver',
  assumptions=[
   'Selected lexical keys are read from physical RULESMD, optional LANGRULE, Battle and Hills files; native scalar readers execute. No whole physical native INI/scenario load.',
   'Original RadSite constructors append to supplied empty vector storage; the Logic55B5CD reverse-vector traversal selects numeric update order. Original48x48 flat CellClass table. Source RadLevel and CellSpread are native-read. Twelve small histories sample seven cells each; zero-step direct-kernel controls explicitly override RadLevelDelay600 and do not claim site lifetime reachability; no Python field arithmetic supplies results.',
   'Activation65B580..65B74C initializes real timers/steps then original spread65B9C0 executes. Independent LightSource creation/output is excluded. Tick prefix65B800..65B858 executes every frame through90; light-timer and lifetime-deletion suffix excluded.',
   'The large-level control explicitly overrides RadLevel50000. Per-site current_level executes signed getter65B510; it is distinct from same-center merge65B530..65B570, which executes its float effective-level and Decrease65BB50 before activation; original Decay65BD00 and CellSubtract487D00 execute. Memory-store boundaries are retained.',
   'Field rows use the historical original Foot4DA554 boundary, neutral300HP Object receiver control. The separate populated DOG control uses existing FootMissions native type/actor/Walk/House/map construction and full517FA0 receiver with the captured three-site packet.',
   'DOG concrete control uses physical Battle/Anytown reader context; selected DOG/RadSite values match the field Battle/Hills context. Independent source-layer identities are retained. No three-DESO live native match or rendering claim.'
  ],substitutions=[
   'Existing Landing allocator/delete/CRT TLS and cached lexical INI transport. Supplied flat map cell table and bounded object state for field rows.',
   'Populated DOG control inherits FootMissions OS/COM, map crop, one human House and sound transport boundaries. Original class/receiver code executes. No unknown receiver state is replaced with zeroes.'
  ],entry_points={'radiation_reader':0x66CF70,'spread':0x65B9C0,'merge':0x65B530,'decrease':0x65BB50,
    'decay':0x65BD00,'cell_add':0x487CE0,'cell_subtract':0x487D00,'foot':0x4DA554,
    'factor':0x4DA5FA,'infantry_receiver':0x517FA0,'foot_receiver':0x4D7330,'techno_receiver':0x701900,'object_receiver':0x5F5390})


def light_lifecycle_source_paths():
    """UTF-8 run guards; physical/executable bytes are guarded separately."""
    names = (
        'native_oracle.py', 'oracle_sources.py',
        'spatial_oracle/radiation_damage_boundary.py', 'spatial_oracle/light_publication.py',
        'spatial_oracle/tmp_clear_render.py', 'spatial_oracle/bridge_shadow_render.py',
        'spatial_oracle/bridge_constructor.py', 'spatial_oracle/bridge_recalc_presentation.py',
        'spatial_oracle/terrain_recalc.py', 'spatial_oracle/building_body_rules.py',
        'spatial_oracle/map_queries.py', 'spatial_oracle/estimated_damage.py',
        'spatial_oracle/engineer_repair_admission.py', 'spatial_oracle/building_construction.py',
        'spatial_oracle/refinery_dock.py', 'spatial_oracle/track_destination.py',
        'spatial_oracle/unit_entry.py', 'spatial_oracle/unit_scatter_state.py',
        'spatial_oracle/unit_source_scatter.py', 'spatial_oracle/anim_bouncer_launch.py',
        'spatial_oracle/building_slot_replacement.py',
        'spatial_oracle/anytown_damage/navigation_inputs.py',
        'spatial_oracle/shrapnel_repair/map_facts.py',
        'rules_oracle/bridge_landing_inputs.py', 'rules_oracle/bridge_anim_lists.py',
        'rules_oracle/bridge_anim_inputs.py', 'rules_oracle/bridge_child_sound.py',
        'rules_oracle/theater_general_reader.py', 'projectile_oracle/flat_art.py',
        'projectile_oracle/bridge_render_inputs.py',
        'projectile_oracle/bridge_render_inputs_palette.py',
        'projectile_oracle/bridge_render_shape.py', 'projectile_oracle/bridge_render.py',
        'spatial_oracle/bridge_damage_admission.py', 'palette_oracle/oracle.py',
        'palette_oracle/check.py', 'sidebar_oracle/stock.py', 'native_slope.py',
        'bridge_click_oracle.py', 'rmg_oracle/gen_rng_vectors.py',
        'spatial_oracle/anytown_damage/inputs.py',
        'spatial_oracle/anytown_damage/next_family_native.py',
        'spatial_oracle/anytown_damage/publication.py',
        'spatial_oracle/bridge_rim.py', 'spatial_oracle/building_art_transition.py',
        'spatial_oracle/cliff_render.py', 'spatial_oracle/harvest_field.py',
        'spatial_oracle/mapgen_range.py', 'spatial_oracle/native_id_snapshot.py',
        'spatial_oracle/object_health.py', 'spatial_oracle/placement_render.py',
        'spatial_oracle/shrapnel_repair/packet_io.py',
        'spatial_oracle/shrapnel_repair/production_inputs.py',
        'spatial_oracle/shrapnel_repair/retail_inputs.py',
        'spatial_oracle/shrapnel_repair/shrapnel_repair.py',
        'spatial_oracle/slave_manager.py', 'spatial_oracle/smudge_can_place.py',
        'spatial_oracle/smudge_render.py', 'storage_oracle/shell_relayout.py',
    )
    root = Path(__file__).resolve().parents[1]
    return {name: root / name for name in names}


def _light_binary_identity(paths):
    result = {}
    for label, path in sorted(paths.items()):
        digest = hashlib.sha256()
        with path.open('rb') as source:
            for block in iter(lambda: source.read(1024 * 1024), b''):
                digest.update(block)
        result[label] = dict(bytes=path.stat().st_size, sha256=digest.hexdigest())
    return result


class LightLifecycle:
    """Full native single-site light transaction in the existing raster VM.

    Geometry and sparse registration are explicit physical component inputs.
    No Python radiation/light/Cell color formula or supplied lifecycle return.
    """

    MAP, DUMMY = 0x87F7E8, 0xABDC50
    SAMPLE_OFFSETS = ((0, 0), (1, 0), (0, 1), (5, 0), (10, 0), (11, 0))

    def __init__(self, center, *, snow=False):
        from tools.spatial_oracle import bridge_shadow_render as render
        from tools.spatial_oracle import tmp_clear_render as tile
        from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
        from tools.spatial_oracle.anytown_damage.navigation_inputs import Inputs
        from tools.spatial_oracle.bridge_constructor import OriginalBridgeConstructor
        from tools.native_oracle import initialize_empty_windows_seh

        self.render, self.tile, self.center = render, tile, tuple(center)
        self.snow = snow
        self.map_name = render.RADIATION_SNOW_SOURCES['map'] if snow else 'XMP03T4.MAP'
        binary_before = self.binary_sources()
        self.m, _types, self.inputs = render.prepare(radiation=True, snow=snow)
        m, u = self.m, self.m.u
        self.u, self.phase, self.frame = u, 'setup', 0
        self.events, self.publication_cells, self.redraws, self.platform = [], [], [], []
        self.ai_visits, self.gather_visits, self.recalc_visits = 0, 0, 0
        self.site = self.light = 0
        self.activation_colors = []
        self.inputs['central_runtime'] = 'existing bridge_shadow_render.prepare(radiation=True)/PaletteReader'
        if snow:
            self.inputs['central_runtime'] = 'existing bridge_shadow_render.prepare(radiation=True,snow=True)/PaletteReader'
        self.inputs['palette_assertion_boundary'] = (
            'initialize_cell_convert retains its historical isolated palette_table validation VM; '
            'the existing lexical index transport also retains isolated native CRC probes. '
            'All RadSite/LightSource/Cell/TMP lifecycle instructions execute in this central VM.')
        parsed, physical = decode_cells(m.assets[self.map_name.upper()])
        assert self.center in physical, self.center
        # Physical provenance is distinct from this deliberately sparse map.
        # All native area/field loops still run; original5657E0/5657A0 own
        # allocation admission and the shared stamped Dummy on missing slots.
        physical_crop = {xy: value for xy, value in physical.items()
                         if abs(xy[0] - center[0]) <= 14 and abs(xy[1] - center[1]) <= 14}
        sample_cells = [(center[0] + dx, center[1] + dy) for dx, dy in self.SAMPLE_OFFSETS]
        assert all(xy in physical_crop for xy in sample_cells), sample_cells
        self.physical = {xy: physical_crop[xy] for xy in sample_cells}
        assert physical[self.center]['overlay'] is None, 'selected TMP witness requires an empty physical Cell'
        selected = [(f'cell_{x}_{y}', dict(cell=[x, y], **value))
                    for (x, y), value in sorted(self.physical.items(), key=lambda row: (row[0][1], row[0][0]))]
        self.assets = m.assets
        theater = self.inputs['theater'] if snow else Inputs.read_map_theater(m, self.assets[self.map_name.upper()])
        assert theater['source_value'] == ('SNOW' if snow else 'TEMPERATE') and theater['value'] == int(snow), theater
        self.inputs['theater'] = theater
        (self.tiles, mapping, mappings, filenames, _filename, self.heads,
         file_io, self.lighting) = render.prepare_selected_tiles(m, self.assets, self.inputs, selected,
                                                               variant_tile=physical[self.center]['tile'], snow=snow)
        self.inputs['tmp'] = dict(mappings=mappings, names=list(filenames), file_io=file_io)
        self.variant_names = self.inputs['tmp_families'][self.tiles[physical[self.center]['tile']]]['names']
        assert 1 < len(self.variant_names) <= (8 if snow else 4), 'unsupported selected component family'
        if snow: assert len(self.variant_names) == 8, 'Snow component requires the complete physical Clear01 family'
        self.variant_by_head = {self.heads[name]: (index, name) for index, name in enumerate(self.variant_names)}
        self.inputs['lighting'] = self.lighting
        self.scenario = m.read32(0xA8B230)
        self.rules = m.alloc(0x2000)
        u.mem_write(0x8871E0, dwords(self.rules))
        m.invoke(0x665650, self.rules)
        self.source_inputs = self.read_layers()
        initialize_empty_windows_seh(u)
        OriginalBridgeConstructor.empty_registries(u, m.alloc, preserve=(0xB0F670,))
        # Execute original static vector stores, stopping before CRT atexit.
        self.inputs['registry_startup'] = []
        for begin, stop in ((0x554640, 0x554676), (0x65B160, 0x65B196), (0x40CB80, 0x40CBB6)):
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, begin, stop, count=1000)
            self.inputs['registry_startup'].append([f'{begin:08X}', f'{stop:08X}'])
        self.inputs['cell_initializers'] = []
        for entry in struct.unpack('<13I', u.mem_read(0x8129FC, 13 * 4)):
            m.invoke(entry, 0)
            self.inputs['cell_initializers'].append(f'{entry:08X}')
        self.cells, self.cell_inputs = {}, []
        table = m.alloc(0x100000)
        u.mem_write(self.MAP + 0x13C, dwords(table, 0x40000))
        u.mem_write(0x87F924, dwords(table))
        cursor_before = m.read32(self.scenario + 0x214)
        for xy, authored in sorted(self.physical.items(), key=lambda row: (row[0][1], row[0][0])):
            pointer, pair = m.alloc(0x148), m.alloc(4)
            m.invoke(0x47BBF0, pointer)
            u.mem_write(pair, struct.pack('<hh', *xy))
            m.invoke(0x485240, pointer, (pair,))
            u.mem_write(pointer + 0x38, dwords(authored['tile']))
            u.mem_write(pointer + 0x11A, bytes((authored['subtile'], authored['level'])))
            assert authored['tile'] != 0xFFFF, 'selected sample requires a physical TMP material'
            head = self.heads[self.tiles[authored['tile']]]
            slope = m.invoke(0x5471B0, head, (authored['subtile'],)) & 255
            land = m.invoke(0x544BE0, head, (authored['subtile'],))
            if xy == self.center:
                self.inputs['tmp']['variant_metadata'] = []
                for index, name in enumerate(self.variant_names):
                    variant = self.heads[name]
                    variant_slope = m.invoke(0x5471B0, variant, (authored['subtile'],)) & 255
                    variant_land = m.invoke(0x544BE0, variant, (authored['subtile'],))
                    image = m.read32(variant + 0xA4)
                    assert bytes(u.mem_read(image, 16)) == self.assets[name.upper()][:16]
                    assert (variant_slope, variant_land) == (slope, land)
                    assert m.read32(variant + 0x2F0) == len(self.variant_names) - index
                    assert m.read32(variant + 0x2BC) == (self.heads[self.variant_names[index + 1]]
                                                       if index + 1 < len(self.variant_names) else 0)
                    self.inputs['tmp']['variant_metadata'].append(dict(file_index=index, filename=name,
                        descriptor=variant, next=m.read32(variant + 0x2BC), count=m.read32(variant + 0x2F0),
                        subtile=authored['subtile'], slope=variant_slope, land=variant_land,
                        header=bytes(u.mem_read(image, 16)).hex()))
            u.mem_write(pointer + 0x11C, bytes((slope,)))
            u.mem_write(pointer + 0xEC, dwords(land))
            assert m.read32(pointer + 0x38) == authored['tile']
            u.mem_write(table + (xy[1] * 512 + xy[0]) * 4, dwords(pointer))
            self.cells[xy] = pointer
            self.cell_inputs.append(dict(cell=list(xy), **authored, slope=slope, land=land,
                                         flags=m.read32(pointer + 0x140)))
        m.invoke(0x47BBF0, self.DUMMY)
        self.inputs['geometry'] = dict(map=self.map_name, selected_cell=list(self.center),
            map_fields=parsed['Map'], allocated=self.cell_inputs,
            physical_crop=[dict(cell=list(xy), **authored) for xy, authored in
                           sorted(physical_crop.items(), key=lambda row: (row[0][1], row[0][0]))],
            cell_cursor_before=cursor_before, cell_cursor_after=m.read32(self.scenario + 0x214),
            boundary='Six allocated physical sample Cells use original Cell/Dummy constructors/coordinate/TMP queries. physical_crop is provenance only, not allocated native map state. Original full area/field walks, allocation admission and shared stamped Dummy execute; omitted field targets alias Dummy and its actual state is recorded. No full physical-radius publication/cache/ID equivalence, MapResize/Recalc/actors or authored Overlay construction. No supplied map return.')
        self.coord = m.alloc(12)
        m.invoke(0x486840, self.cells[self.center], (self.coord,))
        self.inputs['source_xyz'] = list(struct.unpack('<3i', u.mem_read(self.coord, 12)))
        # Actual final FullInit store enables immediate area publication.
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x687B00, 0x687B07, count=10)
        u.mem_write(0xA8EB78, dwords(2))
        self.inputs['publication_prior'] = dict(enabled=u.mem_read(0x829AE4, 1)[0], detail=2,
            producer='00687B00..00687B07', mode=0, queued=m.read32(0xABCA50))
        from tools.spatial_oracle.bridge_recalc_presentation import SELECTOR_FLAG, LOAD_MAIN
        m.invoke(0x65C6D0, LOAD_MAIN, (0,))
        u.mem_write(SELECTOR_FLAG, b'\0')
        self.inputs['selector'] = dict(seed=0, selected=m.invoke(0x4814F0, self.cells[self.center], (0, 1)))
        from tools.spatial_oracle.bridge_recalc_presentation import SELECTOR_TABLE
        self.selector_fixed = render.ints(u, 0x81CCA8, 16)
        self.selector_generated = render.ints(u, SELECTOR_TABLE, 64)
        self.inputs['selector']['coverage'] = dict(fixed_address='0081CCA8', fixed_table=self.selector_fixed,
            generated_address=f'{SELECTOR_TABLE:08X}', generated_table=self.selector_generated,
            boundary='Native4814F0 uses fixed16 for count<=4; per-phase rotations are explicit component selector inputs. Both tables/flag/live RNG are restored. No whole-world Main/table bootstrap claim.')
        if snow:
            self.inputs['selector']['coverage']['boundary'] = (
                'Native4814F0 count>4 uses its original Main-generated64 table; '
                'per-phase rotations are explicit component selector inputs. Both tables/flag/live RNG '
                'are restored. No whole-world Main/table bootstrap claim.')
            assert self.signed(self.scenario + 0x1258) == 1
        self.rngs = dict(main=0x886B88, scenario=self.scenario + 0x218, mapgen=0xABE890)
        self.inputs['setup_rng'] = self.rng()
        for pointer in self.rngs.values():
            m.invoke(0x65C6D0, pointer, (0,))
        self.rng_entry = self.rng()
        self.install_observers()
        assert self.binary_sources() == binary_before, 'native binary/material setup drift'
        self.inputs['binary_sources'] = binary_before
        self.inputs['asset_cache'] = {name: dict(bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())
                                      for name, raw in sorted(m.assets.items())}

    def signed(self, pointer):
        return int.from_bytes(self.u.mem_read(pointer, 4), 'little', signed=True)

    def read_layers(self):
        """Native ctor/current defaults and individual physical reader passes."""
        from tools.projectile_oracle.bridge_render_inputs import lexical
        m, u = self.m, self.u
        name, weapon = m.cstring('RadEruptionWeapon'), m.alloc(0x200)
        u.mem_write(0x887568, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
        m.invoke(0x771C70, weapon, (name,))
        eruption = Landing.warhead(m, 'RadEruptionWarhead')
        radsite = Landing.warhead(m, 'RadSite')
        defaults = dict(rad_level=self.signed(weapon + 0x158), radiation=self.rule_fields())
        layers = []
        selected_names = {'Radiation', 'RadSite', 'RadEruptionWeapon', 'RadEruptionWarhead'}
        from tools.native_oracle import configured_gamemd
        root = configured_gamemd().parent
        lang = root / 'LANGRULE.INI'
        inputs = [('RULESMD.INI', self.assets['RULESMD.INI']),
                  ('LANGRULE.INI', lang.read_bytes() if lang.exists() else None),
                  ('MPBattleMD.ini', self.assets['MPBATTLEMD.INI']),
                  (self.map_name, self.assets[self.map_name.upper()])]
        for label, raw in inputs:
            if raw is None:
                layers.append(dict(file=label, absent=True))
                continue
            sections, lines = lexical(raw, selected_names)
            m.make_ini(sections)
            before = dict(rad_level=self.signed(weapon + 0x158), radiation=self.rule_fields())
            admitted = m.invoke(0x66CF70, self.rules, (INI,)) & 255
            weapon_admitted = m.invoke(0x526810, INI, (name,))
            if weapon_admitted:
                # Exact current-default RadLevel read/store, including the
                # neighboring unchanged IsRadEruption byte's retained store.
                for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, weapon),
                    (UC_X86_REG_EDI, INI), (UC_X86_REG_EBX, name),
                    (UC_X86_REG_EAX, u.mem_read(weapon + 0x155, 1)[0])):
                    u.reg_write(register, value)
                run_checked(u, 0x7728CD, 0x7728FC, count=2000000,
                            required_addresses=(0x5276D0, 0x7728F6))
            for pointer in (eruption, radsite):
                m.invoke(0x75D3A0, pointer, (INI,))
            layers.append(dict(file=label, sha256=hashlib.sha256(raw).hexdigest(), source_lines=lines,
                radiation_admitted=admitted, weapon_admitted=bool(weapon_admitted), before=before,
                after=dict(rad_level=self.signed(weapon + 0x158), radiation=self.rule_fields()),
                eruption=Landing.whstate(m, eruption), radsite=Landing.whstate(m, radsite)))
        spread = Landing.whstate(m, eruption)['cell_spread']
        assert 0 < self.signed(weapon + 0x158) <= 1000 and 0 < spread <= 10
        return dict(defaults=defaults, layers=layers, weapon='RadEruptionWeapon',
            rad_level=self.signed(weapon + 0x158), spread_float_bits=Landing.whstate(m, eruption)['cell_spread_bits'],
            warhead='RadEruptionWarhead', eruption_pointer=eruption,
            radiation=self.rule_fields(), boundary='Full native Rules/Weapon/Warhead constructors, selected current-default RadLevel block and full Radiation/Warhead per-layer readers; no full Weapon reader, DESO shot or paid Factory claim.')

    def rule_fields(self):
        p = self.rules
        return dict(duration_multiple=self.signed(p + 0x1804), application_delay=self.signed(p + 0x1808),
            level_max=self.signed(p + 0x180C), level_delay=self.signed(p + 0x1810), light_delay=self.signed(p + 0x1814),
            factor_bits=f'{int.from_bytes(self.u.mem_read(p + 0x1818, 8), "little"):016x}',
            light_factor_bits=f'{int.from_bytes(self.u.mem_read(p + 0x1820, 8), "little"):016x}',
            tint_factor_bits=f'{int.from_bytes(self.u.mem_read(p + 0x1828, 8), "little"):016x}',
            color=list(self.u.mem_read(p + 0x1830, 3)))

    def rng(self):
        return {name: bytes(self.u.mem_read(pointer, 0x3F4)).hex() for name, pointer in self.rngs.items()}

    def binary_sources(self):
        from tools.native_oracle import configured_gamemd
        from tools.projectile_oracle.bridge_render_inputs import assets_root
        root = configured_gamemd().parent
        paths = {'gamemd.exe': configured_gamemd()}
        paths.update({f'retail/{name}': root / name for name in
                      ('ra2.mix', 'ra2md.mix', 'multimd.mix', 'expandmd01.mix')})
        # Reader loads this existing extraction eagerly, before physical
        # selected resources replace its cache entries. Guard that readset too.
        paths.update({f'initial_palette_cache/{p.name}': p for p in assets_root().glob('*')})
        lang = root / 'LANGRULE.INI'
        if lang.exists(): paths['retail/LANGRULE.INI'] = lang
        return _light_binary_identity(paths)

    def install_observers(self):
        from tools.native_oracle import NativeCallTrace, checked_is_bad_read_ptr_transport
        from tools.spatial_oracle.light_publication import publication_observer
        m, u = self.m, self.u
        self.trace = NativeCallTrace(u, m.read32, self.events)
        self.return_pcs = set()
        self.requests, self.raw_draws, self.frees = [], [], []
        self.dummy_misses, self.dummy_field_counts, self.dummy_field_last = 0, {}, {}
        specs = {
            0x65B1E0: ('radsite_ctor', 0, 0), 0x65B4C0: ('set_center', 1, 4),
            0x65B4D0: ('set_spread', 1, 4), 0x65B4F0: ('set_level', 1, 4),
            0x65B580: ('activate', 0, 0), 0x65B530: ('merge', 1, 4),
            0x65B2F0: ('site_destructor', 0, 0), 0x65BED0: ('site_delete', 1, 4),
            0x554760: ('light_ctor', 8, 32), 0x554A60: ('light_enable', 1, 4),
            0x554AA0: ('light_update', 5, 20), 0x554AF0: ('light_area', 1, 4),
            0x555150: ('light_delete', 1, 4), 0x7258D0: ('pointer_expiry', 0, 0),
            0x487C70: ('set_cell_site', 1, 4), 0x65BD00: ('decay', 0, 0),
            0x65BB50: ('decrease', 0, 0), 0x486840: ('cell_coords', 1, 4),
        }
        watched_pcs = set(specs) | {0x7CAA5E, 0x65B800, 0x484180, 0x483E30,
            0x65C780, 0x65C7E0, 0x65C79D, 0x65C84B, 0x65BEE0, 0x5551E2,
            0x5657CF, 0x487CE0, 0x487D00}
        if self.snow: watched_pcs.add(0x65B604)
        def observe(uc, pc, _size, _data):
            # The palette hot loop needs no source observation. Retain every
            # watched entry and every actual pending return PC before reading
            # ESP; NativeCallTrace still owns matching the nested cleaned ESP.
            if pc not in watched_pcs and pc not in self.return_pcs: return
            sp = uc.reg_read(UC_X86_REG_ESP)
            if checked_is_bad_read_ptr_transport(u, pc, sp, self.platform): return
            if self.phase != 'live': return
            if self.snow and pc == 0x65B604:
                # Observe actual working channels AFTER the original SNOW1
                # branch, BEFORE signed integer normalization/ftol. No host
                # tint calculation or supplied native return is involved.
                self.activation_colors.append(dict(frame=self.frame, pc='0065B604',
                    theater=self.signed(self.scenario + 0x1258),
                    red_bits=f'{uc.reg_read(UC_X86_REG_EAX):08X}',
                    green_bits=f'{uc.reg_read(UC_X86_REG_EBP):08X}',
                    blue_bits=f'{uc.reg_read(UC_X86_REG_EBX):08X}'))
            if self.trace.returned(pc, sp): self.refresh_return_pcs()
            if pc in specs:
                index = self.trace.entered(pc, sp, specs[pc])
                self.refresh_return_pcs()
                self.events[index].update(frame=self.frame)
                if pc == 0x65B1E0: self.site = uc.reg_read(UC_X86_REG_ECX)
                if pc == 0x554760: self.light = uc.reg_read(UC_X86_REG_ECX)
                if pc == 0x7258D0: self.events[index]['parameter_dl'] = uc.reg_read(UC_X86_REG_EDX) & 255
            if pc == 0x65B800: self.ai_visits += 1
            if pc == 0x484180: self.gather_visits += 1
            if pc == 0x483E30: self.recalc_visits += 1
            if pc == 0x5657CF: self.dummy_misses += 1
            if pc in (0x487CE0, 0x487D00) and uc.reg_read(UC_X86_REG_ECX) == self.DUMMY:
                operation = 'add' if pc == 0x487CE0 else 'subtract'
                self.dummy_field_counts[operation] = self.dummy_field_counts.get(operation, 0) + 1
                self.dummy_field_last[operation] = dict(frame=self.frame, receiver=self.DUMMY,
                    caller=f'{m.read32(sp):08X}', cell=list(struct.unpack('<2h', uc.mem_read(self.DUMMY + 0x24, 4))),
                    argument_bits=f'{int.from_bytes(uc.mem_read(sp + 4, 8), "little"):016x}')
            if pc in (0x65C780, 0x65C7E0):
                self.requests.append(dict(frame=self.frame, pc=f'{pc:08X}', caller=f'{m.read32(sp):08X}',
                    stream=next((name for name, pointer in self.rngs.items() if pointer == uc.reg_read(UC_X86_REG_ECX)), 'unknown'),
                    args=[self.signed(sp + 4), self.signed(sp + 8)] if pc == 0x65C7E0 else []))
            if pc in (0x65C79D, 0x65C84B):
                self.raw_draws.append(dict(frame=self.frame, pc=f'{pc:08X}', value=uc.reg_read(UC_X86_REG_ESI)))
            # Observe the original CALL before Lists' existing no-op delete
            # transport mutates ESP at7C8B3D. Both callsites follow PUSH ESI;
            # this records the actual native operand, without reconstructing
            # a return address from an already-returned allocator seam.
            if pc in (0x65BEE0, 0x5551E2):
                assert bytes(uc.mem_read(pc, 1)) == b'\xe8'
                target = pc + 5 + int.from_bytes(uc.mem_read(pc + 1, 4), 'little', signed=True)
                assert target == 0x7C8B3D
                self.frees.append(dict(frame=self.frame, pointer=m.read32(sp),
                    callsite=f'{pc:08X}', target=f'{target:08X}'))
        u.hook_add(UC_HOOK_CODE, observe)
        publication = publication_observer(u, m.read32, lambda p: p in self.cells.values(),
                                          self.publication_cells, self.redraws, substitute_cell=False)
        def publish(uc, pc, size, data):
            if self.phase == 'live': publication(uc, pc, size, data)
        for pc in (0x483E30, 0x4F42F0):
            u.hook_add(UC_HOOK_CODE, publish, begin=pc, end=pc)

    def refresh_return_pcs(self):
        # This index is derived only from the sole trace owner's pending keys,
        # after every entered/completed returned/manual stop mutation. It is
        # never a second return stack or a source of expected return addresses.
        self.return_pcs = {pc for pc, _sp in self.trace.pending}

    def invoke(self, entry, pointer, args=()):
        # These outer calls publish a whole area and may construct many
        # native27/53-shade Cell Convert tables. Keep the shared reader's
        # historical2M/10s default for every other invocation/caller.
        limits = (dict(instruction_limit=20_000_000, timeout_us=120_000_000)
                  if entry in (0x65B580, 0x65B530, 0x65B800) else {})
        if self.snow: assert self.signed(self.scenario + 0x1258) == 1
        result = self.m.invoke(entry, pointer, args, **limits)
        if self.snow: assert self.signed(self.scenario + 0x1258) == 1
        assert self.u.reg_read(UC_X86_REG_EIP) == RET_MAGIC
        self.trace.returned(RET_MAGIC, self.u.reg_read(UC_X86_REG_ESP))
        self.refresh_return_pcs()
        return result

    def registered(self, header):
        return [self.m.read32(self.m.read32(header + 4) + i * 4)
                for i in range(self.m.read32(header + 16))]

    def snapshot(self):
        m, u = self.m, self.u
        sites, lights = self.registered(0xB04BD0), self.registered(0xABCA10)
        assert sites in ([], [self.site]) and lights in ([], [self.light]), (sites, lights)
        samples = []
        for dx, dy in self.SAMPLE_OFFSETS:
            xy = (self.center[0] + dx, self.center[1] + dy)
            assert xy in self.cells, xy
            p = self.cells[xy]
            samples.append(dict(cell=list(xy), radiation_bits=f'{int.from_bytes(u.mem_read(p + 0xF0, 8), "little"):016x}',
                radsite=m.read32(p + 0xF8), scale=self.signed(p + 0x104),
                scalars=list(struct.unpack('<4h', u.mem_read(p + 0x108, 8))),
                rgb=list(struct.unpack('<3h', u.mem_read(p + 0x110, 6))),
                convert_rgb=self.render.ints(u, m.read32(p + 0x34) + 0x198, 3) if m.read32(p + 0x34) else None))
        site = None if not sites else dict(pointer=self.site, vtable=f'{m.read32(self.site):08X}',
            center=list(struct.unpack('<2h', u.mem_read(self.site + 0x40, 4))),
            light=m.read32(self.site + 0x24), radius=self.signed(self.site + 0x48),
            level=self.signed(self.site + 0x4C), level_steps=self.signed(self.site + 0x50),
            intensity=self.signed(self.site + 0x54), tint=self.render.ints(u, self.site + 0x58, 3),
            light_steps=self.signed(self.site + 0x64), light_decrement=self.signed(self.site + 0x68),
            duration=self.signed(self.site + 0x6C), remaining=self.signed(self.site + 0x70),
            level_timer=[self.signed(self.site + 0x28), self.signed(self.site + 0x30)],
            light_timer=[self.signed(self.site + 0x34), self.signed(self.site + 0x3C)])
        light = None if not lights else dict(pointer=self.light, vtable=f'{m.read32(self.light):08X}',
            intensity=self.signed(self.light + 0x24), tint=self.render.ints(u, self.light + 0x28, 3),
            detail=self.signed(self.light + 0x34), xyz=self.render.ints(u, self.light + 0x38, 3),
            radius=self.signed(self.light + 0x44), active=u.mem_read(self.light + 0x48, 1)[0])
        rng = self.rng()
        return dict(frame=self.frame, site=site, light=light, registered_sites=sites,
            registered_lights=lights, samples=samples, runtime_cursor=m.read32(self.scenario + 0x214),
            dummy=dict(pointer=self.DUMMY, vtable=f'{m.read32(self.DUMMY):08X}',
                cell=list(struct.unpack('<2h', u.mem_read(self.DUMMY + 0x24, 4))),
                radiation_bits=f'{int.from_bytes(u.mem_read(self.DUMMY + 0xF0, 8), "little"):016x}',
                radsite=m.read32(self.DUMMY + 0xF8), lookup_misses=self.dummy_misses,
                field_receiver_counts=dict(self.dummy_field_counts),
                last_field_calls=dict(self.dummy_field_last)),
            rng_same_as_entry=rng == self.rng_entry, **({} if rng == self.rng_entry else dict(rng=rng)))

    def raster(self, name):
        from tools.spatial_oracle.bridge_recalc_presentation import SELECTOR_FLAG, SELECTOR_TABLE
        self.phase = 'raster'
        before = self.rng()
        offsets = (0x352C, 0x3534, 0x3538, 0x353C, 0x3540, 0x3544)
        lighting_before = [bytes(self.u.mem_read(self.scenario + offset, 4)) for offset in offsets]
        fixed_before = bytes(self.u.mem_read(0x81CCA8, 64))
        generated_before = bytes(self.u.mem_read(SELECTOR_TABLE, 256))
        flag_before = bytes(self.u.mem_read(SELECTOR_FLAG, 1))
        assert fixed_before == dwords(*self.selector_fixed)
        assert generated_before == dwords(*self.selector_generated)
        rows, covered = [], set()
        try:
            generated = len(self.variant_names) > 4
            for shift in range(8 if generated else 4):
                # Original count<=4 uses the fixed16 table, not generated64.
                # Rotate its original0..3 values as declared selector inputs;
                # original4814F0 and547CF0 still select/follow the descriptor.
                if generated:
                    # Same original-generated64 value rotations as the
                    # existing Clear oracle; native4814F0 still selects.
                    rotated = [(value + shift) % 8 for value in self.selector_generated]
                    selector_input = dict(selector_table=rotated)
                else:
                    rotated = [(value + shift) % 4 for value in self.selector_fixed]
                    self.u.mem_write(0x81CCA8, dwords(*rotated))
                    selector_input = dict(fixed_selector_input=dict(address='0081CCA8', shift=shift, table=rotated))
                self.u.mem_write(SELECTOR_FLAG, b'\1')
                descriptors = []
                hook = self.u.hook_add(UC_HOOK_CODE, lambda uc, pc, size, data:
                    descriptors.append(uc.reg_read(UC_X86_REG_ECX)), begin=0x547F49, end=0x547F49)
                try:
                    row = self.tile.execute(self.m, dict(name=name, coords=list(self.center),
                        level=self.physical[self.center]['level'], cell_point=[96, 120], scene_lighting=self.lighting,
                        **selector_input),
                        current_cell=self.cells[self.center])
                finally:
                    self.u.hook_del(hook)
                assert len(descriptors) == 1 and descriptors[0] in self.variant_by_head, descriptors
                index, filename = self.variant_by_head[descriptors[0]]
                assert len(row['draws']) == 1
                row['phase'] = name
                row['material'] = dict(raw_file_index=row['output']['selected_file_index'],
                    file_index=index, filename=filename, descriptor=descriptors[0], observed_at='00547F49')
                row['name'] = f'{name}_variant_{index}'
                if index not in covered:
                    rows.append(row)
                    covered.add(index)
                if len(covered) == len(self.variant_names): break
            assert covered == set(range(len(self.variant_names))), covered
            rows.sort(key=lambda row: row['material']['file_index'])
        finally:
            self.u.mem_write(0x81CCA8, fixed_before)
            self.u.mem_write(SELECTOR_TABLE, generated_before)
            self.u.mem_write(SELECTOR_FLAG, flag_before)
        assert before == self.rng(), 'raster unexpectedly advances a live RNG stream'
        assert lighting_before == [bytes(self.u.mem_read(self.scenario + offset, 4)) for offset in offsets], 'raster changes the selected native Lighting profile'
        assert fixed_before == bytes(self.u.mem_read(0x81CCA8, 64))
        assert generated_before == bytes(self.u.mem_read(SELECTOR_TABLE, 256))
        assert flag_before == bytes(self.u.mem_read(SELECTOR_FLAG, 1))
        if self.snow: assert self.signed(self.scenario + 0x1258) == 1
        values = set(struct.unpack('<' + 'H' * self.render.COUNT,
                                   self.u.mem_read(self.render.A, self.render.COUNT * 2)))
        assert len(values) == 1, 'selected raster requires its declared uniform A prior'
        alpha = values.pop()
        for row in rows: row['surface_a_value'] = alpha
        self.phase = 'live'
        return rows

    def run(self, *, refresh_frame=None):
        self.phase = 'live'
        rasters = self.raster('baseline')
        baseline = self.snapshot()
        self.site = self.m.alloc(0x74)
        self.invoke(0x65B1E0, self.site)
        pair = self.m.alloc(4)
        self.u.mem_write(pair, struct.pack('<hh', *self.center))
        self.invoke(0x65B4C0, self.site, (pair,))
        # The selected caller's CellSpread->ftol conversion is original.
        self.u.mem_write(SP, dwords(RET_MAGIC))
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_EAX, self.source_inputs['eruption_pointer'])
        run_checked(self.u, 0x469180, 0x46918B, count=1000)
        spread = self.u.reg_read(UC_X86_REG_EAX)
        self.invoke(0x65B4D0, self.site, (spread,))
        self.invoke(0x65B4F0, self.site, (self.source_inputs['rad_level'],))
        assert 0 < self.signed(self.site + 0x48) < 14 * 256
        self.invoke(0x65B580, self.site)
        self.invoke(0x487C70, self.cells[self.center], (self.site,))
        activated = self.snapshot()
        assert activated['site'] and activated['light'] and activated['light']['active'] == 1
        assert activated['site']['vtable'] == '007F0810' and activated['light']['vtable'] == '007ED028'
        self.source_inputs['spread'] = spread
        rasters.extend(self.raster('activated'))
        stages, prior, first_fade = [dict(name='activated', state=activated)], activated, False
        for frame in range(1, 5001):
            self.frame = frame
            self.u.mem_write(0xA8ED84, dwords(frame))
            start, publication_start, redraw_start = len(self.events), len(self.publication_cells), len(self.redraws)
            if frame == refresh_frame:
                self.invoke(0x65B530, self.site, (self.source_inputs['rad_level'],))
            # Actual source-vector order, without caching a deleted receiver.
            for pointer in reversed(self.registered(0xB04BD0)):
                self.invoke(0x65B800, pointer)
            after = self.snapshot()
            changed = self.events[start:]
            if changed:
                if stages[-1]['state']['frame'] != prior['frame']:
                    stages.append(dict(name='before_native_change', state=prior))
                stages.append(dict(name='refresh' if frame == refresh_frame else 'native_change', state=after,
                    event_range=[start, len(self.events)],
                    publication_cells=self.publication_cells[publication_start:], redraws=self.redraws[redraw_start:]))
                if not first_fade and any(event['name'] == 'light_update' and event['caller'] == '0x65b8a9' for event in changed):
                    rasters.extend(self.raster('first_light_decay'))
                    first_fade = True
            prior = after
            if not after['registered_sites']:
                break
        else:
            raise AssertionError('native site failed to retire within the declared5000-frame bound')
        rasters.extend(self.raster('expired'))
        assert first_fade and not prior['registered_lights']
        assert all(sample['radsite'] == 0 for sample in prior['samples'])
        assert self.ai_visits == self.frame and self.gather_visits and self.recalc_visits
        assert any(event['name'] == 'site_destructor' for event in self.events)
        assert any(event['name'] == 'light_delete' for event in self.events)
        assert {event['ecx'] for event in self.events if event['name'] == 'pointer_expiry'} == {hex(self.site), hex(self.light)}
        assert [row['pointer'] for row in self.frees] == [self.light, self.site], self.frees
        assert not self.trace.pending, self.trace.pending
        assert all(row['output']['runs'] for row in rasters), 'selected TMP must really rasterize'
        for index in range(len(self.variant_names)):
            outputs = {row['phase']: row['output'] for row in rasters
                       if row['material']['file_index'] == index}
            assert outputs['baseline']['color_sha256'] != outputs['activated']['color_sha256'], 'missing positive light-to-pixels witness'
            for key in ('color_sha256', 'depth_sha256'):
                assert outputs['expired'][key] == outputs['baseline'][key], (index, key)
        assert not self.requests and not self.raw_draws and self.rng() == self.rng_entry
        # Keep full nested-return observation until native execution and its
        # pending-return checks have completed. Repeated field GetCoords calls
        # are not consumed by the small corpus; retain every other event and
        # map stage ranges onto their exact filtered slices, without changing
        # state, native inputs or any executed instructions.
        events, prefix = [], [0]
        for event in self.events:
            if event['name'] != 'cell_coords': events.append(event)
            prefix.append(len(events))
        for stage in stages:
            if 'event_range' not in stage: continue
            begin, end = stage['event_range']
            assert 0 <= begin <= end <= len(self.events)
            mapped = [prefix[begin], prefix[end]]
            assert events[mapped[0]:mapped[1]] == [event for event in self.events[begin:end]
                                                 if event['name'] != 'cell_coords']
            stage['event_range'] = mapped
        result = dict(input=dict(center=list(self.center), refresh_frame=refresh_frame, seed=0,
            source=self.source_inputs, setup=self.inputs), baseline=baseline, stages=stages,
            events=events, rasters=rasters, publication_cells=self.publication_cells,
            redraws=self.redraws, ai_visits=self.ai_visits, recalc_visits=self.recalc_visits,
            gather_visits=self.gather_visits, frees=self.frees, platform=self.platform,
            rng_before=self.rng_entry, rng_after=self.rng(), requests=self.requests, raw_draws=self.raw_draws,
            final=prior, deferred_count=self.m.read32(0xB0F6A8))
        if self.snow:
            assert len(self.activation_colors) == (2 if refresh_frame is not None else 1)
            assert all(row['theater'] == 1 for row in self.activation_colors)
            result['activation_colors'] = self.activation_colors
        return result

    def tint_integer_controls(self):
        """Original low32 IMUL/signed IDIV prefix, after actual source cleanup.

        Operand storage is an explicit numeric prior, not another RadSite or
        VM. No lifecycle call or host tint formula supplies any output.
        """
        assert not self.registered(0xB04BD0) and not self.registered(0xABCA10)
        assert not self.trace.pending
        m, u = self.m, self.u
        operands = m.alloc(0x74)
        required = (0x65B877, 0x65B87F, 0x65B881, 0x65B886,
                    0x65B88A, 0x65B88E, 0x65B890,
                    0x65B894, 0x65B898, 0x65B89D)
        cases = (
            ('normal', (17, 1000, 2000), 431, 500),
            ('low32_overflow', (0, 1000, 2000), 3_000_000, 3_000_000),
            ('signed', (-17, -1000, 2000), 431, 500),
        )
        rows = []
        for name, base, remaining, duration in cases:
            before = self.snapshot()
            before_rng = self.rng()
            before_source = (bytes(u.mem_read(self.site, 0x74)), bytes(u.mem_read(self.light, 0x4C)))
            before_counts = (len(self.events), len(self.publication_cells), len(self.redraws),
                             len(self.platform), len(self.frees), self.ai_visits,
                             self.gather_visits, self.recalc_visits,
                             len(self.requests), len(self.raw_draws), m.read32(0xB0F6A8))
            cpu, stack, phase = u.context_save(), bytes(u.mem_read(SP - 0x20, 0x40)), self.phase
            registers = (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
                         UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP, UC_X86_REG_EBP,
                         UC_X86_REG_EIP, UC_X86_REG_EFLAGS, UC_X86_REG_FPCW, UC_X86_REG_FPSW)
            before_registers = [u.reg_read(register) for register in registers]
            visited = []
            hook = u.hook_add(UC_HOOK_CODE, lambda uc, pc, size, data:
                visited.append(f'{pc:08X}') if pc in required else None,
                begin=0x65B871, end=0x65B89E)
            try:
                self.phase = 'integer_kernel'
                u.mem_write(operands, bytes(0x74))
                u.mem_write(operands + 0x24, dwords(0))
                u.mem_write(operands + 0x58, dwords(*base))
                u.mem_write(operands + 0x6C, dwords(duration, remaining))
                u.mem_write(SP, dwords(RET_MAGIC))
                u.reg_write(UC_X86_REG_ESP, SP)
                u.reg_write(UC_X86_REG_ESI, operands)
                u.reg_write(UC_X86_REG_EBX, 0x13579BDF)
                stop = run_checked(u, 0x65B871, 0x65B89E, count=200,
                    required_addresses=required,
                    context=dict(mode='radiation_tint_integer', case=name,
                                 base=list(base), remaining=remaining, duration=duration))
                sp = u.reg_read(UC_X86_REG_ESP)
                assert stop == 0x65B89E and sp == SP - 20
                raw = bytes(u.mem_read(sp, 20))
                words = list(struct.unpack('<5I', raw))
                assert words[3:] == [0, 0x13579BDF]
                assert words[0] == u.reg_read(UC_X86_REG_EAX)
                assert visited == [f'{pc:08X}' for pc in required]
                rows.append(dict(name=name, input=dict(base=list(base), remaining=remaining, duration=duration),
                    entry='0065B871', stop=f'{stop:08X}', visited=visited,
                    output=dict(tint=list(struct.unpack('<3i', raw[:12])),
                                stack_words=words, esp_delta=sp - SP)))
            finally:
                u.hook_del(hook)
                u.mem_write(SP - 0x20, stack)
                u.context_restore(cpu)
                self.phase = phase
            assert [u.reg_read(register) for register in registers] == before_registers
            assert bytes(u.mem_read(SP - 0x20, 0x40)) == stack
            assert self.snapshot() == before and self.rng() == before_rng
            assert (bytes(u.mem_read(self.site, 0x74)), bytes(u.mem_read(self.light, 0x4C))) == before_source
            assert (len(self.events), len(self.publication_cells), len(self.redraws),
                    len(self.platform), len(self.frees), self.ai_visits,
                    self.gather_visits, self.recalc_visits,
                    len(self.requests), len(self.raw_draws), m.read32(0xB0F6A8)) == before_counts
            assert not self.trace.pending
        return rows

    def light_update_controls(self):
        """Full554AA0 on declared inactive numeric storage, without a ctor.

        Active0 excludes area publication. The native comparison gate and
        parameter writes still execute; neither result is supplied by Python.
        """
        assert not self.registered(0xB04BD0) and not self.registered(0xABCA10)
        assert not self.trace.pending
        m, u = self.m, self.u
        operands = m.alloc(0x4C)
        rows = []
        for name, intensity, tint in (
            ('same_intensity_green_only', 50, (0, 1000, 0)),
            ('changed_intensity', 40, (0, 820, 0)),
        ):
            before = self.snapshot()
            before_rng = self.rng()
            before_source = (bytes(u.mem_read(self.site, 0x74)), bytes(u.mem_read(self.light, 0x4C)))
            before_counts = (len(self.events), len(self.publication_cells), len(self.redraws),
                             len(self.platform), len(self.frees), self.ai_visits,
                             self.gather_visits, self.recalc_visits,
                             len(self.requests), len(self.raw_draws), m.read32(0xB0F6A8))
            cpu, stack, phase = u.context_save(), bytes(u.mem_read(SP - 0x20, 0x40)), self.phase
            registers = (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
                         UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP, UC_X86_REG_EBP,
                         UC_X86_REG_EIP, UC_X86_REG_EFLAGS, UC_X86_REG_FPCW, UC_X86_REG_FPSW)
            before_registers = [u.reg_read(register) for register in registers]
            visited = []
            def observe(_uc, pc, _size, _data):
                assert pc != 0x554AF0, 'inactive numeric control unexpectedly entered LightArea'
                visited.append(f'{pc:08X}')
            hook = u.hook_add(UC_HOOK_CODE, observe, begin=0x554AA0, end=0x554AF0)
            try:
                self.phase = 'integer_kernel'
                u.mem_write(operands, bytes(0x4C))
                u.mem_write(operands + 0x24, dwords(50, 0, 800, 0))
                supplied = dict(intensity=50, tint=[0, 800, 0], active=0)
                raw_before = bytes(u.mem_read(operands, 0x4C))
                m.invoke(0x554AA0, operands, (intensity, *tint, 0), instruction_limit=200)
                assert u.reg_read(UC_X86_REG_EIP) == RET_MAGIC
                assert u.reg_read(UC_X86_REG_ESP) == SP + 24
                assert visited[0] == '00554AA0' and visited[-1] == '00554AE3'
                raw_after = bytes(u.mem_read(operands, 0x4C))
                assert raw_after[:0x24] == raw_before[:0x24]
                assert raw_after[0x34:] == raw_before[0x34:]
                values = struct.unpack('<4i', raw_after[0x24:0x34])
                rows.append(dict(name=name, input=dict(current=supplied,
                    proposed=dict(intensity=intensity, tint=list(tint), mode=0)),
                    entry='00554AA0', stop=f'{RET_MAGIC:08X}', visited=visited,
                    output=dict(intensity=values[0], tint=list(values[1:]), active=raw_after[0x48])))
            finally:
                u.hook_del(hook)
                u.mem_write(SP - 0x20, stack)
                u.context_restore(cpu)
                self.phase = phase
            assert [u.reg_read(register) for register in registers] == before_registers
            assert bytes(u.mem_read(SP - 0x20, 0x40)) == stack
            assert self.snapshot() == before and self.rng() == before_rng
            assert (bytes(u.mem_read(self.site, 0x74)), bytes(u.mem_read(self.light, 0x4C))) == before_source
            assert (len(self.events), len(self.publication_cells), len(self.redraws),
                    len(self.platform), len(self.frees), self.ai_visits,
                    self.gather_visits, self.recalc_visits,
                    len(self.requests), len(self.raw_draws), m.read32(0xB0F6A8)) == before_counts
            assert not self.trace.pending
        return rows


def generate_light_lifecycle(center=(33, 88), *, snow=False):
    from tools.native_oracle import image_sha256
    histories = []
    for name, refresh in (('single_lifetime', None), ('same_center_refresh', 45)):
        owner = LightLifecycle(center, snow=snow)
        before = owner.inputs['binary_sources']
        result = owner.run(refresh_frame=refresh)
        assert owner.binary_sources() == before, 'native binary/material input drift'
        histories.append(dict(name=name, **result))
    tint_controls = owner.tint_integer_controls()
    update_controls = owner.light_update_controls()
    assert owner.binary_sources() == before, 'native binary/material kernel input drift'
    alpha = {row['surface_a_value'] for history in histories for row in history['rasters']}
    assert len(alpha) == 1
    return dict(schema_version=1, native_sha256=image_sha256(), histories=histories,
                tint_integer_controls=tint_controls,
                light_update_controls=update_controls,
                surface=dict(width=192, height=160, bytes_per_pixel=2, baseline=32768,
                             a_value=alpha.pop(), a_uniform=True, background=65535, old_z=65535))


def light_lifecycle_metadata(*, snow=False):
    result = provenance(scope='Two original single-site TEMPERATE radiation light lifecycles through real Cell gathering and selected TMP pixels',
        assumptions=[
            'Each independent history uses the existing PaletteReader/TMP runtime, actual expansionRULESMD/fixedART/Battle/XMP and native per-layer Radiation/selected weapon/Warhead readers. Six allocated physical sample tuples/native Cell and Dummy constructors/ground getters precede original RadSite activation; full physical crop is separately labeled provenance only. No VERA values initialize source fields or expected pixels.',
            'Original65B580/65B800 execute completely, including both native timers, registered LightSource constructors/updates, actual Cell483E30/484180/Convert and real expiry/deleting virtual slots. Actual native source registry order drives each supplied elapsed-frame visit. Same-center refresh at declared45 is an independent component input, not normal DESO ROF or whole-world scheduling.',
            'Main/Scenario/MapGen are originally seeded0 at the declared post-setup entry. Complete states, actual requests/raw calls, source callbacks, both expiry receivers and native frees are retained. Real selected TMP rasters run on that same source/Cell runtime; surfaces/projection/detail/A127 are explicit component priors.',
            'Only full outer activation65B580/refresh65B530/AI65B800 use the existing invocation owner with20M instructions/120s limits for native area publication and27/53-shade Convert work. Every other lifecycle invocation retains the historical2M/10s default; completion at actualRET_MAGIC remains mandatory.',
            'After both histories clean up, the last central runtime executes three explicit numeric priors through original65B871..65B89E: normal, low32 multiplication overflow and signed operands. Native pushed red/green/blue words and actual IMUL/IDIV visits are observed with a200-instruction bound, stopping before LightSource access. Two further numeric controls execute full554AA0 on inactive zeroed operand storage with supplied current/proposed parameters; actual memory records its comparison gate and writes. These are not constructed RadSite/LightSource objects or additional scenario histories. CPU/stack, all three RNG streams, live registries, final Cell/source state and callback counts are restored or asserted unchanged; no host arithmetic or lifecycle result supplies an output.',
            'The selected center enrolls its complete contiguous physical TMP suffix family through the same original descriptor/lazy-loader owner. Actual native metadata validates each file. Four phases each retain one native row per physical file; count<=4 uses original fixed16 selector data81CCA8, rotated as declared component inputs while original4814F0/547CF0 select and follow the descriptor observed at547F49. Both selector tables/flag, live RNG and lighting profile are preserved/restored. Raw selector argument and effective physical descriptor identity are distinct observations. This is not whole-world selector or Main bootstrap equivalence.',
            'The historical isolated palette-table assertion VM and lexical-index native CRC probes remain in shared setup; this is not a literal single-VM process. RadSite/LightSource/Cell/raster lifecycle never crosses into another VM.'],
        substitutions=[
            'Existing shared lexical/MIX/file transports, bounded PaletteReader allocator, operator-delete storage seam, CRT/TLS, RGB565 surfaces and empty Windows SEH/checkedIsBadReadPtr. No source/Cell/timer/lifecycle/native drawing result is supplied.',
            'LightPublication existing Map4F42F0 redraw sink returns after observing its argument; original Cell light sink is disabled, so real483E30/484180 run. Sparse six-Cell fixed512 map enrollment and empty live listeners are component priors. Original5657E0 skips missing light targets, while original5657A0 stamps missing field targets into shared Dummy; actual Dummy doubles/coords/registration and alias-call counts are retained without resets. No full physical-circle publication/cache/ID, native scenario, paid DESO, deploy/shot/audio or whole-frame parity.'],
        entry_points=dict(activate=0x65B580, ai=0x65B800, site_ctor=0x65B1E0,
            site_destructor=0x65B2F0, site_delete=0x65BED0, light_ctor=0x554760,
            light_update=0x554AA0, light_delete=0x555150, area=0x554AF0,
            cell_light=0x483E30, cell_gather=0x484180, cell_site=0x487C70,
            expiry=0x7258D0, cell_draw=0x480350, tmp_draw=0x547CF0,
            tint_integer_begin=0x65B871, tint_integer_end=0x65B89E))
    if snow:
        result['scope'] = 'Two original single-site SNOW radiation light lifecycles through real Cell gathering and selected eight-file TMP pixels'
        result['assumptions'][0] = result['assumptions'][0].replace('Battle/XMP', 'Battle/archivedXXmas')
        result['assumptions'][5] = (
            'The selected Snow center enrolls its complete physical Clear01.sno eight-file suffix family through the same original descriptor/lazy-loader owner. '
            'Actual native metadata validates every file. Four phases each retain one native row per physical file; count>4 uses original Main-generated64 '
            'table89E620 rotations as explicit component inputs while original4814F0/547CF0 select and follow the descriptor observed at547F49. '
            'Both selector tables/flag, live RNG and lighting profile are restored. Raw selector argument and effective physical descriptor identity '
            'are distinct observations. No whole-world selector or Main bootstrap equivalence.')
        result['assumptions'].append(
            'Original Map/Theater687631..68764F reads archivedXXmas.MAP and stores Scenario+1258=SNOW1 before the selected ISOSNO.PAL/SNOW.PAL '
            'CellConvert setup. Full65B580 executes its signed Snow red/blue override and signed normalization/ftol; actual working channel bits are '
            'observed at65B604 before conversion, without calculated expected tint. Native SNOWMD General/TileSet and XXmas Lighting readers execute. '
            'The before-AI45 refresh, separate remaining/light timers, full3RNG and real two-receiver expiry/delete histories retain their existing component bounds.')
        result['entry_points']['snow_activation_channels'] = '0x0065B604'
    return result

if __name__=='__main__':
 if '--light-lifecycle' in sys.argv or '--snow-light-lifecycle' in sys.argv:
  import argparse
  parser=argparse.ArgumentParser(add_help=False)
  modes=parser.add_mutually_exclusive_group()
  modes.add_argument('--light-lifecycle',action='store_true')
  modes.add_argument('--snow-light-lifecycle',action='store_true')
  parser.add_argument('--light-cell',default='37,86' if '--snow-light-lifecycle' in sys.argv else '33,88')
  selected,remaining=parser.parse_known_args()
  center=tuple(int(value) for value in selected.light_cell.split(','))
  if len(center)!=2 or any(not 0<=value<512 for value in center):parser.error('--light-cell requires an allocated signed Cell pair in0..511')
  output='radiation_snow_light_lifecycle.json' if selected.snow_light_lifecycle else 'radiation_light_lifecycle.json'
  finish_vectors(lambda:generate_light_lifecycle(center,snow=selected.snow_light_lifecycle),Path(__file__).with_name(output),
   provenance=lambda:light_lifecycle_metadata(snow=selected.snow_light_lifecycle),argv=remaining,source_paths=light_lifecycle_source_paths())
 elif '--overlap' in sys.argv:
  finish_vectors(generate_overlap,Path(__file__).with_name('radiation_overlap.json'),
   provenance=overlap_metadata,argv=[arg for arg in sys.argv[1:] if arg!='--overlap'])
 else:finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
