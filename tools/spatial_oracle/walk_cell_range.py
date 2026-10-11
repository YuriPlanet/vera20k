"""Original Coord->Cell range wrapper and Cell range geometry/query order.

Rows with `target_object` range an Infantry object through CanFireAt 0x6F77B0
instead. Rows with `subject_to_elevation` add InRange's elevation bonus
(0x6F6F60 on both arms, 0x6F70E0 on the arcing arm). Rows that supply
`elevation` or `arcing` also record `bonuses`, each bonus InRange computed.
Arcing rows run the arc test (Ballistic_Launch_Speed 0x48AB90, Can_Reach
0x48ABC0) under the row's `gravity` and `floater`, then the bridge ceiling.

Rows with `building` use the original Building vtable, GetCoords 0x447AC0,
GetWeapon 0x4526F0, IsOccupied 0x458DD0 and HalfFoundation 0x458E00. Their
`source` is Building Location; `source_geometry` observes GetCoords, and the
range event observes the final source passed to InRange. OccupyWeaponRange
is supplied by `occupy_range` (default 5), independently of the weapon Range.

The weapon and object fields are supplied. Infantry rows supply GetWeapon's
slot; Building rows execute it. No range verdict, coordinate getter, map lookup,
distance calculation or line-of-fire callable is substituted.

--bunker writes/checks a separate corpus; the historical default payload and
identity sidecar stay untouched. Set VERA20K_BUNKER_ASSETS to an extraction
directory containing RULESMD.INI and MPBattleMD.ini, plus LANGRULE.INI if present.
The final map-rule layer is explicitly empty. No map file, ART, SHP or sound assets are
required. Its original readers provide LTNK/ATGUN/Cannon and Rules inputs,
followed by Unit/Building range controls. No Rust numeric output is used as a
native expected value. The empty layer is a retained-reader control, not a
complete map replay; see tools/native_oracle.md for extraction requirements.
"""
from pathlib import Path
import argparse
import hashlib
import os
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_FPCW
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed

ACTOR, TYPE, HOUSE, VT, WEAPON, SLOT, PROJECTILE, CELLS, COORD, RULES, TARGET = [
    SCRATCH + i * 0x2000 for i in range(11)]
MAP, TABLE, DUMMY = 0x87F7E8, 0xC00000, 0xABDC50
TARGET_TYPE, OCCUPANT, OCCUPANT_TYPE, OCCUPANTS = [
    SCRATCH + offset for offset in (0x16000, 0x18000, 0x1A000, 0x1C000)]
INFANTRY_VTABLE, BUILDING_VTABLE = 0x7EB058, 0x7E3EBC
UNIT_VTABLE = 0x7F5C70


