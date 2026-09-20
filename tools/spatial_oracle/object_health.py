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
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP,
    UC_X86_REG_EIP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    load_image, run_checked, finish_vectors, provenance,
    STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC,
)
from tools.spatial_oracle.map_queries import dwords

OBJ, TYPE, VT, PACKET, RULES = [SCRATCH+n for n in (0, 0x2000, 0x4000, 0x5000, 0x6000)]
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
        self.period = None

    def write(self, u, _access, address, size, value, _data):
        if address in (OBJ+0x6C, OBJ+0x70):
            assert size == 4
            self.writes.append(dict(instruction=f'{u.reg_read(UC_X86_REG_EIP):08X}',
                                   field='actual' if address == OBJ+0x6C else 'estimated',
                                   value=i32(value)))

    def code(self, u, address, _size, _data):
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
            assert slot == 0xE0 and arg == 0
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

    def receiver(self, current, strength, damage, *, kind='unit', can_c4=True, alive=1, callback='none'):
        self.reset(kind, current, strength, alive)
        self.u.mem_write(TYPE+0x1577, bytes((int(can_c4),)))
        self.callback_mode = callback
        for slot in (0x148, 0xE0, 0xDC):
            stub = SCRATCH+0x9000+slot
            self.u.mem_write(VT+slot, dwords(stub))
            self.stubs[stub] = slot
        self.u.mem_write(PACKET, dwords(damage))
        # int*damage, distance0, null warhead/source, ignoreDefenses1,
        # preventPassengerEscape1, null sourceHouse. Forced path never dereferences warhead.
        self.u.mem_write(SP, dwords(RET_MAGIC, PACKET, 0, 0, 0, 1, 1, 0))
        run_checked(self.u, 0x5F5390, RET_MAGIC, count=2000, required_addresses=(0x5F53A1,))
        assert self.u.reg_read(UC_X86_REG_ESP) == SP+32
        assert self.read(OBJ+0x70) == ESTIMATE
        self.unchanged()
        return dict(input=dict(kind=kind, current=current, strength=strength, damage=damage,
                               can_c4=can_c4, alive=alive, callback=callback),
                    output=dict(actual=self.read(OBJ+0x6C), estimated=self.read(OBJ+0x70),
                                damage_packet=self.read(PACKET), result=i32(self.u.reg_read(UC_X86_REG_EAX)),
                                alive=int(self.u.mem_read(OBJ+0x90, 1)[0]), writes=self.writes,
                                callbacks=self.callbacks, branches=self.branches))

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


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
