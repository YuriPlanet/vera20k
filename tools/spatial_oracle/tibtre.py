"""Original Terrain TIBTRE AI, forced neighbor spread and ore placement.

Run ``python -m tools.spatial_oracle.tibtre --write`` once to publish references;
the default/``--check`` repeats original execution and compares them read-only.
The companion Markdown and provenance declare prepared state and service sinks.
"""
from functools import lru_cache
import hashlib
import os
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI,
    UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools.native_oracle import RET_MAGIC, configured_gamemd, finish_vectors, provenance, run_checked
from tools.rules_oracle.bridge_landing_inputs import Landing, physical_inputs
from tools.spatial_oracle.bridge_constructor import OriginalBridgeConstructor
from tools.spatial_oracle.building_body_rules import INI, SP as READER_SP
from tools.spatial_oracle.harvest_field import (
    ADD_GROWTH, FIELD, IMAGES, OVERLAYS, QUEUES, TIBS, bitmap_index,
    place_occupants, place_tiberium,
)
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import BLD, BTYPE, EXTRA, cell, cell_xy, make_dock_fixture
from tools.spatial_oracle.unit_source_scatter import NEIGHBORS, SCENARIO
from tools.spatial_oracle.unit_scatter_state import SP
from tools.spatial_oracle.terrain_recalc import LAT_GLOBALS
from tools.sidebar_oracle.stock import mix, mix_hash

AI, SPREAD, CAN_GERMINATE, PLACE = 0x71C730, 0x483780, 0x4838E0, 0x487190
RANDOM, RANGED = 0x65C780, 0x65C7E0
TREE, TREE_TYPE, SHAPE = EXTRA + 0x22000, EXTRA + 0x24000, EXTRA + 0x25000
ARENA, ARENA_SIZE = FIELD + 0x100000, 0x40000
INI_ROOT = Path(os.environ.get('VERA20K_TIBTRE_INI', 'ini'))


def i32(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def initialize_instance_stage(u, obj, typ, frame):
    """Two original constructor member/reset blocks; placement is excluded."""
    u.mem_write(0xA8ED84, dwords(frame))
    u.mem_write(SP - 0x100, bytes(0x200))
    u.mem_write(SP + 0x20, dwords(typ))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, obj)
    run_checked(u, 0x71BB9E, 0x71BBD1, required_addresses=(0x71BBA4, 0x71BBC7))
    run_checked(u, 0x71BC86, 0x71BCA5, required_addresses=(0x71BC93, 0x71BC9F))


def instance_constructor_controls():
    m = Landing()
    typ = m.terrain('TIBTRE01')
    obj = m.alloc(0x200)
    rows = []
    for frame in (0, 17, 2147483647, -2147483648):
        m.u.mem_write(obj, bytes([0xA5]) * 0x200)
        initialize_instance_stage(m.u, obj, typ, frame)
        rows.append(dict(frame=frame, stage=i32(m.u, obj + 0xAC),
                         changed=bool(m.u.mem_read(obj + 0xB0, 1)[0]),
                         timer=list(struct.unpack('<3i', m.u.mem_read(obj + 0xB4, 12))),
                         rate=i32(m.u, obj + 0xC0), step=i32(m.u, obj + 0xC4)))
    return rows


