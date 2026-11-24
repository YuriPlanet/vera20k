"""Physical CRATER11 through original current-Cell/type/SHP drawing.

This mode composes the existing native drawing VM, field readers, projection,
CellConvert, SHP and A/Z surface owners. No host drawing or palette math lives
here. The fatal producer has a separate already-consumed lifecycle comparison.
"""
from pathlib import Path
import hashlib
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP
from tools import native_oracle as native
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.projectile_oracle.bridge_render_shape import prepare_surface
from tools.spatial_oracle import tmp_clear_render as tile
from tools.spatial_oracle.bridge_shadow_render import (
    physical_assets, ints, snapshot, CELL, POINT, CLIP, WIDTH, HEIGHT, COUNT,
    PIXELS, Z, A,
)
from tools.spatial_oracle.building_body_rules import SP, RULES, dwords
from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
from tools.spatial_oracle.smudge_can_place import Cells
from tools.spatial_oracle.bridge_recalc_presentation import LOAD_MAIN
from tools.rmg_oracle.gen_rng_vectors import STRUCT_LEN

ORIGIN = (121, 43)
FOOTPRINT = ((0, 0), (1, 0), (0, 1), (1, 1))


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def type_state(m, pointer):
    return dict(index=m.read32(pointer + 0x294), width=m.read32(pointer + 0x298),
        height=m.read32(pointer + 0x29C), crater=m.u.mem_read(pointer + 0x2A0, 1)[0],
        burn=m.u.mem_read(pointer + 0x2A1, 1)[0], theater=m.u.mem_read(pointer + 0x22C, 1)[0],
        image=m.string(pointer + 0x1F8), image_present=bool(m.read32(pointer + 0xA4)))


def prepare():
    m, inputs, files = tile.prepare()
    assets, sources = physical_assets(smudge=True)
    m.assets.update(assets)
    declarations, registry_lines = lexical(assets['RULESMD.INI'], {'SmudgeTypes'})
    names = list(declarations['SmudgeTypes'].values())
    # Original registry initialization, stopping only before CRT atexit callback
    # registration. Physical declaration-order names cross this fixture boundary;
    # the existing stock fatal producer executes the actual registry INI loop.
    m.u.reg_write(UC_X86_REG_ESP, SP)
    native.run_checked(m.u, 0x4E7260, 0x4E7296)
    selected = None
    for name in names:
        pointer = m.alloc(0x300)
        m.invoke(0x6B5260, pointer, (m.cstring(name),))
        if name == 'CRATER11':
            selected = pointer
    assert selected is not None
    defaults = type_state(m, selected)
    art, art_lines = lexical(assets['ARTMD.INI'], {'CRATER11'})
    m.make_ini(art)
    layers = []
    for filename, raw in (('RULESMD.INI', assets['RULESMD.INI']),
                          ('MPBattleMD.ini', assets['MPBATTLEMD.INI']),
                          ('XMP03T4.MAP', files['XMP03T4.MAP'])):
        fields, lines = lexical(raw, {'CRATER11'})
        m.rules_cache(fields)
        loaded = len(m.asset_loaded)
        admitted = m.invoke(0x6B56D0, selected, (RULES,)) & 255
        layers.append(dict(file=filename, sha256=sha(raw), input_sections=fields,
            source_lines=lines, admitted=admitted, state=type_state(m, selected),
            assets=m.asset_loaded[loaded:]))
    assert m.read32(selected + 0x294) == names.index('CRATER11') == 44
    assert m.read32(selected + 0x298) == m.read32(selected + 0x29C) == 2
    assert m.read32(selected + 0xA4)
    parsed, authored = decode_cells(files['XMP03T4.MAP'])
    coords = [(ORIGIN[0] + dx, ORIGIN[1] + dy) for dx, dy in FOOTPRINT]
    assert all(authored[xy]['tile'] == 0 and authored[xy]['subtile'] == 0
               and authored[xy]['level'] == 4 and authored[xy]['overlay'] is None
               for xy in coords)
    inputs['sources'] += sources
    inputs['smudge_type'] = dict(name='CRATER11', registry_names=names,
        registry_source_lines=registry_lines, constructor='006B5260',
        reader='006B56D0', defaults=defaults, art_sections=art,
        art_source_lines=art_lines, layers=layers, langrule_absent=True,
        registry_boundary='Physical declaration-order ctor transport; full active registry reader independently consumed in building_death_anims_joined_stock_smudges. No original enumeration-key instrumentation is replaced here.')
    inputs['map'] = dict(name='XMP03T4.MAP', physical_map_section=parsed['Map'],
        cells=[dict(coords=list(xy), **authored[xy]) for xy in coords],
        boundary='Authored clear/no-overlay level4 tuples, original Cell constructors/TMP getters and sparse registration; no full native map/Recalc/LAT or natural shot bootstrap.')
    inputs['profiles'] = dict(default=tile.profile(m), XMP03T4=tile.profile(m, files['XMP03T4.MAP']),
                              Hills=tile.profile(m, assets['HILLS.MAP']))
    return m, selected, inputs, files


