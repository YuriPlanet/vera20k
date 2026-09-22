"""Original Foot4D9C10 entry seed across the eight active retail locomotors.

All eight real ILocomotion+1C slots point at the same constant-zero55ABF0.
The supplied interface only needs its vtable: this body reads no instance state.
"""
from pathlib import Path
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX
from tools.native_oracle import finish_vectors, provenance, SCRATCH, SCRATCH_SIZE
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.locomotor_at_coord import (
    OriginalQuery, fixture, FAMILIES, FALSE_VTABLES, LOCO, FOOT, OUTPUT,
)


def generate():
    r = OriginalQuery(fixture('drive', 'entry_seed'))
    u = r.uc
    tables = {name: data['vtable'] for name, data in FAMILIES.items()} | FALSE_VTABLES
    rows = []
    assert bytes(u.mem_read(0x55ABF0, 5)).hex() == '33c0c20800'
    for family, table in tables.items():
        entry = r.read32(table + 0x1C)
        assert entry == 0x55ABF0
        u.mem_write(LOCO, dwords(table))
        for coord in ((0, 0), (11, 10), (-32768, 32767)):
            for enabled in (False, True):
                u.mem_write(FOOT + 0x674, dwords(LOCO))
                u.mem_write(OUTPUT + 0x24, packed(*coord))
                before = bytes(u.mem_read(SCRATCH, SCRATCH_SIZE))
                u.reg_write(UC_X86_REG_ECX, FOOT)
                r.call(0x4D9C10, [OUTPUT, -1, -1, 0, enabled],
                       [0x4D9C10, entry] if enabled else [0x4D9C10])
                assert bytes(u.mem_read(SCRATCH, SCRATCH_SIZE)) == before
                rows.append(dict(family=family, vtable=f'0x{table:08X}',
                                 slot_1c=f'0x{entry:08X}', coord=coord, enabled=enabled,
                                 result=u.reg_read(UC_X86_REG_EAX)))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='48 complete Foot4D9C10 calls: eight active retail ILocomotion+1C slots, three packed Cells and the caller flag. Original55ABF0 is XOR EAX,EAX;RET8. Constant-zero seed proof for these interfaces; excludes dormant TS Mech/Tunnel and full class entry.',
        entry_points={'foot_entry': 0x4D9C10, 'shared_loco_entry': 0x55ABF0},
        assumptions=['Original immutable ILocomotion vtables, supplied Foot674 interface and Cell24 packed coordinate. No constructors needed: shared leaf reads no instance fields. Interface present; direction/height -1 and previous NULL are ignored by Foot4D9C10. Entire scratch memory unchanged.'],
        substitutions=[]))
