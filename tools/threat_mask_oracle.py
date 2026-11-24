"""Native references for Greatest_Threat's mask pieces.

Run python -m tools.threat_mask_oracle --check (or explicit --write).
Rust consumer: src/sim/combat/greatest_threat_mask_oracle_tests.rs.

Sections, each executed in a fresh emulator per case:
- flags: Greatest_Threat 0x6F8DF0's flags word 0x6F8F29..0x6F8F76 from the
  effective mask.
- quarry_terms: Evaluate_Candidate 0x6F7CA0 after its VHP transform,
  0x6F875F..0x6F88BF (All-To-Hunt, the quarry's building terms and rejects),
  then its final acceptance 0x6F8928..0x6F8948.
- enemy_bonus: Calculate_Threat_Score 0x70CD10's SpecialThreatValue term and
  EnemyHouseThreatBonus, 0x70CEDC..0x70CF1D.
- cutoff: Evaluate_Candidate's distance cutoff 0x6F8049..0x6F8178, from the
  candidate's vt+0x54 through both GetCoords, Sqrt_Approx and ftol to the
  range-argument test.
- disguise: Evaluate_Candidate's disguise arm 0x6F84B1..0x6F854B, from
  IsDisguisedTo and DetectDisguise= through the candidate's +0x1EC/+0x1F4
  blink, IsControlledByHuman and the Scenario RandomRanged(0, 99) against
  DisabledDisguiseDetectionPercent= by the scanner house's +0x184.

Control flow the rows do not vary (the ThreatAvoidance factor between the two
quarry slices, dormant without a Supress= weapon) rests on instruction
reading; see src/sim/combat/greatest_threat.rs.
"""
from pathlib import Path
import random
import struct

from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
                               UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.ai_base_building_oracle import FAKE, FRAME, RULES, STUBS, Emu, u32
from tools.native_oracle import (NATIVE_FPCW, STACK_BASE, STACK_SIZE, finish_vectors,
                                 provenance, run_checked)

SP = STACK_BASE + STACK_SIZE - 0x1000
SCANNER = FAKE + 0x400000
CANDIDATE = FAKE + 0x401000
CANDIDATE_VTABLE = FAKE + 0x402000
TYPE = FAKE + 0x404000
HOUSES = FAKE + 0x410000
HOUSE_SIZE = 0x6000
ARRAY = FAKE + 0x430000
WEAPON = FAKE + 0x431000
HOUSE_ARRAY = 0xA8022C
STUB_WHAT_AM_I = STUBS + 0x200
STUB_OCCUPANTS = STUBS + 0x210
STUB_WEAPON = STUBS + 0x220
BUILDING, UNIT = 6, 0x28


def i32(value):
    return struct.unpack('<i', u32(value))[0]


def bits(value):
    return struct.pack('>d', value).hex()


class MaskEmu(Emu):
    def __init__(self):
        super().__init__()
        self.uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.uc.reg_write(UC_X86_REG_ESP, SP)

    def write8(self, address, value):
        self.uc.mem_write(address, bytes([value & 0xFF]))

    def write_double(self, address, value):
        self.uc.mem_write(address, struct.pack('<d', value))

    def read_double_bits(self, address):
        return bytes(self.uc.mem_read(address, 8))[::-1].hex()


# ---------------------------------------------------------------- flags

FLAGS, FLAGS_END = 0x6F8F29, 0x6F8F76


def flags():
    rows = []
    masks = [0, 1, 2, 3, 4, 5, 8, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x800, 0x1000,
             0x2000, 0x4000, 0x8000, 0x10000, 0x4008, 0x4010, 0x4009, 0xB8, 0xB9, 0xBC,
             0x1BA60, 0x18200, 0xFFFFFFFF]
    rng = random.Random(0x6F8F29)
    masks += [rng.getrandbits(18) for _ in range(40)]
    for mask in masks:
        emu = MaskEmu()
        emu.uc.reg_write(UC_X86_REG_EBX, mask)
        emu.uc.reg_write(UC_X86_REG_EDI, 0)
        emu.write32(SP + 0x70, mask)
        run_checked(emu.uc, FLAGS, FLAGS_END, count=40)
        rows.append(dict(mask=mask, flags=emu.read32(SP + 0x14)))
    return rows


