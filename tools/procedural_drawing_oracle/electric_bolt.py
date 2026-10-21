"""Original ordinary EBolt producer, retained tactical manager and packed pixels.

The existing original-image, Building, cached-INI and packed-surface fixtures
own transport. This producer adds observations, never a Python bolt algorithm.
See electric_bolt.md for entry boundaries and supplied runtime state.
"""
from functools import lru_cache
from pathlib import Path
import hashlib
import struct
import sys

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
    UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP,
    UC_X86_REG_ESI, UC_X86_REG_ESP)

from tools import native_oracle as native
from tools.spatial_oracle import building_prism as prism
from tools.procedural_drawing_oracle import rally
from tools.projectile_oracle.bridge_render_inputs_palette import PaletteReader, initialize
from tools.rmg_oracle.gen_rng_vectors import seeded_struct
from tools.sidebar_oracle.stock import mix, mix_hash
from tools.projectile_oracle.bridge_render_inputs import BulletReader, lexical
from tools.projectile_oracle.ifv_impact import (initialize_effect_registries,
    initialize_effect_world, retirement_transport)
from tools.projectile_oracle.ifv_launch import bullet_addref_transport
from tools.projectile_oracle.projectile_trailer import fire_bullet
from tools.rules_oracle import weapon_speed
from tools.rules_oracle.weapon_laser import physical_layers
from tools.spatial_oracle.building_body_rules import INI, RULES, SP as READER_SP

CTOR, INIT, DRAW, MANAGER, CLEAR = 0x4C1E10, 0x4C2A60, 0x4C1F20, 0x4C2830, 0x4C29E0
VECTOR_INIT, VECTOR = 0x4C1D90, 0x8A0E88
MAIN, MAPGEN = 0x886B88, 0xABE890
SP = prism.SP


