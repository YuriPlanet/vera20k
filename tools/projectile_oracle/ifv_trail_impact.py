"""Original full impact tail with native admitted-Unlimbo LineTrail producer.
This adds no launch/world admission proof; it executes the admitted tail explicitly.
"""
import importlib.util,json
from pathlib import Path
from unicorn.x86_const import *
from tools.native_oracle import edited_source,finish_vectors,provenance,require,run_checked
from tools.spatial_oracle.building_body_rules import SP,dwords
spec=importlib.util.spec_from_file_location('_trail_impact',Path(__file__).with_name('ifv_impact.py'));impact=importlib.util.module_from_spec(spec);spec.loader.exec_module(impact)
old=impact.launch.launch
last=None
def launch(*args,**kwargs):
 global last
 m,b,c,r=old(*args,**kwargs);u=m.u
 m.invoke(0x556940,0);u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,0x5569a0,0x5569d6)
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,0xa8eb60);run_checked(u,0x5fa350,0x5fa377)
 u.reg_write(UC_X86_REG_ESI,b);u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,0x5f514b,0x5f5210)
 last=(m,b,m.read32(b+0xa8));r['trail_produced']=dict(pointer=hex(last[2]),registry_count=m.read32(0xabcb88),owner=hex(m.read32(last[2]+4)))
 return m,b,c,r
impact.launch.launch=launch
exec(edited_source(impact.execute,("0x4690b0:'Detonate'", "0x556b30:'TrailDetach',0x5f5210:'DetachAll',0x5f528e:'DetachAllTrailCall',0x5f3d56:'ObjectDtorTrailCall',0x4690b0:'Detonate'")),impact.__dict__)
def generate():
 r=impact.execute();require('failure' not in r,str(r.get('failure')));m,b,t=last;r['trail_after']=dict(owner=hex(m.read32(t+4)),owner_slot=hex(m.read32(b+0xa8)),registry_count=m.read32(0xabcb88));return dict(row=r)
def metadata():
 return provenance(scope='Native admitted-Unlimbo LineTrail creation joined to selected Bullet impact and Conceal/DetachAll detachment before drain',assumptions=[
  'Reuses ifv_impact selected rise_live case. Explicitly enters already-admitted Unlimbo tail5F514B..5F5210 after launch: this is not prior world admission proof. Native DRAGON ARTUseLineTrail/RGB/decrement drive allocation/constructor and Bullet+A8 owner attachment.',
  'Original LineTrail zero-coordinate/registry initializer and Options constructor prefix establish DetailLevel2. Full Bullet impact/UnInit/expiry/Conceal/enqueue/drain/Release/destructor then execute.',
  'The corrected shared launch admission state makes UnInit/Conceal traverse DetachAll5F5280: original5F528E calls556B30 and clears Bullet+A8 before pending drain. ObjectDtor later sees the cleared slot and does not detach again. The earlier raw-copy launch boundary incorrectly retained constructor InLimbo and delayed this detach to ObjectDtor5F3D56. Detached trail remains registered with owner0 and Bullet slot0; no subsequent trail fade/draw or failed/re-Fire claim.',
 ],substitutions=['Inherited ifv_impact transport/world-admission boundaries; native trail allocation/constructor/detach execute without replacement.'],entry_points={'producer_tail':0x5f514b,'trail_ctor':0x556a20,'trail_detach':0x556b30,'dtor_call':0x5f3d56,'detach_all_call':0x5f528e,'drain':0x725c70})

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
