"""Native references for the house self-heal arithmetic (issue #1219).

Run python -m tools.selfheal_oracle --check (or explicit --write).

Four leaf functions, each executed in fresh state through tools.native_oracle.call:

- predicate: `HouseClass::HasInfSelfHeal` 0x0050D9C0 and `HasUnitSelfHeal`
  0x0050D9D0 鈥?the two `return count > 0` readers of `House+0x164` / `House+0x168`.
- step: `HouseClass::GetInfSelfHealStep` 0x0050D9E0 and `GetUnitSelfHealStep`
  0x0050D9F0 鈥?`Rules[+0x34] * House[+0x164]` and `Rules[+0x3C] * House[+0x168]`.
  The Rules pointer is written at 0x008871E0 and the *other* amount field holds a
  sentinel, so every row also shows which Rules field the arm does **not** read.

The `TechnoClass::AI_Update` arms and the three `BuildingClass` lifecycle sites
(0x00446382, 0x004459AE, 0x00448AC8) are not executed here: they need a populated
TechnoClass/BuildingClass with owner, type and stack arguments, and their only
arithmetic beyond these leaves is an add, a subtract and a clamp-at-zero that the
Rust regression tests cover. Their instruction bytes were read directly.

Consumers: src/sim/world/techno_ai_selfheal_oracle_tests.rs pins the Rust port
against these rows.

Prerequisites: `tools/requirements-test.txt` (Unicorn + Capstone) and a supported
retail `gamemd.exe`; see tools/native_oracle.md.
"""
from pathlib import Path
import random
import struct

from tools.native_oracle import call, finish_vectors, provenance

RULES_PTR = 0x8871E0
FAKE = 0x20000000  # tools.native_oracle.SCRATCH
HOUSE = FAKE + 0x1000  # inside the harness scratch page
RULES = FAKE + 0x4000

HG_INF, HG_UNIT = 0x164, 0x168
FG_INF, FG_UNIT = 0x16CC, 0x16CE
SENTINEL = 0x5A5A5A5A

HAS_INF, HAS_UNIT = 0x50D9C0, 0x50D9D0
STEP_INF, STEP_UNIT = 0x50D9E0, 0x50D9F0

U32 = (1 << 32) - 1


def w32(value):
    return struct.pack('<I', value & U32)


def s32(value):
    return struct.unpack('<i', w32(value))[0]


def house(infantry, units):
    """A fixture house carrying only the two self-heal counts."""
    return {HOUSE + HG_INF: w32(infantry), HOUSE + HG_UNIT: w32(units)}


def predicate():
    """`count > 0` over the whole signed range, per arm, with the other arm zero."""
    rows = []
    for arm, func, own, other in (
        ('infantry', HAS_INF, HG_INF, HG_UNIT),
        ('units', HAS_UNIT, HG_UNIT, HG_INF),
    ):
        for raw in (0, 1, 2, 100, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF):
            # The arm reads its own counter; the other one holds a large value
            # so a row that reported it would be visible immediately.
            writes = {HOUSE + own: w32(raw), HOUSE + other: w32(12345)}
            hit = call(func, ecx=HOUSE, writes=writes)
            rows.append(dict(arm=arm, count=s32(raw), mask=raw,
                             result=int(hit['eax'] & 0xFF != 0),
                             other_counter_read=False,
                             house_offset=own, address=func))
    return rows


def step():
    """`Rules[amount] * House[count]` with the other Rules amount sentinel-set."""
    rows = []
    rng = random.Random(0x50D9E0)
    fixed = [(0, 0), (0, 5), (1, 1), (5, 2), (20, 3), (255, 4), (0xFFFF, 0xFFFF),
             (0x7FFF, 0x10000), (0x7FFFFFFF, 2), (0xFFFFFFFF, 1)]
    random_pairs = [(rng.randrange(0, 0x10000), rng.randrange(0, 0x10000))
                    for _ in range(40)]
    for arm, func, own, amount_offset, other_offset in (
        ('infantry', STEP_INF, HG_INF, 0x34, 0x3C),
        ('units', STEP_UNIT, HG_UNIT, 0x3C, 0x34),
    ):
        cases = fixed + random_pairs
        for amount, count in cases:
            writes = {HOUSE + HG_INF: w32(count if own == HG_INF else SENTINEL),
                      HOUSE + HG_UNIT: w32(count if own == HG_UNIT else SENTINEL),
                      RULES_PTR: w32(RULES),
                      RULES + amount_offset: w32(amount),
                      RULES + other_offset: w32(SENTINEL)}
            hit = call(func, ecx=HOUSE, writes=writes)
            rows.append(dict(arm=arm, amount=s32(amount), count=s32(count),
                             step=s32(hit['eax']), house_offset=own,
                             rules_amount_offset=amount_offset,
                             rules_amount_field=f'Rules+0x{amount_offset:X}',
                             address=func))
    return rows


def generate():
    return dict(predicate=predicate(), step=step())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'),
                   provenance=lambda: provenance(
                       scope=('HouseClass::HasInfSelfHeal / HasUnitSelfHeal and '
                              'GetInfSelfHealStep / GetUnitSelfHealStep: the count '
                              'predicate and the count x amount step of the Tech '
                              'Hospital and Tech Machine Shop self-heal.'),
                       assumptions=[
                           'The fixture house carries only House+0x164 and +0x168.',
                           'The fixture Rules block is reached through 0x008871E0 and '
                           'carries only the two amount fields plus a sentinel.',
                       ],
                       substitutions=[
                           'The other counter and the other Rules amount field hold '
                           'sentinels, so a row that read the wrong one differs.',
                           'TechnoClass::AI_Update\'s two arms and the three '
                           'BuildingClass lifecycle sites are read from instructions, '
                           'not executed here.',
                       ],
                       entry_points={'predicate_infantry': HAS_INF,
                                     'predicate_units': HAS_UNIT,
                                     'step_infantry': STEP_INF,
                                     'step_units': STEP_UNIT}))