def cell_state(m, pointer):
    convert = m.read32(pointer + 0x34)
    return dict(coords=list(struct.unpack('<hh', m.u.mem_read(pointer + 0x24, 4))),
        level=m.u.mem_read(pointer + 0x11B, 1)[0], slope=m.u.mem_read(pointer + 0x11C, 1)[0],
        type_index=struct.unpack('<i', m.u.mem_read(pointer + 0x48, 4))[0],
        piece=m.u.mem_read(pointer + 0x11F, 1)[0],
        top_scalar=struct.unpack('<h', m.u.mem_read(pointer + 0x10A, 2))[0],
        common_scalar=struct.unpack('<h', m.u.mem_read(pointer + 0x10C, 2))[0],
        bottom_scalar=struct.unpack('<h', m.u.mem_read(pointer + 0x10E, 2))[0],
        convert_rgb_key=ints(m.u, convert + 0x198, 3) if convert else None,
        cell_normalized_rgb=list(struct.unpack('<3h', m.u.mem_read(pointer + 0x110, 6))),
        shade_count=m.read32(convert + 0x16C) if convert else None)


def enroll(m, typ, origin, level):
    cells = {}
    head = m.read32(m.read32(0xA8ED2C))
    for dx, dy in FOOTPRINT:
        xy = (origin[0] + dx, origin[1] + dy)
        pointer = m.alloc(0x148)
        m.invoke(0x47BBF0, pointer)
        pair = m.alloc(4)
        m.u.mem_write(pair, struct.pack('<hh', *xy))
        m.invoke(0x485240, pointer, (pair,))
        # Source tuples are either physical authored clear tiles or declared
        # flat component controls. Actual TMP getters establish slope/land.
        slope = m.invoke(0x5471B0, head, (0,)) & 255
        land = m.invoke(0x544BE0, head, (0,))
        m.u.mem_write(pointer + 0x38, dwords(0))
        m.u.mem_write(pointer + 0x11A, bytes((0, level & 255, slope, 0)))
        m.u.mem_write(pointer + 0xEC, dwords(land))
        cells[xy] = pointer
    before = [cell_state(m, pointer) for pointer in cells.values()]
    # Reuse the existing smudge fixture's redraw notification sink. Original
    # Place owns the real type/data stores; dirty publication is not certified.
    redraw = object.__new__(Cells)
    redraw.machine, redraw.redraws = m, []
    def transport(u, pc, _size, _data):
        if tile.cell_transport(m, pc, cells, admission=False):
            return
        if pc == 0x486E70:
            redraw.hook(u, pc)
    hook = m.u.hook_add(UC_HOOK_CODE, transport)
    m.u.mem_write(POINT, struct.pack('<hh', *origin))
    m.invoke(0x6B6080, typ, (POINT,))
    m.u.hook_del(hook)
    return cells, dict(before=before, after=[cell_state(m, pointer) for pointer in cells.values()],
                       redraws=redraw.redraws, entry='006B6080', redraw_boundary='existing smudge_can_place.Cells notification sink486E70')


