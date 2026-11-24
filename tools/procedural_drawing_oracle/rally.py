"""Original Tactical6DA9D0 rally producer and DSurface4C0750 RGB565 pixels.

The original GetCoords, HasRallyPoint, terrain query, projection, clipping and
patterned pixel bodies execute. Prepared selection, object/type/House fields,
map cells and ABuffer are explicit boundaries, not a whole native Scenario.
DirectDraw locking is replaced by original BSurface memory-surface methods;
the original DSurface patterned draw slot remains installed in the fixture.
"""

from pathlib import Path
import hashlib
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP,
                              UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EDX,
                              UC_X86_REG_ESI, UC_X86_REG_ESP)

from tools.native_oracle import finish_vectors, provenance, run_checked, RET_MAGIC
from tools.rules_oracle.bridge_child_sound import sections
from tools.spatial_oracle.bridge_damage_admission import (
    base, call, words, MEM, TABLE, SP,
)
from tools.spatial_oracle.engineer_bridge_cursor_caller import (
    HutTypeReader, READER_INI, READER_SP,
)

SIZE = (160, 120)
SURFACE, VTABLE, PIXELS, ALPHA, ABUFFER = (MEM + n for n in (
    0x30000, 0x30100, 0x40000, 0x50000, 0x30200))
ALPHA_SURFACE = MEM + 0x30300
TACTICAL, HOUSE, BUILDING, TYPE, SELECTED = (MEM + n for n in (
    0x10000, 0x12000, 0x19000, 0x1A000, 0x1C000))
BUILDING_VTABLE, TARGET = MEM + 0x1D000, MEM + 0x1E000
BACKGROUND = 0x39E7


def stock_type_physical():
    """Original readers parse these strings; recorded paths use portable slashes."""
    inputs = {}
    for kind, filename, keys in (
        ('rules', 'rulesmd.ini', ('Image', 'Factory', 'UnitRepair', 'Cloning')),
        ('art', 'artmd.ini', ('Foundation',)),
    ):
        path = Path('ini') / filename
        raw = path.read_bytes()
        section = sections(raw)['GAPILE']
        inputs[kind] = dict(path=path.as_posix(), bytes=len(raw),
            sha256=hashlib.sha256(raw).hexdigest(), section='GAPILE',
            keys={key: section.get(key) for key in keys})
    return inputs


