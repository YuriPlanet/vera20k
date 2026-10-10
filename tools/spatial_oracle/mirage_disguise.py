"""Original stationary MGTK disguise on the existing constructed Foot fixture.

FootMissions owns the native VM, physical map/type prerequisites, placement,
RNG and receiver transport. This producer adds observations and original calls;
it does not implement the disguise decisions in Python.
"""
from pathlib import Path
import hashlib
import json
import os
import struct
import sys

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, NativeCallTrace, finish_vectors, image_sha256, provenance,
    run_checked,
)
from tools.projectile_oracle.bridge_render_inputs import BulletReader, lexical
from tools.spatial_oracle.anytown_damage import mtnk_attack as base
from tools.spatial_oracle.anytown_damage.foot_missions import FootMissions, source_paths
from tools.spatial_oracle.building_body_rules import INI, RULES, SP, dwords
# attach_world imports this existing owner lazily; pin it before generation.
from tools.spatial_oracle import fire_error as _fire_error
from tools.spatial_oracle.engineer_repair_admission import NativeAudioPlatform
from tools.spatial_oracle.naval_lifetime_controls import raw_load_sound_reset
from tools.spatial_oracle.naval_occupants import Native as NavalNative

HERE = Path(__file__).resolve().parent
TYPE_KEYS = {'CanDisguise': 0xD2F, 'PermaDisguise': 0xD30,
             'DetectDisguise': 0xD31, 'DisguiseWhenStill': 0xD32}


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def type_flags(m, typ):
    return {key: m.u.mem_read(typ + offset, 1)[0]
            for key, offset in TYPE_KEYS.items()}


def unit_art_state(m, typ):
    fields = {'standing_frames': 0xE1C, 'death_frames': 0xE20,
              'death_frame_rate': 0xE24, 'start_stand_frame': 0xE28,
              'start_walk_frame': 0xE2C, 'start_firing_frame': 0xE30,
              'start_death_frame': 0xE34, 'max_death_counter': 0xE38,
              'facings': 0xE3C}
    return dict(**{name: signed(m.u, typ+offset) for name,offset in fields.items()},
                walk_frames=struct.unpack('<b', m.u.mem_read(typ+0xE5C,1))[0],
                firing_frames=struct.unpack('<b', m.u.mem_read(typ+0xE5D,1))[0],
                turret=m.u.mem_read(typ+0xCA1,1)[0])


def general_state(m, rules):
    count, data = m.read32(rules + 0x1008), m.read32(rules + 0xFFC)
    assert count < 128
    pointers = [m.read32(data + index * 4) for index in range(count)]
    return dict(disguises=[m.string(p + 0x24) if p else None for p in pointers],
                pointers=[hex(p) for p in pointers],
                infantry_blink_disguise_time=signed(m.u, rules + 0x1014))


def read_flags(m, typ, sections):
    m.rules_cache(sections)
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, typ),
                            (UC_X86_REG_EBX, typ + 0x24), (UC_X86_REG_ESI, RULES)):
        m.u.reg_write(register, value)
    run_checked(m.u, 0x714404, 0x71446C, required_addresses=[0x5295F0])
    assert m.u.reg_read(UC_X86_REG_ESP) == SP
    return type_flags(m, typ)


def read_general(m, rules, sections):
    m.rules_cache(sections)
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, rules),
                            (UC_X86_REG_EDI, RULES),
                            (UC_X86_REG_EAX, m.read32(rules + 0xFF4))):
        m.u.reg_write(register, value)
    run_checked(m.u, 0x671D3E, 0x671D92,
                required_addresses=[0x67BDD0, 0x67A310, 0x67A060, 0x5276D0])
    assert m.u.reg_read(UC_X86_REG_ESP) == SP
    return general_state(m, rules)


