"""Original Bullet AI trailer cadence and the resulting complete Anim lifetime.

Reuses the original type readers and guided/impact world owners. No expected
cadence, constructor value, RNG draw or lifetime is computed in Python.
"""
import hashlib
import struct
from pathlib import Path

from unicorn import UC_ERR_EXCEPTION, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)

from tools.native_oracle import (
    RET_MAGIC, NativeCallTrace, NativeExecutionError,
    finish_vectors, initialize_empty_windows_seh, provenance, run_checked,
)
from tools.projectile_oracle.bridge_render_art_state import ArtStateReader
from tools.projectile_oracle.bridge_render_inputs import assets_root, lexical
from tools.projectile_oracle.guided_step import create, i32, vec, xyz
from tools.projectile_oracle.ifv_impact import (
    initialize_effect_registries, initialize_effect_world, retirement_transport,
)
from tools.projectile_oracle.ifv_launch import bullet_addref_transport
from tools.rmg_oracle.gen_rng_vectors import seeded_struct
from tools.spatial_oracle.building_body_rules import INI, RULES, SP, dwords

LAYERS = ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map')
ANIM_CTOR = 0x421EA0
HEADER_END = 0x4668BD


class TrailerReader(ArtStateReader):
    """One explicit allocation-failure control at the shared allocator boundary."""
    fail_anim_allocation = False

    def hook(self, u, address, size, data):
        if address == 0x7C8E17 and self.fail_anim_allocation:
            sp = u.reg_read(UC_X86_REG_ESP)
            if self.read32(sp + 4) == 0x1C8:
                self.ret(0)
                return
        super().hook(u, address, size, data)


def source_files():
    rows = []
    for name in (*LAYERS, 'ARTMD.INI', 'BBBLELRG.SHP', 'dragon.shp'):
        path = assets_root() / name
        if not path.exists():
            if name != 'LANGRULE.INI':
                raise FileNotFoundError(path)
            rows.append(dict(file=name, absent=True))
            continue
        raw = path.read_bytes()
        rows.append(dict(file=name, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest()))
    return rows


def prepare(*, delay=None, trailer=True, random_rate=None, admit=True):
    root = assets_root()
    art, _ = lexical((root / 'ARTMD.INI').read_bytes(),
                     {'AAHeatSeeker2', 'DRAGON', 'FV', 'SUBT', 'BBBLELRG'})
    if delay is not None:
        art['SUBT']['SpawnDelay'] = str(delay)
    if not trailer:
        art['SUBT'].pop('Trailer', None)
    if random_rate is not None:
        art['BBBLELRG']['RandomRate'] = random_rate
    m = TrailerReader(art)
    m, _, cells, _ = create(False, reader=m)
    u = m.u
    initialize_effect_registries(m)
    # The Anim-remove-listener registry needed by the expired-wait branch.
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x7254D0, 0x725506)
    w = m.invoke(0x772FA0, m.cstring('SubTorpedo'))
    p = m.construct('Torpedo')
    ctor = dict(spawn_delay=i32(u, p + 0x2E4), scaled_spawn_delay=i32(u, p + 0x2E8),
                scalable=bool(u.mem_read(p + 0x2EC, 1)[0]), trailer=m.read32(p + 0x2D8))
    layers = []
    for filename in LAYERS:
        path = root / filename
        if not path.exists():
            continue
        sections, _ = lexical(path.read_bytes(), {'SubTorpedo', 'Torpedo'})
        m.rules_cache(sections)
        m.invoke(0x772080, w, (RULES,))
        read = m.read_layer(p, sections)
        m.invoke(0x7729F0, w)
        layers.append(dict(file=filename, admitted=read['admitted'],
                           physical_keys=sections,
                           spawn_delay=i32(u, p + 0x2E4),
                           scalable=bool(u.mem_read(p + 0x2EC, 1)[0]),
                           trailer=read['state']['trailer']))
    a = m.read32(p + 0x2D8)
    anim_type = None
    if a:
        m.make_ini(art)
        m.asset_loaded = []
        admitted = m.invoke(0x427D00, a, (INI,))
        anim_type = dict(m.result('BBBLELRG', a, admitted),
                         physical_art_keys=art['BBBLELRG'], asset_loads=m.asset_loaded)
    initialize_effect_world(m, cells)
    u.mem_write(0x886B88, seeded_struct(31))
    b, references = construct_bullet(m, p, w, cells)
    initialize_empty_windows_seh(u)
    admission = fire_bullet(m, b, (2688, 5248, 624)) if admit else None
    return m, b, p, cells, dict(constructor=ctor, layers=layers, anim_type=anim_type,
                               bullet_addref=references, admission=admission)


def construct_bullet(m, p, w, cells):
    """Shared original constructor, AddRef and Configure owner for this corpus."""
    u = m.u
    b = m.alloc(0x180)
    m.invoke(0x466380, b)
    references = []
    hook = u.hook_add(UC_HOOK_CODE, lambda _u, pc, _size, _data:
                      bullet_addref_transport(m, pc, references))
    try:
        m.invoke(0x46AFD0, 0, (b,))
    finally:
        u.hook_del(hook)
    m.invoke(0x4664C0, b, (p, cells[16, 20], 0, m.read32(w + 0xA4),
                          m.read32(w + 0xAC), m.read32(w + 0xA8), 0))
    u.mem_write(b + 0x130, dwords(w))
    return b, references