def words(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def u32(u, p):
    return struct.unpack('<I', u.mem_read(p, 4))[0]


def ints(u, p, count):
    return list(struct.unpack('<' + 'i' * count, u.mem_read(p, count * 4)))


def signed(n):
    return struct.unpack('<i', words(n))[0]


class RngTrace:
    """Observe original requested bounds and raw stores, including rejection."""
    def __init__(self, u, scenario, seed=1):
        self.u = u
        self.streams = {'main': MAIN, 'scenario': scenario + 0x218, 'mapgen': MAPGEN}
        for index, pointer in enumerate(self.streams.values()):
            u.mem_write(pointer, seeded_struct(seed + index))
        self.requests, self.pending = [], []
        u.hook_add(UC_HOOK_CODE, self.observe)

    def states(self):
        return {name: bytes(self.u.mem_read(p, 0x3F4)).hex() for name, p in self.streams.items()}

    def observe(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if self.pending and (pc, sp) == self.pending[-1][:2]:
            _, _, row = self.pending.pop()
            value = u.reg_read(UC_X86_REG_EAX)
            row['result'] = signed(value) if row['kind'] == 'ranged' else value
        if pc in (0x65C780, 0x65C7E0):
            this = u.reg_read(UC_X86_REG_ECX)
            stream = next((name for name, p in self.streams.items() if p == this), None)
            assert stream is not None, hex(this)
            row = dict(kind='ranged' if pc == 0x65C7E0 else 'raw', stream=stream,
                       caller=hex(u32(u, sp)), raw_draws=[])
            if pc == 0x65C7E0:
                row.update(zip(('low', 'high'), ints(u, sp + 4, 2)))
            self.requests.append(row)
            self.pending.append((u32(u, sp), sp + (12 if pc == 0x65C7E0 else 4), row))
        if pc in (0x65C79D, 0x65C84B):
            assert self.pending
            self.pending[-1][2]['raw_draws'].append(u.reg_read(UC_X86_REG_ESI))


@lru_cache(maxsize=1)
def physical_palette():
    root = native.configured_gamemd().parent
    cache = mix(mix((root / 'ra2.mix').read_bytes())[mix_hash('cache.mix')])
    m = PaletteReader({})
    m.assets = {name.upper(): cache[mix_hash(name)] for name in ('palette.pal', 'anim.pal')}
    initialize(m)
    p = m.read32(0x87F6C4)
    table = m.read32(p + 0x174)
    raw = bytes(m.u.mem_read(table, 512))
    return dict(loads=m.asset_loaded, bytes_per_pixel=m.read32(p + 4),
                table_hex=raw.hex(), table_sha256=hashlib.sha256(raw).hexdigest(),
                selected={str(i): struct.unpack_from('<H', raw, 2*i)[0] for i in (5, 10, 15)})


def bolt_state(u, p):
    return dict(source=ints(u, p, 3), target=ints(u, p + 12, 3),
                z_adjust=signed(u32(u, p + 0x18)), phase=signed(u32(u, p + 0x1C)),
                owner=u32(u, p + 0x20), weapon_slot=signed(u32(u, p + 0x24)),
                decay=signed(u32(u, p + 0x28)), alternate=bool(u.mem_read(p + 0x2C, 1)[0]))


def weapon_state(m, p):
    return dict(is_laser=bool(m.u.mem_read(p + 0x149, 1)[0]),
                is_electric_bolt=bool(m.u.mem_read(p + 0x151, 1)[0]),
                is_alternate_color=bool(m.u.mem_read(p + 0x153, 1)[0]),
                range_leptons=signed(m.read32(p + 0xB4)))


def physical_weapon(m=None, *, read_warhead=False):
    if m is None:
        m, weapon = weapon_speed.fresh('CoilBolt')
    else:
        m.u.mem_write(0x887568, words(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
        weapon = m.invoke(0x772FA0, m.cstring('CoilBolt'))
    constructor, rows = weapon_state(m, weapon), []
    for name, archive, raw in physical_layers():
        if raw is None:
            rows.append(dict(file=name, absent=True))
            continue
        sections, _ = lexical(raw, {'CoilBolt', 'Invisible', 'Electric'})
        m.rules_cache(sections)
        m.reads.clear()
        admitted = bool(m.invoke(0x772080, weapon, (RULES,)) & 255)
        projectile = m.read32(weapon + 0xA0)
        if projectile:
            m.invoke(0x46BEE0, projectile, (RULES,))
        if read_warhead:
            m.invoke(0x75D3A0, m.read32(weapon + 0xAC), (RULES,))
        rows.append(dict(file=name, archive=archive, bytes=len(raw),
                         sha256=hashlib.sha256(raw).hexdigest(), sections=sections,
                         admitted=admitted, state=weapon_state(m, weapon),
                         reads=list(m.reads)))
    return m, weapon, dict(constructor=constructor, layers=rows)


@lru_cache(maxsize=1)
def spark_target_inputs():
    """Physical GAPOWR contact flags through the existing original type fixture."""
    m = rally.HutTypeReader()
    p, u = m.construct('GAPOWR'), m.u

    def fields():
        return dict(laser_fence=bool(u.mem_read(p + 0x16BF, 1)[0]),
                    undeploys_into_present=bool(m.read32(p + 0x408)))

    constructor = fields()
    seams = sorted({row['pc'] for row in m.trace if row['kind'] == 'fixture_seam'})
    rows, reads = [], []

    def observe(_u, pc, _size, _data):
        if pc not in (0x5295F0, 0x528A10):
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        default = m.read32(sp + 12)
        reads.append(dict(reader=hex(pc), section=m.string(m.read32(sp + 4)),
            key=m.string(m.read32(sp + 8)),
            default=m.string(default) if pc == 0x528A10 else default & 255))

    u.hook_add(UC_HOOK_CODE, observe)
    for name, archive, raw in physical_layers():
        if raw is None:
            rows.append(dict(file=name, absent=True))
            continue
        sections, _ = lexical(raw, {'GAPOWR'})
        m.make_ini(sections)
        admitted = bool(m.invoke(0x526810, rally.READER_INI, (p + 0x24,)) & 255)
        mark, before = len(reads), fields()
        if admitted:
            for begin, end, delta in ((0x460AA0, 0x460AC0, 0),
                                       (0x71329D, 0x7132EA, -16)):
                u.reg_write(UC_X86_REG_ESP, rally.READER_SP)
                u.reg_write(UC_X86_REG_EBP, p)
                u.reg_write(UC_X86_REG_EBX, p + 0x24)
                u.reg_write(UC_X86_REG_ESI, rally.READER_INI)
                # The LaserFence prefix stores the preceding unrelated AL.
                u.reg_write(UC_X86_REG_EAX, u.mem_read(p + 0x16BE, 1)[0])
                native.run_checked(u, begin, end, count=200000)
                assert u.reg_read(UC_X86_REG_ESP) == rally.READER_SP + delta
        rows.append(dict(file=name, archive=archive, sha256=hashlib.sha256(raw).hexdigest(),
            sections=sections, admitted=admitted, before=before, reads=reads[mark:], after=fields()))
    result = fields()
    result['foundation_index'] = prism.laser_type_inputs('GAPOWR')['result']['foundation']
    # The foundation was independently read by the existing original owner.
    u.mem_write(p + 0xEF0, words(result['foundation_index']))
    result['one_cell_undeployer'] = bool(m.invoke(0x465D40, p) & 255)
    m.unchanged()
    return dict(type_id='GAPOWR', constructor=constructor, constructor_fixture_seams=seams,
        layers=rows, result=result, foundation_evidence='type_inputs.GAPOWR', text_unchanged=True)


class ElectricBirth(prism.LaserScene):
    """Original Building selection, getters and admitted FireAt effect arm."""
    def at(self, address, handler):
        # The older caller-only Prism fixture supplies SelectWeapon. This
        # chain executes it; every inherited active getter is still original.
        if address != prism.SELECT_WEAPON:
            super().at(address, handler)

    def __init__(self, case):
        fields = prism.laser_type_inputs('TESLA')['result']
        super().__init__(dict(case, location=[9856, 12416, 0],
            primary_flh=fields['primary_flh'], pixel_offset=fields['primary_pixel'],
            primary_dual=fields['primary_dual'], is_laser=False,
            target_building=dict(location=[11392, 12416, 0], foundation_index=3)))
        u = self.u
        reader, weapon, self.weapon_inputs = physical_weapon()
        projectile = reader.read32(weapon + 0xA0)
        u.mem_write(prism.SHOT_WEAPON, bytes(reader.u.mem_read(weapon, 0x1A0)))
        u.mem_write(prism.SHOT_WEAPON + 0xA0, words(prism.PROJECTILE))
        u.mem_write(prism.PROJECTILE, bytes(reader.u.mem_read(projectile, 0x310)))
        u.mem_write(self.address[self.first] + 0x2B4,
                    words(0 if case.get('clear_retained_target') else prism.ACTOR))
        u.mem_write(self.kind + 0x8B4, words(0))
        if case.get('reselect_secondary'):
            # Explicit non-stock two-slot selector control: original HasTurrets
            # reads +808 and chooses CurrentWeaponNumber +138 before GetWeapon.
            u.mem_write(self.kind + 0x808, words(2))
            u.mem_write(self.address[self.first] + 0x138, words(1))
            u.mem_write(self.kind + 0x8B4, words(prism.SUPPORT_WEAPON))
            u.mem_write(prism.SUPPORT_WEAPON, bytes(u.mem_read(prism.SHOT_WEAPON, 0x1A0)))
            u.mem_write(prism.SUPPORT_WEAPON + 0x153, b'\1')
            u.mem_write(self.kind + 0x8B8, words(21, 17, 89))
        for key, off in (('is_laser', 0x149), ('is_electric_bolt', 0x151), ('alternate', 0x153)):
            if key in case:
                u.mem_write(prism.SHOT_WEAPON + off, bytes([case[key]]))
        self.invoke(VECTOR_INIT, 0)
        u.mem_write(VECTOR + 4, words(prism.PRISM + 0x47000, 16))
        u.mem_write(VECTOR + 0xD, b'\0')
        self.trace = RngTrace(u, prism.SCENARIO, case.get('seed', 1))
        self.effective_input = dict(self.case,
            is_laser=bool(u.mem_read(prism.SHOT_WEAPON + 0x149, 1)[0]),
            is_electric_bolt=bool(u.mem_read(prism.SHOT_WEAPON + 0x151, 1)[0]),
            alternate=bool(u.mem_read(prism.SHOT_WEAPON + 0x153, 1)[0]),
            fire_at_selected_slot=0, selector_turret_count=signed(self.read32(self.kind + 0x808)),
            current_weapon_number=signed(self.read32(self.address[self.first] + 0x138)),
            secondary_weapon_present=bool(self.read32(self.kind + 0x8B4)),
            secondary_flh=ints(u, self.kind + 0x8B8, 3),
            secondary_alternate=(bool(u.mem_read(prism.SUPPORT_WEAPON + 0x153, 1)[0])
                                 if case.get('reselect_secondary') else None), seed=case.get('seed', 1),
            birth_frame=self.frame0)
        self.effective_input.update(
            retained_target=not case.get('clear_retained_target', False),
            type_turret=bool(u.mem_read(self.kind + 0xCA1, 1)[0]),
            primary_facing_state=ints(u, self.address[self.first] + 0x388, 6),
            secondary_facing_state=ints(u, self.address[self.first] + 0x3A0, 6))
        self.birth_events, self.constructor_states = [], []
        u.hook_add(UC_HOOK_CODE, self.observe_electric)

    def observe_electric(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x62DC50:
            args = [self.read32(sp + i) for i in range(4, 28, 4)]
            self.birth_events.append(dict(event='spark_constructor_boundary',
                coords=ints(u, args[1], 3), target=ints(u, args[4], 3),
                target_object=args[2], owner=args[3], house=args[5]))
            self.ret(24, u.reg_read(UC_X86_REG_ECX))
        if pc in (0x6FD460, 0x6FD570, 0x6F3330, 0x453840, 0x445E50, 0x447AC0,
                  0x4500A0, CTOR, INIT):
            self.birth_events.append(dict(event=hex(pc), caller=hex(self.read32(sp))))
        if pc == INIT:
            self.constructor_states.append(bolt_state(u, u.reg_read(UC_X86_REG_ECX)))

    def execute(self):
        u = self.u
        before = self.trace.states()
        u.mem_write(prism.COORD, words(0, 0, 0, 0))
        for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, prism.COORD),
                          (UC_X86_REG_ESI, self.address[self.first]),
                          (UC_X86_REG_EBX, prism.SHOT_WEAPON), (UC_X86_REG_EDI, prism.ACTOR)):
            u.reg_write(reg, value)
        native.run_checked(u, 0x6FF4CC, 0x6FF656, count=200000)
        assert u.reg_read(UC_X86_REG_ESP) == SP and not self.trace.pending
        count = self.read32(VECTOR + 0x10)
        pointers = [self.read32(self.read32(VECTOR + 4) + i*4) for i in range(count)]
        assert hashlib.sha256(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)).hexdigest() == self.text_sha256
        return dict(input=self.effective_input, events=self.birth_events, allocations=self.events,
                    constructor_states=self.constructor_states,
                    bolts=[bolt_state(u, p) for p in pointers], registered=count,
                    rng_before=before, rng_after=self.trace.states(), rng_requests=self.trace.requests,
                    text_unchanged=True)


def particle_type_state(m, system, particle):
    u = m.u
    colors = m.read32(particle + 0x2BC)
    count = m.read32(particle + 0x2C8)
    return dict(system=dict(holds_what=signed(m.read32(system + 0x294)),
        particle_cap=signed(m.read32(system + 0x2A4)), behavior=signed(m.read32(system + 0x2B4)),
        lifetime=signed(m.read32(system + 0x2B8)),
        spawn_direction_bits=[m.read32(system + 0x2BC + i*4) for i in range(3)],
        spawn_spark_percentage_bits=bytes(u.mem_read(system + 0x2F8, 8)).hex(),
        spark_spawn_frames=signed(m.read32(system + 0x300)), light_size=signed(m.read32(system + 0x304)),
        one_frame_light=bool(u.mem_read(system + 0x30C, 1)[0]),
        logic=bool(u.mem_read(system + 0x234, 1)[0])),
        particle=dict(x_velocity=signed(m.read32(particle + 0x2A0)),
        y_velocity=signed(m.read32(particle + 0x2A4)), min_z_velocity=signed(m.read32(particle + 0x2A8)),
        z_velocity_range=signed(m.read32(particle + 0x2AC)), max_ec=signed(m.read32(particle + 0x2E0)),
        color_speed_bits=bytes(u.mem_read(particle + 0x2B0, 8)).hex(),
        color_list=[list(u.mem_read(colors + i*3, 3)) for i in range(count)],
        start_color1=list(u.mem_read(particle + 0x2D4, 3)), start_color2=list(u.mem_read(particle + 0x2D7, 3)),
        behavior=signed(m.read32(particle + 0x314)), logic=bool(u.mem_read(particle + 0x234, 1)[0])))


def prepare_spark_capacity(m, addresses=(0xA83D68, 0xA83D98, 0xA80208, 0xA83DC8, 0xB0F720, 0xAC1678)):
    """Shared existing empty registry-capacity input; native constructors insert."""
    for address in addresses:
        m.u.mem_write(address, words(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))


def read_spark_types(m, layers, *, cache=None, ini_pointer=RULES, overrides=None,
                     system_name='SparkSys', particle_name='Spark'):
    """Existing original Spark input owner on a caller-owned reader VM.

    The optional Welding mode selects another physical pair without changing
    the historical SparkSys payload or creating a second type reader.
    """
    u, rules = m.u, m.rules
    cache = m.rules_cache if cache is None else cache
    if not hasattr(m, 'reads'):
        m.reads = []
    m.system_type, m.particle_type = m.alloc(0x310), m.alloc(0x318)
    m.invoke(0x6440A0, m.system_type, (m.cstring(system_name),))
    m.invoke(0x644BE0, m.particle_type, (m.cstring(particle_name),))
    m.type_constructor = particle_type_state(m, m.system_type, m.particle_type)
    m.type_layers = []
    system_keys = {}
    for name, archive, raw in layers:
        if raw is None:
            m.type_layers.append(dict(file=name, absent=True))
            continue
        sections, _ = lexical(raw, {system_name, particle_name, 'CombatDamage', 'AudioVisual'})
        # Existing original readers own scalar/reference/default semantics.
        selected = {k: v for k, v in sections.items() if k in (system_name, particle_name)}
        if system_name in selected:
            system_keys.update(selected[system_name])
        selected.update({k: {key: value for key, value in sections.get(k, {}).items() if key in wanted}
                         for k, wanted in [('CombatDamage', {'DefaultSparkSystem'}), ('AudioVisual', {'Gravity'})]
                         if k in sections})
        cache(selected)
        m.reads.clear()
        m.invoke(0x6442D0, m.system_type, (ini_pointer,))
        m.invoke(0x644F50, m.particle_type, (ini_pointer,))
        m.invoke(0x66BBB0, rules, (ini_pointer,))
        u.reg_write(UC_X86_REG_ESP, READER_SP)
        u.reg_write(UC_X86_REG_ESI, rules)
        u.reg_write(UC_X86_REG_EDI, ini_pointer)
        native.run_checked(u, 0x66B3C4, 0x66B3E4)
        m.type_layers.append(dict(file=name, archive=archive, bytes=len(raw),
            sha256=hashlib.sha256(raw).hexdigest(), sections=selected, reads=list(m.reads),
            state=particle_type_state(m, m.system_type, m.particle_type),
            default_spark_is_selected=m.read32(rules + 0x1020) == m.system_type,
            gravity=signed(m.read32(rules + 0x16B8))))
    if system_name == 'SparkSys':
        assert m.read32(rules + 0x1020) == m.system_type
    if overrides:
        cache({system_name: dict(system_keys, **overrides)})
        m.invoke(0x6442D0, m.system_type, (ini_pointer,))
    m.effective_types = particle_type_state(m, m.system_type, m.particle_type)


class SparkBirth(BulletReader):
    """Cached retail readers and actual original Init/Spark world continuation."""
    def __init__(self, case, endpoint, *, layers=None, system_name='SparkSys',
                 particle_name='Spark'):
        self.runtime = False
        super().__init__({})
        self.case, self.endpoint = case, endpoint
        u = self.u
        self.rules = rules = self.alloc(0x2000)
        u.mem_write(0x8871E0, words(rules))
        self.invoke(0x665650, rules)
        # Actual static Logic/Display constructors and supplied empty pool capacity.
        self.invoke(0x40CB80, 0)
        self.invoke(0x4A8630, 0)
        prepare_spark_capacity(self)
        read_spark_types(self, physical_layers() if layers is None else layers,
            overrides=case.get('system_type_overrides'), system_name=system_name,
            particle_name=particle_name)
        initialize_effect_registries(self)
        table = self.alloc(0x100000)
        u.mem_write(0x87F924, words(table, 262144))
        cells = {}
        for y in range(46, 53):
            for x in range(case.get('cell_x_min', 42), 49):
                cell = self.alloc(0x160)
                cells[x, y] = cell
                u.mem_write(cell, words(0x7E4EEC))
                u.mem_write(cell + 0x24, struct.pack('<2h', x, y))
                u.mem_write(table + (y*512 + x)*4, words(cell))
        u.mem_write(0xABDC50, words(0x7E4EEC))
        initialize_effect_world(self, cells)
        self.cells = cells
        u.mem_write(0xA8EB78, words(case.get('detail', 2)))
        u.mem_write(0x89E7C0, words(104))
        u.mem_write(0xAC4A8C, words(416))
        u.mem_write(prism.FRAME, words(case.get('frame', 36)))
        self.scenario = self.read32(0xA8B230)
        u.mem_write(self.scenario + 0x214, words(case.get('native_id', 1000)))
        self.trace = RngTrace(u, self.scenario, case.get('seed', 1))
        self.invoke(VECTOR_INIT, 0)
        self.events, self.returns, self.systems, self.particles, self.lights = [], [], [], [], []
        self.crc_words, self.phase = [], 'birth'
        self.text_sha = hashlib.sha256(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)).hexdigest()
        self.runtime = True

    def observe_return(self, pc, sp):
        """Observe the actual native return, including a stopped sentinel."""
        if self.returns and (pc, sp) == self.returns[-1][:2]:
            _, _, row, pointer = self.returns.pop()
            row['returned_eax'] = self.u.reg_read(UC_X86_REG_EAX)
            if row['pc'] in ('0x62dc50', '0x62b5e0'):
                row['native_id'] = signed(self.read32(pointer + 0x10))
                row['rng_requests_at_return'] = len(self.trace.requests)

    def hook(self, u, pc, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x7C978A:
            self.ret(0)
            return
        if self.runtime:
            if pc == prism.OPERATOR_NEW and self.case.get('fail_system_allocation') and self.read32(sp + 4) == 0x100:
                self.events.append(dict(phase=self.phase, pc=hex(pc), allocation_failure_bytes=0x100))
                self.ret(0)
                return
            self.observe_return(pc, sp)
            cleanups = {0x62DC50:24, 0x62B5E0:16, 0x68BCB0:0,
                        0x5F4EC0:8, 0x55BAA0:8, 0x5FF250:16}
            if pc in cleanups:
                pointer = u.reg_read(UC_X86_REG_ECX)
                row = dict(phase=self.phase, pc=hex(pc), caller=hex(self.read32(sp)),
                           native_id_before=signed(self.read32(self.scenario + 0x214)),
                           rng_requests_at_entry=len(self.trace.requests))
                if pc in (0x62DC50, 0x62B5E0):
                    (self.systems if pc == 0x62DC50 else self.particles).append(pointer)
                    row['position'] = ints(u, self.read32(sp + 8), 3)
                if pc == 0x5FF250:
                    self.lights.append(pointer)
                    row['arguments'] = ints(u, sp + 4, 4)
                self.events.append(row)
                self.returns.append((self.read32(sp), sp + 4 + cleanups[pc], row, pointer))
            if pc in (0x62FD60, 0x62E840, 0x62CE40, 0x62C6E0, 0x410410, 0x630100):
                self.events.append(dict(phase=self.phase, pc=hex(pc), caller=hex(self.read32(sp)),
                    native_id=signed(self.read32(u.reg_read(UC_X86_REG_ECX) + 0x10))))
            if pc == 0x4A1D50:
                self.crc_words.append(dict(caller=hex(self.read32(sp)), value=signed(self.read32(sp + 4))))
        super().hook(u, pc, size, data)

    def runtime_state(self):
        u = self.u
        return dict(native_id_cursor=signed(self.read32(self.scenario + 0x214)),
            logic_native_ids=[signed(self.read32(self.read32(self.read32(0x87F77C) + i*4) + 0x10))
                              for i in range(self.read32(0x87F788))],
            systems=[dict(native_id=signed(self.read32(p + 0x10)), position=ints(u, p + 0x9C, 3),
                target=ints(u, p + 0xD4, 3), owner=self.read32(p + 0xE0), target_object=self.read32(p + 0xE4),
                house=self.read32(p + 0xFC), lifetime=signed(self.read32(p + 0xEC)),
                spark_spawn_frames=signed(self.read32(p + 0xF0)), particle_count=self.read32(p + 0xCC),
                particle_native_ids=[signed(self.read32(self.read32(self.read32(p + 0xC0) + i*4) + 0x10))
                                     for i in range(self.read32(p + 0xCC))],
                time_to_die=bool(u.mem_read(p + 0xF8, 1)[0]), logic=bool(u.mem_read(p + 0x98, 1)[0]))
                for p in self.systems],
            particles=[dict(native_id=signed(self.read32(p + 0x10)), position=ints(u, p + 0x9C, 3),
                velocity_bits=[self.read32(p + 0x10C + i*4) for i in range(3)],
                color=list(u.mem_read(p + 0xB0, 3)), color_index=signed(self.read32(p + 0xB4)),
                color_factor_bits=bytes(u.mem_read(p + 0xB8, 8)).hex(),
                remaining_ec=struct.unpack('<H', u.mem_read(p + 0x128, 2))[0],
                logic=bool(u.mem_read(p + 0x98, 1)[0]), time_to_die=bool(u.mem_read(p + 0x131, 1)[0]))
                for p in self.particles],
            lights=[dict(position=ints(u, p, 3), ramp=signed(self.read32(p + 12)),
                         size=signed(self.read32(p + 16)), flags=self.read32(p + 20)) for p in self.lights],
            light_registry_count=self.read32(0xAC1688))

    def logic_visit(self, phase, frame):
        self.phase = phase
        self.trace.requests.clear()
        before = self.trace.states()
        self.u.reg_write(UC_X86_REG_ESP, READER_SP)
        self.u.reg_write(UC_X86_REG_EDI, 0x87F778)
        self.u.mem_write(prism.FRAME, words(frame))
        # At count zero the full caller's pre-gate skips this body.
        if self.read32(0x87F788):
            native.run_checked(self.u, 0x55B5FF, 0x55B61B, count=500000)
        return dict(state=self.runtime_state(), rng_before=before, rng_after=self.trace.states(),
                    rng_requests=list(self.trace.requests))

    def execute(self):
        before = self.trace.states()
        bolt = self.alloc(0x30)
        self.invoke(CTOR, bolt)
        self.invoke(INIT, bolt, (*self.endpoint['source'], *self.endpoint['target'], self.endpoint['z_adjust']))
        birth = dict(bolt=bolt_state(self.u, bolt), state=self.runtime_state(),
                     rng_before=before, rng_after=self.trace.states(), rng_requests=list(self.trace.requests))
        first = self.logic_visit('first_logic_visit', self.case.get('frame', 36))
        second = self.logic_visit('second_logic_visit', self.case.get('frame', 36) + 1)
        self.phase = 'checksum'
        crc = self.alloc(0x20)
        if self.systems and self.u.mem_read(self.systems[0] + 0x90, 1)[0]:
            self.invoke(0x630100, self.systems[0], (crc,))
        checksum = dict(bytes=bytes(self.u.mem_read(crc, 0x20)).hex(), words=list(self.crc_words))
        self.phase = 'light_updates'
        light_updates = []
        for _ in range(10):
            self.invoke(0x5FF390, 0)
            light_updates.append(self.runtime_state()['lights'] + [dict(registry_count=self.read32(0xAC1688))])
        assert not self.trace.pending and not self.returns
        assert second['rng_after'] == self.trace.states()
        assert hashlib.sha256(self.u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)).hexdigest() == self.text_sha
        return dict(input=self.case, endpoint=self.endpoint, type_constructor=self.type_constructor,
                    type_layers=self.type_layers, effective_types=self.effective_types,
                    birth=birth, first_logic_visit=first,
                    second_logic_visit=second,
                    events=self.events, checksum=checksum, light_updates=light_updates, text_unchanged=True)


class LiveSparkBirth(SparkBirth):
    """Actual Inviso Bullet predecessor, impact/retirement and live Spark AI.

    FireAt's BulletFire6FF014 precedes SpawnElectricBolt6FF58A. This component
    supplies that established producer order; it does not execute the omitted
    caller prefix, source Building AI or a damageable target receiver.
    """
    def __init__(self, endpoint, *, occupied=False, charge_birth_frame=None,
                 charge_type='NATSLA_B'):
        self.occupied = occupied
        self.bullet, self.impact_anims = 0, []
        self.charge, self.charge_warmup = None, []
        super().__init__(dict(name='inviso_bullet_before_spark', seed=1, native_id=1000,
            frame=36, detail=2, cell_x_min=38, map_size=[64,64], game_speed=0,
            target_location=[11392,12416,0], target_foundation_index=3,
            firer=None, incoming_velocity=[1.0,0.0,0.0],
            damage_receivers='empty; supplied stationary Building is a coordinate receiver only'), endpoint)
        if occupied:
            self.case.update(name='inviso_bullet_before_spark_occupied_gapowr',
                target_occupancy_before_second_visit=[[44,48], [45,48], [44,49], [45,49]],
                target_contact_runtime=dict(health=750, alive=True, laser_fence_stage=0,
                                            next_object=None, layer='ground'),
                damage_receivers='empty during Bullet impact; target receives no damage',
                occupancy_boundary='Supplied cell object lists after Bullet impact; no Building placement or damage receiver')
        if charge_birth_frame is not None:
            assert not occupied
            assert charge_birth_frame in (9, 10) and charge_type in ('NATSLA_B', 'NATSLA_BD')
            self.case.update(
                name=('damaged_charge_before_inviso_bullet' if charge_type == 'NATSLA_BD'
                      else 'natural_charge_expiry_before_inviso_bullet' if charge_birth_frame == 9
                      else 'live_charge_before_inviso_bullet'),
                charge_birth_frame=charge_birth_frame,
                charge_type=charge_type, damaged_charge=charge_type == 'NATSLA_BD',
                charge_position=[9728, 12288, 0],
                charge_constructor=dict(delay=0, loop_count=1, draw_flags=0x1600),
                charge_building_slot_marker=True,
                charge_boundary='Supplied charge birth frame and charge-before-Bullet order; no source Building AI or expiry listener')
        self.runtime = False
        u = self.u
        _, weapon, self.live_weapon_inputs = physical_weapon(self, read_warhead=True)
        projectile, warhead = self.read32(weapon + 0xA0), self.read32(weapon + 0xAC)
        root = native.configured_gamemd().parent
        ra2, yr = mix((root / 'ra2.mix').read_bytes()), mix((root / 'ra2md.mix').read_bytes())
        art_raw = mix(yr[mix_hash('localmd.mix')])[mix_hash('artmd.ini')]
        shapes = mix(ra2[mix_hash('conquer.mix')])
        self.impact_type_inputs = []
        for i in range(self.read32(warhead + 0x114)):
            pointer = self.read32(self.read32(warhead + 0x108) + i*4)
            name = self.string(pointer + 0x24)
            art, _ = lexical(art_raw, {name})
            raw = shapes[mix_hash(name + '.SHP')]
            self.assets[(name + '.SHP').upper()] = raw
            self.make_ini(art)
            self.reads.clear()
            self.asset_loaded.clear()
            admitted = self.invoke(0x427D00, pointer, (INI,))
            self.impact_type_inputs.append(dict(name=name,
                art=dict(archive='ra2md.mix/localmd.mix', file='ARTMD.INI',
                         sha256=hashlib.sha256(art_raw).hexdigest(), sections=art),
                shape=dict(archive='ra2.mix/conquer.mix', file=name + '.SHP',
                           bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest()),
                native=self.result(name, pointer, admitted), reads=list(self.reads)))
        u.mem_write(0xA8ED40, words(0x7EB6D4, self.alloc(4096), 1024, 1, 0, 10))
        # The same bounded Building coordinate receiver as the birth fixture.
        # Its actual native virtual getters run; it is not an AreaDamage member.
        target, target_type = self.alloc(0x800), self.alloc(0x1800)
        self.target, self.target_type = target, target_type
        u.mem_write(target, words(0x7E3EBC))
        u.mem_write(target + 0x14, words(3))
        u.mem_write(target + 0x520, words(target_type))
        u.mem_write(target + 0x9C, words(*self.case['target_location']))
        u.mem_write(target + 0x6C, words(750))
        u.mem_write(target + 0x90, b'\1')
        u.mem_write(target_type + 0xEF0, words(self.case['target_foundation_index']))
        u.mem_write(0x87F7E8 + 0xF4, words(*self.case['map_size']))
        u.mem_write(0xA8EB60, words(self.case['game_speed']))
        u.mem_write(self.scenario + 0x214, words(self.case['native_id']))
        for i, pointer in enumerate(self.trace.streams.values()):
            u.mem_write(pointer, seeded_struct(self.case['seed'] + i))
        self.trace.requests.clear()
        self.events.clear()
        self.live_events, self.transports = [], []
        self.runtime = True
        self.phase = 'bullet_birth'
        u.hook_add(UC_HOOK_CODE, self.observe_live)
        if charge_birth_frame is not None:
            self.admit_charge_predecessor()
        self.bullet = self.alloc(0x180)
        before = self.trace.states()
        self.invoke(0x466380, self.bullet)
        self.invoke(0x46AFD0, 0, (self.bullet,))
        self.invoke(0x4664C0, self.bullet, (projectile, target, 0,
                    self.read32(weapon + 0xA4), warhead, self.read32(weapon + 0xA8), 0))
        u.mem_write(self.bullet + 0x130, words(weapon))
        native.initialize_empty_windows_seh(u)
        placement = fire_bullet(self, self.bullet, endpoint['source'], self.case['incoming_velocity'])
        self.bullet_birth = dict(placement=placement, state=self.runtime_state(),
            rng_before=before, rng_after=self.trace.states(), rng_requests=list(self.trace.requests),
            projectile_type=self.bullet_state(projectile))
        self.trace.requests.clear()
        self.phase = 'birth'

    def charge_state(self):
        p, u = self.charge, self.u
        return dict(native_id=signed(self.read32(p + 0x10)),
            alive=bool(u.mem_read(p + 0x90, 1)[0]),
            logic=bool(u.mem_read(p + 0x98, 1)[0]),
            frame=signed(self.read32(p + 0xAC)),
            timer_start=signed(self.read32(p + 0xB4)),
            timer_duration=signed(self.read32(p + 0xBC)),
            completed=bool(u.mem_read(p + 0x179, 1)[0]),
            first_ai_guard=bool(u.mem_read(p + 0x19C, 1)[0]))

    def admit_charge_predecessor(self):
        """Physical charge AI, never a supplied expiry/unregister result.

        Building451970 supplies delay0/loops1/flags1600 and45199B sets
        Anim+118. The source Building and its listener are outside this
        control; building_slot_replacement.expiry owns the generic delayed
        fire completion callback that restores Active3.
        """
        u = self.u
        root = native.configured_gamemd().parent
        ra2 = mix((root / 'ra2.mix').read_bytes())
        yr = mix((root / 'ra2md.mix').read_bytes())
        art_raw = mix(yr[mix_hash('localmd.mix')])[mix_hash('artmd.ini')]
        # Both variants use Image=NATSLA_B; ObjectType reads that section
        # before AnimType reads its own normal/damaged section.
        sections, _ = lexical(art_raw, {self.case['charge_type'], 'NATSLA_B'})
        shape_raw = mix(ra2[mix_hash('generic.mix')])[mix_hash('NGTSLA_B.SHP')]
        self.assets['NGTSLA_B.SHP'] = shape_raw
        self.make_ini(sections)
        p = self.invoke(0x428B80, self.cstring(self.case['charge_type']))
        self.asset_loaded.clear()
        self.reads.clear()
        admitted = self.invoke(0x427D00, p, (INI,))
        self.charge_inputs = dict(
            art=dict(archive='ra2md.mix/localmd.mix', file='ARTMD.INI',
                     sha256=hashlib.sha256(art_raw).hexdigest(), sections=sections),
            shape=dict(archive='ra2.mix/generic.mix', file='NGTSLA_B.SHP',
                       bytes=len(shape_raw), sha256=hashlib.sha256(shape_raw).hexdigest()),
            native=self.result(self.case['charge_type'], p, admitted),
            reads=list(self.reads), loads=list(self.asset_loaded))
        assert self.charge_inputs['native']['raw_shp_frame_count']
        u.mem_write(prism.FRAME, words(self.case['charge_birth_frame']))
        position = self.alloc(12)
        u.mem_write(position, words(*self.case['charge_position']))
        self.phase = 'charge_birth'
        self.charge = self.alloc(0x1C8)
        before = self.trace.states()
        mark = len(self.trace.requests)
        self.invoke(0x421EA0, self.charge, (p, position, 0, 1, 0x1600, 0, 0))
        u.mem_write(self.charge + 0x118, b'\1')
        self.charge_birth = dict(state=self.charge_state(), rng_before=before,
            rng_after=self.trace.states(), rng_requests=list(self.trace.requests[mark:]))
        before = self.trace.states()
        requests = []
        for frame in range(self.case['charge_birth_frame'], self.case['frame']):
            visit = SparkBirth.logic_visit(self, 'charge_warmup', frame)
            self.charge_warmup.append(dict(frame=frame, state=self.charge_state()))
            requests.extend(dict(frame=frame, **row) for row in visit['rng_requests'])
        self.charge_warmup_rng = dict(before=before, after=self.trace.states(), requests=requests)
        assert self.charge_state()['alive']
        u.mem_write(prism.FRAME, words(self.case['frame']))
        self.trace.requests.clear()
        self.phase = 'bullet_birth'

    def observe_live(self, u, pc, _size, _data):
        if bullet_addref_transport(self, pc, self.transports) or retirement_transport(self, pc, self.transports):
            return
        if pc == 0x421EA0 and u.reg_read(UC_X86_REG_ECX) != self.charge:
            self.impact_anims.append(u.reg_read(UC_X86_REG_ECX))
        if self.charge is not None and pc in (0x424B31, 0x4255B0, 0x7258D0):
            self.live_events.append(dict(phase=self.phase, pc=hex(pc),
                charge=self.charge_state(),
                logic_native_ids=self.runtime_state()['logic_native_ids']))
        if self.occupied and pc in (0x62C888, 0x62C8C8, 0x62C8D2, 0x62CA34, 0x62ED0E):
            sp = u.reg_read(UC_X86_REG_ESP)
            row = dict(phase=self.phase, pc=hex(pc))
            if pc != 0x62ED0E:
                row['native_id'] = signed(self.read32(u.reg_read(UC_X86_REG_EBP) + 0x10))
            if pc == 0x62C888:
                row.update(candidate=ints(u, sp + 0x50, 3),
                           found_target=u.reg_read(UC_X86_REG_EAX) == self.target)
            elif pc == 0x62C8C8:
                row['one_cell_undeployer'] = bool(u.reg_read(UC_X86_REG_EAX) & 255)
            elif pc == 0x62C8D2:
                row['accepted_building_or_wall'] = bool(u.reg_read(UC_X86_REG_EBX) & 255)
            self.collision_events.append(row)
        if pc in (0x466380,0x4664C0,0x468670,0x4666E0,0x468D80,0x4690B0,
                  0x489280,0x5F65F0,0x5F4D30,0x55BAE0,0x55BAA0,0x62DC50,
                  0x62FD60,0x62E840,0x55B610,0x421EA0,0x423AC0,0x725C70,0x466560):
            pointer = u.reg_read(UC_X86_REG_ECX)
            row = dict(phase=self.phase, pc=hex(pc),
                caller=hex(self.read32(u.reg_read(UC_X86_REG_ESP))),
                logic_native_ids=self.runtime_state()['logic_native_ids'])
            if pc == 0x55B610:
                row.update(index=u.reg_read(UC_X86_REG_ESI),
                           native_id=signed(self.read32(pointer + 0x10)))
            elif self.occupied and pc in (0x5F65F0, 0x5F4D30, 0x55BAE0):
                row['native_id'] = signed(self.read32(pointer + 0x10))
            self.live_events.append(row)

    def runtime_state(self):
        out = super().runtime_state()
        u = self.u
        out['bullet'] = None if not self.bullet else dict(
            native_id=signed(self.read32(self.bullet + 0x10)),
            position=ints(u, self.bullet + 0x9C, 3), alive=bool(u.mem_read(self.bullet + 0x90,1)[0]),
            logic=bool(u.mem_read(self.bullet + 0x98,1)[0]))
        out['impact_anims'] = [dict(native_id=signed(self.read32(p + 0x10)),
            position=ints(u,p + 0x9C,3), alive=bool(u.mem_read(p + 0x90,1)[0]),
            logic=bool(u.mem_read(p + 0x98,1)[0]), frame=signed(self.read32(p + 0xAC)),
            timer_start=signed(self.read32(p + 0xB4)), timer_duration=signed(self.read32(p + 0xBC)))
            for p in self.impact_anims]
        out['pending_native_ids'] = [signed(self.read32(self.read32(self.read32(0xB0F69C) + i*4) + 0x10))
                                     for i in range(self.read32(0xB0F6A8))]
        if self.occupied:
            for row, pointer in zip(out['systems'], self.systems):
                row['alive'] = bool(u.mem_read(pointer + 0x90, 1)[0])
        if self.charge is not None:
            out['charge_anim'] = self.charge_state()
        return out

    def logic_visit(self, phase, frame):
        if phase == 'second_logic_visit':
            self.phase = 'between_passes_deferred_drain'
            self.u.mem_write(prism.FRAME, words(frame))
            before = self.trace.states()
            mark = len(self.trace.requests)
            self.invoke(0x725C70, 0)
            self.between_passes = dict(state=self.runtime_state(), rng_before=before,
                rng_after=self.trace.states(), rng_requests=list(self.trace.requests[mark:]))
            if self.occupied:
                self.target_occupancy = spark_target_inputs()
                fields = self.target_occupancy['result']
                assert not fields['undeploys_into_present']
                self.u.mem_write(self.target_type + 0x16BF, bytes([fields['laser_fence']]))
                self.u.mem_write(self.target_type + 0x408, words(0))
                self.u.mem_write(self.target_type + 0xEF0, words(fields['foundation_index']))
                for cell in self.case['target_occupancy_before_second_visit']:
                    assert self.read32(self.cells[tuple(cell)] + 0xE4) == 0
                    self.u.mem_write(self.cells[tuple(cell)] + 0xE4, words(self.target))
                self.collision_events = []
        result = super().logic_visit(phase, frame)
        result['binary_frame'] = frame
        if phase == 'second_logic_visit' and self.occupied:
            self.phase = 'after_second_pass_deferred_drain'
            before, mark = self.trace.states(), len(self.trace.requests)
            self.invoke(0x725C70, 0)
            self.after_second_pass_deferred_drain = dict(state=self.runtime_state(),
                rng_before=before, rng_after=self.trace.states(),
                rng_requests=list(self.trace.requests[mark:]))
        return result

    def execute(self):
        out = super().execute()
        out.update(bullet_birth=self.bullet_birth, weapon_inputs=self.live_weapon_inputs,
            impact_type_inputs=self.impact_type_inputs, between_passes=self.between_passes,
            live_events=self.live_events, transports=self.transports,
            caller_order=dict(bullet_fire='0x6ff014', electric_spawn='0x6ff58a'))
        if self.charge is not None:
            out.update(charge_inputs=self.charge_inputs, charge_birth=self.charge_birth,
                       charge_warmup=self.charge_warmup, charge_warmup_rng=self.charge_warmup_rng)
        if self.occupied:
            out.update(target_occupancy=self.target_occupancy, collision_events=self.collision_events,
                after_second_pass_deferred_drain=self.after_second_pass_deferred_drain)
            out['checksum']['skipped'] = 'No live particle system after native retirement'
            assert hashlib.sha256(self.u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)).hexdigest() == self.text_sha
        return out


