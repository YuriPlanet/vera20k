"""Original FlyLocomotionClass Process (0x004CCB40), frame by frame, for a living aircraft.

Run: python -m tools.spatial_oracle.fly_process [--check | --write]

Each row builds one airborne Aircraft on the original Aircraft, AircraftType, Fly,
Facing and (for dock rows) Building vtables and constructors, over the 128x128 block of
real cells of ``aircraft_fire_location`` with MapSize 64x64, the original air tracker
and Display layers of ``fly_landing_phase``, and calls ILocomotion Process once per
frame. Every callee runs natively: Horizontal_Step (0x004CEFB0) with IFlyControl,
GetDockCoord (0x00447B20) and Find_Attack_Cell (0x00418E20); UpdateFlightMotion
(0x004CD600) with Queue_Mission (0x0041BA90), the paid step, the map-edge admission,
the height step, the landing drift and trigger, Begin_Landing (0x004CFA70) and the
ramp; Ready_To_Commence and Commence (0x0041B5E0, 0x0041B870); the phase transitions
(0x004CD2A0) with the landing callback; and the playfield latch (0x004CD510).

Exceptions, recorded per frame in ``calls`` and returned without running their
bodies: Aircraft vt+0x484 Enter_Idle_Mode (Begin_Landing's refusal) and vt+0x150
(Horizontal_Step's deselect of a shrouded enemy aircraft).
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools.native_oracle import OracleError, finish_vectors, provenance, run_checked
from tools.spatial_oracle.aircraft_fire_location import (
    OWNER, SCENARIO, SIDE, TARGET, TYPE, WEAPON, cell,
)
from tools.spatial_oracle.fly_landing_phase import HOUSE, LOCO, RULES, fixture as landing_fixture
from tools.spatial_oracle.map_queries import dwords, packed
from tools.native_oracle import SCRATCH

PROCESS = 0x4CCB40
FACING_CONSTRUCTOR, FACING_SET_ROT, FACING_SNAP, FACING_CURRENT = 0x4C91C0, 0x4C9680, 0x4C9300, 0x4C93D0
TYPE_SPEED_CONVERSION = (0x71465F, 0x71469F)
FRAME, GAME_ACTIVE = 0xA8ED84, 0xA8E9A0
AIRCRAFT_VT = 0x7E22A4
BUILDING_VT, BUILDING_TYPE_VT = 0x7E3EBC, 0x7E4570
FOUNDATION_WIDTHS, FOUNDATION_HEIGHTS = 0x8192B8, 0x819310
VTABLE = SCRATCH + 0xA0000             # clone of the Aircraft vtable, 0x600 bytes
STUBS = SCRATCH + 0xA1000              # INT3 stubs, 0x10 apart
BUILDING, BUILDING_TYPE = SCRATCH + 0xB0000, SCRATCH + 0xB4000
DOCKS = SCRATCH + 0xB8000              # BuildingType DockingOffsets items
NAV_CELL = SCRATCH + 0xC0000           # a NavCom stand-in (Guard rows only test non-null)
ARG = SCRATCH + 0xC1000
ITEMS = SCRATCH + 0xC2000              # the Foot and Aircraft vectors' items
CELL_VT = 0x7E4EEC
# Is_Cell_Free_For_Landing walks the Foot vector, IsLandZoneClear the Aircraft one.
FOOT_VECTOR, AIRCRAFT_VECTOR = (0x8B3DC4, 0x8B3DD0), (0xA8E394, 0xA8E3A0)
STUB_SLOTS = {0x484: ('enter_idle_mode', 8), 0x150: ('deselect', 0)}
START = (60, 64)


def i32(raw):
    return struct.unpack('<i', raw)[0]


def foundation_index(u, width, height):
    for index in range(22):
        if (i32(u.mem_read(FOUNDATION_WIDTHS + 4 * index, 4)),
                i32(u.mem_read(FOUNDATION_HEIGHTS + 4 * index, 4))) == (width, height):
            return index
    raise OracleError(f'no foundation {width}x{height}')


def build(case, stub_slots=STUB_SLOTS):
    x, y = case.get('cell', START)
    z = case.get('z', 1500)
    f, _ = landing_fixture(dict(cell=[x, y], z=z, landing=False, registration_z=z,
                                airport_bound=case.get('airport_bound', False)))
    u = f.u
    calls = []
    # Owner: the Aircraft vtable cloned so the exceptions can be recorded.
    u.mem_write(STUBS, b'\xCC' * 0x100)
    vt = bytearray(u.mem_read(AIRCRAFT_VT, 0x600))
    stub_at = {}
    for n, (slot, stub) in enumerate(stub_slots.items()):
        address = STUBS + 0x10 * n
        vt[slot:slot + 4] = dwords(address)
        stub_at[address] = stub
    u.mem_write(VTABLE, bytes(vt))
    u.mem_write(OWNER, dwords(VTABLE))

    def on_stub(_u, address, _size, _data):
        # A slot given an original body runs it when its first argument is NULL.
        name, pops, *original = stub_at[address]
        sp = u.reg_read(UC_X86_REG_ESP)
        args = [i32(u.mem_read(sp + 4 + 4 * k, 4)) for k in range(pops // 4)]
        calls.append([name] + args)
        if original and args[0] == 0:
            u.reg_write(UC_X86_REG_EIP, original[0])
            return
        u.reg_write(UC_X86_REG_ESP, sp + 4 + pops)
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_EIP, struct.unpack('<I', u.mem_read(sp, 4))[0])

    u.hook_add(UC_HOOK_CODE, on_stub, begin=STUBS, end=STUBS + 0xFF)
    u.mem_write(FRAME, dwords(case.get('frame', 1000)))
    # The in-game flag the cell's building lookup (0x0047C520) and occupier
    # queries read; Main__PrepareSession raises it for every scenario.
    u.mem_write(GAME_ACTIVE, b'\1')
    # Type: Speed through the original ReadINI conversion, flags per row.
    u.reg_write(UC_X86_REG_EBP, TYPE)
    u.reg_write(UC_X86_REG_EAX, case.get('ini_speed', 14) & 0xFFFFFFFF)
    u.reg_write(UC_X86_REG_ESP, f.sp)
    run_checked(u, *TYPE_SPEED_CONVERSION, count=100)
    u.mem_write(TYPE + 0x618, dwords(case.get('flight_level', -1)))
    u.mem_write(TYPE + 0x2F8, dwords(case.get('slowdown', 500)))
    u.mem_write(TYPE + 0xD27, bytes([case.get('hunter_seeker', False)]))
    u.mem_write(TYPE + 0xC95, bytes([case.get('dropship', False)]))
    u.mem_write(TYPE + 0xE0B, bytes([case.get('fly_by', False)]))
    u.mem_write(TYPE + 0xE0E, bytes([case.get('fighter', False)]))
    u.mem_write(TYPE + 0xE0A, bytes([case.get('landable', True)]))
    u.mem_write(TYPE + 0xA0, dwords(case.get('strength', 150)))
    u.mem_write(TYPE + 0x3B0, struct.pack('<d', case.get('pitch_angle', 0.0)))
    # Projectile ROT <= 1 and not Inviso makes IFlyControl Is_Strafe (0x0041B7F0).
    projectile = struct.unpack('<I', u.mem_read(WEAPON + 0xA0, 4))[0]
    u.mem_write(projectile + 0x2DC, dwords(0 if case.get('strafe', False) else 3))
    u.mem_write(RULES + 0x44, dwords(case.get('pose_dir', 2)))
    u.mem_write(RULES + 0xFA8, dwords(0))
    # Find_Attack_Cell's search radius: the RulesClass constructor's 0x2000
    # (0x00667235), which no INI key writes.
    u.mem_write(RULES + 0x1478, dwords(0x2000))
    # Every cell a real flat Clear CellClass, whose virtuals Find_Attack_Cell's
    # ring queries call, with Track land cost 1.0; the aircraft in both vectors.
    for cy in range(SIDE):
        for cx in range(SIDE):
            u.mem_write(cell(cx, cy), dwords(CELL_VT))
            u.mem_write(cell(cx, cy) + 0x44, dwords(-1))
    u.mem_write(0x89EA40, struct.pack('<90f', *[1.0] * 90))
    for n, (vector, count) in enumerate((FOOT_VECTOR, AIRCRAFT_VECTOR)):
        u.mem_write(ITEMS + 0x100 * n, dwords(OWNER))
        u.mem_write(vector, dwords(ITEMS + 0x100 * n))
        u.mem_write(count, dwords(1))
    u.mem_write(OWNER + 0x520, dwords(-1))
    # Facings: original constructor, ROT and heading for Primary and Secondary.
    for offset in (0x388, 0x3A0):
        f.call(FACING_CONSTRUCTOR, OWNER + offset, [])
        f.call(FACING_SET_ROT, OWNER + offset, [case.get('rot', 3)])
        u.mem_write(ARG, dwords(case.get('facing', 0x4000)))
        f.call(FACING_SNAP, OWNER + offset, [ARG])
    # Owner.
    u.mem_write(OWNER + 0x6C, dwords(case.get('health', 150)))
    u.mem_write(OWNER + 0xAC, dwords(case.get('mission', 2)))
    u.mem_write(OWNER + 0xB4, dwords(case.get('queued', -1)))
    u.mem_write(OWNER + 0x2FC, dwords(case.get('ammo', 1)))
    u.mem_write(OWNER + 0x2B4, dwords(TARGET if case.get('target') else 0))
    if case.get('target'):
        u.mem_write(TARGET + 0x9C, dwords(*case['target']))
    nav_com = case.get('nav_com')
    u.mem_write(OWNER + 0x5A4, dwords(BUILDING if nav_com == 'dock' else NAV_CELL if nav_com else 0))
    u.mem_write(OWNER + 0x3D4, bytes([case.get('loaner', False), case.get('in_playfield', True)]))
    u.mem_write(OWNER + 0x6D2, bytes([case.get('locked', False)]))
    u.mem_write(OWNER + 0x6D4, bytes([case.get('ready', False)]))
    u.mem_write(OWNER + 0x2E8, struct.pack('<f', case.get('pitch', 0.0)))
    # Fly.
    u.mem_write(LOCO + 0x1C, dwords(*case.get('destination', [80 * 256 + 128, 64 * 256 + 128, 0])))
    u.mem_write(LOCO + 0x34, bytes([case.get('moving', True)]))
    u.mem_write(LOCO + 0x38, dwords(case.get('target_height', 1500)))
    u.mem_write(LOCO + 0x40, struct.pack('<dd', case.get('target_speed', 1.0),
                                         case.get('speed', 1.0)))
    u.mem_write(LOCO + 0x50, bytes([case.get('taking_off', False), case.get('landing', False), 0]))
    u.mem_write(LOCO + 0x5C, bytes([case.get('cruise', False)]))
    if 'dock' in case:
        place_dock(f, case['dock'])
    return f, calls


def place_dock(f, dock):
    """A Helipad building on the destination's cell whose radio contacts hold the aircraft."""
    u = f.u
    bx, by = dock['origin']
    u.mem_write(BUILDING, dwords(BUILDING_VT))
    u.mem_write(BUILDING + 0x14, dwords(5))
    u.mem_write(BUILDING + 0x21C, dwords(HOUSE))
    u.mem_write(BUILDING + 0x9C, dwords(bx * 256 + 128, by * 256 + 128, 0))
    u.mem_write(BUILDING + 0x520, dwords(BUILDING_TYPE))
    u.mem_write(BUILDING_TYPE, dwords(BUILDING_TYPE_VT))
    u.mem_write(BUILDING_TYPE + 0xEF0, dwords(foundation_index(u, *dock.get('foundation', (3, 2)))))
    u.mem_write(BUILDING_TYPE + 0x16CB, bytes([dock.get('helipad', True)]))
    u.mem_write(BUILDING_TYPE + 0x16A9, bytes([dock.get('unit_repair', False)]))
    offsets = dock.get('offsets', [[0, -128, 0], [0, 128, 0], [-256, -128, 0], [-256, 128, 0]])
    u.mem_write(BUILDING_TYPE + 0x1780, dwords(len(offsets)))
    u.mem_write(DOCKS, b''.join(dwords(*xyz) for xyz in offsets))
    u.mem_write(BUILDING_TYPE + 0x1788, dwords(DOCKS, len(offsets)))
    width, height = dock.get('foundation', (3, 2))
    for dy in range(height):
        for dx in range(width):
            u.mem_write(cell(bx + dx, by + dy) + 0xE4, dwords(BUILDING))
    slot = dock.get('slot')
    contacts = SCRATCH + 0xBC000
    u.mem_write(BUILDING + 0xE4, dwords(contacts, len(offsets)))
    if slot is not None:
        u.mem_write(contacts + 4 * slot, dwords(OWNER))
        u.mem_write(SCRATCH + 0x70000, dwords(BUILDING))


