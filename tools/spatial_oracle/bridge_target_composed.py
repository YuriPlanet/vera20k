"""Original empty-FV passive AI block through scan, Evaluate, G27 and assignment.

Research for the next chain only. Native Unit lifecycle/map admission and House
construction are supplied; full native AI/movement/bridge mutation are not claimed.
No scan, fire-error, candidate, score, assignment or RNG return is substituted.
"""
import hashlib, json, struct
from pathlib import Path
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC, run_checked, finish_vectors, provenance
from tools.projectile_oracle.ifv_fire_coord import prepare
from tools.projectile_oracle.bridge_render_inputs import assets_root, lexical
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords

BEGIN, END = 0x6FA65A, 0x6FA6F5
DUMMY = 0xABDC50
GENERAL_KEYS = {'NormalTargetingDelay','GuardAreaTargetingDelay',
 'MyEffectivenessCoefficientDefault','TargetEffectivenessCoefficientDefault',
 'TargetSpecialThreatCoefficientDefault','TargetStrengthCoefficientDefault','TargetDistanceCoefficientDefault',
 'DumbMyEffectivenessCoefficient','DumbTargetEffectivenessCoefficient',
 'DumbTargetSpecialThreatCoefficient','DumbTargetStrengthCoefficient','DumbTargetDistanceCoefficient'}
SLICES = ((BEGIN,END),(0x709820,0x7099CD),(0x6F8682,0x6F86FE),
          (0x565730,0x565798),(0x735592,0x7355C6))
# These additional fields are reached by the ordinary FV scan/fire-error path.
# Physical FV leaves the keys absent, but execute their original retained reads
# as well as the constructor so the preparation does not silently assume zero.
RETAINED_TECHNO_READERS = (
 ('LandTargeting',0x604,4,0x71219A,0x7121B7),
 ('IsTrain',0xC94,1,0x712270,0x71228A),
 ('VHPScan',0x394,4,0x71256D,0x712587),
 ('OpenTopped',0x5E4,1,0x7143B6,0x7143D0),
 ('MobileFire',0x6AE,1,0x71481C,0x714836),
 ('OpportunityFire',0x6AF,1,0x714836,0x714850),
 ('SprayAttack',0x691,1,0x71490B,0x714925),
 ('Natural',0x693,1,0x71493F,0x714959),
 ('Invisible',0xC9A,1,0x714A97,0x714AB1),
 ('NoAutoFire',0xD20,1,0x714AF3,0x714B14),
 ('HunterSeeker',0xD27,1,0x714CA7,0x714CC8),
 ('Organic',0xD97,1,0x715024,0x715045),
 ('AttackFriendlies',0x6C0,1,0x715227,0x715248),
)
RETAINED_UNIT_FIELDS = (('DeployToFire',0xE12),('IsSimpleDeployer',0xE13),
 ('SmallVisceroid',0xE18),('LargeVisceroid',0xE19),('NonVehicle',0xE1B))

def signed(m,p):return struct.unpack('<i',m.u.mem_read(p,4))[0]
def coord(m,p):return list(struct.unpack('<3i',m.u.mem_read(p,12)))

def retained_type_state(m,typ):
 result={name:int.from_bytes(m.u.mem_read(typ+offset,size),'little',signed=size==4)
  for name,offset,size,_,_ in RETAINED_TECHNO_READERS}
 result.update({name:m.u.mem_read(typ+offset,1)[0]for name,offset in RETAINED_UNIT_FIELDS})
 result.update({name:m.u.mem_read(typ+offset,1)[0]for name,offset in
  (('LegalTarget',0x231),('Insignificant',0x232),('Immune',0x233))})
 return result