def reader_rows():
    root = Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
    m = BulletReader({}, root)
    m.u.mem_write(0xA8E318, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    m.invoke(0x71D580, 0)
    rules, typ = m.alloc(0x3000), m.alloc(0x1900)
    m.invoke(0x665650, rules)
    m.invoke(0x710AF0, typ, (m.cstring('MGTK'),))
    constructor = dict(type=type_flags(m, typ), general=general_state(m, rules))
    layers = []
    for name, path in base.layers():
        if not path.exists():
            assert name == 'LANGRULE.INI'
            layers.append(dict(file=name, absent=True))
            continue
        raw = path.read_bytes()
        sections, lines = lexical(raw, {'MGTK', 'General'})
        selected = {'MGTK': {k: v for k, v in sections.get('MGTK', {}).items()
                              if k in TYPE_KEYS},
                    'General': {k: v for k, v in sections.get('General', {}).items()
                                if k in ('DefaultMirageDisguises', 'InfantryBlinkDisguiseTime')}}
        before = dict(type=type_flags(m, typ), general=general_state(m, rules))
        after = dict(type=read_flags(m, typ, selected),
                     general=read_general(m, rules, selected))
        layers.append(dict(file=name, sha256=hashlib.sha256(raw).hexdigest(),
                           sections=selected, before=before, after=after))
    flag_controls = []
    for name, keys in [
        ('all_yes', {key: 'yes' for key in TYPE_KEYS}),
        ('missing', {}), ('empty', {key: '' for key in TYPE_KEYS}),
        ('wrong_case_keys', {key.lower(): 'no' for key in TYPE_KEYS}),
        ('malformed', {key: 'garbage' for key in TYPE_KEYS}),
        ('numeric_nonzero', {key: '2' for key in TYPE_KEYS}),
        ('all_no', {key: 'no' for key in TYPE_KEYS}),
        ('numeric_nonzero_after_false', {key: '2' for key in TYPE_KEYS}),
        ('mixed_case_values', dict(CanDisguise='YeS', PermaDisguise='nO',
                                   DetectDisguise='TRUE', DisguiseWhenStill='false')),
    ]:
        before = type_flags(m, typ)
        after = read_flags(m, typ, {'MGTK': keys})
        flag_controls.append(dict(name=name, sections={'MGTK': keys}, before=before, after=after))
    general_controls = []
    for name, keys in [
        ('duplicates_case', dict(DefaultMirageDisguises='TREE01,tree02,TREE01')),
        ('missing', {}), ('empty', dict(DefaultMirageDisguises='')),
        ('wrong_case_key', dict(defaultmiragedisguises='TREE04', infantryblinkdisguisetime='99')),
        ('none_tokens', dict(DefaultMirageDisguises='none,<none>,TREE03,NONE')),
        ('unknown_allocated', dict(DefaultMirageDisguises='AUTHORED_MIRAGE,TREE02,AUTHORED_MIRAGE')),
        ('mixed_case_first_allocation', dict(DefaultMirageDisguises='AuthoredTree,aUtHoReDtReE,TREE01')),
        ('sentinel_only', dict(DefaultMirageDisguises='none,<none>')),
        ('retail_restore', dict(DefaultMirageDisguises='TREE01,TREE02,TREE03,TREE04')),
        ('negative_duration', dict(InfantryBlinkDisguiseTime='-3')),
        ('empty_duration_after_negative', dict(InfantryBlinkDisguiseTime='')),
        ('malformed_duration', dict(InfantryBlinkDisguiseTime='bad')),
        ('empty_duration', dict(InfantryBlinkDisguiseTime='')),
        ('duration_suffix', dict(InfantryBlinkDisguiseTime='17tail')),
    ]:
        before = general_state(m, rules)
        count_before = m.read32(0xA8E328)
        after = read_general(m, rules, {'General': keys})
        general_controls.append(dict(name=name, sections={'General': keys}, before=before,
                                     after=after, terrain_count_before=count_before,
                                     terrain_count_after=m.read32(0xA8E328)))
    return dict(constructor=constructor, layers=layers, flag_controls=flag_controls,
                general_controls=general_controls)


class Mirage(FootMissions):
    def observe(self, u, pc, size, data):
        if pc in getattr(self, 'stream_callbacks', ()):
            # The existing raw-load helper owns these exact IStream callbacks.
            # Its hook transports bytes after this observer permits the entry.
            return
        platform = getattr(self, 'file_platform', None)
        if platform is not None and platform.hook(u, pc, size):
            return
        super().observe(u, pc, size, data)

    def read32(self, address):
        return self.m.read32(address)

    def ret(self, value=0, cleanup=0):
        self.m.ret(value, cleanup)

    def initialize_mirage(self):
        self.initialize_companion()
        m, u = self.m, self.u
        self.phase = 'setup'
        # Original static initializers produce Tactical's height projection.
        # The inherited combat fixture never required these presentation fields.
        for entry in (0x6D1830, 0x6D18C0, 0x6D1BB0):
            m.invoke(entry, 0)
        self.projection = dict(initializers=['0x6d1830','0x6d18c0','0x6d1bb0'],
                               cell_diagonal_bits=bytes(u.mem_read(0xB0CD78,8)).hex(),
                               rad60_bits=bytes(u.mem_read(0xB0CD88,8)).hex(),
                               adjust_for_z_multiplier_bits=bytes(u.mem_read(0xB0CD48,8)).hex())
        # Same registered-vector startup prior as the owning reader fixtures.
        # FootMissions does not initialize this otherwise unused registry.
        u.mem_write(0xA8E318, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
        self.mirage_type = m.alloc(0x1900)
        m.invoke(0x7470D0, self.mirage_type, (m.cstring('MGTK'),))
        self.type_constructor = type_flags(m, self.mirage_type)
        self.unit_art_constructor = unit_art_state(m, self.mirage_type)
        self.unit_art_constructor_bytes = {
            offset: bytes(u.mem_read(self.mirage_type+offset,size))
            for offset,size in ((0xE1C,0x24),(0xE5C,2))}
        raw_art = (Path(os.environ['VERA20K_SHRAPNEL_INPUTS']) / 'ARTMD.INI').read_bytes()
        art, _ = lexical(raw_art, {'MGTK', 'RTNK', 'TREE01', 'TREE02', 'TREE03', 'TREE04',
                                  'MTNK', 'GTNK', 'E1', 'GI', 'GISequence', 'VTMUZZLE'})
        self.art_input = dict(file='ARTMD.INI',sha256=hashlib.sha256(raw_art).hexdigest(),
                              sections=art)
        m.make_ini(art)
        self.mirage_layers = []
        for name, path in base.layers():
            if not path.exists():
                self.mirage_layers.append(dict(file=name, absent=True))
                continue
            raw = path.read_bytes()
            sections, _ = lexical(raw, {'General', 'MGTK', 'MirageGun', 'MirageGunE', 'InvisibleLow', 'MirageWH',
                                        'TREE01', 'TREE02', 'TREE03', 'TREE04'})
            general = {k: v for k, v in sections.get('General', {}).items()
                       if k in ('DefaultMirageDisguises', 'InfantryBlinkDisguiseTime')}
            general_read = read_general(m, self.rules, {'General': general})
            sections['General'] = general
            m.rules_cache(sections)
            admitted = m.invoke(0x747620, self.mirage_type, (RULES,)) & 255
            weapon_reads = []
            for weapon in {m.read32(self.mirage_type + 0x898)} - {0}:
                weapon_al = m.invoke(0x772080, weapon, (RULES,)) & 255
                projectile, warhead = m.read32(weapon + 0xA0), m.read32(weapon + 0xAC)
                projectile_al = m.invoke(0x46BEE0, projectile, (RULES,)) & 255
                warhead_al = m.invoke(0x75D3A0, warhead, (RULES,)) & 255
                m.invoke(0x7729F0, weapon)
                weapon_reads.append(dict(name=m.string(weapon+0x24), weapon_al=weapon_al,
                                         projectile_al=projectile_al, warhead_al=warhead_al))
            terrain_reads = []
            for pointer in [m.read32(m.read32(self.rules + 0xFFC) + i * 4)
                            for i in range(m.read32(self.rules + 0x1008))]:
                terrain_reads.append(dict(name=m.string(pointer+0x24),
                                          admitted=m.invoke(0x71DEA0, pointer, (RULES,)) & 255))
            self.mirage_layers.append(dict(file=name, sha256=hashlib.sha256(raw).hexdigest(),
                                           admitted=admitted, sections=sections,
                                           flags=type_flags(m, self.mirage_type),
                                           general=general_read,
                                           image=m.string(self.mirage_type + 0x1F8),
                                           unit_art=unit_art_state(m,self.mirage_type),
                                           weapon_reads=weapon_reads, terrain_reads=terrain_reads))
        # The existing Foot fixture does not need token retirement. Execute its
        # original global vector initializer before constructing this actor.
        m.invoke(0x633900, 0)
        self.actor = m.alloc(0x1000)
        # An explicit allocation-content control: the full constructor owns all
        # writes. The middle timer word is observed, never given a cell meaning.
        for offset, size in ((0x1D8, 1), (0x1DC, 16), (0x518, 8)):
            u.mem_write(self.actor + offset, b'\xA5' * size)
        m.invoke(0x7353C0, self.actor, (self.mirage_type, 0))
        self.actor_constructor = self.disguise_state()
        u.mem_write(self.actor + 0x21C, dwords(self.house))
        u.mem_write(self.actor + 0x14C, dwords(self.house))
        self.phase = 'placement'
        self.mirage_unlimbo = []
        for xy in ((86, 51), (86, 52), (88, 51), (85, 50)):
            if xy not in self.resident.ptrs:
                continue
            m.invoke(0x486840, self.resident.ptrs[xy], (self.coord,))
            result = m.invoke(0x737BA0, self.actor, (self.coord, 0x80)) & 255
            self.mirage_unlimbo.append(dict(cell=list(xy), returned_al=result))
            if result:
                self.actor_cell = xy
                break
        assert self.mirage_unlimbo[-1]['returned_al'], self.mirage_unlimbo
        # The inherited fixture has an unrelated enemy MTNK. Original Limbo
        # removes it before the bounded ordinary standing/no-target UnitAI.
        self.enemy_limbo = m.invoke(0x7440B0, self.candidate) & 255
        assert u.mem_read(self.candidate + 0x81, 1)[0]
        asset_root = Path(os.environ['VERA20K_MIRAGE_ASSETS'])
        files = {f'TREE{i:02d}.TEM': (asset_root / f'TREE{i:02d}.TEM').read_bytes()
                 for i in range(1, 5)}
        self.file_platform = NativeAudioPlatform(self, asset_root)
        self.file_platform.configure_transport(prepared_files=files)
        self.phase = 'setup'
        # Original Terrain InitTheater forms names, allocates and reads complete
        # SHPs through the existing immutable RawFile/Win32 transport owner.
        # Prepared archive winners replace archive startup, not the image reader.
        m.invoke(0x71DCA0, 0)
        self.tree_images = []
        for i in range(m.read32(self.rules + 0x1008)):
            typ = m.read32(m.read32(self.rules + 0xFFC) + i*4)
            shp = m.read32(typ + 0xA4)
            self.tree_images.append(dict(name=m.string(typ+0x24), pointer=hex(typ),
                                         image_pointer=hex(shp), voxel=u.mem_read(typ+0x236,1)[0],
                                         header8=bytes(u.mem_read(shp,8)).hex() if shp else None))
        assert all(row['image_pointer'] != '0x0' for row in self.tree_images), self.tree_images
        for off in (0x5528, 0x5578):
            u.mem_write(self.enemy + off + 4, dwords(m.alloc(4096), 1024))
            u.mem_write(self.enemy + off + 0x10, dwords(0))
        self.enemy_e1 = m.alloc(0x1000)
        m.invoke(0x517A50, self.enemy_e1, (self.e1_type, 0))
        u.mem_write(self.enemy_e1 + 0x21C, dwords(self.enemy))
        u.mem_write(self.enemy_e1 + 0x14C, dwords(self.enemy))
        self.ring = m.alloc(0x200)
        self.phase = 'logic'
        self.bind_entries()

    def bind_entries(self):
        m = self.m
        self.entries = {0x7360C0: ('unit_ai', 0, 0), 0x4DA530: ('foot_ai', 0, 0),
                        0x7468C0: ('update_disguise', 0, 0), 0x746720: ('clear_disguise', 0, 0),
                        0x47EC40: ('first_infantry', 1, 4), 0x70CCF0: ('radar_dirty', 0, 0),
                        0x65C7E0: ('rng_range', 2, 8), 0x65C780: ('rng_raw', 0, 0),
                        0x737C90: ('unit_damage', 7, 28), 0x701900: ('techno_damage', 7, 28),
                        0x4D7330: ('foot_damage', 7, 28), 0x5F5390: ('object_damage', 7, 28),
                        0x50B6F0: ('house_player_control', 0, 0), 0x4F9A90: ('allied_object', 1, 4),
                        0x522780: ('infantry_clear_disguise', 0, 0), 0x41C030: ('base_clear_disguise', 0, 0),
                        0x4DE5D0: ('foot_uninit', 0, 0), 0x725C70: ('deferred_drain', 0, 0),
                        0x735780: ('unit_destructor', 0, 0), 0x4D3590: ('foot_destructor', 0, 0),
                        0x6F4500: ('techno_destructor', 0, 0), 0x7258D0: ('detach_all', 0, 0),
                        0x405C00: ('release_sound', 0, 0)}
        loco = m.read32(self.actor + 0x674)
        self.entries[m.read32(m.read32(loco)+0x10)] = ('drive_is_moving', 1, 4)

    def disguise_state(self):
        m, u, p = self.m, self.u, self.actor
        typ, house = m.read32(p + 0x518), m.read32(p + 0x51C)
        return dict(disguised=u.mem_read(p + 0x1D8, 1)[0],
                    creation_frame=signed(u, p + 0x1DC), type=m.string(typ + 0x24) if typ else None,
                    type_pointer=hex(typ), house_pointer=hex(house),
                    reveal_start=signed(u, p + 0x1E0), reveal_middle_raw=m.read32(p + 0x1E4),
                    reveal_duration=signed(u, p + 0x1E8),
                    xyz=base.xyz(u, p + 0x9C), health=signed(u, p + 0x6C),
                    alive=u.mem_read(p + 0x90, 1)[0], limbo=u.mem_read(p + 0x81, 1)[0],
                    attached=hex(m.read32(p + 0x2C8)))

    def checkpoint(self):
        return ([(a, bytes(self.u.mem_read(a, b-a+1))) for a,b,_ in self.u.mem_regions()],
                self.u.context_save(), self.m.cursor, self.actor, self.frame)

    def restore(self, state):
        for address, raw in state[0]:
            self.u.mem_write(address, raw)
        self.u.context_restore(state[1])
        self.m.cursor, self.actor, self.frame = state[2:]
        self.events.clear(); self.pending.clear(); self.trace.clear()

    def relocate_infantry(self, actor, cell):
        m, u = self.m, self.u
        if not u.mem_read(actor+0x81, 1)[0]:
            assert m.invoke(0x51DF10, actor) & 255
        m.invoke(0x486840, self.resident.ptrs[cell], (self.coord,))
        result = m.invoke(0x51DFF0, actor, (self.coord, 0x80)) & 255
        return dict(actor=hex(actor), cell=list(cell), returned_al=result,
                    xyz=base.xyz(u, actor+0x9C), limbo=u.mem_read(actor+0x81,1)[0])

    def rng_state(self):
        return {key: bytes(self.u.mem_read(pointer, 0x3F4)).hex()
                for key, pointer in self.resident.rngs.items()}

    def visit(self, name, frame, entry=0x7468C0, args=()):
        m, u = self.m, self.u
        self.frame = frame
        u.mem_write(0xA8ED84, dwords(frame))
        calls, writes, opaque_reads, raw_draws = [], [], [], []
        trace = NativeCallTrace(u, m.read32, calls)
        before, rng_before = self.disguise_state(), self.rng_state()
        foot_before = self.navigation_snap(self.actor, 0) if entry == 0x7360C0 else None

        def observe(uc, pc, size, data):
            sp = uc.reg_read(UC_X86_REG_ESP)
            trace.returned(pc, sp)
            if pc in self.entries:
                trace.entered(pc, sp, self.entries[pc])
            if pc in (0x65C84B, 0x65C79D):
                raw_draws.append(dict(pc=hex(pc), value=uc.reg_read(UC_X86_REG_ESI)))

        def written(uc, access, address, size, value, data):
            if self.actor + 0x1D8 <= address < self.actor + 0x1EC or address in (
                    self.actor + 0x518, self.actor + 0x51C):
                writes.append(dict(pc=hex(uc.reg_read(UC_X86_REG_EIP)), offset=hex(address-self.actor),
                                   size=size, value=value))

        def read(uc, access, address, size, value, data):
            if address < self.actor + 0x1E8 and address + size > self.actor + 0x1E4:
                opaque_reads.append(dict(pc=hex(uc.reg_read(UC_X86_REG_EIP)),
                                         offset=hex(address-self.actor), size=size))

        hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_WRITE, written),
                 u.hook_add(UC_HOOK_MEM_READ, read, begin=self.actor+0x1E4, end=self.actor+0x1E7)]
        try:
            result = m.invoke(entry, self.actor, args)
            trace.returned(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
        finally:
            for hook in hooks:
                u.hook_del(hook)
        return dict(name=name, frame=frame, entry=hex(entry), args=list(args),
                    before=before, after=self.disguise_state(), rng_before=rng_before,
                    rng_after=self.rng_state(), returned_eax=result, calls=calls,
                    writes=writes, reveal_middle_reads=opaque_reads, raw_draws=raw_draws,
                    **(dict(foot_before=foot_before, foot_after=self.navigation_snap(self.actor, 0))
                       if foot_before is not None else {}))


def radar_control(n, name, observer, disguised=True, allied=False):
    m,u=n.m,n.u
    state=n.checkpoint()
    try:
        u.mem_write(0xA83D4C,dwords(observer)); u.mem_write(n.actor+0x1D8,bytes([disguised]))
        u.mem_write(n.actor+0x51C,dwords(0)); u.mem_write(n.house+0x5788,dwords((1<<m.read32(observer+0x30)) if allied else 0))
        # Original constructor slice supplies +330; only registry/name prior is supplied.
        scheme=m.alloc(0x400)
        for reg,val in ((UC_X86_REG_ESI,scheme),(UC_X86_REG_EBX,0),(UC_X86_REG_EAX,53),(UC_X86_REG_ESP,SP)):
            u.reg_write(reg,val)
        run_checked(u,0x68C769,0x68C7DC)
        u.mem_write(scheme+0x304,dwords(m.cstring('LightGrey'))); u.mem_write(scheme+0x310,dwords(1))
        registry=m.alloc(4);u.mem_write(registry,dwords(scheme));u.mem_write(0xB054D4,dwords(registry));u.mem_write(0xB054E0,dwords(1))
        # Existing original BSurface format, explicit 4x4 RGB565 memory backing.
        surface,pixels,radar,point=(m.alloc(z) for z in (32,32,0x1600,8))
        u.mem_write(surface,dwords(0x7E2070,4,4,0,2,pixels,32,0));u.mem_write(pixels,b'\xA5'*32)
        u.mem_write(radar+0x121C,dwords(surface));u.mem_write(point,dwords(1,1))
        u.mem_write(n.house+0x56F9,bytes([31,127,223]))
        for address,value in ((0x8A0DD0,11),(0x8A0DD4,3),(0x8A0DD8,0),(0x8A0DDC,3),(0x8A0DE0,5),(0x8A0DE4,2)):
            u.mem_write(address,dwords(value))
        u.mem_write(n.actor+0x174,dwords(-1,0,0))
        before=n.rng_state(); calls=[]
        trace=NativeCallTrace(u,m.read32,calls)
        entries={0x7465F0:('disguise_house',1,4),0x68CA50:('find_scheme',0,0),m.read32(0x7E2070+0x24):('surface_set_pixel',2,8)}
        def observe(uc,pc,size,data):
            sp=uc.reg_read(UC_X86_REG_ESP);trace.returned(pc,sp)
            if pc in entries:trace.entered(pc,sp,entries[pc])
        h=u.hook_add(UC_HOOK_CODE,observe)
        try:
            for reg,val in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,radar),(UC_X86_REG_EBP,n.actor),(UC_X86_REG_EDI,point)):
                u.reg_write(reg,val)
            run_checked(u,0x655F48,0x65608F,required_addresses=[0x65604D,0x65608C])
            trace.returned(0x65608F,u.reg_read(UC_X86_REG_ESP))
        finally:u.hook_del(h)
        return dict(name=name,input=dict(observer=hex(observer),disguised=disguised,owner_allied=allied,house_rgb=[31,127,223],point=[1,1],surface_rgb565=True),constructor_index=m.read32(scheme+0x330),returned_ebx=u.reg_read(UC_X86_REG_EBX),pixel=struct.unpack('<H',u.mem_read(pixels+10,2))[0],pixels_hex=bytes(u.mem_read(pixels,32)).hex(),calls=calls,rng_before=before,rng_after=n.rng_state())
    finally:n.restore(state)

