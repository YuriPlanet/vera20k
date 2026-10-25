"""Placement mode of the shared Cell/SHP raster owner; no pixel/VM parser copy."""
import hashlib
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import configured_gamemd, provenance, run_checked
from tools.spatial_oracle.bridge_shadow_render import (
    prepare as shared_prepare, CELL, POINT, CLIP, WIDTH, HEIGHT, COUNT,
    PIXELS, Z, A, ints, snapshot,
)
from tools.spatial_oracle.building_body_rules import INI, RULES, SP, dwords
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.bridge_click_oracle import MATRIX_INITIALIZER


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def physical_ramp(m):
    """Existing original TMP/RawFile and theater reader owners, selected files."""
    from tools.spatial_oracle.tmp_clear_render import load_tmp_files, theater_tile_files
    u = m.u
    tiles, mapping = theater_tile_files(m, m.assets['URBANMD.INI'], 512, 'urb')
    m.phase = 'placement_tmp_loading'
    names = ('rmpfx03.urb', 'clat03.urb', 'clat04.urb', 'proad02.urb')
    loaded, file_io = load_tmp_files(m, {name: m.assets[name.upper()] for name in names})
    heads = {}
    for name in names:
        head = loaded[name]
        heads[name] = dict(pointer=head, slope=m.invoke(0x5471B0, head, (0,)) & 255,
                          land=m.invoke(0x544BE0, head, (0,)))
    ramp = heads['rmpfx03.urb']
    return dict(name='rmpfx03.urb', subtile=0, slope=ramp['slope'], land=ramp['land'], mapping=mapping,
        boundary='Original full lazy readers/getters; selected pristine registrations are supplied for the following full Cell Recalc comparison, not whole map construction.',
        file_io=file_io), heads, tiles


def recalculate(m, table, heads, tiles, authored):
    """Complete selected Recalc/LAT in the existing raster VM, twice per cell."""
    u = m.u
    cliff = []
    for name in ('RULESMD.INI', 'MPBATTLEMD.INI', 'GOLDST.MMX'):
        sections, _ = lexical(m.assets[name], {'General'})
        m.rules_cache(sections)
        for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, m.rules), (UC_X86_REG_EDI, RULES)):
            u.reg_write(reg, value)
        # Existing shrapnel_repair.retail_inputs Rules reader boundary.
        run_checked(u, 0x66F1CB, 0x66F1EC)
        cliff.append(dict(file=name, value=u.mem_read(m.rules + 0x664, 1)[0]))
    registry = m.alloc(len(tiles) * 4)
    for tile, name in tiles.items():
        if name in heads:
            u.mem_write(registry + tile * 4, dwords(heads[name]['pointer']))
    u.mem_write(0xA8ED2C, dwords(registry))
    u.mem_write(0xA8ED38, dwords(len(tiles)))
    u.mem_write(0x87F7E8 + 0x68, dwords(m.alloc(1000000), 100000, m.alloc(1000000)))
    m.invoke(0x49F2F0, 0)
    rows = []
    for xy in ((106, 34), (106, 35)):
        u.mem_write(table, bytes(0x100000))
        source = authored[xy]
        assert source['tile'] == 512 and source['subtile'] == 0 and source['overlay'] is None
        set_cell(m, table, CELL, dict(coords=list(xy), level=source['level'], slope=0, flags=0, revealed=True))
        u.mem_write(CELL + 0x38, dwords(source['tile']))
        neighbors = []
        lat_neighbors = ((xy[0] - 1, xy[1]), (xy[0] + 1, xy[1]))
        needed = dict.fromkeys((*lat_neighbors, *((xy[0] + dx, xy[1] + dy)
            for dx, dy in ((0, -1), (-1, 0), (2, 2), (1, 1), (-1, 1), (1, -1)))))
        for neighbor in needed:
            raw = authored[neighbor]
            physical = heads[tiles[raw['tile']]] if neighbor in lat_neighbors else None
            if physical is not None:
                assert raw['subtile'] == 0 and physical['slope'] == 0
            set_cell(m, table, m.alloc(0x148), dict(coords=list(neighbor), level=raw['level'],
                slope=physical['slope'] if physical else 0, flags=0, revealed=True))
            neighbors.append(dict(coords=list(neighbor), authored_tile=raw['tile'],
                subtile=raw['subtile'], level=raw['level'], source=tiles[raw['tile']],
                **(dict(slope=physical['slope']) if physical else {})))
        results = []
        for _ in range(2):
            m.invoke(0x47D2B0, CELL, (-1,))
            results.append(dict(tile=m.read32(CELL + 0x38), subtile=u.mem_read(CELL + 0x11A, 1)[0],
                level=u.mem_read(CELL + 0x11B, 1)[0], slope=u.mem_read(CELL + 0x11C, 1)[0], land=m.read32(CELL + 0xEC)))
        assert results[0] == results[1]
        rows.append(dict(coords=list(xy), neighbors=neighbors, output=results[-1]))
    head = heads['rmpfx03.urb']['pointer']
    return dict(entry='0047D2B0', repeats=2, cliff_back_layers=cliff,
        tile_animation=struct.unpack('<i', u.mem_read(head + 0x2C8, 4))[0],
        shadow_caster=u.mem_read(head + 0x2E1, 1)[0], rows=rows,
        boundary='Original full Recalc/LAT on physically loaded pristine TMPs with native theater/Rules reads and source-derived neighbor flat publications. Sparse selected registry/zone storage and two repeated selected calls are supplied; not whole native map initialization/sweeps.')


