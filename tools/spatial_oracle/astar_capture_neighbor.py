"""Original AStar neighbor-call/admission corridor with concrete Infantry entry.

The interior AStar frame is supplied. Original51BF90 and429830 execute, then
429FEA selects node creation versus blocked-goal abort. No full route claim.
"""
from pathlib import Path
from collections import Counter
import argparse,copy,hashlib,json,struct
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_READ
from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC,run_checked,provenance,finish_vectors
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.projectile_oracle.bridge_render_inputs import lexical,assets_root
from tools.spatial_oracle.building_body_rules import INI,SP,dwords
from tools.spatial_oracle.map_queries import packed

HERE=Path(__file__).resolve().parent
ASSETS=assets_root()
MAP=0x87F7E8

def digest(obj):return hashlib.sha256(json.dumps(obj,sort_keys=True,separators=(',',':')).encode()).hexdigest()

def prepare(row):
    m=Reader(ASSETS,{})
    u=m.u
    m.invoke(0x49F2F0,0)
    land_names=[m.string(m.read32(0x839D68+i*4))for i in range(12)]
    layers=[]
    for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
        path=ASSETS/name
        if not path.exists():layers.append(dict(file=name,absent=True));continue
        raw=path.read_bytes();sections,_=lexical(raw,set(land_names))
        m.make_ini(sections);m.invoke(0x674000,0,(INI,))
        layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest()))
    alloc=m.alloc
    u.mem_write(MAP+0xF4,dwords(*row['native_size'],*row['local_size']))
    table=alloc(0x40000*4);u.mem_write(MAP+0x13C,dwords(table,0x40000))
    u.mem_write(0xA8ED84,dwords(row['frame']))
    u.mem_write(0xA8E7AC,dwords(0)) # ScenarioInit false; original51C144 reader.
    cells={}
    for cell in row['cells']:
        ptr=alloc(0x200);cells[tuple(cell['coord'])]=ptr
        u.mem_write(ptr,dwords(0x7E4EEC));u.mem_write(ptr+0x24,packed(*cell['coord']))
        u.mem_write(ptr+0x44,dwords(cell['overlay']))
        u.mem_write(ptr+0x54,dwords(*cell['occupation_owners']))
        u.mem_write(ptr+0xEC,dwords(cell['land']))
        u.mem_write(ptr+0x116,struct.pack('<h',cell['tube_index']))
        u.mem_write(ptr+0x11B,bytes((cell['level'],cell['slope'])))
        u.mem_write(ptr+0x124,dwords(*cell['occupation']))
        u.mem_write(ptr+0x140,dwords(cell['bridge_flags']))
        x,y=cell['coord'];u.mem_write(table+(y*512+x)*4,dwords(ptr))
    u.mem_write(0xABDC50+0x116,struct.pack('<h',-1))
    actor,typ,hut,huttype=alloc(0x800),alloc(0xF00),alloc(0x900),alloc(0x1800)
    a=row['actor'];h=row['hut']
    u.mem_write(actor,dwords(0x7EB058));u.mem_write(actor+0x6C0,dwords(typ))
    u.mem_write(typ+0x67C,dwords(a['speed_type']))
    u.mem_write(actor+0x8C,bytes([a.get('on_bridge',False)]))
    u.mem_write(actor+0x9C,dwords(*a['coord']))
    u.mem_write(actor+0xAC,dwords(a['current_mission']));u.mem_write(actor+0xB4,dwords(a['queued_mission']))
    u.mem_write(actor+0x3D5,bytes([a['in_playfield']]))
    u.mem_write(actor+0x418,bytes([a['terrain_bypass']]))
    assert a['team'] is None and a['slave_owner'] is None
    u.mem_write(actor+0x69C,dwords(0));u.mem_write(actor+0x2DC,dwords(0))
    u.mem_write(actor+0x5A4,dwords(hut if a['nav_is_hut'] else 0))
    u.mem_write(actor+0x2B4,dwords(hut if a['attack_is_hut'] else 0))
    u.mem_write(hut,dwords(0x7E3EBC));u.mem_write(hut+0x520,dwords(huttype))
    u.mem_write(hut+0x9C,dwords(*h['coord']))
    u.mem_write(hut+0x270,bytes([h['warping_out']]))
    u.mem_write(hut+0x18C,dwords(h['iron_curtain_start'],0,h['iron_curtain_duration']))
    identities={a['id']:actor,h['id']:hut}
    for cell in row['cells']:
        ptr=cells[tuple(cell['coord'])]
        for key,offset in [('ground_objects',0xE4),('upper_objects',0xE8)]:
            ordered=cell[key];u.mem_write(ptr+offset,dwords(identities[ordered[0]]if ordered else 0))
            for i,obj in enumerate(ordered):
                u.mem_write(identities[obj]+0x30,dwords(identities[ordered[i+1]]if i+1<len(ordered)else 0))
    return m, actor, cells, table, layers