def draw_inputs(n):
    m, u = n.m, n.u
    cell = n.resident.ptrs[n.actor_cell]
    facing_out = m.alloc(4)
    m.invoke(0x4C93D0, n.actor+0x388, (facing_out,))
    loco = m.read32(n.actor+0x674)
    moving = m.invoke(m.read32(m.read32(loco)+0x10), 0, (loco,)) & 255
    return dict(xyz=base.xyz(u, n.actor+0x9C), cell=list(n.actor_cell),
                body_counter=m.read32(n.actor+0x538),
                facing_u16=m.read32(facing_out), is_moving=bool(moving),
                native_world_z=m.invoke(0x5F5F30, n.actor),
                native_height=m.invoke(m.read32(m.read32(n.actor)+0x1C8), n.actor),
                native_z_adjust=struct.unpack('<i', dwords(m.invoke(0x4DAFC0, n.actor)))[0],
                adjust_for_z_multiplier_bits=bytes(u.mem_read(0xB0CD48,8)).hex(),
                cell_state=dict(level=struct.unpack('<b', u.mem_read(cell+0x11B,1))[0],
                                ramp=u.mem_read(cell+0x11C,1)[0],
                                flags=m.read32(cell+0x140), land=m.read32(cell+0xEC),
                                tile_index=signed(u,cell+0x38)),
                depth_coefficients={key:signed(u,n.mirage_type+off) for key,off in
                                    [('cliff',0xDC0),('column',0xDC4),
                                     ('tunnel',0xDC8),('bridge',0xDCC)]})


