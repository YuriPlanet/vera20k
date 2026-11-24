"""Original DropPayload coordinate prefix, after a passenger was removed.

Runs 415C7D..415D78 with the real Aircraft GetCoords, FacingClass::Current,
retail sine/cosine and ftol. The stop is before Map::GetCell/admission. No
gameplay result or instruction is replaced; fixtures supply the object state.
"""
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EIP,
    UC_X86_REG_FPCW, UC_X86_REG_FPSW,
)

from tools.native_oracle import (
    NATIVE_FPCW, RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)

ENTRY, STOP = 0x415C7D, 0x415D78
OWNER, PASSENGER = SCRATCH, SCRATCH + 0x1000
SP = STACK_BASE + STACK_SIZE - 0x1000
ORIGINS = ((12928, 5248), (0, 0), (131071, 130816))
SAMPLE_HEADINGS = (
    0, 1, 0x7F, 0x80, 0xFF, 0x100, 0x1FFF, 0x2000, 0x2001,
    0x3FFE, 0x3FFF, 0x4000, 0x4001, 0x6000, 0x7FFF, 0x8000,
    0x8001, 0xA000, 0xBFFF, 0xC000, 0xC001, 0xE000, 0xFF7F,
    0xFF80, 0xFFFE, 0xFFFF,
)


def sweep(origin, post_count):
    machine = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(machine)
    machine.mem_map(SCRATCH, 0x2000)
    machine.mem_map(STACK_BASE, STACK_SIZE)
    machine.mem_map(RET_MAGIC, 0x1000)
    machine.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    assert struct.unpack('<I', machine.mem_read(0x822D80, 4))[0] == NATIVE_FPCW
    assert struct.unpack('<d', machine.mem_read(0x7E2808, 8))[0] == 128.0
    machine.mem_write(OWNER, struct.pack('<I', 0x7E22A4))
    machine.mem_write(OWNER + 0x9C, struct.pack('<iii', *origin, 1040))
    heading = 0
    digest = hashlib.sha256()
    samples = []

    def prepare():
        machine.mem_write(SP, bytes(0x80))
        machine.mem_write(OWNER + 0x2FC, struct.pack('<i', post_count + 1))
        # Current() returns the retained full word for zero turn rate.
        machine.mem_write(OWNER + 0x388, struct.pack('<6I', heading, heading, 0, 0, 0, 0))
        machine.reg_write(UC_X86_REG_EDI, OWNER)
        machine.reg_write(UC_X86_REG_ESI, PASSENGER)
        machine.reg_write(UC_X86_REG_ESP, SP)

    def completed(uc, _address, _size, _data):
        nonlocal heading
        assert uc.reg_read(UC_X86_REG_ESP) == SP - 8
        assert uc.reg_read(UC_X86_REG_FPCW) == NATIVE_FPCW
        assert (uc.reg_read(UC_X86_REG_FPSW) >> 11) & 7 == 0
        assert struct.unpack('<i', uc.mem_read(OWNER + 0x2FC, 4))[0] == post_count
        result = struct.unpack('<iii', uc.mem_read(SP + 0x20, 12))
        assert result[2] == 1040
        digest.update(struct.pack('<ii', *result[:2]))
        if heading in SAMPLE_HEADINGS:
            samples.append(dict(facing=heading, world_xy=list(result[:2])))
        heading += 1
        if heading == 65536:
            uc.reg_write(UC_X86_REG_EIP, RET_MAGIC)
        else:
            prepare()
            uc.reg_write(UC_X86_REG_EIP, ENTRY)

    machine.hook_add(UC_HOOK_CODE, completed, begin=STOP, end=STOP)
    prepare()
    run_checked(
        machine, ENTRY, RET_MAGIC, count=30_000_000, timeout_us=60_000_000,
        required_addresses=(0x415C8D, 0x415C93, 0x4C93D0, 0x4CACB0, 0x4CAD00, 0x7C5F00),
        context=dict(case='all-paradrop-headings', origin=origin, post_count=post_count),
    )
    assert heading == 65536
    return dict(origin=list(origin), post_count=post_count, facing_count=heading,
                world_xy_sha256=digest.hexdigest(), samples=samples)


def generate():
    return dict(sweeps=[sweep(origin, count) for origin in ORIGINS for count in (0, 1)])


if __name__ == '__main__':
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='393216 original DropPayload coordinate prefixes: every u16 heading, both payload parities, three supplied world origins. Hashes pack signed little-endian XY pairs in ascending heading order; selected pairs are also retained.',
            assumptions=[
                'Start after a passenger was removed; EDI is a supplied Aircraft using the original vtable, ESI is the removed passenger, ESP is the native post-prologue frame.',
                'Location XY is supplied, Z=1040; payload count is 1 or 2 before the original decrement; FacingClass has the selected full word and zero turn rate.',
                'Ambient x87 PC53/chop0E7F; original ftol cached control word and radius128 are checked. No INI inputs occur in this numeric prefix.',
                'Stops before Map::GetCell, passenger admission, subcell selection, Reveal and retry. No complete paradrop or arbitrary-i32 coordinate parity claim.',
            ],
            substitutions=['No code or callee substitutions. At the stop boundary, a hook records output and supplies the next independent object/CPU frame.'],
            entry_points=dict(prefix=ENTRY, before_map_lookup=STOP, facing_current=0x4C93D0,
                              sin=0x4CACB0, cos=0x4CAD00, ftol=0x7C5F00),
        ),
        source_paths={'generator': Path(__file__), 'runner': Path(__file__).parents[1] / 'native_oracle.py'},
    )
