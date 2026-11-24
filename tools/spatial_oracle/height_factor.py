"""Original g_HeightFactor startup chain.

Runs the CRT static initializers that seed the building height factor:
0x0045AFA0 (Sqrt_Approx(2 * pow(256, 2)) into 0x0089C8B8), 0x0045B030 (pi/3
into 0x0089C8C0), 0x0045B050 (pi/2 into 0x0089C898) and HeightFactor_Init
0x0045B070 (ftol(tan(pi/2 - pi/3) * 362.04 * 0.5) into 0x0089DDB8), under the
process control word 0x0E7F. BuildingTypeClass::Dimension2 0x00464AF0 multiplies
art Height= (+0xEF4) by this value; the Jumpjet cell top height 0x00485080 adds it.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_FPCW
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.walk_head_occupation import Original


def generate():
    native = Original()
    u = native.uc
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    for entry in (0x0045AFA0, 0x0045B030, 0x0045B050, 0x0045B070):
        native.call(entry, 0, [])
    read_double = lambda address: struct.unpack('<Q', bytes(u.mem_read(address, 8)))[0]
    return dict(
        sqrt_two_cells_bits=read_double(0x0089C8B8),
        pi_over_three_bits=read_double(0x0089C8C0),
        pi_over_two_bits=read_double(0x0089C898),
        height_factor=struct.unpack('<i', bytes(u.mem_read(0x0089DDB8, 4)))[0],
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Startup chain of g_HeightFactor (0x0089DDB8) used by BuildingTypeClass::Dimension2 0x00464AF0.',
        entry_points={'sqrt_two_cells': 0x45afa0, 'pi_over_three': 0x45b030, 'pi_over_two': 0x45b050,
                      'height_factor_init': 0x45b070},
        assumptions=['FPCW 0E7F (WinMain _controlfp(0x300, 0x300) at 0x006BBFC1); static tables from the image; '
                     'CRT pow 0x007C8FB0 runs natively.'],
        substitutions=['None: the four initializers run in startup order without supplied callbacks.'],
    ))