def draw_control(n, name, frame, owner=False, selected=False, brightness=1000, no_shadow=None):
    m,u=n.m,n.u;state=n.checkpoint()
    try:
        u.mem_write(0xA8ED84,dwords(frame));u.mem_write(0xA83D4C,dwords(n.house if owner else n.enemy))
        tree=m.read32(m.read32(n.rules+0xFFC));u.mem_write(n.actor+0x518,dwords(tree,0));u.mem_write(n.actor+0x1D8,b'\1');u.mem_write(n.actor+0x83,bytes([selected]))
        cell=n.resident.ptrs[n.actor_cell];convert=m.alloc(0x200)
        u.mem_write(cell+0x34,dwords(convert));u.mem_write(cell+0x10C,struct.pack('<h',brightness));u.mem_write(0x822CF1,b'\1')
        if no_shadow is not None:u.mem_write(n.mirage_type+0xD98,bytes([no_shadow]))
        draws=[]; calls=[];trace=NativeCallTrace(u,m.read32,calls)
        entries={0x4DED70:('foot_get_image',0,0),0x70EE30:('draw_type_gate',1,4),0x7465B0:('disguise_type',1,4),0x70ED80:('disguise_flags',1,4),0x705E00:('techno_draw',16,64)}
        def observe(uc,pc,size,data):
            sp=uc.reg_read(UC_X86_REG_ESP);trace.returned(pc,sp)
            if pc in entries:trace.entered(pc,sp,entries[pc])
            if pc==0x4AED70:
                args=list(struct.unpack('<14i',uc.mem_read(sp+4,56)))
                draws.append(dict(caller=hex(m.read32(sp)),frame=args[1],image=hex(args[0]),point=list(struct.unpack('<2i',uc.mem_read(args[2],8))),clip=list(struct.unpack('<4i',uc.mem_read(args[3],16))),flags=args[4],z_adjust=args[6],gradient=args[7],brightness=args[8],convert=hex(uc.reg_read(UC_X86_REG_EDX)),raw_args=args))
                m.ret(0,56)
        observed_inputs=draw_inputs(n)
        h=u.hook_add(UC_HOOK_CODE,observe);before=n.rng_state()
        try:
            result=m.invoke(0x73C5F0,n.actor,(100,120,0,0,800,600,0,0))
            trace.returned(RET_MAGIC,u.reg_read(UC_X86_REG_ESP))
        finally:u.hook_del(h)
        return dict(name=name,input=dict(frame=frame,creation_frame=signed(u,n.actor+0x1DC),owner=owner,selected=selected,point=[100,120],clip=[0,0,800,600],brightness=brightness,no_shadow=no_shadow,**observed_inputs),actual_type=dict(name='MGTK',facings=signed(u,n.mirage_type+0xE3C),no_shadow=u.mem_read(n.mirage_type+0xD98,1)[0]),selected_type=m.string(tree+0x24),selected_shp_header=bytes(u.mem_read(m.read32(tree+0xA4),8)).hex(),cell_convert=hex(convert),draws=draws,calls=calls,rng_before=before,rng_after=n.rng_state())
    finally:n.restore(state)