def stock_type_inputs():
    """Stock GAPILE prerequisites, separate from prepared drawing vectors.

    Reuse the whole type-constructor/INI-cache owner. Each reader range includes
    the original field store; interleaved pushes for the following reader are
    retained and their stack deltas checked. This does not run whole ReadINI,
    physical INI loading, layered Scenario rules, placement or construction AI.
    """
    physical = stock_type_physical()
    m = HutTypeReader()
    typ = m.construct('GAPILE')
    u = m.u

    def fields():
        return dict(type_id=m.string(typ + 0x24), image=m.string(typ + 0x1F8),
            factory=m.read32(typ + 0xEB8), foundation=m.read32(typ + 0xEF0),
            unit_repair=u.mem_read(typ + 0x16A9, 1)[0],
            cloning=u.mem_read(typ + 0x16AC, 1)[0])

    constructor = fields()
    constructor_seams = sorted({row['pc'] for row in m.trace
                                if row['kind'] == 'fixture_seam'
                                and row['phase'] == 'constructor_GAPILE'})
    reads = []

    def observe(_u, pc, _size, _data):
        if pc not in (0x474FF0, 0x5295F0, 0x474DA0, 0x528A10):
            return
        if pc == 0x528A10 and m.phase != 'image':
            return  # The outer typed readers record the relevant default.
        sp = u.reg_read(UC_X86_REG_ESP)
        row = dict(phase=m.phase, reader=f'0x{pc:08X}',
                   section=m.string(m.read32(sp + 4)),
                   key=m.string(m.read32(sp + 8)))
        if pc == 0x528A10:
            row.update(default=m.string(m.read32(sp + 12)),
                       capacity=m.read32(sp + 20), destination_offset='0x1F8')
        else:
            raw = m.read32(sp + 12)
            row.update(default=(raw & 0xFF) if pc == 0x5295F0 else raw,
                       raw_default_hex=f'0x{raw:08X}')
        reads.append(row)

    u.hook_add(UC_HOOK_CODE, observe)
    m.make_ini({'GAPILE': {key: value for key, value in physical['rules']['keys'].items()
                         if value is not None}})
    slices = []
    for name, begin, end, stack_delta, required in (
        ('image', 0x5F92F9, 0x5F9340, -4, (0x528A10,)),
        ('factory', 0x46051A, 0x46054B, -16, (0x474FF0, 0x40DCE0)),
        ('unit_repair', 0x460906, 0x46092F, -12, (0x5295F0,)),
        ('cloning', 0x46094E, 0x46097D, -12, (0x5295F0,)),
        ('foundation', 0x461225, 0x46125D, 0, (0x474DA0, 0x528A10)),
    ):
        if name == 'foundation':
            assert fields()['image'] == physical['art']['section']
            m.make_ini({'GAPILE': {key: value for key, value in physical['art']['keys'].items()
                                 if value is not None}})
        m.phase = name
        m.trace = []
        before, first_read = fields(), len(reads)
        u.mem_write(READER_SP, words(RET_MAGIC))
        # Image's supplied interior caller frame carries the rules INI argument.
        u.mem_write(READER_SP + 0x1B4, words(READER_INI))
        u.reg_write(UC_X86_REG_ESP, READER_SP)
        u.reg_write(UC_X86_REG_EBP, typ)
        u.reg_write(UC_X86_REG_EBX, typ if name == 'image' else typ + 0x24)
        u.reg_write(UC_X86_REG_ESI, READER_INI)
        u.reg_write(UC_X86_REG_EDI, typ + 0x1F8)
        # Cloning's prefix stores the preceding unrelated reader's AL at16AB.
        # Preserve that constructor byte; its reader is outside this slice.
        u.reg_write(UC_X86_REG_EAX, u.mem_read(typ + 0x16AB, 1)[0])
        run_checked(u, begin, end, count=200000, required_addresses=required)
        assert u.reg_read(UC_X86_REG_ESP) == READER_SP + stack_delta
        assert not any(row['kind'] == 'fixture_seam' for row in m.trace)
        m.unchanged()
        code = bytes(u.mem_read(begin, end - begin))
        slices.append(dict(name=name, begin=f'0x{begin:08X}', end_exclusive=f'0x{end:08X}',
            bytes_hex=code.hex(), sha256=hashlib.sha256(code).hexdigest(),
            stack_delta=stack_delta, before=before, reads=reads[first_read:], after=fields()))
    width = m.invoke(0x45EC90, typ)
    height = m.invoke(0x45ECA0, typ, (0,))
    building = m.alloc(0x530)
    u.mem_write(building + 0x520, words(typ))
    has_rally = bool(m.invoke(0x455DA0, building) & 0xFF)
    m.unchanged()
    return dict(type_id='GAPILE', physical_files=physical, constructor=constructor,
                constructor_fixture_seams=constructor_seams, reader_slices=slices,
                result=dict(**fields(), foundation_size=[width, height], has_rally_point=has_rally),
                text_unchanged=True, text_sha256=m.original)


def production_crop_inputs():
    """Prepared native state from the two pinned release capture receipts.

    Keep the existing surface arena. At zoom1, native camera is VERA camera
    plus crop origin minus the established15px world-Y bias. All three full
    rows fit; the crop does not change clipping or patterned-line phase.
    """
    common = dict(frame=1500, source=[8064, 21888, 416], target_cell=[35, 89],
        level=4, slope=0, flags=0, foundation=5, factory=16,
        unit_repair=False, cloning=False, alive=True, selected=True, enemy=False,
        rgb=[240, 248, 64], alpha='clear', camera=[-1685, 1707],
        production_camera=[-2100, 1426], crop_origin=[415, 296],
        production_surface_size=[800, 600], world_y_bias=15, production_zoom=1,
        production_owner='VERA-OBSERVER', production_actor_id=1477,
        production_cell=[31, 85], production_color_priority=0,
        color_scheme_name='Gold')
    return [dict(common, name='captured_gapile_clear_target', no_target=False,
                 capture_reference='rally-clear-v2'),
            dict(common, name='captured_gapile_clear_no_target', no_target=True,
                 capture_reference='rally-stop-v2')]