def execute(row):
    m,actor,cells,table,layers=prepare(row)
    u=m.u;alloc=m.alloc;a=row['actor']
    pf=alloc(0xD00)
    # Execute original scalar constructor prefix; vectors/pools are outside this
    # bounded corridor and are intentionally not constructed or consumed.
    u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,pf)
    run_checked(u,0x42A6D0,0x42A718,count=20000)
    ctor=bytes(u.mem_read(pf,9)).hex()
    u.mem_write(pf+8,bytes([row.get('fifth_flag',1)]))
    current=cells[tuple(row['from'])];candidate=cells[tuple(row['candidate'])]
    current_slot=table+(row['from'][1]*512+row['from'][0])*4
    candidate_slot=table+(row['candidate'][1]*512+row['candidate'][0])*4
    assert row['direction'] in range(8)
    u.mem_write(pf+0x30,dwords(row['height'],row['goal_height']))
    u.mem_write(pf+0x28,dwords(1));u.mem_write(pf+0x3C,dwords(row['urgency']))
    closed=alloc(4);u.mem_write(pf+0x18,dwords(closed,closed))
    # Native-local selected-layer state, supplied after the earlier zone/closed
    # candidate admission. Closed index is rebased to zero for this one candidate.
    stack=SP-0x1000;u.mem_write(stack,bytes(0x90))
    u.mem_write(stack+0x12,bytes([a['is_train'],False]))
    u.mem_write(stack+0x18,dwords(row['direction']))
    u.mem_write(stack+0x20,dwords(current_slot,candidate_slot))
    u.mem_write(stack+0x30,dwords(candidate+0x24))
    u.mem_write(stack+0x38,dwords(candidate_slot))
    u.mem_write(stack+0x60,bytes([not row['bridge_list']]))
    u.mem_write(stack+0x68,dwords(actor))
    u.reg_write(UC_X86_REG_ESP,stack);u.reg_write(UC_X86_REG_ESI,pf)
    u.reg_write(UC_X86_REG_EBX,candidate);u.reg_write(UC_X86_REG_EBP,0)
    visits=[];calls=Counter();entry_args=[];can_enter=[];ignored_arg_watch={};ignored_arg_reads=[]
    boundaries={0x42A01E:'admit_node_creation',0x42A3DE:'blocked_goal_abort',0x42A1A1:'skip_neighbor'}
    trace_points=(0x51BF90,0x4D9C60,0x51C300,0x51C37D,0x51C71B,0x51C77F,0x51C7D0,0x429830,0x429FEA,0x42A17D,0x41BF40,0x70C5B0)
    def observe(_u,pc,_size,_data):
        calls[pc]+=1
        if pc in trace_points:visits.append(hex(pc))
        if pc in (0x7C8E17,0x7C8B3D,0x7D140B,0x5B40B0):raise AssertionError('unexpected substituted callable in measured corridor')
        if pc==0x51BF90:
            sp=u.reg_read(UC_X86_REG_ESP);args=struct.unpack('<5I',u.mem_read(sp+4,20))
            assert args[:4]==(candidate,row['direction'],row['height']&0xffffffff,current) and args[4]&255==row.get('fifth_flag',1),args
            ignored_arg_watch.update(active=True,address=sp+20)
            entry_args.append(dict(candidate=row['candidate'],direction=args[1],height=args[2],previous=row['from'],flag=args[4]))
        if pc==0x429F5A:
            can_enter.append(u.reg_read(UC_X86_REG_EAX));ignored_arg_watch['active']=False
    def watch_read(_u,_access,address,size,_value,_data):
        if ignored_arg_watch.get('active') and address<ignored_arg_watch['address']+4 and address+size>ignored_arg_watch['address']:
            ignored_arg_reads.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),address=hex(address),size=size))
    spans=[(0x429F37,0x42A1A1),(0x51BF90,0x51C890),(0x429830,0x429A86)]
    code=[bytes(u.mem_read(x,y-x))for x,y in spans]
    hook=u.hook_add(UC_HOOK_CODE,observe)
    read_hook=u.hook_add(UC_HOOK_MEM_READ,watch_read)
    end=run_checked(u,0x429F37,tuple(boundaries),count=200000,required_addresses=(0x51BF90,0x429830,0x429FEA))
    u.hook_del(hook);u.hook_del(read_hook)
    assert ignored_arg_reads==[]
    assert code==[bytes(u.mem_read(x,y-x))for x,y in spans]
    assert u.reg_read(UC_X86_REG_ESP)==stack
    assert len(can_enter)==1
    scalar=struct.unpack('<f',u.mem_read(stack+0x34,4))[0]
    return dict(input=row,input_sha256=digest(row),native_entry_arguments=entry_args,
                can_enter_class=can_enter[0],outcome=boundaries[end],boundary=hex(end),
                step_cost=scalar,step_cost_hex=bytes(u.mem_read(stack+0x34,4)).hex(),
                constructor_scalar_bytes=ctor,reached=visits,code_unchanged=True,fifth_argument_reads=ignored_arg_reads,
                land_speed_bits=bytes(u.mem_read(0x89EA40+(next(c['land']for c in row['cells']if c['coord']==row['candidate'])*9+a['speed_type'])*4,4)).hex(),
                native_layers=layers,measured_substitutions=[])

