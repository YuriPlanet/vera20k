"""Destination missions through original Event construction and plan publication.

The existing keyboard fixture owns the VM and constructor allocator transport.
This companion to planning_mode observes the record before enqueue; it does
not supply a mission/target filter or emulate the PlanMgr's node admission.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools import native_oracle as native
from tools.storage_oracle.keyboard_bindings import KeyboardFixture


def generate():
    rows = []
    cases = [
        (1, (202, 0x34), (0, 0)), (2, (0, 0), (6005, 11)),
        (7, (0, 0), (202, 0x34)), (8, (0, 0), (202, 0x34)),
        (9, (0, 0), (202, 0x34)), (10, (0, 0), (6005, 11)),
        (11, (202, 0x34), (0, 0)), (16, (0, 0), (0, 0)),
        (16, (101, 0x34), (0, 0)),
        (17, (0, 0), (202, 0x34)), (25, (0, 0), (6005, 11)),
        (29, (0, 0), (6005, 11)),
    ]
    for mission, target, destination in cases:
        fixture = KeyboardFixture()
        u = fixture.uc
        u.mem_map(native.RET_MAGIC, 0x1000)
        event = native.SCRATCH + 0xF000
        u.mem_write(event, b'\xA5' * 111)
        fixture.put(0xA8ED84, 55)
        args = (native.RET_MAGIC, 3, 101, 0x34, mission, *target, *destination, 0, 0)
        u.mem_write(fixture.stack, struct.pack('<11I', *args))
        u.reg_write(UC_X86_REG_ESP, fixture.stack)
        u.reg_write(UC_X86_REG_ECX, event)
        native.run_checked(u, 0x4C6860, native.RET_MAGIC,
                           required_addresses=[0x4C6860, 0x4C6877, 0x4C68C7])
        constructed = bytes(u.mem_read(event, 111))
        u.mem_write(fixture.stack, struct.pack('<I', native.RET_MAGIC) + constructed + b'\0')
        u.reg_write(UC_X86_REG_ESP, fixture.stack)
        native.run_checked(u, 0x637DD0, 0x6521C0,
                           required_addresses=[0x637DE8, 0x637DF5])
        planned = bytes(u.mem_read(u.reg_read(UC_X86_REG_ESP) + 4, 111))
        if not (planned[0x1D] == 1 and constructed[0x1D] == 0
                and constructed[:0x1D] + constructed[0x1E:]
                == planned[:0x1D] + planned[0x1E:]):
            raise AssertionError('Original planning wrapper changed fields other than its flag')
        rows.append(dict(mission=mission, target=list(target), destination=list(destination),
                         constructed=constructed.hex(), planned=planned.hex()))
    return rows


def metadata():
    return native.provenance(
        scope='Twelve original Event4C6860 constructors and Plan publish637DD0 wrappers.',
        assumptions=['Numeric House3, UID101/202 and Cell(5,6) tokens are supplied inputs. '
                     'Original record padding begins A5; the ignored NULL auxiliary value is0. '
                     'KeyboardFixture initializes its ordinary registry with its standard allocator transport.'],
        substitutions=['Stops on entry to6521C0 before queue admission. No click, object identity '
                       'resolution, node append/commit, actor AI or gameplay execution is certified. '
                       'The constructor and planning-copy/flag instructions execute unchanged.'],
        entry_points={'constructor': 0x4C6860, 'plan_publish': 0x637DD0,
                      'enqueue_boundary': 0x6521C0})


if __name__ == '__main__':
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
