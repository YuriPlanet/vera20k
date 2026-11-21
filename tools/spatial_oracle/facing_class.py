"""Original FacingClass constructors, signed ROT, setters and timed queries.

Each history retains the actual 24-byte native object between calls. No native
instructions or receivers are replaced. Padding and the unused timer word are
excluded from observations; directions, rate, timer epoch/duration and returns
are retained. This primitive corpus does not certify aircraft mission steering.

With `--ares-dll`, a few histories also sample Ares 3.0's own copy of Current
(Ares.dll 0x10009CC0) on the same object, into facing_class_ares.json.
"""
import argparse
from pathlib import Path
import struct
import sys

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, map_pe32_image, provenance, run_checked,
)

FACING, ARG, OUTPUT = SCRATCH, SCRATCH + 0x100, SCRATCH + 0x200
# Ares.dll's preferred base is the shared stack's, so its histories move the stack.
ARES_STACK_BASE = 0x40000000


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def execute(case, ares=None):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    stack = STACK_BASE
    if ares is not None:
        map_pe32_image(u, ares, image_base=0x10000000, image_size=0xE0000, header_size=0x400)
        stack = ARES_STACK_BASE
    u.mem_map(stack, STACK_SIZE)
    sp = stack + STACK_SIZE - 0x1000
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)

    def call(entry, *args):
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, FACING)
        run_checked(u, entry, RET_MAGIC, count=10000)
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + 4 * len(args)
        return u.reg_read(UC_X86_REG_EAX)

    def frame(value):
        u.mem_write(0xA8ED84, dwords(value))

    frame(case['start'])
    if case['rate_constructor']:
        call(0x4C91E0, case['rot'])
    else:
        call(0x4C91C0)
        call(0x4C9680, case['rot'])
    u.mem_write(ARG, dwords(case['initial']))
    call(0x4C9300, ARG)
    observations = []
    for operation in case['operations']:
        frame(operation['frame'])
        result = None
        if operation['kind'] == 'rate':
            call(0x4C9680, operation['value'])
        elif operation['kind'] in ('set', 'snap'):
            u.mem_write(ARG, dwords(operation['value']))
            result = bool(call(0x4C9220 if operation['kind'] == 'set' else 0x4C9300, ARG) & 0xFF)
        else:
            assert operation['kind'] == 'sample'
        call(0x4C93D0, OUTPUT)
        animated = struct.unpack('<H', u.mem_read(OUTPUT, 2))[0]
        rotating = bool(call(0x4C9480))
        destination, previous, start, _unused, duration, rate = struct.unpack(
            '<6I', u.mem_read(FACING, 24))
        observation = dict(destination=destination & 0xFFFF, previous=previous & 0xFFFF,
                           start=start, duration=duration, rate=rate & 0xFFFF,
                           animated=animated, rotating=rotating, result=result)
        if ares is not None:
            call(0x10009CC0, OUTPUT)
            observation['ares_animated'] = struct.unpack('<H', u.mem_read(OUTPUT, 2))[0]
            assert struct.unpack('<6I', u.mem_read(FACING, 24))[:3] == (destination, previous, start)
        observations.append(observation)
    return dict(input=case, observations=observations)


def histories(ares=None):
    rows = []

    def history(rot, start, actions, *, initial=0x4000, rate_constructor=False):
        operations = [dict(kind=kind, frame=(start + delta) & 0xFFFFFFFF, value=value)
                      for delta, kind, value in actions]
        rows.append(execute(dict(rot=rot, start=start, initial=initial,
                                 rate_constructor=rate_constructor, operations=operations), ares))
    return rows, history