def query(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x20000)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    def read32(p): return struct.unpack('<I', u.mem_read(p, 4))[0]
    def xyz(p): return list(struct.unpack('<iii', u.mem_read(p, 12)))
    def cell_coord(p): return list(struct.unpack('<hh', u.mem_read(p + 0x24, 4)))
    def ret(cleanup, result):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, result & 0xffffffff)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + cleanup + 4)
    building = row.get('building')
    source_vtable = BUILDING_VTABLE if building is not None else (
        UNIT_VTABLE if row.get('unit') else INFANTRY_VTABLE)
    u.mem_write(VT, bytes(u.mem_read(source_vtable, 0x600)))
    u.mem_write(ACTOR, dwords(VT))
    u.mem_write(ACTOR + 0x6C0, dwords(TYPE))
    if row.get('unit'):
        u.mem_write(ACTOR + 0x6C4, dwords(TYPE))
    u.mem_write(ACTOR + 0x21C, dwords(HOUSE))
    u.mem_write(ACTOR + 0x9C, dwords(*row.get('source', [2624, 2624, 0])))
    u.mem_write(ACTOR + 0x74, bytes([int(row.get('marked', False))]))
    u.mem_write(ACTOR + 0x8C, bytes([int(row.get('on_bridge', False))]))
    # InRange 0x6F72C8: +0x82 (in an open-topped transport) adds Rules+0xF5C << 8.
    u.mem_write(ACTOR + 0x82, bytes([int(row.get('open_topped', False))]))
    # 6F72A2..6F72C8 reads only the presence of +2E4 and the actor's RTTI;
    # the pointed-to host is deliberately not given a substitute body.
    u.mem_write(ACTOR + 0x2E4, dwords(OCCUPANTS if row.get('bunkered') else 0))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0xF54, dwords(row.get('bunker_bonus', 0)))
    u.mem_write(RULES + 0xF5C, dwords(row.get('open_topped_bonus', 2)))
    u.mem_write(RULES + 0xF48, dwords(row.get('occupy_range', 5)))
    u.mem_write(TYPE + 0x68C, dwords(row.get('air_range_bonus', 0)))
    if building is not None:
        u.mem_write(ACTOR + 0x14, b'\x03')
        u.mem_write(ACTOR + 0x520, dwords(TYPE))
        u.mem_write(ACTOR + 0x688, dwords(OCCUPANTS))
        count = building.get('occupants', 1)
        u.mem_write(ACTOR + 0x694, dwords(count))
        u.mem_write(ACTOR + 0x69C, dwords(building.get('fire_index', 0)))
        u.mem_write(TYPE + 0xEF0, dwords(building.get('foundation_id', 3)))
        u.mem_write(TYPE + 0x157B, bytes([int(building.get('can_be_occupied', True))]))
        u.mem_write(TYPE + 0x157C, bytes([int(building.get('can_occupy_fire', True))]))
        # Every supplied occupant points at the same Infantry fixture. Its
        # OccupyWeapon and the building's own Primary share this row's weapon;
        # GetWeapon itself still runs on both the occupied and fallback routes.
        u.mem_write(OCCUPANTS, dwords(*([OCCUPANT] * count)))
        u.mem_write(OCCUPANT, dwords(INFANTRY_VTABLE))
        u.mem_write(OCCUPANT + 0x6C0, dwords(OCCUPANT_TYPE))
        u.mem_write(OCCUPANT_TYPE + 0xE04, dwords(WEAPON))
        u.mem_write(TYPE + 0x898, dwords(WEAPON))
    # [ElevationModel] Rules+0x1838/+0x1840/+0x1848, the constructor's 0/1.0/0.0
    # unless supplied, and BulletType+0x297 SubjectToElevation / +0x29B Arcing.
    increment, increment_bonus, cap = row.get('elevation', [0, 1.0, 0.0])
    u.mem_write(RULES + 0x1838, dwords(increment))
    u.mem_write(RULES + 0x1840, struct.pack('<dd', increment_bonus, cap))
    u.mem_write(PROJECTILE + 0x297, bytes([int(row.get('subject_to_elevation', False))]))
    u.mem_write(PROJECTILE + 0x29B, bytes([int(row.get('arcing', False))]))
    # The arcing arm's arc test reads Rules+0x16B8 Gravity (0 unless supplied),
    # halved through 0x48ACF0 for a BulletType+0x295 Floater.
    u.mem_write(RULES + 0x16B8, dwords(row.get('gravity', 0)))
    u.mem_write(PROJECTILE + 0x295, bytes([int(row.get('floater', False))]))
    u.mem_write(WEAPON + 0xA0, dwords(PROJECTILE))
    u.mem_write(WEAPON + 0xB4, dwords(row.get('range', 768)))
    u.mem_write(WEAPON + 0xB8, dwords(row.get('minimum', 0)))
    u.mem_write(WEAPON + 0x134, bytes([int(row.get('cell_rangefinding', False))]))
    u.mem_write(SLOT, dwords(WEAPON))
    u.mem_write(COORD, dwords(*row.get('target', [3036, 2780, 123])))
    u.mem_write(0x89E7C0, dwords(104))
    u.mem_write(0xAC13C8, dwords(104))
    u.mem_write(0xAC13BC, dwords(416))
    u.mem_write(0xB0EB24, dwords(416))
    u.mem_write(0xB0EB34, dwords(104))
    u.mem_write(0xAA0738, dwords(row.get('water_base', 314)))
    table = bytearray(0x100000)
    def put_cell(p, c):
        u.mem_write(p, dwords(0x7E4EEC))
        u.mem_write(p + 0x24, packed(*c['coord']))
        u.mem_write(p + 0x38, dwords(c.get('tile', 0)))
        u.mem_write(p + 0x11B, bytes([c.get('level', 0) & 255, c.get('slope', 0)]))
        u.mem_write(p + 0x140, dwords(c.get('flags', 0)))
    cells = row.get('cells', [{'coord': [10, 10]}, {'coord': [11, 10]}])
    for i, c in enumerate(cells):
        p = CELLS + i * 0x200
        put_cell(p, c)
        x, y = c.get('slot', c['coord'])
        struct.pack_into('<I', table, (y * 512 + x) * 4, p)
    put_cell(DUMMY, {'coord': [99, 98], 'tile': 65535, **row.get('dummy', {})})
    u.mem_write(TABLE, bytes(table))
    target_object = row.get('target_object')
    if target_object is not None:
        target_vtable = INFANTRY_VTABLE if building is not None or row.get('unit') else VT
        u.mem_write(TARGET, dwords(target_vtable))
        u.mem_write(TARGET + 0x6C0, dwords(TARGET_TYPE if building is not None or row.get('unit') else TYPE))
        u.mem_write(TARGET + 0x21C, dwords(HOUSE))
        u.mem_write(TARGET + 0x9C, dwords(*target_object['location']))
        u.mem_write(TARGET + 0x74, bytes([int(target_object.get('marked', True))]))
        u.mem_write(TARGET + 0x8C, bytes([int(target_object.get('on_bridge', False))]))
    u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    events = []
    geometry = None
    source_geometry = None
    bonuses = []
    range_stages = []
    def observe(_u, address, _size, _data):
        nonlocal geometry, source_geometry
        sp = u.reg_read(UC_X86_REG_ESP)
        if building is not None and address == 0x6F77D6:
            source_geometry = xyz(u.reg_read(UC_X86_REG_EAX))
        if row.get('trace_range_stages') and address in (0x6F72A2, 0x6F72C8, 0x6F72E3):
            stage = {0x6F72A2: 'before_bunker', 0x6F72C8: 'after_bunker',
                     0x6F72E3: 'after_open_topped'}[address]
            value = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EDI)))[0]
            range_stages.append([stage, value])
        # Returns of the direct (0x6F72FF) and arcing (0x6F746B) bonus calls.
        if address in (0x6F7304, 0x6F7470):
            bonus = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
            bonuses.append(['direct' if address == 0x6F7304 else 'arcing', bonus])
        if address == read32(VT + 0x3F8):
            events.append(['weapon', read32(sp + 4)])
            if building is None:
                ret(4, SLOT)
        elif address in (0x565730, 0x578080):
            events.append([hex(address), xyz(read32(sp + 4))])
        elif address == 0x5657A0:
            events.append([hex(address), list(struct.unpack('<hh', u.mem_read(read32(sp + 4), 4)))])
        elif address in (0x486840, 0x4867E0):
            p = u.reg_read(UC_X86_REG_ECX)
            events.append([hex(address), p == DUMMY, cell_coord(p)])
        elif address == 0x47B3A0:
            p = u.reg_read(UC_X86_REG_ECX)
            events.append([hex(address), p == DUMMY, cell_coord(p)])
        elif address == 0x6F7220:
            p = read32(sp + 8)
            events.append(['range', xyz(read32(sp + 4)), p == DUMMY, cell_coord(p)])
        elif address == 0x6F7379:
            geometry = xyz(sp + 0x20)
        elif address == 0x4CC310:
            events.append(['line', xyz(u.reg_read(UC_X86_REG_ECX)), xyz(u.reg_read(UC_X86_REG_EDX))])
    u.hook_add(UC_HOOK_CODE, observe)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    entry, target = (0x6F77B0, TARGET) if target_object is not None else (0x6F7970, COORD)
    u.mem_write(sp, dwords(RET_MAGIC, target, 0))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, ACTOR)
    required = [entry, 0x6F77B0, 0x6F7220]
    if row.get('trace_range_stages') and row.get('range', 768) != -512:
        required += [0x6F72A2, 0x6F72C8, 0x6F72E3]
        if row.get('bunkered'):
            required += [0x6F72B3]
            if building is None:
                required += [0x6F72C6]
    if building is not None:
        required += [0x447AC0, 0x4526F0]
        if row.get('range', 768) != -512:
            required += [0x458DD0, 0x6F727E]
    run_checked(u, entry, RET_MAGIC, count=100000, required_addresses=required)
    assert u.reg_read(UC_X86_REG_ESP) == sp + 12
    out = {'input': row, 'result': bool(u.reg_read(UC_X86_REG_EAX) & 255), 'target_geometry': geometry, 'events': events, 'dummy_coord': cell_coord(DUMMY)}
    if 'elevation' in row or row.get('arcing'):
        out.update(bonuses=bonuses)
    if building is not None:
        out.update(source_geometry=source_geometry)
    if row.get('trace_range_stages'):
        out.update(range_stages=range_stages)
    return out


