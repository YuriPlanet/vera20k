"""Original RNG support for the Rust nested bridge continuation regression.

This executes consecutive scalar draws, not a native nested receiver chain.
"""
import struct
from pathlib import Path

from tools.spatial_oracle.bridge_damage_admission import (
    base, call, seed_bytes, SCENARIO, signed,
)
from tools.native_oracle import finish_vectors, provenance


def generate():
    native = base({})
    native.mem_write(SCENARIO + 0x218, seed_bytes(1))
    draws = [signed(call(native, 0x65C7E0, SCENARIO + 0x218, (1, 65536)))
             for _ in range(2)]
    state = bytes(native.mem_read(SCENARIO + 0x218, 0x3F4))
    continuation = [call(native, 0x65C780, SCENARIO + 0x218) for _ in range(4)]
    return dict(seed=1, range=[1, 65536], draws=draws,
                rng_indices=list(struct.unpack_from('<2I', state, 4)),
                next_rng=continuation)


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Two consecutive original Scenario RandomRanged(1,65536) draws and four original raw continuations from native seed1. This is scalar RNG support, not nested receiver execution.',
        assumptions=['Native65C6D0-seeded Scenario Random state reused from seed_bytes.'],
        substitutions=[],
        entry_points={'range': 0x65C7E0, 'raw': 0x65C780, 'seed': 0x65C6D0},
    ))
