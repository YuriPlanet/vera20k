"""Physical static cliff TMP through the existing original Cell raster owner."""
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDI
from tools import native_oracle as native
from tools.spatial_oracle import tmp_clear_render as tile
from tools.spatial_oracle.bridge_shadow_render import CELL, WIDTH, HEIGHT, COUNT, PIXELS, Z, ints
from tools.spatial_oracle.building_body_rules import dwords
from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells

COORDS = (18, 69)
NAME = 'cliff01.tem'


def prepare():
    m, inputs, files = tile.prepare(tmp_names=(NAME,), hills=True)
    u = m.u
    parsed, authored = decode_cells(files['XMP03T4.MAP'])
    source = authored[COORDS]
    tiles, mapping = tile.theater_tile_files(m, files['temperatmd.ini'], source['tile'], 'tem')
    assert tiles[source['tile']] == NAME
    assert source['subtile'] == 2 and source['level'] == 4 and source['overlay'] is None
    head = m.read32(m.read32(0xA8ED2C))
    registry = m.alloc(len(tiles) * 4)
    u.mem_write(registry + source['tile'] * 4, dwords(head))
    u.mem_write(0xA8ED2C, dwords(registry))
    u.mem_write(0xA8ED38, dwords(len(tiles)))
    inputs['binding'] = dict(name=NAME, mapping=mapping, registered_count=len(tiles),
        file_count=m.read32(head + 0x2F0), next_file=m.read32(head + 0x2BC),
        boundary='Native theater scalar/string reads and original TMP constructor/lazy loader; sparse selected primary registry publication is supplied, not full theater registration.')
    inputs['theater'] = dict(name='TEMPERATE', selected=mapping,
        clear_tile_ordinal=inputs['theater']['clear_tile_ordinal'],
        boundary='Original General and selected ordinal readers; the ClearTile default is retained as an independently read global, not the selected cliff identity.')
    inputs['map'] = dict(name='XMP03T4.MAP', physical_map_section=parsed['Map'],
        cells=[dict(coords=list(COORDS), **source)],
        boundary='Physical authored tile/subtile/height/overlay tuple; sparse native Cell initialization, not full native map/LAT/Recalc.')
    inputs['profiles'] = dict(default=tile.profile(m), XMP03T4=tile.profile(m, files['XMP03T4.MAP']),
                              Hills=tile.profile(m, files['Hills.map']))
    hills_ini, hills_cells = decode_cells(files['Hills.map'])
    hills_source = hills_cells[(103, 64)]
    assert hills_source['tile'] == source['tile'] and hills_source['subtile'] == source['subtile']
    assert hills_source['level'] == 2 and hills_source['overlay'] is None
    inputs['hills_map'] = dict(name='Hills.mmx', inner_name='Hills.map',
        physical_map_section=hills_ini['Map'], cells=[dict(coords=[103, 64], **hills_source)],
        boundary='Physical authored tuple; sparse original native Cell enrollment remains a component boundary, not native map construction.')
    return m, inputs, files, head, source