def synthetic_cases():
    actor=dict(id=1,coord=[2624,2624,1040],current_mission=8,queued_mission=-1,in_playfield=True,terrain_bypass=False,team=None,slave_owner=None,nav_is_hut=True,attack_is_hut=False,speed_type=1,is_train=False)
    hut=dict(id=2,coord=[11*256+128,9*256+128,1040],warping_out=False,iron_curtain_start=-1,iron_curtain_duration=0)
    cell=lambda xy:dict(coord=xy,overlay=-1,occupation_owners=[-1,-1],land=7,tube_index=-1,level=10,slope=0,occupation=[0,0],bridge_flags=0,ground_objects=[],upper_objects=[])
    row=dict(name='synthetic_flat_capture',origin='Synthetic fixture control; not captured Hills gameplay',native_size=[8,8],local_size=[0,0,8,8],frame=112,cells=[cell([10,10]),cell([11,9])],actor=actor,hut=hut,**{'from':[10,10]},candidate=[11,9],direction=1,height=10,goal_height=10,urgency=0,bridge_list=False)
    row['cells'][1]['ground_objects']=[2]
    rows=[row]
    for flag in (0,255):
        q=copy.deepcopy(row);q['name']+='_'+str(flag)+'_fifth_argument';q['fifth_flag']=flag;rows.append(q)
    for bits in (28,32):
        q=copy.deepcopy(row);q['name']+='_'+str(bits);q['cells'][1]['occupation'][0]=bits;rows.append(q)
    q=copy.deepcopy(row);q['name']='synthetic_queued_capture';q['actor']['current_mission']=-1;q['actor']['queued_mission']=8;rows.append(q)
    q=copy.deepcopy(row);q['name']='synthetic_iron_curtain_control';q['hut'].update(iron_curtain_start=110,iron_curtain_duration=100);rows.append(q)
    q=copy.deepcopy(row);q['name']='synthetic_rock_speed_control';q['cells'][1]['land']=3;rows.append(q)
    for current,goal,initial in ((10,10,4),(14,10,10),(-128,127,127),(127,-128,-128),(-1,0,0),(0,-1,0),(131,127,127)):
        q=copy.deepcopy(row);q['name']=f'synthetic_signed_height_current{current}_goal{goal}_initial{initial}'
        q['origin']='Synthetic supplied signed AStar height boundary; initial_height_context is not consumed by this interior corridor; no map-loader reachability claim'
        q['height']=current;q['goal_height']=goal;q['initial_height_context']=initial
        q['cells'][0]['level']=current&255;q['cells'][1]['level']=goal&255
        if current==131:q['cells'][0].update(level=127,bridge_flags=256)
        q['cells'][0]['slope']=1;q['cells'][1]['slope']=1
        q['hut'].update(iron_curtain_start=110,iron_curtain_duration=100)
        rows.append(q)
    return rows

def generate(input_path=None):
    rows=json.loads(input_path.read_text())['cases'] if input_path else synthetic_cases()
    return dict(schema_version=1,scope=__doc__,
                source=input_path.name if input_path else 'explicit synthetic cases',
                source_sha256=hashlib.sha256(input_path.read_bytes()).hexdigest() if input_path else None,
                cases=[execute(row) for row in rows])

def metadata():
    return provenance(scope=__doc__,assumptions=['Supplied interior AStar frame after zone/closed-list candidate admission; sparse native map, actor/type and hut storage come from each explicitly labelled input case. No full42C900/429A90 or route reconstruction executes.','Original Pathfinder scalar constructor prefix42A6D0..42A718 supplies cost scale1 and flag1. Search current/goal heights, generation stamp, selected list, raw table pointers and re-based single closed index supplied.','Physical applicable rules layers are processed by original674000 using lexical INI caches; all selected land-speed bytes come from native reading, not Rust scalar conversion.','Original unmodified Infantry/Building vtables, original51BF90+4D9C60, mission, warping and IronCurtain readers, original429830 and CMP7 branch execute.','ScenarioInit false, no team/slave and ordinary Infantry receiver explicitly supplied. Counterfactuals are labelled separately.'],substitutions=['Setup INI reader inherits allocator/free/TLS/file seams; none is reached during measured AStar neighbor corridor.'],entry_points={'astar_neighbor':0x429F37,'can_enter':0x51BF90,'height_gate':0x4D9C60,'edge_cost':0x429830,'cost_class_cmp':0x429FEA,'admitted_before_create_node':0x42A01E,'blocked_goal_tail':0x42A3DE})

if __name__=='__main__':
    parser=argparse.ArgumentParser(add_help=False)
    parser.add_argument('--input',type=Path)
    args,remaining=parser.parse_known_args()
    finish_vectors(lambda:generate(args.input),Path(__file__).with_suffix('.json'),provenance=metadata,argv=remaining)
