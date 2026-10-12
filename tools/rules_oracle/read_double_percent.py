"""Original CCINIClass::ReadDouble percent product and its chopped consumers.

A, `reader`: ReadDouble 0x005283D0 executes whole on a cached INI value. The
building_body_rules fixture supplies the section, index and entry objects with
the native key CRC (0x004A1DE0); the CRT floating-scanner initializer 0x007C8F5E
runs first. sscanf "%f" (0x007CA530), the widening FSTP, strchr('%') (0x007CAF30)
and `fld qword; fmul qword [0x007E3808]; fstp qword` (0x0052857A..0x00528584)
are original. Every row runs under the game's FPCW 0x0E7F: Math__ftol 0x007C5F00
loads [0x00822D80] = 0x0E7F and never restores it. Token and string rows also run
under round-to-nearest 0x027F as a control. An FSTP stub at the return address
stores the returned ST0. Rows: every distinct `%` token in retail rulesmd.ini (a
lexical scan chose these inputs; scalar keys and Verses alike), integer percents
0..1000, and fractional, signed, spaced, doubled and plain strings.

B, `prone`: InfantryClass::ReceiveDamage 0x00517FA0 executes from its entry
through the prone head (0x00517FC1..0x00517FEF) and the InfDeath test to
0x00518016, before FootClass::ReceiveDamage (0x00518042). The warhead's +0xF8
double is group A's native result for the same string. InfDeath is 0.

C, `c4`: InfantryClass::Per_Cell_Process's C4 plant on a zeroed building, from
0x0051A5A7 to the Flash call's push (EAX of ftol(C4Delay * 900.0 * 0.5) at
0x0051A5CD; the vt+0x148 call is not executed), then 0x0051A5D6..0x0051A60B, the
C4Timer store (Frame [0x00A8ED84], TimeLeft ftol(C4Delay * 900.0)) and
C4AppliedBy. Rules+0x1750 holds group A's native result for the string, or what
the constructor's stores at 0x006675F0 leave there when the key is absent.
"""
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.native_oracle import (NATIVE_SHA256, RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE,
                                 call, finish_vectors, load_image, provenance, run_checked)
from tools.spatial_oracle.building_body_rules import (ENTRY, INDEX, INI, RAW, SECTION, SP,
                                                       TYPE, Fixture)

GAME_FPCW = 0x0E7F
NEAREST_FPCW = 0x027F
KEY = SCRATCH + 0x7000
ST0_SLOT = RET_MAGIC + 0x100
# Non-restoring control-word writers: Math__ftol, _control87, _fpreset.
FLDCW_SITES = (0x7C5F2F, 0x7CBF41, 0x7CBF67)
RETAIL_TOKENS = [
    '0%', '1%', '2%', '3%', '5%', '6%', '8%', '10%', '15%', '20%', '25%', '28%', '30%',
    '35%', '40%', '45%', '50%', '60%', '65%', '70%', '75%', '80%', '85%', '90%', '100%',
    '125%', '150%', '200%', '250%', '300%', '350%', '400%', '500%', '600%']
STRINGS = [
    '2.5%', '0.1%', '12.5%', '99.9%', '33.3%', '-70%', '-0%', '7e1%', '70%%', '70 %',
    ' 70% ', '\t80%', '80%junk', '1e-5%', '3.4e38%', '1e39%', '0.7', '70', '1.25', '-12.5']
RETAIL_PRONE = ['30%', '50%', '70%', '80%', '100%', '150%', '300%', '350%', '600%']
MODDED_PRONE = ['1%', '0%', '35%', '90%', '0.7', '-50%']
PRONE_DAMAGES = list(range(1, 201)) + [250, 500, 1000, 11000, 65535, 1_000_000]
# Products beyond the signed 32-bit range: Math__ftol stores a qword and the head
# keeps EAX, its low dword.
PRONE_EXTREMES = [('600%', 357_913_942), ('600%', 1_000_000_000), ('600%', 2_147_483_647),
                  ('350%', 2_147_483_647), ('150%', 2_147_483_647), ('300%', 1_431_655_766),
                  ('1e30', 1), ('3.4e38%', 100), ('1e39%', 1)]
