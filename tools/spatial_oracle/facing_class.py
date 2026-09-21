"""Original FacingClass constructors, signed ROT, setters and timed queries.

Each history retains the actual 24-byte native object between calls. No native
instructions or receivers are replaced. Padding and the unused timer word are
excluded from observations; directions, rate, timer epoch/duration and returns
are retained. This primitive corpus does not certify aircraft mission steering.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)

FACING, ARG, OUTPUT = SCRATCH, SCRATCH + 0x100, SCRATCH + 0x200
SP = STACK_BASE + STACK_SIZE - 0x1000


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def execute(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)

    def call(entry, *args):
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, FACING)
        run_checked(u, entry, RET_MAGIC, count=10000)
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4 + 4 * len(args)
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
        observations.append(dict(destination=destination & 0xFFFF, previous=previous & 0xFFFF,
                                 start=start, duration=duration, rate=rate & 0xFFFF,
                                 animated=animated, rotating=rotating, result=result))
    return dict(input=case, observations=observations)


def generate():
    rows = []

    def history(rot, start, actions, *, initial=0x4000, rate_constructor=False):
        operations = [dict(kind=kind, frame=(start + delta) & 0xFFFFFFFF, value=value)
                      for delta, kind, value in actions]
        rows.append(execute(dict(rot=rot, start=start, initial=initial,
                                 rate_constructor=rate_constructor, operations=operations)))

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


if __name__ == '__main__':
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