def native_type_input(keys):
    """Original TerrainType ctor and selected original RULES reader slices."""
    m = Landing()
    p = m.terrain('TIBTRE01')
    u = m.u
    constructor = dict(rate=i32(u, p + 0x2A0), probability_bits=m.read32(p + 0x2A4),
                       animated=bool(u.mem_read(p + 0x2B3, 1)[0]),
                       spawns=bool(u.mem_read(p + 0x2B1, 1)[0]))
    cached = {key: value.strip(''.join(map(chr, range(33)))) for key, value in keys.items()
              if value is not None and value.strip(''.join(map(chr, range(33))))}
    cached.setdefault('FixtureOnly', '1')
    m.make_ini({'TIBTRE01': cached})
    # The preceding IsVeinhole/IsFlammable reader is outside these slices.
    # Their adjacent stores retain the original constructor default supplied AL.
    u.reg_write(UC_X86_REG_ESP, READER_SP)
    u.reg_write(UC_X86_REG_ESI, p)
    u.reg_write(UC_X86_REG_EBX, INI)
    u.reg_write(UC_X86_REG_EDI, p + 0x24)
    u.reg_write(UC_X86_REG_EAX, u.mem_read(p + 0x2B0, 1)[0])
    run_checked(u, 0x71DF27, 0x71DF56, required_addresses=(0x5295F0, 0x71DF50))
    spawns = bool(u.mem_read(p + 0x2B1, 1)[0])
    u.reg_write(UC_X86_REG_ESP, READER_SP)
    u.reg_write(UC_X86_REG_ECX, u.mem_read(p + 0x2B3, 1)[0])
    u.reg_write(UC_X86_REG_EAX, p + 0x29C)
    run_checked(u, 0x71E014, 0x71E06B,
                required_addresses=(0x5295F0, 0x5276D0, 0x5283D0))
    # Include the original FSTP dword at71E073, rather than a Python f32 cast.
    run_checked(u, 0x71E06B, 0x71E079)
    assert u.reg_read(UC_X86_REG_ESP) == READER_SP
    return dict(raw_keys=keys, constructor=constructor,
                rate=i32(u, p + 0x2A0), probability_bits=m.read32(p + 0x2A4),
                probability=struct.unpack('<f', u.mem_read(p + 0x2A4, 4))[0],
                animated=bool(u.mem_read(p + 0x2B3, 1)[0]), spawns=spawns)


@lru_cache(maxsize=1)
def retail_inputs():
    rows, physical = [], {}
    for filename in ('RULESMD.INI', 'ARTMD.INI'):
        raw = (INI_ROOT / filename).read_bytes()
        sections, _ = physical_inputs(raw)
        physical[filename] = dict(bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                                  tibtre01_keys=sections['TIBTRE01'])
    rules = physical['RULESMD.INI']['tibtre01_keys']
    keys = {key: rules.get(key) for key in
            ('SpawnsTiberium', 'IsAnimated', 'AnimationRate', 'AnimationProbability')}
    rows.append(dict(name='retail_tibtre01', **native_type_input(keys)))
    ra2 = mix((configured_gamemd().parent / 'ra2.mix').read_bytes())
    shapes = []
    for archive, suffix in (('temperat.mix', 'TEM'), ('snow.mix', 'SNO')):
        members = mix(ra2[mix_hash(archive)])
        for number in range(1, 4):
            name = f'TIBTRE{number:02}.{suffix}'
            raw = members[mix_hash(name)]
            header = list(struct.unpack_from('<4H', raw))
            formats = [raw[16 + index * 24] for index in range(header[3])]
            shapes.append(dict(archive=f'ra2.mix/{archive}', name=name,
                               bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                               header=header, frame_formats=formats))
    return dict(physical=physical, shapes=shapes, rows=rows)