class ElectricPixels(prism.LaserPixels):
    """Reuse the one original RGB565/A/Z surface fixture and packed primitive."""
    def __init__(self, case):
        self.active = False
        super().__init__(case)
        self.active = True
        u = self.u
        self.heap_cursor = prism.HEAP + 0x1000
        self.bolts, self.deleted, self.spark_calls, self.draw_order, self.clips = [], [], [], [], []
        self.calls = []
        self.scenario = prism.PRISM + 0x50000
        u.mem_write(0xA8B230, words(self.scenario))
        u.mem_write(self.scenario + 0x214, words(case.get('native_id', 1000)))
        self.trace = RngTrace(u, self.scenario, case.get('seed', 1))
        self.call(VECTOR_INIT)
        u.mem_write(VECTOR + 4, words(prism.PRISM + 0x47000, 16))
        u.mem_write(VECTOR + 0xD, b'\0')
        u.mem_write(0x88731C, words(rally.SURFACE))
        palette, table = prism.PRISM + 0x48000, prism.PRISM + 0x49000
        u.mem_write(palette + 4, words(2))
        u.mem_write(palette + 0x174, words(table))
        u.mem_write(table, bytes.fromhex(physical_palette()['table_hex']))
        u.mem_write(0x87F6C4, words(palette))
        rules = prism.PRISM + 0x4A000
        u.mem_write(0x8871E0, words(rules))
        # Init dereferences only DefaultSparkSystem before the explicitly
        # recorded constructor boundary in these isolated drawing controls.
        u.mem_write(rules + 0x1020, words(0))
        self.birth_before = self.trace.states()
        specs = case.get('bolts', [case])
        self.births = []
        for spec in specs:
            p = self.allocate(0x30)
            self.call(CTOR, p)
            ctor = bolt_state(u, p)
            self.call(INIT, p, *spec.get('source', [3038, 3038, 0]),
                      *spec.get('target', [3600, 2700, 0]), spec.get('z_adjust', -4))
            u.mem_write(p + 0x2C, bytes([spec.get('alternate', False)]))
            for key, off in (('phase', 0x1C), ('decay', 0x28)):
                if key in spec:
                    u.mem_write(p + off, words(spec[key]))
            self.bolts.append(p)
            self.births.append(dict(constructor=ctor, bolt=bolt_state(u, p)))
        self.birth_after = self.trace.states()
        self.birth_requests = list(self.trace.requests)
        self.trace.requests.clear()
        self.trail_input = None
        if case.get('mixed_families'):
            self.call(0x556940)
            self.call(0x5569A0)
            self.trail = self.allocate(0x210)
            owner = self.allocate(0xB0)
            self.call(0x556A20, self.trail)
            u.mem_write(self.trail, bytes(case.get('trail_rgb', [220, 180, 90])))
            u.mem_write(self.trail + 4, words(owner))
            for point in (case['source'], case['target']):
                u.mem_write(owner + 0x9C, words(*point))
                self.call(0x556B70, self.trail)
            self.trail_input = dict(owner_position=ints(u, owner + 0x9C, 3),
                rgb=list(u.mem_read(self.trail, 3)), decrement=signed(u32(u, self.trail + 8)),
                head=u32(u, self.trail + 12),
                ring=[dict(xyz=ints(u, self.trail + 16 + i*16, 3),
                           strength=signed(u32(u, self.trail + 28 + i*16))) for i in range(32)])

    def allocate(self, size):
        p = self.heap_cursor
        self.heap_cursor += (size + 15) & ~15
        assert self.heap_cursor < prism.HEAP_END
        return p

    def return_boundary(self, cleanup=0, value=0):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, u32(u, sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def observe(self, u, pc, size, data):
        super().observe(u, pc, size, data)
        if not self.active:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == prism.OPERATOR_NEW:
            self.return_boundary(value=self.allocate(u32(u, sp + 4)))
        elif pc == prism.OPERATOR_DELETE:
            self.deleted.append(u32(u, sp + 4))
            self.return_boundary()
        elif pc == 0x62DC50:
            args = [u32(u, sp + i) for i in range(4, 28, 4)]
            self.spark_calls.append(dict(coords=ints(u, args[1], 3), target=ints(u, args[4], 3),
                                         target_object=args[2], owner=args[3], house=args[5]))
            self.return_boundary(24, u.reg_read(UC_X86_REG_ECX))
        elif pc == DRAW:
            self.draw_order.append(self.bolts.index(u.reg_read(UC_X86_REG_ECX)))
        elif pc == 0x7BC2B0 and u32(u, sp) == 0x4C28F9:
            self.clips.append(dict(from_point=ints(u, u.reg_read(UC_X86_REG_ECX), 2),
                                   to_point=ints(u, u.reg_read(UC_X86_REG_EDX), 2)))
        elif pc == 0x4C28F9:
            self.clips[-1]['accepted'] = bool(u.reg_read(UC_X86_REG_EAX) & 255)
        if pc == 0x4BEAC0:
            args = [u32(u, sp + i) for i in range(4, 32, 4)]
            self.calls.append(dict(entry=hex(pc), clip=ints(u, args[0], 4),
                from_point=ints(u, args[1], 2), to_point=ints(u, args[2], 2),
                rgb=list(u.mem_read(args[3], 3)), intensity=args[4],
                z_start=signed(args[5]), z_end=signed(args[6])))
        if self.case.get('mixed_families') and pc in (0x4BEAC0, prism.LASER_PACKED, prism.LASER_ADDITIVE):
            self.calls[-1]['family'] = self.family

    def registered(self):
        return [u32(self.u, u32(self.u, VECTOR + 4) + i*4)
                for i in range(u32(self.u, VECTOR + 0x10))]

    def visit(self, clear=False, frame=200):
        u = self.u
        before = self.trace.states()
        native_id_before = signed(u32(u, self.scenario + 0x214))
        self.calls, self.draw_order, self.clips = [], [], []
        self.trace.requests.clear()
        u.mem_write(prism.FRAME, words(frame))
        stages = []
        if self.case.get('mixed_families') and not clear:
            self.family = 'laser'
            self.call(prism.LASER_DRAW_ALL)
            stages.append(dict(family='laser', segment_end=len(self.calls),
                               pixel_sha256=hashlib.sha256(u.mem_read(rally.PIXELS, self.pixel_bytes)).hexdigest()))
            self.draw_order = []
        self.family = 'bolt'
        if clear:
            u.reg_write(UC_X86_REG_ESP, SP)
            native.run_checked(u, 0x53493F, 0x534944, count=200000)
        else:
            self.call(MANAGER)
        if self.case.get('mixed_families') and not clear:
            stages.append(dict(family='bolt', segment_end=len(self.calls),
                               pixel_sha256=hashlib.sha256(u.mem_read(rally.PIXELS, self.pixel_bytes)).hexdigest()))
            self.family = 'trail'
            self.call(0x556D40)
            stages.append(dict(family='trail', segment_end=len(self.calls),
                               pixel_sha256=hashlib.sha256(u.mem_read(rally.PIXELS, self.pixel_bytes)).hexdigest()))
        assert not self.trace.pending
        raw = bytes(u.mem_read(rally.PIXELS, self.pixel_bytes))
        assert bytes(u.mem_read(self.z_pixels, self.pixel_bytes)) == self.before_z
        assert bytes(u.mem_read(rally.ALPHA, self.pixel_bytes)) == struct.pack('<'+'H'*len(self.surface.alpha), *self.surface.alpha)
        assert bytes(u.mem_read(rally.PIXELS - 32, 32)) == self.guard
        assert bytes(u.mem_read(rally.PIXELS + self.pixel_bytes, 32)) == self.guard
        assert hashlib.sha256(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)).hexdigest() == self.text_sha256
        w, _ = self.surface.size
        registered = self.registered()
        return dict(clear=clear, binary_frame=frame, draw_order=list(self.draw_order), clips=list(self.clips),
                    stages=stages,
                    native_id_before=native_id_before,
                    native_id_after=signed(u32(u, self.scenario + 0x214)),
                    rng_before=before, rng_after=self.trace.states(), rng_requests=list(self.trace.requests),
                    segments=list(self.calls), registered=[self.bolts.index(p) for p in registered],
                    bolts=[bolt_state(u, p) for p in self.bolts],
                    freed=[self.bolts.index(p) for p in self.deleted if p in self.bolts],
                    pixels=[[i % w, i // w, x[0]] for i, x in enumerate(struct.iter_unpack('<H', raw))
                            if raw[i*2:i*2+2] != self.before[i*2:i*2+2]],
                    pixel_sha256=hashlib.sha256(raw).hexdigest(), z_unchanged=True,
                    alpha_unchanged=True, guard_unchanged=True, text_unchanged=True)


def draw_case(case):
    case = dict(native_id=1000, **case)
    m = ElectricPixels(case)
    return dict(input=case, births=m.births, birth_rng_before=m.birth_before,
                birth_rng_after=m.birth_after, birth_rng_requests=m.birth_requests,
                spark_constructor_boundary=m.spark_calls,
                laser=prism.laser_state(m.u, m.pointer) if case.get('mixed_families') else None,
                trail=m.trail_input,
                visits=[m.visit(clear=(i == case.get('clear_at')), frame=case.get('frame', 200))
                        for i in range(case.get('visits', 1))])


def draw_cases(endpoint):
    base = dict(source=[3038, 3038, 0], target=[3600, 2700, 0], z_adjust=-34,
                camera=[-60, 250], seed=1)
    cases = [dict(base, name='ordinary'), dict(base, name='alternate', alternate=True),
             dict(base, name='mixed_alpha_depth', alpha='mixed', z=[65535, 32760, 32790]),
             dict(base, name='zero_length', target=base['source']),
             dict(base, name='offscreen', camera=[3000,3000], visits=18),
             dict(base, name='lifetime_same_frame', visits=18),
             dict(base, name='clear_live', visits=3, clear_at=1),
             dict(base, name='phase_wrap', phase=2147483647, visits=2),
             dict(base, name='zero_decay', decay=0),
             dict(base, name='last_decay', decay=1, visits=2),
             dict(base, name='negative_phase', phase=-1),
             dict(base, name='reverse_order', bolts=[dict(base), dict(base, alternate=True, phase=9)], visits=2)]
    cases += [dict(base, name='odd_signed_midpoint', source=[-79, 3039, 101], target=[483, 2700, 37], camera=[-420, 85]),
              dict(base, name='accepted_clip', camera=[20, 300]),
              dict(base, name='production_endpoints', source=endpoint['source'], target=endpoint['target'],
                   z_adjust=endpoint['z_adjust'], camera=[-330, 1220]),
              dict(base, name='caller_coordinate_residual',
                   source=[endpoint['source'][0] - 1, *endpoint['source'][1:]],
                   target=endpoint['target'], z_adjust=endpoint['z_adjust'], camera=[-330, 1220],
                   native_source=endpoint['source'], supplied_source_delta=[-1, 0, 0],
                   coordinate_boundary='Shared caller coordinate residual; no upstream GetFLH equality claim')]
    for length in (1,63,64,65,127,128,129,255,256,257,16384,16385):
        cases.append(dict(base, name=f'length_{length}', target=[3038 + length,3038,0]))
    return [draw_case(c) for c in cases]


def light_indices():
    """Execute Draw's projection/gates/index prefix; stop before unsafe lookup."""
    m = ElectricPixels(dict(name='light_indices', source=[3038,3038,0],
                            target=[3600,2700,0], camera=[-60,250]))
    u = m.u
    u.mem_write(0xAC1678, words(0x7EB6D4, m.allocate(64), 16, 1, 0, 10))
    pointer = m.allocate(0x18)
    m.call(0x5FF250, pointer, 3038, 3038, 0, 15)
    u.mem_write(0x887314, words(rally.SURFACE))
    rows = []
    for size in (15, 64, 256, 65536, 2147483647, -1):
        for stage in range(0, 80, 8):
            u.mem_write(pointer + 12, words(stage, size))
            u.mem_write(SP, words(native.RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, pointer)
            native.run_checked(u, 0x5FF850, 0x5FF93F, count=10000)
            rows.append(dict(size=size, stage=stage, index=signed(u.reg_read(UC_X86_REG_ECX))))
    return dict(stop_before_lookup='0x5ff93f', stage_table_hex=bytes(u.mem_read(0x83358C, 80)).hex(), rows=rows)


def light_admission_case(case):
    """Original reverse manager and complete admission, with draw-body boundary."""
    m = ElectricPixels(case)
    u = m.u
    u.mem_write(0xAC1678, words(0x7EB6D4, m.allocate(64), 16, 1, 0, 10))
    pointers = []
    for spec in case['lights']:
        pointer = m.allocate(0x18)
        m.call(0x5FF250, pointer, *spec['position'], 15)
        u.mem_write(pointer + 20, words(spec['flags']))
        pointers.append(pointer)
    u.mem_write(0xABCD44, words(case['fps']))
    u.mem_write(0xABCD50, bytes([case['reduced']]))
    u.mem_write(0x829FF4, words(case['minimum'], case['buffer']))
    u.mem_write(m.scenario, words(case['scenario_flags']))
    visits, entries = [], []
    def observe(u, pc, _size, _data):
        if pc == 0x5FF850:
            pointer = u.reg_read(UC_X86_REG_ECX)
            entries.append(dict(index=pointers.index(pointer), position=ints(u, pointer, 3),
                flags=u32(u, pointer + 20), queried=False, threshold=None,
                reduced_before=bool(u.mem_read(0xABCD50, 1)[0]), projected=None,
                fog_called=False, fog_result=None, admitted=False))
        elif pc == 0x55AF60:
            entries[-1]['queried'] = True
        elif pc == 0x5FF873:
            entries[-1]['threshold'] = u.reg_read(UC_X86_REG_EAX)
        elif pc == 0x5FF890:
            entries[-1]['projected'] = bool(u.reg_read(UC_X86_REG_EAX) & 255)
        elif pc == 0x5865E0:
            entries[-1]['fog_called'] = True
        elif pc == 0x5FF8B0:
            entries[-1]['fog_result'] = bool(u.reg_read(UC_X86_REG_EAX) & 255)
        elif pc == 0x5FF8B8:
            entries[-1]['admitted'] = True
            # The admission body has finished. Reuse its original epilogue,
            # leaving packed output to light_draw_cases below.
            u.reg_write(UC_X86_REG_EIP, 0x5FFF77)
        elif pc == 0x5FFFBB:
            entries[-1]['reduced_after'] = bool(u.mem_read(0xABCD50, 1)[0])
    u.hook_add(UC_HOOK_CODE, observe)
    before_rng = m.trace.states()
    for _ in range(case['visits']):
        entries = []
        m.call(0x5FFFA0)
        visits.append(dict(entries=entries, reduced=bool(u.mem_read(0xABCD50, 1)[0])))
    assert m.trace.states() == before_rng
    assert bytes(u.mem_read(rally.PIXELS, m.pixel_bytes)) == m.before
    assert hashlib.sha256(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)).hexdigest() == m.text_sha256
    return dict(input=case, visits=visits, rng_unchanged=True, pixels_unchanged=True,
                text_unchanged=True, draw_boundary='0x5ff8b8')


def light_admission():
    base = dict(source=[3038,3038,0], target=[3600,2700,0], camera=[-60,250],
                fps=60, minimum=15, buffer=5, reduced=False, scenario_flags=0, visits=2,
                lights=[dict(position=[3038 + i*8,3038,0], flags=flags)
                        for i, flags in enumerate((0,1,2))])
    cases = [dict(base, name='below_minimum', fps=0),
             dict(base, name='below_boundary', fps=14),
             dict(base, name='equal_minimum', fps=15),
             dict(base, name='retained_hysteresis', fps=15, reduced=True),
             dict(base, name='recovery', fps=20, reduced=True),
             dict(base, name='unsigned_sum_wrap', fps=0, minimum=-1, buffer=1,
                  lights=[dict(position=[3038 + i*8,3038,0], flags=flags)
                          for i, flags in enumerate((0,0,0,2,1))]),
             dict(base, name='scenario_fog_stub', scenario_flags=0x1000),
             dict(base, name='offscreen', camera=[3000,3000])]
    return [light_admission_case(case) for case in cases]


class LightPixels(ElectricPixels):
    """The same surface, original native mask construction and whole draw."""
    def allocate(self, size):
        if not hasattr(self, 'light_cursor'):
            return super().allocate(size)
        p = self.light_cursor
        self.light_cursor += (size + 15) & ~15
        assert self.light_cursor < 0x27000000
        return p


def construct_light_masks(m, count):
    """Original normal-bank loop prefix on this existing surface VM."""
    assert 0 < count <= 64
    u = m.u
    u.mem_write(SP, words(native.RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, SP)
    native.run_checked(u, 0x5FF420, 0x5FF444, count=10000)
    for _ in range(count):
        native.run_checked(u, 0x5FF444, 0x5FF5A4, count=2000000, timeout_us=20000000)
        native.run_checked(u, 0x5FF5A4, 0x5FF444, count=10)
    assert u.reg_read(UC_X86_REG_EBP) == 0xAC1698 + count*4
    masks = []
    for index in range(count):
        surface = u32(u, 0xAC1698 + index*4)
        raw = bytes(u.mem_read(u32(u, surface + 0x14), 256*128))
        masks.append(dict(index=index, size=ints(u, surface + 4, 2),
                          bytes_hex=raw.hex(), sha256=hashlib.sha256(raw).hexdigest()))
    return masks


def light_pixels():
    case = dict(name='stock_spark_light', source=[3038,3038,0], target=[3600,2700,0],
                camera=[-60,250], background=rally.BACKGROUND, fps=60, detail=2)
    m = LightPixels(case)
    u = m.u
    u.mem_map(0x26000000, 0x1000000)
    m.light_cursor = 0x26000000
    u.mem_write(0xAC1678, words(0x7EB6D4, m.allocate(64), 16, 1, 0, 10))
    pointer = m.allocate(0x18)
    m.call(0x5FF250, pointer, *case['source'], 15)
    u.mem_write(0x887314, words(rally.SURFACE))
    u.mem_write(0x833588, words(2))  # supplied RGB565 display mode
    u.mem_write(0xAC1838, words(0))  # direct native channel path, no optional LUT
    before_rng = m.trace.states()
    # Execute the original loop body and its backedge, retaining its own
    # registers/stack and radius writes. All slots needed by ordinary Size15
    # are constructed; the unrelated type16 bank is outside this prefix.
    masks = construct_light_masks(m, 15)
    rows = []
    for stage in range(0, 80, 8):
        u.mem_write(rally.PIXELS, m.before)
        u.mem_write(pointer + 12, words(stage))
        m.call(0x5FFFA0)
        raw = bytes(u.mem_read(rally.PIXELS, m.pixel_bytes))
        rows.append(dict(stage=stage, size=signed(u32(u, pointer + 16)), flags=u32(u, pointer + 20),
            pixels=[[i % m.surface.size[0], i // m.surface.size[0], pixel[0]]
                    for i, pixel in enumerate(struct.iter_unpack('<H', raw))
                    if raw[i*2:i*2+2] != m.before[i*2:i*2+2]],
            pixel_sha256=hashlib.sha256(raw).hexdigest()))
    assert m.trace.states() == before_rng
    assert bytes(u.mem_read(m.z_pixels, m.pixel_bytes)) == m.before_z
    assert bytes(u.mem_read(rally.ALPHA, m.pixel_bytes)) == struct.pack('<'+'H'*len(m.surface.alpha), *m.surface.alpha)
    assert bytes(u.mem_read(rally.PIXELS - 32, 32)) == m.guard
    assert bytes(u.mem_read(rally.PIXELS + m.pixel_bytes, 32)) == m.guard
    assert hashlib.sha256(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)).hexdigest() == m.text_sha256
    return dict(input=case, position=case['source'], masks=masks, draws=rows,
                rng_unchanged=True, z_unchanged=True, alpha_unchanged=True,
                text_unchanged=True, guards_unchanged=True)


def welding_layers(map_path=None):
    """The same physical layer owner, with the actual selected map payload.

    Hills.mmx is a MIX container, not INI text. stock.mix remains the sole
    archive decoder; lexical remains only cached-INI transport.
    """
    layers = physical_layers()[:3]
    if map_path is None:
        path = native.configured_gamemd().parent / 'Hills.mmx'
        container = path.read_bytes()
        raw = mix(container)[mix_hash('Hills.map')]
        identity = dict(file='Hills.mmx', bytes=len(container),
            sha256=hashlib.sha256(container).hexdigest(), entry_id='D5FE80AC')
        layers.append(('Hills.map', 'loose Hills.mmx', raw))
    else:
        path = Path(map_path)
        raw = path.read_bytes()
        identity = dict(file=path.name, bytes=len(raw),
            sha256=hashlib.sha256(raw).hexdigest(), entry_id=None)
        layers.append((path.name, 'supplied unchanged scenario INI', raw))
    identity['ini_bytes'] = len(raw)
    identity['ini_sha256'] = hashlib.sha256(raw).hexdigest()
    return layers, identity


def welding_weapon_state(m, p):
    def name(pointer):
        return m.string(pointer + 0x24) if pointer else None
    return dict(use_spark_particles=bool(m.u.mem_read(p + 0x12A, 1)[0]),
        attached_particle_system=name(m.read32(p + 0x11C)),
        rof=signed(m.read32(p + 0xB0)), burst=signed(m.read32(p + 0x9C)),
        damage=signed(m.read32(p + 0xA4)), range_leptons=signed(m.read32(p + 0xB4)),
        projectile=name(m.read32(p + 0xA0)), warhead=name(m.read32(p + 0xAC)))


class WeldingBirth(SparkBirth):
    """One existing reader/world VM for original Welding ctor and live AI.

    Unit/Foot prefixes and Drive constructors reuse shroud's existing mobile
    fixture. Whole Unit construction/placement, boarding, targeting and Bullet
    production are not supplied as successful predicate answers here.
    """
    def invoke(self, address, obj, args=(), **kwargs):
        result = super().invoke(address, obj, args, **kwargs)
        if self.runtime and hasattr(self, 'returns'):
            # Unicorn's primary end stops before code hooks. The original RET
            # has already produced EIP/ESP/EAX here; observe, never replace it.
            self.observe_return(self.u.reg_read(UC_X86_REG_EIP), self.u.reg_read(UC_X86_REG_ESP))
        return result

    def __init__(self, case, *, map_path=None, actors=True,
                 system_name='WeldingSys', particle_name='WeldingSpark'):
        layers, self.map_identity = welding_layers(map_path)
        super().__init__(dict(name=case['name'], seed=case.get('seed', 1),
            frame=36, detail=2, cell_x_min=38,
            fail_system_allocation=case.get('fail_system_allocation', False)),
            None, layers=layers, system_name=system_name, particle_name=particle_name)
        self.control = case
        self.runtime = False
        u = self.u
        # The existing LiveSparkBirth/IFV platform owner supplies FS:[0] for
        # original CRT RTTI reached by deferred725C70. Do not skip its body.
        native.initialize_empty_windows_seh(u)
        self.invoke(0x6F2A50, 0)
        self.zero_coord_hex = bytes(u.mem_read(0xB0EA90, 12)).hex()
        assert self.zero_coord_hex == '00' * 12
        prepare_spark_capacity(self, (0x887568,))
        self.weapon = self.invoke(0x772FA0, self.cstring('RepairBullet'))
        self.weapon_constructor = welding_weapon_state(self, self.weapon)
        self.weapon_layers = []
        for name, archive, raw in layers:
            if raw is None:
                self.weapon_layers.append(dict(file=name, absent=True))
                continue
            sections, _ = lexical(raw, {'RepairBullet', 'Invisible', 'Mechanical'})
            self.rules_cache(sections)
            self.reads.clear()
            admitted = bool(self.invoke(0x772080, self.weapon, (RULES,)) & 255)
            self.weapon_layers.append(dict(file=name, archive=archive,
                bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(), sections=sections,
                admitted=admitted, state=welding_weapon_state(self, self.weapon),
                reads=list(self.reads)))
        if system_name == 'WeldingSys':
            assert self.read32(self.weapon + 0x11C) == self.system_type
        self.selected_system_name = system_name
        self.roles = {}
        self.mobile_inputs = []
        self.house = self.alloc(0x18000)
        u.mem_write(self.house + 0x1A8, struct.pack('<d', 1.0))
        if actors:
            from tools.procedural_drawing_oracle.shroud import _prepare_unit_rank
            # initialize_effect_registries ran after SparkBirth's first capacity
            # preparation. Supply expiry capacity once after that native reset;
            # the three original Object constructors retain their own appends.
            prepare_spark_capacity(self, (0xA8EB00, 0xB0F720))
            for role, type_name, position in (
                ('owner', 'FV', [11136, 12416, 0]),
                ('target', 'MTNK', [11392, 12416, 0]),
                ('other_target', 'MTNK', [11648, 12416, 0])):
                typ, actor = self.alloc(0x1900), self.alloc(0x1000)
                self.invoke(0x7470D0, typ, (self.cstring(type_name),))
                runtime = _prepare_unit_rank(self, typ, actor, self.house,
                    existing_cells=list(self.cells.values()),
                    reuse_existing_expiry_registry=True)
                u.mem_write(actor + 0x9C, words(*position))
                u.mem_write(actor + 0x21C, words(self.house))
                u.mem_write(actor + 0x90, b'\1')
                u.mem_write(actor + 0x6C, words(100))
                self.roles[role] = actor
                self.mobile_inputs.append(dict(role=role, type_id=type_name,
                    position=position, native_id=signed(self.read32(actor + 0x10)),
                    who_am_i=self.invoke(self.read32(self.read32(actor) + 0x2C), actor),
                    constructor=runtime['evidence']))
                if role == 'owner':
                    self.owner_type = typ
                    # Actual GetROF's existing Unit GetWeapon virtual selects
                    # the already-reader-produced slot. Slot0 is a component
                    # prior; normal Engineer boarding selects slot1 in VERA.
                    u.mem_write(typ + 0x898, words(self.weapon))
                    u.mem_write(typ + 0xA94, words(self.weapon))
            self.owner, self.target = self.roles['owner'], self.roles['target']
            self.roles['cell'] = self.cells[44, 48]
            u.mem_write(self.owner + 0x2B4, words(self.target))
            # Object5F3A76..5F3A8B must retain every constructor subscription.
            # Notify72593E..72595F traverses this registry independently of Logic.
            listeners = self.registry_members(0xB0F720)
            for row in self.mobile_inputs:
                count = listeners.count(self.roles[row['role']])
                assert count == 1, ('native expiry subscription', row['role'], count)
                row['expiry_registered'] = True
        for i, pointer in enumerate(self.trace.streams.values()):
            u.mem_write(pointer, seeded_struct(case.get('seed', 1) + i))
        self.trace.requests.clear()
        assert not self.trace.pending
        self.events, self.returns, self.systems, self.particles, self.lights = [], [], [], [], []
        self.transports = []
        self.retirement_hook = u.hook_add(UC_HOOK_CODE,
            lambda _u, pc, _size, _data: retirement_transport(self, pc, self.transports))
        self.runtime = True

    def identity(self, pointer):
        if not pointer:
            return None
        role = next((name for name, p in self.roles.items() if pointer == p), None)
        if role:
            return role
        if pointer in self.systems:
            return 'system'
        if pointer in self.particles:
            return dict(particle_native_id=signed(self.read32(pointer + 0x10)))
        raise AssertionError(('unclassified pointer', hex(pointer)))

    def system_state(self, pointer):
        u = self.u
        return dict(native_id=signed(self.read32(pointer + 0x10)),
            position=ints(u, pointer + 0x9C, 3), offset=ints(u, pointer + 0xB0, 3),
            target_coords=ints(u, pointer + 0xD4, 3),
            owner=self.identity(self.read32(pointer + 0xE0)),
            retained_target=self.identity(self.read32(pointer + 0xE4)),
            owner_house_present=bool(self.read32(pointer + 0xFC)),
            lifetime=signed(self.read32(pointer + 0xEC)),
            spark_spawn_frames=signed(self.read32(pointer + 0xF0)),
            stage=signed(self.read32(pointer + 0xF4)),
            spawn_timer_f32_bits=self.read32(pointer + 0xE8),
            done=bool(u.mem_read(pointer + 0xF8, 1)[0]),
            direction_used=bool(u.mem_read(pointer + 0xF9, 1)[0]),
            particle_count=self.read32(pointer + 0xCC),
            particle_native_ids=[signed(self.read32(self.read32(self.read32(pointer + 0xC0) + i*4) + 0x10))
                                 for i in range(self.read32(pointer + 0xCC))],
            alive=bool(u.mem_read(pointer + 0x90, 1)[0]),
            logic=bool(u.mem_read(pointer + 0x98, 1)[0]))

    def link_state(self):
        p = self.owner
        return dict(spark_link=self.identity(self.read32(p + 0x308)),
            target=self.identity(self.read32(p + 0x2B4)),
            burst=signed(self.read32(p + 0x3B8)), rof=signed(self.read32(p + 0x2F8)),
            rearm=ints(self.u, p + 0x2EC, 3))

    def registry_members(self, vector):
        data, count = self.read32(vector + 4), self.read32(vector + 0x10)
        return [self.read32(data + i*4) for i in range(count)]

    def logical_state(self, pointer):
        systems = self.registry_members(0xA80208)  # original62DD35..62DD83 inserts here
        logic = self.registry_members(0x87F778)
        pending = self.registry_members(0xB0F698)
        listeners = self.registry_members(0xB0F720)
        def ids(objects):
            return [signed(self.read32(p + 0x10)) for p in objects]
        return dict(present_in_system_registry=pointer in systems,
            present_in_logic=pointer in logic, pending_system=pointer in pending,
            system_registry_native_ids=ids(systems), logic_native_ids=ids(logic),
            pending_native_ids=ids(pending),
            expiry_registered_roles=[name for name in ('owner', 'target', 'other_target')
                                     if self.roles[name] in listeners],
            link=self.link_state(),
            native_id_cursor=signed(self.read32(self.scenario + 0x214)),
            scenario_rng_hex=self.trace.states()['scenario'])

    def pending_particle_state(self, pointer, system):
        """Observe an allocated queued child before actual725C70 destroys it."""
        u = self.u
        return dict(native_id=signed(self.read32(pointer + 0x10)),
            alive=bool(u.mem_read(pointer + 0x90, 1)[0]),
            in_limbo=bool(u.mem_read(pointer + 0x81, 1)[0]),
            on_map=bool(u.mem_read(pointer + 0x80, 1)[0]),
            display_layer_index=signed(self.read32(pointer + 0x94)),
            # DisplayRemove4A9821..4A982A traverses these five native layers.
            display_registered=any(pointer in self.registry_members(0x8A0360 + i*0x18)
                                   for i in range(5)),
            object_registered=pointer in self.registry_members(0xA8E360),
            particle_array_registered=pointer in self.registry_members(0xA83DC8),
            expiry_registered=pointer in self.registry_members(0xB0F720),
            in_logic=bool(u.mem_read(pointer + 0x98, 1)[0]),
            in_parent_vector=pointer in self.registry_members(system + 0xBC),
            marked_for_deletion=bool(u.mem_read(pointer + 0x131, 1)[0]))

    def checked(self):
        assert not self.trace.pending and not self.returns
        assert hashlib.sha256(self.u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)).hexdigest() == self.text_sha

    def construct(self, *, supplied_target='target', owner='owner', tarcom='target',
                  fifth=(0, 0, 0), position=(11136, 12416, 96), bridge=False):
        u = self.u
        if 'owner' in self.roles:
            u.mem_write(self.owner + 0x2B4, words(self.roles.get(tarcom, 0)))
        u.mem_write(self.roles['cell'] + 0x140, words(0x100 if bridge else 0))
        coords = self.alloc(12)
        fifth_coords = 0xB0EA90 if tuple(fifth) == (0, 0, 0) else self.alloc(12)
        u.mem_write(coords, words(*position))
        if fifth_coords != 0xB0EA90:
            u.mem_write(fifth_coords, words(*fifth))
        pointer = self.alloc(0x100)
        before, cursor = self.trace.states(), signed(self.read32(self.scenario + 0x214))
        self.phase = 'constructor'
        self.invoke(0x62DC50, pointer, (self.system_type, coords,
            self.roles.get(supplied_target, 0), self.roles.get(owner, 0), fifth_coords, 0))
        self.checked()
        return dict(input=dict(system_type=self.selected_system_name,
            supplied_target=supplied_target, owner=owner, tarcom=tarcom,
            fifth=list(fifth), fifth_from_native_zero_coord=fifth_coords == 0xB0EA90,
            position=list(position), cell_bridge=bridge),
            state=self.system_state(pointer), native_id_before=cursor,
            native_id_after=signed(self.read32(self.scenario + 0x214)),
            rng_unchanged=before == self.trace.states(),
            rng_requests=list(self.trace.requests), text_unchanged=True)

    def fire_birth(self):
        u, case = self.u, self.control
        u.mem_write(self.weapon + 0x12A, bytes([case.get('use_spark_particles', True)]))
        if case.get('retained_link'):
            self.construct()
            u.mem_write(self.owner + 0x308, words(self.systems[0]))
        before, cursor = self.trace.states(), signed(self.read32(self.scenario + 0x214))
        count_before = len(self.systems)
        self.trace.requests.clear()
        # The caller's exact muzzle and FireAt target are supplied priors.
        # The six pushes, allocation, full constructor and +308 store execute.
        u.mem_write(READER_SP, bytes(0x200))
        u.mem_write(READER_SP + 0x44, words(11136, 12416, 96))
        u.mem_write(READER_SP + 0x100, words(0, 0, self.target, 0))
        for reg, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, READER_SP + 0x100),
                           (UC_X86_REG_ESI, self.owner), (UC_X86_REG_EBX, self.weapon)):
            u.reg_write(reg, value)
        self.phase = 'fireat_birth'
        native.run_checked(u, 0x6FF1AD, 0x6FF1FB, count=300000)
        assert u.reg_read(UC_X86_REG_ESP) == READER_SP
        self.checked()
        return dict(input=case, supplied_muzzle=[11136, 12416, 96], supplied_target='target',
            zero_coord_hex=self.zero_coord_hex, systems_before=count_before,
            systems_after=len(self.systems), systems=[self.system_state(p) for p in self.systems],
            link=self.link_state(), native_id_before=cursor,
            native_id_after=signed(self.read32(self.scenario + 0x214)),
            rng_unchanged=before == self.trace.states(), rng_requests=list(self.trace.requests),
            text_unchanged=True)