def prepare():
    m, _, inputs = shared_prepare(placement=True)
    u = m.u
    # Original resource startup publishes PLACE; no decoded host image enters VM.
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x4A885B, 0x4A8873, required_addresses=(0x5B40B0,))
    inputs['place'] = dict(canvas=list(struct.unpack('<HH', u.mem_read(m.read32(0x8A03FC) + 2, 4))),
                          loaded=m.asset_loaded[-1:])
    convert = m.read32(0x87F6C4)
    assert m.read32(convert) == 0x7E5358 and m.read32(convert + 0x16C) == 53
    inputs['convert'] = dict(global_address='0087F6C4', constructor='0048E740',
        source='PALETTE.PAL', destination='TEMPERAT.PAL', shade_count=53,
        table_sha256=sha(bytes(u.mem_read(m.read32(convert + 0x170), 53 * 512))),
        destination_expanded_sha256=sha(bytes(u.mem_read(0x885780, 768))))
    # Actual static foundation tables precede BuildingType full reader.
    m.invoke(0x45B1C0, 0)
    m.rules = m.alloc(0x5000)
    u.mem_write(0x8871E0, dwords(m.rules))
    m.invoke(0x665650, m.rules)
    m.invoke(0x4E7CF0, 0)
    typ = m.alloc(0x2000)
    m.invoke(0x45DD90, typ, (m.cstring('GAPOWR'),))
    fields = {'speed_type': (0x67C, 4), 'naval': (0xCCE, 1),
        'foundation': (0xEF0, 4), 'to_tile': (0xE58, 4),
        'powers_up_building_first_byte': (0xE88, 1), 'place_anywhere': (0x1703, 1),
        'adjacent': (0xEB4, 4), 'to_overlay': (0xE54, 4),
        'gate': (0x16B7, 1), 'laser_fence': (0x16BF, 1),
        'laser_fence_post': (0x16BE, 1)}
    def state():
        return {name: int.from_bytes(u.mem_read(typ + off, size), 'little', signed=size == 4)
                for name, (off, size) in fields.items()}
    inputs['building'] = dict(name='GAPOWR', constructor=state(), layers=[])
    inputs['building']['langrule_absent'] = not (configured_gamemd().parent / 'LANGRULE.INI').exists()
    assert inputs['building']['langrule_absent'], 'Selected corpus requires the physically absent optional LANGRULE layer'
    for name in ('RULESMD.INI', 'MPBattleMD.ini', 'GoldSt.mmx'):
        sections, lines = lexical(m.assets[name.upper()], {'GAPOWR', 'General', 'Clear', 'Road',
            'Rough', 'Rock', 'Tiberium', 'Water', 'Beach', 'Ice', 'Railroad', 'Tunnel', 'Weeds'})
        m.rules_cache(sections)
        result = m.invoke(0x45FE50, typ, (RULES,)) & 255
        m.invoke(0x674000, 0, (RULES,))
        inputs['building']['layers'].append(dict(file=name, admitted=result,
            physical_keys=sections.get('GAPOWR'), source_lines=lines, retained=state()))
    # Original getter runs lazy sentinel initialization. Only CRT registration
    # service is outside the retained gameplay/resource lifetime.
    u.hook_add(UC_HOOK_CODE, lambda _u, pc, _n, _d: m.ret(0) if pc == 0x7C978A else None)
    foundation = m.invoke(0x45EC20, typ, (1,))
    offsets = []
    for i in range(30):
        xy = list(struct.unpack('<hh', u.mem_read(foundation + i * 4, 4)))
        if xy == [32767, 32767]:
            break
        offsets.append(xy)
    else:
        raise AssertionError('unterminated original foundation')
    inputs['building']['offsets'] = offsets
    inputs['building']['dimensions'] = [m.invoke(0x45EC90, typ), m.invoke(0x45ECA0, typ, (0,))]
    # Full postpass, with the actual selected layered keys (none supplies ToTile).
    selected = {}
    for name in ('RULESMD.INI', 'MPBATTLEMD.INI', 'GOLDST.MMX'):
        section, _ = lexical(m.assets[name], {'GAPOWR'})
        selected.update(section.get('GAPOWR', {}))
    m.rules_cache({'GAPOWR': selected})
    u.mem_write(0x887048, dwords(RULES))
    registry = m.alloc(4)
    u.mem_write(registry, dwords(typ))
    u.mem_write(0xA83C68, dwords(0x7EB6D4, registry, 1, 1, 1, 10))
    start = len(m.reads)
    m.invoke(0x465CC0, 0)
    reads = [read for read in m.reads[start:] if read.get('key') == 'ToTile']
    assert len(reads) == 1 and reads[0]['section'] == 'GAPOWR'
    inputs['building']['to_tile_reads'] = reads
    inputs['building']['to_tile_registry_boundary'] = 'Supplied one-entry selected BuildingType registry containing the actually constructed GAPOWR; original complete postpass executes.'
    inputs['building']['to_tile_after_postpass'] = m.read32(typ + 0xE58)
    # Execute the existing original startup table and Map projection prerequisites.
    starts = list(struct.unpack('<13I', u.mem_read(0x8129FC, 52)))
    for entry in (*starts, 0x561710, 0x5617A0, 0x5617C0, 0x5617E0,
                  0x6D1830, 0x6D18C0, 0x6D1BB0):
        m.invoke(entry, 0)
    assert m.read32(0x89E7C0) == m.read32(0xABDE88) == 104
    table = m.alloc(512 * 512 * 4)
    u.mem_write(0x87F924, dwords(table, 512 * 512))
    u.mem_write(0x87F7E8 + 0xF4, dwords(103, 82, 2, 4, 99, 72))
    inputs['map'] = dict(name='GoldSt.mmx', boundary='Physical authored packs; sparse live Cell/visibility priors are supplied, not whole native scenario loading.')
    from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
    parsed, cells = decode_cells(m.assets['GOLDST.MMX'])
    inputs['map']['physical_map_section'] = parsed['Map']
    inputs['map']['cells'] = [dict(coords=list(xy), **cells[xy]) for xy in ((106, 34), (106, 35), (107, 34), (107, 35))]
    inputs['ramp'], heads, tiles = physical_ramp(m)
    inputs['map']['native_recalc'] = recalculate(m, table, heads, tiles, cells)
    return m, typ, foundation, table, inputs


