"""Original AStar node-height prefix, node-cost and selected-list controls.

Execute 42A460..42A523 and 429E54..429E7F on supplied scalar Cells, node
descriptors and current heights. No CanEnter, full route or loader reachability
is claimed. Rust bridge_walkable is deliberately metadata only: native code
tests Cell+140 structural0x100 and signed ground bytes.

The binary32 costs: AStar_create_node 42A460 through its return (g, the
Sqrt_Approx 4CAC40 heuristic and f, from a supplied parent, edge argument and
whole-cell offset to the goal); the main loop's edge, 429F72..429FA1 (the
AStar_compute_edge_cost 429830 call for a class, urgency and 0x40000 marker,
then FMUL Pathfinder+4, FADD the 81872C direction tiebreak, FSTP); and a closed
ground candidate's stored g against the current g plus 1.009, 429EE2..429F04.
"""
from fractions import Fraction
from pathlib import Path
import hashlib,json,struct
from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC,run_checked,provenance,finish_vectors
from tools.spatial_oracle.tube_hierarchy import initialized_process_fixture
from tools.spatial_oracle.map_queries import dwords,packed
from tools.spatial_oracle.guarded_arena import Arena


def execute(row):
    u,sp=initialized_process_fixture();arena=Arena(u)
    alloc=lambda n:arena.allocate(n,'supplied AStar scalar fragment input')
    pf=alloc(0xD00);node_pool=alloc(0x100004);descriptor_pool=alloc(0x180004)
    parent_cell=alloc(0x200);candidate_cell=alloc(0x200)
    parent_slot=alloc(4);candidate_slot=alloc(4);parent_descriptor=alloc(12);parent_node=alloc(16);goal=alloc(4)
    u.mem_write(pf+0x0C,dwords(descriptor_pool,node_pool));u.mem_write(pf+0x30,dwords(row['current_height']))
    for ptr,xy,key in ((parent_cell,[10,10],'parent'),(candidate_cell,[11,9],'candidate')):
        u.mem_write(ptr+0x24,packed(*xy));u.mem_write(ptr+0x11B,bytes([row[key+'_ground_raw']]))
        u.mem_write(ptr+0x140,dwords(row[key+'_flags']))
    u.mem_write(parent_slot,dwords(parent_cell));u.mem_write(candidate_slot,dwords(candidate_cell))
    u.mem_write(parent_descriptor,dwords(parent_slot,row['current_height'],0));u.mem_write(parent_node,dwords(parent_descriptor,0,0,1))
    u.mem_write(goal,packed(11,9))
    spans=((0x42A460,0x42A523),(0x429E54,0x429E7F))
    code=[bytes(u.mem_read(a,b-a))for a,b in spans]
    u.mem_write(sp,dwords(RET_MAGIC,0 if row.get('initial_node',False)else parent_node,candidate_slot,goal,0))
    u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,pf)
    run_checked(u,0x42A460,0x42A523,count=2000)
    descriptor=u.reg_read(UC_X86_REG_EDI)
    assert descriptor==descriptor_pool
    height=struct.unpack('<i',u.mem_read(descriptor+4,4))[0]
    counts=[struct.unpack('<I',u.mem_read(node_pool+0x100000,4))[0],struct.unpack('<I',u.mem_read(descriptor_pool+0x180000,4))[0]]
    assert counts==[1,1]
    u.mem_write(sp,bytes(0x80));u.mem_write(sp+0x60,b'\x7f')
    u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ESI,pf);u.reg_write(UC_X86_REG_EBX,candidate_cell)
    run_checked(u,0x429E54,0x429E7F,count=2000)
    selected=bytes(u.mem_read(sp+0x60,1))[0];assert selected in (0,1)
    assert code==[bytes(u.mem_read(a,b-a))for a,b in spans]
    arena.guard_check()
    return dict(input=row,node_height=height,selected_list='ground'if selected else 'deck',
                selected_list_native_local=selected,pool_counters=counts,code_unchanged=True,allocation_guards_intact=True)

