"""Original Unit simple-deployer lifecycle and independently read retail ART.

Set VERA20K_SIMPLE_DEPLOY_ASSETS to an asset extract directory containing
artmd.ini and schpdepl.shp. Run --write to publish, --check to reproduce.
Uses the shared refinery Unit/map fixture, AnimType ART reader, and Jumpjet
state fixture. No complete Windows game, renderer or AnimClass AI claim.
"""
import hashlib
import os
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import NATIVE_SHA256, finish_vectors, provenance, run_checked
from tools.rules_oracle.bridge_anim_inputs import Reader, read_types
from tools.rules_oracle.bridge_child_sound import sections
from tools.spatial_oracle.building_body_rules import INI, TYPE as READER_TYPE, SP as READER_SP
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.refinery_dock import make_dock_fixture, cell, cell_xy
from tools.spatial_oracle.unit_entry import EXTRA
from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE, SP
from tools.spatial_oracle.unit_source_scatter import SCENARIO

ASSETS = Path(os.environ.get('VERA20K_SIMPLE_DEPLOY_ASSETS', 'target/asset/simple-deploy/extract'))
ANIM, ANIM_TYPE = EXTRA + 0x24000, EXTRA + 0x25000
DEPLOY, UNDEPLOY, UNLOAD = 0x739AC0, 0x739CD0, 0x73D630


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def asset(name):
    found = [p for p in ASSETS.iterdir() if p.name.lower() == name.lower()]
    assert len(found) == 1, (name, found)
    return found[0]


def retail_art():
    raw = asset('artmd.ini').read_bytes()
    keys = sections(raw)['SCHPDEPL']
    row = read_types(ASSETS, {'SCHPDEPL': keys}, ['SCHPDEPL'])[0]
    hva = []
    for name in ['schp.hva', 'schd.hva']:
        data = asset(name).read_bytes()
        frames, parts = struct.unpack_from('<2I', data, 16)
        hva.append(dict(name=name, sha256=hashlib.sha256(data).hexdigest(),
                        bytes=len(data), file_header24_hex=data[:24].hex(),
                        frames=frames, sections=parts))
    return dict(art_sha256=hashlib.sha256(raw).hexdigest(), art_bytes=len(raw), result=row, physical_hva=hva)


