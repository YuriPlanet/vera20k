"""Original Map583820 bridge exit controls through the shared FV continuation."""
from pathlib import Path
import struct,json
from . import pursuit
from tools.spatial_oracle.building_body_rules import dwords
from tools.native_oracle import finish_vectors,provenance

HERE=Path(__file__).resolve().parent

def generate():
 q=pursuit.Continuation(dict(name='bridge_exit_setup',stage='healthy'))
 q.plane();q.construct(True);q.pathfinder();q.graphs();q.checkpoint();out=[]
 sentinel={hex(a):bytes(q.u.mem_read(a,4)).hex() for a in (0x89C278,0xABD480)}
 for name,axis,pick,rock,original in [
  *[(f'vertical_exit_{i}','vertical',i,False,(87,51))for i in range(6)],
  ('vertical_no_matching_zone','vertical',None,False,(87,51)),
  ('horizontal_positive','horizontal',0,False,(87,51)),
  ('horizontal_negative_side','horizontal',5,False,(87,51)),
  ('rock_positive_exit','vertical',3,True,(87,51)),
  ('fixed_stride_alias','vertical',0,False,(599,50)),
  ('ordinary_cell','ordinary',None,False,(87,51)),
  ('dummy_cell','ordinary',None,False,(-1,-1)),
 ]:
  q.reset();m=q.m;u=q.u
  horizontal=axis=='horizontal';d=(1,0)if horizontal else(0,1);p=(87+2*d[0],51+2*d[1]);n=(87-2*d[0],51-2*d[1]);perp=(0,1)if horizontal else(-1,0)
  choices=[p,(p[0]+perp[0],p[1]+perp[1]),(p[0]-perp[0],p[1]-perp[1]),n,(n[0]+perp[0],n[1]+perp[1]),(n[0]-perp[0],n[1]-perp[1])]
  facts=[]
  for y in range(48,55):
   for x in range(84,91):
    xy=(x,y);cp=q.cells[xy];flags=0
    if axis!='ordinary' and xy in [(87-d[0],51-d[1]),(87,51),(87+d[0],51+d[1])]:flags=0x900 if horizontal else 0x100
    tile=10000 if xy in(p,n)else 0;land=3 if rock and xy==p else 1
    u.mem_write(cp+0x38,dwords(tile));u.mem_write(cp+0xEC,dwords(land));u.mem_write(cp+0x140,dwords(flags));u.mem_write(cp+0x11B,bytes((0,0)))
    facts.append(dict(coord=xy,tile=tile,land=land,flags=flags,level=0,slope=0))
  u.mem_write(0xAA0E28,dwords(10000));u.mem_write(0xABAD1C,dwords(-1));plane=m.read32(0x87F858)
  for i,xy in enumerate(choices):u.mem_write(plane+(xy[1]*q.side+xy[0])*10,struct.pack('<H',900 if i==pick else 901))
  inp=m.alloc(8);u.mem_write(inp,struct.pack('<hh',*original));u.mem_write(inp+4,dwords(0xDEADBEEF));q.queries=[];q.flow=[];q.phase='exit_control'
  before={k:pursuit.proof.state(m,v)for k,v in q.resident.rngs.items()};m.invoke(0x583820,0x87F7E8,(inp+4,inp,0,900));assert before=={k:pursuit.proof.state(m,v)for k,v in q.resident.rngs.items()}
  result=list(struct.unpack('<hh',u.mem_read(inp+4,4)))
  out.append(dict(name=name,source=original,axis=axis,pick=pick,rock=rock,cells=facts,choices=choices,zone_ids=[900 if i==pick else 901 for i in range(6)],bounds=q.bounds,sentinels=sentinel,result=result,queries=q.queries))
  print(name,result,flush=True)
 q.close();return dict(controls=out)
def metadata():
 result=provenance(scope=__doc__,assumptions=['FV shared full native setup and constructors; supplied Cell flags, bridge-set indices, land, level and hierarchy IDs are focused synthetic inputs.'],substitutions=['Only supplied input state; no native return or instruction substitution.'],entry_points={'bridge_exit':0x583820})
 return result
if __name__=='__main__':finish_vectors(generate,HERE/'zone_exits.json',provenance=metadata)
