"""Native references for RandomRanged draws scaled onto [0, 1] (issue #584).

Run python -m tools.unit_draw_oracle --check (or explicit --write).

gamemd scales a `RandomRanged(0, 0x7FFFFFFE)` draw with `FILD draw;
FMUL [0x007E3570]`. Each row runs original instructions from the one after
the RandomRanged call, with the draw supplied as its return value in EAX (the
scenario RNG has its own goldens), under the game's control word 0x0E7F.

`gates`: a caller's probability gate, run to the two targets of its branch.
Each compares the scaled draw against `band` with FCOMP, then `TEST AH,1`
passes it when C0 is set: a product below the band, or an unordered compare.

- `bridge_outer`: CellClass::BlowUpBridge 0x0047DE59..0x0047DE72, band
  `[0x007E4F58]`;
- `bridge_metallic`: 0x0047DF48..0x0047DF61, band `[0x007E1738]`;
- `scorch`: AnimClass::Middle 0x00425092..0x004250AB, band `[0x007E1738]`;
- `spark`: TechnoClass::AI 0x006FAE29..0x006FAE40, band the
  Condition{Red,Yellow}SparkingProbability double at [ESP+0x1C], supplied;
- `crew`: 0x0073820E..0x0073822D, band Rules+0x5C0 (`CrewEscape=`), supplied.

Draws straddle each band's boundary: 0, 1, the top draw, and the first draw
whose exact product reaches the band with its two neighbours.

`fill_earliest`: the AI unit chooser 0x004FEE59..0x004FEE89 compares
`FillEarliestTeamProbability` (Rules+0x13F4, the house's difficulty +0x184)
times `[0x007E3808]` against the scaled draw with FCOMPP, `TEST AH,0x41`;
`earliest` is the fall-through that takes the earliest team's type.

`crate_timer`: `CrateSlot__PlaceOverlayAndInitTimer @ 0x004A17C0` sets a
placed crate's slot timer from `[General] CrateRegen=` (Rules+0x1678):

- `0x004A1854..0x004A188A`: lower = regen * 450.0 (`[0x007E5D78]`) and
  upper = regen * `[0x007E44B8]`, each stored as a double;
- `0x004A1893..0x004A18B5`: (upper - lower) * (draw * `[0x007E3570]`) + lower,
  then Math::ftol (`0x007C5F00`);
- `0x004A18BA..0x004A18C5`: the slot's start frame (`[0x00A8ED84]`), the high
  dword of upper and the duration.

Consumers: `util::native_random` (`unit_draw_threshold`, `minutes_timer_frames`
through `sim::crates::state`) and the gate owners' tests: bridge debris, the
scorch coin, the damage Spark thresholds, crew escape and the AI unit chooser.

Prerequisites: `tools/requirements-test.txt` (Unicorn + Capstone) and a supported
retail `gamemd.exe`; see tools/native_oracle.md.
"""
from fractions import Fraction
from math import ceil, inf, nan
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI,
    UC_X86_REG_ESP, UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    NATIVE_FPCW, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE, OracleError,
    finish_vectors, load_image, provenance, run_checked,
)

RULES_PTR = 0x8871E0
FRAME = 0xA8ED84
RULES = SCRATCH + 0x1000
SLOT = SCRATCH + 0x100
CELL = SCRATCH + 0x200
HOUSE = SCRATCH + 0x300
FILL = SCRATCH + 0x800
FRAME_BASE = STACK_BASE + STACK_SIZE - 0x1000

UNIT_SCALE = 0x7E3570
PERCENT_SCALE = 0x7E3808
DRAW_TOP = 0x7FFFFFFE

# Each gate starts at the instruction after its RandomRanged call, which
# stores the draw (EAX) to `slot` for the FILD, and ends at the two targets of
# its branch. `band` is an image address, or where a supplied band goes.
GATES = {
    'bridge_outer': dict(entry=0x47DE59, band=0x7E4F58, passes=0x47DE78, fails=0x47E031),
    'bridge_metallic': dict(entry=0x47DF48, band=0x7E1738, passes=0x47DF63, fails=0x47DFBF),
    'scorch': dict(entry=0x425092, band=0x7E1738, passes=0x4250AD, fails=0x4250D1),
    'spark': dict(entry=0x6FAE29, band='esp+0x1c', passes=0x6FAE46, fails=0x6FAEE0),
    'crew': dict(entry=0x73820E, band='rules+0x5c0', passes=0x738233, fails=0x73838A),
}
FILL_EARLIEST = dict(entry=0x4FEE59, earliest=0x4FEE8B, most_needed=0x4FEEA5)


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


