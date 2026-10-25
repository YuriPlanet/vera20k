"""Original AreaDamage admission/receiver-dispatch receipt and IC isolation.
Receiver method is an explicit external-result boundary; AreaDamage runs whole.
"""
import hashlib,struct,sys
from pathlib import Path
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.projectile_oracle.ifv_select_anim import setup
from tools.projectile_oracle.ifv_impact import i32,xyz
from tools.native_oracle import RET_MAGIC,run_checked,finish_vectors,provenance
from tools.spatial_oracle.building_body_rules import SP,RULES,dwords
from tools.rmg_oracle.gen_rng_vectors import seeded_struct

def generate():
 m,b,wh,cell,obj,typ,initial=setup();u=m.u;m.invoke(0x561910,0)
 for x in range(6,25):
  for y in range(16,25):
   p=m.read32(0x26000000+4*(y*512+x));u.mem_write(p+0x38,dwords(-1));u.mem_write(p+0x44,dwords(-1));u.mem_write(p+0xe4,dwords(0));u.mem_write(p+0x140,dwords(0))
 u.mem_write(0x87f914,dwords(64,64));u.mem_write(0xabdc50+0x38,dwords(-1));u.mem_write(0xabdc50+0x44,dwords(-1));u.mem_write(0xa8ed84,dwords(100))
 scenario=m.read32(0xa8b230);position=m.alloc(12);u.mem_write(position,dwords(2688,5248,624));fresh=bytes(u.mem_read(wh,0x180));rows=[]
 specs=[dict(name='empty',empty=True),dict(name='dispatch_return0'),dict(name='dispatch_return1',receiver_result=1),dict(name='dispatch_return2',receiver_result=2),dict(name='dispatch_return4',receiver_result=4),dict(name='dead',alive=False),dict(name='zero_health',health=0),dict(name='limbo',limbo=True),dict(name='unmarked',marked=False),dict(name='zero_damage',damage=0),dict(name='scenario_no_damage',scenario_flags=0x20),dict(name='null_warhead',null_wh=True)]
 for distance in (0,84,85):
  for spread in ('0.5','0.5001'):
   specs.append(dict(name=f'ic_distance{distance}_spread{spread}',ic=True,distance=distance,spread=spread))
 specs += [dict(name='ic_kind1',ic=True,ic_kind=1),dict(name='ic_expired',ic=True,duration=0),dict(name='ic_elapsed_equal',ic=True,start=90,duration=10),dict(name='ic_elapsed_inside',ic=True,start=90,duration=11)]
 for overrides in specs:
  spec=dict(damage=25,health=100,alive=True,marked=True,limbo=False,ic=False,distance=0,spread='0.5',receiver_result=0,scenario_flags=0,start=100,duration=10,ic_kind=0);spec.update(overrides)
  u.mem_write(wh,fresh);m.rules_cache({'HE':{'CellSpread':spec['spread']}});m.invoke(0x75d3a0,wh,(RULES,))
  u.mem_write(cell+0xe4,dwords(0 if spec.get('empty') else obj));u.mem_write(obj,dwords(0x7f5c70));u.mem_write(obj+0x14,dwords(1));u.mem_write(obj+0x30,dwords(0));u.mem_write(obj+0x6c,dwords(spec['health']));u.mem_write(obj+0x90,bytes((spec['alive'],)));u.mem_write(obj+0x74,bytes((spec['marked'],)));u.mem_write(obj+0x81,bytes((spec['limbo'],)));u.mem_write(obj+0x9c,dwords(2688+spec['distance'],5248,624));u.mem_write(obj+0x18c,dwords(spec['start']));u.mem_write(obj+0x194,dwords(spec['duration'] if spec['ic'] else 0));u.mem_write(obj+0x1c4,dwords(spec['ic_kind']));u.mem_write(scenario,dwords(spec['scenario_flags']));seed=seeded_struct(31);u.mem_write(scenario+0x218,seed)
  dispatch=[];flags=[];events=[]
  def observe(uc,a,n,d):
   sp=u.reg_read(UC_X86_REG_ESP)
   if a==0x737c90:
    dispatch.append(dict(receiver=hex(u.reg_read(UC_X86_REG_ECX)),damage=i32(u,m.read32(sp+4)),distance=i32(u,sp+8),supplied_result=spec['receiver_result']))
    m.ret(spec['receiver_result'],28)
   if a in (0x489968,0x4894a4,0x489b12,0x48a47e):flags.append(dict(pc=hex(a),isolated=bool(u.mem_read(sp+0x17,1)[0]),dispatched=bool(u.mem_read(sp+0x1f,1)[0])))
   if a in (0x41bf40,0x489e87):events.append(hex(a))
   if a in (0x65c640,0x65c660,0x65c780,0x65c7e0):raise AssertionError(('unexpected_rng',hex(a)))
  h=u.hook_add(UC_HOOK_CODE,observe)
  try:
   u.mem_write(SP,dwords(RET_MAGIC,0,0 if spec.get('null_wh') else wh,0,0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,position);u.reg_write(UC_X86_REG_EDX,spec['damage']);run_checked(u,0x489280,RET_MAGIC,count=1000000)
  finally:u.hook_del(h)
  rows.append(dict(input=spec,result=u.reg_read(UC_X86_REG_EAX),dispatch=dispatch,flags=flags,events=events,rng_unchanged=bytes(u.mem_read(scenario+0x218,len(seed)))==seed))
 return dict(scope='Full original AreaDamage with supplied Unit occupancy/state and explicit ReceiveDamage result boundary; original collection/admission/IC timer/dispatch/continuation/receipt execute',initial=initial,rows=rows)

def metadata():
 return provenance(scope='Original AreaDamage returns0/1/2 and receiver dispatch versus IronCurtain isolation controls',assumptions=['Physical HE full reader, prepared flat level6 cells and original Unit vtable/getters. Original CellSpread reader consumes0.5/0.5001. Incoming health/flags/occupancy/XYZ/IC timer/kind are supplied; no Unit lifecycle claim.','Full AreaDamage489280 executes; original IC timer41BF40 and collector gates execute. No bridge overlay or damageable structural state. Seed31 remains unchanged. Result records receipt semantics, not actual receiver damage/kill behavior.'],substitutions=['Inherited reader archive/allocation/TLS boundaries. Unit ReceiveDamage737C90 is an explicit result boundary returning supplied0/1/2/4 with28bytecleanup; it does not mutate health. No AreaDamage, getter, timer, collection or receipt instruction replaced.'],entry_points={'area_damage':0x489280,'unit_receiver_boundary':0x737c90,'ic_timer':0x41bf40,'ground_isolation':0x489968,'air_isolation':0x4894a4,'return_isolated':0x489b3b,'return_receipt':0x48a47e})

def rocker_generate():
 """Post-receiver area numeric seam; retain existing physical reader/VM owners."""
 from tools.voxel_oracle.raster import render_native
 from tools.projectile_oracle.bridge_render_inputs import assets_root,lexical
 from tools.rmg_oracle.gen_rng_vectors import STRUCT_LEN
 root=assets_root()
 files={name:(root/name).read_bytes() for name in ('GTNK.VXL','GTNK.HVA','VOXELS.VPL')}
 loaded=render_native(files['GTNK.VXL'],files['GTNK.HVA'],files['VOXELS.VPL'],drive_pose=dict(frame=0))['drive']
 model={key:loaded[key] for key in ('model_header_hex','is_voxel','first_section_size','half_size_bits')}
 model['files']={name:dict(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest()) for name,raw in files.items()}
 m,_bullet,_he,_cell,_object,typ,_initial=setup();u=m.u
 m.invoke(0x49F2F0,0)
 weapon=m.invoke(0x772FA0,m.cstring('BlimpBomb'));layers=[];wh=None
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=root/name
  if not path.exists():
   assert name=='LANGRULE.INI';layers.append(dict(file=name,absent=True));continue
  raw=path.read_bytes();sections,_=lexical(raw,{'BlimpBomb','BlimpHE'})
  m.rules_cache(sections);m.invoke(0x772080,weapon,(RULES,));wh=m.read32(weapon+0xAC)
  m.invoke(0x75D3A0,wh,(RULES,))
  layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),
   selected={section:{key:value for key,value in fields.items() if key in ('Rocker','CellSpread','Warhead','Damage')}
             for section,fields in sections.items()}))
 assert m.string(wh+0x24)=='BlimpHE' and u.mem_read(wh+0x14E,1)==b'\x01'
 fresh_wh=bytes(u.mem_read(wh,0x180))
 header=m.alloc(32);u.mem_write(header,bytes.fromhex(model['model_header_hex']))
 u.mem_write(typ+0xB0,dwords(header))
 for offset,bits in zip((0x360,0x368),model['half_size_bits']):u.mem_write(typ+offset,struct.pack('<Q',int(bits,16)))
 weight_bits=f'{struct.unpack("<Q",u.mem_read(typ+0x370,8))[0]:016x}'
 assert weight_bits=='3ff0000000000000'
 source=m.alloc(0x1000);impact=m.alloc(12)
 targets=[m.alloc(0x1000),m.alloc(0x1000)]
 entries=[dict(name='center',cell=[10,20],position=[2688,5248,624],facing=0x6000),
          dict(name='neighbor',cell=[11,20],position=[2944,5248,624],facing=0x2000)]
 for target,entry in zip(targets,entries):
  u.mem_write(target,dwords(0x7F5C70));u.mem_write(target+0x6C4,dwords(typ))
  u.mem_write(target+0x388,dwords(entry['facing'],entry['facing']));u.mem_write(target+0x398,dwords(0))
  assert m.invoke(0x6F9E10,target)&255==1
 scenario=m.read32(0xA8B230)
 rngs={'main':0x886B88,'scenario':scenario+0x218,'mapgen':0xABE890}
 for seed,(name,address) in enumerate(rngs.items(),31):m.invoke(0x65C6D0,address,(seed,))
 rng_state=lambda:{name:bytes(u.mem_read(address,STRUCT_LEN)).hex() for name,address in rngs.items()}
 field_bits=lambda target:list(struct.unpack('<4I',u.mem_read(target+0x328,16)))
 normal=dict(damage=250,source=[2432,4992,1374],impact=[2694,5253,624],cell_spread='2',selected_layer='ground')
 cases=[dict(name='coupled_center_neighbor'),dict(name='vertical_center',source=[2688,5248,1374]),
  dict(name='absent_source',source=None),dict(name='zero_source_distance',source=[2688,5248,624]),
  dict(name='zero_spread',cell_spread='0'),dict(name='threshold30',damage=30),
  dict(name='threshold31',damage=31),dict(name='cap400',damage=400),dict(name='cap401',damage=401)]
 rows=[]
 for case in cases:
  spec=dict(normal,**case)
  u.mem_write(wh,fresh_wh)
  m.rules_cache({'BlimpHE':{'CellSpread':spec['cell_spread']}});m.invoke(0x75D3A0,wh,(RULES,))
  position=m.alloc(4)
  for x in range(7,14):
   for y in range(17,24):
    u.mem_write(position,struct.pack('<2h',x,y));cell=m.invoke(0x5657A0,0x87F7E8,(position,))
    u.mem_write(cell+0xE4,dwords(0));u.mem_write(cell+0xE8,dwords(0))
  for target,entry in zip(targets,entries):
   u.mem_write(target+0x9C,dwords(*entry['position']));u.mem_write(target+0x328,bytes(16));u.mem_write(target+0x30,dwords(0))
   u.mem_write(position,struct.pack('<2h',*entry['cell']));cell=m.invoke(0x5657A0,0x87F7E8,(position,))
   u.mem_write(cell+0xE4,dwords(target))
  if spec['source'] is not None:u.mem_write(source+0x9C,dwords(*spec['source']))
  u.mem_write(impact,dwords(*spec['impact']));u.mem_write(SP,bytes(0x300))
  # Exact pre-existing AreaDamage locals, at the declared post-receiver seam.
  ebp=SP+0x200;u.mem_write(ebp+8,dwords(source if spec['source'] is not None else 0,wh))
  u.mem_write(SP+0x18,struct.pack('<2h',10,20));u.mem_write(SP+0x1E,b'\x00')
  u.mem_write(SP+0x24,dwords(spec['damage'],impact))
  calls=[];stores=[];pending={};visits=[];before=rng_state()
  def observe(uc,pc,n,data):
   sp=u.reg_read(UC_X86_REG_ESP)
   if pc in pending:
    target,event=pending.pop(pc);event['after_bits']=field_bits(target)
   if pc==0x489C37:
    ptr=u.reg_read(UC_X86_REG_EDI)
    visits.append(next(entry['name'] for target,entry in zip(targets,entries) if target==ptr))
   if pc==0x70B280:
    target=u.reg_read(UC_X86_REG_ECX)
    event=dict(target=next(entry['name'] for pointer,entry in zip(targets,entries) if pointer==target),
     point=xyz(u,m.read32(sp+4)),force_bits=m.read32(sp+8),no_dampen=m.read32(sp+12),
     caller=f'{m.read32(sp):08X}',before_bits=field_bits(target))
    calls.append(event);pending[m.read32(sp)]=(target,event)
   if pc in (0x65C640,0x65C660,0x65C780,0x65C7E0):raise AssertionError(('unexpected_area_rocker_rng',hex(pc)))
  def write(uc,access,address,size,value,data):
   for target,entry in zip(targets,entries):
    if target+0x328<=address<target+0x338:
     stores.append(dict(target=entry['name'],offset=address-target,instruction=f'{u.reg_read(UC_X86_REG_EIP):08X}',bits=value,size=size))
  h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,write)
  try:
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,ebp)
   run_checked(u,0x489B49,0x489E87,count=1000000)
  finally:u.hook_del(h);u.hook_del(w)
  assert not pending
  after=rng_state();assert before==after
  rows.append(dict(input=spec,warhead_after_read=dict(name=m.string(wh+0x24),rocker=bool(u.mem_read(wh+0x14E,1)[0]),
   cell_spread_bits=f'{m.read32(wh+0x124):08x}'),rng_before=before,rng_after=after,
   force_f64_bits=f'{struct.unpack("<Q",u.mem_read(SP+0x88,8))[0]:016x}',visits=visits,calls=calls,stores=stores,
   final=[dict(target=entry['name'],bits=field_bits(target)) for target,entry in zip(targets,entries)]))
 return dict(schema=1,initial=dict(model_input=model,layers=layers,
  type_input=dict(name=m.string(typ+0x24),weight_bits=weight_bits),residents=entries,
  actor_boundary='Unit-shaped component inputs using original Unit vtable/getters; not constructors or placement'),rows=rows)


