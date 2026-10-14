"""Original damage-smoke particle bodies over the existing effect runtime.

SparkBirth/BulletReader owns the machine, image, INI, archive and OS transport;
RngTrace observes original Random bodies. This fixture supplies prior state and
observes Smoke constructor/dispatch/system/movement instructions, not a Python
implementation of their behavior. See damage_smoke.md for admission boundaries.
"""
from functools import lru_cache
from pathlib import Path
import hashlib
import struct

from unicorn.x86_const import UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP

from tools import native_oracle as native
from tools.procedural_drawing_oracle.electric_bolt import SparkBirth, ints, signed, words
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.rules_oracle.weapon_laser import physical_layers
from tools.sidebar_oracle.stock import mix, mix_hash
from tools.spatial_oracle.building_body_rules import INI, RULES, SP
from tools.spatial_oracle.building_prism import FRAME, TEXT_BEGIN, TEXT_SIZE

SYSTEM_NAMES = ('SmallGreySSys', 'BigGreySmokeSys')
PARTICLE_NAMES = ('SmallGreySmoke', 'LargeGreySmoke')
ORIGIN = (44 * 256 + 128, 48 * 256 + 128, 0)
CALLS = {
    0x62DC50: 'system_ctor', 0x62B5E0: 'particle_ctor',
    0x62FD60: 'system_ai', 0x62ED40: 'smoke_system_ai',
    0x62CE40: 'particle_dispatch', 0x62C540: 'smoke_ai',
    0x62D5E0: 'move_dispatch', 0x62D3F0: 'move_smoke',
    0x5F65F0: 'particle_uninit', 0x5F4D30: 'limbo',
    0x5F5280: 'detach_all', 0x7258D0: 'announce_expired',
    0x7077C0: 'techno_expired', 0x62FE90: 'system_expired',
    0x62D9A0: 'particle_destructor', 0x62E070: 'system_destructor',
}


