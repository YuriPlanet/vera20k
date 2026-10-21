"""Expanded original guided-step controls; supplied incoming state, no Rust expectations."""
import json,struct,sys,hashlib
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.projectile_oracle import guided_step as g
from tools.native_oracle import run_checked,finish_vectors,provenance
from tools.spatial_oracle.building_body_rules import SP,RULES,dwords

def specs():
 yield dict(name='asymmetric_target',old=[2688,5329,800],velocity=[3.5,-1.25,.75])
 for name,v in [('zero',[0.,0.,0.]),('fractional',[.125,-.375,1.875]),('negative',[-3.5,-2.75,-.125]),('overspeed',[200.,17.,-5.])]:yield dict(name='velocity_'+name,velocity=v)
 for v in [-1,0,1,102]:yield dict(name='maximum_'+str(v),maximum=v,velocity=[102.,.25,-.125])
 for v in [-3,0,1,3]:yield dict(name='acceleration_'+str(v),type_keys={'Acceleration':str(v)},velocity=[.125,-.375,1.875])
 for n in [-1,0,2,3,4]:
  yield dict(name='locked_counter_'+str(n),type_keys={'CourseLockDuration':'3'},locked=True,counter=n)
 yield dict(name='unlocked_counter4',type_keys={'CourseLockDuration':'3'},locked=False,counter=4)
 for n in [9,10,11]:yield dict(name='closing_'+str(n),old=[2688,5329,800],velocity=[3.5,-1.25,.75],closing_count=n,closing_accum=.375)
 for key in ['Level','Airburst','VeryHigh']:
  yield dict(name='type_'+key,type_keys={key:'yes'},old=[2688,5329,1100],velocity=[.125,-.375,1.875])
 for name,height in [('below',1123),('equal',1124),('above',1125)]:yield dict(name='null_safety_'+name,target=None,old=[2688,5329,height],velocity=[3.5,-1.25,.75],safety=500)
 yield dict(name='null_low',target=None,old=[2688,5329,800],velocity=[3.5,-1.25,.75])
 yield dict(name='aircraft',target='aircraft',old=[2688,5329,1100],velocity=[3.5,-1.25,.75])
 yield dict(name='aircraft_level',target='aircraft',type_keys={'Level':'yes'},old=[2688,5329,1100],velocity=[3.5,-1.25,.75])