def draw(m, typ, case, *, alpha=None):
    case = dict(coords=list(ORIGIN), level=4, viewport_y=0, clip=[0, 0, WIDTH, HEIGHT],
                background=65535, old_z=2000, repeat=1, a=127, detail=2,
                full_view=dict(view_origin=[2244, 2319], redraw_rect=[0, 0, WIDTH, HEIGHT])) | case
    cells, placement = enroll(m, typ, tuple(case['coords']), case['level'])
    if alpha is not None:
        case.pop('a', None)
        case['a_hex'] = alpha.hex()
    draws, cell_calls, leaf_calls, reached = [], [], [], set()
    active = None
    def observe(u, pc, _size, _data):
        nonlocal active
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x6B55F0,0x4AED70,0x490E50,0x437A10,0x4978C0):
            reached.add(pc)
        if pc == 0x480350:
            active = u.reg_read(UC_X86_REG_ECX)
            cell_calls.append(dict(coords=cell_state(m, active)['coords'],
                                   point=ints(u, m.read32(sp + 4), 2)))
        elif pc == 0x6B55F0:
            args = ints(u, sp + 4, 5)
            draws.append(dict(coords=list(struct.unpack('<hh', u.mem_read(args[4], 4))),
                point=ints(u, args[0], 2), clip=ints(u, args[1], 4), piece=args[2],
                level_arg=args[3], cell=cell_state(m, active)))
        elif pc == 0x4AED70:
            args = ints(u, sp + 4, 14)
            draws[-1]['shape'] = dict(frame=args[1], point=ints(u, args[2], 2),
                clip=ints(u, args[3], 4), flags=args[4], gradient=args[5],
                z_adjust=args[6], extra=args[7:], shade_count=m.read32(u.reg_read(UC_X86_REG_EDX)+0x16C))
        elif pc == 0x4978C0:
            leaf_calls.append(dict(entry='004978C0', width=m.read32(sp+12)))
    hook = m.u.hook_add(UC_HOOK_CODE, observe)
    main_before = bytes(m.u.mem_read(LOAD_MAIN, STRUCT_LEN))
    pipeline = tile.execute(m, case, current_cell=cells[tuple(case['coords'])], current_cells=cells)
    m.u.hook_del(hook)
    assert len(draws) == 4 * case['repeat'] and leaf_calls
    assert reached == {0x6B55F0,0x4AED70,0x490E50,0x437A10,0x4978C0}
    assert all(row['shape']['flags'] == 0xE00 for row in draws)
    assert bytes(m.u.mem_read(LOAD_MAIN, STRUCT_LEN)) == main_before
    # Original Cell calls above establish exact type arguments/order/material.
    # Isolate this selected layer by replaying those captured arguments through
    # the full original type/SHP body on the same VM/surface; no host pixels.
    state = prepare_surface(m.u, dict(background=case['background'],old_z=case['old_z'],a_value=case.get('a',127)), size=(WIDTH,HEIGHT))
    if alpha is not None:
        m.u.mem_write(A, b''.join(struct.pack('<H', value) for value in alpha))
    alpha_before = bytes(m.u.mem_read(A, COUNT * 2))
    prefix = []
    hook = m.u.hook_add(UC_HOOK_CODE, lambda u, pc, n, d: tile.cell_transport(m, pc, cells, admission=False))
    for row in draws:
        xy = tuple(row['coords'])
        m.u.mem_write(POINT, dwords(*row['point']))
        m.u.mem_write(CLIP, dwords(*row['clip']))
        m.invoke(0x6B55F0, typ, (POINT, CLIP, row['piece'], row['level_arg'], cells[xy]+0x24))
        prefix.append(sha(bytes(m.u.mem_read(PIXELS, COUNT * 2))))
    m.u.hook_del(hook)
    assert bytes(m.u.mem_read(Z, COUNT * 2)) == state['depths']
    assert bytes(m.u.mem_read(A, COUNT * 2)) == alpha_before
    output = snapshot(m.u, case['background'], case['old_z'])
    primary = next(row for row in cell_calls if row['coords'] == case['coords'])
    case['cell_point'] = primary['point']
    return dict(name=case['name'], input=case, placement=placement, cell_calls=cell_calls,
        cells=[cell_state(m, pointer) for pointer in cells.values()], draws=draws,
        reached=sorted(set(pipeline['reached']) | {f'{pc:08X}' for pc in reached}),
        full_cell_boundary='Full original caller/terrain/type raster executes first. Output below isolates the smudge layer by replaying its captured arguments on the same native VM; it is not a whole-frame golden.',
        output=dict(output, prefix_color_sha256=prefix,
                    alpha_sha256=sha(bytes(value[0] for value in struct.iter_unpack('<H',alpha_before))), z_unchanged=True,
                    main_rng_unchanged=True))


