"""Native references for a spy's infiltration of a building.

Run python -m tools.spy_infiltration_oracle --check (or explicit --write).
Rust consumers: src/sim/spy_infiltration_tests.rs (house and Super state)
and src/app/match_runtime/sound_dispatch.rs (radar event and EVA lines).

Section, each case executed in a fresh emulator:
- infiltrate: BuildingClass::Infiltrate 0x4571E0 on a building of house H0
  by a spy of house H1 (or of H0 itself), over each arm and their order:
  Radar=, Power=, [AI] BuildTech=, SuperWeapon=, Storage=, Factory=. The
  local player (PlayerPtr 0xA83D4C, GameMode 1) is H0, H1 or a third house,
  and the radar event's answer is given per row. Every supplied House starts
  with RecheckTechTree (+0x1FC) clear; repeated-gain controls supply an already
  true stolen-tech/veterancy flag while leaving that request clear.

Native: HouseClass::IsPlayer 0x50B6F0, ReshroudMap 0x50BD10,
SpyPowerSabotage 0x50BC90, Available_Money 0x4F6990 (through the money
interface at House+0x24), Spend_Money 0x4F9790, Add_Credits 0x4F9950,
Math::ftol 0x7C5F00 and SuperClass::Reset 0x6CE0B0 all run. Recorded stubs:
MapClass::Reset_Shroud 0x577AB0, CreateRadarEvent 0x65FA70, VoxClass::PlayEVA
0x752700 and the building's vt+0x1B8 (its cell). Both House +0x1FC bytes are
recorded with gains/money; the building's vt+0x124 (Mark) and sidebar byte
0x884B8E remain outside the Rust state comparison. See spy_infiltration_oracle.md.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX

from tools.ai_base_building_oracle import Emu, FAKE, HOUSE, HOUSE_TYPE, MONEY_VTABLE, RULES, STUBS
from tools.native_oracle import OracleError, finish_vectors, provenance

INFILTRATE = 0x4571E0
AVAILABLE_MONEY = 0x4F6990
RESET_SHROUD = 0x577AB0
CREATE_RADAR_EVENT = 0x65FA70
PLAY_EVA = 0x752700

INFILTRATOR = FAKE + 0x8000
BYSTANDER = FAKE + 0x10000
BUILDING = FAKE + 0x40000
BUILDING_TYPE = FAKE + 0x42000
BUILDING_VTABLE = FAKE + 0x44000
SUPER = FAKE + 0x45000
SUPER_TYPE = FAKE + 0x45100
SUPERS = FAKE + 0x45400
BUILD_TECH = FAKE + 0x45800
STUB_CELL = STUBS + 0x300
STUB_MARK = STUBS + 0x310

GAME_MODE = 0xA8B238
PLAYER_PTR = 0xA83D4C
FRAME = 0xA8ED84

CELL = (37, 52)
FACTORY = {'unit': 0x28, 'infantry': 0x10, 'aircraft': 0x03, 'building': 0x07}
HOUSES = {'owner': HOUSE, 'infiltrator': INFILTRATOR, 'bystander': BYSTANDER}


def f32(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


def write8(emu, address, value):
    emu.uc.mem_write(address, bytes([int(value) & 0xFF]))


def read8(emu, address):
    return emu.uc.mem_read(address, 1)[0]


def read_name(emu, address):
    return bytes(emu.uc.mem_read(address, 0x40)).split(b'\0', 1)[0].decode('ascii')


def house_name(address):
    return next((name for name, house in HOUSES.items() if house == address), hex(address))


def house(emu, address, balance):
    """A house as tools.house_money_oracle keeps it: no silo storage
    (+0x2FC), a capacity of 200 (+0x310), IncomeMult 1."""
    emu.write32(address + 0x24, MONEY_VTABLE)
    emu.write32(address + 0x34, HOUSE_TYPE)
    emu.write32(address + 0x30C, balance)
    emu.write32(address + 0x310, 200)
    # A settled input boundary, not House constructor's initially true latch.
    write8(emu, address + 0x1FC, 0)


def run(row):
    """Execute one row; returns its expected outcome."""
    emu = Emu()
    kind = row['type']
    emu.write32(GAME_MODE, 1)
    emu.write32(PLAYER_PTR, HOUSES[row['local']])
    emu.write32(FRAME, row['frame'])
    emu.write32(HOUSE_TYPE + 0x148, f32(1.0))
    emu.write32(MONEY_VTABLE + 0x18, AVAILABLE_MONEY)
    house(emu, HOUSE, row['owner']['balance'])
    house(emu, INFILTRATOR, row['infiltrator']['balance'])
    house(emu, BYSTANDER, 0)
    # Declared prior gains use the same five native bytes as the result. No
    # Infiltrate result or recheck answer is supplied by this initialization.
    gains = row.get('initial_gains', [0] * 5)
    if len(gains) != 5 or any(value not in (0, 1) for value in gains):
        raise OracleError('initial_gains requires five boolean bytes')
    for offset, value in zip(range(0x2BC, 0x2C1), gains):
        write8(emu, INFILTRATOR + offset, value)
    write8(emu, HOUSE + 0x577A, row['owner']['spy_sat'])

    emu.write32(RULES + 0xD64, row['rules']['blackout'])
    emu.write32(RULES + 0xD68, f32(float(row['rules']['percent'])))
    emu.write32(BUILD_TECH, BUILDING_TYPE)
    emu.write32(RULES + 0x920, BUILD_TECH)
    emu.write32(RULES + 0x92C, int(kind['build_tech']))

    emu.write32(BUILDING, BUILDING_VTABLE)
    emu.write32(BUILDING + 0x21C, HOUSE)
    emu.write32(BUILDING + 0x520, BUILDING_TYPE)
    emu.write32(BUILDING_VTABLE + 0x1B8, STUB_CELL)
    emu.write32(BUILDING_VTABLE + 0x124, STUB_MARK)
    write8(emu, BUILDING_TYPE + 0x16A4, kind['radar'])
    emu.write32(BUILDING_TYPE + 0xEE0, kind['power'])
    emu.write32(BUILDING_TYPE + 0x6D0, kind['side'])
    emu.write32(BUILDING_TYPE + 0x800, kind['storage'])
    emu.write32(BUILDING_TYPE + 0xEB8, FACTORY.get(kind['factory'], 0xFFFFFFFF))
    super_ = row['super']
    emu.write32(BUILDING_TYPE + 0x16F0, 0 if kind['super_weapon'] else 0xFFFFFFFF)
    emu.write32(HOUSE + 0x258, SUPERS)
    emu.write32(SUPERS, SUPER)
    if super_ is not None:
        emu.write32(SUPER + 0x24, 0xFFFFFFFF)
        emu.write32(SUPER + 0x28, SUPER_TYPE)
        emu.write32(SUPER + 0x2C, HOUSE)
        emu.write32(SUPER + 0x30, super_['start'])
        emu.write32(SUPER + 0x38, super_['duration'])
        write8(emu, SUPER + 0x6F, super_['ready'])
        emu.write32(SUPER_TYPE + 0xB0, super_['recharge'])

    def cell(e):
        out = e.arg(0)
        e.uc.mem_write(out, struct.pack('<hh', *CELL))
        return out

    def radar_event(e):
        x, y = struct.unpack('<hh', struct.pack('<I', e.arg(0)))
        e.events.append(['radar_event', e.uc.reg_read(UC_X86_REG_ECX), [x, y]])
        return int(row['radar_accepted'])

    def play_eva(e):
        if e.uc.reg_read(UC_X86_REG_EDX) != 0xFFFFFFFF or e.arg(0) != 0xFFFFFFFF:
            raise OracleError('PlayEVA with a priority other than -1')
        e.events.append(['eva', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])

    emu.hook(STUB_CELL, cell, 4)
    emu.hook(STUB_MARK, lambda e: None, 4)
    emu.hook(CREATE_RADAR_EVENT, radar_event, 4)
    emu.hook(PLAY_EVA, play_eva, 4)
    emu.hook(RESET_SHROUD, lambda e: e.events.append(['reset_shroud', house_name(e.arg(0))]), 4)

    infiltrator = HOUSE if row['own'] else INFILTRATOR
    emu.invoke(INFILTRATE, ecx=BUILDING, args=(infiltrator,))

    expected = {
        'events': emu.events,
        'owner': {
            'balance': emu.read_i32(HOUSE + 0x30C),
            'spent': emu.read_i32(HOUSE + 0x2DC),
            'recheck_tech': read8(emu, HOUSE + 0x1FC),
            'recheck_power': read8(emu, HOUSE + 0x5778),
            'blackout': [emu.read_i32(HOUSE + 0x2A4), emu.read_i32(HOUSE + 0x2AC)],
        },
        'infiltrator': {
            'balance': emu.read_i32(INFILTRATOR + 0x30C),
            'recheck_tech': read8(emu, INFILTRATOR + 0x1FC),
            'stolen': [read8(emu, INFILTRATOR + offset) for offset in (0x2BC, 0x2BD, 0x2BE)],
            'veteran': [read8(emu, INFILTRATOR + 0x2BF), read8(emu, INFILTRATOR + 0x2C0)],
        },
    }
    if super_ is not None:
        expected['super'] = {
            'ready': read8(emu, SUPER + 0x6F),
            'timer': [emu.read_i32(SUPER + 0x30), emu.read_i32(SUPER + 0x38)],
        }
    return expected


def row(name, *, own=False, local='owner', radar_accepted=True, frame=1200, radar=False,
        power=0, side=0, build_tech=False, super_weapon=False, storage=0, factory=None,
        blackout=1000, percent='.5', spy_sat=False, balance=1000, infiltrator_balance=500,
        super_=None, initial_gains=None):
    case = {
        'name': name, 'own': own, 'local': local, 'radar_accepted': radar_accepted,
        'frame': frame,
        'type': {'radar': radar, 'power': power, 'side': side, 'build_tech': build_tech,
                 'super_weapon': super_weapon, 'storage': storage, 'factory': factory},
        'rules': {'blackout': blackout, 'percent': percent},
        'owner': {'spy_sat': spy_sat, 'balance': balance},
        'infiltrator': {'balance': infiltrator_balance},
        'super': super_,
    }
    if initial_gains is not None:
        case['initial_gains'] = list(initial_gains)
    case['expected'] = run(case)
    return case


CHARGING = {'start': 300, 'duration': 9000, 'ready': False, 'recharge': 9000}


def locals_and_answers():
    """Each local player, and each answer of the radar event the owner's or
    the spy's local test lets it ask."""
    for local in ('owner', 'infiltrator'):
        for accepted in (True, False):
            yield local, accepted
    yield 'bystander', True


