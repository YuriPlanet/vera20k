"""Original TechnoClass::PassiveAcquireGate 0x00709290 with its CanAcquireTarget 0x007091D0.

Both bodies execute natively on a scratch object with a scratch vtable. The gate decides
whether an object's passive scan may run (TechnoClass::AI_Update's post-mission block):
the computer team arm, CanAcquireTarget, the two Move arms for a parked object
(0x00709301..0x007093D3: a BalloonHover= Foot, or a Unit whose type is IsSimpleDeployer=,
whose NavCom is its own cell and whose waypoint-planning token +0x514 is NULL or holds no
nodes), OpportunityFire=, and the Guard mission's AreaFire refusal.

A row records the returned byte and the ordered names of the supplied calls. Supplied:
    what_am_i      vt+0x2C -> class (Unit 1, Aircraft 2, Building 6, Infantry 0xF)
    type           vt+0x84 -> the scratch TechnoType
    warping        vt+0x1DC -> bool (Temporal holds a target)
    occupants      vt+0x408 -> int (building occupants)
    engineer       vt+0x330 -> bool
    armed          vt+0x2AC -> bool
    cell           vt+0x1BC -> the object's own cell (a scratch cell object)
    weapon_slot    vt+0x3E4 -> int
    weapon         vt+0x3F8 (slot) -> the scratch WeaponStruct (its WeaponType +0x150 AreaFire)
    select_weapon  vt+0x2E4 (target) -> int
    human          HouseClass 0x0050B730 (ECX = the owner's house) -> bool
    capture_full   CaptureManagerClass 0x004722A0 (ECX = +0x2BC) -> bool
The token getter 0x00636DC0 executes natively on the scratch token.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP

from tools.native_oracle import (load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE,
                                 RET_MAGIC, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

(OWNER, TYPE, VTABLE, CALLBACKS, CELL, OTHER_CELL, TOKEN, TEAM, TEAM_TYPE, HOUSE,
 WEAPON_STRUCT, WEAPON_TYPE, TARGET, SLAVE_OWNER, CAPTURE, BUILDING_TYPE) = [
    SCRATCH + n * 0x2000 for n in range(16)]
SP = STACK_BASE + STACK_SIZE - 0x1000
GATE = 0x709290
CAN_ACQUIRE = 0x7091D0
HUMAN = 0x50B730
CAPTURE_FULL = 0x4722A0
MISSION = dict(none=-1, sleep=0, attack=1, move=2, guard=5, area_guard=0x14, hunt=0xB)
CLASS = dict(unit=1, aircraft=2, building=6, infantry=0xF)

# (slot, row key, default, bytes popped)
QUERIES = [(0x2C, 'what_am_i', None, 0), (0x84, 'type', None, 0), (0x1DC, 'warping', 0, 0),
           (0x408, 'occupants', 0, 0), (0x330, 'engineer', 0, 0), (0x2AC, 'armed', 1, 0),
           (0x1BC, 'cell', None, 0), (0x3E4, 'weapon_slot', 1, 0), (0x3F8, 'weapon', None, 4),
           (0x2E4, 'select_weapon', 0, 4)]


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, 0x20000)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.calls = []
        self.row = {}
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def ret(self, pop, value):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        back = struct.unpack('<I', u.mem_read(sp, 4))[0]
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_ESP, sp + 4 + pop)
        u.reg_write(UC_X86_REG_EIP, back)

    def observe(self, u, pc, _size, _data):
        if CALLBACKS <= pc < CALLBACKS + 0x100 * len(QUERIES) and (pc - CALLBACKS) % 0x100 == 0:
            self.calls.append(QUERIES[(pc - CALLBACKS) // 0x100][1])
        elif pc == HUMAN:
            assert u.reg_read(UC_X86_REG_ECX) == HOUSE
            self.calls.append('human')
            self.ret(0, int(self.row.get('human', True)))
        elif pc == CAPTURE_FULL:
            assert u.reg_read(UC_X86_REG_ECX) == CAPTURE
            self.calls.append('capture_full')
            self.ret(0, int(self.row.get('capture_full', False)))

    def execute(self, row):
        u = self.u
        self.row = row
        u.mem_write(SCRATCH, bytes(0x20000))
        answers = dict(what_am_i=CLASS[row.get('class', 'infantry')], type=TYPE, cell=CELL,
                       weapon=WEAPON_STRUCT)
        for n, (slot, key, default, pop) in enumerate(QUERIES):
            value = answers[key] if key in answers else int(row.get(key, default))
            pointer = CALLBACKS + n * 0x100
            u.mem_write(VTABLE + slot, dwords(pointer))
            code = b'\xB8' + dwords(value & 0xFFFFFFFF)
            code += b'\xC2' + struct.pack('<H', pop) if pop else b'\xC3'
            u.mem_write(pointer, code)
        u.mem_write(OWNER, dwords(VTABLE))
        # A building is no Foot; every other class is.
        foot = row.get('foot', row.get('class', 'infantry') != 'building')
        u.mem_write(OWNER + 0x14, bytes([4 if foot else 0]))
        u.mem_write(OWNER + 0xAC, dwords(MISSION[row.get('mission', 'move')] & 0xFFFFFFFF))
        u.mem_write(OWNER + 0x2B4, dwords(TARGET if row.get('target') else 0))
        u.mem_write(OWNER + 0x21C, dwords(HOUSE))
        u.mem_write(OWNER + 0x2DC, dwords(SLAVE_OWNER if row.get('slave') else 0))
        u.mem_write(OWNER + 0x2BC, dwords(CAPTURE if row.get('capture_full') is not None else 0))
        u.mem_write(OWNER + 0x520, dwords(BUILDING_TYPE))
        u.mem_write(BUILDING_TYPE + 0x157B, bytes([int(row.get('can_be_occupied', False))]))
        team = row.get('team')
        u.mem_write(OWNER + 0x5D4, dwords(TEAM if team else 0))
        if team:
            u.mem_write(TEAM + 0x24, dwords(TEAM_TYPE))
            u.mem_write(TEAM_TYPE + 0xAF, bytes([int(team.get('suicide', False))]))
            u.mem_write(TEAM_TYPE + 0xAD, bytes([int(team.get('aggressive', True))]))
        nav = row.get('nav_com', 'own')
        u.mem_write(OWNER + 0x5A4, dwords({'own': CELL, 'other': OTHER_CELL, None: 0}[nav]))
        token = row.get('token')
        u.mem_write(OWNER + 0x514, dwords(0 if token is None else TOKEN))
        if token is not None:
            u.mem_write(TOKEN + 0x14, dwords(token & 0xFFFFFFFF))
        # A Unit's type pointer (+0x6C4), read by the IsSimpleDeployer arm.
        u.mem_write(OWNER + 0x6C4, dwords(TYPE))
        u.mem_write(TYPE + 0xD6A, bytes([int(row.get('balloon_hover', True))]))
        u.mem_write(TYPE + 0xD99, bytes([int(row.get('can_passive_acquire', True))]))
        u.mem_write(TYPE + 0x6AF, bytes([int(row.get('opportunity_fire', False))]))
        u.mem_write(TYPE + 0xE13, bytes([int(row.get('simple_deployer', False))]))
        u.mem_write(WEAPON_STRUCT, dwords(WEAPON_TYPE if row.get('has_weapon', True) else 0))
        u.mem_write(WEAPON_TYPE + 0x150, bytes([int(row.get('area_fire', False))]))
        self.calls = []
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, OWNER)
        run_checked(u, GATE, RET_MAGIC, count=20000)
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4
        return dict(passes=bool(u.reg_read(UC_X86_REG_EAX) & 0xFF), calls=self.calls)


def inputs():
    rows = []
    # A Rocketeer parked after a Move order: a BalloonHover= Infantry whose NavCom is
    # its own cell, of a human house, in no team.
    base = dict(name='parked_rocketeer')
    rows.append(base)
    rows += [dict(name=f'nav_com_{n}', nav_com=None if n == 'none' else n) for n in ('other', 'none')]
    rows += [dict(name=f'token_{t}', token=t) for t in (0, -1, 1, 7)]
    rows.append(dict(name='balloon_not_foot', foot=False))
    rows.append(dict(name='not_balloon', balloon_hover=False))
    rows += [dict(name=f'{m}_mission', mission=m) for m in ('attack', 'sleep', 'hunt', 'area_guard', 'none')]
    rows.append(dict(name='opportunity_fire_elsewhere', opportunity_fire=True, nav_com='other'))
    rows.append(dict(name='opportunity_fire_attack', opportunity_fire=True, mission='attack'))
    # The other classes on arm 1, and the IsSimpleDeployer= Unit arm.
    for cls in ('unit', 'aircraft', 'building'):
        rows.append(dict(name=f'balloon_{cls}', **{'class': cls}))
    for simple in (True, False):
        for balloon in (True, False):
            for nav in ('own', 'other'):
                rows.append(dict(name=f'unit_simple{int(simple)}_balloon{int(balloon)}_{nav}',
                                 **{'class': 'unit'}, simple_deployer=simple,
                                 balloon_hover=balloon, nav_com=nav))
    rows.append(dict(name='simple_deployer_infantry', balloon_hover=False, simple_deployer=True))
    rows.append(dict(name='unit_simple_token_1', **{'class': 'unit'}, balloon_hover=False,
                     simple_deployer=True, token=1))
    # CanAcquireTarget ahead of the Move arms.
    rows += [dict(name='warping', warping=1), dict(name='slave', slave=True),
             dict(name='no_passive_acquire', can_passive_acquire=False),
             dict(name='capture_full', capture_full=True),
             dict(name='capture_not_full', capture_full=False),
             dict(name='engineer_human', engineer=1), dict(name='engineer_ai', engineer=1, human=False),
             dict(name='unarmed', armed=0),
             dict(name='empty_garrison', **{'class': 'building'}, balloon_hover=False,
                  mission='guard', nav_com=None, can_be_occupied=True, occupants=0),
             dict(name='garrisoned', **{'class': 'building'}, balloon_hover=False,
                  mission='guard', nav_com=None, can_be_occupied=True, occupants=2)]
    # The computer team arm: no target, a Foot of an AI house, Aggressive=yes and
    # Suicide=no, on Move, passes without CanAcquireTarget.
    for aggressive in (True, False):
        for suicide in (False, True):
            rows.append(dict(name=f'team_aggr{int(aggressive)}_suicide{int(suicide)}', human=False,
                             armed=0, nav_com='other',
                             team=dict(aggressive=aggressive, suicide=suicide)))
    rows.append(dict(name='team_with_target', human=False, armed=0, nav_com='other', target=True,
                     team=dict(aggressive=True, suicide=False)))
    rows.append(dict(name='team_human', human=True, armed=0, nav_com='other',
                     team=dict(aggressive=True, suicide=False)))
    # Guard: the AreaFire refusal.
    for area in (False, True):
        for selected in (0, 1):
            rows.append(dict(name=f'guard_area{int(area)}_select{selected}', mission='guard',
                             nav_com='other', area_fire=area, select_weapon=selected,
                             weapon_slot=1))
    rows.append(dict(name='guard_no_weapon', mission='guard', nav_com='other', has_weapon=False))
    rows.append(dict(name='guard_parked', mission='guard'))
    return rows


def generate():
    fixture = Fixture()
    return [dict(input=row, output=fixture.execute(row)) for row in inputs()]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original TechnoClass::PassiveAcquireGate 0x00709290 with its CanAcquireTarget '
              '0x007091D0, on a scratch object: the computer team arm, CanAcquireTarget, the '
              'Move arms for a parked BalloonHover= Foot and IsSimpleDeployer= Unit '
              '(0x00709301..0x007093D3), OpportunityFire= and the Guard AreaFire refusal.',
        entry_points={'passive_acquire_gate': GATE, 'can_acquire_target': CAN_ACQUIRE},
        assumptions=['The object +0x14 bit 2 is AbstractClass IsFoot; +0x5A4 NavCom, +0x514 the '
                     'waypoint-planning token (its +0x14 read by 0x00636DC0), +0x5D4 Team '
                     '(TeamType +0x24: Suicide +0xAF, Aggressive +0xAD), +0x2DC slave owner, '
                     '+0x2BC CaptureManager, +0x520 the building type (+0x157B CanBeOccupied), '
                     'a Unit\'s +0x6C4 type (+0xE13 IsSimpleDeployer); TechnoType BalloonHover '
                     '+0xD6A, CanPassiveAquire +0xD99, OpportunityFire +0x6AF.'],
        substitutions=['Scratch vtable queries (what_am_i, type, warping, occupants, engineer, '
                       'armed, cell, weapon_slot, weapon, select_weapon) return the row\'s values; '
                       'HouseClass 0x0050B730 (human) and CaptureManagerClass 0x004722A0 '
                       '(full) are entry-hooked returns.'],
    ))
