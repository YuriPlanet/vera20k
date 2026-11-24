"""Original Object low/high-flight queries through live native terrain height.

Unit vtables and physical map cells inherited from the FV preparation. Original
Object CRT initializers establish height constants; XYZ/mark/OnBridge are supplied.
No altitude, height, map getter or flight-query return is substituted.
"""
import hashlib,struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_ESP
from tools.native_oracle import finish_vectors,provenance
from tools.projectile_oracle.ifv_fire_coord import prepare
from tools.spatial_oracle.building_body_rules import dwords

def initialize_object_scalars(u, invoke):
 """Run original Object table8141D8's14CRT entries in table order.

 5F37C0..5F37F0 writes AC13C8;5F3860 writes AC13BC. These
 distinct Object scalars feed GetHeight/IsHighFlying, including Jumpjet's
 live owner-layer query. Map and Jumpjet startup do not initialize them.
 Reuse the caller's existing native invocation owner; supply no scalar.
 """
 initializers=struct.unpack('<14I',u.mem_read(0x8141D8,56))
 for address in initializers:invoke(address,0,())
 return initializers

def generate():
 m,source,typ,weapon,cells,initial=prepare();u=m.u
 initializers=initialize_object_scalars(u,m.invoke)
 cases=[]
 for marked in (0,1):
  for on_bridge in (0,1):
   for relative in (-1,0,207,208,209):
    cases.append(dict(name=f'marked{marked}_bridge{on_bridge}_relative{relative}',marked=marked,on_bridge=on_bridge,xyz=[2688,5248,624+416*on_bridge+relative],cell_level=6))
 cases += [dict(name='ground_level_changed',marked=1,on_bridge=0,xyz=[2688,5248,832],cell_level=7),
           dict(name='deck_ground_level_changed',marked=1,on_bridge=1,xyz=[2688,5248,1248],cell_level=7),
           dict(name='missing_cell_below_threshold',marked=1,on_bridge=0,xyz=[10496,10496,207],cell_level=6),
           dict(name='missing_cell_at_threshold',marked=1,on_bridge=0,xyz=[10496,10496,208],cell_level=6)]
 rows=[];scenario=m.read32(0xA8B230)
 for case in cases:
  u.mem_write(cells[10,20]+0x11B,bytes((case['cell_level'],0)));u.mem_write(cells[10,20]+0x140,dwords(0x100))
  u.mem_write(source+0x9C,dwords(*case['xyz']));u.mem_write(source+0x74,bytes((case['marked'],)));u.mem_write(source+0x8C,bytes((case['on_bridge'],)))
  u.mem_write(0xABDC50+0x24,struct.pack('<2h',111,-222));u.mem_write(0xABDC50+0x11B,b'\0\0')
  queries=[];cursor=m.read32(scenario+0x214);rng=bytes(u.mem_read(scenario+0x218,0x3F4))
  def observe(uc,pc,size,data):
   if pc==0x578080:
    p=m.read32(u.reg_read(UC_X86_REG_ESP)+4);queries.append(dict(xyz=list(struct.unpack('<3i',u.mem_read(p,12)))))
   elif pc in (0x65C7E0,0x65C780,0x68BCB0):raise AssertionError(hex(pc))
  h=u.hook_add(UC_HOOK_CODE,observe)
  try:
   height=struct.unpack('<i',dwords(m.invoke(0x5F5F40,source)))[0];height_queries=list(queries);queries.clear()
   low=m.invoke(0x5F6B60,source)&255;low_queries=list(queries);queries.clear()
   high=m.invoke(0x5F6B90,source)&255;high_queries=list(queries)
  finally:u.hook_del(h)
  assert m.read32(scenario+0x214)==cursor and bytes(u.mem_read(scenario+0x218,0x3F4))==rng
  rows.append(dict(input=case,height=height,low_flying=low,high_flying=high,height_queries=height_queries,low_queries=low_queries,high_queries=high_queries,dummy_xy=list(struct.unpack('<2h',u.mem_read(0xABDC50+0x24,4)))))
 return dict(object_initializers=[f'{a:08x}'for a in initializers],height_globals={f'{a:08x}':m.read32(a)for a in(0xAC13C8,0xAC13BC)},rows=rows)

def metadata():return provenance(scope=__doc__,entry_points={'object_crt_table':0x8141D8,'object_get_height':0x5F5F40,'map_floor':0x578080,'low_flying':0x5F6B60,'high_flying':0x5F6B90},assumptions=['24 controls:marked0/1,OnBridge0/1,relativeheights-1/0/207/208/209; two livegroundlevelchanges and two missing-cell cases.','Unit pose/lifecycle and sparse flat level6/7 cells are supplied. Original Object translation-unit14CRT entries execute; no native world loading/admission.','Direct GetHeight then low then high execute independently on unchanged object/terrain. Recorded query lists establish markedfalse skips GetHeight for low/high.','Uses native Map578080 and original Cell ground calculation; liveflags supplied100. No native bridge destruction/repair or aircraft/jumpjet virtual override claim.'],substitutions=['Preparation inherits selected physical FV/weapon INI caches and allocation boundaries. Claimed getter bodies execute unchanged without substituted calls.'])
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