def generate():
    rows = [
        {},
        {'range': 326}, {'range': 325}, {'minimum': 326}, {'minimum': 327},
        {'range': -512, 'target': [3550, 2780, 0], 'cells': []},
        {'target': [3550, 2780, 0], 'range': 1024},
        {'target': [3550, 2780, 0], 'range': 1024, 'dummy': {'level': 2, 'slope': 1}},
        {'target': [3550, 2780, 0], 'range': 1024, 'dummy': {'level': -1}},
        # The first missing target and subsequent missing shooter cell share
        # one pointer. Source recentering overwrites the target's coordinates.
        {'target': [3550, 2780, 0], 'source': [5184, 2624, 0], 'cell_rangefinding': True, 'range': 1, 'dummy': {'level': 2}},
        {'target': [-257, 2780, 0], 'source': [128, 2688, 0], 'range': 1024,
         'cells': [{'coord': [0, 10], 'level': 3, 'flags': 256}], 'dummy': {'level': 1}},
        {'target': [-130816, 512, 0], 'source': [384, 384, 0], 'range': 1,
         'cells': [{'coord': [1, 1]}]},
        {'cell_rangefinding': True},
        {'source': [2624, 2624, 800], 'marked': True},
        {'source': [2624, 2624, 800], 'marked': True, 'cell_rangefinding': True},
        {'cells': [{'coord': [10, 10], 'flags': 256}, {'coord': [11, 10], 'flags': 256}]},
        {'cells': [{'coord': [10, 10], 'flags': 256}, {'coord': [11, 10], 'flags': 256}], 'cell_rangefinding': True, 'on_bridge': True},
        # +0x82: the target cell's centre is 326 leptons away, so a 70-lepton
        # weapon reaches it exactly with one bonus cell and misses it by one
        # lepton at 69; the bonus needs the flag, and the retail two cells
        # lift a 0-lepton weapon past it.
        {'range': 70, 'open_topped': True, 'open_topped_bonus': 1},
        {'range': 69, 'open_topped': True, 'open_topped_bonus': 1},
        {'range': 70, 'open_topped_bonus': 1},
        {'range': 0, 'open_topped': True},
        {'range': 326, 'open_topped': True, 'open_topped_bonus': 0},
        {'range': 582, 'open_topped': True, 'open_topped_bonus': -1},
        {'range': 581, 'open_topped': True, 'open_topped_bonus': -1},
        {'minimum': 327, 'open_topped': True},
    ]
    for tile in [313, 314, 327, 328, -1]:
        rows.append({'cells': [{'coord': [10, 10]}, {'coord': [11, 10], 'tile': tile, 'flags': 256, 'level': 1}], 'range': 500})
    for base, tile in [(-1, 0), (-1, 13), (2147483640, 2147483640)]:
        rows.append({'water_base': base, 'cells': [{'coord': [10, 10]}, {'coord': [11, 10], 'tile': tile, 'flags': 256}], 'range': 400})
    # InRange's elevation bonus: the actor at cell (10,10) on `level` shoots at
    # cell (13,10). Each case runs at the range where the distance equals the
    # range plus the bonus, and one lepton short of it.
    retail = [4, 2.0, 2.0]
    def elevated(level, target_level=0, source_flags=0, target_tile=0):
        return {'source': [2624, 2624, level * 104], 'target': [3550, 2780, 0], 'marked': True,
                'elevation': retail, 'subject_to_elevation': True,
                'cells': [{'coord': [10, 10], 'level': level, 'flags': source_flags},
                          {'coord': [13, 10], 'level': target_level, 'tile': target_tile}]}
    infantry = {'source': [2624, 2624, 416], 'marked': True, 'elevation': retail, 'subject_to_elevation': True,
                'cells': [{'coord': [10, 10], 'level': 4}, {'coord': [13, 10]}]}
    for edge, case in [
        (273, elevated(4)),
        (932, {**elevated(4), 'subject_to_elevation': False}),
        (578, elevated(3)),
        (202, elevated(8)),
        (932, elevated(0, target_level=4)),
        (932, {**elevated(4), 'marked': False}),
        (49, {**elevated(7), 'elevation': [3, 1.5, 4.0]}),
        (234, {**elevated(6), 'elevation': [2, 0.7, 10.0]}),
        (273, {**elevated(4), 'elevation': [-4, 2.0, 2.0]}),
        (175, elevated(0, source_flags=0x80)),
        (932, elevated(4, target_tile=314)),
        (234, {**elevated(4), 'cells': [{'coord': [10, 10], 'level': 4}], 'dummy': {'level': -2}}),
        # Truncation toward zero takes the Dummy's centre (-128) into cell 0.
        (256, {'target': [-257, 2780, 0], 'source': [128, 2688, 312], 'marked': True, 'elevation': retail,
               'subject_to_elevation': True, 'cells': [{'coord': [0, 10], 'level': 3}], 'dummy': {'level': 1}}),
        (322, {**elevated(4), 'arcing': True}),
        (1346, {**elevated(4), 'arcing': True, 'elevation': [-4, 2.0, 2.0]}),
        # Sqrt_Approx gives 254 for a 255-lepton 2-D distance.
        (254, {'source': [2689, 2688, 0], 'marked': True, 'arcing': True}),
        (273, {**infantry, 'target_object': {'location': [3456, 2688, 0]}}),
        (932, {**infantry, 'target_object': {'location': [3456, 2688, 0], 'marked': False}}),
    ]:
        rows += [{**case, 'range': edge}, {**case, 'range': edge - 1}]
    # The arcing arm's arc test: the speed the range gives must reach the
    # target's height. The actor stands 834 leptons from cell (13,10)'s centre;
    # each case runs at the least range the native test admits, and one less.
    def arc(target_level, **extra):
        return {'arcing': True, 'gravity': 6, 'target': [3550, 2780, 0],
                'cells': [{'coord': [10, 10]}, {'coord': [13, 10], 'level': target_level}], **extra}
    for edge, case in [
        (834, arc(0)),
        (912, arc(2)),
        (1004, arc(3)),
        (1126, arc(4)),
        (903, arc(2, floater=True)),
        (834, arc(2, gravity=-6)),
        (912, arc(0, gravity=-6, source=[2624, 2624, 208])),
        (322, {**elevated(4), 'arcing': True, 'gravity': 6}),
        # No 2-D distance: Can_Reach measures 0.001 leptons.
        (701, arc(4, source=[3456, 2688, 0])),
        # An Infantry target two levels up, snapped to its cell's ground.
        (912, arc(2, target_object={'location': [3456, 2688, 208]})),
    ]:
        rows += [{**case, 'range': edge}, {**case, 'range': edge - 1}]
    # The bridge ceiling: a target point whose cell carries the bridge bit
    # (snapped onto the deck, 416 up) is refused from 312 leptons below.
    def ceiling(source_z, gravity=0, flags=256, rng=834, level=0):
        return {'arcing': True, 'gravity': gravity, 'range': rng, 'target': [3550, 2780, 0],
                'source': [2624, 2624, source_z],
                'cells': [{'coord': [10, 10]}, {'coord': [13, 10], 'level': level, 'flags': flags}]}
    rows += [
        ceiling(104), ceiling(105), ceiling(104, flags=0, level=4),
        ceiling(104, gravity=6, rng=1100), ceiling(208, gravity=6, rng=1100),
        # The Dummy's centre (-128) truncates into real cell (0,10), whose bit decides.
        {'arcing': True, 'target': [-257, 2780, 0], 'source': [128, 2688, 0], 'range': 1024,
         'cells': [{'coord': [0, 10], 'level': 3, 'flags': 256}], 'dummy': {'level': 1, 'flags': 256}},
        {'arcing': True, 'target': [-257, 2780, 0], 'source': [128, 2688, 0], 'range': 1024,
         'cells': [{'coord': [0, 10], 'level': 3}], 'dummy': {'level': 1, 'flags': 256}},
    ]
    # Occupied Building controls keep the native GetCoords/weapon/occupancy/
    # foundation helpers live. A target marked false uses its exact Location;
    # marked high targets also exercise AirRangeBonus before its replacement.
    def occupied(name, foundation_id=3, **extra):
        return {'name': name, 'building': {'foundation_id': foundation_id},
                'source': [2688, 2688, 0], 'range': 256, **extra}
    rows += [
        occupied('occupied_short_weapon_reaches', target_object={'location': [4352, 2816, 0], 'marked': False}),
        occupied('occupied_short_weapon_edge', target_object={'location': [4353, 2816, 0], 'marked': False}),
        occupied('occupied_short_weapon_beyond', target_object={'location': [4354, 2816, 0], 'marked': False}),
        occupied('occupied_long_weapon_is_replaced', range=3072, target_object={'location': [4816, 2816, 0], 'marked': False}),
        occupied('occupied_minimum_still_applies', minimum=1800, target_object={'location': [4352, 2816, 0], 'marked': False}),
        occupied('occupied_always_in_range_precedes_override', range=-512, target_object={'location': [8000, 2816, 0], 'marked': False}),
        occupied('occupied_high_target_discards_air_bonus', air_range_bonus=2048,
                 target_object={'location': [4816, 2816, 416]},
                 cells=[{'coord': [11, 11]}, {'coord': [18, 11]}]),
        occupied('occupied_cell_from_foundation_centre', foundation_id=14,
                 target=[2688, 4480, 0], cells=[{'coord': [10, 12]}, {'coord': [10, 17]}]),
        occupied('occupied_cell_rangefinding_from_foundation_centre', cell_rangefinding=True,
                 target=[4480, 2944, 0], cells=[{'coord': [11, 11]}, {'coord': [17, 11]}]),
        occupied('occupied_cell_beyond_reach', target=[4480, 2944, 0],
                 cells=[{'coord': [11, 11]}, {'coord': [17, 11]}]),
        occupied('occupied_odd_shorter_side', foundation_id=7,
                 target_object={'location': [4352, 3200, 0], 'marked': False}),
        occupied('occupied_even_shorter_side', foundation_id=18,
                 target_object={'location': [4864, 3072, 0], 'marked': False}),
        occupied('occupied_zero_rule', occupy_range=0,
                 target_object={'location': [3072, 2816, 0], 'marked': False}),
        occupied('occupied_zero_rule_beyond', occupy_range=0,
                 target_object={'location': [3074, 2816, 0], 'marked': False}),
        occupied('occupied_negative_rule', occupy_range=-2,
                 target_object={'location': [2826, 2816, 0], 'marked': False}),
        occupied('production_short_weapon_acquires_inside_reach', range=512, occupy_range=3,
                 marked=True, subject_to_elevation=True, elevation=retail,
                 target_object={'location': [2688, 3712, 0]},
                 cells=[{'coord': [10, 10]}, {'coord': [11, 11]}, {'coord': [10, 14]}]),
        occupied('production_long_weapon_refuses_outside_reach', range=2048, occupy_range=3,
                 marked=True, subject_to_elevation=True, elevation=retail,
                 target_object={'location': [2688, 3968, 0]},
                 cells=[{'coord': [10, 10]}, {'coord': [11, 11]}, {'coord': [10, 15]}]),
        occupied('occupied_elevation_bonus_follows_reach', marked=True,
                 source=[2688, 2688, 416], subject_to_elevation=True, elevation=retail,
                 target_object={'location': [4608, 2816, 0]},
                 cells=[{'coord': [10, 10], 'level': 4}, {'coord': [11, 11], 'level': 4}, {'coord': [18, 11]}]),
    ]
    for name, changes in [
        ('empty_building_uses_weapon', {'occupants': 0}),
        ('building_cannot_be_occupied_uses_weapon', {'can_be_occupied': False}),
        ('building_occupants_cannot_fire_uses_weapon', {'can_occupy_fire': False}),
    ]:
        rows.append(occupied(name, building={'foundation_id': 3, **changes},
                             target_object={'location': [4352, 2816, 0], 'marked': False}))
    return [query(row) for row in rows]


