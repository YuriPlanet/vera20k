"""Additive ore radar mode of the existing Cell/SHP VM and resource owners.

Original655250/6547C0 write every reference pixel. No sampler, decoder, palette,
VM, native relocation or reduction implementation is duplicated here.
"""
import hashlib
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import RET_MAGIC, run_checked
from tools.spatial_oracle.bridge_shadow_render import CELL, ints, physical_assets
from tools.spatial_oracle.building_body_rules import INI, RULES, SP, dwords
from tools.spatial_oracle.tmp_clear_render import load_tmp_files, theater_tile_files
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells

MAP = 0x87F7E8


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def surface_output(m, radar):
    u = m.u
    surface = m.read32(radar + 0x1220)
    width, height = ints(u, surface + 4, 2)
    raw = bytes(u.mem_read(m.read32(surface + 0x14), width * height * 2))
    words = struct.unpack('<' + 'H' * (width * height), raw)
    runs = []
    for y in range(height):
        x = 0
        while x < width:
            color = words[y * width + x]
            if not color:
                x += 1
                continue
            first = x
            while x < width and words[y * width + x] == color:
                x += 1
            runs.append([first, y, x - first, color])
    return dict(generated_size=[width, height], rgb565_sha256=sha(raw), runs=runs)


def weighted_controls(m):
    controls = []
    for width, height in ((173, 113), (137, 149)):
        radar = m.alloc(0x1600)
        # This deterministic input is the existing terrain_tests fixture,
        # explicitly supplied, never an expected output or a sample formula.
        raw = bytes(value & 255 for y in range(height) for x in range(width)
                    for value in (x * 17 + y * 3, x * 5 + y * 29, x * 41 + y * 7))
        buffer = m.alloc(len(raw))
        m.u.mem_write(buffer, raw)
        m.u.mem_write(radar + 0x123C, dwords(buffer, width, height))
        bounds, result = m.alloc(16), m.alloc(16)
        m.u.mem_write(bounds, dwords(0, 0, width, height))
        m.u.mem_write(SP, dwords(RET_MAGIC, result, bounds, bounds, False))
        m.u.reg_write(UC_X86_REG_ESP, SP)
        m.u.reg_write(UC_X86_REG_ECX, radar)
        run_checked(m.u, 0x6547C0, RET_MAGIC, count=100000000, timeout_us=120000000,
                    required_addresses=(0x654DBC,))
        output = surface_output(m, radar)
        # Full random-pattern runs would be bulk; one hash consumes all stores.
        del output['runs']
        output['native_rect'] = ints(m.u, result, 4)
        controls.append(dict(input=dict(raw_size=[width, height], pattern='terrain_tests_coordinate_v1'),
                             output=output))
    return controls


