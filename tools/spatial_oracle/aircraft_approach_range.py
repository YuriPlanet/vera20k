"""Original Aircraft Mission_Attack range branch, with real virtual receivers.

The caller has already selected the range branch at 4180F4. Execute GetWeapon(0),
Object::Distance_To and its original coordinate/foundation/math callees, stopping
at the two range successors before mission navigation or firing. No substitutions.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.crate_speed_effect import fixture
from tools.spatial_oracle.map_queries import dwords, packed

WEAPON, ELITE_WEAPON, TARGET, TYPE = [SCRATCH + offset for offset in
                                      (0x18000, 0x19000, 0x1A000, 0x1B000)]


def execute(case):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind='aircraft')]))
    owner = actors[0]
    owner_type = owner + 0x800
    u.mem_write(owner + 0x9C, dwords(*case['source']))
    u.mem_write(owner + 0x150, struct.pack('<f', case.get('veterancy', 0)))
    u.mem_write(owner_type + 0x898, dwords(WEAPON))
    u.mem_write(owner_type + 0xA94, dwords(ELITE_WEAPON if case.get('elite_weapon', True) else 0))
    u.mem_write(WEAPON + 0xB4, dwords(case.get('range', 1536)))
    u.mem_write(ELITE_WEAPON + 0xB4, dwords(case.get('elite_range', 2304)))
    kind = case.get('kind', 'unit')
    u.mem_write(TARGET, dwords({'unit': 0x7F5C70, 'building': 0x7E3EBC, 'cell': 0x7E4EEC}[kind]))
    u.mem_write(TARGET + 0x9C, dwords(*case['target']))
    dimensions = None
    if kind == 'building':
        u.mem_write(TARGET + 0x520, dwords(TYPE))
        foundation = case.get('foundation', 0)
        u.mem_write(TYPE + 0xEF0, dwords(foundation))
        u.mem_write(TYPE + 0x1570, bytes([int(case.get('bib', False))]))
        dimensions = [struct.unpack('<i', u.mem_read(table + foundation * 4, 4))[0]
                      for table in (0x8192B8, 0x819310)]
    elif kind == 'cell':
        u.mem_write(TARGET + 0x24, packed(*case['cell']))
    u.mem_write(owner + 0x2B4, dwords(TARGET))
    # Execute the complete read-only distance method before the calling branch.
    before_owner = bytes(u.mem_read(owner, 0x800))
    before_target = bytes(u.mem_read(TARGET, 0x800))
    u.mem_write(sp, dwords(RET_MAGIC, TARGET))
    u.reg_write(UC_X86_REG_ECX, owner)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x5F6440, RET_MAGIC, count=20000,
                required_addresses=[0x4CAC40, 0x7C5F00])
    distance = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
    assert u.reg_read(UC_X86_REG_ESP) == sp + 8
    observed = {}

    def observe(_u, address, _size, _data):
        if address == 0x4180FF:
            observed['weapon'] = 'elite' if u.reg_read(UC_X86_REG_EAX) == owner_type + 0xA94 else 'primary'

    u.hook_add(UC_HOOK_CODE, observe)
    u.reg_write(UC_X86_REG_ESI, owner)
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_ESP, sp)
    successor = run_checked(u, 0x4180F4, (0x418117, 0x418131), count=20000,
                            required_addresses=[0x70E140, 0x5F6440, 0x41810F])
    observed['in_range'] = successor == 0x418117
    assert u.reg_read(UC_X86_REG_ESP) == sp
    assert bytes(u.mem_read(owner, 0x800)) == before_owner
    assert bytes(u.mem_read(TARGET, 0x800)) == before_target
    return dict(input=case, foundation_dimensions=dimensions, distance=distance, **observed)


def generate():
    cases = []
    source = [2688, 2688, 1500]
    for veterancy, elite_weapon in ((0, True), (1, True), (2, True), (2, False)):
        for dx, dy in ((0, 0), (1, 0), (1281, 0), (1535, 0), (1536, 0), (1537, 0),
                       (2303, 0), (2304, 0), (2305, 0), (1536, 1536), (2304, 2304),
                       (1086, 1086), (1087, 1087), (-1536, 0), (0, -1536)):
            cases.append(dict(source=source, target=[2688 + dx, 2688 + dy, 0],
                              veterancy=veterancy, elite_weapon=elite_weapon))
    for foundation in range(22):
        for bib in (False, True):
            for delta in (0, 1536, 2304):
                cases.append(dict(source=source, target=[2688 + delta, 2688, 9000],
                                  kind='building', foundation=foundation, bib=bib))
    for cell in ([10, 10], [16, 10], [19, 10], [16, 16]):
        cases.append(dict(source=source, target=[0, 0, 0], kind='cell', cell=cell))
    for distance in (0, 1, 2, 127, 128, 129, 512, 513, 1280, 1281, 1282, 12801, 130000):
        for delta in (-1, 0, 1):
            cases.append(dict(source=source, target=[2688 + distance, 2688, 0], range=distance + delta))
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'aircraft_range': 0x4180F4, 'distance_to': 0x5F6440,
                      'get_weapon': 0x70E140, 'building_coords': 0x447AC0,
                      'foundation_width': 0x45EC90, 'foundation_height': 0x45ECA0,
                      'sqrt': 0x4CAC40, 'ftol': 0x7C5F00},
        assumptions=['Original Aircraft/Unit/Building/Cell vtables; supplied physical XYZ and native primary/elite weapon slots.',
                     'All 22 original foundation entries, with Bib both ways; immutable dimension arrays retained.',
                     'PC53/chop 0E7F; flat Cell target; source altitude and target altitude differ deliberately.',
                     'Entry after auxiliary+18 has selected the range branch. Does not establish that admission predicate.'],
        substitutions=[],
        scope='235 original full Distance_To queries and Aircraft range branches, including primary/elite fallback, native sqrt ties, strict signed range boundary, cell targets and building centre/discount/clamp. Stops before navigation or firing; excludes mission scheduling and other attack-state branches.',
    ))