def set_cell(m, table, pointer, case):
    u = m.u
    u.mem_write(pointer, bytes(0x148))
    u.mem_write(pointer, dwords(0x7E4EEC))
    u.mem_write(pointer + 0x24, struct.pack('<hh', *case['coords']))
    u.mem_write(pointer + 0x44, dwords(-1))
    u.mem_write(pointer + 0x11B, bytes((case['level'] & 255, case['slope'])))
    u.mem_write(pointer + 0x12C, dwords(case['flags'] | (8 if case['revealed'] else 0)))
    x, y = case['coords']
    u.mem_write(table + (y * 512 + x) * 4, dwords(pointer))
    u.mem_write(0xABDC50 + 0x12C, dwords(8 if case['revealed'] else 0))


def execute(m, typ, table, case):
    case = dict(coords=[106, 34], level=5, slope=1, flags=2, revealed=True,
        proximity=True, cell_point=[66, 156], clip=[0, 0, WIDTH, HEIGHT],
        viewport_y=0, old_z=65535, background=65535, a=127, repeat=1) | case
    u = m.u
    set_cell(m, table, CELL, case)
    u.mem_write(0x880990, dwords(typ if case['flags'] & 2 else 0))
    u.mem_write(0x88097C, dwords(typ if case['flags'] & 4 else 0))
    u.mem_write(0x880968, bytes((case['proximity'],)))
    u.mem_write(POINT, dwords(*case['cell_point']))
    u.mem_write(CLIP, dwords(*case['clip']))
    u.mem_write(0x886FA4, dwords(case['viewport_y']))
    u.mem_write(PIXELS, struct.pack('<H', case['background']) * COUNT)
    u.mem_write(Z, struct.pack('<H', case['old_z']) * COUNT)
    u.mem_write(A, struct.pack('<H', case['a']) * COUNT)
    draws, reached, queries = [], set(), []
    returns = {}
    def observe(_u, pc, _n, _d):
        if pc in returns:
            kind, arg = returns.pop(pc)
            result = u.reg_read(UC_X86_REG_EAX)
            queries.append(dict(kind=kind, coords=arg,
                result=result & 255 if kind == '00586360' else struct.unpack('<i', dwords(result))[0]))
        if pc in (0x578080, 0x586360):
            ptr = m.read32(u.reg_read(UC_X86_REG_ESP) + 4)
            returns[m.read32(u.reg_read(UC_X86_REG_ESP))] = (f'{pc:08X}', ints(u, ptr, 3))
        if pc in (0x47C620, 0x4AED70, 0x490E50, 0x437A10, 0x4960A0):
            reached.add(f'{pc:08X}')
        if pc == 0x4AED70:
            args = ints(u, u.reg_read(UC_X86_REG_ESP) + 4, 14)
            draws.append(dict(frame=args[1], point=ints(u, args[2], 2), clip=ints(u, args[3], 4),
                flags=args[4], z_adjust=args[6], gradient=args[7], brightness=args[8]))
    hook = u.hook_add(UC_HOOK_CODE, observe)
    admitted = []
    for _ in range(case['repeat']):
        admitted.append(m.invoke(0x47EC90, CELL, (POINT, CLIP, 0)) & 255)
    u.hook_del(hook)
    return dict(name=case.pop('name'), input=case, admitted=admitted, draws=draws,
                queries=queries, reached=sorted(reached), output=snapshot(u, case['background'], case['old_z']))


