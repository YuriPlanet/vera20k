"""Native-generated synthetic raster goldens, including difficult branches.

python -m tools.voxel_oracle.raster_fixtures --write / --check
The input files are generated test art, not extracted commercial assets.

--bullet instead consumes extracted ZBOMB.VXL/HVA, VOXELS.VPL and current
RULESMD/ARTMD/MPBattleMD/Hills files from VERA20K_PROJECTILE_RENDER_ASSETS,
and records Bullet's original matrix/raster plus bounded Vertical flight.
"""
from pathlib import Path
import hashlib
import struct
import sys

from tools.native_oracle import finish_vectors, provenance
from tools.voxel_oracle.lighting import native_camera, native_facing, native_multiply
from tools.voxel_oracle.raster import render_native

IDENTITY = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.]


def test_files(reverse=False, no_hva=False, long_hull=False):
    body, headers, tailers, matrices = bytearray(), bytearray(), bytearray(), []
    count, nx, ny, nz = 4, 6, 5, 8
    for section in range(count):
        name = f'fixture{section}'.encode().ljust(16, b'\0')
        headers += name + struct.pack('<3I', section, 1, 0)
        stream, starts, ends = bytearray(), [], []
        for column in range(nx * ny):
            if section == 3 or column in (5, 9, 20):
                starts.append(-1)
                ends.append(-1)
                continue
            starts.append(len(stream))
            if no_hva and section == 2:
                # Native consumes these valid empty runs without imposing a
                # voxel-height-based group limit. Each still advances bytes.
                stream += bytes(30)
            for run, (skip, run_count) in enumerate(((0, 2), (2, 1), (2, 1))):
                stream += bytes((skip, run_count))
                for voxel in range(run_count):
                    color = 17 if (column + run + section) % 4 == 0 else 32 + section * 20 + run
                    stream += bytes((color, (column * 7 + run * 19 + voxel) % 245))
                stream.append(run_count)
            ends.append(len(stream) - 1)
        start_offset, end_offset = len(body), len(body) + nx * ny * 4
        data_offset = len(body) + nx * ny * 8
        body += struct.pack('<' + 'i' * len(starts), *starts)
        body += struct.pack('<' + 'i' * len(ends), *ends)
        body += stream
        scale = (0.5, 0.25, 2., 1.)[section]
        # The fourth section is empty but still changes native union/crop.
        bounds = [-2.37, -1.91, -0.43, 3.78, 3.04, 7.79]
        if long_hull:
            bounds[0], bounds[3] = -89.37, 88.78
        tailer_matrix = IDENTITY.copy()
        if no_hva:
            tailer_matrix[3], tailer_matrix[7], tailer_matrix[11] = 41.25, -19.5, 77.75
        tailers += struct.pack('<3If12f6f4B', start_offset, end_offset, data_offset,
                               scale, *tailer_matrix, *bounds, nx, ny, nz, 4)
        matrix = IDENTITY.copy()
        if reverse:
            matrix[5], matrix[6], matrix[9], matrix[10] = -.6, -.8, .8, -.6
        if section == 2:
            matrix[11] = -3.
        elif section == 3:
            matrix[3], matrix[7], matrix[11] = 9.375, -1.125, 2.625
        matrices.append(matrix)
    vxl = b'Voxel Animation\0' + struct.pack('<4I', 1, count, count, len(body))
    vxl += bytes(770) + headers + body + tailers
    hva = b'NativeFixture\0'.ljust(16, b'\0') + struct.pack('<2I', 1, count)
    hva += b''.join(f'fixture{i}'.encode().ljust(16, b'\0') for i in range(count))
    hva += b''.join(struct.pack('<12f', *matrix) for matrix in matrices)
    vpl = struct.pack('<4I', 16, 31, 32, 0) + bytes(768)
    vpl += bytes(0 if color == 17 else color for _page in range(32) for color in range(256))
    return vxl, None if no_hva else hva, vpl


