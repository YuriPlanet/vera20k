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
MISSION = {'construction': 0x12, 'selling': 0x13, 'guard': 5}
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
            'mission': [mission(case) for case in mission_cases()]}


def main(argv=None):
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='BuildingType Buildup rate slice 0x45F2AA..0x45F310, BuildingClass::Begin_Mode 0x447780, '
                  'BuildingClass::UpdateAnimation 0x4509D0 per frame and BuildingClass::Mission_Construction '
                  '0x449A50 per visit',
            entry_points={'buildup_slice': 0x45F2AA, 'begin_mode': BEGIN_MODE,
                          'update_animation': UPDATE_ANIMATION, 'mission_construction': MISSION_CONSTRUCTION},
            assumptions=['rate rows: building_body_rules fixture (supplied SHP header word +6, supplied Rules '
                         '+0x1518 double), FPCW 0x0E7F',
                         'stepping/mission rows: the slave_manager fixture refinery (Building vtables, 2x2) with a '
                         'supplied construction control at Type+0xF04, the TechnoClass constructor stage state '
                         '(stage 0, step 1, rate 0, timer at the current frame), BState -1, the row mission '
                         '(+0xAC), UndeploysInto (Type+0x408) and ArchiveTarget (+0x218)'],
            substitutions=['UpdateAnimation presentation callees 0x451F60, 0x452170, 0x456FB0, 0x705D70 answered; '
                           'Grand_Opening 0x445F80, the radio broadcast 0x65ACB0, VocClass::PlayAt 0x7509E0, the '
                           'loop update 0x750D40 and SoundEvent::Release 0x406060 observed and answered']),
        argv=argv)


if __name__ == '__main__':
    main()
