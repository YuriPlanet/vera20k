"""Original successful A* tail, from reconstruction through both finishing passes.

Companion to astar_structural_height's --finishing mode. Supplied parent chains
and scalar Unit/Cell states are controls, not full A* or MTNK Scenario evidence.
No concrete admission, threat, path, height or RNG result is substituted.
"""
from pathlib import Path
from types import SimpleNamespace
import hashlib
import json
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import run_checked, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.unit_source_scatter import make_source_fixture, CELLS, ENTRY, QUEUE, SET, TABLE
from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE, SP
from tools.spatial_oracle.mapgen_range import Machine

STORAGE = 0x24000000
HOUSE = STORAGE + 0x10000
DELTAS = ((0,-1),(1,-1),(1,0),(1,1),(0,1),(-1,1),(-1,0),(-1,-1))
DUMMY = 0xABDC50
FROZEN_FIRST33_SHA256 = 'b5a9a3c9917110eb79fe11f6759d7ba5cf183253e2510f92b43eea6a9e756273'


def cases():
    out = []
    def add(name, directions, **options):
        row=dict(name=name,start=[10,10],directions=directions)
        row.update(options)
        out.append(row)
    for direction in (1,3,5,7):
        add(f'corner_{direction}', [direction]*3 + [(direction+2)&7]*3)
    add('cardinal_staircase', [2,4]*5)
    add('axis_reversal', [2,2,4,6,4,2,2])
    add('second_pass_rewrites_interior', [3,3,2,1,1,2,3,3])
    for name, cells, heights in (
        ('deck_continuation', [[11,10,0,0x300],[12,10,0,0x300],[13,10,0,0x300],[14,10,0,0x300]], [4]*5),
        ('underpass_continuation', [[11,10,0,0x300],[12,10,0,0x300],[13,10,0,0x300],[14,10,0,0x300]], [0]*5),
        ('ramp_transition', [[11,10,4,0],[12,10,0,0x300],[13,10,0,0x300],[14,10,0,0x300]], [4]*5),
    ):
        add(name, [1,1,3,3], cells=cells, heights=heights)
    add('corner_first_candidate_marker', [1,1,3,3], cells=[[11,10,0,0x40000]])
    add('corner_goal_marker', [1,3], cells=[[12,10,0,0x40000]])
    add('corner_blocked_terrain', [1,1,3,3], blocked_terrain=[[11,10]])
    add('corner_ground_occupation', [1,1,3,3], raw=[[11,10,0x20,0]])
    add('corner_deck_occupation', [1,1,3,3], cells=[[11,10,0,0x300]], raw=[[11,10,0,0x20]], heights=[4]*5)
    add('deck_ignores_ground_occupation', [1,1,3,3],
        cells=[[x,10,0,0x300]for x in range(11,15)],raw=[[11,10,0x20,0]],heights=[4]*5)
    add('underpass_ignores_deck_occupation', [1,1,3,3],
        cells=[[x,10,0,0x300]for x in range(11,15)],raw=[[11,10,0,0x20]])
    add('structural_without_walkable_deck', [1,1,3,3],
        cells=[[x,10,0,0x100]for x in range(11,15)],heights=[4]*5)
    add('first_pass_height_drops_to_candidate_ground', [1,1,3,3],
        cells=[[11,10,0,0x300],[12,10,1,0],[13,10,1,0],[14,10,1,0]],heights=[4]*5)
    for coefficient, threat in ((0,100),(0.005,100),(0.01,100),(1,1)):
        add(f'corner_threat_{coefficient}_{threat}', [1,1,3,3], coefficient=coefficient, threat=threat)
    for coefficient in (0,0.00001,0.000011,0.001,0.01):
        add(f'straight_threat_{coefficient}',[2,4]*5,coefficient=coefficient,threat=10)
    for length in (19,20,21,25):
        add(f'window_{length}', [2,4]*(length//2)+([2] if length&1 else []))
    # Reconstruction carries signed parent heights, including native raw-domain
    # controls. They do not assert map-loader reachability for these values.
    add('signed_parent_heights', [2,2,2], heights=[-128,-124,127,131])
    def extended(name, directions, **options):
        add(name, directions, extended=True, **options)
    tube_nodes=[[10,10],[20,20],[21,19],[22,18],[23,19],[24,20]]
    extended('tube8_valid_exit_then_corner', [8,1,1,3,3], nodes=tube_nodes,
             tubes=[dict(cell=[10,10],exit=[20,20],direction=3)])
    extended('tube8_missing_index_resets_zero_then_corner', [8,1,1,3,3], nodes=tube_nodes)
    extended('tube8_dummy_missing_index_resets_zero_then_corner', [8,1,1,3,3], nodes=tube_nodes,
             missing_cells=[[10,10]])
    extended('tube8_after_smoothed_corner', [1,3,8,1,3],
             nodes=[[10,10],[11,9],[12,10],[20,20],[21,19],[22,20]],
             tubes=[dict(cell=[12,10],exit=[20,20],direction=3)])
    extended('dummy_retained_candidate_changes_during_entry', [1,1,3,3], start=[31,10])
    extended('dummy_marker_still_performs_failed_extra_lookup', [1,1,3,3], start=[31,10],
             dummy=dict(level=0,flags=0x40000))
    extended('dummy_signed_ground_and_structural_lift', [1,1,3,3], start=[31,10],
             dummy=dict(level=255,flags=0x300),heights=[3]*5)
    # Endpoint/descriptor mismatch is explicit scalar-input coverage: it lets
    # the original finishing walk cross the signed-word boundary while the
    # separately supplied reconstruction directions remain ordinary compass
    # steps. This is not an A* producer/reachability claim.
    for x in (32766,-32768):
        extended(f'tube8_supplied_exit_x{x}_packed_walk', [8,1,1,3,3], nodes=tube_nodes,
                 tubes=[dict(cell=[10,10],exit=[x,10],direction=3)])
    for bits in (0x3ee4f8b588e368f0,0x3ee4f8b588e368f2):
        coefficient=struct.unpack('<d',struct.pack('<Q',bits))[0]
        extended(f'coefficient_threshold_neighbor_{bits:016x}',[2,4]*5,coefficient=coefficient,
                 coefficient_bits=f'{bits:016x}',threat=10)
    for bits in (0x3f847ae147ae147a,0x3f847ae147ae147c):
        coefficient=struct.unpack('<d',struct.pack('<Q',bits))[0]
        extended(f'corner_product_threshold_neighbor_{bits:016x}',[1,1,3,3],coefficient=coefficient,
                 coefficient_bits=f'{bits:016x}',threat=100)
    tube_staircase=[[10,10],[20,20]]
    for direction in [2,4]*4:
        dx,dy=DELTAS[direction]
        tube_staircase.append([tube_staircase[-1][0]+dx,tube_staircase[-1][1]+dy])
    extended('tube8_resets_second_pass_staircase_origin',[8]+[2,4]*4,nodes=tube_staircase,
             tubes=[dict(cell=[10,10],exit=[20,20],direction=3)])
    # Native-executed reversal route exposes the final tail branch: at .001*10
    # its three-cell candidate is accepted, while a four-cell candidate fails
    # both diagonal-first and cardinal-first attempts. Interior attempts occur
    # beforehand and are deliberately retained as part of this caller chain.
    tail=[6,6,2,6,6,2,6,2,2,0,2,4,6,2,4,2,4,4,4]
    extended('tail_three_positive_threat_cells',tail,start=[15,15],coefficient=.001,threat=10)
    extended('tail_four_positive_threat_cells',tail+[4],start=[15,15],coefficient=.001,threat=10)
    for bits in (0x3f50624dd2f1a9fb,0x3f50624dd2f1a9fd):
        coefficient=struct.unpack('<d',struct.pack('<Q',bits))[0]
        extended(f'straight_product_threshold_neighbor_{bits:016x}',[2,4]*5,
                 coefficient=coefficient,coefficient_bits=f'{bits:016x}',threat=10)
    return out


def execute(row):
    u, call, read32 = make_source_fixture(dict(row, live_entry=True))
    u.mem_map(STORAGE, 0x40000)
    startup = Machine.startup(SimpleNamespace(u=u))
    u.mem_write(TYPE, dwords(0x7F6218))
    u.mem_write(ACTOR+0x14, dwords(5))
    u.mem_write(ACTOR+0x21C, dwords(HOUSE))
    if 'coefficient_bits' in row:
        assert struct.pack('<d',row['coefficient'])==struct.pack('<Q',int(row['coefficient_bits'],16))
    u.mem_write(ACTOR+0x530, struct.pack('<d', row.get('coefficient',0)))
    u.mem_write(HOUSE+0x30, dwords(0))
    threat = row.get('threat',0)
    u.mem_write(HOUSE+0x59F0, dwords(*([threat]*(130*130))))
    u.mem_write(0xA8E9A0, b'\x01')
    # Exclude only CRT atexit registration for the function-local static. The
    # original reconstruction body, including Math__ftol, executes unchanged.
    u.mem_write(0x89A300, b'\x01')
    for address in (0x886B88, read32(0xA8B230)+0x218, 0xABE890):
        call(0x65C6D0,address,[1])
    rngs = (0x886B88, read32(0xA8B230)+0x218, 0xABE890)
    before_rng = [bytes(u.mem_read(p,1012)).hex() for p in rngs]
    coords = [tuple(c)for c in row['nodes']] if 'nodes' in row else [tuple(row['start'])]
    if 'nodes' not in row:
        for direction in row['directions']:
            dx,dy=DELTAS[direction]
            coords.append(((coords[-1][0]+dx)&65535,(coords[-1][1]+dy)&65535))
    assert len(coords)==len(row['directions'])+1
    if row.get('extended'):
        for i in range(32*32):
            u.mem_write(CELLS+i*0x200+0x116,struct.pack('<h',-1))
        dummy=row.get('dummy',{})
        # Explicit empty Cell scalar fixture (including -1 overlay/object
        # indexes), rather than uninitialized executable .bss constructor data.
        u.mem_write(DUMMY,bytes(u.mem_read(CELLS,0x200)))
        u.mem_write(DUMMY+0x116,struct.pack('<h',-1))
        u.mem_write(DUMMY+0x11B,bytes([dummy.get('level',0)&255]))
        u.mem_write(DUMMY+0x140,dwords(dummy.get('flags',0)))
        u.mem_write(DUMMY+0x24,packed(0,0))
        for x,y in row.get('missing_cells',[]):
            u.mem_write(TABLE+(y*512+x)*4,dwords(0))
        tubes=row.get('tubes',[])
        u.mem_write(0x8B413C,dwords(STORAGE+0x9000))
        u.mem_write(0x8B4148,dwords(len(tubes)))
        for i,tube in enumerate(tubes):
            cell=CELLS+(tube['cell'][1]*32+tube['cell'][0])*0x200
            u.mem_write(cell+0x116,struct.pack('<h',i))
            pointer=STORAGE+0xA000+i*0x200
            u.mem_write(STORAGE+0x9000+i*4,dwords(pointer))
            u.mem_write(pointer+0x24,packed(*tube['cell'])+packed(*tube['exit'])+dwords(tube['direction']))
    heights = row.get('heights',[0]*len(coords))
    assert len(heights)==len(coords) and 2<=len(coords)<=64
    guards=[]
    def supply(offset, raw):
        p=STORAGE+offset
        u.mem_write(p-16,b'\xa5'*16);u.mem_write(p,raw);u.mem_write(p+len(raw),b'\xa5'*16)
        guards.append((p,len(raw)))
        return p
    node_cells=[]
    for i,(x,y) in enumerate(coords):
        if 0<=x<32 and 0<=y<32:
            node_cells.append(CELLS+(y*32+x)*0x200)
        else:
            # Distinct guarded descriptor-only scalar Cells; Map's sparse
            # table is deliberately unchanged for these coordinates.
            raw=bytearray(0x200);struct.pack_into('<I',raw,0,0x7E4EEC)
            struct.pack_into('<hh',raw,0x24,x if x<32768 else x-65536,y if y<32768 else y-65536)
            node_cells.append(supply(0x30000+i*0x220,bytes(raw)))
    slots=supply(0x1000,dwords(*node_cells))
    descriptors=supply(0x2000,b''.join(dwords(slots+i*4,height,STORAGE+0x2000+(i-1)*12 if i else 0) for i,height in enumerate(heights)))
    node=supply(0x3000,dwords(descriptors+(len(coords)-1)*12,0,0,len(coords)))
    directions=supply(0x4000,dwords(*([0x7f7f7f7f]*(len(coords)+2))))
    pf=supply(0x5000,bytes(0xD00))
    initial_actor=bytes(u.mem_read(ACTOR,0x800)).hex()
    text_before=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    events=[];pending={};snapshots=[];reached=set();writes=[];helper_calls=[]
    def cell_state(pointer):
        return dict(identity='dummy'if pointer==DUMMY else'real',
                    coord=list(struct.unpack('<hh',u.mem_read(pointer+0x24,4))),
                    level=struct.unpack('<b',u.mem_read(pointer+0x11B,1))[0],flags=read32(pointer+0x140),
                    dummy_coord=list(struct.unpack('<hh',u.mem_read(DUMMY+0x24,4))))
    def snapshot(name):
        count=read32(0x89A2E0)
        return dict(name=name,start=list(struct.unpack('<hh',u.mem_read(0x89A2D8,4))),count=count,
                    directions=list(struct.unpack('<'+'i'*(len(coords)+2),u.mem_read(directions,(len(coords)+2)*4))),
                    retained_heights=list(struct.unpack('<'+'i'*(len(coords)-1),u.mem_read(0x89A324,(len(coords)-1)*4))))
    def observe(_u,address,_size,_data):
        if address in (ENTRY,QUEUE,SET):raise AssertionError(('unexpected substituted gameplay seam',hex(address)))
        if address in pending:
            for event in pending.pop(address):
                event['returned']=u.reg_read(UC_X86_REG_EAX)
                if row.get('extended'):
                    pointer=event.pop('_cell_pointer',DUMMY)
                    if event['pc']=='005657a0':pointer=event['returned']
                    event['after']=cell_state(pointer)
        if address in (0x42AA90,0x42B210,0x42B420,0x42B7F0,0x42BCA0,0x42BE20,0x73F0A0,0x4D9C10,0x4DC760):reached.add(f'{address:08x}')
        sp=u.reg_read(UC_X86_REG_ESP)
        if address==0x42BE20 and row.get('extended'):
            args=[read32(sp+4+i*4)for i in range(7)]
            helper_calls.append(dict(caller=f'{read32(sp):08x}',length=args[1],
                start=list(struct.unpack('<hh',u.mem_read(args[2],4))),
                delta=list(struct.unpack('<hh',u.mem_read(args[3],4))),
                height=struct.unpack('<i',dwords(args[5]))[0],tail=bool(args[6])))
        if address in (0x5657A0,0x56BCD0,0x73F0A0):
            args=list(struct.unpack('<'+'I'*(5 if address==0x73F0A0 else 2 if address==0x56BCD0 else 1),u.mem_read(sp+4,20 if address==0x73F0A0 else 8 if address==0x56BCD0 else 4)))
            e=dict(pc=f'{address:08x}',caller=f'{read32(sp):08x}',xy=list(struct.unpack('<hh',u.mem_read(args[0]+(0x24 if address==0x73F0A0 else 0),4))))
            if address==0x73F0A0:
                e.update(direction=args[1],height=struct.unpack('<i',dwords(args[2]))[0],previous=args[3],arg5=args[4],flags=read32(args[0]+0x140))
                if row.get('extended'):
                    e['before']=cell_state(args[0]);e['_cell_pointer']=args[0]
                assert args[3:]==[0,1]
            elif address==0x56BCD0:assert args[1]==HOUSE
            events.append(e);pending.setdefault(read32(sp),[]).append(e)
        if address==0x42A40B:snapshots.append(snapshot('reconstructed'))
        if address==0x42A41A:snapshots.append(snapshot('after_corners'))
    def written(_u,_access,address,size,value,_data):
        assert address+size<=0x401000 or address>=0x7E1000,('native text write',hex(address))
        if directions<=address<directions+(len(coords)+2)*4:
            writes.append(dict(pc=f'{u.reg_read(UC_X86_REG_EIP):08x}',index=(address-directions)//4,size=size,value=value))
    u.hook_add(UC_HOOK_CODE,observe);u.hook_add(UC_HOOK_MEM_WRITE,written)
    # Original successful caller supplies node, output list and Foot receiver;
    # stop immediately after the second pass, before marker cleanup/epilogue.
    u.mem_write(SP+0x68,dwords(ACTOR,directions))
    u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,pf);u.reg_write(UC_X86_REG_EAX,node)
    run_checked(u,0x42A3FE,0x42A423,count=1000000,
                required_addresses=(0x42A406,0x42AA90,0x42A415,0x42B210,0x42A41E,0x42B7F0))
    snapshots.append(snapshot('after_straight'))
    assert not pending,pending
    assert u.reg_read(UC_X86_REG_ESP)==SP
    assert before_rng==[bytes(u.mem_read(p,1012)).hex()for p in rngs]
    assert initial_actor==bytes(u.mem_read(ACTOR,0x800)).hex()
    assert text_before==hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    for p,size in guards:
        assert bytes(u.mem_read(p-16,16))==bytes(u.mem_read(p+size,16))==b'\xa5'*16
    result=dict(input=row,supplied_nodes=[dict(cell=list(c),height=h)for c,h in zip(coords,heights)],
                startup=startup,snapshots=snapshots,events=events,direction_writes=writes,reached=sorted(reached),
                dummy_coord=list(struct.unpack('<hh',u.mem_read(0xABDC50+0x24,4))),
                actor_unchanged=True,rng_unchanged=True,text_sha256=text_before,allocation_guards_intact=True)
    if row.get('extended'):result['reroute_calls']=helper_calls
    return result


def generate():
    rows=[execute(row)for row in cases()]
    frozen=json.dumps(rows[:33],sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
    assert hashlib.sha256(frozen).hexdigest()==FROZEN_FIRST33_SHA256,'historical33 native cases changed'
    return dict(schema_version=1,scope=__doc__,cases=rows)


def metadata():
    result=provenance(scope=__doc__,entry_points={'successful_tail':0x42A3FE,'stop':0x42A423,
        'reconstruction':0x42AA90,'corners':0x42B210,'single_corner':0x42B420,
        'straight':0x42B7F0,'anchor':0x42BCA0,'reroute':0x42BE20,'unit_entry':0x73F0A0,'threat':0x56BCD0},
        assumptions=['Supplied parent descriptors and Unit/House/Cell scalar prestates. Original reconstruction and both passes execute in their original caller order, with original concrete Unit admission and Drive/COM methods.',
            'This is not complete A* search, actor/type construction, native Scenario load, physical MTNK placement or blocked-repath marker production. Height/raw occupation/marker and threat controls have explicitly supplied state.',
            'Existing unit_source_scatter live-entry fixture supplies original class vtables; its Queue/Set seams are asserted unreached. CRT atexit guard is already set; no atexit method executes.',
            'Original CRT precision tail and WinMain chop selection execute through existing mapgen_range owner. Three original seed1 RNGs remain byte-identical; Unit body and original executable text are checked unchanged.',
            'Cell table is a supplied allocated32x32 square in native512 stride. Guarded parent/path storage and adequate House130x130 threat data are inputs; geometry/occupants are not native-produced.'],
        substitutions=['Existing fixture substitutes only OS InterlockedIncrement/Decrement. No path/admission/threat/height/RNG return or executable code is substituted.'])
    result['extended_bounds']=[
        'Historical first33 case payloads are frozen by canonical SHA256'+FROZEN_FIRST33_SHA256+'. New cases only append.',
        'Tube records/indexes and missing Map slots are supplied. Original reconstruction emits direction8 from distinct nonadjacent parent Cells; both finishing passes execute unchanged.',
        'Wrap controls explicitly mismatch the supplied Tube exit and reconstruction parent coordinates to exercise finishing packed-word arithmetic. They do not establish valid AStar production of those raw-domain combinations.',
        'Out-of-square parent Cells are distinct guarded descriptor-only inputs in a separate arena and are not inserted into the native Map table. Missing map queries retain the one native Dummy, supplied with empty Cell fields and -1 overlay/object/Tube indexes; new before/after receipts expose identity and nested entry writes.',
        'Adjacent binary64 threshold controls are supplied finite coefficients. Original FPCW startup, FMUL and comparison instructions establish returned admission; no Python/Rust arithmetic supplies the result.']
    return result