def facing(f, offset):
    f.call(FACING_CURRENT, OWNER + offset, [ARG])
    current = struct.unpack('<H', f.u.mem_read(ARG, 2))[0]
    desired = struct.unpack('<H', f.u.mem_read(OWNER + offset, 2))[0]
    return [current, desired]


def process_row(case):
    f, calls = build(case)
    u = f.u
    frames = []
    frame = case.get('frame', 1000)
    for n in range(case.get('frames', 120)):
        u.mem_write(FRAME, dwords(frame + n))
        before = len(calls)
        returned = f.call(PROCESS, 0, [LOCO + 4]) & 0xFF
        target, current = struct.unpack('<dd', u.mem_read(LOCO + 0x40, 16))
        frames.append(dict(
            xyz=list(struct.unpack('<iii', u.mem_read(OWNER + 0x9C, 12))),
            primary=facing(f, 0x388), secondary=facing(f, 0x3A0),
            target_speed=target, speed=current,
            flight_level=i32(u.mem_read(LOCO + 0x38, 4)),
            destination=list(struct.unpack('<iii', u.mem_read(LOCO + 0x1C, 12))),
            moving=u.mem_read(LOCO + 0x34, 1)[0], phase=list(u.mem_read(LOCO + 0x50, 3)),
            cruise=u.mem_read(LOCO + 0x5C, 1)[0],
            mission=[i32(u.mem_read(OWNER + 0xAC, 4)), i32(u.mem_read(OWNER + 0xB4, 4))],
            locked=u.mem_read(OWNER + 0x6D2, 1)[0],
            in_playfield=u.mem_read(OWNER + 0x3D5, 1)[0],
            returned=returned, calls=calls[before:]))
        if case.get('stop_when_parked') and frames[-1]['xyz'][2] == 0 and not returned:
            break
        # A recorded exception would have changed the owner natively; stop there.
        if frames[-1]['calls']:
            break
    return dict(input=case, frames=frames,
                next_random=f.call(0x65C780, SCENARIO + 0x218, []))