# ---------------------------------------------------------------- quarry_terms

TERMS, TERMS_CONTINUE = 0x6F875F, 0x6F88BF
TERMS_REJECTS = (0x6F894F, 0x6F888F)
FINISH, FINISH_ACCEPT, FINISH_REJECT = 0x6F8928, 0x6F8939, 0x6F8948


def quarry_terms_row(*, mask, score, building, power, max_occupants, needs_engineer,
                     factory, can_be_occupied, occupants, armed, bias, enemy, enemy_owns):
    """`enemy`: whether the scanner's house has a current enemy (index 1)."""
    emu = MaskEmu()
    uc = emu.uc
    scanner_house, enemy_house, other_house = (HOUSES + i * HOUSE_SIZE for i in range(3))
    emu.write32(HOUSE_ARRAY, ARRAY)
    emu.write32(ARRAY + 4, enemy_house)
    emu.write32(SCANNER + 0x21C, scanner_house)
    emu.write8(scanner_house + 0x249, int(bias))
    emu.write32(scanner_house + 0x5600, 1 if enemy else -1)
    emu.write32(CANDIDATE, CANDIDATE_VTABLE)
    emu.write32(CANDIDATE + 0x21C, enemy_house if enemy_owns else other_house)
    emu.write32(CANDIDATE + 0x520, TYPE)
    for slot, stub in ((0x2C, STUB_WHAT_AM_I), (0x408, STUB_OCCUPANTS), (0x3F4, STUB_WEAPON)):
        emu.write32(CANDIDATE_VTABLE + slot, stub)
    what_am_i = BUILDING if building else UNIT
    emu.hook(STUB_WHAT_AM_I, lambda _e: what_am_i, 0)
    emu.hook(STUB_OCCUPANTS, lambda _e: occupants, 0)
    emu.write32(WEAPON, 0x12345678 if armed else 0)
    emu.hook(STUB_WEAPON, lambda _e: WEAPON, 0)
    emu.write32(TYPE + 0xEE0, power)
    emu.write32(TYPE + 0x1580, max_occupants)
    emu.write8(TYPE + 0x1552, int(needs_engineer))
    emu.write32(TYPE + 0xEB8, 3 if factory else 0)
    emu.write8(TYPE + 0x157B, int(can_be_occupied))
    score_address = SP + 0x200
    emu.write32(score_address, score)
    emu.write32(SP + 0x40, mask)
    emu.write32(SP + 0x4C, what_am_i)
    emu.write32(SP + 0x54, TYPE)
    uc.reg_write(UC_X86_REG_EDI, SCANNER)
    uc.reg_write(UC_X86_REG_ESI, CANDIDATE)
    uc.reg_write(UC_X86_REG_EBP, score_address)
    result = None
    if run_checked(uc, TERMS, (TERMS_CONTINUE,) + TERMS_REJECTS, count=200) == TERMS_CONTINUE:
        uc.reg_write(UC_X86_REG_ESP, SP)
        if run_checked(uc, FINISH, (FINISH_ACCEPT, FINISH_REJECT), count=20) == FINISH_ACCEPT:
            result = i32(uc.reg_read(UC_X86_REG_EAX))
    return dict(mask=mask, score=score, building=building, power=power,
                max_occupants=max_occupants, needs_engineer=needs_engineer, factory=factory,
                can_be_occupied=can_be_occupied, occupants=occupants, armed=armed, bias=bias,
                enemy=enemy, enemy_owns=enemy_owns, result=result)


