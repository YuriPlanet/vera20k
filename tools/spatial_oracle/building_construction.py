"""Original building construction timing: the Buildup rate, the stage stepping
and the Construction mission.

- `rate` rows: the BuildingType load slice 0x45F2AA..0x45F310 (count =
  Buildup SHP frames / 2, or GateStages + 1 for a gate; rate =
  ftol(BuildupTime * 900 / count), 1 when count <= 0) through
  building_body_rules' fixture, with retail BuildupTime (.06, read as a float)
  and the RulesClass constructor's .05 over the retail Buildup frame counts.
- `stepping` rows: BuildingClass::Begin_Mode(0) (0x447780) then
  BuildingClass::UpdateAnimation (0x4509D0) once per frame with the frame
  counter advanced, on the slave_manager fixture's refinery (a 2x2 building on
  the harvest_field map) with a supplied construction control at Type+0xF04.
  Per frame: the stage (+0xF8) and the animation-complete byte (+0x6DD). The
  presentation callees BuildingClass::UpdateAnimFacingAndDirection 0x451F60,
  SetAnimRemap 0x452170, 0x456FB0 and TechnoClass 0x705D70 (whose result only
  feeds the first) are answered.
- `mission` rows: BuildingClass::Mission_Construction (0x449A50) visit by visit
  over the same stepping. Its sound calls (VocClass::PlayAt 0x7509E0, the loop
  update 0x750D40, SoundEvent::Release 0x406060) and Grand_Opening (vt+0x4DC =
  0x445F80) and the radio broadcasts (vt+0x274 = 0x65ACB0) are observed and
  answered; Begin_Mode and Queue_Mission run natively.
- `route` rows: a building's frames from its creation, through
  BuildingClass::Update's construction pieces in its order, each native:
  UpdateAnimation (0x43FE22), the ready check that commences a queued mission
  unless BState is 0 (0x43FE27..0x43FE54), TechnoClass::AI's mission dispatch
  MissionClass::AI (0x6FA655 -> 0x5B3060, which runs Mission_Construction or
  BuildingClass::Sell 0x449C30), the ready check that commences (0x43FF91..
  0x43FFB4) and the queued-BState block (0x43FFB4..0x440042). The rest of
  TechnoClass::AI is not run. The creation at frame 0: a human player's
  placement (HouseClass::Place_Production: Unlimbo's vt+0x484(1, 1) =
  0x44D6A0, then the factory's OVER_OUT, BuildingClass::Receive_Radio(3)
  0x43C2D0), a computer house's (ExitObject: vt+0x484(1, 1) then Commence
  0x5B3570, 0x445329..0x44533F), a deploy (vt+0x484(1, 1), UnitClass::Deploy's
  Queue_Mission(Construction) 0x7396D5 and +0x6DD 0x73984E; its first Update
  in the same frame), or an UndeploysInto sale of an idle building
  (Sell_Back(-1) 0x447110) with or without an ArchiveTarget, stopped at the
  stage-2 visit that finds +0x6DD (0x449CA7, before the unit is built). The
  factory's other radio traffic (TechnoClass::Receive_Radio 0x6F4AB0), the
  undeploy voice (0x459C20), Sell's broadcasts (0x65ACE0), IsHumanPlayer
  (0x50B6F0, answered no), the sale's survivor count (0x451330, answered 0:
  no crew) and its occupy list (0x5F5B90, answered empty) are answered.

Usage: python -m tools.spatial_oracle.building_construction [--check|--write]
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle import building_body_rules as body_rules
from tools.spatial_oracle import slave_manager as sm
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.unit_scatter_state import SP

BEGIN_MODE, UPDATE_ANIMATION, MISSION_CONSTRUCTION = 0x447780, 0x4509D0, 0x449A50
GRAND_OPENING, PLAY_AT, LOOP_UPDATE, SOUND_RELEASE = 0x445F80, 0x7509E0, 0x750D40, 0x406060
# RadioClass's broadcast to every contact (Building vt+0x274).
RADIO_BROADCAST = 0x65ACB0
# Presentation callees of UpdateAnimation (argument bytes each pops).
PRESENTATION = {0x451F60: 8, 0x452170: 4, 0x456FB0: 4, 0x705D70: 0}
FRAME = 0xA8ED84
MISSION = {'construction': 0x12, 'selling': 0x13, 'guard': 5, 'none': -1}
# BuildingClass::Update's construction pieces (see the `route` rows).
READY_COMMENCE_UNLESS_BUILDING = (0x43FE27, 0x43FE54)
MISSION_AI = 0x5B3060
READY_COMMENCE = (0x43FF91, 0x43FFB4)
QUEUED_BSTATE = (0x43FFB4, 0x440042)
ENTER_CONSTRUCTION, RECEIVE_RADIO, TECHNO_RECEIVE_RADIO = 0x44D6A0, 0x43C2D0, 0x6F4AB0
QUEUE_MISSION, COMMENCE, SELL_BACK = 0x5B35E0, 0x5B3570, 0x447110
SELL_CONVERTS = 0x449CA7
UNDEPLOY_VOICE, RADIO_BROADCAST_ALL, IS_HUMAN_PLAYER = 0x459C20, 0x65ACE0, 0x50B6F0
# BuildingClass survivor count (vt+0x2D0), which a sale's stage 1 spends on crew,
# and the occupy list (vt+0x108 = ObjectClass 0x5F5B90) it places them on.
SURVIVOR_COUNT, OCCUPY_LIST = 0x451330, 0x5F5B90
SCENARIO_INIT, SCENARIO_FLAG_ED6B = 0xA8E7AC, 0xA8ED6B
# Retail `BuildupTime=.06` as CCINIClass::ReadDouble (0x5283D0) stores it (`%f`
# into a float, widened), and the RulesClass constructor's .05.
BUILDUP_TIME = {'retail': '3faeb851e0000000', 'default': '3fa999999999999a'}


def rate_rows():
    fixture = body_rules.Fixture()
    rows = []
    for time_name, bits in BUILDUP_TIME.items():
        for frames in (None, 0, 2, 4, 34, 36, 50, 52, 54, 58, 108):
            rows.append(dict(buildup_time=time_name, frames=frames,
                             control=fixture.buildup(frames, False, 9, bits)))
    for gate, stages in ((True, 9), (True, 0)):
        rows.append(dict(buildup_time='retail', frames=50, gate=gate, stages=stages,
                         control=fixture.buildup(50, gate, stages, BUILDUP_TIME['retail'])))
    return rows


def building_fixture(case):
    """The slave_manager fixture's refinery with the row's construction control,
    mission, ArchiveTarget and UndeploysInto, its stage at the TechnoClass
    constructor's (stage 0, step 1, rate 0) and BState -1."""
    u, call, read32, events = sm.make_fixture(dict(name=case['name'], manager_state=0, nodes=[], ore=[]))
    building, kind = sm.YAREFN, sm.YTYPE
    u.mem_write(kind + 0xF04, dwords(*case['control']))
    u.mem_write(kind + 0x408, dwords(sm.YTYPE if case.get('undeploys') else 0))
    u.mem_write(building + 0xAC, dwords(MISSION[case.get('mission', 'construction')]))
    u.mem_write(building + 0xB4, dwords(-1))
    u.mem_write(building + 0xBC, dwords(0))
    u.mem_write(building + 0x218, dwords(sm.cell(12, 12) if case.get('archive') else 0))
    u.mem_write(building + 0x534, dwords(-1))
    u.mem_write(building + 0x6DD, bytes([0]))
    frame = read32(FRAME)
    u.mem_write(building + 0xF8, dwords(0))
    u.mem_write(building + 0x100, dwords(frame, 0, 0, 0, 1))
    calls = []

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def hook(_u, address, _size, _data):
        if address in PRESENTATION:
            ret(PRESENTATION[address])
        elif address == RADIO_BROADCAST:
            calls.append(['radio', read32(u.reg_read(UC_X86_REG_ESP) + 4)])
            ret(4)
        elif address == GRAND_OPENING:
            calls.append(['grand_opening', read32(u.reg_read(UC_X86_REG_ESP) + 4)])
            ret(4)
        elif address == PLAY_AT:
            calls.append(['play_sound', struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_ECX)))[0]])
            ret(4)
        elif address == LOOP_UPDATE:
            ret(0)
        elif address == SOUND_RELEASE:
            calls.append(['sound_release'])
            ret(0)

    u.hook_add(UC_HOOK_CODE, hook)
    return u, read32, building, frame, calls