def bunker_inputs():
    """Original selected LTNK/ATGUN/Cannon and Rules reader inputs.

    Reuse the established native INI-cache and Bullet/Weapon reader fixture.
    Physical archive precedence and full Rules/Techno readers are not claimed.
    """
    from tools.projectile_oracle.bridge_render_inputs import BulletReader, lexical
    from tools.spatial_oracle.building_body_rules import RULES as INI_COPY, SP
    root = Path(os.environ['VERA20K_BUNKER_ASSETS'])
    m = BulletReader({}, root)
    u = m.u
    for address in (0x887568, 0xA8EB00):
        u.mem_write(address, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    techno = m.alloc(0xE00)
    m.invoke(0x710AF0, techno, (m.cstring('LTNK'),))
    rules = m.alloc(0x2000)
    u.mem_write(0x8871E0, dwords(rules))

    def construct_rules():
        u.reg_write(UC_X86_REG_ESI, rules)
        u.reg_write(UC_X86_REG_ESP, SP)
        # ECX=2 dominates both original integer constructor stores.
        run_checked(u, 0x666AC8, 0x666AE5,
                    required_addresses=[0x666AD3, 0x666ADF])
        run_checked(u, 0x6674D6, 0x6674E0)

    def signed(pointer):
        return struct.unpack('<i', u.mem_read(pointer, 4))[0]

    def rules_state():
        return dict(bunker_bonus=signed(rules + 0xF54),
                    open_topped_bonus=signed(rules + 0xF5C),
                    gravity=signed(rules + 0x16B8))

    def rules_read(begin, end):
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, rules)
        u.reg_write(UC_X86_REG_EDI, INI_COPY)
        run_checked(u, begin, end, required_addresses=[0x5276D0])
        assert u.reg_read(UC_X86_REG_ESP) == SP

    construct_rules()
    constructor = rules_state()
    layers = []
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', None):
        if filename is None:
            # An explicit no-overrides input. No local authored map file is
            # needed to reproduce this cache/reader-retention control.
            sections = {}
            layer_identity = dict(input_kind='supplied_empty_map_rules', sections={})
        else:
            path = root / filename
            if not path.exists():
                assert filename == 'LANGRULE.INI', f'missing required retail layer: {filename}'
                layers.append(dict(file=filename, absent=True))
                continue
            raw = path.read_bytes()
            sections, _ = lexical(raw, {'LTNK', 'ATGUN', 'Cannon', 'CombatDamage', 'AudioVisual'})
            layer_identity = dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(),
                                  bytes=len(raw))
        m.rules_cache(sections)
        m.reads = []
        admitted = {section: bool(m.invoke(0x526810, INI_COPY, (m.cstring(section),)))
                    for section in ('LTNK', 'CombatDamage', 'AudioVisual')}
        # Same Primary/ElitePrimary block as the existing MTNK input owner;
        # current constructor/previous-layer values supply reader defaults.
        if admitted['LTNK']:
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_EBP, techno)
            u.reg_write(UC_X86_REG_EBX, techno + 0x24)
            u.reg_write(UC_X86_REG_ESI, INI_COPY)
            run_checked(u, 0x7129AB, 0x712A1D, required_addresses=[0x528A10])
            assert u.reg_read(UC_X86_REG_ESP) == SP
        weapon = m.read32(techno + 0x898)
        assert weapon, 'retail LTNK Primary reader did not bind a weapon'
        weapon_read = bool(m.invoke(0x772080, weapon, (INI_COPY,)) & 255)
        bullet = m.read32(weapon + 0xA0)
        assert bullet, 'native weapon reader did not bind a projectile'
        bullet_read = bool(m.invoke(0x46BEE0, bullet, (INI_COPY,)) & 255)
        if admitted['CombatDamage']:
            rules_read(0x66C723, 0x66C743)
            rules_read(0x66C767, 0x66C787)
        if admitted['AudioVisual']:
            rules_read(0x66B3C4, 0x66B3E4)
        state = dict(primary=m.string(weapon + 0x24),
                     projectile=m.string(bullet + 0x24),
                     range=signed(weapon + 0xB4), minimum=signed(weapon + 0xB8),
                     cell_rangefinding=bool(u.mem_read(weapon + 0x134, 1)[0]),
                     arcing=bool(u.mem_read(bullet + 0x29B, 1)[0]),
                     subject_to_elevation=bool(u.mem_read(bullet + 0x297, 1)[0]),
                     floater=bool(u.mem_read(bullet + 0x295, 1)[0]),
                     **rules_state())
        # Retain only the keys read by this bounded input chain, not a second
        # copy of entire retail sections or unrelated reader event receipts.
        used = {(r['section'], r['key']) for r in m.reads}
        selected = {section: {key: value for key, value in keys.items()
                             if (section, key) in used}
                    for section, keys in sections.items()}
        layers.append(dict(**layer_identity, keys=selected, weapon_read=weapon_read,
                           bullet_read=bullet_read, section_admission=admitted, state=state))
    retail = state
    controls = []
    for name, changes in [
        ('missing_retains_constructor', [{}]),
        ('explicit_zero', [{'BunkerWeaponRangeBonus': '0'}]),
        ('negative', [{'BunkerWeaponRangeBonus': '-1'}]),
        ('hexadecimal', [{'BunkerWeaponRangeBonus': '$10'}]),
        ('invalid_integer_becomes_zero', [{'BunkerWeaponRangeBonus': 'invalid'}]),
        ('wrong_key_case', [{'bunkerweaponrangebonus': '9'}]),
        ('layer_retains_previous', [{'BunkerWeaponRangeBonus': '4'}, {}]),
        ('layer_overrides_previous', [{'BunkerWeaponRangeBonus': '4'},
                                     {'BunkerWeaponRangeBonus': '1'}]),
    ]:
        construct_rules()
        values = []
        for keys in changes:
            m.rules_cache({'CombatDamage': keys})
            rules_read(0x66C723, 0x66C743)
            values.append(signed(rules + 0xF54))
        controls.append(dict(name=name, layers=changes, values=values))
    return dict(constructor=constructor, layers=layers,
                bunker_reader_controls=controls, retail=retail)


