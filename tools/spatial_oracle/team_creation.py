"""Original Team creation and selected normal reinforcement prerequisites.

Run: python -m tools.spatial_oracle.team_creation [--check | --write]
Selected Full member/cargo/scalar controls:
python -m tools.spatial_oracle.team_creation --full-transport [--check | --write]
Selected Droppod/transport-input controls:
python -m tools.spatial_oracle.team_creation --paradrop-transport [--check | --write]

The legacy39 rows retain full Trigger4/CreateTeam and empty constructors.
Additive rows execute shared6EC300, signed script50/countdown, recent-cell660B80,
full65D8E0/65DD30/65E010 on genuine Unit/Infantry/Aircraft factories and full
TeamAI/script3/CoordinateMove controls, plus zero-origin scalar cleanup flag
controls and anchored member Tag/OnTransOnly controls. The existing FootMissions owner supplies its
physical type readers/crop and platform boundaries. Coverage is per case_kind;
this is neither whole Team AI nor campaign startup/terrain parity.
"""
from pathlib import Path
import hashlib
import json
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_EIP, UC_X86_REG_ESP)
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import Fixture, SCENARIO, MAP, cell
from tools.spatial_oracle.map_queries import dwords, packed

HOUSES = [SCRATCH + 0x20000, SCRATCH + 0x26000]
TEAM_TYPE, SCRIPT_TYPE, TAG_TYPE, ACTION = [SCRATCH+n for n in (0x40000,0x41000,0x42000,0x47000)]
TRIGGER_TYPES = [SCRATCH+0x43000, SCRATCH+0x44000]
EVENTS = [SCRATCH+0x45000, SCRATCH+0x46000]
HEAP, HEAP_END = SCRATCH+0x60000, SCRATCH+0x80000
REGISTRIES = {
    'team':0x8B40E8, 'abstract':0xB0F670, 'team_base_a':0xB0F618,
    'team_base_b':0xB0F720, 'team_base_c':0xB0F6F0, 'script':0x8872B0,
    'tag':0xB0E720, 'tag_trigger_base':0xB0F708, 'trigger':0xA8EAE8,
}


def u32(u, address):
    return struct.unpack('<I',u.mem_read(address,4))[0]


def i32(u, address):
    return struct.unpack('<i',u.mem_read(address,4))[0]


def execute(case):
    f=Fixture(dict(seed=case.get('seed',31)))
    u=f.u
    # Run the original static CellStruct zero initializers used by the
    # constructor's empty-waypoint comparison and TeamType waypoint default.
    f.call(0x6E8A20,0,[])
    f.call(0x6F0610,0,[])
    u.mem_write(0xA8ED84,dwords(case.get('frame',123)))
    u.mem_write(0xA8E7AC,dwords(case.get('creation_depth',0)))
    u.mem_write(0xA8B238,dwords(case.get('game_mode',0)))
    for index,(name,base) in enumerate(REGISTRIES.items()):
        pointer=SCRATCH+0x48000+index*0x400
        # Enough initialized capacity: constructor array publication executes
        # unchanged, while vector growth is outside this fixture's coverage.
        u.mem_write(base+4,dwords(pointer,64,0,0,64))
    house_array=SCRATCH+0x4C000
    u.mem_write(0xA8022C,dwords(house_array))
    u.mem_write(0xA80238,dwords(len(HOUSES)))
    u.mem_write(house_array,dwords(*HOUSES))
    for index,house in enumerate(HOUSES):
        u.mem_write(house+0x566C,dwords(7+index))
    slot=case.get('multiplayer_slot',-1)
    u.mem_write(TEAM_TYPE+0xC8,dwords(slot))
    u.mem_write(SCENARIO+0x1180,dwords(*case.get('slot_houses',[1,0,-1,-1,-1,-1,-1,-1])))
    default_owner=case.get('default_owner',0)
    u.mem_write(TEAM_TYPE+0xC4,dwords(HOUSES[default_owner] if default_owner>=0 else 0))
    u.mem_write(TEAM_TYPE+0xB8,dwords(case.get('max',-1)))
    u.mem_write(TEAM_TYPE+0xDC,dwords(case.get('type_count',0)))
    u.mem_write(TEAM_TYPE+0xD4,dwords(case.get('waypoint_index',-1)))
    u.mem_write(TEAM_TYPE+0xE0,dwords(SCRIPT_TYPE))
    u.mem_write(TEAM_TYPE+0xF6,bytes([case.get('base_defense',False)]))
    u.mem_write(SCENARIO+0x632+4*4,packed(*case.get('waypoint',[64,64])))
    old_teams=case.get('existing',[])
    for index,(owner,same_type) in enumerate(old_teams):
        old=SCRATCH+0x80000+index*0x100
        u.mem_write(old+0x24,dwords(TEAM_TYPE if same_type else TEAM_TYPE+0x200,0,HOUSES[owner]))
        u.mem_write(u32(u,REGISTRIES['team']+4)+index*4,dwords(old))
    u.mem_write(REGISTRIES['team']+16,dwords(len(old_teams)))
    tags=case.get('triggers',[])
    if case.get('tag',False) or tags:
        u.mem_write(TEAM_TYPE+0xD0,dwords(TAG_TYPE))
        u.mem_write(TAG_TYPE+0xA0,dwords(TRIGGER_TYPES[0] if tags else 0))
        for index,definition in enumerate(tags):
            trigger=TRIGGER_TYPES[index]
            u.mem_write(trigger+0xA8,dwords(TRIGGER_TYPES[index+1] if index+1<len(tags) else 0))
            u.mem_write(trigger+0x9C,bytes(definition.get('difficulty',[True,True,True,True])))
            event=definition.get('event')
            if event:
                u.mem_write(trigger+0xAC,dwords(EVENTS[index]))
                u.mem_write(EVENTS[index]+0x2C,dwords(event[0]))
                u.mem_write(EVENTS[index]+0x34,dwords(event[1]))
        u.mem_write(SCENARIO+0x60C,dwords(case.get('difficulty',1)))
    cursor=HEAP
    allocations,calls,draws=[],[],[]
    observed={0x6F09C0:'create',0x510F60:'is_multiplayer_slot',0x510ED0:'resolve_multiplayer_slot',
              0x5095D0:'house_team_count',0x6E8A90:'team_ctor',0x6913C0:'script_ctor',
              0x6E4DE0:'tag_ctor',0x725FA0:'trigger_ctor',0x726400:'trigger_reset',
              0x6F18A0:'team_waypoint',0x68BCC0:'waypoint',0x5657A0:'cell_lookup',
              0x65C7E0:'ranged_rng'}
    def observe(_u,pc,_size,_data):
        nonlocal cursor
        if pc in observed: calls.append(observed[pc])
        # RandomRanged inlines the next-value recurrence. Count each original
        # update, including rejection draws; observing65C780 would miss them.
        if pc==0x65C837: draws.append(1)
        if pc!=0x7C8E17: return
        sp=u.reg_read(UC_X86_REG_ESP)
        size=u32(u,sp+4)
        pointer=cursor
        cursor+=(max(size,1)+15)&~15
        assert cursor<HEAP_END
        # Poison fresh storage so zero output must come from a native writer.
        u.mem_write(pointer,b'\xA5'*max(size,1))
        allocations.append(size)
        u.reg_write(UC_X86_REG_EAX,pointer)
        u.reg_write(UC_X86_REG_ESP,sp+4)
        u.reg_write(UC_X86_REG_EIP,u32(u,sp))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    ranges=[(0x6F09C0,0x6F0A6E),(0x6E8A90,0x6E8DCF),(0x6913C0,0x691454),
            (0x6E4DE0,0x6E4F5C),(0x725FA0,0x726133),(0x6DEB57,0x6DEB8D)]
    before_code=[bytes(u.mem_read(start,end-start)) for start,end in ranges]
    before_rng=bytes(u.mem_read(SCENARIO+0x218,0x3F4))
    if case.get('trigger_action',False):
        u.mem_write(ACTION+0x2C,dwords(4,0 if case.get('null_team_type',False) else TEAM_TYPE))
        # Trigger owner's argument deliberately differs from TeamType owner.
        returned=f.call(0x6DD8B0,ACTION,[HOUSES[1],0,0,0])&255
    else:
        explicit=case.get('explicit_owner',-1)
        returned=f.call(0x6F09C0,TEAM_TYPE,[HOUSES[explicit] if explicit>=0 else 0])
    u.hook_del(hook)
    assert before_code==[bytes(u.mem_read(start,end-start)) for start,end in ranges]
    counts={name:i32(u,base+16) for name,base in REGISTRIES.items()}
    created=counts['team']>len(old_teams)
    result=None
    if created:
        team=u32(u,u32(u,REGISTRIES['team']+4)+len(old_teams)*4)
        script=u32(u,team+0x28)
        tag=u32(u,team+0x70)
        target=u32(u,team+0x34)
        if target:
            target=list(struct.unpack('<hh',u.mem_read(target+0x24,4)))
        triggers=[]
        current=u32(u,tag+0x28) if tag else 0
        while current:
            triggers.append(dict(type_index=TRIGGER_TYPES.index(u32(u,current+0x24)),
                enabled=u.mem_read(current+0x44,1)[0],
                timer_start=i32(u,current+0x34),timer_duration=i32(u,current+0x3C),
                events_completed=u32(u,current+0x40)))
            current=u32(u,current+0x28)
            assert len(triggers)<=2
        result=dict(owner_index=HOUSES.index(u32(u,team+0x2C)),target_cell=target or None,
                    flags_74_84=list(u.mem_read(team+0x74,17)),
                    created_frame=i32(u,team+0x50),timers=[i32(u,team+n) for n in (0x58,0x60,0x64,0x6C)],
                    member_count=i32(u,team+0x48),member_type_counts=list(struct.unpack('<6i',u.mem_read(team+0x88,24))),
                    script_cursor=i32(u,script+0x2C),script_aux=i32(u,script+0x28),
                    tag_present=bool(tag),triggers=triggers)
    return dict(input=case,created=created,result=result,calls=calls,allocations=allocations,
                registry_counts=counts,type_count=i32(u,TEAM_TYPE+0xDC),
                house_base_defense_counts=[i32(u,h+0x566C) for h in HOUSES],
                creation_depth=i32(u,0xA8E7AC),
                return_value=returned if case.get('trigger_action',False) else bool(returned),
                rng_draws=len(draws),rng_changed=before_rng!=bytes(u.mem_read(SCENARIO+0x218,0x3F4)),
                next_random=f.call(0x65C780,SCENARIO+0x218,[]))


def inputs():
    rows=[dict(name='default'),dict(name='explicit_wins',explicit_owner=1),
          dict(name='no_owner',default_owner=-1),dict(name='waypoint',waypoint_index=4),
          dict(name='empty_waypoint',waypoint_index=4,waypoint=[0,0]),
          dict(name='house_counter',base_defense=True),dict(name='negative_frame',frame=-1),
          dict(name='empty_tag',tag=True),dict(name='tag_two_triggers',triggers=[{},{}]),
          dict(name='tag_timer',triggers=[dict(event=[13,7])]),
          dict(name='tag_random_timer',triggers=[dict(event=[51,7])]),
          dict(name='tag_reverse_rng_order',triggers=[dict(event=[51,7]),dict(event=[51,19])]),
          dict(name='disabled_random_timer',triggers=[dict(event=[51,7],difficulty=[True,False,True,True])])]
    rows += [dict(name=f'slot_{slot}',default_owner=-1,multiplayer_slot=slot)
             for slot in (-1,0,0x117A,0x117B,0x117C,0x117D,0x1182,0x1183)]
    rows += [dict(name=f'limit_{mode}_{count}_{maximum}',game_mode=mode,type_count=count+1,max=maximum,
                  existing=[[0,True]]*count+[[1,True],[0,False]])
             for mode in (0,1) for count,maximum in ((0,0),(0,1),(1,1),(2,1),(2,-1))]
    rows += [dict(name=f'trigger_{index}',trigger_action=True,**values) for index,values in enumerate([
        {},dict(max=0),dict(max=0,creation_depth=7),dict(null_team_type=True),
        dict(default_owner=-1),dict(default_owner=-1,multiplayer_slot=0x117B),
        dict(tag=True,waypoint_index=4),dict(creation_depth=-1,max=0),
    ])]
    return rows