class Unit:
    def __init__(self, case, art):
        self.u, self.call, self.read32 = make_dock_fixture(dict(
            harvester=False, linked=False, mission='unload', west_building=False))
        self.events = []
        u = self.u
        self.case = case
        u.mem_write(TYPE + 0xE13, bytes([case.get('simple', True)]))
        u.mem_write(TYPE + 0x6AD, bytes([case.get('deploy_to_land', True)]))
        u.mem_write(TYPE + 0x6BC, dwords(ANIM_TYPE if case.get('has_anim_type', True) else 0))
        u.mem_write(TYPE + 0x56C, dwords(-1, -1))
        u.mem_write(ACTOR + 0x130, dwords(ANIM if case.get('has_anim', True) else 0))
        u.mem_write(ACTOR + 0x134, bytes([case.get('landing', False)]))
        u.mem_write(ACTOR + 0x6E0, bytes(case.get('flags', [0, 0, 0])))
        u.mem_write(ACTOR + 0xA4, dwords(case.get('height', 0)))
        u.mem_write(ACTOR + 0xF8, dwords(case.get('stage', 37)))
        u.mem_write(ACTOR + 0xFC, bytes([case.get('changed', False)]))
        u.mem_write(ACTOR + 0x110, dwords(case.get('increment', 1)))
        u.mem_write(ANIM + 0xC8, dwords(ANIM_TYPE))
        u.mem_write(ANIM_TYPE + 0x2B0, dwords(
            case.get('rate', art['rate']), case.get('start', art['start']),
            0, 0, case.get('count', art['end'])))
        u.hook_add(UC_HOOK_CODE, self.observe)

    def ret(self, cleanup=0, value=0):
        sp = self.u.reg_read(UC_X86_REG_ESP)
        self.u.reg_write(UC_X86_REG_EAX, value)
        self.u.reg_write(UC_X86_REG_EIP, self.read32(sp))
        self.u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x7C8E17:
            self.events.append(['allocate', self.read32(sp + 4)])
            self.ret(value=ANIM)
        elif address == 0x421EA0:
            args = [self.read32(sp + 4 + 4 * i) for i in range(7)]
            self.events.append(['anim_constructor', args[0] == ANIM_TYPE,
                                list(struct.unpack('<3i', u.mem_read(args[1], 12))), *args[2:]])
            self.ret(28, ANIM)
        elif address == 0x424B50:
            self.events.append(['set_anim_owner', self.read32(sp + 4) == ACTOR])
            self.ret(4)
        elif address == 0x705D70:
            self.events.append(['get_remap_colour'])
            self.ret(value=0x12345678)
        elif address == 0x703590:
            self.events.append(['nearby_location', u.reg_read(UC_X86_REG_ECX) == ACTOR,
                                self.read32(sp + 8) == ACTOR])
            output = self.read32(sp + 4)
            u.mem_write(output, packed(11, 10))
            self.ret(8, output)
        elif address == 0x741970:
            self.events.append(['set_destination', cell_xy(self.read32(sp + 4)), self.read32(sp + 8)])
            self.ret(8)
        elif address in (0x65C7E0, 0x65C780):
            self.events.append(['scenario_random', hex(address)])
        elif address == 0x5B35E0:
            self.events.append(['queue_mission', self.read32(sp + 4), self.read32(sp + 8)])
        elif address == 0x5B3570:
            self.events.append(['commence'])

    def state(self):
        u = self.u
        return dict(flags=list(u.mem_read(ACTOR + 0x6E0, 3)), landing=u.mem_read(ACTOR + 0x134, 1)[0],
                    anim_present=bool(self.read32(ACTOR + 0x130)),
                    stage=signed(u, ACTOR + 0xF8), changed=u.mem_read(ACTOR + 0xFC, 1)[0],
                    timer_start=signed(u, ACTOR + 0x100), duration=signed(u, ACTOR + 0x108),
                    rate=signed(u, ACTOR + 0x10C), increment=signed(u, ACTOR + 0x110),
                    mission=signed(u, ACTOR + 0xAC), queued=signed(u, ACTOR + 0xB4),
                    anim_palette=self.read32(ANIM + 0xD4),
                    rng_indices=[self.read32(SCENARIO + 0x21C), self.read32(SCENARIO + 0x220)])

    def invoke(self, entry):
        before = self.state()
        self.events = []
        self.call(entry, ACTOR, [])
        return dict(before=before, after=self.state(), events=self.events.copy(),
                    return_value=self.u.reg_read(UC_X86_REG_EAX) if entry == UNLOAD else None)

    def stage_tick(self, frame):
        self.u.mem_write(0xA8ED84, dwords(frame))
        self.u.reg_write(UC_X86_REG_ESI, ACTOR)
        self.u.reg_write(UC_X86_REG_EBP, 0)
        self.u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(self.u, 0x6FABC4, 0x6FAC31, count=200)


def lifecycle(art):
    cases = [
        dict(name='start_existing_anim'), dict(name='start_allocated_anim', has_anim=False),
        dict(name='no_animation', has_anim_type=False, has_anim=False),
        dict(name='not_simple', simple=False), dict(name='air_requires_landing', height=500),
        dict(name='air_without_deploy_to_land', height=500, deploy_to_land=False),
        dict(name='landing_pending', landing=True), dict(name='already_deployed', flags=[1, 0, 0]),
        dict(name='before_deploy_end', flags=[0, 1, 0], stage=9),
        dict(name='at_deploy_end', flags=[0, 1, 0], stage=10),
        dict(name='after_deploy_end', flags=[0, 1, 0], stage=11),
        dict(name='missing_active_anim_restarts', flags=[0, 1, 0], has_anim=False),
        dict(name='restart_keeps_increment_changed', increment=-1, changed=True),
        dict(name='undeploy_start', flags=[1, 0, 0], entry=UNDEPLOY),
        dict(name='undeploy_allocated_anim', flags=[1, 0, 0], has_anim=False, entry=UNDEPLOY),
        dict(name='undeploy_no_anim', flags=[1, 0, 0], has_anim_type=False, has_anim=False, entry=UNDEPLOY),
        dict(name='undeploy_before_end', flags=[1, 0, 1], stage=8, entry=UNDEPLOY),
        dict(name='undeploy_at_end', flags=[1, 0, 1], stage=9, entry=UNDEPLOY),
        dict(name='undeploy_without_landing', flags=[1, 0, 1], stage=9, deploy_to_land=False, entry=UNDEPLOY),
        dict(name='undeploy_inactive', flags=[0, 0, 0], entry=UNDEPLOY),
    ]
    for start, count in [(7, 3), (-3, 1), (0, 0), (2147483647, 2), (-2147483648, -1)]:
        for stage in [-2147483648, -4, -1, 0, 7, 8, 9, 2147483647]:
            for entry, flags in [(DEPLOY, [0, 1, 0]), (UNDEPLOY, [1, 0, 1])]:
                cases.append(dict(name='signed_threshold', start=start, count=count, stage=stage,
                                  entry=entry, flags=flags, deploy_to_land=False))
    return [dict(input=case, **Unit(case, art).invoke(case.get('entry', DEPLOY))) for case in cases]