def infiltrate():
    rows = []
    add = rows.append
    add(row('its own house: nothing', own=True, radar=True, power=100))
    for spy_sat in (False, True):
        for local, accepted in locals_and_answers():
            add(row(f'radar, spy sat {spy_sat}, local {local}, radar event {accepted}',
                    radar=True, spy_sat=spy_sat, local=local, radar_accepted=accepted))
    for local, accepted in locals_and_answers():
        add(row(f'power, local {local}, radar event {accepted}', power=200,
                local=local, radar_accepted=accepted))
    add(row('power, blackout 0 (the constructor default)', power=1, blackout=0))
    add(row('power, a negative blackout', power=50, blackout=-30, frame=77))
    for side in (0, 1, 2, -1):
        add(row(f'BuildTech, AIBasePlanningSide {side}', build_tech=True, side=side,
                local='infiltrator'))
    for local, accepted in locals_and_answers():
        add(row(f'BuildTech, local {local}, radar event {accepted}', build_tech=True,
                local=local, radar_accepted=accepted))
    for local, accepted in locals_and_answers():
        add(row(f'super weapon, local {local}, radar event {accepted}', super_weapon=True,
                super_=CHARGING, local=local, radar_accepted=accepted))
    for name, state in (
        ('charged', {'start': 300, 'duration': 9000, 'ready': True, 'recharge': 9000}),
        ('on hold', {'start': -1, 'duration': 4000, 'ready': False, 'recharge': 9000}),
        ('on hold, RechargeTime 0', {'start': -1, 'duration': 10, 'ready': False, 'recharge': 0}),
        ('on hold, a negative RechargeTime', {'start': -1, 'duration': 10, 'ready': False,
                                              'recharge': -5}),
    ):
        add(row(f'super weapon {name}', super_weapon=True, super_=state))
    for local, accepted in locals_and_answers():
        add(row(f'cash, local {local}, radar event {accepted}', storage=200,
                local=local, radar_accepted=accepted))
    for balance, percent in ((1, '.5'), (0, '.5'), (-50, '.5'), (999, '.5'), (1000, '0.3'),
                             (3333, '0.07'), (1000, '1'), (1000, '2'), (0x7FFFFFFF, '.5'),
                             (777, '-0.5')):
        add(row(f'cash, balance {balance}, SpyMoneyStealPercent {percent}', storage=1,
                balance=balance, percent=percent, local='infiltrator'))
    for factory in ('unit', 'infantry'):
        for local, accepted in locals_and_answers():
            add(row(f'{factory} factory, local {local}, radar event {accepted}',
                    factory=factory, local=local, radar_accepted=accepted))
    for factory in ('aircraft', 'building'):
        add(row(f'{factory} factory: no arm', factory=factory, local='infiltrator'))
    add(row('no arm', local='infiltrator'))
    add(row('radar before power', radar=True, power=100, local='infiltrator'))
    add(row('power before BuildTech', power=100, build_tech=True, local='infiltrator'))
    add(row('BuildTech before the super weapon', build_tech=True, super_weapon=True,
            super_=CHARGING, local='infiltrator'))
    add(row('the super weapon before cash', super_weapon=True, super_=CHARGING, storage=100,
            local='infiltrator'))
    add(row('cash before the factory', storage=100, factory='unit', local='infiltrator'))
    add(row('a negative Power= takes no arm', power=-150, local='infiltrator'))
    # Requests are unconditional, even if the gain already holds. Earlier
    # control inputs/order remain unchanged.
    for side, gains in ((0, [0, 0, 1, 0, 0]), (1, [0, 1, 0, 0, 0]),
                        (2, [1, 0, 0, 0, 0])):
        add(row(f'BuildTech side {side}, gain already true and recheck clear',
                build_tech=True, side=side, initial_gains=gains, local='infiltrator'))
    for factory, gains in (('unit', [0, 0, 0, 0, 1]), ('infantry', [0, 0, 0, 1, 0])):
        add(row(f'{factory} factory, gain already true and recheck clear',
                factory=factory, initial_gains=gains, local='infiltrator'))
    add(row('own BuildTech: no gain or recheck', own=True, build_tech=True, side=1))
    add(row('own infantry factory: no gain or recheck', own=True, factory='infantry'))
    return rows