def generate():
    camera, models = native_camera(), []
    seen_reverse = False
    seen_unstable = False
    seen_zero_erase = False
    for name, reverse, no_hva, long_hull in (
            ('ordinary_sections', False, False, False),
            ('tilted_reverse', True, False, False),
            ('no_hva_ignores_tailer_transform', False, True, False),
            ('long_sections', False, False, True)):
        vxl, hva, vpl = test_files(reverse, no_hva, long_hull)
        cases = []
        for step in (0, 3, 8, 12, 16, 20, 24, 28):
            result = render_native(vxl, hva, vpl, native_multiply(camera, native_facing(step)))
            seen_reverse |= any(p['entry'] == 0x7dfae0 for p in result['raster_params'])
            seen_zero_erase |= result['write_counts']['zero_erases_nonzero'] > 0
            assert len(result['raster_params']) == 4, 'empty section must still submit and raster-dispatch'
            # Sections 0/1 have identical bounds and HVA but different colors;
            # the pairwise exchange sort may reverse their submitted tie order.
            order = result['paint_order']
            seen_unstable |= order.index(1) < order.index(0)
            cases.append({'step': step, **result})
        models.append({'name': name, 'vxl': vxl.hex(),
                       'hva': hva.hex() if hva is not None else None,
                       'vpl': vpl.hex(), 'cases': cases})
    assert seen_reverse, 'fixture must exercise native backward encoded traversal'
    assert seen_unstable, 'fixture must exercise indirectly reversed equal-depth section ties'
    assert seen_zero_erase, 'fixture must exercise zero result erasing an earlier visible pixel'
    return {'source': 'unicorn/gamemd.exe', 'models': models}


def bullet_flights(root):
    """Physical readers -> original launch -> Vertical motion; admission supplied."""
    from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP
    from tools.native_oracle import run_checked
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.projectile_oracle.bridge_render_art_state import ArtStateReader
    from tools.projectile_oracle import fireat_speed, fireat_fixture, vertical_motion
    from tools.spatial_oracle.building_body_rules import RULES, SP, dwords
    art_raw = (root / 'ARTMD.INI').read_bytes()
    art, _ = lexical(art_raw, {'BlimpBombP', 'ZBOMB'})
    m = ArtStateReader(art)
    u = m.u
    u.mem_write(0x887568, dwords(0x7eb6d4, m.alloc(4096), 1024, 1, 0, 10))
    weapons = {name: m.invoke(0x772fa0, m.cstring(name)) for name in ('BlimpBomb', 'BlimpBombE')}
    rr = m.alloc(0x2000)
    u.mem_write(0x8871e0, dwords(rr))
    u.reg_write(UC_X86_REG_ESI, rr)
    run_checked(u, 0x6674d6, 0x6674e0)
    layers = []
    for name in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'):
        path = root / name
        if not path.exists():
            assert name == 'LANGRULE.INI'
            layers.append(dict(file=name, absent=True))
            continue
        raw = path.read_bytes()
        sections, _ = lexical(raw, {'ZEP', 'BlimpBomb', 'BlimpBombE', 'BlimpBombP', 'AudioVisual'})
        m.rules_cache(sections)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, rr)
        u.reg_write(UC_X86_REG_EDI, RULES)
        run_checked(u, 0x66b3c4, 0x66b3e4)
        processed = {}
        for weapon_name, w in weapons.items():
            m.invoke(0x772080, w, (RULES,))
            m.read_layer(m.read32(w + 0xa0), sections)
            before = m.read32(w + 0xa8)
            m.invoke(0x7729f0, w)
            p = m.read32(w + 0xa0)
            processed[weapon_name] = dict(reader_speed=before, processed_speed=m.read32(w + 0xa8),
                gravity=m.read32(rr + 0x16b8), range_leptons=m.read32(w + 0xb4),
                vertical=bool(u.mem_read(p + 0x2c0, 1)[0]),
                arcing=bool(u.mem_read(p + 0x29b, 1)[0]), floater=bool(u.mem_read(p + 0x295, 1)[0]))
        layers.append(dict(file=name, sha256=hashlib.sha256(raw).hexdigest(), sections=sections,
                           processed=processed))
    rows = []
    for name, w in weapons.items():
        p = m.read32(w + 0xa0)
        signed = lambda offset: struct.unpack('<i', u.mem_read(p + offset, 4))[0]
        state = dict(identity=m.string(p + 0x24), voxel=bool(u.mem_read(p + 0x236, 1)[0]),
                     vertical=bool(u.mem_read(p + 0x2c0, 1)[0]), acceleration=signed(0x2d0),
                     rot=signed(0x2dc), arm=signed(0x2f0),
                     arcing=bool(u.mem_read(p + 0x29b, 1)[0]),
                     floater=bool(u.mem_read(p + 0x295, 1)[0]))
        assert state['identity'] == 'BlimpBombP' and state['voxel'] and state['vertical']
        origin, target = [3200, 3200, 1000], [3456, 3200, 0]
        distance = fireat_speed.distance(dict(source=origin[:2], target=target[:2]))
        speed = fireat_speed.get_speed(dict(projectile=True, rot=state['rot'],
            floater=state['floater'], gravity=m.read32(rr + 0x16b8),
            speed=m.read32(w + 0xa8), distance=distance['distance']))
        delta = [target[i] - origin[i] for i in range(3)]
        launch = fireat_fixture.run(*delta, speed=speed['speed'], arcing=state['arcing'],
            floater=state['floater'], variant='voxel', voxel=state['voxel'], vertical=state['vertical'],
            stored_weapon_speed=m.read32(w + 0xa8))
        assert launch['success']
        motion = vertical_motion.run(launch['velocity'], state['acceleration'], launch['max_speed'], origin)
        rows.append(dict(weapon=name, projectile=state, origin=origin, target=target,
                         distance=distance, speed=speed, launch=launch,
                         frames=[dict(frame=i + 1, **motion['frames'][i]) for i in (0, 3, 7)]))
    return dict(art_sha256=hashlib.sha256(art_raw).hexdigest(), art=art, layers=layers, shots=rows)