def invoke(u, entry, this, *args):
    u.mem_write(SP, dwords(RET_MAGIC, *args))
    u.reg_write(UC_X86_REG_ECX, this)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, entry, RET_MAGIC, count=2_000_000)
    return u.reg_read(UC_X86_REG_EAX)


def snapshot(u, building):
    signed = lambda address: struct.unpack('<i', u.mem_read(address, 4))[0]
    return dict(stage=signed(building + 0xF8), done=u.mem_read(building + 0x6DD, 1)[0],
                bstate=signed(building + 0x534), mission=signed(building + 0xAC),
                queued=signed(building + 0xB4), status=signed(building + 0xBC),
                timer=[signed(building + 0x100), signed(building + 0x108)], rate=signed(building + 0x10C))


def stepping(case):
    """Begin_Mode(0) at frame 0, then UpdateAnimation at frames 1.. until the
    row's frame count."""
    u, read32, building, frame, _calls = building_fixture(case)
    # GameOptionsClass's stored speed (0xA8EB60), which SpeedNormalize
    # (0x5FB2E0) reads when a wrap re-derives the rate.
    game_speed = read32(0xA8EB60)
    invoke(u, BEGIN_MODE, building, 0)
    frames = [dict(frame=0, **snapshot(u, building))]
    for k in range(1, case['frames'] + 1):
        u.mem_write(FRAME, dwords(frame + k))
        invoke(u, UPDATE_ANIMATION, building)
        frames.append(dict(frame=k, **snapshot(u, building)))
    return dict(input=case, game_speed=game_speed, frames=frames)