def generate_bunker():
    inputs = bunker_inputs()
    retail = inputs['retail']
    base = {key: value for key, value in retail.items()
            if key not in ('primary', 'projectile')}
    base.update(unit=True, trace_range_stages=True, source=[2688, 2688, 0],
                target_object={'location': [4352, 2688, 0], 'marked': False},
                cells=[{'coord': [10, 10]}, {'coord': [17, 10]}])
    rows = []
    for arcing in (False, True):
        kind = 'retail_cannon_arcing' if arcing else 'direct_control'
        for installed in (False, True):
            row = {**base, 'arcing': arcing, 'bunkered': installed}
            # Discover the accepted/failing neighboring physical coordinates
            # by executing native range verdicts; no hand-derived golden edge.
            lo, hi = 0, 4096
            while lo + 1 < hi:
                middle = (lo + hi) // 2
                probe = {**row, 'target_object': {
                    'location': [2688 + middle, 2688, 0], 'marked': False}}
                if query(probe)['result']:
                    lo = middle
                else:
                    hi = middle
            for label, distance in [('edge', lo), ('beyond', hi)]:
                rows.append({**row, 'name': f'{kind}_{"installed" if installed else "empty"}_{label}',
                             'target_object': {'location': [2688 + distance, 2688, 0], 'marked': False}})
    rows += [
        {**base, 'name': 'retail_installed_at_six_and_half_cells', 'bunkered': True},
        {**base, 'name': 'retail_empty_at_six_and_half_cells', 'bunkered': False},
        {**base, 'name': 'minimum_still_refuses', 'bunkered': True, 'minimum': 2000},
        {**base, 'name': 'sentinel_precedes_all_bonuses', 'bunkered': True,
         'range': -512, 'minimum': 2000, 'open_topped': True},
        {**base, 'name': 'open_topped_after_bunker', 'bunkered': True,
         'bunker_bonus': 1, 'open_topped': True, 'open_topped_bonus': 3},
        {**base, 'name': 'negative_bunker_bonus', 'bunkered': True, 'bunker_bonus': -1},
        {**base, 'name': 'zero_bunker_bonus', 'bunkered': True, 'bunker_bonus': 0},
        {**base, 'name': 'shift_wraps_i32', 'bunkered': True, 'bunker_bonus': 0x1000002},
        {**base, 'name': 'add_wraps_i32', 'bunkered': True, 'range': 0x7FFFFFF0},
        {**base, 'name': 'air_bonus_before_bunker', 'bunkered': True, 'air_range_bonus': 768,
         'target_object': {'location': [4352, 2688, 416], 'marked': True},
         'cells': [{'coord': [10, 10]}, {'coord': [17, 10]}]},
        # Installed-link bytes on a Building are a defensive RTTI control,
        # not a claimed legal bunker occupancy state. Its existing occupied
        # range replacement still executes before the class-6 exclusion.
        {**base, 'name': 'building_rtti_excludes_bunker_bonus', 'unit': False,
         'building': {'foundation_id': 3}, 'bunkered': True, 'bunker_bonus': 100},
        {**base, 'name': 'empty_building_rtti_excludes_bunker_bonus', 'unit': False,
         'building': {'foundation_id': 3, 'occupants': 0},
         'bunkered': True, 'bunker_bonus': 100},
    ]
    return dict(schema_version=1, inputs=inputs, rows=[query(row) for row in rows])