def rng_state(m):
    rows = {}
    for name, address in [('main', 0x886B88), ('scenario', m.read32(0xA8B230) + 0x218)]:
        raw = bytes(m.u.mem_read(address, 0x3F4))
        rows[name] = dict(cursor=list(struct.unpack_from('<2i', raw, 4)),
                          sha256=hashlib.sha256(raw).hexdigest())
    return rows


class PlacementObservation:
    """Observe original placement consumers without supplying their results."""
    CALLS = {
        0x468670: ('BulletFire', 2, 8),
        0x5F4EC0: ('ObjectUnlimbo', 2, 8),
        0x46C4F0: ('BulletTypeFixupCoord', 2, 8),
        0x578080: ('MapGround', 1, 4),
        0x5F6940: ('SetLocation', 1, 4),
        0x4E1130: ('ProximitySetup', 4, 16),
    }

    def __init__(self, m):
        self.m = m
        self.calls = NativeCallTrace(m.u, m.read32)
        self.hook = m.u.hook_add(UC_HOOK_CODE, self.observe)

    def returned(self, pc):
        for index in self.calls.returned(pc, self.m.u.reg_read(UC_X86_REG_ESP)):
            row = self.calls.calls[index]
            if row['name'] == 'BulletTypeFixupCoord':
                row['adjusted'] = xyz(self.m.u, row['args'][0])
            elif row['name'] == 'MapGround':
                row['ground_z'] = struct.unpack('<i', struct.pack('<I', row['return_eax']))[0]

    def observe(self, u, pc, _size, _data):
        self.returned(pc)
        if pc not in self.CALLS:
            return
        index = self.calls.entered(pc, u.reg_read(UC_X86_REG_ESP), self.CALLS[pc])
        row = self.calls.calls[index]
        if pc in (0x468670, 0x5F4EC0, 0x578080, 0x5F6940):
            row['position'] = xyz(u, row['args'][0])
        elif pc == 0x46C4F0:
            row['position'] = xyz(u, row['args'][1])
        elif pc == 0x4E1130:
            row['position'] = xyz(u, row['args'][0])
            row['target'] = xyz(u, row['args'][1])

    def close(self):
        self.m.u.hook_del(self.hook)


def fire_bullet(m, b, origin, velocity=(1.0, 0.0, 0.0)):
    """One full original Fire owner for admission and placement controls."""
    u = m.u
    source = m.alloc(12)
    incoming = m.alloc(24)
    u.mem_write(source, dwords(*origin))
    u.mem_write(incoming, struct.pack('<3d', *velocity))
    row = dict(before_rng=rng_state(m),
               before_native_id=i32(u, m.read32(0xA8B230) + 0x214))
    observation = PlacementObservation(m)
    try:
        row['returned'] = m.invoke(0x468670, b, (source, incoming)) & 255
        observation.returned(RET_MAGIC)
        assert row['returned'] == 1
        assert all('return_eax' in call for call in observation.calls.calls)
        row.update(location=xyz(u, b + 0x9C), fire_coord=xyz(u, b + 0x134),
                   last_cell=list(struct.unpack('<2h', u.mem_read(b + 0x14C, 4))),
                   target_coord=xyz(u, b + 0x140), velocity=vec(u, b + 0xE8),
                   admission_bytes_80_81=list(u.mem_read(b + 0x80, 2)),
                   detector=dict(first_timer_start=i32(u, b + 0xB8),
                       first_timer_duration=i32(u, b + 0xC0),
                       arm_start=i32(u, b + 0xC4), arm_duration=i32(u, b + 0xCC),
                       reference=xyz(u, b + 0xD0), distance_watermark=i32(u, b + 0xDC)),
                   after_rng=rng_state(m),
                   after_native_id=i32(u, m.read32(0xA8B230) + 0x214),
                   calls=observation.calls.calls)
        row['ground_queries'] = [dict(position=call['position'], ground_z=call['ground_z'])
                                 for call in row['calls'] if call['name'] == 'MapGround']
    finally:
        observation.close()
    return row


