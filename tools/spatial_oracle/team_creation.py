"""Original trigger-4 / TeamType creation and Team/Script/Tag constructors.

Acceptance: execute original owner resolution, limit checks, constructors and
registry writes. Observe the empty Team's retained state, initial waypoint,
script cursor, per-instance Tag/Trigger state and RNG continuation. Only native
operator-new's storage allocation is supplied; no gameplay call is replaced.
This does not execute recruitment, Team AI, trigger spring or deletion.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import Fixture, SCENARIO
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
    return rows


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        entry_points={'trigger_execute':0x6DD8B0,'create_team':0x6F09C0,'team_constructor':0x6E8A90,
                      'script_constructor':0x6913C0,'tag_constructor':0x6E4DE0,'trigger_constructor':0x725FA0},
        assumptions=['Resolved TeamType/ScriptType/TagType/TriggerType and waypoint inputs, two registered houses and optional prior teams.',
                     'All registry arrays have spare capacity; vector growth and memory exhaustion are excluded.',
                     'Original static initializers6E8A20/6F0610 establish the zero cell constants; original Scenario RNG is seeded.',
                     'Tag trigger event lists contain zero or one event; original event13/51 timer reset and RNG execute.'],
        substitutions=['Operator-new7C8E17 supplies bounded poisoned arena storage. No gameplay call, return value or instruction is replaced.'],
        scope='Original full TeamType create and Team/Script/Tag/Trigger constructors, optionally through TriggerAction4 dispatch. Pins owner/limit/depth behavior, initial retained state, publication, waypoint and Tag timer/RNG effects. Excludes definition loading, trigger spring, recruitment/activation/AI and deletion.',
    ))
