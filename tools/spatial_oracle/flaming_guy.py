"""Physical FLAMEGUY: original cell admission, destination and running AI.

Import and --help are inert. VERA20K_FLAMING_GUY_ASSETS names external,
unchanged ARTMD.INI, RULESMD.INI and selected SHPs; see flaming_guy.md.
No expected gameplay answer or VERA-produced terrain result enters the VM.
"""
from pathlib import Path
import os
import struct
import tempfile


AI, RUN, NEXT, ENTER = 0x423AC0, 0x425670, 0x425D10, 0x4260F0
MAP, CENTER = 0x87F7E8, (32, 32)


def physical_inputs():
    from tools.projectile_oracle.bridge_render_inputs import lexical
    root = Path(os.environ['VERA20K_FLAMING_GUY_ASSETS'])
    names = ('ARTMD.INI', 'RULESMD.INI', 'FLAMEGUY.SHP', 'H2O_EXP3.SHP')
    assets = {name: (root / name).read_bytes() for name in names}
    art, _ = lexical(assets['ARTMD.INI'], {'FLAMEGUY', 'H2O_EXP1', 'H2O_EXP2', 'H2O_EXP3'})
    rules, _ = lexical(assets['RULESMD.INI'], {'CombatDamage'})
    return assets, art, rules