def foundation_scene(m, typ, foundation, table):
    u = m.u
    cells = [dict(coords=[x, y], level=level, slope=slope, flags=0, revealed=True)
             for x, y, level, slope in ((106, 34, 5, 1), (106, 35, 5, 1),
                                       (107, 34, 6, 0), (107, 35, 6, 0))]
    for cell in cells:
        cell['pointer'] = m.alloc(0x148)
        set_cell(m, table, cell['pointer'], cell)
    u.mem_write(0x88095C, struct.pack('<hh', 106, 34))
    m.invoke(0x4A8BF0, 0x87F7E8, (foundation,))
    tactical = m.alloc(0x1000)
    u.reg_write(UC_X86_REG_ESP, SP); u.reg_write(UC_X86_REG_ESI, tactical); u.reg_write(UC_X86_REG_EBX, 0)
    run_checked(u, *MATRIX_INITIALIZER)
    view = [2064, 1944]
    u.mem_write(tactical + 0xB0, dwords(*view)); u.mem_write(0x887324, dwords(tactical))
    u.mem_write(0x886FA0, dwords(0, 0, WIDTH, HEIGHT))
    mouse = m.alloc(0x100)
    u.mem_write(SP, dwords(0, 0, 0)); u.reg_write(UC_X86_REG_ESP, SP); u.reg_write(UC_X86_REG_ECX, mouse)
    run_checked(u, 0x7B8730, 0x7B8856)
    u.mem_write(0x887640, dwords(mouse)); u.mem_write(mouse + 0x1C, dwords(96, 80))
    u.mem_write(0x880990, dwords(typ)); u.mem_write(0x88097C, dwords(0)); u.mem_write(0x880968, b'\1')
    u.mem_write(PIXELS, struct.pack('<H', 65535) * COUNT); u.mem_write(Z, struct.pack('<H', 65535) * COUNT)
    u.mem_write(A, struct.pack('<H', 127) * COUNT)
    draws, order = [], []
    def observe(_u, pc, _n, _d):
        if pc == 0x47EC90:
            receiver = u.reg_read(UC_X86_REG_ECX)
            cell = next(c for c in cells if c['pointer'] == receiver)
            point = ints(u, m.read32(u.reg_read(UC_X86_REG_ESP) + 4), 2)
            order.append(cell['coords'])
            cell['cell_point'] = point
        if pc == 0x4AED70:
            args = ints(u, u.reg_read(UC_X86_REG_ESP) + 4, 9)
            draws.append(dict(frame=args[1], point=ints(u, args[2], 2), flags=args[4], z_adjust=args[6], brightness=args[8]))
    hook = u.hook_add(UC_HOOK_CODE, observe)
    m.invoke(0x6D5030, tactical, (0,))
    u.hook_del(hook)
    for cell in cells:
        cell['flags'] = m.read32(cell.pop('pointer') + 0x12C) & ~8
    return dict(name='goldst_mixed_foundation', cells=cells, native_order=order,
        input=dict(view_origin=view, background=65535, old_z=65535, a=127, clip=[0, 0, WIDTH, HEIGHT]),
        draws=draws, output=snapshot(u, 65535, 65535))