def reveal_controls(n):
    m,u=n.m,n.u;state=n.checkpoint();rows=[]
    # Independent controls restore memory/CPU/RNG, not a game save operation.
    for name,frame,offset,active,first in [
        ('east_enemy_frame3',3,(1,0),1,'enemy'),('east_enemy_frame8',8,(1,0),1,'enemy'),
        ('east_enemy_frame0',0,(1,0),1,'enemy'),('east_enemy_frame7',7,(1,0),1,'enemy'),
        ('inactive_global',3,(1,0),0,'enemy'),('first_allied_hides_enemy',3,(1,0),1,'allied_then_enemy'),
        ('enemy_unit_only',3,(1,0),1,'unit')]+[(f'neighbor_{i}',3,xy,1,'enemy') for i,xy in enumerate(struct.iter_unpack('<hh',u.mem_read(0x89F688,32)))]:
        n.restore(state);x,y=n.actor_cell;cell=n.resident.ptrs[x+offset[0],y+offset[1]]
        u.mem_write(0xA8E9A0,bytes([active]));u.mem_write(n.enemy_e1+0x30,dwords(0))
        chain=n.enemy_e1
        if first=='allied_then_enemy':
            u.mem_write(n.e1+0x30,dwords(n.enemy_e1));chain=n.e1
        if first=='unit':u.mem_write(n.candidate+0x30,dwords(0));chain=n.candidate
        u.mem_write(cell+0xE4,dwords(chain));u.mem_write(cell+0xE8,dwords(0))
        row=n.visit(name,frame);row['input']=dict(neighbor_offset=list(offset),game_active=active,chain=first,occupancy_boundary='Supplied category-linked Cell+E4 prior; original first-infantry traversal executes.')
        rows.append(row)
    for tile,start,layer in [(99,100,'ground'),(100,100,'bridge'),(115,100,'bridge'),(116,100,'ground'),(0,-1,'ground')]:
        n.restore(state);here=n.resident.ptrs[n.actor_cell];cell=n.resident.ptrs[n.actor_cell[0]+1,n.actor_cell[1]]
        u.mem_write(0xAA0E28,dwords(start));u.mem_write(here+0x38,dwords(tile))
        u.mem_write(cell+0xE4,dwords(n.enemy_e1 if layer=='ground' else 0));u.mem_write(cell+0xE8,dwords(n.enemy_e1 if layer=='bridge' else 0));u.mem_write(n.enemy_e1+0x30,dwords(0))
        row=n.visit(f'bridge_selector_{start}_{tile}',3);row['input']=dict(tile_index=tile,bridge_base=start,enemy_layer=layer,opposite_layer_empty=True,enemy_category='Infantry');rows.append(row)
    n.restore(state)
    # The complete supplied owner admits original placement. Hostility is a
    # declared prior for UpdateDisguise, not a proved ownership-transfer path.
    u.mem_write(n.enemy_e1+0x21C,dwords(n.house));u.mem_write(n.enemy_e1+0x14C,dwords(n.house))
    actual=n.relocate_infantry(n.enemy_e1,(n.actor_cell[0]+1,n.actor_cell[1]+1));assert actual['returned_al']
    u.mem_write(n.enemy_e1+0x21C,dwords(n.enemy));u.mem_write(n.enemy_e1+0x14C,dwords(n.enemy))
    history=[n.visit('actual_enemy_arrival',3),n.visit('skip_scan_frame8',8),n.visit('enemy_refresh',9)]
    u.mem_write(n.enemy_e1+0x21C,dwords(n.house));u.mem_write(n.enemy_e1+0x14C,dwords(n.house))
    removed=m.invoke(0x51DF10,n.enemy_e1)&255;assert removed
    history += [n.visit('expiry_minus1',28),n.visit('expiry_exact',29),n.visit('retained_after_expiry',30)]
    native_history=dict(placement=actual,remove_returned_al=removed,rows=history,
        boundary='Original Infantry ctor/Unlimbo/Limbo and Cell occupancy execute under complete supplied owner. Enemy House prior is supplied only between placement and removal; no ownership transfer or complete enemy-House startup claim.')
    n.restore(state)
    for name,start,duration,frame in [('zero',0,0,8),('future',10,0,8),('remaining',1,20,20),('exact',1,20,21),('late',1,20,22),('stopped_zero',-1,0,8),('stopped_positive',-1,20,8),('stopped_negative',-1,-3,8),('negative_duration',1,-3,8),('wrap_elapsed',0x7FFFFFFC,10,-2147483640)]:
        n.restore(state);m.invoke(0x746720,n.actor);u.mem_write(n.actor+0x1E0,dwords(start,0xA5A5A5A5,duration))
        row=n.visit('timer_'+name,frame);row['input']=dict(start=start,duration=duration);rows.append(row)
    n.restore(state)
    return dict(controls=rows,physical_history=native_history)


def ring_controls(n):
    m,u=n.m,n.u;state=n.checkpoint();rows=[]
    for name,mode,viewer,human,player,raw in [('owner',5,n.house,1,0,1),('enemy',5,n.enemy,1,0,1),('allied_viewer',5,n.enemy,1,0,1),('campaign_human',0,n.enemy,1,0,1),('campaign_player',0,n.enemy,0,1,1),('campaign_ai',0,n.house,0,0,1),('revealed_enemy',5,n.enemy,1,0,0)]:
        n.restore(state);u.mem_write(0xA8B238,dwords(mode));u.mem_write(0xA83D4C,dwords(viewer));u.mem_write(n.house+0x1EC,bytes([human,player]));u.mem_write(n.actor+0x2C8,dwords(n.ring));u.mem_write(n.ring+0x19D,b'\xA5');u.mem_write(n.actor+0x1D8,bytes([raw]))
        if not raw:u.mem_write(n.mirage_type+0xD32,b'\0')
        if name=='allied_viewer':u.mem_write(n.house+0x5788,dwords(1<<m.read32(n.enemy+0x30)))
        row=n.visit(name,8);row['input']=dict(mode=mode,viewer=hex(viewer),owner=hex(n.house),owner_is_current_house=viewer==n.house,owner_allied_with_viewer=viewer==n.house or name=='allied_viewer',human=human,player=player,raw_disguised=raw,attached_buffer_constructor_executed=False,hidden_before=165);row['hidden_after']=u.mem_read(n.ring+0x19D,1)[0];rows.append(row)
    n.restore(state);return rows


def damage_controls(n):
    m,u=n.m,n.u;state=n.checkpoint();rows=[]
    for name,damage,ignore,raw,can,perma in [('five_ignore',5,1,1,1,0),('five_defenses',5,0,1,1,0),('zero',0,0,1,1,0),('healing',-5,0,1,1,0),('already_revealed',7,1,0,1,0),('cannot_disguise',5,1,1,0,0),('permanent',5,1,1,1,1),('lethal_ignore',1000,1,1,1,0)]:
        n.restore(state);packet=m.alloc(4);u.mem_write(packet,dwords(damage));u.mem_write(n.actor+0x1D8,bytes([raw]));u.mem_write(n.mirage_type+0xD2F,bytes([can,perma]))
        if damage<0:u.mem_write(n.actor+0x6C,dwords(190))
        owner_type=m.read32(n.house+0x34)
        modifiers=dict(owner_type=m.string(owner_type+0x24),
                       house_type_armor_units_bits=m.read32(owner_type+0x10C),
                       armor_multiplier_bits=hex(struct.unpack('<Q',u.mem_read(n.actor+0x158,8))[0]),
                       veterancy_bits=m.read32(n.actor+0x150))
        row=n.visit(name,40,0x737C90,(packet,0,m.read32(n.weapon+0xAC),0,ignore,1,0));row['input']=dict(damage=damage,ignore_defenses=bool(ignore),raw_disguised=raw,can_disguise=can,perma_disguise=perma,warhead=m.string(m.read32(n.weapon+0xAC)+0x24),**modifiers);row['modified_damage']=signed(u,packet);rows.append(row)
    n.restore(state);return rows


def clear_controls(n):
    m,u=n.m,n.u;state=n.checkpoint();rows=[]
    for kind,pointer,table in [('unit',n.actor,0x7F5C70),('infantry',n.e1,0x7EB058),('building',None,0x7E3EBC),('aircraft',None,0x7E22A4)]:
        n.restore(state)
        if pointer is None:pointer=m.alloc(0x1000);u.mem_write(pointer,dwords(table))
        n.actor=pointer;u.mem_write(pointer+0x1D8,b'\1');u.mem_write(pointer+0x518,dwords(n.mirage_type,n.enemy));entry=m.read32(table+0x470)
        row=n.visit(kind,40,entry);row['input']=dict(category=kind,vtable=hex(table),full_constructor=kind in ('unit','infantry'));rows.append(row)
    for side in (0,1,2):
        n.restore(state);n.actor=n.e1;u.mem_write(n.e1_type+0xD30,b'\1');u.mem_write(n.house+0x1E8,dwords(side));u.mem_write(n.rules+0xD58,dwords(n.e1_type,n.typ,n.mirage_type))
        row=n.visit(f'infantry_perma_side{side}',40,0x522780);row['input']=dict(category='infantry',perma_disguise=1,owner_side=side,authored_default_disguise_names=['E1','MTNK','MGTK']);rows.append(row)
    n.restore(state);return rows