def quarry_terms():
    rng = random.Random(0x6F875F)
    masks = (0, 0x800, 0x8000, 0x10000, 0x1000, 0x2000, 0x1800, 0x9000, 0x3000, 0x18800)
    scores = (-7, 0, 1, 2, 999, 100000, 0x7FFFFFFF, -0x80000000)
    rows = []
    for number in range(400):
        rows.append(quarry_terms_row(
            mask=masks[number % len(masks)], score=rng.choice(scores),
            building=rng.random() < 0.8, power=rng.choice((-100, 0, 1, 50, 2200000)),
            max_occupants=rng.choice((-1, 0, 5, 3000000)), needs_engineer=rng.random() < 0.5,
            factory=rng.random() < 0.5, can_be_occupied=rng.random() < 0.5,
            occupants=rng.choice((0, 3)), armed=rng.random() < 0.5, bias=rng.random() < 0.3,
            enemy=rng.random() < 0.7, enemy_owns=rng.random() < 0.5))
    return rows


# ---------------------------------------------------------------- enemy_bonus

BONUS, BONUS_END = 0x70CEDC, 0x70CF1D


def enemy_bonus_row(*, score, coefficient, special, bonus, enemy, enemy_owns):
    emu = MaskEmu()
    uc = emu.uc
    scanner_house, enemy_house, other_house = (HOUSES + i * HOUSE_SIZE for i in range(3))
    emu.write32(SCANNER + 0x21C, scanner_house)
    emu.write32(scanner_house + 0x5600, 1 if enemy else -1)
    emu.write32(enemy_house + 0x30, 1)
    emu.write32(other_house + 0x30, 2)
    emu.write32(CANDIDATE + 0x21C, enemy_house if enemy_owns else other_house)
    emu.write_double(TYPE + 0x2C0, special)
    emu.write_double(RULES + 0x1090, bonus)
    emu.write_double(SP + 0x10, score)
    emu.write_double(SP + 0x20, coefficient)
    uc.reg_write(UC_X86_REG_EAX, TYPE)
    uc.reg_write(UC_X86_REG_EDI, SCANNER)
    uc.reg_write(UC_X86_REG_ESI, CANDIDATE)
    run_checked(uc, BONUS, BONUS_END, count=20)
    return dict(score_bits=bits(score), coefficient_bits=bits(coefficient),
                special_bits=bits(special), bonus_bits=bits(bonus), enemy=enemy,
                enemy_owns=enemy_owns, result_bits=emu.read_double_bits(SP + 0x10))


def enemy_bonus():
    rng = random.Random(0x70CEE6)
    rows = []
    for _ in range(60):
        rows.append(enemy_bonus_row(
            score=rng.choice((0.0, -3.5, 12345.678, 1e300, rng.uniform(-1e6, 1e6))),
            coefficient=rng.choice((0.0, 1.0, 0.1, rng.uniform(-10, 10))),
            special=rng.choice((0.0, 1.0, 5.0, rng.uniform(0, 100))),
            bonus=rng.choice((0.0, 400.0, 0.1, -50.0, 1e308)),
            enemy=rng.random() < 0.8, enemy_owns=rng.random() < 0.6))
    return rows


# ---------------------------------------------------------------- cutoff

CUTOFF = 0x6F8049
CUTOFF_ADMIT = (0x6F8176, 0x6F8178)
CUTOFF_REJECT = (0x6F80E8, 0x6F894F)
SCANNER_VTABLE = FAKE + 0x403000
STUB_HIGH_FLYING = STUBS + 0x230
STUB_SCANNER_COORDS = STUBS + 0x240
STUB_CANDIDATE_COORDS = STUBS + 0x250