def generate():
    rows=[execute(case) for case in inputs()]
    named={row['input']['name']:row for row in rows}
    assert named['slot_4475']['result']['owner_index']==1
    assert named['slot_4476']['result']['owner_index']==0
    assert not named['slot_4477']['created']
    assert named['explicit_wins']['result']['owner_index']==1
    assert not named['limit_0_0_1']['created']
    assert named['limit_1_0_1']['created']
    assert named['trigger_1']['created']
    assert named['trigger_1']['creation_depth']==0
    assert named['trigger_1']['result']['owner_index']==0
    assert not named['trigger_3']['created'] and named['trigger_3']['return_value']==1
    assert named['disabled_random_timer']['result']['triggers'][0]['enabled']==0
    for row in rows:
        assert row['rng_changed']==bool(row['rng_draws']), row['input']['name']
        if row['created']:
            assert row['result']['script_cursor']==-1
            assert row['result']['member_count']==0
    # Additive corpora retain the39 existing creation rows byte-for-byte in
    # canonical JSON; consumers select new case_kind rows explicitly.
    assert len(rows)==39
    assert hashlib.sha256(json.dumps(rows,sort_keys=True,separators=(',',':'),allow_nan=False).encode()).hexdigest()=='0cbe504881d4911878172bf407a3c5f5522d8f3516efec882f2820c658c9b448'
    rows.extend(execute_waypoint(case) for case in waypoint_inputs())
    rows.extend(execute_flash(case) for case in flash_inputs())
    rows.extend(execute_recent_cells(case) for case in recent_cell_inputs())
    fixture=reinforcement_fixture()
    reinforcement_rows=execute_reinforcement_rows(fixture)
    # Preserve physical reader inputs and original post-read values alongside
    # the executions which consume them. These are native corpus inputs, not
    # a test-run receipt. The FootMissions dependency corpus owns MTNK/E1.
    reinforcement_rows[0]['aircraft_type_inputs']=dict(
        fixed_art=fixture.orca_art_inputs,layers=fixture.orca_reads)
    reinforcement_rows[0]['subcell_startup']=fixture.subcell_startup
    rows.extend(reinforcement_rows)
    rows.extend(execute_script_move_rows(fixture))
    rows.extend(execute_placement_retry_rows(fixture))
    assert len(rows)==99
    assert hashlib.sha256(json.dumps(rows,sort_keys=True,separators=(',',':'),allow_nan=False).encode()).hexdigest()=='da80e80715f0d2c16398c81c175675b94bff8e6a782c3a3343af271f908bff3c'
    rows.extend(execute_scalar_cleanup_rows(fixture))
    assert len(rows)==111
    assert hashlib.sha256(json.dumps(rows,sort_keys=True,separators=(',',':'),allow_nan=False).encode()).hexdigest()=='04815ae6b318af5af403425f32c8358ea8c1e81389b97a4979e12c0d241c7db0'
    rows.extend(execute_member_tag_rows())
    rows.append(execute_on_trans_only_reader())
    return rows


def waypoint_inputs():
    rows=[dict(name='waypoint_inside',waypoint=[64,64]),
          dict(name='waypoint_outside',waypoint=[127,127]),
          dict(name='waypoint_unset',waypoint=[0,0]),
          dict(name='waypoint_dummy',waypoint=[-7,64]),
          dict(name='waypoint_last_slot',waypoint=[127,127],index=701),
          dict(name='waypoint_forming',waypoint=[127,127],formed=False),
          dict(name='waypoint_before_first',waypoint=[127,127],cursor=-1),
          dict(name='waypoint_after_last',waypoint=[127,127],cursor=2),
          dict(name='waypoint_other_action',waypoint=[127,127],action=2)]
    rows += [dict(name=f'waypoint_level_{level}',waypoint=[64,40],level=level)
             for level in (0,1,7,-1,-128)]
    return rows


def execute_waypoint(case):
    """Full6EC300/Script readers/Scenario68BCC0/Map578460; raw prior state."""
    f=Fixture(dict(seed=31,local=case.get('local',[0,0,64,64])))
    u=f.u;team=SCRATCH+0x50000;script=SCRATCH+0x51000;typ=SCRATCH+0x52000
    index=case.get('index',7);x,y=case['waypoint']
    u.mem_write(team+0x28,dwords(script));u.mem_write(team+0x7F,bytes([case.get('formed',True)]))
    u.mem_write(script+0x24,dwords(typ,0,case.get('cursor',0)))
    u.mem_write(typ+0xA0,dwords(2,case.get('action',3),index,2,0))
    u.mem_write(SCENARIO+0x632+index*4,packed(x,y))
    if 0<=x<128 and 0<=y<128:u.mem_write(cell(x,y)+0x11B,bytes([case.get('level',0)&255]))
    u.mem_write(0xABDC50+0x24,packed(-91,77))
    observed={0x6915D0:'has_current',0x691500:'current_action',0x68BCC0:'waypoint',
              0x578460:'playfield_mode1',0x5657A0:'cell_lookup'};calls=[]
    hook=u.hook_add(UC_HOOK_CODE,lambda _u,pc,_size,_data: calls.append(observed[pc]) if pc in observed else None)
    before=bytes(u.mem_read(SCENARIO+0x218,0x3F4))
    try:answer=f.call(0x6EC300,team,[])&255
    finally:u.hook_del(hook)
    assert before==bytes(u.mem_read(SCENARIO+0x218,0x3F4))
    return dict(case_kind='waypoint',input=case,result=bool(answer),calls=calls,
                dummy=list(struct.unpack('<hh',u.mem_read(0xABDC50+0x24,4))))


def flash_inputs():
    return [dict(name=f'flash_{value}_{first}_{count}',argument=value,first=first,count=count)
            for value in (0,1,100,150,-1,-2147483648,2147483647)
            for first,count in ((True,3),(True,0),(False,3))]


def execute_flash(case):
    """Original script50 dispatch fragment, then full signed countdown4CC770.

    The supplied prior Team/member list is the input; no Techno constructor,
    full TeamAI or full TechnoAI scheduling claim is made by this slice.
    """
    f=Fixture(dict(seed=31));u=f.u;team=SCRATCH+0x50000
    members=[SCRATCH+0x52000+n*0x1000 for n in range(case['count'])]
    initial=[17,-7,2147483647][:case['count']]
    u.mem_write(team+0x54,dwords(members[0] if members else 0))
    for index,member in enumerate(members):
        u.mem_write(member+0xF0,dwords(initial[index]));u.mem_write(member+0xF4,b'\xA5')
        u.mem_write(member+0x5D8,dwords(members[index+1] if index+1<len(members) else 0))
    # Match the real TeamAI prologue's four saved registers and0x28 locals.
    u.mem_write(f.sp,dwords(*([0]*14),RET_MAGIC))
    for reg,value in ((UC_X86_REG_ESP,f.sp),(UC_X86_REG_ESI,team),
                      (UC_X86_REG_EBX,int(case['first'])),(UC_X86_REG_ECX,case['argument']&0xffffffff)):
        u.reg_write(reg,value)
    before=bytes(u.mem_read(SCENARIO+0x218,0x3F4))
    run_checked(u,0x6E9CF7,RET_MAGIC)
    assert u.reg_read(UC_X86_REG_ESP)==f.sp+60
    stored=[i32(u,p+0xF0) for p in members];steps=[]
    for _ in range(4):
        replies=[bool(f.call(0x4CC770,p+0xF0,[])&255) for p in members]
        steps.append(dict(counters=[i32(u,p+0xF0) for p in members],active=replies,
                          parity=[u.mem_read(p+0xF4,1)[0] for p in members]))
    assert before==bytes(u.mem_read(SCENARIO+0x218,0x3F4))
    return dict(case_kind='elite_flash',input=case,initial=initial,stored=stored,
                advance=bool(u.mem_read(team+0x80,1)[0]),steps=steps)


def recent_cell_inputs():
    cells=[[index*3-13,index*7+9] for index in range(14)]
    return [dict(name='recent_14',counter=0,cells=cells),
            dict(name='recent_wrap',counter=7,cells=cells[:4]),
            dict(name='recent_negative',counter=-1,cells=cells[:3]),
            dict(name='recent_signed_overflow',counter=2147483646,cells=cells[:3])]


def execute_recent_cells(case):
    """Original660B80 ring pushes; physical slots and review order observed."""
    f=Fixture(dict(seed=31));u=f.u;point=SCRATCH+0x50000
    u.mem_write(0xB04D88,dwords(case['counter']));u.mem_write(0xB04D48,bytes(32))
    history=[]
    for requested in case['cells']:
        u.mem_write(point,packed(*requested));f.call(0x660B80,point,[])
        counter=i32(u,0xB04D88)
        slots=[list(struct.unpack('<hh',u.mem_read(0xB04D48+index*4,4))) for index in range(8)]
        # Native stores its signed-modulo slot directly; the abstract review
        # order follows newest backwards. Match ordering, not a rotated slot.
        slot=i32(u,0xB04DD8)
        history.append(dict(counter=counter,cursor=slot,slots=slots,
            newest_first=[slots[(slot-index)&7] for index in range(min(len(history)+1,8))]))
    return dict(case_kind='recent_cells',input=case,history=history)


def reinforcement_inputs():
    return [dict(name='reinforce_two_mtnk',entries=[['MTNK',2]],origin=[87,50]),
            dict(name='reinforce_three_e1',entries=[['E1',3]],origin=[87,49]),
            dict(name='reinforce_mixed',entries=[['MTNK',1],['E1',2]],origin=[88,50]),
            dict(name='reinforce_veteran',entries=[['MTNK',1]],origin=[88,49],veteran=2),
            dict(name='reinforce_elite',entries=[['E1',1]],origin=[86,49],veteran=3),
            dict(name='reinforce_nonallied',entries=[['MTNK',1]],origin=[86,50],allied=False),
            dict(name='reinforce_unset_origin',entries=[['MTNK',2]],origin=[0,0]),
            dict(name='reinforce_two_orca',entries=[['ORCA',2]],origin=[87,50]),
            dict(name='reinforce_orca_edge3',entries=[['ORCA',1]],origin=[88,49],edge=3),
            dict(name='reinforce_off_table',entries=[['MTNK',1]],origin=[-7,64])]


