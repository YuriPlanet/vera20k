"""Original hierarchy construction and local bridge-change rebuilds.

The original graph producers, vector copies, edge clears and pathfinder scratch
refresh run. Only successful bounded allocation and release are supplied.
"""
import struct
import sys
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.bridge_records import MAP, TABLE, CELLS, DUMMY, RECORDS
from tools.spatial_oracle.bridge_repair_zones import base_fixture, BASE, PLANE, QUERY, u32
from tools.spatial_oracle.terrain_recalc import LAT_GLOBALS

HEAP, HEAP_SIZE = 0x20000000, 0x1000000


def execute(case):
    uc, sp, width, side, _ = base_fixture(case)
    uc.mem_map(HEAP, HEAP_SIZE)
    cursor = HEAP

    def allocate(size):
        nonlocal cursor
        pointer = cursor
        cursor += (max(size, 1) + 15) & ~15
        assert cursor < HEAP + HEAP_SIZE
        uc.mem_write(pointer, bytes(max(size, 1)))
        return pointer

    queries, dummy_writes, calls, patch_calls, recalcs, high_pair_calls = [], [], [], [], [], []
    batch_queries = []

    def observe(_uc, address, _size, _data):
        if address == 0x578460:
            esp = uc.reg_read(UC_X86_REG_ESP)
            coord = struct.unpack('<hh', uc.mem_read(u32(uc, esp + 4), 4))
            queries.append(list(coord))
            assert u32(uc, esp + 8) == 1
            # Exact return addresses of586990's two outer mode-one queries.
            # Nested Recalc and584550 queries remain in the full transcript.
            if u32(uc, esp) in (0x5869BA, 0x586A5B):
                batch_queries.append(list(coord))
        if address in (0x581F90, 0x584550, 0x5824A0, 0x42C1C0):
            calls.append(hex(address))
        if address == 0x584550:
            esp = uc.reg_read(UC_X86_REG_ESP)
            patch_calls.append(list(struct.unpack('<hh', uc.mem_read(u32(uc, esp + 4), 4))))
        if address == 0x47D2B0:
            receiver = uc.reg_read(UC_X86_REG_ECX)
            recalcs.append(list(struct.unpack('<hh', uc.mem_read(receiver + 0x24, 4))))
        if address == 0x582D70:
            esp = uc.reg_read(UC_X86_REG_ESP)
            record = u32(uc, esp + 4)
            high_pair_calls.append([u32(uc, esp + 8),
                list(struct.unpack('<hh', uc.mem_read(record, 4))),
                list(struct.unpack('<hh', uc.mem_read(record + 4, 4)))])
        if address not in (0x7C8E17, 0x7C8B3D):
            return
        esp = uc.reg_read(UC_X86_REG_ESP)
        if address == 0x7C8E17:
            uc.reg_write(UC_X86_REG_EAX, allocate(u32(uc, esp + 4)))
        uc.reg_write(UC_X86_REG_EIP, u32(uc, esp))
        uc.reg_write(UC_X86_REG_ESP, esp + 4)

    def write(_uc, _access, address, size, value, _data):
        if address == DUMMY + 0x24:
            assert size == 4
            dummy_writes.append(list(struct.unpack('<hh', dwords(value))))

    uc.hook_add(UC_HOOK_CODE, observe)
    uc.hook_add(UC_HOOK_MEM_WRITE, write)

    def invoke(address, receiver, args=(), required=()):
        uc.mem_write(sp, dwords(RET_MAGIC, *args))
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_ECX, receiver)
        run_checked(uc, address, RET_MAGIC, count=3000000, required_addresses=required)

    classes, levels, base_ids = case['classes'], case['levels'], case['base_ids']
    assert len(classes) == len(levels) == len(base_ids) == width * width
    nodes = bytearray(b'\x07\x00\x00\x00' * (side * side))
    for y in range(width):
        for x in range(width):
            i = y * width + x
            struct.pack_into('<BBH', nodes, (y * side + x) * 4,
                             classes[i], levels[i], base_ids[i])
            uc.mem_write(CELLS + i * 0x200 + 0x11B, bytes([levels[i]]))
            if case.get('allocation') == 'size_diamond':
                w, h = case['size']
                if not (w < x + y <= w + 2 * h and abs(x-y) < w):
                    uc.mem_write(TABLE + (y * 512 + x) * 4, bytes(4))
    uc.mem_write(BASE, bytes(nodes))
    uc.mem_write(PLANE, bytes(side * side * 10))
    uc.mem_write(MAP + 0x60, dwords(0))
    uc.mem_write(0x87E8B8 + 0x40, bytes(9 * 4))
    raw_count = max(base_ids) + 1
    uc.mem_write(MAP + 0x4C, dwords(raw_count))
    raw_pointers = []
    raw_rows = []
    for row in range(13):
        values = [0] + [([1, 2, 65535][row % 3]) for _ in range(raw_count - 1)]
        data = struct.pack('<' + 'H' * raw_count, *values)
        pointer = allocate(len(data))
        uc.mem_write(pointer, data)
        uc.mem_write(MAP + 0x18 + row * 4, dwords(pointer))
        raw_pointers.append(pointer)
        raw_rows.append(data)

    # Original invalid-TMP Recalc branch, shared with terrain_recalc.py. It
    # retains current Cell height and recalculates its class/cache normally.
    # These cells isolate batch orchestration without substituting Recalc.
    rules = allocate(0x800)
    uc.mem_write(0x8871E0, dwords(rules))
    tiles = allocate(8)
    uc.mem_write(0xA8ED2C, dwords(tiles))
    uc.mem_write(0xA8ED38, dwords(2))
    for address in LAT_GLOBALS:
        uc.mem_write(address, dwords(-1))
    for land in range(9):
        uc.mem_write(0x89EA48 + land * 36, struct.pack('<f', 1.0))
    for i in range(width * width):
        uc.mem_write(CELLS + i * 0x200 + 0x38, dwords(2))
        uc.mem_write(CELLS + i * 0x200 + 0x44, dwords(-1))
        uc.mem_write(CELLS + i * 0x200 + 0xEC, dwords(3))
    if case.get('slope_recalc'):
        # Same relocated1x1 TMP fixture as terrain_recalc.py, with slope1.
        # Recalc changes the front-row mode1 result before batch pass two.
        head, tmp = allocate(0x400), allocate(88)
        image = bytearray(88)
        struct.pack_into('<5I', image, 0, 1, 1, 8, 4, tmp + 20)
        image[60:63] = bytes((3, 11, 1))
        uc.mem_write(tmp, bytes(image))
        uc.mem_write(head, dwords(0x7ECC48))
        uc.mem_write(head + 0xA4, dwords(tmp))
        uc.mem_write(head + 0x2C8, dwords(-1))
        uc.mem_write(head + 0x2D4, dwords(-1))
        uc.mem_write(tiles, dwords(head))
    records = case.get('records', [])
    uc.mem_write(MAP + 0x54, dwords(RECORDS, 64, 1, len(records), 10))
    for i, (a, b, active, tile) in enumerate(records):
        # High records only. Native582D70 reads raw endpoint-A identity and
        # the already supplied concrete high-set base100 from base_fixture.
        uc.mem_write(RECORDS + i * 16, packed(*a) + packed(*b) + dwords(active, 0))
        uc.mem_write(CELLS + (a[1] * width + a[0]) * 0x200 + 0x38, dwords(tile))

    # The zero-capacity58B070 constructor writes only its receiver; capture
    # that actual initialized24-byte value once for all supplied empty slots.
    template = allocate(24)
    invoke(0x58B070, template, [0, 0])
    uc.mem_write(template, dwords(0x7ED520))
    uc.mem_write(template + 16, dwords(0, 20))
    bucket_template = bytes(uc.mem_read(template, 24))
    for level in range(3):
        # MapClass565090 constructs the final36-byte record vector this way.
        # Original growth constructs capacity slots before copying into them.
        header = MAP + 0x8C + level * 24
        invoke(0x58AE60, header, [0, 0])
        uc.mem_write(header, dwords(0x7ED4A0))
        # InitZoneMap567110 replaces the constructor growth with this count.
        w, h = case.get('size', [8, 8])
        uc.mem_write(header + 16, dwords(0, w * h * 4 // (1 << (2 * (level + 1)))))
        buckets = allocate(256 * 24)
        root = allocate(16)
        uc.mem_write(root, dwords(buckets, 0x56CB80, 256, 20))
        uc.mem_write(MAP + 0x80 + level * 4, dwords(root))
        # Init_Alloc565800 selects this12-byte-vector vtable. All subsequent
        # original virtual clear/growth/copy operations execute normally.
        uc.mem_write(buckets, bucket_template * 256)

    if 'flood' in case:
        spec = case['flood']
        for y in range(5, 8):
            for x in range(5, 9):
                seed = y == 6 and x in (6, 7)
                zone = 0 if seed else 2
                if spec.get('vertical') and y == 6 and x in (5, 8):
                    zone = 0
                pointer = PLANE + (y * side + x) * 10
                uc.mem_write(pointer, struct.pack('<HHHHBB', zone, 0, 0, 1 if seed else 9, 0, 0))
        for x, y in spec.get('missing', []):
            uc.mem_write(TABLE + (y * 512 + x) * 4, bytes(4))
        root = u32(uc, MAP + 0x80)
        buckets = u32(uc, root)
        if spec.get('seed_pair'):
            pointer = buckets + 0x2A * 24
            data = allocate(12)
            pair = (2 << 16) | 10
            uc.mem_write(data, dwords(pair, pair, 1))
            uc.mem_write(pointer + 4, dwords(data, 1, 1, 1, 20))
        block = allocate(16)
        uc.mem_write(block, dwords(6, 6, 2, 1))
        uc.mem_write(QUERY, packed(6, 6))
        invoke(0x5824A0, MAP, [PLANE + (6 * side + 6) * 10, 0, 10, block, QUERY])
        pairs = []
        for bucket in range(256):
            pointer = buckets + bucket * 24
            for i in range(u32(uc, pointer + 16)):
                data = u32(uc, pointer + 4) + i * 12
                pair = u32(uc, data)
                assert pair == u32(uc, data + 4)
                pairs.append([pair >> 16, pair & 65535, uc.mem_read(data + 8, 1)[0]])
        return dict(input=case, pairs=pairs, queries=queries, dummy_writes=dummy_writes,
                    dummy=list(struct.unpack('<hh', uc.mem_read(DUMMY + 0x24, 4))),
                    return_advance=uc.reg_read(UC_X86_REG_EAX))

    for level in (2, 1, 0):
        invoke(0x581F90, MAP, [level], [0x5824A0])

    def snapshot():
        plane = bytes(uc.mem_read(PLANE, side * side * 10))
        graphs = []
        for level in range(3):
            count = u32(uc, MAP + 0x74 + level * 4)
            header = MAP + 0x8C + level * 24
            assert count == u32(uc, header + 16) <= u32(uc, header + 8)
            pointer = u32(uc, header + 4)
            records = []
            for zone in range(count):
                node = pointer + zone * 36
                parent = struct.unpack('<H', uc.mem_read(node + 24, 2))[0]
                edge_count = u32(uc, node + 16)
                edge_ptr = u32(uc, node + 4)
                edges = [[u32(uc, edge_ptr + i * 8), uc.mem_read(edge_ptr + i * 8 + 4, 1)[0]]
                         for i in range(edge_count)]
                records.append(dict(parent=parent, zone_type=u32(uc, node + 28), edges=edges))
            ids = [struct.unpack_from('<H', plane, (y * side + x) * 10 + level * 2)[0]
                   for y in range(width) for x in range(width)]
            padding_ids = [struct.unpack_from('<H', plane, (y * side + x) * 10 + level * 2)[0]
                           for y in range(side) for x in range(side) if x >= width or y >= width]
            graphs.append(dict(ids=ids, padding_ids=padding_ids, records=records))
        retained = bytes(uc.mem_read(BASE, side * side * 4))
        assert plane[8::10] == retained[1::4]
        for row, (pointer, data) in enumerate(zip(raw_pointers, raw_rows)):
            assert u32(uc, MAP + 0x18 + row * 4) == pointer
            assert bytes(uc.mem_read(pointer, len(data))) == data
        assert u32(uc, MAP + 0x4C) == raw_count
        recalculated_cells = []
        for x, y in recalcs:
            index = y * 512 + x
            pointer = u32(uc, TABLE + index * 4) if 0 <= index < 0x40000 else 0
            pointer = pointer or DUMMY
            recalculated_cells.append(dict(coord=[x, y], zone=u32(uc, pointer + 0x4C),
                level=uc.mem_read(pointer + 0x11B, 1)[0], slope=uc.mem_read(pointer + 0x11C, 1)[0]))
        return dict(graphs=graphs, queries=list(queries), dummy_writes=list(dummy_writes),
                    dummy=list(struct.unpack('<hh', uc.mem_read(DUMMY + 0x24, 4))), calls=list(calls),
                    patch_calls=list(patch_calls), recalcs=list(recalcs), recalculated_cells=recalculated_cells,
                    high_pair_calls=list(high_pair_calls), batch_queries=list(batch_queries),
                    classes=[retained[(y * side + x) * 4] for y in range(width) for x in range(width)],
                    levels=[retained[(y * side + x) * 4 + 1] for y in range(width) for x in range(width)],
                    base_ids=[struct.unpack_from('<H', retained, (y * side + x) * 4 + 2)[0]
                              for y in range(width) for x in range(width)],
                    raw_rows=[list(struct.unpack('<' + 'H' * raw_count, data)) for data in raw_rows])

    initial = snapshot()
    states = []
    for action in case.get('actions', []):
        queries.clear(); dummy_writes.clear(); calls.clear(); patch_calls.clear(); recalcs.clear(); high_pair_calls.clear()
        batch_queries.clear()
        for change in action.get('changes', []):
            x, y = change['cell']; i = y * width + x; native = y * side + x
            if 'cell_level' in change:
                uc.mem_write(CELLS + i * 0x200 + 0x11B, bytes([change['cell_level']]))
            if 'tile' in change:
                uc.mem_write(CELLS + i * 0x200 + 0x38, dwords(change['tile']))
            if 'class' in change:
                uc.mem_write(BASE + native * 4, bytes([change['class']]))
            if 'level' in change:
                uc.mem_write(BASE + native * 4 + 1, bytes([change['level']]))
                uc.mem_write(PLANE + native * 10 + 8, bytes([change['level']]))
            if 'base_id' in change:
                uc.mem_write(BASE + native * 4 + 2, struct.pack('<H', change['base_id']))
            if change.get('clear_level0'):
                uc.mem_write(PLANE + native * 10, bytes(2))
            if change.get('missing'):
                uc.mem_write(TABLE + (y * 512 + x) * 4, bytes(4))
        if 'bounds' in action:
            uc.mem_write(MAP + 0xFC, dwords(*action['bounds']))
        before_base = bytes(uc.mem_read(BASE, side * side * 4))
        if 'cells' in action:
            vector = allocate(24)
            coords = allocate(4 * len(action['cells']))
            uc.mem_write(vector, dwords(0, coords, len(action['cells']), 1, len(action['cells']), 10))
            for i, coord in enumerate(action['cells']):
                uc.mem_write(coords + i * 4, packed(*coord))
            invoke(0x586990, MAP, [vector])
        else:
            uc.mem_write(QUERY, packed(*action['coord']))
            invoke(0x584550, MAP, [QUERY])
        after_base = bytes(uc.mem_read(BASE, side * side * 4))
        assert after_base[2::4] == before_base[2::4] and after_base[3::4] == before_base[3::4]
        if 'cells' not in action:
            assert after_base == before_base
        states.append(snapshot())
    return dict(input=case, width=width, initial=initial, states=states)


def inputs():
    classes = [0 if 8 < x + y <= 24 and abs(x-y) < 8 else 7
               for y in range(16) for x in range(16)]
    yield dict(name='clear_local', size=[8, 8], classes=classes, levels=[0] * 256,
               base_ids=[1 if value != 7 else 0 for value in classes], actions=[dict(coord=[10, 10])])
    for coords in ([[10, 10], [12, 10]], [[12, 10], [10, 10]],
                   [[10, 10], [10, 10], [12, 10]], [[10, 10], [11, 10]]):
        yield dict(name=f'batch_{coords}', size=[8, 8], classes=classes, levels=[0] * 256,
                   base_ids=[1 if value != 7 else 0 for value in classes],
                   actions=[dict(cells=coords)])
    for name, actions in (
        ('live_height_excludes_cached_ground', [dict(coord=[11, 10],
            changes=[dict(cell=[10, 10], cell_level=20)])]),
        ('live_playfield_parent_for_cached_outside', [dict(coord=[10, 10], bounds=[0, 0, 8, 7],
            changes=[dict(cell=[12, 10], clear_level0=True, **{'class':7})])]),
        ('batch_refreshes_retained_height', [dict(cells=[[10, 10], [12, 10]],
            changes=[dict(cell=[12, 10], cell_level=4)])]),
        ('batch_missing_and_outside', [dict(cells=[[-1, 0], [5, 5], [10, 10], [5, 5]],
            changes=[dict(cell=[5, 5], missing=True)])]),
        ('repeated_local_preserves_holes', [dict(coord=[10, 10]), dict(coord=[10, 10])]),
    ):
        yield dict(name=name, size=[8, 8], classes=classes, levels=[0] * 256,
                   base_ids=[1 if value != 7 else 0 for value in classes], actions=actions)
    yield dict(name='batch_recalc_slope_changes_second_pass_admission', size=[8, 8],
               classes=classes, levels=[0] * 256, slope_recalc=True,
               base_ids=[1 if value != 7 else 0 for value in classes],
               actions=[dict(cells=[[5, 4], [10, 10]], changes=[dict(cell=[5, 4], tile=0)])])
    yield dict(name='active_high_records_local_reinsertion', size=[8, 8], classes=classes,
               levels=[0] * 256, base_ids=[1 if value != 7 else 0 for value in classes],
               records=[[[5, 5], [10, 5], True, 103], [[6, 5], [11, 5], True, 103],
                        [[6, 6], [11, 6], False, 103]],
               actions=[dict(coord=[5, 5]), dict(coord=[6, 5])])
    # Normalized bounds and Resize membership; supplied height252 is signed-4
    # to the live predicate. This is not a stock nonnegative-height witness.
    width = 19
    alias_levels = [10 if (x, y) == (16, 10) else 252 if (x, y) == (3, 9) else 0
                    for y in range(width) for x in range(width)]
    alias_classes = [0 if (x, y) in ((16, 10), (3, 9)) or
                    (15 < x + y <= 17 and abs(x-y) < 7) else 7
                     for y in range(width) for x in range(width)]
    yield dict(name='allocated_signed_height_partial_block_alias', size=[11, 8], bounds=[2, 2, 7, 0],
               allocation='size_diamond', classes=alias_classes, levels=alias_levels,
               base_ids=[1 if value != 7 else 0 for value in alias_classes],
               actions=[dict(coord=[16, 10])])

    # Coherent initial attributes, normalized bounds and actual Resize allocation.
    # A raw +4 write followed by586990 can admit class0 while retaining baseID0.
    # The hierarchy then crosses native padding and aliases32 represented cells.
    heights = {(16, 8):4, (17, 8):3, (18, 8):1, (16, 9):5, (17, 9):6,
               (18, 9):2, (16, 10):5, (17, 10):7, (16, 11):8}
    padding_levels = [heights.get((x, y), 0) for y in range(width) for x in range(width)]
    padding_classes = [0 if (x, y) in ((11, 5), (10, 6), (11, 6), (9, 7), (10, 7),
        (8, 8), (9, 8), (7, 9), (8, 9), (6, 10), (7, 10), (5, 11), (6, 11)) else 7
        for y in range(width) for x in range(width)]
    padding_base = [1 if value != 7 else 0 for value in padding_classes]
    yield dict(name='allocated_nonnegative_height_batch_padding', size=[11, 8], bounds=[2, 2, 7, 0],
        allocation='size_diamond', classes=padding_classes, levels=padding_levels, base_ids=padding_base,
        actions=[dict(cells=[[16, 10]], changes=[dict(cell=[16, 10], cell_level=9)])])
    # Full load-time reconstruction from that retained cache; also re-patch it.
    retained_classes, retained_levels = list(padding_classes), list(padding_levels)
    retained_classes[10 * width + 16] = 0
    retained_levels[10 * width + 16] = 9
    for name, actions in (('retained_base0_padding_full', []),
                          ('retained_base0_padding_local', [dict(coord=[16, 10])])):
        yield dict(name=name, size=[11, 8], bounds=[2, 2, 7, 0], allocation='size_diamond',
            classes=retained_classes, levels=retained_levels, base_ids=padding_base, actions=actions)

    # A preceding56D460/56D5A0 returns for retained class7; its caller still runs
    # the independent584550 below with the original coordinate and live Cell.
    yield dict(name='sentinel_base_still_updates_live_hierarchy', size=[8, 8],
        classes=classes, levels=[0] * 256,
        base_ids=[1 if value != 7 else 0 for value in classes],
        actions=[dict(coord=[10, 10], changes=[dict(cell=[10, 10], **{'class':7})])])

    # Keep the retained plane from the wider bounds while581F90 reads the
    # narrower current bounds. This distinguishes live full-world construction
    # from replacing mode1 membership with cached-class admission.
    yield dict(name='full_live_bounds_narrower_than_cached_classes', size=[8, 8],
        bounds=[0, 0, 8, 4], classes=classes, levels=[0] * 256,
        base_ids=[1 if value != 7 else 0 for value in classes], actions=[])


def flood_inputs():
    for name, spec in (
        ('horizontal_repeated_neighbor', dict(missing=[[5, 6]])),
        ('horizontal_existing_pair_retains_flag', dict(missing=[[5, 6]], seed_pair=True)),
        ('vertical_reference_first', dict(vertical=True, missing=[[6, 6], [5, 5]])),
    ):
        yield dict(name=name, size=[8, 8], classes=[0] * 256, levels=[0] * 256,
                   base_ids=[1] * 256, flood=spec)


def cases():
    def collect(rows):
        result = []
        for case in rows:
            result.append(execute(case))
            print('Native hierarchy:', case['name'], file=sys.stderr, flush=True)
        return result
    result = dict(cases=collect(inputs()), floods=collect(flood_inputs()))
    rows = {row['input']['name']: row for row in result['cases']}
    assert len(rows) == 18 and len(result['floods']) == 3
    # Cached classes still seed the same IDs; live bounds change graph edges.
    narrow = rows['full_live_bounds_narrower_than_cached_classes']['initial']
    clear = rows['clear_local']['initial']
    assert narrow['graphs'][0]['ids'] == clear['graphs'][0]['ids']
    assert narrow['graphs'][0]['records'] != clear['graphs'][0]['records']
    slope = rows['batch_recalc_slope_changes_second_pass_admission']['states'][0]
    assert slope['recalcs'] == [[10, 10], [5, 4]] and slope['patch_calls'] == [[10, 10]]
    assert slope['recalculated_cells'][1]['slope'] == 1 and slope['classes'][4 * 16 + 5] == 7
    parent = rows['live_playfield_parent_for_cached_outside']['states'][0]
    assert parent['graphs'][0]['records'][0]['parent'] != 0
    alias = rows['allocated_signed_height_partial_block_alias']
    assert alias['initial']['graphs'][2]['ids'][9 * 19 + 3] != 0
    assert alias['states'][0]['graphs'][2]['ids'][9 * 19 + 3] == 0
    for name, source in (('allocated_nonnegative_height_batch_padding', 'states'),
                         ('retained_base0_padding_full', 'initial')):
        row = rows[name]
        state = row['states'][0] if source == 'states' else row['initial']
        assert any(state['graphs'][2]['padding_ids'])
        assert sum(state['graphs'][2]['ids'][y * 19 + x] != 0
                   for y in range(9, 17) for x in range(4)) == 32
    high = rows['active_high_records_local_reinsertion']
    assert [call[1] for call in high['initial']['high_pair_calls']] == [[5, 5], [6, 5]] * 3
    for state, fine_endpoint in zip(high['states'], ([5, 5], [6, 5])):
        # Both endpoints intersect the coarse8/4 blocks; only the selected
        # endpoint intersects the final2 block. Each eligible set is reversed.
        assert [call[1] for call in state['high_pair_calls']] == [[6, 5], [5, 5]] * 2 + [fine_endpoint]
    return result


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original581F90 hierarchy construction,584550 local rebuild and586990 two-pass batch including original Recalc',
        assumptions=['Supplied native Size, bounds, class/height/base-ID planes and allocated Cell table',
                     'Original graph/bucket vector constructors and successful bounded storage; allocation failure and native signed-negative node pointers excluded',
                     'Most batch cells use invalid tile2 with supplied registry length2; one supplied resident1x1 Road TMP changes slope0 to1 and changes second-pass admission; no overlay/objects, CliffBack0 and Wheel1.0 for all lands',
                     'Most small supplied bounds are branch fixtures, not normalized retail-map dimensions; alias case uses normalized11,8 bounds and Resize allocation with supplied signed-negative Cell height252, not a stock nonnegative-height witness',
                     'Empty temporary bucket values copied from one original zero-capacity constructor result; native per-bucket virtual clear/growth executes',
                     'All13 input raw rows contain supplied sentinel/ordinary labels and remain byte-identical; all base-ID words remain unchanged through local/batch actions',
                     'One full/local case supplies two active and one inactive high record with raw high tile103; full forward and local reverse582D70 reinsertion execute, no Tube/TS claims',
                     'Three direct flood witnesses supply a2x1 seed run, surrounding neighbor2 and optional preexisting flag1 pair; compare low flag byte only',
                     'Three normalized11,8/Resize/nonnegative-height cases retain baseID0 after a supplied raw +4 height change; actual batch, full reconstruction and local re-patch preserve mutable padding and32 aliased represented cells',
                     'One local target retains class7 while the live query admits it; one full build retains wider cached classes with narrower supplied current bounds, preserving IDs while changing ordered edges',
                     'Not a complete native map-load or gameplay execution'],
        substitutions=['Successful bounded operator_new7C8E17 and no-op operator_delete7C8B3D'],
        entry_points={'build':0x581F90, 'local':0x584550, 'flood':0x5824A0, 'batch':0x586990, 'recalc':0x47D2B0,
                      'playfield':0x578460, 'scratch':0x42C1C0, 'high_pairs':0x582D70}))