def mission_timelines(art):
    rows = []
    for name, flags in [('deploy', [0, 0, 0]), ('undeploy', [1, 0, 0])]:
        m = Unit(dict(flags=flags, has_anim=False), art)
        first = m.invoke(UNLOAD)
        m.stage_tick(200)
        frames = [dict(frame=200, **first, after_stage_tick=m.state())]
        for frame in range(201, 400):
            m.u.mem_write(0xA8ED84, dwords(frame))
            result = m.invoke(UNLOAD)
            m.stage_tick(frame)
            frames.append(dict(frame=frame, **result, after_stage_tick=m.state()))
            if result['return_value'] != 1:
                break
        rows.append(dict(name=name, frames=frames))
    for case in [dict(name='mission_waits_for_landing', height=500),
                 dict(name='mission_no_anim', has_anim_type=False, has_anim=False),
                 dict(name='mission_reverse_no_anim', flags=[1, 0, 0], has_anim_type=False, has_anim=False)]:
        rows.append(dict(name=case['name'], input=case, result=Unit(case, art).invoke(UNLOAD)))
    return rows


def ownership_and_admission(art):
    m = Unit(dict(flags=[1, 1, 1], landing=True), art)
    u = m.u
    u.reg_write(UC_X86_REG_ESI, ACTOR)
    u.reg_write(UC_X86_REG_EBX, 0)
    run_checked(u, 0x735422, 0x735434)
    run_checked(u, 0x6F2BB8, 0x6F2BC4)
    init = m.state()
    detach = []
    for matches in [False, True]:
        m = Unit(dict(flags=[0, 1, 0]), art)
        m.call(0x710410, ACTOR, [ANIM if matches else ANIM + 0x100])
        detach.append(dict(matches=matches, after=m.state()))
    admission = []
    for flags in [[0, 0, 0], [1, 0, 0], [0, 1, 0], [1, 0, 1]]:
        for tube in [-1, 0]:
            m = Unit(dict(flags=flags), art)
            m.u.mem_write(ACTOR + 0x6D8, dwords(-1))
            m.u.mem_write(ACTOR + 0x684, bytes([tube & 255]))
            m.call(0x700D50, ACTOR, [])
            admission.append(dict(flags=flags, tube_index=tube,
                                  admitted=bool(m.u.reg_read(UC_X86_REG_EAX) & 255)))
    return dict(constructor_fields=init, animation_pointer_expired=detach, simple_deployer_admission=admission)


