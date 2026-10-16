"""Original Object receiver HP authority, self-heal pulse and Strength-copy slices.

Object5F5390 executes unchanged with ignoreDefenses=1 (no warhead kernel), real
copied Unit/Building type/RTTI getters, null trigger linkage/source, ObjectAlive1.
Only changed/kill/destroy virtual callbacks are supplied, explicitly recorded.
This is not Techno/Foot/Infantry wrapper or full death/repair lifecycle parity.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UcError, UC_ERR_EXCEPTION, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP,
    UC_X86_REG_EIP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    load_image, run_checked, finish_vectors, provenance,
    STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC,
)
from tools.spatial_oracle.map_queries import dwords

OBJ, TYPE, VT, PACKET, RULES = [SCRATCH+n for n in (0, 0x2000, 0x4000, 0x5000, 0x6000)]
SOURCE, TAG = SCRATCH+0xA000, SCRATCH+0xB000
SP = STACK_BASE + STACK_SIZE - 0x1000
ESTIMATE = -987654321
RANGES = ((0x5F5390, 0x5F584D), (0x70BE80, 0x70BF47),
          (0x6FA743, 0x6FA75A), (0x6F3270, 0x6F3278),
          (0x741490, 0x741497), (0x459EE0, 0x459EE7),
          (0x746E20, 0x746E26), (0x459EC0, 0x459EC6),
          (0x7355BA, 0x7355C6), (0x517D51, 0x517D69),
          (0x414051, 0x41405D), (0x442C75, 0x442C81),
          (0x7C5F00, 0x7C5F2C), (0x74FF90, 0x74FFB7),
          (0x750010, 0x750028),
          (0x41B3A5, 0x41B3EE), (0x7435DA, 0x743623),
          (0x51FE5C, 0x51FEA6), (0x44FB67, 0x44FBD2),
          (0x41B1A3, 0x41B1AE), (0x743334, 0x743348),
          (0x51FB9A, 0x51FBA5), (0x44F8F7, 0x44F904))
BRANCHES = (0x5F53AA, 0x5F53B7, 0x5F544F, 0x5F545C,
            0x5F546A, 0x5F5478, 0x5F5480, 0x5F548C,
            0x5F54B8, 0x5F54C2, 0x5F54F9, 0x5F550D,
            0x5F578D, 0x5F579A, 0x5F57AF, 0x5F5830, 0x5F583E)
BUILDING_ZERO_HP_RANGES = ((0x43B762, 0x43B795), (0x43B95A, 0x43B960),
                           (0x440042, 0x4400F2))
BUILDING_ZERO_HP_SINKS = {0x445880: ('limbo', 0),
                          0x442D90: ('spawn_survivors', 4),
                          0x5F65F0: ('uninit', 0),
                          0x441F60: ('after_destruction', 0)}


def i32(value):
    return struct.unpack('<i', dwords(value))[0]


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        for base, size in ((STACK_BASE, STACK_SIZE), (SCRATCH, 0x10000), (RET_MAGIC, 0x1000)):
            self.u.mem_map(base, size)
        self.original = [bytes(self.u.mem_read(a, b-a)) for a, b in RANGES]
        self.u.hook_add(UC_HOOK_CODE, self.code)
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.write)

    def read(self, address):
        return struct.unpack('<i', self.u.mem_read(address, 4))[0]

    def reset(self, kind, current, strength, alive=1):
        self.u.mem_write(SCRATCH, bytes(0x10000))
        self.u.mem_write(STACK_BASE+STACK_SIZE-0x2000, bytes(0x2000))
        original_vt = 0x7F5C70 if kind == 'unit' else 0x7E3EBC
        self.u.mem_write(VT, bytes(self.u.mem_read(original_vt, 0x600)))
        self.u.mem_write(OBJ, dwords(VT))
        self.u.mem_write(OBJ+(0x6C4 if kind == 'unit' else 0x520), dwords(TYPE))
        self.u.mem_write(OBJ+0x6C, dwords(current, ESTIMATE))
        self.u.mem_write(OBJ+0x90, bytes((alive,)))
        self.u.mem_write(TYPE+0xA0, dwords(strength))
        self.u.mem_write(0x8871E0, dwords(RULES))
        self.u.mem_write(RULES+0x1708, struct.pack('<d', .25))
        self.u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ECX, OBJ)
        self.u.reg_write(UC_X86_REG_ESI, OBJ)
        self.writes, self.callbacks, self.branches = [], [], []
        self.stubs, self.callback_mode = {}, 'none'
        self.damage_source, self.tag_context = 0, None
        self.tag_calls = []
        self.period = None
        self.building_zero_hp_context = None

    def write(self, u, _access, address, size, value, _data):
        if address in (OBJ+0x6C, OBJ+0x70):
            assert size == 4
            self.writes.append(dict(instruction=f'{u.reg_read(UC_X86_REG_EIP):08X}',
                                   field='actual' if address == OBJ+0x6C else 'estimated',
                                   value=i32(value)))

    def code(self, u, address, _size, _data):
        if self.building_zero_hp_context is not None and address in BUILDING_ZERO_HP_SINKS:
            name, argument_bytes = BUILDING_ZERO_HP_SINKS[address]
            sp = u.reg_read(UC_X86_REG_ESP)
            assert u.reg_read(UC_X86_REG_ECX) == OBJ
            row = dict(call=name, entry=f'{address:08X}', health=self.read(OBJ+0x6C))
            if argument_bytes:
                row['source_argument'] = self.read(sp+4)
            self.building_zero_hp_context.append(row)
            # Explicit ordered callout observations only. No world removal,
            # crew/RNG, smudge or rubble behavior is supplied or certified.
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_ESP, sp+4+argument_bytes)
            u.reg_write(UC_X86_REG_EIP, struct.unpack('<I', u.mem_read(sp, 4))[0])
            return
        if address == 0x6E53A0 and self.tag_context is not None:
            sp = u.reg_read(UC_X86_REG_ESP)
            event,actor,xy,force,source = struct.unpack('<IIIII',u.mem_read(sp+4,20))
            assert u.reg_read(UC_X86_REG_ECX) == TAG
            row = dict(kind=event,actor=actor,cell=xy,force=force,source=source,
                       health=self.read(OBJ+0x6C),alive=u.mem_read(OBJ+0x90,1)[0])
            if event == 6:
                if self.tag_context['mutation'] == 'detach_after6':
                    u.mem_write(OBJ+0x34,dwords(0))
                elif self.tag_context['mutation'] == 'clear_alive_after6':
                    u.mem_write(OBJ+0x90,b'\0')
            self.tag_calls.append(row)
            # Explicit callout sink, not original Tag/Trigger execution.
            u.reg_write(UC_X86_REG_EAX,1)
            u.reg_write(UC_X86_REG_ESP,sp+24)
            u.reg_write(UC_X86_REG_EIP,struct.unpack('<I',u.mem_read(sp,4))[0])
            return
        if address == 0x70BF0F:
            self.period = i32(u.reg_read(UC_X86_REG_EAX))
        if address in BRANCHES:
            self.branches.append(f'{address:08X}')
        if address not in self.stubs:
            return
        slot = self.stubs[address]
        sp = u.reg_read(UC_X86_REG_ESP)
        arg = self.read(sp+4)
        assert u.reg_read(UC_X86_REG_ECX) == OBJ
        row = dict(slot=f'{slot:03X}', argument=arg, health_before=self.read(OBJ+0x6C),
                   alive_before=int(u.mem_read(OBJ+0x90, 1)[0]), supplied_writes=[])
        mutation = None
        if slot == 0x148:
            assert arg == 7
            mutation = {'heal_zero': 0, 'heal_negative': -17, 'heal_above': 70001}.get(self.callback_mode)
        elif slot == 0xDC:
            assert arg == 1
            mutation = {'destroy_revive': 70001}.get(self.callback_mode)
        else:
            assert slot == 0xE0 and arg == self.damage_source
        if mutation is not None:
            u.mem_write(OBJ+0x6C, dwords(mutation))
            row['supplied_writes'].append(dict(field='actual', value=mutation))
        if (slot, self.callback_mode) in ((0x148, 'heal_clear_alive'), (0xDC, 'destroy_clear_alive'), (0xE0, 'kill_clear_alive')):
            u.mem_write(OBJ+0x90, b'\0')
            row['supplied_writes'].append(dict(field='alive', value=0))
        row['health_after'] = self.read(OBJ+0x6C)
        row['alive_after'] = int(u.mem_read(OBJ+0x90, 1)[0])
        self.callbacks.append(row)
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_ESP, sp+8)
        u.reg_write(UC_X86_REG_EIP, struct.unpack('<I', u.mem_read(sp, 4))[0])

    def unchanged(self):
        assert self.original == [bytes(self.u.mem_read(a, b-a)) for a, b in RANGES]

    def receiver(self, current, strength, damage, *, kind='unit', can_c4=True, alive=1, callback='none', tag_context=None):
        self.reset(kind, current, strength, alive)
        self.tag_context = tag_context
        if tag_context is not None:
            self.damage_source = SOURCE if tag_context['source'] else 0
            self.u.mem_write(OBJ+0x34,dwords(TAG if tag_context['tag'] else 0))
            self.u.mem_write(SOURCE+0x21C,dwords(0))
        self.u.mem_write(TYPE+0x1577, bytes((int(can_c4),)))
        self.callback_mode = callback
        for slot in (0x148, 0xE0, 0xDC):
            stub = SCRATCH+0x9000+slot
            self.u.mem_write(VT+slot, dwords(stub))
            self.stubs[stub] = slot
        self.u.mem_write(PACKET, dwords(damage))
        # int*damage, distance0, null warhead/source, ignoreDefenses1,
        # preventPassengerEscape1, null sourceHouse. Forced path never dereferences warhead.
        self.u.mem_write(SP, dwords(RET_MAGIC, PACKET, 0, 0, self.damage_source, 1, 1, 0))
        run_checked(self.u, 0x5F5390, RET_MAGIC, count=2000, required_addresses=(0x5F53A1,))
        assert self.u.reg_read(UC_X86_REG_ESP) == SP+32
        assert self.read(OBJ+0x70) == ESTIMATE
        self.unchanged()
        result = dict(input=dict(kind=kind, current=current, strength=strength, damage=damage,
                               can_c4=can_c4, alive=alive, callback=callback),
                    output=dict(actual=self.read(OBJ+0x6C), estimated=self.read(OBJ+0x70),
                                damage_packet=self.read(PACKET), result=i32(self.u.reg_read(UC_X86_REG_EAX)),
                                alive=int(self.u.mem_read(OBJ+0x90, 1)[0]), writes=self.writes,
                                callbacks=self.callbacks, branches=self.branches))
        if tag_context is not None:
            result['input']['tag_context'] = tag_context
            result['output']['tag_calls'] = self.tag_calls
        return result

    def self_heal(self, current, strength, frame, *, kind='unit', enabled=True, repair_rate=1.0, expect_trap=False):
        self.reset(kind, current, strength)
        self.u.mem_write(TYPE+0xD14, bytes((int(enabled),)))
        # SelfHealing=yes bypasses rank; disabled rows use zero rookie rank.
        self.u.mem_write(RULES+0x16E0, struct.pack('<d', repair_rate))
        self.u.mem_write(0xA8ED84, dwords(frame))
        if expect_trap:
            run_checked(self.u, 0x6FA743, 0x70BF17, count=500)
            assert self.period == 0 or (i32(frame) == -2147483648 and self.period == -1)
            try:
                self.u.emu_start(0x70BF17, 0x70BF19, count=1)
            except UcError as error:
                assert error.errno == UC_ERR_EXCEPTION
            else:
                raise AssertionError('Original IDIV unexpectedly did not fault')
            assert self.u.reg_read(UC_X86_REG_EIP) == 0x70BF17
            assert self.u.reg_read(UC_X86_REG_ESP) == SP-16
            assert not self.writes
            stop = 0x70BF17
        else:
            stop = run_checked(self.u, 0x6FA743, (0x6FA75A, 0x6FA793), count=500)
            assert self.u.reg_read(UC_X86_REG_ESP) == SP
        assert self.read(OBJ+0x70) == ESTIMATE
        assert not self.callbacks
        self.unchanged()
        return dict(input=dict(kind=kind, current=current, strength=strength, frame=frame,
                               repair_rate_bits=struct.pack('>d', repair_rate).hex(), self_healing=enabled),
                    output=dict(actual=self.read(OBJ+0x6C), estimated=self.read(OBJ+0x70),
                                period=self.period, frame_signed=i32(frame), divide_error=expect_trap,
                                entered_increment=stop == 0x6FA75A, stop=f'{stop:08X}', writes=self.writes))

    def map_health(self, kind, authored, strength):
        self.reset('building' if kind == 'building' else 'unit', -123, strength)
        self.u.mem_write(OBJ+0x6C0, dwords(TYPE))
        self.u.reg_write(UC_X86_REG_EDI, OBJ)
        start, stop, local = {
            'aircraft': (0x41B3A5, 0x41B3EE, 0x20),
            'unit': (0x7435DA, 0x743623, 0x48),
            'infantry': (0x51FE5C, 0x51FEA6, 0x24),
            'building': (0x44FB67, 0x44FBD2, 0x18),
        }[kind]
        self.u.mem_write(SP+local, dwords(authored))
        run_checked(self.u, start, stop, count=120)
        # Foot/Aircraft push mission callback arguments before the estimate store.
        assert self.u.reg_read(UC_X86_REG_ESP) == SP-(0 if kind == 'building' else 8)
        assert not self.callbacks
        self.unchanged()
        return dict(input=dict(kind=kind, authored=authored, strength=strength),
                    output=dict(actual=self.read(OBJ+0x6C), estimated=self.read(OBJ+0x70),
                                authored_after=self.read(SP+local), writes=self.writes))

    def constructor_copy(self, kind, strength):
        self.reset('unit', -123, strength)
        self.u.reg_write(UC_X86_REG_EAX, TYPE)
        self.u.mem_write(OBJ+0x6C0, dwords(TYPE))
        start, stop = {'unit': (0x7355BA, 0x7355C6), 'infantry': (0x517D51, 0x517D69),
                       'aircraft': (0x414051, 0x41405D), 'building': (0x442C75, 0x442C81)}[kind]
        run_checked(self.u, start, stop, count=12)
        assert self.u.reg_read(UC_X86_REG_ESP) == SP
        self.unchanged()
        return dict(input=dict(kind=kind, strength=strength),
                    output=dict(actual=self.read(OBJ+0x6C), estimated=self.read(OBJ+0x70), writes=self.writes))

    def building_zero_health(self, health, constructor_frame, frame, *, timer=None):
        """Constructor timer/default-byte slices, then the original AI gate.

        Empty fire-Anims and an admitted Building are supplied priors. The
        original Health sampler executes, with a valid supplied House; full
        constructor, Unlimbo, preceding AI, and terminal callees are excluded.
        """
        self.reset('building', health, 1000)
        original = [bytes(self.u.mem_read(a, b-a)) for a, b in BUILDING_ZERO_HP_RANGES]
        self.u.mem_write(OBJ+0x21C, dwords(SOURCE))
        self.u.mem_write(0xA8ED84, dwords(constructor_frame))
        self.u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(self.u, 0x43B762, 0x43B795, count=20,
                    required_addresses=(0x43B76B, 0x43B771, 0x43B789))
        # BL=0 is owned by the same constructor's 43B754, not a type default.
        self.u.mem_write(OBJ+0x6E0, b'\x7f')
        run_checked(self.u, 0x43B95A, 0x43B960, count=2)
        defaults = dict(timer_start=self.read(OBJ+0x528),
                        timer_duration=self.read(OBJ+0x530),
                        source=self.read(OBJ+0x540),
                        no_survivor=self.u.mem_read(OBJ+0x6E0, 1)[0])
        if timer is not None:
            self.u.mem_write(OBJ+0x528, dwords(timer[0], 0, timer[1]))
        self.u.mem_write(0xA8ED84, dwords(frame))
        # Both upstream AI arms establish EBP=-1 at 43FB73 / 43FBFD.
        self.u.reg_write(UC_X86_REG_EBP, 0xFFFFFFFF)
        self.building_zero_hp_context = []
        stop = run_checked(self.u, 0x440042, (0x4400EA, 0x4400F2, 0x440573), count=200,
                           required_addresses=(0x440072,))
        assert self.u.reg_read(UC_X86_REG_ESP) == SP
        assert original == [bytes(self.u.mem_read(a, b-a)) for a, b in BUILDING_ZERO_HP_RANGES]
        self.unchanged()
        return dict(input=dict(health=health, constructor_frame=constructor_frame,
                               frame=frame, timer_override=timer),
                    output=dict(constructor_defaults=defaults,
                                timer_start=self.read(OBJ+0x528),
                                timer_duration=self.read(OBJ+0x530),
                                health=self.read(OBJ+0x6C),
                                health_sample=self.read(OBJ+0x544),
                                stop=f'{stop:08X}',
                                terminal_callouts=self.building_zero_hp_context))


def generate():
    f = Fixture()
    hp = (-2147483648, -1, 0, 1, 2, 49, 50, 51, 75, 100, 101, 65535, 65536, 2147483647)
    strengths = (-2147483648, -1, 0, 1, 100, 65536, 2147483647)
    damages = (-2147483648, -65536, -1, 0, 1, 65536, 2147483647)
    receiver = [f.receiver(*args) for args in product(hp, strengths, damages)]
    receiver += [f.receiver(current, 300, damage) for current, damage in
                 ((151, 1), (150, 1), (149, 1), (76, 1), (75, 1), (74, 1), (301, 151), (151, 77))]
    for can_c4, args in product((False, True), ((100, 100, -2147483648), (100, 100, -1),
                                             (100, 100, 0), (100, 100, 1), (1, 100, 2),
                                             (65536, 70000, 65536), (-1, 100, 1))):
        receiver.append(f.receiver(*args, kind='building', can_c4=can_c4))
    receiver += [f.receiver(90, 100, -1, callback=mode) for mode in
                 ('heal_zero', 'heal_negative', 'heal_above', 'heal_clear_alive')]
    receiver += [f.receiver(100, 100, 100, callback=mode) for mode in
                 ('destroy_revive', 'destroy_clear_alive', 'kill_clear_alive')]
    receiver += [f.receiver(100, 100, delta, alive=0) for delta in (-1, 1, 100)]
    pairs = ((-2147483648, 100), (-1, 100), (0, 100), (1, 100), (99, 100), (100, 100),
             (101, 100), (65535, 65536), (65536, 65536), (65536, 70000),
             (2147483647, 100), (2147483647, 2147483647), (1, 0), (1, -1),
             (-1, -1), (0, 0))
    self_heal = [f.self_heal(current, strength, frame, kind=kind)
                 for (current, strength), frame, kind in product(pairs, (0, 899, 900), ('unit', 'building'))]
    self_heal += [f.self_heal(current, 100, 0, enabled=False) for current in (-1, 99, 2147483647)]
    cadence = ((4294966396, 1.0), (4294966500, 1.0), (900, -1.0),
               (0, -1.0), (4294966396, -1.0), (2147483647, 1.0),
               (2147483648, 1.0), (2147483648, -1.0),
               (2147483647, .002), (2147483648, .002),
               (2147483647, -.002), (4294967295, -.002),
               (4294966396, .5), (4294966396, -.5),
               (3254779904, 8388608.0), (3774873600, 4194304.0))
    self_heal += [f.self_heal(99, 100, frame, kind=kind, repair_rate=rate)
                  for (frame, rate), kind in product(cadence, ('unit', 'building'))]
    # Fault cases use fresh CPU instances: an x86 divide exception is not resumed
    # or normalized into an ordinary eligibility result.
    traps = [Fixture().self_heal(99, 100, frame, repair_rate=rate, expect_trap=True)
             for frame, rate in ((0, 0.0), (900, .0001), (900, 1e30),
                                 (900, 4294967296.0), (2147483648, -.002))]
    constructors = [f.constructor_copy(kind, strength) for kind, strength in product(
        ('unit', 'infantry', 'aircraft', 'building'),
        (-2147483648, -1, 0, 1, 100, 65535, 65536, 2147483647))]
    map_health = [f.map_health(kind, authored, strength) for kind, authored, strength in product(
        ('unit', 'infantry', 'aircraft', 'building'),
        (-2147483648, -1, 0, 1, 128, 248, 249, 250, 251, 255, 256, 257, 2147483647),
        (-2147483648, -1, 0, 1, 3, 100, 65536, 2147483647))]
    assert (len(receiver), len(self_heal), len(constructors), len(traps), len(map_health)) == (718, 131, 32, 5, 416)
    return dict(schema_version=2, receiver=receiver, self_heal=self_heal,
                self_heal_traps=traps, constructors=constructors, map_health=map_health)


def metadata():
    f = Fixture()
    result = provenance(
        scope='Original Object5F5390 HP commit/return, original self-heal eligibility70BE80/cadence/TechnoAI increment, four class Strength-copy and authored-health arithmetic slices',
        assumptions=[
            'PC53/chop FPCW0E7F; signed dword HP/Strength/damage; estimate sentinel -987654321 independently observed',
            'Receiver real forced ABI ignoreDefenses1/preventEscape1, null source/house/warhead, separate nonaliased damage packet, null trigger pointer+34 and redraw byte+830; copied retail Unit/Building vtables retain original RTTI/type getters',
            'Receiver main grid 14 HP x7 Strength x7 damage=686; plus8 crossing,14 Building CanC4,7 callback mutation and3 initial Alive0 controls=718 rows',
            'Self-heal original99 rows plus32 signed-frame/negative/wide-period cadence rows=131. Separate5 rows execute faulting IDIV for zero divisor (literal, truncated tiny rate, masked ftol indefinite, valid signed64 low32 wrap) and INT_MIN/-1; observed CPU exception is recorded, never normalized. Caller stops before post-increment smoke/animation tail',
            'Constructor32 rows execute only original class initial Strength copies with valid type pointer; no allocation/whole constructor, map admission, or malformed type-pointer policy claim',
            'Forced Object path omits Techno defense math, ordinary warhead kernel, Infantry/Foot/Building wrappers, Cyborg Infantry special arm and trigger scripts. Kill/Destroy/changed callback bodies and full lifecycle are not implemented by this fixture',
            'Positive code1/2/3/4/5 and healing return0 are native EAX observations, not host-side classification. Callback writes are separately labelled supplied_writes; native writes include instruction address',
            'Map416 rows supply parsed signed authored-health dword and valid type pointer, executing original class-specific arithmetic/snap/minimum and estimate copy. Building upper256 input clamp executes; Unit/Aircraft/Infantry slices have no such clamp. Whole textual parser/CRT overflow, lookup, constructor, Unlimbo and mission callbacks are excluded; saved lookup-rejection bytes are navigation/evidence only, not executed in these rows',
        ],
        substitutions=['Copied-vtable slots148/E0/DC point to explicit Python callback boundaries (argument7/nullSource/1). No-op by default, seven rows mutate actual HP or ObjectAlive synchronously; supplied callback EAX0. No original executable bytes are changed.'],
        entry_points=dict(object_receiver=0x5F5390, self_heal_caller=0x6FA743,
                          self_heal_eligibility=0x70BE80, unit_copy=0x7355BA,
                          infantry_copy=0x517D51, aircraft_copy=0x414051, building_copy=0x442C75,
                          unit_map=0x7435DA, infantry_map=0x51FE5C,
                          aircraft_map=0x41B3A5, building_map=0x44FB67))
    result['original_code'] = {f'{a:08X}..{b:08X}': dict(hex=raw.hex(), sha256=hashlib.sha256(raw).hexdigest())
                               for (a, b), raw in zip(RANGES, f.original)}
    result['copied_vtables'] = {f'{a:08X}': hashlib.sha256(bytes(f.u.mem_read(a, 0x600))).hexdigest()
                                for a in (0x7F5C70, 0x7E3EBC)}
    result['native_constants'] = {f'{a:08X}': bytes(f.u.mem_read(a, n)).hex()
                                  for a, n in ((0x7E1740, 8), (0x7E27F8, 8))}
    audit_ranges = ((0x41CAA0, 0x41CAE1), (0x747370, 0x7473B1),
                    (0x523C90, 0x523CF1), (0x45E7B0, 0x45E7F1),
                    (0x41B1E8, 0x41B201), (0x743379, 0x743391),
                    (0x51FBD6, 0x51FBEF), (0x44F904, 0x44F91D),
                    (0x41B391, 0x41B3A5), (0x7435C6, 0x7435DA),
                    (0x51FE48, 0x51FE5C), (0x44FBD2, 0x44FC07))
    result['map_audit_bytes_not_executed'] = {
        f'{a:08X}..{b:08X}': dict(hex=(raw := bytes(f.u.mem_read(a, b-a))).hex(),
                                sha256=hashlib.sha256(raw).hexdigest())
        for a, b in audit_ranges}
    return result


def generate_damage_tags():
    fixture = Fixture()
    cases = []
    for name,source,tag,damage,alive,mutation in (
        ('ordinary',True,True,1,1,'none'),
        ('no_source',False,True,1,1,'none'),
        ('no_tag',True,False,1,1,'none'),
        ('no_damage',True,True,0,1,'none'),
        ('healing',True,True,-1,1,'none'),
        ('lethal',True,True,80,1,'none'),
        ('already_not_alive',True,True,1,0,'none'),
        ('tag_detach',True,True,1,1,'detach_after6'),
        ('alive_clear',True,True,1,1,'clear_alive_after6')):
        row = fixture.receiver(80,100,damage,alive=alive,
            tag_context=dict(source=source,tag=tag,mutation=mutation))
        row['name'] = name
        cases.append(row)
    return cases


def damage_tag_metadata():
    return provenance(
        scope='Nine unchanged Object5F5390 receiver controls for surviving damage Tag6/44 order, null-source/null-tag, early returns and synchronous detach/alive mutation. Not original Tag-body, Techno wrapper or complete receiver parity.',
        assumptions=[
            'Reuse the existing Object HP Fixture with copied original Unit vtable/type getters, strength100/current80, ignoreDefenses=true and explicit source/tag storage.',
            'Tag6E53A0 is a recorded sink returning1. Only named mutation controls change the supplied Object attachment/alive fields; original event predicates/actions are excluded.',
            'Changed/kill/destroy virtual sinks remain the established HP fixture boundary. Actual native Object instructions and arithmetic execute; no gameplay getter is replaced.'],
        substitutions=['Original Tag callout is supplied and recorded; platform transport and existing virtual HP callback sinks remain.'],
        entry_points={'object_receive_damage':0x5F5390,'tag_callout':0x6E53A0})


class DamageEventsObservation:
    """Observe the existing receiver VM, including declared callee mutations.

    Writes at the genuine return PC supply the sink's post-call fields before
    the next original instruction. No Tag action or death body is simulated.
    This observer is used only by tag_lifecycle's additive --damage-events.
    """
    TAG_RETURNS = {0x5F561C:39,0x5F5643:42,0x5F5674:40,0x5F569B:43,
        0x5F56F6:38,0x5F5719:41,0x5F5744:41,0x5F57E0:6,0x5F580C:44}
    CALLBACK_RETURNS = {0x5F548C:'148',0x5F57A0:'0E0',0x5F57B5:'0DC'}
    MARKERS = {0x5F55F8:'save_pre_tag_health',0x5F5752:'read_pre_tag_health',
        0x5F575A:'read_live_health_gate',0x5F5765:'read_exact_zero',
        0x5F579A:'call_kill_e0',0x5F57AF:'call_destroy',
        0x5F5830:'return_saved_result',0x5F583E:'return_five'}

    def __init__(self):
        from tools.input_oracle.campaign_start import CampaignReader
        from tools.spatial_oracle.trigger_type_flags import FlagsFixture
        self.fixture=f=Fixture();f.reset('unit',100,100)
        # Outside the existing Fixture's resettable64KiB supplied objects.
        self.scenario=0x05000000;f.u.mem_map(self.scenario,0x10000)
        f.u.mem_write(0xA8B230,dwords(self.scenario))
        for ptr in (self.scenario+0x218,0x886B88,0xABE890):
            FlagsFixture.call(f,0x65C6D0,[31],ecx=ptr)
        self.rng_reader=CampaignReader.rng_state
        self.seeded=self.rng_reader(self)
        self.seed_code=bytes(f.u.mem_read(0x65C6D0,0x1BD))
        self.active={};self.hook=f.u.hook_add(UC_HOOK_CODE,self.observe)

    @property
    def u(self):
        return self.fixture.u

    def scalar_state(self):
        f=self.fixture
        return dict(health=f.read(OBJ+0x6C),alive=self.u.mem_read(OBJ+0x90,1)[0],
            attached_tag=f.read(OBJ+0x34)!=0,strength=f.read(TYPE+0xA0))

    def observe(self,u,pc,size,_data):
        a,f=self.active,self.fixture
        if not a:return
        if pc in (0x65C780,0x65C7E0,0x7258D0,0x6E4F60):
            a['rng_detach_calls'].append(f'0x{pc:08X}')
        if pc in self.MARKERS:
            marker=dict(pc=f'0x{pc:08X}',meaning=self.MARKERS[pc],live=self.scalar_state())
            if pc==0x5F5752:marker['saved_pre_tag_health']=f.read(u.reg_read(UC_X86_REG_ESP)+0x38)
            a['native_markers'].append(marker)
        returned=None
        if pc in self.TAG_RETURNS and len(f.tag_calls)>a['seen_tag_returns']:
            assert len(f.tag_calls)==a['seen_tag_returns']+1
            tag=f.tag_calls[-1];assert tag['kind']==self.TAG_RETURNS[pc]
            returned=dict(family='tag',kind=tag['kind'],source=tag['source'],
                native_return_pc=f'0x{pc:08X}',live=self.scalar_state())
            a['seen_tag_returns']+=1
        elif pc in self.CALLBACK_RETURNS and len(f.callbacks)>a['seen_virtual_returns']:
            assert len(f.callbacks)==a['seen_virtual_returns']+1
            assert f.callbacks[-1]['slot']==self.CALLBACK_RETURNS[pc]
            returned=dict(family='virtual',slot=f.callbacks[-1]['slot'],
                native_return_pc=f'0x{pc:08X}',live=self.scalar_state())
            a['seen_virtual_returns']+=1
        if returned is None:return
        a['ordered_returns'].append(returned)
        mutation=a['case'].get('mutation')
        if mutation is None or int(mutation['return_pc'],16)!=pc:return
        assert not a['mutations_applied']
        prior=self.scalar_state()
        for field,value in mutation['writes'].items():
            if field=='health':u.mem_write(OBJ+0x6C,dwords(value))
            elif field=='alive':u.mem_write(OBJ+0x90,bytes([bool(value)]))
            elif field=='attached_tag':u.mem_write(OBJ+0x34,dwords(TAG if value else 0))
            elif field=='strength':u.mem_write(TYPE+0xA0,dwords(value))
            else:raise ValueError(field)
        a['mutations_applied'].append(dict(
            seam='declared callee effects supplied at original return PC, before resumed native instruction',
            after_return=returned,requested_writes=mutation['writes'],prior=prior,after=self.scalar_state()))

    def run(self,case):
        a,f=self.active,self.fixture
        a.clear();a.update(case=case,seen_tag_returns=0,seen_virtual_returns=0,
            ordered_returns=[],native_markers=[],mutations_applied=[],rng_detach_calls=[])
        before=self.rng_reader(self);assert before==self.seeded
        result=f.receiver(case['initial_hp'],case['strength'],case['damage'],
            kind=case.get('kind','unit'),alive=case.get('alive',1),callback=case.get('callback','none'),
            tag_context=dict(source=case['source'],tag=True,mutation='none'))
        assert bool(a['mutations_applied'])==(case.get('mutation') is not None)
        assert a['seen_tag_returns']==len(f.tag_calls) and a['seen_virtual_returns']==len(f.callbacks)
        after=self.rng_reader(self);assert after==before and not a['rng_detach_calls']
        f.unchanged();assert bytes(self.u.mem_read(0x65C6D0,0x1BD))==self.seed_code
        return dict(name=case['name'],controlled_input=case,native_receiver=result,
            final_live=self.scalar_state(),ordered_returns=list(a['ordered_returns']),
            native_markers=list(a['native_markers']),mutations_applied=list(a['mutations_applied']),
            rng_before_ref='full_seed31',rng_after_ref='full_seed31',
            three_full_rng_states_compared_equal=True,rng_detach_calls=[])

    def finish(self):
        self.active.clear();self.u.hook_del(self.hook)
        return dict(full_seed31=self.seeded,full_after=self.rng_reader(self),unchanged=True)


def damage_event_mutation_inputs():
    """Declared post-callee state priors; output numbers come from Object code."""
    def row(name,pc=None,writes=None,**kw):
        return dict(name=name,initial_hp=kw.pop('initial_hp',100),strength=100,
            damage=kw.pop('damage',51),alive=kw.pop('alive',1),
            callback=kw.pop('callback','none'),source=True,
            mutation=None if pc is None else dict(return_pc=f'0x{pc:08X}',writes=writes),**kw)
    return [
        row('positive_post_tag_hp_zero',0x5F561C,{'health':0}),
        row('positive_post_tag_hp_negative',0x5F561C,{'health':-1}),
        row('lethal_post_tag_revived',0x5F5674,{'health':80},damage=100),
        row('half_tag_detached',0x5F561C,{'attached_tag':False}),
        row('first_tag_detached',0x5F56F6,{'attached_tag':False},damage=1),
        row('half_changes_strength_before_first',0x5F561C,{'strength':110}),
        row('half_restores_old_health_before_first',0x5F561C,{'health':100}),
        row('first_restores_old_health_inside_first_group',0x5F56F6,{'health':100},damage=1),
        row('half_combat_clears_alive',0x5F561C,{'alive':False}),
        row('half_anysource_clears_alive',0x5F5643,{'alive':False}),
        row('red_combat_clears_alive',0x5F5674,{'alive':False},damage=76),
        row('red_anysource_clears_alive',0x5F569B,{'alive':False},damage=76),
        row('first_combat_clears_alive',0x5F56F6,{'alive':False},damage=1),
        row('first_anysource_null_clears_alive',0x5F5719,{'alive':False},damage=1),
        row('first_anysource_source_clears_alive',0x5F5744,{'alive':False},damage=1),
        row('initial_alive_zero_half',alive=0),
        row('initial_alive_zero_red',alive=0,damage=76),
        row('lethal_kill_clears_alive',damage=100,callback='kill_clear_alive'),
        row('lethal_kill_restores_hp',0x5F57A0,{'health':80},damage=100),
        row('lethal_destroy_clears_alive',damage=100,callback='destroy_clear_alive'),
        row('surviving_six_clears_alive',0x5F57E0,{'alive':False},damage=1,initial_hp=80),
        row('surviving_fortyfour_clears_alive',0x5F580C,{'alive':False},damage=1,initial_hp=80),
        row('surviving_six_sets_health_zero',0x5F57E0,{'health':0},damage=1,initial_hp=80),
    ]


def damage_event_receiver_controls():
    """One observer/receiver owner supplies18 ordinary and23 mutation controls."""
    observation=DamageEventsObservation();controls=[]
    for name,hp,strength,damage,source,kind in (
        ('first_small_source',100,100,1,True,'unit'),
        ('first_small_no_source',100,100,1,False,'unit'),
        ('first_half_source',100,100,51,True,'unit'),
        ('first_quarter_source',100,100,76,True,'unit'),
        ('first_lethal_source',100,100,100,True,'unit'),
        ('first_half_no_source',100,100,51,False,'unit'),
        ('first_quarter_no_source',100,100,76,False,'unit'),
        ('half_equal_post',100,100,50,True,'unit'),
        ('half_equal_previous',50,100,1,True,'unit'),
        ('red_equal_post',26,100,1,True,'unit'),
        ('red_equal_previous',25,100,1,True,'unit'),
        ('red_cross',26,100,2,True,'unit'),
        ('odd_half_cross',50,101,1,True,'unit'),
        ('nonfirst_small',80,100,1,True,'unit'),
        ('healing',80,100,-1,True,'unit'),
        ('zero',100,100,0,True,'unit'),
        ('building_first_half',100,100,51,True,'building'),
        ('building_first_quarter',100,100,76,True,'building')):
        observed=observation.run(dict(name=name,initial_hp=hp,strength=strength,
            damage=damage,source=source,kind=kind))
        controls.append(dict(name=name,**observed['native_receiver'],
            rng_before_ref='full_seed31',rng_after_ref='full_seed31',
            three_full_rng_states_compared_equal=True,observed_rng_detach_entries=[]))
    mutations=[observation.run(case) for case in damage_event_mutation_inputs()]
    return controls,mutations,observation.finish()


def damage_event_techno_anger_controls():
    """Execute the real post-Object math, stopping before House504790's body.

    Uses the same receiver VM, actual Unit type getter and Type.GetCost711EB0.
    No division/multiply/ftol/delta result is supplied by Python.
    """
    from unicorn.x86_const import UC_X86_REG_FPSW
    observation=DamageEventsObservation();f=observation.fixture;u=f.u
    source_house,victim_house=SCRATCH+0xC000,SCRATCH+0xD000
    spans=((0x701DFD,0x701E6D),(0x711EB0,0x711EB7),(0x7C5F00,0x7C5F2C))
    original=[bytes(u.mem_read(a,b-a)) for a,b in spans]
    trace=[]
    watched={0x701E12:'type_for_cost',0x701E32:'type_for_strength',
        0x701E4A:'virtual_get_cost',0x711EB0:'type_get_cost_body',
        0x7C5F00:'original_ftol',0x701E68:'call_house_anger',
        0x65C780:'rng_next',0x65C7E0:'rng_ranged',0x7258D0:'detach',0x6E4F60:'tag_detach'}
    def observe(_u,pc,size,data):
        if pc in watched:trace.append(dict(pc=f'0x{pc:08X}',kind=watched[pc]))
    hook=u.hook_add(UC_HOOK_CODE,observe);rows=[]
    for name,packet,strength,cost,object_result in (
        ('ordinary_post_object',51,100,700,2),
        ('live_strength_after_tag',51,110,700,2),
        ('negative_healing_packet',-7,100,700,0),
        ('zero_packet',0,100,700,0),
        ('zero_strength_masked_divide',51,0,700,1),
        ('nonintegral_ratio',1,3,10,1),
        ('large_positive_dwords',2147483647,1,2147483647,1),
        ('large_negative_packet',-2147483648,3,2147483647,0)):
        f.reset('unit',49,strength)
        # Real original UnitType vtable; GetCost reads native Type+610.
        # Owners are explicit opaque supplied pointers, not House constructors.
        u.mem_write(TYPE,dwords(0x7F6218));u.mem_write(TYPE+0x610,dwords(cost))
        u.mem_write(PACKET,dwords(packet));u.mem_write(OBJ+0x21C,dwords(victim_house))
        u.mem_write(SOURCE+0x21C,dwords(source_house));u.mem_write(SP+0xD4,dwords(SOURCE))
        timer_offsets=(0x174,0x1E0)
        for offset in timer_offsets:u.mem_write(OBJ+offset,dwords(29,17,90))
        timers_before={f'0x{offset:X}':list(struct.unpack('<3i',u.mem_read(OBJ+offset,12)))
                       for offset in timer_offsets}
        for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,OBJ),
                          (UC_X86_REG_EBX,PACKET),(UC_X86_REG_EAX,object_result)):
            u.reg_write(reg,value)
        before=observation.rng_reader(observation);assert before==observation.seeded
        control_before=u.reg_read(UC_X86_REG_FPCW);status_before=u.reg_read(UC_X86_REG_FPSW)
        trace.clear()
        run_checked(u,0x701DFD,0x504790,count=1000,
            required_addresses=(0x701E12,0x701E32,0x701E4A,0x711EB0,0x701E5C,0x701E68))
        actual_sp=u.reg_read(UC_X86_REG_ESP)
        return_pc,delta,peer=struct.unpack('<IiI',u.mem_read(actual_sp,12))
        timers_after={f'0x{offset:X}':list(struct.unpack('<3i',u.mem_read(OBJ+offset,12)))
                      for offset in timer_offsets}
        after=observation.rng_reader(observation)
        assert before==after and timers_before==timers_after
        assert not any(row['kind'] in ('rng_next','rng_ranged','detach','tag_detach') for row in trace)
        assert original==[bytes(u.mem_read(a,b-a)) for a,b in spans]
        f.unchanged()
        rows.append(dict(name=name,input=dict(damage_packet=packet,strength=strength,cost=cost,
            entry='0x00701DFD',stop_before='0x00504790',object_result=object_result,
            registers=dict(eax=object_result,esi=OBJ,ebx=PACKET,esp=SP),
            stack_source_offset='0xD4',source_object=SOURCE,source_owner=source_house,
            victim_object=OBJ,victim_owner=victim_house,type_object=TYPE,
            type_vtable='0x007F6218',type_getter_entry=f'0x{f.read(VT+0x84):08X}',
            cost_getter_entry='0x00711EB0',timers_before=timers_before),
            output=dict(delta=delta,house_receiver=u.reg_read(UC_X86_REG_ECX),
                source_house_argument=peer,return_pc=f'0x{return_pc:08X}',stack_pointer=actual_sp,
                damage_packet_after=f.read(PACKET),timers_after=timers_after,
                fp_control_before=control_before,fp_control_after=u.reg_read(UC_X86_REG_FPCW),
                fp_status_before=status_before,fp_status_after=u.reg_read(UC_X86_REG_FPSW),
                native_entries=list(trace)),
            rng_before_ref='receiver_rng.full_seed31',rng_after_ref='receiver_rng.full_seed31',
            three_full_rng_states_compared_equal=True))
    u.hook_del(hook)
    return rows,observation.finish()


def generate_building_zero_health():
    fixture = Fixture()
    rows = []
    for name, health, constructor_frame, frame, timer in (
        ('default_zero_at_frame0', 0, 0, 0, None),
        ('default_zero_same_frame', 0, 73, 73, None),
        ('default_zero_next_frame', 0, 73, 74, None),
        ('positive_health_continues', 1, 73, 74, None),
        ('pending_timer_waits', 0, 73, 103, (100, 8)),
        ('elapsed_timer_terminal', 0, 73, 108, (100, 8)),
        ('stopped_zero_terminal', 0, 73, 108, (-1, 0)),
        ('stopped_positive_waits', 0, 73, 108, (-1, 5)),
        ('stopped_negative_waits', 0, 73, 108, (-1, -1)),
        ('running_negative_terminal', 0, 73, 100, (100, -1)),
        ('default_signed_frame_wrap', 0, 2147483647, 2147483648, None),
    ):
        row = fixture.building_zero_health(health, constructor_frame, frame, timer=timer)
        row['name'] = name
        rows.append(row)
    return rows


def building_zero_health_metadata():
    fixture = Fixture()
    result = provenance(
        scope='Eleven bounded original Building constructor timer/default-byte and AI Health==0 terminal-gate controls; ordered terminal callees are explicit observation sinks, not lifecycle or crew parity.',
        assumptions=[
            'An already admitted Building, original copied Building vtable, valid supplied House, empty eight fire-Anim slots, and signed Health are input state. Full constructor, authored-health parser/Unlimbo, earlier AI and scheduling are excluded.',
            'Constructor43B762..43B795 executes original frame-start/duration0/sourceNULL/sample0 writes with EBX0 from original43B754. Separate original43B95A..43B960 clears NoSurvivor+6E0. Constructor frame and current frame are explicit signed DWORD inputs.',
            'Original440042..4400EA samples health and executes the exact-zero health gate, signed timer subtraction/comparison and ordered terminal calls. EBP=-1 is the preserved prior set by both upstream AI branches43FB73/43FBFD.',
            'Original-vtable Limbo445880, UnInit5F65F0, direct SpawnSurvivors442D90 and AfterDestruction441F60 are recorded sinks returning0; none mutates the supplied Building. The observed terminal route does not establish occupancy removal, survivor admission/RNG/smudges, deferred destruction or rubble behavior.',
            'No general damage receiver, Tag damage/kill callbacks, destruction anims, debris or death weapon executes in this bounded original gate. SpawnSurvivors is an independent existing owner and is not assumed a no-op when its source argument is NULL.',
            'Stopped negative-duration controls retain native nonzero remaining behavior; positive-health control exits before timer consumption. No host calculation supplies the stop/call sequence outputs.',
        ],
        substitutions=['Four terminal call boundaries are supplied observation sinks with exact thiscall stack cleanup; caller instruction bytes are unchanged.'],
        entry_points={'constructor_timer_slice':0x43B762, 'constructor_no_survivor':0x43B95A,
                      'building_health_sample':0x440042, 'building_zero_hp_gate':0x440072,
                      'limbo_call':0x4400C5, 'spawn_survivors_call':0x4400D4,
                      'uninit_call':0x4400DD, 'after_destruction_call':0x4400E5})
    # Selected upstream instructions establish fixture register priors; these
    # bytes are static evidence and are not part of the executed narrow slices.
    audit = ((0x43B750, 0x43B762), (0x43FB6D, 0x43FB7A),
             (0x43FBF7, 0x43FC00), (0x44FB67, 0x44FBD2))
    for key, ranges in (('original_code', BUILDING_ZERO_HP_RANGES),
                        ('audit_bytes_not_executed', audit)):
        result[key] = {f'{a:08X}..{b:08X}': dict(
            hex=(raw := bytes(fixture.u.mem_read(a, b-a))).hex(),
            sha256=hashlib.sha256(raw).hexdigest()) for a, b in ranges}
    return result


if __name__ == '__main__':
    import sys
    args = sys.argv[1:]
    if '--building-zero-health' in args:
        args.remove('--building-zero-health')
        finish_vectors(generate_building_zero_health,
            Path(__file__).with_name('building_zero_health.json'),
            provenance=building_zero_health_metadata, argv=args)
    elif '--damage-tags' in args:
        args.remove('--damage-tags')
        finish_vectors(generate_damage_tags,Path(__file__).with_name('object_damage_tags.json'),
            provenance=damage_tag_metadata,argv=args)
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,argv=args)