def fixture(case):
    u, call, read32 = make_dock_fixture(dict(seed=case.get('seed', 1), linked=False))
    place_tiberium(u, read32, case)
    u.mem_map(ARENA, ARENA_SIZE)
    cursor = ARENA

    def allocate(size):
        nonlocal cursor
        pointer = cursor
        cursor += (max(size, 1) + 15) & ~15
        assert cursor < ARENA + ARENA_SIZE
        return pointer

    OriginalBridgeConstructor.empty_registries(u, allocate, capacity=128)
    # Empty abstract listener owner, initialized by startup40B563.
    u.mem_write(0x87F5D8, dwords(0x7E17CC))
    call(0x5FC310, 0, [])
    u.mem_write(0xA8E9A0, b'\x01')
    # Reuse the shared terrain_recalc/bridge_constructor resident1x1 TMP
    # fixture: real attribute and zone-cache writes execute after Overlay Mark.
    head, tmp, tiles = allocate(0x400), allocate(88), allocate(4)
    zones, levels = allocate(33 * 33 * 4), allocate(33 * 33 * 10)
    u.mem_write(0x87F7E8 + 0x68, dwords(zones, 33 * 33, levels))
    u.mem_write(head, dwords(0x7ECC48))
    u.mem_write(head + 0xA4, dwords(tmp))
    u.mem_write(head + 0x2C8, dwords(-1))
    u.mem_write(head + 0x2D4, dwords(-1))
    u.mem_write(head + 0x306, b'\x01')  # supplied tile AllowTiberium
    u.mem_write(tmp, dwords(1, 1, 8, 4, tmp + 20))
    u.mem_write(tmp + 60, bytes((3, 11, 0)))
    u.mem_write(0xA8ED2C, dwords(tiles))
    u.mem_write(0xA8ED38, dwords(1))
    u.mem_write(tiles, dwords(head))
    for address in LAT_GLOBALS:
        u.mem_write(address, dwords(-1))
    for index in range(4):
        tib = TIBS + index * 0x200
        u.mem_write(tib + 0xE4, dwords(12))
        u.mem_write(tib + 0xB0, struct.pack('<d', case.get('growth_percentage', 0.1)))
    # Buildable is the byte at land-row+0x20 (89EA60 + land*36), read by
    # CanPlaceTiberium; native Rules674216..67421B reads/stores Buildable.
    for land in (0, 5):
        u.mem_write(0x89EA60 + land * 36, b'\x01')
    inputs = case.get('type', retail_inputs()['rows'][0])
    u.mem_write(TREE_TYPE, dwords(0x7F5458))
    u.mem_write(TREE_TYPE + 0x1F4, dwords(-1))
    u.mem_write(TREE_TYPE + 0xA4, dwords(SHAPE))
    u.mem_write(TREE_TYPE + 0x2A0, dwords(inputs['rate'], inputs['probability_bits']))
    u.mem_write(TREE_TYPE + 0x2B1, bytes([inputs['spawns']]))
    u.mem_write(TREE_TYPE + 0x2B3, bytes([inputs['animated']]))
    u.mem_write(SHAPE + 6, struct.pack('<h', case.get('shape_frames', 22)))
    # Original OverlayType pointers needed by OverlayClass GetType/Mark.
    for index in range(128):
        typ = OVERLAYS + index * 0x300
        u.mem_write(typ, dwords(0x7EF600))
        u.mem_write(typ + 0x1F4, dwords(-1))
        if any(base <= index < base + main + extra for base, main, extra in IMAGES.values()):
            u.mem_write(typ + 0x298, dwords(5))
    coords = case.get('tree_cells', [[16, 16]])
    for index, (x, y) in enumerate(coords):
        obj = TREE + index * 0x200
        u.mem_write(obj, dwords(0x7F522C))
        u.mem_write(obj + 0xC8, dwords(TREE_TYPE))
        u.mem_write(obj + 0x64, dwords(-1))
        u.mem_write(obj + 0x9C, dwords(x * 256 + 128, y * 256 + 128, 0))
        initialize_instance_stage(u, obj, TREE_TYPE, case.get('constructor_frame', 0))
        if 'stage' in case:
            stage = case['stage']
            # Explicit retained stage/frame, timer start/gap/left, rate, step.
            u.mem_write(obj + 0xAC, dwords(stage[0]))
            u.mem_write(obj + 0xB4, dwords(*stage[1:]))
        u.mem_write(cell(x, y) + 0xE4, dwords(obj))
        u.mem_write(cell(x, y) + 0x124, dwords(0x40))
    place_occupants(u, call, case)
    plain = allocate(0x200)
    plain_type = allocate(0x400)
    u.mem_write(plain, dwords(0x7F522C))
    u.mem_write(plain + 0xC8, dwords(plain_type))
    u.mem_write(plain_type, dwords(0x7F5458))
    u.mem_write(BLD + 0x6C, dwords(case.get('building_health', 900)))
    u.mem_write(BTYPE + 0xC9A, bytes([case.get('building_invisible', False)]))
    u.mem_write(BTYPE + 0x1701, bytes([case.get('building_invisible_in_game', False)]))
    u.mem_write(head + 0x306, bytes([case.get('tile_allows_tiberium', True)]))
    for x, y, kind in case.get('blocked', []):
        target = cell(x, y)
        if kind == 'overlay':
            u.mem_write(target + 0x44, dwords(7))
        elif kind == 'land':
            u.mem_write(target + 0xEC, dwords(1))
        elif kind == 'slope':
            u.mem_write(target + 0x11C, b'\x01')
        elif kind == 'bridge':
            u.mem_write(target + 0x140, dwords(0x100))
        elif kind == 'tree':
            u.mem_write(target + 0xE4, dwords(TREE))
        elif kind == 'ordinary_tree':
            u.mem_write(target + 0xE4, dwords(plain))
        elif kind == 'building':
            u.mem_write(target + 0xE4, dwords(BLD))
        else:
            raise ValueError(kind)
    events, pending = [], []

    def ret(value=0, cleanup=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def observe(_uc, address, _size, _data):
        sp, this = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        if pending and address == pending[-1][0]:
            _, index = pending.pop()
            events[index]['result'] = u.reg_read(UC_X86_REG_EAX)
            if events[index]['event'] == 'can_germinate':
                events[index]['result'] &= 0xFF
            if events[index]['event'] in ('random', 'ranged'):
                events[index]['rng_after'] = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
        if address == RANDOM:
            events.append(dict(event='random', caller=f'{read32(sp):08X}', result=None))
            events[-1]['rng_before'] = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
            pending.append((read32(sp), len(events) - 1))
        elif address == RANGED:
            events.append(dict(event='ranged', caller=f'{read32(sp):08X}',
                               lo=i32(u, sp + 4), hi=i32(u, sp + 8), result=None))
            events[-1]['rng_before'] = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
            pending.append((read32(sp), len(events) - 1))
        elif address == CAN_GERMINATE:
            # Native Cells carry their coordinates at+24, including the
            # resident dummy updated by MapClass lookup beyond this arena.
            events.append(dict(event='can_germinate', cell=list(struct.unpack('<hh', u.mem_read(this + 0x24, 4))),
                               caller=f'{read32(sp):08X}', result=None))
            pending.append((read32(sp), len(events) - 1))
        elif address == SPREAD:
            events.append(dict(event='spread', cell=cell_xy(this), force=read32(sp + 4)))
        elif address == PLACE:
            events.append(dict(event='place', cell=cell_xy(this), kind=read32(sp + 4), amount=read32(sp + 8)))
        elif address == ADD_GROWTH:
            events.append(dict(event='growth_queue', kind=(this - TIBS) // 0x200,
                               cell=list(struct.unpack('<hh', u.mem_read(read32(sp + 4), 4)))))
        elif address == 0x7C8E17:
            ret(allocate(read32(sp + 4)))
        elif address == 0x7C8B3D:
            ret()
        elif address == 0x47D2B0:
            events.append(dict(event='recalc', cell=cell_xy(this)))
        elif address in (0x47FDE0, 0x47FB90, 0x47FF80):
            out = read32(sp + 4)
            u.mem_write(out, dwords(0, 0, 0, 0))
            ret(out, 4)
        elif address == 0x6D2790:
            events.append(dict(event='tactical_dirty_sink'))
            ret(cleanup=20)
        elif address == 0x6551C0:
            events.append(dict(event='radar_dirty_sink', cell=list(struct.unpack('<hh', u.mem_read(read32(sp + 4), 4)))))
            ret(cleanup=4)
    u.hook_add(UC_HOOK_CODE, observe)
    return u, call, read32, coords, events


def state(u, read32, coords):
    actors = []
    for index, _ in enumerate(coords):
        obj = TREE + index * 0x200
        actors.append(dict(stage=i32(u, obj + 0xAC), changed=bool(u.mem_read(obj + 0xB0, 1)[0]),
                           timer=list(struct.unpack('<3i', u.mem_read(obj + 0xB4, 12))),
                           rate=i32(u, obj + 0xC0), step=i32(u, obj + 0xC4)))
    cells = []
    observed = sorted({(x + dx, y + dy) for x, y in coords for dx in (-1, 0, 1) for dy in (-1, 0, 1)})
    for x, y in observed:
        overlay = i32(u, cell(x, y) + 0x44)
        data = u.mem_read(cell(x, y) + 0x11E, 1)[0]
        cells.append(dict(cell=[x, y], overlay=overlay, data=data,
                          land=i32(u, cell(x, y) + 0xEC)))
    queues = []
    for kind in (0, 1):
        g = QUEUES + kind * 0x1000 + 0x800
        count = read32(TIBS + kind * 0x200 + 0x10C)
        entries = [dict(cell=list(struct.unpack('<hh', u.mem_read(g + 0x100 + i * 8, 4))),
                        priority_bits=read32(g + 0x104 + i * 8)) for i in range(count)]
        queues.append(dict(kind=kind, count=count, entries=entries))
    rng = bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    return dict(actors=actors, cells=cells, growth=queues,
                rng_indices=[read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)],
                rng_sha256=hashlib.sha256(rng).hexdigest(),
                overlay_registrations=read32(0xA8EC60), pending_deletes=read32(0xB0F6A8))


