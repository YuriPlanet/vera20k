"""Original FlyLocomotionClass Stop_Moving (0x004CCFD0) on a living aircraft, and the
playfield edge cell it takes (MapClass 0x00586AC0).

Run: python -m tools.spatial_oracle.fly_stop [--check | --write]

Stop rows build the airborne Aircraft of ``fly_process`` (its 128x128 cell block, MapSize
64x64, LocalSize 0,0,64,64, Fly and owner state) and call ILocomotion Stop_Moving once.
Everything runs natively: Is_Moving (0x004CCA90), IsCellInPlayfield (0x00578460), vt+0x4DC
(0x0041B890), the edge cell, GetCurrentMission (0x005B3040), Find_Attack_Cell (0x00418E20),
and Find_Nearest_Friendly_Airfield (0x0041A160) with its House building walk, its
ObjectClass::Array walk (0x00A8E364) and Find_Nearby_Passable_Cell (0x0056DC20). Exceptions,
recorded with their arguments and returned without running their bodies: Aircraft vt+0x480
Assign_Destination and vt+0x16C ReceiveDamage, and MapClass::GetZoneID (0x0056D230), which
answers zone 1 for every cell (the fixture has no zone tables), so every zone test the search
makes passes. A row's buildings stand on cells whose content (+0xE4) is the building and whose
ground occupation (+0x124) carries the building bit 0x80, as its Mark leaves them.

Stun rows call FootClass::Stun (0x004D5660, Aircraft vt+0x3A0) on the same aircraft at
Health 0, as TechnoClass::ReceiveDamage's death arm does: its NULL destinations run
AircraftClass::Assign_Destination (0x0041AA80) and Foot's setter natively, Stop_Driver and
TechnoClass::Stun (0x006FCD40) with its Assign_Target too; the radio broadcast (vt+0x280),
Detach_All (vt+0xDC) and the deselect (vt+0x150) are recorded and returned.

Edge rows call 0x00586AC0 directly for cells around several Size/LocalSize rectangles.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import CELLS, MAP, SCENARIO, SIDE, TYPE, cell
from tools.spatial_oracle.fly_landing_phase import HOUSE, LOCO, OWNER, RULES
from tools.spatial_oracle.fly_process import (
    ARG, BUILDING_TYPE_VT, BUILDING_VT, CELL_VT, STUB_SLOTS, build, foundation_index, i32,
)
from tools.spatial_oracle.map_queries import dwords

STOP_MOVING, EDGE_CELL, AIRFIELD, GET_ZONE_ID = 0x4CCFD0, 0x586AC0, 0x41A160, 0x56D230
CELL_CONSTRUCTOR = 0x47BBF0
UNIT_VT = 0x7F5C70
DUMMY_CELL = 0xABDC50
# ObjectClass::Array (a DynamicVectorClass at 0x00A8E360): items and count.
OBJECTS, OBJECT_COUNT = 0xA8E364, 0xA8E370
OBJECT_ITEMS = SCRATCH + 0xC4000
HOUSE_ITEMS = SCRATCH + 0xC5000        # House+0x6C, the House's buildings
DOCK_ITEMS = SCRATCH + 0xC6000         # AircraftType+0x3EC, Dock=
C4 = SCRATCH + 0xC7000                 # Rules+0xFA8, a warhead the row can recognise
TECHNOS = SCRATCH + 0xC8000            # other technos, 0x1000 apart
BUILDINGS = SCRATCH + 0xD0000          # buildings, 0x1000 apart
BUILDING_TYPES = SCRATCH + 0xE0000     # their types, 0x2000 apart
ENEMY = SCRATCH + 0xF0000              # another House
STOP_STUBS = dict(STUB_SLOTS)
STOP_STUBS.update({0x480: ('assign_destination', 8), 0x16C: ('receive_damage', 0x1C)})
# FootClass::Stun (Aircraft vt+0x3A0), as TechnoClass::ReceiveDamage's death arm calls it.
# Its NULL destinations run AircraftClass::Assign_Destination (0x0041AA80); the
# re-targets Stop_Moving makes are recorded as above.
STUN, ASSIGN_DESTINATION = 0x4D5660, 0x41AA80
STUN_STUBS = dict(STOP_STUBS)
STUN_STUBS.update({0x480: ('assign_destination', 8, ASSIGN_DESTINATION),
                   0x280: ('broadcast', 4), 0xDC: ('detach_all', 4)})


def target(u, pointer):
    """A pointer the stop passed as data: a cell of the block or the dummy cell."""
    if pointer == 0:
        return None
    if CELLS <= pointer < CELLS + SIDE * SIDE * 0x200 and (pointer - CELLS) % 0x200 == 0:
        index = (pointer - CELLS) // 0x200
        return ['cell', index % SIDE, index // SIDE]
    if pointer == DUMMY_CELL:
        return ['dummy'] + list(struct.unpack('<hh', u.mem_read(DUMMY_CELL + 0x24, 4)))
    raise AssertionError(f'unexpected pointer 0x{pointer:08X}')


def place_building(u, n, building):
    """Building ``n`` of the row's House list, Marked on its foundation cells."""
    address, kind = BUILDINGS + 0x1000 * n, BUILDING_TYPES + 0x2000 * building.get('type', 0)
    bx, by = building['origin']
    width, height = building.get('foundation', (2, 2))
    u.mem_write(address, dwords(BUILDING_VT))
    u.mem_write(address + 0x14, dwords(5))
    u.mem_write(address + 0x21C, dwords(ENEMY if building.get('enemy') else HOUSE))
    u.mem_write(address + 0x9C, dwords(bx * 256 + 128, by * 256 + 128, 0))
    u.mem_write(address + 0x81, bytes([building.get('limbo', False)]))
    u.mem_write(address + 0x520, dwords(kind))
    u.mem_write(kind, dwords(BUILDING_TYPE_VT))
    u.mem_write(kind + 0xEF0, dwords(foundation_index(u, width, height)))
    if not building.get('limbo'):
        for dy in range(height):
            for dx in range(width):
                u.mem_write(cell(bx + dx, by + dy) + 0xE4, dwords(address))
                u.mem_write(cell(bx + dx, by + dy) + 0x124, b'\x80')
    return address


