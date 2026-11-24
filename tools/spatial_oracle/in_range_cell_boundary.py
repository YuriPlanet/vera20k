"""Original FV InRange cell query order with explicit sparse-map controls.

Executes full6F7220 on the physical FV/HoverMissile inputs and original Object
and Techno CRT initialization from bridge_target_composed. Map membership, dummy
fields and placement are supplied; no query/flight/range return is substituted.
"""
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_EAX,UC_X86_REG_ECX
from tools.native_oracle import finish_vectors,provenance
from tools.spatial_oracle import bridge_target_composed as c
from tools.spatial_oracle.building_body_rules import dwords


def execute(row):
 m,source,target,typ,weapon,cells,rules,inputs=c.setup();u=m.u
 # Add one explicitly supplied real Cell to the otherwise sparse inherited map.
 mapped=row['mapped_source'];cell=m.alloc(0x200)
 u.mem_write(cell,bytes(u.mem_read(cells[10,20],0x148)))
 u.mem_write(cell+0x24,struct.pack('<2h',*mapped['xy']))
 u.mem_write(cell+0x11B,bytes((mapped['level'],0)))
 u.mem_write(cell+0x140,dwords(mapped['flags']))
 table=m.read32(0x87F7E8+0x13C)
 u.mem_write(table+(mapped['xy'][1]*512+mapped['xy'][0])*4,dwords(cell))
 dummy=c.DUMMY;d=row['dummy']
 u.mem_write(dummy+0x24,struct.pack('<2h',*d['xy']))
 u.mem_write(dummy+0x11B,bytes((d['level'],0)));u.mem_write(dummy+0x140,dwords(d['flags']))
 u.mem_write(source+0x9C,dwords(*row['source_xyz']))
 u.mem_write(target+0x9C,dwords(*row['target_xyz']))
 u.mem_write(target+0x74,bytes((row['target_marked'],)))
 u.mem_write(target+0x8C,bytes((row['target_on_bridge'],)))
 retail_range=c.signed(m,weapon+0xB4);retail_minimum=c.signed(m,weapon+0xB8)
 u.mem_write(weapon+0xB4,dwords(row['range'],row['minimum_range']))
 out=m.alloc(12);u.mem_write(out,dwords(*row['source_xyz']))
 queries=[];ground_queries=[];flight=[];pending={};pending_ground={};adjusted=[];instruction=[0]
 def observe(uc,pc,n,data):
  instruction[0]+=1
  sp=u.reg_read(UC_X86_REG_ESP)
  if pc in pending:
   for event in pending.pop(pc):event['result']=u.reg_read(UC_X86_REG_EAX)&255
  if pc in pending_ground:
   for event in pending_ground.pop(pc):
    event['result']=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]
    event['dummy_xy']=list(struct.unpack('<2h',u.mem_read(dummy+0x24,4)))
  if pc==0x565730:
   queries.append(dict(caller=f'{m.read32(sp)-5:08x}',xyz=c.coord(m,m.read32(sp+4)),instruction=instruction[0]))
  elif pc==0x578080:
   event=dict(caller=f'{m.read32(sp)-5:08x}',xyz=c.coord(m,m.read32(sp+4)),instruction=instruction[0])
   ground_queries.append(event);pending_ground.setdefault(m.read32(sp),[]).append(event)
  elif pc in(0x5F6B60,0x5F6B90):
   event=dict(kind='low' if pc==0x5F6B60 else 'high',caller=f'{m.read32(sp):08x}',instruction=instruction[0])
   flight.append(event);pending.setdefault(m.read32(sp),[]).append(event)
  elif pc==0x6F7379:adjusted.append(c.coord(m,sp+0x20))
 h=u.hook_add(UC_HOOK_CODE,observe)
 try:answer=m.invoke(0x6F7220,source,(out,target,weapon))&255
 finally:u.hook_del(h)
 return dict(input=row,result=answer,adjusted_target_xyz=adjusted[-1] if adjusted else None,
  dummy_xy=list(struct.unpack('<2h',u.mem_read(dummy+0x24,4))),queries=queries,ground_queries=ground_queries,flight_getters=flight,
  source_type=m.string(typ+0x24),weapon=m.string(weapon+0x24),
  retail_weapon_range=retail_range,retail_weapon_minimum_range=retail_minimum,
  techno_height_globals=inputs['initial']['techno_height_globals'])


def generate():
 base=dict(source_xyz=[40*256+128,41*256+128,104],target_xyz=[41*256+128,41*256+128,104],
  target_marked=1,target_on_bridge=0,range=1280,minimum_range=0,
  dummy=dict(xy=[111,-222],level=1,flags=0),mapped_source=dict(xy=[40,41],level=1,flags=0))
 rows=[dict(base,name='missing_target_clear'),dict(base,name='missing_target_structural',dummy=dict(base['dummy'],flags=0x100)),
  dict(base,name='missing_source_underbridge',source_xyz=[42*256,42*256,0],target_xyz=[41*256+128,41*256+128,520],dummy=dict(base['dummy'],flags=0x100)),
  dict(base,name='minimum_range_early_return',minimum_range=1000),
  dict(base,name='unlimited_range_no_queries',range=-512),
  dict(base,name='unmarked_target_no_height_queries',target_marked=0)]
 return dict(rows=[execute(row)for row in rows])


def metadata():
 return provenance(scope=__doc__,entry_points={'in_range':0x6F7220,'target_high_call':0x6F7263,
  'target_low_call':0x6F732F,'target_snap_floor':0x6F7340,'target_snap_flags':0x6F7353,
  'source_flags':0x6F7601,'source_floor':0x6F7617},assumptions=[
  'Six full original FV/nonarcing HoverMissile range calls, actual constructor/retail-reader preparation and Object/Techno CRT groups inherited from composed.',
  'Supplied sparse Cell table adds real40,41 level1; target41,41 and optional source42,42 are missing and share the original Dummy. Dummy level1/slope0, flags and initial coordinate are explicit state.',
  'All rows explicitly supply range/minimum fields (baseline1280/0, MinimumRange1000 and unlimitedRange-512). They are not physical HoverMissile tuning; original pre-override retail readback1536/256 is separately recorded. Target mark/OnBridge/XYZ and source coordinate argument are supplied.',
  'No full map/object placement, movement, Building center/physical distinction, arcing or aircraft override claim.'
 ],substitutions=['Only original preparation input/allocation/OS boundaries inherited from composed; all claimed runtime instructions/callees run unchanged.'])

if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