def bunker_metadata():
    return dict(provenance(
        scope='Original CanFireAt6F7970/6F77B0 -> InRange6F7220 Unit/Building bunker range stage; native LTNK Primary, ATGUN Weapon, Cannon Bullet and selected retained Rules readers.',
        entry_points={'coordinate_cell_wrapper': 0x6F7970, 'range_source': 0x6F77B0,
                      'range': 0x6F7220, 'bunker_stage': 0x6F72A2,
                      'bunker_rules_ctor': 0x666AC8, 'bunker_rules_reader': 0x66C723,
                      'integer_reader': 0x5276D0, 'techno_ctor': 0x710AF0,
                      'primary_reader': 0x7129AB, 'weapon_reader': 0x772080,
                      'bullet_reader': 0x46BEE0},
        assumptions=[
            'Rules inputs execute original constructor stores and complete selected read/store blocks. Original TechnoType constructor plus Primary block, full Weapon and Bullet readers consume lexical entries from RULESMD, optional LANGRULE and MPBattleMD. The final supplied_empty_map_rules layer is explicitly an empty cache input and tests retained values; no local custom map file is required or claimed as replayed. Physical INI/MIX/map loading, whole type registry/read order and full RulesClass/TechnoType readers are outside this bounded comparison.',
            'Inherited BulletReader supplies original cached INI indexes, allocation/TLS and physical image-load boundary. No Rust-interpreted numeric values initialize native readers. Missing SHP images and empty sound registry are unrelated to compared scalar range/projectile fields; render/weapon damage behavior is not claimed.',
            'Range queries supply retained actor/type/map fields and copied original Unit7F5C70/Building7E3EBC vtables. GetCoords and RTTI execute. +2E4 is supplied as null or nonnull; the host pointer is never dereferenced by the observed range stage. This does not establish bunker installation/lifecycle.',
            'Retail LTNK/ATGUN/Cannon scalar inputs feed the arcing rows; direct rows deliberately override Arcing=false. Position, object-target marked/bridge bits, map levels and synthetic bonus/overflow/minimum/sentinel controls are declared inputs. Native execution discovers adjacent admitted/refused range edges.',
            'Building +2E4 and simultaneous bunkered/open-topped inputs are defensive arithmetic/ordering controls, not claims that those combinations arise in retail play. AirRange, occupied replacement, bunker and open-topped stage order is observed via registers.',
            'PC53/chop FPCW0E7F, independently established 104-height/416-bridge constants, flat map and default zero elevation increment. No GreatestThreat search-radius changes, acquisition/scoring, complete fire/flight, RNG or production game capture claimed.'],
        substitutions=[
            'Range-query GetWeapon+3F8 supplies one original-shaped weapon slot for Unit/Infantry sources, as in the existing query owner. Building sources execute GetWeapon. All range, minimum, arithmetic, coordinate, distance, elevation, ballistic and line-of-fire calls execute original instructions.',
            'Native input-reader fixture inherits existing allocation/delete, CRT TLS and physical image-load transport hooks from BulletReader/Reader/Lists; no scalar parser result is substituted.']),
        commands=['python -m tools.spatial_oracle.walk_cell_range --bunker --write',
                  'python -m tools.spatial_oracle.walk_cell_range --bunker --check'],
        required_assets=['supported gamemd.exe', 'RULESMD.INI', 'MPBattleMD.ini'],
        optional_assets=['LANGRULE.INI when present in the selected installation'],
        asset_environment='VERA20K_BUNKER_ASSETS names the extracted INI directory; VERA20K_GAMEMD_EXE selects the executable. No map, ART, SHP or sound file is required.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--bunker', action='store_true')
    selected, remaining = parser.parse_known_args()
    if selected.bunker:
        finish_vectors(generate_bunker, Path(__file__).with_name('walk_cell_range_bunker.json'),
                       provenance=bunker_metadata, argv=remaining,
                       source_paths={
                           'walk_cell_range': Path(__file__),
                           'native_oracle': Path(__file__).parents[1] / 'native_oracle.py',
                           'map_queries': Path(__file__).with_name('map_queries.py'),
                           'bullet_reader': Path(__file__).parents[1] / 'projectile_oracle/bridge_render_inputs.py',
                           'ini_reader': Path(__file__).parents[1] / 'rules_oracle/bridge_anim_inputs.py',
                           'lists': Path(__file__).parents[1] / 'rules_oracle/bridge_anim_lists.py',
                           'reader_fixture': Path(__file__).with_name('building_body_rules.py'),
                           'ini_crc': Path(__file__).parents[1] / 'projectile_oracle/flat_art.py',
                       })
        raise SystemExit(0)
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: dict(provenance(
        scope='Original6F7970->6F77B0->6F7220 Infantry and occupied-Building Cell/object-target range and lookup ordering, including live Building GetCoords/GetWeapon/IsOccupied/HalfFoundation and the occupied range replacement; the +0x82 OpenToppedRangeBonus stage; the SubjectToElevation bonus (6F6F60 both arms, 6F70E0 arcing arm), the arcing arm\'s 2-D distance, its arc test (48AB90, 48ACF0, 48ABC0) under supplied Gravity/Floater and its bridge ceiling (6F74D7); explicit supplied object/weapon/map fields.',
        entry_points={'coordinate_cell_wrapper': 0x6F7970, 'range_source': 0x6F77B0, 'range': 0x6F7220, 'building_coords': 0x447AC0, 'building_get_weapon': 0x4526F0, 'is_occupied': 0x458DD0, 'half_foundation': 0x458E00, 'elevation_direct': 0x6F6F60, 'elevation_arcing': 0x6F70E0, 'cell_height': 0x487D50, 'map_cell_packed': 0x5657A0, 'cell_coords': 0x486840, 'cell_tile_gate': 0x4867E0, 'cell_ground': 0x47B3A0, 'map_ground': 0x578080, 'map_cell': 0x565730, 'line': 0x4CC310, 'launch_speed': 0x48AB90, 'floater_gravity': 0x48ACF0, 'can_reach': 0x48ABC0},
        assumptions=['Supplied original Infantry table7EB058 (object target and Infantry source), Building table7E3EBC (Building source), and Cell table7E4EEC; no bunker/veteran range bonuses. Building source rows supply type+520, Foundation+EF0, occupancy flags+157B/+157C, vector+688/count+694/index+69C and Infantry occupants with OccupyWeapon+E04. Upgrades+702=0. The open-topped rows set +0x82 and Rules+0xF5C (Rules at0x8871E0, bonus2 unless supplied). Projectile flags are false but the rows\' Floater +0x295, SubjectToElevation +0x297 and Arcing +0x29B: no wall/cliff collision; original line callable executes.', 'Supplied independently established104 level/208 high-flight and416 bridge constants and the104 Techno level height0xB0EB34 (StaticInit6F2970), x87 control0E7F. WaterSet base is a supplied theater input; tile and Dummy level/slope/flags are supplied current state. [ElevationModel] Rules+0x1838/+0x1840/+0x1848 are supplied per row, the constructor\'s 0/1.0/0.0 otherwise.', 'Rules+0x16B8 Gravity and BulletType+0x295 Floater are supplied per row, Gravity 0 unless given: a zero gravity gives launch speed 0 and Can_Reach48ABC0 admits the row. Rows exercise gravity 6, 6 halved by Floater, -6 and 0, a zero 2-D distance, and the bridge ceiling on a real cell, without and with retail gravity, and through a Dummy target centre that truncates into a real cell.', 'Rules+F48 OccupyWeaponRange is5 unless supplied. Building rows distinguish short/long weapon Range from occupied reach, false occupancy flags and no occupants, minimum/sentinel stages, AirRangeBonus replacement and subsequent elevation bonus, odd/even foundations and CellRangefinding centre. The two production reach inputs use occupy range3, weapon512/2048 and native Building Location(2688,2688,0) against Infantry Location(2688,3712/3968,0). No constructors, INI/ART readers, complete acquisition/ring/scoring, PerCell or shot/flight behavior claimed.'],
        substitutions=['Infantry source rows: GetWeapon+3F8 records requested slot and supplies one original-shaped weapon slot. Building source rows execute original GetWeapon, IsOccupied, occupant count and HalfFoundation against supplied type/occupant fields. No other callable substitution.']),
        commands=['python -m tools.spatial_oracle.walk_cell_range --write', 'python -m tools.spatial_oracle.walk_cell_range --check']),
        source_paths={'walk_cell_range': Path(__file__), 'native_oracle': Path(__file__).parents[1] / 'native_oracle.py', 'map_queries': Path(__file__).with_name('map_queries.py')})
