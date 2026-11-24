"""Original retail MTNK inputs and the Foot threat coefficient lifecycle.

Companion to astar_structural_height's --mtnk-inputs mode. The existing Inputs
owner reads physical Hills rules layers and constructs MTNK's type; original
Unit/Foot constructor prefix, Drive construction and the admitted Unlimbo coefficient
copy execute. This does not execute whole ReadINI, Unlimbo, Scenario loading,
placement, high-bridge Cell production, A* search or path finishing.
"""
from types import SimpleNamespace
import hashlib
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)
from tools.native_oracle import RET_MAGIC, provenance, run_checked
from tools.projectile_oracle.bridge_render_inputs import assets_root, lexical
from tools.spatial_oracle.anytown_damage import navigation_inputs, next_family_native
from tools.spatial_oracle.building_body_rules import SP, dwords
from tools.spatial_oracle.mapgen_range import Machine


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def type_state(m):
    p = m.mtnk
    signed = lambda off: struct.unpack('<i', m.u.mem_read(p + off, 4))[0]
    return dict(name=m.string(p + 0x24), strength=signed(0xA0),
                speed_type=signed(0x67C), movement_zone=signed(0x5B4),
                crusher=m.u.mem_read(p + 0xD28, 1)[0],
                threat_coefficient_f64_bits=bytes(m.u.mem_read(p + 0x2F0, 8)).hex())


class RecordedInputs(navigation_inputs.Inputs):
    """Observe the shared reader owner without reproducing its reads."""

    def __init__(self, theater, map_file):
        self.mtnk_reads = []
        self.mtnk_ctor = None
        self.original_text = None
        super().__init__(theater, map_file=map_file)

    def invoke(self, fn, this, args=()):
        if self.original_text is None:
            self.original_text = sha(bytes(self.u.mem_read(0x401000, 0x3E0000)))
        result = super().invoke(fn, this, args)
        if fn == 0x7470D0 and this == getattr(self, 'mtnk', None):
            self.mtnk_ctor = type_state(self)
        return result

    def block(self, begin, end, regs):
        spans = {
            0x5F94B3: ('strength_armor_immune', 'EBX'),
            0x7121D1: ('speed_type', 'EBP'),
            0x712270: ('is_train', 'EBP'),
            0x7122BE: ('explodes', 'EBP'),
            0x712452: ('threat_coefficient', 'EBP'),
            0x714CC8: ('crusher', 'EBP'),
            0x71605E: ('movement_zone', 'EBP'),
            0x7476D3: ('unit_speed_postpass', 'EDI'),
        }
        # Read only the register carrying the actual type receiver. Terrain's
        # shared ObjectType Strength block must not be mistaken for MTNK.
        from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_EDI
        registers = {'EBX': UC_X86_REG_EBX, 'EBP': UC_X86_REG_EBP, 'EDI': UC_X86_REG_EDI}
        selected = spans.get(begin)
        receiver = getattr(self, 'mtnk', None)
        if selected and receiver is not None and regs.get(registers[selected[1]]) == receiver:
            event = dict(reader=selected[0], begin=f'{begin:08x}', end=f'{end:08x}',
                         layer_index=len(self.receipts), before=type_state(self))
            super().block(begin, end, regs)
            event['after'] = type_state(self)
            self.mtnk_reads.append(event)
        else:
            super().block(begin, end, regs)


