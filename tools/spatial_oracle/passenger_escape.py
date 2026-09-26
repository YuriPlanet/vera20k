"""Original passenger escape of a dying Unit (UnitClass::ReceiveDamage 0x737F80..0x7381BC).

Run python -m tools.spatial_oracle.passenger_escape --check (or --write).

On the refinery_dock fixture (the real Unit vtable over a constructed Drive on
the 32x32 map, House, Rules and the Scenario RNG) the transport is the
fixture's Unit, absent from its cell's lists and occupation bits as its
Mark(UP) (0x737F74) leaves it. Its Infantry passengers (the real Infantry
vtable over a constructed Walk locomotor) sit in limbo as InfantryClass::Limbo
(0x51DF10) leaves a boarded man (Doing 0, not prone), chained through +0x30
from the cargo head +0x118 (count +0x114), each with its Transporter +0x11C.
Passenger 0 is the head, the last to board.

The block runs from 0x737F80 to the crew block 0x7381BC with the
ReceiveDamage frame supplied: ESI the transport, EBX and [esp+0x54] the
attacker, [esp+0x58] IgnoreDefenses and [esp+0x13] the selected-by-the-player
local (0x737C98..0x737CB6). Original bytes run for ClearAllInOpenTransport
(0x7104C0), GetHeight (0x5F5F40), KillPassengers (0x707CB0),
RemoveFirstPassenger (0x4DE710), the map lookup (0x565730),
InfantryClass::Can_Enter_Cell (0x51BF90), GetCell (0x5F6960),
CellClass::GetCoords (0x486840), FacingClass::Current (0x4C93D0),
InfantryClass::Unlimbo (0x51DFF0) with PlaceInfantryInCell (0x481180) and the
occupation mark (0x5217C0), InfantryClass::Scatter (0x51D0D0) with its
Scenario draw and Find_Nearby_Passable_Cell (0x56DC20), IsControlledByHuman
(0x50B730) and Queue_Mission (0x5B35E0). The Infantry startup initialisers
(table 0x813490) set the 416-lepton threshold at 0xA8F234.

Observed at entry and answered (see `substitutions` in the meta file):
RecordKill 0x702D40, FootClass::UnInit 0x4DE5D0, FootClass::Unlimbo 0x4D7170,
the Unit's RemoveGunner 0x7464E0, InfantryClass::Assign_Target 0x51B1F0,
Select 0x6FBFA0, TeamClass::Add_Member 0x6EA500, the Infantry destination
setter 0x51AA40 and the Walk Process slot 0x75AC80.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle.infantry_entry_raw import STARTUP
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import ACTOR, HOUSE, TYPE, cell, cell_xy, make_dock_fixture
from tools.spatial_oracle.unit_scatter_state import SP
from tools.spatial_oracle.unit_source_scatter import SCENARIO

START, END = 0x737F80, 0x7381BC
RECORD_KILL, UNINIT, FOOT_UNLIMBO, REMOVE_GUNNER = 0x702D40, 0x4DE5D0, 0x4D7170, 0x7464E0
ASSIGN_TARGET, SELECT, TEAM_ADD, TEAM_REMOVE = 0x51B1F0, 0x6FBFA0, 0x6EA500, 0x6EA870
FNPC, SETTER, WALK_PROCESS, RANDOM = 0x56DC20, 0x51AA40, 0x75AC80, 0x65C7E0
KILL_PASSENGERS, CAN_ENTER, SCATTER, UNLIMBO, PLACE = 0x707CB0, 0x51BF90, 0x51D0D0, 0x51DFF0, 0x481180
REMOVE_FIRST, CLEAR_OPEN, HEIGHT, QUEUE = 0x4DE710, 0x7104C0, 0x5F5F40, 0x5B35E0
WALK_CTOR, FACING_CTOR, FACING_ROT = 0x75AA90, 0x4C91C0, 0x4C9680
# A region of its own: the passengers and the cell's other infantry (each the
# object, then its Walk at +0x1000), the passengers' type, a second house,
# the attacker and a team.
REGION = 0x23000000
PAX_BASE, OCC_BASE, SLOT = REGION, REGION + 0x10000, 0x2000
ITYPE, FOREIGN, ATTACKER, TEAM = (REGION + 0x20000 + n for n in (0, 0x2000, 0x8000, 0xA000))
COORD, MAP, FLOOR = REGION + 0x2F000, 0x87F7E8, 0x578080
SPOT_OFFSETS = [(128, 128, 0), (64, 64, 0), (192, 64, 0), (64, 192, 0), (192, 192, 0)]
MISSION_GUARD, MISSION_HUNT = 5, 15
LAND_WATER = 2


def pax(index):
    return PAX_BASE + index * SLOT


def occupant(index):
    return OCC_BASE + index * SLOT


def name_of(pointer):
    if pointer == 0:
        return None
    if pointer == ATTACKER:
        return 'attacker'
    if pointer == ACTOR:
        return 'transport'
    if PAX_BASE <= pointer < OCC_BASE:
        return (pointer - PAX_BASE) // SLOT
    if OCC_BASE <= pointer < ITYPE:
        return f'occupant{(pointer - OCC_BASE) // SLOT}'
    return hex(pointer)


def make_infantry(u, call, address, owner, coord, limbo):
    """An Infantry as its constructor and a boarding Limbo leave it: Doing 0,
    on Guard, full Strength, a settled facing and a Walk locomotor."""
    loco = address + 0x1000
    u.mem_write(address, dwords(0x7EB058, 0x7EB03C, 0x7EB034, 0x7EB02C))
    u.mem_write(address + 0x14, dwords(5))
    u.mem_write(address + 0x21C, dwords(owner))
    u.mem_write(address + 0x6C0, dwords(ITYPE))
    u.mem_write(address + 0x6C4, dwords(0))
    u.mem_write(address + 0x6C, dwords(125, 125))
    u.mem_write(address + 0x90, b'\x01')
    u.mem_write(address + 0x94, dwords(-1))
    u.mem_write(address + 0x74, bytes([not limbo]))
    u.mem_write(address + 0x81, bytes([limbo]))
    u.mem_write(address + 0x9C, dwords(*coord))
    u.mem_write(address + 0xAC, dwords(MISSION_GUARD))
    u.mem_write(address + 0xB4, dwords(-1))
    u.mem_write(address + 0x684, b'\xff')
    call(FACING_CTOR, address + 0x388, [])
    call(FACING_ROT, address + 0x388, [8])
    call(WALK_CTOR, loco, [])
    u.mem_write(loco + 0xC, dwords(address))
    u.mem_write(address + 0x674, dwords(loco + 4))


def make_fixture(case):
    x, y = case.get('cell', [15, 15])
    u, call, read32 = make_dock_fixture(dict(
        miner_cell=[x, y], mission='guard', linked=False, seed=case.get('seed', 31),
        frame=case.get('frame', 200), facing=case.get('facing', 0xC000),
        human=case.get('human', False)))
    u.mem_map(REGION, 0x30000)
    # The Infantry startup initialisers: the Unlimbo occupation threshold
    # (0xA8F234, 416 leptons) among them.
    assert list(struct.unpack('<10I', u.mem_read(0x813490, 40))) == STARTUP
    for entry in STARTUP:
        call(entry, 0, [])
    assert read32(0xA8F234) == 416
    # LevelHeight 104 (the cell ground query 0x0047B3A0 caches it on first
    # use, so nothing may have queried a height yet) and the 416-lepton
    # bridge deck (GetHeight 0x005F5F40 subtracts 0xAC13BC on a bridge).
    assert u.mem_read(0x89E770, 1)[0] & 7 == 0
    for address, value in [(0x89E7C0, 104), (0xAC13C8, 104), (0xAC13BC, 416), (0xABC5DC, 416),
                           (0x8B3CAC, 416)]:
        u.mem_write(address, dwords(value))
    # PlaceInfantryInCell's spot offsets (0x89E9F0) and its no-spot answer
    # (0x89E778), and the empty coordinate Unlimbo and Scatter compare
    # against (0xA8F200), filled by static initialisers the fixture does not run.
    u.mem_write(0x89E778, dwords(0, 0, 0))
    u.mem_write(0xA8F200, dwords(0, 0, 0))
    u.mem_write(0x89E9F0, dwords(*(v for spot in SPOT_OFFSETS for v in spot)))
    # Water: no SpeedType crosses it (ground speed row LandType 2).
    u.mem_write(0x89EA40 + LAND_WATER * 9 * 4, struct.pack('<9f', *[0.0] * 9))
    # A bridge runs east-west through the row's cell (cell flag 0x100);
    # water lies under it, or under the row's cell alone.
    here = cell(x, y)
    bridge = [(x - 1, y), (x, y), (x + 1, y)] if case.get('bridge') else []
    for bx, by in bridge:
        u.mem_write(cell(bx, by) + 0x140, dwords(0x100))
    if case.get('water'):
        for wx, wy in bridge or [(x, y)]:
            u.mem_write(cell(wx, wy) + 0xEC, dwords(LAND_WATER))
    if 'slope' in case:
        u.mem_write(here + 0x11C, bytes([case['slope']]))
    # The passengers' type: InfantryType vtable, Strength 125, MovementZone
    # Infantry, SpeedType Foot, unarmed.
    u.mem_write(ITYPE, dwords(0x7EB610))
    u.mem_write(ITYPE + 0xA0, dwords(125))
    u.mem_write(ITYPE + 0x5B4, dwords(7))
    u.mem_write(ITYPE + 0x67C, dwords(0))
    # The transport: its type's OpenTopped (+0x5E4), Gunner (+0x805) and
    # Crashable (+0xD95), its Location, OnBridge (+0x8C), IsFallingDown
    # (+0x8F) and Team (+0x5D4).
    u.mem_write(TYPE + 0x5E4, bytes([case.get('open_topped', False)]))
    u.mem_write(TYPE + 0x805, bytes([case.get('gunner', False)]))
    u.mem_write(TYPE + 0xD95, bytes([case.get('crashable', False)]))
    # The transport stands on the ground under its XY (MapClass floor
    # 0x578080), or on the bridge deck 416 leptons above it, plus the row's
    # height.
    sub = case.get('sub', [128, 128])
    u.mem_write(COORD, dwords(x * 256 + sub[0], y * 256 + sub[1], 0))
    call(FLOOR, MAP, [COORD])
    floor = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
    z = floor + (416 if case.get('on_bridge') else 0) + case.get('height', 0)
    u.mem_write(ACTOR + 0x9C, dwords(x * 256 + sub[0], y * 256 + sub[1], z))
    u.mem_write(ACTOR + 0x8C, bytes([case.get('on_bridge', False)]))
    u.mem_write(ACTOR + 0x8F, bytes([case.get('falling', False)]))
    u.mem_write(ACTOR + 0x5D4, dwords(TEAM if case.get('team') else 0))
    # A second house (ArrayIndex 1), allied both ways only when the row says;
    # the fixture's House is index 0.
    u.mem_write(FOREIGN + 0x30, dwords(1))
    u.mem_write(FOREIGN + 0x1EC, bytes([case.get('foreign_human', False)]))
    if case.get('foreign_allied'):
        u.mem_write(HOUSE + 0x5788, dwords(0b10))
        u.mem_write(FOREIGN + 0x5788, dwords(0b01))
    # The cargo: head +0x118, count +0x114.
    passengers = case.get('passengers', [{}])
    for index, spec in enumerate(passengers):
        address = pax(index)
        owner = FOREIGN if spec.get('foreign') else HOUSE
        make_infantry(u, call, address, owner, [x * 256 + 128, y * 256 + 128, 0], limbo=True)
        u.mem_write(address + 0x11C, dwords(ACTOR))
        u.mem_write(address + 0x82, bytes([case.get('open_topped', False)]))
        u.mem_write(address + 0x30, dwords(pax(index + 1) if index + 1 < len(passengers) else 0))
    u.mem_write(ACTOR + 0x114, dwords(len(passengers), pax(0) if passengers else 0))
    # Infantry already standing in the cell, each in its spot: listed in the
    # cell (+0xE4 through +0x30) with its spot bit and owner in the ground
    # occupation (+0x124, owner +0x54), as the occupation mark leaves them.
    bits, head = 0, 0
    for index, spec in reversed(list(enumerate(case.get('occupants', [])))):
        address = occupant(index)
        spot = spec['spot']
        owner = FOREIGN if spec.get('foreign') else HOUSE
        offset = SPOT_OFFSETS[spot]
        make_infantry(u, call, address, owner, [x * 256 + offset[0], y * 256 + offset[1], 0], limbo=False)
        u.mem_write(address + 0x30, dwords(head))
        head = address
        bits |= 1 << spot
        u.mem_write(here + 0x54, dwords(read32(owner + 0x30)))
    if head:
        u.mem_write(here + 0xE4, dwords(head))
        u.mem_write(here + 0x124, bytes([bits]))
    return u, call, read32


def observe(u, read32, case, events):
    returns = {}

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def coord(pointer):
        return list(struct.unpack('<3i', u.mem_read(pointer, 12)))

    def pair(pointer):
        return list(struct.unpack('<hh', u.mem_read(pointer, 4)))

    def on_return(finish):
        # Completes the event just appended when its call returns.
        returns[read32(u.reg_read(UC_X86_REG_ESP))] = (len(events) - 1, finish)

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        if address in returns:
            index, finish = returns.pop(address)
            events[index].append(finish(u.reg_read(UC_X86_REG_EAX)))
        if address == RANDOM:
            events.append(['random', read32(sp + 4), read32(sp + 8)])
            on_return(lambda value: value)
        elif address == HEIGHT and this == ACTOR:
            events.append(['height'])
            on_return(lambda value: struct.unpack('<i', dwords(value))[0])
        elif address == CLEAR_OPEN:
            events.append(['clear_all_in_open_transport', name_of(this)])
        elif address == KILL_PASSENGERS:
            events.append(['kill_passengers', name_of(this), name_of(read32(sp + 4))])
        elif address == REMOVE_FIRST:
            events.append(['remove_first_passenger'])
            on_return(name_of)
        elif address == CAN_ENTER:
            events.append(['can_enter', name_of(this), cell_xy(read32(sp + 4)),
                           *[struct.unpack('<i', dwords(read32(sp + 8 + 4 * i)))[0] for i in range(4)]])
            on_return(lambda value: value)
        elif address == UNLIMBO:
            events.append(['unlimbo', name_of(this), coord(read32(sp + 4)), read32(sp + 8)])
            on_return(lambda value: value & 0xFF)
        elif address == PLACE:
            events.append(['place_infantry_in_cell', cell_xy(this), coord(read32(sp + 8)), read32(sp + 12) & 0xFF])
            on_return(coord)
        elif address == FOOT_UNLIMBO:
            # FootClass::Unlimbo's placement, without its facing, cell list,
            # sight reveal, layer submission and Enter_Idle_Mode: the
            # coordinate, out of limbo, on the map.
            placed = coord(read32(sp + 4))
            events.append(['foot_unlimbo', name_of(this), placed, read32(sp + 8)])
            u.mem_write(this + 0x9C, dwords(*placed))
            u.mem_write(this + 0x81, b'\x00')
            u.mem_write(this + 0x74, b'\x01')
            ret(8, 1)
        elif address == RECORD_KILL:
            events.append(['record_kill', name_of(this), name_of(read32(sp + 4))])
            ret(4)
        elif address == UNINIT:
            events.append(['uninit', name_of(this)])
            ret(0)
        elif address == REMOVE_GUNNER:
            events.append(['remove_gunner', name_of(this), name_of(read32(sp + 4))])
            ret(4)
        elif address == ASSIGN_TARGET:
            events.append(['assign_target', name_of(this), read32(sp + 4)])
            ret(4)
        elif address == SELECT:
            events.append(['select', name_of(this)])
            ret(0, 1)
        elif address == TEAM_ADD:
            events.append(['team_add', name_of(read32(sp + 4)), read32(sp + 8)])
            ret(8, 1)
        elif address == TEAM_REMOVE:
            events.append(['team_remove', name_of(read32(sp + 4))])
            ret(12)
        elif address == SCATTER:
            events.append(['scatter', name_of(this), hex(read32(sp + 4)), read32(sp + 8) & 0xFF,
                           read32(sp + 12) & 0xFF])
        elif address == FNPC:
            # Out, seed, SpeedType, zone, MovementZone, bridge-aware, W, H,
            # reject-overlay, height gate, obstacle gate, allow bridge, the
            # reference cell, quadrant skip, occupancy.
            args = [read32(sp + 4 * i) for i in range(1, 16)]
            signed = lambda value: struct.unpack('<i', dwords(value))[0]
            events.append(['nearby_passable_cell', pair(args[1]), *[signed(v) for v in args[2:12]],
                           pair(args[12]), args[13], args[14]])
            on_return(pair)
        elif address == QUEUE:
            events.append(['queue_mission', name_of(this), read32(sp + 4)])
        elif address == SETTER:
            target = read32(sp + 4)
            events.append(['set_destination', name_of(this), cell_xy(target), read32(sp + 8) & 0xFF])
            u.mem_write(this + 0x5A4, dwords(target))
            ret(8)
        elif address == WALK_PROCESS:
            events.append(['process', name_of(read32(read32(sp + 4) + 8))])
            ret(4, 0)

    u.hook_add(UC_HOOK_CODE, hook)


def run(case):
    u, call, read32 = make_fixture(case)
    events = []
    observe(u, read32, case, events)
    attacker = ATTACKER if case.get('attacker', True) else 0
    frame = SP - 0x200
    u.mem_write(frame, bytes(0x80))
    u.mem_write(frame + 0x13, bytes([case.get('selected', False)]))
    u.mem_write(frame + 0x54, dwords(attacker))
    u.mem_write(frame + 0x58, bytes([case.get('ignore_defenses', False)]))
    u.reg_write(UC_X86_REG_ESI, ACTOR)
    u.reg_write(UC_X86_REG_EBX, attacker)
    u.reg_write(UC_X86_REG_ESP, frame)
    run_checked(u, START, END, count=5_000_000, required_addresses=[START])
    assert u.reg_read(UC_X86_REG_ESP) == frame, 'the block leaves its frame balanced'
    return u, read32, events


def state(u, read32, case):
    signed = lambda address: struct.unpack('<i', u.mem_read(address, 4))[0]
    passengers = []
    for index in range(len(case.get('passengers', [{}]))):
        address = pax(index)
        passengers.append(dict(
            limbo=u.mem_read(address + 0x81, 1)[0],
            coord=list(struct.unpack('<3i', u.mem_read(address + 0x9C, 12))),
            on_bridge=u.mem_read(address + 0x8C, 1)[0], transporter=name_of(read32(address + 0x11C)),
            in_open_topped=u.mem_read(address + 0x82, 1)[0],
            mission=signed(address + 0xAC), queued=signed(address + 0xB4),
            nav=cell_xy(read32(address + 0x5A4))))
    here = cell(*case.get('cell', [15, 15]))
    return dict(passengers=passengers, cargo=[signed(ACTOR + 0x114), name_of(read32(ACTOR + 0x118))],
                scenario_init=signed(0xA8E7AC),
                occupation=dict(ground=u.mem_read(here + 0x124, 1)[0], deck=u.mem_read(here + 0x128, 1)[0],
                                owners=[signed(here + 0x54), signed(here + 0x58)]),
                random_indices=[read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)])


def query(case):
    u, read32, events = run(case)
    return dict(input=case, events=events, state=state(u, read32, case))


def cases():
    five = [{}] * 5
    three_allied = [dict(spot=2), dict(spot=3), dict(spot=4)]
    return [
        # One passenger, computer and human; the scatter's FNPC picks a
        # neighbour by frame (200 and 203).
        dict(name='computer_one'),
        dict(name='human_one', human=True),
        dict(name='human_one_frame_203', human=True, frame=203),
        # A full Battle Fortress, last boarded first.
        dict(name='computer_five', passengers=five),
        # The kill gates: IgnoreDefenses, a falling transport, a cell the
        # passengers cannot enter (water; three stationary allied infantry;
        # an enemy infantryman), and no attacker.
        dict(name='ignore_defenses', passengers=[{}] * 3, ignore_defenses=True),
        dict(name='ignore_defenses_no_attacker', passengers=[{}] * 2, ignore_defenses=True, attacker=False),
        dict(name='falling', passengers=[{}] * 2, falling=True),
        dict(name='water', passengers=[{}] * 2, water=True),
        dict(name='three_allied_standing', passengers=[{}] * 2, occupants=three_allied),
        dict(name='two_allied_standing', passengers=[{}] * 2, occupants=three_allied[:2]),
        dict(name='enemy_standing', passengers=[{}] * 2, occupants=[dict(spot=2, foreign=True)]),
        dict(name='enemy_standing_allied', passengers=[{}] * 2,
             occupants=[dict(spot=2, foreign=True)], foreign_allied=True),
        # Above 0xD0 leptons every passenger dies with the attacker credited
        # (KillPassengers), for any Unit; at 0xD0 they escape.
        dict(name='high', passengers=[{}] * 2, height=0xD1),
        dict(name='at_threshold', passengers=[{}] * 2, height=0xD0),
        # A Crashable= type skips the loop; its passengers stay aboard.
        dict(name='crashable', passengers=[{}] * 2, crashable=True),
        dict(name='crashable_high', passengers=[{}] * 2, crashable=True, height=0xD1),
        # An IFV: the emptying pop hands the gunner's weapon back.
        dict(name='gunner', passengers=[{}] * 2, gunner=True),
        dict(name='gunner_ignore_defenses', passengers=[{}] * 2, gunner=True, ignore_defenses=True),
        # Open-topped: every passenger's +0x82 is cleared first, and a
        # passenger of another house drops its target.
        dict(name='open_topped', passengers=[{}] * 2, open_topped=True),
        dict(name='open_topped_foreign', passengers=[dict(foreign=True), {}], open_topped=True),
        dict(name='open_topped_foreign_allied', passengers=[dict(foreign=True), {}], open_topped=True,
             foreign_allied=True),
        # The player's selected transport selects its escapees; a team takes
        # a computer passenger instead of Hunt.
        dict(name='selected', passengers=[{}] * 2, human=True, selected=True),
        dict(name='team', passengers=[{}] * 2, team=True),
        # The escapee's facing comes from the transport's body facing.
        dict(name='facing_north', facing=0x0000),
        dict(name='facing_0x1234', facing=0x1234),
        dict(name='facing_0xFF80', facing=0xFF80),
        # Off the cell centre: the priority spot of the transport's quadrant.
        dict(name='north_east', passengers=[{}] * 2, sub=[200, 40]),
        dict(name='south_west', passengers=[{}] * 2, sub=[40, 200]),
        dict(name='north_west', passengers=[{}] * 2, sub=[40, 40]),
        # A ramp: the cell centre's height is not the floor under the
        # transport, so Unlimbo keeps the exact coordinate.
        dict(name='ramp_off_centre', passengers=[{}] * 2, slope=1, sub=[200, 40]),
        dict(name='ramp_centre', passengers=[{}] * 2, slope=1),
        dict(name='ramp_2_off_centre', passengers=[{}] * 2, slope=2, sub=[40, 200]),
        dict(name='corner_ramp_off_centre', passengers=[{}] * 2, slope=5, sub=[200, 200]),
        dict(name='south_east', passengers=[{}] * 2, sub=[200, 200]),
        # On a bridge deck: the transport's own coordinate and OnBridge, over
        # clear ground and over water; and a transport under a bridge.
        dict(name='bridge_deck', passengers=[{}] * 2, bridge=True, on_bridge=True),
        dict(name='bridge_deck_over_water', passengers=[{}] * 2, bridge=True, on_bridge=True, water=True),
        dict(name='bridge_deck_off_centre', passengers=[{}] * 2, bridge=True, on_bridge=True,
             sub=[200, 40]),
        dict(name='under_bridge', passengers=[{}] * 2, bridge=True),
        dict(name='under_bridge_on_water', passengers=[{}] * 2, bridge=True, water=True),
        # A bridge deck transport above the kill height (a falling deck).
        dict(name='bridge_deck_high', passengers=[{}] * 2, bridge=True, on_bridge=True, height=0xD1),
    ]


def generate():
    return {'source': 'unicorn/gamemd.exe', 'rows': [query(case) for case in cases()]}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='A dying Unit\'s passengers (UnitClass::ReceiveDamage 0x737F80..0x7381BC, after its '
              'Mark(UP)): the open-topped clear, the height kill, the Crashable skip and the escape '
              'loop, with native Can_Enter_Cell, Unlimbo placement and occupation, Scatter (its '
              'Scenario draw and Find_Nearby_Passable_Cell) and Queue_Mission. Infantry passengers '
              'of one type on flat, ramp, water and bridge cells of the 32x32 map, with standing '
              'allied or enemy infantry, one or two houses, teams and selection.',
        entry_points={'escape_block': START, 'remove_first_passenger': REMOVE_FIRST,
                      'infantry_can_enter_cell': CAN_ENTER, 'infantry_unlimbo': UNLIMBO,
                      'place_infantry_in_cell': PLACE, 'infantry_scatter': SCATTER,
                      'find_nearby_passable_cell': FNPC, 'kill_passengers': KILL_PASSENGERS,
                      'queue_mission': QUEUE, 'map_floor': FLOOR},
        assumptions=['The refinery_dock fixture (seed 31, the row frame, default 200): its Unit on '
                     'Guard, unlinked, as the transport, absent from its cell\'s list and bits; its '
                     'House (ArrayIndex 0) human or computer by row, game mode 1.',
                     'Passengers and standing infantry: the Infantry vtable over the original Walk '
                     'constructor, FacingClass ctor + SetROT(8), Strength 125, Guard, Doing 0, one '
                     'unarmed InfantryType (SpeedType Foot); passengers in limbo with Transporter '
                     'set, chained from the head +0x118 as boarding leaves them.',
                     'Standing infantry are listed in the cell (+0xE4 via +0x30) with their spot bit '
                     'and owner index in the ground occupation, as the occupation mark leaves them.',
                     'A second house at ArrayIndex 1, allied both ways only when the row says '
                     '(Allies +0x5788).',
                     'The ten Infantry startup initialisers (table 0x813490) run; LevelHeight 104 '
                     '(0x89E7C0, 0xAC13C8) and the 416 bridge scalars (0xAC13BC, 0xABC5DC, 0x8B3CAC) '
                     'are written as the scenario leaves them; the spot offsets 0x89E9F0, the '
                     'no-spot answer 0x89E778 and the empty coordinate 0xA8F200 are written.',
                     'Water: LandType 2 with a zero ground-speed row; the fixture\'s LandType 0 row '
                     'is 1.0. A bridge is the cell flag 0x100 on three cells in a west-east line.',
                     'The transport stands on MapClass floor 0x578080 under its XY, 416 higher on a '
                     'bridge deck, plus the row height. The ReceiveDamage frame supplies the '
                     'attacker ([esp+0x54], EBX), IgnoreDefenses ([esp+0x58]) and the '
                     'selected-by-the-player byte ([esp+0x13]).'],
        substitutions=['RecordKill 0x702D40, FootClass::UnInit 0x4DE5D0, the Unit RemoveGunner '
                       '0x7464E0, InfantryClass::Assign_Target 0x51B1F0, Select 0x6FBFA0, '
                       'TeamClass::Add_Member 0x6EA500 and Remove_Member 0x6EA870 are recorded and '
                       'return without effect.',
                       'FootClass::Unlimbo 0x4D7170 is recorded and only writes the coordinate, '
                       'clears limbo and sets IsOnMap: no facing, cell list, reveal, layer or '
                       'Enter_Idle_Mode. Later escapees therefore do not see earlier ones in the '
                       'cell list (Can_Enter_Cell\'s stationary count) but do see their occupation '
                       'bits and owner.',
                       'The Infantry destination setter 0x51AA40 is recorded and writes only the '
                       'NavCom +0x5A4; the Walk Process slot 0x75AC80 is recorded and returns not '
                       'moving.']))