def generate():
    m, typ, inputs, files = prepare()
    retail = inputs['profiles']['XMP03T4']
    rows = [draw(m, typ, dict(name='fatal_footprint_flat0', coords=[10,10],level=0,
                full_view=dict(view_origin=[-96,219],redraw_rect=[0,0,WIDTH,HEIGHT]),
                scene_lighting=inputs['profiles']['Hills'])),
            draw(m, typ, dict(name='retail_xmp_clear', scene_lighting=retail)),
            draw(m, typ, dict(name='retail_a63', a=63, scene_lighting=retail)),
            draw(m, typ, dict(name='retail_a0', a=0, scene_lighting=retail)),
            draw(m, typ, dict(name='retail_near_z', old_z=0, scene_lighting=retail)),
            draw(m, typ, dict(name='retail_repeat', repeat=2, scene_lighting=retail)),
            draw(m, typ, dict(name='retail_clipped', clip=[70,100,25,12], scene_lighting=retail)),
            draw(m, typ, dict(name='component_flat0', coords=[10,20],level=0,
                full_view=dict(view_origin=[-396,369],redraw_rect=[0,0,WIDTH,HEIGHT]),
                scene_lighting=inputs['profiles']['default']))]
    alpha, inputs['shroud_edge'] = tile.physical_shroud_edge(m, files['SHROUD.SHP'], point=(66,111))
    rows.append(draw(m, typ, dict(name='physical_shroud_edge', scene_lighting=retail), alpha=alpha))
    return dict(surface=dict(width=WIDTH,height=HEIGHT,format='RGB565',background=65535), inputs=inputs, rows=rows)


def metadata():
    return native.provenance(scope=__doc__, assumptions=[
        'Existing TMP/PaletteReader/physical archive/CellConvert/SHP/A/Z owners, RGB565/MMX and High detail2; no native instructions are replaced.',
        'Physical expansion RULESMD/ARTMD/Battle/XMP03T4 exact lexical inputs and native selected constructor/whole reader. Physical declaration-order registry transport matches existing consumed stock fatal producer; integer-key registry enumeration is not executed in this drawing fixture.',
        'Sparse four native Cell constructors and original TMP slope/land getters over physical authored flat ClearTile0/sub0/level4. Original Place6B6080 owns piece stores; existing486E70 notification sink remains a supplied dirty boundary. No natural shot, full native map/LAT/Recalc or object retirement is claimed.',
        'Original full-view6D7560/currentCell480350/TMP/type6B55F0/shape4AED70 execute in the same VM. Smudge-only output then replays the exact captured native point/piece/Cell material arguments on a supplied RGB565 destination and unchanged A/Z, without a host pixel decoder or renderer.',
        'Uniform127/63/0 A, clip, viewport, repeat and destination/Z priors are supplied controls; separate physical SHROUD frame15 executes original47EFE0 in this same A surface. It is not a natural source-knowledge reveal.',
        'Drawing leaves Main RNG unchanged; no direct timer/detach. Full fatal producer/RNG/ID/expiry cleanup remains independently consumed in building_death_anims_joined_stock_smudges, not recertified here. Cell/Convert resource lifetime outside this bounded VM is open.',
    ], substitutions=['Existing allocator, CRT/TLS, physical resource and bounded Map current-Cell/admission transports. No drawing, palette, SHP, selector or pixel-store result is supplied.'],
    entry_points=dict(cell_ctor=0x47BBF0,cell_coord=0x485240,type_ctor=0x6B5260,type_reader=0x6B56D0,
        place=0x6B6080,cell_draw=0x480350,full_view=0x6D7560,smudge_draw=0x6B55F0,
        shape=0x4AED70,rle_selector=0x490E50,rle_rowwalker=0x437A10,opaque_a_leaf=0x4978C0))