INF = SCRATCH + 0x8000
WH = SCRATCH + 0x9000
DMG = SCRATCH + 0xA000
PRONE_SP = STACK_BASE + 0x80000
# None is the absent key. The others are modded inputs around retail `.03`.
C4_DELAYS = [None, '.03', '0.03', '.02', '.06', '0.1', '.5', '1', '2.5', '0', '-.03']
RULES = SCRATCH + 0xB000
BUILDING = SCRATCH + 0xD000
PLANTER = SCRATCH + 0xE000
C4_SP = STACK_BASE + 0x90000
C4_FRAME = 1234


def bits_hex(value):
    return f'{value:016x}'


class Reader(Fixture):
    def __init__(self):
        super().__init__()
        self.crc = {}
        # Write the stub before RET_MAGIC's page is first used as a stop address.
        self.u.mem_write(RET_MAGIC, b'\xdd\x1d' + struct.pack('<I', ST0_SLOT))
        self.write(SP, RET_MAGIC + 0x80)
        self.u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(self.u, 0x7C8F5E, RET_MAGIC + 0x80)
        self.events = []
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def observe(self, uc, address, _size, _data):
        if address == 0x52855D:
            self.events.append(('scan', uc.reg_read(UC_X86_REG_EAX)))
        elif address == 0x52857A:
            self.events.append(('percent',))
        elif address in FLDCW_SITES:
            self.events.append(('fldcw', address))

    def supply(self, key_name, raw):
        u = self.u
        if key_name not in self.crc:
            name = key_name.encode()
            self.crc[key_name] = call(0x4A1DE0, ecx=SCRATCH, stack_args=[SCRATCH + 0x100, len(name)],
                                      writes={SCRATCH: bytes(16), SCRATCH + 0x100: name})['eax']
        u.mem_write(KEY, key_name.encode() + b'\0')
        u.mem_write(INI, bytes(0x40))
        self.write(INI + 4, TYPE + 0x1F8)
        self.write(INI + 8, SECTION)
        self.write(SECTION + 0x2C, INDEX)
        self.write(SECTION + 0x30, 1)
        self.write(SECTION + 0x38, 1)
        self.write(SECTION + 0x3C, INDEX)
        self.write(INDEX, self.crc[key_name])
        self.write(INDEX + 4, ENTRY)
        self.write(ENTRY + 0x10, RAW)
        u.mem_write(RAW, raw.encode('ascii') + b'\0')

    def read(self, raw, fpcw=GAME_FPCW, key_name='ProneDamage'):
        """ReadDouble(section, key, 1.0) on a present value; returns the ST0 bits."""
        u = self.u
        self.supply(key_name, raw)
        u.mem_write(SP, struct.pack('<IIIQ', RET_MAGIC, TYPE + 0x1F8, KEY, 0x3FF0000000000000))
        u.mem_write(ST0_SLOT, bytes(8))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, INI)
        u.reg_write(UC_X86_REG_FPCW, fpcw)
        self.events = []
        run_checked(u, 0x5283D0, RET_MAGIC + 6, count=200_000,
                    required_addresses=(0x528558, RET_MAGIC))
        result = struct.unpack('<Q', u.mem_read(ST0_SLOT, 8))[0]
        assert u.reg_read(UC_X86_REG_FPCW) == fpcw, (raw, hex(u.reg_read(UC_X86_REG_FPCW)))
        assert not any(event[0] == 'fldcw' for event in self.events), (raw, self.events)
        assert self.events[0] == ('scan', 1), (raw, self.events)
        assert (('percent',) in self.events) == ('%' in raw), (raw, self.events)
        return result


def emulator():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    return u


