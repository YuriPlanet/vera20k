"""Original4FD150 center/radius projection, stopping before sector work."""
from pathlib import Path
import argparse
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_FPCW
from tools.native_oracle import finish_vectors, load_image, run_checked, provenance, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC
from tools.spatial_oracle.map_queries import dwords, packed
from tools.rmg_oracle.gen_rng_vectors import seeded_struct, draws, STRUCT_LEN
HOUSE = SCRATCH
COUNTRY = SCRATCH + 24576
OBJECTS = SCRATCH + 28672
TYPES = SCRATCH + 65536
RULES = SCRATCH + 139264
PAD = SCRATCH + 151552
FREE = SCRATCH + 159744
CELLS = SCRATCH + 196608
VECTOR = SCRATCH + 163840
BASE = SCRATCH + 393216
RAW = SCRATCH + 401408
MAP = 8910824
TABLE = 12582912
DUMMY = 11263056


def initialize_nearby_zone_plane(u, *, size, plane, rows):
    """Existing flat FNPC fixture storage, shared with composed locomotion.

    This supplies an empty base-group plane and one Normal row (labels1/2);
    original zone/FNPC/projection bodies own every later query. It does not
    claim native map loading or connectivity construction.
    """
    width, height = size
    count = (width + height + 1) ** 2
    u.mem_write(MAP + 0x68, dwords(plane, count))
    u.mem_write(plane, bytes(count * 4))
    u.mem_write(MAP + 0x18, dwords(rows))
    u.mem_write(rows, packed(1, 2))


def make_fixture(row):
    """Shared physical House/Building/Map fixture; no original code is patched."""
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 458752)
    u.mem_map(RET_MAGIC, 4096)
    u.reg_write(UC_X86_REG_FPCW, 3711)
    u.mem_write(HOUSE + 52, dwords(COUNTRY))
    u.mem_write(COUNTRY + 276, struct.pack('<5f', 1, 1, 1, 1, 1))
    u.mem_write(HOUSE + 21392, struct.pack('<5f', 1, row.get('unit_bonus', 1), 1, 1, 1))
    u.mem_write(8942048, dwords(RULES))
    u.mem_write(RULES + 2908, dwords(PAD))
    u.mem_write(RULES + 6120, b'\x01')
    u.mem_write(PAD, dwords(PAD + 256, PAD + 256))
    u.mem_write(PAD + 256 + 1004, dwords(PAD + 2048))
    u.mem_write(FREE, dwords(8348184))
    u.mem_write(FREE + 1552, dwords(1400))
    u.mem_write(HOUSE + 752, dwords(row['tracked_count']))
    u.mem_write(HOUSE + 21648, packed(14, 14))
    u.mem_write(HOUSE + 21652, packed(*row.get('alternate', [0, 0])))
    u.mem_write(HOUSE + 21656, dwords(9999))
    u.mem_write(HOUSE + 108, dwords(VECTOR))
    u.mem_write(HOUSE + 120, dwords(len(row['buildings'])))
    for i, b in enumerate(row['buildings']):
        actor = OBJECTS + i * 4096
        typ = TYPES + i * 8192
        u.mem_write(VECTOR + i * 4, dwords(actor))
        u.mem_write(actor, dwords(8273596))
        u.mem_write(actor + 1312, dwords(typ))
        u.mem_write(actor + 108, dwords(b.get('health', 100)))
        u.mem_write(actor + 129, bytes([b.get('limbo', False)]))
        u.mem_write(actor + 156, dwords(*b['xyz']))
        u.mem_write(typ, dwords(8275312))
        u.mem_write(typ + 1552, dwords(b['cost']))
        u.mem_write(typ + 3744, dwords(FREE if b.get('free', False) else 0))
        u.mem_write(typ + 3824, dwords(0))
    u.mem_write(MAP + 244, dwords(8, 8, 0, 0, 8, 8))
    u.mem_write(MAP + 316, dwords(TABLE, 262144))
    u.mem_write(TABLE, bytes(1048576))
    initialize_nearby_zone_plane(u, size=(8, 8), plane=BASE, rows=RAW)
    u.mem_write(9038400, struct.pack('<90f', *[1.0] * 90))
    u.mem_write(11070852, dwords(100))
    u.mem_write(DUMMY, bytes(512))
    u.mem_write(DUMMY, dwords(8277740))
    u.mem_write(DUMMY + 68, dwords(-1))
    for y in range(17):
        for x in range(17):
            c = CELLS + (y * 17 + x) * 512
            u.mem_write(c, dwords(8277740))
            u.mem_write(c + 36, packed(x, y))
            u.mem_write(c + 68, dwords(-1))
            u.mem_write(TABLE + (y * 512 + x) * 4, dwords(c))
    for p in (9037760, 11277256, 11263624):
        u.mem_write(p, dwords(104))
    return u