class Probe:
    """One fresh Reader VM with original constructors and sparse scene inputs."""
    def __init__(self, assets, art, rules, case):
        from unicorn import UC_HOOK_CODE
        from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP
        from tools import native_oracle as native
        from tools.native_slope import initialize_slope_tables
        from tools.rules_oracle.bridge_anim_inputs import Reader
        from tools.spatial_oracle.bridge_constructor import OriginalBridgeConstructor
        from tools.spatial_oracle.building_body_rules import INI, SP, dwords
        from tools.spatial_oracle.trigger_action_values import initialize_discovery_geometry
        self.native, self.SP, self.words = native, SP, dwords
        # Reader normally loads every supplied directory member. An empty
        # transport directory bounds this composition to exactly four files.
        with tempfile.TemporaryDirectory(prefix='flaming-guy-reader-') as empty:
            self.m = Reader(Path(empty), art, arena_size=0x1000000)
        m, u = self.m, self.m.u
        self.u, self.case = u, case
        m.assets.update(assets)
        self.events, self.pending, self.tick = [], {}, 0
        self.phase = 'setup'
        self.original = bytes(u.mem_read(0x401000, 0x3E0000))
        self.vtables = {a: bytes(u.mem_read(a, n)) for a, n in
                        ((0x7E3354, 0x210), (0x7E3608, 0xA4), (0x7E4EEC, 0x80))}
        # This translation unit owns its own level/deck/sentinel copies.
        # Execute the original 13-entry startup group before any Anim body.
        self.anim_startup = [m.read32(0x81252C + 4 * index) for index in range(13)]
        for entry in self.anim_startup:
            m.invoke(entry, 0)
        self.anim_constants = dict(level=self.signed(0x89A1C0),
            deck=self.signed(0x89A1B4),
            zero_cell=list(struct.unpack('<hh', u.mem_read(0x89A150, 4))),
            zero_coord=list(struct.unpack('<iii', u.mem_read(0x89A178, 12))))
        assert self.anim_constants == dict(level=104, deck=416,
                                           zero_cell=[0, 0], zero_coord=[0, 0, 0])
        self.geometry = initialize_discovery_geometry(m.invoke, m.read32, u)
        initialize_slope_tables(u, SP, native.RET_MAGIC)
        self.rules = m.alloc(0x5000)
        u.mem_write(0x8871E0, dwords(self.rules))
        m.invoke(0x665650, self.rules)
        OriginalBridgeConstructor.empty_registries(u, m.alloc, capacity=64,
                                                   preserve=(0xB0F670,))
        # Logic's original static header constructor, as in
        # building_construction.JoinedFixture. Stop before its CRT atexit
        # registration, then supply spare storage. Without this prior the
        # original RegisterObject fails its zero-capacity/zero-growth gate;
        # AnimClass::Array membership would not establish Logic admission.
        u.reg_write(UC_X86_REG_ESP, SP)
        native.run_checked(u, 0x40CB80, 0x40CBB6, count=100)
        u.mem_write(0x87F77C, dwords(m.alloc(256), 64))
        # Existing Ground-membership owner's exact Display-array prefix;
        # the unrelated trailing CRT atexit call remains outside this seam.
        u.reg_write(UC_X86_REG_ESP, SP)
        native.run_checked(u, 0x4A8630, 0x4A866D, count=100)
        for layer in range(5):
            u.mem_write(0x8A0360 + layer * 24 + 4, dwords(m.alloc(256), 64))
        u.mem_write(0xA8E9A8, dwords(0x7E9F24, m.alloc(256), 64, 1, 0, 10))
        u.mem_write(0xA8E9A0, b'\1')
        u.mem_write(0xA8E7AC, dwords(0))
        u.mem_write(0xA8ED84, dwords(0))
        u.mem_write(0xA8EB60, dwords(4))
        self.scenario = m.alloc(0x5000)
        u.mem_write(0xA8B230, dwords(self.scenario))
        for stream in (self.scenario + 0x218, 0x886B88, 0xABE890):
            m.invoke(0x65C6D0, stream, (case.get('seed', 1),))
        # Shared map-query geometry: original methods consume the supplied
        # Size/LocalSize, sparse pointer table and already-allocated raw caches.
        table = m.alloc(512 * 512 * 4)
        u.mem_write(MAP + 0x13C, dwords(table, 512 * 512))
        u.mem_write(MAP + 0xF4, dwords(48, 48, 0, 0, 48, 48))
        count = 97 * 97
        plane, levels = m.alloc(count * 4), m.alloc(count * 10)
        u.mem_write(plane, b'\7\0\0\0' * count)
        u.mem_write(MAP + 0x68, dwords(plane, count, levels))
        self.cells = {}
        m.invoke(0x47BBF0, 0xABDC50)
        for y in range(16, 49):
            for x in range(16, 49):
                p = m.alloc(0x148)
                m.invoke(0x47BBF0, p)
                u.mem_write(p + 0x24, struct.pack('<hh', x, y))
                u.mem_write(p + 0x44, dwords(-1))
                u.mem_write(p + 0x11B, bytes([0, 0]))
                u.mem_write(p + 0x12C, dwords(0x18))
                u.mem_write(p + 0x130, dwords(1))
                u.mem_write(table + (y * 512 + x) * 4, dwords(p))
                self.cells[x, y] = p
        self.configure_cells(case)
        # Original constructor defaults and full ART readers own FLAMEGUY's
        # IsFlamingGuy, RunningFrames, image and endpoint values.
        self.types = {}
        for name in ('FLAMEGUY', 'H2O_EXP3', 'H2O_EXP2', 'H2O_EXP1'):
            self.types[name] = m.invoke(0x428B80, m.cstring(name))
        self.readers = []
        for name, typ in self.types.items():
            admitted = m.invoke(0x427D00, typ, (INI,))
            self.readers.append(dict(name=name, admitted=bool(admitted & 255),
                image_present=bool(m.read32(typ + 0xA4)),
                raw_frames=(struct.unpack('<h', u.mem_read(m.read32(typ + 0xA4) + 6, 2))[0]
                            if m.read32(typ + 0xA4) else 0),
                running_frames=self.signed(typ + 0x350),
                is_flaming_guy=bool(u.mem_read(typ + 0x354, 1)[0]),
                end=self.signed(typ + 0x2C0), loop_end=self.signed(typ + 0x2BC),
                rate=self.signed(typ + 0x2B0),
                start_sound=self.signed(typ + 0x2F8),
                stop_sound=self.signed(typ + 0x2FC)))
        assert self.readers[0]['admitted'] and self.readers[0]['image_present']
        assert self.readers[0]['is_flaming_guy']
        # Same exact SplashList reader block as bridge_landing_inputs.py.
        # The whole original Rules ctor initialized its destination vector.
        m.make_ini(rules)
        from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, self.rules)
        u.reg_write(UC_X86_REG_EDI, INI)
        native.run_checked(u, 0x66C184, 0x66C287, count=2_000_000)
        assert u.reg_read(UC_X86_REG_ESP) == SP
        vector, n = m.read32(self.rules + 0xBC4), m.read32(self.rules + 0xBD0)
        self.splashes = [m.string(m.read32(vector + 4 * i) + 0x24) for i in range(n)]
        assert self.splashes and self.splashes[0] == 'H2O_EXP3'
        m.make_ini(art)
        self.anim = m.alloc(0x1C8)
        coordinate = m.alloc(12)
        u.mem_write(coordinate, dwords(32 * 256 + 128, 32 * 256 + 128, case.get('z', 0)))

        def observe(_u, pc, _size, _data):
            sp = u.reg_read(UC_X86_REG_ESP)
            key = (pc, sp)
            if key in self.pending:
                self.pending.pop(key)['result'] = self.signed_register(UC_X86_REG_EAX)
            if pc in (0x65C780, 0x65C7E0):
                row = dict(kind='ranged' if pc == 0x65C7E0 else 'next',
                    phase=self.phase, tick=self.tick, caller=f'{m.read32(sp):08X}')
                if pc == 0x65C7E0:
                    row.update(min=self.signed(sp + 4), max=self.signed(sp + 8))
                self.events.append(row)
                cleanup = 8 if pc == 0x65C7E0 else 0
                self.pending[m.read32(sp), sp + 4 + cleanup] = row
            elif pc in (0x65C79D, 0x65C84B):
                self.events.append(dict(kind='raw_rng', phase=self.phase,
                    tick=self.tick, value=u.reg_read(UC_X86_REG_ESI)))
            elif pc in (RUN, NEXT, ENTER, AI, 0x5F3E70, 0x4255B0, 0x5F65F0):
                row = dict(kind='entry', entry=f'{pc:08X}', phase=self.phase, tick=self.tick)
                if pc == ENTER:
                    row['cell'] = list(struct.unpack('<hh', u.mem_read(m.read32(sp + 4), 4)))
                self.events.append(row)
            elif pc == 0x421EA0:
                typ = m.read32(sp + 4)
                self.events.append(dict(kind='constructor', phase=self.phase, tick=self.tick,
                    type=m.string(typ + 0x24),
                    coordinate=list(struct.unpack('<3i', u.mem_read(m.read32(sp + 8), 12))),
                    delay=self.signed(sp + 12), loops=self.signed(sp + 16),
                    flags=m.read32(sp + 20), z_adjust=self.signed(sp + 24)))
        self.hook = u.hook_add(UC_HOOK_CODE, observe)
        self.phase = 'constructor'
        self.rng_before_constructor = self.rng()
        m.invoke(0x421EA0, self.anim, (self.types['FLAMEGUY'], coordinate, 0, 1, 0x600, 0, 0))
        self.constructed = self.state()
        assert self.constructed['in_logic'], 'physical constructor must admit primary to Logic'
        self.rng_after_constructor = self.rng()
        self.phase = 'control'

    def signed(self, pointer):
        return struct.unpack('<i', self.u.mem_read(pointer, 4))[0]

    def signed_register(self, register):
        value = self.u.reg_read(register)
        return struct.unpack('<i', struct.pack('<I', value & 0xFFFFFFFF))[0]

    def rng(self):
        return {name: bytes(self.u.mem_read(p, 0x3F4)).hex() for name, p in
                (('scenario', self.scenario + 0x218), ('main', 0x886B88), ('mapgen', 0xABE890))}

    def state(self):
        a = self.anim
        fields = {'frame': 0xAC, 'rate': 0xC0, 'step': 0xC4,
                  'timer_start': 0xB4, 'timer_duration': 0xBC, 'retries': 0x114}
        result = {name: self.signed(a + offset) for name, offset in fields.items()}
        result.update(location=list(struct.unpack('<3i', self.u.mem_read(a + 0x9C, 12))),
            destination=list(struct.unpack('<3i', self.u.mem_read(a + 0x108, 12))))
        result.update({name: bool(self.u.mem_read(a + offset, 1)[0]) for name, offset in
            (('alive', 0x90), ('falling', 0x8D), ('expired', 0x19A),
             ('inactive', 0x19B), ('first_ai', 0x19C))})
        # Logic singleton87F778 owns its vector and Object+98. A8E9A8 is
        # the distinct AnimClass::Array, retaining deferred physical objects.
        items, count = self.m.read32(0x87F77C), self.m.read32(0x87F788)
        assert count <= self.m.read32(0x87F780)
        result['in_logic'] = any(self.m.read32(items + 4 * index) == a
                                 for index in range(count))
        assert result['in_logic'] == bool(self.u.mem_read(a + 0x98, 1)[0]), \
            'original Logic vector and Object+98 must agree'
        result['fall_rate'] = self.signed(a + 0x2C)
        return result

    def configure_cells(self, case):
        u, words = self.u, self.words
        if case.get('blocked'):
            for p in self.cells.values():
                u.mem_write(p + 0x124, words(0xE0))
        for row in case.get('cells', []):
            p = self.cells[tuple(row['xy'])]
            for name, offset in (('land', 0xEC), ('flags', 0x140),
                                 ('ground_occupation', 0x124), ('alt_occupation', 0x128)):
                if name in row:
                    u.mem_write(p + offset, words(row[name]))
            if 'level' in row:
                u.mem_write(p + 0x11B, bytes([row['level'] & 255]))

    def execute(self):
        m, case = self.m, self.case
        before, rng_before = self.state(), self.rng()
        if case['kind'] == 'next':
            out = m.alloc(12)
            m.invoke(NEXT, self.anim, (out,))
            output = list(struct.unpack('<3i', self.u.mem_read(out, 12)))
            visits = []
        elif case['kind'] == 'enter':
            xy = tuple(case.get('target', (33, 32)))
            out = m.alloc(4)
            self.u.mem_write(out, struct.pack('<hh', *xy))
            output = bool(m.invoke(ENTER, self.anim, (out,)) & 255)
            visits = []
        else:
            output, visits = None, []
            for frame in range(case.get('max_frames', 500)):
                self.tick = frame
                self.u.mem_write(0xA8ED84, self.words(frame))
                m.invoke(AI, self.anim)
                state = self.state()
                visits.append(dict(tick=frame, **state))
                if not state['alive']:
                    break
            assert visits and not visits[-1]['alive'], 'bounded full AI did not retire the Anim'
        assert bytes(self.u.mem_read(0x401000, 0x3E0000)) == self.original
        assert all(bytes(self.u.mem_read(a, len(raw))) == raw for a, raw in self.vtables.items())
        return dict(input=case, reader=self.readers, splash_list=self.splashes,
            startup=self.anim_constants,
            constructor=self.constructed, constructor_rng_before=self.rng_before_constructor,
            constructor_rng_after=self.rng_after_constructor, before=before, after=self.state(),
            rng_before=rng_before, rng_after=self.rng(), output=output, visits=visits,
            events=self.events, guards=dict(original_code_unchanged=True, vtables_unchanged=True))