def welding_admission_case(use_spark, live, *, berserk=False):
    m = WeldingBirth(dict(name=f'flag{int(use_spark)}_live{int(live)}_berserk{int(berserk)}'))
    m.fire_birth()
    u, owner = m.u, m.owner
    u.mem_write(m.weapon + 0x12A, bytes([use_spark]))
    u.mem_write(owner + 0x308, words(m.systems[0] if live else 0))
    u.mem_write(owner + 0x298, bytes([berserk]))
    controls = []
    before, cursor = m.trace.states(), signed(m.read32(m.scenario + 0x214))
    m.trace.requests.clear()
    for name, entry, continuation, weapon_register in (
        ('fireat_guard', 0x6FDE36, 0x6FDE4A, UC_X86_REG_EBX),
        ('t37', 0x6FC6D5, 0x6FC6ED, UC_X86_REG_EAX),
        ('t46', 0x6FC9C7, 0x6FC9EA, UC_X86_REG_EBX)):
        u.mem_write(READER_SP, bytes(0x200))
        u.mem_write(READER_SP + 0x20, words(native.RET_MAGIC, m.target, 0, 0))
        u.mem_write(READER_SP + 0x100, words(0, native.RET_MAGIC, m.target, 0))
        for reg, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_ESI, owner),
                           (UC_X86_REG_EBP, READER_SP + 0x100),
                           (weapon_register, m.weapon)):
            u.reg_write(reg, value)
        stopped = native.run_checked(u, entry, (continuation, native.RET_MAGIC), count=10000)
        controls.append(dict(name=name, entry=hex(entry), stop=hex(stopped),
            continued=stopped == continuation,
            returned_eax=(signed(u.reg_read(UC_X86_REG_EAX)) if stopped == native.RET_MAGIC else None)))
    m.phase = 'get_rof'
    u.mem_write(owner + 0x3B8, words(1))
    mark = len(m.trace.requests)
    rof = signed(m.invoke(0x6FCFA0, owner, (0,)))
    rof_requests = list(m.trace.requests[mark:])
    # The actual FireAt burst/GetROF/Berserk/rearm/IDIV tail runs independently
    # from the same declared frame and pre-tail burst, not a host timer formula.
    u.mem_write(owner + 0x3B8, words(0))
    u.mem_write(READER_SP, bytes(0x200))
    u.mem_write(READER_SP + 0x100, words(0, native.RET_MAGIC, m.target, 0))
    for reg, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, READER_SP + 0x100),
                       (UC_X86_REG_ESI, owner), (UC_X86_REG_EBX, m.weapon)):
        u.reg_write(reg, value)
    mark = len(m.trace.requests)
    m.phase = 'rearm'
    native.run_checked(u, 0x6FF274, 0x6FF2D1, count=100000)
    assert u.reg_read(UC_X86_REG_ESP) == READER_SP
    m.checked()
    return dict(input=dict(use_spark_particles=use_spark, live=live, berserk=berserk,
        weapon_slot=0, queried_burst=1, pre_rearm_burst=0, frame=36,
        house_rof_f64_bits='3ff0000000000000', rookie=True, timer_middle=0,
        rng_seeds=dict(main=1, scenario=2, mapgen=3)),
        controls=controls, get_rof=rof, get_rof_requests=rof_requests,
        rearm=m.link_state(), rearm_requests=list(m.trace.requests[mark:]),
        native_id_unchanged=cursor == signed(m.read32(m.scenario + 0x214)),
        rng_before=before, rng_after=m.trace.states(), text_unchanged=True)