def observer_controls(n):
    m,u=n.m,n.u;state=n.checkpoint();rows=[]
    cases=[dict(name=f'owner_frame{frame}',frame=frame,creation=1,owner=True) for frame in (1,4,5,8,9,12,13,16,17,48,49,52,53,56,57,60,61,64,65,257,265)]
    cases += [dict(name='owner_signed_wrap',frame=-2147483639,creation=2147483640,owner=True),dict(name='owner_negative_remainder',frame=-100,creation=1,owner=True),dict(name='enemy_fresh',frame=1),dict(name='enemy_selected',frame=1,selected=True),dict(name='enemy_sensor',frame=1,sensor=1),dict(name='enemy_negative_sensor',frame=1,sensor=-1),dict(name='allied_viewer',frame=1,allied=True),dict(name='enemy_blink_active',frame=1,blink_start=1,blink_duration=10),dict(name='enemy_blink_exact',frame=11,blink_start=1,blink_duration=10),dict(name='undisguised',frame=1,raw=False)]
    for case in cases:
        n.restore(state);owner=case.get('owner',False);viewer=n.house if owner else n.enemy;frame=case['frame'];creation=case.get('creation',1);raw=case.get('raw',True);sensor=case.get('sensor',0)
        u.mem_write(0xA8ED84,dwords(frame));u.mem_write(0xA83D4C,dwords(viewer));u.mem_write(0xA8B238,dwords(5));u.mem_write(n.actor+0x1DC,dwords(creation));u.mem_write(n.actor+0x1D8,bytes([raw]));u.mem_write(n.actor+0x83,bytes([case.get('selected',False)]));u.mem_write(n.actor+0x1EC,dwords(case.get('blink_start',-1),0,case.get('blink_duration',0)))
        tree=m.read32(m.read32(n.rules+0xFFC));u.mem_write(n.actor+0x518,dwords(tree,0))
        if case.get('allied'):u.mem_write(n.house+0x5788,dwords(1<<m.read32(viewer+0x30)))
        cell=n.resident.ptrs[n.actor_cell];u.mem_write(cell+0xAC+m.read32(viewer+0x30)*2,struct.pack('<h',sensor))
        before=n.rng_state();calls=[];trace=NativeCallTrace(u,m.read32,calls)
        entries={0x70EE30:('draw_type_gate',1,4),0x746750:('disguised_to',1,4),0x7465B0:('disguise_type',1,4),0x7465F0:('disguise_house',1,4),0x4DED70:('foot_get_image',0,0),0x70ED80:('disguise_flags',1,4)}
        def observe(uc,pc,size,data):
            sp=uc.reg_read(UC_X86_REG_ESP);trace.returned(pc,sp)
            if pc in entries:trace.entered(pc,sp,entries[pc])
        h=u.hook_add(UC_HOOK_CODE,observe)
        try:
            outputs={}
            for key,entry,args in [('draw_actual',0x70EE30,(viewer,)),('disguised_to',0x746750,(viewer,)),('type_arg0',0x7465B0,(0,)),('type_arg1',0x7465B0,(1,)),('house_arg0',0x7465F0,(0,)),('house_arg1',0x7465F0,(1,)),('image',0x4DED70,()),('flags_arg256',0x70ED80,(256,))]:
                value=m.invoke(entry,n.actor,args);trace.returned(RET_MAGIC,u.reg_read(UC_X86_REG_ESP))
                outputs[key]=value&255 if key in ('draw_actual','disguised_to') else value
        finally:u.hook_del(h)
        # Original DrawIt selector; stop before either render entry.
        for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,n.actor)):
            u.reg_write(reg,value)
        run_checked(u,0x73D2CA,0x73D317)
        route_type=u.reg_read(UC_X86_REG_EDI)
        outputs['draw_route_type']=route_type
        outputs['draw_route_voxel']=u.mem_read(route_type+0x236,1)[0]
        rows.append(dict(name=case['name'],input=dict(frame=frame,creation=creation,owner=owner,owner_allied=owner or case.get('allied',False),raw_disguised=raw,selected=case.get('selected',False),sensor_count=sensor,game_mode=5,cell_present=True,blink_start=case.get('blink_start',-1),blink_duration=case.get('blink_duration',0)),outputs=outputs,actual_type=hex(n.mirage_type),disguise_type=hex(tree),actual_house=hex(n.house),disguise_house='0x0',actual_image=hex(m.read32(n.mirage_type+0xA4)),disguise_image=hex(m.read32(tree+0xA4)),calls=calls,rng_before=before,rng_after=n.rng_state()))
    n.restore(state);return rows


class RawLoadAdapter:
    """Borrow the existing Unit raw-load owner in this already constructed VM."""
    block = NavalNative.block

    def __init__(self, native):
        self.native = native
        self.u = native.u
        self.actor = native.actor
        self.ret = native.m.ret
        self.invoke = native.m.invoke
        self.read32 = native.m.read32

    def alloc(self, size):
        pointer = self.native.m.alloc(size)
        if size == 16:
            self.native.stream_callbacks.add(pointer)
        return pointer


def load_controls(n):
    m, u = n.m, n.u
    state = n.checkpoint()
    rows = []
    for name, revealed in [('retained_disguise', False), ('revealed_timer', True)]:
        n.restore(state)
        if revealed:
            packet = m.alloc(4)
            u.mem_write(packet, dwords(5))
            produced = n.visit('damage_before_raw_save', 40, 0x737C90,
                               (packet, 0, m.read32(n.weapon+0xAC), 0, 1, 1, 0))
        else:
            produced = None
        before, rng_before = n.disguise_state(), n.rng_state()
        source_actor = n.actor
        source_loco = m.read32(source_actor+0x674)
        size = m.invoke(m.read32(m.read32(source_actor)+0x30), source_actor)
        assert size == 0x8E8
        raw = bytes(u.mem_read(source_actor, size))
        n.stream_callbacks = set()
        adapter = RawLoadAdapter(n)
        try:
            load = raw_load_sound_reset(adapter, raw)
        finally:
            n.stream_callbacks.clear()
        n.actor = adapter.actor
        after_load = n.disguise_state()
        rng_after_load = n.rng_state()
        # COM locomotor loading/swizzle is outside the reused raw-load fixture.
        # Reattach the already constructed original Drive interface explicitly.
        assert m.read32(n.actor+0x674) == 0
        u.mem_write(n.actor+0x674, dwords(source_loco))
        u.mem_write(source_loco+8, dwords(n.actor))
        continuation = ([n.visit('loaded_expiry_minus1', 49),
                         n.visit('loaded_expiry_exact', 50),
                         n.visit('loaded_retained_after_expiry', 51)] if revealed
                        else [n.visit('loaded_retained', 8)])
        rows.append(dict(name=name, produced=produced, before=before,
                         raw_size=size, raw_sha256=hashlib.sha256(raw).hexdigest(),
                         load=load, after_load=after_load, rng_before=rng_before,
                         rng_after_load=rng_after_load, continuation=continuation,
                         boundary='Original raw AbstractLoad and existing FootLoad/no-init Unit reconstruction. Raw object bytes, external pointer identity and reattachment of the existing Drive interface are supplied; full dynamic/COM/swizzle/scenario Save/Load is excluded.'))
    n.restore(state)
    return rows


def active_registry(n):
    m, u = n.m, n.u
    result = {}
    for name, base_address in [('unit', 0x8B4108), ('techno', 0xA8EC78),
                               ('logic', 0x87F778)]:
        data, count = m.read32(base_address+4), m.read32(base_address+16)
        assert count < 1000
        pointers = [m.read32(data+i*4) for i in range(count)]
        result[name] = dict(count=count, actor_present=n.actor in pointers)
    return result


def cleanup_controls(n):
    state = n.checkpoint()
    try:
        before = active_registry(n)
        uninit = n.visit('original_uninit', 41, 0x4DE5D0)
        after_uninit = active_registry(n)
        destructor = n.visit('original_deferred_destructor', 41, 0x725C70)
        return dict(before=before, uninit=uninit, after_uninit=after_uninit,
                    destructor=destructor, after_destructor=active_registry(n))
    finally:
        n.restore(state)


