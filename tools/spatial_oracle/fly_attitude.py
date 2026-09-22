"""Original Fly approach pitch producer and landing settle histories.

Executes original4CE2E5..4CE3C0 and4CE8CB..4CE910 ranges with real type
virtual dispatch. Runtime stack locals are supplied; no calls are replaced.
This is numeric/gameplay gate evidence, not full Process/Unload evidence.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_ESI,UC_X86_REG_EBP,UC_X86_REG_EDI
from tools.native_oracle import SCRATCH,run_checked,finish_vectors,provenance
from tools.spatial_oracle.aircraft_fire_location import Fixture,OWNER,TYPE,dwords
LOCO=SCRATCH+0x30000

def execute(case):
 f=Fixture({});u=f.u
 f.call(0x4CC9A0,LOCO,[])
 u.mem_write(LOCO+0xC,dwords(OWNER))
 u.mem_write(LOCO+0x48,struct.pack('<d',case.get('speed',0.5)))
 u.mem_write(LOCO+0x50,bytes([case.get('taking_off',False)]))
 u.mem_write(OWNER+0x6C,dwords(case.get('health',100)))
 u.mem_write(OWNER+0x2E8,struct.pack('<f',case.get('initial',0)))
 u.mem_write(TYPE+0x2F8,dwords(case['slowdown']))
 # Original degree conversion constant7F4FB8, rather than Python math.pi.
 radians=case['degrees']*struct.unpack('<d',u.mem_read(0x7F4FB8,8))[0]
 u.mem_write(TYPE+0x3B0,struct.pack('<d',radians))
 u.mem_write(f.sp+0x13,bytes([case.get('dropship',True)]))
 u.mem_write(f.sp+0x20,dwords(case['distance']))
 u.mem_write(f.sp+0x2C,dwords(TYPE))
 u.reg_write(UC_X86_REG_ESP,f.sp);u.reg_write(UC_X86_REG_ESI,LOCO);u.reg_write(UC_X86_REG_EBP,0)
 run_checked(u,0x4CE2E5,0x4CE3C0,count=1000)
 pitch=lambda:struct.unpack('<f',u.mem_read(OWNER+0x2E8,4))[0]
 history=[pitch()]
 for _ in range(100):
  if pitch()<=0:break
  u.reg_write(UC_X86_REG_ESI,LOCO);u.reg_write(UC_X86_REG_EDI,0)
  run_checked(u,0x4CE8CB,0x4CE910,count=1000)
  history.append(pitch())
 assert pitch()<=0
 return dict(input=case,radians=radians,history=history,settle_ticks=len(history)-1)

def generate():
 cases=[dict(degrees=a,slowdown=s,distance=d) for a in (0,20,45,90) for s in (1,500,50000) for d in sorted(set((0,s//2,s*3//5,s-1,s,s+1)))]
 cases += [dict(degrees=20,slowdown=500,distance=1,initial=0.25,**gate) for gate in ({'health':0},{'health':-1},{'speed':0},{'taking_off':True},{'dropship':False})]
 return [execute(c) for c in cases]

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
  entry_points={'approach_range':0x4CE2E5,'settle_range':0x4CE8CB},
  assumptions=['Real Aircraft/Type/Fly tables and ctor. Supplied nonnegative distance, SlowdownDistance, speed, health and stack-local IsDropship flag. Original degree constant. Native control word supplied by shared Fixture.', 'Landing decay range is entered at sampled height0; calling it is explicitly separate from full landing effects and admission.'],
  substitutions=[],scope='Approach writer gates and f32 pitch histories through zero. Compare deterministic SimFixed pitch and completion tick counts; does not prove full horizontal Process, AircraftUnload, animation or landing.'))