def cases():
    rows = [dict(name='next_dry_seed1', kind='next', seed=1),
            dict(name='next_dry_seed7', kind='next', seed=7),
            dict(name='next_water_bias', kind='next', cells=[
                dict(xy=[30, 32], land=2), dict(xy=[34, 33], land=2)]),
            dict(name='next_water_bridge_excluded', kind='next', cells=[
                dict(xy=[33, 32], land=2, flags=0x100)]),
            dict(name='next_blocked', kind='next', blocked=True)]
    for name, cell, z in (
        ('ground_clear', {}, 0), ('land_rock', {'land': 3}, 0),
        ('land_weeds', {'land': 10}, 0), ('land_wall', {'land': 4}, 0),
        ('ground_occupied', {'ground_occupation': 0xE0}, 0),
        ('climb_two_levels', {'level': 2}, 0), ('climb_three_levels', {'level': 3}, 0),
        ('bridge_high_clear', {'flags': 0x100, 'ground_occupation': 0xE0}, 416),
        ('bridge_high_occupied', {'flags': 0x100, 'alt_occupation': 0xE0}, 416),
        ('bridge_at_two_levels', {'flags': 0x100, 'alt_occupation': 0xE0}, 208)):
        rows.append(dict(name='enter_' + name, kind='enter', z=z,
                         cells=[dict(xy=[33, 32], **cell)]))
    rows.extend([dict(name='ai_dry_run', kind='ai', seed=1),
        dict(name='ai_water_run', kind='ai', seed=7, cells=[dict(xy=[33, 32], land=2)]),
        dict(name='ai_blocked_expiry', kind='ai', blocked=True),
        dict(name='ai_fall_dry', kind='ai', z=500),
        dict(name='ai_fall_water', kind='ai', z=500,
             cells=[dict(xy=[x, y], land=2) for y in range(30, 35) for x in range(30, 35)])])
    return rows