def draw_frames(art):
    m = Unit({}, art['result'])
    u = m.u
    alternate, disguise, hva = EXTRA + 0x26000, EXTRA + 0x28000, EXTRA + 0x2A000
    view_fn = m.read32(m.read32(ACTOR) + 0x440)
    disguise_fn = m.read32(m.read32(ACTOR) + 0xCC)
    supplied = {}

    def hook(_u, address, size, data):
        if address == view_fn:
            m.ret(4, int(supplied['visible']))
        elif address == disguise_fn:
            m.ret(4, disguise)

    u.hook_add(UC_HOOK_CODE, hook)
    frames_by_name = {r['name']: r['frames'] for r in art['physical_hva']}
    for i, (p, count) in enumerate([(TYPE, frames_by_name['schp.hva']),
                                   (alternate, frames_by_name['schd.hva']), (disguise, 3)]):
        u.mem_write(p + 0xB4, dwords(hva + 0x100 * i))
        u.mem_write(hva + 0x100 * i + 8, dwords(count))
        u.mem_write(p + 0xBC, dwords(hva + 0x400))
    u.mem_write(hva + 0x408, dwords(4))
    rows = []
    for deployed in [False, True]:
        for unloading_class in [False, True]:
            for visible in [False, True]:
                supplied['visible'] = visible
                u.mem_write(ACTOR + 0x6E0, bytes([deployed]))
                u.mem_write(TYPE + 0x6B8, dwords(alternate if unloading_class else 0))
                for body, turret in [(0, 0), (0, 5), (1, 5), (2, 5), (3, 5), (-1, -5), (2147483647, 2147483647)]:
                    u.mem_write(ACTOR + 0x538, dwords(body))
                    u.mem_write(ACTOR + 0x148, dwords(turret))
                    u.reg_write(UC_X86_REG_ESP, SP)
                    u.reg_write(UC_X86_REG_EBP, ACTOR)
                    u.reg_write(UC_X86_REG_EDX, m.read32(ACTOR))
                    run_checked(u, 0x73B494, 0x73B50E)
                    sp = u.reg_read(UC_X86_REG_ESP)
                    selected = {TYPE: 'actual', alternate: 'unloading', disguise: 'disguise'}[u.reg_read(UC_X86_REG_EBX)]
                    rows.append(dict(deployed=deployed, unloading_class=unloading_class, visible=visible,
                                     body_counter=body, turret_counter=turret, selected=selected,
                                     body_frame=signed(u, sp + 0x4C), turret_frame=signed(u, sp + 0x38)))
    return rows


def readers():
    m = Reader(ASSETS, {})
    u = m.u
    u.reg_write(UC_X86_REG_ESP, READER_SP)
    u.reg_write(UC_X86_REG_ECX, READER_TYPE)
    run_checked(u, 0x665650, 0x6656C0)
    ctor = signed(u, READER_TYPE + 0x48)
    direction_rows = []
    for raw in [None, '', 'junk', '2', '-1', '8', '9', '255', '256', '2147483647', '2147483648', '$2', '2tail']:
        m.make_ini({'AudioVisual': {} if raw is None else {'DeployDir': raw}})
        u.mem_write(READER_TYPE + 0x48, dwords(64))
        u.mem_write(0x7F0C7C, dwords(m.cstring('AudioVisual')))
        for reg, value in [(UC_X86_REG_ESP, READER_SP), (UC_X86_REG_ESI, READER_TYPE), (UC_X86_REG_EDI, INI)]:
            u.reg_write(reg, value)
        # Stop immediately after +48 store; the following key's pushes are
        # not consumed, so each row restores ESP before the next read.
        run_checked(u, 0x669272, 0x66929B)
        direction_rows.append(dict(raw=raw, initial_raw=64, native_raw=signed(u, READER_TYPE + 0x48)))
    anim_rows = []
    u.mem_write(READER_TYPE + 0x6BC, dwords(0))
    for raw in ['SCHPDEPL', None, '', '  ', 'none', '<none>', 'SCHPDEPL']:
        m.make_ini({'SCHP': {} if raw is None else {'DeployingAnim': raw}})
        name = m.cstring('SCHP')
        for reg, value in [(UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, READER_TYPE),
                           (UC_X86_REG_ESI, INI), (UC_X86_REG_EBX, name), (UC_X86_REG_EAX, 0)]:
            u.reg_write(reg, value)
        run_checked(u, 0x714706, 0x71474C, count=2000000)
        p = m.read32(READER_TYPE + 0x6BC)
        anim_rows.append(dict(raw=raw, native_name=m.string(p + 0x24) if p else None))
    # TechnoType body rates and the InitialAmmo/Ammo block at71474C are RULES
    # reads. Exercise the body rates' original constructor
    # stores and scalar read blocks rather than infer defaults from SCHP.
    u.reg_write(UC_X86_REG_ESI, READER_TYPE)
    u.reg_write(UC_X86_REG_EBX, 0)
    run_checked(u, 0x710B02, 0x710B14)
    body_ctor = [signed(u, READER_TYPE + 0x294), signed(u, READER_TYPE + 0x298)]
    body_rows = []
    for keys in [{}, {'WalkRate': '4', 'IdleRate': '8'}, {},
                 {'WalkRate': '-3', 'IdleRate': '-7'}, {'WalkRate': '0', 'IdleRate': '0'}]:
        m.make_ini({'SCHP': keys})
        for reg, value in [(UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, READER_TYPE),
                           (UC_X86_REG_ESI, INI), (UC_X86_REG_EBX, m.cstring('SCHP'))]:
            u.reg_write(reg, value)
        run_checked(u, 0x712222, 0x712256)
        body_rows.append(dict(keys=keys, native=[signed(u, READER_TYPE + 0x294), signed(u, READER_TYPE + 0x298)]))
    return dict(deploy_dir_constructor_raw=ctor, deploy_dir=direction_rows,
                deploying_anim_history=anim_rows, body_rates_constructor=body_ctor,
                body_rate_history=body_rows)