def f32_bits(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


@lru_cache(maxsize=1)
def sources():
    root = native.configured_gamemd().parent
    yr = mix((root / 'ra2md.mix').read_bytes())
    art = mix(yr[mix_hash('localmd.mix')])[mix_hash('artmd.ini')]
    ra2 = mix((root / 'ra2.mix').read_bytes())
    return physical_layers(), art, mix(ra2[mix_hash('conquer.mix')])


class DamageSmoke(SparkBirth):
    """Add only Smoke observations and supplied inputs to the shared runtime."""
    def __init__(self, case):
        self.observing, self.smoke_events, self.owner = False, [], 0
        super().__init__(case, {})
        self.types, self.keys, self.smoke_layers = {}, {}, []
        u = self.u
        layers, art_raw, shapes = sources()
        wanted = set(SYSTEM_NAMES + PARTICLE_NAMES)
        image_names = set(PARTICLE_NAMES)
        for _, _, raw in layers:
            if raw is not None:
                sections, _ = lexical(raw, wanted)
                image_names.update(s['Image'] for s in sections.values() if s.get('Image'))
        art, _ = lexical(art_raw, image_names)
        self.make_ini(art)
        for name in image_names:
            filename = name + '.SHP'
            raw = shapes.get(mix_hash(filename))
            if raw is not None:
                self.assets[filename.upper()] = raw
        for name, ctor, size in [(n, 0x644BE0, 0x318) for n in PARTICLE_NAMES] + [
                (n, 0x6440A0, 0x310) for n in SYSTEM_NAMES]:
            pointer = self.alloc(size)
            self.invoke(ctor, pointer, (self.cstring(name),))
            self.types[name] = pointer
        self.smoke_constructor = self.type_state()
        self.wind_constructor = signed(self.read32(self.rules + 0x7AC))
        for name, archive, raw in layers:
            if raw is None:
                self.smoke_layers.append(dict(file=name, absent=True))
                continue
            sections, _ = lexical(raw, wanted | {'General'})
            selected = {n: s for n, s in sections.items() if n in wanted}
            selected['General'] = {k: v for k, v in sections.get('General', {}).items()
                                   if k == 'WindDirection'}
            for n, keys in selected.items():
                if n in wanted:
                    self.keys.setdefault(n, {}).update(keys)
            self.rules_cache(selected)
            self.reads.clear()
            self.asset_loaded.clear()
            for n in SYSTEM_NAMES + PARTICLE_NAMES:
                self.invoke(0x6442D0 if n in SYSTEM_NAMES else 0x644F50,
                            self.types[n], (RULES,))
            self.read_wind()
            self.smoke_layers.append(dict(file=name, archive=archive,
                sha256=hashlib.sha256(raw).hexdigest(), sections=selected,
                state=self.type_state(), wind=signed(self.read32(self.rules + 0x7AC)),
                image_loads=list(self.asset_loaded)))
        for name, keys in case.get('type_overrides', {}).items():
            self.rules_cache({name: dict(self.keys[name], **keys)})
            self.invoke(0x6442D0 if name in SYSTEM_NAMES else 0x644F50,
                        self.types[name], (RULES,))
        if 'wind' in case:
            self.rules_cache({'General': {'WindDirection': str(case['wind'])}})
            self.read_wind()
        # Existing terrain/collision evidence owns these independent scalars;
        # this is supplied active-retail prior state, not a startup proof.
        for address, value in ((0xAC4A18, 104), (0xAC4A0C, 416), (0x89E7C0, 104)):
            u.mem_write(address, words(value))
        u.mem_write(self.scenario + 0x214, words(case.get('native_id', 1000)))
        self.types_by_pointer = {p: n for n, p in self.types.items()}
        self.observing = True

    def read_wind(self):
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ESI, self.rules)
        self.u.reg_write(UC_X86_REG_EDI, RULES)
        native.run_checked(self.u, 0x66F2AF, 0x66F2CF, required_addresses=(0x5276D0,))

    def invoke(self, address, receiver, args=(), **kwargs):
        result = super().invoke(address, receiver, args, **kwargs)
        # The shared checked runner stops before a hook at RET_MAGIC. A direct
        # constructor control therefore completes its observation here; nested
        # native callers are observed by SparkBirth's existing return hook.
        if hasattr(self, 'returns'):
            while self.returns and self.returns[-1][:2] == (
                    native.RET_MAGIC, self.u.reg_read(UC_X86_REG_ESP)):
                _, _, row, pointer = self.returns.pop()
                row['returned_eax'] = result
                if row['pc'] in ('0x62dc50', '0x62b5e0'):
                    row['native_id'] = signed(self.read32(pointer + 0x10))
                    row['rng_requests_at_return'] = len(self.trace.requests)
        return result

    def type_state(self):
        output = {}
        for name, p in self.types.items():
            if name in SYSTEM_NAMES:
                output[name] = dict(holds_what=signed(self.read32(p + 0x294)),
                    spawns=bool(self.u.mem_read(p + 0x298, 1)[0]),
                    spawn_frames=signed(self.read32(p + 0x29C)),
                    slowdown_bits=self.bits(p + 0x2A0),
                    cap=signed(self.read32(p + 0x2A4)), radius=signed(self.read32(p + 0x2A8)),
                    cutoff_bits=self.bits(p + 0x2AC), trans_cutoff_bits=self.bits(p + 0x2B0),
                    behavior=signed(self.read32(p + 0x2B4)), lifetime=signed(self.read32(p + 0x2B8)))
            else:
                image = self.read32(p + 0xA4)
                output[name] = dict(max_ec=signed(self.read32(p + 0x2E0)),
                    velocity_bits=self.bits(p + 0x2FC), deacc_bits=self.bits(p + 0x300),
                    wind_effect=signed(self.read32(p + 0x2F8)), radius=signed(self.read32(p + 0x304)),
                    delete=bool(self.u.mem_read(p + 0x308, 1)[0]),
                    state_bytes=list(self.u.mem_read(p + 0x309, 3)),
                    translucency=signed(self.read32(p + 0x2F4)),
                    next_particle=signed(self.read32(p + 0x310)),
                    behavior=signed(self.read32(p + 0x314)),
                    image=self.string(p + 0x1F8), image_present=bool(image),
                    image_frames=struct.unpack('<h', self.u.mem_read(image + 6, 2))[0] if image else 0)
        return output

    def bits(self, address):
        return f'{self.read32(address):08x}'

    def hook(self, u, pc, size, data):
        if self.observing and pc in CALLS:
            from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX
            pointer = u.reg_read(UC_X86_REG_ECX)
            row = dict(call=CALLS[pc], pc=f'{pc:08x}',
                native_id=signed(self.read32(pointer + 0x10)),
                frame=signed(self.read32(FRAME)))
            if pc == 0x7258D0:
                row['control'] = u.reg_read(UC_X86_REG_EDX) & 255
            if self.owner:
                row['owner_has_smoke'] = bool(self.read32(self.owner + 0x310))
            self.smoke_events.append(row)
        super().hook(u, pc, size, data)

    def building_owner(self, coords=ORIGIN):
        p, kind = self.alloc(0x800), self.alloc(0x1800)
        self.invoke(0x5F3900, p)
        self.u.mem_write(p, words(0x7E3EBC))
        self.u.mem_write(p + 0x14, words(3))
        # Existing LiveSparkBirth's bounded GAPOWR coordinate receiver supplies
        # this same foundation index. Native447AC0/45EC90/45ECA0 own its center.
        self.u.mem_write(p + 0x520, words(kind))
        self.u.mem_write(kind + 0xEF0, words(3))
        # Supplied empty Techno pointer histories, using their native vtable as
        # in the existing Foot expiry fixtures; native expiry performs lookup.
        for offset in (0x458, 0x470):
            self.u.mem_write(p + offset, words(0x7E91EC, 0, 0, 1, 0, 10))
        # Building ctor43B8A1..B901 supplies its two empty expiry vectors.
        for offset in (0x66C, 0x684):
            self.u.mem_write(p + offset, words(0x7E43C8, 0, 0, 1, 0, 10))
        self.u.mem_write(p + 0x9C, words(*coords))
        self.u.mem_write(p + 0x90, b'\1')
        self.owner = p
        return p

    def system(self, name='SmallGreySSys', coords=ORIGIN, *, owner=False):
        owner_pointer = self.building_owner(coords) if owner else 0
        pointer, location, target = self.alloc(0x100), self.alloc(12), self.alloc(12)
        self.u.mem_write(location, words(*coords))
        self.invoke(0x62DC50, pointer, (self.types[name], location, 0, owner_pointer, target, 0))
        if owner:
            self.u.mem_write(owner_pointer + 0x310, words(pointer))
        return pointer

    def particle(self, system, name='SmallGreySmoke', coords=ORIGIN):
        pointer, location, target = self.alloc(0x138), self.alloc(12), self.alloc(12)
        self.u.mem_write(location, words(*coords))
        self.invoke(0x62B5E0, pointer, (self.types[name], location, target, system))
        item = self.alloc(4)
        self.u.mem_write(item, words(pointer))
        assert self.invoke(0x630250, system + 0xBC, (item,)) & 255
        return pointer

    def prepared_raw_state(self, values):
        # A supplied Random prior state: first words are caller controls, B's
        # disjoint window is zero. Original65C780 owns XOR, cursors and stores.
        assert len(values) < 103
        self.u.mem_write(self.scenario + 0x218,
                        words(0, 0, 103, *values, *([0] * (250 - len(values)))))

    def particle_state(self, p):
        return dict(native_id=signed(self.read32(p + 0x10)),
            type=self.types_by_pointer[self.read32(p + 0xAC)], coords=ints(self.u, p + 0x9C, 3),
            drift=ints(self.u, p + 0xC0, 3), velocity_bits=self.bits(p + 0xE4),
            remaining_ec=struct.unpack('<H', self.u.mem_read(p + 0x128, 2))[0],
            state=self.u.mem_read(p + 0x12E, 1)[0], translucency=self.u.mem_read(p + 0x12F, 1)[0],
            marked=bool(self.u.mem_read(p + 0x131, 1)[0]))

    def system_state(self, p):
        items = self.read32(p + 0xC0)
        members = [self.read32(items + i * 4) for i in range(self.read32(p + 0xCC))]
        return dict(native_id=signed(self.read32(p + 0x10)), coords=ints(self.u, p + 0x9C, 3),
            accumulator_bits=self.bits(p + 0xE8), lifetime=signed(self.read32(p + 0xEC)),
            done=bool(self.u.mem_read(p + 0xF8, 1)[0]), alive=bool(self.u.mem_read(p + 0x90, 1)[0]),
            limbo=bool(self.u.mem_read(p + 0x81, 1)[0]),
            owner_has_smoke=bool(self.read32(self.owner + 0x310)) if self.owner else None,
            particles=[self.particle_state(member) for member in members])

    def begin(self, values=None):
        assert not self.trace.pending and not self.returns
        if values is not None:
            self.prepared_raw_state(values)
        self.trace.requests.clear()
        self.smoke_events.clear()
        self.before_rng = self.trace.states()

    def result(self):
        after = self.trace.states()
        assert not self.trace.pending and not self.returns
        assert all(after[n] == self.before_rng[n] for n in ('main', 'mapgen'))
        assert hashlib.sha256(self.u.mem_read(TEXT_BEGIN, TEXT_SIZE)).hexdigest() == self.text_sha
        return dict(scenario_before=self.before_rng['scenario'], scenario_after=after['scenario'],
            rng_requests=list(self.trace.requests), events=list(self.smoke_events),
            main_mapgen_unchanged=True, text_unchanged=True)