def welding_expiry_cases():
    rows = []
    for removed in (0, 1):
        for expired in ('system', 'other_target'):
            m = WeldingBirth(dict(name=f'techno_expiry_{expired}_{removed}'))
            m.fire_birth()
            pointer = m.systems[0] if expired == 'system' else m.roles[expired]
            before = m.trace.states()
            initial = m.link_state()
            m.phase = 'techno_pointer_expired'
            m.invoke(0x7077C0, m.owner, (pointer, removed))
            m.checked()
            rows.append(dict(input=dict(receiver='owner', expired=expired, removed=removed,
                entry='0x7077c0'), before=initial, after=m.link_state(),
                rng_unchanged=before == m.trace.states(),
                rng_requests=list(m.trace.requests), text_unchanged=True))
        for expired in ('owner', 'target'):
            m = WeldingBirth(dict(name=f'system_expiry_{expired}_{removed}'))
            m.fire_birth()
            pointer = m.systems[0]
            before = m.trace.states()
            initial = m.system_state(pointer)
            m.phase = 'system_pointer_expired'
            m.invoke(0x62FE90, pointer, (m.roles[expired], removed))
            m.checked()
            rows.append(dict(input=dict(receiver='system', expired=expired, removed=removed,
                entry='0x62fe90'), before=initial, after=m.system_state(pointer),
                rng_unchanged=before == m.trace.states(),
                rng_requests=list(m.trace.requests), text_unchanged=True))
    m = WeldingBirth(dict(name='target_change_keeps_live_link'))
    m.fire_birth()
    initial, before = m.link_state(), m.trace.states()
    m.phase = 'target_changes'
    setter = m.read32(m.read32(m.owner) + 0x3C8)
    steps = []
    for target in ('other_target', None):
        m.invoke(setter, m.owner, (m.roles.get(target, 0),))
        steps.append(m.link_state())
    m.checked()
    rows.append(dict(input=dict(receiver='owner', entry=hex(setter),
        targets=['other_target', None]), before=initial, steps=steps,
        rng_unchanged=before == m.trace.states(),
        rng_requests=list(m.trace.requests), text_unchanged=True))
    m = WeldingBirth(dict(name='stop_setters_keep_live_link'))
    m.fire_birth()
    initial, before = m.link_state(), m.trace.states()
    m.phase = 'stop_setter_fragment'
    m.u.reg_write(UC_X86_REG_ESP, READER_SP)
    m.u.reg_write(UC_X86_REG_ESI, m.owner)
    m.u.reg_write(UC_X86_REG_EDI, 0)
    native.run_checked(m.u, 0x4C75E6, 0x4C75FE, count=300000)
    assert m.u.reg_read(UC_X86_REG_ESP) == READER_SP
    m.checked()
    rows.append(dict(input=dict(receiver='owner', entry='0x4c75e6', stop='0x4c75fe',
        boundary='Stop NULL-destination/NULL-target setter fragment; full Event admission excluded'),
        before=initial, after=m.link_state(), rng_unchanged=before == m.trace.states(),
        rng_requests=list(m.trace.requests), text_unchanged=True))
    return rows