def run_foot_counter_slice(u, actor, stack, *, through_move_sound=False):
    """Execute the shared original Foot counter region, optionally its sound tail.

    The legacy cadence boundary starts after an admitted Process. The joined
    boundary starts at the original locomotor read/saved counter and continues
    through MoveSound. Its caller owns any declared Process/audio transports;
    this helper never supplies a counter delta or an IsMovingNow result.
    """
    for reg, value in ((UC_X86_REG_ESP, stack), (UC_X86_REG_ESI, actor),
                       (UC_X86_REG_EBX, 0)):
        u.reg_write(reg, value)
    start = 0x4DA806 if through_move_sound else 0x4DA886
    end = (0x4DAB3C, 0x4DAF00) if through_move_sound else 0x4DAA01
    return run_checked(u, start, end, count=500000)


def body_cadence(art):
    cases = []
    for moving in [False, True]:
        for deploy_to_land, height in [(False, 0), (True, 0), (True, 500)]:
            for frame in [0, 1, 2, 3, 4, -1, -2147483648]:
                cases.append(dict(moving=moving, deploy_to_land=deploy_to_land, height=height,
                                  frame=frame, walk_rate=3, idle_rate=4))
    for flags in [[0, 0, 0], [1, 0, 0], [0, 1, 0], [1, 0, 1]]:
        for target in [False, True]:
            for hover_attack in [False, True]:
                cases.append(dict(moving=False, deploy_to_land=False, height=0, frame=3,
                                  flags=flags, target=target, hover_attack=hover_attack,
                                  walk_rate=3, idle_rate=0))
    for blocking in ['warp_out', 'warp_in', 'locomotor_swap']:
        for moving in [False, True]:
            cases.append(dict(moving=moving, deploy_to_land=True, height=500, frame=0,
                              **{blocking: True}, walk_rate=1, idle_rate=1))
    for counter in [2147483647, 4294967295]:
        cases.append(dict(moving=False, deploy_to_land=True, height=500, frame=1,
                          walk_rate=1, idle_rate=0, counter=counter))
    rows = []
    for case in cases:
        m = Unit(case, art)
        u = m.u
        u.mem_write(TYPE + 0x294, dwords(case['walk_rate'], case['idle_rate']))
        u.mem_write(TYPE + 0x390, bytes([case.get('hover_attack', False)]))
        u.mem_write(ACTOR + 0x2B4, dwords(ACTOR if case.get('target', False) else 0))
        u.mem_write(ACTOR + 0x270, bytes([case.get('warp_in', False), case.get('warp_out', False)]))
        u.mem_write(ACTOR + 0x6AD, bytes([case.get('locomotor_swap', False)]))
        u.mem_write(ACTOR + 0x538, dwords(case.get('counter', 0)))
        u.mem_write(0xA8ED84, dwords(case['frame']))
        moving_now = m.read32(m.read32(m.read32(ACTOR + 0x674)) + 0x80)

        def hook(_u, address, size, data):
            if address == moving_now:
                m.ret(4, int(case['moving']))

        u.hook_add(UC_HOOK_CODE, hook)
        run_foot_counter_slice(u, ACTOR, SP)
        rows.append(dict(input=case, counter=m.read32(ACTOR + 0x538)))
    return rows


