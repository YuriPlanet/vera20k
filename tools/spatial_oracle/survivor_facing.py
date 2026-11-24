"""Original BuildingClass survivor direction: `+0x388` Current() as a DirType.

SpawnSurvivors reads the dying building's body FacingClass and passes it to
the survivor's Unlimbo (`0x00442F3C..0x00442F63`): FacingClass::Current
(`0x004C93D0`) into a stack DirStruct, a 32-bit load of that slot, then
`((raw >> 7) + 1) >> 1 & 0xFF`. Each row builds the facing with the original
constructor, Set_ROT, Set_Current and Set (the facing_class corpus's calls),
fills the stack slot's upper word with garbage, executes the block and stops
before the Unlimbo call, where the pushed direction is read.

Run python -m tools.spatial_oracle.survivor_facing (--check by default).
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDI,
                               UC_X86_REG_ESI, UC_X86_REG_ESP)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)

BUILDING, SURVIVOR, VTABLE, ARG = SCRATCH, SCRATCH + 0x1000, SCRATCH + 0x1100, SCRATCH + 0x1400
FACING = BUILDING + 0x388
SP = STACK_BASE + STACK_SIZE - 0x1000
FRAME = 0xA8ED84
BLOCK, UNLIMBO_CALL = 0x442F3C, 0x442F63


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def execute(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)

    def call(entry, *args):
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, FACING)
        run_checked(u, entry, RET_MAGIC, count=10000)
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4 + 4 * len(args)

    u.mem_write(FRAME, dwords(case['start']))
    call(0x4C91C0)
    call(0x4C9680, case['rot'])
    u.mem_write(ARG, dwords(case['initial']))
    call(0x4C9300, ARG)
    if case['target'] is not None:
        u.mem_write(FRAME, dwords(case['set_frame']))
        u.mem_write(ARG, dwords(case['target']))
        call(0x4C9220, ARG)
    u.mem_write(FRAME, dwords(case['sample_frame']))

    u.mem_write(SURVIVOR, dwords(VTABLE))
    u.mem_write(SP + 0x28, dwords(case['slot_garbage']))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EDI, BUILDING)
    u.reg_write(UC_X86_REG_ESI, SURVIVOR)
    run_checked(u, BLOCK, UNLIMBO_CALL, count=10000)
    esp = u.reg_read(UC_X86_REG_ESP)
    coord, direction = struct.unpack('<2I', u.mem_read(esp, 8))
    assert coord == SP + 0x38 and esp == SP - 8
    current = struct.unpack('<H', u.mem_read(SP + 0x28, 2))[0]
    return dict(input=case, current=current, direction=direction)


def generate():
    rows = []

    def case(rot, initial, target=None, set_frame=100, sample_frame=100, garbage=0xA5A50000):
        rows.append(execute(dict(rot=rot, start=90, initial=initial, target=target,
                                 set_frame=set_frame, sample_frame=sample_frame,
                                 slot_garbage=garbage)))

    for word in (0x0000, 0x0001, 0x007F, 0x0080, 0x00FF, 0x0100, 0x017F, 0x0180,
                 0x3F7F, 0x3F80, 0x4000, 0x7F7F, 0x7F80, 0x8000, 0xBF80, 0xC07F,
                 0xFE80, 0xFF7F, 0xFF80, 0xFFFF):
        case(5, word)
    case(5, 0xFF80, garbage=0xFFFF0000)
    case(5, 0x7F80, garbage=0x00000000)
    # A building turret mid-turn at ROT=5 (0x4000 -> 0xC000, 25 frames).
    for frame in (100, 101, 103, 107, 112, 124, 125, 126):
        case(5, 0x4000, 0xC000, sample_frame=frame)
    for rot, target in ((1, 0x2345), (127, 0x8123), (-1, 0x8123), (0, 0x0180)):
        for frame in (100, 101, 102):
            case(rot, 0x0000, target, sample_frame=frame)
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'constructor': 0x4C91C0, 'set_rot': 0x4C9680, 'snap': 0x4C9300,
                      'set': 0x4C9220, 'current': 0x4C93D0, 'survivor_direction': BLOCK},
        assumptions=['EDI is the building; its FacingClass at +0x388 is built by the original constructor, Set_ROT, Set_Current and Set at supplied global frames.',
                     'ESI is a survivor whose vtable pointer is read but not followed; execution stops at the Unlimbo call 0x00442F63 and reads the pushed direction.',
                     'The stack DirStruct slot starts with a supplied upper-word value, which the 32-bit load at 0x00442F4C reads.'],
        substitutions=[],
        scope='42 rows: settled headings across byte and rounding boundaries (including the 0xFF80 wrap), stack-slot garbage, a ROT=5 turn sampled over its arc, and ROT 1, 127, -1 and 0. Covers only the direction SpawnSurvivors passes; not the survivor spawn, cell search or RNG.',
    ))