def query(row, *, trace_geometry=False):
    """Original 4FD150 through its existing center/radius publication boundaries."""
    u = make_fixture(row)
    native_calls = []
    geometry_writes = []
    original = bytes(u.mem_read(0x4FD150, 0x3B1))
    if trace_geometry:
        def trace(_u, pc, _size, _data):
            sp = u.reg_read(UC_X86_REG_ESP)
            if pc == 0x45EDD0:
                typ = u.reg_read(UC_X86_REG_ECX)
                native_calls.append(dict(
                    cost_index=(typ - TYPES) // 8192,
                    raw_cost=struct.unpack("<i", u.mem_read(typ + 0x610, 4))[0],
                    caller=f"0x{read_u32(u, sp):08X}",
                ))
            elif pc == 0x447AC0:
                actor = u.reg_read(UC_X86_REG_ECX)
                native_calls.append(dict(
                    coordinate_index=(actor - OBJECTS) // 4096,
                    raw_xyz=list(struct.unpack("<iii", u.mem_read(actor + 0x9C, 12))),
                    caller=f"0x{read_u32(u, sp):08X}",
                ))
            elif pc == 0x4FD22F:
                native_calls.append(dict(weight_total=struct.unpack("<i", u.mem_read(sp + 0x10, 4))[0]))
            elif pc == 0x4FD248:
                native_calls.append(dict(sum_x=u.reg_read(UC_X86_REG_EBP), sum_y=u.reg_read(UC_X86_REG_EBX)))

        def written(_u, _access, address, size, value, _data):
            if address in (HOUSE + 0x5490, HOUSE + 0x5498):
                geometry_writes.append(dict(
                    field="primary" if address == HOUSE + 0x5490 else "radius",
                    size=size, raw=value, pc=f"0x{u.reg_read(UC_X86_REG_EIP):08X}",
                ))

        u.hook_add(UC_HOOK_CODE, trace)
        u.hook_add(UC_HOOK_MEM_WRITE, written)
    events = []

    def observe(_u, a, _s, _d):
        if a in (4582864, 4487872, 5692448, 5230987, 5231313, 5231657, 5231854):
            events.append(hex(a))
    u.hook_add(UC_HOOK_CODE, observe)
    sp = STACK_BASE + STACK_SIZE - 4096
    u.mem_write(sp, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, HOUSE)
    run_checked(u, 5230928, (5231663, 5231864), count=2000000 if trace_geometry else 200000, required_addresses=[5230928, 5230987])
    result = dict(input=row, primary=list(struct.unpack('<hh', u.mem_read(HOUSE + 21648, 4))), radius=struct.unpack('<i', u.mem_read(HOUSE + 21656, 4))[0], endpoint=hex(u.reg_read(UC_X86_REG_EIP)), events=events)
    assert original == bytes(u.mem_read(0x4FD150, 0x3B1))
    if trace_geometry:
        result.update(native_calls=native_calls, geometry_writes=geometry_writes, original_projection_code_unchanged=True)
    return result

def generate():
    first = dict(cost=2000, free=True, xyz=[5 * 256 + 128, 5 * 256 + 128, 0])
    second = dict(cost=1000, xyz=[13 * 256 + 128, 9 * 256 + 128, 0])
    inputs = [dict(tracked_count=0, buildings=[first, second]), dict(tracked_count=1, buildings=[]), dict(tracked_count=2, buildings=[first, second]), dict(tracked_count=2, buildings=[first, second], unit_bonus=0.75), dict(tracked_count=2, buildings=[dict(first, limbo=True), second]), dict(tracked_count=2, buildings=[dict(first, health=0), second]), dict(tracked_count=2, buildings=[first, second], alternate=[1, 1])]
    return [query(r) for r in inputs]


def read_u32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def signed_dword(value):
    return struct.unpack('<i', dwords(value))[0]


