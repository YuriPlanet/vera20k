"""Native references for the house wallet's money primitives.

Run python -m tools.house_money_oracle --check (or explicit --write).
Rust consumer: src/sim/economy.rs (tests).

Sections, each case executed in a fresh emulator on a house whose silo
storage (+0x2FC) is empty. The first four sections use capacity (+0x310)
200; fill_silos supplies a capacity per row:
- spend_money: HouseClass::Spend_Money 0x4F9790 over the cash arm, the
  shortfall arm (a negative balance included) and 32-bit wrap of the
  balance and the spending statistic (+0x2DC).
- add_credits: HouseClass::Add_Credits 0x4F9950.
- add_tiberium_credits: HouseClass::Add_Tiberium_Credits 0x4F9610, the
  refinery deposit: the balance (+0x30C) and the score (+0x54E8) after
  `amount` units of a tiberium worth `value` for a house whose
  HouseType IncomeMult (+0x148) is `income`. A row names the amount as
  VERA builds it, `bales` x `scale_ppm` / 1e6, and the IncomeMult as
  `income_ppm` / 1e6; both pass to the original as floats.
- available_money: HouseClass::Available_Money 0x4F6990 (the money
  interface's slot +0x18, called on House+0x24).
- fill_silos: original startup interior 0x684F02..0x684F7F with one
  synthetic house, the original money-interface vtable and the original
  money/storage/credit bodies. This does not execute map admission or
  House/Building constructors. The ending instruction is not executed.
"""
import struct
from pathlib import Path

from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EIP,
                               UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.ai_base_building_oracle import Emu, FAKE, HOUSE, HOUSE_TYPE, SCENARIO
from tools.native_oracle import (NATIVE_FPCW, OracleError, STACK_BASE, STACK_SIZE,
                                 finish_vectors, provenance, run_checked)

SPEND_MONEY = 0x4F9790
ADD_CREDITS = 0x4F9950
ADD_TIBERIUM_CREDITS = 0x4F9610
AVAILABLE_MONEY = 0x4F6990
AVAILABLE_STORAGE = 0x4F69D0
FILL_SILOS_BEGIN, FILL_SILOS_END = 0x684F02, 0x684F7F
HOUSE_MONEY_VTABLE = 0x7EA834
HOUSES, HOUSE_COUNT = 0xA8022C, 0xA80238
HOUSE_LIST = FAKE + 0x42000

TIBERIUMS = 0xB0F4EC
TIBERIUM_LIST = FAKE + 0x40000
TIBERIUM = FAKE + 0x41000

BALANCE = 0x30C
CAPACITY = 0x310
SPENT = 0x2DC
SCORE = 0x54E8
INCOME_MULT = 0x148
MONEY_INTERFACE = 0x24
STORAGE = 0x2FC

INT_MAX = 0x7FFFFFFF
INT_MIN = -0x80000000