def placement_controls():
    """Original type fixup plus full BulletFire with real Unlimbo/Display."""
    direct = [
        ('surface_below', 0, (2688, 5248, -1), False),
        ('surface_equal', 0, (2688, 5248, 0), False),
        ('surface_above', 0, (2688, 5248, 1), False),
        ('elevated_below', 6, (2688, 5248, 623), False),
        ('elevated_equal', 6, (2688, 5248, 624), False),
        ('elevated_above', 6, (2688, 5248, 625), False),
        ('distinct_xy', 0, (2699, 5367, 0), False),
        ('live_bridge_still_ground', 6, (2688, 5248, 624), True),
        ('signed_min_height', 0, (2688, 5248, -2147483648), False),
        ('signed_max_height', 0, (2688, 5248, 2147483647), False),
    ]
    fire = [
        ('surface_below_far', 0, (2688, 5248, -1536), False),
        ('surface_equal', 0, (2688, 5248, 0), False),
        ('surface_above', 0, (2688, 5248, 1), False),
        ('elevated_below', 6, (2688, 5248, 623), False),
        ('elevated_equal', 6, (2688, 5248, 624), False),
        ('elevated_above', 6, (2688, 5248, 625), False),
    ]
    output = dict(type_identity='Torpedo', fixup=[], fire=[])
    for kind, cases in [('fixup', direct), ('fire', fire)]:
        for name, level, origin, bridge in cases:
            m, b, p, cells, _ = prepare(admit=False)
            u = m.u
            for cell in cells.values():
                u.mem_write(cell + 0x11B, bytes((level, 0)))
                u.mem_write(cell + 0x140, dwords(0x100 if bridge else 0))
            source = m.alloc(12)
            adjusted = m.alloc(12)
            u.mem_write(source, dwords(*origin))
            floor = struct.unpack('<i', struct.pack('<I',
                m.invoke(0x578080, 0x87F7E8, (source,))))[0]
            assert m.read32(m.read32(p) + 0x6C) == 0x46C4F0
            row = dict(name=name, supplied=dict(level=level, slope=0,
                       live_bridge=bridge, origin=list(origin)), floor=floor)
            if kind == 'fixup':
                observation = PlacementObservation(m)
                try:
                    m.invoke(0x46C4F0, p, (adjusted, source))
                    row['adjusted'] = xyz(u, adjusted)
                    observation.returned(RET_MAGIC)
                    assert all('return_eax' in call for call in observation.calls.calls)
                    row['calls'] = observation.calls.calls
                    row['ground_queries'] = [dict(position=call['position'], ground_z=call['ground_z'])
                                             for call in row['calls'] if call['name'] == 'MapGround']
                finally:
                    observation.close()
            else:
                row['supplied'].update(target_cell=[16, 20], velocity=[1.0, 0.0, 0.0])
                row.update(fire_bullet(m, b, origin))
            output[kind].append(row)
    return output


def anim_state(m, pointer):
    u = m.u
    return dict(native_id=i32(u, pointer + 0x10), alive=bool(u.mem_read(pointer + 0x90, 1)[0]),
                position=xyz(u, pointer + 0x9C), stage=i32(u, pointer + 0xAC),
                timer=[i32(u, pointer + offset) for offset in (0xB4, 0xBC)],
                rate=i32(u, pointer + 0xC0), step=i32(u, pointer + 0xC4),
                owner_object=m.read32(pointer + 0xCC), bullet=m.read32(pointer + 0x17C),
                z_adjust=i32(u, pointer + 0x100), delay=i32(u, pointer + 0x184),
                draw_flags=m.read32(pointer + 0x190), loops=u.mem_read(pointer + 0x195, 1)[0],
                brand_new=bool(u.mem_read(pointer + 0x19C, 1)[0]))


def counts(m):
    return dict(bullets=i32(m.u, 0xA8ED50), anims=i32(m.u, 0xA8E9B8),
                pending=i32(m.u, 0xB0F6A8))


class Observation:
    CALLS = {
        0x421EA0: ('AnimCtor', 7, 28),
        0x424CE0: ('AnimStart', 0, 0),
        0x424F00: ('AnimMiddle', 0, 0),
        0x4255B0: ('AnimDestroy', 0, 0),
        0x426590: ('AnimScalarDtor', 1, 4),
        0x466560: ('BulletDtor', 0, 0),
        0x5F4EC0: ('Unlimbo', 2, 8),
        0x5F4D30: ('Conceal', 0, 0),
        0x5F65F0: ('UnInit', 0, 0),
        0x7258D0: ('PointerExpired', 0, 0),
        0x725C70: ('Drain', 0, 0),
        0x65C780: ('RandomRaw', 0, 0),
        0x65C7E0: ('RandomRanged', 2, 8),
    }

    def __init__(self, m):
        self.m = m
        self.calls = NativeCallTrace(m.u, m.read32)
        self.emissions = []
        self.raw_draws = []
        self.transport = []
        self.divisions = []
        self.hook = m.u.hook_add(UC_HOOK_CODE, self.observe)

    def returned(self, pc):
        m = self.m
        for index in self.calls.returned(pc, m.u.reg_read(UC_X86_REG_ESP)):
            row = self.calls.calls[index]
            if row['name'] == 'AnimCtor':
                self.emissions[row['emission_index']]['constructed'] = anim_state(m, row['return_eax'])

    def observe(self, u, pc, _size, _data):
        self.returned(pc)
        m = self.m
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in self.CALLS:
            index = self.calls.entered(pc, sp, self.CALLS[pc])
            row = self.calls.calls[index]
            row['frame'] = i32(u, 0xA8ED84)
            if pc == 0x7258D0:
                row['edx_control'] = u.reg_read(UC_X86_REG_EDX)
            if pc == ANIM_CTOR:
                args = row['args']
                row['emission_index'] = len(self.emissions)
                self.emissions.append(dict(pointer=u.reg_read(UC_X86_REG_ECX),
                    anim_type=m.string(args[0] + 0x24), position=xyz(u, args[1]),
                    delay=args[2], loops=args[3], draw_flags=args[4],
                    z_adjust=args[5], reverse=args[6]))
        if pc in (0x65C79D, 0x65C84B):
            self.raw_draws.append(dict(pc=hex(pc), word=u.reg_read(UC_X86_REG_ESI),
                                       frame=i32(u, 0xA8ED84)))
        if pc in (0x466846, 0x466882):
            self.divisions.append(dict(pc=hex(pc), remainder=u.reg_read(UC_X86_REG_EDX)))
        retirement_transport(m, pc, self.transport)

    def close(self):
        self.m.u.hook_del(self.hook)