def mission(case):
    """The placement's Begin_Mode(0) at frame 0 (vt+0x484), then per frame
    UpdateAnimation followed by a Mission_Construction visit, as
    BuildingClass::Update orders them (0x43FE22, then TechnoClass::AI's
    dispatch); the visit's delay is recorded."""
    u, read32, building, frame, calls = building_fixture(case)
    invoke(u, BEGIN_MODE, building, 0)
    frames = []
    for k in range(1, case['frames'] + 1):
        u.mem_write(FRAME, dwords(frame + k))
        invoke(u, UPDATE_ANIMATION, building)
        before = len(calls)
        delay = invoke(u, MISSION_CONSTRUCTION, building)
        frames.append(dict(frame=k, delay=delay, calls=calls[before:], **snapshot(u, building)))
        if any(call[0] == 'grand_opening' for call in calls[before:]):
            break
    return dict(input=case, frames=frames)


def stepping_cases():
    return [
        dict(name='s_3x2', control=[0, 3, 2], frames=8),
        dict(name='s_4x1', control=[0, 4, 1], frames=6),
        dict(name='s_2x3', control=[0, 2, 3], frames=8),
        dict(name='s_17x3', control=[0, 17, 3], frames=52),
        dict(name='s_25x2', control=[0, 25, 2], frames=52),
        dict(name='s_29x1', control=[0, 29, 1], frames=32),
        # No Buildup SHP: the constructor's {0, 1, 0}.
        dict(name='s_no_buildup', control=[0, 1, 0], frames=3),
        # One frame, rate 53: the stage never rests on count - 1 after a step.
        dict(name='s_one_frame', control=[0, 1, 53], frames=110),
        # Past the last frame without the Construction or Selling mission.
        dict(name='s_guard_wraps', control=[0, 3, 2], frames=10, mission='guard'),
        # Selling: the reverse build-up; an archive-less UndeploysInto sale
        # completes at stage 0x17.
        dict(name='s_selling', control=[0, 29, 1], frames=30, mission='selling'),
        dict(name='s_selling_undeploy_no_archive', control=[0, 29, 1], frames=30, mission='selling',
             undeploys=True),
        dict(name='s_selling_undeploy_archive', control=[0, 29, 1], frames=30, mission='selling',
             undeploys=True, archive=True),
    ]