def acquisition_controls(n):
    m, u = n.m, n.u
    state = n.checkpoint()
    rows = []
    for name, raw, still, contact_slot, moving in [
            ('new_stationary', False, True, None, False),
            ('new_still_disabled', False, False, None, False),
            ('retained_still_disabled', True, False, None, False),
            ('new_radio_slot0', False, True, 0, False),
            ('new_radio_slot1', False, True, 1, False),
            ('retained_radio_slot0', True, True, 0, False),
            ('new_drive_reports_moving', False, True, None, True),
            ('retained_drive_reports_moving', True, True, None, True)]:
        n.restore(state)
        if not raw:
            m.invoke(0x746720, n.actor)
        u.mem_write(n.mirage_type+0xD32, bytes([still]))
        data = m.alloc(8)
        u.mem_write(n.actor+0xE4, dwords(data, 2))
        u.mem_write(data, dwords(n.e1 if contact_slot == 0 else 0,
                                n.e1 if contact_slot == 1 else 0))
        loco = m.read32(n.actor+0x674)
        if moving:
            # Supplied Drive destination prior; original IsMoving executes.
            # This does not execute Process or change the held movement owner.
            u.mem_write(loco+0x30, dwords(1, 0, 0))
        row = n.visit(name, 8)
        row['input'] = dict(raw_disguised=raw, disguise_when_still=still,
                            radio_contact_slot=contact_slot, drive_destination_prior=moving)
        rows.append(row)
    for name, can, perma in [('caller_cannot_disguise', False, False),
                              ('caller_permanent', True, True)]:
        n.restore(state)
        m.invoke(0x746720, n.actor)
        u.mem_write(n.mirage_type+0xD2F, bytes([can, perma]))
        row = n.visit(name, 8, 0x7360C0)
        row['input'] = dict(can_disguise=can, perma_disguise=perma)
        rows.append(row)
    n.restore(state)
    return rows


def unit_art_controls(n):
    """Original retained Unit ART reads and postpass, with declared field priors."""
    m, u = n.m, n.u
    checkpoint = n.checkpoint()
    rows = []
    specifications = [
        ('default_no_turret', 8, False, {}),
        ('default_turret', 8, True, {}),
        ('retained_turret', 5, True, {}),
        ('default_firing_frames', 8, False, {'FiringFrames': '1'}),
        ('firing_frames_low_byte_zero', 8, False, {'FiringFrames': '256'}),
        ('firing_frames_low_byte_one', 8, False, {'FiringFrames': '257'}),
        ('firing_frames_negative', 8, False, {'FiringFrames': '-1'}),
        ('authored_facings', 8, False, {'Facings': '16'}),
        ('zero_facings', 8, False, {'Facings': '0'}),
        ('negative_facings', 8, False, {'Facings': '-2'}),
        ('large_facings', 8, False, {'Facings': '40'}),
        ('empty_facings', 8, False, {'Facings': ''}),
        ('malformed_facings', 8, False, {'Facings': 'bad'}),
        ('wrong_case_facings', 8, False, {'facings': '16'}),
        ('authored_frame_starts', 8, False,
            {'WalkFrames': '4', 'FiringFrames': '3', 'StandingFrames': '2',
             'DeathFrames': '5', 'DeathFrameRate': '0', 'Facings': '8'}),
        ('explicit_frame_starts', 8, False,
            {'WalkFrames': '4', 'FiringFrames': '3', 'StandingFrames': '2',
             'DeathFrames': '5', 'Facings': '8', 'StartStandFrame': '91',
             'StartWalkFrame': '7', 'StartFiringFrame': '101',
             'StartDeathFrame': '111', 'MaxDeathCounter': '117'}),
        ('retained_offsets_after_firing_change', None, False,
            {'WalkFrames': '4', 'FiringFrames': '3', 'Facings': '8'}),
    ]
    try:
        for name, facings, turret, fields in specifications:
            n.restore(checkpoint)
            if facings is not None:
                for offset, raw in n.unit_art_constructor_bytes.items():
                    u.mem_write(n.mirage_type+offset, raw)
                u.mem_write(n.mirage_type+0xE3C, dwords(facings))
            u.mem_write(n.mirage_type+0xCA1, bytes([turret]))
            m.make_ini({'RTNK': fields})
            before = unit_art_state(m, n.mirage_type)
            calls = []
            trace = NativeCallTrace(u, m.read32, calls)

            def observe(uc, pc, size, data):
                sp = uc.reg_read(UC_X86_REG_ESP)
                trace.returned(pc, sp)
                if pc == 0x5276D0:
                    index = trace.entered(pc, sp, ('art_integer', 3, 12))
                    calls[index].update(section=m.string(m.read32(sp+4)),
                        key=m.string(m.read32(sp+8)), default=signed(uc, sp+12))

            hook = u.hook_add(UC_HOOK_CODE, observe)
            try:
                for begin,end in ((0x7477D8, 0x747820), (0x7478B6, 0x747AAE)):
                    for register,value in ((UC_X86_REG_ESP, SP),
                            (UC_X86_REG_EDI, n.mirage_type),
                            (UC_X86_REG_ESI, n.mirage_type+0x1F8)):
                        u.reg_write(register, value)
                    run_checked(u, begin, end, required_addresses=[0x5276D0])
                    trace.returned(end, u.reg_read(UC_X86_REG_ESP))
                    assert u.reg_read(UC_X86_REG_ESP) == SP
            finally:
                u.hook_del(hook)
            rows.append(dict(name=name,
                input=dict(prior=before, sections={'RTNK': fields}),
                before=before, after=unit_art_state(m, n.mirage_type), calls=calls))
    finally:
        n.restore(checkpoint)
    return rows


def generate():
    retail = reader_rows()
    native = Mirage()
    native.initialize_mirage()
    # Read the inherited registration owner at the actual first-AI boundary.
    # Limbo does not erase constructor registration in the category arrays.
    registration = native.registration()
    roles = dict(source=native.src, e1=native.e1, victim=native.victim,
                 candidate=native.candidate, mirage=native.actor, enemy_e1=native.enemy_e1)
    actors = {}
    for role,pointer in roles.items():
        typ = native.m.read32(pointer+(0x6C0 if native.m.read32(pointer)==0x7EB058 else 0x6C4))
        actors[role] = dict(pointer=hex(pointer), type_pointer=hex(typ),
            type_name=native.m.string(typ+0x24), house=hex(native.m.read32(pointer+0x21C)),
            health=signed(native.u,pointer+0x6C), xyz=base.xyz(native.u,pointer+0x9C),
            alive=native.u.mem_read(pointer+0x90,1)[0], limbo=native.u.mem_read(pointer+0x81,1)[0],
            display_layer=native.m.read32(pointer+0x94),
            logic_registered=native.u.mem_read(pointer+0x98,1)[0],
            foot=native.navigation_snap(pointer,0))
    current_cell = native.resident.ptrs[native.actor_cell]
    mirage_cell = dict(**native.cell_readback(current_cell),
                       tile_index=signed(native.u,current_cell+0x38))
    rows = [native.visit('first_whole_unit_ai', 1, 0x7360C0),
            native.visit('retained_whole_unit_ai', 2, 0x7360C0),
            native.visit('retained_frame8', 8)]
    result = dict(schema_version=1, native_sha256=image_sha256(), retail=retail,
                initialization=dict(type_constructor=native.type_constructor,
                    unit_art_constructor=native.unit_art_constructor,
                    art_input=native.art_input,
                    actor_constructor=native.actor_constructor, layers=native.mirage_layers,
                    actor_roles={role:hex(pointer) for role,pointer in roles.items()},
                    actors=actors, registration=registration,
                    candidate_limbo_returned_al=native.enemy_limbo,
                    mirage_cell=mirage_cell,
                    constructor_prior=dict(frame=1,poison_byte=165,
                        poison_ranges=[['0x1d8',1],['0x1dc',16],['0x518',8]]),
                    projection=native.projection,
                    inherited_inputs=native.inputs,
                    rng_pointers={key:hex(pointer) for key,pointer in native.resident.rngs.items()},
                    unlimbo=native.mirage_unlimbo, cell=list(native.actor_cell),
                    tree_images=native.tree_images, image_io=native.file_platform.file_io,
                    neighbor_offsets=[list(pair) for pair in
                                      struct.iter_unpack('<hh', native.u.mem_read(0x89F688, 32))]),
                histories=rows, acquisition=acquisition_controls(native), load=load_controls(native),
                cleanup=cleanup_controls(native), adjacency=reveal_controls(native), damage=damage_controls(native),
                clear=clear_controls(native), ring=ring_controls(native), observer=observer_controls(native),
                radar=[radar_control(native, 'enemy_null_house', native.enemy),
                       radar_control(native, 'owner', native.house),
                       radar_control(native, 'allied_viewer', native.enemy, allied=True),
                       radar_control(native, 'enemy_undisguised', native.enemy, disguised=False)],
                drawing=[draw_control(native,'enemy_frame10',10),
                         draw_control(native,'enemy_selected_frame10',10,selected=True),
                         draw_control(native,'enemy_actual_no_shadow',10,no_shadow=True),
                         draw_control(native,'owner_phase10',10,owner=True),
                         draw_control(native,'owner_phase17',17,owner=True),
                         draw_control(native,'enemy_cell_brightness750',10,brightness=750)],
                unit_art_controls=unit_art_controls(native))
    assert hashlib.sha256(bytes(native.u.mem_read(0x401000,0x3E0000))).hexdigest() == native.resident.code_hash
    assert all(bytes(native.u.mem_read(int(address,16),len(bytes.fromhex(raw)))).hex() == raw
               for address,raw in native.vtables.items())
    result['native_text_sha256'] = native.resident.code_hash
    result['native_text_and_vtables_unchanged'] = True
    return result