def rocker_metadata():
 return provenance(scope='Nine original post-receiver AreaDamage489B49..489E87 numeric controls through full70B280',
  assumptions=[
   'Existing IFV physical reader/setup/VM and original full BlimpHE reader consume RULESMD/LANGRULE when present/mode/map; row spread0 is an explicit native-reader override. Incoming damage/source/impact, fixed selected ground-layer local, ordered center/neighbor resident inputs and initial zero angles/rates are declared component priors. No full shot or receiver-lifecycle claim.',
   'Existing voxel_oracle.raster executes physical GTNK VXL/HVA/VPL loading/postpass. Produced header/error flag/half-size bytes are explicit model inputs in the Area VM, as in the joined Drone fixture; full original IsVoxel/Unit getters/ApplyRocker execute with native constructor Weight1. Full joined TypeReadVXL binding is not claimed.',
   'Coupled source displacement, source absent, source/candidate equality, pure vertical source at floor624+stockJumpjetHeight750, CellSpread0 and damage30/31/400/401 are compact numeric controls. Original source normalization/sqrt/ftol, threshold/cap and ordered four-word stores execute; no computed Rust or Python expected output.',
   'Full Main/Scenario/Mapgen structs are natively seeded31/32/33 before controls and unchanged across every selected numeric body. Receiver deaths, particle/Anim construction, bridge continuation, whole flight/frame scheduling and native scene pixels are excluded.'
  ],substitutions=['Inherited physical INI/file/archive/allocation/CRT transport only. The post-receiver frame and live resident/source/model priors are supplied; no source/getter/cast/Area numeric/ApplyRocker/RNG return is supplied.'],
  entry_points=dict(area_rocker=0x489B49,after_rocker=0x489E87,apply_rocker=0x70B280,cast=0x40DD70,
   warhead_reader=0x75D3A0,model_load=0x755DB0,is_voxel=0x6F9E10))


if __name__=='__main__':
 if '--rocker' in sys.argv[1:]:
  finish_vectors(rocker_generate,Path(__file__).with_name('area_rocker.json'),provenance=rocker_metadata,
   argv=[arg for arg in sys.argv[1:] if arg!='--rocker'])
 else:finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
