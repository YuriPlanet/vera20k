"""Original shot-down aircraft crash: Crash, the Fly fall, the impact and the crash smoke.

Run: python -m tools.spatial_oracle.aircraft_crash [--check | --write]

Four families, each on real Aircraft/Fly/Facing vtables and constructors:

- ``crash``: the whole shared Foot Crash ``0x004DEBB0`` on a dead or living
  airborne aircraft: the height refusal, the live-object prefix, the latch, the
  three Scenario spin draws and their float rates, and the calls it makes.
- ``fall``: FlyLocomotionClass ``ILoco_Process 0x004CCB40`` once per frame for
  a Health-0 crashing aircraft, from the kill until the impact, with every
  intermediate coordinate, the fall counter and the impact's calls.
- ``rocking``: TechnoClass ``RockingUpdate 0x0070B570``'s crashing branch
  accumulating the spin rates Crash wrote.
- ``smoke``: the red-health smoke block of ``AircraftClass::AI``
  (``0x00415085..0x0041512C``), its Scenario draw and threshold.

Substitutions are declared per family and recorded in each row's ``calls``.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP,
    UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    NATIVE_FPCW, RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, OracleError, finish_vectors,
    load_image, provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords, packed

# ---- native identities ------------------------------------------------------------------------
AIRCRAFT_VT, AIRCRAFT_TYPE_VT, FLY_CONTROL_VT = 0x7E22A4, 0x7E2868, 0x7E2250
MAP, FRAME, RULES_PTR, SCENARIO_PTR = 0x87F7E8, 0xA8ED84, 0x8871E0, 0xA8B230
I_KNOW_WHAT_IM_DOING = 0xA8E7AC
TABLE_PTR = 0x87F924                                # MapClass +0x13C: cell table, then its length
NULL_COORD_INIT, FLY_CONSTRUCTOR = 0x4CC940, 0x4CC9A0
FACING_CONSTRUCTOR, FACING_SET_ROT, FACING_SET = 0x4C91C0, 0x4C9680, 0x4C9300
RNG_SEED, RANDOM, RANDOM_RANGED = 0x65C6D0, 0x65C780, 0x65C7E0
CRASH, ILOCO_PROCESS, ROCKING_UPDATE = 0x4DEBB0, 0x4CCB40, 0x70B570
SMOKE_BEGIN, SMOKE_END = 0x415085, 0x41512C
TYPE_SPEED_CONVERSION = (0x71465F, 0x71469F)
# Vtable slots the families substitute (Aircraft vtable offsets).
SLOT_MARK, SLOT_UNINIT, SLOT_RECORD_KILL = 0x124, 0xF8, 0xE0
SLOT_TRANSMIT_FIRST, SLOT_STUN = 0x274, 0x3A0

# ---- scratch layout (runner SCRATCH is 0x20000000, 0x10000 bytes; more is mapped below) ------
REGION, REGION_SIZE = SCRATCH, 0x40000
OWNER = REGION + 0x1000                 # AircraftClass
TYPE = REGION + 0x3000                  # AircraftTypeClass
HOUSE = REGION + 0x5000
RULES = REGION + 0x6000                 # RulesClass (0x2000 bytes)
SCENARIO = REGION + 0x9000              # ScenarioClass; its RNG at +0x218
RNG = SCENARIO + 0x218
LOCO = REGION + 0xA000                  # FlyLocomotionClass (interface at +4)
VTABLE = REGION + 0xB000                # clone of the Aircraft vtable, 0x600 bytes
STUBS = REGION + 0xC000                 # INT3 stubs, 0x10 apart
ARG = REGION + 0xD000
CELLS, CELLS_SIZE = 0x21000000, 0x80000  # SIDE x SIDE CellClass records, 0x200 apart
SIDE, CELL_SIZE, ORIGIN = 24, 0x200, (44, 44)   # cells (44..67, 44..67)
TABLE = 0xC00000                        # MapClass cell pointer table, 0x40000 entries
SP = STACK_BASE + STACK_SIZE - 0x1000
MAP_SIZE = (64, 64)                     # MapClass +0xF4/+0xF8 for Cell_in_bounds_check
START = 52 * 256 + 128                  # the aircraft's X and Y at the kill (cell 52, 52)

STUB_POPS = {'mark': 4, 'uninit': 0, 'record_kill': 4, 'transmit_first': 4, 'stun': 0}
STUB_SLOTS = {SLOT_MARK: 'mark', SLOT_UNINIT: 'uninit', SLOT_RECORD_KILL: 'record_kill',
              SLOT_TRANSMIT_FIRST: 'transmit_first', SLOT_STUN: 'stun'}
STUB_AT = {name: STUBS + 0x10 * n for n, name in enumerate(STUB_POPS)}
STUB_NAME = {address: name for name, address in STUB_AT.items()}
# Direct calls replaced at their entry: address -> (name, bytes popped by its RET).
ENTRY_STUBS = {
    0x4138C0: ('tracker_update', 0xC), 0x4135D0: ('tracker_remove', 4),
    0x4A9770: ('display_remove', 4), 0x4A9720: ('display_submit', 4),
    0x70D690: ('fire_death_weapon', 4), 0x7509E0: ('play_at', 4),
    0x707CB0: ('kill_passengers', 4), 0x7258D0: ('announce_expired', 0),
}


def u32(value):
    return struct.pack('<I', value & 0xFFFFFFFF)


def i32(raw):
    return struct.unpack('<i', raw)[0]


class Fixture:
    """One emulator with a single airborne aircraft over a flat block of cells."""

    def __init__(self, case, family):
        self.case, self.family = case, family
        self.calls = []
        u = self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(REGION, REGION_SIZE)
        u.mem_map(CELLS, CELLS_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.call(NULL_COORD_INIT, None)
        # Map: a flat block of real cells around ORIGIN; everything else is empty.
        u.mem_write(TABLE, bytes(0x40000 * 4))
        u.mem_write(TABLE_PTR, dwords(TABLE, 0x40000))
        u.mem_write(MAP + 0xF4, dwords(*MAP_SIZE))
        land = case.get('land_type', 0)
        for dy in range(SIDE):
            for dx in range(SIDE):
                x, y = ORIGIN[0] + dx, ORIGIN[1] + dy
                cell = CELLS + (dy * SIDE + dx) * CELL_SIZE
                u.mem_write(TABLE + (y * 512 + x) * 4, dwords(cell))
                u.mem_write(cell, dwords(0x7E4EEC, 0x7E4ED0))      # CellClass vtables
                u.mem_write(cell + 0x24, packed(x, y))
                u.mem_write(cell + 0x11B, bytes([case.get('level', 0), 0]))
                u.mem_write(cell + 0xEC, dwords(land))
                u.mem_write(cell + 0x140, dwords(0x100 if case.get('bridge') else 0))
        # LevelHeight 104 (read by the cell ground query 0x0047B3A0) and the 416 bridge scalars.
        for address, value in [(0x89E7C0, 104), (0xAC13C8, 104), (0xAC13BC, 416), (0xABC5DC, 416),
                               (0x8B3CAC, 416)]:
            u.mem_write(address, dwords(value))
        u.mem_write(FRAME, dwords(case.get('frame', 1000)))
        u.mem_write(RULES_PTR, dwords(RULES))
        u.mem_write(RULES + 0x7B4, dwords(1500))
        u.mem_write(RULES + 0x200, dwords(case.get('rules_water_sound', 70)))
        u.mem_write(RULES + 0x204, dwords(case.get('rules_land_sound', 71)))
        u.mem_write(SCENARIO_PTR, dwords(SCENARIO))
        u.mem_write(I_KNOW_WHAT_IM_DOING, dwords(case.get('i_know', 0)))
        # Scenario RNG: the original seeder's state for this seed.
        self.call(RNG_SEED, RNG, case.get('seed', 1))
        # Owner: the Aircraft vtable is cloned so virtual calls can be substituted.
        u.mem_write(STUBS, b'\xCC' * 0x100)
        vt = bytearray(u.mem_read(AIRCRAFT_VT, 0x600))
        for offset, name in STUB_SLOTS.items():
            vt[offset:offset + 4] = u32(STUB_AT[name])
        u.mem_write(VTABLE, bytes(vt))
        u.mem_write(OWNER, dwords(VTABLE))
        u.mem_write(OWNER + 0x14, dwords(4))
        u.mem_write(OWNER + 0x21C, dwords(HOUSE))
        u.mem_write(OWNER + 0x6C, dwords(case.get('health', 0)))
        u.mem_write(OWNER + 0x90, bytes([1]))                  # IsAlive
        u.mem_write(OWNER + 0x9C, dwords(*case.get('xyz', (START, START, 1500))))
        u.mem_write(OWNER + 0x425, bytes([case.get('crashing', 1)]))
        u.mem_write(OWNER + 0x6C4, dwords(TYPE))
        u.mem_write(OWNER + 0x6C0, dwords(FLY_CONTROL_VT))
        u.mem_write(OWNER + 0x674, dwords(LOCO + 4))
        u.mem_write(OWNER + 0x330, struct.pack('<ff', *case.get('rates', (0.0, 0.0))))
        u.mem_write(OWNER + 0x328, struct.pack('<ff', *case.get('angles', (0.0, 0.0))))
        u.mem_write(TYPE, dwords(AIRCRAFT_TYPE_VT))
        u.mem_write(TYPE + 0x618, dwords(-1))                  # FlightLevel: Rules fallback
        u.mem_write(TYPE + 0xA0, dwords(case.get('strength', 150)))
        u.mem_write(TYPE + 0x53C, dwords(case.get('water_sound', -1)))
        u.mem_write(TYPE + 0x540, dwords(case.get('land_sound', 12)))
        u.mem_write(TYPE + 0xD6A, bytes([case.get('balloon_hover', 0)]))
        # Type Speed through the original ReadINI conversion block.
        from unicorn.x86_const import UC_X86_REG_EBP
        u.mem_write(TYPE + 0x678, dwords(0))
        u.reg_write(UC_X86_REG_EBP, TYPE)
        u.reg_write(UC_X86_REG_EAX, case.get('ini_speed', 14) & 0xFFFFFFFF)
        run_checked(u, *TYPE_SPEED_CONVERSION, count=100)
        # Facing: original constructor, ROT and current heading.
        self.call(FACING_CONSTRUCTOR, OWNER + 0x388)
        self.call(FACING_SET_ROT, OWNER + 0x388, case.get('rot', 5))
        u.mem_write(ARG, dwords(case.get('facing', 0x4000)))
        self.call(FACING_SET, OWNER + 0x388, ARG)
        if 'turn_to' in case:
            u.mem_write(FRAME, dwords(case.get('frame', 1000) - case.get('turn_age', 0)))
            u.mem_write(ARG, dwords(case['turn_to']))
            self.call(0x4C9220, OWNER + 0x388, ARG)
            u.mem_write(FRAME, dwords(case.get('frame', 1000)))
        # Fly: original constructor, then the in-flight state at the kill.
        self.call(FLY_CONSTRUCTOR, LOCO)
        u.mem_write(LOCO + 0xC, dwords(OWNER))
        u.mem_write(LOCO + 0x1C, dwords(*case.get('destination', (64 * 256, 52 * 256, 1500))))
        u.mem_write(LOCO + 0x34, bytes([case.get('moving', 1)]))
        u.mem_write(LOCO + 0x38, dwords(case.get('target_height', 1500)))
        u.mem_write(LOCO + 0x40, struct.pack('<d', case.get('target_speed', 1.0)))
        u.mem_write(LOCO + 0x48, struct.pack('<d', case.get('speed_bits', 65536) / 65536))
        u.mem_write(LOCO + 0x58, dwords(case.get('counter', 0)))
        u.mem_write(LOCO + 0x5C, bytes([case.get('cruise', 1)]))
        u.hook_add(UC_HOOK_CODE, self.on_stub, begin=STUBS, end=STUBS + 0xFF)
        for address in ENTRY_STUBS:
            u.hook_add(UC_HOOK_CODE, self.on_entry_stub, begin=address, end=address)

    # -- helpers ---------------------------------------------------------------------------------
    def call(self, entry, ecx, *args, count=200_000):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        if ecx is not None:
            u.reg_write(UC_X86_REG_ECX, ecx)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, entry, RET_MAGIC, count=count)
        return u.reg_read(UC_X86_REG_EAX)

    def word(self, address):
        return struct.unpack('<I', self.u.mem_read(address, 4))[0]

    def owner_xyz(self):
        return list(struct.unpack('<iii', self.u.mem_read(OWNER + 0x9C, 12)))

    def ret(self, pops, eax=None):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        if eax is not None:
            u.reg_write(UC_X86_REG_EAX, eax)
        u.reg_write(UC_X86_REG_ESP, sp + 4 + pops)
        u.reg_write(UC_X86_REG_EIP, self.word(sp))

    def on_stub(self, u, address, _size, _data):
        name = STUB_NAME.get(address)
        if name is None:
            raise OracleError(f'unexpected stub address 0x{address:08X}')
        sp = u.reg_read(UC_X86_REG_ESP)
        entry = {'call': name}
        if STUB_POPS[name]:
            entry['arg'] = i32(u.mem_read(sp + 4, 4))
        if name in ('uninit', 'mark'):
            entry['xyz'] = self.owner_xyz()
        self.calls.append(entry)
        self.ret(STUB_POPS[name], eax=1 if name == 'stun' else 0)

    def on_entry_stub(self, u, address, _size, _data):
        name, pops = ENTRY_STUBS[address]
        sp = u.reg_read(UC_X86_REG_ESP)
        entry = {'call': name}
        if name == 'fire_death_weapon':
            entry.update(arg=i32(u.mem_read(sp + 4, 4)), xyz=self.owner_xyz(),
                         health=i32(u.mem_read(OWNER + 0x6C, 4)))
        elif name == 'play_at':
            entry.update(sound=i32(dwords(u.reg_read(UC_X86_REG_ECX))),
                         xyz=list(struct.unpack('<iii', u.mem_read(u.reg_read(UC_X86_REG_EDX), 12))),
                         controller=self.word(sp + 4))
        elif name == 'kill_passengers':
            entry['arg'] = i32(u.mem_read(sp + 4, 4))
        elif name == 'announce_expired':
            entry['removed'] = u.reg_read(UC_X86_REG_EDX) & 0xFF
        self.calls.append(entry)
        self.ret(pops, eax=0)

    def next_random(self):
        """The Scenario stream's next raw draw, continuing the row's state."""
        return self.call(RANDOM, RNG) & 0xFFFFFFFF


# ---- families ---------------------------------------------------------------------------------
def crash_row(case):
    f = Fixture(dict(case, crashing=0), 'crash')
    u = f.u
    draws = []

    def on_ranged(_u, _address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        draws.append([i32(u.mem_read(sp + 4, 4)), i32(u.mem_read(sp + 8, 4))])

    u.hook_add(UC_HOOK_CODE, on_ranged, begin=RANDOM_RANGED, end=RANDOM_RANGED)
    result = f.call(CRASH, OWNER, case.get('attacker', 0))
    rates = struct.unpack('<ff', u.mem_read(OWNER + 0x330, 8))
    rate_bits = struct.unpack('<II', u.mem_read(OWNER + 0x330, 8))
    return dict(input=case, returned=bool(result & 0xFF), crashing=u.mem_read(OWNER + 0x425, 1)[0],
                health=i32(u.mem_read(OWNER + 0x6C, 4)), draws=draws,
                sideways=rates[0], forwards=rates[1],
                sideways_bits=f'{rate_bits[0]:08x}', forwards_bits=f'{rate_bits[1]:08x}',
                calls=f.calls, next_random=f.next_random())


def fall_row(case):
    f = Fixture(case, 'fall')
    u = f.u
    frames = []
    frame = case.get('frame', 1000)
    for n in range(case.get('max_frames', 120)):
        u.mem_write(FRAME, dwords(frame + n))
        before = len(f.calls)
        result = f.call(ILOCO_PROCESS, None, LOCO + 4, count=2_000_000)
        speed = struct.unpack('<d', u.mem_read(LOCO + 0x48, 8))[0]
        routine = ('mark', 'tracker_update', 'display_remove', 'display_submit')
        names = [c['call'] for c in f.calls[before:]]
        row = dict(xyz=f.owner_xyz(), counter=i32(u.mem_read(LOCO + 0x58, 4)),
                   moving=u.mem_read(LOCO + 0x34, 1)[0], speed=speed,
                   returned=result & 0xFF,
                   # The fall block's Remove/SetLocation/Submit ran (a legal cell).
                   placed='display_remove' in names and 'display_submit' in names,
                   calls=[c for c in f.calls[before:] if c['call'] not in routine])
        frames.append(row)
        if any(c['call'] == 'uninit' for c in f.calls[before:]):
            break
    else:
        raise OracleError(f"{case['name']}: no impact within {case.get('max_frames', 120)} frames")
    return dict(input=case, frames=frames, next_random=f.next_random())


def rocking_row(case):
    f = Fixture(case, 'rocking')
    history = []
    for _ in range(case.get('frames', 8)):
        f.call(ROCKING_UPDATE, OWNER)
        raw = bytes(f.u.mem_read(OWNER + 0x328, 16))
        history.append([f'{x:08x}' for x in struct.unpack('<IIII', raw)])
    return dict(input=case, history=history, calls=f.calls)


def smoke_row(case):
    f = Fixture(case, 'smoke')
    u = f.u
    u.mem_write(RULES + 0x1708, struct.pack('<d', case.get('condition_red', 0.25)))
    u.mem_write(TYPE + 0xA0, dwords(case.get('strength', 150)))
    anims = []

    def on_anim(_u, _address, _size, _data):
        anims.append(f.owner_xyz())

    u.hook_add(UC_HOOK_CODE, on_anim, begin=0x421EA0, end=0x421EA0)
    draws = []

    def on_ranged(_u, _address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        draws.append([i32(u.mem_read(sp + 4, 4)), i32(u.mem_read(sp + 8, 4))])

    u.hook_add(UC_HOOK_CODE, on_ranged, begin=RANDOM_RANGED, end=RANDOM_RANGED)
    # Entry state of the block: ESI = the aircraft, EBX = &Location.
    u.reg_write(UC_X86_REG_ESI, OWNER)
    u.reg_write(UC_X86_REG_EBX, OWNER + 0x9C)
    u.reg_write(UC_X86_REG_ESP, SP)
    # The anim allocation and constructor are replaced by a zero allocation:
    # operator new returning NULL skips the constructor (0x004150E7).
    def on_new(_u, _address, _size, _data):
        f.ret(0, eax=0)

    u.hook_add(UC_HOOK_CODE, on_new, begin=0x7C8E17, end=0x7C8E17)
    allocations = []

    def on_alloc(_u, _address, _size, _data):
        allocations.append(True)

    u.hook_add(UC_HOOK_CODE, on_alloc, begin=0x4150D6, end=0x4150D6)
    run_checked(u, SMOKE_BEGIN, SMOKE_END, count=20_000)
    return dict(input=case, draws=draws, smoke=bool(allocations), next_random=f.next_random())


# ---- cases ------------------------------------------------------------------------------------
def crash_cases():
    cases = []
    for seed in range(1, 11):
        cases.append(dict(name=f'dead_seed{seed}', seed=seed, health=0, attacker=0x1234))
    cases += [
        dict(name='dead_grounded', health=0, xyz=(START, START, 0)),
        dict(name='dead_one_lepton', health=0, xyz=(START, START, 1), seed=4),
        dict(name='dead_i_know', health=0, i_know=1, seed=3),
        dict(name='live_seed5', health=75, seed=5, attacker=0),
        dict(name='live_grounded', health=75, xyz=(START, START, 0)),
    ]
    return [crash_row(case) for case in cases]


def fall_cases():
    base = dict(health=0, crashing=1, ini_speed=14, speed_bits=65536, facing=0x4000)
    cases = [
        dict(base, name='cruise_1500_full_speed'),
        dict(base, name='cruise_1500_half_speed', speed_bits=32768),
        dict(base, name='cruise_1500_stopped', speed_bits=0),
        dict(base, name='not_moving', moving=0),
        dict(base, name='low_300', xyz=(START, START, 300)),
        dict(base, name='low_21', xyz=(START, START, 21)),
        dict(base, name='low_1', xyz=(START, START, 1)),
        dict(base, name='above_target_2200', xyz=(START, START, 2200)),
        dict(base, name='heading_se', facing=0xA000),
        dict(base, name='turning', facing=0x4000, turn_to=0xC000, rot=5, turn_age=0),
        dict(base, name='level4_ground', level=4, xyz=(START, START, 1500 + 416)),
        dict(base, name='water_impact', land_type=2, water_sound=33),
        dict(base, name='water_rules_fallback', land_type=2, water_sound=-1),
        dict(base, name='land_rules_fallback', land_sound=-1),
        dict(base, name='over_bridge', bridge=True),
    ]
    return [fall_row(case) for case in cases]


def rocking_cases():
    return [rocking_row(case) for case in [
        dict(name='spin', rates=(0.1834, 0.0412), frames=40),
        dict(name='negative_spin', rates=(-0.25, 0.1), frames=40),
        dict(name='balloon_clamp', rates=(0.2, 0.05), balloon_hover=1, frames=12),
        dict(name='from_angles', rates=(0.15, 0.02), angles=(1.0, -0.5), frames=6),
    ]]


def smoke_cases():
    cases = []
    for health, strength in ((0, 150), (37, 150), (38, 150), (150, 150), (1, 150)):
        for seed in (1, 2, 3):
            cases.append(dict(name=f'h{health}_s{seed}', health=health, strength=strength, seed=seed))
    cases += [dict(name='grounded_dead', health=0, xyz=(START, START, 0), seed=1),
              dict(name='exact_red', health=375, strength=1500, seed=2)]
    return [smoke_row(case) for case in cases]


def generate():
    return dict(crash=crash_cases(), fall=fall_cases(), rocking=rocking_cases(),
                smoke=smoke_cases())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'crash': CRASH, 'iloco_process': ILOCO_PROCESS, 'process': 0x4CD600,
                      'rocking_update': ROCKING_UPDATE, 'smoke_begin': SMOKE_BEGIN,
                      'smoke_end': SMOKE_END, 'random_ranged': RANDOM_RANGED,
                      'rng_seed': RNG_SEED, 'fly_constructor': FLY_CONSTRUCTOR},
        assumptions=[
            'One Aircraft on the original (cloned) Aircraft vtable, real AircraftType, FacingClass and FlyLocomotionClass constructed by their original code; a flat 24x24 block of real cells (level/LandType per row) in the MapClass table; MapSize 64x64 (the block lies inside its playable diamond).',
            'Scenario RNG seeded by the original seeder 0x0065C6D0 per row; next_random is the next raw Random() on the same stream after the row.',
            'fall: Health 0, IsCrashing, Fly moving flag/current speed/target height/destination supplied as at the kill (the preceding Stun and Crash are not re-run). PC53/chop.',
            'smoke: entry state of the block (ESI owner, EBX &Location); operator new returns NULL so the AnimClass constructor is not run; the draw and the allocation decision are the outputs.',
        ],
        substitutions=[
            'crash: vt+0x274 Transmit_Radio_ToFirst, vt+0x3A0 Stun, vt+0xE0 RecordKill, KillPassengers 0x00707CB0 and AnnounceExpiredPointer 0x007258D0 recorded and returned without running.',
            'fall: vt+0x124 Mark, vt+0xF8 UnInit, AircraftTracker 0x004138C0/0x004135D0, DisplayClass 0x004A9770/0x004A9720, Fire_Death_Weapon 0x0070D690 and VocClass::PlayAt 0x007509E0 recorded and returned without running.',
        ],
        scope='Crash on dead/live/grounded/IKnowWhatImDoing aircraft over 15 seeds; the full per-frame Fly fall of a Health-0 aircraft from several heights, speeds, headings and grounds to its impact frame, death-weapon call, impact sound choice and UnInit; the crashing rocking accumulation; the red-health smoke gate and draw. Excludes the Techno death arm, the death weapon bullet, AnimClass construction and sound playback.',
    ))
