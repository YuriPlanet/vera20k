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
  presentation callees BuildingClass::UpdateSlotAnimLighting 0x451F60,
  SetAnimRemap 0x452170, 0x456FB0 and TechnoClass 0x705D70 (whose result only
  feeds the first) are answered.
- `mission` rows: BuildingClass::Mission_Construction (0x449A50) visit by visit
  over the same stepping. Its sound calls (VocClass::PlayAt 0x7509E0, the loop
  update 0x750D40, SoundEvent::Release 0x406060) and Grand_Opening (vt+0x4DC =
  0x445F80) and FirstContact (vt+0x274 = 0x65ACB0) are observed and
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
  0x5B3570, 0x445329..0x44533F; its first Update in the same frame), a deploy
  (vt+0x484(1, 1), UnitClass::Deploy's Queue_Mission(Construction) 0x7396D5
  and +0x6DD 0x73984E; its first Update in the same frame), or an UndeploysInto sale of an idle building
  (Sell_Back(-1) 0x447110) with or without an ArchiveTarget, stopped at the
  stage-2 visit that finds +0x6DD (0x449CA7, before the unit is built). The
  factory's other radio traffic (TechnoClass::Receive_Radio 0x6F4AB0), the
  undeploy voice (0x459C20), Sell's broadcasts (0x65ACE0), IsHumanPlayer
  (0x50B6F0, answered no), the sale's survivor count (0x451330, answered 0:
  no crew) and its occupy list (0x5F5B90, answered empty) are answered.