def production_crop_reference():
    """Snapshot identities, not runtime dependencies on ignored capture files."""
    return dict(
        captures={
            'rally-clear-v2': dict(
                profile=dict(path='logs/procedural-drawing/rally-clear-v2/profile.json',
                    source_path='tools/map_observation.rally-drawing.example.json',
                    bytes=5621, sha256='479a15285c4a2c4406c603cc1a5f267312329e09d2cca1876ccfea9e585a9318'),
                receipt=dict(path='logs/procedural-drawing/rally-clear-v2/child-output/capture.json',
                    bytes=20099715, sha256='77a2ff4dfee9a68457f7dffb3a32ed1ede67ec71595c98734570d3ca63502851'),
                run=dict(path='logs/procedural-drawing/rally-clear-v2/run.json',
                    bytes=20096382, sha256='125c833508911cd0b72aec5b0a6297f27f60647bb5b13b09dd206362d37f6adf'),
                frame=dict(path='logs/procedural-drawing/rally-clear-v2/child-output/frame.bgra',
                    bytes=1920000, sha256='2c0d473797443e112299954f78f165313ff2b5085eee29741b4e0419cfa800df'),
                observed_archive={'Cell': [35, 89]}),
            'rally-stop-v2': dict(
                profile=dict(path='logs/procedural-drawing/rally-stop-v2/profile.json',
                    bytes=5781, sha256='e16d3b457dc087940f647a8d45d741dfe805692f46a8c18d3990b16d9491d862'),
                receipt=dict(path='logs/procedural-drawing/rally-stop-v2/child-output/capture.json',
                    bytes=20099843, sha256='5f3bf464dd02f8d3d848b55d844136f8ba569571512a8515b63a24feaee6e8a9'),
                run=dict(path='logs/procedural-drawing/rally-stop-v2/run.json',
                    bytes=20096417, sha256='be48386482d94197cd094fb298addee1d15300292d440c20ef03e0d6b2a58cb6'),
                frame=dict(path='logs/procedural-drawing/rally-stop-v2/child-output/frame.bgra',
                    bytes=1920000, sha256='e78f4e0027aadad78f2322071f644465a88ed960b7c2f56b4857624d8cf45dd7'),
                observed_archive=None, stop_issue_after_step=1490),
        },
        release_executable=dict(label='procedural-rally-production-v2', bytes=31708240,
            sha256='edb373c72f58c45a6b79cdd4d74d7923425efecb5c03e70cbb56f52f3ae45e31'),
        map_source=dict(logical_name='XMP03T4.MAP', archive='multimd.mix', bytes=156269,
            sha256='7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e'),
        capture_state=dict(status='COMPLETE', run_status='VALID', binary_frame=1500,
            simulation_tick=1500, surface_format='Bgra8UnormSrgb', pixel_layout='BGRA8',
            row_stride=3200, gpu_backend='Metal', gpu_name='Apple M4',
            camera_input_idle=True, static_default_cursor=True,
            loaded_shroud=False, loaded_fog_of_war=False,
            source_cell=dict(cell=[31, 85], allocated=True, level=4, slope=0,
                raw_bridge_flags=0, has_deck=False, tile=[293, 2]),
            target_cell=dict(cell=[35, 89], allocated=True, level=4, slope=0,
                raw_bridge_flags=0, has_deck=False, tile=[493, 0]),
            producer_type=dict(type_id='GAPILE', interned_id=275,
                active=True, in_limbo=False, actually_placed=True)),
        prerequisites=dict(
            type='rally.json stock_type_inputs: original GAPILE constructor/readers establish resolved Image GAPILE, Factory16, Foundation5/3x2, UnitRepair0 and Cloning0',
            color='house_color.json colors[Gold]: physical Colors Gold=43,239,255; original scalar/CMOV/MMX N53 row26 and House50B840/campaign extraction give RGB240,248,64 and RGB5650xF7C8. Profile color priority0 is supplied; the Rust production comparison must establish its scheme lookup.',
            stop='factory_destination.json: original Stop destination/ArchiveTarget consumer evidence. These two drawing rows begin after the supplied Archive state, not at Stop command execution.',
            camera='native_crop_camera = production_camera + crop_origin - [0,15] at integer zoom1. Existing native projection producer owns coordinates; no raster was fitted to the screenshot.'),
        limits=[
            'The receipts establish VERA production actor, terrain, camera, binary frame, loaded options and command history. They do not establish native Scenario loading, building placement/construction, selection generation, SetRally timing or reveal history.',
            'A127 is explicitly supplied for this disabled-shroud/fog view; no A plane is extracted from the release readback. House56F9 and type fields are supplied from the separate native prerequisite corpora, not serialized by the capture receipt.',
            'Both native rows execute existing Rally6DA9D0/4C0750 with its original GetCoords, terrain, projection, clipping and raster leaves. The targetless row observes Archive=null from the Stop capture but does not execute Stop or whole native gameplay.',
            'The constant native background is only a write-mask sentinel. Native draw order puts clear rally before object bodies, so replaying the writes onto a final Stop frame alone does not establish occlusion. Screenshot comparison must separately declare its unobstructed floor region and preserve packed RGB565 versus BGRA display conversion distinctions.',
        ])