def draw(m, head, source, case, *, inspect_planes=False, warm=False, allow_empty=False):
    u = m.u
    case = dict(coords=list(COORDS), level=source['level'], viewport_y=0,
        clip=[0, 0, WIDTH, HEIGHT], old_z=65535, background=65535, a=127, repeat=1,
        type=NAME, state=0, detail=2,
        full_view=dict(view_origin=[-1626, 1164], redraw_rect=[0, 0, WIDTH, HEIGHT])) | case
    xy = tuple(case['coords'])
    if not warm:
        m.invoke(0x47BBF0, CELL)
        pair = m.alloc(4)
        u.mem_write(pair, struct.pack('<hh', *xy))
        m.invoke(0x485240, CELL, (pair,))
        u.mem_write(CELL + 0x38, dwords(source['tile']))
        slope = m.invoke(0x5471B0, head, (source['subtile'],)) & 255
        land = m.invoke(0x544BE0, head, (source['subtile'],))
        u.mem_write(CELL + 0x11A, bytes((source['subtile'], case['level'], slope, 0)))
        u.mem_write(CELL + 0xEC, dwords(land))
    reached, extra_writes, zero_sources, plane = set(), set(), set(), {}
    def observe(u, pc, n, d):
        if pc in (0x544CB0, 0x549133, 0x549161, 0x5492EC, 0x549323, 0x549397, 0x5493EA,
                  0x54953F, 0x549577, 0x549592):
            reached.add(pc)
        if inspect_planes and pc == 0x549133:
            plane['diamond_depth'] = bytes(u.mem_read(Z, COUNT * 2))
            header = u.reg_read(UC_X86_REG_EBX)
            plane['header'] = ints(u, header, 10)
        if inspect_planes and pc == 0x54953F and u.mem_read(u.reg_read(UC_X86_REG_EDI), 1) == b'\0':
            dest = u.reg_read(UC_X86_REG_EAX)
            assert PIXELS <= dest < PIXELS + COUNT * 2
            zero_sources.add((dest - PIXELS) // 2)
    hook = u.hook_add(UC_HOOK_CODE, observe)
    def writes(u, access, address, size, value, data):
        if 'diamond_depth' in plane and PIXELS <= address < PIXELS + COUNT * 2:
            assert size == 2
            extra_writes.add((address - PIXELS) // 2)
    write_hook = u.hook_add(UC_HOOK_MEM_WRITE, writes) if inspect_planes else None
    row = tile.execute(m, case, current_cell=CELL, current_cells={xy: CELL}, allow_empty=allow_empty)
    u.hook_del(hook)
    if write_hook is not None:
        u.hook_del(write_hook)
        before = struct.unpack('<' + 'H' * COUNT, plane['diamond_depth'])
        after = struct.unpack('<' + 'H' * COUNT, u.mem_read(Z, COUNT * 2))
        overlaps = [i for i in sorted(extra_writes) if before[i] != case['old_z']]
        row['output']['plane_writes'] = dict(
            diamond_pixels=sum(z != case['old_z'] for z in before),
            extra_pixels=len(extra_writes), overlap_pixels=len(overlaps))
        row['native_plane_observations'] = dict(header_dwords=plane['header'],
            overlap_depth_pairs=sorted({(before[i], after[i]) for i in overlaps}),
            extra_source_zero_pixels=len(zero_sources),
            extra_source_zero_examples=[[i % WIDTH, i // WIDTH] for i in sorted(zero_sources)[:4]])
    row['reached'] = sorted(set(row['reached']) | {f'{pc:08X}' for pc in reached})
    row['output']['main_rng_unchanged'] = True
    return row


def generate():
    m, inputs, files, head, source = prepare()
    retail = inputs['profiles']['XMP03T4']
    rows = [draw(m, head, source, dict(name='retail_cliff', scene_lighting=retail), inspect_planes=True)]
    baseline = rows[0]
    assert baseline['output']['plane_writes']['overlap_pixels'] == 0
    assert baseline['native_plane_observations']['extra_source_zero_pixels'] > 0
    for name, clip in (('face_only_viewport', [0, 51, WIDTH, 30]),
                       ('top_clip', [0, 70, WIDTH, 90]),
                       ('side_clip', [100, 0, 92, HEIGHT])):
        rows.append(draw(m, head, source, dict(name=name, clip=clip,
            full_view=dict(view_origin=[-1626, 1164], redraw_rect=clip), scene_lighting=retail)))
    for name, a in (('a_zero', 0), ('a63', 63)):
        rows.append(draw(m, head, source, dict(name=name, a=a, scene_lighting=retail)))
    rows.append(draw(m, head, source, dict(name='repeat', repeat=2, scene_lighting=retail)))
    # Candidate is a genuine original extra-face raster output, not a host
    # projection/depth formula or a Rust-produced fixture input.
    x, y, _length, _color, candidate = next(run for run in baseline['output']['runs'] if run[1] < 81)
    for name, old_z in (('near', candidate + 1), ('equal', candidate), ('far', candidate - 1)):
        row = draw(m, head, source, dict(name=f'extra_depth_{name}', old_z=old_z,
            clip=[x, y, 1, 1], scene_lighting=retail))
        row['depth_probe'] = dict(pixel=[x, y], candidate=candidate,
            source='Original retail_cliff extra-face raster output above diamond top81')
        rows.append(row)
    alpha, inputs['shroud_edge'] = tile.physical_shroud_edge(m, files['SHROUD.SHP'], point=(66, 51))
    rows.append(draw(m, head, source, dict(name='physical_shroud_edge', a_hex=alpha.hex(), scene_lighting=retail)))
    assert rows[-1]['output']['color_sha256'] != baseline['output']['color_sha256']
    # Reuse the final live Cell/Convert across actual camera departure and
    # return. Surface/A/Z priors are reset by the existing execute owner.
    rows.append(draw(m, head, source, dict(name='camera_away', scene_lighting=retail,
        full_view=dict(view_origin=[-1626, 2164], redraw_rect=[0, 0, WIDTH, HEIGHT])), warm=True, allow_empty=True))
    rows.append(draw(m, head, source, dict(name='camera_return', scene_lighting=retail), warm=True))
    rows.append(draw(m, head, source, dict(name='retail_hills_cliff', coords=[103, 64], level=2,
        scene_map='Hills.mmx', scene_lighting=inputs['profiles']['Hills'],
        full_view=dict(view_origin=[1074, 2394], redraw_rect=[0, 0, WIDTH, HEIGHT]))))
    return dict(inputs=inputs,
        surface=dict(width=WIDTH, height=HEIGHT, baseline=32768, bytes_per_pixel=2), rows=rows)


def metadata():
    return native.provenance(scope='Original ordinary TEMPERATE Cell480350 diamond and TMP extra face for physical cliff01.tem sub2 at authored XMP03T4 cell18,69/level4 and Hills103,64/level2, with exact native RGB565/Z output. No whole map bootstrap or native whole-frame claim.', assumptions=[
        'Existing PaletteReader VM, archive extraction, RGB565/MMX startup, original TMP constructor5447C0/GetImage544CB0/full lazy547020, CellConvert and A/Z/color surfaces are reused. File IO runs during preparation; subsequent Cell getters use that resident original resource. No host TMP decoder, palette kernel, drawing loop or expected pixels are introduced.',
        'Original General545535..545C3F/ordinal545CEF..545FA3 and selected scalar/string reads establish TileSet0010/Cliff/base49/count40. Sparse selected primary838-entry registry publication, pristine one-file chain metadata and native Cell initialization from authored XMP03T4 packs are explicit fixture boundaries, not full theater registration, Map bootstrap, LAT or Recalc.',
        'Original Scenario defaults and full normal Lighting reader68A817..68A98A provide the physical XMP03T4 and Hills profiles. Original cold483E30/484180/544E70 selects the actual ISO-source CellConvert; each cold row records normalized RGB, quantized Convert key and native top/common/bottom scalars. Warm camera-away/return retains this same original Cell/cache.',
        'Original full6D7560 traversal,47FF80 extra-aware bounds,480350 and547CF0 run on the same VM. Surface clip/redraw/camera, destination/Z and uniform A are supplied controls. A physical SHROUD frame15 executes original47EFE0 in this same A plane and demonstrably changes selected face colors; natural reveal scheduling is outside scope.',
        'Original diamond and extra pixel-store observation on the first row records900 diamond and906 extra pixels with zero destination overlap; physical sub2 has no covered self-overlap for a conditional old-Z control. Native extra source0 skips546 pixels. Real extra raster output supplies the near/equal/far candidate; no synthetic TMP or Rust-derived depth is used.',
        'The shared preparation initializes the historical64-entry process table through original4814F0/Main seed0 (128 original draws), preserving its legacy controls. This selected single-file cliff bypasses the variant selector. Every selected draw leaves Main bytes unchanged; repeated calls retain A/Z/color until completion. No timer or detach body is passed by the selected local drawing chain. General resource retirement outside the prepared original file IO and CellConvert cache boundary remains open.',
    ], substitutions=['Existing allocator/CRT/RawFile and bounded Map lookup/admission transports. No drawing instructions or native pixel results are replaced.'],
    entry_points=dict(full_view=0x6D7560, cell=0x480350, bounds=0x47FF80, tmp=0x547CF0,
        tmp_ctor=0x5447C0, tmp_get_image=0x544CB0, tmp_lazy=0x547020,
        cell_light=0x483E30, gather=0x484180, convert_select=0x544E70,
        extra_admission=0x549133, extra_color=0x5492EC, extra_depth=0x549323,
        extra_scalar_loop=0x54953F, extra_color_store=0x549577, extra_depth_store=0x549592,
        wrapped_extra_loop=0x549397, shroud_leaf=0x47EFE0))