def run_block(u, building, block, ebp=0):
    """Run one straight-line block of BuildingClass::Update with ESI = the
    building (and EBP = -1 where the block compares against it)."""
    from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_ESI
    u.mem_write(SP, bytes(0x80))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, building)
    u.reg_write(UC_X86_REG_EBP, ebp & 0xFFFFFFFF)
    run_checked(u, block[0], block[1], count=10_000)


def building_update(u, building):
    """BuildingClass::Update's construction pieces in its order."""
    invoke(u, UPDATE_ANIMATION, building)
    run_block(u, building, READY_COMMENCE_UNLESS_BUILDING)
    invoke(u, MISSION_AI, building)
    run_block(u, building, READY_COMMENCE)
    run_block(u, building, QUEUED_BSTATE, ebp=-1)


def route(case):
    """A building's frames from its creation through BuildingClass::Update's
    construction pieces (module doc), per frame: BState, queued BState, stage,
    +0x6DD, mission, queue, mission status and whether Grand_Opening ran or the
    sale reached its conversion."""
    u, read32, building, frame, calls = building_fixture(case)
    stop = {'converts': False}

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def hook(_u, address, _size, _data):
        if address == TECHNO_RECEIVE_RADIO:
            ret(12, 1)
        elif address == UNDEPLOY_VOICE:
            calls.append(['undeploy_voice'])
            ret(0)
        elif address == RADIO_BROADCAST_ALL:
            calls.append(['radio', read32(u.reg_read(UC_X86_REG_ESP) + 4)])
            ret(4)
        elif address == IS_HUMAN_PLAYER:
            ret(0, 0)
        elif address == SURVIVOR_COUNT:
            calls.append(['survivors'])
            ret(0, 0)
        elif address == OCCUPY_LIST and case['route'] == 'sale':
            # An empty list (the 0x7FFF terminator) outside native memory.
            u.mem_write(RET_MAGIC + 0x800, bytes([0xFF, 0x7F, 0xFF, 0x7F]))
            ret(4, RET_MAGIC + 0x800)
        elif address == SELL_CONVERTS:
            stop['converts'] = True
            u.emu_stop()

    u.hook_add(UC_HOOK_CODE, hook)
    u.mem_write(SCENARIO_INIT, dwords(0))
    u.mem_write(SCENARIO_FLAG_ED6B, bytes([0]))
    # In play, and +0x6E9 as BuildingClass::Init_Managers leaves it for a type
    # with a Buildup SHP (0x442CCF; Sell_Back acts only then).
    u.mem_write(building + 0x90, bytes([1]))
    u.mem_write(building + 0x6E9, bytes([1]))
    u.mem_write(building + 0x538, dwords(-1))
    u.mem_write(building + 0xC8, dwords(frame, 0, 0))
    kind = case['route']
    if kind == 'sale':
        # An idle building: BState 1 (the idle control), Guard current.
        invoke(u, BEGIN_MODE, building, 1)
        invoke(u, SELL_BACK, building, 0xFFFFFFFF)
    else:
        invoke(u, ENTER_CONSTRUCTION, building, 1, 1)
        if kind == 'computer':
            invoke(u, COMMENCE, building)
        elif kind == 'player':
            invoke(u, RECEIVE_RADIO, building, sm.YAREFN + 0x1000, 3, 0)
        elif kind == 'deploy':
            invoke(u, QUEUE_MISSION, building, 0x12, 0)
            u.mem_write(building + 0x6DD, bytes([1]))
    signed = lambda address: struct.unpack('<i', u.mem_read(address, 4))[0]
    frames = []
    # A deployed building's first Update is in its creation frame.
    for k in range(0 if kind == 'deploy' else 1, case['frames'] + 1):
        u.mem_write(FRAME, dwords(frame + k))
        before = len(calls)
        try:
            building_update(u, building)
        except Exception:
            if not stop['converts']:
                raise
        grand = any(call[0] == 'grand_opening' for call in calls[before:])
        frames.append(dict(frame=k, bstate=signed(building + 0x534), queued_bstate=signed(building + 0x538),
                           stage=signed(building + 0xF8), done=u.mem_read(building + 0x6DD, 1)[0],
                           mission=signed(building + 0xAC), queue=signed(building + 0xB4),
                           status=signed(building + 0xBC), grand_opening=grand, converts=stop['converts'],
                           calls=calls[before:]))
        if grand or stop['converts']:
            break
    return dict(input=case, frames=frames)