def read_retained_type_fields(m,typ):
 u=m.u
 # The entry also stores the preceding Selectable result; preserve that actual
 # current byte while executing LegalTarget, Armor, Strength, Immune and
 # Insignificant through their final stores.
 for reg,val in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBX,typ),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EBP,typ+0x24),(UC_X86_REG_EAX,u.mem_read(typ+0x230,1)[0])):u.reg_write(reg,val)
 run_checked(u,0x5F9499,0x5F9521)
 for _,_,_,begin,end in RETAINED_TECHNO_READERS:
  for reg,val in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,typ),(UC_X86_REG_EBX,typ+0x24),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EDI,RULES)):u.reg_write(reg,val)
  run_checked(u,begin,end)
 for begin,end in ((0x74766B,0x74769F),(0x747862,0x74789C),(0x74789C,0x7478B6)):
  # Visceroid entry has an interleaved CanBeach store at747868. Preserve
  # its already-retained byte; it is not a targeting input in these controls.
  for reg,val in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EDI,typ),(UC_X86_REG_EBP,typ+0x24),(UC_X86_REG_EBX,RULES),(UC_X86_REG_EAX,u.mem_read(typ+0xE17,1)[0])):u.reg_write(reg,val)
  run_checked(u,begin,end)

def setup():
 m,source,typ,weapon,cells,initial=prepare();u=m.u
 # Object translation-unit CRT initializers are the actual PE table entries.
 # They establish GetHeight bridge416 and IsHighFlying level104 constants.
 object_initializers=struct.unpack('<14I',u.mem_read(0x8141D8,56))
 for address in object_initializers:m.invoke(address,0)
 initial['object_initializers']=[f'{a:08x}'for a in object_initializers]
 initial['object_height_globals']={f'{p:08x}':signed(m,p)for p in (0xAC13C8,0xAC13BC)}
 # Techno has its own height constants consumed by InRange/GetFireError.
 # Run the original scalar CRT group, not Object's numerically equal globals.
 # The following two CRT entries construct unrelated vectors and are excluded.
 techno_initializers=struct.unpack('<14I',u.mem_read(0x815040,56))
 for address in techno_initializers:m.invoke(address,0)
 initial['techno_initializers']=[f'{a:08x}'for a in techno_initializers]
 initial['techno_height_globals']={f'{p:08x}':signed(m,p)for p in (0xB0EB34,0xB0EB24)}
 rules=m.read32(0x8871E0);m.invoke(0x665650,rules)
 initial['rules_max_damage_constructor']=signed(m,rules+0x16C8)
 initial['retained_type_constructor']=retained_type_state(m,typ)
 warhead=m.read32(weapon+0xAC);layers=[]
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=assets_root()/name
  if not path.exists():layers.append(dict(file=name,absent=True));continue
  raw=path.read_bytes();data,_=lexical(raw,{'HE','FV','General','CombatDamage'})
  # Only this CombatDamage key is consumed by the selected passive estimate.
  # Keep the physical section's presence for original section admission.
  if 'CombatDamage' in data:data['CombatDamage']={k:v for k,v in data['CombatDamage'].items()if k=='MaxDamage'}
  m.rules_cache(data);m.invoke(0x75D3A0,warhead,(RULES,))
  type_admitted=bool(m.invoke(0x526810,RULES,(typ+0x24,))&255)
  if type_admitted:
   read_retained_type_fields(m,typ)
   for begin,end in ((0x71446C,0x714486),(0x71479A,0x7147B4),(0x714850,0x71486A)):
    for reg,val in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,typ),(UC_X86_REG_EBX,typ+0x24),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EDI,RULES)):u.reg_write(reg,val)
    run_checked(u,begin,end)
  selected={k:v for k,v in data.get('General',{}).items() if k in GENERAL_KEYS}
  # Original Process executes on selected physical General keys; unrelated
  # sections omitted. This is not complete retail Process/type discovery.
  m.rules_cache({'General':selected} if selected else {});m.invoke(0x668BF0,rules,(RULES,))
  m.rules_cache(data)
  combat_admitted=bool(m.invoke(0x526810,RULES,(m.cstring('CombatDamage'),))&255)
  if combat_admitted:
   # Original retained MaxDamage read. The entry also stores the preceding
   # HomingScatter result, so preserve that existing field in EAX.
   for reg,val in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,rules),(UC_X86_REG_EDI,RULES),(UC_X86_REG_EAX,m.read32(rules+0x1730))):u.reg_write(reg,val)
   run_checked(u,0x66CE2C,0x66CE57)
  if type_admitted:
   for begin,end in ((0x7122A4,0x7122BE),(0x7149C7,0x7149E1),(0x71551E,0x71574E)):
    u.mem_write(SP+0x380,dwords(RULES))
    for reg,val in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,typ),(UC_X86_REG_EBX,typ+0x24),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EDI,RULES)):u.reg_write(reg,val)
    run_checked(u,begin,end)
  layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),general=selected,
   combat_damage=data.get('CombatDamage',{}),combat_damage_admitted=combat_admitted,max_damage=signed(m,rules+0x16C8),
   type_admitted=type_admitted,retained_type=retained_type_state(m,typ),
   fv={k:data.get('FV',{}).get(k)for k in ('Strength','Armor','CanPassiveAquire','DistributedFire','Ammo','InitialAmmo','GuardRange','AirRangeBonus','ThreatPosed','SpecialThreatValue','MyEffectivenessCoefficient','TargetEffectivenessCoefficient','TargetSpecialThreatCoefficient','TargetStrengthCoefficient','TargetDistanceCoefficient','LegalTarget','Insignificant','Immune')+tuple(r[0]for r in RETAINED_TECHNO_READERS)+tuple(r[0]for r in RETAINED_UNIT_FIELDS)}))
 # ART is fixed, not layered. Execute both FiringSyncFrame%d reads/stores;
 # the preceding MaxDeathCounter store is preserved from the actual constructor.
 for reg,val in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EDI,typ),(UC_X86_REG_ESI,typ+0x24),(UC_X86_REG_EAX,m.read32(typ+0xE38))):u.reg_write(reg,val)
 run_checked(u,0x747AA2,0x747B03)
 initial['art_firing_sync_frames']=list(struct.unpack('<2i',u.mem_read(typ+0xE40,8)))
 houses=[]
 for index in (0,1):
  house=m.alloc(0x6000);country=m.alloc(0x300)
  u.mem_write(house+0x30,dwords(index,country));u.mem_write(house+0x1EC,b'\x01')
  # Supplied two hostile human Houses and unmodified combat factors, not
  # claimed native House/Country initialization.
  u.mem_write(house+0x188,struct.pack('<d',1.));u.mem_write(country+0x100,struct.pack('<6f',*([1.]*6)))
  u.mem_write(house+0x5600,dwords(-1))
  # Actual non-null HouseType constructor arm: ordinary player Houses select
  # their TechnoType coefficients, not the distinct Dumb General values.
  for reg,val in ((UC_X86_REG_EBP,house),(UC_X86_REG_ESI,country),(UC_X86_REG_EBX,0)):u.reg_write(reg,val)
  run_checked(u,0x4F643B,0x4F6455);houses.append(house)
 target=m.alloc(0x1000);u.mem_write(target,bytes(u.mem_read(source,0x1000)))
 for obj,house,xyz in ((source,houses[0],(2688,5248,624)),(target,houses[1],(3200,5248,624))):
  loco=m.alloc(0x100);m.invoke(0x4AF540,loco);u.mem_write(loco+0xC,dwords(obj));u.mem_write(obj+0x674,dwords(loco+4))
  u.mem_write(obj+0x14,dwords(5));u.mem_write(obj+0x21C,dwords(house));u.mem_write(obj+0x9C,dwords(*xyz))
  u.mem_write(obj+0x2B4,dwords(0));u.mem_write(obj+0xAC,dwords(5))
  u.reg_write(UC_X86_REG_ESI,obj);u.reg_write(UC_X86_REG_EDI,0xFFFFFFFF)
  run_checked(u,0x735592,0x7355C6) # actual type->Ammo/Cloak/HP/estimated HP initialization
  run_checked(u,0x735416,0x73541C) # actual ctor sentinel Unit+6D8=-1
  u.mem_write(obj+0x158,struct.pack('<2d',1.,1.));u.mem_write(obj+0x74,b'\x01');u.mem_write(obj+0x3D5,b'\x01')
 # Supplied native map diamond Size20x20, fixed-stride sparse cells inherited
 # from the IFV fixture, plus AirTracker's read-only extent scalars64x64.
 # Tracker buckets remain empty. No Map loader or bridge driver is executed.
 u.mem_write(0x87F8DC,dwords(20,20));u.mem_write(0x87F914,dwords(64,64))
 u.mem_write(0xA8B238,dwords(1));u.mem_write(0xA8ED84,dwords(173))
 scenario=m.read32(0xA8B230);m.invoke(0x65C6D0,scenario+0x218,(31,))
 for c in cells.values():u.mem_write(c+0xE4,dwords(0,0));u.mem_write(c+0x140,dwords(0))
 u.mem_write(DUMMY+0x24,struct.pack('<2h',111,-222))
 return m,source,target,typ,weapon,cells,rules,dict(layers=layers,initial=initial,
  supplied_world=dict(source_xy=[2688,5248],target_xy=[3200,5248],cell_level=6,cell_slope=0,ground_z=624,deck_z=1040,map_size=[20,20],air_tracker_extent=[64,64],source_mission=5,source_heading=0,turret_heading=16383,human_house_indices=[0,1],country_armor_factors=[1]*6,firepower=1),
  source_type=m.string(typ+0x24),weapon=m.string(weapon+0x24),strength=signed(m,typ+0xA0),armor=signed(m,typ+0x9C),
  weapon_range=signed(m,weapon+0xB4),weapon_minimum_range=signed(m,weapon+0xB8),weapon_damage=signed(m,weapon+0xA4),weapon_rof=signed(m,weapon+0xB0),
  projectile=m.string(m.read32(weapon+0xA0)+0x24),projectile_aa=u.mem_read(m.read32(weapon+0xA0)+0x2A4,1)[0],projectile_ag=u.mem_read(m.read32(weapon+0xA0)+0x2A5,1)[0],
  air_range_bonus=signed(m,typ+0x68C),max_damage=signed(m,rules+0x16C8),
  normal_delay=signed(m,rules+0xE08),area_delay=signed(m,rules+0xE04),
  guard_range=signed(m,typ+0x5B8),threat_posed=signed(m,typ+0x670),special_threat=struct.unpack('<d',u.mem_read(typ+0x2C0,8))[0],house_selects_own_coefficients=u.mem_read(houses[0]+0x1FB,1)[0],
  type_coefficients=list(struct.unpack('<5d',u.mem_read(typ+0x2C8,40))),
  general_default_coefficients=list(struct.unpack('<5d',u.mem_read(rules+0x1040,40))),
  dumb_coefficients_bits=[f'{struct.unpack("<Q",u.mem_read(rules+x,8))[0]:016x}'for x in (0x1068,0x1070,0x1078,0x1080,0x1088)])