def bullet_generate():
    from tools.projectile_oracle.bridge_render_inputs import assets_root
    root = assets_root()
    assets = {key: (root / name).read_bytes() for key, name in
              (('vxl', 'ZBOMB.VXL'), ('hva', 'ZBOMB.HVA'), ('vpl', 'VOXELS.VPL'))}
    flights = bullet_flights(root)
    vectors = [('zero', (0., 0., 0.)), ('signed_zero', (-0., -0., -0.))]
    vectors += [(name, vector) for name, vector in (
        ('east', (1., 0., 0.)), ('west', (-1., 0., 0.)), ('south', (0., 1., 0.)),
        ('north', (0., -1., 0.)), ('up', (0., 0., 1.)), ('down', (0., 0., -1.)))]
    # Fixed binary64 probes bracket actual native atan-table transitions,
    # found by native-only output observation. Host trigonometry is not an input.
    boundaries = [
        ('heading_boundary_below', ('3ff0000000000000', '3fbf3f7fffffffff', '0000000000000000')),
        ('heading_boundary_above', ('3ff0000000000000', '3fbf3fcccccccccc', '0000000000000000')),
        ('heading_wrap_below', ('bfb934b333333333', 'bff0000000000000', '0000000000000000')),
        ('heading_wrap_above', ('bfb9350000000000', 'bff0000000000000', '0000000000000000')),
        ('pitch_boundary_below', ('3ff0000000000000', '0000000000000000', '3fbf3f7fffffffff')),
        ('pitch_boundary_above', ('3ff0000000000000', '0000000000000000', '3fbf3fcccccccccc')),
    ]
    vectors = [(name, [f'{word:016x}' for word in struct.unpack('<3Q', struct.pack('<3d', *vector))])
               for name, vector in vectors] + boundaries
    controls = []
    for name, bits in vectors:
        body = render_native(**{f'{key}_bytes': value for key, value in assets.items()}, bullet_velocity_bits=bits)
        observation = body.pop('bullet')
        controls.append(dict(name=name, velocity_bits=bits, **observation, matrix_bits=body['input_matrix_bits'], body=body))
    by_name = {row['name']: row for row in controls}
    for name, field in (('heading_boundary', 'heading_step'), ('heading_wrap', 'heading_step'),
                        ('pitch_boundary', 'pitch_step')):
        assert by_name[name + '_below'][field] != by_name[name + '_above'][field]
    for shot in flights['shots']:
        for frame, bits in [(0, shot['launch']['bits'])] + [(f['frame'], f['bits']) for f in shot['frames']]:
            body = render_native(**{f'{key}_bytes': value for key, value in assets.items()}, bullet_velocity_bits=bits)
            observation = body.pop('bullet')
            controls.append(dict(name=f'{shot["weapon"]}_frame{frame}', velocity_bits=bits,
                                 **observation, matrix_bits=body['input_matrix_bits'], body=body))
    sources = dict(vxl=('ZBOMB.VXL', 'ra2.mix -> local.mix', 'B6F904E0'),
                   hva=('ZBOMB.HVA', 'ra2.mix -> local.mix', '09600347'),
                   vpl=('VOXELS.VPL', 'ra2md.mix -> localmd.mix', 'DECF668B'))
    return dict(source='unicorn/gamemd.exe', assets={
        key: dict(name=sources[key][0], archive=sources[key][1], entry_id=sources[key][2],
                  sha256=hashlib.sha256(value).hexdigest(), bytes=len(value))
        for key, value in assets.items()}, controls=controls, flight=flights)