def timeline(case):
    u, call, read32, coords, events = fixture(case)
    rows = []
    for frame in range(case.get('first_frame', 1), case.get('last_frame', 42) + 1):
        u.mem_write(0xA8ED84, dwords(frame))
        for index in case.get('order', list(range(len(coords)))):
            first = len(events)
            # Native timer+4 receives an uninitialized caller local. Give this
            # padding a declared zero value so it is reproducible, not semantic.
            u.mem_write(SP - 0x100, bytes(0x100))
            call(AI, TREE + index * 0x200, [])
            rows.append(dict(frame=frame, actor=index, events=events[first:], state=state(u, read32, coords)))
    next_random = []
    for _ in range(4):
        call(RANDOM, SCENARIO + 0x218, [])
        next_random.append(u.reg_read(UC_X86_REG_EAX))
    return dict(input=case, rows=rows, next_random=next_random)


def direct_spread(case):
    case = dict(case, frame=case.get('frame', 200))
    u, call, read32, coords, events = fixture(case)
    u.mem_write(0xA8ED84, dwords(case['frame']))
    call(SPREAD, cell(*coords[0]), [1])
    result, observed_events, final_state = u.reg_read(UC_X86_REG_EAX) & 0xFF, list(events), state(u, read32, coords)
    next_random = []
    for _ in range(4):
        call(RANDOM, SCENARIO + 0x218, [])
        next_random.append(u.reg_read(UC_X86_REG_EAX))
    return dict(input=case, result=result, events=observed_events, state=final_state,
                next_random=next_random)