def process_cases():
    east = [80 * 256 + 128, 64 * 256 + 128, 0]
    # Move_To (0x004CCE1C..0x004CCFA7) gives an armed aircraft a destination at
    # ground + FlightLevel and cruise mode; the armed rows start in that state.
    armed = dict(destination=[80 * 256 + 128, 64 * 256 + 128, 1500], cruise=True,
                 target=[80 * 256 + 128, 64 * 256 + 128, 0])
    rows = [
        dict(name='move_lands_at_ground_cell', destination=east, frames=260, stop_when_parked=True),
        dict(name='cruise_to_ground_cell', destination=east, cruise=True, frames=220),
        dict(armed, name='fighter_with_target_circles', fighter=True, frames=260),
        dict(armed, name='fighter_out_of_ammo_lands', fighter=True, ammo=0, cruise=False,
             destination=east, frames=260, stop_when_parked=True),
        dict(armed, name='strafer_with_target', strafe=True, frames=260),
        dict(armed, name='bomber_with_target_slows', frames=260),
        dict(name='turn_north', destination=[60 * 256 + 128, 40 * 256 + 128, 0], frames=320,
             stop_when_parked=True),
        dict(name='turn_back_west', destination=[40 * 256 + 128, 66 * 256 + 128, 0], frames=340,
             stop_when_parked=True),
        dict(name='locked_holds_heading', destination=[60 * 256 + 128, 40 * 256 + 128, 0],
             locked=True, frames=40),
        dict(name='hunter_seeker', destination=east, hunter_seeker=True, frames=160),
        dict(name='hunter_seeker_with_target', destination=east, hunter_seeker=True,
             target=[80 * 256 + 128, 64 * 256 + 128, 0], frames=200),
        dict(name='fly_by', destination=[80 * 256 + 128, 64 * 256 + 128, 1500], fly_by=True,
             cruise=True, frames=200),
        dict(name='guard_queues_move', mission=5, nav_com=True, destination=east, frames=3),
        dict(name='guard_airport_bound_keeps', mission=5, nav_com=True, airport_bound=True,
             destination=east, frames=3),
        dict(name='ready_commences', mission=5, queued=2, ready=True, destination=east, frames=3),
        dict(name='restores_flight_level', target_height=0, destination=east, frames=5),
        dict(name='climbs_to_cruise', z=200, target_height=1500, taking_off=True,
             destination=east, frames=60),
        dict(name='descends_with_drift', z=1500, target_height=0, landing=True,
             destination=[61 * 256 + 40, 64 * 256 + 200, 0], speed=0.0, target_speed=0.0,
             frames=120, stop_when_parked=True),
        dict(name='near_cruise_flight_level', cruise=True,
             destination=[62 * 256 + 128, 64 * 256 + 128, 900], frames=80),
        dict(name='dropship_flight_level', dropship=True, slowdown=1200,
             destination=east, frames=140),
        dict(name='not_moving_stays', moving=False, speed=0.0, target_speed=0.0, frames=4),
        dict(name='dock_contact', destination=[80 * 256 + 128, 64 * 256 + 128, 0],
             dock=dict(origin=[79, 63], slot=3), frames=260, stop_when_parked=True),
        dict(name='enter_helipad_without_drift', mission=7, nav_com='dock', cruise=True,
             destination=[80 * 256 + 128, 64 * 256, 0], dock=dict(origin=[79, 63], slot=2),
             frames=260, stop_when_parked=True),
        dict(name='dock_without_contact', destination=[80 * 256 + 128, 64 * 256 + 128, 0],
             dock=dict(origin=[79, 63]), frames=260, stop_when_parked=True),
        dict(name='airport_bound_refused', airport_bound=True, destination=east, frames=200),
    ]
    return [process_row(case) for case in rows]