def welding_live_case():
    """Small boundary corpus from uninterrupted original System/child AI."""
    m = WeldingBirth(dict(name='welding_live_boundaries', seed=1))
    birth = m.fire_birth()
    pointer = m.systems[0]
    initial_rng = m.trace.states()
    visits, totals, previous, first_particle = [], dict(raw=0, ranged=0), None, None
    retirement_before_drain = None
    for tick in range(600):
        m.logic_visit('welding_live', 36 + tick)
        state = m.system_state(pointer)
        if first_particle is None and m.particles:
            p = m.particles[0]
            first_particle = dict(world=ints(m.u, p + 0x9C, 3),
                start_rgb=list(m.u.mem_read(p + 0xB0, 3)),
                color_index=signed(m.read32(p + 0xB4)),
                color_accumulator_f64_bits=f"{struct.unpack('<Q', m.u.mem_read(p + 0xB8, 8))[0]:016x}",
                damage=signed(m.read32(m.particle_type + 0x2E8)))
        before_drain = m.logical_state(pointer)
        if not state['alive']:
            pending_particles = [p for p in m.registry_members(0xB0F698) if p in m.particles]
            assert len(pending_particles) <= 5, 'final queued-child observation exceeded its bound'
            retirement_before_drain = dict(tick=tick + 1, frame=36 + tick,
                system=state, logical=before_drain,
                pending_particles=[m.pending_particle_state(p, pointer) for p in pending_particles],
                rng_requests=list(m.trace.requests))
        m.phase = 'live_deferred_drain'
        m.invoke(0x725C70, 0, context=dict(control='welding_live', tick=tick + 1,
            frame=36 + tick, phase='deferred_drain'))
        logical = m.logical_state(pointer)
        for row in m.trace.requests:
            totals[row['kind']] += 1
        # A removed instance has no post-drain live System state. Keep the
        # still-allocated pre-drain body separately and read only live rosters,
        # pending queue, source link, cursor and RNG after actual destruction.
        sample = dict(tick=tick + 1, frame=36 + tick,
            system=(m.system_state(pointer) if logical['present_in_system_registry'] else None),
            logical=logical, link=m.link_state(),
            native_id_cursor=signed(m.read32(m.scenario + 0x214)),
            scenario_rng_hex=m.trace.states()['scenario'], rng_requests_total=dict(totals))
        if tick + 1 in (1, 19, 20, 21):
            sample['rng_requests'] = list(m.trace.requests)
            visits.append(sample)
        if not logical['present_in_system_registry']:
            if previous and previous['tick'] not in {row['tick'] for row in visits}:
                visits.append(previous)
            if sample['tick'] not in {row['tick'] for row in visits}:
                visits.append(sample)
            break
        previous = sample
    else:
        raise AssertionError('Welding did not retire within the declared600-visit bound')
    assert first_particle is not None
    assert retirement_before_drain is not None
    assert visits[-1]['link']['spark_link'] is None, 'native retirement did not notify its source'
    final_rng = m.trace.states()
    m.checked()
    return dict(input=dict(name='welding_live_boundaries', seed=1,
        rng_seeds=dict(main=1, scenario=2, mapgen=3), first_frame=36,
        max_visits=600, cells='flat64x64; mapped38..48 by46..52; original default-cell fallback',
        world_receivers='empty Cell occupants; retained actor expiry listeners',
        expiry_registered_roles=[row['role'] for row in m.mobile_inputs if row['expiry_registered']]),
        birth=birth, boundaries=visits,
        retirement_before_drain=retirement_before_drain,
        first_produced_particle=first_particle,
        scenario_rng_initial_hex=initial_rng['scenario'],
        main_rng_unchanged=initial_rng['main'] == final_rng['main'],
        mapgen_rng_unchanged=initial_rng['mapgen'] == final_rng['mapgen'],
        text_unchanged=True)