class ProneHead:
    def __init__(self):
        self.u = emulator()

    def run(self, damage, multiplier_bits, prone=True, ignore_defenses=False):
        u = self.u
        u.mem_write(INF, bytes(0x800))
        u.mem_write(WH, bytes(0x200))
        u.mem_write(INF + 0x6DB, bytes([int(prone)]))
        u.mem_write(WH + 0xF8, struct.pack('<Q', multiplier_bits))
        u.mem_write(DMG, struct.pack('<i', damage))
        # stdcall: return, int* damage, distance, warhead, source, ignoreDefenses,
        # preventsPassengerEscape, sourceHouse.
        u.mem_write(PRONE_SP, struct.pack('<8I', RET_MAGIC, DMG, 0, WH, 0,
                                          int(ignore_defenses), 0, 0))
        u.reg_write(UC_X86_REG_ESP, PRONE_SP)
        u.reg_write(UC_X86_REG_ECX, INF)
        u.reg_write(UC_X86_REG_FPCW, GAME_FPCW)
        scaled = prone and damage > 0 and not ignore_defenses
        run_checked(u, 0x517FA0, 0x518016, count=10_000,
                    required_addresses=(0x517FC1, 0x517FF9) + ((0x517FDD, 0x517FE8) if scaled else ()))
        assert u.reg_read(UC_X86_REG_FPCW) == GAME_FPCW
        return struct.unpack('<i', u.mem_read(DMG, 4))[0]


class C4Plant:
    def __init__(self):
        self.u = emulator()

    def constructor_default(self):
        """C4Delay as RulesClass's constructor stores it (0x006675F0, 0x006675FA)."""
        u = self.u
        u.mem_write(RULES, bytes(0x2000))
        u.reg_write(UC_X86_REG_ESI, RULES)
        run_checked(u, 0x6675F0, 0x667604, count=10)
        return struct.unpack('<Q', u.mem_read(RULES + 0x1750, 8))[0]

    def run(self, delay_bits):
        """Returns the Flash argument and the C4Timer's TimeLeft."""
        u = self.u
        u.mem_write(RULES, bytes(0x2000))
        u.mem_write(RULES + 0x1750, struct.pack('<Q', delay_bits))
        u.mem_write(0x8871E0, struct.pack('<I', RULES))
        u.mem_write(0xA8ED84, struct.pack('<i', C4_FRAME))
        u.mem_write(BUILDING, bytes(0x800))
        u.reg_write(UC_X86_REG_EDI, BUILDING)
        u.reg_write(UC_X86_REG_ESI, PLANTER)
        u.reg_write(UC_X86_REG_ESP, C4_SP)
        u.reg_write(UC_X86_REG_FPCW, GAME_FPCW)
        run_checked(u, 0x51A5A7, 0x51A5CD, count=100, required_addresses=(0x51A5C8,))
        flash = struct.unpack('<i', struct.pack('<I', u.reg_read(UC_X86_REG_EAX)))[0]
        run_checked(u, 0x51A5D6, 0x51A60B, count=100, required_addresses=(0x51A5EE, 0x51A608))
        start, _clock, fuse = struct.unpack('<iIi', u.mem_read(BUILDING + 0x528, 12))
        applied_by = struct.unpack('<I', u.mem_read(BUILDING + 0x540, 4))[0]
        assert (start, applied_by, u.mem_read(BUILDING + 0x6DF, 1)[0]) == (C4_FRAME, PLANTER, 1)
        assert u.reg_read(UC_X86_REG_FPCW) == GAME_FPCW
        return flash, fuse


def reader_rows(reader):
    def row(raw):
        return {'raw': raw, 'bits': bits_hex(reader.read(raw)),
                'nearest_bits': bits_hex(reader.read(raw, fpcw=NEAREST_FPCW))}
    return {'retail_tokens': [row(raw) for raw in RETAIL_TOKENS],
            'percent_sweep': [bits_hex(reader.read(f'{p}%')) for p in range(1001)],
            'strings': [row(raw) for raw in STRINGS]}