def ints(u, pointer, count):
    return list(struct.unpack('<' + 'i' * count, u.mem_read(pointer, count * 4)))


class Rally:
    def __init__(self, case, *, size=SIZE):
        self.case = case
        if len(size) != 2 or any(type(n) is not int or n <= 0 for n in size):
            raise ValueError('surface size must contain two positive integer dimensions')
        self.size = tuple(size)
        w, h = self.size
        self.pixel_bytes = w * h * 2
        # Existing color/A reservations are64KiB each; retain32bytes for the
        # action fixture's end guard. A ends before the paired-actor arena.
        # Optional160x160 crops fit without relocating or duplicating a surface.
        if self.pixel_bytes + 32 > 0x10000:
            raise ValueError('surface exceeds the existing guarded64KiB plane reservation')
        self.u = u = base({'flags': 0, 'level': 0})
        # Original BSurface storage/locking, replacing only the patterned slot
        # whose BSurface implementation is a no-op. No instruction is patched.
        u.mem_write(VTABLE, bytes(u.mem_read(0x7E2070, 0x84)))
        u.mem_write(VTABLE + 0x4C, words(0x4C0750))
        u.mem_write(SURFACE, words(VTABLE, w, h, 0, 2, PIXELS, w * h * 2, 0))
        u.mem_write(ALPHA_SURFACE, words(0x7E2070, w, h, 0, 2, ALPHA, w * h * 2, 0))
        u.mem_write(ABUFFER, words(0, 0, w, h, 0, ALPHA_SURFACE, ALPHA,
                                 ALPHA + w * h * 2, w * h * 2, 32768, w))
        u.mem_write(0x87E8A4, words(ABUFFER))
        u.mem_write(0x887314, words(SURFACE))
        for pointer, value in ((0x8A0DD0, 11), (0x8A0DD4, 3),
                               (0x8A0DD8, 0), (0x8A0DDC, 3),
                               (0x8A0DE0, 5), (0x8A0DE4, 2)):
            u.mem_write(pointer, words(value))
        call(u, 0x4E8120)  # Original default radar background RGB initializer.
        u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
        u.mem_write(0xB0CEB4, words(416))
        u.mem_write(0x89E7C0, words(104))
        self.clip = case.get('clip', [0, 0, w, h])
        self.camera = case.get('camera', [-320, 440])
        u.mem_write(0x886FA0, words(*self.clip))
        u.mem_write(0xB0CE30, words(w, h))
        u.mem_write(TACTICAL + 0xB0, words(*self.camera))
        u.mem_write(0x887324, words(TACTICAL))
        self.alpha = [self.alpha_at(x, y) for y in range(h) for x in range(w)]
        u.mem_write(ALPHA, struct.pack('<' + 'H' * len(self.alpha), *self.alpha))
        u.mem_write(PIXELS, struct.pack('<H', BACKGROUND) * (w * h))
        self.draws = []
        self.clips = []
        u.hook_add(UC_HOOK_CODE, self.observe)

    def alpha_at(self, x, y):
        kind = self.case.get('alpha', 'clear')
        if kind == 'clear':
            return 127
        if kind == 'black':
            return 0
        return (0, 1, 63, 127, 255)[(x // 9 + y // 7) % 5]

    def observe(self, u, address, _size, _data):
        if address == 0x7BC2B0:
            self.clips.append(dict(from_point=ints(u, u.reg_read(UC_X86_REG_ECX), 2),
                                   to_point=ints(u, u.reg_read(UC_X86_REG_EDX), 2)))
        if address == 0x4C0750:
            args = ints(u, u.reg_read(UC_X86_REG_ESP) + 4, 6)
            self.draws.append(dict(from_point=ints(u, args[0], 2),
                to_point=ints(u, args[1], 2), color=args[2],
                pattern=list(u.mem_read(args[3], 16)), phase=args[4], pass_id=args[5]))

    def pixels(self):
        raw = bytes(self.u.mem_read(PIXELS, self.pixel_bytes))
        return [[i % self.size[0], i // self.size[0], word[0]]
                for i, word in enumerate(struct.iter_unpack('<H', raw))
                if word[0] != BACKGROUND]

    def setup_building(self):
        u, case = self.u, self.case
        u.mem_write(BUILDING_VTABLE + 0x2C, words(0x459EC0))
        u.mem_write(BUILDING_VTABLE + 0x48, words(0x447AC0))
        u.mem_write(BUILDING_VTABLE + 0x284, words(0x455DA0))
        u.mem_write(BUILDING, words(BUILDING_VTABLE))
        u.mem_write(BUILDING + 0x520, words(TYPE))
        u.mem_write(TYPE + 0xEB8, words(case.get('factory', 0x28)))
        u.mem_write(TYPE + 0x16A9, bytes([case.get('unit_repair', False)]))
        u.mem_write(TYPE + 0x16AC, bytes([case.get('cloning', False)]))
        u.mem_write(TYPE + 0xEF0, words(case.get('foundation', 0)))
        u.mem_write(BUILDING + 0x90, bytes([case.get('alive', True)]))
        u.mem_write(BUILDING + 0x83, bytes([case.get('selected', True)]))
        u.mem_write(BUILDING + 0x9C, words(*case.get('source', [2688, 5248, 0])))
        u.mem_write(BUILDING + 0x218, words(0 if case.get('no_target') else TARGET))
        u.mem_write(BUILDING + 0x21C, words(HOUSE))
        u.mem_write(0xA83D4C, words(HOUSE + int(case.get('enemy', False)) * 4))
        u.mem_write(HOUSE + 0x56F9, bytes(case.get('rgb', [248, 40, 8])))
        u.mem_write(SELECTED, words(BUILDING))
        u.mem_write(0xA8ECBC, words(SELECTED))
        u.mem_write(0xA8ECC8, words(1))
        x, y = case.get('target_cell', [14, 20])
        u.mem_write(TARGET, words(0x7E4EEC))
        u.mem_write(TARGET + 0x24, struct.pack('<hh', x, y))
        u.mem_write(TARGET + 0x11B, bytes([case.get('level', 0), case.get('slope', 0)]))
        u.mem_write(TARGET + 0x140, words(case.get('flags', 0)))
        u.mem_write(TABLE + (y * 512 + x) * 4, words(TARGET))
        if 'target_object' in case:
            cell = MEM + 0x1F000
            u.mem_write(cell, bytes(u.mem_read(TARGET, 0x148)))
            u.mem_write(TABLE + (y * 512 + x) * 4, words(cell))
            target_vtable = MEM + 0x1F800
            u.mem_write(target_vtable + 0x48, words(0x5F65A0))
            u.mem_write(TARGET, words(target_vtable))
            u.mem_write(TARGET + 0x9C, words(*case['target_object']))
        u.mem_write(0xA8ED84, words(case.get('frame', 0)))

    def producer(self):
        self.setup_building()
        width = call(self.u, 0x45EC90, TYPE)
        height = call(self.u, 0x45ECA0, TYPE, (0,))
        source = MEM + 0x31800
        call(self.u, 0x447AC0, BUILDING, (source,))
        source_coords = ints(self.u, source, 3)
        return dict(input=self.case, size=self.size, clip=self.clip, camera=self.camera,
                    foundation_size=[width, height], source_coords=source_coords,
                    passes=self.draw_passes())

    def draw_passes(self):
        output = []
        for pass_id in (0, 1):
            self.draws = []
            self.clips = []
            self.u.mem_write(SP, words(RET_MAGIC, pass_id))
            self.u.reg_write(UC_X86_REG_ESP, SP)
            self.u.reg_write(UC_X86_REG_ECX, TACTICAL)
            run_checked(self.u, 0x6DA9D0, RET_MAGIC, count=250000,
                        context={'case': self.case, 'pass': pass_id})
            assert self.alpha == list(struct.unpack('<' + 'H' * len(self.alpha),
                self.u.mem_read(ALPHA, len(self.alpha) * 2))), 'rally changed ABuffer'
            output.append(dict(pass_id=pass_id, draws=self.draws.copy(),
                               clips=self.clips.copy(), pixels=self.pixels()))
        return output

    def producer_pair(self):
        self.setup_building()
        u = self.u
        second = MEM + 0x60000
        buildings = [BUILDING, second]
        u.mem_write(second, bytes(u.mem_read(BUILDING, 0x1000)))
        for pointer, source in zip(buildings, self.case['sources']):
            u.mem_write(pointer + 0x9C, words(*source))
        order = self.case['selection_order']
        u.mem_write(SELECTED, words(*[buildings[i] for i in order]))
        u.mem_write(0xA8ECC8, words(len(buildings)))
        width = call(u, 0x45EC90, TYPE)
        height = call(u, 0x45ECA0, TYPE, (0,))
        source = MEM + 0x31800
        source_coords = []
        for pointer in buildings:
            call(u, 0x447AC0, pointer, (source,))
            source_coords.append(ints(u, source, 3))
        queries = []

        def observe(_u, address, _size, _data):
            if address == 0x447AC0:
                queries.append(buildings.index(u.reg_read(UC_X86_REG_ECX)))

        hook = u.hook_add(UC_HOOK_CODE, observe)
        passes = self.draw_passes()
        u.hook_del(hook)
        assert queries == list(reversed(order)) * 2
        return dict(input=self.case, size=self.size, clip=self.clip, camera=self.camera,
                    foundation_size=[width, height], source_coords=source_coords,
                    factory_order_by_pass=[queries[:2], queries[2:]], passes=passes)

    def leaf(self):
        case = self.case
        a, b = MEM + 0x31000, MEM + 0x31010
        self.u.mem_write(a, words(*case['from_point']))
        self.u.mem_write(b, words(*case['to_point']))
        self.u.mem_write(SP, words(RET_MAGIC, a, b, 0xF941, 0x842930,
                                  case['phase'], case['pass_id']))
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ECX, SURFACE)
        run_checked(self.u, 0x4C0750, RET_MAGIC, count=250000, context={'case': case})
        return dict(input=case, pixels=self.pixels())


def generate():
    producers = [dict(name=f'frame_{frame}', frame=frame, alpha='mixed')
                 for frame in (*range(17), 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF)]
    producers += [dict(name='gate_' + key, **{key: value}) for key, value in (
        ('alive', False), ('selected', False), ('enemy', True),
        ('no_target', True), ('factory', -1))]
    producers += [dict(name='repair', factory=-1, unit_repair=True),
                  dict(name='cloning', factory=-1, cloning=True)]
    producers += [dict(name=f'clip_{i}', camera=camera, clip=clip, alpha='mixed')
                  for i, (camera, clip) in enumerate((
                      ([-250, 475], [0, 0, 80, 45]),
                      ([-450, 415], [9, 7, 60, 40]),
                      ([-200, 420], [0, 0, 160, 120]),
                      ([-320, 465], [0, 0, 120, 1]),
                  ))]
    producers += [dict(name=f'height_{level}_{flags}', level=level, flags=flags,
                       source=[2688, 5248, level * 104], camera=[-320, 320])
                  for level in (0, 2, 6) for flags in (0, 256)]
    producers += [dict(name=f'foundation_{f}', foundation=f, alpha='mixed')
                  for f in (1, 2, 3, 5, 6, 9, 17)]
    producers += [dict(name='alpha_' + mode, alpha=mode) for mode in ('black', 'clear')]
    producers += [dict(name=f'slope_{s}', slope=s, level=2, camera=[-320, 375])
                  for s in range(1, 17)]
    producers += [dict(name='object_target', target_object=[3768, 5217, 4096]),
                  dict(name='object_target_bridge', target_object=[3768, 5217, 4096], flags=256,
                       camera=[-320, 365]),
                  dict(name='infantry_factory', factory=16)]
    leaves = []
    for delta in ((0, 0), (48, 0), (0, 48), (48, 48), (48, 17), (17, 48),
                  (48, -17), (17, -48), (32, 16), (16, 32)):
        for reverse in (False, True):
            start, end = [50, 60], [50 + delta[0], 60 + delta[1]]
            if reverse:
                start, end = end, start
            for phase in (0, 1, 4, 7, 8, 14, 15, -1, -14):
                for pass_id in (0, 1):
                    leaves.append(dict(from_point=start, to_point=end, phase=phase,
                                       pass_id=pass_id, alpha='mixed'))
    clipping_pairs = [dict(name=f'overlap_{order[0]}{order[1]}_{alpha}',
                  frame=5, alpha=alpha, selection_order=list(order),
                  sources=[[2688, 5248, 0], [2688, 5504, 0]])
             for order in ((0, 1), (1, 0)) for alpha in ('clear', 'mixed')]
    # Keep the original clipped controls as native evidence. Their first
    # endpoint exposes the known presentation-only chop53 versus host-f64
    # rounding difference; the distinct ordering controls stay fully in bounds.
    pairs = [dict(case, name=case['name'] + '_inbounds', camera=[-335, 440])
             for case in clipping_pairs]
    crops = [Rally(case).producer() for case in production_crop_inputs()]
    for row in crops:
        for draw_pass in row['passes']:
            for draw in draw_pass['draws']:
                assert all(0 <= p[0] < SIZE[0] and 0 <= p[1] < SIZE[1]
                           for p in (draw['from_point'], draw['to_point']))
            if row['input']['no_target']:
                assert not draw_pass['draws'] and not draw_pass['pixels']
    return dict(source='unicorn/gamemd.exe', background=BACKGROUND, size=SIZE,
                pattern=list(Rally({}).u.mem_read(0x842930, 16)),
                stock_type_inputs=stock_type_inputs(),
                producer_cases=[Rally(case).producer() for case in producers],
                multi_producer_cases=[Rally(case).producer_pair() for case in pairs],
                clipping_rounding_controls=[Rally(case).producer_pair() for case in clipping_pairs],
                production_crop_cases=crops,
                leaf_cases=[Rally(case).leaf() for case in leaves])


def metadata():
    result = provenance(
        scope='Original rally producer, projection/clipping and patterned RGB565 writes, stock GAPILE constructor/selected physical RULESMD/ARTMD readers, and two prepared crops from pinned release rally/Stop captures; prepared selection/map/House/ABuffer boundaries',
        assumptions=['Original binary, FPCW0E7F, RGB565; 160x120 surface with fixed initial destination',
            'Native selected-object vector, object/type fields, House RGB, Camera and map cell state supplied; no selection/rally command or Scenario execution',
            'Original default-black initializer and Building GetCoords/HasRallyPoint execute; foundation indices and factory enums are explicit fixture inputs; width/height getters and source coordinate return values are recorded',
            'ABuffer values0,1,63,127,255 expose complementary passes; no native shroud producer in this corpus',
            'Signed binary-frame wrap and clipped row-to-row mutation included; native Z is untouched by this mechanism',
            'Four in-bounds two-factory controls at camera[-335,440] preserve reverse selection and per-object black/color/color row order, with overlapping lines, both passes and clear/mixed ABuffer; native selection production is still supplied',
            'The original four camera[-320,440] overlap payloads are retained verbatim in clipping_rounding_controls. First row[-10,42]->[140,87] becomes[0,44]->[140,87] under original7BC2B0: chop53 produces44.99999999999999 before integer truncation. VERA host-nearest f64 produces45, a bounded one-pixel presentation residual; moving the separate ordering controls in bounds does not claim clipping equivalence',
            'stock_type_inputs independently executes whole BuildingType45DD90 for GAPILE, then original Image5F92F9..5F9340, Factory46051A..46054B, UnitRepair460906..46092F, Cloning46094E..46097D and Foundation461225..46125D reader/store ranges. It records constructor defaults, exact reader arguments, before/after fields, code bytes and unchanged text. Width45EC90, Height45ECA0(0) and HasRallyPoint455DA0 consume the resulting native fields.',
            'Physical base RULESMD/ARTMD file identities and literal selected key strings are pinned below. Existing lexical extraction supplies raw strings to the existing native CRC/INI-cache owner; original readers own numeric/enum/bool/Foundation parsing. Full physical INI loading, LANGRULE/mode/map layers and complete ReadINI are excluded. GAPILE has no Image override; its actual ART Foundation is3x2 (enum5), not2x3 (enum4).',
            'The stock reader slices receive prepared interior frames. The Image slice includes the original retained-image default copy and25-byte ReadString. Interleaved pushes for the next reader remain and stack deltas are recorded. The Cloning prefix receives AL equal to the preceding unrelated field16AB constructor byte, preserving it while that reader remains outside this slice.',
            'The stock type result does not establish Building object placement/physical Location, selection production, ArchiveTarget command timing, terrain loading, House color, A generation, object occlusion or a native full Scenario.',
            'production_crop_cases execute the existing Rally owner with prepared state from the pinned valid release rally/Stop receipts below. Surface storage stays160x120; native crop camera equals production camera plus crop origin minus[0,15] at zoom1. All three full rows stay in bounds; targetless Archive=null draws nothing. Pixel comparison with the production captures is separate from this native corpus generation.'],
        substitutions=['Original BSurface storage/lock/pitch helpers replace platform DirectDraw surface access; slot4C remains original4C0750',
            'The existing HutTypeReader constructor fixture supplies bump allocation at7C8E17, no-op delete at7C8B3D and CRT TLS storage at7D140B. Reached constructor seams are recorded; no such seam or physical asset lookup is admitted during selected type reader slices.'],
        entry_points={'rally':0x6DA9D0,'patterned_line':0x4C0750,'clip':0x7BC2B0,
                      'source_coords':0x447AC0,'rally_gate':0x455DA0,'black_init':0x4E8120,
                      'building_type_ctor':0x45DD90,'image_reader':0x528A10,
                      'factory_reader':0x474FF0,'factory_name_decoder':0x40DCE0,
                      'bool_reader':0x5295F0,'foundation_reader':0x474DA0,
                      'foundation_width':0x45EC90,'foundation_height':0x45ECA0})
    result['stock_type_physical_inputs'] = stock_type_physical()
    result['production_crop_reference'] = production_crop_reference()
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
        source_paths={'producer':Path(__file__),
            'fixture_base':Path('tools/spatial_oracle/bridge_damage_admission.py'),
            'type_reader_fixture':Path('tools/spatial_oracle/engineer_bridge_cursor_caller.py'),
            'ini_registry_fixture':Path('tools/rules_oracle/bridge_anim_inputs.py'),
            'allocation_fixture':Path('tools/rules_oracle/bridge_anim_lists.py'),
            'ini_fixture':Path('tools/spatial_oracle/building_body_rules.py'),
            'crc_fixture':Path('tools/projectile_oracle/flat_art.py'),
            'lexical_fixture':Path('tools/rules_oracle/bridge_child_sound.py'),
            'native_oracle':Path('tools/native_oracle.py')})
