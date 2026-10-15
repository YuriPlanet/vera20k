"""Bounded original Anim bounce-contact -> Terrain damage and immediate death cleanup.

The --bullet-observer controls enter Terrain damage directly and record four
Bullet callbacks before membership removal; no deferred destructor runs.
All substitutions and fixture state are declared in the companion metadata.
"""
import hashlib,json,struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.spatial_oracle.bridge_damage_admission import *
from tools.rules_oracle.bridge_landing_inputs import recorded_retail_inputs
LANDING=recorded_retail_inputs()
OBJ=MEM+0x30000
TYPE=MEM+0x31000
DAMAGE=MEM+0x32000
DELETE=MEM+0x33000
ANIM=MEM+0x35000
ATYPE=MEM+0x36000
OBJ2=MEM+0x37000
C4=MEM+0x38000

def run(case):
 u=base(dict(flags=0,level=2,anchor_overlay=-1))
 call(u,0x49f2f0)
 neighbors=[]
 for i,(dx,dy) in enumerate(struct.iter_unpack('<hh',u.mem_read(0x89f688,32))):
  ptr=MEM+0x50000+i*0x200;neighbors.append(ptr)
  u.mem_write(ptr,words(0x7e4eec));u.mem_write(ptr+0x24,struct.pack('<hh',10+dx,20+dy));u.mem_write(ptr+0x44,words(-1));u.mem_write(ptr+0x122,b'\x01')
  u.mem_write(TABLE+((20+dy)*512+10+dx)*4,words(ptr))
 u.mem_write(0x89E7C0,words(104));u.mem_write(0x87F914,words(64,64))
 u.mem_write(0x8871E0,words(RULES));u.mem_write(0xA8B230,words(SCENARIO))
 tree=LANDING['terrain_types']['TREE01']
 rules=LANDING['rules'];he=LANDING['warheads']['HE'];c4=LANDING['warheads']['Super']
 u.mem_write(SCENARIO+0x218,seed_bytes(1));u.mem_write(RULES+0x16c8,words(rules['max_damage']))
 u.mem_write(RULES+0x1708,struct.pack('<Q',int(rules['condition_red_bits'],16)))
 u.mem_write(RULES+0x1700,struct.pack('<Q',int(rules['condition_yellow_bits'],16)))
 assert rules['c4_warhead']=='Super'
 u.mem_write(RULES+0xFA8,words(C4));u.mem_write(C4+0xa0,struct.pack('<11Q',*(int(x,16) for x in c4['verses_bits'])))
 u.mem_write(C4+0x124,struct.pack('<I',int(c4['cell_spread_bits'],16)));u.mem_write(C4+0x12c,struct.pack('<I',int(c4['percent_at_max_bits'],16)))
 u.mem_write(C4+0x144,bytes([c4['wall']]));u.mem_write(C4+0x147,bytes([c4['wood']]))
 u.mem_write(0xA8ED84,words(1000))
 u.mem_write(0xAC1398,words(1,0x7fff7fff))
 u.mem_write(0xB0F69C,words(DELETE,32));u.mem_write(0xB0F6A8,words(0))
 u.mem_write(OBJ,words(0x7F522C));u.mem_write(OBJ+0x14,words(2));u.mem_write(OBJ+0x6c,words(case.get('health',10)))
 u.mem_write(OBJ+0x83,b'\x00');u.mem_write(OBJ+0x90,b'\x01');u.mem_write(OBJ+0x98,b'\x01')
 u.mem_write(0xA8E9A0,b'\x01')
 u.mem_write(OBJ+0x9c,words(2688,5248,208));u.mem_write(OBJ+0xc8,words(TYPE))
 u.mem_write(TYPE,words(0x7f5458));u.mem_write(TYPE+0x2b8,words(MEM+0x34000));u.mem_write(MEM+0x34000,words(0,0x7fff7fff));u.mem_write(TYPE+0x2a8,words(tree['temperate_occupation_bits'],tree['snow_occupation_bits']));u.mem_write(TYPE+0x234,b'\x01\x01');u.mem_write(TYPE+0xa0,words(tree['strength']));u.mem_write(TYPE+0x9c,words(tree['armor']));u.mem_write(TYPE+0x233,bytes([case.get('immune',tree['immune'])]));u.mem_write(TYPE+0x2b1,bytes([case.get('spawns',tree['spawns_tiberium'])]))
 u.mem_write(WARHEAD+0x147,bytes([case.get('wood',he['wood'])]));u.mem_write(WARHEAD+0xa0,struct.pack('<11Q',*(int(x,16) for x in he['verses_bits'])));u.mem_write(WARHEAD+0x124,struct.pack('<I',int(he['cell_spread_bits'],16)));u.mem_write(WARHEAD+0x12c,struct.pack('<I',int(he['percent_at_max_bits'],16)))
 u.mem_write(DAMAGE,words(case.get('damage',20)));u.mem_write(CELL+0xe4,words(OBJ));u.mem_write(CELL+0x124,words(0xfc));u.mem_write(CELL+0x122,b'\x00')
 u.mem_write(0xB0CD48,struct.pack('<Q',0x3FC25E5374344960))
 if case.get('contact'):
  u.mem_write(ANIM,words(0x7E3354));u.mem_write(ANIM+0xC8,words(ATYPE));u.mem_write(ANIM+0x90,b'\x01')
  u.mem_write(ANIM+0x9c,words(2688+case.get('offset_x',0),5248+case.get('offset_y',0),208));u.mem_write(ANIM+0x140,struct.pack('<3f',2688.0+case.get('offset_x',0),5248.0+case.get('offset_y',0),208.0))
  u.mem_write(ATYPE+0x2a8,struct.pack('<d',case.get('damage',20)))
  u.mem_write(ATYPE+0x330,words(0 if case.get('null_warhead') else WARHEAD,case.get('radius',80)))
  if case.get('second'):
   u.mem_write(OBJ2,bytes(u.mem_read(OBJ,0x200)));u.mem_write(OBJ2+0x6c,words(200));u.mem_write(OBJ+0x30,words(OBJ2))
 u.mem_write(0x87f778,words(0x7e18fc,MEM+0x3a000,4))
 u.mem_write(0x87f788,words(2 if case.get('second') else 1))
 u.mem_write(MEM+0x3a000,words(OBJ,OBJ2))
 observer=MEM+0x9000;observer_type=MEM+0xa000;observer_source=MEM+0xb000
 if case.get('bullet_observer'):
  # Use original constructors to populate the common Object observer vector.
  # The receiver boundary supplies a Bullet target, not full child Fire/AI.
  for i,vector in enumerate((0xa8e360,0xb0f720,0xb0f670,0xb0f618,0xa8ed40,0xa83c80)):
   u.mem_write(vector,words(0x7eb6d4,MEM+0xc000+i*0x200,64,1,0,10))
  u.mem_write(observer_source,words(0x7f5c70))
 observer_events=[];observer_returns={}
 def observer_state():
  target=read32(u,observer+0x10c);source=read32(u,observer+0xb0)
  if target==OBJ:identity=dict(kind='terrain',cell=[10,20],location=list(struct.unpack('<3i',u.mem_read(OBJ+0x9c,12))))
  elif target==CELL:identity=dict(kind='cell',cell=list(struct.unpack('<2h',u.mem_read(CELL+0x24,4))))
  elif target==0:identity=dict(kind='none')
  else:raise AssertionError(('unexpected observer target',hex(target)))
  assert source in (0,observer_source)
  return dict(target=identity,source='source' if source else None,
              tree_alive=bool(u.mem_read(OBJ+0x90,1)[0]),tree_limbo=bool(u.mem_read(OBJ+0x81,1)[0]),
              tree_is_ground_head=read32(u,CELL+0xe4)==OBJ,
              tree_in_logic=bool(u.mem_read(OBJ+0x98,1)[0]),logic_count=read32(u,0x87f788))
 events=[];recent=[];heap=MEM+0x40000;substitutions=[];receiver_results=[];pending_receivers=[];rng_calls=[]
 rng_before=bytes(u.mem_read(SCENARIO+0x218,0x3f4))
 def ret(value,cleanup):
  sp=u.reg_read(UC_X86_REG_ESP);dest=read32(u,sp)
  u.reg_write(UC_X86_REG_EAX,value);u.reg_write(UC_X86_REG_ESP,sp+4+cleanup);u.reg_write(UC_X86_REG_EIP,dest)
 def hook(uc,a,s,d):
  nonlocal heap
  recent.append(a)
  if len(recent)>32:recent.pop(0)
  sp=u.reg_read(UC_X86_REG_ESP)
  if case.get('bullet_observer'):
   if (a,sp) in observer_returns:
    observer_events[observer_returns.pop((a,sp))]['after']=observer_state()
   if a==0x4684e0:
    expired=read32(u,sp+4)
    observer_events.append(dict(event='bullet_pointer_expired',
       expired='tree' if expired==OBJ else 'source',control=u.mem_read(sp+8,1)[0],before=observer_state()))
    observer_returns[(read32(u,sp),sp+12)]=len(observer_events)-1
   elif a in (0x5f65f0,0x71c930,0x47ea90,0x55bae0):
    observer_events.append(dict(event={0x5f65f0:'uninit',0x71c930:'terrain_limbo',
       0x47ea90:'cell_remove',0x55bae0:'logic_remove'}[a],state=observer_state()))
  if a in (0x47ea90,0x71c070,0x71b920,0x5f5390,0x5f5280,0x7258d0,0x5f65f0,0x71c930,0x5f4d30,0x71bfb0,0x5687f0,0x489280,0x5f42f0,0x422b80):
   e=dict(a=f'{a:08X}',obj=f'{u.reg_read(UC_X86_REG_ECX):08X}',caller=f'{read32(u,sp):08X}',first_health=signed(read32(u,OBJ+0x6c)));events.append(e)
   if a==0x71b920:
    dp=read32(u,sp+4);e.update(damage=signed(read32(u,dp)),distance=signed(read32(u,sp+8)),args=[read32(u,sp+i*4) for i in range(1,8)]);pending_receivers.append((read32(u,sp),u.reg_read(UC_X86_REG_ECX),dp))
   elif a==0x489280:
    e.update(coord=list(struct.unpack('<3i',u.mem_read(u.reg_read(UC_X86_REG_ECX),12))),damage=signed(u.reg_read(UC_X86_REG_EDX)),args=[read32(u,sp+i*4) for i in range(1,5)])
  if pending_receivers and a==pending_receivers[-1][0]:
   _,obj,dp=pending_receivers.pop();receiver_results.append(dict(obj=obj,result=signed(u.reg_read(UC_X86_REG_EAX)),damage_after=signed(read32(u,dp)),health=signed(read32(u,obj+0x6c))))
  if a in (0x65c780,0x65c7e0):rng_calls.append(dict(a=f'{a:08X}',caller=f'{read32(u,sp):08X}'))
  if a in (0x7c8e17,0x7c8b3d,0x439b00,0x71d160,0x6d2790,0x5f5850,0x439150,0x4a9770,0x47d2b0,0x56d460,0x584550,0x6551c0):
   substitutions.append(dict(a=f'{a:08X}',caller=f'{read32(u,sp):08X}',this=u.reg_read(UC_X86_REG_ECX)))
  if a==0x7c8e17:
   if read32(u,sp)==0x71b9c5:ret(0,0)
   else:
    p=heap;heap+=(read32(u,sp+4)+15)&~15;ret(p,0)
  elif a==0x7c8b3d:ret(0,0)
  elif a==0x439b00:ret(1,0)
  elif a==0x71d160:
   out=read32(u,sp+4);u.mem_write(out,words(0,0,1,1));ret(out,4)
  elif a==0x6d2790:ret(0,20)
  elif a==0x5f5850:ret(1,4)
  elif a in (0x439150,0x4a9770,0x47d2b0,0x56d460,0x584550,0x6551c0):ret(0,4)
 u.hook_add(UC_HOOK_CODE,hook)
 observer_initial=None;observer_source_before_damage=None
 if case.get('bullet_observer'):
  u.mem_write(MEM+0xf000,b'SmallCometP\0')
  call(u,0x46bbc0,observer_type,(MEM+0xf000,))
  call(u,0x466380,observer)
  call(u,0x4664c0,observer,(observer_type,OBJ,observer_source,30,WARHEAD,74,0))
  observer_initial=observer_state()
  if case.get('source_first'):
   call(u,0x4684e0,observer,(observer_source,0))
   for pending,index in list(observer_returns.items()):
    if pending[0]==RET_MAGIC:
     observer_events[index]['after']=observer_state();del observer_returns[pending]
   observer_source_before_damage=observer_state()
 u.mem_write(SP,words(RET_MAGIC,DAMAGE,case.get('distance',0),0 if case.get('null_warhead') else WARHEAD,0,0,0,0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,ANIM if case.get('contact') else OBJ)
 try:run_checked(u,0x423930 if case.get('contact') else 0x71B920,RET_MAGIC,count=200000)
 except Exception:
  print(json.dumps(events,indent=2));print('recent',','.join(hex(a) for a in recent));raise
 if pending_receivers:
  _,obj,dp=pending_receivers.pop();receiver_results.append(dict(obj=obj,result=signed(u.reg_read(UC_X86_REG_EAX)),damage_after=signed(read32(u,dp)),health=signed(read32(u,obj+0x6c))))
 result=signed(u.reg_read(UC_X86_REG_EAX))
 observer_after_receiver=None;observer_repeated=None;observer_source_expired=None
 if case.get('bullet_observer'):
  observer_after_receiver=observer_state()
  # run_checked stops before RET_MAGIC, so resolve that last observer return
  # after each direct call without inventing a callback result.
  for expired,label in ((OBJ,'repeat_tree'),(observer_source,'source_expiry')):
   call(u,0x4684e0,observer,(expired,1 if expired==OBJ else 0))
   for pending,index in list(observer_returns.items()):
    if pending[0]==RET_MAGIC:
     observer_events[index]['after']=observer_state();del observer_returns[pending]
   if label=='repeat_tree':observer_repeated=observer_state()
   else:observer_source_expired=observer_state()
  assert not observer_returns
 rng_after=bytes(u.mem_read(SCENARIO+0x218,0x3f4))
 rng_unchanged=rng_before==rng_after
 observer_rng_calls=list(rng_calls)
 continuation=[call(u,0x65c780,SCENARIO+0x218) for _ in range(4)]
 output=dict(case=case,events=events,receiver_results=receiver_results,substitutions=substitutions,neighbor_counts=[u.mem_read(p+0x122,1)[0] for p in neighbors],rng_unchanged=rng_unchanged,next_rng=continuation,result=result,health=signed(read32(u,OBJ+0x6c)),alive=u.mem_read(OBJ+0x90,1)[0],limbo=u.mem_read(OBJ+0x81,1)[0],ground_head=read32(u,CELL+0xe4),occupation=read32(u,CELL+0x124),delete_count=read32(u,0xB0F6A8),successor=read32(u,OBJ+0x30),second_hp=signed(read32(u,OBJ2+0x6c)),logic_count=read32(u,0x87f788),in_logic=u.mem_read(OBJ+0x98,1)[0])
 if case.get('bullet_observer'):
  output['observer']=dict(initial=observer_initial,events=observer_events,
     after_receiver=observer_after_receiver,after_repeat=observer_repeated,
     after_source_expiry=observer_source_expired,
     after_source_before_damage=observer_source_before_damage,
     scenario_before=rng_before.hex(),scenario_after=rng_after.hex(),rng_calls=observer_rng_calls)
 return output
def generate():
 cases=[]
 for health in (200,16,15,14,10,1,0):
  cases.append(dict(name=f'large_contact_hp_{health}',contact=True,health=health,damage=20))
 for damage in (10,0,-20):
  cases.append(dict(name=f'contact_damage_{damage}',contact=True,health=10,damage=damage))
 for gate in ('immune','wood','null_warhead'):
  cases.append(dict(name=f'contact_gate_{gate}',contact=True,health=10,damage=20,**{gate:0 if gate=='wood' else 1}))
 for radius,damage in ((80,20),(50,10)):
  for distance in (radius-1,radius,radius+1):
   cases.append(dict(name=f'contact_radius_{radius}_distance_{distance}',contact=True,health=200,damage=damage,radius=radius,offset_x=distance))
 for health in (200,10):
  cases.append(dict(name=f'linked_contact_hp_{health}',contact=True,health=health,second=True))
 cases.append(dict(name='custom_destructible_spawner_allocator_failure',contact=True,health=10,spawns=True))
 cases.append(dict(name='stock_immune_spawner',contact=True,health=200,spawns=True,immune=True))
 cases.append(dict(name='direct_receive',health=10,damage=20,distance=11))
 return dict(cases=[run(case) for case in cases])

def metadata():
 result=provenance(scope='Original423930 bounce-contact admission through original71B920 Terrain receiver, Object damage kernel, immediate Limbo/Cell unlink/Logic removal/deferred deletion. Bounded plain-tree state; custom destructible SpawnsTiberium row supplies allocation failure and executes original nested C4 area body. Not full debris physics, animation, observers, pathfinding, or whole Terrain parity.', assumptions=[
  'Pinned retail image loaded; no executable code writes. Native x87 control word0x0E7F. Original49F2F0 initializes actual8-neighbor offsets; actual map table contains cell10,20 level2 and all8 neighboringcells. One-cell footprint; originalvtable pointers for Terrain/TerrainType/Anim/Cell/Logic; no houses/entities/bombs/animations in their global vectors.',
  'Terrain location2688,5248,208, maxHP200, armorwood6; independent native rules_oracle/bridge_landing_inputs executes TerrainType ctor and selected rules/ART readers for TREE01/TreeStrength200/Armor6/Immune0/Spawns0/occupation4,6/Foundation1x1, HE and Super. Anim fields have independent rules_oracle/bridge_anim_inputs evidence. This harness consumes those results; no INI readers execute inside each contact row.',
  'HE/Super exact scalar bits and Rules fields come from independently executed original readers in rules_oracle/bridge_landing_inputs. Active retail HE wood0.75, CellSpread/PAM0.5; MaxDamage10000; ConditionRed0.25 at+1708, Yellow0.5 at+1700. C4 Super allverses1, Woodfalse, spread0, PAM1; emptyAnimList supplied. NoDamagefalse; gameRunningtrue; frame1000; ScenarioRNG originalseed1.',
  'Second linked Terrain is a synthetic mixed-list continuation boundary, not legal map-placement proof. Both occupy suppliedsamecell; only first has neighbor contribution. Allfixtures use one-cell native footprint, terrain occupationbits4 (production retail TREE01), rawoccupation0xFC, Logic membershippresent, no tag/audio/activewarp effect state.',
  'Process initialized footprint sentinel supplied atAC1398; B0CD48 supplied native render-depth double bits0x3FC25E5374344960. Pending-delete buffer32 and Logicbuffer4 preallocated. Unrelated global vectors empty. Neighbor counts initially1; center count0.',
 ], substitutions=[
  '439B00 BounceClass::Update returnscontact1 with suppliedbodyfloat position. This bounds evidence after physics; original423930 reads/ftol/body and handles contact/distance/receiver/next-link logic.',
  '7C8E17 operatornew supplied bump allocator; allocation from71B9C0 alwaysfails in explicitcustomSpawns row, skippingExplosionAnim selection/construction. Other area collection allocationssucceed. 7C8B3D delete no-op.',
  '71D160 Terrain render rectangle returns supplied0,0,1,1;6D2790 TacticalDirty no-op;5F5850 ObjectMark returns1 so originalTerrainMark/ExitCell/CellRemoveContent/occupation execute.',
  '439150 empty BombListPointerGotInvalid boundary;4A9770 displayremoval boundary;47D2B0 cellrecalc,56D460 orphanZone,584550 incrementalZone,6551C0 radarDirty boundaries eachreturn0. Originalcall order logged, callees notproved.',
 ],entry_points={'contact':0x423930,'terrain_receiver':0x71B920,'object_receiver':0x5F5390,'damage_kernel':0x489180,'terrain_limbo':0x71C930,'object_limbo':0x5F4D30,'cell_remove':0x47EA90,'logic_unregister':0x55BAE0,'uninit':0x5F65F0,'area':0x489280,'neighbor_init':0x49F2F0})
 result['native_landing_inputs_sha256']=hashlib.sha256((Path(__file__).parent.parent/'rules_oracle/bridge_landing_inputs.json').read_bytes()).hexdigest()
 return result
def bullet_observer_generate():
 return dict(cases=[run(dict(name=name,bullet_observer=True,health=10,damage=20,
                            source_first=source_first))
                    for name,source_first in (('tree_before_source',False),
                                               ('source_before_tree',True))])

def bullet_observer_metadata():
 result=provenance(
  scope='Original Terrain lethal receiver and populated Object observer broadcast through '
        'Bullet PointerExpired before Terrain Limbo, repeated expiry and independent source expiry. '
        'Two direct receiver controls; no child Fire/AI or deferred destructor execution.',
  assumptions=[
   'Pinned original image; native x87 control0x0E7F. Original49F2F0 initializes the8-neighbor '
   'offsets. Supplied map has cell10,20 at level2 and all8 neighbors. One TREE01 occupies '
   'the ground head and Logic vector, with one-cell occupationbits4, rawoccupation0xFC, '
   'neighbor counts1 and center count0. No second linked Terrain, Anim contact, or custom spawner.',
   'Terrain has retained Location2688,5248,208, supplied health10, maxHP200 and armorwood6. '
   'Independent rules_oracle/bridge_landing_inputs executes original TREE01 rules/ART readers '
   'and HE/Super readers. This receiver consumes TREE01 Immune0/Spawns0/occupation4,6, '
   'HE Woodtrue/woodverses0.75 and Rules MaxDamage10000/ConditionRed0.25/ConditionYellow0.5. '
   'Damage20, distance0, gameRunningtrue and frame1000 are supplied. NoDamagefalse. '
   'No INI reader executes inside either receiver row; initialized Super/C4 fields are unused.',
   'Original BulletType46BBC0, Bullet466380 and Construct4664C0 populate real Object observer '
   'vectorB0F724 with one observer targeting Terrain and a separate supplied Unit-vtable source. '
   'Other observer vectors are empty. BulletType keeps constructor defaults; SmallCometP is '
   'its supplied ID, not a physical type read. Construct receives Damage30, HE, speed74 and '
   'brightfalse; these payload values do not control PointerExpired and no Fire/AI runs.',
   'Original ObjectReceiveDamage DetachAll, Terrain lethal DetachAll, UnInit direct broadcast '
   'and ObjectLimbo DetachAll produce four Bullet callbacks before Cell/Logic removal. '
   'No deferred destructor executes. Tree-before-source then directly repeats tree expiry '
   '(control1) and expires source(control0); source-before-tree first expires source(control0). '
   'Callback control records the byte consumed by4685CC, excluding upper stack bytes. '
   'Complete Scenario seed1 state is captured before/after this interval; four subsequent '
   'native RNG words are only the separately recorded continuation check.',
   'Process footprint sentinelAC1398 and render-depthB0CD48 bits0x3FC25E5374344960 are supplied. '
   'Pending-delete capacity32 and Logic capacity4 are preallocated. No tag, audio, warp, '
   'house, bomb or live animation state; full rendering/pathfinding is excluded.',
  ],
  substitutions=[
   'Executed:71D160 render rectangle returns0,0,1,1;6D2790 TacticalDirty returns0; '
   '5F5850 ObjectMark returns1 so original TerrainMark/ExitCell/CellRemoveContent execute.',
   'Executed:439150 empty BombListPointerGotInvalid,4A9770 display removal,47D2B0 cell recalc, '
   '56D460 orphan zone,584550 incremental zone and6551C0 radar dirty each return0. '
   'Original call order is recorded; these external effects are not proved.',
   'Installed but unexecuted for these rows:439B00 Anim bounce-contact,7C8E17 allocator '
   '(including its custom-spawner failure arm),7C8B3D delete. The shared runner retains '
   'those hooks for its other controls; no result here depends on them.',
  ],entry_points=dict(terrain_receiver=0x71B920,object_receiver=0x5F5390,
                     damage_kernel=0x489180,terrain_limbo=0x71C930,object_limbo=0x5F4D30,
                     cell_remove=0x47EA90,logic_unregister=0x55BAE0,uninit=0x5F65F0,
                     neighbor_init=0x49F2F0,bullet_type_ctor=0x46BBC0,bullet_ctor=0x466380,
                     bullet_construct=0x4664C0,bullet_pointer_expired=0x4684E0,
                     detach_all=0x5F5280,observer_broadcast=0x7258D0))
 result['native_landing_inputs_sha256']=hashlib.sha256((Path(__file__).parent.parent/'rules_oracle/bridge_landing_inputs.json').read_bytes()).hexdigest()
 return result

if __name__=='__main__':
 import sys
 if '--bullet-observer' in sys.argv:
  args=[arg for arg in sys.argv[1:] if arg!='--bullet-observer']
  finish_vectors(bullet_observer_generate,Path(__file__).with_name('terrain_bullet_expiry.json'),
                 provenance=bullet_observer_metadata,argv=args)
 else:
  finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
