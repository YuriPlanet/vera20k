"""Original Techno GetFireError early center-cell query boundaries.

Physical FV weapon/HE readers and supplied Unit/House lifecycle come from the
composed fixture. Temporal-manager/limbo/lifted/cloak state are explicit controls,
not claimed ordinary empty-FV lifecycle producers.
"""
import struct,hashlib
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX,UC_X86_REG_EAX,UC_X86_REG_ESP
from tools.native_oracle import finish_vectors,provenance
from tools.spatial_oracle.building_body_rules import dwords
from tools.spatial_oracle import bridge_target_composed as composed

def execute(name):
 m,source,target,typ,weapon,cells,rules,inputs=composed.setup();u=m.u
 u.mem_write(source+0x9C,dwords(2688,5248,624));u.mem_write(target+0x9C,dwords(10496,10496,624))
 if name=='temporal_held_target':
  temporal=m.alloc(0x40);u.mem_write(temporal+0x28,dwords(target));u.mem_write(source+0x274,dwords(temporal))
 elif name=='firer_lifted':u.mem_write(source+0x6AD,b'\1')
 elif name=='target_limbo':u.mem_write(target+0x81,b'\1')
 elif name=='target_cloaked':u.mem_write(target+0x220,dwords(2))
 rows=[];sensors=[];center=[]
 def who(p):return 'dummy' if p==0xABDC50 else next((f'cell_{x}_{y}'for(x,y),v in cells.items()if v==p),f'{p:08x}')
 def observe(uc,pc,size,data):
  sp=u.reg_read(UC_X86_REG_ESP)
  if pc==0x565730:
   p=m.read32(sp+4);rows.append(dict(caller=f'{m.read32(sp)-5:08x}',xyz=list(struct.unpack('<3i',u.mem_read(p,12)))))
  elif pc==0x6FC19C:center.append(who(u.reg_read(UC_X86_REG_EAX)))
  elif pc==0x4870D0:sensors.append(dict(receiver=who(u.reg_read(UC_X86_REG_ECX)),house_index=m.read32(sp+4),caller=f'{m.read32(sp):08x}'))
 h=u.hook_add(UC_HOOK_CODE,observe)
 try:result=m.invoke(0x6FC0B0,source,(target,0,0))
 finally:u.hook_del(h)
 return dict(name=name,supplied=dict(target_xyz=[10496,10496,624],initial_dummy_xy=[111,-222],state=name),result=result,queries=rows,retained_center=center,sensors=sensors,dummy_xy=list(struct.unpack('<2h',u.mem_read(0xABDC50+0x24,4))))

def cell_centers():
 m,source,target,typ,weapon,cells,rules,inputs=composed.setup();u=m.u;out=m.alloc(12);rows=[]
 for name,xy,receiver in (('dummy_negative_center',[17,-19],0xABDC50),('dummy_alias_center',[-1,1],0xABDC50),('real_cell_center',[10,20],cells[10,20])):
  u.mem_write(0xABDC50+0x24,struct.pack('<2h',111,-222));u.mem_write(receiver+0x24,struct.pack('<2h',*xy))
  m.invoke(0x486840,receiver,(out,));xyz=list(struct.unpack('<3i',u.mem_read(out,12)))
  result=m.invoke(0x565730,0x87F7E8,(out,))
  rows.append(dict(name=name,input_xy=xy,receiver='dummy' if receiver==0xABDC50 else 'cell_10_20',center_xyz=xyz,identity='dummy' if result==0xABDC50 else 'cell_10_20',dummy_xy=list(struct.unpack('<2h',u.mem_read(0xABDC50+0x24,4)))))
 return rows

def generate():return dict(rows=[execute(n)for n in('temporal_held_target','firer_lifted','target_limbo','target_cloaked')],cell_centers=cell_centers())
def metadata():return provenance(scope=__doc__,entry_points={'techno_fire_error':0x6FC0B0,'target_center_lookup':0x6FC197,'sensor':0x4870D0,'cell_center':0x486840,'map_cell':0x565730},assumptions=['Full original TechnoGetFireError body, supplied inherited setup and explicit special lifecycle fields. No gate/query/sensor return substitutions.','Three separate Cell486840 thenMap565730 controls execute original bodies with a supplied retained Cell/dummy identity and coordinates; this leaf is not itself a whole FireError invocation.',
 'Target physical/missing-cellXYZ supplied; center identity retained by actual direct call. This FV receiver does not distinguish centerXY fromphysicalXY.'],substitutions=['Only preparation boundaries inherited from composed fixture.'])
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