def probability_control(inputs, raw):
    """Supply one native RNG-state word, then execute its original Random/AI."""
    case = dict(type=inputs, next_raw=raw, seed=1)
    u, call, read32, coords, events = fixture(case)
    first, second = read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)
    a, b = SCENARIO + 0x224 + first * 4, SCENARIO + 0x224 + second * 4
    u.mem_write(a, dwords((raw & 0xFFFFFFFF) ^ read32(b)))
    before = state(u, read32, coords)
    u.mem_write(0xA8ED84, dwords(1))
    u.mem_write(SP - 0x100, bytes(0x100))
    call(AI, TREE, [])
    after = state(u, read32, coords)
    return dict(input=case, before_rng_indices=before['rng_indices'], events=events,
                started=after['actors'][0]['rate'] != 0, state=after)


def generate():
    inputs = retail_inputs()
    custom = [dict(name=name, **native_type_input(keys)) for name, keys in (
        ('zero_probability', {'SpawnsTiberium': 'yes', 'IsAnimated': 'yes', 'AnimationRate': '3', 'AnimationProbability': '0'}),
        ('always_rate3', {'SpawnsTiberium': 'yes', 'IsAnimated': 'yes', 'AnimationRate': '3', 'AnimationProbability': '1'}),
        ('always_rate0', {'SpawnsTiberium': 'yes', 'IsAnimated': 'yes', 'AnimationRate': '0', 'AnimationProbability': '1'}),
        ('always_rate_minus1', {'SpawnsTiberium': 'yes', 'IsAnimated': 'yes', 'AnimationRate': '-1', 'AnimationProbability': '1'}),
        ('not_animated', {'SpawnsTiberium': 'yes', 'IsAnimated': 'no', 'AnimationRate': '3', 'AnimationProbability': '1'}),
        ('not_spawner', {'SpawnsTiberium': 'no', 'IsAnimated': 'yes', 'AnimationRate': '3', 'AnimationProbability': '1'}),
        ('constructor_only', {}),
    )]
    by_name = {row['name']: row for row in custom}
    rows = [dict(name='stock22_seed1', seed=1, last_frame=42),
            dict(name='stock22_seed4', seed=4, last_frame=80)]
    rows.extend(dict(name=name, seed=1, last_frame=38, type=by_name[name]) for name in by_name)
    # A supplied already-playing state exercises normal expiry independent
    # of the rare idle admission without replacing RNG or animation calls.
    rows.append(dict(name='stock22_already_started', seed=1, last_frame=40, stage=[0, 0, 0, 3, 3, 1]))
    rows.append(dict(name='not_animated_already_started', seed=1, last_frame=38,
                     stage=[0, 0, 0, 3, 3, 1], type=by_name['not_animated']))
    blocked = [[16 + dx, 16 + dy, 'overlay'] for dx in (-1, 0, 1) for dy in (-1, 0, 1) if dx or dy]
    rows.append(dict(name='stock22_no_neighbor', seed=1, last_frame=34, stage=[0, 0, 0, 3, 3, 1], blocked=blocked))
    rows.append(dict(name='two_spawners_ordered', seed=1, last_frame=40,
                     tree_cells=[[16, 16], [18, 16]], stage=[0, 0, 0, 3, 3, 1]))
    rows.append(dict(name='two_spawners_reversed', seed=1, last_frame=40, order=[1, 0],
                     tree_cells=[[16, 16], [18, 16]], stage=[0, 0, 0, 3, 3, 1]))
    spreads = [dict(name=f'direction_seed_{seed}', seed=seed) for seed in (1, 2, 3, 5, 8, 9, 22, 137)]
    spreads.append(dict(name='all_blocked', blocked=blocked))
    spreads.append(dict(name='existing_ore_rejected', ore=[[15, 17, 0, 3, 7], [15, 16, 1, 2, 6]]))
    spreads.append(dict(name='source_gems', ore=[[16, 16, 1, 2, 6]]))
    spreads.append(dict(name='spread_disabled_forced', spreads=False))
    for kind in ('overlay', 'land', 'slope', 'bridge', 'tree'):
        spreads.append(dict(name=f'admission_{kind}', blocked=[[x, y, kind] for x, y, _ in blocked]))
    spreads.append(dict(name='units_allow_new_ore', units=[[x, y] for x, y, _ in blocked]))
    spreads.append(dict(name='ordinary_tree_constructor_refuses_overlay',
                        blocked=[[x, y, 'ordinary_tree'] for x, y, _ in blocked]))
    spreads.append(dict(name='live_building_blocks_new_ore',
                        blocked=[[x, y, 'building'] for x, y, _ in blocked]))
    spreads.append(dict(name='dead_building_allows_new_ore', building_health=0,
                        blocked=[[x, y, 'building'] for x, y, _ in blocked]))
    spreads.append(dict(name='invisible_building_allows_new_ore', building_invisible=True,
                        blocked=[[x, y, 'building'] for x, y, _ in blocked]))
    spreads.append(dict(name='invisible_in_game_building_allows_new_ore', building_invisible_in_game=True,
                        blocked=[[x, y, 'building'] for x, y, _ in blocked]))
    spreads.append(dict(name='tile_disallows_new_ore', tile_allows_tiberium=False))
    probability_types = [inputs['rows'][0], by_name['zero_probability'],
                         dict(name='probability_half', **native_type_input({'SpawnsTiberium': 'yes', 'IsAnimated': 'yes', 'AnimationRate': '3', 'AnimationProbability': '.5'}))]
    probability_rows = [probability_control(typ, raw) for typ in probability_types
                        for raw in (0, 1, 2999, 3000, 3001, 499999, 500000, 500001,
                                    999999, 1000000, -3000, -2147483648, 2147483647)]
    return dict(schema_version=1, retail_inputs=inputs, custom_type_inputs=custom,
                instance_constructor_controls=instance_constructor_controls(),
                probability_controls=probability_rows,
                timelines=[timeline(case) for case in rows],
                spread_cases=[direct_spread(case) for case in spreads])