def constructor(case):
    m = DamageSmoke(case)
    system = m.system()
    m.begin(case['raw_state_words'])
    before_id = signed(m.read32(m.scenario + 0x214))
    particle = m.particle(system, case.get('particle_type', 'SmallGreySmoke'))
    return dict(input=case, native_id_cursor_before=before_id,
                particle=m.particle_state(particle), **m.result())


def particle_tick(case):
    m = DamageSmoke(case)
    system = m.system()
    particle = m.particle(system)
    u = m.u
    u.mem_write(FRAME, words(case.get('frame', 40)))
    u.mem_write(particle + 0x128, struct.pack('<H', case.get('remaining_ec', 80)))
    u.mem_write(particle + 0x12E, bytes([case.get('state', 0)]))
    u.mem_write(particle + 0xC0, words(*case.get('drift', [0, 0, 0])))
    if 'velocity_bits' in case:
        u.mem_write(particle + 0xE4, words(case['velocity_bits']))
    m.begin(case.get('raw_state_words', []))
    before = m.particle_state(particle)
    m.invoke(0x62CE40, particle)
    return dict(input=case, before=before, after=m.particle_state(particle), **m.result())


def system_tick(case):
    m = DamageSmoke(case)
    system = m.system(case.get('system_type', 'SmallGreySSys'), owner=case.get('owner', False))
    u = m.u
    for pinput in case.get('parents', []):
        p = m.particle(system, pinput.get('type', 'SmallGreySmoke'),
                       pinput.get('coords', ORIGIN))
        u.mem_write(p + 0x128, struct.pack('<H', pinput.get('remaining_ec', 1)))
        u.mem_write(p + 0x12E, bytes([pinput.get('state', 20)]))
    if case.get('done'):
        m.invoke(0x6301E0, system)
    if 'accumulator_bits' in case:
        u.mem_write(system + 0xE8, words(case['accumulator_bits']))
    if 'moved_owner_coords' in case:
        u.mem_write(m.owner + 0x9C, words(*case['moved_owner_coords']))
    if case.get('bridge'):
        u.mem_write(m.cells[44, 48] + 0x140, words(0x100))
    m.begin(case.get('raw_state_words', []))
    before = m.system_state(system)
    visits = []
    for frame in case.get('frames', [40]):
        u.mem_write(FRAME, words(frame))
        m.invoke(0x62FD60, system)
        visits.append(dict(frame=frame, state=m.system_state(system)))
    return dict(input=case, before=before, visits=visits, **m.result())