def prepare_terrain(m):
    u = m.u
    assets, sources = physical_assets(radar=True)
    m.assets.update(assets)
    tiles, mapping = theater_tile_files(m, assets['URBANMD.INI'], 141, 'urb')
    assert tiles[141] == 'clat10.urb'
    m.phase = 'ore_radar_tmp_loading'
    heads, file_io = load_tmp_files(m, {'clat10.urb': assets['CLAT10.URB']})
    head = heads['clat10.urb']
    registry = m.alloc(len(tiles) * 4)
    u.mem_write(registry + 141 * 4, dwords(head))
    u.mem_write(0xA8ED2C, dwords(registry))
    u.mem_write(0xA8ED38, dwords(len(tiles)))
    # Reuse the existing original Rules constructor/selected CliffBack and land
    # readers, rather than supplying a favorable post-Recalc land scalar.
    m.rules = m.alloc(0x5000)
    u.mem_write(0x8871E0, dwords(m.rules))
    m.invoke(0x665650, m.rules)
    layers = []
    land_sections = {'General', 'Clear', 'Road', 'Rough', 'Rock', 'Tiberium',
                     'Water', 'Beach', 'Ice', 'Railroad', 'Tunnel', 'Weeds'}
    for name in ('RULESMD.INI', 'MPBATTLEMD.INI', 'GOLDST.MMX'):
        sections, _ = lexical(m.assets[name], land_sections)
        m.rules_cache(sections)
        for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, m.rules), (UC_X86_REG_EDI, RULES)):
            u.reg_write(reg, value)
        run_checked(u, 0x66F1CB, 0x66F1EC)
        m.invoke(0x674000, 0, (RULES,))
        layers.append(dict(file=name, cliff_back=u.mem_read(m.rules + 0x664, 1)[0]))
    parsed, authored = decode_cells(m.assets['GOLDST.MMX'])
    source = authored[123, 33]
    assert (source['tile'], source['subtile'], source['level'], source['overlay']) == (141, 0, 5, 102)
    table = m.read32(MAP + 0x13C)
    u.mem_write(table, bytes(0x100000))
    u.mem_write(table + (33 * 512 + 123) * 4, dwords(CELL))
    u.mem_write(CELL + 0x38, dwords(source['tile']))
    u.mem_write(CELL + 0x44, dwords(102))
    u.mem_write(CELL + 0xE4, dwords(0))
    u.mem_write(CELL + 0x11A, bytes((source['subtile'], source['level'], 0, 0, 1)))
    u.mem_write(CELL + 0x140, dwords(0))
    needed = dict.fromkeys(((123, 32), (124, 33), (123, 34), (122, 33),
                          (125, 35), (124, 34), (122, 34), (124, 32)))
    neighbors = []
    for xy in needed:
        raw = authored[xy]
        pointer = m.alloc(0x148)
        u.mem_write(pointer, dwords(0x7E4EEC))
        u.mem_write(pointer + 0x24, struct.pack('<hh', *xy))
        u.mem_write(pointer + 0x38, dwords(raw['tile']))
        # Overlay-empty live neighbors isolate the radar consumer, not native
        # growth reseeding on the physical GoldSt ore patch.
        u.mem_write(pointer + 0x44, dwords(-1))
        u.mem_write(pointer + 0x11A, bytes((raw['subtile'], raw['level'], 0)))
        u.mem_write(table + (xy[1] * 512 + xy[0]) * 4, dwords(pointer))
        neighbors.append(dict(coords=list(xy), tile=raw['tile'], subtile=raw['subtile'],
            level=raw['level'], authored_overlay=raw['overlay'], authored_data=raw['frame'],
            supplied_overlay=-1))
    u.mem_write(MAP + 0xF4, dwords(103, 82, 2, 4, 99, 72))
    scenario = m.read32(0xA8B230)
    u.mem_write(scenario + 0x1258, dwords(2))
    image = m.read32(head + 0xA4)
    subimage = m.read32(image + 0x10)
    return dict(sources=sources, mapping=mapping, file_io=file_io,
        map_section=parsed['Map'], coords=[123, 33], tile=141, subtile=0, level=5,
        native_tmp_slope=m.invoke(0x5471B0, head, (0,)) & 255,
        native_tmp_land=m.invoke(0x544BE0, head, (0,)),
        radar_metadata=list(u.mem_read(subimage + 0x2B, 6)),
        theater_index=2, brightness_bits=bytes(u.mem_read(0x7E1BD0 + 2 * 0x70, 4)).hex(),
        cliff_back_layers=layers, neighbors=neighbors,
        boundary='Original Urban General/ordinal projection and TMP lazy IO; selected141 registration and prefix sum supplied. Physical neighbor tile/levels, but empty-overlay live priors and unused slope0 fields, not a natural GoldSt ore-neighbor/queue or whole map bootstrap claim.')


def prepare_dirty_tracking(m, width, height, column, row, *, capacity=16):
    """Existing supplied radar backing/dirty storage, actual native writers."""
    u = m.u
    buffer, dirty = m.alloc(width * height * 3), m.alloc(capacity * 4)
    u.mem_write(MAP + 0x1220, dwords(0))
    u.mem_write(MAP + 0x1224, dwords(0x7EB6D4, dirty, capacity, 1, 0, 10))
    u.mem_write(MAP + 0x123C, dwords(buffer, width, height))
    u.mem_write(MAP + 0x1274, dwords(0))
    u.mem_write(MAP + 0x1490, dwords(column))
    u.mem_write(MAP + 0x1498, dwords(row))
    return dict(width=width,height=height,column=column,row=row,capacity=capacity)