def f32(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


def house(balance, *, spent=0, score=0, income_ppm=1_000_000):
    emu = Emu()
    emu.write32(HOUSE + 0x34, HOUSE_TYPE)
    emu.write32(HOUSE_TYPE + INCOME_MULT, f32(income_ppm / 1_000_000))
    emu.write32(HOUSE + BALANCE, balance)
    emu.write32(HOUSE + CAPACITY, 200)
    emu.write32(HOUSE + SPENT, spent)
    emu.write32(HOUSE + SCORE, score)
    return emu


def spend_money():
    rows = []
    for name, balance, spent, amount in (
        ('cash covers the amount', 1000, 0, 300),
        ('cash covers it exactly', 1000, 50, 1000),
        ('shortfall takes the balance', 1000, 0, 1500),
        ('nothing to take', 0, 7, 50),
        ('a negative balance is taken and zeroed', -200, 100, 50),
        ('a negative amount adds to the balance', 100, 0, -50),
        ('a negative amount within a negative balance', -200, 0, -300),
        ('the balance wraps', INT_MAX, 0, -1),
        ('the statistic wraps', 10, INT_MAX, 1),
    ):
        emu = house(balance, spent=spent)
        emu.invoke(SPEND_MONEY, ecx=HOUSE, args=(amount & 0xFFFFFFFF,))
        rows.append({'name': name, 'balance': balance, 'spent': spent, 'amount': amount,
                     'expected': {'balance': emu.read_i32(HOUSE + BALANCE),
                                  'spent': emu.read_i32(HOUSE + SPENT)}})
    return rows


def add_credits():
    rows = []
    for name, balance, amount in (
        ('adds', 100, 50),
        ('a negative amount subtracts past zero', 100, -500),
        ('wraps', INT_MAX, 1),
    ):
        emu = house(balance)
        emu.invoke(ADD_CREDITS, ecx=HOUSE, args=(amount & 0xFFFFFFFF,))
        rows.append({'name': name, 'balance': balance, 'amount': amount,
                     'expected': {'balance': emu.read_i32(HOUSE + BALANCE)}})
    return rows


def add_tiberium_credits():
    rows = []
    for name, balance, score, bales, scale_ppm, value, income_ppm in (
        ('a deposit', 500, 70, 40, 1_000_000, 25, 1_000_000),
        ('gems', 0, 0, 12, 1_000_000, 50, 1_000_000),
        ('a purifier bonus truncates once', 100, 3, 10, 250_000, 25, 1_000_000),
        ('three purifiers', 0, 0, 3, 750_000, 25, 1_000_000),
        ('IncomeMult 1.25', 10, 0, 7, 1_000_000, 25, 1_250_000),
        ('IncomeMult 0.5 with a bonus', 0, 0, 3, 250_000, 25, 500_000),
        ('negative sums truncate toward zero', -100, -20, 10, 250_000, 25, 1_000_000),
        ('the balance wraps', 0x7FFFFFF0, 0, 40, 1_000_000, 25, 1_000_000),
        ('IncomeMult 0.9 is a float', 0, 0, 40, 1_000_000, 25, 900_000),
        ('a deposit too small to pay still scores', 0, 0, 1, 1_000_000, 25, 10_000),
    ):
        emu = house(balance, score=score, income_ppm=income_ppm)
        emu.write32(TIBERIUMS, TIBERIUM_LIST)
        emu.write32(TIBERIUM_LIST, TIBERIUM)
        emu.write32(TIBERIUM + 0xB8, value)
        amount = f32(bales * scale_ppm / 1_000_000)
        emu.invoke(ADD_TIBERIUM_CREDITS, ecx=HOUSE, args=(amount, 0))
        rows.append({'name': name, 'balance': balance, 'score': score, 'bales': bales,
                     'scale_ppm': scale_ppm, 'value': value, 'income_ppm': income_ppm,
                     'expected': {'balance': emu.read_i32(HOUSE + BALANCE),
                                  'score': emu.read_i32(HOUSE + SCORE)}})
    return rows


def available_money():
    rows = []
    for name, balance, income_ppm in (
        ('the balance', 700, 1_000_000),
        ('a negative balance', -50, 1_250_000),
    ):
        emu = house(balance, income_ppm=income_ppm)
        answer = emu.invoke(AVAILABLE_MONEY, args=(HOUSE + MONEY_INTERFACE,))
        rows.append({'name': name, 'balance': balance, 'income_ppm': income_ppm,
                     'expected': struct.unpack('<i', struct.pack('<I', answer))[0]})
    return rows


def fill_silos():
    rows = []
    for name, enabled, balance, capacity, value, score in (
        ('false flag', False, 5000, 1, 25, 0),
        ('zero capacity', True, 5000, 0, 25, 0),
        ('negative capacity', True, 5000, -1, 25, 0),
        ('budget below Value', True, 24, 1, 25, 0),
        ('budget equals Value', True, 25, 1, 25, 0),
        ('budget is Value plus one', True, 26, 1, 25, 0),
        ('ordinary 5000 budget', True, 5000, 1, 25, 0),
        ('negative Value terminates after signed budget wrap', True, INT_MAX, 1, -1, 0),
        ('score wraps', True, 26, 1, 25, INT_MAX),
    ):
        spent, income_ppm = 7, 1_000_000
        emu = house(balance, spent=spent, score=score, income_ppm=income_ppm)
        uc = emu.uc
        emu.write32(HOUSE + CAPACITY, capacity)
        emu.write32(HOUSE + MONEY_INTERFACE, HOUSE_MONEY_VTABLE)
        emu.write32(HOUSES, HOUSE_LIST)
        emu.write32(HOUSE_COUNT, 1)
        emu.write32(HOUSE_LIST, HOUSE)
        uc.mem_write(SCENARIO + 0x34B2, bytes([enabled]))
        emu.write32(TIBERIUMS, TIBERIUM_LIST)
        emu.write32(TIBERIUM_LIST, TIBERIUM)
        emu.write32(TIBERIUM + 0xB8, value)
        # Numeric fixture priors, not a native constructor/startup history.
        for slot in range(4):
            emu.write32(HOUSE + STORAGE + slot * 4, 0)
        if (emu.read32(HOUSE_MONEY_VTABLE + 0x18) != AVAILABLE_MONEY or
                emu.read32(HOUSE_MONEY_VTABLE + 0x1C) != AVAILABLE_STORAGE):
            raise OracleError('original House money-interface vtable differs')
        code = [(address, bytes(uc.mem_read(address, size))) for address, size in (
            (FILL_SILOS_BEGIN, FILL_SILOS_END - FILL_SILOS_BEGIN),
            (AVAILABLE_MONEY, 0x3A), (AVAILABLE_STORAGE, 0x23),
            (ADD_TIBERIUM_CREDITS, 0x57), (0x6C9650, 0x30), (0x7C5F00, 0x3E),
        )]
        calls = {'available_money': AVAILABLE_MONEY,
                 'available_storage': AVAILABLE_STORAGE,
                 'add_tiberium_credits': ADD_TIBERIUM_CREDITS}
        for event, address in calls.items():
            emu.mark(address, event)
        sp = STACK_BASE + STACK_SIZE - 0x1000
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        uc.reg_write(UC_X86_REG_EAX, SCENARIO)
        uc.reg_write(UC_X86_REG_EBP, 0)
        uc.reg_write(UC_X86_REG_ESI, 0)
        run_checked(uc, FILL_SILOS_BEGIN, FILL_SILOS_END,
                    required_addresses=(FILL_SILOS_BEGIN,), context={'case': name})
        if uc.reg_read(UC_X86_REG_ESP) != sp:
            raise OracleError('FillSilos interior left an unexpected stack')
        if uc.reg_read(UC_X86_REG_EIP) != FILL_SILOS_END or any(
                bytes(uc.mem_read(address, len(before))) != before for address, before in code):
            raise OracleError('FillSilos boundary or original-code readback differs')
        counts = {event: emu.events.count(event) for event in calls}
        budget = struct.unpack('<i', struct.pack('<I', uc.reg_read(UC_X86_REG_ESI)))[0]
        rows.append(dict(name=name, enabled=enabled, balance=balance, score=score,
                         spent=spent, capacity=capacity, value=value, income_ppm=income_ppm,
                         expected=dict(balance=emu.read_i32(HOUSE + BALANCE),
                                       score=emu.read_i32(HOUSE + SCORE),
                                       spent=emu.read_i32(HOUSE + SPENT),
                                       capacity=emu.read_i32(HOUSE + CAPACITY),
                                       amount_bits=[emu.read32(HOUSE + STORAGE + slot * 4)
                                                    for slot in range(4)],
                                       budget=budget if counts['available_money'] else None,
                                       calls=counts)))
    return rows


def generate():
    return dict(spend_money=spend_money(), add_credits=add_credits(),
                add_tiberium_credits=add_tiberium_credits(),
                available_money=available_money(), fill_silos=fill_silos())


def metadata():
    record = provenance(
        scope=('HouseClass::Spend_Money, Add_Credits, Add_Tiberium_Credits and '
               'Available_Money on a house with empty silo storage. Synthetic fixtures '
               'only; the silo-drain arm of Spend_Money is not reached.'),
        assumptions=['The house holds no silo storage (+0x2FC) and a capacity (+0x310) of '
                     '200, so Spend_Money never drains buildings and '
                     'Update_Silo_Damage_Frames compares two empty fill ratios and returns.'],
        substitutions=[],
        entry_points={'spend_money': SPEND_MONEY, 'add_credits': ADD_CREDITS,
                      'add_tiberium_credits': ADD_TIBERIUM_CREDITS,
                      'available_money': AVAILABLE_MONEY},
    )
    record['fill_silos'] = dict(
        scope='Original 0x684F02 through the unexecuted 0x684F7F interior boundary only.',
        entry_points={'begin': f'0x{FILL_SILOS_BEGIN:08X}',
                      'end_before': f'0x{FILL_SILOS_END:08X}',
                      'available_storage': f'0x{AVAILABLE_STORAGE:08X}'},
        assumptions=[
            'One synthetic non-null House in A8022C/A80238; EAX is a supplied Scenario '
            'with +34B2 flag. No map admission, House/Building constructor or full startup runs.',
            'All four House +2FC amount words are explicitly zero. Balance, score, spent, '
            'capacity, first Tiberium Value and HouseType IncomeMult are supplied scalar priors.',
            'House +24 uses original vtable 7EA834. Original 4F6990, 4F69D0, 4F9610, '
            '6C9650 and 7C5F00 execute without selected-query or credit substitutions.',
            'The inherited Emu CRT/allocation transport is not entered by this interior. '
            'No RNG, timers, detachment, placement or nonempty-storage history is claimed.',
            'Positive Values terminate normally. The sole negative Value is -1 with INT_MAX '
            'budget, terminating after one signed SUB wrap. Zero and other potentially '
            'nonterminating custom Values are not executed.',
        ],
        observations='Signed cash, score, spent, capacity and local ESI budget; four raw amount '
                     'words and native query/credit call counts. Budget is null when no money '
                     'query ran. Original code is read back unchanged after every row.',
        substitutions=[],
    )
    return record


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