def movement(case):
    m = DamageSmoke(case)
    system = m.system()
    coords = (ORIGIN[0], ORIGIN[1], case.get('z', 0))
    p = m.particle(system, coords=coords)
    m.u.mem_write(p + 0xE4, words(case.get('velocity_bits', f32_bits(9.75))))
    m.u.mem_write(p + 0xC0, words(*case.get('drift', [0, 0, 0])))
    m.u.mem_write(m.cells[44, 48] + 0x140, words(0x100 if case.get('bridge') else 0))
    m.begin([])
    before = m.particle_state(p)
    m.invoke(0x62D5E0, p)
    return dict(input=case, before=before, after=m.particle_state(p), **m.result())


def accumulator_walk():
    case = dict(name='stock_f32_accumulator_to_cutoff', frame=0)
    m = DamageSmoke(case)
    # Constructor default-1 HoldsWhat is supplied to isolate accumulator/spawn
    # timing; native still spends the admitted two offset draws before this gate.
    m.u.mem_write(m.types['SmallGreySSys'] + 0x294, words(-1))
    system = m.system()
    m.begin()
    checkpoints = []
    for frame in range(1301):
        m.u.mem_write(FRAME, words(frame))
        m.invoke(0x62FD60, system)
        state = m.system_state(system)
        if frame in (0, 398, 399, 400, 798, 799, 800, 1198, 1199, 1200) or not state['alive']:
            checkpoints.append(dict(frame=frame, state=state, request_count=len(m.trace.requests)))
        if not state['alive']:
            break
    else:
        raise AssertionError('Stock accumulator did not retire in the bounded 1301 visits')
    kept_frames = {row['frame'] for row in checkpoints}
    m.smoke_events[:] = [row for row in m.smoke_events if row['frame'] in kept_frames]
    return dict(input=case, supplied_holds_what=-1, checkpoints=checkpoints, **m.result())