class WeldingPixels(LightPixels):
    """Whole Spark/System draw on the existing packed surface/light VM."""
    def __init__(self, case, *, masks=False):
        super().__init__(dict(case, bolts=[]))
        self.current_draw, self.light_row = None, None
        self.point_type = self.allocate(0x318)
        self.system_type = self.allocate(0x310)
        self.system_pointer = self.allocate(0x100)
        self.color_list = self.allocate(128)
        self.bounds = self.allocate(16)
        self.u.hook_add(UC_HOOK_CODE, self.observe_welding)
        self.masks = []
        if masks:
            self.u.mem_map(0x26000000, 0x1000000)
            self.light_cursor = 0x26000000
            self.u.mem_write(0xAC1678, words(0x7EB6D4, self.allocate(64), 16, 1, 0, 10))
            self.u.mem_write(0x833588, words(2))
            self.u.mem_write(0xAC1838, words(0))
            # LightSize25 and the inspected table's maximum63 bound every
            # selected index below25. The actual reached indices are observed
            # in whole5FF850, not supplied by a Python index calculation.
            assert max(self.u.mem_read(0x83358C, 80)) == 63
            self.masks = construct_light_masks(self, 25)

    def reset(self, case):
        u = self.u
        width, height = self.surface.size
        assert case['size'] == [width, height]
        self.before = struct.pack('<H', case['background']) * (width*height)
        u.mem_write(rally.PIXELS, self.before)
        self.surface.alpha = [case.get('a_default', 127)] * (width*height)
        z = [case.get('z_default', 65535)] * (width*height)
        for values, key in ((self.surface.alpha, 'a_patches'), (z, 'z_patches')):
            for x, y, value in case.get(key, []):
                assert 0 <= x < width and 0 <= y < height and 0 <= value <= 65535
                values[y*width + x] = value
        self.before_a = struct.pack('<' + 'H' * len(self.surface.alpha), *self.surface.alpha)
        self.before_z = struct.pack('<' + 'H' * len(z), *z)
        u.mem_write(rally.ALPHA, self.before_a)
        u.mem_write(self.z_pixels, self.before_z)
        u.mem_write(self.bounds, words(*case['clip']))
        u.mem_write(0x886FA0, words(*case['clip']))
        u.mem_write(0x886FA4, words(case.get('radar_viewport_y', case['clip'][1])))
        u.mem_write(rally.TACTICAL + 0xB0, words(*case['camera']))
        u.mem_write(0xB0CD48, struct.pack('<Q', int(case.get('adjust_for_z_f64_bits', '3fc25e5374344960'), 16)))
        u.mem_write(self.z_object + 0x24, words(case.get('z_origin_term', -32768)))
        u.mem_write(self.z_object + 4, words(case.get('z_bottom_term', 0)))
        for address, value in ((0xABCD44, case['fps']), (0x829FF4, case['minimum']),
                               (0x829FF8, case['buffer']), (0xA8EB78, case.get('detail', 2)),
                               (self.scenario, case['scenario_flags'])):
            u.mem_write(address, words(value))
        u.mem_write(0xABCD50, bytes([case['reduced']]))
        # Both optional native fog paths test this client gate. Its original
        # 5865E0 callee still runs, returning its actual active-retail value.
        u.mem_write(0xB73550, words(1))
        u.mem_write(0xA8ED6B, b'\0')
        self.current_draw, self.light_row = None, None

    def observe_welding(self, u, pc, _size, _data):
        row = self.current_draw
        sp = u.reg_read(UC_X86_REG_ESP)
        if row is not None:
            if pc == 0x62CECE:
                row['threshold'] = u.reg_read(UC_X86_REG_EAX)
                row['performance_passed'] = u32(u, 0xABCD44) >= row['threshold']
            elif pc == 0x62CEEA:
                row['detail_passed'] = u32(u, 0xA8EB78) != 0
            elif pc == 0x5865E0:
                row['fog_called'] = True
            elif pc == 0x62CF5D:
                row['fog_result'] = bool(u.reg_read(UC_X86_REG_EAX) & 255)
            elif pc == 0x62D075:
                row['screen'] = [signed(u.reg_read(UC_X86_REG_EBX)), signed(u.reg_read(UC_X86_REG_EAX))]
            elif pc == 0x62D0B9:
                row['a_word'] = u.reg_read(UC_X86_REG_EBX)
            elif pc == 0x62D11C:
                row['candidate_z'] = signed(u.reg_read(UC_X86_REG_ESI))
                row['z_word'] = u.reg_read(UC_X86_REG_ECX)
            elif pc == 0x62D21A:
                row['interpolated_rgb'] = [signed(u.reg_read(r)) for r in
                    (UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_EAX)]
            elif pc == 0x62D233:
                row['modulated_rgb'] = [signed(u.reg_read(r)) for r in
                    (UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_EAX)]
            elif pc == 0x62D288:
                row['write'] = dict(screen=ints(u, u32(u, sp), 2), packed_value=u32(u, sp + 4) & 65535)
        if pc == 0x5FF250 and u32(u, sp) == 0x62E34C:
            self.light_row = dict(base_size=signed(u32(u, sp + 16)), stage=None,
                surface_index=None, flags=None, threshold=None, admitted=False,
                performance_passed=None, projected=None, fog_called=False, fog_result=None)
        elif self.light_row is not None:
            light = self.light_row
            if pc == 0x5FF850:
                p = u.reg_read(UC_X86_REG_ECX)
                light['stage'], light['flags'] = signed(u32(u, p + 12)), u32(u, p + 20)
            elif pc == 0x5FF873:
                light['threshold'] = u.reg_read(UC_X86_REG_EAX)
                light['performance_passed'] = u32(u, 0xABCD44) >= light['threshold']
            elif pc == 0x5FF890:
                light['projected'] = bool(u.reg_read(UC_X86_REG_EAX) & 255)
            elif pc == 0x5865E0:
                light['fog_called'] = True
            elif pc == 0x5FF8B0:
                light['fog_result'] = bool(u.reg_read(UC_X86_REG_EAX) & 255)
            elif pc == 0x5FF8B8:
                light['admitted'] = True
            elif pc == 0x5FF93F:
                index = signed(u.reg_read(UC_X86_REG_ECX))
                assert 0 <= index < len(self.masks), ('unchecked light bank index', index)
                light['surface_index'] = index

    def finish(self, before_rng, before_id):
        u = self.u
        raw = bytes(u.mem_read(rally.PIXELS, self.pixel_bytes))
        invariants = dict(rng_unchanged=before_rng == self.trace.states(),
            native_id_unchanged=before_id == signed(u32(u, self.scenario + 0x214)),
            z_unchanged=bytes(u.mem_read(self.z_pixels, self.pixel_bytes)) == self.before_z,
            alpha_unchanged=bytes(u.mem_read(rally.ALPHA, self.pixel_bytes)) == self.before_a,
            guards_unchanged=(bytes(u.mem_read(rally.PIXELS - 32, 32)) == self.guard and
                bytes(u.mem_read(rally.PIXELS + self.pixel_bytes, 32)) == self.guard),
            text_unchanged=hashlib.sha256(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)).hexdigest() == self.text_sha256)
        assert all(invariants.values()) and not self.trace.pending
        assert not self.spark_calls, 'draw control reached an isolated producer boundary'
        width = self.surface.size[0]
        return dict(pixels=[[i % width, i // width, pixel[0]]
            for i, pixel in enumerate(struct.iter_unpack('<H', raw))
            if raw[i*2:i*2+2] != self.before[i*2:i*2+2]], invariants=invariants)

    def points(self, case, *, reset=True):
        if reset:
            self.reset(case)
        u = self.u
        colors = bytes(channel for rgb in case['color_list'] for channel in rgb)
        assert len(colors) <= 128
        u.mem_write(self.color_list, colors)
        u.mem_write(self.point_type + 0x2BC, words(self.color_list))
        u.mem_write(self.point_type + 0x314, words(3))
        before, cursor = self.trace.states(), signed(u32(u, self.scenario + 0x214))
        rows = []
        for index, point in enumerate(case['points']):
            pointer = self.allocate(0x134)
            u.mem_write(pointer + 0x9C, words(*point['world']))
            u.mem_write(pointer + 0xAC, words(self.point_type))
            u.mem_write(pointer + 0xB0, bytes(point['start_rgb']))
            u.mem_write(pointer + 0xB4, words(point['color_index']))
            u.mem_write(pointer + 0xB8, struct.pack('<Q', int(point['color_accumulator_f64_bits'], 16)))
            u.mem_write(self.point_type + 0x2E8, words(point['damage']))
            row = dict(point=index, threshold=None, performance_passed=None,
                detail_passed=None, fog_called=False, fog_result=None, screen=None,
                candidate_z=None, a_word=None, z_word=None, interpolated_rgb=None,
                modulated_rgb=None, write=None, reduced_after=None)
            self.current_draw = row
            # At62D06D, [ESP+0x3C] is caller argument2: entry's0x24-byte
            # locals plus four saved registers place argument1 at+0x38.
            # The selected Spark arm ignores argument1 and dereferences2.
            self.call(0x62CEC0, pointer, 0, self.bounds)
            row['reduced_after'] = bool(u.mem_read(0xABCD50, 1)[0])
            rows.append(row)
        self.current_draw = None
        return dict(input=case, draws=rows, **self.finish(before, cursor))

    def system(self, case, *, reset=True):
        if reset:
            self.reset(case)
        u, typ, pointer = self.u, self.system_type, self.system_pointer
        u.mem_write(typ + 0x2A4, words(case['particle_cap']))
        u.mem_write(typ + 0x304, words(case['light_size']))
        u.mem_write(typ + 0x30C, bytes([case['one_frame_light']]))
        u.mem_write(pointer + 0x9C, words(*case['position']))
        u.mem_write(pointer + 0xAC, words(typ))
        u.mem_write(pointer + 0xCC, words(case['particle_count']))
        u.mem_write(pointer + 0xF4, words(case['stage']))
        before, cursor = self.trace.states(), signed(u32(u, self.scenario + 0x214))
        # The same two-argument Draw ABI; this System body reads neither.
        self.call(0x62E280, pointer, 0, self.bounds)
        assert u32(u, 0xAC1688) == 0, 'temporary light survived SystemDraw'
        return dict(input=case, light=self.light_row,
            reduced_after=bool(u.mem_read(0xABCD50, 1)[0]), **self.finish(before, cursor))


def welding_draw_cases(types, produced):
    colors = types['particle']['color_list']
    point = dict(world=[3038, 3038, 0], start_rgb=types['particle']['start_color1'],
        color_index=0, color_accumulator_f64_bits='3fe0000000000000', damage=0)
    base = dict(name='ordinary', points=[point], color_list=colors,
        camera=[-60, 650], clip=[0, 0, 160, 120], radar_viewport_y=0,
        adjust_for_z_f64_bits='3fc25e5374344960', z_origin_term=-32768,
        z_bottom_term=0, size=[160, 120], background=rally.BACKGROUND,
        a_default=127, a_patches=[], z_default=65535, z_patches=[],
        fps=60, minimum=15, buffer=5, reduced=False, detail=2, scenario_flags=0)
    m = WeldingPixels(base)
    # Original projection selects the camera for a declared [80,60] crop.
    m.u.mem_write(rally.TACTICAL + 0xB0, words(0, 0))
    coord, output = m.allocate(12), m.allocate(8)
    m.u.mem_write(coord, words(*point['world']))
    m.call(0x6D2140, rally.TACTICAL, coord, output)
    projected = ints(m.u, output, 2)
    base['camera'] = [projected[0] - 80, projected[1] - 60]
    first = m.points(base)
    candidate = first['draws'][0]['candidate_z']
    assert candidate is not None and 0 < candidate < 65535
    cases = [dict(base, name=f'a_{a}', a_default=a)
             for a in (0, 1, 126, 127, 128, 255, 256, 32768, 65535)]
    cases += [dict(base, name=f'z_{label}', z_default=candidate + delta)
              for label, delta in (('below', -1), ('equal', 0), ('above', 1))]
    cases += [dict(base, name='negative_candidate',
            z_origin_term=first['draws'][0]['screen'][1], z_default=0),
        dict(base, name='origin_wrap_and_row_offset', z_origin_term=32767, z_bottom_term=12),
        dict(base, name='nonzero_viewport', clip=[7, 13, 140, 100], radar_viewport_y=13),
        dict(base, name='outside_clip', clip=[0, 0, 16, 16]),
        dict(base, name='fps_rejected', fps=0), dict(base, name='detail_rejected', detail=0),
        dict(base, name='fog_callee', scenario_flags=0x1000),
        dict(base, name='damage_bypasses_fps', fps=0, points=[dict(point, damage=1)]),
        dict(base, name='color_transition', points=[dict(point, color_index=1,
            color_accumulator_f64_bits='3fefffffffffffff')]),
        dict(base, name='unsigned_threshold_wrap', fps=0, minimum=4294967295, buffer=1,
            points=[point, point, point]),
        dict(base, name='overlap_forward', points=[point, dict(point, color_index=2)]),
        dict(base, name='overlap_reverse', points=[dict(point, color_index=2), point])]
    for name, control in (
        ('first_produced_welding_child', dict(produced)),
        ('odd_signed_projection_and_height', dict(point, world=[-79, 3039, 101])),
        ('wrapping_planar_products', dict(point, world=[2147483647, -2147483647, 0]))):
        m.u.mem_write(rally.TACTICAL + 0xB0, words(0, 0))
        m.u.mem_write(coord, words(*control['world']))
        m.call(0x6D2140, rally.TACTICAL, coord, output)
        projected = ints(m.u, output, 2)
        cases.append(dict(base, name=name, points=[control],
            camera=[projected[0] - 80, projected[1] - 60]))
    rows = [m.points(case) for case in cases]
    negative = next(row for row in rows if row['input']['name'] == 'negative_candidate')
    assert negative['draws'][0]['candidate_z'] < 0 and negative['draws'][0]['write'] is not None
    return rows


def welding_system_draw_cases(types, spark_case):
    system = types['system']
    base = dict(name='ordinary', position=[3038, 3038, 0], particle_count=1,
        particle_cap=system['particle_cap'], light_size=system['light_size'],
        one_frame_light=system['one_frame_light'], stage=29,
        camera=[-60, 650], clip=[0, 0, 160, 120], size=[160, 120],
        background=rally.BACKGROUND, fps=60, minimum=15, buffer=5,
        reduced=False, detail=2, scenario_flags=0)
    m = WeldingPixels(base, masks=True)
    coord, output = m.allocate(12), m.allocate(8)
    m.u.mem_write(rally.TACTICAL + 0xB0, words(0, 0))
    m.u.mem_write(coord, words(*base['position']))
    m.call(0x6D2140, rally.TACTICAL, coord, output)
    projected = ints(m.u, output, 2)
    base['camera'] = [projected[0] - 80, projected[1] - 60]
    cases = [dict(base, name=f'count{count}_stage{stage}', particle_count=count, stage=stage)
        for count in (0, 1, 6, 12, 15, 30) for stage in (17, 29, 41)]
    cases += [dict(base, name='one_frame_disabled', one_frame_light=False),
        dict(base, name='size_zero', light_size=0), dict(base, name='fps_rejected', fps=0),
        dict(base, name='retained_hysteresis', fps=15, reduced=True),
        dict(base, name='fog_callee', scenario_flags=0x1000),
        dict(base, name='offscreen', camera=[3000, 3000])]
    rows = [m.system(case) for case in cases]
    mixed = []
    for order in (['system', 'particle'], ['particle', 'system']):
        particle_input = dict(spark_case, name='mixed_point', camera=base['camera'])
        system_input = dict(base, name='mixed_system', particle_count=12)
        m.reset(particle_input)
        before, cursor = m.trace.states(), signed(u32(m.u, m.scenario + 0x214))
        outputs = {}
        for family in order:
            outputs[family] = (m.system(system_input, reset=False) if family == 'system'
                               else m.points(particle_input, reset=False))
        mixed.append(dict(input=dict(name='_then_'.join(order), order=order,
            system=system_input, particles=particle_input),
            light=outputs['system']['light'], draws=outputs['particle']['draws'],
            **m.finish(before, cursor)))
    reached = {row['light']['surface_index'] for row in rows
               if row['light'] is not None and row['light']['surface_index'] is not None}
    reached.update(row['light']['surface_index'] for row in mixed if row['light'] is not None)
    assert mixed[0]['pixels'] != mixed[1]['pixels'], 'mixed body order did not affect the native destination'
    return (rows, [mask for mask in m.masks if mask['index'] in reached],
            bytes(m.u.mem_read(0x83358C, 80)).hex(), mixed)


def welding_generate(map_paths=()):
    from tools.spatial_oracle.ifv_turret_switching import Machine as IfvReader, PAIR_NAMES
    selected_reads = {
        'HoldsWhat', 'BehavesLike', 'SpawnFrames', 'ParticleCap', 'SpawnRadius', 'Lifetime',
        'SpawnDirection', 'SparkSpawnFrames', 'LightSize', 'OneFrameLight', 'SpawnSparkPercentage',
        'MaxEC', 'ColorSpeed', 'XVelocity', 'YVelocity', 'MinZVelocity', 'ZVelocityRange',
        'StartColor1', 'StartColor2', 'ColorList', 'Damage', 'Image', 'Velocity',
        'StateAIAdvance', 'Translucency', 'MaxDC', 'DefaultSparkSystem', 'Gravity',
    }
    weapon_reads = {'UseSparkParticles', 'AttachedParticleSystem', 'ROF', 'Burst',
                    'Damage', 'Range', 'Projectile', 'Warhead'}
    ifv_keys = {'IFVMode', 'TurretCount', 'WeaponCount', 'IsChargeTurret'} | {
        prefix + suffix for prefix in PAIR_NAMES for suffix in ('TurretWeapon', 'TurretIndex')}
    profiles = []
    for path in (None, *map_paths):
        m = WeldingBirth(dict(name='physical_type_inputs'), map_path=path, actors=False)
        m.checked()
        layers, _ = welding_layers(path)
        ifv = {}
        for type_id in ('FV', 'ENGINEER'):
            reader = IfvReader(type_id)
            history = []
            for name, _archive, raw in layers:
                if raw is None:
                    history.append(dict(name=name, absent=True))
                    continue
                sections, _ = lexical(raw, {type_id})
                selected = {section: {key: value for key, value in keys.items() if key in ifv_keys}
                            for section, keys in sections.items()}
                history.append(reader.read_layer(selected, name))
            ifv[type_id] = dict(constructor=reader.constructor, layers=history,
                                result=reader.type_state())
        type_history = [dict(row, reads=[read for read in row['reads'] if read['key'] in selected_reads])
                        if not row.get('absent') else row for row in m.type_layers]
        weapon_history = [dict(row, reads=[read for read in row['reads'] if read['key'] in weapon_reads])
                          if not row.get('absent') else row for row in m.weapon_layers]
        profiles.append(dict(name='Hills' if path is None else Path(path).stem,
            map=m.map_identity, constructor=m.type_constructor, layers=type_history,
            effective_types=m.effective_types,
            weapon=dict(constructor=m.weapon_constructor, layers=weapon_history,
                        effective=welding_weapon_state(m, m.weapon)),
            ifv=ifv,
            text_unchanged=True))
    # Each profile executes the actual layer history, including absent sections;
    # agreement is a native output check, not a host parser prediction.
    assert all(profile['effective_types'] == profiles[0]['effective_types'] for profile in profiles)
    assert all(profile['weapon']['effective'] == profiles[0]['weapon']['effective'] for profile in profiles)
    constructors = []
    controls = (
        ('entity_owner', {}),
        ('entity_owner_tarcom_differs', dict(tarcom='other_target')),
        ('entity_no_owner', dict(owner=None)),
        ('null_target_owner', dict(supplied_target=None)),
        ('null_target_no_owner_fifth', dict(supplied_target=None, owner=None, fifth=(7, -9, 31))),
        ('owner_tarcom_null', dict(tarcom=None)),
        ('cell_ground', dict(supplied_target='cell', owner=None)),
        ('cell_bridge', dict(supplied_target='cell', owner=None, bridge=True)),
    )
    mobile_inputs = None
    for name, control in controls:
        m = WeldingBirth(dict(name=name))
        constructors.append(dict(name=name, **m.construct(**control)))
        if mobile_inputs is None:
            mobile_inputs = m.mobile_inputs
    # The actual common constructor, rather than a Welding-only substitute,
    # also executes with the two already-existing effect families.
    for system, particle in (('SmallGreySSys', 'SmallGreySmoke'), ('SparkSys', 'Spark')):
        for suffix, control in (('entity_tarcom_differs', dict(tarcom='other_target')),
                                ('null_target_owner', dict(supplied_target=None))):
            name = system + '_' + suffix
            m = WeldingBirth(dict(name=name), system_name=system, particle_name=particle)
            constructors.append(dict(name=name, **m.construct(**control)))
    births = [WeldingBirth(case).fire_birth() for case in (
        dict(name='ordinary'), dict(name='flag_disabled', use_spark_particles=False),
        dict(name='retained_link', retained_link=True),
        dict(name='allocation_failure', fail_system_allocation=True))]
    admissions = [welding_admission_case(flag, live) for flag in (False, True) for live in (False, True)]
    admissions.append(welding_admission_case(True, True, berserk=True))
    live = welding_live_case()
    types = profiles[0]['effective_types']
    points = welding_draw_cases(types, live['first_produced_particle'])
    systems, masks, stages, mixed = welding_system_draw_cases(types,
        next(row['input'] for row in points if row['input']['name'] == 'a_127'))
    return dict(schema_version=1,
        type_inputs=dict(profiles=profiles), constructor_priors=mobile_inputs,
        constructor_cases=constructors, birth_cases=births, admission_cases=admissions,
        lifecycle_cases=welding_expiry_cases(), live_case=live,
        spark_draw_cases=points, system_draw_cases=systems,
        mixed_draw_cases=mixed, light_masks=masks, stage_table_hex=stages)


def generate():
    births = [ElectricBirth(case).execute() for case in (
        dict(name='ordinary'), dict(name='electric_gate_disabled', is_electric_bolt=False),
        dict(name='laser_precedence', is_laser=True), dict(name='bolt_allocation_failure', new_fails=True),
        dict(name='alternate_control', alternate=True),
        dict(name='reselect_secondary', reselect_secondary=True),
        dict(name='cleared_retained_target', clear_retained_target=True))]
    endpoint = births[0]['bolts'][0]
    spark = [SparkBirth(case, endpoint).execute() for case in (
        dict(name='ordinary_detail2'), dict(name='detail0', detail=0),
        dict(name='detail1', detail=1),
        dict(name='size0', system_type_overrides={'LightSize':'0'}),
        dict(name='size_negative', system_type_overrides={'LightSize':'-1'}),
        dict(name='size256', system_type_overrides={'LightSize':'256'}),
        dict(name='one_frame_light', system_type_overrides={'OneFrameLight':'yes'}),
        dict(name='native_id_wrap', native_id=0x7FFFFFFE, seed=2),
        dict(name='system_allocation_failure', fail_system_allocation=True))]
    return dict(schema_version=1,
                palette=physical_palette(),
                type_inputs={name: prism.laser_type_inputs(name) for name in ('TESLA', 'GAPOWR')},
                weapon_inputs=physical_weapon()[2], birth_cases=births, spark_cases=spark,
                live_logic_cases=[LiveSparkBirth(endpoint, occupied=occupied).execute()
                                  for occupied in (False, True)],
                charge_logic_cases=[LiveSparkBirth(endpoint, charge_birth_frame=frame,
                                                  charge_type=kind).execute()
                                    for frame, kind in ((9, 'NATSLA_B'), (10, 'NATSLA_B'),
                                                        (9, 'NATSLA_BD'))],
                light_indices=light_indices(),
                light_admission=light_admission(),
                light_draw_cases=[light_pixels()],
                draw_cases=draw_cases(endpoint), mixed_cases=[draw_case(dict(
                    name='laser_bolt_trail_overlap', mixed_families=True,
                    source=[3038,3038,0], target=[3600,2700,0], z_adjust=-34,
                    camera=[-60,250], seed=1, alpha='mixed', z=[65535,32760,32790],
                    rgb=[140,190,240], width=5, supported=True, age=7, visits=2))])


def metadata():
    result = native.provenance(scope=__doc__, assumptions=[
        'Original EBolt4C1E10/4C2A60/4C1F20/4C2830/4C29E0 and packed4BFD30 execute unchanged; supplied fixtures are bounded components, not a complete Scenario or Windows frame.',
        'Birth executes original FireAt6FF4CC..6FF656, including its IsLaser precedence, IsElectricBolt gate, Spawn6FD570, real SelectWeapon6F3330, BuildingGetWeapon/GetFLH and target coordinate receivers. Isolated birth rows record the Spark constructor boundary; spark_cases hand off their original-derived endpoints to a fresh native Init plus complete ParticleSystem62DC50 and first two live Logic55B5FF..55B61B visits. Earlier Bullet production/damage/report/rearm and later ammo/reveal are outside this slice.',
        'Physical CoilBolt/Invisible/Electric, SparkSys/Spark/DefaultSparkSystem/Gravity read through original constructors and readers from exact lexical strings in RULESMD, optionalLANGRULE, MPBattleMD and XMP03T4. Physical IO/cache preparation is supplied. TESLA/GAPOWR FLH/foundation inputs reuse original BuildingType constructor and reader slices. Prepared instance locations match the ordinary production fixture; no whole Scenario load claim.',
        'Spark world uses original empty Logic/Display/effect registries, supplied flat level0 cells42..48 by46..52, activeGame flag, nativeID cursor1000 or explicit wrapping input, and frame36/37. Whole native Unlimbo/DisplaySubmit/LogicRegister, particle constructors, SparkAI, particleAI and systemComputeCRC execute. Only the system joins Logic; its contained particles are visited by systemAI. Abstract IDs are observed, never assigned by hooks.',
        'live_logic_cases supplies the producer order established by FireAt BulletFire6FF014 before electricSpawn6FF58A. Original Bullet466380/4664C0/468670 admits a physical Invisible before Spark, then whole BulletAI4666E0/ResolveImpact468D80/Detonate4690B0/AreaDamage489280, physical TSTIMPCT construction/AI, UnInit/Conceal/LogicUnregister55BAE0, deferred drain725C70 and two live Logic passes execute. Supplied legal64x64 map rectangle, flat cells38..48 by46..52, stationary2x2 coordinate-only Building, null firer and empty damage receivers bound this control; no source BuildingAI or whole FireAt/damage-receiver execution is claimed. Actual ARTMD and TSTIMPCT.SHP bytes from ra2md/localmd and ra2/conquer feed the existing original AnimType reader.',
        'The occupied live_logic row supplies GAPOWR at four ground Cell+E4 links only after the original Bullet impact and before Spark firstAI. The existing BuildingType constructor and original LaserFence460AA0/UndeploysInto71329D readers consume physical layers; the existing foundation reader supplies index3. Original Cell47C520, Building457620/465D40, Spark62C6E0 collision, backward childUnInit, system retirement and deferred destruction execute. No Building placement, occupied-world AreaDamage or target damage is claimed. State arrays retain observed allocations for inspection; alive/Logic flags, particle membership and pending queue establish retirement, not allocation-array presence. The retired system checksum is skipped.',
        'charge_logic_cases adds physical NATSLA_B/NATSLA_BD from ARTMD.INI and NGTSLA_B.SHP to the same live world before Bullet creation. Original AnimType427530/427D00, Anim421EA0/423AC0, completion424B31, UnInit4255B0, pointer expiry7258D0, Logic unregister and deferred destruction execute. Supplied birth frame9 or10, delay0/loops1/flags1600 and Building caller marker118=1 produce all remaining timer/frame/completion state natively. The first case warms up at frames9..35, then charge expiry at36 shifts the new Bullet behind the live cursor and Spark firstAI occurs before the frame37 impact. The later-birth control retains charge at36 and visits Bullet first; charge expiry at37 skips Spark again. The damaged type reader yields Start10/End19 but its constructor starts currentFrame0, so birth9 remains live through both observed passes and Spark firstAI follows impact at37. Empty receivers and no source Building AI or Building expiry listener bound these rows; Active3 restoration is independently owned by building_slot_replacement.expiry delayed_fire cases. Neither a whole firing-frame scene nor full FireAt RNG prefix is established.',
        'All three original RNG objects are seeded through65C6D0. Main requests observe raw stores including RandomRanged rejection; full states and the supplied Scenario native-ID cursor surround every tactical visit. Repeated visits retain the same supplied Logic frame.',
        'RGB565 surfaces, initial destination, A/Z planes, camera and endpoints are explicit inputs. Original BSurface lock/stride/unlock replaces DirectDraw transport; original projection, clipping and packed drawing execute. Physical palette.pal/anim.pal startup uses the existing Convert producer.',
        'caller_coordinate_residual supplies ordinary native source.x minus one lepton to original EBolt constructor/Init/draw. It bounds downstream rendering and Main RNG for the preserved shared caller coordinate; it does not establish upstream GetFLH equality.',
        'Mixed family rows execute original Laser550240, EBolt4C2830, LineTrail556D40 sequentially on one retained destination and unchanged A/Z. Trail constructor556A20 and update556B70 produce its two-node ring from explicit owner positions; no Python line or particle algorithm supplies output.',
        'Ordinary Spark persistent light executes original5FF250 and10UpdateAll5FF390 calls. Draw-index controls run whole5FF850 prefix through projection/gates up to5FF93F, before the unchecked native surface-array lookup; out-of-bank indices are observations, not valid rendering fixtures. Light draw rows execute original5FF420 mask loop for slots0..14 and whole5FFFA0/5FF850/7DEEFA with actual masks, supplied RGB565 mode2 and no optional color LUT. The unrelated type16 mask bank and full Windows startup are excluded.',
        'Light admission rows run original reverse5FFFA0 and5FF850 through FPS, projection and optional fog calls; admitted draws redirect at5FF8B8 to the original epilogue before mask access. Threshold55AF60 and retail fog stub5865E0 execute unchanged. FPS/minimum/buffer/latch are supplied runtime state; full OS clock and frame throttle remain owned by building_prism laser_fps evidence.',
    ], substitutions=[
        'Isolated draw rows record and return ParticleSystem62DC50 without executing its world producer; separate birth evidence owns that constructor and its continuation. Allocator/delete and CRT atexit transport are supplied.',
        'Existing BulletReader supplies allocator/delete/CRT/TLS, cached INI objects and empty sound/archive bindings. Type report names remain unresolved in this component and do not establish audio playback or whole-frame audio scheduling. Existing Building scene supplies unrelated admission/power state; no attached infantry bolt path, charge donors, overpowered Tesla, movement or aircraft behavior is claimed.',
        'Live Bullet placement reuses projectile_trailer.fire_bullet; existing ifv_launch/ifv_impact helpers supply only verified Windows InterlockedIncrement/Decrement and mapped-memory IsBadReadPtr transport. No predecessor AI, detonation, vector-compaction, RNG or Spark result is substituted.',
    ], entry_points=dict(constructor=CTOR, init=INIT, draw=DRAW, manager=MANAGER,
                         clear=CLEAR, packed_line=0x4BFD30, projection=0x6D2140,
                         ranged=0x65C7E0, raw=0x65C780, spawn=0x6FD570, create=0x6FD460,
                         spark_system_ctor=0x62DC50, particle_ctor=0x62B5E0,
                         logic_loop=0x55B5FF, system_ai=0x62FD60, spark_ai=0x62E840,
                         system_crc=0x630100, native_id=0x68BCB0,
                         bullet_ctor=0x466380, bullet_configure=0x4664C0, bullet_fire=0x468670,
                         bullet_ai=0x4666E0, impact_resolve=0x468D80, detonate=0x4690B0,
                         area_damage=0x489280, logic_unregister=0x55BAE0,
                         spark_particle_ai=0x62C6E0, cell_building=0x47C520,
                         building_contact_gate=0x457620, one_cell_undeployer=0x465D40,
                         impact_anim_ctor=0x421EA0, impact_anim_ai=0x423AC0, deferred_drain=0x725C70,
                         anim_complete=0x424B31, anim_uninit=0x4255B0, pointer_expiry=0x7258D0,
                         light_ctor=0x5FF250, light_update=0x5FF390, light_draw=0x5FF850,
                         light_masks=0x5FF420, light_rgb565=0x7DEEFA))
    result['reproduction_commands'] = {
        'working_directory': 'repository root with the configured retail installation',
        'check': 'python -m tools.procedural_drawing_oracle.electric_bolt --check',
        'write': 'python -m tools.procedural_drawing_oracle.electric_bolt --write',
    }
    return result


def welding_metadata():
    out = native.provenance(
        scope='Original RepairBullet Welding constructor, live-link admission/expiry, Spark AI and whole Particle/System packed drawing',
        assumptions=[
            'Optional --welding mode has a separate consumed corpus. Default electric_bolt outputs retain their historical schema and controls. Original image text and vtables are never patched.',
            'Existing stock.mix/lexical/cache transport supplies physical RULESMD, optional LANGRULE, MPBattleMD, unchanged inner HILLS.MAP from loose Hills.mmx and each explicitly supplied generated scenario INI. Original complete ParticleSystemType6440A0/6442D0, ParticleType644BE0/644F50 and WeaponType771C70/772080 own selected defaults/read semantics. Layer hashes are physical inputs, not repository source pins.',
            'Fresh FV/ENGINEER histories reuse ifv_turret_switching.Machine.read_layer unchanged: only its IFVMode, fixed FV turret mappings, TurretCount, WeaponCount and IsChargeTurret scope is claimed. Gunner/passenger limits/numbered weapons and complete boarding retain their existing production/native owners. These scenario maps author no effect-type overrides; native selected outputs must agree across profiles.',
            'Existing BulletReader/SparkBirth supplies the one reader/world VM for each independent birth/control. After initialize_effect_registries executes original7252D0 and resets the B0F720 header, existing prepare_spark_capacity supplies its empty capacity once before the first actor. shroud._prepare_unit_rank owns original Unit7353C0..7354CE before CoCreate, Foot/Techno prefixes and Drive4AF540. Its selected reuse flag preserves that capacity; actual Object5F3A76..5F3A8B appends every actor, each guarded exactly once after all three constructors. Native72593E..72595F notification traverses this registry independently of Logic. Type constructors, stationary coordinates, ownerHouseROF1, Health100/alive and an unplaced/non-Logic source are explicit component priors, not a whole Unit construction, placement, boarding, target acquisition or Bullet launch.',
            'Original static initializer6F2A50 writes actual B0EA90 zero-coordinate bytes. Whole ParticleSystem62DC50 executes with Entity/NULL/Cell target, owner/NULL and ownerTarCom different from supplied target. The existing real Cell getter/RTTI and bridge flag140/AC4A8C416 own the Cell controls. Retail SmallGreySSys/SmallGreySmoke and SparkSys/Spark controls use the same constructor, not caller-specific behavior.',
            'FireAt birth6FF1AD..6FF1FB supplies its already-computed muzzle and actual target; original pushes, allocation, complete constructor and Techno+308 store execute. Earlier Bullet/healing/report/FLH/FireAt admission and the full shot scheduler are excluded. Guard6FDE36, T37 at6FC6D5 and T46 at6FC9C7 start from declared original-register/stack priors and run to their native continuation or return. Whole GetROF6FCFA0 and actual FireAt6FF274..6FF2D1 own ROF, Berserk+298 halving, timer stores and signed burst division.',
            'Existing RngTrace uses Main seed1, Scenario seed2 and Mapgen seed3 for default case seed1. Seeds are reset after supplied mobile preparation. Constructors consume original Abstract IDs; no hook supplies IDs, probability, velocity, life, color, RNG value, timer, collision or pointer-expiry outcome. Admission rows deliberately vary current weapon flag and retained native pointer to test their intersection.',
            'Whole Techno7077C0 and System62FE90 PointerExpired execute with both removed controls, matching and different pointers. Actual target setter6FCDB0 and Stop NULL-destination/target setter fragment4C75E6..4C75FE execute. Full Event admission/queue/mission dispatch and every possible target writer are excluded. Natural continuation retains original live Logic55B5FF..55B61B, System62FD60, Spark62E840, child62C6E0 and actual725C70 deferred drain after each visit, bounded to600 visits on the existing flat-cell fixture. Spawn boundaries, final pre-drain still-allocated System and at most five queued children are separate from post-drain live rosters/pending/source link/IDs/RNG; freed bytes are never live state. Only compact boundaries/RNG totals and final child registry/physical flags are retained, not a replay dump.',
            'Whole ParticleDraw62CEC0 uses declared valid physical-Welding color states plus one originally produced child. The actual Draw ABI is ECX=receiver, argument2=bounds, RET8; selected Spark/System bodies ignore argument1, suppliedNULL. Native55AF60 owns FPS hysteresis; actual5865E0 runs under its optional retail fog gates and returns its active-retail false. Original6D2140/6D20E0 projection, clip, full-u16 A read, strict signed candidate versus unsigned Z, x87 interpolation, A modulation, format packing and BSurface7BAEB0 pixel store execute. FPCW0E7F is the existing owner PC53/chop input. Invalid color indices, NaN/Infinity, malformed types and other particle families are excluded.',
            'Whole SystemDraw62E280 owns positive count/cap/light admission, binary64(0.4)/binary32(1.0) clamp, x87 multiply/ftol, temporary5FF250 light, current F4 stage, complete5FF850 and immediate destruction. Original normal-bank5FF420 prefix constructs slots0..24; LightSize25/table maximum63 bounds all selected indices, whose actual whole-draw values select the retained masks. No persistent-light ageing, type16 bank or hand light raster supplies a result.',
            'RGB565 destination, BSurface/A/Z circular planes, current camera/viewport/format, raw u32 FPS/detail state and signed i16 Z terms are declared transport. Original6D2140 chooses the explicit80,60 crop where used. Draws must preserve all three RNG states, native ID cursor, A/Z planes, destination guards and original text. Mixed rows execute complete System then Particle bodies and reversed component order on one destination without reset; they do not execute or certify the full Display object loop or common Object Render prefix.',
        ], substitutions=[
            'Reuse existing BulletReader allocator/delete/CRT/TLS, cached INI and empty archive/sound transport, existing native.initialize_empty_windows_seh storage for original CRT RTTI, and existing ifv_impact.retirement_transport for Windows atomic-decrement/checked-readability calls during actual deferred destruction. Original RTTI/receiver bodies still execute. Existing original mobile/flat-world fixtures supply only their documented constructor/lifecycle/world boundaries; no new mapper, VM, scalar parser or gameplay stub is introduced.',
            'Reuse LightPixels/Rally BSurface storage/lock/stride and existing operator-new/delete/atexit transport. Draw controls contain no EBolt and assert the older isolated ParticleSystem producer boundary is never reached. Original System/Particle/light numerical bodies and real BSurface pixel methods execute unchanged.',
        ], entry_points=dict(zero_coord_init=0x6F2A50, system_ctor=0x62DC50,
            system_type_ctor=0x6440A0, system_type_reader=0x6442D0,
            particle_type_ctor=0x644BE0, particle_type_reader=0x644F50,
            weapon_ctor=0x771C70, weapon_reader=0x772080,
            fireat_birth=0x6FF1AD, fireat_guard=0x6FDE36, t37=0x6FC6D5, t46=0x6FC9C7,
            get_rof=0x6FCFA0, fireat_rearm=0x6FF274,
            techno_pointer_expired=0x7077C0, system_pointer_expired=0x62FE90,
            set_target=0x6FCDB0, stop_setters=0x4C75E6,
            logic=0x55B5FF, system_ai=0x62FD60, spark_ai=0x62E840,
            spark_particle_ai=0x62C6E0, particle_draw=0x62CEC0, system_draw=0x62E280,
            projection=0x6D2140, adjust_z=0x6D20E0, detail=0x55AF60, fog=0x5865E0,
            pixel=0x7BAEB0, light_ctor=0x5FF250, light_draw=0x5FF850,
            light_destructor=0x5FF2D0, light_masks=0x5FF420))
    out['reproduction_commands'] = dict(
        working_directory='repository root with configured original retail installation',
        check='python -m tools.procedural_drawing_oracle.electric_bolt --welding --welding-map <ordinary-ifv-welding.map> --welding-map <ordinary-ifv-full-health.map> --check',
        write='python -m tools.procedural_drawing_oracle.electric_bolt --welding --welding-map <ordinary-ifv-welding.map> --welding-map <ordinary-ifv-full-health.map> --write')
    return out


if __name__ == '__main__':
    import argparse
    selector = argparse.ArgumentParser(add_help=False)
    selector.add_argument('--welding', action='store_true')
    selector.add_argument('--welding-map', type=Path, action='append', default=[])
    selected, remaining = selector.parse_known_args()
    if selected.welding_map and not selected.welding:
        selector.error('--welding-map requires --welding')
    sources = {name: Path(module.__file__) for name, module in sorted(sys.modules.items())
               if name.startswith('tools.') and getattr(module, '__file__', None)
               and str(module.__file__).endswith('.py')}
    sources['producer'] = Path(__file__)
    if selected.welding:
        for name in ('shroud', 'ifv_turret_switching'):
            sources[name] = Path(__file__).parents[1] / (
                'procedural_drawing_oracle' if name == 'shroud' else 'spatial_oracle') / (name + '.py')
        native.finish_vectors(lambda: welding_generate(selected.welding_map),
            Path(__file__).with_name('welding.json'), provenance=welding_metadata,
            argv=remaining, source_paths=sources)
    else:
        native.finish_vectors(generate, Path(__file__).with_suffix('.json'),
                              provenance=metadata, argv=remaining, source_paths=sources)