def place_techno(u, n, techno):
    """Another techno: a Unit at the row's coordinate."""
    address = TECHNOS + 0x1000 * n
    u.mem_write(address, dwords(UNIT_VT))
    u.mem_write(address + 0x14, dwords(5))
    u.mem_write(address + 0x21C, dwords(ENEMY if techno.get('enemy') else HOUSE))
    u.mem_write(address + 0x9C, dwords(*techno['xyz']))
    u.mem_write(address + 0x81, bytes([techno.get('limbo', False)]))
    u.mem_write(address + 0x6C4, dwords(TYPE))
    return address


def stop_row(case):
    stun = case.get('stun', False)
    f, calls = build(case, STUN_STUBS if stun else STOP_STUBS)
    u = f.u
    zones = []
    # The shared dummy cell as startup leaves it: CellClass::Constructor on it.
    f.call(CELL_CONSTRUCTOR, DUMMY_CELL, [])
    u.mem_write(RULES + 0xFA8, dwords(C4))
    u.mem_write(OWNER + 0x2E8, struct.pack('<f', case.get('pitch', 0.0)))
    if 'location' in case:
        u.mem_write(OWNER + 0x9C, dwords(*case['location']))
    if 'local' in case:
        u.mem_write(MAP + 0xFC, dwords(*case['local']))
    # Dock= list: the BuildingType indices of the row's types.
    docks = case.get('docks', [])
    if docks:
        u.mem_write(DOCK_ITEMS, dwords(*[BUILDING_TYPES + 0x2000 * k for k in docks]))
    u.mem_write(TYPE + 0x3EC, dwords(DOCK_ITEMS))
    u.mem_write(TYPE + 0x3F8, dwords(len(docks)))
    buildings = [place_building(u, n, b) for n, b in enumerate(case.get('buildings', []))]
    u.mem_write(HOUSE + 0x6C, dwords(HOUSE_ITEMS))
    u.mem_write(HOUSE + 0x78, dwords(len(buildings)))
    if buildings:
        u.mem_write(HOUSE_ITEMS, dwords(*buildings))
    objects = [OWNER] + [place_techno(u, n, t) for n, t in enumerate(case.get('technos', []))]
    objects += [b for b, spec in zip(buildings, case.get('buildings', [])) if spec.get('object', True)]
    u.mem_write(OBJECT_ITEMS, dwords(*objects))
    u.mem_write(OBJECTS, dwords(OBJECT_ITEMS))
    u.mem_write(OBJECT_COUNT, dwords(len(objects)))

    def on_zone(_u, _address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        ret, where, movement_zone, bridge = struct.unpack('<IIIi', u.mem_read(sp, 16))
        if AIRFIELD <= ret < 0x41A564:
            zones.append(list(struct.unpack('<hh', u.mem_read(where, 4))) + [movement_zone,
                                                                              bridge & 0xFF])
        u.reg_write(UC_X86_REG_ESP, sp + 4 + 0xC)
        u.reg_write(UC_X86_REG_EAX, 1)
        u.reg_write(UC_X86_REG_EIP, ret)

    u.hook_add(UC_HOOK_CODE, on_zone, begin=GET_ZONE_ID, end=GET_ZONE_ID)
    if stun:
        # The death arm's object: Health 0, its TarCom one of the row's technos.
        u.mem_write(OWNER + 0x6C, dwords(0))
        if 'target' in case:
            u.mem_write(OWNER + 0x2B4, dwords(objects[1 + case['target']]))
        # Up to three Find_Attack_Cell searches around an occupied cell.
        f.call(STUN, OWNER, [], count=3_000_000)
    else:
        f.call(STOP_MOVING, 0, [LOCO + 4])
    recorded = []
    for call in calls:
        if call[0] == 'assign_destination':
            recorded.append(['assign_destination', target(u, call[1] & 0xFFFFFFFF), call[2]])
        elif call[0] == 'receive_damage':
            damage, distance, warhead, attacker, ignore, escape, house = call[1:]
            assert damage & 0xFFFFFFFF == OWNER + 0x6C and warhead & 0xFFFFFFFF == C4
            recorded.append(['receive_damage', i32(u.mem_read(OWNER + 0x6C, 4)), distance,
                             'c4', attacker, ignore & 0xFF, escape & 0xFF, house])
        else:
            recorded.append(call)
    return dict(input=case, calls=recorded, zones=zones,
                destination=list(struct.unpack('<iii', u.mem_read(LOCO + 0x1C, 12))),
                next_random=f.call(0x65C780, SCENARIO + 0x218, []))


def stop_cases():
    at = lambda x, y, z=1500: [x * 256 + 128, y * 256 + 128, z]
    margin = [6, 6, 52, 52]
    pad, depot = dict(type=0), dict(type=1)
    rows = [
        dict(name='not_moving', moving=False),
        dict(name='pitch_counts_as_moving', moving=False, pitch=0.25),
        dict(name='move_takes_cell_under_it'),
        dict(name='guard_takes_cell_under_it', mission=5),
        dict(name='cell_under_it_holds_building',
             buildings=[dict(depot, origin=[59, 63], foundation=(3, 2))]),
        # Attack: the House's nearest building, the first Dock= type a quarter as far.
        dict(name='attack_dock_building', mission=1, docks=[0],
             buildings=[dict(pad, origin=[80, 70])]),
        dict(name='attack_dock_quarter_beats_nearer', mission=1, docks=[0],
             buildings=[dict(depot, origin=[66, 64]), dict(pad, origin=[84, 64])]),
        dict(name='attack_nearer_building_beats_quarter', mission=1, docks=[0],
             buildings=[dict(depot, origin=[64, 64]), dict(pad, origin=[100, 64])]),
        dict(name='attack_second_dock_type_unweighted', mission=1, docks=[1, 0],
             buildings=[dict(depot, origin=[70, 64]), dict(pad, origin=[78, 64])]),
        dict(name='attack_list_order_tie', mission=1, docks=[0],
             buildings=[dict(depot, origin=[66, 60]), dict(depot, origin=[66, 68])]),
        dict(name='attack_quarter_distance_counts_height', mission=1, docks=[0],
             buildings=[dict(pad, origin=[62, 64])]),
        dict(name='attack_at_airfield_searches_own_cell', mission=1, docks=[0], z=300,
             buildings=[dict(pad, origin=[61, 64])]),
        dict(name='attack_at_airfield_own_cell_held', mission=1, docks=[0], z=300,
             buildings=[dict(pad, origin=[60, 64])]),
        dict(name='attack_building_in_limbo_is_skipped', mission=1, docks=[0],
             buildings=[dict(pad, origin=[70, 64], limbo=True)],
             technos=[dict(xyz=at(70, 70, 0))]),
        dict(name='attack_without_dock_list_takes_nearest_techno', mission=1,
             technos=[dict(xyz=at(75, 64, 0)), dict(xyz=at(66, 70, 0)),
                      dict(xyz=at(61, 64, 0), enemy=True)]),
        dict(name='attack_techno_in_limbo_is_skipped', mission=1,
             technos=[dict(xyz=at(62, 64, 0), limbo=True), dict(xyz=at(70, 64, 0))]),
        dict(name='attack_alone_takes_own_cell', mission=1),
        dict(name='attack_without_buildings_takes_technos', mission=1, docks=[0],
             technos=[dict(xyz=at(68, 64, 0))]),
        # Outside the playfield: the edge cell, unless the aircraft is a loaner. The margin
        # rows inset LocalSize so the cell stays inside the Size diamond.
        dict(name='margin_top_clamped', local=margin, cell=[36, 36]),
        dict(name='margin_right_clamped', local=margin, cell=[90, 34]),
        dict(name='margin_left_clamped', local=margin, cell=[34, 90]),
        dict(name='margin_bottom_clamped', local=margin, cell=[95, 92]),
        dict(name='margin_loaner_not_clamped', local=margin, cell=[90, 34], loaner=True),
        dict(name='margin_attack_takes_airfield', local=margin, cell=[90, 34], mission=1,
             docks=[0], buildings=[dict(pad, origin=[70, 40])]),
        dict(name='outside_top_clamped', cell=[30, 30]),
        dict(name='outside_right_clamped', cell=[100, 30]),
        dict(name='outside_left_clamped', cell=[30, 100]),
        dict(name='outside_loaner_not_clamped', cell=[100, 30], loaner=True),
        dict(name='cell_zero_loaner_self_destructs', cell=[0, 0], loaner=True),
        dict(name='cell_zero_negative_location', location=[-100, -100, 1500], loaner=True),
        # The death arm's Stun: two NULL destinations around Stop_Driver, each reaching
        # Stop_Moving unless an Attack aircraft still holds its TarCom.
        dict(name='stun_move_stops_three_times', stun=True),
        dict(name='stun_move_over_building', stun=True,
             buildings=[dict(depot, origin=[59, 63], foundation=(3, 2))]),
        dict(name='stun_guard_over_building', stun=True, mission=5,
             buildings=[dict(depot, origin=[59, 63], foundation=(3, 2))]),
        dict(name='stun_attack_with_target', stun=True, mission=1, docks=[0], target=0,
             buildings=[dict(pad, origin=[80, 70])],
             technos=[dict(xyz=at(90, 90, 0), enemy=True)]),
        dict(name='stun_attack_without_target', stun=True, mission=1, docks=[0],
             buildings=[dict(pad, origin=[80, 70])]),
        dict(name='stun_not_moving', stun=True, moving=False),
    ]
    return [stop_row(case) for case in rows]


def edge_cases():
    rows = []
    rects = [(64, [0, 0, 64, 64]), (80, [2, 2, 76, 72]), (100, [5, 7, 60, 40]),
             (65, [3, 4, 50, 40])]
    for width, local in rects:
        f, _ = build(dict(moving=False))
        u = f.u
        u.mem_write(MAP + 0xF4, dwords(width))
        u.mem_write(MAP + 0xFC, dwords(*local))
        for inset in (0, 1):
            for y in range(-12, 150, 13):
                for x in range(-12, 150, 13):
                    u.mem_write(ARG + 8, struct.pack('<hh', x, y))
                    f.call(EDGE_CELL, MAP, [ARG, ARG + 8, inset])
                    rows.append([width, *local, x, y, inset,
                                 *struct.unpack('<hh', u.mem_read(ARG, 4))])
    return rows


def generate():
    return dict(stop=stop_cases(), edge=edge_cases())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'stop_moving': STOP_MOVING, 'edge_cell': EDGE_CELL,
                      'find_nearest_friendly_airfield': AIRFIELD,
                      'find_attack_cell': 0x418E20, 'nearby_passable_cell': 0x56DC20},
        assumptions=[
            'The aircraft, map and Fly of fly_process: one Aircraft on the cloned Aircraft vtable with the original AircraftType, FacingClass and FlyLocomotionClass constructors; the 128x128 cell block of aircraft_fire_location with MapSize 64x64 and LocalSize 0,0,64,64, every cell a flat Clear CellClass (vtable 0x007E4EEC, no overlay) with land cost 1.0 for every speed type; the aircraft alone in the Foot and Aircraft vectors. The shared dummy cell 0x00ABDC50 is built by CellClass::Constructor 0x0047BBF0, as startup builds it.',
            'Rows supply the owner state (mission, loaner +0x3D4, Location, Fly moving byte and pitch), a LocalSize where they inset it, the AircraftType Dock= list (+0x3EC, count +0x3F8), the House building list (+0x6C, count +0x78) and ObjectClass::Array (the aircraft first, then the row\'s Units, then its buildings). Buildings are BuildingClass objects with original BuildingType foundations; other technos are Units on the UnitClass vtable sharing the aircraft\'s type pointer.',
        ],
        substitutions=[
            'Aircraft vt+0x480 Assign_Destination and vt+0x16C ReceiveDamage recorded with their arguments and returned without running; in Stun rows a NULL Assign_Destination runs its original body.',
            'Stun rows: vt+0x280 radio broadcast, vt+0xDC Detach_All and vt+0x150 deselect recorded with their arguments and returned without running.',
            'MapClass::GetZoneID 0x0056D230 answers 1 for every call; the calls Find_Nearest_Friendly_Airfield makes are recorded with their cell, movement zone and bridge flag.',
        ],
        scope='ILocomotion Stop_Moving of a living aircraft: not moving, pitch, Find_Attack_Cell around the cell under it (free and held by a building), Find_Nearest_Friendly_Airfield on Attack (Dock= weighting, list order, the own-cell search near a building, Limbo, the ObjectClass fallback and the own-cell answer), the edge cell outside the playfield (in a LocalSize margin and beyond the Size diamond) and the loaner exception, and the cell (0, 0) self-destruct; the death-arm Stun of a moving, attacking (with and without a TarCom) and still aircraft over free and building-held cells; and 0x00586AC0 over four Size/LocalSize rectangles. Excludes the arm for a Fly owner that is not an Aircraft, team members (vt+0x4DC through 0x006EC300), real zone tables, and cells beyond the MapClass cell array (the shared dummy cell, whose coordinate every later lookup restamps).',
    ))
