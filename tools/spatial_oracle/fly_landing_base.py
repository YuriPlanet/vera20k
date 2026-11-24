"""Original Carryall reader and Fly MoveTo landing-base/takeoff decision.

Executes4CCE71..4CCED1/4CCED9, stopping before BeginTakeoff or the following
destination-mode logic. Real QueryInterface, radio, mission, RTTI and height
queries execute unchanged. This does not establish full MoveTo behavior.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI,
    UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, INI, SP
from tools.spatial_oracle.crate_speed_effect import fixture, CELL
from tools.spatial_oracle.map_queries import dwords, packed, EMPTY_TABLE, TABLE, GLOBAL_TABLE, DUMMY

LOCO, CONTACTS = SCRATCH + 0x1B000, SCRATCH + 0x19000


def reader(raw):
    f = Fixture()
    f.ini(0x818028, raw)
    u = f.u
    # AircraftType ctor41C8C3/41C8D0 seeds Carryall=false.
    u.mem_write(TYPE + 0xDFC, b'\0')
    u.reg_write(UC_X86_REG_ESI, TYPE)
    u.reg_write(UC_X86_REG_EBX, INI)
    u.reg_write(UC_X86_REG_EDI, TYPE + 0x1F8)
    u.reg_write(UC_X86_REG_ESP, SP)
    # The intervening pushes prepare the next key; stop before its ReadBool.
    run_checked(u, 0x41CC9B, 0x41CCCD, count=10000,
                required_addresses=[0x5295F0, 0x41CCC7])
    assert u.reg_read(UC_X86_REG_ESP) == SP - 12
    return dict(raw=raw, carryall=bool(u.mem_read(TYPE + 0xDFC, 1)[0]))


def execute(case):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind=case.get('kind', 'aircraft'))]))
    owner = actors[0]
    u.mem_write(TABLE, EMPTY_TABLE)
    u.mem_write(GLOBAL_TABLE, dwords(TABLE, 0x40000))
    if not case.get('missing_cell', False):
        u.mem_write(TABLE + (10 * 512 + 10) * 4, dwords(CELL))
    u.mem_write(DUMMY + 0x24, packed(-7, -8))
    u.mem_write(CELL + 0x11B, bytes([case.get('level', 0), case.get('slope', 0)]))
    u.mem_write(0xAC13C8, dwords(104))
    u.mem_write(0xAC13BC, dwords(416))
    u.mem_write(owner + 0x6C, dwords(case.get('health', 100)))
    u.mem_write(owner + 0x8C, bytes([int(case.get('on_bridge', False))]))
    u.mem_write(owner + 0xA4, dwords(case.get('z', 100)))
    u.mem_write(owner + 0xAC, dwords(case.get('current', 7)))
    u.mem_write(owner + 0xB4, dwords(case.get('queued', -1)))
    u.mem_write(owner + 0x118, dwords(SCRATCH + 0x1C000 if case.get('loaded') else 0))
    u.mem_write(owner + 0x800 + 0xDFC, bytes([int(case.get('carryall', False))]))
    if case.get('kind', 'aircraft') == 'aircraft':
        u.mem_write(owner + 0x6C0, dwords(0x7E2250))
    pointers = []
    for i, contact in enumerate(case.get('contacts', [])):
        if contact is None:
            pointers.append(0)
            continue
        actor = SCRATCH + 0xD000 + i * 0x3000
        pointers.append(actor)
        u.mem_write(actor, dwords(0x7F5C70 if contact == 'unit' else 0x7E3EBC))
        u.mem_write(actor + 0x520, dwords(actor + 0x800))
        u.mem_write(actor + 0x800 + 0x16A9, bytes([int(contact == 'repair')]))
        u.mem_write(actor + 0x800 + 0x16CB, bytes([int(contact == 'helipad')]))
    u.mem_write(CONTACTS, dwords(*pointers))
    u.mem_write(owner + 0xE4, dwords(CONTACTS, len(pointers)))
    u.reg_write(UC_X86_REG_ECX, LOCO)
    u.reg_write(UC_X86_REG_ESP, sp)
    u.mem_write(sp, dwords(RET_MAGIC))
    run_checked(u, 0x4CC9A0, RET_MAGIC, count=1000)
    u.mem_write(LOCO + 0xC, dwords(owner))
    u.mem_write(LOCO + 0x50, bytes([int(case.get('taking_off', False)),
                                 int(case.get('landing', False))]))
    u.reg_write(UC_X86_REG_EBX, LOCO + 4)
    u.reg_write(UC_X86_REG_EDI, LOCO + 0xC)
    u.reg_write(UC_X86_REG_ESP, sp)
    def observe(_u, address, _size, _data):
        if address == 0x47B3A0:
            expected = DUMMY if case.get('missing_cell', False) else CELL
            assert u.reg_read(UC_X86_REG_ECX) == expected, 'height query selected wrong cell'
    u.hook_add(UC_HOOK_CODE, observe)
    run_checked(u, 0x4CCE71, 0x4CCEAA, count=10000)
    base = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_ESI)))[0]
    run_checked(u, 0x4CCEAA, (0x4CCED1, 0x4CCED9), count=10000)
    assert u.reg_read(UC_X86_REG_ESP) == sp
    return dict(input=case, landing_base=base,
                begin_takeoff=u.reg_read(UC_X86_REG_EIP) == 0x4CCED1,
                dummy_coord=list(struct.unpack('<hh', u.mem_read(DUMMY + 0x24, 4))))


def generate():
    cases = []
    for carryall in (False, True):
        for loaded in (False, True):
            for contacts in ([], [None], ['unit'], ['plain'], ['repair'], ['helipad'],
                             [None, 'repair'], ['unit', 'repair'], ['repair', None]):
                for current, queued in ((7, -1), (5, 7), (-1, 7), (16, 7), (-1, 5)):
                    cases.append(dict(carryall=carryall, loaded=loaded, contacts=contacts,
                                      current=current, queued=queued))
    for carryall in (False, True):
        for z in (-1, 0, 1, 99, 100, 101, 40000):
            for taking_off, landing in ((False, False), (True, False), (False, True), (True, True)):
                cases.append(dict(carryall=carryall, loaded=True, z=z,
                                  taking_off=taking_off, landing=landing))
    for health in (0, -1):
        cases.append(dict(carryall=True, loaded=True, health=health, landing=True))
    cases += [dict(kind='unit', carryall=True, loaded=True),
              dict(carryall=True, loaded=True, z=360, level=2, slope=1),
              dict(carryall=True, loaded=True, z=361, level=2, slope=1),
              dict(carryall=True, loaded=True, z=516, on_bridge=True),
              dict(carryall=True, loaded=True, z=517, on_bridge=True)]
    for health, taking_off, landing in ((100, False, False), (100, True, False),
                                       (100, False, True), (100, True, True),
                                       (0, False, False), (-1, False, False)):
        cases.append(dict(carryall=True, loaded=True, missing_cell=True, health=health,
                          taking_off=taking_off, landing=landing))
    return dict(rules=[reader(raw) for raw in (None, '', 'yes', 'no', 'true', 'false', '1', '0', 'junk')],
                moves=[execute(case) for case in cases])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'carryall_reader': 0x41CC9B, 'landing_base': 0x41B6A0,
                      'move_query': 0x4CCE71, 'takeoff_decision': 0x4CCEAA,
                      'aircraft_query_interface': 0x414290, 'has_contact': 0x65AE30,
                      'contact_at_slot': 0x65AD40, 'effective_mission': 0x5B3040,
                      'get_height': 0x5F5F40},
        assumptions=['Cached INI entries supplied; original Carryall reader and constructor-default false are used.',
                     'Real Aircraft/Unit vtables, constructed Fly locomotor, supplied Aircraft auxiliary interface and sparse radio slots. Contact Building types supply UnitRepair+16A9/Helipad+16CB, as proven by460929/4604E0 readers.',
                     'Health, exact coordinates, current/queued mission, cargo head and locomotor phase bytes are supplied. No cargo object is dereferenced.',
                     'Map table AND length plus one real cell10,10 are initialized. Six missing-cell cases instead check the retained Dummy coordinate stamp and short-circuit query order; read-only observers assert the expected real/Dummy identity.104/416 ground/bridge scalars supplied.'],
        substitutions=[],
        scope='Nine Carryall reads and249 MoveTo landing-base/takeoff decisions across cargo, sparse radio slots, Enter/queued mission, building type, health, phase, height, ramps, OnBridge and Dummy query cadence. Stops before BeginTakeoff: excludes preceding MoveTo refusal/destination writes, subsequent mode logic, phase callbacks and complete flight/combat behavior.',
    ))