def generate():
    return dict(infiltrate=infiltrate())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=('BuildingClass::Infiltrate on synthetic houses, building type, rules and one '
               'Super: every arm, their order, the local-player and radar-event gates of the '
               'radar event and the EVA lines. The Super has no anim and no custom charge '
               'time (+0x24 = -1).'),
        assumptions=['GameMode 1, so IsPlayer compares each house with PlayerPtr.',
                     'Both houses hold no silo storage (+0x2FC) and a capacity (+0x310) of '
                     '200, so Spend_Money never drains buildings.',
                     'Each supplied House explicitly starts with RecheckTechTree+1FC=0. '
                     'Optional initial_gains supplies five2BC..2C0 bytes in native '
                     'Third/Soviet/Allied/veteranInfantry/veteranUnit order. Whole Infiltrate '
                     'owns its unconditional gain/recheck writes, including repeat gains.',
                     'This is isolated4571E0. Real519FF8 caller, Capture mission RNG/timer, '
                     'Tag48, UnInit/expiry/Limbo/scalar cleanup and later House suffix/'
                     '4456D0/retained-sidebar insertion are separate production integration '
                     'responsibilities. The leaf has no direct RNG or detach; Super/blackout '
                     'timer effects execute in their selected arms.'],
        substitutions=['MapClass::Reset_Shroud 0x577AB0, CreateRadarEvent 0x65FA70 (answered '
                       'per row) and VoxClass::PlayEVA 0x752700 are recorded stubs.',
                       "The building's vt+0x1B8 answers a fixed cell; vt+0x124 (Mark) returns."],
        entry_points={'infiltrate': INFILTRATE},
    ))