def run_case(name, *, frame=3, identity=None, delay=None, scaled=0, alive=True,
             waiting='no', trailer=True, allocation_failure=False,
             position=(2688, 5248, 624), random_rate=None, lifecycle=False, flight=False,
             expect_divide_fault=False, prepared=None):
    m, b, p, cells, inputs = prepared if prepared is not None else prepare(delay=delay, trailer=trailer, random_rate=random_rate)
    u = m.u
    u.mem_write(0xA8ED84, dwords(frame))
    u.mem_write(b + 0x9C, dwords(*position))
    if identity is not None:
        u.mem_write(b + 0x10, dwords(identity))
    u.mem_write(b + 0x90, bytes([alive]))
    u.mem_write(b + 0x158, bytes([waiting != 'no']))
    if waiting == 'attached':
        u.mem_write(b + 0x154, dwords(m.alloc(0x1C8)))
    if scaled is not None:
        m.invoke(0x46C840, p, (scaled,))
    if flight:
        u.mem_write(b + 0xE8, struct.pack('<3d', 30.0, 0.0, 0.0))
        reference = m.alloc(12)
        m.invoke(0x486890, cells[16, 20], (reference,))
        m.invoke(0x4E1130, b + 0xB8, (b + 0x9C, reference, 2, 0x7FFFFFFF))
    m.fail_anim_allocation = allocation_failure
    observation = Observation(m)
    before = rng_state(m)
    row = dict(name=name, supplied=dict(frame=frame, identity=identity, art_delay=delay,
               scaled_delay=scaled, alive=alive, waiting=waiting, trailer=trailer,
               allocation_failure=allocation_failure, position=list(position),
               random_rate=random_rate, flight_velocity=[30.0, 0.0, 0.0] if flight else None),
               spawn_delay=i32(u, p + 0x2E4), before_rng=before,
               before_native_id=i32(u, m.read32(0xA8B230) + 0x214),
               bullet_native_id=i32(u, b + 0x10))
    try:
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, b)
        try:
            stop = run_checked(u, 0x4666E0, (HEADER_END, RET_MAGIC, 0x468D80),
                               context=dict(case=name))
            observation.returned(stop)
            row['stop'] = hex(stop)
        except NativeExecutionError as exc:
            # Native #DE is a deliberately selected domain observation. Every
            # other emulation fault, early stop or budget failure still fails.
            details = exc.diagnostics
            pc = u.reg_read(UC_X86_REG_EIP)
            if (not expect_divide_fault or details['reason'] != 'fault'
                    or details['fault']['errno'] != UC_ERR_EXCEPTION
                    or pc not in (0x466844, 0x46687C)):
                raise
            row['divide_fault'] = dict(pc=hex(pc), errno=details['fault']['errno'],
                                        message=details['fault']['message'])
        if expect_divide_fault and 'divide_fault' not in row:
            raise AssertionError('Selected native divide-fault control returned normally')
        row.update(after_rng=rng_state(m),
                   after_native_id=i32(u, m.read32(0xA8B230) + 0x214),
                   retained_position=xyz(u, b + 0x9C),
                   counts=counts(m), divisions=observation.divisions,
                   waiting_after=bool(u.mem_read(b + 0x158, 1)[0]))
        if flight:
            if row.get('stop') != hex(HEADER_END):
                raise AssertionError('Flight continuation requires admitted original header')
            sp = u.reg_read(UC_X86_REG_ESP)
            stop = run_checked(u, HEADER_END, 0x467B7A, required_addresses=(0x5B20F0,))
            observation.returned(stop)
            row['flight'] = dict(stop=hex(stop), candidate=xyz(u, sp + 0x24),
                                  retained_position=xyz(u, b + 0x9C), after_rng=rng_state(m))
        if lifecycle:
            if len(observation.emissions) != 1 or row.get('stop') != hex(HEADER_END):
                raise AssertionError('Lifecycle requires one original completed emission')
            a = observation.emissions[0]['pointer']
            row['anim_type_after_ctor'] = m.result('BBBLELRG', m.read32(a + 0xC8), 1)
            # Execute the existing Bullet UnInit owner, followed by the shared
            # deferred deletion owner. The free-standing trail must remain.
            m.invoke(0x5F65F0, b)
            observation.returned(RET_MAGIC)
            row['after_bullet_uninit'] = dict(counts=counts(m), anim=anim_state(m, a))
            m.invoke(0x725C70, 0)
            observation.returned(RET_MAGIC)
            row['after_bullet_drain'] = dict(counts=counts(m), anim=anim_state(m, a))
            states = []
            for step in range(128):
                if not i32(u, 0xA8E9B8):
                    break
                # The first appended Anim AI uses the same binary frame. This
                # fixture supplies the scheduling, not a complete Logic loop.
                u.mem_write(0xA8ED84, dwords(frame + step))
                m.invoke(0x423AC0, a)
                observation.returned(RET_MAGIC)
                states.append(dict(frame=frame + step, anim=anim_state(m, a), counts=counts(m)))
                if i32(u, 0xB0F6A8):
                    m.invoke(0x725C70, 0)
                    observation.returned(RET_MAGIC)
            if counts(m) != dict(bullets=0, anims=0, pending=0):
                raise AssertionError(('Incomplete original retirement', counts(m)))
            row['anim_frames'] = states
            row['final_counts'] = counts(m)
            row['final_rng'] = rng_state(m)
        row.update(emissions=observation.emissions, calls=observation.calls.calls,
                   raw_draws=observation.raw_draws, transport=observation.transport)
    finally:
        observation.close()
    return row, inputs