def history(name, steps, direct=False):
 m,source,target,typ,weapon,cells,rules,inputs=setup();u=m.u;scenario=m.read32(0xA8B230)
 alternate=0
 if any('alternative' in step for step in steps):
  alternate=m.alloc(0x1000);u.mem_write(alternate,bytes(u.mem_read(target,0x1000)))
  loco=m.alloc(0x100);m.invoke(0x4AF540,loco);u.mem_write(loco+0xC,dwords(alternate));u.mem_write(alternate+0x674,dwords(loco+4))
 immutable=[bytes(u.mem_read(a,b-a))for a,b in SLICES]
 lookup_by_pc={0x6FC197:'fire_error_target',0x741078:'unit_fire_error_source',0x6F86AA:'g27_source',0x6F86D5:'g27_target'}
 ptrs={source:'source',target:'target',0:'null',DUMMY:'dummy',**{p:f'cell_{x}_{y}'for (x,y),p in cells.items()}}
 if alternate:ptrs[alternate]='alternative'
 events=[];writes=[];last=[];lookups=[];cell_lookups=[];pending_returns={};pending_ops={};getter_stack=[];instruction=[0]
 def who(p):return ptrs.get(p,f'{p:08x}')
 def state():return dict(target=who(m.read32(source+0x2B4)),passive=u.mem_read(source+0x50C,1)[0],
  last_scan=signed(m,source+0x4FC),timer=list(struct.unpack('<3i',u.mem_read(source+0x180,12))),
  target_health=signed(m,target+0x6C),estimated_health=signed(m,target+0x70),alternative_estimated_health=signed(m,alternate+0x70) if alternate else None,
  dummy_coord=list(struct.unpack('<2h',u.mem_read(DUMMY+0x24,4))),native_cursor=signed(m,scenario+0x214))
 field_addresses={source+0x4FC:'last_scan',source+0x180:'timer_start',source+0x184:'timer_aux',source+0x188:'timer_duration',source+0x2B4:'target',source+0x50C:'passive',target+0x70:'estimated_health',DUMMY+0x24:'dummy_coord'}
 def observe(uc,pc,size,data):
  instruction[0]+=1
  previous=last[-1] if last else None;last.append(pc);del last[:-20]
  sp=u.reg_read(UC_X86_REG_ESP)
  if pc in pending_ops:
   for row in pending_ops.pop(pc):
    row['return']=u.reg_read(UC_X86_REG_EAX)&255 if row['op']=='high_flying' else struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]
    if row['op']=='object_get_cell':
     row['return']=who(u.reg_read(UC_X86_REG_EAX));row['return_identity']=row['return'];getter_stack.pop()
  if pc in (0x5F6960,0x5F6B90,0x5F5F40):
   op={0x5F6960:'object_get_cell',0x5F6B90:'high_flying',0x5F5F40:'height'}[pc]
   row=dict(op=op,caller=f'{m.read32(sp):08x}',object=who(u.reg_read(UC_X86_REG_ECX)),instruction=instruction[0])
   events.append(row);pending_ops.setdefault(m.read32(sp),[]).append(row)
   if op=='object_get_cell':getter_stack.append(row)
  if pc in pending_returns:
   for row in pending_returns.pop(pc):
    row['return_identity']=who(u.reg_read(UC_X86_REG_EAX));row['dummy_coord']=state()['dummy_coord']
  if pc==0x5657A0:
   row=dict(caller=f'{m.read32(sp)-5:08x}',xy=list(struct.unpack('<2h',u.mem_read(m.read32(sp+4),4))),instruction=instruction[0])
   cell_lookups.append(row);pending_returns.setdefault(m.read32(sp),[]).append(row)
  if pc==0x565730:
   caller=m.read32(sp)-5;row=dict(caller=f'{caller:08x}',owner=lookup_by_pc.get(caller,'other'),xyz=coord(m,m.read32(sp+4)))
   row['instruction']=instruction[0]
   if getter_stack:row.update(object_get_cell_caller=getter_stack[-1]['caller'],object=getter_stack[-1]['object'])
   lookups.append(row);pending_returns.setdefault(m.read32(sp),[]).append(row)
  if pc in (0x68BCB0,0x7258D0,0x5F65F0):raise AssertionError(('unexpected identity/detach',hex(pc)))
  if pc in (0x7C8E17,0x7C8B3D,0x7D140B,0x5B40B0):raise AssertionError(('unexpected substituted runtime boundary',hex(pc)))
  if pc in (0x709290,0x709820,0x743190,0x4D9920,0x6F8DF0,0x6F7CA0,0x6F8682,0x6F86FE,0x70CD10):
   row=dict(pc=f'{pc:08x}')
   if pc==0x6F7CA0:row.update(candidate=who(m.read32(sp+16)),method=m.read32(sp+4),flags=m.read32(sp+8),range=signed(m,sp+12))
   if pc==0x6F8DF0:row['mask']=m.read32(sp+4)
   events.append(row)
  elif pc==0x65C7E0:events.append(dict(pc=f'{pc:08x}',bounds=[signed(m,sp+4),signed(m,sp+8)],caller=f'{m.read32(sp):08x}'))
  elif pc==0x65C84B:events.append(dict(raw=u.reg_read(UC_X86_REG_ESI)))
  elif pc==0x709888:events.append(dict(ranged_return=u.reg_read(UC_X86_REG_EAX)))
  elif pc==0x6F7CEE:events.append(dict(fire_error_return=u.reg_read(UC_X86_REG_EAX),dummy_coord=state()['dummy_coord']))
  elif pc==0x7098EC:events.append(dict(retained_fire_error_return=u.reg_read(UC_X86_REG_EAX)))
  elif pc==0x6F901E:events.append(dict(resolved_scan_range=u.reg_read(UC_X86_REG_EAX)))
  elif pc==0x6F9148:events.append(dict(scanner_radius=signed(m,sp+0x38),scan_range_argument=signed(m,sp+0x2C)))
  elif pc==0x6F7379:
   bp=u.reg_read(UC_X86_REG_EBP)
   events.append(dict(in_range_target=coord(m,sp+0x20),in_range_source=coord(m,m.read32(bp+8))))
  elif pc==0x6F7655:events.append(dict(in_range_return=0))
  elif pc==0x6F764C:events.append(dict(in_range_return=u.reg_read(UC_X86_REG_EAX)&255))
  elif pc==0x6F8710:events.append(dict(score_before_vhp=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]))
  elif pc==0x6F894F:events.append(dict(rejected_after=f'{previous:08x}'))
  elif pc==0x6FCDB0:events.append(dict(assign=who(m.read32(sp+4))))
  elif pc==0x7099B5:events.append(dict(estimated_damage=u.reg_read(UC_X86_REG_EAX)))
 def written(uc,access,address,size,value,data):
  if address in field_addresses:writes.append(dict(field=field_addresses[address],size=size,value=who(value) if field_addresses[address]=='target' else value,pc=f'{u.reg_read(UC_X86_REG_EIP):08x}'))
 rows=[]
 for step in steps:
  frame=step.get('frame',173);u.mem_write(0xA8ED84,dwords(frame))
  layers=step.get('on_bridge',[0,0]);flags=step.get('flags',[0x100,0x100])
  for obj,layer,xy,flag in zip((source,target),layers,((10,20),(12,20)),flags):
   u.mem_write(obj+0x8C,bytes((layer,)));u.mem_write(obj+0x9C,dwords(xy[0]*256+128,xy[1]*256+128,1040 if layer else 624))
   u.mem_write(cells[xy]+0x140,dwords(flag));u.mem_write(cells[xy]+0xE4,dwords(0 if layer else obj,obj if layer else 0))
  if alternate:
   mode=step['alternative'];xy=(12,21) if mode=='other_cell' else (12,20)
   u.mem_write(alternate+0x9C,dwords(xy[0]*256+128,xy[1]*256+128,624));u.mem_write(alternate+0x8C,b'\0')
   if mode=='same_list':u.mem_write(target+0x30,dwords(alternate))
   else:u.mem_write(cells[xy]+0xE4,dwords(alternate))
   u.mem_write(cells[xy]+0x140,dwords(step.get('alternative_flags',0x100)))
  if step.get('clear_target'):m.invoke(0x6FCDB0,source,(0,))
  if 'rearm' in step:
   u.mem_write(source+0x2EC,dwords(step['rearm'][0]));u.mem_write(source+0x2F4,dwords(step['rearm'][1]))
  if 'health' in step:u.mem_write(target+0x6C,dwords(step['health']))
  if 'target_xyz' in step:u.mem_write(target+0x9C,dwords(*step['target_xyz']))
  if 'timer' in step:u.mem_write(source+0x180,dwords(*step['timer']))
  # Native709872/98A8 copies an inactive timer member from reused stack
  # scratch. Clear scratch before entry; intervening native calls overwrite it.
  # The copied word is recorded, not claimed as authoritative timer behavior.
  u.mem_write(SP-0x1000,bytes(0x2000));events.clear();writes.clear();lookups.clear();cell_lookups.clear();pending_returns.clear();pending_ops.clear();getter_stack.clear();instruction[0]=0;last.clear()
  before=state();rng_before_hex=bytes(u.mem_read(scenario+0x218,0x3F4)).hex();rng_before=hashlib.sha256(bytes.fromhex(rng_before_hex)).hexdigest()
  h1=u.hook_add(UC_HOOK_CODE,observe);h2=u.hook_add(UC_HOOK_MEM_WRITE,written)
  try:
   if direct:
    score=m.alloc(4);u.mem_write(SP,dwords(RET_MAGIC,0xBD,0x8046,0,target,score,-1,0xB0EA90));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,source)
    stop=run_checked(u,0x6F7CA0,RET_MAGIC,count=1000000)
   else:
    for reg,val in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,source),(UC_X86_REG_EBP,0)):u.reg_write(reg,val)
    stop=run_checked(u,BEGIN,END,count=1000000)
  finally:u.hook_del(h1);u.hook_del(h2)
  assert all(bytes(u.mem_read(a,b-a))==old for (a,b),old in zip(SLICES,immutable))
  assert u.reg_read(UC_X86_REG_ESP)==SP+(32 if direct else 0)
  assert state()['native_cursor']==before['native_cursor']
  rows.append(dict(supplied=step,before=before,after=state(),stop=f'{stop:08x}',events=list(events),writes=list(writes),lookups=list(lookups),cell_lookups=list(cell_lookups),
   rng_before_hex=rng_before_hex,rng_after_hex=bytes(u.mem_read(scenario+0x218,0x3F4)).hex(),rng_before=rng_before,rng_after=hashlib.sha256(bytes(u.mem_read(scenario+0x218,0x3F4))).hexdigest()))
 return dict(name=name,direct_evaluate=direct,inputs=inputs,steps=rows)