def metadata():
    result = provenance(scope=__doc__, assumptions=[
        'Existing FootMissions physical Anytown crop, supplied House/startup setup and native constructors/placement; not full scenario startup.',
        'Original Rules665650 and TechnoType710AF0 constructors establish defaults; exact bool714404..71446C and General671D3E..671D92 reader blocks retain native parser/list allocation behavior across physical RULESMD, optional LANGRULE, MPBattleMD and XMP03T4 layers. Authored controls remain explicit. Full UnitType747620 plus actual referenced weapon/projectile/warhead and Terrain71DEA0 readers initialize the measured actor. ARTMD is fixed, with physical MGTK Image=RTNK.',
        'Unit ART controls reuse the same VM and execute7477D8..747820 plus7478B6..747AAE, including original signed-byte storage, retained-default reads and sentinel frame-start postpass. Constructor field bytes or recorded retail fields plus declared authored priors are inputs; no frame arithmetic or parser result is supplied. Actual initial actor roles/registries/current Cell come from existing FootMissions readback owners before the first whole UnitAI.',
        'Full poisoned Unit7353C0 construction, actual737BA0 placement and complete first/repeated UnitAI execute. Only declared direct Update controls supply radio, Drive destination, timer, linked-cell or caller-flag priors. Movement Process/pathfinding is not ported or certified by these controls.',
        'The eight initialized neighbor offsets, original first-Infantry traversal, bridge selector and signed timer/RNG execute. Physical adjacency history uses original Infantry ctor/Unlimbo/Limbo under the complete supplied owner, with hostile House pointers supplied between placement and removal; not native ownership transfer or full hostile-House startup.',
        'Complete original Unit/Foot/Techno/Object damage executes with physical AP warhead and observed armor/veterancy inputs, including admitted lethal result4. Selected damage/disguise observations do not certify every downstream death effect.',
        'Raw reveal middle word+1E4 is an unnamed observation, not a cell coordinate or deterministic state contract. Constructor leaves its poisoned bytes unchanged. Body controls record actual reads/writes; whole raw load transports the opaque word without establishing a behavioral meaning.',
        'Category clear controls execute original Unit746720, Infantry522780 and base41C030. Building/Aircraft receivers are supplied buffers with actual vtables; authored permanent Infantry side defaults are controls, not a full Spy lifecycle.',
        'Attached-ring controls provide an unconstructed Anim-sized buffer and execute only the reached hidden-byte tail. Ownership/capture/Anim lifecycle beyond that tail is excluded.',
        'Observer controls execute original disguise getters/draw gate/GetImage/flag helper and admitted DrawIt selector. Direct flags_arg256 includes cases outside the actual caller gate and must not be treated as already-admitted draw flags.',
        'Original Terrain InitTheater71DCA0 forms filenames and reads complete physical TREE01..04.TEM through the canonical RawFile transport. Original projection initializers6D1830/6D18C0/6D1BB0 produce the nonzero startup scale; drawing records original UnitSHP and TechnoDraw arguments against these loaded images, not TerrainClass drawing.',
        'Radar starts at admitted655F48 with inactive flash, actual disguise getter/LightGrey lookup and original BSurface pixel write into a supplied4x4 RGB565 surface. Original ColorScheme constructor slice68C769..68C7DC receives ordinary index53 and produces+330. This is the final stored pixel for the admitted branch, not full radar visibility or renderer scheduling.',
        'Raw-load controls reuse naval_lifetime_controls.raw_load_sound_reset: original AbstractLoad410380, FootLoad sound-reset suffix, Foot no-init constructor and Unit vtable reconstruction. Saved raw bytes and retained external identity are supplied. The existing original Drive interface is explicitly reattached for subsequent Update; full COM/vector/swizzle/scenario Save/Load is excluded.',
        'Full Foot UnInit and deferred Unit/Foot/Techno destructors execute with no attached Anim, target, bomb or planning group. Original633900 initializes the previously unused global token vector. Registry observations qualify removal; raw freed-object bytes are not live state.',
        'Independent controls restore mapped bytes/CPU/allocator to their prior experiment checkpoint; this is harness isolation, not a simulated native save operation. Original text and inherited vtables are checked unchanged after all controls.',
    ], substitutions=[
        'Inherited allocation, Windows COM transport, visual asset setup and radar tracker boundaries are retained explicitly from FootMissions/Mission.',
        'Prepared physical INI section caches and extracted theater archive winners replace file/archive startup. Native selected readers, tree filename selection, CCFile/RawFile reads and SHP allocation execute. Actual file lengths/hashes and IO are retained.',
        'CC_Draw_Shape4AED70 records its14 original arguments, pointed position/clip, Convert and surface, then returns with56-byte callee cleanup. Current Cell Convert identity and light are supplied inputs; no shape pixels or palette-constructor parity claim.',
        'Existing raw-load helper supplies exact IStreamRead bytes. No gameplay, timer, RNG, target, disguise or projection arithmetic result is supplied.',
    ], entry_points=dict(update_disguise=0x7468C0, unit_ai=0x7360C0,
                        clear_disguise=0x746720, first_infantry=0x47EC40,
                        type_constructor=0x710AF0, unit_type_reader=0x747620,
                        general_reader=0x671D3E, unit_constructor=0x7353C0,
                        unit_damage=0x737C90, draw_gate=0x70EE30,
                        unit_shp_draw=0x73C5F0, techno_draw=0x705E00,
                        terrain_init_theater=0x71DCA0, projection_init=0x6D1BB0,
                        radar_pixel_suffix=0x655F48, raw_load=0x410380,
                        foot_uninit=0x4DE5D0, retirement_drain=0x725C70))
    result['command'] = 'python -m tools.spatial_oracle.mirage_disguise --check'
    return result


if __name__ == '__main__':
    guarded = source_paths()
    guarded[str(Path(__file__).resolve().relative_to(HERE.parents[1]))] = Path(__file__)
    root = HERE.parents[1]
    # The existing stream transport imports its established construction/audio
    # owners. Guard the actual loaded closure without rewriting their old seals.
    for module in tuple(sys.modules.values()):
        if (name := getattr(module, '__file__', None)):
            path = Path(name).resolve()
            if path.is_relative_to(root/'tools') and path.suffix == '.py':
                guarded[str(path.relative_to(root))] = path
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                   source_paths=guarded)