def centering(m, table):
    """Original fixed-list bounds/offset and selected refinery mark lifetime.

    Runs after the retained raster scene so its allocations/state cannot alter
    that comparison. Only original prerequisite field blocks are enrolled for
    GAREFN: the complete visual reader needs additional Bib resource transport.
    """
    u = m.u
    typ = m.alloc(0x2000)
    m.invoke(0x45DD90, typ, (m.cstring('GAREFN'),))
    def type_state():
        return dict(foundation=m.read32(typ + 0xEF0), adjacent=m.read32(typ + 0xEB4),
                    image=m.string(typ + 0x1F8))
    building = dict(name='GAREFN', constructor=type_state(), layers=[])
    for name in ('RULESMD.INI', 'MPBattleMD.ini', 'GoldSt.mmx'):
        sections, _ = lexical(m.assets[name.upper()], {'GAREFN'})
        m.rules_cache(sections)
        u.mem_write(SP, dwords(0))
        u.mem_write(SP + 0x1B4, dwords(RULES))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBX, typ)
        run_checked(u, 0x5F92F9, 0x5F9340, required_addresses=(0x528A10,))
        for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, typ),
                           (UC_X86_REG_EDI, RULES), (UC_X86_REG_ECX, RULES),
                           (UC_X86_REG_ESI, typ + 0x24)):
            u.reg_write(reg, value)
        run_checked(u, 0x45FFAF, 0x45FFC7, required_addresses=(0x5276D0,))
        building['layers'].append(dict(file=name, physical_keys=sections.get('GAREFN'),
                                       retained=type_state()))
    art, _ = lexical(m.assets['ARTMD.INI'], {'GAREFN'})
    m.make_ini(art)
    for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, typ),
                       (UC_X86_REG_EBX, typ + 0x24), (UC_X86_REG_EDI, typ + 0x1F8),
                       (UC_X86_REG_ESI, INI)):
        u.reg_write(reg, value)
    run_checked(u, 0x461225, 0x46125D, required_addresses=(0x474DA0, 0x528A10))
    building['art_fields'] = {key: art['GAREFN'][key] for key in
        ('Foundation', 'NewTheater', 'BibShape', 'Buildup', 'DemandLoadBuildup')}
    selector = m.read32(typ + 0xEF0)
    building['foundation_selector'] = selector

    def bind(index):
        # Supplied selector prior; actual native table binding/getter executes.
        u.mem_write(typ + 0xEF0, dwords(index))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, typ)
        run_checked(u, 0x46152C, 0x461547)
        return m.invoke(0x45EC20, typ, (1,))

    foundation = bind(selector)
    offsets = []
    for index in range(120):
        xy = list(struct.unpack('<hh', u.mem_read(foundation + index * 4, 4)))
        if xy == [32767, 32767]:
            break
        offsets.append(xy)
    else:
        raise AssertionError('unterminated original occupied foundation')
    building['offsets'] = offsets
    building['dimensions'] = [m.invoke(0x45EC90, typ), m.invoke(0x45ECA0, typ, (0,))]
    shell = m.alloc(0x1000)
    u.mem_write(shell, dwords(0x7E3EBC))
    u.mem_write(shell + 0x520, dwords(typ))
    building['object_footprint_matches_type'] = m.invoke(0x5F5B90, shell, (1,)) == foundation
    assert building['object_footprint_matches_type']
    # Retire the old GAPOWR scene before supplying a fresh sparse marker map.
    m.invoke(0x4A8BF0, 0x87F7E8, (0,))
    u.mem_write(table, bytes(512 * 512 * 4))
    cells = {}
    for x in range(104, 119):
        for y in range(27, 42):
            pointer = m.alloc(0x148)
            cells[x, y] = pointer
            set_cell(m, table, pointer, dict(coords=[x, y], level=0, slope=0,
                                            flags=0, revealed=True))
    calls = []
    def observe(_u, pc, _n, _d):
        if pc == 0x4A95A0:
            sp = u.reg_read(UC_X86_REG_ESP)
            calls.append(dict(origin=list(struct.unpack('<hh', u.mem_read(m.read32(sp + 4), 4))),
                              mark=bool(m.read32(sp + 8))))
    hook = u.hook_add(UC_HOOK_CODE, observe)
    def state():
        return dict(center=list(struct.unpack('<hh', u.mem_read(0x88095C, 4))),
            offset=list(struct.unpack('<hh', u.mem_read(0x880960, 4))),
            marked=[list(xy) for xy, pointer in cells.items() if m.read32(pointer + 0x12C) & 2])
    controls = []
    bounds = m.alloc(4)
    for ordinal in range(22):
        name = m.string(m.read32(0x81B9D8 + ordinal * 8))
        index = m.read32(0x81B9DC + ordinal * 8)
        current = bind(index)
        m.invoke(0x4A94F0, 0x87F7E8, (bounds, current))
        m.invoke(0x4A8BF0, 0x87F7E8, (0,))
        u.mem_write(0x88095C, struct.pack('<hh', 110, 33))
        calls.clear()
        m.invoke(0x4A8BF0, 0x87F7E8, (current,))
        assert len(calls) == 1 and calls[0]['mark']
        controls.append(dict(input=dict(selector=index, name=name, center=[110, 33]),
            output=dict(bounds=list(struct.unpack('<hh', u.mem_read(bounds, 4))),
                        offset=state()['offset'], marker_origin=calls[0]['origin'])))
    m.invoke(0x4A8BF0, 0x87F7E8, (0,))
    foundation = bind(selector)
    edge_controls = []
    for center in ([0, 0], [1, 1], [32767, 32767], [32768, 32768], [65535, 65535]):
        u.mem_write(0x88095C, struct.pack('<HH', *center))
        calls.clear()
        m.invoke(0x4A8BF0, 0x87F7E8, (foundation,))
        assert len(calls) == 1 and calls[0]['mark']
        edge_controls.append(dict(input=dict(selector=selector, center=center),
                                  output=dict(marker_origin=calls[0]['origin'])))
        m.invoke(0x4A8BF0, 0x87F7E8, (0,))
    u.mem_write(0x88095C, struct.pack('<hh', 110, 33))
    calls.clear()
    m.invoke(0x4A8BF0, 0x87F7E8, (foundation,))
    steps = [dict(action='activate', marker_calls=list(calls), output=state())]
    coord, old = m.alloc(4), m.alloc(4)
    u.mem_write(coord, struct.pack('<hh', 111, 34))
    calls.clear()
    u.mem_write(SP, dwords(0, old, coord))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, 0x87F7E8)
    # Full supplied-coordinate center/mark prefix, stopping before the
    # independent proximity/whole-foundation visibility publications.
    run_checked(u, 0x4A91B0, 0x4A943A, required_addresses=(0x6DA360, 0x4A95A0))
    steps.append(dict(action='move', marker_calls=list(calls), output=state()))
    calls.clear()
    m.invoke(0x4A8BF0, 0x87F7E8, (0,))
    steps.append(dict(action='clear', marker_calls=list(calls), output=state()))
    u.hook_del(hook)
    assert len(steps[0]['output']['marked']) == len(steps[1]['output']['marked']) == 12
    assert not steps[2]['output']['marked']
    return dict(building=building, controls=controls, edge_controls=edge_controls,
        lifecycle=dict(input=dict(initial_center=[110, 33], moved_center=[111, 34]), steps=steps),
        boundary='Original GAREFN constructor and layered Image/Adjacent plus fixed ART Foundation blocks, native occupied-table binding/getters and ObjectGetFootprint on a supplied Building shell. All22 selector priors are supplied numeric controls. Sparse level0/revealed Cell shells and cursor centers are supplied. Move stops after center/mark publication at4A943A, before proximity/visibility. No whole ReadINI/Bib resource, paid GAREFN/ManualPlace, inverse, event or map bootstrap claim.')