def metadata():
    return provenance(
        scope='Original TerrainClass71C730 idle probability/animation timer/midpoint producer through forced Cell483780 neighbor walk, admission4838E0, Place487190, Overlay constructor5FC380/Mark, native Cell RecalcAttributes and Tiberium growth queue7235A0. Selected original TerrainType constructor and RULES reader slices establish scalar inputs. Prepared map/objects/registries; dirty services bounded. No full scenario, physical INI/archive load, native pixels, or whole TIBTRE parity claim.',
        assumptions=[
            'Stock TIBTRE01 physical RULESMD and ARTMD selected sections are lexically extracted by existing bridge_landing_inputs.physical_inputs; original scalar readers and float store execute. ART has Theater/Foundation only; measured SpawnsTiberium, IsAnimated, AnimationRate and AnimationProbability belong to RULES. Original TerrainType ctor71DA80 establishes defaults. Reader slices supply successful base-reader admission; unrelated adjacent store AL retained as declared constructor value.',
            'Shared refinery_dock map/Scenario RNG fixture and harvest_field overlay/Tiberium queue tables, plus original startup direction49F2F0 and EmptyCell5FC310. Tree centers16,16 or18,16; shape header count22 supplied from pinned physical TIBTRE01/02/03 TEM and SNO bytes, decoded through existing stock.mix/mix_hash input preparation. Original instance constructor member/reset blocks71BB9E..71BBD1 and71BC86..71BCA5 establish fresh stage0, changedfalse, timerstart=currentframe, duration/rate0,step1. Full Terrain constructor/placement and whole Logic scheduler excluded. Timer+4 copies an uninitialized caller local; supplied caller stack zero makes this padding reproducible without assigning gameplay meaning. Silent alive on-map Terrain body/type state has no dying byte or other active effects.',
            'Overlay/Tiberium prepared registry data: Riparius base102/main12/extra8, Cruentus base27/main12, maxstage12, growth/spread percentages0.1. Original Overlay constructors/Mark and queue insert bodies execute; empty original startup registries have supplied capacity128. GrowthAllowed land bytes for Clear0/Tiberium5 supplied. Map diamond width16,height16 is inherited from harvest_field.',
            'Original Recalc47D2B0 executes with the existing terrain_recalc/bridge_constructor synthetic resident1x1 TMP:8x4 pixels, rawheight3, rawland11, slope0, supplied AllowTiberiumtrue; spare33x33 zone/level caches and allLAT bases-1. OverlayType Land5 for ore/gems. Native tile reader, overlay classification and zone-cache writes execute; physical TMP loading/relocation remain outside this corpus.',
            'Native x87 control0x0E7F; original CRT float scanner initializer7C8F5E executes in reader fixture. No Python expected arithmetic implements probability, timer, queue priority, overlay selection or RNG. All RNG original Scenario seeded state; timelines invoke Terrain AI in explicit actor order once per frame.',
            'Probability controls replace one data word in the original seeded Scenario RNG state so original Random65C780 XOR produces the recorded next_raw; no RNG function/caller result is hooked. Stock.003 boundaries2999/3000/3001, half.5 boundaries499999/500000/500001, zero, negative raw, INT_MIN and modulo wrap are executed independently. Direct spread seeds cover every native start direction0..7. Actor-order controls schedule two original AI objects in both orders; they do not execute whole Logic dispatch.',
            'Adjacent Units reuse original-vtable/Drive constructed harvest_field.place_occupants fixtures. Ordinary Terrain uses original Terrain vtable with SpawnsTiberiumfalse. Building neighbors refer to the shared Building fixture, with explicit health and native flagsInvisible+C9A / InvisibleInGame+1701 controls. Fields are independently supplied retained-state branch probes: native InvisibleInGame post-read also forcesInvisibletrue andRadarVisiblefalse, but that full Building reader/layer reachability is not claimed here. These supplied object list entries exercise original CanGerminate gates, not complete legal object placement or building constructors.',
        ],
        substitutions=[
            'Inherited shared fixtures substitute OS InterlockedIncrement/Decrement integer services during locomotor setup; no reached TIBTRE gameplay query substitutes its result.',
            'Operator new7C8E17 supplies bounded arena storage; delete7C8B3D is a no-op. Full original Overlay constructor/Mark executes after allocation.',
            'Cell47FDE0/47FB90/47FF80 rectangle queries return empty rectangles; Tactical6D2790 and Radar6551C0 record dirty effects and return0. These sinks do not supply admission, placement, density, timer, queue, classification or RNG decisions.',
            'Type input reader inherits Landing bounded arena allocation/free and CRT TLS accessor7D140B setup. Physical lexical file loading, file layer admission and archive resources are supplied; selected original reader bodies/store instructions execute.',
        ],
        entry_points={'terrain_ai': AI, 'stage_timer': 0x426630, 'forced_spread': SPREAD,
                      'can_germinate': CAN_GERMINATE, 'place_tiberium': PLACE,
                      'overlay_ctor': 0x5FC380, 'overlay_mark': 0x5FC570,
                      'cell_recalc': 0x47D2B0,
                      'growth_queue': ADD_GROWTH, 'random': RANDOM, 'random_ranged': RANGED,
                      'terrain_type_ctor': 0x71DA80, 'spawns_read': 0x71DF27,
                      'instance_ctor_members': 0x71BB9E, 'instance_ctor_reset': 0x71BC86,
                      'animation_reads': 0x71E014, 'probability_store': 0x71E073},
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                   source_paths={'producer': Path(__file__),
                                 'harvest_field': Path(__file__).with_name('harvest_field.py'),
                                 'refinery_dock': Path(__file__).with_name('refinery_dock.py'),
                                 'type_inputs': Path(__file__).parent.parent / 'rules_oracle/bridge_landing_inputs.py',
                                 'physical_shapes': Path(__file__).parent.parent / 'sidebar_oracle/stock.py',
                                 'resident_tmp': Path(__file__).with_name('terrain_recalc.py'),
                                 'overlay_registries': Path(__file__).with_name('bridge_constructor.py')})