def tube_admission(art):
    m = Unit({}, art)
    positions = [(10, 10), (10, 9), (10, 8), (10, 7), (9, 10), (8, 10), (7, 10)]
    rows = []
    m.u.mem_write(0x8B4148, dwords(1))
    for mask in range(128):
        for i, xy in enumerate(positions):
            present = bool(mask & (1 << i))
            m.u.mem_write(cell(*xy) + 0x116, struct.pack('<h', 0 if present else -1))
            m.u.mem_write(cell(*xy) + 0xEC, dwords(10 if present else 0))
        m.call(0x484AE0, cell(10, 10), [])
        rows.append(dict(mask=mask, refused=bool(m.u.reg_read(UC_X86_REG_EAX) & 255)))
    return dict(mask_cells=positions, tube_count=1, rows=rows)


def animation_constructor():
    from tools.rules_oracle.bridge_anim_lists import HEAP as READER_HEAP
    from tools.spatial_oracle import anim_bouncer_launch as launch
    keys = sections(asset('artmd.ini').read_bytes())['SCHPDEPL']
    reader = Reader(ASSETS, {'SCHPDEPL': keys})
    pointer = reader.alloc(0x400)
    reader.invoke(0x427530, pointer, [reader.cstring('SCHPDEPL')])
    reader.invoke(0x427D00, pointer, [INI])
    rows = []
    for reverse in [False, True]:
        m = launch.Machine(1)
        u = m.uc
        # Transfer the independently original-read type and unchanged SHP
        # backing bytes into the existing constructor fixture. No scalar
        # translation/recalculation supplies the frame/rate values.
        u.mem_map(READER_HEAP, 0x400000)
        u.mem_write(READER_HEAP, bytes(reader.u.mem_read(READER_HEAP, 0x400000)))
        m.types[pointer] = 'SCHPDEPL'
        anim, coord = m.heap, launch.STUB + 100
        u.mem_write(coord, dwords(2688, 2688, 0))
        u.mem_write(launch.SP, dwords(launch.STOP, pointer, coord, 0, 1, 0x600, 0, reverse))
        u.reg_write(UC_X86_REG_ESP, launch.SP)
        u.reg_write(UC_X86_REG_ECX, anim)
        before = m.rng()
        run_checked(u, launch.CTOR, launch.STOP, count=500000)
        rows.append(dict(reverse=reverse, state=launch.constructor_state(u, anim),
                         events=m.events, raw_scenario_draw_count=m.advances,
                         scenario_rng_unchanged=before == m.rng()))
    return rows


def jumpjet_landing():
    from tools.spatial_oracle.jumpjet_states import States, BASE, centre
    from tools.spatial_oracle.walk_head_occupation import OWNER, LOCO
    from tools.spatial_oracle.jumpjet_coordinates import RULES
    rows = []
    for landing in [False, True]:
        m = States(dict(BASE, simple_deployer=True, deploy_to_land=True, phase=2, moving=True,
                        start=[*centre(10), 500], target_height=500))
        m.uc.mem_write(LOCO + 0x40, dwords(*centre(10), 0))
        m.uc.mem_write(OWNER + 0x134, bytes([landing]))
        m.call(0x54BD30, LOCO, [])
        rows.append(dict(name='hold', landing_before=landing,
                         landing_after=m.uc.mem_read(OWNER + 0x134, 1)[0], state=m.state(), events=m.events))
    for raw in [0, 64, 288, -32]:
        m = States(dict(BASE, simple_deployer=True, deploy_to_land=True, phase=4,
                        start=[*centre(10), 500], target_height=0))
        m.uc.mem_write(OWNER + 0x134, b'\x01')
        m.uc.mem_write(RULES + 0x48, dwords(raw))
        m.call(0x54C550, LOCO, [])
        rows.append(dict(name='deploy_facing', deploy_dir_raw=raw, state=m.state(), events=m.events))
    m = States(dict(BASE, simple_deployer=True, deploy_to_land=True, phase=4, moving=True,
                    start=[*centre(10), 0], target_height=0))
    m.uc.mem_write(LOCO + 0x40, dwords(*centre(10), 0))
    m.uc.mem_write(OWNER + 0x134, b'\x01')
    m.uc.mem_write(RULES + 0x48, dwords(64))
    m.call(0x54C550, LOCO, [])
    rows.append(dict(name='touchdown', landing_before=True,
                     landing_after=m.uc.mem_read(OWNER + 0x134, 1)[0], state=m.state(), events=m.events))
    return rows


