"""Original live Tag/Trigger construction, dispatch, timers and attachments.

Only operator-new storage is supplied. Original predicates, actions, RNG,
reference updates and detach/deferred-enqueue instructions execute unchanged.
Resolved definitions and spare registry capacity are fixture inputs; this does
not certify scenario loading, Team AI or final destruction.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import Fixture, SCENARIO, cell
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.team_creation import u32, i32

HOUSE, HOUSE_TYPE, TAG_TYPE = [SCRATCH+n for n in (0x20000, 0x30000, 0x40000)]
TRIGGER_TYPES = [SCRATCH+0x41000+n*0x100 for n in range(8)]
EVENTS, ACTIONS, CONTROL = [SCRATCH+n for n in (0x43000, 0x46000, 0x49000)]
HEAP, HEAP_END = SCRATCH+0x60000, SCRATCH+0x80000
REGISTRIES = {
    'abstract':0xB0F670, 'object_base':0xB0F618, 'tag':0xB0E720,
    'tag_trigger_base':0xB0F708, 'trigger':0xA8EAE8, 'pending':0xB0F698,
    'logic_tags':0x8B40C8, 'map_tags':0x8B41A8, 'cell_tags':0x880944,
}


def execute(case):
    f = Fixture(dict(seed=case.get('seed',31), include_self=False))
    u = f.u
    # CRT dynamic_cast in DetachAll installs an SEH frame at fs:[0]. Supply
    # the empty thread exception chain; its original RTTI body still runs.
    u.mem_map(0,0x1000)
    u.mem_write(0,dwords(0xFFFFFFFF))
    u.mem_write(0xA8ED84, dwords(case.get('frame',123)))
    u.mem_write(0xA8ED6B, b'\0')
    u.mem_write(SCENARIO+0x60C, dwords(case.get('difficulty',1)))
    for index, base in enumerate(REGISTRIES.values()):
        u.mem_write(base+4, dwords(SCRATCH+0x50000+index*0x400,128,0,0,128))
    # Original constructor/static-initializer vtables:565157,4E7F4D,4E7FCD.
    # Detach invokes their real Find bodies even when the vectors are empty.
    u.mem_write(REGISTRIES['cell_tags'],dwords(0x7E3890))
    u.mem_write(REGISTRIES['map_tags'],dwords(0x7EA5A4))
    u.mem_write(REGISTRIES['logic_tags'],dwords(0x7EA5A4))
    u.mem_write(0xA8022C, dwords(SCRATCH+0x4F000))
    u.mem_write(0xA80238, dwords(1))
    u.mem_write(SCRATCH+0x4F000, dwords(HOUSE))
    u.mem_write(HOUSE+0x34, dwords(HOUSE_TYPE))
    u.mem_write(HOUSE_TYPE+0xB4, dwords(0,0))
    definitions = case.get('triggers',[dict(events=[[47,0]], actions=[[28,7]])])
    u.mem_write(TAG_TYPE+0x9C, dwords(case.get('repeat',2), TRIGGER_TYPES[0] if definitions else 0))
    event_cursor, action_cursor = EVENTS, ACTIONS
    for index, definition in enumerate(definitions):
        typ = TRIGGER_TYPES[index]
        u.mem_write(typ+0x9C, bytes(definition.get('flags',[1,1,1,1])))
        u.mem_write(typ+0xA4, dwords(HOUSE_TYPE, TRIGGER_TYPES[index+1] if index+1<len(definitions) else 0))
        events = definition.get('events',[])
        actions = definition.get('actions',[])
        u.mem_write(typ+0xAC, dwords(event_cursor if events else 0, action_cursor if actions else 0))
        for n, event in enumerate(events):
            u.mem_write(event_cursor+0x28, dwords(event_cursor+0x60 if n+1<len(events) else 0,event[0],0,event[1]))
            event_cursor += 0x60
        for n, action in enumerate(actions):
            u.mem_write(action_cursor+0x28, dwords(action_cursor+0xA0 if n+1<len(actions) else 0,action[0]))
            u.mem_write(action_cursor+0x90, dwords(action[1]))
            if len(action)>2:
                u.mem_write(action_cursor+0x50, dwords(TRIGGER_TYPES[action[2]]))
            action_cursor += 0xA0
    cursor = HEAP
    allocations, actions_run, draws, calls, tags = [], [], [], [], []
    def observe(_u, pc, _size, _data):
        nonlocal cursor
        if pc == 0x65C837:
            draws.append(1)
        if pc in (0x6E4DE0,0x725FA0,0x726400,0x726720,0x7258D0):
            calls.append(f'{pc:08X}')
        if pc == 0x6DD8B0:
            action = u.reg_read(UC_X86_REG_ECX)
            actions_run.append([i32(u,action+0x2C),i32(u,action+0x90)])
        if pc != 0x7C8E17:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        size = u32(u,sp+4)
        pointer = cursor
        cursor += (max(size,1)+15)&~15
        assert cursor < HEAP_END
        u.mem_write(pointer,b'\xA5'*max(size,1))
        allocations.append(size)
        u.reg_write(UC_X86_REG_EAX,pointer)
        u.reg_write(UC_X86_REG_ESP,sp+4)
        u.reg_write(UC_X86_REG_EIP,u32(u,sp))
    hook = u.hook_add(UC_HOOK_CODE,observe)
    ranges = [(0x6E4DE0,0x6E5AA0),(0x725FA0,0x726920),(0x71E940,0x71FA40),
              (0x6DD8B0,0x6E45D0),(0x689670,0x689A90)]
    code = [bytes(u.mem_read(a,b-a)) for a,b in ranges]
    history = []
    def snapshot(returned):
        tag_states = []
        for tag in tags:
            instances, trigger = [], u32(u,tag+0x28)
            while trigger:
                instances.append(dict(type=TRIGGER_TYPES.index(u32(u,trigger+0x24)),
                    enabled=u.mem_read(trigger+0x44,1)[0],pending=u.mem_read(trigger+0x30,1)[0],
                    start=i32(u,trigger+0x34),duration=i32(u,trigger+0x3C),completed=u32(u,trigger+0x40)))
                trigger = u32(u,trigger+0x28)
                assert len(instances)<=len(definitions)
            tag_states.append(dict(refs=i32(u,tag+0x2C),pending=u.mem_read(tag+0x34,1)[0],triggers=instances))
        pending_base = REGISTRIES['pending']
        pending = [u32(u,u32(u,pending_base+4)+n*4) for n in range(i32(u,pending_base+16))]
        return dict(returned=returned,tags=tag_states,actions=list(actions_run),rng_draws=len(draws),
            pending_count=len(pending),pending_tags=[tags.index(p) for p in pending if p in tags],
            globals=[n for n in range(50) if u.mem_read(SCENARIO+0x1CB0+n*41,1)[0]],
            locals=[n for n in range(100) if u.mem_read(SCENARIO+0x24B2+n*41,1)[0]],
            changed=u.mem_read(SCENARIO+0x34AA,1)[0])
    for op in case.get('operations',[['shared'],['dispatch',0,13],['dispatch',0,13]]):
        kind, *args = op
        returned = None
        if kind in ('shared','fresh'):
            if kind=='shared':
                tag = f.call(0x6E52A0,TAG_TYPE,[])
            else:
                tag = cursor
                cursor += 0x40
                u.mem_write(tag,b'\xA5'*0x38)
                f.call(0x6E4DE0,tag,[TAG_TYPE])
            if tag not in tags:
                tags.append(tag)
            returned = tags.index(tag)
        elif kind=='frame':
            u.mem_write(0xA8ED84,dwords(args[0]))
        elif kind=='attach_cell':
            f.call(0x485250,cell(args[1],args[2]),[tags[args[0]]])
        elif kind=='dispatch':
            tag_index,event = args[:2]
            xy = args[2] if len(args)>2 else [0,0]
            force = args[3] if len(args)>3 else 0
            returned = f.call(0x6E53A0,tags[tag_index],[event,0,int.from_bytes(packed(*xy),'little'),force,0])&255
        elif kind=='global' or kind=='local':
            returned = f.call(0x689670 if kind=='global' else 0x689910,SCENARIO,args)&255
        elif kind=='action':
            action_kind,typ = args
            u.mem_write(CONTROL+0x2C,dwords(action_kind))
            u.mem_write(CONTROL+0x50,dwords(TRIGGER_TYPES[typ]))
            returned = f.call(0x6DD8B0,CONTROL,[HOUSE,0,0,0])&255
        elif kind=='mark_tag':
            f.call(0x6E5230,tags[args[0]],[])
        else:
            raise ValueError(op)
        history.append(snapshot(returned))
    u.hook_del(hook)
    assert code == [bytes(u.mem_read(a,b-a)) for a,b in ranges]
    return dict(input=case,history=history,allocations=allocations,calls=calls,
                next_random=f.call(0x65C780,SCENARIO+0x218,[]))


def inputs():
    rows = [dict(name='repeat'),dict(name='empty',triggers=[]),
            dict(name='sharing',operations=[['shared'],['shared'],['fresh'],['shared'],['fresh']]),
            dict(name='pending_shared',operations=[['shared'],['mark_tag',0],['shared'],['dispatch',0,13]]),
            dict(name='reverse_chain',triggers=[dict(events=[[47,0]],actions=[[28,7]]),
                                              dict(events=[[47,0]],actions=[[28,8]])])]
    rows += [dict(name=f'mode_{mode}_refs_{refs}',repeat=mode,
                  operations=[['shared']]+[['attach_cell',0,50+n,60] for n in range(refs)]+
                             [['dispatch',0,13,[50,60]],['dispatch',0,13,[51,60]]])
             for mode in (0,1,2,3) for refs in (0,1,2)]
    rows += [dict(name=f'timer_{event}',triggers=[dict(events=[[event,2]],actions=[[28,7]])],
                  operations=[['shared'],['dispatch',0,13],['frame',153],['dispatch',0,13],
                              ['frame',183],['dispatch',0,51]]) for event in (13,51)]
    rows += [dict(name=f'{kind}_resets_{event}',triggers=[dict(events=[[event,7],[51,6]],actions=[])],
                  operations=[['shared'],['fresh'],['frame',170],[kind,7,1],[kind,7,1],[kind,8,1],
                              [kind,7,0],['action',54,0],[kind,7,1],['action',53,0]])
             for kind, event in (('global',27),('global',28),('local',36),('local',37))]
    rows += [dict(name=f'force_enabled_{enabled}',triggers=[dict(flags=[1,1,1,enabled],events=[[47,999]],actions=[[28,7]])],
                  operations=[['shared'],['fresh'],['action',22,0],['dispatch',0,13],
                              ['action',53,0],['action',22,0]]) for enabled in (0,1)]
    rows += [dict(name=f'difficulty_{difficulty}',difficulty=difficulty,
                  triggers=[dict(flags=[1,0,1,1],events=[[51,7]],actions=[[28,7]])],
                  operations=[['shared'],['action',53,0],['action',22,0]]) for difficulty in (-1,0,1,2,3)]
    rows += [dict(name=f'completion_mode_{mode}',repeat=mode,
                  triggers=[dict(events=[[2,0],[47,10]],actions=[[28,7]])],
                  operations=[['shared'],['dispatch',0,2],['frame',150],['dispatch',0,13],
                              ['dispatch',0,13]]) for mode in (0,2)]
    rows += [dict(name=f'timer_boundary_{value}',frame=0x7FFFFFF0,
                  triggers=[dict(events=[[13,value]],actions=[[28,7]])],
                  operations=[['shared'],['dispatch',0,13],['frame',0x80000010],['dispatch',0,13]])
             for value in (-1,0,2,143165577,2147483647)]
    rows += [dict(name='shared_timer_last_event',
                  triggers=[dict(events=[[51,7],[13,4],[51,3]],actions=[[28,7]])]),
             dict(name='force_predicate_resets_timer',triggers=[dict(events=[[51,7]],actions=[[28,7]])],
                  operations=[['shared'],['dispatch',0,13,[0,0],1],['action',22,0]])]
    return rows


def generate():
    rows=[execute(case) for case in inputs()]
    named={r['input']['name']:r for r in rows}
    assert [h['returned'] for h in named['sharing']['history']]==[0,0,1,0,2]
    assert named['reverse_chain']['history'][1]['actions']==[[28,8],[28,7]]
    assert named['mode_1_refs_2']['history'][-2]['globals']==[]
    assert named['mode_1_refs_2']['history'][-1]['globals']==[7]
    assert named['mode_0_refs_2']['history'][-1]['tags'][0]['refs']==0
    assert named['completion_mode_2']['history'][1]['tags'][0]['triggers'][0]['completed']==1
    assert named['completion_mode_2']['history'][-1]['globals']==[7]
    assert named['completion_mode_0']['history'][-1]['globals']==[]
    for row in rows:
        # All retained instances must have been initialized by the real ctor.
        for state in row['history']:
            for tag in state['tags']:
                assert tag['pending'] in (0,1)
                assert all(t['pending'] in (0,1) and t['enabled'] in (0,1) for t in tag['triggers'])
    return rows


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        entry_points={'find_or_create':0x6E52A0,'tag_constructor':0x6E4DE0,
                      'process_event':0x6E53A0,'trigger_predicate':0x7264C0,'spring':0x7265C0,
                      'reset':0x726400,'execute_action':0x6DD8B0},
        assumptions=['Resolved definitions, one registered House and initialized Scenario RNG.',
                     'Registries have spare capacity; map cells exist; no physical Objects or Teams are registered.'],
        substitutions=['Operator-new7C8E17 supplies bounded poisoned arena storage. Fresh Tag storage is supplied directly to its original constructor. No gameplay calls or instructions are replaced.'],
        scope='Original Tag find/create, linked Trigger construction and timer/RNG, repeated dispatch, global/local reset fanout, enable/disable/force, cell attachment and detach/enqueue. Excludes map definition loading, production scenario dispatch, Team/Object lifecycle and deferred destruction.',
    ))