def bullet_metadata():
    return provenance(scope='Original Bullet468090 orientation and46B0C0 packed ZBOMB raster through754510; stop before490B90. Fourteen synthetic velocity boundaries and eight native Vertical flight states.',
        assumptions=[
            'Original x87 0E7F, camera startup and light/viewer initialization. Immutable retail ZBOMB.VXL/HVA and VOXELS.VPL inputs; one frame and all sections.',
            'Asset metadata identifies local retail inputs; no retail VXL/HVA/VPL file bytes are serialized. Rust consumers independently load those files through the existing retail-assets owner and check their recorded length/SHA256.',
            'Physical weapon/Bullet readers establish BlimpBomb/BlimpBombE and BlimpBombP flags, speed and acceleration. Existing ArtStateReader supplies the prior ART cache invalidation and skips voxel asset binding; raster owner independently executes VXL/HVA loaders on physical bytes.',
            'Each admitted layer records raw keys, native Speed reader result before7729F0 and processed result afterward. AudioVisual.Gravity is read through66B3C4 before the postpass; current retail rawSpeed20 reads51 and postprocesses52 at Gravity6/Range384. Vertical motion consumes Acceleration1 and retained maximum52; no gravity is applied in that branch.',
            'Source[3200,3200,1000] and target[3456,3200,0] are supplied coordinates, not an executed ZEP locomotor/aim producer. Shared original distance/GetSpeed/FireAt-tail/Vertical blocks supply velocities. Admitted commits are supplied between Vertical visits; no full AI/collision/Logic claim.',
            'Fixed binary64 inputs bracket original atan-table heading/pitch transitions. All expected words, quantized steps, matrices, normal pages and raster pixels are produced by original instructions.',
            'Type Color1 is supplied only for observed pointer selection after raster. Generic native474A90 current0/missing->LightGold1 is separately consumed in campaign_start_houses.json. This corpus does not prove Bullet Color reads, RGB565, full surface clipping, GPU or whole-scene parity.',
        ], substitutions=['Immutable file vtable IO and bounded allocation, as shared raster owner.',
                          'Inherited physical reader and shared FireAt-tail supplied virtual/world boundaries. No numeric/rotation/raster leaf replaced.'],
        entry_points=dict(bullet_draw=0x468090, voxel_draw=0x46b0c0, raster=0x754510,
                          stop_before_convert=0x46b21a, vertical_begin=0x4671e0, vertical_end=0x467334))


if __name__ == '__main__' and '--bullet' in sys.argv:
    args = sys.argv[1:]
    args.remove('--bullet')
    finish_vectors(bullet_generate, Path(__file__).with_name('bullet_fixtures.json'),
                   provenance=bullet_metadata, argv=args)
elif __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Four synthetic VXL models at eight facings: ordinary/reverse-Z, no HVA and long bounds; full native buffers and metadata',
        assumptions=[
            'original x87 0E7F and original startup camera/light/viewer initializers',
            'immutable generated VXL/HVA/VPL files, normal mode 4, frame zero, unmirrored opaque Render core',
            'all sections submitted including empty; original HVA scaling uses limb zero',
            'native loader, normal loop, bounding corners, section exchange sort and both 7DF9C0/7DFAE0 execute',
            'zero VPL output, raw run/skip/duplicate grouping, unequal limb scales and empty section are intentional test inputs',
            'no-HVA model carries nonidentity tailer transforms and ten empty prefix runs in one section',
        ],
        substitutions=['file vtable open/read/seek/close over bounded immutable input bytes',
                       'operator new returns distinct mapped emulator heap allocations'],
        entry_points={'vxl_loader': 0x755DB0, 'hva_loader': 0x5BD5C0, 'hva_scale': 0x5BD730,
                      'submit_box': 0x7540F0, 'sort_rasterize': 0x754510,
                      'section_prepare': 0x756590, 'forward': 0x7DF9C0, 'backward': 0x7DFAE0}))