Usage: python -m tools.spatial_oracle.building_construction [--check|--write]
Add --joined for the retail-input joined execution corpus and its separate
provenance sidecar. This additive mode does not replace the original payload.
Add --manual-primary for the ordinary factory self-click/Event17 history and
its separate corpus. The original default and joined payloads stay unchanged.
"""
from pathlib import Path
import hashlib
import os
import struct
import sys

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, require, run_checked
from tools.spatial_oracle import building_body_rules as body_rules
from tools.spatial_oracle import slave_manager as sm
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.unit_scatter_state import SP

BEGIN_MODE, UPDATE_ANIMATION, MISSION_CONSTRUCTION = 0x447780, 0x4509D0, 0x449A50
GRAND_OPENING, PLAY_AT, LOOP_UPDATE, SOUND_RELEASE = 0x445F80, 0x7509E0, 0x750D40, 0x406060
# Legacy fixture constant name: RadioClass::FirstContact (Building vt+0x274).
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


def invoke(u, entry, this, *args, timeout_us=10_000_000):
    u.mem_write(SP, dwords(RET_MAGIC, *args))
    u.reg_write(UC_X86_REG_ECX, this)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, entry, RET_MAGIC, count=2_000_000, timeout_us=timeout_us)
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


def run_block(u, building, block, ebp=0, *, instruction_count=10_000):
    """Run one straight-line block of BuildingClass::Update with ESI = the
    building (and EBP = -1 where the block compares against it).

    Other explicitly admitted slices may declare a larger instruction_count;
    construction callers retain their original 10,000-instruction limit.
    """
    from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_ESI
    u.mem_write(SP, bytes(0x80))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, building)
    u.reg_write(UC_X86_REG_EBP, ebp & 0xFFFFFFFF)
    run_checked(u, block[0], block[1], count=instruction_count)


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
    # A deployed building's first Update is in its creation frame, and so is
    # a computer house's: Deploy runs in the unit's Update and ExitObject in
    # the yard's, the new building joins the Logic vector and LogicClass::AI
    # re-reads its count (0x55B613).
    for k in range(0 if kind in ('deploy', 'computer') else 1, case['frames'] + 1):
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


# Additive joined mode. The old generator and its checked payload are retained.
# Prepare VERA20K_BUILDING_CONSTRUCTION_ASSETS with extracted RULESMD.INI,
# ARTMD.INI, SOUNDMD.INI, MPBattleMD.ini, Hills.mmx, the three Temperate
# GTCNSTMK/GTPOWRMK/GTPILEMK SHPs and GGCNST_A/GGPOWR_A/GGPILE_A/GGCNST_B.
# The MK files live in ra2.mix/isotemp.mix: asset's ordinary catalog lookup is
# not a mounted-theater precedence proof. Supply their real extracted bytes;
# filename formation and SHP metadata execute natively below.
JOINED_NAMES = ('GACNST', 'GAPOWR', 'GAPILE')
JOINED_ART_NAMES = JOINED_NAMES + ('GACNST_A', 'GACNST_AD', 'GACNST_B', 'GACNST_BD',
                                 'GAPOWR_A', 'GAPOWR_AD', 'GAPILE_A', 'GAPILE_AD')
JOINED_ROOT = Path(os.environ.get('VERA20K_BUILDING_CONSTRUCTION_ASSETS',
                                str(Path(os.environ.get('CARGO_TARGET_DIR', 'target')) /
                                    'asset/building-construction/extract')))
JOINED_MEMORY = 0x28000000


def manual_primary_type_state(u, pointer):
    """Observed native fields used by the primary input/FindFactory paths."""
    ints = {'strength': 0xA0, 'build_category': 0xE08, 'factory': 0xEB8, 'foundation': 0xEF0,
            'ownable': 0x6CC, 'passengers': 0x5E0}
    flags = {'naval': 0xCCE, 'gap_generator': 0xCD1, 'construction_yard': 0x16B9,
             'unit_absorb': 0x16AE, 'infantry_absorb': 0x16AF,
             'invisible_in_game': 0x1701}
    return dict(**{k: struct.unpack('<i', u.mem_read(pointer + o, 4))[0]
                   for k, o in ints.items()},
                **{k: u.mem_read(pointer + o, 1)[0] for k, o in flags.items()})


def joined_inputs(root, *, layers=None, building_names=JOINED_NAMES,
                  art_names=JOINED_ART_NAMES, country_names=(), unit_names=(),
                  primary_field_names=(), planning_admission=False):
    """Original constructors and scalar readers; lexical caches/file IO supplied.

    Sound owns the source-order INI links; Reader owns the signed-CRC caches,
    allocator and physical-byte image boundary. No new parser/allocator copy.
    """
    from tools.rules_oracle.bridge_anim_inputs import Reader
    from tools.rules_oracle.bridge_child_sound import Sound, sections
    from tools.spatial_oracle.building_body_rules import INI
    from unicorn.x86_const import UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_EBX

    m = Sound.__new__(Sound)
    m.samples, m.calls = [], []
    sound = sections((root / 'SOUNDMD.INI').read_bytes())
    sound_names = ('Dummy', 'MenuScold') if planning_admission else ('Dummy',)
    selected = {n: sound[n] for n in ('Defaults',) + sound_names}
    selected['SoundList'] = {k: v for k, v in sound['SoundList'].items() if v in sound_names}
    if planning_admission:
        # Original Sound reader requests the physical MenuScold sample name.
        # Its immutable sample-catalog identity is supplied; playback is not.
        m.sample_ids = {'umenscol': 0}
    Reader.__init__(m, root, selected)
    u = m.u
    m.selected = selected
    u.mem_write(0x87E2A0, dwords(1))
    u.mem_write(0x87E294, dwords(m.alloc(0x100)))
    m.invoke(0x4072C0, 0x87E250)
    u.mem_write(0xB1D378, dwords(0x7EB6D4, m.alloc(64), 16, 1, 0, 10))
    m.invoke(0x7510D0, INI)
    # The lookup allocates; keep it outside a check that `python -O` drops.
    dummy = m.invoke(0x7514D0, m.cstring('Dummy'))
    require(dummy == 0, f'Dummy sound index {dummy}')
    m.rules = m.alloc(0x5000)
    u.mem_write(0x8871E0, dwords(m.rules))
    m.invoke(0x665650, m.rules)
    m.scold_constructor_default = struct.unpack('<i', u.mem_read(m.rules + 0x700, 4))[0]
    m.invoke(0x4E7CF0, 0)  # all original MissionControl static constructors.
    scene = m.alloc(0x1500)
    u.mem_write(0xA8B230, dwords(scene))
    u.mem_write(scene + 0x1258, dwords(0))  # supplied prior Scenario: Temperate.
    art = sections((root / 'ARTMD.INI').read_bytes())
    m.make_ini({n: art[n] for n in art_names if n in art})
    art_cache = bytes(u.mem_read(INI, 0x40))
    m.types = {}
    if layers is not None:
        # A selected live-type history retains one real constructor registry.
        # Legacy fixtures keep their historical supplied registry boundary.
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x4E7A60, 0x4E7A96)
    for name in building_names:
        pointer = m.alloc(0x2000)
        m.invoke(0x45DD90, pointer, (m.cstring(name),))
        m.types[name] = pointer
    m.primary_constructor_inputs = {
        name: manual_primary_type_state(u, m.types[name]) for name in primary_field_names}
    m.country_catalog = []
    if country_names:
        # Supply registry capacity and physical [Countries] name/order, not a
        # second country reader. Real constructors enroll each name/index;
        # original Owner lookup4750D0/5117D0 consumes this native catalog.
        countries = m.alloc(len(country_names) * 4)
        u.mem_write(0xA83C98, dwords(0x7EB6D4, countries, len(country_names),
                                    1, 0, 10))
        for index, name in enumerate(country_names):
            pointer = m.alloc(0x200)
            m.invoke(0x5113F0, pointer, (m.cstring(name),))
            assert m.read32(pointer + 0xB8) == index
            assert m.read32(countries + index * 4) == pointer
            m.country_catalog.append(dict(index=index, name=name))
    m.unit_types = {}
    for name in unit_names:
        pointer = m.alloc(0x1100)
        m.invoke(0x7470D0, pointer, (m.cstring(name),))
        m.unit_types[name] = pointer
    m.layers = []
    m.scold_layers = []
    # Prior selected AnimTypes from the real rules list, in its source order.
    # Whole Rules::Process/other registry entries remain outside this fixture.
    animations = sections((root / 'RULESMD.INI').read_bytes())['Animations']
    m.animation_list = []
    for key, name in animations.items():
        if name in art_names and name not in building_names:
            m.invoke(0x428B80, m.cstring(name))
            m.animation_list.append(dict(key=key, name=name))
    for filename in (layers if layers is not None else
                     ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.mmx')):
        path = root / filename
        if not path.is_file():
            assert filename == 'LANGRULE.INI', filename
            m.layers.append(dict(file=filename, absent=True))
            continue
        physical = sections(path.read_bytes())
        mission_names = tuple(m.string(m.read32(0x816CAC + i * 4)) for i in range(32))
        names = building_names + unit_names + ('General', 'AudioVisual') + mission_names
        retained = {n: physical[n] for n in names if n in physical}
        m.make_ini(retained)
        rules_ini = m.alloc(0x40)
        u.mem_write(rules_ini, bytes(u.mem_read(INI, 0x40)))
        u.mem_write(INI, art_cache)
        for begin, end in ((0x670C99, 0x670CC0), (0x66B35E, 0x66B385),
                           (0x66A96A, 0x66A9BD)):
            u.mem_write(SP, bytes(0x200))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ESI, m.rules)
            u.reg_write(UC_X86_REG_EDI, rules_ini)
            # 66A96A includes the previous sound's retained store/push prefix.
            u.reg_write(UC_X86_REG_EAX, m.read32(m.rules + 0x6C0))
            run_checked(u, begin, end, count=2_000_000)
        if planning_admission:
            # Same retained-default ScoldSound block used by foot_scold_latch;
            # the real 7514D0 lookup resolves the selected native registry.
            before = struct.unpack('<i', u.mem_read(m.rules + 0x700, 4))[0]
            u.mem_write(SP, bytes(0x200))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ESI, m.rules)
            u.reg_write(UC_X86_REG_EDI, rules_ini)
            run_checked(u, 0x66ABCD, 0x66AC18)
            m.scold_layers.append(dict(file=filename,
                physical_value=physical.get('AudioVisual', {}).get('ScoldSound'),
                native_buffer=m.string(SP + 0x14), before=before,
                after=struct.unpack('<i', u.mem_read(m.rules + 0x700, 4))[0]))
        reads = {n: bool(m.invoke(0x45FE50, p, (rules_ini,)) & 255)
                 for n, p in m.types.items() if n not in primary_field_names}
        for name in primary_field_names:
            pointer = m.types[name]
            # Existing placement_render's exact native Image block supplies
            # the ART section name before the unlayered Foundation read.
            u.mem_write(SP, bytes(0x200))
            u.mem_write(SP + 0x1B4, dwords(rules_ini))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_EBX, pointer)
            run_checked(u, 0x5F92F9, 0x5F9340)
            # The primary-action profile needs these scalar fields, not
            # GAWEAP's demand-loaded buildup/file/image/animation lifecycle.
            # Execute exact retained-default/store blocks, never VERA values.
            for begin, end in ((0x45FE78, 0x45FE97),  # BuildCat
                               (0x46051A, 0x46054B),  # Factory
                               (0x713F92, 0x713FAC),  # GapGenerator
                               (0x460A24, 0x460A44),  # ConstructionYard
                               (0x7149E1, 0x7149FB),  # Owner
                               (0x714A63, 0x714A7D),  # Naval
                               (0x714B35, 0x714B56),  # Passengers
                               (0x460988, 0x4609D6),  # Absorb flags
                               (0x460DEB, 0x460E07),  # InvisibleInGame
                               (0x5F94D3, 0x5F94F3)):  # Strength
                u.mem_write(SP, bytes(0x200))
                u.reg_write(UC_X86_REG_ESP, SP)
                u.reg_write(UC_X86_REG_EBP, pointer)
                u.reg_write(UC_X86_REG_EBX, pointer + 0x24)
                u.reg_write(UC_X86_REG_ESI, rules_ini)
                u.reg_write(UC_X86_REG_EDI, rules_ini)
                if begin == 0x5F94D3:
                    u.reg_write(UC_X86_REG_EBP, pointer + 0x24)
                    u.reg_write(UC_X86_REG_EBX, pointer)
                run_checked(u, begin, end)
        for name, pointer in m.unit_types.items():
            # FindFactory(0,0,0,House) reads only the query's real RTTI,
            # Ownable and Naval fields. Run their actual common reader blocks;
            # the wider UnitType reader/weapon and production admission are
            # outside this profile, rather than supplied gameplay answers.
            for begin, end in ((0x7149E1, 0x7149FB), (0x714A63, 0x714A7D)):
                u.mem_write(SP, bytes(0x200))
                u.reg_write(UC_X86_REG_ESP, SP)
                u.reg_write(UC_X86_REG_EBP, pointer)
                u.reg_write(UC_X86_REG_EDI, rules_ini)
                u.reg_write(UC_X86_REG_EBX, m.cstring(name))
                run_checked(u, begin, end)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, rules_ini)
        run_checked(u, 0x679C92, 0x679CAF, required_addresses=(0x5B3760,))
        m.layers.append(dict(file=filename, absent=False, sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                             type_read=reads))
        if primary_field_names:
            m.layers[-1]['primary_fields'] = {
                name: manual_primary_type_state(u, m.types[name]) for name in primary_field_names}
    if primary_field_names:
        m.invoke(0x45B1C0, 0)
        for name in primary_field_names:
            pointer = m.types[name]
            for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, pointer),
                               (UC_X86_REG_EBX, pointer + 0x24),
                               (UC_X86_REG_EDI, pointer + 0x1F8), (UC_X86_REG_ESI, INI)):
                u.reg_write(reg, value)
            run_checked(u, 0x461225, 0x46125D)
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_EBP, pointer)
            run_checked(u, 0x46152C, 0x461547)
    m.anim_rows = []
    for i in range(m.read32(0x8B4160)):
        pointer = m.read32(m.read32(0x8B4154) + i * 4)
        name = m.string(pointer + 0x24)
        read = m.invoke(0x427D00, pointer, (INI,))
        m.anim_rows.append(dict(pointer=pointer, **m.result(name, pointer, read)))
    m.type_rows = []
    for name, p in m.types.items():
        signed = lambda offset: struct.unpack('<i', u.mem_read(p + offset, 4))[0]
        byte = lambda offset: u.mem_read(p + offset, 1)[0]
        m.type_rows.append(dict(name=name, strength=signed(0xA0),
            buildup_control=list(struct.unpack('<3i', u.mem_read(p + 0xF04, 12))),
            idle_control=list(struct.unpack('<3i', u.mem_read(p + 0xF10, 12))),
            buildup_sound=signed(0xE6C), factory=signed(0xEB8),
            free_unit=bool(m.read32(p + 0xEA0)), produce_cash=signed(0x1558),
            infantry_self_heal=signed(0x1564), unit_self_heal=signed(0x1568),
            # ctor45DEE6 retains building-facing0; DeployFacing ctor45DEEC
            # starts128 and reader460C6C stores it separately. Grand4463C0
            # reads the Helipad byte at16CB, never the facing dword atED8.
            super_weapon=signed(0x16F0), helipad=byte(0x16CB),
            building_facing=signed(0xED8), deploy_facing=signed(0xEDC),
            has_stupid_guard_mode=byte(0x16B5),
            powered_special=byte(0x1574), refinery=byte(0x16BB),
            slots=[dict(slot=i, name=m.string(p + 0xF4C + i * 0x44),
                        damaged=m.string(p + 0xF5C + i * 0x44),
                        powered=byte(0xF8C + i * 0x44),
                        powered_special=byte(0xF8F + i * 0x44))
                   for i in range(21) if m.string(p + 0xF4C + i * 0x44)]))
    return m


class JoinedFixture:
    """Composes existing map fixture with native retail-created types/Anims.

    Building/House/map admission is supplied prior state. Whole Grand_Opening,
    FirstContact/Transmit/receivers, Anim allocation/constructor/Unlimbo/Start/AI
    and scalar deletion/destructors execute. No gameplay call is answered.
    """
    def __init__(self, inputs, seed=31, *, arena_size=0x200000):
        from tools.spatial_oracle.refinery_dock import make_dock_fixture, HOUSE
        from tools.rules_oracle.bridge_anim_lists import HEAP
        from tools.spatial_oracle.building_slot_replacement import VECTORS
        from tools.native_oracle import image_bytes, _sections
        from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EBX, UC_X86_REG_FPCW

        self.u, self.call, self.read32 = make_dock_fixture(
            dict(seed=seed, linked=False, refinery_flag=False, dock_unload=False, human=True))
        u = self.u
        assert u.reg_read(UC_X86_REG_FPCW) == 0x0E7F
        input_arena_size = getattr(inputs, 'arena_size', 0x400000)
        u.mem_map(HEAP, input_arena_size)
        u.mem_write(HEAP, bytes(inputs.u.mem_read(HEAP, input_arena_size)))
        for address, size in ((0x8B4150, 24), (0xB1D378, 24), (0x87E180, 0x12C),
                              (0xA8E3A8, 32 * 32)):
            u.mem_write(address, bytes(inputs.u.mem_read(address, size)))
        u.mem_write(0x8871E0, dwords(inputs.rules))
        self.types, self.rules = inputs.types, inputs.rules
        if arena_size < 0x200000 or arena_size % 0x1000:
            raise ValueError('Joined arena must be page-aligned and at least 2MiB')
        u.mem_map(JOINED_MEMORY, arena_size)
        self.heap = JOINED_MEMORY + 0x20000
        self.heap_end = JOINED_MEMORY + arena_size
        self.vbuf = JOINED_MEMORY + 0x8000
        self.events, self.draws, self.advances, self.pending = [], [], [], []
        self.allocations, self.executed = [], set()
        native_image = image_bytes()
        code_sections = [(rva, raw, size) for rva, raw, size, _, flags
                         in _sections(native_image) if size and flags & 0x20000000]
        self.code_spans = [(0x400000 + rva, size) for rva, _, size in code_sections]
        self.original_code = [bytes(u.mem_read(a, n)) for a, n in self.code_spans]
        assert self.original_code == [native_image[raw:raw + size] for _, raw, size in code_sections]
        u.hook_add(UC_HOOK_CODE, self.hook)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x40CB80, 0x40CBB6)
        u.mem_write(0x87F77C, dwords(self.vbuf, 128))
        run_checked(u, 0x4A8630, 0x4A866D)
        for i in range(5):
            u.mem_write(0x8A0360 + i * 24 + 4, dwords(self.vbuf + 0x1000 + i * 0x200, 128))
        for i, (address, vt, _store) in enumerate(VECTORS):
            u.mem_write(address, dwords(vt, self.vbuf + 0x2000 + i * 0x200, 128, 1, 0, 10))
        tact = JOINED_MEMORY + 0x5000
        u.mem_write(tact, dwords(0x7F4348, 0x7F432C, 0x7F4324, 0x7F431C))
        u.reg_write(UC_X86_REG_ESI, tact)
        u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(u, 0x6D1DC5, 0x6D1E24)
        u.mem_write(0xB0CE30, dwords(640, 480))
        u.mem_write(0x886FA8, dwords(640, 480))
        u.mem_write(0xB0CE08, dwords(0, 0, 0))
        u.mem_write(0xB0CD48, struct.pack('<d', 0.1953125))
        u.mem_write(0xA8EB60, dwords(3))
        u.mem_write(0xB1D310, struct.pack('<hh', -1, -1))
        # Original no-sample Dummy still allocates an event; never skip it.
        pool, event = JOINED_MEMORY + 0x100, JOINED_MEMORY + 0x200
        u.mem_write(pool, dwords(event))
        u.mem_write(event, bytes(0x288))
        u.mem_write(0x87E2A8, dwords(pool))
        u.mem_write(0x87E2A4, dwords(1))
        u.mem_write(0x816108, dwords(1))
        u.mem_write(0x8464AC, b'\1')
        invoke(u, 0x4072C0, 0x87E180)
        invoke(u, 0x65C6D0, 0x886B88, seed)
        for offset in (0x1FC, 0x5778, 0x5779):
            u.mem_write(HOUSE + offset, b'\0')
        u.mem_write(HOUSE + 0x2A4, dwords(-1, 0, 0))
        u.mem_write(SCENARIO_INIT, dwords(0))
        u.mem_write(SCENARIO_FLAG_ED6B, b'\0')
        u.mem_write(FRAME, dwords(0))
        self.events.clear()

    def ret(self, value=0, cleanup=0):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, self.read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def allocate(self, size):
        """One arena owner for fixture priors and original operator-new calls."""
        pointer = self.heap
        self.heap += (size + 15) & ~15
        assert self.heap < self.heap_end
        return pointer

    def anim_type_identity(self, pointer):
        # AbstractType ctor410812/41088D..4108B7 retains the canonical ID in
        # its25-byte field+24. Instance ctor421EE5 retains this pointer at+C8;
        # +CC is the independent attached Object pointer (cleared421EEE).
        assert pointer
        raw = bytes(self.u.mem_read(pointer + 0x24, 25))
        assert b'\0' in raw
        return dict(type=pointer, type_name=raw.split(b'\0', 1)[0].decode('latin1'))

    def hook(self, u, pc, _size, _data):
        from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI
        self.executed.add(pc)
        for pending in self.pending[:]:
            if pc == pending['return_pc']:
                row = pending['row']
                row['result'] = u.reg_read(UC_X86_REG_EAX)
                row['state_after'] = bytes(u.mem_read(row['this'], 0x3F4)).hex()
                self.pending.remove(pending)
        entries = {0x65ACB0: 'first_contact', 0x65A970: 'transmit',
                   0x43C2D0: 'building_receive', 0x6F4AB0: 'techno_receive',
                   GRAND_OPENING: 'grand_opening', 0x451890: 'slot_allocate',
                   0x421EA0: 'anim_ctor', 0x5F4EC0: 'unlimbo',
                   0x5F5850: 'mark', 0x4A9720: 'layer_submit',
                   0x55BAA0: 'logic_add', 0x424CE0: 'anim_start',
                   0x423AC0: 'anim_ai', 0x426590: 'scalar_delete',
                   0x4228E0: 'anim_dtor', 0x5F3B80: 'object_dtor',
                   0x55B8D0: 'logic_remove', 0x4549B0: 'operational_edge',
                   PLAY_AT: 'sound_play', LOOP_UPDATE: 'sound_watch',
                   SOUND_RELEASE: 'sound_release', 0x4055C0: 'sound_update'}
        sp, this = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        frame = self.read32(FRAME)
        if pc in entries:
            row = dict(event=entries[pc], this=this, frame=frame)
            if pc == 0x65ACB0:
                row['message'] = self.read32(sp + 4)
            if pc == 0x65A970:
                row.update(message=self.read32(sp + 4), receiver=self.read32(sp + 12))
            if pc == 0x451890:
                row['slot'] = self.read32(sp + 8)
            if pc == 0x421EA0:
                row.update(**self.anim_type_identity(self.read32(sp + 4)),
                           delay=self.read32(sp + 12),
                           flags=self.read32(sp + 20))
            if pc == 0x423AC0:
                from tools.spatial_oracle.anim_bouncer_launch import constructor_state
                row['runtime_before'] = constructor_state(u, this)['runtime']
            self.events.append(row)
        if pc in (0x65C780, 0x65C7E0):
            row = dict(entry=pc, site=self.read32(sp) - 5, frame=frame, this=this,
                       args=[self.read32(sp + 4), self.read32(sp + 8)] if pc == 0x65C7E0 else [],
                       state_before=bytes(u.mem_read(this, 0x3F4)).hex())
            self.draws.append(row)
            self.pending.append(dict(return_pc=self.read32(sp), row=row))
        if pc == 0x65C84B:
            self.advances.append(dict(frame=frame, this=u.reg_read(UC_X86_REG_EDX),
                                      raw=u.reg_read(UC_X86_REG_ESI)))
        if pc == 0x65C880:
            self.advances[-1].update(masked=u.reg_read(UC_X86_REG_EAX),
                                    range=u.reg_read(UC_X86_REG_EDI),
                                    rejected=u.reg_read(UC_X86_REG_EAX) > u.reg_read(UC_X86_REG_EDI))
        if pc in PRESENTATION:
            self.events.append(dict(event='presentation_boundary', address=pc, frame=frame))
            self.ret(cleanup=PRESENTATION[pc])
        elif pc == 0x7C8E17:
            size = self.read32(sp + 4)
            pointer = self.allocate(size)
            self.allocations.append(dict(pointer=pointer, size=size, frame=frame))
            self.ret(pointer)
        elif pc == 0x7C8B3D:
            self.events.append(dict(event='arena_release', pointer=self.read32(sp + 4), frame=frame))
            self.ret()

    def rng(self, *, include_mapgen=False):
        from tools.spatial_oracle.unit_source_scatter import SCENARIO
        streams = (('main', 0x886B88), ('scenario', SCENARIO + 0x218))
        if include_mapgen:
            streams += (('mapgen', 0xABE890),)
        return {name: bytes(self.u.mem_read(pointer, 0x3F4)).hex()
                for name, pointer in streams}

    def code_unchanged(self):
        return self.original_code == [bytes(self.u.mem_read(a, n)) for a, n in self.code_spans]

    def building(self, name, pointer, radio_items, coord=(6, 9), health=None):
        from tools.spatial_oracle.refinery_dock import place_building, cell
        u = self.u
        u.mem_write(pointer, bytes(0x800))
        place_building(u, pointer, coord)
        u.mem_write(pointer + 0x520, dwords(self.types[name]))
        strength = self.read32(self.types[name] + 0xA0)
        u.mem_write(pointer + 0x6C, dwords(strength if health is None else health))
        u.mem_write(pointer + 0x90, b'\1')
        u.mem_write(pointer + 0x74, b'\1')
        u.mem_write(cell(*coord) + 0x12C, dwords(0x18))  # supplied visible/unshrouded cell.
        # Execute the actual Mission/Techno constructor stores for this chain,
        # excluding the surrounding admission/constructor RNG lifecycle.
        from unicorn.x86_const import UC_X86_REG_ESI
        u.reg_write(UC_X86_REG_ESI, pointer)
        run_checked(u, 0x5B2DA8, 0x5B2DE9)
        u.reg_write(UC_X86_REG_ESI, pointer)
        run_checked(u, 0x6F2B4B, 0x6F2B87)
        u.mem_write(pointer + 0x534, dwords(-1, -1))
        u.mem_write(pointer + 0x6E9, b'\1\1')
        u.mem_write(pointer + 0xE0, dwords(0x7E180C, radio_items, 1, 1))
        u.mem_write(radio_items, dwords(0))
        # Original Building constructor43BAA9/43BAB4 calls handle ctor405BE0.
        invoke(u, 0x405BE0, pointer + 0x6A0)
        invoke(u, 0x405BE0, pointer + 0x6B4)
        # Both DrawingLayer and Logic are already admitted; this is not Unlimbo.
        u.mem_write(pointer + 0x98, b'\1')

    def snapshot(self, pointer):
        from tools.spatial_oracle.refinery_dock import HOUSE
        from tools.spatial_oracle.anim_bouncer_launch import constructor_state
        u, read = self.u, self.read32
        anims = [read(read(0xA8E9AC) + i * 4) for i in range(read(0xA8E9B8))]
        return dict(building=snapshot(u, pointer),
                    body_facing=[struct.unpack('<H', u.mem_read(pointer + o, 2))[0]
                                 for o in (0x388, 0x38C)],
                    mission_timer=list(struct.unpack('<3i', u.mem_read(pointer + 0xC8, 12))),
                    placed=u.mem_read(pointer + 0x6E4, 1)[0],
                    old_operational=u.mem_read(pointer + 0x6C8, 1)[0],
                    queued_bstate=struct.unpack('<i', u.mem_read(pointer + 0x538, 4))[0],
                    house_flags=[u.mem_read(HOUSE + o, 1)[0] for o in (0x1FC, 0x5778, 0x5779)],
                    contacts=[read(read(pointer + 0xE4) + i * 4) for i in range(read(pointer + 0xE8))],
                    slots=[read(pointer + 0x55C + i * 4) for i in range(21)],
                    sound_handle=list(struct.unpack('<4I', u.mem_read(pointer + 0x6A0, 16))),
                    logic=[read(self.vbuf + i * 4) for i in range(read(0x87F788))],
                    anims=[dict(pointer=a, **self.anim_type_identity(read(a + 0xC8)),
                                owner_object=read(a + 0xCC), **constructor_state(u, a)) for a in anims])


def joined_route(inputs, name, route, health=None, guard_control=False):
    from tools.spatial_oracle.refinery_dock import BLD, OTHER, BLD_ITEMS, OTHER_ITEMS
    f = JoinedFixture(inputs)
    u, read = f.u, f.read32
    f.building(name, BLD, BLD_ITEMS, health=health)
    if guard_control:
        # Explicit asymmetric control, never an ordinary-retail input claim.
        u.mem_write(f.types[name] + 0x16B5, b'\0')
    f.building('GACNST', OTHER, OTHER_ITEMS, coord=(12, 12))
    invoke(u, BEGIN_MODE, OTHER, 1)
    u.mem_write(OTHER + 0xAC, dwords(5))
    u.mem_write(OTHER + 0x6E4, b'\1')
    u.mem_write(OTHER + 0x6C8, b'\1')
    u.mem_write(f.vbuf, dwords(BLD))
    u.mem_write(0x87F788, dwords(1))
    u.mem_write(FRAME, dwords(0))
    before = f.rng()
    f.events.clear()
    invoke(u, ENTER_CONSTRUCTION, BLD, 1, 1)
    if route == 'player':
        # HousePlace4FB1F1 HELLO yard -> child, then4FB2AD C child -> yard,
        # then4FB4A6 yard.FirstContact(BREAK). Queue release/Unlimbo supplied.
        invoke(u, 0x65A970, OTHER, 2, 0, BLD)
        invoke(u, 0x65A970, BLD, 0xC, 0, OTHER)
        invoke(u, 0x65ACB0, OTHER, 3)
    elif route == 'computer':
        invoke(u, COMMENCE, BLD)
    else:
        invoke(u, QUEUE_MISSION, BLD, 18, 0)
        # Native Unit::Deploy73984E store, after QueueMission7396D5.
        u.mem_write(BLD + 0x6DD, b'\1')
    creation = dict(frame=0, **f.snapshot(BLD), events=f.events.copy(), rng=f.rng())
    frames, completion = [], None
    for frame in range(0 if route in ('deploy', 'computer') else 1, 160):
        u.mem_write(FRAME, dwords(frame))
        mark = len(f.events)
        # Original operational/header logic precedes the original completion
        # pieces. Whole Building::AI/Techno::AI and Logic scheduler are excluded.
        u.reg_write(UC_X86_REG_ECX, BLD)
        run_block(u, BLD, (0x43FB20, 0x43FC39))
        building_update(u, BLD)
        # Native Logic55B613 re-reads count, so appended live Anim visits run in
        # this frame. This fixture has exactly one admitted building first.
        i = 1
        while i < read(0x87F788):
            invoke(u, 0x423AC0, read(f.vbuf + i * 4))
            i += 1
        # Presentation scheduler's first native no-sample event update.
        event = read(BLD + 0x6A0)
        if event and read(event + 0x138):
            invoke(u, 0x4055C0, event)
        new_events = f.events[mark:]
        if any(e['event'] == 'grand_opening' for e in new_events):
            completion = frame
        row = dict(frame=frame, **f.snapshot(BLD), events=new_events,
                   rng_sha256={k: hashlib.sha256(bytes.fromhex(v)).hexdigest() for k, v in f.rng().items()})
        if completion is not None:
            row['rng'] = f.rng()
        frames.append(row)
        if completion is not None and frame == completion + 1:
            break
    assert completion is not None, (name, route, 'no completion')
    assert not f.pending
    assert f.code_unchanged()
    required = {GRAND_OPENING, 0x451890, 0x421EA0, 0x424CE0, 0x423AC0}
    assert required <= f.executed, (name, route, [hex(a) for a in required - f.executed], frames[-1])
    return dict(input=dict(name=name, route=route, seed=31, health=health,
                           clear_stupid_guard_control=guard_control),
                creation=creation, completion_frame=completion, frames=frames,
                allocations=f.allocations, rng_requests=f.draws, rng_advances=f.advances,
                rng_before=before, rng_after=f.rng(), original_executable_sections_unchanged=True)


def joined_factory_controls(inputs):
    from tools.spatial_oracle.refinery_dock import BLD, OTHER, BLD_ITEMS, OTHER_ITEMS, HOUSE
    f = JoinedFixture(inputs)
    u, read = f.u, f.read32
    f.building('GACNST', BLD, BLD_ITEMS)
    f.building('GACNST', OTHER, OTHER_ITEMS, coord=(20, 20))
    items = JOINED_MEMORY + 0x1800
    u.mem_write(HOUSE + 0x6C, dwords(items))
    u.mem_write(HOUSE + 0x78, dwords(2))
    u.mem_write(items, dwords(BLD, OTHER))
    # Supplied prior HouseType registry/Ownable mask. Full country startup is
    # outside the selected Rules/ART cache, so this is a control, not retail
    # owner-mask evidence. FindFactory still runs all its original consumers.
    for name in ('GACNST', 'GAPOWR'):
        u.mem_write(f.types[name] + 0x6CC, dwords(1))
    u.mem_write(0xA8B238, dwords(1))
    rows = []
    for label, primary, limbo, selling, naval in (
            ('last_without_primary', [0, 0], [0, 0], [0, 0], 0),
            ('first_primary', [1, 0], [0, 0], [0, 0], 0),
            ('last_primary', [0, 1], [0, 0], [0, 0], 0),
            ('limbo_primary_refused', [0, 1], [0, 1], [0, 0], 0),
            ('selling_primary_refused', [0, 1], [0, 0], [0, 1], 0),
            ('queued_selling_primary_refused', [0, 1], [0, 0], [0, 2], 0),
            ('naval_factory_refused_for_building', [1, 0], [0, 0], [0, 0], 1)):
        for i, pointer in enumerate((BLD, OTHER)):
            u.mem_write(pointer + 0x3D3, bytes([primary[i]]))
            u.mem_write(pointer + 0x81, bytes([limbo[i]]))
            u.mem_write(pointer + 0xAC, dwords(19 if selling[i] == 1 else 5))
            u.mem_write(pointer + 0xB4, dwords(19 if selling[i] == 2 else -1))
        u.mem_write(f.types['GACNST'] + 0xCCE, bytes([naval]))
        result = invoke(u, 0x5F7900, f.types['GAPOWR'], 0, 0, 0, HOUSE)
        rows.append(dict(name=label, order=[BLD, OTHER], primary=primary,
                         limbo=limbo, selling=selling, naval=naval, found=result))
    u.mem_write(f.types['GACNST'] + 0xCCE, b'\0')
    for pointer in (BLD, OTHER):
        u.mem_write(pointer + 0x81, b'\0')
        u.mem_write(pointer + 0x3D3, b'\0')
        u.mem_write(pointer + 0xAC, dwords(5))
        u.mem_write(pointer + 0xB4, dwords(-1))
    primary = []
    for pointer in (BLD, OTHER):
        value = invoke(u, 0x448070, pointer, 0)
        primary.append(dict(admitted=pointer, changed=value,
                            primary=[u.mem_read(p + 0x3D3, 1)[0] for p in (BLD, OTHER)]))
    assert primary[0]['primary'] == [1, 0] and primary[1]['primary'] == [1, 0]
    assert f.code_unchanged()
    return dict(find=rows, set_primary=primary, original_executable_sections_unchanged=True,
                limits=['FindFactory(0,0,false,House) is the ordinary human PLACE identity path; optional CanBuild/online/aircraft/unit admission is excluded.',
                        'Building objects/House order and Ownable mask1 are supplied prior state; full country/map admission is excluded.',
                        'Full SetPrimary448070 including HouseDetermineEdge50DB00 runs; Limbo and ChangeOwner transitions are not executed by this control.'])


def joined_dummy_sound(inputs):
    """Stock Dummy's actual event lifecycle and mid-construction sound slice."""
    from tools.spatial_oracle.refinery_dock import BLD, BLD_ITEMS
    from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI
    cases = []
    for cleanup in ('completion', 'destruction_sound_slice'):
        f = JoinedFixture(inputs)
        f.building('GAPOWR', BLD, BLD_ITEMS)
        u, read = f.u, f.read32
        h, xyz = BLD + 0x6A0, BLD + 0x9C
        before = f.rng()
        phases = []
        event = 0

        def save(phase):
            phases.append(dict(phase=phase, handle=list(struct.unpack('<4I', u.mem_read(h, 16))),
                event=event, event_state=read(event + 0x1C) if event else None,
                event_flags=read(event + 0x18) if event else None,
                event_serial=read(event + 0x138) if event else None,
                live_serial=read(0x81610C), event_count=read(0x87E28C)))

        save('before')
        u.mem_write(SP, dwords(RET_MAGIC, h))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, 0)  # selected original Dummy index, fixture-relative.
        u.reg_write(UC_X86_REG_EDX, xyz)
        run_checked(u, PLAY_AT, RET_MAGIC, count=2_000_000)
        event = u.reg_read(UC_X86_REG_EAX)
        assert event and read(h) == event
        save('play')
        if cleanup == 'completion':
            invoke(u, 0x4055C0, event)
            save('first_audio_update')
            u.reg_write(UC_X86_REG_EDX, h)
            invoke(u, LOOP_UPDATE, xyz)
            save('construction_watch')
            invoke(u, SOUND_RELEASE, h)
            save('construction_completion_release')
        else:
            # Real destructor43BD1F..43BD3A: release+6A0, reset+6B4/+6A0.
            # The wider Building destructor/country/count cleanup is excluded.
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ESI, BLD)
            u.reg_write(UC_X86_REG_EDI, BLD + 0x6B4)
            run_checked(u, 0x43BD1F, 0x43BD3A, count=2_000_000,
                        required_addresses=(SOUND_RELEASE, 0x405C00))
            save('destruction_sound_release_reset')
        assert f.rng() == before and not f.draws and f.code_unchanged()
        cases.append(dict(cleanup=cleanup, phases=phases, events=f.events,
                          rng_before=before, rng_after=f.rng(), rng_requests=f.draws,
                          original_executable_sections_unchanged=True))
    return dict(cases=cases,
                limits=['Dummy sample count is zero, but its definition is enabled: native Play allocates an event and assigns a valid tagged handle.',
                        'No sample/device path or global sound pool cleanup executes; event_count can remain1 after state/handle cleanup.',
                        'The destruction case executes only43BD1F..43BD3A, not the whole Building destructor.'])


