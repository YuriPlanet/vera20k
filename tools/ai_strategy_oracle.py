"""Native references for a computer house's strategy tick.

Run python -m tools.ai_strategy_oracle --check (or explicit --write).
Rust consumer: src/sim/house_strategy_tests.rs.

Sections, each executed in a fresh emulator per case (the fixture machinery
is tools.ai_base_building_oracle's):
- schedule: HouseClass::Update's Strategy block 0x4F8FBE..0x4F9038 (the
  +0x5634/+0x563C timer, the human and MultiplayPassive gates and the
  restart) with AI_Building_Strategy answered.
- strategy: HouseClass::AI_Building_Strategy 0x4FD500 with the original
  UpdateAngerNodes 0x504790, CellStruct == 0x50E470, Sqrt_Approx 0x4CAC40 and
  ftol 0x7C5F00. The cell lookup 0x5657A0 and the cell's coordinate (vt+0x48),
  AI_TryFireSW 0x5098F0, the IHouse money query (vt+0x18), Fire_Sale
  (vt+0x34), All_To_Hunt (vt+0x38), Check_Build_Need 0x4FD9A0,
  Manage_Build_Queue 0x4FDD10 and RandomRanged 0x65C7E0 are answered and
  recorded in call order.
- fire_sale: IHouse Fire_Sale 0x5013A0 over a building list; Sell_Back
  (vt+0x1A0) is recorded.
- all_to_hunt: IHouse All_To_Hunt 0x501400 over TechnoClass::Array; the
  type query (vt+0x84), ReceiveDamage (vt+0x16C), Team remove 0x6EA870,
  Queue_Mission (vt+0x1E8), WhatAmI (vt+0x2C), the occupant count (vt+0x408)
  and the garrison release 0x457DE0 are answered and recorded.
"""
from pathlib import Path
import random
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP

from tools.ai_base_building_oracle import (FAKE, HOUSE, RULES, STUBS, Emu, i16s)
from tools.native_oracle import (STACK_BASE, STACK_SIZE, OracleError, finish_vectors,
                                 provenance, run_checked)

GAME_MODE = 0xA8B238
FRAME = 0xA8ED84
HOUSES = 0xA8022C
HOUSE_COUNT = 0xA80238
TECHNOS = 0xA8EC7C
TECHNO_COUNT = 0xA8EC88

UPDATE_BLOCK, UPDATE_BLOCK_END = 0x4F8FBE, 0x4F9038
STRATEGY = 0x4FD500
ANGER = 0x504790
TRY_FIRE_SW = 0x5098F0
CHECK_BUILD_NEED = 0x4FD9A0
MANAGE_BUILD_QUEUE = 0x4FDD10
CELL_AT = 0x5657A0
FIRE_SALE = 0x5013A0
ALL_TO_HUNT = 0x501400
TEAM_REMOVE = 0x6EA870
GARRISON_RELEASE = 0x457DE0

# Fixture regions past tools.ai_base_building_oracle's.
PEERS = FAKE + 0x400000
PEER_SIZE = 0x6000
HOUSE_ARRAY = FAKE + 0x440000
TYPE_ACTIVE = FAKE + 0x441000
TYPE_PASSIVE = FAKE + 0x442000
ANGER_ITEMS = FAKE + 0x443000
BUILDING_ITEMS = FAKE + 0x444000
BUILDINGS = FAKE + 0x450000
BUILDING_SIZE = 0x600
FACTORY_TYPE = FAKE + 0x460000
PLAIN_TYPE = FAKE + 0x462000
IHOUSE_VTABLE = FAKE + 0x464000
CELL = FAKE + 0x465000
CELL_VTABLE = FAKE + 0x465100
OBJECT_VTABLE = FAKE + 0x466000
TECHNO_ITEMS = FAKE + 0x468000
OBJECTS = FAKE + 0x470000
OBJECT_SIZE = 0x800
OBJECT_TYPES = FAKE + 0x4C0000
OBJECT_TYPE_SIZE = 0x400
# Fixture-only object words the stubs read (no native reader in the
# functions under test): the type, WhatAmI and the occupant count.
OBJECT_TYPE_SLOT, OBJECT_KIND_SLOT, OBJECT_OCCUPANTS_SLOT = 0x7F0, 0x7F4, 0x7F8

