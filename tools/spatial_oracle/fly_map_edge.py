"""Original Fly candidate admission, correction and Team waypoint predicate.

Runs 4CDB4C up to the SetCoords call boundary or its skip branch. All map,
Aircraft, Mission, Team, Script and Scenario RNG callees execute original code.
Retained object/Team states are explicit inputs, not proofs of their producers.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, run_checked, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import Fixture, OWNER, TYPE, TARGET, MAP, SCENARIO, cell
from tools.spatial_oracle.map_queries import dwords, packed

LOCO, TEAM, SCRIPT, SCRIPT_TYPE = [SCRATCH+n for n in (0x30000,0x40000,0x41000,0x42000)]


def execute(case):
    f = Fixture(dict(seed=case.get('seed',31)))
    u, sp = f.u, f.sp
    width,height = case.get('size',[64,64])
    u.mem_write(MAP+0xF4,dwords(width,height))
    u.mem_write(MAP+0xFC,dwords(*case.get('local',[2,2,60,56])))
    # Raw +12C is independently supplied, so no inferred width convention is
    # smuggled into the oracle. Resize566332 is the normal producer.
    u.mem_write(MAP+0x12C,dwords(case.get('span_12c',127)))
    f.call(0x4CC9A0,LOCO,[])
    u.mem_write(LOCO+0xC,dwords(OWNER))
    u.mem_write(OWNER+0x2B4,dwords(TARGET if case.get('target',False) else 0))
    u.mem_write(OWNER+0xAC,dwords(case.get('mission',5)))
    u.mem_write(OWNER+0xB4,dwords(case.get('queued_mission',-1)))
    u.mem_write(OWNER+0x3D4,bytes([case.get('mission_only',True),case.get('in_playfield',True)]))
    u.mem_write(TYPE+0xE0B,bytes([case.get('fly_by',False)]))
    u.mem_write(TYPE+0xD54,bytes([case.get('spawned',False)]))
    team=case.get('team')
    if team is not None:
        u.mem_write(OWNER+0x5D4,dwords(TEAM))
        u.mem_write(TEAM+0x7F,bytes([team.get('active',True)]))
        u.mem_write(TEAM+0x28,dwords(SCRIPT))
        u.mem_write(SCRIPT+0x24,dwords(SCRIPT_TYPE))
        cursor=team.get('cursor',0)
        u.mem_write(SCRIPT+0x2C,dwords(cursor))
        u.mem_write(SCRIPT_TYPE+0xA0,dwords(team.get('count',1)))
        if 0 <= cursor < 20:
            u.mem_write(SCRIPT_TYPE+0xA4+cursor*8,dwords(team.get('action',3),4))
        waypoint=team.get('waypoint',[0,0])
        u.mem_write(SCENARIO+0x632+4*4,packed(*waypoint))
        if all(0 <= n < 128 for n in waypoint):
            u.mem_write(cell(*waypoint)+0x11B,bytes([team.get('level',0)&255,team.get('slope',0)]))
    proposed=case.get('proposed',[128*256,64*256,1500])
    u.mem_write(sp+0x50,dwords(*proposed))
    u.reg_write(UC_X86_REG_ESI,LOCO)
    u.reg_write(UC_X86_REG_ESP,sp)
    calls,draws,shapes,local_coords=[],[],[],[]
    observe_calls={0x41B890:'aircraft_gate',0x6EC300:'team_waypoint_gate',
                   0x6915D0:'script_has_action',0x691500:'script_action',
                   0x68BCC0:'waypoint',0x578460:'playfield_mode_one',
                   0x565660:'to_local',0x49F420:'scatter'}
    def observe(_u,pc,_size,_data):
        if pc in observe_calls: calls.append(observe_calls[pc])
        if pc==0x568300:
            ptr=struct.unpack('<I',u.mem_read(u.reg_read(UC_X86_REG_ESP)+4,4))[0]
            shapes.append(list(struct.unpack('<hh',u.mem_read(ptr,4))))
        if pc==0x65C780: draws.append(1)
        if pc==0x4CDC35:
            local_coords.append(list(struct.unpack('<hh',u.mem_read(u.reg_read(UC_X86_REG_EAX),4))))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    original=bytes(u.mem_read(0x4CDB4C,0x1C1))
    before_rng=bytes(u.mem_read(SCENARIO+0x218,0x3F4))
    end=run_checked(u,0x4CDB4C,(0x4CDCFD,0x4CDD0D),count=10000)
    u.hook_del(hook)
    assert u.reg_read(UC_X86_REG_ESP)==sp
    assert original==bytes(u.mem_read(0x4CDB4C,0x1C1))
    result=list(struct.unpack('<iii',u.mem_read(sp+0x50,12)))
    rng_changed=before_rng!=bytes(u.mem_read(SCENARIO+0x218,0x3F4))
    return dict(input=case,commit=end==0x4CDCFD,candidate=result,calls=calls,
                shape_queries=shapes,local_coords=local_coords,rng_draws=len(draws),rng_changed=rng_changed,
                next_random=f.call(0x65C780,SCENARIO+0x218,[]))


def inputs():
    rows=[dict(name='inside',proposed=[64*256,64*256,1500]),dict(name='right_edge')]
    rows += [dict(name=f'right_sub_{sub}_seed_{seed}',seed=seed,proposed=[128*256+sub,64*256,1500])
             for sub in (-129,-128,-1,0,1,127,128,255,256,511) for seed in (0,1,31)]
    rows += [dict(name=f'edge_{x}_{y}',proposed=[x,y,317]) for x,y in
             ((32*256,32*256),(64*256,127*256),(96*256,96*256),
              (32*256+127,32*256+127),(96*256+127,96*256+127),
              (0,0),(-257,-1),(131071,131071))]
    rows += [dict(name=f'gate_{key}_{value}',**{key:value}) for key,value in
             (('mission_only',False),('in_playfield',False),('target',True),
              ('fly_by',True),('spawned',True),('mission',26),('mission',27),('mission',31))]
    rows += [dict(name=f'queued_{mission}',mission=-1,queued_mission=mission) for mission in (5,26,27,31)]
    rows += [dict(name='target_mission31',target=True,mission=31),
             dict(name='target_queued31',target=True,mission=-1,queued_mission=31)]
    rows += [dict(name=f'team_{index}',team=team) for index,team in enumerate([
        {},dict(active=False),dict(cursor=-1),dict(cursor=1),dict(action=2),
        dict(waypoint=[64,64]),dict(waypoint=[32,36]),
        dict(waypoint=[32,36],level=5),dict(waypoint=[32,36],level=-1,slope=1),
        dict(cursor=2,count=3),dict(cursor=-2147483648),dict(cursor=0,count=0),
    ])]
    rows += [dict(name=f'team_height_{level}_slope_{slope}',
                  team=dict(waypoint=[33,36],level=level,slope=slope))
             for level,slope in ((0,0),(1,0),(0,1),(-1,1))]
    rows += [dict(name='team_without_mission_only',mission_only=False,team={}),
             dict(name='team_fly_by',fly_by=True,team={})]
    rows += [dict(name=f'local_{index}',local=local,span_12c=span,proposed=[32*256,32*256,0])
             for index,(local,span) in enumerate([([0,0,64,64],127),([30,0,20,64],127),
                                                ([2,2,60,56],-7),([2,2,60,56],0)])]
    rows += [dict(name='odd_width',size=[65,63],span_12c=127),
             dict(name='nonsquare',size=[80,24],span_12c=103,proposed=[81*256,1*256,0])]
    return rows


def generate():
    rows=[execute(case) for case in inputs()]
    # Guard fixture coverage as well as pinning the native outputs. In-shape
    # coordinates would silently prevent the retained-state cases reaching
    # the Aircraft predicate that they are intended to compare.
    named={row['input']['name']:row for row in rows}
    assert not named['inside']['calls']
    for row in rows:
        name=row['input']['name']
        if name.startswith(('gate_','queued_','target_','team_')):
            assert 'aircraft_gate' in row['calls'], name
        assert row['rng_draws'] in (0,1), name
        assert row['rng_changed']==bool(row['rng_draws']), name
    assert named['gate_fly_by_True']['rng_draws']==0
    assert named['gate_spawned_True']['rng_draws']==1
    assert named['team_height_0_slope_0']['commit']
    assert not named['team_height_1_slope_0']['commit']
    assert not named['team_height_0_slope_1']['commit']
    assert named['team_height_-1_slope_1']['commit']
    assert any(row['commit'] and row['rng_draws'] for row in rows)
    assert any(not row['commit'] and row['rng_draws'] for row in rows)
    assert any(row['calls']==['aircraft_gate','to_local'] for row in rows)
    return rows


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        entry_points={'candidate_admission':0x4CDB4C,'set_coords_boundary':0x4CDCFD,
                      'skip_set_coords':0x4CDD0D,'aircraft_gate':0x41B890,'team_gate':0x6EC300,
                      'shape':0x568300,'to_local':0x565660,'scatter':0x49F420},
        assumptions=['Real Aircraft/Fly vtables and retained target,mission,+3D4,+3D5,FlyBy inputs.',
                     'Team,Script,cursor,waypoint and cell height are supplied state; lifecycle producers are excluded.',
                     'Native Map Size,normalized LocalSize and raw +12C supplied independently.',
                     'Entry is after paid coordinate math and MarkREMOVE; stops before SetCoords or at its skip target.'],
        substitutions=[],scope='Original Fly candidate validation including original Aircraft/Team/Script/map/RNG callees. Records both coordinate commit/refusal branches and exact draw counts. Does not prove Team creation/activation, prior Process admission or downstream SetCoords/Mark/height/mission effects.',
    ))