def manual_primary_cases():
    """Inputs only. All action, event, byte and factory outputs execute natively."""
    factory = lambda **kw: dict(type='GAWEAP', **kw)
    return [
        dict(name='ordinary_self_click_switch_back_and_stale_event',
             actors=[factory(), factory()],
             operations=[dict(op='automatic', actor=1), dict(op='automatic', actor=2),
                         dict(op='click', actor=1), dict(op='click', actor=2),
                         dict(op='click', actor=2), dict(op='click', actor=1),
                         dict(op='repeat_event', event=1),
                         dict(op='click', actor=2, planning=True)]),
        dict(name='retained_limbo_primary_bytes',
             supplied_control='No ordinary stock GAWEAP live/true-primary Limbo producer is claimed.',
             actors=[factory(primary=1, limbo=1), factory()],
             operations=[dict(op='event', actor=2), dict(op='event', actor=1),
                         dict(op='event', actor=1)]),
        dict(name='exact_factory_naval_and_owner_peer_groups',
             actors=[factory(primary=1), factory(), factory(primary=1),
                     factory(primary=1, type_overrides=dict(naval=1)),
                     dict(type='GAPILE', primary=1), factory(owner=1, primary=1)],
             operations=[dict(op='event', actor=2), dict(op='event', actor=6),
                         dict(op='repeat_event', event=1)]),
        dict(name='receiver_liveness_and_body_gates',
             actors=[factory(primary=1), factory(alive=0),
                     dict(type='GAPOWR')],
             operations=[dict(op='event', actor=2), dict(op='event', actor=None),
                         dict(op='event', actor=3),
                         dict(op='event', actor=2, changes=dict(alive=1, limbo=1,
                              online=0, health=0, mission=19, queued_mission=19)),
                         dict(op='repeat_event', event=3)]),
        dict(name='invisible_factory_self_action_and_received_event',
             actors=[factory(primary=1),
                     factory(type_overrides=dict(invisible_in_game=1))],
             operations=[dict(op='click', actor=2), dict(op='event', actor=2)]),
        dict(name='construction_yard_received_event_edge',
             actors=[dict(type='GACNST'), dict(type='GACNST')],
             operations=[dict(op='event', actor=1), dict(op='event', actor=2),
                         dict(op='repeat_event', event=1)]),
    ]