# Reader values (the constructor defaults 0.02 and 0.01, f32 values widened by
# ReadDouble) and every band magnitude the threshold conversion distinguishes.
SPARK_BANDS = (0.02, 0.01, f32(1e-30), 2.0 ** -149, 2.0 ** -126, 2.0 ** -1074,
               2.0 ** -73, 2.0 ** -72, 1e-10, 1e-9, 0.5, 1 - 2.0 ** -53, 1.0, 1.5,
               0.0, -0.0, -1.0, inf, nan)
CREW_BANDS = (0.5, 0.25, nan)
FILL_PERCENTS = (100, 50, 33, 1, 0, -1, 101)

REGENS = (3.0, 1.0, 0.5, 7.3, 1.0e20, -1.0e20)
DRAWS = (0, 1, 0x3FFFFFFF, 0x40000000, 123456789, 2000000000, 0x7FFFFFFD, 0x7FFFFFFE)
FRAMES = (-1, 77, -0x80000000)


def machine():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_write(RULES_PTR, struct.pack('<I', RULES))
    uc.reg_write(UC_X86_REG_ESP, FRAME_BASE)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    return uc


def image_double(address):
    return struct.unpack('<d', bytes(machine().mem_read(address, 8)))[0]


def bits(value):
    return f"0x{struct.unpack('<Q', struct.pack('<d', value))[0]:016x}"


