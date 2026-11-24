"""Original ordinary local-player scroll availability and retained disable bits."""
from pathlib import Path
import json
from tools.sidebar_oracle.geometry import machine,call,put32,SCRATCH
from tools.native_oracle import finish_vectors,provenance

def generate():
 result=[]
 for case in json.loads(Path(__file__).with_name('geometry.json').read_text())['cases']:
  for count in [0,case['globals'][10]//50*2,case['globals'][10]//50*2+1]:
   for initial in [0,1]:
    u=machine();put32(u,0xA8B230,SCRATCH);put32(u,SCRATCH+0x34B8,case['side'])
    put32(u,0xA83D4C,1);put32(u,0xAC1198,2)
    put32(u,0x886F9C,case['body'][3]);put32(u,0x886F94,case['body'][1]);put32(u,0xB0B4F8,case['globals'][7])
    for ptr in [0xB0B328,0xB0B408]:
     call(u,0x69DCF0,ecx=ptr);u.mem_write(ptr+0x1e,bytes([initial]))
    obj=SCRATCH+0x4000;put32(u,obj+0x539C,0);put32(u,obj+0x1598,count)
    call(u,0x6A6610,ecx=obj)
    result.append(dict(screen=case['screen'],side=case['side'],count=count,initial=initial,
      disabled=[u.mem_read(ptr+0x1e,1)[0] for ptr in [0xB0B328,0xB0B408]]))
 return dict(cases=result)

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
 scope='126 original ordinary local-player scroll availability cases over 21 native layouts.',
 assumptions=['PlayerPtr distinct from observer owner AC1198; selected strip0 with explicit item count.',
 'Layout inputs are outputs of geometry.py original execution; initial disabled state is staged.',
 'Original69DCF0 constructs both actual global SBGadget objects and original virtual invalidation executes.'],
 substitutions=[],entry_points={'gadget_ctor':0x69DCF0,'availability':0x6A6610}))
