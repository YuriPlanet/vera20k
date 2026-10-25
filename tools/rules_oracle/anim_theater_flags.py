"""Original fresh AnimType Image-section flags and own-section overrides.

Runs the original type constructor, full ART reader and theater postpass. Named
SHPs are declared eight-byte headers supplied at the existing MIX lookup seam;
no retail image bytes, instance lifecycle or drawing are part of this corpus.
"""
from pathlib import Path
import struct
import tempfile

from tools.native_oracle import finish_vectors, provenance

HERE = Path(__file__).resolve().parent
STOCK_ART = {
    'CAMOV02_A': {
        'NewTheater': 'yes', 'Normalized': 'yes', 'LoopStart': '0',
        'LoopEnd': '2', 'LoopCount': '-1', 'Rate': '100', 'Layer': 'ground',
        'Shadow': 'yes', 'DemandLoad': 'true',
    },
    'CAMOV02_AD': {
        'Image': 'CAMOV02_A', 'Normalized': 'yes', 'Start': '2',
        'LoopStart': '2', 'LoopEnd': '4', 'LoopCount': '-1', 'Rate': '100',
        'Layer': 'ground', 'Shadow': 'yes', 'DemandLoad': 'true',
    },
}
HEADER = dict(width=2, height=2, raw_frame_count=8)


def cases():
    rows = []

    def add(label, art, *, name='CAMOV02_AD', theater=2, literal=False):
        filenames = ['CUMOV02_A.SHP'] if theater == 2 else []
        if literal or theater == 1:
            filenames.append('CAMOV02_A.SHP')
        rows.append(dict(case=label, name=name, theater=theater,
                         art_sections=art,
                         available_images={n: dict(HEADER) for n in filenames}))

    add('stock_art_urban_normal', STOCK_ART, name='CAMOV02_A')
    add('stock_art_urban_damaged', STOCK_ART)
    add('stock_art_snow_damaged', STOCK_ART, theater=1)
    add('missing_art_constructor_defaults', {}, name='PRIVATE_MISSING')
    common = {'Image': 'CAMOV02_A', 'Shadow': 'yes', 'Start': '2',
              'LoopStart': '2', 'LoopEnd': '4'}

    def control(label, own, target, *, literal=False):
        art = {'CAMOV02_AD': dict(common, **own)}
        if target is not None:
            art['CAMOV02_A'] = target
        add(label, art, literal=literal)

    for field in ('NewTheater', 'Theater'):
        prefix = 'new_theater' if field == 'NewTheater' else 'theater'
        for own, target in [('yes', 'no'), ('no', 'yes')]:
            for literal in (False, True):
                control(f'{prefix}_own_{own}_target_{target}_literal_{int(literal)}',
                        {field: own}, {field: target}, literal=literal)
        control(f'{prefix}_own_absent_target_yes', {}, {field: 'yes'})
        control(f'{prefix}_own_invalid_target_yes', {field: 'garbage'}, {field: 'yes'})
        control(f'{prefix}_own_invalid_target_no', {field: 'garbage'}, {field: 'no'})
        control(f'{prefix}_own_yes_target_missing', {field: 'yes'}, None)
        control(f'{prefix}_own_absent_target_missing', {}, None)
        control(f'{prefix}_target_invalid_own_absent', {}, {field: 'garbage'})
    control('image_alias_is_one_hop', {},
            {'Image': 'PRIVATE_UNREGISTERED', 'NewTheater': 'yes'})
    control('empty_image_skips_own_flags',
            {'Image': '', 'Theater': 'yes', 'NewTheater': 'yes'},
            {'Theater': 'yes', 'NewTheater': 'yes'})
    return rows