def initialize_scalable_vector(m):
    """Original CRT initializer writes; stop before unsupported CRT atexit."""
    m.u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(m.u, 0x4662E0, 0x466316)
    if (i32(m.u, 0x89DE28), i32(m.u, 0x89DE2C)) != (0, 10):
        raise AssertionError('Original Scalable vector initializer did not finish')


def scalable_state(m, types, bullets):
    u = m.u
    n = i32(u, 0x89DE28)
    data = m.read32(0x89DE1C)
    return dict(members=[bullets[m.read32(data + i * 4)] for i in range(n)], count=n,
                types={name: dict(base_delay=i32(u, p + 0x2E4), scaled_delay=i32(u, p + 0x2E8),
                                 scalable=bool(u.mem_read(p + 0x2EC, 1)[0]),
                                 trailer=m.string(m.read32(p + 0x2D8) + 0x24) if m.read32(p + 0x2D8) else None)
                       for name, p in types.items()})


def scalable_action(m, name, kind, pointer, receiver, bullets, types, *, control=None):
    u = m.u
    before_rng = rng_state(m)
    before_id = i32(u, m.read32(0xA8B230) + 0x214)
    before = scalable_state(m, types, bullets)
    observation = Observation(m)
    callbacks = []

    def observe_callback(u, pc, _size, _data):
        if pc == 0x4684E0:
            sp = u.reg_read(UC_X86_REG_ESP)
            callbacks.append(dict(receiver=bullets.get(u.reg_read(UC_X86_REG_ECX)),
                                  expired=bullets.get(m.read32(sp + 4)),
                                  control=bool(m.read32(sp + 8) & 255)))

    hook = u.hook_add(UC_HOOK_CODE, observe_callback)
    try:
        if kind == 'enroll':
            m.invoke(0x46B280, pointer)
        elif kind == 'expire':
            m.invoke(0x4684E0, receiver, (pointer, int(control)))
        elif kind == 'uninit':
            m.invoke(0x5F65F0, pointer)
        elif kind == 'drain':
            m.invoke(0x725C70, 0)
        else:
            raise ValueError(kind)
        observation.returned(RET_MAGIC)
        return dict(name=name, action=kind, bullet=bullets[pointer], control=control,
                    bullet_type=m.string(m.read32(pointer + 0xAC) + 0x24) if kind != 'drain' else None,
                    before=before, after=scalable_state(m, types, bullets),
                    before_rng=before_rng, after_rng=rng_state(m), before_native_id=before_id,
                    after_native_id=i32(u, m.read32(0xA8B230) + 0x214), counts=counts(m),
                    calls=observation.calls.calls, callbacks=callbacks, transport=observation.transport)
    finally:
        u.hook_del(hook)
        observation.close()


def scalable_sole_retirement():
    m, b, p, _cells, _inputs = prepare()
    initialize_scalable_vector(m)
    authored = {'Torpedo': dict(Image='SUBT', Scalable='yes')}
    read = m.read_layer(p, authored)
    bullets, types = {b: 1}, {'Torpedo': p}
    return dict(authored_rules=authored, reader_admitted=read['admitted'],
                rows=[scalable_action(m, name, kind, b, b, bullets, types)
                      for name, kind in [('sole_enroll', 'enroll'), ('sole_uninit', 'uninit'), ('sole_drain', 'drain')]])


def scalable_overflow_controls():
    rows = []
    for delay in (2147483647, -2147483648):
        m, first, p, cells, _inputs = prepare(delay=delay)
        initialize_scalable_vector(m)
        authored = {'Torpedo': dict(Image='SUBT', Scalable='yes')}
        read = m.read_layer(p, authored)
        w = m.read32(first + 0x130)
        states, bullets = [], {first: 1}
        for identity in range(1, 9):
            b = first
            if identity != 1:
                b, _ = construct_bullet(m, p, w, cells)
                fire_bullet(m, b, (2688, 5248, 624))
                bullets[b] = identity
            m.invoke(0x46B280, b)
            states.append(scalable_state(m, {'Torpedo': p}, bullets))
        rows.append(dict(art_spawn_delay=str(delay), authored_rules=authored,
                         reader_admitted=read['admitted'], states=states))
    return rows