def prone_rows(reader):
    head = ProneHead()
    rows = []
    for raw in RETAIL_PRONE + MODDED_PRONE:
        multiplier = reader.read(raw)
        rows.append({'raw': raw, 'retail': raw in RETAIL_PRONE, 'multiplier_bits': bits_hex(multiplier),
                     'results': [head.run(damage, multiplier) for damage in PRONE_DAMAGES]})
    extremes = [{'raw': raw, 'damage': damage, 'result': head.run(damage, reader.read(raw))}
                for raw, damage in PRONE_EXTREMES]
    seventy = reader.read('70%')
    gates = [{'case': case, 'damage': damage, 'prone': prone, 'ignore_defenses': ignore,
              'result': head.run(damage, seventy, prone=prone, ignore_defenses=ignore)}
             for case, damage, prone, ignore in (
                 ('not_prone', 10, False, False), ('ignore_defenses', 10, True, True),
                 ('zero_damage', 0, True, False), ('negative_damage', -10, True, False),
                 ('negative_damage_ignore_defenses', -10, True, True))]
    return {'damages': PRONE_DAMAGES, 'rows': rows, 'extremes': extremes, 'gates': gates}


def c4_rows(reader):
    plant = C4Plant()
    rows = []
    for raw in C4_DELAYS:
        bits = plant.constructor_default() if raw is None else reader.read(raw, key_name='C4Delay')
        flash, fuse = plant.run(bits)
        rows.append({'raw': raw, 'bits': bits_hex(bits), 'flash': flash, 'fuse': fuse})
    return rows


def generate():
    reader = Reader()
    return {'native_sha256': NATIVE_SHA256, 'reader': reader_rows(reader), 'prone': prone_rows(reader),
            'c4': c4_rows(reader)}


def metadata():
    return provenance(
        scope='Original ReadDouble percent product, the InfantryClass::ReceiveDamage prone head and the C4 plant\'s flash and fuse',
        assumptions=[
            'ReadDouble 0x005283D0 executes whole after the CRT floating-scanner initializer 0x007C8F5E. Every row asserts one sscanf assignment, the percent arm (0x0052857A) exactly when the string holds a percent sign, no control-word load inside the call and an unchanged FPCW.',
            'FPCW 0x0E7F is the game word: WinMain selects chop at 0x006BBFC1 and caches the word at 0x006BBFC9; Math__ftol 0x007C5F00 loads [0x00822D80] and never restores it. Rows under 0x027F are a rounding-mode control, not a game state.',
            'Retail tokens come from a lexical scan of retail rulesmd.ini and serve only as inputs; the native body produces every recorded value.',
            'The prone head runs from the real entry 0x00517FA0 to 0x00518016 with a zeroed InfantryClass (prone byte +0x6DB supplied) and a zeroed WarheadTypeClass (+0xF8 from the native reader, InfDeath +0x120 zero). FootClass::ReceiveDamage and everything after it are not executed.',
            'The C4 rows run InfantryClass::Per_Cell_Process 0x0051A5A7..0x0051A5CD and 0x0051A5D6..0x0051A60B with a zeroed building in EDI, a planter pointer in ESI, Frame 1234 and a zeroed RulesClass whose +0x1750 holds the native reader result or the constructor stores 0x006675F0..0x00667604. The Flash call (vt+0x148) between them, the gates of the arm before 0x0051A5A7 and the stop, rearm and Scatter of the planter after 0x0051A60B are not executed.'],
        substitutions=[
            'building_body_rules.Fixture supplies the cached INI section, index and entry objects; no physical INI load.',
            'An FSTP qword stub at the return address stores ReadDouble\'s ST0 result.'],
        entry_points={'read_double': 0x5283D0, 'crt_float_init': 0x7C8F5E, 'ini_crc': 0x4A1DE0,
                      'infantry_receive_damage': 0x517FA0, 'math_ftol': 0x7C5F00,
                      'c4_plant': 0x51A5A7, 'rules_ctor_c4_delay': 0x6675F0})


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