def cutoff_row(*, high_flying, scanner, candidate, range_):
    emu = MaskEmu()
    uc = emu.uc
    emu.write32(SCANNER, SCANNER_VTABLE)
    emu.write32(SCANNER_VTABLE + 0x48, STUB_SCANNER_COORDS)
    emu.write32(CANDIDATE, CANDIDATE_VTABLE)
    emu.write32(CANDIDATE_VTABLE + 0x48, STUB_CANDIDATE_COORDS)
    emu.write32(CANDIDATE_VTABLE + 0x54, STUB_HIGH_FLYING)

    def coords(values):
        def answer(e):
            out = e.arg(0)
            e.uc.mem_write(out, struct.pack('<iii', *values))
            return out
        return answer

    emu.hook(STUB_SCANNER_COORDS, coords(scanner), 4)
    emu.hook(STUB_CANDIDATE_COORDS, coords(candidate), 4)
    emu.hook(STUB_HIGH_FLYING, lambda _e: int(high_flying), 0)
    emu.write32(SP + 0x48, range_)
    uc.reg_write(UC_X86_REG_EDI, SCANNER)
    uc.reg_write(UC_X86_REG_ESI, CANDIDATE)
    stop = run_checked(uc, CUTOFF, CUTOFF_ADMIT + CUTOFF_REJECT, count=400,
                       required_addresses=[0x4CAC40, 0x7C5F00])
    return dict(high_flying=high_flying, scanner=list(scanner), candidate=list(candidate),
                range=range_, distance=i32(uc.reg_read(UC_X86_REG_EBP)),
                admitted=stop in CUTOFF_ADMIT)


def cutoff():
    base = (20 * 256 + 128, 30 * 256 + 64, 208)
    offsets = [(1000, 0, 0), (1000, 0, 416), (0, -1000, -416), (255, 0, 0), (0, 255, 0),
               (724, 724, 0), (724, 724, 208), (-3, 4, 0), (3000, 2000, 832), (0, 0, 0),
               (0, 0, 312), (46340, 0, 0), (-128, -127, 104)]
    rows = []
    for dx, dy, dz in offsets:
        candidate = (base[0] + dx, base[1] + dy, base[2] + dz)
        for high_flying in (False, True):
            probe = cutoff_row(high_flying=high_flying, scanner=base, candidate=candidate,
                               range_=1)
            distance = probe['distance']
            for range_ in sorted({distance, distance - 1, distance + 1, 0, -1, -256, 1}):
                rows.append(cutoff_row(high_flying=high_flying, scanner=base,
                                       candidate=candidate, range_=range_))
    return rows


# ---------------------------------------------------------------- disguise

DISGUISE = 0x6F84B1
DISGUISE_ACCEPT, DISGUISE_REJECT = 0x6F8551, 0x6F894F
SCANNER_TYPE = FAKE + 0x406000
PERCENTS = RULES + 0x2000
STUB_DISGUISED_TO = STUBS + 0x260
STUB_SCANNER_TYPE = STUBS + 0x270
IS_CONTROLLED_BY_HUMAN = 0x50B730


def disguise_row(*, disguised, detects, start, duration, frame, human, difficulty, roll):
    emu = MaskEmu()
    uc = emu.uc
    scanner_house = HOUSES
    emu.write32(SCANNER, SCANNER_VTABLE)
    emu.write32(SCANNER_VTABLE + 0x84, STUB_SCANNER_TYPE)
    emu.write32(SCANNER + 0x21C, scanner_house)
    emu.write32(scanner_house + 0x184, difficulty)
    emu.write8(SCANNER_TYPE + 0xD31, int(detects))
    emu.write32(CANDIDATE, CANDIDATE_VTABLE)
    emu.write32(CANDIDATE_VTABLE + 0xC8, STUB_DISGUISED_TO)
    emu.write32(CANDIDATE + 0x1EC, start)
    emu.write32(CANDIDATE + 0x1F4, duration)
    emu.write32(FRAME, frame)
    emu.write32(RULES + 0xE14, PERCENTS)
    for index, percent in enumerate((15, 5, 2)):
        emu.write32(PERCENTS + 4 * index, percent)
    houses = []
    emu.hook(STUB_DISGUISED_TO, lambda e: houses.append(e.arg(0)) or int(disguised), 4)
    emu.hook(STUB_SCANNER_TYPE, lambda _e: SCANNER_TYPE, 0)
    emu.hook(IS_CONTROLLED_BY_HUMAN, lambda e: int(human), 0)
    emu.draws([roll])
    uc.reg_write(UC_X86_REG_EDI, SCANNER)
    uc.reg_write(UC_X86_REG_ESI, CANDIDATE)
    stop = run_checked(uc, DISGUISE, (DISGUISE_ACCEPT, DISGUISE_REJECT), count=60)
    if houses != [scanner_house]:
        raise AssertionError(f'IsDisguisedTo asked {houses}')
    return dict(disguised=disguised, detects=detects, start=start, duration=duration,
                frame=frame, human=human, difficulty=difficulty, roll=roll,
                draws=[event[1:4] for event in emu.events if event[0] == 'draw'],
                admitted=stop == DISGUISE_ACCEPT)