def generate():
    m, typ, foundation, table, inputs = prepare()
    cases = [dict(name='active_slope1'), dict(name='active_flat', slope=0),
        dict(name='pending_slope1', flags=4), dict(name='no_marker', flags=0),
        dict(name='outside_proximity', slope=0, proximity=False),
        dict(name='hidden', revealed=False), dict(name='a_zero', a=0),
        dict(name='old_z_zero', old_z=0), dict(name='old_z_mid', old_z=32768),
        dict(name='clipped', clip=[0, 0, 100, 88]), dict(name='repeat', repeat=2)]
    cases += [dict(name=f'level_{level}', level=level, cell_point=point)
              for level, point in ((0, [66, 81]), (6, [66, 171]), (-128, [66, -1839]))]
    rows = [execute(m, typ, table, case) for case in cases]
    scene = foundation_scene(m, typ, foundation, table)
    return dict(inputs=inputs,
        surface=dict(width=WIDTH, height=HEIGHT, bytes_per_pixel=2), rows=rows,
        scenes=[scene], centering=centering(m, table))


def metadata():
    return provenance(scope='Original ordinary GAPOWR Cell placement marker/full bounded foundation caller on physical PLACE.SHP, plus GAREFN prerequisite readers and native fixed-foundation centering/mark lifetime; original53-row plain Convert and opaque no-Z/A raster. No native whole scenario or ready-factory lifecycle execution.',
        assumptions=[
            'Shared physical archive/lexical/PaletteReader/surface/raster owners. Original TEMPERAT load52BC49..52BCBB precedes original PALETTE Convert constructor/publication52BE61..52BFCE, matching native Init_Game order. No palette/shape pixels are reconstructed by Python.',
            'Actual BuildingType45DD90 constructor, full45FE50 RULESMD/MPBattleMD/GoldSt reads, ART Foundation, land rules674000, static foundation45B1C0, getter45EC20 and full ToTile postpass465CC0 execute. LANGRULE is absent in the selected installation. Physical Rmpfx03.urb and its selected W/E neighbor files execute the original full lazy TMP loaders/getters. Full47D2B0/47CA80 executes twice per selected cell with actual native theater projection and Rules CliffBack reads; sparse selected type/zone storage and neighbor publications are supplied. Whole registry/map loading remains excluded.',
            'Sparse authored GoldSt coordinates and supplied live levels/slopes/empty objects/visibility/proximity are component priors. Original578080 ground and586360 visibility execute; all-visible Dummy represents supplied revealed surrounding cells. Signed-128, pending and surface controls are boundaries, not natural stock producers.',
            'Original47EC90/47C620/4AED70/490E50/437A10/4960A0 executes without behavior replacements.20600 uses plain palette translation and ignores both A and Z; output runs include retained Z. Foundation scene executes6D5030, actual matrix/projection/mouse queries and mark producer4A8BF0; its X-outer rectangular order is independent of the row-major physical occupied list.',
            'Additive centering controls execute original GAREFN45DD90, layered Image5F92F9..5F9340 and Adjacent45FFAF..45FFC7, fixed ART Foundation461225..46125D, occupied-table binding46152C..461547, getters and ObjectGetFootprint5F5B90 on a supplied Building shell. Whole GAREFN45FE50/visual Bib loading is excluded; complete reader/visual-loader instructions retain the selected foundation/occupied-pointer/Adjacent fields after these blocks. All22 fixed selector values and sparse revealed level0 Cell shells are supplied numeric priors. Original4A94F0/4A8BF0/4A95A0 capture bounds, signed division and marker origins; move4A91B0 stops at4A943A after mark/center publication, before proximity/visibility. No event or ManualPlace execution is added.',
        ], substitutions=['Existing allocation/archive/TLS services; CRT atexit registration is a no-op service. Sparse map storage and initialized surface priors supplied. No placement/admission/projection/raster body is replaced.'],
        entry_points=dict(cell=0x47EC90, admission=0x47C620, foundation=0x6D5030,
            marker_startup=0x4A885B, convert=0x48E740, shape=0x4AED70,
            rowwalker=0x437A10, leaf=0x4960A0, ground=0x578080, visibility=0x586360,
            foundation_bounds=0x4A94F0, set_active=0x4A8BF0, mark=0x4A95A0,
            update_center_prefix=0x4A91B0, update_center_stop=0x4A943A))