def chop53(value):
    """`value` truncated toward zero to a 53-bit significand, as PC53/chop."""
    if value == 0:
        return value
    magnitude = abs(value)
    exponent = magnitude.numerator.bit_length() - magnitude.denominator.bit_length()
    while Fraction(2) ** exponent > magnitude:
        exponent -= 1
    while Fraction(2) ** (exponent + 1) <= magnitude:
        exponent += 1
    unit = Fraction(2) ** (exponent - 52)
    return (magnitude // unit) * unit * (1 if value > 0 else -1)


def straddling_draws(scale, band):
    """0, 1, the top draw, and the first draw whose exact product reaches
    `band` with its neighbours; a NaN band has no boundary."""
    draws = {0, 1, DRAW_TOP}
    if band == band:
        if band <= 0:
            first = 0
        elif band == inf:
            first = DRAW_TOP + 1
        else:
            first = min(ceil(Fraction(band) / scale), DRAW_TOP + 1)
        draws |= {first - 1, first, first + 1}
    else:
        draws.add(0x3FFFFFFF)
    return sorted(draw for draw in draws if 0 <= draw <= DRAW_TOP)


def gate_passes(name, band, draw):
    gate = GATES[name]
    uc = machine()
    if gate['band'] == 'esp+0x1c':
        uc.mem_write(FRAME_BASE + 0x1C, struct.pack('<d', band))
    elif gate['band'] == 'rules+0x5c0':
        uc.mem_write(RULES + 0x5C0, struct.pack('<d', band))
    uc.reg_write(UC_X86_REG_EAX, draw)
    reached = run_checked(uc, gate['entry'], (gate['passes'], gate['fails']),
                          context=dict(gate=name, band=bits(band), draw=draw))
    return reached == gate['passes']


def gate_rows(scale):
    rows = []
    for name, gate in GATES.items():
        if isinstance(gate['band'], int):
            bands = (image_double(gate['band']),)
        else:
            bands = SPARK_BANDS if name == 'spark' else CREW_BANDS
        for band in bands:
            for draw in straddling_draws(scale, band):
                rows.append(dict(gate=name, band=bits(band), draw=draw,
                                 passed=gate_passes(name, band, draw)))
    return rows


def fill_earliest_rows(scale):
    percent_scale = Fraction(image_double(PERCENT_SCALE))
    rows = []
    for percent in FILL_PERCENTS:
        band = chop53(percent * percent_scale)
        for draw in straddling_draws(scale, band):
            uc = machine()
            uc.mem_write(RULES + 0x13F4, struct.pack('<I', FILL))
            uc.mem_write(FILL, struct.pack('<i', percent))
            uc.mem_write(HOUSE + 0x184, struct.pack('<i', 0))
            uc.reg_write(UC_X86_REG_EBP, HOUSE)
            uc.reg_write(UC_X86_REG_EAX, draw)
            reached = run_checked(
                uc, FILL_EARLIEST['entry'],
                (FILL_EARLIEST['earliest'], FILL_EARLIEST['most_needed']),
                context=dict(fill_percent=percent, draw=draw))
            rows.append(dict(fill_percent=percent, draw=draw,
                             earliest=reached == FILL_EARLIEST['earliest']))
    return rows


def timer_words(regen, draw, frame):
    uc = machine()
    uc.mem_write(RULES + 0x1678, struct.pack('<d', regen))
    uc.mem_write(FRAME, struct.pack('<i', frame))
    uc.reg_write(UC_X86_REG_EDI, CELL)
    uc.reg_write(UC_X86_REG_ESI, SLOT)
    uc.reg_write(UC_X86_REG_ECX, 0)
    run_checked(uc, 0x4A1854, 0x4A188E, context=dict(regen=regen, draw=draw))
    # RandomRanged is __thiscall with two stack arguments: it returns the draw
    # in EAX and pops both.
    uc.reg_write(UC_X86_REG_ESP, FRAME_BASE)
    uc.reg_write(UC_X86_REG_EAX, draw)
    run_checked(uc, 0x4A1893, 0x4A18C8, context=dict(regen=regen, draw=draw))
    start, aux, duration = struct.unpack('<iIi', bytes(uc.mem_read(SLOT, 12)))
    return dict(regen=regen, draw=draw, frame=frame,
                start_frame=start, aux=aux, duration=duration)


def generate():
    scale = Fraction(image_double(UNIT_SCALE))
    if scale != Fraction((1 << 30) + 1, 1 << 61):
        raise OracleError(f'[0x007E3570] is {scale}, not (2^30 + 1) * 2^-61')
    crate_timer = []
    for regen in REGENS:
        for index, draw in enumerate(DRAWS):
            crate_timer.append(timer_words(regen, draw, FRAMES[index % len(FRAMES)]))
    return dict(gates=gate_rows(scale), fill_earliest=fill_earliest_rows(scale),
                crate_timer=crate_timer)


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'),
                   provenance=lambda: provenance(
                       scope=('RandomRanged(0, 0x7FFFFFFE) draws scaled by [0x007E3570]: '
                              'five FCOMP probability gates and the AI unit chooser\'s FCOMPP '
                              'gate at straddling draws, and the start frame, upper-bound high '
                              'dword and duration a placed crate slot stores from CrateRegen=.'),
                       assumptions=[
                           'Control word 0x0E7F (53-bit precision, chop), as Math::ftol '
                           'leaves it.',
                           'The fixture Rules block is reached through 0x008871E0 and '
                           'carries only the field each row reads: CrewEscape at +0x5C0, '
                           'the FillEarliestTeamProbability pointer at +0x13F4, CrateRegen '
                           'at +0x1678.',
                       ],
                       substitutions=[
                           'No RandomRanged call is executed: each row supplies the draw '
                           'as its return value in EAX; the crate timer pops its two '
                           'arguments.',
                           'The Spark band is written to [ESP+0x1C] where TechnoClass::AI '
                           'keeps the Rules double it selected.',
                           'The chooser\'s house is scratch memory with difficulty 0 at '
                           '+0x184; the crate slot and cell pointer are scratch memory.',
                       ],
                       entry_points={**{name: gate['entry'] for name, gate in GATES.items()},
                                     'fill_earliest': FILL_EARLIEST['entry'],
                                     'crate_timer_bounds': 0x4A1854,
                                     'crate_timer_duration_and_slot_writes': 0x4A1893}))