STUB_MONEY = STUBS + 0x200
STUB_FIRE_SALE = STUBS + 0x210
STUB_ALL_TO_HUNT = STUBS + 0x220
STUB_COORD = STUBS + 0x230
STUB_SELL_BACK = STUBS + 0x240
STUB_TECHNO_TYPE = STUBS + 0x250
STUB_RECEIVE_DAMAGE = STUBS + 0x260
STUB_QUEUE_MISSION = STUBS + 0x270
STUB_WHAT_AM_I = STUBS + 0x280
STUB_OCCUPANTS = STUBS + 0x290

C4_WARHEAD = 0x00C4C4C4
BUILDING_KIND = 6


def i32(value):
    return struct.unpack('<i', struct.pack('<I', value & 0xFFFFFFFF))[0]


def byte(emu, address, value):
    emu.uc.mem_write(address, bytes([int(value)]))


def read_byte(emu, address):
    return emu.uc.mem_read(address, 1)[0]


def cell(emu, address, xy):
    emu.uc.mem_write(address, i16s(*xy))


# ---------------------------------------------------------------- schedule

def schedule_row(*, start, delay, frame, game_mode=1, human=False, control=False,
                 passive=False, answer=108):
    emu = Emu()
    emu.write32(GAME_MODE, game_mode)
    emu.write32(FRAME, frame)
    byte(emu, HOUSE + 0x1EC, human)
    byte(emu, HOUSE + 0x1ED, control)
    emu.write32(HOUSE + 0x34, TYPE_PASSIVE if passive else TYPE_ACTIVE)
    byte(emu, TYPE_PASSIVE + 0x1A6, 1)
    emu.write32(HOUSE + 0x5634, start)
    emu.write32(HOUSE + 0x563C, delay)

    def strategy(e):
        e.events.append(['strategy'])
        return answer
    emu.hook(STRATEGY, strategy, 0)
    uc = emu.uc
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    uc.reg_write(UC_X86_REG_ESI, HOUSE)
    run_checked(uc, UPDATE_BLOCK, UPDATE_BLOCK_END)
    return dict(start=i32(start), delay=i32(delay), frame=i32(frame), game_mode=game_mode,
                human=human, control=control, passive=passive, answer=answer,
                called=emu.events == [['strategy']],
                start_after=emu.read_i32(HOUSE + 0x5634),
                delay_after=emu.read_i32(HOUSE + 0x563C))


def schedule():
    rows = []
    # The constructor's timer (start = the construction frame, delay 0).
    for frame in (0, 1, 8):
        rows.append(schedule_row(start=0, delay=0, frame=frame))
    # Paused (start -1): the delay alone is the remainder.
    rows.append(schedule_row(start=-1, delay=0, frame=500))
    rows.append(schedule_row(start=-1, delay=5, frame=500))
    # Running: elapsed below, at and above the delay; a start in the future.
    for frame in (207, 208, 300, 99):
        rows.append(schedule_row(start=100, delay=108, frame=frame))
    # Wrapping frame subtraction.
    rows.append(schedule_row(start=0x7FFFFFF0, delay=100, frame=0x7FFFFFF0 + 99))
    rows.append(schedule_row(start=0x7FFFFFF0, delay=100, frame=0x7FFFFFF0 + 100))
    # Human gates: PlayerControl counts only in a campaign.
    for game_mode in (0, 1):
        for human, control in ((False, False), (True, False), (False, True), (True, True)):
            rows.append(schedule_row(start=0, delay=0, frame=16, game_mode=game_mode,
                                     human=human, control=control))
    rows.append(schedule_row(start=0, delay=0, frame=16, passive=True))
    for answer in (106, 112):
        rows.append(schedule_row(start=40, delay=3, frame=50, answer=answer))
    return rows


# ---------------------------------------------------------------- strategy

def house_address(index, self_index):
    if index == self_index:
        return HOUSE
    return PEERS + PEER_SIZE * index