def generate():
 cases=[]
 for name,flags,layers in (
  ('same_ground',[0x100,0x100],[0,0]),('candidate_on_deck',[0x100,0x100],[0,1]),
  ('source_on_deck',[0x100,0x100],[1,0]),('same_deck',[0x100,0x100],[1,1]),
  ('source_only_structural',[0x100,0],[0,1]),('target_only_structural',[0,0x100],[0,1]),
  ('nonstructural_raw_flags',[0x200,0x400],[0,1]),
 ):
  cases.append(history(name,[dict(flags=flags,on_bridge=layers)]))
 cases.append(history('retained_ready_cleared_by_t58',[
  dict(frame=173),dict(frame=201,on_bridge=[0,1]),dict(frame=202,on_bridge=[0,1]),
  dict(frame=231,on_bridge=[0,1],clear_target=True)]))
 cases.append(history('retained_rearm_then_explicitly_cleared',[
  dict(frame=173),dict(frame=201,on_bridge=[0,1],rearm=[201,50]),
  dict(frame=202,on_bridge=[0,1]),dict(frame=231,on_bridge=[0,1],clear_target=True)]))
 for name,mode in (('deck_head_no_ground_fallback','ground'),('selected_list_no_tail_retry','same_list'),('later_other_cell_selected','other_cell')):
  cases.append(history(name,[dict(on_bridge=[0,1],rearm=[173,10],alternative=mode)]))
 cases.append(history('rearm_opposite_layers',[dict(on_bridge=[0,1],rearm=[173,10])]))
 cases.append(history('rearm_same_layer',[dict(rearm=[173,10])]))
 cases.append(history('rearm_source_deck_target_ground',[dict(on_bridge=[1,0],rearm=[173,10])]))
 cases.append(history('rearm_same_deck',[dict(on_bridge=[1,1],rearm=[173,10])]))
 cases.append(history('deck_retained_ready_cleared_by_t58',[
  dict(frame=173,on_bridge=[1,1]),dict(frame=201,on_bridge=[1,0]),
  dict(frame=202,on_bridge=[1,0]),dict(frame=231,on_bridge=[1,0],clear_target=True)]))
 cases.append(history('deck_retained_rearm_then_explicitly_cleared',[
  dict(frame=173,on_bridge=[1,1]),dict(frame=201,on_bridge=[1,0],rearm=[201,50]),
  dict(frame=202,on_bridge=[1,0]),dict(frame=231,on_bridge=[1,0],clear_target=True)]))
 cases.append(history('later_nonstructural_ground_cell_selected',[
  dict(on_bridge=[0,1],rearm=[173,10],alternative='other_cell',alternative_flags=0)]))
 cases.append(history('timer_not_due',[dict(timer=[173,0,28])]))
 cases.append(history('health_zero_after_real_probe',[dict(health=0)]))
 cases.append(history('missing_cell_health_zero_probe_side_effect',[dict(health=0,target_xyz=[10496,10496,624])],direct=True))
 return dict(cases=cases)