def manual_primary_admission_cases():
    """Supplied prior path counts; original admission decides the outcomes."""
    return [dict(name=f'planning_prior_{count}_selected_no_path',
                 planning_admission=dict(prior_house_nonempty_paths=count),
                 actors=[dict(type='GAWEAP', primary=1), dict(type='GAWEAP')],
                 operations=[dict(op='click', actor=2, planning=True)])
            for count in (0, 127, 128)]


def manual_primary_case(inputs, case):
    """One JoinedFixture history; no action/event/selection decision is supplied."""
    from tools.native_oracle import (NativeCallTrace, initialize_empty_windows_seh,
                                    checked_is_bad_read_ptr_transport)
    from tools.spatial_oracle.refinery_dock import HOUSE
    from tools.spatial_oracle.unit_source_scatter import SCENARIO
    from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EBX

    f = JoinedFixture(inputs)
    u, read = f.u, f.read32
    invoke(u, 0x45B1C0, 0)
    invoke(u, 0x65C6D0, 0xABE890, 31)
    if not any(start <= 0 <= end for start, end, _ in u.mem_regions()):
        initialize_empty_windows_seh(u)
    houses = [HOUSE, f.allocate(0x6000)]
    u.mem_write(houses[1], bytes(u.mem_read(HOUSE, 0x6000)))
    for index, house in enumerate(houses):
        u.mem_write(house + 0x30, dwords(index))
    house_items = f.allocate(8)
    u.mem_write(house_items, dwords(*houses))
    u.mem_write(0xA8022C, dwords(house_items))
    u.mem_write(0xA83D4C, dwords(HOUSE))
    u.mem_write(0xA8B238, dwords(1))
    u.mem_write(SCENARIO + 0x214, dwords(0))
    # Actual cached keyboard reader53EC90 returns these supplied settled keys;
    # no GetAsyncKeyState return or action result is answered.
    u.mem_write(0xAA0444, dwords(1))
    u.mem_write(0xAA0168, bytes(512))
    u.mem_write(0xA8ECC8, dwords(1))
    selection = f.allocate(4)
    u.mem_write(0xA8ECBC, dwords(selection))
    u.mem_write(0xA802C8, dwords(0, 0, 0))
    clock = f.allocate(16)
    u.mem_write(0x7E1530, dwords(clock))  # timeGetTime import, not a sim timer.
    actors, constructor_primary = [], []
    offsets = dict(primary=(0x3D3, 1), limbo=(0x81, 1), alive=(0x90, 1),
                   online=(0x660, 1), health=(0x6C, 4), mission=(0xAC, 4),
                   queued_mission=(0xB4, 4))

    def fields(pointer, values):
        for key, value in values.items():
            offset, size = offsets[key]
            u.mem_write(pointer + offset, bytes([value]) if size == 1 else dwords(value))

    for index, actor in enumerate(case['actors']):
        pointer = f.allocate(0x800)
        actors.append(pointer)
        f.building(actor['type'], pointer, f.allocate(16), coord=(6 + index * 3, 9))
        # The actual Techno constructor primary-byte store; wider ctor/Unlimbo
        # remains supplied scene admission, as in JoinedFixture.building.
        u.reg_write(UC_X86_REG_ESI, pointer)
        u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(u, 0x6F2F4F, 0x6F2F55)
        constructor_primary.append(u.mem_read(pointer + 0x3D3, 1)[0])
        invoke(u, 0x410230, 0, pointer + 4)
        u.mem_write(pointer + 0x21C, dwords(houses[actor.get('owner', 0)]))
        u.mem_write(pointer + 0x83, b'\0')
        fields(pointer, dict(mission=5, queued_mission=-1) |
                        {k: v for k, v in actor.items() if k in offsets})
        overrides = actor.get('type_overrides', {})
        if overrides:
            typ = f.allocate(0x2000)
            u.mem_write(typ, bytes(u.mem_read(read(pointer + 0x520), 0x2000)))
            for key, value in overrides.items():
                offset, size = dict(naval=(0xCCE, 1), factory=(0xEB8, 4),
                                    invisible_in_game=(0x1701, 1))[key]
                u.mem_write(typ + offset, bytes([value]) if size == 1 else dwords(value))
            u.mem_write(pointer + 0x520, dwords(typ))
    for index, house in enumerate(houses):
        owned = [p for p, a in zip(actors, case['actors']) if a.get('owner', 0) == index]
        items = f.allocate(max(4, 4 * len(owned)))
        u.mem_write(items, dwords(*owned))
        u.mem_write(house + 0x6C, dwords(items))
        u.mem_write(house + 0x78, dwords(len(owned)))
    # The original ID producer assigned the keys. Runtime publication into a
    # pre-sorted ID catalog is an explicit scene prior; native lookup/RTTI run.
    catalog = f.allocate(8 * len(actors))
    for index, pointer in enumerate(actors):
        u.mem_write(catalog + index * 8, dwords(read(pointer + 0x10), pointer))
    u.mem_write(0xB0E840, dwords(catalog, len(actors)))
    u.mem_write(0xB0E84C, b'\1')
    u.mem_write(0xB0E850, dwords(0))
    u.mem_write(0xB78828, dwords(0))
    admission = case.get('planning_admission')
    if admission is not None:
        # Explicit scene prior: other actors already have nonempty paths. The
        # original counter increment helper, not a supplied decision, records
        # that prior. Constructing/playing those other paths is outside this VM.
        u.mem_write(0xAC4B84, bytes(24 * 4))
        for _ in range(admission['prior_house_nonempty_paths']):
            invoke(u, 0x639F60, 0)
        u.mem_write(0xAC4C08, b'\0')
        u.mem_write(0xB0EA80, dwords(-1, 0, 0))
        u.mem_write(HOUSE + 0x16054, dwords(0))  # supplied local message color.
        text = inputs.planner_csf['text'].encode('utf-16-le') + b'\0\0'
        localized_text = f.allocate(len(text))
        u.mem_write(localized_text, text)
    identities = {p: index + 1 for index, p in enumerate(actors)}
    trace, effects, imports = NativeCallTrace(u, read), [], []
    entries = {0x447210: ('building_what_action', 2, 8),
               0x6FFEC0: ('techno_what_action', 2, 8),
               0x443410: ('building_click', 3, 12),
               0x6E6AB0: ('target_pack', 1, 4),
               0x4C65E0: ('target_event_ctor', 4, 16),
               0x4C6CB0: ('event_execute', 0, 0),
               0x6E7A80: ('target_as_building', 0, 0),
               0x6386E0: ('clear_planning_tokens', 0, 0),
               0x639040: ('planning_compatible', 0, 0),
               0x639130: ('planning_capacity', 0, 0),
               0x448160: ('manual_primary', 0, 0),
               0x448070: ('automatic_primary', 1, 4),
               0x43F180: ('building_mark', 1, 4),
               0x50DB00: ('determine_edge', 1, 4),
               0x5F7900: ('find_factory', 4, 16),
               0x70D4A0: ('detach', 1, 4), 0x7258D0: ('pointer_expiry', 2, 8)}
    if admission is not None:
        entries.update({0x734E60: ('localized_string', 2, 8),
                        0x730A90: ('planning_message', 0, 0),
                        0x5D3BA0: ('message_list_add', 7, 28),
                        0x750920: ('planning_scold', 2, 8),
                        0x70D150: ('action_line_timer_start', 0, 0)})

    def hook(uc, pc, _size, _data):
        sp, this = uc.reg_read(UC_X86_REG_ESP), uc.reg_read(UC_X86_REG_ECX)
        trace.returned(pc, sp)
        if pc in entries:
            trace.entered(pc, sp, entries[pc])
        if checked_is_bad_read_ptr_transport(uc, pc, sp, imports):
            return
        if pc == clock:
            imports.append(dict(event='OS_timeGetTime', supplied_result=100000))
            f.ret(100000)
        elif admission is not None and pc == 0x734E60:
            key = bytes(uc.mem_read(this, 128)).split(b'\0', 1)[0].decode('latin1')
            assert key == inputs.planner_csf['key']
            imports.append(dict(event='localized_string', key=key,
                                text=inputs.planner_csf['text']))
            f.ret(localized_text, 8)
        elif admission is not None and pc == 0x5D3BA0:
            args = list(struct.unpack('<7I', uc.mem_read(sp + 4, 28)))
            assert args[2] == localized_text
            effects.append(dict(event='planning_refusal_message',
                                key=inputs.planner_csf['key'],
                                text=inputs.planner_csf['text'],
                                color=args[3], flags=args[4], duration=args[5],
                                force=args[6]))
            f.ret(0, 28)
        elif admission is not None and pc == 0x750920:
            effects.append(dict(event='planning_scold',
                sound_index=struct.unpack('<i', dwords(this))[0],
                pan=uc.reg_read(UC_X86_REG_EDX),
                volume_f32_hex=bytes(uc.mem_read(sp + 4, 4)).hex(),
                handle=read(sp + 8)))
            f.ret(0, 8)
        elif pc == 0x752700:
            key = bytes(uc.mem_read(this, 128)).split(b'\0', 1)[0].decode('latin1')
            effects.append(dict(event='eva', key=key,
                                priority=struct.unpack('<i', dwords(uc.reg_read(UC_X86_REG_EDX)))[0],
                                type=struct.unpack('<i', uc.mem_read(sp + 4, 4))[0]))
            f.ret(0, 4)
        elif pc in (0x43F180, 0x50DB00, 0x70D4A0, 0x7258D0):
            effects.append(dict(event=entries[pc][0], actor=identities.get(this),
                                argument=identities.get(read(sp + 4), read(sp + 4))))

    u.hook_add(UC_HOOK_CODE, hook)

    def call(entry, this, *args):
        result = invoke(u, entry, this, *args)
        trace.returned(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
        return result

    def state():
        selected = call(0x5F7900, inputs.unit_types['MTNK'], 0, 0, 0, HOUSE)
        observed = dict(primary=[u.mem_read(p + 0x3D3, 1)[0] for p in actors],
            find_factory=identities.get(selected),
            actors=[dict(id=i + 1, uid=read(p + 0x10),
                         coords=list(struct.unpack('<3i', u.mem_read(p + 0x9C, 12))),
                         selected=u.mem_read(p + 0x83, 1)[0],
                         flags={k: (u.mem_read(p + off, 1)[0] if size == 1 else
                                    struct.unpack('<i', u.mem_read(p + off, 4))[0])
                                for k, (off, size) in offsets.items() if k != 'primary'},
                         marked=u.mem_read(p + 0x74, 1)[0],
                         mission_timer=list(struct.unpack('<3i', u.mem_read(p + 0xC8, 12))),
                         stage_timer=list(struct.unpack('<5i', u.mem_read(p + 0x100, 20))))
                    for i, p in enumerate(actors)],
            houses=[dict(index=i, recheck=u.mem_read(p + 0x1FC, 1)[0], edge=read(p + 0x577C),
                         timer=list(struct.unpack('<3i', u.mem_read(p + 0x2A4, 12))),
                         factory_slots=[read(p + off) for off in
                                        (0x53AC, 0x53B0, 0x53B4, 0x53B8, 0x53BC, 0x53CC)])
                    for i, p in enumerate(houses)])
        if admission is not None:
            observed['planning'] = dict(
                house_nonempty_paths=[read(0xAC4B84 + i * 4) for i in range(len(houses))],
                actor_path_present=[bool(read(p + 0x514)) for p in actors],
                refusal_latch=u.mem_read(0xAC4C08, 1)[0],
                action_line_timer=list(struct.unpack('<3i', u.mem_read(0xB0EA80, 12))))
        return observed

    issued, phases = [], []
    initial = state()
    before = f.rng(include_mapgen=True)
    for frame, operation in enumerate(case['operations'], 1):
        u.mem_write(FRAME, dwords(frame))
        pointer = actors[operation['actor'] - 1] if operation.get('actor') else 0
        if operation.get('changes'):
            fields(pointer, operation['changes'])
        first_call, first_effect, first_import = len(trace.calls), len(effects), len(imports)
        first_joined_event = len(f.events)
        result = {}
        if operation['op'] == 'automatic':
            result['return_al'] = call(0x448070, pointer, 0) & 255
        elif operation['op'] == 'click':
            u.mem_write(selection, dwords(pointer))
            for actor in actors:
                u.mem_write(actor + 0x83, bytes([actor == pointer]))
            u.mem_write(0xAC4CF4, bytes([bool(operation.get('planning'))]))
            action = call(0x447210, pointer, pointer, 0)
            if operation.get('planning'):
                # Real LeftUp's compatibility/capacity branch for Action4.
                # Its caller locals/selection are supplied; no gate result or
                # planning mission/event is synthesized by the driver.
                u.mem_write(SP, bytes(0x200))
                u.reg_write(UC_X86_REG_ESP, SP)
                stop = run_checked(u, 0x4ABF78, (0x4ABF94, 0x4AC294))
                result['planning_admission_stop'] = f'{stop:08X}'
                if admission is None:
                    assert stop == 0x4ABF94, ('ordinary empty-planning control refused', stop)
                else:
                    result['admitted'] = stop == 0x4ABF94
            prior_count, prior_tail = read(0xA802C8), read(0xA802D0)
            if result.get('admitted', True):
                call(0x443410, pointer, action, pointer, 0)
                if admission is not None:
                    # Actual LeftUp4ABFAE suffix, only after admitted click.
                    # The timer's unused middle word inherits caller scratch.
                    u.mem_write(SP - 0x200, bytes(0x200))
                    call(0x70D150, 0)
            records = []
            for index in range(read(0xA802C8) - prior_count):
                record = 0xA802D4 + ((prior_tail + index) & 127) * 0x6F
                records.append(record)
            result.update(action=action, issued=len(records))
            for record in records:
                issued.append(record)
                call(0x4C6CB0, record)
        elif operation['op'] == 'event':
            record = f.allocate(0x70)
            owner = read(pointer + 0x21C) if pointer else HOUSE
            uid = read(pointer + 0x10) if pointer else 0x12345678
            call(0x4C65E0, record, read(owner + 0x30), 17, uid, 0x34)
            issued.append(record)
            call(0x4C6CB0, record)
        elif operation['op'] == 'repeat_event':
            record = issued[operation['event']]
            call(0x4C6CB0, record)
        else:
            raise ValueError(operation)
        observed = state()
        phases.append(dict(input=operation, frame=frame, **result, state=observed,
            effects=effects[first_effect:], imports=imports[first_import:],
            calls=trace.calls[first_call:], joined_events=f.events[first_joined_event:],
            rng=f.rng(include_mapgen=True)))
    assert not f.pending and not trace.pending
    assert f.code_unchanged()
    return dict(input=case, actor_addresses=actors, constructor_primary=constructor_primary,
                initial=initial, rng_before=before, phases=phases,
                events=[dict(event=read(p) & 255, issuer=u.mem_read(p + 2, 1)[0],
                             frame=read(p + 3), uid=read(p + 7),
                             target_kind=u.mem_read(p + 11, 1)[0]) for p in issued],
                rng_requests=f.draws, rng_advances=f.advances,
                original_executable_sections_unchanged=True)


def manual_primary_generate():
    from tools.rules_oracle.bridge_child_sound import sections
    # Same existing cache owner and physical-layer selection. GAWEAP only
    # needs the exact original scalar blocks declared by primary_field_names.
    physical = sections((JOINED_ROOT / 'RULESMD.INI').read_bytes())
    m = joined_inputs(JOINED_ROOT,
        layers=('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'),
        building_names=JOINED_NAMES + ('GAWEAP',), art_names=JOINED_ART_NAMES + ('GAWEAP',),
        country_names=tuple(physical['Countries'].values()), unit_names=('MTNK',),
        primary_field_names=('GAWEAP',))
    cases = [manual_primary_case(m, case) for case in manual_primary_cases()]
    eva_path = Path(__file__).resolve().parents[2] / 'ini/evamd.ini'
    eva_raw = eva_path.read_bytes()
    cue = 'EVA_PrimaryBuildingSelected'
    m.make_ini({cue: sections(eva_raw)[cue]})
    cue_fields = {}
    from tools.spatial_oracle.building_body_rules import INI
    for key in ('Text', 'Allied', 'Russian', 'Yuri', 'Priority'):
        buffer = m.alloc(256)
        m.invoke(0x528A10, INI, (m.cstring(cue), m.cstring(key), m.cstring(''), buffer, 256))
        cue_fields[key] = m.string(buffer)
    # A separate input run leaves all six original histories/address mappings
    # untouched while registering the physical refusal sound for these controls.
    a = joined_inputs(JOINED_ROOT,
        layers=('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'),
        building_names=JOINED_NAMES + ('GAWEAP',), art_names=JOINED_ART_NAMES + ('GAWEAP',),
        country_names=tuple(physical['Countries'].values()), unit_names=('MTNK',),
        primary_field_names=('GAWEAP',), planning_admission=True)
    from tools.storage_oracle.keyboard_bindings import stock_csf
    csf, csf_hash = stock_csf()
    a.planner_csf = dict(file='langmd.mix/ra2md.csf', sha256=csf_hash,
                        key='MSG:PlannerMaximum', text=csf['MSG:PLANNERMAXIMUM'])
    admission_controls = [manual_primary_case(a, case)
                          for case in manual_primary_admission_cases()]
    scold_index = a.invoke(0x7514D0, a.cstring('MenuScold'))
    return dict(schema_version=1, source='unicorn/gamemd.exe',
                seed=31, layers=m.layers, country_catalog=m.country_catalog,
                retail_files=[dict(name=name, size=(JOINED_ROOT / name).stat().st_size,
                                   sha256=hashlib.sha256((JOINED_ROOT / name).read_bytes()).hexdigest())
                              for name in ('RULESMD.INI', 'ARTMD.INI', 'SOUNDMD.INI',
                                           'MPBattleMD.ini', 'Hills.map')],
                eva=dict(file='EVAMD.INI', sha256=hashlib.sha256(eva_raw).hexdigest(),
                         key=cue, native_strings=cue_fields),
                constructor_type_inputs=m.primary_constructor_inputs,
                native_type_inputs={name: manual_primary_type_state(m.u, p)
                                    for name, p in m.types.items()},
                query_unit=dict(name='MTNK', ownable=m.read32(m.unit_types['MTNK'] + 0x6CC),
                                naval=m.u.mem_read(m.unit_types['MTNK'] + 0xCCE, 1)[0],
                                rtti=m.invoke(m.read32(m.read32(m.unit_types['MTNK']) + 0x2C),
                                              m.unit_types['MTNK'])),
                cases=cases,
                planning_admission=dict(csf=a.planner_csf,
                    scold_sound=dict(key='ScoldSound', section='AudioVisual',
                        constructor_default=a.scold_constructor_default, layers=a.scold_layers,
                        name='MenuScold', registry_index=scold_index,
                        selected_registry=list(a.selected['SoundList'].values())),
                    controls=admission_controls))


def manual_primary_metadata():
    return provenance(scope='Ordinary selected stock GAWEAP self-action/click -> original TargetEvent17/receiver/manual primary448160 -> original MTNK FindFactory, plus explicit receiver/group/limbo/edge controls',
        entry_points=dict(building_what_action=0x447210, techno_what_action=0x6FFEC0,
                          building_click=0x443410, target_pack=0x6E6AB0,
                          target_event_ctor=0x4C65E0, event_execute=0x4C6CB0,
                          event17_arm=0x4C718C, target_as_building=0x6E7A80,
                          clear_planning_tokens=0x6386E0, manual_primary=0x448160,
                          automatic_primary=0x448070, find_factory=0x5F7900,
                          building_mark=0x43F180, determine_edge=0x50DB00,
                          primary_ctor_store=0x6F2F4F, create_id=0x410230,
                          next_id=0x68BCB0, local_house=0x50B6F0,
                          eva_request=0x752700, planning_compatible=0x639040,
                          planning_capacity=0x639130, increment_plan_count=0x639F60,
                          scold_sound_read=0x66ABCD, string_lookup=0x734E60,
                          planning_message=0x730A90, message_list_add=0x5D3BA0,
                          centered_scold_sound=0x750920, action_line_timer=0x70D150),
        assumptions=[
            'Extends the existing JoinedFixture/Reader/Lists/Sound owners, not a new VM or parser. Full original BuildingType constructors execute. GACNST/GAPOWR/GAPILE keep the existing complete readers. GAWEAP executes exact Image5F92F9..5F9340, Strength5F94D3..5F94F3, BuildCat45FE78..45FE97, Factory46051A..46054B, GapGenerator713F92..713FAC, ConstructionYard460A24..460A44, Owner7149E1..7149FB, Naval714A63..714A7D, Passengers714B35..714B56, absorb460988..4609D6 and InvisibleInGame460DEB..460E07 blocks, then fixed ART Foundation461225..46125D/table binding46152C..461547 after original45B1C0 table initialization. UnitAbsorb is+16AE/key81AABC; InfantryAbsorb is+16AF/key81AAAC. The demand-loaded buildup/image/animation lifecycle is excluded.',
            'Source-order lexical strings from physical RULESMD.INI, absent LANGRULE.INI, MPBattleMD.ini and Hills.map feed original native scalar/enum readers with their retained constructor defaults. Country vector capacity/physical [Countries] name order are supplied; full5113F0 enrolls/indexes each country before original Owner lookup. Country inherited metadata and full Rules/Process are outside this selected catalog.',
            'MTNK uses its full UnitType constructor7470D0 and original Owner/Naval read blocks. FindFactory(0,0,0,House) executes unchanged with its actual RTTI; CanBuild/online-required/aircraft contact/paid production/delivery are excluded.',
            'Building/House/map admission and ordered House building vectors are supplied. Existing Mission/stage constructor stores and sound-handle constructors execute; primary byte store6F2F4F and UID producer410230/68BCB0 execute. Runtime ID catalog publication is supplied sorted storage; original token binary search/RTTI and empty-planning cleanup execute. Cached keyboard is supplied settled with one selected actor. No normal input result is answered.',
            'The ordinary history executes whole447210,443410, native OutList copy,4C65E0 and4C6CB0. The final empty-planning click also runs actual LeftUp4ABF78..4ABF94 compatibility/capacity gates639040/639130 on one selected actor with null+514 and zero House plan count. Earlier Display geometry/picking/selection and full LeftUp/4AE750 scheduling remain supplied. Event transport/scheduling supplies actual issued record pointers immediately to the original receiver. Direct event controls execute original constructor/receiver without claiming their supplied stale/foreign/limbo/zero-health priors arise from normal clicking.',
            'Separate planning_admission controls retain the six original histories and supply0/127/128 other nonempty paths as House0 counter priors through original639F60 increments. They select one no-path GAWEAP, execute actual639040/639130 and LeftUp4ABF78 branch, and only on actual acceptance continue the original click/event and70D150 timer suffix. They do not construct128 other paths or execute whole Display/PlanMgr ticks. AC4B84 counts nonempty paths: append638470 increments empty-to-nonempty; consume636718..636760 releases the count at zero remaining commands, before an empty executing path is acknowledged. Dispatch6386E0 and ownership-change701585->638840 clear nonempty paths before owner replacement; capture does not transfer an existing count. Delayed old-issuer appends after capture, arbitrary compatibility and counter corruption are outside these three controls.',
            'Admission sound inputs use the same Sound/Reader owners, with physical Dummy/MenuScold entries and supplied immutable sample identityumenscol=0. Rules ctor666027 suppliesScoldSound=-1; exact66ABCD..66AC18 ReadString128/7514D0 blocks run in physical RULESMD, optional LANGRULE, MPBattleMD and Hills.map order. Unlayered langmd.mix/ra2md.csf bytes are read by existing keyboard_bindings.stock_csf, without another CSF/INI parser. The native refusal itself selects MSG:PlannerMaximum, sets AC4C08, and calls original730A90 duration480 then750920 centered pan8192/volume1/null handle. No decision result is supplied.',
            'Each actual per-Building primary byte is observed independently from live FindFactory. A supplied alive+Limbo+primary prior is a representability/control bound, not established stock GAWEAP producer reachability. Factory0, liveness, exact raw Factory/Naval grouping, foreign/local EVA and stale duplicate controls retain original body gates only.',
            'Scenario/Main/MapGen seed31 use original65C6D0 and retain full1012-byte buffers continuously across each case, with original shared request/advance tracing. Building/House mission, stage and House timer bytes are observed. Native constructor/admission before supplied scene priors, whole frame scheduling and downstream audio cue/device RNG are outside this boundary.',
            'Original BuildingMark(2) entry/switch body and DetermineEdge execute. Mark2 uses existing705D70 presentation-getter transport, reached8 times in the original six histories and once in each admitted capacity control; its palette/tint/attached-Anim outputs are not claimed unsubstituted. The ordinary untethered scene takes redraw/cache propagation, not occupancy/PlaceUp/PlaceDown. Native executable PE sections compare byte-for-byte before/after. No gameplay, Mark entry, timer, random, factory or peer-loop return or instruction is replaced.'],
        substitutions=[
            'Inherited native-input lexical caches, immutable file/image IO, successful shared bump allocation/delete and CRT TLS transports remain. Scene/registry/selection priors are explicit inputs.',
            'Existing initialize_empty_windows_seh and checked_is_bad_read_ptr_transport supply Windows exception/readability boundaries for original RTTI. timeGetTime IAT7E1530 supplies100000 milliseconds for original OutList transport; no simulation timer result is supplied.',
            'Only the separate planning_admission controls supply physical localized text at734E60 (RET8), sink the final MessageList5D3BA0 insertion (RET28) after original730A90, and record centered Voc750920 (RET8) before downstream playback/device/RNG. HUD queue layout, pixels and audible playback are not certified. Action-line70D150 executes with zero caller scratch below ESP; its raw middle timer word is unused storage, not a gameplay comparison claim. Existing cases do not install these reached transports.',
            'Vox752700 records actual literal EVA_PrimaryBuildingSelected, priority/type and returns0 with RET4; audible playback, queue/device state and downstream audio RNG are not certified. Existing JoinedFixture705D70 palette getter returns0 when Mark2 reaches it (8 recorded presentation_boundary entries in the original histories, plus2 in the admission controls). Other inherited UpdateAnimation presentation sinks remain installed but unreached. Common primary toggles change no palette/tint/cloak input; wider cloak/flash/attached-Anim refresh and pixels need their existing presentation owners.'])


def joined_generate():
    import json
    m = joined_inputs(JOINED_ROOT)
    receipts_path = JOINED_ROOT / 'receipts.json'
    receipts = json.loads(receipts_path.read_text()) if receipts_path.is_file() else []
    receipts = [{k: v for k, v in row.items() if k in ('asset', 'source_archive', 'entry_id',
                                                     'size', 'sha256', 'catalog_lookup_only')}
                for row in receipts]
    files = [dict(name=p.name, size=p.stat().st_size, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
             for p in sorted(JOINED_ROOT.iterdir()) if p.is_file() and p.suffix.lower() != '.json']
    routes = [joined_route(m, 'GACNST', 'deploy'),
              joined_route(m, 'GAPOWR', 'player'),
              joined_route(m, 'GAPILE', 'player'),
              joined_route(m, 'GAPOWR', 'computer'),
              joined_route(m, 'GAPOWR', 'player', 250),
              joined_route(m, 'GAPOWR', 'computer', guard_control=True)]
    return dict(schema_version=1, source='unicorn/gamemd.exe',
                retail_files=files, extraction_receipts=receipts, layers=m.layers,
                native_type_inputs=m.type_rows, native_anim_inputs=m.anim_rows,
                selected_animation_list=m.animation_list,
                native_asset_requests=[{k: v for k, v in row.items() if k != 'header8_hex'}
                                       for row in m.asset_loaded],
                mission_control={str(i): bytes(m.u.mem_read(0xA8E3A8 + i * 32, 32)).hex()
                                 for i in (5, 18)},
                rules=dict(buildup_time_f64_hex=bytes(m.u.mem_read(m.rules + 0x1518, 8)).hex(),
                           condition_yellow_f64_hex=bytes(m.u.mem_read(m.rules + 0x1700, 8)).hex(),
                           construction_sound=struct.unpack('<i', m.u.mem_read(m.rules + 0x6C8, 4))[0]),
                routes=routes, factory_controls=joined_factory_controls(m),
                stock_dummy_sound=joined_dummy_sound(m))


def joined_metadata():
    return provenance(scope='Joined ordinary GACNST deploy / GAPOWR and GAPILE placement construction through whole Grand_Opening and Anim lifecycle, completion C and operational C+1; native retail input readers and human PLACE factory identity controls',
        entry_points={'rules_ctor': 0x665650, 'buildup_time_read': 0x670C99,
                      'condition_yellow_read': 0x66B35E, 'construction_sound_read': 0x66A96A,
                      'building_type_ctor': 0x45DD90, 'building_type_read': 0x45FE50,
                      'building_facing_ctor': 0x45DEE6, 'deploy_facing_ctor': 0x45DEEC,
                      'deploy_facing_read': 0x460C6C, 'grand_helipad_read': 0x4463C0,
                      'construction_facing_read': 0x449AFE,
                      'mission_control_ctor': 0x4E7CF0, 'mission_control_read_table': 0x679C92,
                      'mission_control_read': 0x5B3760,
                      'mission_ctor_state': 0x5B2DA8, 'stage_ctor_state': 0x6F2B4B,
                      'anim_type_ctor': 0x427530, 'anim_type_read': 0x427D00,
                      'enter_construction': ENTER_CONSTRUCTION, 'first_contact': 0x65ACB0,
                      'transmit': 0x65A970, 'grand_opening': GRAND_OPENING,
                      'slot_allocate': 0x451890, 'anim_ctor': 0x421EA0,
                      'anim_ctor_type_store': 0x421EE5, 'anim_ai_type_read': 0x423B05,
                      'abstract_type_id_ctor': 0x410800,
                      'anim_start': 0x424CE0, 'anim_ai': 0x423AC0,
                      'operational_header': 0x43FB20, 'operational_edge': 0x4549B0,
                      'find_factory': 0x5F7900, 'set_primary': 0x448070,
                      'random_ranged': 0x65C7E0, 'inline_advance': 0x65C84B,
                      'anim_scalar_delete': 0x426590, 'anim_dtor': 0x4228E0,
                      'sound_registry': 0x7510D0, 'sound_play': PLAY_AT,
                      'sound_handle_ctor': 0x405BE0, 'building_destructor_sound_slice': 0x43BD1F,
                      'sound_update': 0x4055C0, 'sound_watch': LOOP_UPDATE,
                      'sound_release': SOUND_RELEASE},
        assumptions=[
            'Selected exact-case lexical INI strings are supplied in original signed-CRC/source-order caches. Full Rules/BuildingType/AnimType and MissionControl constructors and complete BuildingType/AnimType readers execute; original Rules BuildupTime ([General]), ConditionYellow and Construction ([AudioVisual]) key blocks and MissionControl read-table loop execute in RULESMD, optional LANGRULE, Battle mode, Hills map order. Physical INI loading, other rule keys and country startup are excluded.',
            'Selected AnimTypes use original FindOrAllocate428B80 in the real [Animations] list source order; unselected entries/whole Rules::Process are excluded, so AnimType and Dummy sound indices/pointers are fixture-relative identities.',
            'Constructor events record the real AnimType argument pointer and its canonical ID field+24; live snapshots read retained type pointer+0xC8 (421EE5 store,423B05 AI consumer). Attached Object pointer+0xCC is separately labeled owner_object, never interpreted as a type.',
            'BuildingType facing dword+ED8 is immutable constructor state0 (45DD9A XOR EBX,45DEE6 store); no INI key is established. DeployFacing is the separate dword+EDC, constructor128 and reader460C6C..460C86. Helipad is byte+16CB (reader4604CC..4604E0, Grand4463C0). Creation and frame snapshots retain raw little-endian current/destination body-facing u16 values at Building+388/+38C.',
            'Actual extracted SHP bytes answer original requested filenames. Theater=Temperate is prior Scenario state. MK extraction receipts explicitly remain catalog-only; production theater mount/precedence and rendered body/cameo pixels need independent validation. Missing body/cameo file requests return null and are recorded.',
            'Already admitted healthy/damaged Building, House flags/map, DrawingLayer/Logic membership and radio vectors are explicit prior state. Mission/stage constructor stores5B2DA8..5B2DE9/6F2B4B..6F2B87 and sound handle constructor405BE0 execute at frame0; wider Building constructor/Unlimbo/reveal/power/country/HousePlace factory queue are not executed. Human HELLO, C and FirstContact BREAK are real radio calls in original PLACE4FB1F1/4FB2AD/4FB4A6 order. Computer Commence and deploy queued Construction/+6DD are separate entry cases.',
            'Each frame runs original Building header43FB20..43FC39, UpdateAnimation, ready/mission/queued-BState pieces, then every appended live Anim AI in vector order. Full Building/Techno AI and full Logic scheduler are excluded; native Logic55B613 count-re-read is the bounded scheduling premise.',
            'Whole Grand_Opening/slot allocation/Anim ctor/Unlimbo/Start/first-AI guard/destructor execute unchanged. Stock ordinary keys keep FreeUnit, cash, healing, purifier, helipad and special branches dormant; this corpus does not certify those mechanisms.',
            'Dummy is bound by original SoundList/Defaults/reader with fixture-relative index0. Enabled audio context, one pooled event, unshrouded map and viewport are supplied. Original event play/no-sample update/watch/release execute, without device/sample mixing or global pool cleanup. Sound pool scheduling is presentation-only in this bounded comparison.',
            'Scenario seed31/map state come from existing original source-scatter fixture; Main constructor65C6D0 receives seed31. Whole before/after RNG buffers, each request state/result, each inline underlying raw/masked/rejected advance are retained. Constructor draws before supplied admission are excluded. Stock HasStupidGuardMode=true avoids the Guard random branch; an explicitly asymmetric control clears that type byte and executes the ranged rejection branch, without claiming it is stock input.',
            'Every executable PE section is compared with original file bytes before the fixture and with those same bytes after execution; no original instructions are patched. PC53/chop FPCW0E7F inherited from the native source-scatter fixture is asserted.'],
        substitutions=[
            'Existing Reader/Lists bounded cache, original requested-file byte IO, bump allocator/delete and CRT TLS boundaries are reused for input readers.',
            'Runtime operator_new7C8E17 returns recorded arena storage and operator_delete7C8B3D records release. UpdateAnimation presentation451F60/452170/456FB0/705D70 return zero; their argument cleanup is exact. No runtime radio, Grand_Opening, animation, sound or RNG body is answered.'])


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if '--joined' in argv or '--manual-primary' in argv:
        if '--joined' in argv and '--manual-primary' in argv:
            raise ValueError('--joined and --manual-primary select separate corpora')
        manual = '--manual-primary' in argv
        argv.remove('--manual-primary' if manual else '--joined')
        root = Path(__file__).resolve().parents[2]
        sources = ('tools/spatial_oracle/building_construction.py',
                   'tools/native_oracle.py', 'tools/rules_oracle/bridge_anim_inputs.py',
                   'tools/rules_oracle/bridge_child_sound.py', 'tools/rules_oracle/bridge_anim_lists.py',
                   'tools/projectile_oracle/flat_art.py', 'tools/spatial_oracle/refinery_dock.py',
                   'tools/spatial_oracle/track_destination.py', 'tools/spatial_oracle/unit_entry.py',
                   'tools/spatial_oracle/unit_scatter_state.py', 'tools/spatial_oracle/unit_source_scatter.py',
                   'tools/spatial_oracle/building_body_rules.py', 'tools/spatial_oracle/anim_bouncer_launch.py',
                   'tools/spatial_oracle/building_slot_replacement.py')
        if manual:
            sources += ('tools/storage_oracle/keyboard_bindings.py', 'tools/sidebar_oracle/stock.py')
        finish_vectors(manual_primary_generate if manual else joined_generate,
                       Path(__file__).with_name('building_manual_primary.json' if manual else
                                                'building_construction_joined.json'),
                       provenance=manual_primary_metadata if manual else joined_metadata, argv=argv,
                       source_paths={name: root / name for name in sources})
        return
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