def install_houses(emu, peers, self_index, *, self_passive, centre, alternate):
    """HouseClass::Array: `peers` (dicts) in array order with this house
    inserted at `self_index`; returns every house address in array order."""
    count = len(peers) + 1
    addresses = [house_address(index, self_index) for index in range(count)]
    emu.write32(HOUSES, HOUSE_ARRAY)
    emu.write32(HOUSE_COUNT, count)
    byte(emu, TYPE_PASSIVE + 0x1A6, 1)
    others = iter(peers)
    for index, address in enumerate(addresses):
        emu.write32(HOUSE_ARRAY + 4 * index, address)
        emu.write32(address + 0x30, index)
        if address == HOUSE:
            emu.write32(address + 0x34, TYPE_PASSIVE if self_passive else TYPE_ACTIVE)
            cell(emu, address + 0x5490, centre)
            cell(emu, address + 0x5494, alternate)
            continue
        peer = next(others)
        emu.write32(address + 0x34, TYPE_PASSIVE if peer['passive'] else TYPE_ACTIVE)
        byte(emu, address + 0x1F5, peer['defeated'])
        cell(emu, address + 0x5490, peer['centre'])
        cell(emu, address + 0x5494, peer['alternate'])
    return addresses


def install_anger(emu, peers, addresses):
    """This house's anger nodes: every other house in array order."""
    others = [address for address in addresses if address != HOUSE]
    for slot, (address, peer) in enumerate(zip(others, peers)):
        emu.write32(ANGER_ITEMS + 8 * slot, address)
        emu.write32(ANGER_ITEMS + 8 * slot + 4, peer['anger'])
    emu.write32(HOUSE + 0x5608, ANGER_ITEMS)
    emu.write32(HOUSE + 0x5614, len(others))
    allies = 0
    for address, peer in zip(others, peers):
        if peer['ally']:
            allies |= 1 << emu.read32(address + 0x30)
    emu.write32(HOUSE + 0x5788, allies)


def install_buildings(emu, buildings):
    """House+0x68: None is a null item; otherwise alive (+0x90), limbo
    (+0x81) and whether the type has Factory= (+0xEB8)."""
    emu.write32(FACTORY_TYPE + 0xEB8, 7)
    for slot, building in enumerate(buildings):
        if building is None:
            emu.write32(BUILDING_ITEMS + 4 * slot, 0)
            continue
        address = BUILDINGS + BUILDING_SIZE * slot
        emu.write32(BUILDING_ITEMS + 4 * slot, address)
        byte(emu, address + 0x90, building['alive'])
        byte(emu, address + 0x81, building['limbo'])
        emu.write32(address + 0x520, FACTORY_TYPE if building['factory'] else PLAIN_TYPE)
    emu.write32(HOUSE + 0x6C, BUILDING_ITEMS)
    emu.write32(HOUSE + 0x78, len(buildings))


def peer(*, passive=False, defeated=False, ally=False, centre=(20, 20), alternate=(0, 0),
         anger=0):
    return dict(passive=passive, defeated=defeated, ally=ally, centre=list(centre),
                alternate=list(alternate), anger=anger)


def building(*, alive=True, limbo=False, factory=True):
    return dict(alive=alive, limbo=limbo, factory=factory)


FACTORY = building()
PLAIN = building(factory=False)