def scalable_controls():
    """Original list producer/removal, then the existing header/Anim owner."""
    m, first, primary, cells, inputs = prepare()
    initialize_scalable_vector(m)
    u = m.u
    # Authored final rules/ART pass over real Torpedo, through native readers.
    authored_a = {'Torpedo': dict(Image='SUBT', Scalable='yes')}
    read_a = m.read_layer(primary, authored_a)
    art, _ = lexical((assets_root() / 'ARTMD.INI').read_bytes(), {'SUBT', 'BBBLELRG'})
    art['SUBT']['SpawnDelay'] = '7'
    m.make_ini(art)
    secondary = m.construct('ScalableTrailerB')
    authored_b = {'ScalableTrailerB': dict(Image='SUBT', Scalable='yes')}
    read_b = m.read_layer(secondary, authored_b)
    w = m.read32(first + 0x130)
    types = {'Torpedo': primary, 'ScalableTrailerB': secondary}
    bullets = {first: 1}
    records = []

    def state():
        return scalable_state(m, types, bullets)

    def action(name, kind, pointer, *, control=None):
        records.append(scalable_action(m, name, kind, pointer, first, bullets, types, control=control))

    sequence = [first]
    for identity in range(1, 12):
        p = secondary if identity in (9, 11) else primary
        b = first
        if identity != 1:
            b, _ = construct_bullet(m, p, w, cells)
            fire_bullet(m, b, (2688, 5248, 624))
            bullets[b] = identity
            sequence.append(b)
        action('count_' + str(identity), 'enroll', b)
        if identity in (7, 8, 11):
            for frame in ((3, 4) if identity == 8 else (3,) if identity == 7 else (9,)):
                header, _ = run_case('enrolled_header_' + str(identity) + '_frame_' + str(frame),
                                     frame=frame, scaled=None, prepared=(m, b, p, cells, inputs))
                records.append(dict(name=header['name'], action='header', bullet=identity,
                                    before=state(), after=state(), header=header))
    action('expire_false', 'expire', sequence[4], control=False)
    action('expire_true', 'expire', sequence[4], control=True)
    action('expire_repeat', 'expire', sequence[4], control=True)
    # The original UnInit/Conceal broadcasts remove the last ordered member before its
    # actual deferred destructor. This is independent of direct callback tests.
    action('full_uninit', 'uninit', sequence[-1])
    action('full_drain', 'drain', sequence[-1])
    fresh, _ = construct_bullet(m, secondary, w, cells)
    fire_bullet(m, fresh, (2688, 5248, 624))
    bullets[fresh] = 12
    action('reenroll_after_removal', 'enroll', fresh)

    # One real retail Medusa type through every applicable selected rules layer.
    medusa = m.construct('MedusaProjectile')
    medusa_layers = []
    fixed, _ = lexical((assets_root() / 'ARTMD.INI').read_bytes(), {'MEDUSA'})
    m.make_ini(fixed)
    for filename in LAYERS:
        path = assets_root() / filename
        if not path.exists():
            continue
        sections, _ = lexical(path.read_bytes(), {'MedusaProjectile'})
        read = m.read_layer(medusa, sections)
        medusa_layers.append(dict(file=filename, physical_keys=sections, admitted=read['admitted']))
    types['MedusaProjectile'] = medusa
    b, _ = construct_bullet(m, medusa, w, cells)
    fire_bullet(m, b, (2688, 5248, 624))
    bullets[b] = 13
    action('stock_medusa_enroll', 'enroll', b)
    header, _ = run_case('stock_medusa_null_trailer', frame=12, scaled=None,
                         prepared=(m, b, medusa, cells, inputs))
    records.append(dict(name=header['name'], action='header', bullet=13,
                        before=state(), after=state(), header=header))
    return dict(authored_rules=[authored_a, authored_b], authored_art=art,
                authored_reader_admitted=[read_a['admitted'], read_b['admitted']],
                medusa_layers=medusa_layers, rows=records,
                sole_retirement=scalable_sole_retirement(),
                overflow_controls=scalable_overflow_controls())