def run_case(case):
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
    from tools.projectile_oracle.bridge_render_inputs import BulletReader
    from tools.spatial_oracle.building_body_rules import INI, dwords

    # The shared reader accepts a directory of resources. An empty temporary
    # directory prevents ambient files from influencing this declared catalog.
    with tempfile.TemporaryDirectory(prefix='vera20k-anim-theater-') as directory:
        m = BulletReader(case['art_sections'], Path(directory))
    u = m.u
    m.assets.update({name: struct.pack('<4H', 0, data['width'], data['height'],
                                      data['raw_frame_count'])
                     for name, data in case['available_images'].items()})
    u.mem_write(m.read32(0xA8B230) + 0x1258, dwords(case['theater']))
    assert struct.unpack('<i', u.mem_read(0x822CF8, 4))[0] == -1
    assert u.mem_read(0xA8ED6B, 1) == b'\0'
    original_text = bytes(u.mem_read(0x401000, 0x3E0000))
    original_table = bytes(u.mem_read(0x7E3608, 0xB0))
    reached, resources, bool_reads = set(), [], []
    pending = None
    phase = 'constructor'

    def observe(_u, pc, _size, _data):
        nonlocal pending
        sp = u.reg_read(UC_X86_REG_ESP)
        if pending and pc == pending[0] and sp == pending[1]:
            pending[2]['returned'] = bool(u.reg_read(UC_X86_REG_EAX) & 0xFF)
            pending = None
        if pc in (0x428B80, 0x427530, 0x427D00, 0x427A80, 0x427B50,
                  0x428C30, 0x428E50, 0x5F9070):
            reached.add(pc)
        if pc == 0x5295F0:
            key = m.string(m.read32(sp + 8))
            if key in ('Theater', 'NewTheater'):
                assert pending is None
                row = dict(section=m.string(m.read32(sp + 4)), key=key,
                           default=bool(m.read32(sp + 12) & 0xFF))
                bool_reads.append(row)
                pending = (m.read32(sp), sp + 16, row)
        if pc == 0x5B40B0:
            name = m.string(u.reg_read(UC_X86_REG_ECX))
            resources.append(dict(phase=phase, filename=name,
                                  available=name.upper() in m.assets))
        # These rows exercise ordinary freshly constructed types. A newly
        # reached demand-file path must not be silently answered by a fixture.
        assert pc != 0x428C55, 'unexpected demand-load body'

    hook = u.hook_add(UC_HOOK_CODE, observe)
    try:
        typ = m.invoke(0x428B80, m.cstring(case['name']))
        assert m.read32(typ) == 0x7E3608
        assert m.read32(m.read32(typ) + 0x60) == 0x427A80

        def state():
            ptr = m.read32(typ + 0xA4)
            names = [name for name, address in m.asset_ptr.items() if address == ptr]
            assert not ptr or len(names) == 1
            return dict(image=m.string(typ + 0x1F8), image_present=bool(ptr),
                        loaded_filename=names[0] if ptr else None,
                        theater=bool(u.mem_read(typ + 0x22C, 1)[0]),
                        new_theater=bool(u.mem_read(typ + 0x237, 1)[0]),
                        demand_load=bool(u.mem_read(typ + 0x35E, 1)[0]),
                        raw_shp_frame_count=struct.unpack('<h', u.mem_read(ptr + 6, 2))[0] if ptr else 0,
                        end=struct.unpack('<i', u.mem_read(typ + 0x2C0, 4))[0],
                        loop_end=struct.unpack('<i', u.mem_read(typ + 0x2BC, 4))[0])

        constructor = state()
        phase = 'reader'
        admitted = bool(m.invoke(0x427D00, typ, (INI,)) & 0xFF)
        after_reader = state()
        assert pending is None
        phase = 'theater_postpass'
        m.invoke(0x427A80, typ, (case['theater'],))
        after_postpass = state()
        phase = 'get_image'
        returned = m.invoke(0x428C30, typ)
        assert returned == m.read32(typ + 0xA4)
        assert state() == after_postpass
        assert {0x428B80, 0x427530, 0x427D00, 0x427A80, 0x428C30} <= reached
        if admitted:
            # ObjectType's RTTI==5 branch bypasses its image load; literal
            # availability cannot suppress the fresh Anim own-flag reads.
            assert 0x428E50 in reached
        assert original_text == bytes(u.mem_read(0x401000, 0x3E0000))
        assert original_table == bytes(u.mem_read(0x7E3608, 0xB0))
        return dict(input=case, reader_admitted=admitted, constructor=constructor,
                    after_reader=after_reader, after_postpass=after_postpass,
                    boolean_reads=bool_reads, resource_requests=resources,
                    reached=[f'{pc:08X}' for pc in sorted(reached)])
    finally:
        u.hook_del(hook)


def generate():
    inputs = cases()
    assert len(inputs) == 26
    assert len({row['case'] for row in inputs}) == len(inputs)
    return dict(schema=1, cases=[run_case(case) for case in inputs])


def metadata():
    return provenance(
        scope='Fresh AnimType Image-section Theater/NewTheater defaults, own-section overrides and theater postpass',
        assumptions=[
            'Fresh original type factory/constructor per row. INI caches contain the declared exact lexical sections. Original ReadString/ReadBool and full ObjectType/AnimType readers execute; whole INI file parsing and Scenario startup do not.',
            'The first three rows use the selected stock CAMOV02_A/AD ART keys, but all images are declared synthetic eight-byte SHP headers, not retail payloads. Named availability is a fixture input. No frame decoding or drawing is attempted.',
            'Original per-type LoadTheaterSpecificArt427A80 is called at its startup boundary with the supplied scene theater. Untouched LastTheater=-1 and map-editor=false are asserted. The whole Scenario type-array loop is excluded.',
            'Original text401000..7E1000 and AnimType vtable7E3608 lengthB0 remain byte-identical. GetImage428C30 executes without injecting DemandLoad, flags, image pointers or frame bounds.',
            'Only fresh type parsing is compared. The own-flag block conditional on a resident image during later INI reloads, demand-loaded histories, instance lifetime, Building slots, damage/repair and full Theater filename policy are not certified.'],
        substitutions=[
            'Existing Lists/Reader successful heap, TLS and deallocation transport; cached INI section/index storage.',
            'Existing LoadFileFromMIX5B40B0 resource transport returns the declared named header bytes or NULL. Original filename formation, read order, flag decisions and header-derived bounds execute unchanged.'],
        entry_points=dict(factory=0x428B80, constructor=0x427530,
                          object_reader=0x5F92D0, anim_reader=0x427D00,
                          boolean_reader=0x5295F0, theater_postpass=0x427A80,
                          image_and_bounds=0x427B50, get_image=0x428C30,
                          anim_type_rtti=0x428E50))


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'),
                   provenance=metadata,
                   source_paths={
                       'producer': Path(__file__),
                       'bullet_reader': HERE.parent / 'projectile_oracle/bridge_render_inputs.py',
                       'anim_reader': HERE / 'bridge_anim_inputs.py',
                       'lists': HERE / 'bridge_anim_lists.py',
                       'ini_storage': HERE.parent / 'spatial_oracle/building_body_rules.py',
                       'ini_crc': HERE.parent / 'projectile_oracle/flat_art.py',
                   })