def generate():
    reader = DamageSmoke(dict(name='physical_inputs'))
    constructors = [dict(name=name, raw_state_words=raw) for name, raw in (
        ('even_delta', [0, 0]), ('positive_delta', [1, 5]),
        ('negative_delta', [0xFFFFFFFF, 0xFFFFFFFE]),
        ('signed_min_delta', [0x80000000, 0x7FFFFFFF]))]
    ticks = [dict(name='even_id_cadence', native_id=1002),
             dict(name='odd_id_cadence', native_id=1003),
             dict(name='odd_id_cadence_nondivisible', native_id=1003, remaining_ec=79),
             dict(name='signed_id_wrap_cadence', native_id=0x7FFFFFFD, remaining_ec=0),
             dict(name='zero_ec_wrap', remaining_ec=0),
             dict(name='one_ec_death', remaining_ec=1),
             dict(name='odd_drift_x_clamp', frame=41, drift=[5, 0, 8], raw_state_words=[4, 0, 2]),
             dict(name='odd_drift_y_signed', frame=41, drift=[0, -5, 8],
                  raw_state_words=[0xFFFFFFFC, 0xFFFFFFFF, 0xFFFFFFFE]),
             dict(name='odd_drift_signed_min', frame=41,
                  raw_state_words=[0x80000000, 0, 0x80000000]),
             dict(name='odd_drift_no_branch', frame=41, drift=[1, 2, 8], raw_state_words=[1]),
             dict(name='even_drift_untouched', drift=[1, 2, 8], raw_state_words=[4, 0, 2]),
             dict(name='end_state_deletion', native_id=1002, state=19),
             dict(name='end_state_no_loop', state=20,
                  type_overrides={'SmallGreySmoke': {'DeleteOnStateLimit': 'no'}}),
             dict(name='deacc_below_three', velocity_bits=f32_bits(3.01))]
    systems = [dict(name='ordinary_periodic_spawn', raw_state_words=[5, 7, 0xFFFFFFFF, 0xFFFFFFFC]),
        dict(name='big_grey_periodic_spawn', system_type='BigGreySmokeSys',
             raw_state_words=[10, 11, 1, 9]),
        dict(name='spawns_cap_ignored', type_overrides={'SmallGreySSys': {'Spawns': 'no', 'ParticleCap': '0'}}),
        dict(name='reverse_parent_children', done=True,
             type_overrides={'LargeGreySmoke': {'NextParticle': 'SmallGreySmoke', 'Radius': '16'}},
             parents=[{'type': 'LargeGreySmoke'}, {'type': 'LargeGreySmoke'}],
             raw_state_words=[0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0]),
        dict(name='bridge_move_death_next_visit', owner=True, done=True, bridge=True, frames=[40, 42],
             parents=[dict(coords=[ORIGIN[0], ORIGIN[1], 150], remaining_ec=80, state=0)]),
        dict(name='empty_retirement_owner_slot', owner=True, done=True),
        dict(name='stationary_building_does_not_follow', owner=True,
             moved_owner_coords=[ORIGIN[0] + 256, ORIGIN[1] + 256, 0]),
        dict(name='unrounded_cutoff_comparison', accumulator_bits=f32_bits(13.0), frames=[39],
             type_overrides={'SmallGreySSys': {'Slowdown': '0.0000001'}}),
        dict(name='big_grey_cutoff', system_type='BigGreySmokeSys',
             accumulator_bits=f32_bits(15.0), frames=[44]),
        dict(name='translucency_cutoff_equal', accumulator_bits=f32_bits(12.5), frames=[36]),
        dict(name='minimum_speed', accumulator_bits=0x41200001,
             type_overrides={'SmallGreySmoke': {'Velocity': '2'}}),
        dict(name='spawn_divisor_crosses_integer', accumulator_bits=0x412FFFFF, frames=[110, 111])]
    moves = [dict(name='stock_rise'),
             dict(name='wind_direction_one', wind=1, drift=[2, -3, 0],
                  type_overrides={'SmallGreySmoke': {'WindEffect': '1'}}),
             dict(name='bridge_below_threshold', bridge=True, z=145),
             dict(name='bridge_threshold_equal', bridge=True, z=147),
             dict(name='above_deck', bridge=True, z=416)]
    return dict(native_sha256=native.image_sha256(),
        type_constructor=reader.smoke_constructor, wind_constructor=reader.wind_constructor,
        physical_layers=reader.smoke_layers,
        constructors=[constructor(case) for case in constructors],
        particle_ticks=[particle_tick(case) for case in ticks],
        system_ticks=[system_tick(case) for case in systems],
        movement=[movement(case) for case in moves], accumulator_walk=accumulator_walk())