def generate():
    rows = []
    cases = [(f'stock_frame_{frame}', dict(frame=frame)) for frame in range(-4, 8)]
    cases += [(f'phase_identity_{identity}', dict(identity=identity))
              for identity in (0, 1, 100, 2147483647, -2147483648)]
    cases += [
        ('negative_delay_hit', dict(delay=-3, frame=-6)),
        ('negative_delay_miss', dict(delay=-3, frame=-5)),
        ('delay_one', dict(delay=1, frame=2147483647)),
        ('delay_min', dict(delay=-2147483648, frame=-2147483648)),
        ('delay_max', dict(delay=2147483647, frame=2147483647)),
        ('scaled_hit', dict(scaled=4, frame=8)),
        ('scaled_overrides_base', dict(scaled=4, frame=9)),
        ('scaled_negative', dict(scaled=-4, frame=-8)),
        ('scaled_overrides_zero', dict(delay=0, scaled=4, frame=8)),
        ('base_zero_trap', dict(delay=0, expect_divide_fault=True)),
        ('base_overflow_trap', dict(delay=-1, frame=-2147483648, expect_divide_fault=True)),
        ('scaled_overflow_trap', dict(scaled=-1, frame=-2147483648, expect_divide_fault=True)),
        ('dead_before_divide', dict(delay=0, alive=False)),
        ('waiting_attached_before_divide', dict(delay=0, waiting='attached')),
        ('waiting_expired_before_divide', dict(delay=0, waiting='expired')),
        ('no_trailer_before_divide', dict(delay=0, trailer=False)),
        ('allocation_failure', dict(allocation_failure=True)),
        ('distinct_current_position', dict(position=(3200, 5504, 720))),
        ('emission_before_guidance', dict(flight=True)),
        ('stock_lifetime', dict(lifecycle=True)),
        ('constructor_random_rate', dict(random_rate='450,225', lifecycle=True)),
    ]
    for name, supplied in cases:
        row, inputs = run_case(name, **supplied)
        rows.append(row)
        if name == 'stock_lifetime':
            retail = inputs
    return dict(schema=1, sources=source_files(),
                scenario_rng_seed=31, main_rng_seed=31, retail=retail,
                seeded_rng_state_hex=seeded_struct(31).hex(),
                retail_projectiles=retail_projectiles(),
                placement_controls=placement_controls(), rows=rows,
                scalable_controls=scalable_controls())


def retail_projectiles():
    """Original readers for a declared, physically referenced type inventory.

    Selection is lexical rather than original Rules::Process discovery. The
    Rust consumer can compare this inventory against its registered types;
    neither that comparison nor stock coverage certifies authored Scalable.
    """
    root = assets_root()
    raw = (root / 'RULESMD.INI').read_bytes()
    names = {line.strip()[1:line.strip().index(']')]
             for line in raw.decode('latin1').splitlines()
             if line.strip().startswith('[') and ']' in line.strip()}
    rules, _ = lexical(raw, names)
    selected = sorted({section['Projectile'] for section in rules.values()
                       if section.get('Projectile') and section['Projectile'] in rules}
                      | {'GiantNukeUp', 'GiantNukeDown'})
    layers = []
    for name in LAYERS:
        path = root / name
        if path.exists():
            layers.append((name, lexical(path.read_bytes(), set(selected))[0]))
    rows = []
    for identity in selected:
        wanted = {identity} | {sections[identity]['Image'][:24]
                               for _, sections in layers
                               if identity in sections and sections[identity].get('Image')}
        art, _ = lexical((root / 'ARTMD.INI').read_bytes(), wanted)
        m = ArtStateReader(art)
        p = m.construct(identity)
        passes = []
        for filename, sections in layers:
            m.read_layer(p, {identity: sections[identity]} if identity in sections else {})
            state = m.bullet_state(p)
            passes.append(dict(file=filename, scalable=bool(m.u.mem_read(p + 0x2EC, 1)[0]),
                               trailer=state['trailer'], spawn_delay=state['spawn_delay']))
        rows.append(dict(identity=identity, image=m.string(p + 0x1F8), passes=passes))
    return dict(selection='Physical RULESMD Projectile references plus GiantNukeUp/GiantNukeDown; not native registry discovery',
                rows=rows)