def allocation_guard(direction, allocated):
    u,sp=initialized_process_fixture();arena=Arena(u)
    slot=arena.allocate(4,'supplied native candidate table slot')
    cell=arena.allocate(0x200,'allocated candidate storage')
    u.mem_write(slot,dwords(cell if allocated else 0))
    u.mem_write(sp+0x18,dwords(direction))
    u.mem_write(0xABDC50+0x24,packed(71,77))
    dummy_before=bytes(u.mem_read(0xABDC50,0x200))
    code=bytes(u.mem_read(0x429E19,0x429E27-0x429E19))
    u.reg_write(UC_X86_REG_EAX,slot);u.reg_write(UC_X86_REG_ESP,sp)
    end=run_checked(u,0x429E19,(0x429E27,0x42A1A1),count=100,
                    required_addresses=(0x429E19,0x429E1F))
    assert code==bytes(u.mem_read(0x429E19,len(code)))
    arena.guard_check()
    return dict(direction=direction,allocated=allocated,skipped=end==0x42A1A1,
                endpoint=hex(end),dummy_unchanged=bytes(u.mem_read(0xABDC50,0x200))==dummy_before,
                selected_pointer_matches=u.reg_read(UC_X86_REG_EBX)==(cell if allocated else 0),
                original_span_sha256=hashlib.sha256(code).hexdigest())

def rows():
    result=[]
    def add(name,pg,pflags,current,cg,cflags,**extra):
        result.append(dict(name=name,parent_ground_raw=pg,parent_flags=pflags,current_height=current,
                           candidate_ground_raw=cg,candidate_flags=cflags,**extra))
    add('ordinary_ground_hills10',10,0,10,10,0)
    for cg in (5,6,7,8,9,10,11):add(f'structural_candidate_raw{cg}_from_ground10',10,0,10,cg,0x100)
    add('candidate_walkable_only_raw6_from_ground10',10,0,10,6,0,candidate_rust_bridge_walkable=True)
    add('candidate_not_walkable_raw6_from_ground10',10,0,10,6,0,candidate_rust_bridge_walkable=False)
    add('structural_parent_on_deck_continues_structural_candidate',6,0x100,10,2,0x100)
    add('walkable_only_parent_does_not_force_deck_continuation',6,0,10,2,0x100,parent_rust_bridge_walkable=True)
    add('structural_parent_under_deck_keeps_candidate_ground',6,0x100,6,3,0x100)
    add('walkable_only_parent_allows_ground_to_deck_transition',6,0,6,3,0x100,parent_rust_bridge_walkable=True)
    add('structural_parent_deck_to_candidate_walkable_only',6,0x100,10,6,0,candidate_rust_bridge_walkable=True)
    add('raw127_structural_deck_is131',127,0x100,131,127,0x100)
    add('raw127_walkable_only_candidate_stays127',127,0x100,131,127,0,candidate_rust_bridge_walkable=True)
    add('raw128_structural_deck_is_minus124',128,0x100,-124,128,0x100)
    add('raw255_ground_to_raw252_structural_deck_is0',255,0,-1,252,0x100)
    add('signed_minus128_to127_is_large_selected_deck',128,0,-128,127,0x100)
    add('signed_minus1_to0_is_adjacent_selected_ground',255,0,-1,0,0x100)
    add('initial_node_preserves_supplied131',127,0,131,127,0,initial_node=True)
    return result

def f32_bits(value):
    return struct.unpack('<I',struct.pack('<f',value))[0]

def edge_cost(row):
    u,sp=initialized_process_fixture();arena=Arena(u)
    alloc=lambda n:arena.allocate(n,'supplied AStar edge-cost input')
    pf=alloc(0xD00);foot=alloc(0x800)
    current_cell=alloc(0x200);candidate_cell=alloc(0x200);current_slot=alloc(4);candidate_slot=alloc(4)
    # Pathfinder+4 is the constructor's 1.0f (42A6EB); +3C the urgency.
    u.mem_write(pf+0x04,dwords(0x3F800000));u.mem_write(pf+0x3C,dwords(row['urgency']))
    u.mem_write(current_cell+0x24,packed(10,10));u.mem_write(candidate_cell+0x24,packed(11,9))
    u.mem_write(candidate_cell+0x140,dwords(0x40000 if row['marker'] else 0))
    u.mem_write(current_slot,dwords(current_cell));u.mem_write(candidate_slot,dwords(candidate_cell))
    frame=sp-0x100;u.mem_write(frame,bytes(0x80))
    # [ESP+0x20] is the current cell's slot (0x429D8D), [ESP+0x24] the candidate's.
    u.mem_write(frame+0x18,dwords(row['direction']));u.mem_write(frame+0x20,dwords(current_slot,candidate_slot))
    u.mem_write(frame+0x60,b'\x01')
    code=bytes(u.mem_read(0x429F72,0x429FA1-0x429F72))
    u.reg_write(UC_X86_REG_ESP,frame);u.reg_write(UC_X86_REG_ESI,pf)
    u.reg_write(UC_X86_REG_EBX,row['cost_class']);u.reg_write(UC_X86_REG_EDI,foot)
    run_checked(u,0x429F72,0x429FA1,count=5000,required_addresses=(0x429830,))
    assert u.reg_read(UC_X86_REG_ESP)==frame
    assert code==bytes(u.mem_read(0x429F72,len(code)))
    arena.guard_check()
    edge=struct.unpack('<I',u.mem_read(frame+0x34,4))[0]
    return dict(input=row,edge_bits=f'{edge:08x}',code_unchanged=True,allocation_guards_intact=True)