def metadata():
 return provenance(scope=__doc__,entry_points={'passive_ai_block':BEGIN,'stop_after_passive_publication':END,
  'passive_gate':0x709290,'scanner':0x709820,'greatest_threat':0x6F8DF0,'evaluate':0x6F7CA0,
  'unit_fire_error':0x740FD0,'bridge_gate':0x6F8682,'threat_score':0x70CD10,'assign_target':0x6FCDB0,'estimate_damage':0x6FDB80},
  assumptions=[
   'Starts after the native mission/earlier AI phases at6FA65A with ESI source and EBP0 supplied; ends at6FA6F5 before bomb/remaining AI. No full Unit scheduler.',
   'Physical empty FV/weapon prepare and actual UnitType ctor inherited from ifv_fire_coord. Added original retained FV LegalTarget/Armor/Strength/Immune/Insignificant, CanPassiveAquire/DistributedFire, GuardRange/AirRangeBonus/ThreatPosed and coefficient readers plus full HE readers; original General Process precedes each layer coefficient read. Original section admission controls the selected type blocks. Full Rules ctor+Process executes only selected physical General keys; unrelated type sections omitted.',
   'Consumed-field audit additionally executes retained LandTargeting, VHPScan, IsTrain, OpenTopped, MobileFire, OpportunityFire, SprayAttack, Natural, Invisible, NoAutoFire, HunterSeeker, Organic, AttackFriendlies, Unit DeployToFire/IsSimpleDeployer, SmallVisceroid/LargeVisceroid and NonVehicle blocks. Physical FV leaves these keys absent: MobileFire/LegalTarget remain1 and the other recorded fields0. Fixed FV ART omits FiringSyncFrame0/1; original loop747AA2..747B03 retains constructor-1/-1.',
   'EstimateDamage reaches Rules MaxDamage+16C8. Original constructor supplies1000; section admission526810 and retained block66CE2C..66CE57 consume physical CombatDamage MaxDamage10000. Later absent sections retain10000. The selected raw damage25 remains below both caps, so this input correction changes no retained pick/debit outcome. Adjacent stores preserve their native-produced current values; no VERA scalar initializes these fields.',
   'Selected HoverMissile original full reader retains physical Range1536, MinimumRange256, Damage25, ROF50; AAHeatSeeker2 full reader retains AA/AG. FV AirRangeBonus71479A..7147B4 reads physical4 into1024. Actual GreatestThreat fallback resolves range0 and radius11. Earlier joined traces with omitted AirRangeBonus/radius7 are superseded.',
   'Supplied zero-initialized on-map Guard Unit fields, actual vtables, original Drive construction with supplied owner link, actual Unit ammo/health initialization735592..7355C6 and respawn sentinel735416..73541C. No complete Unit ctor/Unlimbo or registry prefix.',
   'Supplied sparse real cells, raw bridge flags, native map size20x20 and AirTracker extent64x64/empty buckets, two hostile human Houses/countries, combat multipliers1, original non-null HouseType ctor arm4F643B..6455 sets house+1FB=1. No map/House/Country loading or bridge collapse/repair drivers.',
   'Each history starts actual Scenario Seed31. Passive scanner executes original ranged RNG, timer, traversal, real fire-error/evaluation/score/assignment/debit and outer passive byte. Read-only hooks observe; claimed runtime rejects allocator/TLS/archive substitution or native constructor/detach calls.',
   'Between-step OnBridge/XYZ/list/flag and frame changes are supplied. Clear-target control invokes original AssignTarget. Existing target retention and timer-not-due path are actual scanner outcomes.',
   'Pointer-valued ObjectGetCell returns and target-pointer writes are normalized to symbolic identities; allocator layout is not gameplay output. Height, score, RNG, native-ID cursor and timer numbers remain original numeric values.',
   'Original Object CRT table8141D8..81420C executes before runtime, establishing level104 and bridge416 for actual GetHeight/IsHighFlying. Read-only return observers record height/highFlying and ObjectGetCell parent callers.',
   'Original Techno scalar CRT table815040..815074 executes before runtime; separate B0EB34=104 and B0EB24=416 are consumed by InRange/GetFireError. Same-ground structural and ground-to-deck rearm cases reject at InRange6F81B8 before G27; deck-to-ground rearm reaches G27. Earlier zero-Techno-CRT exploratory results are invalid.',
   'Three ring controls supply another hostile FV: same-cell ground list under rejected deck head, deliberately mixed same deck-list tail with ground OnBridge/pose, or legal later cell12,21. They test original selection/retry behavior; the mixed tail is not ordinary admission.',
   'One direct full Evaluate control supplies a stale/missing-cell target with health0 specifically to observe the early GetFireError lookup side effect before later health rejection; not an ordinary valid ring-membership claim.',
   'Native inactive timer member comes from reused stack scratch (initially cleared, subsequently overwritten by real calls); its recorded pointer-like value is fixture-specific. Ordinary OnBridge0/1 only. Native x87 PC53/chop0E7F.',
  ],substitutions=['Preparation inherits lexical INI cache, bump allocator/delete, CRT TLS and archive input boundaries. Runtime claimed block uses original callees only; world/lifecycle/house inputs are supplied.'])

if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
