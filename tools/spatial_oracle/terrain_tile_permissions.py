"""Original signed current-tile permission gates, not complete placement checks.

Supplied two-entry registry varies both permission bits. Execute original blocks
up to their next gate/return boundary without changing executable instructions.
"""
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EDI, UC_X86_REG_ESI

from tools.native_oracle import SCRATCH, load_image, run_checked, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords

CELL, TILES, HEAD0, HEAD1 = (SCRATCH + offset for offset in (0, 0x200, 0x400, 0x800))


def native_case(tile, tile0_permission):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(SCRATCH, 0x1000)
    for address, blob in {
        CELL + 0x38: dwords(tile),
        0xA8ED2C: dwords(TILES), 0xA8ED38: dwords(2),
        TILES: dwords(HEAD0, HEAD1),
        HEAD0 + 0x2E0: bytes((tile0_permission,)),
        HEAD0 + 0x306: bytes((tile0_permission,)),
        HEAD1 + 0x2E0: bytes((not tile0_permission,)),
        HEAD1 + 0x306: bytes((not tile0_permission,)),
    }.items():
        uc.mem_write(address, blob)
    uc.reg_write(UC_X86_REG_ESI, CELL)
    run_checked(uc, 0x6B601A, 0x6B603A, count=64, required_addresses=[0x6B6034])
    smudge = bool(uc.reg_read(UC_X86_REG_EAX) & 0xFF)
    uc.reg_write(UC_X86_REG_EDI, CELL)
    boundary = run_checked(uc, 0x4839C0, (0x4839E2, 0x4839E9), count=64)
    return dict(tile=tile, tile0_permission=tile0_permission,
                smudge=smudge, tiberium=boundary == 0x4839E2)


def cases():
    return {'cases': [native_case(tile, permission)
                      for permission in (False, True)
                      for tile in (-0x80000000, -1, 0, 1, 2, 0xFFFF, 0x10000, 0x7FFFFFFF)]}


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Sixteen synthetic original-block witnesses for current-tile Morphable and AllowTiberium queries',
        assumptions=[
            'Supplied two-entry registry with opposite permission bits; count2 and Cell+38 span signed and narrowing boundaries',
            'Smudge starts at6B601A with ESI=Cell and stops before TEST AL at6B603A',
            'Tiberium starts at4839C0 with EDI=Cell and stops before accepted4839E2 or rejected4839E9 return sequence',
            'Prior placement gates are assumed passed; later smudge gates and complete placement behavior are excluded',
            'Registry construction and stock reachability are outside these synthetic boundary witnesses',
        ], substitutions=[], entry_points={'smudge_tile_gate': 0x6B601A,
                                            'tiberium_tile_gate': 0x4839C0}))
