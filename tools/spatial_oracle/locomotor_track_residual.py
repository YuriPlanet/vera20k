"""Original Drive/Ship residual scalar helper and Cell-identity selection gate.

Separate interior witnesses, with supplied current/delta vectors and Cell
identities. This does not emulate Cell lookup, movement callbacks, or the whole
Process_Track call. All instructions and nested math helpers are original.
"""
from pathlib import Path

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EDI, UC_X86_REG_ESI

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.locomotor_track_cursor import OriginalCursor, LOCO, FOOT, SP


RANGES = {
    'drive': dict(scalar=(0x4B23B2, 0x4B23EE), delta=0xC0,
                  gate=(0x4B2452, 0x4B24AD), full=0x6C),
    'ship': dict(scalar=(0x6A19FA, 0x6A1A36), delta=0xAC,
                 gate=(0x6A1AA3, 0x6A1AFE), full=0x74),
}


class OriginalResidual(OriginalCursor):
    def scalar(self, delta, residual):
        self.seed(0, 0, False, 0)
        r, u = RANGES[self.name], self.uc
        u.mem_write(LOCO + 0x4C, dwords(residual))
        u.mem_write(SP + r['delta'], dwords(*delta))
        u.reg_write(UC_X86_REG_EAX, FOOT + 0x400)
        u.reg_write(UC_X86_REG_EDI, 0)
        self.run(*r['scalar'], required=(0x75F540, 0x7C5F00))
        return dict(factor_bits=self.ints(SP - 4)[0] & 0xFFFFFFFF,
                    delta=self.ints(u.reg_read(UC_X86_REG_EAX), 3))

    def gate(self, current, interpolated, full, cells, residual):
        self.seed(0, 0, False, 0)
        r, u = RANGES[self.name], self.uc
        u.mem_write(LOCO + 0x4C, dwords(residual))
        u.mem_write(FOOT + 0x9C, dwords(*current))
        u.mem_write(SP + 0x24, dwords(*interpolated))
        u.mem_write(SP + r['full'], dwords(*full))
        # These are supplied pointer identities, not emulated map lookups.
        u.reg_write(UC_X86_REG_EAX, FOOT + 0x400 + cells[0] * 0x100)
        u.reg_write(UC_X86_REG_ESI, FOOT + 0x400 + cells[1] * 0x100)
        u.reg_write(UC_X86_REG_EDI, FOOT + 0x400 + cells[2] * 0x100)
        self.run(*r['gate'])
        return dict(chosen=self.ints(SP + 0x18, 3))


def generate():
    result = {}
    deltas = ([7, -7, 0], [11, -11, 0], [-8, 8, 0], [0, 0, 0],
              [-670, -68, 0], [-2147483648, 2147483647, 0])
    for family in RANGES:
        original = OriginalResidual(family)
        scalar, gates = [], []
        for delta in deltas:
            for residual in range(1, 8):
                inputs = dict(delta=list(delta), residual=residual)
                scalar.append(dict(input=inputs, output=original.scalar(**inputs)))
        for cells in ([0, 1, 0], [0, 0, 1], [0, 1, 2], [0, 0, 0]):
            for residual in (1, 3, 4, 7):
                inputs = dict(current=[9987, -19, 731], interpolated=[9989, -21, 731],
                              full=[9994, -26, 731], cells=list(cells), residual=residual)
                gates.append(dict(input=inputs, output=original.gate(**inputs)))
        result[family] = dict(scalar=scalar, gates=gates)
    return result


def metadata():
    return provenance(
        scope='Original Drive/Ship residual f32 scaling and three-way Cell-identity coordinate selection',
        assumptions=[
            'Supplied residual/delta/current/full/interpolated vectors and Cell pointer identities',
            'Original nested math helpers75F540/7C5F00 run with supplied FPCW0xE7F and ftol control word0xE7F, consistent with saved startup capture',
            'Scalar cases include sign, zero, one-seventh boundaries and extreme signed component inputs',
            'Gate cases compare pointer identity, including all-equal, without performing Cell lookup',
        ],
        substitutions=[
            'Cell lookups, dummy identity production, paid-point callbacks and whole Process_Track execution are outside these interior witnesses',
            'No original instructions or nested math call results are patched; Rust production integration is not tested here',
        ],
        entry_points={f'{family}_{operation}': limits[operation][0]
                      for family, limits in RANGES.items() for operation in ('scalar', 'gate')},
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