def generate():
 m,b,cells,initial=g.create();u=m.u;p=m.read32(b+0xac);r=m.read32(0x8871e0)
 fresh=bytes(u.mem_read(b,0x180));type_fresh=bytes(u.mem_read(p,0x400));rule_fresh=bytes(u.mem_read(r,0x2000))
 rows=[]
 air=m.alloc(0x800);u.mem_write(air,dwords(0x7e22a4));u.mem_write(air+0x9c,dwords(4224,5248,1300))
 for spec in specs():
  name=spec['name'];u.mem_write(b,fresh);u.mem_write(p,type_fresh);u.mem_write(r,rule_fresh)
  old=spec.get('old',[2688,5248,800]);velocity=spec.get('velocity',[1.,0.,0.]);target=spec.get('target',(16,20))
  if 'type_keys' in spec:m.read_layer(p,{'AAHeatSeeker2':spec['type_keys']})
  for c in cells.values():u.mem_write(c+0x140,dwords(0))
  target_ptr=0 if target is None else air if target=='aircraft' else cells[target]
  u.mem_write(b+0x10,dwords(100));u.mem_write(b+0x10c,dwords(target_ptr));u.mem_write(b+0x9c,dwords(*old));u.mem_write(b+0xe8,struct.pack('<3d',*velocity));u.mem_write(b+0x105,bytes((spec.get('locked',False),)));u.mem_write(b+0x108,dwords(spec.get('counter',0)));u.mem_write(b+0x118,dwords(spec.get('closing_count',0)));u.mem_write(b+0x120,struct.pack('<d',spec.get('closing_accum',0.)))
  if 'maximum' in spec:u.mem_write(b+0x110,dwords(spec['maximum']))
  if 'safety' in spec:u.mem_write(r+0x5a0,dwords(spec['safety']))
  reference=m.alloc(12);u.mem_write(reference,dwords(0,0,0))
  if target_ptr:
   getter=m.read32(m.read32(target_ptr)+0x58);m.invoke(getter,target_ptr,(reference,))
  u.mem_write(0xa8ed84,dwords(0));m.invoke(0x4e1130,b+0xb8,(b+0x9c,reference,2,0x7fffffff));u.mem_write(0xa8ed84,dwords(1));u.mem_write(SP,bytes(0x200));u.mem_write(SP+0x24,dwords(*old))
  events=[];snapshots=[];failure=None
  def observe(uc,a,n,d):
   if a in (0x65c640,0x65c660,0x65c780,0x65c7e0):raise AssertionError(('unexpected RNG',hex(a)))
   sp=u.reg_read(UC_X86_REG_ESP)
   if a in (0x565730,0x578080):events.append(dict(pc=hex(a),query=g.xyz(u,m.read32(sp+4))))
   if a==0x5b20f0:events.append(dict(pc=hex(a),target=g.xyz(u,m.read32(sp+4)),word=struct.unpack('<H',u.mem_read(m.read32(sp+8),2))[0],flags=[m.read32(sp+12+i*4)&255 for i in range(4)]))
   if a in (0x466b0c,0x466d36,0x466db1,0x466eb6,0x466f2c,0x466fa7,0x467032,0x467b7a):snapshots.append(dict(pc=hex(a),candidate=g.xyz(u,SP+0x24),target_local=g.xyz(u,SP+0x30),old_local=g.xyz(u,SP+0x44),velocity=g.vec(u,b+0xe8),closing_count=g.i32(u,b+0x118),closing_bits=bytes(u.mem_read(b+0x120,8)).hex()))
  h=u.hook_add(UC_HOOK_CODE,observe)
  try:
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,b);run_checked(u,0x4668bd,0x467b7a,count=500000)
  except Exception as e:failure=str(e)
  finally:u.hook_del(h)
  row=dict(input=spec,target_coord=g.xyz(u,reference),candidate=g.xyz(u,SP+0x24),velocity=g.vec(u,b+0xe8),mode=g.i32(u,SP+0x20),impact=u.mem_read(SP+0x18,1)[0],locked=bool(u.mem_read(b+0x105,1)[0]),counter=g.i32(u,b+0x108),closing_count=g.i32(u,b+0x118),closing_bits=bytes(u.mem_read(b+0x120,8)).hex(),events=events,snapshots=snapshots)
  assert failure is None,(name,failure)
  row['prepared']=dict(old=old,velocity=velocity,target_kind='none' if target is None else 'aircraft' if target=='aircraft' else 'cell',target_cell=list(target) if isinstance(target,tuple) else None,unique_id=100,binary_frame=1,level=6,slope=0,maximum_speed=g.i32(u,b+0x110),acceleration=g.i32(u,p+0x2d0),course_lock_duration=g.i32(u,p+0x2e0),airburst=bool(u.mem_read(p+0x294,1)[0]),very_high=bool(u.mem_read(p+0x299,1)[0]),level_flight=bool(u.mem_read(p+0x29d,1)[0]),safety_altitude=g.i32(u,r+0x5a0),missile_rot_var_bits=bytes(u.mem_read(r+0x598,8)).hex())
  rows.append(row)
 return dict(initial=initial,scope='Expanded supplied incoming guided-step controls; full original4668BD..467B7A and reached native getters/homing/floor. Type overrides use full original reader, runtime counters/velocities/target lifecycle are supplied. No world commit/launch or damage in this corpus.',rows=rows)
def metadata():
 return provenance(scope='Thirty-one expanded original guided-flight controls before coordinate commit',assumptions=[
  'Full original4668BD..467B7A executes, including reached HomingTrack5B20F0, native target getters, Cell lookups and floor. Each row starts from the same original Bullet constructor/configure state and native physical HoverMissile/AAHeatSeeker2 reader setup in guided_step.create.',
  'Physical RULESMD, optional LANGRULE, MPBattleMD and Hills layer hashes and constructor initializers are recorded under initial. The shared create fixture is not the full Rules Process chronology; guided ROT60 speed102 is independently bounded.',
  'Incoming signed XYZ, binary64 velocity, counters, latch, maximum speed and binary-frame/native-ID are supplied. Native type overrides run the full BulletType reader. Mapped flat level6 cells, null source and an Aircraft-shaped target with original vtable7E22A4 and supplied XYZ are fixture inputs, not a native Aircraft lifecycle claim.',
  'Target-null controls use the native zero-coordinate fallback; query order, cached target/old coordinate locals and raw closing accumulator bits are retained. Safety controls bound old-height499/500/501 for SafetyAltitude500.',
  'Original x87 state is53-bit/truncate0E7F. No Rust/fixed-point expectation is used; no world commit, collision effect, renderer, launch or scheduler claim.',
 ],substitutions=[
  'Inherited BulletReader supplies physical lexical INI caches, allocation/delete/CRT TLS/archive services. Observation hooks record original calls/state and reject unexpected RNG; no guided instruction or callee is replaced.',
 ],entry_points={'bullet_ctor':0x466380,'configure':0x4664c0,'guided_step':0x4668bd,'homing':0x5b20f0,'contact':0x467032,'precommit_boundary':0x467b7a,'proximity_setup':0x4e1130,'bullet_type_reader':0x46bee0,'aircraft_vtable_writer':0x413d87})

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