def edge_rows():
    result=[]
    for cost_class in range(7):
        for urgency in ((0,1,2) if cost_class==2 else (0,)):
            for marker in (False,True):
                for direction in range(8):
                    result.append(dict(cost_class=cost_class,urgency=urgency,marker=marker,direction=direction))
    return result

def node_cost(row):
    u,sp=initialized_process_fixture();arena=Arena(u)
    alloc=lambda n:arena.allocate(n,'supplied AStar node-cost input')
    pf=alloc(0xD00);node_pool=alloc(0x100004);descriptor_pool=alloc(0x180004)
    parent_cell=alloc(0x200);candidate_cell=alloc(0x200)
    parent_slot=alloc(4);candidate_slot=alloc(4);parent_descriptor=alloc(12);parent_node=alloc(16);goal=alloc(4)
    u.mem_write(pf+0x0C,dwords(descriptor_pool,node_pool))
    (gx,gy),(dx,dy)=row['goal'],row['offset']
    u.mem_write(parent_cell+0x24,packed(gx+dx-1,gy+dy));u.mem_write(candidate_cell+0x24,packed(gx+dx,gy+dy))
    u.mem_write(parent_slot,dwords(parent_cell));u.mem_write(candidate_slot,dwords(candidate_cell))
    u.mem_write(parent_descriptor,dwords(parent_slot,0,0));u.mem_write(goal,packed(gx,gy))
    parent=0
    if row['parent_g_bits'] is not None:
        u.mem_write(parent_node,dwords(parent_descriptor,int(row['parent_g_bits'],16),0,row['parent_depth']))
        parent=parent_node
    code=bytes(u.mem_read(0x42A460,0x42A5A7-0x42A460))
    u.mem_write(sp,dwords(RET_MAGIC,parent,candidate_slot,goal,int(row['edge_bits'],16)))
    u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,pf)
    run_checked(u,0x42A460,RET_MAGIC,count=5000,required_addresses=(0x4CAC40,))
    node=u.reg_read(UC_X86_REG_EAX);assert node==node_pool
    g,f,depth=struct.unpack('<III',u.mem_read(node+4,12))
    assert code==bytes(u.mem_read(0x42A460,len(code)))
    arena.guard_check()
    return dict(input=row,g_bits=f'{g:08x}',f_bits=f'{f:08x}',depth=depth,code_unchanged=True,allocation_guards_intact=True)

def node_rows():
    # Every candidate cell stays at non-negative coordinates.
    goal=[520,520]
    gs=[0.0,1.001,2.006,23.087,1000.005,4013.37,65535.996,131071.99,16777215.0,1.5e8]
    edges=[1.001,1.005,1.008,4.002,60.007,1000.008,4000.004,3.0]
    offsets=[(0,0),(1,0),(0,1),(1,1),(2,1),(-3,4),(5,-12),(10,10),(100,37),(-255,0),(300,-400),
             (511,511),(-511,-1),(7,24),(127,128),(46,46)]
    result=[]
    def add(name,g,edge,offset,depth=5):
        result.append(dict(name=name,parent_g_bits=None if g is None else f'{f32_bits(g):08x}',parent_depth=depth,
                           edge_bits=f'{f32_bits(edge):08x}',goal=goal,offset=list(offset)))
    for offset in offsets[:8]:add(f'initial_{offset[0]}_{offset[1]}',None,0.0,offset,depth=0)
    for g in gs:
        for edge in edges:add(f'g_{g}_plus_{edge}',g,edge,(0,0))
    for g,edge in ((0.0,1.001),(23.087,1.005),(131071.99,4000.004)):
        for offset in offsets:add(f'f_{g}_{edge}_{offset[0]}_{offset[1]}',g,edge,offset)
    return result