def initialize_subcell_startup(q):
    """Execute the original process CRT's retained Cell placement producer.

    Original PE entry7CD80F calls7CBDAF at7CD8B4, before WinMain6BB9A0.
    7CBDC9..7CBDD3 sends [812000,815DA4) to the original initializer loop
    7CBED3. Its actual slot812B28 points to48E480, whose body initializes
    the five CoordStruct offsets at89E9F0. Run that loop over its one actual
    table slot; no subcell coordinate or placement answer is supplied.
    """
    if hasattr(q,'subcell_startup'):
        assert bytes(q.u.mem_read(0x89E9F0,60)).hex()==q.subcell_startup['after_hex']
        return
    m,u=q.m,q.u
    assert m.read32(0x812B28)==0x48E480
    before=bytes(u.mem_read(0x89E9F0,60)).hex()
    before_rng={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
    called=[]
    def observe(_u,pc,_size,_data):
        if pc==0x48E480:called.append(hex(pc))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    try:m.invoke(0x7CBED3,0,(0x812B28,0x812B2C),context={'case':'original Cell subcell CRT producer'})
    finally:u.hook_del(hook)
    after_rng={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
    code_hash=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    assert called==['0x48e480'] and before_rng==after_rng and code_hash==q.resident.code_hash
    q.subcell_startup=dict(process_entry='0x007CD80F',crt_entry='0x007CBDAF',
        initializer_loop='0x007CBED3',original_table=['0x00812000','0x00815DA4'],
        executed_table=['0x00812B28','0x00812B2C'],original_crt_entry=m.read32(0x812B28),
        called=called,before_hex=before,after_hex=bytes(u.mem_read(0x89E9F0,60)).hex(),
        rng_before=before_rng,rng_after=after_rng,native_code_sha256=code_hash)


def reinforcement_fixture(*, unit_geometry=False):
    """Keep original process geometry initialization before real placement."""
    from tools.spatial_oracle.anytown_damage.foot_missions import FootMissions
    q=FootMissions()
    initialize_subcell_startup(q)
    if unit_geometry:
        from tools.spatial_oracle.anytown_damage.unit_unlimbo import initialize_unit_geometry
        q.unit_geometry_startup=initialize_unit_geometry(q,foot=True)
    q.initialize_companion()
    return q


def prepare_reinforcement_case(q, case):
    """One supplied resolved-definition/waypoint input owner for action7."""
    m,u=q.m,q.u
    scenario=m.read32(0xA8B230)
    typ=m.alloc(0x100);m.invoke(0x6F06E0,typ,(m.cstring(case['name']),))
    script=m.alloc(0x234);u.mem_write(script,bytes(0x234))
    steps=case.get('script_steps',[(3,4)])
    u.mem_write(script+0xA0,dwords(len(steps),*(value for step in steps for value in step)))
    task_force=m.alloc(0x100);u.mem_write(task_force,bytes(0x100))
    u.mem_write(task_force+0x9C,dwords(len(case['entries'])))
    types={'MTNK':q.typ,'E1':q.e1_type,'ORCA':q.orca_type,
           **getattr(q,'reinforcement_types',{})}
    for index,(name,amount) in enumerate(case['entries']):
        u.mem_write(task_force+0xA4+index*8,dwords(amount,types[name]))
    u.mem_write(typ+0xA0,dwords(case.get('veteran',1)))
    u.mem_write(typ+0xA5,bytes([case.get('full',False)]))
    u.mem_write(typ+0xB0,bytes([case.get('droppod',False)]))
    u.mem_write(typ+0xB1,bytes([case.get('use_transport_origin',False)]))
    u.mem_write(typ+0xD8,dwords(case.get('transport_waypoint',-1)))
    u.mem_write(typ+0xC4,dwords(q.house));u.mem_write(typ+0xD4,dwords(4))
    u.mem_write(typ+0xE0,dwords(script,task_force))
    u.mem_write(scenario+0x632+4*4,packed(*case['origin']))
    if 'transport_origin' in case:
        u.mem_write(scenario+0x632+5*4,packed(*case['transport_origin']))
    u.mem_write(q.house+0x1E0,dwords(case.get('edge',0)))
    u.mem_write(0xA83D4C,dwords(q.house if case.get('allied',True) else q.enemy))
    u.mem_write(0xA8E7AC,dwords(0))
    u.mem_write(0xB04D88,dwords(0));u.mem_write(0xB04D48,bytes(32))
    return typ


def execute_reinforcement_rows(fixture=None, cases=None):
    """Full65D8E0/65DD30/65E010 with original Unit/Infantry/Aircraft factories.

    Reuse the existing composed Foot fixture's physical terrain, retail type
    readers, COM factories and OS transports. No Type factory, Unlimbo,
    CanEnter, member mutation or cleanup return is supplied here. TeamType,
    ScriptType and TaskForce prior inputs are supplied resolved definitions;
    their complete INI loading and SOV01's full terrain are outside this crop.
    """
    q=fixture or reinforcement_fixture()
    initialize_subcell_startup(q)
    m,u=q.m,q.u
    initialize_aircraft(q)
    scenario=m.read32(0xA8B230)
    for entry in (0x6E8A20,0x6F0610):m.invoke(entry,0)
    # Install only uninitialized registries. Existing Abstract/Foot arrays
    # retain all genuine setup objects and their original publication order.
    for base in (*REGISTRIES.values(),0xA8ECA0,0xAC4C78):
        if not m.read32(base):u.mem_write(base,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
    rows=[]
    for case in reinforcement_inputs() if cases is None else cases:
        typ=prepare_reinforcement_case(q,case)
        before_id=i32(u,scenario+0x214)
        before_rng={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
        before_counts={name:i32(u,base+16) for name,base in REGISTRIES.items()}
        constructed=[];placement=[];calls=[];teams=[];returns={};ids=[]
        entries={0x65D8E0:'spawn',0x65DD30:'create_members',0x65E010:'place_members',
                 0x6E8A90:'team_ctor',0x6913C0:'script_ctor',0x6EA500:'add_member',
                 0x6EA870:'remove_member',0x7258D0:'detach_all',0x660B80:'recent_cell',
                 0x7353C0:'unit_ctor',0x517A50:'infantry_ctor',0x410230:'create_id',
                 0x413D20:'aircraft_ctor',0x6C4A90:'fly_factory',0x4CC9A0:'fly_ctor',
                 0x68BCB0:'scenario_id',0x65C7E0:'rng_ranged',0x65C780:'rng_next',
                 0x737BA0:'unit_unlimbo',0x51DFF0:'infantry_unlimbo',0x414310:'aircraft_unlimbo',0x568300:'in_size'}
        def observe(_u,pc,_size,_data):
            sp=u.reg_read(UC_X86_REG_ESP);receiver=u.reg_read(UC_X86_REG_ECX)
            if pc in returns:
                for record in returns.pop(pc):record['returned_al']=u.reg_read(UC_X86_REG_EAX)&255
            if pc in entries:calls.append(entries[pc])
            if pc==0x6E8A90:teams.append(receiver)
            if pc==0x68BCB0:ids.append(i32(u,scenario+0x214))
            if pc==0x65DE3C:
                pointer=u.reg_read(UC_X86_REG_EAX)
                if pointer:
                    constructed.append(dict(pointer=hex(pointer),native_id=i32(u,pointer+0x10),
                        type=m.string(m.read32(pointer+(0x6C0 if m.read32(pointer)==0x7EB058 else 0x6C4))+0x24),limbo=u.mem_read(pointer+0x81,1)[0],
                        phase=u.mem_read(pointer+0x3C8,2).hex()))
            if pc in (0x737BA0,0x51DFF0,0x414310):
                coord=m.read32(sp+4)
                record=dict(pointer=hex(receiver),native_id=i32(u,receiver+0x10),
                    xyz=list(struct.unpack('<3i',u.mem_read(coord,12))),facing=m.read32(sp+8),
                    placement_depth=i32(u,0xA8E7AC))
                placement.append(record);returns.setdefault(m.read32(sp),[]).append(record)
        hook=u.hook_add(UC_HOOK_CODE,observe);q.phase='placement';q.events.clear();q.pending.clear()
        try:
            u.reg_write(UC_X86_REG_EDX,case.get('override',-1)&0xffffffff)
            answer=m.invoke(0x65D8E0,typ,context={'case':case,'constructor_fixture':'FootMissions physical crop'})&255
        except Exception as exc:
            raise RuntimeError(dict(case=case,calls=calls[-30:],events=q.events[-10:],constructed=constructed)) from exc
        finally:u.hook_del(hook)
        team=teams[0] if teams else 0
        members=[];member=m.read32(team+0x54) if team else 0
        while member:
            members.append(dict(native_id=i32(u,member+0x10),type=m.string(m.read32(member+(0x6C0 if m.read32(member)==0x7EB058 else 0x6C4))+0x24),
                limbo=u.mem_read(member+0x81,1)[0],alive=u.mem_read(member+0x90,1)[0],
                xyz=list(struct.unpack('<3i',u.mem_read(member+0x9C,12))),
                initiated=u.mem_read(member+0x689,1)[0],veterancy=bytes(u.mem_read(member+0x150,4)).hex(),
                mission=i32(u,member+0xAC),queued=i32(u,member+0xB4)))
            member=m.read32(member+0x5D8);assert len(members)<64
        code_hash=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
        assert code_hash==q.resident.code_hash
        rows.append(dict(case_kind='reinforcement',input=case,returned=answer,constructed=constructed,
            team_pointer=hex(team),
            placement=placement,members=members,calls=calls,id_before=before_id,id_after=i32(u,scenario+0x214),
            create_id_inputs=ids,rng_before=before_rng,
            rng_after={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()},
            registry_before=before_counts,registry_after={name:i32(u,base+16) for name,base in REGISTRIES.items()},
            team_native_id=i32(u,team+0x10) if team else None,
            team_flags=list(u.mem_read(team+0x74,17)) if team else None,
            recent_counter=i32(u,0xB04D88),recent_cell=list(struct.unpack('<hh',u.mem_read(0xB04D48+4,4))),
            creation_depth=i32(u,0xA8E7AC),native_code_sha256=code_hash))
    return rows


def initialize_aircraft(q, name='ORCA'):
    """Physical aircraft41CC20 reader and original Fly6C4A90 COM factory.

    The activation/OleRun boundary follows the composed Drive/Walk owner's
    convention: route the parsed GUID to its original class factory, whose
    allocation, ctor, QueryInterface, AddRef and Link_To_Object all execute.
    """
    import os
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.spatial_oracle.anytown_damage import mission as native_owner
    from tools.spatial_oracle.building_body_rules import RULES
    m,u=q.m,q.u
    for house in (q.house,q.enemy):
        for offset in (0x553C,0x558C):
            if not m.read32(house+offset):
                u.mem_write(house+offset,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
    field={'ORCA':'orca_type','PDPLANE':'pdplane_type'}[name]
    if hasattr(q,field):return
    for base in (0xA8B218,0xA8E390):
        if not m.read32(base):u.mem_write(base,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
    type_pointer=m.alloc(0x1900);setattr(q,field,type_pointer)
    q.phase='setup'
    m.invoke(0x41C8B0,type_pointer,(m.cstring(name),))
    art_raw=(Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])/'ARTMD.INI').read_bytes()
    art,art_lines=lexical(art_raw,{name,'FALC'})
    setattr(q,field+'_art_inputs',dict(file='ARTMD.INI',sha256=hashlib.sha256(art_raw).hexdigest(),
        inputs=art,lines=art_lines))
    m.make_ini(art)
    reads=[]
    for layer_name,path in native_owner.base.layers():
        if not path.exists():continue
        raw=path.read_bytes();selected,lines=lexical(raw,{name})
        m.rules_cache(selected)
        returned=m.invoke(0x41CC20,type_pointer,(RULES,))&255
        reads.append(dict(file=layer_name,sha256=hashlib.sha256(raw).hexdigest(),inputs=selected,
            lines=lines,reader_al=returned,locomotor=bytes(u.mem_read(type_pointer+0x34C,16)).hex(),
            flight_level=i32(u,type_pointer+0x618),speed_type=i32(u,type_pointer+0x67C)))
    setattr(q,field+'_reads',reads)
    # Keep legacy ORCA receipt keys byte-for-byte compatible.
    if field=='orca_type':
        q.orca_art_inputs=getattr(q,field+'_art_inputs');q.orca_reads=reads
    # Mission.observe owns Fly activation/OleRun as well as Drive/Ship.
    # Installing another413DE2 hook would interpret its already rewritten
    # COM stack a second time. Original6C4ACC ->4CC9A0 still executes once.
    q.phase='logic'


def initialize_chrono_unit(q, name, map_raw):
    """Add a physical UnitType to the same constructor/placement fixture."""
    import os
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.spatial_oracle.building_body_rules import RULES
    m,u=q.m,q.u
    if not hasattr(q,'reinforcement_types'):q.reinforcement_types={}
    if name in q.reinforcement_types:return q.reinforcement_types[name]
    q.phase='setup'
    typ=m.alloc(0x1900);m.invoke(0x7470D0,typ,(m.cstring(name),))
    root=Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
    art_raw=(root/'ARTMD.INI').read_bytes()
    art,art_lines=lexical(art_raw,{name,'MCV','MTNK','GTNK'})
    m.make_ini(art);layers=[]
    sources=[('RULESMD.INI',(root/'RULESMD.INI').read_bytes())]
    if (root/'LANGRULE.INI').exists():sources.append(('LANGRULE.INI',(root/'LANGRULE.INI').read_bytes()))
    sources.append(('ALL07SMD.MAP',map_raw))
    for label,raw in sources:
        selected,lines=lexical(raw,{name});m.rules_cache(selected)
        returned=m.invoke(0x747620,typ,(RULES,))&255
        layers.append(dict(file=label,sha256=hashlib.sha256(raw).hexdigest(),inputs=selected,
            lines=lines,returned_al=returned,initial_ammo=i32(u,typ+0x680),ammo=i32(u,typ+0x684),
            speed_type=i32(u,typ+0x67C),
            locomotor=bytes(u.mem_read(typ+0x34C,16)).hex()))
    q.reinforcement_types[name]=typ
    if not hasattr(q,'chrono_unit_reads'):q.chrono_unit_reads={}
    q.chrono_unit_reads[name]=dict(art_sha256=hashlib.sha256(art_raw).hexdigest(),
        art_inputs=art,art_lines=art_lines,layers=layers)
    q.phase='logic'
    return typ


def execute_chrono_reinforcement_rows(q, cases):
    """Connected original107/65EC30, using ActionFixture on this same image.

    The existing FootMissions owner keeps all constructor, type, geometry,
    allocator and platform transports. Resolved Team/Script/TaskForce records
    and a transplanted waypoint in its small physical crop are explicit priors.
    """
    from tools.spatial_oracle.trigger_action_values import ActionFixture
    m,u=q.m,q.u
    action=ActionFixture.__new__(ActionFixture);action.__dict__=m.__dict__
    action_ptr=m.alloc(0x94);buffer=m.alloc(512)
    scenario=m.read32(0xA8B230)
    rows=[]
    q.chrono_rows=rows
    def rng():return {key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
    def chain(p,offset):
        result=[]
        while p:
            result.append(i32(u,p+0x10));p=m.read32(p+offset);assert len(result)<64
        return result
    def member(p):
        interface=m.read32(p+0x674)
        return dict(native_id=i32(u,p+0x10),type=m.string(m.read32(p+(0x6C0 if m.read32(p)==0x7EB058 else 0x6C4))+0x24),
            kind=m.invoke(m.read32(m.read32(p)+0x2C),p),ammo=i32(u,p+0x2FC),
            xyz=list(struct.unpack('<3i',u.mem_read(p+0x9C,12))),
            warp_destination=list(struct.unpack('<3i',u.mem_read(p+0x288,12))),
            being_warped=bool(u.mem_read(p+0x271,1)[0]),pending=i32(u,p+0x280),delay=i32(u,p+0x284),
            mission=i32(u,p+0xAC),queued=i32(u,p+0xB4),limbo=bool(u.mem_read(p+0x81,1)[0]),
            target_id=i32(u,m.read32(p+0x2B4)+0x10) if m.read32(p+0x2B4) else None,
            object_next=bool(m.read32(p+0x30)),team_member=bool(m.read32(p+0x5D4)),
            locomotor_vtable=f'{m.read32(interface):08X}',
            locomotor_state=i32(u,interface+0x34),locomotor_piggyback=bool(m.read32(interface+0x44)),
            locomotor_destination=list(struct.unpack('<3i',u.mem_read(interface+0x18,12))),
            locomotor_marked=list(struct.unpack('<3i',u.mem_read(interface+0x24,12))),
            locomotor_moving=bool(u.mem_read(interface+0x30,1)[0]),
            locomotor_timer=[i32(u,interface+n) for n in (0x38,0x40)])
    for case in cases:
        case={'frame':i32(u,0xA8ED84),'override_cell':[88,49],**case}
        q.chrono_case=case
        prior=[]
        for index in range(i32(u,0xA8EC88)):
            p=m.read32(m.read32(0xA8EC7C)+index*4)
            prior.append(dict(native_id=i32(u,p+0x10),
                type=m.string(m.read32(p+(0x6C0 if m.read32(p)==0x7EB058 else 0x6C4))+0x24),
                xyz=list(struct.unpack('<3i',u.mem_read(p+0x9C,12))),
                owner_house_index=i32(u,m.read32(p+0x21C)+0x30),
                in_playfield=bool(u.mem_read(p+0x3D5,1)[0]),
                limbo=bool(u.mem_read(p+0x81,1)[0]),alive=bool(u.mem_read(p+0x90,1)[0]),
                being_warped=bool(u.mem_read(p+0x271,1)[0]),object_74=u.mem_read(p+0x74,1)[0]))
        typ=prepare_reinforcement_case(q,case)
        u.mem_write(scenario+0x632+6*4,packed(*case.get('override_cell',[88,49])))
        raw=case.get('action',f"107,1,{case['name']},0,0,0,0,G")
        read=action.read_action(raw,address=action_ptr,buffer=buffer)
        if 'waypoint_override' in case:u.mem_write(action_ptr+0x44,dwords(case['waypoint_override']))
        before=rng();ids_before=i32(u,scenario+0x214)
        created=[];teams=[];calls=[];placements=[];fnpc=[];writes=[];creation=None;returns={}
        entries={0x65EC30:'chrono_reinforce',0x65DD30:'create_members',0x6E8A90:'team_ctor',
            0x6EA500:'add_member',0x6EA870:'remove_member',0x7258D0:'detach_all',
            0x6C4C10:'teleport_factory',0x718000:'teleport_ctor',
            0x56DC20:'fnpc',0x481180:'subcell',0x4D3780:'mark',0x5B3570:'commence',0x660B80:'recent_cell'}
        def observe(vm,pc,_size,_data):
            nonlocal creation
            sp=vm.reg_read(UC_X86_REG_ESP);receiver=vm.reg_read(UC_X86_REG_ECX)
            if pc in returns:
                for record in returns.pop(pc):
                    record['returned_al']=vm.reg_read(UC_X86_REG_EAX)&255
                    if record.get('kind')=='fnpc':
                        record['result']=list(struct.unpack('<hh',u.mem_read(vm.reg_read(UC_X86_REG_EAX),4)))
            if pc in entries:calls.append(dict(kind=entries[pc],id=i32(u,receiver+0x10) if pc in (0x4D3780,0x5B3570) else None))
            if pc==0x6E8A90:teams.append(receiver)
            if pc==0x65DE3C and vm.reg_read(UC_X86_REG_EAX):
                pointer=vm.reg_read(UC_X86_REG_EAX);created.append(pointer)
            if pc==0x65ECCC:
                creation=dict(head=chain(vm.reg_read(UC_X86_REG_EAX),0x30),
                    team_members=chain(m.read32(teams[0]+0x54),0x5D8),
                    ammo=[i32(u,p+0x2FC) for p in created],rng=rng())
            if pc==0x56DC20:
                record=dict(kind='fnpc',origin=list(struct.unpack('<hh',u.mem_read(m.read32(sp+8),4))),
                    caller=f'{m.read32(sp):08X}',
                    args=[i32(u,sp+12+n*4) for n in range(10)]+[i32(u,sp+56),i32(u,sp+60)])
                fnpc.append(record);returns.setdefault(m.read32(sp),[]).append(record)
            if pc in (0x737BA0,0x51DFF0,0x414310):
                record=dict(id=i32(u,receiver+0x10),xyz=list(struct.unpack('<3i',u.mem_read(m.read32(sp+4),12))),
                    facing=i32(u,sp+8),creation_depth=i32(u,0xA8E7AC))
                placements.append(record);returns.setdefault(m.read32(sp),[]).append(record)
        def written(vm,_access,address,size,value,_data):
            for p in created:
                if address-p in (0x30,0x271,0x280,0x284,0x288,0x28C,0x290,0x2FC):
                    writes.append(dict(id=i32(u,p+0x10),pc=f'{vm.reg_read(UC_X86_REG_EIP):08X}',
                        offset=f'{address-p:X}',size=size,value=value))
        h=u.hook_add(UC_HOOK_CODE,observe);w=u.hook_add(UC_HOOK_MEM_WRITE,written)
        q.phase='placement';q.events.clear();q.pending.clear()
        try:
            returned=m.invoke(0x6DD8B0,action_ptr,(q.enemy,0,0,0),timeout_us=30_000_000,
                context={'case':case,'fixture':'existing FootMissions physical crop'})&255
        finally:u.hook_del(h);u.hook_del(w)
        after=rng();team=teams[0] if teams else 0
        result=dict(input=case,prior=prior,read=read,returned=returned,id_before=ids_before,id_after=i32(u,scenario+0x214),
            created_ids=[i32(u,p+0x10) for p in created],creation=creation,
            members=[member(p) for p in created],team_members=chain(m.read32(team+0x54),0x5D8) if team else [],
            team_flags=list(u.mem_read(team+0x74,17)) if team else None,
            placements=placements,fnpc=fnpc,writes=writes,calls=calls,rng_before=before,rng_after=after,
            rng_events=[event for event in q.events if event['kind'] in ('rng','raw')],
            recent_counter=i32(u,0xB04D88),recent_cell=list(struct.unpack('<hh',u.mem_read(0xB04D48+4,4))),
            creation_depth=i32(u,0xA8E7AC))
        if case.get('first_process') and created:
            result['process']=[]
            p=created[0];interface=m.read32(p+0x674)
            q.phase='logic'
            if case.get('process_target'):
                case['process_target_id']=i32(u,q.victim+0x10)
                m.invoke(0x6FCDB0,p,(q.victim,))
            for frame in case.get('process_frames',[i32(u,0xA8ED84)]):
                q.frame=frame
                u.mem_write(0xA8ED84,dwords(frame));before_process=member(p)
                process_calls=[];event_start=len(q.events)
                def process_observe(vm,pc,_size,_data):
                    if pc in (0x421EA0,0x7509E0,0x6EA870,0x7258D0):
                        process_calls.append(dict(pc=f'{pc:08X}',id=i32(u,scenario+0x214)))
                h=u.hook_add(UC_HOOK_CODE,process_observe)
                try:m.invoke(0x7192F0,0,(interface,))
                finally:u.hook_del(h)
                result['process'].append(dict(frame=frame,before=before_process,after=member(p),rng=rng(),calls=process_calls,
                    rng_events=[event for event in q.events[event_start:] if event['kind'] in ('rng','raw')]))
            u.mem_write(0xA8ED84,dwords(case['frame']))
            q.frame=case['frame']
        # Subsequent cases retain genuine live objects and native registry/RNG
        # history. Killing an already placed actor is a separate mechanism.
        q.phase='logic'
        assert hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==q.resident.code_hash
        rows.append(result)
    return rows


def script_move_inputs():
    return [dict(name='script3_head_tie',ratings=[5,5]),
            dict(name='script3_last_higher',ratings=[2,9]),
            dict(name='script3_uninitiated_infantry',ratings=[9,5],head_initiated=False),
            dict(name='script3_limbo_excluded',ratings=[2,9],last_limbo=True),
            dict(name='script3_limbo_in_scope',ratings=[2,9],last_limbo=True,depth=1),
            dict(name='script3_dead_excluded',ratings=[2,9],last_alive=False),
            dict(name='script3_zero_health_excluded',ratings=[2,9],last_health=0),
            dict(name='script3_negative_head_fallback',ratings=[-2,-1]),
            dict(name='script3_outside',ratings=[5,5],target=[127,127]),
            dict(name='script3_repeated',ratings=[5,5],first=False)]


def execute_script_move_rows(q):
    """Full6E9140 caller/6EC7D0/CoordinateMove on genuine members.

    The outside case enters TeamAI with the actual constructor flags/cursor;
    its original recalc, formation, advance-clear and dispatch all execute.
    Other leadership/liveness/formed/repeat inputs follow original recalc and
    explicitly supply a stable formed prior, so that their isolated leader
    controls reach Script3. No recalc, admission, movement, entry, FNPC,
    target-assignment or RNG return is supplied. The Rust leader replay uses
    the original picked native ID; full startup terrain is not claimed.
    """
    m,u=q.m,q.u;scenario=m.read32(0xA8B230);rows=[]
    for case in script_move_inputs():
        spawn=execute_reinforcement_rows(q,[dict(name=case['name'],entries=[['MTNK',1],['E1',1]],origin=[87,50])])[0]
        team=int(spawn['team_pointer'],16);head=m.read32(team+0x54);last=m.read32(head+0x5D8)
        assert (m.string(m.read32(head+0x6C0)+0x24),m.string(m.read32(last+0x6C4)+0x24))==('E1','MTNK')
        script=m.read32(team+0x28);q.phase='logic'
        constructor_entry=case['name']=='script3_outside'
        preparation=None
        if not constructor_entry:
            # Original Recalc derives full-strength/member flags; no answer
            # is supplied. Liveness controls below are declared stable priors.
            before_flags=list(u.mem_read(team+0x74,17))
            recalc_al=m.invoke(0x6EA3E0,team)&255
            assert recalc_al and u.mem_read(team+0x7D,1)[0]==0
            preparation=dict(entry='0x006EA3E0',returned_al=recalc_al,
                flags_before=before_flags,flags_after=list(u.mem_read(team+0x74,17)))
            first=case.get('first',True)
            u.mem_write(team+0x7F,b'\1')
            u.mem_write(team+0x80,bytes([first]))
            u.mem_write(script+0x2C,dwords(-1 if first else 0))
        target=case.get('target',[88,49]);u.mem_write(scenario+0x632+4*4,packed(*target))
        old_ratings=[bytes(u.mem_read(ptr+0x5FC,4)) for ptr in (q.e1_type,q.typ)]
        for ptr,rating in zip((q.e1_type,q.typ),case['ratings']):u.mem_write(ptr+0x5FC,dwords(rating))
        u.mem_write(head+0x689,bytes([case.get('head_initiated',True)]))
        u.mem_write(last+0x81,bytes([case.get('last_limbo',False)]))
        u.mem_write(last+0x90,bytes([case.get('last_alive',True)]))
        if 'last_health' in case:u.mem_write(last+0x6C,dwords(case['last_health']))
        u.mem_write(0xA8E7AC,dwords(case.get('depth',0)))
        before_rng={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
        prior=[dict(native_id=i32(u,p+0x10),type=m.string(m.read32(p+(0x6C0 if p==head else 0x6C4))+0x24),
            alive=u.mem_read(p+0x90,1)[0],health=i32(u,p+0x6C),limbo=u.mem_read(p+0x81,1)[0],
            initiated=u.mem_read(p+0x689,1)[0],rating=rating) for p,rating in zip((head,last),case['ratings'])]
        caller_prior=dict(formed=bool(u.mem_read(team+0x7F,1)[0]),
            altered=bool(u.mem_read(team+0x7D,1)[0]),
            advance=bool(u.mem_read(team+0x80,1)[0]),cursor=i32(u,script+0x2C))
        calls=[];leader=[];entry=[];dispatch=[];advance_clear=[]
        def observe(_u,pc,_size,_data):
            if pc==0x6E937A:
                advance_clear.append(dict(advance_before=u.mem_read(team+0x80,1)[0],
                    cl=u.reg_read(UC_X86_REG_ECX)&255,cursor_before=i32(u,script+0x2C)))
            if pc==0x6EC7D0:
                sp=u.reg_read(UC_X86_REG_ESP)
                dispatch.append(dict(advance=bool(u.mem_read(team+0x80,1)[0]),
                    cursor=i32(u,script+0x2C),first=bool(u.mem_read(sp+8,1)[0]),
                    formed=bool(u.mem_read(team+0x7F,1)[0]),
                    altered=bool(u.mem_read(team+0x7D,1)[0])))
            if pc==0x6EC859:leader.append(i32(u,u.reg_read(UC_X86_REG_EDI)+0x10))
            if pc in {0x6E9050,0x6EBAD0,0x56DC20,0x65C780,0x65C7E0}:
                calls.append(hex(pc))
            if pc==0x6EC8F8:
                sp=u.reg_read(UC_X86_REG_ESP);cell=m.read32(sp)
                entry.append(dict(native_id=i32(u,u.reg_read(UC_X86_REG_ECX)+0x10),
                    cell=list(struct.unpack('<hh',u.mem_read(cell+0x24,4))),
                    args=[i32(u,sp+n) for n in (4,8,12,16)]))
        hook=u.hook_add(UC_HOOK_CODE,observe)
        try:m.invoke(0x6E9140,team,context={'case':case,'caller_prior':caller_prior})
        finally:
            u.hook_del(hook)
            for ptr,old in zip((q.e1_type,q.typ),old_ratings):u.mem_write(ptr+0x5FC,old)
            u.mem_write(0xA8E7AC,dwords(0))
        assert len(dispatch)==1 and dispatch[0]['first']==case.get('first',True)
        assert not dispatch[0]['advance'] and dispatch[0]['cursor']==0
        assert advance_clear==([dict(advance_before=1,cl=0,cursor_before=-1)] if case.get('first',True) else [])
        code_hash=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
        assert code_hash==q.resident.code_hash
        target_ptr=m.read32(team+0x3C)
        rows.append(dict(case_kind='script_move',input=case,prior_members=prior,
            caller='0x006E9140',constructor_entry=constructor_entry,preparation=preparation,
            caller_prior=caller_prior,advance_clear=advance_clear,dispatch=dispatch[0],
            leader=leader[0] if leader else None,entry=entry,calls=calls,
            mission_target=list(struct.unpack('<hh',u.mem_read(target_ptr+0x24,4))) if target_ptr else None,
            advance=bool(u.mem_read(team+0x80,1)[0]),rng_before=before_rng,
            members_after=[dict(native_id=i32(u,p+0x10),mission=i32(u,p+0xAC),queued=i32(u,p+0xB4),
                nav_cell=list(struct.unpack('<hh',u.mem_read(m.read32(p+0x5A4)+0x24,4))) if m.read32(p+0x5A4) else None)
                for p in (head,last)],
            rng_after={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()},
            native_code_sha256=code_hash))
    return rows


def execute_placement_retry_rows(q):
    """Original retry control on an already placed genuine Unit.

    Its fresh native Spawn/Unlimbo finishes before this direct PlaceMembers
    call. This is deliberately an existing-object prior-state control, not a
    claim that normal fresh reinforcement construction reaches the refusal.
    Record exact tested candidates and cleanup.65E53A pushes before the+30/+32
    accesses, so the X+2C/Y+2E packed local is addressed at+30 after that push.
    """
    m,u=q.m,q.u
    spawned=execute_reinforcement_rows(q,[dict(name='retry_existing_unit',entries=[['MTNK',1]],origin=[87,50])])[0]
    team=int(spawned['team_pointer'],16);typ=m.read32(team+0x24);actor=m.read32(team+0x54)
    argument=m.alloc(4);u.mem_write(argument,packed(87,50));u.mem_write(actor+0x30,dwords(0))
    u.reg_write(UC_X86_REG_EDX,actor)
    attempts=[];candidates=[];cleanup=[];replies={}
    def observe(_u,pc,_size,_data):
        sp=u.reg_read(UC_X86_REG_ESP)
        if pc in replies:
            for record in replies.pop(pc):record['result']=u.reg_read(UC_X86_REG_EAX)&255
        if pc==0x737BA0:
            attempts.append(list(struct.unpack('<3i',u.mem_read(m.read32(sp+4),12))))
        if pc==0x568300:
            record=dict(cell=list(struct.unpack('<hh',u.mem_read(m.read32(sp+4),4))))
            candidates.append(record);replies.setdefault(m.read32(sp),[]).append(record)
        if pc in (0x6EA870,0x7258D0):cleanup.append(hex(pc))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    before_rng={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
    try:returned=m.invoke(0x65E010,typ,(argument,0))&255
    finally:u.hook_del(hook)
    return [dict(case_kind='placement_retry',input=dict(name='retry_existing_unit',requested=[87,50]),
        returned=returned,attempts=attempts,candidates=candidates,cleanup=cleanup,
        remaining_members=i32(u,team+0x48),creation_depth=i32(u,0xA8E7AC),rng_before=before_rng,
        rng_after={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()})]


def execute_scalar_cleanup_rows(q):
    """Original action7 zero-origin constructor-complete limbo disposal.

    Only A8E9A0/game-active and A8ED5C/scenario-started are supplied at the
    first actual class scalar entry. Alive1/limbo1, Type, Team/list/slot and
    timers are genuine constructor/AddMember outputs. This never turns a
    placed or dead queue-ready object into a scalar-disposal prior.
    """
    m,u=q.m,q.u
    # Reuse the same registry/definition preparation as every action7 row.
    execute_reinforcement_rows(q,[])
    rows=[]
    class_dtors={0x735780:'unit_dtor',0x517D90:'infantry_dtor',0x414080:'aircraft_dtor'}
    entries={**class_dtors,0x6EA870:'remove_member',0x4FF550:'remove_tracking',
             0x4DB260:'foot_limbo',0x7258D0:'removed_expiry',0x4D3590:'foot_dtor',
             0x4D3632:'cached_air_cleanup',0x6F4500:'techno_dtor',0x5F3B80:'object_dtor',
             0x5F5B50:'object_tag_replace',0x707CB0:'kill_passengers'}
    for name in ('MTNK','E1','ORCA'):
        for active in (0,1):
            for started in (0,1):
                case=dict(name=f'scalar_{name}_active{active}_started{started}',
                    entries=[[name,1]],origin=[0,0])
                # Existing FootMissions bootstrap supplies game-active1. Keep
                # construction identical; vary flags only at disposal entry.
                u.mem_write(0xA8E9A0,b'\x01');u.mem_write(0xA8ED5C,b'\x00')
                typ=prepare_reinforcement_case(q,case)
                actor=team=scalar=class_return=0
                prior=None;after_dtor=None;rng_before=None
                calls=[];observations=[]
                def rng_state():
                    return {key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
                def state():
                    return dict(alive=u.mem_read(actor+0x90,1)[0],limbo=u.mem_read(actor+0x81,1)[0],
                        member_count=i32(u,team+0x48),member_total=i32(u,team+0x4C),
                        taskforce_counts=list(struct.unpack('<6i',u.mem_read(team+0x88,24))),
                        head_present=bool(m.read32(team+0x54)),
                        head_is_actor=m.read32(team+0x54)==actor,
                        foot_has_team=m.read32(actor+0x5D4)==team,
                        foot_next_nonnull=bool(m.read32(actor+0x5D8)),
                        object_tag_present=bool(m.read32(actor+0x34)),
                        team_tag_present=bool(m.read32(team+0x70)),
                        suspended_mission=i32(u,actor+0xB0),
                        suspended_nav_present=bool(m.read32(actor+0x5A8)),
                        suspended_attack_present=bool(m.read32(actor+0x2B8)),
                        elite_flash_frames=i32(u,actor+0xF0),
                        team_timers=[i32(u,team+n) for n in (0x58,0x60,0x64,0x6C)],
                        team_flags=list(u.mem_read(team+0x74,17)))
                def observe(_u,pc,_size,_data):
                    nonlocal actor,team,scalar,class_return,prior,after_dtor,rng_before
                    receiver=u.reg_read(UC_X86_REG_ECX);sp=u.reg_read(UC_X86_REG_ESP)
                    if pc==0x6E8A90:team=receiver
                    if pc==0x65DE3C and u.reg_read(UC_X86_REG_EAX):
                        actor=u.reg_read(UC_X86_REG_EAX)
                        scalar=m.read32(m.read32(actor)+0x20)
                    if scalar and pc==scalar and receiver==actor:
                        assert prior is None and i32(u,sp+4)==1
                        prior=state()
                        prior.update(native_id=i32(u,actor+0x10),type_present=bool(m.read32(actor+(0x6C0 if name=='E1' else 0x6C4))),
                            bootstrap_game_active=u.mem_read(0xA8E9A0,1)[0],
                            bootstrap_scenario_started=u.mem_read(0xA8ED5C,1)[0])
                        u.mem_write(0xA8E9A0,bytes([active]));u.mem_write(0xA8ED5C,bytes([started]))
                        rng_before=rng_state();calls.append('scalar_dtor')
                        observations.append(dict(operation='scalar_dtor',entry=hex(pc),deleting_flag=i32(u,sp+4)))
                    if prior is None:return
                    if pc in class_dtors and receiver==actor:class_return=m.read32(sp)
                    if class_return and pc==class_return:
                        after_dtor=state();calls.append('class_dtor_return')
                    operation=entries.get(pc)
                    # Sound APIs have other Techno receivers. Capture the
                    # existing Foot sound subobject+544 only, not their calls.
                    if pc==0x406060 and receiver==actor+0x544:operation='foot_sound_release'
                    if pc==0x405C00 and receiver==actor+0x544:operation='foot_sound_dtor'
                    if pc==0x6EAE60 and receiver==team:operation='team_pointer_expired'
                    if pc==0x707D1C and receiver==actor:operation='kill_passengers_empty_return'
                    if operation:
                        calls.append(operation)
                        event=dict(operation=operation,entry=hex(pc))
                        if pc==0x6EA870:
                            event.update(member_is_actor=m.read32(sp+4)==actor,
                                argument=i32(u,sp+8),noidle=i32(u,sp+12))
                        if pc==0x4FF550:event['member_is_actor']=m.read32(sp+4)==actor
                        if pc==0x7258D0:event.update(member_is_actor=receiver==actor,removed=u.reg_read(UC_X86_REG_EDX)&255)
                        if pc==0x6EAE60:event.update(member_is_actor=m.read32(sp+4)==actor,
                            removed=m.read32(sp+8)&255,head_present=bool(m.read32(team+0x54)),
                            member_count=i32(u,team+0x48))
                        if pc in (0x707CB0,0x707D1C):event['passenger_count']=i32(u,actor+0x118)
                        observations.append(event)
                hook=u.hook_add(UC_HOOK_CODE,observe)
                q.phase='placement';q.events.clear();q.pending.clear()
                try:
                    u.reg_write(UC_X86_REG_EDX,0xFFFFFFFF)
                    returned=m.invoke(0x65D8E0,typ,context={'case':case,'scalar_flags':[active,started]})&255
                finally:u.hook_del(hook)
                assert prior is not None and after_dtor is not None,case
                assert prior['alive']==1 and prior['limbo']==1 and prior['type_present'],prior
                assert prior['member_count']==1 and prior['taskforce_counts']==[1,0,0,0,0,0],prior
                code_hash=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
                assert code_hash==q.resident.code_hash
                rows.append(dict(case_kind='scalar_cleanup',input=dict(name=case['name'],type=name,
                    game_active_byte=active,scenario_started_byte=started,origin=[0,0]),
                    returned=returned,prior=prior,after_dtor=after_dtor,after=state(),
                    calls=calls,observations=observations,rng_before=rng_before,rng_after=rng_state(),
                    native_code_sha256=code_hash))
                u.mem_write(0xA8E9A0,b'\x01');u.mem_write(0xA8ED5C,b'\x00')
    return rows


def execute_member_tag_rows():
    """Original6EA582..6EA5B0 Tag NULL/B3/Type.Passengers gate.

    Genuine E1 actors retain constructor Limbo; placement-scope1, partial
    House human bytes0/0 and Type.Passengers0/1 are supplied controls. Genuine
    Object anchors keep both empty-definition Tags referenced. No zero-ref
    destruction, detach, queue drain, cargo object or CARRIER reader is tested.
    """
    from tools.spatial_oracle.building_body_rules import RULES, SP
    q=reinforcement_fixture();m,u=q.m,q.u
    execute_reinforcement_rows(q,[])
    if not m.read32(0xB0E780):
        u.mem_write(0xB0E780,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
    u.mem_write(q.house+0x1EC,b'\0\0')
    assert (m.invoke(0x50B730,q.house)&255)==0
    def rng():
        return {key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
    def new_object():
        actor=m.alloc(0x1000)
        m.invoke(0x517A50,actor,(q.e1_type,q.house))
        return actor
    def new_tag_type(name):
        typ=m.alloc(0xA4)
        m.invoke(0x6E5B60,typ,(m.cstring(name),))
        assert m.read32(typ+0xA0)==0
        return typ
    rows=[]
    for tagged in (False,True):
        for on_trans in (False,True):
            for passengers in (0,1):
                name=f'teamtag{int(tagged)}_trans{int(on_trans)}_pass{passengers}'
                construct_rng_before=rng()
                typ=prepare_reinforcement_case(q,dict(name=name,entries=[['E1',1]],origin=[87,50]))
                m.rules_cache({name:{'OnTransOnly':'yes' if on_trans else 'no'}})
                u.reg_write(UC_X86_REG_ESP,SP)
                u.reg_write(UC_X86_REG_ESI,typ)
                u.reg_write(UC_X86_REG_EBX,RULES)
                u.reg_write(UC_X86_REG_EDI,m.cstring(name))
                run_checked(u,0x6F12B6,0x6F12D6,required_addresses=(0x5295F0,))
                assert bool(u.mem_read(typ+0xB3,1)[0])==on_trans
                if tagged:u.mem_write(typ+0xD0,dwords(new_tag_type('Team_'+name)))
                team=m.invoke(0x6F09C0,typ,(q.house,))
                assert team
                team_tag=m.read32(team+0x70)
                assert bool(team_tag)==tagged
                actor,author_anchor,team_anchor=new_object(),new_object(),new_object()
                author=m.alloc(0x38)
                m.invoke(0x6E4DE0,author,(new_tag_type('Auth_'+name),))
                m.invoke(0x5F5B50,actor,(author,))
                m.invoke(0x5F5B50,author_anchor,(author,))
                if team_tag:m.invoke(0x5F5B50,team_anchor,(team_tag,))
                u.mem_write(q.e1_type+0x5E0,dwords(passengers))
                u.mem_write(0xA8E7AC,dwords(1));q.phase='logic'
                calls=[]
                def tag_name(pointer):
                    return None if not pointer else ('author' if pointer==author else 'team' if pointer==team_tag else hex(pointer))
                def state():
                    return dict(object_tag=tag_name(m.read32(actor+0x34)),author_refs=i32(u,author+0x2C),
                        team_refs=i32(u,team_tag+0x2C) if team_tag else None,
                        foot_team=hex(m.read32(actor+0x5D4)),team_count=i32(u,team+0x48))
                def observe(_u,pc,_size,_data):
                    if pc in (0x5F5B50,0x6E5120,0x6E5160,0x6E5230,0x7258D0,0x50B730,
                              0x65C780,0x65C7E0,0x6E4F60,0x725C70):
                        sp=u.reg_read(UC_X86_REG_ESP);receiver=u.reg_read(UC_X86_REG_ECX)
                        event=dict(pc=hex(pc),receiver=tag_name(receiver) if pc in (0x6E5120,0x6E5160,0x6E5230) else hex(receiver))
                        if pc==0x5F5B50:event['new_tag']=tag_name(m.read32(sp+4))
                        calls.append(event)
                prior=state();before_rng=rng()
                hook=u.hook_add(UC_HOOK_CODE,observe)
                try:
                    added=m.invoke(0x6EA500,team,(actor,0))&255
                    assert added
                    after_add=state()
                    removed=m.invoke(0x6EA870,team,(actor,-1,0))&255
                    after_remove=state()
                finally:u.hook_del(hook)
                after_rng=rng()
                code_hash=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
                assert code_hash==q.resident.code_hash
                assert all(event['pc'] not in ('0x6e5230','0x7258d0','0x6e4f60','0x725c70') for event in calls)
                rows.append(dict(case_kind='member_tag',input=dict(name=name,nonnull_team_tag=tagged,
                    on_trans_only=on_trans,passengers=passengers,placement_scope=1,
                    actor_limbo=u.mem_read(actor+0x81,1)[0],house_human_bytes=[0,0]),
                    native_type_B3=u.mem_read(typ+0xB3,1)[0],prior=prior,added_al=added,after_add=after_add,
                    removed_al=removed,after_remove=after_remove,calls=calls,
                    construction_rng_before=construct_rng_before,construction_rng_after=before_rng,
                    member_rng_before=before_rng,member_rng_after=after_rng,native_code_sha256=code_hash))
    return rows


def execute_on_trans_only_reader():
    """Constructor6F0765 +B3=0; exact-key843028 retained read6F12B6."""
    # Local import avoids module initialization recursion: StockPrefix imports
    # only this module's numeric observers, and neither source inventory runs
    # another generator or metadata callback.
    from tools.spatial_oracle.tag_lifecycle import StockPrefix
    from tools.native_oracle import image_bytes, file_span
    s=StockPrefix(1);m,u=s.m,s.u
    typ=m.alloc(0xF8);constructor_rng_before=m.rng_state()
    m.invoke(0x6F06E0,typ,(m.cstring('RTT'),))
    constructor=u.mem_read(typ+0xB3,1)[0];constructor_rng_after=m.rng_state()
    before_rng=m.rng_state()
    values=[('OnTransOnly','yes'),('OnTransOnly','junk'),('OnTransOnly',None),('OnTransOnly',''),
        ('ontransonly','no'),('OnTransOnly','2'),('OnTransOnly','on'),('OnTransOnly','no'),
        ('OnTransOnly','bad'),('OnTransOnly','yes'),('OnTransOnly',None),('OnTransOnly','')]
    rows=[]
    for key,value in values:
        m.ordered_ini({'RTT':{} if value is None else {key:value}})
        prior=u.mem_read(typ+0xB3,1)[0]
        u.reg_write(UC_X86_REG_ESI,typ);u.reg_write(UC_X86_REG_EBX,s.ini)
        u.reg_write(UC_X86_REG_EDI,m.cstring('RTT'))
        m.region(0x6F12B6,0x6F12D6,required=(0x5295F0,))
        rows.append(dict(key=key,value=value,before=prior,after=u.mem_read(typ+0xB3,1)[0]))
    stock_type=m.alloc(0xF8)
    m.invoke(0x6F06E0,stock_type,(m.cstring('0BCF57DC'),))
    m.ordered_ini({'0BCF57DC':s.physical_map['0BCF57DC']})
    u.reg_write(UC_X86_REG_ESI,stock_type);u.reg_write(UC_X86_REG_EBX,s.ini)
    u.reg_write(UC_X86_REG_EDI,m.cstring('0BCF57DC'))
    m.region(0x6F12B6,0x6F12D6,required=(0x5295F0,))
    m.check_code()
    return dict(case_kind='on_trans_only_reader',constructor_B3=constructor,
        reader='6F12B6..6F12D6',read_bool='5295F0',key_address='843028',
        key=file_span(image_bytes(),0x843028,24)[1].split(b'\0')[0].decode('ascii'),rows=rows,
        stock=dict(id='0BCF57DC',literal=s.physical_map['0BCF57DC']['OnTransOnly'],
            tag_literal=s.physical_map['0BCF57DC'].get('Tag'),B3=u.mem_read(stock_type+0xB3,1)[0]),
        constructor_rng_before=constructor_rng_before,constructor_rng_after=constructor_rng_after,
        reader_rng_before=before_rng,reader_rng_after=m.rng_state(),rng_unchanged=before_rng==m.rng_state(),
        map_sha256=s.sources['sov01umd.map']['sha256'])


def source_paths():
    """Reuse the composed owner's dependency closure, including legacy leaves."""
    from tools.spatial_oracle.anytown_damage.foot_missions import source_paths as foot_sources
    paths=foot_sources();repo=Path(__file__).resolve().parents[2]
    for relative in ('tools/spatial_oracle/team_creation.py','tools/spatial_oracle/anytown_damage/unit_unlimbo.py','tools/spatial_oracle/infantry_entry_raw.py','tools/spatial_oracle/aircraft_fire_location.py',
                     'tools/spatial_oracle/map_queries.py','tools/rules_oracle/bridge_anim_inputs.py'):
        paths[relative]=repo/relative
    from tools.spatial_oracle.tag_lifecycle import source_paths as tag_sources
    for path in tag_sources().values():
        paths[str(path.relative_to(repo))]=path
    return paths


def metadata():
    record=provenance(
        entry_points={'trigger_execute':0x6DD8B0,'create_team':0x6F09C0,'team_constructor':0x6E8A90,
            'script_constructor':0x6913C0,'tag_constructor':0x6E4DE0,'trigger_constructor':0x725FA0,
            'team_waypoint_predicate':0x6EC300,'script3':0x6EC7D0,'coordinate_move':0x6EBAD0,
            'team_ai_caller':0x6E9140,'team_recalc':0x6EA3E0,
            'script50_dispatch':0x6E9CF7,'signed_countdown':0x4CC770,'recent_cell':0x660B80,
            'reinforcement_spawn':0x65D8E0,'create_members':0x65DD30,'place_members':0x65E010,
            'process_entry':0x7CD80F,'process_crt':0x7CBDAF,'initializer_loop':0x7CBED3,
            'subcell_crt_pointer':0x812B28,'subcell_startup':0x48E480,
            'aircraft_type_constructor':0x41C8B0,'aircraft_type_reader':0x41CC20,
            'aircraft_constructor':0x413D20,'fly_factory':0x6C4A90,'fly_constructor':0x4CC9A0,
            'unit_destructor':0x735780,'infantry_destructor':0x517D90,
            'aircraft_destructor':0x414080,'foot_destructor':0x4D3590,
            'team_remove_member':0x6EA870,'house_remove_tracking':0x4FF550,
            'team_pointer_expired':0x6EAE60,
            'team_type_constructor':0x6F06E0,'on_trans_only_constructor_write':0x6F0765,
            'on_trans_only_read':0x6F12B6,'on_trans_only_read_end':0x6F12D6,
            'on_trans_only_key':0x843028,'read_bool':0x5295F0,'team_add_member':0x6EA500,
            'tag_type_constructor':0x6E5B60,'object_tag_replace':0x5F5B50,
            'tag_add_ref':0x6E5120,'tag_release':0x6E5160,'tag_mark_delete':0x6E5230,
            'kill_passengers':0x707CB0,'kill_passengers_empty_return':0x707D1C,
            'removed_expiry':0x7258D0,'foot_sound_release':0x406060,
            'game_active_byte':0xA8E9A0,'scenario_started_byte':0xA8ED5C},
        assumptions=[
            'Legacy39: resolved Team/Script/Tag/Trigger definitions, two registered houses and optional prior teams; spare-capacity registries; original static zero initializers and Scenario RNG seed. Their canonical payload remains0cbe504881d4911878172bf407a3c5f5522d8f3516efec882f2820c658c9b448.',
            'waypoint14 and elite_flash21: explicitly supplied prior Team/script/member fields; original6EC300/Scenario702-slot lookup/Map mode1 and original script50 fragment/full signed countdown execute. No complete constructor/TeamAI/TechnoAI claim from these controls.',
            'recent_cells4: pure original660B80 ring pushes including14 calls and wrap; compare observable review ordering, not Rust/native physical-slot rotation. Full reinforcement rows independently establish allied success, non-allied success and zero-origin cleanup eligibility.',
            'reinforcement10 and script_move10: existing FootMissions physical RULESMD/LANG/mode/map reader order, MTNK/E1 constructors and actual crop; added physical ORCA AircraftType reader and original Fly factory. Before companion construction/placement the original7CBED3 initializer loop executes its actual table slot812B28 ->48E480, producing all Cell offsets at89E9F0. Original PE entry7CD80F calls7CBDAF, which passes the containing [812000,815DA4) table before WinMain; only this required slot executes here. Before/after native bytes, unchanged three RNG streams and original code hash are recorded. Cold-offset research outputs are superseded, not fitted into Rust.',
            'Resolved TeamType/ScriptType/TaskForce definitions, authored edge/current house/waypoint are explicit inputs. Empty Team/Script/planning and Aircraft/House count registries receive declared spare storage.',
            'Script3 outside control executes full original6E9140 from genuine action7 constructor flags/cursor; native recalc, formation and pending1/cursor-1 -> pending0/cursor0/first1 dispatch execute. Other nine controls run original6EA3E0 before explicitly supplying formed/first-or-repeat and signed leadership/alive/health/limbo/init/creation-scope priors. Full6E9140/6EC7D0, class entry, FNPC and CoordinateMove execute; observed6E937A CL0 clear and handler-entry state are recorded. Earlier direct-handler outputs with uncleared constructor+80 are superseded. These inputs pin the selected admission/leader/gate/order/RNG behavior on this crop, not arbitrary SOV01 terrain or all TeamAI branches.',
            'placement_retry1 is a direct call with an already placed genuine Unit; fresh normal reinforcement refusal is not established. Original8 XY neighbors and inverted Size gate execute before original scalar cleanup; no admission or cleanup return is supplied.',
            'scalar_cleanup12: one genuine MTNK/E1/ORCA constructor/AddMember per full action7 zero-origin refusal. Existing bootstrap game-active1/scenario-started0 is retained throughout construction; only A8E9A0 byte0/1 and A8ED5C byte0/1 are supplied at the first actual class scalar entry. Prior alive1/limbo1, nonnull Type, Team/list/slot and timers are observed original outputs. Class scalar(flag1), RemoveMember(-1,0), House tracking, removed=true7258D0, selected Team6EAE60 callback and Foot cached-air/sound/destructor order execute; ORCA also calls original KillPassengers707CB0 before member release. Its genuine constructor-empty Techno+118 count0 and actual707D1C early return are observed; nonempty passenger teardown is excluded. All three RNG streams and selected Team timers/elite-flash/suspension fields are captured at scalar entry and action return; this excludes constructor RNG draws. No Object/Team Tag is installed, so these are not tagged-object reference parity controls. Live placed direct scalar, whole game-state transitions and save/load are excluded. With game-active0, removed expiry drops the native linked head while stored Team count/slot1 and FootTeam remain; these fields are recorded separately. after_dtor reads retained emulated allocation bytes at actual class-dtor return before inherited deallocation, not a valid live object. The previous99 canonical payload remains da80e80715f0d2c16398c81c175675b94bff8e6a782c3a3343af271f908bff3c.',
            'member_tag8: genuine TeamType/TagType/Team/Script/Tag/E1 constructors on the existing physical FootMissions owner. Resolved empty Tag definitions, placement-scope A8E7AC=1, original E1 constructor Limbo, partial House human bytes0/0 (real50B730 false) and Type.Passengers+5E0 values0/1 are declared priors; this is not a CARRIER rule read or real passenger occupancy. The original OnTransOnly6F12B6..6F12D6 field reader selects B3 from supplied yes/no inputs. AddMember6EA582..6EA5B0 executes its nonnull-Team-Tag, B3 and member Type.Passengers gates, then original5F5B50/refcount callbacks; RemoveMember6EA870 executes its matching-tag/nonhuman release. Author/team Object anchors retain references so zero-ref deletion, pointer detach, destructors and pending-delete drain are excluded and monitored as absent. Construction RNG includes three genuine E1 actors; separate mutation RNG before/after covers only AddMember/RemoveMember. No whole TeamAI, tagged scene teardown or tagged stock opening equivalence is claimed.',
            'on_trans_only_reader1: existing read-only StockPrefix/CampaignReader bootstrap, original TeamType6F06E0 constructor B3=0 writer6F0765, original exact-case OnTransOnly literal843028 and ReadBool5295F0 block6F12B6..6F12D6. Twelve sequential supplied physical-cache transactions cover recognized, malformed, missing, empty and wrong-case keys with the previously retained B3 as default. Constructor and reader RNG states are separate. The stock0BCF57DC pass takes literal OnTransOnly/Tag data from physical sov01umd.map bytes, but only the OnTransOnly field block executes here; full stock Tag resolution remains with tag_lifecycle. Full TeamType reader/scenario loading are excluded. The previous111 canonical payload remains04815ae6b318af5af403425f32c8358ea8c1e81389b97a4979e12c0d241c7db0.',
            'Full/Droppod/all-Tunnel transports, missing factory allocation, complete definition/file loading, stock campaign startup, arbitrary off-map retained Dummy targets, recruitment/other TeamAI branches and whole scene parity are excluded.'
        ],
        substitutions=[
            'Legacy allocation boundary: operator-new7C8E17 supplies bounded poisoned storage; all original gameplay instructions execute.',
            'Composed rows inherit the existing FootMissions platform/asset/INI transport boundaries and partial supplied House/crop. Parsed Drive/Walk/Fly COM GUIDs route to original factories; OleRun/atomics/CRT/clock/asset IO and radar tracker transports remain supplied as that owner declares. No Team member, Type factory, Unlimbo, CanEnter, FNPC, movement or scalar cleanup return is supplied.'
        ],
        scope='39 retained constructor/Trigger4 rows,14 waypoint predicate controls,21 signed flash/write/countdown controls,4 recent-cell histories,10 genuine normal reinforcement cases with original Cell geometry startup,10 full TeamAI/Script3 leader/entry/FNPC/move controls (one genuine constructor admission and nine declared formed priors),1 existing-Unit retry control,12 actual zero-origin constructor-complete limbo scalar cleanup controls,8 anchored member Tag gate/refcount controls and1 OnTransOnly constructor/retained-reader aggregate. Coverage and supplied prior state are separated by case_kind; full campaign/Team/scene equivalence is not claimed.',
    )
    record['command']='python -m tools.spatial_oracle.team_creation --check'
    record['required_inputs']={'VERA20K_ANYTOWN_INPUTS':'existing extracted Anytown owner corpus',
        'VERA20K_SHRAPNEL_INPUTS':'existing physical retail RULESMD.INI/ARTMD.INI and mode/map source owner',
        'VERA20K_CAMPAIGN_START_ASSETS':'existing extracted retail sov01umd.map campaign source owner',
        'VERA20K_TAG_LIFECYCLE_ASSETS':'existing extracted aimd.ini/evamd.ini for read-only StockPrefix bootstrap'}
    return record


def full_transport_generate():
    """Selected complete65DD30 plus two scalar-placement boundaries.

    Existing real MTNK/E1 constructors are retained. Passengers16 is an
    explicit MTNK Type prior, not a YHVR retail read or hover topology claim.
    No constructor, cargo operation, class deletion or member call is a sink.
    """
    rows=[]
    for stage in ('members','null_placement','scalar_scope1'):
        q=reinforcement_fixture();m,u=q.m,q.u
        initialize_aircraft(q)
        for entry in (0x6E8A20,0x6F0610):m.invoke(entry,0)
        for base in (*REGISTRIES.values(),0xA8ECA0,0xAC4C78):
            if not m.read32(base):u.mem_write(base,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        u.mem_write(q.typ+0x5E0,dwords(16))
        u.mem_write(0xA8E9A0,b'\1')
        case=dict(name='Full transport control',full=True,entries=[('E1',3),('MTNK',1)],
            origin=[0,0],script_steps=[(3,4),(8,2)],stage=stage)
        typ=prepare_reinforcement_case(q,case)
        created=[];teams=[];calls=[]
        def observe(vm,pc,_size,_data):
            if pc==0x65DE3C and vm.reg_read(UC_X86_REG_EAX):
                created.append(vm.reg_read(UC_X86_REG_EAX))
            elif pc==0x6E8A90:teams.append(vm.reg_read(UC_X86_REG_ECX))
            if pc in (0x4733A0,0x473430,0x4FF550,0x6EA870,0x7258D0,0x65C780,0x65C7E0):
                calls.append(f'{pc:08X}')
        scenario=m.read32(0xA8B230)
        id_before=i32(u,scenario+0x214)
        before={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
        hook=u.hook_add(UC_HOOK_CODE,observe)
        try:
            head=m.invoke(0x65DD30,typ)
            def chain(pointer,offset):
                result=[]
                while pointer:
                    result.append(i32(u,pointer+0x10))
                    pointer=m.read32(pointer+offset);assert len(result)<32
                return result
            cargo_before=chain(m.read32(head+0x118),0x30)
            team=teams[0]
            members_before=chain(m.read32(team+0x54),0x5D8)
            creation_rng={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
            create_count=i32(u,scenario+0x214)
            returned=None
            if stage=='null_placement':
                point=m.alloc(4);u.mem_write(point,dwords(0));u.reg_write(UC_X86_REG_EDX,head)
                returned=m.invoke(0x65E010,typ,(point,0))&255
            elif stage=='scalar_scope1':
                u.mem_write(0xA8E7AC,dwords(1))
                scalar=m.read32(m.read32(head)+0x20)
                m.invoke(scalar,head,(1,))
            present=[]
            for pointer in created:
                base=REGISTRIES['abstract']
                present.append(pointer in [m.read32(m.read32(base+4)+index*4)
                    for index in range(i32(u,base+16))])
            rows.append(dict(input=case,created_ids=[i32(u,p+0x10) for p in created],
                head_id=i32(u,head+0x10),cargo_before=cargo_before,members_before=members_before,
                created_present=present,members_after=chain(m.read32(team+0x54),0x5D8),
                returned=returned,creation_depth=i32(u,0xA8E7AC),
                id_before=id_before,id_after=create_count,rng_before=before,
                rng_after_creation=creation_rng,
                rng_after={key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()},
                calls=calls))
        finally:u.hook_del(hook)
        assert hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==q.resident.code_hash
    return rows


def full_transport_metadata():
    return provenance(
        scope='Three bounded Full65DD30/cargo4733A0 controls and original null65E010/scope1 Unit scalar cleanup boundaries',
        assumptions=[
            'Existing FootMissions original MTNK/E1 type/weapon readers and concrete constructors/crop/House transport are reused. MTNK Passengers+5E0=16 is a declared prior, not a retail YHVR or complete Rules Process result.',
            'Resolved TeamType/Script/TaskForce priors explicitly supply Full1, threeE1 then oneMTNK, script3/8, original House and zero waypoint. No campaign startup or Full hover movement/rendering claim.',
            'Members case calls whole65DD30; null-placement subsequently calls whole65E010 at Cell0 with scope0; scalar_scope1 directly calls the genuine Unit scalar destructor with A8E7AC1. That third row establishes its gate, not a reachable whole refusal history.',
            'Created IDs, cargo/member head order, registry presence, scalar call order and three RNG streams are native outputs; freed allocation bytes are used only for retained native ID identity, never live object state.',
            'Storage0 and non-Naval type priors leave Full storage fill and aircraft/native naval transport selection outside these controls. Production ALL01 rules and those source branch decisions need their own narrow integration checks.'],
        substitutions=[
            'Inherited FootMissions platform/asset/INI transports and declared prior type/Team fields only; original create65DD30, Cargo4733A0/473430, placement65E010, constructors, membership, tracking and class scalar destructors execute unchanged.'],
        entry_points=dict(create_members=0x65DD30,cargo_add=0x4733A0,
            cargo_remove_head=0x473430,place_members=0x65E010,unit_destructor=0x735780))


def paradrop_transport_generate():
    """Whole65D8E0 Droppod branch on real Team/member/PDPLANE objects."""
    from tools.spatial_oracle.building_body_rules import RULES, SP
    rows=[]
    for use_origin in (True,False):
        q=reinforcement_fixture();m,u=q.m,q.u
        execute_reinforcement_rows(q,[])
        initialize_aircraft(q,'PDPLANE')
        format_calls=m.install_waypoint_character_format_transport()
        case=dict(name='Droppod transport control',entries=[['E1',2]],origin=[87,50],
            droppod=True,use_transport_origin=use_origin,transport_waypoint=5,
            transport_origin=[86,49],edge=0)
        case['transport_waypoint_letters']=m.string(m.invoke(0x763610,5))
        typ=prepare_reinforcement_case(q,case)
        created=[];teams=[];planes=[];calls=[]
        def observe(vm,pc,_size,_data):
            if pc==0x65DE3C and vm.reg_read(UC_X86_REG_EAX):created.append(vm.reg_read(UC_X86_REG_EAX))
            elif pc==0x6E8A90:teams.append(vm.reg_read(UC_X86_REG_ECX))
            elif pc==0x65DBE5 and vm.reg_read(UC_X86_REG_EAX):planes.append(vm.reg_read(UC_X86_REG_EAX))
            if pc in (0x413D20,0x414310,0x4733A0,0x41B870,0x4AA440,0x65C780,0x65C7E0):
                calls.append(f'{pc:08X}')
        def rng():return {key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
        def chain(pointer,offset):
            result=[]
            while pointer:
                result.append(i32(u,pointer+0x10));pointer=m.read32(pointer+offset);assert len(result)<32
            return result
        scenario=m.read32(0xA8B230);before=rng();id_before=i32(u,scenario+0x214)
        hook=u.hook_add(UC_HOOK_CODE,observe);q.phase='placement';q.events.clear();q.pending.clear()
        try:
            u.reg_write(UC_X86_REG_EDX,0xFFFFFFFF)
            returned=m.invoke(0x65D8E0,typ)&255
        finally:u.hook_del(hook)
        assert len(planes)==1 and len(teams)==1 and len(created)==2
        plane,team=planes[0],teams[0];created.append(plane)
        target=m.read32(plane+0x2B4)
        rows.append(dict(case_kind='droppod_transport',input=case,returned=returned,
            id_before=id_before,id_after=i32(u,scenario+0x214),created_ids=[i32(u,p+0x10) for p in created],
            plane=dict(native_id=i32(u,plane+0x10),xyz=list(struct.unpack('<3i',u.mem_read(plane+0x9C,12))),
                mission=i32(u,plane+0xAC),queued=i32(u,plane+0xB4),limbo=bool(u.mem_read(plane+0x81,1)[0]),
                mission_only=bool(u.mem_read(plane+0x3D4,1)[0]),team_present=bool(m.read32(plane+0x5D4)),
                nav_present=bool(m.read32(plane+0x5A4)),target_cell=list(struct.unpack('<hh',u.mem_read(target+0x24,4))),
                cargo=chain(m.read32(plane+0x118),0x30),payload_latch=bool(u.mem_read(plane+0x6C9,1)[0])),
            members=chain(m.read32(team+0x54),0x5D8),member_limbo=[bool(u.mem_read(p+0x81,1)[0]) for p in created[:-1]],
            recent_counter=i32(u,0xB04D88),recent_cell=list(struct.unpack('<hh',u.mem_read(0xB04D48+4,4))),
            creation_depth=i32(u,0xA8E7AC),rng_before=before,rng_after=rng(),calls=calls,
            plane_rules_reads=q.pdplane_type_reads,plane_art_inputs=q.pdplane_type_art_inputs,
            windows_character_formats=list(format_calls)))
        assert hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==q.resident.code_hash

    # Two newly represented inputs use their original retained-field readers.
    # Reuse one concrete TeamType and physical lexical-cache transport; no
    # Boolean, ToLetters, ReadString or FromLetters output is supplied.
    q=reinforcement_fixture();m,u=q.m,q.u;execute_reinforcement_rows(q,[])
    format_calls=m.install_waypoint_character_format_transport()
    typ=prepare_reinforcement_case(q,dict(name='TRANSPORT_INPUT',entries=[['E1',1]],origin=[87,50]))
    transactions=[{'UseTransportOrigin':'yes','TransportWaypoint':'EN'}, {},
        {'UseTransportOrigin':'junk'}, {'UseTransportOrigin':'no','TransportWaypoint':'CI'}]
    def rng():return {key:bytes(u.mem_read(ptr,0x3F4)).hex() for key,ptr in q.resident.rngs.items()}
    def state():
        value=i32(u,typ+0xD8);letters=m.string(m.invoke(0x763610,value&0xFFFFFFFF))
        return dict(use_transport_origin=bool(u.mem_read(typ+0xB1,1)[0]),transport_waypoint=value,letters=letters)
    for fields in transactions:
        format_start=len(format_calls)
        before=state();rng_before=rng();m.rules_cache({'TRANSPORT_INPUT':fields})
        for lo,hi,required in ((0x6F121A,0x6F123A,(0x5295F0,)),
                (0x6F1446,0x6F1479,(0x763610,0x528A10))):
            u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,typ)
            u.reg_write(UC_X86_REG_EBX,RULES);u.reg_write(UC_X86_REG_EDI,m.cstring('TRANSPORT_INPUT'))
            run_checked(u,lo,hi,required_addresses=required)
        rows.append(dict(case_kind='transport_input',fields=fields,before=before,after=state(),
            rng_before=rng_before,rng_after=rng(),
            windows_character_formats=format_calls[format_start:]))
    assert hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()==q.resident.code_hash
    return rows


def paradrop_transport_metadata():
    return provenance(scope='Two whole original65D8E0 Droppod transport controls and four retained TeamType input reads',
        assumptions=[
            'Existing original MTNK/E1/ORCA/PDPLANE type readers, concrete constructors, partial House and physical FootMissions crop are reused. Resolved TeamType/TaskForce priors are twoE1, Droppod1, waypoint87/50, TransportWaypoint5 at86/49 and UseTransportOrigin1/0. This is not actual ENGINEER/INIT TaskForce construction, full ALL01 map, scene/rendering or complete rules registry parity.',
            'Whole65D8E0 executes original65DD30, PDPLANE41CAA0/type factory, origin selection, class mission/target/destination/Unlimbo, Cargo4733A0 and Commence. No gameplay receiver result is supplied; same-frame aircraft flight AI and later drops are outside these creation controls.',
            'Four reader controls retain one genuinely constructed TeamType+B1/D8 and execute original exact-case ReadBool and ToLetters/ReadString64/FromLetters blocks with physical-cache transactions. ToLetters calculates characters in original instructions; only Windows wsprintfA at763635/763671 is supplied for its exact %c/%c%c format arguments, with per-row calls/arguments/bytes recorded. IDs, members/cargo, plane mission/position/targets, review admission and three RNG states are native outputs.',
            'Native Aircraft+6C9 payload latch is observed but remains the established paradrop module residual: selected retail PDPLANE is unselectable and missions26/27 do not consume its fire gate.'],
        substitutions=['Inherited existing platform/asset/INI/partial House/crop boundary and resolved Team definition inputs; Reader supplies the unresolved USER32!wsprintfA IAT7E14B0 at the two original ToLetters character-format call sites. Each input/output is retained; cdecl caller stack cleanup executes. No constructor, cargo, placement, mission, origin, numeric ToLetters arithmetic or field-reader return is supplied.'],
        entry_points=dict(spawn=0x65D8E0,create_members=0x65DD30,plane_find=0x41CAA0,
            aircraft_reader=0x41CC20,transport_origin_read=0x6F121A,transport_waypoint_read=0x6F1446,
            transport_waypoint_get=0x6F18E0,cargo_add=0x4733A0,commence=0x41B870))


if __name__=='__main__':
    import sys
    args=sys.argv[1:]
    if '--full-transport' in args:
        args.remove('--full-transport')
        finish_vectors(full_transport_generate,Path(__file__).with_name('team_full_transport.json'),
            provenance=full_transport_metadata,source_paths=source_paths(),argv=args)
    elif '--paradrop-transport' in args:
        args.remove('--paradrop-transport')
        finish_vectors(paradrop_transport_generate,Path(__file__).with_name('team_paradrop_transport.json'),
            provenance=paradrop_transport_metadata,source_paths=source_paths(),argv=args)
    else:
        finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata,source_paths=source_paths())
