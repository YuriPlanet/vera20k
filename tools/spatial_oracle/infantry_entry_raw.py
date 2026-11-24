"""Original Infantry CRT height threshold and Unlimbo raw-entry boundary.

Executes the original ten-entry initializer chain, current Map ground-height
query, caller51E0F6..51E114, and complete Infantry5217C0 raw writer. The earlier
Unlimbo adjustment/placement corridor supplies the declared incoming XYZ.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDI, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import RET_MAGIC, STACK_BASE, STACK_SIZE, finish_vectors, provenance, run_checked
from tools.spatial_oracle.jumpjet_entry_discovery import Entry
from tools.spatial_oracle.walk_head_occupation import CURRENT, MAP, OUTPUT, OWNER
from tools.spatial_oracle.map_queries import dwords

STARTUP = [0x517840, 0x517870, 0x517890, 0x5178B0, 0x5178D0,
           0x5178F0, 0x517910, 0x517950, 0x517980, 0x5179B0]


def case(row):
    fixture = Entry(row)
    u = fixture.uc
    assert list(struct.unpack('<10I', u.mem_read(0x813490, 40))) == STARTUP
    assert u.reg_read(UC_X86_REG_FPCW) == 0xE7F
    fixture.stage = 'infantry_startup'
    for entry in STARTUP:
        fixture.call(entry, 0, [])
    visits = [address for stage, address in fixture.visits
              if stage == 'infantry_startup' and address in STARTUP]
    assert visits == STARTUP
    threshold = fixture.read32(0xA8F234)
    coord = [2496, 2624, row['level'] * 104 + row['height']]
    u.mem_write(OUTPUT, dwords(*coord))
    u.mem_write(OWNER + 0x9C, dwords(*coord))
    u.mem_write(CURRENT + 0x124, dwords(0, 0))
    u.mem_write(CURRENT + 0x54, dwords(0xFFFFFFFF, 0xFFFFFFFF))
    # Original earlier Unlimbo ground query578080 supplies EBX. This fixture
    # supplies the coordinate after the preceding placement/height adjustments.
    fixture.stage = 'ground'
    fixture.call(0x578080, MAP, [OUTPUT])
    ground = u.reg_read(UC_X86_REG_EAX)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC, 0, 0, *coord, *([0] * 10)))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_EBX, ground)
    u.reg_write(UC_X86_REG_EDI, OWNER)
    fixture.stage = 'raw_entry'
    run_checked(u, 0x51E0F6, 0x51E114, count=30000,
                required_addresses=[0x51E0F6, 0x51E103])
    raw_visits = {address for stage, address in fixture.visits if stage == 'raw_entry'}
    writes = 0x5217C0 in raw_visits
    assert writes == (coord[2] <= ground + threshold)
    if writes:
        assert {0x4810A0, 0x565730, 0x578080} <= raw_visits
    return dict(input=row, output=dict(
        fpcw=hex(u.reg_read(UC_X86_REG_FPCW)), threshold=threshold,
        base_height=fixture.read32(0xA8F240), ground=ground,
        raw_callback=writes, raw=[fixture.read32(CURRENT + 0x124), fixture.read32(CURRENT + 0x128)],
        owners=[fixture.read32(CURRENT + 0x54), fixture.read32(CURRENT + 0x58)],
    ))


def generate():
    return [case(dict(level=level, bridge=bridge, height=height))
            for level in [0, 2] for bridge in [False, True] for height in [415, 416, 417]]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original ten-entry Infantry CRT and height-gated raw Unlimbo entry; no whole Unlimbo, initial Jumpjet Process, or generic map claim.',
        entry_points={'startup_first': STARTUP[0], 'startup_last': STARTUP[-1],
                      'ground': 0x578080, 'entry_gate': 0x51E0F6, 'raw_put': 0x5217C0},
        assumptions=[
            'Actual table813490 contains the ten called entries in startup order. Original rodata/math execute under captured startup FPCW0E7F. A8F240/A8F234 are initialized, not supplied.',
            'Entry fixture supplies original Infantry vtable7EB058 and two real Cells with blank TMP/overlay metadata. Ground levels0/2 and structural flag100 are declared. Generic Map ground/rectangle/projection domains remain outside this comparison.',
            'Incoming XYZ after prior Unlimbo adjustment and successful placement is supplied. Original578080 produces the ground value put in caller EBX; exact stack+C XYZ and stack+14 Z feed51E0F6..114. Whole preceding Unlimbo is not executed.',
            'Raw bitmap0/0 and ownerFFFFFFFF/FFFFFFFF are declared empty prestates. Original5217C0 computes the subcell bit, selected ground/deck plane and owner-index stores; no raw writer substitution.',
        ],
        substitutions=[
            'Inherits the Entry fixture setup and hook set. Its footprint/shroud/FNPC/selected-index substitutions are not visited by the measured CRT/ground/raw-entry corridor. Original raw writer, map lookup, ground and owner query execute.',
        ],
    ))