def closed_retry(row):
    u,sp=initialized_process_fixture();arena=Arena(u)
    alloc=lambda n:arena.allocate(n,'supplied AStar closed-candidate input')
    pf=alloc(0xD00);stored=alloc(4);node=alloc(16)
    u.mem_write(pf+0x24,dwords(stored));u.mem_write(stored,dwords(int(row['stored_g_bits'],16)))
    u.mem_write(node+4,dwords(int(row['current_g_bits'],16)))
    frame=sp-0x100;u.mem_write(frame,bytes(0x80));u.mem_write(frame+0x14,dwords(node))
    code=bytes(u.mem_read(0x429EE2,0x429F04-0x429EE2))
    u.reg_write(UC_X86_REG_ESP,frame);u.reg_write(UC_X86_REG_ESI,pf);u.reg_write(UC_X86_REG_EBP,0)
    end=run_checked(u,0x429EE2,(0x42A1A1,0x429F37),count=100)
    assert code==bytes(u.mem_read(0x429EE2,len(code)))
    arena.guard_check()
    return dict(input=row,retried=end==0x429F37,endpoint=hex(end),code_unchanged=True,allocation_guards_intact=True)

def closed_rows():
    """Stored g at the binary32 values either side of current + 1.009."""
    threshold=Fraction(struct.unpack('<d',bytes.fromhex('3ff024dd2f1a9fbe')[::-1])[0])
    result=[]
    for current in (0.0,1.001,5.0234,100.5,12345.678,999999.9):
        bits=f32_bits(current)
        exact=Fraction(struct.unpack('<f',struct.pack('<I',bits))[0])+threshold
        below=f32_bits(float(exact))
        while Fraction(struct.unpack('<f',struct.pack('<I',below))[0])>=exact:below-=1
        while Fraction(struct.unpack('<f',struct.pack('<I',below+1))[0])<exact:below+=1
        for stored in (below-1,below,below+1,below+2,f32_bits(current),f32_bits(current+2.0)):
            result.append(dict(current_g_bits=f'{bits:08x}',stored_g_bits=f'{stored:08x}'))
    return result

def generate():
    result=dict(schema_version=1,scope=__doc__,cases=[execute(row)for row in rows()],
                allocation_guards=[allocation_guard(direction,allocated) for direction in (2,8) for allocated in (False,True)],
                edge_costs=[edge_cost(row) for row in edge_rows()],node_costs=[node_cost(row) for row in node_rows()],
                closed_retries=[closed_retry(row) for row in closed_rows()])
    return result

def metadata():
    return provenance(scope=__doc__,entry_points={'node_height_prefix_begin':0x42A460,'node_height_prefix_stop':0x42A523,'selected_list_begin':0x429E54,'selected_list_stop':0x429E7F,'candidate_pointer_guard':0x429E19,
                                                  'create_node':0x42A460,'sqrt_approx':0x4CAC40,'edge_cost_begin':0x429F72,'edge_cost':0x429830,'edge_cost_stop':0x429FA1,
                                                  'closed_retry_begin':0x429EE2,'closed_retry_skip':0x42A1A1,'closed_retry_continue':0x429F37},
                    assumptions=['Supplied Cells and parent node/descriptor data. Arena provides distinct preallocated original-size node/descriptor pools; original first allocation counters and descriptor writes execute.',
                                 'Node prefix stops before g/heuristic computation and node completion; selected-list fragment stops before hierarchy lookup. No concrete entry, closed-list admission, route reconstruction or full-search comparison.',
                                 'Raw ground bytes cover native storage-domain sign extension and deck+4. These controls do not claim map-loader or legal gameplay reachability for all values.',
                                 'Rust bridge_walkable hints are labels only and never appear in the native image. Structural Cell+140 bit0x100 alone changes the original branches.',
                                 'Candidate allocation controls supply a null or real table slot for compass2/tube8 and execute429E19 through the skip or allocated continuation. They stop before any Cell layer/zone/entry semantics and compare retained Dummy bytes unchanged.',
                                 'Node-cost rows execute the whole create_node with plain Cells (no structural flag) under the captured process control word: supplied parent g/depth (or no parent), binary32 edge argument and whole-cell offset to the goal. The parent and edge values are chosen inputs, not search states.',
                                 'Edge rows execute the main loop from its compute_edge_cost argument pushes through the binary32 store: supplied class, urgency and candidate 0x40000 marker, a candidate with no occupier (so class 2 keeps its base at urgency 0) and the dormant flank flag Pathfinder+1 zero. Pathfinder+4 is supplied as the constructor value 1.0f.',
                                 'Closed rows execute the ground-list test with supplied current and stored g; its deck twin at 429F17 is the same instructions on the +20 array.'],substitutions=[])