def generate():
    return dict(process=process_cases())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'process': PROCESS, 'update_flight_motion': 0x4CD600,
                      'horizontal_step': 0x4CEFB0, 'begin_landing': 0x4CFA70,
                      'get_dock_coord': 0x447B20, 'find_attack_cell': 0x418E20,
                      'phase_transitions': 0x4CD2A0, 'playfield_latch': 0x4CD510},
        assumptions=[
            'The in-game flag 0x00A8E9A0 is raised, as Main__PrepareSession does for a scenario. One Aircraft on the cloned Aircraft vtable with real AircraftType (Speed through the original ReadINI conversion), FacingClass and FlyLocomotionClass constructed by their original code; the 128x128 cell block, MapSize 64x64 and LocalSize 0,0,64,64 of aircraft_fire_location, every cell carrying the CellClass vtable 0x007E4EEC with Track land cost 1.0, as aircraft_move has them; the aircraft alone in the Foot and Aircraft vectors; the original air tracker and Display layers of fly_landing_phase, the aircraft registered at its starting height.',
            'Rows supply the in-flight state at their first frame (Fly destination, moving, flight level, speeds, phase flags, cruise byte; owner mission, Target, Ammo, NavCom, lock and ready latches); the producers of that state are not run. Rules FlightLevel 1500, PoseDir per row, Find_Attack_Cell radius 0x2000 (Rules+0x1478, the constructor value).',
            'Dock rows place one Helipad building (BuildingType foundation from the original tables, NumberOfDocks and DockingOffsets per row) on the destination cell; a contact row lists the aircraft in the building\'s radio slot and the building in the aircraft\'s one slot.',
        ],
        substitutions=['Aircraft vt+0x484 Enter_Idle_Mode and vt+0x150 recorded with their arguments and returned 0 without running.'],
        scope='ILocomotion Process of a living aircraft per frame: cruise, turns, arrival slowdown and landing at a ground cell, cruise-mode and armed circling, strafe, lock, HunterSeeker, FlyBy, Guard to Move, Commence, flight-level restore and selection, climb, the landing drift, IsDropship flight level, dock contact and attack-cell approach, and the AirportBound refusal. Excludes the dead fall (aircraft_crash), the map edge (fly_map_edge), Stop_Moving, Power_Off and the spin.',
    ))