def disguise():
    row = dict(disguised=True, detects=False, start=100, duration=15, frame=100, human=False,
               difficulty=1, roll=5)
    rows = [disguise_row(**{**row, 'disguised': disguised, 'detects': detects, 'roll': 99})
            for disguised, detects in ((False, False), (False, True), (True, True))]
    blinks = ((100, 15), (100, 0), (-1, 7), (-1, 0), (-1, -3), (90, 15), (85, 15),
              (101, 15), (0x7FFFFFF0, 40), (100, -5))
    for start, duration in blinks:
        rows.append(disguise_row(**{**row, 'start': start, 'duration': duration, 'human': True,
                                    'roll': 0}))
        for difficulty in (0, 1, 2):
            percent = (15, 5, 2)[difficulty]
            for roll in (percent, percent + 1):
                rows.append(disguise_row(**{**row, 'start': start, 'duration': duration,
                                            'difficulty': difficulty, 'roll': roll}))
    rows += [disguise_row(**{**row, 'roll': roll}) for roll in (0, 99)]
    return rows


def generate():
    return dict(flags=flags(), quarry_terms=quarry_terms(), enemy_bonus=enemy_bonus(),
                cutoff=cutoff(), disguise=disguise())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=('Greatest_Threat\'s flags word over masks; Evaluate_Candidate\'s All-To-Hunt '
               'and quarry terms over synthetic candidates, then its final acceptance; '
               'Calculate_Threat_Score\'s SpecialThreatValue term and EnemyHouseThreatBonus; '
               'Evaluate_Candidate\'s distance cutoff over coordinate offsets, the candidate\'s '
               'high-flying answer and positive, zero and negative ranges; Evaluate_Candidate\'s '
               'disguise arm over disguise, DetectDisguise=, blink timers, attacker control, '
               'difficulty and roll.'),
        assumptions=['x87 control word 0x0E7F (PC53, chop), the harness default.',
                     'Fixture objects carry only the fields the slices read.'],
        substitutions=['The candidate\'s WhatAmI (vt+0x2C), occupant count (vt+0x408) and '
                       'current weapon (vt+0x3F4) answer from the case.',
                       'For the cutoff, the scanner\'s and the candidate\'s GetCoords (vt+0x48) '
                       'write the case\'s coordinates and the candidate\'s vt+0x54 answers its '
                       'high-flying flag; Sqrt_Approx and ftol run.',
                       'Between the quarry slices, the ThreatAvoidance factor '
                       '(0x6F88BF..0x6F8926) is skipped: 1.0, no score change.',
                       'For the disguise arm, the candidate\'s IsDisguisedTo (vt+0xC8), the '
                       'scanner\'s GetTechnoType (vt+0x84), IsControlledByHuman 0x50B730 and '
                       'RandomRanged 0x65C7E0 answer from the case; the draw\'s stream and range '
                       'are recorded. DisabledDisguiseDetectionPercent= holds retail 15,5,2.'],
        entry_points={'flags': FLAGS, 'quarry_terms': TERMS, 'finish': FINISH,
                      'enemy_bonus': BONUS, 'cutoff': CUTOFF, 'disguise': DISGUISE},
    ))