if __name__ == "__main__":
    import sys
    if '--hills-markers' in sys.argv:
        from tools.spatial_oracle import astar_hills_route, bridge_records
        hills = astar_hills_route.inputs
        finish_vectors(astar_hills_route.generate_markers, Path(__file__).with_name('astar_hills_markers.json'),
                       provenance=astar_hills_route.marker_metadata,
                       argv=[arg for arg in sys.argv[1:] if arg != '--hills-markers'],
                       source_paths={'height_owner':Path(__file__), 'hills_route':Path(astar_hills_route.__file__),
                                     'hills_inputs':Path(hills.__file__), 'mtnk_inputs':Path(hills.astar_mtnk_inputs.__file__),
                                     'navigation_inputs':Path(hills.navigation_inputs.__file__),
                                     'navigation':Path(hills.navigation.__file__),
                                     'bridge_constructor':Path(hills.bridge_constructor.__file__),
                                     'bridge_records':Path(bridge_records.__file__)})
    elif '--hills-route' in sys.argv:
        from tools.spatial_oracle import astar_hills_route, bridge_records
        hills = astar_hills_route.inputs
        finish_vectors(astar_hills_route.generate, Path(__file__).with_name('astar_hills_route.json'),
                       provenance=astar_hills_route.metadata,
                       argv=[arg for arg in sys.argv[1:] if arg != '--hills-route'],
                       source_paths={'height_owner':Path(__file__), 'hills_route':Path(astar_hills_route.__file__),
                                     'hills_inputs':Path(hills.__file__), 'mtnk_inputs':Path(hills.astar_mtnk_inputs.__file__),
                                     'navigation_inputs':Path(hills.navigation_inputs.__file__),
                                     'navigation':Path(hills.navigation.__file__),
                                     'bridge_constructor':Path(hills.bridge_constructor.__file__),
                                     'bridge_records':Path(bridge_records.__file__)})
    elif '--hills-inputs' in sys.argv:
        from tools.spatial_oracle import astar_hills_bridge_inputs
        finish_vectors(astar_hills_bridge_inputs.generate, Path(__file__).with_name('astar_hills_bridge_inputs.json'),
                       provenance=astar_hills_bridge_inputs.metadata,
                       argv=[arg for arg in sys.argv[1:] if arg != '--hills-inputs'],
                       source_paths={'height_owner':Path(__file__), 'hills_inputs':Path(astar_hills_bridge_inputs.__file__),
                                     'mtnk_inputs':Path(astar_hills_bridge_inputs.astar_mtnk_inputs.__file__),
                                     'navigation_inputs':Path(astar_hills_bridge_inputs.navigation_inputs.__file__),
                                     'navigation':Path(astar_hills_bridge_inputs.navigation.__file__),
                                     'bridge_constructor':Path(astar_hills_bridge_inputs.bridge_constructor.__file__)})
    elif '--threat-inputs' in sys.argv:
        from tools.spatial_oracle import astar_threat_inputs, astar_mtnk_inputs
        finish_vectors(astar_threat_inputs.generate, Path(__file__).with_name('astar_threat_inputs.json'),
                       provenance=astar_threat_inputs.metadata,
                       argv=[arg for arg in sys.argv[1:] if arg != '--threat-inputs'],
                       source_paths={'height_owner':Path(__file__), 'threat_inputs':Path(astar_threat_inputs.__file__),
                                     'mtnk_inputs':Path(astar_mtnk_inputs.__file__),
                                     'navigation_inputs':Path(astar_threat_inputs.navigation_inputs.__file__)})
    elif '--mtnk-inputs' in sys.argv:
        from tools.spatial_oracle import astar_mtnk_inputs
        finish_vectors(astar_mtnk_inputs.generate, Path(__file__).with_name('astar_mtnk_inputs.json'),
                       provenance=astar_mtnk_inputs.metadata,
                       argv=[arg for arg in sys.argv[1:] if arg != '--mtnk-inputs'],
                       source_paths={'height_owner':Path(__file__), 'mtnk_inputs':Path(astar_mtnk_inputs.__file__),
                                     'navigation_inputs':Path(astar_mtnk_inputs.navigation_inputs.__file__),
                                     'next_family':Path(astar_mtnk_inputs.next_family_native.__file__)})
    elif '--finishing' in sys.argv:
        from tools.spatial_oracle import (astar_path_finishing, map_queries, mapgen_range,
                                          unit_scatter_state, unit_source_scatter)
        finish_vectors(astar_path_finishing.generate, Path(__file__).with_name('astar_path_finishing.json'),
                       provenance=astar_path_finishing.metadata,
                       argv=[arg for arg in sys.argv[1:] if arg != '--finishing'],
                       source_paths={'height_owner':Path(__file__),'finishing':Path(astar_path_finishing.__file__),
                                     **{module.__name__: Path(module.__file__) for module in
                                        (unit_source_scatter, unit_scatter_state, mapgen_range, map_queries)}})
    else:
        finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
