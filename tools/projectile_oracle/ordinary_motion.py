"""Original live-gravity and ordinary candidate producer; collision supplied.

Run ``python -m tools.projectile_oracle.ordinary_motion`` to compare without writing.
"""
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_FPCW
from tools.native_oracle import NATIVE_FPCW, finish_vectors, load_image, provenance, run_checked

BASE = 0x20000000
BULLET, PTYPE, RULES, SP = BASE, BASE+0x2000, BASE+0x4000, BASE+0xA000

def run(velocity, gravity_sequence, floater, origin):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(BASE, 0x10000)
    for address, value in ((BULLET+0xAC, PTYPE), (0x8871E0, RULES)):
        u.mem_write(address, struct.pack('<i', value))
    u.mem_write(PTYPE+0x295, bytes([floater]))
    u.mem_write(BULLET+0xE8, struct.pack('<ddd', *velocity))
    u.mem_write(SP+0x24, struct.pack('<iii', *origin))
    frames = []
    for gravity in gravity_sequence:
        u.mem_write(RULES+0x16B8, struct.pack('<i', gravity))
        u.mem_write(SP+0x90, bytes(u.mem_read(BULLET+0xE8, 24)))
        for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, BULLET), (UC_X86_REG_FPCW, NATIVE_FPCW)):
            u.reg_write(reg, value)
        run_checked(u, 0x46718F, 0x467494, count=30000)
        raw = bytes(u.mem_read(SP+0x90, 24))
        candidate = bytes(u.mem_read(SP+0x44, 12))
        frames.append(dict(bits=[f'{b:016x}' for b in struct.unpack('<QQQ', raw)], candidate=list(struct.unpack('<iii', candidate)), candidate_bits=[f'{b:016x}' for b in struct.unpack('<QQQ', u.mem_read(SP+0x68, 24))]))
        # Supply only the admitted fallthrough commit between visits. Native
        # collision/early-tail/reflection ownership has a separate oracle.
        u.mem_write(BULLET+0xE8, raw)
        u.mem_write(SP+0x24, candidate)
    return dict(velocity=velocity, input_bits=[f'{b:016x}' for b in struct.unpack('<QQQ', struct.pack('<ddd', *velocity))], gravity_sequence=gravity_sequence, floater=floater, origin=origin, frames=frames)

def generate() -> list[dict]:
    """Run independent cases, carrying only each case's successive frame state."""
    return [run(v, g, floater, origin)
            for v in ((0.0, -0.0, 0.0), (98.82575273513794, -0.0, 15.279717743396759), (0.25, -0.75, -1.5), (3.25, -4.5, 6.75))
            for g in ((6,)*8, (5,)*8, (0,)*8, (-1,)*8, (6, 3, 1, 0, -1, 2, 5, 6))
            for floater in (False, True)
            for origin in ((1280, -1280, 1000), (2147483646, -2147483647, -10), (3200, 3200, 100))]


def main() -> None:
    finish_vectors(
        generate,
        Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="Eight successive ordinary Bullet AI gravity/candidate blocks for 120 cases; collision and full AI excluded.",
            assumptions=[
                "Synthetic bullet, type, rules, and stack state supply the selected block's inputs.",
                "Each frame sets x87 FPCW to 0x0E7F: 53-bit precision, round toward zero, all exceptions masked.",
                "Each case starts a fresh emulator; native velocity and candidate bytes continue within that case.",
            ],
            substitutions=[
                "Between block visits the script supplies an admitted fallthrough velocity/position commit; no collision is executed."
            ],
            entry_points={"ordinary_candidate_begin": 0x46718F, "ordinary_candidate_end": 0x467494},
        ),
    )


if __name__ == "__main__":
    main()
