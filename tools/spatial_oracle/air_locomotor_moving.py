"""Original Fly/Jumpjet IsMoving: retained request, pitch and independent phase.

Supplied states use the original interface slots. Queries must preserve both
objects. This does not execute movement, MoveTo or pitch/phase producers.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.locomotor_at_coord import OriginalQuery, fixture, LOCO, FOOT
from tools.spatial_oracle.map_queries import dwords

VTABLES = {'fly': 0x7E89F4, 'jumpjet': 0x7ECD68}
ENTRIES = {'fly': 0x4CCA90, 'jumpjet': 0x54AE50}


def inputs():
    for destination in ([0, 0, 0], [3200, 2688, 0]):
        for moving in (False, True):
            for pitch in (-1, 0, 1/65536, 1):
                yield dict(family='fly', destination=destination, moving=moving, pitch=pitch)
            for phase in range(7):
                yield dict(family='jumpjet', destination=destination, moving=moving, phase=phase)


def write_state(u, interface, owner, case):
    family = case['family']
    u.mem_write(interface, dwords(VTABLES[family]))
    u.mem_write(interface+8, dwords(owner))
    if family == 'fly':
        u.mem_write(interface+0x18, dwords(*case['destination']))
        u.mem_write(interface+0x30, bytes([case['moving']]))
        u.mem_write(owner+0x2E8, struct.pack('<f', case['pitch']))
    else:
        u.mem_write(interface+0x3C, dwords(*case['destination']))
        u.mem_write(interface+0x48, bytes([case['moving']]))
        u.mem_write(interface+0x4C, dwords(case['phase']))


def query(case):
    runner = OriginalQuery(fixture('drive', 'air_motion'))
    u = runner.uc
    write_state(u, LOCO, FOOT, case)
    before = bytes(u.mem_read(LOCO, 0x100)), bytes(u.mem_read(FOOT, 0x700))
    entry = runner.read32(runner.read32(LOCO)+0x10)
    assert entry == ENTRIES[case['family']]
    runner.call(entry, [LOCO], [entry])
    moving = bool(u.reg_read(UC_X86_REG_EAX) & 255)
    assert before == (bytes(u.mem_read(LOCO, 0x100)), bytes(u.mem_read(FOOT, 0x700)))
    return dict(input=case, moving=moving)


def generate():
    return [query(case) for case in inputs()]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Fly4CCA90 and Jumpjet54AE50 IsMoving over independent retained state',
        entry_points=ENTRIES, substitutions=[], assumptions=[
            'Original interface vtables and owner pointer; supplied request byte and destination XYZ.',
            'Fly pitch finite and Q16/f32-exact; Jumpjet phases0..6 supplied independently of its moving byte.',
            'No MoveTo, Process, pitch writers or scenario lifecycle. Owner/locomotor memory must be unchanged.']))
