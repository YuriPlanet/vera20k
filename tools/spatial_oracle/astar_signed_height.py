"""Original AStar source/goal height producer joined to class7 goal-height tail.

The class7 verdict and post-hop current height (where specified) are supplied.
No full path/actor/rules or map-loading reachability claim for raw-byte controls.
"""
from pathlib import Path
import json,struct
from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC,run_checked,provenance,finish_vectors
from tools.spatial_oracle.tube_hierarchy import initialized_process_fixture
from tools.spatial_oracle.map_queries import dwords,packed
from tools.spatial_oracle.guarded_arena import Arena

def execute(row):
    u,sp=initialized_process_fixture();arena=Arena(u)
    alloc=lambda n:arena.allocate(n,'supplied bounded height fixture')
    table=alloc(0x40000*4);pf=alloc(0xD00);actor=alloc(0x800)
    u.mem_write(0x87F924,dwords(table));u.mem_write(actor,dwords(0x7EB058))
    u.mem_write(actor+0x8C,bytes([row['source_on_bridge']]))
    ptrs=[]
    for xy,level,flags in (([10,10],row['source_level_raw'],row['source_flags']),([11,9],row['goal_level_raw'],row['goal_flags'])):
        ptr=alloc(0x200);q=alloc(4);ptrs.append(q)
        u.mem_write(ptr+0x24,packed(*xy));u.mem_write(ptr+0x11B,bytes([level]));u.mem_write(ptr+0x140,dwords(flags))
        u.mem_write(table+(xy[1]*512+xy[0])*4,dwords(ptr));u.mem_write(q,packed(*xy))
    u.mem_write(sp,dwords(RET_MAGIC,*ptrs,actor,0,-1,0))
    u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,pf)
    run_checked(u,0x429A90,0x429B5A,count=50000,required_addresses=(0x429B23,0x429B57))
    produced=struct.unpack('<2i',u.mem_read(pf+0x30,8))
    if 'current_height_after_hops_supplied' in row:
        u.mem_write(pf+0x30,dwords(row['current_height_after_hops_supplied']))
    consumed=struct.unpack('<2i',u.mem_read(pf+0x30,8))
    goal_slot=table+(9*512+11)*4
    u.mem_write(sp,bytes(0x80));u.mem_write(sp+0x38,dwords(goal_slot))
    u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ESI,pf);u.reg_write(UC_X86_REG_EDI,goal_slot)
    end=run_checked(u,0x42A17D,(0x42A3DE,0x42A1A1),count=2000,required_addresses=(0x42A18B,0x42A198))
    arena.guard_check()
    return dict(input=row,produced_initial_current_height=produced[0],produced_goal_height=produced[1],consumed_current_height=consumed[0],consumed_goal_height=consumed[1],branch='blocked_goal_abort'if end==0x42A3DE else 'skip_neighbor',boundary=hex(end),signed_difference=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0],allocation_guards_intact=True)

def generate():
    rows=[]
    for start,goal,start_bridge,goal_bridge in ((10,10,False,False),(128,127,False,False),(127,128,False,False),(255,0,False,False),(0,255,False,False),(127,127,True,False),(127,127,False,True),(127,127,True,True),(128,128,True,False)):
        rows.append(dict(name=f'raw{start}_{goal}_bridge{int(start_bridge)}{int(goal_bridge)}',source_level_raw=start,goal_level_raw=goal,source_on_bridge=start_bridge,source_flags=256 if start_bridge else 0,goal_flags=256 if goal_bridge else 0))
    rows.append(dict(name='initial4_current10_goal10',source_level_raw=4,goal_level_raw=10,source_on_bridge=False,source_flags=0,goal_flags=0,current_height_after_hops_supplied=10))
    rows.append(dict(name='initial10_current14_goal10',source_level_raw=10,goal_level_raw=10,source_on_bridge=False,source_flags=0,goal_flags=0,current_height_after_hops_supplied=14))
    result=dict(schema_version=1,scope=__doc__,cases=[execute(row)for row in rows])
    return result

def metadata():
    return provenance(scope=__doc__,entry_points={'height_producer_begin':0x429A90,'height_producer_stop':0x429B5A,'class7_goal_tail':0x42A17D,'signed_absolute_difference':0x42A18B,'blocked_goal_abort':0x42A3DE,'skip':0x42A1A1},assumptions=['Supplied ordinary Infantry vtable and source OnBridge; two real Cells with raw unsigned storage bytes and source/goal flags. Original initial AStar instructions sign-extend ground bytes and add4 for deck heights into dword fields.','Stop before type getter/train handling, node creation and full main loop. These raw storage-domain controls do not prove map-loader or legal gameplay reachability.','Class7 admission has already occurred in the separate native neighbor corpus. Here a supplied goal-matching/non-amphibious local frame enters original class7 goal tail directly.','Two post-hop current heights are supplied after original initial-height production solely to distinguish consumed current height from initial source height.'],substitutions=[])

if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