def generate():
    art = retail_art()
    return dict(schema_version=1, native_sha256=NATIVE_SHA256, retail_art=art,
                readers=readers(), lifecycle=lifecycle(art['result']),
                mission_timelines=mission_timelines(art['result']), jumpjet=jumpjet_landing(),
                ownership=ownership_and_admission(art['result']), draw_frames=draw_frames(art),
                body_cadence=body_cadence(art['result']), tube_admission=tube_admission(art['result']),
                animation_constructor=animation_constructor())


def metadata():
    return provenance(
        scope='Original simple-deployer update/undeploy and whole Unit Mission_Unload simple branch; shared Stage tick cadence; native retail SCHPDEPL ART prerequisites and reader controls; Jumpjet hold/descend selection',
        assumptions=[
            'Real Unit vtable and stopped Drive/map fixture inherited from refinery_dock. Unit and Techno constructors are not run by lifecycle rows; flags, stages and animation references are explicit inputs.',
            'Lifecycle AnimType rate/start/end inputs come from original AnimType427530/427D00 over unchanged physical ARTMD and SCHPDEPL.SHP, not Rust. Report remains -1 in an empty native sound registry; playback is not claimed.',
            'Timers and signed/wrapping thresholds execute original instructions. Timelines run Mission_Unload then shared Stage6FABC4 once per frame, matching native TechnoAI order; this is a component cadence with literal1 return, not complete world scheduler execution.',
            'DeployDir rows execute original constructor prefix and AudioVisual reader block. DeployingAnim history executes the original rules block and original AnimType factory. Cached INI objects are supplied; no physical INI loader/layer orchestration claim.',
            'Jumpjet rows reuse jumpjet_states declared corridor and supplied callback boundaries. They execute original handlers, not a full deployment flight.'],
        substitutions=[
            'Anim allocation returns fixed mapped storage. AnimConstructor421EA0 and SetOwner424B50 are argument observers; no substitute animation AI is executed. GetRemapColour705D70 supplies a palette sentinel.',
            'NearbyLocation703590 records exact owner/anchor and supplies cell11,10; SetDestination741970 records the resulting original MapGetCell pointer and flag without locomotor work. Nearby search and post-undeploy flight are not certified.',
            'DrawVoxel selector/frame slice73B494..73B50E executes with supplied visibility/disguise callbacks. Body counts come from unchanged physical SCHP/SCHD HVA headers; disguise count3 and turret count4 are explicit arithmetic controls. No rasterization or HVA matrix loader claim.',
            'Foot body-counter slice4DA886..4DAA01 begins after admitted Process/alive checks. Its IsMovingNow callback is supplied; Unit/type/warp/deployed/height readers execute. No separate voxel timing model is used.',
            'Separate animation_constructor rows reuse anim_bouncer_launch.Machine and execute full original421EA0 with original-read SCHPDEPL bytes. Its declared Map/mark/display/Start sinks remain: animation Start/audio playback and later AnimAI are not executed.',
            'Inherited refinery fixture substitutes only Interlocked imports before these observer boundaries. QueueMission/Commence/FootUnload and the full simple-deployer receivers execute unchanged.'],
        entry_points={'deploy': DEPLOY, 'undeploy': UNDEPLOY, 'mission_unload': UNLOAD,
                      'foot_unload': 0x4DA2B0, 'stage_tick': 0x6FABC4,
                      'anim_type_ctor': 0x427530, 'anim_type_art_reader': 0x427D00,
                      'deploying_anim_reader': 0x714706, 'deploy_dir_reader': 0x669272,
                      'jumpjet_hold': 0x54BD30, 'jumpjet_descend': 0x54C550,
                      'anim_pointer_expired': 0x710410, 'deploy_admission': 0x700D50,
                      'draw_voxel_selector': 0x73B494, 'foot_body_counter': 0x4DA886,
                      'tube_neighborhood': 0x484AE0, 'body_rate_reader': 0x712222})


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