def route_cases():
    cases = []
    for kind in ('player', 'computer', 'deploy'):
        for control, frames in (([0, 3, 2], 12), ([0, 4, 1], 10), ([0, 2, 1], 8), ([0, 1, 0], 6),
                                ([0, 26, 2], 60), ([0, 1, 53], 120)):
            cases.append(dict(name=f'{kind}_{control[1]}x{control[2]}', route=kind, control=control,
                              frames=frames, mission='none'))
    for archive in (True, False):
        for control, frames in (([0, 3, 2], 12), ([0, 4, 1], 10), ([0, 1, 0], 6), ([0, 29, 1], 40)):
            cases.append(dict(name=f'sale_{control[1]}x{control[2]}_{"archive" if archive else "no_archive"}',
                              route='sale', control=control, frames=frames, mission='guard', undeploys=True,
                              archive=archive))
    return cases


def mission_cases():
    return [
        dict(name='m_3x2', control=[0, 3, 2], frames=12),
        dict(name='m_4x1', control=[0, 4, 1], frames=10),
        dict(name='m_2x1', control=[0, 2, 1], frames=10),
        dict(name='m_25x2', control=[0, 25, 2], frames=60),
        dict(name='m_no_buildup', control=[0, 1, 0], frames=6),
    ]


def generate():
    return {'source': 'unicorn/gamemd.exe',
            'rate': rate_rows(),
            'stepping': [stepping(case) for case in stepping_cases()],
            'mission': [mission(case) for case in mission_cases()],
            'route': [route(case) for case in route_cases()]}


def main(argv=None):
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='BuildingType Buildup rate slice 0x45F2AA..0x45F310, BuildingClass::Begin_Mode 0x447780, '
                  'BuildingClass::UpdateAnimation 0x4509D0 per frame, BuildingClass::Mission_Construction '
                  '0x449A50 per visit, and placement, deploy and UndeploysInto sale routes through '
                  'BuildingClass::Update\'s construction pieces',
            entry_points={'buildup_slice': 0x45F2AA, 'begin_mode': BEGIN_MODE,
                          'update_animation': UPDATE_ANIMATION, 'mission_construction': MISSION_CONSTRUCTION,
                          'enter_construction': ENTER_CONSTRUCTION, 'receive_radio': RECEIVE_RADIO,
                          'commence': COMMENCE, 'queue_mission': QUEUE_MISSION, 'sell_back': SELL_BACK,
                          'mission_ai': MISSION_AI, 'update_ready_commence_unless_building': 0x43FE27,
                          'update_ready_commence': 0x43FF91, 'update_queued_bstate': 0x43FFB4},
            assumptions=['rate rows: building_body_rules fixture (supplied SHP header word +6, supplied Rules '
                         '+0x1518 double), FPCW 0x0E7F',
                         'stepping/mission rows: the slave_manager fixture refinery (Building vtables, 2x2) with a '
                         'supplied construction control at Type+0xF04, the TechnoClass constructor stage state '
                         '(stage 0, step 1, rate 0, timer at the current frame), BState -1, the row mission '
                         '(+0xAC), UndeploysInto (Type+0x408) and ArchiveTarget (+0x218)'],
            substitutions=['UpdateAnimation presentation callees 0x451F60, 0x452170, 0x456FB0, 0x705D70 answered; '
                           'Grand_Opening 0x445F80, the radio broadcast 0x65ACB0, VocClass::PlayAt 0x7509E0, the '
                           'loop update 0x750D40 and SoundEvent::Release 0x406060 observed and answered',
                           'route rows: TechnoClass::Receive_Radio 0x6F4AB0, the undeploy voice 0x459C20, the '
                           'broadcast 0x65ACE0, IsHumanPlayer 0x50B6F0 (no), the survivor count 0x451330 (0) and '
                           'the sale\'s occupy list 0x5F5B90 (empty) answered; the rest of TechnoClass::AI is not run; the sale stops at 0x449CA7']),
        argv=argv)


if __name__ == '__main__':
    main()
