"""Checked native arc solver domain and failure observations.

Supplied scalar/register inputs call the original angle and word solvers. Native
failure return values remain observations; execution errors abort the whole run.
"""
import itertools
from pathlib import Path
import struct

from tools.native_oracle import SCRATCH, call, finish_vectors, provenance


def words(value):
    return list(struct.unpack('<II', struct.pack('<d', value)))


def generate():
    rows = []
    for distance, height, speed, gravity, mode in itertools.product(
            [0, 1, 500], [-200, -1, 0, 1], [0, 1, 100], [-6., 0., 1., 6.], [0, 1]):
        row = dict(range=distance, height=height, speed=speed, gravity=gravity, mode=mode)
        for address, label, size in [(0x48A9D0, 'angle', 8), (0x48A8D0, 'word', 4)]:
            result = call(
                address, ecx=mode, edx=speed,
                stack_args=[distance, height & 0xFFFFFFFF, *words(gravity), SCRATCH],
                writes={SCRATCH: b'\xcd' * 8}, dumps={'out': (SCRATCH, size)},
            )
            row[label + '_ok'] = result['eax'] & 255
            row[label + '_raw'] = result['dumps']['out']
            if result['eax'] & 255:
                row[label] = struct.unpack(
                    '<d' if size == 8 else '<I', bytes.fromhex(result['dumps']['out']),
                )[0]
        rows.append(row)
    return rows


def metadata():
    return provenance(
        scope='288 supplied arc-domain cases, each executing original angle48A9D0 and word48A8D0 solvers. Observes success bytes and raw outputs, including native rejection; not a projectile launch, flight or impact witness.',
        assumptions=[
            'Each solver runs in fresh shared call() state with verified PE bytes, a return sentinel and default live FPCW0E7F. No native OS/CRT/scenario/type startup executes.',
            'ECX supplies mode, EDX supplies speed; stack arguments supply range, signed height bits, binary64 gravity and the output pointer. Scratch begins with eightCD bytes so unchanged failure outputs remain visible.',
            'Original case and call order is preserved. Native Boolean failure is recorded normally; faults, exhausted budgets, incomplete returns or other exceptions abort generation and cannot become golden error fields.',
            'Negative/zero inputs characterize solver domain boundaries and are not claimed to arise from active retail weapon rules. No INI defaults or complete FireAt admission are supplied by this fixture.',
            'The Rust consumer checks angle status and successful raw angle bits. Word outputs remain checked native observations without a direct Rust consumer.',
            'No timer, RNG draw, detach, upstream targeting or downstream trajectory/lifecycle equivalence is claimed.',
        ],
        substitutions=['None; original solver bodies and reached code execute without behavioral hooks or instruction patches.'],
        entry_points=dict(angle=0x48A9D0, word=0x48A8D0),
    )


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata, argv=argv)


if __name__ == '__main__':
    main()