def metadata():
    return native.provenance(
        scope='Original Smoke constructors, Particle dispatch and AI, reverse SmokeSystem children/spawn, movement and bounded float accumulator; supplied ordinary Building coordinate receiver and prior state.',
        assumptions=[
            'Historical original executable; shared SparkBirth/BulletReader machine, cached physical INI readers, archive/CRT/OS transport and native53/chop x87 control word.',
            'Selected SmallGreySSys/BigGreySmokeSys and SmallGreySmoke/LargeGreySmoke original constructors/readers over RULESMD, optional LANGRULE, MPBattleMD and XMP03T4 strings; ART and physical SHPs supplied through existing archive owner. This is not whole Rules::Process or physical INI/MIX traversal.',
            'Existing empty Logic/Display/effect registries and flat level0 cells42..48 by46..52; native level104/deck416 globals supplied from existing independently initialized scalar evidence.',
            'Native ID cursor reseeded after selected type readers. Original Particle/System derived constructors call the native ID allocator; no fixture hook assigns constructed IDs.',
            'Synthetic prior Random state uses disjoint A/B windows, or the inherited original Seed path; original Random bodies execute without returned-value substitution. Main/MapGen state and original text are checked unchanged.',
            'Prior state controls include word EC0/1, signed RNG/state/ID parity, byte state-end, f32 accumulator/cutoff/speed boundaries, and structural bridge bit100. They do not establish production formation of all supplied nonstock states.',
            'Building owner is a bounded original Object constructor plus actual Building vtable/flags/raw-coordinate/alive receiver and supplied GAPOWR foundation index3, reusing LiveSparkBirth prior-state admission. Native coordinate getters execute. This is not full Building constructor, placement, House, sale or ReceiveDamage. Empty effect-slot expiry executes original Building/Techno callbacks.',
            'Stock accumulator walk supplies constructor-default HoldsWhat=-1 to isolate arithmetic and offset-draw timing, records only key checkpoints, stops immediately on native logical retirement. No renderer/audio/save/load or whole-match execution.',
        ], substitutions=[
            'All execution and OS/archive/INI transport inherited from existing SparkBirth/BulletReader. No Smoke body, RNG call, math result, ID allocation, timer, detach or retirement return is substituted.',
        ], entry_points=dict(particle_ctor=0x62B5E0, particle_dispatch=0x62CE40,
            smoke_ai=0x62C540, move_dispatch=0x62D5E0, move_smoke=0x62D3F0,
            system_ctor=0x62DC50, system_ai=0x62FD60, smoke_system_ai=0x62ED40,
            system_type_reader=0x6442D0, particle_type_reader=0x644F50,
            wind_reader_slice=0x66F2AF, limbo=0x5F4D30, detach_all=0x5F5280,
            expiry=0x7258D0, techno_expired=0x7077C0))


if __name__ == '__main__':
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