def depletion(m, types):
    u = m.u
    inputs = prepare_terrain(m)
    # Explicit component radar geometry, independent of whole-map bounds boot.
    width, height, column, row = 173, 113, 10, 100
    prepare_dirty_tracking(m,width,height,column,row)
    buffer = m.read32(MAP + 0x123C)
    left, right = m.alloc(4), m.alloc(4)
    bounds, result = m.alloc(16), m.alloc(16)
    u.mem_write(bounds, dwords(0, 0, width, height))
    rng_address = m.read32(0xA8B230) + 0x218
    rng_before = bytes(u.mem_read(rng_address, 0x3F4))
    events = []
    def observe(_u, pc, _n, _d):
        if pc in (0x480A80, 0x6551C0, 0x655250, 0x47C060, 0x6547C0, 0x47D2B0, 0x47CA80, 0x65C780):
            events.append(f'{pc:08X}')
        elif pc == 0x6D2790:
            m.ret(0, 0x14)
    hook = u.hook_add(UC_HOOK_CODE, observe)
    def capture(name, removed=None, first=None):
        first = len(events) if first is None else first
        queued = m.read32(MAP + 0x1234)
        m.invoke(0x47C060, CELL, (left, right))
        color = [list(u.mem_read(pointer, 3)) for pointer in (left, right)]
        m.invoke(0x655250, MAP)
        raw = bytes(u.mem_read(buffer, width * height * 3))
        changed = [[index % width, index // width, *raw[index * 3:index * 3 + 3]]
                   for index in range(width * height) if any(raw[index * 3:index * 3 + 3])]
        state = dict(overlay_id=ints(u, CELL + 0x44, 1)[0], data=u.mem_read(CELL + 0x11E, 1)[0],
            tile=m.read32(CELL + 0x38), subtile=u.mem_read(CELL + 0x11A, 1)[0],
            level=u.mem_read(CELL + 0x11B, 1)[0], slope=u.mem_read(CELL + 0x11C, 1)[0],
            land=m.read32(CELL + 0xEC))
        output = surface_output(m, MAP)
        # Compare Rust's full-rebuild strategy with the actual original full
        # raster, then restore the incremental history for the next reduction.
        surface = m.read32(MAP + 0x1220)
        header = bytes(u.mem_read(surface, 32))
        backing = m.read32(surface + 0x14)
        pixels = bytes(u.mem_read(backing, output['generated_size'][0] * output['generated_size'][1] * 2))
        radar_state = bytes(u.mem_read(MAP + 0x121C, 0x1278 - 0x121C))
        history_events = list(events[first:])
        u.mem_write(SP, dwords(RET_MAGIC, result, bounds, bounds, False))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, MAP)
        run_checked(u, 0x6547C0, RET_MAGIC, count=100000000, timeout_us=120000000,
                    required_addresses=(0x654DBC,))
        rebuild_output = surface_output(m, MAP)
        assert bytes(u.mem_read(buffer, width * height * 3)) == raw
        assert bytes(u.mem_read(surface, 32)) == header
        assert bytes(u.mem_read(MAP + 0x121C, len(radar_state))) == radar_state
        u.mem_write(backing, pixels)
        del events[first + len(history_events):]
        return dict(name=name, removed=removed, state=state, color_pair=color,
            queue_before=queued, queue_after=m.read32(MAP + 0x1234),
            raw_pixels=changed, raw_rgb_sha256=sha(raw), dirty_rect=ints(u, MAP + 0x1248, 4),
            output=output, rebuild_output=rebuild_output, events=history_events)
    # The initial ore Cell fields also come from its original Recalc, rather
    # than the old sentinel fixture's retained land or a host scalar.
    first = len(events)
    m.invoke(0x47D2B0, CELL, (-1,))
    m.invoke(0x6551C0, MAP, (CELL + 0x24,))
    rows = [capture('density_1', first=first)]
    for name in ('density_0', 'empty'):
        first = len(events)
        removed = m.invoke(0x480A80, CELL, (1,))
        rows.append(capture(name, removed, first))
    rows.append(capture('unchanged_empty'))
    u.hook_del(hook)
    assert rows[0]['state']['data'] == 1 and rows[1]['state']['data'] == 0
    assert rows[1]['state']['overlay_id'] == 102 and rows[1]['queue_before'] == 0
    assert rows[2]['state']['overlay_id'] == -1 and rows[2]['queue_before'] == 1
    assert rows[2]['state']['tile'] == 141 and rows[2]['state']['slope'] == 0
    assert all(row['queue_after'] == 0 for row in rows)
    assert bytes(u.mem_read(rng_address, 0x3F4)) == rng_before
    assert '0065C780' not in events
    return dict(input=dict(raw_size=[width, height], column_base=column, row_base=row,
                           background_rgb=[0, 0, 0]), terrain=inputs,
        overlay=dict(id=102, tiberium=u.mem_read(types['TIB01'] + 0x2A9, 1)[0],
                     cell_anim=m.read32(types['TIB01'] + 0x29C)), rows=rows,
        rng_before_sha256=sha(rng_before), rng_after_sha256=sha(bytes(u.mem_read(rng_address, 0x3F4))),
        boundary='Full480A80/47D2B0/LAT/6551C0/655250/47C060/6547C0 in the shared ore VM. Sparse Cell/TMP registry, empty-overlay neighbors and raw radar geometry supplied; only Tactical dirtiness remains a sink. No online-radar/viewer, normal harvester or whole-map bounds/bootstrap claim.')


def generate(m, types):
    # Historical ore RGB/lifecycle results have already been captured.
    lifecycle = depletion(m, types)
    return dict(fpcw=m.u.reg_read(UC_X86_REG_FPCW), depletion=lifecycle,
                weighted_controls=weighted_controls(m))