RETURN_SPANS = {
    'ordinary_return': (0x500200, 0xF9),
    'random_cell': (0x501AC0, 0x628),
    'random_direction': (0x49F420, 0x121),
    'base_cell': (0x50DEF0, 0x3C),
    'clamp': (0x586E50, 0x169),
    'nearby': (0x56DC20, 0xBA0),
    'zone': (0x56D230, 0x16A),
    'raw_random': (0x65C780, 0x51),
    'ranged_random': (0x65C7E0, 0xAD),
    'cell_ground': (0x486840, 0x41),
}
RETURN_ARGUMENT_COUNTS = {
    0x65C7E0: 2, 0x56D230: 3, 0x56DC20: 15,
    0x586E50: 2, 0x49F420: 2, 0x578460: 2,
}


def query_return(row):
    """Execute original 500200 with Unit/Infantry vtables and trace its callees."""
    u = make_fixture(dict(tracked_count=0, buildings=[]))
    scenario = SCRATCH + 0x66000
    rng = scenario + 0x218
    actor = OBJECTS
    output = SCRATCH + 0x65F00
    vtable = {'MTNK': 0x7F5C70, 'E1': 0x7EB058}[row['family']]
    u.mem_write(actor, bytes(0x700))
    u.mem_write(actor, dwords(vtable))
    u.mem_write(actor + 0x9C, dwords(*row.get('current', [2688, 2688, 0])))
    u.mem_write(actor + 0x8C, bytes([row.get('on_bridge', False)]))
    u.mem_write(HOUSE + 0x5490, packed(*row.get('primary', [8, 8])))
    u.mem_write(HOUSE + 0x5494, packed(*row.get('alternate', [0, 0])))
    u.mem_write(HOUSE + 0x5498, dwords(row['radius']))
    for cell in row.get('cells', []):
        x, y = cell['xy']
        assert 0 <= x < 17 and 0 <= y < 17
        address = CELLS + (y * 17 + x) * 512
        if 'level' in cell:
            u.mem_write(address + 0x11B, bytes([cell['level'] & 255]))
        if 'slope' in cell:
            u.mem_write(address + 0x11C, bytes([cell['slope']]))
        if 'flags' in cell:
            u.mem_write(address + 0x140, dwords(cell['flags']))
        if 'land' in cell:
            u.mem_write(address + 0xEC, dwords(cell['land']))
    dummy = row.get('dummy', dict(xy=[-7, 1], level=0, slope=0))
    u.mem_write(DUMMY + 0x24, packed(*dummy['xy']))
    u.mem_write(DUMMY + 0x11B, bytes([dummy['level'] & 255, dummy['slope']]))
    if 'bounds' in row:
        u.mem_write(MAP + 0xF4, dwords(*row['bounds']))
    for index, zone in enumerate(row.get('zone_overrides', []), start=1):
        x, y = zone['xy']
        assert 0 <= x < 17 and 0 <= y < 17
        u.mem_write(BASE + (y * 17 + x) * 4 + 2, struct.pack('<H', index))
        u.mem_write(RAW + index * 2, struct.pack('<H', zone['group']))
    initial_rng = seeded_struct(row['seed'])
    u.mem_write(rng, initial_rng)
    u.mem_write(0xA8B230, dwords(scenario))
    u.mem_write(output, packed(123, 456))
    original_code = {name: bytes(u.mem_read(p, n)) for name, (p, n) in RETURN_SPANS.items()}
    original_tables = {hex(p): bytes(u.mem_read(p, 0x600)) for p in (0x7F5C70, 0x7EB058, 0x7E4E6C)}
    events = []
    pending = []
    visited = set()
    dummy_writes = []

    def xy(address):
        return list(struct.unpack('<hh', u.mem_read(address, 4)))

    def coordinate(address):
        return list(struct.unpack('<iii', u.mem_read(address, 12)))

    def observe(_u, pc, _size, _data):
        visited.add(pc)
        sp = u.reg_read(UC_X86_REG_ESP)
        for item in pending[:]:
            if pc == item['return_pc']:
                item['event']['result'] = signed_dword(u.reg_read(UC_X86_REG_EAX))
                if 'out' in item:
                    item['event']['output'] = xy(item['out'])
                if 'coord_out' in item:
                    item['event']['output'] = coordinate(item['coord_out'])
                pending.remove(item)
        if pc in RETURN_ARGUMENT_COUNTS:
            args = [read_u32(u, sp + 4 + i * 4) for i in range(RETURN_ARGUMENT_COUNTS[pc])]
            event = dict(
                entry=f'0x{pc:08X}', caller=f'0x{read_u32(u, sp):08X}',
                arguments=[signed_dword(a) for a in args], dummy=xy(DUMMY + 0x24),
            )
            item = dict(event=event, return_pc=read_u32(u, sp))
            if pc == 0x56D230:
                event['xy'] = xy(args[0])
            elif pc == 0x56DC20:
                event['seed'] = xy(args[1])
                event['target'] = xy(args[12])
                event['packed_arguments'] = args[2:12] + args[13:]
                item['out'] = args[0]
            elif pc == 0x586E50:
                event['xy'] = xy(args[1])
                item['out'] = args[0]
            elif pc == 0x49F420:
                event['current'] = coordinate(u.reg_read(UC_X86_REG_EDX))
                item['coord_out'] = u.reg_read(UC_X86_REG_ECX)
            elif pc == 0x578460:
                event['xy'] = xy(args[0])
            events.append(event)
            pending.append(item)
        elif pc == 0x501AC0:
            events.append(dict(entry='0x00501AC0', variant=read_u32(u, sp + 8), radius=signed_dword(read_u32(u, HOUSE + 0x5498))))
        elif pc == 0x501B15:
            events.append(dict(clamped_radius=signed_dword(u.reg_read(UC_X86_REG_EAX))))
        elif pc == 0x65C84B:
            events.append(dict(raw_draw=u.reg_read(UC_X86_REG_ESI), owner='ranged', indices=[read_u32(u, rng + 4), read_u32(u, rng + 8)]))
        elif pc == 0x65C7D0:
            events.append(dict(raw_draw=u.reg_read(UC_X86_REG_EAX), owner='direction', indices=[read_u32(u, rng + 4), read_u32(u, rng + 8)]))

    def written(_u, _access, address, size, value, _data):
        if address == DUMMY + 0x24:
            assert size == 4
            dummy_writes.append(dict(pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}', xy=list(struct.unpack('<hh', dwords(value)))))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, written)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC, output, actor))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, HOUSE)
    run_checked(u, 0x500200, RET_MAGIC, count=300000, required_addresses=[
        0x500200, 0x501AC0, 0x49F420, 0x65C7E0, 0x65C780, 0x56D230, 0x56DC20,
    ])
    assert u.reg_read(UC_X86_REG_ESP) == sp + 12
    assert u.reg_read(UC_X86_REG_EAX) == output
    assert not pending, pending
    final_rng = bytes(u.mem_read(rng, STRUCT_LEN))
    raw_draws = [event['raw_draw'] for event in events if 'raw_draw' in event]
    standalone_draws, standalone_rng = draws(initial_rng, len(raw_draws))
    assert raw_draws == standalone_draws
    assert final_rng == standalone_rng
    assert original_code == {name: bytes(u.mem_read(p, n)) for name, (p, n) in RETURN_SPANS.items()}
    assert original_tables == {hex(p): bytes(u.mem_read(p, 0x600)) for p in (0x7F5C70, 0x7EB058, 0x7E4E6C)}
    return dict(
        input=row, output=xy(output), events=events,
        raw_draw_count=len(raw_draws), raw_draws=raw_draws,
        rng_before=initial_rng.hex(), rng_after=final_rng.hex(),
        rng_after_sha256=hashlib.sha256(final_rng).hexdigest(),
        final_dummy=xy(DUMMY + 0x24), dummy_writes=dummy_writes,
        clamp_executed=0x586E50 in visited, original_code_and_vtables_unchanged=True,
    )


def generate_ordinary_return():
    """Additional 4FD150 edge rows and ordinary concrete-class 500200 rows."""
    first = dict(cost=2000, free=True, xyz=[1408, 1408, 0])
    second = dict(cost=1000, xyz=[3456, 2432, 0])
    center = [2176, 2176, 0]
    wrapped = [2176 + 0x40000000, 2176 + 0x40000000, 0]
    projection_inputs = [
        dict(name='cost_minus999_weight1', tracked_count=1, buildings=[dict(cost=-999, xyz=[1408, 1408, 0])]),
        dict(name='cost_minus1000_weight0', tracked_count=1, buildings=[dict(cost=-1000, xyz=[1408, 1408, 0])]),
        dict(name='cost_minus1001_weight0', tracked_count=1, buildings=[dict(cost=-1001, xyz=[1408, 1408, 0])]),
        dict(name='cost_minus2000_weight_minus1', tracked_count=1, buildings=[dict(cost=-2000, xyz=[1408, 1408, 0])]),
        dict(name='zero_weight_still_in_radius', tracked_count=2, buildings=[dict(cost=-1000, xyz=[1408, 1408, 0]), second]),
        dict(name='negative_weight_still_in_radius', tracked_count=2, buildings=[dict(cost=-2000, xyz=[1408, 1408, 0]), second]),
        dict(name='owned_order_forward', tracked_count=2, buildings=[first, second]),
        dict(name='owned_order_reversed', tracked_count=2, buildings=[second, first]),
        dict(name='negative_current_cell', tracked_count=1, buildings=[dict(cost=0, xyz=[-1024, -257, 0])]),
        dict(name='weighted_xy_wrap', tracked_count=1, buildings=[dict(cost=3000, xyz=wrapped)]),
        dict(name='radius_sum_wrap', tracked_count=3, buildings=[dict(cost=3000, xyz=wrapped)] * 3),
        dict(name='radius_single_control', tracked_count=1, buildings=[dict(cost=3000, xyz=center)]),
    ]
    return_inputs = [
        dict(name=f'{family}_radius_{radius}', family=family, radius=radius, seed=31)
        for family in ('MTNK', 'E1')
        for radius in (0, 512, 767, 768, 769, 2047, 2048, 2049)
    ]
    return_inputs.extend([
        dict(name='alternate_origin', family='MTNK', radius=0, seed=31, alternate=[11, 7]),
        dict(name='current_on_bridge', family='MTNK', radius=0, seed=31, current=[2688, 2688, 416], on_bridge=True, cells=[dict(xy=[10, 10], flags=0x100)]),
        dict(name='base_ground_level2', family='MTNK', radius=0, seed=31, cells=[dict(xy=[8, 8], level=2)]),
        dict(name='base_bridge_level2', family='MTNK', radius=0, seed=31, cells=[dict(xy=[8, 8], level=2, flags=0x100)]),
        dict(name='narrow_bounds_ground', family='MTNK', radius=0, seed=31, bounds=[8, 8, 0, 0, 8, 2]),
        dict(name='narrow_bounds_level4', family='MTNK', radius=0, seed=31, bounds=[8, 8, 0, 0, 8, 2], cells=[dict(xy=[x, y], level=4) for y in range(17) for x in range(17)]),
        dict(name='invalid_primary', family='MTNK', radius=0, seed=31, primary=[0, 0]),
        dict(name='alternate_when_primary_invalid', family='E1', radius=0, seed=31, primary=[0, 0], alternate=[8, 8]),
        dict(name='signed_current_coordinate', family='E1', radius=0, seed=31, current=[-257, -1, 0]),
        dict(name='current_ground_zone2', family='MTNK', radius=0, seed=31, zone_overrides=[dict(xy=[10, 10], group=2)]),
        dict(name='current_bridge_ground_zone2', family='E1', radius=0, seed=31, current=[2688, 2688, 416], on_bridge=True, cells=[dict(xy=[10, 10], flags=0x100)], zone_overrides=[dict(xy=[10, 10], group=2)]),
        dict(name='current_raw_zone_ffff', family='MTNK', radius=0, seed=31, zone_overrides=[dict(xy=[10, 10], group=65535)]),
    ])
    return dict(
        projection=[query(row, trace_geometry=True) for row in projection_inputs],
        home_return=[query_return(row) for row in return_inputs],
    )


def ordinary_return_provenance():
    return provenance(
        scope=(
            'Original House4FD150 additional center/radius rows, stopping before4FD42F sector aggregation or at4FD4F8 epilogue; '
            'original House500200 through RET8 for concrete Unit/Infantry ordinary return, original501AC0 variant0, '
            '49F420 snap1,578460, conditional586E50,56D230 and56DC20. Full Scenario RNG draw order/count/state and '
            'shared Dummy writes are recorded. Native execution establishes supplied-state arithmetic and query behavior; '
            'not retail constructor/map loader, building lifecycle, Mission_Rescue dispatch/movement or whole bridge/Rust parity.'
        ),
        assumptions=[
            'Projection retains the existing shared House/Building/Map fixture: physical ordered Building array, independent supplied tracked_count, '
            'original Building/BuildingType/UnitType vtables, foundationindex0, supplied signed Cost and optional FreeUnit1400. '
            'Country/House factors1, SeparateAircrafttrue, valid nonmatchingpad/dock pointer. '
            'Negative Cost and extreme coordinates are explicit nonretail arithmetic stress inputs, not stock placement claims.',
            'House500200 uses original Unit7F5C70 or Infantry7EB058 vtables with supplied postconstruction actor coordinates/OnBridge; '
            'MTNK/E1 name ordinary concrete-class routes, not original type construction or INI reader execution. '
            'House primary8,8/alternate0,0 unless explicit, radius supplied. Original virtual+2D4/+2D8/+2DC bodies execute; '
            'all three are zero for these classes, selecting501AC0 variant0 without an extra RandomRanged1,4.',
            'Shared supplied map Size8,8 LocalSize0,0,8,8, allocated17x17 cells, no overlay/occupation, '
            'uniform ground raw zone1, all90landspeeds1; explicit bound/level/bridge/current-zone overrides per row. '
            'Ground zone2/FFFF inputs are direct zone-buffer premises, not flood-fill producer or retail topology proofs. '
            'Default Dummy(-7,1), level0/slope0; physical height globals104, frame100 and FPCW0E7F.',
            'Scenario raw RNG initialized by original65C6D0 with seed31; original65C7E0/65C780 execute in the chain. '
            'Observed raw draws and the entire final0x3F4 generator are cross-checked against the existing original standalone RNG owner. '
            'No methods or vtables are patched; observers only read and record native calls/writes; original code/vtable spans remain unchanged.',
        ],
        substitutions=[],
        entry_points={
            'projection': 0x4FD150, 'radius_published_stop': 0x4FD42F, 'epilogue_stop': 0x4FD4F8,
            'cost': 0x45EDD0, 'building_center': 0x447AC0, 'ordinary_return': 0x500200,
            'random_cell': 0x501AC0, 'random_direction': 0x49F420, 'base_cell': 0x50DEF0,
            'height_aware_bounds': 0x578460, 'clamp': 0x586E50, 'zone': 0x56D230,
            'nearby': 0x56DC20, 'cell_ground': 0x486840,
            'rng_constructor': 0x65C6D0, 'raw_random': 0x65C780, 'ranged_random': 0x65C7E0,
        },
    )


if __name__ == '__main__':
    selector = argparse.ArgumentParser(add_help=False)
    selector.add_argument('--ordinary-return', action='store_true')
    options, remaining = selector.parse_known_args()
    if options.ordinary_return:
        from tools import native_oracle
        from tools.rmg_oracle import gen_rng_vectors
        from tools.spatial_oracle import map_queries
        finish_vectors(
            generate_ordinary_return, Path(__file__).with_name('house_base_return.json'),
            provenance=ordinary_return_provenance, argv=remaining,
            source_paths={
                'tools/spatial_oracle/house_base_projection.py': Path(__file__),
                'tools/native_oracle.py': Path(native_oracle.__file__),
                'tools/spatial_oracle/map_queries.py': Path(map_queries.__file__),
                'tools/rmg_oracle/gen_rng_vectors.py': Path(gen_rng_vectors.__file__),
            },
        )
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(scope='original House4FD150 entry through center/radius publication, stopping before4FD42F sector aggregation or at4FD4F8 epilogue. Actual BuildingType+84 cost, Building+48 center and FNPC execute. Supplied tracking count, ordered Building records and cost multiplier premise; not count lifecycle or whole House AI/Rust parity.', assumptions=['House primary14,14/radius9999 to expose reset, optional alternate1,1. Declared object health/limbo, original Building/BuildingType/UnitType vtables, foundationindex0, suppliedCost/FreeUnit1400. House and country factors1 except explicitUnit.75. SeparateAircrafttrue; valid nonmatchingpad/dock pointer.', 'Map Size8,8 LocalSize0,0,8,8 with allocated17x17flatcells/nooverlay/rawoccupation0; uniformNormalrawgroup1, all90landspeeds1; frame100/FPCW0E7F and104heightconstants. No map loader, actual Building placement or House field producer claim.'], substitutions=[], entry_points={'projection': 5230928, 'radius_published_stop': 5231663, 'epilogue_stop': 5231864, 'cost': 4582864, 'building_center': 4487872, 'nearby': 5692448}), argv=remaining)