def generate():
    rows, history = histories()

    rates = [-2147483648, -513, -257, -256, -255, -129, -128, -127, -1,
             0, 1, 5, 32, 126, 127, 128, 255, 256, 2147483647]
    ordinary = [(0, 'set', 0xC000), (1, 'sample', 0), (2, 'set', 0xC000),
                (3, 'set', 0xE123), (4, 'sample', 0), (5, 'snap', 0x8123),
                (6, 'set', 0xFF00), (7, 'set', 0x100), (140, 'sample', 0)]
    for rot in rates:
        for constructor in (False, True):
            history(rot, 100, ordinary, rate_constructor=constructor)
        # Changing a live timer's rate must retain its epoch and duration.
        history(5, 100, [(0, 'set', 0xC000), (1, 'rate', rot), (2, 'sample', 0),
                         (3, 'set', 0xE123), (4, 'rate', 5), (5, 'sample', 0),
                         (6, 'snap', 0x100), (7, 'set', 0x4000)])
    for start in (0x7FFFFFFE, 0xFFFFFFFD, 0xFFFFFFFF):
        history(5, start, ordinary)
    history(32, 100, [(0, 'set', 0x3FFF), (0, 'snap', 0), (1, 'sample', 0)], initial=0)
    return rows


def generate_ares(ares):
    rows, history = histories(ares)
    # A half turn (signed diff -0x8000), a quarter turn each way, a retarget
    # mid-turn, a one-step turn, a sub-step turn, an instant rate and a
    # frame wrap, each sampled from its first frame to past its end.
    samples = [(delta, 'sample', 0) for delta in (1, 2, 6, 11, 12, 13)]
    history(5, 100, [(0, 'set', 0xC000)] + samples + [(24, 'sample', 0), (25, 'sample', 0)])
    history(5, 100, [(0, 'set', 0x8000)] + samples, initial=0x4000)
    history(5, 100, [(0, 'set', 0x0000)] + samples, initial=0x4000)
    history(7, 100, [(0, 'set', 0x5432), (3, 'sample', 0), (4, 'set', 0xF00D),
                     (5, 'sample', 0), (9, 'sample', 0), (17, 'sample', 0)], initial=0x1234)
    history(32, 100, [(0, 'set', 0x3FFF), (0, 'sample', 0), (1, 'sample', 0)], initial=0)
    history(1, 100, [(0, 'set', 0x4080), (0, 'sample', 0), (1, 'sample', 0)])
    history(-1, 100, [(0, 'set', 0xC000), (1, 'sample', 0)])
    history(5, 0x7FFFFFFE, [(0, 'set', 0x1000), (1, 'sample', 0), (2, 'sample', 0),
                            (3, 'sample', 0), (40, 'sample', 0)])
    return rows


if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--ares-dll', type=Path)
    known, rest = parser.parse_known_args()
    if known.ares_dll is not None:
        from tools.rules_oracle.ares_prerequisites import ARES_SHA256, identified_bytes
        ares = identified_bytes(known.ares_dll, ARES_SHA256)
        finish_vectors(lambda: generate_ares(ares), Path(__file__).with_name('facing_class_ares.json'),
                       argv=rest, provenance=lambda: provenance(
            entry_points={'set': 0x4C9220, 'snap': 0x4C9300, 'current': 0x4C93D0,
                          'ares_current': 0x10009CC0},
            assumptions=['Same setup as the stock histories; Ares 3.0 (ARES_SHA256) mapped at 0x10000000 without its loader.',
                         'After each stock Current, Ares Current reads the same retained object and global frame; it writes nothing else.'],
            substitutions=[],
            scope='8 histories: half and quarter turns both ways, mid-turn retarget, one-step, sub-step and instant turns, and frame wrap. Ares Current only; the SHP and AlphaImage draws that read it are not executed.',
        ))
        sys.exit()
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'constructor': 0x4C91C0, 'rate_constructor': 0x4C91E0,
                      'set_rot': 0x4C9680, 'set': 0x4C9220, 'snap': 0x4C9300,
                      'current': 0x4C93D0, 'is_rotating': 0x4C9480},
        assumptions=['Original constructors followed by original snap to the supplied initial heading.',
                     'Original 24-byte object retained between calls; supplied global frame counter, including signed and unsigned wrap boundaries.',
                     'Only direction words, rate word, timer epoch/duration and public query/setter returns are compared; padding and the unused timer word are excluded.'],
        substitutions=[],
        scope='61 histories: nineteen signed ROT inputs, both constructors, active-timer rate changes, retarget/snap/equality, direction wrap, frame wrap and timer epoch -1. Does not establish any complete locomotor or combat mission.',
    ))
