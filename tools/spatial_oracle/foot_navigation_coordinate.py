"""Original Foot+4C wrapper over live Walk/Drive/Ship/Hover heads and Tube exit."""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import (
    STACK_BASE, STACK_SIZE, RET_MAGIC, SCRATCH, run_checked, finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.locomotor_at_coord import OriginalQuery, FAMILIES, LOCO, FOOT, OUTPUT


def query(row):
    fixture = OriginalQuery(row)
    u = fixture.uc
    # Original concrete owner table, including +48 Object-coordinate dispatch.
    u.mem_write(FOOT, dwords(0x7EB058 if row['family'] == 'walk' else 0x7F5C70))
    u.mem_write(FOOT + 0x674, dwords(LOCO))
    u.mem_write(0x8B3DA8, dwords(0, 0, 0))
    tube = row.get('tube')
    if tube is None:
        u.mem_write(FOOT + 0x684, b'\xff')
    else:
        table, descriptor = SCRATCH + 0x7000, SCRATCH + 0x7100
        u.mem_write(FOOT + 0x684, b'\x00')
        u.mem_write(0x8B413C, dwords(table))
        u.mem_write(table, dwords(descriptor))
        u.mem_write(descriptor + 0x28, packed(*tube))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC, OUTPUT, 0))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, FOOT)
    required = [0x4DBDF0, 0x4DBE01] if tube is not None else [0x4DBDF0, FAMILIES[row['family']]['head']]
    run_checked(u, 0x4DBDF0, RET_MAGIC, count=5000, required_addresses=required)
    assert u.reg_read(UC_X86_REG_ESP) == sp + 12
    assert u.reg_read(UC_X86_REG_EAX) == OUTPUT
    return dict(input=row, coordinate=fixture.coord(OUTPUT),
                retained_head=fixture.coord(LOCO + FAMILIES[row['family']]['head_offset']),
                current=fixture.coord(FOOT + 0x9C))


def generate():
    rows = []
    for family in FAMILIES:
        base = dict(family=family, current=[1408, 1408, 416],
                    stored_head=[1664, 1408, 17], turn_index=-1, cursor=0)
        variants = [dict(name='retained_without_selector'),
                    dict(name='null_head_current', stored_head=[0, 0, 0]),
                    dict(name='both_null', stored_head=[0, 0, 0], current=[0, 0, 0]),
                    dict(name='signed_head', stored_head=[-257, -1, -2147483648]),
                    dict(name='selector_independent', turn_index=1, cursor=-1)]
        variants += [dict(name=f'partial_null_{axis}', stored_head=[int(i == axis) for i in range(3)]) for axis in range(3)]
        variants += [dict(name=f'tube_{x}_{y}', tube=[x, y])
                     for x, y in [(10, 20), (-1, -32768), (32767, 0)]]
        rows += [query(base | variant) for variant in variants]
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Foot4DBDF0 navigation coordinate wrapper, including Tube override and original active Walk/Drive/Ship/Hover ILocomotion+18 head queries. Supplied retained state only; no producers/FindPath/target callbacks.',
        entry_points={'foot_coordinate': 0x4DBDF0, 'tube_override': 0x4DBE01,
                      **{family + '_head': state['head'] for family, state in FAMILIES.items()}},
        assumptions=['Original concrete Infantry table for Walk and Unit table for the other three active families. Original ILocomotion tables and supplied link pointer; no callable replacement.',
                     'Native null-coordinate globals are supplied zero. Retained XYZ and physical XYZ are independent and observed after each call. Track selectors and cursors cannot gate HeadTo dispatch.',
                     'Foot+684=-1 without a Tube; explicit Tube rows supply index0 and a one-entry Tube table with signed exit CellStruct. Wrapper must bypass the locomotor query and return signed exit center with Z0.',
                     'Inherited OriginalQuery maps verified unmodified retail bytes and observes existing helper addresses; its separate IsAtCoord and track transform methods are not invoked.'],
        substitutions=[]))