def strategy_row(label, *, game_mode=1, frame=1000, peers=(), self_index=0, self_passive=False,
                 enemy=-1, centre=(40, 40), alternate=(0, 0), mode=0, attack_frame=-5000,
                 iq=0, iq_superweapons=3, money=5000, buildings=(FACTORY,), need=0, draw=4):
    peers = list(peers)
    emu = Emu()
    emu.write32(GAME_MODE, game_mode)
    emu.write32(FRAME, frame)
    addresses = install_houses(emu, peers, self_index, self_passive=self_passive,
                               centre=centre, alternate=alternate)
    install_anger(emu, peers, addresses)
    install_buildings(emu, buildings)
    emu.write32(HOUSE + 0x24C, iq)
    emu.write32(RULES + 0x1438, iq_superweapons)
    emu.write32(HOUSE + 0x250, mode)
    emu.write32(HOUSE + 0x54D8, attack_frame)
    emu.write32(HOUSE + 0x5600, enemy)
    # The constructor's enemy-search timer (no other writer): expired.
    emu.write32(HOUSE + 0x5640, 0)
    emu.write32(HOUSE + 0x5648, 0)

    index_of = {address: index for index, address in enumerate(addresses)}
    emu.write32(HOUSE + 0x24, IHOUSE_VTABLE)
    emu.write32(IHOUSE_VTABLE + 0x18, STUB_MONEY)
    emu.write32(IHOUSE_VTABLE + 0x34, STUB_FIRE_SALE)
    emu.write32(IHOUSE_VTABLE + 0x38, STUB_ALL_TO_HUNT)

    def record(event, answer=None):
        def stub(e):
            e.events.append(list(event))
            return answer
        return stub
    emu.hook(STUB_MONEY, record(['money'], money), 4)
    emu.hook(STUB_FIRE_SALE, record(['fire_sale'], 0), 4)
    emu.hook(STUB_ALL_TO_HUNT, record(['all_to_hunt'], 0), 4)
    emu.hook(TRY_FIRE_SW, record(['try_fire_sw']), 0)
    emu.hook(CHECK_BUILD_NEED, record(['check_build_need'], need), 0)

    def manage(e):
        e.events.append(['manage', i32(e.arg(0))])
    emu.hook(MANAGE_BUILD_QUEUE, manage, 4)

    looked_up = []

    def cell_at(e):
        x, y = struct.unpack('<hh', e.uc.mem_read(e.arg(0), 4))
        looked_up.append((x, y))
        e.events.append(['cell', x, y])
        return CELL
    emu.hook(CELL_AT, cell_at, 4)
    emu.write32(CELL, CELL_VTABLE)
    emu.write32(CELL_VTABLE + 0x48, STUB_COORD)

    def coord(e):
        x, y = looked_up[-1]
        out = e.arg(0)
        e.uc.mem_write(out, struct.pack('<iii', x * 256 + 128, y * 256 + 128, 0))
        return out
    emu.hook(STUB_COORD, coord, 4)

    def anger(uc, _address, _size, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        delta = i32(emu.read32(sp + 4))
        house = emu.read32(sp + 8)
        if uc.reg_read(UC_X86_REG_ECX) != HOUSE or house not in index_of:
            raise OracleError('UpdateAngerNodes reached with a foreign house')
        emu.events.append(['anger', delta, index_of[house]])
    emu.uc.hook_add(UC_HOOK_CODE, anger, begin=ANGER, end=ANGER)
    emu.draws([draw])

    delay = i32(emu.invoke(STRATEGY, ecx=HOUSE))
    return dict(label=label, game_mode=game_mode, frame=frame, peers=peers,
                self_index=self_index, self_passive=self_passive, enemy=enemy,
                centre=list(centre), alternate=list(alternate), mode=mode,
                attack_frame=attack_frame, iq=iq, iq_superweapons=iq_superweapons,
                money=money, buildings=list(buildings), need=need, events=emu.events,
                delay=delay, mode_after=emu.read_i32(HOUSE + 0x250),
                enemy_after=emu.read_i32(HOUSE + 0x5600),
                anger_after=[emu.read_i32(ANGER_ITEMS + 8 * slot + 4)
                             for slot in range(len(peers))])


def strategy():
    rows = []
    four = [peer(), peer(), peer(), peer()]
    # Enemy search: the first eligible house wins whatever its distance.
    rows.append(strategy_row('first_eligible', peers=[
        peer(centre=(90, 90)), peer(centre=(41, 40)), peer(centre=(5, 5))]))
    rows.append(strategy_row('self_in_the_middle', self_index=2, peers=[
        peer(centre=(90, 90)), peer(centre=(41, 40)), peer(centre=(5, 5))]))
    rows.append(strategy_row('skips_passive_and_defeated', peers=[
        peer(passive=True), peer(defeated=True), peer(passive=True, defeated=True),
        peer(centre=(90, 90)), peer(centre=(41, 40))]))
    rows.append(strategy_row('no_eligible_house', peers=[
        peer(passive=True), peer(defeated=True)]))
    rows.append(strategy_row('alone', peers=[]))
    rows.append(strategy_row('ally_first_gets_anger_but_no_enemy', peers=[
        peer(ally=True), peer()]))
    rows.append(strategy_row('ally_first_with_an_angry_rival', peers=[
        peer(ally=True), peer(anger=5)]))
    rows.append(strategy_row('first_eligible_already_angry', peers=[
        peer(anger=3), peer(anger=7)]))
    rows.append(strategy_row('alternate_centre', alternate=(12, 30), peers=four))
    rows.append(strategy_row('no_centre', centre=(0, 0), alternate=(0, 0), peers=four))
    rows.append(strategy_row('alternate_only', centre=(0, 0), alternate=(7, 9), peers=four))
    rows.append(strategy_row('campaign_skips_search', game_mode=0, peers=four))
    rows.append(strategy_row('passive_self_skips_search', self_passive=True, peers=four))
    rows.append(strategy_row('enemy_already_set', enemy=2, peers=[
        peer(anger=4), peer(anger=9), peer()]))
    # The defeated enemy's anger is cancelled and the enemy forgotten.
    rows.append(strategy_row('defeated_enemy', enemy=2, peers=[
        peer(anger=4), peer(defeated=True, anger=9), peer(anger=2)]))
    rows.append(strategy_row('defeated_enemy_zero_anger', enemy=1, peers=[
        peer(defeated=True), peer(anger=6)]))
    rows.append(strategy_row('defeated_enemy_negative_anger', enemy=1, peers=[
        peer(defeated=True, anger=-3), peer(anger=6)]))
    rows.append(strategy_row('defeated_enemy_in_campaign', game_mode=0, enemy=1, peers=[
        peer(defeated=True, anger=5), peer(anger=2)]))
    # The superweapon gate.
    for game_mode, iq in ((0, 2), (0, 3), (0, 4), (1, 0)):
        rows.append(strategy_row(f'superweapon_gate_mode{game_mode}_iq{iq}',
                                 game_mode=game_mode, iq=iq, iq_superweapons=3, peers=four))
    # The emergency block.
    for mode in (0, 1, 2, 3, 4, 5, -1):
        for money in (24, 25):
            for attack in (-5000, 100, 99, 101, 150):
                rows.append(strategy_row(f'emergency_{mode}_{money}_{attack}', mode=mode,
                                         money=money, attack_frame=attack, frame=1000,
                                         peers=four))
    rows.append(strategy_row('attack_deadline_wraps', mode=0, attack_frame=0x7FFFFFFF - 100,
                             frame=-0x7FFFFFFF + 700, peers=four))
    # The no-factory urgency and its dispatch.
    factories = {
        'no_buildings': (),
        'plain_only': (PLAIN, PLAIN),
        'live_factory': (PLAIN, FACTORY),
        'limbo_factory': (building(limbo=True), PLAIN),
        'dead_factory': (building(alive=False), PLAIN),
        'null_then_factory': (None, FACTORY),
    }
    for name, buildings in factories.items():
        for mode in (0, 3, 4):
            for game_mode in (0, 1):
                rows.append(strategy_row(f'urgency_{name}_mode{mode}_game{game_mode}',
                                         buildings=buildings, mode=mode, game_mode=game_mode,
                                         attack_frame=900 if mode == 3 else -5000,
                                         peers=four))
    for need in (0, 1, 2, 3, 4, -1):
        for buildings in ((FACTORY,), (PLAIN,)):
            rows.append(strategy_row(f'build_need_{need}_{len(buildings)}', need=need,
                                     buildings=buildings, peers=four))
    for draw in range(1, 8):
        rows.append(strategy_row(f'draw_{draw}', draw=draw, peers=four))
    # Seeded mixtures of every input above.
    generator = random.Random(0x4FD500)
    for case in range(40):
        count = generator.randint(0, 5)
        peers = [peer(passive=generator.random() < 0.2, defeated=generator.random() < 0.2,
                      ally=generator.random() < 0.3, anger=generator.choice((0, 0, 1, 3, 8, -2)),
                      centre=(generator.randint(0, 60), generator.randint(0, 60)))
                 for _ in range(count)]
        self_index = generator.randint(0, count)
        enemy = -1
        if count and generator.random() < 0.4:
            enemy = generator.choice([index for index in range(count + 1) if index != self_index])
        buildings = tuple(generator.choice((FACTORY, PLAIN, building(limbo=True),
                                            building(alive=False), None))
                          for _ in range(generator.randint(0, 3)))
        rows.append(strategy_row(
            f'mixed_{case}', game_mode=generator.choice((0, 1, 1)), peers=peers,
            self_index=self_index, enemy=enemy,
            centre=generator.choice(((40, 40), (0, 0))),
            alternate=generator.choice(((0, 0), (0, 0), (8, 3))),
            mode=generator.choice((0, 1, 3, 4)), money=generator.choice((0, 24, 25, 900)),
            attack_frame=generator.choice((-5000, 150, 50)), frame=1000,
            iq=generator.randint(0, 5), buildings=buildings, need=generator.choice((0, 1)),
            draw=generator.randint(1, 7)))
    return rows


# ---------------------------------------------------------------- fire_sale

def fire_sale_row(label, *, buildings, current=None):
    """`buildings`: None (null item) or (limbo, health)."""
    emu = Emu()
    count = sum(1 for entry in buildings if entry is not None) if current is None else current
    emu.write32(HOUSE + 0x2F0, count)
    emu.write32(HOUSE + 0x6C, BUILDING_ITEMS)
    emu.write32(HOUSE + 0x78, len(buildings))
    emu.write32(OBJECT_VTABLE + 0x1A0, STUB_SELL_BACK)
    slots = {}
    for slot, entry in enumerate(buildings):
        if entry is None:
            emu.write32(BUILDING_ITEMS + 4 * slot, 0)
            continue
        limbo, health = entry
        address = BUILDINGS + BUILDING_SIZE * slot
        slots[address] = slot
        emu.write32(BUILDING_ITEMS + 4 * slot, address)
        emu.write32(address, OBJECT_VTABLE)
        byte(emu, address + 0x81, limbo)
        emu.write32(address + 0x6C, health)

    def sell_back(e):
        e.events.append(['sell', slots[e.uc.reg_read(UC_X86_REG_ECX)], i32(e.arg(0))])
        return 1
    emu.hook(STUB_SELL_BACK, sell_back, 4)
    result = emu.invoke(FIRE_SALE, args=(HOUSE + 0x24,))
    return dict(label=label, current=count,
                buildings=[None if entry is None else list(entry) for entry in buildings],
                events=emu.events, result=i32(result))


def fire_sale():
    rows = [
        fire_sale_row('sells_each_live_building', buildings=[(False, 100), (False, 1), (False, 5)]),
        fire_sale_row('skips_limbo_dead_and_null',
                      buildings=[(True, 100), (False, 0), None, (False, -4), (False, 30)]),
        fire_sale_row('no_current_buildings', buildings=[(False, 100)], current=0),
        fire_sale_row('negative_current_buildings', buildings=[(False, 100)], current=-1),
        fire_sale_row('current_count_ignores_the_list', buildings=[], current=3),
        fire_sale_row('empty', buildings=[]),
    ]
    return rows


# ---------------------------------------------------------------- all_to_hunt

def techno(*, owner=True, down=True, limbo=False, permanent=False, insignificant=False,
           foot=True, team=False, kind=None, occupants=0, strength=300):
    return dict(owner=owner, down=down, limbo=limbo, permanent=permanent,
                insignificant=insignificant, foot=foot, team=team,
                kind=kind if kind is not None else (1 if foot else BUILDING_KIND),
                occupants=occupants, strength=strength)


def all_to_hunt_row(label, *, technos, human=False):
    emu = Emu()
    byte(emu, HOUSE + 0x1EC, human)
    emu.write32(RULES + 0xFA8, C4_WARHEAD)
    emu.write32(TECHNOS, TECHNO_ITEMS)
    emu.write32(TECHNO_COUNT, len(technos))
    other_house = PEERS
    for slot, entry in enumerate(technos):
        address = OBJECTS + OBJECT_SIZE * slot
        kind_type = OBJECT_TYPES + OBJECT_TYPE_SIZE * slot
        emu.write32(TECHNO_ITEMS + 4 * slot, address)
        emu.write32(address, OBJECT_VTABLE)
        emu.write32(address + 0x14, 4 if entry['foot'] else 0)
        emu.write32(address + 0x21C, HOUSE if entry['owner'] else other_house)
        byte(emu, address + 0x74, entry['down'])
        byte(emu, address + 0x81, entry['limbo'])
        byte(emu, address + 0x2C4, entry['permanent'])
        emu.write32(address + 0x5D4, 0x7EA70000 + slot if entry['team'] else 0)
        emu.write32(address + OBJECT_TYPE_SLOT, kind_type)
        emu.write32(address + OBJECT_KIND_SLOT, entry['kind'])
        emu.write32(address + OBJECT_OCCUPANTS_SLOT, entry['occupants'])
        emu.write32(kind_type + 0xA0, entry['strength'])
        byte(emu, kind_type + 0x232, entry['insignificant'])
    for offset, stub in ((0x2C, STUB_WHAT_AM_I), (0x84, STUB_TECHNO_TYPE),
                         (0x16C, STUB_RECEIVE_DAMAGE), (0x1E8, STUB_QUEUE_MISSION),
                         (0x408, STUB_OCCUPANTS)):
        emu.write32(OBJECT_VTABLE + offset, stub)

    def slot_of(address):
        slot, rest = divmod(address - OBJECTS, OBJECT_SIZE)
        if rest or not 0 <= slot < len(technos):
            raise OracleError(f'call on a foreign object 0x{address:08X}')
        return slot

    def this(e):
        return e.uc.reg_read(UC_X86_REG_ECX)

    emu.hook(STUB_WHAT_AM_I, lambda e: e.read32(this(e) + OBJECT_KIND_SLOT), 0)
    emu.hook(STUB_TECHNO_TYPE, lambda e: e.read32(this(e) + OBJECT_TYPE_SLOT), 0)
    emu.hook(STUB_OCCUPANTS, lambda e: e.read32(this(e) + OBJECT_OCCUPANTS_SLOT), 0)

    def receive_damage(e):
        args = [e.arg(index) for index in range(7)]
        e.events.append(['damage', slot_of(this(e)), i32(e.read32(args[0]))]
                        + [i32(value) for value in args[1:]])
        return 0
    emu.hook(STUB_RECEIVE_DAMAGE, receive_damage, 0x1C)

    def queue_mission(e):
        e.events.append(['mission', slot_of(this(e)), i32(e.arg(0)), i32(e.arg(1))])
        return 1
    emu.hook(STUB_QUEUE_MISSION, queue_mission, 8)

    def team_remove(e):
        e.events.append(['team_remove', slot_of(e.arg(0)), i32(e.arg(1)), i32(e.arg(2))])
        return 1
    emu.hook(TEAM_REMOVE, team_remove, 0xC)

    def release(e):
        e.events.append(['release', slot_of(this(e)), i32(e.arg(0)), i32(e.arg(1))])
    emu.hook(GARRISON_RELEASE, release, 8)

    result = emu.invoke(ALL_TO_HUNT, args=(HOUSE + 0x24,))
    return dict(label=label, human=human, technos=technos, events=emu.events,
                result=i32(result), latch=bool(read_byte(emu, HOUSE + 0x249)),
                c4_warhead=C4_WARHEAD)


def all_to_hunt():
    mixed = [
        techno(),
        techno(owner=False),
        techno(down=False),
        techno(limbo=True),
        techno(team=True),
        techno(foot=False, occupants=2),
        techno(foot=False, occupants=0),
        techno(foot=False, kind=3, occupants=4),
        techno(permanent=True, insignificant=True, strength=125),
        techno(permanent=True, insignificant=False),
        techno(permanent=True, insignificant=True, foot=False, occupants=1, strength=900),
    ]
    return [
        all_to_hunt_row('mixed_computer', technos=mixed),
        all_to_hunt_row('mixed_human', technos=mixed, human=True),
        all_to_hunt_row('empty', technos=[]),
        all_to_hunt_row('only_foreign', technos=[techno(owner=False), techno(owner=False)]),
    ]


def generate():
    return {'source': 'unicorn/gamemd.exe', 'schedule': schedule(), 'strategy': strategy(),
            'fire_sale': fire_sale(), 'all_to_hunt': all_to_hunt()}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=('computer strategy tick: the HouseClass::Update Strategy block, whole '
               'AI_Building_Strategy runs, IHouse Fire_Sale and IHouse All_To_Hunt'),
        assumptions=['fresh emulator per case; fixture House/Rules/Type layouts from live disassembly',
                     'x87 control word 0x0E7F (53-bit chop), the process word ftol callers assume',
                     'every anger node belongs to another house, in HouseClass::Array order'],
        substitutions=['AI_Building_Strategy 0x4FD500 answers a delay in the schedule section',
                       'RandomRanged 0x65C7E0 answers the row draw',
                       'MapClass::operator[] 0x5657A0 answers one cell whose vt+0x48 coordinate is (x*256+128, y*256+128, 0)',
                       'AI_TryFireSW 0x5098F0, Check_Build_Need 0x4FD9A0 and Manage_Build_Queue 0x4FDD10 are recorded stubs',
                       'IHouse Available_Money, Fire_Sale and All_To_Hunt are recorded stubs in the strategy section',
                       'Sell_Back, the object type query, WhatAmI, the occupant count, ReceiveDamage, Queue_Mission, Team remove 0x6EA870 and the garrison release 0x457DE0 are recorded stubs',
                       'operator new/delete and atexit are fixture stubs'],
        entry_points={'HouseClass_Update_strategy_block': UPDATE_BLOCK,
                      'AI_Building_Strategy': STRATEGY, 'UpdateAngerNodes': ANGER,
                      'IHouse_Fire_Sale': FIRE_SALE, 'IHouse_All_To_Hunt': ALL_TO_HUNT}))