def metadata():
    result = provenance(scope='Original Bullet placement/Fire, AI trailer cadence and complete free-standing BBBLELRG Anim lifetime', assumptions=[
        'The full original BulletType constructor and readers consume supplied physical lexical RULESMD, optional LANGRULE, MPBattleMD and Hills inputs; fixed ART reads consume SUBT and BBBLELRG. Hills.map is the unchanged inner INI entryD5FE80AC extracted from loose Hills.mmx, not the MMX container or XHills.MAP. Native physical archive/INI walking is outside this witness.',
        'The existing guided and impact fixture owners initialize selected original registries and flat level6 Cell objects. Each header Bullet executes full original Fire468670/Unlimbo before documented in-flight position/alive/wait/frame overrides; admission is retained in retail.admission. GameActive, empty receiver pools, target Cell and incoming Fire velocity are supplied boundaries. No SUB placement, upstream Techno FireAt, collision, rendering or complete startup pool identity is claimed.',
        'Header execution begins at4666E0 including ObjectAI and ends before flight4668BD, at the original dead/wait return, or the expired-wait explosion entry468D80. One continuation executes native Torpedo guidance through precommit467B7A with supplied incoming velocity and records its distinct candidate coordinate after the earlier Anim constructor. No native arithmetic result or branch outcome is calculated by Python. Explicit #DE controls accept only processor-exception faults at the two original IDIV instructions; all other execution failures abort publication.',
        'The original38 header rows remain unchanged: their scaled controls execute original setter46C840 with supplied input. The separate scalable_controls execute whole original46B280 with constructor/reader-backed types, ordered vector growth, direct PointerExpired controls and full UnInit/drain, including sole-member self callbacks. It explicitly executes vector initializer4662E0..466316, stopping BEFORE unsupported CRT atexit registration; not full CRT startup. Its two extreme ART base delays execute original readers and eight whole enrollments each to observe wrapping addition; they are authored arithmetic controls, not stock tuning. ART delay controls go through original readers. Main and Scenario RNG are independently seeded by original65C6D0 with seed31; original raw draw stores and both retained state hashes are recorded.',
        'Every admitted emission executes full Anim421EA0 including native identity, registration, Unlimbo and timer setup. Two histories remove the original Bullet through UnInit and deferred drain, then run the surviving independent AnimAI from the same frame through deferred retirement. The host supplies that schedule; the complete Logic loop, renderer and audio device are outside coverage.',
        'One authored RandomRate=450,225 ART control executes the original reader conversion before constructor RNG ordering and continuation; stock BBBLELRG retains its physical ART. Reader dependencies use unchanged shared default fixture setup rather than a separate world implementation.',
        'Original Bullet466380 plus AddRef46AFD0 establishes one host-held COM reference; original Configure4664C0 reads the physical weapon/type outputs. This does not execute upstream Unit FireAt or COM activation.',
        'The auxiliary Scalable inventory is selected from physical RULESMD Projectile references plus the two explicit nuclear roots. Every included constructor/full layered reader executes; this selection is not original native registry discovery and excludes no duplicate selected ART section silently.',
        'Placement controls execute the concrete BulletType virtual+6C owner46C4F0 and full BulletFire468670, including original ObjectUnlimbo, SetLocation, Display and ProximitySetup. Flat Cell level/slope/live-bridge inputs, raw muzzle XYZ, mapped target Cell and incoming velocity are supplied; no upstream Unit GetFLH/FireAt or native whole-map placement is claimed. Dummy/unallocated source/cell-target aliasing is outside these mapped-cell controls. Each control constructs a fresh unadmitted physical Torpedo through the existing world owner. Direct controls include signed input-height extremes without supplying an impossible INT_MAX terrain result.',
    ], substitutions=[
        'Inherited reader supplies signed-CRC INI indexes, exact extracted archive bytes, allocation/delete/CRT/TLS and the unused Color entry. Original ObjectType LoadVoxel5F8110 is skipped only after scalar reads, as in ArtStateReader. Runtime voxel loading is not established.',
        'The one allocation-failure control returns null at the shared operator_new boundary only for the producer requested1C8 bytes. Existing shared Windows InterlockedIncrement/Decrement and checked IsBadReadPtr transports admit original native reference/retirement/RTTI with an empty supplied FS exception chain.',
    ], entry_points={'bullet_ai': 0x4666E0, 'base_idiv': 0x46687C,
        'scaled_idiv': 0x466844, 'anim_ctor': ANIM_CTOR, 'anim_ai': 0x423AC0,
        'anim_reader': 0x427D00, 'bullet_type_ctor': 0x46BBC0,
        'bullet_type_reader': 0x46BEE0, 'scaled_setter': 0x46C840,
        'scalable_vector_init_prefix': 0x4662E0, 'scalable_vector_init_end': 0x466316,
        'init_scalable': 0x46B280, 'bullet_pointer_expired': 0x4684E0,
        'bullet_type_fixup_coord': 0x46C4F0, 'bullet_fire': 0x468670,
        'object_unlimbo': 0x5F4EC0, 'set_location': 0x5F6940,
        'map_ground': 0x578080, 'proximity_setup': 0x4E1130,
        'uninit': 0x5F65F0, 'drain': 0x725C70})
    result['command'] = 'PYTHONDONTWRITEBYTECODE=1 python -m tools.projectile_oracle.projectile_trailer --check'
    result['input_environment'] = 'RA2_DIR or VERA20K_GAMEMD_EXE selects the supported executable; VERA20K_PROJECTILE_RENDER_ASSETS selects the extracted files identified in the payload sources.'
    return result


def main(argv=None):
    root = Path(__file__).resolve().parents[2]
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata, argv=argv,
                   source_paths={name: root / name for name in (
                       'tools/projectile_oracle/projectile_trailer.py',
                       'tools/projectile_oracle/guided_step.py',
                       'tools/projectile_oracle/ifv_impact.py',
                       'tools/projectile_oracle/ifv_launch.py',
                       'tools/projectile_oracle/bridge_render_inputs.py',
                       'tools/projectile_oracle/bridge_render_art_state.py',
                       'tools/projectile_oracle/flat_art.py',
                       'tools/rules_oracle/bridge_anim_inputs.py',
                       'tools/rules_oracle/bridge_anim_lists.py',
                       'tools/spatial_oracle/building_body_rules.py',
                       'tools/spatial_oracle/map_queries.py',
                       'tools/rmg_oracle/gen_rng_vectors.py',
                       'tools/native_oracle.py')})


if __name__ == '__main__':
    main()
