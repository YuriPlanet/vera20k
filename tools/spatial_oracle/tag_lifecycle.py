"""Original live Tag/Trigger construction, dispatch, timers and attachments.

Only allocation/free storage boundaries are supplied. Original predicates, actions, RNG,
reference updates and detach/deferred-enqueue instructions execute unchanged.
Resolved definitions and spare registry capacity are fixture inputs. Polling
cases execute the original Logic tag prefix with a supplied registration list;
this does not certify scenario loading, registration classification or Team AI.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.aircraft_fire_location import Fixture, SCENARIO, cell
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.team_creation import u32, i32

HOUSE, HOUSE_TYPE, TAG_TYPE = [SCRATCH+n for n in (0x20000, 0x30000, 0x40000)]
TAG_TYPES = [TAG_TYPE+n*0x100 for n in range(8)]
TRIGGER_TYPES = [SCRATCH+0x41000+n*0x100 for n in range(8)]
EVENTS, ACTIONS, CONTROL = [SCRATCH+n for n in (0x43000, 0x46000, 0x49000)]
HEAP, HEAP_END = SCRATCH+0x60000, SCRATCH+0x80000
REGISTRIES = {
    'abstract':0xB0F670, 'object_base':0xB0F618, 'tag':0xB0E720,
    'tag_trigger_base':0xB0F708, 'trigger':0xA8EAE8, 'pending':0xB0F698,
    'logic_tags':0x8B40C8, 'map_tags':0x8B41A8, 'cell_tags':0x880944,
    'object':0xA8E360, 'object_base_b':0xB0F720,
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
    u.mem_write(SCENARIO+0x11E8, dwords(-1))
    for index, base in enumerate(REGISTRIES.values()):
        u.mem_write(base+4, dwords(SCRATCH+0x50000+index*0x400,128,0,0,128))
    # Original constructor/static-initializer vtables:565157,4E7F4D,4E7FCD.
    # Detach invokes their real Find bodies even when the vectors are empty.
    u.mem_write(REGISTRIES['cell_tags'],dwords(0x7E3890))
    u.mem_write(REGISTRIES['map_tags'],dwords(0x7EA5A4))
    u.mem_write(REGISTRIES['logic_tags'],dwords(0x7EA5A4))
    for base, writer in ((0xB0F670,0x72536D),(0xB0F708,0x72546D),
                         (0xB0E720,0x6E4D7D),(0xA8EAE8,0x4E65FD),
                         (0xB0F618,0x7253ED),(0xB0F698,0x72586D)):
        instruction=bytes(u.mem_read(writer,10))
        assert instruction[:6]==b'\xC7\x05'+dwords(base)
        u.mem_write(base,instruction[6:])
    u.mem_write(0xA8022C, dwords(SCRATCH+0x4F000))
    u.mem_write(0xA80238, dwords(1))
    u.mem_write(SCRATCH+0x4F000, dwords(HOUSE))
    u.mem_write(HOUSE+0x34, dwords(HOUSE_TYPE))
    u.mem_write(HOUSE_TYPE+0xB4, dwords(0,0))
    definitions = case.get('triggers',[dict(events=[[47,0]], actions=[[28,7]])])
    for typ in TAG_TYPES:
        u.mem_write(typ+0x9C, dwords(case.get('repeat',2), TRIGGER_TYPES[0] if definitions else 0))
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
    allocations, actions_run, draws, calls, tags, objects = [], [], [], [], [], []
    dispatches = []
    allocated, freed, allocation_types, frees = set(), set(), {}, []
    pointer_probe=RET_MAGIC+0x100
    u.mem_write(0x7E115C,dwords(pointer_probe))
    def observe(_u, pc, _size, _data):
        nonlocal cursor
        if pc==pointer_probe:
            # Original CRT typeid probes its real TypeDescriptor through this
            # Windows import. Validate memory before supplying OS success.
            sp=u.reg_read(UC_X86_REG_ESP)
            ret,pointer,length=struct.unpack('<3I',u.mem_read(sp,12))
            u.mem_read(pointer,length)
            u.reg_write(UC_X86_REG_EAX,0)
            u.reg_write(UC_X86_REG_ESP,sp+12)
            u.reg_write(UC_X86_REG_EIP,ret)
            return
        if pc == 0x65C837:
            draws.append(1)
        if pc == 0x6E53A0:
            dispatches.append([tags.index(u.reg_read(UC_X86_REG_ECX)),
                               i32(u,u.reg_read(UC_X86_REG_ESP)+4)])
        if pc in (0x6E4DE0,0x725FA0,0x726400,0x726720,0x7258D0,0x6E4F60,0x726950):
            calls.append(f'{pc:08X}')
        if pc in (0x6E4DE0,0x725FA0):
            allocation_types[u.reg_read(UC_X86_REG_ECX)]='tag' if pc==0x6E4DE0 else 'trigger'
        if pc == 0x6DD8B0:
            action = u.reg_read(UC_X86_REG_ECX)
            actions_run.append([i32(u,action+0x2C),i32(u,action+0x90)])
        if pc == 0x7C8B3D:
            sp=u.reg_read(UC_X86_REG_ESP)
            pointer=u32(u,sp+4)
            assert pointer in allocated and pointer not in freed, (hex(pointer),hex(u32(u,sp)),allocated,freed)
            freed.add(pointer)
            frees.append(allocation_types[pointer])
            u.reg_write(UC_X86_REG_ESP,sp+4)
            u.reg_write(UC_X86_REG_EIP,u32(u,sp))
            return
        if pc != 0x7C8E17:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        size = u32(u,sp+4)
        pointer = cursor
        cursor += (max(size,1)+15)&~15
        assert cursor < HEAP_END
        u.mem_write(pointer,b'\xA5'*max(size,1))
        allocations.append(size)
        allocated.add(pointer)
        u.reg_write(UC_X86_REG_EAX,pointer)
        u.reg_write(UC_X86_REG_ESP,sp+4)
        u.reg_write(UC_X86_REG_EIP,u32(u,sp))
    hook = u.hook_add(UC_HOOK_CODE,observe)
    ranges = [(0x6E4DE0,0x6E5AA0),(0x725FA0,0x726B00),(0x71E940,0x71FA40),
              (0x7258D0,0x725D90),(0x5F3900,0x5F3B40),(0x55AFB0,0x55B205),
              (0x6DD8B0,0x6E45D0),(0x689670,0x689A90)]
    code = [bytes(u.mem_read(a,b-a)) for a,b in ranges]
    history = []
    def snapshot(returned):
        tag_states = []
        for tag in tags:
            if tag in freed:
                tag_states.append(None)
                continue
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
        state=dict(returned=returned,tags=tag_states,actions=list(actions_run),rng_draws=len(draws),
            pending_count=len(pending),pending_tags=[tags.index(p) for p in pending if p in tags],
            globals=[n for n in range(50) if u.mem_read(SCENARIO+0x1CB0+n*41,1)[0]],
            locals=[n for n in range(100) if u.mem_read(SCENARIO+0x24B2+n*41,1)[0]],
            changed=u.mem_read(SCENARIO+0x34AA,1)[0])
        if case.get('lifecycle',False):
            state.update(frees=list(frees),registry_counts={name:i32(u,base+16) for name,base in REGISTRIES.items()},
                         object_tags=[tags.index(u32(u,obj+0x34)) if u32(u,obj+0x34) else None for obj in objects])
        if case.get('polling',False):
            base=REGISTRIES['logic_tags']
            state.update(dispatches=list(dispatches),
                         logic_tags=[tags.index(u32(u,u32(u,base+4)+n*4)) for n in range(i32(u,base+16))],
                         scenario_flags=[u.mem_read(SCENARIO+n,1)[0] for n in (0x34AA,0x34A9,0x34AB,0x34BE)])
        return state
    for op in case.get('operations',[['shared'],['dispatch',0,13],['dispatch',0,13]]):
        kind, *args = op
        returned = None
        if kind in ('shared','fresh'):
            tag_type=TAG_TYPES[args[0] if args else 0]
            if kind=='shared':
                tag = f.call(0x6E52A0,tag_type,[])
            else:
                tag = cursor
                cursor += 0x40
                allocated.add(tag)
                u.mem_write(tag,b'\xA5'*0x38)
                f.call(0x6E4DE0,tag,[tag_type])
            if tag not in tags:
                tags.append(tag)
            returned = tags.index(tag)
        elif kind=='frame':
            u.mem_write(0xA8ED84,dwords(args[0]))
        elif kind=='attach_cell':
            f.call(0x485250,cell(args[1],args[2]),[tags[args[0]]])
        elif kind=='object':
            obj=SCRATCH+0x90000+len(objects)*0x1000
            u.mem_write(obj,b'\xA5'*0x300)
            f.call(0x5F3900,obj,[])
            objects.append(obj)
        elif kind=='attach_object':
            returned=f.call(0x5F5B50,objects[args[0]],[tags[args[1]] if args[1]>=0 else 0])&255
        elif kind=='dispatch':
            tag_index,event = args[:2]
            xy = args[2] if len(args)>2 else [0,0]
            force = args[3] if len(args)>3 else 0
            obj = objects[args[4]] if len(args)>4 else 0
            returned = f.call(0x6E53A0,tags[tag_index],[event,obj,int.from_bytes(packed(*xy),'little'),force,0])&255
        elif kind=='global' or kind=='local':
            returned = f.call(0x689670 if kind=='global' else 0x689910,SCENARIO,args)&255
        elif kind=='action':
            action_kind,typ = args
            u.mem_write(CONTROL+0x2C,dwords(action_kind))
            u.mem_write(CONTROL+0x50,dwords(TRIGGER_TYPES[typ]))
            returned = f.call(0x6DD8B0,CONTROL,[HOUSE,0,0,0])&255
        elif kind=='mark_tag':
            f.call(0x6E5230,tags[args[0]],[])
        elif kind=='mark_trigger':
            trigger=u32(u,tags[args[0]]+0x28)
            for _ in range(args[1]): trigger=u32(u,trigger+0x28)
            f.call(0x726720,trigger,[])
        elif kind=='drain':
            f.call(0x725C70,0,[])
        elif kind=='register_logic':
            base=REGISTRIES['logic_tags']
            assert len(args)<=128
            u.mem_write(u32(u,base+4),dwords(*(tags[n] for n in args)))
            u.mem_write(base+16,dwords(len(args)))
        elif kind=='scenario_flags':
            assert len(args)==4
            for offset,value in zip((0x34AA,0x34A9,0x34AB,0x34BE),args):
                u.mem_write(SCENARIO+offset,bytes([value]))
        elif kind=='poll':
            # Run the complete original tag-poll prefix, including live vector
            # mutation and dirty-flag clearing. Stop before unrelated Logic AI.
            u.reg_write(UC_X86_REG_ESP,f.sp)
            u.reg_write(UC_X86_REG_ECX,0x87F778)
            run_checked(u,0x55AFB0,0x55B205)
            assert u.reg_read(UC_X86_REG_ESP)==f.sp-0x38
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
    rows += [dict(name=f'drain_mode_{mode}',repeat=mode,lifecycle=True,
                  operations=[['shared'],['object'],['attach_object',0,0],['attach_cell',0,50,60],
                              ['dispatch',0,13,[0,0],0,0],['dispatch',0,13,[50,60]],['drain'],['shared']])
             for mode in (0,1,2)]
    rows += [dict(name='explicit_tag_removal',lifecycle=True,
                  triggers=[dict(events=[[51,7]]),dict(events=[[51,3]])],
                  operations=[['shared'],['object'],['attach_object',0,0],['mark_tag',0],['mark_tag',0],
                              ['drain'],['shared']]),
             dict(name='trigger_splice',lifecycle=True,triggers=[{},{}],
                  operations=[['shared'],['mark_trigger',0,1],['mark_trigger',0,1],['drain'],
                              ['mark_trigger',0,0],['drain'],['mark_tag',0],['drain']]),
             dict(name='object_reattachment',lifecycle=True,
                  operations=[['shared'],['fresh'],['object'],['attach_object',0,0],
                              ['attach_object',0,0],['attach_object',0,1],['attach_object',0,-1]])]
    rows += [dict(name=f'logic_poll_repeat_{mode}',repeat=mode,polling=True,lifecycle=True,
                  operations=[['shared',0],['shared',1],['shared',2],['register_logic',0,1,2],
                              ['poll'],['poll'],['drain']]) for mode in (0,2)]
    rows += [dict(name='logic_poll_all_flags',polling=True,triggers=[dict(events=[[47,999]])],
                  operations=[['shared'],['register_logic',0],['scenario_flags',1,1,1,1],['poll'],['poll']]),
             dict(name='logic_poll_random_timer',polling=True,triggers=[dict(events=[[51,7]],actions=[[28,7]])],
                  operations=[['shared'],['register_logic',0],['poll'],['frame',400],['poll'],['poll']])]
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
    for mode in (0,1):
        state=named[f'drain_mode_{mode}']['history'][-2]
        assert state['frees']==['trigger','tag'] and state['pending_count']==0
        assert state['object_tags']==[None] and state['registry_counts']['cell_tags']==0
    state=named['explicit_tag_removal']['history'][-2]
    assert state['frees']==['tag','trigger','trigger'] and state['pending_count']==0
    assert named['trigger_splice']['history'][-1]['registry_counts']['abstract']==0
    assert named['logic_poll_repeat_0']['history'][4]['logic_tags']==[1]
    assert named['logic_poll_repeat_0']['history'][4]['dispatches']==[[0,13],[2,27]]
    assert named['logic_poll_repeat_0']['history'][5]['dispatches']==[[0,13],[2,27],[1,13]]
    assert named['logic_poll_repeat_2']['history'][4]['dispatches']==[[0,13],[1,27],[2,27]]
    assert named['logic_poll_all_flags']['history'][3]['dispatches']==[[0,n] for n in (50,27,28,36,37,45,46,13,51)]
    assert named['logic_poll_all_flags']['history'][3]['scenario_flags']==[0,0,0,0]
    for row in rows:
        # All retained instances must have been initialized by the real ctor.
        for state in row['history']:
            for tag in state['tags']:
                if tag is None: continue
                assert tag['pending'] in (0,1)
                assert all(t['pending'] in (0,1) and t['enabled'] in (0,1) for t in tag['triggers'])
    return rows


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        entry_points={'find_or_create':0x6E52A0,'tag_constructor':0x6E4DE0,
                      'process_event':0x6E53A0,'trigger_predicate':0x7264C0,'spring':0x7265C0,
                      'reset':0x726400,'execute_action':0x6DD8B0,'object_constructor':0x5F3900,
                      'object_attach':0x5F5B50,'deferred_drain':0x725C70,
                      'logic_tag_poll_prefix':0x55AFB0,
                      'tag_destructor':0x6E4F60,'trigger_deleting_destructor':0x726950},
        assumptions=['Resolved definitions, one registered House and initialized Scenario RNG.',
                     'Registries have spare capacity and native initialized vtables; map cells exist. Lifecycle cases construct real base Objects; no Teams are registered.',
                     'Polling cases supply the Logic registration list and run55AFB0..55B205; the scenario countdown is stopped, excluding its expiry/UI callback. Distinct TagTypes may share TriggerTypes.',
                     'Thread has an empty SEH chain. Gameplay/shutdown flagsA8E9A0/A8ED5C remain false, excluding destructor House-win/cursor updates.'],
        substitutions=['Operator-new7C8E17 supplies bounded poisoned arena storage;7C8B3D records freeing it. Fresh Tag and Object storage is supplied directly to original constructors.',
                       'Windows IsBadReadPtr succeeds only after Unicorn validates the probed memory. Original CRT typeid and gameplay callees/instructions are unchanged.'],
        scope='Original Tag find/create, linked Trigger construction and timer/RNG, dispatch, reset fanout, force/enable/disable, Object/cell attachment, detach, duplicate pending removal, linked Trigger splicing, deferred destructors and Logic tag polling with live list mutation/dirty flags. Excludes map loading/registration classification, scenario countdown expiry, later Logic AI, Team lifecycle, physical Object destruction and gated House-win/cursor destructor updates.',
    ))