def generate():
    assets, art, rules = physical_inputs()
    import sys
    rows = []
    for case in cases():
        print('native FLAMEGUY: ' + case['name'], file=sys.stderr, flush=True)
        rows.append(Probe(assets, art, rules, case).execute())
    return dict(schema=1, lexical_art=art, rows=rows)


def metadata():
    from tools.native_oracle import provenance
    return provenance(scope='Fresh physical FLAMEGUY reader/constructor then original4260F0 admission,425D10 destination or bounded423AC0/425670 AI. Sparse supplied map/cell priors; no complete damage producer, map startup, sound catalog, draw or pixel claim.',
        assumptions=[
            'External ARTMD.INI/RULESMD.INI/FLAMEGUY.SHP/H2O_EXP3.SHP lexical/file inputs. Native cached-INI reader executes; physical INI/MIX loading is excluded.',
            'Original Rules/AnimType/Anim/Cell constructors, scalar readers and SplashList reader execute. Empty sound registry: sound name resolution is native under a declared absent catalog, not stock audio parity.',
            'Original thirteen-entry Anim startup group81252C initializes level104/deck416/zero sentinels; existing discovery and slope-table startup execute. Supplied Size48x48/LocalSize0,0,48,48, sparse cells16..48, level0/slope0/no overlays/default ground/alternate occupation except explicit controls; raw preallocated Map caches. No terrain classification result is supplied.',
            'Whole original SetLocation, Mark, Display, Map queries, FlamingGuyAI, destination/admission methods and ObjectAI run. Full AI histories schedule only the primary Anim. Child constructors execute, but children are not separately scheduled.',
            'Original Logic header constructor40CB80..40CBB6 precedes supplied spare storage. Native Unlimbo performs real Logic admission; observations check live Logic87F778 vector membership against Object+98, never substitute the distinct deferred AnimClass array.',
            'Original code/vtables checked unchanged; native IDs, heap pointers and clock padding are not published. No hand-computed expected XY, stage or RNG output is supplied.'],
        substitutions=['Inherited Reader file lookup returns exact named physical SHP bytes; operator new is a bounds-checked bump allocator, delete is a no-op and CRT TLS is supplied. No gameplay-return substitutions are installed by this producer.'],
        entry_points=dict(anim_ai=AI, flaming_ai=RUN, next_coord=NEXT, enter_cell=ENTER,
                          anim_ctor=0x421EA0, anim_type_reader=0x427D00,
                          splash_reader=0x66C184, object_ai=0x5F3E70,
                          logic_startup=0x40CB80, logic_register=0x55BAA0,
                          logic_unregister=0x55BAE0))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