def generate():
    map_file = assets_root() / 'Hills.map'
    theater = next_family_native.theater()
    m = RecordedInputs(theater, map_file)
    u = m.u
    startup = Machine.startup(SimpleNamespace(u=u))
    final_type = type_state(m)
    assert m.mtnk_ctor is not None
    # The reader receipts preserve source order and retained state when the
    # selected section is absent. The native parent ReadINI admission itself is
    # outside this slice fixture; section presence supplies that boundary.
    sources = [navigation_inputs.ri.ASSETS / name for name in
               ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini')] + [map_file]
    layers = []
    retained = m.mtnk_ctor
    for i, path in enumerate(sources):
        reads = [r for r in m.mtnk_reads if r['layer_index'] == i]
        row = dict(file=path.name, before=retained, reads=reads)
        if path.exists():
            raw = path.read_bytes()
            sections, lines = lexical(raw, {'MTNK'})
            keys = {'SpeedType', 'MovementZone', 'ThreatAvoidanceCoefficient', 'Crusher'}
            row.update(sha256=sha(raw), bytes=len(raw),
                       section_present='MTNK' in sections,
                       physical_keys={k: v for k, v in sections.get('MTNK', {}).items() if k in keys},
                       source_lines=[line for line in lines if line['key'] in keys])
        else:
            assert path.name == 'LANGRULE.INI'
            row['absent'] = True
        retained = reads[-1]['after'] if reads else retained
        row['after'] = retained
        layers.append(row)
    assert retained == final_type

    scenario = m.read32(0xA8B230)
    rngs = {'main': 0x886B88, 'scenario': scenario + 0x218, 'mapgen': 0xABE890}
    for p in rngs.values():
        m.invoke(0x65C6D0, p, (0,))
    rng_state = lambda: {name: bytes(u.mem_read(p, 1012)).hex() for name, p in rngs.items()}
    actor, drive = m.alloc(0x800), m.alloc(0x100)
    # Original Unit constructor publishes into this supplied spare-capacity
    # Techno vector. Allocation/list capacity is not native Scenario loading.
    u.mem_write(0xB0F720, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    events, writes = [], []
    phase = 'unit_constructor'

    def observe(_u, pc, _size, _data):
        if pc in (0x7353C0, 0x4D31E0, 0x4AF540, 0x55A6C0, 0x6F3270,
                  0x65C780, 0x65C7E0, 0x6F3250):
            event = dict(phase=phase, pc=f'{pc:08x}', this=f'{u.reg_read(UC_X86_REG_ECX):08x}')
            if pc in (0x65C780, 0x65C7E0):
                this = u.reg_read(UC_X86_REG_ECX)
                event['stream'] = next((name for name, p in rngs.items() if p == this), 'other')
            events.append(event)
        if pc in (0x73F0A0, 0x42B210, 0x42B7F0):
            raise AssertionError(('input witness unexpectedly entered gameplay consumer', hex(pc)))

    def written(_u, _access, address, size, value, _data):
        assert address + size <= 0x401000 or address >= 0x7E1000, ('native text write', hex(address))
        if actor + 0x530 <= address < actor + 0x538:
            writes.append(dict(phase=phase, pc=f'{u.reg_read(UC_X86_REG_EIP):08x}',
                               offset=address - actor, size=size, value=value))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, written)
    before_ctor = rng_state()
    u.mem_write(SP, dwords(RET_MAGIC, m.mtnk, 0))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, actor)
    run_checked(u, 0x7353C0, 0x7354CE, count=300000,
                required_addresses=(0x4D31E0, 0x4D3217, 0x4D321D))
    ctor = dict(flags=m.read32(actor + 0x14), type_pointer_matches=m.read32(actor + 0x6C4) == m.mtnk,
                team_pointer=m.read32(actor + 0x5D4),
                coefficient_f64_bits=bytes(u.mem_read(actor + 0x530, 8)).hex(),
                rng_before=before_ctor, rng_after=rng_state())
    assert ctor['coefficient_f64_bits'] == '0000000000000000' and ctor['team_pointer'] == 0
    assert ctor['flags'] == 7 and ctor['type_pointer_matches']

    phase = 'drive_constructor'
    before_drive = rng_state()
    m.invoke(0x4AF540, drive)
    u.mem_write(drive + 0xC, dwords(actor))
    u.mem_write(drive + 0x14, dwords(1))
    u.mem_write(actor + 0x674, dwords(drive + 4))
    drive_result = dict(ilocomotion_vtable=f'{m.read32(drive + 4):08x}',
                        rng_before=before_drive, rng_after=rng_state())
    assert drive_result['ilocomotion_vtable'] == '007e7eb0'
    assert before_drive == rng_state()

    phase = 'unlimbo_coefficient_copy'
    before_copy = rng_state()
    # Interior admitted Unlimbo frame: ESI=Foot; original virtual type getter,
    # FLD Type+2F0 and FSTP Foot+530 execute through the native shared owner.
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, actor)
    run_checked(u, 0x4D72E0, 0x4D72FA, count=1000,
                required_addresses=(0x6F3270, 0x4D72EA, 0x4D72F4))
    copied = bytes(u.mem_read(actor + 0x530, 8)).hex()
    assert copied == final_type['threat_coefficient_f64_bits']
    assert before_copy == rng_state()
    assert m.original_text == sha(bytes(u.mem_read(0x401000, 0x3E0000)))
    return dict(schema_version=1, scope=__doc__,
                startup=startup, theater_sha256=theater['sha256'],
                type_constructor=m.mtnk_ctor, layers=layers, final_type=final_type,
                unit_constructor=ctor, drive_constructor=drive_result,
                unlimbo_copy=dict(coefficient_f64_bits=copied, rng_before=before_copy, rng_after=rng_state()),
                unit_vtable=f'{m.read32(actor):08x}',
                can_enter_slot=f'{m.read32(m.read32(actor) + 0x1AC):08x}',
                type_getter_slot=f'{m.read32(m.read32(actor) + 0x84):08x}',
                events=events, coefficient_writes=writes,
                text_sha256=m.original_text, code_unchanged=True)


def metadata():
    result = provenance(scope=__doc__, entry_points={
        'unit_type_ctor': 0x7470D0, 'speed_reader': 0x7121D1,
        'coefficient_reader': 0x712452, 'crusher_reader': 0x714CC8,
        'movement_zone_reader': 0x71605E, 'unit_speed_postpass': 0x7476D3,
        'unit_ctor_prefix': 0x7353C0, 'unit_ctor_stop': 0x7354CE,
        'drive_ctor': 0x4AF540, 'unlimbo_copy_begin': 0x4D72E0,
        'unlimbo_copy_stop': 0x4D72FA, 'type_getter': 0x6F3270},
        assumptions=[
            'Uses the existing navigation_inputs.Inputs owner on physical Hills.map and retained retail RULESMD/LANGRULE/MPBattleMD lexical caches. Host section presence supplies reached type-reader bodies; full INI parsing, discovery and parent ReadINI admission are excluded.',
            'Inputs also executes its existing Unit Speed postpass for layers without MTNK. The receipt records those calls and preserved state explicitly; it does not claim full UnitType ReadINI reachability on an absent section.',
            'Original MTNK type constructor, selected scalar readers, complete Unit/Foot constructor prefix and Drive constructor execute. Supplied zero backing storage, owner=NULL, native registry capacity and three original seed0 RNGs are fixture boundaries.',
            'Original CRT precision and WinMain rounding tail execute through mapgen_range. The admitted Unlimbo copy executes only its original type getter and coefficient load/store; preceding placement and later Unlimbo effects are excluded.',
            'No high-bridge flags, loaded Cells, actor placement, ordinary Move, A* or path finishing is established by this prerequisite witness.'],
        substitutions=[
            'Inherited successful bounded malloc/free and CRT TLS seams; physical archive/file loading and INI cache preparation are host supplied. No gameplay, scalar-reader, coefficient, RNG or constructor return is supplied.'])
    return result
