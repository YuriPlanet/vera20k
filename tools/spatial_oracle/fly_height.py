"""Original Fly vertical controller with real Aircraft/Unit and Object methods.

Runs the bounded4CDD0D..4CDFBC/4CE145 range. Process admission, horizontal
motion, crash relocation, descent drift and phase updates are outside coverage.
Supplied native phase bytes are never inferred from VERA's legacy AirMovePhase.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.crate_speed_effect import fixture, CELL, RULES
from tools.spatial_oracle.map_queries import dwords, EMPTY_TABLE, TABLE, GLOBAL_TABLE

LOCO = SCRATCH + 0x1B000


def execute(case):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind=case.get('kind', 'aircraft'))]))
    owner = actors[0]
    object_type = owner + 0x800
    u.mem_write(TABLE, EMPTY_TABLE)
    u.mem_write(GLOBAL_TABLE, dwords(TABLE))
    u.mem_write(TABLE + (10 * 512 + 10) * 4, dwords(CELL))
    u.mem_write(CELL + 0x11B, bytes([case.get('level', 0), case.get('slope', 0)]))
    u.mem_write(CELL + 0x140, dwords(0x100 if case.get('bridge', False) else 0))
    for address, value in [(0xAC13C8, 104), (0xAC13BC, 416), (0xABC5DC, 416), (0x8B3CAC, 416)]:
        u.mem_write(address, dwords(value))
    u.mem_write(RULES + 0x7B4, dwords(1500))
    u.mem_write(object_type + 0x618, dwords(case.get('flight_level', -1)))
    u.mem_write(owner + 0x6C, dwords(case.get('health', 100)))
    # Marked=false avoids SetHeight's render-mark callbacks, not its coordinate
    # and slope/bridge arithmetic. Full marking transactions remain separate.
    u.mem_write(owner + 0x74, b'\0')
    u.mem_write(owner + 0x8C, bytes([int(case.get('on_bridge', False))]))
    u.mem_write(owner + 0x9C, dwords(2688, 2688, case['z']))
    if case.get('kind', 'aircraft') == 'aircraft':
        # Real Aircraft constructor413DA2 installs the auxiliary vtable here.
        # QueryInterface414290 returns this subobject for IID822410.
        u.mem_write(owner + 0x6C0, dwords(0x7E2250))
    u.mem_write(owner + 0x118, dwords(SCRATCH + 0x1C000 if case.get('loaded', False) else 0))
    u.mem_write(LOCO + 0xC, dwords(owner))
    u.mem_write(LOCO + 0x1C, dwords(2688, 2688, 0))
    u.mem_write(LOCO + 0x38, dwords(case['target']))
    u.mem_write(LOCO + 0x51, bytes([int(case.get('landing', False))]))
    u.mem_write(sp + 0x13, bytes([int(case.get('dropship', False))]))
    u.reg_write(UC_X86_REG_ESI, LOCO)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x4CDD0D, (0x4CDFBC, 0x4CE145), count=20000,
                required_addresses=[0x4CDD1A, 0x4CAC40, 0x7C5F00])
    assert u.reg_read(UC_X86_REG_ESP) == sp
    z = struct.unpack('<i', u.mem_read(owner + 0xA4, 4))[0]
    on_bridge = bool(u.mem_read(owner + 0x8C, 1)[0])
    u.mem_write(sp, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ECX, owner)
    run_checked(u, 0x5F5F40, RET_MAGIC, count=1000)
    height = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
    return dict(input=case, z=z, on_bridge=on_bridge, height=height)


def generate():
    cases = []
    for loaded in (False, True):
        for z in (0, 1, 10, 1480, 1489, 1490, 1499, 1500, 1501, 1519, 1520, 1900, 2500):
            cases.append(dict(name=f'ordinary_{loaded}_{z}', z=z, target=1500, loaded=loaded))
    for landing in (False, True):
        for z in (0, 1, 6, 16, 19, 20, 49, 50, 199, 200, 759, 760, 1500, 1501, 1515):
            for target in (0, 1500):
                cases.append(dict(name=f'dropship_{landing}_{z}_{target}', z=z, target=target,
                                  dropship=True, landing=landing))
    for z in (0, 1, 19, 20, 49, 50, 99, 100, 415, 416, 417, 435, 436, 466, 1916):
        cases.append(dict(name=f'ordinary_land_{z}', z=z, target=0))
        cases.append(dict(name=f'bridge_land_{z}', z=z, target=0, bridge=True))
    cases += [dict(name='bridge_takeoff', z=416, target=1500, bridge=True, on_bridge=True),
              dict(name='bridge_loaded_takeoff', z=416, target=1500, bridge=True, on_bridge=True, loaded=True),
              dict(name='bridge_already_attached_descent', z=417, target=0, bridge=True, on_bridge=True),
              dict(name='slope_climb', z=260, target=1500, level=2, slope=1),
              dict(name='slope_land', z=261, target=0, level=2, slope=1),
              dict(name='unit_no_aircraft_interface', kind='unit', z=0, target=1500, loaded=True),
              dict(name='dropship_type_override', z=1600, target=1500, dropship=True, flight_level=1500),
              dict(name='dropship_distinct_type_override', z=1600, target=1500, dropship=True, flight_level=2200)]
    for health in (0, -1):
        for z in (0, 1, 19, 20, 50, 1500):
            cases.append(dict(name=f'health_{health}_{z}', z=z, target=1500, health=health))
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'height_start': 0x4CDD0D, 'after_height': 0x4CDFBC,
                      'no_descent_end': 0x4CE145, 'get_height': 0x5F5F40,
                      'set_height': 0x5F5FA0, 'aircraft_query_interface': 0x414290,
                      'has_passenger': 0x41B7D0, 'type_flight_level': 0x717800},
        assumptions=['Supplied post-horizontal frame, target height, full-locomotor landing byte+51 and Type.IsDropship at its captured stack+13 location.',
                     'Real Aircraft/Unit vtables and Aircraft auxiliary interface; FirstPassenger is a supplied nonnull/zero link, never dereferenced by the query.',
                     'Unmarked owner and one real map cell;104/416 terrain scalars and416 Fly bridge adjustment supplied. No render Mark callbacks execute.',
                     'Destination XY equals owner XY; original distance calculation runs. PC53/chop ambient state.',
                     'Health0 and negative cases test this bounded range only, not admission or the earlier fall/crash controller.'],
        substitutions=[],
        scope='136 original vertical steps: unloaded/loaded climb, descent overshoot, IsDropship and landing differences, bridge attachment, ramps, type/global FlightLevel and health. Excludes full Process, preceding XY/crash relocation, following descent drift, speed, phase, sound/animation and Display transactions; no Rust parity claim.',
    ))
