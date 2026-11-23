"""Bounded original Capture target admission and unchanged FindPath core goal.

Stops BEFORE AStar executes. Supplies object NavCom after the original Cell
setter; it does not claim object-order producer or successful AStar execution.
"""
from pathlib import Path
from tools.spatial_oracle import walk_failed_path as base
from tools.native_oracle import edited_source, finish_vectors, provenance, require

class CoreReached(Exception):
    pass

source = edited_source(
    base.query,
    (' before=state()', ''' # Supplied matching Building object destination/list after original Cell setter.
 u.mem_write(TARGET,dwords(0x7E3EBC))
 u.mem_write(TARGET+0x520,dwords(TYPE))
 u.mem_write(TARGET+0x9C,dwords(row['destination_x']*256+128,10*256+128,0))
 u.mem_write(ACTOR+0x5A4,dwords(TARGET))
 u.mem_write(CELL+0x200+0xE4,dwords(TARGET))
 u.mem_write(CELL+0x200+0x124,dwords(row['raw_bits']))
 u.mem_write(CELL+0x200+0x54,dwords(-1,-1))
''' + ' before=state()'),
    ('  recent.append(hex(address))', '  recent.append(hex(address))' + '''
  if address in (0x51C300,0x51C37D,0x51C71B,0x51C77F,0x4D3E0A,0x4D3CDD):
   events.append(['branch',hex(address)])
  if address==0x4D3A92:
   events.append(['destination_can_enter_result',read32(ACTOR+0x5A4),u.reg_read(UC_X86_REG_EAX)])
  if address==0x4CBBA0:
   goal=read32(sp+4)
   events.append(['core_goal_before_execution',list(struct.unpack('<hh',u.mem_read(goal,4)))])
   raise CoreReached()
'''),
    ('  call(0x75AEC0,LOCO,[0])', '''  try:
   call(0x75AEC0,LOCO,[0])
  except CoreReached:
   pass
  else:
   raise AssertionError('Expected core boundary was not reached')'''),
)
namespace = dict(vars(base), CoreReached=CoreReached)
exec(compile(source, '<capture-core-goal-query>', 'exec'), namespace)
query=namespace['query']

def generate():
    rows=[]
    for x in (11,13):
        for bits in (0,28,32):
            row=dict(mission=8,target=False,retries=10,native_find_path=True,astar_null=True,destination_x=x,frames=[100],doing=0,human=True,raw_bits=bits)
            result=query(row)
            events=result['phases'][0]['events']
            for event in (['destination_can_enter_result',base.TARGET,0],
                          ['branch','0x51c71b'], ['core_goal_before_execution',[x,10]]):
                require(event in events, f'{row}: missing {event}')
            rows.append(result)
    return rows

if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(scope='Six supplied matching Building/Capture NavCom states execute original Walk Process, Foot wrapper and Infantry CanEnter; stop before AStar execution and observe unchanged requested goal. No object-order producer, successful route, full Capture or Rust parity claim.', assumptions=['Inherited flat two-cell ordinary human Infantry fixture, original Cell setter followed by supplied matching Building NavCom and destination ground-object list. Building uses original7E3EBC vtable; all unused object state zero. Mission8 Capture and nonzero terrain speeds supplied. Target raw bits0/28/32 contrast; not a complete marked object population.'], substitutions=['Stops at4CBBA0 before core executes. Inherited OS Interlocked count seams only; no reached gameplay callable result is supplied. The inherited astar_null seam is never reached because the observer stops first.'], entry_points={'process': 7712448, 'wrapper': 5060896, 'can_enter': 5357456, 'core_boundary': 5028768}))
