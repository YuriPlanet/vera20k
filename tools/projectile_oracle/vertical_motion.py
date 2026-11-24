"""Original Vertical ramp and integer candidate stores, no emulated leaves.

Run ``python -m tools.projectile_oracle.vertical_motion`` to compare without writing.
"""
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_FPCW
from tools.native_oracle import NATIVE_FPCW, finish_vectors, load_image, provenance, run_checked

BASE = 0x20000000
BULLET, PTYPE, SP = BASE, BASE + 0x2000, BASE + 0x8000

def run(velocity, acceleration, maximum, origin):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(BASE, 0x10000)
    for address, value in ((BULLET+0xAC, PTYPE), (BULLET+0x110, maximum), (PTYPE+0x2D0, acceleration)):
        u.mem_write(address, struct.pack('<i', value))
    u.mem_write(BULLET+0xE8, struct.pack('<ddd', *velocity))
    u.mem_write(SP+0x24, struct.pack('<iii', *origin))
    frames = []
    for frame in range(8):
        for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, BULLET), (UC_X86_REG_EBX, BULLET+0xE8), (UC_X86_REG_FPCW, NATIVE_FPCW)):
            u.reg_write(reg, value)
        run_checked(u, 0x4671E0, 0x467334, count=30000)
        frames.append(dict(bits=[f'{b:016x}' for b in struct.unpack('<QQQ', u.mem_read(BULLET+0xE8, 24))], candidate=list(struct.unpack('<iii', u.mem_read(SP+0x24, 12)))))
    return dict(velocity=velocity, input_bits=[f'{b:016x}' for b in struct.unpack('<QQQ', struct.pack('<ddd', *velocity))], acceleration=acceleration, maximum=maximum, origin=origin, frames=frames)

def generate() -> list[dict]:
    """Run independent cases, carrying only each case's successive frame state."""
    rows = [run(v, a, m, origin)
            for v in ((0.0, -0.0, 0.0), (1.0, 0.0, 0.0), (-2.449137817620151e-16, -0.0, -0.9999389052391052), (0.25, -0.75, -1.5), (3.25, -4.5, 6.75), (99.75, 0.125, -0.25))
            for a in (0, 1, 3)
            for m in (1, 10, 100)
            for origin in ((1280, -1280, 1000), (2147483646, -2147483647, -10))]
    rows.append(run((1.0, 0.0, 0.0), 10, 100, (640, 640, 5)))
    rows.append(run((0.0, 0.0, 1.0), 1, 50, (0, 0, 0)))
    return rows


def main() -> None:
    finish_vectors(
        generate,
        Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="Eight successive Vertical Bullet AI velocity/candidate blocks for 110 cases; full AI and collision excluded.",
            assumptions=[
                "Synthetic bullet, type, and stack state supply the selected block's inputs.",
                "Each frame sets x87 FPCW to 0x0E7F: 53-bit precision, round toward zero, all exceptions masked.",
                "Each case starts a fresh emulator; native output memory continues within that case.",
            ],
            substitutions=[],
            entry_points={"vertical_candidate_begin": 0x4671E0, "vertical_candidate_end": 0x467334},
        ),
    )


if __name__ == "__main__":
    main()
